"""Prove that the parser still gives the same numbers for the rounds already stored.

    check_reparse.py [--results FILE] [--transcripts ROOT]
    check_reparse.py --old FILE --new FILE
    check_reparse.py --print-order [--transcripts ROOT]

Round six changed bench_parse.py. Every cell of rounds one to five must come out exactly as it is stored in
benchmark/data/results.json, byte for byte. Two checks:

  stored rows   the cells are built again from the stored agent rows with bench_parse.build_cells and
                compared with the stored cells. This needs nothing but the repository.
  transcripts   with --transcripts ROOT, the folder that holds the workflow transcripts (one wf_* folder per
                run), every transcript is read again, in the order the stored rows name their runs, and the
                whole result is compared with the stored file. This is the full proof; the transcripts
                stay on the machine that ran the rounds.

With --old and --new it checks a result that was parsed after a new round: --new may hold more agents and
cells than --old, and only those of round six arms, and every other row and cell must be the same in both,
byte for byte.

Exit code 0 when everything matches, 1 otherwise.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import sys

sys.path.append(str(pathlib.Path(__file__).resolve().parent))  # python -I leaves the script's folder off the path; the standard library stays first
import bench_parse
import round6_common as common


def stored_text(path: pathlib.Path) -> str:
    return path.read_bytes().decode("utf-8").replace("\r\n", "\n")


def first_difference(left: str, right: str) -> str:
    for number, (a, b) in enumerate(zip(left.split("\n"), right.split("\n")), 1):
        if a != b:
            return f"line {number}: stored {a.strip()[:70]!r}, parsed {b.strip()[:70]!r}"
    return f"the lengths differ ({len(left)} and {len(right)})"


def check_stored_rows(results: pathlib.Path) -> list[str]:
    stored = json.loads(stored_text(results))
    table = bench_parse.build_cells(stored["agents"])
    left, right = json.dumps(stored["cells"], indent=1), json.dumps(table, indent=1)
    return [] if left == right else [f"cells built from the stored rows differ from the stored cells, {first_difference(left, right)}"]


def run_order(results: pathlib.Path) -> list[str]:
    """The run folders in the order the stored rows name them. bench_parse.py appends the agents folder by
    folder, so a file that is parsed again lists its rows in the same order only when the folders are given in
    this order."""
    stored = json.loads(stored_text(results))
    return list(dict.fromkeys(row["run"] for row in stored["agents"]))


def check_transcripts(results: pathlib.Path, root: pathlib.Path) -> list[str]:
    runs = run_order(results)
    missing = [run for run in runs if not (root / run).is_dir()]
    if missing:
        return [f"{len(missing)} of {len(runs)} runs have no folder under {root}: {', '.join(missing[:3])}"]
    agents = bench_parse.read_folders([root / run for run in runs])
    text = json.dumps({"agents": agents, "cells": bench_parse.build_cells(agents)}, indent=1)
    expected = stored_text(results)
    return [] if text == expected else [f"the transcripts parse to a different file, {first_difference(expected, text)}"]


def check_added_only(old: pathlib.Path, new: pathlib.Path) -> list[str]:
    """New rows may only be round six rows: the rows and cells of every other arm are the same in both files."""
    before, after = json.loads(stored_text(old)), json.loads(stored_text(new))
    problems = []
    for kind in ("agents", "cells"):
        left = [row for row in before[kind] if not bench_parse.is_team(row["arm"])]
        right = [row for row in after[kind] if not bench_parse.is_team(row["arm"])]
        if json.dumps(left, indent=1) != json.dumps(right, indent=1):
            problems.append(f"the {kind} of the earlier rounds differ between the two files "
                            f"({len(left)} and {len(right)} rows), {first_difference(json.dumps(left, indent=1), json.dumps(right, indent=1))}")
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--results", type=pathlib.Path, default=common.DATA / "results.json")
    parser.add_argument("--transcripts", type=pathlib.Path)
    parser.add_argument("--old", type=pathlib.Path)
    parser.add_argument("--new", type=pathlib.Path)
    parser.add_argument("--print-order", action="store_true",
                        help="print the run folders in the order the stored rows name their runs (under --transcripts ROOT when given) and exit")
    args = parser.parse_args(argv)
    if args.print_order:
        for run in run_order(args.results):
            print(args.transcripts / run if args.transcripts else run)
        return 0
    if args.old or args.new:
        if not (args.old and args.new):
            parser.error("--old and --new go together")
        problems = check_added_only(args.old, args.new)
        print("earlier rounds:", "rows and cells identical" if not problems else problems[0])
        return 1 if problems else 0
    problems = check_stored_rows(args.results)
    print("stored rows:", "cells identical" if not problems else problems[0])
    if args.transcripts:
        more = check_transcripts(args.results, args.transcripts)
        print("transcripts:", "the whole file is identical" if not more else more[0])
        problems += more
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
