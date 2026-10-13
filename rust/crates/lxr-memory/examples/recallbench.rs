//! Recall quality on real text: do the strategies find a note again from half of its words?
//!
//! Usage: `recallbench DOCSTRINGS.json`, the file written by `pick_docstrings.py`: 200
//! notes of the form "NAME in FILE: first docstring sentence" and 60 queries, each the
//! sentence of one note with the function name and file name removed and every second word
//! dropped. It retains the 200 notes into a fresh bank and reports hit@1, hit@5 and the mean
//! reciprocal rank (ranks counted to 20) for all strategies, for the words alone and for the
//! meaning alone.

use lxr_memory::{Bank, Kind, NoCode, RecallOpts, Recalled, RetainOpts, Strategies};
use lxr_text::Embedder;
use serde_json::Value;
use std::error::Error;
use std::fs;

fn rank_of(found: &[Recalled], id: &str) -> Option<usize> {
    found.iter().position(|r| r.note.id == id).map(|p| p + 1)
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("usage: recallbench DOCSTRINGS.json")?;
    let model = Embedder::locate().ok_or("no embedding model is installed")?;
    let embedder = Embedder::open(&model)?;
    let data: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    let dir = std::env::temp_dir().join(format!("lxr-recallbench-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let mut bank = Bank::open(&dir)?;
    let now: i64 = 1_773_230_400;
    let mut ids = Vec::new();
    for note in data["notes"].as_array().ok_or("no notes")? {
        let text = note["text"].as_str().unwrap_or_default();
        let opts = RetainOpts { now: Some(now), ..RetainOpts::default() };
        let made = bank.retain(text, &opts, &NoCode, Some(&embedder))?;
        ids.push(made[0].id.clone());
    }
    let queries: Vec<(usize, String)> = data["queries"]
        .as_array()
        .ok_or("no queries")?
        .iter()
        .map(|q| (q["note"].as_u64().unwrap_or(0) as usize, q["query"].as_str().unwrap_or_default().to_string()))
        .collect();
    let stats = bank.stats();
    println!("{} notes retained ({} in the bank, {} links), {} queries", ids.len(), stats.notes, stats.links, queries.len());
    let configs: [(&str, Strategies); 4] = [
        ("all strategies", Strategies::ALL),
        ("words only", Strategies::only("words").unwrap_or_default()),
        ("meaning only", Strategies::only("meaning").unwrap_or_default()),
        ("meaning + words", Strategies { links: false, time: false, ..Strategies::ALL }),
    ];
    let modes: [(&str, Vec<Kind>); 2] = [
        ("observations included: any note may come back, the target must be first", Vec::new()),
        ("facts only (kinds = World): the 200 retained notes compete", vec![Kind::World]),
    ];
    for (mode, kinds) in modes {
        println!("\n{mode}");
        let opts = RecallOpts { limit: 20, max_tokens: 1_000_000, kinds, now: Some(now), ..RecallOpts::default() };
        println!("{:<16} {:>7} {:>7} {:>7} {:>9}", "", "hit@1", "hit@5", "MRR", "not in 20");
        for (label, strategies) in &configs {
            let (mut hit1, mut hit5, mut rr, mut lost) = (0, 0, 0.0f64, 0);
            for (note, query) in &queries {
                let found = bank.recall_with(query, &opts, &NoCode, Some(&embedder), *strategies);
                match rank_of(&found, &ids[*note]) {
                    Some(rank) => {
                        hit1 += usize::from(rank == 1);
                        hit5 += usize::from(rank <= 5);
                        rr += 1.0 / rank as f64;
                    }
                    None => lost += 1,
                }
            }
            let n = queries.len() as f64;
            println!("{label:<16} {:>6.1}% {:>6.1}% {:>7.3} {:>9}", 100.0 * hit1 as f64 / n, 100.0 * hit5 as f64 / n, rr / n, lost);
        }
    }
    let opts = RecallOpts { limit: 20, max_tokens: 1_000_000, now: Some(now), ..RecallOpts::default() };
    if std::env::var_os("SHOW_MISSES").is_some() {
        for (note, query) in &queries {
            let found = bank.recall(query, &opts, &NoCode, Some(&embedder));
            let rank = rank_of(&found, &ids[*note]);
            if rank != Some(1) {
                println!("rank {:?}: {query:?}\n    wanted {}\n    got    {}", rank, data["notes"][*note]["text"], found.first().map_or("", |f| f.note.text.as_str()));
            }
        }
    }
    drop(bank);
    let _ = fs::remove_dir_all(&dir);
    Ok(())
}
