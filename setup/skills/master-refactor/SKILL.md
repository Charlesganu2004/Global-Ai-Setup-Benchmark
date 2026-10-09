---
name: master-refactor
description: Improve the structure of existing code without changing what it does. Use when code is hard to read, duplicated, over-nested, badly named, or about to receive a feature it cannot cleanly hold. Carries the behaviour-preservation rule, the named transformation list, and the test gate that separates a refactor from a rewrite.
---

# Refactor

A refactor is a change to the inside of code that leaves the outside identical.
That definition is the whole discipline, and it is the part most often lost:
the moment a behaviour changes, the work stops being a refactor and becomes an
edit that nobody can review, because the diff no longer answers the only
question a reviewer has, which is whether anything moved.

The technique draws on github/awesome-copilot (MIT), whose refactor skill is the
original; this is our own statement of it. `repo-lists/skill-upstreams.json`
records the version checked against, and `scripts/skill_upstreams.py --check`
reports when upstream moves.

So there are two rules that come before the transformation list.

## Tests green before, tests green after

Green before the first change, and green again after each one. Not green at the
end of the session, after eleven transformations landed together and one of
them broke something. The value of the small step is that when the suite goes
red you already know which change did it.

No suite over the code being changed means no refactor yet. Write the
characterisation tests first: call the function with the inputs it actually
receives, record what it currently returns, and assert that. Those tests do not
document what the code should do. They document what it does, which is the
thing a refactor must not alter.

If tests cannot be written, say so and stop. A refactor without a test is a
rewrite with optimism attached.

## One transformation at a time, and never mixed with a feature

Extract the method, run the tests, commit. Then rename, run the tests, commit.
A commit holding a rename and a bug fix cannot be reverted without losing one
of them, and the reviewer reading it has to hold both in their head to check
either.

If a bug surfaces mid-refactor, stop, note it, finish or revert the current
transformation, then fix the bug as its own change. Fixing it in place feels
faster and costs more.

## The smells, and what each one is asking for

A smell is a reason to look, never on its own a reason to change. Code that is
long and clear beats code that is short and clever.

    long function              Extract Method, on the block that has a name
    deep nesting               Simplify Conditional; guard clauses, early return
    duplication in 3+ places   Extract Method, then call it from each site
    the same triple of args    Introduce Parameter Object
    a bare 86400 or 0.8        Replace Magic Number with Constant
    a name that lies           Rename Symbol, and rename every caller with it
    a variable used once       Inline Variable
    a method on the wrong type Move Method
    a branch nothing reaches   Remove Dead Code, once a search proves it dead
    behaviour split by a flag  Replace Conditional with Polymorphism, or a
                               strategy, if the branches keep growing

Duplication of two lines twice is not a smell. Extracting it produces a
function whose name is longer than its body and a call site that now hides what
it does. Three occurrences, or two that must change together, is the threshold.

## Remove Dead Code needs proof, not confidence

`grep` for the symbol across the whole repository, including tests, configs,
templates, string-keyed dispatch tables and dynamic imports. Quote the search
that returned nothing. A symbol reached by name at runtime is invisible to a
call-graph tool and to a reading of the file, and deleting it fails in
production rather than in CI.

## The order, when several apply

1. Rename first. It is the safest, and it makes the rest readable.
2. Then extract, working outward-in: the innermost coherent block first.
3. Then move, once the pieces have names that say where they belong.
4. Then simplify the conditionals the extraction exposed.
5. Then delete what the previous four proved unreachable.

Doing this in reverse order deletes code you would have kept had you read the
extracted version of it.

## Verify

Run the suite and quote the count. Then check the three things a green suite
does not cover:

- The public surface is unchanged: same exported names, same signatures, same
  return shapes, same raised types.
- The diff contains no behaviour change you did not name. Read it once as a
  reviewer would.
- Performance did not regress where it was the reason the ugly version existed.
  A hand-rolled loop replaced by a clean comprehension over the same 4 million
  rows is a refactor that shipped a problem.

## What this skill does not cover

Restructuring that intentionally changes behaviour, changes an API, or moves
work between processes is a redesign. It gets a plan, a migration path and its
own review, not this skill's small-step loop. Say which one you are doing
before the first edit.

Related: `master-refactor-ui` for the same discipline applied to a surface a
person looks at, `verify-before-complete` for the evidence rule the test gate
above is one instance of, and `scope-guard` for the boundary between the
refactor asked for and the rewrite that was not.
