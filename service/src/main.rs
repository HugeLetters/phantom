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
use proto::{Refusal, Request, Response, Setting, SpeedLimit};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

mod awake;
mod engine;
mod machine;
mod settings;

use engine::{Destination, Engine, Failure};
use machine::{Admit, Command, Do, Event, Needs, Observed, State};
use proto::tolerant::Decoded;
use settings::Settings;
use settings::machine as port;

struct Service {
    /// The engine; empty while it restarts, when `main`'s loop holds it. A request takes a handle
    /// for as long as it's answered.
    running: Mutex<Option<Arc<Running>>>,
    dir: PathBuf,
    /// One settings change at a time.
    changing: tokio::sync::Mutex<()>,
    /// How many restarts have ended, and why the last one failed, if it did.
    restarts: watch::Sender<(u64, Option<String>)>,
    settings: Mutex<Settings>,
    /// The settings reset when `settings.json` was read, reported with the settings.
    reset: Vec<String>,
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
    /// Starts the engine as `settings` say, deciding the port ([`settings::machine`]).
    async fn start(dir: &std::path::Path, settings: &mut Settings) -> Result<Self> {
        let (port, listen_port) = settings.start_listening(dir)?;
        info!(port, upnp = settings.upnp, "peer listener");
        let engine = Engine::start(engine::Config {
            data_dir: dir.to_owned(),
            download_dir: download_dir(),
            listen_port: port.get(),
            upnp: settings.upnp,
            offline: false,
            size_check_every: Duration::from_secs(5),
            trash: engine::system_trash(),
            speed_limits: settings.speed_limits,
        })
        .await?;
        Ok(Self { engine, listening_on: port, listen_port })
    }
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

    /// Stops the engine and starts it with the settings as they are now; the settings it picks
    /// are kept. In-flight requests on the old engine finish first: they're short, as an add
    /// fetches its .torrent before it takes the engine.
    async fn restart(&self) -> bool {
        let old = self.running.lock().unwrap().take();
        if let Some(old) = old {
            let deadline = Instant::now() + Duration::from_secs(30);
            while Arc::strong_count(&old) > 1 && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            old.engine.stop().await;
        }
        let mut settings = self.settings.lock().unwrap().clone();
        info!(port = ?settings.listen_port, upnp = settings.upnp, "restarting the engine with new settings");
        let started = Running::start(&self.dir, &mut settings).await;
        let (ok, error) = match started {
            Ok(running) => {
                *self.settings.lock().unwrap() = settings;
                *self.running.lock().unwrap() = Some(Arc::new(running));
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
    let running = Running::start(&dir, &mut settings).await?;
    let awake = awake::KeepAwake::start(settings.keep_awake, awake::MacOs::default());

    let socket = proto::socket_path();
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    info!(socket = %socket.display(), pid = std::process::id(), "service listening");

    let (events, mut inbox) = mpsc::unbounded_channel();
    state = machine::decide(&state, &observed(Observed::Ready)).expect("observed");
    let (lifetime, published) = watch::channel(state.clone());
    let service = Arc::new(Service {
        running: Mutex::new(Some(Arc::new(running))),
        dir,
        changing: tokio::sync::Mutex::new(()),
        restarts: watch::channel((0, None)).0,
        settings: Mutex::new(settings),
        reset,
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
        let changes = matches!(request, Ok(Request::SetSetting { .. }));
        let response = match (request, service.admit()) {
            (Err(e), _) => Response::Error { message: format!("bad request: {e}") },
            (Ok(_), Err(refusal)) => Response::Refused(refusal),
            (Ok(Request::Shutdown), Ok(())) => {
                let _ = service.events.send(Event::Command(Command::Shutdown));
                return Ok(());
            }
            (Ok(Request::Subscribe { watching }), Ok(())) => return stream_snapshots(lines, writer, service, watching).await,
            (Ok(request), Ok(())) => service.handle(request).await.unwrap_or_else(|failure| match failure {
                Failure::Refused(refusal) => Response::Refused(refusal),
                Failure::Error(e) => Response::Error { message: format!("{e:#}") },
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
    // What it last sent: (epoch, version, watching). `None`: send whatever is latest.
    let mut sent = None;
    loop {
        let snapshot = published.borrow_and_update().for_client(&watching, &service.reset);
        // Not one it has: e.g. woken by another client connecting.
        let this = (snapshot.epoch, snapshot.version, watching.clone());
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
            Request::Torrent { id, command } => {
                self.running().await?.engine.command(id, command).await?;
                Response::Ok
            }
            Request::Subscribe { .. } => unreachable!("handled by serve_requests"),
            Request::Settings => Response::Settings(self.settings(&*self.running().await?)),
            Request::SetSetting { setting } => Response::Settings(self.set(setting).await?),
            Request::Shutdown => unreachable!("handled by serve_requests"),
        })
    }

    fn settings(&self, running: &Running) -> proto::Settings {
        let settings = self.settings.lock().unwrap();
        proto::Settings {
            default_destination: settings.default_destination.clone(),
            listen_port: running.listen_port,
            listening_on: running.listening_on,
            upnp: settings.upnp,
            keep_awake: settings.keep_awake,
            speed_limits: settings.speed_limits,
            reset: self.reset.clone(),
        }
    }

    /// Changes one setting and saves them; one that changes the listener or UPnP has `main`'s
    /// loop restart the engine, and is answered once it's back. A default destination is parsed as
    /// an add's is: one that doesn't exist is never created.
    async fn set(&self, setting: Setting) -> Result<proto::Settings, Failure> {
        let _one_at_a_time = self.changing.lock().await;
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
            Setting::SpeedLimit { direction, bytes_per_sec } => {
                let limit = bytes_per_sec.map(SpeedLimit::new).transpose();
                new.speed_limits.set(direction, limit.map_err(|problem| Refusal::SpeedLimit { direction, problem })?);
            }
        }
        // Refused before anything is saved, as when the service is stopping: what's saved is what's
        // applied. Let go before a restart, which waits for every holder of the engine.
        let running = self.running().await?;
        new.save(&self.dir)?;
        *self.settings.lock().unwrap() = new.clone();
        self.awake.set(new.keep_awake);
        if new.speed_limits != old.speed_limits {
            running.engine.set_limits(new.speed_limits);
        }
        drop(running);
        if restarts(&old, &new) {
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
                return Err(Failure::Error(anyhow::anyhow!("the torrents didn't start again with the new settings: {e}")));
            }
        }
        Ok(self.settings(&*self.running().await?))
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
            dir: dir.to_owned(),
            changing: tokio::sync::Mutex::new(()),
            restarts: watch::channel((0, None)).0,
            settings: Mutex::new(Settings::default()),
            reset: Vec::new(),
            events,
            lifetime,
            build_id: String::new(),
            awake: awake::KeepAwake::start(false, awake::MacOs::default()),
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
        ]
    }

    proptest! {
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
                        speed_limits: Default::default(),
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
                    let answer = service.set(change.clone()).await;
                    let after = std::fs::read(dir.path().join("settings.json")).ok();
                    match &answer {
                        Err(_) => prop_assert_eq!(&after, &before, "refused, nothing saved: {:?}", change),
                        Ok(answered) => {
                            let saved = Settings::load(dir.path()).unwrap().value;
                            prop_assert_eq!(saved.speed_limits, answered.speed_limits, "saved what it answers");
                            let engine = &running.as_ref().expect("answered, so running").engine;
                            let applied = engine.limits();
                            let bps = |l: Option<SpeedLimit>| l.map(SpeedLimit::bytes_per_sec);
                            prop_assert_eq!((applied.download_bps, applied.upload_bps), (bps(saved.speed_limits.download), bps(saved.speed_limits.upload)), "and applied");
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
    }
}
