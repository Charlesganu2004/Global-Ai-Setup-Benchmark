#!/usr/bin/env python3
"""Turn the measured benchmark data into what the site and the README read.

    python benchmark/harness/build_site.py

Reads   benchmark/data/results.json   every agent run, written by bench_parse.py
        benchmark/data/index.json     index build times and sizes
        benchmark/data/engine.json    optional: the "public" rows of engine speed
        benchmark/data/install.json   the "public" install block: release links, files, commands (the install page is built from it)
        benchmark/data/video.json     optional: the films the page plays, and the files each can be played from
        benchmark/data/codex.json     optional: read only to learn whether a second agent program was measured
        benchmark/harness/arms.json   what each method is and the commands it was given
Writes  report/                       the full report, in the repository and not served by the site: its data files, text and asset hashes
        docs/data/showcase.js         window.SHOWCASE, the public showcase's data
        docs/*.html                   the showcase pages' computed text, top bar, foot and asset hashes (the page shells, docs/assets and
                                      docs/media are written by hand; this script fills the slots between the <!--slot:NAME--> marks)
        benchmark/RESULTS.md          the same numbers as tables

The full report (report/) is in the repository but is not served by the site. The showcase (docs/) is published by GitHub Pages,
shows measured outcomes only, and every build ends by scanning it for anything that must not be on the site.
    python benchmark/harness/build_site.py --guard DIR   run only that scan, on any folder
    python benchmark/harness/build_site.py --guard-selftest   plant one violation for each rule of the scan in a
                                                              scratch copy of docs/ and check that each is caught

Every number in the site's prose is computed here from those files, so the
text cannot drift from the tables.
"""
from __future__ import annotations

import datetime
import json
import pathlib
import re
import statistics
import sys

HERE = pathlib.Path(__file__).resolve().parent
BENCH = HERE.parent
REPO = BENCH.parent
REPORT = REPO / "report"   # the full report: in the repository, not served by the site
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


def models_of() -> dict:
    """benchmark/data/models.json: the models the three classes of the runs meant on the days of the runs, with the effort each
    ran at, read from the run transcripts. The site names these models wherever it used to say light, middle or top class."""
    path = BENCH / "data" / "models.json"
    if not path.is_file():
        raise SystemExit("benchmark/data/models.json is missing: the pages name the models the runs used")
    data = json.loads(path.read_text(encoding="utf-8"))
    tiers = {}
    for item in data.get("tiers") or []:
        for key in ("tier", "name", "effort"):
            if not isinstance(item.get(key), str) or not item[key]:
                raise SystemExit(f"models.json: every tier needs a {key}")
        tiers[item["tier"]] = item
    if sorted(tiers) != ["A", "B", "C"]:
        raise SystemExit("models.json: tiers must be C, B and A")
    codex = next((h for h in data.get("harnesses") or [] if h.get("model_name")), None)
    return {"how": str(data.get("how", "")), "read": str(data.get("read", "")), "tiers": tiers, "codex": codex}


MODELS = models_of()
TIER_OF = {"light": "C", "mid": "B", "top": "A"}


def model_label(tier: str) -> str:
    """The model and its effort as a label: Claude Haiku 5.5, low effort."""
    item = MODELS["tiers"][tier]
    return f"{item['name']}, {item['effort']} effort"


def model_at(tier: str) -> str:
    """The same inside a sentence: Claude Haiku 5.5 at low effort."""
    item = MODELS["tiers"][tier]
    return f"{item['name']} at {item['effort']} effort"


def models_line() -> str:
    """The one short line that says what the model names mean: the runs asked for a class of model, and these are the models
    that class meant on the days of the runs."""
    dates = re.search(r"(\d{4}-\d{2}-\d{2}) to (\d{4}-\d{2}-\d{2})", MODELS["how"])
    when = f" on the days of rounds one to five, {dates.group(1)} to {dates.group(2)}" if dates else " on the days of the runs"
    named = ", ".join(model_at(tier) for tier in "CBA")
    return (f"The runs asked for a class of model: the smallest, a middle one and the largest. These are the models those classes meant{when}: "
            f"{named}. Round six, on {ROUND_SIX_DATE}, asked for the same classes; its records name only the model family, so its versions are taken to be the same.")


def modelize(text):
    """Say the model where a sentence says a class: the light, mid or top class and the tiers C, B and A become the model and
    its effort from models.json. Strings only; anything else comes back unchanged."""
    if not isinstance(text, str) or not text:
        return text
    low, mid, top = model_at("C"), model_at("B"), model_at("A")
    both = {"light": low, "mid": mid, "top": top}
    text = re.sub(r"(?i)\bthe light, mid and top (?:model )?classes\b", f"{low}, {mid} and {top}", text)
    text = re.sub(r"(?i)\bthe light and mid (?:model )?classes\b", f"{low} and {mid}", text)
    text = re.sub(r"(?i)\bthe light and top (?:model )?classes\b", f"{low} and {top}", text)
    text = re.sub(r"(?i)\b(?:the )?(light|mid|top) (?:model )?class\b", lambda m: both[m.group(1).lower()], text)
    text = re.sub(r"(?i)\bone (light|mid|top) agent\b", lambda m: f"one {MODELS['tiers'][TIER_OF[m.group(1).lower()]]['name']} agent", text)
    # The class as an adjective: "light-class runs", "three light workers", "a top-tier orchestrator".
    adjective = {key: f"{MODELS['tiers'][tier]['name']} ({MODELS['tiers'][tier]['effort']} effort)" for key, tier in TIER_OF.items()}
    adjective.update({"middle": adjective["mid"], "apex": adjective["top"]})
    text = re.sub(r"(?i)\b(light|mid|top)-class\b", lambda m: adjective[m.group(1).lower()], text)
    text = re.sub(r"(?i)\b(light|mid) (workers?|agents?)\b", lambda m: f"{adjective[m.group(1).lower()]} {m.group(2)}", text)
    text = re.sub(r"(?i)\b(top|apex|mid|middle)-tier (orchestrators?|reviewers?|workers?|subagents?)\b",
                  lambda m: f"{adjective[m.group(1).lower()]} {m.group(2)}", text)
    text = re.sub(r"(?i)\bboth model classes\b", "both models", text)
    text = re.sub(r"(?i)\bmodel class\b", "model", text)
    text = re.sub(r"(?i)\b(light|mid|top)(?=, (?:original|held-out) questions\b)", lambda m: both[m.group(1).lower()], text)
    text = re.sub(r"(?i)\b(a|the) (larger|smaller) model class\b", lambda m: f"{m.group(1)} {m.group(2)} model", text)
    text = re.sub(r"(?i)\b(?:the )?(?:cheapest|smallest|light) model tier\b", low, text)
    text = re.sub(r"(?i)\bthe (light|top|mid|middle) model\b", lambda m: {"light": low, "top": top}.get(m.group(1).lower(), mid), text)
    text = re.sub(r"(?i)\btier[\s_-]*([abc])(?:,\s*(?:scout|coder|apex))?\b", lambda m: model_at(m.group(1).upper()), text)
    text = re.sub(r"(?i)\b(?:the )?(?:top|apex)[ -]tier\b", top, text)
    text = re.sub(r"(?i)\b(?:the )?(?:mid|middle)[ -]tier\b", mid, text)
    text = re.sub(r"(?i)\b(?:the )?(?:cheapest|light)[ -]tier\b", low, text)
    text = re.sub(r"(?i)\bmodel classes\b", "models", text)
    text = re.sub(r"(?i)\bthe (?:cheapest|smallest) model\b", low, text)
    text = re.sub(r"(?i)\btier by tier\b", "model by model", text)
    text = re.sub(r"(?i)\b(this|that|each|every|one|another|a|the same) tier\b", lambda m: f"{m.group(1)} model", text)
    text = re.sub(r"(?i)\bmodel tiers\b", "models", text)
    text = re.sub(r"(?i)\bmodel tier\b", "model", text)
    text = re.sub(r"(?i)\btiers\b", "models", text)
    return text


def modelize_all(value):
    """modelize over every string of a nested structure."""
    if isinstance(value, str):
        return modelize(value)
    if isinstance(value, (list, tuple)):
        return [modelize_all(item) for item in value]
    if isinstance(value, dict):
        return {key: modelize_all(item) for key, item in value.items()}
    return value


def footprint_of() -> dict | None:
    """benchmark/data/footprint.json: the measured sizes on disk and the one sentence that says what the engine is written in."""
    path = BENCH / "data" / "footprint.json"
    if not path.is_file():
        return None
    data = json.loads(path.read_text(encoding="utf-8"))
    for key in ("how", "first_generation", "pgr", "language"):
        if key not in data:
            raise SystemExit(f"footprint.json lacks {key!r}")
    return data


def platforms_of() -> dict | None:
    """benchmark/data/platforms.json: what was tested on which system."""
    path = BENCH / "data" / "platforms.json"
    if not path.is_file():
        return None
    data = json.loads(path.read_text(encoding="utf-8"))
    for key in ("tests", "same_answers", "installers"):
        if not isinstance(data.get(key), list):
            raise SystemExit(f"platforms.json lacks the list {key!r}")
    return data
# How the findings shown in the top-level README begin.
README_FINDINGS = ("An agent starts", "Where the shipped build", "The shipped build on", "Does adding token-goat",
                   "Do the engines do as well", "The model tier moves", "In these runs orchestration",
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
            f"In these runs orchestration came with more fully correct runs, and it is the expensive part. With Tier C subagents "
            f"the orchestrator's two calls were {share(orch_share)} of the run's cost. Without a brief or a reviewer, {len(flawed)} of "
            f"{len(par)} three-subagent runs returned a wrong detail; orchestrated on the same tier, "
            f"{sum(c['quality'] >= 0.999 for c in checked)} of {len(checked)} ended fully correct, {before} of them "
            f"before the review changed anything. The orchestrator is of a larger model class than the subagents, so this "
            f"compares more than orchestration, and every cost is an estimate at list prices, with the cost of planning "
            f"imputed from the few planning calls that were measured.")
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
        "lede": f"{len(agents)} agents measured on one repository: three lookup questions and, in round five, three held-out ones, {measured} ways of finding the "
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


def arm_detail(arm: dict) -> str:
    """What the agent was given, as RESULTS.md words it. The held-out twin of a round-six arm is not run on questions nobody tuned for:
    the pack of that round was written after the questions had been run."""
    text = arm["detail"]
    if arm.get("group") == "team" and "three questions nobody tuned for" in text:
        text = text.replace("three questions nobody tuned for", "the held-out set of three questions, which for round six is a second set and not an untouched one, "
                            "because the pack was written after these questions had been run")
    return text


def markdown(points: list[dict], index: dict, arms: dict, text: dict) -> str:
    name = {arm["id"]: arm["name"] for arm in arms["arms"]}
    order = [arm["id"] for arm in arms["arms"]]
    out = ["# Benchmark results", "", text["lede"], "",
           "The charts are on the project site. This file is the same data as tables, written by "
           "`benchmark/harness/build_site.py`. Read the section \"Known weaknesses of the study\" at the end before quoting a number. "
           "A \"Correct\" column is a mean score between 0 and 1 over the answers, with partial credit, not a share of fully correct runs. "
           "The no-index control of the second session had its reads altered by another tool on the test machine; "
           "its figures are shown as measured and are not set against the product's for correctness.",
           "", "## Findings", ""]
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
    out += [f"| {arm['id']} | {arm['name']} | {arm_detail(arm)} |" for arm in arms["arms"] if arm["id"] in {p["arm"] for p in points}]
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
           "| # | Method | Runs | Turns | Tokens | Cost, cache warm | Mean score |", "|---:|---|---:|---:|---:|---:|---:|"]
    for place, p in enumerate(board, 1):
        out.append(f"| {place} | {p['arm']} {name[p['arm']]} | {p['n']} | {p['requests']:.1f} | "
                   f"{p['tokens']:,.0f} | {money(p['cost_warm'])} | {share(p['quality'])} |")
    out += ["", "The mean score is the average score of the answers, with partial credit; it is not a share of fully correct runs."]
    control = next((a["id"] for a in arms["arms"] if a.get("public", {}).get("family") == "control"), None)
    if control and any(p["arm"] == control for p in board):
        out.append(f"{control}: another tool on the test machine, token-goat's secret-redaction hook, altered some of this row's reads, so its "
                   "mean score is not comparable with the others and its fully correct runs are not set against the product's. "
                   "See \"Known weaknesses of the study\" in benchmark/RESULTS.md.")
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
        body = json.dumps(modelize_all(rounded(value)), ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/")
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
            ("It came with more fully correct runs, at a price. " if better else "Not on accuracy, in these runs. ")
            + f"On the cheapest subagents {t['orch_perfect']} of {t['orch_runs']} "
            f"orchestrated runs ended fully correct, against {t['parallel_perfect']} of {t['parallel_runs']} for three "
            f"subagents with nobody reviewing, and a run cost an estimated {statistics.fmean(ratios):.0f} times one agent at list prices.")
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

PUBLIC_CLASSES = tuple((tier, cid, model_label(tier)) for tier, cid in (("C", "light"), ("B", "mid"), ("A", "top")))   # the ids stay light, mid and top in the data; the pages say the model
PUBLIC_FAMILIES = {"control": "Control", "tool": "Separate tools", "engine-v1": "Our engine, first generation",
                   "engine-v2": "Our engine, second generation", "team": "Our engine in team setups"}
# The arms of the team round (group "team" in arms.json) are named here, in plain words and without an internal id. A team arm that
# has results and no name here is refused, so an id can never reach a page. The demonstration set arms are not published.
TEAM_PUBLIC = {
    "R6": {"slug": "team-baseline", "name": "PGR, graph and search, round six", "family": "team", "order": 20},
    "TP": {"slug": "team-brief-pack", "name": "PGR, graph and search, with a brief pack", "family": "team", "order": 21},
    "TPG": {"slug": "team-graph-pack", "name": "PGR, code graph only, with a graph pack", "family": "team", "order": 22},
    "TPS": {"slug": "team-search-pack", "name": "PGR, search only, with a search pack", "family": "team", "order": 23},
    "TH": {"slug": "team-hub", "name": "PGR, graph and search, with a team hub", "family": "team", "order": 24},
    "TPH": {"slug": "team-pack-and-hub", "name": "PGR, graph and search, with a brief pack and a team hub", "family": "team", "order": 25},
    "R6V": {"slug": "team-ubuntu-vm", "name": "PGR, graph and search, in an Ubuntu virtual machine", "family": "team", "order": 26},
    "THV": {"slug": "team-hub-ubuntu-vm", "name": "PGR, graph and search, with a team hub, in an Ubuntu virtual machine", "family": "team", "order": 27},
    "TPHV": {"slug": "team-pack-and-hub-ubuntu-vm", "name": "PGR, graph and search, with a brief pack and a team hub, in an Ubuntu virtual machine", "family": "team", "order": 28},
    "RN": {"slug": "team-no-neural-network", "name": "PGR, graph and search, without the neural network", "family": "team", "order": 29},
    "TPN": {"slug": "team-brief-pack-no-neural-network", "name": "PGR, graph and search, with a brief pack, without the neural network", "family": "team", "order": 30},
    "TPX": {"slug": "team-pack-only", "name": "A brief pack alone, without a lookup command", "family": "team", "order": 31},
}
TEAM_UNPUBLISHED = {"T0", "TL", "TI"}


def public_arm(arm: dict) -> dict | None:
    """The public name, slug, family and order of an arm: its own "public" object in arms.json, or its row of TEAM_PUBLIC."""
    return arm.get("public") or TEAM_PUBLIC.get(arm["id"])
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
PUBLIC_CLASS_TEXT = {"light": f"{MODELS['tiers']['C']['name']}, run at {MODELS['tiers']['C']['effort']} reasoning effort. The smallest of the three.",
                     "mid": f"{MODELS['tiers']['B']['name']}, run at {MODELS['tiers']['B']['effort']} reasoning effort. The middle one.",
                     "top": f"{MODELS['tiers']['A']['name']}, run at {MODELS['tiers']['A']['effort']} reasoning effort. The largest."}
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
     "text": "A second set of three questions of the same three shapes, written later by the same team. The people and "
             "agents who built the engine through round five did not see them while building it. The pack of round six was written "
             "after these questions had been run, so for round six they are a second set of questions, not an untouched one."},
)
SET_OF = {s["key"]: s["id"] for s in PUBLIC_SETS}
TIER_OF_CLASS = {cid: tier for tier, cid, _ in PUBLIC_CLASSES}
# The four cells the best configuration is chosen on: the light and mid classes, on both question sets.
BEST_CELLS = tuple((cid, s["id"]) for s in PUBLIC_SETS for _, cid, _ in PUBLIC_CLASSES[:2])
# What the site calls the highlighted configuration. The rule that picks it was written after the results were known,
# so the site does not call it "best"; the label says what the rule measures.
BEST_LABEL = "Most reliable"
TIE_TOKENS = 1000                    # configurations this close in tokens are said to tie
EARLIER_LABEL = "Earlier session"
ROUND_SIX_DATE = "2026-10-14"   # the day round six ran, as the commit that holds its results says; results.json carries no dates
TEAM_LABEL = "Round six, run a day after round five."
CONTROL_NOTE = "Its reads were altered by another tool on the test machine."
CONTROL_LINK_TEXT = "Why this matters"
CONTROL_LINK = "study.html#control"


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
    questions among those with at least BEST_MIN_RUNS runs there, then the fewer tokens. Every published configuration is
    eligible, those of the team round included: the rule was set after round five and before round six ran."""
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


def saving_range(versus: dict) -> str:
    """The lowest and the highest saving in tokens against no index, as a page words it: "28% to 54%"."""
    low, high = f"{versus['low']:.0%}", f"{versus['high']:.0%}"
    return low if low == high else f"{low} to {high}"


def versus_control(configs: list[dict]) -> dict | None:
    """The highlighted configuration against no index in each of the four cells where both were measured: tokens and
    turns of each, and the saving in tokens; then the lowest and the highest saving. Tokens and turns only. The
    fully correct counts of the control are not set against the product's: another tool on the test machine altered
    some of the control's reads, which lowered its correctness."""
    best = next((c for c in configs if c["best"]), None)
    control = next((c for c in configs if c["family"] == "control"), None)
    cells = []
    for cid, sid in BEST_CELLS:
        b, c = cell_of(best, cid, sid), cell_of(control, cid, sid)
        if b and c:
            cells.append({"class": cid, "set": sid, "tokens": b["tokens"], "control_tokens": c["tokens"],
                          "turns": b["turns"], "control_turns": c["turns"], "saving": 1 - b["tokens"] / c["tokens"]})
    if not cells:
        return None
    savings = [cell["saving"] for cell in cells]
    return {"cells": cells, "low": min(savings), "high": max(savings)}


def ordered_rows(configs: list[dict], class_id: str, set_id: str) -> list[tuple[dict, dict]]:
    """Configurations with a point on the class and the question set: the highlighted one first, then fewest tokens.
    A configuration not measured on the set is not in the list."""
    rows = [(c, pt) for c in configs for pt in c["points"] if pt["class"] == class_id and pt["set"] == set_id]
    return sorted(rows, key=lambda row: (not row[0]["best"], row[1]["tokens"]))


def view_note(rows: list[tuple[dict, dict]]) -> str:
    """The line under one leaderboard view. Where another configuration used fewer tokens than the highlighted one, it
    says so, with the exact tokens, and says how many configurations are within TIE_TOKENS of the lowest. Where the
    highlighted configuration was not measured, it says that no row is marked."""
    if not rows:
        return ""
    label = BEST_LABEL.lower()
    marked = next(((c, pt) for c, pt in rows if c["best"]), None)
    if not marked:
        return f'The {label} configuration was not measured on this class with these questions, so no row is marked.'
    mine = marked[1]["tokens"]
    fewer = sorted(((c, pt) for c, pt in rows if pt["tokens"] < mine), key=lambda row: row[1]["tokens"])
    if not fewer:
        return f"No other configuration used fewer tokens than the {label} one here."
    count = len(fewer)
    shown = "; ".join(f'"{c["name"]}", {pt["tokens"]:,.0f}' for c, pt in fewer[:3]) + (f"; and {count - 3} more" if count > 3 else "")
    text = (f"{number_word(count, True)} other {'configuration' if count == 1 else 'configurations'} used fewer tokens than the "
            f"{label} one ({mine:,.0f}): {shown}.")
    lowest = fewer[0][1]["tokens"]
    tied = [row for row in rows if row[1]["tokens"] - lowest <= TIE_TOKENS]
    if len(tied) >= 2:
        text += (f" {number_word(len(tied), True)} configurations are within a thousand tokens of the lowest ({lowest:,.0f})"
                 + (f", the {label} one among them" if any(c["best"] for c, _ in tied) else "") + ".")
    return text


def public_findings(configs: list[dict], cells: list[dict], orchestration: dict, hooks: dict, arms: dict,
                    hook_test: set, owner: dict, engine: list[dict]) -> dict:
    """The findings in plain sentences with their numbers. `owner` maps the id of an arm to the slug of the public
    configuration its runs belong to. A sentence is written only when the numbers it quotes exist. Every sentence
    that counts runs says "in these runs" or gives the count; none claims a difference the runs cannot support, and
    no fully correct count of the control is set against the product's."""
    by_slug = {c["slug"]: c for c in configs}
    best = next((c for c in configs if c["best"]), None)
    control = next((c for c in configs if c["family"] == "control"), None)
    first = first_generation(configs)
    by_arm = {arm_id: by_slug[slug] for arm_id, slug in owner.items() if slug in by_slug}
    light = {slug: cell_of(c, "light", "original") for slug, c in by_slug.items()}
    versus = versus_control(configs)
    headline = []
    if best and control and light[best["slug"]] and light[control["slug"]]:
        b, c = light[best["slug"]], light[control["slug"]]
        text = (f"On the light class and the original questions, in these runs, one agent using \"{best['name']}\" needed "
                f"{tokens(b['tokens'])} tokens and {b['turns']:.1f} turns for the three lookups, against {tokens(c['tokens'])} "
                f"tokens and {c['turns']:.1f} turns with no index: {1 - b['tokens'] / c['tokens']:.0%} fewer tokens in this one cell.")
        if versus and len(versus["cells"]) > 1:
            text += (f" Across the {number_word(len(versus['cells']))} cells where both were measured, the light and mid classes on "
                     f"the original and the held-out questions, the saving against no index was {saving_range(versus)} in tokens.")
        headline.append(text)
    if best and first and best is not first and light[best["slug"]] and light[first["slug"]]:
        b, f = light[best["slug"]], light[first["slug"]]
        text = (f"In these runs \"{best['name']}\" was fully correct in {of(b['perfect_runs'], b['runs'])} runs on the light class "
                f"and the original questions, and \"{first['name']}\" in {of(f['perfect_runs'], f['runs'])}.")
        cb, cf = cell_counts(best), cell_counts(first)
        if cb and cf and cb[2] == cf[2] == len(BEST_CELLS):
            text += f" Over the {number_word(cb[2])} cells where both were measured, {of(*cb[:2])} against {of(*cf[:2])}."
        text += (" A run is fully correct when all three answers are. Fully correct counts are not set against no index: another "
                 "tool on the test machine altered some of the no-index agent's reads, which lowered its correctness.")
        headline.append(text)
    # The floor: two turns, one to ask and one to answer. Said of the two configurations only when both reach it.
    if best and first and best is not first:
        def classes_of(config: dict, sid: str) -> list:
            return [cell_of(config, cid, sid) for cid in ("light", "mid")]
        both = {config["slug"]: classes_of(config, "original") for config in (best, first)}
        # A configuration whose agent has no command to call has no asking turn: its floor is the one turn that answers.
        floor = {config["slug"]: 1.05 if config.get("commandless") else 2.05 for config in (best, first)}
        if all(pt and pt["turns"] <= floor[slug] for slug, pts in both.items() for pt in pts):
            def spent(config: dict) -> str:
                pts = both[config["slug"]]
                return f"{tokens(pts[0]['tokens'])} tokens on the light class and {tokens(pts[1]['tokens'])} on the mid class"
            if best.get("commandless"):
                text = ("Two turns is the floor for a task that needs a lookup command: one to ask and one to answer. On the original questions "
                        f"\"{first['name']}\" took 2 turns on both classes, with {spent(first)}. \"{best['name']}\" has no command to call: with the pack "
                        f"already in its prompt it answered in 1 turn on both classes, with {spent(best)}.")
            else:
                text = ("Two turns is the floor for this task: one to ask and one to answer. On the original questions "
                        f"\"{best['name']}\" answered in 2 turns on both classes, with {spent(best)}; \"{first['name']}\" also "
                        f"took 2 turns on both, with {spent(first)}.")
            if not best.get("commandless") and all(abs(a["tokens"] / z["tokens"] - 1) <= 0.01 for a, z in zip(both[best["slug"]], both[first["slug"]])):
                text += (" On these lookups the first generation matched it in tokens and turns"
                         + ("; the measured difference between the two generations is speed, under Engine speed." if engine else "."))
            held = {config["slug"]: classes_of(config, "held-out") for config in (best, first)}
            if all(pt for pts in held.values() for pt in pts):
                def turns_of(pts: list) -> str:
                    return f"{pts[0]['turns']:.1f} turns on the light class and {pts[1]['turns']:.1f} on the mid class"
                text += (f" On the held-out questions \"{best['name']}\" took {turns_of(held[best['slug']])}, "
                         f"and \"{first['name']}\" {turns_of(held[first['slug']])}.")
            headline.append(text)
    repeated = [(c, p) for c in configs for p in [light[c["slug"]]] if p and p["runs"] >= 2]
    if len(repeated) >= 2:
        lean = min(repeated, key=lambda row: (row[1]["turns"], row[1]["tokens"]))
        heavy = max(repeated, key=lambda row: (row[1]["turns"], row[1]["tokens"]))
        headline.append(
            "Tokens follow turns, because most of a run's tokens are the agent's start-up context, read again on every turn. On "
            f"the light class and the original questions the leanest published configuration, \"{lean[0]['name']}\", took "
            f"{lean[1]['turns']:.1f} turns and {tokens(lean[1]['tokens'])} tokens, and the heaviest, \"{heavy[0]['name']}\", "
            f"{heavy[1]['turns']:.1f} turns and {tokens(heavy[1]['tokens'])} tokens.")
    if control:
        by_class = {pt["class"]: pt for pt in control["points"] if pt["set"] == "original"}
        shown = [cid for _, cid, _ in PUBLIC_CLASSES if cid in by_class]
        if len(shown) >= 2:
            used = [by_class[cid]["tokens"] for cid in shown]

            def listed(items: list[str]) -> str:
                return items[0] if len(items) == 1 else ", ".join(items[:-1]) + " and " + items[-1]
            headline.append(
                "With no index, in these runs, one agent used "
                + listed([f"{tokens(by_class[cid]['tokens'])} tokens on the {cid} class ({by_class[cid]['runs']} runs)" for cid in shown])
                + " on the original questions." + (" The larger model class did not need far fewer tokens for the same lookups."
                                                   if min(used) >= 0.7 * max(used) else ""))
    findings = {"headline": headline}

    # Held-out: who stayed fully correct on both sets, and the best configuration against the first generation and
    # against no index, in tokens and turns.
    both_sets = [c for c in configs if {"original", "held-out"} <= set(c["sets"])]
    if both_sets:
        steady = [c for c in both_sets if all(pt["perfect_runs"] == pt["runs"] for pt in c["points"])]
        text = [(f"Of the {len(both_sets)} configurations measured on both question sets, {quoted([c['name'] for c in steady])} "
                 f"{'was' if len(steady) == 1 else 'were'} fully correct in every run, in these runs." if steady
                 else f"None of the {len(both_sets)} configurations measured on both question sets was fully correct in every run.")]
        rows = []
        held = {config["slug"]: [cell_of(config, cid, "held-out") for cid in ("light", "mid")]
                for config in (best, first, control) if config}
        if best and first and best is not first and all(pt for config in (best, first) for pt in held[config["slug"]]):
            def figures(pts: list[dict]) -> str:
                return (f"{tokens(pts[0]['tokens'])} tokens and {pts[0]['turns']:.1f} turns on the light class and "
                        f"{tokens(pts[1]['tokens'])} and {pts[1]['turns']:.1f} on the mid class")
            text.append(f"On the held-out questions \"{best['name']}\" needed {figures(held[best['slug']])}; "
                        f"\"{first['name']}\" needed {figures(held[first['slug']])}.")
            if control and all(held[control["slug"]]):
                saved = [1 - b["tokens"] / c["tokens"] for b, c in zip(held[best["slug"]], held[control["slug"]])]
                text.append(f"No index needed {figures(held[control['slug']])}, so on the held-out questions \"{best['name']}\" used "
                            f"{saved[0]:.0%} fewer tokens on the light class and {saved[1]:.0%} fewer on the mid class.")
            original = [cell_of(best, cid, "original") for cid in ("light", "mid")]
            if all(original) and any(h["turns"] > o["turns"] + 0.05 for h, o in zip(held[best["slug"]], original)):
                text.append(f"These are higher than on the original questions, where \"{best['name']}\" needed "
                            f"{tokens(original[0]['tokens'])} tokens and {original[0]['turns']:.1f} turns on the light class and "
                            f"{tokens(original[1]['tokens'])} and {original[1]['turns']:.1f} on the mid class.")
            for label, config in ((BEST_LABEL, best), ("First generation", first)):
                for (_, cid, cname), pt in zip(PUBLIC_CLASSES[:2], held[config["slug"]]):
                    rows.append((f"{label}, {cid} class",
                                 f"{tokens(pt['tokens'])} tokens, {pt['turns']:.1f} turns, {of(pt['perfect_runs'], pt['runs'])} fully correct"))
            if control and all(held[control["slug"]]):
                for (_, cid, cname), pt in zip(PUBLIC_CLASSES[:2], held[control["slug"]]):
                    rows.append((f"No index, {cid} class", f"{tokens(pt['tokens'])} tokens, {pt['turns']:.1f} turns, {pt['runs']} runs"))
        findings["held_out"] = {"title": "Held-out questions", "text": " ".join(text), "rows": rows,
                                "steady": [c["name"] for c in steady], "measured_on_both": len(both_sets)}

    # Parts: what search alone and graph alone do, on the callers question and in the four cells.
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
            pairs = [(cid, sid, cell_of(graph, cid, sid), cell_of(joint, cid, sid)) for cid, sid in BEST_CELLS]
            if all(g and j for _, _, g, j in pairs):
                g_turns, j_turns = (statistics.fmean(p[n]["turns"] for p in pairs) for n in (2, 3))
                g_tokens, j_tokens = (statistics.fmean(p[n]["tokens"] for p in pairs) for n in (2, 3))
                fewer = sum(g["tokens"] < j["tokens"] for _, _, g, j in pairs)
                level = sum(g["tokens"] >= j["tokens"] and g["tokens"] <= 1.02 * j["tokens"] for _, _, g, j in pairs)
                text.append(
                    f"Over the {number_word(len(pairs))} cells, graph alone averaged {g_turns:.1f} turns and {tokens(g_tokens)} tokens, and "
                    f"graph and search together {j_turns:.1f} turns and {tokens(j_tokens)}. Cell by cell, graph alone used fewer tokens in "
                    f"{of(fewer, len(pairs))} cells and within 2% of graph and search in {of(level, len(pairs))} more.")
                for cid, sid, g, j in pairs:
                    rows.append((f"Graph alone, {cid} class, {sid}", f"{tokens(g['tokens'])} tokens, {g['turns']:.1f} turns"))
                    rows.append((f"Graph and search, {cid} class, {sid}", f"{tokens(j['tokens'])} tokens, {j['turns']:.1f} turns"))
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
                    f"and used {kilo(min(gaps))} to {kilo(max(gaps))} more tokens than graph and search without memory. That is a "
                    "best case, since the memory held the answers.")
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
                + f"; it was fully correct in {of(*mine[:2])} runs, against {of(*theirs[:2])} without memory. With three runs in a "
                "cell, differences of this size are inside the spread between repeats.")
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
        parts = [f"One call that answers all three lookups took {fast['query_ms']:g} ms with \"{fast['name']}\" and "
                 f"{slow['query_ms']:g} ms with \"{slow['name']}\", {times:.0f} times as long."]
        if fast.get("lookup_ms") and slow.get("lookup_ms"):
            parts.append(f"A single lookup took {fast['lookup_ms']:g} ms and {slow['lookup_ms']:g} ms, "
                         f"{slow['lookup_ms'] / fast['lookup_ms']:.0f} times as long.")
        build = (f"Building the index took {fast['build_seconds']:g} s against {slow['build_seconds']:g} s, both with embeddings")
        if slow.get("build_seconds_no_embeddings"):
            build += f"; \"{slow['name']}\" took {slow['build_seconds_no_embeddings']:g} s without them"
        parts.append(build + f". The index is {fast['index_mb']:g} MB against {slow['index_mb']:g} MB, and the second figure leaves "
                     "out a cache that engine shares across repositories.")
        parts.append("Times are medians of 20 whole-process runs on one machine on one day"
                     + ("; the build time of the first generation was measured in an earlier session." if "first generation" in slow["name"].lower() else "."))
        findings["speed"] = {
            "title": "Speed",
            "text": " ".join(parts),
            "rows": [(f"{r['name']}, three lookups in one call", f"{r['query_ms']:g} ms") for r in (fast, slow)]
                    + ([(f"{r['name']}, one lookup", f"{r['lookup_ms']:g} ms") for r in (fast, slow)] if fast.get("lookup_ms") and slow.get("lookup_ms") else [])
                    + [(f"{r['name']}, build time", f"{r['build_seconds']:g} s") for r in (fast, slow)]
                    + [(f"{r['name']}, index size", f"{r['index_mb']:g} MB") for r in (fast, slow)]}

    # The proof on the Overview: fully correct runs across both question sets and both model classes, for the best
    # configuration against the first generation. The control is left out: its correctness was lowered by another tool.
    if best and first and best is not first:
        counted = {"best": (best, cell_counts(best)), "first_generation": (first, cell_counts(first))}
        if all(found and found[2] == len(BEST_CELLS) for _, found in counted.values()):
            sums = {key: {"name": config["name"], "perfect_runs": found[0], "runs": found[1]} for key, (config, found) in counted.items()}
            text = (f"Across both question sets and both model classes, in these runs, one agent using \"{best['name']}\" was fully "
                    f"correct in {of(sums['best']['perfect_runs'], sums['best']['runs'])} runs, against "
                    f"{of(sums['first_generation']['perfect_runs'], sums['first_generation']['runs'])} with \"{first['name']}\".")
            held_cells = tuple(cell for cell in BEST_CELLS if cell[1] == "held-out")
            held_counts = {key: cell_counts(config, held_cells) for key, (config, _) in counted.items()}
            if all(found and found[2] == len(held_cells) for found in held_counts.values()):
                text += (f" On the held-out questions alone, {of(*held_counts['best'][:2])} against {of(*held_counts['first_generation'][:2])}"
                         + (", though the held-out set is not held out for a round-six configuration." if best.get("session") == "team" else "."))
            findings["proof"] = dict(sums, text=text)

    # Orchestration: a reviewing agent on top of three light workers, against one light agent alone.
    def complete(setup: str, among: set | None = None) -> list[dict]:
        return [c for c in cells if c["complete"] and c["setup"] == setup and c["tier"] == "C"
                and c["arm"] not in hook_test and (among is None or c["arm"] in among)]

    orchestrated = complete("orch")
    alone = complete("single", {c["arm"] for c in orchestrated})
    pairs = [r for r in orchestration["setups"] if r["tier"] == "C" and r["single"] and r["orch"]]
    if orchestrated and alone and pairs:
        right_orch = sum(c["quality"] >= 0.999 for c in orchestrated)
        right_alone = sum(c["quality"] >= 0.999 for c in alone)
        cost = lambda side: (sum(r[side]["cost_warm"] * r[side]["n"] for r in pairs) / sum(r[side]["n"] for r in pairs))
        times = cost("orch") / cost("single")
        sets = sorted({SET_OF.get(c.get("set", "A"), "original") for c in orchestrated})
        imputed = sum(bool(c.get("plan_imputed")) for c in orchestrated)
        reviewer = next((t["id"] for t in arms["tiers"] if t["model"].lower().startswith(str(orchestrated[0].get("reviewer")).lower())), None)
        larger = reviewer is not None and TIER_ORDER.index(reviewer) > TIER_ORDER.index("C")
        findings["orchestration"] = {
            "title": "Orchestration",
            "text": (f"With a reviewing agent on top of three light workers, {of(right_orch, len(orchestrated))} runs ended fully correct, "
                     f"against {of(right_alone, len(alone))} for one light agent alone, on the {' and '.join(sets)} questions. "
                     + ("The reviewing agent is of a larger model class than the workers, so this compares more than orchestration. " if larger else "")
                     + f"A run cost about {times:.0f} times as much. That is an estimate: a price list applied to the measured tokens, "
                     f"pooled over {len(pairs)} configurations, and for {of(imputed, len(orchestrated))} orchestrated runs the planning cost "
                     "is itself estimated from the planning calls that were measured."),
            "orchestrated": {"perfect_runs": right_orch, "runs": len(orchestrated)},
            "alone": {"perfect_runs": right_alone, "runs": len(alone)}, "cost_multiple": times,
            "planning_estimated": imputed}
    effect = hook_effect(hooks, arms)
    if effect:
        first_part = ("A tool on the test machine that alters what an agent reads lowered token use on these lookups"
                      if effect["saved"] else "A tool on the test machine that alters what an agent reads did not lower token use on these lookups")
        findings["hooks"] = {
            "title": "A tool that alters reads",
            "text": (f"{first_part} ({effect['change']:+.0%}, against {effect['spread']:.0%} between repeats of one prompt). Without an "
                     f"index, {of(effect['right_with'], effect['runs_with'])} runs with it ended fully correct and "
                     f"{of(effect['right_without'], effect['runs_without'])} without it: too few runs to call that a clear effect. The same "
                     "tool altered some of the no-index control's reads in the second session, which is why the control's correctness is "
                     "not set against the others."),
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
    def means(arm_id: str) -> dict:
        found = {}
        for tier, cid, _ in PUBLIC_CLASSES:
            pt = pick(points, arm_id, "single", tier)
            if pt:
                found[cid] = [pt["tokens"], pt["requests"], pt["n"]]
        return found

    if today and first_session:
        facts["control"] = {"today": {"original": perfect(today["id"]), "held-out": perfect(held["id"]) if held else {}},
                            "earlier": perfect(first_session["id"]),
                            "tokens": {"today": means(today["id"]), "earlier": means(first_session["id"])}}
    return facts


def study_facts(configs: list[dict], cells: list[dict], points: list[dict], owner: dict, arms: dict, hook_test: set) -> dict:
    """What the Study page quotes besides its scale rows, all counted from the published configurations and their
    runs: how many single-agent runs stand behind the leaderboard and in cells of what size, how far repeated runs of
    one cell differ, what a token count is made of, which question separates the configurations, and how many
    unpublished configurations reached the same two turns."""
    import collections
    best = next((c for c in configs if c["best"]), None)
    control = next((c for c in configs if c["family"] == "control"), None)
    graph = next((c for c in configs if c["slug"] == owner.get("RG")), None)
    published = [pt for c in configs for pt in c["points"]]
    sizes = collections.Counter(pt["runs"] for pt in published)
    facts = {"behind": {"runs": sum(pt["runs"] for pt in published), "cells": len(published),
                        "by_size": {str(n): sizes[n] for n in sorted(sizes)}}}
    repeated = [pt for pt in published if pt["runs"] >= 2]
    ratios = sorted(pt["tokens_high"] / pt["tokens_low"] - 1 for pt in repeated)
    facts["spread"] = {
        "cells": len(repeated), "median": statistics.median(ratios) if ratios else 0.0, "highest": ratios[-1] if ratios else 0.0,
        "wide": sum(r >= 0.5 for r in ratios), "mixed": sum(0 < pt["perfect_runs"] < pt["runs"] for pt in published),
        "turn": statistics.fmean(pt["tokens"] / pt["turns"] for pt in published),
        "best_cells": [{"class": pt["class"], "set": pt["set"], "runs": pt["runs"], "low": pt["tokens_low"], "high": pt["tokens_high"]}
                       for cid, sid in BEST_CELLS for pt in [cell_of(best, cid, sid)] if pt]}
    runs = [c for c in cells if c["complete"] and c["setup"] == "single" and c["arm"] in owner]

    def context(config: dict | None, cid: str, sid: str) -> float | None:
        found = [c for c in runs if config and owner[c["arm"]] == config["slug"] and c["tier"] == TIER_OF_CLASS[cid]
                 and SET_OF[c.get("set", "A")] == sid]
        return statistics.fmean((c["start"] + c["reread"]) / c["tokens"] for c in found) if found else None
    facts["tokens"] = {
        "runs": len(runs), "cached": sum(c["cache_read"] + c["cache_write"] for c in runs) / sum(c["tokens"] for c in runs),
        "start": statistics.fmean(c["start"] for c in runs),
        "context": {"best": context(best, "light", "original"), "control": context(control, "light", "original")}}
    imperfect = [c for c in runs if c["quality"] < 0.999]
    full_purpose = lambda rows: sum((c["scores"]["q3"] or 0) >= 0.999 for c in rows)
    no_search = [c for c in runs if owner[c["arm"]] in {cfg["slug"] for cfg in (control, graph) if cfg}]
    # How often the other tool acted in the second session: on the control's runs and on all the other published ones.
    second = {a["id"] for a in arms["arms"] if a.get("group") in ("rust", "heldout", "rerun")}
    in_control = lambda c: bool(control) and owner[c["arm"]] == control["slug"]
    acted = lambda rows: [sum(c["hook_calls"] > 0 for c in rows), len(rows)]
    facts["tool_acted"] = {"control": acted([c for c in runs if c["arm"] in second and in_control(c)]),
                           "others": [{"name": config["name"], "acted": acted([c for c in runs if c["arm"] in second and owner[c["arm"]] == config["slug"]])}
                                      for config in configs if config["family"] != "control"
                                      if any(c["arm"] in second and owner[c["arm"]] == config["slug"] for c in runs)]}
    facts["graded"] = {"runs": len(runs), "imperfect": len(imperfect),
                       "callers": sum((c["scores"]["q1"] or 0) < 0.999 for c in imperfect),
                       "purpose": [full_purpose(runs), len(runs)], "purpose_no_search": [full_purpose(no_search), len(no_search)]}
    base_of = lambda a: a.get("base", a["id"])
    published_ids = set(owner) | {base_of(a) for a in arms["arms"] if a["id"] in owner}
    tried = [p for p in points if p["setup"] == "single" and p["tier"] == "C" and p["set"] == "A" and p["n"] >= 3
             and p["arm"] not in published_ids and p["arm"] not in hook_test]
    facts["unpublished"] = {"measured": len(tried),
                            "two_turns": sum(p["requests"] <= 2.05 and p["spread"]["perfect_runs"] == p["n"] for p in tried)}
    return facts


def install_of() -> dict:
    """The public install block: benchmark/data/install.json "public". It holds the release links, the two installers, the
    program files and wheels with their sizes, the first commands, the rule an agent is given, and what was tested. The
    install page and the commands on the other pages are built from it, so a new release is one edit of that file."""
    path = BENCH / "data" / "install.json"
    if not path.is_file():
        raise SystemExit("benchmark/data/install.json is missing: the install page is built from it")
    install = json.loads(path.read_text(encoding="utf-8")).get("public") or {}
    if install.get("status") != "public":
        raise SystemExit("install.json: public.status must be 'public'")
    needed = ("version", "repository", "release_page", "download_base", "versioned_base", "installers", "programs", "wheels",
              "support", "first_commands", "agent_rule", "tested", "licence")
    missing = [key for key in needed if key not in install]
    if missing:
        raise SystemExit(f"install.json: public lacks {missing}")
    base = str(install["repository"])
    for key in ("release_page", "download_base", "versioned_base"):
        if not str(install[key]).startswith(base + "/"):
            raise SystemExit(f"install.json: public.{key} must be a link under the repository, {base}")
    ids = [item.get("id") for item in install["installers"]]
    if ids != ["windows", "unix"]:
        raise SystemExit("install.json: public.installers must be the Windows one and then the macOS and Linux one")
    if install.get("command") != install["installers"][0]["command"]:
        raise SystemExit("install.json: public.command must be the Windows installer's command")
    for group in ("programs", "wheels", "support"):
        for item in install[group]:
            if not isinstance(item.get("name"), str) or not isinstance(item.get("bytes"), int):
                raise SystemExit(f"install.json: every entry of public.{group} needs a name and a size in bytes")
    return install


def engine_rows() -> list[dict]:
    """The public engine speed rows: benchmark/data/engine.json "public". A row that names its measurement with
    "from" (a key of "detail") takes its figures from that measurement: the build time, the time of one call that
    answers all three lookups, the time of one lookup and the index size. Without the file there are no rows."""
    path = BENCH / "data" / "engine.json"
    if not path.exists():
        return []
    data = json.loads(path.read_text(encoding="utf-8"))
    detail = data.get("detail") or {}
    rows = []
    for row in data.get("public") or []:
        measured = detail.get(row.get("from", ""), {})
        merged = {"name": row.get("name"),
                  "build_seconds": measured.get("build_seconds", row.get("build_seconds")),
                  "query_ms": measured.get("ask_ms", row.get("query_ms")),
                  "lookup_ms": measured.get("about_ms", row.get("lookup_ms")),
                  "index_mb": round(measured["index_mb"], 1) if "index_mb" in measured else row.get("index_mb"),
                  "build_seconds_no_embeddings": measured.get("build_seconds_no_embeddings", row.get("build_seconds_no_embeddings")),
                  "build_note": row.get("build_note")}
        missing = [key for key in ("name", "build_seconds", "query_ms", "index_mb") if merged[key] is None]
        if missing:
            raise SystemExit(f"engine.json: a public row lacks {missing}")
        rows.append({key: value for key, value in merged.items() if value is not None})
    return rows


VIDEO_KEYS = ("src", "poster", "captions", "width", "height", "seconds", "title")
VIDEO_KINDS = ("trailer", "explainer")   # in the order the page shows them: the short film first


def videos_of() -> list[dict]:
    """The public footage: benchmark/data/video.json "public" when that file exists, one film or a list of films;
    otherwise nothing, and then the page has no footage section. The files a film names must exist under
    docs/media/. A film says what it is with "kind"; one that does not is an explainer. Each film carries the files it
    can be played from (see film_sources), and a larger poster when one is beside the poster it names."""
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
        film = {"kind": kind, **{key: video[key] for key in VIDEO_KEYS}}
        film["sources"] = film_sources(video, where)
        larger = str(video["poster"]).replace("-poster.", "-poster-2160.")
        if larger != video["poster"] and (DOCS / larger).is_file():
            film["poster_large"] = larger
        # The length the page shows is the length of the file, not a value typed into video.json that can fall behind it.
        measured = mp4_seconds(DOCS / str(video["src"]))
        if measured is not None and abs(measured - float(video["seconds"])) > 0.05:
            print(f"film {video['src']}: video.json says {video['seconds']} s, the file is {measured:.1f} s; the page shows the file's")
            film["seconds"] = round(measured, 1)
        film["title"] = film_name(film)
        films.append(film)
    return sorted(films, key=lambda film: VIDEO_KINDS.index(film["kind"]))


def mp4_seconds(path: pathlib.Path) -> float | None:
    """The duration an MP4 file declares in its movie header, or None when the file holds no readable header. The
    top-level boxes are walked, so a header at the end of the file is found and bytes inside the media data are not
    mistaken for one."""
    import struct
    try:
        with path.open("rb") as handle:
            end = path.stat().st_size

            def boxes(start: int, stop: int):
                at = start
                while at + 8 <= stop:
                    handle.seek(at)
                    size, kind = struct.unpack(">I4s", handle.read(8))
                    head = 8
                    if size == 1:
                        size, head = struct.unpack(">Q", handle.read(8))[0], 16
                    elif size == 0:
                        size = stop - at
                    if size < head:
                        return
                    yield kind, at + head, min(at + size, stop)
                    at += size
            for kind, body, stop in boxes(0, end):
                if kind != b"moov":
                    continue
                for inner, start, _ in boxes(body, stop):
                    if inner == b"mvhd":
                        handle.seek(start)
                        version = handle.read(1)[0]
                        handle.seek(start + (20 if version == 1 else 12))
                        scale, length = struct.unpack(">IQ", handle.read(12)) if version == 1 else struct.unpack(">II", handle.read(8))
                        return length / scale if scale else None
    except (OSError, struct.error):
        return None
    return None


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
    held = [a for a in arms["arms"] if a.get("group") == "heldout" or (a.get("group") == "team" and a.get("base"))]
    configs, owner, claimed, not_repeated = [], {}, set(), []
    for arm in sorted((a for a in arms["arms"] if public_arm(a)), key=lambda a: public_arm(a)["order"]):
        spec = public_arm(arm)
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
                if found and found["set"] in SET_OF:
                    rows.append(public_point(found, class_of[tier]))
        keys = [(row["class"], row["set"]) for row in rows]
        if len(keys) != len(set(keys)):
            raise SystemExit(f"arms.json: {spec['slug']} has two measured points on one class and question set")
        if rows:
            rows.sort(key=lambda row: (set_order.index(row["set"]), class_order.index(row["class"])))
            configs.append({"slug": spec["slug"], "name": spec["name"], "family": spec["family"], "best": False,
                            # the agent answers from a pack in its prompt and may call no command and no tool
                            "commandless": arm.get("uses") == ["pack_only"],
                            # "day": measured on the day of the second session; "earlier": kept from the first session
                            "session": "team" if arm.get("group") == "team" else "day" if arm.get("group") in ("rust", "rerun") else "earlier",
                            "sets": [sid for sid in set_order if any(row["set"] == sid for row in rows)], "points": rows})
            if arm.get("group") not in ("rust", "rerun", "team"):
                not_repeated.append(spec["name"])
    best, rule = best_config(configs)
    for config in configs:
        config["best"] = config["slug"] == best
    system = re.search(r"\b(Windows \d+|macOS [\w.]+|Ubuntu [\d.]+|Linux)\b", index["dataset"])
    match = re.search(r"(\d+) Python files", index["dataset"])
    if not match:
        raise SystemExit("index.json: the dataset text no longer says how many Python files the repository has")
    complete = [c for c in cells if c["complete"]]
    per_cell = [p["n"] for p in points if p["arm"] not in hook_test]
    measured = {p["arm"] for p in points if p["arm"] not in hook_test}
    # A held-out or repeated arm is the same configuration as its base, so it is counted once.
    tested = {base_of(a) for a in arms["arms"] if a["id"] in measured}
    held_points = [pt for c in configs for pt in c["points"] if pt["set"] == "held-out"]
    engine = engine_rows()
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
        "labels": {"best": BEST_LABEL, "rule": best_rule(rule), "control_note": CONTROL_NOTE,
                   "control_link_text": CONTROL_LINK_TEXT, "control_link": CONTROL_LINK, "earlier": EARLIER_LABEL, "team": TEAM_LABEL},
        "versus": versus_control(configs),
        # The line under each leaderboard view, computed once and read by the page and by its script.
        "view_notes": {f"{cid}|{sid}": view_note(ordered_rows(configs, cid, sid))
                       for sid in set_order for cid in class_order},
        "configs": configs,
        "findings": public_findings(configs, cells, orchestration, hooks, arms, hook_test, owner, engine)}
    public["study"].update(study_facts(configs, cells, points, owner, arms, hook_test))
    public["pick"] = pick_facts(configs)
    public["team"] = team_facts(points, cells, agents, arms)
    if system:
        public["study"]["system"] = system.group(1)
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
    shown = modelize_all({key: value for key, value in public.items() if key != "install"})   # the install page is written from it, the scripts do not read it
    body = json.dumps(rounded(shown), ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/")
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
    items = "".join(f'\n      <li class="row"><span class="label">{esc(label)}</span><span class="value">{glue(value)}</span></li>'
                    for label, value in rows)
    return f'<ul class="rows">{items}\n    </ul>'


def text_rows_html(rows: list[tuple[str, str]]) -> str:
    items = "".join(f'\n      <li class="row stack"><span class="label">{esc(title)}</span>'
                    f'<span class="detail">{esc(glue(text))}</span></li>' for title, text in rows)
    return f'<ul class="rows">{items}\n    </ul>'


def index_words(public: dict) -> tuple[str, list[tuple[str, str]]]:
    """The words of the Overview's opening, in two parts. The lead says plainly what the product is, what it works with and what
    it was measured under, and is the first thing read on the first screen. The key is what a reader needs to take the numbers:
    where they come from, what a token and a turn are, what the classes and the question sets are."""
    behind = public["study"]["behind"]
    common = max(behind["by_size"], key=lambda size: behind["by_size"][size])
    most = behind["by_size"][common] * 2 > behind["cells"]
    lead = (f"{BRAND} (pgr) is a desktop command-line tool for AI coding agents, written in Rust. The agent asks it where a function "
            "is defined and what calls it, and gets the answer in one step. It works with any agent that can run a command.")
    key = [("Evidence", "The results on this site come from one repository, a Python project: "
                        f"{behind['runs']} runs of a single agent in {behind['cells']} cells, {number_word(int(common))} runs in "
                        f"{'most of them' if most else 'the commonest cell size'}. The engine itself is written in Rust."),
           ("Tokens and turns", "A token count is all the text the model read or wrote over a task, including the conversation it reads "
                                "again on every turn, and a turn is one round trip to the model."),
           ("Models", models_line()),
           ("Questions", "The original questions are the three lookups used in every round; the held-out questions are a second set "
                         "of three.")]
    return lead, key


def lede_note_slot(public: dict) -> str:
    """The line under the lede of the Overview: that it runs only on the visitor's machine, and what it was measured under."""
    return f"It runs only on your own machine, with no account, no server and no network use. {harness_line()}"


def key_slot(public: dict) -> str:
    items = "".join(f'\n    <div><dt>{esc(name)}</dt><dd>{esc(text)}</dd></div>' for name, text in index_words(public)[1])
    return f'<dl class="key" aria-label="How to read these results">{items}\n  </dl>'


def watch_slot(public: dict) -> str:
    """The link at the top of the Overview to the shortest film, with its length from the data."""
    films = public.get("videos") or ([public["video"]] if public.get("video") else [])
    if not films:
        return ""
    first = films[0]
    return f'<a class="btn ghost" href="#films">Watch the {VIDEO_KIND_TEXT.get(first.get("kind", "explainer"), "film").lower()}, {first["seconds"]:.0f} seconds</a>'


def lede_slot(page: str, public: dict) -> str:
    if page == "index.html":
        return index_words(public)[0]
    if page == "benchmarks.html":
        return ("Tokens, turns and mean score for each published configuration, measured on the same three lookups and on a second "
                "set of three questions of the same shapes. Pick a model and a question set to compare.")
    if page == "study.html":
        return "How the measurements were made, what they cover and what they leave out."
    return (f"{BRAND} (pgr) is a desktop command-line tool for AI coding agents, for Windows, macOS and Linux. It works with any agent "
            f"that can run a command, and it is written in Rust. {harness_line()} One paste installs it.")


GLUE = re.compile(r"(?<=\d) (?=(?:ms|s|MB|kB|k|M|%|times|seconds|turns|tokens|runs|run|files|lines|of)\b)|(?<=\d[kM]) (?=to \d)|(?<= to) (?=\d)|(?<=\d) (?=of \d)|(?<= of) (?=\d)")


def glue(text: str) -> str:
    """No-break spaces between a number and its unit, and inside "6 of 6" and "134k to 204k", so a narrow column
    breaks a line between values and never inside one. Only the text between tags is touched."""
    parts = re.split(r"(<[^>]*>)", text)
    return "".join(part if part.startswith("<") else GLUE.sub(" ", part) for part in parts)


def lane_html(kind: str, name: str, text: str, value: float) -> str:
    return (f'<span class="rl-lane {kind}"><span class="rl-name">{esc(name)}</span><span class="rl-num">{esc(text)}</span>'
            f'<span class="rl-bar"><i style="--v:{max(0.0, min(1.0, value)):.4f}"></i></span></span>')


def figures_slot(public: dict) -> str:
    found = headline_of(public)
    if not found:
        return '<p class="note">No configuration has been run enough times on the light class to be compared yet.</p>'
    b, c = found["b"], found["c"]
    best, control = found["best"], found["control"]
    first = first_generation(public["configs"])
    f = cell_of(first, "light", "original") if first and first is not best else None
    versus = public.get("versus")
    held = [cell_of(best, "light", "held-out"), cell_of(control, "light", "held-out")]
    turns_note = f"No index: {c['turns']:.1f}" if c else ""
    if c and all(held):
        turns_note += f". Held-out questions: {held[0]['turns']:.1f}, no index {held[1]['turns']:.1f}"
    # Each measure is a pair of lanes drawn to one scale, the comparison above and the best configuration below. Tokens and
    # turns are set against no index. Fully correct runs and the mean score are set against the first generation only:
    # another tool on the test machine altered the control's reads. The number beside each pair counts from the
    # comparison's value to the best one's when its row is lit, and the number is written finished.
    name, other = best["name"], (first["name"] if f else "")
    rows = [("Tokens per task", tokens(b["tokens"]),
             f"No index: {tokens(c['tokens'])}, {1 - b['tokens'] / c['tokens']:.0%} fewer in this cell" if c else "",
             "",
             [("ref", "No index", tokens(c["tokens"]), c["tokens"] / max(b["tokens"], c["tokens"])),
              ("best", name, tokens(b["tokens"]), b["tokens"] / max(b["tokens"], c["tokens"]))] if c else []),
            ("Turns per task", f"{b['turns']:.1f}", turns_note,
             "",
             [("ref", "No index", f"{c['turns']:.1f}", c["turns"] / max(b["turns"], c["turns"])),
              ("best", name, f"{b['turns']:.1f}", b["turns"] / max(b["turns"], c["turns"]))] if c else []),
            ("Fully correct runs", of(b["perfect_runs"], b["runs"]), f"{first['name']}: {of(f['perfect_runs'], f['runs'])}" if f else "", "",
             [("ref", other, of(f["perfect_runs"], f["runs"]), f["perfect_runs"] / f["runs"]),
              ("best", name, of(b["perfect_runs"], b["runs"]), b["perfect_runs"] / b["runs"])] if f else []),
            ("Mean score", share(b["correct"]), f"{first['name']}: {share(f['correct'])}" if f else "", "",
             [("ref", other, share(f["correct"]), f["correct"]), ("best", name, share(b["correct"]), b["correct"])] if f else [])]
    items = "".join(
        f'\n      <div class="rl-row"><dt class="rl-label">{esc(label)}</dt><dd class="rl-value"{attrs}>{esc(value)}</dd>'
        + (f'<dd class="rl-note">{esc(glue(note))}</dd>' if note else "")
        + (('<dd class="rl-fig" aria-hidden="true">' + "".join(lane_html(*lane) for lane in lanes) + "</dd>") if lanes else "") + "</div>"
        for label, value, note, attrs, lanes in rows)
    group = (f"{BEST_LABEL} configuration, light model class, original questions, one agent answering three lookups"
             + (". Tokens and turns are set against no index, fully correct runs and mean score against the first generation" if c and f else ""))
    lead = ""
    if c and c["tokens"] > b["tokens"]:
        saving = 1 - b["tokens"] / c["tokens"]
        across = (f"Across the {number_word(len(versus['cells']))} cells measured: {saving_range(versus)}."
                  if versus and len(versus["cells"]) > 1 else "")
        lead = (f'\n    <div class="saving-lead"><p class="lead-kicker">Fewer tokens than no index</p>'
                f'<div class="ring"><svg viewBox="0 0 100 100" aria-hidden="true" focusable="false"><circle class="ring-track" cx="50" cy="50" r="44"/>'
                f'<circle class="ring-fill" cx="50" cy="50" r="44" pathLength="1" style="--v:{saving:.4f}"/></svg>'
                f'<p class="lead-num"><b>{esc(share(saving))}</b></p></div>'
                '<p class="lead-cap">In this one cell: the light class, the original questions.</p>'
                + (f'<p class="lead-range">{esc(across)}</p>' if across else "")
                + f'<div class="gauge" aria-hidden="true"><span class="g-track"><i class="g-fill" style="--v:{b["tokens"] / c["tokens"]:.4f}"></i></span>'
                f'<span class="g-labels"><span>{esc(tokens(b["tokens"]))} with pgr</span><span>{esc(tokens(c["tokens"]))} no index</span></span></div></div>')
    return (f'<p class="group-title">{esc(group)}</p>\n    <div class="saving-grid">{lead}\n    <dl class="rl">{items}\n    </dl>\n    </div>')


def notes_slot(public: dict) -> str:
    """The two notes that qualify the headline figures, set under the chapter that shows them: that the saving is for one cell
    and what the range over the cells is, and why fully correct runs and the mean score are not set against no index."""
    found = headline_of(public)
    if not found:
        return ""
    best, c = found["best"], found["c"]
    versus = public.get("versus")
    notes = ""
    if versus and len(versus["cells"]) > 1:
        by_cell = "; ".join(f"{cell['class']} class, {cell['set']} questions, {cell['saving']:.0%}" for cell in versus["cells"])
        notes += (f'\n      <p class="footnote">This is one cell. Across the {number_word(len(versus["cells"]))} cells where "{esc(best["name"])}" and '
                  f'no index were both measured, the saving in tokens was {saving_range(versus)} ({esc(by_cell)}).</p>')
    if c:
        notes += (f'\n      <p class="footnote">Fully correct runs and mean score are not set against no index: another tool on the test machine '
                  f'altered some of its reads, which lowered its correctness. <a href="{CONTROL_LINK}">Why this matters</a>.</p>')
    return f'<div class="fine">{notes}\n    </div>' if notes else ""


def proof_html(public: dict) -> str:
    """The result under the headline figures: fully correct runs across both question sets and both model classes, for
    the best configuration and the first-generation engine. It counts both sets, so it says so in its heading, and it
    describes the held-out set as it is."""
    proof = public["findings"].get("proof")
    if not proof:
        return ""
    return ('\n    <p class="group-title spaced">Fully correct runs over both question sets</p>\n    '
            f'<p class="proof-text">{esc(glue(proof["text"]))}</p>')


def proof_note(public: dict) -> str:
    """The sentence under the count of fully correct runs: which cells it covers, and what the held-out set is."""
    held = next((s for s in public["sets"] if s["id"] == "held-out"), None)
    about = f" The held-out questions are {held['text'][0].lower() + held['text'][1:]}" if held else ""
    if any(c.get("session") == "team" for c in public["configs"]):
        about += (f" Round six ran on {ROUND_SIX_DATE}, a day after round five. Its rows are set against their own baseline in the Round six chapter, "
                  "except the highlighted one when it is a round-six row: its no-index figure is from round five.")
    return ('<p class="footnote">Counted over four cells: the light and mid classes, on the original and the held-out questions.'
            f'{esc(about)}</p>') if public["findings"].get("proof") else ""


def best_rule(rule: str | None = "cells") -> str:
    """One plain sentence: how the highlighted configuration is chosen, that the rule was set after the runs, and which
    configurations could be chosen."""
    label = f'"{BEST_LABEL}"'
    if rule == "light":
        return (f"{label} marks the configuration with the highest share of fully correct runs on the light class and the original "
                f"questions, among those run at least {BEST_MIN_RUNS} times there, a tie going to the fewer tokens; no configuration was "
                "measured on both question sets, and the rule was set after the runs were known.")
    return (f"{label} marks the one configuration with the highest share of fully correct runs over four cells (the light and mid "
            "classes, on the original and the held-out questions), a tie going to the lower mean tokens; only configurations run at "
            f"least {BEST_MIN_RUNS} times in each of those four cells, and so measured on both question sets, were eligible. The rule was set after "
            "the runs of rounds one to five were known and before round six ran.")


def range_plot(versus: dict | None) -> str:
    """The saving in tokens in each cell where the most reliable configuration and no index were both measured, on one axis: the
    range it spans is drawn as a bar and each cell as a dot, so the single cell shown elsewhere is seen as one of several."""
    if not versus or len(versus["cells"]) < 2:
        return ""
    cells = versus["cells"]
    lo, hi = min(cell["saving"] for cell in cells), max(cell["saving"] for cell in cells)
    top = 0.6 if hi <= 0.6 else -(-hi * 5 // 1) / 5
    ticks = "".join(f'<span style="--v:{n * 0.2 / top:.4f}">{round(n * 20)}%</span>' for n in range(int(top / 0.2 + 0.5) + 1))
    dots = "".join(f'<i class="rp-dot" style="--v:{cell["saving"] / top:.4f}"></i>' for cell in cells)
    legend = "".join(f'<li><b>{cell["saving"]:.0%}</b><span>{esc(cell["class"])} class, {esc(cell["set"])} questions</span></li>'
                     for cell in sorted(cells, key=lambda cell: -cell["saving"]))
    return (f'<div class="rp" role="img" aria-label="Fewer tokens than no index in each of {number_word(len(cells))} cells: {esc(saving_range(versus))}">'
            f'<div class="rp-axis" aria-hidden="true">{ticks}</div>'
            f'<div class="rp-track" aria-hidden="true"><span class="rp-span" style="--a:{lo / top:.4f};--b:{hi / top:.4f}"></span>{dots}</div></div>'
            f'<ul class="rp-legend">{legend}</ul>')


def best_slot(public: dict) -> str:
    """The most reliable configuration: its name, the saving in each cell where it was set against no index drawn on one axis, the
    facts about it that the headline figures above do not already carry, the count of fully correct runs over both question
    sets, and the sentence that says how it was chosen."""
    found = headline_of(public)
    if not found:
        return ('<p class="note">The most reliable configuration is named once one has been run at least '
                f'{BEST_MIN_RUNS} times on the light class and the original questions.</p>')
    best, pt = found["best"], found["b"]
    rows = [("Shown for", "Light class, original questions")]
    if range_text(pt):
        rows.append(("Tokens per task", esc(tokens(pt["tokens"])) + f' <span class="sub">{esc(range_text(pt))}</span>'))
    rows.append(("Runs", esc(pt["runs"])))
    four = cell_counts(best)
    if four and four[2] == len(BEST_CELLS):
        rows.append(("Fully correct, all four cells", esc(of(four[0], four[1]))))
    versus = public.get("versus")
    if found["c"]:
        rows.append(("Fewer tokens than no index, this cell", esc(f"{1 - pt['tokens'] / found['c']['tokens']:.0%}")))
    if versus and len(versus["cells"]) > 1:
        rows.append(("Fewer tokens than no index, across the cells measured", esc(saving_range(versus))))
    if best.get("session") == "team" and found["c"]:
        rows.append(("When no index ran", esc(f"Round five, a day before round six ({ROUND_SIX_DATE})")))
    items = "".join(f'\n      <div><dt>{esc(label)}</dt><dd>{glue(value)}</dd></div>' for label, value in rows)
    plot = range_plot(versus)
    return (f'<p class="rl-who"><span class="rl-who-name">{esc(best["name"])}</span><span class="tag">{esc(BEST_LABEL)}</span></p>\n'
            f'    {plot}\n    <dl class="ledger">{items}\n    </dl>{proof_html(public)}\n'
            f'    <div class="fine">{proof_note(public)}\n      <p class="footnote">{esc(best_rule(public.get("best_rule")))}</p>\n    </div>{pick_slot(public)}')


def board_row(config: dict, pt: dict) -> str:
    sub = f'<span class="sub">{esc(range_text(pt))}</span>' if range_text(pt) else ""
    tag = f' <span class="tag">{esc(BEST_LABEL)}</span>' if config["best"] else ""
    notes = ""
    if config["family"] == "control":
        notes += (f'<span class="sub row-note" style="white-space:normal">{esc(CONTROL_NOTE)} '
                  f'<a href="{CONTROL_LINK}">{esc(CONTROL_LINK_TEXT)}</a></span>')
    if config.get("session") == "earlier":
        notes += f'<span class="sub row-note" style="white-space:normal">{esc(EARLIER_LABEL)}</span>'
    if config.get("session") == "team":
        notes += f'<span class="sub row-note" style="white-space:normal">{esc(TEAM_LABEL)}</span>'
    if config.get("commandless"):
        notes += '<span class="sub row-note" style="white-space:normal">The agent has no command to call: the pack is in its prompt.</span>'
    return (f'\n        <tr><th scope="row" class="cfg"><span class="cfg-main"><span class="cfg-name">{esc(config["name"])}</span>{tag}{notes}</span></th>'
            f'<td data-label="Runs">{pt["runs"]}</td><td data-label="Tokens"><span class="v">{esc(tokens(pt["tokens"]))}{sub}</span></td>'
            f'<td data-label="Turns">{pt["turns"]:.1f}</td><td data-label="Mean score">{esc(share(pt["correct"]))}</td>'
            f'<td data-label="Fully correct runs">{esc(of(pt["perfect_runs"], pt["runs"]))}</td></tr>')


def ordered(public: dict, class_id: str, set_id: str) -> list[tuple[dict, dict]]:
    """Configurations with a point on the class and the question set: the highlighted one first, then fewest tokens."""
    return ordered_rows(public["configs"], class_id, set_id)


def views(public: dict) -> list[tuple[dict, dict]]:
    """Every pair of question set and model class, the sets in order and the classes in order within each."""
    return [(s, cls) for s in public["sets"] for cls in public["classes"]]


def view_title(cls: dict, qset: dict) -> str:
    return f"{cls['name']}, {qset['name'].lower()} questions"


def empty_html(cls: dict, qset: dict) -> str:
    return (f'<p class="note empty">No configuration was measured on {esc(cls["name"])} '
            f'with the {esc(qset["name"].lower())} questions.</p>')


def boards_slot(public: dict) -> str:
    out = []
    rule = public["labels"]["rule"]
    for qset, cls in views(public):
        rows = ordered(public, cls["id"], qset["id"])
        title = view_title(cls, qset)
        body = empty_html(cls, qset) if not rows else (
            '<div class="card">\n      <table class="board">\n        '
            f'<caption class="sr-only">{esc(title)}: runs, tokens, turns and mean score by configuration</caption>\n        '
            '<thead><tr><th scope="col">Configuration</th><th scope="col">Runs</th><th scope="col">Tokens</th>'
            '<th scope="col">Turns</th><th scope="col">Mean score</th><th scope="col">Fully correct runs</th></tr></thead>\n'
            '        <tbody>' + "".join(board_row(c, pt) for c, pt in rows) + '\n        </tbody>\n      </table>\n    </div>')
        note = public["view_notes"].get(f"{cls['id']}|{qset['id']}", "") if rows else ""
        out.append(f'<div class="panel board-panel{" on" if not out else ""}" data-class="{esc(cls["id"])}" data-set="{esc(qset["id"])}" '
                   f'role="group" aria-label="{esc(title)}, leaderboard">\n'
                   f'    <h3 class="panel-title">{esc(title)}</h3>\n'
                   f'    <p class="group-title">One agent answering three lookups. The {esc(BEST_LABEL.lower())} configuration comes first, '
                   f'then the rest by fewest tokens. {esc(rule)}</p>\n'
                   f'    {body}\n' + (f'    <p class="footnote view-note">{esc(note)}</p>\n' if note else "") + '  </div>')
    return ("\n  ".join(out) + '\n  <p class="footnote">What a token count is, and the limits of these runs, are on the '
            '<a href="study.html#tokens">Study</a> page.</p>')


def charts_slot(public: dict) -> str:
    out = []
    for qset, cls in views(public):
        rows = ordered(public, cls["id"], qset["id"])
        title = view_title(cls, qset)
        if not rows:
            inner = empty_html(cls, qset)
        else:
            lines = "".join(
                f'\n          <tr><th scope="row" class="cfg"><span class="cfg-name">{esc(c["name"])}</span></th>'
                f'<td data-label="Tokens">{esc(tokens(pt["tokens"]))}</td><td data-label="Mean score">{esc(share(pt["correct"]))}</td>'
                f'<td data-label="Wall time">{esc(round(pt["seconds"]))} s</td></tr>' for c, pt in rows)
            inner = ('<div class="card chart-card">\n      <div class="chart" data-chart="' + esc(cls["id"]) +
                     '" data-set="' + esc(qset["id"]) + '">'
                     '<p class="note chart-fallback">The chart needs JavaScript. The table view below has the same numbers.</p></div>\n      '
                     '<details class="table-view"><summary>Table view</summary>\n        <table class="board board-small">\n          '
                     f'<caption class="sr-only">{esc(title)}: tokens, mean score and wall time by configuration</caption>\n          '
                     '<thead><tr><th scope="col">Configuration</th><th scope="col">Tokens</th><th scope="col">Mean score</th>'
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
    return ('<div class="seg-bar">\n    ' + group("Model", "class", [(c["id"], c["name"]) for c in public["classes"]]) + '\n    '
            + group("Questions", "set", [(s["id"], s["name"]) for s in public["sets"]]) + '\n  </div>\n'
            f'  <p class="group-title seg-note" id="set-note">{notes.rstrip()}</p>\n'
            '  <p class="sr-only" id="seg-status" aria-live="polite"></p>')


def engine_slot(public: dict) -> str:
    rows = public.get("engine")
    if not rows:
        return ""
    has_lookup = all("lookup_ms" in r for r in rows)

    def build_text(r: dict) -> str:
        text = f"{format(r['build_seconds'], 'g')} s" + (f", {r['build_note']}" if r.get("build_note") else "")
        if r.get("build_seconds_no_embeddings"):
            text += f" ({format(r['build_seconds_no_embeddings'], 'g')} s without)"
        return text
    lines = "".join(
        f'\n          <tr><th scope="row" class="cfg"><span class="cfg-name">{esc(r["name"])}</span></th>'
        f'<td data-label="Build time">{esc(build_text(r))}</td>'
        f'<td data-label="Three lookups, one call">{esc(format(r["query_ms"], "g"))} ms</td>'
        + (f'<td data-label="One lookup">{esc(format(r["lookup_ms"], "g"))} ms</td>' if has_lookup else "")
        + f'<td data-label="Index size">{esc(format(r["index_mb"], "g"))} MB</td></tr>' for r in rows)
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
        callout = (f'{fastest["name"]} answers three lookups in one call in {format(fastest["query_ms"], "g")} ms and {slowest["name"]} in '
                   f'{format(slowest["query_ms"], "g")} ms, {times:.1f} times as long.')
        if fastest.get("lookup_ms") and slowest.get("lookup_ms"):
            callout += (f' For a single lookup the times are {format(fastest["lookup_ms"], "g")} ms and {format(slowest["lookup_ms"], "g")} ms, '
                        f'{slowest["lookup_ms"] / fastest["lookup_ms"]:.1f} times as long.')
        race = ('<p class="group-title">Time for one call that answers all three lookups. A bar is as long as its time, and every bar is '
                'drawn to one scale.</p>\n  '
                '<figure class="card race" id="race" aria-labelledby="race-cap">' + bars +
                f'\n    <figcaption id="race-cap" class="race-callout">{esc(callout)}</figcaption>\n    '
                '<p class="race-status" hidden>Motion is off. Each bar is shown finished.</p>\n    '
                '<div class="race-bar"><button type="button" class="btn quiet replay">Replay</button></div>\n  </figure>\n  ')
    head = ('<th scope="col">Configuration</th><th scope="col">Build time</th><th scope="col">Three lookups, one call</th>'
            + ('<th scope="col">One lookup</th>' if has_lookup else "") + '<th scope="col">Index size</th>')
    return ('<section class="section" aria-labelledby="engine">\n  <h2 id="engine" class="section-title">Engine speed</h2>\n  ' + race +
            f'<p class="group-title{" spaced" if race else ""}">Time to build the index, time to answer and size on disk. Times are medians of 20 '
            'whole-process runs on one machine on one day, except the first generation\'s build time, which was measured in an earlier session. '
            'The first generation\'s index size leaves out a cache it shares across repositories.</p>\n  '
            '<div class="card">\n    <table class="board board-small">\n      <caption class="sr-only">Engine speed by configuration</caption>\n      '
            f'<thead><tr>{head}</tr></thead>\n      <tbody>' + lines + '\n      </tbody>\n    </table>\n  </div>\n</section>\n')


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
            ("Estimated cost of one run", f"about {f['cost_multiple']:.0f} times"),
            ("Orchestrated runs with an estimated planning cost", of(f["planning_estimated"], f["orchestrated"]["runs"]))]),
                      ("hooks", lambda f: [
            ("Fully correct, tool on", of(f["with_hooks"]["perfect_runs"], f["with_hooks"]["runs"])),
            ("Fully correct, tool off", of(f["without_hooks"]["perfect_runs"], f["without_hooks"]["runs"])),
            ("Change in token use", f"{f['token_change']:+.0%}"),
            ("Spread between repeats of one prompt", f"{f['spread']:.0%}")])):
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


def stats_html(items: list[tuple[str, str]]) -> str:
    """The figures a section of the Study page is about, set large at its head: each is a number and the words for it."""
    return '<ul class="stats">' + "".join(f'<li><b>{esc(figure)}</b><small>{esc(label)}</small></li>' for figure, label in items) + '</ul>'


def bench_lead_slot(public: dict) -> str:
    """The comparison the Benchmarks page opens with, beside its title: the tokens and turns of the most reliable configuration and
    of no index in the headline cell, the saving in that one cell and across the cells, and how many runs stand behind it. The
    figures are the ones the Overview gives, and count from the no-index value to the measured one when they come into view."""
    found = headline_of(public)
    if not found or not found["c"]:
        return ""
    b, c = found["b"], found["c"]
    versus = public.get("versus")
    across = f" Across {number_word(len(versus['cells']))} cells: {saving_range(versus)}." if versus and len(versus["cells"]) > 1 else ""
    saving = 1 - b["tokens"] / c["tokens"]
    figures = [
        ("Tokens per task", tokens(b["tokens"]), "", f"No index: {tokens(c['tokens'])}"),
        ("Fewer tokens", share(saving), "", f"In this one cell.{across}"),
        ("Turns per task", f"{b['turns']:.1f}", "", f"No index: {c['turns']:.1f}"),
        ("Runs in the cell", str(b["runs"]), "", f"{number_word(b['runs'], True)} runs cannot support a claim of significance, and this site makes none.")]
    items = "".join(f'\n      <li class="bl-fig"><span class="bl-k">{esc(label)}</span><b{attrs}>{esc(value)}</b><span class="bl-v">{esc(glue(note))}</span></li>'
                    for label, value, attrs, note in figures)
    return (f'<div class="bench-lead"><p class="bl-cap">{esc(BEST_LABEL)} configuration against no index, light class, original questions.</p>'
            f'<ul class="bl-figs">{items}\n    </ul>{pick_caveat(public)}</div>')


def study_slots(public: dict) -> dict:
    study = public["study"]
    repo = study["repository"]
    sessions, held, sets = study["sessions"], study["held_out"], public["sets"]
    behind, spread, tok, graded, unpub = (study[key] for key in ("behind", "spread", "tokens", "graded", "unpublished"))
    best = next((c for c in public["configs"] if c["best"]), None)
    per_set = len(study["questions"])
    sizes = sorted(behind["by_size"], key=int)
    in_cells = "; ".join(f"{behind['by_size'][n]} cells of {n} {'run' if n == '1' else 'runs'}" for n in sizes)
    start = sessions["start_up"]
    names = {c["id"]: c["name"] for c in public["classes"]}

    short = [("Repository", f"A {repo['language']} project, {repo['files']} files, about {repo['lines']:,} lines"),
             ("Questions", f"{per_set} in each of {len(sets)} sets, each asked in every run on its set"),
             ("Runs per published cell", f"{sizes[0]} to {sizes[-1]}")]
    long = [("Runs behind the leaderboard",
             f"{behind['runs']} single-agent runs in {behind['cells']} cells: {in_cells}."),
            ("Agents and runs in all",
             f"{study['agents']:,} agents and {study['runs']:,} runs, which also count configurations that are not published, tests of "
             "another tool and setups with several agents. They show effort, not the sample behind a result."),
            ("Configurations",
             f"{study['configurations_tested']} were tested and {study['configurations_published']} are published here. The authors "
             f"chose which to publish; no rule did. {number_word(unpub['two_turns'], True)} of the {unpub['measured']} unpublished "
             "configurations run at least 3 times on the light class and the original questions also took 2 turns with every run fully "
             f"correct. Those runs are from the first session, when the start-up context was {1 - start['earlier'] / start['later']:.0%} "
             "smaller, so their tokens are not directly comparable.")]
    classes = names

    cut = lambda text: text[0].lower() + text[1:]
    limits = [
        ("One repository", f"Every result comes from one repository of {repo['files']} files, a Python project. Other repositories may behave differently."),
        ("One language", "The repository the questions were asked about is a Python project, and nothing here says how the configurations do on "
                         "other languages. The engine itself is written in Rust; that is a separate matter from the language of the repository it is asked about."),
        ("Six questions",
         f"{number_word(per_set * len(sets), True)} lookup questions of modest difficulty, in {number_word(len(sets))} sets of "
         f"{number_word(per_set)}. Only one question separates the configurations: of the {graded['imperfect']} published single-agent runs "
         f"that were not fully correct, {graded['callers']} missed part of the answer to \"{study['questions'][0]['title']}\". The "
         f"\"{study['questions'][2]['title']}\" question was fully answered in {of(*graded['purpose'])} runs, {of(*graded['purpose_no_search'])} "
         "of them by configurations without search, so these runs cannot show whether searching by meaning helps. Harder questions may "
         "separate the configurations more, or less."),
        ("Questions that suit the tool",
         "Two of the three questions in each set ask for the callers of a function, which a lookup tool answers directly, and none was "
         "written to need the body of a function to be read. Questions about what a function does may come out differently."),
        ("A weak baseline",
         "The no-index agent had only its own basic file search and file reading tools, and no command line. A stronger unaided "
         "baseline, such as an agent with a shell and a syntax-aware search, was not measured, so the savings shown are against this "
         "baseline only.")]
    if spread["best_cells"]:
        where = "; ".join(f"{cell['class']} class, {cell['set']} questions, {tokens(cell['low'])} to {tokens(cell['high'])}"
                          for cell in spread["best_cells"])
    limits.append((
        "Few runs, one task",
        f"Each published cell holds {sizes[0]} to {sizes[-1]} runs, and every run answers the same {per_set} questions, so the runs of a cell are "
        "repeats of one task, not samples of many. Three runs cannot support a claim of significance, and this site makes none. A "
        f"difference of one run or one turn is inside the run-to-run spread. In the {spread['cells']} published cells with at least two "
        f"runs, the highest run used {spread['median']:.0%} more tokens than the lowest at the median and {spread['highest']:.0%} more at "
        f"most, and in {spread['wide']} of them at least half as many again. One turn is about {tokens(spread['turn'])} tokens. In "
        f"{spread['mixed']} cells some runs were fully correct and some were not."
        + (f" For \"{best['name']}\", the lowest and highest run in each of its four cells were: {where}." if best and spread["best_cells"] else "")))
    limits += [
        ("Unequal instructions",
         "Each configuration's agent was given a description of its own tool. The descriptions differ in length and detail, and this was "
         "not controlled. See What each agent was told."),
        ("The same team", "The same team built the engine, wrote both question sets and ran the study."),
        ("Start-up context",
         f"An agent starts with about {kilo(tok['start'])} tokens of context from the test machine: its instructions, skills and tools. On "
         "another machine each turn costs a different number of tokens, and so does the saving."),
        ("Machine load", "Wall time depends on what else the test machine was doing. Token counts do not."),
        ("Cost figures", "Where a run is said to cost a multiple of another, the cost is an estimate: a price list typed into the analysis "
                         "and applied to the measured tokens, not read from a bill.")]

    # The question sets: what the held-out set is and why it exists.
    set_rows = [(f"{sets[0]['name']} questions",
                 "The three questions above. Every round has used them, and the people who built the engine saw them while building it, "
                 "so a result on them could reflect tuning to them.")]
    if held:
        classes_text = " and ".join(held["classes"])
        set_rows.append((f"{sets[1]['name']} questions",
                         f"{sets[1]['text']} It is not independent: the same team wrote both sets, on the same repository, in the same three "
                         "shapes, and the job descriptions in the which-function questions follow the wording of the functions' own "
                         "documentation. It checks whether the engine was fitted to the first three questions, not whether it works on other "
                         f"kinds of question. They were run on {number_word(held['configurations'])} configurations, on the {classes_text} "
                         f"{'class' if len(held['classes']) == 1 else 'classes'}, with {held['runs_per_cell']['min']} to "
                         f"{held['runs_per_cell']['max']} runs in each cell."))

    # The two sessions, the start-up context and the memory.
    team = public.get("team")
    session_rows = [(
        "Three sessions" if team else "Two sessions",
        ("The results come from three sessions, the first a few days before the others. The first measured the earlier configurations. The second, round five, measured the "
         if team else "The results come from two sessions a few days apart. The first measured the earlier configurations. The second measured the ")
        + "second-generation engine and the held-out questions, and repeated three earlier configurations: no index and two builds of the "
        "first-generation engine. The leaderboard shows the repeats, which were measured on the same day as the second generation."
        + (f" The third, round six, ran on {ROUND_SIX_DATE}, a day later, and measured the engine in team setups with its own baseline; the no-index agent was not run again, so the "
           "highlighted configuration, which comes from round six, is set against the no-index agent of round five." if team else "")
        + (f" {number_word(len(sessions['not_repeated']), True)} configurations from the first session were not repeated. They keep their "
           f"first-session figures and are marked \"{EARLIER_LABEL}\" on the leaderboard: " + "; ".join(sessions["not_repeated"]) + ". Their "
           "tokens are not directly comparable with the rest." if sessions["not_repeated"] else "")),
        ("Start-up context",
         f"An agent's start-up context, the text it reads before it sees a question, grew between the sessions. On the "
         f"{' and '.join(start['classes'])} {'class' if len(start['classes']) == 1 else 'classes'} it averaged {kilo(start['earlier'])} tokens "
         f"in the earlier runs of the three repeated configurations ({start['earlier_agents']} agents) and {kilo(start['later'])} in their "
         f"repeats ({start['later_agents']} agents). Every turn reads it again, so the same configuration costs more tokens now than it did "
         "then. That is why the leaderboard compares runs made on the same day"
         + (f", with one exception: the highlighted configuration of round six against the no-index agent of round five, whose start-up contexts differ by {abs(team['start_drift']):,.0f} tokens "
            f"({abs(team['start_drift']) / team['start_five']:.1%})." if team and team.get("start_drift") is not None and team.get("start_five") else ".")),
        ("How the memory was filled",
         "Some configurations use a memory of earlier sessions. Before the runs, the memory was filled by one of two earlier sessions: one "
         "that answered the same questions the runs ask, and one that explored the repository without seeing any question. The first kind "
         "is a best case, because the memory already holds the answers, and the configurations that use it say so in their names. Neither "
         "is a test of the task memory is for: work that spans sessions was not measured.")]

    # What a token count is.
    ctx = tok["context"]
    token_rows = [
        ("What is counted",
         "The tokens of a run are the input, the output and the cache reads and writes of the agent, added up over the whole task. "
         f"{tok['cached']:.0%} of the tokens counted in the {tok['runs']} published single-agent runs were cache reads or writes."),
        ("Where they come from",
         f"An agent starts with a context of about {kilo(tok['start'])} tokens: the instructions, skills and tool descriptions of the test "
         "machine. Every turn reads it again."
         + (f" On the light class with the original questions that context and the conversation read again on each turn made up {ctx['best']:.0%} of the tokens of \"{best['name']}\""
            + (f" and {ctx['control']:.0%} of those of the no-index agent." if ctx["control"] else ".") if best and ctx["best"] else "")),
        ("What follows",
         f"Tokens and turns are therefore one measurement counted twice, and most of the saving shown here is the saving of turns: about "
         f"{tokens(spread['turn'])} tokens for each turn avoided. The output of the lookups themselves is small beside that."),
        ("Money",
         "A token read from the cache costs less than one that is not, so a saving in tokens is not a saving in money of the same size. "
         "This site gives no money figures except the estimate in the orchestration finding.")]

    # How answers were graded.
    graded_rows = [
        "Every answer was checked by a script against an answer key, and no score was given by a person judging an answer. The keys for "
        "the questions about callers and definitions were read from the repository's syntax tree; the key for the which-function question "
        "was written by hand. The authors reviewed all of them. For the which-function question on the held-out set, the key accepts either "
        "of two functions, because both do the job.",
        "Each answer scores between 0 and 1, with partial credit: a list of callers scores by how many are named and how many are wrong. "
        "The mean score is the average of these scores. A run is fully correct when all three of its answers score full marks, so one missed "
        "caller is enough to make a run not fully correct."]

    # The control.
    control = sessions.get("control")
    control_rows = []
    if control and control["today"]["original"] and control["earlier"]:
        today, earlier = control["today"], control["earlier"]

        def per_class(counts: dict) -> str:
            shown = [f"{pair_of(counts[cid])} {cid}-class runs" for cid in names if cid in counts]
            return shown[0] if len(shown) == 1 else ", ".join(shown[:-1]) + " and " + shown[-1]
        acted = study.get("tool_acted")
        control_rows.append((
            "What happened",
            "On the test machine, another tool altered some of the lines the no-index agent read. In some runs it hid the very lines that "
            "show one of the callers the callers question asks for, so those agents left that caller out. That is a fact about the test "
            "machine, not about having no index."))
        if acted:
            touched = [o for o in acted["others"] if o["acted"][0]]
            untouched = [o for o in acted["others"] if not o["acted"][0]]
            control_rows.append((
                "How often",
                f"In the second session the tool acted on {of(*acted['control'])} no-index runs. Among the other published configurations "
                "it acted on " + ("; ".join(f"{o['name']} ({of(*o['acted'])} runs)" for o in touched) if touched else "none")
                + f", and on none of the runs of the other {number_word(len(untouched))}"
                + (f", among them \"{best['name']}\"" if best and any(o["name"] == best["name"] for o in untouched) else "") + "."))
        tool = (public.get("team") or {}).get("tool")
        if tool and tool["altered"]:
            control_rows.append((
                "How often, round six",
                f"In round six the tool acted in {tool['acted']} of the round's {tool['runs']} runs and altered a read in {tool['altered']}, among them {tool['missed_altered']} of the "
                f"{tool['missed']} runs that were not fully correct. The Benchmarks page gives each finding that rests on such a run with the change it would show without them."))
        control_rows.append((
            "The day's counts",
            f"In the second session the no-index agent was fully correct in {per_class(today['original'])} on the original questions"
            + (f", and in {per_class(today['held-out'])} on the held-out questions" if today["held-out"] else "") + "."))
        control_rows.append((
            "The earlier session's counts",
            f"In the first session, which had no such tool, it was fully correct in {per_class(earlier)}."))
        tokens_by_session = control.get("tokens")
        if tokens_by_session and tokens_by_session["today"].get("light") and tokens_by_session["earlier"].get("light"):
            t, e = tokens_by_session["today"]["light"], tokens_by_session["earlier"]["light"]
            control_rows.append((
                "Tokens in each session",
                f"On the light class and the original questions the no-index agent used {tokens(e[0])} tokens and {e[1]:.1f} turns in the "
                f"first session ({e[2]} runs) and {tokens(t[0])} tokens and {t[1]:.1f} turns in the second ({t[2]} runs)."
                + (" The second session's control used fewer tokens, so it is the less favourable one for the product on tokens."
                   if t[0] < e[0] else "")))
        hooks = public["findings"].get("hooks")
        if hooks:
            control_rows.append((
                "Effect on tokens",
                f"In earlier tests of the same tool, token use changed by {hooks['token_change']:+.0%}, against {hooks['spread']:.0%} between "
                "repeats of one prompt."))
        control_rows.append((
            "What this site does about it",
            "The no-index figures are shown as measured, with a note on the leaderboard. The product is set against no index on tokens and "
            "turns only. Fully correct runs and mean score are set only against the first-generation engine, which was measured on the same "
            "day on the same machine"
            + (" and which the tool did not act on" if acted and all(not o["acted"][0] for o in acted["others"] if "first generation" in o["name"].lower()) else "")
            + "." + (f" For the highlighted configuration of round six that comparison crosses a day: its runs were made on {ROUND_SIX_DATE}, the first generation's the day before." if team else "")
            + " Running the control again with that tool off, in the same session as the others, would remove the problem."))
    ctx_best = ctx["best"] if ctx.get("best") else None
    stats = {
        "stats_asked": stats_html([(str(per_set), "questions in a set"), (str(len(sets)), "question sets"), (str(per_set * len(sets)), "questions in all")]),
        "stats_scale": stats_html([(str(behind["runs"]), "single-agent runs"), (str(behind["cells"]), "published cells"),
                                   (f"{sizes[0]} to {sizes[-1]}", "runs in a cell"), (str(graded["imperfect"]), "runs not fully correct")]),
        "stats_tokens": stats_html([(f"{tok['cached']:.0%}", "of the tokens were cache reads or writes"), (kilo(tok["start"]), "tokens of start-up context")]
                                   + ([(f"{ctx_best:.0%}", "of the most reliable configuration's tokens were that context")] if ctx_best else [])),
        "stats_sessions": stats_html([("3" if team else "2", "sessions" if team else "sessions, a few days apart"), (f"{kilo(start['earlier'])} to {kilo(start['later'])}", "tokens of start-up context, earlier to later")]),
        "stats_limits": stats_html([("1", "repository"), ("1", "language"), (f"{sizes[0]} to {sizes[-1]}", "runs in a cell"), (str(per_set * len(sets)), "questions")]),
    }
    return {
        **stats,
        "questions": text_rows_html([(q["title"], q["text"]) for q in study["questions"]]),
        "sets": text_rows_html(set_rows),
        "graded": "".join(f"\n      <p>{esc(text)}</p>" for text in graded_rows) + "".join(
            f"\n      <p>{esc(text)}</p>" for text in [
                f"Only one question separates the configurations in these runs: of the {graded['imperfect']} published single-agent runs "
                f"that were not fully correct, {graded['callers']} missed part of the answer to \"{study['questions'][0]['title']}\"."])
        + "\n    ",
        "setups": text_rows_html([(f"Setup {i + 1}", text) for i, text in enumerate(study["setups"])]),
        "scale": rows_html([(label, esc(value)) for label, value in short]) + "\n      " + text_rows_html(long),
        "tokens": text_rows_html(token_rows),
        "classes": text_rows_html([(classes[c["id"]], c["text"]) for c in study["classes"]]) + models_lanes(),
        "sessions": text_rows_html(session_rows),
        "control": text_rows_html(control_rows) if control_rows else "<p class=\"note\">The control was not run in both sessions.</p>",
        "limits": text_rows_html(limits)}


def team_install_block(inst: dict, team: dict | None) -> str:
    """The Install page's section for a team of agents: the pattern round six supports, in four steps, each marked for what was
    measured and what is advice. Written only when round six is in the data; the figures are the ones the Benchmarks page gives."""
    ev = (team or {}).get("evidence") or {}
    if not all(key in ev for key in ("pack_alone", "briefs", "with_commands", "hub")):
        return ""
    newer = release_gap({"team": team, "install": inst})
    late = ('\n      <div class="proto"><span class="tag">Version</span>\n        <p>Round six was measured with pgr ' + esc(newer) + ', which is not released yet. The release this page installs is '
            + esc(inst["version"]) + '. Its <code>pgr pack</code> works, but its pack lacks the section of named definitions that the packs of round six opened with, so it is not the pack that was measured. '
            'The commands <code>pgr handoff</code> and <code>pgr loadout</code> come with ' + esc(newer) + '. Run <code>pgr help</code> to see which commands your copy has.</p></div>' if newer else "")
    pack, briefs, cmds, hub = ev["pack_alone"], ev["briefs"], ev["with_commands"], ev["hub"]
    orchestrated = ev.get("orchestrated")
    notes = ev.get("notes")

    def step(title: str, measured: str, advice: str) -> str:
        return (f'\n      <li><h3>{title}</h3><p class="team-m"><span class="tag">Measured</span> {esc(glue(measured))}</p>'
                f'<p class="team-a"><span class="tag quiet">Advice</span> {advice}</p></li>')

    steps = step(
        "Run <code>pgr pack</code> on each subagent's task and paste the output into its brief",
        f"In round six the harness made a pack for each question before the agent started and put it in the prompt. {PACK_MADE} Three subagents started together "
        f"used {briefs['change']} tokens with a pack in each brief than without, and {of(briefs['perfect'], briefs['runs'])} runs were fully correct.",
        "Give the command the task in the words you would give a colleague: a pack is built from the words it is given. Make one pack for each subagent's own task, not one for the whole job.")
    steps += step(
        "Tell it to answer from the pack, and to look up only what is missing",
        f"Each pack came with the note that it may be all the agent needs and that anything it does not show can be looked up. Even so, an agent that also had the lookup commands ran one in "
        f"{of(cmds['looked'], cmds['runs'])} runs and answered in one turn in {of(cmds['one_turn'], cmds['runs'])}. An agent with the pack and no command answered in {pack['turns']} on "
        f"{pack['tokens']} tokens, fully correct in {of(pack['perfect'], pack['runs'])} runs.",
        "Say it plainly in the brief, and keep the lookup command within reach for any work that is open-ended: an agent with no command cannot look further, and round six did not test open-ended work.")
    steps += step(
        "Give the reviewer the same packs",
        (f"In the orchestrated runs the reviewer got the same packs as the subagents, and the total moved {orchestrated['change']} tokens against runs with no packs "
         f"({of(orchestrated['perfect'], orchestrated['runs'])} runs fully correct), inside the spread of three runs. What the reviewer's packs are worth alone was not isolated."
         if orchestrated else "In the orchestrated runs the reviewer got the same packs as the subagents. What the reviewer's packs are worth alone was not isolated."),
        "A reviewer that starts from the packs can spend its calls on checking what the subagents said instead of finding it again.")
    steps += step(
        "Use the hub only for findings that cannot be looked up again",
        f"On lookup work the hub used {hub['change']} tokens and gained no accuracy: {of(hub['perfect'], hub['runs'])} runs were fully correct, with the hub and without."
        + (f" On a demonstration set whose answers are notes an earlier agent left, an agent got {of(*notes['none'])} note fields right with no notes and {of(*notes['brief'])} with the notes in its brief"
           + ((", at the lowest mean tokens of the three setups" + ("" if notes.get("apart") else ", though the runs of the three overlap")) if notes.get("lowest") else "") + "." if notes else ""),
        "File a decision, a ticket number, a reason, or a finding someone confirmed. Do not file what <code>pgr ask</code> answers again. <code>pgr handoff</code> files a note and "
        "<code>pgr loadout</code> reads what was filed for a task; <code>pgr help</code> lists their options.")
    return ('\n    <section class="block" aria-labelledby="team">\n      <h2 id="team" class="block-title">Use it in a team of agents</h2>\n'
            '      <p class="block-lede">The pattern round six supports, in four steps. Each step says what was measured and what is advice that follows from it. '
            'The questions were written by the builders of the engine and none of this was tested on open-ended work.</p>'
            f'{late}\n      <ol class="steps team-steps">{steps}\n      </ol>\n    </section>\n')


def install_slot(public: dict) -> str:
    """The Install page: the command for the visitor's system, the Python route, every release file with its size and link, what
    an installer does and where it puts the file, the first commands, the rule to give an agent, what was tested and what was
    not, and the licence. Everything is built from install.json, so a new release is one edit of that file."""
    inst = public["install"]
    by_id = {item["id"]: item for item in inst["installers"]}
    latest, pinned = inst["download_base"], inst["versioned_base"]
    version = inst["version"]
    wheels = inst["wheels"]
    toc = [("get", "Install"), ("python", "pip, pipx and uv"), ("files", "Release files"), ("how", "What it does"), ("first", "First commands"),
           ("agent", "Your agent")] + ([("team", "A team of agents")] if team_install_block(inst, public.get("team")) else []) + [
           ("tested", "What was tested"), ("licence", "Licence")]
    toc_html = ('<nav class="toc" aria-label="On this page"><ul>' + "".join(f'<li><a href="#{anchor}">{esc(text)}</a></li>' for anchor, text in toc)
                + '</ul></nav>')

    def file_row(item: dict, base: str, kind: str) -> str:
        os_name = OS_OF.get(item.get("system", ""), "")
        mark = f' data-os="{os_name}"' if os_name else ""
        return (f'\n          <li class="file"{mark}><a class="file-name" href="{esc(base)}/{esc(item["name"])}">{esc(item["name"])}</a>'
                f'<span class="file-for">{esc(item.get("label", ""))}</span><span class="file-size">{esc(size_text(item["bytes"]))}</span></li>')

    first_items = "".join(
        f'\n        <li class="first-step"><p class="first-text">{esc(item["text"])}</p>{cmd_line(item["command"], "Copy: " + item["command"])}</li>'
        for item in inst["first_commands"])
    rule = "\n".join(inst["agent_rule"])
    wheel_lines = "".join(
        cmd_line(f"{tool} {pinned}/{wheel['name']}", f"Copy the command for {tool.split()[0]}, {wheel['label']}",
                 f' data-tool="{tool.split()[0]}" data-sys="{wheel["system"]}"')
        for wheel in wheels for tool in PYTHON_TOOLS)
    tool_tabs = "".join(
        f'<button type="button" role="radio" aria-checked="{"true" if n == 0 else "false"}" tabindex="{0 if n == 0 else -1}" data-value="{tool.split()[0]}">'
        f'{tool.split()[0]}</button>' for n, tool in enumerate(PYTHON_TOOLS))
    options = "".join(f'<option value="{esc(w["system"])}">{esc(w["label"])}</option>' for w in wheels)
    ran = "".join(f"\n          <li>{esc(line)}</li>" for line in inst["tested"]["ran"])
    not_run = "".join(f"\n          <li>{esc(line)}</li>" for line in inst["tested"]["not_run"])
    what = (
        '\n    <ol class="steps">'
        f'\n      <li><h3>It downloads one program file</h3><p>The file for your system and its checksum list, SHA256SUMS.txt, from the release at {esc(inst["release_page"])}. '
        'Nothing else is downloaded: no Python, no runtime, no packages.</p></li>'
        '\n      <li><h3>It checks the file</h3><p>The installer compares the file with its published SHA-256. If they differ, it stops and nothing is installed.</p></li>'
        '\n      <li><h3>It puts the file in a folder of your user account</h3>'
        '<p>On Windows that is %LOCALAPPDATA%\\Programs\\pgr\\pgr.exe, and the folder is added to your user PATH, so open a new terminal afterwards. '
        'On macOS and Linux it is ~/.local/bin/pgr; if that folder is not on your PATH, the installer prints the line to add. No administrator rights and no root are used.</p></li>'
        '\n      <li><h3>It runs the program once</h3><p>The installer asks the new file for its version, so a file that cannot run on your system is reported at once.</p></li>'
        '\n    </ol>')
    return f'''<div class="install-layout">
  {toc_html}
  <div class="install-main">
    <section class="block get" aria-labelledby="get">
      <h2 id="get" class="block-title">Install with one command</h2>
      <p class="block-lede">Your system is selected for you. Paste the line into PowerShell or a terminal. The installer needs no administrator rights and no root.</p>
      {cmdbox_html(inst, big=True)}
      <div class="get-grid">
        <ul class="facts">
          <li><b>One program file</b><span>{esc(size_text(inst["programs"][0]["bytes"]))} on Windows, about the same elsewhere.</span></li>
          <li><b>Checked before it is installed</b><span>Against the published SHA256SUMS.txt.</span></li>
          <li><b>Local only</b><span>No account, no server, no network use once it is installed.</span></li>
          <li><b>Works with any agent</b><span>Any agent that can run a command. The benchmark on this site measured it under Claude Code.</span></li>
        </ul>
        {installer_term(inst)}
      </div>
    </section>

    <section class="block" aria-labelledby="python">
      <h2 id="python" class="block-title">Install with pip, pipx and uv</h2>
      <p class="block-lede">These are optional ways to deliver the same program, which is written in Rust: the wheel carries the program itself and needs no Python packages. Pass the address of the wheel for your system, from the release page, to uv, pipx, or pip.</p>
      <div class="py-controls" hidden>
        <div class="seg" role="radiogroup" aria-label="Tool" data-seg="tool" style="--count:3"><span class="seg-thumb" aria-hidden="true"></span>{tool_tabs}</div>
        <label class="select-wrap"><span class="sr-only">System</span><select class="select" data-wheel aria-label="System">{options}</select></label>
      </div>
      <div class="py-lines">{wheel_lines}</div>
      <p class="fine-print">For another system, take the wheel from the list below; its name carries the version, {esc(version)}.</p>
    </section>

    <section class="block" aria-labelledby="files">
      <h2 id="files" class="block-title">Release files</h2>
      <p class="block-lede">Program files carry no version in their name, so the address <span class="mono-inline">{esc(latest)}/NAME</span> always gives the newest. Every file is on the <a href="{esc(inst["release_page"])}">release page</a>.</p>
      <div class="files-grid">
        <div class="file-group"><h3>Program files</h3><ul class="files">{"".join(file_row(item, latest, "program") for item in inst["programs"])}
        </ul></div>
        <div class="file-group"><h3>Wheels for pip, pipx and uv, version {esc(version)}</h3><ul class="files">{"".join(file_row(item, pinned, "wheel") for item in wheels)}
        </ul></div>
        <div class="file-group"><h3>Installers and checksums</h3><ul class="files">{"".join(file_row(item, latest, "support") for item in inst["support"])}
        </ul></div>
      </div>
    </section>

    <section class="block" aria-labelledby="how">
      <h2 id="how" class="block-title">What the installer does</h2>{what}
    </section>

    <section class="block" aria-labelledby="first">
      <h2 id="first" class="block-title">Your first three commands</h2>
      <p class="block-lede">Open a terminal inside a git repository. Replace SomeFunction with a name from your code.</p>
      <ol class="firsts">{first_items}
      </ol>
    </section>

    <section class="block" aria-labelledby="agent">
      <h2 id="agent" class="block-title">Tell your agent to use it</h2>
      <p class="block-lede">An agent uses the command when its instructions say so. Paste these four lines into the instruction file your agent reads at the start of a session.</p>
      <div class="rule-card"><pre class="rule" tabindex="0"><code>{esc(rule)}</code></pre>{copy_button(rule, "Copy the four lines for your agent")}</div>
      <div class="proto"><span class="tag">Not part of the measured benchmark</span>
        <p>The program also runs as a Model Context Protocol server, for agents that speak the protocol instead of running commands. Start it with the command below. The benchmark on this site measured agents that ran commands, not this route.</p>
        {cmd_line("pgr mcp", "Copy: pgr mcp")}</div>
    </section>

{team_install_block(inst, public.get("team"))}
    <section class="block" aria-labelledby="tested">
      <h2 id="tested" class="block-title">What was tested, and what was not</h2>
      <div class="tested-grid">
        <div class="card-plain"><h3>What ran</h3><ul>{ran}
        </ul></div>
        <div class="card-plain"><h3>What did not run</h3><ul>{not_run}
        </ul></div>
      </div>{platform_block()}
    </section>

    <section class="block" aria-labelledby="licence">
      <h2 id="licence" class="block-title">Licence</h2>
      <p class="block-lede licence">{esc(inst["licence"])}</p>
      <p class="close-actions"><a class="btn quiet" href="study.html">How it was measured</a><a class="btn quiet" href="benchmarks.html">Open the benchmarks</a></p>
    </section>
  </div>
</div>
'''


# The mark: a P and the token it hands over, one colour, drawn on a 24 unit grid. The top bar and the foot show it.
MARK_PATHS = '<path d="M7 4v16M7 4h6.5a4.25 4.25 0 0 1 0 8.5H7"/><circle cx="18.5" cy="18.6" r="1.9"/>'
MARK_SVG = f'<svg class="logo" viewBox="0 0 24 24" aria-hidden="true" focusable="false">{MARK_PATHS}</svg>'
def nav_slots(name: str, sections: dict, public: dict) -> dict:
    """The top bar and the foot of a page. The foot holds the site map, every page with its sections as links: it is the
    way round the site without a script, and the command palette reads it."""
    install = public["install"]
    by_id = {item["id"]: item for item in install["installers"]}
    current = ' aria-current="page"'
    links = "".join(f'\n      <a href="{file}"{current if file == name else ""}>{title}</a>' for file, title in PUBLIC_PAGES)
    bar = (f'<header class="nav" id="nav" data-cmd-windows="{esc(by_id["windows"]["command"])}" data-cmd-unix="{esc(by_id["unix"]["command"])}">\n'
           f'    <div class="nav-in">\n      <a class="brand" href="index.html" aria-label="{BRAND}, overview">{MARK_SVG}<span>{BRAND}</span></a>\n'
           f'      <nav class="nav-links" id="nav-links" aria-label="Pages"><span class="nav-ind" aria-hidden="true"></span>{links}\n      </nav>\n'
           '      <div class="nav-tools">\n'
           '        <button type="button" class="kbd-btn" data-palette aria-haspopup="dialog" aria-label="Open the command palette"><span>Search</span><kbd data-mod>Ctrl</kbd><kbd>K</kbd></button>\n'
           '        <button type="button" class="icon-btn" data-theme-toggle aria-label="Switch the theme"><svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">'
           '<circle class="sun" cx="12" cy="12" r="4.2"/><path class="rays" d="M12 2.8v2.4M12 18.8v2.4M2.8 12h2.4M18.8 12h2.4M5.5 5.5l1.7 1.7M16.8 16.8l1.7 1.7M5.5 18.5l1.7-1.7M16.8 7.2l1.7-1.7"/>'
           '<path class="moon" d="M19.6 14.4A8 8 0 0 1 9.6 4.4a8 8 0 1 0 10 10z"/></svg></button>\n'
           f'        <a class="icon-btn" href="{esc(install["repository"])}" aria-label="The repository on GitHub"><svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">'
           '<path d="M9 19c-4.3 1.4-4.3-2.5-6-3m12 5v-3.5c0-1 .1-1.4-.5-2 2.8-.3 5.5-1.4 5.5-6a4.6 4.6 0 0 0-1.3-3.2 4.2 4.2 0 0 0-.1-3.2s-1.1-.3-3.5 1.3a12.3 12.3 0 0 0-6.2 0C6.5 2.8 5.4 3.1 5.4 3.1a4.2 4.2 0 0 0-.1 3.2A4.6 4.6 0 0 0 4 9.5c0 4.6 2.7 5.7 5.5 6-.6.6-.6 1.2-.5 2V21"/></svg></a>\n'
           '      </div>\n    </div>\n  </header>')
    columns = "".join(
        f'\n      <div class="sm-col"><a class="sm-page" href="{file}">{title}</a><ul>'
        + "".join(f'<li><a href="{file}#{anchor}">{esc(text)}</a></li>' for anchor, text in sections.get(file, []))
        + "</ul></div>" for file, title in PUBLIC_PAGES)
    foot = ('<footer class="foot" id="foot">\n    <div class="foot-in">\n      <div class="foot-brand"><p class="foot-name">' + MARK_SVG + f'<span>{BRAND}</span></p>\n'
            '        <p>A desktop command-line tool for AI coding agents.</p>\n'
            f'        <p class="foot-built">Results built {esc(public["built"])}.</p></div>\n'
            f'      <nav class="sitemap" aria-label="Site map">{columns}\n      </nav>\n    </div>\n'
            f'    <div class="foot-line"><p>The benchmark pages report measured outcomes: counts of tokens, turns and answers. What is not measured is marked.</p>\n'
            f'      <p class="foot-links"><a href="{esc(install["release_page"])}">Latest release</a><a href="{esc(install["repository"])}">Repository</a></p></div>\n  </footer>')
    toc = ('<nav class="toc" aria-label="On this page"><ul>'
           + "".join(f'<li><a href="#{anchor}">{esc(text)}</a></li>' for anchor, text in sections.get(name, [])) + '</ul></nav>')
    return {"nav": bar, "foot": foot, "toc": toc}


# --- the Overview streams, the footage and the turn runners -------------------------------------

def problem_slot(public: dict) -> str:
    """The first chapter of the Overview: a task as a stack of turns. One block is one turn at the average tokens a turn, so a
    stack is as tall as its tokens per task, and both stacks are drawn to one scale. Under a stack are its turns and the tokens
    of the whole task, written as measured and never counted up. A script lets the blocks land as the scroll passes and lights
    the blocks already there each time one lands; the page is written finished."""
    found = headline_of(public)
    if not found or not found["c"]:
        return ""
    b, c = found["b"], found["c"]
    longest = max(b["tokens"], c["tokens"])

    def stack(kind: str, name: str, pt: dict) -> str:
        count = max(1, min(12, -int(-pt["turns"] // 1)))           # whole turns, the last one possibly a part
        per = pt["tokens"] / pt["turns"]
        parts = [min(1.0, pt["turns"] - i) for i in range(count)]
        slabs = "".join(f'<i class="slab" style="--h:{per * part / longest * 100:.3f}%;--i:{i}"><b class="ec"></b><span>{i + 1}</span></i>'
                        for i, part in enumerate(parts))
        return (f'\n      <div class="stack {kind}" style="--n:{count}">'
                f'<div class="stack-col">{slabs}</div>'
                f'<p class="stack-total"><b>{pt["turns"]:.1f}</b> turns</p>'
                f'<p class="stack-name">{esc(name)}<span>about {esc(tokens(per))} a turn, <b class="stack-tok">{esc(tokens(pt["tokens"]))}</b> in all</span></p></div>')

    with_commands = ((public.get("team") or {}).get("evidence") or {}).get("with_commands")
    floor = ("Two turns is the floor for a lookup that needs a command: one to ask and one to answer. A pack in the prompt, with no command to call, takes the asking turn away"
             + (f"; given the commands as well, agents still ran a lookup in {of(with_commands['looked'], with_commands['runs'])} runs. " if with_commands else ". ")
             if found["best"].get("commandless") and b["turns"] < 1.05
             else "Two turns is the floor for a lookup: one to ask and one to answer. " if b["turns"] < 2.05 else "")
    held = (cell_of(found["best"], "light", "held-out"), cell_of(found["control"], "light", "held-out"))
    later = (f" On the held-out questions it took {held[0]['turns']:.1f} turns, and no index {held[1]['turns']:.1f}." if all(held) else "")
    return ('<section class="chapter ch-problem pin" aria-labelledby="problem">\n  <div class="pin-stage">\n    '
            '<div class="ch-copy">\n      <h2 id="problem" class="kin" aria-label="Every turn re-reads everything">'
            '<span class="kl" aria-hidden="true"><span>Every turn</span></span> <span class="kl" aria-hidden="true"><span>re-reads</span></span> '
            '<span class="kl" aria-hidden="true"><span>everything</span></span></h2>\n      '
            '<p class="ch-lede">An agent works in turns, and on each turn it reads its whole conversation again. '
            'Every extra turn pays for the same context one more time.</p>\n      '
            f'<p class="ch-lede">{floor}{esc(found["best"]["name"])} took {b["turns"]:.1f} turns per task on the light class and the original '
            f'questions. No index took {c["turns"]:.1f}.{esc(later)}</p>\n    </div>\n    '
            '<figure class="stacks" aria-labelledby="stacks-cap">' + stack("ref", "No index", c) + stack("best", found["best"]["name"], b) +
            '\n      <figcaption id="stacks-cap">One block is one turn, at the average tokens a turn, and each turn reads the blocks below it again. '
            'Both stacks are drawn to one scale. Light model class, original questions, one agent answering three lookups.</figcaption>\n    </figure>\n  </div>\n</section>')


VIDEO_TYPES = {"mp4": "video/mp4", "webm": "video/webm"}
VIDEO_KIND_TEXT = {"trailer": "Trailer", "explainer": "Explainer"}


def film_name(video: dict) -> str:
    """What the page calls a film: the title video.json gives it. Its kind (trailer or explainer) is shown beside the length
    measured from the file, so the length cannot disagree with the file. A length spoken in the title ("in twenty seconds") is
    set to the file's length when it is not the same."""
    title = str(video["title"])
    seconds = round(float(video["seconds"]))
    tens = {10: "ten", 20: "twenty", 30: "thirty", 40: "forty", 50: "fifty", 60: "sixty"}
    spoken = tens.get(seconds, NUMBER_WORDS[seconds] if 0 <= seconds < len(NUMBER_WORDS) else str(seconds))
    return re.sub(r"\b(\w+) seconds\b", lambda m: f"{spoken} seconds" if m.group(1).lower() in set(tens.values()) | set(NUMBER_WORDS) | {str(n) for n in range(1, 300)} else m.group(0), title)


def video_slot(public: dict) -> str:
    """The footage section: present only when the data holds a film. Both films are framed alike and drawn at the same size;
    with a script one is shown at a time and a control chooses it, without one both are listed. Without a script each is a
    native player that never starts with sound by itself and loads nothing until it is played; with one, the page's own control
    bar replaces the native one (play, a scrub bar, captions, the quality, full screen). The 1080p file comes first, so a
    browser with no script plays that; a script picks the size by the screen and offers the others as a quality choice."""
    films = public.get("videos") or ([public["video"]] if public.get("video") else [])
    if not films:
        return ""

    def kind_of(video: dict) -> str:
        return VIDEO_KIND_TEXT.get(video.get("kind", "explainer"), str(video.get("kind", "explainer")).capitalize())

    def player(video: dict) -> str:
        kind = video.get("kind", "explainer")
        width, height = int(video["width"]), int(video["height"])
        offered = json.dumps([{"src": item["src"], "w": item["width"], "h": item["height"], "fps": item["fps"], "type": item["type"],
                               "mb": item["megabytes"], "label": item["label"]} for item in video["sources"]], separators=(",", ":"))
        large = f' data-poster-large="{esc(video["poster_large"])}"' if video.get("poster_large") else ""
        # In a single-quoted attribute a link ends at the closing quote, so the guard sees exactly the address that was approved.
        offered_attr = offered.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("'", "&#39;")
        return (f'\n    <figure class="cine" data-kind="{esc(kind)}">\n      '
                f'<div class="cine-frame" style="aspect-ratio:{width}/{height}">\n        '
                f'<video controls playsinline preload="none" width="{width}" height="{height}" poster="{esc(video["poster"])}"{large} '
                f"data-sources='{offered_attr}' data-seconds=\"{video['seconds']}\" aria-label=\"{esc(film_name(video))}\">\n          "
                f'<source src="{esc(video["src"])}" type="video/mp4">\n          '
                f'<track kind="captions" src="{esc(video["captions"])}" srclang="en" label="English" default>\n          '
                '<p>This browser cannot play the video.</p>\n        </video>\n      </div>\n      '
                f'<figcaption class="cine-cap"><span class="cine-title">{esc(film_name(video))}</span>'
                f'<span class="cine-meta">{esc(kind_of(video))}, {video["seconds"]:.0f} seconds, with captions</span></figcaption>\n    </figure>')

    pick = ""
    if len(films) > 1:
        buttons = "".join(
            f'\n      <button type="button" role="radio" aria-checked="{"true" if n == 0 else "false"}" tabindex="{0 if n == 0 else -1}" '
            f'data-value="{esc(video.get("kind", "explainer"))}">{esc(kind_of(video))}, {video["seconds"]:.0f} seconds</button>'
            for n, video in enumerate(films))
        pick = (f'\n  <div class="seg reel-pick" role="radiogroup" aria-label="Film" data-seg="film" style="--count:{len(films)}" hidden>'
                f'<span class="seg-thumb" aria-hidden="true"></span>{buttons}\n  </div>')
    return ('<section class="section cinema" aria-labelledby="films">\n  '
            f'<h2 id="films" class="section-title">{"Films" if len(films) > 1 else "Film"}</h2>{pick}\n  '
            '<div class="cine-grid">' + "".join(player(video) for video in films) + '\n  </div>\n</section>\n')


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
            inner = (f'<p class="note empty">No index and one other configuration were not both measured on '
                     f'{esc(cls["name"])} with the {esc(qset["name"].lower())} questions.</p>')
        else:
            config, pt = other
            who = f"the {BEST_LABEL.lower()} configuration" if config["best"] else config["name"]
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
    study["lab"] = lab_slot(public)
    study["second"] = codex_study_slot(public)
    study["team"] = team_study_slot(public)
    slots = {
        "index.html": {"lede": esc(lede_slot("index.html", public)), "ledenote": esc(lede_note_slot(public)), "watch": watch_slot(public), "cmdbox": cmdbox_slot(public),
                       "scene": scene_slot(public), "key": key_slot(public), "problem": problem_slot(public),
                       "figures": figures_slot(public), "video": video_slot(public), "best": best_slot(public),
                       "notes": notes_slot(public), "term": term_slot(public), "size": size_slot(public),
                       "team": team_overview_slot(public), "platforms": esc(platform_line())},
        "benchmarks.html": {"lede": esc(lede_slot("benchmarks.html", public)), "seg": seg_slot(public),
                            "boards": boards_slot(public), "charts": charts_slot(public), "engine": engine_slot(public),
                            "headline": headline_slot(public), "findings": findings_slot(public), "runners": runners_slot(public),
                            "lead": bench_lead_slot(public), "harness-two": codex_slot(public), "size": size_slot(public),
                            "team": team_bench_slot(public)},
        "study.html": dict(lede=esc(lede_slot("study.html", public)), **study),
        "install.html": {"lede": esc(lede_slot("install.html", public)), "install": install_slot(public)},
    }
    # Where a sentence said a class of model, the page says the model and its effort.
    return {page: {name: modelize(text) for name, text in filled.items()} for page, filled in slots.items()}


def write_showcase_pages(public: dict) -> list[str]:
    """Write the numbers and findings into the four pages, so they are there without JavaScript and
    cannot fall behind the data. Every slot a page holds must be filled, and every slot filled must exist. The top bar and the
    foot are filled last: the foot lists the sections of every page, read from the pages as they now are."""
    slot = re.compile(r"<!--slot:([\w-]+)-->(.*?)<!--/slot:\1-->", re.S)
    pages, before = {}, {}
    for name, filled in showcase_slots(public).items():
        path = DOCS / name
        if not path.is_file():
            raise SystemExit(f"{path.relative_to(REPO)} is missing")
        before[name] = path.read_bytes().decode("utf-8")
        raw = before[name].replace("\r\n", "\n")
        wanted = {m.group(1) for m in slot.finditer(raw)} - NAV_SLOTS
        if wanted != set(filled):
            raise SystemExit(f"{name}: slots in the page {sorted(wanted)} do not match the slots built {sorted(filled)}")
        pages[name] = slot.sub(lambda m: f"<!--slot:{m.group(1)}-->{filled[m.group(1)]}<!--/slot:{m.group(1)}-->"
                               if m.group(1) in filled else m.group(0), raw)
        pages[name] = re.sub(r'(<meta name="description" content="[^"]*?)Measured under Claude Code\.[^"]*?(?:\(see Benchmarks\)\.)?(")',
                             lambda m: m.group(1) + esc(harness_line()) + m.group(2), pages[name])
    sections = {name: sections_of(page) for name, page in pages.items()}
    changed = []
    for name, page in pages.items():
        navs = nav_slots(name, sections, public)
        page = slot.sub(lambda m: f"<!--slot:{m.group(1)}-->{navs[m.group(1)]}<!--/slot:{m.group(1)}-->"
                        if m.group(1) in navs else m.group(0), page)
        if page != before[name]:
            (DOCS / name).write_bytes(page.encode("utf-8"))
            changed.append(name)
    return changed


PYTHON_TOOLS = ("uv tool install", "pipx install", "pip install")
PGR_COMMANDS = ("refresh", "ask", "about", "find", "affected", "recall", "retain", "pack", "handoff", "loadout", "help", "mcp")   # the product's own commands the pages may show
HARNESS_NAMES = ("Claude Code", "Codex CLI", "Model Context Protocol", "pgr command", "mean reciprocal rank")   # phrases the pages may use, exactly as written
# What the pages may say about the language of the engine, besides the sentence footprint.json carries.
RUST_PHRASES = ("written in Rust", "the Rust program", "pip, pipx and uv", "Mean reciprocal rank", "Programs\\pgr to your user PATH")
# A link that goes on, with a path or a name, is not the link that was approved: the address ends where nothing of an address follows.
LINK_END = r"(?![\w/#?&=%~-])"
# A name of an agent program followed by a version number says which build was measured, and a version is never named on the site.
HARNESS_VERSION = re.compile(r"(?:Claude Code|Codex CLI)\s*(?:v\d|version\s*\d|\d+(?:\.\d+)+)", re.I)


def approved_strings(install: dict | None) -> list[str]:
    """The text the guard lets through, longest first and in every spelling a file may hold it (plain, escaped for a script,
    escaped for a page): the product's own commands, the install commands and the release links of install.json, and the
    names of the agent programs the benchmark ran under. Anything else the guard checks for stays checked."""
    import html
    found = [f"pgr {name}" for name in PGR_COMMANDS] + list(HARNESS_NAMES) + list(RUST_PHRASES)
    found += [item["name"] for item in MODELS["tiers"].values()]                      # exactly the models of models.json
    if MODELS["codex"]:
        found.append(str(MODELS["codex"]["model_name"]))
    footprint = footprint_of()
    if footprint:
        found += [str(footprint["language"]), str(footprint["how"])]
        if footprint.get("pgr_0_2"):
            found += [str(footprint["pgr_0_2"]["how"])]
    if install:
        found += [item["command"] for item in install["installers"]]
        found += [item["command"] for item in install["first_commands"]]
        for wheel in install["wheels"]:
            url = f"{install['versioned_base']}/{wheel['name']}"
            found += [f"{tool} {url}" for tool in PYTHON_TOOLS]
    out = []
    for text in found:
        out += [text, json.dumps(text)[1:-1], html.escape(text), html.escape(text, quote=False)]
    return sorted(set(out), key=len, reverse=True)


def approved_links(install: dict | None) -> list["re.Pattern"]:
    """The addresses the guard lets through, each exactly as approved and nothing that continues them: the repository, the release
    page, and each release file by its name. A link into another folder of the repository (the report, a blob, a tree) is not
    one of them and is refused as a request to another host."""
    if not install:
        return []
    urls = [install["repository"], install["release_page"], install["download_base"], install["versioned_base"], f"{install['download_base']}/NAME"]
    urls += [f"{install['download_base']}/{item['name']}" for group in ("programs", "support") for item in install[group]]
    urls += [f"{install['versioned_base']}/{item['name']}" for item in install["wheels"]]
    urls += release_film_urls()
    return [re.compile(re.escape(url) + LINK_END) for url in sorted(set(urls), key=len, reverse=True)]


def codex_facts() -> dict | None:
    """What benchmark/data/codex.json says about the second agent program, once it holds finished sessions: for each question set,
    the mean tokens and commands of a session without an index and with pgr, the saving the file computes, how many sessions were
    fully correct, and the lowest and highest session of each cell. Only the summary, the saving block and the token totals of the
    sessions are read: the setup, the prompts and the command template of that file cannot reach a page."""
    path = BENCH / "data" / "codex.json"
    if not path.is_file():
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        sets = []
        for key, set_id in (("A", "original"), ("B", "held-out")):
            sides = {}
            for method, label in (("no-index", "none"), ("pgr", "pgr")):
                row = data["summary"][method][key]
                if not row["with_usage"] or row["with_usage"] != row["sessions"]:
                    return None
                totals = [float(x["usage"]["total_tokens"]) for x in data["sessions"] if x["method"] == method and x["set"] == key]
                sides[label] = {"tokens": float(row["mean_total_tokens"]), "commands": float(row["mean_commands"]),
                                "perfect": int(row["fully_correct"]), "of": int(row["of"]), "sessions": int(row["sessions"]),
                                "low": min(totals), "high": max(totals)}
            saved = float(data["pgr_saving_vs_no_index"][key]["total_tokens_saved_percent"])
            sets.append({"id": set_id, "key": key, "saved": saved, **sides})
        sessions = len(data["sessions"])
        effort = str(data.get("effort", ""))
    except (ValueError, KeyError, TypeError):
        return None
    if not sessions or not all(item["none"]["tokens"] > 0 for item in sets):
        return None
    for item in sets:
        item["change"] = item["pgr"]["tokens"] / item["none"]["tokens"] - 1
        gap = abs(item["none"]["tokens"] - item["pgr"]["tokens"])
        widest = max(item["none"]["high"] - item["none"]["low"], item["pgr"]["high"] - item["pgr"]["low"])
        item["spread_wider_than_gap"] = widest >= gap
    every = sum(side["perfect"] for item in sets for side in (item["none"], item["pgr"]))
    total = sum(side["of"] for item in sets for side in (item["none"], item["pgr"]))
    return {"sets": sets, "sessions": sessions, "per_cell": min(side["sessions"] for item in sets for side in (item["none"], item["pgr"])),
            "all_correct": every == total, "correct": every, "of": total, "effort": effort}


def codex_verdict(found: dict) -> str:
    """The result in a few words, from the signs of the two changes, so that a mixed result is never described as a saving."""
    signs = {item["change"] < 0 for item in found["sets"]}
    return "saved tokens on both question sets" if signs == {True} else "cost more tokens on both question sets" if signs == {False} else "showed no consistent saving"


def codex_headline(found: dict) -> str:
    """The headline of the second-harness block, from the signs of the two changes."""
    verdict = codex_verdict(found)
    return {"saved tokens on both question sets": "Fewer tokens with pgr on both question sets under that agent program.",
            "cost more tokens on both question sets": "More tokens with pgr on both question sets under that agent program."}.get(
                verdict, "No consistent saving was shown under that agent program.")


def codex_direction(item: dict) -> str:
    """Fewer or more tokens with pgr on one question set, from the sign of the change."""
    return "fewer" if item["change"] < 0 else "more"


def second_harness() -> str | None:
    """The second agent program the benchmark ran under, once benchmark/data/codex.json holds its finished sessions."""
    return "Codex CLI" if codex_facts() else None


def harness_line() -> str:
    """What the benchmark ran under: always Claude Code. Once the sessions of the second agent program are in, a second sentence
    says that a smaller check was made under it and how it came out; its figures are on the Benchmarks page and are not added to
    the Claude Code ones."""
    found = codex_facts()
    if not found:
        return "Measured under Claude Code."
    return f"Measured under Claude Code. A smaller check under the Codex CLI {codex_verdict(found)} (see Benchmarks)."


def codex_model_line() -> str:
    """The agent program and its model, as models.json names them."""
    codex = MODELS["codex"]
    if not codex:
        return "the Codex CLI"
    return f"the Codex CLI with {codex['model_name']} at {codex.get('effort', 'default')} effort"


def codex_slot(public: dict) -> str:
    """The small block of the Benchmarks page for the second agent program, never mixed with the Claude Code numbers: for each
    question set the mean tokens of a session without an index and with pgr, to one scale, the sessions that were fully correct and
    the lowest and highest session of each cell. The headline is what the numbers support: no consistent saving was shown."""
    found = codex_facts()
    if not found:
        return ""
    names = {item["id"]: item["name"] for item in public["sets"]}
    cards = []
    for item in found["sets"]:
        none, with_pgr = item["none"], item["pgr"]
        longest = max(none["tokens"], with_pgr["tokens"])
        more = item["saved"] < 0
        cards.append(
            f'\n    <div class="card cx-card"><h3 class="cx-title">{esc(names[item["id"]])} questions</h3>'
            f'\n      <p class="cx-verdict"><b>{abs(item["saved"]):.1f}%</b> {"more" if more else "fewer"} tokens with pgr</p>'
            f'\n      <div class="rl-fig" aria-hidden="true">{lane_html("ref", "No index", tokens(none["tokens"]), none["tokens"] / longest)}'
            f'{lane_html("best", "With pgr", tokens(with_pgr["tokens"]), with_pgr["tokens"] / longest)}</div>'
            f'\n      <ul class="cx-facts"><li>Mean tokens in a session: {esc(tokens(none["tokens"]))} with no index, {esc(tokens(with_pgr["tokens"]))} with pgr.</li>'
            f'<li>Fully correct sessions: {of(none["perfect"], none["of"])} with no index, {of(with_pgr["perfect"], with_pgr["of"])} with pgr.</li>'
            f'<li>The sessions of a cell ranged from {esc(tokens(none["low"]))} to {esc(tokens(none["high"]))} with no index and from '
            f'{esc(tokens(with_pgr["low"]))} to {esc(tokens(with_pgr["high"]))} with pgr'
            f'{", a spread as wide as the gap between the means" if item["spread_wider_than_gap"] else ""}.</li></ul>\n    </div>')
    limits = [f"{number_word(found['per_cell'], True)} sessions in each cell support no claim of significance.",
              "One repository, the same Python project as the rest of this site.",
              "The Codex CLI counts tokens its own way, with its own instructions, tools and caching, so its counts are not comparable with "
              "the Claude Code counts on this site. Only the two methods inside this check can be compared with each other.",
              "One agent answers three questions in a session. Orchestration, subagents and the memory were not measured under the Codex CLI.",
              "The questions were written by the builders of pgr."]
    every = ("Every session was fully correct, so correctness separated nothing." if found["all_correct"]
             else f"{found['correct']} of {found['of']} sessions were fully correct.")
    return ('<section class="section" aria-labelledby="second-harness">\n  <h2 id="second-harness" class="section-title">Under the Codex CLI</h2>\n  '
            f'<p class="cx-headline">{esc(codex_headline(found))}</p>\n  '
            f'<p class="group-title">A small second check with {esc(codex_model_line())}: the same three lookups, one agent a session, '
            f'{number_word(found["per_cell"])} sessions in each cell, on both question sets. On the first set the sessions with pgr, against those with no index, used {codex_direction(found["sets"][0])} tokens; on the second set they used {codex_direction(found["sets"][1])}. '
            f'{esc(every)} The figures are the means of the sessions of each cell, and they are not set beside the Claude Code figures above.</p>\n  '
            f'<div class="cx-grid draw">{"".join(cards)}\n  </div>\n  '
            f'<ul class="plain cx-limits">{"".join(f"<li>{esc(line)}</li>" for line in limits)}</ul>\n</section>\n')


def codex_study_slot(public: dict) -> str:
    """The method of the second check, for the Study page: what ran, how many sessions, what it cannot show."""
    found = codex_facts()
    if not found:
        return ""
    rows = [("Agent program and model", f"{codex_model_line()[0].upper()}{codex_model_line()[1:]}. Run on {found['sessions']} sessions."),
            ("What was run", f"{number_word(found['per_cell'], True)} sessions in each of four cells: no index and pgr, on the original and on the held-out questions. "
                             "One agent answers all three questions in a session."),
            ("How it came out", f"{codex_verdict(found)[0].upper()}{codex_verdict(found)[1:]}. On the original questions the sessions with pgr, against those with no index, used {abs(found['sets'][0]['saved']):.1f}% "
                                f"{'more' if found['sets'][0]['saved'] < 0 else 'fewer'} tokens, on the held-out questions {abs(found['sets'][1]['saved']):.1f}% "
                                f"{'more' if found['sets'][1]['saved'] < 0 else 'fewer'}. "
                                + ("Every session was fully correct." if found["all_correct"] else f"{found['correct']} of {found['of']} sessions were fully correct.")),
            ("What it does not cover", "Other models, other efforts, other repositories and other agent programs. Orchestration, subagents and the memory."),
            ("Why its counts stand alone", "The Codex CLI counts tokens its own way, with its own instructions, tools and caching. Only the two methods inside this check can be compared with each other; the counts are not comparable with the Claude Code counts.")]
    return ('<section class="section" aria-labelledby="second">\n    <h2 id="second" class="section-title">A second agent program</h2>\n    '
            f'<ul class="rows">{"".join(f"<li><b>{esc(a)}</b><span>{esc(glue(b))}</span></li>" for a, b in rows)}</ul>\n  </section>\n')


def models_lanes() -> str:
    """How many of the agent transcripts of the benchmark ran on each model, to one scale, with what models.json says about how that
    was read. The block under the list of the three models on the Study page."""
    items = [MODELS["tiers"][tier] for tier in "CBA" if MODELS["tiers"][tier].get("transcripts")]
    if not items:
        return ""
    most = max(int(item["transcripts"]) for item in items)
    lanes = "".join(lane_html("best", f"{item['name']}, {item['effort']} effort", f"{int(item['transcripts']):,} transcripts", int(item["transcripts"]) / most)
                    for item in items)
    return f'\n      <div class="rl-fig draw">{lanes}</div>\n      <p class="footnote">{esc(MODELS["how"])}</p>'


def packed_how(packed: dict) -> str:
    """footprint.json's note on the packed index, with its first words made into a sentence of their own."""
    how = packed.get("how", "")
    lead = "The same test repository indexed by the release build, every index file written"
    return how.replace(lead, "The same test repository was indexed by pgr 0.2.0, with every index file written", 1) if how.startswith(lead) else how


def size_slot(public: dict) -> str:
    """Size on disk: the programs and runtimes of the first generation against the one pgr file, with and without its model, and
    the index of the test repository for each. What is counted is said as benchmark/data/footprint.json says it."""
    data = footprint_of()
    if not data:
        return ""
    first, pgr = data["first_generation"], data["pgr"]
    packed = data.get("pgr_0_2")   # the index of the next version, written with a lossless codec, measured in bytes
    top_a = max(first["programs_and_runtimes_mb"], pgr["program_with_model_mb"], pgr["program_without_model_mb"])
    # footprint.json counts in mebibytes (1,048,576 bytes); the pages use millions of bytes, the unit of the Install page.
    mb = lambda value: f"{value * 1.048576:.1f} MB"
    top_b = max(first["index_of_test_repository_mb"], pgr["index_of_test_repository_mb"]) * 1.048576
    if packed:
        top_b = max(top_b, packed["index_before_bytes"] / 1e6, packed["index_of_test_repository_bytes"] / 1e6)
    parts = "; ".join(f"{part['what']}, {mb(part['mb'])}" for part in first["parts"])
    programs = (lane_html("ref", "First generation: separate tools and their runtimes", mb(first["programs_and_runtimes_mb"]), first["programs_and_runtimes_mb"] / top_a)
                + lane_html("best", "pgr, one file, with its model", mb(pgr["program_with_model_mb"]), pgr["program_with_model_mb"] / top_a)
                + lane_html("best", "pgr, one file, without its model", mb(pgr["program_without_model_mb"]), pgr["program_without_model_mb"] / top_a))
    index = lane_html("ref", "First generation", mb(first["index_of_test_repository_mb"]), first["index_of_test_repository_mb"] * 1.048576 / top_b)
    if packed:
        mb_b = lambda count: f"{count / 1e6:.1f} MB"
        index += (lane_html("best", f"pgr {pgr['version']}", mb_b(packed["index_before_bytes"]), packed["index_before_bytes"] / 1e6 / top_b)
                  + lane_html("best", "pgr 0.2.0, packed", mb_b(packed["index_of_test_repository_bytes"]), packed["index_of_test_repository_bytes"] / 1e6 / top_b)
                  + lane_html("best", "pgr 0.2.0, packed, without the vectors", mb_b(packed["index_without_vectors_bytes"]), packed["index_without_vectors_bytes"] / 1e6 / top_b))
    else:
        index += (lane_html("best", "pgr", mb(pgr["index_of_test_repository_mb"]), pgr["index_of_test_repository_mb"] * 1.048576 / top_b)
                  + lane_html("best", "pgr, without the vectors", mb(pgr["index_without_vectors_mb"]), pgr["index_without_vectors_mb"] * 1.048576 / top_b))
    apart = (f"Listed apart from the programs: the first generation's models, caches and index folders hold {mb(first['models_and_index_data_mb'] if 'models_and_index_data_mb' in first else first['models_caches_and_index_data_mb'])}, "
             f"for a total of {mb(first['total_mb'])}. The new program is {pgr['files']} file."
             + (f" The model inside the program file was {packed['model_in_program']}." if packed and packed.get("model_in_program") else ""))
    when_packed = f"; the packed index of pgr 0.2.0 on {packed['measured']}" if packed and packed.get("measured") else ""
    return (f'<h2 id="size" class="section-title">Size on disk</h2>\n    <p class="group-title">{esc(data["how"])} Sizes are in megabytes of a million bytes, the unit of the Install page. Measured {esc(data["measured"])} on {esc(data["machine"])}{esc(when_packed)}.</p>\n    '
            f'<div class="size-grid draw">\n      <div class="card size-card"><h3>Programs and runtimes</h3><div class="rl-fig">{programs}</div>'
            f'<p class="footnote">The first generation is {esc(parts)}.</p></div>\n      '
            f'<div class="card size-card"><h3>Index of the test repository</h3><div class="rl-fig">{index}</div>'
            '<p class="footnote">The index is what the engine builds once for a repository and reads on every lookup.'
            + (f' The packed index of pgr 0.2.0 is {abs(packed["index_change_percent"]):.0f}% smaller than the one before, {mb_b(packed["index_before_bytes"])} to '
               f'{mb_b(packed["index_of_test_repository_bytes"])}, and lossless. {esc(packed_how(packed))}' if packed else "")
            + '</p></div>\n    </div>\n    '
            f'<p class="footnote">{esc(apart)}</p>\n    <p class="lang-line">{esc(data["language"])}</p>')


def platform_line() -> str:
    """One line for the Overview: what was tested on which system, and that the Windows and Linux builds gave the same answers."""
    data = platforms_of()
    if not data:
        return ""
    systems = [item["system"] for item in data["tests"]]
    same = str(data["same_answers"][0]["result"]).split(" (")[0] if data["same_answers"] else ""
    where = " and on ".join(systems[:2]) + (f", and the engine's tests also run on {systems[2]} build machines" if len(systems) > 2 else "")
    return f"Tested on {where}. " + (f"Same answers on Windows and Linux: {same}." if same else "")


def plain_pip(text: str) -> str:
    """The test notes name the pip route as a command; the page says it in words."""
    return text.replace("pip install of the wheel from the release", "installing the wheel from the release with pip")


def platform_block() -> str:
    """What was tested where, for the Install page: the test suites, the identical answers, the installers, and what was built
    and not run on this machine."""
    data = platforms_of()
    if not data:
        return ""
    tests = "".join(f"\n          <li><b>{esc(t['system'])}</b> ({esc(t['where'])}): {esc(t['what'])}. {esc(t['result'])}.</li>" for t in data["tests"])
    same = "".join(f"\n          <li>{esc(t['what'])}: {esc(t['result'])}.</li>" for t in data["same_answers"])
    ran = "".join(f"\n          <li><b>{esc(t['system'])}</b>: {esc(plain_pip(t['what']))}: {esc(t['result'])}.</li>" for t in data["installers"])
    built = "".join(f"\n          <li>{esc(line)}</li>" for line in data.get("built_not_run_here") or [])
    return (f'\n      <p class="block-lede">Checked {esc(data["checked"])}.</p>\n      <div class="tested-grid">\n'
            f'        <div class="card-plain"><h3>Test suites</h3><ul>{tests}\n        </ul></div>\n'
            f'        <div class="card-plain"><h3>Same answers on different systems</h3><ul>{same}\n        </ul></div>\n'
            f'        <div class="card-plain"><h3>The installers, run from the public release</h3><ul>{ran}\n        </ul></div>\n'
            f'        <div class="card-plain"><h3>Built but not run here</h3><ul>{built}\n        </ul></div>\n      </div>')


LAB_NAMES = {"words": "Words only, no neural network", "meaning": "Meaning only, with embeddings", "hybrid": "Both together, the default"}
LAB_LIMITS = (
    "The queries come from developer-written test names and commit subjects, not from questions an agent would ask.",
    "Credit for a commit subject is file-level and lenient: any hit in a changed file counts.",
    "There are five code bases in three languages. One of them has no named tests, so it is judged on commit subjects alone.",
    "The default variant was built by people who had seen none of these challenges, and the figures here are the held-out half.",
    "Pooled rows weight each challenge equally, so the larger code bases count more.",
    "A test-name query and its answer share words by construction. The numbers compare variants; they are not a forecast of agent behaviour.",
    "Credit by cover matching goes to a block that contains the function, so a variant that returns coarser blocks gains from it. The data file keeps the exact-matching numbers beside the main ones for that reason.",
    "Answers in example, documentation, compatibility and legacy folders are left out, because the engine demotes most of them on purpose. A change to that policy is not measured here.",
    "A variant whose weights were searched with the tuning tool was searched on the tune half only and is judged here on the held-out half. The tune half holds "
    "few test-name challenges and a few hundred commit-subject challenges, so a tuned variant can fit it and gain nothing held out.")


def lab_facts() -> dict | None:
    """benchmark/data/search_lab.json: for the held-out half, the pooled recall at 1 and at 5 and the mean reciprocal rank of each
    variant on the commit-subject challenges with their intervals, the paired difference of each against the default, and the size
    of the test-name set."""
    path = BENCH / "data" / "search_lab.json"
    if not path.is_file():
        return None
    try:
        lab = json.loads(path.read_text(encoding="utf-8"))
        half = lab["evaluation"]["held-out"]
        base = half["baseline"]
        rows = {m["variant"]: m for m in half["metrics"] if m["scope"] == "pooled" and m["kind"] == "C"}
        pairs = {m["variant"]: m for m in half["paired"] if m["scope"] == "pooled" and m["kind"] == "C"}
        tests = {m["variant"]: m for m in half["metrics"] if m["scope"] == "pooled" and m["kind"] == "T"}
        corpora = len(lab["corpora"])
        c_corpora = len({cell["corpus"] for cell in half["cells"] if cell["kind"] == "C" and cell["n"] > 0})
        shares = sorted(float(entry["stats"]["c_top_file_share"]) for entry in lab["challenges"]["per_corpus"].values()
                        if "c_top_file_share" in entry.get("stats", {}) and entry["funnel"]["C"]["kept"] >= 10)
        resamples, interval, limits = int(lab["bootstrap"]["resamples"]), float(lab["bootstrap"]["interval"]), len(lab["limits"])
    except (ValueError, KeyError, TypeError):
        return None
    if base not in rows or not set(LAB_NAMES) <= set(rows):
        return None
    if limits != len(LAB_LIMITS):
        raise SystemExit(f"search_lab.json states {limits} limits and LAB_LIMITS of build_site.py says {len(LAB_LIMITS)}: put the change in plain words")
    return {"rows": rows, "pairs": pairs, "tests": tests, "base": base, "corpora": corpora, "k": lab.get("k"), "c_corpora": c_corpora,
            "shares": shares, "resamples": resamples, "interval": interval}


def lab_slot(public: dict) -> str:
    """One block of the Study page: the search without a neural network, measured on challenges made from five code bases."""
    found = lab_facts()
    if not found:
        return ""
    pct = lambda cell: f"{cell['v'] * 100:.1f}%"
    span = lambda cell: f"{cell['lo'] * 100:.1f}% to {cell['hi'] * 100:.1f}%"
    # A figure between 0 and 1 drawn as a bar, with its interval as a bracket on the same scale.
    mbar = lambda cell: (f'<span class="mbar" aria-hidden="true"><i style="--v:{cell["v"]:.4f}"></i>'
                         f'<b style="--a:{cell["lo"]:.4f};--b:{cell["hi"]:.4f}"></b></span>')
    order = [found["base"]] + [v for v in LAB_NAMES if v != found["base"]]
    body = ""
    for variant in order:
        m = found["rows"][variant]
        body += (f'\n        <tr><th scope="row" class="cfg"><span class="lab-name">{esc(LAB_NAMES[variant])}</span></th>'
                 f'<td data-label="Recall at 1">{pct(m["recall1"])}<span class="sub">{span(m["recall1"])}</span>{mbar(m["recall1"])}</td>'
                 f'<td data-label="Recall at 5">{pct(m["recall5"])}<span class="sub">{span(m["recall5"])}</span>{mbar(m["recall5"])}</td>'
                 f'<td data-label="Mean reciprocal rank">{m["mrr"]["v"]:.3f}<span class="sub">{m["mrr"]["lo"]:.3f} to {m["mrr"]["hi"]:.3f}</span>{mbar(m["mrr"])}</td></tr>')
    words = []
    for variant in order[1:]:
        pair = found["pairs"].get(variant)
        if not pair:
            continue
        bits = []
        for key, label in (("recall1", "recall at 1"), ("recall5", "recall at 5"), ("mrr", "mean reciprocal rank")):
            cell = pair[key]
            scale = 1 if key == "mrr" else 100
            unit = "" if key == "mrr" else " points"
            bits.append(f"{label} {cell['v'] * scale:+.{3 if key == 'mrr' else 1}f}{unit} (interval {cell['lo'] * scale:+.{3 if key == 'mrr' else 1}f} to "
                        f"{cell['hi'] * scale:+.{3 if key == 'mrr' else 1}f}, {'excludes' if cell['excludes_zero'] else 'includes'} zero)")
        words.append(f"<li><b>{esc(LAB_NAMES[variant])}</b>, against {esc(LAB_NAMES[found['base']].lower())}: {esc('; '.join(bits))}.</li>")
    limits = list(LAB_LIMITS)
    if found["shares"]:
        low, high = found["shares"][0] * 100, found["shares"][-1] * 100
        limits.insert(1, f"In {number_word(len(found['shares']))} of the code bases a single file is the answer to {low:.0f} to {high:.0f}% of the commit-subject challenges, "
                         "so credit piles up on that file.")
    tests = found["tests"].get(found["base"])
    small = (f'<p class="cx-note">The test-name challenges are too few to settle anything: {tests["n"]} held-out queries pooled over all the code bases. '
             f'For the default variant their recall at 1 is {pct(tests["recall1"])} with an interval of {span(tests["recall1"])}, wide enough to hold every variant. '
             'They are reported and not interpreted.</p>' if tests else "")
    return ('<section class="section" aria-labelledby="lab">\n    <h2 id="lab" class="section-title">Search without a neural network</h2>\n    '
            f'<p class="group-title">Challenges made from {number_word(found["corpora"])} code bases, with no model and no human labels: the query is the words of a commit subject, '
            f'and the right answer is a file that commit changed; the commit-subject challenges come from {number_word(found["c_corpora"])} of the code bases. '
            'Three variants of the search are scored on the same challenges, held-out half. '
            f'Intervals are {found["interval"]:.0%} from {found["resamples"]:,} resamples of the challenges.</p>\n    '
            f'<div class="table-wrap draw"><table class="lab-table"><thead><tr><th scope="col">Variant</th><th scope="col">Recall at 1</th><th scope="col">Recall at 5</th>'
            f'<th scope="col">Mean reciprocal rank</th></tr></thead><tbody>{body}\n      </tbody></table></div>\n    '
            f'<p class="group-title">Paired difference against the default, on the same challenges:</p>\n    <ul class="plain lab-pairs">{"".join(words)}</ul>\n    '
            f'{small}\n    <ul class="plain cx-limits">{"".join(f"<li>{esc(line)}</li>" for line in limits)}</ul>\n  </section>\n')


# ----------------------------------------------------------------------------------------------
# Round six: teams of agents. Every figure the three pages show about it is computed here from the team group of arms.json
# and the parsed results; the guard reads these pages as it reads the rest.
# ----------------------------------------------------------------------------------------------

TEAM_SETS = (("original", ""), ("held-out", "h"))   # the held-out twin of a team arm has the id of the arm and an h
TEAM_CLASS = {"C": "light", "B": "mid", "A": "top"}
TEAM_NOTE_ARMS = (("T0", "no notes"), ("TL", "the agent loads the notes"), ("TI", "the notes in the brief"))


def change_words(change: float) -> str:
    """A change in tokens as a page words it: 32% fewer, 13% more, or about the same under half a percent."""
    return "about the same" if abs(change) < 0.005 else f"{round(abs(change) * 100)}% {'fewer' if change < 0 else 'more'}"


def changes_text(changes: list[float]) -> str:
    """Several changes in tokens as one phrase: 27% to 32% fewer, or from 3% fewer to 3% more when they differ in sign."""
    if all(c < 0 for c in changes) or all(c >= 0 for c in changes):
        whole = sorted(round(abs(c) * 100) for c in changes)
        low, high = whole[0], whole[-1]
        word = "fewer" if changes[0] < 0 else "more"
        if low == 0 and high > 0:
            return f"up to {high}% {word}"
        return f"{low}% {word}" if low == high else f"{low}% to {high}% {word}"
    ordered = sorted(changes)
    return f"from {change_words(ordered[0])} to {change_words(ordered[-1])}"


def signed_range(changes: list[float]) -> str:
    """Several changes in tokens as one range with its signs: −15% to +3%."""
    sign = lambda value: ("−" if value < 0 else "+") + f"{round(abs(value) * 100)}%"
    low, high = sign(min(changes)), sign(max(changes))
    return low if low == high else f"{low} to {high}"


def span_text(values: list[float], one) -> str:
    """The lowest and the highest of some values, as one value when they print the same: 70k, or 70k to 71k."""
    low, high = one(min(values)), one(max(values))
    return low if low == high else f"{low} to {high}"


def turns_phrase(values: list[float]) -> str:
    """Turns as a page words them: one turn, or 2.0 to 2.7 turns."""
    return "one turn" if all(abs(v - 1) < 0.05 for v in values) else span_text(values, lambda v: f"{v:.1f}") + " turns"


def scaled(groups: list[tuple[str, list[tuple[str, str, float]]]]) -> list[dict]:
    """Groups of lanes, (title, [(kind, name, tokens)]), drawn to one scale: the longest bar is the most tokens in any group."""
    longest = max((value for _, items in groups for _, _, value in items), default=1.0) or 1.0
    return [{"title": title, "items": [{"kind": kind, "name": name, "text": tokens(value), "value": round(value / longest, 4)}
                                       for kind, name, value in items]} for title, items in groups]


def team_facts(points: list[dict], cells: list[dict], agents: list[dict], arms: dict) -> dict | None:
    """Round six in plain words with its numbers: what ran, what it found (one finding for each question the round asked) and
    what it cannot show. None when the results hold no team cell. A finding is written only when the numbers it quotes exist."""
    ids = {a["id"] for a in arms["arms"] if a.get("group") == "team"}
    runs = [c for c in cells if c["complete"] and c["arm"] in ids]
    if not runs:
        return None
    light, mid, top = (MODELS["tiers"][t]["name"] for t in "CBA")
    named = {"light": "C", "mid": "B", "top": "A"}

    def at(arm: str, setup: str, tier: str, sid: str) -> dict | None:
        return pick(points, arm + dict(TEAM_SETS)[sid], setup, tier)

    def row(pt: dict) -> dict:
        return {"tokens": pt["tokens"], "turns": pt["requests"], "runs": pt["n"], "perfect": pt["spread"]["perfect_runs"],
                "low": pt["spread"]["tokens"][0], "high": pt["spread"]["tokens"][1]}

    def runs_at(arm: str, setup: str, tier: str, sid: str) -> list[dict]:
        """The runs of one cell: an arm, a setup, a model and a question set."""
        return [c for c in runs if c["arm"] == arm + dict(TEAM_SETS)[sid] and c["setup"] == setup and c["tier"] == tier]

    def altered_in(pt: dict) -> int:
        """How many runs of a point had a read altered by another tool on the test machine (the tool the Study page describes
        under the no-index control). Such a run can lose lines it needed, or re-read them, so it is counted wherever it sits."""
        return sum(c["hook_rewrites"] > 0 for c in runs if (c["arm"], c["setup"], c["tier"]) == (pt["arm"], pt["setup"], pt["tier"]))

    def compare(base: tuple, other: tuple, tier: str = "C", tag: str = "") -> list[dict]:
        """One entry for each question set: the base setup and the other, each as (arm, setup), on the same model. The tag says
        which setup the pair is, for the rows and lanes of a finding that holds several. "altered" counts the runs of each side that
        had a read altered by the other tool, and "clean" is the change in mean tokens with those runs set aside (None when it
        would leave a side with no run, or when no run was altered)."""
        found = []
        for sid, _ in TEAM_SETS:
            b, o = at(base[0], base[1], tier, sid), at(other[0], other[1], tier, sid)
            if b and o:
                hit = (altered_in(b), altered_in(o))
                kept_b = [c["tokens"] for c in runs_at(base[0], base[1], tier, sid) if not c["hook_rewrites"]]
                kept_o = [c["tokens"] for c in runs_at(other[0], other[1], tier, sid) if not c["hook_rewrites"]]
                found.append({"set": sid, "class": TEAM_CLASS[tier], "tier": tier, "tag": tag, "base": row(b), "other": row(o),
                              "change": o["tokens"] / b["tokens"] - 1, "altered": hit,
                              "clean": (statistics.fmean(kept_o) / statistics.fmean(kept_b) - 1) if sum(hit) and kept_b and kept_o else None})
        return found

    def where(p: dict) -> str:
        return f"{model_at(p['tier'])}, {p['set']} questions" + (f", {p['tag']}" if p.get("tag") else "")

    def tool_note(pairs: list[dict]) -> str:
        """When the other tool altered a read in a run of these pairs: how many runs, and what each such pair's change is without them.
        An empty string when it altered none. Nothing here says the tool changed a result; it says a result rests on such a run."""
        hit = [p for p in pairs if sum(p["altered"])]
        if not hit:
            return ""
        count, total = sum(sum(p["altered"]) for p in pairs), sum(p["base"]["runs"] + p["other"]["runs"] for p in pairs)
        aside = [p for p in hit if p["clean"] is not None]
        return (f" Another tool on the test machine altered a read in {count} of the {total} runs of {'these pairs' if len(pairs) > 1 else 'this pair'}"
                + ("; with those runs set aside the change would be " + "; ".join(f"{change_words(p['clean'])} ({where(p)})" for p in aside) if aside else "") + ".")

    def overlaps(p: dict) -> bool:
        """Whether, in one pair, the lowest and highest run of the two sides cover some tokens in common."""
        return max(p["base"]["low"], p["other"]["low"]) <= min(p["base"]["high"], p["other"]["high"])

    def overlap(pairs: list[dict]) -> bool:
        """Whether, in every pair, the lowest and highest run of the two sides cover some tokens in common."""
        return all(overlaps(p) for p in pairs)

    def widest(pairs: list[dict]) -> float:
        """How far apart the lowest and the highest of the three runs of one cell lie, at most, over both sides of every pair."""
        return max(r["high"] / r["low"] - 1 for p in pairs for r in (p["base"], p["other"]) if r["low"] > 0)

    def lane_groups(pairs: list[dict], base_name: str, other_name: str) -> list[tuple[str, list[tuple[str, str, float]]]]:
        return [(where(p), [("ref", base_name, p["base"]["tokens"]), ("best", other_name, p["other"]["tokens"])]) for p in pairs]

    def counted(pairs: list[dict], side: str | None = None) -> tuple[int, int]:
        """Fully correct runs and runs over one side of every pair, or over both."""
        sides = (side,) if side else ("base", "other")
        return sum(p[s]["perfect"] for p in pairs for s in sides), sum(p[s]["runs"] for p in pairs for s in sides)

    def altered_words(count: int) -> str:
        """The mark a row carries when the other tool altered a read in some of its runs."""
        return f", {count} {'run' if count == 1 else 'runs'} with a read altered by another tool" if count else ""

    def pair_rows(pairs: list[dict], base_name: str, other_name: str) -> list[dict]:
        return [{"label": f"{name}, {where(p)}", "value": f"{tokens(p[side]['tokens'])} tokens, {p[side]['turns']:.1f} turns, {of(p[side]['perfect'], p[side]['runs'])} fully correct"
                                                         + altered_words(p["altered"][0 if side == "base" else 1])}
                for p in pairs for name, side in ((base_name, "base"), (other_name, "other"))]

    def point_row(label: str, pt: dict) -> dict:
        return {"label": label, "value": f"{tokens(pt['tokens'])} tokens, {pt['requests']:.1f} turns, {of(pt['spread']['perfect_runs'], pt['n'])} fully correct" + altered_words(altered_in(pt))}

    findings, figures, evidence = [], [], {}   # evidence: the figures the Install page quotes for its steps
    all_points = [pt for pt in points if pt["arm"] in ids]
    per_cell = sorted({pt["n"] for pt in all_points})
    team_agents = [a for a in agents if a["arm"] in ids]

    # 1. A pack and no tool at all.
    quad = [(tier, sid, at("TPX", "single", tier, sid)) for tier in "CB" for sid, _ in TEAM_SETS]
    if all(pt for _, _, pt in quad):
        tok = [pt["tokens"] for _, _, pt in quad]
        turns = [pt["requests"] for _, _, pt in quad]
        k, n = sum(pt["spread"]["perfect_runs"] for _, _, pt in quad), sum(pt["n"] for _, _, pt in quad)
        same_day = [at("R6", "single", "C", sid) for sid, _ in TEAM_SETS]
        text = (f"With a pack in the prompt and no tool at all, one agent answered the three lookups in {turns_phrase(turns)} on {span_text(tok, tokens)} tokens "
                f"and was fully correct in {of(k, n)} runs over four cells: {light} and {mid}, on the original and the held-out questions.")
        rows = [point_row(f"A pack alone, {model_at(tier)}, {sid} questions", pt) for tier, sid, pt in quad]
        groups = [(f"{model_at(tier)}, {sid} questions", [("best", "A pack alone", pt["tokens"])]) for tier, sid, pt in quad]
        if all(same_day):
            light_quad = [q for q in quad if q[0] == "C"]
            cut = [1 - q[2]["tokens"] / s["tokens"] for q, s in zip(light_quad, same_day)]
            text += (f" The baseline of the same round, one agent with the lookup command and no pack, needed {span_text([s['tokens'] for s in same_day], tokens)} tokens over "
                     f"{turns_phrase([s['requests'] for s in same_day])} on {light}, so on that model the pack alone used {changes_text([-c for c in cut])} tokens."
                     + tool_note(compare(("R6", "single"), ("TPX", "single"))))
            rows += [point_row(f"Lookup command, no pack, {model_at('C')}, {sid} questions", pt) for (sid, _), pt in zip(TEAM_SETS, same_day)]
            for (sid, _), s in zip(TEAM_SETS, same_day):
                next(g for g in groups if g[0] == f"{model_at('C')}, {sid} questions")[1].append(("ref", "Lookup command, no pack", s["tokens"]))
        findings.append({"id": "pack-alone", "title": "A pack alone", "text": text, "rows": rows, "lanes": scaled(groups)})
        evidence["pack_alone"] = {"tokens": span_text(tok, tokens), "turns": turns_phrase(turns), "perfect": k, "runs": n}
        figures.append((span_text(tok, tokens), f"tokens in {turns_phrase(turns)} with a pack and no tool at all; fully correct in {of(k, n)} runs", ""))

    # 2. Packs in the briefs of three subagents started together, and of an orchestrated team.
    para = compare(("R6", "parallel"), ("TP", "parallel"), tag="three subagents")
    orch = compare(("R6", "orch"), ("TP", "orch"), tag="with an orchestrator")
    if para:
        k, n = counted(para)
        text = (f"Three subagents started together, one question each, with nobody checking their answers ({light}): with a pack in each brief the three used "
                f"{changes_text([p['change'] for p in para])} tokens than without, and every answer was fully correct ({of(k, n)} runs, both ways).")
        rows = pair_rows(para, "No pack", "A pack in each brief")
        if orch:
            ko, no = counted(orch)
            evidence["orchestrated"] = {"change": changes_text([p["change"] for p in orch]), "perfect": ko, "runs": no}
            text += (f" With an orchestrator ({top}) that writes the briefs and checks the answers, the packs moved the total {changes_text([p['change'] for p in orch])} tokens "
                     f"({of(ko, no)} runs fully correct)" + (", and the runs of the two sides overlap." if overlap(orch) else ".") + tool_note(orch))
            rows += pair_rows(orch, "No pack", "A pack")
        evidence["briefs"] = {"change": changes_text([p["change"] for p in para]), "perfect": k, "runs": n}
        findings.append({"id": "pack-in-briefs", "title": "Packs in the briefs of three subagents", "text": text, "rows": rows,
                         "lanes": scaled(lane_groups(para, "No pack", "A pack in each brief"))})
        figures.append((changes_text([p["change"] for p in para]).replace(" fewer", ""),
                        f"with a pack in the brief of each of three subagents; {of(k, n)} runs fully correct", "fewer tokens"))

    # 3. One agent with the pack and the lookup commands as well.
    solo = compare(("R6", "single"), ("TP", "single"), tag="one agent")
    both = [c for c in runs if c["arm"] in ("TP", "TPh") and c["setup"] == "single"]
    if solo and both:
        looked, one_turn = sum(c.get("pgr_lookups", 0) > 0 for c in both), sum(c["requests"] <= 1 for c in both)
        text = (f"One agent given the pack and the lookup commands as well, on {light}, used {changes_text([p['change'] for p in solo])} tokens than the same agent with no pack. "
                f"Over its {number_word(len(both))} runs, on both models and both question sets, it ran a lookup anyway in {looked} and answered in one turn in {one_turn}: "
                "with commands in reach, the pack did not take the asking turn away." + tool_note(solo))
        evidence["with_commands"] = {"looked": looked, "runs": len(both), "one_turn": one_turn}
        findings.append({"id": "pack-with-commands", "title": "A pack and the commands together", "text": text,
                         "rows": pair_rows(solo, "No pack", "A pack and the commands"), "lanes": scaled(lane_groups(solo, "No pack", "A pack and the commands"))})

    # 4. The team hub, on lookup work and on a demonstration set built to need it.
    hub_a, hub_b = compare(("R6", "orch"), ("TH", "orch"), tag="orchestrated"), compare(("TP", "orch"), ("TPH", "orch"), tag="orchestrated")
    if hub_a or hub_b:
        hub = hub_a + hub_b
        k, n = counted(hub)
        text = (f"On lookup work an orchestrated team ({top} orchestrating, {light} answering) used {changes_text([p['change'] for p in hub])} tokens when its subagents filed what "
                f"they found in the hub and the reviewer started from it, and gained no accuracy: {of(k, n)} runs were fully correct, with the hub and without." + tool_note(hub))
        rows = pair_rows(hub_a, "No hub", "A team hub") + pair_rows(hub_b, "A pack, no hub", "A pack and a team hub")
        notes = {}
        for arm, _ in TEAM_NOTE_ARMS:
            mine, point = [c for c in runs if c["arm"] == arm and c["tier"] == "C"], pick(points, arm, "single", "C")
            if mine and point and all("note_fields" in c for c in mine):
                notes[arm] = (sum(c["note_fields"]["right"] for c in mine), sum(c["note_fields"]["of"] for c in mine), point)
        demo = len(notes) == 3 and len({v[1] for v in notes.values()}) == 1
        lowest = apart = False
        if demo:
            spans = {arm: notes[arm][2]["spread"]["tokens"] for arm in notes}
            lowest = min(notes, key=lambda arm: notes[arm][2]["tokens"]) == "TI"
            apart = all(spans["TI"][1] < spans[other][0] for other in ("T0", "TL"))   # the slowest run with the notes in the brief is under every other run
            rank = (", the lowest mean of the three" + ("" if apart else ", though the runs of the three overlap")) if lowest else ""
            text += (f" A small demonstration set, written by the builders so that each answer needs a note an earlier agent filed, shows what the hub is for: {light} got "
                     f"{of(notes['T0'][0], notes['T0'][1])} note fields right with no notes, {of(notes['TL'][0], notes['TL'][1])} when it loaded the notes itself and "
                     f"{of(notes['TI'][0], notes['TI'][1])} when the notes were put in its brief{rank}: {tokens(notes['TI'][2]['tokens'])} tokens against "
                     f"{tokens(notes['TL'][2]['tokens'])} to load them and {tokens(notes['T0'][2]['tokens'])} with none.")
            rows += [{"label": f"Demonstration set, {label}, {model_at('C')}",
                      "value": f"{of(notes[arm][0], notes[arm][1])} note fields right, {tokens(notes[arm][2]['tokens'])} tokens, {notes[arm][2]['requests']:.1f} turns"}
                     for arm, label in TEAM_NOTE_ARMS]
        evidence["hub"] = {"change": changes_text([p["change"] for p in hub]), "perfect": k, "runs": n}
        if demo:
            evidence["notes"] = {"none": [notes["T0"][0], notes["T0"][1]], "loaded": [notes["TL"][0], notes["TL"][1]], "brief": [notes["TI"][0], notes["TI"][1]],
                                 "lowest": lowest, "apart": apart}
        findings.append({"id": "hub", "title": "The team hub", "text": text, "rows": rows,
                         "lanes": scaled(lane_groups(hub_a + hub_b, "No hub", "A team hub"))})
        figures.append((changes_text([p["change"] for p in hub]).replace(" more", ""), "with the team hub on lookup work, and no accuracy gained", "more tokens"))
        if demo:
            figures.append((of(notes["TI"][0], notes["TI"][1]), f"note fields right with the notes in the brief, against {of(notes['T0'][0], notes['T0'][1])} with none; a demonstration set", ""))

    # 5. The tools inside an Ubuntu virtual machine.
    vm = (compare(("R6", "single"), ("R6V", "single"), tag="one agent") + compare(("TH", "orch"), ("THV", "orch"), tag="orchestrated, with the hub")
          + compare(("TPH", "orch"), ("TPHV", "orch"), tag="orchestrated, with a pack and the hub"))
    if vm:
        k, n = counted(vm, "other")
        kb, nb = counted(vm, "base")
        text = (f"With the repository and the tools inside an Ubuntu virtual machine (WSL 2) and the agent outside it, {of(k, n)} runs were fully correct, against {of(kb, nb)} on Windows, "
                f"and the mean tokens moved {changes_text([p['change'] for p in vm])} over {number_word(len(vm))} pairs, while the three runs of one cell differ from each other by up to "
                f"{widest(vm):.0%}." + tool_note(vm) + " The tools ran inside the machine and no step used the desktop, the mouse or the keyboard; "
                "nothing here measured an agent working beside a person at the same computer.")
        findings.append({"id": "virtual-machine", "title": "The tools in a virtual machine", "text": text,
                         "rows": pair_rows(vm, "On Windows", "In the virtual machine"), "lanes": scaled(lane_groups(vm, "On Windows", "In the virtual machine"))})
        figures.append((of(k, n), "runs fully correct with the tools in an Ubuntu virtual machine; on Windows " + of(kb, nb), ""))

    # 6. No neural network.
    words, words_pack = compare(("R6", "single"), ("RN", "single"), tag="no pack"), compare(("TP", "single"), ("TPN", "single"), tag="with a pack")
    if words:
        kw, nw = counted(words, "other")
        kb, nb = counted(words, "base")
        lab = lab_facts()
        tied = ""
        if lab and "words" in lab["pairs"] and not any(lab["pairs"]["words"][key]["excludes_zero"] for key in ("recall1", "recall5", "mrr")):
            tied = " In the search lab, on the Study page, the search without it was not told apart from the default."
        text = (f"With the search working on words alone, {of(kw, nw)} runs were fully correct, against {of(kb, nb)} with the neural network, on {light}; the mean tokens moved "
                f"{changes_text([p['change'] for p in words])}.{tied}")
        # The runs that were not fully correct without the network: did the other tool alter a read in them?
        word_runs = [c for sid, _ in TEAM_SETS for c in runs_at("RN", "single", "C", sid)]
        missed = [c for c in word_runs if c["quality"] < 0.999]
        missed_hit = [c for c in missed if c["hook_rewrites"] > 0]
        untouched_right = all(c["quality"] >= 0.999 for c in word_runs if not c["hook_rewrites"])
        if missed and len(missed_hit) == len(missed):
            blame = "Both" if len(missed) == 2 else "All " + number_word(len(missed))
            text += (f" {blame} of the runs that were not fully correct without the network had a read altered by another tool on the test machine"
                     + (", and the runs without the network that the tool did not touch were all fully correct" if untouched_right else "")
                     + ", so these runs cannot tell the missing network from that tool as the cause.")
        elif missed_hit:
            text += (f" {number_word(len(missed_hit), True)} of the {number_word(len(missed))} runs that were not fully correct had a read altered by another tool on the test machine, "
                     "so these runs cannot fully separate the missing network from that tool.")
        text += tool_note(words)
        if words_pack:
            kp, npk = counted(words_pack, "other")
            text += f" With a pack made on the same words-only search, {of(kp, npk)} runs were fully correct."
        findings.append({"id": "no-neural-network", "title": "No neural network", "text": text,
                         "rows": pair_rows(words, "With the neural network", "Words only") + pair_rows(words_pack, "A pack, with the neural network", "A pack, words only"),
                         "lanes": scaled(lane_groups(words + words_pack, "With the neural network", "Words only"))})
        figures.append((of(kw, nw), f"runs fully correct without the neural network, against {of(kb, nb)} with it"
                        + ("; " + ("both" if len(missed) == 2 else "all") + " of the misses had a read altered by another tool, so the network is not shown to be the cause"
                           if missed and len(missed_hit) == len(missed) else ""), ""))

    # 7. Three models, each in its role.
    roles = compare(("R6", "orch"), ("R6", "orch-mid"), tag="no pack") + compare(("TP", "orch"), ("TP", "orch-mid"), tag="with a pack")
    if roles:
        k, n = counted(roles)
        fewer, other_way = [p["change"] for p in roles if p["change"] < 0], [p["change"] for p in roles if p["change"] >= 0]
        text = (f"{top} plans, {light} looks things up and {MODELS['tiers']['B']['name']} reviews, against the same team with {top} reviewing as well: the total moved "
                f"{changes_text([p['change'] for p in roles])} tokens over {number_word(len(roles))} pairs, and {of(k, n)} runs were fully correct, both ways.")
        shared = sum(overlaps(p) for p in roles)
        if shared:
            text += (f" In {number_word(shared)} of the {number_word(len(roles))} pairs the lowest and highest runs of the two sides cover tokens in common, "
                     "so a difference there is not shown to be real.")
        text += tool_note(roles)
        findings.append({"id": "three-models", "title": "Three models, each in its role", "text": text,
                         "rows": pair_rows(roles, "One model reviewing", "Three models in their roles"),
                         "lanes": scaled(lane_groups(roles, "One model reviewing", "Three models in their roles"))})
        figures.append((signed_range([p["change"] for p in roles]),
                        f"with three models in their roles, against {top} reviewing as well: fewer in {number_word(len(fewer))} of {number_word(len(roles))} pairs"
                        + (f", {changes_text(other_way)} in the other" if other_way else "") + (f"; the runs of the two sides overlap in {number_word(shared)} of them" if shared else "")
                        + f"; {of(k, n)} runs fully correct", "change in tokens"))

    cells_n = len(all_points)
    try:
        packs = json.loads((BENCH / "data" / "packs.json").read_text(encoding="utf-8"))["packs"]["gs"]
        sizes = [q["tokens"] for letter in packs.values() for q in letter.values()]
    except (OSError, ValueError, KeyError):
        sizes = []
    five = [c["start"] for c in cells if c["complete"] and c["arm"] == "RGS" and c["tier"] == "C" and c["setup"] == "single"]
    six = [c["start"] for c in runs if c["arm"] == "R6" and c["tier"] == "C" and c["setup"] == "single"]
    rewrote = [c for c in runs if c["hook_rewrites"] > 0]
    missed_all = [c for c in runs if c["quality"] < 0.999]
    tool = {"runs": len(runs), "acted": sum(c["hook_calls"] > 0 for c in runs), "altered": len(rewrote), "missed": len(missed_all),
            "missed_altered": sum(c["hook_rewrites"] > 0 for c in missed_all)}
    try:
        version = json.loads((BENCH / "data" / "packs.json").read_text(encoding="utf-8"))["pgr_version"].split()[-1]
    except (OSError, ValueError, KeyError, IndexError):
        version = ""
    intro = (f"Round six, run on {ROUND_SIX_DATE}, put the engine in the setups a team of agents uses: {len(team_agents)} agents in {len(runs)} runs over {cells_n} cells, "
             f"{number_word(per_cell[0]) if len(per_cell) == 1 else 'one to ' + number_word(per_cell[-1])} runs a cell. Each finding below sets two setups of this round against "
             "each other, on the same repository and the same question sets. The one comparison that crosses rounds is the highlighted configuration against no index, "
             "because the no-index agent was last run in round five, a day earlier; it is marked where it appears. The builders of the engine wrote the questions.")
    limits = [
        f"{number_word(per_cell[-1], True)} runs a cell cannot support a claim of significance. A difference of one turn between two setups is noise, and so is a difference of one fully correct run.",
        "The questions were written by the builders of the engine, and two of the three in each set name the function in backticks. That suits a pack, which is built from the words of the question. "
        "The third describes a job in words; the pack alone answered it too.",
        "None of this was tested on open-ended work: no bug to find, no change to make, no question that names no function and describes none.",
        "The demonstration set was written to show one feature of the hub. It is not a test nobody tuned for.",
        "The held-out questions are not held out for the packs of this round. The questions were written and committed before pgr 0.2.0, and the part of its pack that finds the "
        "functions a question names was written after them. For round six they are a second set of questions, not an untouched one.",
    ]
    if tool["altered"]:
        limits.insert(1, f"Another tool on the test machine, the one the Study page describes under the no-index control, was installed in round six as well. It altered a read in {tool['altered']} of the "
                         f"round's {tool['runs']} runs, and in {tool['missed_altered']} of the {tool['missed']} runs that were not fully correct. A finding that rests on a pair with such a run says so, and "
                         "gives the change without it. Running the round again with that tool off would settle it; that was not done.")
    return {"agents": len(team_agents), "runs": len(runs), "cells": cells_n, "per_cell": per_cell, "intro": intro, "tool": tool, "pgr_version": version,
            "figures": figures, "findings": findings, "evidence": evidence, "limits": limits,
            "pack_tokens": {"low": min(sizes), "high": max(sizes), "count": len(sizes)} if sizes else None,
            "start_drift": (statistics.fmean(six) - statistics.fmean(five)) if five and six else None,
            "start_five": statistics.fmean(five) if five else None}


def pick_facts(configs: list[dict]) -> dict | None:
    """What the rule picked and why: the picked configuration, how many could have been picked, how many were fully correct in
    every run of the four cells, and the three that follow it in the rule's order."""
    best = next((c for c in configs if c["best"]), None)
    if not best:
        return None
    order = []
    for config in configs:
        found = [cell_of(config, cid, sid) for cid, sid in BEST_CELLS]
        if all(pt and pt["runs"] >= BEST_MIN_RUNS for pt in found):
            order.append({"slug": config["slug"], "name": config["name"], "commandless": bool(config.get("commandless")),
                          "perfect": sum(pt["perfect_runs"] for pt in found), "runs": sum(pt["runs"] for pt in found),
                          "tokens": statistics.fmean(pt["tokens"] for pt in found), "turns": statistics.fmean(pt["turns"] for pt in found)})
    order.sort(key=lambda r: (-r["perfect"] / r["runs"], r["tokens"]))
    mine = next((r for r in order if r["slug"] == best["slug"]), None)
    if not mine:
        return None
    return {**mine, "eligible": len(order), "all_correct": sum(r["perfect"] == r["runs"] for r in order),
            "following": [r for r in order if r["slug"] != best["slug"]][:3]}


PACK_MADE = "Making a pack used no language model: the program's own search found the matches, with its small neural table switched on."


def release_gap(public: dict) -> str:
    """The version of pgr that round six measured, when the release the Install page installs is older than it. Empty when the
    release is as new as the round, or when either version is unknown."""
    team, inst = public.get("team") or {}, public.get("install") or {}
    parse = lambda text: tuple(int(part) for part in re.findall(r"\d+", str(text))[:3])
    measured, shipped = parse(team.get("pgr_version", "")), parse(inst.get("version", ""))
    return team["pgr_version"] if measured and shipped and shipped < measured else ""


def pick_caveat(public: dict) -> str:
    """One paragraph under the headline figures when the highlighted configuration is a pack alone: what it is and the four things to
    read before relying on it, with a link to the section that gives them in full. Empty for any other pick."""
    facts = public.get("pick")
    if not facts or not facts["commandless"]:
        return ""
    version = release_gap(public)
    made = f"A pack made by pgr {version}, not yet released, before the agent started; " if version else "A pack made before the agent started; "
    return ('<p class="pick-line"><b>Read before relying on it.</b> ' + esc(made) + "the agent had no command to call. The builders wrote the questions, and the held-out set is not held out for it. "
            "Open-ended work was not tested. The no-index figure is from round five. "
            '<a href="#pick">What it means</a></p>')


def pick_slot(public: dict) -> str:
    """Beside the highlighted configuration, when the rule picked one that gives the agent no command: what the rule did, what that
    means for a person setting up their own agents, what the questions were, what was not tested, which program version made the
    packs, and that the saving against no index crosses rounds. Empty for any other pick."""
    facts = public.get("pick")
    if not facts or not facts["commandless"]:
        return ""
    team = public.get("team") or {}
    version = release_gap(public)
    after = "".join(f'<li><b>{esc(r["name"])}</b><span>{esc(of(r["perfect"], r["runs"]))} fully correct, {esc(tokens(r["tokens"]))} tokens, {r["turns"]:.1f} turns</span></li>'
                    for r in facts["following"])
    drift, start_five = team.get("start_drift"), team.get("start_five")
    crossing = (f" The same configuration run in both rounds started with a context {abs(drift):,.0f} tokens {'larger' if drift > 0 else 'smaller'} in round six, "
                f"{abs(drift) / start_five:.1%} of that context." if drift is not None and start_five else "")
    unreleased = (('        <li><b>It was measured with a version that is not released.</b> The packs were made with pgr ' + esc(version) + '. The release the Install page installs is '
                   + esc((public.get("install") or {}).get("version", "")) + '; its pack lacks the section of named definitions that these packs open with, so it is not the pack measured here.</li>\n')
                  if version else "")
    return ('\n    <div class="pick" id="pick">\n      <h3 class="pick-title">What the rule picked, and what that means</h3>\n'
            f'      <p class="pick-text">The rule is the highest share of fully correct runs over four cells, then the fewest tokens. {number_word(facts["eligible"], True)} configurations were '
            f'eligible and {number_word(facts["all_correct"])} of them were fully correct in every run of the four cells. A pack alone used the fewest tokens of those, so it is the one marked. '
            'The rule was written after round five and before round six ran. After it, in the rule\'s order, and averaged over the same four cells:</p>\n'
            f'      <ul class="pick-after">{after}</ul>\n'
            '      <ul class="pick-caveats">\n'
            '        <li><b>What a pack alone means for your own agents.</b> The harness that starts the agent runs <code>pgr pack</code> on the task and puts the output in the agent\'s brief. '
            f'The agent answers from it and calls no command and no tool. {esc(PACK_MADE)} The agent made one call, with the pack in its prompt.</li>\n'
            '        <li><b>The questions suit it.</b> The builders wrote them, and two of the three in each set name the function in backticks. A pack is built from the words of the question, so it finds such a function at once.</li>\n'
            '        <li><b>The held-out questions are not held out for it.</b> The part of the pack that finds the functions a question names was written after the held-out questions had been run in round five. '
            'For this pick they are a second set of questions, not an untouched one.</li>\n'
            + unreleased +
            '        <li><b>It has not been tested on open-ended work.</b> Nothing here asked an agent to find a bug, change code or explore a repository it was not pointed at. Where the pack misses what is needed, an agent with no command cannot look further.</li>\n'
            f'        <li><b>The saving against no index crosses rounds.</b> No index was last run in round five, a day earlier than round six.{esc(crossing)}</li>\n'
            '      </ul>\n    </div>')


def team_lanes_html(groups: list[dict]) -> str:
    """The lanes of one finding, a title and two bars for each pair, all drawn to the one scale scaled() chose."""
    if not groups:
        return ""
    return ('\n      <div class="team-lanes draw">' + "".join(
        f'<div class="team-lane"><p class="team-lane-title">{esc(group["title"])}</p><div class="rl-fig">'
        + "".join(lane_html(item["kind"], item["name"], item["text"], item["value"]) for item in group["items"]) + "</div></div>"
        for group in groups) + "</div>")


def team_tiles(figures: list) -> str:
    """The figures of round six set large: the number or range, the direction as a word under it when it is a change in tokens, and what it counts."""
    items = "".join(f'<li><b><span class="fig-n">{esc(figure)}</span>' + (f'<i class="fig-dir">{esc(direction)}</i>' if direction else "")
                    + f'</b><small>{esc(glue(label))}</small></li>' for figure, label, direction in figures)
    return f'<ul class="stats team-stats">{items}</ul>'


def team_overview_slot(public: dict) -> str:
    """The Overview's chapter on round six: what it was, its headline figures set large, what it cannot show, and the way on."""
    team = public.get("team")
    if not team:
        return ""
    tiles = team_tiles(team["figures"])
    fine = "".join(f'\n      <p class="footnote">{esc(glue(line))}</p>' for line in team["limits"][:3])
    return ('<section class="chapter ch-team" aria-labelledby="teams">\n    <h2 id="teams" class="section-title">Teams of agents</h2>\n    '
            f'<p class="group-title">{esc(glue(team["intro"]))}</p>\n    {tiles}\n    '
            f'<div class="fine">{fine}\n    </div>\n    '
            '<p class="close-actions"><a class="btn quiet" href="benchmarks.html#team">The whole round</a>'
            '<a class="btn quiet" href="install.html#team">Use it in a team of agents</a></p>\n  </section>')


def team_bench_slot(public: dict) -> str:
    """The Benchmarks chapter on round six: what the rule picked from it, then one card for each finding with its numbers."""
    team = public.get("team")
    if not team:
        return ""
    cards = ""
    for finding in team["findings"]:
        rows = "".join(f'\n      <li class="row"><span class="label">{esc(r["label"])}</span><span class="value">{esc(glue(r["value"]))}</span></li>' for r in finding["rows"])
        cards += (f'\n    <div class="card finding" id="team-{finding["id"]}">\n      <h3 class="finding-title">{esc(finding["title"])}</h3>\n'
                  f'      <p class="finding-text">{esc(glue(finding["text"]))}</p>{team_lanes_html(finding["lanes"])}\n      <ul class="rows">{rows}\n      </ul>\n    </div>')
    fine = "".join(f'\n      <p class="footnote">{esc(glue(line))}</p>' for line in team["limits"])
    return ('<section class="section" aria-labelledby="team">\n    <h2 id="team" class="section-title">Round six: teams of agents</h2>\n    '
            f'<p class="group-title">{esc(glue(team["intro"]))}</p>{pick_slot(public)}\n    <div class="finding-list">{cards}\n    </div>\n    '
            f'<div class="fine">{fine}\n    </div>\n  </section>')


def team_study_slot(public: dict) -> str:
    """The Study page's section on round six: what ran, in plain words, and what it cannot show."""
    team = public.get("team")
    if not team:
        return ""
    light, mid, top = (MODELS["tiers"][t]["name"] for t in "CBA")
    packs = team.get("pack_tokens")
    drift = team.get("start_drift")
    stats = stats_html([(f"{team['agents']}", "agents in team setups"), (f"{team['runs']}", "runs"), (f"{team['cells']}", "cells, three runs each" if team["per_cell"] == [3] else "cells")])
    items = [
        ("What ran", f"{team['agents']} agents in {team['runs']} runs over {team['cells']} cells, on the same Python repository as every round and the same two sets of three questions, "
                     f"plus a small demonstration set. One agent answered all three questions on {light} at low effort or {mid} at medium effort; three subagents ({light}) were started "
                     f"together, one question each; an orchestrator ({top} at maximum effort) wrote their briefs and checked their answers. Each finding on the Benchmarks page is between two setups of this round; the one comparison across rounds is the highlighted configuration against no index."),
        ("A pack", (f"A pack is what the program's pack command prints for a question: the definitions, callers and matching code it finds, up to a fixed size "
                    f"({packs['low']} to {packs['high']} tokens each in these runs, estimated at four bytes to a token). " if packs else "A pack is what the program's pack command prints for a question. ")
                   + "The harness made each pack before the agent started and put it in the prompt. " + PACK_MADE + " A pack is part of the prompt, so the agent reads it again on every turn."),
        ("A pack alone", "In this setup the agent has the pack in its prompt and may call no command and no tool, and may say that a field is unknown. It measures whether the pack is enough by itself, and what an answer costs when it is not."),
        ("The team hub", "A shared bank of notes, kept by the program with no model and no server, built from the ideas of TencentCloud's TencentDB-Agent-Memory. Subagents file what they found; the reviewer starts from what was filed. "
                         "A note says which roles filed it and how old it is, and is marked stale when the code it names has changed since."),
        ("The demonstration set", "Three questions written by the builders to show one feature of the hub: each needs one lookup and one note that an earlier agent filed. A decision with a ticket, a note that has gone stale, and a fact confirmed by two roles. "
                                  "The notes were filed before the runs. It is a demonstration, not a test nobody tuned for."),
        ("The virtual machine", "An Ubuntu machine under WSL 2. The repository, the Linux build of the program, its index and the note banks are inside the machine and the agent stays outside it; code is printed from inside. "
                                "Wall time is not compared, because it includes starting the machine for every call."),
        ("No neural network", "The program with its one neural part, the table that gives search its sense of meaning, switched off for the index and for every call. Search then works on words alone."),
        ("Three models, each in its role", f"{top} plans the work, {light} does the lookups and {mid} reviews the answers with the program's commands, against {top} doing the planning and the review."),
    ]
    if drift is not None:
        items.append(("Across rounds", f"The no-index control ran in round five. The same configuration run in both rounds started with a context {abs(drift):,.0f} tokens "
                                       f"{'larger' if drift > 0 else 'smaller'} in round six, so the saving of the highlighted configuration against no index crosses two rounds. "
                                       f"The difference is {abs(drift) / team['start_five']:.1%} of that context."))
    rows = "".join(f'\n        <li class="row stack"><span class="label">{esc(name)}</span><span class="detail">{esc(glue(text))}</span></li>' for name, text in items)
    fine = "".join(f'<li>{esc(glue(line))}</li>' for line in team["limits"])
    return ('<section class="section" aria-labelledby="team">\n    <h2 id="team" class="section-title">Round six: teams of agents</h2>\n    '
            f'{stats}\n    <div class="card"><ul class="rows">{rows}\n      </ul></div>\n    <ul class="plain team-limits">{fine}</ul>\n  </section>')


def round_six(public: dict) -> list[str]:
    """The Round six section of RESULTS.md: what the team round ran and found, in the sentences the site gives, with every row."""
    team = public.get("team")
    if not team:
        return []
    out = ["", "## Round six: teams of agents", "", team["intro"], ""]
    pick = public.get("pick")
    if pick:
        out += [f'The highlighted configuration ("{BEST_LABEL}" on the site) is {pick["name"]}: {of(pick["perfect"], pick["runs"])} fully correct over the four cells, '
                f'{tokens(pick["tokens"])} tokens and {pick["turns"]:.1f} turns on average. {pick["eligible"]} configurations were eligible and {pick["all_correct"]} of them '
                "were fully correct in every run of the four cells.", ""]
        if pick["commandless"]:
            version = team.get("pgr_version", "")
            out += ["Read before relying on it: the agent had no command to call, and its pack was made before it started" + (f" by pgr {version}" if version else "") + ". "
                    "The questions were written by the builders and two of the three in each set name the function in backticks, which suits a pack; the held-out set is not held out for it, "
                    "because the part of the pack that finds the functions a question names was written after those questions had been run; nothing asked an agent to do open-ended work; "
                    f"and the saving against no index crosses rounds, because no index was last run in round five, a day before round six ({ROUND_SIX_DATE}).", ""]
    for finding in team["findings"]:
        out += [f"### {finding['title']}", "", finding["text"], ""] + [f"- {row['label']}: {row['value']}" for row in finding["rows"]] + [""]
    out += ["### What round six cannot show", ""] + [f"- {line}" for line in team["limits"]] + [""]
    return out


def size_text(count: int) -> str:
    return f"{count / 1e6:.1f} MB" if count >= 1e5 else f"{count / 1e3:.1f} kB"


ICON_COPY = ('<svg class="i-copy" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><rect x="8.5" y="8.5" width="11" height="11" rx="2.2"/>'
             '<path d="M15.5 8.5V6.7a2.2 2.2 0 0 0-2.2-2.2H6.7a2.2 2.2 0 0 0-2.2 2.2v6.6a2.2 2.2 0 0 0 2.2 2.2h1.8"/></svg>'
             '<svg class="i-check" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="m5 12.8 4.4 4.4L19 7.6"/></svg>')


def copy_button(text: str, label: str) -> str:
    return (f'<button type="button" class="copy" data-copy="{esc(text)}" aria-label="{esc(label)}">{ICON_COPY}'
            '<span class="copy-label">Copy</span></button>')


def cmd_line(text: str, label: str, attrs: str = "") -> str:
    return f'<div class="cmd-line"{attrs}><code>{esc(text)}</code>{copy_button(text, label)}</div>'


def cmdbox_html(install: dict, big: bool = False) -> str:
    """The install command with the system chosen by a control: the two commands are both in the page, a script shows the one
    for the visitor's system, and without a script both are listed. A command is never cut short: where the line is too narrow
    it wraps."""
    by_id = {item["id"]: item for item in install["installers"]}
    tabs = "".join(
        f'<button type="button" role="radio" aria-checked="{"true" if n == 0 else "false"}" tabindex="{0 if n == 0 else -1}" '
        f'data-sys="{item["id"]}">{esc(item["system"])}</button>' for n, item in enumerate(install["installers"]))
    lines = "".join(cmd_line(item["command"], f"Copy the install command for {item['system']}", f' data-sys="{item["id"]}"')
                    for item in (by_id["windows"], by_id["unix"]))
    notes = "".join(f'<p class="cmd-note" data-sys="{item["id"]}">Run it in {esc(item["shell"])}. {esc(item["note"])}</p>' for item in install["installers"])
    return (f'<div class="cmdbox{" big" if big else ""}" data-cmdbox><div class="cmd-tabs" role="radiogroup" aria-label="System">{tabs}</div>'
            f'{lines}{notes}</div>')


def cmdbox_slot(public: dict) -> str:
    return cmdbox_html(public["install"])


def installer_logs(inst: dict) -> str:
    """What each installer prints, as the terminals on the Install page and at the foot of the Overview show it: one log for each
    system, the command that was typed, the lines of the script, and the commands to run next."""
    latest, version = inst["download_base"], inst["version"]
    out = []
    for item in inst["installers"]:
        windows = item["id"] == "windows"
        folder = "C:\\Users\\...\\AppData\\Local\\Programs\\pgr"
        where = f"{folder}\\pgr.exe" if windows else "~/.local/bin/pgr"
        after = (f'\n          <p class="lg">pgr: added {esc(folder)} to your user PATH. Open a new terminal to use the pgr command.</p>' if windows else "")
        nexts = "".join(f'\n          <p class="lg lg-next"><span class="lg-cmd">{esc(cmd)}</span><span class="lg-note">{esc(note)}</span></p>' for cmd, note in (
            ("pgr refresh", "build the index"), ("pgr ask about:SomeFunction", "definition, callers and what it calls"),
            ('pgr find "where tokens expire"', "search by meaning"), ("pgr help", "every command")))
        out.append(f'\n        <div class="log" data-sys="{item["id"]}">\n          <p class="lg lg-in"><span class="lg-cmd">{esc(item["command"])}</span></p>\n          '
                   f'<p class="lg">pgr: downloading from {esc(latest)}</p>\n          '
                   f'<p class="lg">pgr: installed pgr {esc(version)} to {esc(where)}</p>{after}\n          '
                   f'<p class="lg lg-gap">Next, inside a git repository:</p>{nexts}\n        </div>')
    return "".join(out)


def installer_term(inst: dict) -> str:
    """The terminal that replays the installer, for the Install page and the foot of the Overview."""
    return ('<figure class="term term-install" data-install-term aria-label="What the installer prints">\n          '
            '<figcaption class="term-bar"><span class="term-dots" aria-hidden="true"><i></i><i></i><i></i></span>'
            '<span class="term-title">Terminal</span><span class="term-state">done</span></figcaption>\n          '
            f'<div class="term-body">{installer_logs(inst)}\n          </div>\n        </figure>')


def term_slot(public: dict) -> str:
    return installer_term(public["install"])


SCENE_TASK = "Three lookups: who calls load, where parse_config is defined, and which function loads the settings."
SCENE_ASK = 'pgr ask callers:load about:parse_config "find:where settings load"'
SCENE_OUT = ("== callers:load", "  main  src/cli.py:7-40 @19", "== about:parse_config", "  src/config.py:41-58  (path, strict)",
             "== find: where settings load", "  load_settings  src/config.py:12-30")   # the first four lines are the ones a short window shows


def scene_slot(public: dict) -> str:
    """The first screen of the Overview: two terminal windows replaying one session, an agent without an index and an agent
    with pgr. The number of turns and the tokens each window shows are the measured means of the headline cell, which is one
    agent answering three lookups; the file names in the replay are made up, and the page says so. The page is written
    finished, every turn and every number in place, so it reads without a script; a script hides the turns and plays them in
    order. The line that states the saving is always on the page, and the replay only lights it when both windows are done."""
    found = headline_of(public)
    if not found or not found["c"]:
        return ""
    b, c = found["b"], found["c"]
    longest = max(b["tokens"], c["tokens"])

    packed = bool(found["best"].get("commandless")) and b["turns"] < 1.5   # the agent has no command: one turn, the answer

    def window(kind: str, title: str, pt: dict) -> str:
        pack_only = packed and kind == "best"
        n = 1 if pack_only else max(2, min(8, round(pt["turns"])))
        per = pt["tokens"] / n
        rows = []
        for i in range(n):
            if pack_only:
                cmd, out = "answer", ["the pack is already in the prompt", "answers the three lookups"]
            elif kind == "ref":
                cmd = 'search "parse_config"' if i == 0 else ("answer" if i == n - 1 else "read file")
                out = ["matches in several files"] if i == 0 else (["the conversation is read again"] if i < n - 1 else ["answers the three lookups"])
            else:
                cmd = SCENE_ASK if i == 0 else ("answer" if i == n - 1 else "read file")
                out = list(SCENE_OUT) if i == 0 else (["answers the three lookups"] if i == n - 1 else ["the conversation is read again"])
            lines = "".join(f'<span class="ol">{esc(line)}</span>' for line in out)
            rows.append(f'\n        <li class="turn" style="--i:{i};--w:{per / longest * 100:.3f}%"><span class="turn-n">{i + 1}</span>'
                        f'<span class="turn-cmd{" is-cmd" if i == 0 and not pack_only else ""}">{esc(cmd)}</span><span class="turn-plus">+{esc(tokens(per))}</span>'
                        f'<span class="turn-out">{lines}</span></li>')
        segs = "".join(f'<i class="blk" style="--i:{i};--w:{per / longest * 100:.3f}%"></i>' for i in range(n))
        done = f"answered in {n} turn" + ("" if n == 1 else "s")
        named = f'<span class="bt-name">{esc(found["best"]["name"])}</span>' if kind == "best" else ""
        pack_line = ('<p class="term-task term-pack"><span>pack</span>Made by the program before the agent started: the definitions, callers and '
                     'matching code for the three lookups.</p>\n      ' if pack_only else "")
        return (f'\n    <figure class="term term-{kind}" data-turns="{n}" data-tokens="{pt["tokens"]:.0f}">\n      '
                f'<figcaption class="term-bar"><span class="term-dots" aria-hidden="true"><i></i><i></i><i></i></span>'
                f'<span class="term-title">{esc(title)}</span><span class="term-state" data-done="{done}">{done}</span></figcaption>\n      '
                f'<div class="term-body"><p class="term-task"><span>task</span>{esc(SCENE_TASK)}</p>\n      {pack_line}<ol class="turns">{"".join(rows)}\n      </ol></div>\n      '
                f'<div class="meter"><div class="meter-track" aria-hidden="true">{segs}</div>'
                f'<p class="meter-read"><span class="meter-label">Context read</span><b class="tok" data-to="{pt["tokens"]:.0f}">{esc(tokens(pt["tokens"]))}</b>'
                f'<span class="meter-unit">tokens</span><span class="meter-turns">{n} turn{"" if n == 1 else "s"}</span>{named}</p></div>\n    </figure>')

    saved = c["tokens"] - b["tokens"]
    versus = public.get("versus")
    across = (f" Across {number_word(len(versus['cells']))} cells: {saving_range(versus)}." if versus and len(versus["cells"]) > 1 else "")
    delta = (f'<p class="duel-delta"><b>{esc(tokens(saved))} fewer tokens</b><span class="pct">{saved / c["tokens"]:.0%}</span>'
             f'<span class="why">in this one cell, the light class with the original questions.{esc(across)}</span></p>' if saved > 0 else "")
    spoken = (f"With no index, an agent read {tokens(c['tokens'])} tokens over {c['turns']:.1f} turns to answer three lookups. With "
              f"{'a pack made by the program and no command to call' if packed else 'pgr'}: {tokens(b['tokens'])} tokens over {b['turns']:.1f} turns.")
    return ('<div class="duel" id="duel" role="group" aria-label="Three lookups, replayed with and without an index">\n    '
            f'<p class="sr-only">{esc(spoken)}</p>' + window("ref", "Agent, no index", c) + window("best", "Agent, with pgr", b) +
            f'\n    <div class="duel-foot">{delta}<div class="duel-ctl" hidden></div></div>'
            f'\n    {pick_caveat(public)}'
            '\n    <p class="duel-key">An illustrative session. The turns and the tokens are the measured means for one agent answering three '
            'lookups; the file names are made up.</p>\n  </div>')


def sections_of(page: str) -> list[tuple[str, str]]:
    """The sections of a page, read from its own headings, so a renamed section cannot leave the site map behind."""
    import html
    return [(m.group(1), html.unescape(re.sub(r"<[^>]+>", "", m.group(2))).strip())
            for m in re.finditer(r'<h2 id="([^"]+)"[^>]*>(.*?)</h2>', page, re.S)]


OS_OF = {"windows": "windows", "macos": "mac", "macos-intel": "mac", "linux": "linux", "linux-arm": "linux"}


def mp4_dimensions(path: pathlib.Path) -> tuple[int, int] | None:
    """The picture size an MP4 file declares in its first video track, or None when it holds no readable header."""
    import struct
    try:
        with path.open("rb") as handle:
            end = path.stat().st_size

            def boxes(start: int, stop: int):
                at = start
                while at + 8 <= stop:
                    handle.seek(at)
                    size, kind = struct.unpack(">I4s", handle.read(8))
                    head = 8
                    if size == 1:
                        size, head = struct.unpack(">Q", handle.read(8))[0], 16
                    elif size == 0:
                        size = stop - at
                    if size < head:
                        return
                    yield kind, at + head, min(at + size, stop)
                    at += size
            for kind, body, stop in boxes(0, end):
                if kind != b"moov":
                    continue
                for inner, start, limit in boxes(body, stop):
                    if inner != b"trak":
                        continue
                    for leaf, begin, _ in boxes(start, limit):
                        if leaf != b"tkhd":
                            continue
                        handle.seek(begin)
                        version = handle.read(1)[0]
                        handle.seek(begin + (88 if version == 1 else 76))
                        width, height = struct.unpack(">II", handle.read(8))
                        if width and height:
                            return width >> 16, height >> 16
    except (OSError, struct.error):
        return None
    return None


RELEASE_FILES = "https://github.com/Charlesganu2004/Global-Ai-Setup-Benchmark/releases/latest/download/"


def film_label(height: int, fps: float) -> str:
    return f"{height}p" + (f"{round(fps)}" if fps >= 50 else "")


def film_sources(video: dict, where: str) -> list[dict]:
    """The files a film can be played from, best first. video.json lists them in "sources", each with src, width, height, fps, type
    and megabytes; a src that starts with "release:" is a file attached to the GitHub release and is linked from there. Without
    that list the 1080p file named by "src" is the only one. A local file must exist under docs/media/, and its picture size is
    read from the file."""
    listed = video.get("sources")
    found = []
    for item in (listed if isinstance(listed, list) and listed else [{"src": video["src"]}]):
        if not isinstance(item, dict) or not item.get("src"):
            raise SystemExit(f"video.json: {where}.sources needs objects with a src")
        name = str(item["src"])
        if name.startswith("release:"):
            file = name[len("release:"):]
            if not re.fullmatch(r"[\w.-]+\.mp4", file):
                raise SystemExit(f"video.json: {where}: a release source must name an .mp4 file, got {name!r}")
            url = RELEASE_FILES + file
            try:
                width, height = int(item["width"]), int(item["height"])
            except (KeyError, TypeError, ValueError):
                raise SystemExit(f"video.json: {where}: the release source {name!r} needs a width and a height")
        else:
            if not name.startswith("media/") or ".." in name or not (DOCS / name).is_file():
                raise SystemExit(f"video.json: {where} names a source that is not under docs/media/: {name!r}")
            size = mp4_dimensions(DOCS / name)
            if size is None:
                try:
                    size = (int(item["width"]), int(item["height"]))
                except (KeyError, TypeError, ValueError):
                    raise SystemExit(f"video.json: {where}: {name} holds no readable picture size and the source gives none")
                print(f"film {name}: no readable header yet (it may still be rendering); the size video.json gives is used")
            url, (width, height) = name, size
        fps = float(item.get("fps") or 0)
        found.append({"src": url, "width": width, "height": height, "fps": fps, "label": film_label(height, fps),
                      "type": str(item.get("type") or 'video/mp4'),
                      "megabytes": (round(float(item["megabytes"]), 1) if item.get("megabytes") is not None else None)})
    return sorted(found, key=lambda item: (-item["height"], -item["fps"]))


def release_film_urls() -> list[str]:
    """The release addresses of the film files video.json names, so that the guard lets exactly those links through."""
    path = BENCH / "data" / "video.json"
    if not path.is_file():
        return []
    public = json.loads(path.read_text(encoding="utf-8")).get("public") or []
    urls = []
    for video in public if isinstance(public, list) else [public]:
        for item in (video.get("sources") or []) if isinstance(video, dict) else []:
            if isinstance(item, dict) and str(item.get("src", "")).startswith("release:"):
                urls.append(RELEASE_FILES + str(item["src"])[len("release:"):])
    return urls


NAV_SLOTS = {"nav", "foot", "toc"}


# --- the guard ------------------------------------------------------------------------------

# Binary media under docs/media/ is not text and is not read as text; its file name is still checked. A caption
# file (.vtt) is text, so it is scanned like a page.
MEDIA_BINARY = ("mp4", "webm", "jpg", "jpeg", "avif", "png", "webp")

# Words that must never appear in anything under docs/, in any case and in any spelling that joins or splits them with
# a space, a hyphen or an underscore: tool names, file names, vendor models, and the words for how the engine works.
FORBIDDEN_WORDS = (
    "codanna", "semble", "graphify", "token-goat", "tokengoat", "hindsight", "model2vec", "potion", "tree-sitter",
    "rustpython", "oxc", "ast-grep", "ripgrep", "lx", "lxr", "caveman", "cavecrew", "CLAUDE.md", "AGENTS.md",
    "session_index", "arms.json", "bench_parse", "build_site", "grep", "haiku", "sonnet", "opus", "claude", "anthropic",
    "openai", "gemini", "mcp", "bm 25", "reciprocal rank", "sqlite", "hnsw", "faiss", "lancedb", "tantivy", "onnx",
    "PreToolUse", "PostToolUse", "SessionStart", "UserPromptSubmit", "settings.json", "settings.local.json",
    "local-index", "graphify-out", "results.json", "hooks.json", "engine.json", "install.json", "index.json",
    # other tools and vendors, and model families
    "codex", "copilot", "windsurf", "aider", "cline", "llama", "qwen", "mistral", "deepseek", "grok", "chatgpt", "gpt",
    "gemma", "mixtral", "phi", "kimi", "nova", "cohere", "command r", "glm", "falcon", "jamba", "granite", "olmo", "starcoder", "codestral",
    "pixtral", "bard", "palm", "yi", "ernie", "hunyuan", "minimax", "moonshot",
    # how the engine works, in words
    "vector index", "vector search", "vector store", "vector database", "nearest neighbour", "nearest neighbor",
    "graph traversal", "call graph", "rrf", "tokenizer", "inverted index", "cosine", "rust", "cargo", "ctags", "crate")
# Shapes that must never appear: command lines and flags, the setup's paths, vendor model versions, tier labels,
# and any request to another host. All are matched in any case.
TEXT_KINDS = ("html", "js", "json", "vtt", "txt", "svg", "xml", "md")   # files a flag can hide in; the stylesheets are not
FORBIDDEN_PATTERNS = (
    # A flag follows a space or starts a line. A CSS custom property sits inside quotes or var( ), and the
    # stylesheets are not checked for flags at all.
    ("a command-line flag", r"(?<![\w'\"(-])--[a-z][a-z0-9-]+", TEXT_KINDS),
    ("a flag with a value", r"--[a-z][a-z0-9-]*=", TEXT_KINDS),
    ("a shell prompt", r"(?m)^\s*[$>]\s+\w", None),
    ("a package command", r"\b(?:pip3?|pipx|npm|npx|cargo|brew|winget|choco|curl|wget|sudo)\s+[-\w]", None),
    ("an interpreter command", r"\bpython3?\s+-", None),
    ("a git command", r"\bgit\s+(?:clone|checkout|pull|push|commit|add|init)\b", None),
    ("a path of the setup", r"\.claude\b|\.codex\b|\.codanna\b|(?<![\w.])(?:setup|benchmark|harness|rust)[/\\]", None),
    ("a model version", r"\bgpt-?\d\w*|\bo[1-9](?:-(?:mini|pro|preview))?\b", None),
    # A parameter count written beside a model: 27B, 8x22B.
    ("a model size", r"(?-i:\b(?:\d+x)?\d{1,3}B\b)", ("html", "vtt", "txt", "md", "json")),
    # The three classes of model are named by the model, never by a label.
    ("a model class label", r"\b(?:light|mid|middle|top)[ -](?:model[ -])?(?:class(?:es)?|tier)\b|\bmodel classes\b|\bclasses:\s*light\b|\b(?:light|mid) (?:workers?|agents?)\b|\b(?:light|mid|top), (?:original|held-out) questions\b", None),
    ("a tier label", r"\btier[\s_-]*[abc]\b", None),
    ("a request to another host", r"https?://(?!www\.w3\.org/2000/svg)|(?<![:\w])//[\w-]+\.[a-z]{2,}/|@import", None),
    # The product's own commands are masked before the scan (see approved_strings), so what is left of "pgr" and a word is
    # a command the pages may not show. The prose never writes the command name in lower case followed by a word.
    ("an unlisted pgr command", r"\b(?-i:pgr)\s+[a-z-]+", None),
    # The short film is a trailer. The word ad is not its name, in a label, a class, a file name or a link.
    ("Python as the language of the engine", r"\b(?:engine|program|binary|pgr|tool)\s+(?:is |was |are )?(?:written|built|made|coded|implemented)\s+(?:in|with)\s+python|\bpython\s+(?:engine|program|binary|implementation)\b|\b(?:pgr|peterganurusty|the engine|the program|the tool)\s+(?:is|was|are)\s+(?:a |an |the )?python\b", None),
    ("a film called an ad", r"(?:\b(?:an?|the) ad\b(?! hoc)|\bad[ ,.]+\d|>\s*ad\s*<|pgr-ad[-.]|[\"'=]ad[\"']|\bad (?:film|video)|\bad\s*\(\s*\d|\d[- ]sec(?:ond)?s?[- ]ad\b)", None),
    ("the word ad", r"(?<![\w'./#-])ad(?![\w'-])(?! hoc)", ("html", "vtt", "txt", "md", "json")),
)
# Arm ids that are also ordinary names in markup, styles or scripts. They are not looked for inside identifiers.
AMBIGUOUS_IDS = {"h1", "h2", "h3", "h4", "h5", "h6", "hb", "sb"}
# A word that is also an ordinary word in one kind of file: "rust" is a colour in a style sheet.
WORD_EXEMPT = {"rust": ("css",)}


def applies(kinds, kind: str) -> bool:
    """Whether a check with this `kinds` setting looks at a file of this kind: everywhere (None), only in the listed kinds
    (a tuple), or everywhere except the listed kinds ("not:css")."""
    if kinds is None:
        return True
    if isinstance(kinds, tuple):
        return kind in kinds
    return not (isinstance(kinds, str) and kinds.startswith("not:") and kind in kinds[4:].split(","))


def word_regex(word: str) -> "re.Pattern":
    """A word, found whole and in any case, whether its parts are joined, or split by spaces, hyphens, underscores or a line break."""
    parts = [re.escape(part) for part in re.split(r"[\s_-]+", word) if part]
    return re.compile(r"(?<![A-Za-z0-9])" + r"[\s_-]*".join(parts) + r"(?![A-Za-z0-9])", re.I)


def command_phrases(arms: dict) -> list[str]:
    """The first two words of each command in arms.json, "pgr about" for example, so a bare subcommand cannot slip through."""
    phrases = set()
    for command in arms["commands"].values():
        found = re.match(r"([A-Za-z][\w.-]*)\s+([a-z]{2,})\b", command.strip())
        if found and found.group(1).lower() not in ("grep", "glob", "read", "python", "python3"):
            phrases.add(f"{found.group(1)} {found.group(2)}")
    return sorted(phrases)


def forbidden_checks(arms: dict) -> list[tuple[str, "re.Pattern", tuple | str | None, str | None]]:
    """Every check the guard makes, compiled: words and phrases, shapes, and what arms.json says about the methods.
    Each is (label, pattern, kinds, needle): the needle is a lower-case string that must be in the text for the pattern
    to be worth running, which keeps the scan fast; for text copied from arms.json it is the whole string."""
    first = lambda text: re.split(r"[\s_-]+", text)[0].lower()
    checks = [(f"the word {word!r}", word_regex(word), f"not:{','.join(WORD_EXEMPT[word.lower()])}" if word.lower() in WORD_EXEMPT else None,
               first(word)) for word in FORBIDDEN_WORDS]
    checks += [(f"the command {phrase!r}", word_regex(phrase), None, first(phrase)) for phrase in command_phrases(arms)]
    checks += [(label, re.compile(pattern, re.I), kinds, None) for label, pattern, kinds in FORBIDDEN_PATTERNS]
    ids = sorted({arm["id"] for arm in arms["arms"]}, key=len, reverse=True)
    # An id is checked as written: lower case "h1" and "rs" are ordinary names, and the ids that clash with them are
    # matched only in compound identifiers (see identifier_problems).
    checks.append(("an internal method id", re.compile(r"(?<![\w#.\-])(?:" + "|".join(map(re.escape, ids)) + r")(?![\w\-])"), None, None))
    squash = lambda text: re.sub(r"\s+", " ", text).strip()
    strings = {squash(command) for command in arms["commands"].values()}
    strings |= {squash(arm[key]) for arm in arms["arms"] for key in ("detail", "reads") if arm.get(key)}
    for text in sorted(strings, key=len, reverse=True):
        checks.append((f"text copied from arms.json ({text[:40]!r}...)", re.compile(re.escape(text), re.I), "squash", text.lower()))
    uses = {key for arm in arms["arms"] for key in arm.get("uses", [])}
    checks += [(f"the command key {key!r}", re.compile(r"(?<![A-Za-z0-9])" + re.escape(key) + r"(?![A-Za-z0-9])"), None, key.lower())
               for key in sorted(uses)]
    return checks


PROMPT_RUN = 7          # words in a row of the benchmark's prompts that no page may repeat
_PROMPT_RUNS: frozenset | None = None


def words_of(text: str) -> list[str]:
    return re.findall(r"[a-z0-9']+", text.lower())


def runs_of(words: list[str]) -> set:
    return {tuple(words[i:i + PROMPT_RUN]) for i in range(len(words) - PROMPT_RUN + 1)}


def prompt_runs() -> frozenset:
    """Every run of PROMPT_RUN words in the prompts the agents were given (benchmark/data/codex.json and the workflow script),
    less the runs of the question texts and the setups, which the pages show on purpose."""
    global _PROMPT_RUNS
    if _PROMPT_RUNS is None:
        found: set = set()
        codex = BENCH / "data" / "codex.json"
        if codex.is_file():
            for prompt in (json.loads(codex.read_text(encoding="utf-8")).get("prompts") or {}).values():
                found |= runs_of(words_of(str(prompt)))
        flow = HERE / "token-benchmark.workflow.js"
        if flow.is_file():
            flow_text = flow.read_text(encoding="utf-8")
            first = flow_text.find("This is one run of a benchmark")
            last = flow_text.find("return lines.join", first)
            if first < 0 or last < 0:
                raise SystemExit("token-benchmark.workflow.js: the prompt text was not found, so the guard cannot check the pages against it")
            found |= runs_of(words_of(flow_text[first:last]))
        shown = [q["text"] for q in PUBLIC_QUESTIONS] + [q["title"] for q in PUBLIC_QUESTIONS] + list(PUBLIC_SETUPS)
        for text in shown:
            found -= runs_of(words_of(text))
        _PROMPT_RUNS = frozenset(found)
    return _PROMPT_RUNS


def prompt_run_in(text: str) -> str | None:
    """The first run of the benchmark's prompts that the text repeats, or None."""
    words = words_of(text)
    runs = prompt_runs()
    return next((" ".join(run) for run in zip(*[words[i:] for i in range(PROMPT_RUN)]) if run in runs), None)


def views_of(text: str) -> list[tuple[str, str]]:
    """The text as written, and then the text as a reader or a browser would take it: character references, \\u and \\x
    escapes and percent escapes decoded, quoted strings joined where a script adds them, and every long base64 run
    that decodes to text. A word hidden by any of these is found in one of the views."""
    import base64
    import html
    import urllib.parse
    views = [("as written", text)]
    decoded = html.unescape(text)
    decoded = re.sub(r"\\u([0-9a-fA-F]{4})", lambda m: chr(int(m.group(1), 16)), decoded)
    decoded = re.sub(r"\\x([0-9a-fA-F]{2})", lambda m: chr(int(m.group(1), 16)), decoded)
    decoded = urllib.parse.unquote(decoded)
    decoded = re.sub(r"([\"'`])\s*\+\s*\1", "", decoded)
    if decoded != text:
        views.append(("after decoding", decoded))
    for run in sorted(set(re.findall(r"[A-Za-z0-9+/]{24,}={0,2}", decoded)), key=len, reverse=True)[:200]:
        try:
            raw = base64.b64decode(run + "=" * (-len(run) % 4), validate=True).decode("utf-8")
        except (ValueError, UnicodeDecodeError):
            continue
        if raw and sum(ch.isprintable() or ch.isspace() for ch in raw) >= 0.95 * len(raw):
            views.append(("base64 text", raw))
    return views


def identifier_problems(text: str, kind: str, arms: dict) -> list[str]:
    """The class names, ids, custom properties and identifiers of a script, a style sheet or a page are split into their
    parts (camel case, hyphens, underscores, digits) and checked for the tool names above and for internal method ids,
    alone or joined: graphifyHelper, lx-box, token_goat_cache, .rgs-card."""
    if kind not in ("js", "css", "html"):
        return []
    if kind == "html":
        names = re.findall(r"""\b(?:class|id|for|name|data-[\w-]+)=["']([^"']*)["']""", text) + re.findall(r"\bdata-[\w-]+", text)
        tokens_ = [token for chunk in names for token in re.findall(r"[A-Za-z_][\w-]*", chunk)]
    else:
        tokens_ = re.findall(r"[A-Za-z_][\w-]*", text)
    words = {re.sub(r"[\s_-]+", "", word.lower()) for word in FORBIDDEN_WORDS if kind not in WORD_EXEMPT.get(word.lower(), ())}
    ids = {arm["id"].lower() for arm in arms["arms"] if len(arm["id"]) >= 3 and arm["id"].lower() not in AMBIGUOUS_IDS}
    found = []
    for token in sorted(set(tokens_)):
        if len(token) < 2:
            continue
        parts = [part.lower() for part in re.findall(r"[A-Z]+(?![a-z])|[A-Z]?[a-z]+|[0-9]+", token)]
        joined = {"".join(parts[i:j]) for i in range(len(parts)) for j in range(i + 1, len(parts) + 1)}
        hit = next((word for word in sorted(words) if word in joined and (len(word) > 3 or len(parts) > 1)), None)
        if hit is None and len(parts) > 1:
            hit = next((i for i in sorted(ids) if i in joined), None)
        if hit is None and token.lower() in ids:
            hit = token.lower()
        if hit is not None:
            found.append(f"identifier {token!r} holds {hit!r}")
    return found


def public_names(arms: dict) -> tuple[set, set]:
    """The configuration names and slugs the public data may carry: those of the "public" objects in arms.json, the
    names of the families, and the name of the product."""
    objects = [public_arm(arm) for arm in arms["arms"] if public_arm(arm)]
    return ({o["name"] for o in objects} | set(PUBLIC_FAMILIES.values()) | {BRAND, "PGR", "No index"}, {o["slug"] for o in objects})


def name_problems(root: pathlib.Path, arms: dict) -> list[str]:
    """Every configuration name and slug in the public data and in the pages must come from a "public" object in
    arms.json. A name or a slug typed into the data or into a page, or left over from an earlier build, is refused."""
    import html
    names, slugs = public_names(arms)
    pairs = {(public_arm(arm)["slug"], public_arm(arm)["name"]) for arm in arms["arms"] if public_arm(arm)}
    problems = []
    data = root / "data" / "showcase.js"
    if data.is_file():
        raw = data.read_text(encoding="utf-8")
        start = raw.find("window.SHOWCASE = ")
        try:
            shown = json.loads(raw[start + len("window.SHOWCASE = "):].strip().rstrip(";")) if start >= 0 else {}
        except ValueError:
            shown = {}
            problems.append("  docs/data/showcase.js: the data does not parse, so its names cannot be checked")
        for config in shown.get("configs", []):
            if (config.get("slug"), config.get("name")) not in pairs:
                problems.append(f"  docs/data/showcase.js: configuration {config.get('slug')!r} named {config.get('name')!r} is not a public object of arms.json")
        for family in shown.get("families", []):
            if family.get("name") not in names:
                problems.append(f"  docs/data/showcase.js: family named {family.get('name')!r} is not a public name")
        for row in shown.get("engine", []):
            if row.get("name") not in names:
                problems.append(f"  docs/data/showcase.js: engine row named {row.get('name')!r} is not a public name")
    shown_in_page = re.compile(
        r'class="(?:cfg-name|best-name|race-name|lane-name|bt-name|st-best)"[^>]*>(.*?)</|<li><b>\d+</b><span>(.*?)</span></li>', re.S)
    for page in sorted(root.glob("*.html")):
        for match in shown_in_page.finditer(page.read_text(encoding="utf-8")):
            name = html.unescape(re.sub(r"<[^>]+>", "", match.group(1) or match.group(2) or "")).strip()
            if name and name not in names:
                problems.append(f"  docs/{page.name}: the configuration name {name!r} is not a public name of arms.json")
    slug_attr = re.compile(r'\b(?:data-slug|data-config)="([^"]*)"')
    for page in sorted(root.glob("*.html")):
        for match in slug_attr.finditer(page.read_text(encoding="utf-8")):
            if match.group(1) not in slugs:
                problems.append(f"  docs/{page.name}: the slug {match.group(1)!r} is not a public slug of arms.json")
    return problems


def guard_public(arms: dict, install: dict | None = None, root: pathlib.Path | None = None, only: set | None = None,
                 checks: list | None = None) -> int:
    """Scan every file under the public folder, its file names included, for anything that must never be
    published: words in any case and any spelling, command lines and flags, identifiers and class names, text copied
    from arms.json, and configuration names that do not come from arms.json. The text is read as written and as
    decoded, and a word is looked for across line breaks. Exits with the file, the place and the offending text. The
    text let through is what approved_strings names: the product's own commands, the install commands and release links of
    install.json, and the names of the agent programs the benchmark ran under. Returns
    the number of files scanned. `only` limits the scan to those files (relative paths); the self-test uses it."""
    import html
    root = root or DOCS
    checks = checks or forbidden_checks(arms)
    approved = approved_strings(install)
    links = approved_links(install)
    problems, scanned = [], 0
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        shown = path.relative_to(root).as_posix()
        if only is not None and shown not in only:
            continue
        scanned += 1
        kind = path.suffix.lstrip(".").lower()
        try:
            text = "" if kind in MEDIA_BINARY and shown.startswith("media/") else path.read_bytes().decode("utf-8")
        except UnicodeDecodeError:
            text = ""
        unmasked = text
        for token in approved:
            text = text.replace(token, " ")
        for link in links:
            text = link.sub(" ", text)
        # Comment markers are not flags: the two dashes of "<!--" and "-->" are blanked before the check.
        flat = text.replace("<!--", "    ").replace("-->", "   ")
        seen = set()
        for view, body in [("file name", shown)] + [(label, content) for label, content in views_of(flat)]:
            lowered = body.lower()
            for label, pattern, kinds, needle in checks:
                if kinds == "squash" or not applies(kinds, kind) or (needle and needle not in lowered):
                    continue
                hit = pattern.search(body)
                if hit:
                    place = view if view == "file name" else f"line {body.count(chr(10), 0, hit.start()) + 1} ({view})"
                    key = (label, hit.group(0).lower())
                    if key not in seen:
                        seen.add(key)
                        problems.append(f"  docs/{shown}, {place}: {label}: found {hit.group(0)!r}")
        for found in identifier_problems(flat, kind, arms):
            problems.append(f"  docs/{shown}: {found}")
        # Copied text may run over several lines: the long strings from arms.json are looked for in the whole file.
        whole = re.sub(r"\s+", " ", unmasked.replace("<!--", "    ").replace("-->", "   ")).lower()
        for label, pattern, kinds, needle in checks:
            if kinds == "squash" and needle in whole:
                problems.append(f"  docs/{shown}, whole file: {label}")
        repeated = prompt_run_in(whole) if text else None
        if repeated:
            problems.append(f"  docs/{shown}, whole file: a run of the benchmark's prompt: {repeated!r}")
    problems += name_problems(root, arms)
    if problems:
        shown_problems = problems[:30] + ([f"  and {len(problems) - 30} more"] if len(problems) > 30 else [])
        raise SystemExit("GUARD FAILED: the public folder holds something that must not be published.\n"
                         + "\n".join(shown_problems) + "\nNothing is safe to publish until this is fixed.")
    if only is None:
        unreferenced = unreferenced_media(root)
        if unreferenced:
            total = sum((root / name).stat().st_size for name in unreferenced) / 1e6
            print(f"warning: {len(unreferenced)} files under docs/media/ are not used by any page ({total:.0f} MB) and would still be "
                  "published: " + ", ".join(unreferenced))
    return scanned


def unreferenced_media(root: pathlib.Path) -> list[str]:
    """Files under media/ that no page, script or data file names. They are published all the same."""
    media = root / "media"
    if not media.is_dir():
        return []
    text = "".join(path.read_text(encoding="utf-8", errors="ignore") for path in root.rglob("*")
                   if path.is_file() and path.suffix.lstrip(".").lower() in ("html", "js", "css", "json", "vtt"))
    return sorted(f"media/{path.name}" for path in media.iterdir() if path.is_file() and f"media/{path.name}" not in text)


def film_problems(public: dict, captions: dict) -> list[str]:
    """What the films say against what the Overview says. The page names its highlighted configuration with its tokens and its saving
    against no index, and the saving across the cells measured. The trailer says the first two in its captions and the explainer says
    the third. A caption file without the figure belongs to an older headline: the film shows a result the page no longer gives. The
    guard cannot tell, because it reads words and not numbers, so this check does. `captions` maps a file name to its text; one line
    comes back for each figure that is missing."""
    found = headline_of(public)
    if not found or not found["c"]:
        return []
    b, c, versus = found["b"], found["c"], public.get("versus")
    need = {"trailer": [(tokens(b["tokens"]), "the headline tokens"), (f"{1 - b['tokens'] / c['tokens']:.0%}", "the headline saving")],
            "explainer": [(saving_range(versus), "the range of the saving across cells")] if versus and len(versus["cells"]) > 1 else []}
    out = []
    for name, text in sorted(captions.items()):
        for film, figures in need.items():
            if film in name:
                out += [f"{name} does not say {figure!r}, {what} on the Overview" for figure, what in figures if figure not in text]
    return out


def guard_selftest(arms: dict, install: dict | None = None) -> int:
    """Plant one violation for each rule of the guard in a scratch copy of docs/ and check that the guard refuses it
    with the rule's own message. The copy is removed afterwards; docs/ itself is never touched. The text the guard lets
    through (the product's own commands, the install commands, the release links, the names of the two agent programs) is
    planted too, to check that exactly that text passes and a near miss does not. Returns the number of plants that were not
    caught (0 when every rule works)."""
    import shutil
    import tempfile
    names, slugs = public_names(arms)
    some_id = next(arm["id"] for arm in arms["arms"] if len(arm["id"]) >= 3 and arm["id"].lower() not in AMBIGUOUS_IDS)
    base = install
    elsewhere = "https://downloads.example.test"
    PROMPT_PLANTS = ("Be exact and do not guess. Stop as soon as the work is answered, then return the answer.",
                     "This is one run of a benchmark that measures how many tokens a code lookup costs.")
    plants = [
        # (rule, file to write or edit, text or edit, text the refusal must contain)
        ("case-insensitive pattern", "plant.txt", "Run it on Tier A for a day.", "a tier label"),
        ("case-insensitive word", "plant.txt", "built with CODANNA and TOKEN-GOAT", "the word 'codanna'"),
        ("a vendor model", "plant.txt", "tested against GPT-5 and Codex", "a model version"),
        ("a word across lines", "plant.html", "<p>an agent with token-\ngoat installed</p>", "the word 'token-goat'"),
        ("a caption file", "media/plant.vtt", "WEBVTT\n\n1\n00:00:00.000 --> 00:00:01.000\nPowered by graphify.\n", "the word 'graphify'"),
        ("a flag in a caption file", "media/plant.vtt", "WEBVTT\n\n1\n00:00:00.000 --> 00:00:01.000\nRun pgr affected X --depth 3\n", "a command-line flag"),
        ("a data file", "data/plant.json", '{"note": "uses Semble for search"}', "the word 'semble'"),
        ("a pgr command that is not listed", "plant.txt", "then run pgr snapshot on the shared notes", "an unlisted pgr command"),
        ("a pgr command with a flag", "plant.html", "<code>pgr --bank team-notes recall</code>", "a command-line flag"),
        ("a character reference", "plant.html", "<p>&#103;raphify</p>", "after decoding"),
        ("an escape in a script", "assets/plant.js", "var s = '\\u0067raphify';", "after decoding"),
        ("strings joined by a script", "assets/plant.js", "var s = 'gra' + 'phify';", "after decoding"),
        ("text hidden in base64", "plant.txt", "payload Z3JhcGhpZnkgYW5kIGNvZGFubmE= end", "base64 text"),
        ("text copied from arms.json over several lines", "plant.txt", "\n".join(
            re.sub(r"\s+", " ", arms["commands"]["pgr_about"]).strip().split(" ")), "text copied from arms.json"),
        ("an identifier in a script", "assets/plant.js", "var graphifyHelper = 1;", "identifier 'graphifyHelper'"),
        ("a class name in a style sheet", "assets/plant.css", ".lx-box { margin: 0; }", "identifier 'lx-box'"),
        ("an internal id in a class name", "plant.html", f'<div class="card {some_id.lower()}-card">x</div>', f"holds {some_id.lower()!r}"),
        ("a link to another host", "plant.html", f'<a href="{elsewhere}/request">go</a>', "a request to another host"),
        ("the install command with another host", "plant.html", f"<code>irm {elsewhere}/install.ps1 | iex</code>", "a request to another host"),
        ("a pipe to a shell from another host", "plant.html", f"<pre>curl -fsSL {elsewhere}/install.sh | sh</pre>", "a package command"),
        ("a package install from another host", "plant.txt", f"pip install {elsewhere}/pgr.whl", "a package command"),
        ("another repository", "plant.html", '<a href="https://github.com/someone-else/tools/releases/latest">get it</a>', "a request to another host"),
        ("an agent program outside its phrase", "plant.txt", "Tested with Codex and with Claude", "the word 'codex'"),
        ("the protocol name in short", "plant.txt", "an mcp server for the agent", "the word 'mcp'"),
        ("a path of the setup", "plant.txt", "the files are in the harness/ folder", "a path of the setup"),
        ("a model of another version", "plant.txt", "Measured with Claude Haiku 4.5 at low effort.", "the word 'haiku'"),
        ("a model of another vendor", "plant.txt", "Also tried with Gemini and GPT-5.", "the word 'gemini'"),
        ("a model name split into parts", "plant.html", "<p>Claude <b>Opus</b> 5.5</p>", "the word 'opus'"),
        ("a model class written as a label", "plant.txt", "Run it on Tier C for a day.", "a tier label"),
        ("the engine in another language", "plant.txt", "The engine is written in Python.", "Python as the language of the engine"),
        ("Rust mentioned beside a build tool", "plant.txt", "Built with cargo from a Rust crate.", "the word 'cargo'"),
        ("a film called an ad", "plant.html", '<span class="cine-meta">Ad, 22 seconds, with captions</span>', "a film called an ad"),
        ("a film file named for an ad", "plant.html", '<source src="media/pgr-ad-1080p.mp4">', "a film called an ad"),
        ("a model family that is not in models.json", "plant.txt", "Also run with Gemma 3 27B.", "the word 'gemma'"),
        ("a mixture-of-experts model", "plant.txt", "and Mixtral 8x22B", "the word 'mixtral'"),
        ("small models", "plant.txt", "Phi-4 and Kimi K2 did worse.", "the word 'phi'"),
        ("a small vendor model", "plant.txt", "o5-mini and Nova Pro were tried.", "a model version"),
        ("a model size", "plant.html", "<p>A 70B model was tried.</p>", "a model size"),
        ("a model class as a label", "plant.html", "<p>In 2 of 3 light-class runs and 0 of 3 mid-class runs.</p>", "a model class label"),
        ("a model class in a caption", "media/plant.vtt", "WEBVTT\n\n1\n00:00:00.000 --> 00:00:01.000\nThree model classes: light, mid and top.\n", "a model class label"),
        ("a class id in a list of cells", "plant.html", "<p>light, original questions, 54%; mid, held-out questions, 28%</p>", "a model class label"),
        ("workers named by class", "plant.txt", "With a reviewer on top of three light workers.", "a model class label"),
        ("a film called an ad, in words", "plant.txt", "Watch the 30-second ad.", "a film called an ad"),
        ("a film called an ad, as a button", "plant.html", "<button>Ad (30 s)</button>", "a film called an ad"),
        ("the word ad on its own", "plant.html", "<p>The ad has captions.</p>", "a film called an ad"),
        ("pgr as a python tool", "plant.txt", "PGR is a Python tool.", "Python as the language of the engine"),
        ("a prompt sentence of the codex runs", "plant.txt", PROMPT_PLANTS[0], "a run of the benchmark's prompt"),
        ("a prompt sentence of the workflow", "plant.txt", PROMPT_PLANTS[1], "a run of the benchmark's prompt"),
        ("an internal id of the team round", "plant.html", '<div class="card tphv-card">x</div>', "holds 'tphv'"),
        ("a configuration name not in arms.json", "plant.html", '<th class="cfg"><span class="cfg-name">PGR build 9</span></th>', "is not a public name"),
        ("a configuration slug not in arms.json", "data/showcase.js", "SLUG", "is not a public object of arms.json"),
    ]
    missed = 0
    checks = forbidden_checks(arms)
    with tempfile.TemporaryDirectory() as scratch:
        target = pathlib.Path(scratch) / "docs"
        shutil.copytree(DOCS, target, ignore=lambda folder, files: [f for f in files if f.endswith((".mp4", ".webm"))])
        try:
            guard_public(arms, base, target, checks=checks)
            print("  ok       the unchanged copy of docs/ passes")
        except SystemExit as err:
            lines = str(err).splitlines()
            print("  MISSED   the unchanged copy of docs/ fails:", lines[1] if len(lines) > 1 else err)
            missed += 1
        data = target / "data" / "showcase.js"
        original = data.read_text(encoding="utf-8")
        for rule, name, body, expect in plants:
            planted = target / name
            if body == "SLUG":
                shown = json.loads(original.split("window.SHOWCASE = ", 1)[1].strip().rstrip(";"))
                shown["configs"].append(dict(shown["configs"][0], slug="invented-build", name="Invented build"))
                data.write_text("window.SHOWCASE = " + json.dumps(shown) + ";\n", encoding="utf-8")
            else:
                planted.parent.mkdir(parents=True, exist_ok=True)
                planted.write_text(body, encoding="utf-8")
            try:
                guard_public(arms, base, target, only={name}, checks=checks)
                print(f"  MISSED   {rule}: the guard passed")
                missed += 1
            except SystemExit as err:
                caught = expect.lower() in str(err).lower() and name.replace("\\", "/") in str(err).replace("\\", "/")
                print(f"  {'caught ' if caught else 'WRONG  '}  {rule}: {expect!r} in {name}")
                missed += 0 if caught else 1
            finally:
                data.write_text(original, encoding="utf-8")
                if body != "SLUG":
                    planted.unlink()
        # What the owner approved, exactly as approved: the commands, the release links and the two program names pass.
        wheel = base["wheels"][0]
        allowed = ([item["command"] for item in base["installers"]] + [item["command"] for item in base["first_commands"]]
                   + [f"{tool} {base['versioned_base']}/{wheel['name']}" for tool in PYTHON_TOOLS] + [f"pgr {name}" for name in PGR_COMMANDS])
        names = [item["name"] for item in MODELS["tiers"].values()] + ([str(MODELS["codex"]["model_name"])] if MODELS["codex"] else [])
        text = "\n".join(f"<code>{esc(line)}</code>" for line in allowed) + (
            "\n<p>" + " ".join(f"{name} at low effort." for name in names) + " The engine is written in Rust. The repository is a Python project. "
            "Trailer, 30 seconds, with captions. " + models_line() + " " + str((footprint_of() or {}).get("language", "")) + "</p>"
            f'\n<p>{harness_line()} Measured under Claude Code and under the Codex CLI. A Model Context Protocol server.</p>'
            f'\n<a href="{base["release_page"]}">release</a> <a href="{base["repository"]}">repository</a>')
        planted = target / "plant.html"
        planted.write_text(text, encoding="utf-8")
        try:
            guard_public(arms, base, target, only={"plant.html"}, checks=checks)
            print("  ok       the approved commands, links and names pass")
        except SystemExit as err:
            print("  MISSED   the approved text was refused:", str(err).splitlines()[1] if len(str(err).splitlines()) > 1 else err)
            missed += 1
        finally:
            planted.unlink()
    # Every team arm that is not a held-out twin and is not left out on purpose has a public name, or its results could not be shown.
    unnamed = [arm["id"] for arm in arms["arms"] if arm.get("group") == "team" and not arm.get("base")
               and arm["id"] not in TEAM_UNPUBLISHED and not public_arm(arm)]
    if unnamed:
        print(f"  MISSED   team arms without a public name: {unnamed}")
        missed += 1
    else:
        print("  ok       every team arm has a public name or is left out on purpose")
    # The films' captions are held to the page's headline: a caption file from an older headline is reported, a current one is not.
    shown = {"configs": [{"best": True, "family": "ours", "slug": "x", "points": [{"class": "light", "set": "original", "tokens": 70325.0}]},
                         {"best": False, "family": "control", "slug": "c", "points": [{"class": "light", "set": "original", "tokens": 293560.0}]}],
             "versus": {"cells": [{"saving": 0.72}, {"saving": 0.80}], "low": 0.72, "high": 0.80}}
    stale = film_problems(shown, {"pgr-trailer.en.vtt": "134k tokens.\n54% fewer", "pgr-explainer.en.vtt": "28% to 54% fewer"})
    current = film_problems(shown, {"pgr-trailer.en.vtt": "70k tokens.\n76% fewer", "pgr-explainer.en.vtt": "72% to 80% fewer"})
    if len(stale) == 3 and not current:
        print("  ok       a caption file from an older headline is reported, a current one is not")
    else:
        print(f"  MISSED   the film check: stale gave {stale}, current gave {current}")
        missed += 1
    print(f"guard self-test: {len(plants) + 4 - missed} of {len(plants) + 4} checks as expected")
    return missed


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
           "a held-out set B: a second set of three questions of the same three shapes, written later by the same team and not "
           "seen by the people and agents who built the engine (it is not independent). One agent answering all three questions; "
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
            "- The control's file reads pass through token-goat's hooks on the test machine. The secret-redaction hook overwrote the two "
            "lines that call the first question's target (its name contains \"api_key\") with a placeholder, so the control left that "
            "caller out in four of its six original-question runs. The hook matches the file-reading tools only; the product "
            "configurations ran their lookups through the shell tool and were not touched. The earlier session (A0) ran without the hooks. "
            "The public site therefore does not set the control's fully correct counts or mean score against the product's; it compares "
            "tokens and turns only, and fully correct counts only with the first generation. See Known weaknesses of the study."]
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


def saving_section(public: dict) -> list[str]:
    """The saving against no index, cell by cell, and the first generation beside the highlighted configuration, with the
    lowest and the highest figure. These are the numbers the public site quotes as a range; they are computed from the
    same configurations. The private report may say what the site only hints at."""
    versus = public.get("versus")
    configs = public["configs"]
    best = next((c for c in configs if c["best"]), None)
    first = first_generation(configs)
    if not versus or not best:
        return []
    out = ["", "## Saving against no index, cell by cell", "",
           f"The highlighted configuration (\"{BEST_LABEL}\" on the site) is {best['name']}. Tokens and turns of it and of the "
           "no-index control in each of the four cells where both were measured. The site quotes the lowest and the highest saving "
           "and labels any single cell it shows. It does not set the control's fully correct counts against the product's: in "
           "these runs the control's reads were altered by token-goat's secret-redaction hook (see Known weaknesses of the study).", "",
           "| Class | Questions | Tokens, highlighted | Tokens, no index | Saving | Turns, highlighted | Turns, no index |",
           "|---|---|---:|---:|---:|---:|---:|"]
    for cell in versus["cells"]:
        out.append(f"| {cell['class']} | {cell['set']} | {cell['tokens']:,.0f} | {cell['control_tokens']:,.0f} | {cell['saving']:.1%} | "
                   f"{cell['turns']:.2f} | {cell['control_turns']:.2f} |")
    out += ["", f"Range of the saving in tokens: {versus['low']:.1%} to {versus['high']:.1%} ({saving_range(versus)} on the site).", ""]
    if first and first is not best:
        out += [f"Against the first generation ({first['name']}), the only comparison of fully correct counts the site makes, "
                "because it ran in the same session under the same conditions:", "",
                "| Class | Questions | Tokens, highlighted | Tokens, first generation | Turns, highlighted | Turns, first generation | "
                "Fully correct, highlighted | Fully correct, first generation |",
                "|---|---|---:|---:|---:|---:|---:|---:|"]
        for cid, sid in BEST_CELLS:
            b, f = cell_of(best, cid, sid), cell_of(first, cid, sid)
            if b and f:
                out.append(f"| {cid} | {sid} | {b['tokens']:,.0f} | {f['tokens']:,.0f} | {b['turns']:.2f} | {f['turns']:.2f} | "
                           f"{of(b['perfect_runs'], b['runs'])} | {of(f['perfect_runs'], f['runs'])} |")
        out.append("")
    return out


def known_weaknesses(public: dict) -> list[str]:
    """The method review's list of weaknesses of the study, in plain words, each with what it could change and the
    cheapest fix. The counts are the reviewer's own, taken from the transcripts and the harness; this build does not
    recompute them, except where a sentence says so. The private report may name everything."""
    study = public["study"]
    behind = study["behind"]
    items = [
        ("The control's accuracy deficit comes from another tool, not from having no index",
         "Token-goat's secret-redaction hook overwrote the two lines where the first question's target is called with a placeholder, "
         "because the target's own name contains \"api_key\". Every one of the six control runs of the second session had the hook act "
         "on its reads, and four of them left that caller out; the held-out control, whose first question has another target, had no "
         "redaction and missed nothing. The hook matches only the file-reading tools, so the product configurations, which ran their "
         "lookups through the shell tool the hook does not see, were not touched. In the first session, without the hook, the control was "
         "fully correct in 10 of 11 runs across the light, mid and top classes on the original questions.",
         "The control's fully correct counts (7 of 11 in the second session, against 15 of 16 for the first-session control and the "
         "held-out control together), any claim that the index improves accuracy, and the control's tokens, which the hook lowered while it damaged the "
         "answers. The public site no longer sets the control's correctness against the product's.",
         "Rerun the control, original and held-out, with the hooks off, in the same session as the product runs and in alternating "
         "order: about six light-class runs. Or choose a first-question target whose name does not contain \"api_key\"."),
        ("The held-out set is not an independent test",
         "It is a second draw from the same generator by the same people: the same filter, the same three shapes of question, unique "
         "function names with four to twelve callers, and job descriptions that follow the wording of the functions' own documentation. "
         "The answer key matches callers by bare name, which is how a name-based index resolves them, so the key and the index agree "
         "by construction. Functions that share a name with another function (about one in eight) are never asked about. The output "
         "formats of the tools were tuned on the first set over five rounds.",
         "How much the held-out result says about questions of other shapes or other repositories. It checks fitting to the first "
         "three questions, no more.",
         "Pre-register a random draw stratified by duplicate-name and caller-count strata, have someone who has not seen the "
         "documentation write the concept questions, add three questions that need a function body to answer, and include targets "
         "whose name is shared."),
        ("The held-out result is worse than the original one, and the headline quoted the best cell",
         "Configurations that took two turns on every original-question run needed more on the held-out questions (the first "
         "generation took three and five turns on the mid class, with two runs). The saving against the same-day control was largest in "
         "the cell the headline showed. The site now gives the range over the four cells and says that two turns is true of the original "
         "questions.",
         "The size of the headline saving, and the claim that two turns is a floor for any question.",
         "Show all four cells, and run the third runs that are missing."),
        ("Three runs per cell cannot support a claim of significance",
         "With three runs against three, the smallest exact two-sided permutation p-value is 0.10, and every product-against-control "
         "token cell of the second session sits at that floor although the ranges do not overlap. A cell holds three repeats of one "
         "bundle of three questions, so the effective sample for generalising is two bundles. Tokens equal about 65k times the turns, so "
         "a cell is three draws of a small whole number. Differences of one turn or one fully correct run are inside the spread, among "
         "them graph alone against graph and search on turns, and the memory comparisons.",
         "Every finding that rests on one turn or one run. The site now says \"in these runs\" and gives the counts.",
         "Ten repeats per cell for the control and the two leading configurations on the light class, both sets (about two dollars at the "
         "table's prices), and about twenty randomly drawn questions so that the question dimension has a sample."),
        ("The highlighted configuration is chosen by a rule written after the results",
         "Only configurations with three runs in each of four cells can be chosen. The control and the first generation miss it only "
         "because a third run of one cell was never started, and the memory configurations have no held-out cell, so the field is four "
         "sibling configurations. Two of them tie on fully correct runs and the token tie-break decides. Other reasonable rules choose "
         "the first generation or the code-graph configuration instead.",
         "Which configuration is highlighted. The site now calls it \"Most reliable\", states the rule and that it was set after the runs, "
         "and says that only configurations measured on both question sets were eligible.",
         "Run the two missing third runs (four agents), publish the rule before the next round, or show every configuration with its "
         "interval."),
        ("The benchmark cannot tell the second generation from the first",
         "On the original questions the two use the same tokens and turns and are fully correct as often. The second generation's "
         "measured advantage is speed and size, not tokens or accuracy.",
         "The ranking narrative. The site says the first generation matched it on tokens and turns and puts the difference under speed.",
         "State the tie, or run both on a set built to need what the second generation added, such as impact analysis."),
        ("The agents were told different amounts",
         "The product configurations received command descriptions of 400 to 500 characters that list the fields the grader checks, show "
         "how to batch questions and say not to verify. The control received one line naming its tools, with no hint to batch or to read "
         "only what it needs. Every prompt also said the task measures what a code lookup costs, and the start-up context carries the "
         "sponsor's own routing rules.",
         "The two-turn floor, partly produced by the prompt, and the control's turn count. The Study page now says the descriptions "
         "differ and were not controlled.",
         "Give the control a usage note of the same length, give both sides the same \"when to read\" sentence, delete the sentence "
         "about measuring tokens, and rerun the control (six runs)."),
        ("The baseline is the weakest available and the questions suit a call graph",
         "The control has file search and read tools and no shell, so no ripgrep, no context lines around a match, no ctags or "
         "ast-grep, no language server. Two of the three questions ask for the function that encloses each call, which a plain search "
         "cannot print, so the control needs an extra dependent turn, and in several rows it names the wrong enclosing function. No "
         "question needs a function body to be read.",
         "How large the saving is against a competent user of ordinary tools.",
         "Add a tuned-search arm and a shell arm with context lines and a syntax-aware search, and add three questions about behaviour "
         "whose answer needs a body read."),
        ("The token headline is against a control the hook had made cheaper",
         "The hooked control of the second session used fewer tokens than the clean control of the first (about 19% fewer on the light "
         "class) while giving worse answers. Against the clean control the saving is larger (about 64% light and 53% mid). Within the "
         "second session the start-up context of the control and the product differ by little, so the hook is the live difference.",
         "The saving, upward. The site compares with the same-day control on tokens and turns and says so; the Study page gives the tokens of "
         "both controls.",
         "Run control and product in one session with the hooks off, in alternating order, and report one control."),
        ("A token count weighs cached re-reads like fresh text",
         "About 87% of all tokens are cache reads, which a bill weighs at a tenth. The size of the saving depends on the cache state: "
         "in dollars it is much larger with a warm cache than with a cold one, and the absolute saving on the light class is a fraction "
         "of a cent. The prices are constants typed into the analysis, and the light-class entry is about a tenth of the list price the "
         "reviewer knew of. They cannot be checked from the repository.",
         "Every money figure, including the orchestration cost multiple. The site now defines a token count, says most of the saving is "
         "turns, and calls the multiple an estimate.",
         "Show warm and cold dollars beside tokens, check the price table against a bill, and describe the saving as about two fewer "
         "round trips."),
        ("Grading is not entirely free of human choices",
         "The key for the which-function question is hand-written, and for the held-out set a person widened it to accept either of two "
         "functions. The parser has false negatives: a parameter written as name and type is read as nothing, a defensible alternative "
         "function scores a partial mark, and one missed caller is enough to make a run not fully correct. None of the recorded answers "
         "hit the parameter case.",
         "Whether a single run counts as fully correct. The site now says the key was read from the syntax tree and reviewed by the "
         "authors, and calls the percentage a mean score.",
         "Parse parameter names with the syntax tree, publish the accepted answers before the runs, and score a defensible "
         "alternative as its own class."),
        ("Memory filled with the answers is a lookup, not a result",
         "Four memory configurations were filled by a session that answered the same questions and were never run on the held-out set. "
         "The only realistic test, memory from an exploring session, was neutral or worse on tokens in most cells and may overlap the "
         "held-out targets, because the exploring notes were not compared with the questions.",
         "Any suggestion that memory helps. The site now says \"filled with the answers\" in those four names and calls them a best case.",
         "Label them as an upper bound, compare the exploring notes with both question sets, and test memory on a task that spans two "
         "sessions."),
        ("The speed table is not like for like",
         "It compares a first generation written in Python with a binary written in Rust, so it measures the runtime and the "
         "architecture, not an algorithm. The 130 second build of the first generation is from an earlier session and uses embeddings "
         "(12 seconds without); the query figure is the whole-process median of one call answering the original-set questions. "
         "Search alone, however fast, was often wrong on the callers question.",
         "The 48-times and the build-time factors. The site now labels the query as one call answering three lookups, shows the single "
         "lookup gap, and names the build.",
         "Measure both engines on the same day, with and without embeddings, 20 runs each with the spread, on a larger repository."),
        ("The competitor rows are not like for like",
         "Three published comparators come from the first session with one to three runs, and are driven through descriptions written "
         "by the sponsor, one of them through an awkward command-line wrapper of a tool-protocol call. "
         f"{study['configurations_published']} of {study['configurations_tested']} configurations are published, chosen by a flag.",
         "How the comparators compare when used as their makers intend. The leaderboard marks these rows as an earlier session and the "
         "Study page no longer says everything was measured on the same day.",
         "Rerun them on the same day with at least five runs and the maker's recommended use, and publish the thirty other rows in this report."),
        ("Which transcripts enter the data is decided by hand",
         "The parser takes a list of folders on the command line, with no manifest and no list of what was left out. Some labelled "
         "agents on disk are not in the data, among them a control run that never produced a structured answer. A few runs with "
         "off-policy tool use are kept. Nothing suggests a run was silently dropped, and nothing proves it.",
         "The control's tally, by one run, and the trust in the data.",
         "Commit a manifest of the included folders, refuse any labelled transcript not in one, report off-policy runs separately, and "
         "count the control run that never answered."),
        ("The orchestration claim rests on estimates and one set of briefs",
         "Only three planning calls were ever measured and the mean is charged to the other orchestrated runs; every orchestrated run "
         "of the original set reuses the same briefs; the reviewer is the top model class with tools, so the comparison mixes "
         "orchestration with model class; and the single-agent comparison pools many configurations, including weak ones.",
         "The cost multiple and the accuracy gap. The site now says the cost is an estimate, pooled, and that the reviewer is a larger "
         "class.",
         "Compare like for like on the same configurations and questions with independent plans for each run."),
    ]
    out = ["", "## Known weaknesses of the study", "",
           "The method review of the study found the weaknesses below. Each has what it could change and the cheapest fix. The counts are "
           "the reviewer's, taken from the transcripts and the harness; this build does not recompute them. "
           f"For scale: {behind['runs']} single-agent runs in {behind['cells']} cells stand behind the public leaderboard, computed by this build.", ""]
    for n, (title, what, changes, fix) in enumerate(items, 1):
        out += [f"### {n}. {title}", "", what, "", f"- What it could change: {changes}", f"- Cheapest fix: {fix}", ""]
    out += ["### Three claims the data does not support", "",
            "1. That the index improves accuracy (12 of 12 against 7 of 11 fully correct). The gap is the redaction artefact; the clean "
            "control was fully correct in 15 of 16 runs, and on the held-out set the control and the leading configuration were both "
            "fully correct in every run.",
            "2. That the highlighted configuration is better than the first generation. Tokens, turns and correct runs on the original "
            "questions are the same within noise.",
            "3. That the held-out result shows the engine was not fitted, or that it generalises beyond this repository and these "
            "shapes of question. The held-out set is a second draw of the same shapes, and it shows more turns. The speed factors are "
            "not like for like and the benefit of memory is unmeasured.", "",
            "### The claims it does support", "",
            "1. On this repository, for caller, definition and find-by-description questions, a one-call lookup lets light and mid agents "
            "finish in about two turns where unaided search and read tools needed three to six. The saving in tokens against the "
            "same-day control over the four cells is in the table above, and its direction does not depend on the hook.",
            "2. Tokens here are turns times the start-up context. The index shrinks the tool output, which is small next to one extra turn. "
            "Search without a call graph failed the callers question on the light class.",
            "3. Memory does not help single lookups: memory holding the answers only matches the two-turn floor, and memory from an exploring "
            "session used fewer tokens in a minority of cells.", ""]
    return out


def main() -> int:
    if len(sys.argv) == 2 and sys.argv[1] == "--guard-selftest":
        return 1 if guard_selftest(load("harness/arms.json"), install_of()) else 0
    if len(sys.argv) == 3 and sys.argv[1] == "--guard":
        scanned = guard_public(load("harness/arms.json"), install_of(), pathlib.Path(sys.argv[2]))
        print(f"guard passed: {scanned} files in {sys.argv[2]} hold nothing that must stay off the site")
        return 0
    results, index, arms = load("data/results.json"), load("data/index.json"), load("harness/arms.json")
    cells, agents = results["cells"], results["agents"]
    points = points_of(cells)
    used = {p["arm"] for p in points}
    known = {arm["id"] for arm in arms["arms"]}
    unnamed = sorted(arm["id"] for arm in arms["arms"] if arm.get("group") == "team" and arm["id"] in used and not arm.get("base")
                     and not public_arm(arm) and arm["id"] not in TEAM_UNPUBLISHED)
    if unnamed:
        raise SystemExit(f"team arms with results and no public name in TEAM_PUBLIC: {unnamed}")
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
    text = modelize_all(text)   # the lines are chosen; from here they say the model where they said a class
    data = {
        "generated": datetime.date.today().isoformat(),
        "totals": {"agents": len(agents), "cells": len(cells), "start": start,
                   "tokens": sum(a["input"] + a["output"] + a["cache_read"] + a["cache_write"] for a in agents)},
        "groups": arms["groups"],
        "arms": described,
        "tiers": [{**tier, "name": model_label(tier["id"]), "model": model_at(tier["id"])} for tier in arms["tiers"]],
        "setups": [s for s in arms["setups"] if any(p["setup"] == s["id"] for p in points)],
        "questions": arms["questions"],
        "sections": site_sections(),
        "points": points,
        "runs": sorted(cells, key=lambda c: (order.index(c["arm"]), setups.index(c["setup"]),
                                             TIER_ORDER.index(c["tier"]), c["rep"])),
        "index": index,
        "context": {"parts": context_parts(start)},
        "cache": [{"tier": model_at(t["id"]), "tokens": start, "cold": start * PRICE[t["id"]][3] / 1e6,
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
    public = public_of(points, cells, agents, index, orchestration, hooks, arms)
    (BENCH / "RESULTS.md").write_text(modelize(
        markdown(points, index, arms, text) + more_tables(orchestration, hooks, arms, text)
        + "\n".join(round_five(points, cells, agents, arms) + saving_section(public) + round_six(public) + known_weaknesses(public)) + "\n"),
        encoding="utf-8")
    in_readme = write_readme(modelize(readme_block(original, arms, text)))
    print("site data: " + ", ".join(f"{name} {size:,} bytes" for name, size in written))
    print(f"wrote {target.relative_to(REPO)} ({target.stat().st_size:,} bytes), benchmark/RESULTS.md"
          f"{' and the README results block' if in_readme else ' (README has no results markers)'}: "
          f"{len(points)} points from {len(cells)} runs, {len(agents)} agents, {len(described)} methods")
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
    # The last step of every build, so nothing that must stay off the site can be published by accident.
    scanned = guard_public(arms, public["install"])
    print(f"guard passed: {scanned} files under docs/ hold nothing that must stay off the site")
    # The guard is only worth having while it still catches what it was written to catch.
    if guard_selftest(arms, public["install"]):
        raise SystemExit("GUARD SELF-TEST FAILED: a planted violation was not caught.")
    # The films are made by another builder. Until their captions carry the figures of the page, they show an older result.
    stale = film_problems(public, {path.name: path.read_text(encoding="utf-8") for path in sorted((DOCS / "media").glob("*.vtt"))})
    for line in stale:
        print("warning: " + line)
    if stale:
        print("warning: THE FILMS ARE OUT OF DATE against the Overview. Do not publish before the films and their captions carry the figures above.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
