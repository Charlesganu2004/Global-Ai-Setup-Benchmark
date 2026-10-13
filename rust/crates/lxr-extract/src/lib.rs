//! Reads source files and reports their definitions, calls and imports.
//!
//! Python, JavaScript, TypeScript and Rust go through a full parser. Every other
//! language, and any file a parser rejects, goes through the token-based extractor
//! in `lexical`, and its facts carry [`ParseGrade::Lexical`] so that callers can say so.

mod common;
mod ecma;
mod lexical;
mod python;
mod rust_lang;
pub mod walk;

pub use common::Parts;
pub use walk::{walk, walk_checked, Unreadable, WalkOpts, Walked};

use lxr_core::{hash64, FileFacts, Lang, ParseGrade};
use std::path::Path;

/// The facts of one file. `path` is its path from the repository root with forward
/// slashes. Never panics on malformed source: a file no parser accepts still gets
/// the facts the token-based extractor can find.
pub fn extract(path: &str, lang: Lang, text: &str) -> FileFacts {
    let parsed = match lang {
        Lang::Python => guarded(|| python::parse(text)),
        Lang::JavaScript | Lang::Jsx | Lang::TypeScript | Lang::Tsx => guarded(|| ecma::parse(text, lang)),
        Lang::Rust => guarded(|| rust_lang::parse(text)),
        _ => None,
    };
    match parsed {
        Some(parts) => common::finish(path, lang, text, ParseGrade::Ast, parts),
        None => common::finish(path, lang, text, ParseGrade::Lexical, lexical::parse(text, tokens_as(path, lang, text))),
    }
}

/// The language the token-based extractor reads `text` as. A `.h` file is C by its extension,
/// but many headers are C++, and the C recogniser knows no `class`, `namespace` or `template`.
/// A header that uses one of them is read as C++, whose recogniser reads plain C declarations
/// as well. The facts keep the language of the extension.
fn tokens_as(path: &str, lang: Lang, text: &str) -> Lang {
    let header = Path::new(path).extension().and_then(|e| e.to_str()).map_or(false, |e| e.eq_ignore_ascii_case("h"));
    if lang == Lang::C && header && lexical::uses_cplusplus(text) {
        Lang::Cpp
    } else {
        lang
    }
}

/// Reads the file at `root/path` and extracts it. `None` when the extension is not
/// a known language or the file cannot be read. Bytes that are not UTF-8 are replaced
/// for the reading, but `bytes` and `hash` of the facts describe the file as it is on disk.
pub fn extract_path(root: &Path, path: &str) -> Option<FileFacts> {
    let lang = lang_of(Path::new(path))?;
    let bytes = std::fs::read(root.join(path)).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut facts = extract(path, lang, &text);
    // A replacement character is three bytes where the bad byte was one, and two different bad
    // bytes read the same. A change of the file must still change its hash, and the hash must be
    // the one of the bytes that a later check reads back.
    facts.bytes = bytes.len() as u64;
    facts.hash = hash64(&bytes);
    Some(facts)
}

/// The language of a path, by its extension.
pub fn lang_of(path: &Path) -> Option<Lang> {
    Lang::from_extension(path.extension()?.to_str()?)
}

/// Runs a parser and turns a panic inside it into "this parser has no answer", so
/// that one odd file cannot stop an index build.
fn guarded(parse: impl FnOnce() -> Option<Parts> + std::panic::UnwindSafe) -> Option<Parts> {
    std::panic::catch_unwind(parse).ok().flatten()
}
