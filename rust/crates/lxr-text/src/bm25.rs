//! An inverted index with BM25 scoring, built to give the scores semble gives.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Term-frequency saturation.
const K1: f64 = 1.5;
/// Document length normalisation.
const B: f64 = 0.75;

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

impl Bm25 {
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
            let df = (to - from) as f64;
            let idf = (1.0 + (n as f64 - df + 0.5) / (df + 0.5)).ln();
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
