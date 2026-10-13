//! Speed and size of a bank of 5,000 notes with the real embedder.
//!
//! Usage: `membench [NOTES]` (default 5000). Run it as a release build:
//! `cargo run --release -p lxr-memory --example membench`.
//!
//! The notes are generated, deterministically, from a small grammar: about 120 characters
//! each, 400 distinct entity names (made-up words in backticks), a different pair of them in
//! every note. It reports:
//! * the time to retain all of them, one `retain` call per note, and per call;
//! * the size of the three files;
//! * the time to open the bank with current caches (median of 20);
//! * the time to rebuild the caches when `index.bin` and `vectors.bin` are gone;
//! * the median time of 50 recalls at the Mid budget.

use lxr_memory::{Bank, Budget, NoCode, RecallOpts, RetainOpts};
use lxr_text::Embedder;
use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const VERBS: [&str; 20] = [
    "reads", "writes", "caches", "validates", "retries", "flushes", "compacts", "indexes", "renders", "parses", "signs", "queues", "drops",
    "merges", "splits", "tracks", "audits", "encodes", "polls", "seals",
];
const NOUNS: [&str; 20] = [
    "request", "snapshot", "manifest", "payload", "ticket", "session", "journal", "bundle", "report", "digest", "channel", "ledger", "token",
    "segment", "record", "buffer", "profile", "archive", "schedule", "message",
];
const PHASES: [&str; 12] = [
    "startup", "shutdown", "recovery", "migration", "rollout", "cleanup", "warmup", "failover", "handoff", "audit", "upgrade", "backup",
];

fn made_up_words(count: usize, rng: &mut Rng) -> Vec<String> {
    let consonants: Vec<char> = "bdfgklmnprstvz".chars().collect();
    let vowels: Vec<char> = "aeiou".chars().collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    while out.len() < count {
        let mut word = String::new();
        for _ in 0..3 {
            word.push(consonants[rng.below(consonants.len())]);
            word.push(vowels[rng.below(vowels.len())]);
        }
        word.push(consonants[rng.below(consonants.len())]);
        if seen.insert(word.clone()) {
            out.push(word);
        }
    }
    out
}

fn note_text(rng: &mut Rng, entities: &[String]) -> String {
    let (a, mut b) = (rng.below(entities.len()), rng.below(entities.len()));
    if a == b {
        b = (b + 1) % entities.len();
    }
    let (v1, v2) = (VERBS[rng.below(VERBS.len())], VERBS[rng.below(VERBS.len())]);
    let (n1, n2) = (NOUNS[rng.below(NOUNS.len())], NOUNS[rng.below(NOUNS.len())]);
    let phase = PHASES[rng.below(PHASES.len())];
    match rng.below(3) {
        0 => format!("The `{}` module {v1} the {n1} cache whenever `{}` {v2} a {n2} during {phase} handling.", entities[a], entities[b]),
        1 => format!("After `{}` {v1} the {n1}, the `{}` worker {v2} every {n2} before the {phase} step finishes.", entities[a], entities[b]),
        _ => format!("During {phase}, `{}` {v1} each {n1} and hands the {n2} to `{}`, which {v2} it again.", entities[a], entities[b]),
    }
}

fn query_text(rng: &mut Rng, entities: &[String]) -> String {
    let a = rng.below(entities.len());
    format!("how does `{}` handle the {} {}", entities[a], NOUNS[rng.below(NOUNS.len())], PHASES[rng.below(PHASES.len())])
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn main() -> Result<(), Box<dyn Error>> {
    let count: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(5000);
    let model = Embedder::locate().ok_or("no embedding model is installed")?;
    let embedder = Embedder::open(&model)?;
    let dir = std::env::temp_dir().join(format!("lxr-membench-{}", if std::env::var_os("MEMBENCH_KEEP").is_some() { 0 } else { std::process::id() }));
    let _ = fs::remove_dir_all(&dir);
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    let entities = made_up_words(400, &mut rng);
    let base: i64 = 1_773_230_400;

    // ---- retain ----
    let mut bank = Bank::open(&dir)?;
    let mut chars = 0usize;
    let mut each = Vec::with_capacity(count);
    let started = Instant::now();
    for i in 0..count {
        let text = note_text(&mut rng, &entities);
        chars += text.chars().count();
        let opts = RetainOpts { now: Some(base + i as i64 * 600), ..RetainOpts::default() };
        let t = Instant::now();
        bank.retain(&text, &opts, &NoCode, Some(&embedder))?;
        each.push(t.elapsed());
    }
    let retain_total = started.elapsed();
    let flush_started = Instant::now();
    bank.flush()?;
    let flush_time = flush_started.elapsed();
    let stats = bank.stats();
    println!("RETAIN {count} notes, one call each, {:.0} characters on average", chars as f64 / count as f64);
    println!("  total {:.2} s   per call: median {:.3} ms  p99 {:.3} ms", retain_total.as_secs_f64(), ms(median(each.clone())), {
        each.sort();
        ms(each[each.len() * 99 / 100])
    });
    println!("  flush of the caches afterwards: {:.1} ms", ms(flush_time));
    println!(
        "  bank: {} notes (World {}, Experience {}, Observation {}, Directive {}), {} entities, {} links",
        stats.notes, stats.by_kind[0], stats.by_kind[1], stats.by_kind[2], stats.by_kind[3], stats.entities, stats.links
    );
    println!("FILES");
    let (log, vectors, index) = (size(&dir.join("notes.jsonl")), size(&dir.join("vectors.bin")), size(&dir.join("index.bin")));
    println!("  notes.jsonl {log} bytes   vectors.bin {vectors} bytes   index.bin {index} bytes   total {} bytes", log + vectors + index);
    drop(bank);

    // ---- open with current caches ----
    let mut opens = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let bank = Bank::open(&dir)?;
        opens.push(t.elapsed());
        assert_eq!(bank.len(), stats.notes);
    }
    println!("OPEN with current caches (median of 20): {:.2} ms   (min {:.2} ms, max {:.2} ms)", ms(median(opens.clone())), ms(*opens.iter().min().unwrap()), ms(*opens.iter().max().unwrap()));

    // ---- rebuild ----
    fs::remove_file(dir.join("index.bin"))?;
    fs::remove_file(dir.join("vectors.bin"))?;
    let t = Instant::now();
    let mut bank = Bank::open(&dir)?;
    let open_time = t.elapsed();
    let t = Instant::now();
    bank.recall("rebuild", &RecallOpts::default(), &NoCode, Some(&embedder));
    let vectors_time = t.elapsed();
    let t = Instant::now();
    bank.flush()?;
    let write_time = t.elapsed();
    println!(
        "REBUILD of the caches from notes.jsonl: {:.0} ms in all = open {:.0} ms (entities, temporal and causal links, no embedder) + embed {} notes and make {} semantic links and the keyword index {:.0} ms + write {:.0} ms",
        ms(open_time + vectors_time + write_time),
        ms(open_time),
        bank.len(),
        bank.stats().links,
        ms(vectors_time),
        ms(write_time)
    );
    assert_eq!(bank.stats().notes, stats.notes);
    drop(bank);

    // ---- recall ----
    let bank = Bank::open(&dir)?;
    let opts = RecallOpts { budget: Budget::Mid, now: Some(base + count as i64 * 600), ..RecallOpts::default() };
    bank.recall("warm up", &opts, &NoCode, Some(&embedder));
    let mut times = Vec::new();
    let mut returned = 0;
    for _ in 0..50 {
        let query = query_text(&mut rng, &entities);
        let t = Instant::now();
        let found = bank.recall(&query, &opts, &NoCode, Some(&embedder));
        times.push(t.elapsed());
        returned += found.len();
    }
    let slowest = *times.iter().max().unwrap();
    println!(
        "RECALL at the Mid budget, 50 queries (all four strategies, embedder): median {:.2} ms   max {:.2} ms   ({:.1} notes returned per query)",
        ms(median(times)),
        ms(slowest),
        returned as f64 / 50.0
    );
    drop(bank);
    if std::env::var_os("MEMBENCH_KEEP").is_none() {
        let _ = fs::remove_dir_all(&dir);
    } else {
        println!("kept {}", dir.display());
    }
    Ok(())
}
