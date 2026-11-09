//! Service settings persisted as `settings.json` in the data dir. Settings are the defaults plus
//! whatever the file overrides: a setting that is missing, or invalid, has its default, so files
//! from older versions load ([`proto::tolerant`]). The ones reset are reported, and the bad file
//! is kept as `settings.invalid.json`.

use std::net::{Ipv6Addr, TcpListener};
use std::path::{Path, PathBuf};

use anyhow::Result;
use proto::AbsolutePath;
use proto::tolerant::{self, Decoded};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Port for incoming peer connections. 0 means "not picked yet".
    pub listen_port: u16,
    /// The user chose `listen_port` (e.g. for a manual router forward), so never change it automatically.
    pub listen_port_pinned: bool,
    /// Ask the router to forward `listen_port` to us.
    pub upnp: bool,
    /// Where an add that names no destination goes. None: clients ask on every add.
    pub default_destination: Option<AbsolutePath>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { listen_port: 0, listen_port_pinned: false, upnp: true, default_destination: None }
    }
}

impl Settings {
    /// The settings in `dir`, and the names of those reset. A file with resets is repaired: the
    /// settings are written over it, after a copy of it is kept.
    pub fn load(dir: &Path) -> Result<Decoded<Self>> {
        let decoded = tolerant::load::<Self>(&path(dir))?;
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

    /// Returns the port to listen on this session. An automatically picked port that is
    /// taken gets replaced and saved; a pinned one is kept and a temporary port used instead.
    //
    // Port policy: a random port is picked once and kept, so a forward the user set up by hand
    // on their router keeps working across restarts. A port the user set (pinned) is never
    // changed. UPnP is on by default, as in Transmission and qBittorrent, so most routers
    // forward the port without the user doing anything.
    pub fn resolve_listen_port(&mut self, dir: &Path) -> Result<u16> {
        if self.listen_port != 0 && port_is_free(self.listen_port) {
            return Ok(self.listen_port);
        }
        let fresh = random_free_port()?;
        if self.listen_port_pinned {
            warn!(pinned = self.listen_port, temporary = fresh, "pinned listen port is taken, using a temporary one");
            return Ok(fresh);
        }
        if self.listen_port != 0 {
            info!(old = self.listen_port, new = fresh, "listen port is taken, picked a new one");
        }
        self.listen_port = fresh;
        self.save(dir)?;
        Ok(fresh)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn settings() -> impl Strategy<Value = Settings> {
        (any::<u16>(), any::<bool>(), any::<bool>(), proptest::option::of("/[a-z]{1,8}(/[a-z]{1,8}){0,3}")).prop_map(
            |(listen_port, listen_port_pinned, upnp, folder)| Settings {
                listen_port,
                listen_port_pinned,
                upnp,
                default_destination: folder.map(|f| AbsolutePath::new(f).unwrap()),
            },
        )
    }

    proptest! {
        #[test]
        fn saved_settings_load_back_as_they_were(settings in settings()) {
            let dir = tempfile::tempdir().unwrap();
            settings.save(dir.path()).unwrap();
            prop_assert_eq!(Settings::load(dir.path()).unwrap(), Decoded { value: settings, reset: vec![] });
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
fn port_is_free(port: u16) -> bool {
    TcpListener::bind((Ipv6Addr::UNSPECIFIED, port)).is_ok()
}

/// Lets the OS pick a free port from its ephemeral range (49152-65535 on macOS).
fn random_free_port() -> Result<u16> {
    Ok(TcpListener::bind((Ipv6Addr::UNSPECIFIED, 0))?.local_addr()?.port())
}
