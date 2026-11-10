//! The real service, over its Unix socket.
//!
//! One link thread owns the connection: it runs `client`'s [`machine`] (connecting, replacing a
//! service from another build or one that's stopping, starting one), and once connected holds
//! the long-lived connection that keeps the service running while the app is open (window closed
//! or not), fetching the list every second with the details of the torrent the app watches.
//! Nothing else connects, starts or replaces a service.
//!
//! A command is a message to the link with its completion ([ADR 0007]): held until connected
//! (also while a service that stopped or crashed is restarted), failed while lost or outdated.
//! Once connected the link opens it a connection of its own, on its own thread, so a .torrent URL
//! that takes a while to download never blocks pausing or the list; the thread queues the
//! command's completion, with the result, after the listing it fetches.
//! Threads queue [`Incoming`]s and call `wake` so the app drains them.
//!
//! [ADR 0007]: ../../../../docs/adr/0007-commands-answer-their-caller.md

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use client::machine::{self, Command, Do, Event, Observed, State, Why, World};
use client::{Client, Failure, Socket, display};
use proto::{AbsolutePath, AddSource, Added, Refusal, TorrentCommand, TorrentId};

use super::{AddError, AddRequest, Done, Incoming, Live, Service, answer};

const POLL_INTERVAL: Duration = Duration::from_secs(1);

pub struct SocketService {
    shared: Arc<Shared>,
    link: Sender<Job>,
}

/// A command, given the connection the link opened for it, or why there's none.
type Job = Box<dyn FnOnce(Result<Client, Failure>) + Send>;

struct Shared {
    incoming: Mutex<Vec<Incoming>>,
    /// The torrent whose details the link fetches.
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
    /// Starts the link thread, which starts the service if none is running.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::link(wake, |_| Link::new(State::Connecting, socket()))
    }

    /// As [`start`](Self::start), but connects and fetches the first list before returning, for
    /// headless runs whose virtual clock would otherwise snapshot before the list arrives.
    pub fn start_loaded(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::link(wake, |shared| {
            let mut link = Link::new(State::Connecting, socket());
            link.state = machine::settle(State::Connecting, &mut link.socket);
            link.connection = link.socket.take();
            link.show(shared);
            link.poll(shared);
            link
        })
    }

    fn link(wake: impl Fn() + Send + Sync + 'static, link: impl FnOnce(&Shared) -> Link) -> Self {
        let shared = Arc::new(Shared { incoming: Mutex::default(), watching: Mutex::default(), wake: Box::new(wake) });
        let (to_link, jobs) = mpsc::channel();
        let mut link = link(&shared);
        let on_link = shared.clone();
        thread::spawn(move || link.run(&on_link, &jobs));
        Self { shared, link: to_link }
    }

    /// Runs `op` on a connection of its own, then refreshes the list so its effect shows at once:
    /// queues the listing, then the live data `op` returns, then `done` with its result.
    fn run<T: Send + 'static>(
        &self,
        op: impl FnOnce(&mut Client) -> Result<(T, Option<Live>), Failure> + Send + 'static,
        done: Done<Result<T, Failure>>,
    ) {
        let shared = self.shared.clone();
        let job: Job = Box::new(move |connection| {
            let ran = connection.and_then(|mut c| {
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
        // The link only ends with the app.
        let _ = self.link.send(job);
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
        // is the link's: e.g. removed by another client, which the next listing shows.
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

fn socket() -> Socket {
    // Without our own executable's folder there's no service to start: every Spawn fails, saying so.
    Socket::new(&client::sibling_service_exe().unwrap_or_default())
}

fn add(c: &mut Client, request: &AddRequest, destination: Option<AbsolutePath>) -> Result<Added, Failure> {
    let source = match request {
        AddRequest::Url(url) => AddSource::Url(url.clone()),
        AddRequest::File(path) => AddSource::TorrentFile(std::fs::read(path).with_context(|| format!("can't read {}", path.display()))?),
    };
    c.add(source, false, destination)
}

/// The link thread's own: the machine's state, the socket it runs through, and once connected
/// the connection that keeps the service running.
struct Link {
    state: State,
    socket: Socket,
    connection: Option<Client>,
    /// Commands that came before the link connected.
    held: Vec<Job>,
    /// The state the app was last told.
    shown: Option<State>,
}

impl Link {
    fn new(state: State, socket: Socket) -> Self {
        Self { state, socket, connection: None, held: Vec::new(), shown: None }
    }

    /// Runs the link until the app ends: turns of the machine, and once connected the list every
    /// [`POLL_INTERVAL`] and commands as they come.
    fn run(&mut self, shared: &Shared, jobs: &Receiver<Job>) {
        let mut poll_at = Instant::now();
        loop {
            self.show(shared);
            let connected = matches!(self.state, State::Connected { .. });
            if connected {
                self.held.drain(..).for_each(start);
            }
            let now = Instant::now();
            let until = match machine::next(&self.state, now) {
                Some(Do::Wait(until)) => Some(until),
                Some(act) => {
                    let what = self.socket.run(&act);
                    self.observe(what);
                    continue;
                }
                // Outdated, for good: only commands to refuse.
                None if !connected => None,
                None if now >= poll_at => {
                    self.poll(shared);
                    poll_at = now + POLL_INTERVAL;
                    continue;
                }
                None => Some(poll_at),
            };
            let received = match until {
                Some(until) => jobs.recv_timeout(until.saturating_duration_since(now)),
                None => jobs.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(job) => match machine::decide(&self.state, &Event::Command(Command::Request)) {
                    Ok(_) if connected => start(job),
                    Ok(_) => self.held.push(job),
                    Err(why) => job(Err(lost(&why))),
                },
                Err(RecvTimeoutError::Timeout) if connected => {}
                Err(RecvTimeoutError::Timeout) => self.observe(Observed::Tick),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Fetches the list, and the watched torrent's details, on the link's connection; losing it is
    /// what the machine observes.
    fn poll(&mut self, shared: &Shared) {
        let Some(c) = &mut self.connection else { return };
        let watching = *shared.watching.lock().unwrap();
        match c.list() {
            Ok(listing) => {
                shared.live(Live::Listing(listing));
                // E.g. removed by another client: the next listing closes the pane.
                if let Some(Ok(details)) = watching.map(|id| c.details(id)) {
                    shared.live(Live::Details(details));
                }
            }
            Err(Failure::Refused(Refusal::Stopping)) => self.observe(Observed::Going),
            Err(_) => self.observe(Observed::Closed),
        }
    }

    fn observe(&mut self, what: Observed) {
        self.state = machine::decide(&self.state, &Event::Observed { what, at: Instant::now() }).expect("observations are never refused");
        match &self.state {
            State::Connected { .. } => self.connection = self.connection.take().or_else(|| self.socket.take()),
            State::Lost { why, .. } => {
                self.connection = None;
                self.held.drain(..).for_each(|job| job(Err(lost(why))));
            }
            State::Outdated => {
                self.connection = None;
                self.held.drain(..).for_each(|job| job(Err(lost(&Why::Outdated))));
            }
            State::Connecting | State::Replacing { .. } | State::Spawning { .. } => self.connection = None,
        }
    }

    /// Tells the app the state, when it changed.
    fn show(&mut self, shared: &Shared) {
        if self.shown.as_ref() != Some(&self.state) {
            self.shown = Some(self.state.clone());
            shared.live(Live::Connection(self.state.clone()));
        }
    }
}

/// Runs a command on a connection the link opens for it, on a thread of its own.
fn start(job: Job) {
    match Client::open() {
        Ok(connection) => drop(thread::spawn(move || job(Ok(connection)))),
        Err(e) => job(Err(e.into())),
    }
}

fn lost(why: &Why) -> Failure {
    anyhow!("{}", display::lost(why)).into()
}
