# keycloak-cli

Command-line interface for the [Keycloak](https://www.keycloak.org) Admin REST
API, built on the [`keycloak`](https://crates.io/crates/keycloak) crate.

> **Status: skeleton.** The repository was just created: the command line shape,
> the connection configuration and the version scheme exist, API calls do not.
> See [`doc/ROADMAP.md`](doc/ROADMAP.md) for the planned scope.

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
