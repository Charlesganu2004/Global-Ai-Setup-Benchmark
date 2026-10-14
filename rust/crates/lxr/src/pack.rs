//! `pgr pack "TASK"`: one answer for a task, meant to be pasted into an agent's brief so that the
//! agent does not spend a turn looking things up. Its sections, in this order: the definitions the
//! task text names, the best search hits, a walk of the graph seeded by the task text, and the
//! notes that go with the task. A section is left out when it is empty.
//!
//! The budget is shared by one rule. The names section may use up to half of it. What is left is
//! shared equally by the find, graph and memory sections that are on, in that order, and what a
//! section does not use passes to the sections after it.

use crate::cli::FindArgs;
use crate::ctx::{Ctx, Engine, Needs};
use crate::fail::Res;
use crate::hub::{self, Want};
use crate::{findcmd, graphcmd, memcmd};
use lxr_core::est_tokens;
use lxr_graph::Graph;
use lxr_rag::Mode;
use std::collections::{HashSet, VecDeque};

/// Hits asked of the search before they are cut to the section's share.
const HITS_ASKED: usize = 8;
/// Notes asked of the bank before they are cut to the section's share.
const NOTES_ASKED: usize = 6;
/// The most definitions the names section describes.
const MAX_NAMES: usize = 4;
/// The most names of a task text that are looked up in the graph, however long the text is.
const MAX_LOOKUPS: usize = 64;

/// Cuts a line to at most `bytes` bytes of UTF-8, after a whole word: at a character boundary, and
/// back to the end of the last word that fits when the cut would fall inside one. A first word
/// that does not fit leaves nothing.
fn cut_words(line: String, bytes: usize) -> String {
    if line.len() <= bytes {
        return line;
    }
    let mut at = bytes;
    while !line.is_char_boundary(at) {
        at -= 1;
    }
    let head = &line[..at];
    let inside_a_word = !line[at..].starts_with(char::is_whitespace);
    let head = match head.rfind(char::is_whitespace) {
        Some(space) if inside_a_word => &head[..space],
        None if inside_a_word => "",
        _ => head,
    };
    head.trim_end().to_string()
}

/// A line the index cut to size ends in `...`. When that cut left half a word, the line goes back to
/// the end of the word before it, so that pack prints no half word. A line whose last word ends in
/// punctuation, or that holds no other word, is left as it is.
fn whole_words(line: &str) -> String {
    let Some(head) = line.strip_suffix("...") else { return line.to_string() };
    if !head.ends_with(|c: char| c.is_alphanumeric() || c == '_') {
        return line.to_string();
    }
    match head.rfind(char::is_whitespace).map(|space| head[..space].trim_end()) {
        Some(kept) if !kept.is_empty() => format!("{kept}..."),
        _ => line.to_string(),
    }
}

/// What a line costs in the budget: its tokens and its line break.
fn cost(line: &str) -> usize {
    est_tokens(line) + 1
}

/// What the lines of a section cost.
fn cost_of(lines: &[String]) -> usize {
    lines.iter().map(|l| cost(l)).sum()
}

/// Keeps whole groups of lines while they fit in `tokens`. When even the first group is
/// too long, its first line is kept as far as `tokens` allow (four bytes to a token, as
/// `est_tokens` counts them), up to the end of a word, and the section holds nothing else. A line
/// of which not even the first word fits is left out.
fn fit_groups(groups: Vec<Vec<String>>, tokens: usize) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0usize;
    for group in groups {
        let cost: usize = group.iter().map(|l| est_tokens(l) + 1).sum();
        if kept.is_empty() {
            if cost > tokens {
                let room = tokens.saturating_mul(4);
                kept.extend(group.into_iter().take(1).map(|l| cut_words(l, room)).filter(|l| !l.is_empty()));
                break;
            }
        } else if used + cost > tokens {
            break;
        }
        used += cost;
        kept.extend(group);
    }
    kept
}

/// Adds a section to the answer when it has lines.
fn section(out: &mut String, title: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    ln!(out, "== {title}");
    for line in lines {
        ln!(out, "{line}");
    }
}

/// True for a character a code name is made of, or of a path that leads to one.
fn name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | ':' | '/')
}

/// The name without the punctuation at its ends: what is not a letter, a digit or an underscore.
fn trim_punctuation(raw: &str) -> &str {
    raw.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_'))
}

/// True for a word that looks like a name from code: it has an underscore, a dot or `::`, or a
/// capital letter after its first character (`fooBar`, `FooBar`).
fn looks_like_code(word: &str) -> bool {
    word.contains(['_', '.']) || word.contains("::") || word.chars().skip(1).any(char::is_uppercase)
}

/// The names found in a task text: in the order they first appear, each once, and no more than
/// `MAX_LOOKUPS`, since no more are looked up.
#[derive(Default)]
struct Names {
    list: Vec<String>,
    seen: HashSet<String>,
}

impl Names {
    fn full(&self) -> bool {
        self.list.len() >= MAX_LOOKUPS
    }

    /// Adds a name, without the punctuation at its ends, unless it is empty, known or one too many.
    fn push(&mut self, raw: &str) {
        let name = trim_punctuation(raw);
        if !self.full() && !name.is_empty() && self.seen.insert(name.to_string()) {
            self.list.push(name.to_string());
        }
    }
}

/// The code names in a task text, in the order they first appear, each once, at most
/// `MAX_LOOKUPS`. A name is a run of text between backticks; or a word that looks like code (see
/// `looks_like_code`); or a word directly followed by `(`. The punctuation at the ends of a name
/// is dropped.
fn code_names(text: &str) -> Vec<String> {
    let mut names = Names::default();
    let mut at = 0;
    while at < text.len() && !names.full() {
        let Some(c) = text[at..].chars().next() else { break };
        if c == '`' {
            if let Some(length) = text[at + 1..].find('`') {
                let run = &text[at + 1..at + 1 + length];
                // `name(args)` names the function and not its arguments.
                names.push(run.split('(').next().unwrap_or(run));
                at += length + 2;
                continue;
            }
        } else if name_char(c) {
            let rest = &text[at..];
            let length = rest.find(|ch: char| !name_char(ch)).unwrap_or(rest.len());
            let word = &rest[..length];
            if rest[length..].starts_with('(') || looks_like_code(trim_punctuation(word)) {
                names.push(word);
            }
            at += length;
            continue;
        }
        at += c.len_utf8();
    }
    names.list
}

/// The block `pgr about` prints for one definition a task text names.
struct Block {
    node: u32,
    /// Path, first line and last line of the definition: what a search hit is compared by.
    span: (String, u32, u32),
    lines: Vec<String>,
}

/// The blocks of the first definitions the task text names. A name counts when `Graph::find` gives
/// exactly one definition for it, and a definition counts once however it was spelled. A word with
/// a `/` and no `::` that names no definition is two names written together
/// (`load_config/parse_text`): its parts are tried in its place. At most `MAX_LOOKUPS` names are
/// looked up, the parts included.
fn named_blocks(graph: &Graph, task: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut queue: VecDeque<String> = code_names(task).into();
    let mut lookups = 0;
    while let Some(name) = queue.pop_front() {
        if blocks.len() == MAX_NAMES || lookups == MAX_LOOKUPS {
            break;
        }
        lookups += 1;
        let found = graph.find(&name);
        if found.is_empty() && name.contains('/') && !name.contains("::") {
            for part in name.split('/').rev().map(trim_punctuation).filter(|part| !part.is_empty()) {
                queue.push_front(part.to_string());
            }
            continue;
        }
        let [node] = found[..] else { continue };
        if blocks.iter().any(|b| b.node == node) {
            continue;
        }
        let mut text = String::new();
        graphcmd::about(graph, std::slice::from_ref(&name), &mut text);
        let definition = &graph.nodes[node as usize];
        blocks.push(Block {
            node,
            span: (graph.path_of(node).to_string(), definition.start, definition.end),
            lines: text.lines().map(whole_words).collect(),
        });
    }
    blocks
}

fn more_lines(count: usize) -> String {
    format!("  +{count} more lines")
}

/// Cuts the blocks to `tokens`. A block that does not fit whole is cut at a line end and followed
/// by `  +K more lines`, and it is the last one. Gives the lines and the number of blocks that
/// have at least one line in them.
fn fit_blocks(blocks: &[Block], tokens: usize) -> (Vec<String>, usize) {
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0usize;
    let mut shown = 0usize;
    for block in blocks {
        let price = cost_of(&block.lines);
        if used + price <= tokens {
            used += price;
            lines.extend(block.lines.iter().cloned());
            shown += 1;
            continue;
        }
        let total = block.lines.len();
        let mut kept = 0usize;
        let mut taken = used;
        for (i, line) in block.lines.iter().enumerate() {
            let left = total - i - 1;
            let reserve = if left > 0 { cost(&more_lines(left)) } else { 0 };
            if taken + cost(line) + reserve > tokens {
                break;
            }
            taken += cost(line);
            kept += 1;
        }
        if kept > 0 {
            lines.extend(block.lines[..kept].iter().cloned());
            lines.push(more_lines(total - kept));
            shown += 1;
        }
        break;
    }
    (lines, shown)
}

/// Answers a task description. `for_task` is the task whose notes go into the answer; with none,
/// the notes are those the task text recalls.
pub fn run(ctx: &mut Ctx, task: &str, budget: usize, for_task: Option<&str>) -> Res<String> {
    let for_task = ctx.task(for_task)?;
    // The answer is the same for every caller, but a role name is refused wherever it comes from.
    ctx.role(None)?;
    let budget = hub::floor_budget(budget);
    let graph_on = ctx.engines.is_on(Engine::Graph);
    let search_on = ctx.engines.is_on(Engine::Search);
    if graph_on || search_on {
        ctx.need(Needs { graph: graph_on, search: search_on, embed: false })?;
    }
    let memory_on = ctx.engines.is_on(Engine::Memory)
        && match for_task {
            Some(_) => ctx.bank_dir.is_dir(),
            None => ctx.bank_has_notes(),
        };

    let mut out = String::new();
    let mut left = budget;
    let mut shown: Vec<Block> = Vec::new();
    if graph_on {
        let named = named_blocks(ctx.graph()?, task);
        let (lines, count) = fit_blocks(&named, budget / 2);
        left = left.saturating_sub(cost_of(&lines));
        section(&mut out, "names", &lines);
        shown = named;
        shown.truncate(count);
    }

    // The sections that are on share what is left equally, in order.
    let mut waiting = [search_on, graph_on, memory_on].into_iter().filter(|on| *on).count();
    if search_on {
        let share = left / waiting;
        waiting -= 1;
        let args = FindArgs { query: task.to_string(), k: HITS_ASKED, mode: Mode::Hybrid };
        let hits = findcmd::hits(ctx, &args)?;
        let groups = hits
            .iter()
            .filter(|hit| !shown.iter().any(|b| b.span.0 == hit.path && b.span.1 == hit.start && b.span.2 == hit.end))
            .map(findcmd::hit_lines)
            .collect();
        let lines = fit_groups(groups, share);
        left = left.saturating_sub(cost_of(&lines));
        section(&mut out, "find", &lines);
    }
    if graph_on {
        let share = left / waiting;
        waiting -= 1;
        let graph = ctx.graph()?;
        let answer = graph.query(task, share);
        if !answer.nodes.is_empty() {
            let groups = graphcmd::query_lines(graph, &answer).into_iter().map(|l| vec![l]).collect();
            let lines = fit_groups(groups, share);
            left = left.saturating_sub(cost_of(&lines));
            section(&mut out, "graph", &lines);
        }
    }
    if memory_on {
        let share = left / waiting;
        let lines = match for_task {
            // The layers of a loadout for the task, without its closing lines.
            Some(name) => hub::load(ctx, &Want { task: Some(name.as_str()), query: "", budget: share, related: false })?.lines,
            None => {
                let notes = memcmd::relevant_notes(ctx, task, NOTES_ASKED, share)?;
                let now = ctx.now();
                let groups = notes.iter().map(|n| vec![memcmd::note_line(n, now)]).collect();
                fit_groups(groups, share)
            }
        };
        section(&mut out, "memory", &lines);
    }
    if out.is_empty() {
        ln!(out, "nothing found for this task");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_are_kept_whole_while_they_fit() {
        let groups = vec![vec!["a".repeat(40), "b".repeat(40)], vec!["c".repeat(40)], vec!["d".repeat(40)]];
        let kept = fit_groups(groups, 40);
        assert_eq!(kept.len(), 3);
        assert!(kept[2].starts_with('c'));
    }

    #[test]
    fn a_cut_line_is_cut_in_bytes_and_not_in_characters() {
        let kept = fit_groups(vec![vec!["数数数 ".repeat(40)]], 10);
        assert_eq!(kept.len(), 1);
        assert!(est_tokens(&kept[0]) <= 10, "{} tokens", est_tokens(&kept[0]));
        assert!(kept[0].split(' ').all(|word| word == "数数数"), "{:?}", kept[0]);
    }

    #[test]
    fn a_one_token_share_keeps_at_most_four_bytes_and_only_whole_words() {
        let kept = fit_groups(vec![vec!["ab cdefgh".to_string()]], 1);
        assert_eq!(kept, ["ab"]);
        // A first word that does not fit leaves nothing.
        assert!(fit_groups(vec![vec!["x".repeat(100)]], 1).is_empty());
    }

    #[test]
    fn the_first_group_is_cut_when_it_alone_is_too_long() {
        let kept = fit_groups(vec![vec!["xxxxxxx ".repeat(60), "y".repeat(10)]], 10);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].len(), 39);
        assert!(!kept[0].ends_with(' '));
    }

    #[test]
    fn a_line_is_never_cut_inside_a_word() {
        let line = "alpha beta gamma delta epsilon".to_string();
        for bytes in 0..=line.len() {
            let cut = cut_words(line.clone(), bytes);
            assert!(cut.len() <= bytes, "{bytes}: {cut:?}");
            assert!(cut.is_empty() || line.split(' ').any(|word| cut.split(' ').last() == Some(word)), "{bytes}: {cut:?}");
        }
        assert_eq!(cut_words(line.clone(), 10), "alpha beta");
        assert_eq!(cut_words(line.clone(), 11), "alpha beta");
        assert_eq!(cut_words(line.clone(), 4), "");
        assert_eq!(cut_words(line.clone(), 5), "alpha");
    }

    #[test]
    fn a_line_the_index_cut_inside_a_word_goes_back_to_the_word_before() {
        // Half a word goes, whatever stands before it.
        assert_eq!(whole_words("f app/a.py:1-2 (alpha, beta, gam..."), "f app/a.py:1-2 (alpha, beta,...");
        assert_eq!(whole_words("  Return the first configured API k..."), "  Return the first configured API...");
        // A cut that left a word, a mark or a space behind it is whole, and so is a line never cut.
        assert_eq!(whole_words("f app/a.py:1-2 (alpha, beta,..."), "f app/a.py:1-2 (alpha, beta,...");
        assert_eq!(whole_words("f app/a.py:1-2 (alpha, ..."), "f app/a.py:1-2 (alpha, ...");
        assert_eq!(whole_words("  0 callers"), "  0 callers");
        // A line with no word before the cut keeps what it has.
        assert_eq!(whole_words("  conf..."), "  conf...");
        assert_eq!(whole_words("conf..."), "conf...");
    }

    #[test]
    fn at_most_64_distinct_names_are_kept_in_the_order_they_appear() {
        let text = (0..1000).map(|i| format!("name_{i}")).collect::<Vec<_>>().join(" ");
        let names = code_names(&text);
        assert_eq!(names.len(), MAX_LOOKUPS);
        assert_eq!((names[0].as_str(), names[63].as_str()), ("name_0", "name_63"));
        // A name written again is not another name, so it takes no place from the others.
        let again = (0..200).map(|i| format!("name_{} name_{}", i % 10, i)).collect::<Vec<_>>().join(" ");
        assert_eq!(code_names(&again).len(), MAX_LOOKUPS);
    }

    #[test]
    fn backticked_runs_are_names_whatever_they_look_like() {
        assert_eq!(code_names("change `Store` and `reload` in `app/core.py::load_config`"), ["Store", "reload", "app/core.py::load_config"]);
        assert_eq!(code_names("call `load_config(path, 3)` and `Store.reload()` now"), ["load_config", "Store.reload"]);
        assert!(code_names("a lone ` backtick, never closed").is_empty());
    }

    #[test]
    fn words_with_an_underscore_a_dot_a_double_colon_or_an_inner_capital_are_names() {
        assert_eq!(code_names("fix read_text, then Store.get; see app::main and fooBar and FooBar."), ["read_text", "Store.get", "app::main", "fooBar", "FooBar"]);
        // A word that only starts with a capital, and plain words, are not.
        assert!(code_names("Fix the Store class and read the file").is_empty());
    }

    #[test]
    fn a_word_directly_followed_by_a_bracket_is_a_name_and_one_after_a_space_is_not() {
        assert_eq!(code_names("make slugify(text) faster, and reload (twice)"), ["slugify"]);
        assert_eq!(code_names("(see load_config)."), ["load_config"]);
    }

    #[test]
    fn punctuation_at_the_ends_is_dropped_and_each_name_comes_once_in_the_order_of_first_appearance() {
        assert_eq!(code_names("read_text, parse_text. read_text! (parse_text) `read_text`"), ["read_text", "parse_text"]);
        assert_eq!(code_names("see core.py."), ["core.py"]);
        assert!(code_names("").is_empty() && code_names("no names here at all").is_empty());
    }

    fn block(node: u32, lines: usize) -> Block {
        Block { node, span: ("p.py".to_string(), 1, 2), lines: (0..lines).map(|i| format!("line {i} of block {node}")).collect() }
    }

    #[test]
    fn blocks_that_fit_are_kept_whole() {
        let blocks = [block(1, 3), block(2, 2)];
        let (lines, shown) = fit_blocks(&blocks, 1000);
        assert_eq!((lines.len(), shown), (5, 2));
    }

    #[test]
    fn a_block_that_does_not_fit_is_cut_at_a_line_end_and_counts_the_lines_left_out() {
        let blocks = [block(1, 2), block(2, 5), block(3, 2)];
        let each = cost("line 0 of block 2");
        let tokens = cost_of(&blocks[0].lines) + 2 * each + cost(&more_lines(3));
        let (lines, shown) = fit_blocks(&blocks, tokens);
        assert_eq!(shown, 2);
        assert_eq!(&lines[..2], &blocks[0].lines[..]);
        assert_eq!(&lines[2..4], &blocks[1].lines[..2]);
        assert_eq!(lines[4], "  +3 more lines");
        assert_eq!(lines.len(), 5, "the block after the cut one is not started");
        assert!(cost_of(&lines) <= tokens);
    }

    #[test]
    fn nothing_is_shown_when_not_even_one_line_of_the_first_block_fits() {
        let (lines, shown) = fit_blocks(&[block(1, 3)], 2);
        assert!(lines.is_empty());
        assert_eq!(shown, 0);
    }
}
