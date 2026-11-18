//! The default destination, watched like a torrent's destination is (#209): whether it can be
//! reached now (a drive that isn't connected, a folder that's gone, no permission), looked at off
//! the actor as every look is ([`super::looks`]), and one that takes longer than [`super::space::STALLED_AFTER`]
//! is reported unreachable (`CantOpen`) until it answers. Every
//! snapshot then says why it can't be reached, so clients show it as it changes: Settings' row,
//! and `phantom settings`.
//!
//! The service tells the engine which folder it is ([`super::Engine::set_default_destination`]):
//! at start, and whenever the setting changes.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use proto::{AbsolutePath, Unavailable};
use tokio::sync::watch;

use super::looks::Looked;

/// How a folder is looked at: [`look`], or in tests one that stalls.
pub type Look = Arc<dyn Fn(&AbsolutePath) -> Option<Unavailable> + Send + Sync>;

/// Why the folder can't be reached, if it can't: it only reads, never creates anything.
pub fn look() -> Look {
    Arc::new(super::files::unavailable)
}

/// What a look at `folder` found, for the actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub folder: AbsolutePath,
    pub unavailable: Option<Unavailable>,
}

/// The folder to look at, as [`super::looks::watch`] takes it: none, or the one.
pub type Folder = BTreeMap<AbsolutePath, ()>;

/// Looks at the folder every `every`, and at once when it changes ([`super::looks::watch`]), and
/// tells `report` what it found; while a look is stuck, that the folder can't be opened.
pub fn watch(look: Look, folder: watch::Receiver<Folder>, every: Duration, report: impl Fn(Report) -> bool + Send + Sync + 'static) {
    let look: Arc<dyn Fn(&AbsolutePath, &()) -> Option<Unavailable> + Send + Sync> = Arc::new(move |folder, _| look(folder));
    super::looks::watch(look, folder, every, move |folder, found| {
        let unavailable = match found {
            Looked::Read(unavailable) => unavailable,
            Looked::Stalled => Some(Unavailable::CantOpen { path: folder.clone() }),
        };
        report(Report { folder, unavailable })
    });
}
