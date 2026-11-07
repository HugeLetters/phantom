//! `phantom`: the command line client. Parses arguments, makes requests through `client` and
//! prints the answers.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use client::{Client, format};
use proto::{AddSource, Listing, PendingId, TorrentId};

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
    },
    Pause { id: TorrentId },
    Resume { id: TorrentId },
    /// Remove a torrent, keeping downloaded files unless --delete is given
    #[command(alias = "rm")]
    Remove {
        id: TorrentId,
        #[arg(long)]
        delete: bool,
    },
    /// Cancel an add that is still resolving (the P-numbered lines in `list`)
    Cancel { id: PendingId },
    /// Keep the service running until Ctrl+C
    Keepalive,
    /// Stop the service, even if other clients are connected
    Stop,
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
        Command::Add { source, paused } => {
            let source = if Path::new(&source).is_file() {
                AddSource::TorrentFile(std::fs::read(&source).with_context(|| format!("reading {source}"))?)
            } else {
                AddSource::Url(source)
            };
            eprintln!("adding (magnets may take a while to resolve)...");
            let (id, name, already_existed) = client.add(source, paused)?;
            let name = name.as_deref().unwrap_or("<unknown>");
            if already_existed {
                println!("already added as {id}: {name}");
            } else {
                println!("added {id}: {name}");
            }
        }
        Command::Pause { id } => client.pause(id)?,
        Command::Resume { id } => client.resume(id)?,
        Command::Remove { id, delete } => client.remove(id, delete)?,
        Command::Cancel { id } => client.cancel_add(id)?,
        Command::Keepalive => {
            println!("keeping service {} alive, Ctrl+C to release", client.service_version);
            client.wait_closed()?;
            println!("service stopped");
        }
        Command::Stop => unreachable!(),
    }
    Ok(())
}

fn print_listing(Listing { torrents, pending }: &Listing) {
    if torrents.is_empty() && pending.is_empty() {
        println!("no torrents");
        return;
    }
    println!("{:>3}  {:<12} {:>6}  {:>10}  {:>10}  {:>10}  {:>5}  {:>8}  NAME", "ID", "STATE", "DONE", "SIZE", "DOWN", "UP", "PEERS", "ETA");
    for t in torrents {
        println!(
            "{:>3}  {:<12} {:>5.1}%  {:>10}  {:>8}/s  {:>8}/s  {:>5}  {:>8}  {}",
            t.id,
            format::status(t),
            t.progress() * 100.0,
            format::bytes(t.total_bytes),
            format::bytes(t.download_bytes_per_sec),
            format::bytes(t.upload_bytes_per_sec),
            t.live_peers,
            t.eta().map(format::duration).unwrap_or_else(|| "-".into()),
            t.name.as_deref().unwrap_or("<unknown>"),
        );
        if let Some(error) = &t.error {
            println!("     error: {error}");
        }
    }
    for p in pending {
        let name = p.name.as_deref().unwrap_or(&p.source);
        let waiting = format::duration(std::time::Duration::from_secs(p.elapsed_secs));
        println!("{:>3}  {:<12} {:>6}  {:>10}  {:>10}  {:>10}  {:>5}  {:>8}  {name} (waiting {waiting})", format!("P{}", p.id), "resolving", "", "", "", "", "", "");
    }
}
