# Second harness: the Codex CLI

The main benchmark measured pgr under one agent harness, Claude Code subagents. This check asks the same lookup questions of a GPT agent run by the Codex CLI, with and without pgr, and stores the result in `benchmark/data/codex.json`. It is a second look, not a replacement. Its numbers are never mixed with the main benchmark's.

## What it measures

One Codex session answers the three questions of one set, through a structured answer. Two methods, two question sets, three repeats each: 12 sessions, run one after another.

- **no-index**: the agent may search only with ripgrep (`rg`) and read only by printing line ranges of files.
- **pgr**: the agent may use `pgr ask <kind>:<what>...` and print a line range of a file only when an answer needs it. `rg` is allowed only when pgr returns nothing useful, as in the main benchmark's policy.

Question sets A and B, their answer schemas and the one-line description of `pgr ask` are read from `token-benchmark.workflow.js` at run time (`Q`, `Q_B`, `S`, `S_B`, `ALL`, `ALL_B`, `PGR_ASK`). The truth and the grader are `TRUTH`, `TRUTH_B` and `grade` from `bench_parse.py`. There is no second copy to drift. A set A or B answer is fully correct when the mean of its three question grades, rounded to four places, is at least 0.999, the main benchmark's rule.

The repository is the same fixture as the main benchmark: 86 Python files, 68,316 lines. It is read-only, pgr runs with `PGR_NO_REFRESH=1`, and the script compares a listing of every file in it before and after the runs (`fixture.changed_by_runs` must be empty).

## How to run

```
python -I benchmark/harness/codex_bench.py --selftest
python -I benchmark/harness/codex_bench.py --dry
python -I benchmark/harness/codex_bench.py --fixture C:\path\to\lxbench\repo --pgr C:\path\to\pgr.exe --model MODEL --effort medium --date YYYY-MM-DD [--codex PATH] [--scratch DIR] [--resume]
```

- `--selftest` checks the grader against the truth (a perfect answer is fully correct, a wrong one is not), the leak check and the command judge, including commands as the CLI really printed them. It calls nothing.
- `--rejudge --fixture DIR --scratch DIR` judges the commands of the stored sessions again from the event streams saved in the scratch folder, after the judge changed. It calls nothing and leaves tokens, answers and grades alone.
- `--dry` builds both prompts and the schema files and prints them with the leak check and the run order. It calls nothing.
- A real run needs `--fixture`, `--model` and `--date`. `--codex` defaults to the newest `codex.exe` under `%LOCALAPPDATA%\OpenAI\Codex\bin`. The model name is never in the script: it is an argument and is recorded from what the CLI reports.
- One trivial call without `--json` comes first, only to read the header the CLI prints (version, model, effort, sandbox, approval). Its tokens are not in any cell.
- Each session is `codex exec -m MODEL -c model_reasoning_effort=EFFORT --disable hooks --disable multi_agent -s read-only --skip-git-repo-check --ephemeral -C FIXTURE --output-schema SCHEMA -o OUT --json -`, with the prompt on standard input. The user's hooks are off because they run scripts and rewrite shell commands; sub-agents are off because the policy forbids them. Everything else is the user's normal Codex setup, including the global instruction file, which the prompt overrides in the same words the main benchmark uses.
- A session over 8 minutes is killed with its child processes and recorded as `timeout`.
- When the CLI says the usage limit is hit, the run stops, the session is not recorded as a measurement, and `usage_limit` holds the message and the time to try again. `--resume` keeps the sessions already stored and runs the missing ones.
- The order is by pair: one set and repeat, both methods back to back, and the method that goes first alternates from pair to pair (`--dry` prints the 12 steps). A stop leaves at most one unpaired session.

## The two prompts

They differ only in the allowed-commands block. The text below is set A; set B has the same text with its own three questions (listed in `codex.json`). The script fails the run if a prompt contains a truth value that its questions do not already contain.

```
This is one run of a benchmark that measures how many tokens a code lookup costs.
Repository under test: your working directory (Python, 86 files). It is read-only: never edit, create or delete anything in it, and never build or refresh an index. Stay inside it: never read, list or search any path outside the working directory.

TOOL POLICY FOR THIS RUN. It replaces the local-index and read-gate rules and any token-goat gate in your global instructions. Use only what is listed here. No sub-agents, no web access, no other search or index command.
Allowed shell commands. Each is one call of your shell tool (PowerShell); the working directory is already the repository root.
  rg <pattern> [path...]     ripgrep, the only search command: searches file contents by regular expression; rg --files lists files
To see code, print a line range of a file with Get-Content <file> | Select-Object -Skip <first line minus one> -First <count>, and only when the answer needs it.
Be exact and do not guess. Stop as soon as the work is answered, then return the answer through the structured output.

Answer all three questions.
Q1. Which functions call `_get_backend_api_key`? Give the bare name of every calling function in the repository.
Q2. Where is the function `dispatch_command` defined (file, and the line its `def` is on), what are its parameter names, and which functions call it?
Q3. Which function registers the git merge driver for graph.json? Give its name, its file, the line its `def` is on, and the name of the function that removes that registration again.
```

```
This is one run of a benchmark that measures how many tokens a code lookup costs.
Repository under test: your working directory (Python, 86 files). It is read-only: never edit, create or delete anything in it, and never build or refresh an index. Stay inside it: never read, list or search any path outside the working directory.

TOOL POLICY FOR THIS RUN. It replaces the local-index and read-gate rules and any token-goat gate in your global instructions. Use only what is listed here. No sub-agents, no web access, no other search or index command.
Allowed shell commands. Each is one call of your shell tool (PowerShell); the working directory is already the repository root.
  pgr ask <kind>:<what>...     several questions in one call, each answer under its own "== kind:what" header. Kinds: about (definition, signature, doc line, callers and what it calls together), callers, def, affected, and find (search by meaning; quote the whole argument). Example: pgr ask about:parse_config callers:load "find:where tokens expire"
After a command answers, print a line range of a file with Get-Content <file> | Select-Object -Skip <first line minus one> -First <count> only on a line range it returned, and only when the answer needs it. Use rg only when the listed commands return nothing useful.
Be exact and do not guess. Stop as soon as the work is answered, then return the answer through the structured output.

Answer all three questions.
Q1. Which functions call `_get_backend_api_key`? Give the bare name of every calling function in the repository.
Q2. Where is the function `dispatch_command` defined (file, and the line its `def` is on), what are its parameter names, and which functions call it?
Q3. Which function registers the git merge driver for graph.json? Give its name, its file, the line its `def` is on, and the name of the function that removes that registration again.
```

## Fields in `codex.json`

Top level
- `date`, `harness`, `cli_version`, `auth`: when it ran, `codex exec`, the version `codex --version` prints, the sign-in the CLI reports.
- `model`, `effort`, `sandbox`, `approval`: as the CLI printed them in its header. `requested` holds what the script asked for.
- `fixture`, `pgr_version`, `prompts`, `schemas`, `command_template`, `environment`, `feature_switches_off`: what ran. Paths are replaced by `C:\path\to\lxbench\repo`.
- `limits`, `not_measured`, `usage_limit`: what the result does not show, and the usage-limit stop if there was one.
- `sessions`, `summary`, `pgr_saving_vs_no_index`: the rows, the cells, and pgr against no-index per set.

Per session
- `method`, `set`, `repeat`, `order`: the cell and its place in the run.
- `answer`, `grades` (q1 to q3, from `bench_parse.grade`), `mean_grade`, `fully_correct`, `schema_errors`.
- `usage`: `input_tokens` (cached ones included), `cached_input_tokens`, `output_tokens` (reasoning included), `reasoning_output_tokens`, `total_tokens` (input plus output) and `fresh_tokens` (input minus cached plus output, which is what the CLI prints as `tokens used`). The CLI reports one usage record per session, summed over its model requests.
- `turns` is the number of `turn.completed` events, which is one. `requests` is null: the events do not give a request count.
- `commands`: `count`, `programs` (how often each program ran) and `list`, each with the command as the CLI reported it (the PowerShell wrapper removed; the CLI doubles backslashes and rewrites the quoting around a pattern that holds a `^`), its exit code and the size of its output.
- `off_policy`, `off_policy_commands`: a command outside the method's allowed set, with the reason. See below.
- `wall_seconds`, `failure` (`timeout`, `cli_error`, `no_usage` or `schema_failure`, with a detail line), `warnings`.

Per method and set (`summary`): `sessions`, `with_usage`, `mean_total_tokens`, `mean_fresh_tokens`, `mean_cached_input_tokens`, `mean_output_tokens`, `mean_commands`, `mean_grade`, `fully_correct` of `of`, `off_policy_sessions`, `failed_sessions`, `mean_wall_seconds`. Means are over sessions with usage; a failed session without usage counts in `of` and in no mean.

`off_policy` is a first-word check of the pipeline stages of the PowerShell command, nested groups included. The first word of a command must be allowed; a later stage is flagged only when it is plainly another program (a known tool, a `Verb-Noun` cmdlet, a name ending in `.exe` or `.py`), because the CLI's quoting of a `^` pattern can leave regex text that looks like a command. Allowed: `rg`, line-range printing (`Get-Content`, `cat`, `type` with a range or `-First`/`-Skip`/`-TotalCount`/`-Tail`, `sed`, `head`, `tail`), plus pipeline helpers (`Select-Object`, `Where-Object`, `Set-Location`). The pgr method also allows `pgr ask`. Anything else (`pgr` under no-index, another `pgr` verb, `git`, `python`, `Select-String`, `Get-ChildItem`, `lx`), a whole-file `Get-Content` without a range, a redirect to a file, or a path outside the fixture is flagged. It is a heuristic: a command hidden inside a script is flagged as the interpreter, and an unknown tool in a later stage is not seen.

## Limits

- Three repeats a cell support no claim of significance.
- One repository.
- Token counts come from a different harness, with a different system prompt, different tools and different caching than the main benchmark, so the two are not comparable. Only no-index against pgr inside `codex.json` is. About 26,000 input tokens per request are the harness's own baseline (system prompt, tool definitions, the user's global instruction file) and are the same in both methods.
- The questions were written by the builders of pgr.
- Not measured: model requests per session, other models or efforts, other repositories, orchestration or parallel agents, pgr's memory part.
