#!/usr/bin/env python3
"""Turn the measured benchmark data into what the site and the README read.

    python benchmark/harness/build_site.py

Reads   benchmark/data/results.json   every agent run, written by bench_parse.py
        benchmark/data/index.json     index build times and sizes
        benchmark/harness/arms.json   what each method is and the commands it was given
Writes  docs/data/benchmark.js        one object, window.BENCH, for the site's pages
        benchmark/RESULTS.md          the same numbers as tables

Every number in the site's prose is computed here from those files, so the
text cannot drift from the tables.
"""
from __future__ import annotations

import datetime
import json
import pathlib
import statistics
import sys

HERE = pathlib.Path(__file__).resolve().parent
BENCH = HERE.parent
REPO = BENCH.parent

# Dollars per million tokens: input, output, cache read, cache write (5 minute).
PRICE = {"C": (0.10, 0.50, 0.01, 0.125), "B": (2.00, 10.00, 0.20, 2.50), "A": (4.00, 20.00, 0.20, 5.00)}
MEAN = ("requests", "tool_calls", "start", "reread", "added", "output", "tokens", "cost", "cost_warm",
        "cost_cold", "quality", "wall_seconds", "tool_output_chars", "input", "cache_read", "cache_write")
# Characters of each block of the per-agent context, read from one transcript.
CONTEXT_CHARS = {"skills": 33278, "instructions": 21660, "deferred": 15977, "rest": 5166}
SHARED_PREFIX = 38476   # tokens every agent read from cache: system prompt and tool definitions
TIER_ORDER = "CBA"
# How the findings shown in the top-level README begin.
README_FINDINGS = ("An agent starts", "Where the shipped build", "The shipped build on", "Does adding token-goat",
                   "Do the engines do as well", "The model tier moves", "Orchestration buys")


def money(value: float) -> str:
    return f"${value:.2f}" if value >= 1 else f"${value:.3f}" if value >= 0.1 else f"${value:.4f}"


def tokens(value: float) -> str:
    return f"{value / 1e6:.2f}M" if value >= 1e6 else f"{round(value / 1e3)}k"


def load(name: str) -> dict:
    return json.loads((BENCH / name).read_text(encoding="utf-8"))


def points_of(cells: list[dict]) -> list[dict]:
    """One row per method, setup and tier: the mean of its complete runs."""
    groups: dict[tuple, list[dict]] = {}
    for cell in cells:
        if cell["complete"]:
            groups.setdefault((cell["arm"], cell["setup"], cell["tier"]), []).append(cell)
    points = []
    for (arm, setup, tier), runs in groups.items():
        point = {"arm": arm, "setup": setup, "tier": tier, "n": len(runs)}
        for field in MEAN:
            point[field] = statistics.fmean(run[field] for run in runs)
        point["scores"] = {q: statistics.fmean(run["scores"][q] or 0 for run in runs) for q in ("q1", "q2", "q3")}
        # Three questions per run: correct answers bought by a million tokens.
        point["efficiency"] = 3 * point["quality"] / (point["tokens"] / 1e6)
        point["spread"] = {"tokens": [min(run["tokens"] for run in runs), max(run["tokens"] for run in runs)],
                           "requests": [min(run["requests"] for run in runs), max(run["requests"] for run in runs)],
                           "perfect_runs": sum(run["quality"] >= 0.999 for run in runs)}
        if setup == "orch":
            point["orchestrator_cost_warm"] = statistics.fmean(run["orchestrator_cost_warm"] for run in runs)
            point["orchestrator_tokens"] = statistics.fmean(run["orchestrator_tokens"] for run in runs)
        points.append(point)
    return points


def pick(points: list[dict], arm: str, setup: str, tier: str) -> dict | None:
    return next((p for p in points if (p["arm"], p["setup"], p["tier"]) == (arm, setup, tier)), None)


def context_parts(start: float) -> list[dict]:
    rest = start - SHARED_PREFIX
    chars = sum(CONTEXT_CHARS.values())
    share = {key: rest * value / chars for key, value in CONTEXT_CHARS.items()}
    estimated = "estimated: its share of the characters in the part that is not shared"
    return [
        {"name": "System prompt and tool definitions", "tokens": round(SHARED_PREFIX),
         "how": "measured: the part every agent read from cache", "owner": "the client and its connected tools"},
        {"name": "Skill listing", "tokens": round(share["skills"]), "how": estimated,
         "owner": "you: one line per installed skill"},
        {"name": "Global rules and memory index", "tokens": round(share["instructions"]), "how": estimated,
         "owner": "you: the global rules this setup installs, plus the memory index"},
        {"name": "Names of tools not yet loaded", "tokens": round(share["deferred"]), "how": estimated,
         "owner": "you: connectors and MCP servers that are switched on"},
        {"name": "The task itself and session notes", "tokens": round(share["rest"]), "how": estimated,
         "owner": "the brief"},
    ]


def ranked(points: list[dict], setup: str, tier: str, minimum: int = 1) -> list[dict]:
    """Methods for one setup and tier, fewest tokens first, among those run at least `minimum` times."""
    rows = [p for p in points if p["setup"] == setup and p["tier"] == tier and p["n"] >= minimum]
    return sorted(rows, key=lambda p: p["tokens"])


def text_of(points: list[dict], agents: list[dict], index: dict, arms: dict, start: float) -> dict:
    total = sum(a["input"] + a["output"] + a["cache_read"] + a["cache_write"] for a in agents)
    reads = sum(a["cache_read"] for a in agents)
    writes = sum(a["cache_write"] for a in agents)
    outputs = sum(a["output"] for a in agents)
    name = {arm["id"]: arm["name"] for arm in arms["arms"]}
    single = [p for p in points if p["setup"] == "single"]
    # A single run is an example, not a result: the extremes quoted are of methods run at least twice.
    repeated = [p for p in single if p["n"] >= 2] or single
    # Among ties, the one with the most runs behind it.
    fewest = min(repeated, key=lambda p: (p["requests"], -p["n"], p["tokens"]))
    most = max(repeated, key=lambda p: (p["requests"], p["n"], p["tokens"]))

    def runs(p: dict) -> str:
        return f"{p['n']} run{'s' if p['n'] > 1 else ''}"
    cold = {tier: start * PRICE[tier][3] / 1e6 for tier in PRICE}
    warm = {tier: start * PRICE[tier][2] / 1e6 for tier in PRICE}

    def row(arm: str, setup: str, tier: str) -> dict | None:
        return pick(points, arm, setup, tier)

    def line(p: dict) -> str:
        return (f"{p['arm']} {name[p['arm']]}: {tokens(p['tokens'])} tokens, {p['requests']:.1f} turns, "
                f"{p['quality']:.0%} correct over {p['n']} run{'s' if p['n'] > 1 else ''}")

    findings = [
        f"An agent starts with about {tokens(start)} tokens of context before it reads one line of code, and every "
        f"later turn re-reads all of it. Across the {len(agents)} agents measured, {reads / total:.0%} of all tokens "
        f"were cache reads, {writes / total:.0%} were cache writes and {outputs / total:.1%} were output. What an "
        "index saves in tool output is small next to that. What it really buys is fewer turns.",
        f"Tokens follow turns. Among methods run at least twice, one agent answering all three questions averaged "
        f"from {fewest['requests']:.1f} turns ({name[fewest['arm']]}, Tier {fewest['tier']}, {runs(fewest)}: "
        f"{tokens(fewest['tokens'])} tokens) to {most['requests']:.1f} turns ({name[most['arm']]}, Tier "
        f"{most['tier']}, {runs(most)}: {tokens(most['tokens'])} tokens).",
    ]
    board = ranked(points, "single", "C", minimum=3)
    if board:
        place = {p["arm"]: i + 1 for i, p in enumerate(board)}
        top = "; ".join(line(p) for p in board[:5])
        findings.append(f"Cheapest tier, one agent, methods run at least three times, fewest tokens first: {top}.")
        named = [arm for arm in ("S1", "S3", "A9", "R1", "A0") if arm in place]
        if named and len(board) >= 5:
            findings.append(
                "Where the shipped build, the round 1 leaders and the control stand among those "
                f"{len(board)} methods: " + "; ".join(f"{line(row(arm, 'single', 'C'))}, place {place[arm]}" for arm in named) + ".")
    # Two turns is the floor: one to ask, one to answer. Which methods hit it every time?
    for tier in arms["tiers"]:
        every = [p for p in single if p["tier"] == tier["id"] and p["n"] >= 2
                 and p["spread"]["requests"][1] <= 2 and p["quality"] >= 0.999]
        if every:
            listed = ", ".join(f"{p['arm']} {name[p['arm']]}" for p in sorted(every, key=lambda p: p["tokens"]))
            findings.append(
                f"Two turns is the floor for this task: one to ask and one to answer. On {tier['name']}, "
                f"{len(every)} methods took exactly two turns in every run and answered everything correctly: {listed}.")

    def compare(label: str, pairs: list[tuple[str, str]], tiers: str = "CB") -> None:
        for tier in tiers:
            parts = []
            for first, second in pairs:
                one, two = row(first, "single", tier), row(second, "single", tier)
                if one and two:
                    parts.append(f"{name[first]} {tokens(one['tokens'])} and {one['requests']:.1f} turns "
                                 f"({one['n']} run{'s' if one['n'] > 1 else ''}), against {name[second]} "
                                 f"{tokens(two['tokens'])} and {two['requests']:.1f} turns "
                                 f"({two['n']} run{'s' if two['n'] > 1 else ''})")
            if parts:
                findings.append(f"{label}. Tier {tier}, one agent: " + "; ".join(parts) + ".")

    compare("The shipped build on an index with Rust embeddings and on the default index without them",
            [("S3", "S1"), ("S4", "S2")], "CBA")
    compare("What merging in the Rust index's name and doc search added on the default index: the shipped build "
            "against the round 2 build", [("S1", "N1"), ("S2", "N2")])
    compare("Round 2, before that search was merged in: the same commands on an index built with Rust embeddings "
            "and without", [("L4", "N1"), ("C13", "N2")])
    compare("Does adding token-goat beside lx change anything? Each pair is the same lx build without it and with it",
            [("A9", "C3"), ("R1", "C2"), ("C13", "C14"), ("L4", "C15")])
    compare("Do the engines do as well without lx in front of them? lx with every command against the raw commands of the engines it wraps",
            [("C13", "C5"), ("C13", "C1"), ("C13", "C4")])
    for tier in arms["tiers"]:
        base = row("A0", "single", tier["id"])
        full = [p for p in single if p["tier"] == tier["id"] and p["arm"] != "A0" and p["quality"] >= 0.999]
        # A single run is an example, not a result: prefer methods run more than once.
        full = [p for p in full if p["n"] >= 2] or full
        if base and full:
            best = min(full, key=lambda p: p["tokens"])
            findings.append(
                f"{tier['name']}, one agent: no index used {tokens(base['tokens'])} tokens and "
                f"{money(base['cost_warm'])} over {base['n']} run{'s' if base['n'] > 1 else ''}; the leanest "
                f"method that never answered wrongly was {name[best['arm']]} at {tokens(best['tokens'])} tokens and "
                f"{money(best['cost_warm'])} over {best['n']} run{'s' if best['n'] > 1 else ''}, "
                f"{1 - best['tokens'] / base['tokens']:.0%} fewer tokens.")
    c, b, a = (row("A0", "single", t) for t in TIER_ORDER)
    if c and b and a:
        findings.append(
            f"The model tier moves cost far more than the method does. The same no-index job cost "
            f"{money(c['cost_warm'])} on Tier C, {money(b['cost_warm'])} on Tier B and {money(a['cost_warm'])} on "
            "Tier A, for the same correct answers. Send lookups to the cheapest tier.")
    orch = [p for p in points if p["setup"] == "orch"]
    lowest = [p for p in orch if p["tier"] == "C"]
    if lowest:
        share = statistics.fmean(p["orchestrator_cost_warm"] / p["cost_warm"] for p in lowest)
        par = [p for p in points if p["setup"] == "parallel"]
        flawed = [p for p in par if p["quality"] < 0.999]
        findings.append(
            f"Orchestration buys accuracy and it is the expensive part. With Tier C subagents the orchestrator's two "
            f"calls were {share:.0%} of the run's cost. Without a reviewer, {len(flawed)} of {len(par)} three-subagent "
            f"runs returned a wrong detail; with the orchestrator checking, {sum(p['quality'] >= 0.999 for p in orch)} "
            f"of {len(orch)} orchestrated runs ended fully correct.")
    findings.append(
        f"A cold start costs far more than a warm one. Writing the {tokens(start)} start-up context to the cache "
        f"costs {money(cold['A'])} on Tier A against {money(warm['A'])} to read it back, {cold['A'] / warm['A']:.0f} "
        "times as much. Subagents started together all pay the cold price; started one after another within five "
        "minutes they share the cached part.")

    stacks = {s["id"]: s for s in index["stacks"]}
    timed = [s for s in index["stacks"] if s["cold"] is not None]
    smallest = min(index["stacks"], key=lambda s: s["disk"])
    slowest = max(timed, key=lambda s: s["cold"])
    combo, with_emb = stacks["combo"], stacks["combo-emb"]
    index_text = (
        f"One repository of {index['dataset']} "
        f"The combined default builds in {combo['cold']:g} s, takes {combo['one_file']:g} s to catch up after one "
        f"file changes, and uses {combo['disk']:g} MB: semble {combo['parts']['semble']:g} MB, graphify "
        f"{combo['parts']['graphify']:g} MB, Rust symbols {combo['parts']['codanna']:g} MB. Turning the Rust "
        f"index's embeddings on takes the same stack to {with_emb['cold']:g} s, {with_emb['one_file']:g} s and "
        f"{with_emb['disk']:g} MB. The smallest index measured is {smallest['name']} at {smallest['disk']:g} MB; "
        f"the slowest first build is {slowest['name']} at {slowest['cold']:g} s.")
    measured = len({p["arm"] for p in points})
    return {
        "lede": f"{len(agents)} agent runs on one repository: three lookup questions, {measured} ways of finding the "
                f"answer, three model tiers, three ways of organising the agents. {total / 1e6:.1f} million tokens "
                f"measured, {reads / total:.0%} of them cache reads.",
        "tiles": [
            {"value": tokens(start), "label": "tokens an agent starts with, before any work"},
            {"value": f"{reads / total:.0%}", "label": "of all measured tokens were cache reads"},
            {"value": f"{fewest['requests']:.1f} to {most['requests']:.1f}",
             "label": "average turns for the same three answers, leanest method to heaviest, each run at least twice"},
            {"value": f"{cold['A'] / warm['A']:.0f}x", "label": "cold start against warm start on the top tier"},
        ],
        "findings": findings,
        "cache": (
            f"Every agent in this test began with {tokens(start)} tokens: the client's system prompt, its tool "
            f"definitions, the skill listing, the global rules and the list of connected tools. About "
            f"{tokens(SHARED_PREFIX)} of that is identical for every agent and can be read from cache when another "
            f"agent ran in the last five minutes. The other {tokens(start - SHARED_PREFIX)} comes after the task "
            "text, so each new subagent writes it again. That is the floor under every delegated lookup."),
        "context": "The first bar segment is measured. The rest is the remaining tokens split by each block's share of characters.",
        "index": index_text,
        "method": [
            "Repository: 86 Python files, 2.97 MB, one commit, read-only for every agent. Ground truth for the three "
            "questions comes from Python's syntax tree, and every answer is graded against it by a script.",
            "Tiers: C is the Haiku class at low effort, B the Sonnet class at medium effort, A the Opus class at "
            "maximum effort. Orchestrators always ran at Tier A. Model names are resolved at run time by alias.",
            "Tokens are the usage fields the API returned for each request, summed per agent. Start-up context is the "
            "whole prompt of an agent's first request; re-read is cache-read tokens on later requests; added is new "
            "context written on later requests (tool results and the agent's own earlier output).",
            "Cost uses list prices per million tokens: Tier A $4 in, $20 out, $0.20 cache read; Tier B $2, $10, "
            "$0.20; Tier C $0.10, $0.50. Cache writes are priced at 1.25 times input and the Tier C cache read at a "
            "tenth of input, which are the published multipliers rather than figures printed for these models. "
            "Warm and cold differ only in how the start-up context is priced: all read from cache, or all written.",
            "The orchestrator's planning prompt is the same for every method and tier, so it was run "
            f"{len([x for x in agents if x['role'] == 'plan'])} times and its briefs reused; a run that reused them "
            "is charged the mean of the measured planning calls.",
            "Limits. One repository, one language, three easy questions, and between one and six runs per cell: a "
            "difference of one turn between two methods is inside the noise, and a single-run cell is an example, not "
            "an average. Every agent carried this machine's client, skills and connectors, so the start-up figure is "
            "this setup's, not a constant. Wall-clock seconds in round 2 were taken while another job held every "
            "processor core, so only round 1 timings mean anything; token counts do not depend on machine load.",
            "lx changed during the test and each row names the build it was measured with: first version, improved, "
            "shipped after round 1, the round 2 commands (about, cards, hybrid, ask), and the build that ships, "
            "whose merged search also asks the Rust index's name and doc search. Rows marked default index ran on "
            "a copy of the repository indexed without Rust embeddings.",
        ],
    }


def markdown(points: list[dict], index: dict, arms: dict, text: dict) -> str:
    name = {arm["id"]: arm["name"] for arm in arms["arms"]}
    order = [arm["id"] for arm in arms["arms"]]
    out = ["# Benchmark results", "", text["lede"], "",
           "The charts are on the project site. This file is the same data as tables, written by "
           "`benchmark/harness/build_site.py`.", "", "## Findings", ""]
    out += [f"- {line}" for line in text["findings"]]
    for setup in arms["setups"]:
        rows = sorted((p for p in points if p["setup"] == setup["id"]),
                      key=lambda p: (TIER_ORDER.index(p["tier"]), p["tokens"]))
        if not rows:
            continue
        out += ["", f"## {setup['name']}", "", setup["detail"] + " Fewest tokens first within each tier.", "",
                "| Method | Tier | Runs | Turns | Start-up | Re-read | Added | Output | Total tokens | Cost warm | Cost cold | Correct |",
                "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
        for p in rows:
            out.append(f"| {p['arm']} {name[p['arm']]} | {p['tier']} | {p['n']} | {p['requests']:.1f} | "
                       f"{p['start']:,.0f} | {p['reread']:,.0f} | {p['added']:,.0f} | {p['output']:,.0f} | "
                       f"{p['tokens']:,.0f} | {money(p['cost_warm'])} | {money(p['cost_cold'])} | {p['quality']:.0%} |")
    out += ["", "## Index build and size", "", index["dataset"], "",
            "| Index stack | First build, s | Nothing changed, s | One file changed, s | Disk, MB |",
            "|---|---:|---:|---:|---:|"]
    for s in index["stacks"]:
        cell = lambda v: "not measured" if v is None else f"{v:g}"
        out.append(f"| {s['name']} | {cell(s['cold'])} | {cell(s['unchanged'])} | {cell(s['one_file'])} | {cell(s['disk'])} |")
    out += ["", "## Methods", "", "| Code | Method | What the agent was given |", "|---|---|---|"]
    out += [f"| {arm['id']} | {arm['name']} | {arm['detail']} |" for arm in arms["arms"] if arm["id"] in {p["arm"] for p in points}]
    assert set(order) >= {p["arm"] for p in points}, "a measured method is missing from arms.json"
    out += ["", "## How it was measured", ""] + [f"- {line}" for line in text["method"]] + [""]
    return "\n".join(out)


def readme_block(points: list[dict], arms: dict, text: dict) -> str:
    """The short results section of the top-level README: the cheapest tier with
    one agent, fewest tokens first, then the findings."""
    name = {arm["id"]: arm["name"] for arm in arms["arms"]}
    board = ranked(points, "single", "C", minimum=3) or ranked(points, "single", "C")
    out = [text["lede"], "",
           "One agent on the cheapest model tier, fewest tokens first"
           + (" (methods run at least three times):" if board and board[0]["n"] >= 3 else ":"), "",
           "| # | Method | Runs | Turns | Tokens | Cost, cache warm | Correct |", "|---:|---|---:|---:|---:|---:|---:|"]
    for place, p in enumerate(board, 1):
        out.append(f"| {place} | {p['arm']} {name[p['arm']]} | {p['n']} | {p['requests']:.1f} | "
                   f"{p['tokens']:,.0f} | {money(p['cost_warm'])} | {p['quality']:.0%} |")
    # The README carries the short list; RESULTS.md and the site carry all of it.
    short = [line for line in text["findings"] if line.startswith(README_FINDINGS)]
    out += [""] + [f"- {line}" for line in short]
    return "\n".join(out)


def write_readme(block: str) -> bool:
    path, start, end = REPO / "README.md", "<!-- results:start -->", "<!-- results:end -->"
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return False
    if text.count(start) != 1 or text.count(end) != 1:
        return False
    head, rest = text.split(start)
    path.write_text(head + start + "\n" + block + "\n" + end + rest.split(end)[1], encoding="utf-8")
    return True


def measured_plan(agents: list[dict]) -> str:
    plans = [a for a in agents if a["role"] == "plan"]
    cold = [a["cost"] for a in plans if not a["start_cached"]]
    warm = [a["cost"] for a in plans if a["start_cached"]]
    if not cold or not warm:
        return ""
    return (f"planning call: {', '.join(money(v) for v in cold)} cold; "
            f"{', '.join(money(v) for v in warm)} warm")


def main() -> int:
    results, index, arms = load("data/results.json"), load("data/index.json"), load("harness/arms.json")
    cells, agents = results["cells"], results["agents"]
    points = points_of(cells)
    used = {p["arm"] for p in points}
    known = {arm["id"] for arm in arms["arms"]}
    if used - known:
        raise SystemExit(f"measured but not described in arms.json: {sorted(used - known)}")
    start = statistics.fmean(a["start"] for a in agents if a["role"] in ("all", "q1", "q2", "q3"))
    text = text_of(points, agents, index, arms, start)
    order = [arm["id"] for arm in arms["arms"]]
    setups = [s["id"] for s in arms["setups"]]
    described = []
    for arm in arms["arms"]:
        if arm["id"] in used:
            described.append({**arm, "commands": [arms["commands"][key] for key in arm["uses"]],
                              "reads": arm.get("reads", "Grep, Glob, Read" if arm["id"] == "A0" else "Read tool, on a range a command returned")})
    data = {
        "generated": datetime.date.today().isoformat(),
        "totals": {"agents": len(agents), "cells": len(cells), "start": start,
                   "tokens": sum(a["input"] + a["output"] + a["cache_read"] + a["cache_write"] for a in agents)},
        "groups": arms["groups"],
        "arms": described,
        "tiers": arms["tiers"],
        "setups": [s for s in arms["setups"] if any(p["setup"] == s["id"] for p in points)],
        "questions": arms["questions"],
        "points": points,
        "runs": sorted(cells, key=lambda c: (order.index(c["arm"]), setups.index(c["setup"]),
                                             TIER_ORDER.index(c["tier"]), c["rep"])),
        "index": index,
        "context": {"parts": context_parts(start)},
        "cache": [{"tier": f"{t['name']}: {t['model']}", "tokens": start, "cold": start * PRICE[t["id"]][3] / 1e6,
                   "warm": start * PRICE[t["id"]][2] / 1e6,
                   "measured": measured_plan(agents) if t["id"] == "A" else ""} for t in arms["tiers"]],
        "price": {tier: dict(zip(("input", "output", "cache_read", "cache_write"), rate)) for tier, rate in PRICE.items()},
        "text": text,
    }
    target = REPO / "docs" / "data" / "benchmark.js"
    target.parent.mkdir(parents=True, exist_ok=True)
    # "</" would end a script block early if it ever appeared inside the data.
    body = json.dumps(data, ensure_ascii=False, indent=1).replace("</", "<\\/")
    target.write_text("// Written by benchmark/harness/build_site.py. Do not edit by hand.\n"
                      f"window.BENCH = {body};\n", encoding="utf-8")
    (BENCH / "RESULTS.md").write_text(markdown(points, index, arms, text), encoding="utf-8")
    in_readme = write_readme(readme_block(points, arms, text))
    print(f"wrote {target.relative_to(REPO)} ({target.stat().st_size:,} bytes), benchmark/RESULTS.md"
          f"{' and the README results block' if in_readme else ' (README has no results markers)'}: "
          f"{len(points)} points from {len(cells)} runs, {len(agents)} agents, {len(described)} methods")
    return 0


if __name__ == "__main__":
    sys.exit(main())
