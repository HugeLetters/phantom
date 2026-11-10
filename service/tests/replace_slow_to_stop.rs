//! Replacing a service of another build that takes longer to stop than a new service waits for
//! the lock (#113): the client waits for the old one to let the data dir go before starting the
//! new one. An example rather than generated, as `connect_while_stopping.rs` is.

mod fake;

use std::time::Duration;

use client::Client;

#[test]
fn replacing_a_build_that_is_slow_to_stop_waits_for_it() {
    let dir = tempfile::tempdir().unwrap();
    fake::isolate(dir.path());
    fake::service(fake::Answer::OtherBuild { stopping: Duration::from_secs(7) }, Duration::from_secs(60));

    let mut client = Client::connect_or_spawn(env!("CARGO_BIN_EXE_phantom-service").as_ref()).unwrap();
    assert_eq!(client.service_version, env!("CARGO_PKG_VERSION"));
    client.shutdown().unwrap();
}
