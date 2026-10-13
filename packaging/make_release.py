"""Turns the wheels of one build into the files of a GitHub release.

Usage: python -I packaging/make_release.py WHEEL_DIR OUT_DIR

WHEEL_DIR is searched for peterganurusty-*.whl (the artifacts of one run of the wheels
workflow, downloaded with `gh run download RUN --dir WHEEL_DIR`). OUT_DIR receives:

  the wheels themselves                         for pip, pipx and uv
  pgr-windows-x86_64.exe, pgr-linux-x86_64,     the pgr program taken out of each wheel, for the
  pgr-linux-aarch64, pgr-macos-x86_64,          installers: no Python needed. The names carry no
  pgr-macos-aarch64                             version so that releases/latest/download/NAME works.
  install.ps1, install.sh                       copies of packaging/install/
  SHA256SUMS.txt                                one line per file above: "<sha256>  <name>"

Nothing is uploaded here. All wheels must be of one version, and each platform may appear once.
"""
import hashlib
import pathlib
import shutil
import sys
import zipfile

PLATFORMS = (
    # (what the wheel's platform tag contains, what it ends with, the program's release name)
    ("win", "amd64", "pgr-windows-x86_64.exe"),
    ("manylinux", "x86_64", "pgr-linux-x86_64"),
    ("manylinux", "aarch64", "pgr-linux-aarch64"),
    ("macosx", "x86_64", "pgr-macos-x86_64"),
    ("macosx", "arm64", "pgr-macos-aarch64"),
)
INSTALLERS = ("install.ps1", "install.sh")


def release_name(wheel):
    """The release name of the program inside a wheel, from the wheel's platform tag."""
    tag = wheel.stem.split("-")[-1]
    for contains, ends, name in PLATFORMS:
        if contains in tag and tag.endswith(ends):
            return name
    raise SystemExit(f"{wheel.name}: no release name for the platform tag {tag}")


def program_of(wheel):
    """The bytes of the pgr program a wheel installs."""
    with zipfile.ZipFile(wheel) as archive:
        entries = [n for n in archive.namelist() if n.endswith((".data/scripts/pgr", ".data/scripts/pgr.exe"))]
        if len(entries) != 1:
            raise SystemExit(f"{wheel.name}: expected one program under .data/scripts, found {entries}")
        return archive.read(entries[0])


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def main(argv):
    if len(argv) != 3:
        raise SystemExit(__doc__)
    wheel_dir, out = pathlib.Path(argv[1]), pathlib.Path(argv[2])
    wheels = sorted(wheel_dir.rglob("peterganurusty-*.whl"))
    if not wheels:
        raise SystemExit(f"no peterganurusty wheel under {wheel_dir}")
    versions = {w.name.split("-")[1] for w in wheels}
    if len(versions) != 1:
        raise SystemExit(f"wheels of more than one version: {sorted(versions)}")
    out.mkdir(parents=True, exist_ok=True)
    if any(out.iterdir()):
        raise SystemExit(f"{out} is not empty")

    seen = {}
    for wheel in wheels:
        name = release_name(wheel)
        if name in seen:
            raise SystemExit(f"two wheels for {name}: {seen[name].name} and {wheel.name}")
        seen[name] = wheel
        shutil.copyfile(wheel, out / wheel.name)
        (out / name).write_bytes(program_of(wheel))

    here = pathlib.Path(__file__).resolve().parent
    for installer in INSTALLERS:
        text = (here / "install" / installer).read_bytes()
        # The shell installer is piped into sh: a carriage return in it would break every line.
        if installer.endswith(".sh"):
            text = text.replace(b"\r\n", b"\n")
        (out / installer).write_bytes(text)

    files = sorted(p for p in out.iterdir() if p.name != "SHA256SUMS.txt")
    lines = [f"{sha256(p)}  {p.name}\n" for p in files]
    (out / "SHA256SUMS.txt").write_bytes("".join(lines).encode("ascii"))

    print(f"version {versions.pop()}: {len(wheels)} wheels, {len(seen)} programs")
    for p in files:
        print(f"  {p.stat().st_size:>12,}  {p.name}")
    missing = [name for _, _, name in PLATFORMS if name not in seen]
    if missing:
        print("not built: " + ", ".join(missing))


if __name__ == "__main__":
    main(sys.argv)
