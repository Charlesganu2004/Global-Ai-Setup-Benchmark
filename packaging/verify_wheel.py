"""Checks a built peterganurusty wheel before it is handed to anyone.

    python -I verify_wheel.py WHEEL [--model-dir DIR] [--forbid TEXT]...

The wheel holds the program, LICENSE.txt and THIRD-PARTY-NOTICES.txt, and nothing else but its
own metadata: the embedding model is compiled into the program.

Fails (exit 1) when:
  - the file name is not peterganurusty-VERSION-TAG.whl
  - the wheel holds a source file (.rs, Cargo.toml, Cargo.lock, .git), a .py file or an sdist-like member
  - the wheel holds a model data file, or any data file besides the two above
  - the pgr program is missing from the scripts folder, is not a stripped executable, or contains
    one of the --forbid texts (use it for the builder's home folder and the repository path)
  - the program does not contain model.safetensors and tokenizer.json of --model-dir byte for byte,
    or those files differ from the SHA-256 values pinned in stage.py
  - the licence file, the third-party notices (which must name no LGPL crate) or the licence
    classifier are missing

It prints one line per check.
"""

import argparse
import hashlib
import importlib.util
import struct
import sys
import zipfile
from pathlib import Path

SOURCE_SUFFIXES = (".rs", ".py", ".pyi", ".c", ".h", ".cpp")
SOURCE_NAMES = ("Cargo.toml", "Cargo.lock", ".gitignore", "pyproject.toml")
SHARE = "share/peterganurusty/"
# The only data files of the wheel, under SHARE.
DATA_FILES = ("LICENSE.txt", "THIRD-PARTY-NOTICES.txt")


def load_stage():
    path = Path(__file__).resolve().parent / "stage.py"
    spec = importlib.util.spec_from_file_location("stage", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def pe_is_stripped(data):
    """True for a PE file with no COFF symbol table and no DWARF (.debug_*, or /N) sections."""
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("not a PE file")
    _machine, sections, _stamp, symbols_at, symbols, optional, _flags = struct.unpack_from("<HHIIIHH", data, pe + 4)
    table = pe + 24 + optional
    names = [data[table + 40 * i:table + 40 * i + 8].rstrip(b"\0") for i in range(sections)]
    debug = [n for n in names if n.startswith(b".debug") or n.startswith(b"/")]
    return symbols_at == 0 and symbols == 0 and not debug, (symbols, debug)


def elf_is_stripped(data):
    """True for a 64-bit little-endian ELF file with no .symtab and no .debug_* section.

    The section names are read from the section header table. Searching the whole file for the
    text ".symtab" does not work: the Rust standard library carries those names as literals for
    its own backtraces, so every Rust program contains them, stripped or not.
    """
    if data[4] != 2 or data[5] != 1:
        raise ValueError("not a 64-bit little-endian ELF file")
    table = struct.unpack_from("<Q", data, 0x28)[0]
    entry, count, names_at = struct.unpack_from("<HHH", data, 0x3A)
    if not table or not count or names_at >= count:
        raise ValueError("no section header table")

    def section(index):
        return struct.unpack_from("<IIQQQQ", data, table + entry * index)

    _name, _kind, _flags, _addr, strings, strings_size = section(names_at)
    names = []
    for index in range(count):
        start = strings + section(index)[0]
        if start >= strings + strings_size:
            raise ValueError("a section name lies outside the name table")
        names.append(data[start:data.index(b"\0", start)].decode("ascii", "replace"))
    found = [n for n in names if n == ".symtab" or n.startswith(".debug")]
    return not found, found


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("wheel")
    parser.add_argument("--model-dir", default=str(Path(__file__).resolve().parent / "stage" / "model"), help="the folder the program was built with (PGR_EMBED_MODEL_DIR)")
    parser.add_argument("--forbid", action="append", default=[], help="text that must not occur in the binary")
    args = parser.parse_args()

    wheel = Path(args.wheel)
    stage = load_stage()
    failures = []

    def report(ok, text):
        print(("ok    " if ok else "FAIL  ") + text)
        if not ok:
            failures.append(text)

    parts = wheel.name[:-4].split("-") if wheel.name.endswith(".whl") else []
    report(len(parts) == 5 and parts[0] == "peterganurusty", f"file name {wheel.name} is peterganurusty-VERSION-PYTHON-ABI-PLATFORM.whl")
    version = parts[1] if len(parts) == 5 else "?"

    with zipfile.ZipFile(wheel) as zf:
        names = zf.namelist()
        report(zf.testzip() is None, f"zip integrity ({len(names)} members)")
        source = [n for n in names if n.endswith(SOURCE_SUFFIXES) or Path(n).name in SOURCE_NAMES or "/.git" in n or n.startswith(".git")]
        report(not source, "no source files in the wheel" + (f": {source}" if source else ""))
        text_members = [n for n in names if n.endswith((".json", ".txt", ".md", "METADATA", "RECORD")) and not n.endswith("tokenizer.json")]
        for member in text_members:
            leaked = [t for t in args.forbid if t.encode() in zf.read(member) or t.replace("\\", "/").encode() in zf.read(member)]
            report(not leaked, f"{member} does not contain {leaked or 'the forbidden texts'}")

        data_prefix = f"peterganurusty-{version}.data/"
        scripts = [n for n in names if n.startswith(data_prefix + "scripts/")]
        report(len(scripts) == 1 and Path(scripts[0]).stem == "pgr", f"one script, pgr: {scripts}")
        if scripts:
            program = zf.read(scripts[0])
            if program[:2] == b"MZ":
                try:
                    stripped, (symbols, debug) = pe_is_stripped(program)
                    report(stripped, f"pgr is a stripped PE file (COFF symbols {symbols}, debug sections {debug})")
                except (ValueError, struct.error) as error:
                    report(False, f"pgr PE header unreadable: {error}")
            elif program[:4] == b"\x7fELF":
                try:
                    stripped, found = elf_is_stripped(program)
                    report(stripped, f"pgr is a stripped ELF file (symbol and debug sections: {found})")
                except (ValueError, struct.error) as error:
                    report(False, f"pgr is a readable ELF file ({error})")
            else:
                print("note  pgr is neither PE nor ELF: the strip check was not run")
            for text in args.forbid:
                found = any(variant.encode() in program for variant in {text, text.replace("\\", "/"), text.replace("/", "\\")})
                report(not found, f"pgr does not contain {text!r}")
            print(f"info  pgr is {len(program)} bytes")

        # The model is inside the program: both files, byte for byte, and the pinned ones.
        model_dir = Path(args.model_dir)
        if scripts:
            for name, (expected, size) in stage.MODEL_FILES.items():
                path = model_dir / name
                if not path.is_file():
                    report(False, f"{path} exists, so that the program can be compared with it")
                    continue
                blob = path.read_bytes()
                report(len(blob) == size and hashlib.sha256(blob).hexdigest() == expected, f"{name} in {model_dir} matches the pinned SHA-256")
                at = program.find(blob[:1 << 20])
                report(at >= 0 and program[at:at + len(blob)] == blob, f"pgr contains {name} byte for byte" + (f" (at offset {at})" if at >= 0 else ""))

        data_members = sorted(n for n in names if n.startswith(data_prefix + "data/"))
        wanted = sorted(f"{data_prefix}data/{SHARE}{name}" for name in DATA_FILES)
        report(data_members == wanted, f"the data files are exactly {list(DATA_FILES)} (found {[m[len(data_prefix):] for m in data_members]})")
        report(not any("/model" in n or n.endswith((".safetensors", "tokenizer.json")) for n in names), "the wheel holds no model file")
        notices = zf.read(f"{data_prefix}data/{SHARE}THIRD-PARTY-NOTICES.txt").decode("utf-8") if f"{data_prefix}data/{SHARE}THIRD-PARTY-NOTICES.txt" in names else ""
        report("LGPL" not in notices.upper(), "THIRD-PARTY-NOTICES.txt names no LGPL crate")
        report("potion-code-16M-v2" in notices and "MIT License" in notices, "THIRD-PARTY-NOTICES.txt carries the licence notice of the model")
        dist_info = f"peterganurusty-{version}.dist-info/"
        metadata = zf.read(dist_info + "METADATA").decode("utf-8") if dist_info + "METADATA" in names else ""
        report("Name: peterganurusty" in metadata and f"Version: {version}" in metadata, "METADATA names peterganurusty and the version")
        report("Classifier: License :: Other/Proprietary License" in metadata, "METADATA has the Other/Proprietary License classifier")
        report("Requires-Dist" not in metadata, "METADATA has no dependencies")

    print(f"{'FAILED' if failures else 'PASSED'}: {len(failures)} failing checks")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
