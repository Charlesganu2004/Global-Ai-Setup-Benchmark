//! Building the graph: nodes first, then the imports, the inheritance, and last the calls.
//! The rules that tie a call to a definition live in `resolve`.

use crate::modules::{dir_of, is_js, PathIndex};
use crate::types::*;
use lxr_core::{FileFacts, Import, Lang, SymKind};
use std::collections::{BTreeMap, HashMap};

/// What a name may stand for when it is looked up.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Want {
    /// A function, a method, or a class-like definition: anything a call can reach.
    Target,
    /// A class-like definition only.
    Class,
}

/// True for the kinds that stand for a type: a class, a struct, an enum, a trait or an interface.
pub(crate) fn is_class_like(kind: SymKind) -> bool {
    matches!(kind, SymKind::Class | SymKind::Struct | SymKind::Enum | SymKind::Trait | SymKind::Interface)
}

impl Want {
    /// Whether a definition of this kind is an answer.
    pub(crate) fn accepts(self, kind: SymKind) -> bool {
        match self {
            Want::Target => kind.is_callable() || is_class_like(kind),
            Want::Class => is_class_like(kind),
        }
    }
}

/// The module system a file's imports follow.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModuleSystem {
    Python,
    Ecma,
    Rust,
    Other,
}

/// A name that an import brings into a file.
pub(crate) struct Binding<'a> {
    /// The file the name comes from.
    pub file: u32,
    /// The name inside that file. Empty for a default or whole-module import.
    pub orig: &'a str,
    /// The name it is bound to here.
    pub alias: &'a str,
}

/// What one file imports, in the shapes the call rules need.
#[derive(Default)]
pub(crate) struct FileImports<'a> {
    /// Names bound by an import, by the bound name.
    pub names: HashMap<&'a str, Vec<Binding<'a>>>,
    /// Modules bound by an import, by the bound name or the dotted module name.
    pub modules: HashMap<&'a str, u32>,
    /// Files whose every name is brought in by a star import.
    pub stars: Vec<u32>,
}

/// Everything the build keeps while it works.
pub(crate) struct Builder<'a> {
    pub files: Vec<&'a FileFacts>,
    pub index: PathIndex<'a>,
    pub nfiles: u32,
    /// Node id of the first symbol of each file.
    pub sym_base: Vec<u32>,
    pub nodes: Vec<Node>,
    /// Kind of each symbol node, `None` for a file node.
    pub kind: Vec<Option<SymKind>>,
    /// The node a node sits in: its parent symbol, or its file for a top-level symbol.
    pub up: Vec<u32>,
    /// Definitions by (the node that holds them, name), in source order.
    pub scope_defs: HashMap<(u32, &'a str), Vec<u32>>,
    /// Functions, methods and class-like definitions by name.
    pub global: HashMap<&'a str, Vec<u32>>,
    /// The family of each class-like node, an index into `groups`, or `NONE`.
    pub fam: Vec<u32>,
    /// Class-like nodes that are one type: a struct and its impl blocks, say.
    pub groups: Vec<Vec<u32>>,
    /// The resolved base classes of each node.
    pub bases_of: Vec<Vec<u32>>,
    pub imps: Vec<FileImports<'a>>,
    /// Folder of each file.
    pub dirs: Vec<&'a str>,
    pub is_test: Vec<bool>,
    /// Calls to a bare name may reach a method of the enclosing class, as in Java.
    pub implicit_scope: Vec<bool>,
    /// Files of one family may call each other by name.
    pub family: Vec<u8>,
    edges: Vec<Edge>,
    import_edges: BTreeMap<(u32, u32), Vec<u32>>,
    calls: HashMap<(u32, u32), (Confidence, Vec<u32>)>,
    loose: BTreeMap<(u32, &'a str), Vec<u32>>,
    tally: CallTally,
}

/// The last part of a path.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// True for a path in a test folder or with a test file name.
pub(crate) fn is_test_path(path: &str) -> bool {
    let mut parts = path.split('/').collect::<Vec<_>>();
    let file = parts.pop().unwrap_or("");
    if parts.iter().any(|p| p.eq_ignore_ascii_case("test") || p.eq_ignore_ascii_case("tests")) {
        return true;
    }
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file).to_lowercase();
    stem.starts_with("test_") || stem.ends_with("_test") || stem.ends_with(".test") || stem.ends_with(".spec")
}

/// The languages whose calls and imports can meet by name.
fn language_family(lang: Lang) -> u8 {
    match lang {
        Lang::Python => 0,
        Lang::JavaScript | Lang::Jsx | Lang::TypeScript | Lang::Tsx => 1,
        Lang::Rust => 2,
        Lang::Go => 3,
        Lang::Java | Lang::Kotlin => 4,
        Lang::C | Lang::Cpp => 5,
        Lang::CSharp => 6,
        Lang::Swift => 7,
        Lang::Ruby => 8,
        Lang::Php => 9,
        Lang::Shell => 10,
    }
}

fn module_system(lang: Lang) -> ModuleSystem {
    match lang {
        Lang::Python => ModuleSystem::Python,
        l if is_js(l) => ModuleSystem::Ecma,
        Lang::Rust => ModuleSystem::Rust,
        _ => ModuleSystem::Other,
    }
}

/// Builds the graph of the files. See [`Graph::build`].
pub(crate) fn build_graph(input: &[FileFacts]) -> Graph {
    let mut order: Vec<&FileFacts> = input.iter().collect();
    order.sort_by(|a, b| {
        (a.path.as_str(), a.hash, a.bytes, a.lines)
            .cmp(&(b.path.as_str(), b.hash, b.bytes, b.lines))
            // Two files of one path and one size can still differ in what was extracted from them.
            // Their facts settle the tie, so that the one kept does not depend on the input order.
            .then_with(|| format!("{a:?}").cmp(&format!("{b:?}")))
    });
    order.dedup_by(|a, b| a.path == b.path);
    let mut builder = Builder::new(order);
    builder.add_nodes();
    builder.link_families();
    builder.add_imports();
    builder.add_inherits();
    builder.add_calls();
    builder.finish()
}

impl<'a> Builder<'a> {
    fn new(files: Vec<&'a FileFacts>) -> Builder<'a> {
        let index = PathIndex::new(&files);
        let dirs = files.iter().map(|f| dir_of(f.path.as_str())).collect();
        let is_test = files.iter().map(|f| is_test_path(&f.path)).collect();
        let implicit_scope = files
            .iter()
            .map(|f| matches!(f.lang, Lang::Java | Lang::Kotlin | Lang::CSharp | Lang::Cpp | Lang::Swift | Lang::Ruby))
            .collect();
        let family = files.iter().map(|f| language_family(f.lang)).collect();
        let imps = files.iter().map(|_| FileImports::default()).collect();
        Builder {
            nfiles: files.len() as u32,
            files,
            index,
            sym_base: Vec::new(),
            nodes: Vec::new(),
            kind: Vec::new(),
            up: Vec::new(),
            scope_defs: HashMap::new(),
            global: HashMap::new(),
            fam: Vec::new(),
            groups: Vec::new(),
            bases_of: Vec::new(),
            imps,
            dirs,
            is_test,
            implicit_scope,
            family,
            edges: Vec::new(),
            import_edges: BTreeMap::new(),
            calls: HashMap::new(),
            loose: BTreeMap::new(),
            tally: CallTally::default(),
        }
    }

    /// Creates the file nodes, then the symbol nodes, with their containment edges.
    fn add_nodes(&mut self) {
        let files = self.files.clone();
        for (fi, &file) in files.iter().enumerate() {
            self.nodes.push(Node {
                kind: NodeKind::File,
                name: basename(&file.path).to_string(),
                qual: file.path.clone(),
                file: fi as u32,
                start: 1,
                end: file.lines,
                sig: String::new(),
                doc: String::new(),
                hash: file.hash,
            });
            self.kind.push(None);
            self.up.push(NONE);
        }
        for (fi, &file) in files.iter().enumerate() {
            let base = self.nodes.len() as u32;
            self.sym_base.push(base);
            for (i, sym) in file.symbols.iter().enumerate() {
                let id = base + i as u32;
                let up = match sym.parent {
                    Some(p) if (p as usize) < i => base + p,
                    _ => fi as u32,
                };
                self.nodes.push(Node {
                    kind: NodeKind::Symbol(sym.kind),
                    name: sym.name.clone(),
                    qual: sym.qual.clone(),
                    file: fi as u32,
                    start: sym.start,
                    end: sym.end,
                    sig: sym.sig.clone(),
                    doc: sym.doc.clone(),
                    hash: sym.hash,
                });
                self.kind.push(Some(sym.kind));
                self.up.push(up);
                self.scope_defs.entry((up, sym.name.as_str())).or_default().push(id);
                if Want::Target.accepts(sym.kind) {
                    self.global.entry(sym.name.as_str()).or_default().push(id);
                }
                let rel = if sym.kind == SymKind::Method { Relation::Method } else { Relation::Contains };
                self.edges.push(Edge { from: up, to: id, rel, conf: Confidence::Extracted, lines: Vec::new() });
            }
        }
        self.fam = vec![NONE; self.nodes.len()];
        self.bases_of = vec![Vec::new(); self.nodes.len()];
    }

    /// Groups the containers that are one type: same file and same qualified name, so a
    /// struct and its `impl` blocks meet. An `impl` block whose type is defined in another
    /// file joins that type when the type's name is unique.
    fn link_families(&mut self) {
        let mut by_key: HashMap<(u32, &'a str), u32> = HashMap::new();
        for id in self.nfiles..self.nodes.len() as u32 {
            let Some(kind) = self.kind[id as usize] else { continue };
            if !kind.is_container() {
                continue;
            }
            let file = self.nodes[id as usize].file;
            let file_facts: &'a FileFacts = self.files[file as usize];
            let local = (id - self.sym_base[file as usize]) as usize;
            let qual: &'a str = file_facts.symbols[local].qual.as_str();
            let group = *by_key.entry((file, qual)).or_insert_with(|| {
                self.groups.push(Vec::new());
                (self.groups.len() - 1) as u32
            });
            self.groups[group as usize].push(id);
            self.fam[id as usize] = group;
        }
        let impl_only: Vec<u32> = (0..self.groups.len() as u32)
            .filter(|&g| {
                let members = &self.groups[g as usize];
                !members.is_empty() && members.iter().all(|&m| self.kind[m as usize] == Some(SymKind::Impl))
            })
            .collect();
        for g in impl_only {
            let first = self.groups[g as usize][0];
            let file = self.nodes[first as usize].file as usize;
            let name = self.nodes[first as usize].name.as_str();
            let types: Vec<u32> = self
                .global
                .get(name)
                .map(|ids| {
                    ids.iter()
                        .copied()
                        .filter(|&t| {
                            self.kind[t as usize].map_or(false, is_class_like)
                                && self.family[self.nodes[t as usize].file as usize] == self.family[file]
                        })
                        .collect()
                })
                .unwrap_or_default();
            if types.len() == 1 {
                let target = types[0];
                let into = self.fam[target as usize];
                if into != NONE && into != g {
                    let moved = std::mem::take(&mut self.groups[g as usize]);
                    for &m in &moved {
                        self.fam[m as usize] = into;
                    }
                    self.groups[into as usize].extend(moved);
                    self.groups[into as usize].sort_unstable();
                }
            }
        }
    }

    /// Resolves every import to a file and records what it binds.
    fn add_imports(&mut self) {
        let files = self.files.clone();
        for (fi, &file) in files.iter().enumerate() {
            let system = module_system(file.lang);
            for imp in &file.imports {
                match system {
                    ModuleSystem::Python => self.bind_python(fi as u32, imp),
                    ModuleSystem::Ecma => self.bind_ecma(fi as u32, imp),
                    ModuleSystem::Rust => self.bind_rust(fi as u32, imp),
                    ModuleSystem::Other => {}
                }
            }
        }
    }

    fn import_edge(&mut self, from: u32, to: u32, line: u32) {
        if from != to {
            self.import_edges.entry((from, to)).or_default().push(line);
        }
    }

    /// True when `file` defines `name` at its top level.
    fn has_top_level(&self, file: u32, name: &str) -> bool {
        self.scope_defs.contains_key(&(file, name))
    }

    fn bind_python(&mut self, f: u32, imp: &'a Import) {
        let level = imp.level;
        if imp.name == "*" && (imp.alias.is_empty() || imp.alias == "*") {
            if let Some(t) = self.index.python_module(f, &imp.module, level) {
                self.imps[f as usize].stars.push(t);
                self.import_edge(f, t, imp.line);
            }
            return;
        }
        if imp.name.is_empty() || imp.name == "*" {
            // `import a.b`, `import a.b as c`: the module itself is bound.
            let Some(t) = self.index.python_module(f, &imp.module, level) else { return };
            self.import_edge(f, t, imp.line);
            let last = imp.module.rsplit('.').next().unwrap_or("");
            let spelled_alias = !imp.alias.is_empty() && imp.alias != imp.module && imp.alias != last;
            let imps = &mut self.imps[f as usize];
            if !imp.alias.is_empty() {
                imps.modules.insert(imp.alias.as_str(), t);
            }
            if !spelled_alias {
                imps.modules.insert(imp.module.as_str(), t);
            }
            if !spelled_alias && level == 0 {
                // `import a.b.c` also binds the packages `a` and `a.b`.
                let mut end = 0;
                while let Some(dot) = imp.module[end..].find('.') {
                    end += dot;
                    let prefix = &imp.module[..end];
                    if let Some(p) = self.index.python_module(f, prefix, 0) {
                        self.imps[f as usize].modules.entry(prefix).or_insert(p);
                    }
                    end += 1;
                }
            }
            return;
        }
        // `from m import x [as y]`: x is a name of m, or a module inside the package m.
        let alias: &'a str = if imp.alias.is_empty() { imp.name.as_str() } else { imp.alias.as_str() };
        let in_module = self.index.python_module(f, &imp.module, level);
        let joined = if imp.module.is_empty() { imp.name.clone() } else { format!("{}.{}", imp.module, imp.name) };
        let submodule = self.index.python_module(f, &joined, level);
        match (in_module, submodule) {
            (Some(m), _) if self.has_top_level(m, &imp.name) => {
                self.bind_name(f, alias, Binding { file: m, orig: imp.name.as_str(), alias });
                self.import_edge(f, m, imp.line);
            }
            (_, Some(sub)) => {
                self.imps[f as usize].modules.insert(alias, sub);
                self.import_edge(f, sub, imp.line);
            }
            (Some(m), None) => {
                self.bind_name(f, alias, Binding { file: m, orig: imp.name.as_str(), alias });
                self.import_edge(f, m, imp.line);
            }
            (None, None) => {}
        }
    }

    fn bind_name(&mut self, f: u32, key: &'a str, binding: Binding<'a>) {
        self.imps[f as usize].names.entry(key).or_default().push(binding);
    }

    fn bind_ecma(&mut self, f: u32, imp: &'a Import) {
        let Some(t) = self.index.js_module(f, &imp.module) else { return };
        self.import_edge(f, t, imp.line);
        let alias: &'a str = if imp.alias.is_empty() { imp.name.as_str() } else { imp.alias.as_str() };
        if imp.name == "*" {
            if alias.is_empty() || alias == "*" {
                self.imps[f as usize].stars.push(t);
            } else {
                self.imps[f as usize].modules.insert(alias, t);
            }
        } else if imp.name.is_empty() || imp.name == "default" {
            if !alias.is_empty() && alias != "default" {
                // A default import stands for the file's default export. When that is a class, the
                // class answers for `alias.member`, so the file's top-level names must not.
                if imp.name.is_empty() || self.default_export(t, alias, Want::Class).is_none() {
                    self.imps[f as usize].modules.insert(alias, t);
                }
                self.bind_name(f, alias, Binding { file: t, orig: "", alias });
            }
        } else {
            self.bind_name(f, alias, Binding { file: t, orig: imp.name.as_str(), alias });
        }
    }

    fn bind_rust(&mut self, f: u32, imp: &'a Import) {
        let mut segments: Vec<&'a str> = imp.module.split("::").filter(|s| !s.is_empty()).collect();
        let star = imp.name == "*";
        if !imp.name.is_empty() && !star && imp.name != "self" {
            segments.push(imp.name.as_str());
        }
        let Some((t, used)) = self.index.rust_path(f, &segments) else { return };
        self.import_edge(f, t, imp.line);
        if star {
            if used == segments.len() {
                self.imps[f as usize].stars.push(t);
            }
            return;
        }
        let alias: &'a str = if imp.alias.is_empty() { segments.last().copied().unwrap_or("") } else { imp.alias.as_str() };
        if alias.is_empty() || alias == "_" {
            return;
        }
        if used == segments.len() {
            self.imps[f as usize].modules.insert(alias, t);
        } else {
            self.bind_name(f, alias, Binding { file: t, orig: segments[used], alias });
        }
    }

    /// Resolves the base names of every class-like definition into `Inherits` edges.
    fn add_inherits(&mut self) {
        for id in self.nfiles..self.nodes.len() as u32 {
            let file = self.nodes[id as usize].file as usize;
            let facts: &'a FileFacts = self.files[file];
            let sym = &facts.symbols[(id - self.sym_base[file]) as usize];
            for base in &sym.bases {
                let Some((target, conf)) = self.resolve_base(file, id, base) else { continue };
                if target == id || self.bases_of[id as usize].contains(&target) {
                    continue;
                }
                self.bases_of[id as usize].push(target);
                self.edges.push(Edge { from: id, to: target, rel: Relation::Inherits, conf, lines: vec![sym.start] });
            }
        }
    }

    /// Resolves every call into a `Calls` edge, or into a loose call.
    fn add_calls(&mut self) {
        let files = self.files.clone();
        for (fi, &file) in files.iter().enumerate() {
            let base = self.sym_base[fi];
            for call in &file.calls {
                if call.name.is_empty() {
                    continue;
                }
                let caller = match call.caller {
                    Some(i) if (i as usize) < file.symbols.len() => base + i,
                    _ => fi as u32,
                };
                match self.resolve_call(fi, caller, &call.name, &call.recv) {
                    Some((target, conf)) => {
                        let entry = self.calls.entry((caller, target)).or_insert((conf, Vec::new()));
                        if conf == Confidence::Extracted {
                            entry.0 = Confidence::Extracted;
                        }
                        entry.1.push(call.line);
                        match conf {
                            Confidence::Extracted => self.tally.extracted += 1,
                            Confidence::Inferred => self.tally.inferred += 1,
                        }
                    }
                    None => {
                        self.loose.entry((caller, call.name.as_str())).or_default().push(call.line);
                        self.tally.loose += 1;
                    }
                }
            }
        }
    }

    /// Puts every edge in its final order and hands over the finished graph.
    fn finish(self) -> Graph {
        let mut edges = self.edges;
        for ((from, to), mut lines) in self.import_edges {
            lines.sort_unstable();
            lines.dedup();
            edges.push(Edge { from, to, rel: Relation::Imports, conf: Confidence::Extracted, lines });
        }
        for ((from, to), (conf, mut lines)) in self.calls {
            lines.sort_unstable();
            lines.dedup();
            edges.push(Edge { from, to, rel: Relation::Calls, conf, lines });
        }
        edges.sort_by_key(|e| (e.from, e.to, e.rel));
        let loose = self
            .loose
            .into_iter()
            .map(|((from, name), mut lines)| {
                lines.sort_unstable();
                lines.dedup();
                LooseCall { name: name.to_string(), from, lines }
            })
            .collect();
        let mut graph = Graph {
            files: self.files.iter().map(|f| f.path.clone()).collect(),
            nodes: self.nodes,
            edges,
            loose,
            tally: self.tally,
            tables: Tables::default(),
        };
        graph.rebuild();
        graph
    }
}

#[cfg(test)]
mod tests {
    use crate::testkit::*;
    use crate::{Confidence, Graph, Relation};
    use lxr_core::{Lang, SymKind};

    fn calls(g: &Graph, from: &str) -> Vec<String> {
        link_texts(g, &g.callees(one(g, from)))
    }

    fn called_by(g: &Graph, to: &str) -> Vec<String> {
        link_texts(g, &g.callers(one(g, to)))
    }

    #[test]
    fn calls_through_self_cls_and_super_follow_the_class_and_its_bases() {
        let g = Graph::build(&py_project());
        // self.save() in User reaches Base one level down; Admin's reaches it two levels down
        assert_eq!(calls(&g, "User.update"), ["app/models.py::Base.save [14] E", "app/models.py::User.audit [15] E"]);
        assert_eq!(calls(&g, "User.create"), ["app/models.py::Base.make [21] E"]);
        // super().update() skips Admin's own update, which is the one being extended
        assert_eq!(
            calls(&g, "Admin.update"),
            ["app/models.py::Base.save [30] E", "app/models.py::User.update [29] E"]
        );
        assert_eq!(calls(&g, "Admin.run"), ["app/models.py::Base.validate [33] E"]);
        // a class whose base is named through a module
        assert_eq!(calls(&g, "Guest.greet"), ["app/models.py::User.audit [5] E"]);
    }

    #[test]
    fn recursion_is_in_the_edge_list_but_not_in_callers_or_callees() {
        let g = Graph::build(&py_project());
        for (name, line) in [("fact", 20), ("User.walk", 24)] {
            let node = one(&g, name);
            let loop_edge = g.edges.iter().find(|e| e.rel == Relation::Calls && e.from == node && e.to == node);
            assert_eq!(loop_edge.map(|e| e.lines.clone()), Some(vec![line]), "{name}");
            assert!(g.callers(node).is_empty(), "{name}");
            assert!(g.callees(node).is_empty(), "{name}");
        }
    }

    #[test]
    fn bare_names_follow_scope_then_file_then_imports() {
        let g = Graph::build(&py_project());
        // sibling nested function, then a function's own nested function
        assert_eq!(calls(&g, "inner_b"), ["app/util.py::outer.inner_a [12] E"]);
        assert_eq!(calls(&g, "outer"), ["app/util.py::outer.inner_a [15] E", "app/util.py::outer.inner_b [14] E"]);
        // a top-level function of the same file, with every call line
        assert_eq!(called_by(&g, "app/main.py::local_fn"), ["app/main.py::run [11, 22] E"]);
        // an imported name under an alias, a relative import two levels up, and a module-level call
        assert_eq!(
            called_by(&g, "parse_args"),
            [
                "app/main.py::run [9, 12] E",
                "app/services/store.py::open_store [6] E",
                "app/util.py::app/util.py [30] E",
            ]
        );
        // a relative import of a name, and a module-level call made in main.py itself
        assert_eq!(called_by(&g, "slugify"), ["app/main.py::run [10] E"]);
        assert_eq!(called_by(&g, "app/main.py::run"), ["app/main.py::app/main.py [30] E"]);
        // calling a class by its name points at the class
        assert_eq!(called_by(&g, "User"), ["app/main.py::run [21] E", "app/services/store.py::open_store [7] E"]);
    }

    #[test]
    fn a_unique_name_is_inferred_and_the_nearest_of_several_wins() {
        let g = Graph::build(&py_project());
        // the only definition of the name in the repository
        assert_eq!(called_by(&g, "unique_fn"), ["app/main.py::run [15] I"]);
        // three definitions of render: tests/ is dropped for a caller outside tests, then the same folder wins
        assert_eq!(called_by(&g, "app/views.py::render"), ["app/main.py::run [16] I"]);
        assert!(called_by(&g, "tools/format.py::render").is_empty());
        // a caller inside tests keeps the test candidates, and its own folder wins
        assert_eq!(called_by(&g, "tests/helpers.py::render"), ["tests/test_views.py::test_render [3] I"]);
    }

    #[test]
    fn ambiguity_and_unknown_names_stay_loose() {
        let g = Graph::build(&py_project());
        let run = one(&g, "app/main.py::run");
        let main_file = one(&g, "app/main.py");
        // fmt exists in two folders, neither nearer to app/: no pick
        let fmt = g.loose_callers("fmt");
        assert_eq!(link_texts(&g, &fmt), ["app/main.py::run [17] I"]);
        assert_eq!(fmt[0].node, run);
        // a very common member name, two methods of one name, a builtin, and a module-level builtin
        assert_eq!(link_texts(&g, &g.loose_callers("get")), ["app/main.py::run [19] I"]);
        assert_eq!(link_texts(&g, &g.loose_callers("twice")), ["app/main.py::run [20] I"]);
        assert_eq!(link_texts(&g, &g.loose_callers("isinstance")), ["app/main.py::run [23] I"]);
        assert_eq!(g.loose_callers("print")[0].node, main_file);
        assert!(g.loose_callers("nothing").is_empty());
        assert_eq!(g.loose.len(), 5);
        assert!(g.callees(run).iter().all(|l| l.lines.iter().all(|&n| ![17, 19, 20, 23].contains(&n))));
    }

    #[test]
    fn module_and_class_receivers_resolve_to_members() {
        let g = Graph::build(&py_project());
        // import app.util / import app.models as m / from app.services import store (a submodule)
        // and `from . import cache` give module receivers; User, imported, is a class receiver
        assert_eq!(
            calls(&g, "open_store"),
            [
                "app/models.py::Base.save [10] E",
                "app/models.py::User [7] E",
                "app/models.py::User.create [9] E",
                "app/services/cache.py::warm [8] E",
                "app/util.py::parse_args [6] E",
            ]
        );
        assert_eq!(calls(&g, "close_store"), ["app/services/cache.py::flush [15] E"]);
        assert_eq!(
            calls(&g, "app/main.py::run"),
            [
                "app/helpers.py::slugify [10] E",
                "app/main.py::local_fn [11, 22] E",
                "app/models.py::Base.save [24] I",
                "app/models.py::User [21] E",
                "app/models.py::User.create [13] E",
                "app/services/store.py::open_store [14] E",
                "app/util.py::parse_args [9, 12] E",
                "app/views.py::render [16] I",
                "tools/format.py::unique_fn [15] I",
                "tools/format.py::Report.unique_method [18] I",
            ]
        );
        // Base.save is reached through self, through a class named by an import, and, only
        // inferred, through Admin, a class that main.py never imports
        assert_eq!(
            called_by(&g, "Base.save"),
            [
                "app/main.py::run [24] I",
                "app/models.py::User.update [14] E",
                "app/models.py::Admin.update [30] E",
                "app/services/store.py::open_store [10] E",
            ]
        );
    }

    #[test]
    fn any_other_receiver_needs_a_unique_non_common_method() {
        let g = Graph::build(&py_project());
        assert_eq!(called_by(&g, "unique_method"), ["app/main.py::run [18] I"]);
        assert!(called_by(&g, "Report.twice").is_empty());
    }

    #[test]
    fn bases_are_resolved_by_scope_import_dotted_module_or_unique_name() {
        let g = Graph::build(&py_project());
        let bases = |name: &str| labels(&g, &g.bases(one(&g, name)));
        assert_eq!(bases("User"), ["app/models.py::Base"]);
        assert_eq!(bases("Admin"), ["app/models.py::User"]);
        assert_eq!(bases("Guest"), ["app/models.py::User"]);
        assert_eq!(bases("Child"), ["tools/format.py::UniqueBase"]);
        assert!(bases("Weird").is_empty());
        assert!(bases("Base").is_empty());
        assert_eq!(labels(&g, &g.subclasses(one(&g, "User"))), ["app/extras.py::Guest", "app/models.py::Admin"]);
        let inherit_conf = |name: &str| {
            let node = one(&g, name);
            g.edges.iter().find(|e| e.rel == Relation::Inherits && e.from == node).map(|e| (e.conf, e.lines.clone()))
        };
        assert_eq!(inherit_conf("Guest"), Some((Confidence::Extracted, vec![3])));
        assert_eq!(inherit_conf("Child"), Some((Confidence::Inferred, vec![10])));
        assert_eq!(inherit_conf("Weird"), None);
    }

    #[test]
    fn imports_become_edges_between_files() {
        let g = Graph::build(&py_project());
        let main = one(&g, "app/main.py");
        let util = one(&g, "app/util.py");
        assert_eq!(
            labels(&g, &g.imports_of(main)),
            [
                "app/helpers.py::app/helpers.py",
                "app/models.py::app/models.py",
                "app/services/store.py::app/services/store.py",
                "app/util.py::app/util.py",
            ]
        );
        assert_eq!(
            labels(&g, &g.importers_of(util)),
            ["app/main.py::app/main.py", "app/services/store.py::app/services/store.py"]
        );
        assert_eq!(
            labels(&g, &g.imports_of(one(&g, "app/services/store.py"))),
            [
                "app/models.py::app/models.py",
                "app/services/cache.py::app/services/cache.py",
                "app/util.py::app/util.py",
            ]
        );
        let lines = |from: &str, to: &str| {
            let (f, t) = (one(&g, from), one(&g, to));
            g.edges.iter().find(|e| e.rel == Relation::Imports && e.from == f && e.to == t).map(|e| e.lines.clone())
        };
        assert_eq!(lines("app/main.py", "app/util.py"), Some(vec![1, 3]));
        assert_eq!(lines("app/main.py", "app/models.py"), Some(vec![4, 6]));
        assert_eq!(lines("app/main.py", "app/services/store.py"), Some(vec![5]));
        assert_eq!(lines("app/services/store.py", "app/util.py"), Some(vec![1]));
        assert_eq!(lines("app/util.py", "app/main.py"), None);
    }

    #[test]
    fn members_are_the_contains_and_method_children() {
        let g = Graph::build(&py_project());
        assert_eq!(
            labels(&g, &g.members(one(&g, "User"))),
            [
                "app/models.py::User.update",
                "app/models.py::User.audit",
                "app/models.py::User.create",
                "app/models.py::User.walk",
            ]
        );
        assert_eq!(
            labels(&g, &g.members(one(&g, "outer"))),
            ["app/util.py::outer.inner_a", "app/util.py::outer.inner_b"]
        );
        assert_eq!(g.members(one(&g, "app/helpers.py")), [one(&g, "slugify")]);
    }

    #[test]
    fn the_stats_count_every_edge_and_every_call() {
        let g = Graph::build(&py_project());
        let s = g.stats();
        assert_eq!((s.files, s.symbols), (15, 41));
        assert_eq!(
            s.edges,
            [
                (Relation::Contains, 28),
                (Relation::Method, 13),
                (Relation::Imports, 8),
                (Relation::Calls, 31),
                (Relation::Inherits, 4),
            ]
        );
        assert_eq!((s.calls_extracted, s.calls_inferred, s.calls_loose), (28, 5, 5));
        assert_eq!(g.nodes.len(), 56);
        assert!(g.nodes[..15].iter().all(|n| n.kind == crate::NodeKind::File));
        assert_eq!(g.nodes[0].qual, "app/__init__.py");
        assert_eq!(g.nodes[0].name, "__init__.py");
        let helpers = &g.nodes[one(&g, "app/helpers.py") as usize];
        assert_eq!((helpers.start, helpers.end), (1, 40));
    }

    #[test]
    fn javascript_imports_resolve_by_path_and_by_binding() {
        let g = Graph::build(&js_project());
        // named import under an alias, a default import of an index file, a namespace import,
        // a require alias; a package outside the repository leaves the call loose
        assert_eq!(
            calls(&g, "main"),
            [
                "src/lib/index.js::connect [9] E",
                "src/models/user.js::User.create [13] I",
                "src/models/user.js::createUser [10] E",
                "src/util.ts::format [8, 11] E",
            ]
        );
        assert_eq!(link_texts(&g, &g.loose_callers("render")), ["src/app.js::main [12] I"]);
        let app = one(&g, "src/app.js");
        assert_eq!(
            labels(&g, &g.imports_of(app)),
            ["src/lib/index.js::src/lib/index.js", "src/models/user.js::src/models/user.js", "src/util.ts::src/util.ts"]
        );
        let lines = g
            .edges
            .iter()
            .find(|e| e.rel == Relation::Imports && e.to == one(&g, "src/util.ts"))
            .map(|e| e.lines.clone());
        assert_eq!(lines, Some(vec![1, 5]));
    }

    #[test]
    fn rust_paths_use_declarations_and_impl_blocks_resolve() {
        let g = Graph::build(&rust_project());
        assert_eq!(
            calls(&g, "crates/app/src/main.rs::main"),
            [
                "crates/app/src/cli.rs::parse [10, 11] E",
                "crates/store/src/lib.rs::open [7] E",
                "crates/store/src/model.rs::Row.new [8] E",
                "crates/store/src/model.rs::Row.describe [9] E",
            ]
        );
        // super::open() from a nested module reaches the crate root, where open lives
        assert_eq!(
            calls(&g, "crates/store/src/page.rs::render"),
            ["crates/store/src/lib.rs::open [3] E", "crates/store/src/model.rs::Row.new [2] E"]
        );
        // Self::new() inside an impl block, and Row::new() after `use super::model::Row`
        assert_eq!(
            called_by(&g, "Row.new"),
            [
                "crates/app/src/main.rs::main [8] E",
                "crates/store/src/model.rs::Row.describe [12] E",
                "crates/store/src/page.rs::render [2] E",
            ]
        );
        let imports = |path: &str| labels(&g, &g.imports_of(one(&g, path)));
        assert_eq!(
            imports("crates/app/src/main.rs"),
            [
                "crates/app/src/cli.rs::crates/app/src/cli.rs",
                "crates/store/src/lib.rs::crates/store/src/lib.rs",
                "crates/store/src/model.rs::crates/store/src/model.rs",
            ]
        );
        assert_eq!(imports("crates/store/src/lib.rs"), ["crates/store/src/model.rs::crates/store/src/model.rs"]);
        assert_eq!(imports("crates/store/src/page.rs"), ["crates/store/src/model.rs::crates/store/src/model.rs"]);
        let lines = |from: &str, to: &str| {
            let (f, t) = (one(&g, from), one(&g, to));
            g.edges.iter().find(|e| e.rel == Relation::Imports && e.from == f && e.to == t).map(|e| e.lines.clone())
        };
        assert_eq!(lines("crates/app/src/main.rs", "crates/store/src/lib.rs"), Some(vec![1, 3]));
        assert_eq!(lines("crates/store/src/lib.rs", "crates/store/src/model.rs"), Some(vec![1, 2]));
    }

    #[test]
    fn names_are_followed_through_star_imports_and_re_exports_and_cycles_end() {
        let py = Lang::Python;
        let mut init = Fx::new("lib/__init__.py", py);
        init.import("core", "*", "*", 1, 1);
        init.import("util", "helper", "helper", 1, 2);
        let mut core = Fx::new("lib/core.py", py);
        core.func("build_all", 1, 3);
        let mut util = Fx::new("lib/util.py", py);
        util.func("helper", 1, 2);
        let mut main = Fx::new("main.py", py);
        main.import("lib", "build_all", "build_all", 0, 1);
        main.import("lib", "helper", "h", 0, 2);
        let go = main.func("go", 4, 9);
        main.call(Some(go), "", "build_all", 5);
        main.call(Some(go), "", "h", 6);
        // a ring of star imports with a name that is defined nowhere
        let mut a = Fx::new("cyc/a.py", py);
        a.import("b", "*", "*", 1, 1);
        let mut b = Fx::new("cyc/b.py", py);
        b.import("a", "*", "*", 1, 1);
        let mut c = Fx::new("cyc/c.py", py);
        c.import("a", "*", "*", 1, 1);
        let run = c.func("run", 3, 5);
        c.call(Some(run), "", "missing", 4);
        let files = [init.done(), core.done(), util.done(), main.done(), a.done(), b.done(), c.done()];
        let g = Graph::build(&files);
        assert_eq!(calls(&g, "go"), ["lib/core.py::build_all [5] E", "lib/util.py::helper [6] E"]);
        assert_eq!(link_texts(&g, &g.loose_callers("missing")), ["cyc/c.py::run [4] I"]);
    }

    #[test]
    fn a_long_chain_of_star_imports_does_not_overflow_the_stack() {
        let py = Lang::Python;
        let links = 100_000;
        let mut files = Vec::with_capacity(links + 1);
        for i in 0..links {
            let mut link = Fx::new(&format!("chain/m{i:06}.py"), py);
            if i + 1 < links {
                link.import(&format!("m{:06}", i + 1), "*", "*", 1, 1);
            } else {
                link.func("leaf", 1, 3);
            }
            files.push(link.done());
        }
        let mut main = Fx::new("chain/z_main.py", py);
        main.import("m000000", "leaf", "leaf", 1, 1);
        let go = main.func("go", 3, 6);
        main.call(Some(go), "", "leaf", 4);
        files.push(main.done());
        let g = Graph::build(&files);
        assert_eq!(calls(&g, "go"), ["chain/m099999.py::leaf [4] E"]);
    }

    #[test]
    fn a_file_reached_again_under_another_name_is_searched_again() {
        let py = Lang::Python;
        // main asks a for x; a renames b.y to x; b renames a.z to y; a defines z itself
        let mut a = Fx::new("a.py", py);
        a.func("z", 1, 2);
        a.import("b", "y", "x", 0, 4);
        let mut b = Fx::new("b.py", py);
        b.import("a", "z", "y", 0, 1);
        let mut main = Fx::new("main.py", py);
        main.import("a", "x", "x", 0, 1);
        let go = main.func("go", 3, 5);
        main.call(Some(go), "", "x", 4);
        let g = Graph::build(&[a.done(), b.done(), main.done()]);
        assert_eq!(calls(&g, "go"), ["a.py::z [4] E"]);
    }

    #[test]
    fn a_default_imported_class_answers_for_its_members_before_the_file_does() {
        let mut user = Fx::new("src/user.js", Lang::JavaScript);
        let class = user.class("User", &[], 1, 10);
        user.method(class, "save", 2, 4);
        user.func("save", 12, 14);
        let mut app = Fx::new("src/app.js", Lang::JavaScript);
        app.import("./user", "default", "User", 0, 1);
        let main = app.func("main", 3, 6);
        app.call(Some(main), "User", "save", 4);
        let g = Graph::build(&[app.done(), user.done()]);
        assert_eq!(calls(&g, "main"), ["src/user.js::User.save [4] E"]);
    }

    #[test]
    fn a_default_import_of_a_file_without_a_class_still_answers_for_its_top_level_names() {
        let mut util = Fx::new("src/util.js", Lang::JavaScript);
        util.func("format", 1, 3);
        let mut app = Fx::new("src/app.js", Lang::JavaScript);
        app.import("./util", "default", "util", 0, 1);
        let main = app.func("main", 3, 6);
        app.call(Some(main), "util", "format", 4);
        let g = Graph::build(&[app.done(), util.done()]);
        assert_eq!(calls(&g, "main"), ["src/util.js::format [4] E"]);
    }

    #[test]
    fn crate_paths_in_main_rs_reach_main_rs_and_everyone_else_reaches_lib_rs() {
        let rs = Lang::Rust;
        let mut lib = Fx::new("src/lib.rs", rs);
        lib.func("f", 1, 3);
        let run = lib.func("run", 5, 8);
        lib.call(Some(run), "crate", "f", 6);
        let mut main = Fx::new("src/main.rs", rs);
        main.func("f", 1, 3);
        let start = main.func("start", 5, 8);
        main.call(Some(start), "crate", "f", 6);
        let g = Graph::build(&[lib.done(), main.done()]);
        assert_eq!(calls(&g, "run"), ["src/lib.rs::f [6] E"]);
        assert_eq!(calls(&g, "start"), ["src/main.rs::f [6] E"]);
    }

    #[test]
    fn a_receiver_that_only_starts_with_super_is_not_the_superclass() {
        let py = Lang::Python;
        let mut sup = Fx::new("supervisor.py", py);
        sup.func("save", 1, 3);
        let mut child = Fx::new("child.py", py);
        child.import("supervisor", "", "supervisor", 0, 1);
        let base = child.class("Base", &[], 3, 6);
        child.method(base, "save", 4, 6);
        let kid = child.class("Child", &["Base"], 8, 14);
        let run = child.method(kid, "run", 9, 14);
        child.call(Some(run), "supervisor", "save", 10);
        child.call(Some(run), "super()", "save", 11);
        let g = Graph::build(&[child.done(), sup.done()]);
        assert_eq!(calls(&g, "Child.run"), ["child.py::Base.save [11] E", "supervisor.py::save [10] E"]);
    }

    #[test]
    fn a_later_python_import_of_a_name_shadows_an_earlier_one() {
        let py = Lang::Python;
        let mut a = Fx::new("a.py", py);
        a.func("f", 1, 2);
        let mut b = Fx::new("b.py", py);
        b.func("f", 1, 2);
        // both imports sit in the calling file
        let mut direct = Fx::new("direct.py", py);
        direct.import("a", "f", "f", 0, 1);
        direct.import("b", "f", "f", 0, 2);
        let go = direct.func("go", 4, 6);
        direct.call(Some(go), "", "f", 5);
        // both imports sit in a file that the caller imports the name from
        let mut hub = Fx::new("hub.py", py);
        hub.import("a", "f", "f", 0, 1);
        hub.import("b", "f", "f", 0, 2);
        let mut user = Fx::new("user.py", py);
        user.import("hub", "f", "f", 0, 1);
        let run = user.func("run", 3, 5);
        user.call(Some(run), "", "f", 4);
        let g = Graph::build(&[a.done(), b.done(), direct.done(), hub.done(), user.done()]);
        assert_eq!(calls(&g, "go"), ["b.py::f [5] E"]);
        assert_eq!(calls(&g, "run"), ["b.py::f [4] E"]);
    }

    #[test]
    fn an_inherited_method_is_found_however_long_the_chain_of_bases() {
        let depth = 100;
        let mut f = Fx::new("chain.py", Lang::Python);
        let root = f.class("C0", &[], 1, 3);
        f.method(root, "lookup", 2, 3);
        let mut last = root;
        for i in 1..=depth {
            let base = format!("C{}", i - 1);
            last = f.class(&format!("C{i}"), &[base.as_str()], 3 * i + 1, 3 * i + 3);
        }
        let run = f.method(last, "run", 3 * depth + 4, 3 * depth + 6);
        f.call(Some(run), "self", "lookup", 3 * depth + 5);
        let g = Graph::build(&[f.done()]);
        assert_eq!(calls(&g, &format!("C{depth}.run")), [format!("chain.py::C0.lookup [{}] E", 3 * depth + 5)]);
    }

    #[test]
    fn duplicate_paths_with_equal_metadata_give_one_graph_in_any_order() {
        let mut x = Fx::new("dup.py", Lang::Python);
        x.func("alpha", 1, 3);
        let mut y = Fx::new("dup.py", Lang::Python);
        y.func("beta", 1, 3);
        let (x, y) = (x.done(), y.done());
        assert_eq!((x.hash, x.bytes, x.lines), (y.hash, y.bytes, y.lines));
        let forward = Graph::build(&[x.clone(), y.clone()]);
        let backward = Graph::build(&[y, x]);
        assert!(forward == backward, "the surviving definitions depend on the input order");
    }

    #[test]
    fn a_class_body_is_not_a_scope_in_python_but_is_one_in_java() {
        let mut py = Fx::new("p.py", Lang::Python);
        let first = py.class("P1", &[], 1, 6);
        py.method(first, "f", 2, 3);
        let g_method = py.method(first, "g", 4, 6);
        let second = py.class("P2", &[], 8, 10);
        py.method(second, "f", 9, 10);
        py.call(Some(g_method), "", "f", 5);
        let g = Graph::build(&[py.done()]);
        // two methods named f and no class scope: the bare name is ambiguous in one file
        assert!(g.callees(one(&g, "P1.g")).is_empty());
        assert_eq!(g.loose.len(), 1);

        let mut java = Fx::new("Calc.java", Lang::Java);
        let calc = java.class("Calc", &[], 1, 12);
        java.method(calc, "add", 2, 4);
        let run = java.method(calc, "run", 5, 8);
        let sub = java.class("Sub", &["Calc"], 14, 20);
        let go = java.method(sub, "go", 15, 18);
        java.method(sub, "add", 19, 20);
        java.call(Some(run), "", "add", 6);
        java.call(Some(go), "", "run", 16);
        let g = Graph::build(&[java.done()]);
        assert_eq!(link_texts(&g, &g.callees(one(&g, "Calc.run"))), ["Calc.java::Calc.add [6] E"]);
        assert_eq!(link_texts(&g, &g.callees(one(&g, "Sub.go"))), ["Calc.java::Calc.run [16] E"]);
    }

    #[test]
    fn malformed_facts_never_panic() {
        let mut f = Fx::new("pkg/bad.py", Lang::Python);
        f.sym(SymKind::Function, "a", Some(5), 1, 2);
        f.sym(SymKind::Method, "b", Some(1), 3, 4);
        f.sym(SymKind::Class, "C", None, 5, 6);
        f.call(Some(9), "", "a", 10);
        f.call(Some(0), "", "", 11);
        f.call(Some(1), "self", "a", 12);
        f.call(None, "super", "zzz", 13);
        f.import("", "", "", 9, 1);
        f.import("..", "x", "x", 3, 2);
        f.import("a.b", "*", "*", 0, 3);
        f.import("crate::x", "", "", 0, 4);
        let mut r = Fx::new("src/lib.rs", Lang::Rust);
        r.import("", "", "", 0, 1);
        r.import("super::super::super", "x", "x", 0, 2);
        r.import("::::", "*", "", 0, 3);
        let mut j = Fx::new("a/b.js", Lang::JavaScript);
        j.import("../../../x", "y", "y", 0, 1);
        j.import(".", "", "", 0, 2);
        let g = Graph::build(&[f.done(), r.done(), j.done()]);
        // the call from a caller that does not exist counts as module level; the call through
        // `self` inside a method with no class around it falls to the unique name
        let a = one(&g, "a");
        assert_eq!(link_texts(&g, &g.callers(a)), ["pkg/bad.py::pkg/bad.py [10] E", "pkg/bad.py::b [12] I"]);
        assert_eq!(g.stats().symbols, 3);
    }

    #[test]
    fn a_graph_of_twenty_thousand_symbols_builds_saves_loads_and_answers_fast() {
        use std::time::Instant;
        let files = synthetic_project(20_000, 80_000);
        let t = Instant::now();
        let g = Graph::build(&files);
        let build = t.elapsed();
        let s = g.stats();
        assert_eq!(s.symbols, 20_000);
        assert_eq!(s.calls_extracted + s.calls_inferred + s.calls_loose, 80_000);

        let dir = std::env::temp_dir().join(format!("lxr-graph-perf-{}", std::process::id()));
        let path = dir.join("graph.bin");
        let t = Instant::now();
        g.save(&path).unwrap();
        let save = t.elapsed();
        let t = Instant::now();
        let back = Graph::load(&path).unwrap();
        let load = t.elapsed();
        let bytes = std::fs::metadata(&path).unwrap().len();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(back == g);

        let busiest = g.hubs(1)[0].0;
        let t = Instant::now();
        let callers = g.callers(busiest);
        let callers_time = t.elapsed();
        let t = Instant::now();
        let reach = g.affected(busiest, 3);
        let affected_time = t.elapsed();
        let t = Instant::now();
        let answer = g.query("shared_7 fn_3000", 2000);
        let query_time = t.elapsed();
        let t = Instant::now();
        let groups = g.communities();
        let communities_time = t.elapsed();
        println!(
            "PERF symbols=20000 calls=80000 files={} edges={} loose={} extracted={} inferred={} bytes={}",
            g.files.len(),
            g.edges.len(),
            s.calls_loose,
            s.calls_extracted,
            s.calls_inferred,
            bytes
        );
        println!(
            "PERF build={:?} save={:?} load={:?} callers={:?} ({} callers) affected={:?} ({} nodes)",
            build,
            save,
            load,
            callers_time,
            callers.len(),
            affected_time,
            reach.len()
        );
        println!(
            "PERF query={:?} ({} nodes kept of {}) communities={:?} ({} groups)",
            query_time,
            answer.nodes.len(),
            answer.found,
            communities_time,
            groups.len()
        );
        assert!(!callers.is_empty());
        assert!(build.as_secs() < 120, "build took {build:?}");
    }
}
