## 5. CaveCrew and dynamic tier routing

The model the user selected for the session is always the master orchestrator
and lead reviewer: it plans, delegates and gives final sign-off. No model
version is written in this file or in any agent file. Tiers are classes of
model, resolved at run time through Claude Code's aliases, which follow each
new release.

Tiers:
- **Tier A, apex**: the most intelligent model on offer, run at its highest
  reasoning setting. Here that is `inherit` when the session is on the
  flagship, otherwise the strongest alias the Agent tool lists. Orchestration,
  multi-file architecture, concurrency and race conditions, thorny debugging,
  high-stakes review.
- **Tier B, coder**: the fast frontier coding class. Here `sonnet` for work
  that spans files and `haiku` for single-file work. Implementations, test
  suites, refactors.
- **Tier C, scout**: the lightest model on offer at a low reasoning setting.
  Here `haiku`. Recon, symbol lookup, log and diff triage, mechanical
  transforms.

Crew, user-level agents in `~/.claude/agents/`, dispatched with the Agent tool:
- `cavecrew-investigator`: read-only locator and triage, bound by the read gate
  in section 6. Tier C. Escalate to Tier A only for polyglot or architecture
  puzzles.
- `cavecrew-builder`: implementation, fixes, tests, stepwise refactors. Full
  output, never placeholders. Tier B; pass the session model's own alias for a
  hard lane and `model: "haiku"` for boilerplate.
- `cavecrew-reviewer`: adversarial verification, security, anti-slop,
  test-assertion and regression checks. Tier A at maximum effort;
  `model: "haiku"` for a cheap first pass. The orchestrator signs off last.

The Agent tool's per-call `model` outranks the agent file: that is how a lane
moves up or down a tier. Pick the lowest tier that does the lane well, and
spawn Tier A subagents only when the lane needs the session model's full
reasoning. A new alias in the Agent tool's model list is placed by capability,
never by writing a version here.

Delegation rules:
- Delegate only independent lanes worth a fresh context. A lookup that takes a
  handful of calls stays inline: a subagent pays for its whole start-up context
  before it reads a line (about 60k tokens when this setup was measured) and
  re-reads it on every step it takes.
- Lookups go to Tier C. Measured on the same three lookups, Tier B cost about
  20 times as much and Tier A 30 times or more, for the same answers. Tier C
  now and then drops one item from a long list, so a result that must be
  complete gets a count check by the orchestrator, not a higher tier.
- The crew and other custom or general-purpose subagents load this file
  themselves. Their brief carries the task, what is already ruled out, the
  paths to read and the session's `local-index:` line, never a pasted copy of
  these rules.
- The built-in Explore and Plan agents do not load this file. Use
  `cavecrew-investigator` for code search instead of Explore; a brief to either
  built-in carries section 6's command list.
- Never trust subagent output blind. Inspect the real diff, run the tests,
  quote the failing line first.
