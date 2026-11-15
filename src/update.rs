//! `self-update`: replace this binary with a published release.
//!
//! The release workflow attaches one raw binary per target plus a `SHA256SUMS`
//! file, so updating is three steps with nothing to unpack: ask GitHub for the
//! latest tag, download the binary for this platform, verify it against the
//! checksum, and rename it over the running executable (same directory, so the
//! rename is atomic). `install.sh` performs the same steps from a shell.
//!
//! The HTTP side takes the base URLs as fields, so the whole flow can be tested
//! against a loopback stub instead of GitHub.

use std::path::{Path, PathBuf};

use keycloak::prelude::reqwest;
use sha2::{Digest, Sha256};

use crate::error::Error;

/// Where the releases live.
pub const REPOSITORY: &str = "rust-utility/keycloak-cli";

/// File name of the checksum list in a release.
pub const CHECKSUMS: &str = "SHA256SUMS";

/// The release target of the running platform, when binaries are published for it.
pub fn target(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-musl"),
        _ => None,
    }
}

/// The target of this build.
pub fn current_target() -> Option<&'static str> {
    target(std::env::consts::OS, std::env::consts::ARCH)
}

/// What `self-update` should do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Only report whether a newer release exists.
    pub check: bool,
    /// Install this version instead of the latest one, e.g. `26.8.101`.
    pub version: Option<String>,
    /// Install even when the version is already the running one.
    pub force: bool,
}

/// The outcome of a run, for the caller to print.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The running version is the one that would be installed.
    UpToDate { version: String },
    /// A newer release exists (with `--check`).
    Available { current: String, latest: String },
    /// The binary was replaced.
    Updated {
        from: String,
        to: String,
        path: PathBuf,
    },
}

/// Talking to the release host.
#[derive(Clone, Debug)]
pub struct Update {
    api: String,
    download: String,
    target: &'static str,
    repository: String,
}

impl Default for Update {
    fn default() -> Self {
        Self::new(REPOSITORY, current_target().unwrap_or_default())
    }
}

impl Update {
    /// An updater for `repository`, downloading binaries for `target`.
    pub fn new(repository: &str, target: &'static str) -> Self {
        Self {
            api: "https://api.github.com".to_owned(),
            download: "https://github.com".to_owned(),
            target,
            repository: repository.to_owned(),
        }
    }

    /// Points the updater at other hosts (tests, mirrors).
    pub fn with_hosts(mut self, api: impl Into<String>, download: impl Into<String>) -> Self {
        self.api = api.into();
        self.download = download.into();
        self
    }

    /// The target the updater downloads for.
    pub fn target(&self) -> &'static str {
        self.target
    }

    /// The tag of the latest release, without `v`.
    pub async fn latest(&self) -> Result<String, Error> {
        let url = format!(
            "{}/repos/{}/releases/latest",
            self.api.trim_end_matches('/'),
            self.repository
        );
        let response = self.get(&url).await?;
        let body: serde_json::Value = serde_json::from_slice(&response)
            .map_err(|error| Error::Update(format!("unexpected release description: {error}")))?;
        let tag = body
            .get("tag_name")
            .and_then(|tag| tag.as_str())
            .ok_or_else(|| Error::Update("the release has no tag".to_owned()))?;
        Ok(strip_tag(tag).to_owned())
    }

    /// Where the binary of `version` is.
    pub fn asset_url(&self, version: &str) -> String {
        format!(
            "{}/{}/releases/download/v{}/keycloak-cli-{}",
            self.download.trim_end_matches('/'),
            self.repository,
            strip_tag(version),
            self.target
        )
    }

    /// Where the checksum list of `version` is.
    pub fn checksums_url(&self, version: &str) -> String {
        format!(
            "{}/{}/releases/download/v{}/{}",
            self.download.trim_end_matches('/'),
            self.repository,
            strip_tag(version),
            CHECKSUMS
        )
    }

    /// The SHA-256 the release publishes for this platform's binary.
    pub async fn expected_sha256(&self, version: &str) -> Result<String, Error> {
        let body = self.get(&self.checksums_url(version)).await?;
        let text = String::from_utf8_lossy(&body);
        let name = format!("keycloak-cli-{}", self.target);
        parse_checksums(&text, &name)
            .ok_or_else(|| Error::Update(format!("{CHECKSUMS} has no entry for {name}")))
    }

    /// Downloads and verifies the binary of `version`.
    pub async fn download(&self, version: &str) -> Result<Vec<u8>, Error> {
        let expected = self.expected_sha256(version).await?;
        let bytes = self.get(&self.asset_url(version)).await?;
        let actual = sha256(&bytes);
        if actual != expected {
            return Err(Error::Update(format!(
                "checksum mismatch for {}: expected {expected}, got {actual}",
                self.asset_url(version)
            )));
        }
        Ok(bytes)
    }

    /// Runs the update: resolve the version, download, verify and replace `exe`.
    pub async fn run(
        &self,
        exe: &Path,
        current: &str,
        options: &Options,
    ) -> Result<Outcome, Error> {
        if self.target.is_empty() {
            return Err(Error::Update(format!(
                "no release binary for {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )));
        }
        let latest = match options.version.as_deref() {
            Some(version) => strip_tag(version).to_owned(),
            None => self.latest().await?,
        };

        // Compare as versions, not as strings: a build newer than the latest
        // release (a local build, or a release that was pulled) is up to date.
        if !options.force && !is_newer(&latest, current) {
            return Ok(Outcome::UpToDate {
                version: strip_tag(current).to_owned(),
            });
        }
        if options.check {
            return Ok(Outcome::Available {
                current: strip_tag(current).to_owned(),
                latest,
            });
        }

        let bytes = self.download(&latest).await?;
        install(&bytes, exe)?;
        Ok(Outcome::Updated {
            from: strip_tag(current).to_owned(),
            to: latest,
            path: exe.to_owned(),
        })
    }

    /// GET a URL, turning a non-success status into an error with its body.
    async fn get(&self, url: &str) -> Result<Vec<u8>, Error> {
        let response = reqwest::Client::builder()
            .user_agent(concat!("keycloak-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Error::Update(error.to_string()))?
            .get(url)
            .send()
            .await
            .map_err(|error| Error::Update(format!("{url}: {error}")))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|error| Error::Update(format!("{url}: {error}")))?;
        if !status.is_success() {
            return Err(Error::Update(format!(
                "{url}: HTTP {} — {}",
                status.as_u16(),
                first_line(&String::from_utf8_lossy(&body))
            )));
        }
        Ok(body.to_vec())
    }
}

/// Writes `bytes` next to `exe` and renames them over it.
fn install(bytes: &[u8], exe: &Path) -> Result<(), Error> {
    let temporary = exe.with_extension("new");
    std::fs::write(&temporary, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&temporary, exe)?;
    Ok(())
}

/// The body of an error response, cut to one line.
fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let mut line: String = line.chars().take(200).collect();
    if line.is_empty() {
        line.push_str("(no body)");
    }
    line
}

/// Whether `candidate` is a later version than `current`, by the `x.y.z` order
/// the package uses. Unparseable versions fall back to string inequality.
fn is_newer(candidate: &str, current: &str) -> bool {
    match (
        crate::version::Version::parse(candidate),
        crate::version::Version::parse(strip_tag(current)),
    ) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => candidate != strip_tag(current),
    }
}

/// `v26.8.101` and `26.8.101` both mean `26.8.101`.
fn strip_tag(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// The hex SHA-256 of `bytes`.
fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Finds the checksum of `name` in a `SHA256SUMS` file (`<hex>  <name>`).
fn parse_checksums(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let checksum = fields.next()?;
        let file = fields.next()?;
        // `sha256sum` marks binary mode with a leading `*`.
        let file = file.strip_prefix('*').unwrap_or(file);
        let file = file.strip_prefix("./").unwrap_or(file);
        (file == name && checksum.len() == 64).then(|| checksum.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platforms_map_to_release_targets() {
        assert_eq!(target("macos", "aarch64"), Some("aarch64-apple-darwin"));
        assert_eq!(target("macos", "x86_64"), Some("x86_64-apple-darwin"));
        assert_eq!(target("linux", "x86_64"), Some("x86_64-unknown-linux-musl"));
        assert_eq!(
            target("linux", "aarch64"),
            Some("aarch64-unknown-linux-musl")
        );
        assert_eq!(target("windows", "x86_64"), None);
        assert_eq!(target("freebsd", "x86_64"), None);
    }

    #[test]
    fn urls_name_the_release_assets() {
        let update = Update::new(REPOSITORY, "aarch64-apple-darwin");
        assert_eq!(
            update.asset_url("26.8.101"),
            "https://github.com/rust-utility/keycloak-cli/releases/download/v26.8.101/keycloak-cli-aarch64-apple-darwin"
        );
        assert_eq!(update.asset_url("v26.8.101"), update.asset_url("26.8.101"));
        assert_eq!(
            update.checksums_url("26.8.101"),
            "https://github.com/rust-utility/keycloak-cli/releases/download/v26.8.101/SHA256SUMS"
        );
    }

    #[test]
    fn checksums_are_found_for_the_target() {
        let text = format!(
            "{}  keycloak-cli-aarch64-apple-darwin\n{}  *keycloak-cli-x86_64-unknown-linux-musl\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        assert_eq!(
            parse_checksums(&text, "keycloak-cli-aarch64-apple-darwin"),
            Some("a".repeat(64))
        );
        // `sha256sum` binary-mode marker is accepted too.
        assert_eq!(
            parse_checksums(&text, "keycloak-cli-x86_64-unknown-linux-musl"),
            Some("b".repeat(64))
        );
        assert_eq!(parse_checksums(&text, "keycloak-cli-other"), None);

        // Comments, extra spaces and a `./` prefix do not hide an entry, and a
        // truncated checksum is rejected rather than trusted.
        let messy = format!(
            "# generated by CI\n  {}   ./keycloak-cli-aarch64-apple-darwin\n",
            "a".repeat(64)
        );
        assert_eq!(
            parse_checksums(&messy, "keycloak-cli-aarch64-apple-darwin"),
            Some("a".repeat(64))
        );
        assert_eq!(
            parse_checksums(
                "abc  keycloak-cli-aarch64-apple-darwin",
                "keycloak-cli-aarch64-apple-darwin"
            ),
            None
        );
    }

    #[test]
    fn newer_versions_are_ordered_not_compared_as_strings() {
        assert!(is_newer("26.8.102", "26.8.101"));
        assert!(is_newer("v26.8.102", "26.8.101"));
        assert!(!is_newer("26.8.101", "26.8.102"));
        assert!(!is_newer("26.8.101", "26.8.101"));
        // 26.8.10 is a *fix release* of 26.8.1, so it sorts after it.
        assert!(is_newer("26.8.10", "26.8.2"));
        // Anything unparseable is compared literally.
        assert!(is_newer("nightly", "26.8.101"));
        assert!(!is_newer("26.8.101", "26.8.101"));
    }

    #[test]
    fn sha256_matches_the_known_digest() {
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn installing_replaces_the_file_and_keeps_it_executable() {
        let dir = std::env::temp_dir().join(format!(
            "keycloak-cli-update-{}-{}",
            std::process::id(),
            crate::session::now()
        ));
        std::fs::create_dir_all(&dir).expect("the directory exists");
        let exe = dir.join("keycloak-cli");
        std::fs::write(&exe, b"old").expect("the old binary exists");

        install(b"new", &exe).expect("the binary is replaced");
        assert_eq!(std::fs::read(&exe).expect("readable"), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&exe).expect("stat").permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
            assert!(!dir.join("keycloak-cli.new").exists(), "no leftovers");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
