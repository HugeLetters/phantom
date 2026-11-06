//! The app's one service: whatever runs the torrents.
//!
//! `main` picks the implementation: a real window gets [`SocketDaemon`], headless runs and tests
//! get [`MemoryDaemon`], so they can't touch real torrents unless handed the socket one on purpose.
//!
//! Actions are fire-and-forget; results come back as [`Event`]s that the app drains in Rune's
//! `poll`, once per frame. The fake answers on the app clock, which keeps tests deterministic.

mod memory;
mod socket;

use std::path::PathBuf;
use std::time::Duration;

use proto::{Listing, PendingId, TorrentId};

pub use memory::MemoryDaemon;
pub use socket::SocketDaemon;

pub trait Daemon {
    fn add(&mut self, source: AddRequest);
    fn pause(&mut self, ids: &[TorrentId]);
    fn resume(&mut self, ids: &[TorrentId]);
    fn remove(&mut self, ids: &[TorrentId], delete_files: bool);
    fn cancel_adds(&mut self, ids: &[PendingId]);
    /// Everything that happened since the last call. `now` is the app clock in seconds.
    fn poll(&mut self, now: f64) -> Vec<Event>;
    /// Actions sent that haven't answered yet.
    fn in_flight(&self) -> usize;
    /// When the app should poll again even if nothing wakes it (a fake simulating progress).
    fn next_wake(&self) -> Option<Duration>;
}

pub enum AddRequest {
    /// Magnet link, bare info hash, or http(s) URL of a .torrent file.
    Url(String),
    /// A local .torrent file.
    File(PathBuf),
}

#[derive(Debug)]
pub enum Event {
    Listing(Listing),
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
