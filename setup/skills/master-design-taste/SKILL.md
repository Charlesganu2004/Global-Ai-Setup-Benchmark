---
name: master-design-taste
description: Design or review anything a person will see, from a single component to a whole site, with a deliberate direction instead of model defaults. Use before building or changing an interface, when choosing type, colour, layout or motion, and when reviewing a surface that looks generic. Covers reading the brief, the three dials, choosing a real design system versus an aesthetic, the rules that hold everywhere, accessibility as a gate, and verifying in a real browser.
---

# Design taste

The failure this prevents is not ugliness. It is the interface that looks like
every other generated interface: the same purple gradient, the same three cards,
the same centred hero. It is chosen by nobody. Taste here means making each
decision deliberately and being able to say why it fits this audience.

The approach draws on Leonxlnx/taste-skill (MIT), whose taste skill is the
original and far longer; this is our own condensed statement with house rules
added. `repo-lists/skill-upstreams.json` records the version checked against.

## 1. Audit what exists before changing it

On an existing surface, first find what already governs it: design docs, a
tokens file, a component library, the CSS actually loaded. Use those before
inventing values. A redesign that ignores the existing system produces two
systems. Say what you found before proposing anything.

## 2. Read the brief, then state the read

Before any code, write one line:

    Reading this as: <page kind> for <audience>, in a <vibe> language,
    leaning toward <design system or aesthetic family>.

Signals to read: what kind of page it is, words the person used ("calm",
"dense", "playful", "trust-first"), products they named, who the audience is,
brand assets that already exist, and quiet constraints such as accessibility-first
audiences or regulated industries. The audience picks the aesthetic, not you.

If two readings would produce materially different work, ask one question.
Otherwise declare the read and proceed.

## 3. Set the three dials

    VARIANCE  1 perfect symmetry ... 10 deliberate chaos
    MOTION    1 static ... 10 cinematic
    DENSITY   1 gallery, airy ... 10 cockpit, packed data

Defaults by kind:

    marketing or landing page     7 / 6 / 4
    developer tool or docs        5 / 4 / 5
    data dashboard                3 / 3 / 8
    public sector, trust-first    3 / 2 / 5
    editorial or long-form        6 / 4 / 3

Every later choice follows the dials. High density with high motion is almost
always wrong.

## 4. Real system or aesthetic

If the brief matches an existing design system, use the official package rather
than recreating it: Fluent for Microsoft-style enterprise, Material for
Google-style product, Carbon for IBM-style analytics, GOV.UK or USWDS for public
sector, Primer for GitHub-style tools. One system per project; do not mix them.

If the brief is an aesthetic (editorial, brutalist, glass, bento, dark-tech),
build it with the project's own CSS and say honestly in a comment that it is an
approximation, not an official system.

## 5. Rules that hold everywhere

**Colour.** One accent, used consistently across the whole surface. Neutral base
tinted from one grey family, warm or cool, not both. No pure #000 background; use
an off-black. Saturation of the accent below about 80 percent. No AI-purple
default unless the brand genuinely is purple.

**Shape.** One radius scale, followed everywhere. Sharp, soft or pill, pick one,
or document a rule (buttons full-pill, cards 12px, inputs 8px) and keep it.

**Type.** One family, or one plus a monospace for code and numbers. A modular
scale, not arbitrary sizes. Body text capped near 65 characters a line. Tabular
figures for numbers in tables. Negative letter-spacing on large display text only.

**Layout.** Grid over percentage flexbox arithmetic. A max-width container. No
three-equal-cards feature row by default. Full-height sections use `min-height:
100dvh`, not `100vh`, so mobile address bars do not break them.

**Hierarchy.** Size, weight and colour, in that order of strength. If everything
is bold, nothing is. Often the fix is making surrounding text quieter.

**States.** Hover, active, focus, loading, empty, error. A surface with only its
success state is not finished, and the empty state is the first thing a new user
sees.

**Content.** Real copy, not lorem ipsum. Plausible names and messy real numbers,
not "John Doe" and 99.99%.

## 6. Accessibility is a gate, not polish

Pass or fail, checked with a measurement:

- text contrast at least 4.5:1, large text 3:1; check buttons especially
- a visible focus ring on everything reachable by keyboard
- the whole flow usable by keyboard alone
- `prefers-reduced-motion` honoured
- touch targets about 44 px where practical
- semantic elements: nav, main, button for buttons, labels on inputs

## 7. Verify in a real browser

Load the actual page in a real browser at 100 percent zoom, at more than one
viewport width, and look at it. Counting DOM elements cannot tell a working page
from one hidden behind an opaque layer; a screenshot and a hit-test can. This
repository shipped a page where every element was present and nothing could be
clicked, and only looking found it.

Related: `master-refactor-ui` for fixing an existing surface in the right order,
`master-anti-slop` for the default patterns to reach past, `verify-before-complete`
for why the screenshot is the evidence.
