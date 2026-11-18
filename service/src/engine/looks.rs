//! Looking at what lives on a disk without ever waiting for one (`statfs`, `read_dir` and `stat`
//! can hang on a network volume that stops answering): one watcher for every kind of look
//! ([`super::space`] for each destination's disk, [`super::default_folder`] for the default
//! folder, `actor::letgo` for the files a Move to Trash took that are gone from the Trash).
//!
//! [`watch`] looks at each key it's asked about every `every`, and at once when the keys change,
//! each look in a thread of its own, at most one at a time per key, and tells `report` what it
//! found. A look that has taken longer than [`STALLED_AFTER`] is reported [`Looked::Stalled`],
//! each tick, until it answers. A stalled report never overtakes the answer it's about: `report`
//! is called with the bookkeeping locked, by the tick and by the look's task alike, so once a look
//! has answered, nothing says it's stalled.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{oneshot, watch as channel};

use super::space::STALLED_AFTER;

/// What a look came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Looked<T> {
    Read(T),
    /// The look has taken longer than [`STALLED_AFTER`], and is still going.
    Stalled,
}

/// Looks at each key `wanted` says (with the input to look with) as above. Ends when `wanted`'s
/// sender goes, or `report` says nobody's listening (`false`). `report` must not block.
pub fn watch<K, I, T>(
    look: Arc<dyn Fn(&K, &I) -> T + Send + Sync>,
    mut wanted: channel::Receiver<BTreeMap<K, I>>,
    every: Duration,
    report: impl Fn(K, Looked<T>) -> bool + Send + Sync + 'static,
) where
    K: Ord + Clone + Send + Sync + 'static,
    I: Clone + Send + Sync + 'static,
    T: Send + 'static,
{
    let report = Arc::new(report);
    // The keys being looked at, since when.
    let looking: Arc<Mutex<BTreeMap<K, Instant>>> = Arc::new(Mutex::new(BTreeMap::new()));
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                changed = wanted.changed() => if changed.is_err() { return },
            }
            let now = wanted.borrow_and_update().clone();
            for (key, input) in now {
                let mut looking_now = looking.lock().unwrap();
                if let Some(since) = looking_now.get(&key).copied() {
                    if since.elapsed() >= STALLED_AFTER && !report(key, Looked::Stalled) {
                        return;
                    }
                    continue;
                }
                looking_now.insert(key.clone(), Instant::now());
                drop(looking_now);
                let (found, result) = oneshot::channel();
                let (look, at, with) = (look.clone(), key.clone(), input);
                // Not tokio's blocking pool: a look that never returns mustn't hold up its shutdown.
                std::thread::spawn(move || {
                    let _ = found.send(look(&at, &with));
                });
                let (report, looking) = (report.clone(), looking.clone());
                tokio::spawn(async move {
                    let result = result.await;
                    // Under the lock the tick reports stalled by: it's over, then it's said.
                    let mut looking = looking.lock().unwrap();
                    looking.remove(&key);
                    if let Ok(found) = result {
                        report(key, Looked::Read(found));
                    }
                });
            }
        }
    });
}
