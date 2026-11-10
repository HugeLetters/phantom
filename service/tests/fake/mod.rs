//! A stand-in for a service that's on its way out, for the tests that connect while one stops: it
//! holds the data dir's lock as a service does, and answers on the socket as `answer` says.

// Each test binary uses one of the answers.
#![allow(dead_code)]

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::thread;
use std::time::Duration;

use proto::{Request, Response, socket_path};

/// The data and download dirs in `dir`, for the only test in a binary.
pub fn isolate(dir: &Path) {
    // SAFETY: each binary holds one test, so nothing else reads the environment meanwhile.
    unsafe {
        std::env::set_var("PHANTOM_DATA_DIR", dir.join("data"));
        std::env::set_var("PHANTOM_DOWNLOAD_DIR", dir.join("downloads"));
    }
    std::fs::create_dir_all(dir.join("data")).unwrap();
}

/// What the fake does with a request.
pub enum Answer {
    /// Closes the connection unanswered: a service exiting.
    Close,
    /// Answers `Hello` as a service of another build; `Shutdown` removes the socket at once and
    /// keeps the lock for `stopping`, as a build that takes that long to stop.
    OtherBuild { stopping: Duration },
}

/// Serves `answer` until it's done, then removes the socket and lets the lock go after `stopping`.
pub fn service(answer: Answer, serving: Duration) {
    let lock = File::create(proto::lock_path()).unwrap();
    lock.try_lock().unwrap();
    let listener = UnixListener::bind(socket_path()).unwrap();
    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        let until = std::time::Instant::now() + serving;
        let mut stopping = None;
        while std::time::Instant::now() < until && stopping.is_none() {
            let Ok((stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(10));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            let mut writer = stream.try_clone().unwrap();
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                match (&answer, serde_json::from_str::<Request>(&line).unwrap()) {
                    (Answer::Close, _) => break,
                    (Answer::OtherBuild { .. }, Request::Hello) => {
                        let hello = Response::Hello { build_id: "another-build".into(), service_version: "old".into() };
                        writeln!(writer, "{}", serde_json::to_string(&hello).unwrap()).unwrap();
                    }
                    (Answer::OtherBuild { stopping: d }, Request::Shutdown) => {
                        stopping = Some(*d);
                        break;
                    }
                    (_, other) => panic!("unexpected {other:?}"),
                }
            }
        }
        drop(listener);
        std::fs::remove_file(socket_path()).unwrap();
        thread::sleep(stopping.unwrap_or_default());
        drop(lock);
    });
}
