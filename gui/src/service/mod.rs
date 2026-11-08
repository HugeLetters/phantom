//! The GUI's seam to the service: [`Service`] is everything the app asks of whatever runs the
//! torrents.
//!
//! `main` picks the implementation: a real window gets [`SocketService`], headless runs and tests
//! get [`MemoryService`], so they can't touch real torrents unless handed the socket one on
//! purpose.
//!
//! Actions are fire-and-forget; results come back as [`Event`]s that the app drains in Rune's
//! `poll`, once per frame. The fake answers on the app clock, which keeps tests deterministic.

mod memory;
mod socket;

use std::path::PathBuf;
use std::time::Duration;

use proto::{AbsolutePath, Added, Details, Priority, TorrentId, TorrentInfo};

pub use memory::MemoryService;
pub use socket::SocketService;

pub trait Service {
    /// Adds the torrents, one after another, into `destination`, or the default destination if
    /// `None`. Without either the service refuses: [`Event::NoDestination`] hands them back.
    fn add(&mut self, sources: Vec<AddRequest>, destination: Option<AbsolutePath>);
    fn pause(&mut self, ids: &[TorrentId]);
    fn resume(&mut self, ids: &[TorrentId]);
    /// Checks the torrents' files on disk, keeping what's valid.
    fn check(&mut self, ids: &[TorrentId]);
    /// Takes the torrents off the list, keeping their files; torrents fetching info stop.
    fn remove(&mut self, ids: &[TorrentId]);
    /// Removes the torrents and deletes their files.
    fn delete(&mut self, ids: &[TorrentId]);
    /// Keep sending [`Event::Details`] of this torrent, fresh with the list, until told another
    /// or `None`.
    fn watch_details(&mut self, id: Option<TorrentId>);
    /// Sets the priority of the torrent's files at `files` (their indices).
    fn set_priority(&mut self, id: TorrentId, files: &[usize], priority: Priority);
    /// Everything that happened since the last call. `now` is the app clock in seconds.
    fn poll(&mut self, now: f64) -> Vec<Event>;
    /// When the app should poll again even if nothing wakes it (a fake simulating progress).
    fn next_wake(&self) -> Option<Duration>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum AddRequest {
    /// Magnet link, bare info hash, or http(s) URL of a .torrent file.
    Url(String),
    /// A local .torrent file.
    File(PathBuf),
}

#[derive(Debug)]
pub enum Event {
    Listing(Vec<TorrentInfo>),
    /// How an add ended, after the listing that shows it.
    Added(Added),
    /// These adds named no destination and there's no default destination: ask, and add them
    /// again into the folder chosen.
    NoDestination(Vec<AddRequest>),
    /// The files of the torrent [`Service::watch_details`] named.
    Details(Details),
    Connection(Connection),
    Notice(Notice),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Connection {
    Connecting,
    Connected,
    Lost(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

impl Notice {
    pub fn info(text: impl Into<String>) -> Self {
        Self { text: text.into(), error: false }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self { text: text.into(), error: true }
    }
}
