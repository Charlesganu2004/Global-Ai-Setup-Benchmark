"""Checks for the optional token-goat step of tools.py.

    python check_token_goat_step.py PATH/TO/tools.py

Every outside command is faked and the home directory is a throwaway one, so
nothing is installed and no client file is touched.
"""
import contextlib
import importlib.util
import io
import pathlib
import sys
import tempfile

spec = importlib.util.spec_from_file_location("tools", sys.argv[1])
tools = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tools)

failed = []


def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (" | " + detail if detail and not ok else ""))
    if not ok:
        failed.append(name)


def drive(wanted, *, offline=False, have=("token-goat", "npm"), node=(24, 15), codes=None,
          folders=("claude", "codex", "copilot"), trial=False, shim="C:/fake/{}.cmd"):
    """Run the step in a throwaway home. It counts as the real home unless `trial` is set, in which
    case the real home is some other folder. Returns (result, printed, calls); the folder each
    command ran in is kept in drive.ran_in, with drive.home the home that was used."""
    calls = []
    drive.ran_in = []
    codes = codes or {}
    with tempfile.TemporaryDirectory() as scratch:
        home = pathlib.Path(scratch).resolve()
        for name in folders:
            (home / f".{name}").mkdir()
        present = set(have)

        drive.home = home

        def fake_run(argv, env=None, timeout=1800, cwd=None):
            calls.append([str(part) for part in argv])
            drive.ran_in.append(cwd)
            if "install" in argv and "-g" in argv:
                if codes.get("npm", 0) == 0:
                    present.add("token-goat")
                return codes.get("npm", 0), "added 1 package"
            key = "codex" if "--codex" in argv else "copilot" if "--copilot" in argv else "claude"
            return codes.get(key, 0), "Installed token-goat hooks (user) \u2192 somewhere\nUpdated rules \u2192 elsewhere"

        saved = (tools.run, tools.on_search_path, tools.node_version, tools.pathlib.Path.home)
        tools.run = fake_run
        tools.on_search_path = lambda name: shim.format(name) if name in present else ""
        tools.node_version = lambda: node
        elsewhere = home / "the-real-home"
        tools.pathlib.Path.home = classmethod(lambda cls: elsewhere if trial else home)
        out = io.StringIO()
        try:
            with contextlib.redirect_stdout(out):
                result = tools.install_token_goat(home, wanted, offline)
        finally:
            tools.run, tools.on_search_path, tools.node_version, tools.pathlib.Path.home = saved
    return result, out.getvalue(), calls


result, said, calls = drive("claude,bogus")
check("unknown client: fails and names it", result is False and "FAILED" in said and "bogus" in said and not calls, said)
result, said, calls = drive("")
check("no client named: fails", result is False and said.startswith("FAILED") and not calls, said)
result, said, calls = drive("all,codex", folders=("claude",))
check("all anywhere in the list means every folder found, plus the named ones",
      result is True and [c for c in calls] == [["C:/fake/token-goat.cmd", "install", "--no-index"],
                                                ["C:/fake/token-goat.cmd", "install", "--codex", "--no-index"]], str(calls))
result, said, calls = drive("all", folders=())
check("all with no client folder: skipped, not a failure", result is True and said.startswith("skipped") and not calls, said)
result, said, calls = drive("claude", offline=True, have=())
check("not installed and offline: skipped, not a failure, nothing run", result is True and "skipped" in said and not calls, said)
result, said, calls = drive("claude", have=())
check("no npm: a FAILED line and a failure", result is False and said.startswith("FAILED") and "22.16" in said and not calls, said)
result, said, calls = drive("claude", have=("npm",), node=(20, 11))
check("old Node.js: refused before npm runs", result is False and "FAILED" in said and "20.11" in said and not calls, said)
result, said, calls = drive("claude", have=("npm",), node=None)
check("npm without node: refused", result is False and "FAILED" in said and not calls, said)
result, said, calls = drive("claude", have=("npm",))
check("installs the pinned package, then the hooks", result is True and calls[0][1:] == ["install", "-g", tools.TOKEN_GOAT]
      and calls[1][1:] == ["install", "--no-index"], str(calls))
result, said, calls = drive("claude", have=("npm",), codes={"npm": 1})
check("npm fails: a FAILED line and a failure", result is False and "FAILED" in said and len(calls) == 1, said)
result, said, calls = drive("claude,copilot", codes={"claude": 1})
check("the first client's installer fails: FAILED for it, the step fails, the next client still runs",
      result is False and "FAILED     token-goat for claude" in said and "installed  token-goat for copilot" in said
      and len(calls) == 2 and "--copilot" in calls[1], said + str(calls))
result, said, calls = drive("claude")
check("its output is printed without characters a console may not have",
      result is True and "? somewhere" in said and "\u2192" not in said, said)
check("the note names all three uninstall commands and the package removal",
      all(word in said for word in ("`token-goat uninstall`,", "uninstall --codex", "uninstall --copilot",
                                    "npm uninstall -g token-goat")), said)
result, said, calls = drive("claude", have=("npm",))
check("npm and the installer both run in the home folder, never where the installer was started",
      result is True and len(drive.ran_in) == 2 and all(folder == drive.home for folder in drive.ran_in), str(drive.ran_in))
result, said, calls = drive("claude", trial=True)
check("a trial home is refused: nothing runs, and that is not a failure",
      result is True and said.startswith("skipped") and "--home" in said and not calls, said + str(calls))
result, said, calls = drive("claude", shim="C:/Users/R&D/npm/{}.cmd")
check("a token-goat command under a path cmd.exe would split is refused before it runs",
      result is False and "FAILED" in said and not calls, said + str(calls))
result, said, calls = drive("claude", have=("npm",), shim="C:/Users/R&D/npm/{}.cmd")
check("an npm under such a path is refused before it runs", result is False and "FAILED" in said and not calls, said + str(calls))

# shim_safe and on_search_path, for real
check("a .cmd under a path with & is refused", not tools.shim_safe(r"C:\Users\R&D\npm\token-goat.cmd"))
check("a .cmd under an ordinary path is accepted", tools.shim_safe(r"C:\Users\Jane Doe\npm\token-goat.cmd"))
with tempfile.TemporaryDirectory() as scratch:
    import os
    here = pathlib.Path(scratch).resolve()
    (here / ("planted-command.cmd" if os.name == "nt" else "planted-command")).write_text("@echo off\n", encoding="ascii")
    if os.name != "nt":
        os.chmod(here / "planted-command", 0o755)
    old_cwd, old_path = os.getcwd(), os.environ.get("PATH", "")
    os.chdir(here)
    try:
        check("on_search_path ignores the current folder", tools.on_search_path("planted-command") == "")
        os.environ["PATH"] = str(here) + os.pathsep + old_path
        check("even when the current folder is on PATH", tools.on_search_path("planted-command") == "")
        check("and still finds a real command", bool(tools.on_search_path("git")), "git not found")
    finally:
        os.chdir(old_cwd)
        os.environ["PATH"] = old_path

# exit status of main(): --offline never forgives the optional step
def exit_of(goat_result, argv):
    saved = (tools.install_semble_and_ast_grep, tools.install_graphify, tools.install_codanna, tools.make_front_end,
             tools.warm_model, tools.report, tools.install_token_goat, tools.pick_bin, sys.argv)
    tools.install_semble_and_ast_grep = tools.install_graphify = tools.make_front_end = lambda *a, **k: None
    tools.install_codanna = lambda *a, **k: None
    tools.warm_model = lambda *a, **k: "ready"
    tools.report = lambda *a, **k: False            # an index tool is missing
    tools.pick_bin = lambda *a, **k: pathlib.Path(".")
    seen = []
    tools.install_token_goat = lambda home, wanted, offline: (seen.append(wanted), goat_result)[1]
    sys.argv = ["tools.py", *argv]
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            return tools.main(), seen
    finally:
        (tools.install_semble_and_ast_grep, tools.install_graphify, tools.install_codanna, tools.make_front_end,
         tools.warm_model, tools.report, tools.install_token_goat, tools.pick_bin, sys.argv) = saved


code, seen = exit_of(False, ["--offline", "--token-goat", "claude"])
check("--offline and a failed token-goat step: exit 1", code == 1 and seen == ["claude"], str((code, seen)))
code, seen = exit_of(True, ["--offline", "--token-goat"])
check("--offline, a missing index tool, token-goat fine: exit 0, asked for all", code == 0 and seen == ["all"], str((code, seen)))
code, seen = exit_of(True, ["--offline"])
check("no --token-goat: the step is not called", code == 0 and seen == [], str((code, seen)))
code, seen = exit_of(False, ["--offline", "--token-goat", ""])
check("an empty client list reaches the step, which fails it", code == 1 and seen == [""], str((code, seen)))
code, seen = exit_of(True, [])
check("without --offline a missing index tool is still exit 1", code == 1, str(code))

print(f"{len(failed)} failed" if failed else "all passed")
sys.exit(1 if failed else 0)
