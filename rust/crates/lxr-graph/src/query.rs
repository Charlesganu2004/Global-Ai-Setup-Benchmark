//! Answering a free-text question: find the nodes the words point at, walk around them,
//! and keep as much of the neighbourhood as the budget allows.
//!
//! This follows graphify's query: words are weighted by how rare they are among the
//! names, the best matches become seeds, and a short walk collects their surroundings.

use crate::types::*;
use lxr_core::est_tokens;
use regex::Regex;
use std::collections::{HashSet, VecDeque};
use std::sync::OnceLock;

/// Words of a question that say nothing about the code: question and auxiliary words,
/// "work", and common fillers of a few other languages.
const STOP_WORDS: &[&str] = &[
    "about", "after", "again", "all", "also", "and", "any", "are", "because", "been", "before", "being", "both", "but", "can",
    "could", "did", "does", "doing", "done", "each", "else", "for", "from", "had", "has", "have", "having", "her", "here",
    "him", "his", "how", "into", "its", "just", "may", "might", "more", "most", "must", "not", "off", "once", "only", "other",
    "our", "out", "over", "per", "she", "should", "some", "such", "than", "that", "the", "their", "them", "then", "there",
    "these", "they", "this", "those", "through", "too", "under", "very", "was", "were", "what", "when", "where", "which",
    "while", "who", "whom", "whose", "why", "will", "with", "work", "works", "would", "you", "your",
    // German
    "das", "dem", "den", "der", "die", "ein", "eine", "einen", "ist", "mit", "nicht", "und", "wie", "wir", "von", "auf",
    "für", "was", "wo", "welche", "welcher",
    // French
    "les", "des", "une", "est", "que", "qui", "dans", "pour", "pas", "comment", "avec", "sur", "sont", "quel", "quelle", "où",
    // Spanish
    "los", "las", "una", "por", "con", "para", "como", "donde", "dónde", "qué", "cuál", "del",
    // Portuguese
    "uma", "não", "são", "onde", "quais", "qual", "mais", "pelo", "pela",
    // Italian
    "gli", "che", "per", "come", "dove", "sono", "della", "delle", "dello", "nel", "nella", "cosa",
];

/// Words that talk about relations instead of naming code. They are not given a seed of their
/// own when the question has other words.
const RELATION_WORDS: &[&str] = &[
    "call", "calls", "called", "calling", "use", "uses", "used", "using", "import", "imports", "imported", "importing",
    "extend", "extends", "extended", "extending", "implement", "implements", "implemented", "implementing", "depend",
    "depends", "depended", "depending", "dependency", "dependencies", "reference", "references", "referenced", "referencing",
];

/// How many seeds the ranking itself may give.
const RANKED_SEEDS: usize = 3;
/// A ranked seed must score at least this share of the top score.
const SEED_SHARE: f64 = 0.2;
/// How far the walk goes from the seeds.
const WALK_DEPTH: u32 = 2;

fn word_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\w+").expect("constant pattern"))
}

fn piece_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"[^\W_]+").expect("constant pattern"))
}

/// The terms of a question: lower-case words of more than two letters (or with a
/// non-ASCII letter) that are not stop words. When nothing is left, every word.
fn query_terms(question: &str) -> Vec<String> {
    let lower = question.to_lowercase();
    let words: Vec<&str> = word_pattern().find_iter(&lower).map(|m| m.as_str()).collect();
    let kept: Vec<String> = words
        .iter()
        .filter(|w| w.chars().count() > 2 || w.chars().any(|c| c.is_alphabetic() && !c.is_ascii()))
        .filter(|w| !STOP_WORDS.contains(*w))
        .map(|w| w.to_string())
        .collect();
    if kept.is_empty() {
        words.iter().map(|w| w.to_string()).collect()
    } else {
        kept
    }
}

/// The distinct letter-and-digit pieces of the terms, in order of first appearance.
fn query_tokens(terms: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut tokens = Vec::new();
    for term in terms {
        for piece in piece_pattern().find_iter(term) {
            if seen.insert(piece.as_str()) {
                tokens.push(piece.as_str().to_string());
            }
        }
    }
    tokens
}

/// A name with its separators taken out: `parse_config` becomes `parseconfig`.
fn joined_pieces(text: &str) -> String {
    piece_pattern().find_iter(text).map(|m| m.as_str()).collect()
}

/// The lower-case texts of one node that a question is matched against.
struct Texts {
    name: String,
    name_joined: String,
    qual: String,
    qual_joined: String,
    doc: String,
}

impl Graph {
    /// How many distinct nodes `node` is joined to by any edge, in either direction.
    fn degree(&self, node: u32) -> u32 {
        self.tables.deg.get(node as usize).copied().unwrap_or(0)
    }

    /// Answers a question in words with a neighbourhood of the graph, at most
    /// `budget_tokens` large (counted over one `name path:first-last` row per node and one
    /// short row per edge, at four characters to a token).
    ///
    /// The words are weighted by rarity among the names. Up to three of the best matches
    /// above a fifth of the top score become seeds, then the best match for each word. A
    /// walk of two steps over all edges, taken as undirected and not expanding any busy
    /// non-seed node, collects the neighbourhood. Nodes are kept in order of (steps,
    /// neighbours, number) while they fit; a seed is always kept.
    pub fn query(&self, question: &str, budget_tokens: usize) -> QueryAnswer {
        let total = self.nodes.len();
        let tokens = query_tokens(&query_terms(question));
        if total == 0 || tokens.is_empty() {
            return QueryAnswer::default();
        }
        let texts: Vec<Texts> = self
            .nodes
            .iter()
            .map(|n| {
                let name = n.name.to_lowercase();
                let qual = n.qual.to_lowercase();
                Texts {
                    name_joined: joined_pieces(&name),
                    qual_joined: joined_pieces(&qual),
                    name,
                    qual,
                    doc: n.doc.to_lowercase(),
                }
            })
            .collect();
        let paths: Vec<String> = self.files.iter().map(|p| p.to_lowercase()).collect();
        let weights: Vec<(String, f64)> = tokens
            .iter()
            .map(|t| {
                let df = texts.iter().filter(|x| x.name.contains(t.as_str()) || x.qual.contains(t.as_str())).count();
                (t.clone(), (1.0 + total as f64 / (1.0 + df as f64)).ln())
            })
            .collect();
        let score_of = |id: usize, wanted: &[(String, f64)]| -> f64 {
            let x = &texts[id];
            let path = paths.get(self.nodes[id].file as usize).map(String::as_str).unwrap_or("");
            let best = wanted.iter().map(|w| w.1).fold(0.0, f64::max);
            let joined: String = wanted.iter().map(|w| w.0.as_str()).collect();
            let mut score = 0.0;
            let labels = [x.name.as_str(), x.name_joined.as_str(), x.qual.as_str(), x.qual_joined.as_str()];
            if !joined.is_empty() {
                if labels.iter().any(|l| *l == joined) {
                    score += 10000.0 * best;
                } else if labels.iter().any(|l| l.starts_with(&joined)) {
                    score += 1000.0 * best;
                }
            }
            let mut exact_or_prefix = 0.0;
            let mut matched = 0usize;
            for (token, w) in wanted {
                let tier = |label: &str| {
                    if label == token.as_str() {
                        3
                    } else if label.starts_with(token.as_str()) {
                        2
                    } else if label.contains(token.as_str()) {
                        1
                    } else {
                        0
                    }
                };
                match tier(&x.name).max(tier(&x.qual)) {
                    3 => {
                        exact_or_prefix += 1000.0 * w;
                        matched += 1;
                    }
                    2 => {
                        exact_or_prefix += 100.0 * w;
                        matched += 1;
                    }
                    1 => {
                        score += w;
                        matched += 1;
                    }
                    _ => {}
                }
                if path.contains(token.as_str()) {
                    score += 0.5 * w;
                }
                if x.doc.contains(token.as_str()) {
                    score += 0.75 * w;
                }
            }
            let share = matched as f64 / wanted.len() as f64;
            score + exact_or_prefix * share * share
        };

        let mut ranked: Vec<(f64, usize)> =
            (0..total).map(|id| (score_of(id, &weights), id)).filter(|&(s, _)| s > 0.0).collect();
        ranked.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then(self.nodes[a.1].name.len().cmp(&self.nodes[b.1].name.len()))
                .then(a.1.cmp(&b.1))
        });
        let mut seeds: Vec<u32> = Vec::new();
        if let Some(&(top, _)) = ranked.first() {
            let mut labels = HashSet::new();
            for &(score, id) in &ranked {
                if seeds.len() >= RANKED_SEEDS || score < SEED_SHARE * top {
                    break;
                }
                if labels.insert(texts[id].name.as_str()) {
                    seeds.push(id as u32);
                }
            }
        }
        let mut alphabetical: Vec<&(String, f64)> = weights.iter().collect();
        alphabetical.sort_by(|a, b| a.0.cmp(&b.0));
        let only_relations = tokens.iter().all(|t| RELATION_WORDS.contains(&t.as_str()));
        for word in alphabetical {
            if RELATION_WORDS.contains(&word.0.as_str()) && !only_relations {
                continue;
            }
            let owned = [(word.0.clone(), word.1)];
            let best = (0..total)
                .map(|id| (score_of(id, &owned), id))
                .filter(|&(s, _)| s > 0.0)
                .min_by(|a, b| {
                    b.0.total_cmp(&a.0)
                        .then(self.degree(b.1 as u32).cmp(&self.degree(a.1 as u32)))
                        .then(self.nodes[a.1].name.len().cmp(&self.nodes[b.1].name.len()))
                        .then(a.1.cmp(&b.1))
                });
            if let Some((_, id)) = best {
                if !seeds.contains(&(id as u32)) {
                    seeds.push(id as u32);
                }
            }
        }
        if seeds.is_empty() {
            return QueryAnswer::default();
        }

        let mut hops = vec![NONE; total];
        let mut queue = VecDeque::new();
        for &s in &seeds {
            hops[s as usize] = 0;
            queue.push_back(s);
        }
        while let Some(current) = queue.pop_front() {
            let here = hops[current as usize];
            let is_seed = seeds.contains(&current);
            if here >= WALK_DEPTH || (!is_seed && self.degree(current) >= self.tables.hub_cut) {
                continue;
            }
            for &ei in self.tables.und.row(current) {
                let edge = &self.edges[ei as usize];
                let other = if edge.from == current { edge.to } else { edge.from };
                if hops[other as usize] == NONE {
                    hops[other as usize] = here + 1;
                    queue.push_back(other);
                }
            }
        }
        let mut reached: Vec<u32> = (0..total as u32).filter(|&n| hops[n as usize] != NONE).collect();
        let found = reached.len();
        reached.sort_unstable_by_key(|&n| (hops[n as usize], std::cmp::Reverse(self.degree(n)), n));

        let mut kept = vec![false; total];
        let mut order: Vec<(u32, u32)> = Vec::new();
        let mut text = String::new();
        for &node in &reached {
            let mark = text.len();
            let n = &self.nodes[node as usize];
            text.push_str(&format!("{} {}:{}-{}\n", n.name, self.path_of(node), n.start, n.end));
            // Edges between this node and the nodes kept so far, each counted once.
            let leaving = self.tables.out.row(node).iter().filter(|&&ei| {
                let to = self.edges[ei as usize].to;
                to == node || kept[to as usize]
            });
            let arriving = self.tables.inc.row(node).iter().filter(|&&ei| {
                let from = self.edges[ei as usize].from;
                from != node && kept[from as usize]
            });
            for &ei in leaving.chain(arriving) {
                let edge = &self.edges[ei as usize];
                let a = &self.nodes[edge.from as usize].name;
                let b = &self.nodes[edge.to as usize].name;
                text.push_str(&format!("{} {} {}\n", a, edge.rel.word(), b));
            }
            if est_tokens(&text) > budget_tokens && !seeds.contains(&node) {
                text.truncate(mark);
                break;
            }
            kept[node as usize] = true;
            order.push((node, hops[node as usize]));
        }
        let truncated = order.len() < found;
        let mut edges: Vec<usize> = Vec::new();
        for &(node, _) in &order {
            for &ei in self.tables.out.row(node) {
                if kept[self.edges[ei as usize].to as usize] {
                    edges.push(ei as usize);
                }
            }
        }
        edges.sort_unstable();
        QueryAnswer { seeds, nodes: order, edges, found, truncated }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use lxr_core::Lang;

    #[test]
    fn terms_drop_stop_words_and_short_words_but_never_all_of_them() {
        assert_eq!(query_terms("How does the parser work?"), ["parser"]);
        assert_eq!(query_terms("is it on"), ["is", "it", "on"]);
        assert_eq!(query_terms("ñu parse"), ["ñu", "parse"]);
        assert_eq!(query_terms("Parse_Config, load!"), ["parse_config", "load"]);
        assert!(query_terms("???").is_empty());
    }

    #[test]
    fn tokens_are_the_distinct_pieces_of_the_terms() {
        let terms: Vec<String> = ["parse_config", "parse", "snake_case2"].iter().map(|s| s.to_string()).collect();
        assert_eq!(query_tokens(&terms), ["parse", "config", "snake", "case2"]);
        assert_eq!(joined_pieces("parse_config.v2"), "parseconfigv2");
    }

    #[test]
    fn a_question_picks_the_exact_name_the_best_word_matches_and_walks_two_steps() {
        let g = Graph::build(&query_project());
        let ids = |names: &[&str]| -> Vec<u32> { names.iter().map(|n| one(&g, n)).collect() };
        let answer = g.query("parse config", 10_000);
        // parse_config joins both words, Config is the best match for "config",
        // and parse_args ties parse_config for "parse" but is shorter and as well connected
        assert_eq!(answer.seeds, ids(&["Config.parse_config", "Config", "parse_args"]));
        let order: Vec<(u32, u32)> = answer.nodes.clone();
        let expected: Vec<(u32, u32)> = vec![
            (one(&g, "Config"), 0),
            (one(&g, "Config.parse_config"), 0),
            (one(&g, "parse_args"), 0),
            (one(&g, "start_server"), 1),
            (one(&g, "server.py"), 1),
            (one(&g, "Config.load"), 1),
            (one(&g, "config.py"), 1),
            (one(&g, "render_page"), 2),
        ];
        assert_eq!(order, expected);
        assert_eq!(answer.found, 8);
        assert!(!answer.truncated);
        // eleven edges join those eight nodes, in edge order, and none leaves the set
        assert_eq!(answer.edges.len(), 11);
        assert!(answer.edges.windows(2).all(|w| w[0] < w[1]));
        let kept: Vec<u32> = order.iter().map(|x| x.0).collect();
        for &e in &answer.edges {
            assert!(kept.contains(&g.edges[e].from) && kept.contains(&g.edges[e].to));
        }
        // render_header and render.py are three steps away and never found
        assert!(!kept.contains(&one(&g, "render_header")));
        assert!(!kept.contains(&one(&g, "render.py")));
    }

    #[test]
    fn the_budget_keeps_a_prefix_of_the_order_and_never_drops_a_seed() {
        let g = Graph::build(&query_project());
        // Rows are "name path:first-last" plus one short row for each edge to a kept node.
        // The three seeds take 104 characters, 26 tokens; start_server adds 84, 47 tokens in all;
        // server.py adds 87 more, 69 tokens in all.
        let count = |budget: usize| g.query("parse config", budget).nodes.len();
        assert_eq!(count(0), 3);
        assert_eq!(count(1), 3);
        assert_eq!(count(26), 3);
        assert_eq!(count(46), 3);
        assert_eq!(count(47), 4);
        assert_eq!(count(68), 4);
        assert_eq!(count(69), 5);
        let small = g.query("parse config", 46);
        assert!(small.truncated);
        assert_eq!(small.found, 8);
        assert_eq!(small.seeds.len(), 3);
        // the edges are those among the nodes kept
        let kept: Vec<u32> = small.nodes.iter().map(|x| x.0).collect();
        assert_eq!(small.edges.len(), 1);
        for &e in &small.edges {
            assert!(kept.contains(&g.edges[e].from) && kept.contains(&g.edges[e].to));
        }
        assert!(!g.query("parse config", 10_000).truncated);
    }

    #[test]
    fn words_that_match_nothing_give_an_empty_answer() {
        let g = Graph::build(&query_project());
        assert_eq!(g.query("zzzz qqqq", 100), QueryAnswer::default());
        assert_eq!(g.query("what is it", 100), QueryAnswer::default());
        assert_eq!(g.query("", 100), QueryAnswer::default());
        assert_eq!(g.query("   ", 100), QueryAnswer::default());
    }

    #[test]
    fn a_word_about_relations_gets_no_seed_of_its_own_unless_it_is_alone() {
        let mut f = Fx::new("x.py", Lang::Python);
        f.func("import_data", 1, 3);
        f.func("load", 5, 7);
        let g = Graph::build(&[f.done()]);
        let (data, load) = (one(&g, "import_data"), one(&g, "load"));
        assert_eq!(g.query("import load", 1000).seeds, [load]);
        assert_eq!(g.query("import", 1000).seeds, [data]);
    }

    #[test]
    fn a_busy_node_is_visited_but_not_expanded() {
        let mut f = Fx::new("big.py", Lang::Python);
        for i in 0..60 {
            f.func(&format!("func{i:02}"), i + 1, i + 1);
        }
        let g = Graph::build(&[f.done()]);
        let answer = g.query("func07", 1000);
        // the file node has 60 neighbours, so the other 59 functions are not walked to
        assert_eq!(answer.seeds, [one(&g, "func07")]);
        assert_eq!(answer.found, 2);
        assert_eq!(answer.nodes.len(), 2);
        assert_eq!(answer.nodes[1], (0, 1));
    }

    #[test]
    fn the_same_question_always_gets_the_same_answer() {
        let g = Graph::build(&py_project());
        let first = g.query("user save update", 300);
        assert!(!first.seeds.is_empty());
        for _ in 0..3 {
            assert_eq!(g.query("user save update", 300), first);
        }
        assert_eq!(Graph::build(&py_project()).query("user save update", 300), first);
    }
}
