//! Static embeddings in the manner of model2vec: the mean of the vectors of a text's
//! WordPiece tokens, scaled to length one.

use crate::tokenizer::Tokenizer;
use memmap2::Mmap;
use rayon::prelude::*;
use serde_json::Value;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// The folder name of the model under `~/.local-index/models`.
const MODEL_NAME: &str = "potion-code-16M-v2";
/// The folder name of the model in a Hugging Face cache.
const HUB_FOLDER: &str = "models--minishlab--potion-code-16M-v2";
/// Tokens kept per query text, as in model2vec's `encode`.
const MAX_LENGTH: usize = 512;

/// A static embedding model: a tokenizer and a table with one vector per token.
///
/// The table stays in the memory-mapped model file, so opening is cheap and only the
/// rows that texts use are read from disk.
#[derive(Debug)]
pub struct Embedder {
    tokenizer: Tokenizer,
    map: Mmap,
    /// Offset of the first table byte in the file.
    data_start: usize,
    /// Number of table rows.
    rows: usize,
    dim: usize,
    /// True when the table holds 32-bit floats, false for 16-bit ones.
    wide: bool,
    cache_hit: bool,
}

impl Embedder {
    /// Finds the model folder: the first of these that holds both `model.safetensors`
    /// and `tokenizer.json`. The folder named by `LXR_MODEL_DIR`;
    /// `~/.local-index/models/potion-code-16M-v2`; the newest snapshot of
    /// `minishlab/potion-code-16M-v2` in the Hugging Face cache (`HF_HUB_CACHE`, else
    /// `HF_HOME/hub`, else `~/.cache/huggingface/hub`).
    pub fn locate() -> Option<PathBuf> {
        locate_with(&|name| std::env::var_os(name))
    }

    /// Opens the model in `dir`. The vocabulary comes from a cache file under the
    /// index home when that is current for this `tokenizer.json`, and otherwise from the
    /// JSON, which is slow; the cache is written then.
    pub fn open(dir: &Path) -> io::Result<Embedder> {
        Embedder::open_with_cache(dir, &lxr_core::index_home().join("model-cache"))
    }

    /// Like [`Embedder::open`], with the cache folder given.
    pub fn open_with_cache(dir: &Path, cache_dir: &Path) -> io::Result<Embedder> {
        let tokenizer_json = dir.join("tokenizer.json");
        let weights = dir.join("model.safetensors");
        for file in [&tokenizer_json, &weights] {
            if !file.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("model folder {} has no {}", dir.display(), file.file_name().unwrap_or_default().to_string_lossy()),
                ));
            }
        }
        let (tokenizer, cache_hit) = Tokenizer::load(&tokenizer_json, cache_dir)?;
        let file = File::open(&weights)?;
        // SAFETY: the model file is only read, and nothing in this process writes to it.
        let map = unsafe { Mmap::map(&file)? };
        let table = read_table(&map)?;
        // Every id the tokenizer can give, added tokens and the padding id included, must
        // have a row; a missing row would silently count as zero.
        if tokenizer.id_bound() > table.rows {
            return Err(bad(format!(
                "the tokenizer uses ids up to {} but the embedding table only has {} rows",
                tokenizer.id_bound() - 1,
                table.rows
            )));
        }
        Ok(Embedder {
            tokenizer,
            map,
            data_start: table.start,
            rows: table.rows,
            dim: table.dim,
            wide: table.wide,
            cache_hit,
        })
    }

    /// Length of the vectors.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// True when `open` took the vocabulary from the cache file.
    pub fn used_cache(&self) -> bool {
        self.cache_hit
    }

    /// The ids the tokenizer alone gives for `text`, unknown words included, cut to the
    /// tokenizer's own limit (512 for this model).
    pub fn tokenizer_ids(&self, text: &str) -> Vec<u32> {
        self.tokenizer.encode(text)
    }

    /// The ids that [`Embedder::embed`] averages: the text cut to 512 times the median
    /// token length in characters, tokenised, unknown words dropped, and the first 512 kept.
    pub fn token_ids(&self, text: &str) -> Vec<u32> {
        self.kept_ids(text, true)
    }

    /// The unit vector for a query text, as model2vec's `encode` gives it for one text:
    /// the mean of the token vectors, divided by its length plus 1e-32. A text with no
    /// usable token gives the zero vector.
    pub fn embed(&self, text: &str) -> Vec<f32> {
        self.vector(&self.kept_ids(text, true))
    }

    /// Embeds many texts in parallel, each on its own, as semble does when it indexes: no
    /// cut by length beyond the tokenizer's own 512 tokens. This is `embed` without the
    /// character cut. It does not reproduce the padding that model2vec adds to the shorter
    /// texts of a batch; [`Embedder::embed_padded`] does.
    pub fn embed_many(&self, texts: &[&str]) -> Vec<Vec<f32>> {
        texts.par_iter().map(|text| self.vector(&self.kept_ids(text, false))).collect()
    }

    /// Embeds one batch of texts exactly as model2vec's `encode` does for a list: the
    /// tokenizer pads every text with its padding token up to the length of the longest
    /// text of the batch, before unknown words are dropped, and the padding rows are part
    /// of the mean. semble embeds the chunks of one file this way (a batch is at most 1024
    /// texts there), so a short chunk next to a long one has a vector that leans towards
    /// the padding row. A batch of one text is the same as `embed_many`.
    pub fn embed_padded(&self, texts: &[&str]) -> Vec<Vec<f32>> {
        let encoded: Vec<Vec<u32>> = texts.par_iter().map(|text| self.tokenizer.encode(text)).collect();
        let longest = encoded.iter().map(Vec::len).max().unwrap_or(0);
        let (unk, pad) = (self.tokenizer.unk_id(), self.tokenizer.pad_id());
        encoded
            .par_iter()
            .map(|raw| {
                let mut ids: Vec<u32> = raw.iter().copied().filter(|&id| Some(id) != unk).collect();
                if let Some(pad) = pad {
                    ids.resize(ids.len() + (longest - raw.len()), pad);
                }
                self.vector(&ids)
            })
            .collect()
    }

    fn kept_ids(&self, text: &str, query: bool) -> Vec<u32> {
        let text = if query {
            let chars = MAX_LENGTH * self.tokenizer.median_len();
            text.char_indices().nth(chars).map_or(text, |(end, _)| &text[..end])
        } else {
            text
        };
        let mut ids = self.tokenizer.encode(text);
        if let Some(unk) = self.tokenizer.unk_id() {
            ids.retain(|&id| id != unk);
        }
        if query {
            ids.truncate(MAX_LENGTH);
        }
        ids
    }

    fn vector(&self, ids: &[u32]) -> Vec<f32> {
        let mut sum = vec![0f32; self.dim];
        if ids.is_empty() {
            return sum;
        }
        for &id in ids {
            self.add_row(id as usize, &mut sum);
        }
        let count = ids.len() as f32;
        for x in &mut sum {
            *x /= count;
        }
        let norm = sum.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>().sqrt() + 1e-32;
        for x in &mut sum {
            *x = (f64::from(*x) / norm) as f32;
        }
        sum
    }

    fn add_row(&self, id: usize, sum: &mut [f32]) {
        if id >= self.rows {
            return;
        }
        let width = if self.wide { 4 } else { 2 };
        let start = self.data_start + id * self.dim * width;
        let bytes = &self.map[start..start + self.dim * width];
        if self.wide {
            for (s, c) in sum.iter_mut().zip(bytes.chunks_exact(4)) {
                *s += f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            }
        } else {
            for (s, c) in sum.iter_mut().zip(bytes.chunks_exact(2)) {
                *s += f16_to_f32(u16::from_le_bytes([c[0], c[1]]));
            }
        }
    }
}

/// Converts an IEEE half-precision float, given as its bits, to single precision.
pub(crate) fn f16_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let fraction = u32::from(bits & 0x3ff);
    match exponent {
        0 if fraction == 0 => f32::from_bits(sign),
        0 => {
            // Subnormal: fraction * 2^-24.
            let magnitude = fraction as f32 * (1.0 / 16_777_216.0);
            if sign == 0 {
                magnitude
            } else {
                -magnitude
            }
        }
        31 => f32::from_bits(sign | 0x7f80_0000 | (fraction << 13)),
        _ => f32::from_bits(sign | ((exponent + 112) << 23) | (fraction << 13)),
    }
}

fn bad(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Where the embedding table sits in a safetensors file.
struct Table {
    start: usize,
    rows: usize,
    dim: usize,
    wide: bool,
}

/// Reads the safetensors header: 8 bytes with the little-endian length of a JSON
/// header, which names the tensors. Only a single `embeddings` tensor of 16 or 32 bit
/// floats is accepted; the weight and mapping tensors of a quantised model are not.
fn read_table(file: &[u8]) -> io::Result<Table> {
    let length = file
        .get(..8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        .ok_or_else(|| bad("model.safetensors is too short for a header"))?;
    let base = usize::try_from(length)
        .ok()
        .and_then(|n| n.checked_add(8))
        .filter(|&end| end <= file.len())
        .ok_or_else(|| bad("model.safetensors: the header length runs past the end of the file"))?;
    let header: Value = serde_json::from_slice(&file[8..base])
        .map_err(|e| bad(format!("model.safetensors: the header is not valid JSON: {e}")))?;
    let tensors = header.as_object().ok_or_else(|| bad("model.safetensors: the header is not an object"))?;
    if let Some(name) = tensors.keys().find(|k| *k != "embeddings" && *k != "__metadata__") {
        return Err(bad(format!(
            "model.safetensors holds a tensor named {name:?}; only plain `embeddings` models are supported"
        )));
    }
    let tensor = tensors.get("embeddings").ok_or_else(|| bad("model.safetensors has no `embeddings` tensor"))?;
    let wide = match tensor.get("dtype").and_then(Value::as_str) {
        Some("F16") => false,
        Some("F32") => true,
        other => return Err(bad(format!("model.safetensors: embeddings dtype {other:?} is not supported, only F16 and F32"))),
    };
    let shape: Vec<u64> = tensor
        .get("shape")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();
    let &[rows, dim] = shape.as_slice() else {
        return Err(bad("model.safetensors: embeddings is not a matrix"));
    };
    let offsets: Vec<u64> = tensor
        .get("data_offsets")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();
    let &[from, to] = offsets.as_slice() else {
        return Err(bad("model.safetensors: embeddings has no data offsets"));
    };
    let width: u64 = if wide { 4 } else { 2 };
    let size = rows.checked_mul(dim).and_then(|n| n.checked_mul(width));
    match (size, to.checked_sub(from)) {
        (Some(expected), Some(given)) if dim > 0 && rows > 0 && expected == given => {}
        _ => return Err(bad("model.safetensors: the embeddings shape does not match its data offsets")),
    }
    let start = (base as u64).checked_add(from).filter(|&s| s.checked_add(to - from).is_some_and(|e| e <= file.len() as u64));
    let start = start.ok_or_else(|| bad("model.safetensors: the embeddings data runs past the end of the file"))?;
    Ok(Table { start: start as usize, rows: rows as usize, dim: dim as usize, wide })
}

fn holds_model(dir: &Path) -> bool {
    dir.join("model.safetensors").is_file() && dir.join("tokenizer.json").is_file()
}

/// [`Embedder::locate`] with the environment given, so that it can be tested.
fn locate_with(get: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |name: &str| get(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("LXR_MODEL_DIR") {
        if holds_model(&dir) {
            return Some(dir);
        }
    }
    let home = var("USERPROFILE").or_else(|| var("HOME"));
    if let Some(home) = &home {
        let dir = home.join(".local-index").join("models").join(MODEL_NAME);
        if holds_model(&dir) {
            return Some(dir);
        }
    }
    let hub = var("HF_HUB_CACHE")
        .or_else(|| var("HF_HOME").map(|h| h.join("hub")))
        .or_else(|| home.map(|h| h.join(".cache").join("huggingface").join("hub")))?;
    newest_snapshot(&hub.join(HUB_FOLDER).join("snapshots"))
}

/// The most recently modified folder in `snapshots` that holds a model.
fn newest_snapshot(snapshots: &Path) -> Option<PathBuf> {
    fs::read_dir(snapshots)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| holds_model(path))
        .max_by_key(|path| {
            let modified = fs::metadata(path).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (modified, path.clone())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::tiny_json;
    use std::collections::HashMap;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lxr-text-embed-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A safetensors file with one `embeddings` tensor of the given data.
    fn safetensors(dtype: &str, shape: [usize; 2], data: &[u8]) -> Vec<u8> {
        let header = format!(
            r#"{{"embeddings":{{"dtype":"{dtype}","shape":[{},{}],"data_offsets":[0,{}]}}}}"#,
            shape[0],
            shape[1],
            data.len()
        );
        let mut out = (header.len() as u64).to_le_bytes().to_vec();
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(data);
        out
    }

    /// A model over the words "x", "y", "z" with ids 1, 2, 3 (id 0 is [UNK], id 4 is [PAD]).
    fn tiny_model(dir: &Path, wide: bool) {
        fs::write(dir.join("tokenizer.json"), tiny_json(&["x", "y", "z"])).unwrap();
        // Rows of dimension 2: [UNK] (1, 1), x (1, 0), y (0, 2), z (-1, 0), [PAD] (0, 1).
        let values: [[f32; 2]; 5] = [[1.0, 1.0], [1.0, 0.0], [0.0, 2.0], [-1.0, 0.0], [0.0, 1.0]];
        let half = |v: f32| -> u16 {
            if v == 1.0 {
                0x3C00
            } else if v == 2.0 {
                0x4000
            } else if v == -1.0 {
                0xBC00
            } else {
                0x0000
            }
        };
        let mut data = Vec::new();
        for row in values {
            for v in row {
                if wide {
                    data.extend_from_slice(&v.to_le_bytes());
                } else {
                    data.extend_from_slice(&half(v).to_le_bytes());
                }
            }
        }
        fs::write(dir.join("model.safetensors"), safetensors(if wide { "F32" } else { "F16" }, [5, 2], &data)).unwrap();
    }

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    #[test]
    fn half_floats_convert() {
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert!(f16_to_f32(0x8000).is_sign_negative() && f16_to_f32(0x8000) == 0.0);
        assert_eq!(f16_to_f32(0x3C00), 1.0);
        assert_eq!(f16_to_f32(0xBC00), -1.0);
        assert_eq!(f16_to_f32(0x3800), 0.5);
        assert_eq!(f16_to_f32(0x7BFF), 65504.0);
        assert_eq!(f16_to_f32(0x0001), 5.960_464_5e-8);
        assert_eq!(f16_to_f32(0x03FF), 6.097_555_2e-5);
        assert_eq!(f16_to_f32(0x0400), 6.103_515_6e-5);
        assert_eq!(f16_to_f32(0x7C00), f32::INFINITY);
        assert_eq!(f16_to_f32(0xFC00), f32::NEG_INFINITY);
        assert!(f16_to_f32(0x7E00).is_nan());
        assert_eq!(f16_to_f32(0x3555), 0.333_251_95);
    }

    #[test]
    fn embeds_the_mean_of_the_token_vectors_scaled_to_length_one() {
        for wide in [false, true] {
            let dir = temp(if wide { "wide" } else { "half" });
            tiny_model(&dir, wide);
            let model = Embedder::open_with_cache(&dir, &dir.join("cache")).unwrap();
            assert_eq!(model.dim(), 2);
            // x and y: mean (0.5, 1.0), length sqrt(1.25).
            let s = 1.25f32.sqrt();
            assert!(close(&model.embed("x y"), &[0.5 / s, 1.0 / s]), "{:?}", model.embed("x y"));
            // z alone: (-1, 0).
            assert!(close(&model.embed("Z"), &[-1.0, 0.0]));
            // Unknown words are dropped, not averaged in.
            assert!(close(&model.embed("x unknown"), &[1.0, 0.0]));
            // [PAD] is a real token with a row of its own: it counts in the mean.
            let h = std::f32::consts::FRAC_1_SQRT_2;
            assert!(close(&model.embed("x [pad]"), &[h, h]));
            fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn a_padded_batch_leans_towards_the_padding_row() {
        let dir = temp("padded");
        tiny_model(&dir, false);
        let model = Embedder::open_with_cache(&dir, &dir.join("cache")).unwrap();
        // "w" is unknown: three tokens of the second text are real, two are unknown, so the
        // batch is five long and the first text, "x", is padded with four rows of (0, 1).
        let batch = model.embed_padded(&["x", "x y z w w", ""]);
        let s = 17f32.sqrt();
        assert!(close(&batch[0], &[1.0 / s, 4.0 / s]), "{:?}", batch[0]);
        // x + y + z = (0, 2): no padding is needed.
        assert!(close(&batch[1], &[0.0, 1.0]), "{:?}", batch[1]);
        // The empty text is padded too, with five rows.
        assert!(close(&batch[2], &[0.0, 1.0]), "{:?}", batch[2]);
        // A batch of one is not padded.
        assert!(close(&model.embed_padded(&["x"])[0], &[1.0, 0.0]));
        assert!(model.embed_padded(&[]).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_token_gives_the_zero_vector() {
        let dir = temp("zero");
        tiny_model(&dir, false);
        let model = Embedder::open_with_cache(&dir, &dir.join("cache")).unwrap();
        for text in ["", "   ", "???", "unknown words only", "\u{4e2d}"] {
            assert_eq!(model.embed(text), vec![0.0, 0.0], "{text:?}");
        }
        assert_eq!(model.embed_many(&[]), Vec::<Vec<f32>>::new());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn embed_many_agrees_with_embed_for_short_texts() {
        let dir = temp("many");
        tiny_model(&dir, false);
        let model = Embedder::open_with_cache(&dir, &dir.join("cache")).unwrap();
        let texts = ["x y", "z", "", "y y y x", "nothing"];
        let many = model.embed_many(&texts);
        assert_eq!(many.len(), texts.len());
        for (text, vector) in texts.iter().zip(&many) {
            assert_eq!(*vector, model.embed(text), "{text:?}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_query_is_cut_by_length_but_a_document_is_not() {
        let dir = temp("cut");
        tiny_model(&dir, false);
        let model = Embedder::open_with_cache(&dir, &dir.join("cache")).unwrap();
        // The median entry length of this vocabulary is 1 character (x, y, z, ids 1 to 3
        // are 1 long, [UNK] and [PAD] 5): sorted 1 1 1 5 5, median 1. So a query keeps
        // 512 characters, which is 256 words of "x ".
        let long = "x ".repeat(2000);
        assert_eq!(model.token_ids(&long).len(), 256);
        // A document is only cut by the tokenizer's 512 tokens.
        assert!(close(&model.embed_many(&[long.as_str()])[0], &[1.0, 0.0]));
        assert_eq!(model.tokenizer_ids(&long).len(), 512);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_vocabulary_cache_is_written_used_and_replaced_when_stale() {
        let dir = temp("cache");
        tiny_model(&dir, false);
        let cache = dir.join("cache");
        let first = Embedder::open_with_cache(&dir, &cache).unwrap();
        assert!(!first.used_cache());
        let second = Embedder::open_with_cache(&dir, &cache).unwrap();
        assert!(second.used_cache());
        assert_eq!(first.embed("x y z"), second.embed("x y z"));
        // A different tokenizer file (other size) makes the cache stale.
        fs::write(dir.join("tokenizer.json"), tiny_json(&["x", "y", "z", "w"])).unwrap();
        let third = Embedder::open_with_cache(&dir, &cache);
        assert!(third.is_err(), "the table has fewer rows than the new vocabulary has ids");
        fs::write(dir.join("tokenizer.json"), tiny_json(&["x", "y", "z"]).replace("\"max_length\":512", "\"max_length\":100")).unwrap();
        let fourth = Embedder::open_with_cache(&dir, &cache).unwrap();
        assert!(!fourth.used_cache());
        assert_eq!(fourth.tokenizer_ids(&"x ".repeat(300)).len(), 100);
        // A damaged cache file is ignored.
        for entry in fs::read_dir(&cache).unwrap() {
            fs::write(entry.unwrap().path(), b"damaged").unwrap();
        }
        assert!(!Embedder::open_with_cache(&dir, &cache).unwrap().used_cache());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_keeps_its_cache_in_the_index_home() {
        let dir = temp("home");
        tiny_model(&dir, false);
        let home = dir.join("home");
        // Nothing else in the test run reads the index home, so setting it is safe here.
        std::env::set_var("LXR_HOME", &home);
        let first = Embedder::open(&dir).unwrap();
        let second = Embedder::open(&dir).unwrap();
        std::env::remove_var("LXR_HOME");
        assert!(!first.used_cache() && second.used_cache());
        let files = fs::read_dir(home.join("model-cache")).unwrap().count();
        assert_eq!(files, 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn locate_on_this_machine_returns_a_folder_with_a_model_or_nothing() {
        if let Some(dir) = Embedder::locate() {
            assert!(holds_model(&dir), "{}", dir.display());
        }
    }

    #[test]
    fn an_unsupported_tokenizer_fails_open_with_a_clear_error() {
        let dir = temp("bpe");
        tiny_model(&dir, false);
        fs::write(
            dir.join("tokenizer.json"),
            r#"{"model": {"type": "BPE", "vocab": {"a": 0}, "merges": []}, "pre_tokenizer": {"type": "BertPreTokenizer"}}"#,
        )
        .unwrap();
        let error = Embedder::open_with_cache(&dir, &dir.join("cache")).unwrap_err();
        assert!(error.to_string().contains("BPE") && error.to_string().contains("WordPiece"), "{error}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_or_broken_model_fails_open_without_a_panic() {
        let dir = temp("broken");
        assert!(Embedder::open_with_cache(&dir, &dir.join("cache")).is_err());
        tiny_model(&dir, false);
        let good = fs::read(dir.join("model.safetensors")).unwrap();
        for broken in [
            Vec::new(),
            vec![1, 2, 3],
            u64::MAX.to_le_bytes().to_vec(),
            good[..good.len() - 4].to_vec(),
            safetensors("I8", [5, 2], &[0; 10]),
            safetensors("F16", [5, 3], &[0; 20]),
            {
                let mut v = good.clone();
                v[8] = b'[';
                v
            },
        ] {
            fs::write(dir.join("model.safetensors"), &broken).unwrap();
            assert!(Embedder::open_with_cache(&dir, &dir.join("cache")).is_err());
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A model folder with the given `tokenizer.json` text and a table of `rows` rows of
    /// dimension 2, row `i` holding `2i + 1` and `2i + 2`.
    fn model_folder(dir: &Path, tokenizer: &str, rows: usize) {
        fs::write(dir.join("tokenizer.json"), tokenizer).unwrap();
        let data: Vec<u8> = (0..rows * 2).flat_map(|i| (i as f32 + 1.0).to_le_bytes()).collect();
        fs::write(dir.join("model.safetensors"), safetensors("F32", [rows, 2], &data)).unwrap();
    }

    /// A tokenizer file whose vocabulary holds the ids 0 to 2. `top` is more top-level
    /// fields, each followed by a comma.
    fn three_ids(top: &str) -> String {
        format!(
            r#"{{{top} "model": {{"type": "WordPiece", "unk_token": "[UNK]", "vocab": {{"[UNK]": 0, "x": 1, "y": 2}}}}, "pre_tokenizer": {{"type": "BertPreTokenizer"}}}}"#
        )
    }

    #[test]
    fn an_added_token_or_padding_id_beyond_the_table_fails_open() {
        let dir = temp("ids");
        let open = |tokenizer: String, label: &str| {
            model_folder(&dir, &tokenizer, 3);
            Embedder::open_with_cache(&dir, &dir.join(format!("cache-{label}")))
        };
        // All three ids are rows of the three-row table: the model opens and uses them.
        let inside = open(three_ids(r#""added_tokens": [{"id": 2, "content": "z"}], "padding": {"pad_id": 2},"#), "inside").unwrap();
        assert!(inside.embed("z").iter().any(|&x| x != 0.0));
        // On Windows the model file cannot be rewritten while it is mapped.
        drop(inside);
        // An added token with id 3, or padding with id 3, has no row: opening must say so
        // instead of letting the token embed as zero.
        let added_outside = three_ids(r#""added_tokens": [{"id": 3, "content": "z"}],"#);
        let pad_outside = three_ids(r#""padding": {"pad_id": 3},"#);
        for (tokenizer, label) in [(added_outside, "added"), (pad_outside, "pad")] {
            let error = open(tokenizer.clone(), label).unwrap_err();
            assert!(error.to_string().contains("rows"), "{error}");
            // The second open reads the vocabulary cache the first one wrote, and refuses as well.
            let again = open(tokenizer, label).unwrap_err();
            assert!(again.to_string().contains("rows"), "{again}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn locate_follows_the_search_order() {
        let root = temp("locate");
        let make = |dir: &Path| {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join("model.safetensors"), b"x").unwrap();
            fs::write(dir.join("tokenizer.json"), b"{}").unwrap();
        };
        let env = |pairs: Vec<(&'static str, PathBuf)>| {
            let map: HashMap<&str, OsString> = pairs.into_iter().map(|(k, v)| (k, v.into_os_string())).collect();
            move |name: &str| map.get(name).cloned()
        };
        // Nothing anywhere.
        assert_eq!(locate_with(&env(vec![("USERPROFILE", root.join("home"))])), None);
        // The Hugging Face cache under home: the newest snapshot that holds both files.
        let snaps = root.join("home/.cache/huggingface/hub").join(HUB_FOLDER).join("snapshots");
        make(&snaps.join("aaa"));
        std::thread::sleep(std::time::Duration::from_millis(30));
        make(&snaps.join("bbb"));
        fs::create_dir_all(snaps.join("ccc")).unwrap();
        fs::write(snaps.join("ccc").join("model.safetensors"), b"x").unwrap();
        assert_eq!(locate_with(&env(vec![("USERPROFILE", root.join("home"))])), Some(snaps.join("bbb")));
        // HF_HOME and HF_HUB_CACHE move the cache.
        let other = root.join("hf");
        let other_snaps = other.join("hub").join(HUB_FOLDER).join("snapshots").join("rev");
        make(&other_snaps);
        assert_eq!(
            locate_with(&env(vec![("USERPROFILE", root.join("home")), ("HF_HOME", other.clone())])),
            Some(other_snaps.clone())
        );
        let direct = root.join("direct");
        let direct_snap = direct.join(HUB_FOLDER).join("snapshots").join("rev");
        make(&direct_snap);
        assert_eq!(
            locate_with(&env(vec![("HF_HOME", other), ("HF_HUB_CACHE", direct)])),
            Some(direct_snap)
        );
        // The local-index folder comes before the Hugging Face cache.
        let local = root.join("home/.local-index/models").join(MODEL_NAME);
        make(&local);
        assert_eq!(locate_with(&env(vec![("USERPROFILE", root.join("home"))])), Some(local.clone()));
        // LXR_MODEL_DIR comes first, but only when it holds the model.
        let named = root.join("named");
        make(&named);
        assert_eq!(
            locate_with(&env(vec![("USERPROFILE", root.join("home")), ("LXR_MODEL_DIR", named.clone())])),
            Some(named)
        );
        assert_eq!(
            locate_with(&env(vec![("USERPROFILE", root.join("home")), ("LXR_MODEL_DIR", root.join("nope"))])),
            Some(local)
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// Text-parity checks against the installed model2vec. They run when the named
    /// files exist: `LXR_EMBED_PARITY` is the JSON written by `parity/embed_reference.py`
    /// and `LXR_EMBED_ZOO` the file written by `parity/zoo_reference.py`.
    fn real_model() -> Option<Embedder> {
        let dir = Embedder::locate()?;
        let cache = std::env::temp_dir().join("lxr-text-test-model-cache");
        Some(Embedder::open_with_cache(&dir, &cache).expect("the located model opens"))
    }

    #[test]
    fn matches_model2vec_on_the_reference_texts() {
        let Some(path) = std::env::var_os("LXR_EMBED_PARITY") else {
            eprintln!("LXR_EMBED_PARITY is not set: skipping the model2vec comparison");
            return;
        };
        let model = real_model().expect("the real model is installed");
        let reference: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let items = reference["texts"].as_array().unwrap();
        let ids = |v: &Value| v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect::<Vec<u32>>();
        let (mut worst_cos, mut worst_diff) = (("", 1.0f64), ("", 0.0f64));
        let mut id_mismatches = Vec::new();
        for item in items {
            let label = item["label"].as_str().unwrap();
            let text = item["text"].as_str().unwrap();
            if model.tokenizer_ids(text) != ids(&item["raw_ids"]) {
                id_mismatches.push(format!("{label}: raw ids differ"));
            }
            if model.token_ids(text) != ids(&item["kept_ids"]) {
                id_mismatches.push(format!("{label}: kept ids differ"));
            }
            let want: Vec<f64> = item["vector"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            let got = model.embed(text);
            assert_eq!(got.len(), want.len(), "{label}");
            let dot: f64 = got.iter().zip(&want).map(|(a, b)| f64::from(*a) * b).sum();
            let (na, nb) = (
                got.iter().map(|a| f64::from(*a).powi(2)).sum::<f64>().sqrt(),
                want.iter().map(|b| b * b).sum::<f64>().sqrt(),
            );
            let cosine = if na == 0.0 && nb == 0.0 { 1.0 } else { dot / (na * nb) };
            let diff = got.iter().zip(&want).map(|(a, b)| (f64::from(*a) - b).abs()).fold(0.0, f64::max);
            if cosine < worst_cos.1 {
                worst_cos = (label, cosine);
            }
            if diff > worst_diff.1 {
                worst_diff = (label, diff);
            }
            assert!(cosine >= 0.9999, "{label}: cosine {cosine}");
            assert!(diff <= 0.001, "{label}: largest difference {diff}");
        }
        eprintln!(
            "parity: {} texts, worst cosine {:.8} ({}), worst abs diff {:.6} ({}), id mismatches {}",
            items.len(),
            worst_cos.1,
            worst_cos.0,
            worst_diff.1,
            worst_diff.0,
            id_mismatches.len()
        );
        assert!(id_mismatches.is_empty(), "{id_mismatches:?}");
    }

    /// `LXR_EMBED_BATCH` names the JSON written by `parity/batch_reference.py`: batches of
    /// chunks as semble embeds them, with the vectors model2vec gives for each batch.
    #[test]
    fn padded_batches_match_model2vec() {
        let Some(path) = std::env::var_os("LXR_EMBED_BATCH") else {
            eprintln!("LXR_EMBED_BATCH is not set: skipping the batch comparison");
            return;
        };
        let model = real_model().expect("the real model is installed");
        let reference: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let (mut count, mut worst_cosine, mut worst_diff) = (0usize, 1.0f64, 0.0f64);
        let (mut unpadded_worst, mut padded_texts) = (1.0f64, 0usize);
        for batch in reference["batches"].as_array().unwrap() {
            let texts: Vec<&str> = batch["texts"].as_array().unwrap().iter().map(|t| t.as_str().unwrap()).collect();
            let got = model.embed_padded(&texts);
            let plain = model.embed_many(&texts);
            for (i, want) in batch["vectors"].as_array().unwrap().iter().enumerate() {
                let want: Vec<f64> = want.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
                let cosine = |a: &[f32]| {
                    let dot: f64 = a.iter().zip(&want).map(|(x, y)| f64::from(*x) * y).sum();
                    let na = a.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
                    let nb = want.iter().map(|y| y * y).sum::<f64>().sqrt();
                    dot / (na * nb)
                };
                worst_cosine = worst_cosine.min(cosine(&got[i]));
                worst_diff = got[i].iter().zip(&want).map(|(x, y)| (f64::from(*x) - y).abs()).fold(worst_diff, f64::max);
                unpadded_worst = unpadded_worst.min(cosine(&plain[i]));
                if cosine(&plain[i]) < 0.9999 {
                    padded_texts += 1;
                }
                count += 1;
            }
        }
        eprintln!(
            "batch parity: {count} texts, embed_padded worst cosine {worst_cosine:.8}, worst abs diff {worst_diff:.6}; \
             embed_many (unpadded) worst cosine {unpadded_worst:.4}, {padded_texts} texts below 0.9999"
        );
        assert!(worst_cosine >= 0.9999, "worst cosine {worst_cosine}");
        assert!(worst_diff <= 0.001, "worst difference {worst_diff}");
    }

    #[test]
    fn matches_the_tokenizers_library_on_random_strings() {
        let Some(path) = std::env::var_os("LXR_EMBED_FUZZ") else {
            eprintln!("LXR_EMBED_FUZZ is not set: skipping the random string comparison");
            return;
        };
        let model = real_model().expect("the real model is installed");
        let text = fs::read_to_string(path).unwrap();
        match check_random_strings(&model, &text) {
            Ok(checked) => eprintln!("fuzz: {checked} random strings checked, 0 different"),
            Err(wrong) => panic!("{:?}", &wrong[..wrong.len().min(5)]),
        }
    }

    /// Compares the tokenizer with a reference of `{"t": text, "ids": [...]}` lines. Gives the
    /// number of lines checked, or the lines that differ.
    fn check_random_strings(model: &Embedder, reference: &str) -> Result<usize, Vec<String>> {
        let (mut checked, mut wrong) = (0usize, Vec::new());
        for line in reference.lines() {
            let item: Value = serde_json::from_str(line).unwrap();
            let want: Vec<u32> = item["ids"].as_array().unwrap().iter().map(|n| n.as_u64().unwrap() as u32).collect();
            let input = item["t"].as_str().unwrap();
            checked += 1;
            let got = model.tokenizer_ids(input);
            if got != want {
                wrong.push(format!("{input:?}: got {got:?}, want {want:?}"));
            }
        }
        if checked == 0 {
            return Err(vec!["the reference has no cases, so nothing was compared".to_string()]);
        }
        if wrong.is_empty() {
            Ok(checked)
        } else {
            Err(wrong)
        }
    }

    #[test]
    fn a_reference_with_no_cases_is_not_a_passing_comparison() {
        let dir = temp("fuzzcheck");
        tiny_model(&dir, false);
        let model = Embedder::open_with_cache(&dir, &dir.join("cache")).unwrap();
        let empty = check_random_strings(&model, "").expect_err("nothing was compared");
        assert!(empty[0].contains("no cases"), "{empty:?}");
        assert_eq!(check_random_strings(&model, "{\"t\":\"x y\",\"ids\":[1,2]}\n"), Ok(1));
        let wrong = check_random_strings(&model, "{\"t\":\"x y\",\"ids\":[2,1]}\n").unwrap_err();
        assert_eq!(wrong.len(), 1, "{wrong:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn matches_the_tokenizers_library_on_every_code_point() {
        let Some(path) = std::env::var_os("LXR_EMBED_ZOO") else {
            eprintln!("LXR_EMBED_ZOO is not set: skipping the code point comparison");
            return;
        };
        let model = real_model().expect("the real model is installed");
        let text = fs::read_to_string(path).unwrap();
        let (mut checked, mut wrong) = (0usize, Vec::new());
        for line in text.lines() {
            let (cp, ids) = line.split_once('\t').unwrap();
            let c = char::from_u32(cp.parse().unwrap()).unwrap();
            let want: Vec<u32> = ids.split_whitespace().map(|n| n.parse().unwrap()).collect();
            let got = model.tokenizer_ids(&format!("a{c}b"));
            checked += 1;
            if got != want {
                wrong.push(format!("U+{:04X}: got {got:?}, want {want:?}", c as u32));
            }
        }
        eprintln!("zoo: {checked} code points checked, {} different", wrong.len());
        assert!(wrong.is_empty(), "{:?}", &wrong[..wrong.len().min(20)]);
    }
}
