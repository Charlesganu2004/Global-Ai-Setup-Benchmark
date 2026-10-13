//! Rank fusion and string similarity.

use std::collections::{HashMap, HashSet};

/// Reciprocal rank fusion of ranked lists of ids.
///
/// The score of an id is the sum, over the lists it is in, of `weight / (k + rank)`,
/// where the rank is the 1-based position in the list. A list without a weight in
/// `weights` counts with weight 1. An id repeated in one list counts at its first
/// position only. The result is highest score first, lower id first among equals.
pub fn rrf(lists: &[&[u32]], k: f32, weights: &[f32]) -> Vec<(u32, f32)> {
    let mut scores: HashMap<u32, f64> = HashMap::new();
    for (i, list) in lists.iter().enumerate() {
        let weight = f64::from(weights.get(i).copied().unwrap_or(1.0));
        let mut seen: HashSet<u32> = HashSet::with_capacity(list.len());
        for (position, &id) in list.iter().enumerate() {
            if seen.insert(id) {
                *scores.entry(id).or_insert(0.0) += weight / (f64::from(k) + (position + 1) as f64);
            }
        }
    }
    let mut fused: Vec<(u32, f32)> = scores.into_iter().map(|(id, score)| (id, score as f32)).collect();
    fused.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    fused
}

/// The pg_trgm similarity of two strings, from 0 to 1.
///
/// Both are lowercased and cut into words at every character that is not a letter or
/// digit. Each word is padded with two blanks in front and one behind, and all
/// three-character windows of it are its trigrams. The result is the number of trigrams
/// the two strings share divided by the number of distinct trigrams in either. A string
/// with no word has no trigram, and then the similarity is 0.
pub fn trigram_similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (trigrams(a), trigrams(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (mut i, mut j, mut shared) = (0, 0, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
        }
    }
    shared as f32 / (a.len() + b.len() - shared) as f32
}

/// The distinct trigrams of a string, sorted.
fn trigrams(text: &str) -> Vec<[char; 3]> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    for word in lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        let padded: Vec<char> = "  ".chars().chain(word.chars()).chain(" ".chars()).collect();
        out.extend(padded.windows(3).map(|w| [w[0], w[1], w[2]]));
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_of_nothing_is_empty() {
        assert!(rrf(&[], 60.0, &[]).is_empty());
        assert!(rrf(&[&[], &[]], 60.0, &[1.0, 1.0]).is_empty());
    }

    #[test]
    fn rrf_of_one_list_keeps_its_order_and_uses_the_formula() {
        let fused = rrf(&[&[7, 3, 9]], 60.0, &[2.0]);
        assert_eq!(fused.iter().map(|f| f.0).collect::<Vec<_>>(), vec![7, 3, 9]);
        assert!((fused[0].1 - 2.0 / 61.0).abs() < 1e-7);
        assert!((fused[2].1 - 2.0 / 63.0).abs() < 1e-7);
    }

    #[test]
    fn rrf_adds_across_lists_with_weights() {
        let first: &[u32] = &[1, 2, 3];
        let second: &[u32] = &[3, 2];
        let fused = rrf(&[first, second], 60.0, &[0.3, 0.7]);
        // id 2: 0.3 / 62 + 0.7 / 62; id 3: 0.3 / 63 + 0.7 / 61; id 1: 0.3 / 61.
        let want = |id: u32| fused.iter().find(|f| f.0 == id).unwrap().1;
        assert!((want(2) - 1.0 / 62.0).abs() < 1e-6);
        assert!((want(3) - (0.3 / 63.0 + 0.7 / 61.0) as f32).abs() < 1e-6);
        assert!((want(1) - 0.3 / 61.0).abs() < 1e-6);
        assert_eq!(fused.iter().map(|f| f.0).collect::<Vec<_>>(), vec![3, 2, 1]);
    }

    #[test]
    fn rrf_breaks_ties_by_lower_id_and_defaults_missing_weights() {
        let first: &[u32] = &[10, 20];
        let second: &[u32] = &[20, 10];
        let fused = rrf(&[first, second], 60.0, &[]);
        assert_eq!(fused.iter().map(|f| f.0).collect::<Vec<_>>(), vec![10, 20]);
        assert_eq!(fused[0].1, fused[1].1);
    }

    #[test]
    fn rrf_counts_a_repeated_id_once_per_list() {
        let fused = rrf(&[&[5, 5, 6]], 60.0, &[1.0]);
        assert_eq!(fused.len(), 2);
        assert!((fused[0].1 - 1.0 / 61.0).abs() < 1e-7);
        assert!((fused[1].1 - 1.0 / 63.0).abs() < 1e-7);
    }

    #[test]
    fn trigram_of_a_word_has_padding() {
        assert_eq!(trigrams("abc").len(), 4);
        assert_eq!(trigrams("a").len(), 2);
        assert!(trigrams("").is_empty());
        assert!(trigrams("  ,.; ").is_empty());
    }

    #[test]
    fn trigram_similarity_matches_pg_trgm_examples() {
        // From the pg_trgm documentation and its regression tests.
        assert!((trigram_similarity("word", "two words") - 0.363_636_37).abs() < 1e-6);
        assert!((trigram_similarity("word", "word") - 1.0).abs() < 1e-7);
        // cat: 4 trigrams, cats: 5, 3 shared: 3 / 6.
        assert!((trigram_similarity("cat", "cats") - 0.5).abs() < 1e-7);
        assert!((trigram_similarity("Hello", "hello") - 1.0).abs() < 1e-7);
    }

    #[test]
    fn trigram_similarity_edge_cases() {
        assert_eq!(trigram_similarity("", ""), 0.0);
        assert_eq!(trigram_similarity("abc", ""), 0.0);
        assert_eq!(trigram_similarity("", "abc"), 0.0);
        assert_eq!(trigram_similarity("abc", "xyz"), 0.0);
        assert_eq!(trigram_similarity("!!!", "???"), 0.0);
        let ab = trigram_similarity("parse_config", "ParseConfig");
        assert!(ab > 0.0 && ab < 1.0);
        let symmetric = trigram_similarity("loader", "download") - trigram_similarity("download", "loader");
        assert_eq!(symmetric, 0.0);
    }
}
