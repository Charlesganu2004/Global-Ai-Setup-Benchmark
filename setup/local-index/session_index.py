#!/usr/bin/env python3
"""Keep the local code indexes fresh, and answer lookups from them in one line per hit.

Three engines, each used for what it does best:
  codanna   Rust symbol and call index. "Where is X defined", "who calls X".
  semble    local embeddings plus BM25. "Where is the code about Y".
  graphify  tree-sitter code graph. Blast radius, paths between symbols, hubs.

As a session-start hook for Claude Code, Codex and GitHub Copilot it finds the
git repository a session opened in, refreshes all three in parallel and prints
one short status line. It never blocks a session and never fails one: every
path exits 0, and outside a repository it prints nothing.

    session_index.py --client claude|codex|copilot [--spawn]   hook entry
    session_index.py --refresh [ROOT]                          hook refresh

As the `lx` command it is the short front-end agents type:

    lx def NAME...         where NAME is defined      name path:first-last signature
    lx callers NAME...     what calls NAME            "N callers of NAME:", then
                                                      name path:first-last
    lx about NAME...       all of the above at once   the definition line, its
                                                      doc line, then its callers
    lx find "words" [-k N] [--docs|--all]             path:first-last name:line
                           the lines that match, then the definitions they sit
                           in and the line each of those starts on
    lx find "words" --cards                           name path:first-last signature
                           each hit as the definition it sits in, then its doc line
    lx find "words" --rust | --hybrid
                           search the Rust index's embeddings, or both engines
                           merged; both need codanna_embeddings in config.json
    lx ask KIND:WHAT...    several questions, one call: def, callers, about, find
    lx graph [ROOT]        build or refresh the graphify graph alone
    lx refresh [ROOT]      rebuild what changed, then print the status line
    lx status [ROOT]       print the status line
    lx prune [--apply]     list index data left behind by folders that are gone;
                           --apply removes it

Hook input arrives as JSON on stdin; only its "cwd" field is used. Everything
here is local: tree-sitter parsing, and a cached static embedding model for
semble. The one network call this can ever make is semble fetching that model
the first time, on a machine that does not have it yet.

The directory a session opens in is untrusted. Nothing is ever run from it or
resolved against it: this process moves to its own folder before it looks up a
single program, and git is always pointed at the repository with -C.
"""
from __future__ import annotations

import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import time

# argparse, concurrent.futures and shutil are imported where they are used: a
# lookup has to answer in a fraction of a second, and those three cost more to
# load than the lookup itself.

HOME = pathlib.Path.home()
BASE = HOME / ".local-index"
STATE = BASE / "state"
LOG = BASE / "refresh.log"

DEFAULTS = {
    # A repository with more tracked files than this is left for a deliberate,
    # hand-run index: the point of the hook is to be invisible.
    "max_files": 20000,
    # Opening three sessions in a minute should not run three refreshes.
    "min_refresh_seconds": 120,
    "codanna": True,
    "graphify": True,
    "semble": True,
    # Build the graphify graph only when `lx graph` asks for it. The Rust index
    # already answers definitions and callers, and the graph is the slowest of
    # the three to keep fresh and a third of the disk.
    "graph_on_demand": False,
    # Give the Rust index its own embeddings, which `lx find --hybrid`, `--rust`
    # and `lx ask` then search. Measured, that saves a turn or more on "find the
    # code that does X" questions. It costs a transformer pass over the code on
    # a CPU (about a second a file on a first build, where the others take
    # seconds in all) and a one-time model download by codanna, so it is off
    # until asked for.
    "codanna_embeddings": False,
    # With embeddings on, a repository with more tracked files than this is
    # still indexed without them: the first build would tie up every core for
    # too long to run unasked.
    "embed_max_files": 600,
    # Which engine `lx find` uses when no flag is given: "semble" or "codanna".
    "find_engine": "semble",
    # Absolute paths that are never indexed, on top of the home directory.
    "skip_roots": [],
    # Set any of these to an absolute path when the tool is not where it is looked for.
    "codanna_cmd": "",
    "graphify_cmd": "",
    "semble_cmd": "",
}

# codanna reads .codanna/settings.toml from the folder it runs in, and that file
# can point its embedding step at a remote server. A repository must never get
# to choose that, so codanna is always handed this file instead, and both it
# and the index live under ~/.local-index, outside the repository. Embeddings
# stay off unless config.json asks for them: on a CPU they make the build ten
# times slower, and semble already covers search by meaning.
CODANNA_SETTINGS = """version = 1
index_path = {index}
workspace_root = {root}

[indexing]
show_progress = false

[semantic_search]
enabled = {semantic}

[file_watch]
enabled = false

[guidance]
enabled = false

[documents]
enabled = false
"""

# Bumped whenever the way codanna is set up changes, so older indexes are rebuilt.
CODANNA_LAYOUT = "2"

VERBS = ("def", "callers", "about", "ask", "find", "graph", "refresh", "status", "prune")
# One hit in codanna's text output: `name (Kind) at path:first-last [symbol_id:N]`.
HIT = re.compile(r"^(\S+) \(\w+\) at (\S+?):(\d+)(?:-(\d+))? \[symbol_id:\d+\]")
CANDIDATE = re.compile(r"symbol_id:(\d+) - \w+ at (\S+?):(\d+)")
GRAPH_CALL = re.compile(r"^- (\S+?)(?:\(\))? \[calls\] (\S+?):L(\d+)", re.MULTILINE)
GRAPH_SOURCE = re.compile(r"^\s*Source:\s+(\S+) L(\d+)", re.MULTILINE)

# graphify before 0.9.70 hands capital-F Fortran sources to the C preprocessor
# when it finds one, and a hostile file can then pull other host files into
# graph.json (GHSA-pcc4-rvhr-2pr8). graphify falls back to reading the file
# as-is when there is no `cpp`, so an older graphify is run with every folder
# that holds a `cpp` taken off its PATH. That closes it for every file, tracked
# or not, without having to guess which files graphify will pick up.
CPP_SAFE_FROM = (0, 9, 70)
CPP_NAMES = ("cpp", "cpp.exe", "cpp.cmd", "cpp.bat", "cpp.com")

LOCK_STALE_SECONDS = 900
NO_WINDOW = 0x08000000 if os.name == "nt" else 0
_config_note = ""


def config() -> dict:
    global _config_note
    merged = dict(DEFAULTS)
    path = BASE / "config.json"
    try:
        loaded = json.loads(path.read_text(encoding="utf-8-sig"))
        if isinstance(loaded, dict):
            merged.update({key: loaded[key] for key in DEFAULTS
                           if key in loaded and isinstance(loaded[key], type(DEFAULTS[key]))})
        else:
            _config_note = "config.json is not an object, defaults in use"
    except FileNotFoundError:
        pass
    except (OSError, ValueError):
        # Say so: a silent fallback would quietly re-enable a repository the
        # owner had switched off in skip_roots.
        _config_note = "config.json unreadable, defaults in use"
    return merged


def run(argv: list[str], timeout: int = 10, env: dict | None = None,
        merge: bool = True, cwd: pathlib.Path | None = None) -> tuple[int, str]:
    """Run a command quietly; return (code, output). The working directory is this
    script's own folder unless a caller has a reason to name another."""
    try:
        done = subprocess.run(
            argv, cwd=str(cwd or BASE), timeout=timeout, env=env, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT if merge else subprocess.DEVNULL,
            creationflags=NO_WINDOW,
        )
        return done.returncode, done.stdout.decode("utf-8", "replace")
    except (OSError, subprocess.SubprocessError) as error:
        return 127, str(error)


class Tools(dict):
    """Where each program is, looked up the first time it is asked for.

    A search of PATH costs tens of milliseconds on Windows and a lookup needs one
    or two programs, not six. Known install locations are tried before PATH.
    """

    def __init__(self, cfg: dict):
        super().__init__()
        self.cfg = cfg

    def __missing__(self, name: str) -> str:
        exe = ".exe" if os.name == "nt" else ""
        scripts = "Scripts" if os.name == "nt" else "bin"
        known = {
            "codanna": [HOME / "bin" / f"codanna{exe}", HOME / ".local" / "bin" / f"codanna{exe}"],
            "graphify": [HOME / ".graphify" / "venv" / scripts / f"graphify{exe}"],
            "semble": [HOME / "Tools" / "codesearch-venv" / scripts / f"semble{exe}"],
        }
        override = str(self.cfg.get(f"{name}_cmd", ""))
        path = override if override and pathlib.Path(override).is_file() else ""
        if self.cfg.get(name) is False:  # switched off in config.json
            self[name] = ""
            return ""
        if not path:
            path = next((str(place) for place in known.get(name, []) if place.is_file()), "")
        if not path:
            import shutil
            path = shutil.which(name) or ""
        self[name] = path
        return path


def nearest_repo(start: pathlib.Path) -> pathlib.Path | None:
    """The repository holding `start`, found by walking up for .git. No process is
    started, which is most of what makes a lookup fast."""
    try:
        here = start.resolve()
    except OSError:
        return None
    for folder in (here, *here.parents):
        if (folder / ".git").exists():
            return folder
    return None


def hook_cwd(fallback: pathlib.Path) -> pathlib.Path:
    """The directory the session opened in, from the hook payload when there is one."""
    try:
        if sys.stdin is not None and not sys.stdin.isatty():
            # Windows shells can prefix piped text with a byte-order mark.
            raw = sys.stdin.buffer.read().decode("utf-8-sig", "replace")
            payload = json.loads(raw or "{}")
            if isinstance(payload, dict) and isinstance(payload.get("cwd"), str):
                return pathlib.Path(payload["cwd"])
    except (OSError, ValueError):
        pass
    return fallback


def git(found: dict, root: pathlib.Path, *args: str, timeout: int = 10) -> tuple[int, str]:
    if not found["git"]:
        return 127, ""
    return run([found["git"], "-C", str(root), *args], timeout=timeout)


def repo_root(found: dict, start: pathlib.Path) -> pathlib.Path | None:
    code, out = git(found, start, "rev-parse", "--show-toplevel", timeout=5)
    if code != 0 or not out.strip():
        return None
    return pathlib.Path(out.strip().splitlines()[-1]).resolve()


def skip_reason(root: pathlib.Path, cfg: dict) -> str:
    """Why this repository is not indexed, or "" when it should be."""
    home = HOME.resolve()
    if root == home or root in home.parents:
        return "home directory is never indexed"
    if (root / ".local-index-skip").exists():
        return "switched off here by .local-index-skip"
    for raw in cfg["skip_roots"]:
        try:
            blocked = pathlib.Path(str(raw)).resolve()
        except OSError:
            continue
        if root == blocked or blocked in root.parents:
            return "listed in skip_roots"
    return ""


def state_path(root: pathlib.Path) -> pathlib.Path:
    key = hashlib.sha1(str(root).lower().encode("utf-8")).hexdigest()[:16]
    return STATE / f"{key}.json"


def read_state(root: pathlib.Path) -> dict:
    try:
        loaded = json.loads(state_path(root).read_text(encoding="utf-8-sig"))
    except (OSError, ValueError):
        return {}
    return loaded if isinstance(loaded, dict) else {}


def write_state(root: pathlib.Path, state: dict) -> None:
    target = state_path(root)
    partial = target.with_suffix(f".{os.getpid()}.tmp")
    partial.write_text(json.dumps(state, indent=1), encoding="utf-8")
    os.replace(partial, target)


def number(value: object) -> float:
    return float(value) if isinstance(value, (int, float)) else 0.0


def age(stamp: float) -> str:
    seconds = max(0, int(time.time() - stamp))
    if seconds < 90:
        return f"{seconds}s"
    if seconds < 5400:
        return f"{seconds // 60}m"
    if seconds < 172800:
        return f"{seconds // 3600}h"
    return f"{seconds // 86400}d"


def status_line(root: pathlib.Path, cfg: dict, found: dict, refreshing: bool) -> str:
    reason = skip_reason(root, cfg)
    if reason:
        return f"local-index: off for {root.name} ({reason}). Use the client's own search."
    state, parts = read_state(root), []
    for key, label, unit in (("codanna", "symbols", ""), ("graphify", "graph", " nodes"),
                             ("semble", "semble", "")):
        if not cfg[key] or not found[key]:
            parts.append(f"{label} absent")
            continue
        entry = state.get(key) if isinstance(state.get(key), dict) else {}
        if entry.get("ok"):
            detail = f"{entry['nodes']}{unit}, " if entry.get("nodes") else ""
            kept = f", {entry['kept']}" if entry.get("kept") else ""
            parts.append(f"{label} ready ({detail}{age(number(entry.get('at')))} old{kept})")
        elif entry.get("note"):
            parts.append(f"{label} off ({entry['note']})")
        elif key == "graphify" and cfg["graph_on_demand"]:
            parts.append("graph on demand (lx graph)")
        elif key == "semble" and cfg["find_engine"] == "codanna" and cfg["codanna_embeddings"]:
            parts.append("find via symbols")
        else:
            parts.append(f"{label} building, first run")
    direct = [name for name in ("ast-grep", "rg") if found[name]]
    if direct:
        parts.append(", ".join(direct) + " ready")
    tail = " Refreshing in background." if refreshing else ""
    note = f" Note: {_config_note}." if _config_note else ""
    return f"local-index: {root.name} | " + " | ".join(parts) + "." + tail + note


def emit(line: str, client: str) -> None:
    if not line:
        return
    if client == "copilot":
        # The Copilot CLI reads the top-level key; VS Code's local agent reads
        # the nested one. Each ignores the other.
        line = json.dumps({"additionalContext": line, "hookSpecificOutput": {
            "hookEventName": "SessionStart", "additionalContext": line}})
    # Bytes, not print: a piped stdout on Windows uses the ANSI code page, and a
    # repository name outside it would raise instead of printing.
    sys.stdout.buffer.write(line.encode("utf-8") + b"\n")
    sys.stdout.buffer.flush()


def refresh_due(root: pathlib.Path, cfg: dict) -> bool:
    last = number(read_state(root).get("refreshed_at"))
    return time.time() - last >= cfg["min_refresh_seconds"]


def spawn_refresh(root: pathlib.Path) -> None:
    """Start the refresh as its own process so the hook returns at once."""
    argv = [sys.executable, str(pathlib.Path(__file__).resolve()), "--refresh", str(root),
            "--quiet"]
    quiet = dict(stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                 stderr=subprocess.DEVNULL, close_fds=True, cwd=str(BASE))
    if os.name != "nt":
        subprocess.Popen(argv, start_new_session=True, **quiet)
        return
    new_group, breakaway = 0x00000200, 0x01000000
    try:
        # A client may run hooks inside a job that ends its children with it.
        subprocess.Popen(argv, creationflags=NO_WINDOW | new_group | breakaway, **quiet)
    except OSError:
        subprocess.Popen(argv, creationflags=NO_WINDOW | new_group, **quiet)


def log(message: str) -> None:
    try:
        BASE.mkdir(parents=True, exist_ok=True)
        if LOG.exists() and LOG.stat().st_size > 262144:
            LOG.replace(LOG.with_suffix(".log.1"))
        with LOG.open("a", encoding="utf-8") as stream:
            stream.write(time.strftime("%Y-%m-%d %H:%M:%S ") + message + "\n")
    except OSError:
        pass


def alive(pid: int) -> bool:
    """Whether a process with this id is still running."""
    if os.name != "nt":
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return False
        except OSError:
            return True
        return True
    # Not os.kill: on Windows that ends the process instead of probing it.
    import ctypes
    kernel = ctypes.windll.kernel32
    handle = kernel.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    if not handle:
        return False
    code = ctypes.c_ulong()
    answered = kernel.GetExitCodeProcess(handle, ctypes.byref(code))
    kernel.CloseHandle(handle)
    return bool(answered) and code.value == 259  # STILL_ACTIVE


def take_lock(root: pathlib.Path) -> pathlib.Path | None:
    """One refresh per repository at a time. A lock whose owner died is taken over:
    a client that exits kills its hooks, and that must not block the next session."""
    lock = state_path(root).with_suffix(".lock")
    STATE.mkdir(parents=True, exist_ok=True)
    try:
        if lock.exists():
            try:
                owner = int(lock.read_text(encoding="utf-8").strip() or "0")
            except ValueError:
                owner = 0
            stale = time.time() - lock.stat().st_mtime > LOCK_STALE_SECONDS
            if stale or not owner or not alive(owner):
                lock.unlink()
        with lock.open("x", encoding="utf-8") as stream:
            stream.write(str(os.getpid()))
        return lock
    except OSError:
        return None


def keep_out_of_git(found: dict, root: pathlib.Path) -> None:
    """Hide graphify-out/ from git status without touching a tracked file."""
    code, out = git(found, root, "rev-parse", "--git-path", "info/exclude", timeout=5)
    if code != 0 or not out.strip():
        return
    exclude = pathlib.Path(out.strip().splitlines()[-1])
    if not exclude.is_absolute():
        exclude = root / exclude
    try:
        present = exclude.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        present = []
    # Read the file rather than ask git: `check-ignore` says no for a folder
    # that does not exist yet, and the entry would be added again every run.
    if any(line.strip().rstrip("/") in ("graphify-out", "/graphify-out") for line in present):
        return
    code, _ = git(found, root, "check-ignore", "-q", "graphify-out", timeout=5)
    if code == 0:
        return
    try:
        exclude.parent.mkdir(parents=True, exist_ok=True)
        with exclude.open("a", encoding="utf-8") as stream:
            stream.write("\n# local code graph, rebuilt on demand\ngraphify-out/\n")
    except OSError:
        pass


def graphify_version(exe: str) -> tuple[int, ...]:
    _, out = run([exe, "--version"], timeout=20)
    digits = out.strip().split()[-1].split(".") if out.strip() else []
    try:
        return tuple(int(part) for part in digits[:3])
    except ValueError:
        return (0,)


def path_without_cpp(path_value: str) -> str:
    kept = []
    for entry in path_value.split(os.pathsep):
        folder = entry.strip('"')
        if folder and not any(os.path.isfile(os.path.join(folder, name)) for name in CPP_NAMES):
            kept.append(entry)
    return os.pathsep.join(kept)


def graph_summary(graph: pathlib.Path) -> dict:
    try:
        nodes = len(json.loads(graph.read_text(encoding="utf-8")).get("nodes", []))
    except (OSError, ValueError, AttributeError):
        nodes = 0
    return {"ok": True, "nodes": nodes, "at": graph.stat().st_mtime}


def refresh_graph(found: dict, root: pathlib.Path) -> dict:
    out_dir = root / "graphify-out"
    graph = out_dir / "graph.json"
    # A graph someone committed, or built by hand with clusters and a report,
    # is theirs: it is used as it stands and never rewritten from here.
    if graph.is_file():
        tracked, _ = git(found, root, "ls-files", "--error-unmatch", "graphify-out/graph.json")
        if tracked == 0:
            return dict(graph_summary(graph), kept="tracked in git, not refreshed")
        if (out_dir / "GRAPH_REPORT.md").exists() or (out_dir / ".graphify_labels.json").exists():
            return dict(graph_summary(graph), kept="hand-built, not refreshed")
    keep_out_of_git(found, root)
    # Newer graphify rewrites its installed agent skills on first run; an
    # unattended hook is not the place for that.
    env = dict(os.environ, GRAPHIFY_NO_TIPS="1", GRAPHIFY_NO_AUTO_REFRESH="1")
    if graphify_version(found["graphify"]) < CPP_SAFE_FROM:
        env["PATH"] = path_without_cpp(env.get("PATH", ""))
    # --code-only is what keeps this local: without it graphify sends documents
    # to whichever model it finds a key for.
    code, out = run([found["graphify"], "extract", str(root), "--code-only", "--no-viz",
                     "--no-cluster"], timeout=900, env=env)
    if code != 0 or not graph.is_file():
        log(f"graphify failed for {root}: {out.strip()[-400:]}")
        return {"ok": False, "note": "last build failed, see ~/.local-index/refresh.log"}
    return dict(graph_summary(graph), at=time.time())


def refresh_semble(found: dict, root: pathlib.Path) -> dict:
    # semble has no index command: a search builds the index on first use and
    # re-reads only changed files afterwards.
    argv = [found["semble"], "search", "entry point", str(root), "-k", "1",
            "--max-snippet-lines", "0"]
    quiet = dict(os.environ, HF_HUB_DISABLE_SYMLINKS_WARNING="1")
    code, out = run(argv, timeout=900, env=dict(quiet, HF_HUB_OFFLINE="1"))
    if code != 0:
        # Offline fails only when the embedding model was never downloaded.
        code, out = run(argv, timeout=900, env=dict(quiet, HF_HUB_OFFLINE="0"))
    if code != 0:
        log(f"semble failed for {root}: {out.strip()[-400:]}")
        return {"ok": False, "note": "last build failed, see ~/.local-index/refresh.log"}
    return {"ok": True, "at": time.time()}


def codanna_settings(root: pathlib.Path, embeddings: bool = False) -> pathlib.Path:
    """This repository's codanna settings file, written by us, outside the repository."""
    folder = BASE / "codanna" / state_path(root).stem
    settings, layout = folder / "settings.toml", folder / "layout"
    wanted = CODANNA_LAYOUT + ("+embeddings" if embeddings else "")
    try:
        current = layout.read_text(encoding="utf-8") == wanted
    except OSError:
        current = False
    if not current or not settings_still_ours(settings, folder / "index", embeddings):
        # Start clean: an index built under other settings is not worth trusting.
        import shutil
        shutil.rmtree(folder / "index", ignore_errors=True)
        folder.mkdir(parents=True, exist_ok=True)
        settings.write_text(CODANNA_SETTINGS.format(
            index=json.dumps(str(folder / "index")), root=json.dumps(str(root)),
            semantic="true" if embeddings else "false"), encoding="utf-8")
        layout.write_text(wanted, encoding="utf-8")
    return settings


def settings_still_ours(settings: pathlib.Path, index: pathlib.Path,
                        embeddings: bool = False) -> bool:
    """codanna rewrites this file on its first run, adding the path it indexed and
    its own defaults. That copy is kept, since replacing it makes codanna index
    every file twice, but only while it still says what ours said: embeddings off,
    no remote server, index in our folder."""
    try:
        import tomllib
        data = tomllib.loads(settings.read_text(encoding="utf-8"))
    except (ImportError, OSError, ValueError):
        return False
    semantic = data.get("semantic_search")
    if not isinstance(semantic, dict) or semantic.get("enabled") is not embeddings:
        return False
    if "remote_url" in semantic:
        return False
    kept = str(data.get("index_path", "")).replace("\\\\?\\", "")
    return os.path.normcase(os.path.normpath(kept)) == os.path.normcase(os.path.normpath(str(index)))


def built_with_embeddings(root: pathlib.Path) -> bool | None:
    """How this repository's Rust index was last built: with embeddings, without
    them, or (None) not by this version at all."""
    try:
        layout = (BASE / "codanna" / state_path(root).stem / "layout").read_text(encoding="utf-8")
    except OSError:
        return None
    return {CODANNA_LAYOUT: False, CODANNA_LAYOUT + "+embeddings": True}.get(layout)


def lookup_settings(root: pathlib.Path) -> pathlib.Path | None:
    """The settings an existing index was built with. A lookup reads the index
    as it stands and only a refresh may rebuild it, so changing the embeddings
    option never empties an index that a lookup is about to read."""
    built = built_with_embeddings(root)
    folder = BASE / "codanna" / state_path(root).stem
    if built is None or not settings_still_ours(folder / "settings.toml", folder / "index", built):
        return None
    return folder / "settings.toml"


def codanna(found: dict, root: pathlib.Path, *args: str, timeout: int = 60) -> tuple[int, str]:
    if not found["codanna"]:
        return 127, ""
    settings = lookup_settings(root) or codanna_settings(
        root, bool(found.cfg.get("codanna_embeddings")))
    return run([found["codanna"], "--config", str(settings), *args], timeout=timeout)


def refresh_codanna(found: dict, root: pathlib.Path, previous: dict,
                    embeddings: bool = False) -> dict:
    # codanna re-reads changed files by a path relative to where it runs, so this
    # one command runs in the repository. It is still started by absolute path
    # and still handed our settings, which a repository's own file cannot override.
    settings = codanna_settings(root, embeddings)
    argv = [found["codanna"], "--config", str(settings), "index", ".", "--no-progress"]
    code, out = run(argv, timeout=3600 if embeddings else 900, cwd=root)
    if code != 0:
        # An index it cannot bring up to date is cheaper to rebuild than to debug.
        code, out = run(argv + ["--force"], timeout=3600 if embeddings else 900, cwd=root)
    if code != 0 and embeddings:
        # Most often the embedding model could not be fetched. Definitions and
        # callers matter more than search by meaning, which semble still covers.
        log(f"codanna with embeddings failed for {root}, building without: {out.strip()[-300:]}")
        # Not tried again for a day: each attempt costs a full rebuild.
        return dict(refresh_codanna(found, root, previous), kept="no embeddings, see refresh.log",
                    embed_retry_after=time.time() + 86400)
    if code != 0:
        log(f"codanna failed for {root}: {out.strip()[-400:]}")
        return {"ok": False, "note": "last build failed, see ~/.local-index/refresh.log"}
    counted = re.search(r"with (\d+) total symbols", out)
    # An unchanged index reports no count, so the last one still stands.
    symbols = int(counted.group(1)) if counted else previous.get("nodes", 0)
    entry = {"ok": True, "nodes": symbols, "at": time.time()}
    if embeddings:
        return dict(entry, kept="embeddings")
    if number(previous.get("embed_retry_after")) > time.time():
        # Still inside the day a failed embeddings build bought: say so, and keep the date.
        entry.update(kept="no embeddings, see refresh.log", embed_retry_after=previous["embed_retry_after"])
    return entry


def refresh(root: pathlib.Path, cfg: dict, found: dict, only: tuple = ()) -> None:
    if skip_reason(root, cfg):
        return
    lock = take_lock(root)
    if lock is None:
        return
    try:
        state = read_state(root)
        _, listing = git(found, root, "ls-files", timeout=30)
        count = len(listing.splitlines())
        if count > cfg["max_files"]:
            note = f"{count} tracked files, over the {cfg['max_files']} limit"
            state.update({key: {"ok": False, "note": note}
                          for key in ("codanna", "graphify", "semble")})
        else:
            earlier = state.get("codanna") if isinstance(state.get("codanna"), dict) else {}
            # Embeddings only where the first build stays short enough to run unasked.
            embed = (bool(cfg["codanna_embeddings"]) and count <= cfg["embed_max_files"]
                     and number(earlier.get("embed_retry_after")) < time.time())
            jobs = {"codanna": lambda: refresh_codanna(found, root, earlier, embed),
                    "graphify": lambda: refresh_graph(found, root),
                    "semble": lambda: refresh_semble(found, root)}
            wanted = {key: job for key, job in jobs.items() if cfg[key] and found[key]}
            if only:
                wanted = {key: job for key, job in wanted.items() if key in only}
            elif cfg["graph_on_demand"]:
                # Left alone here: `lx graph` builds it when a question needs it.
                wanted.pop("graphify", None)
            if cfg["find_engine"] == "codanna" and embed and not only:
                wanted.pop("semble", None)  # the Rust index is doing the searching
            # The three do not share files, so they build side by side.
            import concurrent.futures
            with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
                running = {key: pool.submit(job) for key, job in wanted.items()}
                for key, future in running.items():
                    try:
                        state[key] = future.result()
                    except Exception as error:  # one engine failing must not lose the others
                        log(f"{key} raised for {root}: {error!r}")
                        state[key] = {"ok": False, "note": "last build failed"}
        state.update(root=str(root), refreshed_at=time.time())
        write_state(root, state)
        log(f"refreshed {root}")
    finally:
        try:
            lock.unlink()
        except OSError:
            pass


def hits(output: str) -> list[str]:
    """codanna's blocks reduced to one `name path:first-last` line per hit."""
    rows = []
    for line in output.splitlines():
        match = HIT.match(line)
        if match:
            name, path, first, last = match.groups()
            row = f"{name} {path}:{first}" + (f"-{last}" if last else "")
            if row not in rows:  # one caller that calls three times is still one caller
                rows.append(row)
    return rows


def lookup_callers(found: dict, root: pathlib.Path, name: str) -> list[str]:
    code, out = codanna(found, root, "retrieve", "callers", name)
    if code == 0:
        return hits(out) or ["none"]
    if "Ambiguous" in out:
        # Several symbols share the name: answer for each, labelled by where it lives.
        rows = []
        for symbol_id, path, line in CANDIDATE.findall(out)[:4]:
            rows.append(f"# {name} at {path}:{line}")
            _, each = codanna(found, root, "retrieve", "callers", f"symbol_id:{symbol_id}")
            rows += hits(each) or ["none"]
        return rows
    # No Rust index, or a language it does not parse: the graph can still answer.
    graph = root / "graphify-out" / "graph.json"
    if found["graphify"] and graph.is_file():
        _, out = run([found["graphify"], "affected", name, "--depth", "1",
                      "--graph", str(graph)], timeout=60)
        rows = [f"{caller} {path}:{line}" for caller, path, line in GRAPH_CALL.findall(out)]
        if rows or out.startswith("Affected nodes"):
            return rows or ["none"]
    return ["not found"]


def definitions(output: str) -> list[str]:
    """codanna's symbol blocks as `name path:first-last signature`. The signature
    rides along because the next question is nearly always "what does it take",
    and answering it here saves the caller a whole extra read."""
    rows, signature = [], False
    for line in output.splitlines():
        match = HIT.match(line)
        if match:
            name, path, first, last = match.groups()
            rows.append(f"{name} {path}:{first}" + (f"-{last}" if last else ""))
            signature = False
        elif line.strip() == "Signature:":
            signature = bool(rows)
        elif signature and line[:1].isspace() and line.strip():
            rows[-1] = (rows[-1] + " " + " ".join(line.split()))[:300]
        else:
            signature = False
    return list(dict.fromkeys(rows))


def lookup_def(found: dict, root: pathlib.Path, name: str) -> list[str]:
    code, out = codanna(found, root, "retrieve", "symbol", name)
    rows = definitions(out) if code == 0 else []
    if rows:
        return rows
    graph = root / "graphify-out" / "graph.json"
    if found["graphify"] and graph.is_file():
        _, out = run([found["graphify"], "explain", name, "--graph", str(graph)], timeout=60)
        source = GRAPH_SOURCE.search(out)
        if source:
            return [f"{name} {source.group(1)}:{source.group(2)}"]
    return ["not found"]


CARDS: dict[tuple[str, str], list[dict]] = {}   # one Rust lookup per name, per run


def card_of(symbol: object) -> dict | None:
    """One symbol from the Rust index's JSON, cut down to what a caller needs:
    where it is, what it takes and the first line of what it says it does."""
    if not isinstance(symbol, dict) or not isinstance(symbol.get("range"), dict):
        return None
    span = symbol["range"]
    if not symbol.get("name") or not isinstance(span.get("start_line"), int):
        return None
    last = span.get("end_line") if isinstance(span.get("end_line"), int) else span["start_line"]
    doc = str(symbol.get("doc_comment") or "").strip().split("\n", 1)[0]
    return {"name": str(symbol["name"]),
            "path": str(symbol.get("file_path") or "").replace("\\", "/"),
            "first": span["start_line"] + 1, "last": last + 1,   # codanna counts from zero
            "signature": " ".join(str(symbol.get("signature") or "").split()),
            "doc": " ".join(doc.split())[:160]}


def symbol_cards(found: dict, root: pathlib.Path, name: str) -> list[dict]:
    """Every definition called `name`, each with its callers, from one call to
    the Rust index. Empty when there is no Rust index or it has no such symbol."""
    key = (str(root), name)
    if key not in CARDS:
        _, out = codanna(found, root, "retrieve", "symbol", name, "--json")
        cards = []
        try:
            data = json.loads(out).get("data") or []
        except (ValueError, AttributeError):
            data = []
        for item in data if isinstance(data, list) else ():
            card = card_of(item.get("symbol")) if isinstance(item, dict) else None
            if not card:
                continue
            links = item.get("relationships") if isinstance(item.get("relationships"), dict) else {}
            callers: dict[tuple, dict] = {}
            for pair in links.get("called_by") or []:
                # One entry per call site: a function that calls three times is one caller.
                caller = card_of(pair[0] if isinstance(pair, list) and pair else pair)
                if caller:
                    callers.setdefault((caller["name"], caller["path"], caller["first"]), caller)
            card["callers"] = list(callers.values())
            cards.append(card)
        CARDS[key] = cards
    return CARDS[key]


def where_row(card: dict) -> str:
    return f"{card['name']} {card['path']}:{card['first']}-{card['last']}"


def card_rows(cards: list, docs: bool = True) -> list[str]:
    """Cards as text: `name path:first-last signature`, then the doc line. An
    entry that is already a string (a hit no definition was confirmed for) is
    passed through."""
    rows = []
    for card in cards:
        if isinstance(card, str):
            rows.append(card)
            continue
        rows.append(f"{where_row(card)} {card['signature']}".rstrip()[:300])
        if docs and card["doc"]:
            rows.append("  " + card["doc"])
    return rows


def lookup_about(found: dict, root: pathlib.Path, name: str) -> list[str]:
    """Everything usually asked about a symbol, in one answer: where it is
    defined, its signature, what it says it does and who calls it."""
    cards = symbol_cards(found, root, name)
    if not cards:
        # No Rust index, or a language it does not parse: the other paths still answer.
        where = lookup_def(found, root, name)
        if where == ["not found"]:
            return [f"{name}: not found"]
        callers = lookup_callers(found, root, name)
        callers = [] if callers in (["none"], ["not found"]) else callers
        return where + [f"  {len(callers)} caller{'' if len(callers) == 1 else 's'}"
                        + (":" if callers else "")] + ["    " + row for row in callers]
    rows = []
    for card in cards:
        rows.append(f"{where_row(card)} {card['signature']}".rstrip()[:300])
        if card["doc"]:
            rows.append("  doc: " + card["doc"])
        callers = card["callers"]
        rows.append(f"  {len(callers)} caller{'' if len(callers) == 1 else 's'}" + (":" if callers else ""))
        rows += ["    " + where_row(caller) for caller in callers]
    return rows


def rust_cards(found: dict, root: pathlib.Path, query: str, limit: int) -> list[dict]:
    """Search by meaning in the Rust index. It only has something to search
    when the index was built with embeddings (codanna_embeddings in config.json)."""
    _, out = codanna(found, root, "mcp", "semantic_search_docs", "query:" + query,
                     f"limit:{limit}", "--json", timeout=120)
    try:
        data = json.loads(out).get("data") or []
    except (ValueError, AttributeError):
        return []
    cards = [card_of(item.get("symbol")) for item in data if isinstance(item, dict)]
    return [card for card in cards if card]


def lookup_find_rust(found: dict, root: pathlib.Path, query: str, limit: int) -> list[str]:
    return [where_row(card) for card in rust_cards(found, root, query, limit)] or ["none"]


def semble_rows(found: dict, root: pathlib.Path, query: str, limit: int, content: str) -> list[str]:
    """semble's hits as `path:first-last`, best first."""
    if not found["semble"]:
        return []
    argv = [found["semble"], "search", query, str(root), "-k", str(limit),
            "--max-snippet-lines", "0", "--format", "text"]
    if content:
        argv += ["--content", content]
    env = dict(os.environ, HF_HUB_OFFLINE="1", HF_HUB_DISABLE_SYMLINKS_WARNING="1")
    _, out = run(argv, timeout=300, env=env, merge=False)
    return [line.strip().replace("\\", "/") for line in out.splitlines()
            if re.search(r":\d+(-\d+)?$", line.strip())]


def lookup_find(found: dict, root: pathlib.Path, query: str, limit: int,
                content: str) -> list[str]:
    if not found["semble"]:
        return ["semble is not installed"]
    rows = semble_rows(found, root, query, limit, content)
    return (rows if content else named(found, root, rows)) or ["none"]


def encloses(found: dict, root: pathlib.Path, name: str, path: str, line: int) -> bool:
    """Whether the Rust index puts `line` of `path` inside a definition called `name`."""
    return any(card["path"] == path and card["first"] <= line <= card["last"]
               for card in symbol_cards(found, root, name))


def named(found: dict, root: pathlib.Path, rows: list[str]) -> list[str]:
    """Each `path:first-last` hit followed by `name:line` for the definitions
    those lines belong to, the line being where each one starts. The hit's own
    range is only where the matching text sits, often mid-function."""
    out = []
    for row, names in located(found, root, rows):
        labels = list(dict.fromkeys(f"{name}:{line}" for line, name in names))
        out.append(row + (" " + ", ".join(labels) if labels else ""))
    return out


def cards_for(found: dict, root: pathlib.Path, rows: list[str]) -> list:
    """Search hits as definition cards, best first and one per definition. A hit
    the Rust index confirms no definition for stays as its `path:first-last`."""
    out, seen = [], set()
    for row, names in located(found, root, rows):
        path, matched = row.rpartition(":")[0], False
        for line, name in names[:2]:
            # The graph and the Rust index can disagree by a decorator or two.
            card = next((c for c in symbol_cards(found, root, name)
                         if c["path"] == path and abs(c["first"] - line) <= 3), None)
            if card:
                matched = True
                key = (card["name"], card["path"], card["first"])
                if key not in seen:
                    seen.add(key)
                    out.append(card)
        if not matched and row not in seen:
            seen.add(row)
            out.append(row)
    return out


NOT_DEFINITIONS = {"variable", "constant", "field", "parameter", "module", "import"}


def name_cards(found: dict, root: pathlib.Path, query: str, limit: int) -> list[dict]:
    """The Rust index's own text search over symbol names, signatures and doc
    comments. It needs no embeddings and answers in a tenth of a second, and it
    finds the function whose name says what was asked for when a slice of file
    text does not rank."""
    # Only words reach it: the command reads `key:value` and leading dashes as options.
    words = " ".join(word.lstrip("-") for word in re.sub(r"[^\w.\-]+", " ", query).split()).strip()
    if not words:
        return []
    _, out = codanna(found, root, "retrieve", "search", words, "--limit", str(limit * 4), "--json")
    try:
        data = json.loads(out).get("data") or []
    except (ValueError, AttributeError):
        return []
    cards = []
    for item in data if isinstance(data, list) else ():
        symbol = item.get("symbol") if isinstance(item, dict) else None
        if isinstance(symbol, dict) and str(symbol.get("kind") or "").lower() not in NOT_DEFINITIONS:
            card = card_of(symbol)
            if card:
                cards.append(card)
    return cards[:limit]


def hybrid(found: dict, root: pathlib.Path, query: str, limit: int) -> list:
    """Every search the machine has, asked the same question at once and merged
    by reciprocal rank: semble over the text of the code, the Rust index over
    symbol names and doc comments, and the Rust index's embeddings when it was
    built with them. A definition several of them rank highly comes first, and
    one that only a single engine found is still there."""
    import concurrent.futures
    deep = limit * 2   # each engine's second five often hold what another ranked first
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        by_name = pool.submit(name_cards, found, root, query, deep)
        by_meaning = (pool.submit(rust_cards, found, root, query, deep)
                      if built_with_embeddings(root) else None)
        by_text = cards_for(found, root, semble_rows(found, root, query, deep, ""))
        ranked = [by_text, by_name.result()] + ([by_meaning.result()] if by_meaning else [])
    score: dict = {}
    kept: dict = {}
    for cards in ranked:
        for rank, card in enumerate(cards):
            key = card if isinstance(card, str) else (card["name"], card["path"], card["first"])
            score[key] = score.get(key, 0.0) + 1.0 / (60 + rank)
            kept.setdefault(key, card)
    return [kept[key] for key in sorted(score, key=score.get, reverse=True)][:limit]


def located(found: dict, root: pathlib.Path, rows: list[str]) -> list[tuple[str, list]]:
    """Each `path:first-last` hit with the definitions those lines belong to, as
    (start line, name): the one the hit starts inside, then any that start
    within it. A hit is a slice of a file, and without a name the caller has to
    open the file just to learn what it found. The graph says where definitions
    start and the Rust index confirms the enclosing one, so a name is either
    right or left out."""
    try:
        nodes = json.loads((root / "graphify-out" / "graph.json").read_text(encoding="utf-8"))["nodes"]
    except (OSError, ValueError, KeyError, TypeError):
        return [(row, []) for row in rows]
    starts: dict[str, list[tuple[int, str]]] = {}
    for node in nodes if isinstance(nodes, list) else ():
        if not isinstance(node, dict) or node.get("file_type") != "code":
            continue
        path, where = str(node.get("source_file") or "").replace("\\", "/"), str(node.get("source_location") or "")
        label = str(node.get("label") or "")
        label = label[:-2] if label.endswith("()") else label
        if where[:1] == "L" and where[1:].isdigit() and label and label != path.rsplit("/", 1)[-1]:
            starts.setdefault(path, []).append((int(where[1:]), label))
    out = []
    for row in rows:
        path, _, span = row.rpartition(":")
        first, _, last = span.partition("-")
        first, last = int(first), int(last or first)
        known = sorted(starts.get(path, ()))
        inside = [(line, name) for line, name in known if first < line <= last]
        # Nearest definition above the hit first; a nested one that already
        # ended is passed over for the one that really holds these lines.
        above = [(line, name) for line, name in known if line <= first][::-1][:4]
        before = next(([(line, name)] for line, name in above
                       if encloses(found, root, name, path, first)), [])
        out.append((row, list(dict.fromkeys(before + inside))[:3]))
    return out


def semble_cache() -> pathlib.Path:
    """Where semble keeps one index for every folder it has ever searched."""
    if os.name == "nt":
        local = os.environ.get("LOCALAPPDATA")
        return (pathlib.Path(local) if local else HOME / "AppData" / "Local") / "semble" / "Cache"
    if sys.platform == "darwin":
        return HOME / "Library" / "Caches" / "semble"
    cache = os.environ.get("XDG_CACHE_HOME")
    return (pathlib.Path(cache) if cache else HOME / ".cache") / "semble"


def megabytes(path: pathlib.Path) -> float:
    total = 0
    for item in (path.rglob("*") if path.is_dir() else (path,)):
        try:
            total += item.stat().st_size if item.is_file() else 0
        except OSError:
            pass
    return total / 1e6


def orphans() -> dict[str, list[pathlib.Path]]:
    """Index data kept outside a repository that has since been moved or deleted,
    by the folder it was built for. Nothing else removes it: semble keeps one
    index per folder path for good, and so does the Rust index kept here."""
    gone: dict[str, list[pathlib.Path]] = {}
    for state in sorted(STATE.glob("*.json")):
        try:
            root = json.loads(state.read_text(encoding="utf-8-sig")).get("root")
        except (OSError, ValueError, AttributeError):
            continue
        if isinstance(root, str) and not pathlib.Path(root).exists():
            rust = BASE / "codanna" / state.stem
            gone.setdefault(root, []).extend([state] + ([rust] if rust.is_dir() else []))
    cache = semble_cache()
    for entry in (sorted(cache.iterdir()) if cache.is_dir() else ()):
        if not entry.is_dir() or entry.is_symlink():
            continue
        roots = set()
        for meta in entry.glob("*/metadata.json"):
            try:
                roots.add(json.loads(meta.read_text(encoding="utf-8")).get("root_path"))
            except (OSError, ValueError, AttributeError):
                roots.add(None)
        # An orphan only when every index in the entry names a folder that is gone.
        if roots and all(isinstance(root, str) and not pathlib.Path(root).exists() for root in roots):
            gone.setdefault(sorted(roots)[0], []).append(entry)
    return gone


def prune(apply: bool) -> list[str]:
    import shutil
    rows, freed = [], 0.0
    for root, paths in sorted(orphans().items()):
        size = sum(megabytes(path) for path in paths)
        freed += size
        rows.append(f"{size:7.1f} MB  {root}")
        for path in paths if apply else ():
            if path.is_dir():
                shutil.rmtree(path, ignore_errors=True)
            else:
                path.unlink(missing_ok=True)
    verdict = (f"{'removed' if apply else 'would remove'} {freed:.1f} MB of index data "
               f"for {len(rows)} folders that no longer exist")
    return rows + [verdict + ("" if apply or not rows else ". To remove it: lx prune --apply")]


def say(lines: list[str]) -> None:
    sys.stdout.buffer.write(("\n".join(lines) + "\n").encode("utf-8"))
    sys.stdout.buffer.flush()


ASK = ("def", "callers", "about", "find")


def answer(kind: str, what: str, found: dict, root: pathlib.Path) -> list[str]:
    """The lines one question is answered with. `what` is a symbol name, or for
    `find` a description."""
    if kind == "def":
        rows = lookup_def(found, root, what)
        return [f"{what}: not found"] if rows == ["not found"] else rows
    if kind == "about":
        return lookup_about(found, root, what)
    if kind == "find":
        # Inside `lx ask` a search is the merged one, printed as definition cards.
        return card_rows(hybrid(found, root, what, 5)) or ["none"]
    # A header with the count: several lookups are often run in one call, and
    # their answers must not run together.
    rows = lookup_callers(found, root, what)
    if rows == ["not found"]:
        return [f"{what}: not found"]
    if rows == ["none"]:
        return [f"0 callers of {what}"]
    if rows[0].startswith("# "):
        return rows
    return [f"{len(rows)} caller{'' if len(rows) == 1 else 's'} of {what}:"] + rows


def front_end(verb: str, rest: list[str], opened_in: pathlib.Path, cfg: dict,
              found: Tools) -> int:
    """The `lx` command: one short verb in, one line per hit out."""
    if verb in ("def", "callers", "about"):
        if not rest or any(name.startswith("-") for name in rest):
            say([f"usage: lx {verb} NAME..."])
            return 2
        root = nearest_repo(opened_in)
        if root is None:
            say(["not in a git repository"])
            return 0
        say([row for name in rest for row in answer(verb, name, found, root)])
        return 0
    if verb == "ask":
        # Any mix of questions in one call, each answer under its own header.
        asked = [item.partition(":") for item in rest]
        if not asked or any(kind not in ASK or not what.strip() for kind, _, what in asked):
            say(["usage: lx ask KIND:WHAT...   kinds: " + ", ".join(ASK),
                 'example: lx ask about:parse_config callers:load "find:where tokens expire"'])
            return 2
        root = nearest_repo(opened_in)
        if root is None:
            say(["not in a git repository"])
            return 0
        rows = []
        for kind, _, what in asked:
            rows.append(f"== {kind}:{what}")
            rows += answer(kind, what.strip(), found, root)
        say(rows)
        return 0
    if verb == "prune":
        if rest not in ([], ["--apply"]):
            say(["usage: lx prune [--apply]"])
            return 2
        say(prune(bool(rest)))
        return 0

    import argparse
    parser = argparse.ArgumentParser(prog=f"lx {verb}")
    if verb == "find":
        parser.add_argument("words", nargs="+")
        parser.add_argument("-k", type=int, default=5, dest="limit")
        parser.add_argument("--docs", action="store_true", help="search prose instead of code")
        parser.add_argument("--all", action="store_true", help="search code, prose and config")
        parser.add_argument("--rust", action="store_true",
                            help="search the Rust index instead of semble")
        parser.add_argument("--hybrid", action="store_true",
                            help="ask semble and the Rust index together and merge the answers")
        parser.add_argument("--cards", action="store_true",
                            help="print each hit as its definition: name path:first-last "
                                 "signature, then its doc line")
    else:
        parser.add_argument("root", nargs="?", default=".")
    args = parser.parse_args(rest)

    start = pathlib.Path(getattr(args, "root", "."))
    root = nearest_repo(start if start.is_absolute() else opened_in / start)
    if root is None:
        say(["not in a git repository"])
    elif verb == "find":
        query = " ".join(args.words)
        content = "all" if args.all else "docs" if args.docs else ""
        # The Rust index can only search by meaning when it was built with embeddings.
        embedded = bool(built_with_embeddings(root)) and not content
        rust = embedded and (args.rust or cfg["find_engine"] == "codanna")
        if args.hybrid and not content:
            say(card_rows(hybrid(found, root, query, args.limit)) or ["none"])
        elif rust and args.cards:
            say(card_rows(rust_cards(found, root, query, args.limit)) or ["none"])
        elif rust:
            say(lookup_find_rust(found, root, query, args.limit))
        elif args.cards and not content:
            say(card_rows(cards_for(found, root, semble_rows(found, root, query, args.limit, "")))
                or ["none"])
        else:
            say(lookup_find(found, root, query, args.limit, content))
    else:
        if verb == "refresh":
            refresh(root, cfg, found)
        elif verb == "graph":
            refresh(root, cfg, found, only=("graphify",))
        say([status_line(root, cfg, found, False)])
    return 0


def main() -> int:
    # Take the caller's directory, then leave it before any program is looked
    # up: Windows searches the current directory first for a bare command name.
    opened_in = pathlib.Path.cwd()
    BASE.mkdir(parents=True, exist_ok=True)
    os.chdir(BASE)
    os.environ["NoDefaultCurrentDirectoryInExePath"] = "1"
    cfg = config()
    found = Tools(cfg)
    if len(sys.argv) > 1 and sys.argv[1] in VERBS:
        return front_end(sys.argv[1], sys.argv[2:], opened_in, cfg, found)

    import argparse
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--client", choices=("claude", "codex", "copilot"))
    parser.add_argument("--spawn", action="store_true",
                        help="with --client, also start the refresh in the background")
    parser.add_argument("--refresh", nargs="?", const="", metavar="ROOT")
    parser.add_argument("--status", nargs="?", const="", metavar="ROOT")
    parser.add_argument("--quiet", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()

    def resolve(named: str | None) -> pathlib.Path | None:
        start = pathlib.Path(named) if named else hook_cwd(opened_in)
        if not start.is_absolute():
            start = opened_in / start
        return repo_root(found, start)

    if args.refresh is not None:
        root = resolve(args.refresh)
        # A named ROOT is a deliberate refresh; one taken from a hook payload is
        # throttled, since every new session sends one.
        if root is not None and (args.refresh or refresh_due(root, cfg)):
            refresh(root, cfg, found)
            if args.refresh and not args.quiet:
                emit(status_line(root, cfg, found, False), "")
        return 0

    root = resolve(args.status)
    if root is None:
        return 0
    spawning = bool(args.client and args.spawn and not skip_reason(root, cfg)
                    and refresh_due(root, cfg))
    if spawning:
        spawn_refresh(root)
    emit(status_line(root, cfg, found, spawning), args.client or "")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:  # a hook must never break the session it runs in
        log(f"unexpected: {error!r}")
        sys.exit(0)
