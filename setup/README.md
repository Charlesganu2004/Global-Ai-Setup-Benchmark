# Setup: local index first

One set of rules, three helper agents, the skills the rules name and one
session-start hook, installed for Claude Code, Codex and GitHub Copilot. The aim is fewer input and cache-read
tokens: an agent asks a local index where something is, gets the answer in one
turn, and opens a file only when the answer is not already on the line.

Once installed, everything here runs on the machine. Nothing calls a model or a
remote index.

## Install

Hand an agent `INSTALL.txt`, or run it yourself. It is one file that holds both
the instructions and the program: the header is plain text, and below it every
file in this folder is packed as readable comment lines, nothing encoded.

```
python INSTALL.txt              # rules, agents, hook and tools, all three clients
python INSTALL.txt --client claude    # one client: claude, codex or copilot
python INSTALL.txt --no-tools   # no downloads
python INSTALL.txt --dry-run    # show what would change, write nothing
```

From a checkout of this repository the same two steps are:

```
python setup/apply.py           # rules, agents and the hook
python setup/tools.py           # the search tools and the lx command
```

Anything `apply.py` is about to change is first copied to
`~/.local-index/backups/<timestamp>/`, and it is safe to run again.

## What gets installed

| Part | Where | What it does |
| --- | --- | --- |
| Rules | `~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md`, `~/.copilot/copilot-instructions.md` | Two marked blocks. The protected rules tell an agent to ask the local index before it searches or reads a file, and to read only the lines the index points at. The no-compress block tells any tool that shortens instruction files to leave those rules whole. Everything else in each file is left alone. |
| Agents | `~/.claude/agents/`, `~/.codex/agents/`, `~/.copilot/agents/` | Three helper agents a session can hand work to: `cavecrew-investigator` finds code, `cavecrew-builder` writes it, `cavecrew-reviewer` checks it. Rendered from `agents/*.md`. |
| Skills | `~/.claude/skills/`, `~/.codex/skills/`, `~/.copilot/skills/` | One folder for each skill under `skills/`, because the rules name them. A skill that is already installed is never touched, whatever its version. |
| Hook | `~/.local-index/session_index.py` | Runs at session start. Inside a git repository it refreshes the indexes in the background and prints one `local-index:` line. Outside one it prints nothing. |
| Tools | `~/Tools/codesearch-venv`, `~/.graphify/venv` only when no graphify was found, and launchers in the first of `~/bin`, `~/.local/bin` or the Windows `WindowsApps` folder that is on PATH | semble, ast-grep, codanna, graphify when none is present, and the `lx` launcher. |

No agent file names a model version. Work is routed by tier: Tier A is the
strongest model on offer at its highest reasoning setting, Tier B the fast
coding class, Tier C the lightest. Claude Code reaches them through its
`inherit`, `sonnet` and `haiku` aliases, which follow new releases; Codex and
Copilot agents set no model and inherit the session's.

`tools.py` is the part that downloads: semble 0.6.1 and ast-grep 0.45.3 as
pinned wheels, graphify 0.9.72 only when no graphify is found, codanna 0.16.0
as one release file whose SHA-256 must match the pinned value, and the semble
embedding model once. Only the Windows build of codanna is pinned: on other
systems it is skipped, `lx` answers from the graphify graph, `--rust` and
`codanna_embeddings` do nothing, and `--hybrid` searches with semble alone. It accepts no PyPI package uploaded after 2026-09-30 when the environment's pip has the date filter, and warns when it does not.
With `--no-tools` nothing is downloaded and `lx` is still created.

## The engines

| Layer | Tool | Answers | Where its index lives |
| --- | --- | --- | --- |
| Symbol index | codanna, Rust | where a symbol is defined, what calls it | `~/.local-index/codanna/<repo>/`, outside the repository |
| Search by meaning (local RAG) | semble: static embeddings (word vectors that are looked up, with no model run at query time) plus BM25 keyword scoring | a concept, or where something lives | the user cache folder, one entry per repository |
| Code graph | graphify, built from the syntax tree that tree-sitter parses, `--code-only` | blast radius (what a change can break), paths between symbols, and hubs (the symbols most code passes through) | `<repo>/graphify-out/`, hidden through `.git/info/exclude` |
| Structural search | ast-grep, Rust, no index | an exact code shape | none |
| Literal search | ripgrep, Rust, no index | an exact string | none |

The three indexes build side by side. codanna is always run with a settings
file this setup writes and keeps outside the repository, so a repository's own
`.codanna/settings.toml` cannot switch embeddings on or point it at a server.

## lx, the one command agents type

`lx` routes each question to the engine that answers it best and trims the
answer to a line per hit.

```
lx def NAME...            name path:first-last signature
lx callers NAME...        "N callers of NAME:", then name path:first-last per caller
lx about NAME...          definition, doc line and callers in one answer
lx find "words"           path:first-last of the lines that match, then name:line
                          for each definition they sit in
lx find "words" --cards   each hit as its definition: name path:first-last
                          signature, then the doc line
lx find "words" --rust    search the Rust index's embeddings
lx find "words" --hybrid  every search merged: semble, the Rust index's name and doc
                          search, and its embeddings when the index has them
lx find "words" --docs    search prose; --all adds config; -k N sets how many
lx ask KIND:WHAT...       several questions in one call: def, callers, about, find
lx refresh                rebuild what changed, print the status line
lx graph                  build or refresh the graphify graph alone
lx status                 print the status line
lx prune [--apply]        list index data left behind by folders that are gone
```

`--rust` needs an index built with `codanna_embeddings` on. `--hybrid` and the
`find` inside `lx ask` work on any index and add the embeddings when they are
there. When the Rust index is missing, or the code is in a language it does not parse, `lx`
answers from the graphify graph instead, so the commands stay the same.

Graph questions go straight to graphify, from the repository root:

```
graphify affected "check_token"
graphify path "A" "B"
graphify query "how does auth reach the store" --budget 1500
```

Exact shapes and strings need no index:

```
ast-grep run -p 'check_token($$$)' -l python .
rg -n -F 'check_token(' . -g '!graphify-out'
```

## Options

`~/.local-index/config.json`. The numbers are from one repository of 86 Python
files; the full table is in `benchmark/RESULTS.md`.

| Option | Default | Effect |
| --- | --- | --- |
| `graph_on_demand` | `false` | `true` skips graphify at refresh; `lx graph` builds it when needed. Index 26.2 MB down to 17.6 MB, a refresh after one edit 6.7 s down to 2.8 s. `lx find` then prints ranges without definition names. |
| `codanna_embeddings` | `false` | `true` gives the Rust index its own embeddings, which `lx find --hybrid`, `--rust` and `lx ask` then search. In the benchmark the shipped build answered in two turns with it and three without on the mid model tier (three runs each); the cheapest tier took two turns either way. On the default stack the first build takes about 130 s instead of about 12 s and a refresh after one edit about 29 s instead of about 7 s (the Rust index alone takes about 100 s and 17 s), and codanna fetches an 87 MB model the first time. It embeds doc comments, so undocumented code is found through semble, which `--hybrid` still asks. If the build fails, the index is rebuilt without embeddings. |
| `embed_max_files` | `600` | With embeddings on, a repository with more tracked files than this is still indexed without them, so a first build never ties up every core unasked. `--rust` then falls back to semble, and `--hybrid` merges semble with the name and doc search. |
| `find_engine` | `"semble"` | `"codanna"` makes plain `lx find` search the Rust index and stops building the semble index. It only takes effect when `codanna_embeddings` is on and the repository is within `embed_max_files`; otherwise `lx find` keeps using semble. |
| `max_files` | `20000` | A repository with more tracked files is left for a deliberate, hand-run index. |
| `skip_roots` | `[]` | Paths never indexed. An empty `.local-index-skip` file in a repository root does the same for that repository. |

Changing an option never empties an index a lookup is about to read: only
`lx refresh` rebuilds.

## Checks

```
python setup/tests/check_hook.py setup/local-index/session_index.py EMPTY_DIR
python setup/tests/check_installer.py setup EMPTY_DIR ORIGINALS_DIR
```

These two run from a checkout of the repository; the one-file installer does
not carry them. The first drives the hook and `lx` against small throwaway repositories. The
second installs over copies of real client files in throwaway home directories;
those originals are personal, so you supply your own. `build_install.py`
repacks `INSTALL.txt` and re-renders `rendered/` after any edit here.

## Limits worth knowing

- graphify before 0.9.70 is affected by GHSA-pcc4-rvhr-2pr8: a hostile Fortran
  file can make the C preprocessor pull other host files into `graph.json`.
  The refresh command runs an older graphify with every folder holding a `cpp`
  taken off its PATH, which closes it. Running `graphify extract` by hand does
  not have that guard, which is one reason the rules forbid it.
- codanna's release file is not code-signed. `tools.py` accepts it only when
  its SHA-256 matches the pinned value.
- The rules are strict on purpose: they tell every agent that only the user
  may remove a skill or lift the no-compress block. Read `rules/` and the
  files under `skills/` before installing, so nothing they say surprises you.
- `apply.py` also removes an older wrapper block, marked `MASTER-REPO-USE`,
  that the author's previous setup wrote into the same files. On a machine
  that never had it, that step finds nothing and changes nothing.
- Copilot loads extra instruction folders named in the user variable
  `COPILOT_CUSTOM_INSTRUCTIONS_DIRS`. `apply.py` reports any whose file still
  carries the older wrapper block this setup replaces, and prints the command
  that clears the variable; it never changes the variable itself.
- Codex asks for new hooks to be trusted before it runs them: open `/hooks`
  once and approve the session-start entries.
- Claude Code reads a new `~/.claude/agents/` folder only after a restart.
- token-goat, rtk and the caveman-autocompress hook are not installed by this
  setup. The rules name them as "where installed", so the same files work on a
  machine that has them.
