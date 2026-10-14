# Round six: running it

Team work, brief packs and a team hub. Round five measured one agent; round six measures the Rust engine
(`pgr`) between an orchestrator and its subagents, with and without a **pack** (the harness runs
`pgr pack` on each question and puts the output in the prompt) and a **hub** (the subagents file findings with
`pgr handoff`, the reviewer starts from `pgr loadout`), plus set T, a small demonstration set written by the
builders to show one feature. 81 cells, 201 agents, three repeats each. Paths below are placeholders; use
yours, without spaces. Do steps 1 to 9 in one PowerShell window and start the client from it: the prompts tell
the agents to run a bare `pgr`, and they find the one that this window finds.

The **second batch** adds three more kinds of combination, 54 cells, 150 agents, three repeats each (steps 10 to
14): the tools inside a WSL 2 virtual machine (**R6V**, **THV**, **TPHV**: R6's commands, TH and TPH, with the
copy of the repository, the Linux build of pgr, its index and the hub banks inside the VM, and the agent still in
the harness); the search without the neural network (**RN**, **TPN**: pgr's only neural part is its embedding
table, and `PGR_NO_MODEL=1` makes it work on words alone); and the pack as the only help (**TPX**: the pack of TP
in the prompt, no tool at all, a field may be answered unknown). The cells of R6 and TP in the setup that the
site calls "Three models, each in its role" (the top model plans, the light model does the lookups, the middle
model reviews) are in the second batch too. `python -I benchmark/harness/round6_args.py --count` prints the
cells and agents of the four matrix files by class of model.

## Order of the steps

1. **Build pgr 0.2 and put it where a bare `pgr` finds it.** `rust\cargo.ps1 build --release`. An older pgr may
   be on PATH (0.1.0 rejects every hub command, with "unknown option --bank"). Copy `rust\target\release\pgr.exe`
   to an empty folder, put that folder first on PATH for this window (`$env:PATH = "C:\path\to\pgr02;" + $env:PATH`),
   and check that `pgr --version` says `pgr 0.2.0`. An index is read only by the version that built it, so refresh
   the fixture with that pgr: `pgr --root C:\path\to\lxbench\repo refresh`. In the fixture, `git status --short`
   must print nothing: remove an untracked folder such as `runs-hub`, which a test of pgr once left there.
2. **Check the harness.** `python -I benchmark/harness/test_round6.py` and
   `node benchmark/harness/check_round6_prompts.js` (the dry run: no model is called). Both must pass.
3. **Compute the packs.** `python -I benchmark/harness/prepare_packs.py --pgr pgr --fixture C:\path\to\lxbench\repo`
   writes `benchmark/data/packs.json` (18 packs). Add `--home DIR` to read another index home.
4. **Make set T.** `python -I benchmark/harness/prepare_team.py --pgr pgr --fixture C:\path\to\lxbench\repo --team C:\path\to\lxbench\repo-team`
   copies the fixture, builds the copy's index, files about 30 notes in the bank `team-notes` of the index home,
   makes one note stale and writes `benchmark/data/team_notes.json`. The fixture is never touched. A second run
   needs `--replace` for the copy and the folder `banks\team-notes` of the index home deleted by hand: `--replace`
   does not remove the bank. The bank and the copy's index exist only in the index home they were made in.
5. **Check the team bank.** `python -I benchmark/harness/prepare_team.py --verify` says whether the bank that every TL
   and TI cell reads is still the bank that was prepared. `round6_args.py` does the same before it writes the
   args of a half with set T arms.
6. **Write the args of half A.** `python -I benchmark/harness/round6_args.py --half a --run-tag r6a --repo C:\path\to\lxbench\repo --repo-team C:\path\to\lxbench\repo-team`
   writes `round6-a-args.json` into the temporary folder (the path is printed; it is never written inside the
   repository, because the args hold your folders). It checks the machine first and refuses when `pgr` on PATH is
   older than 0.2.0 or not the version that made `packs.json` and `team_notes.json`, when it reports "no index yet"
   for a fixture, when a bank `r6-TAG-...` of the run tag exists already, or when the team bank is not the prepared one.
7. **Run half A.** Ask Claude Code to run `benchmark/harness/token-benchmark.workflow.js` as a workflow with the
   contents of that file as its args, exactly as written. The model has to type the file out (about 57 KB), so the
   workflow computes the SHA-256 of every pack and loadout again and stops before any agent starts when one differs
   from `sums`, or when a pack, a loadout or the run tag is missing. If it stops on a checksum, ask again.
8. **Write the args and run half B**, with its own tag: `--half b --run-tag r6b --repo C:\path\to\lxbench\repo`.
9. **Check the team bank again** with step 5, then **parse** (below). `build_site.py`, `docs/` and `report/` are
   rebuilt in a later step by someone else.

## The second batch: steps 10 to 14

10. **A pgr that knows `PGR_NO_MODEL`, and the Linux build.** The pgr of step 1 has to be a build with the variable
    (`prepare_words.py` refuses one that ignores it). Build pgr for Linux at the same version, a Linux program
    file (the ELF marker is checked), on any machine or inside the VM itself, and keep it where Windows can read
    it: `C:\path\to\pgr-linux`. The VM needs the same embedding model where pgr looks for it, or the index in the VM
    comes out as "words only" and step 12 stops.
11. **Make the words-only copy** (after step 3, which writes `packs.json` whole and drops variant n).
    `python -I benchmark/harness/prepare_words.py --pgr pgr --fixture C:\path\to\lxbench\repo --words C:\path\to\lxbench\repo-words`
    copies the fixture, builds the copy's index with `PGR_NO_MODEL=1`, checks that its status line says "words
    only" and that its graph is the fixture's, computes the six packs of variant `n` (engines graph and search) and
    adds them to `packs.json` without touching the other 18. It prints how many of the six are the same text as the
    pack with the model; if all six are, TPN sends TP's prompts and the result will say so. A second run needs `--replace`.
12. **Prepare the virtual machine.** `python -I benchmark/harness/prepare_wsl.py --pgr pgr --fixture C:\path\to\lxbench\repo --linux-pgr C:\path\to\pgr-linux`
    (`--distro NAME`, default Ubuntu, and `--vm-dir`, default `lxbench` in the home folder of the VM user, are the
    other options). Every step is one call of `wsl.exe -d DISTRO --exec ...`: it copies the fixture into the Linux file
    system (`cp -r` from its `/mnt` path) as `VM_DIR/repo`, installs the Linux pgr as `VM_DIR/bin/pgr`, builds the index
    there, checks that the status line counts are the Windows index of the fixture, computes the packs for sets A
    and B with the Linux build and asserts that they are byte for byte the packs of `packs.json` (the same pack text
    is then used for both; if one differs it stops, prints the difference and writes nothing). It writes
    `benchmark/data/wsl.json`, with the home folder written as `/home/USER`. It is safe to run twice: it replaces its own
    `VM_DIR/repo` and touches nothing else in the VM. A copy is its own only when the marker `VM_DIR/repo.round6-wsl`
    stands beside it (written before the copy starts); a `VM_DIR/repo` without the marker is refused and left alone.
13. **Write the args of halves c and d**, each with a run tag of its own:
    `python -I benchmark/harness/round6_args.py --half c --run-tag r6c --repo C:\path\to\lxbench\repo --repo-words C:\path\to\lxbench\repo-words --wsl-distro Ubuntu`
    and `python -I benchmark/harness/round6_args.py --half d --run-tag r6d --repo C:\path\to\lxbench\repo --wsl-distro Ubuntu`
    (`--wsl-dir` if step 12 used `--vm-dir`). Besides the checks of step 6 they read the words-only copy with
    `PGR_NO_MODEL=1` and want "words only", and ask the VM, read-only, through `wsl.exe`: the Linux pgr is there,
    executable, the build of `wsl.json` (same version, same SHA-256), it reads the index of the copy as step 12
    left it, and the VM holds no bank of the run tag.
14. **Run halves c and d** like step 7, in the order a, b, c, d and on the same day (R6 and TP appear in every half
    as the comparison). Then parse every folder (below).

## What each args field holds

| field | holds | needed by |
|---|---|---|
| `repo` | the fixture | every arm except set T |
| `repoTeam` | the team copy from step 4 | T0, TL, TI |
| `repoWords` | the words-only copy from step 11 | RN, TPN |
| `wsl` | `{distro, repo, pgr}` of the VM from step 12: the distribution and two absolute Linux paths | R6V, THV, TPHV |
| `runTag` | 1 to 24 characters of `a-z 0-9 . _ -`; names the hub banks `r6-TAG-ARM-SETUP-TIER-REP` | TH, TPH, THV, TPHV |
| `teamBank` | the bank of step 4 (default `team-notes`) | T0, TL, TI |
| `packs` | `packs.json`'s `packs`, by variant `gs`, `g`, `s` and `n` (step 11), set `A`, `B`, question | TP, TPG, TPS, TPH, TPHV, TPN, TPX |
| `loadouts` | `{q1, q2, q3: {task, output, tokens}}` from `team_notes.json` | TI |
| `sums` | the SHA-256 of every pack and loadout text by place (`packs/gs/A/q1`, `loadouts/q1`), checked again by the workflow | TP, TPG, TPS, TPH, TPHV, TPN, TPX, TI |
| `briefs` | optional `{q1, q2, q3}`: the orchestrator's shared briefs for set A | orchestrated cells |
| `matrix` | the blocks `{arms, setup, tier, reps}` of `round6-a.json` to `round6-d.json` | all |

`round6_args.py` fills these in and checks them against the question texts in the workflow. The other args of
earlier rounds (`cells`, `rep`, `repoPlain`) still work and are not used here.

## The four halves

`round6-a.json` is the single and parallel cells (57 cells, 81 agents); `round6-b.json` is the orchestrated
cells (24 cells, 120 agents). The second batch has two more: `round6-c.json` is its single-agent cells (R6V, RN and
TPN on tier C, TPX on tiers C and B, each on sets A and B; 30 cells, 30 agents: 24 light, 6 middle) and
`round6-d.json` its orchestrated cells (R6 and TP in the setup "Three models, each in its role", THV and TPHV in
the standard orchestrated setup, tier C workers, sets A and B; 24 cells, 120 agents: 72 light, 12 middle, 36 top,
each cell counting its planner). Together the second batch is 54 cells, 150 agents. `round6_args.py --count`
prints these numbers from the files. The four halves share no cell and no bank, so the order does not change the
cells, but run them in the order a, b, c, d and all on the same day. Without a pgr for Windows that knows
`PGR_NO_MODEL` and a Linux build, only a and b can be run. R6 is RGS run again so that every comparison in the round is same-day, and the start-up
context of the client drifts between days. Use a **new run tag for every run and every rerun** of a half:
`round6_args.py` refuses a tag whose banks exist, because a resumed workflow re-runs most agents and a hub cell
that finds the notes of its first attempt is not a clean cell. The parser lists such a cell as `bank:rerun`
(an earlier transcript of the same agent had filed), and two cells that share a bank as `bank:shared`.

## Parsing

Parsing rewrites the whole result, so give `bench_parse.py` every folder of rounds one to five first, in the order the
stored rows name their runs (`check_reparse.py --print-order --transcripts TRANSCRIPT_ROOT` prints them), then the new
folders. Write to a new file and compare before replacing:

```
python -I benchmark/harness/check_reparse.py --transcripts TRANSCRIPT_ROOT
python -I benchmark/harness/bench_parse.py results-new.json OLD_FOLDERS_IN_ORDER... NEW_FOLDER_A NEW_FOLDER_B
python -I benchmark/harness/check_reparse.py --old benchmark/data/results.json --new results-new.json
```

The first command re-parses the stored rounds and must report an identical file; the third must report that the
rows and cells of the earlier rounds are identical. Only then copy `results-new.json` over
`benchmark/data/results.json`. Round six rows carry `pgr_lookups`, `hub_calls` (handoff, loadout, recall),
`from_brief` (it answered with no lookup; filing a finding does not count as one) and `banks`; round six cells
carry `from_brief_share`, `pgr_lookups`, `hub_calls`, `banks` and, on set T, `note_fields` and `lookup_fields`.
A cell is listed in `off_policy` when an agent used the hub outside its prompt (`hub:not-allowed`, `bank:foreign`),
when a pgr call came back as a usage error (`pgr:usage-error`: the wrong pgr was on PATH, and the call is counted
nowhere), or for the bank flags above. A cell with any of these is not a clean measurement of its arm.

The second batch adds what the parser recognises behind the `wsl` prefix: a call of the form
`wsl -d DISTRO --cd REPO --exec PROGRAM ARGS` is a pgr call (and a hub call) when PROGRAM is a pgr, and the reading
of code with `sed` is not a lookup. For the three VM arms the shell word `wsl` (or `wsl.exe`) is the only one allowed;
a call that leaves the VM is off policy: a Windows `pgr` at the start of a call is listed as `shell:pgr`, the Read,
Grep and Glob tools as `Read`, `Grep`, `Glob`, a `wsl` call that does not start a program directly as `wsl:no-exec`, a
program inside the VM that is neither pgr nor sed as `wsl:other-command`, and every other way out as `wsl:leaves-vm`:
a Windows program or a path on the Windows drives reached through `wsl`; a `wsl` call whose `--cd` is a Windows path,
is under `/mnt/` or climbs there, or that has no `--cd` (wsl then starts in the Windows folder of the caller); and any
statement or pipe stage of the call that is not a `wsl` call (a Windows `pgr` or `Get-Content` after a `;` counts, a
`Set-Location` prefix, a quoted text piped in and a text filter such as `Select-Object` do not). A bare Windows `pgr` is
not counted as a lookup of a VM arm. An answer's path inside the VM is cut like a Windows one, so `hooks.py` is the
file whichever side it came from, also under another `--vm-dir`. The arms without the neural network start each call with
`$env:PGR_NO_MODEL='1';`, which the parser steps over to find the program. TPX may call no tool at all: any tool it
calls is listed under its own name, and `from_brief` is its normal state.

## Reading the result

The second batch asks three separate questions, and each is read against R6 or TP of the same day. The VM arms
(R6V, THV, TPHV) run the same engine on Linux behind `wsl.exe`: the packs are the same text byte for byte (that
is checked in step 12), so a difference in tokens comes from the calls the agent makes, not from different
context, and a difference in wall time includes the start of `wsl.exe` for every call. The arms without the neural
network (RN, TPN) show what the words alone do; if the six words-only packs are the same text as the packs with
the model, TPN is TP again and only the agent's own calls differ. TPX is the floor of the pack: a graded wrong
field next to "unknown" means the pack did not show it, and its cost per cell is the cost of reading the prompt
once. "Three models, each in its role" is the setup `orch-mid`: the top model plans, the light model does the
lookups, the middle model reviews with the method's tools; R6 and TP run in it for the first time, so compare them
with the R6 and TP cells of the standard orchestrated setup in half b, not with cells of an earlier round.

Set T is a demonstration set, not a held-out test. T0 is the floor: it cannot know the notes, but it can guess. The
true or false question is right half the time, and the task name `cli-freeze` hints at the answer; the role is one
of three. Note fields are compared without case and decoration (`BLD-4172.` is the ticket). The stale note is stale
because the code of the function changed since the note was filed, not because the note is false: the edit takes out
a `.lower()` that `urlparse` makes unnecessary, and the loadout marks the note "stale: code changed". Round six
planners are not averaged into the planner that older cells are charged. The pack and loadout tokens are estimates
(four bytes to a token) and the loadouts carry the age they had when step 4 ran.
