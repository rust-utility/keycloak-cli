# keycloak-cli

Command-line interface for the [Keycloak](https://www.keycloak.org) Admin REST
API, built on the [`keycloak`](https://crates.io/crates/keycloak) crate.

> **Status: generated command tree.** Every operation of the pinned `keycloak`
> crate is a command (415 of them, 36 groups), and each one calls its crate method.
> Shell completion, the output layer and reports are still ahead: see
> [`doc/ROADMAP.md`](doc/ROADMAP.md).

## Install

```sh
cargo install keycloak-cli
```

## Usage

Connection settings come from flags or `KEYCLOAK_*` environment variables
(flags win):

```sh
export KEYCLOAK_ADDR=https://keycloak.example.com
export KEYCLOAK_USER=admin
export KEYCLOAK_PASSWORD=...

keycloak-cli info
```

```
version   26.8.100 (wraps Keycloak Admin REST API 26.8.1)
server    https://keycloak.example.com
realm     master
user      admin
password  set (hidden)
```

Every operation of the API is a command, named after its path and method:

```sh
keycloak-cli clients list --first 10
keycloak-cli clients get 8a1b…            # client uuid
keycloak-cli clients create --body client.json
keycloak-cli users list --search alice --max 5
keycloak-cli -r other users count
```

Realm-level commands take the realm from `-r/--realm` (or `KEYCLOAK_REALM`); deeper
path parameters are positionals in path order, query parameters are flags, and
`--body` takes a JSON file or `-` for standard input. Responses are printed as
JSON on stdout, so they compose with `jq`:

```sh
keycloak-cli clients list | jq '.[].clientId'
```

## Shell completion

Completion is dynamic: the shell asks the binary, so candidates always match the
installed version (and, once live completion lands, the realms, users and groups
of the session).

```sh
# bash — macOS still ships bash 3.2, so the registration is evaluated
eval "$(keycloak-cli init bash)"      # add to ~/.bashrc
# zsh
source <(keycloak-cli init zsh)       # add to ~/.zshrc
# fish
COMPLETE=fish keycloak-cli | source   # add to ~/.config/fish/completions/keycloak-cli.fish
```

`keycloak-cli init <shell>` prints that line for `bash`, `zsh`, `fish`, `elvish`
and `powershell`, with the absolute path of the binary. The line runs the binary
on every shell start, which is deliberate: the completion protocol between shell
and binary is documented as unstable by clap, so re-sourcing (or re-running
`init`) after an upgrade or a move is what keeps completion working.

## Exit codes

| code | meaning |
|------|---------|
| 0 | success |
| 1 | the request or the environment failed |
| 2 | the command line cannot be satisfied (missing `--body`, unknown flag) |
| 3 | Keycloak rejected the credentials (401/403) |
| 4 | the resource does not exist (404) |

`keycloak-cli --version` prints `26.8.100 (Keycloak Admin REST API 26.8.1)`.

## Version scheme

The version follows the Keycloak version it wraps, exactly like the `keycloak`
crate: a crate version `x.y.(z * 100 + v)` targets Keycloak `x.y.z` with the fix
release `v`. `26.8.100` is Keycloak `26.8.1`, `26.7.301` would be the second fix
release for Keycloak `26.7.3`. The mapping is derived from `CARGO_PKG_VERSION`
(see `src/version.rs`), so a version bump is enough to follow a new Keycloak
release.

## Generated sources

The command tree is generated, not hand-written. `scripts/generate.sh`
downloads the `.crate` tarball of the exact version this package wraps (its own
package version) and unpacks it; the tarball carries the OpenAPI description
(`api/openapi.json`), the crate's own generator (which prints the `tag-*`
features) and the generated REST methods. `src/generate/` then emits one module
per tag from the description, and every command it emits calls the crate method
whose name, parameters and types it reads from those sources — so the CLI cannot
claim a command the crate does not have, or call one with the wrong shape:

```sh
scripts/generate.sh          # regenerate src/generated/
scripts/generate.sh --check  # fail if the committed sources are stale
KEYCLOAK_CLI_GENERATE_TAGS=clients,users scripts/generate.sh  # pick the tags
```

One module is emitted per tag of the API, behind the matching `tag-*` feature.
The default build ships every tag (`tags-all`); `--no-default-features --features
tag-users` builds a CLI with only the user operations, and `Cargo.toml`'s feature
list is checked against the crate's own `tags cargo` output by a test.

The output carries no timestamps and is formatted with the toolchain pinned in
`rust-toolchain.toml`, so `--check` is a byte-for-byte comparison and works as a
CI drift guard. Downloads and builds are cached in
`${XDG_CACHE_HOME:-~/.cache}/keycloak-cli/generate` (`--refresh` discards the
cache, `--crate FILE` uses a local tarball). The version inside the tarball is
asserted against the package version, so a version bump without a matching
release fails instead of silently generating a different API.

## Roadmap

Planned commands, milestones and non-goals live in
[`doc/ROADMAP.md`](doc/ROADMAP.md). Publishing setup is documented in
[`doc/PUBLISHING.md`](doc/PUBLISHING.md).

## License

Dual-licensed under the [Unlicense](UNLICENSE) and [MIT](MIT-LICENSE) licenses,
same as the `keycloak` crate.
