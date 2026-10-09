# Global AI Setup Benchmark

A local-index setup for AI coding agents, and the benchmark that shaped it.

The setup installs one set of rules, three crew agents and a session-start hook
for Claude Code, Codex and GitHub Copilot. Agents ask a local index where
something is (`lx`), get the answer in one turn, and open a file only when they
have to. The benchmark measures what that saves, in tokens, turns and dollars,
across every way the setup can answer a lookup.

- **Site, with the charts:** https://charlesganu2004.github.io/Global-Ai-Setup-Benchmark/
- **Every table:** [benchmark/RESULTS.md](benchmark/RESULTS.md)
- **Install guide:** [setup/README.md](setup/README.md)

## What is here

```
setup/        the installable package
  INSTALL.txt     one file: instructions on top, the whole package below. python INSTALL.txt
  apply.py        installs rules, agents and the hook for each client
  tools.py        installs the search tools and the lx command
  local-index/    the session hook and lx (session_index.py) and its options
  rules/          the global rules, one source rendered per client
  agents/         the three crew agents, one source rendered per client
  skills/         skills the rules refer to
  rendered/       the rules as each client receives them, for reading
  tests/          checks for the hook and the installer
benchmark/    the measurements
  RESULTS.md      every table
  data/           every agent run and every index timing, as JSON
  harness/        the workflow that runs the agents, the parser, the grader, the site builder
docs/         the site (GitHub Pages)
```

## Results in short

<!-- results:start -->
502 agent runs on one repository: three lookup questions, 36 ways of finding the answer, three model tiers, three ways of organising the agents. 97.9 million tokens measured, 85% of them cache reads.

One agent on the cheapest model tier, fewest tokens first (methods run at least three times):

| # | Method | Runs | Turns | Tokens | Cost, cache warm | Correct |
|---:|---|---:|---:|---:|---:|---:|
| 1 | S1 Shipped: lx ask, default index | 3 | 2.0 | 129,686 | $0.0018 | 97% |
| 2 | S3 Shipped: lx ask, embeddings on | 3 | 2.0 | 129,954 | $0.0019 | 100% |
| 3 | L4 lx ask, one call | 3 | 2.0 | 130,091 | $0.0020 | 100% |
| 4 | C12 lx about + hybrid | 3 | 2.0 | 130,165 | $0.0021 | 100% |
| 5 | L3 lx + hybrid search | 3 | 2.0 | 130,166 | $0.0021 | 100% |
| 6 | C15 lx ask + token-goat read | 3 | 2.0 | 130,244 | $0.0020 | 100% |
| 7 | C13 lx, every new command | 3 | 2.0 | 130,244 | $0.0019 | 100% |
| 8 | S2 Shipped: all commands, default index | 3 | 2.0 | 130,309 | $0.0020 | 100% |
| 9 | N1 lx ask, no Rust embeddings | 3 | 2.0 | 130,381 | $0.0022 | 100% |
| 10 | N2 lx, every new command, no Rust embeddings | 3 | 2.0 | 130,554 | $0.0021 | 100% |
| 11 | C14 lx, every new command + token-goat | 3 | 2.0 | 130,642 | $0.0020 | 100% |
| 12 | C7 token-goat + semble | 3 | 2.0 | 134,047 | $0.0027 | 100% |
| 13 | C11 lx about + Rust cards | 3 | 2.3 | 152,676 | $0.0022 | 100% |
| 14 | L2 lx + cards | 3 | 2.3 | 152,991 | $0.0024 | 100% |
| 15 | R1 lx shipped + Rust embeddings | 3 | 2.7 | 175,536 | $0.0029 | 100% |
| 16 | C2 lx + Rust embeddings + token-goat | 3 | 2.7 | 176,514 | $0.0028 | 100% |
| 17 | C6 token-goat + graphify + semble | 3 | 2.7 | 180,129 | $0.0031 | 100% |
| 18 | A9 lx, as shipped after round 1 | 6 | 3.0 | 197,045 | $0.0028 | 99% |
| 19 | S4 Shipped: all commands, embeddings on | 3 | 3.0 | 197,588 | $0.0028 | 100% |
| 20 | C10 lx about + cards | 3 | 3.3 | 219,611 | $0.0029 | 100% |
| 21 | L1 lx + about | 3 | 3.3 | 219,739 | $0.0031 | 100% |
| 22 | C9 lx finds, token-goat reads | 3 | 3.3 | 220,870 | $0.0032 | 100% |
| 23 | C3 lx shipped + token-goat | 3 | 3.7 | 242,156 | $0.0031 | 100% |
| 24 | C8 token-goat + graphify | 3 | 3.7 | 248,739 | $0.0037 | 100% |
| 25 | C4 Rust index + token-goat | 3 | 4.0 | 281,589 | $0.0050 | 100% |
| 26 | C5 Rust index + graphify + semble | 3 | 4.0 | 289,122 | $0.0056 | 100% |
| 27 | C1 Rust index + token-goat + graphify + semble | 3 | 4.3 | 310,595 | $0.0052 | 99% |
| 28 | A0 No index | 5 | 5.0 | 364,508 | $0.0074 | 98% |

- An agent starts with about 63k tokens of context before it reads one line of code, and every later turn re-reads all of it. Across the 502 agents measured, 85% of all tokens were cache reads, 14% were cache writes and 0.6% were output. What an index saves in tool output is small next to that. What it really buys is fewer turns.
- Where the shipped build, the round 1 leaders and the control stand among those 28 methods: S1 Shipped: lx ask, default index: 130k tokens, 2.0 turns, 2 of 3 runs fully correct, place 1; S3 Shipped: lx ask, embeddings on: 130k tokens, 2.0 turns, 3 of 3 runs fully correct, place 2; A9 lx, as shipped after round 1: 197k tokens, 3.0 turns, 5 of 6 runs fully correct, place 18; R1 lx shipped + Rust embeddings: 176k tokens, 2.7 turns, 3 of 3 runs fully correct, place 15; A0 No index: 365k tokens, 5.0 turns, 4 of 5 runs fully correct, place 28.
- The shipped build on an index with Rust embeddings and on the default index without them. Tier C, one agent: Shipped: lx ask, embeddings on 130k (2 turns over 3 runs), against Shipped: lx ask, default index 130k (2 turns over 3 runs); Shipped: all commands, embeddings on 198k (2 to 4 turns over 3 runs), against Shipped: all commands, default index 130k (2 turns over 3 runs).
- The shipped build on an index with Rust embeddings and on the default index without them. Tier B, one agent: Shipped: lx ask, embeddings on 129k (2 turns over 3 runs), against Shipped: lx ask, default index 196k (3 turns over 3 runs); Shipped: all commands, embeddings on 130k (2 turns over 3 runs), against Shipped: all commands, default index 196k (3 turns over 3 runs).
- The shipped build on an index with Rust embeddings and on the default index without them. Tier A, one agent: Shipped: lx ask, embeddings on 199k (3 turns over 1 run), against Shipped: lx ask, default index 201k (3 turns over 1 run); Shipped: all commands, embeddings on 199k (3 turns over 1 run), against Shipped: all commands, default index 199k (3 turns over 1 run).
- Does adding token-goat beside lx change anything? Each pair is the same lx build without it and with it. Tier C, one agent: lx, as shipped after round 1 197k (2 to 5 turns over 6 runs), against lx shipped + token-goat 242k (3 to 5 turns over 3 runs); lx shipped + Rust embeddings 176k (2 to 3 turns over 3 runs), against lx + Rust embeddings + token-goat 177k (2 to 3 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against lx, every new command + token-goat 131k (2 turns over 3 runs); lx ask, one call 130k (2 turns over 3 runs), against lx ask + token-goat read 130k (2 turns over 3 runs).
- Does adding token-goat beside lx change anything? Each pair is the same lx build without it and with it. Tier B, one agent: lx, as shipped after round 1 217k (3 to 4 turns over 3 runs), against lx shipped + token-goat 196k (3 turns over 2 runs); lx shipped + Rust embeddings 129k (2 turns over 2 runs), against lx + Rust embeddings + token-goat 129k (2 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against lx, every new command + token-goat 130k (2 turns over 2 runs); lx ask, one call 129k (2 turns over 2 runs), against lx ask + token-goat read 129k (2 turns over 2 runs).
- Do the engines do as well without lx in front of them? lx with every command against the raw commands of the engines it wraps. Tier C, one agent: lx, every new command 130k (2 turns over 3 runs), against Rust index + graphify + semble 289k (3 to 5 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against Rust index + token-goat + graphify + semble 311k (4 to 5 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against Rust index + token-goat 282k (3 to 5 turns over 3 runs).
- Do the engines do as well without lx in front of them? lx with every command against the raw commands of the engines it wraps. Tier B, one agent: lx, every new command 130k (2 turns over 2 runs), against Rust index + graphify + semble 428k (5 to 7 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against Rust index + token-goat + graphify + semble 314k (3 to 6 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against Rust index + token-goat 393k (5 to 6 turns over 2 runs).
- The model tier moves cost far more than the method does. With the start-up context cached, the same no-index job cost $0.0074 on Tier C (98% correct), $0.103 on Tier B (100%) and $0.294 on Tier A (100%). Send lookups to the cheapest tier.
- Orchestration buys accuracy and it is the expensive part. With Tier C subagents the orchestrator's two calls were 97% of the run's cost. Without a reviewer, 4 of 38 three-subagent runs returned a wrong detail; with the orchestrator checking, 44 of 44 orchestrated runs ended fully correct.
<!-- results:end -->

The full tables, the three model tiers, the three agent setups and the limits
of the test are in [benchmark/RESULTS.md](benchmark/RESULTS.md) and on the site.

## Install the setup

```
python setup/INSTALL.txt              # rules, agents, hook and tools, all three clients
python setup/INSTALL.txt --dry-run    # show what would change, write nothing
```

Details, options and limits: [setup/README.md](setup/README.md).

## Run the benchmark again

The harness runs inside Claude Code, because what it measures is a Claude Code
agent with its real start-up context. Steps: [benchmark/README.md](benchmark/README.md).

## Licence

No licence has been chosen yet, so the default applies: you may read the code
and fork it on GitHub, and nothing more is granted until a licence file is
added. The skills under `setup/skills` are the author's own statements of
techniques from other projects, each credited in its text.

## Read this before quoting a number

One repository, one language, three easy lookup questions, and between one and
six runs per cell. A difference of one turn between two methods is noise. The
start-up context every agent carries belongs to the machine the test ran on.
Costs are list prices applied to measured tokens.
