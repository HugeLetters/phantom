//! The real service, over its Unix socket.
//!
//! A poll thread holds one long-lived connection, which is what keeps the service running while
//! the app is open (window closed or not), and fetches the list every second, with the details
//! of the torrent the app watches. Each command runs on its own short-lived connection, so a
//! .torrent URL that takes a while to download never blocks pausing or the list; its thread
//! carries the command's completion and queues it, with the result, after the listing it fetches.
//! Threads queue [`Incoming`]s and call `wake` so the app drains them.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use client::{Client, Failure};
use proto::{AbsolutePath, AddSource, Added, Refusal, TorrentCommand, TorrentId};

use super::{AddError, AddRequest, Connection, Done, Incoming, Live, Service, answer};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

pub struct SocketService {
    shared: Arc<Shared>,
}

struct Shared {
    incoming: Mutex<Vec<Incoming>>,
    /// The torrent whose details the poll thread fetches.
    watching: Mutex<Option<TorrentId>>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn push(&self, incoming: Incoming) {
        self.incoming.lock().unwrap().push(incoming);
        (self.wake)();
    }

    fn live(&self, live: Live) {
        self.push(Incoming::Live(live));
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
        match connect().and_then(|mut c| Ok(c.list()?)) {
            Ok(listing) => {
                service.shared.live(Live::Connection(Connection::Connected));
                service.shared.live(Live::Listing(listing));
            }
            Err(e) => service.shared.live(Live::Connection(Connection::Lost(format!("{e:#}")))),
        }
        // Only now: the poller starting a service at the same time would race this one.
        service.spawn_poller();
        service
    }

    fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self { shared: Arc::new(Shared { incoming: Mutex::default(), watching: Mutex::default(), wake: Box::new(wake) }) }
    }

    fn spawn_poller(&self) {
        let shared = self.shared.clone();
        thread::spawn(move || poll_forever(&shared));
    }

    /// Runs `op` on a fresh connection, then refreshes the list so its effect shows at once:
    /// queues the listing, then the live data `op` returns, then `done` with its result.
    fn run<T: Send + 'static>(
        &self,
        op: impl FnOnce(&mut Client) -> Result<(T, Option<Live>), Failure> + Send + 'static,
        done: Done<Result<T, Failure>>,
    ) {
        let shared = self.shared.clone();
        thread::spawn(move || {
            // Connect or start: a magnet link that launches the app arrives before the poller
            // has brought the service up.
            let ran = connect().map_err(Failure::from).and_then(|mut c| {
                let (result, live) = op(&mut c)?;
                Ok((result, live, c.list()?))
            });
            match ran {
                Ok((result, live, listing)) => {
                    shared.live(Live::Listing(listing));
                    live.into_iter().for_each(|live| shared.live(live));
                    shared.push(answer(done, Ok(result)));
                }
                Err(e) => shared.push(answer(done, Err(e))),
            }
        });
    }
}

impl Service for SocketService {
    /// The first refused for want of a destination answers for the rest too, unsent: they'd be
    /// refused as well. Failing to reach the service fails the add as a whole: one failure.
    fn add(&mut self, sources: Vec<AddRequest>, destination: Option<AbsolutePath>, done: Done<Vec<Result<Added, AddError>>>) {
        let op = move |c: &mut Client| {
            let mut results = Vec::new();
            let mut sources = sources.into_iter();
            while let Some(source) = sources.next() {
                match add(c, &source, destination.clone()) {
                    Ok(added) => results.push(Ok(added)),
                    Err(Failure::Refused(Refusal::NoDestination)) => {
                        results.extend([source].into_iter().chain(sources).map(|s| Err(AddError::NoDestination(s))));
                        break;
                    }
                    Err(e) => results.push(Err(AddError::Failed(e))),
                }
            }
            Ok((results, None))
        };
        self.run(op, Box::new(move |s, ran, tx| done(s, ran.unwrap_or_else(|e| vec![Err(AddError::Failed(e))]), tx)));
    }

    fn command(&mut self, ids: &[TorrentId], command: TorrentCommand, done: Done<Result<(), Failure>>) {
        let ids = ids.to_vec();
        let op = move |c: &mut Client| {
            ids.iter().try_for_each(|&id| c.command(id, command.clone()))?;
            // The files' new priorities, at once.
            match (&command, &ids[..]) {
                (TorrentCommand::SetPriority { .. }, [id]) => Ok(((), Some(Live::Details(c.details(*id)?)))),
                _ => Ok(((), None)),
            }
        };
        self.run(op, done);
    }

    fn watch_details(&mut self, id: Option<TorrentId>) {
        *self.shared.watching.lock().unwrap() = id;
        // Now rather than at the next tick, so a selected torrent's files show at once. A failure
        // is the poll thread's: e.g. removed by another client, which the next listing shows.
        if let Some(id) = id {
            self.run(move |c| Ok(((), Some(Live::Details(c.details(id)?)))), Box::new(|_, _, _| {}));
        }
    }

    fn poll(&mut self, _now: f64) -> Vec<Incoming> {
        std::mem::take(&mut *self.shared.incoming.lock().unwrap())
    }

    fn next_wake(&self) -> Option<Duration> {
        None
    }
}

fn connect() -> anyhow::Result<Client> {
    client::sibling_service_exe().and_then(|exe| Client::connect_or_spawn(&exe))
}

fn add(c: &mut Client, request: &AddRequest, destination: Option<AbsolutePath>) -> Result<Added, Failure> {
    let source = match request {
        AddRequest::Url(url) => AddSource::Url(url.clone()),
        AddRequest::File(path) => AddSource::TorrentFile(std::fs::read(path).with_context(|| format!("can't read {}", path.display()))?),
    };
    c.add(source, false, destination)
}

fn poll_forever(shared: &Shared) {
    loop {
        let client = connect();
        let mut client = match client {
            Ok(client) => client,
            Err(e) => {
                shared.live(Live::Connection(Connection::Lost(format!("{e:#}"))));
                thread::sleep(RECONNECT_DELAY);
                continue;
            }
        };
        shared.live(Live::Connection(Connection::Connected));
        loop {
            let watching = *shared.watching.lock().unwrap();
            let polled = client.list().map(|listing| (listing, watching.map(|id| client.details(id)).transpose()));
            match polled {
                Ok((listing, details)) => {
                    shared.live(Live::Listing(listing));
                    match details {
                        Ok(Some(details)) => shared.live(Live::Details(details)),
                        // E.g. removed by another client: the next listing closes the pane.
                        Ok(None) | Err(_) => {}
                    }
                }
                Err(e) => {
                    shared.live(Live::Connection(Connection::Lost(format!("{e:#}"))));
                    break;
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
        thread::sleep(RECONNECT_DELAY);
    }
}
