#!/usr/bin/env python3
"""Pack this folder into INSTALL.txt and render each client's rules for reading.

    python build_install.py

INSTALL.txt is one file that is both the instructions an agent reads and the
program that carries them out: `python INSTALL.txt`. Its header is plain text
inside a Python docstring; below it sits a small unpacker, then every file in
PACKED as comment lines, each prefixed with `#|`. Nothing is encoded, so the
whole thing can be read and reviewed before it is run.

Run this again after changing anything it packs.
"""
from __future__ import annotations

import pathlib
import sys
import tempfile

import apply

HERE = pathlib.Path(__file__).resolve().parent
PACKED = (
    "README.md",
    "apply.py",
    "tools.py",
    "local-index/session_index.py",
    "local-index/config.json",
    "rules/protected-rules.template.md",
    "rules/cavecrew.claude.md",
    "rules/cavecrew.codex.md",
    "rules/cavecrew.copilot.md",
    "rules/no-compress.md",
    "agents/cavecrew-investigator.md",
    "agents/cavecrew-builder.md",
    "agents/cavecrew-reviewer.md",
) + tuple(sorted(path.relative_to(HERE).as_posix() for path in (HERE / "skills").glob("*/SKILL.md")))
RENDERED = {"claude": "CLAUDE.md", "codex": "AGENTS.md", "copilot": "copilot-instructions.md"}


def text_of(relative: str) -> str:
    return (HERE / relative).read_bytes().decode("utf-8-sig").replace("\r\n", "\n")


def build() -> str:
    header = text_of("install/header.txt").strip("\n")
    if '"""' in header:
        raise SystemExit('install/header.txt may not contain """')
    parts = ['r"""', header, '"""', text_of("install/bootstrap.py").strip("\n"), "", ""]
    for relative in PACKED:
        body = text_of(relative)
        if not body.endswith("\n"):
            raise SystemExit(f"{relative} must end with a newline")
        parts.append(f"#@@FILE {relative}")
        parts += [f"#|{line}" for line in body[:-1].split("\n")]
    parts.append("#@@END")
    return "\n".join(parts) + "\n"


def round_trip(bundle: pathlib.Path) -> None:
    """Unpack the bundle somewhere else and insist every file comes back identical."""
    namespace = {"__file__": str(bundle), "__name__": "bundle"}
    exec(compile(bundle.read_text(encoding="utf-8"), str(bundle), "exec"), namespace)
    with tempfile.TemporaryDirectory() as scratch:
        count = namespace["unpack"](pathlib.Path(scratch))
        for relative in PACKED:
            if (pathlib.Path(scratch) / relative).read_bytes() != text_of(relative).encode("utf-8"):
                raise SystemExit(f"round trip changed {relative}")
    print(f"packed     {count} files, round trip identical")


def main() -> int:
    bundle = HERE / "INSTALL.txt"
    bundle.write_bytes(build().encode("utf-8"))
    round_trip(bundle)
    print(f"wrote      {bundle} ({bundle.stat().st_size} bytes)")

    rendered = HERE / "rendered"
    rendered.mkdir(exist_ok=True)
    for client, name in RENDERED.items():
        text = apply.rendered_rules(client) + "\n\n" + apply.source("rules", "no-compress.md") + "\n"
        (rendered / name).write_bytes(text.encode("utf-8"))
        print(f"rendered   {rendered / name} ({len(text)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
