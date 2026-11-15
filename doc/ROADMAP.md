# Roadmap

`keycloak-cli` wraps the [Keycloak](https://www.keycloak.org) Admin REST API as a
command line tool, built on the [`keycloak`](https://crates.io/crates/keycloak)
crate. This file is the source of truth for planned scope; milestones are
deliberately outcome-based and can be reordered.

Current state: **generated command tree** — every operation of the pinned
`keycloak` crate is a command (415 in 36 groups, 22 `tag-*` features), each one
calling its crate method; terminals log in once (`login`/`logout`), completion is
dynamic and offers the session's realms/users/groups/clients, and responses render
as JSON, YAML, a table, markdown or HTML. Still ahead: the `report` document, write
path safety (`--dry-run`/`--yes`), contexts and release artefacts.

The items below are the product view; the code is generated from the API description
(see `src/generate/` and `scripts/generate.sh`), so "coverage" means the wrapped
crate exposes it.

## Principles

- **Scriptable first.** Stable machine-readable output, meaningful exit codes, no
  prompts unless a command is destructive, so the tool composes in shell and CI.
- **Thin over the crate.** REST coverage comes from the generated `keycloak` crate;
  the CLI adds ergonomics, never its own HTTP layer or a second resource model.
- **Safe by default.** Destructive commands (`… delete`) refuse to run without
  `--yes`; `--dry-run` prints the request that would be sent and sends nothing; no
  secret is ever echoed (`password`, `secret`, tokens are redacted everywhere).
- **Lockstep versions.** `x.y.(z * 100 + v)` mirrors the wrapped Keycloak `x.y.z`
  and the `keycloak` crate release it is built against.

## M1 — Authentication and contexts

- [x] Token acquisition through the `keycloak` crate: password grant and admin token
- [ ] Client credentials (`--client-id/--client-secret`) as an alternative login
- [ ] Contexts (profiles) in `$XDG_CONFIG_HOME/keycloak-cli/config.toml` with
      `--context <name>`, so `kc prod users list` does not need five env vars
- [x] Token cache with expiry: the terminal's session (`login`) is stored with the
      token's expiry and reused until it lapses
- [ ] `--token` for a pre-issued token
- [x] `keycloak-cli login` / `logout` — one session per terminal, not per context
- [ ] Connection diagnostics: `doctor` (reachability, version, granted roles)
- [x] Documented exit codes (`0` ok, `1` error, `2` usage, `3` rejected
      credentials, `4` not found)

## M2 — Read paths

- [x] Every read operation of the Admin API, as generated commands named after the
      path and verb (`realms list`, `users list|get`, `groups list|get`,
      `clients list|get`, `roles list|get`, `events list`, `sessions`…)
- [ ] `--all` pagination (the generated `--first`/`--max` are per request)
- [x] Output layer: `--output json|yaml|table|markdown|html` with `auto` (table on a
      terminal, JSON when piped) and `--fields` projection
- [x] A global `--quiet` and `--verbose` (log to stderr, data on stdout) — `--verbose` prints the request line before sending it, `--quiet` drops informational notes; data always goes to stdout

## M3 — Write paths

- [x] `create|update|delete` for every resource the API exposes, as generated
      commands, with `--body <FILE|->` for the payload
- [x] `--dry-run` everywhere a mutation happens, plus `--yes` for destructive ones — `--dry-run` prints method, URL (with query, positionals and encoded values) and body from the generated description, and every `DELETE` command requires `--yes` (dry runs excepted)
- [x] User management shortcuts — available today as generated commands: `users reset-password update <user-id>`, `users credentials …`, `users groups …`, `users logout`, `users delete --yes`; curated aliases (shorter names, `--all` pagination) are still open. The original list: `users reset-password`,
      `users add-role|remove-role`, `users join-group|leave-group`,
      `users logout`, `users delete --disable-first`
- [x] Client management — generated as `clients client-secret create|rotated`, `clients certificates …`, `clients update` (enable/disable is the `enabled` field); curated aliases remain open
- [x] Realm role mappings and scope mappings — the generated tree has `… role-mappings …` and `… scope-mappings …` groups for users, groups and clients

## M4 — Realm configuration and import/export

- [x] `partial-export` and `partial-import` — generated commands (`partial-export create`, `partial-import create --body …`); curated `export`/`import` aliases with per-resource flags are still open
- [ ] `import` (multipart) and `partial-import` with `--strategy`
- [ ] `diff <file> --against <realm>`: show what an import would change
- [ ] Protect against footguns: refuse to import a realm file over an existing
      realm without `--overwrite`

## M5 — Bulk operations and pipelines

- [ ] NDJSON streaming input (`--input -`), `--continue-on-error`, `--parallel N`
- [ ] `users import-csv` (Keycloak-compatible columns), `users export`
- [ ] Resource-builder based bulk creation (resource-builder feature of the crate)
- [ ] Idempotent apply mode for groups/roles/clients (`ensure` style commands)

## M6 — Ergonomics and distribution

- [x] Shell completions (bash, zsh, fish, elvish, powershell), dynamic and aware of
      the session's values — see `doc/COMPLETION.md`; a man page is still missing
- [x] `keycloak-cli report` — a markdown or self-contained HTML document with ASCII
      charts for users, events by type and sessions per client
- [ ] Coloured tables with `--color auto|always|never`, no colour when piped
- [ ] `keycloak-cli help topics` for common recipes (CI service account, migrate
      realm between servers, find users without MFA)
- [ ] GitHub release artefacts (linux/macos, static where possible) next to
      `cargo install` and (maybe) Homebrew
- [x] crates.io trusted publishing — `keycloak-cli` 26.8.100 is published from the
      `publish` workflow with OIDC (see `doc/PUBLISHING.md`)

## Non-goals

- Replacing the Keycloak admin console or the official `kcadm.sh`; the CLI is for
  scripted, repeatable operations, not for exhaustive UI parity.
- Running or configuring Keycloak itself (containers, systemd, Helm).
- Embedding business logic beyond what the Admin REST API exposes.

## Open questions

- Binary name: keep `keycloak-cli`, or ship a short alias (`kc` is taken by other
  tools and by `kcadm` habits, so probably not)?
- Feature gating of the wrapped crate: answered for now — `tags-all` is the default
  and `--no-default-features --features tag-users` builds a smaller CLI; whether to
  ship such slim builds is a release question.
- Contexts file format and whether secrets may live in it at all (keyring
  integration?), or stay environment/token-file only.
- Whether `--dry-run` should print the curl equivalent, the JSON payload, or both.
