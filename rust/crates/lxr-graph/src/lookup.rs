//! Looking things up: nodes by name, and the neighbours of a node.

use crate::types::*;

/// Splits `path/to/file.py::name` or `file.py:name` into the file part and the name part.
fn split_file_form(query: &str) -> Option<(&str, &str)> {
    let (left, right) = match query.rfind("::") {
        Some(i) => (&query[..i], &query[i + 2..]),
        None => query.split_once(':')?,
    };
    if left.is_empty() || right.is_empty() {
        None
    } else {
        Some((left, right))
    }
}

/// True when `path` is `suffix` or ends with it at a folder boundary.
fn path_ends_with(path: &str, suffix: &str, fold: bool) -> bool {
    let (path, suffix) = if fold { (path.to_lowercase(), suffix.to_lowercase()) } else { (path.to_string(), suffix.to_string()) };
    let suffix = suffix.trim_start_matches("./");
    path == suffix || path.ends_with(&format!("/{suffix}"))
}

impl Graph {
    /// Sort key that orders nodes by (path, start line), then by number.
    pub(crate) fn order_key(&self, node: u32) -> (u32, u32, u32) {
        match self.nodes.get(node as usize) {
            Some(n) => (n.file, n.start, node),
            None => (u32::MAX, u32::MAX, node),
        }
    }

    /// Sorts node numbers by (path, start line) and drops repeats.
    pub(crate) fn sort_nodes(&self, nodes: &mut Vec<u32>) {
        nodes.sort_unstable_by_key(|&n| self.order_key(n));
        nodes.dedup();
    }

    /// The repository path of the file a node lives in. Empty for a node that does not exist.
    pub fn path_of(&self, node: u32) -> &str {
        self.nodes
            .get(node as usize)
            .and_then(|n| self.files.get(n.file as usize))
            .map(String::as_str)
            .unwrap_or("")
    }

    /// The nodes a name stands for, ordered by (path, start line).
    ///
    /// A symbol matches when its name or its qualified name (`Class.method`) is `name`.
    /// `path/to/file.py::name` and `file.py:name` match only symbols in a file whose path
    /// ends with that path. A file node matches when `name` is a file path or ends one at
    /// a folder boundary. When nothing matches exactly, the same is tried ignoring case.
    pub fn find(&self, name: &str) -> Vec<u32> {
        let name = name.trim();
        if name.is_empty() {
            return Vec::new();
        }
        let exact = self.find_with(name, false);
        if !exact.is_empty() {
            return exact;
        }
        self.find_with(name, true)
    }

    fn find_with(&self, query: &str, fold: bool) -> Vec<u32> {
        if let Some((file_part, name_part)) = split_file_form(query) {
            let mut narrowed = self.symbols_named(name_part, fold);
            narrowed.retain(|&n| path_ends_with(self.path_of(n), file_part, fold));
            if !narrowed.is_empty() {
                self.sort_nodes(&mut narrowed);
                return narrowed;
            }
        }
        let mut found = self.symbols_named(query, fold);
        for (i, path) in self.files.iter().enumerate() {
            if path_ends_with(path, query, fold) {
                found.push(i as u32);
            }
        }
        self.sort_nodes(&mut found);
        found
    }

    /// The symbols whose name or qualified name is `name`.
    fn symbols_named(&self, name: &str, fold: bool) -> Vec<u32> {
        if !fold {
            return self.tables.by_name.get(name).cloned().unwrap_or_default();
        }
        let wanted = name.to_lowercase();
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(n.kind, NodeKind::Symbol(_)))
            .filter(|(_, n)| n.name.to_lowercase() == wanted || n.qual.to_lowercase() == wanted)
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// Edges of one relation leaving `node`, as (other end, edge) pairs.
    fn links_out(&self, node: u32, rel: Relation) -> impl Iterator<Item = &Edge> + '_ {
        self.tables.out.row(node).iter().map(|&ei| &self.edges[ei as usize]).filter(move |e| e.rel == rel)
    }

    /// Edges of one relation arriving at `node`.
    fn links_in(&self, node: u32, rel: Relation) -> impl Iterator<Item = &Edge> + '_ {
        self.tables.inc.row(node).iter().map(|&ei| &self.edges[ei as usize]).filter(move |e| e.rel == rel)
    }

    /// Who calls `node`: one link per calling node, ordered by (path, start line). A call
    /// at module level has the file node as caller. A function that calls itself is not
    /// listed as its own caller.
    pub fn callers(&self, node: u32) -> Vec<Link> {
        let mut links: Vec<Link> = self
            .links_in(node, Relation::Calls)
            .filter(|e| e.from != e.to)
            .map(|e| Link { node: e.from, conf: e.conf, lines: e.lines.clone() })
            .collect();
        links.sort_by_key(|l| self.order_key(l.node));
        links
    }

    /// What `node` calls: one link per called node, ordered by (path, start line). The
    /// lines are those of the calls, in the file of `node`. Recursion is not listed.
    pub fn callees(&self, node: u32) -> Vec<Link> {
        let mut links: Vec<Link> = self
            .links_out(node, Relation::Calls)
            .filter(|e| e.from != e.to)
            .map(|e| Link { node: e.to, conf: e.conf, lines: e.lines.clone() })
            .collect();
        links.sort_by_key(|l| self.order_key(l.node));
        links
    }

    /// Call sites of the bare name `name` that no rule tied to a definition, as one link per
    /// calling node, ordered by (path, start line). The confidence of such a link is `Inferred`.
    pub fn loose_callers(&self, name: &str) -> Vec<Link> {
        let mut links: Vec<Link> = self
            .tables
            .loose_by_name
            .get(name)
            .map(|rows| {
                rows.iter()
                    .map(|&i| &self.loose[i as usize])
                    .map(|l| Link { node: l.from, conf: Confidence::Inferred, lines: l.lines.clone() })
                    .collect()
            })
            .unwrap_or_default();
        links.sort_by_key(|l| self.order_key(l.node));
        links
    }

    /// The definitions held by `node`: the top-level definitions of a file, the methods and
    /// nested definitions of a class or function.
    pub fn members(&self, node: u32) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .links_out(node, Relation::Contains)
            .chain(self.links_out(node, Relation::Method))
            .map(|e| e.to)
            .collect();
        self.sort_nodes(&mut out);
        out
    }

    /// The classes `node` extends or implements, among those in the repository.
    pub fn bases(&self, node: u32) -> Vec<u32> {
        let mut out: Vec<u32> = self.links_out(node, Relation::Inherits).map(|e| e.to).collect();
        self.sort_nodes(&mut out);
        out
    }

    /// The classes that extend or implement `node`.
    pub fn subclasses(&self, node: u32) -> Vec<u32> {
        let mut out: Vec<u32> = self.links_in(node, Relation::Inherits).map(|e| e.from).collect();
        self.sort_nodes(&mut out);
        out
    }

    /// The files a file imports, among those in the repository.
    pub fn imports_of(&self, file_node: u32) -> Vec<u32> {
        let mut out: Vec<u32> = self.links_out(file_node, Relation::Imports).map(|e| e.to).collect();
        self.sort_nodes(&mut out);
        out
    }

    /// The files that import a file.
    pub fn importers_of(&self, file_node: u32) -> Vec<u32> {
        let mut out: Vec<u32> = self.links_in(file_node, Relation::Imports).map(|e| e.from).collect();
        self.sort_nodes(&mut out);
        out
    }

    /// Counts that describe the graph. Calls are counted one by one as they were found in the
    /// input, not per edge.
    pub fn stats(&self) -> Stats {
        let files = self.files.len();
        let edges = Relation::ALL
            .iter()
            .map(|&rel| (rel, self.edges.iter().filter(|e| e.rel == rel).count()))
            .collect();
        Stats {
            files,
            symbols: self.nodes.len().saturating_sub(files),
            edges,
            calls_extracted: self.tally.extracted,
            calls_inferred: self.tally.inferred,
            calls_loose: self.tally.loose,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testkit::*;
    use crate::Graph;

    #[test]
    fn find_matches_name_and_qualified_name_in_path_order() {
        let g = Graph::build(&py_project());
        assert_eq!(
            labels(&g, &g.find("render")),
            ["app/views.py::render", "tests/helpers.py::render", "tools/format.py::render"]
        );
        assert_eq!(labels(&g, &g.find("update")), ["app/models.py::User.update", "app/models.py::Admin.update"]);
        assert_eq!(labels(&g, &g.find("Admin.update")), ["app/models.py::Admin.update"]);
        assert_eq!(labels(&g, &g.find("twice")), ["tools/format.py::Report.twice", "tools/format.py::Summary.twice"]);
        assert_eq!(labels(&g, &g.find("  render  ")).len(), 3);
        assert!(g.find("nope").is_empty());
        assert!(g.find("").is_empty());
        assert!(g.find("   ").is_empty());
    }

    #[test]
    fn find_narrows_to_a_file_in_both_spellings() {
        let g = Graph::build(&py_project());
        assert_eq!(labels(&g, &g.find("tools/format.py::render")), ["tools/format.py::render"]);
        assert_eq!(labels(&g, &g.find("views.py:render")), ["app/views.py::render"]);
        assert_eq!(labels(&g, &g.find("models.py::update")), ["app/models.py::User.update", "app/models.py::Admin.update"]);
        // two files end in helpers.py, only one defines render
        assert_eq!(labels(&g, &g.find("helpers.py::render")), ["tests/helpers.py::render"]);
        assert_eq!(labels(&g, &g.find("models.py::Admin.update")), ["app/models.py::Admin.update"]);
        assert!(g.find("models.py::nope").is_empty());
        assert!(g.find("views.py::update").is_empty());
    }

    #[test]
    fn find_returns_a_file_node_for_a_path_or_its_tail() {
        let g = Graph::build(&py_project());
        let util = g.files.iter().position(|p| p == "app/util.py").unwrap() as u32;
        assert_eq!(g.find("app/util.py"), [util]);
        assert_eq!(g.find("util.py"), [util]);
        assert_eq!(g.find("./app/util.py"), [util]);
        assert_eq!(
            labels(&g, &g.find("helpers.py")),
            ["app/helpers.py::app/helpers.py", "tests/helpers.py::tests/helpers.py"]
        );
        // the tail must start at a folder boundary
        assert!(g.find("til.py").is_empty());
        assert_eq!(g.path_of(util), "app/util.py");
        assert_eq!(g.path_of(one(&g, "Admin.update")), "app/models.py");
        assert_eq!(g.path_of(u32::MAX), "");
    }

    #[test]
    fn find_ignores_case_only_when_nothing_matches_exactly() {
        let g = Graph::build(&py_project());
        assert_eq!(labels(&g, &g.find("ADMIN.UPDATE")), ["app/models.py::Admin.update"]);
        assert_eq!(labels(&g, &g.find("user")), ["app/models.py::User"]);
        assert_eq!(labels(&g, &g.find("APP/VIEWS.PY")), ["app/views.py::app/views.py"]);
        assert_eq!(labels(&g, &g.find("VIEWS.PY:RENDER")), ["app/views.py::render"]);
        // an exact match hides the case-insensitive ones
        assert_eq!(labels(&g, &g.find("User")), ["app/models.py::User"]);
        assert_eq!(labels(&g, &g.find("Render")).len(), 3);
    }
}
