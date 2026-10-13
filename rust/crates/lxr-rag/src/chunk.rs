//! Cuts a source file into chunks that follow its definitions.

use lxr_core::FileFacts;

/// A definition whose own text is at most this many characters is one chunk.
///
/// Measured on the lxbench fixture with searchdemo in Hybrid mode and five hits, each
/// function searched for by the first sentence of its docstring: a limit of 750 finds 894
/// of 951 functions (hit@5 0.940) and a limit of 1,500 finds 869 (0.914); on a sample of
/// 60 of them it finds 57 and 56. The limit equals [`PIECE_SIZE`], so a definition above it
/// is cut into pieces, and only a short last piece (see [`TAIL_MIN`]) can make it one chunk again.
const WHOLE_LIMIT: usize = 750;
/// Pieces of a long definition, of a container's loose lines and of module-level code
/// are filled up to this many characters.
const PIECE_SIZE: usize = 750;
/// A last piece of a long definition shorter than this joins the piece before it.
const TAIL_MIN: usize = 150;
/// Owner of a line that lies in no definition.
const NONE: usize = usize::MAX;

/// What a chunk is: a whole definition, one slice of a long definition or of a
/// container, or lines outside every definition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChunkKind {
    /// A definition with no other definition inside it, in one piece.
    Definition,
    /// One slice of a long definition, or a container's own lines between its members.
    Part,
    /// Lines that lie in no definition: imports and module-level code.
    Module,
}

/// A run of whole lines of one file, and the definition it lies in.
#[derive(Clone, PartialEq, Debug)]
pub struct Chunk {
    /// Path of the file from the repository root.
    pub path: String,
    /// First line of the chunk, counted from 1.
    pub start: u32,
    /// Last line of the chunk, inclusive.
    pub end: u32,
    /// What the chunk is.
    pub kind: ChunkKind,
    /// Qualified name of the innermost definition the chunk lies in, empty for a module chunk.
    pub symbol: String,
    /// Declaration line of that definition, empty for a module chunk.
    pub sig: String,
    /// First sentence of that definition's documentation, empty when there is none.
    pub doc: String,
    /// First line of that definition (its declaration), 0 for a module chunk.
    pub def_start: u32,
    /// Last line of that definition, 0 for a module chunk.
    pub def_end: u32,
    /// The lines of the chunk, joined by `\n`, without carriage returns.
    pub text: String,
}

/// The lines of a file with the character counts needed to size chunks. Line numbers
/// are counted from 1, as everywhere else.
struct Source<'a> {
    lines: Vec<&'a str>,
    /// `prefix[l]` is the number of characters of lines `1..=l`, a newline counted for each.
    prefix: Vec<usize>,
}

impl<'a> Source<'a> {
    fn new(text: &'a str) -> Source<'a> {
        let mut lines: Vec<&str> = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect();
        if lines.last() == Some(&"") {
            lines.pop();
        }
        let mut prefix = Vec::with_capacity(lines.len() + 1);
        prefix.push(0);
        for line in &lines {
            let last = prefix[prefix.len() - 1];
            prefix.push(last + line.chars().count() + 1);
        }
        Source { lines, prefix }
    }

    fn count(&self) -> usize {
        self.lines.len()
    }

    fn line(&self, l: usize) -> &'a str {
        self.lines[l - 1]
    }

    fn blank(&self, l: usize) -> bool {
        self.line(l).trim().is_empty()
    }

    /// Characters of lines `a..=b` joined by newlines.
    fn len(&self, a: usize, b: usize) -> usize {
        self.prefix[b] - self.prefix[a - 1] - 1
    }

    fn text(&self, a: usize, b: usize) -> String {
        self.lines[a - 1..b].join("\n")
    }

    /// The range without its blank lines at either end, or `None` when all are blank.
    fn trim(&self, mut a: usize, mut b: usize) -> Option<(usize, usize)> {
        while a <= b && self.blank(a) {
            a += 1;
        }
        while b >= a && self.blank(b) {
            b -= 1;
        }
        (a <= b).then_some((a, b))
    }

    /// Fills pieces line by line up to `max` characters. A line longer than `max` is a
    /// piece of its own. Blank lines at the ends of a piece are dropped, and a piece with
    /// nothing but blank lines is left out.
    fn pack(&self, a: usize, b: usize, max: usize) -> Vec<(usize, usize)> {
        let mut pieces = Vec::new();
        let mut from = a;
        for l in a..=b {
            if l > from && self.len(from, l) > max {
                pieces.extend(self.trim(from, l - 1));
                from = l;
            }
        }
        pieces.extend(self.trim(from, b));
        pieces
    }

    /// The pieces of a long definition within the lines `decl..=b`, where `decl` is its
    /// declaration line, so the first piece carries the signature.
    fn cut(&self, decl: usize, b: usize) -> Vec<(usize, usize)> {
        let mut pieces = self.pack(decl, b, PIECE_SIZE);
        if pieces.len() >= 2 {
            let (before, last) = (pieces[pieces.len() - 2], pieces[pieces.len() - 1]);
            if self.len(last.0, last.1) < TAIL_MIN && self.len(before.0, last.1) <= PIECE_SIZE + TAIL_MIN {
                pieces.pop();
                let at = pieces.len() - 1;
                pieces[at].1 = last.1;
            }
        }
        pieces
    }
}

/// Cuts a file into chunks, in line order.
///
/// A definition with no definition inside it is one [`ChunkKind::Definition`] chunk when
/// its text is at most 750 characters, and otherwise a run of [`ChunkKind::Part`]
/// chunks of about 750 characters cut at line ends, the first of which starts at the
/// declaration. A last piece shorter than 150 characters joins the one before it, so a
/// definition a little over 750 characters can still be one chunk. A definition that
/// holds others gives one part for its own lines up to the
/// first member, then its members are chunked on their own, and its lines between and
/// after the members are merged into parts of at most 750 characters. Lines in no
/// definition (imports, module-level code, and the comments and decorators above a
/// declaration, which are not part of the definition's span) become [`ChunkKind::Module`]
/// chunks of at most 750 characters. Every non-blank line is in exactly one chunk,
/// carriage returns are removed, and a chunk never starts or ends on a blank line.
pub fn chunk_file(facts: &FileFacts, text: &str) -> Vec<Chunk> {
    let source = Source::new(text);
    let n = source.count();
    if n == 0 {
        return Vec::new();
    }
    let spans: Vec<Option<(usize, usize)>> = facts
        .symbols
        .iter()
        .map(|s| {
            let (start, end) = (s.start as usize, (s.end as usize).min(n));
            (start >= 1 && start <= end).then_some((start, end))
        })
        .collect();
    let mut owner = vec![NONE; n + 2];
    paint(&spans, &mut owner);

    let mut leaf: Vec<Option<bool>> = vec![None; spans.len()];
    let mut chunks = Vec::new();
    let mut l = 1;
    while l <= n {
        let who = owner[l];
        let mut r = l;
        while r < n && owner[r + 1] == who {
            r += 1;
        }
        if who == NONE {
            for (a, b) in source.pack(l, r, PIECE_SIZE) {
                chunks.push(make(facts, &source, ChunkKind::Module, a, b, None));
            }
        } else if let Some((s, e)) = spans[who] {
            let whole = *leaf[who].get_or_insert_with(|| (s..=e).all(|x| owner[x] == who));
            let Some((a, b)) = source.trim(l, r) else {
                l = r + 1;
                continue;
            };
            let pieces = if a <= s && s <= b && source.len(s, b) <= WHOLE_LIMIT {
                vec![(a, b)]
            } else if a <= s && s <= b {
                source.cut(s, b)
            } else {
                source.pack(a, b, PIECE_SIZE)
            };
            let single = pieces.len() == 1;
            let kind = if whole && single { ChunkKind::Definition } else { ChunkKind::Part };
            for (x, y) in pieces {
                chunks.push(make(facts, &source, kind, x, y, Some(who)));
            }
        }
        l = r + 1;
    }
    chunks
}

/// Gives every line to the innermost definition that holds it: larger spans are laid
/// down first, so a smaller span inside one overwrites it, and of two equal spans the
/// later one in source order, which is the inner one, wins.
fn paint(spans: &[Option<(usize, usize)>], owner: &mut [usize]) {
    let mut order: Vec<usize> = (0..spans.len()).filter(|&i| spans[i].is_some()).collect();
    order.sort_by_key(|&i| {
        let (s, e) = spans[i].unwrap_or((0, 0));
        (std::cmp::Reverse(e - s), i)
    });
    for i in order {
        if let Some((s, e)) = spans[i] {
            owner[s..=e].fill(i);
        }
    }
}

/// One chunk of lines `a..=b`, of the definition numbered `who` in the facts, if any.
fn make(facts: &FileFacts, source: &Source, kind: ChunkKind, a: usize, b: usize, who: Option<usize>) -> Chunk {
    let (symbol, sig, doc, def_start, def_end) = match who.and_then(|i| facts.symbols.get(i)) {
        Some(s) => (s.qual.clone(), s.sig.clone(), s.doc.clone(), s.start, s.end.min(source.count() as u32)),
        None => (String::new(), String::new(), String::new(), 0, 0),
    };
    Chunk {
        path: facts.path.clone(),
        start: a as u32,
        end: b as u32,
        kind: if who.is_some() { kind } else { ChunkKind::Module },
        symbol,
        sig,
        doc,
        def_start,
        def_end,
        text: source.text(a, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lxr_core::{Lang, ParseGrade, SymKind, Symbol};

    fn symbol(qual: &str, kind: SymKind, start: u32, end: u32, parent: Option<u32>) -> Symbol {
        let name = qual.rsplit('.').next().unwrap_or(qual).to_string();
        Symbol {
            name,
            qual: qual.to_string(),
            kind,
            start,
            end,
            sig: format!("sig of {qual}"),
            doc: format!("doc of {qual}"),
            parent,
            bases: Vec::new(),
            hash: 0,
        }
    }

    fn facts(lang: Lang, text: &str, symbols: Vec<Symbol>) -> FileFacts {
        FileFacts {
            path: "pkg/mod.py".to_string(),
            lang,
            lines: text.lines().count() as u32,
            bytes: text.len() as u64,
            hash: 0,
            grade: ParseGrade::Ast,
            symbols,
            calls: Vec::new(),
            imports: Vec::new(),
        }
    }

    /// Every non-blank line is in exactly one chunk, chunks do not overlap and are in
    /// line order, and no chunk starts or ends on a blank line.
    fn assert_coverage(chunks: &[Chunk], text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        for (i, c) in chunks.iter().enumerate() {
            assert!(c.start >= 1 && c.start <= c.end && c.end as usize <= lines.len(), "{c:?}");
            assert!(!lines[c.start as usize - 1].trim().is_empty() && !lines[c.end as usize - 1].trim().is_empty(), "{c:?}");
            if i > 0 {
                assert!(chunks[i - 1].end < c.start, "overlap or disorder: {:?} {:?}", chunks[i - 1], c);
            }
            assert_eq!(c.text, lines[c.start as usize - 1..c.end as usize].join("\n"));
        }
        for (i, line) in lines.iter().enumerate() {
            let covering = chunks.iter().filter(|c| c.start as usize <= i + 1 && i < c.end as usize).count();
            if line.trim().is_empty() {
                assert!(covering <= 1, "blank line {} in {covering} chunks", i + 1);
            } else {
                assert_eq!(covering, 1, "line {} ({line:?}) is in {covering} chunks", i + 1);
            }
        }
    }

    fn range(c: &Chunk) -> (u32, u32) {
        (c.start, c.end)
    }

    #[test]
    fn short_function_is_one_definition_between_module_chunks() {
        let text = "import os\n\ndef f(x):\n    return x\n\nVALUE = 1\n";
        let f = facts(Lang::Python, text, vec![symbol("f", SymKind::Function, 3, 4, None)]);
        let chunks = chunk_file(&f, text);
        assert_coverage(&chunks, text);
        let shape: Vec<(ChunkKind, (u32, u32))> = chunks.iter().map(|c| (c.kind, range(c))).collect();
        assert_eq!(
            shape,
            vec![(ChunkKind::Module, (1, 1)), (ChunkKind::Definition, (3, 4)), (ChunkKind::Module, (6, 6))]
        );
        let def = &chunks[1];
        assert_eq!((def.symbol.as_str(), def.sig.as_str(), def.doc.as_str()), ("f", "sig of f", "doc of f"));
        assert_eq!((def.def_start, def.def_end), (3, 4));
        assert_eq!(def.text, "def f(x):\n    return x");
        assert_eq!(chunks[0].symbol, "");
        assert_eq!((chunks[0].def_start, chunks[0].def_end), (0, 0));
        assert_eq!(chunks[0].path, "pkg/mod.py");
    }

    #[test]
    fn long_function_is_cut_into_parts_that_start_at_the_declaration() {
        let mut text = String::from("import os\n\ndef big(x):\n");
        for i in 0..40 {
            text.push_str(&format!("    value_{i:02} = compute(x, {i}) + 1000000\n"));
        }
        let end = text.lines().count() as u32;
        let f = facts(Lang::Python, &text, vec![symbol("big", SymKind::Function, 3, end, None)]);
        let chunks = chunk_file(&f, &text);
        assert_coverage(&chunks, &text);
        let parts: Vec<&Chunk> = chunks.iter().filter(|c| c.kind == ChunkKind::Part).collect();
        assert!(parts.len() >= 2, "{chunks:?}");
        assert_eq!(parts[0].start, 3);
        assert!(parts[0].text.starts_with("def big(x):"));
        for p in &parts {
            assert!(p.text.chars().count() <= PIECE_SIZE, "{} characters", p.text.chars().count());
            assert_eq!((p.symbol.as_str(), p.def_start, p.def_end), ("big", 3, end));
            assert_eq!(p.sig, "sig of big");
        }
        assert_eq!(parts.last().map(|p| p.end), Some(end));
        assert!(chunks.iter().all(|c| c.kind != ChunkKind::Definition));
    }

    /// A Python function of exactly `size` characters whose last line is `last` characters
    /// long: the header, then lines of about 100 characters, then that last line.
    fn function_of(size: usize, last: usize) -> String {
        let middle = size - "def f():".len() - (1 + last);
        let (count, extra) = (middle / 101, middle % 101);
        let mut text = String::from("def f():");
        for i in 0..count {
            text.push_str(&format!("\n    {}", "x".repeat(96 + if i == 0 { extra } else { 0 })));
        }
        text.push_str(&format!("\n    {}", "x".repeat(last - 4)));
        assert_eq!(text.chars().count(), size);
        text
    }

    fn chunks_of_function(text: &str) -> Vec<Chunk> {
        let n = text.lines().count() as u32;
        let f = facts(Lang::Python, text, vec![symbol("f", SymKind::Function, 1, n, None)]);
        let chunks = chunk_file(&f, text);
        assert_coverage(&chunks, text);
        chunks
    }

    #[test]
    fn the_whole_limit_is_750_characters_and_a_short_tail_gets_a_little_more() {
        // Exactly 750 characters: one definition.
        let at_limit = chunks_of_function(&function_of(750, 40));
        assert_eq!(at_limit.len(), 1);
        assert_eq!(at_limit[0].kind, ChunkKind::Definition);
        // One more character and a last line of 200: cut at the last line, which is too long
        // to join the piece before it.
        let over = chunks_of_function(&function_of(751, 200));
        assert_eq!(over.len(), 2, "{over:?}");
        assert!(over.iter().all(|c| c.kind == ChunkKind::Part));
        // With a last line of 40 the piece that is cut off is under 150 characters, joins
        // the first piece, and the definition is whole again.
        let joined = chunks_of_function(&function_of(751, 40));
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].kind, ChunkKind::Definition);
        // A tail of 149 characters joins the piece before it and one of 150 does not.
        let tail_149 = chunks_of_function(&function_of(751, 149));
        assert_eq!((tail_149.len(), tail_149[0].kind), (1, ChunkKind::Definition));
        let tail_150 = chunks_of_function(&function_of(751, 150));
        assert_eq!(tail_150.len(), 2, "{tail_150:?}");
        // Beyond 750 plus the 150 a tail may add, it is cut whatever the last line is.
        let long = chunks_of_function(&function_of(901, 40));
        assert!(long.len() >= 2 && long.iter().all(|c| c.kind == ChunkKind::Part), "{long:?}");
    }

    #[test]
    fn class_gives_a_header_chunk_members_and_merged_loose_lines() {
        let text = "\
import os

class Config:
    \"\"\"Settings.\"\"\"
    LIMIT = 3

    def parse(self, s):
        return s

    # between members
    DEFAULT = 1

    def dump(self):
        return 2

    TRAILER = 9

print(Config)
";
        let f = facts(
            Lang::Python,
            text,
            vec![
                symbol("Config", SymKind::Class, 3, 16, None),
                symbol("Config.parse", SymKind::Method, 7, 8, Some(0)),
                symbol("Config.dump", SymKind::Method, 13, 14, Some(0)),
            ],
        );
        let chunks = chunk_file(&f, text);
        assert_coverage(&chunks, text);
        let shape: Vec<(ChunkKind, &str, (u32, u32))> = chunks.iter().map(|c| (c.kind, c.symbol.as_str(), range(c))).collect();
        assert_eq!(
            shape,
            vec![
                (ChunkKind::Module, "", (1, 1)),
                (ChunkKind::Part, "Config", (3, 5)),
                (ChunkKind::Definition, "Config.parse", (7, 8)),
                (ChunkKind::Part, "Config", (10, 11)),
                (ChunkKind::Definition, "Config.dump", (13, 14)),
                (ChunkKind::Part, "Config", (16, 16)),
                (ChunkKind::Module, "", (18, 18)),
            ]
        );
        for c in chunks.iter().filter(|c| c.symbol == "Config") {
            assert_eq!((c.def_start, c.def_end), (3, 16));
        }
        assert_eq!(chunks[2].def_start, 7);
    }

    #[test]
    fn module_level_code_is_merged_at_line_ends_up_to_750_characters() {
        let mut text = String::new();
        for i in 0..30 {
            text.push_str(&format!("CONSTANT_NUMBER_{i:02} = {} # filler\n", "9".repeat(40)));
        }
        let f = facts(Lang::Python, &text, Vec::new());
        let chunks = chunk_file(&f, &text);
        assert_coverage(&chunks, &text);
        assert!(chunks.len() >= 2);
        for c in &chunks {
            assert_eq!(c.kind, ChunkKind::Module);
            assert!(c.text.chars().count() <= PIECE_SIZE);
            assert_eq!((c.symbol.as_str(), c.def_start, c.def_end), ("", 0, 0));
        }
        assert_eq!(chunks[0].start, 1);
        assert_eq!(chunks.last().map(|c| c.end), Some(30));
        // A line is 70 characters, so ten lines are 709 and an eleventh would pass 750: three
        // full pieces. The literals keep this test from following a change of PIECE_SIZE.
        let sizes: Vec<usize> = chunks.iter().map(|c| c.text.chars().count()).collect();
        assert_eq!(sizes, vec![709, 709, 709]);
    }

    #[test]
    fn a_file_with_no_definitions_is_module_chunks_and_a_blank_file_is_nothing() {
        let text = "x = 1\ny = 2\n";
        let chunks = chunk_file(&facts(Lang::Python, text, Vec::new()), text);
        assert_eq!(chunks.len(), 1);
        assert_eq!((chunks[0].kind, range(&chunks[0])), (ChunkKind::Module, (1, 2)));
        assert!(chunk_file(&facts(Lang::Python, "", Vec::new()), "").is_empty());
        assert!(chunk_file(&facts(Lang::Python, "\n  \n\n", Vec::new()), "\n  \n\n").is_empty());
    }

    #[test]
    fn crlf_line_ends_give_the_same_chunks_without_carriage_returns() {
        let unix = "import os\n\ndef f(x):\n    return x\n\nVALUE = 1\n";
        let dos = unix.replace('\n', "\r\n");
        let symbols = vec![symbol("f", SymKind::Function, 3, 4, None)];
        let a = chunk_file(&facts(Lang::Python, unix, symbols.clone()), unix);
        let b = chunk_file(&facts(Lang::Python, &dos, symbols), &dos);
        assert_eq!(a, b);
        assert!(b.iter().all(|c| !c.text.contains('\r')));
    }

    #[test]
    fn comments_above_a_declaration_are_module_lines_because_they_are_outside_the_span() {
        let text = "\
use std::fmt;

/// Adds one.
#[inline]
pub fn add(x: u32) -> u32 {
    x + 1
}

// loose note

fn sub() {}
";
        let f = facts(
            Lang::Rust,
            text,
            vec![symbol("add", SymKind::Function, 5, 7, None), symbol("sub", SymKind::Function, 11, 11, None)],
        );
        let chunks = chunk_file(&f, text);
        assert_coverage(&chunks, text);
        let shape: Vec<(ChunkKind, (u32, u32))> = chunks.iter().map(|c| (c.kind, range(c))).collect();
        assert_eq!(
            shape,
            vec![
                (ChunkKind::Module, (1, 4)),
                (ChunkKind::Definition, (5, 7)),
                (ChunkKind::Module, (9, 9)),
                (ChunkKind::Definition, (11, 11)),
            ]
        );
        assert_eq!((chunks[1].def_start, chunks[1].def_end), (5, 7));
        assert_eq!(chunks[0].text, "use std::fmt;\n\n/// Adds one.\n#[inline]");
    }

    #[test]
    fn two_definitions_on_one_line_leave_every_line_in_one_chunk() {
        let text = "function a() { return 1 } function b() { return 2 }\nconst z = 3;\n";
        let f = facts(
            Lang::JavaScript,
            text,
            vec![symbol("a", SymKind::Function, 1, 1, None), symbol("b", SymKind::Function, 1, 1, None)],
        );
        let chunks = chunk_file(&f, text);
        assert_coverage(&chunks, text);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn symbols_outside_the_text_are_ignored() {
        let text = "x = 1\n";
        let f = facts(
            Lang::Python,
            text,
            vec![symbol("ghost", SymKind::Function, 9, 12, None), symbol("zero", SymKind::Function, 0, 0, None)],
        );
        let chunks = chunk_file(&f, text);
        assert_coverage(&chunks, text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].kind, ChunkKind::Module);
    }

    #[test]
    fn a_last_short_piece_joins_the_one_before_it() {
        let mut text = String::from("def f():\n");
        for i in 0..23 {
            text.push_str(&format!("    line_{i:02} = {}\n", "7".repeat(50)));
        }
        text.push_str("    done\n");
        let n = text.lines().count() as u32;
        assert_eq!(n, 25);
        let f = facts(Lang::Python, &text, vec![symbol("f", SymKind::Function, 1, n, None)]);
        let chunks = chunk_file(&f, &text);
        assert_coverage(&chunks, &text);
        let shape: Vec<(u32, u32)> = chunks.iter().map(range).collect();
        assert_eq!(shape, vec![(1, 12), (13, 25)], "the last two lines stand alone: {chunks:?}");
    }
}
