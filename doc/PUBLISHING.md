# Publishing

`keycloak-cli` is published to crates.io from GitHub Actions with
[trusted publishing](https://crates.io/docs/trusted-publishing): a tag runs
`.github/workflows/publish.yml`, which tests, exchanges the GitHub OIDC token for a
short-lived crates.io token and runs `cargo publish`. No token is stored anywhere.

## Setup (done 2026-11-15, keep for reference)

1. **Trusted publisher on crates.io** — <https://crates.io/crates/keycloak-cli/settings>
   with owner `rust-utility`, repository `keycloak-cli`, workflow `publish.yml`,
   environment `release`.
2. **Repository variable** — `CRATES_IO_PUBLISH=true`:

   ```sh
   gh variable set CRATES_IO_PUBLISH --body true --repo rust-utility/keycloak-cli
   ```

   Without it the workflow's only job is skipped and a tag only runs `ci.yml`.
3. **Environment** — the job runs in the `release` environment; add protection rules
   there if releases should need approval.

If a run fails at the token exchange with *"No Trusted Publishing config found for
repository …"*, the crates.io side is missing or does not match the workflow
(owner, repository, workflow file name, environment).

## Release

The version encodes the wrapped Keycloak version (see the README):
`x.y.(z * 100 + v)` targets Keycloak `x.y.z`, fix release `v`. A release is a
version bump plus a tag, keeping it in lockstep with the `keycloak` crate release it
builds against.

```sh
# 1. bump Cargo.toml to the new version and update the lock file
cargo build
git commit -am "Release v26.8.101"
git push

# 2. tag and push the tag: this triggers publish.yml
git tag -a v26.8.101 -m "keycloak-cli 26.8.101 (Keycloak Admin REST API 26.8.1)"
git push origin v26.8.101

# 3. a GitHub release with notes
gh release create v26.8.101 --title "keycloak-cli 26.8.101" --notes-file notes.md
```

Dry run without uploading (`Packaged` but no `Uploading`):

```sh
gh workflow run publish --ref v26.8.101 -f dry_run=true
```

The workflow pins the wrapped crate: the dependency is `keycloak = "=26.8.100"`, so
a tag can only build against the release the sources were generated from. A *fix
release* of the CLI (`26.8.101`) bumps only the package version — the pin, and
therefore the generated sources, stay where they are.

## Release binaries

A tag also runs `.github/workflows/release.yml`, which builds the CLI for
`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl` and
`aarch64-unknown-linux-musl`, attaches `keycloak-cli-<target>` for each to the
release and adds a `SHA256SUMS` file. `install.sh` and `keycloak-cli self-update`
consume exactly those assets, so a release is not complete until the workflow is
green:

```sh
gh run watch $(gh run list --workflow release --limit 1 --json databaseId -q '.[0].databaseId')
gh release view v26.8.101 --repo rust-utility/keycloak-cli
```

## Verifying a release

- `gh run watch` the publish run: the log shows `Packaged` → `Uploading` →
  `Uploaded` → `Published`.
- The registry API: `curl -s https://crates.io/api/v1/crates/keycloak-cli | jq -r .crate.max_version`.
- docs.rs builds the crate automatically after publication:
  <https://docs.rs/keycloak-cli>.
- The package itself: `cargo package --locked` runs in CI on every push, so a
  missing `README.md`, license file or rejected metadata fails before a tag is cut.
  To inspect the archive locally:

  ```sh
  cargo package --locked --no-verify
  tar -tzf target/package/keycloak-cli-*.crate | sort
  ```

## Installed crate

`cargo install keycloak-cli` builds the CLI with every tag enabled. For a smaller
build, install from a checkout with a tag subset:

```sh
cargo install --path . --no-default-features --features tag-users
```
