---
name: master-full-output
description: Deliver every requested piece completely, with no placeholders, skeletons, elided sections or "similar to above". Use whenever an implementation, file, document, list or set of deliverables is requested, and whenever an answer is long enough that the temptation to abbreviate appears. Covers counting deliverables, the banned shortcuts, what to do when genuinely out of room, and how to check the count before sending.
---

# Full output

A partial output is a broken output. If five files were asked for, five files
arrive, each whole. If a function was asked for, it runs. The failure this skill
exists to stop is not laziness in the ordinary sense; it is the reasonable-looking
abbreviation that saves the writer effort and leaves the reader with something
that does not work, and does not say so.

The idea comes from Leonxlnx/taste-skill (MIT), whose output skill is the
original; this is our statement of it with the house rules added.
`repo-lists/skill-upstreams.json` records the version it was checked against.

## Count first, before writing anything

Read the whole request and write down, at least to yourself, the number of
distinct deliverables: files, functions, sections, answers, fixes, items in a
list. Lock that number. The most common failure of a long turn is a deliverable
that was in the request and quietly never appears, and it never announces
itself. It is only caught by counting.

When a request has several parts joined by "also" and "and then", each part is a
deliverable. When it names a set ("all the designs", "every harness"), the
deliverable is every member of the set, and you enumerate them rather than
trusting a pattern.

## The banned shortcuts

Each of these is a hard failure, however natural it looks at the time.

In code:

    // ...                       // rest of the code
    // implement here            // TODO: finish
    /* same as above */          // similar for the remaining cases
    pass                         # when behaviour was asked for
    ...                          # standing in for omitted code
    raise NotImplementedError    # in a function that was asked for

In prose:

    "and so on" replacing actual content
    "the rest follows the same pattern"
    "for brevity"
    "I'll leave that as an exercise"
    "let me know if you want me to continue"

Structurally:

- a skeleton or outline where an implementation was asked for
- the first and last sections written and the middle summarised
- one example written out and the others described
- a description of what the code should do in place of the code
- a stubbed handler, an empty test, a decorative shell with no behaviour

A test that only matches wording rather than checking behaviour counts as a stub.

## Preserve what you did not touch

When editing an existing file, everything outside the change stays exactly as it
was. Do not "tidy" unrelated code on the way past, do not drop comments you did
not write, do not reformat a region you were not asked to reformat. Rewriting a
whole file to change one function is how unrelated behaviour gets lost.

## Genuinely out of room

Sometimes a response hits a real limit: a token cap the user set with
`/token limit`, a tool output limit, a context window. Then:

1. Do not compress the remaining sections to squeeze them in. Quality drops
   exactly where nobody checks.
2. Do not skip to the conclusion.
3. Finish at a clean boundary: the end of a function, a file, a section.
4. End with a precise statement of what remains, by name:

       Stopped at a clean break: 3 of 5 files written (a.py, b.py, c.py).
       Remaining: d.py (the CLI), e.py (its tests). Ask for "continue" to resume.

5. On "continue", resume exactly where you stopped. No recap, no rewrite of what
   was already delivered.

A named remainder is honest. A silent one is the failure this skill exists for.

## Full output is not the same as long

Completeness is about the deliverables, not the word count. A complete answer to
a one-line question is one line. Full output never licenses padding: the caveman
rules still apply to the prose around the work. What it forbids is omitting work,
not trimming words.

## Before sending

- Count the deliverables in the output. Compare with the number you locked.
- Search your own output for the banned patterns above. `...` and "TODO" are
  worth an actual search, not a glance.
- For code: does it run? If there is a check, run it and quote the result.
- For every item you could not finish: is it named, with the reason?

If the count is short, add what is missing before sending, or state exactly
which item is missing and why.

Related: `master-caveman` for trimming words without trimming content,
`master-review` for counting deliverables against the original request,
`verify-before-complete` for running the check rather than inferring it.
