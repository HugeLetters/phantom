//! Undo of settings end to end through `client` (#221), as `phantom settings undo` asks: changes
//! from any client come back newest first, a new port and UPnP restarting the engine on the way, a
//! default folder that can't be reached any more goes back all the same (its row says why), and
//! with nothing left the undo is refused. Every snapshot carries the settings and the changes, so
//! a subscriber sees another client's change at once, and the change it made itself by its own
//! id. An example rather than generated: it's the real service and its socket, with real restarts;
//! the app's machine and the fake's generated tests check the order.

mod fake;

use std::num::NonZeroU16;
use std::time::{Duration, Instant};

use client::{Client, Failure};
use proto::{AbsolutePath, Direction, Refusal, Setting, SettingName, SpeedLimit, Unavailable};

#[test]
fn settings_changes_are_undone_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    fake::isolate(dir.path());
    let service = env!("CARGO_BIN_EXE_phantom-service");
    let mut client = Client::connect_or_spawn(service.as_ref()).unwrap();
    assert!(matches!(client.undo_setting(), Err(Failure::Refused(Refusal::NothingToUndo))), "nothing yet");
    let start = client.settings().unwrap();

    let (films, music) = (dir.path().join("films"), dir.path().join("music"));
    std::fs::create_dir_all(&films).unwrap();
    std::fs::create_dir_all(&music).unwrap();
    let folder = |path: &std::path::Path| AbsolutePath::new(path).unwrap();
    let limit = SpeedLimit::new(512 * 1024).unwrap();
    let port = NonZeroU16::new(40_000 + (std::process::id() % 9_000) as u16).unwrap();
    client.set_setting(Setting::SpeedLimit { direction: Direction::Download, bytes_per_sec: Some(limit.into()) }).unwrap();
    client.set_setting(Setting::DefaultDestination(Some(folder(&films)))).unwrap();
    client.set_setting_by(Setting::DefaultDestination(Some(folder(&music))), Some(7)).unwrap();
    // The same value again changes nothing: no change to undo.
    client.set_setting(Setting::DefaultDestination(Some(folder(&music)))).unwrap();
    client.set_setting(Setting::ListenPort(Some(port))).unwrap();
    client.set_setting(Setting::Upnp(Some(!start.upnp))).unwrap();

    let configured = latest();
    let names: Vec<SettingName> = configured.changes.iter().map(|c| c.name).collect();
    use SettingName::*;
    assert_eq!(names, [DownloadLimit, DefaultDestination, DefaultDestination, ListenPort, Upnp], "each change, oldest first");
    assert_eq!(configured.changes.iter().map(|c| c.by).collect::<Vec<_>>(), [None, None, Some(7), None, None], "by its client's id");
    assert_eq!(configured.settings.listen_port, proto::ListenPort::Pinned(port), "the settings, as they are now");

    // The folder the undo goes back to is gone meanwhile.
    std::fs::remove_dir(&films).unwrap();
    let undo = |client: &mut Client| client.undo_setting().unwrap();
    let undone = undo(&mut client);
    assert_eq!((undone.change.name, undone.settings.upnp), (Upnp, start.upnp));
    let undone = undo(&mut client);
    assert_eq!((undone.change.name, undone.settings.listen_port), (ListenPort, start.listen_port), "the port it had, picked");
    assert!(matches!(start.listen_port, proto::ListenPort::Picked(_)), "a fresh service picks its port");
    let told = client::display::setting_undone(ListenPort, &undone.settings);
    assert_eq!(told, format!("Port is back to automatic (now {})", start.listen_port.port()));
    let undone = undo(&mut client);
    assert_eq!(undone.settings.default_destination, Some(folder(&films)), "back, though it can't be reached");
    let missing = Some(Unavailable::FolderMissing { path: folder(&films) });
    until(|| latest().settings.default_destination_unavailable == missing);
    let undone = undo(&mut client);
    assert_eq!(undone.settings.default_destination, start.default_destination);
    let undone = undo(&mut client);
    assert_eq!(undone.settings.speed_limits, start.speed_limits);
    assert!(matches!(client.undo_setting(), Err(Failure::Refused(Refusal::NothingToUndo))), "nothing left");
    assert!(latest().changes.is_empty());

    // A subscriber sees another client's change at once, and its undo.
    let mut stream = Client::open().unwrap().subscribe(Vec::new()).unwrap();
    let first = stream.receive().unwrap().settings.unwrap().at;
    client.set_setting(Setting::KeepAwake(Some(!start.keep_awake))).unwrap();
    // The first one sent after the change: one the engine's own changes sent meanwhile has the
    // settings as they were.
    let next = std::iter::repeat_with(|| stream.receive().unwrap().settings.unwrap()).find(|c| c.at > first).unwrap();
    assert!(next.at > first && next.settings.keep_awake != start.keep_awake && next.changes.len() == 1, "{next:?}");
    client.shutdown().unwrap();
}

/// The settings as the latest snapshot has them.
fn latest() -> proto::Configured {
    Client::open().unwrap().snapshot(Vec::new()).unwrap().settings.expect("the service sends its settings")
}

fn until(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "never happened: {:?}", latest());
        std::thread::sleep(Duration::from_millis(50));
    }
}
