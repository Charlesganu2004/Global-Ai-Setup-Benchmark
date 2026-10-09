# Benchmark

What a code lookup costs an AI coding agent in tokens, measured for every way
this setup can answer one: no index, each engine alone, the `lx` command in
each of its builds, and combinations of them.

- [RESULTS.md](RESULTS.md) has every table.
- The charts are on the project site (the `docs/` folder, served by GitHub Pages).

## What is measured

Three questions about one repository, each with a ground truth taken from
Python's syntax tree:

1. Who calls one function (seven callers across four files).
2. Where another function is defined, what parameters it takes and who calls it.
3. Which function registers a git merge driver, and which one removes it.

An agent is told which commands it may use (one "method") and answers through a
structured output, so grading is a script, not a judgment. Each method is run
on three model tiers and in three setups: one agent, three subagents started
together, and an orchestrator that briefs three subagents and checks their work.

For every agent the harness reads the usage the API reported for each request
and splits it into the context the agent started with, what it re-read on later
turns, what it added, and what it wrote.

## Layout

```
benchmark/
  RESULTS.md                      every table, generated
  data/
    results.json                  every agent and every run, generated from transcripts
    index.json                    index build times and sizes, measured by the scripts below
  harness/
    token-benchmark.workflow.js   the agent runs: methods, prompts, tiers, setups
    arms.json                     what each method is and the commands it was given
    truth.py                      ground truth for the questions, from the syntax tree
    bench_parse.py                transcripts to results.json: tokens, cost, tools, grades
    build_site.py                 results.json to docs/data/benchmark.js and RESULTS.md
    combo_bench.py                build time, lookup time and answer size per engine
    emb_bench.py                  the same with the Rust index's embeddings on, and search quality
    lean_bench.py                 the combined index with and without the graph
```

## Run it again

The harness runs inside Claude Code, because the thing being measured is a
Claude Code agent with its real start-up context.

1. Make the fixture: any git repository works. Ours is the source tree of
   graphify 0.9.61 (86 Python files, 2.97 MB) committed once. Pass its path as
   `repo` in the workflow's arguments (and as `repoPlain` a second copy indexed
   without Rust embeddings, for the methods that need one), and regenerate the
   ground truth with `python harness/truth.py REPO NAME...` if you change the questions.
2. Build every index the methods use, from the fixture's root: `lx refresh`
   (with `codanna_embeddings` on for the methods that search the Rust index),
   `codanna init` then `codanna index .` for the methods that use codanna
   directly, and `token-goat index --embed` for the token-goat methods.
3. Ask Claude Code to run `harness/token-benchmark.workflow.js` as a workflow,
   passing the cells to run, for example
   `{"repo": "PATH", "matrix": [{"arms": ["A0", "S1"], "setup": "single", "tier": "C", "reps": ["r1", "r2", "r3"]}]}`.
4. Parse the transcripts it leaves behind, then rebuild the site data:

```
python benchmark/harness/bench_parse.py benchmark/data/results.json TRANSCRIPT_DIR...
python benchmark/harness/build_site.py
```

The index timings come from the three `*_bench.py` scripts, which take a source
folder and a scratch folder and print JSON; `index.json` holds what they printed.

## Reading the numbers

- One repository, one language, three easy questions. Most cells are one to
  six runs. A difference of one turn between two methods is noise.
- The start-up context (about 62k tokens here) belongs to the machine the test
  ran on: its client, skills and connected tools. Yours will differ.
- Costs are list prices applied to measured tokens, shown with the start-up
  context already cached ("warm") and not cached ("cold").
- Round 2 timings were taken while another job held every processor core, so
  its wall-clock seconds mean little. Token counts do not depend on load.
