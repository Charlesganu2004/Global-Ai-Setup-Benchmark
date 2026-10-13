//! `pgr pack "TASK"`: one answer for a task, made of the best search hits, a walk of the
//! graph seeded by the task text and the notes that match it. Each section stays within
//! a third of the budget and is left out when it is empty.

use crate::cli::FindArgs;
use crate::ctx::{Ctx, Engine, Needs};
use crate::fail::Res;
use crate::{findcmd, graphcmd, memcmd};
use lxr_core::est_tokens;
use lxr_rag::Mode;

/// Hits asked of the search before they are cut to the section's share.
const HITS_ASKED: usize = 8;
/// Notes asked of the bank before they are cut to the section's share.
const NOTES_ASKED: usize = 6;

/// Cuts a line to at most `bytes` bytes of UTF-8, at a character boundary.
fn cut_bytes(line: String, bytes: usize) -> String {
    if line.len() <= bytes {
        return line;
    }
    let mut at = bytes;
    while !line.is_char_boundary(at) {
        at -= 1;
    }
    line[..at].to_string()
}

/// Keeps whole groups of lines while they fit in `tokens`. When even the first group is
/// too long, its first line is kept as far as `tokens` allow (four bytes to a token, as
/// `est_tokens` counts them) and the section holds nothing else. A line of which not even one
/// character fits is left out.
fn fit_groups(groups: Vec<Vec<String>>, tokens: usize) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0usize;
    for group in groups {
        let cost: usize = group.iter().map(|l| est_tokens(l) + 1).sum();
        if kept.is_empty() {
            if cost > tokens {
                let room = tokens.saturating_mul(4);
                kept.extend(group.into_iter().take(1).map(|l| cut_bytes(l, room)).filter(|l| !l.is_empty()));
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

/// Answers a task description.
pub fn run(ctx: &mut Ctx, task: &str, budget: usize) -> Res<String> {
    let share = (budget / 3).max(1);
    let mut out = String::new();
    let needs = Needs { graph: ctx.engines.is_on(Engine::Graph), search: ctx.engines.is_on(Engine::Search), embed: false };
    if needs.graph || needs.search {
        ctx.need(needs)?;
    }
    if needs.search {
        let args = FindArgs { query: task.to_string(), k: HITS_ASKED, mode: Mode::Hybrid };
        let groups = findcmd::hits(ctx, &args)?.iter().map(findcmd::hit_lines).collect();
        section(&mut out, "find", &fit_groups(groups, share));
    }
    if needs.graph {
        let graph = ctx.graph()?;
        let answer = graph.query(task, share);
        if !answer.nodes.is_empty() {
            let groups = graphcmd::query_lines(graph, &answer).into_iter().map(|l| vec![l]).collect();
            section(&mut out, "graph", &fit_groups(groups, share));
        }
    }
    if ctx.engines.is_on(Engine::Memory) && ctx.bank_has_notes() {
        let notes = memcmd::relevant_notes(ctx, task, NOTES_ASKED, share)?;
        let now = ctx.now();
        let groups = notes.iter().map(|n| vec![memcmd::note_line(n, now)]).collect();
        section(&mut out, "memory", &fit_groups(groups, share));
    }
    if out.is_empty() {
        ln!(out, "none");
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
        let kept = fit_groups(vec![vec!["数".repeat(100)]], 10);
        assert_eq!(kept.len(), 1);
        assert!(est_tokens(&kept[0]) <= 10, "{} tokens", est_tokens(&kept[0]));
    }

    #[test]
    fn a_one_token_share_keeps_at_most_four_bytes() {
        let kept = fit_groups(vec![vec!["x".repeat(100)]], 1);
        assert!(kept.iter().all(|l| l.len() <= 4), "{kept:?}");
    }

    #[test]
    fn the_first_group_is_cut_when_it_alone_is_too_long() {
        let kept = fit_groups(vec![vec!["x".repeat(500), "y".repeat(10)]], 10);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].len(), 40);
    }
}
