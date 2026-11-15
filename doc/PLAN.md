# Autogenerate keycloak-cli from the keycloak crate: full command tree, live shell completion, rich reports

"Done" means `rust-utility/keycloak-cli` is a generated CLI: its command tree is emitted from the same
generator that produces the `keycloak` crate (all 415 low-level operations, the same 22 `tag-*` features),
`scripts/generate.sh` reproduces it byte-for-byte from a version-pinned published crate, shell completion
suggests live realms/users/groups/clients from the session logged in earlier in that terminal, and listings
render as json/yaml/table/markdown/html with ASCII charts. Out of scope: implementing the Admin REST API, an
interactive TUI dashboard, full `kcadm.sh` flag parity, non-admin APIs, and crate publishing/release automation.

This plan lives in `rust-utility/keycloak-cli`. The `kilork/keycloak` checkout at `../../kilork/keycloak` stays the reference for the
crate's own code and method shapes, but the command-tree generator lives in `src/generate/` here: the newest
published `keycloak` crate is exactly the pinned `26.8.100`, so a generator subcommand added upstream could not
be used by `scripts/generate.sh` until a new release exists, while the script must reproduce the committed
sources from a published crate (F28).

## Research

- F1: `keycloak` 26.8.100 has 415 low-level REST methods over 23 generated files (31 untagged in `other_methods.rs`), 1125 `pub fn` in the fluent layer, and 119 component schemas rendered as 126 type items -- `rg -c 'pub async fn' src/rest/generated_rest/*.rs`, `cargo run --example=openapi -- tags kebab`, `python3 -c "…len(s['components']['schemas'])"` -- the CLI surface is too large to hand-write; codegen is mandatory.
- F2: the generator already holds tag, path, verb, parameter names/types/required, summary, description and return type in `struct RealmMethod` -- `examples/openapi.rs:52-63` -- a `cli` subcommand can emit clap trees without parsing the spec anywhere else.
- F3: features are `tags-all` (default) plus 22 `tag-*` including `tag-none`, and the generator prints them -- `Cargo.toml:16-51`, `cargo run --example=openapi -- tags cargo` -- the CLI can generate its own feature block from the same source, making tag parity checkable.
- F4: the published crate tarball ships the spec and the generator, and the generator runs from the unpacked crate -- `curl crates.io/api/v1/crates/keycloak/26.8.100/download | tar -tz`, then `cargo run --example=openapi -- tags kebab` inside it -- gives a version-pinned sync path with no cross-repo checkout.
- F5: every generator subcommand prints to stdout except `delete_mapping`, which rewrites `examples/openapi.patch.toml` relative to CWD -- `examples/openapi.rs:860-883` -- run it from a temp copy, not from a checkout where that self-heal matters.
- F6: `GET /admin/realms` appears as `KeycloakAdmin::get()`, not `realms_get()`, because the path has no parameters -- `src/rest/generated_rest/realms_admin.rs:17` -- command names must come from OpenAPI path + verb, never from the Rust method name.
- F7: spec path parameters are kebab-case (`user-id`, `client-uuid`, `client-scope-id`, `group-id`, `org-id`) and one is a Rust keyword (`type`); the generator snake-cases them into Rust parameters (`user_id`, `client_scope_id`) and sanitises the keyword -- `api/openapi.json` path parameters, `src/resource/users.rs:147`, `RESERVED_WORDS` at `examples/openapi.rs:74` -- generated flags should use the crate's snake_case names so flags, completers and docs agree.
- F8: clap's dynamic completion lives behind the `unstable-dynamic` feature; API is `CompleteEnv::with_factory(cli).complete()`, `ArgValueCompleter`, `ArgValueCandidates`, `SubcommandCandidates`, `CompletionCandidate` -- `docs.rs/clap_complete/latest/clap_complete/engine/`, `clap_complete/src/env/mod.rs` -- usable, but the shell↔binary interface is documented as unstable, so `init` must emit source-on-startup code.
- F9: clap 4.6.7 + clap_complete 4.6.11 with `unstable-dynamic` build on Rust 1.88 -- scratch crate `/tmp/clapcomp`, `cargo +1.88 build`; both declare MSRV 1.85 -- no MSRV blocker.
- F10: generated shell code bakes the absolute binary path and calls `COMPLETE=<shell> <bin> -- "${words[@]}"` with `_CLAP_COMPLETE_INDEX`, `_CLAP_COMPLETE_COMP_TYPE`, `_CLAP_COMPLETE_SPACE`, `_CLAP_IFS` -- `COMPLETE=bash|zsh /tmp/clapcomp/target/debug/clapcomp` -- completion breaks when the binary moves; document re-sourcing after upgrades.
- F11: the completion subprocess inherits exported environment: with `KEYCLOAK_CLI_SESSION=sess-42` a candidate came back as `session=sess-42` -- same scratch crate, `KEYCLOAK_CLI_SESSION=sess-42 COMPLETE=bash … -- clapcomp users ""` -- session state can be an exported variable set by `init`, no daemon needed.
- F12: `ArgValueCompleter::new` takes `Fn(&OsStr) -> Vec<CompletionCandidate>`: current value only, no `ArgMatches`, no sibling args -- `clap_complete/src/engine/custom.rs:47-90` -- the realm for `--user-id` completion must come from env/session or the raw argv words, not from clap's parse tree.
- F13: the dynamic engine implements Bash, Elvish, Fish, PowerShell and Zsh only; nushell has a separate generator (`clap_complete_nushell` 4.6.2) -- `clap_complete/src/env/shells.rs` -- promise dynamic completion for those five shells, treat nushell as static-only.
- F14: completion code is synchronous while the `keycloak` crate is async (`KeycloakAdmin::new(url, token_supplier, reqwest::Client)`, `KeycloakAdminToken::acquire(...)`) -- `src/rest/mod.rs:294,236` -- each completer needs a current-thread runtime and a hard timeout, since a process spawns per TAB.
- F15: live values come from paginated endpoints: `GET /admin/realms`, `/admin/realms/{realm}/users` (`max` defaults to 100, `briefRepresentation`), `/groups`, `/clients`, `/client-session-stats` -- `api/openapi.json` paths and parameter descriptions -- cap and cache candidates or TAB latency explodes.
- F16: `sudo` implements the "same terminal session" semantics already: `timestamp_type` tty/ppid/global, records keyed by tty device plus the session id of the authenticating process -- `man 5 sudoers`, timestamp_type section -- precedent for tty+session-keyed state with expiry instead of one global login file.
- F17: `ssh-agent` is the reference for session state passed through the environment: unix socket path in `SSH_AUTH_SOCK` -- `man 1 ssh-agent` -- an agent socket is a legitimate later upgrade; file+env is enough first.
- F18: Cobra-based tools (kubectl, gh) do runtime completion through a hidden `__complete` command -- `ShellCompRequestCmd = "__complete"`, `pkg.go.dev/github.com/spf13/cobra#ShellCompRequestCmd` -- independent prior art and a fallback protocol if clap's engine stays unstable.
- F19: `serde_yaml` is archived (last push 2024-03-25) and ships as `0.9.34+deprecated`; maintained forks are `serde_yaml_ng` 0.10.0 (repo active 2025-09-14) and `serde_norway` 0.9.42 (repo link 404s) -- crates.io and GitHub APIs -- use `serde_yaml_ng`, avoid `serde_yml`.
- F20: output stack MSRVs: `comfy-table` 8.0.1 (1.88), `tabled` 0.22, `termimad` 0.35.5 (1.56), `pulldown-cmark` 0.13.6 (1.71), `minijinja` 3.1.0 (1.70), `comrak` 0.59.0 needs **1.89** -- crates.io `rust_version` fields -- comfy-table + pulldown-cmark + minijinja + termimad fit; comrak is excluded.
- F21: ASCII charting options: `textplots` 0.8.7 (line/scatter, last release 2025-02), `sparklines` 0.3.0 (2025-05, `ekroon/sparklines`), `ratatui` 0.30.2 (1.88, TUI widgets); `asciibar` 0.1.3 is a CLI binary, not a library -- crates.io API -- use `sparklines`/`textplots` or in-house Unicode bars; ratatui only for a future interactive view.
- F22: `std::io::IsTerminal` covers `--output auto`; verified compiling on 1.88 -- `/tmp/tty_check.rs`, `rustc +1.88` -- no `is-terminal` dependency.
- F23: `completest` 1.1.0 (MSRV 1.85) asserts completion output -- crates.io -- completion can be unit-tested instead of hand-checked in shells.
- F24: keycloak-cli today is a skeleton: `info`, 8 unit tests, CI green, 26.8.100↔Keycloak 26.8.1, no `keycloak` dependency -- `rust-utility/keycloak-cli@e2ccedd`, run 43735829127 -- M0 is done; every other milestone starts from a compiling crate.
- F25: the wrapped crate pins 1.88 (`rust-version`, CI `dtolnay/rust-toolchain@1.88.0`) -- `Cargo.toml`, `.github/workflows/integration.yml` -- the CLI and its dependencies must stay ≤ 1.88 (see F20's comrak exception).
- F26: report data exists without extra APIs: `/admin/realms/{realm}/events`, `/users/count`, `/client-session-stats`, `/groups` -- `api/openapi.json` -- the charts/report milestone can use real endpoints.
- F27: the pinned spec has 274 paths and 415 operations; 272 paths start with `/admin/realms/{realm}` and the only exceptions are `GET /admin/realms` and `POST /admin/realms`, so command names come from the tail after that prefix, and the realm is better bound to the existing global `-r/--realm` than to a positional -- `python3` over `api/openapi.json` in `keycloak-26.8.100.crate` (round 1).
- F28: `keycloak` 26.8.100 is the newest published crate, so generator code added to the `kilork/keycloak` checkout cannot be used by `scripts/generate.sh` until a release is cut -- the CLI must own the emitter and read `api/openapi.json` from the unpacked tarball; `crates.io/api/v1/crates/keycloak` `newest_version` plus `CARGO_PKG_VERSION` (round 2).
- F29: the naming rules (group = first literal segment, verb + collection/singular target = action, `{realm}` bound to the global flag) give exactly 415 unique commands in 36 groups; only 6 word-sequences are shared by different arities (`localization get`, `clients authz resource-server resource get`, `organizations members organizations list`, …), and spelling the parameters of those 13 operations as words makes the tree unambiguous -- `cargo run --bin keycloak-cli-generate -- names` over the pinned spec (round 2).

## Milestones

- [x] M0: `rust-utility/keycloak-cli` exists with a skeleton CLI, the 26.8.100↔Keycloak 26.8.1 scheme and green CI
- [ ] M1: every operation of the pinned `keycloak` version (415, including the 31 untagged) appears as a generated command that dispatches to its crate method — no stubs — and `scripts/generate.sh` reproduces the committed sources byte-for-byte
- [ ] M2: default build ships all tags; `--no-default-features --features tag-users` builds and `--help` lists only user commands
- [ ] M3: `keycloak-cli init bash|zsh|fish` prints shell code; after evaluating it, TAB completes commands and flags
- [ ] M4: after `login` in a terminal, TAB there offers live realms/users/groups/clients; a new terminal offers none until it logs in
- [ ] M5: live completion is bounded: cold TAB ≤ ~1.5 s, warm (within TTL) ≤ ~100 ms, failures yield no candidates and no stdout noise
- [ ] M6: listing commands render json (piped default), yaml, table, markdown and html via `--output`, with `auto` choosing table on a TTY
- [ ] M7: `keycloak-cli report` writes a markdown and a self-contained html report with ASCII charts for users, events and sessions
- [ ] M8: introducing generation drift, a tag-feature mismatch with the `keycloak` crate, or a completion regression makes CI fail

## Round plan

- [x] Q1 (high impact, low effort) — `scripts/generate.sh` downloads `keycloak-<version>.crate`, unpacks it to a temp dir and runs the generator, starting with `tags cargo` into `src/generated/tags.rs`; version equality is asserted (F4)
- [ ] Q2 (high impact, high effort) — `keycloak-cli-generate` (fed by `api/openapi.json` from the pinned crate) emits clap modules for one tag (`clients`): names from path+verb, path params as positionals, query params as flags, body as `--body <file|->`
  - [x] naming and collision rules plus unit tests for `/admin/realms` vs `/admin/realms/{realm}` (F6, F7) — `src/generate/naming.rs` + `src/generate/spec.rs`, 12 unit tests, and the `names` self-check over all 415 operations (F29)
  - [ ] emitted `clients` module compiles in keycloak-cli after adding `keycloak` with `default-features = false, features = ["reqwest"]` and a `tag-clients` feature
- [ ] Q3 (high impact, medium effort) — all remaining tags emitted behind `#[cfg(feature = "tag-…")]` with `tag-none`, and CLI features mirroring `tags cargo` (F3, F25)
  - [ ] emit the remaining tag modules and the `tag-none` bucket, feature block generated from `tags cargo`
  - [x] CI drift job: `scripts/generate.sh` regenerates from the pinned crate and fails on diff (F4, F5) — wired in round 1 for the files the script emits today; `src/generated/tags.rs` is covered now and every module the script learns to emit is covered automatically
- [ ] Q4 (high impact, medium effort) — generated commands call the API: token from the existing flag/env config, shared tokio runtime, per-command dispatch (a session login replaces the token source in Q6) (F14, F24)
  - [ ] runtime plus `KeycloakAdmin` construction from config; one generated command is asserted against a loopback HTTP stub (method, path, query), since the crate builds and sends requests internally with no render hook
  - [ ] dispatch and `KeycloakError` → exit-code mapping, tested with a stub token supplier
- [ ] Q5 (high impact, medium effort) — `init bash|zsh|fish` emits source-on-startup code using `CompleteEnv`; `build_cli()` shared by parse and completion; `completest` covers the emitted completions; upgrade/re-source documented (F8, F10, F13, F23)
- [ ] Q6 (medium impact, medium effort) — session state: `login` writes a 0600 state file keyed by tty+session with expiry, `init` exports `KEYCLOAK_CLI_SESSION`, `logout` removes it (F11, F16)
- [ ] Q7 (high impact, high effort) — live completion registry: parameter name → value source (realm, user_id, group_id, client_uuid, role_id…), per-session TTL cache, timeout, silent failure, kill switch (F12, F15)
  - [ ] registry, cache and timeout plumbing, unit-tested against a fake source
  - [ ] API wiring for `--realm`, `--user-id`, `--group-id`, `--client-uuid`
- [ ] Q8 (high impact, medium effort) — output layer: `Report` value, five renderers, TTY autodetect and `--fields` projection (F19, F20, F22)
  - [ ] `Report` plus json/yaml/table renderers (comfy-table) with `--output auto` TTY detection
  - [ ] markdown (pulldown-cmark/termimad) and html (minijinja) renderers plus `--fields`
- [ ] Q9 (medium impact, medium effort) — ASCII visualisations (Unicode bars, `sparklines`/`textplots` series) and a `report` command composing users/events/session stats (F21, F26)
- [ ] Q10 (low impact, low effort) — docs and scope alignment: README usage, ROADMAP update, `doc/COMPLETION.md` covering session semantics and security, tag parity test (F3, F24)

## Checks

- cargo test --all
- ./scripts/generate.sh --check
- cd ../../kilork/keycloak && cargo test --lib
