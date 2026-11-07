//! Adds still in flight, mostly magnets waiting for metadata from peers. librqbit only lists a
//! torrent once that arrives, so without this a slow magnet is invisible and can't be stopped.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;

use librqbit::Magnet;
use proto::{AddSource, PendingAdd, PendingId};
use tokio::task::AbortHandle;

#[derive(Default)]
pub struct PendingAdds {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    next: PendingId,
    entries: BTreeMap<PendingId, Entry>,
}

struct Entry {
    name: Option<String>,
    source: String,
    started: Instant,
    abort: Option<AbortHandle>,
}

impl PendingAdds {
    pub fn begin(&self, source: &AddSource) -> PendingId {
        let (name, source) = describe(source);
        let mut inner = self.inner.lock().unwrap();
        let id = inner.next;
        inner.next += 1;
        inner.entries.insert(id, Entry { name, source, started: Instant::now(), abort: None });
        id
    }

    /// The add may already have finished, in which case there is nothing left to abort.
    pub fn set_abort(&self, id: PendingId, abort: AbortHandle) {
        if let Some(entry) = self.inner.lock().unwrap().entries.get_mut(&id) {
            entry.abort = Some(abort);
        }
    }

    pub fn finish(&self, id: PendingId) {
        self.inner.lock().unwrap().entries.remove(&id);
    }

    /// Returns false if no such add is in flight (it may have just finished).
    pub fn cancel(&self, id: PendingId) -> bool {
        let Some(entry) = self.inner.lock().unwrap().entries.remove(&id) else { return false };
        if let Some(abort) = entry.abort {
            abort.abort();
        }
        true
    }

    pub fn list(&self) -> Vec<PendingAdd> {
        let inner = self.inner.lock().unwrap();
        inner
            .entries
            .iter()
            .map(|(&id, e)| PendingAdd {
                id,
                name: e.name.clone(),
                source: e.source.clone(),
                elapsed_secs: e.started.elapsed().as_secs(),
            })
            .collect()
    }
}

fn describe(source: &AddSource) -> (Option<String>, String) {
    match source {
        AddSource::Url(url) if url.starts_with("magnet:") => (Magnet::parse(url).ok().and_then(|m| m.name), url.clone()),
        AddSource::Url(url) => {
            let file = url.split(['?', '#']).next().and_then(|u| u.rsplit('/').next()).filter(|f| !f.is_empty());
            (file.map(|f| f.trim_end_matches(".torrent").to_owned()), url.clone())
        }
        AddSource::TorrentFile(_) => (None, "torrent file".into()),
    }
}
