//! A client that meets a service as it stops waits for it to go, then starts a new one (#113).
//! An example rather than generated: it's the real service starting once the old one has gone,
//! which `client::machine`'s generated tests drive against a fake.

mod fake;

use std::time::Duration;

use client::Client;

#[test]
fn a_client_connecting_while_the_service_stops_starts_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    fake::isolate(dir.path());
    fake::service(fake::Answer::Close, Duration::from_millis(1500));

    let mut client = Client::connect_or_spawn(env!("CARGO_BIN_EXE_phantom-service").as_ref()).unwrap();
    assert_eq!(client.service_version, env!("CARGO_PKG_VERSION"));
    client.shutdown().unwrap();
}
