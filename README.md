# keycloak-cli

Command-line interface for the [Keycloak](https://www.keycloak.org) Admin REST API,
built on the [`keycloak`](https://crates.io/crates/keycloak) crate.

[![crates.io](https://img.shields.io/crates/v/keycloak-cli.svg)](https://crates.io/crates/keycloak-cli)
[![ci](https://github.com/rust-utility/keycloak-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/rust-utility/keycloak-cli/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Unlicense%20OR%20MIT-blue.svg)](#legal)

Notes and background: **<https://kilork.github.io/keycloak-cli/>**

Every operation of the Admin REST API is a command — 415 of them, in 36 groups,
generated from the crate this CLI wraps and pinned to its release. It adds what a
shell needs around them: a session per terminal so the password is typed once,
dynamic completion that offers your realms, users, groups and clients, five output
formats, ASCII reports, and `--dry-run`/`--yes` around destructive calls.

## Features

- **The whole Admin API.** Realms, users, groups, clients, roles, scopes, identity
  providers, organizations, authentication flows, events, sessions and the
  authorization services — one command per operation, each calling its `keycloak`
  crate method with the crate's own types.
- **Sessions.** `keycloak-cli login` stores a token for that terminal (0600 state
  file, keyed by tty and shell session, with expiry); later commands reuse it and
  `logout` forgets it.
- **Shell completion.** Dynamic, for bash, zsh, fish, elvish and PowerShell, with
  live realms, users, groups and clients from your session, bounded to a 1.2 s
  budget per TAB.
- **Output.** `--output auto|json|yaml|table|markdown|html`, `auto` being a table
  on a terminal and JSON when piped, with `--fields` projection.
- **Reports.** `keycloak-cli report` writes a markdown, self-contained HTML or JSON
  document with ASCII charts for users, events and sessions.
- **Safe by default.** `--dry-run` shows the request and sends nothing, every
  `delete` needs `--yes`, and no secret is ever printed.
- **Bounded versions.** `x.y.(z*100+v)` mirrors the wrapped Keycloak `x.y.z` and
  the `keycloak` crate release it builds against.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/rust-utility/keycloak-cli/main/install.sh | sh
```

The script picks the binary for your platform, verifies it against the release's
`SHA256SUMS` and installs it into `~/.local/bin` (override with `--dir` or
`INSTALL_DIR`). Run it again to update, or use the built-in updater:

```sh
keycloak-cli self-update          # download, verify and replace this binary
keycloak-cli self-update --check  # only report whether a newer release exists
```

With Rust installed:

```sh
cargo install keycloak-cli
```

From a checkout, with only some tags (smaller and faster to build):

```sh
cargo install --path . --no-default-features --features tag-users
```

## Usage

Connection settings come from flags or `KEYCLOAK_*` environment variables (flags
win):

```sh
export KEYCLOAK_ADDR=https://keycloak.example.com
export KEYCLOAK_USER=admin
export KEYCLOAK_PASSWORD=...

keycloak-cli info
```

```
version   26.8.101 (wraps Keycloak Admin REST API 26.8.1)
server    https://keycloak.example.com
realm     master
user      admin
password  set (hidden)
```

Commands are named after the API path and verb. The realm comes from `-r/--realm`
(or `KEYCLOAK_REALM`), deeper path parameters are positionals in path order, query
parameters are flags, and `--body` takes a JSON file or `-` for standard input:

```sh
keycloak-cli clients list --first 10
keycloak-cli clients get 8a1b…                 # client uuid
keycloak-cli clients create --body client.json
keycloak-cli users list --search alice --max 5
keycloak-cli users role-mappings realm get <user-id>
keycloak-cli -r other users count
```

`keycloak-cli --help` lists the groups, `keycloak-cli clients --help` its
commands, and every command has the API's own summary and parameter help.

### Sessions

Log in once per terminal; the password is not needed again until the token
expires:

```sh
keycloak-cli login
keycloak-cli clients list      # no password, no token request per command
keycloak-cli logout
```

Each terminal has its own session: `eval "$(keycloak-cli init bash)"` exports
`KEYCLOAK_CLI_SESSION` (derived from the tty), and `login` stores the token under
that id in `$XDG_STATE_HOME/keycloak-cli/sessions.json` — mode 0600 in a 0700
directory, because it holds a bearer token. Another terminal, or one that never ran
`init`, asks for a password as before. Details, security notes and troubleshooting
are in [`doc/COMPLETION.md`](doc/COMPLETION.md).

### Shell completion

Completion is dynamic: the shell asks the binary, so candidates always match the
installed version — and, once you have logged in, they are your objects:

```sh
eval "$(keycloak-cli init bash)"     # ~/.bashrc  (bash 3.2 needs the eval form)
source <(keycloak-cli init zsh)      # ~/.zshrc
COMPLETE=fish keycloak-cli | source  # ~/.config/fish/completions/keycloak-cli.fish
```

`init` accepts `bash`, `zsh`, `fish`, `elvish` and `powershell`. The line runs the
binary at shell start, so re-source it after an upgrade or a move.

### Output

| `--output` | what you get |
|------------|--------------|
| `auto` (default) | a table when stdout is a terminal, JSON when piped |
| `json` | pretty JSON |
| `yaml` | YAML |
| `table` | a bordered table |
| `markdown` | a markdown table |
| `html` | an HTML table fragment |

A listing becomes one row per object, a single object becomes a `field`/`value`
table, nested values are JSON in their cell, and `--fields id,clientId` prints only
those columns (in that order, for every format):

```sh
keycloak-cli clients list                      # table in a terminal, JSON in a pipe
keycloak-cli clients list | jq '.[].clientId'  # JSON is the piped default
keycloak-cli --output yaml users list --max 5
keycloak-cli --output table --fields clientId,enabled clients list
keycloak-cli --output markdown groups list > groups.md
```

### Reports

`keycloak-cli report` writes a document about the realm — users and their
enabled/disabled split, events by type, and active sessions per client — each with
a horizontal ASCII chart. Markdown is the default, HTML is a single self-contained
page, and JSON is available for tooling:

```sh
keycloak-cli report > report.md
keycloak-cli report --output html --file report.html
keycloak-cli report --output json | jq '.sections[].title'
```

A section whose endpoint fails (events are off in many realms) says so instead of
failing the report.

### Safety

```sh
$ keycloak-cli --dry-run clients list --first 5
GET http://localhost:8080/admin/realms/master/clients?first=5
$ keycloak-cli --dry-run clients create --body client.json
POST http://localhost:8080/admin/realms/master/clients
{"clientId":"demo"}
$ keycloak-cli users delete abc
error: users delete deletes data: pass --yes to confirm, or --dry-run to see the request
```

`--dry-run` prints the request (method, URL with its query, and the body a mutation
would send) and changes nothing. Deletes refuse to run without `--yes`; a dry run
needs no confirmation because it deletes nothing. `--verbose` logs the request line
to stderr before sending it, `--quiet` keeps informational messages off stderr.
Data always goes to stdout.

### Exit codes

| code | meaning |
|------|---------|
| 0 | success |
| 1 | the request or the environment failed |
| 2 | the command line cannot be satisfied (missing `--body`, unknown flag) |
| 3 | Keycloak rejected the credentials (401/403) |
| 4 | the resource does not exist (404) |

## How it is built

The command tree is generated, not hand-written: `scripts/generate.sh` downloads
the exact `keycloak` crate pinned in `Cargo.toml`, unpacks it and runs
`src/generate/` over the OpenAPI description and the crate's own generated REST
sources, so every command carries the API's method, path, parameters, types, help
and the crate method it calls. `scripts/generate.sh --check` reproduces the
committed sources byte-for-byte in CI, and a test keeps the CLI's `tag-*` features
equal to the crate's.

`keycloak-cli-generate` (shipped in the crate) is that generator; nothing in the
CLI calls it at runtime.

One module per tag is emitted behind the matching `tag-*` feature. The default
build ships every tag (`tags-all`); `--no-default-features --features tag-users`
builds a CLI with only the user operations.

## Versioning

The package version encodes the wrapped Keycloak version exactly like the
`keycloak` crate: `x.y.(z * 100 + v)` targets Keycloak `x.y.z` with fix release
`v`. `26.8.101` is Keycloak `26.8.1` with fix release 1, `26.7.301` the second fix
release for Keycloak `26.7.3`. A fix release of the CLI does not move the wrapped
crate: the `keycloak` dependency stays pinned.

Publishing, including the trusted-publisher setup and the release recipe, is
documented in [`doc/PUBLISHING.md`](doc/PUBLISHING.md); the product roadmap and
what is still open live in [`doc/ROADMAP.md`](doc/ROADMAP.md).

## Support

You can contribute to the ongoing development and maintenance of **keycloak-cli**
in various ways:

### Sponsorship

Your support, no matter how big or small, helps sustain the project and ensures its
continued improvement. Reach out to explore sponsorship opportunities.

[![](https://img.shields.io/static/v1?label=Sponsor&message=%E2%9D%A4&logo=GitHub&color=%23fe8e86)](https://github.com/sponsors/kilork)

### Feedback

Whether you are a developer, user, or enthusiast, your feedback is invaluable.
Share your thoughts, suggestions, and ideas to help shape the future of the tool.

### Contribution

If you are passionate about open-source and have skills to share, consider
contributing to the project. Every contribution counts!

## Legal

Dual-licensed under the [Unlicense](UNLICENSE) and [MIT](MIT-LICENSE) licenses,
same as the `keycloak` crate.
