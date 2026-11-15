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

## Round 2 — Q2 (first chunk): naming and collision rules

Goal: naming/collision rules plus unit tests for `/admin/realms` vs
`/admin/realms/{realm}` (F6, F7). **Done, and the rules are validated against all
415 operations of the pinned spec.**

### Decision (asked and confirmed)

The generator **lives in `keycloak-cli`**, not in `kilork/keycloak`: the newest
published `keycloak` crate is exactly the pinned `26.8.100` (crates.io
`newest_version`), so a `cli` subcommand added to the checkout could not be used
by `scripts/generate.sh` until a new crate release exists, and M1 requires the
committed sources to be reproducible from a *published* crate (F28). The script
therefore feeds our generator with `api/openapi.json` from the unpacked tarball.

Path parameters: **`{realm}` binds to the existing global `-r/--realm`**; every
deeper path parameter is a positional in path order (F27, F29).

### What exists now

- `src/generate/naming.rs` — the rules, documented in the module header:
  group = first literal segment after the realm, literals are words,
  `{realm}` is never a positional, action = method × target, where a target is
  single when the path ends with a parameter **or** its last literal does not end
  with `s` (so `…/client-secret` gets `get`, `…/clients` gets `list`), and
  `make_unique` spells the parameters of ambiguous groups as words
  (`localization locale key get`) until every command path is unique, reporting
  the operations instead of renaming silently when it cannot. Also
  `ParamName` (kebab + snake spellings) and `ParamName::flag`, which prefixes a
  flag that would shadow a global one with `param-` (`--param-user` on events).
- `src/generate/spec.rs` — the OpenAPI subset the generator needs (paths, five
  methods, tags, parameters with path/query location, request bodies), path-level
  parameters merged into operation-level ones in place, deterministic operation
  order; 3 unit tests.
- `src/bin/keycloak-cli-generate.rs` — internal generator; `names` derives a
  command per operation, makes the names unique and prints `usage ← method path`.
- `scripts/generate.sh` now runs that `names` step against the unpacked crate
  before emitting: a spec the rules cannot cover fails generation.
- `Cargo.toml`: `heck`, `serde`, `serde_json` added (Cargo.lock updated).

### Verified

- `cargo test --all`: **25 tests** green (22 unit + 3 integration, up from 11).
- `cargo run --bin keycloak-cli-generate -- --spec <crate>/api/openapi.json names`
  → `415 operations, 415 commands in 36 groups, 13 spelled with their
  parameters`, exit 0; the tree contains `realms list|create` (from
  `GET|POST /admin/realms`) and `realms get|update|delete` (from
  `/admin/realms/{realm}`) with no positional, `clients list|create`,
  `clients get <client-uuid>`, `clients client-secret get <client-uuid>`,
  `clients roles composites list <client-uuid> <role-name>`.
- `cargo fmt --all -- --check` and `cargo clippy --all-targets -D warnings` clean.
- `scripts/generate.sh --check` → naming step + "up to date with keycloak
  26.8.100", exit 0.

### Known nit / next steps

- Shaping is applied per collision group only, so one sub-path can be spelled two
  ways: `localization create <locale>` next to `localization locale get
  <locale>`. Acceptable for a generated tree; revisit with curated aliases later.
- Next (rest of Q2): emit the `clients` module — query flags with types from the
  parameter schemas, positional value names/help from the descriptions,
  `--body <file|->` from `requestBody`, and the dispatch expression to the
  `keycloak` crate method; then add `keycloak` with `default-features = false,
  features = ["reqwest"]` plus `tag-clients`, and compare with the method names
  the crate actually exposes (`docs.rs/keycloak` / the unpacked
  `src/rest/generated_rest/*.rs`).

## Round 3 — Q2 (second chunk): the emitted `clients` module

Goal: emitted `clients` module compiles in keycloak-cli after adding `keycloak`
with `default-features = false, features = ["reqwest"]` and a `tag-clients`
feature. **Done — and the emitter turned out to cover all 22 tags, so Q3 is now
selection + features, not more codegen.**

### What exists now

- `src/generate/signature.rs` — reads `src/rest/generated_rest/*.rs` from the
  unpacked crate (textual, no `syn`): method name, parameters with their Rust
  types, and the `Result<T, KeycloakError>` payload. `method_name(path, method)`
  is a port of the crate generator's rule (`/admin/realms` → `get`,
  `{client-uuid}` → `with_client_uuid`, CamelCase verb, snake_case), including
  the `type` → `type_` keyword suffix. 5 unit tests.
- `src/generate/emit.rs` — emits one module per tag: `COMMANDS: &[CommandSpec]`
  (words, about/long_about, positionals, flags with `ValueKind`, body,
  deprecated), `owns(words)` and an `async fn dispatch` whose `match words`
  calls the crate method with typed arguments (`&str` positionals bound first,
  `Option<…>` read through helpers, body deserialised into the crate's own type).
  `DefaultResponse` operations return `Value::Null`. It also emits `mod.rs`
  (cfg-gated modules, `commands()`, `owns()`, `dispatch()`). It validates *every*
  tag on every run — a description the rules cannot cover fails generation even
  if that tag is not selected.
- `src/command.rs` — the runtime: `CommandSpec`/`ParamSpec`/`ValueKind`, the
  clap tree builder (nested, merged across tags), panic-free value readers
  (`text`/`boolean`/`integer`/`texts`/`positional`), `--body FILE|-` reading, and
  `to_json`. 5 unit tests.
- `src/generated/tag_clients.rs` (committed, 35 commands) + regenerated
  `src/generated/mod.rs`.
- `Cargo.toml`: `keycloak = { version = "=26.8.100", default-features = false,
  features = ["reqwest"] }`, `[features] default = ["tag-clients"]`,
  `tag-clients = ["keycloak/tag-clients"]`.
- `scripts/generate.sh` now runs `keycloak-cli-generate emit --crate-dir … --out …`
  (tags from `KEYCLOAK_CLI_GENERATE_TAGS`, default `clients`), rustfmts every
  staged file, and still compares byte-for-byte in `--check`.
- `tests/commands.rs` (5 tests): all 35 commands are reachable in the clap tree,
  words unique within the tag, flags/positionals carry the API names and help,
  `clients create` has `--body`, and a real command line parses back into values.

### Verified

- `cargo run --bin keycloak-cli-generate -- --spec … emit --crate-dir …` →
  `415 operations, 415 commands (13 spelled with their parameters), 415 crate
  methods, 1 modules` — every operation of the pinned crate finds its method.
- `cargo build` with the generated module: compiles (the dispatch calls
  type-check against `keycloak` 26.8.100, incl. `ClientRepresentation` bodies and
  `DefaultResponse` units).
- `cargo build --no-default-features`: compiles with zero warnings (the
  generated `dispatch` allows unused arguments when no tag is enabled).
- `cargo test --all`: **43 tests** (35 unit + 5 commands + 3 generated).
- `cargo fmt --all -- --check`, `cargo clippy --all-targets -D warnings` clean
  (also on the generated code), `cargo package --locked` verifies.
- `scripts/generate.sh --check` → up to date; two consecutive runs produce
  identical hashes for every file in `src/generated/`.

### Added in the same round: Q3 (every tag)

The emitter was tag-agnostic, so Q3 became selection and features rather than new
codegen:

- `scripts/generate.sh` emits **all 22 tags by default** now
  (`KEYCLOAK_CLI_GENERATE_TAGS=clients,users` narrows it; `tags_to_emit` in the
  script); `src/generated/` holds `mod.rs` + 22 `tag_*.rs` + `tags.rs` (524 KB,
  415 commands) and the whole crate compiles with **zero warnings**.
- `Cargo.toml` has the 22 `tag-*` features (`tag-x = ["keycloak/tag-x"]`),
  `tags-all` enabling all of them, and `default = ["tags-all"]`.
- Generated tag modules allow `unused_variables`/`deprecated`: a tag without
  arguments and the 11 deprecated operations are properties of the API, not
  warnings a build should fail on.
- `tests/commands.rs` gained a `tags-all` module: 415 specs, 36 groups, every
  command reachable, the 31 untagged operations present (`tag_none`), and the
  shared-word pairs spelled apart (`localization get` vs `localization locale
  get` vs `localization locale key get`).
- `tests/generated.rs` gained a manifest parity test: the `tag-*` features of
  `Cargo.toml` must equal `TAGS`, each must enable `keycloak/tag-*`, `tags-all`
  must list them all and be the default (M8: a tag mismatch fails CI).

Verified: `cargo build` (all tags, 0 warnings), `cargo build
--no-default-features`, `cargo build --no-default-features --features tag-users`,
`cargo test --all` (**48 tests**: 35 unit + 9 commands + 4 generated), `cargo
clippy --all-targets -D warnings` (also `--no-default-features --features
tag-users`), `cargo fmt --check`, `scripts/generate.sh --check`, and a deliberate
type error in a generated module proved the modules are really compiled.


### Added in the same round: Q5 (dynamic completion)

- `src/completion.rs` — `complete()` calls `CompleteEnv::with_factory(build_cli)`
  (the same tree as parsing, which is why the completion of `clients`/`--first`
  matches the build), `init(shell, program)` prints the line for each shell, and
  `SHELLS`/`shell(name)` back the `init` argument.
- `keycloak-cli init <bash|zsh|fish|elvish|powershell>` prints that line with the
  absolute binary path; `main` calls `complete()` before anything can write to
  stdout (a shell parses everything the process prints as the answer).
- Bash uses `eval "$(COMPLETE=bash …)"` rather than clap's `source <(…)`: on
  macOS bash 3.2.57 `source <(…)` defines the completion function in a subshell,
  so TAB silently does nothing. zsh keeps `source <(…)`.
- `tests/completion.rs` (8 tests) drives the real binary with the shell's protocol
  (`COMPLETE`, `_CLAP_COMPLETE_INDEX`, `_CLAP_IFS`): subcommands of a group, flags
  of a command (including the global connection flags), top-level groups, the
  shells of `init`, the registration script, the `init` lines and the exit code of
  an unknown shell; a positional currently falls back to flags, which is where Q7
  plugs live values in.
- README gained a "Shell completion" section with the per-shell lines and the
  re-source-after-upgrade note (the shell↔binary protocol is unstable by clap's
  own documentation, F10).
- Verified by hand: `eval "$(keycloak-cli init bash)"` in bash 3.2.57, then the
  registered function answers `clients` with its subcommands and `--sea` with
  `--search`; zsh registers `_clap_dynamic_completer_keycloak_cli`; no fish,
  elvish or powershell is installed here, so those are covered by their printed
  lines only.

### Notes for the next round (Q6/Q7)

- Completion exists but has no live values: `tests/completion.rs` pins the current
  fallback (flags at a positional). Q6 adds the session (`login` writes a 0600
  state file keyed by tty+session, `init` exports `KEYCLOAK_CLI_SESSION`), Q7 the
  registry param → value source with a TTL cache, timeout and silent failure.
- The dispatch is generic over `KeycloakTokenSupplier` and `client::connect` is the
  only place that builds a token, so a session supplier can replace
  `KeycloakAdminToken::acquire` without touching generated code (F11, F14).
- Arg ids in `CommandSpec` are the crate's snake_case names (`client_uuid`,
  `realm`, …), which is the key a completion registry should use (F12, F15).
- Output is still pretty JSON; Q8's `--output` replaces `lib.rs::write_value`.

## Loop log
- round 1 · Q1 · 41 turns · continue → continue · scripts/generate.sh generates from the published keycloak…
- round 2 · Q2 · 32 turns · continue → continue · naming and collision rules for the generated tree, with u…
- round 3 · Q2 · 73 turns · continue → continue · emitted clients module compiles against the real crate — …
