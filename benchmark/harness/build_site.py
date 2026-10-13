#!/usr/bin/env python3
"""Turn the measured benchmark data into what the site and the README read.

    python benchmark/harness/build_site.py

Reads   benchmark/data/results.json   every agent run, written by bench_parse.py
        benchmark/data/index.json     index build times and sizes
        benchmark/data/engine.json    optional: the "public" rows of engine speed
        benchmark/data/install.json   optional: the "public" install block
        benchmark/harness/arms.json   what each method is and the commands it was given
Writes  report/                       the full report, private: its data files, text and asset hashes
        docs/data/showcase.js         window.SHOWCASE, the public showcase's data
        docs/*.html                   the showcase pages' computed text and asset hashes
        benchmark/RESULTS.md          the same numbers as tables

The full report (report/) is not published. The showcase (docs/) is published by GitHub Pages, shows
measured outcomes only, and every build ends by scanning it for anything that must not be public.
    python benchmark/harness/build_site.py --guard DIR   run only that scan, on any folder

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
REPORT = REPO / "report"   # the full report: private, not published
DOCS = REPO / "docs"       # the public showcase: published by GitHub Pages

# Dollars per million tokens: input, output, cache read, cache write (5 minute).
PRICE = {"C": (0.10, 0.50, 0.01, 0.125), "B": (2.00, 10.00, 0.20, 2.50), "A": (4.00, 20.00, 0.20, 5.00)}
MEAN = ("requests", "tool_calls", "start", "reread", "added", "output", "tokens", "cost", "cost_warm",
        "cost_cold", "quality", "wall_seconds", "tool_output_chars", "input", "cache_read", "cache_write",
        "hook_calls", "hook_rewrites", "hook_denials", "hook_notes", "hook_redactions")
ROLE_FIELDS = ("tokens", "requests", "tool_calls", "output", "cost_warm", "cost_cold", "seconds")
# Characters of each block of the per-agent context, read from one transcript.
CONTEXT_CHARS = {"skills": 33278, "instructions": 21660, "deferred": 15977, "rest": 5166}
SHARED_PREFIX = 38476   # tokens every agent read from cache: system prompt and tool definitions
TIER_ORDER = "CBA"
# How the findings shown in the top-level README begin.
README_FINDINGS = ("An agent starts", "Where the shipped build", "The shipped build on", "Does adding token-goat",
                   "Do the engines do as well", "The model tier moves", "Orchestration buys",
                   "token-goat's hooks, installed")


def money(value: float) -> str:
    return f"${value:.2f}" if value >= 1 else f"${value:.3f}" if value >= 0.1 else f"${value:.4f}"


def share(value: float) -> str:
    """A share as a whole percentage, never rounded up to 100%: 99.6% is 99%."""
    return "100%" if value >= 0.9995 else f"{min(99, int(value * 100 + 0.5))}%"


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
        # The question set the runs answered: "A" is the original set and "B" the held-out one.
        point["set"] = runs[0].get("set", "A")
        if any(run.get("set", "A") != point["set"] for run in runs):
            raise SystemExit(f"{arm} {setup} {tier}: its runs mix question sets")
        for field in MEAN:
            point[field] = statistics.fmean(run[field] for run in runs)
        point["scores"] = {q: statistics.fmean(run["scores"][q] or 0 for run in runs) for q in ("q1", "q2", "q3")}
        # Three questions per run: correct answers bought by a million tokens.
        point["efficiency"] = 3 * point["quality"] / (point["tokens"] / 1e6)
        point["spread"] = {"tokens": [min(run["tokens"] for run in runs), max(run["tokens"] for run in runs)],
                           "requests": [min(run["requests"] for run in runs), max(run["requests"] for run in runs)],
                           "perfect_runs": sum(run["quality"] >= 0.999 for run in runs)}
        if setup.startswith("orch"):
            point["orchestrator_cost_warm"] = statistics.fmean(run["orchestrator_cost_warm"] for run in runs)
            point["orchestrator_tokens"] = statistics.fmean(run["orchestrator_tokens"] for run in runs)
            # Per role, the mean over runs; what the review changed, summed over runs.
            point["roles"] = {role: {field: statistics.fmean(run["roles"][role][field] for run in runs)
                                     for field in ROLE_FIELDS} for role in ("plan", "workers", "verify")}
            point["worker_quality"] = statistics.fmean(run["worker_quality"] for run in runs)
            point["worker_scores"] = {q: statistics.fmean((run["worker_scores"] or {}).get(q) or 0 for run in runs)
                                      for q in ("q1", "q2", "q3")}
            # An answer counts as right only with full marks; the scores above give part credit.
            point["worker_right"] = {q: sum(((run["worker_scores"] or {}).get(q) or 0) >= 0.999 for run in runs)
                                     for q in ("q1", "q2", "q3")}
            point["right"] = {q: sum((run["scores"][q] or 0) >= 0.999 for run in runs) for q in ("q1", "q2", "q3")}
            for field in ("fixed", "broken", "still_wrong"):
                point[field] = sum(run[field] for run in runs)
            point["reviewer"] = runs[0]["reviewer"]
            point["review_costs"] = [run["roles"]["verify"]["cost_warm"] for run in runs]
            point["review_outputs"] = [run["roles"]["verify"]["output"] for run in runs]
        points.append(point)
    return points

def brief(p: dict | None) -> dict | None:
    """The handful of figures one method, setup and tier is compared on."""
    if not p:
        return None
    correct = 3 * p["quality"]
    return {"n": p["n"], "tokens": p["tokens"], "requests": p["requests"], "tool_calls": p["tool_calls"],
            "cost_warm": p["cost_warm"], "cost_cold": p["cost_cold"], "quality": p["quality"],
            "perfect_runs": p["spread"]["perfect_runs"], "wall_seconds": p["wall_seconds"],
            "tool_output_chars": p["tool_output_chars"], "start": p["start"],
            "tokens_low": p["spread"]["tokens"][0], "tokens_high": p["spread"]["tokens"][1],
            # Three questions a run: what one correct answer cost.
            "tokens_per_correct": p["tokens"] / correct if correct else None,
            "cost_per_correct": p["cost_warm"] / correct if correct else None}


def orchestration_of(points: list[dict], arms: dict) -> dict:
    """Everything the orchestration page tabulates: who used what inside a run,
    what the review changed, the three setups side by side, and the reviewer variants."""
    order = [arm["id"] for arm in arms["arms"]]
    setups = [s["id"] for s in arms["setups"]]

    def row(p: dict) -> dict:
        roles = p["roles"]
        lead_cost = roles["plan"]["cost_warm"] + roles["verify"]["cost_warm"]
        lead_tokens = roles["plan"]["tokens"] + roles["verify"]["tokens"]
        return dict(brief(p), arm=p["arm"], setup=p["setup"], tier=p["tier"], roles=roles,
                    lead_cost_share=lead_cost / p["cost_warm"] if p["cost_warm"] else None,
                    lead_token_share=lead_tokens / p["tokens"] if p["tokens"] else None,
                    worker_quality=p["worker_quality"], questions=3 * p["n"], fixed=p["fixed"],
                    broken=p["broken"], still_wrong=p["still_wrong"], reviewer=p["reviewer"],
                    review_costs=p["review_costs"], review_outputs=p["review_outputs"])

    # Runs made with token-goat's hooks installed are compared on the hooks page, not counted here.
    hooked = {on for _, on in HOOK_PAIRS}
    points = [p for p in points if p["arm"] not in hooked]
    orch = [row(p) for p in points if p["setup"].startswith("orch")]
    place = lambda r: (TIER_ORDER.index(r["tier"]), order.index(r["arm"]), setups.index(r["setup"]))
    standard = sorted((r for r in orch if r["setup"] == "orch"), key=place)
    varied = {(r["arm"], r["tier"]) for r in orch if r["setup"] != "orch"}
    reviewers = sorted((r for r in orch if (r["arm"], r["tier"]) in varied),
                       key=lambda r: (order.index(r["arm"]), TIER_ORDER.index(r["tier"]), setups.index(r["setup"])))
    side_by_side = []
    for r in standard:
        one = brief(pick(points, r["arm"], "single", r["tier"]))
        three = brief(pick(points, r["arm"], "parallel", r["tier"]))
        side_by_side.append({"arm": r["arm"], "tier": r["tier"], "single": one, "parallel": three,
                             "orch": {key: r[key] for key in brief(pick(points, r["arm"], "orch", r["tier"]))},
                             "tokens_over_single": r["tokens"] / one["tokens"] if one else None,
                             "cost_over_single": r["cost_warm"] / one["cost_warm"] if one else None})

    def tally(rows: list[dict]) -> dict:
        return {"runs": sum(r["n"] for r in rows), "questions": sum(r["questions"] for r in rows),
                "wrong_before": sum(r["fixed"] + r["still_wrong"] for r in rows),
                "fixed": sum(r["fixed"] for r in rows), "broken": sum(r["broken"] for r in rows),
                "still_wrong": sum(r["still_wrong"] for r in rows),
                "perfect_runs": sum(r["perfect_runs"] for r in rows)}

    by_setup = {s["id"]: dict(tally([r for r in orch if r["setup"] == s["id"]]), name=s["name"])
                for s in arms["setups"] if any(r["setup"] == s["id"] for r in orch)}
    # Per question and subagent tier: how often the subagent was right, and the run after its review.
    scored = [p for p in points if p["setup"] == "orch"]
    per_question = []
    for tier in TIER_ORDER:
        rows = [p for p in scored if p["tier"] == tier]
        runs = sum(p["n"] for p in rows)
        for q in ("q1", "q2", "q3"):
            if runs:
                per_question.append({"tier": tier, "question": q, "answers": runs,
                                     "subagents_right": sum(p["worker_right"][q] for p in rows),
                                     "reviewed_right": sum(p["right"][q] for p in rows),
                                     "subagents": sum(p["worker_scores"][q] * p["n"] for p in rows) / runs,
                                     "reviewed": sum(p["scores"][q] * p["n"] for p in rows) / runs})
    # The standard reviewer on the methods and tier the variants were run on: the like-for-like row.
    matched = [r for r in reviewers if r["setup"] == "orch"]
    # Three subagents without a review against the same methods and tier with one.
    unreviewed = []
    for tier in TIER_ORDER:
        both = [r for r in side_by_side if r["tier"] == tier and r["parallel"]]
        if both:
            unreviewed.append({"tier": tier, "methods": len(both),
                               "parallel_runs": sum(r["parallel"]["n"] for r in both),
                               "parallel_perfect": sum(r["parallel"]["perfect_runs"] for r in both),
                               "orch_runs": sum(r["orch"]["n"] for r in both),
                               "orch_perfect": sum(r["orch"]["perfect_runs"] for r in both)})
    return {"roles": standard, "reviewers": reviewers, "setups": side_by_side, "review": by_setup,
            "review_matched": tally(matched) if matched else None, "unreviewed": unreviewed,
            "questions": per_question}


# The same prompt without token-goat's hooks and with them.
HOOK_PAIRS = (("A0", "H0"), ("A9", "H9"), ("S1", "H1"), ("S3", "H3"), ("A5", "H5"), ("SB", "HB"))


def hooks_of(points: list[dict], cells: list[dict], arms: dict, facts: dict) -> dict:
    rows = []
    for off, on in HOOK_PAIRS:
        for setup in (s["id"] for s in arms["setups"]):
            for tier in TIER_ORDER:
                without, with_hooks = pick(points, off, setup, tier), pick(points, on, setup, tier)
                if without and with_hooks:
                    runs = [c for c in cells if c["complete"] and (c["arm"], c["setup"], c["tier"]) == (on, setup, tier)]
                    twins = [c for c in cells if c["complete"] and (c["arm"], c["setup"], c["tier"]) == (off, setup, tier)]

                    def left_out(group: list[dict]) -> dict:
                        """The callers the lookup agent's own answer to question 1 lacked, with how many runs lacked each."""
                        names: dict = {}
                        for c in group:
                            for name in c.get("lookup_missed") or []:
                                names[name] = names.get(name, 0) + 1
                        return {"runs": sum(bool(c.get("lookup_missed")) for c in group), "names": names}

                    rows.append({"off": off, "on": on, "setup": setup, "tier": tier,
                                 "without": dict(brief(without), q1_incomplete=left_out(twins)),
                                 "with": dict(brief(with_hooks), **{key: with_hooks[key] for key in
                                              ("hook_calls", "hook_rewrites", "hook_denials", "hook_notes",
                                               "hook_redactions")},
                                              q1_incomplete=left_out(runs),
                                              # A run the hooks never ran in cannot have been changed by them.
                                              runs_with_hooks=sum(c["hook_calls"] > 0 for c in runs)),
                                 "token_change": with_hooks["tokens"] / without["tokens"] - 1,
                                 "turn_change": with_hooks["requests"] - without["requests"]})
    return {"pairs": rows, "facts": facts}



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


def text_of(points: list[dict], cells: list[dict], agents: list[dict], index: dict, arms: dict,
            start: float) -> dict:
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
                f"{p['spread']['perfect_runs']} of {p['n']} run{'s' if p['n'] > 1 else ''} fully correct")

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
            few, many = min(p["n"] for p in every), max(p["n"] for p in every)
            counted = f"{few}" if few == many else f"{few} to {many}"
            findings.append(
                f"Two turns is the floor for this task: one to ask and one to answer. On {tier['name']}, "
                f"{len(every)} methods took exactly two turns and answered everything correctly in each of their "
                f"{counted} runs: {listed}.")

    def spread(p: dict) -> str:
        low, high = p["spread"]["requests"]
        turns = f"{low} turns" if low == high else f"{low} to {high} turns"
        return f"{turns} over {p['n']} run{'s' if p['n'] > 1 else ''}"

    def compare(label: str, pairs: list[tuple[str, str]], tiers: str = "CB") -> None:
        for tier in tiers:
            parts = []
            for first, second in pairs:
                one, two = row(first, "single", tier), row(second, "single", tier)
                if one and two:
                    parts.append(f"{name[first]} {tokens(one['tokens'])} ({spread(one)}), against "
                                 f"{name[second]} {tokens(two['tokens'])} ({spread(two)})")
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
        others = [p for p in single if p["tier"] == tier["id"] and p["arm"] != "A0"]
        # A single run is an example, not a result: the comparison is among methods run at least twice.
        full = [p for p in others if p["n"] >= 2 and p["quality"] >= 0.999]
        if base and full:
            best = min(full, key=lambda p: p["tokens"])
            once = [p for p in others if p["n"] == 1 and p["quality"] >= 0.999]
            lowest = min(once, key=lambda p: p["tokens"]) if once else None
            findings.append(
                f"{tier['name']}, one agent: no index used {tokens(base['tokens'])} tokens and "
                f"{money(base['cost_warm'])} over {base['n']} run{'s' if base['n'] > 1 else ''}. "
                + (f"Among the {len(full)} methods run at least twice and fully correct every time, the leanest was "
                   if len(full) > 1 else "Only one method was run twice on this tier and was fully correct both times: ")
                + f"{name[best['arm']]} at {tokens(best['tokens'])} tokens and {money(best['cost_warm'])} over "
                f"{best['n']} runs, {1 - best['tokens'] / base['tokens']:.0%} fewer tokens."
                + (f" {len(once)} method{'s' if len(once) > 1 else ''} ran once on this tier and answered "
                   f"everything; the lowest of those single runs was {name[lowest['arm']]} at "
                   f"{tokens(lowest['tokens'])} tokens." if lowest and lowest["tokens"] < best["tokens"] else ""))
    c, b, a = (row("A0", "single", t) for t in TIER_ORDER)
    if c and b and a:
        findings.append(
            f"The model tier moves cost far more than the method does. With the start-up context cached, the same "
            f"no-index job cost {money(c['cost_warm'])} on Tier C ({share(c['quality'])} correct), "
            f"{money(b['cost_warm'])} on Tier B ({share(b['quality'])}) and {money(a['cost_warm'])} on Tier A "
            f"({share(a['quality'])}). Send lookups to the cheapest tier.")
    orch = [p for p in points if p["setup"] == "orch"]   # the standard reviewer only
    lowest = [p for p in orch if p["tier"] == "C"]
    if lowest:
        orch_share = statistics.fmean(p["orchestrator_cost_warm"] / p["cost_warm"] for p in lowest)
        # Counted in runs, not in methods: a method run twice is two runs.
        par = [c for c in cells if c["setup"] == "parallel" and c["complete"] and c["tier"] == "C"]
        flawed = [c for c in par if c["quality"] < 0.999]
        checked = [c for c in cells if c["setup"] == "orch" and c["complete"] and c["tier"] == "C"]
        before = sum(c["fixed"] + c["still_wrong"] == 0 for c in checked)
        findings.append(
            f"Orchestration buys accuracy and it is the expensive part. With Tier C subagents the orchestrator's two "
            f"calls were {share(orch_share)} of the run's cost. Without a brief or a reviewer, {len(flawed)} of "
            f"{len(par)} three-subagent runs returned a wrong detail; orchestrated on the same tier, "
            f"{sum(c['quality'] >= 0.999 for c in checked)} of {len(checked)} ended fully correct, {before} of them "
            f"before the review changed anything.")
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
        "lede": f"{len(agents)} agent runs on one repository: three lookup questions and, in round five, three held-out ones, {measured} ways of finding the "
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
            "maximum effort. The planning call always ran at Tier A, and so did the review, except in the "
            "reviewer variants described below. Model names are resolved at run time by alias.",
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
            "Reviewer variants. In the standard orchestrated run the reviewer is the Tier A model with the method's "
            "own tools. Three variants swap only the reviewer: Tier A with no tools, Tier B with tools and Tier C "
            "with tools. The planning briefs and the subagents are the same.",
            "Hooks. The runs without token-goat's hooks come from the rounds made before the hooks were ever "
            "installed and from a round made after they were uninstalled again; the runs with them were made in "
            "between, in one round. The client applies a hook change at once, so no restart "
            "separates them. A hook run that let a call through is a row in an agent's transcript, which is how "
            "runs and rewritten calls are counted; a call the hook refused leaves no row, only an error in place of "
            "the result, and each of those is counted as one hook run and one refused call. Methods measured with "
            "the hooks on carry \"hooks on\" in their name and are never averaged with their twins.",
            "One round was stopped part-way and resumed, and the resumed round ran most of its agents a second "
            "time. Where an agent has two transcripts the later one is counted and the earlier one is left out, "
            "so no run is counted twice and no half-finished one is graded.",
            "Limits. One repository, one language, three easy questions, and between one and "
            f"{max(p['n'] for p in points)} runs per cell: a "
            "difference of one turn between two methods is inside the noise, and a single-run cell is an example, not "
            "an average. Every agent carried the test machine's client, skills and connectors, so the start-up "
            "figure belongs to that machine and is not a constant. Wall-clock seconds depend on what else the "
            "machine was doing: round 2 and part of round 4 ran while other jobs held every processor core, and a "
            "row can mix runs from several rounds. Read seconds as a rough guide and never across rounds; token "
            "counts do not depend on machine load.",
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
                       f"{p['tokens']:,.0f} | {money(p['cost_warm'])} | {money(p['cost_cold'])} | {share(p['quality'])} |")
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
                   f"{p['tokens']:,.0f} | {money(p['cost_warm'])} | {share(p['quality'])} |")
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


def orchestration_text(orchestration: dict, arms: dict) -> list[str]:
    """What the orchestration tables say, in sentences computed from them."""
    out = []
    review = orchestration["review"].get("orch")
    if review:
        out.append(
            f"Across {review['runs']} orchestrated runs with the standard reviewer, the subagents handed back "
            f"{review['wrong_before']} wrong answers out of {review['questions']}. The reviewer corrected "
            f"{review['fixed']} of them, left {review['still_wrong']} wrong, and changed {review['broken']} "
            f"right answers into wrong ones. {review['perfect_runs']} of the {review['runs']} runs ended fully correct.")
    for tier in arms["tiers"]:
        rows = [r for r in orchestration["roles"] if r["tier"] == tier["id"]]
        if rows:
            out.append(
                f"With {tier['name']} subagents ({sum(r['n'] for r in rows)} runs over {len(rows)} methods), the "
                f"orchestrator's planning and review were "
                f"{share(statistics.fmean(r['lead_token_share'] for r in rows))} of the tokens and "
                f"{share(statistics.fmean(r['lead_cost_share'] for r in rows))} of the cost of a run, "
                f"cache warm. The three subagents together averaged "
                f"{tokens(statistics.fmean(r['roles']['workers']['tokens'] for r in rows))} tokens and the review "
                f"{tokens(statistics.fmean(r['roles']['verify']['tokens'] for r in rows))}.")
            ratios = [r for r in orchestration["setups"] if r["tier"] == tier["id"] and r["tokens_over_single"]]
            if ratios:
                out[-1] += (
                    f" Against one agent of that tier answering all three questions with the same method, the "
                    f"orchestrated run used {statistics.fmean(r['tokens_over_single'] for r in ratios):.1f} times the "
                    f"tokens (from {min(r['tokens_over_single'] for r in ratios):.1f} to "
                    f"{max(r['tokens_over_single'] for r in ratios):.1f}) and "
                    f"{statistics.fmean(r['cost_over_single'] for r in ratios):.0f} times the money.")
    for tally in orchestration["unreviewed"]:
        out.append(
            f"Three subagents with nobody reviewing them, Tier {tally['tier']}: {tally['parallel_perfect']} of "
            f"{tally['parallel_runs']} runs were fully correct. The same {tally['methods']} methods on that tier with "
            f"the standard review: {tally['orch_perfect']} of {tally['orch_runs']}.")
    reviewer_name = {s["id"]: s.get("reviewer", s["name"]) for s in arms["setups"]}
    standard = {(r["arm"], r["tier"]): r for r in orchestration["reviewers"] if r["setup"] == "orch"}

    def span(values: list[float], fmt) -> str:
        return fmt(values[0]) if values[0] == values[-1] else f"{fmt(values[0])} to {fmt(values[-1])}"

    for key in orchestration["review"]:
        if key == "orch":
            continue
        # A variant is set against the standard reviewer on the methods and tier both were run on, run by run.
        paired = [(r, standard[(r["arm"], r["tier"])]) for r in orchestration["reviewers"]
                  if r["setup"] == key and (r["arm"], r["tier"]) in standard]
        if not paired:
            continue
        costs = sorted(cost for r, _ in paired for cost in r["review_costs"])
        base = sorted(cost for _, twin in paired for cost in twin["review_costs"])
        wrote = sorted(count for r, _ in paired for count in r["review_outputs"])
        base_wrote = sorted(count for _, twin in paired for count in twin["review_outputs"])
        wrong = sum(r["fixed"] + r["still_wrong"] for r, _ in paired)
        fixed, broken = sum(r["fixed"] for r, _ in paired), sum(r["broken"] for r, _ in paired)
        cheaper = sum(statistics.fmean(r["review_costs"]) < statistics.fmean(twin["review_costs"])
                      for r, twin in paired)
        caught = (f"It was handed {wrong} wrong answer{'' if wrong == 1 else 's'}, corrected {fixed} and broke "
                  f"{broken} right ones" if wrong or broken else
                  "Its subagents handed it no wrong answer, so these runs price the reviewer and do not show "
                  "whether it catches a mistake")
        out.append(
            f"{reviewer_name[key]}: {len(costs)} runs on {len(paired)} methods. A review cost {span(costs, money)} "
            f"(median {money(statistics.median(costs))}) against {span(base, money)} (median "
            f"{money(statistics.median(base))}) over {len(base)} runs of the standard reviewer on the same methods, "
            f"cheaper on average on {cheaper} of the {len(paired)}, and wrote {span(wrote, lambda v: format(v, ',.0f'))} output tokens (median "
            f"{statistics.median(wrote):,.0f}) against a median of {statistics.median(base_wrote):,.0f}. {caught}. "
            f"{sum(r['perfect_runs'] for r, _ in paired)} of {len(costs)} runs ended fully correct.")
    return out


def hooks_text(hooks: dict, arms: dict) -> list[str]:
    """What the off and on pairs say, grouped by whether the hooks could see the method's tool calls."""
    by_id = {arm["id"]: arm for arm in arms["arms"]}
    pairs = hooks["pairs"]
    out = []

    def weighed(rows: list[dict], side: str, field: str) -> float:
        # Both sides are weighted by the runs made with the hooks, pair by pair, so the two
        # means are over the same mix of setups and tiers.
        return sum(r[side][field] * r["with"]["n"] for r in rows) / sum(r["with"]["n"] for r in rows)

    def change(rows: list[dict]) -> float:
        return weighed(rows, "with", "tokens") / weighed(rows, "without", "tokens") - 1

    def repeat_spread(rows: list[dict]) -> float | None:
        """How far repeats of one prompt lie apart without the hooks: highest minus lowest run over
        the mean, across the pairs whose hooks-off side was run more than once, weighted like the means."""
        repeated = [r for r in rows if r["without"]["n"] > 1]
        if not repeated:
            return None
        return (sum((r["without"]["tokens_high"] - r["without"]["tokens_low"]) / r["without"]["tokens"]
                    * r["with"]["n"] for r in repeated) / sum(r["with"]["n"] for r in repeated))

    def verdict(rows: list[dict]) -> str:
        spread = repeat_spread(rows)
        if spread is None:
            return "with no repeat of the hooks-off side to judge that against"
        return (("inside" if abs(change(rows)) <= spread else "more than")
                + f" the {spread:.0%} by which repeats of one prompt without the hooks differ")

    def ran(rows: list[dict]) -> str:
        return f"{sum(r['with']['runs_with_hooks'] for r in rows)} of {sum(r['with']['n'] for r in rows)} runs"

    def moved(value: float) -> str:
        return f"{abs(value):.0%} {'more' if value >= 0 else 'fewer'} tokens"

    def rewrote(rows: list[dict]) -> str:
        count = weighed(rows, "with", "hook_rewrites")
        return "rewrote nothing" if count < 0.05 else f"rewrote {count:.1f} tool results a run"

    def summary(rows: list[dict], what: str) -> str:
        off_runs, on_runs = sum(r["without"]["n"] for r in rows), sum(r["with"]["n"] for r in rows)
        thin = sum(min(r["with"]["n"], r["without"]["n"]) == 1 for r in rows)
        return (f"{what}: {on_runs} runs with the hooks against {off_runs} without, compared setup by setup and "
                f"tier by tier ({len(rows)} pairs, {thin} of them with a single run on one side). The hooks ran in "
                f"{ran(rows)}, {weighed(rows, 'with', 'hook_calls'):.1f} times a run on average, "
                f"{rewrote(rows)}, added {weighed(rows, 'with', 'hook_notes'):.1f} notes a run and refused "
                f"{weighed(rows, 'with', 'hook_denials'):.2f} calls a run. Tokens a run: "
                f"{tokens(weighed(rows, 'without', 'tokens'))} without, {tokens(weighed(rows, 'with', 'tokens'))} with "
                f"({change(rows):+.0%}, {verdict(rows)}; pair by pair from "
                f"{min(r['token_change'] for r in rows):+.0%} to {max(r['token_change'] for r in rows):+.0%}). "
                f"Turns: {weighed(rows, 'without', 'requests'):.1f} and "
                f"{weighed(rows, 'with', 'requests'):.1f}. Mean score: "
                f"{share(weighed(rows, 'without', 'quality'))} and {share(weighed(rows, 'with', 'quality'))}. "
                f"Runs fully correct: {perfect(rows, 'without')} of {off_runs} and {perfect(rows, 'with')} of {on_runs}.")

    def perfect(rows: list[dict], side: str) -> int:
        return sum(r[side]["perfect_runs"] for r in rows)

    def redaction(rows: list[dict]) -> str:
        """What the redaction markers did to question 1, counted from the lookup agents' own answers."""
        on_runs, off_runs = sum(r["with"]["n"] for r in rows), sum(r["without"]["n"] for r in rows)
        short_on = sum(r["with"]["q1_incomplete"]["runs"] for r in rows)
        short_off = sum(r["without"]["q1_incomplete"]["runs"] for r in rows)
        names: dict = {}
        for r in rows:
            for name, count in r["with"]["q1_incomplete"]["names"].items():
                names[name] = names.get(name, 0) + count
        listed = ", ".join(f"{name} in {count}" for name, count in sorted(names.items(), key=lambda kv: -kv[1]))
        return (f"The rewritten results carried {weighed(rows, 'with', 'hook_redactions'):.1f} redaction markers a "
                f"run, where the hook put a marker in place of source text it took for a secret. The agent that "
                f"looked up the callers left one out in {short_on} of {on_runs} runs with the hooks"
                + (f" ({listed})" if listed else "") + f", against {short_off} of {off_runs} without them. "
                + str((hooks.get("facts") or {}).get("redaction_note", "")))

    native = [r for r in pairs if by_id[r["off"]]["uses"] == ["grep"]]
    bash = [r for r in pairs if by_id[r["on"]].get("shell") == "Bash"]
    shell = [r for r in pairs if r not in native and r not in bash]
    if native:
        out.append(summary(native, "No index, where every lookup is one of the client's own Grep, Glob and Read calls"))
        if weighed(native, "with", "hook_redactions") > 0:
            out.append(redaction(native))
    if shell:
        quiet = [r for r in shell if r["with"]["runs_with_hooks"] == 0]
        out.append(summary(shell, "Index commands sent through PowerShell (" +
                           ", ".join(sorted({r["off"] for r in shell}, key=list(by_id).index)) + ")"))
        if quiet:
            apart = [r for r in quiet if abs(r["token_change"]) >= 0.05]
            lower = sum(r["token_change"] < 0 for r in apart)
            down = "all of them" if apart and lower == len(apart) else f"{lower} of them"
            least = min(min(r["with"]["n"], r["without"]["n"]) for r in quiet)
            most = max(max(r["with"]["n"], r["without"]["n"]) for r in quiet)
            # One agent a run in these pairs, so the start-up figure is what a single agent began with.
            alone = [r for r in quiet if r["setup"] == "single"] or quiet
            out.append(
                f"In {len(quiet)} of those {len(shell)} method, setup and tier pairs no hook ran at all, because "
                f"PowerShell is not a tool they match. {len(apart)} of the {len(quiet)} still differ by 5% or more in "
                f"tokens, {down} downward (the {len(quiet)} run from "
                f"{min(r['token_change'] for r in quiet):+.0%} to {max(r['token_change'] for r in quiet):+.0%}). No "
                f"hook ran in those runs, so the hooks did not make that gap. The two sides were not otherwise "
                f"identical: every run with the hooks comes from one round, and the agents of each round start from "
                f"slightly different context (an agent began with {weighed(alone, 'with', 'start'):,.0f} tokens with "
                f"the hooks on against {weighed(alone, 'without', 'start'):,.0f} without: the version of the rules "
                f"file the session had loaded, the request it relays to its agents, the skill list). With {least} to "
                f"{most} runs a side, these rows cannot separate that from chance.")
    if bash:
        out.append(summary(bash, "The one-call command sent through Bash, a tool the hooks do match"))
    if native and shell and bash:
        out.insert(0, (
            f"token-goat's hooks, installed in the client: with no index they ran in {ran(native)}, "
            f"{rewrote(native)}, and the runs used {moved(change(native))}, {verdict(native)}; "
            f"{perfect(native, 'with')} of {sum(r['with']['n'] for r in native)} of those runs ended fully correct, "
            f"against {perfect(native, 'without')} of {sum(r['without']['n'] for r in native)} without the hooks. "
            f"With index commands "
            f"sent through PowerShell they ran in {ran(shell)}. Through Bash they ran in {ran(bash)}, "
            f"{rewrote(bash)}, and the runs used {moved(change(bash))}, {verdict(bash)}."))
    facts = hooks.get("facts") or {}
    if facts.get("block_bytes"):
        sizes = ", ".join(f"{size:,} bytes in {path}" for path, size in facts["block_bytes"].items())
        out.append(
            f"Installing the hooks also writes a routing block into each rules file ({sizes}), about "
            f"{facts.get('block_tokens_estimate', 0):,} tokens that every request of every agent then carries. "
            f"The agents measured here did not carry it, so that cost is on top of the figures above.")
    return out


def more_tables(orchestration: dict, hooks: dict, arms: dict, text: dict) -> str:
    """The orchestration and hooks tables of RESULTS.md."""
    name = {arm["id"]: arm["name"] for arm in arms["arms"]}
    setup_name = {s["id"]: s["name"] for s in arms["setups"]}
    reviewer_name = {s["id"]: s.get("reviewer", s["name"]) for s in arms["setups"]}
    cell = lambda value, fmt: "" if value is None else fmt(value)
    out = ["", "## Orchestrated runs", ""] + [f"- {line}" for line in text["orchestration"]]
    out += ["", "### Who used what inside an orchestrated run", "",
            "Means over the runs of each method and subagent tier. Cost is with the start-up context cached.", "",
            "| Method | Subagent tier | Runs | Plan tokens | Subagent tokens | Review tokens | Total tokens | "
            "Plan cost | Subagent cost | Review cost | Total cost | Orchestrator share of cost | Review turns | Seconds |",
            "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for r in orchestration["roles"]:
        roles = r["roles"]
        out.append(f"| {r['arm']} {name[r['arm']]} | {r['tier']} | {r['n']} | {roles['plan']['tokens']:,.0f} | "
                   f"{roles['workers']['tokens']:,.0f} | {roles['verify']['tokens']:,.0f} | {r['tokens']:,.0f} | "
                   f"{money(roles['plan']['cost_warm'])} | {money(roles['workers']['cost_warm'])} | "
                   f"{money(roles['verify']['cost_warm'])} | {money(r['cost_warm'])} | {share(r['lead_cost_share'])} | "
                   f"{roles['verify']['requests']:.1f} | {r['wall_seconds']:.0f} |")
    out += ["", "### What the review changed", "",
            "Counted per question: three questions a run. An answer counts as wrong when it is not fully right. "
            "The two score columns are mean scores, where a partly right answer earns part credit.", "",
            "| Method | Subagent tier | Runs | Subagents, mean score | After review, mean score | Wrong before review | "
            "Corrected | Still wrong | Right answers broken | Runs fully correct |",
            "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for r in orchestration["roles"]:
        out.append(f"| {r['arm']} {name[r['arm']]} | {r['tier']} | {r['n']} | {share(r['worker_quality'])} | "
                   f"{share(r['quality'])} | {r['fixed'] + r['still_wrong']} | {r['fixed']} | {r['still_wrong']} | "
                   f"{r['broken']} | {r['perfect_runs']} of {r['n']} |")
    out += ["", "### One agent, three subagents, orchestrated: the same method and tier", "",
            "| Method | Tier | One agent: tokens | cost | correct | Three subagents: tokens | cost | correct | "
            "Orchestrated: tokens | cost | correct | Orchestrated over one agent, tokens | cost |",
            "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for r in orchestration["setups"]:
        one, three, full = r["single"], r["parallel"], r["orch"]
        out.append(f"| {r['arm']} {name[r['arm']]} | {r['tier']} | "
                   + " | ".join([cell(one, lambda v: f"{v['tokens']:,.0f}"), cell(one, lambda v: money(v['cost_warm'])),
                                 cell(one, lambda v: share(v['quality'])),
                                 cell(three, lambda v: f"{v['tokens']:,.0f}"), cell(three, lambda v: money(v['cost_warm'])),
                                 cell(three, lambda v: share(v['quality'])),
                                 f"{full['tokens']:,.0f}", money(full['cost_warm']), share(full['quality']),
                                 cell(r['tokens_over_single'], lambda v: f"{v:.1f}x"),
                                 cell(r['cost_over_single'], lambda v: f"{v:.0f}x")]) + " |")
    out += ["", "### What one correct answer cost", "",
            "Three questions a run: the tokens and the money of a run divided by three times its mean score, so a "
            "partly right answer counts in part. "
            "Seconds is the wall clock of a run, with subagents running side by side; rounds ran under "
            "different machine load, so it is a rough guide.", "",
            "| Method | Tier | Setup | Runs | Mean score | Tokens per correct answer | Cost per correct answer | Seconds |",
            "|---|---|---|---:|---:|---:|---:|---:|"]
    for r in orchestration["setups"]:
        for key in ("single", "parallel", "orch"):
            b = r[key]
            if b and b["tokens_per_correct"]:
                out.append(f"| {r['arm']} {name[r['arm']]} | {r['tier']} | {setup_name[key]} | {b['n']} | "
                           f"{share(b['quality'])} | {b['tokens_per_correct']:,.0f} | {money(b['cost_per_correct'])} | "
                           f"{b['wall_seconds']:.0f} |")
    if orchestration["questions"]:
        out += ["", "### Which question the subagents got wrong", "",
                "Standard reviewer, every method together. Each run asks each question once.", "",
                "| Subagent tier | Question | Answers | Subagents fully right | After review fully right | "
                "Subagents, mean score | After review, mean score |",
                "|---|---|---:|---:|---:|---:|---:|"]
        for q in orchestration["questions"]:
            out.append(f"| {q['tier']} | {q['question'].upper()} | {q['answers']} | "
                       f"{q['subagents_right']} of {q['answers']} | {q['reviewed_right']} of {q['answers']} | "
                       f"{share(q['subagents'])} | {share(q['reviewed'])} |")
    if orchestration["reviewers"]:
        # The variants ran on a few methods on one tier. The standard reviewer is listed twice: on
        # everything it reviewed, and on those same methods, which is the row to compare a variant with.
        tallies = [(reviewer_name[key] + (", every method and tier" if key == "orch" else ""), tally)
                   for key, tally in orchestration["review"].items()]
        if orchestration["review_matched"]:
            tallies.insert(1, (reviewer_name["orch"] + ", on the methods and tier the variants ran on",
                               orchestration["review_matched"]))
        out += ["", "### Reviews, added up by reviewer", "",
                "| Reviewer | Runs | Answers | Wrong before review | Corrected | Still wrong | Right answers broken | "
                "Runs fully correct |", "|---|---:|---:|---:|---:|---:|---:|---:|"]
        for label, tally in tallies:
            out.append(f"| {label} | {tally['runs']} | {tally['questions']} | {tally['wrong_before']} | "
                       f"{tally['fixed']} | {tally['still_wrong']} | {tally['broken']} | "
                       f"{tally['perfect_runs']} of {tally['runs']} |")
    if orchestration["reviewers"]:
        out += ["", "### Who reviews, and with what", "",
                "| Method | Subagent tier | Reviewer | Runs | Review tokens | Review output tokens | Review turns | "
                "Review cost | Total tokens | Total cost | Subagents, mean score | After review, mean score | Corrected | Still wrong | "
                "Broken | Runs fully correct |",
                "|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
        for r in orchestration["reviewers"]:
            out.append(f"| {r['arm']} {name[r['arm']]} | {r['tier']} | {reviewer_name[r['setup']]} | {r['n']} | "
                       f"{r['roles']['verify']['tokens']:,.0f} | {r['roles']['verify']['output']:,.0f} | "
                       f"{r['roles']['verify']['requests']:.1f} | {money(r['roles']['verify']['cost_warm'])} | "
                       f"{r['tokens']:,.0f} | {money(r['cost_warm'])} | {share(r['worker_quality'])} | "
                       f"{share(r['quality'])} | {r['fixed']} | {r['still_wrong']} | {r['broken']} | "
                       f"{r['perfect_runs']} of {r['n']} |")
    out += ["", "## token-goat hooks, off and on", ""] + [f"- {line}" for line in text["hooks"]]
    if hooks["pairs"]:
        out += ["", "Means per run. Hook runs, calls rewritten, notes added and calls refused are per run with "
                "the hooks on. Runs the hooks ran in counts the runs where a hook fired at least once. Seconds are "
                "left out: the two sides come from different rounds, run under different machine load.", "",
                "| Method | Setup | Tier | Runs off / on | Tokens off | Tokens on | Change | Turns off | Turns on | "
                "Cost off | Cost on | Mean score off | Mean score on | Runs fully correct off | Runs fully correct on | "
                "Start-up tokens off | Start-up tokens on | "
                "Runs the hooks ran in | Hook runs | Calls rewritten | Redaction markers | Notes added | Calls refused |",
                "|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
        for pair in hooks["pairs"]:
            a, b = pair["without"], pair["with"]
            out.append(f"| {pair['off']} {name[pair['off']]} | {setup_name[pair['setup']]} | {pair['tier']} | "
                       f"{a['n']} / {b['n']} | {a['tokens']:,.0f} | {b['tokens']:,.0f} | {pair['token_change']:+.0%} | "
                       f"{a['requests']:.1f} | {b['requests']:.1f} | {money(a['cost_warm'])} | {money(b['cost_warm'])} | "
                       f"{share(a['quality'])} | {share(b['quality'])} | {a['perfect_runs']} of {a['n']} | "
                       f"{b['perfect_runs']} of {b['n']} | {a['start']:,.0f} | "
                       f"{b['start']:,.0f} | {b['runs_with_hooks']} of {b['n']} | {b['hook_calls']:.1f} | "
                       f"{b['hook_rewrites']:.1f} | {b['hook_redactions']:.1f} | {b['hook_notes']:.1f} | "
                       f"{b['hook_denials']:.2f} |")
    facts = hooks.get("facts") or {}
    if facts:
        out += ["", "### What the hooks are", "",
                f"- Version: {facts.get('version', '')}.",
                f"- Licence: {facts.get('licence', '')}.",
                f"- Events hooked in Claude Code: {', '.join(facts.get('claude_events', []))}.",
                f"- Tools matched before a call: `{facts.get('pre_tool_matcher', '')}`.",
                f"- Not matched: {facts.get('not_matched', '')}",
                f"- {facts.get('block_note', '')}",
                f"- {facts.get('redaction_note', '')}",
                f"- {facts.get('skill_note', '')}",
                f"- {facts.get('context_note', '')}",
                f"- How this was measured: {facts.get('how_measured', '')}"]
    return "\n".join(out) + "\n"


# What each page script reads is all that is published. The rest stays in benchmark/data/results.json.
POINT_FIELDS_LEFT_OUT = ("roles", "worker_quality", "worker_scores", "worker_right", "right", "fixed", "broken",
                         "still_wrong", "reviewer", "review_costs", "review_outputs", "orchestrator_tokens", "spread",
                         "hook_calls", "hook_rewrites", "hook_denials", "hook_notes", "hook_redactions",
                         "input", "cache_read", "cache_write", "tool_output_chars")
RUN_FIELDS_LEFT_OUT = ("input", "cache_read", "cache_write", "start_cached", "tool_output_chars", "worker_scores",
                       "lookup_missed", "orchestrator_cost", "orchestrator_tokens", "reviewer", "hook_calls",
                       "hook_rewrites", "hook_denials", "hook_notes", "hook_redactions")


def rounded(value):
    """Floats to eight significant digits. The pages format every number they show, and a mean
    such as 129686.33333333333 costs bytes on every page load for digits nobody sees."""
    if isinstance(value, float):
        return float(f"{value:.8g}")
    if isinstance(value, dict):
        return {key: rounded(item) for key, item in value.items()}
    if isinstance(value, list):
        return [rounded(item) for item in value]
    return value


def write_site_data(data: dict) -> list[tuple[str, int]]:
    """Write the site's data as one small file every page loads and three that only the pages
    showing them load: the single runs, the orchestration tables and the hooks tables."""
    folder = REPORT / "data"
    folder.mkdir(parents=True, exist_ok=True)
    core = {key: value for key, value in data.items() if key not in ("runs", "orchestration", "hooks")}
    core["points"] = [{k: v for k, v in point.items() if k not in POINT_FIELDS_LEFT_OUT} for point in data["points"]]
    runs = [{k: v for k, v in run.items() if k not in RUN_FIELDS_LEFT_OUT} for run in data["runs"]]
    files = (("benchmark.js", "window.BENCH = {};", core),
             ("bench-runs.js", "(window.BENCH = window.BENCH || {}).runs = {};", runs),
             ("bench-orchestration.js", "(window.BENCH = window.BENCH || {}).orchestration = {};", data["orchestration"]),
             ("bench-hooks.js", "(window.BENCH = window.BENCH || {}).hooks = {};", data["hooks"]))
    written = []
    for name, statement, value in files:
        # "</" would end a script block early if it ever appeared inside the data.
        body = json.dumps(rounded(value), ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/")
        path = folder / name
        path.write_text("// Written by benchmark/harness/build_site.py. Do not edit by hand.\n"
                        + statement.replace("{}", body) + "\n", encoding="utf-8")
        written.append((name, path.stat().st_size))
    return written


SITE_PAGES = (("home", "index.html"), ("results", "results.html"), ("orchestration", "orchestration.html"),
              ("hooks", "hooks.html"), ("methods", "methods.html"), ("setup", "setup.html"), ("how", "how.html"))


def site_sections() -> list[dict]:
    """Every section heading of the site's pages, read from their HTML, for the jump box.
    Read, not listed by hand, so a renamed or added section cannot leave the list behind."""
    import html
    import re
    out = []
    for key, name in SITE_PAGES:
        page = (REPORT / name).read_text(encoding="utf-8")
        for match in re.finditer(r'<h2 id="([^"]+)"[^>]*>(.*?)</h2>', page, re.S):
            title = html.unescape(re.sub(r"<[^>]+>", "", match.group(2))).strip()
            out.append({"page": key, "id": match.group(1), "title": title})
    return out


def hook_effect(hooks: dict, arms: dict) -> dict | None:
    """What the hooks changed on the runs with no index: runs and fully correct runs with and without them,
    the change in tokens, and the spread between repeats of one prompt. The report's answers and the
    public findings state the same figures."""
    by_id = {arm["id"]: arm for arm in arms["arms"]}
    native = [r for r in hooks["pairs"] if by_id[r["off"]]["uses"] == ["grep"]]
    if not native:
        return None
    on, off = sum(r["with"]["n"] for r in native), sum(r["without"]["n"] for r in native)
    right_on, right_off = (sum(r[side]["perfect_runs"] for r in native) for side in ("with", "without"))

    def mean(side: str, field: str) -> float:   # weighted by the runs made with the hooks, pair by pair
        return sum(r[side][field] * r["with"]["n"] for r in native) / on

    change = mean("with", "tokens") / mean("without", "tokens") - 1
    repeated = [r for r in native if r["without"]["n"] > 1]
    spread = (sum((r["without"]["tokens_high"] - r["without"]["tokens_low"]) / r["without"]["tokens"] * r["with"]["n"]
                  for r in repeated) / sum(r["with"]["n"] for r in repeated)) if repeated else 0.0
    return {"runs_with": on, "runs_without": off, "right_with": right_on, "right_without": right_off,
            "change": change, "spread": spread,
            "saved": change < -spread,   # fewer tokens by more than repeats of one prompt differ
            "worse": right_on / on < right_off / off,
            "hidden": mean("with", "hook_redactions") > 0 and sum(r["with"]["q1_incomplete"]["runs"] for r in native) > 0}


def answers_of(methods: list[dict], orchestration: dict, hooks: dict, arms: dict) -> dict:
    """One sentence for each question a visitor arrives with, each computed from the data."""
    name = {arm["id"]: arm["name"] for arm in arms["arms"]}
    out = {}
    board = ranked(methods, "single", "C", minimum=3) or ranked(methods, "single", "C")
    base = pick(methods, "A0", "single", "C")
    if board and base:
        best = board[0]
        level = [p for p in board if p["tokens"] <= best["tokens"] * 1.01]   # within one percent of the lowest
        tie = (f"{len(level)} methods are level at about {tokens(best['tokens'])} tokens" if len(level) > 1
               else f"{name[best['arm']]} at {tokens(best['tokens'])} tokens")
        out["cheapest"] = (
            f"{tie} and {best['requests']:.1f} turns for three lookups on the cheapest tier, "
            f"{1 - best['tokens'] / base['tokens']:.0%} fewer than no index ({tokens(base['tokens'])}). "
            f"The build that ships, {name['S1']}, is one of them." if any(p["arm"] == "S1" for p in level) and len(level) > 1 else
            f"{tie} and {best['requests']:.1f} turns for three lookups on the cheapest tier, "
            f"{1 - best['tokens'] / base['tokens']:.0%} fewer than no index ({tokens(base['tokens'])}).")
    lowest = [t for t in orchestration["unreviewed"] if t["tier"] == "C"]
    ratios = [r["cost_over_single"] for r in orchestration["setups"] if r["tier"] == "C" and r["cost_over_single"]]
    if lowest and ratios:
        t = lowest[0]
        better = t["orch_perfect"] / t["orch_runs"] > t["parallel_perfect"] / t["parallel_runs"]
        out["orchestrator"] = (
            ("It buys accuracy at a price. " if better else "Not on accuracy, in these runs. ")
            + f"On the cheapest subagents {t['orch_perfect']} of {t['orch_runs']} "
            f"orchestrated runs ended fully correct, against {t['parallel_perfect']} of {t['parallel_runs']} for three "
            f"subagents with nobody reviewing, and a run cost about {statistics.fmean(ratios):.0f} times one agent.")
    effect = hook_effect(hooks, arms)
    if effect:
        on, off = effect["runs_with"], effect["runs_without"]
        right_on, right_off, change, spread = (effect[key] for key in ("right_with", "right_without", "change", "spread"))
        out["hooks"] = (
            ("They saved tokens on these lookups. " if effect["saved"] and not effect["worse"] else "Not on these lookups. ")
            + (f"Token use fell by {-change:.0%}" if effect["saved"] else
               "Token use stayed inside the spread between repeats" if abs(change) <= spread else
               f"Token use rose by {change:.0%}")
            + f", and with no index {right_on} of {on} runs ended fully correct with the hooks against "
            f"{right_off} of {off} without"
            + (", because a redaction marker hid a line the lookup needed." if effect["worse"] and effect["hidden"] else "."))
    return out


def write_static_text(text: dict) -> list[str]:
    """Write the answers and the 'In short' lines into the pages themselves, so they are there
    without JavaScript and cannot fall behind the data: this runs on every build."""
    import html
    import re

    def items(lines: list[str]) -> str:
        return "".join(f"\n      <li>{html.escape(line, quote=False)}</li>" for line in lines) + "\n    "

    wanted = {"cheapest", "orchestrator", "hooks"} - set(text["answers"])
    if wanted or not text["results_short"] or not text["orchestration_short"] or not text["hooks"]:
        raise SystemExit("the data gives no text for: " + ", ".join(sorted(wanted) or ["an 'In short' block"])
                         + ". Nothing was written into the pages.")
    plans = {
        "index.html": [(rf'(<span class="a" data-ask="{key}">).*?(</span>)', html.escape(answer, quote=False))
                       for key, answer in text["answers"].items()],
        "results.html": [(r'(<ul id="r-short">).*?(</ul>)', items(text["results_short"]))],
        "orchestration.html": [(r'(<ul id="o-verdict">).*?(</ul>)', items(text["orchestration_short"]))],
        "hooks.html": [(r'(<p id="h-verdict">).*?(</p>)', html.escape(text["hooks"][0], quote=False) if text["hooks"] else "")],
    }
    changed = []
    for name, slots in plans.items():
        path = REPORT / name
        raw = path.read_bytes().decode("utf-8")
        page = raw
        for pattern, body in slots:
            page, count = re.subn(pattern, lambda m, body=body: m.group(1) + body + m.group(2), page, count=1, flags=re.S)
            if count != 1:
                raise SystemExit(f"{name}: no place for {pattern}")
        if page != raw:
            path.write_bytes(page.encode("utf-8"))
            changed.append(name)
    return changed


def stamp_assets(docs: pathlib.Path = REPORT, names: tuple | list | None = None) -> int:
    """Give every script and stylesheet link in the pages a `?v=` made from the file it points at.
    A browser keeps scripts for a while; without this, a visitor could get a new page with last
    week's script and a broken layout until the cache ran out."""
    import hashlib
    import re
    stamped = 0

    def marked(match):
        target = docs / match.group(2)
        if not target.is_file():
            return match.group(0)
        digest = hashlib.sha1(target.read_bytes().replace(b"\r\n", b"\n")).hexdigest()[:8]
        return f'{match.group(1)}="{match.group(2)}?v={digest}"'

    for name in names or [name for _, name in SITE_PAGES]:
        page = docs / name
        raw = page.read_bytes().decode("utf-8")
        new = re.sub(r'\b(src|href)="((?:assets|data)/[\w.-]+\.(?:js|css))(?:\?v=[0-9a-f]*)?"', marked, raw)
        if new != raw:
            page.write_bytes(new.encode("utf-8"))
            stamped += 1
    return stamped


def measured_plan(agents: list[dict]) -> str:
    plans = [a for a in agents if a["role"] == "plan"]
    cold = [a["cost"] for a in plans if not a["start_cached"]]
    warm = [a["cost"] for a in plans if a["start_cached"]]
    if not cold or not warm:
        return ""
    return (f"planning call: {', '.join(money(v) for v in cold)} cold; "
            f"{', '.join(money(v) for v in warm)} warm")


# ----------------------------------------------------------------------------------------------
# The public showcase, docs/. Measured outcomes only. Nothing that would let a reader copy the
# approach: no commands, prompts, settings, tool names or internal method ids reach these files,
# and the guard at the end of the build stops it if one does.
# ----------------------------------------------------------------------------------------------

PUBLIC_CLASSES = (("C", "light", "Light"), ("B", "mid", "Mid"), ("A", "top", "Top"))
PUBLIC_FAMILIES = {"control": "Control", "tool": "Separate tools", "engine-v1": "Our engine, first generation",
                   "engine-v2": "Our engine, second generation"}
BRAND = "PeterGanuRusty"          # the product; "PGR" is its short form and "pgr" its command
PUBLIC_PAGES = (("index.html", "Overview"), ("benchmarks.html", "Benchmarks"), ("study.html", "Study"),
                ("install.html", "Install"))
PUBLIC_QUESTIONS = (
    {"id": "callers", "title": "Who calls a function",
     "text": "Name every function in the repository that calls a given function."},
    {"id": "definition", "title": "Where a function is defined",
     "text": "Give the file and the line where a named function is defined, its parameter names and the functions that call it."},
    {"id": "purpose", "title": "Which function does a job",
     "text": "Given a described job, name the function that does it, say where it is defined and name its counterpart, the function that reverses it."},
)
PUBLIC_CLASS_TEXT = {"light": "The smallest model class, run at low reasoning effort.",
                     "mid": "A middle model class, run at medium reasoning effort.",
                     "top": "The largest model class, run at the highest reasoning effort."}
PUBLIC_SETUPS = ("One agent answering all three questions.",
                 "Three agents, one question each, with nobody checking their answers.",
                 "One agent that briefs three workers and checks what they return.")
PUBLIC_REPOSITORY_LINES = 68000   # about; the measured data records the file count, not the line count
BEST_MIN_RUNS = 3                 # a cell run fewer times than this cannot be named the best


# The two question sets. "A" in the measured data is the original set and "B" the held-out one. The held-out
# text is the line the Benchmarks page shows under the control, so it states the set in the owner's words.
PUBLIC_SETS = (
    {"key": "A", "id": "original", "name": "Original",
     "text": "The three lookups every round has used."},
    {"key": "B", "id": "held-out", "name": "Held-out",
     "text": "Three more lookups of the same shapes, written after the engine was built and never seen by the "
             "people or agents who built it."},
)
SET_OF = {s["key"]: s["id"] for s in PUBLIC_SETS}
TIER_OF_CLASS = {cid: tier for tier, cid, _ in PUBLIC_CLASSES}
# The four cells the best configuration is chosen on: the light and mid classes, on both question sets.
BEST_CELLS = tuple((cid, s["id"]) for s in PUBLIC_SETS for _, cid, _ in PUBLIC_CLASSES[:2])


def public_point(p: dict, class_id: str) -> dict:
    return {"class": class_id, "set": SET_OF[p["set"]], "setup": "single", "runs": p["n"], "tokens": p["tokens"],
            "tokens_low": p["spread"]["tokens"][0], "tokens_high": p["spread"]["tokens"][1],
            "turns": p["requests"], "correct": p["quality"], "perfect_runs": p["spread"]["perfect_runs"],
            "seconds": p["wall_seconds"]}


def cell_of(config: dict | None, class_id: str, set_id: str) -> dict | None:
    """The point of one configuration on one model class and one question set."""
    if not config:
        return None
    return next((pt for pt in config["points"] if pt["class"] == class_id and pt["set"] == set_id), None)


def best_config(configs: list[dict]) -> tuple[str | None, str | None]:
    """The slug of the best configuration and the rule that chose it. "cells": among the configurations with at
    least BEST_MIN_RUNS runs in each of the four BEST_CELLS, the highest share of fully correct runs out of all
    its runs; a tie goes to the lower mean tokens over the four cells. "light": when no configuration has all
    four cells, the earlier rule, the highest share of fully correct runs on the light class and the original
    questions among those with at least BEST_MIN_RUNS runs there, then the fewer tokens."""
    from fractions import Fraction
    whole = []
    for config in configs:
        found = [cell_of(config, cid, sid) for cid, sid in BEST_CELLS]
        if all(pt and pt["runs"] >= BEST_MIN_RUNS for pt in found):
            whole.append((Fraction(sum(pt["perfect_runs"] for pt in found), sum(pt["runs"] for pt in found)),
                          -statistics.fmean(pt["tokens"] for pt in found), config["slug"]))
    if whole:
        return max(whole)[2], "cells"
    light = [(Fraction(pt["perfect_runs"], pt["runs"]), -pt["tokens"], config["slug"]) for config in configs
             for pt in [cell_of(config, "light", "original")] if pt and pt["runs"] >= BEST_MIN_RUNS]
    return (max(light)[2], "light") if light else (None, None)


def first_generation(configs: list[dict]) -> dict | None:
    """The first-generation engine configuration measured on the most of the four best cells; the later one
    when two are measured on as many."""
    found = [(sum(cell_of(c, cid, sid) is not None for cid, sid in BEST_CELLS), n, c)
             for n, c in enumerate(configs) if c["family"] == "engine-v1"]
    return max(found, key=lambda item: item[:2])[2] if found else None


def of(count: int, total: int) -> str:
    return f"{count} of {total}"


def quoted(names: list[str]) -> str:
    """Configuration names in quotation marks, joined as a sentence would."""
    marked = [f'"{name}"' for name in names]
    return marked[0] if len(marked) == 1 else ", ".join(marked[:-1]) + " and " + marked[-1]


def kilo(value: float) -> str:
    return f"{value / 1e3:.1f}k"


def cell_counts(config: dict | None, cells: tuple = BEST_CELLS) -> tuple[int, int, int] | None:
    """Fully correct runs, runs and cells found, over the cells a configuration was measured on."""
    found = [pt for pt in (cell_of(config, cid, sid) for cid, sid in cells) if pt]
    return (sum(pt["perfect_runs"] for pt in found), sum(pt["runs"] for pt in found), len(found)) if found else None


def public_findings(configs: list[dict], cells: list[dict], orchestration: dict, hooks: dict, arms: dict,
                    hook_test: set, owner: dict, engine: list[dict]) -> dict:
    """The findings in plain sentences with their numbers. `owner` maps the id of an arm to the slug of the public
    configuration its runs belong to. A sentence is written only when the numbers it quotes exist."""
    by_slug = {c["slug"]: c for c in configs}
    best = next((c for c in configs if c["best"]), None)
    control = next((c for c in configs if c["family"] == "control"), None)
    first = first_generation(configs)
    by_arm = {arm_id: by_slug[slug] for arm_id, slug in owner.items()}
    light = {slug: cell_of(c, "light", "original") for slug, c in by_slug.items()}
    headline = []
    if best and control and light[best["slug"]] and light[control["slug"]]:
        b, c = light[best["slug"]], light[control["slug"]]
        headline.append(
            f"On the light class and the original questions, one agent using \"{best['name']}\" needed {tokens(b['tokens'])} "
            f"tokens and {b['turns']:.1f} turns for the three lookups, against {tokens(c['tokens'])} tokens and {c['turns']:.1f} "
            f"turns with no index: {1 - b['tokens'] / c['tokens']:.0%} fewer tokens.")
        headline.append(
            f"{of(b['perfect_runs'], b['runs'])} runs were fully correct with \"{best['name']}\" and "
            f"{of(c['perfect_runs'], c['runs'])} with no index. A run is fully correct when all three answers are.")
    # The floor: two turns, one to ask and one to answer. Said of the two configurations only when both reach it.
    if best and first and best is not first:
        both = {config["slug"]: [cell_of(config, cid, "original") for cid in ("light", "mid")] for config in (best, first)}
        if all(pt and pt["turns"] <= 2.05 for pts in both.values() for pt in pts):
            def spent(config: dict) -> str:
                pts = both[config["slug"]]
                return f"{tokens(pts[0]['tokens'])} tokens on the light class and {tokens(pts[1]['tokens'])} on the mid class"
            headline.append(
                "Two turns is the floor for this task: one to ask and one to answer. On the original questions "
                f"\"{best['name']}\" answered in 2 turns on both classes, with {spent(best)}; \"{first['name']}\" also "
                f"took 2 turns on both, with {spent(first)}.")
    repeated = [p for p in light.values() if p and p["runs"] >= 2]
    if len(repeated) >= 2:
        lean = min(repeated, key=lambda p: (p["turns"], p["tokens"]))
        heavy = max(repeated, key=lambda p: (p["turns"], p["tokens"]))
        headline.append(
            f"Tokens follow turns. On the light class and the original questions the leanest published configuration took "
            f"{lean['turns']:.1f} turns and {tokens(lean['tokens'])} tokens, the heaviest {heavy['turns']:.1f} turns and "
            f"{tokens(heavy['tokens'])} tokens.")
    if control:
        by_class = {pt["class"]: pt for pt in control["points"] if pt["set"] == "original"}
        shown = [(name, cid) for _, cid, name in PUBLIC_CLASSES if cid in by_class]
        if len(shown) >= 2:
            used = [by_class[cid]["tokens"] for _, cid in shown]
            def listed(items: list[str]) -> str:
                return items[0] if len(items) == 1 else ", ".join(items[:-1]) + " and " + items[-1]
            headline.append(
                "With no index, one agent used " + listed([f"{tokens(by_class[cid]['tokens'])} tokens on the {cid} class" for _, cid in shown])
                + " on the original questions; the answers were " + listed([share(by_class[cid]["correct"]) for _, cid in shown])
                + " correct." + (" A larger model class did not need far fewer tokens for the same lookups."
                                 if min(used) >= 0.7 * max(used) else ""))
    findings = {"headline": headline}

    # Held-out: who stayed fully correct on both sets, and the best configuration against the first generation.
    both_sets = [c for c in configs if {"original", "held-out"} <= set(c["sets"])]
    if both_sets:
        steady = [c for c in both_sets if all(pt["perfect_runs"] == pt["runs"] for pt in c["points"])]
        text = [(f"Of the {len(both_sets)} configurations measured on both question sets, {quoted([c['name'] for c in steady])} "
                 f"{'was' if len(steady) == 1 else 'were'} fully correct in every run." if steady
                 else f"None of the {len(both_sets)} configurations measured on both question sets was fully correct in every run.")]
        rows = []
        held = {config["slug"]: [cell_of(config, cid, "held-out") for cid in ("light", "mid")] for config in (best, first) if config}
        if best and first and best is not first and all(pt for pts in held.values() for pt in pts):
            def figures(pts: list[dict]) -> str:
                return (f"{tokens(pts[0]['tokens'])} tokens and {pts[0]['turns']:.1f} turns on the light class and "
                        f"{tokens(pts[1]['tokens'])} and {pts[1]['turns']:.1f} on the mid class")
            text.append(f"On the held-out questions \"{best['name']}\" needed {figures(held[best['slug']])}; "
                        f"\"{first['name']}\" needed {figures(held[first['slug']])}.")
            for label, config in (("Best", best), ("First generation", first)):
                for (_, cid, cname), pt in zip(PUBLIC_CLASSES[:2], held[config["slug"]]):
                    rows.append((f"{label}, {cid} class",
                                 f"{tokens(pt['tokens'])} tokens, {pt['turns']:.1f} turns, {of(pt['perfect_runs'], pt['runs'])} fully correct"))
        findings["held_out"] = {"title": "Held-out questions", "text": " ".join(text), "rows": rows,
                                "steady": [c["name"] for c in steady], "measured_on_both": len(both_sets)}

    # Parts: what search alone and graph alone do, on the callers question and in turns.
    search, graph, joint = by_arm.get("RS"), by_arm.get("RG"), by_arm.get("RGS")

    def callers_full(config: dict, cid: str) -> tuple[int, int]:
        runs = [c for c in cells if c["complete"] and c["setup"] == "single" and c["tier"] == TIER_OF_CLASS[cid]
                and owner.get(c["arm"]) == config["slug"]]
        return sum((c["scores"]["q1"] or 0) >= 0.999 for c in runs), len(runs)
    if search and graph and all(callers_full(c, cid)[1] for c in (search, graph) for cid in ("light", "mid")):
        said = {(c["slug"], cid): callers_full(c, cid) for c in (search, graph) for cid in ("light", "mid")}
        s_l, s_m, g_l, g_m = (said[(search["slug"], "light")], said[(search["slug"], "mid")],
                              said[(graph["slug"], "light")], said[(graph["slug"], "mid")])
        text = [f"Across both question sets, search alone listed every caller of a function in {of(*s_l)} light-class runs and "
                f"{of(*s_m)} mid-class runs; graph alone did in {of(*g_l)} and {of(*g_m)}."]
        rows = [("Search alone, every caller listed, light class", of(*s_l)), ("Search alone, every caller listed, mid class", of(*s_m)),
                ("Graph alone, every caller listed, light class", of(*g_l)), ("Graph alone, every caller listed, mid class", of(*g_m))]
        counts = {c["slug"]: cell_counts(c) for c in (search, graph)}
        if all(counts.values()):
            rows += [("Search alone, fully correct runs", of(*counts[search["slug"]][:2])),
                     ("Graph alone, fully correct runs", of(*counts[graph["slug"]][:2]))]
        if joint:
            turns = [(sid, cell_of(graph, "light", sid), cell_of(joint, "light", sid)) for sid in ("original", "held-out")]
            turns = [(sid, g, j) for sid, g, j in turns if g and j]
            if turns and all(g["turns"] > j["turns"] for _, g, j in turns):
                text.append("Graph alone took more turns on the light class: " + " and ".join(
                    f"{g['turns']:.1f} on the {sid} questions" for sid, g, j in turns) + ", against " + " and ".join(
                    f"{j['turns']:.1f}" for _, g, j in turns) + " for graph and search together.")
                rows += [(f"Graph alone, turns on the light class, {sid}", f"{g['turns']:.1f}") for sid, g, j in turns]
                rows += [(f"Graph and search, turns on the light class, {sid}", f"{j['turns']:.1f}") for sid, g, j in turns]
        findings["parts"] = {"title": "Parts", "text": " ".join(text), "rows": rows}

    # Memory: memory that already held the answers, and memory from an exploring session, cell by cell.
    text, rows = [], []
    answered = [by_arm[i] for i in ("RM", "RGSM") if i in by_arm]
    if joint and answered:
        cases = [(config, cid, cell_of(config, cid, "original"), cell_of(joint, cid, "original")) for config in answered
                 for cid in ("light", "mid")]
        if all(pt and ref for _, _, pt, ref in cases) and all(pt["turns"] <= 2.05 for _, _, pt, _ in cases):
            gaps = [pt["tokens"] - ref["tokens"] for _, _, pt, ref in cases]
            if min(gaps) >= 0:
                text.append(
                    f"Memory that already held the answers, filled by a session that answered the same questions, matched the "
                    f"two-turn floor and did not beat it: {quoted([c['name'] for c in answered])} took 2 turns on both classes "
                    f"and used {kilo(min(gaps))} to {kilo(max(gaps))} more tokens than graph and search without memory.")
    explored = by_arm.get("RGSO")
    if explored and joint:
        pairs = [(cell_of(explored, cid, sid), cell_of(joint, cid, sid)) for cid, sid in BEST_CELLS]
        if all(a and b for a, b in pairs):
            fewer, more = sum(a["tokens"] < b["tokens"] for a, b in pairs), sum(a["tokens"] > b["tokens"] for a, b in pairs)
            same = len(pairs) - fewer - more
            mine, theirs = cell_counts(explored), cell_counts(joint)
            text.append(
                "Memory filled by an earlier session that explored the repository without seeing any question, compared cell by "
                f"cell with graph and search without memory, used fewer tokens in {of(fewer, len(pairs))} cells and more in "
                f"{of(more, len(pairs))}" + (f" and the same in {of(same, len(pairs))}" if same else "")
                + f"; it was fully correct in {of(*mine[:2])} runs, against {of(*theirs[:2])} without memory.")
            rows += [("Exploring-session memory, fewer tokens than without memory", f"{of(fewer, len(pairs))} cells"),
                     ("Exploring-session memory, more tokens than without memory", f"{of(more, len(pairs))} cells"),
                     ("Exploring-session memory, fully correct runs", of(*mine[:2])),
                     ("Graph and search without memory, fully correct runs", of(*theirs[:2]))]
    if text:
        text.append("These single lookups are not the task memory is for, and tasks that span sessions were not measured.")
        findings["memory"] = {"title": "Memory", "text": " ".join(text), "rows": rows}

    # Speed: from the engine table, the quickest row against the slowest.
    timed = [r for r in engine if r["query_ms"] > 0]
    if len(timed) >= 2:
        fast, slow = min(timed, key=lambda r: r["query_ms"]), max(timed, key=lambda r: r["query_ms"])
        times = slow["query_ms"] / fast["query_ms"]
        findings["speed"] = {
            "title": "Speed",
            "text": (f"A query took {fast['query_ms']:g} ms with \"{fast['name']}\" and {slow['query_ms']:g} ms with \"{slow['name']}\", "
                     f"{times:.0f} times as long. Building the index took {fast['build_seconds']:g} s against "
                     f"{slow['build_seconds']:g} s, and the index is {fast['index_mb']:g} MB against {slow['index_mb']:g} MB. "
                     "Query times were measured on one machine on the same day"
                     + ("; the first generation's build time was measured in an earlier session." if "first generation" in slow["name"].lower() else ".")),
            "rows": [(f"{r['name']}, query time", f"{r['query_ms']:g} ms") for r in (fast, slow)]
                    + [(f"{r['name']}, build time", f"{r['build_seconds']:g} s") for r in (fast, slow)]
                    + [(f"{r['name']}, index size", f"{r['index_mb']:g} MB") for r in (fast, slow)]}

    # The proof on the Overview: fully correct runs across both question sets and both model classes.
    if best and first and control:
        counted = {"best": (best, cell_counts(best)), "first_generation": (first, cell_counts(first)),
                   "control": (control, cell_counts(control))}
        if all(found and found[2] == len(BEST_CELLS) for _, found in counted.values()):
            sums = {key: {"name": config["name"], "perfect_runs": found[0], "runs": found[1]} for key, (config, found) in counted.items()}
            findings["proof"] = dict(sums, text=(
                f"Across both question sets and both model classes, one agent using \"{best['name']}\" was fully correct in "
                f"{of(sums['best']['perfect_runs'], sums['best']['runs'])} runs, against "
                f"{of(sums['first_generation']['perfect_runs'], sums['first_generation']['runs'])} with \"{first['name']}\" and "
                f"{of(sums['control']['perfect_runs'], sums['control']['runs'])} with no index."))

    # Orchestration: a reviewing orchestrator on top of three light workers, against one light agent alone.
    def complete(setup: str, among: set | None = None) -> list[dict]:
        return [c for c in cells if c["complete"] and c["setup"] == setup and c["tier"] == "C"
                and c["arm"] not in hook_test and (among is None or c["arm"] in among)]

    orchestrated = complete("orch")
    alone = complete("single", {c["arm"] for c in orchestrated})
    ratios = [r["cost_over_single"] for r in orchestration["setups"] if r["tier"] == "C" and r["cost_over_single"]]
    if orchestrated and alone and ratios:
        right_orch = sum(c["quality"] >= 0.999 for c in orchestrated)
        right_alone = sum(c["quality"] >= 0.999 for c in alone)
        times = statistics.fmean(ratios)
        findings["orchestration"] = {
            "title": "Orchestration",
            "text": (f"With a reviewing orchestrator on top of three light workers, {of(right_orch, len(orchestrated))} runs "
                     f"ended fully correct, against {of(right_alone, len(alone))} for one light agent alone; a run cost "
                     f"about {times:.0f} times as much."),
            "orchestrated": {"perfect_runs": right_orch, "runs": len(orchestrated)},
            "alone": {"perfect_runs": right_alone, "runs": len(alone)}, "cost_multiple": times}
    effect = hook_effect(hooks, arms)
    if effect:
        first_part = ("Tool hooks that compress and redact reads lowered token use on these lookups"
                      if effect["saved"] else "Tool hooks that compress and redact reads did not lower token use on these lookups")
        second = ("without an index fewer runs ended fully correct" if effect["worse"]
                  else "without an index the share of fully correct runs did not fall")
        findings["hooks"] = {
            "title": "Tool hooks",
            "text": (f"{first_part}, and {second} ({of(effect['right_with'], effect['runs_with'])} runs with the hooks on, "
                     f"{of(effect['right_without'], effect['runs_without'])} with them off)."),
            "with_hooks": {"perfect_runs": effect["right_with"], "runs": effect["runs_with"]},
            "without_hooks": {"perfect_runs": effect["right_without"], "runs": effect["runs_without"]},
            "token_change": effect["change"], "spread": effect["spread"]}
    return findings


def session_facts(cells: list[dict], agents: list[dict], arms: dict, points: list[dict]) -> dict:
    """What the Study page says about the two sessions: the held-out runs, the start-up context of the three repeated
    configurations then and now, and the control's fully correct runs in each session. Counted from the runs."""
    by_id = {a["id"]: a for a in arms["arms"]}
    base_of = lambda a: a.get("base", a["id"])
    repeats = [a for a in arms["arms"] if a.get("group") == "rerun"]
    later = {a["id"] for a in repeats}
    earlier = {a["base"] for a in repeats}
    tiers = {a["tier"] for a in agents if a["arm"] in later}

    def start_of(ids: set) -> tuple[float, int]:
        found = [a["start"] for a in agents if a["arm"] in ids and a["setup"] == "single" and a["tier"] in tiers]
        return statistics.fmean(found), len(found)

    def perfect(arm_id: str) -> dict:
        found = {}
        for tier, cid, _ in PUBLIC_CLASSES:
            runs = [c for c in cells if c["complete"] and c["arm"] == arm_id and c["setup"] == "single" and c["tier"] == tier]
            if runs:
                found[cid] = [sum(c["quality"] >= 0.999 for c in runs), len(runs)]
        return found

    controls = [a for a in arms["arms"] if a.get("control")]
    today = next((a for a in controls if a.get("group") == "rerun"), None)
    first_session = by_id.get(today["base"]) if today else None
    held = next((a for a in arms["arms"] if a.get("group") == "heldout" and today and base_of(a) == base_of(today)), None)
    second_session = {a["id"] for a in arms["arms"] if a.get("group") in ("rust", "heldout", "rerun")}
    short = [p["n"] for p in points if p["arm"] in second_session and p["setup"] == "single"]
    facts = {"start_up": {"earlier": start_of(earlier)[0], "later": start_of(later)[0],
                          "earlier_agents": start_of(earlier)[1], "later_agents": start_of(later)[1],
                          "classes": [cid for tier, cid, _ in PUBLIC_CLASSES if tier in tiers]},
             "cells": {"second_session": len(short), "short": sum(n < 3 for n in short),
                       "fewest": min(short) if short else 0}}
    if today and first_session:
        facts["control"] = {"today": {"original": perfect(today["id"]), "held-out": perfect(held["id"]) if held else {}},
                            "earlier": perfect(first_session["id"])}
    return facts


def install_of() -> dict:
    """The public install block: benchmark/data/install.json "public" when it exists, otherwise a private preview."""
    path = BENCH / "data" / "install.json"
    install = (json.loads(path.read_text(encoding="utf-8")).get("public") if path.exists() else None) \
        or {"status": "private-preview"}
    if install.get("status") not in ("private-preview", "public"):
        raise SystemExit("install.json: public.status must be 'private-preview' or 'public'")
    if install["status"] == "public" and not isinstance(install.get("command"), str):
        raise SystemExit("install.json: a public install needs a 'command' string")
    return install


VIDEO_KEYS = ("src", "poster", "captions", "width", "height", "seconds", "title")
VIDEO_KINDS = ("ad", "explainer")   # in the order the page shows them: the ad first


def videos_of() -> list[dict]:
    """The public footage: benchmark/data/video.json "public" when that file exists, one film or a list of films;
    otherwise nothing, and then the page has no footage section. The files a film names must exist under
    docs/media/. A film says what it is with "kind"; one that does not is an explainer."""
    path = BENCH / "data" / "video.json"
    if not path.exists():
        return []
    public = json.loads(path.read_text(encoding="utf-8")).get("public")
    if not public:
        return []
    several = isinstance(public, list)
    films = []
    for n, video in enumerate(public if several else [public]):
        where = f"public[{n}]" if several else "public"
        if not isinstance(video, dict):
            raise SystemExit(f"video.json: {where} must be an object")
        missing = [key for key in VIDEO_KEYS if key not in video]
        if missing:
            raise SystemExit(f"video.json: {where} lacks {missing}")
        kind = video.get("kind", "explainer")
        if kind not in VIDEO_KINDS:
            raise SystemExit(f"video.json: {where}.kind must be one of {list(VIDEO_KINDS)}, got {kind!r}")
        for key in ("src", "poster", "captions"):
            name = str(video[key])
            if not name.startswith("media/") or ".." in name or not (DOCS / name).is_file():
                raise SystemExit(f"video.json: {where}.{key} must name an existing file under docs/media/, got {name!r}")
        films.append({"kind": kind, **{key: video[key] for key in VIDEO_KEYS}})
    return sorted(films, key=lambda film: VIDEO_KINDS.index(film["kind"]))


def video_of() -> dict | None:
    """The first film, for anything that knows only one."""
    films = videos_of()
    return films[0] if films else None


def public_of(points: list[dict], cells: list[dict], agents: list[dict], index: dict, orchestration: dict,
              hooks: dict, arms: dict) -> dict:
    """What the public site may show, computed from the same parsed results as the full report.
    Only the single-agent setup feeds the configurations; tier ids C, B, A become light, mid, top here, and the
    question sets A and B become original and held-out. A held-out arm joins the configuration of the public arm
    that shares its base: A0h joins A0r, RGSh joins RGS."""
    import re
    class_of = {tier: cid for tier, cid, _ in PUBLIC_CLASSES}
    class_order = [cid for _, cid, _ in PUBLIC_CLASSES]
    set_order = [s["id"] for s in PUBLIC_SETS]
    hook_test = {arm["id"] for arm in arms["arms"] if arm.get("group") == "hooks"}
    base_of = lambda a: a.get("base", a["id"])
    held = [a for a in arms["arms"] if a.get("group") == "heldout"]
    configs, owner, claimed, not_repeated = [], {}, set(), []
    for arm in sorted((a for a in arms["arms"] if "public" in a), key=lambda a: a["public"]["order"]):
        spec = arm["public"]
        if spec["family"] not in PUBLIC_FAMILIES:
            raise SystemExit(f"arms.json: {arm['id']} has public family {spec['family']!r}, expected one of {sorted(PUBLIC_FAMILIES)}")
        members = [arm] + [h for h in held if base_of(h) == base_of(arm)]
        rows = []
        for member in members:
            if member is not arm and member["id"] in claimed:
                raise SystemExit(f"arms.json: the held-out arm {member['id']} joins more than one public configuration")
            claimed.add(member["id"])
            owner[member["id"]] = spec["slug"]
            for tier, _, _ in PUBLIC_CLASSES:
                found = pick(points, member["id"], "single", tier)
                if found:
                    rows.append(public_point(found, class_of[tier]))
        keys = [(row["class"], row["set"]) for row in rows]
        if len(keys) != len(set(keys)):
            raise SystemExit(f"arms.json: {spec['slug']} has two measured points on one class and question set")
        if rows:
            rows.sort(key=lambda row: (set_order.index(row["set"]), class_order.index(row["class"])))
            configs.append({"slug": spec["slug"], "name": spec["name"], "family": spec["family"], "best": False,
                            "sets": [sid for sid in set_order if any(row["set"] == sid for row in rows)], "points": rows})
            if arm.get("group") not in ("rust", "rerun"):
                not_repeated.append(spec["name"])
    best, rule = best_config(configs)
    for config in configs:
        config["best"] = config["slug"] == best
    match = re.search(r"(\d+) Python files", index["dataset"])
    if not match:
        raise SystemExit("index.json: the dataset text no longer says how many Python files the repository has")
    complete = [c for c in cells if c["complete"]]
    per_cell = [p["n"] for p in points if p["arm"] not in hook_test]
    measured = {p["arm"] for p in points if p["arm"] not in hook_test}
    # A held-out or repeated arm is the same configuration as its base, so it is counted once.
    tested = {base_of(a) for a in arms["arms"] if a["id"] in measured}
    held_points = [pt for c in configs for pt in c["points"] if pt["set"] == "held-out"]
    engine = []
    engine_file = BENCH / "data" / "engine.json"
    if engine_file.exists():
        engine = json.loads(engine_file.read_text(encoding="utf-8")).get("public") or []
        for row in engine:
            missing = {"name", "build_seconds", "query_ms", "index_mb"} - set(row)
            if missing:
                raise SystemExit(f"engine.json: a public row lacks {sorted(missing)}")
        engine = [{key: row[key] for key in ("name", "build_seconds", "query_ms", "index_mb")} for row in engine]
    sessions = session_facts(cells, agents, arms, points)
    sessions["not_repeated"] = not_repeated
    public = {
        "built": datetime.date.today().isoformat(),
        "study": {
            "repository": {"language": "Python", "files": int(match.group(1)), "lines": PUBLIC_REPOSITORY_LINES},
            "questions": [dict(q) for q in PUBLIC_QUESTIONS],
            "setups": list(PUBLIC_SETUPS),
            "agents": len(agents), "runs": len(complete),
            "configurations_tested": len(tested),
            "configurations_published": len(configs),
            "runs_per_cell": {"min": min(per_cell), "max": max(per_cell)},
            "classes": [{"id": cid, "text": PUBLIC_CLASS_TEXT[cid]} for _, cid, _ in PUBLIC_CLASSES],
            "held_out": ({"configurations": len({c["slug"] for c in configs if "held-out" in c["sets"]}),
                          "classes": [cid for cid in class_order if any(pt["class"] == cid for pt in held_points)],
                          "runs_per_cell": {"min": min(pt["runs"] for pt in held_points), "max": max(pt["runs"] for pt in held_points)}}
                         if held_points else None),
            "sessions": sessions},
        "sets": [{"id": s["id"], "name": s["name"], "text": s["text"]} for s in PUBLIC_SETS],
        "classes": [{"id": cid, "name": name} for _, cid, name in PUBLIC_CLASSES],
        "families": [{"id": fid, "name": name} for fid, name in PUBLIC_FAMILIES.items()
                     if any(c["family"] == fid for c in configs)],
        "best_rule": rule,
        "configs": configs,
        "findings": public_findings(configs, cells, orchestration, hooks, arms, hook_test, owner, engine)}
    if engine:
        public["engine"] = engine
    public["install"] = install_of()
    films = videos_of()
    if films:
        public["videos"] = films
        public["video"] = films[0]   # the first film, for a page script that knows only one
    return public


def write_showcase_data(public: dict) -> int:
    folder = DOCS / "data"
    folder.mkdir(parents=True, exist_ok=True)
    # "</" would end a script block early if it ever appeared inside the data.
    body = json.dumps(rounded(public), ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/")
    path = folder / "showcase.js"
    path.write_text("// Generated. Do not edit by hand.\nwindow.SHOWCASE = " + body + ";\n", encoding="utf-8")
    return path.stat().st_size


# --- the text of the pages: what a reader sees with JavaScript switched off -------------------

def headline_of(public: dict) -> dict | None:
    """The best configuration and the control on the light class and the original questions, with their points."""
    def light(config):
        return cell_of(config, "light", "original")
    best = next((c for c in public["configs"] if c["best"]), None)
    control = next((c for c in public["configs"] if c["family"] == "control"), None)
    return {"best": best, "control": control, "b": light(best), "c": light(control)} if light(best) else None


def esc(text) -> str:
    import html
    return html.escape(str(text), quote=True)


def range_text(pt: dict) -> str:
    low, high = tokens(pt["tokens_low"]), tokens(pt["tokens_high"])
    return "" if low == high else f"{low} to {high}"


def rows_html(rows: list[tuple[str, str]]) -> str:
    items = "".join(f'\n      <li class="row"><span class="label">{esc(label)}</span><span class="value">{value}</span></li>'
                    for label, value in rows)
    return f'<ul class="rows">{items}\n    </ul>'


def text_rows_html(rows: list[tuple[str, str]]) -> str:
    items = "".join(f'\n      <li class="row stack"><span class="label">{esc(title)}</span>'
                    f'<span class="detail">{esc(text)}</span></li>' for title, text in rows)
    return f'<ul class="rows">{items}\n    </ul>'


def lede_slot(page: str, public: dict) -> str:
    study = public["study"]
    if page == "index.html":
        return (f"{BRAND} (PGR) answers where a function is defined and what calls it in one lookup. We ran "
                f"{study['agents']} agents on one Python repository and measured what each way of finding code "
                "costs in tokens and turns, and how often the answers were right.")
    if page == "benchmarks.html":
        return ("Tokens, turns and correct answers for each published configuration, measured on the same "
                "three lookups and on a held-out set of three more. Pick a model class and a question set to compare.")
    if page == "study.html":
        return "How the measurements were made, what they cover and what they leave out."
    return ("The package is in private preview." if public["install"]["status"] == "private-preview"
            else "The package is public. One command installs it.")


def figures_slot(public: dict) -> str:
    found = headline_of(public)
    if not found:
        return '<p class="note">No configuration has been run enough times on the light class to be compared yet.</p>'
    b, c = found["b"], found["c"]
    # Each figure carries what it counts from (the no-index value) and to (the best value) as attributes, so a
    # script can count the number up; the value in the page is always the final one.
    cards = [
        (tokens(b["tokens"]), "Tokens per task", f"No index: {tokens(c['tokens'])}, {1 - b['tokens'] / c['tokens']:.0%} fewer" if c else "",
         f' data-count="tokens" data-from="{c["tokens"]:.0f}" data-to="{b["tokens"]:.0f}"' if c else ""),
        (f"{b['turns']:.1f}", "Turns per task", f"No index: {c['turns']:.1f}" if c else "",
         f' data-count="turns" data-from="{c["turns"]:.4f}" data-to="{b["turns"]:.4f}"' if c else ""),
        (of(b["perfect_runs"], b["runs"]), "Fully correct runs", f"No index: {of(c['perfect_runs'], c['runs'])}" if c else "",
         f' data-count="of" data-from-text="{esc(of(c["perfect_runs"], c["runs"]))}"' if c else ""),
        (share(b["correct"]), "Answers correct", f"No index: {share(c['correct'])}" if c else "",
         f' data-count="share" data-from="{c["correct"]:.4f}" data-to="{b["correct"]:.4f}"' if c else "")]
    # Beside each number, the same pair drawn to one scale: no index above, the best configuration below.
    # The drawing repeats the note in the figure, so it is hidden from assistive technology.
    def pips(count: float, lit: int | None = None) -> str:
        count = max(1, min(12, round(count)))
        return "".join("<i></i>" if lit is None or n < lit else '<i class="off"></i>' for n in range(count))

    def ring(value: float) -> str:
        return ('<svg viewBox="0 0 26 26" focusable="false"><circle class="t" cx="13" cy="13" r="11"/>'
                f'<circle class="p" cx="13" cy="13" r="11" pathLength="1" style="--v:{value:.4f}"/></svg>')

    def pair(kind: str, ref: str, best: str) -> str:
        return (f'<dd class="meter {kind}" aria-hidden="true"><span class="m ref">{ref}</span>'
                f'<span class="m best">{best}</span></dd>')

    meters = ["", "", "", ""]
    if c:
        longest = max(b["tokens"], c["tokens"])
        meters = [pair("bars", f'<i style="--v:{c["tokens"] / longest:.4f}"></i>', f'<i style="--v:{b["tokens"] / longest:.4f}"></i>'),
                  pair("pips", pips(c["turns"]), pips(b["turns"])),
                  pair("pips", pips(c["runs"], c["perfect_runs"]), pips(b["runs"], b["perfect_runs"])),
                  pair("rings", ring(c["correct"]), ring(b["correct"]))]
    items = "".join(
        f'\n      <div class="figure spot{" lead" if n == 0 else ""}"><dt class="figure-label">{esc(label)}</dt>'
        f'<dd class="figure-value"{attrs}>{esc(value)}</dd>' + (f'<dd class="figure-note">{esc(note)}</dd>' if note else "")
        + meters[n] + "</div>"
        for n, (value, label, note, attrs) in enumerate(cards))
    group = ("Best configuration" + (" against no index" if c else "")
             + ", light model class, original questions, one agent answering three lookups")
    return f'<p class="group-title">{esc(group)}</p>\n    <dl class="figures card">{items}\n    </dl>' + proof_html(public)


def proof_html(public: dict) -> str:
    """The held-out result under the headline figures: fully correct runs across both question sets and both model
    classes, for the best configuration, the first-generation engine and no index."""
    proof = public["findings"].get("proof")
    if not proof:
        return ""
    return ('\n    <p class="group-title spaced">Fully correct runs on questions the builders never saw</p>\n    '
            f'<div class="card prose"><p class="proof-text">{esc(proof["text"])}</p></div>\n    '
            '<p class="footnote">Counted over four cells: the light and mid classes, on the original and the held-out questions.</p>')


def best_rule(rule: str | None = "cells") -> str:
    if rule == "light":
        return (f"The best configuration is chosen on the light class and the original questions, one agent, among those run at least "
                f"{BEST_MIN_RUNS} times: the highest share of fully correct runs, then the fewest tokens.")
    return (f"The best configuration is chosen among those run at least {BEST_MIN_RUNS} times in each of four cells, the original "
            "and the held-out questions on the light and mid classes: the highest share of its runs that were fully correct, "
            "and on a tie the lower mean tokens over the four cells.")


def best_slot(public: dict) -> str:
    found = headline_of(public)
    if not found:
        return ('<p class="note">The best configuration is named once one has been run at least '
                f'{BEST_MIN_RUNS} times on the light class and the original questions.</p>')
    best, pt = found["best"], found["b"]
    rows = [("Shown for", "Light class, original questions"),
            ("Tokens per task", esc(tokens(pt["tokens"])) + (f' <span class="sub">{esc(range_text(pt))}</span>' if range_text(pt) else "")),
            ("Turns per task", esc(f"{pt['turns']:.1f}")),
            ("Fully correct runs", esc(of(pt["perfect_runs"], pt["runs"]))),
            ("Answers correct", esc(share(pt["correct"]))),
            ("Runs", esc(pt["runs"]))]
    four = cell_counts(best)
    if four and four[2] == len(BEST_CELLS):
        rows.append(("Fully correct, all four cells", esc(of(four[0], four[1]))))
    return (f'<div class="card best-card spot tilt">\n    <div class="best-head"><h3 class="best-name">{esc(best["name"])}</h3>'
            f'<span class="tag">Best</span></div>\n    {rows_html(rows)}\n  </div>\n'
            f'  <p class="footnote">{esc(best_rule(public.get("best_rule")))}</p>')


def board_row(config: dict, pt: dict) -> str:
    sub = f'<span class="sub">{esc(range_text(pt))}</span>' if range_text(pt) else ""
    tag = ' <span class="tag">Best</span>' if config["best"] else ""
    return (f'\n        <tr><th scope="row" class="cfg"><span class="cfg-name">{esc(config["name"])}</span>{tag}</th>'
            f'<td data-label="Runs">{pt["runs"]}</td><td data-label="Tokens"><span class="v">{esc(tokens(pt["tokens"]))}{sub}</span></td>'
            f'<td data-label="Turns">{pt["turns"]:.1f}</td><td data-label="Answers correct">{esc(share(pt["correct"]))}</td>'
            f'<td data-label="Fully correct runs">{esc(of(pt["perfect_runs"], pt["runs"]))}</td></tr>')


def ordered(public: dict, class_id: str, set_id: str) -> list[tuple[dict, dict]]:
    """Configurations with a point on the class and the question set: the best first, then fewest tokens. A
    configuration not measured on the set is not in the list."""
    rows = [(c, pt) for c in public["configs"] for pt in c["points"] if pt["class"] == class_id and pt["set"] == set_id]
    return sorted(rows, key=lambda row: (not row[0]["best"], row[1]["tokens"]))


def views(public: dict) -> list[tuple[dict, dict]]:
    """Every pair of question set and model class, the sets in order and the classes in order within each."""
    return [(s, cls) for s in public["sets"] for cls in public["classes"]]


def view_title(cls: dict, qset: dict) -> str:
    return f"{cls['name']} class, {qset['name'].lower()} questions"


def empty_html(cls: dict, qset: dict) -> str:
    return (f'<p class="note empty">No configuration was measured on the {esc(cls["name"].lower())} class '
            f'with the {esc(qset["name"].lower())} questions.</p>')


def boards_slot(public: dict) -> str:
    out = []
    for qset, cls in views(public):
        rows = ordered(public, cls["id"], qset["id"])
        title = view_title(cls, qset)
        body = empty_html(cls, qset) if not rows else (
            '<div class="card">\n      <table class="board">\n        '
            f'<caption class="sr-only">{esc(title)}: runs, tokens, turns and correct answers by configuration</caption>\n        '
            '<thead><tr><th scope="col">Configuration</th><th scope="col">Runs</th><th scope="col">Tokens</th>'
            '<th scope="col">Turns</th><th scope="col">Answers correct</th><th scope="col">Fully correct runs</th></tr></thead>\n'
            '        <tbody>' + "".join(board_row(c, pt) for c, pt in rows) + '\n        </tbody>\n      </table>\n    </div>')
        out.append(f'<div class="panel board-panel{" on" if not out else ""}" data-class="{esc(cls["id"])}" data-set="{esc(qset["id"])}" '
                   f'role="group" aria-label="{esc(title)}, leaderboard">\n'
                   f'    <h3 class="panel-title">{esc(title)}</h3>\n'
                   '    <p class="group-title">One agent answering three lookups. The best configuration first, then fewest tokens.</p>\n'
                   f'    {body}\n  </div>')
    return "\n  ".join(out) + f'\n  <p class="footnote">{esc(best_rule(public.get("best_rule")))}</p>'


def charts_slot(public: dict) -> str:
    out = []
    families = {c["slug"]: c["family"] for c in public["configs"]}
    for qset, cls in views(public):
        rows = ordered(public, cls["id"], qset["id"])
        title = view_title(cls, qset)
        if not rows:
            inner = empty_html(cls, qset)
        else:
            lines = "".join(
                f'\n          <tr><th scope="row" class="cfg"><span class="cfg-name">{esc(c["name"])}</span></th>'
                f'<td data-label="Tokens">{esc(tokens(pt["tokens"]))}</td><td data-label="Answers correct">{esc(share(pt["correct"]))}</td>'
                f'<td data-label="Wall time">{esc(round(pt["seconds"]))} s</td></tr>' for c, pt in rows)
            key = "".join(f'<li><b>{i}</b><span>{esc(c["name"])}</span></li>' for i, (c, _) in enumerate(rows, 1))
            legend = ('<ul class="legend" aria-label="Series"></ul>\n      '
                      if len({families[c["slug"]] for c, _ in rows}) > 1 else "")
            inner = ('<div class="card chart-card">\n      ' + legend + '<div class="chart" data-chart="' + esc(cls["id"]) +
                     '" data-set="' + esc(qset["id"]) + '">'
                     '<p class="note chart-fallback">The chart needs JavaScript. The table view below has the same numbers.</p></div>\n      '
                     f'<ol class="chart-key" aria-label="Key to the numbers on the chart">{key}</ol>\n      '
                     '<details class="table-view"><summary>Table view</summary>\n        <table class="board board-small">\n          '
                     f'<caption class="sr-only">{esc(title)}: tokens, answers correct and wall time by configuration</caption>\n          '
                     '<thead><tr><th scope="col">Configuration</th><th scope="col">Tokens</th><th scope="col">Answers correct</th>'
                     '<th scope="col">Wall time</th></tr></thead>\n          <tbody>' + lines +
                     '\n          </tbody>\n        </table>\n      </details>\n    </div>')
        out.append(f'<div class="panel{" on" if not out else ""}" data-class="{esc(cls["id"])}" data-set="{esc(qset["id"])}" '
                   f'role="group" aria-label="{esc(title)}, chart">\n'
                   f'    <h3 class="panel-title">{esc(title)}</h3>\n    {inner}\n  </div>')
    return "\n  ".join(out)


def seg_slot(public: dict) -> str:
    """The two controls in one sticky bar: the model class and the question set. Both are radio groups of the same
    style. The line under them says what the held-out set is; a page without a script shows both lines."""
    def group(label: str, kind: str, items: list[tuple[str, str]]) -> str:
        buttons = "".join(
            f'\n      <button type="button" role="radio" aria-checked="{"true" if i == 0 else "false"}" '
            f'tabindex="{0 if i == 0 else -1}" data-value="{esc(value)}">{esc(name)}</button>'
            for i, (value, name) in enumerate(items))
        return (f'<div class="seg" role="radiogroup" aria-label="{esc(label)}" data-seg="{kind}" style="--count:{len(items)}">'
                f'<span class="seg-thumb" aria-hidden="true"></span>{buttons}\n    </div>')
    notes = "".join(f'<span data-set="{esc(s["id"])}">{esc(s["name"])} questions: {esc(s["text"][0].lower() + s["text"][1:])}</span> '
                    for s in public["sets"])
    return ('<div class="seg-bar">\n    ' + group("Model class", "class", [(c["id"], c["name"]) for c in public["classes"]]) + '\n    '
            + group("Questions", "set", [(s["id"], s["name"]) for s in public["sets"]]) + '\n  </div>\n'
            f'  <p class="group-title seg-note" id="set-note">{notes.rstrip()}</p>\n'
            '  <p class="sr-only" id="seg-status" aria-live="polite"></p>')


def engine_slot(public: dict) -> str:
    rows = public.get("engine")
    if not rows:
        return ""
    lines = "".join(
        f'\n          <tr><th scope="row" class="cfg"><span class="cfg-name">{esc(r["name"])}</span></th>'
        f'<td data-label="Build time">{esc(format(r["build_seconds"], "g"))} s</td>'
        f'<td data-label="Query time">{esc(format(r["query_ms"], "g"))} ms</td>'
        f'<td data-label="Index size">{esc(format(r["index_mb"], "g"))} MB</td></tr>' for r in rows)
    # The race: one bar for each row, as long as its time to answer a query, all to one scale. Written finished;
    # a script runs each bar for its measured time, so the one that answers first also stops first.
    timed = sorted((r for r in rows if r["query_ms"] > 0), key=lambda r: r["query_ms"])
    race = ""
    if len(timed) > 1:
        slowest, fastest = timed[-1], timed[0]
        bars = "".join(
            f'\n    <div class="race-row{" fast" if r is fastest else ""}" style="--v:{r["query_ms"] / slowest["query_ms"]:.4f}" data-ms="{r["query_ms"]:g}">'
            f'<p class="race-name">{esc(r["name"])}</p><div class="race-track" aria-hidden="true"><i></i></div>'
            f'<p class="race-val"><b>{esc(format(r["query_ms"], "g"))}</b> ms</p></div>' for r in timed)
        times = slowest["query_ms"] / fastest["query_ms"]
        callout = (f'{fastest["name"]} answers a query in {format(fastest["query_ms"], "g")} ms and {slowest["name"]} in '
                   f'{format(slowest["query_ms"], "g")} ms, {times:.1f} times as long.')
        race = ('<p class="group-title">Time to answer one query. A bar is as long as its time, and every bar is drawn to one scale.</p>\n  '
                '<figure class="card race" id="race" aria-labelledby="race-cap">' + bars +
                f'\n    <figcaption id="race-cap" class="race-callout">{esc(callout)}</figcaption>\n    '
                '<p class="race-status" hidden>Motion is off. Each bar is shown finished.</p>\n    '
                '<div class="race-bar"><button type="button" class="btn quiet replay">Replay</button></div>\n  </figure>\n  ')
    return ('<section class="section" aria-labelledby="engine">\n  <h2 id="engine" class="section-title">Engine speed</h2>\n  ' + race +
            f'<p class="group-title{" spaced" if race else ""}">Time to build the index, time to answer a query and size on disk.</p>\n  '
            '<div class="card">\n    <table class="board board-small">\n      <caption class="sr-only">Engine speed by configuration</caption>\n      '
            '<thead><tr><th scope="col">Configuration</th><th scope="col">Build time</th><th scope="col">Query time</th>'
            '<th scope="col">Index size</th></tr></thead>\n      <tbody>' + lines + '\n      </tbody>\n    </table>\n  </div>\n</section>\n')


def findings_slot(public: dict) -> str:
    found, cards = public["findings"], []
    # The findings the second session added carry their own rows; the two older ones are laid out from their numbers.
    for key in ("held_out", "parts", "memory", "speed"):
        if key in found:
            card = found[key]
            rows = rows_html([(a, esc(b)) for a, b in card["rows"]]) if card.get("rows") else ""
            cards.append(f'<div class="card finding">\n    <h3 class="finding-title">{esc(card["title"])}</h3>\n'
                         f'    <p class="finding-text">{esc(card["text"])}</p>' + (f'\n    {rows}' if rows else "") + '\n  </div>')
    for key, rows in (("orchestration", lambda f: [
            ("Fully correct, orchestrated", of(f["orchestrated"]["perfect_runs"], f["orchestrated"]["runs"])),
            ("Fully correct, one light agent", of(f["alone"]["perfect_runs"], f["alone"]["runs"])),
            ("Cost of one run", f"about {f['cost_multiple']:.0f} times")]),
                      ("hooks", lambda f: [
            ("Fully correct, hooks on", of(f["with_hooks"]["perfect_runs"], f["with_hooks"]["runs"])),
            ("Fully correct, hooks off", of(f["without_hooks"]["perfect_runs"], f["without_hooks"]["runs"])),
            ("Change in token use", f"{f['token_change']:+.0%}")])):
        if key in found:
            cards.append(f'<div class="card finding">\n    <h3 class="finding-title">{esc(found[key]["title"])}</h3>\n'
                         f'    <p class="finding-text">{esc(found[key]["text"])}</p>\n    {rows_html([(a, esc(b)) for a, b in rows(found[key])])}\n  </div>')
    return "\n  ".join(cards)


def headline_slot(public: dict) -> str:
    return "".join(f"\n      <li>{esc(line)}</li>" for line in public["findings"]["headline"]) + "\n    "


def pair_of(pair: list[int]) -> str:
    return of(pair[0], pair[1])


NUMBER_WORDS = ("zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve")


def number_word(n: int, start: bool = False) -> str:
    """A small count as a word, so a sentence never begins with a digit."""
    word = NUMBER_WORDS[n] if 0 <= n < len(NUMBER_WORDS) else str(n)
    return word.capitalize() if start else word


def study_slots(public: dict) -> dict:
    study = public["study"]
    repo = study["repository"]
    sessions, held, sets = study["sessions"], study["held_out"], public["sets"]
    per_set = len(study["questions"])
    scale = [("Repository", f"{repo['language']}, {repo['files']} files, about {repo['lines']:,} lines"),
             ("Questions", f"{per_set} in each of {len(sets)} sets, each asked in every run on its set"),
             ("Agents", f"{study['agents']:,}"), ("Runs", f"{study['runs']:,}"),
             ("Configurations tested", f"{study['configurations_tested']}, of which {study['configurations_published']} are published here"),
             ("Runs per configuration and class", f"{study['runs_per_cell']['min']} to {study['runs_per_cell']['max']}")]
    names = {c["id"]: c["name"] for c in public["classes"]}
    limits = [
        ("One repository", f"Every result comes from one Python repository of {repo['files']} files. Other repositories may behave differently."),
        ("One language", "Python only. Nothing here says how the configurations do on other languages."),
        ("Six questions", f"{number_word(per_set * len(sets), True)} lookup questions of modest difficulty, in {number_word(len(sets))} "
                          f"sets of {number_word(per_set)}. Harder questions may separate the configurations more, or less."),
        ("Small run counts",
         f"Each configuration and class was run between {study['runs_per_cell']['min']} and {study['runs_per_cell']['max']} times. "
         "A cell with one run is an example, not an average, and a difference of one turn is inside the noise.")]
    cells = sessions["cells"]
    if cells["second_session"]:
        limits.append(("Three runs per cell",
                       "The second session ran each cell three times"
                       + (f", and {number_word(cells['short'])} of its {cells['second_session']} cells hold {number_word(cells['fewest'])} complete runs"
                          if cells["short"] else "")
                       + ". With three runs, a difference of one fully correct run is inside the noise."))
    limits += [
        ("Machine load", "Wall time depends on what else the test machine was doing. Token counts do not."),
        ("Cost figures", "Where a run is said to cost a multiple of another, the cost is list prices applied to the measured tokens.")]

    # The question sets: what the held-out set is and why it exists.
    set_rows = [(f"{sets[0]['name']} questions",
                 "The three questions above. Every round has used them, and the people who built the engine saw them while building it, "
                 "so a result on them could reflect tuning to them.")]
    if held:
        set_rows.append((f"{sets[1]['name']} questions",
                         "Three more lookups of the same three shapes, written after the engine was built and never seen by the people or "
                         "agents who built it. They exist to check that the engine was not fitted to the first set. "
                         f"They were run on {number_word(held['configurations'])} configurations, on the {' and '.join(held['classes'])} "
                         f"{'class' if len(held['classes']) == 1 else 'classes'}, with {held['runs_per_cell']['min']} to "
                         f"{held['runs_per_cell']['max']} runs in each cell."))

    # The two sessions, the start-up context, the memory and the control.
    start = sessions["start_up"]
    session_rows = []
    session_rows.append((
        "Two sessions",
        "The results come from two sessions a few days apart. The first measured the earlier configurations. The second measured the "
        "second-generation engine and the held-out questions, and repeated three earlier configurations: no index and two builds of the "
        "first-generation engine. The leaderboard shows the repeats, so every configuration compared with the engine was measured on "
        "the same day. The earlier runs of those three are not shown here."
        + (f" {number_word(len(sessions['not_repeated']), True)} configurations from the first session were not repeated and keep "
           "their first-session figures: " + "; ".join(sessions["not_repeated"]) + "." if sessions["not_repeated"] else "")))
    session_rows.append((
        "Start-up context",
        f"An agent's start-up context, the text it reads before it sees a question, grew between the sessions. On the "
        f"{' and '.join(start['classes'])} {'class' if len(start['classes']) == 1 else 'classes'} it averaged {kilo(start['earlier'])} tokens "
        f"in the earlier runs of the three repeated configurations ({start['earlier_agents']} agents) and {kilo(start['later'])} in their "
        f"repeats ({start['later_agents']} agents). Every turn reads it again, so the same configuration costs more tokens now than it did "
        "then. That is why the leaderboard compares runs made on the same day."))
    session_rows.append((
        "How the memory was filled",
        "Some configurations use a memory of earlier sessions. Before the runs, the memory was filled by one of two earlier sessions: one "
        "that answered the same questions the runs ask, and one that explored the repository without seeing any question. The first kind "
        "is a best case, because the memory already holds the answers. Neither is a test of the task memory is for: work that spans "
        "sessions was not measured."))
    control = sessions.get("control")
    if control and control["today"]["original"] and control["earlier"]:
        today, earlier = control["today"], control["earlier"]

        def per_class(counts: dict, named: bool = True) -> str:
            shown = [f"{pair_of(counts[cid])}" + (f" {cid}-class runs" if named else "") for cid in names if cid in counts]
            return shown[0] if len(shown) == 1 else ", ".join(shown[:-1]) + " and " + shown[-1]
        session_rows.append((
            "The control",
            "On the test machine, the control's file reads pass through a read-compression tool, which hid lines the control needed in some "
            f"runs. In the second session the control was fully correct in {per_class(today['original'])} on the original questions"
            + (f", and in {per_class(today['held-out'])} on the held-out questions" if today["held-out"] else "")
            + f". In the first session, which had no such tool, it was fully correct in {per_class(earlier)}."))
    return {
        "questions": text_rows_html([(q["title"], q["text"]) for q in study["questions"]]),
        "sets": text_rows_html(set_rows),
        "setups": text_rows_html([(f"Setup {i + 1}", text) for i, text in enumerate(study["setups"])]),
        "scale": rows_html([(label, esc(value)) for label, value in scale]),
        "classes": text_rows_html([(names[c["id"]], c["text"]) for c in study["classes"]]),
        "sessions": text_rows_html(session_rows),
        "limits": text_rows_html(limits)}


def install_slot(public: dict) -> str:
    install = public["install"]
    gives = rows_html([("Installation", "One command"), ("What is installed", "One binary"),
                       ("Works with", "Terminal coding agents"), ("Platforms", "Windows, macOS and Linux")])
    if install["status"] == "public":
        extra = [("Version", esc(install["version"]))] if isinstance(install.get("version"), str) else []
        return ('<section class="section" aria-labelledby="command">\n  <h2 id="command" class="section-title">Install command</h2>\n  '
                '<div class="card install-card">\n    <div class="code-wrap"><pre class="code" id="install-command" tabindex="0">'
                f'<code>{esc(install["command"])}</code></pre></div>\n    <div class="copy-row"></div>\n    '
                + (rows_html(extra) + "\n  " if extra else "") + '</div>\n</section>\n'
                '<section class="section" aria-labelledby="what-you-get">\n  <h2 id="what-you-get" class="section-title">What installing gives you</h2>\n  '
                f'<div class="card">\n    {gives}\n  </div>\n</section>\n')
    return ('<section class="section" aria-labelledby="availability">\n  <h2 id="availability" class="section-title">Availability</h2>\n  '
            '<div class="card">\n    '
            + rows_html([("Status", '<span class="tag">Private preview</span>'), ("Public registry", "Not yet")]) +
            '\n  </div>\n  <p class="footnote">The package is in private preview and is not yet on a public registry.</p>\n</section>\n'
            '<section class="section" aria-labelledby="what-you-get">\n  <h2 id="what-you-get" class="section-title">What installing will give you</h2>\n  '
            f'<div class="card">\n    {gives}\n  </div>\n</section>\n')


# The mark: a P and the token it hands over, one colour, drawn on a 24 unit grid. The tab bar uses it as the icon of Overview.
MARK_PATHS = '<path d="M7 4v16M7 4h6.5a4.25 4.25 0 0 1 0 8.5H7"/><circle cx="18.5" cy="18.6" r="1.9"/>'
MARK_SVG = f'<svg class="logo" viewBox="0 0 24 24" aria-hidden="true" focusable="false">{MARK_PATHS}</svg>'
NAV_ICONS = {
    "index.html": MARK_PATHS,
    "benchmarks.html": '<path d="M5 20v-8M12 20V5M19 20v-5"/>',
    "study.html": '<path d="M5 5.5A1.5 1.5 0 0 1 6.5 4H18v13H6.5A1.5 1.5 0 0 0 5 18.5zM5 18.5A1.5 1.5 0 0 0 6.5 20H18v-3"/>',
    "install.html": '<path d="M12 4v11M7.5 10.5 12 15l4.5-4.5M5 20h14"/>',
}


def nav_slots(name: str, page: str) -> dict:
    """The sidebar and the tab bar: plain links, the current page marked, and under it the sections
    of this page read from its own headings, so a renamed section cannot leave the list behind."""
    import html
    import re
    sections = "".join(
        f'\n          <li><a href="#{m.group(1)}">{html.unescape(re.sub(r"<[^>]+>", "", m.group(2))).strip()}</a></li>'
        for m in re.finditer(r'<h2 id="([^"]+)"[^>]*>(.*?)</h2>', page, re.S))
    side, tabs, at = [], [], 0
    for n, (file, title) in enumerate(PUBLIC_PAGES):
        here = ' aria-current="page"' if file == name else ""
        at = n if here else at
        inner = f'\n        <ul class="side-sections" aria-label="On this page">{sections}\n        </ul>' if here and sections else ""
        side.append(f'\n        <li><a href="{file}"{here}>{title}</a>{inner}</li>')
        tabs.append(f'\n    <a href="{file}"{here}><svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">{NAV_ICONS[file]}</svg>'
                    f'<span>{title}</span></a>')
    return {"sidebar": ('<nav class="sidebar" aria-label="Pages">\n    <p class="brand">' + MARK_SVG + f'<span>{BRAND}</span></p>\n'
                        f'    <div class="side-wrap">\n      <span class="side-ind" aria-hidden="true" style="--i:{at}"></span>\n'
                        '      <ul class="side-pages">' + "".join(side) + "\n      </ul>\n    </div>\n  </nav>"),
            "tabbar": (f'<nav class="tabbar" aria-label="Pages">\n    <span class="tab-ind" aria-hidden="true" style="--n:{len(PUBLIC_PAGES)};--i:{at}"></span>'
                       + "".join(tabs) + "\n  </nav>")}


# --- the Overview streams, the footage and the turn runners -------------------------------------

def nice_step(longest_k: float) -> int:
    """A tick step, in thousands of tokens, that puts three or four ticks along the longest lane."""
    for step in (10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 5000):
        if longest_k / step <= 4.5:
            return step
    return 10000


def hero_slot(public: dict) -> str:
    """Two lanes drawn to one scale, the longer being the no-index run: lane length is tokens per task. The lanes,
    their values and the difference are plain markup; a script lays a canvas of particles over them."""
    found = headline_of(public)
    if not found or not found["c"]:
        return ""
    b, c = found["b"], found["c"]
    longest = max(b["tokens"], c["tokens"])

    def width(pt: dict) -> str:
        return f"{pt['tokens'] / longest * 100:.2f}".rstrip("0").rstrip(".")

    def lane(kind: str, name: str, pt: dict, extra: str = "") -> str:
        return (f'\n    <div class="lane lane-{kind}" style="--w:{width(pt)}%">'
                f'<p class="lane-label"><span class="lane-name">{esc(name)}</span>'
                f'<span class="lane-val"><b data-tok>{esc(tokens(pt["tokens"]))}</b> tokens, <b data-turns>{pt["turns"]:.1f}</b> turns</span></p>'
                f'<div class="lane-track"></div>{extra}</div>')

    saved = c["tokens"] - b["tokens"]
    gap = (f'<div class="gap"><span>{esc(tokens(saved))} fewer tokens, {saved / c["tokens"]:.0%}</span></div>' if saved > 0 else "")
    step = nice_step(longest / 1e3) * 1000
    marks = [(k * step, "0" if k == 0 else tokens(k * step)) for k in range(int(longest * 0.93 // step) + 1)] + [(longest, tokens(longest))]
    scale = "".join(f'<span style="--t:{value / longest * 100:.2f}" data-v="{value:.0f}">{esc(text)}</span>' for value, text in marks)
    spoken = (f"No index: {tokens(c['tokens'])} tokens and {c['turns']:.1f} turns. Best configuration: {tokens(b['tokens'])} tokens and "
              f"{b['turns']:.1f} turns." + (f" That is {tokens(saved)} fewer tokens, {saved / c['tokens']:.0%}." if saved > 0 else ""))
    return ('<figure class="card stream" id="stream" aria-labelledby="stream-title">\n  '
            '<p class="stream-title" id="stream-title">Tokens per task, light model class, original questions, one agent answering three lookups</p>\n  '
            '<div class="lanes">' + lane("ref", "No index", c) + lane("best", found["best"]["name"], b, gap) + '\n  </div>\n  '
            f'<div class="scale" aria-hidden="true">{scale}</div>\n  '
            '<p class="stream-key">One particle is 1k tokens. Lane length is tokens per task. Stream speed is turns per task.</p>\n  '
            f'<p class="sr-only">{esc(spoken)}</p>\n</figure>')


def stage_slot(public: dict) -> str:
    """What the particle stage of the Overview reads out: the two streams by name with their tokens and turns, the
    count that falls from the no-index value to the best one, and the difference. A script draws the streams and
    drives these; a page without the stage shows the lanes of hero_slot, which carry the same numbers."""
    found = headline_of(public)
    if not found or not found["c"]:
        return ""
    b, c = found["b"], found["c"]

    def tag(kind: str, name: str, pt: dict) -> str:
        return (f'\n      <p class="body-tag tag-{kind}"><span class="bt-name">{esc(name)}</span>'
                f'<span class="bt-val"><b>{esc(tokens(pt["tokens"]))}</b> tokens, <b>{pt["turns"]:.1f}</b> turns</span></p>')

    saved = c["tokens"] - b["tokens"]
    delta = f'{tokens(saved)} fewer tokens, {saved / c["tokens"]:.0%}' if saved > 0 else ""
    spoken = (f"No index: {tokens(c['tokens'])} tokens and {c['turns']:.1f} turns. Best configuration, {found['best']['name']}: "
              f"{tokens(b['tokens'])} tokens and {b['turns']:.1f} turns." + (f" That is {delta}." if delta else ""))
    return ('<div class="hero-read">\n    '
            f'<p class="sr-only">{esc(spoken)}</p>\n    '
            '<div class="hero-vis" aria-hidden="true">' + tag("ref", "No index", c) + tag("best", found["best"]["name"], b) +
            '\n      <div class="hero-count">\n        <p class="hero-count-label">Tokens per task</p>\n        '
            f'<p class="hero-num" data-from="{c["tokens"]:.0f}" data-to="{b["tokens"]:.0f}">{esc(tokens(c["tokens"]))}</p>\n        '
            f'<p class="hero-state"><span class="st-ref">No index</span><span class="st-best">{esc(found["best"]["name"])}</span></p>\n        '
            + (f'<p class="hero-delta">{esc(delta)}</p>\n      ' if delta else "      ") + '</div>\n      '
            '<p class="hero-key">One particle is <b data-per>10</b> tokens. The thickness of a stream is its tokens per task.</p>\n      '
            f'<svg class="logo hero-mark" viewBox="0 0 24 24" focusable="false">{MARK_PATHS}</svg>\n      '
            f'<p class="hero-word">{BRAND}</p>\n    </div>\n  </div>')


def problem_slot(public: dict) -> str:
    """The first chapter of the Overview: a task as a stack of turns. One block is one turn at the average tokens a
    turn, so a stack is as tall as its tokens per task, and both stacks are drawn to one scale."""
    found = headline_of(public)
    if not found or not found["c"]:
        return ""
    b, c = found["b"], found["c"]
    longest = max(b["tokens"], c["tokens"])

    def stack(kind: str, name: str, pt: dict) -> str:
        count = max(1, min(12, -int(-pt["turns"] // 1)))           # whole turns, the last one possibly a part
        per = pt["tokens"] / pt["turns"]
        parts = [min(1.0, pt["turns"] - i) for i in range(count)]
        slabs = "".join(f'<i class="slab" style="--h:{per * part / longest * 100:.3f}%;--i:{i}"><span>{i + 1}</span></i>'
                        for i, part in enumerate(parts))
        return (f'\n      <div class="stack {kind}" style="--n:{count}">'
                f'<p class="stack-total"><b data-od="tokens" data-to="{pt["tokens"]:.0f}">{esc(tokens(pt["tokens"]))}</b> tokens</p>'
                f'<div class="stack-col">{slabs}</div>'
                f'<p class="stack-name">{esc(name)}<span>{pt["turns"]:.1f} turns, about {esc(tokens(per))} a turn</span></p></div>')

    return ('<section class="chapter ch-problem pin" aria-labelledby="problem">\n  <div class="pin-stage">\n    '
            '<div class="ch-copy">\n      <h2 id="problem" class="kin" aria-label="Every turn re-reads everything">'
            '<span class="kl" aria-hidden="true"><span>Every turn</span></span> <span class="kl" aria-hidden="true"><span>re-reads</span></span> '
            '<span class="kl" aria-hidden="true"><span>everything</span></span></h2>\n      '
            '<p class="ch-lede">An agent works in turns, and on each turn it reads its whole conversation again. '
            'Every extra turn pays for the same context one more time.</p>\n    </div>\n    '
            '<figure class="stacks" aria-labelledby="stacks-cap">' + stack("ref", "No index", c) + stack("best", found["best"]["name"], b) +
            '\n      <figcaption id="stacks-cap">One block is one turn, at the average tokens a turn. Both stacks are drawn to one scale. '
            'Light model class, original questions, one agent answering three lookups.</figcaption>\n    </figure>\n  </div>\n</section>')


def change_slot(public: dict) -> str:
    """The second chapter: the path of one task, one node for each turn. The path of the best configuration is drawn
    beside the path with no index."""
    found = headline_of(public)
    if not found or not found["c"]:
        return ""
    b, c = found["b"], found["c"]

    def hop(kind: str, name: str, pt: dict) -> str:
        count = max(2, min(12, round(pt["turns"])))
        xs = [12 + 176 * i / (count - 1) for i in range(count)]
        ys = [30.0] * count if count == 2 else [14.0 if i % 2 == 0 else 46.0 for i in range(count)]
        line = " ".join(f"{x:.1f},{y:.0f}" for x, y in zip(xs, ys))
        nodes = "".join(f'<circle cx="{x:.1f}" cy="{y:.0f}" r="4" style="--i:{i}"/>' for i, (x, y) in enumerate(zip(xs, ys)))
        return (f'\n      <div class="hop {kind}"><svg viewBox="0 0 200 60" style="--c:{count}" aria-hidden="true" focusable="false">'
                f'<polyline points="{line}" pathLength="1"/>{nodes}</svg>'
                f'<p class="hop-val"><b>{pt["turns"]:.1f}</b> turns<span>{esc(name)}</span></p></div>')

    floor = ("Two turns is the floor for a lookup: one to ask and one to answer. " if b["turns"] < 2.05 else "")
    return ('<section class="chapter ch-change" aria-labelledby="change">\n  '
            '<h2 id="change" class="kin wide" aria-label="One question, one answer">'
            '<span class="kl" aria-hidden="true"><span>One question,</span></span> <span class="kl" aria-hidden="true"><span>one answer</span></span></h2>\n  '
            '<div class="change-grid">\n    <figure class="hops" aria-labelledby="hops-cap">' + hop("ref", "No index", c) + hop("best", found["best"]["name"], b) +
            '\n      <figcaption id="hops-cap">One node is one turn of a task, from the question to the answer.</figcaption>\n    </figure>\n    '
            f'<p class="ch-lede">{floor}The best configuration took {b["turns"]:.1f} turns per task on the light class and the original questions. '
            f'No index took {c["turns"]:.1f}.</p>\n  </div>\n</section>')


VIDEO_TYPES = {"mp4": "video/mp4", "webm": "video/webm"}
VIDEO_KIND_TEXT = {"ad": "Ad", "explainer": "Explainer"}


def video_slot(public: dict) -> str:
    """The footage section: present only when the data holds a film, and then one framed player for each, the ad
    first. Each is a native player that never starts by itself; a script replaces its controls with the page's own."""
    films = public.get("videos") or ([public["video"]] if public.get("video") else [])
    if not films:
        return ""

    def player(n: int, video: dict) -> str:
        kind = video.get("kind", "explainer")
        mime = VIDEO_TYPES.get(str(video["src"]).rsplit(".", 1)[-1].lower())
        source = f'<source src="{esc(video["src"])}"' + (f' type="{mime}"' if mime else "") + ">"
        width, height = int(video["width"]), int(video["height"])
        return (f'\n    <figure class="cine{" lead" if n == 0 else ""}" data-kind="{esc(kind)}">\n      '
                f'<div class="cine-frame" style="aspect-ratio:{width}/{height}">\n        '
                f'<video controls playsinline preload="none" width="{width}" height="{height}" poster="{esc(video["poster"])}" '
                f'aria-label="{esc(video["title"])}">\n          '
                f'{source}\n          <track kind="captions" src="{esc(video["captions"])}" srclang="en" label="English" default>\n          '
                '<p>This browser cannot play the video.</p>\n        </video>\n      </div>\n      '
                f'<figcaption class="cine-cap"><span class="cine-title">{esc(video["title"])}</span>'
                f'<span class="cine-meta">{VIDEO_KIND_TEXT[kind]}, {esc(format(video["seconds"], "g"))} seconds, with captions</span></figcaption>\n    </figure>')

    return ('<section class="section cinema" aria-labelledby="films">\n  '
            f'<h2 id="films" class="section-title">{"Films" if len(films) > 1 else "Film"}</h2>\n  '
            f'<div class="cine-grid{" multi" if len(films) > 1 else ""}">' + "".join(player(n, video) for n, video in enumerate(films)) +
            '\n  </div>\n</section>\n')


def runners_slot(public: dict) -> str:
    """The one conceptual figure: a dot for each turn on a track for no index and for the best configuration of the
    model class and question set chosen above (the one with the fewest tokens where the best was not measured
    there). One panel for each pair of class and set, written finished, with every dot parked at the end of its
    run; a script shows the chosen panel and plays its runs once when the figure comes into view."""
    def run(name: str, pt: dict, kind: str) -> str:
        n = max(1, min(12, round(pt["turns"])))
        dots = "".join(f'<i class="dot" style="--i:{i};--k:{n - 1 - i}">{i + 1}</i>' for i in range(n))
        return (f'\n      <div class="runner {kind}"><p class="runner-label"><span class="runner-name">{esc(name)}</span>'
                f'<span class="runner-val">{pt["turns"]:.1f} turns</span></p>'
                f'<div class="runner-track" aria-hidden="true"><span class="start"></span><span class="end"></span>{dots}</div></div>')

    panels = []
    for qset, cls in views(public):
        rows = ordered(public, cls["id"], qset["id"])
        title = view_title(cls, qset)
        control = next((pt for c, pt in rows if c["family"] == "control"), None)
        other = next(((c, pt) for c, pt in rows if c["family"] != "control"), None)
        if not rows:
            inner = empty_html(cls, qset)
        elif not control or not other:
            inner = (f'<p class="note empty">No index and one other configuration were not both measured on the '
                     f'{esc(cls["name"].lower())} class with the {esc(qset["name"].lower())} questions.</p>')
        else:
            config, pt = other
            who = "the best configuration" if config["best"] else config["name"]
            fewer = control["turns"] - pt["turns"]
            callout = (f"{fewer:.1f} fewer turns per task with {who}, so its run ends first." if fewer > 0
                       else f"No index took {control['turns']:.1f} turns and {who} {pt['turns']:.1f}.")
            inner = (run("No index", control, "ref") + run(config["name"], pt, "best") +
                     '\n      <p class="runner-axis" aria-hidden="true"><span>Question asked</span><span>Answer found</span></p>\n      '
                     f'<p class="runner-callout">{esc(callout[0].upper() + callout[1:])}</p>')
        panels.append(f'\n    <div class="panel{" on" if not panels else ""}" data-class="{esc(cls["id"])}" data-set="{esc(qset["id"])}" '
                      f'role="group" aria-label="{esc(title)}, turns">\n      <h3 class="panel-title">{esc(title)}</h3>{inner}\n    </div>')
    return ('<section class="section" aria-labelledby="turns">\n  <h2 id="turns" class="section-title">Turns per task</h2>\n  '
            '<p class="group-title">One dot is one turn, and a task ends when its last turn ends. One agent answering three lookups, '
            'for the model class and question set chosen above.</p>\n  '
            '<figure class="card runners" id="runners">' + "".join(panels) +
            '\n    <p class="runner-status" hidden>Motion is off. Each run is shown finished.</p>\n    '
            '<div class="runners-bar"><button type="button" class="btn quiet replay">Replay</button></div>\n  </figure>\n</section>\n')


def showcase_slots(public: dict) -> dict:
    study = study_slots(public)
    common = {"built": esc(public["built"])}
    return {
        "index.html": dict(common, lede=esc(lede_slot("index.html", public)), stage=stage_slot(public), hero=hero_slot(public),
                           problem=problem_slot(public), change=change_slot(public), figures=figures_slot(public),
                           video=video_slot(public), best=best_slot(public)),
        "benchmarks.html": dict(common, lede=esc(lede_slot("benchmarks.html", public)), seg=seg_slot(public),
                                boards=boards_slot(public), charts=charts_slot(public), engine=engine_slot(public),
                                headline=headline_slot(public), findings=findings_slot(public),
                                runners=runners_slot(public)),
        "study.html": dict(common, lede=esc(lede_slot("study.html", public)), **study),
        "install.html": dict(common, lede=esc(lede_slot("install.html", public)), install=install_slot(public)),
    }


def write_showcase_pages(public: dict) -> list[str]:
    """Write the numbers and findings into the four pages, so they are there without JavaScript and
    cannot fall behind the data. Every slot a page holds must be filled, and every slot filled must exist."""
    import re
    slot = re.compile(r"<!--slot:([\w-]+)-->(.*?)<!--/slot:\1-->", re.S)
    changed = []
    for name, filled in showcase_slots(public).items():
        path = DOCS / name
        if not path.is_file():
            raise SystemExit(f"{path.relative_to(REPO)} is missing")
        raw = path.read_bytes().decode("utf-8").replace("\r\n", "\n")
        wanted = {m.group(1) for m in slot.finditer(raw)} - {"sidebar", "tabbar"}
        if wanted != set(filled):
            raise SystemExit(f"{name}: slots in the page {sorted(wanted)} do not match the slots built {sorted(filled)}")
        page = slot.sub(lambda m: f"<!--slot:{m.group(1)}-->{filled[m.group(1)]}<!--/slot:{m.group(1)}-->"
                        if m.group(1) in filled else m.group(0), raw)
        navs = nav_slots(name, page)
        page = slot.sub(lambda m: f"<!--slot:{m.group(1)}-->{navs[m.group(1)]}<!--/slot:{m.group(1)}-->"
                        if m.group(1) in navs else m.group(0), page)
        if page != path.read_bytes().decode("utf-8"):
            path.write_bytes(page.encode("utf-8"))
            changed.append(name)
    return changed


# --- the guard ------------------------------------------------------------------------------

# Binary media under docs/media/ is not text and is not read as text; its file name is still checked. A caption
# file (.vtt) is text, so it is scanned like a page.
MEDIA_BINARY = ("mp4", "webm", "jpg", "jpeg", "avif", "png", "webp")

# Words that must never appear in anything under docs/, in any case: tool names, file names, vendor models.
FORBIDDEN_WORDS = (
    "codanna", "semble", "graphify", "token-goat", "tokengoat", "hindsight", "model2vec", "potion", "tree-sitter",
    "rustpython", "oxc", "ast-grep", "ripgrep", "lx", "lxr", "caveman", "cavecrew", "CLAUDE.md", "AGENTS.md",
    "session_index", "arms.json", "bench_parse", "build_site", "grep", "haiku", "sonnet", "opus", "claude", "anthropic",
    "openai", "gemini", "mcp", "bm25", "reciprocal rank", "sqlite", "hnsw", "faiss", "lancedb", "tantivy", "onnx",
    "PreToolUse", "PostToolUse", "SessionStart", "UserPromptSubmit", "settings.json", "settings.local.json",
    "local-index", "graphify-out", "results.json", "hooks.json", "engine.json", "install.json", "index.json")
# Shapes that must never appear: command lines and flags, the setup's paths, vendor model versions, tier labels,
# and any request to another host.
FORBIDDEN_PATTERNS = (
    # A flag follows a space or starts a line. A CSS custom property sits inside quotes or var( ), and the
    # stylesheets are not checked for flags at all.
    ("a command-line flag", r"(?<![\w'\"(-])--[a-z][a-z0-9-]+", ("html", "js")),
    ("a flag with a value", r"--[a-z][a-z0-9-]*=", ("html", "js")),
    ("a shell prompt", r"(?m)^\s*[$>]\s+\w", None),
    ("a package command", r"\b(?:pip3?|pipx|npm|npx|cargo|brew|winget|choco|curl|wget|sudo)\s+[-\w]", None),
    ("an interpreter command", r"\bpython3?\s+-", None),
    ("a git command", r"\bgit\s+(?:clone|checkout|pull|push|commit|add|init)\b", None),
    ("a path of the setup", r"\.claude\b|\.codex\b|\.codanna\b|(?<![\w.])(?:setup|benchmark|harness|rust)/", None),
    ("a model version", r"\bgpt-?\d\w*", None),
    ("a tier label", r"\btier [abc]\b", None),
    ("a request to another host", r"https?://(?!www\.w3\.org/2000/svg)|(?<![:\w])//[\w-]+\.[a-z]{2,}/|@import", None),
)


def forbidden_checks(arms: dict) -> list[tuple[str, "re.Pattern", tuple | None]]:
    """Every check the guard makes, compiled: words, shapes, and what arms.json says about the methods."""
    import re
    checks = [(f"the word {word!r}", re.compile(r"(?<![A-Za-z0-9])" + re.escape(word) + r"(?![A-Za-z0-9])", re.I), None)
              for word in FORBIDDEN_WORDS]
    checks += [(label, re.compile(pattern), kinds) for label, pattern, kinds in FORBIDDEN_PATTERNS]
    ids = sorted({arm["id"] for arm in arms["arms"]}, key=len, reverse=True)
    checks.append(("an internal method id", re.compile(r"(?<![\w#.\-])(?:" + "|".join(map(re.escape, ids)) + r")(?![\w\-])"), None))
    squash = lambda text: re.sub(r"\s+", " ", text).strip()
    strings = {squash(command) for command in arms["commands"].values()}
    strings |= {squash(arm[key]) for arm in arms["arms"] for key in ("detail", "reads") if arm.get(key)}
    for text in sorted(strings, key=len, reverse=True):
        checks.append((f"text copied from arms.json ({text[:40]!r}...)", re.compile(re.escape(text), re.I), "squash"))
    uses = {key for arm in arms["arms"] for key in arm.get("uses", [])}
    checks += [(f"the command key {key!r}", re.compile(r"(?<![A-Za-z0-9])" + re.escape(key) + r"(?![A-Za-z0-9])"), None) for key in sorted(uses)]
    return checks


def guard_public(arms: dict, install: dict | None = None, root: pathlib.Path | None = None) -> int:
    """Scan every file under the public folder, its file names included, for anything that must never be
    published. Exits with the file, the line and the offending text. The one thing let through is the
    install command the owner chose to publish in install.json. Returns the number of files scanned."""
    import html
    import re
    root = root or DOCS
    checks = forbidden_checks(arms)
    approved = []
    if install and install.get("status") == "public":
        command = install["command"]
        approved = [command, json.dumps(command)[1:-1], html.escape(command), html.escape(command, quote=False)]
    problems, scanned = [], 0
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        scanned += 1
        shown = path.relative_to(root).as_posix()
        kind = path.suffix.lstrip(".").lower()
        try:
            text = "" if kind in MEDIA_BINARY and shown.startswith("media/") else path.read_bytes().decode("utf-8")
        except UnicodeDecodeError:
            text = ""
        for token in sorted(approved, key=len, reverse=True):
            text = text.replace(token, " ")
        # Comment markers are not flags: the two dashes of "<!--" and "-->" are blanked before the check.
        flat = text.replace("<!--", "    ").replace("-->", "   ")
        subjects = [(shown, "file name", shown)] + [(shown, f"line {n}", line) for n, line in enumerate(flat.splitlines(), 1)]
        for where, place, line in subjects:
            for label, pattern, kinds in checks:
                if kinds == "squash" or (kinds and kind not in kinds):
                    continue
                hit = pattern.search(line)
                if hit:
                    problems.append(f"  docs/{where}, {place}: {label}: found {hit.group(0)!r}")
        # Copied text may run over several lines: the long strings from arms.json are looked for in the whole file.
        whole = re.sub(r"\s+", " ", flat)
        for label, pattern, kinds in checks:
            if kinds == "squash" and pattern.search(whole):
                problems.append(f"  docs/{shown}, whole file: {label}")
    if problems:
        shown_problems = problems[:30] + ([f"  and {len(problems) - 30} more"] if len(problems) > 30 else [])
        raise SystemExit("GUARD FAILED: the public folder holds something that must not be published.\n"
                         + "\n".join(shown_problems) + "\nNothing is safe to publish until this is fixed.")
    return scanned


ENGINE_UNITS = {"build_seconds": ("Build, from nothing", "s"), "build_seconds_no_embeddings": ("Build, without embeddings", "s"),
                "ask_ms": ("One call that answers all three questions", "ms"), "about_ms": ("About, one symbol", "ms"),
                "find_ms": ("Find, one concept", "ms"), "status_ms": ("Status", "ms"), "index_mb": ("Index on disk", "MB"),
                "binary_mb": ("Binary", "MB")}


def round_five(points: list[dict], cells: list[dict], agents: list[dict], arms: dict) -> list[str]:
    """The Round five section of RESULTS.md: one table for each question set with the same-day controls beside the
    new methods, the memory comparison cell by cell, the engine speed with its "how" text, and the measurement
    notes. The private report may name every method and tool."""
    by_id = {arm["id"]: arm for arm in arms["arms"]}
    order = [arm["id"] for arm in arms["arms"]]
    kinds = {"rust": "Rust engine", "rerun": "Same-day control", "heldout": "Held-out run"}
    groups = {arm["id"]: arm["group"] for arm in arms["arms"] if arm.get("group") in kinds}
    round_points = [p for p in points if p["setup"] == "single" and p["arm"] in groups]
    if not round_points:
        return []
    tier_label = {tier: f"{cid} ({tier})" for tier, cid, _ in PUBLIC_CLASSES}
    out = ["", "## Round five", "",
           "The Rust engine in every combination of its parts, three earlier methods run again on the same day, and "
           "a held-out set B of three more questions of the same shapes. One agent answering all three questions; "
           "light is Tier C and mid is Tier B. Hooks were on for every run of the round.", ""]
    for key, title in (("A", "Original questions, set A"), ("B", "Held-out questions, set B")):
        rows = sorted((p for p in round_points if p["set"] == key), key=lambda p: (order.index(p["arm"]), TIER_ORDER.index(p["tier"])))
        if not rows:
            continue
        out += [f"### {title}", "",
                "| Arm | Method | Kind | Class | Runs | Turns | Tokens | Fully correct | Share correct |",
                "|---|---|---|---|---:|---:|---:|---:|---:|"]
        for p in rows:
            arm = by_id[p["arm"]]
            out.append(f"| {p['arm']} | {arm['name']} | {kinds[groups[p['arm']]]} | "
                       f"{tier_label[p['tier']]} | {p['n']} | {p['requests']:.1f} | {p['tokens']:,.0f} | "
                       f"{p['spread']['perfect_runs']} of {p['n']} | {share(p['quality'])} |")
        out.append("")

    # Memory, cell by cell: the same configuration with and without memory.
    out += ["### Memory, cell by cell", "",
            "Each row sets a configuration with memory beside the same configuration without it, on the same question set and class. RGSM had its memory filled by a session that answered the same questions (a best case); RGSO and "
            "RGSOh by a session that explored the repository without seeing any question.", "",
            "| With memory | Without | Set | Class | Tokens with | Tokens without | Change | Turns with | Turns without | Fully correct with | Fully correct without |",
            "|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|"]
    pairs = (("RGSM", "RGS", "A"), ("RGSO", "RGS", "A"), ("RGSOh", "RGSh", "B"))
    compared = {}
    for with_id, without_id, key in pairs:
        for tier in TIER_ORDER:
            one, two = pick(points, with_id, "single", tier), pick(points, without_id, "single", tier)
            if one and two:
                compared.setdefault(with_id, []).append((one, two))
                out.append(f"| {with_id} | {without_id} | {key} | {tier_label[tier]} | {one['tokens']:,.0f} | {two['tokens']:,.0f} | "
                           f"{one['tokens'] / two['tokens'] - 1:+.1%} | {one['requests']:.1f} | {two['requests']:.1f} | "
                           f"{one['spread']['perfect_runs']} of {one['n']} | {two['spread']['perfect_runs']} of {two['n']} |")
    explored = compared.get("RGSO", []) + compared.get("RGSOh", [])
    if explored:
        fewer = sum(a["tokens"] < b["tokens"] for a, b in explored)
        more = sum(a["tokens"] > b["tokens"] for a, b in explored)
        right = sum(a["spread"]["perfect_runs"] for a, _ in explored), sum(a["n"] for a, _ in explored)
        base = sum(b["spread"]["perfect_runs"] for _, b in explored), sum(b["n"] for _, b in explored)
        out += ["", f"Memory from the exploring session (RGSO, RGSOh) used fewer tokens than the same configuration without memory in "
                    f"{fewer} of {len(explored)} cells and more in {more}. It was fully correct in {right[0]} of {right[1]} runs, "
                    f"against {base[0]} of {base[1]} without memory."]
    out.append("")

    # Engine speed.
    engine_file = BENCH / "data" / "engine.json"
    if engine_file.exists():
        engine = json.loads(engine_file.read_text(encoding="utf-8"))
        detail = engine.get("detail") or {}
        if detail:
            labels = list(detail)
            keys = [k for k in ENGINE_UNITS if any(k in detail[label] for label in labels)]
            out += ["### Engine speed", "", f"Measured {engine.get('measured', '')} on {engine.get('fixture', 'the fixture')}.", "",
                    "| Measure | " + " | ".join(label.replace("_", " ") for label in labels) + " |", "|---|" + "---:|" * len(labels)]
            for k in keys:
                title, unit = ENGINE_UNITS[k]
                out.append(f"| {title} | " + " | ".join(f"{detail[label][k]:g} {unit}" if k in detail[label] else "not measured"
                                                       for label in labels) + " |")
            out += ["", engine.get("how", ""), ""]

    # Measurement notes, counted from the runs.
    facts = session_facts(cells, agents, arms, points)
    round_cells = [c for c in cells if groups.get(c["arm"])]
    hooked = [c for c in round_cells if c["hook_calls"] > 0]
    start = facts["start_up"]
    out += ["### Measurement notes", "",
            f"- Hooks were on for every run of the round. Hook calls were recorded in {len(hooked)} of {len(round_cells)} runs "
            f"({sum(c['hook_calls'] for c in hooked)} calls).",
            "- The public control is the same-day one: A0r, S1r and S3r are the repeats of A0, S1 and S3 and are the ones the public "
            "leaderboard shows. The earlier A0, S1 and S3 runs stay in this report only.",
            f"- Start-up context grew between the sessions: the earlier A0, S1 and S3 single-agent runs averaged {start['earlier']:,.0f} "
            f"tokens ({start['earlier_agents']} agents) and the repeats A0r, S1r and S3r {start['later']:,.0f} ({start['later_agents']} agents), "
            f"on the {' and '.join(start['classes'])} classes.",
            "- The control's file reads pass through token-goat's read compression on the test machine, which hid lines the control needed "
            "in some runs. The earlier session (A0) ran without it."]
    control = facts.get("control")
    if control:
        def per_class(counts: dict) -> str:
            return ", ".join(f"{cid} {of(*counts[cid])}" for _, cid, _ in PUBLIC_CLASSES if cid in counts)
        out.append(f"- Control, fully correct runs: A0r {per_class(control['today']['original'])}; A0h {per_class(control['today']['held-out'])}; "
                   f"earlier A0 {per_class(control['earlier'])}.")
    out += ["- Memory was filled before the runs by two earlier sessions: RM, RGM, RSM and RGSM from one that answered the same three "
            "questions, RGSO and RGSOh from one that explored the repository without seeing any question.",
            f"- Cells with fewer than three complete runs in the round: {facts['cells']['short']} of {facts['cells']['second_session']}.",
            ""]
    return out


def main() -> int:
    if len(sys.argv) == 3 and sys.argv[1] == "--guard":
        scanned = guard_public(load("harness/arms.json"), install_of(), pathlib.Path(sys.argv[2]))
        print(f"guard passed: {scanned} files in {sys.argv[2]} hold nothing that must stay private")
        return 0
    results, index, arms = load("data/results.json"), load("data/index.json"), load("harness/arms.json")
    cells, agents = results["cells"], results["agents"]
    points = points_of(cells)
    used = {p["arm"] for p in points}
    known = {arm["id"] for arm in arms["arms"]}
    if used - known:
        raise SystemExit(f"measured but not described in arms.json: {sorted(used - known)}")
    start = statistics.fmean(a["start"] for a in agents if a["role"] in ("all", "q1", "q2", "q3"))
    orchestration = orchestration_of(points, arms)
    facts_file = BENCH / "data" / "hooks.json"
    hooks = hooks_of(points, cells, arms,
                     json.loads(facts_file.read_text(encoding="utf-8")) if facts_file.exists() else {})
    # Reruns made to test token-goat's hooks, and the Bash pair made for the same test, are compared
    # on the hooks page. They are not methods, so the rankings and the method count leave them out.
    hook_test = {arm["id"] for arm in arms["arms"] if arm.get("group") == "hooks"}
    methods = [p for p in points if p["arm"] not in hook_test]
    # The findings and the README table are about the original questions; the held-out runs are tabulated in the
    # Round five section of RESULTS.md and in the report's own tables.
    original = [p for p in methods if p["set"] == "A"]
    text = text_of(original, [c for c in cells if c["arm"] not in hook_test and c.get("set", "A") == "A"], agents, index, arms, start)
    text["orchestration"] = orchestration_text(orchestration, arms)
    text["hooks"] = hooks_text(hooks, arms)
    # The home page and the README carry the headline of each new section.
    text["findings"] += [line for line in text["hooks"][:1] if line.startswith(README_FINDINGS)]
    # The answer before the evidence: a few lines at the top of each findings page.
    text["answers"] = answers_of(original, orchestration, hooks, arms)
    picked = ("Cheapest tier, one agent", "Two turns is the floor", "The model tier moves")
    text["results_short"] = [next((line for line in text["findings"] if line.startswith(start)), "") for start in picked]
    text["results_short"] = [line for line in text["results_short"] if line]
    lines = text["orchestration"]
    text["orchestration_short"] = (lines[:1] + [line for line in lines if line.startswith("With Tier C")][:1]
                                   + [line for line in lines if line.startswith("Three subagents with nobody")][:1])
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
        "sections": site_sections(),
        "points": points,
        "runs": sorted(cells, key=lambda c: (order.index(c["arm"]), setups.index(c["setup"]),
                                             TIER_ORDER.index(c["tier"]), c["rep"])),
        "index": index,
        "context": {"parts": context_parts(start)},
        "cache": [{"tier": f"{t['name']}: {t['model']}", "tokens": start, "cold": start * PRICE[t["id"]][3] / 1e6,
                   "warm": start * PRICE[t["id"]][2] / 1e6,
                   "measured": measured_plan(agents) if t["id"] == "A" else ""} for t in arms["tiers"]],
        "price": {tier: dict(zip(("input", "output", "cache_read", "cache_write"), rate)) for tier, rate in PRICE.items()},
        "orchestration": orchestration,
        "hooks": hooks,
        "text": text,
    }
    written = write_site_data(data)
    refreshed = write_static_text(text)
    if refreshed:
        print("static text refreshed in: " + ", ".join(refreshed))
    stamped = stamp_assets()
    if stamped:
        print(f"asset links restamped in {stamped} pages")
    target = REPORT / "data" / "benchmark.js"
    (BENCH / "RESULTS.md").write_text(
        markdown(points, index, arms, text) + more_tables(orchestration, hooks, arms, text)
        + "\n".join(round_five(points, cells, agents, arms)) + "\n", encoding="utf-8")
    in_readme = write_readme(readme_block(original, arms, text))
    print("site data: " + ", ".join(f"{name} {size:,} bytes" for name, size in written))
    print(f"wrote {target.relative_to(REPO)} ({target.stat().st_size:,} bytes), benchmark/RESULTS.md"
          f"{' and the README results block' if in_readme else ' (README has no results markers)'}: "
          f"{len(points)} points from {len(cells)} runs, {len(agents)} agents, {len(described)} methods")
    public = public_of(points, cells, agents, index, orchestration, hooks, arms)
    size = write_showcase_data(public)
    filled = write_showcase_pages(public)
    restamped = stamp_assets(DOCS, [name for name, _ in PUBLIC_PAGES])
    chosen = next((c for c in public["configs"] if c["best"]), None)
    print(f"showcase: docs/data/showcase.js {size:,} bytes, text refreshed in {len(filled)} pages, asset links restamped "
          f"in {restamped}; {len(public['configs'])} configurations, best: {chosen['name'] if chosen else 'none'}")
    if chosen:
        print(f"best chosen by the {'four-cell' if public['best_rule'] == 'cells' else 'light-class'} rule; " + "; ".join(
            f"{cid} {sid}: {pt['tokens']:,.0f} tokens, {pt['turns']:.1f} turns, {pt['perfect_runs']} of {pt['runs']} fully correct"
            for cid, sid in BEST_CELLS for pt in [cell_of(chosen, cid, sid)] if pt))
    # The last step of every build, so nothing private can be published by accident.
    scanned = guard_public(arms, public["install"])
    print(f"guard passed: {scanned} files under docs/ hold nothing that must stay private")
    return 0


if __name__ == "__main__":
    sys.exit(main())
