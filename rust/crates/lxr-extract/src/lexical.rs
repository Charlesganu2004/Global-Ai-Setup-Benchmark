//! Every other language, from tokens alone.
//!
//! The text is first masked: comments and string literals are blanked so that braces
//! and keywords inside them are ignored. The masked text is cut into tokens, and each
//! language has a small recognizer for its declaration shapes. Spans come from bracket
//! matching, indentation (Python) or a matching `end` (Ruby).

use crate::common::{fold, without_comments, Parts};
use lxr_core::{Call, Import, Lang, SymKind, Symbol};
use std::collections::HashMap;

/// The facts of `text`, found from tokens alone. Never fails: a file this extractor
/// cannot make sense of simply yields fewer facts.
pub fn parse(text: &str, lang: Lang) -> Parts {
    // A byte order mark is not part of the first token. It leaves the line numbers as they are.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let masked = mask(text, lang);
    let mut ex = Ex::new(text, lang, &masked);
    ex.run();
    ex.into_parts()
}

const NONE: usize = usize::MAX;

/// A search for the closing line of a heredoc that ends at the end of the text finds nothing,
/// and the next `<<word` that is not closed either reads the same lines again. The bytes read by
/// searches that found nothing are held to this many for each byte of the text, plus a flat
/// amount for a small text, so that a file of unclosed `<<word` lines costs a few passes over the
/// file and not one pass for each line. Text that is not made to be slow never comes near it;
/// past it, a `<<word` is read as the operator and not as the start of a heredoc.
const HEREDOC_WASTE_PER_BYTE: usize = 8;
const HEREDOC_WASTE_FLOOR: usize = 64 * 1024;

// ---------------------------------------------------------------------------------
// Masking
// ---------------------------------------------------------------------------------

/// The source with comments and string literals blanked out.
pub(crate) struct Masked {
    /// Same length as the source. Blanked bytes are spaces, line breaks are kept.
    pub code: Vec<u8>,
    /// Byte ranges of string literals, delimiters and prefixes included.
    pub strings: Vec<(usize, usize)>,
    /// Byte ranges of comments, markers included, the line break excluded.
    pub comments: Vec<(usize, usize)>,
}

/// Blanks the comments and string literals of `text` for the language family of `lang`.
pub(crate) fn mask(text: &str, lang: Lang) -> Masked {
    let mut masker = Masker {
        b: text.as_bytes(),
        out: text.as_bytes().to_vec(),
        strings: Vec::new(),
        comments: Vec::new(),
        lang,
        heredocs: HashMap::new(),
        heredoc_next: (NONE, 0),
        heredoc_waste: 0,
        line_end_memo: (NONE, 0),
        brace: 0,
        templates: Vec::new(),
        last: 0,
        last_word: (0, 0),
        // A PHP file is text up to its first open tag. A file with no tag at all is taken for code.
        php_text: lang == Lang::Php && (0..text.len()).any(|i| php_open_len(text.as_bytes(), i).is_some()),
    };
    masker.run();
    Masked { code: masker.out, strings: masker.strings, comments: masker.comments }
}

/// True when the C-family source `text` uses something that only C++ has: a class, a
/// namespace, a template, a scope operator or an access label. For a header file, whose
/// extension does not say whether it is C or C++.
pub(crate) fn uses_cplusplus(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let masked = mask(text, Lang::Cpp);
    let b = &masked.code[..];
    let next = |mut j: usize| {
        while b.get(j).map_or(false, |c| c.is_ascii_whitespace()) {
            j += 1;
        }
        b.get(j).copied().unwrap_or(0)
    };
    let mut i = 0;
    while i < b.len() {
        if b[i] == b':' && b.get(i + 1) == Some(&b':') {
            return true;
        }
        if !is_ident_start(b[i]) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && is_word_byte(b[j]) {
            j += 1;
        }
        let after = next(j);
        let only_cpp = match &b[i..j] {
            b"class" => is_ident_start(after),
            b"namespace" => is_ident_start(after) || after == b'{',
            b"template" => after == b'<',
            b"typename" | b"constexpr" | b"nullptr" | b"noexcept" => true,
            b"public" | b"private" | b"protected" => after == b':',
            _ => false,
        };
        if only_cpp {
            return true;
        }
        i = j;
    }
    false
}

fn is_js(lang: Lang) -> bool {
    matches!(lang, Lang::JavaScript | Lang::Jsx | Lang::TypeScript | Lang::Tsx)
}

fn is_ts(lang: Lang) -> bool {
    matches!(lang, Lang::TypeScript | Lang::Tsx)
}

fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

fn utf8_len(c: u8) -> usize {
    if c < 0xc0 {
        1
    } else if c < 0xe0 {
        2
    } else if c < 0xf0 {
        3
    } else {
        4
    }
}

struct Masker<'a> {
    b: &'a [u8],
    out: Vec<u8>,
    strings: Vec<(usize, usize)>,
    comments: Vec<(usize, usize)>,
    lang: Lang,
    /// Heredoc bodies found on the current line, by their first byte: the byte of the closing line.
    heredocs: HashMap<usize, usize>,
    /// Where the body of the next heredoc on the current command line begins, after the bodies
    /// of the ones before it: (first byte of the lines after the command line, that start).
    heredoc_next: (usize, usize),
    /// Bytes read by searches for a closing line that found none. See `HEREDOC_WASTE_PER_BYTE`.
    heredoc_waste: usize,
    /// The last line end found by `line_end`: (where its search began, the line break).
    line_end_memo: (usize, usize),
    /// Curly brace depth, used to find the end of a `${ ... }` hole in a template string.
    brace: i32,
    templates: Vec<i32>,
    /// The last significant byte seen, `a` for a word and `0` for a number, to tell
    /// a regular expression from a division.
    last: u8,
    last_word: (usize, usize),
    /// PHP only: the scan is outside the `<?php ... ?>` tags, in text that is not code.
    php_text: bool,
}

/// The length of the PHP open tag at `i`, `<?php` followed by white space or `<?=`.
fn php_open_len(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'<') || b.get(i + 1) != Some(&b'?') {
        return None;
    }
    if b.get(i + 2) == Some(&b'=') {
        return Some(3);
    }
    let word = b.get(i + 2..i + 5)?;
    let ends = b.get(i + 5).map_or(true, |c| c.is_ascii_whitespace());
    (word.eq_ignore_ascii_case(b"php") && ends).then_some(5)
}

impl<'a> Masker<'a> {
    fn at(&self, i: usize) -> u8 {
        self.b.get(i).copied().unwrap_or(0)
    }

    fn blank(&mut self, from: usize, to: usize) {
        let to = to.min(self.out.len());
        for k in from..to {
            let c = self.out[k];
            if c != b'\n' && c != b'\r' {
                self.out[k] = b' ';
            }
        }
    }

    fn string(&mut self, from: usize, to: usize) {
        let to = to.min(self.b.len());
        self.blank(from, to);
        self.strings.push((from, to));
        self.last = b'"';
    }

    fn comment(&mut self, from: usize, to: usize) {
        self.blank(from, to);
        self.comments.push((from, to));
    }

    fn slash_comments(&self) -> bool {
        !matches!(self.lang, Lang::Python | Lang::Ruby | Lang::Shell)
    }

    fn triple_quotes(&self) -> bool {
        matches!(self.lang, Lang::Python | Lang::Java | Lang::Kotlin | Lang::Swift | Lang::CSharp)
    }

    fn multiline_strings(&self) -> bool {
        matches!(self.lang, Lang::Rust | Lang::Ruby | Lang::Shell | Lang::Php)
    }

    fn run(&mut self) {
        let n = self.b.len();
        let mut i = 0;
        while i < n {
            if self.php_text {
                i = self.php_skip_text(i);
                continue;
            }
            let c = self.b[i];
            let d = self.at(i + 1);
            i = match c {
                b'?' if self.lang == Lang::Php && d == b'>' => {
                    self.blank(i, i + 2);
                    self.php_text = true;
                    i + 2
                }
                b'\n' => self.newline(i),
                b' ' | b'\t' | b'\r' => i + 1,
                b'/' if self.slash_comments() && d == b'/' => self.line_comment(i),
                b'/' if self.slash_comments() && d == b'*' => self.block_comment(i),
                b'/' => self.slash(i),
                b'#' => self.hash(i),
                b'"' => self.double(i),
                b'\'' => self.single(i),
                b'`' => self.backtick(i),
                b'@' => self.at_sign(i),
                b'$' => self.dollar(i),
                b'<' if d == b'<' => self.heredoc(i),
                b'%' => self.percent(i),
                b'=' => self.equals(i),
                b'{' => {
                    self.brace += 1;
                    self.last = c;
                    i + 1
                }
                b'}' => self.close_brace(i),
                b'0'..=b'9' => self.number(i),
                _ if is_ident_start(c) => self.word(i),
                _ => {
                    self.last = c;
                    i + 1
                }
            };
        }
    }

    /// Blanks the text of a PHP file from `i` to the next open tag, and the tag itself, and
    /// returns where the code begins.
    fn php_skip_text(&mut self, i: usize) -> usize {
        let n = self.b.len();
        let mut j = i;
        while j < n {
            if let Some(tag) = php_open_len(self.b, j) {
                self.blank(i, j + tag);
                self.php_text = false;
                return j + tag;
            }
            j += 1;
        }
        self.blank(i, n);
        n
    }

    fn newline(&mut self, i: usize) -> usize {
        if let Some(to) = self.heredocs.remove(&(i + 1)) {
            self.blank(i + 1, to);
            return to.max(i + 1);
        }
        i + 1
    }

    fn line_comment(&mut self, i: usize) -> usize {
        let n = self.b.len();
        let php = self.lang == Lang::Php;
        let mut e = i;
        // In PHP a line comment also ends at a closing tag.
        while e < n && self.b[e] != b'\n' && !(php && self.b[e] == b'?' && self.at(e + 1) == b'>') {
            e += 1;
        }
        let end = if e > i && self.b[e - 1] == b'\r' { e - 1 } else { e };
        let end = end.max(i + 1).min(n);
        self.comment(i, end);
        end
    }

    fn block_comment(&mut self, i: usize) -> usize {
        let n = self.b.len();
        let nested = matches!(self.lang, Lang::Rust | Lang::Kotlin | Lang::Swift);
        let mut depth = 1;
        let mut j = i + 2;
        while j < n {
            if self.b[j] == b'*' && self.at(j + 1) == b'/' {
                depth -= 1;
                j += 2;
                if depth == 0 {
                    break;
                }
            } else if nested && self.b[j] == b'/' && self.at(j + 1) == b'*' {
                depth += 1;
                j += 2;
            } else {
                j += 1;
            }
        }
        let end = j.min(n);
        self.comment(i, end);
        end
    }

    fn hash(&mut self, i: usize) -> usize {
        match self.lang {
            Lang::Python | Lang::Ruby => self.line_comment(i),
            Lang::Shell => {
                let word_start = i == 0
                    || matches!(self.b[i - 1], b' ' | b'\t' | b'\n' | b'\r' | b';' | b'&' | b'|' | b'(' | b')');
                if word_start {
                    self.line_comment(i)
                } else {
                    self.last = b'#';
                    i + 1
                }
            }
            Lang::Php => {
                if self.at(i + 1) == b'[' {
                    self.last = b'#';
                    i + 1
                } else {
                    self.line_comment(i)
                }
            }
            Lang::Swift => self.swift_raw(i),
            _ => {
                self.last = b'#';
                i + 1
            }
        }
    }

    /// A Swift raw string such as `#"..."#`. Anything else starting with `#` is left alone.
    fn swift_raw(&mut self, i: usize) -> usize {
        let mut k = i;
        while self.at(k) == b'#' {
            k += 1;
        }
        let hashes = k - i;
        if self.at(k) != b'"' {
            // No hash in the run can open a raw string either, so the whole run is passed over;
            // starting again one byte on would read the rest of it again, once for each hash.
            self.last = b'#';
            return k;
        }
        let n = self.b.len();
        let mut j = k + 1;
        while j < n {
            if self.b[j] == b'"' && (0..hashes).all(|h| self.at(j + 1 + h) == b'#') {
                j += 1 + hashes;
                self.string(i, j);
                return j;
            }
            j += 1;
        }
        self.string(i, n);
        n
    }

    fn double(&mut self, i: usize) -> usize {
        if self.triple_quotes() && self.at(i + 1) == b'"' && self.at(i + 2) == b'"' {
            return self.triple(i, i, b'"');
        }
        self.quoted(i, i, b'"')
    }

    /// A string whose opening quote is at `q` and whose text (prefix included) starts at `start`.
    fn quoted(&mut self, start: usize, q: usize, quote: u8) -> usize {
        let n = self.b.len();
        let multi = self.multiline_strings();
        let raw = self.lang == Lang::Shell && quote == b'\'';
        let mut j = q + 1;
        while j < n {
            let c = self.b[j];
            if c == b'\\' && !raw {
                j += 2;
                continue;
            }
            if c == quote {
                j += 1;
                self.string(start, j);
                return j;
            }
            if c == b'\n' && !multi {
                break;
            }
            j += 1;
        }
        let end = j.min(n);
        self.string(start, end);
        end
    }

    /// A triple-quoted string (a text block, a raw string) whose first quote is at `q`.
    fn triple(&mut self, start: usize, q: usize, quote: u8) -> usize {
        let n = self.b.len();
        let escapes = !matches!(self.lang, Lang::Kotlin | Lang::CSharp);
        let mut j = q + 3;
        while j < n {
            let c = self.b[j];
            if c == b'\\' && escapes {
                j += 2;
                continue;
            }
            if c == quote && self.at(j + 1) == quote && self.at(j + 2) == quote {
                j += 3;
                self.string(start, j);
                return j;
            }
            j += 1;
        }
        self.string(start, n);
        n
    }

    /// A C# verbatim string: `""` stands for a quote and a backslash is literal.
    fn verbatim(&mut self, start: usize, q: usize) -> usize {
        let n = self.b.len();
        let mut j = q + 1;
        while j < n {
            if self.b[j] == b'"' {
                if self.at(j + 1) == b'"' {
                    j += 2;
                    continue;
                }
                j += 1;
                self.string(start, j);
                return j;
            }
            j += 1;
        }
        self.string(start, n);
        n
    }

    fn single(&mut self, i: usize) -> usize {
        match self.lang {
            Lang::Python => {
                if self.at(i + 1) == b'\'' && self.at(i + 2) == b'\'' {
                    self.triple(i, i, b'\'')
                } else {
                    self.quoted(i, i, b'\'')
                }
            }
            l if is_js(l) => self.quoted(i, i, b'\''),
            Lang::Php | Lang::Ruby | Lang::Shell => self.quoted(i, i, b'\''),
            Lang::Rust => self.rust_tick(i),
            Lang::Swift => {
                self.last = b'\'';
                i + 1
            }
            _ => self.char_literal(i, i),
        }
    }

    /// A character literal starting at `start` with its opening tick at `q`. A tick that
    /// does not open one (a lone apostrophe) is passed over.
    fn char_literal(&mut self, start: usize, q: usize) -> usize {
        let n = self.b.len();
        let d = self.at(q + 1);
        let end = if d == b'\\' {
            let lim = (q + 14).min(n);
            let mut j = q + 3;
            while j < lim && self.b[j] != b'\'' && self.b[j] != b'\n' {
                j += 1;
            }
            if j < lim && self.b[j] == b'\'' {
                Some(j + 1)
            } else {
                None
            }
        } else if d != 0 && d != b'\n' && d != b'\'' {
            let j = q + 1 + utf8_len(d);
            if self.at(j) == b'\'' {
                Some(j + 1)
            } else {
                None
            }
        } else {
            None
        };
        match end {
            Some(e) => {
                self.string(start, e);
                e
            }
            None => {
                self.last = b'\'';
                q + 1
            }
        }
    }

    /// A tick in Rust: a character literal, or the start of a lifetime.
    fn rust_tick(&mut self, i: usize) -> usize {
        self.char_literal(i, i)
    }

    fn backtick(&mut self, i: usize) -> usize {
        let n = self.b.len();
        match self.lang {
            Lang::Go => {
                let mut j = i + 1;
                while j < n && self.b[j] != b'`' {
                    j += 1;
                }
                let end = (j + 1).min(n);
                self.string(i, end);
                end
            }
            l if is_js(l) => self.template(i, i + 1),
            _ => self.quoted(i, i, b'`'),
        }
    }

    /// The text of a template string from `from`, its first piece starting at `start`.
    /// Stops at the closing backtick or at the `${` that opens an expression.
    fn template(&mut self, start: usize, from: usize) -> usize {
        let n = self.b.len();
        let mut j = from;
        while j < n {
            match self.b[j] {
                b'\\' => j += 2,
                b'`' => {
                    j += 1;
                    self.string(start, j);
                    return j;
                }
                b'$' if self.at(j + 1) == b'{' => {
                    j += 2;
                    self.string(start, j);
                    self.brace += 1;
                    self.templates.push(self.brace);
                    return j;
                }
                _ => j += 1,
            }
        }
        self.string(start, n);
        n
    }

    fn close_brace(&mut self, i: usize) -> usize {
        if self.templates.last() == Some(&self.brace) {
            self.templates.pop();
            self.brace -= 1;
            return self.template(i, i + 1);
        }
        self.brace -= 1;
        self.last = b'}';
        i + 1
    }

    fn at_sign(&mut self, i: usize) -> usize {
        let d = self.at(i + 1);
        match self.lang {
            Lang::CSharp if d == b'"' => self.verbatim(i, i + 1),
            Lang::CSharp if d == b'$' && self.at(i + 2) == b'"' => self.verbatim(i, i + 2),
            Lang::Ruby => {
                let mut j = i + 1;
                if self.at(j) == b'@' {
                    j += 1;
                }
                while is_word_byte(self.at(j)) {
                    j += 1;
                }
                self.last = b'a';
                j
            }
            _ => {
                self.last = b'@';
                i + 1
            }
        }
    }

    fn dollar(&mut self, i: usize) -> usize {
        let d = self.at(i + 1);
        match self.lang {
            Lang::CSharp if d == b'"' => {
                if self.at(i + 2) == b'"' && self.at(i + 3) == b'"' {
                    self.triple(i, i + 1, b'"')
                } else {
                    self.quoted(i, i + 1, b'"')
                }
            }
            Lang::CSharp if d == b'@' && self.at(i + 2) == b'"' => self.verbatim(i, i + 2),
            Lang::Shell if d == b'\'' || d == b'"' => self.quoted(i, i + 1, d),
            // Ruby's special globals, such as `$"`, `` $` `` and `$/`, are not quotes.
            Lang::Ruby if matches!(d, b'"' | b'\'' | b'`' | b'/' | b'\\' | b';' | b',' | b'.' | b'<' | b'>' | b'_' | b'~' | b'*' | b'$' | b'?' | b'!' | b'@' | b'&' | b':') => {
                // Keep the global a single word: `$"` becomes `$_`.
                self.out[i + 1] = b'_';
                self.last = b'a';
                i + 2
            }
            _ => self.word(i),
        }
    }

    fn word(&mut self, i: usize) -> usize {
        let n = self.b.len();
        let mut j = i;
        while j < n && (is_word_byte(self.b[j]) || (j == i && self.b[j] == b'$')) {
            j += 1;
        }
        if j == i {
            j = i + 1;
        }
        let q = self.at(j);
        let candidate = q == b'"' || q == b'\'' || (q == b'#' && self.lang == Lang::Rust);
        if candidate {
            if let Some(end) = self.prefixed_string(i, j, q) {
                return end;
            }
        }
        self.last = b'a';
        self.last_word = (i, j);
        j
    }

    /// A string literal with a letter prefix: `f"..."`, `r#"..."#`, `R"(...)"`, `b'x'`.
    fn prefixed_string(&mut self, i: usize, j: usize, q: u8) -> Option<usize> {
        let word = self.b[i..j].to_vec();
        let w = word.as_slice();
        match self.lang {
            Lang::Python => {
                let letters = !w.is_empty()
                    && w.len() <= 2
                    && w.iter().all(|c| matches!(c, b'r' | b'R' | b'b' | b'B' | b'u' | b'U' | b'f' | b'F'));
                if letters && q != b'#' {
                    if self.at(j + 1) == q && self.at(j + 2) == q {
                        return Some(self.triple(i, j, q));
                    }
                    return Some(self.quoted(i, j, q));
                }
                None
            }
            Lang::Rust => match w {
                b"r" | b"br" | b"cr" if q == b'"' || q == b'#' => self.rust_raw(i, j),
                b"b" | b"c" if q == b'"' => Some(self.quoted(i, j, b'"')),
                b"b" if q == b'\'' => Some(self.char_literal(i, j)),
                _ => None,
            },
            Lang::C | Lang::Cpp => {
                if q == b'"' && matches!(w, b"R" | b"LR" | b"uR" | b"UR" | b"u8R") {
                    return self.cpp_raw(i, j);
                }
                if matches!(w, b"L" | b"u" | b"U" | b"u8") {
                    if q == b'"' {
                        return Some(self.quoted(i, j, b'"'));
                    }
                    if q == b'\'' {
                        return Some(self.char_literal(i, j));
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn rust_raw(&mut self, start: usize, j: usize) -> Option<usize> {
        let n = self.b.len();
        let mut k = j;
        while self.at(k) == b'#' {
            k += 1;
        }
        if self.at(k) != b'"' {
            return None;
        }
        let hashes = k - j;
        let mut p = k + 1;
        while p < n {
            if self.b[p] == b'"' && (0..hashes).all(|h| self.at(p + 1 + h) == b'#') {
                let end = p + 1 + hashes;
                self.string(start, end);
                return Some(end);
            }
            p += 1;
        }
        self.string(start, n);
        Some(n)
    }

    fn cpp_raw(&mut self, start: usize, quote: usize) -> Option<usize> {
        let n = self.b.len();
        let mut k = quote + 1;
        while k < n && k - quote <= 17 && !matches!(self.b[k], b'(' | b')' | b'\\' | b' ' | b'"' | b'\n') {
            k += 1;
        }
        if self.at(k) != b'(' {
            return None;
        }
        let mut closer = vec![b')'];
        closer.extend_from_slice(&self.b[quote + 1..k]);
        closer.push(b'"');
        let mut p = k + 1;
        while p + closer.len() <= n {
            if self.b[p..p + closer.len()] == closer[..] {
                let end = p + closer.len();
                self.string(start, end);
                return Some(end);
            }
            p += 1;
        }
        self.string(start, n);
        Some(n)
    }

    fn number(&mut self, i: usize) -> usize {
        let n = self.b.len();
        let mut j = i + 1;
        while j < n {
            let c = self.b[j];
            let tick = c == b'\'' && matches!(self.lang, Lang::C | Lang::Cpp) && self.at(j + 1).is_ascii_alphanumeric();
            let exp = (c == b'+' || c == b'-')
                && matches!(self.b[j - 1], b'e' | b'E')
                && self.at(j + 1).is_ascii_digit()
                && !(self.b[i] == b'0' && matches!(self.at(i + 1), b'x' | b'X'));
            if c.is_ascii_alphanumeric() || c == b'_' || tick || exp || (c == b'.' && self.at(j + 1).is_ascii_digit()) {
                j += 1;
            } else {
                break;
            }
        }
        self.last = b'0';
        j
    }

    fn equals(&mut self, i: usize) -> usize {
        let line_start = i == 0 || self.b[i - 1] == b'\n';
        if self.lang == Lang::Ruby && line_start && self.b[i..].starts_with(b"=begin") {
            let n = self.b.len();
            let mut p = i;
            while p < n {
                while p < n && self.b[p] != b'\n' {
                    p += 1;
                }
                if p >= n {
                    break;
                }
                p += 1;
                if self.b[p..].starts_with(b"=end") {
                    while p < n && self.b[p] != b'\n' {
                        p += 1;
                    }
                    break;
                }
            }
            let end = p.min(n);
            self.comment(i, end);
            return end;
        }
        self.last = b'=';
        i + 1
    }

    fn regex_allowed(&self) -> bool {
        match self.last {
            0 | b'(' | b',' | b'=' | b':' | b'[' | b'!' | b'&' | b'|' | b'?' | b'{' | b';' | b'+' | b'-' | b'*' | b'%'
            | b'<' | b'>' | b'~' | b'^' => true,
            b'a' => {
                let w = &self.b[self.last_word.0..self.last_word.1];
                matches!(
                    w,
                    b"return" | b"typeof" | b"instanceof" | b"in" | b"of" | b"new" | b"delete" | b"void" | b"throw"
                        | b"case" | b"do" | b"else" | b"yield" | b"await" | b"if" | b"unless" | b"while" | b"until"
                        | b"and" | b"or" | b"not" | b"when" | b"then" | b"elsif"
                )
            }
            _ => false,
        }
    }

    /// A `/` that is not a comment: a regular expression literal in JavaScript or Ruby
    /// when it can start one, otherwise a division sign.
    fn slash(&mut self, i: usize) -> usize {
        // In Ruby `:/` is a symbol and `?/` a character, neither starts a regular expression.
        let ruby_literal = self.lang == Lang::Ruby && i > 0 && matches!(self.b[i - 1], b':' | b'?');
        if (is_js(self.lang) || self.lang == Lang::Ruby) && self.regex_allowed() && !ruby_literal {
            let n = self.b.len();
            let mut j = i + 1;
            let mut class = false;
            let mut lines = 0;
            while j < n {
                match self.b[j] {
                    // A Ruby regular expression with the x flag may run over several lines.
                    b'\n' if self.lang == Lang::Ruby && lines < 80 => lines += 1,
                    b'\n' => break,
                    b'\\' => j += 1,
                    b'[' => class = true,
                    b']' => class = false,
                    b'/' if !class => {
                        self.string(i, j + 1);
                        return j + 1;
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        self.last = b'/';
        i + 1
    }

    /// A Ruby percent literal such as `%w[a b]` or `%(text)`.
    fn percent(&mut self, i: usize) -> usize {
        if self.lang != Lang::Ruby {
            self.last = b'%';
            return i + 1;
        }
        let d = self.at(i + 1);
        let expr_start = matches!(self.last, 0 | b'=' | b'(' | b',' | b'[' | b'{' | b'|' | b'&' | b'!' | b':' | b';' | b'?');
        let after_space = i == 0 || matches!(self.b[i - 1], b' ' | b'\t' | b'\n');
        let open_at = if matches!(d, b'q' | b'Q' | b'w' | b'W' | b'i' | b'I' | b'r' | b's' | b'x')
            && matches!(self.at(i + 2), b'(' | b'[' | b'{' | b'<' | b'|' | b'!' | b'/')
            && (expr_start || after_space)
        {
            i + 2
        } else if matches!(d, b'(' | b'[' | b'{' | b'<' | b'|' | b'!' | b'/') && expr_start {
            i + 1
        } else {
            self.last = b'%';
            return i + 1;
        };
        let open = self.b[open_at];
        let close = match open {
            b'(' => b')',
            b'[' => b']',
            b'{' => b'}',
            b'<' => b'>',
            c => c,
        };
        let n = self.b.len();
        let mut depth = 1;
        let mut j = open_at + 1;
        while j < n {
            let c = self.b[j];
            if c == b'\\' {
                j += 2;
                continue;
            }
            if c == close && open != close {
                depth -= 1;
            } else if c == open && open != close {
                depth += 1;
            } else if c == close {
                depth = 0;
            }
            j += 1;
            if depth == 0 {
                break;
            }
        }
        let end = j.min(n);
        self.string(i, end);
        end
    }

    /// A heredoc: the rest of the line is code, the lines after it up to the closing
    /// delimiter are blanked when the newline is reached.
    fn heredoc(&mut self, i: usize) -> usize {
        let lang = self.lang;
        if !matches!(lang, Lang::Shell | Lang::Ruby | Lang::Php) {
            self.last = b'<';
            return i + 2;
        }
        let n = self.b.len();
        let mut j = i + 2;
        let mut indented = false;
        if lang == Lang::Php {
            if self.at(j) != b'<' {
                return i + 2;
            }
            j += 1;
            indented = true;
        } else {
            if self.at(j) == b'<' {
                return i + 3;
            }
            if self.at(j) == b'-' || self.at(j) == b'~' {
                indented = true;
                j += 1;
            }
            if lang == Lang::Shell {
                while matches!(self.at(j), b' ' | b'\t') {
                    j += 1;
                }
            }
        }
        let quote = if matches!(self.at(j), b'\'' | b'"' | b'`') {
            let q = self.at(j);
            j += 1;
            Some(q)
        } else {
            None
        };
        let word_start = j;
        match quote {
            // A quoted delimiter may hold any character, such as `<<-"end;"`.
            Some(q) => {
                while j < n && self.b[j] != q && self.b[j] != b'\n' {
                    j += 1;
                }
                if j == word_start || self.at(j) != q {
                    return i + 2;
                }
            }
            None => {
                while j < n && self.b[j] < 0x80 && is_word_byte(self.b[j]) {
                    j += 1;
                }
                if j == word_start || self.b[word_start].is_ascii_digit() {
                    return i + 2;
                }
            }
        }
        let delim = self.b[word_start..j].to_vec();
        if quote.is_some() {
            j += 1;
        }
        if lang == Lang::Ruby && !indented {
            let upper = delim.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_');
            if !(quote.is_some() || (upper && matches!(self.last, b'=' | b'(' | b','))) {
                return i + 2;
            }
        }
        let eol = self.line_end(j);
        // Heredocs on one command line read their bodies one after the other: a second one
        // starts where the first one's closing line ends.
        let lines_after = (eol + 1).min(n);
        let body_start = if self.heredoc_next.0 == lines_after { self.heredoc_next.1 } else { lines_after };
        if self.heredoc_waste > HEREDOC_WASTE_PER_BYTE * n + HEREDOC_WASTE_FLOOR {
            return i + 2;
        }
        let mut p = body_start;
        while p < n {
            let mut e = p;
            while e < n && self.b[e] != b'\n' {
                e += 1;
            }
            let mut line = &self.b[p..e];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            let line = if indented {
                let skip = line.iter().take_while(|c| **c == b' ' || **c == b'\t').count();
                &line[skip..]
            } else {
                line
            };
            let hit = if lang == Lang::Php {
                line.starts_with(&delim) && !line.get(delim.len()).map_or(false, |c| is_word_byte(*c))
            } else {
                line == &delim[..]
            };
            if hit {
                self.heredocs.insert(body_start, p);
                self.heredoc_next = (lines_after, (e + 1).min(n));
                return j;
            }
            p = e + 1;
        }
        self.heredoc_waste += n - body_start;
        i + 2
    }

    /// The offset of the line break that ends the line holding `from`, or the end of the text.
    /// The search is remembered: a line with many candidates for a heredoc is read to its end once.
    fn line_end(&mut self, from: usize) -> usize {
        let (began, end) = self.line_end_memo;
        if from >= began && from <= end {
            return end;
        }
        let n = self.b.len();
        let mut e = from;
        while e < n && self.b[e] != b'\n' {
            e += 1;
        }
        self.line_end_memo = (from, e);
        e
    }
}

// ---------------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum K {
    Id,
    Num,
    Str,
    P,
}

#[derive(Clone, Copy, Debug)]
struct Tok {
    k: K,
    s: usize,
    e: usize,
    line: u32,
    end_line: u32,
    /// The token follows a backslash line continuation.
    cont: bool,
}

struct Lines {
    starts: Vec<usize>,
    len: usize,
}

impl Lines {
    fn new(text: &str) -> Lines {
        let mut starts = vec![0];
        starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        Lines { starts, len: text.len() }
    }

    fn line_of(&self, off: usize) -> u32 {
        match self.starts.binary_search(&off) {
            Ok(i) => i as u32 + 1,
            Err(i) => i as u32,
        }
    }

    fn start_of(&self, line: u32) -> usize {
        self.starts.get(line.saturating_sub(1) as usize).copied().unwrap_or(self.len)
    }

    fn end_of(&self, line: u32) -> usize {
        self.starts.get(line as usize).copied().unwrap_or(self.len)
    }
}

fn line_start(b: &[u8], i: usize) -> bool {
    b[..i].iter().rev().take_while(|c| **c != b'\n').all(|c| matches!(c, b' ' | b'\t' | b'\r'))
}

fn punct_len(b: &[u8], i: usize) -> usize {
    let d = b.get(i + 1).copied().unwrap_or(0);
    match (b[i], d) {
        (b':', b':') | (b'-', b'>') | (b'=', b'>') | (b'=', b'=') | (b'!', b'=') | (b'<', b'=') | (b'>', b'=')
        | (b'&', b'&') | (b'|', b'|') | (b'+', b'+') | (b'-', b'-') | (b'+', b'=') | (b'-', b'=') | (b'*', b'=')
        | (b'/', b'=') | (b'%', b'=') | (b'&', b'=') | (b'|', b'=') | (b'^', b'=') | (b':', b'=') | (b'?', b'?')
        | (b'.', b'.') => 2,
        (b'?', b'.') if !b.get(i + 2).map_or(false, |c| c.is_ascii_digit()) => 2,
        _ => 1,
    }
}

/// The preprocessor line starting at `i`. Records the file an `#include` names.
fn directive(text: &str, b: &[u8], i: usize, lang: Lang, lines: &Lines, imports: &mut Vec<Import>) -> usize {
    let n = b.len();
    let mut e = i;
    loop {
        while e < n && b[e] != b'\n' {
            e += 1;
        }
        let mut k = e;
        if k > i && b[k - 1] == b'\r' {
            k -= 1;
        }
        if e < n && k > i && b[k - 1] == b'\\' {
            e += 1;
            continue;
        }
        break;
    }
    if matches!(lang, Lang::C | Lang::Cpp) {
        let line = text[i..e].trim_start_matches('#').trim_start();
        if let Some(rest) = line.strip_prefix("include") {
            let rest = rest.trim_start();
            let module = if let Some(r) = rest.strip_prefix('<') {
                r.find('>').map(|c| &r[..c])
            } else if let Some(r) = rest.strip_prefix('"') {
                r.find('"').map(|c| &r[..c])
            } else {
                None
            };
            if let Some(module) = module {
                let alias = module.rsplit('/').next().unwrap_or(module);
                imports.push(Import {
                    module: module.to_string(),
                    name: String::new(),
                    alias: alias.to_string(),
                    level: 0,
                    line: lines.line_of(i),
                });
            }
        }
    }
    e
}

/// A conditional-compilation group (`#if` to `#endif`) that the tokenizer is inside of.
struct Cond {
    /// Index of the first token of each branch met so far.
    branches: Vec<usize>,
    /// How many more brackets each finished branch opens than it closes.
    nets: Vec<i32>,
    /// The same for the branch being read, settled at the next directive.
    net: i32,
}

/// Opens minus closes among the brackets of `toks`.
fn net_brackets(toks: &[Tok], b: &[u8]) -> i32 {
    toks.iter()
        .filter(|t| t.k == K::P && t.e - t.s == 1)
        .map(|t| match b[t.s] {
            b'(' | b'[' | b'{' => 1,
            b')' | b']' | b'}' => -1,
            _ => 0,
        })
        .sum()
}

/// Keeps the brackets of a file with alternative branches (`#if`, `#else`, `#endif`) in step.
///
/// The branches of a group are alternatives, but bracket matching reads them one after the
/// other. When the branches of a group do not each close what they open, as in two versions
/// of a function header that share one body, only the first branch is kept, and the tokens of
/// the others are dropped. Branches that are complete in themselves are all kept.
/// `mark` is the index of the first token that no directive has accounted for yet.
fn conditional(directive: &str, toks: &mut Vec<Tok>, b: &[u8], conds: &mut Vec<Cond>, mark: &mut usize) {
    let word = directive.trim_start_matches('#').trim_start();
    let word = word.split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or("");
    let opens = matches!(word, "if" | "ifdef" | "ifndef");
    let turns = matches!(word, "elif" | "elifdef" | "elifndef" | "elseif" | "else");
    if !opens && !turns && word != "endif" {
        return;
    }
    if let Some(group) = conds.last_mut() {
        group.net += net_brackets(&toks[(*mark).min(toks.len())..], b);
    }
    if opens {
        conds.push(Cond { branches: vec![toks.len()], nets: Vec::new(), net: 0 });
    } else if turns {
        if let Some(group) = conds.last_mut() {
            group.nets.push(group.net);
            group.net = 0;
            group.branches.push(toks.len());
        }
    } else if let Some(mut group) = conds.pop() {
        group.nets.push(group.net);
        let alternatives = group.nets.len() > 1 && group.nets.iter().any(|&net| net != 0);
        let kept = if alternatives {
            toks.truncate(group.branches[1]);
            group.nets[0]
        } else {
            group.nets.iter().sum()
        };
        if let Some(parent) = conds.last_mut() {
            parent.net += kept;
        }
    }
    *mark = toks.len();
}

fn tokenize(text: &str, m: &Masked, lang: Lang, lines: &Lines, imports: &mut Vec<Import>) -> Vec<Tok> {
    let b = &m.code[..];
    let n = b.len();
    let mut toks: Vec<Tok> = Vec::with_capacity(n / 5 + 8);
    let mut conds: Vec<Cond> = Vec::new();
    let mut mark = 0;
    let mut si = 0;
    let mut i = 0;
    let mut cont = false;
    let directives = matches!(lang, Lang::C | Lang::Cpp | Lang::CSharp | Lang::Swift);
    let shell = lang == Lang::Shell;
    let ruby = lang == Lang::Ruby;
    while i < n {
        if si < m.strings.len() && i >= m.strings[si].0 {
            let (s, e) = m.strings[si];
            si += 1;
            if e > s {
                toks.push(Tok { k: K::Str, s, e, line: lines.line_of(s), end_line: lines.line_of(e - 1), cont });
                cont = false;
            }
            i = i.max(e);
            continue;
        }
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' | 0x0c => i += 1,
            b'\\' if b.get(i + 1) == Some(&b'\n') || (b.get(i + 1) == Some(&b'\r') && b.get(i + 2) == Some(&b'\n')) => {
                cont = true;
                i += 1;
            }
            b'#' if directives && line_start(b, i) => {
                let from = i;
                i = directive(text, b, i, lang, lines, imports);
                conditional(&text[from..i], &mut toks, b, &mut conds, &mut mark);
                while si < m.strings.len() && m.strings[si].0 < i {
                    si += 1;
                }
            }
            _ => {
                let start = i;
                let next = b.get(i + 1).copied().unwrap_or(0);
                let (k, end) = if is_ident_start(c) || (ruby && c == b'@' && (next.is_ascii_alphabetic() || next == b'_' || next == b'@')) {
                    let mut j = i + 1;
                    if ruby && c == b'@' && next == b'@' {
                        j += 1;
                    }
                    loop {
                        match b.get(j) {
                            Some(&x) if is_word_byte(x) || x == b'$' => j += 1,
                            Some(&x)
                                if shell
                                    && matches!(x, b'-' | b'.' | b':' | b'+')
                                    && b.get(j + 1).map_or(false, |y| y.is_ascii_alphanumeric() || *y == b'_') =>
                            {
                                j += 1
                            }
                            _ => break,
                        }
                    }
                    if ruby {
                        if let Some(&x) = b.get(j) {
                            let after = b.get(j + 1).copied().unwrap_or(0);
                            let suffix = (x == b'?' || x == b'!')
                                && after != b'='
                                && !(x == b'?' && (is_word_byte(after) || after == b':'));
                            if suffix {
                                j += 1;
                            }
                        }
                    }
                    (K::Id, j)
                } else if c.is_ascii_digit() {
                    let mut j = i + 1;
                    while j < n {
                        let x = b[j];
                        if x.is_ascii_alphanumeric() || x == b'_' || (x == b'.' && b.get(j + 1).map_or(false, |y| y.is_ascii_digit())) {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    (K::Num, j)
                } else {
                    (K::P, i + punct_len(b, i))
                };
                toks.push(Tok { k, s: start, e: end, line: lines.line_of(start), end_line: lines.line_of(end - 1), cont });
                cont = false;
                i = end;
            }
        }
    }
    toks
}

fn match_brackets(toks: &[Tok], bytes: &[u8]) -> Vec<usize> {
    let mut mate = vec![NONE; toks.len()];
    let mut stack: Vec<(usize, u8)> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.k != K::P || t.e - t.s != 1 {
            continue;
        }
        let c = bytes[t.s];
        match c {
            b'(' | b'[' | b'{' => stack.push((i, c)),
            b')' | b']' | b'}' => {
                let want = match c {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                // Look back a bounded way, so that a file with thousands of stray closers
                // cannot make this quadratic.
                let from = stack.len().saturating_sub(512);
                if let Some(pos) = stack[from..].iter().rposition(|&(_, o)| o == want).map(|p| p + from) {
                    let open = stack[pos].0;
                    stack.truncate(pos);
                    mate[open] = i;
                    mate[i] = open;
                }
            }
            _ => {}
        }
    }
    mate
}

// ---------------------------------------------------------------------------------
// The extractor
// ---------------------------------------------------------------------------------

struct Sym {
    name: String,
    qual: String,
    kind: SymKind,
    start_line: u32,
    end_line: u32,
    sig: String,
    doc: String,
    parent: Option<usize>,
    bases: Vec<String>,
    /// First and last token inside the body, for finding the caller of a call.
    body: Option<(usize, usize)>,
}

/// A symbol about to be added.
struct New {
    name: String,
    kind: SymKind,
    /// First token of the declaration, modifiers included.
    first: usize,
    /// Byte offset where the signature stops.
    sig_end: usize,
    end_line: u32,
    body: Option<(usize, usize)>,
    bases: Vec<String>,
    qual: String,
    parent: Option<usize>,
}

/// A run of comment lines (or one block comment) that stands on its own lines.
struct Group {
    last: u32,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FKind {
    Func,
    Type(SymKind),
    Ns,
}

/// A region between braces that the scan is inside of.
struct Frame {
    /// Token index of the closing brace.
    end: usize,
    sym: Option<usize>,
    /// The qualified name that members of this region get as prefix.
    prefix: String,
    kind: FKind,
}

/// What follows a declaration header.
enum Body {
    Braces(usize),
    Arrow(usize),
    Eq(usize),
    Semi(usize),
    Nothing,
}

/// Longest qualified name kept, in bytes. Braces that are never closed nest one name per line,
/// and every name would carry the whole chain before it.
const MAX_QUAL: usize = 512;

/// Most regions a scan is inside of at once. Real code does not nest this deep; a file of
/// unclosed braces would otherwise keep a frame for every line.
const MAX_FRAMES: usize = 128;

/// Deepest group of braces in a Rust `use` tree that is read.
const MAX_USE_DEPTH: usize = 64;

/// `name` under `prefix`. A result longer than `MAX_QUAL` loses its outermost names.
fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        return name.to_string();
    }
    let mut qual = format!("{prefix}.{name}");
    if qual.len() > MAX_QUAL {
        let mut cut = qual.len() - MAX_QUAL;
        while !qual.is_char_boundary(cut) {
            cut += 1;
        }
        // Start at a name, not in the middle of one, when there is a name boundary to start at.
        let from = qual[cut..].find('.').map(|dot| cut + dot + 1).filter(|&from| from < qual.len()).unwrap_or(cut);
        qual.drain(..from);
    }
    qual
}

struct Ex<'a> {
    text: &'a str,
    lang: Lang,
    m: &'a Masked,
    lines: Lines,
    toks: Vec<Tok>,
    mate: Vec<usize>,
    skip: Vec<bool>,
    syms: Vec<Sym>,
    imports: Vec<Import>,
    frames: Vec<Frame>,
    groups: Vec<Group>,
}

impl<'a> Ex<'a> {
    fn new(text: &'a str, lang: Lang, m: &'a Masked) -> Ex<'a> {
        let lines = Lines::new(text);
        let mut imports = Vec::new();
        let toks = tokenize(text, m, lang, &lines, &mut imports);
        let mate = match_brackets(&toks, text.as_bytes());
        let skip = vec![false; toks.len()];
        let mut ex = Ex { text, lang, m, lines, toks, mate, skip, syms: Vec::new(), imports, frames: Vec::new(), groups: Vec::new() };
        ex.groups = ex.build_groups();
        ex
    }

    fn run(&mut self) {
        match self.lang {
            Lang::Python => self.run_python(),
            Lang::Ruby => self.run_ruby(),
            _ => self.run_braced(),
        }
    }

    fn into_parts(self) -> Parts {
        let calls = self.collect_calls();
        let symbols = self
            .syms
            .into_iter()
            .map(|s| Symbol {
                name: s.name,
                qual: s.qual,
                kind: s.kind,
                start: s.start_line,
                end: s.end_line.max(s.start_line),
                sig: s.sig,
                doc: s.doc,
                parent: s.parent.map(|p| p as u32),
                bases: s.bases,
                hash: 0,
            })
            .collect();
        Parts { symbols, calls, imports: self.imports }
    }

    // ----- token access -----

    fn n(&self) -> usize {
        self.toks.len()
    }

    fn k(&self, i: usize) -> Option<K> {
        self.toks.get(i).map(|t| t.k)
    }

    fn tx(&self, i: usize) -> &'a str {
        let text: &'a str = self.text;
        match self.toks.get(i) {
            Some(t) => &text[t.s..t.e],
            None => "",
        }
    }

    fn is_id(&self, i: usize) -> bool {
        self.k(i) == Some(K::Id)
    }

    fn id(&self, i: usize, w: &str) -> bool {
        self.is_id(i) && self.tx(i) == w
    }

    fn p(&self, i: usize, w: &str) -> bool {
        self.k(i) == Some(K::P) && self.tx(i) == w
    }

    /// True when token `i` starts a line, that is, no token before it ends on its line.
    fn first_on_line(&self, i: usize) -> bool {
        i == 0 || self.toks[i].line > self.toks[i - 1].end_line
    }

    fn last_line(&self) -> u32 {
        self.toks.last().map_or(1, |t| t.end_line)
    }

    /// The text between the delimiters of the string token `i`.
    fn str_body(&self, i: usize) -> String {
        let raw = self.tx(i);
        let body = raw.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        for q in ["\"\"\"", "'''"] {
            if let Some(r) = body.strip_prefix(q) {
                return r.strip_suffix(q).unwrap_or(r).to_string();
            }
        }
        match body.chars().next() {
            Some(c @ ('"' | '\'' | '`')) => {
                let inner = &body[1..];
                inner.strip_suffix(c).unwrap_or(inner).to_string()
            }
            _ => body.to_string(),
        }
    }

    // ----- comments and documentation -----

    fn build_groups(&self) -> Vec<Group> {
        let bytes = self.text.as_bytes();
        let mut out: Vec<Group> = Vec::new();
        let mut prev_line_kind = false;
        for &(s, e) in &self.m.comments {
            let l1 = self.lines.line_of(s);
            let l2 = self.lines.line_of(e.saturating_sub(1).max(s));
            let ls = self.lines.start_of(l1);
            let own = bytes[ls..s].iter().all(|c| matches!(c, b' ' | b'\t' | b'\r'));
            let shebang = l1 == 1 && bytes[s..e].starts_with(b"#!");
            if !own || shebang {
                prev_line_kind = false;
                continue;
            }
            let line_kind = !bytes[s..e].starts_with(b"/*") && !bytes[s..e].starts_with(b"=begin");
            match out.last_mut() {
                Some(g) if line_kind && prev_line_kind && g.last + 1 == l1 => {
                    g.last = l2;
                    g.end = e;
                }
                _ => out.push(Group { last: l2, start: s, end: e }),
            }
            prev_line_kind = line_kind;
        }
        out
    }

    /// True when the masked line `ln` is an attribute, annotation or decorator line, which
    /// may sit between a documentation comment and the declaration it documents.
    fn attr_line(&self, ln: u32) -> bool {
        let from = self.lines.start_of(ln);
        let to = self.lines.end_of(ln);
        let line = &self.m.code[from..to.min(self.m.code.len())];
        let trimmed: &[u8] = {
            let skip = line.iter().take_while(|c| c.is_ascii_whitespace()).count();
            &line[skip..]
        };
        match trimmed.first() {
            Some(b'@') => self.lang != Lang::Ruby,
            Some(b'#') => trimmed.get(1) == Some(&b'['),
            Some(b'[') => self.lang == Lang::CSharp,
            _ => false,
        }
    }

    fn comment_text(&self, g: &Group) -> String {
        let mut out = String::new();
        for line in self.text[g.start..g.end].lines() {
            let mut t = line.trim();
            if t.starts_with("=begin") || t.starts_with("=end") {
                continue;
            }
            if t.starts_with('/') || t.starts_with('#') || t.starts_with('*') {
                t = t.trim_start_matches(|c| matches!(c, '/' | '#' | '*' | '!'));
            }
            let t = t.trim_end().trim_end_matches("*/").trim_end_matches('*').trim();
            out.push_str(t);
            out.push('\n');
        }
        out
    }

    /// The comment block directly above the declaration that starts on `line`.
    fn doc_above(&self, line: u32) -> String {
        let mut ln = line;
        while ln > 1 && self.attr_line(ln - 1) {
            ln -= 1;
        }
        if ln <= 1 {
            return String::new();
        }
        let want = ln - 1;
        let idx = self.groups.partition_point(|g| g.last < want);
        match self.groups.get(idx) {
            Some(g) if g.last == want => self.comment_text(g),
            _ => String::new(),
        }
    }

    // ----- symbols and frames -----

    fn add_sym(&mut self, n: New) -> usize {
        let first = self.toks[n.first];
        let sig_end = n.sig_end.max(first.e).min(self.text.len());
        let sig = fold(&without_comments(self.text, first.s, sig_end, &self.m.comments, usize::MAX));
        let doc = self.doc_above(first.line);
        self.syms.push(Sym {
            name: n.name,
            qual: n.qual,
            kind: n.kind,
            start_line: first.line,
            end_line: n.end_line,
            sig,
            doc,
            parent: n.parent,
            bases: n.bases,
            body: n.body,
        });
        self.syms.len() - 1
    }

    /// The enclosing definition and the qualified-name prefix at the current position.
    fn ctx(&self) -> (Option<usize>, String) {
        let parent = self.frames.iter().rev().find_map(|f| f.sym);
        let prefix = self.frames.last().map(|f| f.prefix.clone()).unwrap_or_default();
        (parent, prefix)
    }

    fn container_kind(&self) -> Option<SymKind> {
        match self.frames.last() {
            Some(Frame { kind: FKind::Type(k), .. }) if k.is_container() => Some(*k),
            _ => None,
        }
    }

    fn method_kind(&self) -> SymKind {
        if self.container_kind().is_some() {
            SymKind::Method
        } else {
            SymKind::Function
        }
    }

    fn in_func(&self) -> bool {
        self.frames.iter().any(|f| f.kind == FKind::Func)
    }

    /// Opens a region for the brace at `open` and returns the token to continue from.
    fn enter(&mut self, open: usize, sym: Option<usize>, prefix: String, kind: FKind) -> usize {
        let end = if self.mate[open] == NONE { self.n() } else { self.mate[open] };
        self.push_frame(Frame { end, sym, prefix, kind });
        open + 1
    }

    /// Opens a region, unless the scan is already `MAX_FRAMES` regions deep. What is inside a
    /// region that was not opened is filed under the one around it.
    fn push_frame(&mut self, frame: Frame) {
        if self.frames.len() < MAX_FRAMES {
            self.frames.push(frame);
        }
    }

    /// Line of the brace that closes the one at `open`, or the last line of the file.
    fn close_line(&self, open: usize) -> u32 {
        let m = self.mate[open];
        if m == NONE {
            self.last_line()
        } else {
            self.toks[m].end_line
        }
    }

    fn close_tok(&self, open: usize) -> usize {
        let m = self.mate[open];
        if m == NONE {
            self.n() - 1
        } else {
            m
        }
    }

    /// Marks tokens `from..=to` as not being calls.
    fn mute(&mut self, from: usize, to: usize) {
        let to = to.min(self.n().saturating_sub(1));
        for k in from..=to {
            if k < self.skip.len() {
                self.skip[k] = true;
            }
        }
    }

    /// Index after the generic argument list that opens at `i`, if one does.
    fn skip_angles(&self, i: usize) -> Option<usize> {
        if !self.p(i, "<") {
            return None;
        }
        let mut depth = 0i32;
        let mut j = i;
        while j < self.n() && j < i + 240 {
            if self.toks[j].k == K::P {
                match self.tx(j) {
                    "<" => depth += 1,
                    ">" => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    "(" | "[" | "{" => {
                        let m = self.mate[j];
                        if m == NONE {
                            return None;
                        }
                        j = m;
                    }
                    ";" | "}" | ")" | "]" | "&&" | "||" | "==" | "!=" => return None,
                    _ => {}
                }
            }
            j += 1;
        }
        None
    }

    /// Index of the `<` that the `>` at `p` closes, looking backwards.
    fn angle_open_back(&self, p: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut j = p;
        loop {
            if self.toks[j].k == K::P {
                match self.tx(j) {
                    ">" => depth += 1,
                    "<" => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j);
                        }
                    }
                    ";" | "{" | "}" | "(" | ")" => return None,
                    _ => {}
                }
            }
            if j == 0 || p - j > 120 {
                return None;
            }
            j -= 1;
        }
    }

    /// Where the body of a declaration whose header continues at `from` begins.
    fn find_body(&self, from: usize) -> Body {
        let nl = matches!(self.lang, Lang::Kotlin | Lang::Swift | Lang::Go);
        self.find_body_nl(from, nl)
    }

    /// Like `find_body`; with `nl` a line break ends the header unless the next token
    /// plainly continues it.
    fn find_body_nl(&self, from: usize, nl: bool) -> Body {
        let n = self.n();
        let mut j = from;
        let mut steps = 0;
        while j < n && steps < 4000 {
            steps += 1;
            let t = self.toks[j];
            if nl && j > 0 && t.line > self.toks[j - 1].end_line && !self.header_continues(j) {
                return Body::Nothing;
            }
            if t.k == K::P {
                match self.tx(j) {
                    "{" => {
                        if self.lang == Lang::Go && j > 0 && (self.id(j - 1, "struct") || self.id(j - 1, "interface")) {
                            let m = self.mate[j];
                            if m == NONE {
                                return Body::Nothing;
                            }
                            j = m + 1;
                            continue;
                        }
                        return Body::Braces(j);
                    }
                    "(" | "[" => {
                        let m = self.mate[j];
                        if m == NONE {
                            return Body::Nothing;
                        }
                        j = m + 1;
                        continue;
                    }
                    ";" => return Body::Semi(j),
                    "}" | ")" | "]" => return Body::Nothing,
                    "=>" => return Body::Arrow(j),
                    "=" => return Body::Eq(j),
                    ":" if self.lang == Lang::Cpp => {
                        if let Some(b) = self.cpp_init_body(j + 1) {
                            return Body::Braces(b);
                        }
                    }
                    _ => {}
                }
            }
            j += 1;
        }
        Body::Nothing
    }

    /// The `{` that opens a C++ constructor body after the member initializer list that
    /// starts at `from`, if what follows is such a list (`a(1), b{2}`).
    fn cpp_init_body(&self, from: usize) -> Option<usize> {
        let mut j = from;
        loop {
            if !self.is_id(j) {
                return None;
            }
            while self.p(j + 1, "::") && self.is_id(j + 2) {
                j += 2;
            }
            j += 1;
            if let Some(a) = self.skip_angles(j) {
                j = a;
            }
            if self.p(j, "(") || self.p(j, "{") {
                let m = self.mate[j];
                if m == NONE {
                    return None;
                }
                j = m + 1;
            } else {
                return None;
            }
            if self.p(j, ",") {
                j += 1;
                continue;
            }
            return if self.p(j, "{") { Some(j) } else { None };
        }
    }

    /// True when token `j`, first on its line, still belongs to the header above it.
    fn header_continues(&self, j: usize) -> bool {
        if self.toks[j].k == K::P && matches!(self.tx(j), "{" | ":" | "->" | "=" | "," | "." | "&" | "|" | "?") {
            return true;
        }
        if self.is_id(j) && matches!(self.tx(j), "where" | "throws" | "rethrows" | "async" | "extends" | "implements") {
            return true;
        }
        j > 0 && self.toks[j - 1].k == K::P && matches!(self.tx(j - 1), "," | ":" | "->" | "=" | "&" | "|" | "." | "=>" | "(")
    }

    /// True when the line break before token `j` does not end the statement that
    /// token `j - 1` belongs to.
    fn joins(&self, j: usize) -> bool {
        let prev_op = self.toks[j - 1].k == K::P
            && matches!(
                self.tx(j - 1),
                "," | "=" | "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "<" | ">" | "?" | ":" | "." | "->" | "=>"
                    | "&&" | "||" | "::" | "?." | "(" | "[" | "{" | "+=" | "-="
            );
        let next_op = self.toks[j].k == K::P
            && matches!(self.tx(j), "." | "->" | "?." | "&&" | "||" | "?" | ":" | "+" | "*" | "/" | "|" | "&" | "=>" | "," | ")" | "]" | "}");
        prev_op || next_op
    }

    /// The last token of the statement that starts at `from`: the `;` that ends it, or
    /// the last token before a line break that does not continue it.
    fn stmt_end(&self, from: usize) -> usize {
        let n = self.n();
        let mut j = from;
        let mut last = from.min(n.saturating_sub(1));
        while j < n {
            if j > from && self.toks[j].line > self.toks[last].end_line && !self.joins(j) {
                return last;
            }
            if self.toks[j].k == K::P {
                match self.tx(j) {
                    "(" | "[" | "{" => {
                        let m = self.mate[j];
                        if m == NONE {
                            return n - 1;
                        }
                        last = m;
                        j = m + 1;
                        continue;
                    }
                    ";" => return j,
                    "}" | ")" | "]" => return last,
                    _ => {}
                }
            }
            last = j;
            j += 1;
        }
        last
    }

    /// The `;` that ends the declaration starting at `from`, or the last token before a
    /// closing brace.
    fn semi_end(&self, from: usize) -> usize {
        let n = self.n();
        let mut j = from;
        while j < n {
            if self.toks[j].k == K::P {
                match self.tx(j) {
                    "(" | "[" | "{" => {
                        let m = self.mate[j];
                        if m == NONE {
                            return n - 1;
                        }
                        j = m + 1;
                        continue;
                    }
                    ";" => return j,
                    "}" => return j.saturating_sub(1).max(from),
                    _ => {}
                }
            }
            j += 1;
        }
        n.saturating_sub(1)
    }

    /// The first token of the declaration whose keyword or name is at `i`, going back
    /// over the modifiers that `is_mod` accepts.
    fn mods_back(&self, i: usize, is_mod: fn(&str) -> bool) -> usize {
        let mut j = i;
        while j > 0 {
            let p = j - 1;
            match self.toks[p].k {
                K::Id if is_mod(self.tx(p)) => j = p,
                K::P if self.p(p, ")") && self.mate[p] != NONE && self.mate[p] > 0 && self.id(self.mate[p] - 1, "pub") => {
                    j = self.mate[p] - 1
                }
                K::Str if p > 0 && self.id(p - 1, "extern") => j = p,
                _ => break,
            }
        }
        j
    }

    /// The names a class-like header lists after `extends`, `implements`, `:` and the like,
    /// from token `from` up to token `to`, as written.
    fn bases_between(&self, from: usize, to: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut collecting = false;
        let mut part = NONE;
        let mut j = from;
        let flush = |part: usize, end: usize, out: &mut Vec<String>, this: &Ex| {
            if part == NONE || end <= part {
                return;
            }
            let mut text = this.text[this.toks[part].s..this.toks[end - 1].e].trim();
            for word in ["public ", "private ", "protected ", "virtual ", "internal "] {
                while let Some(r) = text.strip_prefix(word) {
                    text = r.trim_start();
                }
            }
            let text = match text.find('(') {
                Some(c) => &text[..c],
                None => text,
            };
            let text = fold(text);
            if !text.is_empty() {
                out.push(text);
            }
        };
        while j < to {
            let t = self.toks[j];
            if t.k == K::Id && self.tx(j) == "where" {
                break;
            }
            let marker = (t.k == K::Id && matches!(self.tx(j), "extends" | "implements" | "inherits" | "conforms"))
                || self.p(j, ":");
            if marker && (!collecting || t.k == K::Id) {
                if collecting {
                    flush(part, j, &mut out, self);
                }
                collecting = true;
                part = j + 1;
                j += 1;
                continue;
            }
            if t.k == K::P {
                match self.tx(j) {
                    "(" | "[" | "{" => {
                        let m = self.mate[j];
                        if m == NONE || m >= to {
                            break;
                        }
                        j = m + 1;
                        continue;
                    }
                    "<" => {
                        if let Some(a) = self.skip_angles(j) {
                            j = a;
                            continue;
                        }
                    }
                    "," | "+" if collecting => {
                        flush(part, j, &mut out, self);
                        part = j + 1;
                    }
                    _ => {}
                }
            }
            j += 1;
        }
        if collecting {
            flush(part, to.min(j.max(part)), &mut out, self);
        }
        out
    }

    // ----- Python: indentation -----

    fn indent_of(&self, i: usize) -> usize {
        let t = self.toks[i];
        let ls = self.lines.start_of(t.line);
        let mut w = 0;
        for &c in &self.text.as_bytes()[ls..t.s.max(ls)] {
            match c {
                b' ' => w += 1,
                b'\t' => w = (w / 8 + 1) * 8,
                0x0c => w = 0,
                _ => break,
            }
        }
        w
    }

    fn run_python(&mut self) {
        let n = self.n();
        if n == 0 {
            return;
        }
        let bytes = self.text.as_bytes();
        let mut starts: Vec<usize> = Vec::new();
        let mut depth = 0i32;
        for i in 0..n {
            let t = self.toks[i];
            // A `def` or `class` at the left margin cannot sit inside a bracket, so one that
            // appears while a bracket is open means the bracket was never closed.
            if depth > 0 && self.first_on_line(i) && self.starts_definition_at_margin(i) {
                depth = 0;
            }
            if depth == 0 && !t.cont && (i == 0 || t.line > self.toks[i - 1].end_line) {
                starts.push(i);
            }
            if t.k == K::P && t.e - t.s == 1 {
                match bytes[t.s] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth = (depth - 1).max(0),
                    _ => {}
                }
            }
        }
        // Definitions still open, with the indentation of their keyword.
        let mut open: Vec<(usize, usize)> = Vec::new();
        let mut prev_last = 0usize;
        for (li, &first) in starts.iter().enumerate() {
            let last = if li + 1 < starts.len() { starts[li + 1] - 1 } else { n - 1 };
            let indent = self.indent_of(first);
            while let Some(&(ind, sym)) = open.last() {
                if ind < indent {
                    break;
                }
                self.close_indented(sym, prev_last);
                open.pop();
            }
            self.python_imports(first, last);
            let kw = if self.id(first, "async") && self.id(first + 1, "def") { first + 1 } else { first };
            let is_class = self.id(kw, "class");
            if (self.id(kw, "def") || is_class) && self.is_id(kw + 1) {
                let name_i = kw + 1;
                let name = self.tx(name_i).to_string();
                let mut depth = 0i32;
                let mut colon = NONE;
                for j in name_i + 1..=last {
                    let t = self.toks[j];
                    if t.k == K::P && t.e - t.s == 1 {
                        match bytes[t.s] {
                            b'(' | b'[' | b'{' => depth += 1,
                            b')' | b']' | b'}' => depth = (depth - 1).max(0),
                            b':' if depth == 0 => {
                                colon = j;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                let header_end = if colon == NONE { last } else { colon };
                let sig_end = if colon == NONE { self.toks[last].e } else { self.toks[colon].s };
                let parent = open.last().map(|&(_, s)| s);
                let kind = if is_class {
                    SymKind::Class
                } else if parent.map_or(false, |p| self.syms[p].kind == SymKind::Class) {
                    SymKind::Method
                } else {
                    SymKind::Function
                };
                let qual = match parent {
                    Some(p) => join(&self.syms[p].qual, &name),
                    None => name.clone(),
                };
                let bases = if is_class { self.python_bases(name_i, header_end) } else { Vec::new() };
                let inline = colon != NONE && colon < last;
                // The first statement of the body, when it is a lone string, is the docstring.
                let doc_line = if inline {
                    Some((colon + 1, last))
                } else if li + 1 < starts.len() && self.indent_of(starts[li + 1]) > indent {
                    let next_last = if li + 2 < starts.len() { starts[li + 2] - 1 } else { n - 1 };
                    Some((starts[li + 1], next_last))
                } else {
                    None
                };
                let docstring = doc_line
                    .filter(|&(a, b)| (a..=b).all(|j| self.toks[j].k == K::Str))
                    .map(|(a, _)| self.str_body(a));
                let end_line = if inline { self.toks[last].end_line } else { self.toks[header_end].end_line };
                let sym = self.add_sym(New {
                    name,
                    kind,
                    first,
                    sig_end,
                    end_line,
                    body: Some((header_end, if inline { last } else { header_end })),
                    bases,
                    qual,
                    parent,
                });
                self.skip[name_i] = true;
                if let Some(d) = docstring {
                    self.syms[sym].doc = d;
                }
                if !inline {
                    open.push((indent, sym));
                }
            }
            prev_last = last;
        }
        while let Some((_, sym)) = open.pop() {
            self.close_indented(sym, prev_last);
        }
    }

    /// True when token `i` is a `def`, `async def` or `class` that stands in the first column.
    fn starts_definition_at_margin(&self, i: usize) -> bool {
        let keyword = self.id(i, "def") || self.id(i, "class") || (self.id(i, "async") && self.id(i + 1, "def"));
        keyword && self.toks[i].s == self.lines.start_of(self.toks[i].line)
    }

    fn close_indented(&mut self, sym: usize, last_tok: usize) {
        let end_line = self.toks[last_tok].end_line;
        let s = &mut self.syms[sym];
        s.end_line = s.end_line.max(end_line);
        if let Some(b) = s.body.as_mut() {
            b.1 = b.1.max(last_tok);
        }
    }

    /// The base classes in the parentheses after a class name, without keyword arguments.
    fn python_bases(&self, name_i: usize, limit: usize) -> Vec<String> {
        let open = name_i + 1;
        if !self.p(open, "(") {
            return Vec::new();
        }
        let close = self.mate[open];
        if close == NONE || close > limit {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut part = open + 1;
        let mut j = open + 1;
        while j <= close {
            if j == close || self.p(j, ",") {
                if j > part {
                    let keyword = self.is_id(part) && self.p(part + 1, "=");
                    if !keyword {
                        out.push(fold(&self.text[self.toks[part].s..self.toks[j - 1].e]));
                    }
                }
                part = j + 1;
                j += 1;
                continue;
            }
            if self.toks[j].k == K::P && matches!(self.tx(j), "(" | "[" | "{") {
                let m = self.mate[j];
                if m != NONE && m < close {
                    j = m + 1;
                    continue;
                }
            }
            j += 1;
        }
        out
    }

    /// Imports among the tokens of one logical line: `import` and `from` statements,
    /// also after a `;` or after the colon of a one-line `try:` or `if:`.
    fn python_imports(&mut self, first: usize, last: usize) {
        let compound = matches!(self.tx(first), "if" | "elif" | "else" | "try" | "except" | "finally" | "with" | "for" | "while");
        let mut depth = 0i32;
        let mut j = first;
        while j <= last {
            let at_start = j == first || self.p(j - 1, ";") || (compound && depth == 0 && self.p(j - 1, ":"));
            if at_start && self.is_id(j) {
                match self.tx(j) {
                    "import" => {
                        j = self.py_import(j, last);
                        continue;
                    }
                    "from" => {
                        j = self.py_from(j, last);
                        continue;
                    }
                    _ => {}
                }
            }
            if self.toks[j].k == K::P {
                match self.tx(j) {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => depth = (depth - 1).max(0),
                    _ => {}
                }
            }
            j += 1;
        }
    }

    /// A dotted name starting at `j`, and the index after it.
    fn dotted(&self, mut j: usize, last: usize) -> (String, usize) {
        let mut name = String::new();
        while j <= last && self.is_id(j) && !self.id(j, "as") && !self.id(j, "import") {
            name.push_str(self.tx(j));
            j += 1;
            if j <= last && self.p(j, ".") && self.is_id(j + 1) {
                name.push('.');
                j += 1;
            } else {
                break;
            }
        }
        (name, j)
    }

    fn py_import(&mut self, at: usize, last: usize) -> usize {
        let line = self.toks[at].line;
        let mut j = at + 1;
        while j <= last {
            let (module, next) = self.dotted(j, last);
            if module.is_empty() {
                break;
            }
            j = next;
            let mut alias = module.clone();
            if self.id(j, "as") && self.is_id(j + 1) {
                alias = self.tx(j + 1).to_string();
                j += 2;
            }
            self.imports.push(Import { module, name: String::new(), alias, level: 0, line });
            if self.p(j, ",") {
                j += 1;
            } else {
                break;
            }
        }
        j.max(at + 1)
    }

    fn py_from(&mut self, at: usize, last: usize) -> usize {
        let line = self.toks[at].line;
        let mut j = at + 1;
        let mut level = 0usize;
        while j <= last && self.toks[j].k == K::P && self.tx(j).bytes().all(|c| c == b'.') {
            level += self.toks[j].e - self.toks[j].s;
            j += 1;
        }
        let (module, next) = self.dotted(j, last);
        j = next;
        if !self.id(j, "import") {
            return j.max(at + 1);
        }
        j += 1;
        let level = level.min(255) as u8;
        let parens = self.p(j, "(");
        if parens {
            j += 1;
        }
        if self.p(j, "*") {
            self.imports.push(Import { module, name: "*".to_string(), alias: "*".to_string(), level, line });
            return j + 1;
        }
        while j <= last && self.is_id(j) && !self.id(j, "as") {
            let name = self.tx(j).to_string();
            j += 1;
            let mut alias = name.clone();
            if self.id(j, "as") && self.is_id(j + 1) {
                alias = self.tx(j + 1).to_string();
                j += 2;
            }
            self.imports.push(Import { module: module.clone(), name, alias, level, line });
            if self.p(j, ",") {
                j += 1;
            } else {
                break;
            }
        }
        if parens && self.p(j, ")") {
            j += 1;
        }
        j.max(at + 1)
    }

    // ----- calls -----

    /// Index of the `(` that makes token `i` a call, if it does.
    fn call_paren(&self, i: usize) -> Option<usize> {
        let t = self.toks[i];
        let next = i + 1;
        if next >= self.n() {
            return None;
        }
        if self.p(next, "(") {
            return if self.toks[next].line == t.end_line { Some(next) } else { None };
        }
        let generic = is_ts(self.lang) || matches!(self.lang, Lang::CSharp | Lang::Kotlin | Lang::Swift | Lang::Java);
        if generic && self.p(next, "<") && self.toks[next].s == t.e {
            if let Some(a) = self.skip_angles(next) {
                if self.p(a, "(") && self.toks[a].s == self.toks[a - 1].e {
                    return Some(a);
                }
            }
        }
        if self.lang == Lang::Rust && self.p(next, "::") && self.p(next + 1, "<") {
            if let Some(a) = self.skip_angles(next + 1) {
                if self.p(a, "(") {
                    return Some(a);
                }
            }
        }
        None
    }

    /// True for the token that joins a receiver to the member it is called on. The arrow does
    /// that in C, C++, C# (pointers) and PHP only; elsewhere it is the arrow of a lambda, of a
    /// `case` or `when` branch, of a return type, and what comes before it is not a receiver.
    fn is_sep(&self, j: usize) -> bool {
        if self.toks[j].k != K::P {
            return false;
        }
        match self.tx(j) {
            "." | "::" | "?." => true,
            "->" => matches!(self.lang, Lang::C | Lang::Cpp | Lang::CSharp | Lang::Php),
            _ => false,
        }
    }

    /// The chain a call is made on: `self.cache` for `self.cache.load(x)`.
    fn recv_of(&self, name_i: usize) -> String {
        let mut start = name_i;
        let mut cur = name_i;
        let mut guard = 0;
        while cur > 1 && guard < 64 {
            guard += 1;
            let sep = cur - 1;
            if !self.is_sep(sep) {
                break;
            }
            let mut a = sep - 1;
            while a > 0 && (self.p(a, "!") || self.p(a, "?") || (self.lang == Lang::Ruby && self.p(a, "&"))) {
                a -= 1;
            }
            match self.toks[a].k {
                K::Id => {
                    start = a;
                    cur = a;
                }
                K::Num | K::Str => {
                    start = a;
                    break;
                }
                K::P if self.p(a, ")") || self.p(a, "]") => {
                    let mut s = self.mate[a];
                    if s == NONE {
                        break;
                    }
                    loop {
                        if s == 0 {
                            break;
                        }
                        let q = s - 1;
                        if (self.p(q, ")") || self.p(q, "]")) && self.mate[q] != NONE {
                            s = self.mate[q];
                            continue;
                        }
                        if self.is_id(q) && !is_keyword(self.lang, self.tx(q)) {
                            s = q;
                        }
                        break;
                    }
                    start = s;
                    cur = s;
                }
                _ => break,
            }
        }
        if start == name_i {
            return String::new();
        }
        self.text[self.toks[start].s..self.toks[name_i - 1].s].trim_end_matches('?').to_string()
    }

    fn collect_calls(&self) -> Vec<Call> {
        let mut calls = Vec::new();
        if self.lang == Lang::Shell {
            return calls;
        }
        let n = self.n();
        let mut order: Vec<usize> = (0..self.syms.len())
            .filter(|&s| self.syms[s].kind.is_callable() && self.syms[s].body.is_some())
            .collect();
        order.sort_by_key(|&s| self.syms[s].body.map_or(0, |b| b.0));
        let mut stack: Vec<(usize, usize)> = Vec::new();
        let mut p = 0;
        for i in 0..n {
            while let Some(&(end, _)) = stack.last() {
                if end < i {
                    stack.pop();
                } else {
                    break;
                }
            }
            while p < order.len() {
                let (start, end) = self.syms[order[p]].body.unwrap_or((NONE, NONE));
                if start > i {
                    break;
                }
                if end >= i {
                    stack.push((end, order[p]));
                }
                p += 1;
            }
            if self.toks[i].k != K::Id || self.skip[i] {
                continue;
            }
            let Some(_paren) = self.call_paren(i) else { continue };
            let name = self.tx(i);
            let sep_before = i > 0 && self.is_sep(i - 1);
            if !sep_before && is_keyword(self.lang, name) {
                continue;
            }
            if i > 0 {
                if self.p(i - 1, "@") && matches!(self.lang, Lang::Java | Lang::Kotlin | Lang::Swift) {
                    continue;
                }
                if self.is_id(i - 1) && is_declaring_word(self.tx(i - 1)) {
                    continue;
                }
            }
            let recv = self.recv_of(i);
            let caller = stack.last().map(|&(_, s)| s as u32);
            calls.push(Call { caller, name: name.to_string(), recv, line: self.toks[i].line });
        }
        calls
    }
}

/// Words that name what comes next as a declaration, so that the name is not a call.
fn is_declaring_word(w: &str) -> bool {
    matches!(
        w,
        "function" | "fn" | "func" | "fun" | "def" | "class" | "struct" | "enum" | "union" | "interface" | "trait" | "namespace"
            | "record" | "object" | "protocol" | "extension" | "typedef" | "sub"
    )
}

/// True for a word that can be followed by a parenthesis without being a call: a control
/// keyword, an operator written as a word, or a type name of the C family.
fn is_keyword(lang: Lang, w: &str) -> bool {
    if matches!(
        w,
        "if" | "else" | "for" | "while" | "do" | "switch" | "case" | "catch" | "finally" | "try" | "return" | "throw" | "in"
            | "is" | "as" | "class" | "struct" | "enum" | "union" | "interface"
    ) {
        return true;
    }
    match lang {
        Lang::Python => matches!(
            w,
            "elif" | "except" | "raise" | "yield" | "assert" | "with" | "pass" | "not" | "and" | "or" | "lambda" | "def"
                | "del" | "import" | "from" | "global" | "nonlocal" | "await" | "async"
        ),
        Lang::Ruby => matches!(
            w,
            "elsif" | "unless" | "until" | "rescue" | "ensure" | "then" | "begin" | "end" | "def" | "not" | "and" | "or"
                | "yield" | "defined?" | "when" | "module" | "retry" | "redo"
        ),
        Lang::Rust => matches!(
            w,
            "match" | "loop" | "fn" | "impl" | "trait" | "mod" | "where" | "unsafe" | "async" | "await" | "move" | "dyn"
                | "ref" | "mut" | "let" | "use" | "pub" | "crate" | "type"
        ),
        Lang::Go => matches!(w, "go" | "defer" | "select" | "func" | "type" | "var" | "const" | "import" | "range"),
        Lang::Java => {
            matches!(w, "synchronized" | "assert" | "instanceof" | "throws" | "extends" | "implements" | "import" | "package")
                || is_c_type_word(w)
        }
        Lang::CSharp => {
            matches!(
                w,
                "lock" | "using" | "fixed" | "checked" | "unchecked" | "when" | "unsafe" | "foreach" | "nameof" | "default"
                    | "typeof" | "sizeof" | "stackalloc" | "delegate" | "operator" | "async" | "await" | "namespace"
            ) || is_c_type_word(w)
        }
        Lang::C | Lang::Cpp => {
            matches!(
                w,
                "sizeof" | "alignas" | "alignof" | "_Alignof" | "decltype" | "noexcept" | "typeid" | "typeof" | "static_assert"
                    | "_Static_assert" | "__attribute__" | "__declspec" | "defined" | "template" | "typename" | "operator"
                    | "namespace" | "using" | "delete" | "asm" | "__asm__"
            ) || is_c_type_word(w)
        }
        Lang::Kotlin => matches!(w, "when" | "fun" | "object" | "constructor" | "import" | "package"),
        Lang::Swift => matches!(w, "guard" | "defer" | "func" | "repeat" | "import" | "typealias" | "where" | "subscript"),
        Lang::JavaScript | Lang::Jsx | Lang::TypeScript | Lang::Tsx => matches!(
            w,
            "function" | "async" | "await" | "typeof" | "void" | "delete" | "yield" | "with" | "import" | "export" | "debugger"
                | "instanceof"
        ),
        Lang::Php => matches!(
            w,
            "function" | "fn" | "array" | "list" | "isset" | "empty" | "unset" | "echo" | "print" | "exit" | "die" | "include"
                | "include_once" | "require" | "require_once" | "eval" | "declare" | "elseif" | "foreach" | "use" | "endif"
                | "endwhile" | "endfor" | "endforeach" | "endswitch" | "namespace" | "instanceof"
        ),
        Lang::Shell => true,
    }
}

/// A primitive type or qualifier of the C family, which a parenthesis may follow in a
/// cast or a function pointer.
fn is_c_type_word(w: &str) -> bool {
    matches!(
        w,
        "void" | "int" | "char" | "short" | "long" | "float" | "double" | "bool" | "boolean" | "byte" | "sbyte" | "uint"
            | "ulong" | "ushort" | "unsigned" | "signed" | "decimal" | "auto" | "const" | "volatile" | "static" | "register"
            | "extern" | "constexpr" | "consteval" | "inline" | "virtual" | "explicit" | "public" | "private" | "protected"
            | "internal" | "override" | "final" | "abstract"
    )
}

// ---------------------------------------------------------------------------------
// Languages with braces
// ---------------------------------------------------------------------------------

/// Words that may stand before a declaration as modifiers.
fn is_decl_mod(w: &str) -> bool {
    matches!(
        w,
        "public" | "private" | "protected" | "internal" | "static" | "final" | "abstract" | "sealed" | "open" | "override"
            | "virtual" | "partial" | "readonly" | "const" | "constexpr" | "consteval" | "inline" | "extern" | "unsafe"
            | "async" | "suspend" | "noinline" | "crossinline" | "tailrec" | "operator" | "infix" | "data" | "enum"
            | "annotation" | "inner" | "value" | "lateinit" | "mutating" | "nonmutating" | "convenience" | "required"
            | "dynamic" | "indirect" | "optional" | "lazy" | "weak" | "unowned" | "fileprivate" | "synchronized"
            | "native" | "transient" | "volatile" | "strictfp" | "default" | "implicit" | "explicit" | "friend" | "export"
            | "typedef" | "expect" | "actual" | "external" | "companion" | "class" | "fun" | "declare" | "new"
    )
}

fn is_rust_mod(w: &str) -> bool {
    matches!(w, "pub" | "async" | "const" | "unsafe" | "extern" | "default" | "safe" | "crate")
}

fn is_php_mod(w: &str) -> bool {
    matches!(w, "public" | "private" | "protected" | "static" | "abstract" | "final" | "readonly")
}

fn is_js_mod(w: &str) -> bool {
    matches!(
        w,
        "export" | "default" | "async" | "declare" | "abstract" | "static" | "public" | "private" | "protected" | "readonly"
            | "override" | "get" | "set" | "const"
    )
}

/// Words that cannot be part of the type in front of a function name.
fn not_decl_prefix(w: &str) -> bool {
    matches!(
        w,
        "return" | "else" | "new" | "throw" | "delete" | "goto" | "case" | "do" | "await" | "yield" | "typeof" | "sizeof"
            | "in" | "is" | "as" | "if" | "while" | "for" | "switch" | "catch" | "extends" | "implements" | "throws"
            | "instanceof" | "import" | "package" | "using" | "namespace" | "and" | "or" | "not"
    )
}

/// The container a keyword introduces in the given language.
fn type_kind(lang: Lang, w: &str) -> Option<SymKind> {
    Some(match (lang, w) {
        (Lang::Java, "class" | "record")
        | (Lang::Kotlin, "class" | "object")
        | (Lang::CSharp, "class" | "record")
        | (Lang::Cpp, "class")
        | (Lang::Php, "class")
        | (Lang::Swift, "class" | "actor") => SymKind::Class,
        (Lang::Java | Lang::Kotlin | Lang::CSharp | Lang::Php, "interface") | (Lang::Swift, "protocol") => SymKind::Interface,
        (Lang::Java | Lang::CSharp | Lang::C | Lang::Cpp | Lang::Php | Lang::Swift, "enum") => SymKind::Enum,
        (Lang::CSharp | Lang::C | Lang::Cpp | Lang::Swift, "struct") | (Lang::C | Lang::Cpp, "union") => SymKind::Struct,
        (Lang::Php, "trait") => SymKind::Trait,
        (Lang::Swift, "extension") => SymKind::Impl,
        _ => return None,
    })
}

fn reserved_type_name(w: &str) -> bool {
    matches!(
        w,
        "func" | "var" | "let" | "init" | "subscript" | "deinit" | "operator" | "fun" | "val" | "class" | "static" | "final"
            | "override" | "extends" | "implements" | "where" | "struct" | "enum" | "interface"
    )
}

impl<'a> Ex<'a> {
    fn run_braced(&mut self) {
        let n = self.n();
        let mut i = 0;
        while i < n {
            while self.frames.last().map_or(false, |f| f.end < i) {
                self.frames.pop();
            }
            if self.lang == Lang::Rust
                && self.is_id(i)
                && self.p(i + 1, "(")
                && matches!(self.frames.last(), Some(Frame { kind: FKind::Type(SymKind::Enum), .. }))
            {
                self.skip[i] = true;
            }
            i = self.decl(i).filter(|&j| j > i).unwrap_or(i + 1);
        }
    }

    fn decl(&mut self, i: usize) -> Option<usize> {
        match self.lang {
            Lang::Rust => self.decl_rust(i),
            Lang::Go => self.decl_go(i),
            Lang::JavaScript | Lang::Jsx | Lang::TypeScript | Lang::Tsx => self.decl_js(i),
            Lang::Php => self.decl_php(i),
            Lang::Kotlin => self.decl_kotlin(i),
            Lang::Swift => self.decl_swift(i),
            Lang::Java | Lang::CSharp | Lang::C | Lang::Cpp => self.decl_typed(i),
            Lang::Shell => self.decl_shell(i),
            _ => None,
        }
    }

    /// Adds the function whose parameter list opens at `paren`, and enters its body.
    /// Returns the token to continue from.
    fn function_at(
        &mut self,
        first: usize,
        name: String,
        name_tok: usize,
        paren: usize,
        force_method: bool,
        qual: Option<String>,
    ) -> Option<usize> {
        let close = self.mate[paren];
        if close == NONE {
            return None;
        }
        let (parent, prefix) = self.ctx();
        let kind = if force_method { SymKind::Method } else { self.method_kind() };
        let qual = qual.unwrap_or_else(|| join(&prefix, &name));
        self.skip[name_tok] = true;
        let body = self.find_body(close + 1);
        let expression = match body {
            Body::Arrow(a) => Some(a),
            Body::Eq(a) if self.lang == Lang::Kotlin => Some(a),
            _ => None,
        };
        if let Body::Braces(b) = body {
            let end_line = self.close_line(b);
            let sym = self.add_sym(New {
                name,
                kind,
                first,
                sig_end: self.toks[b].s,
                end_line,
                body: Some((b, self.close_tok(b))),
                bases: Vec::new(),
                qual: qual.clone(),
                parent,
            });
            return Some(self.enter(b, Some(sym), qual, FKind::Func));
        }
        if let Some(a) = expression {
            let e = self.stmt_end(a + 1);
            let end_line = self.toks[e].end_line;
            self.add_sym(New {
                name,
                kind,
                first,
                sig_end: self.toks[a].s,
                end_line,
                body: Some((a, e)),
                bases: Vec::new(),
                qual,
                parent,
            });
            return Some(e + 1);
        }
        // No body: a trait or interface member, an abstract method, an external function.
        let last = match body {
            Body::Semi(s) => s,
            Body::Eq(a) | Body::Arrow(a) => self.semi_end(a),
            _ => {
                let mut j = close;
                while j + 1 < self.n() && self.toks[j + 1].line == self.toks[close].end_line && !self.p(j + 1, "}") && !self.p(j + 1, ";") {
                    j += 1;
                }
                j
            }
        };
        let sig_end = if self.p(last, ";") { self.toks[last].s } else { self.toks[last].e };
        let end_line = self.toks[last].end_line;
        self.add_sym(New { name, kind, first, sig_end, end_line, body: None, bases: Vec::new(), qual, parent });
        Some(last + 1)
    }

    /// Adds a class-like definition whose name is at `name_tok`, with the header ending
    /// at `after` and the body (when there is one) opening at `open`.
    #[allow(clippy::too_many_arguments)]
    fn container_at(
        &mut self,
        first: usize,
        name: String,
        name_tok: usize,
        kind: SymKind,
        after: usize,
        open: Option<usize>,
        header_last: usize,
    ) -> Option<usize> {
        let (parent, prefix) = self.ctx();
        let qual = join(&prefix, &name);
        // The type after the colon of a C or C++ enum is not a base.
        let bases = if kind == SymKind::Enum && matches!(self.lang, Lang::C | Lang::Cpp) {
            Vec::new()
        } else {
            self.bases_between(after, open.unwrap_or(header_last + 1))
        };
        let (sig_end, end_line, body) = match open {
            Some(b) => (self.toks[b].s, self.close_line(b), Some((b, self.close_tok(b)))),
            None => {
                let last = self.toks[header_last];
                (if self.p(header_last, ";") { last.s } else { last.e }, last.end_line, None)
            }
        };
        let sym = self.add_sym(New { name, kind, first, sig_end, end_line, body, bases, qual: qual.clone(), parent });
        self.skip[name_tok] = true;
        match open {
            Some(b) => Some(self.enter(b, Some(sym), qual, FKind::Type(kind))),
            None => Some(header_last + 1),
        }
    }

    /// Last token on the line of token `i`, going no further than a brace or semicolon.
    fn line_last(&self, i: usize) -> usize {
        let mut j = i;
        while j + 1 < self.n() && self.toks[j + 1].line == self.toks[i].end_line && !self.p(j + 1, "{") && !self.p(j + 1, ";") && !self.p(j + 1, "}") {
            j += 1;
        }
        j
    }

    // ----- Rust -----

    fn decl_rust(&mut self, i: usize) -> Option<usize> {
        if self.p(i, "#") {
            let mut o = i + 1;
            if self.p(o, "!") {
                o += 1;
            }
            if self.p(o, "[") && self.mate[o] != NONE {
                let m = self.mate[o];
                self.mute(o, m);
                return Some(m + 1);
            }
            return None;
        }
        if !self.is_id(i) || (i > 0 && (self.p(i - 1, ".") || self.p(i - 1, "::"))) {
            return None;
        }
        match self.tx(i) {
            "macro_rules" if self.p(i + 1, "!") => {
                let mut j = i + 2;
                if self.is_id(j) {
                    j += 1;
                }
                if (self.p(j, "{") || self.p(j, "(") || self.p(j, "[")) && self.mate[j] != NONE {
                    let m = self.mate[j];
                    self.mute(j, m);
                    return Some(m + 1);
                }
                None
            }
            "fn" => {
                let name_i = i + 1;
                if !self.is_id(name_i) {
                    return None;
                }
                let mut p = name_i + 1;
                if let Some(a) = self.skip_angles(p) {
                    p = a;
                }
                if !self.p(p, "(") {
                    return None;
                }
                let first = self.mods_back(i, is_rust_mod);
                // The header holds types only: `Fn(i32) -> i32` there is not a call.
                let close = self.mate[p];
                if close != NONE {
                    let end = match self.find_body(close + 1) {
                        Body::Braces(b) => b,
                        Body::Semi(s) => s,
                        _ => close,
                    };
                    self.mute(name_i, end);
                }
                self.function_at(first, self.tx(name_i).to_string(), name_i, p, false, None)
            }
            w @ ("struct" | "enum" | "union" | "trait") => {
                let name_i = i + 1;
                if !self.is_id(name_i) {
                    return None;
                }
                let kind = match w {
                    "enum" => SymKind::Enum,
                    "trait" => SymKind::Trait,
                    _ => SymKind::Struct,
                };
                let mut after = name_i + 1;
                if let Some(a) = self.skip_angles(after) {
                    after = a;
                }
                let first = self.mods_back(i, is_rust_mod);
                let name = self.tx(name_i).to_string();
                match self.find_body(after) {
                    Body::Braces(b) => self.container_at(first, name, name_i, kind, after, Some(b), b),
                    Body::Semi(s) => self.container_at(first, name, name_i, kind, after, None, s),
                    _ => None,
                }
            }
            "impl" => {
                let mut p = i + 1;
                if let Some(a) = self.skip_angles(p) {
                    p = a;
                }
                if self.p(p, "!") {
                    p += 1;
                }
                let (first_name, first_text, mut next) = self.rust_type(p)?;
                let (name, bases) = if self.id(next, "for") {
                    let (ty, _, after) = self.rust_type(next + 1)?;
                    next = after;
                    (ty, vec![first_text])
                } else {
                    (first_name, Vec::new())
                };
                let b = match self.find_body(next) {
                    Body::Braces(b) => b,
                    _ => return None,
                };
                let first = self.mods_back(i, is_rust_mod);
                let (parent, prefix) = self.ctx();
                let qual = join(&prefix, &name);
                let end_line = self.close_line(b);
                let sym = self.add_sym(New {
                    name,
                    kind: SymKind::Impl,
                    first,
                    sig_end: self.toks[b].s,
                    end_line,
                    body: Some((b, self.close_tok(b))),
                    bases,
                    qual: qual.clone(),
                    parent,
                });
                Some(self.enter(b, Some(sym), qual, FKind::Type(SymKind::Impl)))
            }
            "mod" => {
                let name_i = i + 1;
                if !self.is_id(name_i) {
                    return None;
                }
                match self.find_body(name_i + 1) {
                    Body::Braces(b) => {
                        let (_, prefix) = self.ctx();
                        let qual = join(&prefix, self.tx(name_i));
                        Some(self.enter(b, None, qual, FKind::Ns))
                    }
                    Body::Semi(s) => Some(s + 1),
                    _ => None,
                }
            }
            "type" => {
                let name_i = i + 1;
                if !self.is_id(name_i) {
                    return None;
                }
                let end = self.semi_end(name_i + 1);
                let first = self.mods_back(i, is_rust_mod);
                let (parent, prefix) = self.ctx();
                let name = self.tx(name_i).to_string();
                let qual = join(&prefix, &name);
                let sig_end = if self.p(end, ";") { self.toks[end].s } else { self.toks[end].e };
                let end_line = self.toks[end].end_line;
                self.add_sym(New { name, kind: SymKind::TypeAlias, first, sig_end, end_line, body: None, bases: Vec::new(), qual, parent });
                self.skip[name_i] = true;
                Some(end + 1)
            }
            "use" => {
                let line = self.toks[i].line;
                let end = self.semi_end(i + 1);
                let to = if self.p(end, ";") { end } else { end + 1 };
                let mut found = Vec::new();
                self.use_tree(i + 1, to, &mut Vec::new(), &mut found, 0);
                for (module, name, alias) in found {
                    self.imports.push(Import { module, name, alias, level: 0, line });
                }
                Some(end + 1)
            }
            "extern" if self.id(i + 1, "crate") && self.is_id(i + 2) => {
                let name = self.tx(i + 2).to_string();
                let mut alias = name.clone();
                if self.id(i + 3, "as") && self.is_id(i + 4) {
                    alias = self.tx(i + 4).to_string();
                }
                self.imports.push(Import { module: name, name: String::new(), alias, level: 0, line: self.toks[i].line });
                let end = self.semi_end(i + 1);
                Some(end + 1)
            }
            _ => None,
        }
    }

    /// A type as `impl` names it: the last identifier of its path, the text as written,
    /// and the token after it.
    fn rust_type(&self, mut p: usize) -> Option<(String, String, usize)> {
        let start = p;
        loop {
            if self.p(p, "&") || self.p(p, "&&") || self.p(p, "*") {
                p += 1;
            } else if self.p(p, "'") && self.is_id(p + 1) {
                p += 2;
            } else if self.id(p, "mut") || self.id(p, "dyn") || self.id(p, "const") || self.id(p, "unsafe") {
                p += 1;
            } else {
                break;
            }
        }
        let name;
        if self.p(p, "[") || self.p(p, "(") {
            let m = self.mate[p];
            if m == NONE {
                return None;
            }
            name = fold(&self.text[self.toks[p].s..self.toks[m].e]);
            p = m + 1;
        } else if self.is_id(p) {
            let mut last = p;
            p += 1;
            while self.p(p, "::") && self.is_id(p + 1) {
                last = p + 1;
                p += 2;
            }
            name = self.tx(last).to_string();
            if let Some(a) = self.skip_angles(p) {
                p = a;
            }
        } else {
            return None;
        }
        let text = fold(&self.text[self.toks[start].s..self.toks[p - 1].e]);
        Some((name, text, p))
    }

    /// The names a `use` declaration brings in, as (module, name, alias). `depth` is how many
    /// groups of braces the tree is inside of; a tree nested deeper than `MAX_USE_DEPTH` is left
    /// out, so that the recursion cannot use up the stack on a file that is nothing but braces.
    fn use_tree(&self, from: usize, to: usize, prefix: &mut Vec<String>, out: &mut Vec<(String, String, String)>, depth: usize) {
        let base = prefix.len();
        let mut j = from;
        if self.p(j, "::") {
            j += 1;
        }
        let mut alias: Option<String> = None;
        while j < to {
            if self.is_id(j) {
                prefix.push(self.tx(j).to_string());
                j += 1;
                if self.id(j, "as") && j + 1 < to {
                    alias = Some(self.tx(j + 1).to_string());
                    break;
                }
                if self.p(j, "::") {
                    j += 1;
                    continue;
                }
                break;
            } else if self.p(j, "*") {
                out.push((prefix.join("::"), "*".to_string(), "*".to_string()));
                prefix.truncate(base);
                return;
            } else if self.p(j, "{") && self.mate[j] != NONE {
                if depth >= MAX_USE_DEPTH {
                    prefix.truncate(base);
                    return;
                }
                let close = self.mate[j];
                let mut item = j + 1;
                let mut k = j + 1;
                while k <= close {
                    if k == close || self.p(k, ",") {
                        if k > item {
                            self.use_tree(item, k, prefix, out, depth + 1);
                        }
                        item = k + 1;
                    } else if self.p(k, "{") && self.mate[k] != NONE {
                        k = self.mate[k];
                    }
                    k += 1;
                }
                prefix.truncate(base);
                return;
            } else {
                break;
            }
        }
        if prefix.len() > base && prefix.last().map(String::as_str) == Some("self") {
            prefix.pop();
        }
        if let Some(last) = prefix.last().cloned() {
            let alias = alias.unwrap_or_else(|| last.clone());
            if prefix.len() == 1 {
                out.push((last, String::new(), alias));
            } else {
                out.push((prefix[..prefix.len() - 1].join("::"), last, alias));
            }
        }
        prefix.truncate(base);
    }

    // ----- Go -----

    fn decl_go(&mut self, i: usize) -> Option<usize> {
        if !self.is_id(i) {
            return None;
        }
        // A method specification inside an interface body.
        if matches!(self.frames.last(), Some(Frame { kind: FKind::Type(SymKind::Interface), .. }))
            && self.p(i + 1, "(")
            && (self.first_on_line(i) || self.p(i - 1, "{"))
        {
            let close = self.mate[i + 1];
            if close != NONE {
                let last = self.line_last(close);
                let (parent, prefix) = self.ctx();
                let name = self.tx(i).to_string();
                let qual = join(&prefix, &name);
                self.add_sym(New {
                    name,
                    kind: SymKind::Method,
                    first: i,
                    sig_end: self.toks[last].e,
                    end_line: self.toks[last].end_line,
                    body: None,
                    bases: Vec::new(),
                    qual,
                    parent,
                });
                self.mute(i, last);
                return Some(last + 1);
            }
        }
        match self.tx(i) {
            "func" => {
                let k = i + 1;
                let name_i;
                let mut recv: Option<String> = None;
                if self.p(k, "(") {
                    let close = self.mate[k];
                    if close == NONE {
                        return None;
                    }
                    name_i = close + 1;
                    if !self.is_id(name_i) || !(self.p(name_i + 1, "(") || self.p(name_i + 1, "[")) {
                        return None;
                    }
                    let mut last_id = None;
                    let mut j = k + 1;
                    while j < close {
                        if self.p(j, "[") && self.mate[j] != NONE {
                            j = self.mate[j] + 1;
                            continue;
                        }
                        if self.is_id(j) {
                            last_id = Some(j);
                        }
                        j += 1;
                    }
                    recv = last_id.map(|j| self.tx(j).to_string());
                } else if self.is_id(k) && (self.p(k + 1, "(") || self.p(k + 1, "[")) {
                    name_i = k;
                } else {
                    return None;
                }
                let mut p = name_i + 1;
                if self.p(p, "[") && self.mate[p] != NONE {
                    p = self.mate[p] + 1;
                }
                if !self.p(p, "(") {
                    return None;
                }
                let name = self.tx(name_i).to_string();
                let qual = recv.as_ref().map(|r| format!("{r}.{name}"));
                self.function_at(i, name, name_i, p, recv.is_some(), qual)
            }
            "type" => {
                if self.p(i + 1, "(") {
                    let close = self.mate[i + 1];
                    if close == NONE {
                        return None;
                    }
                    let mut j = i + 2;
                    while j < close {
                        if self.is_id(j) && (self.first_on_line(j) || self.p(j - 1, "(") || self.p(j - 1, ";")) {
                            if let Some(next) = self.go_type_spec(j, j, false) {
                                j = next.max(j + 1);
                                continue;
                            }
                        }
                        j += 1;
                    }
                    return Some(close + 1);
                }
                if self.is_id(i + 1) {
                    return self.go_type_spec(i + 1, i, true);
                }
                None
            }
            "import" => {
                let line = self.toks[i].line;
                if self.p(i + 1, "(") {
                    let close = self.mate[i + 1];
                    if close == NONE {
                        return None;
                    }
                    for k in i + 2..close {
                        if self.toks[k].k == K::Str {
                            let at = self.toks[k].line;
                            self.go_import(k, at);
                        }
                    }
                    return Some(close + 1);
                }
                let mut k = i + 1;
                while k < self.n() && k < i + 4 && self.toks[k].k != K::Str {
                    k += 1;
                }
                if self.k(k) == Some(K::Str) {
                    self.go_import(k, line);
                    return Some(k + 1);
                }
                None
            }
            _ => None,
        }
    }

    fn go_import(&mut self, str_tok: usize, line: u32) {
        let module = self.str_body(str_tok);
        let mut alias = String::new();
        if str_tok > 0 && self.toks[str_tok - 1].line == self.toks[str_tok].line {
            let prev = str_tok - 1;
            if self.is_id(prev) && !self.id(prev, "import") || self.p(prev, ".") {
                alias = self.tx(prev).to_string();
            }
        }
        if alias.is_empty() {
            let mut parts = module.rsplit('/');
            let last = parts.next().unwrap_or("");
            let is_version = last.len() > 1 && last.starts_with('v') && last[1..].bytes().all(|c| c.is_ascii_digit());
            alias = if is_version { parts.next().unwrap_or(last).to_string() } else { last.to_string() };
        }
        self.imports.push(Import { module, name: String::new(), alias, level: 0, line });
    }

    /// One `type` specification whose name is at `j`; `first` is the token that starts it.
    fn go_type_spec(&mut self, j: usize, first: usize, enter_body: bool) -> Option<usize> {
        let name = self.tx(j).to_string();
        let mut p = j + 1;
        if self.p(p, "[") && self.mate[p] != NONE {
            let m = self.mate[p];
            let params = self.is_id(p + 1) && (self.is_id(p + 2) || self.p(p + 2, ",") || self.p(p + 2, "~") || self.p(p + 2, "*")) && m > p + 2;
            if params {
                p = m + 1;
            }
        }
        if self.p(p, "=") {
            p += 1;
        }
        let (parent, prefix) = self.ctx();
        let qual = join(&prefix, &name);
        let shape = if self.id(p, "struct") && self.p(p + 1, "{") {
            Some((SymKind::Struct, p + 1))
        } else if self.id(p, "interface") && self.p(p + 1, "{") {
            Some((SymKind::Interface, p + 1))
        } else {
            None
        };
        if let Some((kind, open)) = shape {
            let close = self.close_tok(open);
            let sym = self.add_sym(New {
                name,
                kind,
                first,
                sig_end: self.toks[open].s,
                end_line: self.close_line(open),
                body: Some((open, close)),
                bases: Vec::new(),
                qual: qual.clone(),
                parent,
            });
            self.skip[j] = true;
            if enter_body {
                return Some(self.enter(open, Some(sym), qual, FKind::Type(kind)));
            }
            return Some(close + 1);
        }
        let end = self.stmt_end(p);
        let last = if self.p(end, ";") && end > p { end - 1 } else { end };
        self.add_sym(New {
            name,
            kind: SymKind::TypeAlias,
            first,
            sig_end: self.toks[last].e,
            end_line: self.toks[last].end_line,
            body: None,
            bases: Vec::new(),
            qual,
            parent,
        });
        self.skip[j] = true;
        Some(end + 1)
    }

    // ----- Java, C#, C and C++ -----

    fn decl_typed(&mut self, i: usize) -> Option<usize> {
        let lang = self.lang;
        if self.toks[i].k == K::P {
            let at_statement = i == 0 || matches!(self.tx(i - 1), ";" | "{" | "}");
            if self.p(i, "@") && lang == Lang::Java && self.is_id(i + 1) && !self.id(i + 1, "interface") {
                let mut j = i + 1;
                while self.p(j + 1, ".") && self.is_id(j + 2) {
                    j += 2;
                }
                if self.p(j + 1, "(") && self.mate[j + 1] != NONE {
                    j = self.mate[j + 1];
                }
                self.mute(i, j);
                return Some(j + 1);
            }
            if self.p(i, "[") && lang == Lang::CSharp && at_statement && !self.in_func() && self.mate[i] != NONE {
                let m = self.mate[i];
                self.mute(i, m);
                return Some(m + 1);
            }
            return None;
        }
        if self.toks[i].k != K::Id {
            return None;
        }
        let w = self.tx(i);
        let after_sep = i > 0 && (self.p(i - 1, ".") || self.p(i - 1, "->") || (self.p(i - 1, "::") && lang != Lang::Cpp));
        if after_sep {
            return None;
        }
        if !self.in_func() {
            match (lang, w) {
                (Lang::Java, "import") => return self.jvm_import(i),
                (Lang::CSharp | Lang::Cpp, "using") => return self.using_statement(i),
                (Lang::CSharp | Lang::Cpp, "namespace") => return self.namespace_at(i),
                _ => {}
            }
        }
        if type_kind(lang, w).is_some() {
            if let Some(next) = self.decl_type(i) {
                return Some(next);
            }
        }
        if lang == Lang::CSharp && w == "record" && (self.id(i + 1, "struct") || self.id(i + 1, "class")) {
            return self.decl_type(i);
        }
        if self.in_func() {
            return None;
        }
        if matches!(lang, Lang::Cpp | Lang::CSharp) && w == "operator" {
            let mut j = i + 1;
            while j < self.n() && j < i + 4 && !self.p(j, "(") {
                j += 1;
            }
            if self.p(j, "(") {
                let name = fold(&self.text[self.toks[i].s..self.toks[j - 1].e]);
                return self.typed_method(i, i, j, name);
            }
            return None;
        }
        if is_keyword(lang, w) {
            return None;
        }
        if self.p(i + 1, "(") {
            return self.typed_method(i, i, i + 1, w.to_string());
        }
        if matches!(lang, Lang::CSharp | Lang::Cpp) && self.p(i + 1, "<") {
            if let Some(a) = self.skip_angles(i + 1) {
                if self.p(a, "(") {
                    return self.typed_method(i, i, a, w.to_string());
                }
            }
        }
        None
    }

    /// A class, struct, interface, enum, trait or extension in a language that spells them
    /// with a keyword and a name.
    fn decl_type(&mut self, i: usize) -> Option<usize> {
        let lang = self.lang;
        let w = self.tx(i);
        let kind = type_kind(lang, w).or_else(|| if lang == Lang::CSharp && w == "record" { Some(SymKind::Class) } else { None })?;
        let kind = if lang == Lang::CSharp && w == "record" && self.id(i + 1, "struct") { SymKind::Struct } else { kind };
        let mut name_i = i + 1;
        if lang == Lang::Cpp && w == "enum" && (self.id(name_i, "class") || self.id(name_i, "struct")) {
            name_i += 1;
        }
        if lang == Lang::CSharp && w == "record" && (self.id(name_i, "class") || self.id(name_i, "struct")) {
            name_i += 1;
        }
        let typedef = i > 0 && self.id(i - 1, "typedef");
        let (name, name_tok, after) = if matches!(lang, Lang::C | Lang::Cpp) && self.p(name_i, "{") {
            let m = self.mate[name_i];
            if !typedef || m == NONE || !self.is_id(m + 1) {
                return None;
            }
            (self.tx(m + 1).to_string(), m + 1, name_i)
        } else if self.is_id(name_i) && !reserved_type_name(self.tx(name_i)) {
            let mut a = name_i + 1;
            if let Some(g) = self.skip_angles(a) {
                a = g;
            }
            (self.tx(name_i).to_string(), name_i, a)
        } else if lang == Lang::Kotlin && w == "object" && self.p(name_i, "{") && i > 0 && self.id(i - 1, "companion") {
            ("Companion".to_string(), i, name_i)
        } else {
            return None;
        };
        if matches!(lang, Lang::C | Lang::Cpp) {
            let ok = self.p(after, "{") || (lang == Lang::Cpp && (self.p(after, ":") || self.id(after, "final")));
            if !ok {
                return None;
            }
        }
        let first = {
            let at = if w == "interface" && lang == Lang::Java && i > 0 && self.p(i - 1, "@") { i - 1 } else { i };
            self.mods_back(at, is_decl_mod)
        };
        match self.find_body(after) {
            Body::Braces(b) => self.container_at(first, name, name_tok, kind, after, Some(b), b),
            Body::Nothing if lang == Lang::Kotlin => {
                let last = self.line_last(name_tok.max(after.saturating_sub(1)));
                self.container_at(first, name, name_tok, kind, after, None, last)
            }
            _ => None,
        }
    }

    /// `namespace A.B { ... }`, `namespace A::B { ... }` and the file-scoped `namespace A;`.
    fn namespace_at(&mut self, i: usize) -> Option<usize> {
        let mut j = i + 1;
        let mut name = String::new();
        while self.is_id(j) {
            name.push_str(self.tx(j));
            j += 1;
            if (self.p(j, ".") || self.p(j, "::")) && self.is_id(j + 1) {
                name.push('.');
                j += 1;
            } else {
                break;
            }
        }
        let (_, prefix) = self.ctx();
        let qual = if name.is_empty() { prefix } else { join(&prefix, &name) };
        match self.find_body_nl(j, false) {
            Body::Braces(b) if !self.p(j, "=") => Some(self.enter(b, None, qual, FKind::Ns)),
            Body::Semi(s) if self.lang == Lang::CSharp && !name.is_empty() => {
                self.push_frame(Frame { end: self.n(), sym: None, prefix: qual, kind: FKind::Ns });
                Some(s + 1)
            }
            _ => None,
        }
    }

    /// A method, constructor or function whose parameter list opens at `paren`, in a
    /// language where the type comes first. `name_first` is the first token of the name
    /// (it differs from `name_tok` only for operators).
    fn typed_method(&mut self, name_tok: usize, name_first: usize, paren: usize, name: String) -> Option<usize> {
        let lang = self.lang;
        let close = self.mate[paren];
        if close == NONE {
            return None;
        }
        let mut name = name;
        let mut c0 = name_first;
        if let Some(t) = c0.checked_sub(1) {
            if self.p(t, "~") {
                name = format!("~{name}");
                c0 = t;
            }
        }
        let mut quals: Vec<String> = Vec::new();
        if lang == Lang::Cpp {
            loop {
                if c0 >= 2 && self.p(c0 - 1, "::") {
                    if self.is_id(c0 - 2) {
                        quals.insert(0, self.tx(c0 - 2).to_string());
                        c0 -= 2;
                        continue;
                    }
                    if self.p(c0 - 2, ">") {
                        if let Some(o) = self.angle_open_back(c0 - 2) {
                            if o >= 1 && self.is_id(o - 1) {
                                quals.insert(0, self.tx(o - 1).to_string());
                                c0 = o - 1;
                                continue;
                            }
                        }
                    }
                }
                break;
            }
        }
        let prev_ok = match c0.checked_sub(1) {
            None => true,
            Some(t) => match self.toks[t].k {
                K::P => matches!(self.tx(t), ";" | "{" | "}" | ":" | ">" | "*" | "&" | "&&" | "]" | ")"),
                K::Id => !not_decl_prefix(self.tx(t)),
                _ => false,
            },
        };
        if !prev_ok {
            return None;
        }
        let start_tok = self.type_start(c0);
        let has_type = start_tok < c0 || !quals.is_empty();
        let class_name = self.frames.last().and_then(|f| f.prefix.rsplit('.').next().map(str::to_string));
        let is_ctor = self.container_kind().is_some() && class_name.as_deref() == Some(name.trim_start_matches('~'));
        let in_interface = matches!(self.frames.last(), Some(Frame { kind: FKind::Type(SymKind::Interface), .. }));
        let body = self.find_body_nl(close + 1, false);
        let (parent, prefix) = self.ctx();
        let kind = if !quals.is_empty() || self.container_kind().is_some() { SymKind::Method } else { SymKind::Function };
        let qual = if quals.is_empty() { join(&prefix, &name) } else { join(&prefix, &format!("{}.{}", quals.join("."), name)) };
        let first = start_tok.min(name_first);
        match body {
            Body::Braces(b) => {
                self.mute(paren, b - 1);
                self.skip[name_tok] = true;
                let end_line = self.close_line(b);
                let sym = self.add_sym(New {
                    name,
                    kind,
                    first,
                    sig_end: self.toks[b].s,
                    end_line,
                    body: Some((b, self.close_tok(b))),
                    bases: Vec::new(),
                    qual: qual.clone(),
                    parent,
                });
                Some(self.enter(b, Some(sym), qual, FKind::Func))
            }
            Body::Arrow(a) if has_type || is_ctor => {
                self.mute(paren, a);
                self.skip[name_tok] = true;
                let e = self.stmt_end(a + 1);
                self.add_sym(New {
                    name,
                    kind,
                    first,
                    sig_end: self.toks[a].s,
                    end_line: self.toks[e].end_line,
                    body: Some((a, e)),
                    bases: Vec::new(),
                    qual,
                    parent,
                });
                Some(e + 1)
            }
            Body::Semi(_) | Body::Eq(_) if has_type || is_ctor => {
                let last = match body {
                    Body::Semi(s) => s,
                    Body::Eq(a) => self.semi_end(a),
                    _ => close,
                };
                self.mute(paren, last);
                self.skip[name_tok] = true;
                if in_interface {
                    self.add_sym(New {
                        name,
                        kind: SymKind::Method,
                        first,
                        sig_end: self.toks[close].e,
                        end_line: self.toks[last].end_line,
                        body: None,
                        bases: Vec::new(),
                        qual,
                        parent,
                    });
                }
                Some(last + 1)
            }
            _ => None,
        }
    }

    /// The first token of the type and modifiers in front of a function name at `name_first`.
    fn type_start(&self, name_first: usize) -> usize {
        let mut j = name_first;
        while j > 0 {
            let p = j - 1;
            match self.toks[p].k {
                K::Id => {
                    if not_decl_prefix(self.tx(p)) || (p > 0 && self.p(p - 1, "@")) {
                        break;
                    }
                    j = p;
                }
                K::P => match self.tx(p) {
                    "*" | "&" | "&&" | "::" | "." | "?" => j = p,
                    ">" => match self.angle_open_back(p) {
                        Some(o) if !(o > 0 && self.id(o - 1, "template")) => j = o,
                        _ => break,
                    },
                    "]" => {
                        if p > 0 && self.p(p - 1, "[") {
                            j = p - 1;
                        } else {
                            break;
                        }
                    }
                    _ => break,
                },
                _ => break,
            }
        }
        j
    }

    /// Imports of the JVM languages: `import a.b.C;`, `import static a.B.m;`, `import a.*;`,
    /// and Kotlin's `import a.b.C as D`.
    fn jvm_import(&mut self, i: usize) -> Option<usize> {
        let line = self.toks[i].line;
        let mut j = i + 1;
        if self.id(j, "static") {
            j += 1;
        }
        let mut path: Vec<String> = Vec::new();
        while self.is_id(j) || self.p(j, "*") {
            path.push(self.tx(j).to_string());
            j += 1;
            if self.p(j, ".") {
                j += 1;
            } else {
                break;
            }
        }
        if path.is_empty() {
            return None;
        }
        let mut alias: Option<String> = None;
        if self.id(j, "as") && self.is_id(j + 1) {
            alias = Some(self.tx(j + 1).to_string());
            j += 2;
        }
        let name = path.pop().unwrap_or_default();
        let alias = alias.unwrap_or_else(|| name.clone());
        self.imports.push(Import { module: path.join("."), name, alias, level: 0, line });
        Some(j)
    }

    /// `using` in C# and C++.
    fn using_statement(&mut self, i: usize) -> Option<usize> {
        let line = self.toks[i].line;
        if self.p(i + 1, "(") || self.id(i + 1, "var") {
            return None;
        }
        let end = self.semi_end(i + 1);
        let mut j = i + 1;
        if self.id(j, "static") {
            j += 1;
        }
        let namespace_kw = self.id(j, "namespace");
        if namespace_kw {
            j += 1;
        }
        // `using Alias = A.B.C;`
        if self.is_id(j) && self.p(j + 1, "=") {
            let alias = self.tx(j).to_string();
            let to = if self.p(end, ";") { self.toks[end].s } else { self.toks[end].e };
            let from = self.toks[(j + 2).min(end)].s;
            let target = self.text[from.min(to)..to].trim().to_string();
            if self.lang == Lang::CSharp {
                self.imports.push(Import { module: target, name: String::new(), alias, level: 0, line });
            }
            return Some(end + 1);
        }
        let mut path: Vec<String> = Vec::new();
        while j < end && (self.is_id(j) || self.p(j, ".") || self.p(j, "::")) {
            if self.is_id(j) {
                path.push(self.tx(j).to_string());
            }
            j += 1;
        }
        if path.is_empty() {
            return Some(end + 1);
        }
        if self.lang == Lang::Cpp {
            if namespace_kw {
                self.imports.push(Import { module: path.join("::"), name: "*".to_string(), alias: "*".to_string(), level: 0, line });
            } else if path.len() > 1 {
                let name = path.pop().unwrap_or_default();
                self.imports.push(Import { module: path.join("::"), name: name.clone(), alias: name, level: 0, line });
            }
        } else {
            let alias = path.last().cloned().unwrap_or_default();
            self.imports.push(Import { module: path.join("."), name: String::new(), alias, level: 0, line });
        }
        Some(end + 1)
    }

    // ----- JavaScript and TypeScript -----

    fn decl_js(&mut self, i: usize) -> Option<usize> {
        let private_member = self.p(i, "#") && self.is_id(i + 1);
        if !(self.is_id(i) || private_member) || (i > 0 && (self.p(i - 1, ".") || self.p(i - 1, "?."))) {
            return None;
        }
        let ts = is_ts(self.lang);
        if let Some(Frame { kind: FKind::Type(k), .. }) = self.frames.last() {
            let k = *k;
            if matches!(k, SymKind::Class | SymKind::Interface) {
                if let Some(next) = self.js_member(i, k) {
                    return Some(next);
                }
            }
        }
        if private_member {
            return None;
        }
        // A method written in an object literal: `{ name(a) { ... }, get x() { ... } }`.
        if self.p(i + 1, "(") && self.mate[i + 1] != NONE && !is_keyword(self.lang, self.tx(i)) {
            let mut s = i;
            while s > 0
                && (self.p(s - 1, "*") || (self.is_id(s - 1) && matches!(self.tx(s - 1), "get" | "set" | "async" | "static")))
            {
                s -= 1;
            }
            let close = self.mate[i + 1];
            if s > 0
                && (self.p(s - 1, "{") || self.p(s - 1, ","))
                && (self.p(close + 1, "{") || self.p(close + 1, ":"))
                && matches!(self.find_body_nl(close + 1, false), Body::Braces(_))
            {
                return self.function_at(s, self.tx(i).to_string(), i, i + 1, false, None);
            }
        }
        match self.tx(i) {
            "import" => self.js_import(i),
            "function" => {
                let mut k = i + 1;
                if self.p(k, "*") {
                    k += 1;
                }
                if !self.is_id(k) {
                    return None;
                }
                let mut p = k + 1;
                if let Some(a) = self.skip_angles(p) {
                    p = a;
                }
                if !self.p(p, "(") {
                    return None;
                }
                let first = self.mods_back(i, is_js_mod);
                self.function_at(first, self.tx(k).to_string(), k, p, false, None)
            }
            "class" => {
                let name_i = i + 1;
                if !self.is_id(name_i) || matches!(self.tx(name_i), "extends" | "implements") {
                    return None;
                }
                let mut after = name_i + 1;
                if let Some(a) = self.skip_angles(after) {
                    after = a;
                }
                let Body::Braces(b) = self.find_body_nl(after, false) else { return None };
                let first = self.mods_back(i, is_js_mod);
                self.container_at(first, self.tx(name_i).to_string(), name_i, SymKind::Class, after, Some(b), b)
            }
            "interface" | "enum" if ts => {
                let name_i = i + 1;
                if !self.is_id(name_i) {
                    return None;
                }
                let mut after = name_i + 1;
                if let Some(a) = self.skip_angles(after) {
                    after = a;
                }
                let Body::Braces(b) = self.find_body_nl(after, false) else { return None };
                let kind = if self.tx(i) == "enum" { SymKind::Enum } else { SymKind::Interface };
                let first = self.mods_back(i, is_js_mod);
                self.container_at(first, self.tx(name_i).to_string(), name_i, kind, after, Some(b), b)
            }
            "type" if ts => {
                let name_i = i + 1;
                if !self.is_id(name_i) {
                    return None;
                }
                let mut after = name_i + 1;
                if let Some(a) = self.skip_angles(after) {
                    after = a;
                }
                if !self.p(after, "=") {
                    return None;
                }
                let end = self.stmt_end(after + 1);
                let last = if self.p(end, ";") && end > after + 1 { end - 1 } else { end };
                let first = self.mods_back(i, is_js_mod);
                let (parent, prefix) = self.ctx();
                let name = self.tx(name_i).to_string();
                let qual = join(&prefix, &name);
                self.add_sym(New {
                    name,
                    kind: SymKind::TypeAlias,
                    first,
                    sig_end: self.toks[last].e,
                    end_line: self.toks[last].end_line,
                    body: None,
                    bases: Vec::new(),
                    qual,
                    parent,
                });
                Some(end + 1)
            }
            "namespace" | "module" if ts => {
                let mut j = i + 1;
                let mut name = String::new();
                if self.k(j) == Some(K::Str) {
                    j += 1;
                } else {
                    while self.is_id(j) {
                        name.push_str(self.tx(j));
                        j += 1;
                        if self.p(j, ".") && self.is_id(j + 1) {
                            name.push('.');
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if name.is_empty() {
                        return None;
                    }
                }
                if !self.p(j, "{") || self.mate[j] == NONE {
                    return None;
                }
                let (_, prefix) = self.ctx();
                let qual = if name.is_empty() { prefix } else { join(&prefix, &name) };
                Some(self.enter(j, None, qual, FKind::Ns))
            }
            "const" | "let" | "var" => self.js_binding(i),
            "require" => {
                if !(self.p(i + 1, "(") && self.k(i + 2) == Some(K::Str) && self.p(i + 3, ")")) {
                    return None;
                }
                let module = self.str_body(i + 2);
                let line = self.toks[i].line;
                let default_alias = module.rsplit('/').next().unwrap_or("").to_string();
                if i >= 2 && self.p(i - 1, "=") && self.is_id(i - 2) {
                    let alias = self.tx(i - 2).to_string();
                    self.imports.push(Import { module, name: String::new(), alias, level: 0, line });
                } else if i >= 2 && self.p(i - 1, "=") && self.p(i - 2, "}") && self.mate[i - 2] != NONE {
                    let open = self.mate[i - 2];
                    let mut j = open + 1;
                    while j < i - 2 {
                        if self.is_id(j) {
                            let name = self.tx(j).to_string();
                            let mut alias = name.clone();
                            if self.p(j + 1, ":") && self.is_id(j + 2) {
                                alias = self.tx(j + 2).to_string();
                                j += 2;
                            }
                            self.imports.push(Import { module: module.clone(), name, alias, level: 0, line });
                        }
                        j += 1;
                    }
                } else {
                    self.imports.push(Import { module, name: String::new(), alias: default_alias, level: 0, line });
                }
                Some(i + 4)
            }
            _ => None,
        }
    }

    /// `const NAME = (...) => ...` and `const NAME = function (...) {...}`.
    fn js_binding(&mut self, i: usize) -> Option<usize> {
        let name_i = i + 1;
        if !self.is_id(name_i) {
            return None;
        }
        let mut j = name_i + 1;
        let mut eq = NONE;
        while j < self.n() && j < name_i + 80 {
            if self.toks[j].k == K::P {
                match self.tx(j) {
                    "=" => {
                        eq = j;
                        break;
                    }
                    ";" | "," | "}" | ")" => break,
                    "(" | "[" | "{" => {
                        let m = self.mate[j];
                        if m == NONE {
                            break;
                        }
                        j = m + 1;
                        continue;
                    }
                    _ => {}
                }
            }
            if j > name_i + 1 && self.toks[j].line > self.toks[j - 1].end_line && !self.joins(j) {
                break;
            }
            j += 1;
        }
        if eq == NONE {
            return None;
        }
        let mut k = eq + 1;
        if self.id(k, "async") {
            k += 1;
        }
        let name = self.tx(name_i).to_string();
        let first = self.mods_back(i, is_js_mod);
        if self.id(k, "function") {
            let mut m = k + 1;
            if self.p(m, "*") {
                m += 1;
            }
            if self.is_id(m) {
                m += 1;
            }
            if let Some(a) = self.skip_angles(m) {
                m = a;
            }
            if self.p(m, "(") {
                return self.function_at(first, name, name_i, m, false, None);
            }
            return None;
        }
        let arrow = self.js_arrow(k)?;
        self.arrow_at(first, name, name_i, arrow, false)
    }

    /// The `=>` of an arrow function that starts at `k`, if one does.
    fn js_arrow(&self, k: usize) -> Option<usize> {
        let mut m = k;
        if let Some(a) = self.skip_angles(m) {
            m = a;
        }
        if self.p(m, "(") {
            let close = self.mate[m];
            if close == NONE {
                return None;
            }
            let a = close + 1;
            if self.p(a, "=>") {
                return Some(a);
            }
            if self.p(a, ":") {
                let mut j = a + 1;
                while j < self.n() && j < a + 80 {
                    if self.p(j, "=>") {
                        return Some(j);
                    }
                    if self.p(j, ";") || self.p(j, "=") {
                        return None;
                    }
                    if self.toks[j].k == K::P && matches!(self.tx(j), "(" | "[" | "{") {
                        let mm = self.mate[j];
                        if mm == NONE {
                            return None;
                        }
                        j = mm + 1;
                        continue;
                    }
                    j += 1;
                }
            }
            None
        } else if self.is_id(m) && self.p(m + 1, "=>") {
            Some(m + 1)
        } else {
            None
        }
    }

    /// Adds the function whose arrow is at `a`, and enters its body when it has braces.
    fn arrow_at(&mut self, first: usize, name: String, name_tok: usize, a: usize, force_method: bool) -> Option<usize> {
        let (parent, prefix) = self.ctx();
        let kind = if force_method || self.container_kind().is_some() { SymKind::Method } else { SymKind::Function };
        let qual = join(&prefix, &name);
        self.skip[name_tok] = true;
        let b = a + 1;
        if self.p(b, "{") && self.mate[b] != NONE {
            let sym = self.add_sym(New {
                name,
                kind,
                first,
                sig_end: self.toks[a].s,
                end_line: self.close_line(b),
                body: Some((b, self.close_tok(b))),
                bases: Vec::new(),
                qual: qual.clone(),
                parent,
            });
            return Some(self.enter(b, Some(sym), qual, FKind::Func));
        }
        let e = self.stmt_end(a + 1);
        self.add_sym(New {
            name,
            kind,
            first,
            sig_end: self.toks[a].s,
            end_line: self.toks[e].end_line,
            body: Some((a, e)),
            bases: Vec::new(),
            qual,
            parent,
        });
        Some(e + 1)
    }

    /// A method or property function inside a class or interface body.
    fn js_member(&mut self, i: usize, ckind: SymKind) -> Option<usize> {
        let ok_prev = i == 0 || self.first_on_line(i) || (self.toks[i - 1].k == K::P && matches!(self.tx(i - 1), "{" | "}" | ";" | ")" | "]"));
        if !ok_prev {
            return None;
        }
        let mut j = i;
        while self.is_id(j)
            && matches!(self.tx(j), "static" | "async" | "get" | "set" | "public" | "private" | "protected" | "readonly" | "abstract" | "override" | "declare" | "accessor")
        {
            let nxt = j + 1;
            let name_follows = self.is_id(nxt) || self.p(nxt, "*") || self.p(nxt, "#") || self.k(nxt) == Some(K::Str);
            if !name_follows {
                break;
            }
            j += 1;
        }
        if self.p(j, "*") {
            j += 1;
        }
        let mut name = String::new();
        if self.p(j, "#") && self.is_id(j + 1) {
            name.push('#');
            j += 1;
        }
        if !self.is_id(j) {
            return None;
        }
        let name_i = j;
        name.push_str(self.tx(j));
        let mut p = name_i + 1;
        if self.p(p, "?") || self.p(p, "!") {
            p += 1;
        }
        if let Some(a) = self.skip_angles(p) {
            p = a;
        }
        if self.p(p, "(") {
            let close = self.mate[p];
            if close == NONE {
                return None;
            }
            if ckind == SymKind::Interface {
                let mut last = self.line_last(close);
                if self.p(last + 1, ";") || self.p(last + 1, ",") {
                    last += 1;
                }
                let (parent, prefix) = self.ctx();
                let qual = join(&prefix, &name);
                self.add_sym(New {
                    name,
                    kind: SymKind::Method,
                    first: i,
                    sig_end: self.toks[close].e,
                    end_line: self.toks[last].end_line,
                    body: None,
                    bases: Vec::new(),
                    qual,
                    parent,
                });
                self.mute(name_i, last);
                return Some(last + 1);
            }
            return self.function_at(i, name, name_i, p, true, None);
        }
        if ckind != SymKind::Class || !(self.p(p, "=") || self.p(p, ":")) {
            return None;
        }
        let mut eq = p;
        if self.p(p, ":") {
            eq = NONE;
            let mut k = p + 1;
            while k < self.n() && k < p + 40 {
                if self.p(k, "=") {
                    eq = k;
                    break;
                }
                if self.p(k, ";") || self.p(k, "}") {
                    break;
                }
                if self.toks[k].k == K::P && matches!(self.tx(k), "(" | "[" | "{") {
                    let m = self.mate[k];
                    if m == NONE {
                        break;
                    }
                    k = m + 1;
                    continue;
                }
                k += 1;
            }
            if eq == NONE {
                return None;
            }
        }
        let mut k = eq + 1;
        if self.id(k, "async") {
            k += 1;
        }
        if self.id(k, "function") {
            let mut m = k + 1;
            if self.p(m, "*") {
                m += 1;
            }
            if self.is_id(m) {
                m += 1;
            }
            if self.p(m, "(") {
                return self.function_at(i, name, name_i, m, true, None);
            }
            return None;
        }
        let arrow = self.js_arrow(k)?;
        self.arrow_at(i, name, name_i, arrow, true)
    }

    fn js_import(&mut self, i: usize) -> Option<usize> {
        if self.p(i + 1, "(") || self.p(i + 1, ".") {
            return None;
        }
        let mut j = i + 1;
        let mut from_str = NONE;
        while j < self.n() && j < i + 400 {
            if self.toks[j].k == K::Str {
                from_str = j;
                break;
            }
            if self.p(j, ";") {
                break;
            }
            if self.p(j, "{") && self.mate[j] != NONE {
                j = self.mate[j] + 1;
                continue;
            }
            j += 1;
        }
        if from_str == NONE {
            return None;
        }
        let module = self.str_body(from_str);
        let line = self.toks[i].line;
        let mut end = from_str;
        if end > i + 1 && self.id(end - 1, "from") {
            end -= 1;
        }
        let mut j = i + 1;
        if self.id(j, "type") && ((self.is_id(j + 1) && !self.id(j + 1, "from")) || self.p(j + 1, "{") || self.p(j + 1, "*")) {
            j += 1;
        }
        let mut any = false;
        while j < end {
            if self.p(j, "*") && self.id(j + 1, "as") && self.is_id(j + 2) {
                let alias = self.tx(j + 2).to_string();
                self.imports.push(Import { module: module.clone(), name: "*".to_string(), alias, level: 0, line });
                any = true;
                j += 3;
            } else if self.p(j, "{") && self.mate[j] != NONE {
                let close = self.mate[j];
                let mut k = j + 1;
                while k < close {
                    if self.id(k, "type") && self.is_id(k + 1) && !self.id(k + 1, "as") {
                        k += 1;
                    }
                    if self.is_id(k) || self.k(k) == Some(K::Str) {
                        let name = if self.is_id(k) { self.tx(k).to_string() } else { self.str_body(k) };
                        let mut alias = name.clone();
                        if self.id(k + 1, "as") && self.is_id(k + 2) {
                            alias = self.tx(k + 2).to_string();
                            k += 2;
                        }
                        self.imports.push(Import { module: module.clone(), name, alias, level: 0, line });
                        any = true;
                    }
                    k += 1;
                }
                j = close + 1;
            } else if self.is_id(j) {
                let alias = self.tx(j).to_string();
                let name = if self.p(j + 1, "=") { String::new() } else { "default".to_string() };
                self.imports.push(Import { module: module.clone(), name, alias, level: 0, line });
                any = true;
                j += 1;
            } else {
                j += 1;
            }
        }
        if !any {
            let alias = module.rsplit('/').next().unwrap_or("").to_string();
            self.imports.push(Import { module, name: String::new(), alias, level: 0, line });
        }
        Some(from_str + 1)
    }

    // ----- PHP -----

    fn decl_php(&mut self, i: usize) -> Option<usize> {
        if self.p(i, "#") && self.p(i + 1, "[") && self.mate[i + 1] != NONE {
            let m = self.mate[i + 1];
            self.mute(i, m);
            return Some(m + 1);
        }
        if !self.is_id(i) || (i > 0 && (self.p(i - 1, "->") || self.p(i - 1, "::"))) {
            return None;
        }
        let w = self.tx(i);
        match w {
            "function" => {
                let mut k = i + 1;
                if self.p(k, "&") {
                    k += 1;
                }
                if !self.is_id(k) || !self.p(k + 1, "(") {
                    return None;
                }
                let first = self.mods_back(i, is_php_mod);
                self.function_at(first, self.tx(k).to_string(), k, k + 1, false, None)
            }
            "class" | "interface" | "trait" | "enum" => self.decl_type(i),
            "namespace" => {
                let mut j = i + 1;
                let mut name = String::new();
                while self.is_id(j) {
                    name.push_str(self.tx(j));
                    j += 1;
                    if self.p(j, "\\") && self.is_id(j + 1) {
                        name.push('.');
                        j += 1;
                    } else {
                        break;
                    }
                }
                let (_, prefix) = self.ctx();
                let qual = if name.is_empty() { prefix } else { join(&prefix, &name) };
                if self.p(j, "{") && self.mate[j] != NONE {
                    return Some(self.enter(j, None, qual, FKind::Ns));
                }
                if self.p(j, ";") && !name.is_empty() {
                    self.push_frame(Frame { end: self.n(), sym: None, prefix: qual, kind: FKind::Ns });
                    return Some(j + 1);
                }
                None
            }
            "use" => {
                let at_statement = i == 0 || self.first_on_line(i) || matches!(self.tx(i - 1), ";" | "{" | "}");
                if !at_statement || self.p(i + 1, "(") {
                    return None;
                }
                let end = self.semi_end(i + 1);
                self.php_use(i, end);
                Some(end + 1)
            }
            "require" | "require_once" | "include" | "include_once" => {
                let mut j = i + 1;
                while j < self.n() && j < i + 12 && self.toks[j].k != K::Str && !self.p(j, ";") {
                    j += 1;
                }
                if self.k(j) == Some(K::Str) {
                    let module = self.str_body(j);
                    let alias = module.rsplit('/').next().unwrap_or("").to_string();
                    self.imports.push(Import { module, name: String::new(), alias, level: 0, line: self.toks[i].line });
                    return Some(j + 1);
                }
                None
            }
            _ => None,
        }
    }

    /// `use A\B\C;`, `use A\B\C as D;`, `use A\{B, C as E};` and `use function A\f;`.
    fn php_use(&mut self, i: usize, end: usize) {
        let line = self.toks[i].line;
        let mut j = i + 1;
        if self.id(j, "function") || self.id(j, "const") {
            j += 1;
        }
        while j < end {
            let mut path: Vec<String> = Vec::new();
            loop {
                if self.p(j, "\\") {
                    j += 1;
                }
                if self.is_id(j) {
                    path.push(self.tx(j).to_string());
                    j += 1;
                    if self.p(j, "\\") {
                        continue;
                    }
                }
                break;
            }
            if self.p(j, "{") && self.mate[j] != NONE {
                let close = self.mate[j];
                let mut k = j + 1;
                while k < close {
                    let mut inner: Vec<String> = Vec::new();
                    while self.is_id(k) {
                        inner.push(self.tx(k).to_string());
                        k += 1;
                        if self.p(k, "\\") {
                            k += 1;
                        } else {
                            break;
                        }
                    }
                    let mut alias = inner.last().cloned().unwrap_or_default();
                    if self.id(k, "as") && self.is_id(k + 1) {
                        alias = self.tx(k + 1).to_string();
                        k += 2;
                    }
                    if let Some(name) = inner.pop() {
                        let mut module = path.clone();
                        module.extend(inner);
                        self.imports.push(Import { module: module.join("\\"), name, alias, level: 0, line });
                    }
                    k += 1;
                }
                j = close + 1;
            } else if let Some(name) = path.pop() {
                let mut alias = name.clone();
                if self.id(j, "as") && self.is_id(j + 1) {
                    alias = self.tx(j + 1).to_string();
                    j += 2;
                }
                self.imports.push(Import { module: path.join("\\"), name, alias, level: 0, line });
            }
            if self.p(j, ",") {
                j += 1;
            } else {
                break;
            }
        }
    }

    // ----- Kotlin -----

    /// The last token of the annotation whose `@` is at `i`: `@Foo`, `@Foo(1)` or `@file:Foo("x")`.
    fn annotation_end(&self, i: usize) -> usize {
        let mut j = i + 1;
        while (self.p(j + 1, ".") || self.p(j + 1, ":")) && self.is_id(j + 2) {
            j += 2;
        }
        if self.p(j + 1, "(") && self.toks[j + 1].line == self.toks[j].end_line && self.mate[j + 1] != NONE {
            j = self.mate[j + 1];
        }
        j
    }

    fn decl_kotlin(&mut self, i: usize) -> Option<usize> {
        if self.p(i, "@") && self.is_id(i + 1) {
            let j = self.annotation_end(i);
            self.mute(i, j);
            return Some(j + 1);
        }
        if !self.is_id(i) || (i > 0 && (self.p(i - 1, ".") || self.p(i - 1, "::") || self.p(i - 1, "?."))) {
            return None;
        }
        match self.tx(i) {
            "import" if !self.in_func() => self.jvm_import(i),
            "fun" => {
                if self.id(i + 1, "interface") {
                    return None;
                }
                let mut k = i + 1;
                if let Some(a) = self.skip_angles(k) {
                    k = a;
                }
                let mut last = NONE;
                let mut j = k;
                while self.is_id(j) {
                    last = j;
                    j += 1;
                    if let Some(a) = self.skip_angles(j) {
                        j = a;
                    }
                    if self.p(j, "?") {
                        j += 1;
                    }
                    if self.p(j, ".") {
                        j += 1;
                    } else {
                        break;
                    }
                }
                if last == NONE || !self.p(j, "(") {
                    return None;
                }
                let first = self.mods_back(i, is_decl_mod);
                self.function_at(first, self.tx(last).to_string(), last, j, false, None)
            }
            "constructor" if self.p(i + 1, "(") && self.container_kind().is_some() => {
                let first = self.mods_back(i, is_decl_mod);
                self.function_at(first, "constructor".to_string(), i, i + 1, true, None)
            }
            "class" | "interface" | "object" => self.decl_type(i),
            _ => None,
        }
    }

    // ----- Swift -----

    fn decl_swift(&mut self, i: usize) -> Option<usize> {
        if self.p(i, "@") && self.is_id(i + 1) {
            let j = self.annotation_end(i);
            self.mute(i, j);
            return Some(j + 1);
        }
        if !self.is_id(i) || (i > 0 && (self.p(i - 1, ".") || self.p(i - 1, "?."))) {
            return None;
        }
        match self.tx(i) {
            "import" if !self.in_func() => {
                let line = self.toks[i].line;
                let mut j = i + 1;
                let kinded = self.is_id(j)
                    && matches!(self.tx(j), "typealias" | "struct" | "class" | "enum" | "protocol" | "let" | "var" | "func")
                    && self.is_id(j + 1);
                if kinded {
                    j += 1;
                }
                let mut path: Vec<String> = Vec::new();
                while self.is_id(j) {
                    path.push(self.tx(j).to_string());
                    j += 1;
                    if self.p(j, ".") {
                        j += 1;
                    } else {
                        break;
                    }
                }
                if path.is_empty() {
                    return None;
                }
                if kinded && path.len() > 1 {
                    let name = path.pop().unwrap_or_default();
                    self.imports.push(Import { module: path.join("."), name: name.clone(), alias: name, level: 0, line });
                } else {
                    let alias = path.last().cloned().unwrap_or_default();
                    self.imports.push(Import { module: path.join("."), name: String::new(), alias, level: 0, line });
                }
                Some(j)
            }
            "func" => {
                let k = i + 1;
                let (name, name_tok, mut p) = if self.is_id(k) {
                    (self.tx(k).to_string(), k, k + 1)
                } else {
                    let mut j = k;
                    while j < self.n() && j < k + 4 && self.toks[j].k == K::P && !self.p(j, "(") && !self.p(j, "<") {
                        j += 1;
                    }
                    if j == k {
                        return None;
                    }
                    (self.text[self.toks[k].s..self.toks[j - 1].e].to_string(), k, j)
                };
                if let Some(a) = self.skip_angles(p) {
                    p = a;
                }
                if !self.p(p, "(") {
                    return None;
                }
                let first = self.mods_back(i, is_decl_mod);
                self.function_at(first, name, name_tok, p, false, None)
            }
            "init" | "subscript" if self.container_kind().is_some() => {
                let mut p = i + 1;
                if self.p(p, "?") || self.p(p, "!") {
                    p += 1;
                }
                if let Some(a) = self.skip_angles(p) {
                    p = a;
                }
                if !self.p(p, "(") {
                    return None;
                }
                let first = self.mods_back(i, is_decl_mod);
                self.function_at(first, self.tx(i).to_string(), i, p, true, None)
            }
            "class" | "struct" | "enum" | "protocol" | "extension" | "actor" => self.decl_type(i),
            _ => None,
        }
    }

    // ----- Shell -----

    fn decl_shell(&mut self, i: usize) -> Option<usize> {
        let command = i == 0
            || self.first_on_line(i)
            || (self.toks[i - 1].k == K::P && matches!(self.tx(i - 1), ";" | "&&" | "||" | "|" | "(" | "{" | "!" | "&"))
            || (self.is_id(i - 1) && matches!(self.tx(i - 1), "then" | "do" | "else" | "elif"));
        if !command {
            return None;
        }
        if self.p(i, ".") && self.toks.get(i + 1).map_or(false, |t| t.line == self.toks[i].line && t.s > self.toks[i].e) {
            return self.shell_source(i);
        }
        if !self.is_id(i) {
            return None;
        }
        let w = self.tx(i);
        if w == "source" {
            return self.shell_source(i);
        }
        if w == "function" && self.is_id(i + 1) {
            let name_i = i + 1;
            let mut p = name_i + 1;
            if self.p(p, "(") && self.p(p + 1, ")") {
                p += 2;
            }
            return self.shell_function(i, name_i, p);
        }
        if self.p(i + 1, "(") && self.p(i + 2, ")") {
            return self.shell_function(i, i, i + 3);
        }
        None
    }

    fn shell_function(&mut self, first: usize, name_i: usize, p: usize) -> Option<usize> {
        if !(self.p(p, "{") || self.p(p, "(")) || self.mate[p] == NONE {
            return None;
        }
        let (parent, prefix) = self.ctx();
        let name = self.tx(name_i).to_string();
        let qual = join(&prefix, &name);
        let sym = self.add_sym(New {
            name,
            kind: SymKind::Function,
            first,
            sig_end: self.toks[p - 1].e,
            end_line: self.close_line(p),
            body: Some((p, self.close_tok(p))),
            bases: Vec::new(),
            qual: qual.clone(),
            parent,
        });
        Some(self.enter(p, Some(sym), qual, FKind::Func))
    }

    fn shell_source(&mut self, i: usize) -> Option<usize> {
        let line = self.toks[i].line;
        let end = self.lines.end_of(line).min(self.text.len());
        let rest = self.text[self.toks[i].e..end].trim_start_matches([' ', '\t']);
        let word_end = rest.find(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | ')')).unwrap_or(rest.len());
        let word = rest[..word_end].trim_matches(['"', '\'']);
        if word.is_empty() {
            return None;
        }
        let alias = word.rsplit('/').next().unwrap_or(word).to_string();
        self.imports.push(Import { module: word.to_string(), name: String::new(), alias, level: 0, line });
        Some(i + 1)
    }

    // ----- Ruby -----

    /// True when a block keyword at token `i` starts a statement rather than modifying one.
    fn ruby_opens(&self, i: usize) -> bool {
        if self.first_on_line(i) {
            return true;
        }
        // After an operator or an opening bracket an expression starts, so the keyword
        // opens a block; after a value it is a modifier (`run if ready`).
        let prev = i - 1;
        // The `=` that ends a symbol such as `:name=` does not start an expression.
        let setter_symbol = self.p(prev, "=")
            && prev >= 2
            && self.is_id(prev - 1)
            && self.toks[prev].s == self.toks[prev - 1].e
            && self.p(prev - 2, ":")
            && self.toks[prev - 2].e == self.toks[prev - 1].s;
        if setter_symbol {
            return false;
        }
        match self.toks[prev].k {
            K::P => !matches!(self.tx(prev), ")" | "]" | "}" | "."),
            K::Id => matches!(self.tx(prev), "then" | "else" | "do" | "begin" | "ensure" | "rescue" | "and" | "or" | "not" | "elsif"),
            _ => false,
        }
    }

    /// The name after `def` (at token `j`) and the last token of that name.
    fn ruby_def_name(&self, j: usize) -> Option<(String, usize)> {
        if self.is_id(j) {
            let mut name = self.tx(j).to_string();
            let mut last = j;
            // `def name=(value)` and `def name= value` define a setter.
            if self.p(j + 1, "=") && self.toks[j + 1].s == self.toks[j].e {
                name.push('=');
                last = j + 1;
            }
            return Some((name, last));
        }
        if self.k(j) == Some(K::P) {
            let mut last = j;
            let mut name = self.tx(j).to_string();
            while last + 1 < self.n()
                && last < j + 3
                && self.toks[last + 1].k == K::P
                && self.toks[last + 1].s == self.toks[last].e
                && !self.p(last + 1, "(")
            {
                last += 1;
                name.push_str(self.tx(last));
            }
            if name.chars().all(|c| "[]=<>!~+-*/%&|^@`".contains(c)) {
                return Some((name, last));
            }
        }
        None
    }

    fn run_ruby(&mut self) {
        struct Open {
            sym: Option<usize>,
            prefix: String,
            container: bool,
        }
        let n = self.n();
        let mut stack: Vec<Open> = Vec::new();
        let mut loop_line = 0u32;
        let mut i = 0;
        while i < n {
            if !self.is_id(i) {
                i += 1;
                continue;
            }
            let t = self.toks[i];
            let w = self.tx(i);
            let after_sep = i > 0 && (self.p(i - 1, ".") || self.p(i - 1, "::"));
            let symbol_literal = i > 0 && self.p(i - 1, ":") && self.toks[i - 1].e == t.s;
            let hash_key = self.p(i + 1, ":") && self.toks[i + 1].s == t.e;
            if after_sep || symbol_literal || hash_key {
                i += 1;
                continue;
            }
            let anon = Open { sym: None, prefix: stack.last().map(|o| o.prefix.clone()).unwrap_or_default(), container: false };
            match w {
                "def" => {
                    let mut j = i + 1;
                    let mut singleton = false;
                    if self.is_id(j) && (self.p(j + 1, ".") || self.p(j + 1, "::")) && self.toks[j + 1].s == self.toks[j].e {
                        singleton = true;
                        j += 2;
                    }
                    let Some((name, name_last)) = self.ruby_def_name(j) else {
                        stack.push(anon);
                        i += 1;
                        continue;
                    };
                    let mut header_last = name_last;
                    let p = name_last + 1;
                    if self.p(p, "(") && self.mate[p] != NONE {
                        header_last = self.mate[p];
                    } else if !self.p(p, "=") {
                        // Parameters written without parentheses run to the end of the line.
                        while header_last + 1 < n
                            && self.toks[header_last + 1].line == self.toks[name_last].end_line
                            && !self.p(header_last + 1, ";")
                        {
                            header_last += 1;
                        }
                    }
                    let endless = self.p(header_last + 1, "=") && self.toks[header_last + 1].line == self.toks[header_last].end_line;
                    let parent = stack.iter().rev().find_map(|o| o.sym);
                    let prefix = stack.last().map(|o| o.prefix.clone()).unwrap_or_default();
                    let container = stack.last().map_or(false, |o| o.container);
                    let kind = if container || singleton { SymKind::Method } else { SymKind::Function };
                    let qual = join(&prefix, &name);
                    let (end_line, body_end) = if endless {
                        let e = self.stmt_end(header_last + 2);
                        (self.toks[e].end_line, e)
                    } else {
                        (self.last_line(), n - 1)
                    };
                    let sym = self.add_sym(New {
                        name,
                        kind,
                        first: i,
                        sig_end: self.toks[header_last].e,
                        end_line,
                        body: Some((header_last, body_end)),
                        bases: Vec::new(),
                        qual: qual.clone(),
                        parent,
                    });
                    if self.is_id(j) {
                        self.skip[j] = true;
                    }
                    if endless {
                        i = body_end + 1;
                        continue;
                    }
                    stack.push(Open { sym: Some(sym), prefix: qual, container: false });
                    i = header_last + 1;
                    continue;
                }
                "class" | "module" => {
                    if w == "class" && self.p(i + 1, "<") && self.p(i + 2, "<") {
                        let container = true;
                        stack.push(Open { sym: None, prefix: anon.prefix, container });
                        i += 3;
                        continue;
                    }
                    let mut j = i + 1;
                    if self.p(j, "::") {
                        j += 1;
                    }
                    let mut path: Vec<String> = Vec::new();
                    while self.is_id(j) {
                        path.push(self.tx(j).to_string());
                        j += 1;
                        if self.p(j, "::") && self.is_id(j + 1) {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    let Some(name) = path.last().cloned() else {
                        stack.push(anon);
                        i += 1;
                        continue;
                    };
                    let mut header_last = j - 1;
                    while header_last + 1 < n
                        && self.toks[header_last + 1].line == self.toks[j - 1].end_line
                        && !self.p(header_last + 1, ";")
                    {
                        header_last += 1;
                    }
                    let mut bases = Vec::new();
                    if self.p(j, "<") && header_last > j {
                        bases.push(fold(&self.text[self.toks[j + 1].s..self.toks[header_last].e]));
                    }
                    let parent = stack.iter().rev().find_map(|o| o.sym);
                    let qual = join(&anon.prefix, &path.join("."));
                    let sym = self.add_sym(New {
                        name,
                        kind: SymKind::Class,
                        first: i,
                        sig_end: self.toks[header_last].e,
                        end_line: self.last_line(),
                        body: Some((header_last, n - 1)),
                        bases,
                        qual: qual.clone(),
                        parent,
                    });
                    stack.push(Open { sym: Some(sym), prefix: qual, container: true });
                    i = header_last + 1;
                    continue;
                }
                "end" => {
                    if let Some(open) = stack.pop() {
                        if let Some(s) = open.sym {
                            self.syms[s].end_line = t.end_line;
                            if let Some(b) = self.syms[s].body.as_mut() {
                                b.1 = i;
                            }
                        }
                    }
                }
                "do" => {
                    if loop_line == t.line {
                        loop_line = 0;
                    } else {
                        stack.push(anon);
                    }
                }
                "begin" | "case" | "if" | "unless" if self.ruby_opens(i) => stack.push(anon),
                "while" | "until" | "for" if self.ruby_opens(i) => {
                    loop_line = t.line;
                    stack.push(anon);
                }
                "require" | "require_relative" => {
                    let mut j = i + 1;
                    if self.p(j, "(") {
                        j += 1;
                    }
                    if self.k(j) == Some(K::Str) {
                        let module = self.str_body(j);
                        let alias = module.rsplit('/').next().unwrap_or("").to_string();
                        self.imports.push(Import { module, name: String::new(), alias, level: 0, line: t.line });
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::finish;
    use lxr_core::{FileFacts, ParseGrade};

    const ALL_LANGS: [Lang; 16] = [
        Lang::Python,
        Lang::JavaScript,
        Lang::Jsx,
        Lang::TypeScript,
        Lang::Tsx,
        Lang::Rust,
        Lang::Go,
        Lang::Java,
        Lang::C,
        Lang::Cpp,
        Lang::CSharp,
        Lang::Kotlin,
        Lang::Swift,
        Lang::Ruby,
        Lang::Php,
        Lang::Shell,
    ];

    fn facts(text: &str, lang: Lang) -> FileFacts {
        finish("t", lang, text, ParseGrade::Lexical, parse(text, lang))
    }

    /// One line per symbol: qualified name, kind and span.
    fn defs(f: &FileFacts) -> Vec<String> {
        f.symbols.iter().map(|s| format!("{} {} {}-{}", s.qual, s.kind.word(), s.start, s.end)).collect()
    }

    /// One line per call: caller, what it was called on, the name and the line.
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

    /// One line per import: module, name, alias, level and line, divided by bars.
    fn imports(f: &FileFacts) -> Vec<String> {
        f.imports.iter().map(|i| format!("{}|{}|{}|{}|{}", i.module, i.name, i.alias, i.level, i.line)).collect()
    }

    fn masked(text: &str, lang: Lang) -> String {
        String::from_utf8(mask(text, lang).code).expect("masking keeps the text valid UTF-8")
    }

    // The samples below are small but realistic sources. Each carries traps: braces,
    // keywords and call-like text inside comments and string literals.

    const SRC_GO_TYPES_METHODS_AND_GROUPED_IMPORTS: &str = r###"package main

import (
	"fmt"
	str "strings"
	_ "net/http/pprof"
)

import "os"

// Server handles requests. Run() is not called here.
type Server struct {
	Name string `json:"name(x)"`
	cb   func(int) int
}

type Handler interface {
	Serve(w Writer) error
	Close()
}

type ID int

type (
	Pair struct{ A, B int }
	Alias = Server
)

// Start boots the server.
func (s *Server) Start(port int) error {
	fmt.Println("start { not a brace", port)
	/* helper() in a comment */
	go s.loop()
	return s.listen(port)
}

func (s Server) Label() string {
	return str.ToUpper(s.Name)
}

func NewServer(name string) *Server {
	s := &Server{Name: name}
	defer s.Close()
	fn := func(x int) int { return x + len(name) }
	_ = fn(1)
	return s
}

func Map[T any](xs []T) []T { return xs }
"###;

    const SRC_JAVA_CLASS_INTERFACE_ENUM_AND_IMPORTS: &str = r###"package com.acme;

import java.util.List;
import java.util.*;
import static java.lang.Math.max;

/**
 * A service for users.
 */
@Service
public class UserService extends Base implements Runnable, Comparable<UserService> {
    private static final String NAME = "a { b";
    private final List<String> items = new ArrayList<>();

    @Override
    public void run() {
        System.out.println("running(" + items.size() + ")");
        helper(3);
    }

    public UserService(int x) {
        super(x);
    }

    @SuppressWarnings("unchecked")
    static <T> T pick(List<T> xs) {
        char c = '{';
        return xs.get(0);
    }
}

interface Repo {
    User find(long id);
    default void ping() { log("p"); }
}

enum Color { RED, GREEN }
"###;

    const SRC_C_FUNCTIONS_STRUCTS_AND_INCLUDES: &str = r###"#include <stdio.h>
#include "util/helpers.h"
#define SQUARE(x) ((x) * (x))

/* adds two */
static int add(int a, int b)
{
    return a + b;
}

struct point {
    int x, y;
};

typedef struct {
    int id;
} item_t;

int main(int argc, char **argv) {
    char c = '{';
    printf("%d {\n", add(1, 2));
    if (argc > 1) { puts(argv[1]); }
    return 0;
}
"###;

    const SRC_CPP_NAMESPACE_CLASS_AND_OUT_OF_CLASS_METHOD: &str = r###"#include <vector>
using namespace std;
using std::string;

namespace app {

// A widget.
class Widget : public Base, private Mixin<int> {
public:
    Widget(int w) : width(w) {}
    ~Widget();
    void draw() const;
    int area() const { return compute(width); }
    static Widget make() { return Widget(3); }
private:
    int width;
};

void Widget::draw() const {
    std::cout << "draw(" << width << ")";
    helper();
}

template <typename T>
T clamp(T v, T lo, T hi) {
    return v < lo ? lo : (v > hi ? hi : v);
}

}  // namespace app
"###;

    const SRC_CSHARP_NAMESPACE_CLASS_AND_USING: &str = r###"using System;
using System.Collections.Generic;
using Alias = Foo.Bar;

namespace Acme.Core
{
    public interface IRepo { User Find(long id); }

    /// <summary>A service.</summary>
    public class Service : Base, IRepo
    {
        private readonly string name = "x { y";
        public Service(string name) { this.name = name; }
        [HttpGet("find")]
        public User Find(long id)
        {
            var u = Lookup(id);
            return u;
        }
        public int Count => items.Count();
        public static T Pick<T>(IList<T> xs) where T : class { return xs[0]; }
        public int Twice(int x) => Double(x) * 2;
    }
}
"###;

    const SRC_KOTLIN_CLASSES_FUNCTIONS_AND_IMPORTS: &str = r###"package app

import java.util.List
import foo.bar.Baz as Qux

/** A user. */
data class User(val name: String, val id: Int)

interface Repo {
    fun find(id: Int): User?
}

class Service(private val repo: Repo) : Base(), Repo {
    override fun find(id: Int): User? {
        val msg = "found {$id}"
        println(msg)
        return repo.find(id)
    }

    fun double(x: Int) = twice(x)
}

fun String.shout(): String = this.uppercase()

fun main() {
    val s = Service(object : Repo { override fun find(id: Int) = null })
    s.find(1)
}

abstract class Shape {
    abstract fun step(n: Long)

    /** Resets the shape. */
    @JvmStatic
    open fun reset(c: Int) { step(0) }
}
"###;

    const SRC_SWIFT_PROTOCOL_STRUCT_AND_EXTENSION: &str = r###"import Foundation
import struct Swift.Int

/// A shape.
protocol Shape {
    func area() -> Double
}

struct Circle: Shape {
    var r: Double
    func area() -> Double {
        return Double.pi * r * r
    }
    init(r: Double) { self.r = r }
}

class Canvas {
    func draw(_ s: Shape) {
        print("area: \(s.area())")
        render(s)
    }
}

extension Canvas: CustomStringConvertible {
    var description: String { return "Canvas" }
}

func main() { Canvas().draw(Circle(r: 2)) }

class Button {
    @objc func tap(_ s: Shape) { render(s) }
    func later(_ n: Int)
    func two() {}
}
"###;

    const SRC_RUBY_MODULE_CLASS_AND_METHODS: &str = r###"require 'json'
require_relative "lib/util"

# A greeter module.
module Greeting
  class Greeter < Base
    def initialize(name)
      @name = name # comment with end
    end

    def greet(other = nil)
      puts("hello #{@name} end")
      other.nil? ? default_name : other
    end

    def self.build(name)
      new(name)
    end

    private

    def default_name = "x"
  end
end
"###;

    const SRC_PHP_NAMESPACE_CLASS_AND_USE: &str = r###"<?php
namespace App\Models;

use App\Base\Model;
use App\Support\{Helper, Str as S};

/**
 * A user.
 */
class User extends Model implements JsonSerializable
{
    public function name(): string
    {
        $s = 'function fake() {'; // calls: not_a_call()
        return strtoupper($this->name . $s);
    }

    public static function make(array $a): self { return new self($a); }
}

function helper($x) { return User::make($x); }
"###;

    const SRC_SHELL_FUNCTIONS_SOURCE_AND_HEREDOC: &str = r###"#!/bin/bash
source ./lib/common.sh
. "$DIR/other.sh"

# say hello
greet() {
  echo "hello ${1}"   # {
  cat <<EOF
function fake() {
EOF
}

function build {
  greet world
}
"###;

    const SRC_JAVASCRIPT_FUNCTIONS_CLASS_AND_IMPORTS: &str = r###"import fs from 'fs';
import { a, b as c } from "./mod";
import * as path from 'path';
const x = require('x');

/** Adds. */
function add(a, b) { return a + b; }

const mul = (a, b) => {
  return add(a, b) * 2; // "{"
};

class Counter extends Base {
  constructor() { super(); this.n = 0; }
  inc() { this.n++; return this; }
  static from(n) { return new Counter(); }
}

const re = /[{]+/g;
const s = `value ${fmt(1)} {`;
"###;

    const SRC_TYPESCRIPT_INTERFACE_TYPE_ENUM_AND_CLASS: &str = r###"interface Repo<T> extends Base {
  find(id: number): T;
}
type Id = string | number;
enum Color { Red, Green }
export class Svc implements Repo<User> {
  private cache = new Map<string, User>();
  find(id: number): User { return this.cache.get(String(id))!; }
}
export const handler = async (e: Event): Promise<void> => { await run(e); };
"###;

    const SRC_RUST_STRUCT_IMPL_TRAIT_AND_USE: &str = r###"use std::collections::{HashMap, HashSet as Set};
use crate::store::*;

/// A config.
#[derive(Debug)]
pub struct Config<'a> { name: &'a str }

impl<'a> Config<'a> {
    /// Parses.
    pub fn parse(text: &str) -> Config {
        let c = '{';
        let s = "fn fake() {";
        helper(text)
    }
}

pub trait Shape: Display + Clone { fn area(&self) -> f64; }
enum E { A(i32), B }
fn helper(x: &str) -> usize { x.len() }
"###;

    const SRC_PYTHON_CLASSES_CALLS_AND_IMPORTS: &str = r###"import os, sys as system
from . import util
from ..pkg.sub import (alpha as a, beta)
from x.y import *

MAX = 3

@decorator(1)
class Config(Base, metaclass=Meta):
    """Holds settings.

    More text.
    """

    def parse(self, text: str = default()) -> "Config":
        # helper(): not a call
        s = "def fake(): pass"
        return self.build(text, strip(s))

    async def load(self):
        await fetch(self.url)

def top(a,
        b=1):
    def inner(): return a
    return inner()

if True: import json
"###;

    const SRC_PYTHON_NESTING_ONE_LINERS_AND_CONTINUATIONS: &str = r###"import a.b.c as d, e
from f import (g,
               h as i,)
class A:
    x = 1
    class B:
        def m(self): pass
    def f(self, x=lambda y: y(1)):  # def not_a_def(): in comment
        s = '''
        def inside_string(): pass
        '''
        t = f"{g(1)} {{literal}}"
        return s \
            + t
    @staticmethod
    def s(): return 1

def top(): print(1); other(2)
async def agen():
    async with x as y: await z()

class Empty: pass
class WithDoc:
    '''One line.'''
x = [i for i in range(3) if cond(i)]
"###;

    const SRC_RUST_MODIFIERS_NESTED_COMMENTS_AND_RAW_STRINGS: &str = r###"pub(crate) async fn run<T: Fn(i32) -> i32>(f: T) -> Result<(), E> where T: Send { f(1); Ok(()) }
unsafe impl<T> Send for Wrapper<T> {}
impl fmt::Display for Wrapper<u8> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", 'x') }
}
mod inner { pub fn deep() -> &'static str { r#"fn raw() { "#  } }
/* outer /* nested fn hidden() { */ still comment */
extern "C" { fn ext(x: i32) -> i32; }
type Alias = Vec<u8>;
pub union U { a: u32 }
macro_rules! m { ($x:expr) => { fn gen() {} }; }
struct Tuple(i32, String);
"###;

    const SRC_RUBY_BLOCKS_MODIFIERS_AND_HEREDOC: &str = r###"class Foo
  attr_reader :x
  def initialize(x) = @x = x
  def each
    @items.each do |i|
      yield i
    end
  end
  def ok?
    return true if @x
    while @x > 0 do
      @x -= 1
    end
    case @x
    when 1 then :one
    end
    begin
      risky(1)
    rescue => e
      log(e)
    end
    x = if @x then 1 else 2 end
    text = <<~EOS
      def fake; end
    EOS
    /def (\w+)/ =~ text
  end
end
"###;

    const SRC_JSX_OBJECT_METHODS_PRIVATE_MEMBERS_AND_TEMPLATES: &str = r###"export default function App() {
  return <p>Don't {name(1)} </p>;
}
const o = { m() { return 1; }, n: function() {} };
export const f = async x => x * 2;
class K { get v() { return 1; } static async *g() {} #p() {} }
exports.h = function () {};
const t = `a${`b${c(1)}`}d`;
"###;

    const SRC_GO_GENERIC_RECEIVERS_AND_PACKAGE_LEVEL_CALLS: &str = r###"package repo

func (r *Repo[T]) Get(id int) (T, error) { return r.items[id], nil }
type Number interface { ~int | ~float64 }
const Pi = 3.14
var x = compute()
func init() { register("x") }
type Fn func(int) int
func apply(f Fn, v int) int {
	for i := range items { f(i) }
	switch v { case 1: return g(v) }
	return f(v)
}
"###;

    const SRC_JAVA_NESTED_TYPES_STATIC_BLOCK_AND_LAMBDA: &str = r###"public class Outer<T extends Comparable<T>> {
    static { init(); }
    private final Runnable r = new Runnable() {
        public void run() { go(1); }
    };
    @Deprecated(since = "1")
    public static int sum(int... xs) throws IOException, RuntimeException {
        list.forEach(x -> { use(x); });
        return xs.length;
    }
    class Inner {
        void hi() { greet("x"); }
    }
    interface Cb { static void make() { create(); } }
}
"###;

    const SRC_CPP_OPERATORS_INIT_LISTS_AND_EXTERN: &str = r###"enum class Color : int { Red, Green };
struct Vec {
    float x, y;
    Vec operator+(const Vec& o) const { return Vec{x + o.x, y + o.y}; }
    bool operator==(const Vec& o) const;
    explicit operator bool() const { return x != 0; }
};
template <class T> class Box {
public:
    Box(T v) : v_{v} {}
    T get() const { return v_; }
private:
    T v_;
};
int Vec::len2() const { return (int)(x * x + y * y); }
extern "C" int c_api(int a) { return a; }
#define LOG(x) do { puts(x); } while (0)
static void helper(void) { LOG("hi"); }
"###;

    const SRC_SHELL_NESTED_AND_SUBSHELL_FUNCTIONS: &str = r###"#!/bin/sh
a() ( echo sub )
function b() {
  inner() { echo "${#x} $(date)"; }
  inner
}
case "$1" in
  x) a ;;
esac
my-func () {
  echo '}'
}
"###;

    /// Every sample source with the language it is written in.
    fn corpus() -> Vec<(&'static str, Lang)> {
        vec![
            (SRC_GO_TYPES_METHODS_AND_GROUPED_IMPORTS, Lang::Go),
            (SRC_JAVA_CLASS_INTERFACE_ENUM_AND_IMPORTS, Lang::Java),
            (SRC_C_FUNCTIONS_STRUCTS_AND_INCLUDES, Lang::C),
            (SRC_CPP_NAMESPACE_CLASS_AND_OUT_OF_CLASS_METHOD, Lang::Cpp),
            (SRC_CSHARP_NAMESPACE_CLASS_AND_USING, Lang::CSharp),
            (SRC_KOTLIN_CLASSES_FUNCTIONS_AND_IMPORTS, Lang::Kotlin),
            (SRC_SWIFT_PROTOCOL_STRUCT_AND_EXTENSION, Lang::Swift),
            (SRC_RUBY_MODULE_CLASS_AND_METHODS, Lang::Ruby),
            (SRC_PHP_NAMESPACE_CLASS_AND_USE, Lang::Php),
            (SRC_SHELL_FUNCTIONS_SOURCE_AND_HEREDOC, Lang::Shell),
            (SRC_JAVASCRIPT_FUNCTIONS_CLASS_AND_IMPORTS, Lang::JavaScript),
            (SRC_TYPESCRIPT_INTERFACE_TYPE_ENUM_AND_CLASS, Lang::TypeScript),
            (SRC_RUST_STRUCT_IMPL_TRAIT_AND_USE, Lang::Rust),
            (SRC_PYTHON_CLASSES_CALLS_AND_IMPORTS, Lang::Python),
            (SRC_PYTHON_NESTING_ONE_LINERS_AND_CONTINUATIONS, Lang::Python),
            (SRC_RUST_MODIFIERS_NESTED_COMMENTS_AND_RAW_STRINGS, Lang::Rust),
            (SRC_RUBY_BLOCKS_MODIFIERS_AND_HEREDOC, Lang::Ruby),
            (SRC_JSX_OBJECT_METHODS_PRIVATE_MEMBERS_AND_TEMPLATES, Lang::Jsx),
            (SRC_GO_GENERIC_RECEIVERS_AND_PACKAGE_LEVEL_CALLS, Lang::Go),
            (SRC_JAVA_NESTED_TYPES_STATIC_BLOCK_AND_LAMBDA, Lang::Java),
            (SRC_CPP_OPERATORS_INIT_LISTS_AND_EXTERN, Lang::Cpp),
            (SRC_SHELL_NESTED_AND_SUBSHELL_FUNCTIONS, Lang::Shell),
        ]
    }

    #[test]
    fn go_types_methods_and_grouped_imports() {
        let f = facts(SRC_GO_TYPES_METHODS_AND_GROUPED_IMPORTS, Lang::Go);
        assert_eq!(
            defs(&f),
            [
                "Server struct 12-15",
                "Handler interface 17-20",
                "Handler.Serve method 18-18",
                "Handler.Close method 19-19",
                "ID type 22-22",
                "Pair struct 25-25",
                "Alias type 26-26",
                "Server.Start method 30-35",
                "Server.Label method 37-39",
                "NewServer function 41-47",
                "Map function 49-49",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "Server.Start: fmt.Println @31",
                "Server.Start: s.loop @33",
                "Server.Start: s.listen @34",
                "Server.Label: str.ToUpper @38",
                "NewServer: s.Close @43",
                "NewServer: len @44",
                "NewServer: fn @45",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "fmt||fmt|0|4",
                "strings||str|0|5",
                "net/http/pprof||_|0|6",
                "os||os|0|9",
            ]
        );
    }

    #[test]
    fn java_class_interface_enum_and_imports() {
        let f = facts(SRC_JAVA_CLASS_INTERFACE_ENUM_AND_IMPORTS, Lang::Java);
        assert_eq!(
            defs(&f),
            [
                "UserService class 11-30",
                "UserService.run method 16-19",
                "UserService.UserService method 21-23",
                "UserService.pick method 26-29",
                "Repo interface 32-35",
                "Repo.find method 33-33",
                "Repo.ping method 34-34",
                "Color enum 37-37",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "-: ArrayList @13",
                "UserService.run: System.out.println @17",
                "UserService.run: items.size @17",
                "UserService.run: helper @18",
                "UserService.UserService: super @22",
                "UserService.pick: xs.get @28",
                "Repo.ping: log @34",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "java.util|List|List|0|3",
                "java.util|*|*|0|4",
                "java.lang.Math|max|max|0|5",
            ]
        );
    }

    #[test]
    fn c_functions_structs_and_includes() {
        let f = facts(SRC_C_FUNCTIONS_STRUCTS_AND_INCLUDES, Lang::C);
        assert_eq!(
            defs(&f),
            [
                "add function 6-9",
                "point struct 11-13",
                "item_t struct 15-17",
                "main function 19-24",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "main: printf @21",
                "main: add @21",
                "main: puts @22",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "stdio.h||stdio.h|0|1",
                "util/helpers.h||helpers.h|0|2",
            ]
        );
    }

    #[test]
    fn cpp_namespace_class_and_out_of_class_method() {
        let f = facts(SRC_CPP_NAMESPACE_CLASS_AND_OUT_OF_CLASS_METHOD, Lang::Cpp);
        assert_eq!(
            defs(&f),
            [
                "app.Widget class 8-17",
                "app.Widget.Widget method 10-10",
                "app.Widget.area method 13-13",
                "app.Widget.make method 14-14",
                "app.Widget.draw method 19-22",
                "app.clamp function 25-27",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "app.Widget.area: compute @13",
                "app.Widget.make: Widget @14",
                "app.Widget.draw: helper @21",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "vector||vector|0|1",
                "std|*|*|0|2",
                "std|string|string|0|3",
            ]
        );
    }

    #[test]
    fn csharp_namespace_class_and_using() {
        let f = facts(SRC_CSHARP_NAMESPACE_CLASS_AND_USING, Lang::CSharp);
        assert_eq!(
            defs(&f),
            [
                "Acme.Core.IRepo interface 7-7",
                "Acme.Core.IRepo.Find method 7-7",
                "Acme.Core.Service class 10-23",
                "Acme.Core.Service.Service method 13-13",
                "Acme.Core.Service.Find method 15-19",
                "Acme.Core.Service.Pick method 21-21",
                "Acme.Core.Service.Twice method 22-22",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "Acme.Core.Service.Find: Lookup @17",
                "-: items.Count @20",
                "Acme.Core.Service.Twice: Double @22",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "System||System|0|1",
                "System.Collections.Generic||Generic|0|2",
                "Foo.Bar||Alias|0|3",
            ]
        );
    }

    #[test]
    fn kotlin_classes_functions_and_imports() {
        let f = facts(SRC_KOTLIN_CLASSES_FUNCTIONS_AND_IMPORTS, Lang::Kotlin);
        assert_eq!(
            defs(&f),
            [
                "User class 7-7",
                "Repo interface 9-11",
                "Repo.find method 10-10",
                "Service class 13-21",
                "Service.find method 14-18",
                "Service.double method 20-20",
                "shout function 23-23",
                "main function 25-28",
                "main.find function 26-26",
                "Shape class 30-36",
                "Shape.step method 31-31",
                "Shape.reset method 35-35",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "-: Base @13",
                "Service.find: println @16",
                "Service.find: repo.find @17",
                "Service.double: twice @20",
                "shout: this.uppercase @23",
                "main: Service @26",
                "main: s.find @27",
                "Shape.reset: step @35",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "java.util|List|List|0|3",
                "foo.bar|Baz|Qux|0|4",
            ]
        );
    }

    #[test]
    fn swift_protocol_struct_and_extension() {
        let f = facts(SRC_SWIFT_PROTOCOL_STRUCT_AND_EXTENSION, Lang::Swift);
        assert_eq!(
            defs(&f),
            [
                "Shape interface 5-7",
                "Shape.area method 6-6",
                "Circle struct 9-15",
                "Circle.area method 11-13",
                "Circle.init method 14-14",
                "Canvas class 17-22",
                "Canvas.draw method 18-21",
                "Canvas impl 24-26",
                "main function 28-28",
                "Button class 30-34",
                "Button.tap method 31-31",
                "Button.later method 32-32",
                "Button.two method 33-33",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "Canvas.draw: print @19",
                "Canvas.draw: render @20",
                "main: Canvas @28",
                "main: Canvas().draw @28",
                "main: Circle @28",
                "Button.tap: render @31",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "Foundation||Foundation|0|1",
                "Swift|Int|Int|0|2",
            ]
        );
    }

    #[test]
    fn ruby_module_class_and_methods() {
        let f = facts(SRC_RUBY_MODULE_CLASS_AND_METHODS, Lang::Ruby);
        assert_eq!(
            defs(&f),
            [
                "Greeting class 5-24",
                "Greeting.Greeter class 6-23",
                "Greeting.Greeter.initialize method 7-9",
                "Greeting.Greeter.greet method 11-14",
                "Greeting.Greeter.build method 16-18",
                "Greeting.Greeter.default_name method 22-22",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "Greeting.Greeter.greet: puts @12",
                "Greeting.Greeter.build: new @17",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "json||json|0|1",
                "lib/util||util|0|2",
            ]
        );
    }

    #[test]
    fn php_namespace_class_and_use() {
        let f = facts(SRC_PHP_NAMESPACE_CLASS_AND_USE, Lang::Php);
        assert_eq!(
            defs(&f),
            [
                "App.Models.User class 10-19",
                "App.Models.User.name method 12-16",
                "App.Models.User.make method 18-18",
                "App.Models.helper function 21-21",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "App.Models.User.name: strtoupper @15",
                "App.Models.User.make: self @18",
                "App.Models.helper: User.make @21",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "App\\Base|Model|Model|0|4",
                "App\\Support|Helper|Helper|0|5",
                "App\\Support|Str|S|0|5",
            ]
        );
    }

    #[test]
    fn shell_functions_source_and_heredoc() {
        let f = facts(SRC_SHELL_FUNCTIONS_SOURCE_AND_HEREDOC, Lang::Shell);
        assert_eq!(
            defs(&f),
            [
                "greet function 6-11",
                "build function 13-15",
            ]
        );
        assert!(calls(&f).is_empty());
        assert_eq!(
            imports(&f),
            [
                "./lib/common.sh||common.sh|0|2",
                "$DIR/other.sh||other.sh|0|3",
            ]
        );
    }

    #[test]
    fn javascript_functions_class_and_imports() {
        let f = facts(SRC_JAVASCRIPT_FUNCTIONS_CLASS_AND_IMPORTS, Lang::JavaScript);
        assert_eq!(
            defs(&f),
            [
                "add function 7-7",
                "mul function 9-11",
                "Counter class 13-17",
                "Counter.constructor method 14-14",
                "Counter.inc method 15-15",
                "Counter.from method 16-16",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "-: require @4",
                "mul: add @10",
                "Counter.constructor: super @14",
                "Counter.from: Counter @16",
                "-: fmt @20",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "fs|default|fs|0|1",
                "./mod|a|a|0|2",
                "./mod|b|c|0|2",
                "path|*|path|0|3",
                "x||x|0|4",
            ]
        );
    }

    #[test]
    fn typescript_interface_type_enum_and_class() {
        let f = facts(SRC_TYPESCRIPT_INTERFACE_TYPE_ENUM_AND_CLASS, Lang::TypeScript);
        assert_eq!(
            defs(&f),
            [
                "Repo interface 1-3",
                "Repo.find method 2-2",
                "Id type 4-4",
                "Color enum 5-5",
                "Svc class 6-9",
                "Svc.find method 8-8",
                "handler function 10-10",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "-: Map @7",
                "Svc.find: this.cache.get @8",
                "Svc.find: String @8",
                "handler: run @10",
            ]
        );
        assert!(imports(&f).is_empty());
    }

    #[test]
    fn rust_struct_impl_trait_and_use() {
        let f = facts(SRC_RUST_STRUCT_IMPL_TRAIT_AND_USE, Lang::Rust);
        assert_eq!(
            defs(&f),
            [
                "Config struct 6-6",
                "Config impl 8-15",
                "Config.parse method 10-14",
                "Shape trait 17-17",
                "Shape.area method 17-17",
                "E enum 18-18",
                "helper function 19-19",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "Config.parse: helper @13",
                "helper: x.len @19",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "std::collections|HashMap|HashMap|0|1",
                "std::collections|HashSet|Set|0|1",
                "crate::store|*|*|0|2",
            ]
        );
    }

    #[test]
    fn python_classes_calls_and_imports() {
        let f = facts(SRC_PYTHON_CLASSES_CALLS_AND_IMPORTS, Lang::Python);
        assert_eq!(
            defs(&f),
            [
                "Config class 9-21",
                "Config.parse method 15-18",
                "Config.load method 20-21",
                "top function 23-26",
                "top.inner function 25-25",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "-: decorator @8",
                "-: default @15",
                "Config.parse: self.build @18",
                "Config.parse: strip @18",
                "Config.load: fetch @21",
                "top: inner @26",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "os||os|0|1",
                "sys||system|0|1",
                "|util|util|1|2",
                "pkg.sub|alpha|a|2|3",
                "pkg.sub|beta|beta|2|3",
                "x.y|*|*|0|4",
                "json||json|0|28",
            ]
        );
    }

    #[test]
    fn python_nesting_one_liners_and_continuations() {
        let f = facts(SRC_PYTHON_NESTING_ONE_LINERS_AND_CONTINUATIONS, Lang::Python);
        assert_eq!(
            defs(&f),
            [
                "A class 4-16",
                "A.B class 6-7",
                "A.B.m method 7-7",
                "A.f method 8-14",
                "A.s method 16-16",
                "top function 18-18",
                "agen function 19-20",
                "Empty class 22-22",
                "WithDoc class 23-24",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "-: y @8",
                "top: print @18",
                "top: other @18",
                "agen: z @20",
                "-: range @25",
                "-: cond @25",
            ]
        );
        assert_eq!(
            imports(&f),
            [
                "a.b.c||d|0|1",
                "e||e|0|1",
                "f|g|g|0|2",
                "f|h|i|0|2",
            ]
        );
    }

    #[test]
    fn rust_modifiers_nested_comments_and_raw_strings() {
        let f = facts(SRC_RUST_MODIFIERS_NESTED_COMMENTS_AND_RAW_STRINGS, Lang::Rust);
        assert_eq!(
            defs(&f),
            [
                "run function 1-1",
                "Wrapper impl 2-2",
                "Wrapper impl 3-5",
                "Wrapper.fmt method 4-4",
                "inner.deep function 6-6",
                "ext function 8-8",
                "Alias type 9-9",
                "U struct 10-10",
                "Tuple struct 12-12",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "run: f @1",
                "run: Ok @1",
            ]
        );
        assert!(imports(&f).is_empty());
    }

    #[test]
    fn ruby_blocks_modifiers_and_heredoc() {
        let f = facts(SRC_RUBY_BLOCKS_MODIFIERS_AND_HEREDOC, Lang::Ruby);
        assert_eq!(
            defs(&f),
            [
                "Foo class 1-28",
                "Foo.initialize method 3-3",
                "Foo.each method 4-8",
                "Foo.ok? method 9-28",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "Foo.ok?: risky @18",
                "Foo.ok?: log @20",
            ]
        );
        assert!(imports(&f).is_empty());
    }

    #[test]
    fn jsx_object_methods_private_members_and_templates() {
        let f = facts(SRC_JSX_OBJECT_METHODS_PRIVATE_MEMBERS_AND_TEMPLATES, Lang::Jsx);
        assert_eq!(
            defs(&f),
            [
                "App function 1-3",
                "m function 4-4",
                "f function 5-5",
                "K class 6-6",
                "K.v method 6-6",
                "K.g method 6-6",
                "K.#p method 6-6",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "-: c @8",
            ]
        );
        assert!(imports(&f).is_empty());
    }

    #[test]
    fn go_generic_receivers_and_package_level_calls() {
        let f = facts(SRC_GO_GENERIC_RECEIVERS_AND_PACKAGE_LEVEL_CALLS, Lang::Go);
        assert_eq!(
            defs(&f),
            [
                "Repo.Get method 3-3",
                "Number interface 4-4",
                "init function 7-7",
                "Fn type 8-8",
                "apply function 9-13",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "-: compute @6",
                "init: register @7",
                "apply: f @10",
                "apply: g @11",
                "apply: f @12",
            ]
        );
        assert!(imports(&f).is_empty());
    }

    #[test]
    fn java_nested_types_static_block_and_lambda() {
        let f = facts(SRC_JAVA_NESTED_TYPES_STATIC_BLOCK_AND_LAMBDA, Lang::Java);
        assert_eq!(
            defs(&f),
            [
                "Outer class 1-15",
                "Outer.run method 4-4",
                "Outer.sum method 7-10",
                "Outer.Inner class 11-13",
                "Outer.Inner.hi method 12-12",
                "Outer.Cb interface 14-14",
                "Outer.Cb.make method 14-14",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "-: init @2",
                "-: Runnable @3",
                "Outer.run: go @4",
                "Outer.sum: list.forEach @8",
                "Outer.sum: use @8",
                "Outer.Inner.hi: greet @12",
                "Outer.Cb.make: create @14",
            ]
        );
        assert!(imports(&f).is_empty());
    }

    #[test]
    fn cpp_operators_init_lists_and_extern() {
        let f = facts(SRC_CPP_OPERATORS_INIT_LISTS_AND_EXTERN, Lang::Cpp);
        assert_eq!(
            defs(&f),
            [
                "Color enum 1-1",
                "Vec struct 2-7",
                "Vec.operator+ method 4-4",
                "Vec.operator bool method 6-6",
                "Box class 8-14",
                "Box.Box method 10-10",
                "Box.get method 11-11",
                "Vec.len2 method 15-15",
                "c_api function 16-16",
                "helper function 18-18",
            ]
        );
        assert_eq!(
            calls(&f),
            [
                "helper: LOG @18",
            ]
        );
        assert!(imports(&f).is_empty());
    }

    #[test]
    fn shell_nested_and_subshell_functions() {
        let f = facts(SRC_SHELL_NESTED_AND_SUBSHELL_FUNCTIONS, Lang::Shell);
        assert_eq!(
            defs(&f),
            [
                "a function 2-2",
                "b function 3-6",
                "b.inner function 4-4",
                "my-func function 10-12",
            ]
        );
        assert!(calls(&f).is_empty());
        assert!(imports(&f).is_empty());
    }

    #[test]
    fn masking_keeps_length_line_breaks_and_utf8() {
        for (src, lang) in corpus() {
            let m = mask(src, lang);
            assert_eq!(m.code.len(), src.len());
            let before: Vec<usize> = src.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i).collect();
            let after: Vec<usize> = m.code.iter().enumerate().filter(|(_, b)| **b == b'\n').map(|(i, _)| i).collect();
            assert_eq!(before, after);
            assert!(std::str::from_utf8(&m.code).is_ok());
            assert!(m.strings.windows(2).all(|w| w[0].1 <= w[1].0), "string ranges are ordered");
        }
    }

    #[test]
    fn masking_ignores_braces_in_c_comments_strings_and_chars() {
        let src = "int a; // {\nchar *s = \"x { y\"; /* } */ char q = '{'; char e = '\\''; int n = 1'000;\nint b() { return 0; }\n";
        let m = masked(src, Lang::C);
        assert_eq!(m.matches('{').count(), 1);
        assert_eq!(m.matches('}').count(), 1);
        assert!(m.contains("int b()"));
    }

    #[test]
    fn masking_tells_rust_lifetimes_from_chars_and_reads_raw_strings() {
        let src = "fn f<'a>(x: &'a str) -> char { '{' } // }\nlet r = r#\"a \" { \"#;\n/* a /* { */ } */\n";
        let m = masked(src, Lang::Rust);
        assert_eq!(m.matches('{').count(), 1);
        assert_eq!(m.matches('}').count(), 1);
        assert!(m.contains("'a str"));
    }

    #[test]
    fn masking_python_triple_quotes_prefixes_and_comments() {
        let src = "x = rb'{'  # it's { here\ny = '''a\n{ \"\"\" } '''\nz = f\"{x}\" + 'don\\'t {'\n";
        let m = masked(src, Lang::Python);
        assert!(!m.contains('{') && !m.contains('}'));
        assert!(m.starts_with("x =") && m.contains("\nz ="));
    }

    #[test]
    fn masking_shell_keeps_expansions_and_blanks_heredocs() {
        let src = "n=${#arr[@]}; echo $# # comment {\ncat <<-EOF\n\tbody {\n\tEOF\necho 'a }' \"b {\"\n";
        let m = masked(src, Lang::Shell);
        assert!(m.contains("${#arr[@]}") && m.contains("$#"));
        assert!(!m.contains("comment") && !m.contains("body"));
        assert_eq!(m.matches('{').count(), 1);
        assert_eq!(m.matches('}').count(), 1);
    }

    #[test]
    fn masking_javascript_regex_and_nested_templates() {
        let src = "const r = /[{\"]+/g; const t = `a ${f({x: 1})} {`; const d = 4 / 2 / 1;\n";
        let m = masked(src, Lang::JavaScript);
        // Only the braces of `f({x: 1})` are code: the `${` and its `}` belong to the template.
        assert_eq!(m.matches('{').count(), 1);
        assert_eq!(m.matches('}').count(), 1);
        assert!(m.contains("f({x: 1})"));
        assert!(m.contains("4 / 2 / 1"));
    }

    #[test]
    fn masking_ruby_percent_literals_and_block_comments() {
        let src = "x = %w[a } b]\nputs '#{x}' # }\n=begin\n}\n=end\ny = {a: 1}\n";
        let m = masked(src, Lang::Ruby);
        assert_eq!(m.matches('{').count(), 1);
        assert_eq!(m.matches('}').count(), 1);
    }

    #[test]
    fn python_header_bases_and_docstring() {
        let src = "class C(Base, Mixin[int], metaclass=Meta):\n    '''Doc line.\n\n    More.'''\n    def m(self,\n          x): pass\n";
        let f = facts(src, Lang::Python);
        assert_eq!(f.symbols[0].sig, "class C(Base, Mixin[int], metaclass=Meta)");
        assert_eq!(f.symbols[0].bases, ["Base", "Mixin[int]"]);
        assert_eq!(f.symbols[0].doc, "Doc line.");
        assert_eq!(f.symbols[1].sig, "def m(self, x)");
        assert_eq!(f.symbols[1].kind, SymKind::Method);
        assert_eq!((f.symbols[1].start, f.symbols[1].end), (5, 6));
    }

    #[test]
    fn documentation_is_the_comment_block_directly_above() {
        let rust = "/// First.\n/// Second.\n#[derive(Debug)]\nstruct S;\n";
        assert_eq!(facts(rust, Lang::Rust).symbols[0].doc, "First.");
        let go = "// Foo does it.\nfunc Foo() {}\n\n// Not doc.\n\nfunc Bar() {}\n";
        let f = facts(go, Lang::Go);
        assert_eq!((f.symbols[0].doc.as_str(), f.symbols[1].doc.as_str()), ("Foo does it.", ""));
        let c = "int x; // trailing\nint f() { return 1; }\n/**\n * Block first.\n * Second.\n */\nint g() { return 2; }\n";
        let f = facts(c, Lang::C);
        assert_eq!((f.symbols[0].doc.as_str(), f.symbols[1].doc.as_str()), ("", "Block first."));
        let rb = "# Greets.\n# More.\ndef hi\nend\n";
        assert_eq!(facts(rb, Lang::Ruby).symbols[0].doc, "Greets.");
        let sh = "#!/bin/sh\nrun() { :; }\n";
        assert_eq!(facts(sh, Lang::Shell).symbols[0].doc, "");
        let java = "/** Java doc. */\n@Override\npublic void run() {}\n";
        assert_eq!(facts(java, Lang::Java).symbols[0].doc, "Java doc.");
    }

    #[test]
    fn control_words_and_declarations_are_not_calls() {
        let c = "int main() {\n  if (a) { while (b) { for (;;) { switch (c) { case 1: return sizeof(int) + foo(1); } } } }\n}\n";
        assert_eq!(calls(&facts(c, Lang::C)), ["main: foo @2"]);
        let py = "def f(a):\n    if (a): return not(a) or g(a)\n    print (a)\n";
        assert_eq!(calls(&facts(py, Lang::Python)), ["f: g @2", "f: print @3"]);
        let go = "package p\nfunc F() {\n\tfor i := range xs { go work(i) }\n\tdefer done()\n}\n";
        assert_eq!(calls(&facts(go, Lang::Go)), ["F: work @3", "F: done @4"]);
        let php = "<?php\nfunction f($a) { if (isset($a)) { echo(1); } return count($a); }\n";
        assert_eq!(calls(&facts(php, Lang::Php)), ["f: count @2"]);
    }

    #[test]
    fn receivers_are_the_dotted_arrow_or_scope_chain() {
        let js = "a.b.c(1); f(1).g(2); s[0].h(3); new Foo(4); obj?.m(5); x.y(1).z(2);\n";
        assert_eq!(
            calls(&facts(js, Lang::JavaScript)),
            ["-: a.b.c @1", "-: f @1", "-: f(1).g @1", "-: s[0].h @1", "-: Foo @1", "-: obj.m @1", "-: x.y @1", "-: x.y(1).z @1"]
        );
        let cpp = "void f() { p->run(1); ns::Cls::make(2); this->a.b(3); }\n";
        assert_eq!(calls(&facts(cpp, Lang::Cpp)), ["f: p.run @1", "f: ns::Cls.make @1", "f: this->a.b @1"]);
        let php = "<?php\nfunction f() { $this->svc->run(1); Foo::bar(2); }\n";
        assert_eq!(calls(&facts(php, Lang::Php)), ["f: $this->svc.run @2", "f: Foo.bar @2"]);
    }

    #[test]
    fn generic_calls_are_calls() {
        let ts = "const m = new Map<string, number>(); const v = pick<string>(xs);\n";
        assert_eq!(calls(&facts(ts, Lang::TypeScript)), ["-: Map @1", "-: pick @1"]);
        let rs = "fn f() { let v = Vec::<u8>::with_capacity(4); parse::<i32>(s); }\n";
        assert_eq!(calls(&facts(rs, Lang::Rust)), ["f: with_capacity @1", "f: parse @1"]);
    }

    #[test]
    fn indentation_decides_where_a_python_body_ends() {
        let src = "def a():\n    x = 1\n\n    # trailing comment\n\ndef b():\n    return (1 +\n            2)\ny = 3\n";
        assert_eq!(defs(&facts(src, Lang::Python)), ["a function 1-2", "b function 6-8"]);
        let tabs = "class A:\n\tdef f(self):\n\t\treturn 1\n\tdef g(self):\n\t\treturn 2\n";
        assert_eq!(defs(&facts(tabs, Lang::Python)), ["A class 1-5", "A.f method 2-3", "A.g method 4-5"]);
    }

    #[test]
    fn matching_end_closes_ruby_blocks() {
        let src = "def a\n  x = [1].map do |i|\n    i\n  end\n  puts 1 if x\nend\ndef b; end\n";
        assert_eq!(defs(&facts(src, Lang::Ruby)), ["a function 1-6", "b function 7-7"]);
    }

    #[test]
    fn malformed_input_never_panics() {
        let junk = [
            "", "}", "{{{{", "(((", "'", "\"", "`", "/*", "#!", "<<<EOF", "<<~EOS", "def", "class", "fn", "func(",
            "end", "=begin", "r#\"", "R\"x(", "\u{feff}", "\u{1F600} \u{e9} \u{fc}", "\\", "a.b.c(", "x = (", "if (", "[[[]",
            "@", "$(", "${", "%w[", "/[", "`${",
        ];
        for lang in ALL_LANGS {
            for text in junk {
                let _ = facts(text, lang);
            }
        }
    }

    #[test]
    fn every_prefix_and_every_tail_of_the_samples_parses() {
        for (src, lang) in corpus() {
            for (cut, _) in src.char_indices() {
                let _ = facts(&src[..cut], lang);
            }
            for (cut, c) in src.char_indices() {
                if c == '\n' {
                    let _ = facts(&src[cut + 1..], lang);
                }
            }
        }
    }

    #[test]
    fn every_sample_parses_as_every_language() {
        for (src, _) in corpus() {
            for lang in ALL_LANGS {
                let f = facts(src, lang);
                for s in &f.symbols {
                    assert!(s.start <= s.end && s.end <= f.lines.max(1));
                }
            }
        }
    }

    fn sigs(f: &FileFacts) -> Vec<String> {
        f.symbols.iter().map(|s| s.sig.clone()).collect()
    }

    #[test]
    fn an_arrow_is_a_member_access_only_where_the_language_has_one() {
        let java = "class A {\n  void f(java.util.List<String> xs, int k) {\n    xs.stream().map(x -> convert(x));\n    switch (k) { case 1 -> compute(); default -> other(); }\n  }\n}\n";
        assert_eq!(
            calls(&facts(java, Lang::Java)),
            ["A.f: xs.stream @3", "A.f: xs.stream().map @3", "A.f: convert @3", "A.f: compute @4", "A.f: other @4"]
        );
        let kotlin = "class C {\n    fun f(x: Any) {\n        when (x) {\n            is Foo -> handle(x)\n            1 -> one()\n            else -> other()\n        }\n    }\n}\n";
        assert_eq!(calls(&facts(kotlin, Lang::Kotlin)), ["C.f: handle @4", "C.f: one @5", "C.f: other @6"]);
        // Where the arrow does reach a member, it still does.
        assert_eq!(calls(&facts("void f() { p->run(1); }\n", Lang::C)), ["f: p.run @1"]);
        assert_eq!(calls(&facts("class A { void f() { p->run(1); } }\n", Lang::CSharp)), ["A.f: p.run @1"]);
    }

    #[test]
    fn a_byte_order_mark_changes_nothing_about_what_is_found() {
        for (src, lang) in corpus() {
            let plain = facts(src, lang);
            let marked = facts(&format!("\u{feff}{src}"), lang);
            assert_eq!(defs(&plain), defs(&marked), "{lang:?}");
            assert_eq!(calls(&plain), calls(&marked), "{lang:?}");
            assert_eq!(imports(&plain), imports(&marked), "{lang:?}");
            assert_eq!(sigs(&plain), sigs(&marked), "{lang:?}");
        }
    }

    #[test]
    fn php_text_outside_the_tags_is_not_code() {
        let two = "<?php class A { function f() {} } ?>\n<p>Don't do this</p>\n<?php function g() { h(); } class B { function m() {} } ?>\n";
        let f = facts(two, Lang::Php);
        assert_eq!(defs(&f), ["A class 1-1", "A.f method 1-1", "g function 3-3", "B class 3-3", "B.m method 3-3"]);
        assert_eq!(calls(&f), ["g: h @3"]);

        let page = "<html>\n<p>It's \"here\"</p>\n<?php\nfunction f() { g(); }\n?>\n<b><?= count($a) ?></b>\n";
        let f = facts(page, Lang::Php);
        assert_eq!(defs(&f), ["f function 4-4"]);
        assert_eq!(calls(&f), ["f: g @4", "-: count @6"]);

        // A line comment ends at the closing tag.
        let comment = "<?php // note ?>\n<p>don't</p>\n<?php function z() {}\n";
        assert_eq!(defs(&facts(comment, Lang::Php)), ["z function 3-3"]);

        // A file with no open tag at all is read as code, the way a snippet is.
        assert_eq!(defs(&facts("function f() { g(); }\n", Lang::Php)), ["f function 1-1"]);
    }

    #[test]
    fn an_inactive_preprocessor_branch_that_opens_a_brace_does_not_swallow_the_file() {
        let c = "#ifdef WIN32\nint f(int a) {\n#else\nint f(long a) {\n#endif\n  return a;\n}\nint g(void) { return 1; }\nint h(void) { return 2; }\n";
        let f = facts(c, Lang::C);
        assert_eq!(defs(&f), ["f function 2-7", "g function 8-8", "h function 9-9"]);
        assert_eq!(calls(&f), Vec::<String>::new());

        let chain = "#if A\nvoid f() {\n#elif B\nvoid f2() {\n#else\nvoid f3() {\n#endif\n  g();\n}\nvoid h() {}\n";
        let f = facts(chain, Lang::Cpp);
        assert_eq!(defs(&f), ["f function 2-9", "h function 10-10"]);
        assert_eq!(calls(&f), ["f: g @8"]);

        // Branches that each stand on their own are all kept.
        let alone = "#ifdef _WIN32\nint win(void) { return 1; }\n#else\nint posix(void) { return 2; }\n#endif\nint tail(void) { return 3; }\n";
        assert_eq!(defs(&facts(alone, Lang::C)), ["win function 2-2", "posix function 4-4", "tail function 6-6"]);

        // One branch that opens a scope and a later one that closes it are not alternatives.
        let guard = "#ifdef __cplusplus\nextern \"C\" {\n#endif\nint a(void) { return 1; }\n#ifdef __cplusplus\n}\n#endif\nint b(void) { return 2; }\n";
        assert_eq!(defs(&facts(guard, Lang::Cpp)), ["a function 4-4", "b function 8-8"]);

        // Groups inside a branch are settled first.
        let nested = "#if A\n#if B\nvoid x() {\n#else\nvoid y() {\n#endif\n#else\nvoid z() {\n#endif\n  g();\n}\nvoid w() {}\n";
        assert_eq!(defs(&facts(nested, Lang::C)), ["x function 3-11", "w function 12-12"]);

        // A group that is never closed is left as it is.
        assert_eq!(defs(&facts("#if A\nint a(void) { return 1; }\n#else\nint b(void) { return 2; }\n", Lang::C)), ["a function 2-2", "b function 4-4"]);
    }

    #[test]
    fn a_python_def_at_the_left_margin_ends_an_unclosed_bracket() {
        let py = "x = foo(\ndef a():\n    pass\nclass B:\n    def m(self):\n        pass\n";
        assert_eq!(defs(&facts(py, Lang::Python)), ["a function 2-3", "B class 4-6", "B.m method 5-6"]);
        let asynchronous = "x = [1,\nasync def a():\n    pass\n";
        assert_eq!(defs(&facts(asynchronous, Lang::Python)), ["a function 2-3"]);
        // A bracket that does close still joins its lines, whatever they start with.
        let joined = "x = foo(1,\n    2,\n)\ndef a():\n    pass\n";
        assert_eq!(defs(&facts(joined, Lang::Python)), ["a function 4-5"]);
    }

    #[test]
    fn nested_use_trees_are_read_to_the_depth_of_real_code() {
        let f = facts("use a::{b::{c::{d, e as g}, f}, h};\n", Lang::Rust);
        assert_eq!(imports(&f), ["a::b::c|d|d|0|1", "a::b::c|e|g|0|1", "a::b|f|f|0|1", "a|h|h|0|1"]);
        // Far deeper than anyone writes: the recursion stops instead of using the stack.
        let deep = format!("use {}x{};\nfn f() {{}}\n", "a::{".repeat(500), "}".repeat(500));
        let f = facts(&deep, Lang::Rust);
        assert!(f.imports.is_empty());
        assert_eq!(defs(&f), ["f function 2-2"]);
    }
}
