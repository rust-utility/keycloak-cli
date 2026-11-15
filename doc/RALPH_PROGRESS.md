# Progress

## Round 1 — Q1: generation from the published crate

Goal: `scripts/generate.sh` downloads `keycloak-<version>.crate`, unpacks it and
runs the generator, starting with `tags cargo` into `src/generated/tags.rs`, with
a version-equality assertion (F4). **Done, plus the drift check wired into CI.**

### What exists now

- `scripts/generate.sh` (executable, ~330 lines, bash 3.2 compatible):
  - version = the package version from `Cargo.toml` (no second source of truth);
    asserted to be `x.y.z` shaped;
  - downloads `https://crates.io/api/v1/crates/keycloak/<version>/download` into
    `${KEYCLOAK_CLI_GENERATE_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/keycloak-cli/generate}/download`,
    unpacks into `…/src/keycloak-<version>` and builds with a shared
    `…/target`, so a warm run is ~0.5 s and a cold run ~15 s (this machine);
  - asserts the tarball contains `Cargo.toml`, `api/openapi.json`,
    `examples/openapi.rs`, `examples/openapi.patch.toml` (F4);
  - **version equality**: `version` in the unpacked `Cargo.toml` must equal the
    package version, otherwise it dies with
    `keycloak crate version mismatch: tarball says '26.8.99', expected '26.8.100'`;
  - runs `cargo run --locked --quiet --example openapi -- tags cargo` inside the
    unpacked crate (stdout = features, stderr = the crate's deprecation warnings,
    kept out of the generated file);
  - asserts the generator output's shape *before* transforming it: line 1 is
    `tags-all = [...]`, every other line is `tag-… = []`, and the two lists agree;
  - emits `src/generated/tags.rs` (`KEYCLOAK_CRATE_VERSION`, `TAGS` in spec order
    with `none` last, `TAGS_CARGO` verbatim) and `src/generated/mod.rs`;
  - formats the output with `rustfmt` from the cwd, i.e. the toolchain pinned in
    `rust-toolchain.toml` (CI runs `cargo fmt --check`, so this is required);
  - `--check` regenerates into a temp dir and `diff -ru`s against
    `src/generated`, exiting 1 on drift; `--refresh` drops the cache; `--crate
    FILE` uses a local tarball (unpacked into the temp dir, never into the
    version-keyed cache, so a local tarball cannot poison the cache);
  - no timestamps anywhere, so output is byte-identical across runs.
- `src/generated/{mod.rs,tags.rs}` committed; `pub mod generated;` in
  `src/lib.rs`.
- `tests/generated.rs` (3 tests): generated version == `CARGO_PKG_VERSION`;
  tags are unique/kebab-case/`none`-last; `TAGS_CARGO` parses to exactly
  `TAGS` with the `tag-` prefix.
- `.github/workflows/ci.yml`: step `Generated sources are up to date` running
  `scripts/generate.sh --check` (this also covers the Q3 drift sub-item: any tag
  module the script learns to emit is covered automatically).

### Verified

- `cargo test --all` → 11 tests green (8 old + 3 new); `cargo fmt --all --
  --check` and `cargo clippy --all-targets -- -D warnings` clean.
- Cold run with an empty cache: download + unpack + build + emit in 15 s,
  `git status` clean afterwards (byte-identical to the committed files).
- Warm `--check`: "up to date", 0.5 s. Default cache path (`~/.cache/…`) also
  exercised, 14.5 s cold.
- Drift detection: appended `// drift` to `src/generated/tags.rs` → `--check`
  printed the diff and exited 1; restored → exit 0.
- Version assertion: rebuilt the tarball with `version = "26.8.99"` inside →
  mismatch error, exit 1. Bumped package version to `26.8.101` in a copy →
  `curl: (56) 404` and `cannot download …`, exit 1.
- CLI surface: `--help`, unknown argument, missing option value all behave.

### Notes for the next round (Q2)

- Path inventory of the pinned spec (verified): **274 paths / 415 operations**;
  272 paths start with `/admin/realms/{realm}`, and the two exceptions are
  `GET /admin/realms` and `POST /admin/realms` (tag `Realms Admin`) — exactly the
  collision Q2 must resolve (`realms list|create` vs `realms get|update|delete`).
- Spec tag names are Title Case (`Realms Admin`, `Client Role Mappings`); the
  kebab form the crate uses comes from `heck::ToKebabCase`, and `tags cargo` adds
  `tag-none`. Generated modules must therefore be `realms_admin`, `tag-realms-admin`.
- 21 tags + `none` = 22 feature names, matching F3.
- Open design questions for Q2's naming module (decide before emitting clap):
  1. realm path parameter: bind to the existing global `-r/--realm` flag, or make
     it a positional on `realms get|update|delete`? Binding it to the flag is
     consistent with the skeleton and avoids `realms get <realm> --realm <realm>`;
     `/admin/realms` then needs no positional at all.
  2. action naming: the verb is only a fallback — `/users/{user-id}/logout`,
     `/clients/{client-uuid}/push-revocation` are POSTs whose last segment is a
     name, not a parameter; the rule "last segment is a parameter → get/update/
     delete, otherwise the last segment names the action" needs a disambiguator
     when two methods share such a leaf (`GET+POST /clients/{uuid}/client-secret`).
  3. path parameters other than realm become positionals in path order
     (`clients protocol-mappers models create <client-uuid> <id>`); flags keep
     the crate's snake_case names (F7) for the query parameters.
- Next concrete step: implement the naming/collision functions in
  `kilork/keycloak`'s `examples/openapi.rs` with unit tests over the *whole* spec
  (uniqueness assertion for all 415 operations), then the `cli` emitting
  subcommand for the `clients` tag.
