//! JavaScript and TypeScript, through a full parser.
//!
//! The oxc parser builds the syntax tree and one walk over it collects definitions, calls and
//! imports. A stack of frames, one per function, method or class that has a symbol, tells every
//! new symbol its parent and dotted name, and every call the function it belongs to.
//!
//! Spans are byte offsets. Lines come from [`LineIndex`], and signatures and receivers are cut
//! from the source text so that they keep the author's spacing.

use crate::common::{fold, in_source_order, without_comments, LineIndex, Parts};
use lxr_core::{Call, Import, Lang, SymKind, Symbol};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast::AstKind;
use oxc_ast_visit::Visit;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::GetSpan;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Sender};

/// Stack reserved for parsing files that are large or deeply nested. The parser and the walk
/// are recursive, and a stack overflow cannot be caught, so such files are read on a thread
/// with a stack of this size. Only address space is reserved; pages are used as needed.
const DEEP_STACK: usize = 512 << 20;
/// Files nested deeper than this many brackets are not parsed at all.
const MAX_DEPTH: usize = 20_000;
/// A file this short and this shallow is parsed on the calling thread.
const SHALLOW_BYTES: usize = 1024;
const SHALLOW_DEPTH: usize = 40;

/// Longest piece of source text kept for a signature, a receiver or a base name. `finish`
/// cuts them much shorter; this only stops a huge expression from being copied around.
const SIG_BYTES: usize = 2000;
const TEXT_BYTES: usize = 400;
const DOC_BYTES: usize = 8000;

/// Reads a JavaScript, JSX, TypeScript or TSX file. `None` when the parser could not build a
/// program at all; a file with recoverable syntax errors still gives what was parsed.
pub fn parse(text: &str, lang: Lang) -> Option<Parts> {
    if text.len() >= u32::MAX as usize {
        return None;
    }
    let depth = nesting_depth(text);
    if depth > MAX_DEPTH {
        return None;
    }
    if depth <= SHALLOW_DEPTH && text.len() <= SHALLOW_BYTES {
        parse_tree(text, lang)
    } else {
        let owned = text.to_owned();
        on_deep_stack(move || parse_tree(&owned, lang)).flatten()
    }
}

type Job = Box<dyn FnOnce() + Send>;

thread_local! {
    /// The sending end of the deep-stack worker of this thread, started on first use. The worker
    /// ends when the thread does, because the channel closes with it.
    static DEEP_WORKER: RefCell<Option<Sender<Job>>> = const { RefCell::new(None) };
}

/// Runs `work` on a thread with a very large stack and waits for its result. Starting a thread
/// for every file would cost more than reading most files, so each calling thread keeps one
/// worker. `None` when the worker could not be started or `work` panicked. Shared with the
/// Rust extractor, which has the same need.
pub(crate) fn on_deep_stack<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (done, result) = channel();
    let mut job: Option<Job> = Some(Box::new(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).ok();
        // The caller only goes away if it was itself torn down, and then nobody wants the result.
        let _ = done.send(outcome);
    }));
    DEEP_WORKER.with(|slot| {
        let mut slot = slot.borrow_mut();
        // A worker that has died is replaced once; a second failure gives up.
        for _ in 0..2 {
            if slot.is_none() {
                let (jobs, inbox) = channel::<Job>();
                std::thread::Builder::new()
                    .stack_size(DEEP_STACK)
                    .spawn(move || inbox.into_iter().for_each(|job| job()))
                    .ok()?;
                *slot = Some(jobs);
            }
            match slot.as_ref()?.send(job.take()?) {
                Ok(()) => return Some(()),
                Err(failed) => {
                    job = Some(failed.0);
                    *slot = None;
                }
            }
        }
        None
    })?;
    result.recv().ok().flatten()
}

/// The deepest nesting of brackets, parentheses and braces, ignoring strings and comments.
fn nesting_depth(text: &str) -> usize {
    let bytes = text.as_bytes();
    let (mut i, mut depth, mut deepest) = (0, 0usize, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            quote @ (b'\'' | b'"' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    } else if bytes[i] == b'\n' && quote != b'`' {
                        break;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    deepest
}

fn parse_tree(text: &str, lang: Lang) -> Option<Parts> {
    use oxc_span::SourceType;
    match lang {
        Lang::TypeScript => read(text, SourceType::ts()),
        Lang::Tsx => read(text, SourceType::tsx()),
        // A module first; a script that a module rejects gets a second try.
        _ => read(text, SourceType::mjs().with_jsx(true))
            .or_else(|| read(text, SourceType::script().with_jsx(true))),
    }
}

fn read(text: &str, source_type: oxc_span::SourceType) -> Option<Parts> {
    let allocator = Allocator::default();
    let options = ParseOptions { allow_return_outside_function: true, preserve_parens: false, ..ParseOptions::default() };
    let parsed = Parser::new(&allocator, text, source_type).with_options(options).parse();
    if parsed.fatal_error {
        return None;
    }
    let mut walker = Walker::new(text, &parsed.program);
    walker.visit_program(&parsed.program);
    in_source_order(&mut walker.parts.calls, &walker.call_at);
    Some(walker.parts)
}

/// The first token of a declaration, and the offset its documentation comment must end before.
/// They differ when decorators sit above the declaration.
#[derive(Clone, Copy)]
struct Origin {
    start: u32,
    anchor: u32,
}

/// Where the `export` of an exported declaration starts.
#[derive(Clone, Copy)]
struct Export {
    start: u32,
    default: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Function,
    Arrow,
    Class,
}

/// A function, method or class whose body is being walked.
struct Frame {
    tag: Tag,
    span: Span,
    sym: u32,
    qual: String,
    caller: Option<u32>,
}

/// A function or class that the declaration being walked binds to a name.
struct Bound {
    span: Span,
    name: String,
    kind: SymKind,
    origin: Origin,
}

/// A method or class property whose symbol exists and whose function has not been reached yet.
struct Member {
    span: Span,
    sym: u32,
}

/// The `var`, `let` or `const` declaration being walked.
struct DeclCtx {
    origin: Origin,
    first: u32,
}

struct Note {
    start: u32,
    end: u32,
    jsdoc: bool,
}

struct Walker<'t> {
    text: &'t str,
    lines: LineIndex,
    notes: Vec<Note>,
    /// Byte ranges of the comments, to leave out of a signature.
    comment_spans: Vec<(usize, usize)>,
    parts: Parts,
    /// The byte offset of the called name of each entry of `parts.calls`.
    call_at: Vec<usize>,
    frames: Vec<Frame>,
    exports: HashMap<u32, Export>,
    decls: Vec<DeclCtx>,
    bound: Option<Bound>,
    member: Option<Member>,
    /// Starts of `require` calls that were already turned into an import with a binding.
    taken: HashSet<u32>,
}

impl<'t> Walker<'t> {
    fn new(text: &'t str, program: &Program<'_>) -> Walker<'t> {
        let notes = program
            .comments
            .iter()
            .map(|c| {
                let raw = text.get(c.span.start as usize..c.span.end as usize).unwrap_or("");
                Note { start: c.span.start, end: c.span.end, jsdoc: c.is_block() && raw.len() >= 5 && raw.starts_with("/**") }
            })
            .collect();
        let comment_spans = program.comments.iter().map(|c| (c.span.start as usize, c.span.end as usize)).collect();
        Walker {
            text,
            lines: LineIndex::new(text),
            notes,
            comment_spans,
            parts: Parts::default(),
            call_at: Vec::new(),
            frames: Vec::new(),
            exports: HashMap::new(),
            decls: Vec::new(),
            bound: None,
            member: None,
            taken: HashSet::new(),
        }
    }

    fn slice(&self, from: u32, to: u32) -> &'t str {
        if from > to {
            return "";
        }
        self.text.get(from as usize..to as usize).unwrap_or("")
    }

    /// Source text of a span, cut short.
    fn text_of(&self, span: Span) -> String {
        capped(self.slice(span.start, span.end), TEXT_BYTES).to_string()
    }

    fn line(&self, offset: u32) -> u32 {
        self.lines.line_of(offset as usize)
    }

    fn current_caller(&self) -> Option<u32> {
        self.frames.last().and_then(|frame| frame.caller)
    }

    /// The offset of the next token at or after `from`, skipping white space and comments.
    fn after_trivia(&self, from: u32) -> u32 {
        let bytes = self.text.as_bytes();
        let mut i = from as usize;
        loop {
            match bytes.get(i) {
                Some(b) if b.is_ascii_whitespace() => i += 1,
                Some(b'/') if bytes.get(i + 1) == Some(&b'/') => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
                Some(b'/') if bytes.get(i + 1) == Some(&b'*') => {
                    i += 2;
                    while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                        i += 1;
                    }
                    i = (i + 2).min(bytes.len());
                }
                _ => return i as u32,
            }
        }
    }

    /// Where a declaration that starts at `node_start` really begins: before its `export`, and
    /// after any decorators that come first.
    fn origin_of(&self, node_start: u32, decorators: &[Decorator<'_>]) -> Origin {
        let outer = self.exports.get(&node_start).map_or(node_start, |export| export.start.min(node_start));
        match (decorators.first(), decorators.last()) {
            (Some(first), Some(last)) => {
                let anchor = outer.min(first.span.start);
                let start = if outer < first.span.start { outer } else { self.after_trivia(last.span.end) };
                Origin { start, anchor }
            }
            _ => Origin { start: outer, anchor: outer },
        }
    }

    /// The JSDoc block comment that ends directly before `anchor`, without its markers.
    fn doc_before(&self, anchor: u32) -> String {
        let before = self.notes.partition_point(|note| note.end <= anchor);
        let Some(note) = before.checked_sub(1).and_then(|i| self.notes.get(i)) else { return String::new() };
        if !note.jsdoc || !self.slice(note.end, anchor).trim().is_empty() {
            return String::new();
        }
        let raw = capped(self.slice(note.start, note.end), DOC_BYTES);
        let body = raw.strip_prefix("/**").unwrap_or(raw);
        let body = body.strip_suffix("*/").unwrap_or(body);
        let lines: Vec<&str> = body
            .lines()
            .map(|line| {
                let line = line.trim();
                line.strip_prefix('*').unwrap_or(line).trim()
            })
            .collect();
        lines.join("\n").trim().to_string()
    }

    /// Adds a symbol under the innermost frame and returns its index.
    fn add(&mut self, name: &str, kind: SymKind, origin: Origin, end: u32, sig_end: u32, bases: Vec<String>) -> u32 {
        let (parent, qual) = match self.frames.last() {
            Some(frame) => (Some(frame.sym), format!("{}.{}", frame.qual, name)),
            None => (None, name.to_string()),
        };
        let sig = without_comments(self.text, origin.start as usize, sig_end as usize, &self.comment_spans, SIG_BYTES)
            .trim()
            .to_string();
        let symbol = Symbol {
            name: name.to_string(),
            qual,
            kind,
            start: self.line(origin.start),
            end: self.lines.last_line_of(end as usize),
            sig,
            doc: self.doc_before(origin.anchor),
            parent,
            bases,
            hash: 0,
        };
        self.parts.symbols.push(symbol);
        (self.parts.symbols.len() - 1) as u32
    }

    fn push_frame(&mut self, tag: Tag, span: Span, sym: u32) {
        let symbol = &self.parts.symbols[sym as usize];
        let caller = if symbol.kind.is_callable() { Some(sym) } else { self.current_caller() };
        let frame = Frame { tag, span, sym, qual: symbol.qual.clone(), caller };
        self.frames.push(frame);
    }

    fn leave_frame(&mut self, tag: Tag, span: Span) {
        if self.frames.last().is_some_and(|frame| frame.tag == tag && frame.span == span) {
            self.frames.pop();
        }
    }

    fn take_bound(&mut self, span: Span) -> Option<Bound> {
        if self.bound.as_ref().is_some_and(|bound| bound.span == span) {
            self.bound.take()
        } else {
            None
        }
    }

    fn take_member(&mut self, span: Span) -> Option<u32> {
        if self.member.as_ref().is_some_and(|member| member.span == span) {
            self.member.take().map(|member| member.sym)
        } else {
            None
        }
    }

    fn key_name(&self, key: &PropertyKey<'_>) -> String {
        match key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str().to_string(),
            PropertyKey::PrivateIdentifier(id) => format!("#{}", id.name.as_str()),
            PropertyKey::StringLiteral(s) => s.value.as_str().to_string(),
            other => self.text_of(other.span()),
        }
    }

    fn import(&mut self, module: &str, name: &str, alias: &str, at: u32) {
        let line = self.line(at);
        self.parts.imports.push(Import { module: module.to_string(), name: name.to_string(), alias: alias.to_string(), level: 0, line });
    }

    fn enter_function(&mut self, f: &Function<'_>) {
        let sym = if let Some(sym) = self.take_member(f.span) {
            sym
        } else if let Some(bound) = self.take_bound(f.span) {
            let sig_end = f.body.as_ref().map_or(f.span.end, |body| body.span.start);
            self.add(&bound.name, bound.kind, bound.origin, f.span.end, sig_end, Vec::new())
        } else if matches!(f.r#type, FunctionType::FunctionDeclaration | FunctionType::TSDeclareFunction) {
            let export = self.exports.get(&f.span.start).copied();
            let name = match (&f.id, export) {
                (Some(id), _) => id.name.as_str().to_string(),
                (None, Some(export)) if export.default => "default".to_string(),
                _ => return,
            };
            let origin = self.origin_of(f.span.start, &[]);
            let sig_end = match &f.body {
                Some(body) => body.span.start,
                None => self.without_semicolon(f.span),
            };
            self.add(&name, SymKind::Function, origin, f.span.end, sig_end, Vec::new())
        } else {
            return;
        };
        self.push_frame(Tag::Function, f.span, sym);
    }

    /// The end of a declaration that has no body, before its closing semicolon.
    fn without_semicolon(&self, span: Span) -> u32 {
        let text = self.slice(span.start, span.end);
        (span.start as usize + text.trim_end().trim_end_matches(';').len()) as u32
    }

    fn enter_arrow(&mut self, a: &ArrowFunctionExpression<'_>) {
        let sym = if let Some(sym) = self.take_member(a.span) {
            sym
        } else if let Some(bound) = self.take_bound(a.span) {
            let body_start = match &a.body {
                ArrowFunctionBody::FunctionBody(body) => body.span.start,
                other => other.span().start,
            };
            self.add(&bound.name, bound.kind, bound.origin, a.span.end, body_start, Vec::new())
        } else {
            return;
        };
        self.push_frame(Tag::Arrow, a.span, sym);
    }

    fn enter_class(&mut self, c: &Class<'_>) {
        let bound = self.take_bound(c.span);
        let export = self.exports.get(&c.span.start).copied();
        let name = match (&bound, &c.id, export) {
            (Some(bound), _, _) => bound.name.clone(),
            (None, Some(id), _) => id.name.as_str().to_string(),
            (None, None, Some(export)) if export.default => "default".to_string(),
            _ => return,
        };
        let origin = match &bound {
            Some(bound) => bound.origin,
            None => self.origin_of(c.span.start, &c.decorators),
        };
        let mut bases = Vec::new();
        if let Some(heritage) = &c.heritage {
            bases.push(fold(&self.text_of(heritage.expression.span())));
        }
        for implemented in &c.implements {
            bases.push(fold(&self.text_of(implemented.expression.span())));
        }
        let sym = self.add(&name, SymKind::Class, origin, c.span.end, c.body.span.start, bases);
        self.push_frame(Tag::Class, c.span, sym);
    }

    fn enter_method(&mut self, m: &MethodDefinition<'_>) {
        let name = self.key_name(&m.key);
        let origin = self.origin_of(m.span.start, &m.decorators);
        let sig_end = m.value.body.as_ref().map_or_else(|| self.without_semicolon(m.span), |body| body.span.start);
        let sym = self.add(&name, SymKind::Method, origin, m.span.end, sig_end, Vec::new());
        self.member = Some(Member { span: m.value.span, sym });
    }

    fn enter_property(&mut self, p: &PropertyDefinition<'_>) {
        let Some(value) = &p.value else { return };
        let (span, sig_end) = match unwrap_expr(value) {
            Expression::ArrowFunctionExpression(a) => (
                a.span,
                match &a.body {
                    ArrowFunctionBody::FunctionBody(body) => body.span.start,
                    other => other.span().start,
                },
            ),
            Expression::FunctionExpression(f) => (f.span, f.body.as_ref().map_or(f.span.end, |body| body.span.start)),
            _ => return,
        };
        let name = self.key_name(&p.key);
        let origin = self.origin_of(p.span.start, &p.decorators);
        let sym = self.add(&name, SymKind::Method, origin, span.end, sig_end, Vec::new());
        self.member = Some(Member { span, sym });
    }

    fn enter_declaration(&mut self, d: &VariableDeclaration<'_>) {
        let origin = self.origin_of(d.span.start, &[]);
        let first = d.declarations.first().map_or(u32::MAX, |declarator| declarator.span.start);
        self.decls.push(DeclCtx { origin, first });
    }

    fn enter_declarator(&mut self, d: &VariableDeclarator<'_>) {
        let Some(init) = &d.init else { return };
        let init = unwrap_expr(init);
        if let BindingPattern::BindingIdentifier(id) = &d.id {
            let origin = match self.decls.last() {
                Some(ctx) if ctx.first == d.span.start => ctx.origin,
                _ => Origin { start: d.span.start, anchor: d.span.start },
            };
            let bound = |span: Span, kind: SymKind| Bound { span, name: id.name.as_str().to_string(), kind, origin };
            self.bound = match init {
                Expression::ArrowFunctionExpression(a) => Some(bound(a.span, SymKind::Function)),
                Expression::FunctionExpression(f) => Some(bound(f.span, SymKind::Function)),
                Expression::ClassExpression(c) => Some(bound(c.span, SymKind::Class)),
                _ => None,
            };
        }
        if let Expression::CallExpression(call) = init {
            if let Some(module) = require_module(call) {
                self.bind_require(&d.id, &module, call.span.start);
            }
        }
    }

    /// `const x = require('m')` and `const {a, b: c} = require('m')`. A pattern that cannot be
    /// read leaves the call to be reported as a plain import of the module.
    fn bind_require(&mut self, pattern: &BindingPattern<'_>, module: &str, at: u32) {
        let mut entries: Vec<(String, String)> = Vec::new();
        match pattern {
            BindingPattern::BindingIdentifier(id) => entries.push((String::new(), id.name.as_str().to_string())),
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    let name = match &property.key {
                        PropertyKey::StaticIdentifier(id) if !property.computed => id.name.as_str().to_string(),
                        PropertyKey::StringLiteral(s) if !property.computed => s.value.as_str().to_string(),
                        _ => continue,
                    };
                    if let Some(alias) = pattern_name(&property.value) {
                        entries.push((name, alias));
                    }
                }
            }
            _ => {}
        }
        if entries.is_empty() {
            return;
        }
        self.taken.insert(at);
        for (name, alias) in entries {
            self.import(module, &name, &alias, at);
        }
    }

    fn enter_call(&mut self, call: &CallExpression<'_>) {
        if !self.taken.contains(&call.span.start) {
            if let Some(module) = require_module(call) {
                self.import(&module, "", "", call.span.start);
            }
        }
        self.record_call(&call.callee);
    }

    /// Records a call or `new` of a plain name or of a named member. Other callee shapes are skipped.
    fn record_call(&mut self, callee: &Expression<'_>) {
        let (name, recv, at) = match unwrap_callee(callee) {
            Expression::Identifier(id) => (id.name.as_str().to_string(), String::new(), id.span.start),
            Expression::StaticMemberExpression(m) => {
                (m.property.name.as_str().to_string(), self.text_of(m.object.span()), m.property.span.start)
            }
            Expression::PrivateFieldExpression(p) => {
                (format!("#{}", p.field.name.as_str()), self.text_of(p.object.span()), p.field.span.start)
            }
            _ => return,
        };
        let call = Call { caller: self.current_caller(), name, recv, line: self.line(at) };
        self.parts.calls.push(call);
        self.call_at.push(at as usize);
    }

    fn enter_import(&mut self, d: &ImportDeclaration<'_>) {
        let module = d.source.value.as_str();
        match &d.specifiers {
            Some(specifiers) if !specifiers.is_empty() => {
                for specifier in specifiers {
                    let (name, alias) = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            (export_name(&s.imported), s.local.name.as_str().to_string())
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            ("default".to_string(), s.local.name.as_str().to_string())
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            ("*".to_string(), s.local.name.as_str().to_string())
                        }
                    };
                    self.import(module, &name, &alias, d.span.start);
                }
            }
            _ => self.import(module, "", "", d.span.start),
        }
    }

    fn enter_export_from(&mut self, d: &ExportFromDeclaration<'_>) {
        let module = d.source.value.as_str();
        if d.specifiers.is_empty() {
            self.import(module, "", "", d.span.start);
        }
        for specifier in &d.specifiers {
            self.import(module, &export_name(&specifier.local), &export_name(&specifier.exported), d.span.start);
        }
    }

    fn enter_export_all(&mut self, d: &ExportAllDeclaration<'_>) {
        let alias = d.exported.as_ref().map_or_else(|| "*".to_string(), export_name);
        self.import(d.source.value.as_str(), "*", &alias, d.span.start);
    }

    fn enter_interface(&mut self, i: &TSInterfaceDeclaration<'_>) {
        let origin = self.origin_of(i.span.start, &[]);
        let bases = i.extends.iter().map(|heritage| fold(&self.text_of(heritage.type_name.span()))).collect();
        self.add(i.id.name.as_str(), SymKind::Interface, origin, i.span.end, i.body.span.start, bases);
    }

    fn enter_type_alias(&mut self, t: &TSTypeAliasDeclaration<'_>) {
        let origin = self.origin_of(t.span.start, &[]);
        let sym = self.add(t.id.name.as_str(), SymKind::TypeAlias, origin, t.span.end, t.type_annotation.span().start, Vec::new());
        let symbol = &mut self.parts.symbols[sym as usize];
        symbol.sig = symbol.sig.trim_end_matches('=').trim_end().to_string();
    }

    fn enter_enum(&mut self, e: &TSEnumDeclaration<'_>) {
        let origin = self.origin_of(e.span.start, &[]);
        self.add(e.id.name.as_str(), SymKind::Enum, origin, e.span.end, e.body.span.start, Vec::new());
    }

    /// Notes where the `export` of an exported declaration starts, keyed by the declaration's own
    /// start, so that the declaration can find it when the walk reaches it.
    fn enter_export(&mut self, wrapper: Span, inner: Span, default: bool) {
        self.exports.insert(inner.start, Export { start: wrapper.start, default });
    }

    fn enter_export_default(&mut self, d: &ExportDefaultDeclaration<'_>) {
        let inner = d.declaration.span();
        self.enter_export(d.span, inner, true);
        let function = match unwrap_default_expr(&d.declaration) {
            Some(Expression::ArrowFunctionExpression(a)) => Some(a.span),
            Some(Expression::FunctionExpression(f)) => Some(f.span),
            _ => None,
        };
        if let Some(span) = function {
            let origin = Origin { start: d.span.start, anchor: d.span.start };
            self.bound = Some(Bound { span, name: "default".to_string(), kind: SymKind::Function, origin });
        }
    }
}

impl<'a> Visit<'a> for Walker<'_> {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        match kind {
            AstKind::Function(f) => self.enter_function(f),
            AstKind::ArrowFunctionExpression(a) => self.enter_arrow(a),
            AstKind::Class(c) => self.enter_class(c),
            AstKind::MethodDefinition(m) => self.enter_method(m),
            AstKind::PropertyDefinition(p) => self.enter_property(p),
            AstKind::VariableDeclaration(d) => self.enter_declaration(d),
            AstKind::VariableDeclarator(d) => self.enter_declarator(d),
            AstKind::CallExpression(c) => self.enter_call(c),
            AstKind::NewExpression(n) => self.record_call(&n.callee),
            AstKind::ImportExpression(e) => {
                if let Some(module) = string_value(&e.source) {
                    self.import(&module, "", "", e.span.start);
                }
            }
            AstKind::ImportDeclaration(d) => self.enter_import(d),
            AstKind::ExportFromDeclaration(d) => self.enter_export_from(d),
            AstKind::ExportAllDeclaration(d) => self.enter_export_all(d),
            AstKind::TSImportEqualsDeclaration(d) => {
                if let TSModuleReference::ExternalModuleReference(reference) = &d.module_reference {
                    self.import(reference.expression.value.as_str(), "", d.id.name.as_str(), d.span.start);
                }
            }
            AstKind::ExportDeclaration(d) => self.enter_export(d.span, d.declaration.span(), false),
            AstKind::ExportDefaultDeclaration(d) => self.enter_export_default(d),
            AstKind::TSInterfaceDeclaration(i) => self.enter_interface(i),
            AstKind::TSTypeAliasDeclaration(t) => self.enter_type_alias(t),
            AstKind::TSEnumDeclaration(e) => self.enter_enum(e),
            _ => {}
        }
    }

    fn leave_node(&mut self, kind: AstKind<'a>) {
        match kind {
            AstKind::Function(f) => self.leave_frame(Tag::Function, f.span),
            AstKind::ArrowFunctionExpression(a) => self.leave_frame(Tag::Arrow, a.span),
            AstKind::Class(c) => self.leave_frame(Tag::Class, c.span),
            AstKind::VariableDeclaration(_) => {
                self.decls.pop();
            }
            AstKind::VariableDeclarator(_) | AstKind::ExportDefaultDeclaration(_) => self.bound = None,
            AstKind::MethodDefinition(_) | AstKind::PropertyDefinition(_) => self.member = None,
            _ => {}
        }
    }
}

/// `text` cut to at most `max` bytes, at a character boundary.
fn capped(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// An expression with parentheses and TypeScript wrappers peeled off.
fn unwrap_expr<'b, 'a>(mut expression: &'b Expression<'a>) -> &'b Expression<'a> {
    loop {
        expression = match expression {
            Expression::ParenthesizedExpression(e) => &e.expression,
            Expression::TSAsExpression(e) => &e.expression,
            Expression::TSSatisfiesExpression(e) => &e.expression,
            Expression::TSNonNullExpression(e) => &e.expression,
            Expression::TSTypeAssertion(e) => &e.expression,
            Expression::TSInstantiationExpression(e) => &e.expression,
            _ => return expression,
        };
    }
}

/// A callee with parentheses and non-null assertions peeled off.
fn unwrap_callee<'b, 'a>(mut callee: &'b Expression<'a>) -> &'b Expression<'a> {
    loop {
        callee = match callee {
            Expression::ParenthesizedExpression(e) => &e.expression,
            Expression::TSNonNullExpression(e) => &e.expression,
            _ => return callee,
        };
    }
}

/// The expression of an `export default` that is not a declaration.
fn unwrap_default_expr<'b, 'a>(declaration: &'b ExportDefaultDeclarationKind<'a>) -> Option<&'b Expression<'a>> {
    declaration.as_expression().map(unwrap_expr)
}

/// The text of a string literal, or of a template literal without substitutions.
fn string_value(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::StringLiteral(s) => Some(s.value.as_str().to_string()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            Some(t.quasis[0].value.raw.as_str().to_string())
        }
        _ => None,
    }
}

/// The module of `require('m')`, when the call has exactly that shape.
fn require_module(call: &CallExpression<'_>) -> Option<String> {
    let Expression::Identifier(callee) = &call.callee else { return None };
    if callee.name.as_str() != "require" || call.arguments.len() != 1 {
        return None;
    }
    string_value(call.arguments[0].as_expression()?)
}

/// The name a destructuring pattern binds, looking through a default value.
fn pattern_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str().to_string()),
        BindingPattern::AssignmentPattern(assignment) => pattern_name(&assignment.left),
        _ => None,
    }
}

fn export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(n) => n.name.as_str().to_string(),
        ModuleExportName::IdentifierReference(n) => n.name.as_str().to_string(),
        ModuleExportName::StringLiteral(s) => s.value.as_str().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lxr_core::ParseGrade;

    /// A source from lines, so that a line number in an expectation is the position in the list.
    fn src(lines: &[&str]) -> String {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    fn read_ok(lines: &[&str], lang: Lang) -> Parts {
        parse(&src(lines), lang).expect("the file parses")
    }

    fn symbols(parts: &Parts) -> Vec<String> {
        parts
            .symbols
            .iter()
            .map(|s| {
                let parent = s.parent.map_or("-".to_string(), |p| parts.symbols[p as usize].qual.clone());
                format!(
                    "{} {}|{} {}-{} {:?} {:?} parent={} bases={:?}",
                    s.kind.word(),
                    s.name,
                    s.qual,
                    s.start,
                    s.end,
                    s.sig,
                    s.doc,
                    parent,
                    s.bases
                )
            })
            .collect()
    }

    fn calls(parts: &Parts) -> Vec<String> {
        parts
            .calls
            .iter()
            .map(|c| {
                let caller = c.caller.map_or("-", |i| parts.symbols[i as usize].qual.as_str());
                format!("{}|{}|{}|{}", caller, c.recv, c.name, c.line)
            })
            .collect()
    }

    fn imports(parts: &Parts) -> Vec<String> {
        parts.imports.iter().map(|i| format!("{}|{}|{}|{}", i.module, i.name, i.alias, i.line)).collect()
    }

    #[test]
    fn class_with_static_getter_and_constructor() {
        let parts = read_ok(
            &[
                "/**",
                " * Stores widgets.",
                " * Second line.",
                " */",
                "export class Store extends Base implements Disposable, Other<T> {",
                "  /** Makes one. */",
                "  static create(a: number, b = 2): Store {",
                "    return new Store(a);",
                "  }",
                "",
                "  get size() {",
                "    return this.items.length;",
                "  }",
                "",
                "  constructor(items) {",
                "    super(items);",
                "    this.items = load(items);",
                "  }",
                "}",
            ],
            Lang::TypeScript,
        );
        assert_eq!(
            symbols(&parts),
            [
                r#"class Store|Store 5-19 "export class Store extends Base implements Disposable, Other<T>" "Stores widgets.\nSecond line." parent=- bases=["Base", "Disposable", "Other"]"#,
                r#"method create|Store.create 7-9 "static create(a: number, b = 2): Store" "Makes one." parent=Store bases=[]"#,
                r#"method size|Store.size 11-13 "get size()" "" parent=Store bases=[]"#,
                r#"method constructor|Store.constructor 15-18 "constructor(items)" "" parent=Store bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["Store.create||Store|8", "Store.constructor||load|17"]);
        assert!(parts.imports.is_empty());
    }

    #[test]
    fn functions_arrows_and_default_export() {
        let parts = read_ok(
            &[
                "/** Adds numbers. */",
                "function add(a, b) {",
                "  return a + b;",
                "}",
                "",
                "const mul = (a, b) => a * b;",
                "export const div = async (a, b) => {",
                "  function check() { return helper(b); }",
                "  check();",
                "  return a / b;",
                "};",
                "var old = function () { log.info(\"x\"); };",
                "export default function () {",
                "  run();",
                "}",
            ],
            Lang::JavaScript,
        );
        assert_eq!(
            symbols(&parts),
            [
                r#"function add|add 2-4 "function add(a, b)" "Adds numbers." parent=- bases=[]"#,
                r#"function mul|mul 6-6 "const mul = (a, b) =>" "" parent=- bases=[]"#,
                r#"function div|div 7-11 "export const div = async (a, b) =>" "" parent=- bases=[]"#,
                r#"function check|div.check 8-8 "function check()" "" parent=div bases=[]"#,
                r#"function old|old 12-12 "var old = function ()" "" parent=- bases=[]"#,
                r#"function default|default 13-15 "export default function ()" "" parent=- bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["div.check||helper|8", "div||check|9", "old|log|info|12", "default||run|14"]);
    }

    #[test]
    fn typescript_interfaces_types_enums_and_generics() {
        let parts = read_ok(
            &[
                "/** A pair. */",
                "export interface Pair<A, B> extends Base<A>, Other {",
                "  first: A;",
                "  second(): B;",
                "}",
                "",
                "export type Maybe<T> = T | null;",
                "",
                "declare enum Mode { Fast, Slow }",
                "",
                "export function map<T, U>(items: T[], fn: (item: T) => U): U[] {",
                "  return items.map(fn);",
                "}",
                "",
                "export abstract class Shape<T> implements Named {",
                "  abstract area(): number;",
                "}",
            ],
            Lang::TypeScript,
        );
        assert_eq!(
            symbols(&parts),
            [
                r#"interface Pair|Pair 2-5 "export interface Pair<A, B> extends Base<A>, Other" "A pair." parent=- bases=["Base", "Other"]"#,
                r#"type Maybe|Maybe 7-7 "export type Maybe<T>" "" parent=- bases=[]"#,
                r#"enum Mode|Mode 9-9 "declare enum Mode" "" parent=- bases=[]"#,
                r#"function map|map 11-13 "export function map<T, U>(items: T[], fn: (item: T) => U): U[]" "" parent=- bases=[]"#,
                r#"class Shape|Shape 15-17 "export abstract class Shape<T> implements Named" "" parent=- bases=["Named"]"#,
                r#"method area|Shape.area 16-16 "abstract area(): number" "" parent=Shape bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["map|items|map|12"]);
    }

    #[test]
    fn jsx_components_and_imports() {
        let parts = read_ok(
            &[
                "import React from 'react';",
                "import { render } from './dom.js';",
                "",
                "/** Greeting component. */",
                "export function Greeting({ name }) {",
                "  const label = useLabel(name);",
                "  return <div className={cls(label)}>{format(name)}</div>;",
                "}",
                "",
                "render(<Greeting name=\"x\" />, document.getElementById('root'));",
            ],
            Lang::JavaScript,
        );
        assert_eq!(
            symbols(&parts),
            [r#"function Greeting|Greeting 5-8 "export function Greeting({ name })" "Greeting component." parent=- bases=[]"#]
        );
        assert_eq!(
            calls(&parts),
            ["Greeting||useLabel|6", "Greeting||cls|7", "Greeting||format|7", "-||render|10", "-|document|getElementById|10"]
        );
        assert_eq!(imports(&parts), ["react|default|React|1", "./dom.js|render|render|2"]);
    }

    #[test]
    fn require_and_dynamic_import_forms() {
        let parts = read_ok(
            &[
                "const fs = require('fs');",
                "const { join, resolve: res } = require(\"path\");",
                "const cfg = require(`./cfg`);",
                "require('dotenv').config();",
                "async function boot() {",
                "  const m = await import('./main.js');",
                "  m.start();",
                "}",
            ],
            Lang::JavaScript,
        );
        assert_eq!(
            imports(&parts),
            ["fs||fs|1", "path|join|join|2", "path|resolve|res|2", "./cfg||cfg|3", "dotenv|||4", "./main.js|||6"]
        );
        assert_eq!(
            calls(&parts),
            ["-||require|1", "-||require|2", "-||require|3", "-||require|4", "-|require('dotenv')|config|4", "boot|m|start|7"]
        );
        assert_eq!(symbols(&parts), [r#"function boot|boot 5-8 "async function boot()" "" parent=- bases=[]"#]);
    }

    #[test]
    fn import_and_re_export_forms() {
        let parts = read_ok(
            &[
                "import def, * as ns from './a';",
                "import './side';",
                "import type { T } from './types';",
                "export { x, y as z } from './b';",
                "export * from './c';",
                "export * as d from './d';",
                "export { local };",
            ],
            Lang::TypeScript,
        );
        assert_eq!(
            imports(&parts),
            [
                "./a|default|def|1",
                "./a|*|ns|1",
                "./side|||2",
                "./types|T|T|3",
                "./b|x|x|4",
                "./b|y|z|4",
                "./c|*|*|5",
                "./d|*|d|6",
            ]
        );
    }

    #[test]
    fn class_properties_bound_to_functions() {
        let parts = read_ok(
            &[
                "class Button {",
                "  count = 0;",
                "  handle = (e) => {",
                "    this.count++;",
                "    track(e);",
                "  };",
                "  static label = function () { return fmt(); };",
                "  #secret() { return hash(); }",
                "}",
            ],
            Lang::JavaScript,
        );
        assert_eq!(
            symbols(&parts),
            [
                r#"class Button|Button 1-9 "class Button" "" parent=- bases=[]"#,
                r#"method handle|Button.handle 3-6 "handle = (e) =>" "" parent=Button bases=[]"#,
                r#"method label|Button.label 7-7 "static label = function ()" "" parent=Button bases=[]"#,
                r##"method #secret|Button.#secret 8-8 "#secret()" "" parent=Button bases=[]"##,
            ]
        );
        assert_eq!(calls(&parts), ["Button.handle||track|5", "Button.label||fmt|7", "Button.#secret||hash|8"]);
    }

    #[test]
    fn getters_and_setters_are_methods_with_the_same_name() {
        let parts = read_ok(
            &["class Box {", "  get value() { return this.v; }", "  set value(v) { this.v = check(v); }", "}"],
            Lang::JavaScript,
        );
        assert_eq!(
            symbols(&parts),
            [
                r#"class Box|Box 1-4 "class Box" "" parent=- bases=[]"#,
                r#"method value|Box.value 2-2 "get value()" "" parent=Box bases=[]"#,
                r#"method value|Box.value 3-3 "set value(v)" "" parent=Box bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["Box.value||check|3"]);
    }

    #[test]
    fn typescript_overloads_declarations_and_namespaces() {
        let parts = read_ok(
            &[
                "export function over(a: string): string;",
                "export function over(a: number): number;",
                "export function over(a: any): any { return impl(a); }",
                "declare function ext(x: number): void;",
                "namespace N {",
                "  export function inN() { go(); }",
                "}",
            ],
            Lang::TypeScript,
        );
        assert_eq!(
            symbols(&parts),
            [
                r#"function over|over 1-1 "export function over(a: string): string" "" parent=- bases=[]"#,
                r#"function over|over 2-2 "export function over(a: number): number" "" parent=- bases=[]"#,
                r#"function over|over 3-3 "export function over(a: any): any" "" parent=- bases=[]"#,
                r#"function ext|ext 4-4 "declare function ext(x: number): void" "" parent=- bases=[]"#,
                r#"function inN|inN 6-6 "export function inN()" "" parent=- bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["over||impl|3", "inN||go|6"]);
    }

    #[test]
    fn call_shapes() {
        let parts = read_ok(
            &[
                "a.b.c(1);",
                "obj?.m();",
                "obj[key]();",
                "(0, fn)();",
                "new ns.Thing();",
                "list.forEach(function (x) { show(x); });",
                "curry(x)(y);",
            ],
            Lang::JavaScript,
        );
        assert_eq!(
            calls(&parts),
            ["-|a.b|c|1", "-|obj|m|2", "-|ns|Thing|5", "-|list|forEach|6", "-||show|6", "-||curry|7"]
        );
    }

    #[test]
    fn documentation_must_end_directly_before_the_declaration() {
        let parts = read_ok(
            &[
                "/** Detached. */",
                "const x = 1;",
                "",
                "// not a doc",
                "function a() {}",
                "",
                "/* plain */",
                "function b() {}",
                "",
                "/** Real doc,",
                " * two lines. */",
                "",
                "function c() {}",
            ],
            Lang::JavaScript,
        );
        assert_eq!(
            symbols(&parts),
            [
                r#"function a|a 5-5 "function a()" "" parent=- bases=[]"#,
                r#"function b|b 8-8 "function b()" "" parent=- bases=[]"#,
                r#"function c|c 13-13 "function c()" "Real doc,\ntwo lines." parent=- bases=[]"#,
            ]
        );
    }

    #[test]
    fn decorators_do_not_move_the_start_line() {
        let parts = read_ok(
            &[
                "@Component({ selector: 'app' })",
                "export class AppComponent {",
                "  @Input() name: string;",
                "  @HostListener('click')",
                "  onClick() { this.run(); }",
                "}",
                "export default class extends Base {}",
            ],
            Lang::TypeScript,
        );
        assert_eq!(
            symbols(&parts),
            [
                r#"class AppComponent|AppComponent 2-6 "export class AppComponent" "" parent=- bases=[]"#,
                r#"method onClick|AppComponent.onClick 5-5 "onClick()" "" parent=AppComponent bases=[]"#,
                r#"class default|default 7-7 "export default class extends Base" "" parent=- bases=["Base"]"#,
            ]
        );
        assert_eq!(
            calls(&parts),
            ["-||Component|1", "-||Input|3", "-||HostListener|4", "AppComponent.onClick|this|run|5"]
        );
    }

    #[test]
    fn tsx_generic_arrow_functions() {
        let parts = read_ok(
            &[
                "export const identity = <T,>(value: T): T => value;",
                "export const Box = ({ children }: Props) => <div>{wrap(children)}</div>;",
            ],
            Lang::Tsx,
        );
        assert_eq!(
            symbols(&parts),
            [
                r#"function identity|identity 1-1 "export const identity = <T,>(value: T): T =>" "" parent=- bases=[]"#,
                r#"function Box|Box 2-2 "export const Box = ({ children }: Props) =>" "" parent=- bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["Box||wrap|2"]);
    }

    #[test]
    fn nested_functions_take_the_qualified_name_of_their_parents() {
        let parts = read_ok(
            &[
                "class Outer {",
                "  method() {",
                "    function inner() { deep(); }",
                "    const arrow = () => inner();",
                "    return [1].map((x) => arrow(x));",
                "  }",
                "}",
                "const Mixin = class extends Base { go() { stop(); } };",
            ],
            Lang::JavaScript,
        );
        assert_eq!(
            symbols(&parts),
            [
                r#"class Outer|Outer 1-7 "class Outer" "" parent=- bases=[]"#,
                r#"method method|Outer.method 2-6 "method()" "" parent=Outer bases=[]"#,
                r#"function inner|Outer.method.inner 3-3 "function inner()" "" parent=Outer.method bases=[]"#,
                r#"function arrow|Outer.method.arrow 4-4 "const arrow = () =>" "" parent=Outer.method bases=[]"#,
                r#"class Mixin|Mixin 8-8 "const Mixin = class extends Base" "" parent=- bases=["Base"]"#,
                r#"method go|Mixin.go 8-8 "go()" "" parent=Mixin bases=[]"#,
            ]
        );
        assert_eq!(
            calls(&parts),
            ["Outer.method.inner||deep|3", "Outer.method.arrow||inner|4", "Outer.method|[1]|map|5", "Outer.method||arrow|5", "Mixin.go||stop|8"]
        );
    }

    #[test]
    fn extract_keeps_the_first_documentation_line_and_grades_the_file() {
        let text = src(&["/**", " * Stores widgets.", " * Second line.", " */", "export class Store {}"]);
        let facts = crate::extract("src/store.ts", Lang::TypeScript, &text);
        assert_eq!(facts.grade, ParseGrade::Ast);
        assert_eq!(facts.symbols[0].doc, "Stores widgets.");
        assert_eq!((facts.symbols[0].start, facts.symbols[0].end), (5, 5));
    }

    #[test]
    fn a_file_the_parser_rejects_falls_back_to_the_lexical_extractor() {
        let facts = crate::extract("broken.js", Lang::JavaScript, "function ( {\n");
        assert_eq!(facts.grade, ParseGrade::Lexical);
    }

    #[test]
    fn bracket_depth_ignores_strings_and_comments() {
        assert_eq!(nesting_depth("f(g([1]))"), 3);
        assert_eq!(nesting_depth("a = '((((' + \"[[[\" + `{{{`; // ((((\n/* ((( */ b(c)"), 1);
        assert_eq!(nesting_depth(""), 0);
    }

    fn assert_survives(text: &str) {
        for lang in [Lang::JavaScript, Lang::Jsx, Lang::TypeScript, Lang::Tsx] {
            // Whether the parser accepts the text or not, nothing may panic and the answer must
            // be one of the two allowed ones.
            let _ = parse(text, lang);
            let facts = crate::extract("odd.src", lang, text);
            assert!(facts.symbols.iter().all(|s| s.start >= 1 && s.end >= s.start));
        }
    }

    #[test]
    fn empty_and_comment_only_files() {
        assert_survives("");
        assert_survives("// nothing here\n/* nor here */\n/** nor here */\n");
        let parts = parse("", Lang::JavaScript).expect("an empty file is a program");
        assert!(parts.symbols.is_empty() && parts.calls.is_empty() && parts.imports.is_empty());
    }

    #[test]
    fn a_very_long_single_line() {
        assert_survives(&"x = 1; ".repeat(30_000));
        assert_survives(&format!("const t = {};", "a + ".repeat(60_000) + "a"));
        assert_survives(&format!("const t = x{};", ".m()".repeat(40_000)));
        let long = format!("function f() {{ {} }}", "g(); ".repeat(40_000));
        let parts = parse(&long, Lang::JavaScript).expect("long line parses");
        assert_eq!(parts.calls.len(), 40_000);
    }

    #[test]
    fn deeply_nested_brackets() {
        let parens = format!("{}x{};", "(".repeat(500), ")".repeat(500));
        assert_survives(&parens);
        let calls = format!("{}x{};", "f(".repeat(500), ")".repeat(500));
        let parts = parse(&calls, Lang::JavaScript).expect("500 nested calls parse");
        assert_eq!(parts.calls.len(), 500);
        let arrays = format!("{}{};", "[".repeat(2000), "]".repeat(2000));
        assert_survives(&arrays);
        // Far deeper than any real file: refused rather than risked.
        assert!(parse(&format!("{}{}", "(".repeat(50_000), ")".repeat(50_000)), Lang::JavaScript).is_none());
    }

    /// The deepest nesting that is still parsed must fit in the stack reserved for it.
    #[test]
    #[ignore = "uses a few hundred megabytes of stack"]
    fn the_deepest_accepted_nesting_fits_the_deep_stack() {
        let n = MAX_DEPTH - 10;
        for text in [
            format!("{}x{};", "(".repeat(n), ")".repeat(n)),
            format!("{}{};", "[".repeat(n), "]".repeat(n)),
            format!("{}x{};", "f(".repeat(n), ")".repeat(n)),
            format!("{}1;", "() => ".repeat(n)),
            format!("{}1{};", "{a:".repeat(n), "}".repeat(n)),
        ] {
            assert!(parse(&text, Lang::JavaScript).is_some());
            assert!(parse(&text, Lang::Tsx).is_some());
        }
    }

    #[test]
    fn text_with_replaced_invalid_utf8() {
        let bytes = b"/** d\xff */\nfunction f\xff() { g(\xe2\x82); }\nconst s = '\xc3(';\n";
        let text = String::from_utf8_lossy(bytes).into_owned();
        assert!(text.contains('\u{fffd}'));
        assert_survives(&text);
    }

    #[test]
    fn non_ascii_text_keeps_offsets_right() {
        let parts = read_ok(
            &["const s = 'héllo wörld';", "/** Zażółć. */", "function ünï(ä) { return ö(ä); }"],
            Lang::JavaScript,
        );
        assert_eq!(symbols(&parts), [r#"function ünï|ünï 3-3 "function ünï(ä)" "Zażółć." parent=- bases=[]"#]);
        assert_eq!(calls(&parts), ["ünï||ö|3"]);
    }

    /// Runs `extract_path` over every JavaScript and TypeScript file under the directories named by
    /// `LXR_JS_DIRS` (separated by `;`) and prints the figures. Does nothing when the variable is unset.
    ///
    /// `cargo test -p lxr-extract corpus_javascript -- --ignored --nocapture`
    #[test]
    #[ignore = "reads real files named by LXR_JS_DIRS"]
    fn corpus_javascript() {
        let Ok(dirs) = std::env::var("LXR_JS_DIRS") else { return };
        let mut files = Vec::new();
        for dir in dirs.split(';').filter(|d| !d.is_empty()) {
            collect(std::path::Path::new(dir), &["js", "mjs", "cjs", "jsx", "ts", "tsx"], &mut files);
        }
        let (mut ast, mut lexical, mut symbols, mut calls, mut imports) = (0, 0, 0, 0, 0);
        let started = std::time::Instant::now();
        let mut slowest = Vec::new();
        for path in &files {
            let root = path.parent().unwrap_or(path);
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let began = std::time::Instant::now();
            let Some(facts) = crate::extract_path(root, name) else { continue };
            slowest.push((began.elapsed(), path.display().to_string(), facts.bytes));
            match facts.grade {
                ParseGrade::Ast => ast += 1,
                ParseGrade::Lexical => {
                    lexical += 1;
                    println!("LEXICAL {}", path.display());
                }
            }
            symbols += facts.symbols.len();
            calls += facts.calls.len();
            imports += facts.imports.len();
        }
        println!(
            "files={} ast={} lexical={} symbols={} calls={} imports={} elapsed_ms={}",
            files.len(),
            ast,
            lexical,
            symbols,
            calls,
            imports,
            started.elapsed().as_millis()
        );
        slowest.sort();
        for (time, path, bytes) in slowest.iter().rev().take(3) {
            println!("slowest {:?} {} bytes {}", time, bytes, path);
        }
    }

    /// Prints the symbols, calls and imports of the one file named by `LXR_DUMP`, for checking
    /// them against the file by hand.
    ///
    /// `cargo test -p lxr-extract dump_file -- --ignored --nocapture`
    #[test]
    #[ignore = "reads the file named by LXR_DUMP"]
    fn dump_file() {
        let Ok(path) = std::env::var("LXR_DUMP") else { return };
        let path = std::path::Path::new(&path);
        let root = path.parent().unwrap_or(path);
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(facts) = crate::extract_path(root, name) else { return };
        println!("grade={:?} lines={}", facts.grade, facts.lines);
        for (i, s) in facts.symbols.iter().enumerate() {
            println!("S{} {} {} {}-{} parent={:?} sig={:?} doc={:?}", i, s.kind.word(), s.qual, s.start, s.end, s.parent, s.sig, s.doc);
        }
        for c in &facts.calls {
            println!("C line={} caller={:?} {}.{}", c.line, c.caller, c.recv, c.name);
        }
        for i in &facts.imports {
            println!("I line={} {} {} {}", i.line, i.module, i.name, i.alias);
        }
    }

    fn collect(dir: &std::path::Path, extensions: &[&str], out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if name != "node_modules" && name != ".git" && name != "target" {
                    collect(&path, extensions, out);
                }
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| extensions.contains(&e)) {
                out.push(path);
            }
        }
    }
}
