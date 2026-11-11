//! Service settings persisted as `settings.json` in the data dir. Settings are the defaults plus
//! whatever the file overrides: a setting that is missing, or invalid, has its default, so files
//! from older versions load ([`proto::tolerant`]). The ones reset are reported, and the bad file
//! is kept as `settings.invalid.json`.

pub mod machine;

use std::net::{Ipv6Addr, TcpListener};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::AbsolutePath;
use proto::tolerant::{self, Decoded};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use machine::{Do, Event, Observed, State as ListenPort};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(from = "File", into = "File")]
pub struct Settings {
    /// Port for incoming peer connections ([`machine`]).
    pub listen_port: ListenPort,
    /// Ask the router to forward the port to us.
    pub upnp: bool,
    /// Where an add that names no destination goes. None: clients ask on every add.
    pub default_destination: Option<AbsolutePath>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { listen_port: ListenPort::Unpicked, upnp: true, default_destination: None }
    }
}

/// `settings.json` as written, the same since before [`ListenPort`]: its port is two fields, 0
/// meaning not picked yet. A file pinning 0 is unpicked.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct File {
    listen_port: u16,
    listen_port_pinned: bool,
    upnp: bool,
    default_destination: Option<AbsolutePath>,
}

impl Default for File {
    fn default() -> Self {
        Settings::default().into()
    }
}

impl From<File> for Settings {
    fn from(file: File) -> Self {
        let listen_port = match (NonZeroU16::new(file.listen_port), file.listen_port_pinned) {
            (None, _) => ListenPort::Unpicked,
            (Some(port), false) => ListenPort::Picked(port),
            (Some(port), true) => ListenPort::Pinned(port),
        };
        Self { listen_port, upnp: file.upnp, default_destination: file.default_destination }
    }
}

impl From<Settings> for File {
    fn from(settings: Settings) -> Self {
        Self {
            listen_port: settings.listen_port.port().map_or(0, NonZeroU16::get),
            listen_port_pinned: matches!(settings.listen_port, ListenPort::Pinned(_)),
            upnp: settings.upnp,
            default_destination: settings.default_destination,
        }
    }
}

/// Kept by the service itself: a person doesn't set whether a port is pinned.
const INTERNAL: [&str; 1] = ["listen_port_pinned"];

impl Settings {
    /// The settings in `dir`, and the names of those reset. A file with resets is repaired: the
    /// settings are written over it, after a copy of it is kept.
    pub fn load(dir: &Path) -> Result<Decoded<Self>> {
        let decoded = tolerant::load::<Self>(&path(dir), &INTERNAL)?;
        if !decoded.reset.is_empty() {
            warn!(reset = ?decoded.reset, "invalid settings reset to their defaults, the file is kept as settings.invalid.json");
            decoded.value.save(dir)?;
        }
        Ok(decoded)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::write(path(dir), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    /// The port to listen on as the engine starts, and the port setting as clients see it, decided
    /// by the [`machine`] from whether the port is free; a port newly picked is saved.
    //
    // Port policy: a random port is picked once and kept, so a forward the user set up by hand
    // on their router keeps working across restarts. A port the user set (pinned) is never
    // changed. UPnP is on by default, as in Transmission and qBittorrent, so most routers
    // forward the port without the user doing anything.
    pub fn start_listening(&mut self, dir: &Path) -> Result<(NonZeroU16, proto::ListenPort)> {
        let free = self.listen_port.port().is_some_and(port_is_free);
        let observed = Observed::Starting { free, fresh: random_free_port()? };
        let Ok(decided) = machine::decide(&self.listen_port, &Event::Observed(observed));
        if decided != self.listen_port {
            info!(old = ?self.listen_port, new = ?decided, "picked a listen port");
            self.listen_port = decided;
            self.save(dir)?;
        }
        let sent = decided.sent().context("no listen port after a start")?;
        let port = match machine::next(&decided, &observed).context("no listen port after a start")? {
            Do::Listen(port) => port,
            Do::ListenTemporarily(port) => {
                warn!(pinned = ?decided, temporary = port, "pinned listen port is taken, using a temporary one");
                port
            }
        };
        Ok((port, sent))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn listen_port() -> impl Strategy<Value = ListenPort> {
        let port = || (1..=u16::MAX).prop_map(|p| NonZeroU16::new(p).unwrap());
        prop_oneof![Just(ListenPort::Unpicked), port().prop_map(ListenPort::Picked), port().prop_map(ListenPort::Pinned)]
    }

    fn settings() -> impl Strategy<Value = Settings> {
        (listen_port(), any::<bool>(), proptest::option::of("/[a-z]{1,8}(/[a-z]{1,8}){0,3}")).prop_map(|(listen_port, upnp, folder)| {
            Settings { listen_port, upnp, default_destination: folder.map(|f| AbsolutePath::new(f).unwrap()) }
        })
    }

    proptest! {
        #[test]
        fn saved_settings_load_back_as_they_were(settings in settings()) {
            let dir = tempfile::tempdir().unwrap();
            settings.save(dir.path()).unwrap();
            prop_assert_eq!(Settings::load(dir.path()).unwrap(), Decoded { value: settings, reset: vec![] });
        }

        #[test]
        fn an_invalid_internal_field_is_reset_silently(settings in settings()) {
            let dir = tempfile::tempdir().unwrap();
            let mut file = serde_json::to_value(&settings).unwrap();
            file["listen_port_pinned"] = "yes".into();
            std::fs::write(path(dir.path()), serde_json::to_vec(&file).unwrap()).unwrap();
            let loaded = Settings::load(dir.path()).unwrap();
            let listen_port = settings.listen_port.port().map_or(ListenPort::Unpicked, ListenPort::Picked);
            prop_assert_eq!(loaded, Decoded { value: Settings { listen_port, ..settings }, reset: vec![] });
            prop_assert!(!tolerant::invalid_copy(&path(dir.path())).exists());
        }

        /// Files from before `ListenPort`, which stored any pair, load: 0 is unpicked.
        #[test]
        fn the_two_port_fields_load_as_a_listen_port(port in any::<u16>(), pinned in any::<bool>()) {
            let file = serde_json::json!({ "listen_port": port, "listen_port_pinned": pinned });
            let loaded = tolerant::decode::<Settings>(&serde_json::to_vec(&file).unwrap(), &INTERNAL);
            let expected = match NonZeroU16::new(port) {
                None => ListenPort::Unpicked,
                Some(port) if pinned => ListenPort::Pinned(port),
                Some(port) => ListenPort::Picked(port),
            };
            prop_assert_eq!(loaded.value.listen_port, expected);
            prop_assert!(loaded.reset.is_empty());
        }

        /// A start against real sockets: the port it listens on is the state's when free; a picked
        /// port that's taken is replaced and saved, a pinned one kept.
        #[test]
        fn a_start_listens_where_the_machine_says_and_saves_what_it_picked(listen_port in listen_port(), taken in any::<bool>()) {
            let dir = tempfile::tempdir().unwrap();
            // A port that's free, held while `taken`.
            let holder = TcpListener::bind((Ipv6Addr::UNSPECIFIED, 0)).unwrap();
            let free = NonZeroU16::new(holder.local_addr().unwrap().port()).unwrap();
            let _held = taken.then_some(holder);
            let listen_port = match listen_port {
                ListenPort::Unpicked => ListenPort::Unpicked,
                ListenPort::Picked(_) => ListenPort::Picked(free),
                ListenPort::Pinned(_) => ListenPort::Pinned(free),
            };
            let mut settings = Settings { listen_port, ..Settings::default() };
            let (port, _) = settings.start_listening(dir.path()).unwrap();
            let saved = Settings::load(dir.path()).unwrap().value.listen_port;
            match listen_port {
                ListenPort::Unpicked => prop_assert_eq!(settings.listen_port, ListenPort::Picked(port)),
                ListenPort::Picked(p) if !taken => prop_assert_eq!((settings.listen_port, port), (ListenPort::Picked(p), p)),
                ListenPort::Picked(p) => {
                    prop_assert_eq!(settings.listen_port, ListenPort::Picked(port));
                    prop_assert_ne!(port, p);
                }
                ListenPort::Pinned(p) if !taken => prop_assert_eq!((settings.listen_port, port), (listen_port, p)),
                ListenPort::Pinned(p) => {
                    prop_assert_eq!(settings.listen_port, listen_port);
                    prop_assert_ne!(port, p);
                }
            }
            if settings.listen_port != listen_port {
                prop_assert_eq!(saved, settings.listen_port);
            }
        }

        #[test]
        fn an_invalid_setting_is_reset_the_rest_kept_and_the_file_repaired(settings in settings(), bad in "[a-z ]{1,8}") {
            let dir = tempfile::tempdir().unwrap();
            let mut file = serde_json::to_value(&settings).unwrap();
            file["upnp"] = bad.clone().into();
            let original = serde_json::to_vec(&file).unwrap();
            std::fs::write(path(dir.path()), &original).unwrap();
            let loaded = Settings::load(dir.path()).unwrap();
            prop_assert_eq!(loaded.reset, vec!["upnp"]);
            prop_assert_eq!(loaded.value, Settings { upnp: Settings::default().upnp, ..settings });
            prop_assert_eq!(std::fs::read(tolerant::invalid_copy(&path(dir.path()))).unwrap(), original);
            prop_assert!(Settings::load(dir.path()).unwrap().reset.is_empty(), "the file was repaired");
        }
    }
}

fn path(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

// librqbit listens on [::] (IPv4 and IPv6), so check the same address.
fn port_is_free(port: NonZeroU16) -> bool {
    TcpListener::bind((Ipv6Addr::UNSPECIFIED, port.get())).is_ok()
}

/// Lets the OS pick a free port from its ephemeral range (49152-65535 on macOS).
fn random_free_port() -> Result<NonZeroU16> {
    let port = TcpListener::bind((Ipv6Addr::UNSPECIFIED, 0))?.local_addr()?.port();
    NonZeroU16::new(port).context("the OS gave port 0")
}
