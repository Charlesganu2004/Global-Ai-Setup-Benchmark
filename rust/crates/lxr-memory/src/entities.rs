//! Entities: the names a note is about.
//!
//! Names are found by rules (code identifiers, paths, backticks, capitalised names) and
//! merged with the ones the bank already knows by Hindsight's formula: half name
//! similarity, three tenths co-occurrence, a fifth recency over seven days.
//!
//! Every merge goes through that formula (or through [`fold`], which makes spelling variants of
//! one name equal). There is no separate merge by name similarity alone among the names of one
//! retain call, so two spellings that were never seen together merge only when recency or
//! co-occurrence backs their likeness.

use lxr_text::trigram_similarity;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Most entities kept for one fact.
pub(crate) const MAX_ENTITIES: usize = 12;
/// Score at or above which a new name is merged into a known entity.
pub(crate) const MERGE_AT: f32 = 0.6;
/// Name similarity below which a known entity is not a candidate at all (pg_trgm's default).
const CANDIDATE_SIMILARITY: f32 = 0.3;
/// Notes of one entity looked at when a recall spreads over entity overlap.
pub(crate) const ENTITY_CAP: usize = 200;

/// The form in which two spellings of one name compare equal: lower case, without `_`, `-`
/// and spaces, with `\` as `/` and `::` as `.`. `parse_config`, `parseConfig` and
/// `Parse-Config` fold to the same string.
pub(crate) fn fold(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut chars = name.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '_' | '-' | ' ' | '\t' => {}
            '\\' => out.push('/'),
            ':' if chars.peek() == Some(&':') => {
                chars.next();
                out.push('.');
            }
            c => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// True for common English words and language keywords, in lower case. These are never
/// taken as entities by themselves.
pub(crate) fn is_common(word: &str) -> bool {
    matches!(
        word,
        // English function words and fillers
        "a" | "about" | "above" | "after" | "again" | "against" | "all" | "also" | "am" | "an" | "and"
            | "any" | "are" | "as" | "at" | "be" | "because" | "been" | "before" | "being" | "below"
            | "between" | "both" | "but" | "by" | "can" | "could" | "did" | "do" | "does" | "doing"
            | "done" | "down" | "during" | "each" | "either" | "else" | "even" | "every" | "few"
            | "first" | "for" | "from" | "further" | "get" | "got" | "had" | "has" | "have" | "having"
            | "he" | "her" | "here" | "hers" | "him" | "his" | "how" | "however" | "i" | "if" | "in"
            | "into" | "is" | "it" | "its" | "itself" | "just" | "last" | "let" | "like" | "made"
            | "make" | "many" | "may" | "me" | "might" | "more" | "most" | "much" | "must" | "my"
            | "new" | "next" | "no" | "nor" | "not" | "now" | "of" | "off" | "often" | "ok" | "old"
            | "on" | "once" | "one" | "only" | "or" | "other" | "our" | "ours" | "out" | "over" | "own"
            | "per" | "same" | "say" | "see" | "set" | "shall" | "she" | "should" | "since" | "so"
            | "some" | "still" | "such" | "take" | "than" | "that" | "the" | "their" | "theirs"
            | "them" | "then" | "there" | "these" | "they" | "this" | "those" | "though" | "through"
            | "thus" | "to" | "today" | "together" | "too" | "tomorrow" | "under" | "until" | "up"
            | "upon" | "us" | "use" | "used" | "using" | "very" | "via" | "want" | "was" | "we"
            | "well" | "were" | "what" | "when" | "where" | "whether" | "which" | "while" | "who"
            | "whom" | "whose" | "why" | "will" | "within" | "without" | "would" | "yes" | "yesterday"
            | "yet" | "you" | "your" | "yours"
            // days and months
            | "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday"
            | "january" | "february" | "march" | "april" | "june" | "july" | "august" | "september"
            | "october" | "november" | "december"
            // language keywords
            | "fn" | "mut" | "pub" | "impl" | "struct" | "enum" | "trait" | "mod" | "self" | "super"
            | "crate" | "loop" | "return" | "def" | "class" | "import" | "lambda" | "yield" | "async"
            | "await" | "none" | "true" | "false" | "null" | "undefined" | "var" | "const" | "function"
            | "typeof" | "switch" | "case" | "break" | "continue" | "int" | "str" | "bool"
            | "void" | "char" | "float" | "double" | "long" | "static" | "public" | "private"
            | "protected" | "final" | "interface" | "package" | "namespace" | "nil" | "func" | "defer"
            | "chan" | "range" | "type" | "dyn" | "unsafe" | "extern" | "move" | "elif" | "pass" | "del"
            | "global" | "nonlocal" | "assert" | "raise" | "try" | "except" | "finally" | "with"
            | "match" | "ref" | "box" | "end" | "begin"
    )
}

/// File extensions that make `name.ext` a file name.
fn is_extension(ext: &str) -> bool {
    matches!(
        ext,
        "rs" | "py" | "pyi" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "go" | "java" | "c" | "h"
            | "cc" | "cpp" | "cxx" | "hpp" | "cs" | "kt" | "swift" | "rb" | "php" | "sh" | "bash"
            | "md" | "txt" | "json" | "jsonl" | "toml" | "yaml" | "yml" | "lock" | "cfg" | "ini"
            | "sql" | "bin" | "xml" | "html" | "css" | "csv" | "log" | "ps1" | "bat"
    )
}

/// The entities named in `text`, in the order found: backtick spans, then code-shaped words
/// (`snake_case`, `camelCase`, `a.b.c`, `a::b`, file paths), then capitalised multi-word
/// names. Each entity once by [`fold`], at most [`MAX_ENTITIES`].
pub(crate) fn extract(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let plain = backtick_spans(text, &mut found);
    for raw in plain.split_whitespace() {
        if let Some(name) = code_word(raw) {
            found.push(name);
        }
    }
    capitalised_names(&plain, &mut found);
    dedupe(found)
}

/// Keeps the first of each spelling (by [`fold`]) and at most [`MAX_ENTITIES`] of them.
pub(crate) fn dedupe(names: Vec<String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for name in names {
        let name = name.trim().to_string();
        let key = fold(&name);
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(name);
        if out.len() == MAX_ENTITIES {
            break;
        }
    }
    out
}

/// Pushes the names inside single-backtick spans onto `found` and returns the text with
/// every span (and every fenced block) blanked out.
fn backtick_spans(text: &str, found: &mut Vec<String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut plain = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '`' {
            plain.push(chars[i]);
            i += 1;
            continue;
        }
        let run = chars[i..].iter().take_while(|&&c| c == '`').count();
        let body_start = i + run;
        let mut j = body_start;
        let mut close = None;
        while j < chars.len() {
            if chars[j] == '`' {
                let r = chars[j..].iter().take_while(|&&c| c == '`').count();
                if r == run {
                    close = Some(j);
                    break;
                }
                j += r;
            } else {
                j += 1;
            }
        }
        plain.push(' ');
        match close {
            Some(j) => {
                if run == 1 {
                    let body: String = chars[body_start..j].iter().collect();
                    if let Some(name) = backtick_name(&body) {
                        found.push(name);
                    }
                }
                i = j + run;
            }
            None => i = body_start,
        }
    }
    plain
}

/// The entity a backtick span names: the span without a call's argument list, unless it is
/// empty, long, a number, a flag or a bare common word.
fn backtick_name(body: &str) -> Option<String> {
    let body = body.trim();
    if body.is_empty() || body.contains('\n') || body.chars().count() > 80 {
        return None;
    }
    let body = match body.find('(') {
        Some(at) if at > 0 => body[..at].trim_end(),
        _ => body,
    };
    let body = body.trim_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '!' | '?'));
    if body.is_empty() || !body.chars().any(char::is_alphanumeric) || body.starts_with('-') {
        return None;
    }
    if body.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let plain_word = body.chars().all(|c| c.is_alphanumeric());
    if plain_word && is_common(&body.to_lowercase()) {
        return None;
    }
    Some(body.replace('\\', "/"))
}

/// The entity a single white-space-free word is, if it is shaped like code.
fn code_word(raw: &str) -> Option<String> {
    let word = raw.trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '\\' | ':' | '~' | '-')));
    let word = word.trim_end_matches(|c: char| matches!(c, '.' | ':' | '-' | ';'));
    let word = word.trim_start_matches(|c: char| matches!(c, ':' | '-' | ';'));
    if word.len() < 2 || word.contains("://") {
        return None;
    }
    if word.contains('/') || word.contains('\\') {
        return path_word(word);
    }
    if word.contains("::") {
        return rust_path(word);
    }
    if word.contains('.') {
        return dotted(word);
    }
    identifier(word)
}

fn path_word(word: &str) -> Option<String> {
    let norm = word.replace('\\', "/");
    let norm = norm.trim_end_matches('/');
    let segments: Vec<&str> = norm.split('/').filter(|s| !s.is_empty()).collect();
    let last = *segments.last()?;
    if !segments.iter().all(|s| s.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '~' | ':'))) {
        return None;
    }
    let slashes = norm.matches('/').count();
    let has_extension = last.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && (1..=6).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric())
    });
    let rooted = norm.starts_with("./") || norm.starts_with("../") || norm.starts_with("~/") || norm.starts_with('/');
    let accept = (has_extension && slashes >= 1) || slashes >= 2 || (rooted && slashes >= 1);
    if !accept || segments.iter().all(|s| is_common(&s.to_lowercase())) {
        return None;
    }
    Some(norm.to_string())
}

fn rust_path(word: &str) -> Option<String> {
    let word = word.trim_start_matches("::");
    let parts: Vec<&str> = word.split("::").collect();
    let ok = parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_') && !p.starts_with(|c: char| c.is_ascii_digit())
        });
    if ok {
        Some(word.to_string())
    } else {
        None
    }
}

fn dotted(word: &str) -> Option<String> {
    let parts: Vec<&str> = word.split('.').collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let ident = |p: &str| {
        p.len() >= 2 && p.starts_with(|c: char| c.is_alphabetic() || c == '_') && p.chars().all(|c| c.is_alphanumeric() || c == '_')
    };
    let last = parts[parts.len() - 1].to_ascii_lowercase();
    if is_extension(&last) {
        let stems_ok = parts[..parts.len() - 1].iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-')));
        return if stems_ok { Some(word.to_string()) } else { None };
    }
    if !parts.iter().all(|p| ident(*p)) || parts.iter().all(|p| is_common(&p.to_lowercase())) {
        return None;
    }
    Some(word.to_string())
}

fn identifier(word: &str) -> Option<String> {
    if !word.chars().all(|c| c.is_alphanumeric() || c == '_') || !word.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        return None;
    }
    let core = word.trim_matches('_');
    if core.len() < 2 {
        return None;
    }
    let snake = core.contains('_');
    let dunder = word.starts_with("__") && word.ends_with("__") && core.len() >= 2;
    let chars: Vec<char> = word.chars().collect();
    let camel = chars.len() >= 3 && chars.windows(2).any(|w| w[0].is_lowercase() && w[1].is_uppercase());
    if snake || dunder || camel {
        Some(word.to_string())
    } else {
        None
    }
}

/// Pushes runs of two or more capitalised words (`Saint Martin's University`) onto `found`.
/// A run ends at a word that carries punctuation after it. Leading filler such as `The` or
/// `Yesterday` is dropped from a run first.
fn capitalised_names(text: &str, found: &mut Vec<String>) {
    let mut run: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let ends = word.ends_with(|c: char| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"'));
        let core = word.trim_matches(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '\u{2019}')).replace('\u{2019}', "'");
        if is_capitalised(&core) {
            run.push(core);
            if ends {
                flush_run(&mut run, found);
            }
        } else {
            flush_run(&mut run, found);
        }
    }
    flush_run(&mut run, found);
}

fn is_capitalised(word: &str) -> bool {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => {}
        _ => return false,
    }
    word.chars().skip(1).any(char::is_lowercase) && word.chars().all(|c| c.is_alphanumeric() || c == '\'')
}

fn flush_run(run: &mut Vec<String>, found: &mut Vec<String>) {
    while run.first().is_some_and(|w| is_common(&w.to_lowercase())) {
        run.remove(0);
    }
    if let Some(last) = run.last_mut() {
        if let Some(stem) = last.strip_suffix("'s") {
            *last = stem.to_string();
        }
    }
    if run.len() >= 2 && run.len() <= 6 {
        found.push(run.join(" "));
    }
    run.clear();
}

/// What the bank knows about one entity.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub(crate) struct EntityRec {
    /// The name as it was first seen: the one shown.
    pub name: String,
    pub first_seen: i64,
    pub last_seen: i64,
    /// Notes (not observations) that mention it, counting repeats once.
    pub mentions: u32,
    /// Slots of the notes that mention it, observations included, oldest first.
    pub notes: Vec<u32>,
}

/// All entities of a bank, with the lookups the retain step needs.
#[derive(Default)]
pub(crate) struct EntityTable {
    pub recs: Vec<EntityRec>,
    by_key: HashMap<String, u32>,
    /// Trigram to the entities whose names have it: finds merge candidates quickly.
    tri: HashMap<u64, Vec<u32>>,
    /// Pairs of entities (lower id first) that were mentioned together: count and last time.
    cooc: HashMap<(u32, u32), (u32, i64)>,
}

impl EntityTable {
    /// Rebuilds a table from stored parts.
    pub fn from_parts(recs: Vec<EntityRec>, cooc: Vec<(u32, u32, u32, i64)>) -> EntityTable {
        let mut table = EntityTable { recs, ..EntityTable::default() };
        for id in 0..table.recs.len() {
            let name = table.recs[id].name.clone();
            table.by_key.insert(fold(&name), id as u32);
            table.index_trigrams(id as u32, &name);
        }
        table.cooc = cooc.into_iter().map(|(a, b, n, at)| ((a, b), (n, at))).collect();
        table
    }

    /// The records and the co-occurrence counts, for storing.
    pub fn to_parts(&self) -> (Vec<EntityRec>, Vec<(u32, u32, u32, i64)>) {
        let mut cooc: Vec<(u32, u32, u32, i64)> = self.cooc.iter().map(|(&(a, b), &(n, at))| (a, b, n, at)).collect();
        cooc.sort_unstable();
        (self.recs.clone(), cooc)
    }

    /// True when every id inside the table points at a record.
    pub fn is_consistent(&self, slots: usize) -> bool {
        let n = self.recs.len() as u32;
        self.cooc.keys().all(|&(a, b)| a < n && b < n) && self.recs.iter().all(|r| r.notes.iter().all(|&s| (s as usize) < slots))
    }

    /// The id of the entity whose name folds to the same string as `name`.
    pub fn id_of(&self, name: &str) -> Option<u32> {
        self.by_key.get(&fold(name)).copied()
    }

    /// The entity `name` belongs to, if the bank knows it: the one with the same folded name,
    /// else the best known entity whose score `0.5 x name similarity + 0.3 x co-occurrence +
    /// 0.2 x recency over seven days` reaches 0.6. `nearby` are the other entities of the
    /// same fact.
    pub fn resolve(&self, name: &str, nearby: &[u32], now: i64) -> Option<u32> {
        if let Some(id) = self.id_of(name) {
            return Some(id);
        }
        let mut best: Option<(f32, u32)> = None;
        for id in self.candidates(name) {
            let rec = &self.recs[id as usize];
            let similarity = trigram_similarity(&rec.name, name);
            if similarity < CANDIDATE_SIMILARITY {
                continue;
            }
            let score = 0.5 * similarity + 0.3 * self.cooccurrence(id, nearby) + 0.2 * recency(rec.last_seen, now);
            if score >= MERGE_AT && best.map_or(true, |(top, _)| score > top) {
                best = Some((score, id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// The score of a candidate by co-occurrence: the share of the nearby entities it was
    /// seen together with.
    fn cooccurrence(&self, id: u32, nearby: &[u32]) -> f32 {
        let others: Vec<u32> = nearby.iter().copied().filter(|&o| o != id).collect();
        if others.is_empty() {
            return 0.0;
        }
        let together = others.iter().filter(|&&o| self.cooc.contains_key(&(id.min(o), id.max(o)))).count();
        together as f32 / others.len() as f32
    }

    fn candidates(&self, name: &str) -> Vec<u32> {
        let mut shared: HashMap<u32, u32> = HashMap::new();
        for key in trigram_keys(name) {
            if let Some(list) = self.tri.get(&key) {
                if list.len() <= 2000 {
                    for &id in list {
                        *shared.entry(id).or_insert(0) += 1;
                    }
                }
            }
        }
        let mut ranked: Vec<(u32, u32)> = shared.into_iter().filter(|&(_, n)| n >= 2).collect();
        ranked.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked.into_iter().take(24).map(|(id, _)| id).collect()
    }

    fn index_trigrams(&mut self, id: u32, name: &str) {
        for key in trigram_keys(name) {
            self.tri.entry(key).or_default().push(id);
        }
    }

    /// Adds an entity first seen at `now` and returns its id. A name that folds to a known
    /// one returns that one's id.
    pub fn create(&mut self, name: &str, now: i64) -> u32 {
        if let Some(id) = self.id_of(name) {
            return id;
        }
        let id = self.recs.len() as u32;
        let name = name.trim().to_string();
        self.by_key.insert(fold(&name), id);
        self.index_trigrams(id, &name);
        self.recs.push(EntityRec { name, first_seen: now, last_seen: now, mentions: 0, notes: Vec::new() });
        id
    }

    /// Records that the note in `slot` mentions `ids`: the note joins each entity's list, an
    /// ordinary note counts as a mention, and every pair of the entities counts as together.
    pub fn attach(&mut self, ids: &[u32], slot: u32, now: i64, mention: bool) {
        for &id in ids {
            let rec = &mut self.recs[id as usize];
            if rec.notes.last().map_or(true, |&last| last < slot) {
                rec.notes.push(slot);
            } else if let Err(at) = rec.notes.binary_search(&slot) {
                rec.notes.insert(at, slot);
            }
            if mention {
                rec.mentions += 1;
                rec.last_seen = rec.last_seen.max(now);
            }
        }
        if mention {
            for (n, &a) in ids.iter().enumerate() {
                for &b in &ids[n + 1..] {
                    let entry = self.cooc.entry((a.min(b), a.max(b))).or_insert((0, now));
                    entry.0 += 1;
                    entry.1 = entry.1.max(now);
                }
            }
        }
    }

    /// Takes the note in `slot` out of the entities' lists.
    pub fn detach(&mut self, ids: &[u32], slot: u32, mention: bool) {
        for &id in ids {
            let rec = &mut self.recs[id as usize];
            if let Ok(at) = rec.notes.binary_search(&slot) {
                rec.notes.remove(at);
            }
            if mention {
                rec.mentions = rec.mentions.saturating_sub(1);
            }
        }
    }

    /// Number of entities that at least one note mentions.
    pub fn live(&self) -> usize {
        self.recs.iter().filter(|r| !r.notes.is_empty()).count()
    }
}

/// `max(0, 1 - days / 7)` for an entity last seen at `last_seen`.
fn recency(last_seen: i64, now: i64) -> f32 {
    let days = (now - last_seen).max(0) as f32 / 86_400.0;
    (1.0 - days / 7.0).max(0.0)
}

/// The distinct trigrams of a name as pg_trgm cuts them (lower case, words padded with two
/// spaces in front and one behind), packed into numbers.
fn trigram_keys(name: &str) -> Vec<u64> {
    let lower = name.to_lowercase();
    let mut keys = Vec::new();
    for word in lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        let padded: Vec<char> = "  ".chars().chain(word.chars()).chain(" ".chars()).collect();
        for w in padded.windows(3) {
            keys.push((u64::from(w[0] as u32) << 42) | (u64::from(w[1] as u32) << 21) | u64::from(w[2] as u32));
        }
    }
    keys.sort_unstable();
    keys.dedup();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_makes_spelling_variants_equal() {
        assert_eq!(fold("parse_config"), fold("parseConfig"));
        assert_eq!(fold("Parse-Config"), fold("parse_config"));
        assert_eq!(fold("crate::store"), fold("crate.store"));
        assert_eq!(fold("src\\lib.rs"), "src/lib.rs");
        assert_ne!(fold("parse_config"), fold("parse_confg"));
    }

    #[test]
    fn snake_camel_and_pascal_case_are_entities() {
        let names = extract("The loader calls parse_config and then loadFile on a UserService. Also MAX_RETRIES and __init__.");
        for expected in ["parse_config", "loadFile", "UserService", "MAX_RETRIES", "__init__"] {
            assert!(names.iter().any(|n| n == expected), "{expected} missing from {names:?}");
        }
    }

    #[test]
    fn dotted_and_double_colon_paths_are_entities() {
        let names = extract("It uses self.cache.load and crate::store::save, not e.g. this or 3.5 or v1.2.3.");
        assert!(names.contains(&"self.cache.load".to_string()), "{names:?}");
        assert!(names.contains(&"crate::store::save".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.contains("e.g") || n.contains("3.5") || n.contains("v1")), "{names:?}");
    }

    #[test]
    fn backtick_spans_are_entities_without_their_arguments() {
        let names = extract("Call `Bank::open(dir)` first, then `retain`; `fn` and `the` are not names, nor is `42`.");
        assert!(names.contains(&"Bank::open".to_string()), "{names:?}");
        assert!(names.contains(&"retain".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n == "fn" || n == "the" || n == "42"), "{names:?}");
    }

    #[test]
    fn file_paths_and_file_names_are_entities() {
        let names = extract("Edit crates/lxr-memory/src/lib.rs and also lib.rs in C:\\work\\notes.jsonl, but not and/or.");
        assert!(names.contains(&"crates/lxr-memory/src/lib.rs".to_string()), "{names:?}");
        assert!(names.contains(&"lib.rs".to_string()), "{names:?}");
        assert!(names.contains(&"C:/work/notes.jsonl".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n == "and/or"), "{names:?}");
    }

    #[test]
    fn capitalised_multi_word_names_are_entities() {
        let names = extract("Yesterday Charles Ganu met Dr. Radana Dvorak at Saint Martin's University. The team agreed.");
        assert!(names.contains(&"Charles Ganu".to_string()), "{names:?}");
        assert!(names.contains(&"Radana Dvorak".to_string()), "{names:?}");
        assert!(names.contains(&"Saint Martin's University".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("Yesterday") || n.starts_with("The")), "{names:?}");
    }

    #[test]
    fn common_words_and_single_capitalised_words_are_not_entities() {
        assert!(extract("The cat sat on the mat. Then it left, and Python is fine.").is_empty());
    }

    #[test]
    fn each_entity_appears_once_by_fold() {
        let names = extract("`parse_config`, parseConfig and parse_config again");
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn at_most_twelve_entities_are_kept() {
        let text: String = (0..30).map(|n| format!("name_{n}a ")).collect();
        assert_eq!(extract(&text).len(), MAX_ENTITIES);
    }

    fn table_with(names: &[&str], now: i64) -> EntityTable {
        let mut table = EntityTable::default();
        for (n, name) in names.iter().enumerate() {
            let id = table.create(name, now);
            table.attach(&[id], n as u32, now, true);
        }
        table
    }

    #[test]
    fn a_folded_equal_name_resolves_to_the_known_entity() {
        let table = table_with(&["parse_config"], 1_000);
        assert_eq!(table.resolve("parseConfig", &[], 9_999_999), Some(0));
        assert_eq!(table.id_of("PARSE-CONFIG"), Some(0));
    }

    #[test]
    fn a_misspelling_merges_only_when_the_formula_reaches_the_threshold() {
        let mut table = table_with(&["parse_config", "loader"], 1_000);
        let (parse, loader) = (0u32, 1u32);
        let sim = trigram_similarity("parse_config", "parse_confg");
        assert!(sim >= CANDIDATE_SIMILARITY, "similarity {sim}");
        // Seen together before: co-occurrence 1.0, recent: 0.5 x sim + 0.3 + 0.2 >= 0.6.
        table.attach(&[parse, loader], 5, 1_000, true);
        assert_eq!(table.resolve("parse_confg", &[loader], 1_000), Some(parse));
        // Nothing nearby and the entity last seen a week ago: 0.5 x sim alone is far below 0.6.
        assert_eq!(table.resolve("parse_confg", &[], 1_000 + 8 * 86_400), None);
        // Recent but nothing nearby: 0.5 x sim + 0.2 stays below 0.6 for a similarity under 0.8.
        // The two spellings share 10 of 15 trigrams, 2/3, so the score is 0.533 and they stay apart.
        assert!((sim - 2.0 / 3.0).abs() < 0.01, "similarity {sim}");
        assert_eq!(table.resolve("parse_confg", &[], 1_000), None);
    }

    #[test]
    fn unrelated_names_never_merge() {
        let table = table_with(&["parse_config", "loader"], 1_000);
        assert_eq!(table.resolve("render_html", &[1], 1_000), None);
        assert_eq!(table.resolve("zebra", &[0, 1], 1_000), None);
    }

    #[test]
    fn attach_and_detach_keep_the_lists_and_counts() {
        let mut table = table_with(&["alpha_one"], 10);
        let id = 0;
        table.attach(&[id], 7, 20, true);
        table.attach(&[id], 3, 30, false);
        assert_eq!(table.recs[0].notes, vec![0, 3, 7]);
        assert_eq!(table.recs[0].mentions, 2);
        assert_eq!(table.recs[0].last_seen, 20);
        table.detach(&[id], 3, false);
        table.detach(&[id], 7, true);
        assert_eq!(table.recs[0].notes, vec![0]);
        assert_eq!(table.recs[0].mentions, 1);
        assert_eq!(table.live(), 1);
    }

    #[test]
    fn parts_round_trip() {
        let mut table = table_with(&["alpha_one", "beta_two"], 10);
        table.attach(&[0, 1], 4, 11, true);
        let (recs, cooc) = table.to_parts();
        let again = EntityTable::from_parts(recs.clone(), cooc.clone());
        assert_eq!(again.to_parts(), (recs, cooc));
        assert_eq!(again.id_of("BetaTwo"), Some(1));
        assert!(again.is_consistent(8));
        assert!(!again.is_consistent(2));
    }
}
