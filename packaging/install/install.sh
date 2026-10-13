#!/bin/sh
# Installs pgr (PeterGanuRusty) for the current user on Linux or macOS. No root, no Python,
# no other download: one program file, checked against the published SHA-256.
#
#   curl -fsSL https://github.com/Charlesganu2004/Global-Ai-Setup-Benchmark/releases/latest/download/install.sh | sh
#
# Options, set as environment variables before the line above:
#   PGR_VERSION=0.1.0            a given release instead of the latest
#   PGR_INSTALL_DIR=/some/folder where pgr goes (default: $HOME/.local/bin)
#   PGR_BASE_URL=https://...     another place that holds the release files (for mirrors and tests)

set -eu

repo="Charlesganu2004/Global-Ai-Setup-Benchmark"

fail() {
    echo "pgr: $1" >&2
    exit 1
}

case "$(uname -s)" in
    Linux) os="linux" ;;
    Darwin) os="macos" ;;
    *) fail "this installer covers Linux and macOS. On Windows use install.ps1." ;;
esac

case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="aarch64" ;;
    *) fail "no pgr build for the processor $(uname -m)." ;;
esac

# The Linux builds link the GNU C library. A musl system (Alpine) cannot run them.
if [ "$os" = "linux" ] && [ -e /etc/alpine-release ]; then
    fail "the Linux build needs the GNU C library; Alpine (musl) is not covered yet."
fi

asset="pgr-$os-$arch"

if [ -n "${PGR_BASE_URL:-}" ]; then
    base="${PGR_BASE_URL%/}"
elif [ -n "${PGR_VERSION:-}" ]; then
    base="https://github.com/$repo/releases/download/v$PGR_VERSION"
else
    base="https://github.com/$repo/releases/latest/download"
fi

dir="${PGR_INSTALL_DIR:-$HOME/.local/bin}"

fetch() {
    # fetch URL FILE
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$1" -O "$2"
    else
        fail "neither curl nor wget is installed."
    fi
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d " " -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d " " -f 1
    else
        fail "neither sha256sum nor shasum is installed, so the download cannot be checked."
    fi
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "pgr: downloading from $base"
fetch "$base/SHA256SUMS.txt" "$work/SHA256SUMS.txt"
fetch "$base/$asset" "$work/$asset"

# A line of the list reads: <64 hex digits><two spaces><file name>
want="$(awk -v name="$asset" '{ file = $2; sub(/^\*/, "", file); if (file == name) print tolower($1) }' "$work/SHA256SUMS.txt")"
[ -n "$want" ] || fail "SHA256SUMS.txt does not list $asset."
have="$(sha256_of "$work/$asset")"
if [ "$have" != "$want" ]; then
    fail "the download does not match its published SHA-256 (expected $want, got $have). Nothing was installed."
fi

mkdir -p "$dir"
chmod 755 "$work/$asset"
mv -f "$work/$asset" "$dir/pgr"

version="$("$dir/pgr" --version)" || fail "pgr was installed to $dir/pgr but did not run."
echo "pgr: installed $version to $dir/pgr"

case ":$PATH:" in
    *":$dir:"*) ;;
    *) echo "pgr: $dir is not on your PATH. Add this line to your shell profile:  export PATH=\"$dir:\$PATH\"" ;;
esac

echo ""
echo "Next, inside a git repository:"
echo "  pgr refresh                       build the index"
echo "  pgr ask about:SomeFunction        definition, callers and what it calls"
echo "  pgr find \"where tokens expire\"    search by meaning"
echo "  pgr help                          every command"
