//! What one source file is known to contain: its definitions, its calls and its imports.

use serde::{Deserialize, Serialize};

/// A language the extractor knows by file extension.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Lang {
    Python,
    JavaScript,
    Jsx,
    TypeScript,
    Tsx,
    Rust,
    Go,
    Java,
    C,
    Cpp,
    CSharp,
    Kotlin,
    Swift,
    Ruby,
    Php,
    Shell,
}

impl Lang {
    /// The language of a file extension, given without its dot and in any case.
    pub fn from_extension(ext: &str) -> Option<Lang> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "py" | "pyi" | "pyw" => Lang::Python,
            "js" | "mjs" | "cjs" => Lang::JavaScript,
            "jsx" => Lang::Jsx,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "rs" => Lang::Rust,
            "go" => Lang::Go,
            "java" => Lang::Java,
            "c" | "h" => Lang::C,
            "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Lang::Cpp,
            "cs" => Lang::CSharp,
            "kt" | "kts" => Lang::Kotlin,
            "swift" => Lang::Swift,
            "rb" => Lang::Ruby,
            "php" => Lang::Php,
            "sh" | "bash" | "zsh" => Lang::Shell,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Python => "python",
            Lang::JavaScript => "javascript",
            Lang::Jsx => "jsx",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::Rust => "rust",
            Lang::Go => "go",
            Lang::Java => "java",
            Lang::C => "c",
            Lang::Cpp => "cpp",
            Lang::CSharp => "csharp",
            Lang::Kotlin => "kotlin",
            Lang::Swift => "swift",
            Lang::Ruby => "ruby",
            Lang::Php => "php",
            Lang::Shell => "shell",
        }
    }
}

/// What kind of definition a symbol is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SymKind {
    Class,
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Interface,
    Impl,
    Const,
    TypeAlias,
}

impl SymKind {
    /// The word printed for this kind.
    pub fn word(self) -> &'static str {
        match self {
            SymKind::Class => "class",
            SymKind::Function => "function",
            SymKind::Method => "method",
            SymKind::Struct => "struct",
            SymKind::Enum => "enum",
            SymKind::Trait => "trait",
            SymKind::Interface => "interface",
            SymKind::Impl => "impl",
            SymKind::Const => "const",
            SymKind::TypeAlias => "type",
        }
    }

    /// True for the kinds that can be called and can contain calls.
    pub fn is_callable(self) -> bool {
        matches!(self, SymKind::Function | SymKind::Method)
    }

    /// True for the kinds that can own methods.
    pub fn is_container(self) -> bool {
        matches!(
            self,
            SymKind::Class | SymKind::Struct | SymKind::Enum | SymKind::Trait | SymKind::Interface | SymKind::Impl
        )
    }
}

/// How the facts of a file were obtained.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ParseGrade {
    /// A full parser built a syntax tree.
    Ast,
    /// The parser was not available for this language, or it rejected the file,
    /// and the token-based extractor was used instead.
    Lexical,
}

/// One definition in a file.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Symbol {
    /// The bare name, `parse` for a method `Config.parse`.
    pub name: String,
    /// The name with its containers, joined by dots: `Config.parse`, `outer.inner`.
    /// Equal to `name` for a top-level definition.
    pub qual: String,
    pub kind: SymKind,
    /// The line the declaration itself starts on, counted from 1: the line of the
    /// `def`, `class`, `fn` or `function` keyword (or of the modifiers on that same
    /// declaration, such as `pub` or `async`). Decorators, attributes and
    /// documentation comments above it are not part of the span.
    pub start: u32,
    /// Last line of the definition, counted from 1, inclusive.
    pub end: u32,
    /// The declaration on one line with runs of white space folded to one space,
    /// without the body and without a trailing colon or brace:
    /// `def parse(self, text: str) -> Config`. At most 240 characters; a longer one
    /// is cut and ends in `...`.
    pub sig: String,
    /// The first sentence or first line of the documentation comment or docstring,
    /// trimmed, at most 200 characters. Empty when there is none.
    pub doc: String,
    /// Index, in the same file's `symbols`, of the definition this one sits in.
    pub parent: Option<u32>,
    /// For a class-like symbol, the names it extends or implements, as written.
    pub bases: Vec<String>,
    /// A hash of the definition's source lines, `start` to `end`. It changes when
    /// the definition's text changes and is what memory uses to notice stale notes.
    pub hash: u64,
}

/// One call site.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Call {
    /// Index, in the same file's `symbols`, of the innermost function or method
    /// the call sits in. `None` for a call at module level or in a class body.
    pub caller: Option<u32>,
    /// The called name alone: `load` for `self.cache.load(x)`, `run` for `run()`.
    pub name: String,
    /// What the name was called on, as written and at most 60 characters:
    /// `self.cache` for `self.cache.load(x)`, empty for a bare `run()`.
    pub recv: String,
    /// Line of the call, counted from 1.
    pub line: u32,
}

/// One imported name.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Import {
    /// The module as written, without leading dots: `os.path`, `./util`, `crate::store`.
    pub module: String,
    /// The name taken from the module, or empty when the module itself is imported.
    /// `*` for a star import.
    pub name: String,
    /// The name it is bound to in this file. Equal to `name` (or to the module's
    /// last part) when there is no `as`.
    pub alias: String,
    /// Number of leading dots of a Python relative import. 0 otherwise.
    pub level: u8,
    /// Line of the import, counted from 1.
    pub line: u32,
}

/// Everything known about one source file.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct FileFacts {
    /// Path from the repository root, with forward slashes.
    pub path: String,
    pub lang: Lang,
    /// Number of lines in the file.
    pub lines: u32,
    /// Size of the file in bytes.
    pub bytes: u64,
    /// A hash of the file's bytes.
    pub hash: u64,
    pub grade: ParseGrade,
    /// Definitions in source order. A container comes before what it contains.
    pub symbols: Vec<Symbol>,
    /// Calls in source order.
    pub calls: Vec<Call>,
    /// Imports in source order.
    pub imports: Vec<Import>,
}
