//! `pgr find`: hybrid code search over the search index.

use crate::cli::FindArgs;
use crate::ctx::{Ctx, Engine};
use crate::fail::Res;
use crate::rows::{call_tail, cut_row, looks_callable};
use crate::searchenv;
use lxr_core::SymKind;
use lxr_graph::{Graph, NodeKind};
use lxr_rag::{is_test_path, Hit, Meaning, SearchOpts};
use lxr_text::code_tokens;
use std::collections::HashSet;

/// A definition of at most this many lines, with the few definitions it calls, is a thin
/// wrapper: it stands for what it calls.
const WRAPPER_LINES: u32 = 8;
/// Most definitions a thin wrapper may call. A function that calls more is work of its own.
const WRAPPER_CALLS: usize = 2;
/// Most definitions that follow one hit when `PGR_SEARCH_GRAPH` is on.
const ADDED_PER_HIT: usize = 3;
/// What an added definition scores, as a share of the hit it follows.
const ADDED_SCORE: f32 = 0.5;

/// Runs a search. The caller has made the index current and checked the engine.
pub fn hits(ctx: &Ctx, args: &FindArgs) -> Res<Vec<Hit>> {
    let tuning = searchenv::tuning()?;
    let index = ctx.search_index()?;
    // The model is opened only when the index holds vectors that a query can use.
    let embedder = if index.has_vectors() && tuning.meaning != Meaning::Repo { ctx.embed.get() } else { None };
    let opts = SearchOpts { limit: args.k, mode: args.mode, alpha: None };
    let found = index.search_tuned(&args.query, embedder, &opts, &tuning);
    if searchenv::graph_on()? && ctx.engines.is_on(Engine::Graph) {
        if let Ok(graph) = ctx.graph() {
            return Ok(through_graph(graph, found, &args.query, args.k));
        }
    }
    Ok(found)
}

/// True when a definition is test code: it lies in a test file or folder, or it or a container
/// of it is named `test...` (a test function, a `Test...` class, a `tests` module).
fn looks_like_test(path: &str, qual: &str) -> bool {
    is_test_path(path) || qual.split('.').any(|part| part.to_ascii_lowercase().starts_with("test"))
}

/// The definition of the graph that a hit stands for.
fn definition_of(graph: &Graph, hit: &Hit) -> Option<u32> {
    if hit.symbol.is_empty() {
        return None;
    }
    graph.find(&hit.symbol).into_iter().find(|&n| graph.nodes[n as usize].start == hit.start && graph.path_of(n) == hit.path)
}

/// Follows hits that are tests, or thin wrappers, with the definitions they call: a test is
/// about the function it tests, and a wrapper is a way in to what it wraps. The added
/// definitions come right below their hit, at most three each, the ones whose names share the
/// most words with the query first, then the ones fewer places call. A definition that is
/// a hit already stays where it is. The list is cut to `limit` again, so the added
/// definitions take the place of the lowest hits.
pub fn through_graph(graph: &Graph, hits: Vec<Hit>, query: &str, limit: usize) -> Vec<Hit> {
    let words: HashSet<String> = code_tokens(query).into_iter().collect();
    let mut taken: HashSet<(String, u32, u32)> = hits.iter().map(|h| (h.path.clone(), h.start, h.end)).collect();
    let mut out = Vec::with_capacity(hits.len());
    for hit in hits {
        let added = definition_of(graph, &hit).map(|n| added_below(graph, n, &hit, &words, &mut taken)).unwrap_or_default();
        out.push(hit);
        out.extend(added);
    }
    out.truncate(limit);
    out
}

/// The definitions to put below a hit on definition `node`; none unless it is a test or a thin wrapper.
fn added_below(graph: &Graph, node: u32, hit: &Hit, words: &HashSet<String>, taken: &mut HashSet<(String, u32, u32)>) -> Vec<Hit> {
    let def = &graph.nodes[node as usize];
    let calls: Vec<u32> = graph
        .callees(node)
        .into_iter()
        .map(|link| link.node)
        .filter(|&n| {
            let callee = &graph.nodes[n as usize];
            matches!(callee.kind, NodeKind::Symbol(_)) && !looks_like_test(graph.path_of(n), &callee.qual)
        })
        .collect();
    let is_test = looks_like_test(&hit.path, &hit.symbol);
    let callable = matches!(def.kind, NodeKind::Symbol(SymKind::Function | SymKind::Method));
    let wrapper = callable && def.end.saturating_sub(def.start) < WRAPPER_LINES && (1..=WRAPPER_CALLS).contains(&calls.len());
    if !is_test && !wrapper {
        return Vec::new();
    }
    let mut ranked: Vec<(usize, usize, u32)> = calls
        .iter()
        .map(|&n| {
            let mut own = code_tokens(&graph.nodes[n as usize].name);
            own.sort_unstable();
            own.dedup();
            (own.iter().filter(|t| words.contains(*t)).count(), graph.callers(n).len(), n)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut added = Vec::new();
    for (_, _, n) in ranked {
        if added.len() >= ADDED_PER_HIT {
            break;
        }
        let callee = &graph.nodes[n as usize];
        let span = (graph.path_of(n).to_string(), callee.start, callee.end);
        if !taken.insert(span) {
            continue;
        }
        added.push(Hit {
            path: graph.path_of(n).to_string(),
            start: callee.start,
            end: callee.end,
            symbol: callee.qual.clone(),
            sig: callee.sig.clone(),
            doc: callee.doc.clone(),
            score: hit.score * ADDED_SCORE,
            chunk_start: callee.start,
            chunk_end: callee.end,
        });
    }
    added
}

/// The definition row of a hit, or `PATH:FIRST-LAST` for a hit outside any definition.
pub fn hit_row(hit: &Hit) -> String {
    if hit.symbol.is_empty() {
        return format!("{}:{}-{}", hit.path, hit.start, hit.end);
    }
    let mut row = format!("{} {}:{}-{}", hit.symbol, hit.path, hit.start, hit.end);
    let sig = if looks_callable(&hit.sig) { call_tail(&hit.sig) } else { hit.sig.as_str() };
    if !sig.is_empty() {
        row.push(' ');
        row.push_str(sig);
    }
    cut_row(row)
}

/// The lines of one hit: its row, then its documentation line when it has one.
pub fn hit_lines(hit: &Hit) -> Vec<String> {
    let mut lines = vec![hit_row(hit)];
    if !hit.doc.is_empty() {
        lines.push(format!("  {}", hit.doc));
    }
    lines
}

/// `pgr find WORDS...`.
pub fn run(ctx: &Ctx, args: &FindArgs, out: &mut String) -> Res<()> {
    let found = hits(ctx, args)?;
    if found.is_empty() {
        ln!(out, "none");
    }
    for hit in &found {
        for line in hit_lines(hit) {
            ln!(out, "{line}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lxr_core::{hash64, Call, FileFacts, Lang, ParseGrade, Symbol};

    fn symbol(name: &str, kind: SymKind, start: u32, end: u32) -> Symbol {
        Symbol {
            name: name.to_string(),
            sig: format!("def {name}()"),
            qual: name.to_string(),
            kind,
            start,
            end,
            doc: format!("About {name}."),
            parent: None,
            bases: Vec::new(),
            hash: hash64(format!("{name}:{start}").as_bytes()),
        }
    }

    fn file(path: &str, symbols: Vec<Symbol>, calls: &[(usize, &str)]) -> FileFacts {
        FileFacts {
            path: path.to_string(),
            lang: Lang::Python,
            lines: 100,
            bytes: 2000,
            hash: hash64(path.as_bytes()),
            grade: ParseGrade::Ast,
            symbols,
            calls: calls
                .iter()
                .enumerate()
                .map(|(i, (caller, name))| Call { caller: Some(*caller as u32), name: name.to_string(), recv: String::new(), line: 10 + i as u32 })
                .collect(),
            imports: Vec::new(),
        }
    }

    /// src/app.py: parse_config (10 lines), load_settings (a 3-line wrapper of parse_config),
    /// build_report (30 lines, calls three things), render_table, format_cell, helper.
    /// tests/test_app.py: test_parse_config_defaults calls parse_config and helper.
    fn graph() -> Graph {
        let app = file(
            "src/app.py",
            vec![
                symbol("parse_config", SymKind::Function, 1, 10),
                symbol("load_settings", SymKind::Function, 12, 14),
                symbol("build_report", SymKind::Function, 20, 50),
                symbol("render_table", SymKind::Function, 52, 60),
                symbol("format_cell", SymKind::Function, 62, 70),
                symbol("helper", SymKind::Function, 72, 80),
            ],
            &[(1, "parse_config"), (2, "render_table"), (2, "format_cell"), (2, "helper")],
        );
        let tests = file(
            "tests/test_app.py",
            vec![symbol("test_parse_config_defaults", SymKind::Function, 1, 9)],
            &[(0, "parse_config"), (0, "helper")],
        );
        Graph::build(&[app, tests])
    }

    fn hit(path: &str, symbol: &str, start: u32, end: u32, score: f32) -> Hit {
        Hit { path: path.to_string(), start, end, symbol: symbol.to_string(), sig: String::new(), doc: String::new(), score, chunk_start: start, chunk_end: end }
    }

    fn names(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.symbol.as_str()).collect()
    }

    #[test]
    fn tests_are_told_by_path_and_by_name() {
        assert!(looks_like_test("tests/app.py", "run"));
        assert!(looks_like_test("src/app.py", "test_run"));
        assert!(looks_like_test("src/app.rs", "tests.run"));
        assert!(looks_like_test("src/app.py", "TestApp.run"));
        assert!(!looks_like_test("src/app.py", "App.run"));
        assert!(!looks_like_test("src/latest.py", "latest_value"));
    }

    #[test]
    fn a_test_hit_is_followed_by_what_it_calls_and_a_plain_hit_is_not() {
        let graph = graph();
        let test = hit("tests/test_app.py", "test_parse_config_defaults", 1, 9, 1.0);
        let plain = hit("src/app.py", "build_report", 20, 50, 0.5);
        let found = through_graph(&graph, vec![test, plain], "parse config defaults", 10);
        // The test calls parse_config and helper; the words of the query name parse_config, so
        // it comes first. build_report is long and calls three things: no wrapper, nothing added.
        assert_eq!(names(&found), vec!["test_parse_config_defaults", "parse_config", "helper", "build_report"]);
        assert_eq!(found[1].path, "src/app.py");
        assert_eq!((found[1].start, found[1].end), (1, 10));
        assert_eq!(found[1].score, 0.5);
        assert_eq!(found[1].doc, "About parse_config.");
    }

    #[test]
    fn a_thin_wrapper_is_followed_by_the_definition_it_wraps() {
        let graph = graph();
        let wrapper = hit("src/app.py", "load_settings", 12, 14, 0.8);
        let found = through_graph(&graph, vec![wrapper], "load the settings", 10);
        assert_eq!(names(&found), vec!["load_settings", "parse_config"]);
        // A definition that calls three things is not a wrapper, and neither is a long one.
        let found = through_graph(&graph, vec![hit("src/app.py", "build_report", 20, 50, 0.8)], "report", 10);
        assert_eq!(names(&found), vec!["build_report"]);
    }

    #[test]
    fn nothing_is_added_twice_and_the_list_keeps_its_length() {
        let graph = graph();
        // parse_config is a hit already, below the wrapper: it stays where it is.
        let hits = vec![
            hit("src/app.py", "load_settings", 12, 14, 0.9),
            hit("src/app.py", "build_report", 20, 50, 0.8),
            hit("src/app.py", "parse_config", 1, 10, 0.7),
        ];
        let found = through_graph(&graph, hits.clone(), "settings", 10);
        assert_eq!(names(&found), vec!["load_settings", "build_report", "parse_config"]);
        // Added definitions push the lowest hits out of a list of this length.
        let test = hit("tests/test_app.py", "test_parse_config_defaults", 1, 9, 1.0);
        let found = through_graph(&graph, vec![test, hits[1].clone()], "parse config", 2);
        assert_eq!(names(&found), vec!["test_parse_config_defaults", "parse_config"]);
    }

    #[test]
    fn a_hit_outside_any_definition_or_unknown_to_the_graph_is_left_alone() {
        let graph = graph();
        let module = hit("src/app.py", "", 1, 5, 1.0);
        let unknown = hit("src/gone.py", "vanished", 1, 5, 0.5);
        let found = through_graph(&graph, vec![module.clone(), unknown.clone()], "anything", 5);
        assert_eq!(found, vec![module, unknown]);
        assert!(through_graph(&graph, Vec::new(), "anything", 5).is_empty());
    }
}
