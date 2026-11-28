//! The service: runs the [`Engine`] and serves `proto` requests on a Unix socket, one per data
//! dir, and keeps the settings. `main`'s loop owns its lifetime, a [`machine`]: it stops
//! [`IDLE_GRACE`](machine::IDLE_GRACE) after the last client disconnects, or when asked to, and
//! refuses every request while it stops. A new listening port or UPnP setting restarts the engine,
//! as librqbit can't change its listener while it runs: the machine's `Restarting`, during which
//! requests that need the engine wait and the others are answered. While a torrent downloads it
//! keeps the Mac awake, when the setting is on ([`awake`]).

use std::fs::{self, File, Permissions};
use std::io::IsTerminal;
use std::num::NonZeroU16;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use lifecycle::settings::Changes;
use proto::{Ratio, Refusal, Request, Response, SeedTime, SeedingLimit, ServiceError, Setting, SettingName, SpeedLimit};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

mod awake;
mod engine;
mod machine;
mod platform;
mod settings;
#[cfg(test)]
mod spawn_lock;

use engine::{Destination, Engine, Failure};
use machine::{Admit, Command, Do, Event, Needs, Observed, State};
use proto::tolerant::Decoded;
use settings::Settings;
use settings::machine as port;

struct Service {
    /// The engine; empty while it restarts, when `main`'s loop holds it. A request takes a handle
    /// for as long as it's answered.
    running: Mutex<Option<Arc<Running>>>,
    /// The engine a restart for new settings is stopping, while it waits for what's under way:
    /// Cancel move is its to answer meanwhile, as a Move waiting its turn is (ADR 0008).
    stopping: Mutex<Option<Arc<Running>>>,
    dir: PathBuf,
    /// One settings change at a time.
    changing: tokio::sync::Mutex<()>,
    /// How many restarts have ended, and why the last one failed, if it did.
    restarts: watch::Sender<(u64, Option<String>)>,
    settings: Mutex<Settings>,
    /// Each change of a setting that took effect, with the settings before it, for `UndoSetting`
    /// (#221).
    changes: Mutex<Changes<Settings>>,
    /// [`proto::Configured::at`]: counts up with every change of the settings, and when a restart
    /// ends (the port it listens on is the new engine's), so every subscriber sends them again.
    configured: watch::Sender<u64>,
    /// The settings reset when `settings.json` was read, reported with the settings.
    reset: Vec<String>,
    /// Where the torrents' database was kept aside at the first start, reported to every client of this run.
    records_set_aside: Option<std::path::PathBuf>,
    /// To `main`'s loop, which owns the lifetime.
    events: mpsc::UnboundedSender<Event>,
    /// The lifetime, as `main`'s loop last decided it.
    lifetime: watch::Receiver<State>,
    /// [`proto::exe_id`] of this executable, hashed once at startup.
    build_id: String,
    /// Keeps the Mac awake while a torrent downloads.
    awake: awake::KeepAwake,
}

/// The engine and the settings it was started with.
struct Running {
    engine: Engine,
    listening_on: NonZeroU16,
    /// The port setting it was started with, as clients see it.
    listen_port: proto::ListenPort,
}

impl Running {
    /// Starts the engine as `settings` say, deciding the port ([`settings::machine`]), with the
    /// Moves the engine before it didn't start (a restart for new settings) waiting their turn.
    async fn start(dir: &std::path::Path, settings: &mut Settings, carried: engine::Carried) -> Result<Self> {
        let (port, listen_port) = settings.start_listening(dir)?;
        info!(port, upnp = settings.upnp, "peer listener");
        let engine = Engine::start(engine::Config {
            data_dir: dir.to_owned(),
            download_dir: download_dir(),
            listen_port: port.get(),
            upnp: settings.upnp,
            offline: false,
            size_check_every: Duration::from_secs(5),
            poll_every: engine::POLL_EVERY,
            trash: engine::system_trash(),
            speed_limits: settings.speed_limits,
            seeding: settings.seeding.into(),
        })
        .await?;
        engine.set_default_destination(settings.default_destination.clone()).await;
        engine.resume(carried).await;
        Ok(Self { engine, listening_on: port, listen_port })
    }
}

/// `now` with the setting `name` as it is in `was`.
fn restored(name: SettingName, now: &Settings, was: &Settings) -> Settings {
    let mut back = now.clone();
    match name {
        SettingName::DefaultDestination => back.default_destination = was.default_destination.clone(),
        SettingName::ListenPort => back.listen_port = was.listen_port,
        SettingName::Upnp => back.upnp = was.upnp,
        SettingName::KeepAwake => back.keep_awake = was.keep_awake,
        SettingName::DownloadLimit => back.speed_limits.download = was.speed_limits.download,
        SettingName::UploadLimit => back.speed_limits.upload = was.speed_limits.upload,
        SettingName::SeedRatio => back.seeding.ratio = was.seeding.ratio,
        SettingName::SeedTime => back.seeding.time = was.seeding.time,
    }
    back
}

/// Whether going from `old` to `new` settings needs the engine started again: the listener or
/// UPnP changed. Nothing else does, even while a pinned port is taken.
fn restarts(old: &Settings, new: &Settings) -> bool {
    old.listen_port != new.listen_port || old.upnp != new.upnp
}

impl Service {
    fn observe(&self, what: Observed) {
        let _ = self.events.send(Event::Observed { what, at: Instant::now() });
    }

    /// Whether a request that needs nothing that restarts is answered: refused once the service
    /// is stopping.
    fn admit(&self) -> Result<(), Refusal> {
        machine::decide(&self.lifetime.borrow(), &Event::Command(Command::Request(Needs::Nothing))).map(drop)
    }

    /// The engine, for a request that needs it: at once, or once a restart is over; refused once
    /// the service is stopping.
    async fn running(&self) -> Result<Arc<Running>, Refusal> {
        let mut lifetime = self.lifetime.clone();
        loop {
            let admitted = machine::admit(&lifetime.borrow_and_update(), Needs::Engine);
            match admitted {
                Admit::Refused(refusal) => return Err(refusal),
                // Between the state and the slot, a restart may just have taken it.
                Admit::Now => match self.running.lock().unwrap().clone() {
                    Some(running) => return Ok(running),
                    None => {}
                },
                Admit::Wait => {}
            }
            if lifetime.changed().await.is_err() {
                return Err(Refusal::Stopping);
            }
        }
    }

    /// Cancel move while a restart for new settings stops the engine: that engine's to answer, as
    /// its Moves waiting their turn are, till it hands them to the next (none: once it has, or for
    /// any other command).
    async fn cancel_while_restarting(
        &self,
        ids: &[proto::TorrentId],
        command: &proto::TorrentCommand,
    ) -> Option<Vec<(proto::TorrentId, engine::Failure)>> {
        if *command != proto::TorrentCommand::CancelMove {
            return None;
        }
        let stopping = self.stopping.lock().unwrap().clone()?;
        stopping.engine.commands(ids.to_vec(), proto::TorrentCommand::CancelMove).await.ok()
    }

    /// Stops the engine and starts it with the settings as they are now; the settings it picks
    /// are kept. In-flight requests on the old engine finish first: they're short, as an add
    /// fetches its .torrent before it takes the engine.
    async fn restart(&self) -> bool {
        let old = self.running.lock().unwrap().take();
        // It waits only for what's under way; the Moves waiting their turn go on in the next.
        let carried = match old {
            Some(old) => {
                // Cancel move is its to answer from the moment it's out of `running`.
                *self.stopping.lock().unwrap() = Some(old.clone());
                let deadline = Instant::now() + Duration::from_secs(30);
                while Arc::strong_count(&old) > 2 && Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                let carried = old.engine.stop_to_restart().await;
                *self.stopping.lock().unwrap() = None;
                carried
            }
            None => Default::default(),
        };
        let mut settings = self.settings.lock().unwrap().clone();
        info!(port = ?settings.listen_port, upnp = settings.upnp, "restarting the engine with new settings");
        let started = Running::start(&self.dir, &mut settings, carried).await;
        let (ok, error) = match started {
            Ok(running) => {
                *self.settings.lock().unwrap() = settings;
                *self.running.lock().unwrap() = Some(Arc::new(running));
                self.configured.send_modify(|at| *at += 1);
                (true, None)
            }
            Err(e) => {
                warn!(error = %format!("{e:#}"), "the engine didn't start again: stopping");
                (false, Some(format!("{e:#}")))
            }
        };
        self.restarts.send_modify(|(count, failed)| (*count, *failed) = (*count + 1, error));
        ok
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,librqbit=warn,librqbit_dht=warn".into()),
        )
        // No color codes when logging to service.log.
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    let dir = proto::data_dir();
    fs::create_dir_all(&dir)?;
    // Only our user may reach the socket inside.
    fs::set_permissions(&dir, Permissions::from_mode(0o700))?;

    let observed = |what| Event::Observed { what, at: Instant::now() };
    let mut state = State::Starting;
    // Held for the service's whole life: guarantees a single service per data dir.
    let Some(_lock) = acquire_lock(proto::lock_path()).await? else {
        info!("another service is running, exiting: {:?}", machine::decide(&state, &observed(Observed::LockHeld)));
        return Ok(());
    };

    let Decoded { value: mut settings, reset } = Settings::load(&dir)?;
    let running = Running::start(&dir, &mut settings, Default::default()).await?;
    let records_set_aside = running.engine.records_set_aside().map(Into::into);
    let awake = awake::KeepAwake::start(settings.keep_awake, awake::System::default());

    let socket = proto::socket_path();
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    info!(socket = %socket.display(), pid = std::process::id(), "service listening");

    let (events, mut inbox) = mpsc::unbounded_channel();
    state = machine::decide(&state, &observed(Observed::Ready)).expect("observed");
    let (lifetime, published) = watch::channel(state.clone());
    let service = Arc::new(Service {
        running: Mutex::new(Some(Arc::new(running))),
        stopping: Mutex::new(None),
        dir,
        changing: tokio::sync::Mutex::new(()),
        restarts: watch::channel((0, None)).0,
        settings: Mutex::new(settings),
        changes: Mutex::new(Changes::new(started_at())),
        configured: watch::channel(started_at()).0,
        reset,
        records_set_aside,
        events,
        lifetime: published,
        build_id: proto::exe_id(&std::env::current_exe()?)?,
        awake,
    });

    // Accepts on its own, so a client arriving while the engine stops is refused, not left waiting.
    tokio::spawn(accept(listener, service.clone()));
    tokio::spawn(follow_downloads(service.clone()));
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    while state != State::Exiting {
        let event = match machine::next(&state) {
            Some(Do::StopEngine) => {
                service.awake.stop().await;
                let running = service.running.lock().unwrap().take();
                if let Some(running) = running {
                    running.engine.stop().await;
                }
                observed(Observed::EngineStopped)
            }
            // Meanwhile `serve` tasks read the published state: they answer what needs no engine,
            // and hold the rest; connections opening and closing wait in the inbox.
            Some(Do::RestartEngine) => observed(Observed::EngineRestarted { ok: service.restart().await }),
            wake => {
                let due = async {
                    match wake {
                        Some(Do::WakeAt(at)) => tokio::time::sleep_until(at.into()).await,
                        _ => std::future::pending().await,
                    }
                };
                tokio::select! {
                    Some(event) = inbox.recv() => event,
                    _ = due => observed(Observed::Tick),
                    _ = sigterm.recv() => { info!("SIGTERM"); Event::Command(Command::Shutdown) }
                    _ = sigint.recv() => { info!("SIGINT"); Event::Command(Command::Shutdown) }
                }
            }
        };
        // Only a request, which `serve` decides on its own, can be refused.
        if let Ok(decided) = machine::decide(&state, &event) {
            if decided != state && matches!(decided, State::Stopping | State::Exiting) {
                match &event {
                    Event::Command(command) => info!("{decided:?}: {command:?}"),
                    Event::Observed { what, .. } => info!("{decided:?}: {what:?}"),
                }
            }
            state = decided;
            lifetime.send_replace(state.clone());
        }
    }

    // The socket goes last, with the lock: until then a client meets a service that refuses, and
    // waits for it to go.
    let _ = fs::remove_file(&socket);
    info!("stopped");
    Ok(())
}

/// Tells [`awake`] whether some torrent is downloading, from every snapshot the engine publishes,
/// across its restarts; ends once the service stops.
async fn follow_downloads(service: Arc<Service>) {
    while let Ok(running) = service.running().await {
        // Its snapshots, not the engine: holding that would keep a restart waiting.
        let mut published = running.engine.subscribe();
        drop(running);
        loop {
            service.awake.downloading(published.borrow_and_update().downloading());
            // That engine stopped: it's restarting, or the service is stopping.
            if published.changed().await.is_err() {
                break;
            }
        }
    }
}

/// Now, in nanoseconds since 1970: what this service's counts start from, so a later one's are
/// larger.
fn started_at() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

/// librqbit's own folder; every add names its destination, so nothing goes here. A fresh data dir
/// has no default destination, so an isolated service never adds into the user's folders either.
fn download_dir() -> PathBuf {
    std::env::var_os("PHANTOM_DOWNLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").expect("HOME is not set")).join("Downloads"))
}

/// Waits briefly for the lock: a previous service lets it go just after removing its socket.
async fn acquire_lock(path: PathBuf) -> Result<Option<File>> {
    let file = File::create(path)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if file.try_lock().is_ok() {
            return Ok(Some(file));
        }
        if std::time::Instant::now() > deadline {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn accept(listener: UnixListener, service: Arc<Service>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(serve(stream, service.clone()));
            }
            Err(e) => warn!(error = %e, "accept failed"),
        }
    }
}

async fn serve(stream: UnixStream, service: Arc<Service>) {
    service.observe(Observed::Opened);
    if let Err(e) = serve_requests(stream, &service).await {
        warn!(error = %e, "client connection failed");
    }
    service.observe(Observed::Closed);
}

async fn serve_requests(stream: UnixStream, service: &Service) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        let request = serde_json::from_str::<Request>(&line);
        // A change answers for its own restart, even one that failed and stops the service.
        let changes = matches!(request, Ok(Request::SetSetting { .. } | Request::UndoSetting));
        let response = match (request, service.admit()) {
            (Err(e), _) => Response::Error { error: ServiceError::BadRequest { detail: e.to_string() } },
            (Ok(_), Err(refusal)) => Response::Refused(refusal),
            (Ok(Request::Shutdown), Ok(())) => {
                // Closed only once the service refuses: a client that saw it close and connects
                // again meets one that's going, never this one answering.
                let mut lifetime = service.lifetime.clone();
                let _ = service.events.send(Event::Command(Command::Shutdown));
                while machine::admit(&lifetime.borrow_and_update(), Needs::Nothing) != Admit::Refused(Refusal::Stopping) {
                    if lifetime.changed().await.is_err() {
                        break;
                    }
                }
                return Ok(());
            }
            (Ok(Request::Subscribe { watching }), Ok(())) => return stream_snapshots(lines, writer, service, watching).await,
            (Ok(request), Ok(())) => service.handle(request).await.unwrap_or_else(|failure| match failure {
                Failure::Refused(refusal) => Response::Refused(refusal),
                Failure::Error(error) => Response::Error { error },
            }),
        };
        // One that met the engine stopping is refused as the rest are from then on.
        let response = match service.admit() {
            Err(refusal) if !changes => Response::Refused(refusal),
            _ => response,
        };
        let mut out = serde_json::to_vec(&response)?;
        out.push(b'\n');
        writer.write_all(&out).await?;
    }
    Ok(())
}

/// At most one snapshot per this, so a busy engine doesn't flood a client.
const SNAPSHOTS_EVERY: Duration = Duration::from_millis(100);

/// Sends `out` as one line.
async fn send(writer: &mut tokio::net::unix::OwnedWriteHalf, out: &Response) -> Result<()> {
    let mut line = serde_json::to_vec(out)?;
    line.push(b'\n');
    Ok(writer.write_all(&line).await?)
}

/// A subscribed connection: the latest snapshot at once, then after each change, at most every
/// [`SNAPSHOTS_EVERY`] (`watch` keeps only the latest, so a slow client skips), and at once when
/// it subscribes again. Ends when the client closes it; when the service stops it's told so, as a
/// refusal. A client that reads nothing, so a write waits, is closed instead once the service
/// stops: there's no room left on its socket to tell it. Across a restart of the engine it waits
/// for the new one, then sends its first snapshot at once: a new epoch.
async fn stream_snapshots(
    mut lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    mut writer: tokio::net::unix::OwnedWriteHalf,
    service: &Service,
    mut watching: Vec<proto::TorrentId>,
) -> Result<()> {
    // Its snapshots, not the engine: holding that would keep a restart waiting.
    let mut published = match service.running().await {
        Ok(running) => running.engine.subscribe(),
        Err(refusal) => return send(&mut writer, &Response::Refused(refusal)).await,
    };
    let mut lifetime = service.lifetime.clone();
    let mut configured = service.configured.subscribe();
    // What it last sent: (epoch, version, watching, settings at). `None`: send whatever is latest.
    let mut sent = None;
    loop {
        let mut snapshot = published.borrow_and_update().for_client(&watching, &service.reset, service.records_set_aside.as_deref());
        configured.mark_unchanged();
        snapshot.settings = service.configured_now();
        // Not one it has: e.g. woken by another client connecting.
        let this = (snapshot.epoch, snapshot.version, watching.clone(), snapshot.settings.as_ref().map(|c| c.at));
        if sent.as_ref() != Some(&this) {
            sent = Some(this);
            let out = Response::Snapshot(snapshot);
            tokio::select! {
                sent = send(&mut writer, &out) => sent?,
                () = stopping(&mut lifetime, service) => return Ok(()),
            }
        }
        let next_at = tokio::time::Instant::now() + SNAPSHOTS_EVERY;
        tokio::select! {
            line = lines.next_line() => match line? {
                None => return Ok(()),
                Some(line) => match serde_json::from_str::<Request>(&line) {
                    Ok(Request::Subscribe { watching: now }) => {
                        watching = now;
                        sent = None;
                    }
                    Ok(other) => bail!("only Subscribe on a subscribed connection, not {other:?}"),
                    Err(e) => bail!("bad request: {e}"),
                },
            },
            // A change of the settings shows at once (the sheet a person is looking at follows it).
            Ok(()) = configured.changed() => {}
            changed = published.changed() => match changed {
                Ok(()) => tokio::time::sleep_until(next_at).await,
                // That engine stopped: it's restarting, or the service is stopping.
                Err(_) => match service.running().await {
                    Ok(running) => published = running.engine.subscribe(),
                    Err(refusal) => return send(&mut writer, &Response::Refused(refusal)).await,
                },
            },
            () = stopping(&mut lifetime, service) => {
                let refusal = service.admit().expect_err("stopping");
                return send(&mut writer, &Response::Refused(refusal)).await;
            }
        }
    }
}

/// Returns once the service stops admitting requests; never if its lifetime can't change any more.
async fn stopping(lifetime: &mut watch::Receiver<State>, service: &Service) {
    loop {
        if service.admit().is_err() {
            return;
        }
        if lifetime.changed().await.is_err() {
            return std::future::pending().await;
        }
    }
}

impl Service {
    async fn handle(&self, request: Request) -> Result<Response, Failure> {
        Ok(match request {
            Request::Hello => Response::Hello { build_id: self.build_id.clone(), service_version: env!("CARGO_PKG_VERSION").into() },
            Request::Add { source, paused, destination } => {
                let default = self.settings.lock().unwrap().default_destination.clone();
                let path = destination.or(default).ok_or(Refusal::NoDestination)?;
                let destination = Destination::parse(path).map_err(|reason| Refusal::Destination { reason })?;
                // Before taking the engine: a slow download holds nothing a restart waits for.
                let source = engine::fetched(source).await?;
                Response::Added(self.running().await?.engine.add(source, paused, destination).await?)
            }
            Request::Torrents { ids, command } => {
                let failed = match self.cancel_while_restarting(&ids, &command).await {
                    Some(failed) => failed,
                    None => self.running().await?.engine.commands(ids, command).await?,
                };
                let failed = failed.into_iter().map(|(id, failure)| proto::TorrentFailed { id, failure: failure.into() }).collect();
                Response::Commanded { failed }
            }
            Request::Moves { to, moves } => {
                // The engine let go of once they're sent: a restart for new settings waits for
                // every handle, and a batch is answered only once its last Move is over.
                let answer = self.running().await?.engine.send_moves(to, moves).await?;
                let failed = answer.await?;
                let failed = failed.into_iter().map(|(id, failure)| proto::TorrentFailed { id, failure: failure.into() }).collect();
                Response::Commanded { failed }
            }
            Request::Clashes { ids, to } => Response::Clashes { clashing: self.running().await?.engine.clashes(ids, to).await? },
            Request::FreeName { id, to } => Response::FreeName { name: self.running().await?.engine.free_name(id, to).await? },
            Request::Undo => Response::Undone(self.running().await?.engine.undo().await?),
            Request::Subscribe { .. } => unreachable!("handled by serve_requests"),
            Request::Settings => Response::Settings(self.settings(&*self.running().await?)),
            Request::SetSetting { setting, by } => Response::SettingSet(self.set(setting, by).await?),
            Request::UndoSetting => Response::SettingUndone(self.undo_setting().await?),
            Request::Shutdown => unreachable!("handled by serve_requests"),
        })
    }

    /// The settings and their changes as subscribers see them; `None` while the engine restarts,
    /// when the port it listens on isn't known (the next ones count up once it's back).
    fn configured_now(&self) -> Option<proto::Configured> {
        let running = self.running.lock().unwrap().clone()?;
        let at = *self.configured.borrow();
        let changes = self.changes.lock().unwrap().listed();
        Some(proto::Configured { at, settings: self.settings(&running), changes })
    }

    fn settings(&self, running: &Running) -> proto::Settings {
        let settings = self.settings.lock().unwrap();
        proto::Settings {
            default_destination: settings.default_destination.clone(),
            default_destination_unavailable: running.engine.default_unavailable(),
            listen_port: running.listen_port,
            listening_on: running.listening_on,
            upnp: settings.upnp,
            keep_awake: settings.keep_awake,
            speed_limits: settings.speed_limits,
            seeding: settings.seeding,
            reset: self.reset.clone(),
            records_set_aside: self.records_set_aside.clone(),
        }
    }

    /// Changes one setting and saves them; one that changes the listener or UPnP has `main`'s
    /// loop restart the engine, and is answered once it's back. A default destination is parsed as
    /// an add's is: one that doesn't exist is never created. One that changes the value is a
    /// settings change, `by` the client's id for it, from the moment it's saved.
    async fn set(&self, setting: Setting, by: Option<u64>) -> Result<proto::SettingSet, Failure> {
        let _one_at_a_time = self.changing.lock().await;
        let name = setting.name();
        let old = self.settings.lock().unwrap().clone();
        let mut new = old.clone();
        match setting {
            Setting::DefaultDestination(Some(folder)) => {
                let destination = Destination::parse(folder).map_err(|reason| Refusal::DefaultDestination { reason })?;
                new.default_destination = Some(destination.into_path());
            }
            Setting::DefaultDestination(None) => new.default_destination = None,
            Setting::ListenPort(port) => {
                let command = port.map_or(port::Command::Unset, port::Command::Set);
                let Ok(decided) = port::decide(&new.listen_port, &port::Event::Command(command));
                new.listen_port = decided;
            }
            Setting::Upnp(upnp) => new.upnp = upnp.unwrap_or(Settings::default().upnp),
            Setting::KeepAwake(on) => new.keep_awake = on.unwrap_or(Settings::default().keep_awake),
            Setting::StopSeeding { at, value } => {
                let refused = |problem| Refusal::StopSeeding { at, problem };
                match at {
                    SeedingLimit::Ratio => new.seeding.ratio = value.map(Ratio::new).transpose().map_err(refused)?,
                    SeedingLimit::Time => new.seeding.time = value.map(SeedTime::new).transpose().map_err(refused)?,
                }
            }
            Setting::SpeedLimit { direction, bytes_per_sec } => {
                let limit = bytes_per_sec.map(SpeedLimit::new).transpose();
                new.speed_limits.set(direction, limit.map_err(|problem| Refusal::SpeedLimit { direction, problem })?);
            }
        }
        self.apply(&old, new, Some((name, by))).await
    }

    /// Undoes the newest settings change: its setting goes back to what it was, as a change made by
    /// hand goes, but a default folder isn't checked (one that can't be reached now goes back, and
    /// says so as it's watched). Taken off first, so one refused is gone.
    async fn undo_setting(&self) -> Result<proto::SettingUndone, Failure> {
        let _one_at_a_time = self.changing.lock().await;
        let (change, was) = self.changes.lock().unwrap().take_newest().ok_or(Refusal::NothingToUndo)?;
        self.configured.send_modify(|at| *at += 1);
        let old = self.settings.lock().unwrap().clone();
        let new = restored(change.name, &old, &was);
        let settings = self.apply(&old, new, None).await?.settings;
        Ok(proto::SettingUndone { change, settings })
    }

    /// Goes from `old` settings to `new`, with `changed` the change it is, if it's one to undo.
    async fn apply(
        &self,
        old: &Settings,
        new: Settings,
        changed: Option<(SettingName, Option<u64>)>,
    ) -> Result<proto::SettingSet, Failure> {
        // Refused before anything is saved, as when the service is stopping: what's saved is what's
        // applied. Let go before a restart, which waits for every holder of the engine.
        let running = self.running().await?;
        new.save(&self.dir)?;
        *self.settings.lock().unwrap() = new.clone();
        self.awake.set(new.keep_awake);
        if new.speed_limits != old.speed_limits {
            running.engine.set_limits(new.speed_limits);
        }
        if new.seeding != old.seeding {
            running.engine.set_seeding(new.seeding.into()).await;
        }
        if new.default_destination != old.default_destination {
            running.engine.set_default_destination(new.default_destination.clone()).await;
        }
        drop(running);
        let change = changed
            .filter(|(name, _)| restored(*name, &new, old) != new)
            .map(|(name, by)| self.changes.lock().unwrap().push(name, by, old.clone()));
        self.configured.send_modify(|at| *at += 1);
        let at = *self.configured.borrow();
        if restarts(old, &new) {
            let mut ended = self.restarts.subscribe();
            let before = ended.borrow_and_update().0;
            let _ = self.events.send(Event::Command(Command::Restart));
            // Refused, as once the service is stopping, it never comes.
            let mut lifetime = self.lifetime.clone();
            let stopping = lifetime.wait_for(|state| matches!(state, State::Stopping | State::Exiting));
            let failed = tokio::select! {
                // A failed restart counts before it stops the service: its error comes first.
                biased;
                ended = ended.wait_for(|(count, _)| *count > before) => ended.map_err(|_| Refusal::Stopping)?.1.clone(),
                _ = stopping => return Err(Refusal::Stopping.into()),
            };
            if let Some(e) = failed {
                return Err(ServiceError::NotRestarted { detail: e.to_string() }.into());
            }
        }
        Ok(proto::SettingSet { settings: self.settings(&*self.running().await?), change, at })
    }
}

#[cfg(test)]
mod set_tests {
    use proptest::prelude::*;
    use proto::{Direction, SpeedLimit};

    use super::*;

    /// A service in `dir`, its lifetime `state`, running `running` if any.
    fn service(dir: &std::path::Path, state: State, running: Option<Arc<Running>>) -> Service {
        let (events, _) = mpsc::unbounded_channel();
        let (_, lifetime) = watch::channel(state);
        Service {
            running: Mutex::new(running),
            stopping: Mutex::new(None),
            dir: dir.to_owned(),
            changing: tokio::sync::Mutex::new(()),
            restarts: watch::channel((0, None)).0,
            settings: Mutex::new(Settings::default()),
            changes: Mutex::new(Changes::new(1)),
            configured: watch::channel(1).0,
            reset: Vec::new(),
            records_set_aside: None,
            events,
            lifetime,
            build_id: String::new(),
            awake: awake::KeepAwake::start(false, awake::System::default()),
        }
    }

    /// Changes that don't restart the engine: the folder (gone or not), keeping awake, and limits
    /// in range or not.
    fn change() -> impl Strategy<Value = Setting> {
        let direction = prop_oneof![Just(Direction::Download), Just(Direction::Upload)];
        prop_oneof![
            any::<bool>().prop_map(|on| Setting::KeepAwake(Some(on))),
            Just(Setting::DefaultDestination(None)),
            Just(Setting::DefaultDestination(Some(proto::AbsolutePath::new("/no/such/folder").unwrap()))),
            (direction, proptest::option::of(prop_oneof![0..SpeedLimit::MIN, SpeedLimit::MIN..=SpeedLimit::MAX]))
                .prop_map(|(direction, bytes_per_sec)| Setting::SpeedLimit { direction, bytes_per_sec }),
            (prop_oneof![Just(proto::SeedingLimit::Ratio), Just(proto::SeedingLimit::Time)], proptest::option::of(0..2_000_000u64))
                .prop_map(|(at, value)| Setting::StopSeeding { at, value }),
        ]
    }

    cases::proptest! {
        #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

        /// What's saved is what's applied: a change refused (the service stopping, a value out of
        /// range) leaves `settings.json` as it was; one answered is saved, and its limits are the
        /// running engine's.
        #[test]
        fn a_change_is_saved_exactly_when_it_is_applied(changes in proptest::collection::vec(change(), 1..6), stopping in any::<bool>()) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let dir = tempfile::tempdir().unwrap();
            rt.block_on(async {
                let running = if stopping {
                    None
                } else {
                    let engine = Engine::start(engine::Config {
                        data_dir: dir.path().to_owned(),
                        download_dir: dir.path().join("downloads"),
                        listen_port: 0,
                        upnp: false,
                        offline: true,
                        size_check_every: Duration::from_secs(5),
                        poll_every: engine::POLL_EVERY,
                        trash: engine::system_trash(),
                        speed_limits: Default::default(),
                        seeding: Default::default(),
                    })
                    .await
                    .unwrap();
                    let port = NonZeroU16::new(51413).unwrap();
                    Some(Arc::new(Running { engine, listening_on: port, listen_port: proto::ListenPort::Picked(port) }))
                };
                let state = if stopping { State::Stopping } else { State::Idle { since: Instant::now() } };
                let service = service(dir.path(), state, running.clone());
                for change in changes {
                    let before = std::fs::read(dir.path().join("settings.json")).ok();
                    let answer = service.set(change.clone(), None).await;
                    let after = std::fs::read(dir.path().join("settings.json")).ok();
                    match &answer {
                        Err(_) => prop_assert_eq!(&after, &before, "refused, nothing saved: {:?}", change),
                        Ok(answered) => {
                            let saved = Settings::load(dir.path()).unwrap().value;
                            prop_assert_eq!(saved.speed_limits, answered.settings.speed_limits, "saved what it answers");
                            let engine = &running.as_ref().expect("answered, so running").engine;
                            let applied = engine.limits();
                            let bps = |l: Option<SpeedLimit>| l.map(SpeedLimit::bytes_per_sec);
                            prop_assert_eq!((applied.download_bps, applied.upload_bps), (bps(saved.speed_limits.download), bps(saved.speed_limits.upload)), "and applied");
                            prop_assert_eq!(saved.seeding, answered.settings.seeding, "saved the seeding limits it answers");
                            prop_assert_eq!(engine.seeding().await, saved.seeding.into(), "and goes by them");
                        }
                    }
                    if stopping {
                        prop_assert!(matches!(answer, Err(Failure::Refused(Refusal::Stopping)) | Err(Failure::Refused(_))), "{:?}", change);
                    }
                }
                if let Some(running) = running {
                    drop(service);
                    Arc::into_inner(running).expect("the only holder").engine.stop().await;
                }
                Ok(())
            })?;
        }

        /// Settings changes and undos (#221), against a stack of what each change's setting was: a
        /// change that took effect is the newest, one refused or of the same value isn't one; an
        /// undo takes the newest back, saved and applied as a change is, and with none is refused;
        /// what's listed is the stack.
        #[test]
        fn an_undo_takes_back_the_newest_change_that_took_effect(steps in proptest::collection::vec(proptest::option::of(change()), 1..12)) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let dir = tempfile::tempdir().unwrap();
            rt.block_on(async {
                let engine = Engine::start(engine::Config {
                    data_dir: dir.path().to_owned(),
                    download_dir: dir.path().join("downloads"),
                    listen_port: 0,
                    upnp: false,
                    offline: true,
                    size_check_every: Duration::from_secs(5),
                    poll_every: engine::POLL_EVERY,
                    trash: engine::system_trash(),
                    speed_limits: Default::default(),
                    seeding: Default::default(),
                })
                .await
                .unwrap();
                let port = NonZeroU16::new(51413).unwrap();
                let running = Arc::new(Running { engine, listening_on: port, listen_port: proto::ListenPort::Picked(port) });
                let service = service(dir.path(), State::Idle { since: Instant::now() }, Some(running.clone()));
                let mut stack: Vec<(SettingName, Settings)> = Vec::new();
                for step in steps {
                    let before = service.settings.lock().unwrap().clone();
                    match step {
                        Some(change) => {
                            let name = change.name();
                            if let Ok(set) = service.set(change, None).await {
                                let changed = restored(name, &before, &service.settings.lock().unwrap()) != before;
                                prop_assert_eq!(set.change.as_ref().map(|c| c.name), changed.then_some(name), "the answer names the change it made, if any");
                                if changed {
                                    let listed = service.changes.lock().unwrap().listed();
                                    prop_assert_eq!(listed.last(), set.change.as_ref(), "the newest listed");
                                    stack.push((name, before));
                                }
                            }
                        }
                        None => match (service.undo_setting().await, stack.pop()) {
                            (Ok(undone), Some((name, was))) => {
                                prop_assert_eq!(undone.change.name, name, "the newest");
                                let now = service.settings.lock().unwrap().clone();
                                prop_assert_eq!(&now, &restored(name, &before, &was), "back as it was, the rest as it is");
                                prop_assert_eq!(&Settings::load(dir.path()).unwrap().value, &now, "and saved");
                                prop_assert_eq!(running.engine.limits().download_bps, now.speed_limits.download.map(SpeedLimit::bytes_per_sec), "and applied");
                            }
                            (Err(Failure::Refused(Refusal::NothingToUndo)), None) => {}
                            (answer, expected) => prop_assert!(false, "{:?}, not {:?}", answer.map(|u| u.change), expected.map(|e| e.0)),
                        },
                    }
                    let listed: Vec<SettingName> = service.configured_now().unwrap().changes.iter().map(|c| c.name).collect();
                    prop_assert_eq!(listed, stack.iter().map(|(name, _)| *name).collect::<Vec<_>>(), "it lists the stack");
                }
                drop(service);
                Arc::into_inner(running).expect("the only holder").engine.stop().await;
                Ok(())
            })?;
        }
    }
}

#[cfg(test)]
mod reach_tests {
    use proto::{AbsolutePath, Unavailable};

    use super::*;

    /// A service in `dir` with a running engine.
    async fn running_service(dir: &std::path::Path) -> Service {
        let engine = Engine::start(engine::Config {
            data_dir: dir.to_owned(),
            download_dir: dir.join("downloads"),
            listen_port: 0,
            upnp: false,
            offline: true,
            size_check_every: Duration::from_secs(5),
            poll_every: Duration::from_millis(100),
            trash: engine::system_trash(),
            speed_limits: Default::default(),
            seeding: Default::default(),
        })
        .await
        .unwrap();
        let port = NonZeroU16::new(51413).unwrap();
        let running = Arc::new(Running { engine, listening_on: port, listen_port: proto::ListenPort::Picked(port) });
        let (events, _) = mpsc::unbounded_channel();
        let (_, lifetime) = watch::channel(State::Idle { since: Instant::now() });
        Service {
            running: Mutex::new(Some(running)),
            stopping: Mutex::new(None),
            dir: dir.to_owned(),
            changing: tokio::sync::Mutex::new(()),
            restarts: watch::channel((0, None)).0,
            settings: Mutex::new(Settings::default()),
            changes: Mutex::new(Changes::new(1)),
            configured: watch::channel(1).0,
            reset: Vec::new(),
            records_set_aside: None,
            events,
            lifetime,
            build_id: String::new(),
            awake: awake::KeepAwake::start(false, awake::System::default()),
        }
    }

    async fn settings_of(service: &Service) -> proto::Settings {
        match service.handle(Request::Settings).await.unwrap() {
            Response::Settings(settings) => settings,
            other => panic!("not settings: {other:?}"),
        }
    }

    /// The Settings request says why the default folder can't be reached, as the engine watching it
    /// last found (#209): the service tells the engine which folder it is as the setting changes,
    /// and the answer never reads the disk itself.
    #[tokio::test]
    async fn the_settings_answer_says_why_the_default_folder_cant_be_reached() {
        let dir = tempfile::tempdir().unwrap();
        let service = running_service(dir.path()).await;
        let folder = dir.path().join("films");
        std::fs::create_dir(&folder).unwrap();
        let path = AbsolutePath::new(&folder).unwrap();
        service.set(Setting::DefaultDestination(Some(path.clone())), None).await.unwrap();
        std::fs::remove_dir(&folder).unwrap();
        let missing = Some(Unavailable::FolderMissing { path });
        for _ in 0..100 {
            if settings_of(&service).await.default_destination_unavailable == missing {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(settings_of(&service).await.default_destination_unavailable, missing, "removed, the answer says it's missing");
        service.set(Setting::DefaultDestination(None), None).await.unwrap();
        for _ in 0..100 {
            if settings_of(&service).await.default_destination_unavailable.is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(settings_of(&service).await.default_destination_unavailable, None, "none set, nothing to reach");
    }

    /// Right after the default folder changes, the answer is the new folder's: the old one's
    /// reason (it was unreachable, the new one is fine) is gone from it, not from the next
    /// snapshot (`phantom settings default-destination …` prints that answer).
    #[tokio::test]
    async fn the_answer_to_a_change_of_the_default_folder_isnt_the_old_folders() {
        let dir = tempfile::tempdir().unwrap();
        let service = running_service(dir.path()).await;
        let (old, new) = (dir.path().join("old"), dir.path().join("new"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        let old_path = AbsolutePath::new(&old).unwrap();
        service.set(Setting::DefaultDestination(Some(old_path.clone())), None).await.unwrap();
        std::fs::remove_dir(&old).unwrap();
        let missing = Some(Unavailable::FolderMissing { path: old_path });
        for _ in 0..100 {
            if settings_of(&service).await.default_destination_unavailable == missing {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(settings_of(&service).await.default_destination_unavailable, missing, "the old one is missing");
        let answered = service.set(Setting::DefaultDestination(Some(AbsolutePath::new(&new).unwrap())), None).await.unwrap();
        assert_eq!(answered.settings.default_destination_unavailable, None, "the answer is the new folder's");
    }
}
