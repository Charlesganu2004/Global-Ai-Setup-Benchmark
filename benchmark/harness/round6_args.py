"""Write the args of one half of round six.

    round6_args.py --half a|b|c|d --run-tag TAG --repo DIR [--repo-team DIR] [--repo-words DIR]
                   [--wsl-distro NAME] [--wsl-dir DIR] [--briefs FILE] [--out FILE] [--pgr PROGRAM] [--home DIR]
    round6_args.py --count

The workflow takes everything it cannot compute in its args: the fixture paths, the run tag that names the
hub banks, the packs and loadouts that prepare_packs.py, prepare_words.py and prepare_team.py computed (with
a SHA-256 of each, which the workflow checks again, because the args pass through the model that starts it),
the virtual machine (args.wsl, from prepare_wsl.py), and the cells (round6-a.json: one agent and three
subagents; round6-b.json: orchestrated; round6-c.json: the single-agent cells of the second batch, with the
tools in a virtual machine, without the neural network and with the pack only; round6-d.json: the
orchestrated ones). This reads the data files, checks them against the question texts in the workflow, keeps
only what the half needs, and writes one JSON file that is handed to the workflow as its args. --count
prints, for each of the four matrix files, the number of cells and of agents by model class, and nothing else.

Before it writes anything it also checks the machine, because the prompts tell the agents to run a bare
`pgr`: the pgr that the shell finds (--pgr, default `pgr`) must be 0.2 or newer, the same version as the one
that made the packs and the team notes, and must read the index of --repo (and --repo-team); no bank of the
run tag may exist yet; and for a half with set T arms the team bank must still be the bank that was prepared.
For the arms without the neural network it reads the index of --repo-words with PGR_NO_MODEL=1 and wants it
to say "words only". For the virtual machine arms it asks the VM (read-only calls through wsl.exe): the Linux
pgr must be there, executable, the build prepare_wsl.py installed, read the index of the copy and have no
bank of the run tag. --wsl-dir is the folder prepare_wsl.py used inside the VM (default: lxbench in the
home folder of the VM user). Run it in the shell that will start the client.

Neither path may contain a space: the prompts put it into a shell command unquoted. Use a new run tag
for every run and every rerun of a half: the tag names the banks of the hub arms, so that no run can
read the notes of another. The args hold the owner's folders, so they are never written inside the
repository: the default --out is in the temporary folder.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import shutil
import sys
import tempfile

sys.path.append(str(pathlib.Path(__file__).resolve().parent))  # python -I leaves the script's folder off the path; the standard library stays first
import bench_parse
import prepare_team
import round6_common as common
from round6_common import RoundSixError

# Which arms need what. token-benchmark.workflow.js holds the same maps; test_round6.py checks that the two agree.
PACK_OF = {"TP": "gs", "TPG": "g", "TPS": "s", "TPH": "gs", "TPHV": "gs", "TPN": "n", "TPX": "gs"}
HUB_ARMS = {"TH", "TPH", "THV", "TPHV"}
VM_ARMS = {"R6V", "THV", "TPHV"}
WORDS_ARMS = {"RN", "TPN"}
PACK_ONLY_ARMS = {"TPX"}
TEAM_LOADOUT_ARMS = {"TI"}
RUN_TAG_RULE = re.compile(r"[a-z0-9][a-z0-9._-]{0,23}")
MATRIX_HALVES = ("a", "b", "c", "d")


def needs(matrix: list[dict]) -> dict:
    """What the cells of a matrix need from the harness."""
    arms = {bench_parse.base_arm(arm) for block in matrix for arm in block["arms"]}
    return {
        "variants": sorted({PACK_OF[arm] for arm in arms if arm in PACK_OF}),
        "hub": bool(arms & HUB_ARMS),
        "hub_windows": bool(arms & (HUB_ARMS - VM_ARMS)),
        "hub_vm": bool(arms & HUB_ARMS & VM_ARMS),
        "team": bool(arms & bench_parse.SET_T_ARMS),
        "loadouts": bool(arms & TEAM_LOADOUT_ARMS),
        "vm": bool(arms & VM_ARMS),
        "words": bool(arms & WORDS_ARMS),
    }


def counts_text() -> list[str]:
    """One line per matrix file: its cells and its agents, by class of model."""
    lines = []
    for half in MATRIX_HALVES:
        found = common.count_matrix(json.loads((common.HERE / f"round6-{half}.json").read_text(encoding="utf-8")))
        classes = ", ".join(f"{name} {count}" for name, count in found["by_class"].items())
        lines.append(f"round6-{half}.json: {found['cells']} cells, {found['agents']} agents ({classes}); planners counted in them: "
                     f"{found['planners']}, of which args.briefs would replace {found['planners_saved_by_briefs']}")
    return lines


def build(half: str, run_tag: str, repo: str, repo_team: str | None, briefs: dict | None,
          packs_doc: dict | None, team_doc: dict | None, matrix: list[dict], questions: dict,
          repo_words: str | None = None, wsl: dict | None = None) -> dict:
    """The args object, or an error that says what is missing."""
    need = needs(matrix)
    for name, path in (("--repo", repo), ("--repo-team", repo_team), ("--repo-words", repo_words)):
        if path and " " in path:
            raise RoundSixError(f"{name} {path!r} contains a space; the prompts put the path into a shell command unquoted")
    if not RUN_TAG_RULE.fullmatch(run_tag or ""):
        raise RoundSixError("--run-tag must be 1 to 24 characters of a-z 0-9 . _ - and start with a letter or a digit")
    args: dict = {"repo": repo, "runTag": run_tag}
    if need["words"]:
        if not repo_words:
            raise RoundSixError("this half has arms without the neural network: run prepare_words.py and pass --repo-words")
        args["repoWords"] = repo_words
    if need["vm"]:
        if not wsl or not common.DISTRO_RULE.fullmatch(wsl.get("distro", "")) or not all(
                common.LINUX_PATH_RULE.fullmatch(wsl.get(key, "")) and ".." not in wsl.get(key, "") for key in ("repo", "pgr")):
            raise RoundSixError("this half has virtual machine arms: run prepare_wsl.py and pass --wsl-distro and --wsl-dir (a distribution name and "
                                "a folder inside the VM, letters, digits and . _ - / only, no space)")
        args["wsl"] = {key: wsl[key] for key in ("distro", "repo", "pgr")}
    # The SHA-256 of every pack and loadout text, by place. The args pass through the model that starts the
    # workflow, which types them out; the workflow computes the hashes again and stops on a difference.
    sums: dict = {}
    if briefs is not None:
        args["briefs"] = briefs
    if need["variants"]:
        if packs_doc is None:
            raise RoundSixError("this half has pack arms: run prepare_packs.py first (benchmark/data/packs.json)")
        problems = common.check_packs(packs_doc, questions, tuple(v for v in need["variants"] if v in common.WORDS_VARIANTS))
        if problems:
            hint = " (the words-only packs come from prepare_words.py)" if any(".n." in problem for problem in problems) else ""
            raise RoundSixError("packs.json does not check out: " + "; ".join(problems[:5]) + hint)
        args["packs"] = {variant: packs_doc["packs"][variant] for variant in need["variants"]}
        for variant in need["variants"]:
            for letter in common.SETS:
                for key in common.KEYS:
                    sums[f"packs/{variant}/{letter}/{key}"] = common.sha256(packs_doc["packs"][variant][letter][key]["output"])
    if need["team"]:
        if team_doc is None or not repo_team:
            raise RoundSixError("this half has set T arms: run prepare_team.py first (benchmark/data/team_notes.json) and pass --repo-team")
        args["repoTeam"] = repo_team
        args["teamBank"] = team_doc["bank"]
        if need["loadouts"]:
            loadouts = {}
            for key in common.KEYS:
                task = team_doc["questions"][key]["task"]
                entry = team_doc["loadouts"][task]
                if entry["tokens"] != common.est_tokens(entry["output"]) or entry["sha256"] != common.sha256(entry["output"]):
                    raise RoundSixError(f"team_notes.json: the loadout of {task} does not match its tokens or its hash")
                if f"`{task}`" not in questions["T"][key]:
                    raise RoundSixError(f"team_notes.json: {key} is about task {task}, which the workflow's question does not name")
                loadouts[key] = {"task": task, "output": entry["output"], "tokens": entry["tokens"]}
                sums[f"loadouts/{key}"] = common.sha256(entry["output"])
            args["loadouts"] = loadouts
    if sums:
        args["sums"] = sums
    args["matrix"] = matrix
    return args


def default_out(half: str) -> pathlib.Path:
    """Where the args go when --out is not given: the temporary folder, outside the repository."""
    return pathlib.Path(tempfile.gettempdir()) / f"round6-{half}-args.json"


def preflight_vm(version: str, run_tag: str, wsl: common.Wsl | None, wsl_args: dict | None, wsl_doc: dict | None, need: dict) -> None:
    """The virtual machine arms need the VM that prepare_wsl.py made. Every call here only reads: the Linux
    pgr must be in the VM, executable, the build that was installed (same version as the pgr on PATH and
    the same SHA-256), it must read the index of the copy as prepare_wsl.py left it, and the VM must hold no
    bank of the run tag. The packs are not asked again: prepare_wsl.py proved that the Linux build gives
    the packs of packs.json byte for byte, and wsl.json says so."""
    if wsl is None or wsl_args is None:
        raise RoundSixError("this half has virtual machine arms: pass --wsl-distro (prepare_wsl.py records it in benchmark/data/wsl.json)")
    if not wsl_doc:
        raise RoundSixError("this half has virtual machine arms: run prepare_wsl.py first (benchmark/data/wsl.json)")
    if wsl_doc.get("pgr_version") != version or wsl_doc.get("distro") != wsl_args["distro"]:
        raise RoundSixError(f"wsl.json was made for {wsl_doc.get('pgr_version')} in {wsl_doc.get('distro')}, not for {version} in {wsl_args['distro']}: run prepare_wsl.py again")
    if not (wsl_doc.get("packs") or {}).get("identical"):
        raise RoundSixError("wsl.json does not say that the Linux packs are the Windows packs byte for byte: run prepare_wsl.py again")
    _, _, code = wsl.run("test", "-x", wsl_args["pgr"], check=False)
    if code != 0:
        raise RoundSixError(f"{wsl_args['pgr']} is not in the VM {wsl_args['distro']}, or is not executable: run prepare_wsl.py, and pass --wsl-dir if it used another folder")
    seen = wsl.run(wsl_args["pgr"], "--version")[0].strip()
    if seen != version:
        raise RoundSixError(f"the pgr in the VM says {seen!r} and the pgr on PATH says {version!r}: run prepare_wsl.py again")
    sums = wsl.run("sha256sum", wsl_args["pgr"])[0].split()
    if not sums or sums[0] != wsl_doc.get("pgr_sha256"):
        raise RoundSixError("the pgr in the VM is not the build that prepare_wsl.py installed (its SHA-256 differs from wsl.json): run prepare_wsl.py again")
    status = wsl.run(wsl_args["pgr"], "--root", wsl_args["repo"], "status", cd=wsl_args["repo"])[0].strip()
    if not common.index_ready(status) or common.stable_status(status) != wsl_doc.get("status"):
        raise RoundSixError(f"the copy in the VM reports {status!r}, not the index prepare_wsl.py built ({wsl_doc.get('status')!r}): run prepare_wsl.py again")
    if need["hub_vm"]:
        listing, _, found = wsl.run("ls", f"{wsl.index_home()}/banks", check=False)
        taken = sorted(name for name in listing.split() if name.startswith(f"r6-{run_tag}-")) if found == 0 else []
        if taken:
            raise RoundSixError(f"the run tag {run_tag} is used already: {len(taken)} banks of it exist in the VM ({', '.join(taken[:3])}"
                                f"{', ...' if len(taken) > 3 else ''}). Use a new run tag")


def preflight(pgr: common.Pgr, need: dict, run_tag: str, repo: str, repo_team: str | None,
              packs_doc: dict | None, team_doc: dict | None, repo_words: str | None = None,
              wsl: common.Wsl | None = None, wsl_args: dict | None = None, wsl_doc: dict | None = None) -> str:
    """What has to be true on this machine before the workflow starts, or an error that says what to do.
    Returns the version line of the pgr that the agents will run. `wsl` is the way into the virtual machine
    and `wsl_args` the {distro, repo, pgr} the workflow will be given; both only for a half with VM arms."""
    where = shutil.which(pgr.prefix[0]) if len(pgr.prefix) == 1 else None
    version = pgr.require(fix=f"The agents run a bare `pgr`: the shell that starts the client has to find pgr 0.2 first on its PATH"
                              f"{' (it finds ' + where + ')' if where else ''}. Put the built pgr there, check `pgr --version`, and run this again")
    for what, doc in (("packs.json", packs_doc if need["variants"] else None), ("team_notes.json", team_doc if need["team"] else None)):
        if doc and doc.get("pgr_version") != version:
            raise RoundSixError(f"{what} was made by {doc.get('pgr_version')} and the pgr on PATH is {version}: make it again with the pgr that the agents will run")
    roots = [("--repo", repo)] + ([("--repo-team", repo_team)] if need["team"] else [])
    for name, root in roots:
        status = pgr.run("--root", root, "status")[0].strip()
        if not common.index_ready(status):
            raise RoundSixError(f"{name} {root}: {status!r}. An index is read only by the version that built it ({version}). "
                                f"Run: pgr --root {root} refresh")
    if need["words"]:
        # The arms without the neural network run with PGR_NO_MODEL=1 in front of every call, on a copy whose index was built that way.
        status = pgr.with_env(PGR_NO_MODEL="1").run("--root", repo_words, "status")[0].strip()
        if not common.index_ready(status) or not common.words_only(status):
            raise RoundSixError(f"--repo-words {repo_words}: {status!r}. With PGR_NO_MODEL=1 its status line has to say \"words only\"; "
                                f"run prepare_words.py with this pgr ({version})")
        if (packs_doc or {}).get("words", {}).get("pgr_version") not in (None, version):
            raise RoundSixError(f"the words-only packs were made by {packs_doc['words']['pgr_version']} and the pgr on PATH is {version}: run prepare_words.py again")
    home = common.default_home(pgr.env)
    if need["vm"]:
        preflight_vm(version, run_tag, wsl, wsl_args, wsl_doc, need)
    if need["hub_windows"]:
        taken = sorted(path.name for path in (home / "banks").glob(f"r6-{run_tag}-*"))
        if taken:
            raise RoundSixError(f"the run tag {run_tag} is used already: {len(taken)} banks of it exist in {home / 'banks'} ({', '.join(taken[:3])}"
                                f"{', ...' if len(taken) > 3 else ''}). A cell that finds the notes of an earlier attempt is not a clean cell: use a new run tag")
    if need["team"]:
        changes = prepare_team.verify(team_doc, home)
        if changes:
            raise RoundSixError(f"the bank {team_doc['bank']} in {home} is not the bank that prepare_team.py prepared ({'; '.join(changes[:3])}). "
                                f"Run prepare_team.py again with the pgr on PATH (it needs the bank folder removed first) and make the args after it")
    return version


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--half", choices=list(MATRIX_HALVES))
    parser.add_argument("--run-tag")
    parser.add_argument("--repo", help="the fixture, as the agents will see it")
    parser.add_argument("--repo-team", help="the team copy that prepare_team.py made")
    parser.add_argument("--repo-words", help="the words-only copy that prepare_words.py made")
    parser.add_argument("--wsl-distro", help="the distribution prepare_wsl.py used (default: the one in wsl.json)")
    parser.add_argument("--wsl-dir", help="the folder prepare_wsl.py used inside the VM (default: lxbench in the home folder of the VM user)")
    parser.add_argument("--wsl-program", default="wsl.exe", help="wsl.exe, or a command line for a stand-in in the tests")
    parser.add_argument("--count", action="store_true", help="print the cells and agents of the four matrix files, by class of model, and stop")
    parser.add_argument("--briefs", type=pathlib.Path, help="a JSON file with the shared briefs {q1, q2, q3} of set A, as in earlier rounds")
    parser.add_argument("--packs", type=pathlib.Path, default=common.DATA / "packs.json")
    parser.add_argument("--team-notes", type=pathlib.Path, default=common.DATA / "team_notes.json")
    parser.add_argument("--wsl-notes", type=pathlib.Path, default=common.DATA / "wsl.json")
    parser.add_argument("--out", type=pathlib.Path, help="default: round6-HALF-args.json in the temporary folder; never inside the repository")
    parser.add_argument("--workflow", type=pathlib.Path, default=common.WORKFLOW)
    parser.add_argument("--pgr", default="pgr", help="the pgr that the agents will run: a bare `pgr` is the one the shell finds on PATH (default)")
    parser.add_argument("--home", help="LXR_HOME for the checks (default: the one in the environment, else pgr's own)")
    parsed = parser.parse_args(argv)
    if parsed.count:
        print("\n".join(counts_text()))
        return 0
    if not (parsed.half and parsed.run_tag and parsed.repo):
        parser.error("--half, --run-tag and --repo are required (or --count)")
    out = parsed.out or default_out(parsed.half)
    try:
        if common.REPO in out.resolve().parents:
            raise RoundSixError(f"{out} is inside the repository, which is public, and the args hold your folders: write them outside it")
        matrix = json.loads((common.HERE / f"round6-{parsed.half}.json").read_text(encoding="utf-8"))
        load = lambda path: json.loads(path.read_text(encoding="utf-8")) if path.exists() else None
        packs_doc, team_doc, wsl_doc = load(parsed.packs), load(parsed.team_notes), load(parsed.wsl_notes)
        need = needs(matrix)
        vm = wsl_args = None
        if need["vm"]:
            distro = parsed.wsl_distro or (wsl_doc or {}).get("distro") or ""
            if not distro:
                raise RoundSixError("this half has virtual machine arms: pass --wsl-distro (prepare_wsl.py records it in benchmark/data/wsl.json)")
            vm = common.Wsl(distro, parsed.wsl_program)
            base = parsed.wsl_dir or f"{vm.home()}/lxbench"
            wsl_args = {"distro": distro, "repo": f"{base}/repo", "pgr": f"{base}/bin/pgr"}
        args = build(parsed.half, parsed.run_tag, parsed.repo, parsed.repo_team, load(parsed.briefs) if parsed.briefs else None,
                     packs_doc, team_doc, matrix, common.workflow_questions(parsed.workflow), parsed.repo_words, wsl_args)
        version = preflight(common.Pgr(parsed.pgr, home=parsed.home or None), need, parsed.run_tag, parsed.repo, parsed.repo_team, packs_doc, team_doc,
                            parsed.repo_words, vm, wsl_args, wsl_doc)
    except RoundSixError as error:
        print(f"round6_args: {error}", file=sys.stderr)
        return 1
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(args, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    cells = sum(len(block["reps"]) * len(block["arms"]) for block in matrix)
    print(f"half {parsed.half}: {cells} cells, run tag {parsed.run_tag}, {version} -> {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
