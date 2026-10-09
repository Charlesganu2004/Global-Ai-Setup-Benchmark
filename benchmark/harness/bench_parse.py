"""Turn workflow transcripts into benchmark rows.

    bench_parse.py OUT.json TRANSCRIPT_DIR...

Every agent the benchmark starts is labelled `arm|setup|tier|role|rep`. For each
one this reads the transcript, adds up what the API reported it used, prices
it, lists the tools it ran, and grades its answer against the syntax-tree truth.
Agents are then grouped into cells (arm, setup, tier, rep): the tokens of every
agent in the cell are summed and the cell's final answer is graded.
"""
from __future__ import annotations

import datetime
import json
import pathlib
import re
import sys

# Dollars per million tokens: input, output, cache read, cache write (5 minute,
# 1 hour). Input, output and the Opus and Sonnet cache-read prices are the
# published ones; cache writes use the published multipliers (1.25x and 2x
# input) and the Haiku cache read uses the usual 0.1x input.
PRICE = {
    "opus": (4.00, 20.00, 0.20, 5.00, 8.00),
    "sonnet": (2.00, 10.00, 0.20, 2.50, 4.00),
    "haiku": (0.10, 0.50, 0.01, 0.125, 0.20),
    "haiku-long": (0.50, 2.50, 0.05, 0.625, 1.00),   # prompts over 100K tokens
}

TRUTH = {
    "q1": {"dispatch_command", "_llm_tiebreak", "_call_llm", "detect_backend",
           "extract_files_direct", "_resolve_triage_backend", "triage_with_opus"},
    "q2": {"file": "cli.py", "line": 1064, "parameters": ["cmd"], "called_by": {"_run_cli"}},
    "q3": {"function": "_register_merge_driver", "file": "hooks.py", "line": 736,
           "removed_by": "_unregister_merge_driver"},
}

ALLOWED = {   # first word of a shell command each arm may run
    "A0": set(),
    "A1": {"graphify", "semble"},
    "A2": {"codanna"},
    "A3": {"lx"},
    "A4": {"lx"},
    "A5": {"token-goat"},
    "A6": {"token-goat", "lx"},
    "A7": {"lx"},
    "A8": {"lx"},
    "A9": {"lx"},
    "R1": {"lx"},
    "L1": {"lx"}, "L2": {"lx"}, "L3": {"lx"}, "L4": {"lx"},
    "C1": {"codanna", "token-goat", "graphify", "semble"},
    "C2": {"lx", "token-goat"},
    "C3": {"lx", "token-goat"},
    "C4": {"codanna", "token-goat"},
    "C5": {"codanna", "graphify", "semble"},
    "C6": {"token-goat", "graphify", "semble"},
    "C7": {"token-goat", "semble"},
    "C8": {"token-goat", "graphify"},
    "C9": {"lx", "token-goat"},
    "C10": {"lx"}, "C11": {"lx"}, "C12": {"lx"}, "C13": {"lx"},
    "C14": {"lx", "token-goat"},
    "C15": {"lx", "token-goat"},
    "N1": {"lx"}, "N2": {"lx"},
    "S1": {"lx"}, "S2": {"lx"}, "S3": {"lx"}, "S4": {"lx"},
}
# Arms told to read code through token-goat, never the Read tool.
NO_READ_TOOL = {"C9", "C15"}


def bare(name) -> str:
    """`pkg.mod.fn()`, `cli.py:fn`, `fn (cli.py:10-20)` and `fn` are the same answer."""
    words = str(name).replace("`", " ").split("(")[0].split()
    first = words[0] if words else ""
    return first.split("::")[-1].split(":")[-1].split(".")[-1].strip()


def f1(given, truth: set) -> float:
    given = {bare(item) for item in (given or []) if bare(item)}
    if not given:
        return 0.0
    hit = len(given & truth)
    if not hit:
        return 0.0
    precision, recall = hit / len(given), hit / len(truth)
    return 2 * precision * recall / (precision + recall)


def same_file(given, truth: str) -> bool:
    """The right file, whatever folder prefix, backticks or `:line` came with it."""
    name = str(given or "").replace("`", "").strip().replace("\\", "/").rstrip("/").split("/")[-1]
    return re.sub(r":\d+(-\d+)?$", "", name) == truth


def same_line(given, truth: int) -> bool:
    return str(given).strip().isdigit() and int(str(given).strip()) == truth


def grade(question: str, answer) -> float | None:
    if not isinstance(answer, dict):
        return None
    if question == "q1":
        return f1(answer.get("callers"), TRUTH["q1"])
    if question == "q2":
        want = TRUTH["q2"]
        return sum([same_file(answer.get("file"), want["file"]),
                    same_line(answer.get("line"), want["line"]),
                    [bare(p) for p in answer.get("parameters") or []] == want["parameters"],
                    {bare(c) for c in answer.get("called_by") or []} == want["called_by"]]) / 4
    want = TRUTH["q3"]
    return sum([bare(answer.get("function")) == want["function"],
                same_file(answer.get("file"), want["file"]),
                same_line(answer.get("line"), want["line"]),
                bare(answer.get("removed_by")) == want["removed_by"]]) / 4


def first_word(command: str) -> str:
    """The program a shell call really runs, past the `Set-Location ...;` prefix."""
    for part in re.split(r"[;\n]|&&", command):
        part = part.strip().lstrip("&").strip()
        if not part or re.match(r"(?i)(set-location|cd|push-location|pop-location)\b", part):
            continue
        word = part.split()[0].strip("\"'")
        return pathlib.PureWindowsPath(word).stem.lower() if ("\\" in word or "/" in word) else word.lower()
    return ""


def family(model: str) -> str:
    return next((name for name in ("opus", "sonnet", "haiku") if name in model), model)


def read_agent(path: pathlib.Path) -> dict:
    usage, calls, results, stamps, model = {}, {}, {}, [], ""
    for line in path.open(encoding="utf-8", errors="replace"):
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if isinstance(row.get("timestamp"), str):
            stamps.append(row["timestamp"])
        message = row.get("message")
        if not isinstance(message, dict):
            continue
        content = message.get("content")
        parts = content if isinstance(content, list) else []
        if isinstance(message.get("usage"), dict):
            # A streamed reply is written once per content block under one id,
            # and only the last write carries the final output count.
            key = message.get("id") or row.get("uuid")
            seen = usage.get(key)
            if seen is None or (message["usage"].get("output_tokens") or 0) >= (seen.get("output_tokens") or 0):
                usage[key] = message["usage"]
            model = message.get("model") or model
            for part in parts:
                if isinstance(part, dict) and part.get("type") == "tool_use":
                    calls[part.get("id")] = (part.get("name") or "", part.get("input") or {})
        else:
            for part in parts:
                if isinstance(part, dict) and part.get("type") == "tool_result":
                    body = part.get("content")
                    text = body if isinstance(body, str) else "".join(
                        piece.get("text", "") for piece in body or [] if isinstance(piece, dict))
                    results[part.get("tool_use_id")] = len(text)

    kind = family(model)
    # start: the whole prompt of the first request, the context an agent is born
    # with. Whether it was read from cache or written to it depends only on what
    # happened to run just before, so it is also priced both ways: cost_warm as
    # if all of it was already cached, cost_cold as if none of it was.
    total = dict(input=0, output=0, cache_read=0, cache_write=0, cost=0.0, largest_prompt=0,
                 start=0, start_cached=0, reread=0, added=0, cost_warm=0.0, cost_cold=0.0)
    for position, one in enumerate(usage.values()):
        fresh = one.get("input_tokens") or 0
        out = one.get("output_tokens") or 0
        read = one.get("cache_read_input_tokens") or 0
        write = one.get("cache_creation_input_tokens") or 0
        split = one.get("cache_creation") if isinstance(one.get("cache_creation"), dict) else {}
        hour = split.get("ephemeral_1h_input_tokens") or 0
        five = write - hour
        prompt = fresh + read + write
        rate = PRICE.get("haiku-long" if kind == "haiku" and prompt > 100_000 else kind)
        if rate:
            spent = (fresh * rate[0] + out * rate[1] + read * rate[2]
                     + five * rate[3] + hour * rate[4]) / 1e6
            total["cost"] += spent
            if position == 0:
                total["cost_warm"] += (fresh * rate[0] + out * rate[1] + (read + write) * rate[2]) / 1e6
                total["cost_cold"] += (fresh * rate[0] + out * rate[1] + (read + write) * rate[3]) / 1e6
            else:
                total["cost_warm"] += spent
                total["cost_cold"] += spent
        if position == 0:
            total["start"], total["start_cached"] = prompt, read
        else:
            total["reread"] += read
            total["added"] += write + fresh
        total["input"] += fresh
        total["output"] += out
        total["cache_read"] += read
        total["cache_write"] += write
        total["largest_prompt"] = max(total["largest_prompt"], prompt)

    tools, shell, answer, returned = {}, {}, None, 0
    for call_id, (name, given) in calls.items():
        if "structuredoutput" in name.lower().replace("_", ""):
            answer = given
            continue
        tools[name] = tools.get(name, 0) + 1
        returned += results.get(call_id, 0)
        command = given.get("command") if isinstance(given, dict) else None
        if isinstance(command, str):
            word = first_word(command)
            shell[word] = shell.get(word, 0) + 1

    seconds = None
    if len(stamps) > 1:
        when = sorted(datetime.datetime.fromisoformat(s.replace("Z", "+00:00")) for s in stamps)
        seconds = round((when[-1] - when[0]).total_seconds(), 1)
    return dict(total, model=model, requests=len(usage), tools=tools, shell=shell,
                tool_calls=sum(tools.values()), tool_output_chars=returned,
                answer=answer, seconds=seconds)


HOME_PATH = re.compile(r"(?i)[a-z]:[\\/]+users[\\/]+[^\\/\"']+[\\/]+(?:[^\\/\"']+[\\/]+)*?(lxbench[\\/]+repo(?:-noemb)?)[\\/]*")


def portable(value):
    """An answer with the machine's own folders cut off the front of any path in
    it, so `C:\\Users\\name\\...\\lxbench\\repo\\hooks.py` is stored as `hooks.py`."""
    if isinstance(value, str):
        return HOME_PATH.sub("", value)
    if isinstance(value, list):
        return [portable(item) for item in value]
    if isinstance(value, dict):
        return {key: portable(item) for key, item in value.items()}
    return value


def off_policy(arm: str, row: dict) -> list[str]:
    """Tool use the arm's policy did not allow."""
    found = [f"shell:{word}" for word in row["shell"] if word not in ALLOWED.get(arm, set())]
    if arm == "A0" and row["shell"]:
        found = [f"shell:{word}" for word in row["shell"]]
    for name in row["tools"]:
        if name in ("Agent", "WebSearch", "WebFetch", "Edit", "Write"):
            found.append(name)
        elif name == "Read" and arm in NO_READ_TOOL:
            found.append("Read")
    return found


def main() -> int:
    out, folders = pathlib.Path(sys.argv[1]), [pathlib.Path(p) for p in sys.argv[2:]]
    agents = []
    for folder in folders:
        for meta in sorted(folder.glob("agent-*.meta.json")):
            label = json.loads(meta.read_text(encoding="utf-8")).get("description") or ""
            bits = label.split("|")
            if len(bits) != 5:
                continue
            log = meta.with_name(meta.name.replace(".meta.json", ".jsonl"))
            if not log.exists():
                continue
            arm, setup, tier, role, rep = bits
            row = read_agent(log)
            row.update(arm=arm, setup=setup, tier=tier, role=role, rep=rep, run=folder.name,
                       off_policy=off_policy(arm, row), answer=portable(row["answer"]))
            agents.append(row)

    # The planning prompt is identical for every method and tier, so it was run
    # a few times and its briefs reused. A cell that reused them is charged the
    # mean of the planning calls that were measured.
    SUMMED = ("input", "output", "cache_read", "cache_write", "cost", "requests",
              "tool_calls", "tool_output_chars", "start", "start_cached", "reread", "added",
              "cost_warm", "cost_cold")
    plans = [row for row in agents if row["role"] == "plan"]
    mean_plan = None
    if plans:
        mean_plan = {field: sum(row[field] for row in plans) / len(plans) for field in SUMMED}
        mean_plan.update(role="plan", seconds=sum(row["seconds"] or 0 for row in plans) / len(plans),
                         off_policy=[], answer=None, imputed=True)

    cells = {}
    for row in agents:
        cells.setdefault((row["arm"], row["setup"], row["tier"], row["rep"]), []).append(row)
    table = []
    for (arm, setup, tier, rep), rows in sorted(cells.items()):
        measured = len(rows)
        if setup == "orch" and mean_plan and not any(row["role"] == "plan" for row in rows):
            rows = rows + [mean_plan]
        by_role = {row["role"]: row for row in rows}
        if setup == "single":
            final = (by_role.get("all") or {}).get("answer") or {}
        elif setup == "parallel":
            final = {q: (by_role.get(q) or {}).get("answer") for q in ("q1", "q2", "q3")}
        else:
            final = (by_role.get("verify") or {}).get("answer") or {}
        scores = {q: grade(q, final.get(q)) for q in ("q1", "q2", "q3")}
        workers = {q: grade(q, (by_role.get("w-" + q) or {}).get("answer")) for q in ("q1", "q2", "q3")}
        expected = {"single": 1, "parallel": 3, "orch": 5}[setup]
        # Complete means every agent of the cell ran to its structured answer: a
        # transcript still being written, or one that died, must not count as a run.
        answered = all(row.get("answer") is not None for row in rows if not row.get("imputed"))
        cell = dict(arm=arm, setup=setup, tier=tier, rep=rep, agents=measured,
                    complete=len(rows) == expected and answered,
                    plan_imputed=len(rows) != measured, scores=scores,
                    quality=round(sum(v or 0 for v in scores.values()) / 3, 4),
                    worker_scores=workers if setup == "orch" else None,
                    off_policy=sorted({item for row in rows for item in row["off_policy"]}),
                    wall_seconds=max((row["seconds"] or 0) for row in rows) if setup == "parallel"
                    else round(sum(row["seconds"] or 0 for row in rows if row["role"] in ("all", "plan", "verify"))
                               + max([row["seconds"] or 0 for row in rows if row["role"].startswith("w-")] or [0]), 1))
        for field in SUMMED:
            cell[field] = round(sum(row[field] for row in rows), 6)
        for field in SUMMED:
            if not field.startswith("cost"):
                cell[field] = round(cell[field])
        cell["tokens"] = cell["input"] + cell["output"] + cell["cache_read"] + cell["cache_write"]
        if setup == "orch":
            lead = [row for row in rows if row["role"] in ("plan", "verify")]
            cell["orchestrator_tokens"] = sum(r["input"] + r["output"] + r["cache_read"] + r["cache_write"] for r in lead)
            cell["orchestrator_cost"] = round(sum(r["cost"] for r in lead), 6)
            cell["orchestrator_cost_warm"] = round(sum(r["cost_warm"] for r in lead), 6)
        table.append(cell)

    out.write_text(json.dumps({"agents": agents, "cells": table}, indent=1), encoding="utf-8")
    print(f"{len(agents)} agents in {len(table)} cells -> {out}")
    print(f"{'cell':<22}{'ok':>3}{'req':>5}{'tools':>6}{'input':>8}{'cache wr':>10}{'cache rd':>10}{'output':>8}"
          f"{'total':>10}{'$':>9}{'qual':>6}{'sec':>6}  off-policy")
    for c in table:
        name = f"{c['arm']} {c['setup']} {c['tier']} {c['rep']}"
        print(f"{name:<22}{'y' if c['complete'] else 'N':>3}{c['requests']:>5}{c['tool_calls']:>6}{c['input']:>8}"
              f"{c['cache_write']:>10}{c['cache_read']:>10}{c['output']:>8}{c['tokens']:>10}{c['cost']:>9.4f}"
              f"{c['quality']:>6.2f}{c['wall_seconds']:>6.0f}  {','.join(c['off_policy'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
