#!/bin/sh
# keycloak-cli installer and updater.
#
#   curl -fsSL https://raw.githubusercontent.com/rust-utility/keycloak-cli/main/install.sh | sh
#
# Downloads the release binary for this platform, verifies it against the
# release's SHA256SUMS and installs it. Running it again updates an existing
# installation (or use `keycloak-cli self-update`).
#
# Options:
#   --version VERSION   install this version instead of the latest (v26.8.101 or 26.8.101)
#   --dir DIR           install into DIR (default: $INSTALL_DIR or ~/.local/bin)
#   --help
#
# Environment:
#   INSTALL_DIR         where the binary goes
#   VERSION             same as --version
#
set -eu

REPOSITORY="rust-utility/keycloak-cli"
BINARY="keycloak-cli"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${VERSION:-}"

usage() {
    cat <<'EOF'
Install or update keycloak-cli.

usage: install.sh [--version VERSION] [--dir DIR]

  --version VERSION  install this version instead of the latest (v26.8.101 or 26.8.101)
  --dir DIR          install into DIR (default: $INSTALL_DIR or ~/.local/bin)
  -h, --help         show this help

The script downloads the release binary for your platform, checks it against the
release's SHA256SUMS and installs it into DIR. Running it again updates the
binary; `keycloak-cli self-update` does the same from the CLI.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
    --version)
        [ $# -ge 2 ] || { echo "install.sh: --version needs a value" >&2; exit 1; }
        VERSION="$2"
        shift
        ;;
    --dir)
        [ $# -ge 2 ] || { echo "install.sh: --dir needs a value" >&2; exit 1; }
        INSTALL_DIR="$2"
        shift
        ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        echo "install.sh: unknown option: $1" >&2
        usage >&2
        exit 1
        ;;
    esac
    shift
done

command -v curl >/dev/null 2>&1 || {
    echo "install.sh: curl is required" >&2
    exit 1
}

# Platform -> release target.
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
Darwin) os="macos" ;;
Linux) os="linux" ;;
*)
    echo "install.sh: unsupported operating system: $os" >&2
    exit 1
    ;;
esac
case "$arch" in
x86_64 | amd64) arch="x86_64" ;;
arm64 | aarch64) arch="aarch64" ;;
*)
    echo "install.sh: unsupported architecture: $arch" >&2
    exit 1
    ;;
esac
case "$os-$arch" in
macos-aarch64) target="aarch64-apple-darwin" ;;
macos-x86_64) target="x86_64-apple-darwin" ;;
linux-x86_64) target="x86_64-unknown-linux-musl" ;;
linux-aarch64) target="aarch64-unknown-linux-musl" ;;
*)
    echo "install.sh: no release binary for $os $arch" >&2
    exit 1
    ;;
esac

# Which version: asked for, or the latest release.
if [ -z "$VERSION" ]; then
    VERSION="$(
        curl -fsSL "https://api.github.com/repos/$REPOSITORY/releases/latest" |
            sed -n 's/.*"tag_name" *: *"\([^"]*\)".*/\1/p' | head -n 1
    )"
    [ -n "$VERSION" ] || {
        echo "install.sh: cannot determine the latest release" >&2
        exit 1
    }
fi
VERSION="${VERSION#v}"

# What is installed already, if anything.
installed=""
if [ -x "$INSTALL_DIR/$BINARY" ]; then
    installed="$("$INSTALL_DIR/$BINARY" --version 2>/dev/null |
        sed -n 's/^keycloak-cli \([0-9][0-9.]*\).*/\1/p' | head -n 1)" || installed=""
fi
if [ "$installed" = "$VERSION" ]; then
    echo "$BINARY $VERSION is already installed in $INSTALL_DIR"
    exit 0
fi

temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT INT TERM

base="https://github.com/$REPOSITORY/releases/download/v$VERSION"
asset="$BINARY-$target"

echo "Downloading $BINARY $VERSION for $target..."
if ! curl -fsSL -o "$temporary/$asset" "$base/$asset"; then
    echo "install.sh: release v$VERSION has no binary for $target" >&2
    exit 1
fi
curl -fsSL -o "$temporary/SHA256SUMS" "$base/SHA256SUMS" || {
    echo "install.sh: release v$VERSION has no SHA256SUMS" >&2
    exit 1
}

expected="$(awk -v name="$asset" '$2 == name || $2 == "*" name { print $1; exit }' "$temporary/SHA256SUMS")"
[ -n "$expected" ] || {
    echo "install.sh: SHA256SUMS has no entry for $asset" >&2
    exit 1
}
if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$temporary/$asset" | awk '{ print $1 }')"
else
    actual="$(shasum -a 256 "$temporary/$asset" | awk '{ print $1 }')"
fi
[ "$expected" = "$actual" ] || {
    echo "install.sh: checksum mismatch for $asset" >&2
    echo "  expected $expected" >&2
    echo "  got      $actual" >&2
    exit 1
}

mkdir -p "$INSTALL_DIR"
install -m 755 "$temporary/$asset" "$INSTALL_DIR/.$BINARY.new"
mv -f "$INSTALL_DIR/.$BINARY.new" "$INSTALL_DIR/$BINARY"

if [ -n "$installed" ]; then
    echo "Updated $BINARY $installed -> $VERSION in $INSTALL_DIR"
else
    echo "Installed $BINARY $VERSION to $INSTALL_DIR/$BINARY"
fi

case ":$PATH:" in
*":$INSTALL_DIR:"*) ;;
*) echo "Add $INSTALL_DIR to your PATH to run $BINARY from anywhere." ;;
esac
