//! The service: runs the [`Engine`] and serves `proto` requests on a Unix socket, one per data
//! dir, and keeps the settings. `main`'s loop owns its lifetime, a [`machine`]: it stops
//! [`IDLE_GRACE`](machine::IDLE_GRACE) after the last client disconnects, or when asked to, and
//! refuses every request while it stops.

use std::fs::{self, File, Permissions};
use std::io::IsTerminal;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use proto::{Refusal, Request, Response, Setting};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

mod engine;
mod machine;
mod settings;

use engine::{Destination, Engine, Failure};
use machine::{Command, Do, Event, Observed, State};
use proto::tolerant::Decoded;
use settings::Settings;

struct Service {
    engine: Engine,
    dir: PathBuf,
    settings: Mutex<Settings>,
    /// The settings reset when `settings.json` was read, reported with the settings.
    reset: Vec<String>,
    /// To `main`'s loop, which owns the lifetime.
    events: mpsc::UnboundedSender<Event>,
    /// The lifetime, as `main`'s loop last decided it.
    lifetime: watch::Receiver<State>,
    /// [`proto::exe_id`] of this executable, hashed once at startup.
    build_id: String,
}

impl Service {
    fn observe(&self, what: Observed) {
        let _ = self.events.send(Event::Observed { what, at: Instant::now() });
    }

    /// Whether a request is answered now: refused once the service is stopping.
    fn admit(&self) -> Result<(), Refusal> {
        machine::decide(&self.lifetime.borrow(), &Event::Command(Command::Request)).map(drop)
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
    let port = settings.resolve_listen_port(&dir)?;
    info!(port, upnp = settings.upnp, "peer listener");

    let engine = Engine::start(engine::Config {
        data_dir: dir.clone(),
        download_dir: download_dir(),
        listen_port: port,
        upnp: settings.upnp,
        offline: false,
        size_check_every: Duration::from_secs(5),
    })
    .await?;

    let socket = proto::socket_path();
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    info!(socket = %socket.display(), pid = std::process::id(), "service listening");

    let (events, mut inbox) = mpsc::unbounded_channel();
    state = machine::decide(&state, &observed(Observed::Ready)).expect("observed");
    let (lifetime, published) = watch::channel(state.clone());
    let service = Arc::new(Service {
        engine,
        dir,
        settings: Mutex::new(settings),
        reset,
        events,
        lifetime: published,
        build_id: proto::exe_id(&std::env::current_exe()?)?,
    });

    // Accepts on its own, so a client arriving while the engine stops is refused, not left waiting.
    tokio::spawn(accept(listener, service.clone()));
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    while state != State::Exiting {
        let event = match machine::next(&state) {
            Some(Do::StopEngine) => {
                service.engine.stop().await;
                observed(Observed::EngineStopped)
            }
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
        let response = match (serde_json::from_str::<Request>(&line), service.admit()) {
            (Err(e), _) => Response::Error { message: format!("bad request: {e}") },
            (Ok(_), Err(refusal)) => Response::Refused(refusal),
            (Ok(Request::Shutdown), Ok(())) => {
                let _ = service.events.send(Event::Command(Command::Shutdown));
                return Ok(());
            }
            (Ok(request), Ok(())) => service.handle(request).await.unwrap_or_else(|failure| match failure {
                Failure::Refused(refusal) => Response::Refused(refusal),
                Failure::Error(e) => Response::Error { message: format!("{e:#}") },
            }),
        };
        // One that met the engine stopping is refused as the rest are from then on.
        let response = service.admit().map_or_else(Response::Refused, |()| response);
        let mut out = serde_json::to_vec(&response)?;
        out.push(b'\n');
        writer.write_all(&out).await?;
    }
    Ok(())
}

impl Service {
    async fn handle(&self, request: Request) -> Result<Response, Failure> {
        let engine = &self.engine;
        Ok(match request {
            Request::Hello => Response::Hello { build_id: self.build_id.clone(), service_version: env!("CARGO_PKG_VERSION").into() },
            Request::List => Response::Torrents { torrents: engine.listing().await },
            Request::Add { source, paused, destination } => {
                let default = self.settings.lock().unwrap().default_destination.clone();
                let path = destination.or(default).ok_or(Refusal::NoDestination)?;
                let destination = Destination::parse(path).map_err(|reason| Refusal::Destination { reason })?;
                Response::Added(engine.add(source, paused, destination).await?)
            }
            Request::Torrent { id, command } => {
                engine.command(id, command).await?;
                Response::Ok
            }
            Request::Details { id } => Response::Details(engine.details(id).await?),
            Request::Settings => Response::Settings(self.settings()),
            Request::SetSetting { setting } => {
                self.set(setting)?;
                Response::Settings(self.settings())
            }
            Request::Shutdown => unreachable!("handled by serve_requests"),
        })
    }

    fn settings(&self) -> proto::Settings {
        let settings = self.settings.lock().unwrap();
        proto::Settings { default_destination: settings.default_destination.clone(), reset: self.reset.clone() }
    }

    /// A default destination is parsed as an add's is: one that doesn't exist is never created.
    fn set(&self, setting: Setting) -> Result<(), Failure> {
        let mut settings = self.settings.lock().unwrap();
        match setting {
            Setting::DefaultDestination(Some(folder)) => {
                let destination = Destination::parse(folder).map_err(|reason| Refusal::DefaultDestination { reason })?;
                settings.default_destination = Some(destination.into_path());
            }
            Setting::DefaultDestination(None) => settings.default_destination = None,
        }
        Ok(settings.save(&self.dir)?)
    }
}
