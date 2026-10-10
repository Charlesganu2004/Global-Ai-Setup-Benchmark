import argparse
import pathlib
import subprocess
import sys
import tempfile

FILE_MARK, LINE_MARK, END_MARK = "#@@FILE ", "#|", "#@@END"


def unpack(destination: pathlib.Path) -> int:
    """Write out every file packed below, exactly as it was packed."""
    name, lines, count = "", [], 0

    def flush() -> int:
        if not name:
            return 0
        relative = pathlib.PurePosixPath(name)
        if relative.is_absolute() or ".." in relative.parts:
            raise SystemExit(f"REFUSED    packed path leaves the folder: {name}")
        target = destination.joinpath(*relative.parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(("\n".join(lines) + "\n").encode("utf-8"))
        return 1

    source = pathlib.Path(__file__).read_bytes().decode("utf-8-sig")
    for raw in source.split("\n"):
        line = raw.rstrip("\r")
        if line.startswith(FILE_MARK):
            count += flush()
            name, lines = line[len(FILE_MARK):].strip(), []
        elif line.startswith(END_MARK):
            count += flush()
            name = ""
        elif name and line.startswith(LINE_MARK):
            lines.append(line[len(LINE_MARK):])
    return count


def step(script: pathlib.Path, *flags: str) -> int:
    return subprocess.run([sys.executable, str(script), *flags]).returncode


def main() -> int:
    parser = argparse.ArgumentParser(description="Global AI setup, one-file installer.")
    parser.add_argument("--client", default="all",
                        choices=("all", "claude", "codex", "copilot", "gpt"))
    parser.add_argument("--no-tools", action="store_true",
                        help="skip the package and model downloads")
    parser.add_argument("--dry-run", action="store_true",
                        help="show what would change and write nothing")
    parser.add_argument("--token-goat", action="store_true",
                        help="optional: also install token-goat and its hooks "
                             "(npm package, noncommercial licence)")
    parser.add_argument("--dir", type=pathlib.Path,
                        default=pathlib.Path.home() / "global-ai-setup")
    parser.add_argument("--home", type=pathlib.Path, default=None, help=argparse.SUPPRESS)
    parser.add_argument("--bin", default="", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if sys.version_info < (3, 10):
        raise SystemExit("FAILED     Python 3.10 or newer is needed")
    extra = ["--home", str(args.home)] if args.home else []
    # The optional step goes to every client folder that exists before apply.py
    # runs, which makes all three: on a first run that is the clients the machine
    # really has, on a later run all three unless --client names one.
    home = args.home or pathlib.Path.home()
    goat = {"gpt": "codex"}.get(args.client, args.client)
    if goat == "all":
        goat = ",".join(name for name in ("claude", "codex", "copilot") if (home / f".{name}").is_dir())

    if args.dry_run:
        with tempfile.TemporaryDirectory() as scratch:
            unpack(pathlib.Path(scratch))
            status = step(pathlib.Path(scratch) / "apply.py", "--client", args.client,
                          "--dry-run", *extra)
        if args.token_goat and goat:
            print(f"would run  the token-goat step for {goat}: npm install of the pinned token-goat when the "
                  "command is missing, then token-goat's own installer for each client (hooks, a rules-file "
                  "block, a skill, Codex trust entries, a Copilot MCP entry)")
        elif args.token_goat:
            print("skipped    token-goat: no client folder exists; name one with --client")
        return status

    print(f"unpacked   {unpack(args.dir)} files to {args.dir}", flush=True)
    failed = step(args.dir / "apply.py", "--client", args.client, *extra)
    flags = ["--offline"] if args.no_tools else []
    if args.bin:
        flags += ["--bin", args.bin]
    if args.token_goat and goat:
        flags += ["--token-goat", goat]
    elif args.token_goat:
        print("skipped    token-goat: no client folder exists; name one with --client", flush=True)
    if args.no_tools:
        print("skipped    tool downloads (--no-tools); lx is still set up", flush=True)
    failed = step(args.dir / "tools.py", *flags, *extra) or failed
    print("done       restart each client; in Codex, trust the new hooks under /hooks"
          if not failed else "ATTENTION  see the FAILED, MISSING, REFUSED or ACTION lines above")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
