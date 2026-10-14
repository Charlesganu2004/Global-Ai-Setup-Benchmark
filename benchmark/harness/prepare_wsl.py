"""Prepare the virtual machine for the arms R6V, THV and TPHV: the tools run inside WSL 2.

    prepare_wsl.py --pgr PATH --fixture DIR --linux-pgr FILE [--distro NAME] [--vm-dir DIR] [--home DIR]
                   [--packs FILE] [--out FILE] [--wsl wsl.exe]

No agent program is installed inside the VM, so the agent stays in the harness and everything it runs happens
inside the VM: a copy of the fixture in the Linux file system, the Linux build of pgr, the index of the copy and
the hub banks. This script makes the first three, with one call of wsl.exe -d DISTRO --exec ... for each step:

  1. copies the fixture into the Linux file system (cp -r from its /mnt path, found with wslpath), into
     VM_DIR/repo, without the folders of other tools that prepare_team.py leaves out too. VM_DIR defaults to
     lxbench in the home folder of the VM user. An earlier copy of this script's own, at exactly VM_DIR/repo,
     is removed first, so that the script is safe to run twice. It is this script's own only when the marker
     file VM_DIR/repo.round6-wsl stands beside it; a VM_DIR/repo without the marker is refused and left alone;
  2. installs the Linux pgr (a file for Linux, --linux-pgr, built by the engine's builder) as VM_DIR/bin/pgr and
     checks that it says the same version as the pgr for Windows;
  3. builds the index there (pgr --root VM_DIR/repo refresh --force) and checks that the status line counts are
     the Windows index of the fixture: the same symbols, links, chunks and the same kind of search (a VM that
     does not find the embedding model would print "words only" where Windows prints "embeddings");
  4. computes the packs for sets A and B with the Linux build (variant gs, the one TPHV uses) and asserts that each
     is byte for byte the pack in packs.json. The same pack text is then used for both. If one differs this
     stops, prints the first difference of each, and writes nothing;
  5. writes benchmark/data/wsl.json: what the VM is, the layout with placeholder paths (the home folder is
     written as /home/USER), the SHA-256 of the installed pgr, the status line and the result of the comparison.

It never changes anything outside VM_DIR in the VM, and it removes only VM_DIR/repo, only when VM_DIR is a folder
inside the home folder of the VM user and only when the marker says that this script made VM_DIR/repo. Every call goes through wsl.exe, from Python, as one program with its
arguments (--exec), so that no shell in the VM sees a question text. Run it with the same pgr for Windows that made
packs.json; round6_args.py then asks the VM, read-only, before it writes the args of a half with VM arms.
"""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import sys

sys.path.append(str(pathlib.Path(__file__).resolve().parent))  # python -I leaves the script's folder off the path; the standard library stays first
import round6_common as common
from round6_common import RoundSixError

# Folders of other tools that the copy for the team arms leaves behind too (prepare_team.copy_fixture).
SKIP = {"__pycache__", ".codanna", ".token-goat", "graphify-out", "runs-hub"}
# The file that says, beside VM_DIR/repo, that this script made the copy (prepare_team.py does the same on Windows).
MARKER_SUFFIX = ".round6-wsl"


def first_difference(windows: str, linux: str) -> str:
    for number, (a, b) in enumerate(zip(windows.split("\n"), linux.split("\n")), 1):
        if a != b:
            return f"line {number}: Windows {a.strip()[:70]!r}, Linux {b.strip()[:70]!r}"
    return f"the lengths differ (Windows {len(windows)}, Linux {len(linux)})"


def counts(status: str) -> str:
    """What a status line says about the index, without the name of the folder and the age."""
    return common.stable_status(status).partition(" | ")[2]


def os_name(wsl: common.Wsl) -> str:
    found = re.search(r'^PRETTY_NAME="?([^"\n]*)"?$', wsl.run("cat", "/etc/os-release")[0], re.M)
    return found.group(1) if found else ""


def copy_into_vm(wsl: common.Wsl, fixture: pathlib.Path, vm_dir: str, home: str) -> str:
    """Copy the fixture to VM_DIR/repo and return that path. Only a folder inside the user's home is used."""
    if not common.LINUX_PATH_RULE.fullmatch(vm_dir) or ".." in vm_dir or not vm_dir.startswith(home + "/"):
        raise RoundSixError(f"--vm-dir {vm_dir!r} has to be a folder inside the home folder of the VM user ({home}), letters, digits and . _ - / only")
    repo = f"{vm_dir}/repo"
    marker = f"{repo}{MARKER_SUFFIX}"
    source = wsl.linux_path(fixture.resolve())
    if wsl.run("test", "-e", repo, check=False)[2] == 0:
        # Only a copy this script made is removed; the marker is written before the copy, so a copy cut off halfway can be replaced too.
        if wsl.run("test", "-f", marker, check=False)[2] != 0:
            raise RoundSixError(f"{repo} exists in the VM and was not made by this script (no {marker} beside it): move or remove it yourself, or pass another --vm-dir")
        wsl.run("rm", "-rf", repo)
    wsl.run("mkdir", "-p", repo)
    wsl.run("touch", marker)
    for entry in sorted(os.scandir(fixture), key=lambda item: item.name):
        if entry.name not in SKIP:
            wsl.run("cp", "-r", f"{source}/{entry.name}", f"{repo}/")
    return repo


def prepare(wsl: common.Wsl, pgr: common.Pgr, fixture: pathlib.Path, linux_pgr: pathlib.Path, vm_dir: str | None,
            questions: dict, packs_doc: dict) -> dict:
    """The wsl.json document, or an error that says what differs between the VM and Windows."""
    version = pgr.require()
    if packs_doc.get("pgr_version") != version:
        raise RoundSixError(f"packs.json was made by {packs_doc.get('pgr_version')} and the pgr for Windows is {version}: run prepare_packs.py with this pgr first")
    problems = common.check_packs(packs_doc, questions)
    if problems:
        raise RoundSixError("packs.json does not check out: " + "; ".join(problems[:5]))
    windows_status = pgr.run("--root", fixture, "status")[0].strip()
    if not common.index_ready(windows_status):
        raise RoundSixError(f"the fixture has no index that {version} can read: {windows_status!r}. Run: pgr --root {fixture} refresh")
    binary = linux_pgr.read_bytes()
    if binary[:4] != b"\x7fELF":
        raise RoundSixError(f"--linux-pgr {linux_pgr} is not a Linux program (it does not start with the ELF marker): build pgr for Linux and pass that file")

    home = wsl.home()
    vm_dir = vm_dir or f"{home}/lxbench"
    kernel, machine = wsl.run("uname", "-sr")[0].strip(), wsl.run("uname", "-m")[0].strip()
    repo = copy_into_vm(wsl, fixture, vm_dir, home)
    bin_dir, vm_pgr = f"{vm_dir}/bin", f"{vm_dir}/bin/pgr"
    wsl.run("mkdir", "-p", bin_dir)
    wsl.run("cp", wsl.linux_path(linux_pgr.resolve()), vm_pgr)
    wsl.run("chmod", "755", vm_pgr)
    seen = wsl.run(vm_pgr, "--version")[0].strip()
    if seen != version:
        raise RoundSixError(f"the Linux pgr says {seen!r} and the pgr for Windows says {version!r}: the two builds have to be one version")

    status = wsl.run(vm_pgr, "--root", repo, "refresh", "--force", cd=repo)[0].strip()
    if not common.index_ready(status):
        raise RoundSixError(f"the Linux pgr cannot read the index it built: {status!r}")
    if counts(status) != counts(windows_status):
        hint = (" The VM did not find the embedding model that Windows uses: put it where pgr looks for it inside the VM and run this again."
                if common.words_only(status) and not common.words_only(windows_status) else "")
        raise RoundSixError(f"the index in the VM is not the index of the fixture on Windows.\n  Windows: {windows_status}\n  Linux:   {status}.{hint}")

    budget = packs_doc.get("budget", common.PACK_BUDGET)
    differences, checked = [], {}
    for variant in common.WSL_PACK_VARIANTS:
        for letter in common.SETS:
            for key in common.KEYS:
                want = packs_doc["packs"][variant][letter][key]
                out = wsl.run(vm_pgr, "--root", repo, "--engines", common.VARIANTS[variant], "pack", want["question"], "--budget", budget,
                              cd=repo, env={"PGR_NO_REFRESH": "1"})[0]
                got = out.rstrip("\n")
                if got != want["output"]:
                    differences.append(f"{variant}/{letter}/{key}: {first_difference(want['output'], got)}")
                checked[f"{variant}/{letter}/{key}"] = common.sha256(got)
    if differences:
        raise RoundSixError(f"{len(differences)} of {len(checked)} packs made in the VM are not the Windows packs, so the two cannot share one pack text; nothing was written:\n  "
                            + "\n  ".join(differences[:5]))

    sums = wsl.run("sha256sum", vm_pgr)[0].split()
    if not sums or sums[0] != common.sha256_bytes(binary):
        raise RoundSixError("the pgr installed in the VM is not the file that was given (its SHA-256 differs)")
    layout = dict(common.WSL_PLACEHOLDER, distro=wsl.distro)
    doc = {
        "about": "The virtual machine of the arms R6V, THV and TPHV, made by prepare_wsl.py: the agent stays in the harness and everything it runs happens inside a WSL 2 distribution.",
        "pgr_version": version,
        "distro": wsl.distro,
        "os": os_name(wsl),
        "kernel": kernel,
        "machine": machine,
        "layout": layout,
        "commands": {
            "pgr": f"wsl -d {wsl.distro} --cd {layout['repo']} --exec {layout['pgr']}",
            "code": f"wsl -d {wsl.distro} --cd {layout['repo']} --exec sed -n \"FIRST,LASTp\" PATH",
        },
        "pgr_sha256": sums[0],
        "pgr_bytes": len(binary),
        "status": common.stable_status(status),
        "status_matches_windows": True,
        "packs": {
            "variants": list(common.WSL_PACK_VARIANTS),
            "checked": len(checked),
            "identical": True,
            "sha256": checked,
        },
    }
    text = json.dumps(doc, ensure_ascii=False)
    if home != common.WSL_HOME_PLACEHOLDER and home in text:
        raise RoundSixError(f"wsl.json would hold the home folder of the VM user ({home}); only /home/USER may appear")
    return doc


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--pgr", required=True, help="the pgr 0.2 program for Windows, or a command line for a stand-in")
    parser.add_argument("--fixture", required=True, type=pathlib.Path, help="the fixture repository whose copy goes into the VM")
    parser.add_argument("--linux-pgr", required=True, type=pathlib.Path, help="the Linux build of pgr (a file for Linux, same version)")
    parser.add_argument("--distro", default="Ubuntu", help="the WSL distribution (default: Ubuntu)")
    parser.add_argument("--vm-dir", help="the folder inside the VM for repo/ and bin/ (default: lxbench in the home folder of the VM user)")
    parser.add_argument("--home", help="LXR_HOME for the calls to the pgr for Windows (default: the one in the environment, else pgr's own)")
    parser.add_argument("--packs", type=pathlib.Path, default=common.DATA / "packs.json")
    parser.add_argument("--out", type=pathlib.Path, default=common.DATA / "wsl.json")
    parser.add_argument("--wsl", default="wsl.exe", help="wsl.exe, or a command line for a stand-in in the tests")
    parser.add_argument("--workflow", type=pathlib.Path, default=common.WORKFLOW, help="the workflow whose question texts are used")
    args = parser.parse_args(argv)
    try:
        if not args.packs.exists():
            raise RoundSixError(f"{args.packs} does not exist: run prepare_packs.py first")
        if not args.linux_pgr.is_file():
            raise RoundSixError(f"--linux-pgr {args.linux_pgr} is not a file")
        doc = prepare(common.Wsl(args.distro, args.wsl), common.Pgr(args.pgr, home=args.home or None), args.fixture, args.linux_pgr, args.vm_dir,
                      common.workflow_questions(args.workflow), json.loads(args.packs.read_text(encoding="utf-8")))
        common.write_json(args.out, doc, private=(args.fixture, args.linux_pgr, args.home))
    except RoundSixError as error:
        print(f"prepare_wsl: {error}", file=sys.stderr)
        return 1
    print(f"{doc['pgr_version']} in {doc['distro']} ({doc['os']}, {doc['kernel']}): the index and {doc['packs']['checked']} packs equal the Windows ones -> {args.out}")
    print(f"for round6_args.py: --wsl-distro {doc['distro']}" + (f" --wsl-dir {args.vm_dir}" if args.vm_dir else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
