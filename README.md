# Global AI Setup Benchmark

A local-index setup for AI coding agents, and the benchmark that shaped it.

## PeterGanuRusty (pgr)

PeterGanuRusty (the `pgr` command) is a desktop command-line tool for AI coding agents. Any agent that can run a command can ask it where a function is defined and what calls it. The engine, pgr, is written in Rust, all of it. Python is around it, not inside it: the repository the benchmark asks questions about is a Python project, the first generation's tools, the benchmark harness and this site's generator are Python programs, and uv, pipx and pip are optional ways to deliver the same Rust program.

Install it, on Windows in PowerShell and on macOS and Linux in a terminal:

```
irm https://github.com/Charlesganu2004/Global-Ai-Setup-Benchmark/releases/latest/download/install.ps1 | iex
curl -fsSL https://github.com/Charlesganu2004/Global-Ai-Setup-Benchmark/releases/latest/download/install.sh | sh
```

The files are on the [latest release](https://github.com/Charlesganu2004/Global-Ai-Setup-Benchmark/releases/latest). Every page of the results site names the models the runs used and says what was measured: [results site](https://charlesganu2004.github.io/Global-Ai-Setup-Benchmark/).

The setup installs one set of rules, three crew agents and a session-start hook
for Claude Code, Codex and GitHub Copilot. Agents ask a local index where
something is (`lx`), get the answer in one turn, and open a file only when they
have to. The benchmark measures what that saves, in tokens, turns and dollars,
across every way the setup can answer a lookup.

- **Public site:** https://charlesganu2004.github.io/Global-Ai-Setup-Benchmark/ (the install page, the benchmarks, the study and two films; what was not measured is marked)
- **Full report:** [report/](report/index.html), seven pages with the commands, prompts and internals. It is in this repository; the site shows the measured outcomes only
- **Every table:** [benchmark/RESULTS.md](benchmark/RESULTS.md)
- **Install guide:** [setup/README.md](setup/README.md)

## What is here

```
setup/        the installable package
  INSTALL.txt     one file: instructions on top, the whole package below. python INSTALL.txt
  apply.py        installs rules, agents and the hook for each client
  tools.py        installs the search tools and the lx command; token-goat only when asked
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
report/       the full report: commands, prompts and internals
docs/         the public site (GitHub Pages): install page, benchmarks, study, films
```

## Results in short

<!-- results:start -->
1235 agents measured on one repository: three lookup questions and, in round five, three held-out ones, 59 ways of finding the answer, three models, three ways of organising the agents. 230.1 million tokens measured, 86% of them cache reads.

One agent on Claude Haiku 5.5 at low effort, fewest tokens first (methods run at least three times):

| # | Method | Runs | Turns | Tokens | Cost, cache warm | Mean score |
|---:|---|---:|---:|---:|---:|---:|
| 1 | TPX The pack only, no lookup command | 3 | 1.0 | 70,631 | $0.0012 | 100% |
| 2 | TP pgr, graph and search, with a pack in the prompt | 3 | 1.7 | 118,688 | $0.0017 | 100% |
| 3 | S1 Shipped: lx ask, default index | 3 | 2.0 | 129,686 | $0.0018 | 97% |
| 4 | S3 Shipped: lx ask, embeddings on | 3 | 2.0 | 129,954 | $0.0019 | 100% |
| 5 | L4 lx ask, one call | 3 | 2.0 | 130,091 | $0.0020 | 100% |
| 6 | C12 lx about + hybrid | 3 | 2.0 | 130,165 | $0.0021 | 100% |
| 7 | L3 lx + hybrid search | 3 | 2.0 | 130,166 | $0.0021 | 100% |
| 8 | C15 lx ask + token-goat read | 3 | 2.0 | 130,244 | $0.0020 | 100% |
| 9 | C13 lx, every new command | 3 | 2.0 | 130,244 | $0.0019 | 100% |
| 10 | S2 Shipped: all commands, default index | 3 | 2.0 | 130,309 | $0.0020 | 100% |
| 11 | N1 lx ask, no Rust embeddings | 3 | 2.0 | 130,381 | $0.0022 | 100% |
| 12 | N2 lx, every new command, no Rust embeddings | 3 | 2.0 | 130,554 | $0.0021 | 100% |
| 13 | C14 lx, every new command + token-goat | 3 | 2.0 | 130,642 | $0.0020 | 100% |
| 14 | S1r Shipped: lx ask, default index, run again with round five | 3 | 2.0 | 133,751 | $0.0019 | 100% |
| 15 | S3r Shipped: lx ask, embeddings on, run again with round five | 3 | 2.0 | 133,923 | $0.0021 | 100% |
| 16 | RGS pgr, graph and search in one call | 3 | 2.0 | 134,013 | $0.0019 | 100% |
| 17 | C7 token-goat + semble | 3 | 2.0 | 134,047 | $0.0027 | 100% |
| 18 | RM pgr, memory only | 3 | 2.0 | 134,819 | $0.0022 | 100% |
| 19 | RGSO pgr, all three, memory from an exploring session | 3 | 2.0 | 134,902 | $0.0021 | 100% |
| 20 | RGSM pgr, all three, memory from the same questions | 3 | 2.0 | 134,960 | $0.0021 | 100% |
| 21 | C11 lx about + Rust cards | 3 | 2.3 | 152,676 | $0.0022 | 100% |
| 22 | L2 lx + cards | 3 | 2.3 | 152,991 | $0.0024 | 100% |
| 23 | RGM pgr, graph and memory | 3 | 2.3 | 157,105 | $0.0023 | 100% |
| 24 | R6V pgr, graph and search in one call, inside a virtual machine | 3 | 2.3 | 158,306 | $0.0023 | 100% |
| 25 | R1 lx shipped + Rust embeddings | 3 | 2.7 | 175,536 | $0.0029 | 100% |
| 26 | C2 lx + Rust embeddings + token-goat | 3 | 2.7 | 176,514 | $0.0028 | 100% |
| 27 | A5 token-goat alone | 3 | 2.7 | 177,144 | $0.0027 | 100% |
| 28 | C6 token-goat + graphify + semble | 3 | 2.7 | 180,129 | $0.0031 | 100% |
| 29 | R6 pgr, graph and search in one call, round six | 3 | 2.7 | 181,088 | $0.0025 | 100% |
| 30 | RN pgr, graph and search in one call, without the neural network | 3 | 2.7 | 181,563 | $0.0026 | 97% |
| 31 | TPS pgr, search only, with a search pack | 3 | 2.7 | 187,572 | $0.0030 | 81% |
| 32 | TPG pgr, code graph only, with a graph pack | 3 | 2.7 | 191,727 | $0.0025 | 97% |
| 33 | A9 lx, as shipped after round 1 | 6 | 3.0 | 197,045 | $0.0028 | 99% |
| 34 | S4 Shipped: all commands, embeddings on | 3 | 3.0 | 197,588 | $0.0028 | 100% |
| 35 | TPN pgr, graph and search, with a pack, without the neural network | 3 | 3.0 | 217,000 | $0.0030 | 100% |
| 36 | C10 lx about + cards | 3 | 3.3 | 219,611 | $0.0029 | 100% |
| 37 | L1 lx + about | 3 | 3.3 | 219,739 | $0.0031 | 100% |
| 38 | C9 lx finds, token-goat reads | 3 | 3.3 | 220,870 | $0.0032 | 100% |
| 39 | C3 lx shipped + token-goat | 3 | 3.7 | 242,156 | $0.0031 | 100% |
| 40 | C8 token-goat + graphify | 3 | 3.7 | 248,739 | $0.0037 | 100% |
| 41 | RS pgr, search only | 3 | 3.7 | 251,681 | $0.0037 | 74% |
| 42 | RG pgr, code graph only | 3 | 4.0 | 273,398 | $0.0037 | 100% |
| 43 | C4 Rust index + token-goat | 3 | 4.0 | 281,589 | $0.0050 | 100% |
| 44 | C5 Rust index + graphify + semble | 3 | 4.0 | 289,122 | $0.0056 | 100% |
| 45 | A0r No index, run again with round five | 3 | 4.0 | 293,653 | $0.0065 | 99% |
| 46 | RSM pgr, search and memory | 3 | 4.3 | 301,711 | $0.0044 | 85% |
| 47 | C1 Rust index + token-goat + graphify + semble | 3 | 4.3 | 310,595 | $0.0052 | 99% |
| 48 | A0 No index | 5 | 5.0 | 364,508 | $0.0074 | 98% |

The mean score is the average score of the answers, with partial credit; it is not a share of fully correct runs.
A0r: another tool on the test machine, token-goat's secret-redaction hook, altered some of this row's reads, so its mean score is not comparable with the others and its fully correct runs are not set against the product's. See "Known weaknesses of the study" in benchmark/RESULTS.md.

- An agent starts with about 64k tokens of context before it reads one line of code, and every later turn re-reads all of it. Across the 1235 agents measured, 86% of all tokens were cache reads, 13% were cache writes and 1.0% were output. What an index saves in tool output is small next to that. What it really buys is fewer turns.
- Where the shipped build, the round 1 leaders and the control stand among those 48 methods: S1 Shipped: lx ask, default index: 130k tokens, 2.0 turns, 2 of 3 runs fully correct, place 3; S3 Shipped: lx ask, embeddings on: 130k tokens, 2.0 turns, 3 of 3 runs fully correct, place 4; A9 lx, as shipped after round 1: 197k tokens, 3.0 turns, 5 of 6 runs fully correct, place 33; R1 lx shipped + Rust embeddings: 176k tokens, 2.7 turns, 3 of 3 runs fully correct, place 25; A0 No index: 365k tokens, 5.0 turns, 4 of 5 runs fully correct, place 48.
- The shipped build on an index with Rust embeddings and on the default index without them. Claude Haiku 5.5 at low effort, one agent: Shipped: lx ask, embeddings on 130k (2 turns over 3 runs), against Shipped: lx ask, default index 130k (2 turns over 3 runs); Shipped: all commands, embeddings on 198k (2 to 4 turns over 3 runs), against Shipped: all commands, default index 130k (2 turns over 3 runs).
- The shipped build on an index with Rust embeddings and on the default index without them. Claude Sonnet 5.5 at medium effort, one agent: Shipped: lx ask, embeddings on 129k (2 turns over 3 runs), against Shipped: lx ask, default index 196k (3 turns over 6 runs); Shipped: all commands, embeddings on 130k (2 turns over 3 runs), against Shipped: all commands, default index 196k (3 turns over 3 runs).
- The shipped build on an index with Rust embeddings and on the default index without them. Claude Opus 5.5 at maximum effort, one agent: Shipped: lx ask, embeddings on 199k (3 turns over 1 run), against Shipped: lx ask, default index 201k (3 turns over 1 run); Shipped: all commands, embeddings on 199k (3 turns over 1 run), against Shipped: all commands, default index 199k (3 turns over 1 run).
- Does adding token-goat beside lx change anything? Each pair is the same lx build without it and with it. Claude Haiku 5.5 at low effort, one agent: lx, as shipped after round 1 197k (2 to 5 turns over 6 runs), against lx shipped + token-goat 242k (3 to 5 turns over 3 runs); lx shipped + Rust embeddings 176k (2 to 3 turns over 3 runs), against lx + Rust embeddings + token-goat 177k (2 to 3 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against lx, every new command + token-goat 131k (2 turns over 3 runs); lx ask, one call 130k (2 turns over 3 runs), against lx ask + token-goat read 130k (2 turns over 3 runs).
- Does adding token-goat beside lx change anything? Each pair is the same lx build without it and with it. Claude Sonnet 5.5 at medium effort, one agent: lx, as shipped after round 1 217k (3 to 4 turns over 3 runs), against lx shipped + token-goat 196k (3 turns over 2 runs); lx shipped + Rust embeddings 129k (2 turns over 2 runs), against lx + Rust embeddings + token-goat 129k (2 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against lx, every new command + token-goat 130k (2 turns over 2 runs); lx ask, one call 129k (2 turns over 2 runs), against lx ask + token-goat read 129k (2 turns over 2 runs).
- Do the engines do as well without lx in front of them? lx with every command against the raw commands of the engines it wraps. Claude Haiku 5.5 at low effort, one agent: lx, every new command 130k (2 turns over 3 runs), against Rust index + graphify + semble 289k (3 to 5 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against Rust index + token-goat + graphify + semble 311k (4 to 5 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against Rust index + token-goat 282k (3 to 5 turns over 3 runs).
- Do the engines do as well without lx in front of them? lx with every command against the raw commands of the engines it wraps. Claude Sonnet 5.5 at medium effort, one agent: lx, every new command 130k (2 turns over 2 runs), against Rust index + graphify + semble 428k (5 to 7 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against Rust index + token-goat + graphify + semble 314k (3 to 6 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against Rust index + token-goat 393k (5 to 6 turns over 2 runs).
- In these runs orchestration came with more fully correct runs, and it is the expensive part. With Claude Haiku 5.5 at low effort subagents the orchestrator's two calls were 97% of the run's cost. Without a brief or a reviewer, 5 of 49 three-subagent runs returned a wrong detail; orchestrated on the same model, 66 of 66 ended fully correct, 63 of them before the review changed anything. The orchestrator is of a larger model than the subagents, so this compares more than orchestration, and every cost is an estimate at list prices, with the cost of planning imputed from the few planning calls that were measured.
- token-goat's hooks, installed in the client: with no index they ran in 9 of 9 runs, rewrote 3.7 tool results a run, and the runs used 2% more tokens, inside the 21% by which repeats of one prompt without the hooks differ; 5 of 9 of those runs ended fully correct, against 13 of 17 without the hooks. With index commands sent through PowerShell they ran in 6 of 32 runs. Through Bash they ran in 7 of 7 runs, rewrote nothing, and the runs used 4% more tokens, inside the 26% by which repeats of one prompt without the hooks differ.
<!-- results:end -->

The full tables, the three models, the three agent setups and the limits
of the test are in [benchmark/RESULTS.md](benchmark/RESULTS.md) and in the full report (`report/`).

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

All rights reserved: see [LICENSE.txt](LICENSE.txt). The terms of use for PeterGanuRusty and
for the rest of this repository are to be published in that file; until then no licence to copy,
modify or redistribute is granted. The skills under `setup/skills` are the author's own statements
of techniques from other projects, each credited in its text.

## Read this before quoting a number

One repository, one language, three easy lookup questions in each of two sets, and between
one and six runs per cell, three in most. A difference of one turn or of one fully correct
run between two methods is noise, and nothing here is a claim of significance. The second set
of questions was written later by the same team and is not an independent test. The start-up
context every agent carries belongs to the machine the test ran on, and most of the saving
shown is fewer turns of it. Costs are list prices applied to measured tokens. In the second
session the no-index control had its reads altered by another tool on the test machine, so its
correctness is not set against the product's. The section "Known weaknesses of the study" at the
end of [benchmark/RESULTS.md](benchmark/RESULTS.md) lists everything the method review found.
