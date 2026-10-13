//! The rules that decide what kind of query it is and which files and chunks it favours.
//!
//! They follow semble 0.6.1: which queries look like symbol names, how a file stem is
//! compared with a name, which words of a query match a path, which paths are pushed
//! down, and how a module chunk is tested for defining a name.

use lxr_text::code_tokens;
use regex::Regex;
use std::collections::{BTreeSet, HashSet};
use std::sync::OnceLock;

/// Weight of the vectors in a fused score for a query that looks like a symbol name.
pub(crate) const ALPHA_SYMBOL: f32 = 0.3;
/// Weight of the vectors in a fused score for any other query.
pub(crate) const ALPHA_TEXT: f32 = 0.5;
/// The constant of reciprocal rank fusion.
pub(crate) const RRF_K: f32 = 60.0;
/// Share of the best score added to the best chunk of the best file.
pub(crate) const FILE_BOOST: f64 = 0.2;
/// Multiple of the best score added to a chunk that defines the queried symbol.
pub(crate) const DEFINITION_BOOST: f64 = 3.0;
/// Multiple of the best score added for file names that match the words of a query.
pub(crate) const STEM_BOOST: f64 = 1.0;
/// Strength of a symbol found inside a natural-language query, against a bare symbol query.
pub(crate) const EMBEDDED_SCALE: f64 = 0.5;
/// Shortest file stem that may match the start of an embedded symbol.
const EMBEDDED_STEM_MIN: usize = 4;
/// Least share of the query words that must match a path to earn the stem boost.
const MIN_MATCH_RATIO: f64 = 0.10;
/// Shortest word of a query that is matched against paths.
const MIN_KEYWORD: usize = 3;

const STRONG_PENALTY: f64 = 0.3;
const MODERATE_PENALTY: f64 = 0.5;
const MILD_PENALTY: f64 = 0.7;

/// Words that never count when a query is matched against file names.
const STOPWORDS: [&str; 33] = [
    "a", "an", "and", "are", "as", "at", "be", "by", "do", "does", "for", "from", "has", "have", "how", "if", "in", "is",
    "it", "not", "of", "on", "or", "the", "to", "was", "what", "when", "where", "which", "who", "why", "with",
];

/// Words that start a definition in some language. Matched case-sensitively.
const DEFINITION_WORDS: &str = "class|module|defmodule|def|interface|struct|enum|trait|type|func|function|object|abstract class|data class|fn|fun|package|namespace|protocol|record|typedef";
/// Words that start a SQL definition. Matched in any case.
const SQL_WORDS: &str = "CREATE TABLE|CREATE VIEW|CREATE PROCEDURE|CREATE FUNCTION";
/// An optional chain of qualifiers, then the defined name.
const NAME_TAIL: &str = r"\s+(?:[A-Za-z_][A-Za-z0-9_]*(?:\.|::))*([A-Za-z_][A-Za-z0-9_]*)";

fn symbol_query_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(
            r"^(?:[A-Za-z_][A-Za-z0-9_]*(?:(?:::|\\|->|\.)[A-Za-z_][A-Za-z0-9_]*)+|_[A-Za-z0-9_]*|[A-Za-z][A-Za-z0-9]*[A-Z_][A-Za-z0-9_]*|[A-Z][A-Za-z0-9]*)$",
        )
        .expect("fixed pattern")
    })
}

fn embedded_symbol_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        Regex::new(r"\b(?:[A-Z][a-z][a-zA-Z0-9]*[A-Z][a-zA-Z0-9]*|[a-z][a-zA-Z0-9]*[A-Z][a-zA-Z0-9]+)\b").expect("fixed pattern")
    })
}

fn word_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"[a-zA-Z_][a-zA-Z0-9_]*").expect("fixed pattern"))
}

fn definition_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(&format!(r"(?m)(?:^|\s)(?:{DEFINITION_WORDS}){NAME_TAIL}")).expect("fixed pattern"))
}

fn sql_definition_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(&format!(r"(?mi)(?:^|\s)(?:{SQL_WORDS}){NAME_TAIL}")).expect("fixed pattern"))
}

fn test_file_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        let names = [
            r"test_[^/]*\.py",
            r"[^/]*_test\.py",
            r"[^/]*_test\.go",
            r"[^/]*Tests?\.java",
            r"[^/]*Test\.php",
            r"[^/]*_spec\.rb",
            r"[^/]*_test\.rb",
            r"[^/]*\.test\.[jt]sx?",
            r"[^/]*\.spec\.[jt]sx?",
            r"[^/]*Tests?\.kt",
            r"[^/]*Spec\.kt",
            r"[^/]*Tests?\.swift",
            r"[^/]*Spec\.swift",
            r"[^/]*Tests?\.cs",
            r"test_[^/]*\.cpp",
            r"[^/]*_test\.cpp",
            r"test_[^/]*\.c",
            r"[^/]*_test\.c",
            r"[^/]*Spec\.scala",
            r"[^/]*Suite\.scala",
            r"[^/]*Test\.scala",
            r"[^/]*_test\.dart",
            r"test_[^/]*\.dart",
            r"[^/]*_spec\.lua",
            r"[^/]*_test\.lua",
            r"test_[^/]*\.lua",
            r"test_helpers?[^/]*\.\w+",
        ];
        Regex::new(&format!("(?:^|/)(?:{})$", names.join("|"))).expect("fixed pattern")
    })
}

fn test_dir_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"(?:^|/)(?:tests?|__tests__|spec|testing)(?:/|$)").expect("fixed pattern"))
}

fn compat_dir_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"(?:^|/)(?:compat|_compat|legacy)(?:/|$)").expect("fixed pattern"))
}

fn examples_dir_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"(?:^|/)(?:_?examples?|docs?_src)(?:/|$)").expect("fixed pattern"))
}

/// True when the whole query looks like a symbol name: identifiers joined by `::`, `\`,
/// `->` or `.`; a name with a leading underscore; letters and digits with an uppercase
/// letter or an underscore in them; or a word that starts with an uppercase letter.
/// Lowercase words and anything with a space are not.
pub(crate) fn is_symbol_query(query: &str) -> bool {
    symbol_query_re().is_match(query.trim())
}

/// The weight of the vectors when the caller gives none.
pub(crate) fn auto_alpha(query: &str) -> f32 {
    if is_symbol_query(query) {
        ALPHA_SYMBOL
    } else {
        ALPHA_TEXT
    }
}

/// The text after the last separator of a qualified symbol query. Only the first kind of
/// separator found, in the order `::`, `\`, `->`, `.`, is used.
pub(crate) fn symbol_name(query: &str) -> &str {
    for separator in ["::", "\\", "->", "."] {
        if let Some(at) = query.rfind(separator) {
            return &query[at + separator.len()..];
        }
    }
    query.trim()
}

/// The names a symbol query asks for: the last part, the whole query, and the whole
/// query with its separators written as dots, which is how qualified names are stored.
pub(crate) fn symbol_names(query: &str) -> Vec<String> {
    let query = query.trim();
    let mut names = vec![symbol_name(query).to_string()];
    let dotted = query.replace("::", ".").replace('\\', ".").replace("->", ".");
    for extra in [query.to_string(), dotted] {
        if !names.contains(&extra) {
            names.push(extra);
        }
    }
    names
}

/// True when a lowercased file stem equals `name` (lowercased), or equals it once its
/// underscores are dropped, or once trailing `s` characters are dropped: the stems
/// `parser`, `parsers` and `pars_er` all match `parser`.
pub(crate) fn stem_matches(stem: &str, name: &str) -> bool {
    let joined = stem.replace('_', "");
    stem == name || joined == name || stem.trim_end_matches('s') == name || joined.trim_end_matches('s') == name
}

/// The camelCase and PascalCase words inside a natural-language query, each once.
pub(crate) fn embedded_symbols(query: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for m in embedded_symbol_re().find_iter(query) {
        if !found.iter().any(|f| f == m.as_str()) {
            found.push(m.as_str().to_string());
        }
    }
    found
}

/// True when a file stem fits one of the lowercased embedded symbols: it is the symbol
/// or its underscore-free form, or a stem of at least four characters that starts it.
pub(crate) fn embedded_stem_ok(stem: &str, stem_joined: &str, symbols_lower: &[String]) -> bool {
    symbols_lower.iter().any(|symbol| {
        stem == symbol
            || stem_joined == symbol
            || (stem.chars().count() >= EMBEDDED_STEM_MIN && symbol.starts_with(stem))
            || (stem_joined.chars().count() >= EMBEDDED_STEM_MIN && symbol.starts_with(stem_joined))
    })
}

/// The words of a query that are matched against paths: identifier words of more than
/// two characters, lowercased, without stop words.
pub(crate) fn keywords(query: &str) -> BTreeSet<String> {
    word_re()
        .find_iter(query)
        .map(|m| m.as_str())
        .filter(|w| w.len() >= MIN_KEYWORD)
        .map(str::to_lowercase)
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// The file name of a path without its last extension, as it is written.
pub(crate) fn file_stem(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(at) if at > 0 => &name[..at],
        _ => name,
    }
}

/// The names of the folders of a path, nearest last, without `.` and empty parts.
pub(crate) fn folder_names(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split(['/', '\\']).collect();
    parts.pop();
    parts.retain(|p| !p.is_empty() && *p != "." && *p != "..");
    parts
}

/// The words a path offers to a query: the file stem and the name of its folder, each as
/// a whole and split into its parts, lowercased.
pub(crate) fn path_parts(path: &str) -> HashSet<String> {
    let mut parts: HashSet<String> = code_tokens(file_stem(path)).into_iter().collect();
    if let Some(folder) = folder_names(path).last() {
        parts.extend(code_tokens(folder));
    }
    parts
}

/// How many of the keywords match the path words: exactly, or by sharing a start of at
/// least three characters with one of them.
pub(crate) fn keyword_matches(keywords: &BTreeSet<String>, parts: &HashSet<String>) -> usize {
    let mut count = 0;
    for keyword in keywords {
        if parts.contains(keyword) {
            count += 1;
            continue;
        }
        let near = parts.iter().any(|part| {
            let (shorter, longer) = if keyword.len() <= part.len() { (keyword, part) } else { (part, keyword) };
            shorter.len() >= MIN_KEYWORD && longer.starts_with(shorter.as_str())
        });
        if near {
            count += 1;
        }
    }
    count
}

/// True when `matches` of `total` query words is enough for the stem boost.
pub(crate) fn enough_matches(matches: usize, total: usize) -> bool {
    total > 0 && matches as f64 / total as f64 >= MIN_MATCH_RATIO
}

/// The factor a path's scores are multiplied by: 0.3 for test files and folders, 0.5 for
/// `__init__.py` and `package-info.java`, 0.3 for compat and legacy folders, 0.3 for
/// example and documentation-source folders, 0.7 for `.d.ts` files. Factors multiply.
pub(crate) fn path_penalty(path: &str) -> f64 {
    let path = path.replace('\\', "/");
    let mut factor = 1.0;
    if test_file_re().is_match(&path) || test_dir_re().is_match(&path) {
        factor *= STRONG_PENALTY;
    }
    let name = path.rsplit('/').next().unwrap_or("");
    if name == "__init__.py" || name == "package-info.java" {
        factor *= MODERATE_PENALTY;
    }
    if compat_dir_re().is_match(&path) {
        factor *= STRONG_PENALTY;
    }
    if examples_dir_re().is_match(&path) {
        factor *= STRONG_PENALTY;
    }
    if path.ends_with(".d.ts") {
        factor *= MILD_PENALTY;
    }
    factor
}

/// The names a piece of module-level text defines, found the way semble finds them: a
/// definition word, then the name, then a space, one of `<({:[;`, or the end. Returns the
/// names for the general words, as written, and for the SQL words, lowercased.
pub(crate) fn module_definitions(text: &str) -> (Vec<String>, Vec<String>) {
    (scan_names(definition_re(), text, false), scan_names(sql_definition_re(), text, true))
}

fn scan_names(re: &Regex, text: &str, lower: bool) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let Some(caps) = re.captures_at(text, at) else { break };
        let (Some(whole), Some(name)) = (caps.get(0), caps.get(1)) else { break };
        let closes = text[name.end()..].chars().next().map_or(true, |c| c.is_whitespace() || "<({:[;".contains(c));
        if closes {
            let name = if lower { name.as_str().to_lowercase() } else { name.as_str().to_string() };
            if !found.contains(&name) {
                found.push(name);
            }
        }
        // Go on from just after the start of this match, so that a name which is itself a
        // definition word (`typedef struct Node`) still lets the next definition be found.
        at = whole.start() + whole.as_str().chars().next().map_or(1, char::len_utf8);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_queries_are_told_from_natural_language() {
        for symbol in ["SearchIndex", "parse_config", "_private", "Config.parse", "a::b::C", "a\\b", "a->b", "getHTTP", "X"] {
            assert!(is_symbol_query(symbol), "{symbol}");
        }
        for text in ["parse", "parse config", "retry a failed request", "How does Foo work", "", "foo bar_baz"] {
            assert!(!is_symbol_query(text), "{text}");
        }
        assert!(is_symbol_query("  Config  "));
        assert_eq!(auto_alpha("Config"), ALPHA_SYMBOL);
        assert_eq!(auto_alpha("load the config"), ALPHA_TEXT);
    }

    #[test]
    fn symbol_names_split_at_the_last_separator() {
        assert_eq!(symbol_name("Base"), "Base");
        assert_eq!(symbol_name("Sinatra::Base"), "Base");
        assert_eq!(symbol_name("pkg.mod.Class"), "Class");
        assert_eq!(symbol_name("a->b"), "b");
        assert_eq!(symbol_names("Base"), vec!["Base"]);
        assert_eq!(symbol_names("Sinatra::Base"), vec!["Base", "Sinatra::Base", "Sinatra.Base"]);
        assert_eq!(symbol_names("Config.parse"), vec!["parse", "Config.parse"]);
    }

    #[test]
    fn stems_match_plain_joined_and_plural_forms() {
        assert!(stem_matches("config", "config"));
        assert!(stem_matches("configs", "config"));
        assert!(stem_matches("file_slice", "fileslice"));
        assert!(stem_matches("file_slices", "fileslice"));
        assert!(!stem_matches("configuration", "config"));
        assert!(!stem_matches("other", "config"));
    }

    #[test]
    fn embedded_symbols_are_camel_and_pascal_words_only() {
        assert_eq!(embedded_symbols("where is StateManager used by getHTTPResponse or plain words"), vec!["StateManager", "getHTTPResponse"]);
        assert_eq!(embedded_symbols("fooBar and fooBar again"), vec!["fooBar"]);
        assert!(embedded_symbols("HTTP Config parse").is_empty());
        let lower = vec!["statemanager".to_string()];
        assert!(embedded_stem_ok("statemanager", "statemanager", &lower));
        assert!(embedded_stem_ok("state", "state", &lower));
        assert!(embedded_stem_ok("state_manager", "statemanager", &lower));
        assert!(!embedded_stem_ok("sta", "sta", &lower));
        assert!(!embedded_stem_ok("other", "other", &lower));
    }

    #[test]
    fn keywords_drop_short_words_and_stop_words() {
        let words: Vec<String> = keywords("How does the Parser handle a failed request?").into_iter().collect();
        assert_eq!(words, vec!["failed", "handle", "parser", "request"]);
        assert!(keywords("is it a of").is_empty());
    }

    #[test]
    fn paths_give_stems_folders_and_words() {
        assert_eq!(file_stem("a/b/file_slice.py"), "file_slice");
        assert_eq!(file_stem("a/b/archive.tar.gz"), "archive.tar");
        assert_eq!(file_stem(".bashrc"), ".bashrc");
        assert_eq!(file_stem("README"), "README");
        assert_eq!(folder_names("a/b/c.py"), vec!["a", "b"]);
        assert!(folder_names("c.py").is_empty());
        let parts = path_parts("src/index_store/FileSlice.py");
        for word in ["fileslice", "file", "slice", "index_store", "index", "store"] {
            assert!(parts.contains(word), "{word}");
        }
    }

    #[test]
    fn keywords_match_paths_exactly_or_by_a_shared_start() {
        let parts = path_parts("pkg/dependencies.py");
        let words: BTreeSet<String> = ["dependencies", "depend", "dependency", "xyz", "de"].iter().map(|s| s.to_string()).collect();
        // "dependencies" is exact and "depend" starts it. "dependency" is not a start of
        // "dependencies" nor the other way round, and "de" is too short.
        assert_eq!(keyword_matches(&words, &parts), 2);
        assert!(enough_matches(1, 10));
        assert!(!enough_matches(1, 11));
        assert!(!enough_matches(0, 0));
    }

    #[test]
    fn paths_are_penalised_by_what_they_are() {
        assert_eq!(path_penalty("src/app.py"), 1.0);
        assert_eq!(path_penalty("tests/app.py"), 0.3);
        assert_eq!(path_penalty("src/test_app.py"), 0.3);
        assert_eq!(path_penalty("src/app_test.go"), 0.3);
        assert_eq!(path_penalty("web/app.spec.ts"), 0.3);
        assert_eq!(path_penalty("pkg/__init__.py"), 0.5);
        assert_eq!(path_penalty("tests/__init__.py"), 0.3 * 0.5);
        assert_eq!(path_penalty("pkg/compat/old.py"), 0.3);
        assert_eq!(path_penalty("docs_src/a.py"), 0.3);
        assert_eq!(path_penalty("examples/a.py"), 0.3);
        assert_eq!(path_penalty("types/a.d.ts"), 0.7);
        assert_eq!(path_penalty("src\\tests\\a.py"), 0.3);
        assert_eq!(path_penalty("src/contest.py"), 1.0);
    }

    #[test]
    fn module_text_defines_the_names_after_a_definition_word() {
        let text = "namespace Acme.Core\ntypedef struct Node Node;\ndefmodule Phoenix.Router do\nsubclass Fake\nCREATE TABLE users (id int);\ncreate view Report as";
        let (general, sql) = module_definitions(text);
        assert_eq!(general, vec!["Core", "struct", "Node", "Router"]);
        assert_eq!(sql, vec!["users", "report"]);
        let (none, none_sql) = module_definitions("x = classify(y)\nresult = type_of(z)\n");
        assert!(none.is_empty() && none_sql.is_empty());
    }

    #[test]
    fn a_definition_name_must_end_at_a_space_or_a_bracket() {
        let (general, _) = module_definitions("class Foo, Bar\nclass Baz(Base):\nclass Qux:\nclass Quux");
        assert_eq!(general, vec!["Baz", "Qux", "Quux"]);
    }
}
