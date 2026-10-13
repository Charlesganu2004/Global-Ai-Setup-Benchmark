//! Turning the name of a module into the file that holds it, for Python, JavaScript and
//! TypeScript, and Rust. A name that no file of the repository answers to gives `None`.

use lxr_core::{FileFacts, Lang};
use std::cmp::Reverse;
use std::collections::HashMap;

/// Extensions a JavaScript or TypeScript import may leave out, in the order they are tried.
const JS_EXTS: [&str; 6] = [".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];

/// True for the languages that share one module system.
pub(crate) fn is_js(lang: Lang) -> bool {
    matches!(lang, Lang::JavaScript | Lang::Jsx | Lang::TypeScript | Lang::Tsx)
}

/// The folder part of a path, without a trailing slash. Empty for a file in the root.
pub(crate) fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// How many leading folders two folder paths share.
pub(crate) fn shared_dirs(a: &str, b: &str) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    a.split('/').zip(b.split('/')).take_while(|(x, y)| x == y).count()
}

/// Joins two path pieces with a slash, leaving out the slash when one side is empty.
fn join(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("{a}/{b}"),
    }
}

/// The path of a Python file as a module path: no extension, and a package's
/// `__init__` file named by its folder.
fn python_stem(path: &str) -> Option<&str> {
    let stem = path
        .strip_suffix(".py")
        .or_else(|| path.strip_suffix(".pyi"))
        .or_else(|| path.strip_suffix(".pyw"))?;
    let stem = stem.strip_suffix("/__init__").unwrap_or(stem);
    if stem == "__init__" {
        None
    } else {
        Some(stem)
    }
}

/// A crate name in the form a `use` path spells it.
fn crate_name(folder: &str) -> String {
    folder.to_lowercase().replace('-', "_")
}

/// An index over the repository's paths that answers module questions.
pub(crate) struct PathIndex<'a> {
    paths: Vec<&'a str>,
    langs: Vec<Lang>,
    exact: HashMap<&'a str, u32>,
    /// Every trailing run of folders of a Python module path, with the files that end that way.
    python: HashMap<&'a str, Vec<u32>>,
    /// Workspace crates by name, with the folder of their `src`.
    crates: HashMap<String, String>,
}

impl<'a> PathIndex<'a> {
    /// Indexes the files, which must already be in their final order.
    pub(crate) fn new(files: &[&'a FileFacts]) -> PathIndex<'a> {
        let mut index = PathIndex {
            paths: Vec::with_capacity(files.len()),
            langs: Vec::with_capacity(files.len()),
            exact: HashMap::new(),
            python: HashMap::new(),
            crates: HashMap::new(),
        };
        for (i, &file) in files.iter().enumerate() {
            let path: &'a str = file.path.as_str();
            index.paths.push(path);
            index.langs.push(file.lang);
            index.exact.entry(path).or_insert(i as u32);
            if file.lang == Lang::Python {
                if let Some(stem) = python_stem(path) {
                    let mut at = 0;
                    loop {
                        index.python.entry(&stem[at..]).or_default().push(i as u32);
                        match stem[at..].find('/') {
                            Some(slash) => at += slash + 1,
                            None => break,
                        }
                    }
                }
            }
            if file.lang == Lang::Rust {
                if let Some(folder) = path.strip_suffix("/src/lib.rs") {
                    let name = folder.rsplit('/').next().unwrap_or(folder);
                    index.crates.entry(crate_name(name)).or_insert_with(|| format!("{folder}/src"));
                }
            }
        }
        index
    }

    /// The file at exactly this path, when it is in the language family `ok` accepts.
    fn file_at(&self, path: &str, ok: impl Fn(Lang) -> bool) -> Option<u32> {
        let &i = self.exact.get(path)?;
        if ok(self.langs[i as usize]) {
            Some(i)
        } else {
            None
        }
    }

    /// The file a Python import names. `module` is dotted, without leading dots; `level`
    /// is the number of leading dots.
    ///
    /// An absolute name is the file whose path, minus its extension, ends with the name
    /// written as a path, with a package's `__init__` file counting as the package folder.
    /// Several matches: the shortest path wins, then the one that shares the most leading
    /// folders with the importer. A relative name starts at the importer's folder, going
    /// up one folder for each dot after the first.
    pub(crate) fn python_module(&self, importer: u32, module: &str, level: u8) -> Option<u32> {
        let relative = module.replace('.', "/");
        let here = dir_of(self.paths.get(importer as usize)?);
        if level == 0 {
            if relative.is_empty() {
                return None;
            }
            return self.python.get(relative.as_str())?.iter().copied().min_by_key(|&c| {
                let path = self.paths[c as usize];
                (path.len(), Reverse(shared_dirs(dir_of(path), here)), path)
            });
        }
        let mut folders: Vec<&str> = if here.is_empty() { Vec::new() } else { here.split('/').collect() };
        let up = level as usize - 1;
        if up > folders.len() {
            return None;
        }
        folders.truncate(folders.len() - up);
        let stem = join(&folders.join("/"), &relative);
        let is_py = |l: Lang| l == Lang::Python;
        for ext in [".py", ".pyi"] {
            let found = if stem.is_empty() {
                self.file_at(&format!("__init__{ext}"), is_py)
            } else {
                self.file_at(&format!("{stem}{ext}"), is_py).or_else(|| self.file_at(&format!("{stem}/__init__{ext}"), is_py))
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// The file a JavaScript or TypeScript import names. Only a specifier that starts with
    /// a dot is looked up, against the importer's folder: the path as written, then with
    /// each extension, then as a folder with an `index` file. A `.js` specifier also
    /// finds the `.ts` file TypeScript writes it for. A bare specifier is a package
    /// outside the repository.
    pub(crate) fn js_module(&self, importer: u32, specifier: &str) -> Option<u32> {
        let relative = specifier == "." || specifier == ".." || specifier.starts_with("./") || specifier.starts_with("../");
        if !relative {
            return None;
        }
        let here = dir_of(self.paths.get(importer as usize)?);
        let mut parts: Vec<&str> = if here.is_empty() { Vec::new() } else { here.split('/').collect() };
        for piece in specifier.split('/') {
            match piece {
                "" | "." => {}
                ".." => {
                    parts.pop()?;
                }
                other => parts.push(other),
            }
        }
        let base = parts.join("/");
        let mut tries: Vec<String> = vec![base.clone()];
        for ext in [".js", ".jsx", ".mjs", ".cjs"] {
            if let Some(stem) = base.strip_suffix(ext) {
                tries.push(format!("{stem}.ts"));
                tries.push(format!("{stem}.tsx"));
            }
        }
        for ext in JS_EXTS {
            tries.push(format!("{base}{ext}"));
        }
        for ext in JS_EXTS {
            tries.push(join(&base, &format!("index{ext}")));
        }
        tries.iter().find_map(|path| self.file_at(path, is_js))
    }

    /// The folder holding a Rust crate's modules and the module path of `importer` inside it.
    fn rust_place(&self, importer: u32) -> (String, Vec<&str>) {
        let path = self.paths[importer as usize];
        let parts: Vec<&str> = path.split('/').collect();
        let folders: &[&str] = &parts[..parts.len() - 1];
        let file = parts[parts.len() - 1];
        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        let (root, mut module): (String, Vec<&str>) = match folders.iter().rposition(|d| *d == "src") {
            Some(i) => (folders[..=i].join("/"), folders[i + 1..].to_vec()),
            None => (folders.join("/"), Vec::new()),
        };
        // A `mod.rs` is the module named by its folder, and `lib.rs` or `main.rs` at the top is the crate root.
        let is_root_or_mod = stem == "mod" || ((stem == "lib" || stem == "main") && module.is_empty());
        if !is_root_or_mod {
            module.push(stem);
        }
        (root, module)
    }

    /// The file of the Rust module `segments` inside the crate whose modules sit under `root`.
    /// The crate root is `lib.rs`, or `main.rs` when `bin` says the path starts in a binary.
    fn rust_module_file(&self, root: &str, segments: &[&str], bin: bool) -> Option<u32> {
        let is_rs = |l: Lang| l == Lang::Rust;
        if segments.is_empty() {
            let (first, second) = if bin { ("main.rs", "lib.rs") } else { ("lib.rs", "main.rs") };
            return self.file_at(&join(root, first), is_rs).or_else(|| self.file_at(&join(root, second), is_rs));
        }
        let stem = join(root, &segments.join("/"));
        self.file_at(&format!("{stem}.rs"), is_rs).or_else(|| self.file_at(&format!("{stem}/mod.rs"), is_rs))
    }

    /// Tries `base` followed by `segments[from..]`, longest module prefix first. Returns the
    /// file and how many of `segments` it used up. The prefix must reach `base.len() + extra`.
    fn rust_try(
        &self,
        root: &str,
        bin: bool,
        base: &[&str],
        segments: &[&str],
        from: usize,
        extra: usize,
    ) -> Option<(u32, usize)> {
        let mut full: Vec<&str> = base.to_vec();
        full.extend_from_slice(&segments[from..]);
        for k in (base.len() + extra..=full.len()).rev() {
            if let Some(file) = self.rust_module_file(root, &full[..k], bin) {
                return Some((file, from + (k - base.len())));
            }
        }
        None
    }

    /// The file a Rust path names, and how many of its segments name modules; the rest are
    /// items inside that file. The path starts with `crate`, `self`, `super`, a module of the
    /// importer, a top-level module of the crate, or the name of another crate of the
    /// repository.
    pub(crate) fn rust_path(&self, importer: u32, segments: &[&str]) -> Option<(u32, usize)> {
        let first = *segments.first()?;
        if self.paths.get(importer as usize).is_none() {
            return None;
        }
        let (root, here) = self.rust_place(importer);
        // A file that is its crate's `main.rs` has that file for its root, not a `lib.rs` beside it.
        let bin = self.paths[importer as usize] == join(&root, "main.rs");
        match first {
            "crate" => self.rust_try(&root, bin, &[], segments, 1, 0),
            "self" => self.rust_try(&root, bin, &here, segments, 1, 0),
            "super" => {
                let mut base = here;
                let mut used = 0;
                while segments.get(used) == Some(&"super") {
                    base.pop()?;
                    used += 1;
                }
                self.rust_try(&root, bin, &base, segments, used, 0)
            }
            _ => {
                if let Some(found) = self.rust_try(&root, bin, &here, segments, 0, 1) {
                    return Some(found);
                }
                if let Some(found) = self.rust_try(&root, bin, &[], segments, 0, 1) {
                    return Some(found);
                }
                let other = self.crates.get(&crate_name(first))?;
                self.rust_try(other, false, &[], segments, 1, 0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::Fx;

    fn index(paths: &[(&str, Lang)]) -> (Vec<FileFacts>, Vec<String>) {
        let facts: Vec<FileFacts> = paths.iter().map(|(p, l)| Fx::new(p, *l).done()).collect();
        let names = facts.iter().map(|f| f.path.clone()).collect();
        (facts, names)
    }

    fn with_index<R>(paths: &[(&str, Lang)], f: impl FnOnce(&PathIndex, &[String]) -> R) -> R {
        let (facts, names) = index(paths);
        let refs: Vec<&FileFacts> = facts.iter().collect();
        let idx = PathIndex::new(&refs);
        f(&idx, &names)
    }

    #[test]
    fn python_absolute_name_prefers_the_shortest_path_then_the_nearest_folder() {
        let paths = [
            ("a/deep/pkg/util.py", Lang::Python),
            ("a/pkg/util.py", Lang::Python),
            ("b/pkg/util.py", Lang::Python),
            ("pkg/__init__.py", Lang::Python),
            ("main.py", Lang::Python),
        ];
        with_index(&paths, |idx, names| {
            let at = |p: &str| names.iter().position(|n| n == p).unwrap() as u32;
            // "a/pkg/util.py" and "b/pkg/util.py" tie on length; the importer in a/ is nearer to the first.
            assert_eq!(idx.python_module(at("a/deep/pkg/util.py"), "pkg.util", 0), Some(at("a/pkg/util.py")));
            assert_eq!(idx.python_module(at("b/pkg/util.py"), "pkg.util", 0), Some(at("b/pkg/util.py")));
            assert_eq!(idx.python_module(at("main.py"), "pkg.util", 0), Some(at("a/pkg/util.py")));
            // a package is its __init__ file
            assert_eq!(idx.python_module(at("main.py"), "pkg", 0), Some(at("pkg/__init__.py")));
            assert_eq!(idx.python_module(at("main.py"), "nothing.here", 0), None);
            assert_eq!(idx.python_module(at("main.py"), "", 0), None);
        });
    }

    #[test]
    fn python_relative_import_starts_at_the_importer_folder() {
        let paths = [
            ("app/services/store.py", Lang::Python),
            ("app/services/cache.py", Lang::Python),
            ("app/services/__init__.py", Lang::Python),
            ("app/util.py", Lang::Python),
            ("app/__init__.py", Lang::Python),
            ("top.py", Lang::Python),
        ];
        with_index(&paths, |idx, names| {
            let at = |p: &str| names.iter().position(|n| n == p).unwrap() as u32;
            let store = at("app/services/store.py");
            assert_eq!(idx.python_module(store, "cache", 1), Some(at("app/services/cache.py")));
            assert_eq!(idx.python_module(store, "util", 2), Some(at("app/util.py")));
            assert_eq!(idx.python_module(store, "", 1), Some(at("app/services/__init__.py")));
            assert_eq!(idx.python_module(store, "", 2), Some(at("app/__init__.py")));
            assert_eq!(idx.python_module(store, "util", 1), None);
            assert_eq!(idx.python_module(store, "util", 9), None);
            // a root file's folder is the repository root, so its relative names start there
            assert_eq!(idx.python_module(at("top.py"), "app.util", 1), Some(at("app/util.py")));
            assert_eq!(idx.python_module(at("top.py"), "missing", 1), None);
            assert_eq!(idx.python_module(at("top.py"), "x", 2), None);
        });
    }

    #[test]
    fn javascript_relative_specifiers_try_extensions_then_index_files() {
        let paths = [
            ("src/app.js", Lang::JavaScript),
            ("src/util.ts", Lang::TypeScript),
            ("src/lib/index.js", Lang::JavaScript),
            ("src/lib/deep/x.mjs", Lang::JavaScript),
            ("src/data.js", Lang::JavaScript),
            ("shared.js", Lang::JavaScript),
            ("src/native.py", Lang::Python),
        ];
        with_index(&paths, |idx, names| {
            let at = |p: &str| names.iter().position(|n| n == p).unwrap() as u32;
            let app = at("src/app.js");
            assert_eq!(idx.js_module(app, "./util"), Some(at("src/util.ts")));
            assert_eq!(idx.js_module(app, "./util.js"), Some(at("src/util.ts")));
            assert_eq!(idx.js_module(app, "./lib"), Some(at("src/lib/index.js")));
            assert_eq!(idx.js_module(app, "./lib/deep/x.mjs"), Some(at("src/lib/deep/x.mjs")));
            assert_eq!(idx.js_module(app, "./data.js"), Some(at("src/data.js")));
            assert_eq!(idx.js_module(app, "../shared"), Some(at("shared.js")));
            assert_eq!(idx.js_module(app, "../../shared"), None);
            assert_eq!(idx.js_module(app, "react"), None);
            assert_eq!(idx.js_module(app, "./native"), None);
            assert_eq!(idx.js_module(at("src/lib/deep/x.mjs"), ".."), Some(at("src/lib/index.js")));
        });
    }

    #[test]
    fn rust_paths_resolve_from_the_nearest_src_folder() {
        let paths = [
            ("crates/a/src/lib.rs", Lang::Rust),
            ("crates/a/src/store.rs", Lang::Rust),
            ("crates/a/src/store/page.rs", Lang::Rust),
            ("crates/a/src/net/mod.rs", Lang::Rust),
            ("crates/a/src/net/wire.rs", Lang::Rust),
            ("crates/b/src/lib.rs", Lang::Rust),
            ("crates/b/src/model.rs", Lang::Rust),
            ("tools/loose.rs", Lang::Rust),
            ("tools/helper.rs", Lang::Rust),
        ];
        with_index(&paths, |idx, names| {
            let at = |p: &str| names.iter().position(|n| n == p).unwrap() as u32;
            let lib = at("crates/a/src/lib.rs");
            let page = at("crates/a/src/store/page.rs");
            let wire = at("crates/a/src/net/wire.rs");
            // crate:: with the item name trimmed off
            assert_eq!(idx.rust_path(page, &["crate", "store", "open"]), Some((at("crates/a/src/store.rs"), 2)));
            assert_eq!(idx.rust_path(page, &["crate", "net", "wire"]), Some((wire, 3)));
            assert_eq!(idx.rust_path(page, &["crate", "net"]), Some((at("crates/a/src/net/mod.rs"), 2)));
            // crate:: alone is the crate root; an item right under it stays an item
            assert_eq!(idx.rust_path(page, &["crate", "Thing"]), Some((lib, 1)));
            // super:: from a nested file, self:: from a module file
            assert_eq!(idx.rust_path(page, &["super", "helper_fn"]), Some((at("crates/a/src/store.rs"), 1)));
            assert_eq!(idx.rust_path(wire, &["super", "Frame"]), Some((at("crates/a/src/net/mod.rs"), 1)));
            assert_eq!(idx.rust_path(at("crates/a/src/net/mod.rs"), &["self", "wire", "send"]), Some((wire, 2)));
            assert_eq!(idx.rust_path(at("crates/a/src/store.rs"), &["self", "page"]), Some((page, 2)));
            // a bare name: a child module first, then a top-level module of the crate
            assert_eq!(idx.rust_path(lib, &["store", "page"]), Some((page, 2)));
            assert_eq!(idx.rust_path(page, &["net", "wire", "send"]), Some((wire, 2)));
            // mod x; beside the file, and the importer's own folder when there is no src folder
            assert_eq!(idx.rust_path(at("tools/loose.rs"), &["helper"]), Some((at("tools/helper.rs"), 1)));
            // another crate of the repository by name, and an item re-exported at its root
            assert_eq!(idx.rust_path(page, &["b", "model", "Row"]), Some((at("crates/b/src/model.rs"), 2)));
            assert_eq!(idx.rust_path(page, &["b", "Row"]), Some((at("crates/b/src/lib.rs"), 1)));
            // outside the repository
            assert_eq!(idx.rust_path(page, &["std", "fmt"]), None);
            assert_eq!(idx.rust_path(page, &["serde", "Serialize"]), None);
            assert_eq!(idx.rust_path(page, &[]), None);
            assert_eq!(idx.rust_path(at("tools/loose.rs"), &["super", "super", "x"]), None);
        });
    }

    #[test]
    fn rust_crate_root_is_main_rs_for_main_rs_and_lib_rs_for_the_other_files() {
        let paths = [
            ("src/lib.rs", Lang::Rust),
            ("src/main.rs", Lang::Rust),
            ("src/util.rs", Lang::Rust),
            ("tool/src/main.rs", Lang::Rust),
        ];
        with_index(&paths, |idx, names| {
            let at = |p: &str| names.iter().position(|n| n == p).unwrap() as u32;
            assert_eq!(idx.rust_path(at("src/main.rs"), &["crate", "Thing"]), Some((at("src/main.rs"), 1)));
            assert_eq!(idx.rust_path(at("src/main.rs"), &["self", "Thing"]), Some((at("src/main.rs"), 1)));
            assert_eq!(idx.rust_path(at("src/lib.rs"), &["crate", "Thing"]), Some((at("src/lib.rs"), 1)));
            assert_eq!(idx.rust_path(at("src/util.rs"), &["crate", "Thing"]), Some((at("src/lib.rs"), 1)));
            // a crate with a main.rs alone has no other root to prefer
            assert_eq!(idx.rust_path(at("tool/src/main.rs"), &["crate", "Thing"]), Some((at("tool/src/main.rs"), 1)));
        });
    }
}
