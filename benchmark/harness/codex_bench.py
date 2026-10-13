"""Second-harness check: the same lookup questions under the Codex CLI, with and without pgr.

    codex_bench.py --dry
    codex_bench.py --selftest
    codex_bench.py --fixture DIR --model MODEL --effort EFFORT --date YYYY-MM-DD [--codex PATH] [--pgr PATH]

One Codex session answers the three questions of one set. Two methods (no-index: ripgrep and
line ranges only; pgr: `pgr ask`), two question sets (A and B of the main benchmark), a few
repeats, one session after another, methods alternating. The questions, the answer schemas and
the pgr command line are read from token-benchmark.workflow.js and the truth and the grader from
bench_parse.py, so there is no second copy to drift. The result is benchmark/data/codex.json.
See CODEX.md for what it measures and what it cannot show.

Its numbers come from another harness (other system prompt, other caching) and are never mixed
with the main benchmark's: only no-index against pgr inside the one file is comparable.
"""
from __future__ import annotations

import argparse
import copy
import glob
import importlib.util
import json
import os
import pathlib
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from collections import Counter

HERE = pathlib.Path(__file__).resolve().parent
WORKFLOW = HERE / "token-benchmark.workflow.js"
BENCH_PARSE = HERE / "bench_parse.py"
DEFAULT_OUT = HERE.parent / "data" / "codex.json"
PLACEHOLDER = r"C:\path\to\lxbench\repo"      # what the data shows for the fixture path
METHODS = ("no-index", "pgr")
SETS = ("A", "B")
TIMEOUT = 480                                   # seconds per session, then it is killed
FEATURES_OFF = ("hooks", "multi_agent")         # user hooks run scripts and wrap commands; sub-agents are not allowed
LIMITS = [
    "Three repeats per cell support no claim of significance.",
    "One repository (86 Python files).",
    "Token counts come from the Codex CLI, with its own system prompt, tools and caching. They are not comparable with the main benchmark's counts: only no-index against pgr inside this file is.",
    "The questions were written by the builders of pgr.",
]
NOT_MEASURED = [
    "Model requests per session: the CLI events give one aggregate usage record per turn, not a request count.",
    "Other models, other efforts, other repositories, other agent harnesses.",
    "Orchestration, parallel sub-agents, and the pgr memory part: this file measures one agent answering three questions.",
]


# ---------------------------------------------------------------- reading the workflow file

class JsError(ValueError):
    pass


class JsReader:
    """Reads the constant literals the benchmark needs out of the workflow file.

    Handles what those constants use: quoted strings, numbers, true/false/null, arrays,
    objects with plain or quoted keys, comments, and references such as `S.q1` or `NAMES`."""

    def __init__(self, src: str):
        self.src = src
        self.cache: dict = {}

    def const(self, name: str):
        if name not in self.cache:
            found = re.search(r"^const " + re.escape(name) + r" = ", self.src, re.M)
            if not found:
                raise JsError(f"`const {name} =` not found in {WORKFLOW.name}")
            self.cache[name] = _JsValue(self, found.end()).value()
        return self.cache[name]


class _JsValue:
    def __init__(self, reader: JsReader, pos: int):
        self.reader, self.src, self.pos = reader, reader.src, pos

    def _skip(self):
        src = self.src
        while self.pos < len(src):
            if src[self.pos].isspace():
                self.pos += 1
            elif src.startswith("//", self.pos):
                end = src.find("\n", self.pos)
                self.pos = len(src) if end < 0 else end
            elif src.startswith("/*", self.pos):
                end = src.find("*/", self.pos)
                if end < 0:
                    raise JsError("unterminated comment")
                self.pos = end + 2
            else:
                return

    def _expect(self, char: str):
        self._skip()
        if self.src[self.pos:self.pos + 1] != char:
            raise JsError(f"expected {char!r} at offset {self.pos}: {self.src[self.pos:self.pos + 30]!r}")
        self.pos += 1

    def _string(self) -> str:
        quote = self.src[self.pos]
        self.pos += 1
        out = []
        while self.pos < len(self.src):
            c = self.src[self.pos]
            if c == "\\":
                nxt = self.src[self.pos + 1]
                out.append({"n": "\n", "t": "\t"}.get(nxt, nxt))
                self.pos += 2
            elif c == quote:
                self.pos += 1
                return "".join(out)
            else:
                out.append(c)
                self.pos += 1
        raise JsError("unterminated string")

    def _name(self) -> str:
        found = re.compile(r"[A-Za-z_$][\w$]*").match(self.src, self.pos)
        if not found:
            raise JsError(f"expected a name at offset {self.pos}: {self.src[self.pos:self.pos + 30]!r}")
        self.pos = found.end()
        return found.group(0)

    def value(self):
        self._skip()
        c = self.src[self.pos]
        if c in "'\"":
            return self._string()
        if c == "{":
            self.pos += 1
            out = {}
            while True:
                self._skip()
                if self.src[self.pos] == "}":
                    self.pos += 1
                    return out
                key = self._string() if self.src[self.pos] in "'\"" else self._name()
                self._expect(":")
                out[key] = self.value()
                self._skip()
                if self.src[self.pos] == ",":
                    self.pos += 1
        if c == "[":
            self.pos += 1
            out = []
            while True:
                self._skip()
                if self.src[self.pos] == "]":
                    self.pos += 1
                    return out
                out.append(self.value())
                self._skip()
                if self.src[self.pos] == ",":
                    self.pos += 1
        number = re.compile(r"-?\d+").match(self.src, self.pos)
        if number:
            self.pos = number.end()
            return int(number.group(0))
        name = self._name()
        if name in ("true", "false", "null"):
            return {"true": True, "false": False, "null": None}[name]
        got = self.reader.const(name)
        while self.src.startswith(".", self.pos):
            self.pos += 1
            got = got[self._name()]
        return got


def load_workflow(path: pathlib.Path = WORKFLOW) -> dict:
    """Question texts, answer schemas, key order and the pgr ask line, as the workflow file has them."""
    reader = JsReader(path.read_text(encoding="utf-8"))
    got = {name: reader.const(name) for name in ("KEYS", "Q", "Q_B", "S", "S_B", "ALL", "ALL_B", "PGR_ASK")}
    for name in ("Q", "Q_B"):
        if list(got[name]) != got["KEYS"]:
            raise JsError(f"{name} does not have the keys {got['KEYS']}")
    if not isinstance(got["PGR_ASK"], str) or not got["PGR_ASK"].startswith("pgr ask"):
        raise JsError("PGR_ASK is not a `pgr ask` description")
    return got


def load_bench_parse(tries: int = 4, wait: int = 30):
    """Import bench_parse.py by path (`python -I` has no script directory on sys.path). Another builder
    may be editing it: wait and try again before giving up."""
    last = None
    for attempt in range(tries):
        try:
            spec = importlib.util.spec_from_file_location("bench_parse", BENCH_PARSE)
            module = importlib.util.module_from_spec(spec)
            sys.modules["bench_parse"] = module
            spec.loader.exec_module(module)
            for name in ("grade", "TRUTH", "TRUTH_B"):
                getattr(module, name)
            return module
        except Exception as error:      # mid-edit: syntax error or a missing name
            last = error
            sys.modules.pop("bench_parse", None)
            if attempt + 1 < tries:
                print(f"bench_parse.py would not import ({error}); waiting {wait}s", file=sys.stderr)
                time.sleep(wait)
    raise SystemExit(f"bench_parse.py would not import after {tries} tries: {last}")


# ---------------------------------------------------------------- schemas and prompts

def strict(schema: dict) -> dict:
    """The structured-output form Codex accepts: every property required, no extra properties."""
    out = copy.deepcopy(schema)

    def walk(node):
        if node.get("type") == "object":
            props = node.get("properties", {})
            if set(props) - set(node.get("required", [])):
                raise ValueError(f"properties not listed as required: {sorted(set(props) - set(node['required']))}")
            node["additionalProperties"] = False
            for child in props.values():
                walk(child)
        elif node.get("type") == "array":
            walk(node["items"])

    walk(out)
    return out


def validate(schema: dict, value, where: str = "answer") -> list:
    """Errors of a value against the small schema subset used here (object, array, string, integer)."""
    kind = schema.get("type")
    if kind == "object":
        if not isinstance(value, dict):
            return [f"{where}: not an object"]
        errors = [f"{where}.{key}: missing" for key in schema.get("required", []) if key not in value]
        errors += [f"{where}.{key}: not allowed" for key in value
                   if schema.get("additionalProperties") is False and key not in schema["properties"]]
        for key, child in schema["properties"].items():
            if key in value:
                errors += validate(child, value[key], f"{where}.{key}")
        return errors
    if kind == "array":
        if not isinstance(value, list):
            return [f"{where}: not an array"]
        return [e for i, item in enumerate(value) for e in validate(schema["items"], item, f"{where}[{i}]")]
    if kind == "string":
        return [] if isinstance(value, str) else [f"{where}: not a string"]
    if kind == "integer":
        return [] if isinstance(value, int) and not isinstance(value, bool) else [f"{where}: not an integer"]
    return [f"{where}: unsupported schema type {kind!r}"]


READ_LINES = "Get-Content <file> | Select-Object -Skip <first line minus one> -First <count>"
SHELL_HEAD = ("Allowed shell commands. Each is one call of your shell tool (PowerShell); "
              "the working directory is already the repository root.")


def allowed_block(method: str, pgr_ask: str) -> list:
    """The one part of the prompt that differs between the methods."""
    if method == "no-index":
        return [SHELL_HEAD,
                "  rg <pattern> [path...]     ripgrep, the only search command: searches file contents by regular expression; rg --files lists files",
                f"To see code, print a line range of a file with {READ_LINES}, and only when the answer needs it."]
    return [SHELL_HEAD,
            "  " + pgr_ask,
            f"After a command answers, print a line range of a file with {READ_LINES} only on a line range it returned, "
            "and only when the answer needs it. Use rg only when the listed commands return nothing useful."]


def build_prompt(method: str, which: str, wf: dict) -> str:
    """Same structure as the workflow's policy() plus the numbered questions; the Claude tool names become shell commands."""
    questions = wf["Q" if which == "A" else "Q_B"]
    listed = "\n".join(f"{k.upper()}. {questions[k]}" for k in wf["KEYS"])
    lines = [
        "This is one run of a benchmark that measures how many tokens a code lookup costs.",
        "Repository under test: your working directory (Python, 86 files). It is read-only: never edit, create or delete anything in it, "
        "and never build or refresh an index. Stay inside it: never read, list or search any path outside the working directory.",
        "",
        "TOOL POLICY FOR THIS RUN. It replaces the local-index and read-gate rules and any token-goat gate in your global instructions. "
        "Use only what is listed here. No sub-agents, no web access, no other search or index command.",
        *allowed_block(method, wf["PGR_ASK"]),
        "Be exact and do not guess. Stop as soon as the work is answered, then return the answer through the structured output.",
    ]
    return "\n".join(lines) + f"\n\nAnswer all three questions.\n{listed}"


def leaks(prompt: str, which: str, wf: dict, bp) -> list:
    """Truth strings that appear in the prompt although the question does not already contain them."""
    truth = bp.TRUTH if which == "A" else bp.TRUTH_B
    questions = " ".join(wf["Q" if which == "A" else "Q_B"].values())
    words = set(truth["q1"]) | {truth["q2"]["file"], str(truth["q2"]["line"]), *truth["q2"]["parameters"], *truth["q2"]["called_by"],
                                truth["q3"]["function"], truth["q3"]["file"], str(truth["q3"]["line"])}
    fourth = truth["q3"].get("read_by", truth["q3"].get("removed_by"))
    words |= set(fourth) if isinstance(fourth, (set, frozenset)) else {fourth}
    found = []
    for word in sorted(words):
        pattern = r"(?<![\w.])" + re.escape(word) + r"(?![\w])"
        if re.search(pattern, prompt) and not re.search(pattern, questions):
            found.append(word)
    return found


# ---------------------------------------------------------------- judging the commands a session ran

RANGE_READERS = {"get-content", "gc", "cat", "type"}
ALWAYS_RANGE = {"sed", "head", "tail"}
NEUTRAL = {"set-location", "cd", "sl", "push-location", "pop-location", "select-object", "select", "where-object", "where",
           "?", "foreach-object", "foreach", "%", "sort-object", "sort", "measure-object", "measure", "out-string", "out-host",
           "write-output", "write", "echo", "write-host", "format-table", "ft", "format-list", "fl", "format-wide", "if", "elseif",
           "else", "for", "while", "do", "switch", "try", "catch", "finally", "function", "param", "return"}
ALLOWED = {"no-index": {"rg"}, "pgr": {"pgr", "rg"}}
RANGE_MARK = re.compile(r"(?i)-(?:TotalCount|Head|First|Last|Tail|Skip|SkipLast|Index)\b|\[\s*-?\d+\s*\.\.\s*-?\d+\s*\]")
STATEMENT_SEPS = ["&&", "||", ";", "\n"]


def _split(text: str, delimiters: list) -> list:
    """Split at the delimiters that sit outside quotes and brackets."""
    out, buf, quote, depth, i = [], [], None, 0, 0
    while i < len(text):
        c = text[i]
        if quote:
            buf.append(c)
            if c == "`" and quote == '"' and i + 1 < len(text):
                buf.append(text[i + 1])
                i += 2
                continue
            if c == quote:
                quote = None
        elif c in "'\"":
            quote = c
            buf.append(c)
        elif c == "`" and i + 1 < len(text):
            buf.append(c + text[i + 1])
            i += 2
            continue
        else:
            if c in "({[":
                depth += 1
            elif c in ")}]":
                depth = max(0, depth - 1)
            hit = next((d for d in delimiters if text.startswith(d, i)), None) if depth == 0 else None
            if hit:
                out.append("".join(buf))
                buf = []
                i += len(hit)
                continue
            buf.append(c)
        i += 1
    out.append("".join(buf))
    return [s.strip() for s in out if s.strip()]


def _groups(text: str) -> list:
    """Contents of the outermost (...) and {...} groups outside quotes."""
    out, quote, depth, start, i = [], None, 0, None, 0
    while i < len(text):
        c = text[i]
        if quote:
            if c == "`" and quote == '"':
                i += 1
            elif c == quote:
                quote = None
        elif c in "'\"":
            quote = c
        elif c == "`":
            i += 1
        elif c in "({[":
            if depth == 0 and c != "[":
                start = i
            depth += 1
        elif c in ")}]":
            depth = max(0, depth - 1)
            if depth == 0 and start is not None:
                out.append(text[start + 1:i])
                start = None
        i += 1
    return out


def _blank_quotes(text: str) -> str:
    return re.sub(r"'(?:[^']|'')*'|\"(?:[^\"`]|`.)*\"", lambda m: " " * len(m.group(0)), text)


def _word(stage: str) -> str:
    """The program a pipeline stage runs: lower case, no folder, no .exe. Empty for a group or variable."""
    core = stage.lstrip("&. \t")
    if not core or core[0] in "({$@[":
        return ""
    if core[0] in "'\"":
        end = core.find(core[0], 1)
        if end > 0 and not core[end + 1:].strip():
            return ""                   # a bare string is a value, not a command
        token = core[1:end if end > 0 else None]
    else:
        token = core.split()[0]
    token = token.replace("\\", "/").split("/")[-1].lower()
    return token[:-4] if token.endswith(".exe") else token


def scan(text: str, outer: str = "") -> list:
    """(program, stage, statement) for every pipeline stage in a shell command, nested groups included.
    A group's statement carries the statement around it, so `(Get-Content f)[10..20]` still shows its range."""
    out = []
    for statement in _split(text, STATEMENT_SEPS):
        whole = f"{outer} {statement}".strip()
        for stage in _split(statement, ["|"]):
            core = re.sub(r"^\$[\w:]+\s*=\s*", "", stage, flags=re.S)
            word = _word(core)
            if word:
                out.append((word, core, whole))
            for inner in _groups(core):
                out += scan(inner, whole)
    return out


def unwrap(command: str) -> str:
    """The command out of the `powershell.exe -Command '...'` call the CLI reports, otherwise as reported
    (the CLI doubles backslashes and rewrites some quoting, a `^` in a pattern for one)."""
    found = re.match(r"^\s*\"?[^\"]*?(?:powershell|pwsh)(?:\.exe)?\"?\s+.*?-(?:Command|c)\s+(.*)$", command, re.S | re.I)
    if not found:
        return command
    rest = found.group(1).strip()
    if rest[:1] in ("'", '"'):
        quote, rest = rest[0], rest[1:]
        if rest.endswith(quote):
            rest = rest[:-1]
    return rest


def tidy(text: str) -> str:
    """Undo the reporting artefacts that would confuse the judge: doubled backslashes, escaped quotes, a doubled
    single quote, and the `'"'` the CLI writes around patterns that hold a `^`."""
    return text.replace("\\\\", "\\").replace('\\"', '"').replace("'\"'", '"').replace("''", "'")


# Programs that are recognisable as something else than the allowed set even where the quoting is damaged.
KNOWN_OTHER = {
    "pgr", "lx", "token-goat", "graphify", "semble", "codanna", "ast-grep", "sg", "semgrep", "ctags", "cscope",
    "git", "hg", "svn", "python", "python3", "py", "pip", "node", "npm", "npx", "deno", "bun", "cargo", "rustc", "go",
    "java", "ruby", "perl", "php", "awk", "gawk", "cut", "uniq", "wc", "tr", "xargs", "tree", "ls", "dir", "gci", "find",
    "grep", "egrep", "fgrep", "findstr", "ag", "ack", "fd", "fzf", "sls", "curl", "wget", "iwr", "irm", "iex", "icm", "ssh",
    "cmd", "powershell", "pwsh", "bash", "sh", "wsl", "start", "saps", "rm", "del", "mv", "cp", "copy", "move", "ni", "mkdir", "md",
}


def recognisable(word: str) -> bool:
    """A word that is plainly a program: known, a Verb-Noun cmdlet, or something with an executable suffix."""
    return (word in KNOWN_OTHER or bool(re.fullmatch(r"[a-z]+-[a-z]+", word))
            or bool(re.search(r"\.(?:exe|ps1|py|cmd|bat|sh)$", word)))


def judge(command: str, method: str, fixture: str | None = None) -> dict:
    """Programs a command ran and the reasons it is outside the method's allowed set.

    The first word of the command is held to the full rule. A later stage is flagged only when it is plainly another
    program (see `recognisable`), because the CLI's quoting of a `^` pattern can leave regex text looking like commands."""
    inner = tidy(unwrap(command))
    allowed = ALLOWED[method]
    reasons, programs = [], []
    for index, (word, stage, statement) in enumerate(scan(inner)):
        if index == 0 or word in allowed or word in NEUTRAL or word in ALWAYS_RANGE or word in RANGE_READERS or recognisable(word):
            programs.append(word)       # regex text the damaged quoting leaves behind is not a program
        if word in allowed or word in NEUTRAL or word in ALWAYS_RANGE:
            if word == "pgr":
                rest = stage.lstrip("&. \t").split(None, 1)[1:] or [""]
                sub = next((t for t in rest[0].split() if not t.startswith("-")), "")
                if sub != "ask":
                    reasons.append(f"pgr {sub}".strip())
            continue
        if word in RANGE_READERS:
            if not RANGE_MARK.search(statement):
                reasons.append("read-without-range")
            continue
        if index == 0 or recognisable(word):
            reasons.append(word)
    blanked = _blank_quotes(inner)
    for target in re.findall(r"(?<![>\w-])\d?>>?\s*(\S+)", blanked):
        if target.lower() != "$null" and not target.startswith("&"):
            reasons.append("redirect")
    root = (fixture or PLACEHOLDER).replace("/", "\\").rstrip("\\").lower()
    for path in re.findall(r"[A-Za-z]:[\\/][^\s'\"|;)]*|\.\.[\\/]", inner):
        norm = path.replace("/", "\\").lower()
        if not norm.startswith(root) and not re.search(r"[\\/](?:pgr|rg)(?:\.exe)?$", norm):
            reasons.append("outside-repository")
    return {"programs": programs, "reasons": sorted(set(reasons))}


def command_block(raw_commands: list, method: str, fixture: str | None) -> tuple:
    """The `commands` block of a session and its off-policy list, from the commands the event stream reported."""
    judged = [judge(c["command"], method, fixture) for c in raw_commands]
    listed = [{"command": unwrap(c["command"]), "exit_code": c["exit_code"], "output_chars": c["output_chars"],
               **({"off_policy": j["reasons"]} if j["reasons"] else {})} for c, j in zip(raw_commands, judged)]
    off = [{"command": unwrap(c["command"]), "reasons": j["reasons"]} for c, j in zip(raw_commands, judged) if j["reasons"]]
    block = {"count": len(listed), "programs": dict(Counter(p for j in judged for p in j["programs"])), "list": listed}
    return sanitize(block, fixture), sanitize(off, fixture)


# ---------------------------------------------------------------- machine facts

def find_codex() -> str | None:
    base = os.environ.get("LOCALAPPDATA")
    found = glob.glob(os.path.join(base, "OpenAI", "Codex", "bin", "*", "codex.exe")) if base else []
    return max(found, key=os.path.getmtime) if found else shutil.which("codex")


def sanitize(value, fixture: str | None):
    """Drop machine paths from anything that goes into the data: the fixture becomes the placeholder, a user folder `<user>`."""
    if isinstance(value, str):
        if fixture:
            for form in {fixture, fixture.replace("\\", "/"), fixture.replace("\\", "\\\\")}:
                value = re.sub(re.escape(form), lambda _: PLACEHOLDER, value, flags=re.I)
        return re.sub(r"([A-Za-z]:[\\/]+Users[\\/]+)[^\\/\s'\"]+", r"\1<user>", value, flags=re.I)
    if isinstance(value, list):
        return [sanitize(v, fixture) for v in value]
    if isinstance(value, dict):
        return {k: sanitize(v, fixture) for k, v in value.items()}
    return value


def snapshot(root: str) -> dict:
    """Every file under the fixture with its size and modification time, to prove a run changed nothing."""
    out = {}
    for folder, _dirs, files in os.walk(root):
        for name in files:
            full = os.path.join(folder, name)
            try:
                info = os.stat(full)
            except OSError:
                continue
            out[os.path.relpath(full, root)] = (info.st_size, info.st_mtime_ns)
    return out


def fixture_facts(root: str) -> dict:
    count = lines = 0
    for path in pathlib.Path(root).rglob("*.py"):
        if ".git" in path.parts:
            continue
        text = path.read_bytes().decode("utf-8", "replace")
        count += 1
        lines += text.count("\n") + (1 if text and not text.endswith("\n") else 0)
    return {"python_files": count, "lines": lines}


def tool_env(pgr: str) -> dict:
    env = dict(os.environ)
    env["PGR_NO_REFRESH"] = "1"                 # pgr never writes
    env["PATH"] = os.path.dirname(pgr) + os.pathsep + env.get("PATH", "")
    return env


def run_plain(cmd: list, env: dict | None = None) -> str:
    done = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace",
                          stdin=subprocess.DEVNULL, env=env, timeout=60)
    return (done.stdout + done.stderr).strip()


# ---------------------------------------------------------------- one Codex session

USAGE_LIMIT = re.compile(r"(?i)hit your usage limit")
TRY_AGAIN = re.compile(r"(?i)try again (?:at|in) ([^.\n]+)")


def codex_cmd(args, schema_path: str, out_path: str, json_mode: bool) -> list:
    cmd = [args.codex, "exec", "-m", args.model, "-c", f"model_reasoning_effort={args.effort}"]
    for feature in FEATURES_OFF:
        cmd += ["--disable", feature]
    cmd += ["-s", "read-only", "--skip-git-repo-check", "--ephemeral", "-C", args.fixture,
            "--output-schema", schema_path, "-o", out_path]
    if json_mode:
        cmd.append("--json")
    return cmd + ["-"]                           # the prompt arrives on standard input


def run_codex(cmd: list, prompt: str, env: dict, timeout: int):
    """(returncode, stdout, stderr, timed_out, seconds). A run over the limit is killed with its children."""
    started = time.monotonic()
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                            encoding="utf-8", errors="replace", env=env,
                            creationflags=getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0))
    timed_out = False
    try:
        out, err = proc.communicate(prompt, timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        subprocess.run(["taskkill", "/PID", str(proc.pid), "/T", "/F"], capture_output=True)
        proc.kill()
        out, err = proc.communicate()
    return proc.returncode, out or "", err or "", timed_out, round(time.monotonic() - started, 1)


def parse_events(stdout: str) -> dict:
    """What the --json event stream says: usage, turns, commands, messages, errors."""
    got = {"usage": None, "turns": 0, "commands": [], "messages": [], "errors": [], "failures": [], "types": Counter()}
    started = {}
    for line in stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            event = json.loads(line)
        except ValueError:
            continue
        kind = event.get("type")
        got["types"][kind] += 1
        item = event.get("item") or {}
        if kind == "item.started" and item.get("type") == "command_execution":
            started[item.get("id")] = item.get("command", "")
        elif kind == "item.completed":
            if item.get("type") == "command_execution":
                started.pop(item.get("id"), None)
                got["commands"].append({"command": item.get("command", ""), "exit_code": item.get("exit_code"),
                                        "output_chars": len(item.get("aggregated_output") or "")})
            elif item.get("type") == "agent_message":
                got["messages"].append(item.get("text", ""))
            elif item.get("type") == "error":
                got["errors"].append(item.get("message", ""))
        elif kind == "turn.completed":
            got["usage"] = event.get("usage")
            got["turns"] += 1
        elif kind in ("turn.failed", "error"):
            got["failures"].append(json.dumps(event)[:500])
    for command in started.values():          # started, never finished: the run was cut off
        got["commands"].append({"command": command, "exit_code": None, "output_chars": 0})
    return got


def parse_header(stderr: str) -> dict:
    """The block the CLI prints above a run without --json: version, workdir, model, effort, sandbox..."""
    lines = stderr.splitlines()
    header = {}
    rules = [i for i, text in enumerate(lines) if text.strip() == "--------"]
    for text in lines[:rules[1]] if len(rules) >= 2 else []:
        key, sep, value = text.partition(":")
        if sep:
            header[key.strip().lower().replace(" ", "_")] = value.strip()
        elif text.startswith("OpenAI Codex"):
            header["banner"] = text.strip()
    used = re.search(r"tokens used\s*\n\s*([\d,]+)", stderr)
    if used:
        header["tokens_used"] = int(used.group(1).replace(",", ""))
    return header


def limit_text(*texts: str) -> dict | None:
    blob = "\n".join(texts)
    if not USAGE_LIMIT.search(blob):
        return None
    line = next((l for l in blob.splitlines() if USAGE_LIMIT.search(l)), "")
    again = TRY_AGAIN.search(blob)
    return {"message": line.strip()[:400], "try_again": again.group(1).strip() if again else None}


def final_answer(out_path: str, messages: list):
    """The last message of the run as parsed JSON, or None."""
    for text in ([pathlib.Path(out_path).read_text(encoding="utf-8")] if os.path.exists(out_path) else []) + messages[::-1][:1]:
        try:
            return json.loads(text)
        except ValueError:
            continue
    return None


def grade_answer(bp, answer, which: str, schema: dict) -> dict:
    """Per-question grades by bench_parse.grade, the mean, and whether all three are fully correct (the main benchmark's rule)."""
    grades = {}
    for q in ("q1", "q2", "q3"):
        try:
            got = bp.grade(q, (answer or {}).get(q), which)
        except Exception:
            got = None
        grades[q] = got
    mean = round(sum(v or 0 for v in grades.values()) / 3, 4)
    return {"grades": grades, "mean_grade": mean, "fully_correct": mean >= 0.999,
            "schema_errors": validate(schema, answer) if answer is not None else ["no JSON answer"]}


def run_session(args, ctx: dict, method: str, which: str, rep: int, order: int) -> dict:
    tag = f"{method}-{which}-{rep}"
    scratch = pathlib.Path(args.scratch)
    out_path = str(scratch / f"{tag}-out.json")
    pathlib.Path(out_path).unlink(missing_ok=True)
    prompt = ctx["prompts"][method][which]
    cmd = codex_cmd(args, ctx["schema_paths"][which], out_path, True)
    rc, out, err, timed_out, seconds = run_codex(cmd, prompt, ctx["env"], args.timeout)
    (scratch / f"{tag}-events.jsonl").write_text(out, encoding="utf-8")
    (scratch / f"{tag}-stderr.txt").write_text(err, encoding="utf-8")
    events = parse_events(out)
    limit = limit_text(out, err) if rc != 0 or events["usage"] is None else None
    answer = final_answer(out_path, events["messages"])
    graded = grade_answer(ctx["bp"], answer, which, ctx["schemas"][which])
    usage = events["usage"]
    failure = None
    if limit:
        failure = {"kind": "usage_limit", "detail": limit["message"], "try_again": limit["try_again"]}
    elif timed_out:
        failure = {"kind": "timeout", "detail": f"killed after {args.timeout} seconds"}
    elif rc != 0:
        failure = {"kind": "cli_error", "detail": f"exit code {rc}: " + " ".join(err.split())[-300:]}
    elif usage is None:
        failure = {"kind": "no_usage", "detail": "no turn.completed event"}
    elif graded["schema_errors"]:
        failure = {"kind": "schema_failure", "detail": "; ".join(graded["schema_errors"])[:300]}
    block, off = command_block(events["commands"], method, args.fixture)
    tokens = None
    if usage:
        i, c, o = usage.get("input_tokens", 0), usage.get("cached_input_tokens", 0), usage.get("output_tokens", 0)
        tokens = {"input_tokens": i, "cached_input_tokens": c, "output_tokens": o,
                  "reasoning_output_tokens": usage.get("reasoning_output_tokens"),
                  "total_tokens": i + o, "fresh_tokens": i - c + o}
    return sanitize({
        "method": method, "set": which, "repeat": rep, "order": order,
        "answer": answer, **graded,
        "usage": tokens, "turns": events["turns"], "requests": None,
        "commands": block, "off_policy": bool(off), "off_policy_commands": off,
        "wall_seconds": seconds, "failure": failure, "warnings": events["errors"][:5],
    }, args.fixture)


# ---------------------------------------------------------------- summary

def mean(values):
    values = [v for v in values if v is not None]
    return round(statistics.fmean(values), 1) if values else None


def summarise(sessions: list) -> dict:
    """Per method and set: sessions run, sessions with usage, mean tokens, mean commands, fully correct k of n."""
    out = {}
    for method in METHODS:
        out[method] = {}
        for which in SETS:
            cell = [s for s in sessions if s["method"] == method and s["set"] == which
                    and not (s["failure"] and s["failure"]["kind"] == "usage_limit")]
            used = [s for s in cell if s["usage"]]
            out[method][which] = {
                "sessions": len(cell), "with_usage": len(used),
                "mean_total_tokens": mean(s["usage"]["total_tokens"] for s in used),
                "mean_fresh_tokens": mean(s["usage"]["fresh_tokens"] for s in used),
                "mean_cached_input_tokens": mean(s["usage"]["cached_input_tokens"] for s in used),
                "mean_output_tokens": mean(s["usage"]["output_tokens"] for s in used),
                "mean_commands": mean(s["commands"]["count"] for s in cell),
                "mean_grade": round(statistics.fmean(s["mean_grade"] for s in cell), 4) if cell else None,
                "fully_correct": sum(s["fully_correct"] for s in cell), "of": len(cell),
                "off_policy_sessions": sum(s["off_policy"] for s in cell),
                "failed_sessions": sum(bool(s["failure"]) for s in cell),
                "mean_wall_seconds": mean(s["wall_seconds"] for s in cell),
            }
    return out


def savings(summary: dict) -> dict:
    """pgr against no-index per set, as a share of the no-index mean: positive means pgr used less."""
    out = {}
    for which in SETS:
        base, pgr = summary["no-index"][which], summary["pgr"][which]
        out[which] = {}
        for key in ("mean_total_tokens", "mean_fresh_tokens", "mean_commands"):
            a, b = base[key], pgr[key]
            out[which][key.replace("mean_", "") + "_saved_percent"] = round(100 * (1 - b / a), 1) if a and b is not None else None
    return out


def session_order(repeats: int) -> list:
    """(set, repeat, method) in run order: pairs of one set and repeat, methods alternating inside and between pairs."""
    order = []
    for rep in range(1, repeats + 1):
        for index, which in enumerate(SETS):
            first = (rep + index) % 2
            for method in (METHODS[first], METHODS[1 - first]):
                order.append((which, rep, method))
    return order


def print_table(summary: dict, sv: dict):
    print(f"{'method':9} {'set':3} {'n':>2} {'mean total':>11} {'mean fresh':>11} {'cmds':>5} {'correct':>8} {'off-pol':>7} {'failed':>6}")
    for method in METHODS:
        for which in SETS:
            c = summary[method][which]
            print(f"{method:9} {which:3} {c['sessions']:>2} {c['mean_total_tokens'] or 0:>11,.0f} {c['mean_fresh_tokens'] or 0:>11,.0f} "
                  f"{c['mean_commands'] or 0:>5.1f} {c['fully_correct']:>4} of {c['of']:<1} {c['off_policy_sessions']:>7} {c['failed_sessions']:>6}")
    for which in SETS:
        print(f"saving of pgr on set {which}: {sv[which]}")


# ---------------------------------------------------------------- self test

def perfect(truth: dict) -> dict:
    fourth = "read_by" if "read_by" in truth["q3"] else "removed_by"
    pick = truth["q3"][fourth]
    pick = sorted(pick)[0] if isinstance(pick, (set, frozenset)) else pick
    return {"q1": {"callers": sorted(truth["q1"])},
            "q2": {"file": truth["q2"]["file"], "line": truth["q2"]["line"], "parameters": list(truth["q2"]["parameters"]),
                   "called_by": sorted(truth["q2"]["called_by"])},
            "q3": {"function": truth["q3"]["function"], "file": truth["q3"]["file"], "line": truth["q3"]["line"], fourth: pick}}


def selftest(wf: dict, bp) -> int:
    failed = []

    def check(name, ok):
        print(("ok    " if ok else "FAIL  ") + name)
        if not ok:
            failed.append(name)

    for which, truth in (("A", bp.TRUTH), ("B", bp.TRUTH_B)):
        schema = strict(wf["ALL" if which == "A" else "ALL_B"])
        good = perfect(truth)
        graded = grade_answer(bp, good, which, schema)
        check(f"set {which}: the perfect answer is fully correct and fits the schema",
              graded["fully_correct"] and not graded["schema_errors"] and all(v == 1 for v in graded["grades"].values()))
        for q, damage in (("q1", lambda a: a["q1"].update(callers=[])),
                          ("q2", lambda a: a["q2"].update(line=a["q2"]["line"] + 1)),
                          ("q3", lambda a: a["q3"].update(function="not_the_function"))):
            wrong = copy.deepcopy(good)
            damage(wrong)
            result = grade_answer(bp, wrong, which, schema)
            check(f"set {which}: a wrong {q} is not fully correct", not result["fully_correct"] and result["grades"][q] < 1)
        other = grade_answer(bp, perfect(bp.TRUTH_B if which == "A" else bp.TRUTH), which, schema)
        check(f"set {which}: the other set's perfect answer is not fully correct", not other["fully_correct"])
        check(f"set {which}: a malformed answer is a schema error", bool(grade_answer(bp, {"q1": {}}, which, schema)["schema_errors"]))
        for method in METHODS:
            check(f"set {which}, {method}: the prompt leaks no answer", not leaks(build_prompt(method, which, wf), which, wf, bp))
    bare = {m: build_prompt(m, "A", wf).replace("\n".join(allowed_block(m, wf["PGR_ASK"])) + "\n", "") for m in METHODS}
    check("the two prompts differ only inside the allowed-commands block",
          bare["no-index"] == bare["pgr"] and build_prompt("no-index", "A", wf) != build_prompt("pgr", "A", wf))
    cases = [  # (method, command, expected reasons)
        ("no-index", "rg -n 'def foo' .", []),
        ("no-index", "Get-Content cli.py | Select-Object -Skip 1063 -First 20", []),
        ("no-index", "(Get-Content cli.py)[1063..1080]", []),
        ("no-index", "Get-Content cli.py", ["read-without-range"]),
        ("no-index", "sed -n '10,20p' cli.py", []),
        ("no-index", "rg -l foo | ForEach-Object { Get-Content $_ | Select-Object -First 3 }", []),
        ("no-index", "$f = 'cli.py'; Get-Content $f | Select-Object -Skip 5 -First 5", []),
        ("no-index", "Set-Location .; rg foo 2>$null", []),
        ("no-index", "pgr ask def:x", ["pgr"]),
        ("no-index", "git grep foo", ["git"]),
        ("no-index", "Select-String -Path *.py foo", ["select-string"]),
        ("no-index", "python -c \"print(1)\"", ["python"]),
        ("no-index", "Get-ChildItem -Recurse", ["get-childitem"]),
        ("no-index", "rg foo > out.txt", ["redirect"]),
        ("no-index", "rg foo C:\\elsewhere\\code", ["outside-repository"]),
        ("no-index", "rg foo ..\\other", ["outside-repository"]),
        ("pgr", "pgr ask def:x callers:y", []),
        ("pgr", "& 'C:\\somewhere\\bin\\pgr.exe' ask about:x", []),
        ("pgr", "pgr find x", ["pgr find"]),
        ("pgr", "rg -n foo cli.py", []),
        ("pgr", "lx ask def:x", ["lx"]),
    ]
    for method, command, want in cases:
        wrapped = "\"C:\\\\Windows\\\\System32\\\\WindowsPowerShell\\\\v1.0\\\\powershell.exe\" -Command '" + command.replace("'", "''") + "'"
        got = judge(wrapped, method)["reasons"]
        check(f"judge {method}: {command!r} -> {want}", got == sorted(want))
    shell = r'"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" -Command '
    reported = [   # commands as the CLI printed them in a real run: backslashes doubled, quoting rewritten around a `^` pattern
        shell + r'''"rg -n '"'^def |''^    def |''^async def '"' __main__.py cli.py dedup.py prs.py llm.py"''',
        shell + r'''"rg -n '"'^ {8,}(async )?def '"' cli.py dedup.py prs.py llm.py __main__.py"''',
        shell + r'''"rg -n '_get_backend_api_key|dispatch_command|"'^'"\\s+def ' ."''',
        shell + r'''"rg -n '_get_backend_api_key|dispatch_command|merge.driver|merge driver|graph\\.json' ."''',
        shell + r"""'rg -n "''^def |''^    def |''^        def |'"\\b_wja\\b|\\b_write_json_atomic\\b\" cli.py export.py detect.py"'""",
        shell + r"""'rg -n "''^'"\\s*(async )?def |\\b_wja\\(\" export.py cli.py global_graph.py"'""",
    ]
    for command in reported:
        check(f"judge no-index: a command as the CLI reports it is not flagged: {unwrap(command)[:50]!r}",
              judge(command, "no-index")["reasons"] == [])
    check("a reported command with another program is still flagged",
          judge(shell + r'''"rg -n '"'^def '"' cli.py | git grep foo"''', "no-index")["reasons"] == ["git"])
    check("sanitize drops the fixture path and the user folder",
          sanitize("rg x C:\\Users\\someone\\Downloads\\lxbench\\repo\\cli.py", "C:\\Users\\someone\\Downloads\\lxbench\\repo")
          == "rg x C:\\path\\to\\lxbench\\repo\\cli.py")
    print("self test:", "FAILED " + str(len(failed)) if failed else "passed")
    return 1 if failed else 0


# ---------------------------------------------------------------- main

def write_schemas(wf: dict, scratch: pathlib.Path) -> tuple:
    schemas = {"A": strict(wf["ALL"]), "B": strict(wf["ALL_B"])}
    paths = {}
    for which, schema in schemas.items():
        paths[which] = str(scratch / f"schema-{which}.json")
        pathlib.Path(paths[which]).write_text(json.dumps(schema, indent=2), encoding="utf-8")
    return schemas, paths


def save(path: pathlib.Path, doc: dict):
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    tmp.replace(path)


def rejudge(args) -> int:
    """Rebuild the command lists and the off-policy flags of the stored sessions from their saved event streams,
    after the judge changed. Tokens, answers and grades are untouched."""
    if not args.fixture:
        raise SystemExit("--fixture is required (the judge checks paths against it)")
    args.fixture = str(pathlib.Path(args.fixture).resolve())
    out_path = pathlib.Path(args.out)
    doc = json.loads(out_path.read_text(encoding="utf-8"))
    for s in doc["sessions"]:
        stream = pathlib.Path(args.scratch) / f"{s['method']}-{s['set']}-{s['repeat']}-events.jsonl"
        events = parse_events(stream.read_text(encoding="utf-8"))
        s["commands"], s["off_policy_commands"] = command_block(events["commands"], s["method"], args.fixture)
        s["off_policy"] = bool(s["off_policy_commands"])
    doc["summary"] = summarise(doc["sessions"])
    doc["pgr_saving_vs_no_index"] = savings(doc["summary"])
    save(out_path, doc)
    print_table(doc["summary"], doc["pgr_saving_vs_no_index"])
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--dry", action="store_true", help="build and print the prompts and schema files; call nothing")
    ap.add_argument("--selftest", action="store_true", help="check the grader, the leak check and the command judge; call nothing")
    ap.add_argument("--fixture", help="the repository the questions are about (read-only)")
    ap.add_argument("--codex", default=find_codex(), help="codex executable (default: newest under %%LOCALAPPDATA%%\\OpenAI\\Codex\\bin)")
    ap.add_argument("--pgr", default=shutil.which("pgr"), help="pgr executable")
    ap.add_argument("--model", help="model to run (recorded from what the CLI reports)")
    ap.add_argument("--effort", default="medium", help="reasoning effort")
    ap.add_argument("--date", help="date of the run, YYYY-MM-DD (recorded as given)")
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--timeout", type=int, default=TIMEOUT, help="seconds per session before it is killed")
    ap.add_argument("--out", default=str(DEFAULT_OUT))
    ap.add_argument("--scratch", default=str(pathlib.Path(tempfile.gettempdir()) / "codex-bench"))
    ap.add_argument("--resume", action="store_true", help="keep the sessions already in --out and run only the missing ones")
    ap.add_argument("--rejudge", action="store_true",
                    help="judge the commands of the sessions in --out again from the saved event streams in --scratch; calls nothing")
    args = ap.parse_args()
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    wf = load_workflow()
    bp = load_bench_parse()
    scratch = pathlib.Path(args.scratch)
    scratch.mkdir(parents=True, exist_ok=True)
    schemas, schema_paths = write_schemas(wf, scratch)
    prompts = {m: {s: build_prompt(m, s, wf) for s in SETS} for m in METHODS}
    found = {(m, s): leaks(prompts[m][s], s, wf, bp) for m in METHODS for s in SETS}

    if args.selftest:
        return selftest(wf, bp)
    if args.dry:
        for m in METHODS:
            for s in SETS:
                print(f"===== prompt: {m}, set {s} =====\n{prompts[m][s]}\n")
        for s in SETS:
            print(f"===== answer schema, set {s} (file {schema_paths[s]}) =====\n{json.dumps(schemas[s], indent=2)}\n")
        template = argparse.Namespace(codex="<codex>", model="<model>", effort="<effort>", fixture="<fixture>")
        print("===== command =====\n" + " ".join(codex_cmd(template, "<schema>", "<out>", True)) + "   (prompt on standard input)")
        print("environment: PGR_NO_REFRESH=1, the folder of pgr first on PATH")
        print("===== leak check =====")
        for key, words in found.items():
            print(f"{key[0]:9} set {key[1]}: " + (f"LEAKS {words}" if words else "no answer in the prompt"))
        order = session_order(args.repeats)
        print("===== run order =====\n" + "\n".join(f"{i + 1:2}. set {s} repeat {r} {m}" for i, (s, r, m) in enumerate(order)))
        return 2 if any(found.values()) else 0

    if args.rejudge:
        return rejudge(args)

    for needed in ("fixture", "model", "date"):
        if not getattr(args, needed):
            ap.error(f"--{needed} is required for a real run")
    if any(found.values()):
        raise SystemExit(f"a prompt leaks an answer: {found}")
    for label, path in (("codex", args.codex), ("pgr", args.pgr)):
        if not path or not os.path.exists(path):
            raise SystemExit(f"--{label} not found: {path}")
    args.fixture = str(pathlib.Path(args.fixture).resolve())
    facts = fixture_facts(args.fixture)
    if facts["python_files"] != 86:
        raise SystemExit(f"the prompts say 86 Python files; the fixture has {facts['python_files']}")
    env = tool_env(args.pgr)
    login = run_plain([args.codex, "login", "status"])
    if "Logged in" not in login:
        raise SystemExit(f"codex is not logged in: {login}")
    before = snapshot(args.fixture)
    out_path = pathlib.Path(args.out)
    doc = json.loads(out_path.read_text(encoding="utf-8")) if args.resume and out_path.exists() else {}
    sessions = doc.get("sessions", [])
    ctx = {"prompts": prompts, "schemas": schemas, "schema_paths": schema_paths, "bp": bp, "env": env}

    if not doc.get("header"):
        probe_schema = scratch / "schema-probe.json"
        probe_schema.write_text(json.dumps({"type": "object", "properties": {"reply": {"type": "string"}}, "required": ["reply"],
                                            "additionalProperties": False}), encoding="utf-8")
        rc, out, err, timed_out, _ = run_codex(codex_cmd(args, str(probe_schema), str(scratch / "probe-out.json"), False),
                                               "Run no command. Put the word ok in the reply field.", env, args.timeout)
        limit = limit_text(out, err)
        if limit:
            print("usage limit hit before the first session:", limit)
            return 3
        header = parse_header(err)
        if rc != 0 or timed_out or "model" not in header:
            raise SystemExit(f"the header probe failed (exit {rc}): {' '.join(err.split())[-400:]}")
        header.pop("session_id", None)
        doc["header"] = sanitize(header, args.fixture)
        print("codex reports:", doc["header"])

    header = doc["header"]
    doc.update({
        "harness": "codex exec",
        "cli_version": run_plain([args.codex, "--version"]),
        "auth": login.splitlines()[0],
        "date": args.date,
        "model": header.get("model"), "effort": header.get("reasoning_effort"),
        "requested": {"model": args.model, "effort": args.effort},
        "sandbox": header.get("sandbox"), "approval": header.get("approval"),
        "feature_switches_off": list(FEATURES_OFF),
        "pgr_version": run_plain([args.pgr, "--version"], env),
        "prompts": prompts, "schemas": schemas,
        "command_template": " ".join(codex_cmd(argparse.Namespace(codex="codex", model="<model>", effort="<effort>", fixture=PLACEHOLDER),
                                               "<schema>", "<out>", True)),
        "environment": "PGR_NO_REFRESH=1; the folder of pgr first on PATH; the prompt on standard input",
        "fully_correct_rule": "mean of the three question grades from bench_parse.grade, rounded to 4 places, at least 0.999",
        "token_fields": "total_tokens = input + output; fresh_tokens = input - cached input + output (the CLI's own `tokens used`); "
                        "cached input counted inside input and reasoning inside output, as the usage record reports them",
        "limits": LIMITS, "not_measured": NOT_MEASURED, "usage_limit": None,
    })

    def store(changed: list):
        summary = summarise(sessions)
        doc.update({"fixture": {"path": PLACEHOLDER, "description": f"{facts['python_files']} Python files, {facts['lines']:,} lines",
                                "changed_by_runs": changed},
                    "sessions": sessions, "summary": summary, "pgr_saving_vs_no_index": savings(summary)})
        save(out_path, doc)

    def changed_files() -> list:
        after = snapshot(args.fixture)
        return sorted(k for k in set(before) | set(after) if before.get(k) != after.get(k))[:20]

    store(changed_files())
    done = {(s["method"], s["set"], s["repeat"]) for s in sessions}
    total = len(SETS) * len(METHODS) * args.repeats
    stopped = None
    for number, (which, rep, method) in enumerate(session_order(args.repeats), 1):
        if (method, which, rep) in done:
            continue
        print(f"[{number}/{total}] {method} set {which} repeat {rep} ...", flush=True)
        session = run_session(args, ctx, method, which, rep, number)
        used = session["usage"] or {}
        print(f"    tokens {used.get('total_tokens')}  commands {session['commands']['count']}  grades {session['grades']}  "
              f"fully correct {session['fully_correct']}  off-policy {session['off_policy']}  {session['wall_seconds']}s  "
              f"failure {session['failure']}", flush=True)
        if session["failure"] and session["failure"]["kind"] == "usage_limit":
            stopped = session["failure"]       # not a measurement: kept out of the sessions, recorded as the stop
            doc["usage_limit"] = {"hit_at_session": number, "set": which, "repeat": rep, "method": method,
                                  "message": stopped["detail"], "try_again": stopped["try_again"]}
        else:
            sessions.append(session)
        store(changed_files())
        if stopped:
            print("usage limit hit; try again at", stopped["try_again"])
            break

    print_table(doc["summary"], doc["pgr_saving_vs_no_index"])
    print("fixture changed by the runs:", doc["fixture"]["changed_by_runs"] or "no")
    return 3 if stopped else 0


if __name__ == "__main__":
    raise SystemExit(main())
