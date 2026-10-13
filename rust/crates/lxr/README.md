# pgr (crate `lxr`)

Private developer notes. `pgr` is the command line of PeterGanuRusty: a code graph, hybrid
code search and memory for the repository a coding agent works in. One program, started
once per question. The contract it follows is the CLI contract `pgr-cli.txt` (version 0.1).

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

    pgr [--root DIR] [--engines graph,search,memory] COMMAND ...

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
    ask KIND:WHAT...             def callers about affected explain find recall path (path is A>B)
    pack TASK [--budget N]          == find, == graph, == memory, a third of the budget each
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
| `PGR_BANK` | A plain name: use `LXR_HOME\banks\NAME` as the memory bank of every repository. |
| `USERPROFILE`, `HOME` | The home folder pgr refuses to index; also where the model is looked for. |
| `PGR_EMBED_MODEL_DIR` | Build time only, with the feature `embedded-model`: the folder of the model to compile in. |
| `HF_HUB_CACHE`, `HF_HOME` | Hugging Face cache, searched for the model last. |

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
