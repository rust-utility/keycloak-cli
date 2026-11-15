#!/usr/bin/env bash
#
# Builds the macOS release binaries and attaches them to a GitHub release,
# together with a `SHA256SUMS` file covering every asset.
#
# The release workflow builds the Linux binaries; GitHub never assigned a runner
# to the macOS jobs for this organisation (the jobs are cancelled at creation with
# no steps), so the Apple binaries are built here, on a Mac, until that is fixed.
# The result is the same: `keycloak-cli-<target>` files plus `SHA256SUMS`, which is
# exactly what `install.sh` and `keycloak-cli self-update` consume.
#
# Usage: scripts/release-macos.sh v26.8.101
#
# Requires: the pinned Rust toolchain (`rust-toolchain.toml`), both Apple targets,
# and an authenticated `gh`.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
repository=rust-utility/keycloak-cli
targets=(aarch64-apple-darwin x86_64-apple-darwin)

note() { printf 'release-macos: %s\n' "$*" >&2; }
die() {
    printf 'release-macos: error: %s\n' "$*" >&2
    exit 1
}

tag=${1:-}
[ -n "$tag" ] || die "usage: scripts/release-macos.sh <tag>, e.g. v26.8.101"
[[ $tag == v* ]] || tag="v$tag"

command -v gh >/dev/null 2>&1 || die "gh is required"
gh release view "$tag" --repo "$repository" >/dev/null 2>&1 ||
    die "release $tag does not exist in $repository"

for target in "${targets[@]}"; do
    rustup target add --toolchain 1.88 "$target" >/dev/null 2>&1 ||
        die "cannot add the $target target to the pinned toolchain"
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

for target in "${targets[@]}"; do
    note "building $target"
    (
        cd "$repo_root" &&
            cargo build --release --locked --target "$target"
    ) || die "the $target build failed"
    cp "$repo_root/target/$target/release/keycloak-cli" "$work/keycloak-cli-$target"
    chmod 755 "$work/keycloak-cli-$target"
    # Symbols are not needed by users and roughly halve the download.
    strip -x "$work/keycloak-cli-$target" 2>/dev/null || true
    note "built $(du -h "$work/keycloak-cli-$target" | cut -f1) for $target"
done

note "attaching the macOS binaries to $tag"
gh release upload "$tag" \
    "${targets[@]/#/$work/keycloak-cli-}" \
    --clobber --repo "$repository"

# SHA256SUMS covers every asset of the release, Linux ones included.
note "downloading the other assets to checksum them"
gh release download "$tag" --repo "$repository" \
    --pattern 'keycloak-cli-*' --dir "$work/checksums"
(
    cd "$work/checksums"
    sha256sum keycloak-cli-* >SHA256SUMS 2>/dev/null ||
        shasum -a 256 keycloak-cli-* >SHA256SUMS
    note "checksums:"
    sed 's/^/  /' SHA256SUMS >&2
    gh release upload "$tag" SHA256SUMS --clobber --repo "$repository"
)

note "release $tag now has:"
gh release view "$tag" --repo "$repository" --json assets \
    -q '.assets[] | "  \(.name) \(.size) bytes"' >&2
