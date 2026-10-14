"""Tests of the round six harness.

    python -I benchmark/harness/test_round6.py

Covers the workflow (through check_round6_prompts.js, which needs node), the arms, the matrices, the
parser (on stored rows and on small synthetic transcripts), prepare_packs.py and prepare_team.py (against
fake_pgr.py, a stand-in for the pgr 0.2 contract) and round6_args.py, with its checks of the pgr that the
agents will find, the run tag's banks and the team bank. The second batch of arms (the tools in a virtual machine,
no neural network, the pack only) has its own cases in MoreArmsParser and MoreArmsPreparation: the parser behind a
wsl prefix, prepare_words.py, prepare_wsl.py (against fake_wsl.py, a stand-in for wsl.exe) and the halves c and d
of round6_args.py. Class Review holds the cases an adversarial review of
the first harness found; each failed before its fix. Two checks need the owner's machine and are skipped
elsewhere:

  ROUND6_TRANSCRIPTS=DIR   the folder with the workflow transcripts: every stored round is parsed again and
                           the whole file must come out byte for byte
  ROUND6_FIXTURE=DIR       the fixture: the names the notes use must each be defined once, and the edit's
                           line must be the line the notes were written for
"""
from __future__ import annotations

import contextlib
import hashlib
import io
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import bench_parse  # noqa: E402
import check_reparse  # noqa: E402
import prepare_packs  # noqa: E402
import prepare_team  # noqa: E402
import round6_args  # noqa: E402
import round6_common as common  # noqa: E402

NODE = shutil.which("node")
FAKE = HERE / "fake_pgr.py"
FAKE_WSL = HERE / "fake_wsl.py"
ROUND_SIX_ARMS = ["R6", "TP", "TPG", "TPS", "TH", "TPH", "T0", "TL", "TI"]
MORE_ARMS = ["R6V", "THV", "TPHV", "RN", "TPN", "TPX"]
NEW_FILES = ["round6_common.py", "prepare_packs.py", "prepare_team.py", "round6_args.py", "check_reparse.py", "fake_pgr.py",
             "check_round6_prompts.js", "round6-a.json", "round6-b.json", "test_round6.py", "ROUND6.md",
             "token-benchmark.workflow.js", "bench_parse.py", "arms.json", "prepare_words.py", "prepare_wsl.py", "fake_wsl.py",
             "round6-c.json", "round6-d.json"]


def stand_in() -> str:
    return f'"{sys.executable}" -I "{FAKE}"'


def run_node(*args) -> subprocess.CompletedProcess:
    return subprocess.run([NODE, str(HERE / "check_round6_prompts.js"), *map(str, args)], capture_output=True, text=True, encoding="utf-8")


def make_fixture(root: pathlib.Path) -> None:
    """A tiny repository with every function the notes name, and validate_url with the edit's line at 121."""
    (root / ".git").mkdir(parents=True)
    names = sorted({name for _, _, text in prepare_team.NOTES for name in prepare_team.code_names(text)} - {prepare_team.EDIT["function"]})
    (root / "stubs.py").write_text("".join(f"def {name}():\n    return 1\n\n\n" for name in names), encoding="utf-8")
    lines = [f"# filler {n}" for n in range(1, 103)] + ["def validate_url(url: str) -> str:"]
    lines += [f"    value_{n} = {n}" for n in range(104, 120)] + ["    if True:", prepare_team.EDIT["old"], "            raise ValueError(url)", "    return url", ""]
    (root / "security.py").write_text("\n".join(lines), encoding="utf-8", newline="\n")


def tree_hash(root: pathlib.Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(root.rglob("*")):
        if path.is_file():
            digest.update(path.relative_to(root).as_posix().encode() + path.read_bytes())
    return digest.hexdigest()


def write_agent(folder: pathlib.Path, number: int, label: str, steps: list, answer: dict | None, input_tokens: int = 10,
                stamp: str = "2026-10-13T10:00:00.000Z") -> None:
    """One agent as the client stores it: a meta file with the label and a log of requests and tool results.
    A step is (tool, input) or (tool, input, what the tool printed); the default is "ok"."""
    usage = {"input_tokens": input_tokens, "output_tokens": 5, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}
    rows = []
    for index, (name, given, *printed) in enumerate(steps):
        rows.append({"type": "assistant", "timestamp": stamp, "message": {"id": f"m{number}-{index}", "model": "claude-haiku-4", "usage": usage,
                     "content": [{"type": "tool_use", "id": f"t{number}-{index}", "name": name, "input": given}]}})
        rows.append({"type": "user", "timestamp": stamp, "message": {"content": [{"type": "tool_result", "tool_use_id": f"t{number}-{index}", "content": printed[0] if printed else "ok"}]}})
    if answer is not None:
        rows.append({"type": "assistant", "timestamp": stamp, "message": {"id": f"m{number}-end", "model": "claude-haiku-4", "usage": usage,
                     "content": [{"type": "tool_use", "id": f"t{number}-end", "name": "StructuredOutput", "input": answer}]}})
    (folder / f"agent-{number:04d}.meta.json").write_text(json.dumps({"description": label}), encoding="utf-8")
    (folder / f"agent-{number:04d}.jsonl").write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")


def shell(command: str, printed: str = "ok"):
    return ("PowerShell", {"command": command}, printed)


TRUTH_A_ANSWER = {
    "q1": {"callers": sorted(bench_parse.TRUTH["q1"])},
    "q2": {"file": "cli.py", "line": 1064, "parameters": ["cmd"], "called_by": ["_run_cli"]},
    "q3": {"function": "_register_merge_driver", "file": "hooks.py", "line": 736, "removed_by": "_unregister_merge_driver"},
}
TRUTH_T_ANSWER = {
    "q1": {"ticket": "BLD-4172", "may_rename": False, "file": "cli.py", "line": 1064},
    "q2": {"status": "changed", "filed_by": "reviewer", "called_by": sorted(bench_parse.TRUTH_B["q2"]["called_by"])},
    "q3": {"function": "_register_merge_driver", "roles": ["builder", "scout"], "file": "hooks.py", "line": 736},
}


def build_round_six(folder: pathlib.Path) -> None:
    """Synthetic transcripts of round six agents (and two planners) in one folder."""
    notes = "Set-Location C:\\path\\to\\lxbench\\repo; "
    bank = "r6-t-th-orch-c-r1"
    hub = lambda role: f'{notes}pgr --bank {bank} --task lookups --from scout handoff "found it in a.py:1; b calls it"' if role == "file" else f"{notes}pgr --bank {bank} --task lookups loadout"
    briefs = {"q1": "b1", "q2": "b2", "q3": "b3"}
    # a hub cell: the planner, three workers (lookup and file / file only / lookup, read and file) and a reviewer that loads
    write_agent(folder, 1, "TH|orch|C|plan|r1", [], briefs)
    write_agent(folder, 2, "TH|orch|C|w-q1|r1", [shell(notes + "pgr ask about:foo"), shell(hub("file"))], TRUTH_A_ANSWER["q1"])
    write_agent(folder, 3, "TH|orch|C|w-q2|r1", [shell(hub("file"))], TRUTH_A_ANSWER["q2"])
    write_agent(folder, 4, "TH|orch|C|w-q3|r1", [shell(notes + "pgr ask about:bar"), ("Read", {"file_path": "x"}), shell(hub("file"))], TRUTH_A_ANSWER["q3"])
    write_agent(folder, 5, "TH|orch|C|verify|r1", [shell(hub("load"))], TRUTH_A_ANSWER)
    # the same bank again in a cell of another arm: both must be flagged
    write_agent(folder, 6, "THh|orch|C|verify|r1", [shell(hub("load"))], TRUTH_A_ANSWER)
    # a pack arm that answered from its brief
    write_agent(folder, 7, "TP|single|C|all|r1", [], TRUTH_A_ANSWER)
    # set T: loads two loadouts, gets the status and the roles wrong
    wrong = {**TRUTH_T_ANSWER, "q2": {**TRUTH_T_ANSWER["q2"], "status": "current"}, "q3": {**TRUTH_T_ANSWER["q3"], "roles": ["scout"]}}
    write_agent(folder, 8, "TL|single|C|all|r1", [shell(f"{notes}pgr --bank team-notes --task cli-freeze loadout"), shell(f"{notes}pgr --bank team-notes --task ssrf-review loadout")], wrong)
    # an old arm: no new field
    write_agent(folder, 9, "RGS|single|C|all|r1", [shell(notes + "pgr ask about:foo")], TRUTH_A_ANSWER)
    # a measured planner of an old arm, and a round six planner that would move the mean a great deal
    write_agent(folder, 10, "RGS|orch|C|plan|r1", [], briefs, input_tokens=7)
    write_agent(folder, 11, "R6|orch|C|plan|r2", [], briefs, input_tokens=1000)
    # a round six cell without a planner of its own: charged the mean of the old planners only
    for number, role in enumerate(["w-q1", "w-q2", "w-q3", "verify"], 12):
        write_agent(folder, number, f"R6|orch|C|{role}|r1", [], TRUTH_A_ANSWER if role == "verify" else TRUTH_A_ANSWER[role[2:]])


class Dry(unittest.TestCase):
    """The workflow, run with a fake agent(): no model is called."""

    @classmethod
    def setUpClass(cls):
        if not NODE:
            raise unittest.SkipTest("node is not installed")
        cls.temp = tempfile.TemporaryDirectory()
        cls.dump, cls.inside = pathlib.Path(cls.temp.name) / "dump.json", pathlib.Path(cls.temp.name) / "inside.json"
        cls.done = run_node("--dump", cls.dump, "--inside", cls.inside)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_dry_run_passes(self):
        self.assertEqual(self.done.returncode, 0, self.done.stdout + self.done.stderr)
        self.assertRegex(self.done.stdout, r"ok: \d+ checks passed")

    def test_dry_run_catches_a_broken_workflow(self):
        source = (HERE / "token-benchmark.workflow.js").read_text(encoding="utf-8")
        broken = {
            "R6 differs from RGS": ("ARMS.R6 = ARMS.RGS", "ARMS.R6 = [PGR_ASK, 'pgr extra']", "differs from the prompt of RGS"),
            "bank without the repeat": ("c.tier, c.rep || REP].join", "c.tier].join", "belongs to another cell"),
            "a pack of the wrong variant": ("TPG: 'g', TPS", "TPG: 'gs', TPS", "pack"),
            "Date.now": ("const REP = (args && args.rep) || 'r1'", "const REP = (args && args.rep) || 'r1'\nconst STAMP = Date.now()", "Date.now"),
            "Math.random": ("const REP = (args && args.rep) || 'r1'", "const REP = (args && args.rep) || 'r1'\nconst PICK = Math.random()", "Math.random"),
            "an argless new Date": ("const REP = (args && args.rep) || 'r1'", "const REP = (args && args.rep) || 'r1'\nconst WHEN = new Date()", "argless new Date"),
            "require": ("const REP = (args && args.rep) || 'r1'", "const REP = (args && args.rep) || 'r1'\nconst fs = require('fs')", "require"),
            "a meta block that is not a literal": ("name: 'index-token-benchmark',", "name: 'index-token-benchmark'.toUpperCase(),", "pure literal"),
            "a phase the meta block does not list": ("phase: 'Single',", "phase: 'Solo',", "phase names"),
            "the hub sentence gone": (" The hub commands write to a note bank outside the repository; that is allowed.", "", "hub sentence"),
            "no check before the agents start": ("for (const c of CELLS) check(c)", "", "a missing pack"),
            "a pack that is not held to its checksum": ("proven(`packs/${PACK_OF[arm]}/${set}/${k}`, pack.output, pack.tokens, `the pack for variant ${PACK_OF[arm]}, set ${set}, ${k}`)", "true",
                                                         "a caller line dropped from a pack"),
            "a loadout that is not held to its checksum": ("proven(`loadouts/${k}`, load.output, load.tokens, `the loadout for ${k}`)", "true", "a ticket changed in a loadout"),
            "a wrong constant in the SHA-256": ("0x428a2f98,", "0x428a2f99,", "does not match its checksum"),
            "the finding in double quotes": ("--from scout handoff '<finding>'", "--from scout handoff \"<finding>\"", "double quotes"),
            "the VM reading rule gone": (" Do not use the Read, Grep or Glob tools at all.", "", "reading rule of the VM"),
            "a VM arm on the Windows folder": ("for (const arm of Object.keys(VM)) REPO_OF[arm] = WSL.repo", "for (const arm of Object.keys(VM)) REPO_OF[arm] = REPO", "names a Windows path"),
            "a VM planner on the Windows folder": ("${VM[baseArm(c.arm)] ? WSL.repo : REPO}", "${REPO}", "planner of a virtual machine arm"),
            "a words-only arm without its prefix": ("${WORDS[arm] ? NO_MODEL_START : ''}", "", "words-only prefix"),
            "the pack-only arm with a command": ("const PACK_ONLY = { TPX: true }", "const PACK_ONLY = {}", "holds a command"),
            "the pack-only schema that cannot say unknown": ("schema: PACK_ONLY[baseArm(c.arm)] ? unknownable(set.ALL) : set.ALL", "schema: set.ALL", "let a field be unknown"),
            "a brief that is not rewritten for the VM": ("const brief = k => VM[baseArm(c.arm)] ? inVm(plan[k]) : plan[k].split(REPO).join(root)", "const brief = k => plan[k]", "not rewritten for a VM worker"),
            "only the backslash spelling of the fixture is rewritten": (r"[REPO, REPO.replace(/\\/g, '/'), posix(REPO)]", "[REPO]", "names a Windows path"),
            "a Windows path with forward slashes goes unseen": (r"const WINDOWS_PATH = /(^|[^A-Za-z0-9])[A-Za-z]:[\\/]|\/mnt\/[a-z]\//", r"const WINDOWS_PATH = /[A-Za-z]:\\|\/mnt\/[a-z]\//", "forward-slash Windows"),
            "no guard against a Windows folder in a brief": (" if (VM[baseArm(c.arm)] && KEYS.some(k => WINDOWS_PATH.test(brief(k)))) return { ...c, set: setOf(c), answer: null, failed: 'plan:windows-path' }", "", "does not stop the VM cell"),
            "R6V that sends something else than R6": ("ARMS.R6V = ARMS.RGS", "ARMS.R6V = [PGR_ASK, 'pgr extra']", "is not the prompt of R6"),
        }
        with tempfile.TemporaryDirectory() as temp:
            for name, (old, new, expected) in broken.items():
                self.assertEqual(source.count(old), 1, f"{name}: the text to break is not in the workflow once")
                copy = pathlib.Path(temp) / "workflow.js"
                copy.write_text(source.replace(old, new), encoding="utf-8")
                done = run_node("--workflow", copy)
                self.assertEqual(done.returncode, 1, f"{name} was not caught: {done.stdout[:200]}")
                self.assertIn(expected, done.stdout, name)

    def test_the_question_texts_have_one_source(self):
        inside = json.loads(self.inside.read_text(encoding="utf-8"))
        written = common.workflow_questions()
        self.assertEqual(inside["Q"], written["A"])
        self.assertEqual(inside["Q_B"], written["B"])
        self.assertEqual(inside["Q_T"], written["T"])
        arms = json.loads(HERE.joinpath("arms.json").read_text(encoding="utf-8"))
        self.assertEqual({q["id"]: q["text"] for q in arms["questions"]}, written["A"])
        for key, task in prepare_team.TASKS.items():
            self.assertIn(f"`{task}`", written["T"][key])

    def test_arm_tables_agree_with_the_python_side(self):
        inside = json.loads(self.inside.read_text(encoding="utf-8"))
        self.assertEqual(inside["PACK_OF"], round6_args.PACK_OF)
        self.assertEqual(set(inside["HUB"]), round6_args.HUB_ARMS)
        self.assertEqual(set(inside["SET_T"]), bench_parse.SET_T_ARMS)
        self.assertEqual(set(ROUND_SIX_ARMS + MORE_ARMS), bench_parse.TEAM_ARMS)
        self.assertEqual(set(inside["VM"]), round6_args.VM_ARMS)
        self.assertEqual(set(inside["VM"]), bench_parse.VM_ARMS)
        self.assertEqual(set(inside["WORDS"]), round6_args.WORDS_ARMS)
        self.assertEqual(set(inside["WORDS"]), bench_parse.WORDS_ARMS)
        self.assertEqual(set(inside["PACK_ONLY"]), round6_args.PACK_ONLY_ARMS)
        self.assertEqual(set(inside["PACK_ONLY"]), bench_parse.PACK_ONLY_ARMS)
        self.assertEqual(round6_args.HUB_ARMS, bench_parse.HUB_ARMS)
        # the class of model that each setup's reviewer is, as the count of agents names it
        tier_of = {tuple(model.items()): tier for tier, model in inside["TIER"].items()}
        self.assertEqual({setup: common.TIER_CLASS[tier_of[tuple(entry["who"].items())]] for setup, entry in inside["REVIEW"].items()}, common.REVIEWER_CLASS)

    def test_prompts_carry_the_commands_arms_json_describes(self):
        arms = json.loads(HERE.joinpath("arms.json").read_text(encoding="utf-8"))
        prompts = json.loads(self.dump.read_text(encoding="utf-8"))
        by_arm: dict[str, list[str]] = {}
        for label, text in prompts.items():
            by_arm.setdefault(label.split("|")[0], []).append(text)
        for arm in arms["arms"]:
            if arm["group"] != "team":
                continue
            texts = by_arm[arm["id"]]
            for key in arm["uses"]:
                command = arms["commands"][key]
                if key.startswith("pgr_hub"):
                    found = any(command.replace("BANK", re.search(r"--bank (r6-\S+) ", text).group(1)).replace("TASK", "lookups") in text
                                for text in texts if "--bank r6-" in text)
                else:
                    found = any(command in text for text in texts)
                self.assertTrue(found, f"{arm['id']}: the command {key} is not in any of its prompts")

    def test_the_packs_json_in_the_repository_matches_the_workflow(self):
        path = common.DATA / "packs.json"
        if not path.exists():
            self.skipTest("packs.json has not been made yet (prepare_packs.py needs pgr 0.2)")
        self.assertEqual(common.check_packs(json.loads(path.read_text(encoding="utf-8")), common.workflow_questions()), [])


class Matrices(unittest.TestCase):
    def setUp(self):
        self.a = json.loads((HERE / "round6-a.json").read_text(encoding="utf-8"))
        self.b = json.loads((HERE / "round6-b.json").read_text(encoding="utf-8"))
        self.c = json.loads((HERE / "round6-c.json").read_text(encoding="utf-8"))
        self.d = json.loads((HERE / "round6-d.json").read_text(encoding="utf-8"))
        self.arms = json.loads((HERE / "arms.json").read_text(encoding="utf-8"))

    def cells(self, matrix):
        return [(arm, block["setup"], block["tier"], rep) for block in matrix for rep in block["reps"] for arm in block["arms"]]

    def test_shape_and_repeats(self):
        for matrix in (self.a, self.b, self.c, self.d):
            for block in matrix:
                self.assertEqual(sorted(block), ["arms", "reps", "setup", "tier"])
                self.assertEqual(block["reps"], ["r1", "r2", "r3"])
        every = self.cells(self.a) + self.cells(self.b) + self.cells(self.c) + self.cells(self.d)
        self.assertEqual(len(every), len(set(every)))

    def test_the_cells_the_brief_asks_for(self):
        want = set()
        for tier in "CB":
            want |= {(arm, "single", tier) for arm in ("TP", "TPh")}
        want |= {(arm, "single", "C") for arm in ("TPG", "TPGh", "TPS", "TPSh", "R6", "R6h", "T0")}
        want |= {(arm, "single", tier) for arm in ("TL", "TI") for tier in "CB"}
        want |= {(arm, "parallel", "C") for arm in ("R6", "R6h", "TP", "TPh")}
        want |= {(arm, "orch", "C") for arm in ("R6", "R6h", "TP", "TPh", "TH", "THh", "TPH", "TPHh")}
        got = {(arm, setup, tier) for arm, setup, tier, _ in self.cells(self.a) + self.cells(self.b)}
        self.assertEqual(got, want)
        self.assertTrue(all(setup in ("single", "parallel") for _, setup, _, _ in self.cells(self.a)))
        self.assertTrue(all(setup == "orch" for _, setup, _, _ in self.cells(self.b)))
        self.assertEqual(len(self.cells(self.a)) + len(self.cells(self.b)), 3 * len(want))

    def test_the_second_batch_of_cells(self):
        want_c = {(arm, "single", "C") for arm in ("R6V", "R6Vh", "RN", "RNh", "TPN", "TPNh")} | {(arm, "single", tier) for arm in ("TPX", "TPXh") for tier in "CB"}
        want_d = {(arm, "orch-mid", "C") for arm in ("R6", "R6h", "TP", "TPh")} | {(arm, "orch", "C") for arm in ("THV", "THVh", "TPHV", "TPHVh")}
        found = lambda matrix: {(arm, setup, tier) for arm, setup, tier, _ in self.cells(matrix)}
        self.assertEqual(found(self.c), want_c)
        self.assertEqual(found(self.d), want_d)
        self.assertEqual((len(self.cells(self.c)), len(self.cells(self.d))), (30, 24))
        # the first two files keep the cells of the first batch: 57 and 24
        self.assertEqual((len(self.cells(self.a)), len(self.cells(self.b))), (57, 24))

    def test_the_counts_of_cells_and_agents(self):
        counts = {half: common.count_matrix(getattr(self, half)) for half in "abcd"}
        self.assertEqual([(counts[h]["cells"], counts[h]["agents"]) for h in "abcd"], [(57, 81), (24, 120), (30, 30), (24, 120)])
        self.assertEqual(counts["c"]["by_class"], {"light": 24, "middle": 6, "top": 0})
        self.assertEqual(counts["d"]["by_class"], {"light": 72, "middle": 12, "top": 36})
        self.assertEqual(counts["d"]["planners"], 24)
        self.assertEqual(counts["d"]["planners_saved_by_briefs"], 12)
        self.assertEqual(counts["a"]["by_class"], {"light": 69, "middle": 12, "top": 0})
        self.assertEqual(counts["b"]["by_class"], {"light": 72, "middle": 0, "top": 48})
        said = io.StringIO()
        with contextlib.redirect_stdout(said):
            self.assertEqual(round6_args.main(["--count"]), 0)
        self.assertIn("round6-d.json: 24 cells, 120 agents (light 72, middle 12, top 36)", said.getvalue())
        self.assertIn("round6-c.json: 30 cells, 30 agents (light 24, middle 6, top 0)", said.getvalue())

    def test_the_mid_tier_reviewer_setup_is_named_for_its_three_roles(self):
        setup = next(item for item in self.arms["setups"] if item["id"] == "orch-mid")
        self.assertEqual(setup["name"], "Three models, each in its role")
        for word in ("plans", "lookups", "reviews"):
            self.assertIn(word, setup["detail"])

    def test_every_arm_is_described_in_arms_json(self):
        ids = {arm["id"]: arm for arm in self.arms["arms"]}
        for arm, _, _, _ in self.cells(self.a) + self.cells(self.b) + self.cells(self.c) + self.cells(self.d):
            self.assertIn(arm, ids, f"{arm} is measured but not described")
            self.assertEqual(ids[arm]["group"], "team")
            base = ids[arm].get("base")
            if arm not in ROUND_SIX_ARMS + MORE_ARMS:
                self.assertEqual(base, arm[:-1])
                self.assertIn(base, ROUND_SIX_ARMS + MORE_ARMS)

    def test_arms_json_is_consistent(self):
        groups = {group["id"] for group in self.arms["groups"]}
        self.assertIn("team", groups)
        ids = {arm["id"] for arm in self.arms["arms"]}
        self.assertEqual(len(ids), len(self.arms["arms"]))
        for arm in self.arms["arms"]:
            self.assertIn(arm["group"], groups)
            for key in arm["uses"]:
                self.assertIn(key, self.arms["commands"], f"{arm['id']} uses {key}")
            if "base" in arm:
                self.assertIn(arm["base"], ids)
        raw = (HERE / "arms.json").read_bytes().decode("utf-8").replace("\r\n", "\n")
        self.assertEqual(json.dumps(self.arms, indent=1, ensure_ascii=False) + "\n", raw, "arms.json is not written the way the other files are")
        self.assertEqual({entry["id"] for entry in self.arms["demonstration_set"]["questions"]}, {"q1", "q2", "q3"})


class Parser(unittest.TestCase):
    def test_arm_forms(self):
        for arm, base, which in [("THh", "TH", "B"), ("TPHh", "TPH", "B"), ("R6h", "R6", "B"), ("TPh", "TP", "B"), ("TPGh", "TPG", "B"),
                                 ("TPSh", "TPS", "B"), ("TH", "TH", "A"), ("TPH", "TPH", "A"), ("R6", "R6", "A"), ("T0", "T0", "T"),
                                 ("TL", "TL", "T"), ("TI", "TI", "T"), ("RGSh", "RGS", "B"), ("A0r", "A0", "A")]:
            self.assertEqual(bench_parse.base_arm(arm), base, arm)
            self.assertEqual(bench_parse.question_set(arm), which, arm)
        for arm in ROUND_SIX_ARMS + ["R6h", "THh", "TPHh"]:
            self.assertEqual(bench_parse.allowed_words(arm), {"pgr"}, arm)
        self.assertEqual(bench_parse.allowed_words("R1"), {"lx"})
        for arm, base, which in [("R6V", "R6V", "A"), ("R6Vh", "R6V", "B"), ("THVh", "THV", "B"), ("TPHVh", "TPHV", "B"), ("RNh", "RN", "B"),
                                 ("TPNh", "TPN", "B"), ("TPXh", "TPX", "B"), ("TPX", "TPX", "A"), ("THV", "THV", "A")]:
            self.assertEqual((bench_parse.base_arm(arm), bench_parse.question_set(arm)), (base, which), arm)
            self.assertTrue(bench_parse.is_team(arm), arm)
        self.assertEqual([bench_parse.allowed_words(arm) for arm in ("R6V", "THVh", "RN", "TPN", "TPX")],
                         [{"wsl", "wsl.exe"}, {"wsl", "wsl.exe"}, {"pgr"}, {"pgr"}, set()])
        self.assertTrue(bench_parse.is_team("TPHh") and not bench_parse.is_team("RGSh"))

    def test_grading_set_t(self):
        for q, answer in TRUTH_T_ANSWER.items():
            self.assertEqual(bench_parse.grade(q, answer, "T"), 1.0, q)
        grade = lambda q, **changes: bench_parse.grade(q, {**TRUTH_T_ANSWER[q], **changes}, "T")
        self.assertEqual(grade("q1", ticket="unknown"), 0.75)
        self.assertEqual(grade("q1", ticket="bld-4172"), 1.0, "the case of a ticket does not make it another ticket")
        self.assertEqual(grade("q1", may_rename=None), 0.75)
        self.assertEqual(grade("q1", may_rename=True), 0.75)
        self.assertEqual(grade("q1", ticket="BLD-4180", line=1065), 0.5)
        self.assertAlmostEqual(grade("q2", status="current"), 2 / 3)
        self.assertAlmostEqual(grade("q2", status="unknown", filed_by="scout"), 1 / 3)
        self.assertEqual(grade("q3", roles=["scout", "builder"]), 1.0)
        self.assertEqual(grade("q3", roles=["builder"]), 0.75)
        self.assertEqual(grade("q3", roles=["builder", "scout", "reviewer"]), 0.75)
        self.assertEqual(grade("q3", roles=[]), 0.75)
        self.assertIsNone(bench_parse.grade("q1", None, "T"))
        self.assertEqual(bench_parse.grade("q2", {}, "T"), 0.0)

    def test_truth_t_uses_the_truth_of_sets_a_and_b(self):
        t = bench_parse.TRUTH_T
        self.assertEqual((t["q1"]["file"], t["q1"]["line"]), (bench_parse.TRUTH["q2"]["file"], bench_parse.TRUTH["q2"]["line"]))
        self.assertEqual(t["q2"]["called_by"], bench_parse.TRUTH_B["q2"]["called_by"])
        self.assertEqual((t["q3"]["function"], t["q3"]["file"], t["q3"]["line"]),
                         (bench_parse.TRUTH["q3"]["function"], bench_parse.TRUTH["q3"]["file"], bench_parse.TRUTH["q3"]["line"]))

    def test_shell_calls(self):
        call = bench_parse.pgr_call
        self.assertEqual(call("pgr ask about:foo"), ("ask", ""))
        self.assertEqual(call("& 'C:\\bin\\pgr.exe' --root C:\\x --bank r6-a-th loadout"), ("loadout", "r6-a-th"))
        self.assertEqual(call("pgr --bank=team-notes --task NAME loadout"), ("loadout", "team-notes"))
        self.assertEqual(call("pgr --version"), ("", ""))
        self.assertIsNone(call("Get-Content x"))
        self.assertEqual(bench_parse.statements('Set-Location X; pgr handoff "a; b" && pgr ask q'), ["Set-Location X", 'pgr handoff "a; b"', "pgr ask q"])
        row = {"answer": {}, "tool_calls": 1}
        filed = bench_parse.team_counts(['Set-Location X; pgr --bank b --task lookups --from scout handoff "found x; and y"'], row)
        self.assertEqual((filed["pgr_lookups"], filed["hub_calls"]["handoff"], filed["from_brief"]), (0, 1, True))
        looked = bench_parse.team_counts(["Set-Location X; pgr ask about:foo; pgr loadout"], {"answer": {}, "tool_calls": 1})
        self.assertEqual((looked["pgr_lookups"], looked["hub_calls"]["loadout"], looked["from_brief"]), (1, 1, False))
        self.assertFalse(bench_parse.team_counts([], {"answer": {}, "tool_calls": 1})["from_brief"])
        self.assertFalse(bench_parse.team_counts([], {"answer": None, "tool_calls": 0})["from_brief"])
        self.assertTrue(bench_parse.team_counts([], {"answer": {}, "tool_calls": 0})["from_brief"])

    def test_old_cells_come_out_as_stored(self):
        results = common.DATA / "results.json"
        self.assertEqual(check_reparse.check_stored_rows(results), [])

    @unittest.skipUnless(os.environ.get("ROUND6_TRANSCRIPTS"), "ROUND6_TRANSCRIPTS is not set")
    def test_old_transcripts_parse_to_the_stored_file(self):
        self.assertEqual(check_reparse.check_transcripts(common.DATA / "results.json", pathlib.Path(os.environ["ROUND6_TRANSCRIPTS"])), [])

    def test_round_six_rows_and_cells(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = pathlib.Path(temp) / "wf_synthetic"
            folder.mkdir()
            build_round_six(folder)
            agents = bench_parse.read_folders([folder])
            cells = {(c["arm"], c["setup"], c["tier"], c["rep"]): c for c in bench_parse.build_cells(agents)}
        row = lambda label: next(a for a in agents if "|".join([a["arm"], a["setup"], a["tier"], a["role"], a["rep"]]) == label)
        w1, w2, w3, verify = row("TH|orch|C|w-q1|r1"), row("TH|orch|C|w-q2|r1"), row("TH|orch|C|w-q3|r1"), row("TH|orch|C|verify|r1")
        self.assertEqual((w1["pgr_lookups"], w1["hub_calls"], w1["from_brief"]), (1, {"handoff": 1, "loadout": 0, "recall": 0}, False))
        self.assertEqual((w2["pgr_lookups"], w2["hub_calls"]["handoff"], w2["from_brief"]), (0, 1, True))
        self.assertEqual((w3["pgr_lookups"], w3["from_brief"]), (1, False))
        self.assertEqual((verify["pgr_lookups"], verify["hub_calls"]["loadout"], verify["from_brief"]), (0, 1, False))
        self.assertEqual(w1["banks"], ["r6-t-th-orch-c-r1"])
        self.assertTrue(row("TP|single|C|all|r1")["from_brief"])
        for field in ("pgr_lookups", "hub_calls", "from_brief", "banks"):
            self.assertNotIn(field, row("RGS|single|C|all|r1"), "an old arm gained a field")
        self.assertNotIn("from_brief_share", cells[("RGS", "single", "C", "r1")])
        hub = cells[("TH", "orch", "C", "r1")]
        self.assertEqual(hub["from_brief_share"], 0.25)
        self.assertEqual((hub["pgr_lookups"], hub["hub_calls"]), (2, {"handoff": 3, "loadout": 1, "recall": 0}))
        self.assertEqual(hub["quality"], 1.0)
        self.assertTrue(hub["complete"])
        self.assertEqual(cells[("TP", "single", "C", "r1")]["from_brief_share"], 1.0)
        # two cells that share a bank are both off policy
        self.assertIn("bank:shared", hub["off_policy"])
        self.assertIn("bank:shared", cells[("THh", "orch", "C", "r1")]["off_policy"])
        self.assertNotIn("bank:shared", cells[("TP", "single", "C", "r1")]["off_policy"])
        # set T
        team = cells[("TL", "single", "C", "r1")]
        self.assertEqual(team["set"], "T")
        self.assertEqual(team["scores"], {"q1": 1.0, "q2": 2 / 3, "q3": 0.75})
        self.assertEqual((team["note_fields"], team["lookup_fields"]), ({"right": 3, "of": 5}, {"right": 6, "of": 6}))
        self.assertEqual(team["hub_calls"]["loadout"], 2)
        self.assertEqual(team["off_policy"], [])
        self.assertIsNone(team["lookup_missed"])
        self.assertEqual(team["banks"], ["team-notes"])
        # the planner of a round six arm is not part of the mean that old cells are charged
        imputed = cells[("R6", "orch", "C", "r1")]
        self.assertTrue(imputed["plan_imputed"])
        own = sum(a["input"] for a in agents if a["arm"] == "R6" and a["rep"] == "r1")
        self.assertEqual(imputed["input"], own + 7)

    def test_round_six_rows_leave_the_old_cells_alone(self):
        stored = json.loads(check_reparse.stored_text(common.DATA / "results.json"))
        with tempfile.TemporaryDirectory() as temp:
            folder = pathlib.Path(temp) / "wf_synthetic"
            folder.mkdir()
            build_round_six(folder)
            # Only the round six rows are added: a planner of an old arm would move the mean, and should.
            both = stored["agents"] + [a for a in bench_parse.read_folders([folder]) if bench_parse.is_team(a["arm"])]
        table = bench_parse.build_cells(both)
        old = [cell for cell in table if not bench_parse.is_team(cell["arm"])]
        self.assertEqual(json.dumps(old, indent=1), json.dumps(stored["cells"], indent=1))


BRIEFS = {"q1": "b1", "q2": "b2", "q3": "b3"}
USAGE_ERROR = "pgr: unknown option --bank (try: pgr help)"
LATER = "2026-10-13T11:00:00.000Z"
WORKDIR = "Set-Location C:\\path\\to\\lxbench\\repo; "


def hub_call(bank: str, what: str) -> str:
    """A shell call of a hub cell as an agent writes it: file a finding, load the notes, or pipe a finding in."""
    base = f"{WORKDIR}pgr --bank {bank} --task lookups"
    return {"file": f"{base} --from scout handoff 'found it in a.py:1; b calls it'",
            "load": f"{base} loadout",
            "pipe": f"{WORKDIR}'found it in a.py:1' | pgr --bank {bank} --task lookups --from scout handoff -",
            "tight": f"{WORKDIR}'found it in a.py:1'|pgr --bank {bank} --task lookups --from scout handoff -"}[what]


def write_hub_cell(folder: pathlib.Path, arm: str, bank: str, worker: str, reviewer: str, first: int = 1,
                   stamp: str = "2026-10-13T10:00:00.000Z", printed: str = "ok") -> int:
    """The planner, the three workers and the reviewer of one orchestrated hub cell, each worker making the
    `worker` call and the reviewer the `reviewer` call (see hub_call). Returns the next agent number."""
    write_agent(folder, first, f"{arm}|orch|C|plan|r1", [], BRIEFS, stamp=stamp)
    for offset, key in enumerate(common.KEYS, 1):
        write_agent(folder, first + offset, f"{arm}|orch|C|w-{key}|r1", [shell(hub_call(bank, worker), printed)], TRUTH_A_ANSWER[key], stamp=stamp)
    write_agent(folder, first + 4, f"{arm}|orch|C|verify|r1", [shell(hub_call(bank, reviewer), printed)], TRUTH_A_ANSWER, stamp=stamp)
    return first + 5


class Review(unittest.TestCase):
    """What the review of the first round six harness found. Each test failed before its fix."""

    def parse(self, build):
        with tempfile.TemporaryDirectory() as temp:
            folder = pathlib.Path(temp) / "wf_synthetic"
            folder.mkdir()
            build(folder)
            with contextlib.redirect_stdout(io.StringIO()):
                agents = bench_parse.read_folders([folder])
            cells = {(c["arm"], c["setup"], c["tier"], c["rep"]): c for c in bench_parse.build_cells(agents)}
        return agents, cells

    def test_a_call_that_pgr_refused_is_not_a_hub_call(self):
        bank = "r6-t-th-orch-c-r1"
        agents, cells = self.parse(lambda folder: write_hub_cell(folder, "TH", bank, "file", "load", printed=USAGE_ERROR))
        cell = cells[("TH", "orch", "C", "r1")]
        self.assertIn("pgr:usage-error", cell["off_policy"])
        self.assertEqual(cell["hub_calls"], {"handoff": 0, "loadout": 0, "recall": 0})
        self.assertEqual(sum(row["off_policy"].count("pgr:usage-error") for row in agents), 4)
        # the same calls with an answer from pgr are counted and not flagged
        _, cells = self.parse(lambda folder: write_hub_cell(folder, "TH", bank, "file", "load"))
        cell = cells[("TH", "orch", "C", "r1")]
        self.assertEqual((cell["off_policy"], cell["hub_calls"]), ([], {"handoff": 3, "loadout": 1, "recall": 0}))
        # a PowerShell error wraps the same sentence
        wrapped = "pgr.exe : pgr: unknown command 'handoff' (try: pgr help)\nAt line:1 char:1"
        _, cells = self.parse(lambda folder: write_hub_cell(folder, "TH", bank, "file", "load", printed=wrapped))
        self.assertIn("pgr:usage-error", cells[("TH", "orch", "C", "r1")]["off_policy"])

    def test_a_hub_cell_that_ran_twice_into_one_bank_is_flagged(self):
        bank = "r6-t-th-orch-c-r1"

        def twice(folder):
            at = write_hub_cell(folder, "TH", bank, "file", "load")
            write_hub_cell(folder, "TH", bank, "file", "load", first=at, stamp=LATER)

        agents, cells = self.parse(twice)
        cell = cells[("TH", "orch", "C", "r1")]
        self.assertEqual(len(agents), 5, "the earlier transcripts are dropped")
        self.assertTrue(cell["complete"])
        self.assertIn("bank:rerun", cell["off_policy"])
        self.assertEqual(cell["hub_calls"]["handoff"], 3, "only the kept transcripts are counted")

        def cut_off(folder):
            # the first attempt looked things up and was stopped before it filed anything
            write_agent(folder, 1, "TH|orch|C|w-q1|r1", [shell(f"{WORKDIR}pgr ask about:foo")], None)
            write_hub_cell(folder, "TH", bank, "file", "load", first=2, stamp=LATER)

        _, cells = self.parse(cut_off)
        self.assertEqual(cells[("TH", "orch", "C", "r1")]["off_policy"], [])

        def pack(folder):
            for number, stamp in ((1, "2026-10-13T10:00:00.000Z"), (2, LATER)):
                write_agent(folder, number, "TP|single|C|all|r1", [shell(f"{WORKDIR}pgr ask about:foo")], TRUTH_A_ANSWER, stamp=stamp)

        _, cells = self.parse(pack)
        self.assertEqual(cells[("TP", "single", "C", "r1")]["off_policy"], [])

    def test_hub_use_outside_the_policy_is_flagged(self):
        team = f"{WORKDIR}pgr --bank team-notes --task cli-freeze loadout"

        def build(folder):
            write_agent(folder, 1, "T0|single|C|all|r1", [shell(team)], TRUTH_T_ANSWER)
            write_agent(folder, 2, "TL|single|C|all|r1", [shell(team)], TRUTH_T_ANSWER)
            write_agent(folder, 3, "TL|single|C|all|r2", [shell(hub_call("team-notes", "file"))], TRUTH_T_ANSWER)
            write_agent(folder, 4, "R6|single|C|all|r1", [shell(f"{WORKDIR}pgr --bank x ask about:foo")], TRUTH_A_ANSWER)
            write_agent(folder, 5, "TI|single|C|all|r1", [shell(team)], TRUTH_T_ANSWER)
            at = write_hub_cell(folder, "TH", "r6-t-th-orch-c-r1", "file", "load", first=10)
            # a reviewer that reads a bank that is not the cell's own, and the clean cell of a held-out arm
            write_agent(folder, at, "TPH|orch|C|plan|r1", [], BRIEFS)
            for offset, key in enumerate(common.KEYS, 1):
                write_agent(folder, at + offset, f"TPH|orch|C|w-{key}|r1", [shell(hub_call("r6-t-tph-orch-c-r1", "file"))], TRUTH_A_ANSWER[key])
            write_agent(folder, at + 4, "TPH|orch|C|verify|r1", [shell(hub_call("team-notes", "load"))], TRUTH_A_ANSWER)
            write_hub_cell(folder, "THh", "r6-t-thh-orch-c-r1", "file", "load", first=at + 5)

        _, cells = self.parse(build)
        policy = lambda arm, setup, rep="r1": cells[(arm, setup, "C", rep)]["off_policy"]
        self.assertEqual(policy("T0", "single"), ["bank:foreign", "hub:not-allowed"])
        self.assertEqual(policy("TL", "single"), [], "loading the team bank is what TL is for")
        self.assertEqual(policy("TI", "single"), [])
        self.assertEqual(policy("TL", "single", "r2"), ["hub:not-allowed"], "TL has no handoff command")
        self.assertEqual(policy("R6", "single"), ["bank:foreign"])
        self.assertEqual(policy("TH", "orch"), [])
        self.assertEqual(policy("TPH", "orch"), ["bank:foreign"])
        self.assertEqual(policy("THh", "orch"), [], "the bank of a held-out arm is named after the whole arm id")

    def test_a_finding_piped_into_handoff_is_a_handoff(self):
        bank = "r6-t-th-orch-c-r1"
        for what in ("pipe", "tight"):
            command = hub_call(bank, what)
            calls = [bench_parse.pgr_call(statement) for statement in bench_parse.statements(command)]
            self.assertEqual(calls, [None, ("handoff", bank)], what)
            counts = bench_parse.team_counts([command], {"answer": {}, "tool_calls": 1})
            self.assertEqual((counts["pgr_lookups"], counts["hub_calls"]["handoff"], counts["from_brief"], counts["banks"]), (0, 1, True, [bank]), what)
        looked = bench_parse.team_counts([f"{WORKDIR}pgr ask about:foo | Select-Object -First 5"], {"answer": {}, "tool_calls": 1})
        self.assertEqual((looked["pgr_lookups"], looked["from_brief"]), (1, False))
        self.assertIsNone(bench_parse.pgr_call("Get-Content x | Select-String pgr"))
        # the quoted text in front of the pipe is not a shell program
        _, cells = self.parse(lambda folder: write_hub_cell(folder, "TH", bank, "pipe", "load"))
        cell = cells[("TH", "orch", "C", "r1")]
        self.assertEqual((cell["off_policy"], cell["hub_calls"]["handoff"]), ([], 3))

    def test_note_fields_are_compared_without_case_or_decoration(self):
        grade = lambda q, **changes: bench_parse.grade(q, {**TRUTH_T_ANSWER[q], **changes}, "T")
        for ticket in ("BLD-4172.", "`BLD-4172`", "bld-4172", " BLD-4172 ", "(BLD-4172)", "[BLD-4172]", "**BLD-4172**", "'BLD-4172',"):
            self.assertEqual(grade("q1", ticket=ticket), 1.0, ticket)
        for ticket in ("unknown", "", "BLD-4180", "BLD-417", "Decision BLD-4172", "BLD-4172 BLD-4180"):
            self.assertEqual(grade("q1", ticket=ticket), 0.75, ticket)
        for filed_by in ("Reviewer", "`reviewer`", "reviewer.", " REVIEWER "):
            self.assertEqual(grade("q2", filed_by=filed_by), 1.0, filed_by)
        for filed_by in ("scout", "unknown", "the reviewer", ""):
            self.assertAlmostEqual(grade("q2", filed_by=filed_by), 2 / 3, msg=filed_by)
        self.assertEqual(grade("q2", status="Changed"), 1.0)
        for roles in (["Builder", "Scout"], ["builder+scout"], ["builder, scout"], ["`builder`", "`scout`"], ["scout", "builder"], ["Scout.", "Builder."]):
            self.assertEqual(grade("q3", roles=roles), 1.0, roles)
        for roles in (["scout"], ["builder", "scout", "reviewer"], ["reviewer", "scout"], ["reviewer+scout"], [], "builder+scout", None):
            self.assertEqual(grade("q3", roles=roles), 0.75, roles)


class Reparse(unittest.TestCase):
    def test_a_new_file_may_only_add_round_six_rows(self):
        stored = json.loads(check_reparse.stored_text(common.DATA / "results.json"))
        with tempfile.TemporaryDirectory() as temp:
            temp = pathlib.Path(temp)
            folder = temp / "wf_synthetic"
            folder.mkdir()
            build_round_six(folder)
            added = [a for a in bench_parse.read_folders([folder]) if bench_parse.is_team(a["arm"])]
            agents = stored["agents"] + added
            new = temp / "new.json"
            new.write_text(json.dumps({"agents": agents, "cells": bench_parse.build_cells(agents)}, indent=1), encoding="utf-8")
            self.assertEqual(check_reparse.check_added_only(common.DATA / "results.json", new), [])
            self.assertEqual(check_reparse.main(["--old", str(common.DATA / "results.json"), "--new", str(new)]), 0)
            # an earlier cell that moved is found
            moved = json.loads(new.read_text(encoding="utf-8"))
            moved["cells"][0]["tokens"] += 1
            new.write_text(json.dumps(moved, indent=1), encoding="utf-8")
            problems = check_reparse.check_added_only(common.DATA / "results.json", new)
            self.assertEqual(len(problems), 1)
            self.assertIn("cells of the earlier rounds differ", problems[0])

    def test_the_order_of_the_stored_runs_can_be_printed(self):
        # bench_parse.py wants the old folders in the order the stored rows name their runs, and that order is not written anywhere else.
        stored = json.loads(check_reparse.stored_text(common.DATA / "results.json"))
        runs = list(dict.fromkeys(row["run"] for row in stored["agents"]))
        said = io.StringIO()
        with contextlib.redirect_stdout(said):
            self.assertEqual(check_reparse.main(["--print-order"]), 0)
        self.assertEqual(said.getvalue().split("\n"), runs + [""])
        said = io.StringIO()
        with contextlib.redirect_stdout(said):
            self.assertEqual(check_reparse.main(["--print-order", "--transcripts", "ROOT"]), 0)
        self.assertEqual(said.getvalue().split("\n"), [str(pathlib.Path("ROOT") / run) for run in runs] + [""])


class Scripts(unittest.TestCase):
    def test_every_script_starts_under_isolated_python(self):
        # ROUND6.md runs them as `python -I script`, which leaves the script's own folder off the path.
        for name in ("prepare_packs.py", "prepare_team.py", "round6_args.py", "check_reparse.py", "bench_parse.py", "fake_pgr.py",
                     "prepare_words.py", "prepare_wsl.py", "fake_wsl.py"):
            args = [sys.executable, "-I", str(HERE / name)] + ([] if name == "bench_parse.py" else ["--help"])
            done = subprocess.run(args, capture_output=True, text=True, encoding="utf-8")
            if name in ("fake_pgr.py", "fake_wsl.py"):
                self.assertEqual(done.returncode, 2, name)        # no command: usage error, as pgr and wsl.exe
            elif name == "bench_parse.py":
                self.assertNotIn("ModuleNotFoundError", done.stderr, name)
            else:
                self.assertEqual(done.returncode, 0, f"{name}: {done.stderr[-300:]}")
                self.assertIn("usage:", done.stdout, name)


class Documentation(unittest.TestCase):
    def test_round6_md_uses_real_scripts_flags_and_args(self):
        text = (HERE / "ROUND6.md").read_text(encoding="utf-8")
        commands = re.findall(r"harness/(\w+\.py)([^`\n]*)", text)
        self.assertGreaterEqual(len(commands), 7)
        for script, rest in commands:
            source = (HERE / script).read_text(encoding="utf-8")
            for flag in re.findall(r"(?<=\s)(--[a-z][a-z-]+)", rest):
                self.assertIn(f'"{flag}"', source, f"{script} has no {flag}")
        workflow = (HERE / "token-benchmark.workflow.js").read_text(encoding="utf-8")
        table = text.split("## What each args field holds")[1].split("`round6_args.py` fills")[0]
        for field in re.findall(r"^\| `(\w+)` \|", table, re.M):
            self.assertRegex(workflow, rf"args\.{field}\b", f"the workflow does not read args.{field}")
        for name in ("prepare_packs.py", "prepare_team.py", "round6_args.py", "check_reparse.py", "bench_parse.py", "test_round6.py", "check_round6_prompts.js",
                     "prepare_words.py", "prepare_wsl.py"):
            self.assertIn(name, text)
        self.assertIn("81 cells, 201 agents", text)
        self.assertIn("54 cells, 150 agents", text)

    def test_round6_md_gives_the_order_of_the_four_halves_and_the_second_batch(self):
        text = " ".join((HERE / "ROUND6.md").read_text(encoding="utf-8").split())
        for needle in ("round6-c.json", "round6-d.json", "PGR_NO_MODEL", "wsl.exe", "`wsl`", "`repoWords`", "--linux-pgr", "--repo-words", "--wsl-distro",
                       "R6V", "THV", "TPHV", "RN", "TPN", "TPX", "Three models, each in its role", "byte for byte", "--count"):
            self.assertIn(needle, text)
        order = [text.index(f"--half {half}") for half in "abcd"]
        self.assertEqual(order, sorted(order), "the halves are described in the order a, b, c, d")

    def test_round6_md_tells_what_the_review_found_missing(self):
        text = " ".join((HERE / "ROUND6.md").read_text(encoding="utf-8").split())
        for needle in ("pgr --version", "first on PATH", "unknown option --bank", "runs-hub", "--verify", "does not remove the bank",
                       "--print-order", "bank:rerun", "pgr:usage-error", "`sums`", "never written inside the repository"):
            self.assertIn(needle, text)


class Preparation(unittest.TestCase):
    """prepare_packs.py and prepare_team.py against the stand-in."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        self.fixture, self.home = self.root / "repo", self.root / "home"
        make_fixture(self.fixture)

    def tearDown(self):
        self.temp.cleanup()

    def leaks(self, text: str) -> list[str]:
        """The spellings of this test's temporary folder that a published file holds: as written, with
        slashes, and as written inside a JSON string."""
        spellings = {str(self.root), str(self.root).replace("\\", "/"), json.dumps(str(self.root))[1:-1]}
        return [spelling for spelling in spellings if spelling in text]

    def packs(self, *extra, env=None):
        pgr = common.Pgr(stand_in(), home=self.home)
        pgr.run("--root", self.fixture, "refresh")
        out = self.root / "packs.json"
        saved = {key: os.environ.get(key) for key in (env or {})}
        os.environ.update(env or {})
        try:
            code = prepare_packs.main(["--pgr", stand_in(), "--fixture", str(self.fixture), "--home", str(self.home), "--out", str(out), *extra])
        finally:
            for key, value in saved.items():
                os.environ.pop(key, None) if value is None else os.environ.__setitem__(key, value)
        return code, out

    def test_packs(self):
        code, out = self.packs()
        self.assertEqual(code, 0)
        doc = json.loads(out.read_text(encoding="utf-8"))
        questions = common.workflow_questions()
        self.assertEqual(common.check_packs(doc, questions), [])
        self.assertEqual(doc["pgr_version"], "pgr 0.2.0")
        self.assertTrue(doc["status"].startswith("pgr: repo | graph ready"))
        self.assertEqual(sum(len(by_q) for by_set in doc["packs"].values() for by_q in by_set.values()), 18)
        pack = doc["packs"]["gs"]["A"]["q2"]
        self.assertEqual(pack["question"], questions["A"]["q2"])
        self.assertEqual(pack["tokens"], common.est_tokens(pack["output"]))
        self.assertEqual(pack["sha256"], hashlib.sha256(pack["output"].encode("utf-8")).hexdigest())
        self.assertIn("--engines graph,search pack", pack["command"])
        self.assertIn("--engines graph pack", doc["packs"]["g"]["A"]["q2"]["command"])
        self.assertIn("--engines search pack", doc["packs"]["s"]["A"]["q2"]["command"])
        self.assertTrue(pack["command"].startswith("pgr --root C:\\path\\to\\lxbench\\repo "))
        self.assertTrue(pack["command"].endswith("--budget 800"))
        self.assertEqual(len({doc["packs"][v]["A"]["q1"]["output"] for v in ("gs", "g", "s")}), 3)
        self.assertFalse(self.leaks(out.read_text(encoding="utf-8")))
        self.assertFalse(out.read_bytes().count(b"\r"))

    def test_packs_refuse_a_pgr_that_is_too_old(self):
        code, out = self.packs(env={"FAKE_PGR_VERSION": "0.1.0"})
        self.assertEqual(code, 1)
        self.assertFalse(out.exists())

    def test_packs_refuse_a_missing_pack(self):
        question = common.workflow_questions()["B"]["q2"]
        for variable in ("FAKE_PGR_FAIL_PACK", "FAKE_PGR_EMPTY_PACK"):
            code, out = self.packs(env={variable: question[:40]})
            self.assertEqual(code, 1, variable)
            self.assertFalse(out.exists(), variable)

    def test_packs_refuse_a_fixture_without_an_index(self):
        out = self.root / "packs.json"
        code = prepare_packs.main(["--pgr", stand_in(), "--fixture", str(self.fixture), "--home", str(self.home), "--out", str(out)])
        self.assertEqual((code, out.exists()), (1, False))

    def test_check_packs_finds_what_is_wrong(self):
        code, out = self.packs()
        doc = json.loads(out.read_text(encoding="utf-8"))
        questions = common.workflow_questions()
        self.assertEqual(common.check_packs({**doc, "packs": {"gs": doc["packs"]["gs"]}}, questions)[0], "packs.g.A.q1 is missing")
        stale = json.loads(json.dumps(doc))
        stale["packs"]["g"]["B"]["q3"]["question"] = "an older text"
        stale["packs"]["s"]["A"]["q1"]["tokens"] += 1
        output = stale["packs"]["gs"]["A"]["q2"]["output"]
        stale["packs"]["gs"]["A"]["q2"]["output"] = output.replace("a", "b", 1)
        problems = common.check_packs(stale, questions)
        self.assertEqual(len(problems), 3, problems)
        self.assertTrue(any("another question text" in p for p in problems) and any("tokens" in p for p in problems) and any("sha256" in p for p in problems))

    def prepare(self, *extra, name="team_notes.json"):
        out = self.root / name
        argv = ["--pgr", stand_in(), "--fixture", str(self.fixture), "--team", str(self.root / "repo-team"), "--home", str(self.home), "--out", str(out)]
        return prepare_team.main(argv + list(extra)), out

    def test_team(self):
        before = tree_hash(self.fixture)
        code, out = self.prepare()
        self.assertEqual(code, 0)
        self.assertEqual(tree_hash(self.fixture), before, "the fixture was modified")
        doc = json.loads(out.read_text(encoding="utf-8"))
        self.assertEqual(len(doc["notes"]), len(prepare_team.NOTES))
        self.assertEqual([note["outcome"] for note in doc["notes"]].count("reinforced"), 2)
        self.assertEqual(set(doc["loadouts"]), set(prepare_team.tasks_in_order()))
        self.assertEqual(len(doc["loadouts"]), 8)
        q1, q2, q3 = (doc["loadouts"][prepare_team.TASKS[key]]["output"] for key in common.KEYS)
        self.assertIn("BLD-4172", q1)
        self.assertNotIn("BLD-4180", q1)
        self.assertEqual(sum("stale: code changed" in line for line in q2.split("\n")), 1)
        self.assertNotIn("stale: code changed", q1 + q3)
        self.assertIn("[builder+scout]", q3)
        self.assertIn("[reviewer+scout]", q3, "the near-miss fact is a confirmed note and shows in every core layer")
        for entry in doc["loadouts"].values():
            self.assertEqual(entry["tokens"], common.est_tokens(entry["output"]))
            self.assertEqual(entry["sha256"], common.sha256(entry["output"]))
        # the edit changes one line and nothing else
        edit = doc["edit"]
        self.assertEqual(edit["lines_before"], edit["lines_after"])
        original = (self.fixture / "security.py").read_text(encoding="utf-8").split("\n")
        copy = (self.root / "repo-team" / "security.py").read_text(encoding="utf-8").split("\n")
        self.assertEqual([n + 1 for n, (a, b) in enumerate(zip(original, copy)) if a != b], [121])
        self.assertEqual(copy[102], "def validate_url(url: str) -> str:")
        # nothing private is published
        text = out.read_text(encoding="utf-8")
        self.assertFalse(self.leaks(text))
        self.assertIn(json.dumps(common.TEAM_PLACEHOLDER)[1:-1], text)
        # the bank is unchanged after reads, and the verify mode says when it is not
        self.assertEqual(prepare_team.verify(doc, self.home), [])
        bank_file = self.home / "banks" / doc["bank"] / "notes.json"
        bank_file.write_text(bank_file.read_text(encoding="utf-8") + " ", encoding="utf-8")
        self.assertEqual(prepare_team.verify(doc, self.home), ["changed: notes.json"])
        self.assertEqual(prepare_team.main(["--verify", "--out", str(out), "--home", str(self.home)]), 1)
        # it will not run over an earlier run
        again, _ = self.prepare()
        self.assertEqual(again, 1)

    def test_team_replace_rebuilds_a_copy_this_script_made(self):
        self.assertEqual(self.prepare()[0], 0)
        self.assertEqual(self.prepare(name="again.json")[0], 1, "an existing team folder is not touched without --replace")
        shutil.rmtree(self.home / "banks")
        self.assertEqual(self.prepare("--replace", name="again.json")[0], 0)
        (self.root / "repo-team.round6-team").unlink()
        shutil.rmtree(self.home / "banks")
        self.assertEqual(self.prepare("--replace", name="third.json")[0], 1, "a folder the script did not make must not be removed")
        self.assertTrue((self.root / "repo-team" / "security.py").exists())

    def test_team_refuses_pgr_01_and_a_fixture_that_is_not_the_one(self):
        os.environ["FAKE_PGR_VERSION"] = "0.1.0"
        try:
            self.assertEqual(self.prepare()[0], 1)
        finally:
            del os.environ["FAKE_PGR_VERSION"]
        self.assertFalse((self.root / "repo-team").exists())
        path = self.fixture / "security.py"
        path.write_text(path.read_text(encoding="utf-8").replace("hostname.lower()", "host.lower()"), encoding="utf-8", newline="\n")
        self.assertEqual(self.prepare()[0], 1)


class Args(unittest.TestCase):
    """round6_args.py against the stand-in. The fixture, the packs and the team bank are prepared once; every test
    works on its own copy of the index home, so that a test can spoil a bank without spoiling the next one."""

    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = pathlib.Path(cls.temp.name)
        cls.fixture, cls.prepared_home = cls.root / "repo", cls.root / "prepared-home"
        make_fixture(cls.fixture)
        common.Pgr(stand_in(), home=cls.prepared_home).run("--root", cls.fixture, "refresh")
        quiet = io.StringIO()
        with contextlib.redirect_stdout(quiet):
            made = prepare_packs.main(["--pgr", stand_in(), "--fixture", str(cls.fixture), "--home", str(cls.prepared_home), "--out", str(cls.root / "packs.json")])
            filed = prepare_team.main(["--pgr", stand_in(), "--fixture", str(cls.fixture), "--team", str(cls.root / "repo-team"), "--home", str(cls.prepared_home),
                                       "--out", str(cls.root / "team_notes.json")])
        assert (made, filed) == (0, 0), quiet.getvalue()

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def setUp(self):
        self.home = pathlib.Path(tempfile.mkdtemp(dir=self.root)) / "home"
        shutil.copytree(self.prepared_home, self.home)

    def args_argv(self, half, tag="t1", out=None):
        """round6_args.py against the stand-in, the way ROUND6.md runs it against pgr."""
        return ["--half", half, "--run-tag", tag, "--repo", str(self.fixture), "--repo-team", str(self.root / "repo-team"),
                "--packs", str(self.root / "packs.json"), "--team-notes", str(self.root / "team_notes.json"),
                "--pgr", stand_in(), "--home", str(self.home), "--out", str(out or self.home.parent / f"args-{half}.json")]

    def refused(self, argv) -> str:
        """What round6_args.main prints on standard error when it refuses (exit code 1)."""
        said = io.StringIO()
        with contextlib.redirect_stderr(said):
            self.assertEqual(round6_args.main(argv), 1)
        return said.getvalue()

    def test_args_for_both_halves_start_in_the_workflow(self):
        for half in "ab":
            out = self.home.parent / f"args-{half}.json"
            self.assertEqual(round6_args.main(self.args_argv(half)), 0)
            args = json.loads(out.read_text(encoding="utf-8"))
            self.assertEqual(args["runTag"], "t1")
            self.assertEqual(("loadouts" in args, "repoTeam" in args, "teamBank" in args), (half == "a",) * 3)
            self.assertEqual(sorted(args["packs"]), ["g", "gs", "s"] if half == "a" else ["gs"])
            # every text the workflow is given has a checksum, by place, and the checksum is the hash of the text
            expected = {f"packs/{variant}/{letter}/{key}": common.sha256(args["packs"][variant][letter][key]["output"])
                        for variant in args["packs"] for letter in common.SETS for key in common.KEYS}
            if half == "a":
                expected.update({f"loadouts/{key}": common.sha256(args["loadouts"][key]["output"]) for key in common.KEYS})
            self.assertEqual(args["sums"], expected)
            if NODE:
                done = run_node("--args", out)
                self.assertEqual(done.returncode, 0, done.stdout)
                matrix = json.loads((HERE / f"round6-{half}.json").read_text(encoding="utf-8"))
                agents = sum(len(b["arms"]) * len(b["reps"]) * {"single": 1, "parallel": 3, "orch": 5}[b["setup"]] for b in matrix)
                self.assertIn(f"{agents} agents", done.stdout)

    def test_args_refuse_what_is_missing(self):
        none = self.root / "none.json"
        base = ["--run-tag", "t1", "--repo", "C:\\path\\to\\lxbench\\repo", "--repo-team", "C:\\path\\to\\lxbench\\repo-team",
                "--packs", str(none), "--team-notes", str(none), "--out", str(self.root / "a.json"), "--pgr", stand_in(), "--home", str(self.home)]
        self.assertEqual(round6_args.main(["--half", "a", *base]), 1)
        self.assertEqual(round6_args.main(["--half", "b", *base]), 1)
        self.assertEqual(round6_args.main(["--half", "b", *base[:1], "Bad Tag", *base[2:]]), 1)
        self.assertEqual(round6_args.main(["--half", "b", *base[:3], "C:\\a path\\repo", *base[4:]]), 1)

    def test_args_refuse_a_pgr_that_is_too_old(self):
        # The agents run a bare `pgr`: the one the shell finds. pgr 0.1 rejects every hub command.
        os.environ["FAKE_PGR_VERSION"] = "0.1.0"
        try:
            message = self.refused(self.args_argv("b"))
        finally:
            del os.environ["FAKE_PGR_VERSION"]
        self.assertIn("pgr 0.1.0", message)
        self.assertIn("PATH", message)
        self.assertFalse((self.home.parent / "args-b.json").exists())

    def test_args_refuse_an_index_that_this_pgr_cannot_read(self):
        # pgr 0.2 reports the index that pgr 0.1 built as missing; the stand-in reports a home with no index the same way.
        argv = self.args_argv("a")
        argv[argv.index("--home") + 1] = str(self.root / "another-home")
        message = self.refused(argv)
        self.assertIn("no index yet", message)
        self.assertIn("refresh", message)
        self.assertFalse((self.home.parent / "args-a.json").exists())

    def test_args_refuse_a_run_tag_whose_banks_exist(self):
        # A run that is run again must not find the notes its first attempt filed.
        (self.home / "banks" / "r6-t1-th-orch-c-r1").mkdir(parents=True)
        message = self.refused(self.args_argv("b"))
        self.assertIn("r6-t1-th-orch-c-r1", message)
        self.assertEqual(round6_args.main(self.args_argv("b", tag="t2")), 0)
        self.assertEqual(round6_args.main(self.args_argv("a")), 0, "half a has no hub arm")

    def test_args_refuse_a_team_bank_that_is_not_the_prepared_one(self):
        notes = self.home / "banks" / "team-notes" / "notes.json"
        notes.write_text(notes.read_text(encoding="utf-8") + " ", encoding="utf-8")
        self.assertIn("changed: notes.json", self.refused(self.args_argv("a")))
        self.assertEqual(round6_args.main(self.args_argv("b")), 0, "half b has no set T arm")
        shutil.rmtree(self.home / "banks" / "team-notes")
        self.assertIn("missing: notes.json", self.refused(self.args_argv("a")))

    def test_args_are_not_written_inside_the_repository(self):
        # The args hold the owner's fixture paths and the repository is public.
        inside = HERE / "args-written-by-a-test.json"
        try:
            self.assertIn("repository", self.refused(self.args_argv("a", out=inside)))
            self.assertFalse(inside.exists())
        finally:
            inside.unlink(missing_ok=True)
        default = round6_args.default_out("a")
        self.assertEqual(default.name, "round6-a-args.json")
        self.assertNotIn(common.REPO, [default.parent, *default.parents])


class Wording(unittest.TestCase):
    """Texts that will be published or that an agent reads: they must say what is true."""

    def test_no_question_of_set_t_gives_a_note_field_away(self):
        # A floor that cannot know the notes must not be told them by the question.
        for key, text in common.workflow_questions()["T"].items():
            self.assertIsNone(re.search(r"(?i)\b(scout|builder|reviewer)\b", text), f"{key} names a role")
            self.assertIsNone(re.search(r"\b[A-Z]{2,}-\d+\b", text), f"{key} names a ticket")

    def test_the_schema_of_set_t_is_plain(self):
        source = (HERE / "token-benchmark.workflow.js").read_text(encoding="utf-8")
        self.assertIsNone(re.search(r"type:\s*\[", source), "a type list in a schema: the workflow runtime may not take it")
        self.assertIn("'changed if the code of the function changed since the note was filed", source)

    def test_labels_and_truths_say_what_they_mean(self):
        arms = json.loads((HERE / "arms.json").read_text(encoding="utf-8"))
        names = {arm["id"]: arm["name"] for arm in arms["arms"]}
        for arm in ("T0", "TL", "TI"):
            self.assertIn("demonstration set", names[arm].lower(), arm)
        q2 = next(q for q in arms["demonstration_set"]["questions"] if q["id"] == "q2")
        self.assertNotIn("the note has changed", q2["truth"])
        self.assertIn("the code of the function has changed since the note was filed", q2["truth"])
        self.assertIn("four more fields", " ".join(bench_parse.__doc__.split()))


class Hygiene(unittest.TestCase):
    def test_nothing_personal_and_no_em_dash(self):
        owner = "ch" + "arl"        # the owner's account name, split so that this file does not hold it
        for name in NEW_FILES:
            text = (HERE / name).read_text(encoding="utf-8")
            self.assertNotIn(owner, text.lower(), name)
            # bench_parse.py shows the shape of a home path with the made-up user name "name"
            self.assertNotRegex(text, r"(?i)[a-z]:[\\/]+users[\\/]+(?![\\/]|name\b)", name)
            self.assertNotIn("\u2014", text, f"{name} holds an em dash")

    def test_the_notes_are_what_the_questions_need(self):
        prepare_team.validate_plan(common.workflow_questions())
        texts = [text for _, _, text in prepare_team.NOTES]
        self.assertEqual(len({text[:50] for text in set(texts)}), len(set(texts)), "two notes begin with the same 50 characters")
        self.assertEqual(len(texts), 31)
        self.assertEqual({role for _, role, _ in prepare_team.NOTES}, set(prepare_team.ROLES))

    @unittest.skipUnless(os.environ.get("ROUND6_FIXTURE"), "ROUND6_FIXTURE is not set")
    def test_the_notes_fit_the_fixture(self):
        root = pathlib.Path(os.environ["ROUND6_FIXTURE"])
        import ast

        count: dict[str, int] = {}
        for path in root.rglob("*.py"):
            if ".git" in path.parts:
                continue
            try:
                tree = ast.parse(path.read_text(encoding="utf-8", errors="replace"))
            except SyntaxError:
                continue
            for node in ast.walk(tree):
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    count[node.name] = count.get(node.name, 0) + 1
        for _, _, text in prepare_team.NOTES:
            for name in prepare_team.code_names(text):
                self.assertEqual(count.get(name), 1, f"{name} is defined {count.get(name)} times in the fixture")
        line = (root / prepare_team.EDIT["file"]).read_bytes().split(b"\n")[prepare_team.EDIT["line"] - 1].rstrip(b"\r").decode("utf-8")
        self.assertEqual(line, prepare_team.EDIT["old"])


from unittest import mock  # noqa: E402

import prepare_words  # noqa: E402
import prepare_wsl  # noqa: E402

VM_START = "wsl -d Ubuntu --cd /home/u/lxbench/repo --exec /home/u/lxbench/bin/pgr"
VM_SED = 'wsl -d Ubuntu --cd /home/u/lxbench/repo --exec sed -n "10,20p" cli.py'
WORDS_START = "$env:PGR_NO_MODEL='1'; Set-Location C:\\path\\to\\lxbench\\repo-words; "


def parse_folder(build):
    """The agents and the cells that synthetic transcripts parse to."""
    with tempfile.TemporaryDirectory() as temp:
        folder = pathlib.Path(temp) / "wf_synthetic"
        folder.mkdir()
        build(folder)
        with contextlib.redirect_stdout(io.StringIO()):
            agents = bench_parse.read_folders([folder])
        cells = {(c["arm"], c["setup"], c["tier"], c["rep"]): c for c in bench_parse.build_cells(agents)}
    return agents, cells


def vm_hub_call(bank: str, what: str) -> str:
    base = f"{VM_START} --bank {bank} --task lookups"
    return {"file": f"{base} --from scout handoff 'found it in a.py:1; b calls it'", "load": f"{base} loadout"}[what]


def write_vm_hub_cell(folder: pathlib.Path, arm: str, bank: str, reviewer_bank: str | None = None, first: int = 1) -> int:
    """The planner, three workers that file a finding and a reviewer that loads the notes, every call through wsl."""
    write_agent(folder, first, f"{arm}|orch|C|plan|r1", [], BRIEFS)
    for offset, key in enumerate(common.KEYS, 1):
        write_agent(folder, first + offset, f"{arm}|orch|C|w-{key}|r1", [shell(vm_hub_call(bank, "file"))], TRUTH_A_ANSWER[key])
    write_agent(folder, first + 4, f"{arm}|orch|C|verify|r1", [shell(vm_hub_call(reviewer_bank or bank, "load"))], TRUTH_A_ANSWER)
    return first + 5


class MoreArmsParser(unittest.TestCase):
    """The second batch of arms in the parser: the tools in a virtual machine (R6V, THV, TPHV), no neural network
    (RN, TPN) and the pack only (TPX)."""

    def test_wsl_calls_are_pgr_calls(self):
        call = bench_parse.pgr_call
        self.assertEqual(call(f"{VM_START} ask about:foo"), ("ask", ""))
        self.assertEqual(call(f"{VM_START} --bank r6-t-thv-orch-c-r1 --task lookups --from scout handoff 'a; b'"), ("handoff", "r6-t-thv-orch-c-r1"))
        self.assertEqual(call("wsl.exe -d Ubuntu --cd /x --exec /x/bin/pgr --bank=b --task t loadout"), ("loadout", "b"))
        self.assertEqual(call(f"{VM_START} ask about:foo | Select-Object -First 5"), ("ask", ""))
        self.assertIsNone(call(VM_SED), "sed is not a pgr call")
        # a call that does not start pgr directly, or that starts a Windows program, is not a pgr call of the VM
        for other in ("wsl -d Ubuntu --exec /mnt/c/bin/pgr.exe ask x", "wsl -d Ubuntu -- /x/bin/pgr ask x", "wsl -d Ubuntu /x/bin/pgr ask x", "wsl"):
            self.assertIsNone(call(other), other)
        self.assertEqual(call("pgr ask about:foo"), ("ask", ""), "a Windows pgr is still a pgr call; the shell word marks it off policy")
        # a call that starts in a Windows folder, in none, or that climbs to the drives is no lookup of the VM
        for out in ("wsl -d Ubuntu --cd /mnt/c/x --exec /x/bin/pgr ask x", "wsl -d Ubuntu --cd C:\\x --exec /x/bin/pgr ask x", "wsl -d Ubuntu --exec /x/bin/pgr ask x",
                    "wsl -d Ubuntu --cd /x/repo --exec /x/bin/pgr --root ../../mnt/c/y ask z"):
            self.assertIsNone(call(out), out)
        # for an arm in the VM the bare Windows pgr is not one at all
        self.assertEqual(call(f"{VM_START} ask about:foo", True), ("ask", ""))
        self.assertIsNone(call("pgr ask about:foo", True))

    def test_what_a_vm_arm_may_not_do(self):
        flags = bench_parse.vm_flags
        cd = "--cd /home/u/lxbench/repo"
        self.assertEqual(flags([f"{VM_START} ask about:foo", VM_SED, f"{VM_START} --bank b --task t loadout | Select-Object -First 3"]), [])
        self.assertEqual(flags([f"'found it in a.py:1' | {VM_START} --bank b handoff -", f"Set-Location C:\\x; {VM_START} ask a", f"{VM_START} ask a | Select-String foo"]), [],
                         "a quoted text piped in, a Set-Location prefix and a filter after the pipe stay inside")
        self.assertEqual(flags(["wsl -d Ubuntu -- pgr ask x"]), ["wsl:no-exec"])
        self.assertEqual(flags([f"wsl -d Ubuntu {cd} --exec grep -rn x ."]), ["wsl:other-command"])
        self.assertEqual(flags([f"wsl -d Ubuntu {cd} --exec sed -n 1,3p /mnt/c/x.py"]), ["wsl:leaves-vm"])
        self.assertEqual(flags(["wsl.exe -d Ubuntu --exec /mnt/c/bin/pgr.exe ask x"]), ["wsl:leaves-vm"])
        # a call that runs on the Windows drives: the folder is a Windows one or under /mnt/, there is none (wsl then starts in the
        # Windows folder of the caller), or a path in the call climbs out of the VM's files
        for way in ("--cd /mnt/c/Users/x/lxbench/repo --exec /home/u/lxbench/bin/pgr ask about:x",
                    "--cd C:\\Users\\name\\lxbench\\repo --exec sed -n 1,40p cli.py",
                    "--exec /home/u/lxbench/bin/pgr ask about:x",
                    f"{cd}/../../../../mnt/c --exec /home/u/lxbench/bin/pgr ask about:x",
                    "--cd ~/../../mnt/c --exec /home/u/lxbench/bin/pgr ask about:x",
                    f"{cd} --exec sed -n 1,40p ../../../../mnt/c/Users/x/secret.txt",
                    f"{cd} --exec /home/u/lxbench/bin/pgr --root=/mnt/c/x ask about:x"):
            self.assertEqual(flags([f"wsl -d Ubuntu {way}"]), ["wsl:leaves-vm"], way)
        for inside in (f"{cd} --exec sed -n 1,40p ../bin/notes.txt", "--cd ~ --exec sed -n 1,3p x.py", f"{cd} --exec sed -n 1,3p /etc/os-release"):
            self.assertEqual(flags([f"wsl -d Ubuntu {inside}"]), [], inside)
        # a Windows program is a way out wherever it stands in the call: first, after the wsl call, behind a pipe, or on another line
        for out in (f"Set-Location C:\\x; pgr ask a; {VM_SED}", f"{VM_START} ask a; Set-Location C:\\x; pgr ask b", f"{VM_START} ask a | pgr ask b", "pgr ask a",
                    f"{VM_SED}; Get-Content C:\\x\\cli.py -TotalCount 40", f"{VM_START} ask a && type cli.py", f"{VM_SED}\nrg x .",
                    f"{VM_START} ask a | Select-String foo C:\\x\\y.py", "Get-Content C:\\x\\cli.py"):
            self.assertEqual(flags([out]), ["wsl:leaves-vm"], out)
        # only the wsl calls of an arm in the VM are counted as its lookups
        windows_after = f"{VM_START} ask a; Set-Location C:\\x; pgr ask b"
        counted = lambda vm: bench_parse.team_counts([windows_after], {"answer": {}, "tool_calls": 1}, vm=vm)["pgr_lookups"]
        self.assertEqual((counted(False), counted(True)), (2, 1), "a Windows pgr behind the wsl call is no lookup of the VM")
        row = lambda tools, words=None: {"tools": tools, "shell": words or {}}
        self.assertEqual(bench_parse.off_policy("R6V", row({"PowerShell": 3}, {"wsl": 3}), "single", "all"), [])
        self.assertEqual(bench_parse.off_policy("THVh", row({"PowerShell": 1}, {"wsl.exe": 1}), "orch", "w-q1"), [])
        self.assertEqual(bench_parse.off_policy("R6V", row({"PowerShell": 1, "Read": 2, "Grep": 1, "Glob": 1}, {"pgr": 1}), "single", "all"),
                         ["shell:pgr", "Read", "Grep", "Glob"])
        self.assertEqual(bench_parse.off_policy("R6", row({"PowerShell": 1, "Read": 1, "Grep": 1}, {"pgr": 1}), "single", "all"), [],
                         "the Read and Grep tools are fine for an arm that is not in the VM")
        # no tool at all for the pack-only arm, whichever it was
        self.assertEqual(bench_parse.off_policy("TPX", row({}), "single", "all"), [])
        self.assertEqual(bench_parse.off_policy("TPXh", row({"PowerShell": 1, "Read": 1}, {"pgr": 1}), "single", "all"), ["PowerShell", "Read"])

    def test_the_words_only_prefix_is_not_the_program(self):
        command = WORDS_START + "pgr ask about:foo"
        self.assertEqual(bench_parse.first_word(command, env=True), "pgr")
        self.assertNotEqual(bench_parse.first_word(command), "pgr", "an earlier arm keeps the word it always had")
        counts = bench_parse.team_counts([command], {"answer": {}, "tool_calls": 1})
        self.assertEqual((counts["pgr_lookups"], counts["from_brief"]), (1, False))
        self.assertEqual(bench_parse.off_policy("RN", {"tools": {"PowerShell": 1}, "shell": {"pgr": 1}}, "single", "all"), [])

    def test_a_path_inside_the_vm_is_cut_like_a_windows_one(self):
        self.assertEqual(bench_parse.portable({"file": "/home/alex/lxbench/repo/hooks.py", "callers": ["/home/USER/lxbench/repo/cli.py:12", "main"]}),
                         {"file": "hooks.py", "callers": ["cli.py:12", "main"]})
        self.assertEqual(bench_parse.portable("hooks.py"), "hooks.py")
        self.assertEqual(bench_parse.portable({"file": "/home/alex/projects/rounds/repo/hooks.py"}), {"file": "hooks.py"}, "the folder given as --vm-dir is cut too")
        self.assertEqual(bench_parse.portable("/home/alex/repo-notes/x.py"), "/home/alex/repo-notes/x.py", "only a folder called repo is cut")
        self.assertEqual(bench_parse.portable({"file": "C:\\Users\\name\\Downloads\\lxbench\\repo-words\\graphify\\cache.py"}), {"file": "graphify\\cache.py"},
                         "the words-only copy is cut like the other copies of the fixture")

    def test_rows_and_cells_of_the_new_arms(self):
        def build(folder):
            write_agent(folder, 1, "R6V|single|C|all|r1", [shell(f"{VM_START} ask about:foo"), shell(VM_SED)], TRUTH_A_ANSWER)
            # left the VM: a Windows pgr, the Read tool, Grep
            write_agent(folder, 2, "R6V|single|C|all|r2", [shell(f"{WORKDIR}pgr ask about:foo"), ("Read", {"file_path": "x"}), ("Grep", {"pattern": "x"})], TRUTH_A_ANSWER)
            write_agent(folder, 3, "R6V|single|C|all|r3", [shell("wsl -d Ubuntu --cd /home/u/lxbench/repo --exec grep -rn x .")], TRUTH_A_ANSWER)
            write_agent(folder, 4, "R6V|single|C|all|r4", [shell(f"{VM_START} --bank b ask x", USAGE_ERROR)], TRUTH_A_ANSWER)
            at = write_vm_hub_cell(folder, "THV", "r6-t-thv-orch-c-r1", first=10)
            write_vm_hub_cell(folder, "TPHV", "r6-t-tphv-orch-c-r1", reviewer_bank="team-notes", first=at)
            write_agent(folder, 40, "RN|single|C|all|r1", [shell(WORDS_START + "pgr ask about:foo")], TRUTH_A_ANSWER)
            write_agent(folder, 41, "TPNh|single|C|all|r1", [], TRUTH_A_ANSWER)
            write_agent(folder, 42, "TPX|single|C|all|r1", [], TRUTH_A_ANSWER)
            write_agent(folder, 43, "TPX|single|B|all|r1", [shell("pgr ask about:foo")], TRUTH_A_ANSWER)

        agents, cells = parse_folder(build)
        cell = lambda arm, setup, tier="C", rep="r1": cells[(arm, setup, tier, rep)]
        rows = {(row["arm"], row["role"], row["rep"], row["tier"]): row for row in agents}
        looked = rows[("R6V", "all", "r1", "C")]
        self.assertEqual((looked["pgr_lookups"], looked["from_brief"], looked["shell"], looked["off_policy"]), (1, False, {"wsl": 2}, []))
        self.assertEqual(cell("R6V", "single")["off_policy"], [])
        self.assertEqual(cell("R6V", "single", rep="r2")["off_policy"], ["Grep", "Read", "shell:pgr", "wsl:leaves-vm"])
        self.assertEqual(cell("R6V", "single", rep="r2")["pgr_lookups"], 0, "a Windows pgr is no lookup of the VM")
        self.assertEqual(cell("R6V", "single", rep="r3")["off_policy"], ["wsl:other-command"])
        self.assertEqual(cell("R6V", "single", rep="r4")["off_policy"], ["pgr:usage-error"])
        self.assertEqual(cell("R6V", "single", rep="r4")["pgr_lookups"], 0, "a call pgr refused is counted nowhere")
        # the hub in the VM: three findings filed, the notes loaded, nothing foreign
        hub = cell("THV", "orch")
        self.assertEqual((hub["off_policy"], hub["hub_calls"], hub["banks"]), ([], {"handoff": 3, "loadout": 1, "recall": 0}, ["r6-t-thv-orch-c-r1"]))
        self.assertEqual(hub["from_brief_share"], 0.75, "the three workers only filed; the reviewer loaded the notes")
        self.assertEqual(cell("TPHV", "orch")["off_policy"], ["bank:foreign"])
        # no neural network: the prefix does not hide the program
        words = cell("RN", "single")
        self.assertEqual((words["off_policy"], words["pgr_lookups"], words["from_brief_share"]), ([], 1, 0.0))
        self.assertEqual(rows[("RN", "all", "r1", "C")]["shell"], {"pgr": 1})
        # the pack as the only help
        self.assertEqual((cell("TPNh", "single")["from_brief_share"], cell("TPNh", "single")["off_policy"]), (1.0, []))
        self.assertEqual((cell("TPX", "single")["from_brief_share"], cell("TPX", "single")["off_policy"]), (1.0, []))
        self.assertEqual((cell("TPX", "single", "B")["from_brief_share"], cell("TPX", "single", "B")["off_policy"]), (0.0, ["PowerShell"]))
        self.assertEqual(cell("TPNh", "single")["set"], "B")


class MoreArmsPreparation(unittest.TestCase):
    """prepare_words.py and prepare_wsl.py against the stand-ins fake_pgr.py and fake_wsl.py."""

    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = pathlib.Path(cls.temp.name)
        cls.fixture, cls.prepared_home = cls.root / "repo", cls.root / "prepared-home"
        make_fixture(cls.fixture)
        cls.linux_pgr = cls.root / "pgr-linux"
        cls.linux_pgr.write_bytes(b"\x7fELF" + b"stand-in for the Linux build")
        common.Pgr(stand_in(), home=cls.prepared_home).run("--root", cls.fixture, "refresh")
        quiet = io.StringIO()
        with contextlib.redirect_stdout(quiet):
            made = prepare_packs.main(["--pgr", stand_in(), "--fixture", str(cls.fixture), "--home", str(cls.prepared_home), "--out", str(cls.root / "packs-base.json")])
        assert made == 0, quiet.getvalue()

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def setUp(self):
        self.work = pathlib.Path(tempfile.mkdtemp(dir=self.root))
        self.home, self.packs, self.vm, self.words_dir = self.work / "home", self.work / "packs.json", self.work / "vm", self.work / "repo-words"
        shutil.copytree(self.prepared_home, self.home)
        shutil.copyfile(self.root / "packs-base.json", self.packs)
        self.vm.mkdir()
        patch = mock.patch.dict(os.environ, {"FAKE_WSL_ROOT": str(self.vm)})
        patch.start()
        self.addCleanup(patch.stop)

    def leaks(self, text: str) -> list[str]:
        spellings = {str(self.root), str(self.root).replace("\\", "/"), json.dumps(str(self.root))[1:-1]}
        return [spelling for spelling in spellings if spelling in text]

    def run_main(self, module, argv):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = module.main(argv)
        return code, out.getvalue(), err.getvalue()

    def words_argv(self, *extra):
        return ["--pgr", stand_in(), "--fixture", str(self.fixture), "--words", str(self.words_dir), "--home", str(self.home), "--packs", str(self.packs), *extra]

    def wsl_argv(self, *extra):
        return ["--pgr", stand_in(), "--fixture", str(self.fixture), "--linux-pgr", str(self.linux_pgr), "--home", str(self.home), "--packs", str(self.packs),
                "--out", str(self.work / "wsl.json"), "--wsl", f'"{sys.executable}" -I "{FAKE_WSL}"', *extra]

    # ---- prepare_words.py

    def test_words_packs_are_added_beside_the_others(self):
        before = json.loads(self.packs.read_text(encoding="utf-8"))
        code, out, err = self.run_main(prepare_words, self.words_argv())
        self.assertEqual(code, 0, err)
        after = json.loads(self.packs.read_text(encoding="utf-8"))
        for variant in common.VARIANTS:
            self.assertEqual(after["packs"][variant], before["packs"][variant], f"variant {variant} was touched")
        self.assertEqual(common.check_packs(after, common.workflow_questions(), ("n",)), [])
        self.assertEqual({letter: sorted(after["packs"]["n"][letter]) for letter in after["packs"]["n"]}, {"A": list(common.KEYS), "B": list(common.KEYS)})
        self.assertIn("words only", after["words"]["status"])
        self.assertEqual(after["words"]["fixture"], common.WORDS_PLACEHOLDER)
        self.assertEqual(after["words"]["identical_to_gs"], {"packs": 0, "of": 6})
        pack = after["packs"]["n"]["A"]["q1"]
        self.assertTrue(pack["command"].startswith("$env:PGR_NO_MODEL='1'; pgr --root C:\\path\\to\\lxbench\\repo-words --engines graph,search pack "), pack["command"])
        self.assertNotEqual(pack["output"], after["packs"]["gs"]["A"]["q1"]["output"])
        self.assertEqual(self.leaks(self.packs.read_text(encoding="utf-8")), [])
        self.assertNotIn("same text as the pack with the model: TPN", out)

    def test_words_refuse_a_pgr_that_ignores_the_variable(self):
        before = self.packs.read_bytes()
        with mock.patch.dict(os.environ, {"FAKE_PGR_IGNORE_NO_MODEL": "1"}):
            code, _, err = self.run_main(prepare_words, self.words_argv())
        self.assertEqual(code, 1)
        self.assertIn("ignores PGR_NO_MODEL", err)
        self.assertEqual(self.packs.read_bytes(), before, "nothing is written when the check fails")

    def test_words_need_the_packs_made_first_by_the_same_pgr(self):
        self.packs.unlink()
        code, _, err = self.run_main(prepare_words, self.words_argv())
        self.assertEqual(code, 1)
        self.assertIn("prepare_packs.py first", err)
        shutil.copyfile(self.root / "packs-base.json", self.packs)
        with mock.patch.dict(os.environ, {"FAKE_PGR_VERSION": "0.2.1"}):
            code, _, err = self.run_main(prepare_words, self.words_argv())
        self.assertEqual(code, 1)
        self.assertIn("run prepare_packs.py with this pgr first", err)

    def test_words_a_second_run_needs_replace_and_gives_the_same_file(self):
        self.assertEqual(self.run_main(prepare_words, self.words_argv())[0], 0)
        first = self.packs.read_bytes()
        code, _, err = self.run_main(prepare_words, self.words_argv())
        self.assertEqual(code, 1)
        self.assertIn("--replace", err)
        self.assertEqual(self.run_main(prepare_words, self.words_argv("--replace"))[0], 0)
        self.assertEqual(self.packs.read_bytes(), first)

    # ---- prepare_wsl.py

    def test_wsl_prepares_the_vm_and_records_it_without_paths(self):
        code, out, err = self.run_main(prepare_wsl, self.wsl_argv())
        self.assertEqual(code, 0, err)
        text = (self.work / "wsl.json").read_text(encoding="utf-8")
        doc = json.loads(text)
        self.assertEqual(doc["layout"], {"distro": "Ubuntu", "repo": "/home/USER/lxbench/repo", "pgr": "/home/USER/lxbench/bin/pgr"})
        self.assertEqual(doc["commands"]["pgr"], "wsl -d Ubuntu --cd /home/USER/lxbench/repo --exec /home/USER/lxbench/bin/pgr")
        self.assertNotIn("fakeuser", text)
        self.assertEqual(self.leaks(text), [])
        self.assertEqual(doc["pgr_sha256"], common.sha256_bytes(self.linux_pgr.read_bytes()))
        self.assertEqual((doc["packs"]["checked"], doc["packs"]["identical"], doc["status_matches_windows"]), (6, True, True))
        self.assertNotRegex(doc["status"], r"\d+[smhd] old")
        base = json.loads(self.packs.read_text(encoding="utf-8"))["packs"]["gs"]
        self.assertEqual(doc["packs"]["sha256"]["gs/B/q2"], base["B"]["q2"]["sha256"], "the pack it checked is the pack in packs.json")
        lxbench = self.vm / "home" / "fakeuser" / "lxbench"
        self.assertTrue((lxbench / "repo" / "security.py").is_file() and (lxbench / "repo" / ".git").is_dir() and (lxbench / "bin" / "pgr").is_file())
        self.assertIn("--wsl-distro Ubuntu", out)

    def test_wsl_is_safe_to_run_twice(self):
        self.assertEqual(self.run_main(prepare_wsl, self.wsl_argv())[0], 0)
        first = (self.work / "wsl.json").read_bytes()
        repo = self.vm / "home" / "fakeuser" / "lxbench" / "repo"
        (repo / "stray.py").write_text("x = 1\n", encoding="utf-8")
        (repo / "security.py").write_text("changed\n", encoding="utf-8")
        self.assertEqual(self.run_main(prepare_wsl, self.wsl_argv())[0], 0)
        self.assertEqual((self.work / "wsl.json").read_bytes(), first)
        self.assertTrue((repo.parent / "repo.round6-wsl").is_file(), "the marker that says the copy is this script's own")
        self.assertFalse((repo / "stray.py").exists(), "the earlier copy is replaced, not added to")
        self.assertEqual((repo / "security.py").read_bytes(), (self.fixture / "security.py").read_bytes())

    def test_wsl_leaves_a_folder_it_did_not_make_alone(self):
        home = self.vm / "home" / "fakeuser"
        for given in (None, "/home/fakeuser/projects"):
            repo = (home / "projects" if given else home / "lxbench") / "repo"
            repo.mkdir(parents=True)
            (repo / "mine.txt").write_text("not from the fixture\n", encoding="utf-8")
            code, _, err = self.run_main(prepare_wsl, self.wsl_argv(*(["--vm-dir", given] if given else [])))
            self.assertEqual(code, 1, given)
            self.assertIn("was not made by this script", err)
            self.assertEqual((repo / "mine.txt").read_text(encoding="utf-8"), "not from the fixture\n", "nothing in the folder is removed")
            self.assertEqual(sorted(child.name for child in repo.parent.iterdir()), ["repo"], "nothing is installed beside it either")
            self.assertFalse((self.work / "wsl.json").exists())
        # the folder this script made is replaced on the next run, also when the first run was cut off after the marker
        repo = home / "lxbench" / "repo"
        shutil.rmtree(repo)
        repo.mkdir(parents=True)
        (repo / "half.txt").write_text("cut off\n", encoding="utf-8")
        (repo.parent / "repo.round6-wsl").write_text("", encoding="utf-8")
        self.assertEqual(self.run_main(prepare_wsl, self.wsl_argv())[0], 0)
        self.assertFalse((repo / "half.txt").exists())
        self.assertTrue((repo / "security.py").is_file())

    def test_wsl_stops_and_writes_nothing_when_a_pack_differs(self):
        doc = json.loads(self.packs.read_text(encoding="utf-8"))
        pack = doc["packs"]["gs"]["A"]["q2"]
        pack["output"] += "\n== extra"
        pack["tokens"], pack["sha256"] = common.est_tokens(pack["output"]), common.sha256(pack["output"])
        self.packs.write_text(json.dumps(doc), encoding="utf-8")
        code, _, err = self.run_main(prepare_wsl, self.wsl_argv())
        self.assertEqual(code, 1)
        self.assertIn("gs/A/q2", err)
        self.assertIn("not the Windows packs", err)
        self.assertIn("Windows", err)
        self.assertFalse((self.work / "wsl.json").exists())

    def test_wsl_stops_when_the_vm_does_not_find_the_embedding_model(self):
        with mock.patch.dict(os.environ, {"PGR_NO_MODEL": "1"}):     # the VM sees it, the pgr for Windows is run without it
            code, _, err = self.run_main(prepare_wsl, self.wsl_argv())
        self.assertEqual(code, 1)
        self.assertIn("words only", err)
        self.assertIn("embedding model", err)
        self.assertFalse((self.work / "wsl.json").exists())

    def test_wsl_refuses_what_is_not_the_right_build_or_place(self):
        self.linux_pgr_copy = self.work / "not-linux"
        self.linux_pgr_copy.write_bytes(b"MZ not a linux program")
        code, _, err = self.run_main(prepare_wsl, self.wsl_argv("--linux-pgr", str(self.linux_pgr_copy)))
        self.assertEqual((code, "ELF" in err), (1, True))
        with mock.patch.dict(os.environ, {"FAKE_WSL_PGR_VERSION": "0.2.9"}):
            code, _, err = self.run_main(prepare_wsl, self.wsl_argv())
        self.assertEqual((code, "have to be one version" in err), (1, True))
        code, _, err = self.run_main(prepare_wsl, self.wsl_argv("--vm-dir", "/tmp/lxbench"))
        self.assertEqual((code, "inside the home folder" in err), (1, True))
        code, _, err = self.run_main(prepare_wsl, self.wsl_argv("--distro", "Other"))
        self.assertEqual((code, "no distribution" in err), (1, True), "wsl.exe's own message arrives in UTF-16")
        self.packs.unlink()
        code, _, err = self.run_main(prepare_wsl, self.wsl_argv())
        self.assertEqual((code, "prepare_packs.py first" in err), (1, True))
        self.assertFalse((self.work / "wsl.json").exists())


class MoreArmsArgs(unittest.TestCase):
    """The halves c and d of round6_args.py: the arguments of the second batch and what it asks of the VM and of the words-only copy."""

    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = pathlib.Path(cls.temp.name)
        cls.fixture, cls.home, cls.vm = cls.root / "repo", cls.root / "home", cls.root / "vm"
        cls.words_dir, cls.packs, cls.wsl_json = cls.root / "repo-words", cls.root / "packs.json", cls.root / "wsl.json"
        make_fixture(cls.fixture)
        cls.vm.mkdir()
        linux_pgr = cls.root / "pgr-linux"
        linux_pgr.write_bytes(b"\x7fELF" + b"stand-in for the Linux build")
        common.Pgr(stand_in(), home=cls.home).run("--root", cls.fixture, "refresh")
        wsl = f'"{sys.executable}" -I "{FAKE_WSL}"'
        quiet = io.StringIO()
        with mock.patch.dict(os.environ, {"FAKE_WSL_ROOT": str(cls.vm)}), contextlib.redirect_stdout(quiet), contextlib.redirect_stderr(quiet):
            steps = (prepare_packs.main(["--pgr", stand_in(), "--fixture", str(cls.fixture), "--home", str(cls.home), "--out", str(cls.packs)]),
                     prepare_words.main(["--pgr", stand_in(), "--fixture", str(cls.fixture), "--words", str(cls.words_dir), "--home", str(cls.home), "--packs", str(cls.packs)]),
                     prepare_wsl.main(["--pgr", stand_in(), "--fixture", str(cls.fixture), "--linux-pgr", str(linux_pgr), "--home", str(cls.home),
                                       "--packs", str(cls.packs), "--out", str(cls.wsl_json), "--wsl", wsl]))
        assert steps == (0, 0, 0), quiet.getvalue()
        cls.wsl = wsl

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def setUp(self):
        self.work = pathlib.Path(tempfile.mkdtemp(dir=self.root))
        self.my_home, self.my_vm = self.work / "home", self.work / "vm"
        shutil.copytree(self.home, self.my_home)
        shutil.copytree(self.vm, self.my_vm)
        patch = mock.patch.dict(os.environ, {"FAKE_WSL_ROOT": str(self.my_vm)})
        patch.start()
        self.addCleanup(patch.stop)

    def argv(self, half, tag="t1", **change):
        given = {"--half": half, "--run-tag": tag, "--repo": str(self.fixture), "--repo-words": str(self.words_dir), "--packs": str(self.packs),
                 "--team-notes": str(self.root / "none.json"), "--wsl-notes": str(self.wsl_json), "--wsl-distro": "Ubuntu", "--wsl-program": self.wsl,
                 "--pgr": stand_in(), "--home": str(self.my_home), "--out": str(self.work / f"args-{half}.json")}
        given.update({"--" + key.replace("_", "-"): value for key, value in change.items()})
        return [item for pair in given.items() if pair[1] is not None for item in pair]

    def refused(self, argv) -> str:
        said = io.StringIO()
        with contextlib.redirect_stderr(said):
            self.assertEqual(round6_args.main(argv), 1)
        return said.getvalue()

    def test_args_for_the_halves_c_and_d_start_in_the_workflow(self):
        want = {"c": ["gs", "n"], "d": ["gs"]}
        for half in "cd":
            out = self.work / f"args-{half}.json"
            said = io.StringIO()
            with contextlib.redirect_stdout(said):
                self.assertEqual(round6_args.main(self.argv(half)), 0)
            args = json.loads(out.read_text(encoding="utf-8"))
            self.assertEqual(args["wsl"], {"distro": "Ubuntu", "repo": "/home/fakeuser/lxbench/repo", "pgr": "/home/fakeuser/lxbench/bin/pgr"})
            self.assertEqual(sorted(args["packs"]), want[half])
            self.assertEqual(("repoWords" in args, "loadouts" in args, "repoTeam" in args), (half == "c", False, False))
            expected = {f"packs/{variant}/{letter}/{key}": common.sha256(args["packs"][variant][letter][key]["output"])
                        for variant in args["packs"] for letter in common.SETS for key in common.KEYS}
            self.assertEqual(args["sums"], expected)
            self.assertEqual(args["matrix"], json.loads((HERE / f"round6-{half}.json").read_text(encoding="utf-8")))
            if NODE:
                done = run_node("--args", out)
                self.assertEqual(done.returncode, 0, done.stdout)
                self.assertIn(f"{common.count_matrix(args['matrix'])['agents']} agents", done.stdout)

    def test_the_words_only_copy_must_exist_and_the_pgr_must_know_the_variable(self):
        self.assertIn("--repo-words", self.refused(self.argv("c", repo_words=None)))
        self.assertIn("no index yet", self.refused(self.argv("c", repo_words=str(self.work / "nothing"))))
        with mock.patch.dict(os.environ, {"FAKE_PGR_IGNORE_NO_MODEL": "1"}):     # a pgr that ignores the variable shows its model
            self.assertIn("words only", self.refused(self.argv("c", repo_words=str(self.fixture))))
        # a copy built with the model, whatever the pgr knows: its status line says embeddings even when the call sets the variable
        self.assertIn("words only", self.refused(self.argv("c", repo_words=str(self.fixture))))
        self.assertIn("contains a space", self.refused(self.argv("c", repo_words="C:\\a path\\repo-words")))
        # half d has no words-only arm: it needs neither the copy nor the packs of variant n
        self.assertEqual(round6_args.main(self.argv("d", repo_words=None)), 0)

    def test_the_vm_must_be_the_one_prepare_wsl_made(self):
        lxbench = self.my_vm / "home" / "fakeuser" / "lxbench"
        self.assertIn("pass --wsl-distro", self.refused(self.argv("c", wsl_distro=None, wsl_notes=str(self.root / "none.json"))))
        self.assertIn("run prepare_wsl.py first", self.refused(self.argv("c", wsl_notes=str(self.root / "none.json"))))
        self.assertIn("no distribution", self.refused(self.argv("c", wsl_distro="Other")))
        self.assertIn("is not in the VM", self.refused(self.argv("c", wsl_dir="/home/fakeuser/elsewhere")))
        with mock.patch.dict(os.environ, {"FAKE_WSL_PGR_VERSION": "0.2.7"}):
            self.assertIn("the pgr in the VM says", self.refused(self.argv("c")))
        changed = json.loads(self.wsl_json.read_text(encoding="utf-8"))
        changed["pgr_version"] = "pgr 0.2.9"
        other = self.work / "other-wsl.json"
        other.write_text(json.dumps(changed), encoding="utf-8")
        self.assertIn("run prepare_wsl.py again", self.refused(self.argv("c", wsl_notes=str(other))))
        for index in (self.my_vm / "home" / "fakeuser" / ".local-index" / "rust" / "index").iterdir():
            index.unlink()
        self.assertIn("run prepare_wsl.py again", self.refused(self.argv("c")))
        (lxbench / "bin" / "pgr").write_bytes(b"\x7fELF another build")
        self.assertIn("not the build that prepare_wsl.py installed", self.refused(self.argv("c")))
        (lxbench / "bin" / "pgr").unlink()
        self.assertIn("is not in the VM", self.refused(self.argv("d")))

    def test_a_run_tag_whose_banks_exist_in_the_vm_is_refused_for_the_hub_arms_only(self):
        banks = self.my_vm / "home" / "fakeuser" / ".local-index" / "rust" / "banks"
        (banks / "r6-t1-thv-orch-c-r1").mkdir(parents=True)
        message = self.refused(self.argv("d"))
        self.assertIn("used already", message)
        self.assertIn("in the VM", message)
        self.assertEqual(round6_args.main(self.argv("d", tag="t2")), 0)
        self.assertEqual(round6_args.main(self.argv("c")), 0, "half c has no hub arm")

    def test_the_counts_are_printed_for_the_four_files(self):
        said = io.StringIO()
        with contextlib.redirect_stdout(said):
            self.assertEqual(round6_args.main(["--count"]), 0)
        lines = said.getvalue().strip().split("\n")
        self.assertEqual([line.split(":")[0] for line in lines], [f"round6-{half}.json" for half in "abcd"])
        self.assertIn("57 cells, 81 agents (light 69, middle 12, top 0)", lines[0])
        self.assertIn("24 cells, 120 agents (light 72, middle 0, top 48)", lines[1])


if __name__ == "__main__":
    unittest.main(verbosity=2)
