//! Vectors learned from the repository itself. No model, no network, no pretrained table.
//!
//! The chunks are the documents of the word index. A word gets a vector by random indexing:
//! every chunk has a fixed random sparse vector, and the vector of a word is the sum of the
//! random vectors of the chunks that hold it, each weighted by how much more often the word
//! occurs there than chance gives (positive pointwise mutual information), then scaled to
//! length 1. The vector of a chunk is the sum of the vectors of its words, weighted the same
//! way, scaled to length 1. The vector of a query is made from its words alike. Two chunks
//! come out close when their words keep company with each other across the repository, even
//! if they share none: a meaning channel made of the repository alone.
//!
//! Everything is a function of the word index and a fixed seed, so one index always gives
//! the same vectors. The vectors of the chunks are kept in a file beside the index when the
//! index is built with the switch on, and are made again in memory when that file is missing.
//! The vector of a query needs no table of words: it is made again from the word index, which
//! holds every word's chunks.

use lxr_core::pack;
use lxr_core::store::write_atomic;
use lxr_text::{code_tokens, Bm25, Postings, Vectors};
use rayon::prelude::*;
use std::io;
use std::path::Path;

/// Length of every vector.
pub(crate) const DIM: usize = 256;
/// Non-zero numbers in the random vector of a chunk.
const SPREAD: usize = 8;
/// Fixed seed of the random vectors.
const SEED: u64 = 0x9E37_79B9_7F4A_7C15;
/// A word held by fewer chunks than this has no company to keep.
const MIN_DF: usize = 2;
/// A word held by more than this share of the chunks (one in this many) says nothing.
const MAX_DF_SHARE: usize = 4;
/// The lowest cosine that counts as a match: below it a score is the noise of the projection.
const FLOOR: f32 = 0.1;
/// Most postings of usable words that vectors are made from; a larger index has no such vectors.
const MAX_POSTINGS: usize = 40_000_000;

/// The next number of the splitmix64 sequence that starts at `state`.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Adds `weight` times the random vector of chunk `doc` to `out`: [`SPREAD`] signed units
/// at places the chunk number decides.
fn scatter(doc: u32, weight: f32, out: &mut [f32]) {
    let mut state = SEED ^ u64::from(doc).wrapping_mul(0xD1B5_4A32_D192_ED03);
    for _ in 0..SPREAD {
        let z = splitmix(&mut state);
        let sign = if (z >> 40) & 1 == 0 { weight } else { -weight };
        out[(z % DIM as u64) as usize] += sign;
    }
}

/// How much more often a word occurs in a chunk than chance gives, `ln(tf * total / (cf * len))`,
/// and 0 when it does not occur more often than that.
fn pmi(tf: u32, total: u64, cf: u64, len: u32) -> f32 {
    if tf == 0 || cf == 0 || len == 0 {
        return 0.0;
    }
    let ratio = f64::from(tf) * total as f64 / (cf as f64 * f64::from(len));
    ratio.ln().max(0.0) as f32
}

/// True when a word held by `df` of `n` chunks takes part.
fn usable(df: usize, n: usize) -> bool {
    df >= MIN_DF && df.saturating_mul(MAX_DF_SHARE) <= n
}

/// Scales `v` to length 1. False, and `v` untouched, when it is all zeros.
fn normalise(v: &mut [f32]) -> bool {
    let norm = v.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>().sqrt();
    if norm == 0.0 || !norm.is_finite() {
        return false;
    }
    for x in v.iter_mut() {
        *x = (f64::from(*x) / norm) as f32;
    }
    true
}

/// The weight of every posting of a word, in order.
fn weights<'a>(bm25: &'a Bm25, postings: Postings<'a>) -> impl Iterator<Item = (u32, f32)> + 'a {
    let cf: u64 = postings.tfs.iter().map(|&t| u64::from(t)).sum();
    let total = bm25.total_len();
    postings.docs.iter().zip(postings.tfs).map(move |(&doc, &tf)| (doc, pmi(tf, total, cf, bm25.doc_len(doc))))
}

/// Writes into `out` the unit vector of the word with these postings. False when the word has no
/// chunk where it occurs more often than chance.
fn word_vector(bm25: &Bm25, postings: Postings<'_>, out: &mut [f32]) -> bool {
    out.fill(0.0);
    for (doc, weight) in weights(bm25, postings) {
        if weight > 0.0 {
            scatter(doc, weight, out);
        }
    }
    normalise(out)
}

/// The vectors of all chunks, one row each (a row is all zeros for a chunk without usable
/// words). `None` for an index with no chunk or with more postings than [`MAX_POSTINGS`].
pub(crate) fn compute(bm25: &Bm25) -> Option<Vec<Vec<f32>>> {
    let n = bm25.len();
    if n == 0 {
        return None;
    }
    let mut words: Vec<usize> = Vec::new();
    let mut postings = 0usize;
    for t in 0..bm25.term_count() {
        let (_, run) = bm25.term_at(t)?;
        if usable(run.docs.len(), n) {
            postings = postings.saturating_add(run.docs.len());
            words.push(t);
        }
    }
    if postings > MAX_POSTINGS {
        return None;
    }

    let mut table = vec![0f32; words.len() * DIM];
    table.par_chunks_mut(DIM).zip(words.par_iter()).for_each(|(out, &t)| {
        if let Some((_, run)) = bm25.term_at(t) {
            word_vector(bm25, run, out);
        }
    });

    // The words of each chunk, found by sorting the postings by chunk: counts, offsets, then fill.
    let mut starts = vec![0usize; n + 1];
    for &t in &words {
        if let Some((_, run)) = bm25.term_at(t) {
            for &doc in run.docs {
                starts[doc as usize + 1] += 1;
            }
        }
    }
    for d in 0..n {
        starts[d + 1] += starts[d];
    }
    let mut next = starts.clone();
    let mut entries = vec![(0u32, 0f32); postings];
    for (slot, &t) in words.iter().enumerate() {
        if let Some((_, run)) = bm25.term_at(t) {
            for (doc, weight) in weights(bm25, run) {
                entries[next[doc as usize]] = (slot as u32, weight);
                next[doc as usize] += 1;
            }
        }
    }

    Some(
        (0..n)
            .into_par_iter()
            .map(|doc| {
                let mut row = vec![0f32; DIM];
                for &(slot, weight) in &entries[starts[doc]..starts[doc + 1]] {
                    if weight > 0.0 {
                        let word = &table[slot as usize * DIM..(slot as usize + 1) * DIM];
                        for (x, w) in row.iter_mut().zip(word) {
                            *x += weight * w;
                        }
                    }
                }
                normalise(&mut row);
                row
            })
            .collect(),
    )
}

/// The unit vector of a query, made from the words of the query that the word index knows and
/// that take part, each weighted by its inverse document frequency. `None` when there is none.
pub(crate) fn query_vector(bm25: &Bm25, query: &str) -> Option<Vec<f32>> {
    let mut tokens = code_tokens(query);
    tokens.sort_unstable();
    tokens.dedup();
    let n = bm25.len();
    let mut sum = vec![0f32; DIM];
    let mut word = vec![0f32; DIM];
    for token in &tokens {
        let Some(run) = bm25.postings(token) else { continue };
        if !usable(run.docs.len(), n) || !word_vector(bm25, run, &mut word) {
            continue;
        }
        let idf = bm25.idf(run.docs.len()) as f32;
        for (s, w) in sum.iter_mut().zip(&word) {
            *s += idf * w;
        }
    }
    normalise(&mut sum).then_some(sum)
}

/// The name of the file that holds the vectors of an index whose word index has this hash.
pub(crate) fn file_name(bm25_hash: u64) -> String {
    format!("repo1-{bm25_hash:016x}.bin")
}

/// Writes the rows of [`compute`] to `path`, in the container of `lxr_core::pack` (stored, not
/// compressed): no table records a hash for this file, so the container carries the length and
/// the hash of every byte, in whichever form the vectors are.
pub(crate) fn write(path: &Path, rows: &[Vec<f32>]) -> io::Result<()> {
    write_atomic(path, &pack::store(&Vectors::encode(DIM, rows)?))
}

/// The vectors of the chunks of one index.
#[derive(Debug)]
pub(crate) struct RepoVectors {
    vectors: Vectors,
}

impl RepoVectors {
    /// Reads the file at `path`. `None` when it is missing, not made by [`write`], damaged (a
    /// changed byte anywhere is found by the container's hash), or not the vectors of `docs` chunks.
    pub(crate) fn open(path: &Path, docs: usize) -> Option<RepoVectors> {
        let bytes = pack::unpack(&std::fs::read(path).ok()?).ok()?;
        let vectors = Vectors::from_bytes(&bytes).ok()?;
        (vectors.len() == docs && vectors.dim() == DIM).then_some(RepoVectors { vectors })
    }

    /// Holds the rows of [`compute`] in memory.
    pub(crate) fn in_memory(rows: &[Vec<f32>]) -> Option<RepoVectors> {
        Vectors::from_rows(DIM, rows).ok().map(|vectors| RepoVectors { vectors })
    }

    /// The numbers of the `limit` chunks closest to `query`, closest first, and only those that
    /// score above the floor.
    pub(crate) fn nearest(&self, query: &[f32], limit: usize) -> Vec<u32> {
        self.vectors.nearest(query, limit).into_iter().filter(|hit| hit.1 >= FLOOR).map(|hit| hit.0).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEMES: [[&str; 3]; 4] =
        [["lexer", "token", "parse"], ["pixel", "canvas", "colour"], ["socket", "packet", "route"], ["inode", "block", "mount"]];

    /// 160 chunks in four themes of 40, each theme with its own three words that always come
    /// together, and a filler word that cuts across the themes. Chunks 0 and 1 hold "lexer" and
    /// "parse" but not "token"; chunk 40 holds "pixel" and "colour" but not "canvas".
    fn corpus() -> Bm25 {
        let docs: Vec<Vec<String>> = (0..160)
            .map(|i| {
                let mut words: Vec<String> = THEMES[i / 40].iter().map(|w| w.to_string()).collect();
                if i < 2 {
                    words.retain(|w| w != "token");
                }
                if i == 40 {
                    words.retain(|w| w != "canvas");
                }
                words.push(format!("filler{}", i % 7));
                words
            })
            .collect();
        Bm25::build(docs)
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn rows_are_unit_vectors_or_zeros_and_the_same_every_time() {
        let bm25 = corpus();
        let rows = compute(&bm25).unwrap();
        assert_eq!(rows.len(), 160);
        for row in &rows {
            assert_eq!(row.len(), DIM);
            let norm: f32 = row.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!(norm == 0.0 || (norm - 1.0).abs() < 1e-4, "{norm}");
        }
        let again = compute(&bm25).unwrap();
        assert!(rows.iter().flatten().zip(again.iter().flatten()).all(|(a, b)| a.to_bits() == b.to_bits()));
    }

    #[test]
    fn chunks_with_company_in_common_are_closer_than_chunks_without() {
        let rows = compute(&corpus()).unwrap();
        let (parse, draw) = (&rows[5], &rows[45]);
        assert!(cosine(parse, &rows[6]) > 0.7, "{}", cosine(parse, &rows[6]));
        assert!(cosine(parse, draw).abs() < 0.35, "{}", cosine(parse, draw));
    }

    #[test]
    fn a_query_finds_chunks_by_company_not_only_by_the_word() {
        let bm25 = corpus();
        let repo = RepoVectors::in_memory(&compute(&bm25).unwrap()).unwrap();
        // "token" is in chunks 2..40 only; chunks 0 and 1 hold "lexer" and "parse" but not "token",
        // and they are still among the nearest, because those words keep company with "token".
        let query = query_vector(&bm25, "token").unwrap();
        let near = repo.nearest(&query, 40);
        assert!(near.contains(&0) && near.contains(&1), "{near:?}");
        assert!(near.iter().filter(|&&d| d < 40).count() >= 38, "{near:?}");
        // The same for the second theme: "canvas" misses chunk 40, "pixel" and "colour" find it.
        let draw = repo.nearest(&query_vector(&bm25, "canvas").unwrap(), 40);
        assert!(draw.contains(&40), "{draw:?}");
        assert!(draw.iter().filter(|&&d| (40..80).contains(&d)).count() >= 38, "{draw:?}");
    }

    #[test]
    fn a_query_without_a_usable_word_has_no_vector() {
        let bm25 = corpus();
        assert!(query_vector(&bm25, "").is_none());
        assert!(query_vector(&bm25, "nothing here is known").is_none());
        // A word in every chunk, and a word in one chunk only, take no part.
        let rare = Bm25::build((0..20).map(|i| vec!["common".to_string(), format!("only{i}")]));
        assert!(query_vector(&corpus(), "filler9").is_none());
        assert!(query_vector(&rare, "common only3").is_none());
        assert!(compute(&Bm25::build(Vec::<Vec<String>>::new())).is_none());
    }

    #[test]
    fn the_vectors_survive_a_file() {
        let bm25 = corpus();
        let rows = compute(&bm25).unwrap();
        let dir = std::env::temp_dir().join(format!("lxr-repo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(file_name(0xABCD));
        assert_eq!(file_name(0xABCD), "repo1-000000000000abcd.bin");
        write(&path, &rows).unwrap();
        let back = RepoVectors::open(&path, 160).unwrap();
        let query = query_vector(&bm25, "token").unwrap();
        let held = RepoVectors::in_memory(&rows).unwrap();
        assert_eq!(back.nearest(&query, 10), held.nearest(&query, 10));
        // Another chunk count, a missing file and a damaged file give nothing.
        assert!(RepoVectors::open(&path, 161).is_none());
        assert!(RepoVectors::open(&dir.join("missing.bin"), 160).is_none());
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.truncate(bytes.len() / 2);
        std::fs::write(&path, bytes).unwrap();
        assert!(RepoVectors::open(&path, 160).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Writes the vectors of `rows` to `path` in the plain form or in the form that `write` chooses.
    fn put(path: &Path, rows: &[Vec<f32>], plain: bool) {
        if plain {
            write_atomic(path, &pack::store(&Vectors::encode_plain(DIM, rows).unwrap())).unwrap();
        } else {
            write(path, rows).unwrap();
        }
    }

    #[test]
    fn a_byte_changed_anywhere_in_the_file_is_not_trusted() {
        let rows = compute(&corpus()).unwrap();
        let dir = std::env::temp_dir().join(format!("lxr-repo-flip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(file_name(0xBEEF));
        for plain in [false, true] {
            put(&path, &rows, plain);
            let good = std::fs::read(&path).unwrap();
            assert!(RepoVectors::open(&path, 160).is_some(), "plain={plain}");
            let form: &[u8; 8] = if plain { b"LXRVEC01" } else { b"LXRVEC02" };
            assert_eq!(&pack::unpack(&good).unwrap()[..8], form, "the file is not in the form the test means to damage");
            // Every 331st byte, so that each part of the file is hit (the header, the packed top
            // bytes, the other three bytes of the numbers), and the last one. One bit of each is
            // flipped, the lowest of a number's bytes included.
            let mut at: Vec<usize> = (0..good.len()).step_by(331).collect();
            at.push(good.len() - 1);
            for i in at {
                let mut bytes = good.clone();
                bytes[i] ^= 0x01;
                std::fs::write(&path, &bytes).unwrap();
                assert!(RepoVectors::open(&path, 160).is_none(), "plain={plain}: byte {i} of {} changed and was trusted", good.len());
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
