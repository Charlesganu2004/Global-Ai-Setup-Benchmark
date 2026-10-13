# Runs cargo for this workspace on Windows, with or without Microsoft's C++ build tools.
#
#   .\cargo.ps1 build --release
#   .\cargo.ps1 test -p lxr-graph
#
# With the build tools installed, this is plain cargo. Without them, Rust's default
# Windows toolchain cannot link, so this uses the GNU toolchain instead, which links
# with what rustup ships. One gap remains there: rustup's GNU toolchain has no working
# import-library tool, so the LLVM one from the `llvm-tools` component stands in.
# Nothing here needs a C compiler, because the workspace has no C dependency.
$ErrorActionPreference = 'Stop'
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if ($env:Path -notlike "*$cargoBin*") { $env:Path = "$cargoBin;$env:Path" }

function Test-MsvcLinker {
    if (Get-Command link.exe -ErrorAction SilentlyContinue) { return $true }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $false }
    $found = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    return [bool]$found
}

Push-Location $PSScriptRoot
try {
    if (Test-MsvcLinker) {
        & cargo @args
    } else {
        $toolchain = 'stable-x86_64-pc-windows-gnu'
        $installed = (& rustup toolchain list) -join "`n"
        if ($installed -notmatch [regex]::Escape($toolchain)) {
            & rustup toolchain install $toolchain --profile minimal --no-self-update
        }
        $rustBin = Join-Path $env:USERPROFILE ".rustup\toolchains\$toolchain\lib\rustlib\x86_64-pc-windows-gnu\bin"
        if (-not (Test-Path (Join-Path $rustBin 'llvm-ar.exe'))) {
            & rustup component add llvm-tools --toolchain $toolchain
        }
        # llvm-ar acts as llvm-dlltool when it is started under that name.
        $tools = Join-Path $env:USERPROFILE '.local-index\rust-build'
        $dlltool = Join-Path $tools 'llvm-dlltool.exe'
        if (-not (Test-Path $dlltool)) {
            New-Item -ItemType Directory -Force $tools | Out-Null
            Copy-Item (Join-Path $rustBin 'llvm-ar.exe') $dlltool
        }
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = "-Cdlltool=$dlltool"
        & cargo "+$toolchain" @args
    }
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
