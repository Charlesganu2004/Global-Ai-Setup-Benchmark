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
    folder = REPO / "docs" / "data"
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
        page = (REPO / "docs" / name).read_text(encoding="utf-8")
        for match in re.finditer(r'<h2 id="([^"]+)"[^>]*>(.*?)</h2>', page, re.S):
            title = html.unescape(re.sub(r"<[^>]+>", "", match.group(2))).strip()
            out.append({"page": key, "id": match.group(1), "title": title})
    return out


def answers_of(methods: list[dict], orchestration: dict, hooks: dict, arms: dict) -> dict:
    """One sentence for each question a visitor arrives with, each computed from the data."""
    name = {arm["id"]: arm["name"] for arm in arms["arms"]}
    by_id = {arm["id"]: arm for arm in arms["arms"]}
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
    native = [r for r in hooks["pairs"] if by_id[r["off"]]["uses"] == ["grep"]]
    if native:
        on, off = sum(r["with"]["n"] for r in native), sum(r["without"]["n"] for r in native)
        right_on, right_off = (sum(r[side]["perfect_runs"] for r in native) for side in ("with", "without"))

        def mean(side: str, field: str) -> float:   # weighted by the runs made with the hooks, pair by pair
            return sum(r[side][field] * r["with"]["n"] for r in native) / on

        change = mean("with", "tokens") / mean("without", "tokens") - 1
        repeated = [r for r in native if r["without"]["n"] > 1]
        spread = (sum((r["without"]["tokens_high"] - r["without"]["tokens_low"]) / r["without"]["tokens"] * r["with"]["n"]
                      for r in repeated) / sum(r["with"]["n"] for r in repeated)) if repeated else 0.0
        saved = change < -spread   # fewer tokens by more than repeats of one prompt differ
        worse = right_on / on < right_off / off
        hidden = mean("with", "hook_redactions") > 0 and sum(r["with"]["q1_incomplete"]["runs"] for r in native) > 0
        out["hooks"] = (
            ("They saved tokens on these lookups. " if saved and not worse else "Not on these lookups. ")
            + (f"Token use fell by {-change:.0%}" if saved else
               "Token use stayed inside the spread between repeats" if abs(change) <= spread else
               f"Token use rose by {change:.0%}")
            + f", and with no index {right_on} of {on} runs ended fully correct with the hooks against "
            f"{right_off} of {off} without"
            + (", because a redaction marker hid a line the lookup needed." if worse and hidden else "."))
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
        path = REPO / "docs" / name
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


def stamp_assets() -> int:
    """Give every script and stylesheet link in the pages a `?v=` made from the file it points at.
    A browser keeps scripts for a while; without this, a visitor could get a new page with last
    week's script and a broken layout until the cache ran out."""
    import hashlib
    import re
    docs, stamped = REPO / "docs", 0

    def marked(match):
        target = docs / match.group(2)
        if not target.is_file():
            return match.group(0)
        digest = hashlib.sha1(target.read_bytes().replace(b"\r\n", b"\n")).hexdigest()[:8]
        return f'{match.group(1)}="{match.group(2)}?v={digest}"'

    for _, name in SITE_PAGES:
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


def main() -> int:
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
    text = text_of(methods, [c for c in cells if c["arm"] not in hook_test], agents, index, arms, start)
    text["orchestration"] = orchestration_text(orchestration, arms)
    text["hooks"] = hooks_text(hooks, arms)
    # The home page and the README carry the headline of each new section.
    text["findings"] += [line for line in text["hooks"][:1] if line.startswith(README_FINDINGS)]
    # The answer before the evidence: a few lines at the top of each findings page.
    text["answers"] = answers_of(methods, orchestration, hooks, arms)
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
    target = REPO / "docs" / "data" / "benchmark.js"
    (BENCH / "RESULTS.md").write_text(
        markdown(points, index, arms, text) + more_tables(orchestration, hooks, arms, text), encoding="utf-8")
    in_readme = write_readme(readme_block(methods, arms, text))
    print("site data: " + ", ".join(f"{name} {size:,} bytes" for name, size in written))
    print(f"wrote {target.relative_to(REPO)} ({target.stat().st_size:,} bytes), benchmark/RESULTS.md"
          f"{' and the README results block' if in_readme else ' (README has no results markers)'}: "
          f"{len(points)} points from {len(cells)} runs, {len(agents)} agents, {len(described)} methods")
    return 0


if __name__ == "__main__":
    sys.exit(main())
