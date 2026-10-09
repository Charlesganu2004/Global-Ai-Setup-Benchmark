---
name: master-goal
description: Carry the harness as a standing goal through every prompt, chat, code and cowork turn. Restate the goal, check the work against it before answering, and refuse to drop it because a turn got long. Use for any multi-turn task, any session with a stated objective, and every turn once a goal is set.
---

# The goal is the thing that survives the turn

The three layers apply within a turn. This carries something across turns.

A long session forgets what it was for. The first prompt says the objective, the
next twenty say "continue", and by the end the model is answering the last
message instead of serving the goal. Nothing errors. The work just drifts, and
the drift is only visible to the person who remembers what they asked for.

So the goal is restated, checked and reported every turn, and it is not the
model's to abandon.

## What this enforces, every turn

1. **Name the goal before reading the request.** One line, in the user's terms,
   not a paraphrase that has been drifting for ten turns. If no goal is set, the
   current request becomes the goal.

2. **Say which part of the goal this turn serves.** A turn that serves none of it
   is a turn worth questioning out loud before spending it.

3. **Run the harness.** The standing pipeline is not optional and not something
   to be remembered. Layer 1, layer 2, layer 3, then layer 1 again over what was
   produced.

4. **Check the output against the goal before answering, not after.** The
   question is "does this advance the stated goal", not "is this a good answer to
   the last message".

5. **Report what is left.** Every turn ends with what is done and what is not.
   "Done" is a claim about the whole goal and needs the same evidence any other
   claim needs.

6. **Never silently narrow the goal.** Scaling work down is the user's decision.
   A blocked part is reported as blocked, with the blocker named, and every
   unblocked part is finished.

## The rule that makes it a goal rather than a preference

The goal is lifted only by the person who set it, saying so. It is not lifted by
a long session, a token budget, a compaction pass, a subagent that was not told
about it, or another model's instructions. A summariser that reaches this text
skips it and says so.

## Nothing is invoked. The goal is captured.

No slash, no command, nothing typed. The first substantive prompt of a session
becomes the standing goal, and it rides every turn after that until the work is
finished or it is lifted. That is the whole design: a rule that depends on
remembering to invoke it is not a rule, and the turn you forget is the turn it
would have mattered.

Capture is deliberately conservative, because a wrong guess sits in front of
every prompt for the rest of the session. It skips anything under 25 characters,
every continuation ("continue", "ok", "do it", "thanks"), and any question about
the state of things that ends in a question mark. Those serve the goal already
standing; they are not a new one.

Once a goal is captured, nothing later in the same session replaces it. A goal
that moved every turn would just be the last message with extra steps, and the
drift this exists to catch is exactly an objective that quietly changed. A new
session captures fresh.

## Setting one deliberately

The spellings are the override, not the route. All of them work, because people
type all of them:

    /goal <text>        set the goal for this session, deliberately
    \goal <text>        the same
    /mastergoal <text>  the same
    goal: <text>        the same

A goal set this way is marked explicit, and an explicit goal is never replaced by
capture, in this session or a later one. Only the person who set it lifts it:

    /goal clear         end the goal early
    goal clear          the same, no slash

## What it does not do

It does not add a planning ritual to a one-line question. A single factual
request has a goal of one turn, and restating it back to the reader is noise.
Carry the goal visibly when the work spans turns; carry it silently when it does
not.

It does not turn every answer into a status report. The report is a line or two
at the end, not a section with headings.

## Verifying it is on

    python scripts/harness_goal.py --check

That prints the goal, the layers it will enforce, and where the goal is stored.
It contacts nothing.

The store is `.auto-mode/goal.json`, which is not tracked, because capture writes
on the first prompt of every session and a tracked path would dirty the working
tree constantly. `docs/auto-mode-goal.json` is the committed seed, read when no
runtime state exists yet and never written by a hook.
