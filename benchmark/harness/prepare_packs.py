"""Compute the brief packs of round six before the round runs.

    prepare_packs.py --pgr PATH --fixture DIR [--home DIR] [--out FILE] [--budget N]

The workflow cannot run a command, so the context that the arms with P in their name put in a prompt is
computed here, by pgr 0.2, and passed to the workflow in args.packs (round6_args.py does that). For every
variant, question set and question this runs

    pgr --root FIXTURE [--engines graph|search|graph,search] pack "QUESTION" --budget 800

with PGR_NO_REFRESH=1, so that the index is read as it is and not rebuilt, and LXR_HOME as given. The
variants are gs (engines graph and search: TP and TPH), g (graph: TPG) and s (search: TPS). The question
texts are read out of token-benchmark.workflow.js, the one place they are written.

benchmark/data/packs.json holds the pgr version, the fixture's status line and, for each pack, the exact
command (with the placeholder path C:\\path\\to\\lxbench\\repo), the question, the output, its estimated
tokens and its SHA-256. Nothing is written unless every pack was made: a missing pack for an arm that
needs one is an error before any agent starts, here and again in the workflow.

--pgr is a path, or a command line such as '"python" -I stand_in.py' for a stand-in program.
"""
from __future__ import annotations

import argparse
import pathlib
import sys

sys.path.append(str(pathlib.Path(__file__).resolve().parent))  # python -I leaves the script's folder off the path; the standard library stays first
import round6_common as common
from round6_common import RoundSixError


def prepare(pgr: common.Pgr, fixture: pathlib.Path, questions: dict, budget: int = common.PACK_BUDGET) -> dict:
    """The packs.json document, from a pgr that is at least 0.2."""
    version = pgr.require()
    status = pgr.run("--root", fixture, "status")[0].strip()
    if not common.index_ready(status):
        # An index is read only by the version that built it, so the refresh has to be run with this same program.
        raise RoundSixError(f"the fixture has no index that {version} can read: {status!r}. Run: {' '.join(pgr.prefix)} --root {fixture} refresh")
    spellings = common.private_paths(fixture, pgr.env.get("LXR_HOME"))
    common.refuse_private(status, spellings, "the status line")
    packs: dict = {}
    for variant, engines in common.VARIANTS.items():
        packs[variant] = {}
        for letter in common.SETS:
            packs[variant][letter] = {}
            for key in common.KEYS:
                question = questions[letter][key]
                argv = ["pgr", "--root", common.FIXTURE_PLACEHOLDER, "--engines", engines, "pack", question, "--budget", str(budget)]
                out, _, _ = pgr.run("--root", fixture, "--engines", engines, "pack", question, "--budget", budget)
                if "\r" in out:
                    raise RoundSixError(f"pack {variant}/{letter}/{key} holds a carriage return; pgr prints line feeds only")
                output = out.rstrip("\n")
                if not output.strip():
                    raise RoundSixError(f"pack {variant}/{letter}/{key} is empty")
                common.refuse_private(output, spellings, f"pack {variant}/{letter}/{key}")
                packs[variant][letter][key] = {
                    "question": question,
                    "command": common.render(argv),
                    "output": output,
                    "tokens": common.est_tokens(output),
                    "sha256": common.sha256(output),
                }
    return {
        "about": "Brief packs of round six, made by prepare_packs.py: the output of pgr pack for each question, put in the prompt of the arms with P in their name.",
        "pgr_version": version,
        "status": status,
        "budget": budget,
        "environment": {"PGR_NO_REFRESH": "1"},
        "fixture": common.FIXTURE_PLACEHOLDER,
        "variants": dict(common.VARIANTS),
        "packs": packs,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--pgr", required=True, help="the pgr 0.2 program, or a command line for a stand-in")
    parser.add_argument("--fixture", required=True, type=pathlib.Path, help="the fixture repository whose index the agents will use")
    parser.add_argument("--home", help="LXR_HOME for every call (default: the one in the environment, else pgr's own)")
    parser.add_argument("--out", type=pathlib.Path, default=common.DATA / "packs.json")
    parser.add_argument("--budget", type=int, default=common.PACK_BUDGET)
    parser.add_argument("--workflow", type=pathlib.Path, default=common.WORKFLOW, help="the workflow whose question texts are used")
    args = parser.parse_args(argv)
    try:
        questions = common.workflow_questions(args.workflow)
        pgr = common.Pgr(args.pgr, home=args.home or None, env={"PGR_NO_REFRESH": "1"})
        doc = prepare(pgr, args.fixture, questions, args.budget)
        problems = common.check_packs(doc, questions)
        if problems:
            raise RoundSixError("the packs do not check out: " + "; ".join(problems[:5]))
        common.write_json(args.out, doc, private=(args.fixture, args.home))
    except RoundSixError as error:
        print(f"prepare_packs: {error}", file=sys.stderr)
        return 1
    count = sum(len(sets[letter]) for sets in doc["packs"].values() for letter in sets)
    total = sum(pack["tokens"] for sets in doc["packs"].values() for letter in sets.values() for pack in letter.values())
    print(f"{doc['pgr_version']}: {count} packs, {total} estimated tokens in all -> {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
