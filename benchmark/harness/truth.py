"""Ground truth for the agent benchmark, straight from the syntax tree.

truth.py REPO NAME...   for each NAME: where it is defined, its parameters, and
every function whose body calls it (by bare name or attribute name).
truth.py REPO --candidates   names defined exactly once with 4 to 12 callers.
"""
import ast
import collections
import json
import pathlib
import sys

repo = pathlib.Path(sys.argv[1])
defs = collections.defaultdict(list)      # name -> [(file, line, end, params)]
calls = collections.defaultdict(set)      # callee name -> {(file, enclosing function)}


class Walk(ast.NodeVisitor):
    def __init__(self, rel):
        self.rel, self.stack = rel, []

    def visit_FunctionDef(self, node):
        params = [a.arg for a in node.args.posonlyargs + node.args.args + node.args.kwonlyargs]
        defs[node.name].append((self.rel, node.lineno, node.end_lineno, params))
        self.stack.append(node.name)
        self.generic_visit(node)
        self.stack.pop()

    visit_AsyncFunctionDef = visit_FunctionDef

    def visit_Call(self, node):
        target = node.func
        name = target.id if isinstance(target, ast.Name) else getattr(target, "attr", None)
        if name:
            calls[name].add((self.rel, self.stack[-1] if self.stack else "<module>"))
        self.generic_visit(node)


for path in sorted(repo.rglob("*.py")):
    if ".git" in path.parts or "graphify-out" in path.parts:
        continue
    try:
        tree = ast.parse(path.read_text(encoding="utf-8", errors="replace"))
    except SyntaxError:
        continue
    Walk(path.relative_to(repo).as_posix()).visit(tree)

if "--candidates" in sys.argv:
    for name, where in sorted(defs.items()):
        if len(where) == 1 and 4 <= len(calls[name]) <= 12:
            files = {f for f, _ in calls[name]}
            if len(files) >= 2:
                print(f"{name}  defined {where[0][0]}:{where[0][1]}  callers={len(calls[name])}  files={len(files)}")
else:
    for name in sys.argv[2:]:
        print(json.dumps({"name": name, "defined": defs.get(name),
                          "callers": sorted(calls.get(name, ()))}, indent=1))
