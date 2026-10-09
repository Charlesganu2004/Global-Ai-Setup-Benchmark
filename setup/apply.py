#!/usr/bin/env python3
"""Install the global rules, the CaveCrew agents and the local-index hook for
Claude Code, Codex and GitHub Copilot.

    python apply.py [--client all|claude|codex|copilot] [--home DIR] [--dry-run]

Run it again after editing anything under rules/, agents/ or local-index/: every
step replaces only what this script owns and leaves the rest of each file alone.

Nothing is written until every file has been read, rendered and checked, so a
bad input stops the run with the machine exactly as it was. Files that are about
to change are first copied to ~/.local-index/backups/<timestamp>/.

What it owns:
  rules     the protected-rules block and the NO-COMPRESS block, between their
            markers, in each client's global instruction file. A Master Repo
            wrapper around the NO-COMPRESS block is removed.
  agents    cavecrew-investigator, cavecrew-builder, cavecrew-reviewer, rendered
            from one source per role into each client's own agent format.
  hook      ~/.local-index/session_index.py, registered at session start.
"""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import sys
import time

try:
    import tomllib
except ImportError:  # Python 3.10: the Codex agent files are written unchecked
    tomllib = None

HERE = pathlib.Path(__file__).resolve().parent
ROLES = ("cavecrew-investigator", "cavecrew-builder", "cavecrew-reviewer")
READ_ONLY = {"cavecrew-investigator", "cavecrew-reviewer"}

RULE_FILES = {
    "claude": ".claude/CLAUDE.md",
    "codex": ".codex/AGENTS.md",
    "copilot": ".copilot/copilot-instructions.md",
}
RULES = ("<!-- protected-rules-begin -->", "<!-- protected-rules-end -->")
NO_COMPRESS = ("<!-- NO-COMPRESS:BEGIN -->", "<!-- NO-COMPRESS:END -->")
MASTER_REPO = ("<!-- MASTER-REPO-USE:BEGIN -->", "<!-- MASTER-REPO-USE:END -->")
MASTER_REPO_PREFIXES = ("Master Repo path:", "Master Repo auto mode.")

# Claude Code resolves these aliases to the newest model of each tier, so the
# agent files never name a version. Codex and Copilot have no such aliases:
# there an agent with no model inherits the session's. Nothing is pinned for
# Codex at all, because a value in a Codex agent file outranks the spawn call
# and could never be raised for a hard lane.
CLAUDE_MODEL = {"cavecrew-investigator": "haiku", "cavecrew-builder": "sonnet",
                "cavecrew-reviewer": "inherit"}
# Tier C scouts think little, Tier A reviewers think as hard as the model allows.
# The builder takes the client's default effort.
CLAUDE_EFFORT = {"cavecrew-investigator": "low", "cavecrew-reviewer": "max"}
COPILOT_EFFORT = {"cavecrew-investigator": "low", "cavecrew-reviewer": "high"}
COPILOT_TOOLS = {"cavecrew-investigator": ["read", "search", "execute"],
                 "cavecrew-builder": ["read", "search", "edit", "execute"],
                 "cavecrew-reviewer": ["read", "search", "execute"]}
SKILL_DIRS = {"claude": ".claude/skills", "codex": ".codex/skills", "copilot": ".copilot/skills"}

HOOK_MARK = "session_index.py"
BOM = b"\xef\xbb\xbf"
# Characters that end or split a command in cmd.exe or PowerShell.
UNSAFE_IN_SHELL = set(" \t&()'$;,{}`^%!\"")


class Refused(Exception):
    """An input that cannot be installed safely. Nothing is written."""


class Run:
    """Collects every write, and commits them together or not at all."""

    def __init__(self, home: pathlib.Path, dry: bool):
        self.home, self.dry = home, dry
        self.staged: list[tuple[pathlib.Path, bytes, str]] = []
        self.notes: list[str] = []

    def stage(self, path: pathlib.Path, data: bytes, label: str) -> None:
        self.staged.append((path, data, label))

    def backup_dir(self) -> pathlib.Path:
        base = self.home / ".local-index" / "backups"
        stamp = time.strftime("%Y%m%d-%H%M%S")
        candidate, count = base / stamp, 1
        while candidate.exists():
            count += 1
            candidate = base / f"{stamp}-{count}"
        return candidate

    def commit(self) -> None:
        backups = None
        for path, data, label in self.staged:
            if path.is_file() and path.read_bytes() == data:
                self.notes.append(f"unchanged  {label}: {path}")
                continue
            self.notes.append(f"{'would write' if self.dry else 'wrote':<10} {label}: {path}")
            if self.dry:
                continue
            if path.is_file():
                backups = backups or self.backup_dir()
                backups.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, backups / path.relative_to(self.home).as_posix()
                             .replace("/", "__"))
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        if backups:
            self.notes.append(f"backups    {backups}")


def source(*parts: str) -> str:
    """A file from this folder. Missing or empty is an error, never an empty block."""
    path = HERE.joinpath(*parts)
    try:
        text = path.read_bytes().decode("utf-8-sig").replace("\r\n", "\n").strip("\n")
    except (OSError, UnicodeDecodeError) as error:
        raise Refused(f"cannot read {path}: {error}") from error
    if not text.strip():
        raise Refused(f"{path} is empty")
    return text


# --- rules ------------------------------------------------------------------

def rendered_rules(client: str) -> str:
    template = source("rules", "protected-rules.template.md")
    if template.count("{{CAVECREW}}") != 1:
        raise Refused("rules/protected-rules.template.md needs exactly one {{CAVECREW}}")
    block = template.replace("{{CAVECREW}}", source("rules", f"cavecrew.{client}.md"))
    if not (block.startswith(RULES[0]) and block.endswith(RULES[1])):
        raise Refused("the rules template must start and end with its markers")
    return block


def no_compress_block() -> str:
    block = source("rules", "no-compress.md")
    if not (block.startswith(NO_COMPRESS[0]) and block.endswith(NO_COMPRESS[1])):
        raise Refused("rules/no-compress.md must start and end with its markers")
    return block


def without_master_repo(text: str) -> str:
    """Take the Master Repo wrapper off the NO-COMPRESS block it surrounds.

    Only lines inside the wrapper are touched. When our own NO-COMPRESS block is
    already in the file, a wrapper is a copy the Master Repo installer added
    back, and it goes whole: two NO-COMPRESS blocks would be ambiguous.
    """
    begin, end = MASTER_REPO
    for _ in range(8):
        start = text.find(begin)
        stop = text.find(end, start) if start >= 0 else -1
        if start < 0 or stop < 0:
            break
        stop += len(end)
        outside = text[:start] + text[stop:]
        inner = [] if NO_COMPRESS[0] in outside else [
            line for line in text[start:stop].split("\n")
            if line.strip() not in MASTER_REPO and not line.startswith(MASTER_REPO_PREFIXES)]
        kept = "\n".join(inner).strip("\n")
        before, after = text[:start].rstrip("\n"), text[stop:].lstrip("\n")
        joined = [part for part in (before, kept) if part]
        text = "\n\n".join(joined) + ("\n" + after if after else "\n")
    return text


def upsert(text: str, markers: tuple[str, str], block: str, before: str = "") -> str:
    """Replace the marked block, or add it: ahead of `before` when that is present."""
    begin, end = markers
    if text.count(begin) != text.count(end) or text.count(begin) > 1:
        raise Refused(f"ambiguous {begin} markers")
    if begin in text:
        start, stop = text.index(begin), text.index(end) + len(end)
        if stop < start:
            raise Refused(f"reversed {begin} markers")
        return text[:start] + block + text[stop:]
    if before and before in text:
        at = text.index(before)
        return text[:at].rstrip("\n") + "\n\n" + block + "\n\n" + text[at:]
    return (text.rstrip("\n") + "\n\n" if text.strip() else "") + block + "\n"


def stage_rules(run: Run, client: str) -> None:
    path = run.home / RULE_FILES[client]
    raw = path.read_bytes() if path.is_file() else b""
    had_bom = raw.startswith(BOM)
    try:
        original = raw.decode("utf-8-sig")
    except UnicodeDecodeError as error:
        raise Refused(f"{path} is not UTF-8: {error}") from error
    newline = "\r\n" if "\r\n" in original else "\n"
    text = without_master_repo(original.replace("\r\n", "\n"))
    try:
        text = upsert(text, RULES, rendered_rules(client), before=NO_COMPRESS[0])
        text = upsert(text, NO_COMPRESS, no_compress_block())
    except Refused as error:
        raise Refused(f"{path}: {error}") from error
    data = text.replace("\n", newline).encode("utf-8")
    # Keep a byte-order mark the file already had: Windows PowerShell reads a
    # file without one as ANSI and would mangle it on its next rewrite.
    run.stage(path, (BOM if had_bom else b"") + data, f"{client} rules")


def copilot_env_notes() -> list[str]:
    """Report, never change: other instruction folders Copilot is told to load."""
    values = {os.environ.get("COPILOT_CUSTOM_INSTRUCTIONS_DIRS", "")}
    if os.name == "nt":
        try:
            import winreg
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
                values.add(str(winreg.QueryValueEx(key, "COPILOT_CUSTOM_INSTRUCTIONS_DIRS")[0]))
        except OSError:
            pass
    notes = []
    for folder in sorted({part.strip() for value in values for part in value.split(",")}):
        agents = pathlib.Path(folder) / "AGENTS.md" if folder else None
        try:
            wrapped = agents is not None and MASTER_REPO[0] in agents.read_text(
                encoding="utf-8", errors="replace")
        except OSError:
            wrapped = False
        if wrapped:
            notes.append(
                f"ACTION     Copilot also loads {agents} through the user variable "
                "COPILOT_CUSTOM_INSTRUCTIONS_DIRS, and that file still carries the Master Repo "
                "block. To stop it: [Environment]::SetEnvironmentVariable("
                "'COPILOT_CUSTOM_INSTRUCTIONS_DIRS',$null,'User') then open a new terminal.")
    return notes


# --- agents -----------------------------------------------------------------

def role_source(role: str) -> tuple[str, str]:
    head, _, body = source("agents", f"{role}.md").partition("\n---\n")
    if not head.startswith("description: ") or "\n" in head or not body.strip():
        raise Refused(f"agents/{role}.md needs one 'description: ' line, '---', then the prompt")
    return head[len("description: "):].strip(), body.strip("\n")


def front_matter(pairs: list[tuple[str, str]], description: str) -> str:
    lines = ["---"]
    for key, value in pairs:
        lines.append(f"{key}: {value}")
        if key == "name":
            lines += ["description: >", f"  {description}"]
    return "\n".join(lines + ["---"])


def stage_agents(run: Run, client: str) -> None:
    for role in ROLES:
        description, body = role_source(role)
        if client == "claude":
            pairs = [("name", role), ("model", CLAUDE_MODEL[role])]
            if role in CLAUDE_EFFORT:
                pairs.append(("effort", CLAUDE_EFFORT[role]))
            if role in READ_ONLY:
                pairs.append(("disallowedTools", "Edit, Write, NotebookEdit"))
            path = run.home / ".claude" / "agents" / f"{role}.md"
            text = front_matter(pairs, description) + "\n\n" + body + "\n"
        elif client == "copilot":
            pairs = [("name", role), ("tools", json.dumps(COPILOT_TOOLS[role]))]
            if role in COPILOT_EFFORT:
                pairs.append(("reasoningEffort", COPILOT_EFFORT[role]))
            path = run.home / ".copilot" / "agents" / f"{role}.agent.md"
            text = front_matter(pairs, description) + "\n\n" + body + "\n"
        else:
            if "'''" in body:
                raise Refused(f"agents/{role}.md may not contain ''' (TOML literal string)")
            # No `model` and no effort on purpose: the agent inherits the session's.
            text = "\n".join([f"name = {json.dumps(role)}",
                              f"description = {json.dumps(description)}",
                              "developer_instructions = '''", body, "'''"]) + "\n"
            if tomllib is not None:
                tomllib.loads(text)
            path = run.home / ".codex" / "agents" / f"{role}.toml"
        run.stage(path, text.encode("utf-8"), f"{client} agent")


# --- skills -----------------------------------------------------------------

def stage_skills(run: Run, client: str) -> None:
    """Install each packaged skill the client does not have yet.

    The rules name these skills, so a machine without them would be told to use
    something it cannot find. A skill that is already installed is never touched,
    whatever version it is: this only fills gaps.
    """
    packaged = HERE / "skills"
    if not packaged.is_dir():
        return
    for folder in sorted(path for path in packaged.iterdir() if (path / "SKILL.md").is_file()):
        target = run.home / SKILL_DIRS[client] / folder.name
        if (target / "SKILL.md").exists():
            continue
        for item in sorted(path for path in folder.rglob("*") if path.is_file()):
            run.stage(target / item.relative_to(folder), item.read_bytes(), f"{client} skill")


# --- local-index hook -------------------------------------------------------

def stage_index_script(run: Run) -> pathlib.Path:
    base = run.home / ".local-index"
    script = base / "session_index.py"
    body = source("local-index", "session_index.py") + "\n"
    compile(body, str(script), "exec")
    run.stage(script, body.encode("utf-8"), "index hook")
    if not (base / "config.json").exists():
        config = source("local-index", "config.json") + "\n"
        json.loads(config)
        run.stage(base / "config.json", config.encode("utf-8"), "index config")
    return script


def load_json(path: pathlib.Path) -> tuple[dict, str]:
    """A JSON config and its line ending. Unreadable or not an object is refused."""
    if not path.is_file():
        return {}, "\n"
    try:
        text = path.read_bytes().decode("utf-8-sig")
        data = json.loads(text) if text.strip() else {}
    except (OSError, ValueError) as error:
        raise Refused(f"{path} is not valid JSON, so it is left alone: {error}") from error
    if not isinstance(data, dict) or not isinstance(data.get("hooks", {}), dict):
        raise Refused(f"{path} does not hold a JSON object with a hooks object")
    return data, "\r\n" if "\r\n" in text else "\n"


def dump_json(data: dict, newline: str) -> bytes:
    text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    return text.replace("\n", newline).encode("utf-8")


def set_session_start(config: dict, group: dict) -> dict:
    """Add our SessionStart group, dropping only our own earlier handlers."""
    hooks = config.setdefault("hooks", {})
    kept = []
    for entry in hooks.get("SessionStart", []) or []:
        if isinstance(entry, dict) and isinstance(entry.get("hooks"), list):
            others = [handler for handler in entry["hooks"]
                      if HOOK_MARK not in json.dumps(handler)]
            if len(others) != len(entry["hooks"]):
                if not others:
                    continue
                entry = dict(entry, hooks=others)
        kept.append(entry)
    hooks["SessionStart"] = kept + [group]
    return config


def shell_safe(path: str) -> str:
    """The path, or its 8.3 short form, when one of them needs no quoting. Else ""."""
    if not UNSAFE_IN_SHELL & set(path):
        return path
    if os.name == "nt" and os.path.exists(path):
        import ctypes
        buffer = ctypes.create_unicode_buffer(1024)
        if ctypes.windll.kernel32.GetShortPathNameW(path, buffer, 1024):
            if not UNSAFE_IN_SHELL & set(buffer.value):
                return buffer.value
    return ""


def stage_hook(run: Run, client: str, script: pathlib.Path) -> None:
    python, target = pathlib.Path(sys.executable).as_posix(), script.as_posix()
    if client == "claude":
        # Exec form: no shell, so the same entry works under Git Bash and PowerShell.
        def handler(*args: str, **extra: object) -> dict:
            return {"type": "command", "command": python, "args": [target, *args], **extra}
        group = {"hooks": [handler("--client", "claude", timeout=10),
                           handler("--refresh", **{"async": True, "timeout": 900})]}
        path = run.home / ".claude" / "settings.json"
        config, newline = load_json(path)
        run.stage(path, dump_json(set_session_start(config, group), newline), "claude hook")
    elif client == "codex":
        # Codex runs this line through cmd.exe or PowerShell depending on the
        # session. No quoting works in both, so the paths must need none.
        exe = shell_safe(str(pathlib.PureWindowsPath(python)))
        home = shell_safe(str(pathlib.PureWindowsPath(run.home.as_posix())))
        if os.name == "nt" and not (exe and home):
            run.notes.append(
                "ACTION     codex hook not registered: the Python or home path needs quoting "
                "and has no short form. Rules and agents are installed; the index still "
                "refreshes when an agent runs the refresh command.")
            return
        windows = f"{exe} {home}\\.local-index\\session_index.py"
        def handler(flags: str, **extra: object) -> dict:
            return {"type": "command", "command": f"python3 '{target}' {flags}",
                    "commandWindows": f"{windows} {flags}", **extra}
        group = {"hooks": [handler("--client codex", timeout=10),
                           handler("--refresh", **{"async": True, "timeout": 900})]}
        path = run.home / ".codex" / "hooks.json"
        config, newline = load_json(path)
        run.stage(path, dump_json(set_session_start(config, group), newline), "codex hook")
    else:
        # Copilot has no background hooks, so the script detaches its own refresh.
        flags = "--client copilot --spawn"
        quoted = "'" + python.replace("'", "''") + "' '" + target.replace("'", "''") + "'"
        entry = {"type": "command", "bash": f"python3 '{target}' {flags}",
                 "powershell": f"& {quoted} {flags}", "timeoutSec": 15}
        path = run.home / ".copilot" / "hooks" / "local-index.json"
        run.stage(path, dump_json({"version": 1, "hooks": {"sessionStart": [entry]}}, "\n"),
                  "copilot hook")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--client", default="all", choices=("all", *RULE_FILES, "gpt"),
                        help="gpt is another name for codex")
    parser.add_argument("--home", type=pathlib.Path, default=pathlib.Path.home())
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    chosen = "codex" if args.client == "gpt" else args.client
    clients = tuple(RULE_FILES) if chosen == "all" else (chosen,)

    run = Run(args.home.resolve(), args.dry_run)
    try:
        script = stage_index_script(run)
        for client in clients:
            stage_rules(run, client)
            stage_agents(run, client)
            stage_skills(run, client)
            stage_hook(run, client, script)
    except (Refused, SyntaxError, ValueError) as error:
        print(f"REFUSED    {error}\nNothing was written.")
        return 1
    run.commit()
    if "copilot" in clients and run.home == pathlib.Path.home().resolve():
        run.notes += copilot_env_notes()
    print("\n".join(run.notes))
    return 1 if any(note.startswith(("REFUSED", "ACTION")) for note in run.notes) else 0


if __name__ == "__main__":
    sys.exit(main())
