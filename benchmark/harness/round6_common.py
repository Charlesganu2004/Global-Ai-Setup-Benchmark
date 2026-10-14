"""What prepare_packs.py, prepare_team.py and round6_args.py share.

The question texts live in one place, token-benchmark.workflow.js. The workflow cannot read a file, so
`workflow_questions` reads the texts out of its source; nothing here repeats them.
"""
from __future__ import annotations

import hashlib
import json
import os
import pathlib
import re
import shlex
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
DATA = HERE.parent / "data"
REPO = HERE.parent.parent       # the repository; nothing that holds the owner's folders is written inside it
WORKFLOW = HERE / "token-benchmark.workflow.js"
KEYS = ("q1", "q2", "q3")
SETS = ("A", "B")
# The packs: variant -> the engines `pgr pack` is asked to use. gs is TP and TPH, g is TPG, s is TPS.
VARIANTS = {"gs": "graph,search", "g": "graph", "s": "search"}
# The pack of the arms without the neural network (TPN): variant n, made by prepare_words.py on the words-only
# copy of the fixture with PGR_NO_MODEL=1, with the same engines as gs. It is not part of VARIANTS: prepare_packs.py
# makes the three above, prepare_words.py adds this one to the same file.
WORDS_VARIANTS = {"n": "graph,search"}
# The packs the virtual machine arms use, which prepare_wsl.py makes again with the Linux build and compares.
WSL_PACK_VARIANTS = ("gs",)
PACK_BUDGET = 800
# The fixtures as the harness names them in anything that is published.
FIXTURE_PLACEHOLDER = "C:\\path\\to\\lxbench\\repo"
TEAM_PLACEHOLDER = "C:\\path\\to\\lxbench\\repo-team"
WORDS_PLACEHOLDER = "C:\\path\\to\\lxbench\\repo-words"
# The virtual machine as the harness names it in anything that is published: the home folder is /home/USER.
WSL_HOME_PLACEHOLDER = "/home/USER"
WSL_PLACEHOLDER = {"distro": "Ubuntu", "repo": "/home/USER/lxbench/repo", "pgr": "/home/USER/lxbench/bin/pgr"}
LINUX_PATH_RULE = re.compile(r"/[A-Za-z0-9._/-]{1,200}")
DISTRO_RULE = re.compile(r"[A-Za-z0-9._-]{1,64}")
NAME_RULE = re.compile(r"[A-Za-z0-9._-]{1,64}")
# PGR_* variables change what pgr prints, so a run never inherits them.
PGR_ENV = ("PGR_BANK", "PGR_TASK", "PGR_ROLE", "PGR_ENGINES", "PGR_NO_REFRESH", "PGR_NO_MODEL")


class RoundSixError(Exception):
    """Something the person who runs the round has to fix; the message says what."""


def valid_name(name: str) -> bool:
    """The name rule of the pgr contract for banks, tasks and roles."""
    return NAME_RULE.fullmatch(name) is not None and name not in (".", "..")


def est_tokens(text: str) -> int:
    """lxr_core::est_tokens: four bytes to a token, rounded up."""
    return (len(text.encode("utf-8")) + 3) // 4


def sha256(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()

def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def unescape_js(body: str) -> str:
    """The text of a single-quoted JavaScript string literal."""
    table = {"n": "\n", "t": "\t", "r": "\r"}
    return re.sub(r"\\(.)", lambda found: table.get(found.group(1), found.group(1)), body)


def workflow_questions(path: pathlib.Path = WORKFLOW) -> dict[str, dict[str, str]]:
    """The question texts of sets A, B and T, read out of the workflow source: {"A": {"q1": ...}, ...}."""
    source = path.read_text(encoding="utf-8").replace("\r\n", "\n")
    found: dict[str, dict[str, str]] = {}
    for letter, const in (("A", "Q"), ("B", "Q_B"), ("T", "Q_T")):
        block = re.search(rf"^const {const} = \{{\n(.*?)\n\}}$", source, re.M | re.S)
        if not block:
            raise RoundSixError(f"{path.name} has no `const {const} = {{ ... }}` block")
        texts = dict(re.findall(r"^\s*(q[123]):\s*'((?:[^'\\]|\\.)*)',?\s*$", block.group(1), re.M))
        if sorted(texts) != list(KEYS):
            raise RoundSixError(f"`const {const}` in {path.name} does not hold q1, q2 and q3 as single-quoted strings")
        found[letter] = {key: unescape_js(texts[key]) for key in KEYS}
    return found


def parse_version(text: str) -> tuple[int, ...]:
    """(0, 2, 0) out of "pgr 0.2.0"."""
    found = re.search(r"(\d+)\.(\d+)\.(\d+)", text)
    if not found:
        raise RoundSixError(f"cannot read a version out of {text!r}")
    return tuple(int(part) for part in found.groups())


def command_prefix(value: str) -> list[str]:
    """The program that stands for pgr, as a list. A path that exists is one word; anything else is a command
    line ('python -I stand_in.py'), split on spaces outside double quotes."""
    if os.path.exists(value):
        return [value]
    words = [word.strip('"') for word in shlex.split(value, posix=False)]
    if not words:
        raise RoundSixError("--pgr is empty")
    return words


class Pgr:
    """One way of calling pgr: a program, an index home (LXR_HOME) and fixed environment variables."""

    def __init__(self, program: str, home: str | os.PathLike | None = None, env: dict | None = None):
        self.prefix = command_prefix(program)
        self.env = {key: value for key, value in os.environ.items() if key not in PGR_ENV}
        if home:
            self.env["LXR_HOME"] = str(home)
        self.env.update(env or {})

    def with_env(self, **extra: str) -> "Pgr":
        """The same program and index home with more environment variables set."""
        clone = Pgr.__new__(Pgr)
        clone.prefix, clone.env = list(self.prefix), {**self.env, **extra}
        return clone

    def run(self, *args, text: str | None = None, check: bool = True) -> tuple[str, str, int]:
        """(standard output, standard error, exit code). Output is read as UTF-8 and nothing else."""
        argv = [*self.prefix, *(str(arg) for arg in args)]
        try:
            done = subprocess.run(argv, capture_output=True, env=self.env, input=None if text is None else text.encode("utf-8"))
        except OSError as error:
            raise RoundSixError(f"cannot start {self.prefix[0]}: {error}") from error
        try:
            out = done.stdout.decode("utf-8")
        except UnicodeDecodeError as error:
            raise RoundSixError(f"pgr {' '.join(str(a) for a in args[:5])} printed text that is not UTF-8: {error}") from error
        err = done.stderr.decode("utf-8", "replace")
        if check and done.returncode != 0:
            raise RoundSixError(f"pgr {' '.join(str(a) for a in args[:6])} exited {done.returncode}: {err.strip()[:300]}")
        return out, err, done.returncode

    def version(self) -> str:
        return self.run("--version")[0].strip()

    def require(self, minimum: tuple[int, ...] = (0, 2, 0), fix: str | None = None) -> str:
        """The version line, or an error when pgr is older than the hub commands need. `fix` says what to do
        about it; the default suits the scripts that take the program as --pgr."""
        line = self.version()
        if parse_version(line) < minimum:
            raise RoundSixError(f"{line}: the hub and the new pack need pgr {'.'.join(map(str, minimum))} or newer. "
                                + (fix or "Build it (rust\\cargo.ps1 build --release) and pass its path as --pgr"))
        return line


def index_ready(status: str) -> bool:
    """Whether a `pgr status` line says that this pgr can read the graph and the search index of the root. An
    index is read only by the version that built it: pgr 0.2 calls an index that pgr 0.1 built "no index yet"."""
    return status.startswith("pgr: ") and "graph ready" in status and "search ready" in status


def default_home(env: dict | None = None) -> pathlib.Path:
    """The index home pgr uses: LXR_HOME, else ~/.local-index/rust."""
    given = (os.environ if env is None else env).get("LXR_HOME")
    return pathlib.Path(given) if given else pathlib.Path.home() / ".local-index" / "rust"


def ps_quote(word: str) -> str:
    """A word as PowerShell takes it: single quotes keep backticks and dollar signs as they are."""
    if re.fullmatch(r"[A-Za-z0-9_.:\\/=,@%+-]+", word):
        return word
    return "'" + word.replace("'", "''") + "'"


def render(argv: list[str]) -> str:
    return " ".join(ps_quote(word) for word in argv)


def private_paths(*paths) -> list[str]:
    """Every spelling of the paths that must never be written into a published file."""
    spellings = set()
    for path in paths:
        if not path:
            continue
        # A relative path would match ordinary words, so only absolute spellings count.
        for text in {str(pathlib.Path(path).resolve())} | ({str(path)} if pathlib.Path(path).is_absolute() else set()):
            spellings.update({text, text.replace("\\", "/"), text.replace("/", "\\")})
    home = str(pathlib.Path.home())
    spellings.update({home, home.replace("\\", "/")})
    # The same paths as they are written inside a JSON string.
    spellings.update({spell.replace("\\", "\\\\") for spell in spellings})
    return sorted(spell.lower() for spell in spellings if spell)


def refuse_private(text: str, spellings: list[str], what: str) -> None:
    """A published file may not hold the owner's folders: only the C:\\path\\to\\... placeholders."""
    lowered = text.lower()
    for spelling in spellings:
        if spelling and spelling in lowered:
            raise RoundSixError(f"{what} holds the path {spelling!r}; only the placeholder paths may appear in a published file")
    if re.search(r"(?i)[a-z]:[\\/]+users[\\/]", text):
        raise RoundSixError(f"{what} holds a path under a Users folder")


def write_json(path: pathlib.Path, data, private: tuple = ()) -> None:
    """The way the data files are written: one space of indent, UTF-8, line feeds, a closing newline. The
    paths in `private` (and the home folder) must not appear anywhere in the text."""
    text = json.dumps(data, indent=1, ensure_ascii=False) + "\n"
    refuse_private(text, private_paths(*private), path.name)
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_name(path.name + ".tmp")
    temp.write_bytes(text.encode("utf-8"))
    os.replace(temp, path)


def check_packs(doc: dict, questions: dict[str, dict[str, str]], extra: tuple[str, ...] = ()) -> list[str]:
    """What is wrong with a packs.json document: a missing pack, a pack made for another question text,
    a token count or hash that does not match its output. An empty list is a good file. `extra` names
    variants beyond gs, g and s (the words-only variant n) that are required too."""
    problems = []
    packs = doc.get("packs")
    if not isinstance(packs, dict):
        return ["no `packs` object"]
    for variant in (*VARIANTS, *extra):
        for letter in SETS:
            for key in KEYS:
                pack = ((packs.get(variant) or {}).get(letter) or {}).get(key)
                where = f"packs.{variant}.{letter}.{key}"
                if not isinstance(pack, dict):
                    problems.append(f"{where} is missing")
                    continue
                output = pack.get("output")
                if not isinstance(output, str) or not output:
                    problems.append(f"{where} has no output")
                    continue
                if pack.get("question") != questions[letter][key]:
                    problems.append(f"{where} was made for another question text than the workflow asks")
                if pack.get("tokens") != est_tokens(output):
                    problems.append(f"{where}: tokens {pack.get('tokens')} is not the estimate {est_tokens(output)} of its output")
                if pack.get("sha256") != sha256(output):
                    problems.append(f"{where}: sha256 is not the hash of its output")
                if not isinstance(pack.get("command"), str) or "pack" not in pack["command"]:
                    problems.append(f"{where} has no command")
    return problems


# The status line without its age: two builds of one index differ in it and in nothing else.
AGE = re.compile(r", \d+[smhd] old\)")


def stable_status(status: str) -> str:
    return AGE.sub(")", status)


def words_only(status: str) -> bool:
    """Whether a status line says that the search index works on words alone (no embedding table)."""
    return "words only" in status


def decode_wsl(data: bytes) -> str:
    """Text that came back through wsl.exe. A program's own output is what it wrote (UTF-8 here); the
    messages of wsl.exe itself are UTF-16, which shows as a NUL after every character."""
    if b"\x00" in data:
        try:
            return data.decode("utf-16-le").lstrip("﻿")
        except UnicodeDecodeError:
            pass
    return data.decode("utf-8", "replace")


class Wsl:
    """wsl.exe, used the one way the benchmark uses it: `wsl.exe -d DISTRO [--cd DIR] --exec PROGRAM ARGS...`.
    --exec starts the program directly with the arguments exactly as given and no shell between, so a pack
    question with backticks and quotes reaches pgr unchanged. `program` is `wsl.exe`, or a command line for
    a stand-in (fake_wsl.py) in the tests. Nothing here changes anything inside the VM on its own: each
    caller says what it runs."""

    def __init__(self, distro: str, program: str = "wsl.exe"):
        if not DISTRO_RULE.fullmatch(distro or ""):
            raise RoundSixError(f"{distro!r} is not a distribution name (1 to 64 characters of A-Z a-z 0-9 . _ -)")
        self.distro = distro
        self.prefix = command_prefix(program)
        self._home: str | None = None

    def run(self, *argv, cd: str | None = None, env: dict | None = None, check: bool = True,
            text: str | None = None) -> tuple[str, str, int]:
        """(standard output, standard error, exit code). `env` is set for the program with env(1), because
        wsl.exe passes none of the Windows environment on."""
        words = [*self.prefix, "-d", self.distro, *(["--cd", cd] if cd else []), "--exec"]
        if env:
            words += ["env", *(f"{key}={value}" for key, value in sorted(env.items()))]
        words += [str(word) for word in argv]
        try:
            done = subprocess.run(words, capture_output=True, input=None if text is None else text.encode("utf-8"),
                                  stdin=subprocess.DEVNULL if text is None else None)
        except OSError as error:
            raise RoundSixError(f"cannot start {self.prefix[0]}: {error}") from error
        out, err = decode_wsl(done.stdout), decode_wsl(done.stderr)
        if check and done.returncode != 0:
            raise RoundSixError(f"in the VM {self.distro}: {' '.join(str(a) for a in argv[:6])} exited {done.returncode}: {(err or out).strip()[:300]}")
        return out, err, done.returncode

    def home(self) -> str:
        """The home folder of the user inside the VM."""
        if self._home is None:
            found = self.run("printenv", "HOME")[0].strip()
            if not LINUX_PATH_RULE.fullmatch(found):
                raise RoundSixError(f"the home folder in the VM, {found!r}, is not a plain Linux path (letters, digits and . _ - / only)")
            self._home = found
        return self._home

    def index_home(self) -> str:
        """The index home of pgr inside the VM: LXR_HOME when it is set there, else ~/.local-index/rust."""
        given, _, code = self.run("printenv", "LXR_HOME", check=False)
        return given.strip() if code == 0 and given.strip() else f"{self.home()}/.local-index/rust"

    def linux_path(self, windows_path: str | os.PathLike) -> str:
        """The path inside the VM of a Windows file or folder (wslpath, so a different mount root works too)."""
        return self.run("wslpath", "-a", "-u", str(windows_path))[0].strip()


# What a model class is called in a count, by the tier letter of the matrix files and by the reviewer of a setup.
TIER_CLASS = {"C": "light", "B": "middle", "A": "top"}
REVIEWER_CLASS = {"orch": "top", "orch-notools": "top", "orch-mid": "middle", "orch-low": "light"}


def count_matrix(matrix: list[dict]) -> dict:
    """How many cells a matrix starts and how many agents, by class of model. A single cell is one agent, a
    parallel cell three, an orchestrated cell five: a planner (top class), three workers (the tier of the
    block) and a reviewer (the class its setup names). `planners_saved_by_briefs` is how many of those
    planners args.briefs replaces: the orchestrated cells on set A, whose ids do not end in h."""
    classes = {"light": 0, "middle": 0, "top": 0}
    cells = planners = saved = 0
    for block in matrix:
        tier = TIER_CLASS[block["tier"]]
        for arm in block["arms"]:
            for _ in block["reps"]:
                cells += 1
                if block["setup"] == "single":
                    classes[tier] += 1
                elif block["setup"] == "parallel":
                    classes[tier] += 3
                else:
                    classes[tier] += 3
                    classes["top"] += 1
                    classes[REVIEWER_CLASS[block["setup"]]] += 1
                    planners += 1
                    saved += not arm.endswith("h")
    return {"cells": cells, "agents": sum(classes.values()), "by_class": classes,
            "planners": planners, "planners_saved_by_briefs": saved}
