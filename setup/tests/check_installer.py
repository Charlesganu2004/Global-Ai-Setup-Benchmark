"""Checks for the installer, apply.py.

    python check_installer.py PATH/TO/setup EMPTY_WORK_DIR ORIGINALS_DIR

ORIGINALS_DIR holds copies of real client files to install over, named
claude-CLAUDE.md, claude-settings.json, codex-AGENTS.md and
copilot-copilot-instructions.md. They are personal files, so none are kept in
this repository: point this at copies of your own. Every run happens in
throwaway home directories under the work directory; your real files are never
written.
"""
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys

SETUP = pathlib.Path(sys.argv[1])
WORK = pathlib.Path(sys.argv[2])
BACKUP = pathlib.Path(sys.argv[3])
PY = sys.executable
results = []


def check(name, ok, detail=""):
    results.append((name, bool(ok)))
    print(("PASS " if ok else "FAIL ") + name + (" | " + str(detail) if detail else ""))


def apply(home, *flags, setup=SETUP):
    done = subprocess.run([PY, str(setup / "apply.py"), "--home", str(home), *flags], capture_output=True, timeout=300)
    return done.returncode, done.stdout.decode("utf-8", "replace") + done.stderr.decode("utf-8", "replace")


def snapshot(home):
    return {str(p.relative_to(home)): hashlib.sha1(p.read_bytes()).hexdigest() for p in sorted(home.rglob("*")) if p.is_file()}


def seed(name):
    home = WORK / name
    for folder in (".claude", ".codex", ".copilot"):
        (home / folder).mkdir(parents=True, exist_ok=True)
    shutil.copy2(BACKUP / "claude-CLAUDE.md", home / ".claude" / "CLAUDE.md")
    shutil.copy2(BACKUP / "claude-settings.json", home / ".claude" / "settings.json")
    shutil.copy2(BACKUP / "codex-AGENTS.md", home / ".codex" / "AGENTS.md")
    shutil.copy2(BACKUP / "copilot-copilot-instructions.md", home / ".copilot" / "copilot-instructions.md")
    return home


WORK.mkdir(parents=True, exist_ok=True)

# 1. The real originals.
home = seed("originals")
bom_before = (home / ".claude" / "CLAUDE.md").read_bytes()[:3]
code, out = apply(home)
claude = (home / ".claude" / "CLAUDE.md").read_bytes()
text = claude.decode("utf-8-sig")
check("originals: applies cleanly", code == 0, out.strip().splitlines()[-1][:80])
# Whatever the original had: a byte-order mark stays, and none is added.
check("originals: byte-order mark kept as it was", (claude[:3] == b"\xef\xbb\xbf") == (bom_before == b"\xef\xbb\xbf"))
check("originals: no Master Repo lines, one of each block",
      "Master Repo" not in text.split("# PROTECTED RULES")[1] and "MASTER-REPO-USE" not in text
      and text.count("protected-rules-begin") == 1 and text.count("NO-COMPRESS:BEGIN") == 1)
head = text.split("<!-- protected-rules-begin -->")[0]
original_head = (BACKUP / "claude-CLAUDE.md").read_bytes().decode("utf-8-sig").replace("\r\n", "\n").split("<!-- MASTER-REPO-USE:BEGIN -->")[0]
check("originals: everything above the blocks is byte-identical", head.replace("\r\n", "\n").rstrip("\n") == original_head.rstrip("\n"))
original_text = (BACKUP / "claude-CLAUDE.md").read_bytes().decode("utf-8-sig").replace("\r\n", "\n")
last_line = [line for line in original_text.split("<!-- MASTER-REPO-USE:END -->")[-1].splitlines() if line.strip()][-1:]
check("originals: what followed the blocks still ends the file", not last_line or text.rstrip().endswith(last_line[0].rstrip()))
snap = snapshot(home)
code, out = apply(home)
check("second run changes nothing", snapshot(home) == snap and "wrote" not in out, out.count("unchanged"))

# 2. A missing source file refuses and writes nothing.
broken = WORK / "broken-setup"
shutil.copytree(SETUP, broken, ignore=shutil.ignore_patterns("backup-*", "__pycache__", "rendered", "INSTALL.txt"))
(broken / "rules" / "no-compress.md").unlink()
home2 = seed("missing-source")
snap2 = snapshot(home2)
code, out = apply(home2, setup=broken)
check("missing source: refused, nothing written", code == 1 and "REFUSED" in out and snapshot(home2) == snap2, out.strip()[:90])

# 3. Invalid settings.json refuses and writes nothing.
home3 = seed("bad-json")
(home3 / ".claude" / "settings.json").write_text('{"hooks": {"PreToolUse": [', encoding="utf-8")
snap3 = snapshot(home3)
code, out = apply(home3)
check("invalid settings.json: refused, nothing written", code == 1 and "REFUSED" in out and snapshot(home3) == snap3, out.strip()[:90])

# 4. Master Repo installer puts its wrapper back: handled, not refused.
path = home / ".copilot" / "copilot-instructions.md"
wrapper = ("\n\n<!-- MASTER-REPO-USE:BEGIN -->\nMaster Repo path: C:\\x\nMaster Repo auto mode. Load it.\n\n"
           "<!-- NO-COMPRESS:BEGIN -->\nstale copy\n<!-- NO-COMPRESS:END -->\n<!-- MASTER-REPO-USE:END -->\n")
path.write_text(path.read_text(encoding="utf-8") + wrapper, encoding="utf-8")
code, out = apply(home, "--client", "copilot")
after = path.read_text(encoding="utf-8")
check("wrapper re-added: removed whole, one block left", code == 0 and after.count("NO-COMPRESS:BEGIN") == 1 and "stale copy" not in after and "Master Repo" not in after.split("# PROTECTED RULES")[0] and "MASTER-REPO-USE" not in after, out.strip()[:60])

# 5. A neighbour's handler in the same SessionStart group survives; ours is not duplicated.
settings = home / ".claude" / "settings.json"
data = json.loads(settings.read_text(encoding="utf-8"))
data["hooks"]["SessionStart"][0]["hooks"].insert(0, {"type": "command", "command": "echo neighbour"})
settings.write_text(json.dumps(data, indent=2), encoding="utf-8")
apply(home, "--client", "claude")
groups = json.loads(settings.read_text(encoding="utf-8"))["hooks"]["SessionStart"]
flat = [json.dumps(h) for g in groups for h in g["hooks"]]
check("neighbour handler kept, ours present once each", sum("neighbour" in h for h in flat) == 1 and sum("session_index.py" in h for h in flat) == 2, len(flat))

# 6. A "Master Repo path:" line outside any wrapper is the owner's and stays.
home6 = seed("stray-line")
p6 = home6 / ".codex" / "AGENTS.md"
p6.write_text("Master Repo path: keep me, I am a note\n\n" + p6.read_text(encoding="utf-8"), encoding="utf-8")
apply(home6, "--client", "codex")
check("stray 'Master Repo path:' line outside the wrapper kept", p6.read_text(encoding="utf-8").startswith("Master Repo path: keep me"))

# 7. CRLF hook file keeps CRLF.
home7 = seed("crlf")
(home7 / ".codex" / "hooks.json").write_bytes(b'{\r\n  "hooks": {}\r\n}\r\n')
apply(home7, "--client", "codex")
raw = (home7 / ".codex" / "hooks.json").read_bytes()
check("CRLF hooks.json stays CRLF", b"\r\n" in raw and b"\n" not in raw.replace(b"\r\n", b""))

# 8. A home path with a space: the Codex line runs in cmd.exe and in PowerShell.
home8 = seed("Jane Doe")
code, out = apply(home8, "--client", "codex")
hooks = json.loads((home8 / ".codex" / "hooks.json").read_text(encoding="utf-8"))["hooks"]["SessionStart"][0]["hooks"]
line = hooks[0]["commandWindows"]
payload = json.dumps({"cwd": str(WORK)}).encode()
runs = []
for shell in (["cmd.exe", "/d", "/c", line], ["powershell.exe", "-NoProfile", "-Command", line]):
    done = subprocess.run(shell, input=payload, capture_output=True, timeout=120)
    runs.append((done.returncode, done.stderr.decode("utf-8", "replace").strip()[:80]))
check("space in home: Codex hook line runs under cmd.exe and PowerShell", all(code == 0 for code, _ in runs), "%s | %s" % (line[-70:], runs))

# 9. Dry run writes nothing at all.
home9 = seed("dry")
snap9 = snapshot(home9)
code, out = apply(home9, "--dry-run")
check("dry run: nothing written, no folders made", snapshot(home9) == snap9 and not (home9 / ".local-index").exists(), out.count("would write"))

# 10. Rendered agents parse.
import tomllib  # noqa: E402
toml = tomllib.loads((home / ".codex" / "agents" / "cavecrew-investigator.toml").read_text(encoding="utf-8"))
check("codex agent: required keys, nothing pinned", {"name", "description", "developer_instructions"} <= set(toml) and "model" not in toml and "model_reasoning_effort" not in toml)
front = (home / ".claude" / "agents" / "cavecrew-builder.md").read_text(encoding="utf-8").split("---")[1]
check("claude builder defaults to the coder alias", "model: sonnet" in front, front.strip().splitlines()[-1])

failed = [name for name, ok in results if not ok]
print("\n%d checks, %d failed%s" % (len(results), len(failed), ": " + "; ".join(failed) if failed else ""))
sys.exit(1 if failed else 0)
