"""Turn workflow transcripts into benchmark rows.

    bench_parse.py OUT.json TRANSCRIPT_DIR...

Every agent the benchmark starts is labelled `arm|setup|tier|role|rep`. For each
one this reads the transcript, adds up what the API reported it used, prices
it, lists the tools it ran, and grades its answer against the syntax-tree truth.
Agents are then grouped into cells (arm, setup, tier, rep): the tokens of every
agent in the cell are summed and the cell's final answer is graded.

There are two question sets. Set A is the original q1, q2, q3. Set B is held out:
three questions nobody tuned for, run by an arm id plus `h` ("RGSh" is "RGS" on
set B). A set B answer is still stored under q1, q2, q3. Every agent row and
every cell carries `set`, "A", "B" or "T", and an `h` arm follows the rules of its
base arm: allowed shell words, read rules, repository paths.

Round six (team work, see harness/ROUND6.md) adds the arms R6, TP, TPG, TPS, TH, TPH
and the set T arms T0, TL, TI. Set T is a demonstration set: three questions that need
a note an earlier agent left behind. The rows of a round six arm carry four more
fields (`pgr_lookups`, `hub_calls`, `from_brief`, `banks`) and its cells a few more; the rows
and cells of every earlier round are written exactly as before. A round six row or cell is
also listed as off policy for hub use its arm's prompt does not allow (`hub:not-allowed`,
`bank:foreign`), for a pgr call that pgr itself refused (`pgr:usage-error`: the pgr on PATH was
too old), and for a hub cell that ran into a bank an earlier attempt had filed (`bank:rerun`,
`bank:shared`, `bank:mixed`).
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
# Set B, the held-out questions. The same shapes; the fourth field of q3 is read_by.
TRUTH_B = {
    "q1": {"_prune_graph_json_sources", "_save_global_graph", "_save_manifest", "dispatch_command",
           "save_manifest", "to_canvas", "to_json"},
    "q2": {"file": "security.py", "line": 103, "parameters": ["url"],
           "called_by": {"download_audio", "ingest", "redirect_request", "safe_fetch"}},
    "q3": {"function": "_is_ignored", "file": "detect.py", "line": 1448,
           # Two functions read .graphifyignore files: the loader for a whole tree and
           # the one it uses for a single folder. Either is a correct answer.
           "read_by": frozenset({"_load_graphifyignore", "_load_dir_own_ignore"})},
}
# Set T, the demonstration set of round six. The lookup fields reuse the truth of sets A and B (the
# same functions, graded by the same rules); the note fields come from the notes that
# prepare_team.py files, so test_round6.py checks the two against each other.
TRUTH_T = {
    # a decision: the note cites a ticket and says the parameters must keep their names
    "q1": {"ticket": "BLD-4172", "may_rename": False, "file": TRUTH["q2"]["file"], "line": TRUTH["q2"]["line"]},
    # a stale note: a reviewer described the function, then its body changed
    "q2": {"status": "changed", "filed_by": "reviewer", "called_by": TRUTH_B["q2"]["called_by"]},
    # a confirmed fact: two roles filed the same sentence
    "q3": {"function": TRUTH["q3"]["function"], "roles": ["builder", "scout"],
           "file": TRUTH["q3"]["file"], "line": TRUTH["q3"]["line"]},
}
# The fields of set T that only a note can answer; the others are lookups.
NOTE_FIELDS = {"q1.ticket", "q1.may_rename", "q2.status", "q2.filed_by", "q3.roles"}
TRUTHS = {"A": TRUTH, "B": TRUTH_B, "T": TRUTH_T}

# Round six arms. TH and TPH add the hub (`pgr handoff` and `pgr loadout`), the arms with P in
# the name put a `pgr pack` in the prompt, T0, TL and TI ask set T.
TEAM_ARMS = {"R6", "TP", "TPG", "TPS", "TH", "TPH", "T0", "TL", "TI"}
SET_T_ARMS = {"T0", "TL", "TI"}
HUB_ARMS = {"TH", "TPH"}          # the subagents file with handoff, the reviewer starts from loadout
TEAM_BANK_ARMS = {"TL", "TI"}     # loadout and recall on the team bank
HUB_COMMANDS = ("handoff", "loadout", "recall")
# What pgr prints when it refuses a command line, as pgr 0.1 does for every hub command; each form ends with this hint.
USAGE_ERROR = re.compile(r"pgr: (?:unknown (?:option|command)[^\n]*|no command given) \(try: pgr help\)")

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
    # Round six: R6 starts with R and a digit, so the pgr rule in allowed_words does not see it.
    **{arm: {"pgr"} for arm in TEAM_ARMS},
}
# Arms told to read code through token-goat, never the Read tool.
NO_READ_TOOL = {"C9", "C15"}


def base_arm(arm: str) -> str:
    """`RGSh` is the method `RGS` run on the held-out question set B; every per-arm rule uses the base id."""
    # `A0r` is `A0` run again on the day of a later round, on the same set A.
    return arm[:-1] if arm[-1:] in ("h", "r") and len(arm) > 1 else arm


def question_set(arm: str) -> str:
    if base_arm(arm) in SET_T_ARMS:
        return "T"
    return "B" if arm.endswith("h") and len(arm) > 1 else "A"


def is_team(arm: str) -> bool:
    """A round six arm, in any of its forms (`TPh` is `TP` on set B)."""
    return base_arm(arm) in TEAM_ARMS


def allowed_words(arm: str) -> set:
    """The shell words an arm may run. A pgr arm is R and a letter (RG, RS, RM, RGS, RGM, RSM, RGSM,
    RGSO) and may run `pgr`; R1, which also starts with R, is an lx arm."""
    base = base_arm(arm)
    if re.match(r"R[A-Z]", base):
        return {"pgr"}
    return ALLOWED.get(base, set())


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


def plain(value) -> str:
    """A note field without what an answer may put around it: the case, spaces, quotes, backticks, asterisks,
    brackets and the punctuation at either end. `BLD-4172.` and `bld-4172` are the ticket BLD-4172; the
    decision BLD-4172 and BLD-4180 are not."""
    return str(value or "").strip(" \t\r\n`'\"*_()[]{}<>.,;:!?").lower()


def plain_roles(given) -> list[str] | None:
    """The roles of an answer as sorted plain names, or None when it is not a list. One item may hold
    several roles, joined by + (as a loadout prints them) or a comma."""
    if not isinstance(given, list):
        return None
    return sorted(name for item in given for name in (plain(part) for part in re.split(r"[+,]", str(item))) if name)


def team_fields(question: str, answer: dict) -> dict:
    """Set T: which fields of one answer are right, by field name. A note field is right only when it is the
    same word once the case and the decoration are taken off (see plain): the ticket, the enum word, the
    role, and the role list by name, whatever order it came in; the boolean must be the boolean itself.
    The lookup fields follow the rules of sets A and B: the file by name, the line as a number, the
    callers as a set of bare names."""
    want = TRUTH_T[question]
    if question == "q1":
        return {"ticket": plain(answer.get("ticket")) == want["ticket"].lower(),
                "may_rename": answer.get("may_rename") is want["may_rename"],
                "file": same_file(answer.get("file"), want["file"]),
                "line": same_line(answer.get("line"), want["line"])}
    if question == "q2":
        return {"status": plain(answer.get("status")) == want["status"],
                "filed_by": plain(answer.get("filed_by")) == want["filed_by"],
                "called_by": {bare(c) for c in answer.get("called_by") or []} == want["called_by"]}
    return {"function": bare(answer.get("function")) == want["function"],
            "roles": plain_roles(answer.get("roles")) == want["roles"],
            "file": same_file(answer.get("file"), want["file"]),
            "line": same_line(answer.get("line"), want["line"])}


def grade(question: str, answer, which: str = "A") -> float | None:
    """Score one answer against the truth of question set `which` ("A", "B" or "T")."""
    if not isinstance(answer, dict):
        return None
    if which == "T":
        fields = team_fields(question, answer)
        return sum(fields.values()) / len(fields)
    truth = TRUTHS[which]
    if question == "q1":
        return f1(answer.get("callers"), truth["q1"])
    if question == "q2":
        want = truth["q2"]
        return sum([same_file(answer.get("file"), want["file"]),
                    same_line(answer.get("line"), want["line"]),
                    [bare(p) for p in answer.get("parameters") or []] == want["parameters"],
                    {bare(c) for c in answer.get("called_by") or []} == want["called_by"]]) / 4
    want = truth["q3"]
    fourth = "read_by" if "read_by" in want else "removed_by"
    # A truth may name several functions that all answer the question: any one is right.
    accepted = want[fourth] if isinstance(want[fourth], (set, frozenset)) else {want[fourth]}
    return sum([bare(answer.get("function")) == want["function"],
                same_file(answer.get("file"), want["file"]),
                same_line(answer.get("line"), want["line"]),
                bare(answer.get(fourth)) in accepted]) / 4


def first_word(command: str) -> str:
    """The program a shell call really runs, past the `Set-Location ...;` prefix."""
    for part in re.split(r"[;\n]|&&", command):
        part = part.strip().lstrip("&").strip()
        if not part or re.match(r"(?i)(set-location|cd|push-location|pop-location)\b", part):
            continue
        # A quoted text piped into a program ('a finding' | pgr handoff -) runs the program after the pipe.
        piped = re.match(r"""(?:"[^"]*"|'[^']*')\s*\|\s*(\S.*)""", part, re.S)
        if piped:
            part = piped.group(1).strip()
        word = part.split()[0].strip("\"'")
        return pathlib.PureWindowsPath(word).stem.lower() if ("\\" in word or "/" in word) else word.lower()
    return ""


def statements(command: str) -> list[str]:
    """The statements of one shell call, split at `;`, a line end or `&&` outside quotes, so that a
    finding handed to `pgr handoff` may contain a semicolon."""
    parts, current, quote, at = [], [], "", 0
    while at < len(command):
        char = command[at]
        if quote:
            current.append(char)
            quote = "" if char == quote else quote
        elif char in "\"'":
            current.append(char)
            quote = char
        elif char in ";\n" or command.startswith("&&", at):
            parts.append("".join(current))
            current = []
            at += 1 if char == "&" else 0
        else:
            current.append(char)
        at += 1
    parts.append("".join(current))
    return [part.strip() for part in parts if part.strip()]


def words(statement: str) -> list[str]:
    """The words of one statement with their quotes taken off; a quoted text is one word."""
    found, current, quote, started = [], [], "", False
    for char in statement:
        if quote:
            if char == quote:
                quote = ""
            else:
                current.append(char)
        elif char in "\"'":
            quote, started = char, True
        elif char.isspace():
            if current or started:
                found.append("".join(current))
            current, started = [], False
        else:
            current.append(char)
    if current or started:
        found.append("".join(current))
    return found


# The pgr flags that go before the command and take a value.
PGR_GLOBAL = {"--root", "--engines", "--bank", "--task", "--from"}


def pipe_segments(statement: str) -> list[str]:
    """The parts of a statement on either side of a pipe, split outside quotes, so that a finding that holds
    a bar is not cut."""
    parts, current, quote = [], [], ""
    for char in statement:
        if quote:
            quote = "" if char == quote else quote
        elif char in "\"'":
            quote = char
        elif char == "|":
            parts.append("".join(current))
            current = []
            continue
        current.append(char)
    parts.append("".join(current))
    return [part.strip() for part in parts if part.strip()]


def pgr_call(statement: str) -> tuple[str, str] | None:
    """(command, bank) of a statement that runs pgr, else None. The command is the first word after
    pgr's own flags ("ask", "loadout", ...), empty for `pgr --version`; the bank is the --bank value.
    Either side of a pipe may be the pgr call: `pgr ask x | Select-Object -First 5` and
    `'a finding' | pgr handoff -` both run pgr."""
    for segment in pipe_segments(statement):
        found = [word for word in words(segment) if word != "&"]
        if not found:
            continue
        head = found[0].lstrip("&")
        stem = pathlib.PureWindowsPath(head).stem.lower() if ("\\" in head or "/" in head) else head.lower().removesuffix(".exe")
        if stem != "pgr":
            continue
        bank, at = "", 1
        while at < len(found) and found[at].startswith("--"):
            flag, equals, value = found[at].partition("=")
            step = 1 if equals or flag not in PGR_GLOBAL else 2
            if flag == "--bank":
                bank = value if equals else (found[at + 1] if at + 1 < len(found) else "")
            at += step
        return (found[at] if at < len(found) else ""), bank
    return None


def team_counts(commands: list[str], row: dict, refusals: list[bool] | None = None) -> dict:
    """What a round six agent did with the engine, from the shell calls it made. `refusals` says, for each
    call, whether pgr answered it with a usage error: such a call did nothing, so it is counted nowhere.

    pgr_lookups: pgr statements that look something up (ask, about, find, ...).
    hub_calls:   pgr statements of the hub, by command: handoff, loadout, recall.
    from_brief:  it answered and made no lookup at all. Filing a finding with handoff is not a lookup, so
                 an agent that only filed one is still answering from its brief; any other tool call
                 (a pgr lookup, loadout, recall, Read, Grep) is.
    banks:       every --bank value its pgr statements used.
    """
    hub = {name: 0 for name in HUB_COMMANDS}
    lookups, filing, banks = 0, 0, set()
    for number, command in enumerate(commands):
        refused = bool(refusals and refusals[number])
        kinds = []
        for statement in statements(command):
            call = pgr_call(statement)
            if call is None:
                kinds.append("nav" if re.match(r"(?i)(set-location|cd|push-location|pop-location)\b", statement) else "other")
                continue
            sub, bank = call
            if not refused:
                banks.update([bank] if bank else [])
                if sub in hub:
                    hub[sub] += 1
                elif sub:
                    lookups += 1
            kinds.append("handoff" if sub == "handoff" else "other")
        # A call that only files a finding (beside the Set-Location prefix) is not a lookup.
        filing += "handoff" in kinds and all(kind in ("nav", "handoff") for kind in kinds)
    return {"pgr_lookups": lookups, "hub_calls": hub,
            "from_brief": row["answer"] is not None and row["tool_calls"] - filing == 0,
            "banks": sorted(banks)}


def refused_calls(commands: list[str], refusals: list[bool]) -> int:
    """How many shell calls that run pgr got a usage error back."""
    return sum(1 for command, refused in zip(commands, refusals)
               if refused and any(pgr_call(statement) for statement in statements(command)))


def hub_policy(arm: str, setup: str, tier: str, role: str, rep: str, row: dict) -> list[str]:
    """Hub use that the arm's prompt does not allow. The prompts list, for TH and TPH, a worker's handoff and
    the reviewer's loadout on the cell's own bank (r6-TAG-ARM-SETUP-TIER-REP, lower case, the arm as written
    with its h); for TL and TI, loadout and recall on the team bank; for every other round six arm, no hub
    command and no bank at all."""
    base = base_arm(arm)
    if base in HUB_ARMS:
        allowed = {"handoff"} if role.startswith("w-") else {"loadout"} if role == "verify" else set()
        own = f"-{arm}-{setup}-{tier}-{rep}".lower()
        foreign = [bank for bank in row["banks"] if not (bank.startswith("r6-") and bank.endswith(own))]
    elif base in TEAM_BANK_ARMS:
        allowed, foreign = {"loadout", "recall"}, []
    else:
        allowed, foreign = set(), list(row["banks"])
    found = ["hub:not-allowed"] if any(count and name not in allowed for name, count in row["hub_calls"].items()) else []
    return found + (["bank:foreign"] if foreign else [])


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
    refused = set()     # ids of the tool calls whose result was a pgr usage error
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
                    if USAGE_ERROR.search(text):
                        refused.add(part.get("tool_use_id"))
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

    tools, shell, answer, returned, commands, refusals = {}, {}, None, 0, [], []
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
            commands.append(command)
            refusals.append(call_id in refused)

    seconds = None
    if len(stamps) > 1:
        when = sorted(datetime.datetime.fromisoformat(s.replace("Z", "+00:00")) for s in stamps)
        seconds = round((when[-1] - when[0]).total_seconds(), 1)
    # The class of model, never its version: opus, sonnet or haiku.
    return dict(total, model=kind, requests=len(usage), tools=tools, shell=shell,
                tool_calls=sum(tools.values()), tool_output_chars=returned,
                answer=answer, seconds=seconds, began=min(stamps) if stamps else "", commands=commands, refusals=refusals, **hooks)


HOME_PATH = re.compile(r"(?i)[a-z]:[\\/]+users[\\/]+[^\\/\"']+[\\/]+(?:[^\\/\"']+[\\/]+)*?(lxbench[\\/]+repo(?:-noemb|-mem-orient|-mem|-team)?)[\\/]*")


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
    base = base_arm(arm)
    found = [f"shell:{word}" for word in row["shell"] if word not in allowed_words(base)]
    if base == "A0" and row["shell"]:
        found = [f"shell:{word}" for word in row["shell"]]
    for name in row["tools"]:
        if name in ("Agent", "WebSearch", "WebFetch", "Edit", "Write"):
            found.append(name)
        elif name == "Read" and base in NO_READ_TOOL:
            found.append("Read")
    return found


def read_folders(folders: list[pathlib.Path]) -> list[dict]:
    """Every agent the transcript folders hold, read, priced and graded. A round that was stopped and resumed
    leaves two transcripts under one label; only the later one is kept."""
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
            commands, refusals = row.pop("commands"), row.pop("refusals")
            # The callers this agent's own answer to question 1 left out: None when it was not asked
            # (and for set T, whose first question is not about callers).
            own = row["answer"] if isinstance(row["answer"], dict) else None
            asked = (own or {}).get("q1") if role in ("all", "verify") else own if role in ("q1", "w-q1") else None
            which = question_set(arm)
            row["missed_callers"] = (sorted(TRUTHS[which]["q1"] - {bare(c) for c in asked.get("callers") or []})
                                     if isinstance(asked, dict) and which != "T" else None)
            row.update(arm=arm, setup=setup, tier=tier, role=role, rep=rep, run=folder.name, set=which,
                       off_policy=off_policy(arm, row, setup, role), answer=portable(row["answer"]))
            if is_team(arm):
                row.update(team_counts(commands, row, refusals))
                flags = hub_policy(arm, setup, tier, role, rep, row) + (["pgr:usage-error"] if refused_calls(commands, refusals) else [])
                row["off_policy"] = sorted(set(row["off_policy"]) | set(flags))
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
            earlier = [r for r in rows if r is not kept]
            superseded += earlier
            # An earlier attempt that filed findings left them in the bank that the kept one files into again:
            # the reviewer then reads notes of two attempts, and the cell is not a clean one.
            if any(r.get("hub_calls", {}).get("handoff") for r in earlier):
                kept["off_policy"] = sorted(set(kept["off_policy"]) | {"bank:rerun"})
    if superseded:
        agents = [row for row in agents if not any(row is gone for gone in superseded)]
        print(f"left out {len(superseded)} earlier transcripts of agents that a resumed round ran again "
              f"({sum(r['answer'] is None for r in superseded)} of them cut off before answering)")
    for row in agents:
        del row["began"]
    return agents


def team_cell(rows: list[dict], final: dict, which: str) -> dict:
    """The extra fields of a round six cell, counted over the agents that looked something up or answered
    (the planner, and the planner imputed from earlier rounds, are left out): the share that answered from
    its brief, the pgr lookups and hub calls, the banks used. On set T also how many of the note fields and
    of the lookup fields of the final answers are right."""
    asking = [row for row in rows if not row.get("imputed") and row["role"] != "plan"]
    out = {"from_brief_share": round(sum(bool(row.get("from_brief")) for row in asking) / len(asking), 4) if asking else None,
           "pgr_lookups": sum(row.get("pgr_lookups", 0) for row in asking),
           "hub_calls": {name: sum(row.get("hub_calls", {}).get(name, 0) for row in asking) for name in HUB_COMMANDS},
           "banks": sorted({bank for row in asking for bank in row.get("banks", [])})}
    if which == "T":
        count = {"note": [0, 0], "lookup": [0, 0]}
        for q in ("q1", "q2", "q3"):
            answer = final.get(q)
            for field, right in team_fields(q, answer if isinstance(answer, dict) else {}).items():
                kind = "note" if f"{q}.{field}" in NOTE_FIELDS else "lookup"
                count[kind][0] += right
                count[kind][1] += 1
        out["note_fields"] = {"right": count["note"][0], "of": count["note"][1]}
        out["lookup_fields"] = {"right": count["lookup"][0], "of": count["lookup"][1]}
    return out


def flag_banks(table: list[dict]) -> None:
    """A hub cell writes to a bank of its own (named r6-...). Two cells that share one, or a cell that used
    more than one, could have read notes it should not have seen: it is listed as off policy."""
    owners: dict[str, list[dict]] = {}
    for cell in table:
        own = [bank for bank in cell.get("banks", []) if bank.startswith("r6-")]
        for bank in own:
            owners.setdefault(bank, []).append(cell)
        if len(own) > 1:
            cell["off_policy"] = sorted(set(cell["off_policy"]) | {"bank:mixed"})
    for cells in owners.values():
        if len(cells) > 1:
            for cell in cells:
                cell["off_policy"] = sorted(set(cell["off_policy"]) | {"bank:shared"})


def build_cells(agents: list[dict]) -> list[dict]:
    """Group agents into cells (arm, setup, tier, rep): the tokens of every agent in the cell are summed and the
    cell's final answer is graded. Reads only what the agent rows hold, so it can be run again on stored rows."""
    # The planning prompt is identical for every method and tier, so it was run
    # a few times and its briefs reused. A cell that reused them is charged the
    # mean of the planning calls that were measured. That holds for set A only:
    # a set B cell plans for itself, so it is neither averaged in nor charged the mean.
    SUMMED = ("input", "output", "cache_read", "cache_write", "cost", "requests",
              "tool_calls", "tool_output_chars", "start", "start_cached", "reread", "added",
              "cost_warm", "cost_cold", "hook_calls", "hook_rewrites", "hook_denials", "hook_notes",
              "hook_redactions")
    # Round six plans are left out of the mean, so that the cells of earlier rounds keep their numbers
    # when the stored rows are parsed together with a later round.
    plans = [row for row in agents if row["role"] == "plan" and row["set"] == "A" and not is_team(row["arm"])]
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
        which = question_set(arm)
        if orch and which == "A" and mean_plan and not any(row["role"] == "plan" for row in rows):
            rows = rows + [mean_plan]
        by_role = {row["role"]: row for row in rows}
        if setup == "single":
            final = (by_role.get("all") or {}).get("answer") or {}
        elif setup == "parallel":
            final = {q: (by_role.get(q) or {}).get("answer") for q in ("q1", "q2", "q3")}
        else:
            final = (by_role.get("verify") or {}).get("answer") or {}
        scores = {q: grade(q, final.get(q), which) for q in ("q1", "q2", "q3")}
        workers = {q: grade(q, (by_role.get("w-" + q) or {}).get("answer"), which) for q in ("q1", "q2", "q3")}
        expected = 5 if orch else {"single": 1, "parallel": 3}[setup]
        # Complete means every agent of the cell ran to its structured answer: a
        # transcript still being written, or one that died, must not count as a run.
        answered = all(row.get("answer") is not None for row in rows if not row.get("imputed"))
        cell = dict(arm=arm, setup=setup, tier=tier, rep=rep, set=which, agents=measured,
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
        if is_team(arm):
            cell.update(team_cell(rows, final, which))
        table.append(cell)
    flag_banks(table)
    return table


def main() -> int:
    out, folders = pathlib.Path(sys.argv[1]), [pathlib.Path(p) for p in sys.argv[2:]]
    agents = read_folders(folders)
    table = build_cells(agents)
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
