//! `phantom`: the command line client. Parses arguments, makes requests through `client` and
//! prints the answers.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use client::{Client, Failure, display, setting};
use proto::{
    AddSource, Details, Direction, Priority, Refusal, SeedingLimit, Setting, Settings, TorrentCommand, TorrentId, TorrentInfo,
    TorrentRefusal,
};

#[derive(Parser)]
#[command(name = "phantom", about = "Command-line client for the Phantom service")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List torrents
    #[command(alias = "ls")]
    List,
    /// Refresh the torrent list every second until Ctrl+C
    Watch,
    /// Add a magnet link, .torrent URL, or local .torrent file
    Add {
        source: String,
        #[arg(long)]
        paused: bool,
        /// The folder its files go into (a torrent of several files gets its own folder there);
        /// the default destination if left out
        #[arg(long, value_name = "FOLDER")]
        destination: Option<PathBuf>,
    },
    Pause {
        id: TorrentId,
    },
    /// Start a torrent; one that's missing files checks them first and downloads them again
    Resume {
        id: TorrentId,
    },
    /// Check a torrent's files on disk, keeping what's valid and downloading the rest
    Check {
        id: TorrentId,
    },
    /// Take a torrent off the list, keeping its files (a torrent fetching info stops)
    #[command(alias = "rm")]
    Remove {
        id: TorrentId,
    },
    /// Remove a torrent and move its files to the Trash
    Delete {
        id: TorrentId,
    },
    /// List a torrent's files with their progress, size and priority
    Files {
        id: TorrentId,
    },
    /// Print a torrent's magnet link: its info hash, name and trackers
    Magnet {
        id: TorrentId,
    },
    /// Set the priority of a torrent's files, by the indices `files` lists (Off isn't downloaded)
    Priority {
        id: TorrentId,
        priority: PriorityArg,
        #[arg(required = true)]
        files: Vec<usize>,
    },
    /// Show the settings, or change one
    Settings {
        #[command(subcommand)]
        change: Option<SettingsChange>,
    },
    /// Keep the service running until Ctrl+C
    Keepalive,
    /// Stop the service, even if other clients are connected
    Stop,
}

#[derive(Subcommand)]
enum SettingsChange {
    /// default-destination FOLDER, listening-port PORT (1-65535, kept even when another app takes
    /// it), upnp on|off, keep-awake on|off (the Mac doesn't sleep while a torrent downloads, fetches info or checks), or
    /// download-limit and upload-limit SPEED (for all torrents together: 500K, 2MiB, 1.5 MB/s; a
    /// number alone is KiB/s; from 16 KiB/s), seed-ratio RATIO (2, 1.5) or seed-time TIME (hours,
    /// or 90min, 2d): a finished torrent stops seeding at whichever comes first. A new port or UPnP
    /// restarts the torrents, which come back as they were; a limit applies at once
    Set { name: SettingName, value: String },
    /// Back to the default: no default destination (every add needs --destination), a port the
    /// service picks (keeping the current one until another app takes it), UPnP on, keep-awake off,
    /// no speed limits, seeding never stops by itself
    Unset { name: SettingName },
}

#[derive(Clone, Copy, ValueEnum)]
enum SettingName {
    DefaultDestination,
    ListeningPort,
    Upnp,
    KeepAwake,
    DownloadLimit,
    UploadLimit,
    SeedRatio,
    SeedTime,
}

#[derive(Clone, Copy, ValueEnum)]
enum PriorityArg {
    Normal,
    Off,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Command::Stop = cli.command {
        match Client::connect() {
            Ok(mut client) => client.shutdown()?,
            Err(_) => println!("service is not running"),
        }
        return Ok(());
    }

    let mut client = Client::connect_or_spawn(&client::sibling_service_exe()?)?;
    match cli.command {
        // The first snapshot, then the connection closes.
        Command::List => print_listing(&client.subscribe(Vec::new())?.receive()?.torrents),
        Command::Watch => {
            let mut snapshots = client.subscribe(Vec::new())?;
            loop {
                let snapshot = snapshots.receive()?;
                // Clear screen and move the cursor home.
                print!("\x1b[2J\x1b[H");
                print_listing(&snapshot.torrents);
            }
        }
        Command::Add { source, paused, destination } => {
            let destination = destination.as_deref().map(client::absolute).transpose()?;
            // A file on disk, or else a link, checked here as the GUI's field does.
            let is_file = Path::new(&source).is_file();
            let source_arg = &source;
            let source = if is_file {
                AddSource::TorrentFile(std::fs::read(source_arg).with_context(|| format!("reading {source_arg}"))?)
            } else {
                client::add_source(source_arg).map_err(|problem| anyhow::anyhow!("{}", display::not_added(source_arg, problem)))?
            };
            let added = client.add(source, paused, destination).map_err(|failure| match failure {
                // Whatever the name looks like, the file came first: say so.
                Failure::Error(error) if is_file => error.context(format!("{source_arg} is a file here, read as a .torrent file")),
                failure => with_fix(failure),
            })?;
            // The id is the CLI's own: what the other commands take.
            println!("{} (id {})", display::added(&added), added.id);
        }
        Command::Pause { id } => client.command(id, TorrentCommand::Pause)?,
        Command::Resume { id } => client.command(id, TorrentCommand::Resume)?,
        Command::Check { id } => client.command(id, TorrentCommand::Check)?,
        Command::Remove { id } => client.command(id, TorrentCommand::Remove)?,
        Command::Delete { id } => client.command(id, TorrentCommand::Delete)?,
        Command::Magnet { id } => {
            let listing = client.list()?;
            let torrent =
                listing.iter().find(|t| t.id == id).ok_or(Failure::Refused(Refusal::Torrent { id, why: TorrentRefusal::NotInList }))?;
            println!("{}", client::magnet_link(torrent));
        }
        Command::Files { id } => print_files(&client.details(id)?),
        Command::Priority { id, priority, files } => {
            let priority = match priority {
                PriorityArg::Normal => Priority::Normal,
                PriorityArg::Off => Priority::Off,
            };
            client.command(id, TorrentCommand::SetPriority { files, priority })?;
        }
        Command::Settings { change: None } => print_settings(&client.settings()?),
        Command::Settings { change: Some(change) } => {
            let problem = |p| anyhow::anyhow!("{}", display::value_problem(p));
            let setting = match change {
                SettingsChange::Set { name: SettingName::DefaultDestination, value } => {
                    Setting::DefaultDestination(Some(client::absolute(Path::new(&value))?))
                }
                SettingsChange::Set { name: SettingName::ListeningPort, value } => {
                    Setting::ListenPort(Some(setting::port(&value).map_err(problem)?))
                }
                SettingsChange::Set { name: SettingName::Upnp, value } => Setting::Upnp(Some(setting::on_off(&value).map_err(problem)?)),
                SettingsChange::Unset { name: SettingName::DefaultDestination } => Setting::DefaultDestination(None),
                SettingsChange::Unset { name: SettingName::ListeningPort } => Setting::ListenPort(None),
                SettingsChange::Set { name: SettingName::KeepAwake, value } => {
                    Setting::KeepAwake(Some(setting::on_off(&value).map_err(problem)?))
                }
                SettingsChange::Unset { name: SettingName::Upnp } => Setting::Upnp(None),
                SettingsChange::Unset { name: SettingName::KeepAwake } => Setting::KeepAwake(None),
                SettingsChange::Set { name: SettingName::DownloadLimit, value } => limit(Direction::Download, Some(&value))?,
                SettingsChange::Set { name: SettingName::UploadLimit, value } => limit(Direction::Upload, Some(&value))?,
                SettingsChange::Unset { name: SettingName::DownloadLimit } => limit(Direction::Download, None)?,
                SettingsChange::Unset { name: SettingName::UploadLimit } => limit(Direction::Upload, None)?,
                SettingsChange::Set { name: SettingName::SeedRatio, value } => {
                    let ratio = setting::ratio(&value).map_err(problem)?;
                    Setting::StopSeeding { at: SeedingLimit::Ratio, value: Some(ratio.hundredths()) }
                }
                SettingsChange::Set { name: SettingName::SeedTime, value } => {
                    let time = setting::seed_time(&value).map_err(problem)?;
                    Setting::StopSeeding { at: SeedingLimit::Time, value: Some(time.minutes()) }
                }
                SettingsChange::Unset { name: SettingName::SeedRatio } => Setting::StopSeeding { at: SeedingLimit::Ratio, value: None },
                SettingsChange::Unset { name: SettingName::SeedTime } => Setting::StopSeeding { at: SeedingLimit::Time, value: None },
            };
            print_settings(&client.set_setting(setting)?);
        }
        Command::Keepalive => {
            println!("keeping service {} alive, Ctrl+C to release", client.service_version);
            client.wait_closed()?;
            println!("service stopped");
        }
        Command::Stop => unreachable!(),
    }
    Ok(())
}

/// The CLI's own fix for a refusal, after the shared words.
fn with_fix(failure: Failure) -> anyhow::Error {
    match failure {
        Failure::Refused(Refusal::NoDestination) => {
            anyhow::anyhow!("{failure}: pass --destination or set one with `phantom settings set default-destination <dir>`")
        }
        failure => failure.into(),
    }
}

/// A limit as typed (`None`: unlimited), or why the text isn't one.
fn limit(direction: Direction, text: Option<&str>) -> Result<Setting> {
    let limit = text.map(setting::speed).transpose().map_err(|p| anyhow::anyhow!("{}", display::value_problem(p)))?;
    Ok(Setting::SpeedLimit { direction, bytes_per_sec: limit.map(u64::from) })
}

fn print_settings(settings: &Settings) {
    let destination = match &settings.default_destination {
        Some(folder) => folder.display().to_string(),
        None => "none (every add needs --destination)".into(),
    };
    println!("default-destination  {destination}");
    let port = settings.listen_port.port();
    println!("listening-port       {port} ({})", display::listen_port(settings.listen_port, settings.listening_on));
    println!("upnp                 {}", if settings.upnp { "on" } else { "off" });
    println!("keep-awake           {}", if settings.keep_awake { "on" } else { "off" });
    for (name, direction) in [("download-limit", Direction::Download), ("upload-limit", Direction::Upload)] {
        let limit = settings.speed_limits.get(direction).map_or("unlimited".into(), display::speed_limit);
        println!("{name:<21}{limit}");
    }
    println!("seed-ratio           {}", settings.seeding.ratio.map_or("never".into(), display::ratio));
    println!("seed-time            {}", settings.seeding.time.map_or("never".into(), display::seed_time));
    if let Some(notice) = display::settings_reset(&settings.reset) {
        eprintln!("note: {notice} (the file is kept as settings.invalid.json)");
    }
}

fn print_listing(torrents: &[TorrentInfo]) {
    if torrents.is_empty() {
        println!("no torrents");
        return;
    }
    // As wide as the longest status listed.
    let status = torrents.iter().map(|t| display::status(&t.status).len()).max().unwrap_or(0).max(13);
    println!(
        "{:>3}  {:<status$} {:>6}  {:>10}  {:>10}  {:>10}  {:>5}  {:>8}  NAME",
        "ID", "STATUS", "DONE", "SIZE", "DOWN", "UP", "PEERS", "ETA"
    );
    for t in torrents {
        // Unknown until the torrent's info arrives.
        let (done, size) = match &t.content {
            Some(c) => (format!("{:.1}%", c.progress() * 100.0), display::bytes(c.size_bytes)),
            None => ("-".into(), "-".into()),
        };
        println!(
            "{:>3}  {:<status$} {:>6}  {:>10}  {:>8}/s  {:>8}/s  {:>5}  {:>8}  {}",
            t.id,
            display::status(&t.status).to_lowercase(),
            done,
            size,
            display::bytes(t.download_bytes_per_sec),
            display::bytes(t.upload_bytes_per_sec),
            t.live_peers,
            t.eta().map(display::duration).unwrap_or_else(|| "-".into()),
            t.display_name(),
        );
        // Why, under the row: e.g. "Drive MyDisk isn't connected", or the error.
        if let Some(reason) = display::status_reason(t.display_name(), &t.status) {
            println!("     {reason}");
        }
    }
}

fn print_files(details: &Details) {
    println!("{}", details.location.display());
    println!("{:>5}  {:>7}  {:>10}  {:<8}  PATH", "INDEX", "DONE", "SIZE", "PRIORITY");
    for f in &details.files {
        let done = if f.missing { "missing".into() } else { format!("{:.1}%", f.progress() * 100.0) };
        println!(
            "{:>5}  {:>7}  {:>10}  {:<8}  {}",
            f.index,
            done,
            display::bytes(f.size_bytes),
            display::priority(f.priority).to_lowercase(),
            f.path.display(),
        );
    }
}
