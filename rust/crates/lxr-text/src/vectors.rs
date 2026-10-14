//! A matrix of normalised vectors in one file, searched by dot product.
//!
//! The file comes in two forms. The plain form (`LXRVEC01`) is the numbers as they are, and
//! is memory-mapped. The packed form (`LXRVEC02`) is lossless too: the top byte of every
//! number (the sign and the high bits of the exponent, which take a handful of values in a
//! matrix of unit vectors) is stored as a three-bit code into a palette of the file's seven
//! commonest top bytes, with the others listed after, and the other three bytes of every
//! number are stored as they are. A packed file is put back together in memory, bit for bit,
//! the first time a search reads the numbers (at once for [`Vectors::open`]). Nothing about
//! the other bytes is assumed: random bits make a packed file larger than a plain one, and
//! then the plain form is written. Only the top bytes of a packed file carry a hash of their
//! own; a caller that needs every byte vouched for hashes the whole file.

use lxr_core::store::{hash64, write_atomic};
use memmap2::Mmap;
use rayon::prelude::*;
use std::fmt;
use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const MAGIC: &[u8; 8] = b"LXRVEC01";
const VERSION: u32 = 1;
const MAGIC_PACKED: &[u8; 8] = b"LXRVEC02";
const VERSION_PACKED: u32 = 2;
/// Magic (8), version (4), dimension (4) and row count (8).
const HEADER: usize = 24;
/// The packed form has one more number after the header: the size of the packed top bytes.
const HEADER_PACKED: usize = HEADER + 8;
/// Rows scored by one parallel task.
const CHUNK_ROWS: usize = 2048;
/// Below this many numbers a search runs on one thread.
const PARALLEL_FROM: usize = 1 << 16;

/// Where the numbers are: in a mapped plain file, in memory, or in the bytes of a packed file
/// (held in memory, not mapped) that are put back together when a search first reads them.
enum Store {
    Mapped(Mmap),
    Owned(Vec<f32>),
    Packed { bytes: Vec<u8>, numbers: OnceLock<Vec<f32>> },
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Store::Mapped(map) => write!(f, "Store(mapped, {} bytes)", map.len()),
            Store::Owned(numbers) => write!(f, "Store(in memory, {} numbers)", numbers.len()),
            Store::Packed { bytes, numbers } => {
                write!(f, "Store(packed, {} bytes, {})", bytes.len(), if numbers.get().is_some() { "put back together" } else { "not read yet" })
            }
        }
    }
}

/// A row-major `f32` matrix, memory-mapped from a plain file, held in memory, or read from a
/// packed file and put back together on first use. Rows are expected to be stored normalised,
/// so that the dot product with a normalised query is the cosine.
#[derive(Debug)]
pub struct Vectors {
    store: Store,
    dim: usize,
    count: usize,
}

impl Vectors {
    /// Writes `rows` to `path`, each of length `dim`, replacing the file in one step.
    /// On Windows a file that is currently mapped cannot be replaced: drop its
    /// [`Vectors`] first.
    pub fn write(path: &Path, dim: usize, rows: &[Vec<f32>]) -> io::Result<()> {
        // `write_atomic` gives every write a temporary file of its own, so two writes to one
        // path never share bytes. The file is built in memory first, which costs one extra
        // copy of the matrix while the write runs.
        write_atomic(path, &Vectors::encode_plain(dim, rows)?)
    }

    /// The bytes of the plain form of `rows`, each of length `dim`: what [`Vectors::write`] writes.
    pub fn encode_plain(dim: usize, rows: &[Vec<f32>]) -> io::Result<Vec<u8>> {
        check_rows(dim, rows)?;
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
        Ok(bytes)
    }

    /// Like [`Vectors::write`], in the packed form when that is smaller than the plain one and
    /// the plain form otherwise. Either way [`Vectors::open`] gives back exactly the numbers
    /// that were written, bit for bit (not-a-number payloads and the sign of zero included).
    ///
    /// The packed form is put back together in memory, where the plain one is mapped, and that
    /// costs time and memory in proportion to the size of the matrix whenever a search reads
    /// it. So a matrix of more than [`PACK_UP_TO`] bytes is written plain, and keeps the open
    /// that costs nothing.
    pub fn write_packed(path: &Path, dim: usize, rows: &[Vec<f32>]) -> io::Result<()> {
        write_atomic(path, &Vectors::encode(dim, rows)?)
    }

    /// The bytes that [`Vectors::write_packed`] writes: the packed form when it is smaller and the
    /// matrix is not above [`PACK_UP_TO`], the plain form otherwise.
    pub fn encode(dim: usize, rows: &[Vec<f32>]) -> io::Result<Vec<u8>> {
        Vectors::encode_up_to(dim, rows, PACK_UP_TO)
    }

    fn encode_up_to(dim: usize, rows: &[Vec<f32>], up_to: usize) -> io::Result<Vec<u8>> {
        check_rows(dim, rows)?;
        if HEADER.saturating_add(rows.len().saturating_mul(dim).saturating_mul(4)) > up_to {
            return Vectors::encode_plain(dim, rows);
        }
        let count = rows.len();
        let mut top = Vec::with_capacity(count * dim);
        let mut rest = Vec::with_capacity(count * dim * 3);
        for value in rows.iter().flatten() {
            let bits = value.to_bits();
            top.push((bits >> 24) as u8);
            rest.extend_from_slice(&bits.to_le_bytes()[..3]);
        }
        let packed_top = pack_top(&top);
        if HEADER_PACKED + packed_top.len() + rest.len() >= HEADER + count * dim * 4 {
            return Vectors::encode_plain(dim, rows);
        }
        let mut bytes = Vec::with_capacity(HEADER_PACKED + packed_top.len() + rest.len());
        bytes.extend_from_slice(MAGIC_PACKED);
        bytes.extend_from_slice(&VERSION_PACKED.to_le_bytes());
        bytes.extend_from_slice(&(dim as u32).to_le_bytes());
        bytes.extend_from_slice(&(count as u64).to_le_bytes());
        bytes.extend_from_slice(&(packed_top.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&packed_top);
        bytes.extend_from_slice(&rest);
        // The packed form is used only when the very bytes about to be written give the numbers
        // back exactly. A search puts them back together later and cannot report a failure then.
        let exact = Vectors::from_bytes(&bytes).is_ok_and(|back| {
            let numbers = back.data();
            numbers.len() == count * dim && numbers.iter().zip(rows.iter().flatten()).all(|(a, b)| a.to_bits() == b.to_bits())
        });
        if !exact {
            return Vectors::encode_plain(dim, rows);
        }
        Ok(bytes)
    }

    /// Opens the file at `path`: a plain file is mapped, a packed file is put back together in
    /// memory at once. Fails for a file that is not a vector file of a known version, whose size
    /// does not match its header, or (packed) whose top bytes are damaged.
    pub fn open(path: &Path) -> io::Result<Vectors> {
        Vectors::open_with(path, |_| (), false)?.1
    }

    /// Like [`Vectors::open`], and also gives what `observe` made of the bytes of the file,
    /// which it is shown first. Those are the very bytes that are then decoded, so a caller
    /// that hashes them knows the hash is that of the vectors it got. An error only when the
    /// file cannot be read; a file that cannot be decoded is the second value.
    ///
    /// A packed file is only read here, with its header and size checked, and is put back
    /// together the first time a search reads the numbers: a search that never does (by words
    /// alone) pays nothing for them. So the caller has to vouch for the bytes, with the hash it
    /// has just taken: a packed file whose top bytes turn out damaged finds nothing. The bytes
    /// are held in memory and the file is not left mapped, so it can be replaced meanwhile.
    pub fn open_observing<T>(path: &Path, observe: impl FnOnce(&[u8]) -> T) -> io::Result<(T, io::Result<Vectors>)> {
        Vectors::open_with(path, observe, true)
    }

    fn open_with<T>(path: &Path, observe: impl FnOnce(&[u8]) -> T, lazy: bool) -> io::Result<(T, io::Result<Vectors>)> {
        let bad = |message: String| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {message}", path.display()));
        let file = File::open(path)?;
        if file.metadata()?.len() < HEADER as u64 {
            let short = std::fs::read(path)?;
            return Ok((observe(&short), Err(bad("too short to be a vector file".to_string()))));
        }
        // SAFETY: the file is only read through the map, and vector files are replaced
        // by renaming a new file over them rather than written in place.
        let map = unsafe { Mmap::map(&file)? };
        if lazy && &map[..8] == MAGIC_PACKED {
            // The bytes are copied out first and shown to `observe` as they are copied, so that the
            // hash and the numbers come from the same bytes whatever happens to the file.
            let bytes = map.to_vec();
            drop(map);
            let seen = observe(&bytes);
            return Ok((seen, Vectors::waiting(bytes).map_err(|e| bad(e.to_string()))));
        }
        let seen = observe(&map);
        Ok((seen, Vectors::from_map(map).map_err(|e| bad(e.to_string()))))
    }

    /// The vectors of the bytes of a packed file, whose header and size are checked now and
    /// whose numbers are put back together on first use.
    fn waiting(bytes: Vec<u8>) -> io::Result<Vectors> {
        let layout = Layout::of(&bytes)?;
        Ok(Vectors { store: Store::Packed { bytes, numbers: OnceLock::new() }, dim: layout.dim, count: layout.count })
    }

    /// The vectors of a file that is mapped: a plain file stays mapped, a packed one is put
    /// back together in memory.
    fn from_map(map: Mmap) -> io::Result<Vectors> {
        let bad = |message: String| io::Error::new(io::ErrorKind::InvalidData, message);
        if &map[..8] == MAGIC_PACKED {
            return Vectors::from_bytes(&map);
        }
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
        Ok(Vectors { store: Store::Mapped(map), dim, count: count as usize })
    }

    /// The vectors of a vector file that is already in memory, plain or packed. The numbers
    /// are copied out of `bytes`, so the caller may drop them. This is how a caller that has
    /// checked the bytes it read (a hash, say) opens exactly those bytes.
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Vectors> {
        let bad = |message: &str| io::Error::new(io::ErrorKind::InvalidData, format!("vector file: {message}"));
        if bytes.len() < HEADER {
            return Err(bad("too short to be a vector file"));
        }
        let (magic, version) = (&bytes[..8], u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes")));
        let dim = u32::from_le_bytes(bytes[12..16].try_into().expect("four bytes")) as usize;
        let count = u64::from_le_bytes(bytes[16..24].try_into().expect("eight bytes"));
        let numbers = (count as u128).checked_mul(dim as u128).filter(|_| dim != 0);
        if magic == MAGIC {
            if version != VERSION {
                return Err(bad(&format!("version {version} is not supported")));
            }
            if numbers.and_then(|n| n.checked_mul(4)).and_then(|n| n.checked_add(HEADER as u128)) != Some(bytes.len() as u128) {
                return Err(bad("the size of the file does not match its header"));
            }
            let data = bytes[HEADER..].chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four bytes"))).collect();
            return Ok(Vectors { store: Store::Owned(data), dim, count: count as usize });
        }
        if magic != MAGIC_PACKED {
            return Err(bad("not a vector file (wrong magic)"));
        }
        let layout = Layout::of(bytes)?;
        Ok(Vectors { store: Store::Owned(layout.decode(bytes)?), dim: layout.dim, count: layout.count })
    }

    /// The vectors of rows that are already in memory, each of length `dim`. They are copied
    /// into one matrix, so the caller may drop the rows.
    pub fn from_rows(dim: usize, rows: &[Vec<f32>]) -> io::Result<Vectors> {
        check_rows(dim, rows)?;
        Ok(Vectors { store: Store::Owned(rows.iter().flatten().copied().collect()), dim, count: rows.len() })
    }

    /// Puts a packed file back together now, when that has not been done: for a thread that
    /// does it beside the others, so that the first search finds the numbers ready. Does
    /// nothing for a plain file or numbers that are in memory already.
    pub fn warm(&self) {
        let _ = self.data();
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
        self.data().get(i * self.dim..(i + 1) * self.dim).unwrap_or(&[])
    }

    /// The best `limit` rows for `query` by dot product, as `(row, score)`, highest first,
    /// equal scores with the lower row first. A query of the wrong length finds nothing.
    pub fn nearest(&self, query: &[f32], limit: usize) -> Vec<(u32, f32)> {
        if query.len() != self.dim || limit == 0 || self.count == 0 {
            return Vec::new();
        }
        let data = self.data();
        if data.is_empty() {
            return Vec::new();
        }
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
        if data.is_empty() {
            return Vec::new();
        }
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

    /// The numbers of all rows, one after the other; a packed file is put back together by the
    /// first call. A packed file that cannot be put back together has no numbers, and a search
    /// then finds nothing, never a wrong row. That is not expected: the writer checks that the
    /// file it writes comes back whole, and the caller's hash that it is the file that was written.
    fn data(&self) -> &[f32] {
        let numbers: &[f32] = match &self.store {
            Store::Mapped(map) => {
                // SAFETY: as in `open`: every bit pattern is a valid `f32`, and `open` checked that
                // the whole region is aligned, so the middle part is the data and nothing is left over.
                let (_, data, _) = unsafe { map[HEADER..].align_to::<f32>() };
                data
            }
            Store::Owned(numbers) => numbers,
            Store::Packed { bytes, numbers } => numbers.get_or_init(|| Layout::of(bytes).and_then(|layout| layout.decode(bytes)).unwrap_or_default()),
        };
        if numbers.len() == self.count * self.dim {
            numbers
        } else {
            &[]
        }
    }
}

/// Where the parts of a packed file lie, once its header and its size have been checked.
struct Layout {
    dim: usize,
    count: usize,
    /// Numbers in the file: `count * dim`.
    numbers: usize,
    /// Where the three stored bytes of every number begin.
    rest_at: usize,
}

impl Layout {
    /// Reads and checks the header of a packed file, and that its parts add up to its size.
    fn of(bytes: &[u8]) -> io::Result<Layout> {
        let bad = |message: &str| io::Error::new(io::ErrorKind::InvalidData, format!("vector file: {message}"));
        if bytes.len() < HEADER_PACKED {
            return Err(bad("too short to be a packed vector file"));
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes"));
        if &bytes[..8] != MAGIC_PACKED {
            return Err(bad("not a packed vector file (wrong magic)"));
        }
        if version != VERSION_PACKED {
            return Err(bad(&format!("version {version} is not supported")));
        }
        let dim = u32::from_le_bytes(bytes[12..16].try_into().expect("four bytes")) as usize;
        let count = u64::from_le_bytes(bytes[16..24].try_into().expect("eight bytes"));
        let numbers = (count as u128).checked_mul(dim as u128).filter(|_| dim != 0);
        let packed_len = u64::from_le_bytes(bytes[HEADER..HEADER_PACKED].try_into().expect("eight bytes"));
        let rest_at = (HEADER_PACKED as u128).checked_add(u128::from(packed_len));
        // The three stored bytes of every number must be exactly what is left after the packed top bytes.
        let rest_len = numbers.and_then(|n| n.checked_mul(3));
        if rest_at.zip(rest_len).and_then(|(at, len)| at.checked_add(len)) != Some(bytes.len() as u128) {
            return Err(bad("the size of the file does not match its header"));
        }
        let (numbers, rest_at) = (numbers.expect("checked above") as usize, rest_at.expect("checked above") as usize);
        Ok(Layout { dim, count: count as usize, numbers, rest_at })
    }

    /// The numbers of the packed file `bytes`, which `Layout::of` accepted: the top bytes
    /// unpacked (and checked against their hash) and joined with the stored bytes.
    fn decode(&self, bytes: &[u8]) -> io::Result<Vec<f32>> {
        let top = unpack_top(&bytes[HEADER_PACKED..self.rest_at], self.numbers)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("vector file: {e}")))?;
        Ok(join(&top, &bytes[self.rest_at..]))
    }
}

/// Errors unless there are `dim` numbers in every row, `dim` is a usable length and the row
/// numbers fit 32 bits.
fn check_rows(dim: usize, rows: &[Vec<f32>]) -> io::Result<()> {
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
    Ok(())
}

/// Largest plain size, in bytes, of a matrix that [`Vectors::write_packed`] packs. A packed file
/// costs, whenever a search reads it, a pass over every number and a private copy of the matrix,
/// both in proportion to its size, where a plain file is mapped for nothing. At this size that is
/// a few milliseconds on a thread beside the others (see the contract, section 11); the packing
/// saves 14% of the file, so above it the saving is not worth a cost that grows with the repository.
const PACK_UP_TO: usize = 8 << 20;

/// Palette entries of a packed top-byte block; code 7 stands for a byte that is not in it.
const PALETTE: usize = 7;
/// Palette (7), hash of the original bytes (8) and the number of exceptions (8).
const TOP_HEADER: usize = PALETTE + 16;

/// The top bytes of a matrix as three-bit codes. The seven commonest bytes of `top` are the
/// palette and have the codes 0 to 6; any other byte has the code 7 and is listed, in order,
/// among the exceptions. Layout: palette, hash of `top`, number of exceptions, the exceptions,
/// the codes packed least significant bit first.
fn pack_top(top: &[u8]) -> Vec<u8> {
    let mut seen = [0usize; 256];
    for &byte in top {
        seen[byte as usize] += 1;
    }
    let mut order: Vec<u8> = (0..=255u8).filter(|&b| seen[b as usize] > 0).collect();
    order.sort_by(|&a, &b| seen[b as usize].cmp(&seen[a as usize]).then(a.cmp(&b)));
    let mut palette = [0u8; PALETTE];
    let mut code_of = [PALETTE as u8; 256];
    for (code, &byte) in order.iter().take(PALETTE).enumerate() {
        palette[code] = byte;
        code_of[byte as usize] = code as u8;
    }
    let mut exceptions = Vec::new();
    let mut codes = Vec::with_capacity((top.len() * 3).div_ceil(8));
    let (mut bits, mut held) = (0u32, 0u32);
    for &byte in top {
        let code = code_of[byte as usize];
        if code as usize == PALETTE {
            exceptions.push(byte);
        }
        held |= u32::from(code) << bits;
        bits += 3;
        while bits >= 8 {
            codes.push(held as u8);
            held >>= 8;
            bits -= 8;
        }
    }
    if bits > 0 {
        codes.push(held as u8);
    }
    let mut out = Vec::with_capacity(TOP_HEADER + exceptions.len() + codes.len());
    out.extend_from_slice(&palette);
    out.extend_from_slice(&hash64(top).to_le_bytes());
    out.extend_from_slice(&(exceptions.len() as u64).to_le_bytes());
    out.extend_from_slice(&exceptions);
    out.extend_from_slice(&codes);
    out
}

/// The `count` top bytes that [`pack_top`] packed. An error for a block of another size, one
/// that runs out of exceptions or has some left, or one whose bytes do not hash to the stored
/// hash.
fn unpack_top(block: &[u8], count: usize) -> io::Result<Vec<u8>> {
    let bad = |message: &str| io::Error::new(io::ErrorKind::InvalidData, message.to_string());
    if block.len() < TOP_HEADER {
        return Err(bad("the packed top bytes are cut short"));
    }
    let palette = &block[..PALETTE];
    let hash = u64::from_le_bytes(block[PALETTE..PALETTE + 8].try_into().expect("eight bytes"));
    let listed = u64::from_le_bytes(block[PALETTE + 8..TOP_HEADER].try_into().expect("eight bytes"));
    let codes_len = count.checked_mul(3).map(|bits| bits.div_ceil(8)).ok_or_else(|| bad("too many numbers"))?;
    let listed = usize::try_from(listed).ok().filter(|&n| n <= count).ok_or_else(|| bad("more exceptions than numbers"))?;
    if TOP_HEADER.checked_add(listed).and_then(|n| n.checked_add(codes_len)) != Some(block.len()) {
        return Err(bad("the size of the packed top bytes does not match its header"));
    }
    let (exceptions, codes) = block[TOP_HEADER..].split_at(listed);
    let mut out = vec![0u8; count];
    let mut next = 0usize;
    // The byte for a code: a palette entry, or the next exception listed.
    let mut decode = |code: u32| -> io::Result<u8> {
        if (code as usize) < PALETTE {
            return Ok(palette[code as usize]);
        }
        let byte = exceptions.get(next).copied().ok_or_else(|| bad("the packed top bytes run out of exceptions"))?;
        next += 1;
        Ok(byte)
    };
    let mut groups = out.chunks_exact_mut(8);
    for (group, source) in (&mut groups).zip(codes.chunks_exact(3)) {
        let word = u32::from(source[0]) | u32::from(source[1]) << 8 | u32::from(source[2]) << 16;
        for (k, slot) in group.iter_mut().enumerate() {
            *slot = decode((word >> (3 * k)) & 7)?;
        }
    }
    let tail = groups.into_remainder();
    if !tail.is_empty() {
        let mut word = 0u32;
        for (i, &byte) in codes[count / 8 * 3..].iter().enumerate() {
            word |= u32::from(byte) << (8 * i);
        }
        for (k, slot) in tail.iter_mut().enumerate() {
            *slot = decode((word >> (3 * k)) & 7)?;
        }
    }
    if next != listed {
        return Err(bad("the packed top bytes list exceptions that no code asks for"));
    }
    if hash64(&out) != hash {
        return Err(bad("the packed top bytes are damaged (hash mismatch)"));
    }
    Ok(out)
}

/// The numbers of a matrix from their top bytes and, three to a number, their other bytes.
fn join(top: &[u8], rest: &[u8]) -> Vec<f32> {
    top.iter()
        .zip(rest.chunks_exact(3))
        .map(|(&high, low)| f32::from_bits(u32::from(low[0]) | u32::from(low[1]) << 8 | u32::from(low[2]) << 16 | u32::from(high) << 24))
        .collect()
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
    fn rows_held_in_memory_are_searched_like_a_file() {
        let rows = vec![unit(&[1.0, 0.0]), unit(&[0.0, 1.0]), unit(&[1.0, 1.0])];
        let v = Vectors::from_rows(2, &rows).unwrap();
        assert_eq!((v.len(), v.dim()), (3, 2));
        assert_eq!(v.row(2), &rows[2][..]);
        let hits = v.nearest(&unit(&[1.0, 0.0]), 3);
        assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![0, 2, 1]);
        assert!(Vectors::from_rows(2, &[]).unwrap().is_empty());
        assert!(Vectors::from_rows(0, &[]).is_err());
        assert!(Vectors::from_rows(2, &[vec![1.0]]).is_err());
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

    /// Unit vectors of an embedding-like spread: signs and sizes vary, as in a real matrix.
    fn spread(rows: usize, dim: usize) -> Vec<Vec<f32>> {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 11) as f64 / (1u64 << 53) as f64) as f32 - 0.5
        };
        (0..rows).map(|_| unit(&(0..dim).map(|_| next() * next().abs()).collect::<Vec<f32>>())).collect()
    }

    fn bits(rows: &[Vec<f32>]) -> Vec<u32> {
        rows.iter().flatten().map(|x| x.to_bits()).collect()
    }

    fn opened_bits(v: &Vectors) -> Vec<u32> {
        (0..v.len() as u32).flat_map(|i| v.row(i).iter().map(|x| x.to_bits()).collect::<Vec<_>>()).collect()
    }

    #[test]
    fn a_packed_file_gives_every_number_back_bit_for_bit_and_is_smaller() {
        let dir = temp("packed");
        let rows = spread(300, 64);
        let (plain, packed) = (dir.join("plain.bin"), dir.join("packed.bin"));
        Vectors::write(&plain, 64, &rows).unwrap();
        Vectors::write_packed(&packed, 64, &rows).unwrap();
        assert!(fs::metadata(&packed).unwrap().len() < fs::metadata(&plain).unwrap().len() * 9 / 10);
        assert_eq!(&fs::read(&packed).unwrap()[..8], MAGIC_PACKED);
        let (from_plain, from_packed) = (Vectors::open(&plain).unwrap(), Vectors::open(&packed).unwrap());
        assert_eq!(opened_bits(&from_packed), bits(&rows));
        assert_eq!(opened_bits(&from_plain), bits(&rows));
        let query = &rows[17];
        assert_eq!(from_packed.nearest(query, 10), from_plain.nearest(query, 10));
        assert_eq!(from_packed.nearest_among(query, &[3, 17, 250], 3), from_plain.nearest_among(query, &[3, 17, 250], 3));
        let from_memory = Vectors::from_bytes(&fs::read(&packed).unwrap()).unwrap();
        assert_eq!(opened_bits(&from_memory), bits(&rows));
        drop((from_plain, from_packed, from_memory));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn special_numbers_survive_the_packed_form_with_their_exact_bits() {
        let dir = temp("special");
        let odd = [
            0.0f32,
            -0.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            f32::from_bits(0x7fc0_1234),
            f32::from_bits(0xffa0_0001),
            f32::MIN_POSITIVE,
            f32::from_bits(1),
            f32::from_bits(0x8000_0001),
            f32::MAX,
            f32::MIN,
            1.0,
            -1.0,
            0.1,
        ];
        let rows: Vec<Vec<f32>> = (0..200).map(|r| (0..odd.len()).map(|c| odd[(r + c) % odd.len()]).collect()).collect();
        let path = dir.join("s.bin");
        Vectors::write_packed(&path, odd.len(), &rows).unwrap();
        assert_eq!(opened_bits(&Vectors::open(&path).unwrap()), bits(&rows));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_matrix_above_the_size_limit_is_written_plain_and_stays_mapped() {
        let dir = temp("limit");
        let rows = spread(300, 64);
        let (below, above) = (dir.join("below.bin"), dir.join("above.bin"));
        let plain_size = HEADER + 300 * 64 * 4;
        fs::write(&below, Vectors::encode_up_to(64, &rows, plain_size).unwrap()).unwrap();
        fs::write(&above, Vectors::encode_up_to(64, &rows, plain_size - 1).unwrap()).unwrap();
        assert_eq!(&fs::read(&below).unwrap()[..8], MAGIC_PACKED);
        assert_eq!(&fs::read(&above).unwrap()[..8], MAGIC);
        let (packed, plain) = (Vectors::open(&below).unwrap(), Vectors::open(&above).unwrap());
        assert!(matches!(packed.store, Store::Owned(_)) && matches!(plain.store, Store::Mapped(_)));
        assert_eq!((opened_bits(&packed), opened_bits(&plain)), (bits(&rows), bits(&rows)));
        drop((packed, plain));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_observing_shows_the_bytes_that_it_then_decodes() {
        let dir = temp("observe");
        let rows = spread(200, 32);
        let path = dir.join("o.bin");
        for packed in [true, false] {
            if packed {
                Vectors::write_packed(&path, 32, &rows).unwrap();
            } else {
                Vectors::write(&path, 32, &rows).unwrap();
            }
            let (seen, opened) = Vectors::open_observing(&path, |bytes| (bytes.len(), hash64(bytes))).unwrap();
            let on_disk = fs::read(&path).unwrap();
            assert_eq!(seen, (on_disk.len(), hash64(&on_disk)));
            assert_eq!(opened_bits(&opened.unwrap()), bits(&rows));
        }
        // A file that cannot be decoded still shows its bytes; one that cannot be read is an error.
        fs::write(&path, b"short").unwrap();
        let (seen, opened) = Vectors::open_observing(&path, |bytes| bytes.len()).unwrap();
        assert_eq!(seen, 5);
        assert!(opened.is_err());
        fs::write(&path, [0u8; 64]).unwrap();
        let (seen, opened) = Vectors::open_observing(&path, |bytes| bytes.len()).unwrap();
        assert_eq!(seen, 64);
        assert!(opened.unwrap_err().to_string().contains("magic"));
        fs::remove_file(&path).unwrap();
        assert!(Vectors::open_observing(&path, |_| ()).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_packed_file_opened_for_a_search_is_put_back_together_only_when_its_numbers_are_read() {
        let dir = temp("lazy");
        let rows = spread(200, 32);
        let path = dir.join("l.bin");
        Vectors::write_packed(&path, 32, &rows).unwrap();
        assert_eq!(&fs::read(&path).unwrap()[..8], MAGIC_PACKED);
        let waiting = |v: &Vectors| matches!(&v.store, Store::Packed { numbers, .. } if numbers.get().is_none());
        let lazy = Vectors::open_observing(&path, |_| ()).unwrap().1.unwrap();
        // The header is known at once, and the numbers are not read for it.
        assert_eq!((lazy.len(), lazy.dim()), (200, 32));
        assert!(waiting(&lazy));
        assert!(lazy.row(500).is_empty() && waiting(&lazy), "a row that is not there needs no numbers");
        // The first search that reads them puts them back together, bit for bit, once.
        let eager = Vectors::open(&path).unwrap();
        assert!(matches!(eager.store, Store::Owned(_)), "open puts a packed file back together at once");
        assert_eq!(lazy.nearest(&rows[5], 10), eager.nearest(&rows[5], 10));
        assert!(!waiting(&lazy));
        assert_eq!(opened_bits(&lazy), bits(&rows));
        assert_eq!(lazy.nearest_among(&rows[9], &[1, 9, 150], 3), eager.nearest_among(&rows[9], &[1, 9, 150], 3));
        drop((lazy, eager));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_packed_file_that_cannot_be_put_back_together_is_an_error_at_open_and_finds_nothing_when_read_later() {
        let dir = temp("lazy-damaged");
        let rows = spread(120, 32);
        let path = dir.join("d.bin");
        Vectors::write_packed(&path, 32, &rows).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        // A change in the hash of the top bytes leaves the header and the sizes right, so only
        // putting the numbers back together can tell.
        bytes[HEADER_PACKED + PALETTE] ^= 0xff;
        fs::write(&path, &bytes).unwrap();
        assert!(Vectors::open(&path).is_err());
        let (_, opened) = Vectors::open_observing(&path, |_| ()).unwrap();
        let lazy = opened.unwrap();
        assert_eq!((lazy.len(), lazy.dim()), (120, 32));
        assert!(lazy.row(3).is_empty());
        assert!(lazy.nearest(&rows[3], 5).is_empty());
        assert!(lazy.nearest_among(&rows[3], &[1, 2, 3], 5).is_empty());
        drop(lazy);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn numbers_with_nothing_to_pack_are_written_plain_and_come_back() {
        let dir = temp("nopack");
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let rows: Vec<Vec<f32>> = (0..40)
            .map(|_| {
                (0..16)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        f32::from_bits((state >> 16) as u32)
                    })
                    .collect()
            })
            .collect();
        let path = dir.join("n.bin");
        Vectors::write_packed(&path, 16, &rows).unwrap();
        assert_eq!(&fs::read(&path).unwrap()[..8], MAGIC, "random bits do not shrink, so the plain form is kept");
        assert_eq!(opened_bits(&Vectors::open(&path).unwrap()), bits(&rows));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_damaged_packed_file_is_an_error_where_the_file_itself_can_tell() {
        let dir = temp("damaged");
        let rows = spread(120, 32);
        let path = dir.join("d.bin");
        Vectors::write_packed(&path, 32, &rows).unwrap();
        let good = fs::read(&path).unwrap();
        assert!(Vectors::from_bytes(&good).is_ok());
        for cut in (0..good.len()).step_by(29).chain([good.len() - 1]) {
            assert!(Vectors::from_bytes(&good[..cut]).is_err(), "cut at {cut}");
        }
        // Header fields and the packed top bytes are checked by the file. The three stored
        // bytes of each number are not, so a change there is for the caller's own hash to find.
        let packed_len = u64::from_le_bytes(good[HEADER..HEADER_PACKED].try_into().unwrap()) as usize;
        let want = bits(&rows);
        for at in 0..HEADER_PACKED + packed_len {
            for flip in [0x01u8, 0x55, 0x80] {
                let mut damaged = good.clone();
                damaged[at] ^= flip;
                if let Ok(v) = Vectors::from_bytes(&damaged) {
                    // A bit that no decoder reads (padding at the end of a stream) may change
                    // nothing; it never changes a number.
                    assert_eq!(opened_bits(&v), want, "byte {at} changed and other numbers came back");
                }
            }
        }
        let mut count = good.clone();
        count[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(Vectors::from_bytes(&count).is_err());
        let mut dim = good.clone();
        dim[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert!(Vectors::from_bytes(&dim).is_err());
        let mut size = good.clone();
        size[HEADER..HEADER_PACKED].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(Vectors::from_bytes(&size).is_err());
        fs::write(&path, &good[..good.len() - 1]).unwrap();
        assert!(Vectors::open(&path).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_top_byte_block_round_trips_every_shape_of_input() {
        let mut state = 0x1234_5678_9abc_def1u64;
        let mut random = |n: usize, modulus: u64| -> Vec<u8> {
            (0..n)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state % modulus) as u8
                })
                .collect()
        };
        let skewed: Vec<u8> = random(5000, 1000).into_iter().map(|b| if b < 244 { b } else { 0x3d }).collect();
        let cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            vec![0x3d],
            vec![0x3d; 7],
            vec![0x3d; 8],
            vec![0x3d; 9],
            (0..=255u8).collect(),
            (0..=255u8).cycle().take(1000).collect(),
            random(1001, 3),
            random(1000, 7),
            random(1000, 8),
            random(4096, 256),
            skewed,
        ];
        for top in cases {
            let block = pack_top(&top);
            assert_eq!(unpack_top(&block, top.len()).unwrap(), top, "{} bytes", top.len());
            // The count is the caller's, from the header of the file; a wrong one is not accepted.
            assert!(unpack_top(&block, top.len() + 1).is_err());
            if !top.is_empty() {
                assert!(unpack_top(&block, top.len() - 1).is_err());
            }
        }
    }

    #[test]
    fn a_changed_top_byte_block_is_an_error_or_gives_the_same_bytes() {
        let common = [0x3d, 0xbd, 0x3c, 0xbc, 0x3b, 0xbb, 0x3e, 0xbe, 0x39, 0xb9, 0x31, 0xb1];
        // Twelve values, the last five rare, so that the block has exceptions to lose.
        let top: Vec<u8> = (0..3000u32).map(|i| common[if i % 17 == 0 { 7 + (i / 17 % 5) as usize } else { (i * i % 7) as usize }]).collect();
        let block = pack_top(&top);
        assert!(block.len() < top.len(), "{} against {}", block.len(), top.len());
        for at in 0..block.len() {
            for flip in [0x01u8, 0x04, 0x80, 0xff] {
                let mut damaged = block.clone();
                damaged[at] ^= flip;
                if let Ok(back) = unpack_top(&damaged, top.len()) {
                    // Only the unused bits after the last code can change without a trace.
                    assert_eq!(back, top, "byte {at} changed and other bytes came back");
                }
            }
        }
        for cut in 0..block.len() {
            assert!(unpack_top(&block[..cut], top.len()).is_err(), "cut at {cut}");
        }
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
