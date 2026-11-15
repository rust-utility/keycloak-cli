//! Live values for completion, bounded by a timeout and a cache.
//!
//! A TAB spawns the binary and calls a synchronous completion callback (F12, F14):
//! the values come from an async API, the callback is not async, and a process is
//! started every time. So every lookup is budgeted — a hard timeout, a TTL cache
//! per parameter, a cap on the number of candidates — and any failure yields no
//! candidates and no output at all (M5). Completion must never make the prompt
//! slower or noisier than not having it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Where completion values come from.
///
/// Sources are shared and called from the completion thread, so they must be
/// `Send + Sync`; a source that talks to Keycloak creates its own runtime.
pub trait Source: Send + Sync {
    /// The values to suggest, in the order they should be shown.
    fn values(&self) -> Result<Vec<String>, Error>;
}

/// How many candidates a single TAB may print.
pub const MAX_CANDIDATES: usize = 100;

/// Set this to any non-empty value to turn live completion off.
pub const KILL_SWITCH: &str = "KEYCLOAK_CLI_NO_LIVE_COMPLETION";

/// A source with the cache and the budget around it.
///
/// One registry is built per process, i.e. per TAB: the cache is what makes a
/// second TAB within the TTL cheap without keeping state between processes (F15).
pub struct Registry {
    sources: BTreeMap<String, Arc<dyn Source>>,
    cache: Mutex<BTreeMap<String, Entry>>,
    ttl: Duration,
    timeout: Duration,
    limit: usize,
    /// Where answers survive between processes (one process per TAB), if anywhere.
    disk: Option<PathBuf>,
    /// Distinguishes sessions, servers and realms in the disk cache.
    namespace: String,
}

/// A cached answer and when it was fetched.
struct Entry {
    values: Vec<String>,
    at: Instant,
}

impl Registry {
    /// A registry that keeps answers for `ttl` and waits at most `timeout` per
    /// source call.
    pub fn new(ttl: Duration, timeout: Duration) -> Self {
        Self {
            sources: BTreeMap::new(),
            cache: Mutex::new(BTreeMap::new()),
            ttl,
            timeout,
            limit: MAX_CANDIDATES,
            disk: None,
            namespace: String::new(),
        }
    }

    /// Keeps fresh answers on disk in `dir`, so the next TAB (which is another
    /// process) does not pay for the same request again.
    pub fn with_disk_cache(
        mut self,
        dir: impl Into<PathBuf>,
        namespace: impl Into<String>,
    ) -> Self {
        self.disk = Some(dir.into());
        self.namespace = namespace.into();
        self
    }

    /// Whether `parameter` has a source.
    pub fn has(&self, parameter: &str) -> bool {
        self.sources.contains_key(parameter)
    }

    /// Caps the number of candidates returned.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Adds (or replaces) the source for a parameter name, e.g. `client_uuid`.
    pub fn register(&mut self, parameter: impl Into<String>, source: Arc<dyn Source>) {
        self.sources.insert(parameter.into(), source);
    }

    /// The parameters that have a source.
    pub fn parameters(&self) -> Vec<&str> {
        self.sources.keys().map(String::as_str).collect()
    }

    /// Whether live completion is switched off for this process.
    pub fn disabled() -> bool {
        std::env::var(KILL_SWITCH)
            .map(|value| !value.is_empty() && value != "0")
            .unwrap_or(false)
    }

    /// The candidates of `parameter` that start with `current`.
    ///
    /// Never fails: an unknown parameter, a source error, a timeout or a disabled
    /// kill switch all return an empty list.
    pub fn candidates(&self, parameter: &str, current: &str) -> Vec<String> {
        if Self::disabled() {
            return Vec::new();
        }
        let Some(values) = self.values(parameter) else {
            return Vec::new();
        };
        let current = current.to_ascii_lowercase();
        let mut candidates: Vec<String> = values
            .into_iter()
            .filter(|value| value.to_ascii_lowercase().starts_with(&current))
            .take(self.limit)
            .collect();
        candidates.sort();
        candidates.dedup();
        candidates
    }

    /// The values of `parameter`, from a cache when one is fresh.
    fn values(&self, parameter: &str) -> Option<Vec<String>> {
        let source = self.sources.get(parameter)?;

        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.get(parameter) {
                if entry.at.elapsed() < self.ttl {
                    return Some(entry.values.clone());
                }
            }
        }

        if let Some(values) = self.read_disk(parameter) {
            self.remember(parameter, &values);
            return Some(values);
        }

        let values = fetch(source, self.timeout)?;
        self.remember(parameter, &values);
        self.write_disk(parameter, &values);
        Some(values)
    }

    /// Puts a fresh answer in the in-memory cache.
    fn remember(&self, parameter: &str, values: &[String]) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                parameter.to_owned(),
                Entry {
                    values: values.to_vec(),
                    at: Instant::now(),
                },
            );
        }
    }

    /// The file an answer would be cached in.
    fn disk_path(&self, parameter: &str) -> Option<PathBuf> {
        let dir = self.disk.as_ref()?;
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for byte in self.namespace.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        let name: String = parameter
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        Some(dir.join(format!("{name}-{hash:016x}.json")))
    }

    /// A fresh answer from the disk cache, if there is one.
    fn read_disk(&self, parameter: &str) -> Option<Vec<String>> {
        let path = self.disk_path(parameter)?;
        let text = std::fs::read_to_string(&path).ok()?;
        let cached: Cached = serde_json::from_str(&text).ok()?;
        let now = unix_now();
        if now.saturating_sub(cached.at) >= self.ttl.as_secs() {
            return None;
        }
        Some(cached.values)
    }

    /// Stores a fresh answer for the next TAB.
    fn write_disk(&self, parameter: &str, values: &[String]) {
        let Some(path) = self.disk_path(parameter) else {
            return;
        };
        let Some(dir) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let cached = Cached {
            at: unix_now(),
            values: values.to_vec(),
        };
        if let Ok(text) = serde_json::to_string(&cached) {
            let _ = std::fs::write(&path, text);
        }
    }
}

/// The on-disk shape of a cached answer.
#[derive(Deserialize, Serialize)]
struct Cached {
    /// Unix seconds at which the answer was fetched.
    at: u64,
    values: Vec<String>,
}

/// Unix seconds now.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// The process-wide registry: the Keycloak sources, a 60 s cache and a 1.2 s
/// budget per TAB.
///
/// Completion runs in a process the shell starts, so the disk cache is what makes
/// a second TAB fast: the shell pays for the spawn, the file read and nothing else.
pub fn global() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = Registry::new(Duration::from_secs(60), Duration::from_millis(1200));
        if let Ok(dir) = crate::session::state_dir() {
            let namespace = crate::session::current_id().unwrap_or_default();
            registry = registry.with_disk_cache(dir.join("completion"), namespace);
        }
        for (parameter, source) in crate::live_sources::sources() {
            registry.register(parameter, source);
        }
        registry
    })
}

/// Where the disk cache of `registry` lives, for tests and diagnostics.
pub fn cache_dir(registry: &Registry) -> Option<&Path> {
    registry.disk.as_deref()
}

/// Calls `source`, giving up after `timeout`.
///
/// The call runs on its own thread because the completion callback has no runtime
/// to time out against, and a stuck source must not hang the shell. A timed-out
/// thread is left to finish on its own; the process is about to exit anyway.
fn fetch(source: &Arc<dyn Source>, timeout: Duration) -> Option<Vec<String>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let source = Arc::clone(source);
    std::thread::spawn(move || {
        let _ = sender.send(source.values());
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok(values)) => Some(values),
        // An error or a timeout both mean "no candidates", never a message.
        Ok(Err(_)) | Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// The kill switch is process-wide, so the tests that read it take turns.
    static ENV: Mutex<()> = Mutex::new(());

    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        ENV.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    /// A source that counts how often it was asked.
    struct Fake {
        values: Vec<String>,
        calls: Arc<AtomicUsize>,
        delay: Duration,
        fail: bool,
    }

    impl Fake {
        fn new(values: &[&str]) -> (Self, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    values: values.iter().map(|value| (*value).to_owned()).collect(),
                    calls: Arc::clone(&calls),
                    delay: Duration::ZERO,
                    fail: false,
                },
                calls,
            )
        }
    }

    impl Source for Fake {
        fn values(&self) -> Result<Vec<String>, Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            if self.fail {
                return Err(Error::usage("no values"));
            }
            Ok(self.values.clone())
        }
    }

    fn registry_for(ttl: Duration, timeout: Duration, fake: Fake) -> Registry {
        let mut registry = Registry::new(ttl, timeout);
        registry.register("client_uuid", Arc::new(fake));
        registry
    }

    #[test]
    fn filters_by_what_was_typed() {
        let (fake, _) = Fake::new(&["alpha", "alphabet", "beta"]);
        let registry = registry_for(Duration::from_secs(60), Duration::from_secs(5), fake);
        assert_eq!(
            registry.candidates("client_uuid", "alp"),
            ["alpha", "alphabet"]
        );
        assert_eq!(
            registry.candidates("client_uuid", "ALP"),
            ["alpha", "alphabet"]
        );
        assert_eq!(registry.candidates("client_uuid", "").len(), 3);
        assert!(registry.candidates("client_uuid", "zzz").is_empty());
        assert!(
            registry.candidates("user_id", "").is_empty(),
            "unknown parameter"
        );
    }

    #[test]
    fn a_fresh_answer_comes_from_the_cache() {
        let _guard = exclusive();
        let (fake, calls) = Fake::new(&["alpha"]);
        let registry = registry_for(Duration::from_secs(60), Duration::from_secs(5), fake);
        assert_eq!(registry.candidates("client_uuid", ""), ["alpha"]);
        assert_eq!(registry.candidates("client_uuid", ""), ["alpha"]);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "one fetch, one cache hit");

        let (fake, calls) = Fake::new(&["alpha"]);
        let registry = registry_for(Duration::ZERO, Duration::from_secs(5), fake);
        registry.candidates("client_uuid", "");
        registry.candidates("client_uuid", "");
        assert_eq!(calls.load(Ordering::SeqCst), 2, "a zero ttl never caches");
    }

    #[test]
    fn a_slow_source_is_cut_off() {
        let _guard = exclusive();
        let (mut fake, calls) = Fake::new(&["alpha"]);
        fake.delay = Duration::from_millis(500);
        let registry = registry_for(Duration::from_secs(60), Duration::from_millis(50), fake);

        let started = Instant::now();
        assert!(registry.candidates("client_uuid", "").is_empty());
        assert!(
            started.elapsed() < Duration::from_millis(400),
            "the timeout did not cut the fetch: {:?}",
            started.elapsed()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // A timed-out answer is not cached.
        assert!(registry.candidates("client_uuid", "").is_empty());
    }

    #[test]
    fn a_failing_source_is_silent() {
        let (mut fake, _) = Fake::new(&["alpha"]);
        fake.fail = true;
        let registry = registry_for(Duration::from_secs(60), Duration::from_secs(5), fake);
        assert!(registry.candidates("client_uuid", "").is_empty());
    }

    #[test]
    fn the_number_of_candidates_is_capped() {
        let _guard = exclusive();
        let values: Vec<String> = (0..250).map(|index| format!("value-{index:03}")).collect();
        let borrowed: Vec<&str> = values.iter().map(String::as_str).collect();
        let (fake, _) = Fake::new(&borrowed);
        let registry =
            registry_for(Duration::from_secs(60), Duration::from_secs(5), fake).with_limit(10);
        assert_eq!(registry.candidates("client_uuid", "").len(), 10);
    }

    #[test]
    fn the_kill_switch_returns_nothing() {
        let _guard = exclusive();
        let (fake, calls) = Fake::new(&["alpha"]);
        let registry = registry_for(Duration::from_secs(60), Duration::from_secs(5), fake);
        let previous = std::env::var(KILL_SWITCH).ok();
        std::env::set_var(KILL_SWITCH, "1");
        assert!(registry.candidates("client_uuid", "").is_empty());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a disabled registry never fetches"
        );
        match previous {
            Some(value) => std::env::set_var(KILL_SWITCH, value),
            None => std::env::remove_var(KILL_SWITCH),
        }
    }

    /// A directory of its own for a disk cache.
    fn cache_dir_for(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "keycloak-cli-completion-{name}-{}-{}",
            std::process::id(),
            unix_now()
        ));
        std::fs::create_dir_all(&dir).expect("the cache directory exists");
        dir
    }

    #[test]
    fn a_fresh_disk_answer_is_shared_with_the_next_process() {
        let _guard = exclusive();
        let dir = cache_dir_for("fresh");

        let (fake, calls) = Fake::new(&["alpha"]);
        let registry = registry_for(Duration::from_secs(60), Duration::from_secs(5), fake)
            .with_disk_cache(&dir, "session-1");
        assert_eq!(registry.candidates("client_uuid", ""), ["alpha"]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(cache_dir(&registry).is_some());

        // The next TAB is another process: a new registry, a source that must not
        // be asked because the disk answer is still fresh.
        let (fake, calls) = Fake::new(&["should-not-be-used"]);
        let registry = registry_for(Duration::from_secs(60), Duration::from_secs(5), fake)
            .with_disk_cache(&dir, "session-1");
        assert_eq!(registry.candidates("client_uuid", ""), ["alpha"]);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "served from disk");

        // Another session (another server, another realm) has its own answers.
        let (fake, calls) = Fake::new(&["beta"]);
        let registry = registry_for(Duration::from_secs(60), Duration::from_secs(5), fake)
            .with_disk_cache(&dir, "session-2");
        assert_eq!(registry.candidates("client_uuid", ""), ["beta"]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_stale_disk_answer_is_fetched_again() {
        let _guard = exclusive();
        let dir = cache_dir_for("stale");

        let (fake, _) = Fake::new(&["alpha"]);
        let registry = registry_for(Duration::from_secs(60), Duration::from_secs(5), fake)
            .with_disk_cache(&dir, "session-1");
        registry.candidates("client_uuid", "");

        // A zero TTL makes every stored answer stale.
        let (fake, calls) = Fake::new(&["beta"]);
        let registry = registry_for(Duration::ZERO, Duration::from_secs(5), fake)
            .with_disk_cache(&dir, "session-1");
        assert_eq!(registry.candidates("client_uuid", ""), ["beta"]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn registered_parameters_are_listed() {
        let (fake, _) = Fake::new(&["alpha"]);
        let registry = registry_for(Duration::from_secs(60), Duration::from_secs(5), fake);
        assert_eq!(registry.parameters(), ["client_uuid"]);
    }
}
