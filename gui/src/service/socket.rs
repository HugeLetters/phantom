//! The real service, over its Unix socket.
//!
//! A poll thread holds one long-lived connection, which is what keeps the service running while
//! the app is open (window closed or not), and fetches the list every second, with the details
//! of the torrent the app watches. Each action runs on
//! its own short-lived connection, so a .torrent URL that takes a while to download never
//! blocks pausing or the list. Threads queue [`Event`]s and call `wake` so the app drains them.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use client::{Client, Refused};
use proto::{AbsolutePath, AddSource, Priority, Refusal, TorrentId};

use super::{AddRequest, Connection, Event, Notice, Service};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

pub struct SocketService {
    shared: Arc<Shared>,
}

struct Shared {
    events: Mutex<Vec<Event>>,
    /// The torrent whose details the poll thread fetches.
    watching: Mutex<Option<TorrentId>>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn push(&self, event: Event) {
        self.events.lock().unwrap().push(event);
        (self.wake)();
    }
}

impl SocketService {
    /// Starts the poll thread, which starts the service if none is running.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let service = Self::new(wake);
        service.spawn_poller();
        service
    }

    /// As [`start`](Self::start), but fetches the first list before returning, for headless
    /// runs whose virtual clock would otherwise snapshot before the list arrives.
    pub fn start_loaded(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let service = Self::new(wake);
        match client::sibling_service_exe().and_then(|exe| Client::connect_or_spawn(&exe)?.list()) {
            Ok(listing) => {
                service.shared.push(Event::Connection(Connection::Connected));
                service.shared.push(Event::Listing(listing));
            }
            Err(e) => service.shared.push(Event::Connection(Connection::Lost(format!("{e:#}")))),
        }
        // Only now: the poller starting a service at the same time would race this one.
        service.spawn_poller();
        service
    }

    fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(Shared {
                events: Mutex::default(),
                watching: Mutex::default(),
                wake: Box::new(wake) }),
        }
    }

    fn spawn_poller(&self) {
        let shared = self.shared.clone();
        thread::spawn(move || poll_forever(&shared));
    }

    /// Runs `op` on a fresh connection, then refreshes the list so the result shows at once.
    /// `op` may return events to send after the list.
    fn run<E: IntoIterator<Item = Event>>(&self, op: impl FnOnce(&mut Client) -> Result<E> + Send + 'static) {
        let shared = self.shared.clone();
        thread::spawn(move || {
            // Connect or start: a magnet link that launches the app arrives before the poller
            // has brought the service up.
            let client = client::sibling_service_exe().and_then(|exe| Client::connect_or_spawn(&exe));
            let result = client.and_then(|mut c| {
                let message = op(&mut c)?;
                Ok((message, c.list()?))
            });
            match result {
                Ok((events, listing)) => {
                    shared.push(Event::Listing(listing));
                    events.into_iter().for_each(|event| shared.push(event));
                }
                Err(e) => shared.push(Event::Notice(Notice::error(format!("{e:#}")))),
            }
        });
    }
}

impl Service for SocketService {
    /// The first refused for want of a destination hands back the rest with it: they'd be too.
    fn add(&mut self, sources: Vec<AddRequest>, destination: Option<AbsolutePath>) {
        self.run(move |c| {
            let mut events = Vec::new();
            for (i, request) in sources.iter().enumerate() {
                match add(c, request, destination.clone()) {
                    Ok(added) => events.push(Event::Added(added)),
                    Err(e) if e.downcast_ref::<Refused>() == Some(&Refused(Refusal::NoDestination)) => {
                        events.push(Event::NoDestination(sources[i..].to_vec()));
                        break;
                    }
                    Err(e) => events.push(Event::Notice(Notice::error(format!("{e:#}")))),
                }
            }
            Ok(events)
        });
    }

    fn pause(&mut self, ids: &[TorrentId]) {
        let ids = ids.to_vec();
        self.run(move |c| {
            ids.iter().try_for_each(|&id| c.pause(id))?;
            Ok(None::<Event>)
        });
    }

    fn resume(&mut self, ids: &[TorrentId]) {
        let ids = ids.to_vec();
        self.run(move |c| {
            ids.iter().try_for_each(|&id| c.resume(id))?;
            Ok(None::<Event>)
        });
    }

    fn check(&mut self, ids: &[TorrentId]) {
        let ids = ids.to_vec();
        self.run(move |c| {
            ids.iter().try_for_each(|&id| c.check(id))?;
            Ok(None::<Event>)
        });
    }

    fn remove(&mut self, ids: &[TorrentId]) {
        let ids = ids.to_vec();
        self.run(move |c| {
            ids.iter().try_for_each(|&id| c.remove(id))?;
            Ok(None::<Event>)
        });
    }

    fn delete(&mut self, ids: &[TorrentId]) {
        let ids = ids.to_vec();
        self.run(move |c| {
            ids.iter().try_for_each(|&id| c.delete(id))?;
            Ok(None::<Event>)
        });
    }

    fn watch_details(&mut self, id: Option<TorrentId>) {
        *self.shared.watching.lock().unwrap() = id;
        // Now rather than at the next tick, so a selected torrent's files show at once.
        if let Some(id) = id {
            self.run(move |c| Ok(Some(Event::Details(c.details(id)?))));
        }
    }

    fn set_priority(&mut self, id: TorrentId, files: &[usize], priority: Priority) {
        let files = files.to_vec();
        self.run(move |c| {
            c.set_priority(id, &files, priority)?;
            Ok(Some(Event::Details(c.details(id)?)))
        });
    }

    fn poll(&mut self, _now: f64) -> Vec<Event> {
        std::mem::take(&mut *self.shared.events.lock().unwrap())
    }

    fn next_wake(&self) -> Option<Duration> {
        None
    }
}

fn add(c: &mut Client, request: &AddRequest, destination: Option<AbsolutePath>) -> Result<proto::Added> {
    let source = match request {
        AddRequest::Url(url) => AddSource::Url(url.clone()),
        AddRequest::File(path) => {
            AddSource::TorrentFile(std::fs::read(path).with_context(|| format!("can't read {}", path.display()))?)
        }
    };
    c.add(source, false, destination)
}

fn poll_forever(shared: &Shared) {
    loop {
        let client = client::sibling_service_exe().and_then(|exe| Client::connect_or_spawn(&exe));
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
            let watching = *shared.watching.lock().unwrap();
            let polled = client.list().map(|listing| (listing, watching.map(|id| client.details(id)).transpose()));
            match polled {
                Ok((listing, details)) => {
                    shared.push(Event::Listing(listing));
                    match details {
                        Ok(Some(details)) => shared.push(Event::Details(details)),
                        // E.g. removed by another client: the next listing closes the pane.
                        Ok(None) | Err(_) => {}
                    }
                }
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
