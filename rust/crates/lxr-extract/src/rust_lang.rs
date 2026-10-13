//! Rust, through a full parser.
//!
//! `syn` builds the syntax tree and `proc-macro2` reports where every token sits in the source.
//! Signatures and receivers are cut from the source text by those positions, so they keep the
//! author's spacing. One walk collects definitions, calls and imports; a stack of scopes tells
//! every new symbol its parent and dotted name, and every call the function it sits in.

use crate::common::{fold, LineIndex, Parts};
use crate::ecma::on_deep_stack;
use lxr_core::{Call, Import, SymKind, Symbol};
use proc_macro2::{Span, TokenStream, TokenTree};
use syn::parse::Parser as _;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{
    AttrStyle, Attribute, Expr, ExprCall, ExprLit, ExprMacro, ExprMethodCall, Fields, ImplItemConst, ImplItemFn,
    ImplItemType, ItemConst, ItemEnum, ItemExternCrate, ItemFn, ItemImpl, ItemMod, ItemStatic, ItemStruct,
    ItemTrait, ItemType, ItemUnion, ItemUse, Lit, Macro, Meta, TraitItemConst, TraitItemFn, TraitItemType, Type,
    TypeParamBound, UseTree, Visibility,
};

/// Files nested deeper than this many brackets are not parsed at all.
const MAX_DEPTH: usize = 20_000;
/// A file this short and this shallow is parsed on the calling thread. Any other file is read
/// on a thread with a very large stack, because the parser and the walk are recursive and a
/// stack overflow cannot be caught.
const SHALLOW_BYTES: usize = 1024;
const SHALLOW_DEPTH: usize = 40;

/// Longest piece of source text kept for a signature, a receiver or a base name. `finish`
/// cuts them much shorter; this only stops a huge expression from being copied around.
const SIG_BYTES: usize = 2000;
const TEXT_BYTES: usize = 400;

/// Reads a Rust file. `None` when `syn` rejects it.
pub fn parse(text: &str) -> Option<Parts> {
    let depth = nesting_depth(text);
    if depth > MAX_DEPTH {
        return None;
    }
    if depth <= SHALLOW_DEPTH && text.len() <= SHALLOW_BYTES {
        read(text)
    } else {
        let owned = text.to_owned();
        on_deep_stack(move || read(&owned)).flatten()
    }
}

/// The deepest nesting of brackets, parentheses and braces, ignoring strings, characters and comments.
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
                // Block comments nest in Rust.
                let mut level = 1;
                i += 2;
                while i < bytes.len() && level > 0 {
                    if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                        level += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        level -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            b'r' if matches!(bytes.get(i + 1), Some(b'"' | b'#')) && !prev_is_word(bytes, i) => {
                // A raw string: r"..." or r#"..."#, with any number of hashes.
                let mut j = i + 1;
                let mut hashes = 0;
                while bytes.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if bytes.get(j) == Some(&b'"') {
                    j += 1;
                    while j < bytes.len() {
                        if bytes[j] == b'"' && bytes[j + 1..].iter().take(hashes).filter(|&&b| b == b'#').count() == hashes {
                            j += hashes;
                            break;
                        }
                        j += 1;
                    }
                    i = j + 1;
                    continue;
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'\'' => {
                // A character literal, or a lifetime that has no closing quote.
                if bytes.get(i + 1) == Some(&b'\\') {
                    // The quote, the backslash and the escaped character; then up to the closing quote.
                    i += 3;
                    while i < bytes.len() && bytes[i] != b'\'' && bytes[i] != b'\n' {
                        i += 1;
                    }
                } else if let Some(width) = text.get(i + 1..).and_then(|rest| rest.chars().next()).map(char::len_utf8) {
                    if bytes.get(i + 1 + width) == Some(&b'\'') {
                        i += 1 + width;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    deepest
}

fn prev_is_word(bytes: &[u8], i: usize) -> bool {
    i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')
}

/// The part of `text` that `syn` reads: without a byte order mark and without a `#!` line that
/// is not an attribute. A removed shebang leaves its line break, so line numbers stay right.
fn source_body(text: &str) -> &str {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    if body.starts_with("#!") && !body[2..].trim_start().starts_with('[') {
        return &body[body.find('\n').unwrap_or(body.len())..];
    }
    body
}

fn read(text: &str) -> Option<Parts> {
    let body = source_body(text);
    let parts = syn::parse_file(body).ok().map(|file| {
        let mut walker = Walker::new(body);
        walker.visit_file(&file);
        walker.parts
    });
    // The source map of proc-macro2 lives in the thread and would otherwise grow with every file.
    proc_macro2::extra::invalidate_current_thread_spans();
    parts
}

/// The names a new symbol is filed under.
#[derive(Clone)]
struct Scope {
    /// Dotted prefix for the names inside.
    qual: String,
    /// The symbol that things inside belong to.
    parent: Option<u32>,
    /// The function that calls inside belong to.
    caller: Option<u32>,
}

/// Everything `Walker::define` needs to know about one definition.
struct Def<'x> {
    name: String,
    kind: SymKind,
    /// Byte range of the item from its first token, after attributes, to its last.
    start: usize,
    end: usize,
    /// Where the signature stops: before the body's brace, the `=` or the semicolon.
    cut: usize,
    attrs: &'x [Attribute],
    bases: Vec<String>,
}

struct Walker<'t> {
    text: &'t str,
    lines: LineIndex,
    parts: Parts,
    scopes: Vec<Scope>,
    /// The method call that the walk reaches next along a chain, with the offset where the whole
    /// chain starts. It spares a long chain of calls from looking for its start once per call.
    chain: Option<(*const ExprMethodCall, usize)>,
}

impl<'t> Walker<'t> {
    fn new(text: &'t str) -> Walker<'t> {
        Walker { text, lines: LineIndex::new(text), parts: Parts::default(), scopes: Vec::new(), chain: None }
    }

    fn slice(&self, from: usize, to: usize) -> &'t str {
        if from > to {
            return "";
        }
        self.text.get(from..to).unwrap_or("")
    }

    /// Source text of a span, cut short.
    fn text_of(&self, span: Span) -> String {
        let range = span.byte_range();
        capped(self.slice(range.start, range.end), TEXT_BYTES).to_string()
    }

    fn line(&self, offset: usize) -> u32 {
        self.lines.line_of(offset)
    }

    fn caller(&self) -> Option<u32> {
        self.scopes.last().and_then(|scope| scope.caller)
    }

    /// Adds a symbol under the innermost scope and returns its index.
    fn define(&mut self, def: Def<'_>) -> u32 {
        let (parent, qual) = match self.scopes.last() {
            Some(scope) if scope.qual.is_empty() => (scope.parent, def.name.clone()),
            Some(scope) => (scope.parent, format!("{}.{}", scope.qual, def.name)),
            None => (None, def.name.clone()),
        };
        let sig = capped(self.slice(def.start, def.cut), SIG_BYTES).trim().to_string();
        let symbol = Symbol {
            name: def.name,
            qual,
            kind: def.kind,
            start: self.line(def.start),
            end: self.lines.last_line_of(def.end),
            sig,
            doc: doc_of(def.attrs),
            parent,
            bases: def.bases,
            hash: 0,
        };
        self.parts.symbols.push(symbol);
        (self.parts.symbols.len() - 1) as u32
    }

    /// Opens a scope for what is inside the symbol `sym`.
    fn open(&mut self, sym: u32) {
        let symbol = &self.parts.symbols[sym as usize];
        let caller = if symbol.kind.is_callable() { Some(sym) } else { self.caller() };
        let scope = Scope { qual: symbol.qual.clone(), parent: Some(sym), caller };
        self.scopes.push(scope);
    }

    fn import(&mut self, module: &str, name: &str, alias: &str, at: usize) {
        let line = self.line(at);
        self.parts.imports.push(Import { module: module.to_string(), name: name.to_string(), alias: alias.to_string(), level: 0, line });
    }

    fn call(&mut self, name: String, recv: String, at: usize) {
        let call = Call { caller: self.caller(), name, recv, line: self.line(at) };
        self.parts.calls.push(call);
    }

    /// The offset of the first token of an expression, found without turning the whole
    /// expression into tokens when it is a long chain of calls.
    fn expr_start(&self, mut expr: &Expr) -> usize {
        loop {
            expr = match expr {
                Expr::MethodCall(e) => &e.receiver,
                Expr::Call(e) => &e.func,
                Expr::Field(e) => &e.base,
                Expr::Index(e) => &e.expr,
                Expr::Try(e) => &e.expr,
                Expr::Await(e) => &e.base,
                other => return other.span().byte_range().start,
            };
        }
    }

    /// Source text of the receiver of a method call: from where it starts to the dot.
    fn receiver_text(&mut self, call: &ExprMethodCall) -> String {
        let start = match self.chain.take() {
            Some((next, start)) if std::ptr::eq(next, call) => start,
            _ => self.expr_start(&call.receiver),
        };
        if let Some(next) = next_method_call(&call.receiver) {
            self.chain = Some((next as *const ExprMethodCall, start));
        }
        let end = call.dot_token.span().byte_range().start;
        capped(self.slice(start, end).trim_end(), TEXT_BYTES).to_string()
    }

    fn use_tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>, at: usize) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(plain(&path.ident));
                self.use_tree(&path.tree, prefix, at);
                prefix.pop();
            }
            UseTree::Name(name) => self.use_leaf(prefix, &plain(&name.ident), None, at),
            UseTree::Rename(rename) => self.use_leaf(prefix, &plain(&rename.ident), Some(plain(&rename.rename)), at),
            UseTree::Glob(_) => {
                let module = prefix.join("::");
                self.import(&module, "*", "*", at);
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.use_tree(item, prefix, at);
                }
            }
        }
    }

    /// One imported name. `self` inside braces stands for the module before it.
    fn use_leaf(&mut self, prefix: &[String], name: &str, rename: Option<String>, at: usize) {
        if name == "self" {
            let Some((last, rest)) = prefix.split_last() else { return };
            let module = rest.join("::");
            self.import(&module, last, rename.as_deref().unwrap_or(last), at);
        } else {
            let module = prefix.join("::");
            self.import(&module, name, rename.as_deref().unwrap_or(name), at);
        }
    }

    /// Visits the expressions inside a macro invocation, when its tokens read as expressions.
    fn macro_body(&mut self, mac: &Macro) {
        let name = mac.path.segments.last().map(|segment| plain(&segment.ident)).unwrap_or_default();
        if name == "macro_rules" {
            return;
        }
        // In `matches!(value, pattern)` only the first argument is an expression.
        if matches!(name.as_str(), "matches" | "assert_matches" | "debug_assert_matches") {
            if let Some(first) = split_args(&mac.tokens).into_iter().next() {
                if let Ok(expr) = syn::parse2::<Expr>(first) {
                    syn::visit::visit_expr(self, &expr);
                }
            }
            return;
        }
        let list = Punctuated::<Expr, syn::Token![,]>::parse_terminated.parse2(mac.tokens.clone());
        match list {
            Ok(exprs) => {
                for expr in &exprs {
                    syn::visit::visit_expr(self, expr);
                }
            }
            Err(_) => {
                for segment in split_args(&mac.tokens) {
                    if let Ok(expr) = syn::parse2::<Expr>(segment) {
                        syn::visit::visit_expr(self, &expr);
                    }
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for Walker<'_> {
    fn visit_item_fn(&mut self, i: &'ast ItemFn) {
        let sym = self.define(Def {
            name: plain(&i.sig.ident),
            kind: SymKind::Function,
            start: self.first(&[vis(&i.vis), tok(&i.modifiers.defaultness), Some(i.sig.span())]),
            end: close(&i.block.brace_token.span).end,
            cut: open(&i.block.brace_token.span).start,
            attrs: &i.attrs,
            bases: Vec::new(),
        });
        self.open(sym);
        syn::visit::visit_item_fn(self, i);
        self.scopes.pop();
    }

    fn visit_impl_item_fn(&mut self, i: &'ast ImplItemFn) {
        let sym = self.define(Def {
            name: plain(&i.sig.ident),
            kind: SymKind::Method,
            start: self.first(&[vis(&i.vis), tok(&i.modifiers.defaultness), Some(i.sig.span())]),
            end: close(&i.block.brace_token.span).end,
            cut: open(&i.block.brace_token.span).start,
            attrs: &i.attrs,
            bases: Vec::new(),
        });
        self.open(sym);
        syn::visit::visit_impl_item_fn(self, i);
        self.scopes.pop();
    }

    fn visit_trait_item_fn(&mut self, i: &'ast TraitItemFn) {
        let (end, cut) = match (&i.default, &i.semi_token) {
            (Some(block), _) => (close(&block.brace_token.span).end, open(&block.brace_token.span).start),
            (None, Some(semi)) => {
                let range = semi.span().byte_range();
                (range.end, range.start)
            }
            (None, None) => {
                let end = i.sig.span().byte_range().end;
                (end, end)
            }
        };
        let sym = self.define(Def {
            name: plain(&i.sig.ident),
            kind: SymKind::Method,
            start: self.first(&[tok(&i.modifiers.defaultness), Some(i.sig.span())]),
            end,
            cut,
            attrs: &i.attrs,
            bases: Vec::new(),
        });
        self.open(sym);
        syn::visit::visit_trait_item_fn(self, i);
        self.scopes.pop();
    }

    fn visit_item_struct(&mut self, i: &'ast ItemStruct) {
        let (end, cut) = match (&i.semi_token, &i.fields) {
            (Some(semi), _) => {
                let range = semi.span().byte_range();
                (range.end, range.start)
            }
            (None, Fields::Named(fields)) => (close(&fields.brace_token.span).end, open(&fields.brace_token.span).start),
            (None, fields) => {
                let end = fields.span().byte_range().end;
                (end, end)
            }
        };
        self.define(Def {
            name: plain(&i.ident),
            kind: SymKind::Struct,
            start: self.first(&[vis(&i.vis), Some(i.struct_token.span())]),
            end,
            cut,
            attrs: &i.attrs,
            bases: Vec::new(),
        });
        syn::visit::visit_item_struct(self, i);
    }

    fn visit_item_union(&mut self, i: &'ast ItemUnion) {
        self.define(Def {
            name: plain(&i.ident),
            kind: SymKind::Struct,
            start: self.first(&[vis(&i.vis), Some(i.union_token.span())]),
            end: close(&i.fields.brace_token.span).end,
            cut: open(&i.fields.brace_token.span).start,
            attrs: &i.attrs,
            bases: Vec::new(),
        });
        syn::visit::visit_item_union(self, i);
    }

    fn visit_item_enum(&mut self, i: &'ast ItemEnum) {
        self.define(Def {
            name: plain(&i.ident),
            kind: SymKind::Enum,
            start: self.first(&[vis(&i.vis), Some(i.enum_token.span())]),
            end: close(&i.brace_token.span).end,
            cut: open(&i.brace_token.span).start,
            attrs: &i.attrs,
            bases: Vec::new(),
        });
        syn::visit::visit_item_enum(self, i);
    }

    fn visit_item_trait(&mut self, i: &'ast ItemTrait) {
        let bases = i
            .supertraits
            .iter()
            .filter_map(|bound| match bound {
                TypeParamBound::Trait(t) if t.maybe.is_none() => Some(fold(&self.text_of(t.path.span()))),
                _ => None,
            })
            .collect();
        let sym = self.define(Def {
            name: plain(&i.ident),
            kind: SymKind::Trait,
            start: self.first(&[vis(&i.vis), tok(&i.modifiers.auto_token), tok(&i.unsafety), Some(i.trait_token.span())]),
            end: close(&i.brace_token.span).end,
            cut: open(&i.brace_token.span).start,
            attrs: &i.attrs,
            bases,
        });
        self.open(sym);
        syn::visit::visit_item_trait(self, i);
        self.scopes.pop();
    }

    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        let bases = match &i.trait_ {
            Some((path, _)) => vec![fold(&self.text_of(path.span()))],
            None => Vec::new(),
        };
        let sym = self.define(Def {
            name: self.type_name(&i.self_ty),
            kind: SymKind::Impl,
            start: self.first(&[tok(&i.modifiers.defaultness), tok(&i.unsafety), Some(i.impl_token.span())]),
            end: close(&i.brace_token.span).end,
            cut: open(&i.brace_token.span).start,
            attrs: &i.attrs,
            bases,
        });
        self.open(sym);
        syn::visit::visit_item_impl(self, i);
        self.scopes.pop();
    }

    fn visit_item_const(&mut self, i: &'ast ItemConst) {
        if i.ident != "_" {
            self.define(Def {
                name: plain(&i.ident),
                kind: SymKind::Const,
                start: self.first(&[vis(&i.vis), tok(&i.modifiers.defaultness), Some(i.const_token.span())]),
                end: i.semi_token.span().byte_range().end,
                cut: i.eq_token.span().byte_range().start,
                attrs: &i.attrs,
                bases: Vec::new(),
            });
        }
        syn::visit::visit_item_const(self, i);
    }

    fn visit_item_static(&mut self, i: &'ast ItemStatic) {
        self.define(Def {
            name: plain(&i.ident),
            kind: SymKind::Const,
            start: self.first(&[vis(&i.vis), Some(i.static_token.span())]),
            end: i.semi_token.span().byte_range().end,
            cut: i.eq_token.span().byte_range().start,
            attrs: &i.attrs,
            bases: Vec::new(),
        });
        syn::visit::visit_item_static(self, i);
    }

    fn visit_item_type(&mut self, i: &'ast ItemType) {
        self.define(Def {
            name: plain(&i.ident),
            kind: SymKind::TypeAlias,
            start: self.first(&[vis(&i.vis), tok(&i.modifiers.defaultness), Some(i.type_token.span())]),
            end: i.semi_token.span().byte_range().end,
            cut: i.eq_token.span().byte_range().start,
            attrs: &i.attrs,
            bases: Vec::new(),
        });
        syn::visit::visit_item_type(self, i);
    }

    fn visit_impl_item_const(&mut self, i: &'ast ImplItemConst) {
        if i.ident != "_" {
            self.define(Def {
                name: plain(&i.ident),
                kind: SymKind::Const,
                start: self.first(&[vis(&i.vis), tok(&i.modifiers.defaultness), Some(i.const_token.span())]),
                end: i.semi_token.span().byte_range().end,
                cut: i.eq_token.span().byte_range().start,
                attrs: &i.attrs,
                bases: Vec::new(),
            });
        }
        syn::visit::visit_impl_item_const(self, i);
    }

    fn visit_impl_item_type(&mut self, i: &'ast ImplItemType) {
        self.define(Def {
            name: plain(&i.ident),
            kind: SymKind::TypeAlias,
            start: self.first(&[vis(&i.vis), tok(&i.modifiers.defaultness), Some(i.type_token.span())]),
            end: i.semi_token.span().byte_range().end,
            cut: i.eq_token.span().byte_range().start,
            attrs: &i.attrs,
            bases: Vec::new(),
        });
        syn::visit::visit_impl_item_type(self, i);
    }

    fn visit_trait_item_const(&mut self, i: &'ast TraitItemConst) {
        let semi = i.semi_token.span().byte_range();
        let cut = i.default.as_ref().map_or(semi.start, |(eq, _)| eq.span().byte_range().start);
        if i.ident != "_" {
            self.define(Def {
                name: plain(&i.ident),
                kind: SymKind::Const,
                start: self.first(&[tok(&i.modifiers.defaultness), Some(i.const_token.span())]),
                end: semi.end,
                cut,
                attrs: &i.attrs,
                bases: Vec::new(),
            });
        }
        syn::visit::visit_trait_item_const(self, i);
    }

    fn visit_trait_item_type(&mut self, i: &'ast TraitItemType) {
        let semi = i.semi_token.span().byte_range();
        let cut = i.default.as_ref().map_or(semi.start, |(eq, _)| eq.span().byte_range().start);
        self.define(Def {
            name: plain(&i.ident),
            kind: SymKind::TypeAlias,
            start: self.first(&[tok(&i.modifiers.defaultness), Some(i.type_token.span())]),
            end: semi.end,
            cut,
            attrs: &i.attrs,
            bases: Vec::new(),
        });
        syn::visit::visit_trait_item_type(self, i);
    }

    fn visit_item_mod(&mut self, i: &'ast ItemMod) {
        let name = plain(&i.ident);
        if i.content.is_none() {
            let at = self.first(&[vis(&i.vis), tok(&i.unsafety), Some(i.mod_token.span())]);
            self.import(&name, "", &name, at);
            syn::visit::visit_item_mod(self, i);
            return;
        }
        let outer = self.scopes.last().cloned().unwrap_or(Scope { qual: String::new(), parent: None, caller: None });
        let qual = if outer.qual.is_empty() { name } else { format!("{}.{}", outer.qual, name) };
        self.scopes.push(Scope { qual, ..outer });
        syn::visit::visit_item_mod(self, i);
        self.scopes.pop();
    }

    fn visit_item_use(&mut self, i: &'ast ItemUse) {
        let at = self.first(&[vis(&i.vis), Some(i.use_token.span())]);
        self.use_tree(&i.tree, &mut Vec::new(), at);
        syn::visit::visit_item_use(self, i);
    }

    fn visit_item_extern_crate(&mut self, i: &'ast ItemExternCrate) {
        let at = self.first(&[vis(&i.vis), Some(i.extern_token.span())]);
        let name = plain(&i.ident);
        let alias = i.rename.as_ref().map_or_else(|| name.clone(), |(_, rename)| plain(rename));
        self.import(&name, "", &alias, at);
        syn::visit::visit_item_extern_crate(self, i);
    }

    fn visit_expr_call(&mut self, i: &'ast ExprCall) {
        if let Expr::Path(path) = &*i.func {
            let segments: Vec<String> = path.path.segments.iter().map(|segment| plain(&segment.ident)).collect();
            if let Some((name, before)) = segments.split_last() {
                let at = path.path.segments.last().map_or(0, |segment| segment.ident.span().byte_range().start);
                self.call(name.clone(), before.join("::"), at);
            }
        }
        syn::visit::visit_expr_call(self, i);
    }

    fn visit_expr_method_call(&mut self, i: &'ast ExprMethodCall) {
        let recv = self.receiver_text(i);
        self.call(plain(&i.method), recv, i.method.span().byte_range().start);
        syn::visit::visit_expr_method_call(self, i);
    }

    fn visit_expr_macro(&mut self, i: &'ast ExprMacro) {
        self.macro_body(&i.mac);
    }

    fn visit_macro(&mut self, i: &'ast Macro) {
        self.macro_body(i);
    }
}

impl Walker<'_> {
    /// The offset of the first of these spans that exists.
    fn first(&self, spans: &[Option<Span>]) -> usize {
        spans.iter().flatten().next().map_or(0, |span| span.byte_range().start)
    }

    /// The name an `impl` block is filed under: the self type without generics or references.
    fn type_name(&self, ty: &Type) -> String {
        let name = match ty {
            Type::Path(path) => path.path.segments.last().map(|segment| plain(&segment.ident)),
            Type::Reference(reference) => Some(self.type_name(&reference.elem)),
            Type::Paren(paren) => Some(self.type_name(&paren.elem)),
            Type::Group(group) => Some(self.type_name(&group.elem)),
            Type::Ptr(ptr) => Some(self.type_name(&ptr.elem)),
            _ => None,
        };
        match name {
            Some(name) if !name.is_empty() => name,
            _ => fold(&self.text_of(ty.span())),
        }
    }
}

/// The first method call on the way down the receiver of a chain, through the postfix
/// operators that do not change where the chain starts.
fn next_method_call(mut expr: &Expr) -> Option<&ExprMethodCall> {
    loop {
        expr = match expr {
            Expr::MethodCall(call) => return Some(call),
            Expr::Call(e) => &e.func,
            Expr::Field(e) => &e.base,
            Expr::Index(e) => &e.expr,
            Expr::Try(e) => &e.expr,
            Expr::Await(e) => &e.base,
            _ => return None,
        };
    }
}

/// The span of a visibility keyword, when the item has one.
fn vis(visibility: &Visibility) -> Option<Span> {
    match visibility {
        Visibility::Inherited => None,
        other => Some(other.span()),
    }
}

/// The span of an optional keyword.
fn tok<T: Spanned>(token: &Option<T>) -> Option<Span> {
    token.as_ref().map(|token| token.span())
}

fn open(delim: &proc_macro2::extra::DelimSpan) -> std::ops::Range<usize> {
    delim.open().byte_range()
}

fn close(delim: &proc_macro2::extra::DelimSpan) -> std::ops::Range<usize> {
    delim.close().byte_range()
}

/// An identifier without the `r#` of a raw identifier.
fn plain(ident: &proc_macro2::Ident) -> String {
    let name = ident.to_string();
    match name.strip_prefix("r#") {
        Some(rest) => rest.to_string(),
        None => name,
    }
}

/// The `///` and `#[doc = "..."]` lines of an item, one per line.
fn doc_of(attrs: &[Attribute]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for attr in attrs {
        if !matches!(attr.style, AttrStyle::Outer) {
            continue;
        }
        if let Meta::NameValue(pair) = &attr.meta {
            if let (true, Expr::Lit(ExprLit { lit: Lit::Str(text), .. })) = (pair.path.is_ident("doc"), &pair.value) {
                let value = text.value();
                if value.contains('\n') {
                    // A block comment: its lines may carry a leading star of their own.
                    for line in value.lines() {
                        let line = line.trim();
                        lines.push(line.strip_prefix('*').unwrap_or(line).trim().to_string());
                    }
                } else {
                    lines.push(value.strip_prefix(' ').unwrap_or(&value).to_string());
                }
            }
        }
    }
    lines.join("\n")
}

/// The tokens of a macro split at the commas and semicolons that are not inside brackets,
/// so that `vec![make(); n]` falls apart into its two expressions.
fn split_args(tokens: &TokenStream) -> Vec<TokenStream> {
    let mut segments = Vec::new();
    let mut current = TokenStream::new();
    for tree in tokens.clone() {
        if matches!(&tree, TokenTree::Punct(p) if matches!(p.as_char(), ',' | ';')) {
            segments.push(std::mem::take(&mut current));
        } else {
            current.extend(std::iter::once(tree));
        }
    }
    segments.push(current);
    segments.retain(|segment| !segment.is_empty());
    segments
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

#[cfg(test)]
mod tests {
    use super::*;
    use lxr_core::{Lang, ParseGrade};

    /// A source from lines, so that a line number in an expectation is the position in the list.
    fn src(lines: &[&str]) -> String {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    fn read_ok(lines: &[&str]) -> Parts {
        parse(&src(lines)).expect("the file parses")
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
    fn impl_blocks_for_a_type_and_for_a_trait() {
        let parts = read_ok(&[
            "use std::fmt::{self, Display};",
            "use std::io::{Read as R, *};",
            "",
            "/// A node.",
            "pub struct Node<T> {",
            "    value: T,",
            "}",
            "",
            "impl<T> Node<T> {",
            "    /// Makes a node.",
            "    pub fn new(value: T) -> Node<T> {",
            "        Node { value }",
            "    }",
            "}",
            "",
            "impl<T: Display> Display for Node<T> {",
            "    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {",
            "        write!(f, \"{}\", self.render())",
            "    }",
            "}",
        ]);
        assert_eq!(
            symbols(&parts),
            [
                r#"struct Node|Node 5-7 "pub struct Node<T>" "A node." parent=- bases=[]"#,
                r#"impl Node|Node 9-14 "impl<T> Node<T>" "" parent=- bases=[]"#,
                r#"method new|Node.new 11-13 "pub fn new(value: T) -> Node<T>" "Makes a node." parent=Node bases=[]"#,
                r#"impl Node|Node 16-20 "impl<T: Display> Display for Node<T>" "" parent=- bases=["Display"]"#,
                r#"method fmt|Node.fmt 17-19 "fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result" "" parent=Node bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["Node.fmt|self|render|18"]);
        assert_eq!(
            imports(&parts),
            ["std|fmt|fmt|1", "std::fmt|Display|Display|1", "std::io|Read|R|2", "std::io|*|*|2"]
        );
    }

    #[test]
    fn impl_names_ignore_generics_and_references() {
        let parts = read_ok(&[
            "unsafe impl Send for Raw {}",
            "impl<T> From<Vec<T>> for Wrapper<T> {",
            "    fn from(items: Vec<T>) -> Self { Wrapper(items) }",
            "}",
            "impl<'a> Show for &'a Foo {}",
            "impl Show for [u8] {}",
        ]);
        assert_eq!(
            symbols(&parts),
            [
                r#"impl Raw|Raw 1-1 "unsafe impl Send for Raw" "" parent=- bases=["Send"]"#,
                r#"impl Wrapper|Wrapper 2-4 "impl<T> From<Vec<T>> for Wrapper<T>" "" parent=- bases=["From<Vec<T>>"]"#,
                r#"method from|Wrapper.from 3-3 "fn from(items: Vec<T>) -> Self" "" parent=Wrapper bases=[]"#,
                r#"impl Foo|Foo 5-5 "impl<'a> Show for &'a Foo" "" parent=- bases=["Show"]"#,
                r#"impl [u8]|[u8] 6-6 "impl Show for [u8]" "" parent=- bases=["Show"]"#,
            ]
        );
        assert_eq!(calls(&parts), ["Wrapper.from||Wrapper|3"]);
    }

    #[test]
    fn trait_with_a_default_method() {
        let parts = read_ok(&[
            "pub trait Shape: Debug + Send {",
            "    const SIDES: u32;",
            "    type Unit;",
            "    fn area(&self) -> f64;",
            "    fn describe(&self) -> String {",
            "        format!(\"{} {}\", self.name(), self.area())",
            "    }",
            "}",
        ]);
        assert_eq!(
            symbols(&parts),
            [
                r#"trait Shape|Shape 1-8 "pub trait Shape: Debug + Send" "" parent=- bases=["Debug", "Send"]"#,
                r#"const SIDES|Shape.SIDES 2-2 "const SIDES: u32" "" parent=Shape bases=[]"#,
                r#"type Unit|Shape.Unit 3-3 "type Unit" "" parent=Shape bases=[]"#,
                r#"method area|Shape.area 4-4 "fn area(&self) -> f64" "" parent=Shape bases=[]"#,
                r#"method describe|Shape.describe 5-7 "fn describe(&self) -> String" "" parent=Shape bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["Shape.describe|self|name|6", "Shape.describe|self|area|6"]);
    }

    #[test]
    fn nested_modules_constants_and_nested_functions() {
        let parts = read_ok(&[
            "mod outer {",
            "    pub mod inner {",
            "        pub fn deep() { helper(); }",
            "    }",
            "    pub const LIMIT: usize = 10;",
            "}",
            "pub static NAME: &str = \"x\";",
            "type Alias = Vec<u8>;",
            "pub enum Kind { A, B(u8) }",
            "fn run(items: &[u8]) -> usize {",
            "    fn local(x: u8) -> u8 { x + 1 }",
            "    let total: usize = items.iter().map(|b| local(*b) as usize).sum();",
            "    let v = Vec::new();",
            "    println!(\"{}\", total);",
            "    total",
            "}",
        ]);
        assert_eq!(
            symbols(&parts),
            [
                r#"function deep|outer.inner.deep 3-3 "pub fn deep()" "" parent=- bases=[]"#,
                r#"const LIMIT|outer.LIMIT 5-5 "pub const LIMIT: usize" "" parent=- bases=[]"#,
                r#"const NAME|NAME 7-7 "pub static NAME: &str" "" parent=- bases=[]"#,
                r#"type Alias|Alias 8-8 "type Alias" "" parent=- bases=[]"#,
                r#"enum Kind|Kind 9-9 "pub enum Kind" "" parent=- bases=[]"#,
                r#"function run|run 10-16 "fn run(items: &[u8]) -> usize" "" parent=- bases=[]"#,
                r#"function local|run.local 11-11 "fn local(x: u8) -> u8" "" parent=run bases=[]"#,
            ]
        );
        assert_eq!(
            calls(&parts),
            [
                "outer.inner.deep||helper|3",
                "run|items.iter().map(|b| local(*b) as usize)|sum|12",
                "run|items.iter()|map|12",
                "run|items|iter|12",
                "run||local|12",
                "run|Vec|new|13",
            ]
        );
    }

    #[test]
    fn calls_inside_macros_and_closures() {
        let parts = read_ok(&[
            "fn main() {",
            "    let name = String::from(\"x\");",
            "    println!(\"{}\", compute(&name));",
            "    assert_eq!(parse(&name).len(), 3);",
            "    let v = vec![make(1), make(2)];",
            "    assert!(matches!(lookup(1), Some(_)));",
            "    let f = |x: i32| inner(x);",
            "    f(1);",
            "    let arr = [0u8; 4];",
            "    debug_assert!(v.len() > 1, \"bad {}\", describe(v.len()));",
            "    let w = vec![zero(); 3];",
            "}",
        ]);
        assert_eq!(
            calls(&parts),
            [
                "main|String|from|2",
                "main||compute|3",
                "main|parse(&name)|len|4",
                "main||parse|4",
                "main||make|5",
                "main||make|5",
                "main||lookup|6",
                "main||inner|7",
                "main||f|8",
                "main|v|len|10",
                "main||describe|10",
                "main|v|len|10",
                "main||zero|11",
            ]
        );
    }

    #[test]
    fn macro_definitions_are_skipped_but_their_uses_are_read() {
        let parts = read_ok(&[
            "macro_rules! twice { ($e:expr) => { run($e) + run($e) }; }",
            "fn main() { twice!(f(1)); }",
        ]);
        assert_eq!(calls(&parts), ["main||f|2"]);
    }

    #[test]
    fn attributes_and_documentation_do_not_move_the_start_line() {
        let parts = read_ok(&[
            "#[inline]",
            "#[must_use]",
            "/// Doc line one.",
            "/// Doc line two.",
            "pub(crate) async fn go<T: Read>(reader: T) -> io::Result<Index> where T: Send {",
            "    reader.read_all().await",
            "}",
            "",
            "#[doc = \"Explicit doc.\"]",
            "fn explicit() {}",
            "",
            "/**",
            " * Block doc.",
            " * More.",
            " */",
            "fn block() {}",
        ]);
        assert_eq!(
            symbols(&parts),
            [
                r#"function go|go 5-7 "pub(crate) async fn go<T: Read>(reader: T) -> io::Result<Index> where T: Send" "Doc line one.\nDoc line two." parent=- bases=[]"#,
                r#"function explicit|explicit 10-10 "fn explicit()" "Explicit doc." parent=- bases=[]"#,
                r#"function block|block 16-16 "fn block()" "\nBlock doc.\nMore.\n" parent=- bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["go|reader|read_all|6"]);
    }

    #[test]
    fn modules_extern_crates_and_use_trees() {
        let parts = read_ok(&[
            "mod parser;",
            "extern crate serde as sd;",
            "use crate::store::{self as st, load};",
            "use super::*;",
            "pub use a::b;",
            "use ::std::collections::HashMap;",
            "use rand;",
        ]);
        assert_eq!(
            imports(&parts),
            [
                "parser||parser|1",
                "serde||sd|2",
                "crate|store|st|3",
                "crate::store|load|load|3",
                "super|*|*|4",
                "a|b|b|5",
                "std::collections|HashMap|HashMap|6",
                "|rand|rand|7",
            ]
        );
    }

    #[test]
    fn struct_enum_and_union_forms() {
        let parts = read_ok(&[
            "pub struct Id(pub u32);",
            "struct Unit;",
            "pub struct Pair<A, B> where A: Copy { a: A, b: B }",
            "pub enum E<T> { A(T) }",
            "union U { a: u32, b: f32 }",
            "pub struct Spread {",
            "    x: i32,",
            "}",
        ]);
        assert_eq!(
            symbols(&parts),
            [
                r#"struct Id|Id 1-1 "pub struct Id(pub u32)" "" parent=- bases=[]"#,
                r#"struct Unit|Unit 2-2 "struct Unit" "" parent=- bases=[]"#,
                r#"struct Pair|Pair 3-3 "pub struct Pair<A, B> where A: Copy" "" parent=- bases=[]"#,
                r#"enum E|E 4-4 "pub enum E<T>" "" parent=- bases=[]"#,
                r#"struct U|U 5-5 "union U" "" parent=- bases=[]"#,
                r#"struct Spread|Spread 6-8 "pub struct Spread" "" parent=- bases=[]"#,
            ]
        );
    }

    #[test]
    fn items_inside_a_function_belong_to_it() {
        let parts = read_ok(&[
            "fn outer() {",
            "    struct Local;",
            "    impl Local {",
            "        fn m(&self) { z(); }",
            "    }",
            "}",
        ]);
        assert_eq!(
            symbols(&parts),
            [
                r#"function outer|outer 1-6 "fn outer()" "" parent=- bases=[]"#,
                r#"struct Local|outer.Local 2-2 "struct Local" "" parent=outer bases=[]"#,
                r#"impl Local|outer.Local 3-5 "impl Local" "" parent=outer bases=[]"#,
                r#"method m|outer.Local.m 4-4 "fn m(&self)" "" parent=outer.Local bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["outer.Local.m||z|4"]);
    }

    #[test]
    fn a_shebang_and_a_byte_order_mark_keep_the_line_numbers() {
        let parts = parse("#!/usr/bin/env rust-script\nfn main() { go(); }\n").expect("parses");
        assert_eq!(symbols(&parts), [r#"function main|main 2-2 "fn main()" "" parent=- bases=[]"#]);
        assert_eq!(calls(&parts), ["main||go|2"]);
        let parts = parse("\u{feff}fn a() {}\n").expect("parses");
        assert_eq!(symbols(&parts), [r#"function a|a 1-1 "fn a()" "" parent=- bases=[]"#]);
        // An inner attribute is not a shebang.
        let parts = parse("#![allow(dead_code)]\nfn b() {}\n").expect("parses");
        assert_eq!(symbols(&parts), [r#"function b|b 2-2 "fn b()" "" parent=- bases=[]"#]);
    }

    #[test]
    fn non_ascii_text_keeps_offsets_right() {
        let parts = read_ok(&["const S: &str = \"héllo wörld\";", "/// Zażółć.", "fn ünï(ä: u8) { ö(ä); }"]);
        assert_eq!(
            symbols(&parts),
            [
                r#"const S|S 1-1 "const S: &str" "" parent=- bases=[]"#,
                r#"function ünï|ünï 3-3 "fn ünï(ä: u8)" "Zażółć." parent=- bases=[]"#,
            ]
        );
        assert_eq!(calls(&parts), ["ünï||ö|3"]);
    }

    #[test]
    fn extract_keeps_the_first_documentation_line_and_grades_the_file() {
        let text = src(&["/// First.", "/// Second.", "pub fn f() {}"]);
        let facts = crate::extract("src/f.rs", Lang::Rust, &text);
        assert_eq!(facts.grade, ParseGrade::Ast);
        assert_eq!(facts.symbols[0].doc, "First.");
        assert_eq!((facts.symbols[0].start, facts.symbols[0].end), (3, 3));
    }

    #[test]
    fn a_file_syn_rejects_falls_back_to_the_lexical_extractor() {
        let facts = crate::extract("broken.rs", Lang::Rust, "fn ( {\n");
        assert_eq!(facts.grade, ParseGrade::Lexical);
    }

    #[test]
    fn bracket_depth_ignores_strings_characters_lifetimes_and_comments() {
        assert_eq!(nesting_depth("f(g([1]))"), 3);
        assert_eq!(
            nesting_depth(r##"fn f<'a>(x: &'a str) { let c = '('; let d = '\''; let e = '\\'; let s = r#"((( "#; /* ((( /* ((( */ ((( */ }"##),
            1
        );
        assert_eq!(nesting_depth(""), 0);
    }

    fn assert_survives(text: &str) {
        // Whether syn accepts the text or not, nothing may panic.
        let _ = parse(text);
        let facts = crate::extract("odd.rs", Lang::Rust, text);
        assert!(facts.symbols.iter().all(|s| s.start >= 1 && s.end >= s.start));
    }

    #[test]
    fn empty_and_comment_only_files() {
        assert_survives("");
        assert_survives("// nothing here\n/* nor here */\n");
        assert_survives("/// a doc comment with nothing after it\n");
        let parts = parse("").expect("an empty file is a file");
        assert!(parts.symbols.is_empty() && parts.calls.is_empty() && parts.imports.is_empty());
    }

    #[test]
    fn a_very_long_single_line() {
        let many = "fn f() { g(); } ".repeat(12_500);
        let parts = parse(&many).expect("many functions on one line");
        assert_eq!(parts.symbols.len(), 12_500);
        assert_eq!(parts.calls.len(), 12_500);
        assert_survives(&format!("fn t() {{ let x = {}; }}", "1 + ".repeat(50_000) + "1"));
        assert_survives(&format!("fn t() {{ x{}; }}", ".m()".repeat(30_000)));
    }

    #[test]
    fn deeply_nested_brackets() {
        let parens = format!("fn f() {{ let x = {}1{}; }}", "(".repeat(500), ")".repeat(500));
        assert_survives(&parens);
        let calls = format!("fn f() {{ {}1{}; }}", "g(".repeat(500), ")".repeat(500));
        let parts = parse(&calls).expect("500 nested calls parse");
        assert_eq!(parts.calls.len(), 500);
        let blocks = format!("fn f() {}{}", "{".repeat(2000), "}".repeat(2000));
        assert_survives(&blocks);
        // Far deeper than any real file: refused rather than risked.
        assert!(parse(&format!("fn f() {{ {}{} }}", "(".repeat(50_000), ")".repeat(50_000))).is_none());
    }

    /// The deepest nesting that is still parsed must fit in the stack reserved for it.
    #[test]
    #[ignore = "uses a few hundred megabytes of stack"]
    fn the_deepest_accepted_nesting_fits_the_deep_stack() {
        let n = MAX_DEPTH - 10;
        for text in [
            format!("fn f() {{ let x = {}1{}; }}", "(".repeat(n), ")".repeat(n)),
            format!("fn f() {{ {}1{}; }}", "g(".repeat(n), ")".repeat(n)),
            format!("fn f() {}{}", "{".repeat(n), "}".repeat(n)),
            format!("fn f() {{ let x = {}0{}; }}", "[".repeat(n), "]".repeat(n)),
            format!("fn f() {{ let x = {}1; }}", "|a| ".repeat(n)),
        ] {
            assert!(parse(&text).is_some());
        }
    }

    #[test]
    fn text_with_replaced_invalid_utf8() {
        let bytes = b"/// d\xff\nfn f\xff() { g(\xe2\x82); }\nconst S: &str = \"\xc3(\";\n";
        let text = String::from_utf8_lossy(bytes).into_owned();
        assert!(text.contains('\u{fffd}'));
        assert_survives(&text);
        let bytes = b"/// d\xff\npub fn ok() { g(); }\nconst S: &str = \"\xc3(\";\n";
        let text = String::from_utf8_lossy(bytes).into_owned();
        let parts = parse(&text).expect("a replacement character inside a string or comment is fine");
        assert_eq!(parts.symbols.len(), 2);
    }

    /// Runs `extract_path` over every Rust file under the directories named by `LXR_RS_DIRS`
    /// (separated by `;`) and prints the figures. Does nothing when the variable is unset.
    ///
    /// `cargo test -p lxr-extract corpus_rust -- --ignored --nocapture`
    #[test]
    #[ignore = "reads real files named by LXR_RS_DIRS"]
    fn corpus_rust() {
        let Ok(dirs) = std::env::var("LXR_RS_DIRS") else { return };
        let mut files = Vec::new();
        for dir in dirs.split(';').filter(|d| !d.is_empty()) {
            collect(std::path::Path::new(dir), &mut files);
        }
        let (mut ast, mut lexical, mut symbols, mut calls, mut imports) = (0, 0, 0, 0, 0);
        let started = std::time::Instant::now();
        for path in &files {
            let root = path.parent().unwrap_or(path);
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let Some(facts) = crate::extract_path(root, name) else { continue };
            match facts.grade {
                ParseGrade::Ast => ast += 1,
                ParseGrade::Lexical => {
                    lexical += 1;
                    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned();
                    let reason = match syn::parse_file(source_body(&text)) {
                        Ok(_) => "nesting deeper than the limit".to_string(),
                        Err(error) => error.to_string(),
                    };
                    println!("LEXICAL {} ({})", path.display(), reason);
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
    }

    fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if name != ".git" && name != "target" {
                    collect(&path, out);
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}
