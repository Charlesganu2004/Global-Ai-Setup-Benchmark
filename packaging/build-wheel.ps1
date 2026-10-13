# Builds the peterganurusty wheel (the pgr program as a pip package) for this machine.
#
#   .\packaging\build-wheel.ps1                      # model from the local Hugging Face cache
#   .\packaging\build-wheel.ps1 -ModelDir D:\model   # model from a folder
#   .\packaging\build-wheel.ps1 -Download            # model from the pinned revision on huggingface.co
#
# The wheel lands in packaging\dist. The embedding model is compiled into the program (the cargo
# feature embedded-model, which reads the model from the folder PGR_EMBED_MODEL_DIR names), so the
# wheel holds the program, LICENSE.txt and THIRD-PARTY-NOTICES.txt and nothing else. This script
# never builds a source distribution, and nothing here uploads anything.
#
# Toolchain: with the Microsoft C++ build tools installed this is a plain release build.
# Without them Rust's default Windows toolchain cannot link, so this does what
# rust\cargo.ps1 does: the GNU toolchain, plus an LLVM tool in place of the import-library
# tool that rustup's GNU toolchain lacks. The workspace has no C dependency.
param(
    [string]$Out = (Join-Path $PSScriptRoot 'dist'),
    [string]$ModelDir,
    [switch]$Download
)
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $root 'rust\crates\lxr\Cargo.toml'
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if ($env:Path -notlike "*$cargoBin*") { $env:Path = "$cargoBin;$env:Path" }

function Test-MsvcLinker {
    if (Get-Command link.exe -ErrorAction SilentlyContinue) { return $true }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $false }
    $found = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    return [bool]$found
}

function Invoke-Checked {
    param([string]$What, [scriptblock]$Command)
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# 1. A virtual environment of our own, with maturin in it.
$venv = Join-Path $PSScriptRoot '.venv'
$py = Join-Path $venv 'Scripts\python.exe'
$maturin = Join-Path $venv 'Scripts\maturin.exe'
if (-not (Test-Path $py)) {
    Invoke-Checked 'creating the virtual environment' { & python -I -m venv $venv }
}
if (-not (Test-Path $maturin)) {
    Invoke-Checked 'installing maturin' { & $py -I -m pip install --disable-pip-version-check 'maturin>=1.9,<2' }
}

# 2. The toolchain, the way rust\cargo.ps1 sets it up, and the target to build for.
$targetArgs = @()
$remap = @()
foreach ($pair in @(@($root, '/pgr-src'), @((Join-Path $env:USERPROFILE '.cargo'), '/cargo'), @((Join-Path $env:USERPROFILE '.rustup'), '/rustup'))) {
    # RUSTFLAGS are split at spaces, so a path with a space cannot be remapped.
    if ($pair[0] -notmatch '\s') { $remap += "--remap-path-prefix=$($pair[0])=$($pair[1])" }
}
if (Test-MsvcLinker) {
    Write-Host 'build: Microsoft C++ build tools found, using the default toolchain'
    if (-not $env:RUSTFLAGS -and $remap) { $env:RUSTFLAGS = $remap -join ' ' }
} else {
    $toolchain = 'stable-x86_64-pc-windows-gnu'
    $triple = 'x86_64-pc-windows-gnu'
    Write-Host "build: no Microsoft C++ build tools, using $toolchain"
    $installed = (& rustup toolchain list) -join "`n"
    if ($installed -notmatch [regex]::Escape($toolchain)) {
        Invoke-Checked 'installing the GNU toolchain' { & rustup toolchain install $toolchain --profile minimal --no-self-update }
    }
    $rustBin = Join-Path $env:USERPROFILE ".rustup\toolchains\$toolchain\lib\rustlib\x86_64-pc-windows-gnu\bin"
    if (-not (Test-Path (Join-Path $rustBin 'llvm-ar.exe'))) {
        Invoke-Checked 'adding llvm-tools' { & rustup component add llvm-tools --toolchain $toolchain }
    }
    # llvm-ar acts as llvm-dlltool when it is started under that name.
    $tools = Join-Path $env:USERPROFILE '.local-index\rust-build'
    $dlltool = Join-Path $tools 'llvm-dlltool.exe'
    if (-not (Test-Path $dlltool)) {
        New-Item -ItemType Directory -Force $tools | Out-Null
        Copy-Item (Join-Path $rustBin 'llvm-ar.exe') $dlltool
    }
    $env:RUSTUP_TOOLCHAIN = $toolchain
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = (@("-Cdlltool=$dlltool") + $remap) -join ' '
    $targetArgs = @('--target', $triple)
}

# 3. What goes into the build: the model (compiled into the program), the licence, the notices.
$stageArgs = @('-I', (Join-Path $PSScriptRoot 'stage.py'), '--cargo', (Get-Command cargo).Source)
if ($ModelDir) { $stageArgs += @('--model-dir', $ModelDir) }
if ($Download) { $stageArgs += '--download' }
Invoke-Checked 'staging the model and notices' { & $py @stageArgs }
# The program reads its model from this folder when it is compiled (see rust\crates\lxr\build.rs).
$env:PGR_EMBED_MODEL_DIR = Join-Path $PSScriptRoot 'stage\model'
if (-not (Test-Path (Join-Path $env:PGR_EMBED_MODEL_DIR 'model.safetensors'))) { throw "no model in $env:PGR_EMBED_MODEL_DIR" }

# 4. The wheel. Only `maturin build` without --sdist: a source distribution would hold the source.
New-Item -ItemType Directory -Force $Out | Out-Null
Get-ChildItem $Out -Filter 'peterganurusty-*.whl' -ErrorAction SilentlyContinue | Remove-Item -Force
Invoke-Checked 'maturin build' {
    & $maturin build --release --strip --locked --manifest-path $manifest --out $Out @targetArgs
}
$wheel = Get-ChildItem $Out -Filter 'peterganurusty-*.whl' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $wheel) { throw "maturin built no wheel in $Out" }

# 5. Check what was built, and that the builder's folders are not inside the binary.
Invoke-Checked 'verifying the wheel' {
    & $py -I (Join-Path $PSScriptRoot 'verify_wheel.py') $wheel.FullName --model-dir $env:PGR_EMBED_MODEL_DIR --forbid $env:USERPROFILE --forbid $root
}
Write-Host ("wheel: {0} ({1:N1} MB)" -f $wheel.FullName, ($wheel.Length / 1MB))
