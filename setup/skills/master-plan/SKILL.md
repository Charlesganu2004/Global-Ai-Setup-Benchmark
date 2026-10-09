---
name: master-plan
description: Plan work that has more than a couple of dependent steps before doing any of it, and keep the plan true as the work changes it. Use for implementation, investigation, migration, refactoring and design tasks, and whenever a request has several parts. Covers what a plan must contain, how to slice work, when to parallelise, the stopping condition, and when NOT to plan at all.
---

# Plan

A plan is the cheapest place to find out a task is bigger, different, or
impossible. Every mistake caught in the plan costs a sentence; the same mistake
caught after the code is written costs the code. But a plan is also the easiest
thing to perform rather than use, so this skill is as much about when to skip it
as how to write it.

## When to plan, and when not to

    one step, obvious          do it. A plan here is ceremony.
    a few independent steps    a one-line list is enough
    dependent steps            write the plan below
    unclear what "done" means  plan, and put the definition of done first
    touches shared state,      plan, and read master-architect first: the
    public interfaces, data    shape is the expensive decision

The test: if doing it in the wrong order would waste work, plan.

## What the plan contains

1. **The read.** One or two sentences restating the task in your own words, and
   what you understood the person to want. Most wasted work traces back to a
   misread that a restatement would have exposed.
2. **Definition of done.** Observable, checkable: "the suite passes and the new
   command prints X", not "it works".
3. **Constraints.** What must not change, what is out of scope, what the user
   ruled out, limits on time or tokens (a `/token limit` is a constraint).
4. **Evidence to gather first.** The files, commands and facts you need before
   deciding anything. Retrieve them; do not plan on assumptions about code you
   have not read.
5. **Slices.** The work, cut so each slice can be verified on its own. Ordered by
   dependency. Name which slices are independent.
6. **Verification gate.** For each slice, the check that proves it. For the
   whole, the check that proves done.
7. **Stopping condition.** What finished looks like, and what "blocked" looks
   like. A plan with no stopping condition expands until the budget runs out.

## Slicing

Cut vertically, through the whole path, rather than horizontally by layer. A
thin slice that works end to end teaches more than a complete layer nothing uses
yet, and it is verifiable the moment it exists.

Each slice should be small enough that when its check fails, you know where to
look. If a slice's check can only be run after three other slices, it is too
big or in the wrong order.

Put the riskiest slice early. The part most likely to reveal that the plan is
wrong should run before the parts that assume it is right.

## Parallel work

Slices with no dependency between them can run at once: several files read in
one step, several independent checks together, independent pieces of work
handed to separate agents when the task is large. Verify parallel results
independently rather than trusting the first pass; two things done at once are
two things to check.

Do not parallelise slices that write the same files. That is a conflict waiting
to happen, and resolving it costs more than the parallelism saved.

## Keeping the plan true

A plan is a working document, not a promise. When you discover something that
changes it, change it, and say so. The two failure modes:

- **Plan drift:** the work quietly diverged and the plan still describes the old
  approach. Anyone reading the plan is now misled.
- **Plan loyalty:** evidence shows the plan is wrong and you follow it anyway
  because it was written down.

When a slice turns out much larger than expected, stop and re-plan rather than
pushing through. When a requirement turns out impossible, report it as blocked
with the blocker named; never narrow the scope silently.

## Writing it down

For short work, a few lines in the answer. For long work, a checklist you update
as slices complete, so the state is visible to the person and to you after a
long session has pushed the start of it out of reach.

    Read: add a /token limit command that the proxy enforces as max_tokens.
    Done: `/token limit 800` in a chat request sets max_tokens=800; tests pass.
    Constraints: never raise a caller's smaller limit; OpenAI rejects unknown
      fields, so no Ollama-only keys on OpenAI paths.
    Slices:
      1. parser + tests (independent)
      2. store per session + tests (after 1)
      3. proxy cap keyed on API path + tests (after 1)
      4. docs and the injected rule text (after 2, 3)
    Gate: full suite; a request through the proxy shows the capped field.
    Stop: all four green, or a named blocker.

## Before acting on it

Read the plan once as the person who asked would. Does the definition of done
match what they asked for, all of it? Is anything they asked for missing from
the slices? That comparison, done now, is the one that saves the most.

Related: `master-architect` for deciding the shape before sequencing it,
`master-review` for checking the result against the request, `scope-guard` for
the requirement that is ambiguous or quietly growing.
