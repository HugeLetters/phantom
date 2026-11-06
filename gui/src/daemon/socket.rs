//! The real daemon, over its Unix socket.
//!
//! A poll thread holds one long-lived connection, which is what keeps the daemon running while
//! the app is open (window closed or not), and fetches the list every second. Each action runs on
//! its own short-lived connection, so a magnet that takes a minute to resolve never blocks
//! pausing or the list. Threads queue [`Event`]s and call `wake` so the app drains them.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use proto::{AddSource, Client, PendingId, TorrentId};

use super::{AddRequest, Connection, Daemon, Event, Notice};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

pub struct SocketDaemon {
    shared: Arc<Shared>,
}

struct Shared {
    events: Mutex<Vec<Event>>,
    in_flight: AtomicUsize,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn push(&self, event: Event) {
        self.events.lock().unwrap().push(event);
        (self.wake)();
    }
}

impl SocketDaemon {
    /// Starts the poll thread, which starts the daemon if none is running.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let daemon = Self::new(wake);
        daemon.spawn_poller();
        daemon
    }

    /// As [`start`](Self::start), but fetches the first list before returning, for headless
    /// runs whose virtual clock would otherwise snapshot before the list arrives.
    pub fn start_loaded(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let daemon = Self::new(wake);
        match proto::sibling_daemon_exe().and_then(|exe| Client::connect_or_spawn(&exe)?.list()) {
            Ok(listing) => {
                daemon.shared.push(Event::Connection(Connection::Connected));
                daemon.shared.push(Event::Listing(listing));
            }
            Err(e) => daemon.shared.push(Event::Connection(Connection::Lost(format!("{e:#}")))),
        }
        // Only now: the poller starting a daemon at the same time would race this one.
        daemon.spawn_poller();
        daemon
    }

    fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(Shared { events: Mutex::default(), in_flight: AtomicUsize::new(0), wake: Box::new(wake) }),
        }
    }

    fn spawn_poller(&self) {
        let shared = self.shared.clone();
        thread::spawn(move || poll_forever(&shared));
    }

    /// Runs `op` on a fresh connection, then refreshes the list so the result shows at once.
    /// `op` may return a message to show.
    fn run(&self, op: impl FnOnce(&mut Client) -> Result<Option<String>> + Send + 'static) {
        let shared = self.shared.clone();
        shared.in_flight.fetch_add(1, Ordering::SeqCst);
        thread::spawn(move || {
            // Connect or start: a magnet link that launches the app arrives before the poller
            // has brought the daemon up.
            let client = proto::sibling_daemon_exe().and_then(|exe| Client::connect_or_spawn(&exe));
            let result = client.and_then(|mut c| {
                let message = op(&mut c)?;
                Ok((message, c.list()?))
            });
            shared.in_flight.fetch_sub(1, Ordering::SeqCst);
            match result {
                Ok((message, listing)) => {
                    shared.push(Event::Listing(listing));
                    if let Some(message) = message {
                        shared.push(Event::Notice(Notice::info(message)));
                    }
                }
                Err(e) => shared.push(Event::Notice(Notice::error(format!("{e:#}")))),
            }
        });
    }
}

impl Daemon for SocketDaemon {
    fn add(&mut self, source: AddRequest) {
        self.run(move |c| {
            let source = match source {
                AddRequest::Url(url) => AddSource::Url(url),
                AddRequest::File(path) => AddSource::TorrentFile(
                    std::fs::read(&path).with_context(|| format!("can't read {}", path.display()))?,
                ),
            };
            added(c.add(source, false))
        });
    }

    fn pause(&mut self, ids: &[TorrentId]) {
        let ids = ids.to_vec();
        self.run(move |c| {
            ids.iter().try_for_each(|&id| c.pause(id))?;
            Ok(None)
        });
    }

    fn resume(&mut self, ids: &[TorrentId]) {
        let ids = ids.to_vec();
        self.run(move |c| {
            ids.iter().try_for_each(|&id| c.resume(id))?;
            Ok(None)
        });
    }

    fn remove(&mut self, ids: &[TorrentId], delete_files: bool) {
        let ids = ids.to_vec();
        self.run(move |c| {
            ids.iter().try_for_each(|&id| c.remove(id, delete_files))?;
            Ok(None)
        });
    }

    fn cancel_adds(&mut self, ids: &[PendingId]) {
        let ids = ids.to_vec();
        self.run(move |c| {
            ids.iter().try_for_each(|&id| c.cancel_add(id))?;
            Ok(None)
        });
    }

    fn poll(&mut self, _now: f64) -> Vec<Event> {
        std::mem::take(&mut *self.shared.events.lock().unwrap())
    }

    fn in_flight(&self) -> usize {
        self.shared.in_flight.load(Ordering::SeqCst)
    }

    fn next_wake(&self) -> Option<Duration> {
        None
    }
}

fn added(result: Result<(TorrentId, Option<String>, bool)>) -> Result<Option<String>> {
    let (_, name, already_existed) = match result {
        // Someone stopped it with CancelAdd; not an error.
        Err(e) if e.to_string() == "cancelled" => return Ok(Some("Stopped adding".into())),
        other => other?,
    };
    let name = name.unwrap_or_else(|| "torrent".into());
    Ok(Some(if already_existed { format!("{name} is already in the list") } else { format!("Added {name}") }))
}

fn poll_forever(shared: &Shared) {
    loop {
        let client = proto::sibling_daemon_exe().and_then(|exe| Client::connect_or_spawn(&exe));
        let mut client = match client {
            Ok(client) => client,
            Err(e) => {
                shared.push(Event::Connection(Connection::Lost(format!("{e:#}"))));
                thread::sleep(RECONNECT_DELAY);
                continue;
            }
        };
        shared.push(Event::Connection(Connection::Connected));
        loop {
            match client.list() {
                Ok(listing) => shared.push(Event::Listing(listing)),
                Err(e) => {
                    shared.push(Event::Connection(Connection::Lost(format!("{e:#}"))));
                    break;
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
        thread::sleep(RECONNECT_DELAY);
    }
}
