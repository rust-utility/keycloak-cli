//! Live values for completion, bounded by a timeout and a cache.
//!
//! A TAB spawns the binary and calls a synchronous completion callback (F12, F14):
//! the values come from an async API, the callback is not async, and a process is
//! started every time. So every lookup is budgeted — a hard timeout, a TTL cache
//! per parameter, a cap on the number of candidates — and any failure yields no
//! candidates and no output at all (M5). Completion must never make the prompt
//! slower or noisier than not having it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
        }
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

    /// The values of `parameter`, from the cache when it is fresh.
    fn values(&self, parameter: &str) -> Option<Vec<String>> {
        let source = self.sources.get(parameter)?;

        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.get(parameter) {
                if entry.at.elapsed() < self.ttl {
                    return Some(entry.values.clone());
                }
            }
        }

        let values = fetch(source, self.timeout)?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                parameter.to_owned(),
                Entry {
                    values: values.clone(),
                    at: Instant::now(),
                },
            );
        }
        Some(values)
    }
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

    #[test]
    fn registered_parameters_are_listed() {
        let (fake, _) = Fake::new(&["alpha"]);
        let registry = registry_for(Duration::from_secs(60), Duration::from_secs(5), fake);
        assert_eq!(registry.parameters(), ["client_uuid"]);
    }
}
