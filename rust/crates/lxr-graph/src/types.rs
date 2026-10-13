//! The graph and its parts: nodes, edges, answers, and the tables that make questions cheap.

use lxr_core::SymKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::Path;

/// Marks "no node" in tables that hold node numbers.
pub(crate) const NONE: u32 = u32::MAX;

/// What a node stands for.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NodeKind {
    /// A whole source file.
    File,
    /// A definition inside a file.
    Symbol(SymKind),
}

/// One node of the graph: a file or a definition.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Node {
    /// A file or a kind of definition.
    pub kind: NodeKind,
    /// The bare name. For a file node, the file name.
    pub name: String,
    /// The name with its containers, joined by dots. For a file node, the repository path.
    pub qual: String,
    /// Index into [`Graph::files`] of the file the node lives in.
    pub file: u32,
    /// First line, counted from 1. A file node starts at 1.
    pub start: u32,
    /// Last line, inclusive. For a file node, the line count.
    pub end: u32,
    /// The declaration on one line. Empty for a file node.
    pub sig: String,
    /// First sentence of the documentation. Empty for a file node.
    pub doc: String,
    /// Hash of the definition's text (of the file's bytes for a file node).
    pub hash: u64,
}

/// How two nodes are related.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Relation {
    /// A file or definition holds another definition.
    Contains,
    /// A class-like definition holds a method.
    Method,
    /// A file imports another file.
    Imports,
    /// A function or file calls a definition.
    Calls,
    /// A class-like definition extends or implements another.
    Inherits,
}

impl Relation {
    /// Every relation, in the order edges are sorted.
    pub const ALL: [Relation; 5] =
        [Relation::Contains, Relation::Method, Relation::Imports, Relation::Calls, Relation::Inherits];

    /// The word printed for this relation.
    pub fn word(self) -> &'static str {
        match self {
            Relation::Contains => "contains",
            Relation::Method => "method",
            Relation::Imports => "imports",
            Relation::Calls => "calls",
            Relation::Inherits => "inherits",
        }
    }
}

/// How sure the graph is about an edge.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Confidence {
    /// A rule tied the edge to a definition through scope, import or class membership.
    Extracted,
    /// The edge rests on a name that is unique, or nearest, in the repository.
    Inferred,
}

/// One edge between two nodes.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Edge {
    /// The node the edge leaves: the caller, the importer, the subclass, the container.
    pub from: u32,
    /// The node the edge arrives at.
    pub to: u32,
    /// What kind of edge it is.
    pub rel: Relation,
    /// How sure the graph is. A `Calls` edge made of several calls is `Extracted` when any
    /// one of them was tied to the definition by scope, import or class membership.
    pub conf: Confidence,
    /// Where it happens, in the file of `from`: every call line (sorted, no repeats),
    /// the import line, or the class line of an inheritance. Empty for `Contains` and `Method`.
    pub lines: Vec<u32>,
}

/// A call that no rule could tie to a definition.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct LooseCall {
    /// The called name alone.
    pub name: String,
    /// The calling node: a symbol, or the file node for a module-level call.
    pub from: u32,
    /// Lines of the calls, sorted, no repeats.
    pub lines: Vec<u32>,
}

/// One side of a call: the node on the other end, with the evidence.
#[derive(Clone, PartialEq, Debug)]
pub struct Link {
    /// The node on the other end of the call.
    pub node: u32,
    /// How sure the graph is that the call reaches the definition.
    pub conf: Confidence,
    /// Lines of the calls, in the file of the calling node.
    pub lines: Vec<u32>,
}

/// A node that a change to another node can reach.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Affected {
    /// The node that is reached.
    pub node: u32,
    /// Steps from the start node, at least 1.
    pub depth: u32,
    /// The kind of edge by which the walk reached the node.
    pub via: Relation,
    /// The first line of that edge, in the file of `node`. 0 when the edge has none.
    pub line: u32,
}

/// One step of a path between two nodes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hop {
    /// The node reached at this step.
    pub node: u32,
    /// The relation of the edge that led here. Meaningless for the first hop (`Contains`).
    pub rel: Relation,
    /// True when that edge points along the path, from the previous node to this one.
    /// Meaningless for the first hop (`true`).
    pub forward: bool,
    /// The confidence of that edge.
    pub conf: Confidence,
}

/// The answer to a free-text question.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct QueryAnswer {
    /// The nodes the walk started from, in the order they were chosen.
    pub seeds: Vec<u32>,
    /// The nodes kept within the budget, each with its distance in hops from a seed.
    pub nodes: Vec<(u32, u32)>,
    /// Indexes into [`Graph::edges`] of the edges among the kept nodes.
    pub edges: Vec<usize>,
    /// How many nodes the walk reached before the budget cut anything.
    pub found: usize,
    /// True when the budget cut at least one node.
    pub truncated: bool,
}

/// Counts that describe a graph.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stats {
    /// Number of file nodes.
    pub files: usize,
    /// Number of definition nodes.
    pub symbols: usize,
    /// One entry per relation, in [`Relation::ALL`] order.
    pub edges: Vec<(Relation, usize)>,
    /// Calls tied to a definition by a rule that gives certainty.
    pub calls_extracted: usize,
    /// Calls tied to a definition by a unique or nearest name.
    pub calls_inferred: usize,
    /// Calls left without a definition.
    pub calls_loose: usize,
}

/// How many input calls ended in each outcome. Stored with the graph.
#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct CallTally {
    pub extracted: usize,
    pub inferred: usize,
    pub loose: usize,
}

/// Rows of numbers, one row per node, kept in two flat arrays.
#[derive(Default, Clone, Debug)]
pub(crate) struct Csr {
    start: Vec<u32>,
    items: Vec<u32>,
}

impl Csr {
    /// Groups `(row, item)` pairs by row, keeping the order of the pairs inside a row.
    fn from_pairs(rows: usize, pairs: &[(u32, u32)]) -> Csr {
        let mut start = vec![0u32; rows + 1];
        for &(row, _) in pairs {
            start[row as usize + 1] += 1;
        }
        for i in 0..rows {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut items = vec![0u32; pairs.len()];
        for &(row, item) in pairs {
            items[fill[row as usize] as usize] = item;
            fill[row as usize] += 1;
        }
        Csr { start, items }
    }

    /// The items of one row. Empty for a row that does not exist.
    pub(crate) fn row(&self, row: u32) -> &[u32] {
        let row = row as usize;
        if row + 1 >= self.start.len() {
            return &[];
        }
        &self.items[self.start[row] as usize..self.start[row + 1] as usize]
    }
}

/// Lookup tables derived from the stored fields. Rebuilt by [`Graph::rebuild`].
#[derive(Default, Clone, Debug)]
pub(crate) struct Tables {
    /// Edge numbers by `from`, in edge order.
    pub out: Csr,
    /// Edge numbers by `to`, in edge order.
    pub inc: Csr,
    /// Edge numbers by either end, loops left out, each row ordered by (other end, edge number).
    pub und: Csr,
    /// Distinct neighbours of each node over every edge.
    pub deg: Vec<u32>,
    /// A node with at least this many neighbours is a hub that a query walk does not expand.
    pub hub_cut: u32,
    /// Symbol nodes by name and by qualified name.
    pub by_name: HashMap<String, Vec<u32>>,
    /// Indexes into `loose` by called name.
    pub loose_by_name: HashMap<String, Vec<u32>>,
}

/// The code graph of one repository.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Graph {
    /// Repository paths, sorted. File node `i` stands for `files[i]`.
    pub files: Vec<String>,
    /// File nodes in path order, then symbols in (path, source order). A node id is an index here.
    pub nodes: Vec<Node>,
    /// Edges sorted by (from, to, relation).
    pub edges: Vec<Edge>,
    /// Calls that no rule could tie to a definition, sorted by (from, name).
    pub loose: Vec<LooseCall>,
    pub(crate) tally: CallTally,
    #[serde(skip)]
    pub(crate) tables: Tables,
}

impl PartialEq for Graph {
    fn eq(&self, other: &Graph) -> bool {
        self.files == other.files
            && self.nodes == other.nodes
            && self.edges == other.edges
            && self.loose == other.loose
            && self.tally == other.tally
    }
}

impl Graph {
    /// Builds the graph of a set of files. The result does not depend on the order of `files`.
    pub fn build(files: &[lxr_core::FileFacts]) -> Graph {
        crate::build::build_graph(files)
    }

    /// Writes the graph to `path`, replacing any file there in one step.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        lxr_core::save(path, self)
    }

    /// Reads a graph written by [`Graph::save`] and rebuilds its lookup tables.
    /// A file whose numbers point outside the graph is refused.
    pub fn load(path: &Path) -> io::Result<Graph> {
        let mut graph: Graph = lxr_core::load(path)?;
        graph.validate()?;
        graph.rebuild();
        Ok(graph)
    }

    /// Checks that every number stored in the graph points at something that exists.
    fn validate(&self) -> io::Result<()> {
        let nodes = self.nodes.len();
        let bad = |what: &str| Err(io::Error::new(io::ErrorKind::InvalidData, format!("graph {what} out of range")));
        if self.nodes.iter().any(|n| n.file as usize >= self.files.len()) {
            return bad("node file");
        }
        if self.edges.iter().any(|e| e.from as usize >= nodes || e.to as usize >= nodes) {
            return bad("edge end");
        }
        if self.loose.iter().any(|l| l.from as usize >= nodes) {
            return bad("loose call");
        }
        Ok(())
    }

    /// Rebuilds the lookup tables from the stored fields. `load` calls it; call it again
    /// after changing `nodes`, `edges` or `loose` by hand.
    pub fn rebuild(&mut self) {
        let n = self.nodes.len();
        let mut out = Vec::with_capacity(self.edges.len());
        let mut inc = Vec::with_capacity(self.edges.len());
        let mut und = Vec::with_capacity(self.edges.len() * 2);
        for (i, e) in self.edges.iter().enumerate() {
            if e.from as usize >= n || e.to as usize >= n {
                continue;
            }
            let i = i as u32;
            out.push((e.from, i));
            inc.push((e.to, i));
            if e.from != e.to {
                und.push((e.from, i));
                und.push((e.to, i));
            }
        }
        let out = Csr::from_pairs(n, &out);
        let inc = Csr::from_pairs(n, &inc);
        let mut und = Csr::from_pairs(n, &und);
        let edges = &self.edges;
        let other = |ei: u32, node: usize| {
            let e = &edges[ei as usize];
            if e.from as usize == node {
                e.to
            } else {
                e.from
            }
        };
        for node in 0..n {
            let (s, e) = (und.start[node] as usize, und.start[node + 1] as usize);
            und.items[s..e].sort_unstable_by_key(|&ei| (other(ei, node), ei));
        }
        let mut deg = vec![0u32; n];
        for (node, d) in deg.iter_mut().enumerate() {
            let mut last = NONE;
            for &ei in und.row(node as u32) {
                let o = other(ei, node);
                if o != last {
                    *d += 1;
                    last = o;
                }
            }
        }
        let mut sorted = deg.clone();
        sorted.sort_unstable();
        let p99 = if n == 0 { 0 } else { sorted[((n as f64 * 0.99) as usize).min(n - 1)] };
        let hub_cut = p99.max(50);

        let mut by_name: HashMap<String, Vec<u32>> = HashMap::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if let NodeKind::Symbol(_) = node.kind {
                by_name.entry(node.name.clone()).or_default().push(i as u32);
                if node.qual != node.name {
                    by_name.entry(node.qual.clone()).or_default().push(i as u32);
                }
            }
        }
        let mut loose_by_name: HashMap<String, Vec<u32>> = HashMap::new();
        for (i, l) in self.loose.iter().enumerate() {
            loose_by_name.entry(l.name.clone()).or_default().push(i as u32);
        }
        self.tables = Tables { out, inc, und, deg, hub_cut, by_name, loose_by_name };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;

    #[test]
    fn same_files_in_any_order_give_an_equal_graph() {
        for project in [py_project(), js_project(), rust_project(), query_project()] {
            let a = Graph::build(&project);
            let mut state = 0x2545_f491_4f6c_dd1du64;
            for _ in 0..8 {
                let mut shuffled = project.clone();
                for i in (1..shuffled.len()).rev() {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    shuffled.swap(i, (state % (i as u64 + 1)) as usize);
                }
                let b = Graph::build(&shuffled);
                assert_eq!(a, b);
                assert_eq!(a.stats(), b.stats());
                assert_eq!(a.communities(), b.communities());
                assert_eq!(a.query("user save update", 300), b.query("user save update", 300));
            }
        }
    }

    #[test]
    fn a_duplicate_path_is_kept_once() {
        let mut files = py_project();
        let again = files[0].clone();
        files.push(again);
        assert_eq!(Graph::build(&files), Graph::build(&py_project()));
    }

    #[test]
    fn save_then_load_gives_the_same_graph_and_working_tables() {
        let g = Graph::build(&py_project());
        let dir = std::env::temp_dir().join(format!("lxr-graph-test-{}", std::process::id()));
        let path = dir.join("graph.bin");
        g.save(&path).unwrap();
        let back = Graph::load(&path).unwrap();
        assert_eq!(g, back);
        let run = g.find("run")[0];
        assert_eq!(g.callees(run), back.callees(run));
        assert_eq!(g.affected(g.find("parse_args")[0], 3), back.affected(g.find("parse_args")[0], 3));
        assert_eq!(g.query("user save", 200), back.query("user save", 200));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_graph_with_numbers_outside_it_is_refused_on_load() {
        let mut g = Graph::build(&py_project());
        g.edges[0].to = 9_999_999;
        let dir = std::env::temp_dir().join(format!("lxr-graph-bad-{}", std::process::id()));
        let path = dir.join("graph.bin");
        g.save(&path).unwrap();
        let err = Graph::load(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_or_garbled_file_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("lxr-graph-junk-{}", std::process::id()));
        assert!(Graph::load(&dir.join("nothing.bin")).is_err());
        lxr_core::store::write_atomic(&dir.join("junk.bin"), &[1, 2, 3, 250, 251, 252, 253]).unwrap();
        assert!(Graph::load(&dir.join("junk.bin")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_empty_graph_answers_every_question_with_nothing() {
        let g = Graph::build(&[]);
        assert!(g.find("x").is_empty());
        assert!(g.callers(0).is_empty());
        assert!(g.affected(0, 3).is_empty());
        assert!(g.shortest_path(0, 1).is_none());
        assert!(g.hubs(5).is_empty());
        assert!(g.communities().is_empty());
        assert_eq!(g.query("anything at all", 100), QueryAnswer::default());
        assert_eq!(g.path_of(3), "");
    }
}
