//! A torrent list the service couldn't read (#308): it's kept aside, and every client connected
//! during that run is told where, through a real service and its socket; the next start, with
//! nothing damaged, tells nobody.

mod fake;

use client::Client;

#[test]
fn a_database_set_aside_is_reported_with_its_path_to_every_client_and_to_none_at_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    fake::isolate(dir.path());
    std::fs::write(dir.path().join("data").join("records.db"), b"not a database").unwrap();
    let service = env!("CARGO_BIN_EXE_phantom-service");
    let mut client = Client::connect_or_spawn(service.as_ref()).unwrap();

    let aside = dir.path().join("data").join("records.invalid.db");
    for _ in 0..2 {
        let first = Client::open().unwrap().subscribe(Vec::new()).unwrap().receive().unwrap();
        assert_eq!(first.records_set_aside.as_deref(), Some(aside.as_path()), "told to each client");
    }
    assert_eq!(client.settings().unwrap().records_set_aside.as_deref(), Some(aside.as_path()), "and with the settings");
    assert_eq!(std::fs::read(&aside).unwrap(), b"not a database");

    client.shutdown().unwrap();
    client.wait_closed().unwrap();
    let mut client = Client::connect_or_spawn(service.as_ref()).unwrap();
    let first = Client::open().unwrap().subscribe(Vec::new()).unwrap().receive().unwrap();
    assert_eq!(first.records_set_aside, None, "not at a start after");
    client.shutdown().unwrap();
}
