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
use proto::{AbsolutePath, AddSource, Added, Setting, SettingSet, SettingUndone, Settings, Snapshot, TorrentCommand, TorrentId, Undone};
use rune::Tx;

use crate::model::Phantom;

#[cfg(test)]
pub use memory::{DOWNLOADS, Disk, Probe};
pub use memory::{DiskChange, MemoryService};
pub use socket::SocketService;

pub trait Service {
    /// `proto::Request::Add` for each of the sources, one after another; `done` gets how each
    /// ended, in order, after the listing that shows them. Without a destination or a default
    /// destination they're refused with [`AddError::NoDestination`].
    fn add(&mut self, sources: Vec<AddRequest>, destination: Option<AbsolutePath>, done: Done<Vec<Result<Added, AddError>>>);
    /// `proto::Request::Torrents`: `command` for every torrent of `ids`, in one request (a Remove or a
    /// Delete of several is one undo step, #235): `done` gets the ones that failed, with why (none:
    /// all of them did as asked).
    fn command(&mut self, ids: &[TorrentId], command: TorrentCommand, done: Done<Vec<(TorrentId, Failure)>>);
    /// `proto::Request::Moves`: each of `moves` into `to`, one after another in the order given,
    /// each with its own choice for a clash (#270): `done` gets the ones that didn't move, with why.
    fn moves(&mut self, to: AbsolutePath, moves: Vec<proto::MoveOf>, done: Done<Vec<(TorrentId, Failure)>>);
    /// `proto::Request::Clashes`: which of `ids` a Move into `to` would find files at already.
    fn clashes(&mut self, ids: Vec<TorrentId>, to: AbsolutePath, done: Done<Result<Vec<proto::Clashing>, Failure>>);
    /// `proto::Request::FreeName`: a name torrent `id` could take in `to`, nothing at it there.
    fn free_name(&mut self, id: TorrentId, to: AbsolutePath, done: Done<Result<proto::FileName, Failure>>);
    /// `proto::Request::Undo`: what was undone, or why it couldn't be.
    fn undo(&mut self, done: Done<Result<Undone, Failure>>);
    /// `proto::Request::Settings`: the service's settings.
    fn settings(&mut self, done: Done<Result<Settings, Failure>>);
    /// `proto::Request::SetSetting`, the change `by` the app's id for it: the settings once the change
    /// applies, or why it was refused.
    fn set_setting(&mut self, setting: Setting, by: Option<u64>, done: Done<Result<SettingSet, Failure>>);
    /// `proto::Request::UndoSetting`: the newest settings change undone, or why it couldn't be.
    fn undo_setting(&mut self, done: Done<Result<SettingUndone, Failure>>);
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
