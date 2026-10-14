"""Search lab: a measuring instrument for `pgr find`, with no model and no human labels.

It builds challenges from the material itself (a test name and the function it tests,
a commit subject and the files that commit changed, a doc line and its function),
asks every search variant the same questions, and reports recall and reciprocal rank
with bootstrap intervals.

  python -I search_lab.py prepare    --lab DIR [--pgr PGR]
  python -I search_lab.py challenges --lab DIR [--pgr PGR] [--show N]
  python -I search_lab.py run        --lab DIR [--pgr PGR] [--variant NAME=ARGS ...]
  python -I search_lab.py tune       --lab DIR [--pgr PGR] [--variants-file FILE] [--select N]
  python -I search_lab.py report     [--out FILE] [--half held-out]

Every path is an argument; DIR can also come from SEARCH_LAB_DIR. The README next to
this file says what each step does and what the numbers can and cannot show.
"""
import argparse
import bisect
import collections
import concurrent.futures
import datetime
import hashlib
import json
import os
import pathlib
import random
import re
import shlex
import shutil
import statistics
import subprocess
import sys
import tarfile
import threading
import time

try:
    import numpy as np
except ImportError:
    sys.exit("search_lab.py needs numpy for the bootstrap; install it first, this script installs nothing")

K = 20                      # hits requested per query
MAX_PER_KIND = 400          # challenges kept per kind and corpus
CHALLENGE_SEED = 20261013   # fixed seed for sampling the challenges
BOOT_RESAMPLES = 2000
BOOT_SEED = 20261013
TIME_SAMPLE = 40            # serial calls per variant and corpus for the wall-time median
KINDS = ("T", "C", "D")
KIND_TITLES = {
    "T": "test name to function",
    "C": "commit subject to changed file",
    "D": "doc line to function (control, never pooled with T and C)",
}
HALVES = ("tune", "held-out")
STOP_WORDS = {"test", "tests", "it", "should"}
MAX_ANSWER_CALLERS = 40     # a function called from more places than this is set-up code in most tests, not their subject
MAX_COMMIT_FILES = 8        # a commit that touches more files than this is about the whole project
# Stop words and generic change verbs. A commit subject needs three words outside this list.
GENERIC_WORDS = frozenset("""
a an the this that these those it its i we you he she they them our your their
to of in on at for with from by as into onto over under about after before than then when while where which who what how
and or but nor so if not no yes also only just still even more most less much some any all each both other another same
is are was were be been being am do does did doesn't don't didn't isn't aren't can't won't has have had having
can could should would will shall may might must
fix fixes fixed fixing add adds added adding remove removes removed removing update updates updated updating
use uses used using make makes made making change changes changed changing improve improves improved improving
refactor refactors refactored refactoring clean cleanup cleans move moves moved moving rename renames renamed renaming
prepare handle handles handled handling allow allows allowed support supports new now up out off
simplify simplifies simplified tweak tweaks tweaked minor small slightly little better again keep keeps code stuff
things thing misc various entry entries
""".split())
CACHE_FORMAT = 1
DEFAULT_VARIANTS = {"words": ["--words"], "meaning": ["--meaning"], "hybrid": []}

# name -> how the corpus is obtained; exts are the source extensions that can be an answer
CORPORA = {
    "fixture": {
        "how": "fixture",
        "language": "Python",
        "exts": [".py"],
        "origin": "the benchmark fixture (graphify 0.9.61 source, one commit): tracked files and .git copied from the fixture repository",
    },
    "pgr-engine": {
        "how": "archive",
        "language": "Rust, Python",
        "exts": [".rs", ".py"],
        "origin": "git archive HEAD of this repository (the committed tree); commit subjects come from this repository's history up to that commit",
    },
    "click": {
        "how": "clone",
        "language": "Python",
        "exts": [".py"],
        "url": "https://github.com/pallets/click.git",
        "origin": "https://github.com/pallets/click, git clone --depth 400",
    },
    "express": {
        "how": "clone",
        "language": "JavaScript",
        "exts": [".js", ".mjs", ".cjs"],
        "url": "https://github.com/expressjs/express.git",
        "origin": "https://github.com/expressjs/express, git clone --depth 400",
    },
    "ripgrep": {
        "how": "clone",
        "language": "Rust",
        "exts": [".rs"],
        "url": "https://github.com/BurntSushi/ripgrep.git",
        "origin": "https://github.com/BurntSushi/ripgrep, git clone --depth 400",
    },
}

# Commit subjects that cannot be answered from code are dropped before anything else.
# Case-insensitive; each pattern is searched in the original subject line.
DROP_SUBJECT = [
    r"\bbump(?:s|ed|ing)?\b", r"\brelease[sd]?\b", r"^\s*(?:v|version\s*)?\d+\.\d+", r"\bversion\s+(?:bump|to\s+\d)",
    r"\brevert(?:s|ed|ing)?\b",
    r"\b(?:re)?format(?:s|ted|ting)?\b", r"\bwhitespace\b", r"\blint(?:s|ing|er)?\b", r"\bclippy\b", r"\brustfmt\b",
    r"\bcargo fmt\b", r"\bprettier\b", r"\bblack\b", r"\bisort\b", r"\bpre-?commit\b",
    r"\btypos?\b", r"\bspelling\b", r"\bchangelog\b", r"\bmerge\b", r"\bdependabot\b", r"\bdependenc", r"\bwip\b",
    r"\bci\b", r"\bgithub actions\b", r"\bcve-\d",
]
DROP_SUBJECT_RE = [re.compile(p, re.I) for p in DROP_SUBJECT]

TOKEN_RE = re.compile(r"[A-Z]+(?![a-z])|[A-Z]?[a-z]+|[0-9]+")
WORD_RE = re.compile(r"[A-Za-z][A-Za-z0-9_'-]*")
IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
DUNDER_RE = re.compile(r"^__.*__$")
HUB_RE = re.compile(r"^(\S+) (\S+):(\d+)-(\d+)\s+(\d+) links?$")
RANGE_RE = re.compile(r"^(.+):(\d+)-(\d+)$")
CALLERS_RE = re.compile(r"^  \d+ callers?:?$")
SYMBOLS_RE = re.compile(r"graph ready \((\d+) symbols, (\d+) links")
CFG_TEST_MOD = re.compile(
    r"#\[cfg\((?:all\()?test\b[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")
RUST_RAW = re.compile(r"b?r(#*)\"")
TEST_ATTR = re.compile(r"#\[(?:\w+::)*test\b")
TEST_DIRS = {"test", "tests", "testing", "__tests__", "spec", "specs"}
SUPPORT_RE = re.compile(r"(?:^|/)(?:_?compat|legacy|_?examples?|docs?_src|docs?)(?:/|$)")
LEAD_RE = re.compile(r"\s*(?://|#|/\*|\*|@)")
IS_WINDOWS = os.name == "nt"


# ---------------------------------------------------------------- small helpers

def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def tree_hash(root):
    """Hash of every file under root except .git: names and contents."""
    root = pathlib.Path(root)
    digest = hashlib.sha256()
    count = 0
    for path in sorted(p for p in root.rglob("*") if p.is_file() and ".git" not in p.relative_to(root).parts[:1]):
        digest.update(path.relative_to(root).as_posix().encode("utf-8") + b"\0")
        digest.update(sha256_file(path).encode("ascii") + b"\n")
        count += 1
    return digest.hexdigest(), count


def tokens(text):
    return [t.lower() for t in TOKEN_RE.findall(text)]


def word_count(text):
    return len(WORD_RE.findall(text))


def content_word_count(text):
    """Words that say something: not a stop word and not one of the generic change verbs."""
    return sum(1 for w in WORD_RE.findall(text) if w.lower() not in GENERIC_WORDS)


def bare_name(name):
    return name.rsplit(".", 1)[-1]


def read_json(path, default=None):
    path = pathlib.Path(path)
    if not path.exists():
        return default
    return json.loads(path.read_text(encoding="utf-8"))


def dumps(obj, level=0):
    """JSON with one line per key and lists of plain values on a single line, so long id lists stay readable."""
    pad = " " * (level + 1)
    if isinstance(obj, dict):
        if not obj:
            return "{}"
        rows = [f"{pad}{json.dumps(str(k), ensure_ascii=False)}: {dumps(v, level + 1)}" for k, v in obj.items()]
        return "{\n" + ",\n".join(rows) + "\n" + " " * level + "}"
    if isinstance(obj, (list, tuple)):
        if all(not isinstance(x, (dict, list, tuple)) for x in obj):
            return json.dumps(list(obj), ensure_ascii=False)
        rows = [pad + dumps(x, level + 1) for x in obj]
        return "[\n" + ",\n".join(rows) + "\n" + " " * level + "]"
    return json.dumps(obj, ensure_ascii=False)


def write_json(path, obj):
    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(dumps(obj) + "\n", encoding="utf-8")
    os.replace(tmp, path)


def git(repo, *args, check=True):
    done = subprocess.run(["git", "--no-optional-locks", "-C", str(repo), *args], capture_output=True,
                          stdin=subprocess.DEVNULL)
    if check and done.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed in {repo}: {done.stderr.decode('utf-8', 'replace').strip()}")
    return done.stdout.decode("utf-8", "replace")


def log(message):
    print(message, file=sys.stderr, flush=True)


# ---------------------------------------------------------------- calling pgr

class Pgr:
    """One pgr binary with one index home. Every call carries LXR_HOME so the owner's own home is never touched."""

    def __init__(self, exe, home, extra_env=None, refresh=False):
        self.exe = str(exe)
        self.env = dict(os.environ)
        self.env["LXR_HOME"] = str(home)
        if not refresh:
            self.env["PGR_NO_REFRESH"] = "1"
        self.env.update(extra_env or {})

    def call(self, root, args, check=True):
        started = time.perf_counter()
        flags = 0x08000000 if IS_WINDOWS else 0   # CREATE_NO_WINDOW
        done = subprocess.run([self.exe, "--root", str(root), *args], env=self.env, capture_output=True,
                              stdin=subprocess.DEVNULL, creationflags=flags)
        millis = (time.perf_counter() - started) * 1000.0
        out = done.stdout.decode("utf-8", "replace")
        err = done.stderr.decode("utf-8", "replace")
        if check and done.returncode != 0:
            raise RuntimeError(f"pgr {' '.join(args[:3])} exited {done.returncode}: {(err or out).strip()[:300]}")
        return out, err, millis


def find_pgr(value):
    exe = value or shutil.which("pgr")
    if not exe or not pathlib.Path(exe).is_file():
        sys.exit("pgr not found: pass --pgr PATH or put pgr on PATH")
    return str(pathlib.Path(exe))


def parse_hit(line):
    """Parse one find row: 'NAME PATH:FIRST-LAST SIGNATURE' or 'PATH:FIRST-LAST'. Indented doc lines are not hits."""
    if not line or line[0].isspace():
        return None
    head, _, rest = line.partition(" ")
    m = RANGE_RE.match(head)
    if m:
        return [None, m.group(1), int(m.group(2)), int(m.group(3))]
    loc = rest.split(" ", 1)[0]
    m = RANGE_RE.match(loc)
    if not m:
        return None
    return [head, m.group(1), int(m.group(2)), int(m.group(3))]


def parse_hits(text):
    return [h for h in (parse_hit(line) for line in text.splitlines()) if h]


# ---------------------------------------------------------------- test recognition

def is_test_path(path):
    """A path is a test when a directory is named test, tests, testing, __tests__, spec or specs, or the file is a test file."""
    parts = path.lower().split("/")
    if any(p in TEST_DIRS for p in parts[:-1]):
        return True
    name = parts[-1]
    stem = name.rsplit(".", 1)[0]
    return (stem.startswith("test_") or stem.endswith("_test") or stem.endswith("_tests") or stem in ("tests", "test", "conftest")
            or stem.endswith(".test") or stem.endswith(".spec"))


def is_support_path(path):
    """Example, documentation, compat and legacy folders. The engine under test scales most of them down on
    purpose (rank.rs path_penalty), and documentation folders hold build configuration, not the code that
    is searched, so they are never an answer: asking for them would measure that choice and not the search."""
    return bool(SUPPORT_RE.search(path.lower()))


def skip_string(text, j):
    n = len(text)
    j += 1
    while j < n:
        if text[j] == "\\":
            j += 2
        elif text[j] == '"':
            return j + 1
        else:
            j += 1
    return n


def match_brace(text, start):
    """Index of the brace closing the one at text[start], skipping Rust comments, strings and char literals; -1 if none."""
    n = len(text)
    depth = 0
    j = start
    while j < n:
        c = text[j]
        if c == "/" and text.startswith("//", j):
            j = text.find("\n", j)
            if j < 0:
                return -1
        elif c == "/" and text.startswith("/*", j):
            level = 0
            while j < n:
                if text.startswith("/*", j):
                    level += 1
                    j += 2
                elif text.startswith("*/", j):
                    level -= 1
                    j += 2
                    if level == 0:
                        break
                else:
                    j += 1
        elif c == '"':
            j = skip_string(text, j)
        elif c in "rb" and (j == 0 or not (text[j - 1].isalnum() or text[j - 1] == "_")) and RUST_RAW.match(text, j):
            m = RUST_RAW.match(text, j)
            end = text.find('"' + m.group(1), m.end())
            j = n if end < 0 else end + 1 + len(m.group(1))
        elif c == "'":
            if text.startswith("\\", j + 1):
                k = j + 3
                while k < n and text[k] != "'":
                    k += 1
                j = k + 1
            elif j + 2 < n and text[j + 2] == "'":
                j += 3
            else:
                j += 1
        else:
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    return j
            j += 1
    return -1


class TestMap:
    """Tells whether a path, or a line range inside a file, is test code. Rust unit tests live inside the source
    file in a `#[cfg(test)] mod`, so Rust files are scanned for those modules."""

    def __init__(self, root):
        self.root = pathlib.Path(root)
        self._lines = {}
        self._ranges = {}

    def lines(self, path):
        if path not in self._lines:
            try:
                text = (self.root / path).read_text(encoding="utf-8", errors="replace")
            except OSError:
                text = ""
            self._lines[path] = text.split("\n")
        return self._lines[path]

    def ranges(self, path):
        if path not in self._ranges:
            found = []
            if path.endswith(".rs"):
                text = "\n".join(self.lines(path))
                starts = [0] + [m.end() for m in re.finditer("\n", text)]
                for m in CFG_TEST_MOD.finditer(text):
                    close = match_brace(text, m.end() - 1)
                    if close < 0:
                        close = len(text) - 1
                    found.append((bisect.bisect_right(starts, m.start()), bisect.bisect_right(starts, close)))
            self._ranges[path] = found
        return self._ranges[path]

    def in_test(self, path, first):
        if is_test_path(path):
            return True
        return any(a <= first <= b for a, b in self.ranges(path))

    def has_test_attr(self, path, first):
        if not path.endswith(".rs"):
            return False
        window = self.lines(path)[max(0, first - 5):first]
        return any(TEST_ATTR.search(line) for line in window)

    def lead_start(self, path, first):
        """First line of the comment, doc comment and attribute lines that sit directly above a definition."""
        lines = self.lines(path)
        start = first
        while start > 1 and LEAD_RE.match(lines[start - 2]):
            start -= 1
        return start

    def hit_is_test(self, hit):
        _, path, first, _ = hit
        return self.in_test(path, first) or (hit[0] is not None and self.has_test_attr(path, first))

    def body(self, path, first, last):
        return "\n".join(self.lines(path)[first - 1:last])


# ---------------------------------------------------------------- the index through its command line

def load_defs(pgr, root):
    """Every definition of the index (`pgr hubs` lists all of them), then signature, doc line and callers from `about`."""
    out, _, _ = pgr.call(root, ["hubs", "-k", "100000000"])
    defs = {}
    unparsed = 0
    for line in out.splitlines():
        m = HUB_RE.match(line)
        if not m:
            unparsed += bool(line.strip())
            continue
        key = (m.group(1), m.group(2), int(m.group(3)), int(m.group(4)))
        defs[key] = {"name": key[0], "bare": bare_name(key[0]), "path": key[1], "first": key[2], "last": key[3],
                     "links": int(m.group(5)), "sig": None, "doc": None, "callers": []}
    bares = sorted({d["bare"] for d in defs.values() if not DUNDER_RE.match(d["bare"])})
    for i in range(0, len(bares), 80):
        out, _, _ = pgr.call(root, ["about", *bares[i:i + 80]])
        cur = None
        in_callers = False
        for line in out.splitlines():
            if not line:
                continue
            if not line[0].isspace():
                in_callers = False
                hit = parse_hit(line)
                cur = None
                if hit and hit[0]:
                    cur = defs.get((hit[0], hit[1], hit[2], hit[3]))
                    if cur is not None:
                        rest = line.split(" ", 2)
                        cur["sig"] = rest[2] if len(rest) > 2 else ""
            elif cur is None:
                continue
            elif line.startswith("  doc: "):
                cur["doc"] = line[7:].strip()
            elif CALLERS_RE.match(line):
                in_callers = True
            elif line.startswith("  calls:"):
                in_callers = False
            elif in_callers and line.startswith("    "):
                hit = parse_hit(line.strip())
                if hit:
                    cur["callers"].append(hit)
    for d in defs.values():
        d["func"] = d["sig"] is not None and d["sig"].lstrip().startswith("(")
        d["ext"] = os.path.splitext(d["path"])[1].lower()
    return list(defs.values()), unparsed


def status_line(text):
    for line in text.splitlines():
        if "graph ready" in line:
            return line.strip()
    return text.strip().splitlines()[-1] if text.strip() else ""


# ---------------------------------------------------------------- prepare

def lab_paths(args):
    lab = args.lab or os.environ.get("SEARCH_LAB_DIR")
    if not lab:
        sys.exit("--lab DIR is required (or set SEARCH_LAB_DIR)")
    lab = pathlib.Path(lab)
    return {
        "lab": lab,
        "corpora": pathlib.Path(getattr(args, "corpora", None) or lab / "corpora"),
        "home": pathlib.Path(getattr(args, "home", None) or lab / "home"),
        "cache": pathlib.Path(getattr(args, "cache", None) or lab / "cache"),
        "challenges": pathlib.Path(getattr(args, "challenges_dir", None) or lab / "challenges"),
    }


def registry_path(paths):
    return paths["lab"] / "corpora.json"


def count_files(root, exts, has_git):
    if has_git:
        files = [f for f in git(root, "ls-files").split("\n") if f]
    else:
        files = [p.relative_to(root).as_posix() for p in pathlib.Path(root).rglob("*") if p.is_file()]
    source = [f for f in files if os.path.splitext(f)[1].lower() in exts]
    return len(files), len(source), sum(1 for f in source if is_test_path(f))


def materialise(name, spec, root, args, registry):
    """Create the corpus folder if it is missing. Returns the commit the corpus stands for."""
    if spec["how"] == "fixture":
        if not root.exists():
            src = pathlib.Path(args.fixture_src or pathlib.Path(args.paths["lab"]).parent / "repo")
            if not (src / ".git").is_dir():
                raise RuntimeError(f"fixture source {src} is not a git repository")
            names = [n for n in git(src, "ls-files", "-z").split("\0") if n]
            for rel in names:
                target = root / rel
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(src / rel, target)
            shutil.copytree(src / ".git", root / ".git")
        return git(root, "rev-parse", "HEAD").strip()
    if spec["how"] == "archive":
        entry = registry.get(name)
        if root.exists() and not entry:
            raise RuntimeError(f"{root} exists but is not registered; remove the folder and run prepare again")
        if not root.exists():
            repo = pathlib.Path(args.repo_root)
            commit = git(repo, "rev-parse", "HEAD").strip()
            root.mkdir(parents=True)
            proc = subprocess.Popen(["git", "-C", str(repo), "archive", "--format=tar", commit], stdout=subprocess.PIPE)
            with tarfile.open(fileobj=proc.stdout, mode="r|") as tar:
                tar.extractall(root, filter="data")
            if proc.wait() != 0:
                raise RuntimeError("git archive failed")
            return commit
        return entry["commit"]
    if not root.exists():
        if args.no_clone:
            raise RuntimeError(f"{root} is missing and --no-clone was given")
        done = subprocess.run(["git", "clone", "--depth", "400", "--quiet", spec["url"], str(root)],
                              capture_output=True, stdin=subprocess.DEVNULL)
        if done.returncode != 0:
            shutil.rmtree(root, ignore_errors=True)
            raise RuntimeError(f"clone failed: {done.stderr.decode('utf-8', 'replace').strip()[:300]}")
    return git(root, "rev-parse", "HEAD").strip()


def cmd_prepare(args):
    paths = lab_paths(args)
    args.paths = paths
    for key in ("corpora", "home"):
        paths[key].mkdir(parents=True, exist_ok=True)
    pgr = Pgr(find_pgr(args.pgr), paths["home"], refresh=True)
    registry = read_json(registry_path(paths), {"corpora": {}})["corpora"]
    names = args.corpus or list(CORPORA)
    failed = []
    for name in names:
        spec = CORPORA[name]
        root = paths["corpora"] / name
        try:
            commit = materialise(name, spec, root, args, registry)
            has_git = (root / ".git").is_dir()
            before, _ = tree_hash(root) if name in registry and registry[name].get("tree_sha256") else (None, 0)
            out, err, _ = pgr.call(root, ["refresh", *(["--force"] if args.force else [])])
            line = status_line(err + out)
            m = SYMBOLS_RE.search(line)
            files, source, tests = count_files(root, set(spec["exts"]), has_git)
            digest, hashed = tree_hash(root)
            if before and before != digest:
                raise RuntimeError("corpus files changed since the last prepare")
            entry = {
                "origin": spec["origin"], "language": spec["language"], "exts": spec["exts"], "commit": commit,
                "files": files, "source_files": source, "test_source_files": tests,
                "definitions": int(m.group(1)) if m else None, "links": int(m.group(2)) if m else None,
                "tree_sha256": digest, "files_hashed": hashed, "index_status": line,
            }
            if has_git:
                entry["history_commits"] = len([c for c in git(root, "log", "--format=%H").split("\n") if c])
                entry["shallow"] = (root / ".git" / "shallow").exists()
                entry["history"] = {"repo": str(root), "rev": commit}
            elif name in registry and "history" in registry[name]:
                entry["history"] = registry[name]["history"]
            else:
                entry["history"] = {"repo": str(pathlib.Path(args.repo_root)), "rev": commit}
                entry["history_commits"] = len([c for c in git(entry["history"]["repo"], "log", commit, "--format=%H").split("\n") if c])
            registry[name] = entry
            log(f"prepared {name}: {commit[:10]}, {files} files, {entry['definitions']} definitions")
        except Exception as exc:   # one corpus failing must not stop the others
            failed.append(name)
            log(f"FAILED {name}: {exc}")
    write_json(registry_path(paths), {"corpora": registry})
    if failed:
        log("failed corpora: " + ", ".join(failed))
        return 1
    return 0


def verify_corpora(paths, names):
    """Fail when a corpus file changed since prepare, or a git corpus is no longer clean."""
    registry = read_json(registry_path(paths), {"corpora": {}})["corpora"]
    for name in names:
        entry = registry.get(name)
        if not entry:
            raise RuntimeError(f"{name} is not prepared")
        root = paths["corpora"] / name
        digest, _ = tree_hash(root)
        if digest != entry["tree_sha256"]:
            raise RuntimeError(f"corpus {name} was modified since prepare (tree hash differs)")
        if (root / ".git").is_dir() and git(root, "status", "--porcelain").strip():
            raise RuntimeError(f"corpus {name} is not clean according to git status")
    return registry


def cmd_verify(args):
    paths = lab_paths(args)
    names = args.corpus or list(CORPORA)
    registry = read_json(registry_path(paths), {"corpora": {}})["corpora"]
    names = [n for n in names if n in registry]
    verify_corpora(paths, names)
    print("corpora unchanged since prepare: " + ", ".join(names))
    return 0


# ---------------------------------------------------------------- challenges

def clean_subject(subject):
    text = subject.replace("﻿", "").strip()
    text = re.sub(r"^\s*(?:\[[^\]]{1,30}\]\s*)*(?:[A-Za-z][\w-]{0,29}(?:\([^)]*\))?!?:\s+)?", "", text)
    text = re.sub(r"https?://\S+", "", text)
    text = re.sub(r"\((?:[A-Za-z]+\s*)?#\d+\)|(?<!\w)#\d+\b|\bgh-\d+\b", "", text, flags=re.I)
    text = re.sub(r"\s+", " ", text).strip()
    return re.sub(r"[\s.,;:!?-]+$", "", text)


def contains_run(haystack, needle):
    n = len(needle)
    return n > 0 and any(haystack[i:i + n] == needle for i in range(len(haystack) - n + 1))


def name_in_query(query, bare, name_tokens):
    if re.search(r"(?<![A-Za-z0-9_])" + re.escape(bare) + r"(?![A-Za-z0-9_])", query, re.I):
        return True
    return contains_run(tokens(query), list(name_tokens))


def challenge_id(corpus, kind, query, answer_key):
    digest = hashlib.sha256("\x1f".join((corpus, kind, query, answer_key)).encode("utf-8")).hexdigest()
    return digest[:16]


def half_of(cid):
    return "held-out" if int(cid, 16) & 1 else "tune"


def drop_ambiguous(pool, funnel):
    """A query that appears with two different answers cannot be answered right twice: drop every copy."""
    answers = collections.defaultdict(set)
    for c in pool:
        answers[c["query"]].add(json.dumps(c["answer"], sort_keys=True))
    kept = [c for c in pool if len(answers[c["query"]]) == 1]
    funnel["same_query_other_answer"] = len(pool) - len(kept)
    funnel["kept"] = len(kept)
    return kept


def sample_pool(pool, corpus, kind):
    pool = sorted(pool, key=lambda c: c["id"])
    if len(pool) <= MAX_PER_KIND:
        return pool
    rng = random.Random(f"{CHALLENGE_SEED}:{corpus}:{kind}")
    return sorted(rng.sample(pool, MAX_PER_KIND), key=lambda c: c["id"])


def make_tests(corpus, defs, tm, exts):
    """T: test name to the function under test."""
    funnel = collections.Counter()
    funcs = [d for d in defs if d["func"] and d["ext"] in exts and not DUNDER_RE.match(d["bare"])
             and not is_support_path(d["path"])
             and not tm.in_test(d["path"], d["first"]) and not tm.has_test_attr(d["path"], d["first"])]
    by_tokens = collections.defaultdict(list)
    for d in funcs:
        tk = tuple(tokens(d["bare"]))
        if tk and not set(tk) <= STOP_WORDS and sum(map(len, tk)) >= 3:
            by_tokens[tk].append(d)
    longest = max([len(t) for t in by_tokens] or [1])
    pool = {}
    for t in defs:
        if not t["func"] or t["ext"] not in exts or DUNDER_RE.match(t["bare"]):
            continue
        if not tm.in_test(t["path"], t["first"]):
            continue
        tt = tokens(t["bare"])
        if not ((tt and tt[0] in ("test", "tests")) or tm.has_test_attr(t["path"], t["first"])):
            continue
        funnel["test_functions"] += 1
        found = []
        for i in range(len(tt)):
            for j in range(i + 1, min(len(tt), i + longest) + 1):
                if tuple(tt[i:j]) in by_tokens:
                    found.append((i, j, tuple(tt[i:j])))
        maximal = [f for f in found
                   if not any(g[0] <= f[0] and f[1] <= g[1] and g[1] - g[0] > f[1] - f[0] for g in found)]
        distinct = sorted({f[2] for f in maximal})
        if not distinct:
            funnel["no_name_in_test"] += 1
            continue
        if len(distinct) > 1:
            funnel["several_names_in_test"] += 1
            continue
        answers = by_tokens[distinct[0]]
        if len(answers) != 1:
            funnel["name_defined_more_than_once"] += 1
            continue
        a = answers[0]
        if len(a["callers"]) > MAX_ANSWER_CALLERS:
            funnel["answer_called_from_everywhere"] += 1
            continue
        called = any((c[0], c[1], c[2], c[3]) == (t["name"], t["path"], t["first"], t["last"]) for c in a["callers"])
        # the graph may miss a call, so a call-shaped mention in the body (name followed by an opening
        # parenthesis, or a macro bang) also counts, but only for names of two or more tokens
        mentioned = bool(re.search(r"(?<![A-Za-z0-9_])" + re.escape(a["bare"]) + r"\s*!?\(",
                                   tm.body(t["path"], t["first"], t["last"])))
        run = min(f for f in found if f[2] == distinct[0])
        if not called and not (mentioned and len(distinct[0]) >= 2):
            funnel["test_does_not_call_it"] += 1
            continue
        # a one-token name must open the test name (test_<function>_<scenario>) and be a graph call: in a
        # sentence-style test name a one-token function name is mostly a coincidence of English words
        leading = [k for k, w in enumerate(tt) if w not in STOP_WORDS][:1]
        if len(distinct[0]) == 1 and run[0] not in leading:
            funnel["one_token_name_not_leading"] += 1
            continue
        funnel["called_by_graph" if called else "called_by_text_only"] += 1
        words = [w for k, w in enumerate(tt) if not run[0] <= k < run[1] and w not in STOP_WORDS]
        query = " ".join(words) + (" " + t["doc"] if t["doc"] else "")
        if word_count(query) < 2:
            funnel["fewer_than_two_words"] += 1
            continue
        if name_in_query(query, a["bare"], distinct[0]):
            funnel["answer_name_in_query"] += 1
            continue
        key = f'{a["name"]}|{a["path"]}'
        cid = challenge_id(corpus, "T", query, key)
        if cid in pool:
            funnel["duplicate"] += 1
            continue
        pool[cid] = {"id": cid, "kind": "T", "half": half_of(cid), "query": query,
                     "answer": {"name": a["name"], "path": a["path"], "first": a["first"], "last": a["last"]},
                     "source": {"test": t["name"], "path": t["path"], "with_doc": bool(t["doc"])}}
    funnel["kept"] = len(pool)
    return list(pool.values()), funnel


def make_docs(corpus, defs, tm, exts):
    """D: a function's own doc line (control)."""
    funnel = collections.Counter()
    seen = collections.Counter(d["doc"] for d in defs if d["doc"])
    pool = {}
    for d in defs:
        if not d["func"] or d["ext"] not in exts or DUNDER_RE.match(d["bare"]):
            continue
        if tm.in_test(d["path"], d["first"]) or tm.has_test_attr(d["path"], d["first"]):
            continue
        if is_support_path(d["path"]):
            funnel["support_folder"] += 1
            continue
        funnel["functions"] += 1
        if not d["doc"]:
            funnel["no_doc_line"] += 1
            continue
        if word_count(d["doc"]) < 3:
            funnel["fewer_than_three_words"] += 1
            continue
        if seen[d["doc"]] > 1:
            funnel["doc_line_not_unique"] += 1
            continue
        if name_in_query(d["doc"], d["bare"], tokens(d["bare"])):
            funnel["answer_name_in_query"] += 1
            continue
        cid = challenge_id(corpus, "D", d["doc"], f'{d["name"]}|{d["path"]}')
        pool[cid] = {"id": cid, "kind": "D", "half": half_of(cid), "query": d["doc"],
                     "answer": {"name": d["name"], "path": d["path"], "first": d["first"], "last": d["last"]},
                     "source": {}}
    funnel["kept"] = len(pool)
    return list(pool.values()), funnel


def history_commits(entry):
    """(hash, subject, [(status, path)]) for non-merge commits, shallow boundary commits left out."""
    repo = pathlib.Path(entry["history"]["repo"])
    shallow_file = repo / ".git" / "shallow"
    shallow = set(shallow_file.read_text().split()) if shallow_file.exists() else set()
    out = git(repo, "-c", "core.quotepath=false", "log", entry["history"]["rev"], "--no-merges", "--no-renames",
              "--name-status", "--format=@@@%H%x09%s")
    commits = []
    for line in out.split("\n"):
        if line.startswith("@@@"):
            h, _, subject = line[3:].partition("\t")
            commits.append([h, subject, []])
        elif line.strip() and commits:
            status, _, path = line.partition("\t")
            commits[-1][2].append((status.strip(), path))
    boundary = sum(1 for c in commits if c[0] in shallow)
    return [tuple(c) for c in commits if c[0] not in shallow], boundary


def make_commits(corpus, entry, defs, root, exts):
    """C: commit subject to the source files the commit changed."""
    funnel = collections.Counter()
    commits, boundary = history_commits(entry)
    funnel["shallow_boundary_skipped"] = boundary
    names_by_file = collections.defaultdict(set)
    with_defs = {d["path"] for d in defs}
    runs_by_file = collections.defaultdict(set)
    for d in defs:
        plain = d["bare"].strip("_")
        if len(plain) >= 4:
            names_by_file[d["path"]].add(plain)
        if len(tokens(plain)) >= 2:
            runs_by_file[d["path"]].add(tuple(tokens(plain)))
    pool = {}
    for commit, subject, files in commits:
        funnel["commits"] += 1
        if any(rx.search(subject) for rx in DROP_SUBJECT_RE):
            funnel["dropped_by_keyword"] += 1
            continue
        query = clean_subject(subject)
        if word_count(query) < 3:
            funnel["fewer_than_three_words"] += 1
            continue
        if content_word_count(query) < 3:
            funnel["fewer_than_three_content_words"] += 1
            continue
        src = [p for _, p in files
               if os.path.splitext(p)[1].lower() in exts and not is_test_path(p) and not is_support_path(p)]
        if not src:
            funnel["no_source_file_changed"] += 1
            continue
        if len(src) > 3:
            funnel["more_than_three_source_files"] += 1
            continue
        if len(files) > MAX_COMMIT_FILES:
            funnel["sprawling_commit"] += 1
            continue
        if not all((root / p).is_file() for p in src):
            funnel["a_file_is_gone_at_head"] += 1
            continue
        idents = {i.strip("_") for i in IDENT_RE.findall(query)}
        qtokens = tokens(query)
        if (any(os.path.basename(p) in query or p in query for p in src)
                or any(idents & names_by_file[p] for p in src)
                or any(contains_run(qtokens, list(run)) for p in src for run in runs_by_file[p])):
            funnel["answer_name_in_query"] += 1
            continue
        if any(p not in with_defs for p in src):
            funnel["kept_with_a_file_that_has_no_definition"] += 1
        key = "|".join(sorted(src))
        cid = challenge_id(corpus, "C", query, key)
        if cid in pool:
            funnel["duplicate"] += 1
            continue
        pool[cid] = {"id": cid, "kind": "C", "half": half_of(cid), "query": query,
                     "answer": {"files": sorted(src)}, "source": {"commit": commit[:12], "subject": subject}}
    funnel["kept"] = len(pool)
    return list(pool.values()), funnel


def cmd_challenges(args):
    paths = lab_paths(args)
    out_dir = paths["challenges"]
    out_dir.mkdir(parents=True, exist_ok=True)
    pgr = Pgr(find_pgr(args.pgr), paths["home"])
    registry = verify_corpora(paths, args.corpus or [n for n in CORPORA if n in read_json(registry_path(paths), {"corpora": {}})["corpora"]])
    index = {"seed": CHALLENGE_SEED, "max_per_kind": MAX_PER_KIND, "corpora": {}}
    for name in registry:
        if args.corpus and name not in args.corpus:
            continue
        entry = registry[name]
        root = paths["corpora"] / name
        exts = set(entry["exts"])
        defs, unparsed = load_defs(pgr, root)
        tm = TestMap(root)
        if entry["definitions"] is not None and len(defs) != entry["definitions"]:
            log(f"warning: {name}: index reports {entry['definitions']} symbols, hubs listed {len(defs)}")
        stats = {
            "definitions": len(defs),
            "functions": sum(d["func"] for d in defs),
            "test_region_definitions": sum(tm.in_test(d["path"], d["first"]) for d in defs),
            "hub_lines_unparsed": unparsed,
        }
        made = {}
        made["T"] = make_tests(name, defs, tm, exts)
        made["D"] = make_docs(name, defs, tm, exts)
        if (root / ".git").is_dir() or entry.get("history"):
            made["C"] = make_commits(name, entry, defs, root, exts)
        else:
            made["C"] = ([], collections.Counter({"no_history": 1}))
        chosen, funnel, counts = [], {}, {}
        for kind in KINDS:
            pool, f = made[kind]
            f = dict(f)
            pool = drop_ambiguous(pool, f)
            kept = sample_pool(pool, name, kind)
            f["pool"] = len(pool)
            f["sampled"] = len(kept)
            funnel[kind] = f
            counts[kind] = {h: sum(1 for c in kept if c["half"] == h) for h in HALVES}
            chosen += kept
        c_rows = [c for c in chosen if c["kind"] == "C"]
        if c_rows:   # file-level answers pile up on a few files; a reader should see how much
            top = collections.Counter(f for c in c_rows for f in c["answer"]["files"]).most_common(1)[0][0]
            stats["c_top_file"] = top
            stats["c_top_file_share"] = round(sum(top in c["answer"]["files"] for c in c_rows) / len(c_rows), 3)
        doc = {"corpus": name, "commit": entry["commit"], "seed": CHALLENGE_SEED, "max_per_kind": MAX_PER_KIND,
               "stats": stats, "funnel": funnel, "counts": counts, "challenges": chosen}
        write_json(out_dir / f"{name}.json", doc)
        ids = "\n".join(sorted(c["id"] for c in chosen))
        index["corpora"][name] = {"commit": entry["commit"], "stats": stats, "funnel": funnel, "counts": counts,
                                  "ids_sha256": sha256_bytes(ids.encode("ascii"))}
        log(f"{name}: " + ", ".join(f"{k} pool {funnel[k]['pool']} sampled {funnel[k]['sampled']}" for k in KINDS))
    old = read_json(out_dir / "index.json", {"corpora": {}})
    old["corpora"].update(index["corpora"])
    old.update({"seed": CHALLENGE_SEED, "max_per_kind": MAX_PER_KIND})
    old["sets_sha256"] = sha256_bytes("\n".join(f"{n}:{old['corpora'][n]['ids_sha256']}" for n in sorted(old["corpora"])).encode("ascii"))
    write_json(out_dir / "index.json", old)
    if args.show:
        show_challenges(out_dir, args.show, args.corpus)
    return 0


def show_challenges(out_dir, count, only):
    rng = random.Random(CHALLENGE_SEED)
    for path in sorted(out_dir.glob("*.json")):
        if path.name == "index.json":
            continue
        doc = read_json(path)
        if only and doc["corpus"] not in only:
            continue
        for kind in KINDS:
            rows = [c for c in doc["challenges"] if c["kind"] == kind]
            for c in rng.sample(rows, min(count, len(rows))):
                ans = c["answer"]
                where = ", ".join(ans["files"]) if "files" in ans else f'{ans["name"]} ({ans["path"]})'
                src = c["source"].get("subject") or c["source"].get("test") or ""
                print(f'[{doc["corpus"]} {kind} {c["id"]} {c["half"]}] src="{src}"\n    query: {c["query"]}\n    answer: {where}')


# ---------------------------------------------------------------- evaluation

def parse_variant(spec, default_pgr):
    """NAME=ARGS. ARGS are split like a shell line (use forward slashes in paths). Two special words:
    @pgr=PATH picks another binary, @env:KEY=VALUE sets an environment variable for that variant."""
    name, sep, rest = spec.partition("=")
    if not sep or not re.fullmatch(r"[A-Za-z0-9_.-]+", name):
        sys.exit(f"bad --variant {spec!r}: expected NAME=ARGS with NAME made of letters, digits, _ . -")
    variant = {"name": name, "args": [], "pgr": default_pgr, "env": {}}
    for tok in shlex.split(rest, posix=False):
        tok = tok.strip('"').strip("'") if tok[:1] in "\"'" and tok[-1:] == tok[:1] else tok
        if tok.startswith("@pgr="):
            variant["pgr"] = tok[5:]
        elif tok.startswith("@env:") and "=" in tok:
            key, _, value = tok[5:].partition("=")
            variant["env"][key] = value
        else:
            variant["args"].append(tok)
    if "-k" in variant["args"]:
        sys.exit(f"variant {name}: -k is set by the lab ({K} hits)")
    return variant


def load_variants(args):
    default_pgr = find_pgr(args.pgr)
    variants = []
    for spec in args.variant or []:
        variants.append(parse_variant(spec, default_pgr))
    for path in args.variants_file or []:
        for item in read_json(path):
            variants.append({"name": item["name"], "args": list(item.get("args", [])), "env": dict(item.get("env", {})),
                             "pgr": item.get("pgr") or default_pgr})
    if not variants:
        variants = [{"name": n, "args": a, "pgr": default_pgr, "env": {}} for n, a in DEFAULT_VARIANTS.items()]
    names = [v["name"] for v in variants]
    if len(set(names)) != len(names):
        sys.exit("duplicate variant names")
    return variants


def index_fingerprint(home, corpus):
    """Content hash of the index folder of one corpus, so a rebuilt index never reuses cached answers."""
    digest = hashlib.sha256()
    folders = sorted(p for p in pathlib.Path(home).glob(f"{corpus}-*") if re.fullmatch(re.escape(corpus) + r"-[0-9a-f]{12}", p.name))
    for folder in folders:
        for path in sorted(p for p in folder.rglob("*") if p.is_file() and p.name != "refresh.guard"):
            digest.update(path.relative_to(folder).as_posix().encode("utf-8") + b"\0")
            digest.update(sha256_file(path).encode("ascii"))
    return digest.hexdigest()


class CallCache:
    """Answers of one (binary, corpus commit, index, variant), keyed by query. Written once per group."""

    def __init__(self, path, fresh):
        self.path = path
        self.lock = threading.Lock()
        self.data = {"format": CACHE_FORMAT, "hits": {}, "timing": {}}
        if path.exists() and not fresh:
            loaded = read_json(path)
            if loaded.get("format") == CACHE_FORMAT:
                self.data = loaded

    def save(self):
        write_json(self.path, self.data)


def run_group(variant, corpus, queries, timing_queries, ctx, cache_dir, threads, fresh):
    """Answers (query -> hits) and serial timings for one variant on one corpus."""
    exe = variant["pgr"]
    pgr_hash = ctx["binary_hashes"].setdefault(exe, sha256_file(exe))
    env = dict(variant["env"])
    home = env.get("LXR_HOME", ctx["home"])
    fp = index_fingerprint(home, corpus)
    vkey = sha256_bytes(json.dumps([variant["args"], sorted(env.items()), K]).encode("utf-8"))[:12]
    commit = ctx["registry"][corpus]["commit"]
    path = cache_dir / f"{pgr_hash[:12]}_{corpus}_{commit[:12]}_{fp[:12]}_{vkey}.json"
    cache = CallCache(path, fresh)
    runner = Pgr(exe, home, env)
    root = ctx["corpora"] / corpus
    def one(query):
        out, _, millis = runner.call(root, ["find", *variant["args"], "-k", str(K), "--", query])
        return query, parse_hits(out), millis

    todo = sorted(q for q in set(queries) if q not in cache.data["hits"])
    if todo:
        with concurrent.futures.ThreadPoolExecutor(max_workers=threads) as pool:
            for query, hits, _ in pool.map(one, todo):
                cache.data["hits"][query] = hits
    for query in timing_queries:
        if query not in cache.data["timing"]:
            _, hits, millis = one(query)
            cache.data["timing"][query] = round(millis, 2)
            cache.data["hits"].setdefault(query, hits)
    if todo or any(q not in cache.data["timing"] for q in timing_queries) or not path.exists():
        cache.save()
    return cache.data["hits"], [cache.data["timing"][q] for q in timing_queries], len(todo)


def hit_matches(hit, answer, tm, mode):
    """Does one hit credit the answer?
    C: any hit in a changed file.
    T and D, mode "exact": the definition row of the function, or a chunk that overlaps the function or the
    comment lines directly above it (a doc comment is a chunk of its own).
    T and D, mode "cover": exact, or any hit in the same file whose lines contain the whole function (the
    class or impl block the engine returns for a method)."""
    name, path, first, last = hit
    if "files" in answer:
        return path in answer["files"]
    if path != answer["path"]:
        return False
    if name is not None and name == answer["name"]:
        return True
    if name is None and first <= answer["last"] and tm.lead_start(path, answer["first"]) <= last:
        return True
    return mode == "cover" and first <= answer["first"] and answer["last"] <= last


def rank_of(hits, answer, tm, mode):
    """1-based rank of the first crediting hit once hits in test code are skipped; 0 when absent."""
    position = 0
    for hit in hits:
        if tm.hit_is_test(hit):
            continue
        position += 1
        if hit_matches(hit, answer, tm, mode):
            return position
    return 0


def encode_ranks(ranks):
    return "".join("0123456789abcdefghijk"[r] for r in ranks)


def decode_ranks(text):
    return ["0123456789abcdefghijk".index(c) for c in text]


def cmd_run(args):
    paths = lab_paths(args)
    variants = load_variants(args)
    halves = HALVES if args.half == "both" else (args.half,)
    index = read_json(paths["challenges"] / "index.json")
    if not index:
        sys.exit("no challenges yet: run `challenges` first")
    registry = verify_corpora(paths, list(index["corpora"]))
    out_path = pathlib.Path(args.out)
    result = read_json(out_path, None)
    if result and result.get("challenge_sets_sha256") != index["sets_sha256"] and not args.reset:
        sys.exit(f"{out_path} was made from a different challenge set; pass --reset to start it again")
    if not result or args.reset:
        result = {"evaluation": {}, "variants": {}, "time_ms": {}}
    ctx = {"registry": registry, "corpora": paths["corpora"], "home": str(paths["home"]), "binary_hashes": {}}
    paths["cache"].mkdir(parents=True, exist_ok=True)
    docs = {name: read_json(paths["challenges"] / f"{name}.json") for name in index["corpora"]
            if not args.corpus or name in args.corpus}
    maps = {name: TestMap(paths["corpora"] / name) for name in docs}

    # one cell per half, corpus and kind: the challenge ids in order, and one rank string per variant
    cells = {}
    for half in halves:
        evaluation = result["evaluation"].setdefault(half, {"cells": []})
        existing = {(c["corpus"], c["kind"]): c for c in evaluation["cells"]}
        for name, doc in docs.items():
            for kind in KINDS:
                rows = sorted((c for c in doc["challenges"] if c["kind"] == kind and c["half"] == half), key=lambda c: c["id"])
                if not rows:
                    continue
                ids = [r["id"] for r in rows]
                cell = existing.get((name, kind))
                if cell is None or cell["ids"] != ids:
                    cell = existing[(name, kind)] = {"corpus": name, "kind": kind, "n": len(rows), "ids": ids,
                                                     "ranks": {}, "exact": {}}
                cells[(half, name, kind)] = (cell, rows)
        evaluation["cells"] = [existing[k] for k in sorted(existing)]

    for variant in variants:
        timings = []
        result["time_ms"][variant["name"]] = {}
        for name, doc in docs.items():
            ordered = sorted(doc["challenges"], key=lambda c: c["id"])
            wanted = [c["query"] for c in ordered if c["half"] in halves]
            sample = [c["query"] for c in ordered[:TIME_SAMPLE]]
            started = time.perf_counter()
            hits, millis, called = run_group(variant, name, wanted, sample, ctx, paths["cache"], args.threads, args.fresh)
            log(f"{variant['name']:<10} {name:<11} {len(set(wanted))} queries, {called} called, {time.perf_counter() - started:.1f}s")
            timings += millis
            result["time_ms"][variant["name"]][name] = round(statistics.median(millis), 1) if millis else None
            for half in halves:
                for kind in KINDS:
                    if (half, name, kind) in cells:
                        cell, rows = cells[(half, name, kind)]
                        for mode, field in (("cover", "ranks"), ("exact", "exact")):
                            cell.setdefault(field, {})[variant["name"]] = encode_ranks(
                                [rank_of(hits[c["query"]], c["answer"], maps[name], mode) for c in rows])
        result["time_ms"][variant["name"]]["all"] = round(statistics.median(timings), 1) if timings else None
        exe = variant["pgr"]
        version = subprocess.run([exe, "--version"], capture_output=True, stdin=subprocess.DEVNULL).stdout.decode("utf-8", "replace").strip()
        result["variants"][variant["name"]] = {
            "args": variant["args"], "env_keys": sorted(variant["env"]), "pgr_version": version,
            "pgr_sha256": ctx["binary_hashes"].get(exe) or sha256_file(exe),
        }
        shown = {k: v for k, v in sorted(variant["env"].items()) if k.startswith("PGR_")}
        if shown:
            result["variants"][variant["name"]]["env"] = shown
    finish_result(result, index, registry, args)
    write_json(out_path, result)
    log(f"wrote {out_path}")
    print_report(result, "held-out" if args.half == "both" else args.half)
    return 0


def finish_result(result, index, registry, args):
    today = datetime.date.today().isoformat()
    result.update({
        "name": "search_lab", "schema": 1, "measured": today, "k": K,
        "what": "Recall and reciprocal rank of `pgr find` variants on challenges made from the corpora themselves, with no model and no labels.",
        "challenge_sets_sha256": index["sets_sha256"],
        "bootstrap": {"resamples": BOOT_RESAMPLES, "seed": BOOT_SEED, "interval": 0.95, "paired": "same resampled challenges for both variants"},
        "script_sha256": sha256_file(pathlib.Path(__file__)),
        "corpora": {n: {k: v for k, v in registry[n].items() if k not in ("history",)}
                    for n in index["corpora"]},
        "challenges": {"seed": index["seed"], "max_per_kind": index["max_per_kind"], "kinds": KIND_TITLES,
                       "per_corpus": {n: {k: v for k, v in c.items() if k != "commit"} for n, c in index["corpora"].items()}},
        "rules": rules_text(),
        "limits": limits_text(),
    })
    baseline = args.baseline if args.baseline in result["variants"] else None
    for half, evaluation in result["evaluation"].items():
        evaluation["metrics"], evaluation["paired"] = compute_stats(evaluation["cells"], result["time_ms"], baseline, "ranks")
        evaluation["metrics_exact"], evaluation["paired_exact"] = compute_stats(evaluation["cells"], result["time_ms"], baseline, "exact")
        evaluation["baseline"] = baseline


def rules_text():
    return {
        "T": "A test function (in test code, named test*, or marked #[test]) whose name holds, as a run of whole name tokens (split on underscores and capitals), the name of exactly one non-test function of the corpus; a run inside a longer matching run does not count. That function must be defined once in the corpus, called from at most 40 places (a function used everywhere is set-up code in most tests), and called by the test: a call in the call graph (pgr callers), or for a name of two or more tokens a call-shaped mention in the test body. A one-token name must open the test name and be a graph call, because in a sentence-style test name a one-token function name is mostly a coincidence of English words. Query: the remaining words of the test name (test, tests, it, should dropped) plus the test's first doc line when it has one; at least 2 words must be left. Answer: the function.",
        "C": "A non-merge commit that changed 1 to 3 source files (extension of the corpus language, outside test paths), all still present at HEAD, in at most 8 changed paths of any kind. Query: the subject with a leading tag, issue numbers and trailing punctuation removed; at least 3 words and at least 3 content words (not a stop word or a generic change verb such as fix, add, update, use, refactor); subjects matching the drop list are dropped first. Answer: any hit in one of those files, a definition or a chunk of the file.",
        "D": "Control. Query: a function's own doc line (at least 3 words, unique in the corpus). Answer: that function. Word search should be near perfect here; a low score means the harness is broken.",
        "all": "A query never holds the full name of its answer (identifier or whole-token run; for C, the file name, its path or the name of a definition in the answer files). A query that two challenges share with different answers is dropped. An answer is never in an example, doc, docs_src, compat or legacy folder: the engine under test scales most of those paths down on purpose (rank.rs path_penalty) and documentation folders hold build configuration. Hits in test code are skipped before ranking: a path with a test, tests, testing, __tests__, spec or specs folder, a file named test_*, *_test, *_tests, *.test.*, *.spec.*, tests, test or conftest, and Rust code inside a #[cfg(test)] module or marked #[test]. At most 400 challenges per kind and corpus, sampled with a fixed seed. Id is the first 64 bits of a SHA-256 of corpus, kind, query and answer; an odd id is held-out, an even id is tune. Metrics are reported for held-out. Credit for T and D, cover matching (the main numbers): the definition row of the function, a chunk that overlaps the function or the comment lines directly above it, or any hit in the same file whose lines contain the whole function (the class or impl block the engine returns for a method). Exact matching leaves out the containing block. C is file-level in both.",
        "drop_subject_patterns": DROP_SUBJECT,
        "generic_words": sorted(GENERIC_WORDS),
    }


def limits_text():
    return [
        "Queries come from developer-written test names and commit subjects, not from questions an agent would ask.",
        "File-level credit for C is lenient: any hit in a changed file counts.",
        "Five corpora in three languages; JavaScript has no named tests (mocha callbacks), so T is empty there and express is judged on C alone.",
        "The hybrid variant was built by people who had seen none of these challenges. Any variant tuned later on the tune half must be judged on the held-out half only.",
        "Pooled rows weight each challenge equally, so the larger corpora count more.",
        "A test-name query and its answer share vocabulary by construction; the numbers compare variants, they are not a forecast of agent behaviour.",
        "Cover matching credits a block that contains the function; a variant that returns coarser blocks gains from it. The exact-matching numbers are kept beside the main ones for that reason.",
        "Answers in example, doc, docs_src, compat and legacy folders are left out because the engine demotes most of them on purpose; a change to that policy is not measured here.",
        "A variant whose weights were searched with `tune` was searched on the tune half only and is judged here on the held-out half; the tune half holds few T and a few hundred C challenges, so a tuned variant can fit it and gain nothing held-out.",
    ]


# ---------------------------------------------------------------- statistics

def metric_arrays(ranks):
    r = np.array(ranks, dtype=np.int64)
    return {
        "recall1": (r == 1).astype(float),
        "recall5": ((r >= 1) & (r <= 5)).astype(float),
        "recall10": ((r >= 1) & (r <= 10)).astype(float),
        "mrr": np.where(r > 0, 1.0 / np.maximum(r, 1), 0.0),
    }


METRICS = ("recall1", "recall5", "recall10", "mrr")


def boot_indices(n, group):
    seed = int(sha256_bytes(f"{BOOT_SEED}:{group}".encode("utf-8"))[:8], 16)
    return np.random.default_rng(seed).integers(0, n, size=(BOOT_RESAMPLES, n))


def interval(values, idx):
    means = values[idx].mean(axis=1)
    lo, hi = np.percentile(means, [2.5, 97.5])
    return {"v": round(float(values.mean()), 4), "lo": round(float(lo), 4), "hi": round(float(hi), 4)}


def compute_stats(cells, time_ms, baseline, field="ranks"):
    """Metric rows per variant, corpus and kind (plus pooled), and paired differences against the baseline."""
    variants = sorted({v for c in cells for v in c[field]})
    groups = collections.defaultdict(list)       # (scope, kind) -> cells
    for cell in cells:
        groups[(cell["corpus"], cell["kind"])].append(cell)
        groups[("pooled", cell["kind"])].append(cell)
    metrics, paired = [], []
    for (scope, kind), members in sorted(groups.items(), key=lambda kv: (KINDS.index(kv[0][1]), kv[0][0] != "pooled", kv[0][0])):
        members = sorted(members, key=lambda c: c["corpus"])
        n = sum(c["n"] for c in members)
        idx = boot_indices(n, f"{scope}|{kind}")
        arrays = {}
        for v in variants:
            if all(v in c[field] for c in members):
                ranks = [r for c in members for r in decode_ranks(c[field][v])]
                arrays[v] = metric_arrays(ranks)
        for v, arr in arrays.items():
            row = {"variant": v, "scope": scope, "kind": kind, "n": n}
            for m in METRICS:
                row[m] = interval(arr[m], idx)
            t = time_ms.get(v, {})
            row["median_ms"] = t.get("all") if scope == "pooled" else t.get(scope)
            metrics.append(row)
        if baseline and baseline in arrays:
            for v, arr in arrays.items():
                if v == baseline:
                    continue
                row = {"variant": v, "against": baseline, "scope": scope, "kind": kind, "n": n}
                for m in METRICS:
                    d = arr[m] - arrays[baseline][m]
                    row[m] = interval(d, idx)
                    row[m]["excludes_zero"] = bool(row[m]["lo"] > 0 or row[m]["hi"] < 0)
                paired.append(row)
    return metrics, paired


# ---------------------------------------------------------------- tuning

# The engine's own values of the fusion weights (rust/crates/lxr-rag/src/tuning.rs): the starting
# point of every search. `words` is not searched: only the ratio of words to meaning matters.
WEIGHT_DEFAULTS = {"names": 1.0, "files": 1.0, "meaning": 1.0, "k": 60.0, "doc": 0.0, "refine": 0.5, "terms": 8.0, "repo": 0.5}
# A parameter and the values tried for it. The search moves one parameter at a time.
TUNE_GRID = (
    ("names", (0, 1, 2)),
    ("files", (0, 1, 2)),
    ("meaning", (0.5, 1, 2)),
    ("k", (30, 60, 120)),
    ("doc", (0, 1)),
    ("refine", (0.25, 0.5, 1)),
    ("terms", (4, 8, 16)),
    ("repo", (0.25, 0.5, 0.75)),
)
REFINE_N_GRID = (1, 3, 5)               # how many best hits a refinement pass learns from
TUNE_KINDS = ("T", "C")                 # the kinds the search is scored on; D is a control and never tunes anything
TUNE_SWEEPS = 2                         # passes over the parameters
TUNE_MIN_GAIN = 0.002                   # a value must beat the current one by this much mean reciprocal rank to be taken
TUNE_SEED = 20261014                    # decides the order the parameters are visited in


def env_number(env, name):
    try:
        return int(env.get(name, "0") or 0)
    except ValueError:
        sys.exit(f"{name} must be a whole number, got {env.get(name)!r}")


def tunable_params(variant):
    """The parameters that can change the answers of a variant: the weights of the stages it uses."""
    env = variant["env"]
    if "PGR_SEARCH_WEIGHTS" in env:
        sys.exit(f"variant {variant['name']}: PGR_SEARCH_WEIGHTS belongs to the search, not to the base variant")
    words_only = "--words" in variant["args"]
    refine_on = env_number(env, "PGR_SEARCH_REFINE") > 0
    vectors = env.get("PGR_SEARCH_VECTORS") or "model"
    params = ["names", "files"]
    if not words_only:
        params.append("meaning")
    params += ["k", "doc"]
    if refine_on:
        params += ["refine", "terms", "N"]
    if vectors == "both" and not words_only:
        params.append("repo")
    return params


def weights_text(values):
    """PGR_SEARCH_WEIGHTS for the parameters that are not at the engine's value, in a fixed order."""
    parts = [f"{k}={values[k]:g}" for k in sorted(values) if k != "N" and values[k] != WEIGHT_DEFAULTS[k]]
    return ",".join(parts)


def with_params(base, values):
    """The base variant with the weights (and the number of refined hits) of `values`."""
    env = dict(base["env"])
    text = weights_text(values)
    if text:
        env["PGR_SEARCH_WEIGHTS"] = text
    if "N" in values:
        env["PGR_SEARCH_REFINE"] = str(int(values["N"]))
    return {"name": base["name"], "args": list(base["args"]), "pgr": base["pgr"], "env": env}


class Objective:
    """Mean reciprocal rank on the tune half, T and C each pooled over the corpora, then averaged.
    Every call asks pgr (answers are cached by binary, index, variant and query) and counts."""

    def __init__(self, docs, maps, ctx, cache_dir, threads):
        self.docs, self.maps, self.ctx, self.cache_dir, self.threads = docs, maps, ctx, cache_dir, threads
        self.calls = 0

    def __call__(self, variant):
        self.calls += 1
        ranks = {kind: [] for kind in TUNE_KINDS}
        for name, doc in self.docs.items():
            rows = [c for c in doc["challenges"] if c["half"] == "tune" and c["kind"] in TUNE_KINDS]
            if not rows:
                continue
            hits, _, _ = run_group(variant, name, [c["query"] for c in rows], [], self.ctx, self.cache_dir, self.threads, False)
            for c in rows:
                ranks[c["kind"]].append(rank_of(hits[c["query"]], c["answer"], self.maps[name], "cover"))
        per_kind = {k: round(float(metric_arrays(r)["mrr"].mean()), 4) for k, r in ranks.items() if r}
        if not per_kind:
            sys.exit("the tune half holds no T or C challenge")
        return round(sum(per_kind.values()) / len(per_kind), 4), per_kind


def coordinate_search(base, objective, sweeps, rng):
    """Moves one parameter at a time over its grid, keeping a value only when it beats the current
    one by TUNE_MIN_GAIN, for a fixed number of sweeps in an order the seed decides. Starts at the
    engine's values, so a tuned result is never worse than the default on the half it was tuned on."""
    names = tunable_params(base)
    values = {k: WEIGHT_DEFAULTS[k] for k in names if k != "N"}
    if "N" in names:
        values["N"] = env_number(base["env"], "PGR_SEARCH_REFINE")
    grids = dict(TUNE_GRID)
    grids["N"] = REFINE_N_GRID
    best, per_kind = objective(with_params(base, values))
    start = (best, per_kind)
    trace = [{"params": dict(values), "objective": best, **per_kind}]
    for sweep in range(sweeps):
        order = list(names)
        rng.shuffle(order)
        moved = False
        for param in order:
            kept = values[param]
            for candidate in grids[param]:
                if candidate == kept:
                    continue
                trial = dict(values)
                trial[param] = candidate
                score, kinds = objective(with_params(base, trial))
                trace.append({"params": dict(trial), "objective": score, **kinds})
                if score > best + TUNE_MIN_GAIN:
                    best, per_kind, values, moved = score, kinds, trial, True
        if not moved:
            break
    return values, best, per_kind, start, trace


def cmd_tune(args):
    paths = lab_paths(args)
    bases = load_variants(args)
    index = read_json(paths["challenges"] / "index.json")
    if not index:
        sys.exit("no challenges yet: run `challenges` first")
    registry = verify_corpora(paths, list(index["corpora"]))
    ctx = {"registry": registry, "corpora": paths["corpora"], "home": str(paths["home"]), "binary_hashes": {}}
    paths["cache"].mkdir(parents=True, exist_ok=True)
    docs = {name: read_json(paths["challenges"] / f"{name}.json") for name in index["corpora"]
            if not args.corpus or name in args.corpus}
    maps = {name: TestMap(paths["corpora"] / name) for name in docs}
    objective = Objective(docs, maps, ctx, paths["cache"], args.threads)
    rng = random.Random(args.seed)

    counts = {kind: sum(1 for d in docs.values() for c in d["challenges"] if c["half"] == "tune" and c["kind"] == kind)
              for kind in TUNE_KINDS}
    log(f"tune half: {counts}; objective = mean of pooled MRR over {', '.join(k for k in TUNE_KINDS if counts[k])}")

    # every base at the engine's weights, on the tune half; the best of them are tuned
    rows = []
    for base in bases:
        score, kinds = objective(base)
        rows.append((score, kinds, base))
        log(f"{base['name']:<16} default {score:.4f} {kinds}")
    chosen = {b["name"] for _, _, b in sorted(rows, key=lambda r: (-r[0], r[2]["name"]))[:args.select]}

    result = {
        "name": "search_lab_tuning", "schema": 1, "measured": datetime.date.today().isoformat(),
        "what": "Weights of the fusion searched on the tune half only. Judge a tuned variant on the held-out half.",
        "challenge_sets_sha256": index["sets_sha256"], "script_sha256": sha256_file(pathlib.Path(__file__)),
        "objective": "mean over the kinds T and C of the pooled mean reciprocal rank on the tune half (cover matching)",
        "tune_challenges": counts, "seed": args.seed, "sweeps": args.sweeps, "min_gain": TUNE_MIN_GAIN,
        "start": WEIGHT_DEFAULTS, "grid": dict({k: list(v) for k, v in TUNE_GRID}, N=list(REFINE_N_GRID)),
        "bases": {}, "tuned": {},
    }
    emitted = []
    for score, kinds, base in rows:
        name = base["name"]
        result["bases"][name] = {"args": base["args"], "env": {k: v for k, v in sorted(base["env"].items()) if k.startswith("PGR_")},
                                 "tune_objective": score, "tune_mrr": kinds, "tuned": name in chosen}
        if name not in chosen:
            continue
        values, best, per_kind, start, trace = coordinate_search(base, objective, args.sweeps, rng)
        tuned = with_params(base, values)
        log(f"{name:<16} tuned   {best:.4f} {per_kind} from {start[0]:.4f}: {weights_text(values) or '(the default)'}"
            + (f" refine={values['N']}" if "N" in values else ""))
        result["tuned"][name] = {
            "weights": weights_text(values), "refine_n": int(values["N"]) if "N" in values else 0,
            "values": values, "tune_objective": best, "tune_mrr": per_kind,
            "default_objective": start[0], "evaluations": len(trace), "trace": trace,
        }
        emitted.append({"name": f"{name}-tuned", "args": tuned["args"], "env": tuned["env"], "pgr": tuned["pgr"]})
    result["queries_asked"] = objective.calls
    write_json(pathlib.Path(args.out), result)
    write_json(pathlib.Path(args.variants_out), emitted)
    log(f"wrote {args.out} and {args.variants_out}")
    return 0


# ---------------------------------------------------------------- report

def fmt(cell):
    return f'{cell["v"]:.3f} [{cell["lo"]:.3f},{cell["hi"]:.3f}]'


def fmt_diff(cell):
    mark = "*" if cell.get("excludes_zero") else " "
    return f'{cell["v"]:+.3f} [{cell["lo"]:+.3f},{cell["hi"]:+.3f}]{mark}'


def table(rows, header, widths):
    lines = ["  ".join(h.ljust(w) for h, w in zip(header, widths))]
    for row in rows:
        lines.append("  ".join(str(c).ljust(w) for c, w in zip(row, widths)))
    return "\n".join(lines)


def metric_rows(rows):
    return [(m["variant"], m["n"], fmt(m["recall1"]), fmt(m["recall5"]), fmt(m["recall10"]), fmt(m["mrr"]), m["median_ms"])
            for m in rows]


def print_metrics(rows, scopes, title):
    head = ("variant", "n", "R@1", "R@5", "R@10", "MRR", "ms")
    for scope in scopes:
        chosen = [m for m in rows if m["scope"] == scope]
        if chosen:
            print(f"-- {title}{scope}")
            print(table(metric_rows(chosen), head, (10, 5, 20, 20, 20, 20, 6)))


def print_paired(diffs, baseline, scopes):
    chosen = [p for p in diffs if p["scope"] in scopes]
    if chosen:
        print(f"-- paired difference against {baseline} (* interval excludes zero)")
        head = ("variant", "scope", "n", "dR@1", "dR@5", "dR@10", "dMRR")
        show = [(p["variant"], p["scope"], p["n"], fmt_diff(p["recall1"]), fmt_diff(p["recall5"]),
                 fmt_diff(p["recall10"]), fmt_diff(p["mrr"])) for p in chosen]
        print(table(show, head, (10, 11, 5, 21, 21, 21, 21)))


def shown_env(variant):
    env = variant.get("env")
    return " env " + ",".join(f"{k}={v}" for k, v in env.items()) if env else ""


def print_report(result, half):
    evaluation = result["evaluation"].get(half)
    if not evaluation:
        print(f"no evaluation for half {half!r} in the result file")
        return
    print(f'search lab, measured {result["measured"]}, half {half}, {result["k"]} hits per query, '
          f'95% bootstrap intervals ({result["bootstrap"]["resamples"]} resamples)')
    print("variants: " + "; ".join(f'{n} = find {" ".join(v["args"]) or "(default)"}{shown_env(v)} [{v["pgr_version"]}, {v["pgr_sha256"][:8]}]'
                                   for n, v in sorted(result["variants"].items())))
    metrics, paired, baseline = evaluation["metrics"], evaluation["paired"], evaluation["baseline"]
    corpora = list(result["corpora"])
    for kind in KINDS:
        rows = [m for m in metrics if m["kind"] == kind]
        if not rows:
            continue
        print(f"\n== {kind}: {KIND_TITLES[kind]}")
        print_metrics([m for m in rows if m["kind"] == kind], ["pooled"] + corpora, "")
        print_paired([p for p in paired if p["kind"] == kind], baseline, ["pooled"] + corpora)
    print("\n== exact matching (T and D: only the function itself or its comment counts, not the block around it)")
    exact = evaluation.get("metrics_exact", [])
    for kind in ("T", "D"):
        print_metrics([m for m in exact if m["kind"] == kind], ["pooled"], f"{kind} ")
    control = [m for m in metrics if m["kind"] == "D" and m["variant"] == "words" and m["scope"] != "pooled"]
    strict = {m["scope"]: m for m in exact if m["kind"] == "D" and m["variant"] == "words"}
    if control:
        print("\ncontrol D, words variant, recall at 5 per corpus (cover matching; exact in brackets; must be >= 0.9):")
        for m in control:
            value = m["recall5"]["v"]
            print(f'  {m["scope"]:<11} n={m["n"]:<4} {value:.3f} [{strict[m["scope"]]["recall5"]["v"]:.3f}] {"ok" if value >= 0.9 else "BELOW 0.9"}')


def cmd_report(args):
    result = read_json(args.out)
    if not result:
        sys.exit(f"{args.out} does not exist")
    print_report(result, args.half)
    return 0


# ---------------------------------------------------------------- command line

def build_parser():
    default_out = pathlib.Path(__file__).resolve().parent.parent / "data" / "search_lab.json"
    default_repo = pathlib.Path(__file__).resolve().parent.parent.parent
    parser = argparse.ArgumentParser(description="Search lab: model-free challenges and an evaluator for `pgr find`.")
    sub = parser.add_subparsers(dest="command", required=True)

    def common(p):
        p.add_argument("--lab", help="lab folder (or SEARCH_LAB_DIR); corpora, home, cache and challenges live under it")
        p.add_argument("--corpora", help="corpora folder (default LAB/corpora)")
        p.add_argument("--home", help="index home, passed to pgr as LXR_HOME (default LAB/home)")
        p.add_argument("--pgr", help="pgr binary (default: pgr on PATH)")
        p.add_argument("--corpus", nargs="*", help="only these corpora: " + ", ".join(CORPORA))

    p = sub.add_parser("prepare", help="create the corpora and build their indexes")
    common(p)
    p.add_argument("--repo-root", default=str(default_repo), help="this repository, source of the pgr-engine corpus")
    p.add_argument("--fixture-src", help="the fixture repository (default LAB/../repo)")
    p.add_argument("--no-clone", action="store_true", help="never clone; fail for a missing clone")
    p.add_argument("--force", action="store_true", help="pass --force to pgr refresh")
    p.set_defaults(func=cmd_prepare)

    p = sub.add_parser("verify", help="check that no corpus file changed since prepare")
    common(p)
    p.set_defaults(func=cmd_verify)

    p = sub.add_parser("challenges", help="make and store the challenge sets")
    common(p)
    p.add_argument("--challenges-dir", help="where the sets are stored (default LAB/challenges)")
    p.add_argument("--show", type=int, default=0, help="print N challenges of each kind and corpus")
    p.set_defaults(func=cmd_challenges)

    p = sub.add_parser("run", help="evaluate variants; adds or replaces their rows in the result file")
    common(p)
    p.add_argument("--challenges-dir", help="where the sets are stored (default LAB/challenges)")
    p.add_argument("--cache", help="answer cache folder (default LAB/cache)")
    p.add_argument("--variant", action="append", metavar="NAME=ARGS",
                   help="a variant: name and extra `pgr find` arguments, e.g. words=--words; also @pgr=PATH and @env:KEY=VALUE. "
                        "Default: words=--words, meaning=--meaning, hybrid=")
    p.add_argument("--variants-file", action="append", help="JSON list of {name, args, pgr, env}")
    p.add_argument("--out", default=str(default_out), help="result file (default benchmark/data/search_lab.json)")
    p.add_argument("--half", choices=("held-out", "tune", "both"), default="held-out", help="which half to evaluate")
    p.add_argument("--baseline", default="hybrid", help="variant the paired differences are taken against")
    p.add_argument("--threads", type=int, default=min(8, os.cpu_count() or 4))
    p.add_argument("--fresh", action="store_true", help="ignore cached answers (they are rewritten)")
    p.add_argument("--reset", action="store_true", help="start the result file again")
    p.set_defaults(func=cmd_run)

    p = sub.add_parser("tune", help="search the fusion weights of variants on the tune half only")
    common(p)
    p.add_argument("--challenges-dir", help="where the sets are stored (default LAB/challenges)")
    p.add_argument("--cache", help="answer cache folder (default LAB/cache)")
    p.add_argument("--variant", action="append", metavar="NAME=ARGS", help="a base variant, as for `run`")
    p.add_argument("--variants-file", action="append", help="JSON list of {name, args, pgr, env} base variants")
    p.add_argument("--select", type=int, default=2, help="tune the best N bases, judged on the tune half at the engine's weights")
    p.add_argument("--sweeps", type=int, default=TUNE_SWEEPS, help="passes over the parameters")
    p.add_argument("--seed", type=int, default=TUNE_SEED, help="decides the order the parameters are visited in")
    p.add_argument("--out", default=str(pathlib.Path(__file__).resolve().parent / "tuned.json"), help="the search and its result")
    p.add_argument("--variants-out", default=str(pathlib.Path(__file__).resolve().parent / "tuned_variants.json"),
                   help="the tuned variants, as a file `run --variants-file` reads")
    p.add_argument("--threads", type=int, default=min(8, os.cpu_count() or 4))
    p.set_defaults(func=cmd_tune)

    p = sub.add_parser("report", help="print the tables of a result file")
    p.add_argument("--out", default=str(default_out), help="result file")
    p.add_argument("--half", choices=("held-out", "tune"), default="held-out")
    p.set_defaults(func=cmd_report)
    return parser


def main(argv=None):
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(errors="replace")   # queries hold any character; a legacy console must not stop the run
    args = build_parser().parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
