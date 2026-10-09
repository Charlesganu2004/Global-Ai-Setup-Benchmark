"""Rust-with-embeddings builds, disk breakdown and search quality.

Usage: emb_bench.py NAME SOURCE_DIR WORK_DIR PATH_TO_CODANNA
Windows only: it expects graphify and semble where setup/tools.py puts them.
"""
import ast
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

if len(sys.argv) != 5:
    sys.exit("usage: emb_bench.py NAME SOURCE_DIR WORK_DIR PATH_TO_CODANNA   (Windows; the tools where setup/tools.py puts them)")

HOME = pathlib.Path.home()
NAME, SRC, WORK, CODANNA = sys.argv[1], pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]), sys.argv[4]
GRAPHIFY = str(HOME / ".graphify/venv/Scripts/graphify.exe")
SEMBLE = str(HOME / "Tools/codesearch-venv/Scripts/semble.exe")
SEMBLE_CACHE = pathlib.Path(os.environ["LOCALAPPDATA"]) / "semble" / "Cache"
ENV = dict(os.environ, HF_HUB_OFFLINE="1", GRAPHIFY_NO_TIPS="1", GRAPHIFY_NO_AUTO_REFRESH="1")
SKIP = shutil.ignore_patterns("graphify-out", ".codanna", ".git", "__pycache__", "node_modules")


def run(argv, cwd=None):
    start = time.perf_counter()
    done = subprocess.run(argv, cwd=cwd, env=ENV, capture_output=True, stdin=subprocess.DEVNULL)
    return time.perf_counter() - start, done.stdout.decode("utf-8", "replace"), done.returncode


def together(jobs):
    start = time.perf_counter()
    procs = [subprocess.Popen(argv, cwd=cwd, env=ENV, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                              stdin=subprocess.DEVNULL) for argv, cwd in jobs]
    for proc in procs:
        proc.wait()
    return time.perf_counter() - start


def size_mb(path):
    path = pathlib.Path(path)
    return sum(f.stat().st_size for f in path.rglob("*") if f.is_file()) / 1e6 if path.exists() else 0.0


def fresh(tag):
    target = WORK / f"{NAME}-{tag}"
    if not target.exists():
        shutil.copytree(SRC, target, ignore=SKIP)
        subprocess.run(["git", "init", "-q"], cwd=target, capture_output=True)
        (target / ".git" / "info" / "exclude").write_text("graphify-out/\n.codanna/\n.codannaignore\n", encoding="utf-8")
    return target


def codanna_init(root, embeddings):
    run([CODANNA, "init"], cwd=str(root))
    settings = root / ".codanna" / "settings.toml"
    text = settings.read_text(encoding="utf-8")
    if not embeddings:
        text = re.sub(r"(\[semantic_search\][^\[]*?\nenabled = )true", r"\1false", text)
    settings.write_text(text, encoding="utf-8")
    with (root / ".codannaignore").open("a", encoding="utf-8") as stream:
        stream.write("\ngraphify-out/\n")


graph = lambda root: ([GRAPHIFY, "extract", str(root), "--code-only", "--no-viz", "--no-cluster"], str(WORK))
semble = lambda root: ([SEMBLE, "search", "entry point", str(root), "-k", "1", "--max-snippet-lines", "0"], str(WORK))
codanna = lambda root: ([CODANNA, "index", ".", "--no-progress"], str(root))


def edit(root):
    target = max(root.rglob("*.py"), key=lambda p: p.stat().st_size)
    with target.open("a", encoding="utf-8") as stream:
        stream.write("\n\ndef _bench_probe_%d():\n    return 1\n" % int(time.time()))


out = {"dataset": NAME}

# ---- Rust alone, embeddings on --------------------------------------------
rust = fresh("rust-emb")
codanna_init(rust, True)
out["rust_emb"] = {"cold": run(*codanna(rust))[0]}
out["rust_emb"]["unchanged"] = run(*codanna(rust))[0]
edit(rust)
out["rust_emb"]["one_file"] = run(*codanna(rust))[0]
out["rust_emb"]["disk"] = size_mb(rust / ".codanna")

# ---- combined, Rust embeddings on, with and without semble ------------------
for tag, with_semble in (("combo-emb", True), ("combo-emb-nosemble", False)):
    root = fresh(tag)
    codanna_init(root, True)
    before = set(SEMBLE_CACHE.iterdir()) if SEMBLE_CACHE.exists() else set()
    jobs = lambda: [graph(root), codanna(root)] + ([semble(root)] if with_semble else [])
    cell = {"cold": together(jobs()), "unchanged": together(jobs())}
    edit(root)
    cell["one_file"] = together(jobs())
    cell["disk"] = {"graphify": size_mb(root / "graphify-out"), "codanna": size_mb(root / ".codanna"),
                    "semble": sum(size_mb(d) for d in set(SEMBLE_CACHE.iterdir()) - before) if with_semble else 0.0}
    out[tag] = cell

# ---- where graphify's disk goes ---------------------------------------------
gdir = fresh("combo-emb") / "graphify-out"
out["graphify_breakdown_mb"] = {"graph.json": (gdir / "graph.json").stat().st_size / 1e6,
                                "cache": size_mb(gdir / "cache"),
                                "other": size_mb(gdir) - (gdir / "graph.json").stat().st_size / 1e6 - size_mb(gdir / "cache")}

# ---- search quality: semble versus Rust embeddings --------------------------
quality_root = fresh("combo-emb")
functions = []
for path in sorted(quality_root.rglob("*.py")):
    if "graphify-out" in path.parts:
        continue
    try:
        tree = ast.parse(path.read_text(encoding="utf-8", errors="replace"))
    except SyntaxError:
        continue
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            doc = ast.get_docstring(node) or ""
            first = doc.strip().split("\n")[0].strip()
            if len(first.split()) >= 6 and not node.name.startswith("_bench_probe"):
                functions.append({"file": path.relative_to(quality_root).as_posix(), "name": node.name,
                                  "first": node.lineno, "last": node.end_lineno, "doc": first})
functions = functions[:: max(1, len(functions) // 30)][:30]


def words(name):
    return " ".join(part for part in re.split(r"_+", name) if part)


def semble_hits(query):
    took, raw, _ = run([SEMBLE, "search", query, str(quality_root), "-k", "5", "--max-snippet-lines", "0"], cwd=str(WORK))
    try:
        results = json.loads(raw.strip().splitlines()[-1])["results"]
    except (ValueError, IndexError, KeyError):
        results = []
    return took, [(r["file_path"].replace("\\", "/"), r["start_line"], r["end_line"]) for r in results], len(raw)


def codanna_hits(query):
    took, raw, _ = run([CODANNA, "mcp", "semantic_search_docs", "query:" + query, "limit:5", "--json"], cwd=str(quality_root))
    hits = []
    try:
        for item in json.loads(raw).get("data") or []:
            symbol = item.get("symbol", item)
            span = symbol.get("range", {})
            hits.append((str(symbol.get("file_path", "")).replace("\\", "/"), span.get("start_line", -1) + 1, span.get("end_line", -1) + 1))
    except ValueError:
        pass
    return took, hits, len(raw)


def found(hits, target, top):
    for path, first, last in hits[:top]:
        if path.endswith(target["file"]) and first <= target["last"] and last >= target["first"]:
            return True
    return False


quality = {}
for engine, search in (("semble", semble_hits), ("rust_embeddings", codanna_hits)):
    for style, make in (("docstring", lambda f: f["doc"]), ("name_words", lambda f: words(f["name"]))):
        top1 = top5 = 0
        seconds = []
        for target in functions:
            took, hits, _ = search(make(target))
            seconds.append(took)
            top1 += found(hits, target, 1)
            top5 += found(hits, target, 5)
        quality[f"{engine}/{style}"] = {"top1": round(top1 / len(functions), 2), "top5": round(top5 / len(functions), 2),
                                        "seconds": round(sum(seconds) / len(seconds), 2)}
out["search_quality"] = {"queries": len(functions), **quality}


def rounded(value):
    if isinstance(value, dict):
        return {key: rounded(item) for key, item in value.items()}
    return round(value, 1) if isinstance(value, float) else value


print(json.dumps(rounded(out), indent=1))
(WORK / f"{NAME}-emb.json").write_text(json.dumps(out, indent=1), encoding="utf-8")
