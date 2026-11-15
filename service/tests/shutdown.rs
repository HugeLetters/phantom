//! A service asked to stop answers nothing more once it has closed the asking connection: a client
//! that saw it close and connects again meets a service that refuses, or none, never the old one
//! answering (`phantom undo` right after a stop was refused as stopping, by the service it had just
//! stopped). Generated over the requests sent after it, the other clients connected meanwhile, and
//! whether the asking client did something first; each case is a real service, in a temp dir.

mod fake;

use std::time::{Duration, Instant};

use client::{Client, Failure};
use proptest::prelude::*;
use proto::Refusal;

#[derive(Debug, Clone, Copy)]
enum Ask {
    Hello,
    Undo,
    Settings,
    List,
}

/// What `ask` is answered on a connection opened now: `Some(what)` if the service did it.
fn answered(ask: Ask) -> Option<String> {
    let Ok(mut client) = Client::open() else { return None };
    let answer = match ask {
        Ask::Hello => return hello(),
        Ask::Undo => client.undo().map(|u| format!("{u:?}")),
        Ask::Settings => client.settings().map(|s| format!("{s:?}")),
        Ask::List => client.list().map(|l| format!("{l:?}")),
    };
    match answer {
        Ok(what) => Some(what),
        // Refused as anything but stopping is answered too (nothing to undo).
        Err(Failure::Refused(refusal)) if refusal != Refusal::Stopping => Some(format!("{refusal:?}")),
        Err(_) => None,
    }
}

/// A hello, as a client's first request, on a connection of its own: the answer, unless refused
/// or closed.
fn hello() -> Option<String> {
    use std::io::{BufRead, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(proto::socket_path()).ok()?;
    let mut line = serde_json::to_vec(&proto::Request::Hello).unwrap();
    line.push(b'\n');
    stream.write_all(&line).ok()?;
    let mut answer = String::new();
    std::io::BufReader::new(stream).read_line(&mut answer).ok()?;
    match serde_json::from_str::<proto::Response>(&answer).ok()? {
        proto::Response::Refused(Refusal::Stopping) => None,
        other => Some(format!("{other:?}")),
    }
}

proptest! {
    // A real service per case, about 2 s each to stop: few of them (each went red before the fix).
    #![proptest_config(ProptestConfig { cases: 6, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn once_a_shutdown_has_closed_its_connection_the_service_answers_nothing(
        asks in proptest::collection::vec(prop_oneof![Just(Ask::Hello), Just(Ask::Undo), Just(Ask::Settings), Just(Ask::List)], 1..4),
        others in 0..3usize,
        busy in any::<bool>(),
    ) {
        let dir = tempfile::tempdir().unwrap();
        fake::isolate(dir.path());
        let mut client = Client::connect_or_spawn(env!("CARGO_BIN_EXE_phantom-service").as_ref()).unwrap();
        let _others: Vec<Client> = (0..others).map(|_| Client::open().unwrap()).collect();
        if busy {
            client.list().unwrap();
        }
        client.shutdown().unwrap();
        client.wait_closed().unwrap();
        for ask in asks {
            prop_assert_eq!(answered(ask), None, "{:?} after the shutdown closed", ask);
        }
        // It goes before its temp dir does.
        let deadline = Instant::now() + Duration::from_secs(10);
        while proto::socket_path().exists() {
            prop_assert!(Instant::now() < deadline, "the service never stopped");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
