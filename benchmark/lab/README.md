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
