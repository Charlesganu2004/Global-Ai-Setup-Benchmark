"""Benchmark: graphify + semble, codanna (Rust, embeddings off), and all three combined.

Usage: combo_bench.py NAME SOURCE_DIR WORK_DIR PATH_TO_CODANNA
Windows only: it expects graphify, semble, ast-grep and ripgrep where setup/tools.py puts them.
"""
import collections
import json
import os
import pathlib
import re
import shutil
import statistics
import subprocess
import sys
import time

if len(sys.argv) != 5:
    sys.exit("usage: combo_bench.py NAME SOURCE_DIR WORK_DIR PATH_TO_CODANNA   (Windows; the tools where setup/tools.py puts them)")

HOME = pathlib.Path.home()
NAME, SRC, WORK = sys.argv[1], pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3])
GRAPHIFY = str(HOME / ".graphify/venv/Scripts/graphify.exe")
SEMBLE = str(HOME / "Tools/codesearch-venv/Scripts/semble.exe")
CODANNA = str(pathlib.Path(sys.argv[4]))
ASTGREP, RG = str(HOME / "bin/ast-grep.exe"), str(HOME / "bin/rg.exe")
SEMBLE_CACHE = pathlib.Path(os.environ["LOCALAPPDATA"]) / "semble" / "Cache"
ENV = dict(os.environ, HF_HUB_OFFLINE="1", GRAPHIFY_NO_TIPS="1", GRAPHIFY_NO_AUTO_REFRESH="1")
HEADLINE = re.compile(r"^\S.* at \S+:\d+")
SKIP = shutil.ignore_patterns("graphify-out", ".codanna", ".git", "__pycache__", "node_modules")


def run(argv, cwd=None):
    start = time.perf_counter()
    done = subprocess.run(argv, cwd=cwd, env=ENV, capture_output=True, stdin=subprocess.DEVNULL)
    return time.perf_counter() - start, done.stdout.decode("utf-8", "replace"), done.returncode


def together(jobs):
    start = time.perf_counter()
    procs = [subprocess.Popen(argv, cwd=cwd, env=ENV, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL) for argv, cwd in jobs]
    for proc in procs:
        proc.wait()
    return time.perf_counter() - start


def piped(first, second, cwd):
    """first | second, timed as one command, the way an agent would run it."""
    start = time.perf_counter()
    one = subprocess.Popen(first, cwd=cwd, env=ENV, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL)
    two = subprocess.Popen(second, cwd=cwd, env=ENV, stdin=one.stdout, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    one.stdout.close()
    raw = two.communicate()[0].decode("utf-8", "replace")
    one.wait()
    return time.perf_counter() - start, raw


def size_mb(path):
    return sum(f.stat().st_size for f in pathlib.Path(path).rglob("*") if f.is_file()) / 1e6


def fresh(tag):
    target = WORK / f"{NAME}-{tag}"
    if not target.exists():
        shutil.copytree(SRC, target, ignore=SKIP)
        # A real repository, with each engine's output hidden from the others the
        # way the session hook does it. Without this they re-index each other.
        subprocess.run(["git", "init", "-q"], cwd=target, capture_output=True)
        (target / ".git" / "info" / "exclude").write_text("graphify-out/\n.codanna/\n.codannaignore\n", encoding="utf-8")
    return target


def graph_cmd(root):
    return [GRAPHIFY, "extract", str(root), "--code-only", "--no-viz", "--no-cluster"], str(WORK)


def semble_cmd(root):
    return [SEMBLE, "search", "entry point", str(root), "-k", "1", "--max-snippet-lines", "0"], str(WORK)


def codanna_init(root):
    run([CODANNA, "init"], cwd=str(root))
    settings = root / ".codanna" / "settings.toml"
    text = settings.read_text(encoding="utf-8")
    text = re.sub(r"(\[semantic_search\][^\[]*?\nenabled = )true", r"\1false", text)
    settings.write_text(text, encoding="utf-8")
    with (root / ".codannaignore").open("a", encoding="utf-8") as stream:
        stream.write("\ngraphify-out/\n")


def codanna_cmd(root):
    return [CODANNA, "index", ".", "--no-progress"], str(root)


def edit(root):
    target = max(root.rglob("*.py"), key=lambda p: p.stat().st_size)
    with target.open("a", encoding="utf-8") as stream:
        stream.write("\n\ndef _bench_probe_%d():\n    return 1\n" % int(time.time()))


def tokens(text):
    return round(len(" ".join(text.split())) / 4)


out = {"dataset": NAME}
files = [p for p in SRC.rglob("*") if p.is_file() and not any(part in ("graphify-out", ".codanna", ".git") for part in p.parts)]
out["files"] = len(files)
out["python_files"] = sum(p.suffix == ".py" for p in files)
out["source_mb"] = round(sum(p.stat().st_size for p in files) / 1e6, 2)

# ---- builds, each engine alone on its own fresh copy ----------------------
cur, rust, combo = fresh("cur"), fresh("rust"), fresh("combo")
warm_g, warm_r, warm_c = fresh("cur-b"), fresh("rust-b"), fresh("combo-b")
run(*graph_cmd(warm_g)); run(*semble_cmd(warm_g)); codanna_init(warm_r); run(*codanna_cmd(warm_r))   # first pass absorbs disk-cache and antivirus first-touch cost
before = set(SEMBLE_CACHE.iterdir()) if SEMBLE_CACHE.exists() else set()
t_graph, _, _ = run(*graph_cmd(cur))
t_semble, _, _ = run(*semble_cmd(cur))
semble_mb = sum(size_mb(d) for d in set(SEMBLE_CACHE.iterdir()) - before)
codanna_init(rust)
t_codanna, _, _ = run(*codanna_cmd(rust))
codanna_init(warm_c)
together([graph_cmd(warm_c), semble_cmd(warm_c), codanna_cmd(warm_c)])
out["cold"] = {"graphify": t_graph, "semble": t_semble, "codanna": t_codanna}
out["disk_mb"] = {"graphify": size_mb(cur / "graphify-out"), "semble": semble_mb,
                  "codanna": size_mb(rust / ".codanna")}

# ---- combined: all three at once on one copy -------------------------------
codanna_init(combo)
out["cold"]["combined_parallel"] = together([graph_cmd(combo), semble_cmd(combo), codanna_cmd(combo)])

# ---- nothing changed, then one file changed --------------------------------
out["unchanged"] = {"graphify": run(*graph_cmd(cur))[0], "semble": run(*semble_cmd(cur))[0],
                    "codanna": run(*codanna_cmd(rust))[0],
                    "combined_parallel": together([graph_cmd(combo), semble_cmd(combo), codanna_cmd(combo)])}
for root in (cur, rust, combo):
    edit(root)
out["one_file"] = {"graphify": run(*graph_cmd(cur))[0], "semble": run(*semble_cmd(cur))[0],
                   "codanna": run(*codanna_cmd(rust))[0],
                   "combined_parallel": together([graph_cmd(combo), semble_cmd(combo), codanna_cmd(combo)])}

# ---- pick symbols: private top-level functions defined once, called from several files
_, defs, _ = run([RG, "-N", "--no-filename", "-o", "-r", "$1", r"^def (_[a-z][a-z_0-9]+)\(", "-t", "py",
                  "-g", "!graphify-out", str(combo)])
unique = sorted(name for name, count in collections.Counter(defs.split()).items() if count == 1)
ranked = []
for name in unique:
    _, hits, _ = run([RG, "-l", "-F", name + "(", "-t", "py", "-g", "!graphify-out", str(combo)])
    ranked.append((len(hits.split()), name))
ranked = sorted((c, n) for c, n in ranked if c >= 2)
picks = [ranked[int(i * (len(ranked) - 1) / 11)] for i in range(12)] if len(ranked) >= 12 else ranked
symbols = [name for _, name in picks]
LX = str(pathlib.Path(__file__).with_name("lx_proto.py"))
PIPE = [RG, "-o", "-r", "$1 $2", r"^(\S+) \(\w+\) at (\S+:\d+(?:-\d+)?)"]


def truth_files(name):
    _, raw, _ = run([ASTGREP, "run", "-p", name + "($$$)", "-l", "python", ".", "--json=stream"], cwd=str(combo))
    found = set()
    for line in raw.splitlines():
        try:
            found.add(json.loads(line)["file"].replace("\\", "/").lstrip("./"))
        except (ValueError, KeyError):
            pass
    return found


def graphify_callers(name):
    took, raw, _ = run([GRAPHIFY, "affected", name, "--depth", "1"], cwd=str(combo))
    found = {m.group(1).replace("\\", "/") for m in re.finditer(r"\[calls\] (\S+?):L\d+", raw)}
    return took, raw, found, "No unique node" in raw or "No node matching" in raw


def codanna_callers(name):
    took, raw = piped([CODANNA, "retrieve", "callers", name], PIPE, str(combo))
    found = {m.group(1).replace("\\", "/") for m in re.finditer(r" at (\S+?):\d+", raw)}
    plain = run([CODANNA, "retrieve", "callers", name], cwd=str(combo))[1]
    return took, raw, found, "Ambiguous" in plain


rows = []
for name in symbols:
    truth = truth_files(name)
    g_time, g_raw, g_found, g_miss = graphify_callers(name)
    c_time, c_raw, c_found, c_miss = codanna_callers(name)

    def score(found):
        if not truth:
            return None
        return len(found & truth) / len(truth)

    l_time, l_raw, _ = run([sys.executable, LX, CODANNA, "callers", name], cwd=str(combo))
    l_found = {m.group(1).replace("\\", "/") for m in re.finditer(r" (\S+?):\d+", l_raw)}
    rows.append({"symbol": name, "truth_files": len(truth), "callers_n": len(c_raw.splitlines()),
                 "lx": {"s": l_time, "tok": tokens(l_raw), "recall": score(l_found), "unresolved": c_miss},
                 "graphify": {"s": g_time, "tok": tokens(g_raw), "recall": score(g_found), "unresolved": g_miss},
                 "codanna": {"s": c_time, "tok": tokens(c_raw), "recall": score(c_found), "unresolved": c_miss}})
out["callers"] = rows

# ---- where is X defined -----------------------------------------------------
define = []
for name in symbols[:6]:
    g_time, g_raw, _ = run([GRAPHIFY, "explain", name], cwd=str(combo))
    c_time, c_raw = piped([CODANNA, "retrieve", "symbol", name], PIPE, str(combo))
    define.append({"graphify": (g_time, tokens(g_raw)), "codanna": (c_time, tokens(c_raw))})
out["define"] = define

# ---- semantic: only semble has it in the combined stack ---------------------
semantic = []
for query in ("where are hooks installed", "how is the cache invalidated", "merge two graphs together"):
    took, raw, _ = run([SEMBLE, "search", query, str(combo), "-k", "3", "--max-snippet-lines", "0", "--format", "text"], cwd=str(WORK))
    semantic.append((took, tokens(raw)))
out["semantic"] = semantic


def avg(values):
    values = [v for v in values if v is not None]
    return statistics.mean(values) if values else 0


summary = {
    "dataset": NAME, "files": out["files"], "python_files": out["python_files"], "source_mb": out["source_mb"],
    "cold_s": {k: round(v, 1) for k, v in out["cold"].items()},
    "unchanged_s": {k: round(v, 1) for k, v in out["unchanged"].items()},
    "one_file_s": {k: round(v, 1) for k, v in out["one_file"].items()},
    "disk_mb": {k: round(v, 1) for k, v in out["disk_mb"].items()},
    "symbols_tested": len(rows),
    "callers": {eng: {"seconds": round(avg([r[eng]["s"] for r in rows]), 2),
                      "tokens": round(avg([r[eng]["tok"] for r in rows])),
                      "file_recall": round(avg([r[eng]["recall"] for r in rows]), 2),
                      "unresolved": sum(r[eng]["unresolved"] for r in rows)} for eng in ("graphify", "codanna", "lx")},
    "callers_by_fan_in": {label: {eng: round(avg([r[eng]["tok"] for r in rows if lo <= r["callers_n"] <= hi])) for eng in ("graphify", "codanna", "lx")}
                          for label, lo, hi in (("1-5 callers", 0, 5), ("6-20 callers", 6, 20), ("over 20", 21, 10**6))},
    "command_tokens": {"graphify": tokens('graphify affected "some_symbol_name"'),
                       "codanna": tokens("codanna retrieve callers some_symbol_name | rg -o -r '$1 $2' '^(\\S+) \\(\\w+\\) at (\\S+:\\d+-\\d+)'"),
                       "lx": tokens("lx callers some_symbol_name")},
    "define": {eng: {"seconds": round(avg([d[eng][0] for d in define]), 2),
                     "tokens": round(avg([d[eng][1] for d in define]))} for eng in ("graphify", "codanna")},
    "semantic_semble": {"seconds": round(avg([s for s, _ in semantic]), 2), "tokens": round(avg([t for _, t in semantic]))},
}
print(json.dumps(summary, indent=1))
(WORK / f"{NAME}-detail.json").write_text(json.dumps(out, indent=1, default=str), encoding="utf-8")
