---
name: master-token-reducer
description: Spend tokens on the part of a large input that answers the question, and nothing else. Use before reading a big repository, document, transcript, catalog or log into context, and throughout any long task, not only at the end. Covers narrowing the question, building a retrieval packet, choosing the right tool for the source, measuring the reduction, and what must never be reduced.
---

# Token reducer

Compressing a finished answer is the least valuable place to save tokens: by
then the tokens were already spent getting there. The real savings are upstream,
in what gets loaded in the first place. This skill is about loading less, on
purpose, while keeping everything the answer depends on.

It runs throughout a task, which is why the super harness lists it as a pass that
applies "throughout, not at the end".

## 1. Narrow the question before touching the source

Write the question the source has to answer, as specifically as possible.
"How does auth work" loads the whole auth module. "Where is the session token
validated, and what happens when it has expired" loads two functions. The
narrower question is usually also the one the person actually needed answered.

## 2. Locate before you load

Find where the answer lives, then read only that.

    a symbol or string       search for it (grep, ripgrep) and read the hits
    a file's structure       list or outline it; read the relevant section
    a big log                filter to the error lines and a few around them
    a catalog                retrieve the matching entries, never the whole list
    a long document          read the table of contents, then the section
    a transcript             search for the moment, not the whole conversation

Read a section with a line range rather than the whole file when you know where
the answer is. Open a second file only when the first one sends you there.

## 3. Build a retrieval packet

For a question that spans several places, assemble a compact packet before
reasoning: the handful of excerpts that matter, each labelled with its path and
line range so it can be cited and re-opened. When the local Token Reducer tool is
installed, use its hash backend with a small top-k and a word budget, then open
only the cited chunks. When it cannot parse the input, fall back to focused
searches and bounded sections. Never fall back to dumping the source.

A good packet is small enough to reason over and complete enough that you do not
need to go back for the piece you left out. If you keep going back for more, the
question was too broad; narrow it.

## 4. Route noisy commands

Command output is a large, low-density source. Route supported commands through
rtk when it is present, which filters output before it reaches context. Ask for
the failing lines, not the full run: quiet flags, `--tb=short`, `| tail -n 20`
on a known-long output, a summary line rather than every passing test.

Never paste a whole log, file tree or catalog into an answer. Quote the lines
that matter.

## 5. Measure the reduction

When you compress or reduce deliberately, measure it: bytes or tokens before and
after. A reduction you did not measure is a guess, and "I reduced the context"
with no number is not a claim anyone can check. For a compression pass on a
file, under 15 percent saved means the pass failed; keep the original.

## 6. What is never reduced

Byte for byte, always: code, commands, URLs, file paths, versions, dates,
numbers, headings and exact error strings. A reduction that changes one of these
has not saved tokens, it has introduced a bug.

Never sent through any compression or reduction pass:

- skill, tool, agent, plugin and MCP server definitions, and catalog entries
- anything inside a `NO-COMPRESS:BEGIN` / `NO-COMPRESS:END` block

These are exempt everywhere. Losing a clause from a definition loses a
capability, silently.

## 7. Know when reduction is the wrong goal

Reduce context, never correctness. If narrowing the read means you might miss
the thing that matters (a security review, a migration that touches everything,
a bug whose cause is unknown), read more. The cost of a missed finding is larger
than the tokens saved. Say when you chose to read broadly and why.

## Checking yourself

Before a large read, ask: what specific question does this answer, and is there
a smaller read that answers it? After a task, ask: what did I load that I never
used? That second question is how the next task gets cheaper.

Related: `master-caveman` for fewer tokens in what you write, `retrieval-before-assert`
for retrieving what might have changed instead of recalling it, `master-full-output`
for the rule that reducing never means omitting what was asked for.
