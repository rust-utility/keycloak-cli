#!/usr/bin/env bash
#
# Generates `src/generated/` from the `keycloak` crate pinned by this package's
# version.
#
# The crate published on crates.io carries both the OpenAPI description
# (`api/openapi.json`) and the generator that produces the `keycloak` crate
# (`examples/openapi.rs`), so generation needs no checkout of the `keycloak`
# repository and is pinned to a published release: the tarball is downloaded,
# unpacked into a cache directory and the generator is run from there.
#
# The output is reproducible: it carries no timestamps and is formatted with the
# toolchain pinned in `rust-toolchain.toml`, so `--check` can prove that the
# committed sources match the pinned crate.
#
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cache_root=${KEYCLOAK_CLI_GENERATE_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/keycloak-cli/generate}
crate_url_template=https://crates.io/api/v1/crates/keycloak/{version}/download

# Tags whose modules are written. Q3 widens this to every tag of the crate; the
# generator already emits and validates all of them on every run.
tags_to_emit=${KEYCLOAK_CLI_GENERATE_TAGS:-all}

check=false
refresh=false
crate_file=
output=$repo_root/src/generated

note() { printf 'generate: %s\n' "$*" >&2; }
die() {
    printf 'generate: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: scripts/generate.sh [--check] [--refresh] [--crate FILE] [-o DIR]

Generates src/generated/ from the `keycloak` crate whose version equals this
package's version: the tarball is downloaded from crates.io, unpacked into a
cache directory and its own generator (`examples/openapi.rs`) is run.

  --check         regenerate into a temporary directory and fail if the result
                  differs from the committed sources (CI drift check)
  --refresh       discard the cached tarball and unpacked crate first
  --crate FILE    use a local `.crate` tarball instead of downloading one
  -o, --output DIR  write into DIR instead of `src/generated`

Environment:
  KEYCLOAK_CLI_GENERATE_CACHE  cache directory, default
                               ${XDG_CACHE_HOME:-$HOME/.cache}/keycloak-cli/generate
EOF
}

while [ $# -gt 0 ]; do
    case $1 in
    -h | --help)
        usage
        exit 0
        ;;
    --check) check=true ;;
    --refresh) refresh=true ;;
    --crate)
        [ $# -ge 2 ] || die "--crate needs a value"
        crate_file=$2
        shift
        ;;
    -o | --output)
        [ $# -ge 2 ] || die "$1 needs a value"
        output=$2
        shift
        ;;
    *) die "unknown argument: $1 (try --help)" ;;
    esac
    shift
done

# --- version ---------------------------------------------------------------

# The single source of truth is the package version: same scheme as the
# `keycloak` crate, so `keycloak-cli 26.8.100` wraps `keycloak 26.8.100`.
version=$(
    sed -n 's/^version = "\(.*\)"$/\1/p' "$repo_root/Cargo.toml" | head -n 1
)
[ -n "$version" ] || die "cannot read the version from $repo_root/Cargo.toml"
case $version in
[0-9]*.[0-9]*.[0-9]*) ;;
*) die "package version '$version' is not of the form x.y.z" ;;
esac
case $version in
*[!0-9.]*) die "package version '$version' is not of the form x.y.z" ;;
esac

# --- crate tarball ---------------------------------------------------------

work_dir=$(mktemp -d "${TMPDIR:-/tmp}/keycloak-cli-generate.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT

crate_path=$crate_file
if [ -z "$crate_path" ]; then
    crate_path=$cache_root/download/keycloak-$version.crate
    if $refresh; then
        rm -f "$crate_path"
    fi
    if [ ! -f "$crate_path" ]; then
        url=${crate_url_template/\{version\}/$version}
        note "downloading $url"
        mkdir -p "$(dirname "$crate_path")"
        curl -fsSL --retry 3 --retry-delay 2 -o "$crate_path.part" "$url" ||
            die "cannot download $url"
        mv "$crate_path.part" "$crate_path"
    fi
else
    [ -f "$crate_path" ] || die "no such crate file: $crate_path"
fi

tar -tzf "$crate_path" >"$work_dir/crate-files.txt" ||
    die "$crate_path is not a gzip-compressed tarball"
for required in Cargo.toml api/openapi.json examples/openapi.rs examples/openapi.patch.toml; do
    grep -qx "keycloak-$version/$required" "$work_dir/crate-files.txt" ||
        die "$crate_path does not contain keycloak-$version/$required"
done

# Unpack next to the cache so repeated runs do not pay for it again. A crate
# passed with `--crate` is unpacked into the work directory instead: the cache is
# keyed by version, so a local tarball must not overwrite what is cached for the
# same version.
if [ -n "$crate_file" ]; then
    src_root=$work_dir/src
else
    src_root=$cache_root/src
fi
src_dir=$src_root/keycloak-$version
if [ -n "$crate_file" ] || $refresh || [ ! -f "$src_dir/examples/openapi.rs" ]; then
    note "unpacking $(basename "$crate_path")"
    rm -rf "$src_root"
    mkdir -p "$src_root"
    tar -xzf "$crate_path" -C "$src_root" ||
        die "cannot unpack $(basename "$crate_path")"
fi

# Version equality between the tarball and this package: a tarball whose manifest
# disagrees with its name (or our version) would silently generate the wrong API.
unpacked_version=$(
    sed -n 's/^version = "\(.*\)"$/\1/p' "$src_dir/Cargo.toml" | head -n 1
)
[ "$unpacked_version" = "$version" ] ||
    die "keycloak crate version mismatch: tarball says '$unpacked_version', expected '$version'"
note "generating from keycloak $version (crate sources: $src_dir)"

# --- run the generator -----------------------------------------------------

# `tags cargo` is the generator subcommand whose output is not Rust code for the
# `keycloak` crate itself: it prints the crate's `[features]` block, which is the
# authoritative list of tag features, including the `tag-none` bucket.
(
    cd "$src_dir" &&
        CARGO_TARGET_DIR=$cache_root/target \
            cargo run --locked --quiet --example openapi -- tags cargo
) >"$work_dir/tags-cargo.txt" 2>"$work_dir/generator.log" ||
    {
        cat "$work_dir/generator.log" >&2
        die "the generator failed (log above); sources: $src_dir"
    }

# Structure of the output, asserted before it is turned into Rust: line 1 is the
# `tags-all` list, every following line is one `tag-… = []` feature, and the two
# agree. A generator change that breaks this must be noticed here instead of
# silently producing a wrong feature list.
tags_cargo=$work_dir/tags-cargo.txt
[ -s "$tags_cargo" ] || die "the generator printed nothing"
first_line=$(head -n 1 "$tags_cargo")
case $first_line in
'tags-all = ['*']') ;;
*) die "unexpected first line from the generator: $first_line" ;;
esac

sed -n 's/^\(tag-[a-z0-9-]*\) = \[\]$/\1/p' "$tags_cargo" >"$work_dir/tags-lines.txt"
output_lines=$(wc -l <"$tags_cargo" | tr -d ' ')
tag_lines=$(wc -l <"$work_dir/tags-lines.txt" | tr -d ' ')
[ "$output_lines" -eq $((tag_lines + 1)) ] ||
    die "unexpected line in the generator output; expected only 'tag-… = []' features"

from_all=$(
    printf '%s\n' "$first_line" |
        sed -e 's/^tags-all = \[//' -e 's/\]$//' |
        tr ',' '\n' |
        sed -e 's/^ *"//' -e 's/" *$//' -e '/^$/d'
)
[ "$from_all" = "$(cat "$work_dir/tags-lines.txt")" ] ||
    die "the tags-all list and the tag features disagree; generator output is inconsistent"

# --- command names ---------------------------------------------------------

# Before anything is emitted, the naming rules must cover every operation of the
# pinned description: one command per operation, with unique command words. A
# spec change that breaks the rules shows up here instead of producing a tree
# that clap cannot build (or worse, one that silently drops an operation).
(
    cd "$repo_root" &&
        cargo run --locked --quiet --bin keycloak-cli-generate -- \
            --spec "$src_dir/api/openapi.json" names
) >"$work_dir/commands.txt" 2>"$work_dir/commands.log" ||
    {
        cat "$work_dir/commands.log" >&2
        die "the naming rules do not cover every operation of keycloak $version"
    }
cat "$work_dir/commands.log" >&2

# --- emit the Rust sources -------------------------------------------------

stage=$work_dir/generated
mkdir -p "$stage"

{
    printf '//! Generated by `scripts/generate.sh` from `keycloak %s`; do not edit.\n' "$version"
    printf '//!\n'
    printf '//! Regenerate with `scripts/generate.sh` after bumping the version; `--check`\n'
    printf '//! proves that these sources match the published crate.\n\n'
    printf '/// Version of the `keycloak` crate these sources were generated from.\n'
    printf 'pub const KEYCLOAK_CRATE_VERSION: &str = "%s";\n\n' "$version"
    printf '/// Tags of the Keycloak Admin REST API, in the order of its description.\n'
    printf '///\n'
    printf '/// A tag `t` is the feature `tag-t` of the `keycloak` crate and the generated\n'
    printf '/// module `t`; `none` collects the operations that carry no tag (feature\n'
    printf '/// `tag-none`), and the crate feature `tags-all` enables every entry.\n'
    printf 'pub const TAGS: &[&str] = &[\n'
    sed 's/^tag-//' "$work_dir/tags-lines.txt" | while IFS= read -r tag; do
        printf '    "%s",\n' "$tag"
    done
    printf '];\n\n'
    printf '/// The `[features]` block of the `keycloak` crate, verbatim from\n'
    printf '/// `cargo run --example=openapi -- tags cargo` in the pinned crate.\n'
    printf 'pub const TAGS_CARGO: &str = r#"'
    cat "$tags_cargo"
    printf '"#;\n'
} >"$stage/tags.rs"

# The command modules and `mod.rs` come from our own generator, which reads the
# same description and the signatures of the crate's generated REST methods, so
# every command calls its method with the types the crate declares. It emits and
# validates every tag on the way; `$tags_to_emit` only decides which modules are
# written (the feature set of a build decides which ones are compiled).
note "emitting commands for tags: ${tags_to_emit:-all}"
emit_tags_args=()
if [ -n "${tags_to_emit:-}" ] && [ "${tags_to_emit}" != "all" ]; then
    emit_tags_args=(--tags "$tags_to_emit")
fi
(
    cd "$repo_root" &&
        cargo run --locked --quiet --bin keycloak-cli-generate -- \
            --spec "$src_dir/api/openapi.json" emit \
            --crate-dir "$src_dir" \
            --version "$version" \
            --out "$stage" \
            ${emit_tags_args[@]+"${emit_tags_args[@]}"}
) || die "cannot emit the command modules of keycloak $version"

# Formatting is part of generation: CI runs `cargo fmt --check`, and the rustfmt
# version is pinned by `rust-toolchain.toml`, which rustup resolves from the cwd.
if command -v rustfmt >/dev/null 2>&1; then
    (cd "$repo_root" && rustfmt --edition 2021 "$stage"/*.rs) ||
        die "generated sources do not parse; they are in $stage"
else
    note "warning: rustfmt not found, generated sources are not formatted"
fi

# --- compare or install ----------------------------------------------------

if $check; then
    if ! diff -ru "$output" "$stage"; then
        die "src/generated/ is out of date; run scripts/generate.sh and commit the result"
    fi
    note "src/generated/ is up to date with keycloak $version"
else
    mkdir -p "$output"
    # Everything below the output directory is generated, so replacing its
    # contents keeps it exactly in sync with the generator.
    find "$output" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
    cp -R "$stage"/. "$output"/
    note "wrote $(cd "$output" && find . -type f | sort | tr '\n' ' ')"
fi
