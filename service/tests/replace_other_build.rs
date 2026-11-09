//! A client that meets a service from another build replaces it with the real one.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;

use client::Client;
use proto::{BUILD_ID, Request, Response, socket_path};

/// Stands in for a service of another build: answers `Hello` with a foreign id, tells `done` when
/// it is asked to shut down, then stops listening.
fn other_build_service(done: mpsc::Sender<()>) {
    let listener = UnixListener::bind(socket_path()).unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut writer = stream.try_clone().unwrap();
        for line in BufReader::new(stream).lines() {
            match serde_json::from_str::<Request>(&line.unwrap()).unwrap() {
                Request::Hello => {
                    let hello = Response::Hello { build_id: format!("not-{BUILD_ID}"), service_version: "old".into() };
                    writeln!(writer, "{}", serde_json::to_string(&hello).unwrap()).unwrap();
                }
                Request::Shutdown => break,
                other => panic!("unexpected {other:?}"),
            }
        }
        drop(listener);
        std::fs::remove_file(socket_path()).unwrap();
        done.send(()).unwrap();
    });
}

#[test]
fn client_replaces_service_from_another_build() {
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: the only test in this binary, so nothing else reads the environment meanwhile.
    unsafe {
        std::env::set_var("PHANTOM_DATA_DIR", dir.path().join("data"));
        std::env::set_var("PHANTOM_DOWNLOAD_DIR", dir.path().join("downloads"));
    }
    std::fs::create_dir_all(dir.path().join("data")).unwrap();

    let (done, shut_down) = mpsc::channel();
    other_build_service(done);

    let mut client = Client::connect_or_spawn(env!("CARGO_BIN_EXE_phantom-service").as_ref()).unwrap();
    shut_down.try_recv().expect("the other build's service was told to shut down");
    assert_eq!(client.service_version, env!("CARGO_PKG_VERSION"));
    client.shutdown().unwrap();
}
