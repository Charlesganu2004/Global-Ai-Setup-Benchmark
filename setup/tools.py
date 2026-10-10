#!/usr/bin/env python3
"""Install the local index tools and the `lx` front-end that agents type.

    python tools.py [--offline] [--bin DIR] [--skip-model] [--home DIR]
                    [--token-goat [CLIENTS]]

What it installs, each only when it is not already there:
  semble 0.6.1 and ast-grep 0.45.3   pinned prebuilt wheels from PyPI, in a
                                     private virtual environment. No package
                                     uploaded after CUTOFF is accepted, so a
                                     release from yesterday cannot slip in
                                     through a dependency.
  graphify 0.9.72                    the same way, when no graphify is found.
  codanna 0.16.0                     the Rust symbol index: one release file from
                                     GitHub, refused unless its SHA-256 matches
                                     the pinned value.
  the semble embedding model         about 33 MB from huggingface.co, fetched
                                     once so every later search runs offline.
  lx                                 the front-end command. Needs no download.

--offline skips every download and still makes `lx`, which then answers from
whichever engines the machine already has.

--token-goat is optional and off unless asked for. It installs token-goat
2.9.30 from the npm registry when the command is not there yet (it needs
Node.js 22.16 or newer, and is about 350 MB installed; only that package is
pinned, npm picks its dependencies on the day and some run install scripts).
It then runs token-goat's own installer for each client, which writes that
client's hooks and a routing block in its rules file, and also a skill for
Claude Code, trust entries for its hooks in Codex's config.toml, and an MCP
server entry in Copilot's mcp-config.json. token-goat is under the PolyForm
Noncommercial licence, so it is not for commercial use. Undo it with
`token-goat uninstall`, again with --codex and with --copilot, then
`npm uninstall -g token-goat`.

The index tools above are never compiled, no PATH entry is edited, and it is
safe to run again.
"""
from __future__ import annotations

import argparse
import hashlib
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

PINS = {
    "semble": "semble==0.6.1",
    "ast-grep": "ast-grep-cli==0.45.3",
    # Only installed when no graphify is found. 0.9.70 is the first release
    # without GHSA-pcc4-rvhr-2pr8.
    "graphify": "graphifyy==0.9.72",
}
CUTOFF = "2026-09-30T00:00:00Z"
# Optional, only with --token-goat. The version the benchmark was run with, and
# the oldest Node.js that package says it runs on.
TOKEN_GOAT = "token-goat@2.9.30"
TOKEN_GOAT_NODE = (22, 16)
TOKEN_GOAT_FLAGS = {"claude": [], "codex": ["--codex"], "copilot": ["--copilot"]}

# The publisher's own checksum for this exact file, read from the release's
# SHA256SUMS on 2026-10-08. The binary is unsigned, so this pin is the check.
CODANNA = {
    "version": "0.16.0",
    "url": "https://github.com/bartolli/codanna/releases/download/v0.16.0/"
           "codanna-0.16.0-windows-x64.zip",
    "sha256": "2359f3fff4f27efa107aac89d64ae6033922888e782fd441139e473a5f0558fb",
    "member": "codanna-0.16.0-windows-x64/codanna.exe",
}

WINDOWS = os.name == "nt"
SCRIPTS = "Scripts" if WINDOWS else "bin"
EXE = ".exe" if WINDOWS else ""

OFFLINE_PTH = "local_index_offline.pth"
OFFLINE_LINE = ('import os; os.environ.setdefault("HF_HUB_OFFLINE", "1"); '
                'os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")\n')

# What `lx` runs. It only hands over to the hook script, so updating that
# script updates the command with it.
FRONT_MODULE = '''"""Entry point of the `lx` command."""
import pathlib
import runpy
import sys


def main():
    script = pathlib.Path.home() / ".local-index" / "session_index.py"
    if not script.is_file():
        sys.exit("lx: ~/.local-index/session_index.py is missing; run apply.py")
    sys.argv[0] = str(script)
    runpy.run_path(str(script), run_name="__main__")
'''
MAKE_LAUNCHER = ("import sys; from pip._vendor.distlib.scripts import ScriptMaker; "
                 "maker = ScriptMaker(None, sys.argv[1]); maker.clobber = True; "
                 "maker.variants = {''}; maker.executable = sys.executable; "
                 "maker.make('lx = local_index_front:main')")


def run(argv: list[str], env: dict | None = None, timeout: int = 1800,
        cwd: pathlib.Path | None = None) -> tuple[int, str]:
    try:
        done = subprocess.run([str(part) for part in argv], env=env, timeout=timeout,
                              cwd=str(cwd) if cwd else None,
                              stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                              stdin=subprocess.DEVNULL)
        return done.returncode, done.stdout.decode("utf-8", "replace")
    except (OSError, subprocess.SubprocessError) as error:
        return 127, str(error)


def ensure_venv(path: pathlib.Path) -> pathlib.Path | None:
    """A private environment. Creating one needs no network."""
    python = path / SCRIPTS / f"python{EXE}"
    if not python.is_file():
        code, out = run([sys.executable, "-m", "venv", path])
        if code != 0:
            print(f"FAILED     venv {path}: {out.strip()[-300:]}")
            return None
    return python


def site_packages(venv: pathlib.Path) -> pathlib.Path | None:
    if WINDOWS:
        return venv / "Lib" / "site-packages"
    return next(iter(sorted((venv / "lib").glob("python3*/site-packages"))), None)


def pip_install(python: pathlib.Path, requirement: str) -> bool:
    def knows_cutoff() -> bool:
        return "--uploaded-prior-to" in run([python, "-m", "pip", "install", "--help"],
                                            timeout=120)[1]
    if not knows_cutoff():
        # The pip bundled with a new environment is usually too old for the flag.
        run([python, "-m", "pip", "install", "--quiet", "--upgrade", "pip"])
    argv = [python, "-m", "pip", "install", "--quiet", "--only-binary=:all:"]
    if knows_cutoff():
        argv += ["--uploaded-prior-to", CUTOFF]
    else:
        print(f"WARNING    this pip has no --uploaded-prior-to; {requirement} is pinned, "
              "its dependencies are not date-limited")
    code, out = run(argv + [requirement])
    if code != 0:
        print(f"FAILED     pip install {requirement}: {out.strip()[-400:]}")
    return code == 0


def on_path(directory: pathlib.Path) -> bool:
    wanted = os.path.normcase(os.path.normpath(str(directory)))
    return any(os.path.normcase(os.path.normpath(entry)) == wanted
               for entry in os.environ.get("PATH", "").split(os.pathsep) if entry)


def pick_bin(home: pathlib.Path, override: str) -> pathlib.Path:
    """A launcher directory that is already on PATH, so nothing has to edit PATH."""
    if override:
        return pathlib.Path(override)
    candidates = [home / "bin", home / ".local" / "bin"]
    if WINDOWS:
        local = pathlib.Path(os.environ.get("LOCALAPPDATA", home / "AppData" / "Local"))
        candidates.append(local / "Microsoft" / "WindowsApps")
    for directory in candidates:
        if directory.is_dir() and on_path(directory):
            return directory
    return candidates[0]


def write_launcher(bin_dir: pathlib.Path, name: str, target: pathlib.Path) -> None:
    """Make `name` runnable by bare name in PowerShell, cmd and Git Bash.

    On Windows that is a copy of the real .exe. A .cmd wrapper would hand every
    argument to cmd.exe for a second parse, where & | < > in a search query
    start another command, and Git Bash cannot find a .cmd by bare name at all.
    pip's launchers carry the absolute path of their own interpreter, so a copy
    runs from anywhere.
    """
    bin_dir.mkdir(parents=True, exist_ok=True)
    if WINDOWS:
        destination = bin_dir / f"{name}.exe"
        if not destination.is_file() or destination.read_bytes() != target.read_bytes():
            shutil.copy2(target, destination)
        return
    link = bin_dir / name
    if link.is_symlink() or link.exists():
        link.unlink()
    link.symlink_to(target)


def keep_semble_offline(venv: pathlib.Path) -> None:
    """Default this private environment to offline Hugging Face access.

    semble asks the Hub about its model on every run unless told not to, which
    is a network call and several seconds. A .pth line is how Python lets an
    environment set that for itself; an explicit HF_HUB_OFFLINE=0 still wins.
    """
    packages = site_packages(venv)
    if packages and packages.is_dir():
        (packages / OFFLINE_PTH).write_text(OFFLINE_LINE, encoding="ascii")


def install_semble_and_ast_grep(home: pathlib.Path, bin_dir: pathlib.Path,
                                offline: bool) -> None:
    venv = home / "Tools" / "codesearch-venv"
    semble, ast_grep = venv / SCRIPTS / f"semble{EXE}", venv / SCRIPTS / f"ast-grep{EXE}"
    python = ensure_venv(venv)
    if python is None:
        return
    if not offline:
        if not semble.is_file():
            pip_install(python, PINS["semble"])
        # Separate installs: antivirus has quarantined ast-grep's binary before,
        # and that must not take semble down with it.
        if not ast_grep.is_file():
            pip_install(python, PINS["ast-grep"])
    keep_semble_offline(venv)
    if semble.is_file():
        write_launcher(bin_dir, "semble", semble)
    if ast_grep.is_file():
        write_launcher(bin_dir, "ast-grep", ast_grep)


def install_graphify(home: pathlib.Path, bin_dir: pathlib.Path, offline: bool) -> None:
    own = home / ".graphify" / "venv" / SCRIPTS / f"graphify{EXE}"
    if not own.is_file():
        if offline or on_search_path("graphify"):
            return  # nothing to fetch, or installed some other way and on PATH
        python = ensure_venv(home / ".graphify" / "venv")
        if python is None or not pip_install(python, PINS["graphify"]) or not own.is_file():
            return
    write_launcher(bin_dir, "graphify", own)


def install_codanna(bin_dir: pathlib.Path) -> None:
    target = bin_dir / "codanna.exe"
    if not WINDOWS:
        print("skipped    codanna: only its Windows build is pinned here; "
              "lx answers from graphify instead")
        return
    if target.is_file() and CODANNA["version"] in run([target, "--version"], timeout=60)[1]:
        return
    with tempfile.TemporaryDirectory() as scratch:
        archive = pathlib.Path(scratch) / "codanna.zip"
        try:
            with urllib.request.urlopen(CODANNA["url"], timeout=180) as response, \
                    archive.open("wb") as stream:
                shutil.copyfileobj(response, stream)
        except OSError as error:
            print(f"FAILED     codanna download: {error}. lx answers from graphify instead.")
            return
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        if digest != CODANNA["sha256"]:
            print(f"FAILED     codanna checksum mismatch (got {digest}); not installed")
            return
        with zipfile.ZipFile(archive) as bundle:
            data = bundle.read(CODANNA["member"])
    bin_dir.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)


def make_front_end(home: pathlib.Path, bin_dir: pathlib.Path) -> None:
    """Create `lx`. On Windows it is a real launcher .exe, made the way pip makes them."""
    venv = home / "Tools" / "codesearch-venv"
    python = ensure_venv(venv)
    packages = site_packages(venv) if python else None
    if python is None or packages is None:
        return
    (packages / "local_index_front.py").write_text(FRONT_MODULE, encoding="utf-8")
    bin_dir.mkdir(parents=True, exist_ok=True)
    if WINDOWS:
        code, out = run([python, "-c", MAKE_LAUNCHER, bin_dir], timeout=120)
        if code != 0 or not (bin_dir / "lx.exe").is_file():
            print(f"FAILED     lx front-end: {out.strip()[-300:]}")
        return
    script = bin_dir / "lx"
    script.write_text(f"#!{python}\nimport local_index_front\nlocal_index_front.main()\n",
                      encoding="utf-8")
    script.chmod(0o755)


def warm_model(home: pathlib.Path) -> str:
    """Fetch the embedding model once, so later searches never need the network."""
    semble = home / "Tools" / "codesearch-venv" / SCRIPTS / f"semble{EXE}"
    if not semble.is_file():
        return "semble missing"
    with tempfile.TemporaryDirectory() as scratch:
        (pathlib.Path(scratch) / "sample.py").write_text(
            "def greet(name):\n    return 'hello ' + name\n", encoding="utf-8")
        argv = [semble, "search", "greeting", scratch, "-k", "1", "--max-snippet-lines", "0"]
        quiet = dict(os.environ, HF_HUB_DISABLE_SYMLINKS_WARNING="1")
        code, _ = run(argv, env=dict(quiet, HF_HUB_OFFLINE="1"), timeout=300)
        if code == 0:
            return "model already cached"
        code, out = run(argv, env=dict(quiet, HF_HUB_OFFLINE="0"), timeout=900)
        return "model downloaded" if code == 0 else f"model NOT cached: {out.strip()[-200:]}"


def report(home: pathlib.Path, bin_dir: pathlib.Path) -> bool:
    engines = {
        "codanna": bin_dir / f"codanna{EXE}",
        "graphify": home / ".graphify" / "venv" / SCRIPTS / f"graphify{EXE}",
        "semble": home / "Tools" / "codesearch-venv" / SCRIPTS / f"semble{EXE}",
        "ast-grep": bin_dir / f"ast-grep{EXE}",
    }
    complete = True
    for name, path in engines.items():
        target = str(path) if path.is_file() else on_search_path(name)
        if not target:
            print(f"MISSING    {name}")
            complete = False
            continue
        _, out = run([target, "--version"], timeout=60)
        version = out.strip().splitlines()[-1] if out.strip() else "?"
        print(f"ready      {name:<9} {version:<24} {target}")
    print(f"{'ready' if shutil.which('rg') else 'optional'}      rg        "
          f"{shutil.which('rg') or 'ripgrep not found; the clients ship their own search'}")
    front = bin_dir / ("lx.exe" if WINDOWS else "lx")
    if front.is_file():
        print(f"ready      lx        front-end                {front}")
    else:
        print("MISSING    lx")
        complete = False
    if not on_path(bin_dir):
        print(f"ACTION     {bin_dir} is not on PATH. Add it, then open a new terminal.")
        complete = False
    return complete


def on_search_path(name: str) -> str:
    """Where a command lives on PATH, never in the folder this was started from.

    shutil.which looks in the current directory first on Windows, so a file
    named like the command and lying there would be run in place of the real one.
    """
    here = pathlib.Path.cwd().resolve()
    endings = os.environ.get("PATHEXT", ".COM;.EXE;.BAT;.CMD").split(os.pathsep) if WINDOWS else [""]
    for folder in os.environ.get("PATH", "").split(os.pathsep):
        try:
            if not folder or pathlib.Path(folder).resolve() == here:
                continue
        except OSError:
            continue
        for ending in endings:
            candidate = pathlib.Path(folder) / (name + ending.lower())
            if candidate.is_file() and (WINDOWS or os.access(candidate, os.X_OK)):
                return str(candidate)
    return ""


def shim_safe(command: str) -> bool:
    """npm's Windows commands are .cmd files, which cmd.exe runs: it splits an
    unquoted path at these characters, and Python quotes a path only for a space."""
    return not (command.lower().endswith((".cmd", ".bat")) and any(mark in command for mark in '&^%!<>|"'))


def node_version() -> tuple[int, ...] | None:
    node = on_search_path("node")
    if not node:
        return None
    _, out = run([node, "--version"], timeout=60)
    try:
        return tuple(int(part) for part in out.strip().lstrip("v").split(".")[:2])
    except ValueError:
        return None


def install_token_goat(home: pathlib.Path, wanted: str, offline: bool) -> bool:
    """The optional step: token-goat itself, then its own installer for each client.

    token-goat writes into the real home directory whatever it is told, so this
    never runs when --home points somewhere else: a trial install must not put
    hooks into the clients you actually use.

    Returns False only for a failure, and prints a line starting FAILED for
    each one. A step that was rightly left out prints `skipped` and is not a failure.
    """
    if home != pathlib.Path.home().resolve():
        print("skipped    token-goat: --home is not your home directory, and its hooks "
              "always go to the real one")
        return True
    named = [name.strip().lower() for name in wanted.split(",") if name.strip()]
    unknown = [name for name in named if name != "all" and name not in TOKEN_GOAT_FLAGS]
    if unknown or not named:
        print("FAILED     token-goat: " + (f"unknown client {', '.join(unknown)}" if unknown else "no client named")
              + f"; name all, or any of {', '.join(TOKEN_GOAT_FLAGS)}")
        return False
    clients = [name for name in TOKEN_GOAT_FLAGS
               if name in named or ("all" in named and (home / f".{name}").is_dir())]
    if not clients:
        print("skipped    token-goat: none of ~/.claude, ~/.codex and ~/.copilot exists")
        return True
    exe = on_search_path("token-goat")
    if not exe:
        if offline:
            print("skipped    token-goat: it is not installed, and --offline downloads nothing")
            return True
        npm, node = on_search_path("npm"), node_version()
        floor = ".".join(str(part) for part in TOKEN_GOAT_NODE)
        if not npm or node is None:
            print(f"FAILED     token-goat: npm and node are not both on PATH; it needs Node.js {floor} or newer")
            return False
        if node < TOKEN_GOAT_NODE:
            print(f"FAILED     token-goat: Node.js {'.'.join(str(part) for part in node)} found; "
                  f"it needs {floor} or newer")
            return False
        if not shim_safe(npm):
            print(f"FAILED     token-goat: cannot run npm safely from {npm}")
            return False
        print(f"installing {TOKEN_GOAT} from the npm registry into this user's global packages. Only that "
              "package is pinned: npm picks its dependencies today, and some run install scripts.", flush=True)
        # From the home folder: npm reads the .npmrc of the folder it is started in, even for -g.
        code, out = run([npm, "install", "-g", TOKEN_GOAT], cwd=home)
        exe = on_search_path("token-goat")
        if code != 0 or not exe:
            said = out.strip().splitlines()[-1][:200] if out.strip() else "no output"
            print(f"FAILED     token-goat: npm did not leave a token-goat command on PATH ({plain(said)})")
            return False
    if not shim_safe(exe):
        print(f"FAILED     token-goat: cannot run it safely from {exe}")
        return False
    ok = True
    for client in clients:
        # --no-index: installing hooks must not start indexing whatever folder this runs in.
        code, out = run([exe, "install", *TOKEN_GOAT_FLAGS[client], "--no-index"], timeout=600, cwd=home)
        ok = ok and code == 0
        print(("installed  " if code == 0 else "FAILED     ") + f"token-goat for {client}")
        for line in out.strip().splitlines()[-8:]:
            print("           " + plain(line)[:200])     # what it wrote, or why it could not
    print("note       token-goat is under the PolyForm Noncommercial licence: not for commercial use. "
          "Undo with `token-goat uninstall`, `token-goat uninstall --codex` and `token-goat uninstall "
          "--copilot`, then `npm uninstall -g token-goat` to remove the package itself.")
    return ok


def plain(text: str) -> str:
    """Another program's output, safe to print on a console with any code page."""
    return text.encode("ascii", "replace").decode("ascii")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--offline", action="store_true",
                        help="download nothing; only make lx and the launchers")
    parser.add_argument("--bin", default="", help="directory for the launchers (default: "
                        "the first of ~/bin, ~/.local/bin, WindowsApps that is on PATH)")
    parser.add_argument("--skip-model", action="store_true")
    parser.add_argument("--home", type=pathlib.Path, default=pathlib.Path.home())
    parser.add_argument("--token-goat", nargs="?", const="all", default=None, metavar="CLIENTS",
                        help="optional: also install token-goat and its hooks, for every client "
                             "folder found or for a comma-separated list of claude, codex, copilot")
    args = parser.parse_args()
    home = args.home.resolve()
    bin_dir = pick_bin(home, args.bin)

    install_semble_and_ast_grep(home, bin_dir, args.offline)
    install_graphify(home, bin_dir, args.offline)
    if not args.offline:
        install_codanna(bin_dir)
    make_front_end(home, bin_dir)
    if not args.offline and not args.skip_model:
        print(f"semble     {warm_model(home)}")
    complete = report(home, bin_dir)
    wanted = args.token_goat is None or install_token_goat(home, args.token_goat, args.offline)
    # --offline forgives an index tool it was told not to fetch. It never forgives the optional step.
    return 0 if wanted and (complete or args.offline) else 1


if __name__ == "__main__":
    sys.exit(main())
