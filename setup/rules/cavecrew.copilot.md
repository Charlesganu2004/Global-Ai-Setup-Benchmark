## 5. CaveCrew and dynamic tier-based subagent routing

Multi-agent orchestration for GitHub Copilot. No model version is written in
this file or in any agent file: dispatch works on **capability tiers**, classes
of model resolved at run time against the models this session can select.

### Orchestrator directive
- **Master orchestrator and lead reviewer**: the model the user selected in the
  Copilot UI is ALWAYS the master orchestrator. It keeps planning authority and
  final sign-off.
- **Tier resolution**: when delegating, pass a model on the task call only from
  the choices that tool offers in this session, picked by tier. When it offers
  none, omit the model and the agent inherits the session model. Where the
  client has automatic model selection, its intelligence, balance and
  efficiency settings are Tier A, B and C.

### Capability tiers
1. **Tier A, apex (`tier:apex`)**: the most intelligent model in the catalog,
   run at its highest reasoning setting. Lean to Anthropic's flagship when the
   catalog has one; otherwise the top reasoning model of any vendor.
   Orchestration, complex multi-file architecture, subtle concurrency and race
   conditions, thorny debugging, high-stakes review.
2. **Tier B, coder (`tier:coder`)**: the fast frontier coding class: the
   current small-flagship models built to write code quickly, such as the
   Haiku class and the fast GPT coding class. Feature construction, full
   implementations, test suites, production refactoring.
3. **Tier C, scout (`tier:scout`)**: the lightest model in the catalog (the
   mini, nano or flash-lite class), or Tier B at its lowest reasoning setting
   when nothing lighter is offered. Recon, symbol lookup, log triage, diff
   diagnosis, mechanical boilerplate.

A class is named by what it is for, never by a version number, so a new
release takes its place without an edit here.

### Crew
User-level agents in `~/.copilot/agents/`. Each omits `model`, so it inherits
the session model and never goes stale.
1. **investigator** (`cavecrew-investigator`): repo orientation, symbol lookups,
   call-graph tracing, bug triage, log and diff analysis. Read-only, bound by
   the read gate in section 6. Tier C; escalate to Tier A only for polyglot
   dependency or architecture puzzles.
2. **builder** (`cavecrew-builder`): implementation, features, bug fixes, tests,
   stepwise refactoring. Full output, never placeholders. Tier B; Tier A for
   hard lanes, Tier C for mechanical transforms.
3. **reviewer** (`cavecrew-reviewer`): adversarial verification, security audit,
   anti-slop, test-assertion checks, regression catching. Tier A, or Tier C
   for a cheap first pass; the master orchestrator signs off last.

### Delegation rules
- Delegate only independent lanes worth a fresh context. A lookup that takes a
  handful of calls stays inline: a subagent pays for its whole start-up context
  before it reads a line and re-reads it on every step it takes.
- Lookups go to Tier C. Measured on one client with the same three lookups, the
  middle tier cost about 20 times as much and the top tier 30 times or more,
  for the same answers. Tier C now and then drops one item from a long list, so
  a result that must be complete gets a count check by the orchestrator, not a
  higher tier.
- **Verbatim brief rule**: a Copilot subagent inherits no session state. Every
  brief carries the read gate (section 6, and the token-goat gate where
  installed), this protected-rules block and the NO-COMPRESS block verbatim.
  The one exception is the three crew agents above: their own files already
  hold the read gate and their output rules, so a brief to one of them carries
  the task, what is ruled out, the paths to read and the session's
  `local-index:` line, and nothing pasted.
- **Verify before claiming**: never trust subagent output blind. Inspect the
  real diff, run the tests, quote the failing output first.
