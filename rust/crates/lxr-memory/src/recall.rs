//! Recall: four strategies, reciprocal-rank fusion, Hindsight's boosts, and our check of the
//! code a note talks about.
//!
//! Where this differs from the design it follows:
//! * The fused list is cut to the budget (100, 300 or 1000) before the rank score is made, so
//!   the score spreads over at most that many candidates; it is not reranked from a wider pool.
//!   A `Low` budget therefore scores its ranks over 100 candidates, not over a wider list.
//! * A note without a time of its own is dated by when it was mentioned, so its recency decays
//!   from that moment. The neutral 0.5 applies only when that moment is zero or before.
//! * Directives share the limit and the token allowance with the ranked notes (see
//!   [`recall`]), so a bank full of rules still answers the query.

use crate::bank::{event_time, lock, top, Bank, Derived};
use crate::dates::{parse_span, unix_now, Span, DAY};
use crate::entities::ENTITY_CAP;
use crate::graph::LinkKind;
use crate::store::build_bm25;
use crate::{is_foreign, CodeView, Kind, Note, RecallOpts, Recalled, Reflection, MEANING_FLOOR};
use lxr_core::est_tokens;
use lxr_text::{code_tokens, rrf, Embedder};
use std::collections::HashMap;

/// Reciprocal-rank-fusion constant.
const RRF_K: f32 = 60.0;
/// Strength of the recency, time and proof boosts.
const ALPHA_RECENCY: f32 = 0.2;
const ALPHA_TEMPORAL: f32 = 0.2;
const ALPHA_PROOF: f32 = 0.1;
/// Factor on the score of a note whose code changed, and of one whose code is gone.
const STALE_FACTOR: f32 = 0.6;
const GONE_FACTOR: f32 = 0.4;
/// Seeds of the links strategy.
const SEED_LIMIT: usize = 20;
/// Entry points of the time strategy, and links followed from each.
const TIME_ENTRIES: usize = 10;
const TIME_SPREAD: usize = 10;
/// Decay of the score along a temporal or causal link in the time strategy.
const TIME_DECAY: f32 = 0.7;
/// Boost of a causal link in the time strategy.
const CAUSAL_BOOST: f32 = 2.0;

/// The ranked candidates of each strategy, best first, as `(slot, score)`.
#[derive(Default)]
pub(crate) struct Lists {
    pub meaning: Vec<(u32, f32)>,
    pub words: Vec<(u32, f32)>,
    pub links: Vec<(u32, f32)>,
    pub time: Vec<(u32, f32)>,
}

/// Names of the strategies, in the order of the fusion.
const STRATEGIES: [&str; 4] = ["meaning", "words", "links", "time"];

/// Which of the four strategies feed the fusion of a recall.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Strategies {
    pub meaning: bool,
    pub words: bool,
    pub links: bool,
    pub time: bool,
}

impl Strategies {
    /// All four, as [`Bank::recall`] uses them.
    pub const ALL: Strategies = Strategies { meaning: true, words: true, links: true, time: true };

    /// Only the strategy with this name (`meaning`, `words`, `links` or `time`).
    pub fn only(name: &str) -> Option<Strategies> {
        let none = Strategies { meaning: false, words: false, links: false, time: false };
        match name {
            "meaning" => Some(Strategies { meaning: true, ..none }),
            "words" => Some(Strategies { words: true, ..none }),
            "links" => Some(Strategies { links: true, ..none }),
            "time" => Some(Strategies { time: true, ..none }),
            _ => None,
        }
    }
}

impl Default for Strategies {
    fn default() -> Strategies {
        Strategies::ALL
    }
}

/// Recalls the notes that best answer `query`. See [`Bank::recall`].
pub(crate) fn recall(
    bank: &Bank,
    query: &str,
    opts: &RecallOpts,
    code: &dyn CodeView,
    embedder: Option<&Embedder>,
    strategies: Strategies,
) -> Vec<Recalled> {
    let core = &bank.core;
    if core.by_id.is_empty() || opts.limit == 0 {
        return Vec::new();
    }
    let now = opts.now.unwrap_or_else(unix_now);
    let mut guard = lock(&bank.derived);
    if let Some(embedder) = embedder {
        core.sync_embedder(&mut guard, embedder);
    }
    if guard.bm25.is_none() {
        guard.bm25 = Some(build_bm25(&core.slots));
    }
    let derived: &Derived = &guard;
    let allowed = allowed_slots(bank, opts);
    let window = window_of(query, opts, now);
    let mut lists = gather(bank, derived, query, opts, embedder, &allowed, window);
    // The links need seeds from the first two strategies even when only the links are wanted.
    if !strategies.meaning {
        lists.meaning.clear();
    }
    if !strategies.words {
        lists.words.clear();
    }
    if !strategies.links {
        lists.links.clear();
    }
    if !strategies.time {
        lists.time.clear();
    }

    // Fusion, then the rank score that stands in for a cross-encoder.
    let count = opts.budget.count();
    let ids = |list: &[(u32, f32)]| list.iter().map(|x| x.0).collect::<Vec<u32>>();
    let (m, w, l, t) = (ids(&lists.meaning), ids(&lists.words), ids(&lists.links), ids(&lists.time));
    let mut fused = fuse([&m, &w, &l, &t]);
    fused.truncate(count);
    let n = fused.len();
    let mut via = vec![0u8; core.slots.len()];
    for (bit, list) in [&m, &w, &l, &t].into_iter().enumerate() {
        for &slot in list {
            via[slot as usize] |= 1 << bit;
        }
    }
    let via_names = |slot: u32| -> Vec<&'static str> {
        (0..4usize).filter(|&bit| (via[slot as usize] >> bit) & 1 == 1).map(|bit| STRATEGIES[bit]).collect()
    };

    struct Candidate {
        slot: u32,
        score: f32,
        stale: bool,
        gone: bool,
        other: bool,
    }
    let judge_and_score = |slot: u32, base: f32| -> Option<Candidate> {
        let note = core.note_at(slot)?;
        let (stale, gone) = judge(note, code);
        let other = other_repository(note, code);
        let mut score = base;
        if gone {
            score *= GONE_FACTOR;
        } else if stale {
            score *= STALE_FACTOR;
        }
        Some(Candidate { slot, score, stale, gone, other })
    };

    let mut directives: Vec<Candidate> = Vec::new();
    let mut others: Vec<Candidate> = Vec::new();
    for (rank, &(slot, _)) in fused.iter().enumerate() {
        let Some(note) = core.note_at(slot) else { continue };
        if note.kind == Kind::Directive {
            continue;
        }
        let base = rank_score(rank, n)
            * boost(ALPHA_RECENCY, recency_signal(note, now))
            * boost(ALPHA_TEMPORAL, temporal_signal(note, window))
            * boost(ALPHA_PROOF, proof_signal(note));
        if let Some(candidate) = judge_and_score(slot, base) {
            others.push(candidate);
        }
    }
    for (slot, note) in core.slots.iter().enumerate() {
        if note.as_ref().is_some_and(|n| n.kind == Kind::Directive) && allowed[slot] {
            if let Some(candidate) = judge_and_score(slot as u32, 1.0) {
                directives.push(candidate);
            }
        }
    }
    let by_score = |a: &Candidate, b: &Candidate| b.score.total_cmp(&a.score).then(b.slot.cmp(&a.slot));
    directives.sort_by(by_score);
    // `sort_by` is stable, so equal scores keep the fused order.
    others.sort_by(|a, b| b.score.total_cmp(&a.score));

    // Directives come first, but they share the limit and the token allowance with the notes
    // that answer the query, so they may use half of each. The ranked notes take what they
    // need, and directives left over fill what the ranked notes did not use.
    let cost_of = |candidate: &Candidate| core.note_at(candidate.slot).map_or(0, |note| est_tokens(&note.text));
    let mut tokens = 0usize;
    let mut first: Vec<Candidate> = Vec::new();
    let mut later: Vec<Candidate> = Vec::new();
    for candidate in directives {
        let cost = cost_of(&candidate);
        if first.len() < opts.limit / 2 && tokens + cost <= opts.max_tokens / 2 {
            tokens += cost;
            first.push(candidate);
        } else {
            later.push(candidate);
        }
    }
    let mut ranked: Vec<Candidate> = Vec::new();
    for candidate in others {
        let cost = cost_of(&candidate);
        if first.len() + ranked.len() < opts.limit && tokens + cost <= opts.max_tokens {
            tokens += cost;
            ranked.push(candidate);
        }
    }
    for candidate in later {
        let cost = cost_of(&candidate);
        if first.len() + ranked.len() < opts.limit && tokens + cost <= opts.max_tokens {
            tokens += cost;
            first.push(candidate);
        }
    }
    first.sort_by(by_score);
    first
        .into_iter()
        .chain(ranked)
        .filter_map(|candidate| {
            let note = core.note_at(candidate.slot)?;
            Some(Recalled {
                note: note.clone(),
                score: candidate.score,
                stale: candidate.stale,
                gone: candidate.gone,
                other: candidate.other,
                via: via_names(candidate.slot),
            })
        })
        .collect()
}

/// A recall with one budget step more, sorted into directives, observations and facts.
pub(crate) fn reflect(
    bank: &Bank,
    query: &str,
    opts: &RecallOpts,
    code: &dyn CodeView,
    embedder: Option<&Embedder>,
) -> Reflection {
    let mut wider = opts.clone();
    wider.budget = opts.budget.up();
    let mut reflection = Reflection::default();
    let mut mentions: HashMap<String, Vec<String>> = HashMap::new();
    for item in recall(bank, query, &wider, code, embedder, Strategies::ALL) {
        for entity in &item.note.entities {
            mentions.entry(entity.clone()).or_default().push(item.note.id.clone());
        }
        match item.note.kind {
            Kind::Directive => reflection.directives.push(item),
            Kind::Observation => reflection.observations.push(item),
            Kind::World | Kind::Experience => reflection.facts.push(item),
        }
    }
    let mut by_entity: Vec<(String, Vec<String>)> = mentions.into_iter().collect();
    by_entity.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    reflection.by_entity = by_entity;
    reflection
}

/// Which slots pass the kind, tag and time filters. A forgotten note never does.
pub(crate) fn allowed_slots(bank: &Bank, opts: &RecallOpts) -> Vec<bool> {
    let wanted: Vec<&str> = opts.tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    bank.core
        .slots
        .iter()
        .map(|slot| {
            let Some(note) = slot else { return false };
            if !opts.kinds.is_empty() && !opts.kinds.contains(&note.kind) {
                return false;
            }
            if !wanted.is_empty() {
                let has = |tag: &str| note.tags.iter().any(|t| t.eq_ignore_ascii_case(tag));
                let pass = if opts.tags_all { wanted.iter().all(|t| has(t)) } else { wanted.iter().any(|t| has(t)) };
                if !pass {
                    return false;
                }
            }
            let (start, end) = interval(note);
            opts.since.map_or(true, |s| end >= s) && opts.until.map_or(true, |u| start <= u)
        })
        .collect()
}

/// The time a note is about, as `(first, last)`.
fn interval(note: &Note) -> Span {
    let first = note.occurred_start.or(note.occurred_end).unwrap_or(note.mentioned_at);
    let last = note.occurred_end.or(note.occurred_start).unwrap_or(note.mentioned_at);
    (first.min(last), first.max(last))
}

/// The window a recall is about: the caller's `since` and `until`, else a date in the query.
pub(crate) fn window_of(query: &str, opts: &RecallOpts, now: i64) -> Option<Span> {
    if opts.since.is_some() || opts.until.is_some() {
        let end = opts.until.unwrap_or(now);
        let start = opts.since.unwrap_or(end - 30 * DAY);
        return Some((start.min(end), start.max(end)));
    }
    parse_span(query, now, opts.utc_offset)
}

/// Runs the four strategies. Each returns at most as many candidates as the budget allows.
pub(crate) fn gather(
    bank: &Bank,
    derived: &Derived,
    query: &str,
    opts: &RecallOpts,
    embedder: Option<&Embedder>,
    allowed: &[bool],
    window: Option<Span>,
) -> Lists {
    let core = &bank.core;
    let count = opts.budget.count();
    let mut lists = Lists::default();

    // Meaning: cosine against the query's embedding.
    if let Some(embedder) = embedder {
        if derived.vecs.dim == embedder.dim() {
            let q = embedder.embed(query);
            if q.iter().any(|&x| x != 0.0) {
                let mut hits = Vec::new();
                derived.vecs.scan(&q, |slot, cosine| {
                    if cosine >= MEANING_FLOOR && allowed.get(slot as usize).copied().unwrap_or(false) {
                        hits.push((slot, cosine));
                    }
                });
                lists.meaning = top(hits, count);
            }
        }
    }

    // Words: BM25 over the code tokens of text, context, tags and entities.
    let tokens = code_tokens(query);
    if let Some(bm25) = &derived.bm25 {
        let wanted: Vec<u32> = allowed.iter().enumerate().filter(|&(_, &ok)| ok).map(|(i, _)| i as u32).collect();
        lists.words = if wanted.len() == core.by_id.len() { bm25.search(&tokens, count) } else { bm25.search_among(&tokens, &wanted, count) };
    }

    // Links: spread from the best hits over shared entities and semantic and causal links.
    let seeds: Vec<u32> = if lists.meaning.is_empty() {
        lists.words.iter().take(SEED_LIMIT).map(|x| x.0).collect()
    } else {
        lists.meaning.iter().take(SEED_LIMIT).map(|x| x.0).collect()
    };
    lists.links = link_arm(bank, derived, &seeds, allowed, count);

    // Time: notes in the window, closest to its middle first, then spread over links.
    if let Some(window) = window {
        let mut relevance: HashMap<u32, f32> = HashMap::new();
        let best_word = lists.words.first().map_or(1.0, |x| x.1.max(1e-9));
        for &(slot, score) in &lists.words {
            relevance.insert(slot, score / best_word);
        }
        for &(slot, cosine) in &lists.meaning {
            relevance.insert(slot, cosine);
        }
        lists.time = time_arm(bank, derived, window, allowed, &relevance, count);
    }
    lists
}

/// The links strategy: `tanh(0.5 x shared entities) + semantic link weight + causal link
/// weight` for every note that is not a seed.
fn link_arm(bank: &Bank, derived: &Derived, seeds: &[u32], allowed: &[bool], count: usize) -> Vec<(u32, f32)> {
    if seeds.is_empty() {
        return Vec::new();
    }
    let core = &bank.core;
    let is_seed = |slot: u32| seeds.contains(&slot);
    let mut entities: Vec<u32> = Vec::new();
    for &seed in seeds {
        for &entity in core.note_ents.get(seed as usize).map_or(&[][..], Vec::as_slice) {
            if !entities.contains(&entity) {
                entities.push(entity);
            }
        }
    }
    let mut shared: HashMap<u32, u32> = HashMap::new();
    for &entity in &entities {
        let notes = &core.ents.recs[entity as usize].notes;
        for &slot in &notes[notes.len().saturating_sub(ENTITY_CAP)..] {
            if !is_seed(slot) && allowed.get(slot as usize).copied().unwrap_or(false) {
                *shared.entry(slot).or_insert(0) += 1;
            }
        }
    }
    let mut semantic: HashMap<u32, f32> = HashMap::new();
    let mut causal: HashMap<u32, f32> = HashMap::new();
    for &seed in seeds {
        for edge in derived.graph.neighbors(seed) {
            if is_seed(edge.to) || !allowed.get(edge.to as usize).copied().unwrap_or(false) {
                continue;
            }
            let map = match edge.kind {
                LinkKind::Semantic => &mut semantic,
                LinkKind::Causal => &mut causal,
                LinkKind::Temporal => continue,
            };
            let slot = map.entry(edge.to).or_insert(0.0);
            *slot = slot.max(edge.w);
        }
    }
    let mut scored: HashMap<u32, f32> = HashMap::new();
    for (&slot, &n) in &shared {
        *scored.entry(slot).or_insert(0.0) += (0.5 * n as f32).tanh();
    }
    for map in [&semantic, &causal] {
        for (&slot, &w) in map {
            *scored.entry(slot).or_insert(0.0) += w;
        }
    }
    top(scored.into_iter().collect(), count)
}

/// The time strategy: notes whose time reaches into the window, scored by closeness to its
/// middle, and the neighbours of the ten best over temporal and causal links.
fn time_arm(bank: &Bank, derived: &Derived, window: Span, allowed: &[bool], relevance: &HashMap<u32, f32>, count: usize) -> Vec<(u32, f32)> {
    let core = &bank.core;
    let mut entries: Vec<(u32, f32)> = Vec::new();
    for (slot, note) in core.slots.iter().enumerate() {
        let Some(note) = note else { continue };
        if !allowed[slot] {
            continue;
        }
        let (first, last) = interval(note);
        if last >= window.0 && first <= window.1 {
            entries.push((slot as u32, proximity(event_time(note), window)));
        }
    }
    let rel = |slot: u32| relevance.get(&slot).copied().unwrap_or(0.0);
    entries.sort_by(|a, b| b.1.total_cmp(&a.1).then(rel(b.0).total_cmp(&rel(a.0))).then(b.0.cmp(&a.0)));
    let mut best: HashMap<u32, f32> = entries.iter().copied().collect();
    for &(slot, proximity) in entries.iter().take(TIME_ENTRIES) {
        let mut near: Vec<(u32, f32)> = derived
            .graph
            .neighbors(slot)
            .filter(|e| e.kind != LinkKind::Semantic && allowed.get(e.to as usize).copied().unwrap_or(false))
            .map(|e| (e.to, if e.kind == LinkKind::Causal { (e.w * CAUSAL_BOOST).min(1.0) } else { e.w }))
            .collect();
        near.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        for (other, weight) in near.into_iter().take(TIME_SPREAD) {
            let score = proximity * TIME_DECAY * weight;
            let slot = best.entry(other).or_insert(0.0);
            *slot = slot.max(score);
        }
    }
    let mut ranked: Vec<(u32, f32)> = best.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(rel(b.0).total_cmp(&rel(a.0))).then(b.0.cmp(&a.0)));
    ranked.truncate(count);
    ranked
}

/// Reciprocal-rank fusion of the four lists, equal weights, in the order meaning, words,
/// links, time.
pub(crate) fn fuse(lists: [&[u32]; 4]) -> Vec<(u32, f32)> {
    rrf(&lists, RRF_K, &[1.0, 1.0, 1.0, 1.0])
}

/// The score that replaces a cross-encoder: `1 - 0.9 x rank / (n - 1)`, from 1 for the best
/// to 0.1 for the last.
pub(crate) fn rank_score(rank: usize, n: usize) -> f32 {
    1.0 - 0.9 * rank as f32 / n.saturating_sub(1).max(1) as f32
}

/// `1 + alpha x (signal - 0.5)`.
pub(crate) fn boost(alpha: f32, signal: f32) -> f32 {
    1.0 + alpha * (signal - 0.5)
}

/// `clamp(1 - days / 365, 0.1, 1)` for the time the note is about, which is the moment it was
/// mentioned when it has none of its own; 0.5 only when that time is zero or before.
pub(crate) fn recency_signal(note: &Note, now: i64) -> f32 {
    let t = event_time(note);
    if t <= 0 {
        return 0.5;
    }
    let days = (now - t).max(0) as f32 / DAY as f32;
    (1.0 - days / 365.0).clamp(0.1, 1.0)
}

/// 0.5 when the recall has no window, else how close the note is to the window's middle.
pub(crate) fn temporal_signal(note: &Note, window: Option<Span>) -> f32 {
    window.map_or(0.5, |w| proximity(event_time(note), w))
}

/// `clamp(0.5 + ln(proof_count) / 10, 0, 1)`.
pub(crate) fn proof_signal(note: &Note) -> f32 {
    (0.5 + (note.proof_count.max(1) as f32).ln() / 10.0).clamp(0.0, 1.0)
}

/// `1 - min(distance from the middle of the window / half its length, 1)`, with a half length
/// of at least half a day.
pub(crate) fn proximity(t: i64, window: Span) -> f32 {
    let mid = window.0 as f64 + (window.1 - window.0) as f64 / 2.0;
    let half = ((window.1 - window.0) as f64 / 2.0).max(DAY as f64 / 2.0);
    (1.0 - ((t as f64 - mid).abs() / half).min(1.0)) as f32
}

/// Asks the code view about each anchor: `(stale, gone)`. A view that knows no code judges
/// nothing, and an anchor made in another repository than the one the view reads is not judged.
pub(crate) fn judge(note: &Note, code: &dyn CodeView) -> (bool, bool) {
    if !code.knows_code() || note.anchors.is_empty() {
        return (false, false);
    }
    let (mut stale, mut gone) = (false, false);
    for anchor in note.anchors.iter().filter(|anchor| !is_foreign(anchor, code)) {
        match code.current_hash(anchor) {
            None => gone = true,
            Some(hash) if hash != anchor.hash => stale = true,
            Some(_) => {}
        }
    }
    (stale, gone)
}

/// True when the note names code and every anchor was made in another repository than the one
/// the view reads. A view that knows no code judges nothing, so no note is of another repository.
pub(crate) fn other_repository(note: &Note, code: &dyn CodeView) -> bool {
    code.knows_code() && !note.anchors.is_empty() && note.anchors.iter().all(|anchor| is_foreign(anchor, code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Anchor;

    fn note(proof: u32, start: Option<i64>, mentioned: i64) -> Note {
        Note { id: "n000001".into(), proof_count: proof, occurred_start: start, occurred_end: start, mentioned_at: mentioned, ..Note::default() }
    }

    #[test]
    fn fusion_adds_reciprocal_ranks_and_breaks_ties_by_the_lower_id() {
        let fused = fuse([&[10, 20, 30], &[30, 10], &[20], &[30]]);
        let order: Vec<u32> = fused.iter().map(|x| x.0).collect();
        assert_eq!(order, vec![30, 10, 20]);
        assert!((fused[0].1 - (1.0 / 63.0 + 1.0 / 61.0 + 1.0 / 61.0)).abs() < 1e-6);
        assert!((fused[1].1 - (1.0 / 61.0 + 1.0 / 62.0)).abs() < 1e-6);
        assert!((fused[1].1 - fused[2].1).abs() < 1e-6);
        assert!(fuse([&[], &[], &[], &[]]).is_empty());
    }

    #[test]
    fn rank_scores_run_from_one_to_a_tenth() {
        assert_eq!(rank_score(0, 5), 1.0);
        assert!((rank_score(4, 5) - 0.1).abs() < 1e-6);
        assert!((rank_score(2, 5) - 0.55).abs() < 1e-6);
        assert_eq!(rank_score(0, 1), 1.0);
        assert_eq!(rank_score(0, 0), 1.0);
    }

    #[test]
    fn boosts_follow_the_formulas() {
        let now = 1_000 * DAY;
        assert!((boost(0.2, 1.0) - 1.1).abs() < 1e-6);
        assert!((boost(0.2, 0.1) - 0.92).abs() < 1e-6);
        assert!((boost(0.1, 0.5) - 1.0).abs() < 1e-6);
        // Recency: 1 for today, 0.1 from a year ago, 0.5 without a time.
        assert!((recency_signal(&note(1, None, now), now) - 1.0).abs() < 1e-6);
        assert!((recency_signal(&note(1, None, now - 73 * DAY), now) - 0.8).abs() < 1e-6);
        assert!((recency_signal(&note(1, Some(now - 400 * DAY), now), now) - 0.1).abs() < 1e-6);
        assert_eq!(recency_signal(&note(1, None, 0), now), 0.5);
        // Proof: 0.5 for one sighting, 0.5 + ln(p) / 10 after.
        assert!((proof_signal(&note(1, None, 1)) - 0.5).abs() < 1e-6);
        assert!((proof_signal(&note(10, None, 1)) - (0.5 + 10f32.ln() / 10.0)).abs() < 1e-6);
        assert_eq!(proof_signal(&note(u32::MAX, None, 1)), 1.0);
        // Time: 0.5 without a window, 1 at its middle, 0 at its edge.
        let window = (100 * DAY, 102 * DAY);
        assert_eq!(temporal_signal(&note(1, Some(101 * DAY), 1), None), 0.5);
        assert!((temporal_signal(&note(1, Some(101 * DAY), 1), Some(window)) - 1.0).abs() < 1e-6);
        assert!((temporal_signal(&note(1, Some(100 * DAY), 1), Some(window)) - 0.0).abs() < 1e-6);
        assert!((temporal_signal(&note(1, Some(100 * DAY + DAY / 2), 1), Some(window)) - 0.5).abs() < 1e-6);
        assert_eq!(temporal_signal(&note(1, Some(50 * DAY), 1), Some(window)), 0.0);
    }

    #[test]
    fn a_window_of_one_day_has_a_half_day_reach() {
        let day = (10 * DAY, 11 * DAY - 1);
        assert!((proximity(10 * DAY + DAY / 2, day) - 1.0).abs() < 1e-3);
        assert!(proximity(10 * DAY, day) < 0.01);
    }

    struct Fake;
    impl CodeView for Fake {
        fn anchor(&self, _: &str) -> Option<Anchor> {
            None
        }
        fn current_hash(&self, anchor: &Anchor) -> Option<u64> {
            match anchor.qual.as_str() {
                "same" => Some(anchor.hash),
                "changed" => Some(anchor.hash + 1),
                _ => None,
            }
        }
    }

    fn anchored(quals: &[&str]) -> Note {
        let anchors = quals.iter().map(|q| Anchor { path: "a.rs".into(), qual: (*q).into(), hash: 7, repo: String::new() }).collect();
        Note { anchors, ..Note::default() }
    }

    #[test]
    fn judge_marks_changed_and_missing_definitions() {
        assert_eq!(judge(&anchored(&["same"]), &Fake), (false, false));
        assert_eq!(judge(&anchored(&["same", "changed"]), &Fake), (true, false));
        assert_eq!(judge(&anchored(&["changed", "missing"]), &Fake), (true, true));
        assert_eq!(judge(&anchored(&["missing"]), &Fake), (false, true));
        assert_eq!(judge(&anchored(&[]), &Fake), (false, false));
        assert_eq!(judge(&anchored(&["missing"]), &crate::NoCode), (false, false));
    }
}
