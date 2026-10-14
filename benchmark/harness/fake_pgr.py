"""A stand-in for pgr, for the tests of prepare_packs.py and prepare_team.py. It is not the product.

It answers the commands the two scripts use (--version, status, refresh, pack, handoff, loadout, recall,
memory path) with the text the pgr 0.2 contract describes, from a small bank kept as JSON under
LXR_HOME/banks/NAME. A note is checked against the repository the way pgr checks it: the stand-in
hashes the source of every function a note names, and prints "stale: code changed" when the hash differs
from the one taken when the note was filed. Two notes whose text is the same are one note, seen twice.

Behaviour switches for the tests, set in the environment:
  FAKE_PGR_VERSION      the version it prints (default 0.2.0)
  FAKE_PGR_EMPTY_PACK   a text; a pack whose task contains it prints nothing
  FAKE_PGR_FAIL_PACK    a text; a pack whose task contains it exits with 1
  FAKE_PGR_IGNORE_NO_MODEL  set to 1: the stand-in ignores PGR_NO_MODEL, like a pgr that predates it
  FAKE_PGR_STRIP        a folder whose path is left off the front of a root when the index is looked up, so that
                        a copy of a virtual machine's folder (fake_wsl.py) keeps its index

PGR_NO_MODEL=1 (a pgr 0.2 feature) makes the real engine behave as if no embedding model existed, at index
build and at query. The stand-in does the same: the index remembers whether it was built with words only,
the status line says what the index was built with (not what the call sets) and a pack made with the variable
set differs from a pack made with the model.
"""
from __future__ import annotations

import ast
import hashlib
import json
import os
import pathlib
import re
import sys

NAME_RULE = re.compile(r"[A-Za-z0-9._-]{1,64}")
STATE = {"clock": 0}


def fail(message: str, code: int = 2) -> None:
    print(message, file=sys.stderr)
    sys.exit(code)


def home() -> pathlib.Path:
    return pathlib.Path(os.environ.get("LXR_HOME") or pathlib.Path.home() / ".local-index" / "rust")


def check_name(value: str, what: str) -> str:
    if not NAME_RULE.fullmatch(value) or value in (".", ".."):
        fail(f"pgr: {value} is not a valid {what}")
    return value


def definitions(root: pathlib.Path) -> dict[str, list[tuple[str, int, int, str, str]]]:
    """name -> [(path, first, last, signature, source)] for every function of the repository."""
    found: dict[str, list] = {}
    for path in sorted(root.rglob("*.py")):
        if ".git" in path.parts:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        lines = text.split("\n")
        try:
            tree = ast.parse(text)
        except SyntaxError:
            continue
        for node in ast.walk(tree):
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                params = ", ".join(a.arg for a in node.args.posonlyargs + node.args.args + node.args.kwonlyargs)
                source = "\n".join(lines[node.lineno - 1:node.end_lineno])
                found.setdefault(node.name, []).append((path.relative_to(root).as_posix(), node.lineno, node.end_lineno, f"({params})", source))
    return found


def code_names(text: str) -> list[str]:
    """Words that look like code: an underscore inside, or a name between backticks."""
    names = re.findall(r"`([A-Za-z_][A-Za-z0-9_.]*)`", text) + re.findall(r"\b[A-Za-z]+_[A-Za-z0-9_]+\b|\b_[A-Za-z0-9_]+\b", text)
    return list(dict.fromkeys(names))


def short(source: str) -> str:
    return hashlib.sha1(source.encode("utf-8")).hexdigest()[:12]


def tokens(text: str) -> int:
    return (len(text.encode("utf-8")) + 3) // 4


class Bank:
    def __init__(self, name: str):
        self.path = home() / "banks" / name / "notes.json"
        self.data = json.loads(self.path.read_text(encoding="utf-8")) if self.path.exists() else {"clock": 0, "notes": []}

    def save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.write_text(json.dumps(self.data, indent=1), encoding="utf-8")


def marker_path(root: pathlib.Path) -> pathlib.Path:
    text = str(root.resolve()).lower()
    strip = os.environ.get("FAKE_PGR_STRIP", "").lower()     # fake_wsl.py: the folder that stands for / inside the VM
    if strip and text.startswith(strip):
        text = text[len(strip):]
    return home() / "index" / hashlib.sha1(text.encode()).hexdigest()[:12]


def no_model() -> bool:
    return os.environ.get("PGR_NO_MODEL") == "1" and os.environ.get("FAKE_PGR_IGNORE_NO_MODEL") != "1"


def built_words_only(root: pathlib.Path) -> bool:
    """Whether the index was built with words only. The status line reports this, as the real pgr does: an index
    built with the model says "embeddings" even when PGR_NO_MODEL=1 is set for the call."""
    marker = marker_path(root)
    return marker.exists() and marker.read_text(encoding="utf-8") == "words"


def words_only(root: pathlib.Path) -> bool:
    """Whether a query works on words alone: PGR_NO_MODEL is set, or the index was built that way."""
    return no_model() or built_words_only(root)


def status_line(root: pathlib.Path) -> str:
    if not marker_path(root).exists():
        return f"pgr: {root.name} | no index yet (run: pgr refresh)"
    defs = definitions(root)
    search = "words only" if built_words_only(root) else "embeddings"
    return f"pgr: {root.name} | graph ready ({sum(len(v) for v in defs.values())} symbols, 0 links, 1s old) | search ready (10 chunks, {search}) | memory 0 notes."


def refresh(root: pathlib.Path) -> None:
    marker = marker_path(root)
    marker.parent.mkdir(parents=True, exist_ok=True)
    marker.write_text("words" if no_model() else "ready", encoding="utf-8")
    if no_model():
        print("pgr: no embedding model found, search uses words only", file=sys.stderr)
    print("pgr: index refreshed, 1 files changed, 1 ms", file=sys.stderr)


def pack(root: pathlib.Path, engines: list[str], task: str) -> str:
    for variable, effect in (("FAKE_PGR_FAIL_PACK", "fail"), ("FAKE_PGR_EMPTY_PACK", "empty")):
        marker = os.environ.get(variable)
        if marker and marker in task:
            if effect == "fail":
                fail("pgr: the pack could not be made", 1)
            return ""
    out = []
    defs = definitions(root) if "graph" in engines else {}
    words = words_only(root)
    seed = hashlib.sha1((",".join(engines) + task + ("|words" if words else "")).encode()).hexdigest()
    if "graph" in engines:
        blocks = []
        for name in code_names(task)[:4]:
            if len(defs.get(name, [])) == 1:
                path, first, last, signature, _ = defs[name][0]
                blocks.append(f"{name} {path}:{first}-{last} {signature}")
        if blocks:
            out += ["== names"] + blocks
    if "search" in engines:
        out += ["== find", f"hit_{seed[:6]} a.py:1-9 (x)", f"  a stand-in hit for variant {','.join(engines)}{' (words only)' if words else ''}"]
    if "graph" in engines:
        out += ["== graph", f"query: 2 nodes from {seed[6:12]}"]
    return "\n".join(out) + "\n" if out else "nothing found for this task\n"


def split_facts(text: str) -> list[str]:
    return [piece.strip() for piece in re.split(r"(?<=[.!?])\s+", text) if piece.strip()]


def note_line(note: dict, current: dict) -> tuple[str, str]:
    """(line, state): state is fresh, stale or gone, from the hashes taken when the note was filed."""
    state = "fresh"
    for name, taken in note["anchors"].items():
        now = current.get(name, [])
        if len(now) != 1:
            state = "gone"
            break
        if short(now[0][4]) != taken:
            state = "stale"
    brackets = ["0s ago"]
    if note["seen"] > 1:
        brackets.append(f"seen {note['seen']} times")
    if state != "fresh":
        brackets.append("stale: code changed" if state == "stale" else "gone: code removed")
    roles = "+".join(sorted(note["roles"]))
    return f"- {note['text']}  ({', '.join(brackets)})" + (f" [{roles}]" if roles else "") + f" #{note['id']}", state


def handoff(root: pathlib.Path, flags: dict, rest: list[str]) -> None:
    if not flags["task"]:
        fail("pgr: handoff needs --task NAME")
    whole, kind, tags, pieces, i = False, "world", [], [], 0
    while i < len(rest):
        if rest[i] == "--whole":
            whole = True
        elif rest[i] == "--kind":
            kind = rest[i + 1]
            i += 1
        elif rest[i] == "--tag":
            tags.append(rest[i + 1])
            i += 1
        else:
            pieces.append(rest[i])
        i += 1
    text = sys.stdin.buffer.read().decode("utf-8") if pieces == ["-"] else " ".join(pieces)
    facts = [text.strip()] if whole else split_facts(text)
    bank, defs = Bank(flags["bank"] or "default"), definitions(root)
    lines, naming = [], 0
    for fact in facts:
        key = re.sub(r"\s+", " ", fact.lower())
        names = [n for n in code_names(fact) if len(defs.get(n, [])) == 1]
        naming += bool(names)
        found = next((n for n in bank.data["notes"] if n["key"] == key), None)
        if found:
            found["seen"] += 1
            lines.append(f"reinforced {found['id']} (seen {found['seen']} times)")
        else:
            bank.data["clock"] += 1
            found = {"id": "n" + hashlib.sha1(key.encode()).hexdigest()[:6], "key": key, "text": fact, "kind": kind, "seen": 1,
                     "when": bank.data["clock"], "tags": [], "roles": [], "anchors": {n: short(defs[n][0][4]) for n in names}}
            bank.data["notes"].append(found)
            lines.append(f"kept {found['id']}")
        found["tags"] = sorted(set(found["tags"]) | {f"task:{flags['task']}", f"from:{flags['from']}", *tags})
        found["roles"] = sorted(set(found["roles"]) | {flags["from"]})
    bank.save()
    print("\n".join(lines))
    print(f"handoff {flags['task']}: {len(facts)} notes from {flags['from']}, {naming} name code")


def loadout(root: pathlib.Path, flags: dict, rest: list[str]) -> None:
    # The stand-in never cuts to a budget, so --budget and its value are only skipped.
    skipped = {rest.index("--budget"), rest.index("--budget") + 1} if "--budget" in rest else set()
    words = [w for at, w in enumerate(rest) if at not in skipped]
    bank, defs = Bank(flags["bank"] or "default"), definitions(root)
    notes = sorted(bank.data["notes"], key=lambda n: -n["when"])
    printed: set[str] = set()
    counts = {"fresh": 0, "stale": 0, "gone": 0}
    sections: list[str] = []

    def layer(title: str, chosen: list[dict]) -> None:
        if not chosen:
            return
        rows = []
        for note in chosen:
            line, state = note_line(note, defs)
            rows.append((line, state, note))
        order = {"fresh": 0, "stale": 1, "gone": 2}
        rows.sort(key=lambda row: order[row[1]]) if title.startswith("== task") else None
        sections.append(title)
        for line, state, note in rows:
            sections.append(line)
            counts[state] += 1
            printed.add(note["id"])

    layer("== core", [n for n in notes if n["kind"] == "directive"] +
          [n for n in notes if n["kind"] != "directive" and (n["seen"] >= 3 or len(n["roles"]) >= 2)])
    if flags["task"]:
        layer(f"== task {flags['task']}", [n for n in notes if f"task:{flags['task']}" in n["tags"] and n["id"] not in printed])
    if words:
        layer("== related", [n for n in notes if n["id"] not in printed and any(w.lower() in n["text"].lower() for w in words)][:4])
    if not sections:
        print("no notes")
        return
    body = "\n".join(sections)
    total = sum(counts.values())
    print(body)
    print(f"loadout: {total} notes, {counts['fresh']} fresh, {counts['stale']} stale, {counts['gone']} gone, about {tokens(body)} tokens")
    if counts["stale"] + counts["gone"]:
        print("check stale and gone notes against the code before relying on them")


def main(argv: list[str]) -> int:
    # pgr prints UTF-8 with line feeds only, on Windows too.
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    sys.stderr.reconfigure(encoding="utf-8", newline="\n")
    if argv and argv[0] in ("--version", "-V"):
        print("pgr " + os.environ.get("FAKE_PGR_VERSION", "0.2.0"))
        return 0
    flags = {"root": None, "engines": os.environ.get("PGR_ENGINES") or "graph,search,memory",
             "bank": os.environ.get("PGR_BANK"), "task": os.environ.get("PGR_TASK"), "from": os.environ.get("PGR_ROLE") or "agent"}
    i = 0
    while i < len(argv) and argv[i] in ("--root", "--engines", "--bank", "--task", "--from"):
        flags[argv[i][2:]] = argv[i + 1]
        i += 2
    rest = argv[i:]
    if flags["bank"] is not None:
        check_name(flags["bank"], "bank name")
    if flags["task"] is not None:
        check_name(flags["task"], "task name")
    check_name(flags["from"], "role name")
    root = pathlib.Path(flags["root"] or ".").resolve()
    engines = flags["engines"].split(",")
    if not rest:
        fail("usage: pgr [--root DIR] COMMAND ...")
    command, rest = rest[0], rest[1:]
    if command == "status":
        print(status_line(root))
    elif command == "refresh":
        refresh(root)
        print(status_line(root))
    elif command == "pack":
        sys.stdout.write(pack(root, engines, rest[0]))
    elif command == "handoff":
        handoff(root, flags, rest)
    elif command == "loadout":
        loadout(root, flags, rest)
    elif command == "recall":
        bank = Bank(flags["bank"] or "default")
        hits = [n for n in bank.data["notes"] if any(w.lower() in n["text"].lower() for w in rest)]
        defs = definitions(root)
        print("\n".join(note_line(n, defs)[0] for n in hits) if hits else "no notes")
    elif command == "memory" and rest[:1] == ["path"]:
        print(home() / "banks" / (flags["bank"] or "default"))
    else:
        fail(f"pgr: unknown command {command}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
