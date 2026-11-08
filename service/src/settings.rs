//! Service settings persisted as `settings.json` in the data dir. A setting missing from the
//! file has its default, so files from older versions load.

use std::net::{Ipv6Addr, TcpListener};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::AbsolutePath;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    /// Port for incoming peer connections. 0 means "not picked yet".
    pub listen_port: u16,
    /// The user chose `listen_port` (e.g. for a manual router forward), so never change it automatically.
    pub listen_port_pinned: bool,
    /// Ask the router to forward `listen_port` to us.
    pub upnp: bool,
    /// Where an add that names no destination goes. None: clients ask on every add.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_destination: Option<AbsolutePath>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { listen_port: 0, listen_port_pinned: false, upnp: true, default_destination: None }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Result<Self> {
        match std::fs::read(path(dir)) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("invalid settings.json"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
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
    use super::*;

    #[test]
    fn settings_from_before_the_default_destination_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path(dir.path()), r#"{"listen_port": 51413, "listen_port_pinned": true, "upnp": false}"#).unwrap();
        let settings = Settings::load(dir.path()).unwrap();
        assert_eq!((settings.listen_port, settings.listen_port_pinned, settings.upnp), (51413, true, false));
        assert_eq!(settings.default_destination, None);
    }

    #[test]
    fn a_default_destination_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings { default_destination: Some(AbsolutePath::new("/Volumes/Disk/Torrents").unwrap()), ..Default::default() };
        settings.save(dir.path()).unwrap();
        let loaded = Settings::load(dir.path()).unwrap();
        assert_eq!(loaded.default_destination, settings.default_destination);
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
