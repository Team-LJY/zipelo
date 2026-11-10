use app_core::{Event, Operation, OperationId, Policy, ProcessCleanup, Terminal};
use app_core::{gzip, ustar, zip_create};
use std::convert::Infallible;
use std::time::Duration;

pub type GzipTerminal = Terminal<gzip::Completed, gzip::Error<Infallible>>;
pub type UstarTerminal = Terminal<ustar::Completed, ustar::Error<Infallible>>;
pub type ZipTerminal = Terminal<Vec<u8>, zip_create::Failure>;

#[derive(Debug, PartialEq, Eq)]
pub struct Marker(pub Box<[u8]>);

pub fn fixture(name: &str) -> Vec<u8> {
    let line = include_str!("fixtures.hex")
        .lines()
        .find(|line| line.split_once(' ').unwrap().0 == name)
        .expect("고정 fixture가 있어야 함");
    let hex = line.split_once(' ').unwrap().1;
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

pub fn gzip_request(name: &str, selected: bool) -> gzip::Request {
    gzip::Request {
        input: fixture(name).into_boxed_slice(),
        selected,
    }
}

pub fn gzip_trace(selected: bool) -> (Vec<Event<gzip::Checkpoint>>, GzipTerminal) {
    let (operation, _) = Operation::start().unwrap();
    let mut budget = gzip::Budget::new(gzip::Limits::default()).unwrap();
    let mut events = Vec::new();
    let terminal = operation.gzip(
        gzip_request("gzip_digits", selected),
        &mut budget,
        |event| {
            events.push(event);
            Ok::<_, gzip::Stop<Infallible>>(())
        },
    );
    (events, terminal)
}

pub fn ustar_request() -> ustar::Request {
    ustar::Request {
        input: fixture("ustar_one").into_boxed_slice(),
        policy: ustar::NamePolicy::Linux,
        limits: ustar::Limits::default(),
    }
}

pub fn ustar_trace(selected: bool) -> (Vec<Event<ustar::Checkpoint>>, UstarTerminal) {
    let (operation, _) = Operation::start().unwrap();
    let mut events = Vec::new();
    let terminal = operation.ustar(
        ustar_request(),
        |entries| {
            Ok::<_, Infallible>(if selected {
                vec![entries[0].id.clone()]
            } else {
                Vec::new()
            })
        },
        |event| {
            events.push(event);
            Ok(())
        },
    );
    (events, terminal)
}

pub fn zip_request() -> zip_create::Request {
    zip_create::Request {
        entries: vec![zip_create::Entry {
            name: "digits.txt".to_owned(),
            bytes: fixture("digits").into_boxed_slice(),
        }],
        options: zip_create::Options {
            method: zip_create::Method::Stored,
            ..Default::default()
        },
        limits: zip_create::Limits::default(),
        timeout: Duration::from_secs(300),
    }
}

pub fn zip_trace() -> (Vec<Event<zip_create::Checkpoint>>, ZipTerminal) {
    let (operation, _) = Operation::start().unwrap();
    let mut events = Vec::new();
    let terminal = operation.create_zip(zip_request(), |event| {
        events.push(event);
        Ok(())
    });
    (events, terminal)
}

pub fn assert_stream<C, T, E>(events: &[Event<C>], terminal: &Terminal<T, E>, policy: Policy) {
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.stamp().operation(), terminal.stamp().operation());
        assert_eq!(event.stamp().sequence(), index as u64);
        assert_eq!(event.policy, policy);
    }
    assert_eq!(terminal.stamp().sequence(), events.len() as u64);
    assert_eq!(terminal.policy(), policy);
    assert_eq!(terminal.process_cleanup(), ProcessCleanup::NotApplicable);
}

pub fn assert_gzip_failure<E>(
    terminal: &Terminal<gzip::Completed, gzip::Error<E>>,
    expected: gzip::Failure,
) {
    let failure = match terminal.result() {
        Err(failure) => failure,
        Ok(_) => panic!("실패에서 완료 payload를 반환하면 안 됨"),
    };
    assert_eq!(failure.adapter, None);
    assert!(matches!(&failure.native, Some(gzip::Error::Reader(actual)) if *actual == expected));
}

pub fn assert_ustar_failure<E>(
    terminal: &Terminal<ustar::Completed, ustar::Error<E>>,
    expected: ustar::Failure,
) {
    let failure = match terminal.result() {
        Err(failure) => failure,
        Ok(_) => panic!("실패에서 완료 payload를 반환하면 안 됨"),
    };
    assert_eq!(failure.adapter, None);
    assert!(matches!(&failure.native, Some(ustar::Error::Reader(actual)) if *actual == expected));
}

pub fn assert_zip_failure(terminal: &ZipTerminal, expected: zip_create::Failure) {
    let failure = terminal.result().unwrap_err();
    assert_eq!(failure.adapter, None);
    assert_eq!(failure.native, Some(expected));
}

pub fn accepted_payload(
    receiver: &mut app_core::Receiver,
    terminal: GzipTerminal,
) -> Option<Vec<u8>> {
    if receiver.terminal(&terminal) != Ok(app_core::Delivery::Accepted) {
        return None;
    }
    terminal.into_result().ok()?.into_inner().bytes
}

pub fn id_of<T, E>(terminal: &Terminal<T, E>) -> OperationId {
    terminal.stamp().operation()
}
