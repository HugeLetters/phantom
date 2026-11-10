//! `phantom`: the command line client. Parses arguments, makes requests through `client` and
//! prints the answers.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use client::{Client, Failure, display};
use proto::{AddSource, Details, Priority, Refusal, Setting, Settings, Status, TorrentCommand, TorrentId, TorrentInfo};

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
    /// Remove a torrent and delete its files
    Delete {
        id: TorrentId,
    },
    /// List a torrent's files with their progress, size and priority
    Files {
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
    /// default-destination FOLDER
    Set { name: SettingName, value: String },
    /// Clear the default destination, so every add needs --destination
    Unset { name: SettingName },
}

#[derive(Clone, Copy, ValueEnum)]
enum SettingName {
    DefaultDestination,
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
        Command::List => print_listing(&client.list()?),
        Command::Watch => loop {
            let listing = client.list()?;
            // Clear screen and move the cursor home.
            print!("\x1b[2J\x1b[H");
            print_listing(&listing);
            std::thread::sleep(Duration::from_secs(1));
        },
        Command::Add { source, paused, destination } => {
            let destination = destination.as_deref().map(client::absolute).transpose()?;
            let source = if Path::new(&source).is_file() {
                AddSource::TorrentFile(std::fs::read(&source).with_context(|| format!("reading {source}"))?)
            } else {
                AddSource::Url(source)
            };
            let added = client.add(source, paused, destination).map_err(with_fix)?;
            // The id is the CLI's own: what the other commands take.
            println!("{} (id {})", display::added(&added), added.id);
        }
        Command::Pause { id } => client.command(id, TorrentCommand::Pause)?,
        Command::Resume { id } => client.command(id, TorrentCommand::Resume)?,
        Command::Check { id } => client.command(id, TorrentCommand::Check)?,
        Command::Remove { id } => client.command(id, TorrentCommand::Remove)?,
        Command::Delete { id } => client.command(id, TorrentCommand::Delete)?,
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
            let setting = match change {
                SettingsChange::Set { name: SettingName::DefaultDestination, value } => {
                    Setting::DefaultDestination(Some(client::absolute(Path::new(&value))?))
                }
                SettingsChange::Unset { name: SettingName::DefaultDestination } => Setting::DefaultDestination(None),
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

fn print_settings(settings: &Settings) {
    let destination = match &settings.default_destination {
        Some(folder) => folder.display().to_string(),
        None => "none (every add needs --destination)".into(),
    };
    println!("default-destination  {destination}");
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
        let size = if t.status == Status::FetchingInfo { "-".into() } else { display::bytes(t.size_bytes) };
        println!(
            "{:>3}  {:<status$} {:>5.1}%  {:>10}  {:>8}/s  {:>8}/s  {:>5}  {:>8}  {}",
            t.id,
            display::status(&t.status).to_lowercase(),
            t.progress() * 100.0,
            size,
            display::bytes(t.download_bytes_per_sec),
            display::bytes(t.upload_bytes_per_sec),
            t.live_peers,
            t.eta().map(display::duration).unwrap_or_else(|| "-".into()),
            t.display_name(),
        );
        // Why, under the row: e.g. "Drive MyDisk isn't connected", or the error.
        if let Some(reason) = display::status_reason(&t.status) {
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
