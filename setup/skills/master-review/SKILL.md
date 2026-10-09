---
name: master-review
description: Read finished work the way an adversary would, before handing it over. Use at the end of every substantive turn, and on any diff, document, design or answer about to be shipped. Looks for the claim with no evidence, the case that was never run, the requirement that quietly shrank, and the thing that is present but does not work.
---

# Review

The review pass is the one that gets skipped, because by the time you reach it
you already believe the work is right. That belief is the reason it is needed:
the person best placed to find the defect is the one who just wrote it, and the
one least motivated to look is the same person.

So this is not "read it over". It is a set of specific questions with specific
answers, run against the artifact rather than against your memory of writing it.

## Read the request again first, not the work

Open the original words. Not the summary you have been carrying, which has been
compressing since the first turn.

    every deliverable named        counted, and each one found in the output
    every constraint stated        still true of what was produced
    anything that quietly shrank   named out loud, not left for them to notice

A requirement that was hard and got dropped is the single most common failure of
a long turn, and it never announces itself. It just stops appearing.

## Then read the work as an adversary

For each claim in the output, ask what would have to be true, and whether it was
checked or assumed.

    "the tests pass"        which run, what count, was the new test among them
    "it works"              the specific case, executed, output quoted
    "no other usages"       the search, and its result
    "removed all X"         a search proving zero remain
    "faster" or "smaller"   two measurements, before and after

An unevidenced claim in the output is a defect in the output, whatever the code
does. Downgrade it to what you actually know, or go and get the evidence.

## The failure modes worth checking by name

**Present but inert.** The element renders, the class exists, the count is
right, and it does nothing. Counting cannot distinguish it from working. In this
repository a collapsed scene once painted over an entire page: every element was
present, every count passed, nothing was clickable. Check the behaviour, not the
existence.

**The vacuous pass.** Zero tests collected. A filter matching nothing. A skipped
suite. `0 passed, 0 failed` exits 0 and proves nothing.

**Encoded in the test.** The check was written from the same wrong assumption as
the code, so it agrees. If a test never failed, you do not know it can. Break the
code on purpose once and watch it go red.

**Verified against the wrong thing.** An offline check that validates a file
against itself cannot catch a rule the server applies. An index checked only
against the file it was written in still gets rejected at creation.

**The stale restatement.** Prose describing a rule, count or shape that lives
somewhere else, drifting quietly the moment the source changes. Anything
describing a number should read it.

**Fixed twice, in two places.** The same defect corrected in two files is
duplication that costs something. Say so, even if consolidating is not this
turn's work.

## Then check what is missing rather than what is wrong

Wrongness announces itself. Absence does not.

    the error path         what happens when it fails, not only when it works
    the empty case         zero items, no data, first run
    the boundary           the first, the last, the one over the limit
    the second caller      it works for the one you wrote it for
    the person who is not you  can they use it without this conversation

## Report the failure first

If something did not work, that is the first line, not a note after a paragraph
of what did. A failure buried at the end of a success narrative has been hidden
whether or not that was the intent.

Where the work is genuinely done and checked, say so plainly. Hedging finished
work is its own kind of dishonesty, and it trains the reader to ignore the
hedges that matter.

## When to stop reviewing

One pass, done properly, against the artifact. A second pass over your own
reasoning finds nothing the first did not, because it is the same reasoning. If
the first pass found something, fix it and re-run the check that would catch it,
rather than re-reading.

Related: `verify-before-complete` is the evidence rule this applies at the end,
`self-consistency-check` covers answers that must agree with themselves,
`master-refactor` for structure the review found hard to read, and `scope-guard`
for the requirement that shrank.
