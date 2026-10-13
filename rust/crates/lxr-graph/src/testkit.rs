//! Hand-built files for the tests: a small builder, and the projects the tests share.

use crate::{Confidence, Graph, Link};
use lxr_core::{hash64, Call, FileFacts, Import, Lang, ParseGrade, SymKind, Symbol};

/// Builds the facts of one file, one definition, call or import at a time.
pub struct Fx {
    facts: FileFacts,
}

impl Fx {
    /// A file with no content yet. It claims 40 lines.
    pub fn new(path: &str, lang: Lang) -> Fx {
        Fx {
            facts: FileFacts {
                path: path.to_string(),
                lang,
                lines: 40,
                bytes: 1000,
                hash: hash64(path.as_bytes()),
                grade: ParseGrade::Ast,
                symbols: Vec::new(),
                calls: Vec::new(),
                imports: Vec::new(),
            },
        }
    }

    /// Adds a definition and returns its index in the file.
    pub fn sym(&mut self, kind: SymKind, name: &str, parent: Option<u32>, start: u32, end: u32) -> u32 {
        let qual = match parent.and_then(|p| self.facts.symbols.get(p as usize)) {
            Some(container) => format!("{}.{}", container.qual, name),
            None => name.to_string(),
        };
        let hash = hash64(format!("{}:{}:{}:{}", self.facts.path, qual, start, end).as_bytes());
        self.facts.symbols.push(Symbol {
            name: name.to_string(),
            sig: format!("{} {}", kind.word(), qual),
            qual,
            kind,
            start,
            end,
            doc: String::new(),
            parent,
            bases: Vec::new(),
            hash,
        });
        (self.facts.symbols.len() - 1) as u32
    }

    /// Adds a top-level function.
    pub fn func(&mut self, name: &str, start: u32, end: u32) -> u32 {
        self.sym(SymKind::Function, name, None, start, end)
    }

    /// Adds a class with the given base names.
    pub fn class(&mut self, name: &str, bases: &[&str], start: u32, end: u32) -> u32 {
        let i = self.sym(SymKind::Class, name, None, start, end);
        self.facts.symbols[i as usize].bases = bases.iter().map(|b| b.to_string()).collect();
        i
    }

    /// Adds a method of the container `class`.
    pub fn method(&mut self, class: u32, name: &str, start: u32, end: u32) -> u32 {
        self.sym(SymKind::Method, name, Some(class), start, end)
    }

    /// Adds a function defined inside another definition.
    pub fn nested(&mut self, parent: u32, name: &str, start: u32, end: u32) -> u32 {
        self.sym(SymKind::Function, name, Some(parent), start, end)
    }

    /// Gives the definition `symbol` a documentation line.
    pub fn doc(&mut self, symbol: u32, text: &str) -> &mut Fx {
        self.facts.symbols[symbol as usize].doc = text.to_string();
        self
    }

    /// Adds a call. `caller` is a definition of this file, or `None` at module level.
    pub fn call(&mut self, caller: Option<u32>, recv: &str, name: &str, line: u32) -> &mut Fx {
        self.facts.calls.push(Call { caller, name: name.to_string(), recv: recv.to_string(), line });
        self
    }

    /// Adds an import.
    pub fn import(&mut self, module: &str, name: &str, alias: &str, level: u8, line: u32) -> &mut Fx {
        self.facts.imports.push(Import {
            module: module.to_string(),
            name: name.to_string(),
            alias: alias.to_string(),
            level,
            line,
        });
        self
    }

    /// The finished facts.
    pub fn done(self) -> FileFacts {
        self.facts
    }
}

/// A Python-like project that uses every resolution rule. Files, in path order:
/// `app/__init__.py`, `app/extras.py`, `app/helpers.py`, `app/main.py`, `app/models.py`,
/// `app/services/__init__.py`, `app/services/cache.py`, `app/services/store.py`,
/// `app/util.py`, `app/views.py`, `pkg_a/x.py`, `pkg_b/y.py`, `tests/helpers.py`,
/// `tests/test_views.py`, `tools/format.py`.
pub fn py_project() -> Vec<FileFacts> {
    let py = Lang::Python;
    let mut files = Vec::new();

    files.push(Fx::new("app/__init__.py", py).done());

    let mut f = Fx::new("app/extras.py", py);
    f.import("app.models", "", "models", 0, 1);
    let guest = f.class("Guest", &["models.User"], 3, 8);
    let greet = f.method(guest, "greet", 4, 6);
    f.class("Child", &["UniqueBase"], 10, 12);
    f.class("Weird", &["Missing"], 14, 16);
    f.call(Some(greet), "self", "audit", 5);
    files.push(f.done());

    let mut f = Fx::new("app/helpers.py", py);
    f.func("slugify", 1, 3);
    files.push(f.done());

    let mut f = Fx::new("app/main.py", py);
    f.import("app.util", "parse_args", "pa", 0, 1);
    f.import("helpers", "slugify", "slugify", 1, 2);
    f.import("app.util", "", "util", 0, 3);
    f.import("app.models", "", "m", 0, 4);
    f.import("app.services", "store", "store", 0, 5);
    f.import("app.models", "User", "User", 0, 6);
    let run = f.func("run", 8, 26);
    f.func("local_fn", 28, 29);
    let r = Some(run);
    f.call(r, "", "pa", 9);
    f.call(r, "", "slugify", 10);
    f.call(r, "", "local_fn", 11);
    f.call(r, "app.util", "parse_args", 12);
    f.call(r, "m.User", "create", 13);
    f.call(r, "store", "open_store", 14);
    f.call(r, "", "unique_fn", 15);
    f.call(r, "", "render", 16);
    f.call(r, "", "fmt", 17);
    f.call(r, "obj", "unique_method", 18);
    f.call(r, "obj", "get", 19);
    f.call(r, "obj", "twice", 20);
    f.call(r, "", "User", 21);
    f.call(r, "", "local_fn", 22);
    f.call(r, "", "isinstance", 23);
    f.call(r, "Admin", "save", 24);
    f.call(None, "", "run", 30);
    f.call(None, "", "print", 31);
    files.push(f.done());

    let mut f = Fx::new("app/models.py", py);
    let base = f.class("Base", &[], 1, 10);
    f.method(base, "save", 2, 4);
    f.method(base, "validate", 5, 7);
    f.method(base, "make", 8, 10);
    let user = f.class("User", &["Base"], 12, 25);
    let update = f.method(user, "update", 13, 16);
    f.method(user, "audit", 17, 19);
    let create = f.method(user, "create", 20, 22);
    let walk = f.method(user, "walk", 23, 25);
    let admin = f.class("Admin", &["User"], 27, 35);
    let admin_update = f.method(admin, "update", 28, 31);
    let admin_run = f.method(admin, "run", 32, 35);
    f.call(Some(update), "self", "save", 14);
    f.call(Some(update), "self", "audit", 15);
    f.call(Some(create), "cls", "make", 21);
    f.call(Some(walk), "self", "walk", 24);
    f.call(Some(admin_update), "super()", "update", 29);
    f.call(Some(admin_update), "self", "save", 30);
    f.call(Some(admin_run), "self", "validate", 33);
    files.push(f.done());

    files.push(Fx::new("app/services/__init__.py", py).done());

    let mut f = Fx::new("app/services/cache.py", py);
    f.func("warm", 1, 3);
    f.func("flush", 5, 6);
    files.push(f.done());

    let mut f = Fx::new("app/services/store.py", py);
    f.import("util", "parse_args", "parse_args", 2, 1);
    f.import("models", "User", "User", 2, 2);
    f.import("", "cache", "cache", 1, 3);
    let open_store = f.func("open_store", 5, 12);
    let close_store = f.func("close_store", 14, 16);
    let o = Some(open_store);
    f.call(o, "", "parse_args", 6);
    f.call(o, "", "User", 7);
    f.call(o, "cache", "warm", 8);
    f.call(o, "User", "create", 9);
    f.call(o, "User", "save", 10);
    f.call(Some(close_store), "cache", "flush", 15);
    files.push(f.done());

    let mut f = Fx::new("app/util.py", py);
    f.func("parse_args", 3, 6);
    let outer = f.func("outer", 8, 16);
    f.nested(outer, "inner_a", 9, 10);
    let inner_b = f.nested(outer, "inner_b", 11, 13);
    let fact = f.func("fact", 18, 21);
    f.call(Some(inner_b), "", "inner_a", 12);
    f.call(Some(outer), "", "inner_b", 14);
    f.call(Some(outer), "", "inner_a", 15);
    f.call(Some(fact), "", "fact", 20);
    f.call(None, "", "parse_args", 30);
    files.push(f.done());

    let mut f = Fx::new("app/views.py", py);
    f.func("render", 1, 4);
    files.push(f.done());

    let mut f = Fx::new("pkg_a/x.py", py);
    f.func("fmt", 1, 2);
    files.push(f.done());

    let mut f = Fx::new("pkg_b/y.py", py);
    f.func("fmt", 1, 2);
    files.push(f.done());

    let mut f = Fx::new("tests/helpers.py", py);
    f.func("render", 1, 3);
    files.push(f.done());

    let mut f = Fx::new("tests/test_views.py", py);
    let test_render = f.func("test_render", 1, 5);
    f.call(Some(test_render), "", "render", 3);
    files.push(f.done());

    let mut f = Fx::new("tools/format.py", py);
    f.func("unique_fn", 1, 3);
    let report = f.class("Report", &[], 5, 12);
    f.method(report, "unique_method", 6, 8);
    f.method(report, "twice", 9, 12);
    let summary = f.class("Summary", &[], 14, 18);
    f.method(summary, "twice", 15, 18);
    f.func("render", 20, 22);
    f.class("UniqueBase", &[], 24, 26);
    files.push(f.done());

    files
}

/// A JavaScript and TypeScript project. Files, in path order: `src/app.js`,
/// `src/lib/index.js`, `src/models/user.js`, `src/util.ts`.
pub fn js_project() -> Vec<FileFacts> {
    let mut files = Vec::new();

    let mut f = Fx::new("src/app.js", Lang::JavaScript);
    f.import("./util", "format", "fmt", 0, 1);
    f.import("./lib", "default", "lib", 0, 2);
    f.import("./models/user", "*", "users", 0, 3);
    f.import("react", "default", "React", 0, 4);
    f.import("./util", "", "util", 0, 5);
    let main = f.func("main", 7, 20);
    let m = Some(main);
    f.call(m, "", "fmt", 8);
    f.call(m, "", "lib", 9);
    f.call(m, "users", "createUser", 10);
    f.call(m, "util", "format", 11);
    f.call(m, "React", "render", 12);
    f.call(m, "User", "create", 13);
    files.push(f.done());

    let mut f = Fx::new("src/lib/index.js", Lang::JavaScript);
    f.func("connect", 1, 4);
    files.push(f.done());

    let mut f = Fx::new("src/models/user.js", Lang::JavaScript);
    let user = f.class("User", &[], 1, 10);
    f.method(user, "create", 2, 4);
    f.func("createUser", 12, 14);
    files.push(f.done());

    let mut f = Fx::new("src/util.ts", Lang::TypeScript);
    f.func("format", 1, 3);
    files.push(f.done());

    files
}

/// A Rust workspace of two crates. Files, in path order: `crates/app/src/cli.rs`,
/// `crates/app/src/main.rs`, `crates/store/src/lib.rs`, `crates/store/src/model.rs`,
/// `crates/store/src/page.rs`.
pub fn rust_project() -> Vec<FileFacts> {
    let rs = Lang::Rust;
    let mut files = Vec::new();

    let mut f = Fx::new("crates/app/src/cli.rs", rs);
    f.func("parse", 1, 3);
    files.push(f.done());

    let mut f = Fx::new("crates/app/src/main.rs", rs);
    f.import("store", "open", "open", 0, 1);
    f.import("store::model", "Row", "Row", 0, 2);
    f.import("store", "Row", "R", 0, 3);
    f.import("crate::cli", "", "cli", 0, 4);
    let main = f.func("main", 6, 20);
    let m = Some(main);
    f.call(m, "", "open", 7);
    f.call(m, "Row", "new", 8);
    f.call(m, "R", "describe", 9);
    f.call(m, "cli", "parse", 10);
    f.call(m, "crate::cli", "parse", 11);
    files.push(f.done());

    let mut f = Fx::new("crates/store/src/lib.rs", rs);
    f.import("model", "", "model", 0, 1);
    f.import("model", "Row", "Row", 0, 2);
    f.func("open", 4, 6);
    files.push(f.done());

    let mut f = Fx::new("crates/store/src/model.rs", rs);
    f.sym(SymKind::Struct, "Row", None, 1, 5);
    let imp = f.sym(SymKind::Impl, "Row", None, 7, 15);
    f.method(imp, "new", 8, 10);
    let describe = f.method(imp, "describe", 11, 14);
    f.call(Some(describe), "Self", "new", 12);
    files.push(f.done());

    let mut f = Fx::new("crates/store/src/page.rs", rs);
    f.import("super::model", "Row", "Row", 0, 1);
    let render = f.func("render", 1, 5);
    f.call(Some(render), "Row", "new", 2);
    f.call(Some(render), "super", "open", 3);
    files.push(f.done());

    files
}

/// Three small Python files for the query tests. Files: `config.py`, `render.py`, `server.py`.
pub fn query_project() -> Vec<FileFacts> {
    let py = Lang::Python;
    let mut files = Vec::new();

    let mut f = Fx::new("config.py", py);
    let config = f.class("Config", &[], 1, 20);
    let load = f.method(config, "load", 2, 6);
    f.doc(load, "Read the settings file.");
    f.method(config, "parse_config", 8, 12);
    f.call(Some(load), "self", "parse_config", 4);
    files.push(f.done());

    let mut f = Fx::new("render.py", py);
    let page = f.func("render_page", 1, 8);
    f.func("render_header", 10, 14);
    f.call(Some(page), "", "render_header", 3);
    files.push(f.done());

    let mut f = Fx::new("server.py", py);
    f.import("config", "Config", "Config", 0, 1);
    let start = f.func("start_server", 3, 12);
    f.func("parse_args", 14, 18);
    let s = Some(start);
    f.call(s, "", "parse_args", 4);
    f.call(s, "", "Config", 5);
    f.call(s, "Config", "load", 6);
    f.call(s, "", "render_page", 7);
    files.push(f.done());

    files
}

/// Two clusters of four functions that call each other, one call between the clusters,
/// and one function that stands alone. Files: `alpha.py`, `beta.py`, `gamma.py`.
pub fn cluster_project() -> Vec<FileFacts> {
    let py = Lang::Python;
    let mut files = Vec::new();

    let mut f = Fx::new("alpha.py", py);
    let a: Vec<u32> = (1..=4).map(|i| f.func(&format!("a{i}"), i * 10, i * 10 + 5)).collect();
    for (from, to) in [(0, 1), (1, 2), (2, 3), (3, 0), (0, 2), (1, 3)] {
        f.call(Some(a[from]), "", &format!("a{}", to + 1), 100 + from as u32);
    }
    f.call(Some(a[0]), "", "b1", 120);
    files.push(f.done());

    let mut f = Fx::new("beta.py", py);
    let b: Vec<u32> = (1..=4).map(|i| f.func(&format!("b{i}"), i * 10, i * 10 + 5)).collect();
    for (from, to) in [(0, 1), (1, 2), (2, 3), (3, 0), (0, 2), (1, 3)] {
        f.call(Some(b[from]), "", &format!("b{}", to + 1), 100 + from as u32);
    }
    files.push(f.done());

    let mut f = Fx::new("gamma.py", py);
    f.func("lonely", 1, 3);
    files.push(f.done());

    files
}

/// A synthetic project: `symbols` functions spread over files of 50 functions each, and
/// `calls` calls from them. A third of the calls go to a function in the same file, a third
/// to a function that shares a unique name, and a third to a name that several files define.
pub fn synthetic_project(symbols: usize, calls: usize) -> Vec<FileFacts> {
    let per_file = 50;
    let file_count = symbols.div_ceil(per_file);
    let mut files: Vec<Fx> = (0..file_count)
        .map(|i| Fx::new(&format!("pkg{:03}/mod{:04}.py", i / 20, i), Lang::Python))
        .collect();
    let name_of = |global: usize| -> String {
        if global % 7 == 0 {
            format!("shared_{}", global % 40)
        } else {
            format!("fn_{global}")
        }
    };
    for g in 0..symbols {
        let (file, local) = (g / per_file, g % per_file);
        files[file].func(&name_of(g), local as u32 * 4 + 1, local as u32 * 4 + 3);
    }
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for i in 0..calls {
        let caller = (next() % symbols as u64) as usize;
        let (file, local) = (caller / per_file, (caller % per_file) as u32);
        let target = match i % 3 {
            0 => file * per_file + (next() % per_file as u64) as usize,
            1 => (next() % symbols as u64) as usize,
            _ => (next() % 40) as usize * 7,
        };
        let target = target.min(symbols - 1);
        files[file].call(Some(local), "", &name_of(target), local * 4 + 2);
    }
    files.into_iter().map(Fx::done).collect()
}

/// The one node a name stands for. Fails the test when there are none or several.
pub fn one(g: &Graph, name: &str) -> u32 {
    let found = g.find(name);
    assert_eq!(found.len(), 1, "{name} found {found:?}");
    found[0]
}

/// `path::qualified name` of a node.
pub fn label(g: &Graph, node: u32) -> String {
    format!("{}::{}", g.path_of(node), g.nodes[node as usize].qual)
}

/// `label` of each node.
pub fn labels(g: &Graph, nodes: &[u32]) -> Vec<String> {
    nodes.iter().map(|&n| label(g, n)).collect()
}

/// A link as one line: the node, the lines, and `E` for extracted or `I` for inferred.
pub fn link_text(g: &Graph, link: &Link) -> String {
    let conf = if link.conf == Confidence::Extracted { "E" } else { "I" };
    format!("{} {:?} {}", label(g, link.node), link.lines, conf)
}

/// `link_text` of each link.
pub fn link_texts(g: &Graph, links: &[Link]) -> Vec<String> {
    links.iter().map(|l| link_text(g, l)).collect()
}
