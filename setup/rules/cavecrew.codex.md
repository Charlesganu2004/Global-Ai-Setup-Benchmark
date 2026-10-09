## 5. CaveCrew and dynamic tier routing

The model the user selected for the session is always the master orchestrator
and lead reviewer: it plans, delegates and gives final sign-off. No model slug
is written in this file or in any agent file. Tiers are classes of model.
Codex has no model aliases, so a tier is reached through inheritance, reasoning
effort and, when the session offers it, a model passed on the call.

Tiers:
- **Tier A, apex**: the most intelligent model on offer, run at its highest
  reasoning setting. Here the session model with `reasoning_effort` at the top
  of its range. Orchestration, multi-file architecture, concurrency and race
  conditions, thorny debugging, high-stakes review.
- **Tier B, coder**: the fast frontier coding class. Here the session model at
  the session's own effort, or the current fast coding model when `spawn_agent`
  offers one. Implementations, test suites, refactors.
- **Tier C, scout**: the lightest model on offer at a low reasoning setting.
  Here `reasoning_effort: "low"`, and the smallest current model when
  `spawn_agent` offers one. Recon, symbol lookup, log and diff triage,
  mechanical transforms.

Crew, user-level agents in `~/.codex/agents/cavecrew-*.toml`, spawned with
`spawn_agent` by `agent_type`. Each sets neither a model nor an effort, so it
inherits the session's and never goes stale:
- `cavecrew-investigator`: read-only locator and triage, bound by the read gate
  in section 6. Tier C. Escalate to Tier A only for polyglot or architecture
  puzzles.
- `cavecrew-builder`: implementation, fixes, tests, stepwise refactors. Full
  output, never placeholders. Tier B; Tier A for hard lanes, Tier C for
  boilerplate.
- `cavecrew-reviewer`: adversarial verification, security, anti-slop,
  test-assertion and regression checks. Tier A. The orchestrator signs off
  last.

The tier is set on the call: `spawn_agent` takes `reasoning_effort` and
`model`. A value written in an agent file would outrank the call and could
never be raised, which is why these files set none. A model passed per call
comes from what the session offers at that moment, never from a slug written
here. Pick the lowest tier that does the lane well.

Delegation rules:
- Delegate only independent lanes worth a fresh context. A lookup that takes a
  handful of calls stays inline: a subagent pays for its whole start-up context
  before it reads a line and re-reads it on every step it takes.
- Lookups go to Tier C. Measured on one client with the same three lookups, the
  middle tier cost about 20 times as much and the top tier 30 times or more,
  for the same answers. Tier C now and then drops one item from a long list, so
  a result that must be complete gets a count check by the orchestrator, not a
  higher tier.
- A subagent that already loads this file gets a brief with the task, what is
  ruled out, the paths to read and the session's `local-index:` line. A
  subagent that starts without this file gets the read gate (section 6)
  verbatim as well.
- Never trust subagent output blind. Inspect the real diff, run the tests,
  quote the failing line first.
