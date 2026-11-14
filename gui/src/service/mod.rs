//! The GUI's seam to the service: [`Service`] is everything the app asks of whatever runs the
//! torrents.
//!
//! `main` picks the implementation: a real window gets [`SocketService`], headless runs and tests
//! get [`MemoryService`], so they can't touch real torrents unless handed the socket one on
//! purpose.
//!
//! A command takes a completion, [`Done`], which its result runs: whoever sent it decides what the
//! answer does ([ADR 0007]). Live data (snapshots of the list with the watched torrents' details,
//! and the connection) arrives unasked, pushed by the service. Both come through one ordered queue
//! that the app drains in Rune's `poll`, once per frame, so the snapshot that shows an added
//! torrent comes before the add's answer. The fake answers on the app clock, which keeps tests
//! deterministic.
//!
//! [ADR 0007]: ../../../docs/adr/0007-commands-answer-their-caller.md

mod memory;
mod socket;

use std::path::PathBuf;
use std::time::Duration;

use client::{Failure, LinkProblem};
use proto::{AbsolutePath, AddSource, Added, Setting, Settings, Snapshot, TorrentCommand, TorrentId};
use rune::Tx;

use crate::model::Phantom;

#[cfg(test)]
pub use memory::{Disk, Probe};
pub use memory::{DiskChange, MemoryService};
pub use socket::SocketService;

pub trait Service {
    /// `proto::Request::Add` for each of the sources, one after another; `done` gets how each
    /// ended, in order, after the listing that shows them. Without a destination or a default
    /// destination they're refused with [`AddError::NoDestination`].
    fn add(&mut self, sources: Vec<AddRequest>, destination: Option<AbsolutePath>, done: Done<Vec<Result<Added, AddError>>>);
    /// `proto::Request::Torrent` for each of the torrents, one after another, until one fails.
    /// Sent to every torrent of `ids`, each on its own: `done` gets the ones that failed, with why
    /// (none: all of them did as asked).
    fn command(&mut self, ids: &[TorrentId], command: TorrentCommand, done: Done<Vec<(TorrentId, Failure)>>);
    /// `proto::Request::Settings`: the service's settings.
    fn settings(&mut self, done: Done<Result<Settings, Failure>>);
    /// `proto::Request::SetSetting`: the settings once the change applies, or why it was refused.
    fn set_setting(&mut self, setting: Setting, done: Done<Result<Settings, Failure>>);
    /// Have every [`Live::Snapshot`] carry the details of these torrents (those with info), until
    /// told others.
    fn watch_details(&mut self, ids: Vec<TorrentId>);
    /// Everything that came since the last call, in order. `now` is the app clock in seconds.
    fn poll(&mut self, now: f64) -> Vec<Incoming>;
    /// When the app should poll again even if nothing wakes it (a fake simulating progress).
    fn next_wake(&self) -> Option<Duration>;
}

/// What a command's result does to the app, given by whoever sends it.
pub type Done<T> = Box<dyn FnOnce(&mut Phantom, T, &mut Tx) + Send>;

/// A command's completion with its result, ready to run.
pub type Answer = Box<dyn FnOnce(&mut Phantom, &mut Tx) + Send>;

/// `done` with `result`, to run once the app drains it.
fn answer<T: Send + 'static>(done: Done<T>, result: T) -> Incoming {
    Incoming::Answer(Box::new(move |s, tx| done(s, result, tx)))
}

pub enum Incoming {
    Live(Live),
    Answer(Answer),
}

#[derive(Debug)]
pub enum Live {
    /// The list, with the details of the torrents [`Service::watch_details`] named, from one
    /// moment.
    Snapshot(Snapshot),
    Connection(Connection),
}

#[derive(Debug, Clone, PartialEq)]
pub enum AddRequest {
    /// A link, parsed: `text` as it was given, for the notes to name it.
    Link { text: String, source: AddSource },
    /// A local .torrent file.
    File(PathBuf),
}

impl AddRequest {
    /// The add of the link `text`, if it's one Add accepts ([`client::add_source`]).
    pub fn link(text: &str) -> Result<Self, LinkProblem> {
        Ok(Self::Link { text: text.trim().to_owned(), source: client::add_source(text)? })
    }
}

#[derive(Debug)]
pub enum AddError {
    /// Neither the add nor the settings name a destination: ask, and add it again into the
    /// folder chosen.
    NoDestination(AddRequest),
    Failed(Failure),
}

/// The link to the service: `client`'s connection machine, whose state the status bar words.
pub use client::machine::State as Connection;
