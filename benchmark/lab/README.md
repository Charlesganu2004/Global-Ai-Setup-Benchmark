# Search lab

A measuring instrument for `pgr find`. It needs no model and no human labels: each challenge is made from a corpus itself and has a known right answer. It scores any number of search variants on the same challenges and reports recall at 1, 5 and 10 and mean reciprocal rank (0 when the answer is not in the 20 hits asked for), with 95% bootstrap intervals (2,000 resamples, fixed seed). Differences between variants are paired: same challenges, same resamples. The result is `benchmark/data/search_lab.json`.

## Challenges

| kind | query | right answer |
|---|---|---|
| T | the words of a test name left after the tested function's name is taken out, plus the test's first doc line | that function |
| C | a commit subject without tag, issue numbers and trailing punctuation | any hit in a source file the commit changed |
| D | a function's own doc line; a control, never pooled with T and C | that function |

- **T**: the test name holds, as whole tokens (split on underscores and capitals), the name of exactly one non-test function; that function is defined once, called from at most 40 places (a function used everywhere is set-up in most tests), and called by the test (call graph, or for a name of two or more tokens a call in the body). A one-token name must open the test name and be a graph call, because in a sentence-style name it is mostly a coincidence of English words. At least 2 query words remain (test, tests, it, should dropped).
- **C**: a non-merge commit with 1 to 3 changed source files, all at HEAD, in at most 8 changed paths; 3 words and 3 content words (not a stop word or a generic verb such as fix, add, use). Dropped first: bump, release, version numbers, revert, format, whitespace, lint, clippy, rustfmt, prettier, black, isort, pre-commit, typo, spelling, changelog, merge, dependabot, dependency, wip, ci, github actions, CVE ids. Shallow-clone boundary commits are skipped.
- **D**: a doc line of 3 or more words, unique in the corpus. Word search should score near perfect: the text is in the index. A low score means the harness is broken.
- **All**: a query never holds the full name of its answer (identifier or whole-token run, underscores ignored; for C a file name, path or definition name of the answer files). A query shared by two answers is dropped. Answers are never in test code or in example, doc, docs_src, compat or legacy folders (the engine scales most of these down on purpose). At most 400 per kind and corpus, sampled with a fixed seed; the id is 64 bits of a SHA-256 of corpus, kind, query and answer, odd means held-out, even means tune. Metrics are reported for held-out. The funnel (candidates rejected by each rule) is in the result file.
- **Tests** are skipped before ranking, so a test cannot find itself: a `test`, `tests`, `testing`, `__tests__`, `spec` or `specs` folder; a file named `test_*`, `*_test`, `*_tests`, `*.test.*`, `*.spec.*`, `tests`, `test` or `conftest`; in Rust, a `#[cfg(test)]` module or a `#[test]` function.
- **Credit** for T and D ("cover", the main numbers): the function's row, a chunk overlapping it or the comment lines above it, or any hit in the same file whose lines contain it (the class or impl block the engine returns for a method). "Exact" leaves out the containing block and is kept beside it. C is file-level.

## Run

Use `python -I`. `--lab` (or `SEARCH_LAB_DIR`) is the work folder; corpora, index home, cache and challenges default to folders in it. Corpora are only read as text; nothing in them runs, and `LXR_HOME` points into the lab so your own index home is untouched.

```
python -I search_lab.py prepare    --lab C:\path\to\lab --pgr C:\path\to\pgr.exe
python -I search_lab.py challenges --lab C:\path\to\lab --pgr C:\path\to\pgr.exe [--show 10]
python -I search_lab.py run        --lab C:\path\to\lab --pgr C:\path\to\pgr.exe
python -I search_lab.py report
```

`prepare` makes five corpora (the benchmark fixture, this repository from `git archive HEAD`, click, express and ripgrep cloned at depth 400), indexes them, and records a tree hash; `verify`, also run by `challenges` and `run`, fails if a corpus file changed. `challenges` writes the sets under `LAB/challenges` (not into the repository); make them once with one binary, because the result file refuses a different set without `--reset`.

Variants are rows: `--variant NAME=ARGS`, extra `pgr find` arguments, with `@pgr=PATH` for another binary and `@env:KEY=VALUE` for an environment variable (forward slashes). Default: `words=--words`, `meaning=--meaning`, `hybrid=`. `--variants-file` takes a JSON list of `{name, args, pgr, env}`. A new binary needs its own index: `prepare --pgr NEW --home NEWHOME`, then `@env:LXR_HOME=NEWHOME`. `run` adds or replaces only the variants it is given; `--half tune` scores the tune half (use `--out` for a file of your own). Answers are cached by binary hash, corpus commit, index content, variant and query; `--fresh` ignores the cache.

## Tune

`tune` searches the fusion weights of `pgr find` (the variable `PGR_SEARCH_WEIGHTS`) with the lab's own challenges, with no model and no human labels:

```
python -I search_lab.py tune --lab C:\path\to\lab --pgr C:\path\to\pgr.exe --variants-file variants_methods.json [--select 2]
```

- It scores every base variant at the engine's own weights on the **tune half only**, by the mean of the pooled MRR of T and C (D is a control and tunes nothing), and tunes the best `--select` of them. The choice of what to tune is made on the tune half too.
- The search moves one parameter at a time over a small fixed grid (`TUNE_GRID`; for a variant with `PGR_SEARCH_REFINE` the number of hits the second pass learns from is one more parameter), for two sweeps, in an order a fixed seed decides. A value is taken only when it gains at least 0.002 MRR, and the search starts at the engine's values, so a tuned variant never scores below the default on the half it was tuned on.
- It writes the search with its trace (`tuned.json`) and the tuned variants as `NAME-tuned` (`tuned_variants.json`), which `run --variants-file` reads. Judge them with `run` on the held-out half. The weights are in the `env` of the variant's row.
- `variants_methods.json` holds the variants of the study of the search methods that need no model: `PGR_SEARCH_REFINE` (a second pass that adds the rarest words of the best hits), `PGR_SEARCH_GRAPH` (the definitions a test or a thin wrapper calls follow it), `PGR_SEARCH_VECTORS` (vectors made from the repository itself). The repository vectors are made when the index is built with the variable set, so build the lab index once with `PGR_SEARCH_VECTORS=repo pgr refresh --force`.

## Search methods that need no model (2026-10-14)

Four switches of `pgr find`, all off by default (`rust/crates/lxr/README.md`), measured here on the held-out half with `pgr 0.2.0`: `PGR_SEARCH_REFINE` (a second pass with the rarest words of the best hits), `PGR_SEARCH_GRAPH` (the definitions a test or a thin wrapper calls follow it), `PGR_SEARCH_VECTORS` (vectors made from the repository itself) and `PGR_SEARCH_WEIGHTS` (the numbers of the fusion). Rule fixed before the run: a method is recommended as a new default only if its held-out MRR on the commit-subject challenges (C) beats `hybrid` with the paired interval above zero, nothing else (R@1, R@5, R@10, MRR of T, C and D pooled, MRR of C per corpus) gets worse beyond its interval, and it keeps the speed targets. Pooled MRR, difference against `hybrid` in brackets, `*` where the paired interval excludes zero:

| variant | C MRR (n=213) | T MRR (n=25) | D MRR (n=715, control) |
|---|---|---|---|
| `words` | 0.755 (+0.024) | 0.260 (-0.118) | 0.818 (+0.004) |
| `words-refine` | 0.748 (+0.017) | 0.263 (-0.114) | 0.629 (-0.185*) |
| `words-graph` | 0.753 (+0.022) | 0.421 (+0.043) | 0.815 (+0.001) |
| `words-repo` | 0.735 (+0.004) | 0.307 (-0.070) | 0.850 (+0.036*) |
| `words-all` | 0.728 (-0.003) | 0.473 (+0.095) | 0.748 (-0.066*) |
| `hybrid` | 0.731 | 0.378 | 0.814 |
| `hybrid-refine` | 0.728 (-0.003) | 0.400 (+0.023) | 0.699 (-0.115*) |
| `hybrid-graph` | 0.728 (-0.003*) | 0.442 (+0.064*) | 0.811 (-0.003) |
| `hybrid-repo` | 0.748 (+0.018) | 0.433 (+0.056) | 0.864 (+0.050*) |
| `hybrid-all` | 0.746 (+0.015) | 0.508 (+0.131) | 0.761 (-0.052*) |
| `words-graph-tuned` | 0.768 (+0.038) | 0.498 (+0.121) | 0.921 (+0.107*) |
| `hybrid-all-tuned` | 0.747 (+0.016) | 0.413 (+0.035) | 0.975 (+0.161*) |
| `words-weights` (the tuned weights of `words-graph`, no graph) | 0.771 (+0.040*) | 0.263 (-0.114) | 0.920 (+0.106*) |

What the numbers allow:

- **No variant meets the rule.** The closest, `words-graph-tuned`, has C +0.038 with the interval [-0.001, +0.076]; nothing is worse beyond its interval for it. `words-weights` has C +0.040 with the interval above zero but loses T recall at 10 (-0.200 [-0.360, -0.040], as `words` does).
- **Refinement does not help** and costs the control: D falls by 0.185 (words) and 0.115 (hybrid) MRR and C moves by less than 0.01.
- **The graph step helps the test-name challenges** (T +0.064* on hybrid, +0.043 on words) and leaves D and C alone (C -0.003* on hybrid, one corpus). T has 25 challenges: read it as a direction.
- **Repository vectors are a working meaning channel without a model.** Next to the model's vectors they add D +0.050* and about +0.02 on C and +0.06 on T (intervals include zero). Alone (`words-repo`) they are about as good as `words`.
- **Tuning**: `words-graph` was tuned to `files=0,k=30` on the tune half (tune objective 0.564 to 0.649) and the weights carry over (C 0.753 to 0.768, T 0.421 to 0.498); `words-weights` shows the C gain is the weights, not the graph. `hybrid-all` was tuned to `doc=1,files=2,k=30,meaning=2,names=2,terms=4` (0.544 to 0.583 on the tune half) and gained nothing held-out (C +0.001, T -0.095): it fitted a half of 18 T and 201 C. Its D of 0.975 is the documentation-line boost matching D queries that are documentation lines by construction; D is no evidence for it.
- **The no-model product** (`PGR_NO_MODEL=1`, an index without `vectors.bin`) gives the same rank strings as the same variant with the model present (`nomodel-words-graph`, `-tuned` and `-all`: 24 of 24 cells each). On the fixture the program is 8,506,368 bytes against 42,011,648 with the model, the index 954,920 bytes against 5,913,118 (the repository vectors, 4,971,159 bytes, are only needed by variants that fuse meaning).
- Timings in the result file were taken while a 4K render and other jobs ran: they are not a ranking.

## Limits

- Queries come from developer-written test names and commit subjects, not from questions an agent would ask. A test-name query shares words with its answer by construction.
- C credit is file-level and lenient, and answers pile up: the commonest file answers 40 to 53% of C in click, express and ripgrep (`stats.c_top_file_share`).
- T is small (the rules leave few challenges; see the funnel) and mocha tests have no function names, so express is judged on C alone. Read T intervals before T differences.
- Five corpora, three languages; pooled rows weight each challenge equally, so bigger corpora count more.
- Cover matching rewards a variant that returns coarser blocks; read it beside the exact numbers.
- The hybrid variant was built by people who had seen none of these challenges. Any variant tuned later on the tune half must be judged on the held-out half only.
- Timing is a median of 40 serial calls on a shared machine; two runs gave 50 to 110 ms and a different order of variants. Read it as "about 50 to 100 ms", not a ranking.

## Fairness read

A person read challenge set `322e1c1e` (`challenge_sets_sha256` in the result file): all 43 T, and 32 C and 47 D chosen at random. No wrong answer found; about 10 T and 2 C queries are thin (two or three generic words), and one T query repeats most of its answer's name in its doc line. D is right by construction. An earlier pass, before the rules above were tightened, found 15 of 30 T in ripgrep, pgr-engine and click matching only by a coincidence of English words (`first`, `written`, `root`) and 4 of 36 C subjects unanswerable from their words ("refactor try/except"). The rules were tuned on these reads, so the read checks the rules; it is not an independent test.
