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
