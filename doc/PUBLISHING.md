# Publishing

`keycloak-cli` is published to crates.io from GitHub Actions with
[trusted publishing](https://crates.io/docs/trusted-publishing) (OIDC, no stored
token). The workflow is already in place (`.github/workflows/publish.yml`) but
inert until both steps below are done.

## One-time setup

1. **Trusted publisher on crates.io.** The crate `keycloak-cli` already exists
   (0.1.0, owned by `kilork`); open <https://crates.io/crates/keycloak-cli/settings>
   and add a trusted publisher with:
   - Owner: `rust-utility`
   - Repository: `keycloak-cli`
   - Workflow: `publish.yml`
   - Environment: `release`

2. **Repository variable.** Enable the workflow in this repository:

   ```sh
   gh variable set CRATES_IO_PUBLISH --body true --repo rust-utility/keycloak-cli
   ```

   Until it is set, `.github/workflows/publish.yml` skips its only job, so a tag
   only runs `ci.yml`.

3. **Environment.** The job runs in the `release` environment; add protection
   rules there (required reviewers) if releases should need approval.

## Release

The version encodes the wrapped Keycloak version (see the README):
`x.y.(z * 100 + v)` targets Keycloak `x.y.z`, fix release `v`. A release is a
version bump plus a tag, keeping it in lockstep with the `keycloak` crate
release it builds against.

```sh
# 1. bump Cargo.toml to the new version and update Cargo.lock
cargo build
git commit -am "Release v26.8.100"
git push

# 2. tag and push the tag (this triggers publish.yml)
git tag -a v26.8.100 -m "Release v26.8.100"
git push origin v26.8.100
```

Dry run without uploading: `Actions → publish → Run workflow → dry_run`.

## Verifying the package

`cargo package --locked` runs in CI on every push, so a missing `README.md`,
license file or rejected metadata fails before a tag is cut. To inspect the
archive locally:

```sh
cargo package --locked --no-verify
tar -tzf target/package/keycloak-cli-*.crate | sort
```
