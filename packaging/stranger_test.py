"""Installs a wheel the way a stranger would, and runs pgr from it.

    python -I stranger_test.py WHEEL [--repo DIR] [--no-uv] [--keep]

The embedding model is inside the program, so the test proves two things: that the wheel installs
nothing but the program and two licence files, and that the program finds its model on a machine
where no model can be found anywhere else.

In a new virtual environment in a temporary folder it does a plain `pip install WHEEL`, then
runs pgr with an environment in which nothing but the program itself can supply a model: LXR_HOME
is a scratch folder, LXR_MODEL_DIR is unset, HF_HOME is an empty folder, HF_HUB_CACHE is unset, and
the home folder (USERPROFILE and HOME) is an empty folder too, so ~/.local-index and
~/.cache/huggingface are not seen either.

Sequence: the installed files (the model must not be among them), then, inside --repo (or a small
project the script writes): --version, help, refresh, status (must say "embeddings"), about, find;
then the program copied alone into an empty folder, where status must say "embeddings" too (the
copy that `uv tool install` and a `copy pgr.exe` leave behind). Then `pip uninstall`, and a check
that the command and the share folder are gone.

Then `uv tool install WHEEL`: uv is not assumed to be installed, so it is installed with pip into a
scratch virtual environment, and uv is given scratch tool, bin and cache folders so that nothing
outside the temporary folder is touched. The executable that uv copies into its bin folder must say
"embeddings" as well, and `uv tool uninstall` must remove it. --no-uv leaves this part out.

Every command and its full output are printed. Exit 1 when a check fails.
"""

import argparse
import os
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

SAMPLE_FILES = {
    "settings.py": '''"""Reads the settings of the program."""


def load_settings(path):
    """Reads the settings file at path and returns a dict of its lines."""
    with open(path) as handle:
        return dict(line.strip().split("=", 1) for line in handle if "=" in line)


def dispatch_command(name, args):
    """Runs the command called name with its arguments."""
    return COMMANDS[name](*args)


COMMANDS = {"settings": load_settings}
''',
    "merge.py": '''"""Git integration."""
import subprocess


def install_merge_driver(repo):
    """Registers the git merge driver for the repository."""
    subprocess.run(["git", "config", "merge.pgr.driver", "pgr merge %A %O %B"], cwd=repo, check=True)
''',
    "report.py": '''"""Prints reports."""
from settings import load_settings


def print_report(path):
    """Prints every setting in the file at path."""
    for key, value in load_settings(path).items():
        print(key, value)
''',
}


def run(command, env=None, cwd=None):
    """Runs a command, prints it with its whole output, and returns (exit code, stdout, stderr)."""
    shown = " ".join(f'"{c}"' if " " in c else c for c in command)
    print(f"\n$ {shown}" + (f"    (in {cwd})" if cwd else ""))
    done = subprocess.run(command, env=env, cwd=cwd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if done.stdout:
        print(done.stdout.rstrip("\n"))
    if done.stderr:
        print("[stderr] " + done.stderr.rstrip("\n").replace("\n", "\n[stderr] "))
    print(f"[exit {done.returncode}]")
    return done.returncode, done.stdout, done.stderr


def force_remove(function, path, _info):
    os.chmod(path, stat.S_IWRITE)
    function(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("wheel")
    parser.add_argument("--repo", help="a folder of source files to index (read only); default: a small project the script writes")
    parser.add_argument("--no-uv", action="store_true", help="leave out the `uv tool install` part")
    parser.add_argument("--keep", action="store_true", help="keep the temporary folder")
    args = parser.parse_args()

    wheel = Path(args.wheel).resolve()
    if not wheel.is_file():
        sys.exit(f"no wheel at {wheel}")
    tmp = Path(tempfile.mkdtemp(prefix="pgr-stranger-")).resolve()
    failures = []
    exe_name = "pgr.exe" if os.name == "nt" else "pgr"

    def check(ok, text):
        print(("ok    " if ok else "FAIL  ") + text)
        if not ok:
            failures.append(text)

    try:
        print(f"temporary folder: {tmp}")
        empty_home = tmp / "empty-home"
        empty_home.mkdir()
        (tmp / "empty-hf").mkdir()

        def stranger_env(lxr_home):
            """An environment in which no model can be found except one inside the program."""
            env = {k: v for k, v in os.environ.items() if k not in {"LXR_MODEL_DIR", "HF_HUB_CACHE", "PGR_NO_REFRESH", "PGR_ENGINES", "PGR_BANK"}}
            env.update(LXR_HOME=str(lxr_home), HF_HOME=str(tmp / "empty-hf"), USERPROFILE=str(empty_home), HOME=str(empty_home))
            return env

        if args.repo:
            repo = Path(args.repo)
        else:
            repo = tmp / "project"
            repo.mkdir()
            for name, text in SAMPLE_FILES.items():
                (repo / name).write_text(text, encoding="utf-8")
        about, find = "dispatch_command", "registers the git merge driver"

        def exercise(pgr, lxr_home, label):
            """The commands of the stranger's run with one program. Returns the status line."""
            env = stranger_env(lxr_home)
            code, version, _e = run([str(pgr), "--version"], env, str(repo))
            check(code == 0 and "pgr" in version.lower(), f"{label}: pgr --version answers")
            code, _o, _e = run([str(pgr), "help"], env, str(repo))
            check(code == 0, f"{label}: pgr help answers")
            code, _o, err = run([str(pgr), "refresh"], env, str(repo))
            check(code == 0 and "no embedding model" not in err, f"{label}: pgr refresh succeeds and does not say that no model was found")
            code, status, _e = run([str(pgr), "status"], env, str(repo))
            check(code == 0 and "embeddings" in status.lower(), f"{label}: pgr status says embeddings: the program found its model")
            code, out, _e = run([str(pgr), "about", about], env, str(repo))
            check(code == 0 and about in out, f"{label}: pgr about {about} names it")
            code, out, _e = run([str(pgr), "find", find], env, str(repo))
            check(code == 0 and out.strip() != "", f"{label}: pgr find answers")
            return status

        print("\n--- pip install ---")
        venv = tmp / "venv"
        run([sys.executable, "-I", "-m", "venv", str(venv)])
        scripts = venv / ("Scripts" if os.name == "nt" else "bin")
        venv_python = scripts / ("python.exe" if os.name == "nt" else "python")
        pgr = scripts / exe_name
        share = venv / "share" / "peterganurusty"
        code, _out, _err = run([str(venv_python), "-I", "-m", "pip", "install", "--disable-pip-version-check", "--no-input", str(wheel)])
        check(code == 0, "pip install of the wheel")
        check(pgr.is_file(), f"{pgr.name} is in {scripts}")
        installed = sorted(str(p.relative_to(share)) for p in share.rglob("*") if p.is_file()) if share.exists() else []
        print(f"installed under {share}: {installed}")
        check(installed == ["LICENSE.txt", "THIRD-PARTY-NOTICES.txt"], "the only files besides the program are LICENSE.txt and THIRD-PARTY-NOTICES.txt")
        check(not (share / "model").exists(), "there is no model folder: the model can only be inside the program")

        print("\n--- the stranger's run ---")
        exercise(pgr, tmp / "lxr-home", "installed")

        print("\n--- the program copied alone into an empty folder ---")
        alone = tmp / "alone"
        alone.mkdir()
        copy = alone / exe_name
        # copy, not copyfile: the copy has to keep the permission to run it (Linux and macOS).
        shutil.copy(pgr, copy)
        code, status, _e = run([str(copy), "status"], stranger_env(tmp / "lxr-home"), str(repo))
        check(code == 0 and "embeddings" in status.lower(), "the copy, with nothing else around it, says embeddings")
        # A different index home: the index is built again with the copy, without a model folder anywhere.
        code, _o, err = run([str(copy), "refresh"], stranger_env(tmp / "lxr-home-copy"), str(repo))
        check(code == 0 and "no embedding model" not in err, "the copy builds an index without saying that no model was found")
        code, status, _e = run([str(copy), "status"], stranger_env(tmp / "lxr-home-copy"), str(repo))
        check(code == 0 and "embeddings" in status.lower(), "the index built by the copy has embeddings")

        print("\n--- pip uninstall ---")
        code, _o, _e = run([str(venv_python), "-I", "-m", "pip", "uninstall", "-y", "peterganurusty"])
        check(code == 0, "pip uninstall")
        check(not pgr.exists(), f"{pgr.name} is gone")
        check(not share.exists(), f"{share} is gone")
        leftovers = sorted(str(p.relative_to(venv)) for p in (venv / "share").rglob("*")) if (venv / "share").exists() else []
        print(f"left under {venv / 'share'}: {leftovers or 'nothing'}")

        if not args.no_uv:
            print("\n--- uv tool install ---")
            uv_venv = tmp / "uv-venv"
            run([sys.executable, "-I", "-m", "venv", str(uv_venv)])
            uv_scripts = uv_venv / ("Scripts" if os.name == "nt" else "bin")
            uv_python = uv_scripts / ("python.exe" if os.name == "nt" else "python")
            code, _o, _e = run([str(uv_python), "-I", "-m", "pip", "install", "--disable-pip-version-check", "--no-input", "uv"])
            check(code == 0, "pip install uv into a scratch virtual environment")
            uv = uv_scripts / ("uv.exe" if os.name == "nt" else "uv")
            if code == 0 and uv.is_file():
                tool_dir, tool_bin = tmp / "uv-tools", tmp / "uv-bin"
                uv_env = stranger_env(tmp / "lxr-home-uv")
                uv_env.update(UV_TOOL_DIR=str(tool_dir), UV_TOOL_BIN_DIR=str(tool_bin), UV_CACHE_DIR=str(tmp / "uv-cache"), UV_PYTHON_DOWNLOADS="never")
                run([str(uv), "--version"], uv_env)
                code, _o, _e = run([str(uv), "tool", "install", "--python", sys.executable, str(wheel)], uv_env)
                check(code == 0, "uv tool install of the wheel")
                copied = tool_bin / exe_name
                check(copied.is_file(), f"uv put {copied.name} in {tool_bin}")
                if copied.is_file():
                    inside = tool_dir / "peterganurusty" / ("Scripts" if os.name == "nt" else "bin") / exe_name
                    print(f"the copy that uv made is {copied.stat().st_size} bytes; the program inside the tool environment: {inside} ({inside.stat().st_size if inside.is_file() else 'missing'} bytes)")
                    exercise(copied, tmp / "lxr-home-uv", "uv copy")
                    if inside.is_file():
                        code, status, _e = run([str(inside), "status"], stranger_env(tmp / "lxr-home-uv"), str(repo))
                        check(code == 0 and "embeddings" in status.lower(), "the program inside the uv tool environment says embeddings too")
                code, _o, _e = run([str(uv), "tool", "uninstall", "peterganurusty"], uv_env)
                check(code == 0, "uv tool uninstall")
                check(not (tool_bin / exe_name).exists(), f"{exe_name} is gone from {tool_bin}")
                left = sorted(str(p.relative_to(tool_dir)) for p in tool_dir.rglob("*") if p.is_file()) if tool_dir.exists() else []
                print(f"left under {tool_dir}: {left or 'nothing'}")
                check("peterganurusty" not in " ".join(left), "no file of the tool is left in uv's tool folder")
    finally:
        if args.keep:
            print(f"\nkept: {tmp}")
        else:
            shutil.rmtree(tmp, onerror=force_remove)

    print(f"\n{'FAILED' if failures else 'PASSED'}: {len(failures)} failing checks")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
