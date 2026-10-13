//! Timings of the text engine: run with
//! `cargo run --release --example speed -p lxr-text [fixture folder]`.
//!
//! The fixture folder (default `C:\Users\Charl\Downloads\lxbench\repo`) supplies real text
//! for the embedding and BM25 timings.

use lxr_text::{code_tokens, rrf, trigram_similarity, Bm25, Embedder, Vectors};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn main() {
    let fixture = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Charl\Downloads\lxbench\repo"));
    let chunks = chunks_of(&fixture, 750);
    println!("fixture: {} chunks of about 750 characters", chunks.len());
    if chunks.is_empty() {
        eprintln!("no source text found under {}: pass a folder that holds source files", fixture.display());
        std::process::exit(1);
    }

    let Some(dir) = Embedder::locate() else {
        println!("no model found: the embedding timings are skipped");
        return;
    };
    let cache = std::env::temp_dir().join("lxr-text-speed-cache");
    let _ = fs::remove_dir_all(&cache);
    let started = Instant::now();
    let cold = Embedder::open_with_cache(&dir, &cache).expect("open");
    println!("Embedder::open, no cache (parses tokenizer.json, writes cache): {}", ms(started.elapsed()));
    assert!(!cold.used_cache());
    drop(cold);
    let mut opens = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        let model = Embedder::open_with_cache(&dir, &cache).expect("open");
        opens.push(started.elapsed());
        assert!(model.used_cache());
    }
    println!("Embedder::open, current cache: median {} (min {}, max {} of 20)", ms(median(&mut opens)), ms(opens[0]), ms(opens[19]));
    // The public entry point, with the cache in the index home.
    let _ = Embedder::open(&dir).expect("open");
    let mut opens = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        let _ = Embedder::open(&dir).expect("open");
        opens.push(started.elapsed());
    }
    println!("Embedder::open (index home cache): median {} (min {}, max {} of 20)", ms(median(&mut opens)), ms(opens[0]), ms(opens[19]));
    let model = Embedder::open_with_cache(&dir, &cache).expect("open");

    let query = "how are tokens refreshed when they expire after an hour of use";
    println!("query of {} words", query.split_whitespace().count());
    let mut times = Vec::new();
    for _ in 0..200 {
        let started = Instant::now();
        let vector = model.embed(query);
        times.push(started.elapsed());
        assert_eq!(vector.len(), model.dim());
    }
    println!("embed, one 12-word query: median {} (min {})", ms(median(&mut times)), ms(times[0]));

    let texts: Vec<&str> = (0..5000).map(|i| chunks[i % chunks.len()].as_str()).collect();
    let characters: usize = texts.iter().map(|t| t.chars().count()).sum();
    let tokens: usize = texts.iter().take(500).map(|t| model.token_ids(t).len()).sum::<usize>() * 10;
    let mut runs = Vec::new();
    for _ in 0..3 {
        let started = Instant::now();
        let vectors = model.embed_many(&texts);
        runs.push(started.elapsed());
        assert_eq!(vectors.len(), 5000);
    }
    println!(
        "embed_many, 5000 chunks (mean {} characters, about {} tokens each): best {} of 3, {:.0} chunks per second, {} threads",
        characters / 5000,
        tokens / 5000,
        ms(*runs.iter().min().unwrap()),
        5000.0 / runs.iter().min().unwrap().as_secs_f64(),
        std::thread::available_parallelism().map_or(1, |n| n.get())
    );
    let started = Instant::now();
    for text in texts.iter().take(500) {
        let _ = model.embed(text);
    }
    println!("embed, one by one, 500 chunks: {} per chunk", ms(started.elapsed() / 500));
    let started = Instant::now();
    let padded = model.embed_padded(&texts);
    println!("embed_padded, the same 5000 chunks as one batch: {}", ms(started.elapsed()));
    assert_eq!(padded.len(), 5000);

    // Vectors::nearest over 100,000 rows of 256.
    let (rows, dim) = (100_000usize, 256usize);
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 23) as f32 - 0.5
    };
    let data: Vec<Vec<f32>> = (0..rows)
        .map(|_| {
            let row: Vec<f32> = (0..dim).map(|_| next()).collect();
            let norm = row.iter().map(|v| v * v).sum::<f32>().sqrt();
            row.into_iter().map(|v| v / norm).collect()
        })
        .collect();
    let path = std::env::temp_dir().join("lxr-text-speed-vectors.bin");
    let started = Instant::now();
    Vectors::write(&path, dim, &data).expect("write");
    println!("Vectors::write, {rows} rows of {dim}: {}", ms(started.elapsed()));
    let started = Instant::now();
    let vectors = Vectors::open(&path).expect("open");
    println!("Vectors::open: {}", ms(started.elapsed()));
    let query: Vec<f32> = data[4242].clone();
    let mut times = Vec::new();
    for _ in 0..30 {
        let started = Instant::now();
        let hits = vectors.nearest(&query, 10);
        times.push(started.elapsed());
        assert_eq!(hits[0].0, 4242);
    }
    println!("Vectors::nearest, {rows} rows of {dim}, top 10: median {} (min {})", ms(median(&mut times)), ms(times[0]));
    let subset: Vec<u32> = (0..rows as u32).step_by(10).collect();
    let started = Instant::now();
    let _ = vectors.nearest_among(&query, &subset, 10);
    println!("Vectors::nearest_among, {} rows: {}", subset.len(), ms(started.elapsed()));
    drop(vectors);
    let _ = fs::remove_file(&path);

    // Bm25::search over 20,000 documents made from real chunks.
    let token_lists: Vec<Vec<String>> = chunks.iter().map(|c| code_tokens(c)).collect();
    let docs: Vec<Vec<String>> = (0..20_000).map(|i| token_lists[i % token_lists.len()].clone()).collect();
    let started = Instant::now();
    let index = Bm25::build(docs);
    println!("Bm25::build, {} documents: {}", index.len(), ms(started.elapsed()));
    for text in ["parse the configuration file", "build_graph", "def extract python class function"] {
        let query = code_tokens(text);
        let mut times = Vec::new();
        for _ in 0..100 {
            let started = Instant::now();
            let hits = index.search(&query, 10);
            times.push(started.elapsed());
            assert!(hits.len() <= 10);
        }
        println!("Bm25::search, 20000 documents, {:?} ({} terms): median {} (min {})", text, query.len(), ms(median(&mut times)), ms(times[0]));
    }

    // The small functions.
    let a: Vec<u32> = (0..50).collect();
    let b: Vec<u32> = (25..75).rev().collect();
    let started = Instant::now();
    for _ in 0..10_000 {
        let _ = rrf(&[&a, &b], 60.0, &[0.5, 0.5]);
    }
    println!("rrf of two lists of 50: {} per call", ms(started.elapsed() / 10_000));
    let started = Instant::now();
    for _ in 0..10_000 {
        let _ = trigram_similarity("parse_config_file", "ParseConfigFile");
    }
    println!("trigram_similarity of two names: {} per call", ms(started.elapsed() / 10_000));
    let _ = fs::remove_dir_all(&cache);
}

/// Text cut from the source files under `root` into pieces of about `size` characters,
/// at line ends.
fn chunks_of(root: &Path, size: usize) -> Vec<String> {
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    let mut chunks = Vec::new();
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else { continue };
        let mut current = String::new();
        for line in text.split_inclusive('\n') {
            if !current.is_empty() && current.chars().count() + line.chars().count() > size {
                chunks.push(std::mem::take(&mut current));
            }
            current.push_str(line);
        }
        if current.trim().len() > 80 {
            chunks.push(current);
        }
    }
    chunks
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if ![".git", "node_modules", "__pycache__", "graphify-out"].contains(&name.as_str()) {
                walk(&path, out);
            }
        } else if matches!(path.extension().and_then(|e| e.to_str()), Some("py" | "rs" | "js" | "ts" | "go" | "java" | "c" | "cpp" | "rb")) {
            out.push(path);
        }
    }
}

fn median(times: &mut [Duration]) -> Duration {
    times.sort();
    times[times.len() / 2]
}

fn ms(d: Duration) -> String {
    format!("{:.3} ms", d.as_secs_f64() * 1000.0)
}
