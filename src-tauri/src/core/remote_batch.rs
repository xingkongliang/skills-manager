use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::core::git_fetcher::{self, CacheRefresh};
use crate::core::remote_ref::{RemoteIdentity, RemoteRef};

/// Upper bound on concurrent checkouts during a batch update.
///
/// Lower than the check phase's limit on purpose: a checkout writes a copy of
/// the tree to disk, where a check only reads a reference advertisement.
pub const MAX_UPDATE_CONCURRENCY: usize = 4;

/// The two things a batch update needs from the network, behind one seam.
///
/// Production talks to git; a test records the calls and answers with a fixture,
/// which is how "one trip per Source" and "the cap holds" become assertions
/// instead of hopes.
pub trait RemoteTransport: Send + Sync {
    /// The Source's code on disk, at whatever the branch points to right now
    /// when `refresh` is [`CacheRefresh::Fetch`].
    ///
    /// There is deliberately no separate "what is the head" call: asking the
    /// remote twice per Source (once for the revision, once for the code) was
    /// half the network traffic of an update, and the checkout already knows the
    /// answer.
    fn fetch_checkout(&self, source: &RemoteRef, refresh: CacheRefresh) -> Result<std::path::PathBuf>;
}

/// The real transport: git subprocesses and the repository cache.
pub struct GitTransport {
    proxy_url: Option<String>,
    /// Cancellation for a user-initiated update. Batches pass none: they have no
    /// cancel surface yet, so nothing would set it.
    cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl GitTransport {
    pub fn new(proxy_url: Option<String>) -> Self {
        Self {
            proxy_url,
            cancel: None,
        }
    }

    pub fn with_cancel(
        proxy_url: Option<String>,
        cancel: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Self {
        Self {
            proxy_url,
            cancel: cancel.cloned(),
        }
    }
}

impl RemoteTransport for GitTransport {
    fn fetch_checkout(&self, source: &RemoteRef, refresh: CacheRefresh) -> Result<std::path::PathBuf> {
        git_fetcher::clone_repo_ref_scoped_with(
            &source.clone_url,
            source.branch.as_deref(),
            source.subpath.as_deref(),
            self.cancel.as_ref(),
            self.proxy_url.as_deref(),
            None,
            refresh,
        )
    }
}

/// Run `work` over every item with at most `max` in flight.
pub fn run_concurrent<T, R, F>(items: Vec<T>, max: usize, work: F) -> Vec<(T, R)>
where
    T: Send + Sync + Clone,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(T, R)>> = Mutex::new(Vec::with_capacity(items.len()));
    let workers = max.max(1).min(items.len().max(1));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let at = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(at) else { break };
                let result = work(item);
                if let Ok(mut collected) = results.lock() {
                    collected.push((item.clone(), result));
                }
            });
        }
    });

    results.into_inner().unwrap_or_default()
}

/// Whether each Source in a group must refresh the cache before it is copied.
///
/// Cache slots are per Source **and** per shape — a root install and a
/// subdirectory install of one repository do not share one — so the first of
/// each shape refreshes and its siblings reuse. Order is preserved: the result
/// lines up with the input.
pub fn plan_fetches(sources: &[RemoteRef]) -> Vec<CacheRefresh> {
    let mut refreshed: HashSet<(RemoteIdentity, bool)> = HashSet::new();
    sources
        .iter()
        .map(|source| {
            let has_subpath = source.subpath.as_deref().is_some_and(|s| !s.is_empty());
            if refreshed.insert((source.identity(), has_subpath)) {
                CacheRefresh::Fetch
            } else {
                CacheRefresh::Reuse
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct Recording {
        fetches: Mutex<Vec<(String, CacheRefresh)>>,
        in_flight: AtomicUsize,
        peak: AtomicUsize,
        fail_for: Option<&'static str>,
    }

    impl Recording {
        fn failing_for(url: &'static str) -> Self {
            Self {
                fail_for: Some(url),
                ..Default::default()
            }
        }
    }

    impl RemoteTransport for Recording {
        fn fetch_checkout(
            &self,
            source: &RemoteRef,
            refresh: CacheRefresh,
        ) -> Result<std::path::PathBuf> {
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            self.in_flight.fetch_sub(1, Ordering::SeqCst);

            if self.fail_for == Some(source.clone_url.as_str()) {
                anyhow::bail!("boom");
            }
            self.fetches
                .lock()
                .unwrap()
                .push((source.clone_url.clone(), refresh));
            Ok(std::path::PathBuf::from("/fixture"))
        }
    }

    fn source(url: &str, subpath: Option<&str>) -> RemoteRef {
        RemoteRef {
            original_url: url.to_string(),
            clone_url: url.to_string(),
            branch: None,
            subpath: subpath.map(str::to_string),
            locator: None,
        }
    }

    #[test]
    fn a_failing_checkout_stays_with_its_own_source() {
        let transport = Recording::failing_for("https://example.com/bad.git");
        let good = source("https://example.com/good.git", None);
        let bad = source("https://example.com/bad.git", None);

        assert!(transport.fetch_checkout(&good, CacheRefresh::Fetch).is_ok());
        assert!(transport.fetch_checkout(&bad, CacheRefresh::Fetch).is_err());
        // The good one was still fetched; one bad Source does not undo it.
        assert_eq!(transport.fetches.lock().unwrap().len(), 1);
    }

    #[test]
    fn parallel_checkouts_stay_within_the_cap() {
        let transport = Recording::default();
        let sources: Vec<RemoteRef> = (0..12)
            .map(|i| source(&format!("https://example.com/r{i}.git"), None))
            .collect();

        let fetched = run_concurrent(sources, MAX_UPDATE_CONCURRENCY, |source| {
            transport.fetch_checkout(source, CacheRefresh::Fetch)
        });

        assert_eq!(fetched.len(), 12);
        assert!(
            transport.peak.load(Ordering::SeqCst) <= MAX_UPDATE_CONCURRENCY,
            "peak {} exceeded the cap",
            transport.peak.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn the_first_sibling_of_a_slot_refreshes_and_the_rest_reuse() {
        let plan = plan_fetches(&[
            source("https://example.com/a.git", Some("x")),
            source("https://example.com/a.git", Some("y")),
            source("https://example.com/a.git", None),
            source("https://example.com/b.git", None),
        ]);

        // Two subpath skills share one (Source, sparse) slot; the root skill uses
        // the other slot of the same Source; b is a different Source entirely.
        assert_eq!(plan, vec![
            CacheRefresh::Fetch,
            CacheRefresh::Reuse,
            CacheRefresh::Fetch,
            CacheRefresh::Fetch,
        ]);
    }
}
