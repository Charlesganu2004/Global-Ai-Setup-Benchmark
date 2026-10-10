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
    "H0": set(), "H9": {"lx"}, "H1": {"lx"}, "H3": {"lx"}, "H5": {"token-goat"},
    "SB": {"lx"}, "HB": {"lx"},
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


# What token-goat's hooks write over text they redact, as in `[REDACTED:generic_secret_assignment]`.
REDACTED = "[REDACTED:"
# How the client words the result of a tool call that a hook refused before it ran.
REFUSED = re.compile(r"\s*PreToolUse:\w+ hook error")


def count_hook_row(note: dict, hooks: dict) -> None:
    """Add one transcript row about a hook to the counters, when the hook is token-goat's.

    A run that went through is a `hook_success` row with the hook's answer as JSON in stdout. A note
    the hook added beside a result is a row of its own, `hook_additional_context`, with no command.
    A run that blocked is a `hook_blocking_error` row with the command inside `blockingError`.
    """
    kind = str(note.get("type", ""))
    if kind == "hook_additional_context":
        hooks["hook_notes"] += "token-goat" in str(note.get("content", ""))
        return
    if kind == "hook_blocking_error":
        blocked = note.get("blockingError") if isinstance(note.get("blockingError"), dict) else {}
        if "token-goat" in str(blocked.get("command", "")):
            hooks["hook_calls"] += 1
            hooks["hook_denials"] += 1
        return
    if not kind.startswith("hook") or "token-goat" not in str(note.get("command", "")):
        return
    hooks["hook_calls"] += 1
    try:
        said = json.loads(note.get("stdout") or "{}")
    except ValueError:
        return
    if not isinstance(said, dict):
        return
    specific = said.get("hookSpecificOutput") if isinstance(said.get("hookSpecificOutput"), dict) else {}
    hooks["hook_rewrites"] += "updatedToolOutput" in specific or "updatedInput" in specific
    hooks["hook_denials"] += specific.get("permissionDecision") == "deny" or said.get("decision") == "block"


def read_agent(path: pathlib.Path) -> dict:
    usage, calls, results, stamps, model = {}, {}, {}, [], ""
    hooks = dict(hook_calls=0, hook_rewrites=0, hook_denials=0, hook_notes=0, hook_redactions=0)
    for line in path.open(encoding="utf-8", errors="replace"):
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if isinstance(row.get("timestamp"), str):
            stamps.append(row["timestamp"])
        note = row.get("attachment") if row.get("type") == "attachment" else None
        if isinstance(note, dict):
            count_hook_row(note, hooks)
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
                    # Places where a hook put a marker in place of source text it took for a secret.
                    hooks["hook_redactions"] += text.count(REDACTED)
                    if REFUSED.match(text) and ("[tg]" in text or "token-goat" in text):
                        # A call the hook refused leaves no hook row, only this error in place of the result.
                        hooks["hook_calls"] += 1
                        hooks["hook_denials"] += 1

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
    # The class of model, never its version: opus, sonnet or haiku.
    return dict(total, model=kind, requests=len(usage), tools=tools, shell=shell,
                tool_calls=sum(tools.values()), tool_output_chars=returned,
                answer=answer, seconds=seconds, began=min(stamps) if stamps else "", **hooks)


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


def off_policy(arm: str, row: dict, setup: str = "", role: str = "") -> list[str]:
    """Tool use the arm's policy did not allow."""
    if setup == "orch-notools" and role == "verify":
        # This reviewer is told to call nothing but the structured output.
        return sorted(row["tools"])
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
            # The callers this agent's own answer to question 1 left out: None when it was not asked.
            own = row["answer"] if isinstance(row["answer"], dict) else None
            asked = (own or {}).get("q1") if role in ("all", "verify") else own if role in ("q1", "w-q1") else None
            row["missed_callers"] = (sorted(TRUTH["q1"] - {bare(c) for c in asked.get("callers") or []})
                                     if isinstance(asked, dict) else None)
            row.update(arm=arm, setup=setup, tier=tier, role=role, rep=rep, run=folder.name,
                       off_policy=off_policy(arm, row, setup, role), answer=portable(row["answer"]))
            agents.append(row)

    # A round that was stopped and resumed runs most of its agents a second time
    # and leaves both transcripts under one label. The later one is the run,
    # because it is the one the resumed round went on to use, and the earlier
    # one is left out whether it was cut off or had answered. A later one that
    # never answered is kept too: its cell is then incomplete, which is right,
    # and an earlier answer is never put in its place beside agents that did
    # not see it.
    twins: dict[tuple, list[dict]] = {}
    for row in agents:
        twins.setdefault((row["run"], row["arm"], row["setup"], row["tier"], row["role"], row["rep"]), []).append(row)
    superseded = []
    for rows in twins.values():
        if len(rows) > 1:
            kept = max(rows, key=lambda r: r["began"])
            superseded += [r for r in rows if r is not kept]
    if superseded:
        agents = [row for row in agents if not any(row is gone for gone in superseded)]
        print(f"left out {len(superseded)} earlier transcripts of agents that a resumed round ran again "
              f"({sum(r['answer'] is None for r in superseded)} of them cut off before answering)")
    for row in agents:
        del row["began"]

    # The planning prompt is identical for every method and tier, so it was run
    # a few times and its briefs reused. A cell that reused them is charged the
    # mean of the planning calls that were measured.
    SUMMED = ("input", "output", "cache_read", "cache_write", "cost", "requests",
              "tool_calls", "tool_output_chars", "start", "start_cached", "reread", "added",
              "cost_warm", "cost_cold", "hook_calls", "hook_rewrites", "hook_denials", "hook_notes",
              "hook_redactions")
    plans = [row for row in agents if row["role"] == "plan"]
    mean_plan = None
    if plans:
        mean_plan = {field: sum(row[field] for row in plans) / len(plans) for field in SUMMED}
        mean_plan.update(role="plan", seconds=sum(row["seconds"] or 0 for row in plans) / len(plans),
                         off_policy=[], answer=None, imputed=True, missed_callers=None)

    cells = {}
    for row in agents:
        cells.setdefault((row["arm"], row["setup"], row["tier"], row["rep"]), []).append(row)
    table = []
    for (arm, setup, tier, rep), rows in sorted(cells.items()):
        measured = len(rows)
        orch = setup.startswith("orch")
        if orch and mean_plan and not any(row["role"] == "plan" for row in rows):
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
        expected = 5 if orch else {"single": 1, "parallel": 3}[setup]
        # Complete means every agent of the cell ran to its structured answer: a
        # transcript still being written, or one that died, must not count as a run.
        answered = all(row.get("answer") is not None for row in rows if not row.get("imputed"))
        cell = dict(arm=arm, setup=setup, tier=tier, rep=rep, agents=measured,
                    complete=len(rows) == expected and answered,
                    plan_imputed=len(rows) != measured, scores=scores,
                    quality=round(sum(v or 0 for v in scores.values()) / 3, 4),
                    worker_scores=workers if orch else None,
                    # What the agent that looked question 1 up left out, before any review.
                    lookup_missed=next((row["missed_callers"] for row in rows
                                        if row["role"] in ("all", "q1", "w-q1") and row["missed_callers"] is not None), None),
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
        if orch:
            lead = [row for row in rows if row["role"] in ("plan", "verify")]
            cell["orchestrator_tokens"] = sum(r["input"] + r["output"] + r["cache_read"] + r["cache_write"] for r in lead)
            cell["orchestrator_cost"] = round(sum(r["cost"] for r in lead), 6)
            cell["orchestrator_cost_warm"] = round(sum(r["cost_warm"] for r in lead), 6)

            def part(members: list) -> dict:
                """What one role of the run used: the planner, the three workers together, the reviewer."""
                return {"tokens": round(sum(r["input"] + r["output"] + r["cache_read"] + r["cache_write"] for r in members)),
                        "requests": round(sum(r["requests"] for r in members), 2),
                        "tool_calls": round(sum(r["tool_calls"] for r in members), 2),
                        "output": round(sum(r["output"] for r in members)),
                        "cost_warm": round(sum(r["cost_warm"] for r in members), 6),
                        "cost_cold": round(sum(r["cost_cold"] for r in members), 6),
                        "seconds": round(max([r["seconds"] or 0 for r in members] or [0]), 1)}

            cell["roles"] = {"plan": part([r for r in rows if r["role"] == "plan"]),
                             "workers": part([r for r in rows if r["role"].startswith("w-")]),
                             "verify": part([r for r in rows if r["role"] == "verify"])}
            # What the review changed, question by question.
            right = lambda value: (value or 0) >= 0.999
            cell["worker_quality"] = round(sum(v or 0 for v in workers.values()) / 3, 4)
            cell["fixed"] = sum(1 for q in scores if not right(workers[q]) and right(scores[q]))
            cell["broken"] = sum(1 for q in scores if right(workers[q]) and not right(scores[q]))
            cell["still_wrong"] = sum(1 for q in scores if not right(workers[q]) and not right(scores[q]))
            cell["reviewer"] = family((by_role.get("verify") or {}).get("model") or "")
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
