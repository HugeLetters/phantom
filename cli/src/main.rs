//! `phantom`: the command line client. Parses arguments, makes requests through `client` and
//! prints the answers.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use client::{Client, format};
use proto::{AddSource, Status, TorrentId, TorrentInfo};

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
    /// Take a torrent off the list, keeping its files (a torrent fetching info stops)
    #[command(alias = "rm")]
    Remove { id: TorrentId },
    /// Remove a torrent and delete its files
    Delete { id: TorrentId },
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
            let added = client.add(source, paused)?;
            let name = added.name.as_deref().unwrap_or("<unknown>");
            if added.already_existed {
                println!("already in the list as {}: {name}", added.id);
            } else {
                println!("added {}: {name}", added.id);
            }
        }
        Command::Pause { id } => client.pause(id)?,
        Command::Resume { id } => client.resume(id)?,
        Command::Remove { id } => client.remove(id)?,
        Command::Delete { id } => client.delete(id)?,
        Command::Keepalive => {
            println!("keeping service {} alive, Ctrl+C to release", client.service_version);
            client.wait_closed()?;
            println!("service stopped");
        }
        Command::Stop => unreachable!(),
    }
    Ok(())
}

fn print_listing(torrents: &[TorrentInfo]) {
    if torrents.is_empty() {
        println!("no torrents");
        return;
    }
    println!("{:>3}  {:<13} {:>6}  {:>10}  {:>10}  {:>10}  {:>5}  {:>8}  NAME", "ID", "STATUS", "DONE", "SIZE", "DOWN", "UP", "PEERS", "ETA");
    for t in torrents {
        // Unknown until the torrent's info arrives.
        let size = if t.status == Status::FetchingInfo { "-".into() } else { format::bytes(t.size_bytes) };
        println!(
            "{:>3}  {:<13} {:>5.1}%  {:>10}  {:>8}/s  {:>8}/s  {:>5}  {:>8}  {}",
            t.id,
            t.status.label().to_lowercase(),
            t.progress() * 100.0,
            size,
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
}
