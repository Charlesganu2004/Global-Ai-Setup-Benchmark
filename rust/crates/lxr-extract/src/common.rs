//! What every extractor shares: the shape they return, line lookup, and the last
//! pass that turns raw parts into checked [`FileFacts`].

use lxr_core::{hash64, Call, FileFacts, Import, Lang, ParseGrade, Symbol};
use std::collections::HashMap;

/// What an extractor found in one file, before the last pass.
///
/// An extractor may leave `Symbol::hash` at 0 and need not clip `sig` or `doc`:
/// [`finish`] does both. `parent` and `caller` are indexes into `symbols` as returned.
#[derive(Default, Debug)]
pub struct Parts {
    pub symbols: Vec<Symbol>,
    pub calls: Vec<Call>,
    pub imports: Vec<Import>,
}

/// Byte offset to line number, for a parser that reports byte offsets.
pub struct LineIndex {
    starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> LineIndex {
        let mut starts = vec![0];
        starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        LineIndex { starts, len: text.len() }
    }

    /// The line, counted from 1, that holds the byte at `offset`.
    pub fn line_of(&self, offset: usize) -> u32 {
        let offset = offset.min(self.len);
        match self.starts.binary_search(&offset) {
            Ok(i) => i as u32 + 1,
            Err(i) => i as u32,
        }
    }

    /// The line that holds the last byte of a range that ends, exclusively, at `end`.
    pub fn last_line_of(&self, end: usize) -> u32 {
        self.line_of(end.saturating_sub(1))
    }

    /// Number of lines. A file that ends in a newline does not count an empty last line.
    pub fn count(&self) -> u32 {
        let last = *self.starts.last().unwrap_or(&0);
        let lines = if last >= self.len && self.starts.len() > 1 { self.starts.len() - 1 } else { self.starts.len() };
        if self.len == 0 { 0 } else { lines as u32 }
    }

    /// The bytes of lines `first` to `last`, both counted from 1 and inclusive.
    fn span(&self, first: u32, last: u32) -> (usize, usize) {
        let from = self.starts.get(first.saturating_sub(1) as usize).copied().unwrap_or(self.len);
        let to = self.starts.get(last as usize).copied().unwrap_or(self.len);
        (from.min(self.len), to.max(from).min(self.len))
    }
}

/// Runs of white space folded to one space, ends trimmed.
pub fn fold(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` cut to at most `max` characters. A cut text ends in `...`.
pub fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(3)).collect();
    cut.push_str("...");
    cut
}

/// The first line of a documentation comment that says something.
pub fn doc_line(doc: &str) -> String {
    doc.lines().map(str::trim).find(|line| !line.is_empty()).map(fold).unwrap_or_default()
}

/// `text` cut to at most `max` bytes, at a character boundary.
pub fn capped(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// `text[from..to]` with every comment in `comments` replaced by one space, cut to at most
/// `max` bytes. `comments` are byte ranges of `text` in source order. A signature is cut from
/// the source, and once its line breaks are folded away a comment between two parameters would
/// read as part of it, and a line comment would seem to run on to the end of the signature.
pub fn without_comments(text: &str, from: usize, to: usize, comments: &[(usize, usize)], max: usize) -> String {
    let to = to.min(text.len());
    let mut out = String::new();
    let push = |out: &mut String, a: usize, b: usize| {
        let room = max.saturating_sub(out.len());
        out.push_str(capped(text.get(a..b).unwrap_or(""), room));
    };
    let mut at = from;
    for &(start, end) in &comments[comments.partition_point(|&(_, end)| end <= from)..] {
        if start >= to || out.len() >= max {
            break;
        }
        if start > at {
            push(&mut out, at, start);
        }
        out.push(' ');
        at = at.max(end.min(to));
    }
    if at < to {
        push(&mut out, at, to);
    }
    out
}

/// Puts `calls` in source order. `at[k]` is the byte offset of the name that `calls[k]` calls.
/// A syntax tree is walked from the outside in, so `x.first().second()` is met as `second` before
/// `first`, and `a() if b() else c()` as `b` before `a`; the offsets set that right. Calls at one
/// offset keep the order they came in.
pub fn in_source_order(calls: &mut Vec<Call>, at: &[usize]) {
    if at.windows(2).all(|pair| pair[0] <= pair[1]) {
        return;
    }
    let mut order: Vec<usize> = (0..calls.len()).collect();
    order.sort_by_key(|&k| at.get(k).copied().unwrap_or(usize::MAX));
    let mut slots: Vec<Option<Call>> = calls.drain(..).map(Some).collect();
    calls.extend(order.into_iter().filter_map(|k| slots[k].take()));
}

/// How many bytes are hashed from each end of a definition that is longer than twice this.
const HASH_EDGE: usize = 64 * 1024;

/// A hash of the source of one definition. A definition longer than `2 * HASH_EDGE` bytes is
/// hashed by its first and last `HASH_EDGE` bytes and its length: a change in the middle of
/// it that keeps its length goes unseen, which is the price of a cost per definition that
/// does not grow with the definition, in a file where definitions contain one another.
fn span_hash(bytes: &[u8]) -> u64 {
    if bytes.len() <= 2 * HASH_EDGE {
        return hash64(bytes);
    }
    let mut parts = [0u8; 24];
    parts[..8].copy_from_slice(&hash64(&bytes[..HASH_EDGE]).to_le_bytes());
    parts[8..16].copy_from_slice(&hash64(&bytes[bytes.len() - HASH_EDGE..]).to_le_bytes());
    parts[16..].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
    hash64(&parts)
}

/// The last pass: source order, valid indexes, clipped text and content hashes.
pub fn finish(path: &str, lang: Lang, text: &str, grade: ParseGrade, parts: Parts) -> FileFacts {
    let index = LineIndex::new(text);
    let lines = index.count();
    let Parts { symbols, mut calls, mut imports } = parts;

    // Source order, a container before what it holds. Indexes are remapped to match. An extractor
    // returns a container before its contents and siblings in the order it met them, which is
    // source order, so the position it gave is what breaks a tie between symbols that start on
    // one line: `fn a() {} fn b() {` keeps `a` before `b`, and `class A { m() {} }` keeps `A` first.
    let mut order: Vec<usize> = (0..symbols.len()).collect();
    order.sort_by_key(|&i| (symbols[i].start, i));
    let mut new_index = vec![0u32; symbols.len()];
    for (new, &old) in order.iter().enumerate() {
        new_index[old] = new as u32;
    }
    let remap = |old: Option<u32>| old.and_then(|i| new_index.get(i as usize).copied());

    // Spans are line-granular, so definitions that share their lines share their hash.
    let mut hashes: HashMap<(usize, usize), u64> = HashMap::new();
    let mut sorted: Vec<Symbol> = Vec::with_capacity(symbols.len());
    let mut slots: Vec<Option<Symbol>> = symbols.into_iter().map(Some).collect();
    for (new, &old) in order.iter().enumerate() {
        let mut symbol = slots[old].take().expect("each symbol is moved once");
        symbol.start = symbol.start.clamp(1, lines.max(1));
        symbol.end = symbol.end.clamp(symbol.start, lines.max(symbol.start));
        symbol.sig = clip(&fold(&symbol.sig), 240);
        symbol.doc = clip(&doc_line(&symbol.doc), 200);
        symbol.parent = remap(symbol.parent).filter(|&p| (p as usize) < new);
        let (from, to) = index.span(symbol.start, symbol.end);
        symbol.hash = *hashes.entry((from, to)).or_insert_with(|| span_hash(&text.as_bytes()[from..to]));
        sorted.push(symbol);
    }

    calls.retain(|call| !call.name.is_empty());
    for call in &mut calls {
        call.caller = remap(call.caller);
        call.recv = clip(&fold(&call.recv), 60);
        call.line = call.line.clamp(1, lines.max(1));
    }
    calls.sort_by_key(|call| call.line);
    imports.retain(|import| !import.module.is_empty() || !import.name.is_empty());
    imports.sort_by_key(|import| import.line);

    FileFacts {
        path: path.replace('\\', "/"),
        lang,
        lines,
        bytes: text.len() as u64,
        hash: hash64(text.as_bytes()),
        grade,
        symbols: sorted,
        calls,
        imports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lxr_core::SymKind;

    fn symbol(name: &str, start: u32, end: u32, parent: Option<u32>) -> Symbol {
        Symbol {
            name: name.to_string(),
            qual: name.to_string(),
            kind: SymKind::Function,
            start,
            end,
            sig: format!("def   {name}( )"),
            doc: "\n  First line.\n  Second.".to_string(),
            parent,
            bases: Vec::new(),
            hash: 0,
        }
    }

    #[test]
    fn line_index_counts_and_finds_lines() {
        let index = LineIndex::new("a\nbb\nccc\n");
        assert_eq!(index.count(), 3);
        assert_eq!(index.line_of(0), 1);
        assert_eq!(index.line_of(2), 2);
        assert_eq!(index.line_of(5), 3);
        assert_eq!(index.last_line_of(5), 2);
        assert_eq!(LineIndex::new("").count(), 0);
        assert_eq!(LineIndex::new("no newline").count(), 1);
    }

    #[test]
    fn finish_orders_symbols_and_remaps_indexes() {
        let text = "class A:\n    def m(self):\n        pass\ndef f():\n    A().m()\n";
        let parts = Parts {
            // Given out of order on purpose: f, then m (child of A, index 2), then A.
            symbols: vec![symbol("f", 4, 5, None), symbol("m", 2, 3, Some(2)), symbol("A", 1, 3, None)],
            calls: vec![Call { caller: Some(0), name: "m".to_string(), recv: "A()".to_string(), line: 5 }],
            imports: Vec::new(),
        };
        let facts = finish("pkg\\mod.py", Lang::Python, text, ParseGrade::Ast, parts);
        let names: Vec<&str> = facts.symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["A", "m", "f"]);
        assert_eq!(facts.symbols[1].parent, Some(0));
        assert_eq!(facts.calls[0].caller, Some(2));
        assert_eq!(facts.path, "pkg/mod.py");
        assert_eq!(facts.lines, 5);
        assert_eq!(facts.symbols[0].sig, "def A( )");
        assert_eq!(facts.symbols[0].doc, "First line.");
        assert_ne!(facts.symbols[0].hash, 0);
        assert_ne!(facts.symbols[0].hash, facts.symbols[2].hash);
    }

    #[test]
    fn clip_marks_a_cut() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("abcdefghij", 6), "abc...");
    }

    #[test]
    fn capped_cuts_at_a_character_boundary() {
        assert_eq!(capped("abcdef", 4), "abcd");
        assert_eq!(capped("abc", 4), "abc");
        // Each of these characters is two bytes, so a cut at 3 would split the second.
        assert_eq!(capped("éééé", 3), "é");
    }

    #[test]
    fn comments_are_replaced_by_a_space_in_a_cut_of_the_source() {
        let text = "fn f(a, // one\n b /* two */, c) {}";
        let one = text.find("// one").unwrap();
        let two = text.find("/* two */").unwrap();
        let comments = [(one, one + "// one".len()), (two, two + "/* two */".len())];
        let end = text.find(" {}").unwrap();
        assert_eq!(fold(&without_comments(text, 0, end, &comments, usize::MAX)), "fn f(a, b , c)");
        // A comment that began before the cut is still a comment.
        assert_eq!(without_comments("/* x */fn", 3, 9, &[(0, 7)], usize::MAX), " fn");
        assert_eq!(without_comments("abcdef", 0, 6, &[], 4), "abcd");
        assert_eq!(without_comments("abcdef", 4, 2, &[], 9), "");
    }

    #[test]
    fn the_hash_of_a_long_definition_covers_its_ends_and_its_length() {
        let short = vec![b'a'; 100];
        let mut changed = short.clone();
        changed[50] = b'b';
        assert_ne!(span_hash(&short), span_hash(&changed), "a short definition is hashed whole");

        let long = vec![b'a'; 4 * HASH_EDGE];
        let mut middle = long.clone();
        middle[2 * HASH_EDGE] = b'b';
        assert_eq!(span_hash(&long), span_hash(&middle), "the middle of a long one is not read");
        for at in [0, HASH_EDGE - 1, 3 * HASH_EDGE + 1, 4 * HASH_EDGE - 1] {
            let mut edited = long.clone();
            edited[at] = b'b';
            assert_ne!(span_hash(&long), span_hash(&edited), "byte {at}");
        }
        assert_ne!(span_hash(&long), span_hash(&vec![b'a'; 4 * HASH_EDGE + 1]));
        // The longest definition that is still hashed whole.
        let mut edge = vec![b'a'; 2 * HASH_EDGE];
        let whole = span_hash(&edge);
        edge[HASH_EDGE] = b'b';
        assert_ne!(whole, span_hash(&edge));
    }

    #[test]
    fn definitions_on_the_same_lines_get_the_same_hash() {
        let text = "def a(): pass\nx = 1\n";
        let parts = Parts { symbols: vec![symbol("a", 1, 1, None), symbol("b", 1, 1, None), symbol("c", 2, 2, None)], ..Parts::default() };
        let facts = finish("t.py", Lang::Python, text, ParseGrade::Ast, parts);
        assert_eq!(facts.symbols[0].hash, facts.symbols[1].hash);
        assert_ne!(facts.symbols[0].hash, facts.symbols[2].hash);
        assert_eq!(facts.symbols[0].hash, hash64(b"def a(): pass\n"));
    }
}
