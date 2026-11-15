# Roadmap

`keycloak-cli` wraps the [Keycloak](https://www.keycloak.org) Admin REST API as a
command line tool, built on the [`keycloak`](https://crates.io/crates/keycloak)
crate. This file is the source of truth for planned scope; milestones are
deliberately outcome-based and can be reordered.

Current state: **M0 skeleton** — CLI shape, configuration, version scheme, CI. No
API calls yet.

## Principles

- **Scriptable first.** Stable machine-readable output, meaningful exit codes, no
  prompts unless a command is destructive, so the tool composes in shell and CI.
- **Thin over the crate.** REST coverage comes from the generated `keycloak` crate;
  the CLI adds ergonomics, never its own HTTP layer or a second resource model.
- **Safe by default.** Destructive commands need `--yes`/`--force`; `--dry-run`
  prints the request that would be sent; no secret is ever echoed
  (`password`, `secret`, tokens are redacted everywhere).
- **Lockstep versions.** `x.y.(z * 100 + v)` mirrors the wrapped Keycloak `x.y.z`
  and the `keycloak` crate release it is built against.

## M1 — Authentication and contexts

- [ ] Token acquisition through the `keycloak` crate: password grant and client
      credentials (`--client-id/--client-secret`), admin token
- [ ] Contexts (profiles) in `$XDG_CONFIG_HOME/keycloak-cli/config.toml` with
      `--context <name>`, so `kc prod users list` does not need five env vars
- [ ] Token cache with expiry handling, `--token` for a pre-issued token
- [ ] `keycloak-cli login` / `logout` (store or clear credentials per context)
- [ ] Connection diagnostics: `doctor` (reachability, version, granted roles)
- [ ] Documented exit codes (`0` ok, `1` error, `2` usage, …)

## M2 — Read paths

- [ ] `realms list|get`, `realms keys`
- [ ] `users list|get|search` with `--first/--max/--all` pagination and
      `--brief`, plus `users sessions|consents|credentials`
- [ ] `groups list|tree|get`, `groups members`
- [ ] `clients list|get`, `clients scopes|protocol-mappers`
- [ ] `roles list|get`, `roles composites|members`
- [ ] `sessions list`, `events list` (`--type`, `--date-from`, `--date-to`)
- [ ] Output layer: `--output json|yaml|table|tsv`, `--fields` projection,
      `--jq`-friendly (JSON is the default when stdout is not a TTY)
- [ ] A global `--quiet` and `--verbose` (log to stderr, data on stdout)

## M3 — Write paths

- [ ] `create|update|delete` for realms, users, groups, clients, roles
- [ ] `--from-file -` (stdin) and `-` conventions for JSON payloads
- [ ] `--dry-run` everywhere a mutation happens, plus `--yes` for destructive ones
- [ ] User management shortcuts: `users reset-password`,
      `users add-role|remove-role`, `users join-group|leave-group`,
      `users logout`, `users delete --disable-first`
- [ ] Client management: `clients secret rotate`, certificates, `clients enable`
- [ ] Realm role mappings and scope mappings as first-class commands

## M4 — Realm configuration and import/export

- [ ] `export` (`--users`, `--groups`, `--clients`), `partial-export`
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

- [ ] Shell completions (bash, zsh, fish), man page
- [ ] Coloured tables with `--color auto|always|never`, no colour when piped
- [ ] `keycloak-cli help topics` for common recipes (CI service account, migrate
      realm between servers, find users without MFA)
- [ ] GitHub release artefacts (linux/macos, static where possible) next to
      `cargo install` and (maybe) Homebrew
- [ ] crates.io trusted publishing (workflow in place and gated, see
      `doc/PUBLISHING.md`)

## Non-goals

- Replacing the Keycloak admin console or the official `kcadm.sh`; the CLI is for
  scripted, repeatable operations, not for exhaustive UI parity.
- Running or configuring Keycloak itself (containers, systemd, Helm).
- Embedding business logic beyond what the Admin REST API exposes.

## Open questions

- Binary name: keep `keycloak-cli`, or ship a short alias (`kc` is taken by other
  tools and by `kcadm` habits, so probably not)?
- Feature gating of the wrapped crate: default features
  (`tags-all`, `resource-builder`, `reqwest`) for installs, or a smaller tag set
  to keep build times low and grow by request?
- Contexts file format and whether secrets may live in it at all (keyring
  integration?), or stay environment/token-file only.
- Whether `--dry-run` should print the curl equivalent, the JSON payload, or both.
