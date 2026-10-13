# Installs pgr (PeterGanuRusty) for the current user on Windows. No administrator rights,
# no Python, no other download: one program file, checked against the published SHA-256.
#
#   irm https://github.com/Charlesganu2004/Global-Ai-Setup-Benchmark/releases/latest/download/install.ps1 | iex
#
# Options, set as environment variables before the line above:
#   $env:PGR_VERSION     = "0.1.0"          a given release instead of the latest
#   $env:PGR_INSTALL_DIR = "C:\some\folder" where pgr.exe goes (default: %LOCALAPPDATA%\Programs\pgr)
#   $env:PGR_NO_PATH     = "1"              do not add that folder to the user PATH
#   $env:PGR_BASE_URL    = "https://..."    another place that holds the release files (for mirrors and tests)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$repo = "Charlesganu2004/Global-Ai-Setup-Benchmark"
$asset = "pgr-windows-x86_64.exe"

if (-not [Environment]::Is64BitOperatingSystem) {
    throw "pgr needs 64-bit Windows."
}

if ($env:PGR_BASE_URL) {
    $base = $env:PGR_BASE_URL.TrimEnd("/")
} elseif ($env:PGR_VERSION) {
    $base = "https://github.com/$repo/releases/download/v$($env:PGR_VERSION)"
} else {
    $base = "https://github.com/$repo/releases/latest/download"
}

$dir = if ($env:PGR_INSTALL_DIR) { $env:PGR_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\pgr" }
$work = Join-Path ([IO.Path]::GetTempPath()) ("pgr-install-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $work | Out-Null

try {
    Write-Host "pgr: downloading from $base"
    $sums = Join-Path $work "SHA256SUMS.txt"
    $file = Join-Path $work $asset
    Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS.txt" -OutFile $sums
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $file

    # A line of the list reads: <64 hex digits><two spaces><file name>
    $want = $null
    foreach ($line in Get-Content $sums) {
        $parts = $line.Trim() -split "\s+", 2
        if ($parts.Count -eq 2 -and $parts[1].TrimStart("*") -eq $asset) { $want = $parts[0].ToLowerInvariant() }
    }
    if (-not $want) { throw "SHA256SUMS.txt does not list $asset." }
    $have = (Get-FileHash -Algorithm SHA256 -Path $file).Hash.ToLowerInvariant()
    if ($have -ne $want) {
        throw "The download does not match its published SHA-256 (expected $want, got $have). Nothing was installed."
    }

    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $target = Join-Path $dir "pgr.exe"
    Move-Item -Force -Path $file -Destination $target

    $version = & $target --version
    if ($LASTEXITCODE -ne 0) { throw "pgr.exe was installed to $target but did not run." }
    Write-Host "pgr: installed $version to $target"

    if ($env:PGR_NO_PATH -ne "1") {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $entries = @($userPath -split ";" | Where-Object { $_ })
        if ($entries -notcontains $dir) {
            [Environment]::SetEnvironmentVariable("Path", (($entries + $dir) -join ";"), "User")
            Write-Host "pgr: added $dir to your user PATH. Open a new terminal to use the pgr command."
        }
        if (($env:Path -split ";") -notcontains $dir) { $env:Path = "$env:Path;$dir" }
    } else {
        Write-Host "pgr: PATH left as it is. Run it as $target"
    }

    Write-Host ""
    Write-Host "Next, inside a git repository:"
    Write-Host "  pgr refresh                       build the index"
    Write-Host "  pgr ask about:SomeFunction        definition, callers and what it calls"
    Write-Host "  pgr find ""where tokens expire""    search by meaning"
    Write-Host "  pgr help                          every command"
} finally {
    Remove-Item -Recurse -Force -Path $work -ErrorAction SilentlyContinue
}
