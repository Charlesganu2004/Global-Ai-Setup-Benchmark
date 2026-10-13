//! The team hub: `pgr handoff` files what an agent found for a task, and `pgr loadout` says what
//! the next agent should load before it starts: the notes filed for its task and the durable
//! ones, cut to a token budget. The roles are on the notes, as the ones who filed or confirmed
//! them; the caller's own role does not change what is chosen. The memory is read in layers, the
//! durable and small ones first:
//!
//! - L0 raw: the repository itself. Notes point at definitions (anchors) and are checked
//!   against them, so a note about code that has changed since is printed as stale. A note has
//!   an anchor when it names code in a spelling the entity rules of the bank know, and exactly
//!   one definition has the name: a name in backticks, `snake_case`, `camelCase`, or a dotted name
//!   such as `Store.reload`. A plain word such as `main`, a call form such as `slugify()` and the
//!   form `app/util.py::slugify` are not checked. The anchors are judged against the
//!   repository of the one who reads, so in a bank shared by repositories a note about code that
//!   the reader's repository does not hold reads as gone.
//! - L1 notes: what `retain` and `handoff` keep.
//! - L2 task: the notes filed for one task, tagged `task:NAME`.
//! - L3 core: directives, and notes confirmed by repetition or by more than one role.
//!
//! The ideas of layers, of a loadout and of context put into an agent's brief come from
//! TencentCloud/TencentDB-Agent-Memory (MIT). No code of it is used, and nothing here calls a
//! model: choosing and cutting is plain computation.

use crate::cli::{HandoffArgs, LoadoutArgs, RetainArgs};
use crate::ctx::Ctx;
use crate::fail::{Fail, Res};
use crate::{localtime, memcmd};
use lxr_core::est_tokens;
use lxr_memory::{judge_note, Bank, Budget, CodeView, Kind, Note, RecallOpts};
use lxr_text::Embedder;
use std::collections::HashSet;
use std::io::Read;

/// The most bytes `handoff` reads from standard input.
pub const STDIN_LIMIT: usize = 65_536;
/// The context every note of a handoff carries.
const HANDOFF_CONTEXT: &str = "handoff";
/// A note seen this many times is confirmed by repetition.
const CONFIRMED_SEEN: u32 = 3;
/// A note filed by this many different roles is confirmed by more than one role.
const CONFIRMED_ROLES: usize = 2;
/// The most notes the related layer holds.
const RELATED_NOTES: usize = 4;
/// The line that follows a loadout that has notes the code has moved away from.
const CHECK_LINE: &str = "check stale and gone notes against the code before relying on them";

/// The text on standard input, for `pgr handoff -`.
fn read_stdin() -> Res<String> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(STDIN_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Fail::usage(format!("cannot read standard input: {e}")))?;
    if bytes.len() > STDIN_LIMIT {
        return Err(Fail::usage(format!("the text on standard input is longer than {STDIN_LIMIT} bytes")));
    }
    let text = String::from_utf8(bytes).map_err(|_| Fail::usage("the text on standard input is not valid UTF-8"))?;
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

/// `pgr handoff TEXT...`: keeps the text as `retain` does, tagged with the task and the role, and
/// says how many notes it made or confirmed and how many of them name code.
pub fn handoff(ctx: &mut Ctx, args: &HandoffArgs, out: &mut String) -> Res<()> {
    let task = ctx.task(args.task.as_deref())?.ok_or_else(|| Fail::usage("handoff needs --task NAME"))?;
    let role = ctx.role(args.role.as_deref())?;
    let text = if args.stdin { read_stdin()? } else { args.text.clone() };
    if text.trim().is_empty() {
        return Err(Fail::usage("handoff needs the text to keep"));
    }
    let mut tags = args.tags.clone();
    tags.push(format!("task:{task}"));
    tags.push(format!("from:{role}"));
    let retain = RetainArgs { text: String::new(), kind: args.kind, tags, context: HANDOFF_CONTEXT.to_string(), when: None, whole: args.whole };
    // A fact that is a near-duplicate of an older note reinforces it, and the bank adds the tags
    // to the older note: this is how a fact becomes confirmed by a second role.
    let kept = memcmd::keep(ctx, &text, &memcmd::retain_opts(&retain))?;
    memcmd::kept_lines(&kept, out);
    // A fact that repeats inside the text reinforces the note it made: that note is counted once.
    let mut seen: HashSet<&str> = HashSet::new();
    let (notes, naming_code) = kept
        .iter()
        .filter(|k| seen.insert(k.id.as_str()))
        .fold((0usize, 0usize), |(notes, naming_code), k| (notes + 1, naming_code + usize::from(k.anchored)));
    ln!(out, "handoff {task}: {notes} notes from {role}, {naming_code} name code");
    Ok(())
}

/// Whether the code a note names is as it was when the note was written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Fresh,
    Stale,
    Gone,
}

impl State {
    fn of(stale: bool, gone: bool) -> State {
        if gone {
            State::Gone
        } else if stale {
            State::Stale
        } else {
            State::Fresh
        }
    }
}

/// One note of a layer, ready to print.
struct Entry {
    id: String,
    state: State,
    line: String,
    /// What the line costs in the budget. It does not depend on the age in the line, so that an
    /// age that gains a character (`9s`, `10s`) cannot move the place where a layer is cut.
    cost: usize,
}

/// The roles that filed or confirmed a note, in alphabetical order, each once.
fn roles_of(note: &Note) -> Vec<&str> {
    let mut roles: Vec<&str> = note
        .tags
        .iter()
        .filter_map(|tag| tag.get(..5).filter(|head| head.eq_ignore_ascii_case("from:")).map(|_| &tag[5..]))
        .filter(|role| !role.is_empty())
        .collect();
    roles.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()).then(a.cmp(b)));
    roles.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    roles
}

/// True for a summary the bank built itself (kind observation) from the notes of one entity. Its
/// text is theirs joined, its proof count is how many notes it covers and not how often anyone
/// saw a fact, and the notes it covers are in the layers on their own.
fn is_digest(note: &Note) -> bool {
    note.kind == Kind::Observation
}

/// True for a note confirmed by repetition or by more than one role. A digest is not.
fn confirmed(note: &Note) -> bool {
    !is_digest(note) && (note.proof_count >= CONFIRMED_SEEN || roles_of(note).len() >= CONFIRMED_ROLES)
}

/// True for a note filed for the task.
fn filed_for(note: &Note, task: &str) -> bool {
    let tag = format!("task:{task}");
    note.tags.iter().any(|t| t.eq_ignore_ascii_case(&tag))
}

/// A note as a layer prints it, with the roles before the id.
fn entry(note: &Note, stale: bool, gone: bool, now: i64) -> Entry {
    let roles = roles_of(note);
    let suffix = if roles.is_empty() { String::new() } else { format!(" [{}]", roles.join("+")) };
    // The cost is of the line as it reads at the moment the note was written (an age of `0s`).
    let at_birth = memcmd::line_with(note, stale, gone, memcmd::written(note), &suffix);
    Entry {
        id: note.id.clone(),
        state: State::of(stale, gone),
        line: memcmd::line_with(note, stale, gone, now, &suffix),
        cost: cost(&at_birth),
    }
}

/// The entry of a note judged against the code now.
fn judged(note: &Note, code: &dyn CodeView, now: i64) -> Entry {
    let (stale, gone) = judge_note(note, code);
    entry(note, stale, gone, now)
}

/// What a line costs in the budget: its tokens and its line break.
fn cost(line: &str) -> usize {
    est_tokens(line) + 1
}

fn more_line(notes: usize) -> String {
    format!("  +{notes} more")
}

/// A layer cut to a budget.
struct Layer {
    /// The title, the notes that fit and, when the budget cut some, the `+K more` line. Nothing at
    /// all for a layer with no note whose title and `+K more` line do not fit its share.
    lines: Vec<String>,
    /// The notes that were printed.
    shown: Vec<Entry>,
    /// Tokens the lines cost.
    used: usize,
}

impl Layer {
    /// True when the layer has anything to print besides a title.
    fn prints(&self) -> bool {
        self.lines.len() > 1
    }
}

/// Cuts a layer to `tokens`: its title, then the notes in order while they fit, then `  +K more`
/// for the notes that did not. A note is never cut in the middle. A note that does not fit is
/// left out and counted in the `+K more` line, and the notes after it are still tried, so one long
/// note cannot hide the ones behind it; the notes that are printed keep their order. When the
/// budget has room for the title and the `+K more` line and no note, that is all the layer prints;
/// when it has not even that, the layer prints nothing, so that it never costs more than its share.
fn fit(title: &str, entries: Vec<Entry>, tokens: usize) -> Layer {
    let total = entries.len();
    let mut lines = vec![title.to_string()];
    let mut used = cost(title);
    let mut shown: Vec<Entry> = Vec::new();
    let mut skipped = 0;
    for (i, entry) in entries.into_iter().enumerate() {
        // The notes that would be counted in the `+K more` line if this one is printed and none
        // after it is: those skipped so far and all that follow.
        let left_out = skipped + (total - i - 1);
        let reserve = if left_out > 0 { cost(&more_line(left_out)) } else { 0 };
        if used + entry.cost + reserve > tokens {
            skipped += 1;
            continue;
        }
        used += entry.cost;
        lines.push(entry.line.clone());
        shown.push(entry);
    }
    if skipped > 0 {
        let line = more_line(skipped);
        used += cost(&line);
        lines.push(line);
    }
    if shown.is_empty() && used > tokens {
        return Layer { lines: Vec::new(), shown, used: 0 };
    }
    Layer { lines, shown, used }
}

/// What a loadout chose to print.
#[derive(Default, Debug)]
pub struct Loadout {
    /// Every layer's lines, in order, with their titles.
    pub lines: Vec<String>,
    pub notes: usize,
    pub fresh: usize,
    pub stale: usize,
    pub gone: usize,
    /// Tokens the lines cost, line breaks included.
    pub tokens: usize,
}

/// What a loadout is asked for.
pub struct Want<'a> {
    /// The task whose notes the task layer holds.
    pub task: Option<&'a str>,
    /// Words the related layer looks for.
    pub query: &'a str,
    pub budget: usize,
    /// Whether the related layer is wanted.
    pub related: bool,
}

/// Chooses the layers for a task and cuts them to the budget. The core layer (directives, then
/// the confirmed notes) takes at most a third. The task layer, when there is a task, takes every
/// note filed for the task that the core layer did not print: fresh ones first, then stale, then
/// gone, the newest first in each. The related layer, when it is wanted and the query has words,
/// takes at most four notes a recall for the words finds among the others, within a third; the
/// task layer gets what the other two do not use, never less than a third.
fn choose(bank: &Bank, code: &dyn CodeView, embedder: Option<&Embedder>, now: i64, want: &Want) -> Loadout {
    let Want { task, query, budget, related } = *want;
    let third = (budget / 3).max(1);
    let notes = bank.notes();

    let mut core: Vec<&Note> = notes.iter().copied().filter(|n| n.kind == Kind::Directive).collect();
    core.extend(notes.iter().copied().filter(|n| n.kind != Kind::Directive && confirmed(n)));
    let core = Some(fit("== core", core.into_iter().map(|n| judged(n, code, now)).collect(), third)).filter(Layer::prints);
    let taken: HashSet<String> = core.iter().flat_map(|l| l.shown.iter().map(|e| e.id.clone())).collect();

    // A digest is not a note of the task: the notes it covers are, and they print on their own.
    let mut for_task: Vec<Entry> = match task {
        Some(task) => notes
            .iter()
            .copied()
            .filter(|n| filed_for(n, task) && !is_digest(n) && !taken.contains(&n.id))
            .map(|n| judged(n, code, now))
            .collect(),
        None => Vec::new(),
    };
    // A stable sort: the notes keep their order, newest first, inside each state.
    for_task.sort_by_key(|e| e.state as u8);
    let filed: HashSet<String> = for_task.iter().map(|e| e.id.clone()).collect();

    let related = if related && !query.is_empty() {
        let opts = RecallOpts {
            limit: bank.len().max(1),
            budget: Budget::Mid,
            max_tokens: usize::MAX / 4,
            utc_offset: localtime::utc_offset(),
            ..RecallOpts::default()
        };
        // The recall of `pgr recall WORDS`, as it is: it also finds notes by meaning and by the names
        // they share with the notes that match, and a loadout is not stricter than a recall.
        let found = bank.recall(query, &opts, code, embedder);
        let entries: Vec<Entry> = found
            .iter()
            .filter(|r| !taken.contains(&r.note.id) && !filed.contains(&r.note.id))
            .take(RELATED_NOTES)
            .map(|r| entry(&r.note, r.stale, r.gone, now))
            .collect();
        (!entries.is_empty()).then(|| fit("== related", entries, third)).filter(Layer::prints)
    } else {
        None
    };

    // A layer that prints nothing costs nothing: the task layer gets what the others do not use.
    let spent = core.as_ref().map_or(0, |l| l.used) + related.as_ref().map_or(0, |l| l.used);
    let left = budget.saturating_sub(spent).max(third);
    let task_layer = match task {
        Some(task) if !for_task.is_empty() => Some(fit(&format!("== task {task}"), for_task, left)).filter(Layer::prints),
        _ => None,
    };

    let mut loadout = Loadout::default();
    for layer in [core, task_layer, related].into_iter().flatten() {
        for shown in &layer.shown {
            match shown.state {
                State::Fresh => loadout.fresh += 1,
                State::Stale => loadout.stale += 1,
                State::Gone => loadout.gone += 1,
            }
        }
        loadout.notes += layer.shown.len();
        loadout.tokens += layer.used;
        loadout.lines.extend(layer.lines);
    }
    loadout
}

/// The loadout for a task and some words, read from the bank without changing it. Nothing at all
/// when the bank has no folder or no note to print.
pub fn load(ctx: &mut Ctx, want: &Want) -> Res<Loadout> {
    let wants_words = want.related && !want.query.is_empty();
    ctx.need_memory_with(wants_words)?;
    let Some(bank) = ctx.read_bank()? else {
        return Ok(Loadout::default());
    };
    let embedder = if wants_words { ctx.embed.get() } else { None };
    Ok(choose(&bank, ctx.code_view(), embedder, ctx.now(), want))
}

/// `pgr loadout [WORDS...] [--budget N]`.
pub fn loadout(ctx: &mut Ctx, args: &LoadoutArgs, out: &mut String) -> Res<()> {
    let task = ctx.task(args.task.as_deref())?;
    // The loadout is the same for every caller, but a role name is refused wherever it comes from.
    ctx.role(None)?;
    let chosen = load(ctx, &Want { task: task.as_deref(), query: &args.query, budget: args.budget, related: true })?;
    if chosen.lines.is_empty() {
        ln!(out, "no notes");
        return Ok(());
    }
    for line in &chosen.lines {
        ln!(out, "{line}");
    }
    ln!(
        out,
        "loadout: {} notes, {} fresh, {} stale, {} gone, about {} tokens",
        chosen.notes,
        chosen.fresh,
        chosen.stale,
        chosen.gone,
        chosen.tokens
    );
    if chosen.stale + chosen.gone > 0 {
        ln!(out, "{CHECK_LINE}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lxr_memory::NoCode;

    fn tagged(id: &str, tags: &[&str], proof: u32) -> Note {
        Note {
            id: id.to_string(),
            text: format!("text of {id}"),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            proof_count: proof,
            ..Note::default()
        }
    }

    /// A fresh entry with this line, which costs what the line costs.
    fn plain(id: &str, line: String) -> Entry {
        let cost = cost(&line);
        Entry { id: id.to_string(), state: State::Fresh, line, cost }
    }

    fn entries(count: usize) -> Vec<Entry> {
        (0..count).map(|i| plain(&format!("n{i}"), format!("- note number {i} with some words in it  (1m ago) #n{i}"))).collect()
    }

    /// An entry whose text is `words` words long.
    fn sized(id: &str, words: usize) -> Entry {
        let text = (0..words).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ");
        plain(id, format!("- {text}  (1m ago) #{id}"))
    }

    #[test]
    fn roles_are_alphabetical_each_once_and_read_from_the_from_tags() {
        let note = tagged("n1", &["task:t", "from:scout", "From:Reviewer", "from:scout", "from:", "other"], 1);
        assert_eq!(roles_of(&note), ["Reviewer", "scout"]);
        assert!(roles_of(&tagged("n2", &["task:t"], 1)).is_empty());
    }

    #[test]
    fn a_note_is_confirmed_by_three_sightings_or_two_roles() {
        assert!(!confirmed(&tagged("a", &[], 2)));
        assert!(confirmed(&tagged("b", &[], 3)));
        assert!(!confirmed(&tagged("c", &["from:scout", "from:Scout"], 1)));
        assert!(confirmed(&tagged("d", &["from:scout", "from:reviewer"], 1)));
    }

    #[test]
    fn a_note_is_filed_for_a_task_by_its_tag() {
        let note = tagged("a", &["task:fix-1"], 1);
        assert!(filed_for(&note, "fix-1") && filed_for(&note, "FIX-1"));
        assert!(!filed_for(&note, "fix") && !filed_for(&note, "other"));
    }

    #[test]
    fn the_line_has_the_roles_before_the_id() {
        let note = tagged("n000007", &["from:scout", "from:reviewer"], 3);
        let line = entry(&note, true, false, 0).line;
        assert!(line.starts_with("- text of n000007  (") && line.ends_with("seen 3 times, stale: code changed) [reviewer+scout] #n000007"), "{line}");
        let plain = entry(&tagged("n000008", &[], 1), false, false, 0).line;
        assert!(plain.ends_with(") #n000008") && !plain.contains('['), "{plain}");
    }

    #[test]
    fn a_layer_that_fits_has_no_more_line() {
        let layer = fit("== core", entries(3), 1000);
        assert_eq!(layer.shown.len(), 3);
        assert_eq!(layer.lines.len(), 4);
        assert!(layer.lines.iter().all(|l| !l.contains("more")));
        assert_eq!(layer.used, layer.lines.iter().map(|l| cost(l)).sum::<usize>());
    }

    #[test]
    fn a_layer_the_budget_cuts_ends_with_the_count_of_notes_left_out() {
        let each = cost(&entries(1)[0].line);
        let budget = cost("== core") + 2 * each + cost("  +3 more");
        let layer = fit("== core", entries(5), budget);
        assert_eq!(layer.shown.len(), 2);
        assert_eq!(layer.lines.last().unwrap(), "  +3 more");
        assert!(layer.used <= budget, "{} of {budget}", layer.used);
    }

    #[test]
    fn a_budget_with_room_for_no_note_prints_the_title_and_the_count() {
        let layer = fit("== core", entries(4), cost("== core") + cost("  +4 more"));
        assert!(layer.shown.is_empty());
        assert_eq!(layer.lines, ["== core", "  +4 more"]);
    }

    #[test]
    fn a_budget_with_room_for_not_even_the_title_and_the_count_prints_nothing() {
        let layer = fit("== core", entries(4), cost("== core") + cost("  +4 more") - 1);
        assert!(layer.shown.is_empty() && layer.lines.is_empty());
        assert_eq!(layer.used, 0);
    }

    #[test]
    fn a_layer_never_costs_more_than_its_share() {
        for tokens in 0..300 {
            let list = vec![sized("a", 3), sized("b", 90), sized("c", 8), sized("d", 40), sized("e", 1)];
            let layer = fit("== task t1", list, tokens);
            assert!(layer.used <= tokens, "{} of {tokens}: {:?}", layer.used, layer.lines);
            assert_eq!(layer.used, layer.lines.iter().map(|l| cost(l)).sum::<usize>());
        }
    }

    #[test]
    fn a_note_too_big_for_the_layer_is_counted_in_the_more_line_and_the_notes_after_it_still_print() {
        let mut list = vec![sized("big", 400)];
        list.extend(entries(2));
        let each = cost(&entries(1)[0].line);
        let layer = fit("== core", list, cost("== core") + 2 * each + cost("  +1 more"));
        assert_eq!(layer.shown.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["n0", "n1"]);
        assert_eq!(layer.lines.last().unwrap(), "  +1 more");

        // In the middle of the list as well, and the count is of every note left out.
        let list = vec![entries(1).remove(0), sized("big", 400), sized("huge", 900), sized("b", 1)];
        let layer = fit("== core", list, 60);
        assert_eq!(layer.shown.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["n0", "b"]);
        assert_eq!(layer.lines.last().unwrap(), "  +2 more");
        assert!(layer.used <= 60);
    }

    #[test]
    fn a_digest_the_bank_built_is_not_confirmed_however_many_notes_it_covers() {
        let digest = Note { kind: Kind::Observation, proof_count: 28, ..tagged("n9", &["task:t", "from:scout"], 28) };
        assert!(!confirmed(&digest));
        let two_roles = Note { kind: Kind::Observation, ..tagged("n10", &["from:scout", "from:reviewer"], 2) };
        assert!(!confirmed(&two_roles));
    }

    #[test]
    fn a_line_costs_the_same_at_every_age() {
        // "9s ago" and "10s ago" are one character apart, and the line is three characters short
        // of a token boundary at the first: an age that grows must not move the cut of a layer.
        let note = Note { id: "n000001".to_string(), text: "abc".to_string(), created_at: 1000, ..Note::default() };
        let costs: Vec<usize> = [0, 9, 10, 89, 90, 5_399, 5_400, 172_799, 172_800, 9_000_000]
            .iter()
            .map(|age| entry(&note, false, false, 1000 + age).cost)
            .collect();
        assert!(costs.windows(2).all(|pair| pair[0] == pair[1]), "{costs:?}");
    }

    #[test]
    fn the_last_note_needs_no_room_for_a_more_line() {
        let each = cost(&entries(1)[0].line);
        let layer = fit("== core", entries(2), cost("== core") + 2 * each);
        assert_eq!(layer.shown.len(), 2);
        assert_eq!(layer.lines.len(), 3);
    }

    #[test]
    fn notes_that_name_no_code_are_fresh_and_a_view_without_code_judges_nothing() {
        let note = tagged("a", &[], 1);
        assert_eq!(judged(&note, &NoCode, 0).state, State::Fresh);
    }
}
