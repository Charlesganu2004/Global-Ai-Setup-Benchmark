# Round six: running it

Team work, brief packs and a team hub. Round five measured one agent; round six measures the Rust engine
(`pgr`) between an orchestrator and its subagents, with and without a **pack** (the harness runs
`pgr pack` on each question and puts the output in the prompt) and a **hub** (the subagents file findings with
`pgr handoff`, the reviewer starts from `pgr loadout`), plus set T, a small demonstration set written by the
builders to show one feature. 81 cells, 201 agents, three repeats each. Paths below are placeholders; use
yours, without spaces. Do steps 1 to 9 in one PowerShell window and start the client from it: the prompts tell
the agents to run a bare `pgr`, and they find the one that this window finds.

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

## What each args field holds

| field | holds | needed by |
|---|---|---|
| `repo` | the fixture | every arm except set T |
| `repoTeam` | the team copy from step 4 | T0, TL, TI |
| `runTag` | 1 to 24 characters of `a-z 0-9 . _ -`; names the hub banks `r6-TAG-ARM-SETUP-TIER-REP` | TH, TPH |
| `teamBank` | the bank of step 4 (default `team-notes`) | T0, TL, TI |
| `packs` | `packs.json`'s `packs`, by variant `gs`, `g`, `s`, set `A`, `B`, question | TP, TPG, TPS, TPH |
| `loadouts` | `{q1, q2, q3: {task, output, tokens}}` from `team_notes.json` | TI |
| `sums` | the SHA-256 of every pack and loadout text by place (`packs/gs/A/q1`, `loadouts/q1`), checked again by the workflow | TP, TPG, TPS, TPH, TI |
| `briefs` | optional `{q1, q2, q3}`: the orchestrator's shared briefs for set A | orchestrated cells |
| `matrix` | the blocks `{arms, setup, tier, reps}` of `round6-a.json` or `round6-b.json` | all |

`round6_args.py` fills these in and checks them against the question texts in the workflow. The other args of
earlier rounds (`cells`, `rep`, `repoPlain`) still work and are not used here.

## The two halves

`round6-a.json` is the single and parallel cells (57 cells, 81 agents); `round6-b.json` is the orchestrated
cells (24 cells, 120 agents). They share no cell and no bank, so either order works, in one session or two. Run
them on the same day: R6 is RGS run again so that every comparison in the round is same-day, and the start-up
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

## Reading the result

Set T is a demonstration set, not a held-out test. T0 is the floor: it cannot know the notes, but it can guess. The
true or false question is right half the time, and the task name `cli-freeze` hints at the answer; the role is one
of three. Note fields are compared without case and decoration (`BLD-4172.` is the ticket). The stale note is stale
because the code of the function changed since the note was filed, not because the note is false: the edit takes out
a `.lower()` that `urlparse` makes unnecessary, and the loadout marks the note "stale: code changed". Round six
planners are not averaged into the planner that older cells are charged. The pack and loadout tokens are estimates
(four bytes to a token) and the loadouts carry the age they had when step 4 ran.
