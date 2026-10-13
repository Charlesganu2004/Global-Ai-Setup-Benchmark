//! Python, through a full parser.
//!
//! Definitions, calls and imports come from the syntax tree that `rustpython-parser`
//! builds. Names that no parser can give, such as the header of a definition as it was
//! written, are cut from the source text by offset.
//!
//! A call that sits in a decorator, a default value, an annotation or a base class list
//! belongs to the scope the definition sits in, as Python evaluates it there.

use crate::common::{capped, clip, fold, in_source_order, without_comments, LineIndex, Parts};
use crate::lexical;
use lxr_core::{Call, Import, Lang, SymKind, Symbol};
use rustpython_parser::ast::{self, Constant, Expr, Ranged, Stmt};
use rustpython_parser::text_size::TextRange;
use rustpython_parser::{parse as parse_source, Mode};

/// A file at least this large is parsed on a thread with a big stack. A long chain of
/// operators (`a + b + c + ...`) makes a syntax tree as deep as the chain is long, and
/// the parser's tree is dropped recursively, which overflows an ordinary stack at around
/// 30,000 links. A file smaller than this cannot hold a chain that long.
const BIG_FILE: usize = 12 * 1024;

/// Stack of the thread that parses a big file: enough for a tree one link deep for every
/// two bytes of a file of several megabytes.
const BIG_STACK: usize = 256 * 1024 * 1024;

/// Longest piece of source text kept for a signature, a receiver or a base name. `finish`
/// cuts them much shorter; this only stops a huge expression from being copied around.
const SIG_BYTES: usize = 2000;
const TEXT_BYTES: usize = 400;

/// The definitions, calls and imports of a Python file. `None` when the parser rejects
/// the file.
pub fn parse(text: &str) -> Option<Parts> {
    if text.len() < BIG_FILE {
        return parse_here(text);
    }
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new().stack_size(BIG_STACK).spawn_scoped(scope, || parse_here(text)).ok()?;
        worker.join().ok().flatten()
    })
}

fn parse_here(text: &str) -> Option<Parts> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let module = match parse_source(text, Mode::Module, "<input>").ok()? {
        ast::Mod::Module(module) => module,
        _ => return None,
    };
    let masked = lexical::mask(text, Lang::Python);
    let mut walker = Walker {
        text,
        code: masked.code,
        comments: masked.comments,
        lines: LineIndex::new(text),
        parts: Parts::default(),
        call_at: Vec::new(),
        scopes: Vec::new(),
    };
    walker.body(&module.body);
    in_source_order(&mut walker.parts.calls, &walker.call_at);
    Some(walker.parts)
}

/// A definition that is being walked.
struct Scope {
    index: u32,
    is_class: bool,
}

/// The pieces of a function definition, whichever way it was spelled.
struct Def<'e> {
    range: TextRange,
    name: &'e ast::Identifier,
    args: &'e ast::Arguments,
    returns: Option<&'e Expr>,
    decorators: &'e [Expr],
    type_params: &'e [ast::TypeParam],
    body: &'e [Stmt],
}

struct Walker<'a> {
    text: &'a str,
    /// The source with comments and strings blanked, for finding the end of a header.
    code: Vec<u8>,
    /// Byte ranges of the comments, to leave out of a signature.
    comments: Vec<(usize, usize)>,
    lines: LineIndex,
    parts: Parts,
    /// The byte offset of the called name of each entry of `parts.calls`.
    call_at: Vec<usize>,
    /// The definitions around the current statement, innermost last.
    scopes: Vec<Scope>,
}

impl<'a> Walker<'a> {
    /// The innermost enclosing `def`, never a class.
    fn caller(&self) -> Option<u32> {
        self.scopes.iter().rev().find(|s| !s.is_class).map(|s| s.index)
    }

    fn slice(&self, range: TextRange) -> &'a str {
        let text: &'a str = self.text;
        text.get(range.start().to_usize()..range.end().to_usize()).unwrap_or("")
    }

    fn body(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn exprs<'e>(&mut self, list: impl IntoIterator<Item = &'e Expr>) {
        for e in list {
            self.expr(e);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::FunctionDef(f) => self.function(Def {
                range: f.range,
                name: &f.name,
                args: &f.args,
                returns: f.returns.as_deref(),
                decorators: &f.decorator_list,
                type_params: &f.type_params,
                body: &f.body,
            }),
            Stmt::AsyncFunctionDef(f) => self.function(Def {
                range: f.range,
                name: &f.name,
                args: &f.args,
                returns: f.returns.as_deref(),
                decorators: &f.decorator_list,
                type_params: &f.type_params,
                body: &f.body,
            }),
            Stmt::ClassDef(c) => self.class(c),
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.expr(v);
                }
            }
            Stmt::Delete(d) => self.exprs(&d.targets),
            Stmt::Assign(a) => {
                for target in &a.targets {
                    self.constant(a.range, target, &a.value);
                }
                self.exprs(&a.targets);
                self.expr(&a.value);
            }
            Stmt::TypeAlias(t) => {
                self.expr(&t.name);
                self.type_params(&t.type_params);
                self.expr(&t.value);
            }
            Stmt::AugAssign(a) => {
                self.expr(&a.target);
                self.expr(&a.value);
            }
            Stmt::AnnAssign(a) => {
                if let Some(v) = &a.value {
                    self.constant(a.range, &a.target, v);
                }
                self.expr(&a.target);
                self.expr(&a.annotation);
                if let Some(v) = &a.value {
                    self.expr(v);
                }
            }
            Stmt::For(f) => {
                self.expr(&f.target);
                self.expr(&f.iter);
                self.body(&f.body);
                self.body(&f.orelse);
            }
            Stmt::AsyncFor(f) => {
                self.expr(&f.target);
                self.expr(&f.iter);
                self.body(&f.body);
                self.body(&f.orelse);
            }
            Stmt::While(w) => {
                self.expr(&w.test);
                self.body(&w.body);
                self.body(&w.orelse);
            }
            Stmt::If(first) => {
                // An `elif` chain is a chain of nested `if` statements. Walk it with a
                // loop so that a very long chain cannot exhaust the stack.
                let mut current = first;
                loop {
                    self.expr(&current.test);
                    self.body(&current.body);
                    match current.orelse.as_slice() {
                        [Stmt::If(next)] => current = next,
                        rest => {
                            self.body(rest);
                            break;
                        }
                    }
                }
            }
            Stmt::With(w) => {
                self.with_items(&w.items);
                self.body(&w.body);
            }
            Stmt::AsyncWith(w) => {
                self.with_items(&w.items);
                self.body(&w.body);
            }
            Stmt::Match(m) => {
                self.expr(&m.subject);
                for case in &m.cases {
                    if let Some(guard) = &case.guard {
                        self.expr(guard);
                    }
                    self.body(&case.body);
                }
            }
            Stmt::Raise(r) => {
                if let Some(e) = &r.exc {
                    self.expr(e);
                }
                if let Some(c) = &r.cause {
                    self.expr(c);
                }
            }
            Stmt::Try(t) => {
                self.body(&t.body);
                self.handlers(&t.handlers);
                self.body(&t.orelse);
                self.body(&t.finalbody);
            }
            Stmt::TryStar(t) => {
                self.body(&t.body);
                self.handlers(&t.handlers);
                self.body(&t.orelse);
                self.body(&t.finalbody);
            }
            Stmt::Assert(a) => {
                self.expr(&a.test);
                if let Some(m) = &a.msg {
                    self.expr(m);
                }
            }
            Stmt::Import(i) => {
                let line = self.lines.line_of(i.range.start().to_usize());
                for alias in &i.names {
                    let module = alias.name.to_string();
                    let bound = alias.asname.as_ref().map_or_else(|| module.clone(), |a| a.to_string());
                    self.parts.imports.push(Import { module, name: String::new(), alias: bound, level: 0, line });
                }
            }
            Stmt::ImportFrom(i) => {
                let line = self.lines.line_of(i.range.start().to_usize());
                let module = i.module.as_ref().map(|m| m.to_string()).unwrap_or_default();
                let level = i.level.map_or(0, |l| l.to_u32().min(255) as u8);
                for alias in &i.names {
                    let name = alias.name.to_string();
                    let bound = alias.asname.as_ref().map_or_else(|| name.clone(), |a| a.to_string());
                    self.parts.imports.push(Import { module: module.clone(), name, alias: bound, level, line });
                }
            }
            Stmt::Expr(e) => self.expr(&e.value),
            Stmt::Global(_) | Stmt::Nonlocal(_) | Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }

    fn with_items(&mut self, items: &[ast::WithItem]) {
        for item in items {
            self.expr(&item.context_expr);
            if let Some(vars) = &item.optional_vars {
                self.expr(vars);
            }
        }
    }

    fn handlers(&mut self, handlers: &[ast::ExceptHandler]) {
        for handler in handlers {
            let ast::ExceptHandler::ExceptHandler(h) = handler;
            if let Some(t) = &h.type_ {
                self.expr(t);
            }
            self.body(&h.body);
        }
    }

    fn type_params(&mut self, params: &[ast::TypeParam]) {
        for param in params {
            if let ast::TypeParam::TypeVar(tv) = param {
                if let Some(bound) = &tv.bound {
                    self.expr(bound);
                }
            }
        }
    }

    fn arguments(&mut self, args: &ast::Arguments) {
        let mut found = Vec::new();
        argument_exprs(args, &mut found);
        for e in found {
            self.expr(e);
        }
    }

    fn function(&mut self, d: Def<'_>) {
        self.exprs(d.decorators);
        self.arguments(d.args);
        if let Some(returns) = d.returns {
            self.expr(returns);
        }
        self.type_params(d.type_params);
        let kind = if self.scopes.last().map_or(false, |s| s.is_class) { SymKind::Method } else { SymKind::Function };
        let index = self.symbol(d.range, d.name.as_str(), kind, d.body, Vec::new());
        self.scopes.push(Scope { index, is_class: false });
        self.body(d.body);
        self.scopes.pop();
    }

    fn class(&mut self, c: &ast::StmtClassDef) {
        self.exprs(&c.decorator_list);
        self.exprs(&c.bases);
        for keyword in &c.keywords {
            self.expr(&keyword.value);
        }
        self.type_params(&c.type_params);
        let bases: Vec<String> = c.bases.iter().map(|b| fold(self.slice(b.range()))).collect();
        let index = self.symbol(c.range, c.name.as_str(), SymKind::Class, &c.body, bases);
        self.scopes.push(Scope { index, is_class: true });
        self.body(&c.body);
        self.scopes.pop();
    }

    /// Records a function or class and returns its index.
    fn symbol(&mut self, range: TextRange, name: &str, kind: SymKind, body: &[Stmt], bases: Vec<String>) -> u32 {
        let start = range.start().to_usize();
        let end = range.end().to_usize();
        let parent = self.scopes.last().map(|s| s.index);
        let qual = match parent {
            Some(p) => format!("{}.{}", self.parts.symbols[p as usize].qual, name),
            None => name.to_string(),
        };
        let limit = body.first().map_or(end, |s| s.start().to_usize());
        let header_end = self.header_end(start, limit);
        let sig = without_comments(self.text, start, header_end, &self.comments, SIG_BYTES);
        let index = self.parts.symbols.len() as u32;
        self.parts.symbols.push(Symbol {
            name: name.to_string(),
            qual,
            kind,
            start: self.lines.line_of(start),
            end: self.lines.last_line_of(end),
            sig,
            doc: docstring(body),
            parent,
            bases,
            hash: 0,
        });
        index
    }

    /// Offset of the colon that ends the header that starts at `start`.
    fn header_end(&self, start: usize, limit: usize) -> usize {
        let limit = limit.min(self.code.len());
        let mut depth = 0i32;
        let mut i = start;
        while i < limit {
            match self.code[i] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b':' if depth <= 0 => return i,
                _ => {}
            }
            i += 1;
        }
        limit
    }

    /// A module-level assignment to an ALL_CAPS name.
    fn constant(&mut self, range: TextRange, target: &Expr, value: &Expr) {
        if !self.scopes.is_empty() {
            return;
        }
        let Expr::Name(name) = target else { return };
        let name = name.id.as_str();
        if !is_constant_name(name) {
            return;
        }
        let start = range.start().to_usize();
        let end = range.end().to_usize();
        let value_text = fold(self.slice(value.range()));
        self.parts.symbols.push(Symbol {
            name: name.to_string(),
            qual: name.to_string(),
            kind: SymKind::Const,
            start: self.lines.line_of(start),
            end: self.lines.last_line_of(end),
            sig: format!("{} = {}", name, clip(&value_text, 60)),
            doc: String::new(),
            parent: None,
            bases: Vec::new(),
            hash: 0,
        });
    }

    /// Walks an expression and records every call in it. The walk keeps its own stack,
    /// so a very deep expression such as a long chain of `+` cannot overflow ours.
    fn expr<'e>(&mut self, root: &'e Expr) {
        let caller = self.caller();
        let mut stack: Vec<&'e Expr> = vec![root];
        while let Some(e) = stack.pop() {
            if let Expr::Call(c) = e {
                self.call(c, caller);
            }
            let at = stack.len();
            children(e, &mut stack);
            stack[at..].reverse();
        }
    }

    fn call(&mut self, c: &ast::ExprCall, caller: Option<u32>) {
        let (name, recv, at) = match &*c.func {
            Expr::Name(n) => (n.id.as_str(), "", n.range.start().to_usize()),
            Expr::Attribute(a) => (
                a.attr.as_str(),
                capped(self.slice(a.value.range()), TEXT_BYTES),
                a.range.end().to_usize().saturating_sub(a.attr.len()),
            ),
            _ => return,
        };
        self.parts.calls.push(Call {
            caller,
            name: name.to_string(),
            recv: recv.to_string(),
            line: self.lines.line_of(at),
        });
        self.call_at.push(at);
    }
}

/// True for a name written in capitals, digits and underscores, with at least one capital.
fn is_constant_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() || c == '_' => {}
        _ => return false,
    }
    name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') && name.chars().any(|c| c.is_ascii_uppercase())
}

/// The docstring of a body: a first statement that is a lone string.
fn docstring(body: &[Stmt]) -> String {
    if let Some(Stmt::Expr(e)) = body.first() {
        if let Expr::Constant(c) = &*e.value {
            if let Constant::Str(s) = &c.value {
                return s.clone();
            }
        }
    }
    String::new()
}

/// The expressions inside the parameters of a function or lambda: annotations and defaults.
fn argument_exprs<'e>(args: &'e ast::Arguments, out: &mut Vec<&'e Expr>) {
    for arg in args.posonlyargs.iter().chain(args.args.iter()).chain(args.kwonlyargs.iter()) {
        if let Some(annotation) = &arg.def.annotation {
            out.push(annotation);
        }
        if let Some(default) = &arg.default {
            out.push(default);
        }
    }
    for arg in [&args.vararg, &args.kwarg].into_iter().flatten() {
        if let Some(annotation) = &arg.annotation {
            out.push(annotation);
        }
    }
}

fn comprehension_exprs<'e>(generators: &'e [ast::Comprehension], out: &mut Vec<&'e Expr>) {
    for g in generators {
        out.push(&g.target);
        out.push(&g.iter);
        out.extend(g.ifs.iter());
    }
}

/// The direct sub-expressions of `e`, in source order.
fn children<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    match e {
        Expr::BoolOp(x) => out.extend(x.values.iter()),
        Expr::NamedExpr(x) => {
            out.push(&x.target);
            out.push(&x.value);
        }
        Expr::BinOp(x) => {
            out.push(&x.left);
            out.push(&x.right);
        }
        Expr::UnaryOp(x) => out.push(&x.operand),
        Expr::Lambda(x) => {
            argument_exprs(&x.args, out);
            out.push(&x.body);
        }
        Expr::IfExp(x) => {
            out.push(&x.test);
            out.push(&x.body);
            out.push(&x.orelse);
        }
        Expr::Dict(x) => {
            for (key, value) in x.keys.iter().zip(x.values.iter()) {
                if let Some(key) = key {
                    out.push(key);
                }
                out.push(value);
            }
        }
        Expr::Set(x) => out.extend(x.elts.iter()),
        Expr::ListComp(x) => {
            out.push(&x.elt);
            comprehension_exprs(&x.generators, out);
        }
        Expr::SetComp(x) => {
            out.push(&x.elt);
            comprehension_exprs(&x.generators, out);
        }
        Expr::GeneratorExp(x) => {
            out.push(&x.elt);
            comprehension_exprs(&x.generators, out);
        }
        Expr::DictComp(x) => {
            out.push(&x.key);
            out.push(&x.value);
            comprehension_exprs(&x.generators, out);
        }
        Expr::Await(x) => out.push(&x.value),
        Expr::Yield(x) => {
            if let Some(v) = &x.value {
                out.push(v);
            }
        }
        Expr::YieldFrom(x) => out.push(&x.value),
        Expr::Compare(x) => {
            out.push(&x.left);
            out.extend(x.comparators.iter());
        }
        Expr::Call(x) => {
            out.push(&x.func);
            out.extend(x.args.iter());
            out.extend(x.keywords.iter().map(|k| &k.value));
        }
        Expr::FormattedValue(x) => {
            out.push(&x.value);
            if let Some(spec) = &x.format_spec {
                out.push(spec);
            }
        }
        Expr::JoinedStr(x) => out.extend(x.values.iter()),
        Expr::Constant(_) | Expr::Name(_) => {}
        Expr::Attribute(x) => out.push(&x.value),
        Expr::Subscript(x) => {
            out.push(&x.value);
            out.push(&x.slice);
        }
        Expr::Starred(x) => out.push(&x.value),
        Expr::List(x) => out.extend(x.elts.iter()),
        Expr::Tuple(x) => out.extend(x.elts.iter()),
        Expr::Slice(x) => {
            for part in [&x.lower, &x.upper, &x.step].into_iter().flatten() {
                out.push(part);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::finish;
    use lxr_core::{FileFacts, ParseGrade};

    fn facts(text: &str) -> FileFacts {
        let parts = parse(text).expect("the sample is valid Python");
        finish("t.py", Lang::Python, text, ParseGrade::Ast, parts)
    }

    fn defs(f: &FileFacts) -> Vec<String> {
        f.symbols.iter().map(|s| format!("{} {} {}-{}", s.qual, s.kind.word(), s.start, s.end)).collect()
    }

    fn calls(f: &FileFacts) -> Vec<String> {
        f.calls
            .iter()
            .map(|c| {
                let caller = c.caller.map_or("-".to_string(), |k| f.symbols[k as usize].qual.clone());
                if c.recv.is_empty() {
                    format!("{}: {} @{}", caller, c.name, c.line)
                } else {
                    format!("{}: {}.{} @{}", caller, c.recv, c.name, c.line)
                }
            })
            .collect()
    }

    fn imports(f: &FileFacts) -> Vec<String> {
        f.imports.iter().map(|i| format!("{}|{}|{}|{}|{}", i.module, i.name, i.alias, i.level, i.line)).collect()
    }

    const SAMPLE: &str = r###"import a.b
import a.b as c
from x.y import n as m
from . import n
from ..p import *
MAX_SIZE = 10
lower = 1

@app.route("/")
class Config(Base, Mixin, metaclass=Meta):
    """Holds settings.

    More.
    """
    LIMIT: int = 5

    def parse(self, text: str = default(),
              flag=False) -> str:
        import json
        return self.build(text, strip(clean(text)))

    @staticmethod
    async def load(url):
        def inner():
            return fetch(url)
        handlers[0]()
        return [i for i in make(url)] + (lambda: later())()

print(top(), x.y.z())
def outer(): pass
"###;

    #[test]
    fn definitions_spans_signatures_and_docs() {
        let f = facts(SAMPLE);
        assert_eq!(
            defs(&f),
            [
                "MAX_SIZE const 6-6",
                "Config class 10-27",
                "Config.parse method 17-20",
                "Config.load method 23-27",
                "Config.load.inner function 24-25",
                "outer function 30-30",
            ]
        );
        assert_eq!(f.symbols[0].sig, "MAX_SIZE = 10");
        assert_eq!(f.symbols[1].sig, "class Config(Base, Mixin, metaclass=Meta)");
        assert_eq!(f.symbols[1].bases, ["Base", "Mixin"]);
        assert_eq!(f.symbols[1].doc, "Holds settings.");
        assert_eq!(f.symbols[2].sig, "def parse(self, text: str = default(), flag=False) -> str");
        assert_eq!(f.symbols[3].sig, "async def load(url)");
        assert_eq!(f.symbols[3].kind, SymKind::Method);
        assert_eq!(f.symbols[4].kind, SymKind::Function);
        assert_eq!((f.symbols[4].parent, f.symbols[3].parent, f.symbols[1].parent), (Some(3), Some(1), None));
    }

    #[test]
    fn calls_in_every_position_with_their_callers() {
        let f = facts(SAMPLE);
        assert_eq!(
            calls(&f),
            [
                "-: app.route @9",
                "-: default @17",
                "Config.parse: self.build @20",
                "Config.parse: strip @20",
                "Config.parse: clean @20",
                "Config.load.inner: fetch @25",
                "Config.load: make @27",
                "Config.load: later @27",
                "-: print @29",
                "-: top @29",
                "-: x.y.z @29",
            ]
        );
    }

    #[test]
    fn import_forms() {
        let f = facts(SAMPLE);
        assert_eq!(
            imports(&f),
            ["a.b||a.b|0|1", "a.b||c|0|2", "x.y|n|m|0|3", "|n|n|1|4", "p|*|*|2|5", "json||json|0|19"]
        );
    }

    #[test]
    fn module_level_constants_only() {
        let src = "A = B = 1\nC: int = 2\nD, E = 1, 2\nlower = 3\n_PRIVATE_X = 4\n__all__ = []\nclass K:\n    INSIDE = 1\nif True:\n    COND = (1,\n            2)\n";
        let f = facts(src);
        assert_eq!(defs(&f), ["A const 1-1", "B const 1-1", "C const 2-2", "_PRIVATE_X const 5-5", "K class 7-8", "COND const 10-11"]);
        assert_eq!(f.symbols[0].sig, "A = 1");
        assert_eq!(f.symbols[5].sig, "COND = (1, 2)");
    }

    #[test]
    fn long_values_are_cut_in_the_signature() {
        let src = format!("TABLE = [{}]\n", "1, ".repeat(40));
        let f = facts(&src);
        assert!(f.symbols[0].sig.starts_with("TABLE = [1, 1, 1,"));
        assert!(f.symbols[0].sig.ends_with("..."));
        assert_eq!(f.symbols[0].sig.chars().count(), "TABLE = ".len() + 60);
    }

    #[test]
    fn calls_that_are_neither_names_nor_attributes_are_skipped() {
        let f = facts("x[0]()\nf()()\n(a or b)()\ng(h(k(1)))\n");
        assert_eq!(calls(&f), ["-: f @2", "-: g @4", "-: h @4", "-: k @4"]);
    }

    #[test]
    fn classes_never_count_as_callers() {
        let src = "class A:\n    x = make()\n    def m(self):\n        class B:\n            y = inner()\n        return B\n";
        let f = facts(src);
        assert_eq!(calls(&f), ["-: make @2", "A.m: inner @5"]);
    }

    #[test]
    fn decorators_defaults_and_annotations_belong_to_the_enclosing_scope() {
        let src = "def outer():\n    @deco(1)\n    def inner(a=dflt(), *args: ann(), **kw) -> ret():\n        return body()\n";
        let f = facts(src);
        assert_eq!(
            calls(&f),
            ["outer: deco @2", "outer: dflt @3", "outer: ann @3", "outer: ret @3", "outer.inner: body @4"]
        );
    }

    #[test]
    fn fstrings_comprehensions_and_lambdas() {
        let src = "def f(xs):\n    s = f\"{fmt(x)!r:>{width()}}\"\n    return [g(x) for x in xs if keep(x)], {k: v(k) for k in xs}, (lambda q=dq(): w(q))\n";
        let f = facts(src);
        let names: Vec<&str> = f.calls.iter().map(|c| c.name.as_str()).collect();
        for want in ["fmt", "width", "g", "keep", "v", "dq", "w"] {
            assert!(names.contains(&want), "missing {want} in {names:?}");
        }
        assert!(f.calls.iter().all(|c| c.caller == Some(0)));
    }

    #[test]
    fn a_chain_of_a_hundred_thousand_operators_does_not_overflow_the_stack() {
        // The test thread has a small stack: the chain is only survived because a big
        // file is parsed on a thread with a big one.
        let chain = (0..100_000).map(|_| "1").collect::<Vec<_>>().join("+");
        let f = facts(&format!("TOTAL = {chain}\ndef f():\n    return g()\n"));
        assert_eq!(f.symbols.len(), 2);
        assert_eq!(calls(&f), ["f: g @3"]);
    }

    #[test]
    fn a_receiver_is_copied_only_as_far_as_a_call_needs_it() {
        let chain = format!("x = a{}\n", ".m(1)".repeat(3_000));
        let parts = parse(&chain).expect("the chain is valid Python");
        assert_eq!(parts.calls.len(), 3_000);
        assert!(parts.calls.iter().all(|c| c.recv.len() <= TEXT_BYTES));
        // The calls are in source order, so the innermost call comes first.
        assert_eq!(parts.calls[0].recv, "a");
        assert_eq!(parts.calls[1].recv, "a.m(1)");
        assert_eq!(parts.calls[2_999].recv.len(), TEXT_BYTES);
        // The cut falls inside a two-byte character and does not split it.
        let wide = format!("x = {}.f()\n", "é".repeat(500));
        let parts = parse(&wide).expect("a name may hold letters outside ASCII");
        assert_eq!(parts.calls.len(), 1);
        assert_eq!(parts.calls[0].recv.chars().count(), TEXT_BYTES / 2);
    }

    #[test]
    fn a_file_the_parser_rejects_gives_none() {
        assert!(parse("def broken(:\n    pass\n").is_none());
        assert!(parse("print 'python two'\n").is_none());
        assert!(parse("").is_some());
    }

    #[test]
    fn a_byte_order_mark_and_crlf_line_ends_are_fine() {
        let f = facts("\u{feff}def f():\r\n    return g()\r\n");
        assert_eq!(defs(&f), ["f function 1-2"]);
        assert_eq!(calls(&f), ["f: g @2"]);
    }

    #[test]
    fn long_elif_chains_and_deep_sums_do_not_overflow_the_stack() {
        let mut src = String::from("def f(x):\n    if x == 0:\n        return 0\n");
        for i in 1..3000 {
            src.push_str(&format!("    elif x == {i}:\n        return g({i})\n"));
        }
        let f = facts(&src);
        assert_eq!(f.calls.len(), 2999);
        let sum = format!("total = {}\n", (0..400).map(|i| format!("h({i})")).collect::<Vec<_>>().join(" + "));
        assert_eq!(facts(&sum).calls.len(), 400);
    }
}
