# pgr (crate `lxr`)

Private developer notes. `pgr` is the command line of PeterGanuRusty: a code graph, hybrid
code search and memory for the repository a coding agent works in. One program, started
once per question. The contract it follows is the CLI contract `pgr-cli.txt` (version 0.1) and
its additions in `pgr-hub.txt` (version 0.2: the team hub and brief packs, described below).

## Build and test

Plain `cargo` cannot link on this machine. Use the wrapper in `rust/`, from PowerShell:

    cmd /c "powershell -NoProfile -ExecutionPolicy Bypass -File rust\cargo.ps1 build --release -p lxr 2>&1"
    cmd /c "powershell -NoProfile -ExecutionPolicy Bypass -File rust\cargo.ps1 test -p lxr 2>&1"

The release binary lands at `rust\target\release\pgr.exe`. The package is still called
`lxr`; the binary target is `pgr` (`[[bin]] name = "pgr"`). Other builders compile in the
same target folder, so cargo may wait on a file lock.

The tests (`tests\*.rs`) run the built binary on a small Python project they write into a
temporary folder. Every run sets `LXR_HOME` to a folder inside that workspace, so the real
index is never touched, and hides the embedding model, so answers are made of words only.
`tests\embeddings.rs` is the one exception: it uses the model when the machine has one.

### The model inside the program

With the cargo feature `embedded-model` the program carries the embedding model inside it
(`include_bytes!` of `tokenizer.json` and `model.safetensors`), so it finds the model however the
executable was installed or copied. The build needs the environment variable `PGR_EMBED_MODEL_DIR`,
the absolute path of the folder that holds the two files; without it the build stops with an
error that says so (`build.rs` checks the folder, `src/model.rs` carries the message).

    $env:PGR_EMBED_MODEL_DIR = "C:\path\to\model-folder"
    cmd /c "powershell -NoProfile -ExecutionPolicy Bypass -File rust\cargo.ps1 build --release -p lxr --features embedded-model 2>&1"

The program is about 40 MB then. `packaging\build-wheel.ps1` does this for the pip package.
`tests\embedded_model.rs` exists only in such a build:
`cargo.ps1 test -p lxr --features embedded-model --test embedded_model`, with the variable set.
Without the feature nothing changes. The search order at run time is in `src/model.rs`.

## Commands on one screen

    pgr [--root DIR] [--engines graph,search,memory] [--bank NAME] [--task NAME] [--from ROLE] COMMAND ...

    refresh [--force]            build or update the index, print the status line
    status                       print the status line, never builds
    def NAME...                  one row per definition:  NAME PATH:FIRST-LAST SIGNATURE
    callers NAME...              N callers of NAME:  CALLER PATH:FIRST-LAST @lines
    about NAME...                row, doc, callers, what it calls
    affected NAME [--depth N]    what a change can reach (default depth 2)
    path A B                     shortest chain of links
    hubs [-k N]                  most connected definitions
    explain NAME                 container, members, bases, callers, callees
    query QUESTION [--budget N]  graph neighbourhood for a question (600 tokens)
    map [--budget N]             the largest groups of the code
    find WORDS... [-k N] [--words | --meaning]
    retain TEXT... [--kind world|experience|directive] [--tag T]... [--context C] [--when DATE] [--whole]
    recall WORDS... [-k N] [--budget low|mid|high] [--max-tokens N] [--tag T]... [--since DATE] [--until DATE]
    reflect WORDS...
    memory list [-k N] | forget ID | stats | path
    handoff TEXT... [--kind world|experience|directive] [--tag T]... [--whole]   (needs --task; TEXT - reads standard input)
    loadout [WORDS...] [--budget N]   what to load before starting (600 tokens)
    ask KIND:WHAT...             def callers about affected explain find recall path (path is A>B)
    pack TASK [--budget N]       == names, == find, == graph, == memory (800 tokens)
    mcp                          MCP server on standard input and output
    --version | -V, help | --help | -h

Exit codes: 0 answered (a name that is not found is still 0), 2 usage error or engine
off, 1 the index cannot be read or built. Standard output is the answer; every diagnostic
goes to standard error. Dates are `2026-10-12`, `2026-10-12 14:30` or `2026-10-12T14:30:05Z`.

## Environment variables

| Variable | Effect |
| --- | --- |
| `LXR_HOME` | Folder that holds every index. Default `%USERPROFILE%\.local-index\rust`. |
| `LXR_MODEL_DIR` | Folder of the embedding model (`model.safetensors`, `tokenizer.json`). Tried first; then the model compiled into the program, if there is one; then the places below. |
| `PGR_ENGINES` | Same as `--engines`; the flag wins. |
| `PGR_NO_REFRESH` | `1`: answer from the index as it is; no index at all is exit 1. |
| `PGR_LOCK_WAIT_MS` | Milliseconds a refresh waits for the lock of another process. Default 30000. |
| `PGR_BANK` | Same as `--bank`; the flag wins. A name of 1 to 64 characters out of `A-Z a-z 0-9 . _ -`, not `.` or `..`: use `LXR_HOME\banks\NAME` as the memory bank of every repository. Any other value, `..\x` or `a\b` included, is a usage error (exit 2). An empty variable counts as not set. |
| `PGR_TASK` | Same as `--task`; the flag wins. The task a `handoff`, `loadout` or `pack` belongs to. Same rule for the name. |
| `PGR_ROLE` | Same as `--from`; the flag wins. Who is calling. Default `agent`. Same rule for the name. |
| `USERPROFILE`, `HOME` | The home folder pgr refuses to index; also where the model is looked for. |
| `PGR_EMBED_MODEL_DIR` | Build time only, with the feature `embedded-model`: the folder of the model to compile in. |
| `HF_HUB_CACHE`, `HF_HOME` | Hugging Face cache, searched for the model last. |

## Team hub

Version 0.2 lets several agents, one after another or side by side, work on one task through a
bank they share, and puts what they know into the brief of the next one before it starts, so that
it does not spend a turn looking it up. The memory is read in layers, the durable and small ones
first:

| Layer | What it is |
| --- | --- |
| L0 raw | The repository itself. Notes point at definitions (anchors) and are checked against them: a note about code that has changed since is printed as stale, one about code that is gone as gone. A note has an anchor when it names code in a spelling the bank's entity rules know (see below). |
| L1 notes | What `retain` and `handoff` keep. |
| L2 task | The notes filed for one task, tagged `task:NAME`. |
| L3 core | Directives, and notes confirmed by repetition (seen at least 3 times) or by more than one role. |

Three global flags, before or after the command, with the variables `PGR_BANK`, `PGR_TASK` and
`PGR_ROLE` as defaults (the flag wins). A name is 1 to 64 characters out of `A-Z a-z 0-9 . _ -`,
and not `.` or `..`; anything else is a usage error, from the flag and from the variable alike,
so that a bank name can never leave the banks folder. A bank name must also be a folder name of
its own on every system, so on top of that rule it cannot end in a dot (Windows drops it, so `x.`
would be the bank `x`) or be the name of a Windows device (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to
`COM9`, `LPT1` to `LPT9`, with or without an extension). A task or a role is a tag and has the
rule alone. A bad `PGR_ROLE` or `PGR_TASK` is refused by `handoff`, `loadout` and `pack`, the
commands that read them.

    --bank NAME    use the bank LXR_HOME\banks\NAME, shared by every repository
    --task NAME    the task this call belongs to
    --from ROLE    who is calling (default agent)

Three commands:

- `pgr handoff TEXT...` files what an agent found for a task, for the next agent. It needs
  `--task`. The text is kept as `retain` keeps it (the same splitting into facts, the same
  near-duplicate rule, `--whole` for one note), and every note gets the tags `task:NAME` and
  `from:ROLE` and the context `handoff`. A fact that repeats an older note reinforces that note,
  and the note gains the two tags: this is how a fact becomes confirmed by a second role. `TEXT`
  that is the single argument `-` is read from standard input (UTF-8, at most 65,536 bytes). The
  last line says how many notes were filed and how many of them name code:
  `handoff NAME: N notes from ROLE, K name code`. A fact that repeats inside one text makes one
  note, which is counted once (and is seen more than once, as `retain` counts it).
- `pgr loadout [WORDS...] [--budget N]` prints what an agent loads before it starts, within a
  budget (600 tokens by default). It only reads: it never changes the bank, and the same input
  and the same bank give the same bytes, except for the ages. The layers come in this order, and
  one with nothing to print is left out: `== core` (directives, then confirmed notes, newest first,
  at most a third of the budget), `== task NAME` (with `--task`: every note of the task that core
  did not print, fresh ones first, then stale, then gone, and what the other layers leave of the
  budget), `== related` (with WORDS: at most four notes that `pgr recall WORDS` finds among the
  others, by words, by meaning and by the names they share, within a third). A summary the bank
  builds itself from three or more notes about one definition (a note of kind observation) is
  neither confirmed nor a note of the task, because the notes it covers print on their own. A line
  is the line of `recall` with the roles before the id. A note that does not fit what is left of
  its layer is left out and counted, and the notes after it are still tried; a layer the budget cuts
  ends with `  +K more`, and a layer that has no room for a note, its title and that line is left
  out, so that the loadout never costs more than its budget (below about 20 tokens that is nothing:
  `no notes`). A line costs the same at every age, so the ages move no cut. The closing line is `loadout: N notes, F fresh, S stale, G gone, about
  T tokens`; when S+G is above 0 it is followed by `check stale and gone notes against the code
  before relying on them`. Nothing to print: `no notes`.
- `pgr pack "TASK" [--budget N]` is the answer meant to be pasted into a brief: `== names` (the
  block `pgr about` prints for each code name in the task text that has exactly one definition:
  a name in backticks, a word with an underscore, a dot, `::` or an inner capital letter, or a
  word followed directly by `(`; at most four, using up to half of the budget), `== find` (without
  the hits that already have a block), `== graph`, and `== memory`. The three sections after the
  names share what is left in turn, and what one does not use passes to the next. With `--task`
  the memory section holds the core and task layers of `loadout`, without its closing lines.

The MCP server has the same: the tools `handoff` and `loadout`, and `for_task` in `pack`.

A worked example. Two agents look at a cache that is never emptied; the second one confirms what
the first found, and then the code changes:

    pgr --task cache-bug --from scout handoff "load_config keeps the parsed settings in a module cache and never reloads them."
    kept n000001
    handoff cache-bug: 1 notes from scout, 1 name code

    pgr --task cache-bug --from reviewer handoff "load_config keeps the parsed settings in a module cache and never reloads them"
    reinforced n000001 (seen 2 times)
    handoff cache-bug: 1 notes from reviewer, 1 name code

    pgr --task cache-bug --from scout handoff "reload_settings is the only way to empty the cache and nothing calls it."
    kept n000002
    handoff cache-bug: 1 notes from scout, 1 name code

    pgr --task cache-bug loadout          (after reload_settings was edited)
    == core
    - load_config keeps the parsed settings in a module cache and never reloads them.  (4m ago, seen 2 times) [reviewer+scout] #n000001
    == task cache-bug
    - reload_settings is the only way to empty the cache and nothing calls it.  (4m ago, stale: code changed) [scout] #n000002
    loadout: 2 notes, 1 fresh, 1 stale, 0 gone, about 75 tokens
    check stale and gone notes against the code before relying on them

The first note is in the core layer because two roles filed it; the second is in the task layer
and is marked stale because the function it names changed after the note was written.

The ideas of layers, of a loadout and of context put into an agent's brief before it starts come
from TencentCloud/TencentDB-Agent-Memory (MIT). No code of it is used, nothing here calls a model,
and pgr stays local: choosing, ranking and cutting are plain computation. What pgr adds is that
a note that names code is checked against the code graph. The notes that are checked are those
that name code in a spelling the entity rules of the bank know, when exactly one definition has
that name: a name in backticks, `snake_case`, `camelCase` or a dotted name such as `Store.reload`.
A plain word such as `main` or `slugify`, a call form such as `slugify()`, and the forms
`Store::reload` and `app/util.py::slugify` are not anchors, so such a note is never marked stale or
gone: the closing line counts it as fresh and, once the code has gone, it is still printed among
the fresh notes with no mark. Put the name in backticks to have it checked.

A bank shared by several repositories (`--bank`) is judged by the repository of the one who reads:
the anchors of a note hold a file and a name, and the graph that answers is the reader's. Several
checkouts of one project (clones, worktrees, branches) therefore read a note as fresh, stale or
gone by the code each of them has, which is the point of sharing the bank. A repository that does
not hold the code a note names reads the note as gone (the code is not there), however far apart
the repositories are; the line `check stale and gone notes against the code before relying on
them` is meant for that too.

## Index folder

`LXR_HOME\<name>-<hash of the path>\` holds `manifest.bin` (format number, version, build
time, counts, and path/size/time/hash of every file), `facts.bin` (stored facts of every
file), `graph.bin`, `search\` (chunks, words, vectors) and `memory\` (the bank, never
deleted by a rebuild). `refresh.lock` exists only while an index is being written; one
older than 600 seconds is ignored. Taking over such a lock, and giving a lock back, is
ordered by an operating system lock on `refresh.guard` (the file stays and holds nothing),
and a lock file holds a token, so a process never removes the lock of a successor.

How it stays current: every query command walks the repository once and compares sizes and
times with the manifest. A file is read and hashed only when its size is the same and its
time differs. Nothing changed: nothing else is done. Otherwise changed and new files are
extracted (in parallel when there are many), the stored facts of the rest are reused, the
graph and the search index are built again, and the manifest is written last. A query that
cannot make the index current (the lock is held for more than 30 seconds, a source file
cannot be read, a build fails) says so on standard error and exits 1; it does not answer.
A folder of the repository that cannot be listed is such a failure too: the files in it would
otherwise be taken for deleted ones and dropped from the index. The message names the folder
(`pgr: cannot read the folder app/private in proj: ...`), the index is left as it was, and
`PGR_NO_REFRESH=1` still answers from it.
`PGR_NO_REFRESH=1` is the one way to read the index as it is.

Publishing one generation: a build writes the new graph, the new facts and the new search
folder beside the live ones (`graph.bin.new-PID`, `facts.bin.new-PID`, `search.new-PID`),
then puts them in place, the search folder first and the manifest last. The manifest holds
the build time (the number of the generation), the size and hash of `graph.bin` and
`facts.bin`, and the search folder carries a file `generation` with the same build time.
A reader loads a piece and then checks it against the manifest it started from, so pieces
of two builds are never used together; if they disagree it waits for the lock, looks again
and builds the index only if they still disagree. A build that fails before it replaces
anything leaves the old index whole; one that fails after removes the manifest, so the next
query builds the index again. The search folder is the one piece that is replaced as a
folder: when another process has files in it open, the replacement fails with exit 1 and the
old index stays.

The memory bank is written under an operating system lock on `memory\pgr.lock` (or in the
`PGR_BANK` folder): a bank that is open is brought up to date inside the lock, so it holds every
note written until then, and a note is reported as kept only after the log was made durable. The
MCP server keeps its bank between requests and, before each one, looks at the size and time of
`notes.jsonl`; it reads the notes again only when another process wrote since.

A write has two ways to fail. The log that could not be made durable (or appended to) is an
error: exit 1, `pgr: cannot write the memory bank: ...`, nothing reported as kept. A cache file
(`vectors.bin`, `index.bin`) that could not be written is a warning on standard error
(`pgr: warning: the cache files of the memory bank could not be written ...`) and exit 0: the
note is safe in the log and the next run builds the caches again.

Words such as "today", "yesterday" and "last week" in a retained text or a recall question mean
the days of the machine's time zone (`src/localtime.rs` asks the operating system). The dates
given to `--when`, `--since` and `--until` are UTC, as before.

Entities that are about exactly the same notes (a file and a function in it) share one
observation that lists all of them, instead of one copy each. A bank written by an earlier
version holds such copies; the next write of any kind (`retain`, `memory forget`) merges them,
and a read never changes the bank.

## Start-up cost

The program is started once per question, so it loads only what a command reads: `def`,
`callers`, `about` and the other graph commands read the graph, not the search index or the
model; `find` reads the search index and, when it has vectors, the model; `status` and the
`memory` commands read neither. The parts load on helper threads while the walk runs and
are used when the walk finds nothing to rebuild. A command that does not rebuild starts
no thread pool.

## Known limits

- The release profile has `panic = "abort"`, so the parser guard in `lxr-extract` cannot
  catch a panic there in a release build; a parser panic would end the program.
- The search index is rebuilt whole when any file changed (version 0.1).
- The MCP server reads request lines of at most 1 MiB; a longer line is refused with error
  -32600 and skipped without being kept, and the server goes on.
- Switching an embedding model on later does not change an existing index; run
  `pgr refresh --force`.
- More than 20,000 source files, the home folder (or one above it) and a folder with a
  `.local-index-skip` file are not indexed: `pgr: off for NAME (REASON)`, exit 0.
