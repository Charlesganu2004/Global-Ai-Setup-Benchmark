"""A stand-in for wsl.exe, for the tests of prepare_wsl.py and round6_args.py. It is not the product.

It answers `wsl.exe -d DISTRO [--cd DIR] --exec PROGRAM ARGS...`, the one form the harness uses, with a
folder on this machine as the file system of a Linux virtual machine:

  FAKE_WSL_ROOT     the folder that stands for / inside the VM (required)
  FAKE_WSL_DISTRO   the one distribution that exists (default Ubuntu); another name gives wsl.exe's own
                    error, written in UTF-16 as the real one is
  FAKE_WSL_LXR_HOME set: the VM has this LXR_HOME (a path inside the VM); unset: it has none
  FAKE_WSL_PGR_VERSION  the version the pgr inside the VM prints (default: the stand-in's own)

The programs it knows are the ones the scripts use: uname, printenv, cat, mkdir, touch, rm, cp, chmod, test, ls,
sha256sum, wslpath, sed, env and anything whose name is pgr, which runs fake_pgr.py with the paths mapped
(/mnt/c/... is C:/... on this machine, any other absolute path is under FAKE_WSL_ROOT). The home folder of
the VM is /home/fakeuser.
"""
from __future__ import annotations

import hashlib
import os
import pathlib
import shutil
import sys

HERE = pathlib.Path(__file__).resolve().parent
HOME = "/home/fakeuser"


def root() -> pathlib.Path:
    given = os.environ.get("FAKE_WSL_ROOT")
    if not given:
        print("fake_wsl: FAKE_WSL_ROOT is not set", file=sys.stderr)
        sys.exit(2)
    return pathlib.Path(given)


def host(path: str) -> pathlib.Path:
    """The path on this machine of a path inside the VM. /mnt/c/... is C:/... on Windows; on another system the
    machine's own folders appear under /mnt/host."""
    if path.startswith("/mnt/host/"):
        return pathlib.Path(path[len("/mnt/host"):])
    if path.startswith("/mnt/") and len(path) > 6 and path[6] == "/":
        return pathlib.Path(f"{path[5].upper()}:/{path[7:]}")
    if path.startswith("/"):
        return root() / path.lstrip("/")
    return pathlib.Path(path)


def to_linux(path: str) -> str:
    """wslpath -u: the path inside the VM of a path on this machine."""
    given = path.replace("\\", "/")
    if len(given) > 2 and given[1] == ":" and given[2] == "/":
        return f"/mnt/{given[0].lower()}/{given[3:]}"
    return "/mnt/host" + given if given.startswith("/") else given


def fail(message: str, code: int = 1) -> None:
    print(message, file=sys.stderr)
    sys.exit(code)


def vm_env() -> dict:
    env = dict(os.environ)
    env["HOME"] = HOME
    env["FAKE_PGR_STRIP"] = str(root().resolve())
    given = os.environ.get("FAKE_WSL_LXR_HOME")
    env["LXR_HOME"] = str(host(given)) if given else str(host(f"{HOME}/.local-index/rust"))
    if os.environ.get("FAKE_WSL_PGR_VERSION"):
        env["FAKE_PGR_VERSION"] = os.environ["FAKE_WSL_PGR_VERSION"]
    return env


def run(program: str, args: list[str], cwd: pathlib.Path | None, env: dict) -> int:
    name = pathlib.PurePosixPath(program).name
    out = sys.stdout
    if name == "uname":
        flag = args[0] if args else ""
        print({"-a": "Linux fakevm 5.15.0-fake #1 SMP x86_64 GNU/Linux", "-sr": "Linux 5.15.0-fake", "-m": "x86_64"}.get(flag, "Linux"))
        return 0
    if name == "printenv":
        value = HOME if args == ["HOME"] else os.environ.get("FAKE_WSL_LXR_HOME") if args == ["LXR_HOME"] else None
        if value is None:
            return 1
        print(value)
        return 0
    if name == "cat":
        target = host(args[0])
        if args[0] == "/etc/os-release" and not target.exists():
            out.write('PRETTY_NAME="Ubuntu 24.04 (stand-in)"\nVERSION_ID="24.04"\n')
            return 0
        out.write(target.read_text(encoding="utf-8"))
        return 0
    if name == "mkdir":
        for path in [a for a in args if not a.startswith("-")]:
            host(path).mkdir(parents="-p" in args, exist_ok=True)
        return 0
    if name == "touch":
        for path in args:
            host(path).touch()
        return 0
    if name == "rm":
        for path in [a for a in args if not a.startswith("-")]:
            target = host(path)
            shutil.rmtree(target, ignore_errors=True) if target.is_dir() else target.unlink(missing_ok=True)
        return 0
    if name == "cp":
        paths = [a for a in args if not a.startswith("-")]
        source, dest = host(paths[0]), host(paths[1])
        if source.is_dir():
            if "-r" not in args and "-R" not in args:
                fail(f"cp: -r not specified; omitting directory '{paths[0]}'")
            if dest.is_dir():
                dest = dest / source.name
            shutil.copytree(source, dest)
        else:
            shutil.copyfile(source, dest / source.name if dest.is_dir() else dest)
        return 0
    if name == "chmod":
        return 0 if host(args[-1]).exists() else 1
    if name == "test":
        target = host(args[1])
        return 0 if {"-x": target.is_file(), "-f": target.is_file(), "-d": target.is_dir(), "-e": target.exists()}[args[0]] else 1
    if name == "ls":
        target = host(args[-1])
        if not target.is_dir():
            fail(f"ls: cannot access '{args[-1]}': No such file or directory", 2)
        for child in sorted(target.iterdir()):
            print(child.name)
        return 0
    if name == "sha256sum":
        print(f"{hashlib.sha256(host(args[0]).read_bytes()).hexdigest()}  {args[0]}")
        return 0
    if name == "wslpath":
        print(to_linux(args[-1]))
        return 0
    if name == "sed":
        first, _, last = args[1].rstrip("p").partition(",")
        lines = host(args[2]).read_text(encoding="utf-8").split("\n")
        out.write("".join(line + "\n" for line in lines[int(first) - 1:int(last)]))
        return 0
    if name == "env":
        extra = dict(env)
        rest = list(args)
        while rest and "=" in rest[0]:
            key, _, value = rest.pop(0).partition("=")
            extra[key] = value
        return run(rest[0], rest[1:], cwd, extra)
    if name == "pgr":
        target = host(program)
        if not target.is_file():
            fail(f"fake_wsl: {program}: No such file or directory", 127)
        mapped = list(args)
        for at, word in enumerate(mapped[:-1]):
            if word == "--root":
                mapped[at + 1] = str(host(mapped[at + 1]))
        # The stand-in pgr runs in this process (a process less to start for each call); this process is the VM's.
        sys.path.append(str(HERE))
        import fake_pgr
        os.environ.clear()
        os.environ.update(env)
        if cwd:
            os.chdir(cwd)
        try:
            return fake_pgr.main(mapped)
        except SystemExit as stop:
            return stop.code if isinstance(stop.code, int) else 1
    fail(f"fake_wsl: {program}: command not found", 127)
    return 127


def main(argv: list[str]) -> int:
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    sys.stderr.reconfigure(encoding="utf-8", newline="\n")
    distro, cd, at = None, None, 0
    while at < len(argv) and argv[at] != "--exec":
        if argv[at] in ("-d", "--distribution"):
            distro, at = argv[at + 1], at + 2
        elif argv[at] == "--cd":
            cd, at = argv[at + 1], at + 2
        else:
            fail(f"fake_wsl: unknown option {argv[at]}", 2)
    if at >= len(argv):
        fail("fake_wsl: only --exec is supported", 2)
    program, args = argv[at + 1], argv[at + 2:]
    if distro != os.environ.get("FAKE_WSL_DISTRO", "Ubuntu"):
        sys.stderr.flush()
        sys.stderr.buffer.write("There is no distribution with the supplied name.\r\n".encode("utf-16-le"))
        return 1
    sys.stdout.flush()
    return run(program, args, host(cd) if cd else None, vm_env())


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
