//! Switches of the search that are off unless they are asked for.
//!
//! A search with [`Tuning::default`] is the search of pgr 0.2: the same numbers, the same
//! arithmetic, the same answers. Everything here is a way to try something else on purpose:
//! the weights of the fusion, a second pass over the best hits, and vectors made from the
//! repository itself instead of from a model. The values come from the environment
//! (`PGR_SEARCH_WEIGHTS`, `PGR_SEARCH_REFINE`, `PGR_SEARCH_VECTORS`) and are read by the
//! program; this module only parses them.

use crate::rank;

/// Where the vectors of the meaning channel come from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Meaning {
    /// The embedding model, when there is one. What pgr always did.
    #[default]
    Model,
    /// Vectors learned from the repository's own word co-occurrence. No model is used.
    Repo,
    /// Both: the model's vectors and the repository's, each with a share of the meaning weight.
    Both,
}

/// The numbers of the fusion. The defaults give the search of pgr 0.2.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Weights {
    /// Scale of the lifts for file names and for definitions that a query names (1 = as it was).
    pub names: f64,
    /// Scale of the lift of the file that holds the most good chunks.
    pub files: f64,
    /// Weight of the words in the fusion, as a multiple of what it was.
    pub words: f32,
    /// Weight of the vectors in the fusion, as a multiple of what it was.
    pub meaning: f32,
    /// The constant of reciprocal rank fusion.
    pub k: f32,
    /// Multiple of the best score added to a definition whose documentation line holds the
    /// words of the query, in proportion to their weight (0 = no such boost).
    pub doc: f64,
    /// Weight of the words of a refinement pass, as a share of the weight of the words.
    pub refine: f32,
    /// Most words a refinement pass adds to the query.
    pub terms: usize,
    /// Share of the meaning weight that goes to the repository's vectors when both kinds are used.
    pub repo: f32,
}

impl Default for Weights {
    fn default() -> Weights {
        Weights { names: 1.0, files: 1.0, words: 1.0, meaning: 1.0, k: rank::RRF_K, doc: 0.0, refine: 0.5, terms: 8, repo: 0.5 }
    }
}

/// Every switch of the search, with the defaults off.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tuning {
    pub weights: Weights,
    /// How many of the best hits a refinement pass learns words from. 0 = no refinement.
    pub refine: usize,
    /// Which vectors the meaning channel uses.
    pub meaning: Meaning,
}

impl Default for Tuning {
    fn default() -> Tuning {
        Tuning { weights: Weights::default(), refine: 0, meaning: Meaning::Model }
    }
}

/// The names `PGR_SEARCH_WEIGHTS` knows.
pub const WEIGHT_KEYS: [&str; 9] = ["names", "files", "words", "meaning", "k", "doc", "refine", "terms", "repo"];

/// Most hits a refinement pass may learn from.
const MAX_REFINE: usize = 50;
/// Most words a refinement pass may add.
const MAX_TERMS: usize = 64;

impl Tuning {
    /// Reads the three values as they are written in the environment; `None` is a variable that
    /// is not set. A value that cannot be understood is an error that says which and why:
    /// a misspelt switch must not quietly give the answers of the default.
    pub fn parse(weights: Option<&str>, refine: Option<&str>, vectors: Option<&str>) -> Result<Tuning, String> {
        let mut tuning = Tuning::default();
        if let Some(text) = weights {
            tuning.weights = parse_weights(text).map_err(|e| format!("PGR_SEARCH_WEIGHTS: {e}"))?;
        }
        if let Some(text) = refine {
            tuning.refine = parse_refine(text)?;
        }
        if let Some(text) = vectors {
            tuning.meaning = parse_meaning(text)?;
        }
        Ok(tuning)
    }

    /// True when the search is the search of pgr 0.2.
    pub fn is_default(&self) -> bool {
        *self == Tuning::default()
    }

    /// True when the meaning channel needs vectors made from the repository.
    pub fn uses_repo_vectors(&self) -> bool {
        self.meaning != Meaning::Model
    }
}

fn parse_weights(text: &str) -> Result<Weights, String> {
    let mut weights = Weights::default();
    let mut seen: Vec<&str> = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (key, value) = part.split_once('=').ok_or_else(|| format!("'{part}' is not key=value"))?;
        let (key, value) = (key.trim(), value.trim());
        if !WEIGHT_KEYS.contains(&key) {
            return Err(format!("unknown weight '{key}' (known: {})", WEIGHT_KEYS.join(", ")));
        }
        if seen.contains(&key) {
            return Err(format!("weight '{key}' is given twice"));
        }
        seen.push(key);
        let number: f64 = value.parse().map_err(|_| format!("weight '{key}': '{value}' is not a number"))?;
        if !number.is_finite() || number < 0.0 {
            return Err(format!("weight '{key}': {value} must be a number of 0 or more"));
        }
        match key {
            "names" => weights.names = number,
            "files" => weights.files = number,
            "words" => weights.words = number as f32,
            "meaning" => weights.meaning = number as f32,
            "k" => {
                if number <= 0.0 || number > 1_000_000.0 {
                    return Err(format!("weight 'k': {value} must be above 0 and at most 1000000"));
                }
                weights.k = number as f32;
            }
            "doc" => weights.doc = number,
            "refine" => weights.refine = number as f32,
            "terms" => {
                if number.fract() != 0.0 || number < 1.0 || number > MAX_TERMS as f64 {
                    return Err(format!("weight 'terms': {value} must be a whole number from 1 to {MAX_TERMS}"));
                }
                weights.terms = number as usize;
            }
            _ => {
                if number > 1.0 {
                    return Err(format!("weight 'repo': {value} must be from 0 to 1"));
                }
                weights.repo = number as f32;
            }
        }
    }
    Ok(weights)
}

fn parse_refine(text: &str) -> Result<usize, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(0);
    }
    match text.parse::<usize>() {
        Ok(n) if n <= MAX_REFINE => Ok(n),
        _ => Err(format!("PGR_SEARCH_REFINE: '{text}' must be a whole number from 0 (off) to {MAX_REFINE}")),
    }
}

fn parse_meaning(text: &str) -> Result<Meaning, String> {
    match text.trim() {
        "" | "model" => Ok(Meaning::Model),
        "repo" => Ok(Meaning::Repo),
        "both" => Ok(Meaning::Both),
        other => Err(format!("PGR_SEARCH_VECTORS: '{other}' is not model, repo or both")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_set_is_the_default_search() {
        let tuning = Tuning::parse(None, None, None).unwrap();
        assert!(tuning.is_default());
        assert_eq!(tuning.weights.k, rank::RRF_K);
        assert_eq!(tuning.refine, 0);
        assert_eq!(tuning.meaning, Meaning::Model);
        assert!(!tuning.uses_repo_vectors());
        // Empty values are the same as unset ones.
        assert!(Tuning::parse(Some(""), Some("0"), Some("model")).unwrap().is_default());
        assert!(Tuning::parse(Some(" , "), Some(" "), Some("")).unwrap().is_default());
    }

    #[test]
    fn weights_are_read_by_name() {
        let tuning = Tuning::parse(Some("names=2, words=0.5,meaning=3,k=30,doc=0.25,files=0,refine=1,terms=5,repo=0.75"), None, None).unwrap();
        let w = tuning.weights;
        assert_eq!((w.names, w.files, w.words, w.meaning, w.k), (2.0, 0.0, 0.5, 3.0, 30.0));
        assert_eq!((w.doc, w.refine, w.terms, w.repo), (0.25, 1.0, 5, 0.75));
        assert!(!tuning.is_default());
        // A key that is left out keeps its default.
        let part = Tuning::parse(Some("k=10"), None, None).unwrap().weights;
        assert_eq!(part, Weights { k: 10.0, ..Weights::default() });
    }

    #[test]
    fn a_weight_that_cannot_be_used_is_an_error_that_names_it() {
        for (text, needle) in [
            ("nmes=1", "unknown weight 'nmes'"),
            ("names", "not key=value"),
            ("names=x", "not a number"),
            ("names=-1", "0 or more"),
            ("names=NaN", "0 or more"),
            ("names=inf", "0 or more"),
            ("k=0", "above 0"),
            ("k=2000000", "at most 1000000"),
            ("terms=0", "whole number"),
            ("terms=2.5", "whole number"),
            ("terms=65", "whole number"),
            ("repo=1.5", "0 to 1"),
            ("names=1,names=2", "given twice"),
        ] {
            let error = Tuning::parse(Some(text), None, None).unwrap_err();
            assert!(error.starts_with("PGR_SEARCH_WEIGHTS: ") && error.contains(needle), "{text}: {error}");
        }
    }

    #[test]
    fn refine_and_vectors_are_read() {
        assert_eq!(Tuning::parse(None, Some("3"), None).unwrap().refine, 3);
        assert_eq!(Tuning::parse(None, Some(" 50 "), None).unwrap().refine, 50);
        for bad in ["51", "-1", "x", "1.5"] {
            assert!(Tuning::parse(None, Some(bad), None).is_err(), "{bad}");
        }
        assert_eq!(Tuning::parse(None, None, Some("repo")).unwrap().meaning, Meaning::Repo);
        assert_eq!(Tuning::parse(None, None, Some("both")).unwrap().meaning, Meaning::Both);
        assert!(Tuning::parse(None, None, Some("both")).unwrap().uses_repo_vectors());
        assert!(Tuning::parse(None, None, Some("Repo")).is_err());
        assert!(Tuning::parse(None, None, Some("neural")).is_err());
    }
}
