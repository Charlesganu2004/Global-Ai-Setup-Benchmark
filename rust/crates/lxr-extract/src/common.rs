//! What every extractor shares: the shape they return, line lookup, and the last
//! pass that turns raw parts into checked [`FileFacts`].

use lxr_core::{hash64, Call, FileFacts, Import, Lang, ParseGrade, Symbol};

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

/// The last pass: source order, valid indexes, clipped text and content hashes.
pub fn finish(path: &str, lang: Lang, text: &str, grade: ParseGrade, parts: Parts) -> FileFacts {
    let index = LineIndex::new(text);
    let lines = index.count();
    let Parts { symbols, mut calls, mut imports } = parts;

    // Source order, a container before what it holds. Indexes are remapped to match.
    let mut order: Vec<usize> = (0..symbols.len()).collect();
    order.sort_by_key(|&i| (symbols[i].start, std::cmp::Reverse(symbols[i].end), i));
    let mut new_index = vec![0u32; symbols.len()];
    for (new, &old) in order.iter().enumerate() {
        new_index[old] = new as u32;
    }
    let remap = |old: Option<u32>| old.and_then(|i| new_index.get(i as usize).copied());

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
        symbol.hash = hash64(&text.as_bytes()[from..to]);
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
}
