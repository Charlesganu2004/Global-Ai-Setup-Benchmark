"""Writes THIRD-PARTY-NOTICES.txt for the pgr binary.

The file lists every crate that is linked into pgr (crate, version, licence expression,
repository), the embedding model that is compiled into it, and the licence texts that the
crates carry in their packages. "Linked into pgr" means the normal dependencies of the
`lxr` package, followed transitively through normal dependencies only: dev and
build-only dependencies are left out. The crates of this workspace are the program
itself and are left out too.

    python -I notices.py --manifest ../rust/Cargo.toml --out stage/THIRD-PARTY-NOTICES.txt

The crates that are not under one of the common permissive licences are printed to
standard error and in the summary on standard output.
"""

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

PROGRAM = "PeterGanuRusty (pgr)"
ROOT_PACKAGE = "lxr"

MODEL_NAME = "potion-code-16M-v2"
MODEL_URL = "https://huggingface.co/minishlab/potion-code-16M-v2"
MODEL_REVISION = "e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b"
MODEL_SOFTWARE_URL = "https://github.com/MinishLab/model2vec"

# The licence file of the Model2Vec project, as published at
# https://raw.githubusercontent.com/MinishLab/model2vec/main/LICENSE
MODEL2VEC_LICENSE = """MIT License

Copyright (c) 2024 Thomas van Dongen

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"""

MODEL_NOTICE = f"""{MODEL_NAME}, a static embedding model (about 32 MB), compiled into the program
Source:   {MODEL_URL}, revision {MODEL_REVISION}
Licence:  MIT. The model card (README.md) of that revision declares "license: mit".
          It names Stephan Tulkens and Thomas van Dongen as authors of the Model2Vec
          software that the model is built with, and it carries no copyright line of its own.
          The licence text below is the licence file of that software, {MODEL_SOFTWARE_URL},
          with the copyright line as it is published there.
Origin:   per the model card, distilled from nomic-ai/CodeRankEmbed and trained on the
          CornStack code corpus with Tokenlearn. The terms of those upstream works are not
          examined or restated here.

{MODEL2VEC_LICENSE}"""

# Licence identifiers that count as common and permissive.
PERMISSIVE_IDS = {"MIT", "Apache-2.0", "ISC", "Zlib", "MPL-2.0", "CC0-1.0", "Unlicense", "0BSD"}
PERMISSIVE_PREFIXES = ("BSD-", "Unicode-")

LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|unlicense|notice)([-_.].*)?$", re.IGNORECASE)
MAX_LICENSE_FILE_BYTES = 200_000


def is_permissive_id(identifier):
    identifier = identifier.rstrip("+")
    return identifier in PERMISSIVE_IDS or identifier.startswith(PERMISSIVE_PREFIXES)


def is_permissive(expression):
    """True when the SPDX expression can be satisfied with permissive licences only.

    `A OR B` needs one side, `A AND B` needs both, `A WITH exception` is judged by `A`, and
    the old `A/B` form of Cargo manifests means `A OR B`. An expression that cannot be
    read is not permissive.
    """
    tokens = re.findall(r"\(|\)|[^\s()]+", expression.replace("/", " OR "))
    position = 0

    def peek():
        return tokens[position].upper() if position < len(tokens) else None

    def take():
        nonlocal position
        position += 1
        return tokens[position - 1]

    def parse_or():
        result = parse_and()
        while peek() == "OR":
            take()
            right = parse_and()
            result = result or right
        return result

    def parse_and():
        result = parse_term()
        while peek() == "AND":
            take()
            right = parse_term()
            result = result and right
        return result

    def parse_term():
        if peek() is None:
            raise ValueError("unexpected end")
        if peek() == "(":
            take()
            result = parse_or()
            if peek() != ")":
                raise ValueError("missing )")
            take()
            return result
        result = is_permissive_id(take())
        if peek() == "WITH":
            take()
            take()
        return result

    try:
        result = parse_or()
    except (ValueError, IndexError):
        return False
    return result and position == len(tokens)


def cargo_metadata(manifest, cargo):
    done = subprocess.run(
        [cargo, "metadata", "--format-version", "1", "--locked", "--manifest-path", str(manifest)],
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if done.returncode != 0:
        sys.exit(f"cargo metadata failed (exit {done.returncode}):\n{done.stderr}")
    return json.loads(done.stdout)


def linked_packages(meta):
    """The third-party packages linked into pgr, and the number of workspace crates left out."""
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    members = set(meta["workspace_members"])
    roots = [i for i in members if packages[i]["name"] == ROOT_PACKAGE]
    if len(roots) != 1:
        sys.exit(f"expected one workspace package named {ROOT_PACKAGE}, found {len(roots)}")
    seen = {roots[0]}
    stack = [roots[0]]
    while stack:
        for dep in nodes[stack.pop()]["deps"]:
            # kind is None for a normal dependency, "dev" or "build" otherwise.
            if dep["pkg"] not in seen and any(k["kind"] is None for k in dep["dep_kinds"]):
                seen.add(dep["pkg"])
                stack.append(dep["pkg"])
    third_party = [packages[i] for i in seen if i not in members]
    third_party.sort(key=lambda p: (p["name"], p["version"]))
    return third_party, len(seen & members)


def license_texts(package):
    """(file name, text) for each licence file at the top of the package's folder."""
    found = []
    folder = Path(package["manifest_path"]).parent
    for path in sorted(folder.iterdir()):
        if path.is_file() and LICENSE_FILE.match(path.name) and path.stat().st_size <= MAX_LICENSE_FILE_BYTES:
            found.append((path.name, path.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip()))
    return found


def build_notices(packages, workspace_crates):
    unusual = []
    without_text = []
    lines = []
    texts = {}  # sha256 of the text -> [text, [crate labels]]
    for package in packages:
        label = f"{package['name']} {package['version']}"
        expression = package.get("license")
        if expression is None and package.get("license_file"):
            expression = f"(see {package['license_file']})"
        if expression is None:
            expression = "(none declared)"
        if not is_permissive(expression):
            unusual.append((label, expression))
        repository = package.get("repository") or package.get("homepage") or "(none listed)"
        lines.append(f"{label} | {expression} | {repository}")
        files = license_texts(package)
        if not files:
            without_text.append(label)
        for _name, text in files:
            entry = texts.setdefault(hashlib.sha256(text.encode("utf-8")).hexdigest(), [text, []])
            entry[1].append(label)

    out = [
        f"THIRD-PARTY NOTICES for {PROGRAM}",
        "",
        "The pgr program contains the open-source crates listed in part 1 and carries the",
        "embedding model of part 2 inside it. Each line of part 1 reads: crate version | licence | repository.",
        f"The {workspace_crates} crates of the program itself are not listed. Dev and build-only",
        "dependencies are not listed because they are not part of the binary. Part 3 holds the",
        "licence texts that the crates carry in their own packages, each once.",
        "",
        f"PART 1: {len(packages)} CRATES LINKED INTO pgr",
        "",
        *lines,
        "",
        "PART 2: EMBEDDING MODEL",
        "",
        MODEL_NOTICE.rstrip(),
        "",
        f"PART 3: LICENCE TEXTS OF THE CRATES ({len(texts)} distinct texts)",
    ]
    for number, (text, labels) in enumerate(texts.values(), start=1):
        out += ["", "=" * 78, f"Text {number}, found in: {', '.join(labels)}", "=" * 78, "", text]
    if without_text:
        out += ["", "=" * 78, "Crates whose package holds no licence file (their licence is the expression in part 1):", ", ".join(without_text)]
    return "\n".join(out) + "\n", unusual, without_text


def main():
    here = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--manifest", default=str(here.parent / "rust" / "Cargo.toml"), help="the workspace Cargo.toml")
    parser.add_argument("--out", default=str(here / "stage" / "THIRD-PARTY-NOTICES.txt"))
    parser.add_argument("--model-notice-out", help="also write the model's licence notice to this file (it is part 2 of the notices as well)")
    parser.add_argument("--cargo", default="cargo", help="the cargo command (default: cargo on the search path)")
    args = parser.parse_args()

    packages, workspace_crates = linked_packages(cargo_metadata(args.manifest, args.cargo))
    text, unusual, without_text = build_notices(packages, workspace_crates)
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(text, encoding="utf-8", newline="\n")
    if args.model_notice_out:
        Path(args.model_notice_out).write_text(MODEL_NOTICE, encoding="utf-8", newline="\n")

    print(f"wrote {out} ({out.stat().st_size} bytes)")
    print(f"third-party crates linked into pgr: {len(packages)} (workspace crates left out: {workspace_crates})")
    print(f"crates with no licence file in their package: {len(without_text)}")
    print(f"crates outside the common permissive licences: {len(unusual)}")
    for label, expression in unusual:
        print(f"  UNUSUAL {label}: {expression}")
        print(f"UNUSUAL LICENCE {label}: {expression}", file=sys.stderr)


if __name__ == "__main__":
    main()
