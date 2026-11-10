use crate::support::Marker;
use app_core::{
    Cancellation, Event, MemoryValidated, Operation, Terminal, gzip, ustar, zip_create,
};

fn assert_send<T: Send>() {}

#[test]
fn chosen_send_error_requests_operations_events_and_results_are_send() {
    assert_send::<Marker>();
    assert_send::<Operation>();
    assert_send::<Cancellation>();
    assert_send::<gzip::Request>();
    assert_send::<gzip::Budget>();
    assert_send::<ustar::Request>();
    assert_send::<zip_create::Request>();
    assert_send::<Event<gzip::Checkpoint>>();
    assert_send::<Event<ustar::Checkpoint>>();
    assert_send::<Event<zip_create::Checkpoint>>();
    assert_send::<Terminal<gzip::Completed, gzip::Error<Marker>>>();
    assert_send::<Terminal<ustar::Completed, ustar::Error<Marker>>>();
    assert_send::<Terminal<Vec<u8>, zip_create::Failure>>();
    assert_send::<MemoryValidated<gzip::Completed>>();
    assert_send::<MemoryValidated<ustar::Completed>>();
    assert_send::<MemoryValidated<Vec<u8>>>();
}
