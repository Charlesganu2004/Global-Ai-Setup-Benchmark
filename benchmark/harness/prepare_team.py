"""Prepare set T, the demonstration set of round six, on a copy of the fixture.

    prepare_team.py --pgr PATH --fixture DIR --team DIR [--home DIR] [--bank NAME] [--out FILE] [--replace]
    prepare_team.py --verify --out FILE [--home DIR]

Set T was written by the builders to show one feature of the hub: a question that needs one lookup and
one note that an earlier agent left behind. This script

  1. copies the fixture to --team (it never touches the fixture) and builds the copy's index with pgr,
  2. files about 30 notes in a bank with `pgr --bank B --task T --from R handoff --whole TEXT`:
     three tasks that the questions are about, five other tasks as distractors, three roles; the
     distractors are about real functions of the fixture and include one near-miss for each question,
  3. makes ONE note stale by changing the body of the function it names, validate_url, without adding
     or removing a line, so that no line number of the truth moves, then refreshes the index,
  4. checks what pgr says about all of it, and writes benchmark/data/team_notes.json: every note, the
     edit, the output of `pgr --bank B --task T loadout` for every task (what TI puts in its prompt,
     through args.loadouts) and a hash of every file of the bank.

The three questions (their text is in token-benchmark.workflow.js as Q_T):

  q1  a decision: BLD-4172 says dispatch_command must keep its parameter names
  q2  a stale note: a reviewer described validate_url, then the function's body changed
  q3  a confirmed fact: the scout and the builder filed the same sentence about _register_merge_driver

The lookup part of each question uses a function whose truth sets A and B already hold. bench_parse.py
grades the note fields on an exact match; test_round6.py checks that the notes below and TRUTH_T agree.

--verify recomputes the hash of the bank and says which files changed, because the team bank is read by
every cell of TL and TI and must be the same when the round ends. Run it before the round as well as
after it: round6_args.py does, for a half with set T arms, and refuses to write the args when it fails.

The bank lives in the index home (LXR_HOME, else ~/.local-index/rust) and this script refuses to file into
a bank that exists. --replace builds the team copy again but does not remove the bank: delete the folder
banks/<bank> of the index home yourself first. Run it with the same pgr that the agents will run, in the
same index home, because the bank and the index of the copy exist only there.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import shutil
import stat
import sys

sys.path.append(str(pathlib.Path(__file__).resolve().parent))  # python -I leaves the script's folder off the path; the standard library stays first
import round6_common as common
from round6_common import RoundSixError

BANK = "team-notes"
# The task of each question.
TASKS = {"q1": "cli-freeze", "q2": "ssrf-review", "q3": "merge-driver-audit"}
ROLES = ("scout", "builder", "reviewer")

# The one edit: the line is the call that lowercases the hostname, in the body of validate_url. The new
# line has the same number of lines around it, so def lines and caller lines of the truth stay where they are.
EDIT = {
    "function": "validate_url",
    "file": "security.py",
    "line": 121,
    "old": "        if hostname.lower() in _BLOCKED_HOSTS:",
    "new": "        if hostname in _BLOCKED_HOSTS:",
}

MERGE_DRIVER = "_register_merge_driver writes the graph.json union merge driver into git config and .gitattributes."
STATUS = "_merge_driver_status reports whether the merge driver is registered, by reading both git config and .gitattributes."

# (task, role, text), in the order they are filed. A text that appears twice is one note seen twice,
# filed by two roles: that is how a fact becomes confirmed.
NOTES = [
    # q1: a decision with a ticket
    ("cli-freeze", "builder", "Decision BLD-4172: dispatch_command must keep its parameter names, so the parameter cmd may not be renamed until the freeze is lifted."),
    ("cli-freeze", "scout", "_run_cli reads the command line and then calls dispatch_command with the subcommand."),
    ("cli-freeze", "reviewer", "The freeze lists parameter names only, not behaviour, so the body of dispatch_command can still be refactored."),
    ("cli-freeze", "scout", "dispatch_command is long, so changes to it are reviewed in a separate pass."),
    # q2: a reviewer's note that goes stale
    ("ssrf-review", "reviewer", "validate_url lowercases the hostname before it looks it up in the blocklist of cloud metadata hosts, so mixed-case spellings are blocked too."),
    ("ssrf-review", "scout", "_ip_is_blocked treats carrier-grade NAT addresses as internal, in addition to the private, reserved, loopback and link-local ranges."),
    ("ssrf-review", "builder", "safe_fetch_text only decodes the bytes that safe_fetch returns, replacing bad UTF-8, so it adds no new network path."),
    ("ssrf-review", "scout", "_resolve_and_validate resolves a host once and returns the validated address, so a second DNS answer cannot swap in a private address."),
    # q3: the same fact filed by two roles
    ("merge-driver-audit", "scout", MERGE_DRIVER),
    ("merge-driver-audit", "builder", MERGE_DRIVER),
    ("merge-driver-audit", "reviewer", "_unregister_merge_driver removes the merge-driver keys from git config and the line from .gitattributes."),
    ("merge-driver-audit", "builder", "_pinned_python returns an empty string when sys.executable is not shell-safe, and the merge driver command then falls back to the plain launcher name."),
    # distractors; the first note of this task is the near-miss of q1 (another ticket, a similarly named
    # function) and the second and third, filed by two other roles, are the near-miss of q3
    ("hook-installers", "builder", "BLD-4180 freezes dispatch_install_cli: its parameter cmd keeps its name and may not be renamed, whatever the installer refactor needs."),
    ("hook-installers", "scout", STATUS),
    ("hook-installers", "reviewer", STATUS),
    ("hook-installers", "reviewer", "_install_claude_hook adds the graphify PreToolUse hook to .claude/settings.json."),
    ("hook-installers", "scout", "The codex installer, _install_codex_hook, keeps its hook in .codex/hooks.json and not in a settings file."),
    ("ignore-rules", "scout", "_is_ignored decides whether a path is excluded under the .graphifyignore patterns."),
    ("ignore-rules", "reviewer", "_load_graphifyignore reads the .graphifyignore files and returns pairs of an anchor directory and a pattern."),
    ("ignore-rules", "builder", "_match_anchored_ignore_pattern keeps a star from crossing a slash when a pattern is anchored."),
    ("ignore-rules", "scout", "_parse_gitignore_line parses one raw line of a .graphifyignore file following the gitignore specification."),
    ("atomic-writes", "builder", "write_json_atomic streams the encoded JSON into the temporary file instead of building one large string first."),
    ("atomic-writes", "scout", "write_text_atomic writes UTF-8 text and delegates the swap to _atomic_replace."),
    ("atomic-writes", "reviewer", "_atomic_replace takes a write function, fills a temporary file with it and moves the finished file into place, so a reader never sees half a file."),
    # the near-miss of q2: a reviewer's note about a similarly named function, still current
    ("path-guards", "reviewer", "validate_graph_path resolves the path and verifies that it stays inside the base folder."),
    ("path-guards", "scout", "validate_extraction checks an extraction JSON dict against the graphify schema."),
    ("path-guards", "builder", "validate_semantic_fragment returns the validation errors for an untrusted semantic extraction fragment."),
    ("path-guards", "scout", "load_validated_semantic_fragment rejects oversize files before it parses them."),
    ("llm-keys", "scout", "_get_backend_api_key returns the first configured API key of a backend, or an empty string."),
    ("llm-keys", "builder", "detect_backend returns the name of the backend that has an API key set, or None."),
    ("llm-keys", "reviewer", "_validate_ollama_base_url warns about an unsafe OLLAMA_BASE_URL and hard-blocks link-local and metadata addresses."),
]

ABOUT = ("Set T, the demonstration set of round six. The builders wrote it to show one feature of the hub: each question "
         "needs one lookup and one note that an earlier agent filed. Made by prepare_team.py on a copy of the fixture.")
QUESTION_KINDS = {"q1": "a decision with a ticket", "q2": "a note that has gone stale", "q3": "a fact confirmed by two roles"}
NAMING = re.compile(r"\b[A-Za-z]+_[A-Za-z0-9_]+\b|\b_[A-Za-z0-9_]+\b")


def tasks_in_order() -> list[str]:
    return list(dict.fromkeys(task for task, _, _ in NOTES))


def code_names(text: str) -> list[str]:
    """The function names a note text mentions, as a person would spot them: a word with an underscore."""
    return [name for name in dict.fromkeys(NAMING.findall(text)) if name != name.upper()]


def distinct_notes() -> list[dict]:
    """The notes as the bank will hold them: one entry per text, with every role that filed it."""
    notes: dict[str, dict] = {}
    for task, role, text in NOTES:
        entry = notes.setdefault(text, {"task": task, "roles": [], "text": text})
        if task != entry["task"]:
            raise RoundSixError(f"one text is filed under two tasks: {text[:60]!r}")
        entry["roles"].append(role)
    return list(notes.values())


def validate_plan(questions: dict | None = None) -> None:
    """The plan agrees with itself, with the workflow's question texts and with the truth of set T."""
    import bench_parse

    for task in tasks_in_order():
        if not common.valid_name(task):
            raise RoundSixError(f"task name {task!r} breaks the name rule")
    for name in [BANK, *ROLES]:
        if not common.valid_name(name):
            raise RoundSixError(f"name {name!r} breaks the name rule")
    if sorted({role for _, role, _ in NOTES}) != sorted(ROLES):
        raise RoundSixError("the notes must use all three roles")
    if len(tasks_in_order()) != 8:
        raise RoundSixError(f"{len(tasks_in_order())} tasks, expected 3 question tasks and 5 distractors")
    for _, _, text in NOTES:
        if not code_names(text):
            raise RoundSixError(f"a note names no function, so pgr cannot check it against the code: {text[:60]!r}")
        if re.search(r"\b\w+\.py\b|:\d+\b", text):
            raise RoundSixError(f"a note gives a file or a line, which would answer the lookup part of a question: {text[:60]!r}")
    truth = bench_parse.TRUTH_T
    by_task = {task: [(role, text) for t, role, text in NOTES if t == task] for task in tasks_in_order()}
    # q1: one decision, with the ticket of the truth, saying the parameters may not be renamed
    decision = [text for _, text in by_task[TASKS["q1"]] if truth["q1"]["ticket"] in text]
    if len(decision) != 1 or "may not be renamed" not in decision[0] or truth["q1"]["may_rename"] is not False:
        raise RoundSixError("q1: the decision note and TRUTH_T disagree")
    others = [text for t, _, text in NOTES if t != TASKS["q1"] and truth["q1"]["ticket"] in text]
    if others:
        raise RoundSixError("q1: the ticket of the truth is cited outside its task")
    # q2: one note names the function that is edited, and the truth names the role that filed it
    named = [(role, text) for role, text in by_task[TASKS["q2"]] if EDIT["function"] in code_names(text)]
    if [role for role, _ in named] != [truth["q2"]["filed_by"]] or truth["q2"]["status"] != "changed":
        raise RoundSixError("q2: the note that the edit makes stale and TRUTH_T disagree")
    if sum(EDIT["function"] in code_names(text) for _, _, text in NOTES) != 1:
        raise RoundSixError("q2: more than one note names the edited function, so more than one would go stale")
    # q3: one text filed by exactly the two roles of the truth
    confirmed = [note for note in distinct_notes() if note["task"] == TASKS["q3"] and len(note["roles"]) > 1]
    if len(confirmed) != 1 or sorted(confirmed[0]["roles"]) != truth["q3"]["roles"] or truth["q3"]["function"] not in code_names(confirmed[0]["text"]):
        raise RoundSixError("q3: the confirmed note and TRUTH_T disagree")
    if len([note for note in distinct_notes() if len(note["roles"]) > 1]) != 2:
        raise RoundSixError("exactly two facts should be confirmed: the one of q3 and its near-miss")
    if questions is not None:
        for key, task in TASKS.items():
            if f"`{task}`" not in questions["T"][key]:
                raise RoundSixError(f"the text of {key} in the workflow does not name its task `{task}`")


def writable(function, path, _):
    os.chmod(path, stat.S_IWRITE)
    function(path)


def copy_fixture(fixture: pathlib.Path, team: pathlib.Path, replace: bool) -> None:
    """Copy the fixture to the team folder. An existing team folder is removed only with --replace and only
    when this script made it (it leaves a marker file beside it)."""
    fixture, team = fixture.resolve(), team.resolve()
    if not fixture.is_dir():
        raise RoundSixError(f"the fixture {fixture} is not a folder")
    if team == fixture or fixture in team.parents or team in fixture.parents:
        raise RoundSixError("the team folder and the fixture must be two separate folders")
    marker = team.with_name(team.name + ".round6-team")
    if team.exists():
        if not replace:
            raise RoundSixError(f"{team} exists; pass --replace to build it again")
        if not marker.exists():
            raise RoundSixError(f"{team} exists and was not made by this script; remove it yourself")
        shutil.rmtree(team, onerror=writable)
    # Index data of other tools stays behind; runs-hub holds index homes that a test of pgr once left inside the fixture.
    shutil.copytree(fixture, team, ignore=shutil.ignore_patterns("__pycache__", ".codanna", ".token-goat", "graphify-out", "runs-hub"))
    marker.write_text("made by prepare_team.py\n", encoding="utf-8")


def apply_edit(team: pathlib.Path, edit: dict = EDIT) -> dict:
    """Change one line of the team copy. The line must be exactly the old text and the file keeps its
    line count and its line ends; the def line of the function stays where it was."""
    path = team / edit["file"]
    raw = path.read_bytes()
    lines = raw.split(b"\n")
    at = edit["line"] - 1
    have = lines[at].rstrip(b"\r").decode("utf-8")
    if have != edit["old"]:
        raise RoundSixError(f"{edit['file']} line {edit['line']} is {have!r}, expected {edit['old']!r}: the fixture is not the one set T was written for")
    ending = b"\r" if lines[at].endswith(b"\r") else b""
    lines[at] = edit["new"].encode("utf-8") + ending
    changed = b"\n".join(lines)
    if changed.count(b"\n") != raw.count(b"\n"):
        raise RoundSixError("the edit changed the number of lines")
    path.write_bytes(changed)
    return {**edit, "lines_before": raw.count(b"\n") + 1, "lines_after": changed.count(b"\n") + 1}


def bank_dir(home: pathlib.Path, bank: str) -> pathlib.Path:
    return home / "banks" / bank


def bank_files(home: pathlib.Path, bank: str) -> dict[str, str]:
    """relative path -> SHA-256 of every file of the bank."""
    root = bank_dir(home, bank)
    return {path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(root.rglob("*")) if path.is_file()}


def file_note(pgr: common.Pgr, team: pathlib.Path, bank: str, task: str, role: str, text: str, expected: str) -> str:
    out, _, _ = pgr.run("--root", team, "--bank", bank, "--task", task, "--from", role, "handoff", "--whole", text)
    lines = out.strip().split("\n")
    first = lines[0].split(" ")[0] if lines else ""
    closing = re.fullmatch(r"handoff (\S+): (\d+) notes from (\S+), (\d+) name code", lines[-1]) if lines else None
    if first != expected or not closing:
        raise RoundSixError(f"handoff of {text[:50]!r} printed {out.strip()!r}; expected '{expected} ...' and a closing handoff line")
    if closing.group(1) != task or closing.group(2) != "1" or closing.group(3) != role:
        raise RoundSixError(f"handoff of {text[:50]!r} closed with {lines[-1]!r}")
    if expected == "kept" and int(closing.group(4)) < 1:
        raise RoundSixError(f"pgr found no function in the note {text[:50]!r}; it cannot check the note against the code")
    return first


def check_loadouts(loadouts: dict[str, str]) -> None:
    """What pgr must say about the notes once the edit is made."""
    for task, output in loadouts.items():
        stale = [line for line in output.split("\n") if "stale: code changed" in line]
        gone = [line for line in output.split("\n") if "gone: code removed" in line]
        if gone:
            raise RoundSixError(f"loadout of {task} holds a gone note: {gone[0][:80]!r}")
        if task == TASKS["q2"]:
            if len(stale) != 1 or EDIT["function"] not in stale[0]:
                raise RoundSixError(f"loadout of {task} must hold exactly one stale note, the one about {EDIT['function']}: {stale!r}")
        elif stale:
            raise RoundSixError(f"loadout of {task} holds a stale note, but only the note about {EDIT['function']} should be: {stale[0][:80]!r}")
        if re.search(r"\+\d+ more", output):
            raise RoundSixError(f"loadout of {task} was cut by its budget")
        for note in distinct_notes():
            shown = output.count(note["text"][:50])
            # The core layer holds every confirmed note, whatever its task; the task layer holds the rest of its task.
            if note["task"] == task or len(note["roles"]) > 1:
                if shown != 1:
                    raise RoundSixError(f"loadout of {task} shows the note {note['text'][:50]!r} {shown} times, expected once")
            elif shown:
                raise RoundSixError(f"loadout of {task} shows a note of task {note['task']}: {note['text'][:50]!r}")
    confirmed = [line for line in loadouts[TASKS["q3"]].split("\n") if MERGE_DRIVER[:50] in line]
    if len(confirmed) != 1 or "[builder+scout]" not in confirmed[0]:
        raise RoundSixError(f"loadout of {TASKS['q3']} must show the confirmed note with [builder+scout]: {confirmed!r}")


def prepare(pgr: common.Pgr, fixture: pathlib.Path, team: pathlib.Path, questions: dict, bank: str = BANK,
            home: pathlib.Path | None = None, replace: bool = False) -> dict:
    """Build the team copy, file the notes, make the edit and return the team_notes.json document."""
    version = pgr.require()
    validate_plan(questions)
    if not common.valid_name(bank):
        raise RoundSixError(f"{bank!r} is not a valid bank name")
    home = pathlib.Path(home or pgr.env.get("LXR_HOME") or pathlib.Path.home() / ".local-index" / "rust")
    if bank_dir(home, bank).exists():
        raise RoundSixError(f"the bank {bank_dir(home, bank)} exists already; choose another --bank or remove the folder")
    copy_fixture(fixture, team, replace)
    pgr.run("--root", team, "refresh")
    status = pgr.run("--root", team, "status")[0].strip()
    if "graph ready" not in status:
        raise RoundSixError(f"the team copy has no usable index: {status!r}")
    seen: set[str] = set()
    outcomes = []
    for task, role, text in NOTES:
        expected = "reinforced" if text in seen else "kept"
        outcomes.append(file_note(pgr, team, bank, task, role, text, expected))
        seen.add(text)
    edit = apply_edit(team)
    pgr.run("--root", team, "refresh")
    spellings = common.private_paths(fixture, team, home)
    loadouts = {}
    for task in tasks_in_order():
        argv = ["pgr", "--root", common.TEAM_PLACEHOLDER, "--bank", bank, "--task", task, "loadout"]
        output = pgr.run("--root", team, "--bank", bank, "--task", task, "loadout")[0].rstrip("\n")
        common.refuse_private(output, spellings, f"the loadout of {task}")
        loadouts[task] = {"command": common.render(argv), "output": output, "tokens": common.est_tokens(output), "sha256": common.sha256(output)}
    check_loadouts({task: entry["output"] for task, entry in loadouts.items()})
    return {
        "about": ABOUT,
        "pgr_version": version,
        "status": pgr.run("--root", team, "status")[0].strip(),
        "fixture": common.FIXTURE_PLACEHOLDER,
        "team_fixture": common.TEAM_PLACEHOLDER,
        "bank": bank,
        "roles": list(ROLES),
        "questions": {key: {"task": task, "kind": QUESTION_KINDS[key]} for key, task in TASKS.items()},
        "notes": [{"n": at + 1, "task": task, "role": role, "text": text, "outcome": outcomes[at]}
                  for at, (task, role, text) in enumerate(NOTES)],
        "edit": edit,
        "loadouts": loadouts,
        "bank_files": bank_files(home, bank),
    }


def verify(doc: dict, home: pathlib.Path) -> list[str]:
    """Differences between the bank on disk and the bank as it was prepared; empty when it is unchanged."""
    now = bank_files(home, doc["bank"])
    then = doc["bank_files"]
    return ([f"changed: {name}" for name in sorted(now) if name in then and now[name] != then[name]] +
            [f"added: {name}" for name in sorted(set(now) - set(then))] +
            [f"missing: {name}" for name in sorted(set(then) - set(now))])


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--pgr", help="the pgr 0.2 program, or a command line for a stand-in")
    parser.add_argument("--fixture", type=pathlib.Path, help="the fixture repository (never modified)")
    parser.add_argument("--team", type=pathlib.Path, help="where the team copy goes, for example ...\\lxbench\\repo-team")
    parser.add_argument("--home", help="LXR_HOME for every call (default: the one in the environment, else pgr's own)")
    parser.add_argument("--bank", default=BANK)
    parser.add_argument("--out", type=pathlib.Path, default=common.DATA / "team_notes.json")
    parser.add_argument("--replace", action="store_true", help="build the team copy again if this script made it before")
    parser.add_argument("--verify", action="store_true", help="compare the bank on disk with the bank in --out and exit")
    parser.add_argument("--workflow", type=pathlib.Path, default=common.WORKFLOW)
    args = parser.parse_args(argv)
    try:
        if args.verify:
            doc = json.loads(args.out.read_text(encoding="utf-8"))
            home = pathlib.Path(args.home or os.environ.get("LXR_HOME") or pathlib.Path.home() / ".local-index" / "rust")
            changes = verify(doc, home)
            for line in changes:
                print(line)
            print(f"the bank {doc['bank']} is {'unchanged' if not changes else 'NOT the bank that was prepared'}")
            return 1 if changes else 0
        if not (args.pgr and args.fixture and args.team):
            parser.error("--pgr, --fixture and --team are required")
        questions = common.workflow_questions(args.workflow)
        pgr = common.Pgr(args.pgr, home=args.home or None)
        doc = prepare(pgr, args.fixture, args.team, questions, args.bank, args.home, args.replace)
        common.write_json(args.out, doc, private=(args.fixture, args.team, args.home))
    except RoundSixError as error:
        print(f"prepare_team: {error}", file=sys.stderr)
        return 1
    print(f"{doc['pgr_version']}: {len(doc['notes'])} notes in {len(doc['loadouts'])} tasks, one note made stale -> {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
