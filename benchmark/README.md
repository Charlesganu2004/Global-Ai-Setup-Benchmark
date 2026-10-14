# Benchmark

What a code lookup costs an AI coding agent in tokens, measured for every way
this setup can answer one: no index, each engine alone, the `lx` command in
each of its builds, and combinations of them.

- [RESULTS.md](RESULTS.md) has every table.
- The charts are in the full report (the `report/` folder: in this repository, not served by the site). The public site (the `docs/` folder, served by GitHub Pages) shows measured outcomes only.

## What is measured

Three questions about one repository, each with a ground truth taken from
Python's syntax tree:

1. Who calls one function (seven callers across four files).
2. Where another function is defined, what parameters it takes and who calls it.
3. Which function registers a git merge driver, and which one removes it.

Round five added a second set of three questions of the same three shapes, the held-out set
(set B, the questions above are set A). The same team wrote them, later, and the people and agents
who built the engine did not see them while building it. They are not an independent test: same
repository, same generator, same three shapes. An arm id that ends
in `h` is its base arm run on set B (`RGSh` is `RGS`); an id that ends in `r` is its base arm run
again on the day of round five (`A0r` is `A0`). Every cell and every agent row in `results.json`
carries `"set": "A"` or `"set": "B"`.

An agent is told which commands it may use (one "method") and answers through a
structured output, so grading is a script against an answer key that people fixed by hand
(the question about which function does a job has a hand-written key). Each method is run
on three model tiers and in three setups: one agent, three subagents started
together, and an orchestrator that briefs three subagents and checks their work.

Two more things were varied on a handful of methods. The reviewer of an
orchestrated run was swapped for the same model without tools, the mid-tier
model and the cheapest model. And five methods were run a second time while
token-goat's hooks were installed in the client, to see what the hooks change.

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
    hooks.json                    what token-goat's hooks are: events, matchers, the block they add
    engine.json                   speed of the first-generation engine and of pgr, with a `public` list and a `how` text
  harness/
    token-benchmark.workflow.js   the agent runs: methods, prompts, tiers, setups
    arms.json                     what each method is and the commands it was given
    truth.py                      ground truth for the questions, from the syntax tree
    bench_parse.py                transcripts to results.json: tokens, cost, tools, grades
    build_site.py                 results.json to the full report (report/), the public site (docs/) and RESULTS.md, then a scan that stops the build if docs/ holds anything that must stay off the site
    combo_bench.py                build time, lookup time and answer size per engine
    emb_bench.py                  the same with the Rust index's embeddings on, and search quality
    lean_bench.py                 the combined index with and without the graph
```

## Run it again

The harness runs inside Claude Code, because the thing being measured is a
Claude Code agent with its real start-up context.

1. Make the fixture. The three questions and their graded answers are written
   for ours: the source tree of graphify 0.9.61 (86 Python files, 2.97 MB),
   committed once. Pass its path as `repo` in the workflow's arguments, and as
   `repoPlain` a second copy indexed without Rust embeddings, for the methods
   that need one. Neither path may contain a space: the prompts put it into
   a shell command unquoted. Another repository needs its own questions in the workflow
   script and its own answers in `TRUTH` in `bench_parse.py` and in
   `arms.json`; `python benchmark/harness/truth.py REPO NAME...` prints, from
   the syntax tree, where each named function is defined and who calls it.
2. Build every index the methods use, from the fixture's root: `lx refresh`
   (with `codanna_embeddings` on for the methods that search the Rust index),
   `codanna init` then `codanna index .` for the methods that use codanna
   directly, and `token-goat index --embed` for the token-goat methods.
3. Ask Claude Code to run `harness/token-benchmark.workflow.js` as a workflow,
   passing the cells to run, for example
   `{"repo": "PATH", "matrix": [{"arms": ["A0", "S1"], "setup": "single", "tier": "C", "reps": ["r1", "r2", "r3"]}]}`.
4. Parse the transcripts it leaves behind, then rebuild the report and the public site:

```
python benchmark/harness/bench_parse.py benchmark/data/results.json TRANSCRIPT_DIR...
python benchmark/harness/build_site.py
```

To measure the hooks, run `token-goat install --no-index`, run the methods
whose codes start with H (and HB), run `token-goat uninstall`, then run their
twins (the `twin` field in `arms.json`, and SB). The client applies a hook
change at once. Agents started from a session that was already open keep the
rules file they loaded, so the block token-goat adds to it is not in their
context. The two sides of a pair share one prompt but not one start-up
context: each round's agents inherit what the session had loaded when the
round ran, which differs by a few hundred tokens between rounds. To compare
like with like, run both sides in one session, one right after the other.

The index timings come from the three `*_bench.py` scripts. Each takes a name,
a source folder, a work folder and the path to codanna, prints JSON, and
expects the tools where `setup/tools.py` puts them on Windows; `index.json`
holds what they printed.

## Reading the numbers

- One repository, one language, three easy questions in each of two sets. Most
  cells are one to six runs; the cells of round five are three runs, a few two.
  A difference of one turn between two methods is noise, and so is a difference
  of one fully correct run.
- The start-up context (about 63k tokens here) belongs to the machine the test
  ran on: its client, skills and connected tools. Yours will differ. It grew by
  about 2k tokens between the earlier session and round five, which is why the
  public leaderboard shows the same-day repeats A0r, S1r and S3r and not A0, S1
  and S3.
- In round five the control's file reads passed through token-goat's hooks. Its
  secret-redaction hook overwrote the two lines that call the first question's target
  (the target's own name contains "api_key"), so the control left that caller out in four of
  its six original-question runs. A0, from the earlier session, ran without the hooks. The
  control's fully correct counts for both are in the Round five section of RESULTS.md, and
  the public site does not set them against the product's. It compares the product with the
  control on tokens and turns only, and compares fully correct counts only with the first
  generation, which ran in the same session. Rerun the control with the hooks off before
  using its correctness for anything.
- Three runs in a cell cannot support a claim of significance (the smallest exact two-sided
  p-value for three runs against three is 0.10), and the three questions of a set are asked in
  one prompt, so the runs of a cell are repeats of one task. Every finding on the site says "in
  these runs" or gives the count.
- Agents are told different amounts. Each product arm gets a description of its own commands,
  several sentences long, and the control gets a line of tool names. The descriptions were
  not controlled for length or detail.
- The headline saving against no index is not one cell. The site gives the range over the four
  cells where the highlighted configuration and the control were both measured (light and mid
  class, original and held-out questions) and labels any single cell it shows.
- RESULTS.md ends with the section "Known weaknesses of the study": the method review's
  weaknesses, each with what it could change and the cheapest fix.
- The memory of RM, RGM, RSM and RGSM was filled beforehand by a session that
  answered the same three questions, which is a best case; RGSO and RGSOh by a
  session that explored the repository without seeing any question.
- Costs are list prices applied to measured tokens, shown with the start-up
  context already cached ("warm") and not cached ("cold").
- Round 2 and part of round 4 ran while other jobs held every processor core,
  and a row can mix runs from several rounds, so wall-clock seconds are a
  rough guide and never comparable across rounds. Token counts do not depend
  on load.
- A method measured with the hooks on is its own row, named "hooks on", and is
  never averaged with its twin. Where the hooks did not run once in a run, a
  difference from the twin is the spread between repeats, not the hooks.
- One round was stopped part-way and resumed, and the resumed round ran most
  of its agents a second time. Where an agent has two transcripts,
  `bench_parse.py` counts the later one and leaves the earlier one out. A run
  whose later transcript never reached an answer is listed as not finished.

## The public site

`docs/` is served by GitHub Pages and `report/` is not (it is in the repository). `build_site.py` writes both, and `docs/` carries
only what `arms.json` marks with a `public` object (a slug, a generic name, a family and an
order), the measured numbers for those methods, and findings in plain sentences. A held-out arm
(`group` `heldout`) has no `public` object: its points join the public configuration of the public
arm that shares its `base`, as question set "held-out", and the points of the public arm itself are
question set "original". The three repeats carry the public objects of A0, S1 and S3, so the
leaderboard compares runs made on the same day. The highlighted configuration, called "Most
reliable" on the site, is the one with the highest share of fully correct runs among those run at
least three times in each of the four cells (both question sets, light and mid class), a tie going
to the lower mean tokens over the four cells; when no configuration has all four cells the rule
falls back to the light class on the original questions. The rule was written after the fifth
round's results were known, and the site says so; it was then applied unchanged to round six, which
ran after it was written, and in round six it picks the brief pack with no lookup command. Round six
(arm group `team`, steps in `harness/ROUND6.md`) is eligible like any other round. Under each
leaderboard view the build says in a line which other configurations used fewer tokens.
Two data files add to it:
`benchmark/data/engine.json` (optional: a `public` list of rows with `name`, `build_seconds`,
`query_ms` and `index_mb`, shown as an engine speed table; without it the site has no table) and
`benchmark/data/install.json` (a `public` object with `status` `public`, the release links, the two
installers, the program files and wheels with their sizes, the first commands, the rule for an
agent and what was tested). The package is public, so the Install page and every install
command on the site are built from that file; a new release is one edit of it.

The last step of every build scans every file in `docs/`, its file names and its caption and data
files included, for tool names, vendor and model names, commands, flags, paths of the setup,
internal method ids and text copied from `arms.json`. Words are matched in any case, across line
breaks and after character references, escapes and base64 are decoded; class names, ids and
identifiers in the scripts and style sheets are split into their parts and checked the same way.
It also checks that every configuration name and slug in the public data comes from a `public`
object in `arms.json`. It exits with the file and the text it found. The build then plants one
violation for each rule in a scratch copy of `docs/` and fails if the guard lets any through. Run
the guard alone on any folder with `python benchmark/harness/build_site.py --guard FOLDER`, and
the self-test alone with `--guard-selftest`.
