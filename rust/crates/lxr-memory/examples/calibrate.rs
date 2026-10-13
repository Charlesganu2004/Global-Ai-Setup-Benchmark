//! Measures the three cosine thresholds of the memory crate with the real embedder.
//!
//! Usage: `calibrate DOCSTRINGS.json`, the file written by `pick_docstrings.py`: 200 notes
//! of the form "NAME in FILE: sentence" and 60 queries made from them.
//!
//! It prints how the cosine behaves for the three things the thresholds decide:
//! * the meaning floor: a query against its own note and against every other note;
//! * the near-duplicate threshold: a note against lightly edited copies of itself, and
//!   against the most similar different notes;
//! * the semantic link threshold: how many pairs of notes pass each value, and how many of
//!   those are plausibly related (the same file, or three content words in common).

use lxr_memory::{embedding_text, find_entities, Note};
use lxr_text::Embedder;
use serde_json::Value;
use std::collections::HashSet;
use std::error::Error;
use std::fs;

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn pct(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

fn sorted(mut v: Vec<f32>) -> Vec<f32> {
    v.sort_by(|a, b| a.total_cmp(b));
    v
}

fn summary(label: &str, v: &[f32]) {
    println!(
        "  {label:<28} n={:<6} min {:.3}  p5 {:.3}  p10 {:.3}  p25 {:.3}  p50 {:.3}  p75 {:.3}  p90 {:.3}  p95 {:.3}  p99 {:.3}  p99.9 {:.3}  max {:.3}",
        v.len(),
        pct(v, 0.0),
        pct(v, 0.05),
        pct(v, 0.10),
        pct(v, 0.25),
        pct(v, 0.50),
        pct(v, 0.75),
        pct(v, 0.90),
        pct(v, 0.95),
        pct(v, 0.99),
        pct(v, 0.999),
        pct(v, 1.0)
    );
}

struct Item {
    name: String,
    file: String,
    sentence: String,
}

impl Item {
    fn text(&self, sentence: &str) -> String {
        format!("{} in {}: {}", self.name, self.file, sentence)
    }
}

/// The text of a note as the bank embeds it.
fn doc_text(text: &str) -> String {
    let note = Note { text: text.to_string(), entities: find_entities(text), ..Note::default() };
    embedding_text(&note)
}

fn content_words(text: &str) -> HashSet<String> {
    const STOP: [&str; 24] = [
        "the", "a", "an", "of", "to", "in", "for", "and", "or", "is", "are", "that", "this", "with", "on", "as", "by", "from", "it", "be", "if",
        "not", "when", "which",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !STOP.contains(w))
        .map(str::to_string)
        .collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: calibrate DOCSTRINGS.json")?;
    let model = Embedder::locate().ok_or("no embedding model is installed")?;
    let embedder = Embedder::open(&model)?;
    let data: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    let items: Vec<Item> = data["notes"]
        .as_array()
        .ok_or("no notes")?
        .iter()
        .map(|n| Item {
            name: n["name"].as_str().unwrap_or_default().to_string(),
            file: n["file"].as_str().unwrap_or_default().to_string(),
            sentence: n["sentence"].as_str().unwrap_or_default().to_string(),
        })
        .collect();
    let queries: Vec<(usize, String)> = data["queries"]
        .as_array()
        .ok_or("no queries")?
        .iter()
        .map(|q| (q["note"].as_u64().unwrap_or(0) as usize, q["query"].as_str().unwrap_or_default().to_string()))
        .collect();
    let docs: Vec<String> = items.iter().map(|i| doc_text(&i.text(&i.sentence))).collect();
    let refs: Vec<&str> = docs.iter().map(String::as_str).collect();
    let vecs = embedder.embed_many(&refs);
    let n = vecs.len();
    println!("{} notes, {} queries, vector length {}", n, queries.len(), embedder.dim());

    // ---- meaning floor ----
    println!("\nMEANING FLOOR: query against its own note, and against every other note");
    let mut own = Vec::new();
    let mut other = Vec::new();
    for (note, query) in &queries {
        let q = embedder.embed(query);
        for (j, v) in vecs.iter().enumerate() {
            let c = dot(&q, v);
            if j == *note {
                own.push(c);
            } else {
                other.push(c);
            }
        }
    }
    let (own, other) = (sorted(own), sorted(other));
    summary("own note", &own);
    summary("other notes", &other);
    println!("  floor   share of own notes kept   other notes kept per query");
    for f in [0.05f32, 0.10, 0.15, 0.20, 0.25, 0.30, 0.35, 0.40, 0.50] {
        let kept = own.iter().filter(|&&c| c >= f).count() as f32 / own.len() as f32;
        let junk = other.iter().filter(|&&c| c >= f).count() as f32 / queries.len() as f32;
        println!("  {f:.2}    {:>6.1}%                  {junk:>7.1}", kept * 100.0);
    }

    // ---- near duplicates ----
    println!("\nNEAR DUPLICATE: a note against edited copies of itself");
    type Edit = fn(&str) -> String;
    let edits: [(&str, Edit); 6] = [
        ("drop the articles", |s| s.split_whitespace().filter(|w| !matches!(w.to_lowercase().as_str(), "a" | "an" | "the")).collect::<Vec<_>>().join(" ")),
        ("add 'also' in the middle", |s| {
            let w: Vec<&str> = s.split_whitespace().collect();
            let mid = w.len() / 2;
            [&w[..mid], &["also"][..], &w[mid..]].concat().join(" ")
        }),
        ("swap two middle words", |s| {
            let mut w: Vec<&str> = s.split_whitespace().collect();
            if w.len() > 4 {
                let mid = w.len() / 2;
                w.swap(mid, mid + 1);
            }
            w.join(" ")
        }),
        ("drop the last two words", |s| {
            let w: Vec<&str> = s.split_whitespace().collect();
            w[..w.len().saturating_sub(2).max(1)].join(" ")
        }),
        ("append ', as before'", |s| format!("{}, as before", s.trim_end_matches('.'))),
        ("lower case, no stop", |s| s.trim_end_matches('.').to_lowercase()),
    ];
    for (label, edit) in &edits {
        let edited: Vec<String> = items.iter().map(|i| doc_text(&i.text(&edit(&i.sentence)))).collect();
        let refs: Vec<&str> = edited.iter().map(String::as_str).collect();
        let copies = embedder.embed_many(&refs);
        let scores = sorted(vecs.iter().zip(&copies).map(|(a, b)| dot(a, b)).collect());
        summary(label, &scores);
    }
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            pairs.push((dot(&vecs[i], &vecs[j]), i, j));
        }
    }
    let all = sorted(pairs.iter().map(|p| p.0).collect());
    println!("\nALL PAIRS of different notes");
    summary("cosine", &all);
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("  the most similar different pairs:");
    for &(c, i, j) in pairs.iter().take(12) {
        println!("    {c:.3}  {}  |  {}", items[i].text(&items[i].sentence).chars().take(70).collect::<String>(), items[j].text(&items[j].sentence).chars().take(70).collect::<String>());
    }

    // ---- semantic links ----
    println!("\nSEMANTIC LINK: pairs of different notes at or above a value");
    println!("  value   pairs   links per note   same file   3+ shared content words");
    let words: Vec<HashSet<String>> = items.iter().map(|i| content_words(&i.sentence)).collect();
    let base_same = pairs.iter().filter(|p| items[p.1].file == items[p.2].file).count() as f32 / pairs.len() as f32;
    let base_words = pairs.iter().filter(|p| words[p.1].intersection(&words[p.2]).count() >= 3).count() as f32 / pairs.len() as f32;
    println!("  (all pairs: same file {:.1}%, 3+ shared words {:.1}%)", base_same * 100.0, base_words * 100.0);
    for t in [0.3f32, 0.4, 0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95] {
        let hit: Vec<&(f32, usize, usize)> = pairs.iter().filter(|p| p.0 >= t).collect();
        let same = hit.iter().filter(|p| items[p.1].file == items[p.2].file).count();
        let shared = hit.iter().filter(|p| words[p.1].intersection(&words[p.2]).count() >= 3).count();
        let share = |k: usize| if hit.is_empty() { 0.0 } else { 100.0 * k as f32 / hit.len() as f32 };
        println!("  {t:.2}   {:>6}   {:>8.2}        {:>5.1}%      {:>5.1}%", hit.len(), 2.0 * hit.len() as f32 / n as f32, share(same), share(shared));
    }
    Ok(())
}
