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

## Roadmap

Planned commands, milestones and non-goals live in
[`doc/ROADMAP.md`](doc/ROADMAP.md). Publishing setup is documented in
[`doc/PUBLISHING.md`](doc/PUBLISHING.md).

## License

Dual-licensed under the [Unlicense](UNLICENSE) and [MIT](MIT-LICENSE) licenses,
same as the `keycloak` crate.
