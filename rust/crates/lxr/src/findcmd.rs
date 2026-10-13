//! `pgr find`: hybrid code search over the search index.

use crate::cli::FindArgs;
use crate::ctx::Ctx;
use crate::fail::Res;
use crate::rows::{call_tail, cut_row, looks_callable};
use lxr_rag::{Hit, SearchOpts};

/// Runs a search. The caller has made the index current and checked the engine.
pub fn hits(ctx: &Ctx, args: &FindArgs) -> Res<Vec<Hit>> {
    let index = ctx.search_index()?;
    // The model is opened only when the index holds vectors that a query can use.
    let embedder = if index.has_vectors() { ctx.embed.get() } else { None };
    let opts = SearchOpts { limit: args.k, mode: args.mode, alpha: None };
    Ok(index.search(&args.query, embedder, &opts))
}

/// The definition row of a hit, or `PATH:FIRST-LAST` for a hit outside any definition.
pub fn hit_row(hit: &Hit) -> String {
    if hit.symbol.is_empty() {
        return format!("{}:{}-{}", hit.path, hit.start, hit.end);
    }
    let mut row = format!("{} {}:{}-{}", hit.symbol, hit.path, hit.start, hit.end);
    let sig = if looks_callable(&hit.sig) { call_tail(&hit.sig) } else { hit.sig.as_str() };
    if !sig.is_empty() {
        row.push(' ');
        row.push_str(sig);
    }
    cut_row(row)
}

/// The lines of one hit: its row, then its documentation line when it has one.
pub fn hit_lines(hit: &Hit) -> Vec<String> {
    let mut lines = vec![hit_row(hit)];
    if !hit.doc.is_empty() {
        lines.push(format!("  {}", hit.doc));
    }
    lines
}

/// `pgr find WORDS...`.
pub fn run(ctx: &Ctx, args: &FindArgs, out: &mut String) -> Res<()> {
    let found = hits(ctx, args)?;
    if found.is_empty() {
        ln!(out, "none");
    }
    for hit in &found {
        for line in hit_lines(hit) {
            ln!(out, "{line}");
        }
    }
    Ok(())
}
