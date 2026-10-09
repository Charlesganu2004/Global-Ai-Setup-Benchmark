"""Checks for the session hook and the lx command.

    python check_hook.py PATH/TO/session_index.py EMPTY_WORK_DIR

Builds small throwaway repositories under the work directory and drives the
script the way each client does. Needs graphify, semble and codanna installed.
"""
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

SCRIPT = pathlib.Path(sys.argv[1])
WORK = pathlib.Path(sys.argv[2])
PY = sys.executable
# The graphify the hook itself would find: on PATH first, then this setup's own environment.
GRAPHIFY = shutil.which("graphify") or str(
    pathlib.Path.home() / ".graphify" / "venv" / ("Scripts" if os.name == "nt" else "bin")
    / ("graphify.exe" if os.name == "nt" else "graphify"))
results = []


def check(name, ok, detail=""):
    results.append((name, bool(ok)))
    print(("PASS " if ok else "FAIL ") + name + (" | " + detail if detail else ""))


def sh(argv, cwd=None, data=None, env=None):
    done = subprocess.run(argv, cwd=cwd, input=data, env=env, capture_output=True, timeout=600)
    return done.returncode, done.stdout.decode("utf-8", "replace"), done.stderr.decode("utf-8", "replace")


def make_repo(name, files, commit=True):
    root = WORK / name
    if root.exists():
        shutil.rmtree(root, ignore_errors=True)
    root.mkdir(parents=True)
    for rel, text in files.items():
        target = root / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
    sh(["git", "init", "-q"], cwd=root)
    if commit:
        sh(["git", "add", "-A"], cwd=root)
        sh(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init", "--no-verify"], cwd=root)
    return root


def hook(*flags, cwd=None, payload=None):
    data = json.dumps({"cwd": str(payload)}).encode() if payload else None
    return sh([PY, str(SCRIPT), *flags], cwd=cwd, data=data)


BASIC = {"app.py": "from pkg.auth import check_token\n\ndef handle(r):\n    return check_token(r)\n",
         "pkg/auth.py": "def check_token(t):\n    return bool(t)\n"}
WORK.mkdir(parents=True, exist_ok=True)

# 1. Fortran include: control shows the leak exists, the hook must not reproduce it.
secret = WORK / "secret_host_file.h"
secret.write_text("      subroutine host_only_secret_4711()\n      end subroutine\n", encoding="utf-8")
fortran = '#include "%s"\n      program main\n      end program main\n' % secret.as_posix()
control = make_repo("f-control", dict(BASIC))
(control / "solver.F90").write_text(fortran, encoding="utf-8")
sh([GRAPHIFY, "extract", str(control), "--code-only", "--no-viz", "--no-cluster"], cwd=str(WORK))
leak_control = "host_only_secret_4711" in (control / "graphify-out" / "graph.json").read_text(encoding="utf-8", errors="replace")
guarded = make_repo("f-guarded", dict(BASIC))
(guarded / "solver.F90").write_text(fortran, encoding="utf-8")          # untracked on purpose
code, out, err = hook("--refresh", str(guarded))
graph = guarded / "graphify-out" / "graph.json"
leak_hook = graph.is_file() and "host_only_secret_4711" in graph.read_text(encoding="utf-8", errors="replace")
check("fortran control really leaks without the guard", leak_control, "cpp=%s" % shutil.which("cpp"))
check("fortran: hook builds the graph and nothing is inlined", graph.is_file() and not leak_hook, out.strip()[:120])

# 2. Non-ASCII repository name prints a line for claude and codex.
uni = make_repo("répo язык", dict(BASIC))
code, out, err = hook("--client", "claude", payload=uni)
check("non-ASCII name: status line printed", code == 0 and out.startswith("local-index:") and "répo" in out, repr(out[:60]))

# 3. exclude entry is written once even when graphify-out is gone at refresh time.
once = make_repo("exclude-once", dict(BASIC))
for _ in range(3):
    hook("--refresh", str(once))
    shutil.rmtree(once / "graphify-out", ignore_errors=True)
lines = (once / ".git" / "info" / "exclude").read_text(encoding="utf-8").splitlines()
check("exclude: one graphify-out entry after 3 refreshes", lines.count("graphify-out/") == 1, str(lines.count("graphify-out/")))
code, out, _ = sh(["git", "status", "--short"], cwd=once)
check("exclude: working tree stays clean", out.strip() == "", out.strip())

# 4. A graph committed to git is never rewritten.
tracked = make_repo("tracked-graph", dict(BASIC), commit=False)
(tracked / "graphify-out").mkdir()
(tracked / "graphify-out" / "graph.json").write_text('{"nodes": [{"id": "a"}], "edges": []}', encoding="utf-8")
sh(["git", "add", "-A"], cwd=tracked)
sh(["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init", "--no-verify"], cwd=tracked)
code, out, _ = hook("--refresh", str(tracked))
_, dirty, _ = sh(["git", "status", "--short"], cwd=tracked)
check("tracked graph: untouched and reported", dirty.strip() == "" and "tracked in git" in out, out.strip()[:110])

# 5. A hand-built clustered graph is kept.
curated = make_repo("curated-graph", dict(BASIC))
(curated / "graphify-out").mkdir()
(curated / "graphify-out" / "graph.json").write_text('{"nodes": [{"id": "a", "community": 3}], "edges": []}', encoding="utf-8")
(curated / "graphify-out" / "GRAPH_REPORT.md").write_text("# report\n", encoding="utf-8")
before = (curated / "graphify-out" / "graph.json").read_bytes()
code, out, _ = hook("--refresh", str(curated))
check("hand-built graph: untouched and reported", (curated / "graphify-out" / "graph.json").read_bytes() == before and "hand-built" in out, out.strip()[:110])

# 6. A state file that is JSON but not an object does not wedge the repository.
wedge = make_repo("wedge", dict(BASIC))
hook("--refresh", str(wedge))
sys.path.insert(0, str(SCRIPT.parent))
import session_index as si  # noqa: E402
si.state_path(wedge.resolve()).write_text("[1, 2, 3]", encoding="utf-8")
code, out, _ = hook("--client", "copilot", "--spawn", payload=wedge)
try:
    obj = json.loads(out)
    both = obj["additionalContext"] == obj["hookSpecificOutput"]["additionalContext"] and obj["hookSpecificOutput"]["hookEventName"] == "SessionStart"
except Exception as error:  # noqa: BLE001
    obj, both = None, False
check("corrupt state: copilot still gets one JSON object, both shapes", both, out.strip()[:90])
si.state_path(wedge.resolve()).write_text('{"refreshed_at": "soon"}', encoding="utf-8")
code, out, _ = hook("--client", "claude", payload=wedge)
check("non-numeric refreshed_at: line still printed", out.startswith("local-index:"), out.strip()[:70])

# 7. Programs planted in the session directory are never run.
planted = make_repo("planted", dict(BASIC))
marker = WORK / "planted-ran.txt"
if marker.exists():
    marker.unlink()
for name in ("git.cmd", "git.bat", "graphify.cmd", "semble.cmd", "cpp.cmd", "codanna.cmd", "codanna.bat"):
    (planted / name).write_text('@echo ran ' + name + '>>"' + str(marker) + '"\r\n', encoding="ascii")
env = {k: v for k, v in os.environ.items() if k != "NoDefaultCurrentDirectoryInExePath"}
sh([PY, str(SCRIPT), "--client", "claude"], cwd=planted, data=json.dumps({"cwd": str(planted)}).encode(), env=env)
sh([PY, str(SCRIPT), "--refresh", str(planted)], cwd=planted, env=env)
check("planted programs in the session dir never ran", not marker.exists(), marker.read_text() if marker.exists() else "")

# 8. Explicit refresh prints the status line; hook refresh prints nothing.
plain = make_repo("plain", dict(BASIC))
code, out, _ = hook("--refresh", ".", cwd=plain)
check("--refresh . prints the status line", out.startswith("local-index: plain | symbols ready") and "graph ready" in out and "semble ready" in out, out.strip()[:120])
code, out, _ = hook("--refresh", payload=plain)
check("hook --refresh (payload) prints nothing", out.strip() == "", repr(out[:40]))
code, out, _ = hook("--status", payload=WORK.parent.parent)
check("outside a repository: silent", out.strip() == "", repr(out[:40]))

# 9. Speed of the synchronous call.
laps = []
for _ in range(5):
    start = time.time()
    hook("--client", "claude", payload=plain)
    laps.append(time.time() - start)
# The fastest of five: a busy machine slows a lap, it never speeds one up.
check("status call under 0.6 s", min(laps) < 0.6, "%.0f ms" % (min(laps) * 1000))

# 10. The lx front-end: Rust lookups, lean output, safe fallbacks.
def lx(*args, cwd, env=None):
    return sh([PY, str(SCRIPT), *args], cwd=cwd, env=env)

code, out, _ = lx("def", "check_token", cwd=plain / "pkg")
check("lx def: one line, name path:first-last signature", out.strip() == "check_token pkg/auth.py:1-2 (t)", repr(out.strip()))
code, out, _ = lx("callers", "check_token", cwd=plain)
check("lx callers: a count, then the caller with its line range", out.strip().split("\n") == ["1 caller of check_token:", "handle app.py:3-4"], repr(out.strip()))
code, out, _ = lx("def", "no_such_symbol", cwd=plain)
check("lx def: unknown name says so", out.strip() == "no_such_symbol: not found", repr(out.strip()))
code, out, _ = lx("find", "check a token", "-k", "2", cwd=plain)
check("lx find: path:first-last, then name:line", all(re.fullmatch(r"[\w./-]+:\d+-\d+( \w+:\d+(, \w+:\d+)*)?", row) for row in out.strip().split("\n")) and "pkg/auth.py:1-2 check_token:1" in out, repr(out.strip()))
code, out, _ = lx("find", "--cards", "check a token", "-k", "2", cwd=plain)
check("lx find --cards: hits as definitions", "check_token pkg/auth.py:1-2 (t)" in out.split("\n"), repr(out.strip()))
code, out, _ = lx("find", "--hybrid", "check_token: check a token --now", "-k", "3", cwd=plain)
check("lx find --hybrid: merged search survives option-like words, hits as definitions", "check_token pkg/auth.py:1-2 (t)" in out.split("\n"), repr(out.strip()))
code, out, _ = lx("about", "check_token", cwd=plain)
check("lx about: definition, then its callers", out.rstrip().split("\n") == ["check_token pkg/auth.py:1-2 (t)", "  1 caller:", "    handle app.py:3-4"], repr(out.strip()))
code, out, _ = lx("ask", "def:check_token", "callers:check_token", cwd=plain)
check("lx ask: each answer under its own header", out.rstrip().split("\n") == ["== def:check_token", "check_token pkg/auth.py:1-2 (t)", "== callers:check_token", "1 caller of check_token:", "handle app.py:3-4"], repr(out.strip()))
code, out, _ = lx("ask", "nonsense:check_token", cwd=plain)
check("lx ask: an unknown kind is refused with usage", code == 2 and out.startswith("usage: lx ask"), repr(out.strip()[:60]))
code, out, _ = lx("ask", "def:--config=C:/elsewhere.toml", "callers:-x", cwd=plain)
check("lx ask: a name that looks like an option is never handed to an engine", out.rstrip().split("\n") == ["== def:--config=C:/elsewhere.toml", "--config=C:/elsewhere.toml: not found", "== callers:-x", "-x: not found"], repr(out.strip()))
code, out, _ = lx("ask", "find:--help check a token", cwd=plain)
check("lx ask: option-like words in a description are searched as words", code == 0 and "check_token pkg/auth.py:1-2 (t)" in out.split("\n"), repr(out.strip()))
code, out, _ = lx("callers", "a&b|c", cwd=plain)
check("lx callers: shell metacharacters are just a name", out.strip() == "a&b|c: not found", repr(out.strip()))
code, out, _ = lx("def", "x", cwd=WORK.parent.parent)
check("lx outside a repository: says so", out.strip() == "not in a git repository", repr(out.strip()))

# 11. Two definitions with one name: both are answered, each labelled.
twins = make_repo("twins", {"a.py": "def load(x):\n    return x\n\ndef use_a():\n    return load(1)\n",
                            "b.py": "def load(y):\n    return y\n\ndef use_b():\n    return load(2)\n"})
hook("--refresh", str(twins))
code, out, _ = lx("def", "load", cwd=twins)
check("lx def: both definitions listed", sorted(out.split("\n")[:2]) == ["load a.py:1-2 (x)", "load b.py:1-2 (y)"], repr(out.strip()))

# 12. A repository that ships its own codanna settings cannot redirect the index.
import re as _re
hostile = make_repo("hostile-settings", dict(BASIC))
(hostile / ".codanna").mkdir()
(hostile / ".codanna" / "settings.toml").write_text('version = 1\nindex_path = ".codanna/index"\n[semantic_search]\nenabled = true\nremote_url = "http://203.0.113.9:11434"\n', encoding="utf-8")
start = time.time()
code, out, _ = hook("--refresh", str(hostile))
check("hostile .codanna/settings.toml: ignored, index kept outside the repo",
      "symbols ready" in out and not (hostile / ".codanna" / "index").exists() and time.time() - start < 60, out.strip()[:90])

# 13. With the Rust engine switched off, lx still answers from the graph.
import tempfile
alt = pathlib.Path(tempfile.mkdtemp(prefix="lxhome", dir=str(WORK)))
(alt / ".local-index").mkdir()
(alt / ".local-index" / "config.json").write_text('{"codanna": false}', encoding="utf-8")
env2 = dict(os.environ, USERPROFILE=str(alt), HOME=str(alt))
fallback = make_repo("fallback", dict(BASIC))
sh([PY, str(SCRIPT), "--refresh", str(fallback)], env=env2)
code, out, _ = lx("def", "check_token", cwd=fallback, env=env2)
check("codanna off: lx def answers from graphify", out.strip().startswith("check_token pkg/auth.py:"), repr(out.strip()))
code, out, _ = lx("callers", "check_token", cwd=fallback, env=env2)
check("codanna off: lx callers answers from graphify", out.strip().split("\n")[0] == "1 caller of check_token:" and out.strip().split("\n")[1].startswith("handle app.py:"), repr(out.strip()))
code, out, _ = lx("status", cwd=fallback, env=env2)
check("codanna off: status says symbols absent", "symbols absent" in out, out.strip()[:80])

# 14. A lookup reads the index as it was built. Only a refresh may rebuild it,
# so changing the embeddings option must never empty an index mid-lookup.
import importlib.util
spec = importlib.util.spec_from_file_location("session_index_under_test", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.BASE = WORK / "fake-base"
module.STATE = module.BASE / "state"
probe = WORK / "probe-repo"
folder = module.BASE / "codanna" / module.state_path(probe).stem
(folder / "index").mkdir(parents=True, exist_ok=True)
(folder / "index" / "marker").write_text("kept", encoding="utf-8")
(folder / "settings.toml").write_text(module.CODANNA_SETTINGS.format(
    index=json.dumps(str(folder / "index")), root=json.dumps(str(probe)), semantic="true"), encoding="utf-8")
(folder / "layout").write_text(module.CODANNA_LAYOUT + "+embeddings", encoding="utf-8")
check("lookup: an index built with embeddings is read as it is", module.lookup_settings(probe) == folder / "settings.toml" and module.built_with_embeddings(probe) is True)
check("lookup: its files are still there afterwards", (folder / "index" / "marker").read_text(encoding="utf-8") == "kept")
(folder / "layout").write_text(module.CODANNA_LAYOUT, encoding="utf-8")
check("lookup: settings that disagree with how the index was built are not trusted", module.lookup_settings(probe) is None)
(folder / "layout").write_text("0", encoding="utf-8")
check("lookup: an index from an older layout is not trusted", module.lookup_settings(probe) is None and module.built_with_embeddings(probe) is None)
# And when it is not trusted, a lookup leaves it exactly as it is: it answers
# as if there were no Rust index, and deleting or re-stamping is refresh's job.
started = []
module.run = lambda argv, **options: started.append(argv) or (0, "")
tools = module.Tools(dict(module.DEFAULTS, codanna_cmd=sys.executable))
answer = module.codanna(tools, probe, "retrieve", "symbol", "x")
check("lookup: an untrusted index is neither read, emptied nor re-stamped",
      answer == (127, "") and not started and (folder / "layout").read_text(encoding="utf-8") == "0"
      and (folder / "index" / "marker").read_text(encoding="utf-8") == "kept", repr(answer))
fresh = WORK / "never-indexed"
check("lookup: a repository never refreshed gets no index folder",
      module.codanna(tools, fresh, "retrieve", "symbol", "x") == (127, "")
      and not (module.BASE / "codanna" / module.state_path(fresh).stem).exists())
check("ask: a selector is not a name", module.answer("def", "symbol_id:7", tools, probe) == ["symbol_id:7: not found"] and not started)
later = {"embed_retry_after": time.time() + 3600}
check("embeddings: the retry date survives a failed build", module.keep_retry({"ok": False}, later).get("embed_retry_after") == later["embed_retry_after"]
      and "embed_retry_after" not in module.keep_retry({"ok": False}, {"embed_retry_after": time.time() - 5}))
check("prune: a folder on a drive that is not attached is not gone", not module.gone_for_good("Q:\\no-such-drive\\repo") and not module.gone_for_good("relative/path")
      and module.gone_for_good(str(WORK / "definitely-not-here")) and not module.gone_for_good(str(WORK)))

failed = [name for name, ok in results if not ok]
print("\n%d checks, %d failed%s" % (len(results), len(failed), ": " + "; ".join(failed) if failed else ""))
sys.exit(1 if failed else 0)
