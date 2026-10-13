//! The graph commands: def, callers, about, affected, path, hubs, explain, query, map.
//! Each one takes the loaded graph and appends its answer to a string.

use crate::rows::{at_lines, caller_row, def_row, name_list, ref_row, span_row};
use lxr_core::est_tokens;
use lxr_graph::{Confidence, Graph, Hop, Link, NodeKind, QueryAnswer, Relation};
use std::collections::{HashMap, VecDeque};

/// Most members of a container that `explain` lists.
const MEMBERS_SHOWN: usize = 20;
/// Most things a function calls that `about` names and `explain` lists.
const CALLS_NAMED: usize = 12;
const CALLS_LISTED: usize = 20;
/// The end of the row of a function that is its own caller.
const CALLS_ITSELF: &str = " (calls itself)";

/// The block header `# NAME at PATH:FIRST` that tells apart several definitions of one name.
fn which(graph: &Graph, name: &str, node: u32) -> String {
    format!("# {} at {}:{}", name, graph.path_of(node), graph.nodes[node as usize].start)
}

/// `pgr def NAME...`: one row per definition.
pub fn def(graph: &Graph, names: &[String], out: &mut String) {
    for name in names {
        let found = graph.find(name);
        if found.is_empty() {
            ln!(out, "{name}: not found");
        }
        for id in found {
            ln!(out, "{}", def_row(graph, id));
        }
    }
}

/// The call of a function to itself, from the graph's edge list. `Graph::callers` leaves
/// this link out; the edges are sorted by (from, to, relation), so it is found by search.
fn self_call(graph: &Graph, node: u32) -> Option<Link> {
    let first = graph.edges.partition_point(|e| (e.from, e.to) < (node, node));
    graph.edges[first..]
        .iter()
        .take_while(|e| e.from == node && e.to == node)
        .find(|e| e.rel == Relation::Calls)
        .map(|e| Link { node, conf: e.conf, lines: e.lines.clone() })
}

/// The caller rows of a definition, in the order of (path, first line): the callers the graph
/// gives, and the definition itself, with `(calls itself)` at the end of its row, when it
/// calls itself.
fn caller_rows(graph: &Graph, node: u32) -> Vec<String> {
    let links = graph.callers(node);
    let mut rows: Vec<String> = links.iter().map(|link| caller_row(graph, link)).collect();
    if let Some(own) = self_call(graph, node) {
        let place = |n: u32| (graph.path_of(n), graph.nodes[n as usize].start);
        let at = links.partition_point(|link| place(link.node) <= place(node));
        rows.insert(at, format!("{}{CALLS_ITSELF}", caller_row(graph, &own)));
    }
    rows
}

/// `N callers of NAME:` and one row per caller.
fn callers_block(graph: &Graph, name: &str, node: u32, out: &mut String) {
    let rows = caller_rows(graph, node);
    match rows.len() {
        0 => ln!(out, "0 callers of {name}"),
        1 => ln!(out, "1 caller of {name}:"),
        n => ln!(out, "{n} callers of {name}:"),
    }
    for row in &rows {
        ln!(out, "{row}");
    }
}

/// The calls of a bare name that no rule could tie to a definition.
fn loose_callers(graph: &Graph, found: &[u32]) -> Vec<Link> {
    match found.first().map(|&id| &graph.nodes[id as usize]) {
        Some(node) if matches!(node.kind, NodeKind::Symbol(_)) => graph.loose_callers(&node.name),
        _ => Vec::new(),
    }
}

/// `pgr callers NAME...`.
pub fn callers(graph: &Graph, names: &[String], out: &mut String) {
    for name in names {
        let found = graph.find(name);
        if found.is_empty() {
            ln!(out, "{name}: not found");
            continue;
        }
        for &id in &found {
            if found.len() > 1 {
                ln!(out, "{}", which(graph, name, id));
            }
            callers_block(graph, name, id, out);
        }
        let loose = loose_callers(graph, &found);
        if !loose.is_empty() {
            ln!(out, "by name only:");
            for link in &loose {
                ln!(out, "{}", caller_row(graph, link));
            }
        }
    }
}

/// The caller count and rows of `about` and `explain`, indented by two spaces.
fn callers_summary(graph: &Graph, node: u32, out: &mut String) {
    let rows = caller_rows(graph, node);
    match rows.len() {
        0 => ln!(out, "  0 callers"),
        1 => ln!(out, "  1 caller:"),
        n => ln!(out, "  {n} callers:"),
    }
    for row in &rows {
        ln!(out, "    {row}");
    }
}

/// The documentation line of a definition, when it has one.
fn doc_line(graph: &Graph, node: u32, out: &mut String) {
    let doc = &graph.nodes[node as usize].doc;
    if !doc.is_empty() {
        ln!(out, "  doc: {doc}");
    }
}

/// `pgr about NAME...`: the definition, its documentation, its callers and what it calls.
pub fn about(graph: &Graph, names: &[String], out: &mut String) {
    for name in names {
        let found = graph.find(name);
        if found.is_empty() {
            ln!(out, "{name}: not found");
        }
        for id in found {
            ln!(out, "{}", def_row(graph, id));
            doc_line(graph, id, out);
            callers_summary(graph, id, out);
            let called: Vec<String> = graph.callees(id).iter().map(|l| graph.nodes[l.node as usize].qual.clone()).collect();
            if !called.is_empty() {
                ln!(out, "  calls: {}", name_list(&called, CALLS_NAMED));
            }
        }
    }
}

/// The definition that holds `node`, when it sits inside another definition.
fn container_of(graph: &Graph, node: u32) -> Option<u32> {
    graph
        .edges
        .iter()
        .filter(|e| e.to == node && matches!(e.rel, Relation::Contains | Relation::Method))
        .map(|e| e.from)
        .find(|&from| matches!(graph.nodes.get(from as usize).map(|n| n.kind), Some(NodeKind::Symbol(_))))
}

/// The qualified names of some nodes.
fn quals(graph: &Graph, nodes: &[u32]) -> Vec<String> {
    nodes.iter().map(|&n| graph.nodes[n as usize].qual.clone()).collect()
}

/// `pgr explain NAME`: everything the graph knows about one definition.
pub fn explain(graph: &Graph, name: &str, out: &mut String) {
    let found = graph.find(name);
    if found.is_empty() {
        ln!(out, "{name}: not found");
    }
    for id in found {
        ln!(out, "{}", def_row(graph, id));
        doc_line(graph, id, out);
        if let Some(container) = container_of(graph, id) {
            ln!(out, "  in: {}", graph.nodes[container as usize].qual);
        }
        let members = graph.members(id);
        if !members.is_empty() {
            let names: Vec<String> = members.iter().map(|&m| graph.nodes[m as usize].name.clone()).collect();
            ln!(out, "  members: {}", name_list(&names, MEMBERS_SHOWN));
        }
        let bases = graph.bases(id);
        if !bases.is_empty() {
            ln!(out, "  bases: {}", name_list(&quals(graph, &bases), MEMBERS_SHOWN));
        }
        let subclasses = graph.subclasses(id);
        if !subclasses.is_empty() {
            ln!(out, "  subclasses: {}", name_list(&quals(graph, &subclasses), MEMBERS_SHOWN));
        }
        callers_summary(graph, id, out);
        let callees = graph.callees(id);
        if !callees.is_empty() {
            ln!(out, "  calls:");
            for link in callees.iter().take(CALLS_LISTED) {
                ln!(out, "    {}", def_row(graph, link.node));
            }
            if callees.len() > CALLS_LISTED {
                ln!(out, "    +{}", callees.len() - CALLS_LISTED);
            }
        }
    }
}

/// `pgr affected NAME [--depth N]`: what a change to the definition can reach.
pub fn affected(graph: &Graph, name: &str, depth: u32, out: &mut String) {
    let found = graph.find(name);
    if found.is_empty() {
        ln!(out, "{name}: not found");
    }
    for &id in &found {
        if found.len() > 1 {
            ln!(out, "{}", which(graph, name, id));
        }
        let reached = graph.affected(id, depth);
        if reached.is_empty() {
            ln!(out, "0 affected by {name}");
            continue;
        }
        ln!(out, "{} affected by {name} (depth {depth}):", reached.len());
        for hit in &reached {
            let line = if hit.line > 0 { format!(" @{}", hit.line) } else { String::new() };
            ln!(out, "  d{} {} via {}{}", hit.depth, span_row(graph, hit.node), hit.via.word(), line);
        }
    }
}

/// The shortest chain of links from any of `starts` to any of `ends`. One start and one end is
/// the graph's own search; several are searched together, whatever their number.
fn shortest_between(graph: &Graph, starts: &[u32], ends: &[u32]) -> Option<Vec<Hop>> {
    match (starts, ends) {
        (&[a], &[b]) => graph.shortest_path(a, b),
        _ => shortest_from_any(graph, starts, ends),
    }
}

/// The shortest chain of links from any of `starts` to any of `ends`, taking every link as
/// undirected, in one search that begins at all the starts. It walks the graph as
/// `Graph::shortest_path` does (neighbours by node number, then by edge order), so for one
/// start and one end the two give the same chain. A node that is both a start and an end is a
/// chain of no links. `None` when no start is connected to any end.
fn shortest_from_any(graph: &Graph, starts: &[u32], ends: &[u32]) -> Option<Vec<Hop>> {
    let total = graph.nodes.len();
    let mut is_start = vec![false; total];
    let mut is_end = vec![false; total];
    for &s in starts.iter().filter(|&&s| (s as usize) < total) {
        is_start[s as usize] = true;
    }
    for &e in ends.iter().filter(|&&e| (e as usize) < total) {
        is_end[e as usize] = true;
    }
    let first = |node: u32| Hop { node, rel: Relation::Contains, forward: true, conf: Confidence::Extracted };
    if let Some(node) = (0..total).find(|&n| is_start[n] && is_end[n]) {
        return Some(vec![first(node as u32)]);
    }
    // For every node, the edges that touch it as (other end, edge number), the nearest
    // other end first. A link from a node to itself leads nowhere.
    let mut around: Vec<Vec<(u32, u32)>> = vec![Vec::new(); total];
    for (index, edge) in graph.edges.iter().enumerate() {
        if edge.from == edge.to || edge.from as usize >= total || edge.to as usize >= total {
            continue;
        }
        around[edge.from as usize].push((edge.to, index as u32));
        around[edge.to as usize].push((edge.from, index as u32));
    }
    for row in around.iter_mut() {
        row.sort_unstable();
    }
    let mut reached_by: Vec<Option<(u32, u32)>> = vec![None; total];
    let mut seen = is_start.clone();
    let mut queue: VecDeque<u32> = (0..total as u32).filter(|&n| is_start[n as usize]).collect();
    let mut found = None;
    'search: while let Some(current) = queue.pop_front() {
        for &(other, index) in &around[current as usize] {
            if seen[other as usize] {
                continue;
            }
            seen[other as usize] = true;
            reached_by[other as usize] = Some((current, index));
            if is_end[other as usize] {
                found = Some(other);
                break 'search;
            }
            queue.push_back(other);
        }
    }
    let mut at = found?;
    let mut hops = Vec::new();
    while let Some((before, index)) = reached_by[at as usize] {
        let edge = &graph.edges[index as usize];
        hops.push(Hop { node: at, rel: edge.rel, forward: edge.from == before, conf: edge.conf });
        at = before;
    }
    hops.push(first(at));
    hops.reverse();
    Some(hops)
}

/// `pgr path A B`: the shortest chain of links between two definitions.
pub fn path(graph: &Graph, from: &str, to: &str, out: &mut String) {
    let starts = graph.find(from);
    let ends = graph.find(to);
    if starts.is_empty() || ends.is_empty() {
        if starts.is_empty() {
            ln!(out, "{from}: not found");
        }
        if ends.is_empty() {
            ln!(out, "{to}: not found");
        }
        return;
    }
    let hops = match shortest_between(graph, &starts, &ends) {
        Some(hops) => hops,
        None => {
            ln!(out, "no path between {from} and {to}");
            return;
        }
    };
    ln!(out, "{from} -> {to}: {} hops", hops.len().saturating_sub(1));
    for pair in hops.windows(2) {
        let (before, here) = (pair[0], pair[1]);
        let word = here.rel.word();
        if here.forward {
            ln!(out, "  {} --{word}--> {}", ref_row(graph, before.node), ref_row(graph, here.node));
        } else {
            ln!(out, "  {} <--{word}-- {}", ref_row(graph, before.node), ref_row(graph, here.node));
        }
    }
}

/// `pgr hubs [-k N]`: the most connected definitions.
pub fn hubs(graph: &Graph, k: usize, out: &mut String) {
    let found = graph.hubs(k);
    if found.is_empty() {
        ln!(out, "none");
    }
    for (id, links) in found {
        ln!(out, "{}  {links} links", span_row(graph, id));
    }
}

/// The lines of a query answer: a header, one row per node, one row per link.
pub fn query_lines(graph: &Graph, answer: &QueryAnswer) -> Vec<String> {
    let seeds = quals(graph, &answer.seeds);
    let mut header = format!("query: {} nodes", answer.nodes.len());
    if !seeds.is_empty() && !answer.nodes.is_empty() {
        header.push_str(&format!(" from {}", seeds.join(", ")));
    }
    if answer.truncated {
        header.push_str(" (cut to budget)");
    }
    let mut lines = vec![header];
    for &(node, _) in &answer.nodes {
        lines.push(span_row(graph, node));
    }
    for &index in &answer.edges {
        if let Some(edge) = graph.edges.get(index) {
            let mut row = format!(
                "  {} --{}--> {}",
                graph.nodes[edge.from as usize].qual,
                edge.rel.word(),
                graph.nodes[edge.to as usize].qual
            );
            let lines_at = at_lines(&edge.lines);
            if !lines_at.is_empty() {
                row.push(' ');
                row.push_str(&lines_at);
            }
            lines.push(row);
        }
    }
    lines
}

/// `pgr query "QUESTION" [--budget N]`.
pub fn query(graph: &Graph, question: &str, budget: usize, out: &mut String) {
    let answer = graph.query(question, budget);
    for line in query_lines(graph, &answer) {
        ln!(out, "{line}");
    }
}

/// `pgr map [--budget N]`: the largest groups of the code.
pub fn map(graph: &Graph, budget: usize, out: &mut String) {
    let mut degree = vec![0u32; graph.nodes.len()];
    for edge in &graph.edges {
        if matches!(edge.rel, Relation::Calls | Relation::Inherits | Relation::Method | Relation::Contains) {
            if let Some(d) = degree.get_mut(edge.from as usize) {
                *d += 1;
            }
            if let Some(d) = degree.get_mut(edge.to as usize) {
                *d += 1;
            }
        }
    }
    let mut used = 0usize;
    let mut printed = 0usize;
    for (index, group) in graph.communities().iter().enumerate() {
        if group.len() < 2 {
            break;
        }
        let members: Vec<u32> = group.iter().copied().filter(|&m| (m as usize) < graph.nodes.len()).collect();
        let mut by_links = members.clone();
        by_links.sort_by(|&a, &b| degree[b as usize].cmp(&degree[a as usize]).then(a.cmp(&b)));
        let names: Vec<String> = by_links.iter().take(5).map(|&m| graph.nodes[m as usize].qual.clone()).collect();
        let mut per_file: HashMap<u32, usize> = HashMap::new();
        for &m in &members {
            *per_file.entry(graph.nodes[m as usize].file).or_insert(0) += 1;
        }
        let mut files: Vec<(u32, usize)> = per_file.into_iter().collect();
        files.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let paths: Vec<&str> = files.iter().take(3).filter_map(|(f, _)| graph.files.get(*f as usize)).map(String::as_str).collect();
        let line = format!("group {} ({} symbols): {}; files: {}", index + 1, members.len(), names.join(", "), paths.join(", "));
        used += est_tokens(&line) + 1;
        if printed > 0 && used > budget {
            break;
        }
        ln!(out, "{line}");
        printed += 1;
    }
    if printed == 0 {
        ln!(out, "none");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lxr_extract::{extract, lang_of};
    use std::path::Path;

    /// The graph of some Python files given as (path, text).
    fn graph_of(files: &[(&str, &str)]) -> Graph {
        let facts: Vec<_> = files.iter().map(|(path, text)| extract(path, lang_of(Path::new(path)).unwrap(), text)).collect();
        Graph::build(&facts)
    }

    fn project() -> Graph {
        graph_of(&[
            ("app/core.py", "def load(path):\n    return read(path)\n\n\ndef read(path):\n    return parse(path)\n\n\ndef parse(text):\n    return text\n"),
            ("app/cli.py", "from app.core import load\n\n\ndef main(argv):\n    return load(argv)\n\n\nclass Base:\n    def save(self):\n        return 1\n\n\nclass User(Base):\n    def update(self):\n        return self.save()\n"),
            ("app/rec.py", "def fact(n):\n    return n * fact(n - 1)\n\n\ndef run():\n    return fact(3) + fact(4)\n"),
        ])
    }

    #[test]
    fn a_search_from_one_node_to_one_node_gives_the_chain_of_the_graph() {
        let graph = project();
        let all: Vec<u32> = (0..graph.nodes.len() as u32).collect();
        let mut compared = 0;
        for &a in &all {
            for &b in &all {
                assert_eq!(shortest_from_any(&graph, &[a], &[b]), graph.shortest_path(a, b), "from {a} to {b}");
                compared += 1;
            }
        }
        assert!(compared > 100, "{compared}");
    }

    #[test]
    fn a_search_from_several_nodes_to_several_nodes_takes_the_shortest_pair() {
        let graph = project();
        let ids = |name: &str| graph.find(name);
        let (main, parse) = (ids("main"), ids("parse"));
        let (fact, run) = (ids("fact"), ids("run"));
        // Starting from a node that is far and a node that is near, the near one wins.
        let starts = [fact[0], main[0]];
        let hops = shortest_between(&graph, &starts, &parse).expect("main reaches parse");        let by_pairs = starts.iter().filter_map(|&s| graph.shortest_path(s, parse[0])).map(|h| h.len()).min().unwrap();
        assert_eq!(hops.len(), by_pairs);
        assert_eq!(hops[0].node, main[0]);
        assert_eq!(hops.last().unwrap().node, parse[0]);
        // Nodes that are not connected give no chain, and a node in both lists is a chain of none.
        assert!(shortest_between(&graph, &run, &parse).is_none());
        assert_eq!(shortest_between(&graph, &[main[0]], &[main[0], parse[0]]).unwrap().len(), 1);
        assert!(shortest_between(&graph, &[], &parse).is_none());
        assert!(shortest_between(&graph, &[9999], &parse).is_none());
    }

    #[test]
    fn a_function_that_calls_itself_is_found_in_the_edges() {
        let graph = project();
        let fact = graph.find("fact")[0];
        let run = graph.find("run")[0];
        let own = self_call(&graph, fact).expect("fact calls itself");
        assert_eq!((own.node, own.lines.clone()), (fact, vec![2]));
        assert!(self_call(&graph, run).is_none());
        assert!(self_call(&graph, 9999).is_none());
        assert!(graph.callers(fact).iter().all(|l| l.node != fact), "the graph leaves it out");
    }

    #[test]
    fn the_caller_rows_put_the_function_itself_in_the_order_of_its_place() {
        let graph = project();
        let fact = graph.find("fact")[0];
        let rows = caller_rows(&graph, fact);
        assert_eq!(rows, ["fact app/rec.py:1-2 @2 (calls itself)", "run app/rec.py:5-6 @6"].map(String::from).to_vec());
        let mut out = String::new();
        callers(&graph, &["fact".to_string()], &mut out);
        assert_eq!(out, "2 callers of fact:\nfact app/rec.py:1-2 @2 (calls itself)\nrun app/rec.py:5-6 @6\n");
    }
}
