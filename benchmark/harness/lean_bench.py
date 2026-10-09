"""Time the combined index with and without the graphify graph. Scratch helper.

lean_bench.py NAME SRC WORK CODANNA   prints JSON: first build, nothing changed,
one file changed (seconds) and megabytes on disk, for both variants, each on
its own fresh copy of SRC. Same commands and the same edit as combo_bench.py.
"""
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

HOME = pathlib.Path.home()
NAME, SRC, WORK = sys.argv[1], pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3])
GRAPHIFY = str(HOME / ".graphify/venv/Scripts/graphify.exe")
SEMBLE = str(HOME / "Tools/codesearch-venv/Scripts/semble.exe")
CODANNA = str(pathlib.Path(sys.argv[4]))
SEMBLE_CACHE = pathlib.Path(os.environ["LOCALAPPDATA"]) / "semble" / "Cache"
ENV = dict(os.environ, HF_HUB_OFFLINE="1", GRAPHIFY_NO_TIPS="1", GRAPHIFY_NO_AUTO_REFRESH="1")
SKIP = shutil.ignore_patterns("graphify-out", ".codanna", ".git", "__pycache__", "node_modules")


def together(jobs):
    start = time.perf_counter()
    procs = [subprocess.Popen(argv, cwd=cwd, env=ENV, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL) for argv, cwd in jobs]
    for proc in procs:
        proc.wait()
    return round(time.perf_counter() - start, 1)


def size_mb(path):
    return sum(f.stat().st_size for f in pathlib.Path(path).rglob("*") if f.is_file()) / 1e6


def fresh(tag):
    target = WORK / f"{NAME}-{tag}"
    shutil.copytree(SRC, target, ignore=SKIP)
    subprocess.run(["git", "init", "-q"], cwd=target, capture_output=True)
    (target / ".git" / "info" / "exclude").write_text("graphify-out/\n.codanna/\n.codannaignore\n", encoding="utf-8")
    subprocess.run([CODANNA, "init"], cwd=str(target), env=ENV, capture_output=True, stdin=subprocess.DEVNULL)
    settings = target / ".codanna" / "settings.toml"
    text = re.sub(r"(\[semantic_search\][^\[]*?\nenabled = )true", r"\1false", settings.read_text(encoding="utf-8"))
    settings.write_text(text, encoding="utf-8")
    with (target / ".codannaignore").open("a", encoding="utf-8") as stream:
        stream.write("\ngraphify-out/\n")
    return target


def semble_mb(root):
    for entry in SEMBLE_CACHE.iterdir():
        for meta in entry.glob("*/metadata.json"):
            try:
                if pathlib.Path(json.loads(meta.read_text(encoding="utf-8"))["root_path"]) == root:
                    return size_mb(entry)
            except (OSError, ValueError, KeyError):
                pass
    return 0.0


def measure(tag, with_graph):
    root = fresh(tag)
    jobs = [([CODANNA, "index", ".", "--no-progress"], str(root)),
            ([SEMBLE, "search", "entry point", str(root), "-k", "1", "--max-snippet-lines", "0"], str(WORK))]
    if with_graph:
        jobs.append(([GRAPHIFY, "extract", str(root), "--code-only", "--no-viz", "--no-cluster"], str(WORK)))
    row = {"cold": together(jobs), "unchanged": together(jobs)}
    target = max(root.rglob("*.py"), key=lambda p: p.stat().st_size)
    with target.open("a", encoding="utf-8") as stream:
        stream.write("\n\ndef _bench_probe_%d():\n    return 1\n" % int(time.time()))
    row["one_file"] = together(jobs)
    parts = {"codanna": round(size_mb(root / ".codanna"), 1), "semble": round(semble_mb(root), 1)}
    if with_graph:
        parts["graphify"] = round(size_mb(root / "graphify-out"), 1)
    row["parts"], row["disk"] = parts, round(sum(parts.values()), 1)
    return row


WORK.mkdir(parents=True, exist_ok=True)
print(json.dumps({"with_graph": measure("graph", True), "graph_on_demand": measure("lean", False)}, indent=1))
