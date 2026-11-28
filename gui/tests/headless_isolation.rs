//! A headless run (a still, a GIF) must not touch the user's own data dir, wherever `HOME` is.

use std::path::Path;
use std::process::Command;

/// The directory tree under `dir`, as `(relative path, bytes)`, sorted.
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.strip_prefix(root).unwrap().display().to_string();
            if path.is_dir() {
                out.push((name, vec![]));
                walk(root, &path, out);
            } else {
                out.push((name, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn a_headless_start_with_the_default_data_dir_leaves_it_untouched() {
    let home = tempfile::tempdir().unwrap();
    // Where the default data dir is, with a `gui.json` the startup repair would rewrite.
    let default_dir = if cfg!(target_os = "macos") {
        home.path().join("Library/Application Support/Phantom")
    } else {
        home.path().join(".local/share/phantom")
    };
    std::fs::create_dir_all(&default_dir).unwrap();
    std::fs::write(default_dir.join("gui.json"), br#"{"app_icon_written": 5, "bogus": }"#).unwrap();
    let before = snapshot(home.path());

    let out = tempfile::tempdir().unwrap();
    let mut gui = Command::new(env!("CARGO_BIN_EXE_phantom-gui"));
    // Linux's GPU driver (Mesa) keeps its shader cache in `~/.cache`: not Phantom's to leave alone.
    #[cfg(not(target_os = "macos"))]
    gui.env("XDG_CACHE_HOME", out.path());
    let status = gui
        .args(["--snap"])
        .arg(out.path().join("out.png"))
        .args(["--scene", "demo", "--size", "640x320"])
        .env("HOME", home.path())
        .env_remove("PHANTOM_DATA_DIR")
        .env_remove("PHANTOM_DOWNLOAD_DIR")
        .env_remove("XDG_DATA_HOME")
        .status()
        .unwrap();

    assert!(status.success(), "the headless run failed: {status}");
    assert!(out.path().join("out.png").exists(), "no still was rendered");
    assert_eq!(snapshot(home.path()), before, "a headless run changed the default data dir");
}
