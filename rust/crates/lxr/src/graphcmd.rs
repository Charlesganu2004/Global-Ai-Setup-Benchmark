//! The graph commands: def, callers, about, affected, path, hubs, explain, query, map.
//! Each one takes the loaded graph and appends its answer to a string.

use crate::rows::{at_lines, caller_row, def_row, name_list, ref_row, span_row};
use lxr_core::est_tokens;
use lxr_graph::{Graph, Link, NodeKind, QueryAnswer, Relation};
use std::collections::HashMap;

/// Most members of a container that `explain` lists.
const MEMBERS_SHOWN: usize = 20;
/// Most things a function calls that `about` names and `explain` lists.
const CALLS_NAMED: usize = 12;
const CALLS_LISTED: usize = 20;

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

/// `N callers of NAME:` and one row per caller.
fn callers_block(graph: &Graph, name: &str, node: u32, out: &mut String) {
    let links = graph.callers(node);
    match links.len() {
        0 => ln!(out, "0 callers of {name}"),
        1 => ln!(out, "1 caller of {name}:"),
        n => ln!(out, "{n} callers of {name}:"),
    }
    for link in &links {
        ln!(out, "{}", caller_row(graph, link));
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
    let links = graph.callers(node);
    match links.len() {
        0 => ln!(out, "  0 callers"),
        1 => ln!(out, "  1 caller:"),
        n => ln!(out, "  {n} callers:"),
    }
    for link in &links {
        ln!(out, "    {}", caller_row(graph, link));
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
    let mut best: Option<Vec<lxr_graph::Hop>> = None;
    for &a in starts.iter().take(4) {
        for &b in ends.iter().take(4) {
            if let Some(hops) = graph.shortest_path(a, b) {
                if best.as_ref().map_or(true, |kept| hops.len() < kept.len()) {
                    best = Some(hops);
                }
            }
        }
    }
    let hops = match best {
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
