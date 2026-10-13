//! A matrix of normalised vectors in one memory-mapped file, searched by dot product.

use lxr_core::store::write_atomic;
use memmap2::Mmap;
use rayon::prelude::*;
use std::fs::File;
use std::io;
use std::path::Path;

const MAGIC: &[u8; 8] = b"LXRVEC01";
const VERSION: u32 = 1;
/// Magic (8), version (4), dimension (4) and row count (8).
const HEADER: usize = 24;
/// Rows scored by one parallel task.
const CHUNK_ROWS: usize = 2048;
/// Below this many numbers a search runs on one thread.
const PARALLEL_FROM: usize = 1 << 16;

/// A row-major `f32` matrix backed by a memory-mapped file. Rows are expected to be
/// stored normalised, so that the dot product with a normalised query is the cosine.
#[derive(Debug)]
pub struct Vectors {
    map: Mmap,
    dim: usize,
    count: usize,
}

impl Vectors {
    /// Writes `rows` to `path`, each of length `dim`, replacing the file in one step.
    /// On Windows a file that is currently mapped cannot be replaced: drop its
    /// [`Vectors`] first.
    pub fn write(path: &Path, dim: usize, rows: &[Vec<f32>]) -> io::Result<()> {
        let invalid = |message: &str| io::Error::new(io::ErrorKind::InvalidInput, message.to_string());
        if dim == 0 || dim > u32::MAX as usize {
            return Err(invalid("the vector dimension must be between 1 and 2^32 - 1"));
        }
        if rows.len() > u32::MAX as usize {
            return Err(invalid("too many rows: row numbers are 32-bit"));
        }
        if rows.iter().any(|row| row.len() != dim) {
            return Err(invalid("a row does not have the stated dimension"));
        }
        // `write_atomic` gives every write a temporary file of its own, so two writes to one
        // path never share bytes. The file is built in memory first, which costs one extra
        // copy of the matrix while the write runs.
        let mut bytes = Vec::with_capacity(HEADER + rows.len() * dim * 4);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&(dim as u32).to_le_bytes());
        bytes.extend_from_slice(&(rows.len() as u64).to_le_bytes());
        for row in rows {
            for value in row {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        write_atomic(path, &bytes)
    }

    /// Maps the file at `path`. Fails for a file that is not a vector file of this
    /// version or whose size does not match its header.
    pub fn open(path: &Path) -> io::Result<Vectors> {
        let bad = |message: String| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {message}", path.display()));
        let file = File::open(path)?;
        if file.metadata()?.len() < HEADER as u64 {
            return Err(bad("too short to be a vector file".to_string()));
        }
        // SAFETY: the file is only read through the map, and vector files are replaced
        // by renaming a new file over them rather than written in place.
        let map = unsafe { Mmap::map(&file)? };
        if &map[..8] != MAGIC {
            return Err(bad("not a vector file (wrong magic)".to_string()));
        }
        let version = u32::from_le_bytes([map[8], map[9], map[10], map[11]]);
        if version != VERSION {
            return Err(bad(format!("vector file version {version} is not supported")));
        }
        let dim = u32::from_le_bytes([map[12], map[13], map[14], map[15]]) as usize;
        let count = u64::from_le_bytes([map[16], map[17], map[18], map[19], map[20], map[21], map[22], map[23]]);
        let expected = (count as u128)
            .checked_mul(dim as u128)
            .and_then(|n| n.checked_mul(4))
            .and_then(|n| n.checked_add(HEADER as u128));
        if dim == 0 || expected != Some(map.len() as u128) {
            return Err(bad("the size of the file does not match its header".to_string()));
        }
        // SAFETY: every bit pattern is a valid `f32`; `align_to` only hands out the part of
        // the bytes that is properly aligned, and the check below insists it is all of them.
        let (before, _, after) = unsafe { map[HEADER..].align_to::<f32>() };
        if !before.is_empty() || !after.is_empty() {
            return Err(bad("the vector data is not aligned for f32".to_string()));
        }
        Ok(Vectors { map, dim, count: count as usize })
    }

    /// Number of rows.
    pub fn len(&self) -> usize {
        self.count
    }

    /// True when there are no rows.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Length of each row.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Row `i`; empty when there is no such row.
    pub fn row(&self, i: u32) -> &[f32] {
        let i = i as usize;
        if i >= self.count {
            return &[];
        }
        &self.data()[i * self.dim..(i + 1) * self.dim]
    }

    /// The best `limit` rows for `query` by dot product, as `(row, score)`, highest first,
    /// equal scores with the lower row first. A query of the wrong length finds nothing.
    pub fn nearest(&self, query: &[f32], limit: usize) -> Vec<(u32, f32)> {
        if query.len() != self.dim || limit == 0 || self.count == 0 {
            return Vec::new();
        }
        let data = self.data();
        let score_chunk = |chunk: usize| -> Vec<(u32, f32)> {
            let from = chunk * CHUNK_ROWS;
            let to = (from + CHUNK_ROWS).min(self.count);
            let hits = (from..to)
                .filter_map(|row| {
                    let score = dot(query, &data[row * self.dim..(row + 1) * self.dim]);
                    (!score.is_nan()).then_some((row as u32, score))
                })
                .collect();
            top(hits, limit)
        };
        let chunks = self.count.div_ceil(CHUNK_ROWS);
        if self.count * self.dim < PARALLEL_FROM {
            return top((0..chunks).flat_map(score_chunk).collect(), limit);
        }
        (0..chunks).into_par_iter().map(score_chunk).reduce(Vec::new, |mut all, part| {
            all.extend(part);
            top(all, limit)
        })
    }

    /// Like [`Vectors::nearest`], but only the listed rows are considered. Repeated and
    /// out-of-range row numbers are ignored.
    pub fn nearest_among(&self, query: &[f32], rows: &[u32], limit: usize) -> Vec<(u32, f32)> {
        if query.len() != self.dim || limit == 0 || self.count == 0 {
            return Vec::new();
        }
        let mut wanted: Vec<u32> = rows.iter().copied().filter(|&r| (r as usize) < self.count).collect();
        wanted.sort_unstable();
        wanted.dedup();
        let data = self.data();
        let score = |row: u32| -> Option<(u32, f32)> {
            let at = row as usize * self.dim;
            let score = dot(query, &data[at..at + self.dim]);
            (!score.is_nan()).then_some((row, score))
        };
        let hits: Vec<(u32, f32)> = if wanted.len() * self.dim < PARALLEL_FROM {
            wanted.iter().copied().filter_map(score).collect()
        } else {
            wanted.par_iter().copied().filter_map(score).collect()
        };
        top(hits, limit)
    }

    /// The numbers of all rows, one after the other.
    fn data(&self) -> &[f32] {
        // SAFETY: as in `open`: every bit pattern is a valid `f32`, and `open` checked that
        // the whole region is aligned, so the middle part is the data and nothing is left over.
        let (_, data, _) = unsafe { self.map[HEADER..].align_to::<f32>() };
        data
    }
}

/// The dot product of two slices of equal length.
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut lanes = [0f32; 8];
    let (a_chunks, b_chunks) = (a.chunks_exact(8), b.chunks_exact(8));
    let (a_rest, b_rest) = (a_chunks.remainder(), b_chunks.remainder());
    for (x, y) in a_chunks.zip(b_chunks) {
        for i in 0..8 {
            lanes[i] += x[i] * y[i];
        }
    }
    let mut sum = lanes.iter().sum::<f32>();
    for (x, y) in a_rest.iter().zip(b_rest) {
        sum += x * y;
    }
    sum
}

/// The best `limit` hits: highest score first, lower row first among equals.
fn top(mut hits: Vec<(u32, f32)>, limit: usize) -> Vec<(u32, f32)> {
    let order = |a: &(u32, f32), b: &(u32, f32)| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0));
    if hits.len() > limit {
        hits.select_nth_unstable_by(limit - 1, order);
        hits.truncate(limit);
    }
    hits.sort_unstable_by(order);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lxr-text-vectors-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn unit(values: &[f32]) -> Vec<f32> {
        let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
        values.iter().map(|v| v / norm).collect()
    }

    fn sample(dir: &Path) -> Vectors {
        let rows = vec![unit(&[1.0, 0.0, 0.0]), unit(&[0.0, 1.0, 0.0]), unit(&[1.0, 1.0, 0.0]), unit(&[1.0, 0.0, 0.0])];
        let path = dir.join("v.bin");
        Vectors::write(&path, 3, &rows).unwrap();
        Vectors::open(&path).unwrap()
    }

    #[test]
    fn write_then_open_gives_the_rows_back() {
        let dir = temp("roundtrip");
        let v = sample(&dir);
        assert_eq!((v.len(), v.dim(), v.is_empty()), (4, 3, false));
        assert_eq!(v.row(1), &[0.0, 1.0, 0.0]);
        assert_eq!(v.row(3), v.row(0));
        assert!(v.row(4).is_empty());
        assert!(v.row(u32::MAX).is_empty());
        drop(v);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nearest_orders_by_score_then_row() {
        let dir = temp("nearest");
        let v = sample(&dir);
        let hits = v.nearest(&unit(&[1.0, 0.0, 0.0]), 10);
        assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![0, 3, 2, 1]);
        assert!((hits[0].1 - 1.0).abs() < 1e-6 && hits[3].1.abs() < 1e-6);
        let two = v.nearest(&unit(&[1.0, 0.0, 0.0]), 2);
        assert_eq!(two.iter().map(|h| h.0).collect::<Vec<_>>(), vec![0, 3]);
        drop(v);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nearest_handles_edge_cases() {
        let dir = temp("edges");
        let v = sample(&dir);
        assert!(v.nearest(&[1.0, 0.0, 0.0], 0).is_empty());
        assert!(v.nearest(&[1.0, 0.0], 5).is_empty());
        assert!(v.nearest(&[], 5).is_empty());
        assert_eq!(v.nearest(&[1.0, 0.0, 0.0], usize::MAX).len(), 4);
        drop(v);
        let path = dir.join("none.bin");
        Vectors::write(&path, 3, &[]).unwrap();
        let none = Vectors::open(&path).unwrap();
        assert!(none.is_empty());
        assert!(none.nearest(&[1.0, 0.0, 0.0], 5).is_empty());
        assert!(none.nearest_among(&[1.0, 0.0, 0.0], &[0], 5).is_empty());
        drop(none);
        let one = dir.join("one.bin");
        Vectors::write(&one, 2, &[vec![0.6, 0.8]]).unwrap();
        let one = Vectors::open(&one).unwrap();
        let hits = one.nearest(&[0.6, 0.8], 3);
        assert_eq!(hits.len(), 1);
        assert!((hits[0].1 - 1.0).abs() < 1e-6);
        drop(one);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nearest_among_only_looks_at_the_listed_rows() {
        let dir = temp("among");
        let v = sample(&dir);
        let query = unit(&[1.0, 0.0, 0.0]);
        let hits = v.nearest_among(&query, &[3, 1, 3, 2, 77], 10);
        assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![3, 2, 1]);
        assert_eq!(v.nearest_among(&query, &[3, 1, 2], 1), vec![hits[0]]);
        assert!(v.nearest_among(&query, &[], 10).is_empty());
        assert!(v.nearest_among(&query, &[99], 10).is_empty());
        drop(v);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_large_search_agrees_with_a_plain_scan() {
        let dir = temp("large");
        let (dim, count) = (16usize, 5000usize);
        let mut state = 12345u64;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((state >> 33) as f32 / (1u64 << 31) as f32) - 0.5
        };
        let rows: Vec<Vec<f32>> = (0..count).map(|_| unit(&(0..dim).map(|_| next()).collect::<Vec<f32>>())).collect();
        let path = dir.join("large.bin");
        Vectors::write(&path, dim, &rows).unwrap();
        let v = Vectors::open(&path).unwrap();
        let query = unit(&(0..dim).map(|_| next()).collect::<Vec<f32>>());
        let mut want: Vec<(u32, f32)> = rows.iter().enumerate().map(|(i, r)| (i as u32, dot(&query, r))).collect();
        want.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        want.truncate(10);
        assert_eq!(v.nearest(&query, 10), want);
        let subset: Vec<u32> = (0..count as u32).step_by(7).collect();
        let mut want_among: Vec<(u32, f32)> = subset.iter().map(|&i| (i, dot(&query, &rows[i as usize]))).collect();
        want_among.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        want_among.truncate(5);
        assert_eq!(v.nearest_among(&query, &subset, 5), want_among);
        drop(v);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_wrong_file_is_refused() {
        let dir = temp("guard");
        let path = dir.join("v.bin");
        assert!(Vectors::open(&path).is_err(), "missing file");
        fs::write(&path, b"short").unwrap();
        assert!(Vectors::open(&path).is_err());
        let v = sample(&dir);
        drop(v);
        let good = fs::read(dir.join("v.bin")).unwrap();
        let mut wrong_magic = good.clone();
        wrong_magic[0] = b'X';
        fs::write(&path, &wrong_magic).unwrap();
        assert!(Vectors::open(&path).unwrap_err().to_string().contains("magic"));
        let mut wrong_version = good.clone();
        wrong_version[8] = 9;
        fs::write(&path, &wrong_version).unwrap();
        assert!(Vectors::open(&path).unwrap_err().to_string().contains("version"));
        fs::write(&path, &good[..good.len() - 4]).unwrap();
        assert!(Vectors::open(&path).unwrap_err().to_string().contains("size"));
        let mut huge = good.clone();
        huge[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        fs::write(&path, &huge).unwrap();
        assert!(Vectors::open(&path).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_refuses_rows_of_the_wrong_length_and_a_zero_dimension() {
        let dir = temp("write");
        let path = dir.join("w.bin");
        assert!(Vectors::write(&path, 2, &[vec![1.0, 2.0], vec![1.0]]).is_err());
        assert!(Vectors::write(&path, 0, &[]).is_err());
        assert!(!path.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writing_again_replaces_the_file() {
        let dir = temp("replace");
        let path = dir.join("r.bin");
        Vectors::write(&path, 1, &[vec![1.0]]).unwrap();
        Vectors::write(&path, 1, &[vec![1.0], vec![-1.0]]).unwrap();
        assert_eq!(Vectors::open(&path).unwrap().len(), 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn concurrent_writes_to_one_path_leave_one_whole_matrix() {
        let dir = temp("race");
        let path = dir.join("shared.bin");
        let dim = 8usize;
        // Writer `n` stores `n * 5000` rows that all hold the number `n`, so the matrix that
        // survives says by itself which writer it came from and how long it must be.
        std::thread::scope(|scope| {
            for writer in 1..=8usize {
                let path = &path;
                scope.spawn(move || {
                    let rows = vec![vec![writer as f32; dim]; writer * 5000];
                    for _ in 0..4 {
                        Vectors::write(path, dim, &rows).expect("a concurrent write succeeds");
                    }
                });
            }
        });
        let v = Vectors::open(&path).expect("the surviving file is whole");
        let writer = v.row(0)[0];
        assert_eq!(v.len(), writer as usize * 5000);
        assert!((0..v.len() as u32).all(|i| v.row(i).iter().all(|&x| x == writer)), "rows of two writers are mixed");
        drop(v);
        let leftovers = fs::read_dir(&dir).unwrap().count();
        assert_eq!(leftovers, 1, "a temporary file was left behind");
        fs::remove_dir_all(&dir).unwrap();
    }
}
