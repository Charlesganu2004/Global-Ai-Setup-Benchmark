---
name: master-caveman
description: Cut tokens from prose without losing technical content. Use on every answer, every long session, noisy command output, and any file loaded into context repeatedly (CLAUDE.md, AGENTS.md, notes, prompts). Covers the three intensities, exactly what may be cut and what must survive byte for byte, when to switch it off for safety, and how to prove a compression pass actually saved something.
---

# Caveman

Talk less, say the same. The goal is fewer tokens for the same information,
never less information. A caveman answer that drops a fact is not compressed, it
is wrong, and it is wrong in the way that is hardest to catch because it reads
confidently.

The technique comes from JuliusBrussee/caveman (the skills there are MIT; the
engine-linked directories in that repository are BSL-1.1 and are not used here).
This skill is our own statement of it plus the house rules this repository adds.
`repo-lists/skill-upstreams.json` records the upstream version this was checked
against, and `scripts/skill_upstreams.py --check` reports when upstream moves.

## Where it applies

    your own answers        every turn, at the intensity the moment calls for
    repeatedly-loaded files CLAUDE.md, AGENTS.md, rules, notes: compressed once,
                            paid for on every session afterwards
    command output          route noisy commands through rtk when it is present;
                            never paste a whole log, catalog or file tree
    retrieval               load the matching entry, not the file it lives in

It does not apply to capability definitions or protected blocks, covered below,
and it switches off entirely for the safety cases below.

## Three intensities

Pick one per answer. Start at full and move only with a reason.

**lite.** Grammar stays; filler goes. Drop pleasantries, hedges, restatements of
the question, preambles ("Great question", "Let me explain"), closing offers
("Let me know if..."), and the sentence that repeats the heading above it. Use
for anything a newcomer reads, and for anything that will be pasted somewhere
permanent.

**full.** Fragments allowed. Articles and linking words go when meaning survives
without them. "Run tests before push. Catches regressions early." instead of two
sentences of advice. Lists replace paragraphs that enumerate. This is the
default for working answers.

**ultra.** Telegraphic. One line per fact, arrows for cause and sequence
(`X -> Y`), results before reasons, reasons only when they change what the
reader does. For status updates, diffs described in chat, and dense technical
exchanges where the reader already has context.

What ultra never does, and this is a house rule where some compressors differ:
it does not invent abbreviations. `cfg`, `impl`, `req`, `res`, `fn` look shorter
and tokenize into the same number of pieces as the full word or more, so they
save nothing and cost the reader a decode. Full words are cheaper and clearer.
Standard acronyms everyone already reads (API, DB, HTTP, CI) are fine.

## What to cut

- Articles and filler words where meaning survives: the, a, just, really,
  basically, actually, simply, essentially, quite, very.
- Hedging that carries no information: "it might be worth considering",
  "you may want to", "I would recommend". State the recommendation.
- Connective padding: however, furthermore, additionally, that said, in order to.
- Restatement: the sentence that says what the heading said, the summary that
  repeats the paragraph above it, the "In conclusion" that repeats everything.
- Duplicate examples showing the same pattern. Keep the clearest one.
- Narration of your own process ("First I will look at... now I am going to...")
  unless the reader asked to follow along.

## What must survive byte for byte

Never shortened, reworded, reformatted or paraphrased:

- code blocks and inline code
- shell commands and flags
- file paths and URLs, including link targets
- version numbers, dates, counts, measurements, money
- exact error strings and log lines being discussed
- identifiers: function, class, variable, table, branch, commit names
- headings, in compressed files, so anchors and links keep working
- quoted text the reader is expected to match or search for

If compression would require touching one of these, leave that span alone and
compress around it.

## Never compress these at all

- Skill, tool, agent, plugin and MCP server definitions, and catalog entries.
  They are exempt globally, in every conversation and command. A definition that
  loses a clause loses a capability, and nobody finds out until it matters.
- Any text between `NO-COMPRESS:BEGIN` and `NO-COMPRESS:END`. A pass that
  reaches one skips it and says so. Only Charles lifts that, in his own words;
  for hooks the phrase is APPROVED RECOMPRESS.

## When to switch it off

Write in complete, plain sentences, at whatever length clarity needs, for:

- security warnings and anything about credentials or exposure
- confirmation of a destructive or irreversible action, before and after
- step-by-step instructions where order matters and a skipped step breaks things
- legal, medical, financial or safety statements
- anything the reader will act on under stress: an outage, a failed deploy
- a reader who is visibly confused. Terse answers to confusion make it worse.

Then return to the previous intensity. Compression is a default, not a vow.

## Compressing a file (a repeatedly-loaded one)

1. Read it and record its byte size.
2. Back it up outside the tree, so a skill loader cannot re-ingest the backup as
   a live file. Refuse to overwrite an existing backup; it may be the only clean
   copy from before an earlier pass.
3. Split off YAML frontmatter and leave it untouched.
4. Compress the body using the rules above. Keep every heading's exact text.
5. Verify, and restore the backup if any check fails:
   - code blocks identical to the backup
   - headings identical
   - every URL in the backup still present
   - bytes actually dropped
6. Report original bytes, new bytes, percent saved and the backup path.

**Under 15 percent saved is a failed pass.** Restore the backup and say the file
was already dense. Moving words around and calling it compression is the thing
this rule exists to stop.

## Before and after

A working answer at full intensity:

    Before: "I've taken a look at the failing test, and it seems like the issue
    is probably that the retry helper isn't resetting its backoff counter
    between attempts, which means that after the first failure every subsequent
    retry waits longer than it should."

    After: "Failing test: retry helper never resets backoff between attempts,
    so every retry after the first waits too long."

A status update at ultra:

    Before: "I ran the full test suite and everything passed except for two
    tests in the packaging module, which are skipped because they need an
    environment variable to be set."

    After: "Suite green. 2 packaging tests skipped: need MASTER_HARNESS_BUILD_TEST=1."

A safety case, where caveman switches off:

    "This command deletes the branch on the remote as well as locally, and it
    cannot be undone from here. If anyone else has pushed to it, their work goes
    with it. Confirm the branch name before running it."

## Checking yourself

Before sending, reread the answer once and ask: did any number, name, path or
command change? Did any warning get shorter? If yes, put it back. Then ask
whether a sentence could go without the reader losing anything, and remove it.

Related: `caveman-ultra-compact` and `caveman-ultra-compact-repo` for file
passes, `master-token-reducer` for retrieval packets, `master-full-output` for
the rule that compression never becomes truncation.
