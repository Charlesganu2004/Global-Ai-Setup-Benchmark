---
name: master-anti-slop
description: Remove the fingerprints of generated work from prose, code and interfaces: filler, invented precision, generic names, unsupported claims, template structure and default visual choices. Use on every answer and every artifact before it ships, and when reviewing anything that reads as machine-made. Covers what slop looks like in each medium, the specific replacements, and a final pass checklist.
---

# Anti-slop

Slop is output shaped like an answer without the substance of one. It is not
always wrong; it is unchosen. Every default a model reaches for when it has not
made a decision leaves a mark, and readers learn to see those marks and stop
trusting everything around them.

The prose rules draw on hardikpandya/stop-slop and the interface rules on
Leonxlnx/taste-skill, both MIT; this is our own statement of them plus house
rules. `repo-lists/skill-upstreams.json` records the versions checked against.

## In prose

**No em dashes.** Use a comma, a colon, parentheses, or two sentences. This is a
house rule, not a style preference: the em dash is the most recognisable single
marker of generated text, and removing it forces the sentence to be rebuilt.

**Cut these words and phrases on sight:** delve, tapestry, testament, elevate,
seamless, robust (when it means nothing), leverage (as a verb for "use"),
unleash, game-changer, next-generation, cutting-edge, in the world of, in
today's fast-paced, it's worth noting, it is important to note, navigate the
complexities, a myriad of, plays a crucial role.

**No invented precision.** "Improves performance by 47%" needs a measurement
behind it. Without one, say what was observed or say nothing. A made-up number is
worse than no number, because it will be repeated.

**No unsupported claims.** "This is the best approach" needs a reason a reader
can check. "Industry standard" needs a standard. If you cannot support it,
downgrade it: "this avoids X, which the other approach does not".

**No template structure.** Not every answer needs a heading, an intro, three
bullets and a conclusion. Not every list has three items. Shape the answer to
the content: one sentence if one sentence answers it.

**No throat-clearing and no sign-off.** Start with the answer. End when it ends.

**No false balance.** If one option is clearly better for the stated case, say
so. "Both have pros and cons" is only true when it is useful.

## In code

- **Placeholder data** that ships: `John Doe`, `test@example.com` in a real
  form, `Acme Corp`, `Lorem ipsum`, `foo`/`bar` outside a throwaway example.
- **Dead controls:** a button with no handler, a link to `#`, a setting nothing
  reads, a flag that is parsed and ignored.
- **Swallowed errors:** `except: pass`, an empty `catch {}`, a failure turned
  into a silent default. If an error is deliberately ignored, the line says why.
- **Tests that match wording, not behaviour:** asserting a string appears in a
  file rather than that the thing works. A test that cannot fail is decoration.
- **Comments that restate the code** (`i += 1  # increment i`). A comment says
  why, or what would go wrong, or it is not there.
- **Invented APIs:** a method, flag or package that does not exist. Check it
  before using it.
- **Generic names:** `data`, `result`, `handleClick2`, `utils.py` holding six
  unrelated things. Name what it is.

## In interfaces

- **AI purple:** violet-to-blue gradients as the accent, glowing purple buttons.
  Pick a real accent from a neutral base.
- **One accent.** Exactly one, used consistently. A second accent appearing in
  section seven is a mistake that looks like a choice.
- **One theme, one radius scale.** Mixed warm and cool greys, or round buttons
  on square cards, read as assembled from parts.
- **Three equal cards in a row** as a feature section. Choose a layout that fits
  the content: a list, an asymmetric grid, a comparison.
- **Centred hero over a dark mesh gradient**, glassmorphism on every container,
  pill badges saying "New" on everything, motion with no purpose.
- **Fake product screenshots** and invented metrics on a landing page.
- **Round fake numbers:** 99.9%, 10x, 50% faster.

## Replacements, not deletions

Removing slop is only half the job. For each thing cut, ask what the specific,
true version would be:

    "A robust, scalable solution"  ->  "handles 10k rows in 200 ms, measured"
    "Leverages AI to streamline"   ->  "drafts the reply; you edit and send it"
    "It's important to note that"  ->  (delete; state the point)
    "Delve into the details"       ->  "the details:"

If there is no specific, true version, the sentence had no content and goes.

## Final pass, before sending

1. Search the output for an em dash. Replace every one.
2. Scan for the banned words list.
3. Every number: is there a source or a measurement? If not, remove or qualify.
4. Every claim of quality: is there a checkable reason?
5. Code: any placeholder data, empty handler, swallowed error, invented API?
6. Interface: count accents, count radius values, look for the defaults above.

Related: `master-design-taste` for choosing an aesthetic deliberately,
`master-caveman` for cutting words, `master-review` for the adversarial read.
