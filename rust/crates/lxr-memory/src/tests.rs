//! Tests of the bank as a whole: retain, recall, forget and the files.

use crate::bank::lock;
use crate::dates::midnight;
use crate::recall::{boost, proof_signal, rank_score, recency_signal};
use crate::testutil::{embedder, recall_at, retain_at, FakeCode, TempDir, DAY, NOW};
use crate::*;
use std::fs;

fn open(dir: &TempDir) -> Bank {
    Bank::open(&dir.bank("b")).expect("open the bank")
}

fn add(bank: &mut Bank, text: &str, now: i64) -> Vec<Retained> {
    bank.retain(text, &retain_at(now), &NoCode, None).expect("retain")
}

fn add_tagged(bank: &mut Bank, text: &str, tags: &[&str], now: i64) -> Vec<Retained> {
    let opts = RetainOpts { tags: tags.iter().map(|t| t.to_string()).collect(), ..retain_at(now) };
    bank.retain(text, &opts, &NoCode, None).expect("retain")
}

fn ids(found: &[Recalled]) -> Vec<String> {
    found.iter().map(|r| r.note.id.clone()).collect()
}

fn texts(found: &[Recalled]) -> Vec<String> {
    found.iter().map(|r| r.note.text.clone()).collect()
}

fn observations(bank: &Bank) -> Vec<Note> {
    bank.notes().into_iter().filter(|n| n.kind == Kind::Observation).cloned().collect()
}

// ---- retain ----

#[test]
fn a_text_with_line_breaks_becomes_one_note_per_fact() {
    let dir = TempDir::new("split");
    let mut bank = open(&dir);
    let out = add(&mut bank, "The loader reads the config file.\nThe writer flushes after every append.", NOW);
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|r| r.outcome == RetainOutcome::Added));
    assert_eq!(bank.len(), 2);
    assert_eq!(bank.notes()[0].text, "The writer flushes after every append.", "newest first");
    assert_eq!(bank.get(&out[0].id).unwrap().text, "The loader reads the config file.");
}

#[test]
fn with_split_off_the_whole_text_is_one_note() {
    let dir = TempDir::new("nosplit");
    let mut bank = open(&dir);
    let opts = RetainOpts { split: false, ..retain_at(NOW) };
    let out = bank.retain("First fact.\nSecond fact.", &opts, &NoCode, None).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(bank.get(&out[0].id).unwrap().text, "First fact. Second fact.");
    assert!(bank.retain("   ", &opts, &NoCode, None).unwrap().is_empty());
}

#[test]
fn a_note_has_the_fields_the_caller_and_the_clock_give() {
    let dir = TempDir::new("fields");
    let mut bank = open(&dir);
    let opts = RetainOpts {
        kind: Kind::Experience,
        context: " from the standup ".into(),
        tags: vec!["Team".into(), "team".into(), "ops".into()],
        entities: vec!["Radana".into()],
        ..retain_at(NOW)
    };
    let id = bank.retain("We moved the deploy window to Friday.", &opts, &NoCode, None).unwrap()[0].id.clone();
    let note = bank.get(&id).unwrap();
    assert_eq!(note.kind, Kind::Experience);
    assert_eq!(note.context, "from the standup");
    assert_eq!(note.tags, vec!["Team", "ops"]);
    assert_eq!(note.entities, vec!["Radana"]);
    assert_eq!((note.mentioned_at, note.created_at, note.updated_at, note.proof_count), (NOW, NOW, NOW, 1));
    assert!(note.sources.is_empty() && note.history.is_empty() && note.occurred_start.is_none());
}

#[test]
fn spelling_variants_of_a_name_become_one_entity() {
    let dir = TempDir::new("variants");
    let mut bank = open(&dir);
    let a = add(&mut bank, "`parse_config` reads the file.", NOW)[0].id.clone();
    let b = add(&mut bank, "The function parseConfig is slow.", NOW + 10)[0].id.clone();
    assert_eq!(bank.get(&a).unwrap().entities, vec!["parse_config"]);
    assert_eq!(bank.get(&b).unwrap().entities, vec!["parse_config"]);
    assert_eq!(bank.stats().entities, 1);
}

#[test]
fn a_misspelling_merges_by_the_formula_when_the_names_were_seen_together() {
    let dir = TempDir::new("merge");
    let mut bank = open(&dir);
    let with_loader = |bank: &mut Bank, text: &str, now: i64| {
        let opts = RetainOpts { entities: vec!["loader".into()], ..retain_at(now) };
        bank.retain(text, &opts, &NoCode, None).unwrap()[0].id.clone()
    };
    with_loader(&mut bank, "parse_config and loader work together.", NOW);
    let second = with_loader(&mut bank, "parse_confg and loader fail together.", NOW + 60);
    assert_eq!(bank.get(&second).unwrap().entities, vec!["loader", "parse_config"]);
    assert_eq!(bank.stats().entities, 2);
    // Alone, a week later, the same misspelling is a new entity: 0.5 x similarity is not enough.
    let third = add(&mut bank, "Then parse_confg broke again.", NOW + 9 * DAY)[0].id.clone();
    assert_eq!(bank.get(&third).unwrap().entities, vec!["parse_confg"]);
    assert_eq!(bank.stats().entities, 3);
}

#[test]
fn anchors_name_the_definitions_a_note_talks_about() {
    let dir = TempDir::new("anchors");
    let code = FakeCode::default();
    code.define("parse_config", "src/config.rs", 42);
    let mut bank = open(&dir);
    let id = bank.retain("parse_config is called by load_all.", &retain_at(NOW), &code, None).unwrap()[0].id.clone();
    let anchors = &bank.get(&id).unwrap().anchors;
    assert_eq!(anchors, &vec![Anchor { path: "src/config.rs".into(), qual: "parse_config".into(), hash: 42 }]);
    let none = add(&mut bank, "load_all is plain.", NOW);
    assert!(bank.get(&none[0].id).unwrap().anchors.is_empty());
}

#[test]
fn times_come_from_the_caller_or_from_a_date_in_the_text() {
    let dir = TempDir::new("times");
    let mut bank = open(&dir);
    let day = |y, m, d| midnight(y, m, d).unwrap();
    let a = add(&mut bank, "On 2026-03-05 the loader broke.", NOW)[0].id.clone();
    assert_eq!(bank.get(&a).unwrap().occurred_start, Some(day(2026, 3, 5)));
    assert_eq!(bank.get(&a).unwrap().occurred_end, Some(day(2026, 3, 5) + DAY - 1));
    let b = add(&mut bank, "It was fixed yesterday.", NOW)[0].id.clone();
    assert_eq!(bank.get(&b).unwrap().occurred_start, Some(day(2026, 3, 10)));
    let c = add(&mut bank, "The cache was cleared last week.", NOW)[0].id.clone();
    assert_eq!(bank.get(&c).unwrap().occurred_start, Some(day(2026, 3, 2)));
    let d = add(&mut bank, "The disk filled up 3 days ago.", NOW)[0].id.clone();
    assert_eq!(bank.get(&d).unwrap().occurred_start, Some(day(2026, 3, 8)));
    let e = add(&mut bank, "We met on March 3 to plan it.", NOW)[0].id.clone();
    assert_eq!(bank.get(&e).unwrap().occurred_start, Some(day(2026, 3, 3)));
    let f = add(&mut bank, "The schema changed in February 2026.", NOW)[0].id.clone();
    assert_eq!(bank.get(&f).unwrap().occurred_start, Some(day(2026, 2, 1)));
    assert_eq!(bank.get(&f).unwrap().occurred_end, Some(day(2026, 3, 1) - 1));
    let g = add(&mut bank, "Nothing dated in this one.", NOW)[0].id.clone();
    assert_eq!(bank.get(&g).unwrap().occurred_start, None);
    let opts = RetainOpts { when: Some(1_000_000), ..retain_at(NOW) };
    let h = bank.retain("It happened on 2026-03-05 but the caller knows better.", &opts, &NoCode, None).unwrap()[0].id.clone();
    assert_eq!((bank.get(&h).unwrap().occurred_start, bank.get(&h).unwrap().occurred_end), (Some(1_000_000), Some(1_000_000)));
}

#[test]
fn a_repeat_reinforces_the_note_instead_of_adding_one() {
    let dir = TempDir::new("repeat");
    let mut bank = open(&dir);
    let first = add(&mut bank, "The loader reads the config file.", NOW);
    let second = add(&mut bank, "the loader reads the  CONFIG file", NOW + 5 * DAY);
    assert_eq!(second[0].outcome, RetainOutcome::Reinforced);
    assert_eq!(second[0].id, first[0].id);
    assert_eq!(bank.len(), 1);
    let note = bank.get(&first[0].id).unwrap();
    assert_eq!((note.proof_count, note.updated_at, note.created_at), (2, NOW + 5 * DAY, NOW));
    assert_eq!(note.history.len(), 1);
    assert!(note.history[0].contains("2026-03-16") && note.history[0].contains("same text"), "{:?}", note.history);
    // Another kind is another fact.
    let opts = RetainOpts { kind: Kind::Experience, ..retain_at(NOW) };
    let other = bank.retain("The loader reads the config file.", &opts, &NoCode, None).unwrap();
    assert_eq!(other[0].outcome, RetainOutcome::Added);
    // A repeat with new tags adds them.
    let tagged = add_tagged(&mut bank, "The loader reads the config file.", &["ops"], NOW + 6 * DAY);
    assert_eq!(tagged[0].outcome, RetainOutcome::Reinforced);
    assert_eq!(bank.get(&first[0].id).unwrap().tags, vec!["ops"]);
    assert_eq!(bank.get(&first[0].id).unwrap().proof_count, 3);
}

#[test]
fn a_cue_phrase_links_neighbouring_facts_of_one_call() {
    let dir = TempDir::new("causal");
    let mut bank = open(&dir);
    let out = add(&mut bank, "The nightly build failed on the main branch.\nIt was because the cache was stale.\nTherefore we clear the cache first.", NOW);
    assert_eq!(out.len(), 3);
    let (a, b, c) = (bank.get(&out[0].id).unwrap(), bank.get(&out[1].id).unwrap(), bank.get(&out[2].id).unwrap());
    assert_eq!(a.caused_by, vec![b.id.clone()], "the explanation follows the failure");
    assert_eq!(c.caused_by, vec![b.id.clone()], "the conclusion follows from the explanation");
    assert!(b.caused_by.is_empty());
    assert_eq!(bank.stats().links, 2 + 3, "two causal links and three temporal ones");
    let derived = lock(&bank.derived);
    let causal = derived.graph.neighbors(1).filter(|e| e.kind == crate::graph::LinkKind::Causal).count();
    assert_eq!(causal, 2);
    assert!(derived.graph.neighbors(1).filter(|e| e.kind == crate::graph::LinkKind::Causal).all(|e| e.w == 1.0));
}

#[test]
fn notes_less_than_a_day_apart_get_temporal_links_with_the_formula_weight() {
    let dir = TempDir::new("temporal");
    let mut bank = open(&dir);
    add(&mut bank, "First event happened here.", NOW);
    add(&mut bank, "Second event happened here.", NOW + 12 * 3600);
    add(&mut bank, "Third event happened here.", NOW + 40 * 3600);
    let derived = lock(&bank.derived);
    let first: Vec<_> = derived.graph.neighbors(0).collect();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].to, 1);
    assert!((first[0].w - 0.5).abs() < 1e-3, "12 hours apart: 1 - 12/24");
    assert_eq!(derived.graph.neighbors(2).count(), 0, "28 hours from the second, 40 from the first");
}

#[test]
fn a_note_makes_at_most_twenty_temporal_links() {
    let dir = TempDir::new("temporal-cap");
    let mut bank = open(&dir);
    for n in 0..30 {
        add(&mut bank, &format!("Event number {n} took place."), NOW + n);
    }
    let derived = lock(&bank.derived);
    let own: usize = derived.graph.neighbors(29).count();
    assert_eq!(own, 20);
}

// ---- observations ----

#[test]
fn three_notes_with_one_entity_and_tags_make_an_observation() {
    let dir = TempDir::new("observation");
    let mut bank = open(&dir);
    let mut made = Vec::new();
    for n in 0..3 {
        made.push(add_tagged(&mut bank, &format!("parse_config handles case {n}."), &["proj"], NOW + n)[0].id.clone());
        assert_eq!(observations(&bank).len(), usize::from(n == 2));
    }
    let obs = observations(&bank);
    assert_eq!(obs.len(), 1);
    let o = &obs[0];
    assert_eq!(o.entities, vec!["parse_config"]);
    assert_eq!(o.tags, vec!["proj"]);
    assert_eq!(o.sources, made);
    assert_eq!(o.proof_count, 3);
    assert_eq!(o.text, "parse_config: parse_config handles case 0. parse_config handles case 1. parse_config handles case 2.");
    assert_eq!(bank.len(), 4);
    assert_eq!(bank.stats().by_kind, [3, 0, 1, 0]);
}

#[test]
fn an_observation_is_refreshed_when_its_sources_change() {
    let dir = TempDir::new("observation-refresh");
    let mut bank = open(&dir);
    for n in 0..3 {
        add_tagged(&mut bank, &format!("parse_config handles case {n}."), &["proj"], NOW + n);
    }
    let id = observations(&bank)[0].id.clone();
    let fourth = add_tagged(&mut bank, "parse_config handles case 3.", &["PROJ"], NOW + 100)[0].id.clone();
    let obs = observations(&bank);
    assert_eq!(obs.len(), 1, "one observation per entity and tag set, tags compared without case");
    assert_eq!(obs[0].id, id);
    assert_eq!(obs[0].sources.len(), 4);
    assert_eq!(obs[0].sources[3], fourth);
    assert_eq!(obs[0].proof_count, 4);
    assert_eq!(obs[0].updated_at, NOW + 100);
    assert_eq!(obs[0].history.len(), 2);
    assert!(obs[0].text.ends_with("parse_config handles case 3."));
    // Another tag set is another group, and two notes are too few.
    add_tagged(&mut bank, "parse_config handles case 4.", &["other"], NOW + 200);
    add_tagged(&mut bank, "parse_config handles case 5.", &["other"], NOW + 201);
    assert_eq!(observations(&bank).len(), 1);
    add_tagged(&mut bank, "parse_config handles case 6.", &["other"], NOW + 202);
    assert_eq!(observations(&bank).len(), 2);
    // An observation is never the source of another one.
    for o in observations(&bank) {
        assert!(o.sources.iter().all(|s| bank.get(s).unwrap().kind != Kind::Observation));
    }
}

#[test]
fn an_observation_text_is_cut_to_600_characters() {
    let dir = TempDir::new("observation-cut");
    let mut bank = open(&dir);
    for n in 0..3 {
        add(&mut bank, &format!("parse_config case{n} {}", "word ".repeat(55)), NOW + n);
    }
    let obs = observations(&bank);
    assert_eq!(obs[0].text.chars().count(), 600);
    assert!(obs[0].text.starts_with("parse_config: parse_config case0 word"));
}

#[test]
fn forgetting_a_source_dissolves_an_observation_with_too_few_sources() {
    let dir = TempDir::new("observation-forget");
    let mut bank = open(&dir);
    let mut made = Vec::new();
    for n in 0..4 {
        made.push(add(&mut bank, &format!("parse_config handles case {n}."), NOW + n)[0].id.clone());
    }
    let obs_id = observations(&bank)[0].id.clone();
    assert!(bank.forget(&made[1]).unwrap());
    let obs = observations(&bank);
    assert_eq!(obs[0].id, obs_id);
    assert_eq!(obs[0].sources.len(), 3);
    assert!(!obs[0].sources.contains(&made[1]));
    assert!(bank.forget(&made[2]).unwrap());
    assert!(observations(&bank).is_empty());
    assert!(bank.get(&obs_id).is_none());
    assert_eq!(bank.len(), 2);
}

// ---- recall ----

fn world(texts: &[&str]) -> (TempDir, Bank, Vec<String>) {
    let dir = TempDir::new("recall");
    let mut bank = open(&dir);
    let mut made = Vec::new();
    for (n, text) in texts.iter().enumerate() {
        made.push(add(&mut bank, text, NOW + n as i64)[0].id.clone());
    }
    (dir, bank, made)
}

#[test]
fn the_words_strategy_finds_notes_by_their_tokens() {
    let (_dir, bank, made) = world(&["the scheduler retries failed jobs", "the renderer paints widgets", "disk usage is reported daily"]);
    let found = bank.recall("scheduler retries", &recall_at(NOW), &NoCode, None);
    assert_eq!(ids(&found), vec![made[0].clone()]);
    assert_eq!(found[0].via, vec!["words"]);
    assert!(bank.recall("zebra", &recall_at(NOW), &NoCode, None).is_empty());
}

#[test]
fn words_also_match_the_entities_tags_and_context_of_a_note() {
    let dir = TempDir::new("words-fields");
    let mut bank = open(&dir);
    let opts = RetainOpts { context: "weekly sync".into(), tags: vec!["infra".into()], entities: vec!["Radana".into()], ..retain_at(NOW) };
    let id = bank.retain("We agreed on the window.", &opts, &NoCode, None).unwrap()[0].id.clone();
    for query in ["sync", "infra", "radana"] {
        assert_eq!(ids(&bank.recall(query, &recall_at(NOW), &NoCode, None)), vec![id.clone()], "{query}");
    }
}

#[test]
fn the_links_strategy_reaches_notes_that_share_an_entity_with_a_hit() {
    let (_dir, bank, made) = world(&[
        "the `job_runner` retries failed work",
        "metrics for the `job_runner` are exported hourly",
        "the dashboard is blue",
    ]);
    let found = bank.recall("retries failed work", &recall_at(NOW), &NoCode, None);
    assert_eq!(ids(&found), vec![made[0].clone(), made[1].clone()]);
    assert_eq!(found[0].via, vec!["words"]);
    assert_eq!(found[1].via, vec!["links"]);
}

#[test]
fn the_time_strategy_returns_notes_in_the_window_of_the_query() {
    let (_dir, bank, made) = world(&["the deploy finished on 2026-03-05.", "the deploy finished on 2026-02-01.", "the deploy finished on 2026-03-06."]);
    let found = bank.recall("what finished on 2026-03-05", &recall_at(NOW), &NoCode, None);
    assert_eq!(found[0].note.id, made[0]);
    assert!(found[0].via.contains(&"time") && found[0].via.contains(&"words"));
    let other = found.iter().find(|r| r.note.id == made[1]).unwrap();
    assert!(!other.via.contains(&"time"));
    // The window of since and until works without a date in the query.
    let opts = RecallOpts { since: Some(midnight(2026, 3, 6).unwrap()), ..recall_at(NOW) };
    let only = bank.recall("deploy", &opts, &NoCode, None);
    assert_eq!(ids(&only), vec![made[2].clone()]);
    assert!(only[0].via.contains(&"time"));
}

#[test]
fn recency_and_proof_boosts_reorder_the_ranked_list_as_the_formulas_say() {
    let dir = TempDir::new("boosts");
    let mut bank = open(&dir);
    let mut made = Vec::new();
    for n in 0..12 {
        // All twelve match the query equally well, so the fused rank is the order retained.
        let when = if n == 0 { NOW - 300 * DAY } else { NOW };
        let opts = RetainOpts { when: Some(when), ..retain_at(NOW) };
        made.push(bank.retain(&format!("shared marker word{n}"), &opts, &NoCode, None).unwrap()[0].id.clone());
    }
    for _ in 0..9 {
        add(&mut bank, "shared marker word5", NOW);
    }
    let found = bank.recall("shared marker", &RecallOpts { limit: 12, max_tokens: 10_000, ..recall_at(NOW) }, &NoCode, None);
    assert_eq!(found.len(), 12);
    for (rank, item) in found.iter().enumerate() {
        let slot = made.iter().position(|id| *id == item.note.id).unwrap();
        // Without boosts the ranked list would be in retain order.
        let base = rank_score(slot, 12);
        let expected = base
            * boost(0.2, recency_signal(&item.note, NOW))
            * boost(0.2, 0.5)
            * boost(0.1, proof_signal(&item.note));
        assert!((item.score - expected).abs() < 1e-5, "rank {rank}: {} vs {expected}", item.score);
        assert_eq!(item.via, vec!["words"]);
    }
    let order: Vec<usize> = found.iter().map(|r| made.iter().position(|id| *id == r.note.id).unwrap()).collect();
    // The old note was first by rank; its recency boost of about 0.94 puts it second, behind the
    // best fresh note, whose boost is 1.1: 0.918 x 1.1 > 1.0 x 0.936 > 0.836 x 1.1.
    assert_eq!(&order[..3], &[1, 0, 2], "{order:?}");
    // The same scores as plain numbers, so that a wrong weight in the helpers cannot hide
    // inside the formulas above. The old note, rank 0 of 12: 1 x (1 + 0.2 x (0.1781 - 0.5)).
    // A fresh note of rank 1: (1 - 0.9 / 11) x 1.1. The note seen ten times, rank 5:
    // (1 - 4.5 / 11) x 1.1 x (1 + 0.1 x ln(10) / 10).
    let score_of = |slot: usize| found.iter().find(|r| r.note.id == made[slot]).unwrap().score;
    assert!((score_of(0) - 0.93562).abs() < 1e-3, "{}", score_of(0));
    assert!((score_of(1) - 1.01).abs() < 1e-3, "{}", score_of(1));
    assert!((score_of(5) - 0.66497).abs() < 1e-3, "{}", score_of(5));
    // Scores fall along the list.
    assert!(found.windows(2).all(|w| w[0].score >= w[1].score));
    // The note seen ten times has a proof boost above that of an unrepeated neighbour.
    let five = found.iter().find(|r| r.note.id == made[5]).unwrap();
    assert_eq!(five.note.proof_count, 10);
    assert!((proof_signal(&five.note) - (0.5 + 10f32.ln() / 10.0)).abs() < 1e-6);
}

#[test]
fn stale_and_gone_notes_are_flagged_and_sink() {
    let dir = TempDir::new("stale");
    let code = FakeCode::default();
    code.define("item_a", "src/a.rs", 1);
    let mut bank = open(&dir);
    // Eleven notes that match the query equally well; the first talks about a definition.
    for letter in 'a'..='k' {
        bank.retain(&format!("item_{letter} validates every setting"), &retain_at(NOW), &code, None).unwrap();
    }
    let query = "validates every setting";
    let ask = |code: &dyn CodeView| bank.recall(query, &RecallOpts { limit: 11, max_tokens: 10_000, ..recall_at(NOW) }, code, None);
    let position = |found: &[Recalled]| found.iter().position(|r| r.note.text.starts_with("item_a ")).unwrap();

    let fresh = ask(&code);
    assert!(fresh.iter().all(|r| !r.stale && !r.gone));
    assert_eq!(position(&fresh), 0);
    let base = fresh[0].score;

    // The definition changed: x 0.6 puts the note behind the four with a rank score above 0.6.
    code.define("item_a", "src/a.rs", 2);
    let changed = ask(&code);
    let item = &changed[position(&changed)];
    assert!(item.stale && !item.gone);
    assert!((item.score - base * 0.6).abs() < 1e-6);
    assert_eq!(position(&changed), 4);
    assert_eq!(changed.iter().filter(|r| r.stale || r.gone).count(), 1);

    // The definition is gone: x 0.4 puts it behind six.
    code.remove("item_a");
    let removed = ask(&code);
    let item = &removed[position(&removed)];
    assert!(item.gone && !item.stale);
    assert!((item.score - base * 0.4).abs() < 1e-6);
    assert_eq!(position(&removed), 6);

    // A view that knows no code judges nothing.
    let blind = ask(&NoCode);
    assert!(blind.iter().all(|r| !r.stale && !r.gone));
    assert_eq!(position(&blind), 0);
    assert!((blind[0].score - base).abs() < 1e-6);
}

#[test]
fn kind_tag_and_time_filters_apply() {
    let dir = TempDir::new("filters");
    let mut bank = open(&dir);
    let put = |bank: &mut Bank, text: &str, kind: Kind, tags: &[&str], when: i64| {
        let opts = RetainOpts { kind, tags: tags.iter().map(|t| t.to_string()).collect(), when: Some(when), ..retain_at(NOW) };
        bank.retain(text, &opts, &NoCode, None).unwrap()[0].id.clone()
    };
    let n1 = put(&mut bank, "alpha shared word", Kind::World, &["a"], NOW - 10 * DAY);
    let n2 = put(&mut bank, "beta shared word", Kind::Experience, &["b"], NOW - 5 * DAY);
    let n3 = put(&mut bank, "gamma shared word", Kind::World, &["a", "b"], NOW - DAY);
    let rule = put(&mut bank, "always check the rules", Kind::Directive, &["a"], NOW - DAY);
    let run = |opts: RecallOpts| ids(&bank.recall("shared word", &opts, &NoCode, None));
    let all = RecallOpts { limit: 10, ..recall_at(NOW) };

    assert_eq!(run(RecallOpts { kinds: vec![Kind::Experience], ..all.clone() }), vec![n2.clone()]);
    let mut world_only = run(RecallOpts { kinds: vec![Kind::World], ..all.clone() });
    world_only.sort();
    assert_eq!(world_only, vec![n1.clone(), n3.clone()]);
    let mut tagged_a = run(RecallOpts { tags: vec!["A".into()], ..all.clone() });
    tagged_a.sort();
    let mut expected = vec![n1.clone(), n3.clone(), rule.clone()];
    expected.sort();
    assert_eq!(tagged_a, expected, "any of the tags, compared without case");
    let mut both = run(RecallOpts { tags: vec!["a".into(), "b".into()], tags_all: true, ..all.clone() });
    both.sort();
    assert_eq!(both, vec![n3.clone()]);
    let recent = run(RecallOpts { since: Some(NOW - 6 * DAY), ..all.clone() });
    assert!(recent.contains(&n2) && recent.contains(&n3) && !recent.contains(&n1));
    let old = run(RecallOpts { until: Some(NOW - 6 * DAY), ..all.clone() });
    assert!(old.contains(&n1) && !old.contains(&n2) && !old.contains(&n3));
    let middle = run(RecallOpts { since: Some(NOW - 6 * DAY), until: Some(NOW - 4 * DAY), ..all });
    assert!(middle.contains(&n2) && !middle.contains(&n1) && !middle.contains(&n3));
}

#[test]
fn the_token_budget_skips_a_note_that_does_not_fit_and_the_limit_caps_the_list() {
    let dir = TempDir::new("budget");
    let mut bank = open(&dir);
    let long = format!("budget {}", "padding ".repeat(40));
    let long_id = add(&mut bank, &long, NOW)[0].id.clone();
    let short_a = add(&mut bank, "budget note a", NOW + 1)[0].id.clone();
    let short_b = add(&mut bank, "budget note b", NOW + 2)[0].id.clone();
    let short_c = add(&mut bank, "budget note c", NOW + 3)[0].id.clone();
    let big = RecallOpts { limit: 10, max_tokens: 10_000, ..recall_at(NOW) };
    assert_eq!(bank.recall("budget", &big, &NoCode, None).len(), 4);
    // 4 tokens per short note; the long one costs more than the whole budget.
    let tight = RecallOpts { max_tokens: 12, ..big.clone() };
    let found = ids(&bank.recall("budget", &tight, &NoCode, None));
    assert_eq!(found.len(), 3);
    assert!(!found.contains(&long_id));
    assert!(found.contains(&short_a) && found.contains(&short_b) && found.contains(&short_c));
    let used: usize = bank.recall("budget", &tight, &NoCode, None).iter().map(|r| lxr_core::est_tokens(&r.note.text)).sum();
    assert!(used <= 12);
    let limited = RecallOpts { limit: 2, ..big.clone() };
    assert_eq!(bank.recall("budget", &limited, &NoCode, None).len(), 2);
    assert!(bank.recall("budget", &RecallOpts { limit: 0, ..big }, &NoCode, None).is_empty());
}

#[test]
fn directives_that_pass_the_filters_come_first() {
    let dir = TempDir::new("directives");
    let mut bank = open(&dir);
    add(&mut bank, "the cache layer stores parsed files", NOW);
    let opts = RetainOpts { kind: Kind::Directive, tags: vec!["rules".into()], ..retain_at(NOW + 1) };
    let rule = bank.retain("never commit generated files", &opts, &NoCode, None).unwrap()[0].id.clone();
    let found = bank.recall("cache layer", &recall_at(NOW + 2), &NoCode, None);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].note.id, rule, "the directive is first although the query does not match it");
    assert_eq!(found[1].note.text, "the cache layer stores parsed files");
    // A filter the directive fails removes it.
    let tagged = bank.recall("cache layer", &RecallOpts { tags: vec!["other".into()], ..recall_at(NOW + 2) }, &NoCode, None);
    assert!(tagged.is_empty());
    let kinds = bank.recall("cache layer", &RecallOpts { kinds: vec![Kind::World], ..recall_at(NOW + 2) }, &NoCode, None);
    assert_eq!(texts(&kinds), vec!["the cache layer stores parsed files"]);
}

#[test]
fn reflect_sorts_the_recall_and_lists_the_shared_entities() {
    let dir = TempDir::new("reflect");
    let mut bank = open(&dir);
    for n in 0..3 {
        add(&mut bank, &format!("parse_config handles case {n}."), NOW + n);
    }
    add(&mut bank, "load_all calls parse_config first.", NOW + 10);
    let opts = RetainOpts { kind: Kind::Directive, ..retain_at(NOW + 11) };
    bank.retain("always keep parse_config pure", &opts, &NoCode, None).unwrap();
    let r = bank.reflect("parse_config", &RecallOpts { limit: 20, ..recall_at(NOW + 20) }, &NoCode, None);
    assert_eq!(r.directives.len(), 1);
    assert_eq!(r.observations.len(), 1);
    assert_eq!(r.facts.len(), 4);
    assert_eq!(r.by_entity[0].0, "parse_config");
    assert_eq!(r.by_entity[0].1.len(), 6, "all six returned notes mention it");
    assert_eq!(r.by_entity[1].0, "load_all");
    assert_eq!(r.by_entity[1].1.len(), 1);
}

// ---- forget ----

#[test]
fn a_forgotten_note_is_gone_from_every_answer_and_its_id_is_never_reused() {
    let dir = TempDir::new("forget");
    let mut bank = open(&dir);
    let a = add(&mut bank, "the scheduler retries failed jobs", NOW)[0].id.clone();
    let b = add(&mut bank, "the scheduler logs every retry", NOW + 1)[0].id.clone();
    assert!(bank.forget(&a).unwrap());
    assert!(!bank.forget(&a).unwrap());
    assert!(!bank.forget("n999999").unwrap());
    assert!(bank.get(&a).is_none());
    assert_eq!(bank.len(), 1);
    assert_eq!(ids(&bank.recall("scheduler", &recall_at(NOW), &NoCode, None)), vec![b.clone()]);
    let c = add(&mut bank, "the scheduler is single threaded", NOW + 2)[0].id.clone();
    assert!(c > b, "{c} after {b}");
    let log = fs::read_to_string(dir.bank("b").join("notes.jsonl")).unwrap();
    assert!(log.contains("\"forgotten\":\"n000001\""), "{log}");
    drop(bank);
    let bank = open(&dir);
    assert!(bank.get(&a).is_none());
    assert_eq!(bank.len(), 2);
}

// ---- files ----

#[test]
fn the_log_survives_a_damaged_last_line() {
    let dir = TempDir::new("damaged");
    let mut bank = open(&dir);
    add(&mut bank, "the first fact is safe", NOW);
    add(&mut bank, "the second fact is safe", NOW + 1);
    drop(bank);
    let log = dir.bank("b").join("notes.jsonl");
    let mut text = fs::read_to_string(&log).unwrap();
    text.push_str("{\"id\":\"n000099\",\"kind\":\"Wor");
    fs::write(&log, text).unwrap();
    let mut bank = open(&dir);
    assert_eq!(bank.len(), 2);
    let third = add(&mut bank, "the third fact follows the damage", NOW + 2)[0].id.clone();
    drop(bank);
    let bank = open(&dir);
    assert_eq!(bank.len(), 3);
    assert_eq!(bank.get(&third).unwrap().text, "the third fact follows the damage");
    assert!(third > "n000002".to_string());
}

#[test]
fn damaged_lines_in_the_middle_are_skipped_too() {
    let dir = TempDir::new("damaged-middle");
    let mut bank = open(&dir);
    add(&mut bank, "the first fact is safe", NOW);
    drop(bank);
    let log = dir.bank("b").join("notes.jsonl");
    let mut text = fs::read_to_string(&log).unwrap();
    text.push_str("this is not json\n\n{\"a\":1}\n");
    fs::write(&log, text).unwrap();
    let mut bank = open(&dir);
    add(&mut bank, "the second fact is safe", NOW + 1);
    drop(bank);
    assert_eq!(open(&dir).len(), 2);
}

#[test]
fn a_bank_with_current_caches_opens_without_rebuilding() {
    let dir = TempDir::new("current");
    let mut bank = open(&dir);
    for n in 0..20 {
        add(&mut bank, &format!("the module `mod_{}` handles case {n}.", n % 3), NOW + n);
    }
    let before = bank.stats();
    drop(bank);
    let b = dir.bank("b");
    assert!(b.join("index.bin").exists());
    let bank = open(&dir);
    assert!(!bank.dirty, "the caches were current");
    assert_eq!(bank.stats().notes, before.notes);
    assert_eq!(bank.stats().links, before.links);
    assert_eq!(bank.stats().entities, before.entities);
    assert_eq!(bank.stats().by_kind, before.by_kind);
    assert!(bank.stats().bytes > 0);
}

#[test]
fn caches_are_rebuilt_when_missing_damaged_or_older_than_the_log() {
    let dir = TempDir::new("rebuild");
    let b = dir.bank("b");
    let mut bank = open(&dir);
    for n in 0..6 {
        add(&mut bank, &format!("the module `mod_{}` handles case {n}.", n % 2), NOW + n);
    }
    let want = ids(&bank.recall("module handles", &RecallOpts { limit: 20, ..recall_at(NOW + 100) }, &NoCode, None));
    let stats = bank.stats();
    drop(bank);

    // Deleted.
    fs::remove_file(b.join("index.bin")).unwrap();
    let bank = open(&dir);
    assert!(bank.dirty);
    assert_eq!(ids(&bank.recall("module handles", &RecallOpts { limit: 20, ..recall_at(NOW + 100) }, &NoCode, None)), want);
    assert_eq!((bank.stats().notes, bank.stats().links, bank.stats().entities), (stats.notes, stats.links, stats.entities));
    drop(bank);
    assert!(b.join("index.bin").exists(), "the drop wrote the cache again");

    // Damaged.
    fs::write(b.join("index.bin"), b"not a snapshot").unwrap();
    let bank = open(&dir);
    assert!(bank.dirty);
    assert_eq!(bank.len(), stats.notes);
    drop(bank);
    assert!(!open(&dir).dirty);

    // A line appended behind the bank's back.
    let log = b.join("notes.jsonl");
    let mut text = fs::read_to_string(&log).unwrap();
    text.push_str("{\"id\":\"n000050\",\"kind\":\"World\",\"text\":\"added by hand\"}\n");
    fs::write(&log, text).unwrap();
    let bank = open(&dir);
    assert!(bank.dirty);
    assert_eq!(bank.len(), stats.notes + 1);
    assert_eq!(bank.get("n000050").unwrap().text, "added by hand");
    drop(bank);

    // The log touched without a change in length: the snapshot is older, so it is not trusted.
    let before = fs::metadata(b.join("index.bin")).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(60));
    let text = fs::read(&log).unwrap();
    fs::write(&log, text).unwrap();
    let bank = open(&dir);
    assert!(bank.dirty);
    drop(bank);
    assert!(fs::metadata(b.join("index.bin")).unwrap().modified().unwrap() > before);
    assert!(!open(&dir).dirty);
    // New ids continue after the highest one in the log.
    let mut bank = open(&dir);
    assert!(add(&mut bank, "one more fact for the log", NOW + 500)[0].id.as_str() > "n000050");
}

#[test]
fn two_banks_in_two_folders_do_not_see_each_other() {
    let dir = TempDir::new("isolated");
    let mut one = Bank::open(&dir.bank("one")).unwrap();
    let mut two = Bank::open(&dir.bank("two")).unwrap();
    add(&mut one, "the scheduler retries failed jobs", NOW);
    add(&mut two, "the renderer paints widgets", NOW);
    assert_eq!(one.len(), 1);
    assert_eq!(two.len(), 1);
    assert!(one.recall("renderer", &recall_at(NOW), &NoCode, None).is_empty());
    assert!(two.recall("scheduler", &recall_at(NOW), &NoCode, None).is_empty());
    assert_eq!(one.stats().entities + two.stats().entities, 0);
    drop(one);
    drop(two);
    assert_eq!(Bank::open(&dir.bank("one")).unwrap().notes()[0].text, "the scheduler retries failed jobs");
}

#[test]
fn an_empty_or_new_folder_gives_an_empty_bank() {
    let dir = TempDir::new("empty");
    let bank = Bank::open(&dir.bank("deep").join("er")).unwrap();
    assert!(bank.is_empty());
    assert_eq!(bank.stats(), BankStats::default());
    assert!(bank.recall("anything", &recall_at(NOW), &NoCode, None).is_empty());
    assert_eq!(bank.reflect("anything", &recall_at(NOW), &NoCode, None), Reflection::default());
    assert!(bank.notes().is_empty());
}

// ---- with the embedding model ----

#[test]
fn meaning_finds_a_note_that_shares_no_word_with_the_query() {
    let Some(e) = embedder() else { return };
    let dir = TempDir::new("meaning");
    let mut bank = open(&dir);
    let note = bank.retain("database connection pool exhausted under heavy load", &retain_at(NOW), &NoCode, Some(e)).unwrap()[0].id.clone();
    bank.retain("the sidebar menu is drawn in dark blue", &retain_at(NOW + 1), &NoCode, Some(e)).unwrap();
    let found = bank.recall("db connections ran out when busy", &recall_at(NOW + 2), &NoCode, Some(e));
    assert_eq!(found[0].note.id, note);
    assert!(found[0].via.contains(&"meaning") && !found[0].via.contains(&"words"), "{:?}", found[0].via);
}

#[test]
fn vectors_are_made_the_first_time_an_embedder_is_passed() {
    let Some(e) = embedder() else { return };
    let dir = TempDir::new("lazy-vectors");
    let mut bank = open(&dir);
    let note = add(&mut bank, "database connection pool exhausted under heavy load", NOW)[0].id.clone();
    add(&mut bank, "the sidebar menu is drawn in dark blue", NOW + 1);
    assert!(!lock(&bank.derived).vecs.any());
    let found = bank.recall("db connections ran out when busy", &recall_at(NOW + 2), &NoCode, Some(e));
    assert_eq!(found[0].note.id, note);
    assert!(found[0].via.contains(&"meaning"));
    assert!(lock(&bank.derived).vecs.any());
    drop(bank);
    // The vectors were written, and a reopened bank uses them without an embedder for the notes.
    let bank = open(&dir);
    assert!(!bank.dirty);
    assert!(lock(&bank.derived).vecs.have.iter().all(|&h| h));
    assert_eq!(bank.recall("db connections ran out when busy", &recall_at(NOW + 2), &NoCode, Some(e))[0].note.id, note);
}

#[test]
fn a_near_duplicate_reinforces_only_when_an_entity_is_shared() {
    let Some(e) = embedder() else { return };
    let dir = TempDir::new("near-duplicate");
    let mut bank = open(&dir);
    let put = |bank: &mut Bank, text: &str, entity: &str, now: i64| {
        let opts = RetainOpts { entities: vec![entity.to_string()], ..retain_at(now) };
        bank.retain(text, &opts, &NoCode, Some(e)).unwrap()
    };
    // A long sentence, so that the name in brackets moves the embedding only a little:
    // cosine about 0.96 between the first two notes below, 0.99 between the first and the third.
    let long = concat!(
        "the scheduler retries every failed job three times before it gives up and reports the failure ",
        "to the operator on call and then it writes a summary of every attempt into the shared log file ",
        "so that nobody has to ask what happened"
    );
    let first = put(&mut bank, long, "alpha", NOW);
    assert_eq!(first[0].outcome, RetainOutcome::Added);
    let other_entity = put(&mut bank, &format!("{long} anyway"), "beta", NOW + 5);
    assert_eq!(other_entity[0].outcome, RetainOutcome::Added, "nearly the same words, but no entity in common");
    let same_entity = put(&mut bank, &format!("{long} again"), "alpha", NOW + 10);
    assert_eq!(same_entity[0].outcome, RetainOutcome::Reinforced);
    assert_eq!(same_entity[0].id, first[0].id);
    let note = bank.get(&first[0].id).unwrap();
    assert_eq!(note.proof_count, 2);
    assert!(note.history[0].contains("near duplicate, cosine 0.9"), "{:?}", note.history);
    // A different sentence about the same entity stays a separate note.
    let different = put(&mut bank, "the scheduler writes one log line for every rejected job", "alpha", NOW + 15);
    assert_eq!(different[0].outcome, RetainOutcome::Added);
    assert_eq!(bank.len(), 3);
}

#[test]
fn similar_notes_get_semantic_links_and_unrelated_ones_do_not() {
    let Some(e) = embedder() else { return };
    let dir = TempDir::new("semantic");
    let mut bank = open(&dir);
    let far = 10 * DAY;
    let put = |bank: &mut Bank, text: &str, now: i64| bank.retain(text, &retain_at(now), &NoCode, Some(e)).unwrap();
    // Cosine 0.77 between the first two (above the link threshold, below the duplicate one), 0.1 to the third.
    put(&mut bank, "`parse_config` reads the configuration file when the program starts.", NOW);
    put(&mut bank, "`parse_config` writes a log line for every rejected setting.", NOW + far);
    put(&mut bank, "the sidebar menu is drawn in dark blue", NOW + 2 * far);
    assert_eq!(bank.len(), 3);
    let derived = lock(&bank.derived);
    let semantic = |slot: u32| derived.graph.neighbors(slot).filter(|x| x.kind == crate::graph::LinkKind::Semantic).count();
    assert_eq!((semantic(0), semantic(1), semantic(2)), (1, 1, 0));
    let weight = derived.graph.neighbors(0).find(|x| x.kind == crate::graph::LinkKind::Semantic).unwrap().w;
    assert!(weight >= LINK_THRESHOLD && weight < NEAR_DUPLICATE, "{weight}");
}

#[test]
fn semantic_links_are_made_late_for_notes_retained_without_an_embedder() {
    let Some(e) = embedder() else { return };
    let dir = TempDir::new("semantic-late");
    let mut bank = open(&dir);
    add(&mut bank, "`parse_config` reads the configuration file when the program starts.", NOW);
    add(&mut bank, "`parse_config` writes a log line for every rejected setting.", NOW + 10 * DAY);
    assert_eq!(bank.stats().links, 0);
    bank.recall("settings", &recall_at(NOW + 11 * DAY), &NoCode, Some(e));
    assert_eq!(bank.stats().links, 1, "one semantic link, made when the vectors were");
}

#[test]
fn missing_vectors_are_made_again_and_written_back() {
    let Some(e) = embedder() else { return };
    let dir = TempDir::new("vectors-missing");
    let vectors = dir.bank("b").join("vectors.bin");
    let mut bank = open(&dir);
    let note = bank.retain("database connection pool exhausted under heavy load", &retain_at(NOW), &NoCode, Some(e)).unwrap()[0].id.clone();
    bank.retain("the sidebar menu is drawn in dark blue", &retain_at(NOW + 1), &NoCode, Some(e)).unwrap();
    drop(bank);
    assert!(vectors.exists());
    fs::remove_file(&vectors).unwrap();
    let bank = open(&dir);
    assert!(!bank.dirty, "the index itself is current");
    assert!(!lock(&bank.derived).vecs.any());
    let found = bank.recall("db connections ran out when busy", &recall_at(NOW + 2), &NoCode, Some(e));
    assert_eq!(found[0].note.id, note);
    drop(bank);
    assert!(vectors.exists(), "written again by the drop");
    let bank = open(&dir);
    assert!(lock(&bank.derived).vecs.have.iter().all(|&h| h));
    assert!(!bank.dirty);
}

#[test]
fn recall_with_runs_only_the_strategies_it_is_given() {
    let Some(e) = embedder() else { return };
    let dir = TempDir::new("only");
    let mut bank = open(&dir);
    bank.retain("database connection pool exhausted under heavy load", &retain_at(NOW), &NoCode, Some(e)).unwrap();
    bank.retain("the sidebar menu is drawn in dark blue", &retain_at(NOW + 1), &NoCode, Some(e)).unwrap();
    let query = "database pool exhausted";
    let opts = recall_at(NOW + 2);
    let all = bank.recall(query, &opts, &NoCode, Some(e));
    assert_eq!(all[0].via, vec!["meaning", "words"]);
    let only = |name: &str| bank.recall_with(query, &opts, &NoCode, Some(e), Strategies::only(name).unwrap());
    let words = only("words");
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].via, vec!["words"]);
    let meaning = only("meaning");
    assert_eq!(meaning[0].via, vec!["meaning"]);
    assert_eq!(meaning[0].note.id, all[0].note.id);
    assert!(only("links").is_empty() && only("time").is_empty());
    assert!(Strategies::only("nope").is_none());
    assert_eq!(Strategies::default(), Strategies::ALL);
}

#[test]
fn damaged_vectors_are_made_again() {
    let Some(e) = embedder() else { return };
    let dir = TempDir::new("vectors-damaged");
    let vectors = dir.bank("b").join("vectors.bin");
    let mut bank = open(&dir);
    let note = bank.retain("database connection pool exhausted under heavy load", &retain_at(NOW), &NoCode, Some(e)).unwrap()[0].id.clone();
    bank.retain("the sidebar menu is drawn in dark blue", &retain_at(NOW + 1), &NoCode, Some(e)).unwrap();
    drop(bank);
    let good = fs::read(&vectors).unwrap();
    for damage in [good[..good.len() - 7].to_vec(), b"not a vector file".to_vec(), Vec::new()] {
        fs::write(&vectors, &damage).unwrap();
        let bank = open(&dir);
        assert!(!bank.dirty, "the index is current, only the vectors are not");
        let found = bank.recall("db connections ran out when busy", &recall_at(NOW + 2), &NoCode, Some(e));
        assert_eq!(found[0].note.id, note);
        drop(bank);
        assert_eq!(fs::read(&vectors).unwrap(), good, "written again, the same as before");
    }
}

// ---- banks that share a folder ----

#[test]
fn two_banks_on_one_folder_never_hand_out_one_id() {
    let dir = TempDir::new("shared");
    let mut one = open(&dir);
    let mut two = open(&dir);
    let a = add(&mut one, "the scheduler retries failed jobs", NOW)[0].id.clone();
    let b = add(&mut two, "the renderer paints widgets", NOW + 1)[0].id.clone();
    assert_ne!(a, b, "two banks must not hand out one id");
    let c = add(&mut one, "the loader reads the config file", NOW + 2)[0].id.clone();
    assert_eq!(one.len(), 3, "the first bank saw the note of the second before it wrote");
    assert_eq!(two.len(), 2);
    drop(one);
    drop(two);
    let bank = open(&dir);
    assert_eq!(bank.len(), 3);
    for id in [&a, &b, &c] {
        assert!(bank.get(id).is_some(), "{id} was lost");
    }
}

#[test]
fn banks_on_several_threads_share_one_folder_without_losing_a_note() {
    let dir = TempDir::new("shared-threads");
    let path = dir.bank("b");
    let workers: Vec<_> = (0..3i64)
        .map(|t| {
            let path = path.clone();
            std::thread::spawn(move || {
                let mut bank = Bank::open(&path).expect("open the bank");
                (0..10i64)
                    .map(|n| {
                        let text = format!("worker {t} wrote fact {n}");
                        bank.retain(&text, &retain_at(NOW + t * 100 + n), &NoCode, None).expect("retain")[0].id.clone()
                    })
                    .collect::<Vec<String>>()
            })
        })
        .collect();
    let mut ids: Vec<String> = workers.into_iter().flat_map(|w| w.join().expect("a worker panicked")).collect();
    assert_eq!(ids.len(), 30);
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 30, "every note got an id of its own");
    assert_eq!(open(&dir).len(), 30);
}

#[test]
fn a_stale_bank_cannot_bring_back_a_note_another_bank_forgot() {
    let dir = TempDir::new("resurrect");
    let mut stale = open(&dir);
    let id = add(&mut stale, "the scheduler retries failed jobs", NOW)[0].id.clone();
    let mut other = open(&dir);
    assert!(other.forget(&id).unwrap());
    // The first bank still holds the note in memory. Retaining the same text must not reinforce
    // a note that is gone: that would append its full state after the tombstone.
    let again = add(&mut stale, "the scheduler retries failed jobs", NOW + 1);
    assert_eq!(again[0].outcome, RetainOutcome::Added);
    assert_ne!(again[0].id, id);
    drop(stale);
    drop(other);
    let bank = open(&dir);
    assert!(bank.get(&id).is_none(), "the forgotten note came back");
    assert_eq!(bank.len(), 1);
}

#[test]
fn a_bank_that_is_behind_the_log_leaves_the_caches_alone() {
    let dir = TempDir::new("behind");
    let mut behind = open(&dir);
    add(&mut behind, "the scheduler retries failed jobs", NOW);
    let mut ahead = open(&dir);
    add(&mut ahead, "the renderer paints widgets", NOW + 1);
    drop(ahead);
    // The first bank has not seen the second note. Its caches would be older than the log,
    // and would replace the ones the second bank just wrote.
    drop(behind);
    let bank = open(&dir);
    assert!(!bank.dirty, "the caches of the bank that was ahead were replaced");
    assert_eq!(bank.len(), 2);
}

// ---- a write that fails ----

#[test]
fn a_failed_append_leaves_no_trace_in_the_memory_and_the_next_write_works() {
    let dir = TempDir::new("append-fails");
    let mut bank = open(&dir);
    let log = dir.bank("b").join("notes.jsonl");
    fs::write(&log, b"").unwrap();
    let mut permissions = fs::metadata(&log).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&log, permissions.clone()).unwrap();
    if fs::OpenOptions::new().append(true).open(&log).is_ok() {
        // Read-only files are writable here (a superuser): the failure cannot be made.
        eprintln!("skipped: this account can write to a read-only file");
        permissions.set_readonly(false);
        fs::set_permissions(&log, permissions).unwrap();
        return;
    }
    let failed = bank.retain("the first fact never reaches the disk", &retain_at(NOW), &NoCode, None);
    assert!(failed.is_err());
    assert_eq!(bank.len(), 0, "a note that was not written must not stay in memory");
    permissions.set_readonly(false);
    fs::set_permissions(&log, permissions).unwrap();
    let kept = add(&mut bank, "the second fact is written", NOW + 1);
    assert_eq!(kept[0].id, "n000001", "the id of the note that failed was not used up");
    drop(bank);
    let bank = open(&dir);
    assert_eq!(bank.len(), 1);
    assert_eq!(bank.notes()[0].text, "the second fact is written");
}

// ---- what counts as the same fact ----

#[test]
fn texts_that_differ_in_a_closing_symbol_are_different_facts() {
    let pairs = [
        ("Requires Python 3.9", "Requires Python 3.9+"),
        ("We use C", "We use C++"),
        ("Target is C", "Target is C#"),
        ("Limit is 5", "Limit is 5%"),
        ("Call the loader", "Call the loader (twice)"),
    ];
    for (plain, marked) in pairs {
        let dir = TempDir::new("closing-symbol");
        let mut bank = open(&dir);
        let first = add(&mut bank, plain, NOW);
        let second = add(&mut bank, marked, NOW + 1);
        assert_eq!(second[0].outcome, RetainOutcome::Added, "{plain:?} and {marked:?} are not one fact");
        assert_ne!(first[0].id, second[0].id);
        assert_eq!(bank.len(), 2);
        assert_eq!(bank.get(&second[0].id).unwrap().text, marked);
    }
    // Sentence punctuation and case still make no difference.
    let dir = TempDir::new("closing-dot");
    let mut bank = open(&dir);
    let first = add(&mut bank, "Requires Python 3.9", NOW);
    let again = add(&mut bank, "requires python 3.9.", NOW + 1);
    assert_eq!(again[0].outcome, RetainOutcome::Reinforced);
    assert_eq!(again[0].id, first[0].id);
}

// ---- ids from a log that was edited ----

#[test]
fn a_log_line_with_an_enormous_id_does_not_move_the_counter() {
    let dir = TempDir::new("huge-id");
    let mut bank = open(&dir);
    let first = add(&mut bank, "the first fact is safe", NOW)[0].id.clone();
    drop(bank);
    let log = dir.bank("b").join("notes.jsonl");
    let mut text = fs::read_to_string(&log).unwrap();
    text.push_str("{\"forgotten\":\"n18446744073709551615\"}\n");
    fs::write(&log, text).unwrap();
    let mut bank = open(&dir);
    let second = add(&mut bank, "the second fact is safe", NOW + 1)[0].id.clone();
    let third = add(&mut bank, "the third fact is safe", NOW + 2)[0].id.clone();
    assert!(second != first && third != first && second != third, "{first} {second} {third}");
    drop(bank);
    let bank = open(&dir);
    assert_eq!(bank.len(), 3);
    assert_eq!(bank.get(&first).unwrap().text, "the first fact is safe");
}

// ---- observations of a large group ----

#[test]
fn an_observation_lists_only_its_newest_sources_and_the_log_grows_by_a_line() {
    let dir = TempDir::new("obs-growth");
    let mut bank = open(&dir);
    let log = dir.bank("b").join("notes.jsonl");
    let mut grew: Vec<u64> = Vec::new();
    let mut last = String::new();
    for n in 0..600 {
        let before = fs::metadata(&log).map_or(0, |m| m.len());
        last = add(&mut bank, &format!("`hub_type` handles case {n}."), NOW + n)[0].id.clone();
        grew.push(fs::metadata(&log).unwrap().len() - before);
    }
    let obs = observations(&bank);
    assert_eq!(obs.len(), 1);
    assert_eq!(obs[0].proof_count, 600, "the count is whole although the list is not");
    assert_eq!(obs[0].sources.len(), 50);
    assert_eq!(obs[0].sources.last(), Some(&last));
    // One retain appends the same number of bytes whether the group has 100 or 600 notes.
    assert!(grew[599] < grew[99] * 3 / 2, "{} bytes at 600 notes, {} bytes at 100", grew[599], grew[99]);
}

// ---- directives and the limit ----

#[test]
fn directives_leave_room_for_the_notes_that_match_the_query() {
    let dir = TempDir::new("directives-room");
    let mut bank = open(&dir);
    let validates = add(&mut bank, "parse_config validates the keys.", NOW)[0].id.clone();
    let rejects = add(&mut bank, "parse_config rejects unknown sections.", NOW + 1)[0].id.clone();
    let rules = RetainOpts { kind: Kind::Directive, ..retain_at(NOW + 10) };
    for n in 0..8 {
        bank.retain(&format!("never commit generated file number {n}"), &rules, &NoCode, None).unwrap();
    }
    let found = bank.recall("how does parse_config work", &recall_at(NOW + 20), &NoCode, None);
    assert_eq!(found.len(), 8, "the limit still holds");
    let ranked: Vec<&Recalled> = found.iter().filter(|r| r.note.kind != Kind::Directive).collect();
    assert_eq!(ranked.len(), 2, "{:?}", texts(&found));
    assert!(ranked.iter().any(|r| r.note.id == validates) && ranked.iter().any(|r| r.note.id == rejects));
    assert!(found[..6].iter().all(|r| r.note.kind == Kind::Directive), "directives still come first");
    let r = bank.reflect("how does parse_config work", &recall_at(NOW + 20), &NoCode, None);
    assert_eq!(r.facts.len(), 2, "reflect found no fact");
}

#[test]
fn directives_use_at_most_half_of_the_token_allowance() {
    let dir = TempDir::new("directives-tokens");
    let mut bank = open(&dir);
    add(&mut bank, "parse_config validates the keys.", NOW);
    add(&mut bank, "parse_config rejects unknown sections.", NOW + 1);
    let rules = RetainOpts { kind: Kind::Directive, ..retain_at(NOW + 10) };
    for n in 0..6 {
        let text = format!("never commit generated file number {n} {}", "and keep it out of the repository ".repeat(4));
        bank.retain(&text, &rules, &NoCode, None).unwrap();
    }
    // A directive costs 44 tokens and a note 8: 100 tokens hold one directive in its half
    // and both notes, but not a second directive.
    let found = bank.recall("how does parse_config work", &RecallOpts { max_tokens: 100, ..recall_at(NOW + 20) }, &NoCode, None);
    let ranked = found.iter().filter(|r| r.note.kind != Kind::Directive).count();
    assert_eq!(ranked, 2, "{:?}", texts(&found));
    assert_eq!(found.len() - ranked, 1);
    let used: usize = found.iter().map(|r| lxr_core::est_tokens(&r.note.text)).sum();
    assert!(used <= 100);
}

// ---- entities and anchors ----

#[test]
fn definitions_with_alike_names_keep_their_own_entities_and_anchors() {
    let dir = TempDir::new("alike-names");
    let code = FakeCode::default();
    code.define("get_user", "src/users.rs", 1);
    code.define("get_users", "src/users.rs", 2);
    let mut bank = open(&dir);
    bank.retain("`get_user` fetches the row for `Session`.", &retain_at(NOW), &code, None).unwrap();
    let second = bank.retain("`get_users` pages through `Session` too.", &retain_at(NOW + 60), &code, None).unwrap();
    let note = bank.get(&second[0].id).unwrap().clone();
    assert!(note.entities.contains(&"get_users".to_string()), "{:?}", note.entities);
    assert!(!note.entities.contains(&"get_user".to_string()), "{:?}", note.entities);
    assert_eq!(note.anchors.len(), 1, "{:?}", note.anchors);
    assert_eq!((note.anchors[0].qual.as_str(), note.anchors[0].hash), ("get_users", 2));
    // An edit to get_user leaves the note about get_users alone.
    code.define("get_user", "src/users.rs", 3);
    let found = bank.recall("pages through Session", &recall_at(NOW + 120), &code, None);
    let item = found.iter().find(|r| r.note.id == second[0].id).expect("the note is recalled");
    assert!(!item.stale);
}

#[test]
fn a_misspelling_of_a_name_the_code_does_not_know_still_merges_and_keeps_the_known_anchor() {
    let dir = TempDir::new("misspelt");
    let code = FakeCode::default();
    code.define("get_user", "src/users.rs", 1);
    let mut bank = open(&dir);
    bank.retain("`get_user` fetches the row for `Session`.", &retain_at(NOW), &code, None).unwrap();
    let second = bank.retain("`get_usr` pages through `Session` too.", &retain_at(NOW + 60), &code, None).unwrap();
    let note = bank.get(&second[0].id).unwrap();
    assert!(note.entities.contains(&"get_user".to_string()), "{:?}", note.entities);
    assert_eq!(note.anchors.len(), 1);
    assert_eq!(note.anchors[0].qual, "get_user");
}

#[test]
fn a_repeat_checked_against_the_code_clears_the_stale_mark() {
    let dir = TempDir::new("repeat-anchors");
    let code = FakeCode::default();
    code.define("parse_config", "src/config.rs", 1);
    let mut bank = open(&dir);
    let sentence = "parse_config validates every setting";
    let first = bank.retain(sentence, &retain_at(NOW), &code, None).unwrap();
    assert_eq!(bank.get(&first[0].id).unwrap().anchors[0].hash, 1);
    code.define("parse_config", "src/config.rs", 2);
    let ask = |bank: &Bank| bank.recall("validates every setting", &recall_at(NOW + 2), &code, None);
    assert!(ask(&bank)[0].stale, "the definition changed");
    let again = bank.retain(sentence, &retain_at(NOW + 1), &code, None).unwrap();
    assert_eq!(again[0].outcome, RetainOutcome::Reinforced);
    assert_eq!(again[0].id, first[0].id);
    let found = ask(&bank);
    assert!(!found[0].stale, "the repeat was retained against the current code");
    assert_eq!(found[0].note.anchors[0].hash, 2);
    assert_eq!(found[0].note.proof_count, 2);
    // A definition that is gone stays gone, whatever is retained.
    code.remove("parse_config");
    bank.retain(sentence, &retain_at(NOW + 3), &code, None).unwrap();
    assert!(bank.recall("validates every setting", &recall_at(NOW + 4), &code, None)[0].gone);
    // The new hash is in the log, not only in memory.
    code.define("parse_config", "src/config.rs", 2);
    drop(bank);
    let bank = open(&dir);
    assert!(!bank.recall("validates every setting", &recall_at(NOW + 5), &code, None)[0].stale);
}

#[test]
fn a_near_repeat_checked_against_the_code_clears_the_stale_mark() {
    let Some(e) = embedder() else { return };
    let dir = TempDir::new("near-repeat-anchors");
    let code = FakeCode::default();
    code.define("alpha", "src/alpha.rs", 1);
    let mut bank = open(&dir);
    let put = |bank: &mut Bank, text: &str, now: i64| {
        let opts = RetainOpts { entities: vec!["alpha".to_string()], ..retain_at(now) };
        bank.retain(text, &opts, &code, Some(e)).unwrap()
    };
    let long = concat!(
        "the scheduler retries every failed job three times before it gives up and reports the failure ",
        "to the operator on call and then it writes a summary of every attempt into the shared log file ",
        "so that nobody has to ask what happened"
    );
    let first = put(&mut bank, long, NOW);
    assert_eq!(first[0].outcome, RetainOutcome::Added);
    code.define("alpha", "src/alpha.rs", 2);
    let again = put(&mut bank, &format!("{long} again"), NOW + 10);
    assert_eq!(again[0].outcome, RetainOutcome::Reinforced);
    assert_eq!(again[0].id, first[0].id);
    let note = bank.get(&first[0].id).unwrap();
    assert_eq!(note.anchors[0].hash, 2);
}

// ---- observations of entities that are about the same notes ----

const SHARED_FACTS: [&str; 3] = [
    "config_loader.py defines load_config for reading settings.",
    "config_loader.py exposes load_config to the command line.",
    "load_config in config_loader.py falls back to defaults.",
];

/// A bank with the three notes above, which all name both `config_loader.py` and `load_config`.
fn shared_facts(dir: &TempDir) -> (Bank, Vec<String>) {
    let mut bank = open(dir);
    let made = SHARED_FACTS.iter().enumerate().map(|(n, text)| add(&mut bank, text, NOW + n as i64)[0].id.clone()).collect();
    (bank, made)
}

fn log_lines(dir: &TempDir) -> Vec<String> {
    fs::read_to_string(dir.bank("b").join("notes.jsonl")).unwrap().lines().map(str::to_string).collect()
}

#[test]
fn notes_with_identical_sources_become_one_observation_that_lists_its_subjects() {
    let dir = TempDir::new("obs-shared");
    let (bank, made) = shared_facts(&dir);
    let obs = observations(&bank);
    assert_eq!(obs.len(), 1, "one observation for both subjects, not one copy each: {obs:#?}");
    let o = &obs[0];
    assert_eq!(o.entities, vec!["config_loader.py", "load_config"]);
    assert_eq!(o.sources, made);
    assert_eq!(o.proof_count, 3);
    assert!(o.text.starts_with("config_loader.py, load_config: config_loader.py defines load_config"), "{}", o.text);
    assert_eq!(bank.len(), 4);
    assert_eq!(bank.stats().by_kind, [3, 0, 1, 0]);
    assert_eq!(log_lines(&dir).iter().filter(|line| line.contains("\"Observation\"")).count(), 1, "one observation line in the log");
    // Both subjects lead to it.
    let reflected = bank.reflect("config_loader", &RecallOpts { limit: 20, ..recall_at(NOW + 10) }, &NoCode, None);
    assert_eq!(reflected.observations.len(), 1);
    for subject in ["config_loader.py", "load_config"] {
        let (_, listed) = reflected.by_entity.iter().find(|(name, _)| name == subject).unwrap_or_else(|| panic!("{subject} is not a subject"));
        assert!(listed.contains(&o.id), "{subject} does not list the observation");
    }
}

#[test]
fn a_shared_observation_survives_a_reopen_and_is_refreshed_in_place() {
    let dir = TempDir::new("obs-reopen");
    let (bank, _) = shared_facts(&dir);
    let id = observations(&bank)[0].id.clone();
    drop(bank);
    let mut bank = open(&dir);
    let fourth = add(&mut bank, "config_loader.py hands load_config the path.", NOW + 10)[0].id.clone();
    let obs = observations(&bank);
    assert_eq!(obs.len(), 1);
    assert_eq!(obs[0].id, id, "the same observation, refreshed");
    assert_eq!(obs[0].sources.len(), 4);
    assert_eq!(obs[0].sources[3], fourth);
    assert_eq!(obs[0].entities, vec!["config_loader.py", "load_config"]);
    assert_eq!(log_lines(&dir).iter().filter(|line| line.contains("\"Observation\"")).count(), 2, "the refresh is one more line");
}

#[test]
fn a_shared_observation_splits_when_the_entities_stop_sharing_their_notes_and_joins_again() {
    let dir = TempDir::new("obs-split");
    let (mut bank, made) = shared_facts(&dir);
    let shared = observations(&bank)[0].id.clone();
    // A fourth note names only one of them.
    let fourth = add(&mut bank, "load_config also reads the ini format.", NOW + 10)[0].id.clone();
    let obs = observations(&bank);
    assert_eq!(obs.len(), 2, "{obs:#?}");
    let of = |name: &str| obs.iter().find(|o| o.entities == vec![name.to_string()]).unwrap_or_else(|| panic!("no observation of {name}"));
    assert_eq!(of("config_loader.py").sources, made);
    assert_eq!(of("load_config").sources.len(), 4);
    assert!(of("load_config").sources.contains(&fourth));
    assert!(bank.get(&shared).is_none(), "the shared observation was replaced");
    // Forgetting that note makes the two groups equal again.
    assert!(bank.forget(&fourth).unwrap());
    let obs = observations(&bank);
    assert_eq!(obs.len(), 1, "{obs:#?}");
    assert_eq!(obs[0].entities, vec!["config_loader.py", "load_config"]);
    assert_eq!(obs[0].sources, made);
    drop(bank);
    // What the log says is the same after a reopen.
    let bank = open(&dir);
    let obs = observations(&bank);
    assert_eq!((obs.len(), obs[0].entities.len(), obs[0].sources.len()), (1, 2, 3));
    assert_eq!(bank.len(), 4);
}

#[test]
fn forgetting_a_source_dissolves_a_shared_observation_too() {
    let dir = TempDir::new("obs-shared-forget");
    let (mut bank, made) = shared_facts(&dir);
    let shared = observations(&bank)[0].id.clone();
    assert!(bank.forget(&made[0]).unwrap());
    assert!(observations(&bank).is_empty(), "two notes are too few");
    assert!(bank.get(&shared).is_none());
    assert_eq!(bank.len(), 2);
}

#[test]
fn shared_observations_are_made_per_tag_set() {
    let dir = TempDir::new("obs-shared-tags");
    let mut bank = open(&dir);
    for (n, text) in SHARED_FACTS.iter().enumerate() {
        add_tagged(&mut bank, text, &["proj"], NOW + n as i64);
        add_tagged(&mut bank, &format!("{text} Untagged twin {n}."), &[], NOW + 10 + n as i64);
    }
    let obs = observations(&bank);
    assert_eq!(obs.len(), 2, "{obs:#?}");
    assert!(obs.iter().all(|o| o.entities == vec!["config_loader.py", "load_config"]), "{obs:#?}");
    let mut tags: Vec<Vec<String>> = obs.iter().map(|o| o.tags.clone()).collect();
    tags.sort();
    assert_eq!(tags, vec![Vec::<String>::new(), vec!["proj".to_string()]]);
}

/// The folder of a bank as an earlier version left it after three notes about two entities:
/// the observation written once per entity. Gives the ids of the three notes and of the two copies.
fn bank_with_old_copies(dir: &TempDir) -> (Vec<String>, [&'static str; 2]) {
    let (bank, made) = shared_facts(dir);
    let merged = observations(&bank)[0].clone();
    drop(bank);
    let log = dir.bank("b").join("notes.jsonl");
    let mut lines = log_lines(dir);
    lines.truncate(3);
    let old_ids = ["n000004", "n000005"];
    for (id, entity) in old_ids.iter().zip(["config_loader.py", "load_config"]) {
        let copy = Note { id: id.to_string(), entities: vec![entity.to_string()], ..merged.clone() };
        lines.push(serde_json::to_string(&copy).unwrap());
    }
    fs::write(&log, lines.join("\n") + "\n").unwrap();
    // The caches described the log as it was.
    for cache in ["index.bin", "vectors.bin"] {
        let _ = fs::remove_file(dir.bank("b").join(cache));
    }
    (made, old_ids)
}

#[test]
fn old_copies_per_entity_give_way_to_one_observation_on_the_next_retain_of_both() {
    let dir = TempDir::new("obs-heal-both");
    let (made, old_ids) = bank_with_old_copies(&dir);
    let mut bank = open(&dir);
    assert_eq!(observations(&bank).len(), 2, "opening and reading change nothing");
    let fourth = add(&mut bank, "config_loader.py hands load_config the path.", NOW + 10)[0].id.clone();
    let obs = observations(&bank);
    assert_eq!(obs.len(), 1, "{obs:#?}");
    assert_eq!(obs[0].entities, vec!["config_loader.py", "load_config"]);
    assert_eq!(obs[0].sources.len(), 4);
    assert_eq!(obs[0].sources[..3], made[..]);
    assert_eq!(obs[0].sources[3], fourth);
    for old in old_ids {
        assert!(bank.get(old).is_none(), "{old} is still there");
    }
    drop(bank);
    let reopened = open(&dir);
    assert_eq!(observations(&reopened).len(), 1);
    assert_eq!(reopened.len(), 5);
}

#[test]
fn old_copies_are_merged_by_the_next_write_whatever_it_is_about() {
    let dir = TempDir::new("obs-heal-any");
    let (made, old_ids) = bank_with_old_copies(&dir);
    let mut bank = open(&dir);
    assert_eq!(observations(&bank).len(), 2);
    // A note about something else touches neither entity.
    add(&mut bank, "The renderer paints widgets.", NOW + 10);
    let obs = observations(&bank);
    assert_eq!(obs.len(), 1, "{obs:#?}");
    assert_eq!(obs[0].entities, vec!["config_loader.py", "load_config"]);
    assert_eq!(obs[0].sources, made);
    assert!(obs[0].text.starts_with("config_loader.py, load_config: "), "{}", obs[0].text);
    for old in old_ids {
        assert!(bank.get(old).is_none(), "{old} is still there");
    }
    drop(bank);
    // The log says the same: a reopened bank has the one observation, and a forget of an old
    // copy finds nothing to forget.
    let mut reopened = open(&dir);
    assert_eq!(observations(&reopened).len(), 1);
    assert!(!reopened.forget(old_ids[0]).unwrap());
    // A forget is a write too, and it settles copies as well.
    let dir2 = TempDir::new("obs-heal-forget");
    let (made2, _) = bank_with_old_copies(&dir2);
    let mut bank2 = open(&dir2);
    assert_eq!(observations(&bank2).len(), 2);
    assert!(bank2.forget(&made2[0]).unwrap());
    assert!(observations(&bank2).is_empty(), "two notes are too few, copies included");
}
// ---- days in the zone of the caller ----

#[test]
fn today_and_yesterday_in_a_retained_text_are_the_days_of_the_callers_zone() {
    let dir = TempDir::new("zone-retain");
    let mut bank = open(&dir);
    // 2026-03-11 02:00 UTC is still the evening of the 10th at UTC-5.
    let clock = midnight(2026, 3, 11).unwrap() + 2 * 3600;
    let west = -5 * 3600;
    let put = |bank: &mut Bank, text: &str, utc_offset: i64| {
        let opts = RetainOpts { utc_offset, ..retain_at(clock) };
        bank.retain(text, &opts, &NoCode, None).unwrap()[0].id.clone()
    };
    let utc = put(&mut bank, "The build broke today.", 0);
    let today = put(&mut bank, "The cache was cleared today.", west);
    let yesterday = put(&mut bank, "The disk filled up yesterday.", west);
    let span = |id: &str| (bank.get(id).unwrap().occurred_start.unwrap(), bank.get(id).unwrap().occurred_end.unwrap());
    let day = |d: u8, offset: i64| (midnight(2026, 3, d).unwrap() - offset, midnight(2026, 3, d).unwrap() - offset + DAY - 1);
    assert_eq!(span(&utc), day(11, 0), "no offset reads the UTC day, as before");
    assert_eq!(span(&today), day(10, west));
    assert_eq!(span(&yesterday), day(9, west));
    // A clock that is later in the evening of the same UTC day is the same day at UTC-5.
    let default = bank.retain("The queue stalled today.", &retain_at(clock), &NoCode, None).unwrap();
    assert_eq!(bank.get(&default[0].id).unwrap().occurred_start, Some(midnight(2026, 3, 11).unwrap()));
}

#[test]
fn a_query_with_yesterday_asks_for_the_day_of_the_callers_zone() {
    let dir = TempDir::new("zone-recall");
    let mut bank = open(&dir);
    let at = |t: i64| RetainOpts { when: Some(t), ..retain_at(NOW) };
    // 2026-03-10 12:00 UTC is 07:00 on the 10th at UTC-5; 2026-03-11 20:00 UTC is 15:00 on the 11th.
    let early = bank.retain("the deploy finished early", &at(midnight(2026, 3, 10).unwrap() + 12 * 3600), &NoCode, None).unwrap()[0].id.clone();
    let late = bank.retain("the deploy finished late", &at(midnight(2026, 3, 11).unwrap() + 20 * 3600), &NoCode, None).unwrap()[0].id.clone();
    // 2026-03-12 02:00 UTC is 21:00 on the 11th at UTC-5: yesterday there is the 10th, in UTC it is the 11th.
    let clock = midnight(2026, 3, 12).unwrap() + 2 * 3600;
    let only_time = Strategies::only("time").unwrap();
    let ask = |utc_offset: i64| {
        let opts = RecallOpts { utc_offset, limit: 10, ..recall_at(clock) };
        ids(&bank.recall_with("deploy yesterday", &opts, &NoCode, None, only_time))
    };
    assert_eq!(ask(0), vec![late.clone()], "yesterday in UTC is the 11th");
    assert_eq!(ask(-5 * 3600), vec![early.clone()], "yesterday at UTC-5 is the 10th");
    assert_eq!(ask(RecallOpts::default().utc_offset), ask(0), "the default is UTC");
}

// ---- catching up with another bank ----

#[test]
fn a_bank_that_is_held_catches_up_with_the_notes_another_bank_wrote() {
    let dir = TempDir::new("refresh");
    let mut reader = open(&dir);
    let mut writer = open(&dir);
    assert!(!reader.refresh().unwrap(), "nothing was written");
    let first = add(&mut writer, "the scheduler retries failed jobs", NOW)[0].id.clone();
    assert_eq!(reader.len(), 0, "a bank only looks when it is asked to");
    assert!(reader.refresh().unwrap(), "the log grew");
    assert_eq!(reader.get(&first).unwrap().text, "the scheduler retries failed jobs");
    assert!(!reader.refresh().unwrap(), "and the second look finds nothing new");
    // Another note, and a forget.
    let second = add(&mut writer, "the renderer paints widgets", NOW + 1)[0].id.clone();
    assert!(writer.forget(&first).unwrap());
    assert!(reader.refresh().unwrap());
    assert!(reader.get(&first).is_none(), "the forgotten note is gone");
    assert!(reader.get(&second).is_some());
    assert_eq!(reader.len(), 1);
    // What the reader answers is built from what it caught up with.
    let found = reader.recall("renderer widgets", &recall_at(NOW + 5), &NoCode, None);
    assert_eq!(ids(&found), vec![second]);
}

#[test]
fn a_bank_that_wrote_itself_has_nothing_to_catch_up_with() {
    let dir = TempDir::new("refresh-own");
    let mut bank = open(&dir);
    add(&mut bank, "the scheduler retries failed jobs", NOW);
    assert!(!bank.refresh().unwrap(), "its own append is not news");
    bank.flush().unwrap();
    assert!(!bank.refresh().unwrap(), "nor is writing the caches");
    let id = add(&mut bank, "the renderer paints widgets", NOW + 1)[0].id.clone();
    assert!(bank.forget(&id).unwrap());
    assert!(!bank.refresh().unwrap());
    assert_eq!(bank.len(), 1);
}

#[test]
fn a_bank_catches_up_from_the_caches_the_other_bank_wrote_when_they_are_current() {
    let dir = TempDir::new("refresh-caches");
    let mut reader = open(&dir);
    let mut writer = open(&dir);
    for n in 0..4 {
        add(&mut writer, &format!("the module `mod_{n}` handles case {n}."), NOW + n);
    }
    writer.flush().unwrap();
    assert!(reader.refresh().unwrap());
    assert!(!reader.dirty, "the snapshot of the other bank was current, so nothing was rebuilt");
    assert_eq!(reader.len(), 4);
    // Without the other bank's caches the notes are read from the log.
    add(&mut writer, "the module `mod_9` handles case 9.", NOW + 9);
    assert!(reader.refresh().unwrap());
    assert!(reader.dirty);
    assert_eq!(reader.len(), 5);
}

#[test]
fn a_log_that_changed_without_a_change_in_length_is_read_again() {
    let dir = TempDir::new("refresh-time");
    let mut bank = open(&dir);
    let id = add(&mut bank, "the scheduler retries failed jobs", NOW)[0].id.clone();
    assert!(!bank.refresh().unwrap());
    // The same number of bytes with another word in them, and a new modification time.
    let log = dir.bank("b").join("notes.jsonl");
    let text = fs::read_to_string(&log).unwrap().replace("scheduler", "schedular");
    fs::write(&log, text).unwrap();
    let file = fs::OpenOptions::new().write(true).open(&log).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600)).unwrap();
    drop(file);
    assert!(bank.refresh().unwrap());
    assert_eq!(bank.get(&id).unwrap().text, "the schedular retries failed jobs");
    assert!(!bank.refresh().unwrap());
}

#[test]
fn a_bank_that_catches_up_can_write_without_losing_what_the_other_bank_wrote() {
    let dir = TempDir::new("refresh-write");
    let mut one = open(&dir);
    let mut two = open(&dir);
    let a = add(&mut one, "the scheduler retries failed jobs", NOW)[0].id.clone();
    assert!(two.refresh().unwrap());
    let b = add(&mut two, "the renderer paints widgets", NOW + 1)[0].id.clone();
    assert_ne!(a, b);
    assert!(one.refresh().unwrap());
    assert_eq!(one.len(), 2);
    drop(one);
    drop(two);
    assert_eq!(open(&dir).len(), 2);
}

// ---- what a flush that fails says ----

#[test]
fn a_cache_that_cannot_be_written_is_a_cache_error_and_the_notes_are_safe() {
    let dir = TempDir::new("flush-cache");
    let mut bank = open(&dir);
    let b = dir.bank("b");
    // A folder where index.bin belongs: the snapshot cannot be put in place.
    fs::create_dir_all(b.join("index.bin")).unwrap();
    let kept = add(&mut bank, "the scheduler retries failed jobs", NOW)[0].id.clone();
    let error = bank.flush().unwrap_err();
    assert!(matches!(error, FlushError::Cache(_)), "a cache failure is not a log failure: {error}");
    assert!(!error.is_log());
    assert!(error.to_string().contains("cache"), "{error}");
    assert!(matches!(bank.flush(), Err(FlushError::Cache(_))), "it fails the same way until the cause is gone");
    // The note is in the log, and a bank opened from the log has it.
    assert!(log_lines(&dir).iter().any(|line| line.contains(&kept)));
    assert_eq!(Bank::open(&b).unwrap().len(), 1);
    // Once the cause is gone, the same bank writes its caches.
    fs::remove_dir(b.join("index.bin")).unwrap();
    bank.flush().unwrap();
    assert!(b.join("index.bin").is_file());
    drop(bank);
    assert!(!open(&dir).dirty, "the caches are current");
}

#[test]
fn a_flush_error_converts_to_an_io_error_that_keeps_the_cause() {
    let cause = || std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access is denied");
    for error in [FlushError::Log(cause()), FlushError::Cache(cause())] {
        let text = error.to_string();
        let io: std::io::Error = error.into();
        assert_eq!(io.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(io.to_string().contains("access is denied") && io.to_string() == text, "{io}");
    }
    assert!(FlushError::Log(cause()).to_string().contains("notes"));
    assert!(FlushError::Cache(cause()).to_string().contains("cache"));
}

// ---- read-only use and the judgement of a note ----

/// The bytes of every file in a folder, by name.
fn folder_bytes(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), fs::read(e.path()).unwrap()))
        .collect();
    files.sort();
    files
}

#[test]
fn a_bank_opened_for_reading_writes_nothing_even_when_its_caches_are_missing() {
    let dir = TempDir::new("readonly");
    let b = dir.bank("b");
    {
        let mut bank = open(&dir);
        add(&mut bank, "the scheduler retries failed jobs", NOW);
        add(&mut bank, "the exporter writes csv files", NOW);
    }
    // Without the caches a normal open is out of date and writes them again when it is dropped.
    assert!(b.join("index.bin").is_file(), "a dropped bank writes its caches");
    let missing_caches = || {
        let _ = fs::remove_file(b.join("index.bin"));
        let _ = fs::remove_file(b.join("vectors.bin"));
    };
    missing_caches();
    // The control: a bank opened in the usual way does write them again when it is dropped.
    drop(open(&dir));
    assert!(b.join("index.bin").is_file(), "the test would see a write");
    missing_caches();
    let before = folder_bytes(&b);
    {
        let bank = Bank::open_read_only(&b).unwrap();
        assert_eq!(bank.len(), 2);
        let found = bank.recall("scheduler jobs", &recall_at(NOW), &NoCode, None);
        assert_eq!(texts(&found), ["the scheduler retries failed jobs"]);
    }
    assert_eq!(folder_bytes(&b), before, "nothing was written");
    assert!(!b.join("index.bin").exists() && !b.join("vectors.bin").exists());
}

#[test]
fn a_bank_opened_for_reading_refuses_changes_and_does_not_make_its_folder() {
    let dir = TempDir::new("readonly-refuse");
    let b = dir.bank("b");
    let missing = Bank::open_read_only(&b).unwrap();
    assert!(missing.is_empty());
    drop(missing);
    assert!(!b.exists(), "a folder that is not there stays not there");

    let id = {
        let mut bank = open(&dir);
        add(&mut bank, "the scheduler retries failed jobs", NOW)[0].id.clone()
    };
    let mut bank = Bank::open_read_only(&b).unwrap();
    let error = bank.retain("another fact about the exporter", &retain_at(NOW), &NoCode, None).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(bank.forget(&id).unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
    assert!(bank.flush().is_ok());
    assert_eq!(bank.len(), 1, "the note is still there");
}

#[test]
fn judging_a_note_gives_stale_for_a_changed_definition_and_gone_for_a_removed_one() {
    let dir = TempDir::new("judge");
    let code = FakeCode::default();
    code.define("parse_config", "src/config.rs", 42);
    code.define("load_all", "src/load.rs", 7);
    let mut bank = open(&dir);
    let both = bank.retain("parse_config is called by load_all.", &retain_at(NOW), &code, None).unwrap()[0].id.clone();
    let plain = add(&mut bank, "nothing here names code", NOW)[0].id.clone();
    let judged = |bank: &Bank, id: &str, code: &dyn CodeView| judge_note(bank.get(id).unwrap(), code);

    assert_eq!(judged(&bank, &both, &code), (false, false));
    code.define("parse_config", "src/config.rs", 43);
    assert_eq!(judged(&bank, &both, &code), (true, false));
    code.remove("load_all");
    assert_eq!(judged(&bank, &both, &code), (true, true));
    assert_eq!(judged(&bank, &plain, &code), (false, false), "a note that names no code is never judged");
    assert_eq!(judged(&bank, &both, &NoCode), (false, false), "a view that knows no code judges nothing");
}
