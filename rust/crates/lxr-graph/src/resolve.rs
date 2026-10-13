//! The rules that tie a call, or a base class name, to a definition.
//!
//! Every rule gives a confidence. A rule that follows scope, imports or class membership
//! gives `Extracted`. A rule that rests on a name being unique, or nearest, in the
//! repository gives `Inferred`.

use crate::build::{Binding, Builder, Want};
use crate::modules::shared_dirs;
use crate::types::*;
use lxr_core::Lang;
use std::collections::HashSet;

/// Member names so common that a method with one of them elsewhere in the repository
/// says nothing about what an unknown receiver is.
const COMMON_NAMES: &[&str] = &[
    "add", "all", "and_then", "any", "append", "apply", "as_ref", "as_str", "bind", "borrow", "call", "catch", "clear", "clone",
    "close", "collect", "contains", "copy", "count", "decode", "default", "delete", "dump", "dumps", "emit", "encode",
    "endswith", "entries", "err", "exists", "expect", "extend", "filter", "find", "finally", "flush", "forEach", "format",
    "from", "get", "includes", "index", "insert", "into", "is_empty", "items", "iter", "join", "keys", "len", "length", "load",
    "loads", "log", "lower", "map", "map_err", "new", "next", "off", "ok", "on", "open", "pop", "print", "push", "push_str", "put",
    "read", "readline", "readlines", "reduce", "remove", "replace", "reset", "reverse", "send", "seek", "set", "size", "skip",
    "slice", "sort", "sorted", "split", "start", "startswith", "stop", "strip", "take", "then", "to_owned", "to_string",
    "toString", "unwrap", "update", "upper", "valueOf", "values", "write",
];

/// Splits `a.b::c` at its last separator into `("a.b", "c")`. A name with no separator
/// gives `("", name)`.
pub(crate) fn split_last(name: &str) -> (&str, &str) {
    let colon = name.rfind("::").map(|i| (i, 2));
    let dot = name.rfind('.').map(|i| (i, 1));
    let at = match (colon, dot) {
        (Some(a), Some(b)) => Some(if a.0 > b.0 { a } else { b }),
        (a, b) => a.or(b),
    };
    match at {
        Some((i, width)) => (&name[..i], &name[i + width..]),
        None => ("", name),
    }
}

/// True for a plain identifier.
fn is_ident(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// A base class name as a dotted or `::` path: generic arguments and reference marks
/// removed. Empty when the text is not a name.
fn clean_base(base: &str) -> &str {
    let cut = base.find(['<', '[', '(']).unwrap_or(base.len());
    let text = base[..cut].trim().trim_start_matches(['&', '*']).trim();
    let text = text.strip_prefix("dyn ").unwrap_or(text).trim();
    if text.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | ':')) {
        text
    } else {
        ""
    }
}

/// One pending step of a search through a module's imports.
enum Step<'n> {
    /// The name at the top of a file, followed through that file's imports.
    Module(u32, &'n str),
    /// The default export of a file, when an import binds it under `alias`.
    Default(u32, &'n str),
}

impl<'a> Builder<'a> {
    /// The kind of a symbol node.
    fn sym_kind(&self, node: u32) -> Option<lxr_core::SymKind> {
        self.kind.get(node as usize).copied().flatten()
    }

    /// The nearest class-like container around a node, the node itself excluded.
    pub(crate) fn class_of(&self, node: u32) -> Option<u32> {
        let mut at = node;
        while at >= self.nfiles && (at as usize) < self.nodes.len() {
            at = self.up[at as usize];
            if at < self.nfiles || at as usize >= self.nodes.len() {
                return None;
            }
            if self.sym_kind(at).map_or(false, |k| k.is_container()) {
                return Some(at);
            }
        }
        None
    }

    /// The method `name` of a class, then of its bases, breadth first. With `skip_own` the
    /// class's own members are passed over, as for a call through `super`.
    pub(crate) fn find_member(&self, class: u32, name: &str, skip_own: bool) -> Option<u32> {
        let mut queue: Vec<u32> = vec![class];
        let mut seen: HashSet<u32> = HashSet::new();
        let mut at = 0;
        while at < queue.len() {
            let current = queue[at];
            let own_turn = at == 0;
            at += 1;
            let group = self.fam[current as usize];
            let members: &[u32] = if group != NONE { &self.groups[group as usize] } else { std::slice::from_ref(&current) };
            if members.iter().any(|m| seen.contains(m)) {
                continue;
            }
            seen.extend(members.iter().copied());
            if !(skip_own && own_turn) {
                for &m in members {
                    if let Some(defs) = self.scope_defs.get(&(m, name)) {
                        if let Some(&d) = defs.iter().find(|&&d| self.sym_kind(d).map_or(false, |k| k.is_callable())) {
                            return Some(d);
                        }
                    }
                }
            }
            for &m in members {
                queue.extend_from_slice(&self.bases_of[m as usize]);
            }
        }
        None
    }

    /// The file a receiver names as a module: a module bound by an import, or in a Rust
    /// file a path such as `crate::store`.
    pub(crate) fn module_of_receiver(&self, f: usize, recv: &str) -> Option<u32> {
        if let Some(&file) = self.imps[f].modules.get(recv) {
            return Some(file);
        }
        if self.files[f].lang == Lang::Rust {
            let segments: Vec<&str> = recv.split("::").collect();
            if segments.iter().all(|s| !s.is_empty()) {
                let (file, used) = self.index.rust_path(f as u32, &segments)?;
                if used == segments.len() {
                    return Some(file);
                }
            }
        }
        None
    }

    /// The definition `name` at the top of a module's file, followed through re-exports.
    ///
    /// The search goes depth first: a file's own definitions, then its named imports, the
    /// latest first since a later import of a name shadows an earlier one, then its star
    /// imports. It keeps its own stack, so a long chain of re-exports cannot overflow the
    /// call stack. A file is searched once for each name, so a file met again under another
    /// name, through a rename, still gets its turn. `skip` is a file already searched for
    /// `name`, as the importer of a star import is.
    pub(crate) fn lookup_in_module(&self, file: u32, name: &str, want: Want, skip: Option<u32>) -> Option<u32> {
        let mut seen: HashSet<(u32, &str)> = HashSet::new();
        if let Some(done) = skip {
            seen.insert((done, name));
        }
        let mut stack = vec![Step::Module(file, name)];
        while let Some(step) = stack.pop() {
            let (file, name) = match step {
                Step::Default(file, alias) => {
                    if let Some(found) = self.default_export(file, alias, want) {
                        return Some(found);
                    }
                    continue;
                }
                Step::Module(file, name) => (file, name),
            };
            if !seen.insert((file, name)) {
                continue;
            }
            if let Some(defs) = self.scope_defs.get(&(file, name)) {
                if let Some(&d) = defs.iter().find(|&&d| self.sym_kind(d).map_or(false, |k| want.accepts(k))) {
                    return Some(d);
                }
            }
            // What is pushed last is searched first: the named imports, latest first, ahead of the stars.
            let imports = &self.imps[file as usize];
            for &star in imports.stars.iter().rev() {
                stack.push(Step::Module(star, name));
            }
            if let Some(bindings) = imports.names.get(name) {
                for binding in bindings {
                    stack.push(Self::binding_step(binding));
                }
            }
        }
        None
    }

    /// The step that finds the definition an import binding stands for.
    fn binding_step<'b>(binding: &Binding<'b>) -> Step<'b> {
        if binding.orig.is_empty() || binding.orig == "default" {
            Step::Default(binding.file, binding.alias)
        } else {
            Step::Module(binding.file, binding.orig)
        }
    }

    /// The definition an import binding stands for.
    fn resolve_binding(&self, binding: &Binding<'a>, want: Want) -> Option<u32> {
        match Self::binding_step(binding) {
            Step::Default(file, alias) => self.default_export(file, alias, want),
            Step::Module(file, name) => self.lookup_in_module(file, name, want, None),
        }
    }

    /// What a default or whole-module import of a file gives: the definition named like the
    /// import, one named `default`, one named like the file, or the only definition at the top.
    pub(crate) fn default_export(&self, file: u32, alias: &str, want: Want) -> Option<u32> {
        let fits = |d: u32| self.sym_kind(d).map_or(false, |k| want.accepts(k));
        let pick = |name: &str| self.scope_defs.get(&(file, name)).and_then(|defs| defs.iter().copied().find(|&d| fits(d)));
        let path = self.files[file as usize].path.as_str();
        let stem = path.rsplit('/').next().unwrap_or(path);
        let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
        if let Some(d) = pick(alias).or_else(|| pick("default")).or_else(|| pick(stem)) {
            return Some(d);
        }
        let base = self.sym_base[file as usize];
        let mut only = None;
        let mut count = 0;
        for i in 0..self.files[file as usize].symbols.len() as u32 {
            let id = base + i;
            if self.up[id as usize] == file && fits(id) {
                count += 1;
                only = Some(id);
            }
        }
        if count == 1 {
            only
        } else {
            None
        }
    }

    /// A name brought into a file by an import: a named, default or star import.
    fn resolve_imported(&self, f: usize, name: &str, want: Want, exclude: u32) -> Option<u32> {
        let imports = &self.imps[f];
        if let Some(bindings) = imports.names.get(name) {
            // A later import of a name shadows an earlier one.
            for binding in bindings.iter().rev() {
                if let Some(found) = self.resolve_binding(binding, want) {
                    if found != exclude {
                        return Some(found);
                    }
                }
            }
        }
        for &star in &imports.stars {
            if let Some(found) = self.lookup_in_module(star, name, want, Some(f as u32)) {
                if found != exclude {
                    return Some(found);
                }
            }
        }
        None
    }

    /// A definition named `name` that is the only one in the repository, or the best of
    /// several: test files dropped for a caller outside tests, then the same folder, then
    /// the folder that shares the most leading folders with the caller's.
    fn resolve_global(&self, f: usize, name: &str, want: Want, exclude: u32, caller: u32) -> Option<u32> {
        let file_of = |d: u32| self.nodes[d as usize].file as usize;
        let mut cands: Vec<u32> = self
            .global
            .get(name)?
            .iter()
            .copied()
            .filter(|&d| {
                d != exclude
                    && self.sym_kind(d).map_or(false, |k| want.accepts(k))
                    && self.family[file_of(d)] == self.family[f]
            })
            .collect();
        match cands.len() {
            0 => return None,
            1 => return if cands[0] == caller { None } else { Some(cands[0]) },
            _ => {}
        }
        if !self.is_test[f] {
            cands.retain(|&d| !self.is_test[file_of(d)]);
        }
        let here = self.dirs[f];
        let same: Vec<u32> = cands.iter().copied().filter(|&d| self.dirs[file_of(d)] == here).collect();
        if !same.is_empty() {
            cands = same;
        } else {
            let best = cands.iter().map(|&d| shared_dirs(self.dirs[file_of(d)], here)).max()?;
            cands.retain(|&d| shared_dirs(self.dirs[file_of(d)], here) == best);
        }
        if cands.len() == 1 && cands[0] != caller {
            Some(cands[0])
        } else {
            None
        }
    }

    /// A bare name seen from `start`: the definitions in the enclosing scopes, outward, then
    /// the top of the file, then imported names, then the repository at large.
    pub(crate) fn resolve_bare(
        &self,
        f: usize,
        start: u32,
        name: &str,
        want: Want,
        exclude: u32,
        caller: u32,
    ) -> Option<(u32, Confidence)> {
        let fits = |d: u32| d != exclude && self.sym_kind(d).map_or(false, |k| want.accepts(k));
        let first_fit = |defs: &Vec<u32>| defs.iter().copied().find(|&d| fits(d));
        let implicit = self.implicit_scope[f];
        let mut scope = start;
        while scope >= self.nfiles && (scope as usize) < self.nodes.len() {
            let kind = self.sym_kind(scope)?;
            if kind.is_callable() || (implicit && kind.is_container()) {
                if let Some(d) = self.scope_defs.get(&(scope, name)).and_then(first_fit) {
                    return Some((d, Confidence::Extracted));
                }
                if implicit && kind.is_container() && want == Want::Target {
                    if let Some(m) = self.find_member(scope, name, false) {
                        if m != exclude {
                            return Some((m, Confidence::Extracted));
                        }
                    }
                }
            }
            scope = self.up[scope as usize];
        }
        if let Some(d) = self.scope_defs.get(&(f as u32, name)).and_then(first_fit) {
            return Some((d, Confidence::Extracted));
        }
        if let Some(d) = self.resolve_imported(f, name, want, exclude) {
            return Some((d, Confidence::Extracted));
        }
        self.resolve_global(f, name, want, exclude, caller).map(|d| (d, Confidence::Inferred))
    }

    /// The class-like definition a receiver names, with how sure that is.
    fn class_of_receiver(&self, f: usize, caller: u32, recv: &str) -> Option<(u32, Confidence)> {
        let (prefix, last) = split_last(recv);
        if !is_ident(last) {
            return None;
        }
        if prefix.is_empty() {
            return self.resolve_bare(f, caller, last, Want::Class, NONE, caller);
        }
        let module = self.module_of_receiver(f, prefix)?;
        let found = self.lookup_in_module(module, last, Want::Class, None)?;
        Some((found, Confidence::Extracted))
    }

    /// Resolves one base class name of the class-like node `node` in file `f`.
    pub(crate) fn resolve_base(&self, f: usize, node: u32, base: &str) -> Option<(u32, Confidence)> {
        let name = clean_base(base);
        let (prefix, last) = split_last(name);
        if !is_ident(last) {
            return None;
        }
        if prefix.is_empty() {
            return self.resolve_bare(f, self.up[node as usize], last, Want::Class, node, node);
        }
        let module = self.module_of_receiver(f, prefix)?;
        let found = self.lookup_in_module(module, last, Want::Class, None)?;
        Some((found, Confidence::Extracted))
    }

    /// Ties one call to a definition, by the rules in order, or gives `None` for a loose call.
    ///
    /// 1. `self`, `cls`, `this`, `Self`, `super` and `super(...)` look in the enclosing class and its bases;
    ///    `super` looks in the bases only, since the class's own method is the one being extended.
    /// 2. A bare name is looked up by scope, imports, and the repository's names.
    /// 3. A receiver that names an imported module looks in that module.
    /// 4. A receiver that names a class looks in that class and its bases.
    /// 5. Any other receiver: the one function or method of that name in the repository, unless
    ///    the name is a very common one.
    ///
    /// A rule that finds nothing hands over to the next.
    pub(crate) fn resolve_call(&self, f: usize, caller: u32, name: &str, recv: &str) -> Option<(u32, Confidence)> {
        let recv = recv.trim();
        if recv.is_empty() {
            return self.resolve_bare(f, caller, name, Want::Target, NONE, caller);
        }
        // `super` and `super()` or `super(Child, self)`; not a name that merely starts with it.
        let is_super = recv == "super" || recv.starts_with("super(");
        if matches!(recv, "self" | "cls" | "this" | "Self") || is_super {
            if let Some(class) = self.class_of(caller) {
                if let Some(found) = self.find_member(class, name, is_super) {
                    return Some((found, Confidence::Extracted));
                }
            }
        }
        if let Some(module) = self.module_of_receiver(f, recv) {
            if let Some(found) = self.lookup_in_module(module, name, Want::Target, None) {
                return Some((found, Confidence::Extracted));
            }
        }
        if let Some((class, conf)) = self.class_of_receiver(f, caller, recv) {
            if let Some(found) = self.find_member(class, name, false) {
                return Some((found, conf));
            }
        }
        if COMMON_NAMES.contains(&name) {
            return None;
        }
        let mut only = None;
        for &d in self.global.get(name)? {
            let callable = self.sym_kind(d).map_or(false, |k| k.is_callable());
            if callable && self.family[self.nodes[d as usize].file as usize] == self.family[f] {
                if only.is_some() {
                    return None;
                }
                only = Some(d);
            }
        }
        only.filter(|&d| d != caller).map(|d| (d, Confidence::Inferred))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_last_cuts_at_the_last_separator_of_either_kind() {
        assert_eq!(split_last("run"), ("", "run"));
        assert_eq!(split_last("a.b.c"), ("a.b", "c"));
        assert_eq!(split_last("crate::store::Store"), ("crate::store", "Store"));
        assert_eq!(split_last("a::b.c"), ("a::b", "c"));
        assert_eq!(split_last("a.b::c"), ("a.b", "c"));
        assert_eq!(split_last(""), ("", ""));
    }

    #[test]
    fn base_names_lose_generics_and_marks() {
        assert_eq!(clean_base("Generic[T]"), "Generic");
        assert_eq!(clean_base("From<X>"), "From");
        assert_eq!(clean_base("&dyn Display"), "Display");
        assert_eq!(clean_base("models.User"), "models.User");
        assert_eq!(clean_base("metaclass=Meta"), "");
        assert_eq!(clean_base("  "), "");
    }

    #[test]
    fn common_names_have_no_repeats() {
        let mut sorted = COMMON_NAMES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), COMMON_NAMES.len());
        assert!(COMMON_NAMES.len() >= 80);
    }
}
