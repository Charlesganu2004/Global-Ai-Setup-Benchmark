"""Prepare the words-only copy of the fixture and its packs, for the arms without the neural network.

    prepare_words.py --pgr PATH --fixture DIR --words DIR [--home DIR] [--packs FILE] [--replace]

pgr's only neural part is its embedding table. PGR_NO_MODEL=1 makes the engine behave as if no model
existed, at index build and at query, so that search works on words alone. The arms RN and TPN of round six
measure that. This script

  1. copies the fixture to --words (it never touches the fixture) and builds the copy's index with
     PGR_NO_MODEL=1 in the environment,
  2. checks that the status line of the copy says "words only" and that its graph has the symbols and links
     of the fixture's own index (the graph does not depend on the model),
  3. runs pgr pack for every question of sets A and B with PGR_NO_MODEL=1 and the engines graph and search,
     and adds the result to benchmark/data/packs.json under the variant n, beside the packs that
     prepare_packs.py made (it needs that file first, made by the same pgr). The other packs are not touched.

It also records, in packs.json under "words", the fixture's placeholder path, the status line of the copy and
how many of the six packs are byte for byte the pack that the model gives (gs): a pack that is the same
text makes TPN the same prompt as TP, and the result should say so.

A pgr that ignores PGR_NO_MODEL (a build older than the variable) is refused at step 2. Run this with the
same pgr that the agents will run, in the same index home, because the index of the copy exists only there.
prepare_packs.py writes the whole file again and drops variant n: run this script after it.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

sys.path.append(str(pathlib.Path(__file__).resolve().parent))  # python -I leaves the script's folder off the path; the standard library stays first
import prepare_team
import round6_common as common
from round6_common import RoundSixError

VARIANT = "n"
GRAPH_PART = re.compile(r"graph ready \(\d+ symbols, \d+ links")


def build_words_copy(pgr: common.Pgr, fixture: pathlib.Path, words: pathlib.Path, replace: bool) -> str:
    """Copy the fixture, build the index of the copy with PGR_NO_MODEL=1 and return its status line."""
    version = pgr.require()
    prepare_team.copy_fixture(fixture, words, replace, maker="prepare_words.py", suffix=".round6-words")
    quiet = pgr.with_env(PGR_NO_MODEL="1")
    status = quiet.run("--root", words, "refresh", "--force")[0].strip()
    if not common.index_ready(status):
        raise RoundSixError(f"the index of the words-only copy cannot be read by {version}: {status!r}")
    if not common.words_only(status):
        raise RoundSixError(f"the status line of the words-only copy does not say \"words only\": {status!r}. This pgr ignores PGR_NO_MODEL=1 "
                            f"({version}); use the build that has the variable")
    return status


def prepare(pgr: common.Pgr, fixture: pathlib.Path, words: pathlib.Path, questions: dict, doc: dict, replace: bool = False) -> dict:
    """The packs.json document with variant n added, from a pgr that is at least 0.2 and has PGR_NO_MODEL."""
    version = pgr.require()
    if doc.get("pgr_version") != version:
        raise RoundSixError(f"packs.json was made by {doc.get('pgr_version')} and this pgr is {version}: run prepare_packs.py with this pgr first")
    fixture_status = doc["status"]
    status = build_words_copy(pgr, fixture, words, replace)
    want, got = GRAPH_PART.search(fixture_status), GRAPH_PART.search(status)
    if not (want and got and want.group() == got.group()):
        raise RoundSixError(f"the graph of the words-only copy is not the graph of the fixture: {status!r} against {fixture_status!r}")
    budget = doc.get("budget", common.PACK_BUDGET)
    asker = pgr.with_env(PGR_NO_MODEL="1", PGR_NO_REFRESH="1")
    spellings = common.private_paths(fixture, words, pgr.env.get("LXR_HOME"))
    common.refuse_private(status, spellings, "the status line of the words-only copy")
    engines = common.WORDS_VARIANTS[VARIANT]
    packs: dict = {}
    for letter in common.SETS:
        packs[letter] = {}
        for key in common.KEYS:
            question = questions[letter][key]
            argv = ["pgr", "--root", common.WORDS_PLACEHOLDER, "--engines", engines, "pack", question, "--budget", str(budget)]
            out, _, _ = asker.run("--root", words, "--engines", engines, "pack", question, "--budget", budget)
            if "\r" in out:
                raise RoundSixError(f"pack {VARIANT}/{letter}/{key} holds a carriage return; pgr prints line feeds only")
            output = out.rstrip("\n")
            if not output.strip():
                raise RoundSixError(f"pack {VARIANT}/{letter}/{key} is empty")
            common.refuse_private(output, spellings, f"pack {VARIANT}/{letter}/{key}")
            packs[letter][key] = {
                "question": question,
                "command": "$env:PGR_NO_MODEL='1'; " + common.render(argv),
                "output": output,
                "tokens": common.est_tokens(output),
                "sha256": common.sha256(output),
            }
    same = sum(packs[letter][key]["output"] == doc["packs"]["gs"][letter][key]["output"] for letter in common.SETS for key in common.KEYS)
    made = json.loads(json.dumps(doc))
    made["packs"][VARIANT] = packs
    made["words"] = {
        "about": "Variant n: the packs of the arms without the neural network, made with PGR_NO_MODEL=1 on a words-only copy of the fixture by prepare_words.py.",
        "pgr_version": version,
        "fixture": common.WORDS_PLACEHOLDER,
        "status": status,
        "environment": {"PGR_NO_MODEL": "1", "PGR_NO_REFRESH": "1"},
        "variants": dict(common.WORDS_VARIANTS),
        "identical_to_gs": {"packs": same, "of": len(common.SETS) * len(common.KEYS)},
    }
    return made


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--pgr", required=True, help="the pgr 0.2 program with PGR_NO_MODEL, or a command line for a stand-in")
    parser.add_argument("--fixture", required=True, type=pathlib.Path, help="the fixture repository")
    parser.add_argument("--words", required=True, type=pathlib.Path, help="where the words-only copy goes (args.repoWords)")
    parser.add_argument("--home", help="LXR_HOME for every call (default: the one in the environment, else pgr's own)")
    parser.add_argument("--packs", type=pathlib.Path, default=common.DATA / "packs.json", help="the file prepare_packs.py made; variant n is added to it")
    parser.add_argument("--replace", action="store_true", help="build the copy again (only a copy that this script made)")
    parser.add_argument("--workflow", type=pathlib.Path, default=common.WORKFLOW, help="the workflow whose question texts are used")
    args = parser.parse_args(argv)
    try:
        if " " in str(args.words):
            raise RoundSixError(f"--words {args.words} contains a space; the prompts put the path into a shell command unquoted")
        if not args.packs.exists():
            raise RoundSixError(f"{args.packs} does not exist: run prepare_packs.py first")
        questions = common.workflow_questions(args.workflow)
        pgr = common.Pgr(args.pgr, home=args.home or None)
        doc = prepare(pgr, args.fixture, args.words, questions, json.loads(args.packs.read_text(encoding="utf-8")), args.replace)
        problems = common.check_packs(doc, questions, (VARIANT,))
        if problems:
            raise RoundSixError("the packs do not check out: " + "; ".join(problems[:5]))
        common.write_json(args.packs, doc, private=(args.fixture, args.words, args.home))
    except RoundSixError as error:
        print(f"prepare_words: {error}", file=sys.stderr)
        return 1
    same = doc["words"]["identical_to_gs"]
    total = sum(pack["tokens"] for letter in doc["packs"][VARIANT].values() for pack in letter.values())
    print(f"{doc['pgr_version']}: words-only packs, {same['of']} packs, {total} estimated tokens in all, {same['packs']} of them the same text as the pack with the model -> {args.packs}")
    if same["packs"] == same["of"]:
        print("prepare_words: every pack is the same text as the pack with the model: TPN would send TP's prompts")
    return 0


if __name__ == "__main__":
    sys.exit(main())
