# Packaging PeterGanuRusty (pgr) as a pip package

Private maintainer notes. This folder turns the Rust program `pgr` (package `lxr` in
`rust/crates/lxr`) into a Python wheel named `peterganurusty`, so that people can type
`pip install peterganurusty` and get a `pgr` command, the way they install any Python tool.
The wheel holds a compiled binary only. The source stays in this private repository.

## What is in a wheel

| Where it lands after `pip install` | What |
| --- | --- |
| `<env>/Scripts/pgr.exe` (Windows) or `<env>/bin/pgr` | the stripped program, about 40 MB, with the embedding model inside it |
| `<env>/share/peterganurusty/LICENSE.txt` | placeholder: licence terms not yet published, all rights reserved |
| `<env>/share/peterganurusty/THIRD-PARTY-NOTICES.txt` | every crate linked into `pgr` with version, licence and repository, the licence notice of the embedding model `potion-code-16M-v2` (MIT), and the licence texts the crates carry |

That is all: there is no model folder in the wheel. The model (`model.safetensors` and
`tokenizer.json`, 33 MB) is compiled into the program with the cargo feature `embedded-model`
(`rust/crates/lxr/src/model.rs`, `include_bytes!` from the folder that the environment variable
`PGR_EMBED_MODEL_DIR` names at build time). So `pgr` has its model wherever the executable
is: in a virtual environment, in a copy that `uv tool install` or `copy pgr.exe` leaves in some
other folder, or on a machine that has no Hugging Face cache.

At run time `pgr` looks for a model in this order: the folder in `LXR_MODEL_DIR` (when it holds the
two files), the model inside the program, and only then the places it looked before the model was
compiled in (`share/peterganurusty/model` one and two folders above the program, resolving
symbolic links; `~/.local-index/models/...`; the Hugging Face cache). A program built without the
feature behaves as before. The vocabulary of the model inside the program is cached in
`~/.local-index/rust/model-cache`, in a file named after a hash of the tokenizer bytes.

The files that build a wheel:

| File | Job |
| --- | --- |
| `rust/crates/lxr/pyproject.toml` | maturin configuration: `bindings = "bin"`, strip, the feature `embedded-model`, the data folder, metadata |
| `rust/crates/lxr/build.rs` | checks `PGR_EMBED_MODEL_DIR` (absolute, holds the two files) when the feature is on |
| `packaging/stage.py` | puts the model (checked against pinned SHA-256 values) into `packaging/stage/model/`, and the licence and the notices into `packaging/stage/wheel/` |
| `packaging/notices.py` | writes `THIRD-PARTY-NOTICES.txt` from `cargo metadata` |
| `packaging/build-wheel.ps1` | the whole local build on Windows, with or without the Microsoft C++ tools; sets `PGR_EMBED_MODEL_DIR` |
| `packaging/verify_wheel.py` | checks a built wheel: no source, stripped binary, the model inside the binary byte for byte, only the two licence files as data, no LGPL crate in the notices, metadata |
| `packaging/stranger_test.py` | installs a wheel in a fresh virtual environment, runs `pgr` from it and from a lone copy, then repeats with `uv tool install` |
| `.github/workflows/wheels.yml` | builds the wheels for Linux, macOS and Windows on GitHub (manual start only) |
## Which commands are safe

A wheel contains only the compiled binary and data files. A source distribution contains the
Rust source. So:

Safe:

    maturin build --release        (a wheel only; this is what build-wheel.ps1 runs)
    twine upload dist/*.whl        (only wheel files)
    maturin upload dist/*.whl      (only the files you name)
    maturin publish --no-sdist     (builds a wheel, uploads wheels only)

Leaks the source, never run these:

    maturin sdist                  (writes the source distribution)
    maturin build --sdist          (writes a wheel and the source distribution)
    maturin publish                (without --no-sdist it builds and uploads the sdist to PyPI)
    twine upload dist/*            (uploads whatever is in dist, an sdist included)

`build-wheel.ps1` deletes old `peterganurusty-*.whl` files from `packaging/dist` before it
builds and never creates an sdist. Look in `dist` for a `.tar.gz` before every upload anyway.
Anything on the Python index is public and cannot be taken back: PyPI allows deleting a
release but anyone may already have copied it.

## Build and test on this machine (Windows)

From PowerShell in the repository root:

    .\packaging\build-wheel.ps1                        # model from the local Hugging Face cache
    .\packaging\build-wheel.ps1 -ModelDir D:\model     # model from a folder with the two files
    .\packaging\build-wheel.ps1 -Download              # model from the pinned revision on huggingface.co

The script creates `packaging\.venv` with maturin, stages the model and the data files, sets
`PGR_EMBED_MODEL_DIR` to `packaging\stage\model`, builds the wheel into `packaging\dist`, and
runs `verify_wheel.py` on it. This machine has no Microsoft C++ build
tools, so the script uses the GNU toolchain exactly as `rust\cargo.ps1` does
(`RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-gnu`, `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS`
with `-Cdlltool=...\llvm-dlltool.exe`, target `x86_64-pc-windows-gnu`). When the Microsoft tools
are present it builds with the default toolchain. The script also tells rustc to rewrite the
repository folder, `~/.cargo` and `~/.rustup` in the paths that end up in panic messages, so the
binary does not carry the builder's user name. `verify_wheel.py` fails the build if it does.

Run the stranger test (a new virtual environment, no model anywhere but the wheel's own):

    .\packaging\.venv\Scripts\python.exe -I .\packaging\stranger_test.py .\packaging\dist\peterganurusty-0.1.0-py3-none-win_amd64.whl --repo C:\Users\Charl\Downloads\lxbench\repo

It installs the wheel with pip, runs `pgr`, copies the program alone into an empty folder and runs
that, uninstalls, and then installs `uv` with pip into a scratch environment and does
`uv tool install` of the wheel, with uv's tool, bin and cache folders inside the temporary folder.
`--no-uv` leaves the last part out; it needs network access to the Python package index.

On Linux or macOS the same steps by hand:

    python3 -I -m venv packaging/.venv && packaging/.venv/bin/python -I -m pip install "maturin>=1.9,<2"
    packaging/.venv/bin/python -I packaging/stage.py --download
    export PGR_EMBED_MODEL_DIR="$PWD/packaging/stage/model"
    packaging/.venv/bin/maturin build --release --strip --locked --manifest-path rust/crates/lxr/Cargo.toml --out packaging/dist
    packaging/.venv/bin/python -I packaging/verify_wheel.py packaging/dist/peterganurusty-*.whl

Rust tests for the part that finds and opens the model (the tests of `lxr-text` open a model from a
folder and from bytes in memory; the tests of the program with the model inside it need the
variable):

    cmd /c "powershell -NoProfile -ExecutionPolicy Bypass -File rust\cargo.ps1 test -p lxr-text 2>&1"
    $env:PGR_EMBED_MODEL_DIR = (Resolve-Path packaging\stage\model).Path
    cmd /c "powershell -NoProfile -ExecutionPolicy Bypass -File rust\cargo.ps1 test -p lxr --features embedded-model --test embedded_model 2>&1"

## What was tested

Tested on this Windows machine (2026-10-13) with the wheel `peterganurusty-0.1.0-py3-none-win_amd64.whl`
(34,246,487 bytes; the program inside is 41,733,120 bytes), by `stranger_test.py` in a new virtual
environment with the home folder and the Hugging Face cache empty and `LXR_MODEL_DIR` unset:

- `pip install` puts `pgr.exe` in `Scripts\` and exactly `LICENSE.txt` and `THIRD-PARTY-NOTICES.txt`
  in `share\peterganurusty\`; there is no model folder.
- `pgr refresh` builds an index and `pgr status` says "search ready (5652 chunks, embeddings)": the
  program found its model, which can only be the one inside it.
- A copy of `pgr.exe` alone in an empty folder says "embeddings" too, builds an index, and the index
  has embeddings.
- `pip uninstall` removed the command and the `share\peterganurusty` folder completely.
- `uv tool install <wheel>` (uv 0.13.0, installed with pip into a scratch environment, with uv's tool,
  bin and cache folders inside the temporary folder): uv copies `pgr.exe` into its bin folder, and that
  copy, run with no model anywhere else, says "embeddings"; so does the program inside uv's tool
  environment. `uv tool uninstall` left nothing behind.

Before the model was compiled in, the copy that uv makes was not next to the model and said "no
embedding model found, search uses words only". That limit is gone.

Not tested: `pip install --user` after this change (the model no longer depends on where the
executable sits, so it should not matter), any Linux or macOS wheel, and the effect of
the model being inside the program on antivirus scans of a 40 MB executable.
## Building for the other platforms on GitHub

`.github/workflows/wheels.yml` builds Linux x86_64 and aarch64 (manylinux), macOS x86_64 and
arm64, and Windows x86_64 with the official `PyO3/maturin-action`. To run it, on GitHub open the
repository, Actions, "wheels", "Run workflow" (or `gh workflow run wheels.yml`). It has no
trigger other than that button, no publish step, no secret and no source distribution. Each job
uploads its wheel as an artifact of the run (kept 90 days by default); download them with the web
page or `gh run download RUN_ID`.

The model is not in the repository. Every job runs `python -I packaging/stage.py --download`,
which fetches the two files the program reads (`model.safetensors`, `tokenizer.json`) from
`huggingface.co/minishlab/potion-code-16M-v2` at revision
`e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b` and stops unless each file matches the SHA-256 value
pinned in `stage.py` (computed from the local copies). A pinned revision plus hashes means a
change on Hugging Face cannot alter a wheel without anyone noticing.

The workflow has not been run yet. On its first run, check:

1. The `stage` step downloads both files and the hashes match (it needs outbound access to
   huggingface.co, which redirects to a CDN).
2. `cargo metadata --locked` works on the runner and prints the one unusual licence as here
   (`xxhash-rust`, BSL-1.0), and no LGPL crate.
3. `PyO3/maturin-action@v1` accepts `data = "../../../packaging/stage/wheel"` inside the manylinux
   container (the folder is read from the mounted workspace; if the action runs maturin from a
   different working folder the relative path may need a change).
4. The aarch64 Linux wheel is built (cross-compiled in the action's container) and carries the
   tag `manylinux_2_17_aarch64.manylinux2014_aarch64`; the macOS x86_64 wheel is cross-built on an
   arm64 runner.
5. `verify_wheel.py` passes on every wheel, in particular the strip check on ELF files, and
   `stranger_test.py` passes on Linux x86_64, macOS arm64 and Windows.
6. Wheel sizes are about 34 MB (the program is about 40 MB with the 32 MB model inside it, which
   hardly compresses; the Windows wheel is 34.2 MB), none is much larger. Linux and macOS programs
   are built in a container (`docker-options` gives it `PGR_EMBED_MODEL_DIR=/io/packaging/stage/model`,
   which assumes that the action mounts the checkout at `/io`; check that the build finds the model
   folder, or the `build.rs` assertion says which path it looked at).
7. Pin the actions to commit SHAs once the first run is green (`actions/checkout`,
   `actions/setup-python`, `actions/upload-artifact`, `PyO3/maturin-action`); they are on moving
   major tags now.

## How the wheels could reach users

Nothing in this repository publishes anything. These are the options and what each needs from
the owner. A user needs the wheel that matches the platform; pip picks it by itself on an index
and does not on a plain link.

### (a) The Python package index (PyPI)

Needs: a PyPI account with two-factor login, and an API token the owner creates himself (Account
settings, API tokens; the first token is for the whole account, replace it by one limited to the
`peterganurusty` project after the first upload). The owner runs the upload; nobody else should
hold the token. Try TestPyPI first. Wheels only:

    packaging\.venv\Scripts\python.exe -I -m pip install twine
    packaging\.venv\Scripts\python.exe -I -m twine upload --repository testpypi packaging\dist\*.whl
    packaging\.venv\Scripts\python.exe -I -m twine upload packaging\dist\*.whl

(user name `__token__`, password the token). The name `peterganurusty` was free on PyPI on
2026-10-13; the first upload claims it. A user then types:

    pip install peterganurusty
    pgr --version

Anyone in the world can download the program and, with enough effort, take the binary apart.
Platforms without a wheel get "no matching distribution", because there is no sdist to build
from: that is what keeps the source private, and it means every platform you want to support
needs its wheel in the upload.

### (b) A separate public repository that holds only release files

Needs: a second, public GitHub repository (for example `peterganurusty-releases`) with no source
in it, only a README and release assets. The owner creates it and uploads the wheels as release
assets:

    gh release create v0.1.0 packaging\dist\*.whl --repo OWNER/peterganurusty-releases --title "pgr 0.1.0"

A user then types, for Windows (the file name is the one in the release):

    pip install https://github.com/OWNER/peterganurusty-releases/releases/download/v0.1.0/peterganurusty-0.1.0-py3-none-win_amd64.whl

or, to let pip choose the wheel that fits the machine, point it at the release page of assets
(not tested here):

    pip install peterganurusty --find-links https://github.com/OWNER/peterganurusty-releases/releases/expanded_assets/v0.1.0

No account and no token on the user's side. The wheels are public, as with PyPI, but the owner
controls the repository and no PyPI account is needed.

### (c) Release files on this private repository

Needs: nothing new from the owner except giving each person read access to the repository. Only
those people can fetch the files. A plain `pip install <url>` does not work on a private
repository (GitHub answers 404 without credentials), so the user needs the GitHub command line
tool (`gh`), logged in as a person with access:

    gh release download v0.1.0 --repo OWNER/Global-Ai-Setup-Benchmark --pattern "*win_amd64.whl" --dir .
    pip install .\peterganurusty-0.1.0-py3-none-win_amd64.whl

or from a workflow run (the artifact name is in the Actions page):

    gh run download RUN_ID --repo OWNER/Global-Ai-Setup-Benchmark --name wheel-windows-x86_64
    pip install .\peterganurusty-0.1.0-py3-none-win_amd64.whl

### A user without Python

A wheel is a zip file. The bare program is inside it:

1. Download the wheel for the platform and rename `.whl` to `.zip`, then extract it.
2. Take `peterganurusty-0.1.0.data/scripts/pgr.exe` (or `pgr`) and put it in a folder `X/bin`.
3. Add `X/bin` to the search path. On macOS and Linux run `chmod +x X/bin/pgr`.

The model is inside the program, so nothing else has to travel with it. The two licence files are in
`peterganurusty-0.1.0.data/data/share/peterganurusty/`.

An installer for people without Python (a signed `.msi`, a Homebrew formula, a `curl | sh`
script) is not part of this work.

## Open decisions for the owner

- **Licence terms.** `LICENSE.txt` says the terms are not yet published and all rights are
  reserved, and the wheel is marked `License :: Other/Proprietary License`. Choose real terms
  before the first public upload. Until then, "all rights reserved" means strangers have no
  permission to copy or share the program.
- **Licences of the crates.** No LGPL crate is linked in any more (`rustpython-parser` is built with
  `num-bigint` instead of the five `malachite` crates; `verify_wheel.py` fails a wheel whose notices
  name one). The only licence outside the common permissive ones is `xxhash-rust`, BSL-1.0 (Boost,
  permissive). The model is MIT.
- **Where to publish.** PyPI, a public releases repository, the private repository, or a mix.
- **The name.** `peterganurusty` is free on PyPI; a project named `pgr` already exists there and
  may install a command with the same name `pgr`.
- **Licence texts of 24 crates.** Their packages carry no licence file (the `oxc_*`,
  `rustpython-*` and `unic-*` crates among them). `THIRD-PARTY-NOTICES.txt` gives their licence
  expression and repository only. MIT and BSD ask that the copyright notice travels with the
  binary, so fetch those texts from the repositories before a public release.
