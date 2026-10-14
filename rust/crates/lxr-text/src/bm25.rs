//! An inverted index with BM25 scoring, built to give the scores semble gives.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;

/// Term-frequency saturation.
const K1: f64 = 1.5;
/// Document length normalisation.
const B: f64 = 0.75;

/// The documents that hold one term, with how many times each holds it.
#[derive(Clone, Copy, Debug)]
pub struct Postings<'a> {
    /// Document numbers, ascending.
    pub docs: &'a [u32],
    /// Term counts, one for each document.
    pub tfs: &'a [u32],
}

/// A BM25 index over documents that are already split into terms.
///
/// Documents are numbered from 0 in the order they were given to [`Bm25::build`].
/// Postings are stored flat: the terms sorted, and for each term a run of document
/// numbers with the matching term counts.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(try_from = "Stored")]
pub struct Bm25 {
    /// Number of tokens of each document.
    doc_len: Vec<u32>,
    /// Sum of `doc_len`.
    total_len: u64,
    /// Every distinct term, sorted.
    terms: Vec<String>,
    /// `terms[t]` owns the postings `starts[t]..starts[t + 1]`. One longer than `terms`.
    starts: Vec<u32>,
    /// Document numbers of the postings, ascending within a term.
    docs: Vec<u32>,
    /// Term counts of the postings.
    tfs: Vec<u32>,
}

/// The stored form of a [`Bm25`], field for field. Reading goes through it so that an index
/// whose arrays disagree is refused when it is loaded, not when a search indexes past them.
#[derive(Deserialize)]
struct Stored {
    doc_len: Vec<u32>,
    total_len: u64,
    terms: Vec<String>,
    starts: Vec<u32>,
    docs: Vec<u32>,
    tfs: Vec<u32>,
}

impl TryFrom<Stored> for Bm25 {
    type Error = String;

    fn try_from(stored: Stored) -> Result<Bm25, String> {
        let Stored { doc_len, total_len, terms, starts, docs, tfs } = stored;
        if doc_len.iter().map(|&n| u64::from(n)).sum::<u64>() != total_len {
            return Err("the total length is not the sum of the document lengths".to_string());
        }
        if docs.len() != tfs.len() {
            return Err("the postings and their term counts differ in number".to_string());
        }
        // An index with no terms may have no offsets at all (`Bm25::default`) or the one
        // offset that `build` writes; with terms there is one more offset than terms.
        let no_terms = terms.is_empty() && docs.is_empty() && starts.len() <= 1 && starts.iter().all(|&s| s == 0);
        if !no_terms {
            if starts.len() != terms.len() + 1 {
                return Err("there is not one more posting offset than terms".to_string());
            }
            if starts[0] != 0 || starts[starts.len() - 1] as usize != docs.len() || starts.windows(2).any(|w| w[0] > w[1]) {
                return Err("the posting offsets do not run from 0 to the end of the postings".to_string());
            }
            if terms.windows(2).any(|w| w[0] >= w[1]) {
                return Err("the terms are not sorted and distinct".to_string());
            }
            for w in starts.windows(2) {
                let run = &docs[w[0] as usize..w[1] as usize];
                if run.windows(2).any(|d| d[0] >= d[1]) || run.last().is_some_and(|&d| d as usize >= doc_len.len()) {
                    return Err("the postings of a term are not ascending document numbers of this index".to_string());
                }
            }
        }
        Ok(Bm25 { doc_len, total_len, terms, starts, docs, tfs })
    }
}

/// First bytes of the compact stored form (see [`Bm25::to_packed`]).
const PACKED_MAGIC: &[u8; 4] = b"BMX1";

/// Writes `n` as a little-endian base-128 number, seven bits to a byte.
fn put_varint(out: &mut Vec<u8>, mut n: u64) {
    loop {
        let low = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

/// Reads the numbers and byte runs that [`Bm25::to_packed`] wrote. Every read is checked
/// against the end of the input, so damaged bytes are an error and never a panic.
struct PackedReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> PackedReader<'a> {
    #[inline]
    fn varint(&mut self) -> Result<u64, String> {
        // Most numbers are one byte; the loop below is for the rest.
        if let Some(&byte) = self.bytes.get(self.at) {
            if byte < 0x80 {
                self.at += 1;
                return Ok(u64::from(byte));
            }
        }
        self.varint_long()
    }

    fn varint_long(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.bytes.get(self.at).ok_or("the word index ends in the middle of a number")?;
            self.at += 1;
            let part = u64::from(byte & 0x7f);
            if shift == 63 && part > 1 {
                return Err("a number in the word index is too large".to_string());
            }
            value |= part << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("a number in the word index is too long".to_string())
    }

    #[inline]
    fn u32(&mut self) -> Result<u32, String> {
        u32::try_from(self.varint()?).map_err(|_| "a number in the word index does not fit 32 bits".to_string())
    }

    /// A count of items that each take at least one byte, so it cannot exceed what is left.
    fn count(&mut self) -> Result<usize, String> {
        let n = self.varint()?;
        if n > (self.bytes.len() - self.at) as u64 {
            return Err("the word index claims more items than it has bytes".to_string());
        }
        Ok(n as usize)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&end| end <= self.bytes.len()).ok_or("the word index ends in the middle of a term")?;
        let run = &self.bytes[self.at..end];
        self.at = end;
        Ok(run)
    }

    fn u32s(&mut self, n: usize) -> Result<Vec<u32>, String> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.u32()?);
        }
        Ok(out)
    }
}

impl Bm25 {
    /// The index as bytes for a file, in a form that is much smaller than its postcard bytes
    /// and that [`Bm25::from_packed`] reads back to the same index, field for field.
    ///
    /// The terms are sorted, so each is written as the length of the start it shares with the
    /// term before, then the rest. A term's postings are ascending document numbers, so each
    /// is written as the step from the one before. The offsets into the postings are written
    /// as one count per term. Numbers are base-128 varints; the streams (terms, counts,
    /// document steps, term frequencies) are written one after the other, not mixed, so that
    /// a general-purpose codec over the bytes finds the repeats in each.
    pub fn to_packed(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.docs.len() * 2 + self.terms.len() * 8 + self.doc_len.len() * 2 + 64);
        out.extend_from_slice(PACKED_MAGIC);
        // Bit 0: `starts` is empty, as in `Bm25::default()`, and not the one offset `build` writes.
        out.push(u8::from(self.starts.is_empty()));
        put_varint(&mut out, self.doc_len.len() as u64);
        for &n in &self.doc_len {
            put_varint(&mut out, u64::from(n));
        }
        put_varint(&mut out, self.total_len);
        put_varint(&mut out, self.terms.len() as u64);
        let mut previous: &[u8] = &[];
        for term in &self.terms {
            let bytes = term.as_bytes();
            let shared = previous.iter().zip(bytes).take_while(|(a, b)| a == b).count();
            put_varint(&mut out, shared as u64);
            put_varint(&mut out, (bytes.len() - shared) as u64);
            out.extend_from_slice(&bytes[shared..]);
            previous = bytes;
        }
        for run in self.starts.windows(2) {
            put_varint(&mut out, u64::from(run[1] - run[0]));
        }
        put_varint(&mut out, self.docs.len() as u64);
        for run in self.starts.windows(2) {
            let mut before = 0u32;
            for (i, &doc) in self.docs[run[0] as usize..run[1] as usize].iter().enumerate() {
                put_varint(&mut out, u64::from(if i == 0 { doc } else { doc - before }));
                before = doc;
            }
        }
        for &tf in &self.tfs {
            put_varint(&mut out, u64::from(tf));
        }
        out
    }

    /// Reads bytes made by [`Bm25::to_packed`]. Anything else, and bytes that were cut, is an
    /// error; the checks that apply to a stored index apply to what was read.
    pub fn from_packed(bytes: &[u8]) -> io::Result<Bm25> {
        Bm25::read_packed(bytes).map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))
    }

    fn read_packed(bytes: &[u8]) -> Result<Bm25, String> {
        if bytes.len() < PACKED_MAGIC.len() + 1 || &bytes[..PACKED_MAGIC.len()] != PACKED_MAGIC {
            return Err("not a word index (wrong magic)".to_string());
        }
        let flags = bytes[PACKED_MAGIC.len()];
        if flags > 1 {
            return Err("the word index has flags that are not known".to_string());
        }
        let mut r = PackedReader { bytes, at: PACKED_MAGIC.len() + 1 };
        let docs_count = r.count()?;
        let doc_len = r.u32s(docs_count)?;
        let total_len = r.varint()?;
        let term_count = r.count()?;
        let mut terms: Vec<String> = Vec::with_capacity(term_count);
        for _ in 0..term_count {
            let shared = usize::try_from(r.varint()?).map_err(|_| "a shared length is too large".to_string())?;
            let rest = usize::try_from(r.varint()?).map_err(|_| "a term length is too large".to_string())?;
            let before = terms.last().map_or(&[][..], |t| t.as_bytes());
            if shared > before.len() {
                return Err("a term shares more than the term before it has".to_string());
            }
            let tail = r.take(rest)?;
            let mut term = Vec::with_capacity(shared + rest);
            term.extend_from_slice(&before[..shared]);
            term.extend_from_slice(tail);
            terms.push(String::from_utf8(term).map_err(|_| "a term is not text".to_string())?);
        }
        let mut starts: Vec<u32> = Vec::with_capacity(term_count + 1);
        let mut total = 0u32;
        starts.push(0);
        for _ in 0..term_count {
            total = total.checked_add(r.u32()?).ok_or("the postings are more than 32-bit offsets can number")?;
            starts.push(total);
        }
        let postings = r.count()?;
        if postings != total as usize {
            return Err("the posting counts do not add up to the number of postings".to_string());
        }
        let mut docs: Vec<u32> = Vec::with_capacity(postings);
        for run in starts.windows(2) {
            let mut before = 0u32;
            for i in 0..(run[1] - run[0]) {
                let step = r.u32()?;
                before = if i == 0 { step } else { before.checked_add(step).ok_or("a document number is too large")? };
                docs.push(before);
            }
        }
        let tfs = r.u32s(postings)?;
        if r.at != bytes.len() {
            return Err("the word index has bytes after its end".to_string());
        }
        if term_count == 0 && flags == 1 {
            starts.clear();
        }
        Bm25::try_from(Stored { doc_len, total_len, terms, starts, docs, tfs })
    }

    /// Indexes the documents. A document is the list of its terms, repeats included.
    pub fn build<I: IntoIterator<Item = Vec<String>>>(docs: I) -> Bm25 {
        let mut doc_len: Vec<u32> = Vec::new();
        let mut total_len: u64 = 0;
        let mut postings: HashMap<String, Vec<(u32, u32)>> = HashMap::new();
        for (number, mut tokens) in docs.into_iter().enumerate() {
            let number = number as u32;
            doc_len.push(tokens.len() as u32);
            total_len += tokens.len() as u64;
            tokens.sort_unstable();
            let mut at = 0;
            while at < tokens.len() {
                let mut end = at + 1;
                while end < tokens.len() && tokens[end] == tokens[at] {
                    end += 1;
                }
                let count = (end - at) as u32;
                match postings.get_mut(tokens[at].as_str()) {
                    Some(list) => list.push((number, count)),
                    None => {
                        postings.insert(std::mem::take(&mut tokens[at]), vec![(number, count)]);
                    }
                }
                at = end;
            }
        }
        let mut entries: Vec<(String, Vec<(u32, u32)>)> = postings.into_iter().collect();
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let mut terms = Vec::with_capacity(entries.len());
        let mut starts = Vec::with_capacity(entries.len() + 1);
        let mut docs_flat = Vec::new();
        let mut tfs = Vec::new();
        for (term, list) in entries {
            starts.push(docs_flat.len() as u32);
            terms.push(term);
            for (doc, tf) in list {
                docs_flat.push(doc);
                tfs.push(tf);
            }
        }
        starts.push(docs_flat.len() as u32);
        Bm25 { doc_len, total_len, terms, starts, docs: docs_flat, tfs }
    }

    /// Number of documents.
    pub fn len(&self) -> usize {
        self.doc_len.len()
    }

    /// True when no document was indexed.
    pub fn is_empty(&self) -> bool {
        self.doc_len.is_empty()
    }

    /// Number of distinct terms. Terms are numbered from 0 in sorted order.
    pub fn term_count(&self) -> usize {
        self.terms.len()
    }

    /// The term numbered `t` and its postings, `None` past the last term.
    pub fn term_at(&self, t: usize) -> Option<(&str, Postings<'_>)> {
        let term = self.terms.get(t)?;
        Some((term.as_str(), self.run(t)))
    }

    /// The postings of `term`, `None` when no document holds it.
    pub fn postings(&self, term: &str) -> Option<Postings<'_>> {
        let t = self.terms.binary_search_by(|probe| probe.as_str().cmp(term)).ok()?;
        Some(self.run(t))
    }

    fn run(&self, t: usize) -> Postings<'_> {
        let (from, to) = (self.starts[t] as usize, self.starts[t + 1] as usize);
        Postings { docs: &self.docs[from..to], tfs: &self.tfs[from..to] }
    }

    /// Number of tokens of document `doc`, 0 for a number past the last document.
    pub fn doc_len(&self, doc: u32) -> u32 {
        self.doc_len.get(doc as usize).copied().unwrap_or(0)
    }

    /// Number of tokens of all documents together.
    pub fn total_len(&self) -> u64 {
        self.total_len
    }

    /// The inverse document frequency that [`Bm25::search`] gives a term held by `df` documents.
    pub fn idf(&self, df: usize) -> f64 {
        let n = self.len() as f64;
        let df = df as f64;
        (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
    }

    /// The terms held by at least one of the listed documents, as `(term number, how many of
    /// the listed documents hold it)`, in term order. Numbers past the last document are ignored.
    pub fn terms_in(&self, docs: &[u32]) -> Vec<(usize, usize)> {
        let mut mask = vec![0u64; self.len().div_ceil(64)];
        let mut listed = 0usize;
        for &doc in docs {
            let (word, bit) = (doc as usize / 64, doc % 64);
            if word < mask.len() && mask[word] >> bit & 1 == 0 {
                mask[word] |= 1 << bit;
                listed += 1;
            }
        }
        if listed == 0 {
            return Vec::new();
        }
        let mut found = Vec::new();
        for t in 0..self.terms.len() {
            let held = self.run(t).docs.iter().filter(|&&d| mask[d as usize / 64] >> (d % 64) & 1 == 1).count();
            if held > 0 {
                found.push((t, held));
            }
        }
        found
    }

    /// The best `limit` documents for the query terms, as `(document, score)`.
    ///
    /// Scores fall as the list goes on; a document that matches no term is left out;
    /// equal scores are ordered by lower document number. A term repeated in the query
    /// counts that many times.
    pub fn search(&self, query: &[String], limit: usize) -> Vec<(u32, f32)> {
        self.score(query, None, limit)
    }

    /// Like [`Bm25::search`], but only the documents listed in `allowed` can appear.
    /// Scores are the same as in a search over everything: the statistics of the whole
    /// collection are used, not those of the allowed part.
    pub fn search_among(&self, query: &[String], allowed: &[u32], limit: usize) -> Vec<(u32, f32)> {
        let mut mask = vec![0u64; self.len().div_ceil(64)];
        for &doc in allowed {
            if (doc as usize) < self.len() {
                mask[doc as usize / 64] |= 1 << (doc % 64);
            }
        }
        self.score(query, Some(&mask), limit)
    }

    fn score(&self, query: &[String], mask: Option<&[u64]>, limit: usize) -> Vec<(u32, f32)> {
        let n = self.len();
        if n == 0 || limit == 0 || query.is_empty() || self.total_len == 0 {
            return Vec::new();
        }
        let avg_len = self.total_len as f64 / n as f64;
        let mut sorted: Vec<&str> = query.iter().map(String::as_str).collect();
        sorted.sort_unstable();
        let mut scores = vec![0f64; n];
        let mut touched: Vec<u32> = Vec::new();
        let mut at = 0;
        while at < sorted.len() {
            let mut end = at + 1;
            while end < sorted.len() && sorted[end] == sorted[at] {
                end += 1;
            }
            let query_tf = (end - at) as f64;
            let term = sorted[at];
            at = end;
            let Ok(t) = self.terms.binary_search_by(|probe| probe.as_str().cmp(term)) else {
                continue;
            };
            let (from, to) = (self.starts[t] as usize, self.starts[t + 1] as usize);
            let idf = self.idf(to - from);
            for p in from..to {
                let doc = self.docs[p];
                if let Some(mask) = mask {
                    if mask[doc as usize / 64] >> (doc % 64) & 1 == 0 {
                        continue;
                    }
                }
                let tf = self.tfs[p] as f64;
                let dl = self.doc_len[doc as usize] as f64;
                let norm = K1 * (1.0 - B + B * dl / avg_len) + tf;
                let slot = &mut scores[doc as usize];
                if *slot == 0.0 {
                    touched.push(doc);
                }
                *slot += query_tf * idf * (tf / norm);
            }
        }
        let mut hits: Vec<(u32, f32)> = touched
            .into_iter()
            .map(|doc| (doc, scores[doc as usize] as f32))
            .filter(|&(_, score)| score > 0.0)
            .collect();
        let by_rank = |a: &(u32, f32), b: &(u32, f32)| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0));
        if hits.len() > limit {
            hits.select_nth_unstable_by(limit - 1, by_rank);
            hits.truncate(limit);
        }
        hits.sort_unstable_by(by_rank);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(words: &str) -> Vec<String> {
        words.split_whitespace().map(str::to_string).collect()
    }

    fn q(words: &str) -> Vec<String> {
        doc(words)
    }

    fn sample() -> Bm25 {
        Bm25::build(vec![
            doc("parse config file"),
            doc("parse parse parse json"),
            doc("render html template"),
            doc("config config loader"),
        ])
    }

    #[test]
    fn terms_postings_and_lengths_can_be_read() {
        let index = sample();
        // Sorted terms: config file html json loader parse render template.
        assert_eq!(index.term_count(), 8);
        let (term, postings) = index.term_at(0).unwrap();
        assert_eq!((term, postings.docs, postings.tfs), ("config", &[0u32, 3][..], &[1u32, 2][..]));
        assert!(index.term_at(8).is_none());
        let parse = index.postings("parse").unwrap();
        assert_eq!((parse.docs, parse.tfs), (&[0u32, 1][..], &[1u32, 3][..]));
        assert!(index.postings("zebra").is_none());
        assert_eq!((index.doc_len(0), index.doc_len(1), index.doc_len(9)), (3, 4, 0));
        assert_eq!(index.total_len(), 13);
        let empty = Bm25::build(Vec::<Vec<String>>::new());
        assert_eq!((empty.term_count(), empty.postings("a").is_none(), empty.term_at(0).is_none()), (0, true, true));
        assert!(empty.terms_in(&[0]).is_empty());
    }

    #[test]
    fn the_idf_is_the_one_a_search_uses() {
        let index = Bm25::build(vec![doc("alpha beta alpha")]);
        let idf = index.idf(1);
        assert_eq!(idf, (1.0f64 + 0.5 / 1.5).ln());
        let hit = index.search(&q("alpha"), 1)[0].1 as f64;
        assert!((hit - idf * 2.0 / 3.5).abs() < 1e-6);
        assert!(sample().idf(1) > sample().idf(2));
    }

    #[test]
    fn terms_in_counts_how_many_of_the_listed_documents_hold_each_term() {
        let index = sample();
        let names = |docs: &[u32]| -> Vec<(String, usize)> {
            index.terms_in(docs).into_iter().map(|(t, held)| (index.term_at(t).unwrap().0.to_string(), held)).collect()
        };
        assert_eq!(names(&[1]), vec![("json".to_string(), 1), ("parse".to_string(), 1)]);
        let both = names(&[0, 1, 1, 99]);
        assert!(both.contains(&("parse".to_string(), 2)) && both.contains(&("config".to_string(), 1)));
        assert!(!both.iter().any(|(t, _)| t == "render"));
        assert!(index.terms_in(&[]).is_empty() && index.terms_in(&[99]).is_empty());
    }

    #[test]
    fn empty_index_finds_nothing() {
        let index = Bm25::build(Vec::<Vec<String>>::new());
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
        assert!(index.search(&q("a"), 5).is_empty());
        assert!(index.search_among(&q("a"), &[0], 5).is_empty());
    }

    #[test]
    fn one_document_scores_by_formula() {
        let index = Bm25::build(vec![doc("alpha beta alpha")]);
        assert_eq!(index.len(), 1);
        let hits = index.search(&q("alpha"), 10);
        assert_eq!(hits.len(), 1);
        // N = 1, df = 1: idf = ln(1 + 0.5 / 1.5). tf = 2, dl = avgdl: norm = 1.5 + 2.
        let idf = (1.0f64 + 0.5 / 1.5).ln();
        let expected = idf * 2.0 / (1.5 + 2.0);
        assert_eq!(hits[0].0, 0);
        assert!((hits[0].1 as f64 - expected).abs() < 1e-6, "{} vs {}", hits[0].1, expected);
    }

    #[test]
    fn absent_terms_and_empty_queries_give_nothing() {
        let index = sample();
        assert!(index.search(&q("zebra"), 5).is_empty());
        assert!(index.search(&[], 5).is_empty());
        assert!(index.search(&q("parse"), 0).is_empty());
    }

    #[test]
    fn results_fall_in_score_and_drop_non_matches() {
        let index = sample();
        let hits = index.search(&q("parse"), 10);
        assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![1, 0]);
        assert!(hits[0].1 > hits[1].1);
        assert!(hits.iter().all(|h| h.1 > 0.0));
    }

    #[test]
    fn repeated_query_terms_multiply() {
        let index = sample();
        let once = index.search(&q("config"), 10);
        let twice = index.search(&q("config config"), 10);
        assert_eq!(once.len(), twice.len());
        for (a, b) in once.iter().zip(&twice) {
            assert_eq!(a.0, b.0);
            assert!((a.1 * 2.0 - b.1).abs() < 1e-6);
        }
    }

    #[test]
    fn limit_larger_than_the_collection_returns_every_match() {
        let index = sample();
        assert_eq!(index.search(&q("parse config"), 1000).len(), 3);
        assert_eq!(index.search(&q("parse config"), 2).len(), 2);
    }

    #[test]
    fn equal_scores_go_to_the_lower_document() {
        let index = Bm25::build(vec![doc("same words"), doc("same words"), doc("same words")]);
        let hits = index.search(&q("same"), 10);
        assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![0, 1, 2]);
        let cut = index.search(&q("same"), 2);
        assert_eq!(cut.iter().map(|h| h.0).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn search_among_keeps_scores_and_filters_documents() {
        let index = sample();
        let all = index.search(&q("parse config"), 10);
        let among = index.search_among(&q("parse config"), &[3, 0, 3, 99], 10);
        assert_eq!(among.iter().map(|h| h.0).collect::<Vec<_>>(), vec![0, 3]);
        for hit in &among {
            let same = all.iter().find(|a| a.0 == hit.0).unwrap();
            assert_eq!(same.1, hit.1);
        }
        assert!(index.search_among(&q("parse"), &[], 10).is_empty());
        assert_eq!(index.search_among(&q("parse"), &[0, 1], 1).len(), 1);
    }

    #[test]
    fn survives_a_round_trip_through_the_store() {
        let index = sample();
        let dir = std::env::temp_dir().join(format!("lxr-text-bm25-{}", std::process::id()));
        let path = dir.join("bm25.bin");
        lxr_core::store::save(&path, &index).unwrap();
        let back: Bm25 = lxr_core::store::load(&path).unwrap();
        assert_eq!(back.search(&q("parse config"), 10), index.search(&q("parse config"), 10));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Every stored field of an index, for an exact comparison.
    fn fields(b: &Bm25) -> (&[u32], u64, &[String], &[u32], &[u32], &[u32]) {
        (&b.doc_len, b.total_len, &b.terms, &b.starts, &b.docs, &b.tfs)
    }

    fn wide_sample() -> Bm25 {
        let docs: Vec<Vec<String>> = (0..300u32)
            .map(|d| {
                (0..d % 17 + 1)
                    .map(|k| format!("term{}", (d * 7 + k * 13) % 211))
                    .chain(["naïve".to_string(), "日本語".to_string(), "term".to_string()])
                    .collect()
            })
            .collect();
        Bm25::build(docs)
    }

    #[test]
    fn the_packed_form_gives_back_every_field() {
        for index in [sample(), wide_sample(), Bm25::default(), Bm25::build(Vec::<Vec<String>>::new())] {
            let back = Bm25::from_packed(&index.to_packed()).unwrap();
            assert_eq!(fields(&back), fields(&index));
        }
        let (index, back) = (wide_sample(), Bm25::from_packed(&wide_sample().to_packed()).unwrap());
        assert_eq!(back.search(&q("term7 naïve"), 20), index.search(&q("term7 naïve"), 20));
    }

    #[test]
    fn the_packed_form_is_smaller_than_the_arrays_it_holds() {
        let index = wide_sample();
        let fixed_width = 4 * (index.doc_len.len() + index.starts.len() + index.docs.len() + index.tfs.len());
        assert!(index.to_packed().len() < fixed_width / 2, "{} against {fixed_width}", index.to_packed().len());
    }

    #[test]
    fn damaged_packed_bytes_are_an_error_or_another_valid_index_and_never_a_panic() {
        let packed = wide_sample().to_packed();
        for cut in 0..packed.len() {
            assert!(Bm25::from_packed(&packed[..cut]).is_err(), "cut at {cut}");
        }
        for at in 0..packed.len() {
            for flip in [0x01u8, 0x80, 0xff] {
                let mut damaged = packed.clone();
                damaged[at] ^= flip;
                let _ = Bm25::from_packed(&damaged);
            }
        }
        let mut trailing = packed.clone();
        trailing.push(0);
        assert!(Bm25::from_packed(&trailing).is_err());
        assert!(Bm25::from_packed(b"").is_err());
        assert!(Bm25::from_packed(b"BMX1\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x7f").is_err());
    }

    /// Compares with semble's own tokeniser and BM25 class. It runs when `LXR_BM25_PARITY`
    /// names the JSON written by `parity/bm25_reference.py`.
    #[test]
    fn matches_semble_on_the_reference_corpus() {
        let Some(path) = std::env::var_os("LXR_BM25_PARITY") else {
            eprintln!("LXR_BM25_PARITY is not set: skipping the semble comparison");
            return;
        };
        let reference: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let strings = |v: &serde_json::Value| -> Vec<String> {
            v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
        };
        let docs = reference["docs"].as_array().unwrap();
        let mut token_mismatches = 0;
        let mut corpus = Vec::new();
        for doc in docs {
            let expected = strings(&doc["tokens"]);
            if crate::code_tokens(doc["text"].as_str().unwrap()) != expected {
                token_mismatches += 1;
            }
            corpus.push(expected);
        }
        let index = Bm25::build(corpus);
        assert_eq!(index.len(), docs.len());
        let (mut largest, mut identical, mut tie_swaps, mut presence_errors) = (0f64, 0, 0, 0);
        let queries = reference["queries"].as_array().unwrap();
        for query in queries {
            let tokens = strings(&query["tokens"]);
            if crate::code_tokens(query["text"].as_str().unwrap()) != tokens {
                token_mismatches += 1;
            }
            let want: Vec<f64> = query["scores"].as_array().unwrap().iter().map(|s| s.as_f64().unwrap()).collect();
            let got = index.search(&tokens, docs.len());
            let mut have = vec![0f64; docs.len()];
            for &(doc, score) in &got {
                have[doc as usize] = f64::from(score);
            }
            for (a, b) in have.iter().zip(&want) {
                if (*a == 0.0) != (*b == 0.0) {
                    presence_errors += 1;
                }
                largest = largest.max((a - b).abs());
            }
            let top: Vec<usize> = query["top"].as_array().unwrap().iter().map(|d| d.as_u64().unwrap() as usize).collect();
            let ours: Vec<usize> = got.iter().take(top.len()).map(|h| h.0 as usize).collect();
            if ours == top {
                identical += 1;
            } else {
                for (a, b) in ours.iter().zip(&top) {
                    if a != b {
                        assert!((want[*a] - want[*b]).abs() <= 1e-5, "{}: rank differs: {a} vs {b}", query["text"]);
                        tie_swaps += 1;
                    }
                }
                assert_eq!(ours.len(), top.len(), "{}", query["text"]);
            }
        }
        eprintln!(
            "bm25 parity: {} chunks, {} queries, token mismatches {token_mismatches}, largest score difference {largest:.3e}, \
             top-10 identical {identical}/{}, tie swaps {tie_swaps}, presence errors {presence_errors}",
            docs.len(),
            queries.len(),
            queries.len()
        );
        assert_eq!(token_mismatches, 0);
        assert_eq!(presence_errors, 0);
        assert!(largest < 1e-4, "largest score difference {largest}");
    }

    /// An index as JSON text, from the fields of the stored form.
    fn index_json(fields: &str) -> String {
        format!("{{{fields}}}")
    }

    #[test]
    fn an_inconsistent_stored_index_is_refused_instead_of_panicking_in_search() {
        let valid = r#""doc_len":[2,1],"total_len":3,"terms":["a","b"],"starts":[0,2,3],"docs":[0,1,1],"tfs":[1,1,1]"#;
        let good: Bm25 = serde_json::from_str(&index_json(valid)).expect("a consistent index loads");
        assert_eq!(good.search(&q("a"), 5).len(), 2);
        let broken = [
            // The case of the review: a term without postings offsets.
            r#""doc_len":[1],"total_len":1,"terms":["a"],"starts":[],"docs":[],"tfs":[]"#,
            // Offsets run past the postings, run backwards, or do not start at 0.
            r#""doc_len":[2,1],"total_len":3,"terms":["a","b"],"starts":[0,2,9],"docs":[0,1,1],"tfs":[1,1,1]"#,
            r#""doc_len":[2,1],"total_len":3,"terms":["a","b"],"starts":[0,3,2],"docs":[0,1,1],"tfs":[1,1,1]"#,
            r#""doc_len":[2,1],"total_len":3,"terms":["a","b"],"starts":[1,2,3],"docs":[0,1,1],"tfs":[1,1,1]"#,
            r#""doc_len":[2,1],"total_len":3,"terms":["a","b"],"starts":[0,2,2],"docs":[0,1,1],"tfs":[1,1,1]"#,
            // Postings and counts differ in length; a posting names a document that is not there.
            r#""doc_len":[2,1],"total_len":3,"terms":["a","b"],"starts":[0,2,3],"docs":[0,1,1],"tfs":[1,1]"#,
            r#""doc_len":[2,1],"total_len":3,"terms":["a","b"],"starts":[0,2,3],"docs":[0,1,7],"tfs":[1,1,1]"#,
            // Terms out of order or repeated, documents out of order within a term.
            r#""doc_len":[2,1],"total_len":3,"terms":["b","a"],"starts":[0,2,3],"docs":[0,1,1],"tfs":[1,1,1]"#,
            r#""doc_len":[2,1],"total_len":3,"terms":["a","a"],"starts":[0,2,3],"docs":[0,1,1],"tfs":[1,1,1]"#,
            r#""doc_len":[2,1],"total_len":3,"terms":["a","b"],"starts":[0,2,3],"docs":[1,0,1],"tfs":[1,1,1]"#,
            // The total does not add up.
            r#""doc_len":[2,1],"total_len":4,"terms":["a","b"],"starts":[0,2,3],"docs":[0,1,1],"tfs":[1,1,1]"#,
        ];
        for fields in broken {
            let parsed = serde_json::from_str::<Bm25>(&index_json(fields));
            assert!(parsed.is_err(), "accepted: {fields}");
        }
    }

    #[test]
    fn stored_indexes_of_every_shape_that_build_makes_still_load() {
        let shapes: Vec<Bm25> = vec![
            Bm25::default(),
            Bm25::build(Vec::<Vec<String>>::new()),
            Bm25::build(vec![Vec::new(), Vec::new()]),
            Bm25::build(vec![Vec::new(), doc("only here"), Vec::new()]),
            sample(),
        ];
        for index in shapes {
            let text = serde_json::to_string(&index).unwrap();
            let back: Bm25 = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
            assert_eq!(back.search(&q("parse here"), 10), index.search(&q("parse here"), 10));
            assert_eq!(back.len(), index.len());
        }
    }

    #[test]
    fn empty_documents_do_not_break_scoring() {
        let index = Bm25::build(vec![Vec::new(), doc("only here"), Vec::new()]);
        assert_eq!(index.len(), 3);
        let hits = index.search(&q("here"), 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 1);
    }
}
