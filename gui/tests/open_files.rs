//! The system's open-files event reaches Phantom's hook (`App::on_open_files`, wired in `main`): a
//! headless script delivers it as Rune's own tests do, and the app answers as it does to a file
//! opened from Finder. A file that isn't a .torrent is refused, which the status bar says.

use std::process::Command;

#[test]
fn files_opened_from_outside_reach_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let note = dir.path().join("notes.txt");
    std::fs::write(&note, "not a torrent").unwrap();
    let script = dir.path().join("open.json");
    std::fs::write(
        &script,
        serde_json::json!([{ "t": 0.3, "open_files": [note] }, { "t": 1.0, "type": "mark", "name": "end" }]).to_string(),
    )
    .unwrap();
    let tree = dir.path().join("tree.json");

    let out = Command::new(env!("CARGO_BIN_EXE_phantom-gui"))
        .arg("--script")
        .arg(&script)
        .arg("--tree")
        .arg(&tree)
        .args(["--scene", "demo", "--size", "960x600", "--at", "mark:end"])
        .env("PHANTOM_DATA_DIR", dir.path().join("data"))
        .env("PHANTOM_DOWNLOAD_DIR", dir.path().join("downloads"))
        .output()
        .unwrap();

    assert!(out.status.success(), "the headless run failed: {}", String::from_utf8_lossy(&out.stderr));
    let tree = std::fs::read_to_string(&tree).unwrap();
    assert!(tree.contains("Couldn't add the torrent"), "the refusal isn't shown");
}
