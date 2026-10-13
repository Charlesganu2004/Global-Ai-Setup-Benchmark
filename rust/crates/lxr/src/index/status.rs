//! The status line. It reads the manifest and the bank, never the graph or the search
//! index, and it never builds anything.

use super::manifest::Manifest;
use super::parts_present;
use crate::ctx::Ctx;
use crate::dates::{age_word, now_nanos};
use crate::fail::Res;
use lxr_memory::Bank;

/// How many notes the memory bank holds. 0 when there is no bank folder yet.
pub fn note_count(ctx: &Ctx) -> usize {
    if !ctx.bank_dir.is_dir() {
        return 0;
    }
    Bank::open(&ctx.bank_dir).map(|bank| bank.len()).unwrap_or(0)
}

/// `pgr: NAME | graph ready (...) | search ready (...) | memory N notes.`
pub fn line(ctx: &Ctx) -> Res<String> {
    ctx.check_folder()?;
    let manifest = match Manifest::load(&ctx.dir).filter(|_| parts_present(&ctx.dir)) {
        Some(manifest) => manifest,
        None => return Ok(format!("pgr: {} | no index yet (run: pgr refresh)", ctx.name)),
    };
    let age = age_word((now_nanos().saturating_sub(manifest.built) / 1_000_000_000) as i64);
    let counts = manifest.counts;
    let vectors = if manifest.embeddings { "embeddings" } else { "words only" };
    Ok(format!(
        "pgr: {} | graph ready ({} symbols, {} links, {} old) | search ready ({} chunks, {}) | memory {} notes.",
        ctx.name,
        counts.symbols,
        counts.links,
        age,
        counts.chunks,
        vectors,
        note_count(ctx)
    ))
}
