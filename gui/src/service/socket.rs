//! The real service, over its Unix socket.
//!
//! One link thread owns the connection: it runs `client`'s [`machine`] (connecting, replacing a
//! service from another build or one that's stopping, starting one), and once connected
//! subscribes on the long-lived connection that keeps the service running while the app is open
//! (window closed or not). A reader thread forwards each snapshot the service pushes, with the
//! details of the torrents the app watches; the stream ending is how the link learns the service
//! went. Nothing polls, and nothing else connects, starts or replaces a service.
//!
//! A command is a message to the link with its completion ([ADR 0007]): held until connected
//! (also while a service that stopped or crashed is restarted), failed while lost or outdated.
//! Once connected the link opens it a connection of its own, on its own thread, so a .torrent URL
//! that takes a while to download never blocks pausing or the list; the thread queues the
//! command's completion, with the result, after a snapshot it takes on that connection, so the
//! answer's effect shows first (the app keeps the later of two snapshots).
//! Threads queue [`Incoming`]s and call `wake` so the app drains them.
//!
//! [ADR 0007]: ../../../../docs/adr/0007-commands-answer-their-caller.md

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use client::machine::{self, Command, Do, Event, Observed, State, Why, World};
use client::{Client, Failure, Socket, Watcher, display};
use proto::{AbsolutePath, AddSource, Added, Refusal, Setting, Settings, TorrentCommand, TorrentId};

use super::{AddError, AddRequest, Done, Incoming, Live, Service, answer};
use crate::model::Phantom;
use rune::Tx;

pub struct SocketService {
    shared: Arc<Shared>,
    link: Sender<ToLink>,
}

/// A command, given the connection the link opened for it, or why there's none.
type Job = Box<dyn FnOnce(Result<Client, Failure>) + Send>;

/// What the link hears.
enum ToLink {
    Job(Job),
    /// The `n`-th subscription's stream ended, as this says.
    Ended {
        n: u64,
        what: Observed,
    },
}

struct Shared {
    incoming: Mutex<Vec<Incoming>>,
    /// The torrents whose details the snapshots carry.
    watching: Mutex<Vec<TorrentId>>,
    /// Changes what the subscription watches, while there is one.
    watcher: Mutex<Option<Watcher>>,
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

    /// As [`start`](Self::start), but connects and takes the first snapshot before returning, for
    /// headless runs whose virtual clock would otherwise draw before the list arrives.
    pub fn start_loaded(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::link(wake, |shared| {
            let mut link = Link::new(State::Connecting, socket());
            link.state = machine::settle(State::Connecting, &mut link.socket);
            link.connection = link.socket.take();
            link.show(shared);
            if let Ok(snapshot) = Client::open().and_then(|c| Ok(c.snapshot(Vec::new())?)) {
                shared.live(Live::Snapshot(snapshot));
            }
            link
        })
    }

    fn link(wake: impl Fn() + Send + Sync + 'static, link: impl FnOnce(&Shared) -> Link) -> Self {
        let shared =
            Arc::new(Shared { incoming: Mutex::default(), watching: Mutex::default(), watcher: Mutex::default(), wake: Box::new(wake) });
        let (to_link, heard) = mpsc::channel();
        let mut link = link(&shared);
        let on_link = shared.clone();
        let ended = to_link.clone();
        thread::spawn(move || link.run(&on_link, &heard, &ended));
        Self { shared, link: to_link }
    }

    /// Runs `op` on a connection of its own, then takes a snapshot on it so its effect shows at
    /// once: queues the snapshot, then `done` with its result.
    fn run<T: Send + 'static>(&self, op: impl FnOnce(&mut Client) -> Result<T, Failure> + Send + 'static, done: Done<Result<T, Failure>>) {
        let shared = self.shared.clone();
        let job: Job = Box::new(move |connection| {
            let ran = connection.and_then(|mut c| {
                let result = op(&mut c)?;
                let watching = shared.watching.lock().unwrap().clone();
                Ok((result, c.subscribe(watching)?.receive()?))
            });
            match ran {
                Ok((result, snapshot)) => {
                    shared.live(Live::Snapshot(snapshot));
                    shared.push(answer(done, Ok(result)));
                }
                Err(e) => shared.push(answer(done, Err(e))),
            }
        });
        // The link only ends with the app.
        let _ = self.link.send(ToLink::Job(job));
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
            Ok(results)
        };
        self.run(op, Box::new(move |s, ran, tx| done(s, ran.unwrap_or_else(|e| vec![Err(AddError::Failed(e))]), tx)));
    }

    fn command(&mut self, ids: &[TorrentId], command: TorrentCommand, done: Done<Vec<(TorrentId, Failure)>>) {
        let (ids, all) = (ids.to_vec(), ids.to_vec());
        // Every one, whatever the others did. The snapshot after it has the files' new priorities
        // too, as it watches them.
        let each = move |c: &mut Client| -> Result<Vec<(TorrentId, Failure)>, Failure> {
            Ok(ids.iter().filter_map(|&id| c.command(id, command.clone()).err().map(|e| (id, e))).collect())
        };
        self.run(
            each,
            Box::new(move |s: &mut Phantom, result: Result<Vec<(TorrentId, Failure)>, Failure>, tx: &mut Tx| {
                // Not reaching the service: every one failed alike.
                let failed = result.unwrap_or_else(|e| all.iter().map(|&id| (id, Failure::Error(anyhow!("{e:#}")))).collect());
                done(s, failed, tx)
            }),
        );
    }

    fn settings(&mut self, done: Done<Result<Settings, Failure>>) {
        self.run(|c| c.settings(), done);
    }

    fn set_setting(&mut self, setting: Setting, done: Done<Result<Settings, Failure>>) {
        self.run(move |c| c.set_setting(setting), done);
    }

    /// The service answers with a snapshot at once. A failure is the subscription's, which then
    /// ends; the next one watches these.
    fn watch_details(&mut self, ids: Vec<TorrentId>) {
        let mut watching = self.shared.watching.lock().unwrap();
        *watching = ids.clone();
        if let Some(watcher) = &mut *self.shared.watcher.lock().unwrap() {
            let _ = watcher.watch(ids);
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
        AddRequest::Link { source, .. } => source.clone(),
        AddRequest::File(path) => AddSource::TorrentFile(std::fs::read(path).with_context(|| format!("can't read {}", path.display()))?),
    };
    c.add(source, false, destination)
}

/// The link thread's own: the machine's state, the socket it runs through, and once connected
/// the connection that keeps the service running, until it subscribes on it.
struct Link {
    state: State,
    socket: Socket,
    connection: Option<Client>,
    /// Its connection is the subscription numbered this, whose reader thread tells when it ends.
    subscribed: Option<u64>,
    /// Subscriptions so far.
    subscriptions: u64,
    /// Commands that came before the link connected.
    held: Vec<Job>,
    /// The state the app was last told.
    shown: Option<State>,
}

impl Link {
    fn new(state: State, socket: Socket) -> Self {
        Self { state, socket, connection: None, subscribed: None, subscriptions: 0, held: Vec::new(), shown: None }
    }

    /// Runs the link until the app ends: turns of the machine, and once connected the
    /// subscription and commands as they come.
    fn run(&mut self, shared: &Arc<Shared>, heard: &Receiver<ToLink>, ended: &Sender<ToLink>) {
        loop {
            self.show(shared);
            let connected = matches!(self.state, State::Connected { .. });
            if connected {
                self.subscribe(shared, ended);
                self.held.drain(..).for_each(start);
            }
            let now = Instant::now();
            let until = match machine::next(&self.state, now) {
                Some(Do::Wait(until)) => Some(until),
                Some(act) => {
                    let what = self.socket.run(&act);
                    self.observe(shared, what);
                    continue;
                }
                // Connected, or outdated for good: waiting for what comes.
                None => None,
            };
            let received = match until {
                Some(until) => heard.recv_timeout(until.saturating_duration_since(now)),
                None => heard.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(ToLink::Job(job)) => match machine::decide(&self.state, &Event::Command(Command::Request)) {
                    Ok(_) if connected => start(job),
                    Ok(_) => self.held.push(job),
                    Err(why) => job(Err(lost(&why))),
                },
                // An older one's end, closed when the link was no longer connected, is no news.
                Ok(ToLink::Ended { n, what }) if self.subscribed == Some(n) => {
                    self.subscribed = None;
                    self.observe(shared, what);
                }
                Ok(ToLink::Ended { .. }) => {}
                Err(RecvTimeoutError::Timeout) => self.observe(shared, Observed::Tick),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Subscribes on the link's connection, unless it has; a reader thread forwards the snapshots
    /// and tells the link when the stream ends: the service going, or gone.
    fn subscribe(&mut self, shared: &Arc<Shared>, ended: &Sender<ToLink>) {
        if self.subscribed.is_some() {
            return;
        }
        let Some(connection) = self.connection.take() else { return };
        // With what the app watches now (after a reconnect too), held until the watcher is in
        // place, so a change meanwhile isn't lost between the two.
        let watching = shared.watching.lock().unwrap();
        let mut snapshots = match connection.subscribe(watching.clone()) {
            Ok(snapshots) => snapshots,
            Err(_) => {
                drop(watching);
                return self.observe(shared, Observed::Closed);
            }
        };
        *shared.watcher.lock().unwrap() = snapshots.watcher().ok();
        drop(watching);
        self.subscriptions += 1;
        let n = self.subscriptions;
        self.subscribed = Some(n);
        let (shared, ended) = (shared.clone(), ended.clone());
        thread::spawn(move || {
            let what = loop {
                match snapshots.receive() {
                    Ok(snapshot) => shared.live(Live::Snapshot(snapshot)),
                    Err(Failure::Refused(Refusal::Stopping)) => break Observed::Going,
                    Err(_) => break Observed::Closed,
                }
            };
            let _ = ended.send(ToLink::Ended { n, what });
        });
    }

    fn observe(&mut self, shared: &Shared, what: Observed) {
        self.state = machine::decide(&self.state, &Event::Observed { what, at: Instant::now() }).expect("observations are never refused");
        match &self.state {
            State::Connected { .. } => {
                if self.subscribed.is_none() {
                    self.connection = self.connection.take().or_else(|| self.socket.take());
                }
                return;
            }
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
        // No longer connected: a subscription left ends, and its end is no news.
        self.subscribed = None;
        if let Some(watcher) = shared.watcher.lock().unwrap().take() {
            watcher.close();
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
