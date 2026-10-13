"""Prepares what goes into the wheel, and the model that is compiled into the program.

Writes packaging/stage/ (or --out) with this layout:

    model/model.safetensors             the embedding model that the build compiles into pgr
    model/tokenizer.json                (PGR_EMBED_MODEL_DIR names this folder; see below)
    wheel/data/share/peterganurusty/LICENSE.txt
    wheel/data/share/peterganurusty/THIRD-PARTY-NOTICES.txt

pyproject.toml points maturin at stage/wheel as its data folder: the data/ inside it is unpacked
into the root of the environment the wheel is installed in. (maturin cannot take licence files
from outside the project folder for the wheel's dist-info, so they travel as data files.) The
wheel holds nothing else but the program: the model is inside the program, so it is not a data
file. The licence of the model is part 2 of THIRD-PARTY-NOTICES.txt.

The program is built with the cargo feature `embedded-model`, which reads the two model files from
the folder named by the environment variable PGR_EMBED_MODEL_DIR when the program is compiled.
build-wheel.ps1 sets it to stage/model, and the GitHub workflow does the same.

The model is checked against the SHA-256 values pinned below, wherever it comes from: the local
Hugging Face cache (default), a folder (--model-dir) or the pinned revision on huggingface.co
(--download). A file that does not match stops the build.

    python -I stage.py                    # model from the local Hugging Face cache
    python -I stage.py --model-dir DIR    # model from DIR
    python -I stage.py --download         # model from the pinned revision on huggingface.co
"""

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path

MODEL_REPO = "minishlab/potion-code-16M-v2"
MODEL_REVISION = "e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b"
MODEL_URL = "https://huggingface.co/" + MODEL_REPO + "/resolve/" + MODEL_REVISION + "/{name}"
# SHA-256 and size of each file the program reads at that revision, computed from the copies in the
# local cache. (config.json of the model repository is not read by the program and not compiled in.)
MODEL_FILES = {
    "model.safetensors": ("75cf7a6c2171b230ad19b1e7d8e0b1aee86da5a02af8e7cacedd9921d227623c", 32490072),
    "tokenizer.json": ("107bbdcbad4bff1d299b7a4c3a2fb17c52890688b7dd0e4c9deab79d3c4f3d45", 1024340),
}

SHARE = Path("wheel") / "data" / "share" / "peterganurusty"
MODEL_DIR = Path("model")


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def check(path, name):
    expected, size = MODEL_FILES[name]
    if path.stat().st_size != size or sha256_of(path) != expected:
        sys.exit(f"{path} is not {name} at revision {MODEL_REVISION}: size or SHA-256 differs from the pinned values")


def hub_snapshot():
    """The folder of the pinned revision in the local Hugging Face cache, if it is there."""
    hub = os.environ.get("HF_HUB_CACHE")
    if not hub:
        home = os.environ.get("HF_HOME")
        hub = str(Path(home) / "hub") if home else str(Path.home() / ".cache" / "huggingface" / "hub")
    return Path(hub) / ("models--" + MODEL_REPO.replace("/", "--")) / "snapshots" / MODEL_REVISION


def download(name, target):
    request = urllib.request.Request(MODEL_URL.format(name=name), headers={"User-Agent": "peterganurusty-packaging"})
    with urllib.request.urlopen(request, timeout=120) as response, open(target, "wb") as out:
        shutil.copyfileobj(response, out, 1 << 20)


def main():
    here = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--out", default=str(here / "stage"))
    source = parser.add_mutually_exclusive_group()
    source.add_argument("--model-dir", help="a folder that holds model.safetensors and tokenizer.json")
    source.add_argument("--download", action="store_true", help="fetch the files from the pinned revision on huggingface.co")
    parser.add_argument("--cargo", default="cargo", help="the cargo command, for the notices")
    args = parser.parse_args()

    out = Path(args.out).resolve()
    if out.exists():
        shutil.rmtree(out)
    model_out = out / MODEL_DIR
    model_out.mkdir(parents=True)
    (out / SHARE).mkdir(parents=True)

    if args.download:
        for name in MODEL_FILES:
            download(name, model_out / name)
            check(model_out / name, name)
    else:
        folder = Path(args.model_dir) if args.model_dir else hub_snapshot()
        if not folder.is_dir():
            sys.exit(f"no model folder at {folder}; use --model-dir DIR or --download")
        for name in MODEL_FILES:
            if not (folder / name).is_file():
                sys.exit(f"{folder} has no {name}")
            check(folder / name, name)
            shutil.copyfile(folder / name, model_out / name)

    notices = out / SHARE / "THIRD-PARTY-NOTICES.txt"
    done = subprocess.run(
        [
            sys.executable, "-I", str(here / "notices.py"),
            "--manifest", str(here.parent / "rust" / "Cargo.toml"),
            "--out", str(notices),
            "--cargo", args.cargo,
        ]
    )
    if done.returncode != 0:
        sys.exit("notices.py failed")
    shutil.copyfile(here / "LICENSE.txt", out / SHARE / "LICENSE.txt")

    files = [f for f in out.rglob("*") if f.is_file()]
    total = sum(f.stat().st_size for f in files)
    print(f"staged {len(files)} files, {total} bytes, in {out}")
    print(f"PGR_EMBED_MODEL_DIR for the build: {model_out}")


if __name__ == "__main__":
    main()
