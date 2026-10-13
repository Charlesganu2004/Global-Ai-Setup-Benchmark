//! A WordPiece tokenizer that matches the Hugging Face `tokenizers` pipeline of a BERT
//! style model: added-token splitting, BERT normaliser, BERT pre-tokenizer, WordPiece.
//!
//! The vocabulary is kept in a flat layout (one byte blob, an offset per id, and an
//! open-addressing hash table of ids) so that a cache file holds it ready to use and
//! loading it takes no parsing.

use crate::unicode_data::{CONTROL, MARKS, PUNCT};
use lxr_core::store::{hash64, write_atomic};
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

const CACHE_MAGIC: &[u8; 8] = b"LXRTOK02";

/// What the cache file is keyed by: the size and modification time of `tokenizer.json`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CacheKey {
    size: u64,
    secs: u64,
    nanos: u32,
}

impl CacheKey {
    pub(crate) fn of(path: &Path) -> io::Result<CacheKey> {
        let meta = fs::metadata(path)?;
        let since = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .unwrap_or_default();
        Ok(CacheKey { size: meta.len(), secs: since.as_secs(), nanos: since.subsec_nanos() })
    }
}

/// The text-processing settings read from `tokenizer.json`.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Settings {
    clean_text: bool,
    handle_chinese: bool,
    strip_accents: bool,
    lowercase: bool,
    /// Tokens kept per text by the tokenizer itself, if it truncates.
    truncation: Option<usize>,
    /// A word longer than this many characters becomes the unknown token.
    max_input_chars: usize,
    /// Marks a WordPiece that continues a word.
    prefix: String,
    unk_id: Option<u32>,
    /// The id that batches are padded with, when the tokenizer pads.
    pad_id: Option<u32>,
    /// Median length, in characters, of the vocabulary entries.
    median_len: usize,
    /// Length in bytes of the longest vocabulary entry.
    max_token_bytes: usize,
}

/// A token that is recognised in the text before any other processing.
#[derive(Clone, PartialEq, Debug)]
struct AddedToken {
    id: u32,
    content: String,
    single_word: bool,
    normalized: bool,
}

/// The vocabulary: entries by id, and a hash table from entry to id.
#[derive(Clone, Debug)]
struct Vocab {
    blob: Vec<u8>,
    /// `offs[id]..offs[id + 1]` is the entry of `id` in `blob`; empty when the id is unused.
    offs: Vec<u32>,
    /// Open addressing, linear probing. 0 is an empty slot, otherwise `id + 1`.
    table: Vec<u32>,
}

impl Vocab {
    fn build(by_id: &[Option<String>]) -> Vocab {
        let mut blob = Vec::new();
        let mut offs = Vec::with_capacity(by_id.len() + 1);
        for entry in by_id {
            offs.push(blob.len() as u32);
            if let Some(token) = entry {
                blob.extend_from_slice(token.as_bytes());
            }
        }
        offs.push(blob.len() as u32);
        let mut table = vec![0u32; (by_id.len() * 2).next_power_of_two().max(8)];
        let mask = table.len() - 1;
        for (id, entry) in by_id.iter().enumerate() {
            let Some(token) = entry else { continue };
            if token.is_empty() {
                continue;
            }
            let mut slot = hash64(token.as_bytes()) as usize & mask;
            while table[slot] != 0 {
                slot = (slot + 1) & mask;
            }
            table[slot] = id as u32 + 1;
        }
        Vocab { blob, offs, table }
    }

    fn len(&self) -> usize {
        self.offs.len().saturating_sub(1)
    }

    fn token(&self, id: u32) -> &[u8] {
        &self.blob[self.offs[id as usize] as usize..self.offs[id as usize + 1] as usize]
    }

    fn get(&self, key: &[u8]) -> Option<u32> {
        let mask = self.table.len() - 1;
        let mut slot = hash64(key) as usize & mask;
        loop {
            let entry = self.table[slot];
            if entry == 0 {
                return None;
            }
            if self.token(entry - 1) == key {
                return Some(entry - 1);
            }
            slot = (slot + 1) & mask;
        }
    }
}

/// The tokenizer: settings, added tokens and vocabulary.
#[derive(Clone, Debug)]
pub(crate) struct Tokenizer {
    settings: Settings,
    added: Vec<AddedToken>,
    /// For a token that is matched in normalised text, its normalised form.
    added_norm: Vec<String>,
    vocab: Vocab,
}

fn bad(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn in_ranges(table: &[(u32, u32)], c: u32) -> bool {
    table
        .binary_search_by(|&(low, high)| {
            if c < low {
                std::cmp::Ordering::Greater
            } else if c > high {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// A character that `clean_text` removes.
fn is_control(c: char) -> bool {
    if c >= ' ' && c < '\u{7f}' {
        return false;
    }
    in_ranges(CONTROL, c as u32)
}

/// The ranges the Hugging Face normaliser treats as Chinese characters.
fn is_chinese(c: char) -> bool {
    matches!(
        c as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B920..=0x2CEAF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
    )
}

/// A character the pre-tokenizer splits off as a word of its own.
fn is_split_punct(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_punctuation()
    } else {
        in_ranges(PUNCT, c as u32)
    }
}

/// True for a character that is part of a word in the sense of the regex `\w`.
fn is_word_char(c: char) -> bool {
    static WORD: OnceLock<regex::Regex> = OnceLock::new();
    let re = WORD.get_or_init(|| regex::Regex::new(r"^\w$").expect("constant pattern"));
    let mut buf = [0u8; 4];
    re.is_match(c.encode_utf8(&mut buf))
}

impl Tokenizer {
    /// Reads the settings, added tokens and vocabulary from the text of `tokenizer.json`.
    pub(crate) fn from_json(bytes: &[u8]) -> io::Result<Tokenizer> {
        let root: Value = serde_json::from_slice(bytes).map_err(|e| bad(format!("tokenizer.json is not valid JSON: {e}")))?;
        let model = root.get("model").ok_or_else(|| bad("tokenizer.json has no model section"))?;
        match model.get("type").and_then(Value::as_str) {
            Some("WordPiece") => {}
            Some(other) => {
                return Err(bad(format!(
                    "tokenizer.json: model type {other:?} is not supported, only WordPiece"
                )))
            }
            None => return Err(bad("tokenizer.json: the model has no type, only WordPiece is supported")),
        }
        let (clean_text, handle_chinese, strip_accents, lowercase) = parse_normalizer(root.get("normalizer"))?;
        match root.get("pre_tokenizer") {
            Some(Value::Object(obj)) if obj.get("type").and_then(Value::as_str) == Some("BertPreTokenizer") => {}
            other => {
                return Err(bad(format!(
                    "tokenizer.json: pre-tokenizer {} is not supported, only BertPreTokenizer",
                    describe(other)
                )))
            }
        }
        let truncation = match root.get("truncation") {
            None | Some(Value::Null) => None,
            Some(obj) => {
                if obj.get("direction").and_then(Value::as_str).is_some_and(|d| d != "Right") {
                    return Err(bad("tokenizer.json: only truncation from the right is supported"));
                }
                obj.get("max_length").and_then(Value::as_u64).map(|n| n as usize)
            }
        };
        let vocab_json = model
            .get("vocab")
            .and_then(Value::as_object)
            .ok_or_else(|| bad("tokenizer.json: the model has no vocab"))?;
        let mut max_id = 0u32;
        let mut entries: Vec<(&str, u32)> = Vec::with_capacity(vocab_json.len());
        for (token, id) in vocab_json {
            let id = id
                .as_u64()
                .filter(|&n| n < 100_000_000)
                .ok_or_else(|| bad(format!("tokenizer.json: bad id for token {token:?}")))? as u32;
            max_id = max_id.max(id);
            entries.push((token.as_str(), id));
        }
        if entries.is_empty() {
            return Err(bad("tokenizer.json: the vocabulary is empty"));
        }
        // The tables below are indexed by id, so a few entries with one huge id would ask
        // for gigabytes. A real vocabulary is dense; a few unused ids are allowed.
        if max_id as usize >= entries.len().saturating_mul(2).saturating_add(1024) {
            return Err(bad(format!(
                "tokenizer.json: the largest token id {max_id} is far above the {} vocabulary entries",
                entries.len()
            )));
        }
        let mut by_id: Vec<Option<String>> = vec![None; max_id as usize + 1];
        for (token, id) in &entries {
            by_id[*id as usize] = Some((*token).to_string());
        }
        let unk_token = model.get("unk_token").and_then(Value::as_str).unwrap_or("[UNK]");
        let unk_id = vocab_json.get(unk_token).and_then(Value::as_u64).map(|n| n as u32);
        let prefix = model
            .get("continuing_subword_prefix")
            .and_then(Value::as_str)
            .unwrap_or("##")
            .to_string();
        let max_input_chars = model.get("max_input_chars_per_word").and_then(Value::as_u64).unwrap_or(100) as usize;
        let mut lengths: Vec<usize> = by_id.iter().flatten().map(|t| t.chars().count()).collect();
        lengths.sort_unstable();
        let median_len = match lengths.len() {
            0 => 0,
            n if n % 2 == 1 => lengths[n / 2],
            n => (lengths[n / 2 - 1] + lengths[n / 2]) / 2,
        };
        let max_token_bytes = by_id.iter().flatten().map(String::len).max().unwrap_or(0);
        let mut added = Vec::new();
        if let Some(list) = root.get("added_tokens").and_then(Value::as_array) {
            for item in list {
                let (Some(id), Some(content)) = (
                    item.get("id").and_then(Value::as_u64),
                    item.get("content").and_then(Value::as_str),
                ) else {
                    continue;
                };
                if content.is_empty() {
                    continue;
                }
                let id = u32::try_from(id).map_err(|_| bad(format!("tokenizer.json: the id of added token {content:?} is too large")))?;
                added.push(AddedToken {
                    id,
                    content: content.to_string(),
                    single_word: item.get("single_word").and_then(Value::as_bool).unwrap_or(false),
                    normalized: item.get("normalized").and_then(Value::as_bool).unwrap_or(false),
                });
            }
        }
        let pad_id = match root.get("padding") {
            None | Some(Value::Null) => None,
            Some(obj) => {
                let id = obj.get("pad_id").and_then(Value::as_u64).unwrap_or(0);
                Some(u32::try_from(id).map_err(|_| bad("tokenizer.json: the padding id is too large"))?)
            }
        };
        let settings = Settings {
            clean_text,
            handle_chinese,
            strip_accents,
            lowercase,
            truncation,
            max_input_chars,
            prefix,
            unk_id,
            pad_id,
            median_len,
            max_token_bytes,
        };
        Ok(Tokenizer::assemble(settings, added, Vocab::build(&by_id)))
    }

    fn assemble(settings: Settings, added: Vec<AddedToken>, vocab: Vocab) -> Tokenizer {
        let mut tokenizer = Tokenizer { settings, added, added_norm: Vec::new(), vocab };
        tokenizer.added_norm = tokenizer
            .added
            .iter()
            .map(|t| if t.normalized { tokenizer.normalize(&t.content) } else { t.content.clone() })
            .collect();
        tokenizer
    }

    /// Loads the tokenizer of `tokenizer_json`, from the cache file in `cache_dir` when it
    /// is current and otherwise from the JSON, writing the cache afterwards. The flag is
    /// true when the cache was used.
    pub(crate) fn load(tokenizer_json: &Path, cache_dir: &Path) -> io::Result<(Tokenizer, bool)> {
        let key = CacheKey::of(tokenizer_json)?;
        let cache = cache_file(tokenizer_json, cache_dir);
        if let Ok(bytes) = fs::read(&cache) {
            if let Some(tokenizer) = Tokenizer::from_cache(&bytes, &key) {
                return Ok((tokenizer, true));
            }
        }
        let tokenizer = Tokenizer::from_json(&fs::read(tokenizer_json)?)?;
        // A cache that cannot be written only costs the next start some time.
        let _ = write_atomic(&cache, &tokenizer.to_cache(&key));
        Ok((tokenizer, false))
    }

    /// Number of ids in the vocabulary, counting unused ids below the highest one.
    pub(crate) fn vocab_len(&self) -> usize {
        self.vocab.len()
    }

    /// One above the highest id the tokenizer can give or pad with: the vocabulary ids,
    /// the added tokens and the padding id. The embedding table needs a row for each.
    pub(crate) fn id_bound(&self) -> usize {
        let added = self.added.iter().map(|t| (t.id as usize).saturating_add(1)).max().unwrap_or(0);
        let pad = self.settings.pad_id.map_or(0, |id| (id as usize).saturating_add(1));
        self.vocab_len().max(added).max(pad)
    }

    pub(crate) fn unk_id(&self) -> Option<u32> {
        self.settings.unk_id
    }

    pub(crate) fn pad_id(&self) -> Option<u32> {
        self.settings.pad_id
    }

    /// Median vocabulary entry length in characters: the unit of the query length cut.
    pub(crate) fn median_len(&self) -> usize {
        self.settings.median_len
    }

    // ---- cache file ----

    fn to_cache(&self, key: &CacheKey) -> Vec<u8> {
        let s = &self.settings;
        let mut out = Vec::with_capacity(self.vocab.blob.len() + self.vocab.offs.len() * 4 + self.vocab.table.len() * 4 + 256);
        out.extend_from_slice(CACHE_MAGIC);
        out.extend_from_slice(&key.size.to_le_bytes());
        out.extend_from_slice(&key.secs.to_le_bytes());
        out.extend_from_slice(&key.nanos.to_le_bytes());
        let flags = s.clean_text as u32
            | (s.handle_chinese as u32) << 1
            | (s.strip_accents as u32) << 2
            | (s.lowercase as u32) << 3
            | (s.truncation.is_some() as u32) << 4
            | (s.unk_id.is_some() as u32) << 5
            | (s.pad_id.is_some() as u32) << 6;
        out.extend_from_slice(&flags.to_le_bytes());
        for value in [
            s.truncation.unwrap_or(0) as u32,
            s.max_input_chars as u32,
            s.unk_id.unwrap_or(0),
            s.pad_id.unwrap_or(0),
            s.median_len as u32,
            s.max_token_bytes as u32,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        put_string(&mut out, &s.prefix);
        out.extend_from_slice(&(self.added.len() as u32).to_le_bytes());
        for token in &self.added {
            out.extend_from_slice(&token.id.to_le_bytes());
            out.extend_from_slice(&(token.single_word as u32 | (token.normalized as u32) << 1).to_le_bytes());
            put_string(&mut out, &token.content);
        }
        out.extend_from_slice(&((self.vocab.offs.len() - 1) as u32).to_le_bytes());
        out.extend_from_slice(&(self.vocab.blob.len() as u32).to_le_bytes());
        for off in &self.vocab.offs {
            out.extend_from_slice(&off.to_le_bytes());
        }
        out.extend_from_slice(&self.vocab.blob);
        out.extend_from_slice(&(self.vocab.table.len() as u32).to_le_bytes());
        for slot in &self.vocab.table {
            out.extend_from_slice(&slot.to_le_bytes());
        }
        let checksum = hash64(&out);
        out.extend_from_slice(&checksum.to_le_bytes());
        out
    }

    /// Parses a cache file. `None` for a file that is damaged, from another layout, or
    /// made for a different `tokenizer.json`.
    fn from_cache(bytes: &[u8], key: &CacheKey) -> Option<Tokenizer> {
        let body_len = bytes.len().checked_sub(8)?;
        let (body, tail) = bytes.split_at(body_len);
        if hash64(body) != u64::from_le_bytes(tail.try_into().ok()?) {
            return None;
        }
        let mut r = Reader { b: body, at: 0 };
        if r.take(8)? != CACHE_MAGIC {
            return None;
        }
        let stored = CacheKey { size: r.u64()?, secs: r.u64()?, nanos: r.u32()? };
        if stored != *key {
            return None;
        }
        let flags = r.u32()?;
        let truncation = r.u32()? as usize;
        let max_input_chars = r.u32()? as usize;
        let unk = r.u32()?;
        let pad = r.u32()?;
        let median_len = r.u32()? as usize;
        let max_token_bytes = r.u32()? as usize;
        let prefix = r.string()?;
        let added_count = r.u32()? as usize;
        let mut added = Vec::new();
        for _ in 0..added_count {
            let id = r.u32()?;
            let bits = r.u32()?;
            let content = r.string()?;
            added.push(AddedToken { id, content, single_word: bits & 1 != 0, normalized: bits & 2 != 0 });
        }
        let vocab_len = r.u32()? as usize;
        let blob_len = r.u32()? as usize;
        let offs = r.u32s(vocab_len.checked_add(1)?)?;
        let blob = r.take(blob_len)?.to_vec();
        let table_len = r.u32()? as usize;
        let table = r.u32s(table_len)?;
        if r.at != body.len() || !table_len.is_power_of_two() {
            return None;
        }
        if offs.first().copied()? != 0 || offs.last().copied()? as usize != blob.len() || offs.windows(2).any(|w| w[0] > w[1]) {
            return None;
        }
        if table.iter().any(|&slot| slot as usize > vocab_len) || !table.contains(&0) {
            return None;
        }
        let settings = Settings {
            clean_text: flags & 1 != 0,
            handle_chinese: flags & 2 != 0,
            strip_accents: flags & 4 != 0,
            lowercase: flags & 8 != 0,
            truncation: (flags & 16 != 0).then_some(truncation),
            max_input_chars,
            prefix,
            unk_id: (flags & 32 != 0).then_some(unk),
            pad_id: (flags & 64 != 0).then_some(pad),
            median_len,
            max_token_bytes,
        };
        Some(Tokenizer::assemble(settings, added, Vocab { blob, offs, table }))
    }

    // ---- text processing ----

    /// The BERT normaliser: clean text, space out Chinese characters, strip accents,
    /// lowercase, in that order.
    pub(crate) fn normalize(&self, text: &str) -> String {
        let s = &self.settings;
        if text.is_ascii() {
            let mut out = String::with_capacity(text.len());
            for &b in text.as_bytes() {
                let mut c = b as char;
                if s.clean_text {
                    if is_control(c) {
                        continue;
                    }
                    if c.is_whitespace() {
                        c = ' ';
                    }
                }
                if s.lowercase {
                    c = c.to_ascii_lowercase();
                }
                out.push(c);
            }
            return out;
        }
        let mut cleaned = String::with_capacity(text.len());
        for c in text.chars() {
            if s.clean_text {
                if is_control(c) {
                    continue;
                }
                cleaned.push(if c.is_whitespace() { ' ' } else { c });
            } else {
                cleaned.push(c);
            }
        }
        let spaced = if s.handle_chinese {
            let mut out = String::with_capacity(cleaned.len() + 8);
            for c in cleaned.chars() {
                if is_chinese(c) {
                    out.push(' ');
                    out.push(c);
                    out.push(' ');
                } else {
                    out.push(c);
                }
            }
            out
        } else {
            cleaned
        };
        let stripped: String = if s.strip_accents {
            spaced.nfd().filter(|&c| !in_ranges(MARKS, c as u32)).collect()
        } else {
            spaced
        };
        if s.lowercase {
            stripped.chars().flat_map(char::to_lowercase).collect()
        } else {
            stripped
        }
    }

    /// The ids of `text` as the tokenizer alone gives them: unknown words are the
    /// unknown token, and at most the truncation limit of ids is returned.
    pub(crate) fn encode(&self, text: &str) -> Vec<u32> {
        let limit = self.settings.truncation.unwrap_or(usize::MAX);
        let mut out = Vec::new();
        let mut at = 0;
        // Added tokens that are matched in the text as written.
        while out.len() < limit {
            match self.find_added(text, at, false) {
                Some((start, end, id)) => {
                    self.encode_plain(&text[at..start], &mut out, limit);
                    if out.len() < limit {
                        out.push(id);
                    }
                    at = end;
                }
                None => {
                    self.encode_plain(&text[at..], &mut out, limit);
                    break;
                }
            }
        }
        out.truncate(limit);
        out
    }

    /// A stretch of text with no added token of the first kind in it.
    fn encode_plain(&self, piece: &str, out: &mut Vec<u32>, limit: usize) {
        let norm = self.normalize(piece);
        let mut at = 0;
        while out.len() < limit {
            match self.find_added(&norm, at, true) {
                Some((start, end, id)) => {
                    self.words(&norm[at..start], out, limit);
                    if out.len() < limit {
                        out.push(id);
                    }
                    at = end;
                }
                None => {
                    self.words(&norm[at..], out, limit);
                    break;
                }
            }
        }
    }

    /// The first added token of the given kind at or after `from`: `(start, end, id)`.
    /// A token marked `single_word` only matches when no word character touches it.
    fn find_added(&self, text: &str, from: usize, normalized: bool) -> Option<(usize, usize, u32)> {
        let mut best: Option<(usize, usize, u32)> = None;
        for (token, content) in self.added.iter().zip(&self.added_norm) {
            if token.normalized != normalized || content.is_empty() {
                continue;
            }
            let mut search = from;
            while let Some(found) = text[search..].find(content.as_str()) {
                let start = search + found;
                let end = start + content.len();
                if token.single_word {
                    let before = text[..start].chars().next_back().is_some_and(is_word_char);
                    let after = text[end..].chars().next().is_some_and(is_word_char);
                    if before || after {
                        search = end;
                        continue;
                    }
                }
                let better = match best {
                    None => true,
                    Some((s, e, _)) => start < s || (start == s && end > e),
                };
                if better {
                    best = Some((start, end, token.id));
                }
                break;
            }
        }
        best
    }

    /// Pre-tokenises normalised text into words and WordPieces them: white space
    /// separates words and every punctuation character is a word of its own.
    fn words(&self, text: &str, out: &mut Vec<u32>, limit: usize) {
        let mut word_start: Option<usize> = None;
        for (i, c) in text.char_indices() {
            if out.len() >= limit {
                return;
            }
            if c.is_whitespace() {
                if let Some(start) = word_start.take() {
                    self.word_pieces(&text[start..i], out);
                }
            } else if is_split_punct(c) {
                if let Some(start) = word_start.take() {
                    self.word_pieces(&text[start..i], out);
                }
                self.word_pieces(&text[i..i + c.len_utf8()], out);
            } else if word_start.is_none() {
                word_start = Some(i);
            }
        }
        if let Some(start) = word_start {
            if out.len() < limit {
                self.word_pieces(&text[start..], out);
            }
        }
    }

    /// Greedy longest-match WordPiece of one word. A word with a part that no entry covers,
    /// or one that is too long, is the unknown token as a whole.
    fn word_pieces(&self, word: &str, out: &mut Vec<u32>) {
        let s = &self.settings;
        if word.chars().count() > s.max_input_chars {
            self.push_unknown(out);
            return;
        }
        let first = out.len();
        let prefix = s.prefix.as_bytes();
        let mut key: Vec<u8> = Vec::with_capacity(prefix.len() + word.len());
        let mut start = 0;
        while start < word.len() {
            let room = if start > 0 { s.max_token_bytes.saturating_sub(prefix.len()) } else { s.max_token_bytes };
            let mut end = word.len().min(start + room);
            while !word.is_char_boundary(end) {
                end -= 1;
            }
            let mut found = None;
            while end > start {
                key.clear();
                if start > 0 {
                    key.extend_from_slice(prefix);
                }
                key.extend_from_slice(&word.as_bytes()[start..end]);
                if let Some(id) = self.vocab.get(&key) {
                    found = Some((id, end));
                    break;
                }
                end -= 1;
                while !word.is_char_boundary(end) {
                    end -= 1;
                }
            }
            match found {
                Some((id, end)) => {
                    out.push(id);
                    start = end;
                }
                None => {
                    out.truncate(first);
                    self.push_unknown(out);
                    return;
                }
            }
        }
    }

    fn push_unknown(&self, out: &mut Vec<u32>) {
        if let Some(id) = self.settings.unk_id {
            out.push(id);
        }
    }
}

/// `(clean_text, handle_chinese_chars, strip_accents, lowercase)` of a BERT normaliser.
fn parse_normalizer(value: Option<&Value>) -> io::Result<(bool, bool, bool, bool)> {
    match value {
        None | Some(Value::Null) => Ok((false, false, false, false)),
        Some(obj) if obj.get("type").and_then(Value::as_str) == Some("BertNormalizer") => {
            let flag = |name: &str| obj.get(name).and_then(Value::as_bool);
            let lowercase = flag("lowercase").unwrap_or(true);
            Ok((
                flag("clean_text").unwrap_or(true),
                flag("handle_chinese_chars").unwrap_or(true),
                flag("strip_accents").unwrap_or(lowercase),
                lowercase,
            ))
        }
        other => Err(bad(format!(
            "tokenizer.json: normalizer {} is not supported, only BertNormalizer",
            describe(other)
        ))),
    }
}

fn describe(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "(none)".to_string(),
        Some(v) => v.get("type").and_then(Value::as_str).map_or_else(|| "(untyped)".to_string(), |t| format!("{t:?}")),
    }
}

/// The cache file for a given `tokenizer.json`.
fn cache_file(tokenizer_json: &Path, cache_dir: &Path) -> PathBuf {
    let full = fs::canonicalize(tokenizer_json).unwrap_or_else(|_| tokenizer_json.to_path_buf());
    cache_dir.join(cache_name(&full.to_string_lossy(), cfg!(windows)))
}

/// The cache file name for a canonical path. Where `windows_names` is set, case and the
/// kind of slash do not tell two paths apart, so they are folded; elsewhere `A` and `a` are
/// two folders and `\` is an ordinary character, and the path is hashed as it is.
fn cache_name(path: &str, windows_names: bool) -> String {
    let name = if windows_names { path.replace('\\', "/").to_lowercase() } else { path.to_string() };
    format!("vocab-{:016x}.bin", hash64(name.as_bytes()))
}

fn put_string(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

/// A bounds-checked reader over the cache bytes.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let slice = self.b.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn string(&mut self) -> Option<String> {
        let len = self.u32()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }

    fn u32s(&mut self, count: usize) -> Option<Vec<u32>> {
        let bytes = self.take(count.checked_mul(4)?)?;
        Some(bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }
}

/// A tokenizer over the entries of `words`, for tests. Id 0 is `[UNK]` (an added token
/// matched in the text as written), the words follow, and `[PAD]` comes last (an added
/// token matched in normalised text, as a whole word only).
#[cfg(test)]
pub(crate) fn tiny_json(words: &[&str]) -> String {
    let mut vocab = serde_json::Map::new();
    vocab.insert("[UNK]".to_string(), Value::from(0));
    for (i, w) in words.iter().enumerate() {
        vocab.insert((*w).to_string(), Value::from(i as u64 + 1));
    }
    let pad = words.len() as u64 + 1;
    vocab.insert("[PAD]".to_string(), Value::from(pad));
    let json = serde_json::json!({
        "truncation": {"direction": "Right", "max_length": 512, "strategy": "LongestFirst", "stride": 0},
        "padding": {"strategy": "BatchLongest", "direction": "Right", "pad_to_multiple_of": null, "pad_id": pad, "pad_type_id": 0, "pad_token": "[PAD]"},
        "added_tokens": [
            {"id": 0, "content": "[UNK]", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
            {"id": pad, "content": "[PAD]", "single_word": true, "lstrip": true, "rstrip": true, "normalized": true, "special": true}
        ],
        "normalizer": {"type": "BertNormalizer", "clean_text": true, "handle_chinese_chars": true, "strip_accents": null, "lowercase": true},
        "pre_tokenizer": {"type": "BertPreTokenizer"},
        "model": {"type": "WordPiece", "unk_token": "[UNK]", "continuing_subword_prefix": "##", "max_input_chars_per_word": 100, "vocab": vocab}
    });
    json.to_string()
}

/// The tokenizer of [`tiny_json`].
#[cfg(test)]
pub(crate) fn tiny(words: &[&str]) -> Tokenizer {
    Tokenizer::from_json(tiny_json(words).as_bytes()).expect("tiny tokenizer")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ids: [UNK]=0, the=1, quick=2, brown=3, un=4, ##able=5, (=6, )=7, a=8, ##b=9, e=10, middle=11, [PAD]=12
    fn words() -> Tokenizer {
        tiny(&["the", "quick", "brown", "un", "##able", "(", ")", "a", "##b", "e", "\u{4e2d}"])
    }

    #[test]
    fn splits_on_white_space_and_punctuation() {
        let t = words();
        assert_eq!(t.encode("The  quick\tbrown"), vec![1, 2, 3]);
        assert_eq!(t.encode("(the)"), vec![6, 1, 7]);
        assert_eq!(t.encode(""), Vec::<u32>::new());
        assert_eq!(t.encode("   "), Vec::<u32>::new());
    }

    #[test]
    fn word_pieces_are_greedy_with_continuation_prefix() {
        let t = words();
        assert_eq!(t.encode("unable"), vec![4, 5]);
        assert_eq!(t.encode("ab"), vec![8, 9]);
    }

    #[test]
    fn a_word_with_an_uncovered_part_is_one_unknown() {
        let t = words();
        assert_eq!(t.encode("unabler"), vec![0]);
        assert_eq!(t.encode("zzz the"), vec![0, 1]);
    }

    #[test]
    fn a_word_over_the_character_limit_is_unknown() {
        let t = tiny(&["a", "##a"]);
        assert_eq!(t.encode(&"a".repeat(100)).len(), 100);
        assert_eq!(t.encode(&"a".repeat(101)), vec![0]);
    }

    #[test]
    fn accents_are_stripped_and_case_is_folded() {
        let t = words();
        assert_eq!(t.encode("\u{c9}"), vec![10]);
        assert_eq!(t.encode("e\u{301}"), vec![10]);
        assert_eq!(t.encode("THE"), vec![1]);
    }

    #[test]
    fn chinese_characters_stand_alone() {
        let t = words();
        assert_eq!(t.encode("a\u{4e2d}a"), vec![8, 11, 8]);
    }

    #[test]
    fn control_characters_vanish_and_other_spaces_split() {
        let t = words();
        assert_eq!(t.encode("a\u{0}b"), vec![8, 9]);
        assert_eq!(t.encode("a\u{200b}b"), vec![8, 9]);
        assert_eq!(t.encode("a\u{a0}a"), vec![8, 8]);
        assert_eq!(t.encode("a\r\nb"), vec![8, 0]);
    }

    #[test]
    fn added_tokens_are_found_in_the_text() {
        let t = words();
        assert_eq!(t.encode("the[UNK]quick"), vec![1, 0, 2]);
        assert_eq!(t.encode("the [pad] quick"), vec![1, 12, 2]);
        assert_eq!(t.encode("the [PAD]"), vec![1, 12]);
        // Not a whole word: the brackets are punctuation again.
        assert_eq!(t.encode("the[PAD]quick"), vec![1, 0, 0, 0, 2]);
    }

    #[test]
    fn the_tokenizer_truncates_to_its_limit() {
        let t = words();
        assert_eq!(t.encode(&"the ".repeat(600)).len(), 512);
    }

    #[test]
    fn other_model_types_are_refused_with_a_clear_message() {
        let json = r#"{"model": {"type": "BPE", "vocab": {"a": 0}}, "pre_tokenizer": {"type": "BertPreTokenizer"}}"#;
        let error = Tokenizer::from_json(json.as_bytes()).unwrap_err();
        assert!(error.to_string().contains("BPE"), "{error}");
        assert!(error.to_string().contains("WordPiece"), "{error}");
        assert!(Tokenizer::from_json(b"not json").is_err());
    }

    #[test]
    fn cache_round_trip_keeps_behaviour_and_rejects_damage() {
        let t = words();
        let key = CacheKey { size: 10, secs: 20, nanos: 30 };
        let bytes = t.to_cache(&key);
        let back = Tokenizer::from_cache(&bytes, &key).expect("current cache");
        for text in ["The quick (brown) unable", "a\u{4e2d}a", "zzz [pad] [UNK]", "e\u{301}"] {
            assert_eq!(back.encode(text), t.encode(text), "{text}");
        }
        assert_eq!(back.median_len(), t.median_len());
        assert_eq!(back.vocab_len(), t.vocab_len());
        assert_eq!(back.unk_id(), Some(0));
        assert!(Tokenizer::from_cache(&bytes, &CacheKey { size: 11, ..key }).is_none());
        let mut damaged = bytes.clone();
        let middle = damaged.len() / 2;
        damaged[middle] ^= 0x55;
        assert!(Tokenizer::from_cache(&damaged, &key).is_none());
        assert!(Tokenizer::from_cache(&bytes[..bytes.len() - 3], &key).is_none());
        assert!(Tokenizer::from_cache(&[], &key).is_none());
    }

    #[test]
    fn median_length_counts_characters() {
        let t = tiny(&["a", "bb", "ccc"]);
        // lengths: a 1, bb 2, ccc 3, [UNK] 5, [PAD] 5: sorted 1 2 3 5 5, median 3
        assert_eq!(t.median_len(), 3);
    }

    /// A WordPiece tokenizer file with the given vocabulary text and nothing else.
    fn bare_json(vocab: &str) -> String {
        format!(r#"{{"model": {{"type": "WordPiece", "vocab": {vocab}}}, "pre_tokenizer": {{"type": "BertPreTokenizer"}}}}"#)
    }

    #[test]
    fn a_sparse_vocabulary_is_refused_before_it_allocates() {
        // Storage is indexed by id: two entries must not be able to ask for millions of slots.
        for top in [3_000_000u64, 99_999_999] {
            let json = bare_json(&format!(r#"{{"[UNK]": 0, "x": {top}}}"#));
            // `err()` keeps a failure from printing the whole oversized vocabulary.
            let error = Tokenizer::from_json(json.as_bytes()).err().expect("a sparse vocabulary is refused");
            assert!(error.to_string().contains("token id"), "{error}");
        }
        // A few unused ids are ordinary and still load.
        let gap = Tokenizer::from_json(bare_json(r#"{"[UNK]": 0, "x": 5}"#).as_bytes()).unwrap();
        assert_eq!(gap.vocab_len(), 6);
    }

    #[test]
    fn ids_beyond_32_bits_are_refused_instead_of_wrapping() {
        let added = bare_json(r#"{"[UNK]": 0, "x": 1}"#).replace(
            r#""pre_tokenizer""#,
            r#""added_tokens": [{"id": 4294967297, "content": "z"}], "pre_tokenizer""#,
        );
        assert!(Tokenizer::from_json(added.as_bytes()).is_err());
        let padded = bare_json(r#"{"[UNK]": 0, "x": 1}"#).replace(r#""pre_tokenizer""#, r#""padding": {"pad_id": 4294967297}, "pre_tokenizer""#);
        assert!(Tokenizer::from_json(padded.as_bytes()).is_err());
    }

    #[test]
    fn cache_names_keep_apart_what_the_file_system_keeps_apart() {
        // On a case-sensitive file system two folders that differ in case are two models, and
        // a backslash is an ordinary character of a name.
        assert_ne!(cache_name("/models/A/tokenizer.json", false), cache_name("/models/a/tokenizer.json", false));
        assert_ne!(cache_name("/m/a\\b/tokenizer.json", false), cache_name("/m/a/b/tokenizer.json", false));
        // On Windows neither case nor the kind of slash tells two paths apart.
        assert_eq!(cache_name("C:\\Models\\A\\tokenizer.json", true), cache_name("c:/models/a/tokenizer.json", true));
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_two_spellings_of_one_file_share_a_cache() {
        let dir = std::env::temp_dir().join(format!("lxr-text-cache-name-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("tokenizer.json");
        fs::write(&file, b"{}").unwrap();
        let shouted = PathBuf::from(file.to_string_lossy().to_uppercase());
        assert_eq!(cache_file(&file, &dir), cache_file(&shouted, &dir));
        fs::remove_dir_all(&dir).unwrap();
    }
}
