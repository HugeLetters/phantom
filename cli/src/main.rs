//! `phantom`: the command line client. Parses arguments, makes requests through `client` and
//! prints the answers.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use client::{Client, Failure, display, setting};
use proto::{
    AddSource, Clash, Details, Direction, Priority, Refusal, SeedingLimit, Setting, Settings, TorrentCommand, TorrentId, TorrentInfo,
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
    /// Stop a check that's running; the torrent is Unchecked until it's checked again
    CancelCheck {
        id: TorrentId,
    },
    /// Take torrents off the list, keeping their files (a torrent fetching info stops); one undo
    /// step for all of them
    #[command(alias = "rm")]
    Remove {
        #[arg(required = true)]
        ids: Vec<TorrentId>,
    },
    /// Remove torrents and move their files to the Trash; one undo step for all of them
    Delete {
        #[arg(required = true)]
        ids: Vec<TorrentId>,
    },
    /// Move a torrent's files into FOLDER, its destination from then on: renamed on the same drive,
    /// copied to another (then the originals go); it keeps what it had, without a full check.
    /// Answered once it's moved, or back where it was
    Move {
        id: TorrentId,
        folder: PathBuf,
        /// What to do with files of the torrent's already in FOLDER: use-existing keeps them (the
        /// torrent is checked there; its own copies go to the Trash), replace moves them to the
        /// Trash and puts the torrent's in their place. Without it, a Move that meets one says
        /// which, and moves nothing
        #[arg(long, value_enum)]
        clash: Option<ClashArg>,
    },
    /// Undo the last Remove or Delete (up to 20): the torrents come back as they were, and Delete's
    /// files out of the Trash, where they were
    Undo,
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
    /// Show the settings, or change one, or undo the last change
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
    /// Undo the last change to a setting (up to 20, made by any client since the service started):
    /// it goes back to what it was
    Undo,
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
enum ClashArg {
    UseExisting,
    Replace,
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
                Failure::Failed(error) if is_file => {
                    anyhow::anyhow!(display::service_error(&error)).context(format!("{source_arg} is a file here, read as a .torrent file"))
                }
                failure => with_fix(failure),
            })?;
            // The id is the CLI's own: what the other commands take.
            println!("{} (id {})", display::added(&added), added.id);
        }
        Command::Pause { id } => client.command(id, TorrentCommand::Pause)?,
        Command::Resume { id } => client.command(id, TorrentCommand::Resume)?,
        Command::Check { id } => client.command(id, TorrentCommand::Check)?,
        Command::CancelCheck { id } => client.command(id, TorrentCommand::CancelCheck)?,
        Command::Remove { ids } => commands(&mut client, &ids, TorrentCommand::Remove)?,
        Command::Delete { ids } => commands(&mut client, &ids, TorrentCommand::Delete)?,
        Command::Move { id, folder, clash } => {
            let to = client::absolute(&folder)?;
            let clash = clash.map(|c| match c {
                ClashArg::UseExisting => Clash::UseExisting,
                ClashArg::Replace => Clash::Replace,
            });
            client.command(id, TorrentCommand::Move { to, clash }).map_err(|failure| match failure {
                Failure::Refused(Refusal::Torrent { why: TorrentRefusal::Clash { .. }, .. }) => {
                    anyhow::anyhow!("{failure}: pass --clash use-existing to keep them, or --clash replace to put its own over them")
                }
                failure => failure.into(),
            })?;
        }
        // Of a selection, each torrent that didn't come back says why, and then how many: a partial
        // failure, as a Remove's is.
        Command::Undo => {
            let undone = client.undo()?;
            println!("{}", display::undone(&undone.undo));
            for not in &undone.refused {
                match display::not_back(not) {
                    (short, why) if why.is_empty() => eprintln!("{short}"),
                    (short, why) => eprintln!("{short}: {why}"),
                }
            }
            if !undone.refused.is_empty() {
                let total = undone.undo.torrents.len() + undone.refused.len();
                anyhow::bail!("{}", display::partly_undone(undone.refused.len(), total));
            }
        }
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
        Command::Settings { change: Some(SettingsChange::Undo) } => {
            println!("{}", undone_line(&client.undo_setting()?));
        }
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
                SettingsChange::Undo => unreachable!("undone above"),
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

/// `command` for each of `ids` (each once), in one request (a Remove or a Delete of several is one
/// undo step): one that failed fails as itself; of several, each says why, and then how many failed.
fn commands(client: &mut Client, ids: &[TorrentId], command: TorrentCommand) -> Result<()> {
    let action = command.action();
    let mut seen = std::collections::BTreeSet::new();
    let ids: Vec<TorrentId> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
    let mut failed = client.commands(&ids, command)?;
    match failed.len() {
        0 => Ok(()),
        1 if ids.len() == 1 => Err(failed.remove(0).1.into()),
        n => {
            for (_, failure) in &failed {
                eprintln!("{failure}");
            }
            anyhow::bail!("{}", display::commands_failed(action, n, ids.len()))
        }
    }
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

/// The default destination's lines: where, and under it why it can't be reached, if it can't.
fn destination_lines(settings: &Settings) -> Vec<String> {
    let destination = match &settings.default_destination {
        Some(folder) => folder.display().to_string(),
        None => "none (every add needs --destination)".into(),
    };
    let mut lines = vec![format!("default-destination  {destination}")];
    lines.extend(
        settings.default_destination_unavailable.iter().map(|reason| format!("                     {}", display::unavailable(reason))),
    );
    lines
}

/// What `phantom settings undo` prints: the setting it took back and what it went back to.
fn undone_line(undone: &proto::SettingUndone) -> String {
    display::setting_undone(undone.change.name, &undone.settings)
}

fn print_settings(settings: &Settings) {
    for line in destination_lines(settings) {
        println!("{line}");
    }
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
        // Why, under the row: e.g. "Drive MyDisk isn't connected", or the error; or how far a move is.
        if let Some(reason) = display::status_reason(t.display_name(), &t.status).or_else(|| display::move_progress(&t.status)) {
            println!("     {reason}");
        }
    }
}

fn print_files(details: &Details) {
    println!("{}", details.location.display());
    println!("{:>5}  {:>10}  {:>10}  {:<8}  PATH", "INDEX", "DONE", "SIZE", "PRIORITY");
    for f in &details.files {
        println!(
            "{:>5}  {:>10}  {:>10}  {:<8}  {}",
            f.index,
            file_done(f),
            display::bytes(f.size_bytes),
            display::priority(f.priority).to_lowercase(),
            f.path.display(),
        );
    }
}

/// A file's DONE column: how much of it is in, or why none of it is where it goes.
fn file_done(f: &proto::File) -> String {
    if f.held_aside {
        display::held_aside().to_lowercase()
    } else if f.missing {
        "missing".into()
    } else {
        format!("{:.1}%", f.progress() * 100.0)
    }
}

#[cfg(test)]
mod files_tests {
    use proto::{File, Priority};

    use super::file_done;

    #[test]
    fn a_file_held_aside_says_so_and_one_missing_otherwise_says_missing() {
        let file = |missing, held_aside| File {
            index: 0,
            path: "a".into(),
            size_bytes: 10,
            progress_bytes: 0,
            priority: Priority::Normal,
            missing,
            held_aside,
        };
        assert_eq!(file_done(&file(true, true)), "held aside");
        assert_eq!(file_done(&file(true, false)), "missing");
        assert_eq!(file_done(&file(false, false)), "0.0%");
    }
}

#[cfg(test)]
mod settings_tests {
    use proto::{AbsolutePath, ListenPort, Settings, Unavailable};

    use super::destination_lines;

    fn settings(folder: Option<&str>, unavailable: Option<Unavailable>) -> Settings {
        let port = std::num::NonZeroU16::new(51413).unwrap();
        Settings {
            default_destination: folder.map(|f| AbsolutePath::new(f).unwrap()),
            default_destination_unavailable: unavailable,
            listen_port: ListenPort::Picked(port),
            listening_on: port,
            upnp: true,
            keep_awake: false,
            speed_limits: Default::default(),
            seeding: Default::default(),
            reset: Vec::new(),
        }
    }

    /// `phantom settings undo` asks for the undo of the newest settings change, and says what it
    /// went back to. The service's own test (`settings_undo.rs`) undoes through the same request.
    #[test]
    fn settings_undo_takes_back_the_last_change_and_says_what_it_went_back_to() {
        use clap::Parser;

        use super::{Cli, Command, SettingsChange, undone_line};
        let cli = Cli::try_parse_from(["phantom", "settings", "undo"]).unwrap();
        assert!(matches!(cli.command, Command::Settings { change: Some(SettingsChange::Undo) }));
        let undone = |name, settings| proto::SettingUndone { change: proto::SettingChange { seq: 1, name, by: None }, settings };
        let port = undone(proto::SettingName::ListenPort, settings(None, None));
        assert_eq!(undone_line(&port), "Port is back to automatic (now 51413)", "a port the service picked");
        let pinned = Settings { listen_port: ListenPort::Pinned(std::num::NonZeroU16::new(6881).unwrap()), ..settings(None, None) };
        assert_eq!(undone_line(&undone(proto::SettingName::ListenPort, pinned)), "Port is back to 6881");
        let folder = undone(proto::SettingName::DefaultDestination, settings(Some("/srv/t"), None));
        assert_eq!(undone_line(&folder), "Default folder is back to /srv/t");
        assert_eq!(undone_line(&undone(proto::SettingName::Upnp, settings(None, None))), "UPnP is back to on");
    }

    #[test]
    fn the_default_destination_says_why_it_cant_be_reached_as_the_rows_do() {
        let drive = Unavailable::DriveNotConnected { name: "MyDisk".into() };
        assert_eq!(
            destination_lines(&settings(Some("/Volumes/MyDisk/t"), Some(drive))),
            ["default-destination  /Volumes/MyDisk/t", "                     Drive MyDisk isn't connected"]
        );
        assert_eq!(destination_lines(&settings(Some("/srv/t"), None)), ["default-destination  /srv/t"]);
        assert_eq!(destination_lines(&settings(None, None)), ["default-destination  none (every add needs --destination)"]);
    }
}
