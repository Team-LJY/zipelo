use zipelo_rust_host_errors::*;

const JOB: [u8; 16] = [1; 16];
const ZERO: [u8; 16] = [0; 16];

fn item(number: u32) -> [u8; 16] {
    let mut id = ZERO;
    id[..4].copy_from_slice(&number.to_le_bytes());
    id
}

fn frame(kind: u16, sequence: u64, id: [u8; 16], payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 64];
    bytes[..4].copy_from_slice(b"ZIP2");
    bytes[4..6].copy_from_slice(&2u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&kind.to_le_bytes());
    bytes[8..12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes[16..32].copy_from_slice(&JOB);
    bytes[32..48].copy_from_slice(&id);
    bytes[48..56].copy_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn complete() -> Vec<u8> {
    [
        frame(1, 0, ZERO, &[]),
        frame(2, 1, item(1), &3u64.to_le_bytes()),
        frame(3, 2, item(1), b"abc"),
        frame(4, 3, item(1), &3u64.to_le_bytes()),
        frame(5, 4, ZERO, &[1]),
    ]
    .concat()
}

fn stream() -> Stream {
    Stream::new(JOB, Limits::default()).unwrap()
}

fn binding() -> Binding {
    Binding {
        executable: "/supervisor/pinned/worker".into(),
        expected_sha256: [2; 32],
        observed_sha256: [2; 32],
        pid: 123,
        source_head: WORKER_SOURCE.into(),
        invocation: "--extract-stream-v2".into(),
    }
}

struct Harness {
    session: Session,
    sequence: u64,
}

impl Harness {
    fn new() -> Self {
        Self {
            session: Session::new(JOB, Limits::default(), Some(binding())).unwrap(),
            sequence: 0,
        }
    }

    fn event(&mut self, event: Event<'_>) {
        self.session.observe(JOB, self.sequence, event);
        self.sequence += 1;
    }

    fn settle(&mut self) {
        self.event(Event::IoSettled);
        self.event(Event::CleanupComplete);
    }

    fn natural(&mut self, bytes: &[u8], code: i32) -> Report {
        self.event(Event::Stdout(bytes));
        self.event(Event::StdoutEof);
        self.event(Event::NaturalExit(code));
        self.settle();
        self.session.report()
    }
}

#[test]
fn accepts_fragmentation_at_every_split() {
    let bytes = complete();
    for split in 0..=bytes.len() {
        let mut parser = stream();
        parser.feed(&bytes[..split]).unwrap();
        parser.feed(&bytes[split..]).unwrap();
        let report = parser.finish_input();
        assert!(report.completed);
        assert_eq!(report.frames, 5);
        assert_eq!(report.output_bytes, 3);
        assert_eq!(report.items, 1);
        assert_eq!(report.end, StreamEnd::Boundary);
    }
}

#[test]
fn accepts_one_byte_chunks() {
    let mut parser = stream();
    for byte in complete() {
        parser.feed(&[byte]).unwrap();
    }
    assert!(parser.finish_input().completed);
}

#[test]
fn empty_job_completion_is_protocol_only() {
    let bytes = [frame(1, 0, ZERO, &[]), frame(5, 1, ZERO, &[1])].concat();
    assert_eq!(
        Harness::new().natural(&bytes, 0).outcome,
        Outcome::ProtocolCompletionObserved
    );
}

#[test]
fn reports_only_protocol_completion_with_attested_lifecycle() {
    let report = Harness::new().natural(&complete(), 0);
    assert_eq!(report.outcome, Outcome::ProtocolCompletionObserved);
    assert!(!report.stop_required);
    assert!(report.binding_attested && report.stdout_eof_attested);
}

#[test]
fn rejects_magic_version_reserved_and_unknown_kind() {
    for offset in [0, 4, 6, 12, 56] {
        let mut parser = stream();
        let mut bytes = frame(1, 0, ZERO, &[]);
        bytes[offset] = 99;
        assert_eq!(parser.feed(&bytes), Err(Fault::Header));
    }
}

#[test]
fn rejects_wrong_job_and_sequence() {
    let mut bytes = frame(1, 0, ZERO, &[]);
    bytes[16] = 2;
    assert_eq!(stream().feed(&bytes), Err(Fault::Job));
    assert_eq!(stream().feed(&frame(1, 1, ZERO, &[])), Err(Fault::Sequence));
}

#[test]
fn rejects_skipped_or_reused_item_id() {
    for number in [0, 2] {
        let mut parser = stream();
        parser.feed(&frame(1, 0, ZERO, &[])).unwrap();
        assert_eq!(
            parser.feed(&frame(2, 1, item(number), &0u64.to_le_bytes())),
            Err(Fault::Item)
        );
    }
}

#[test]
fn rejects_data_wrong_active_item() {
    let mut parser = stream();
    parser.feed(&complete()[..136]).unwrap();
    assert_eq!(parser.feed(&frame(3, 2, item(2), b"a")), Err(Fault::Item));
}

#[test]
fn rejects_data_without_begin_and_nested_item() {
    assert_eq!(
        stream().feed(&frame(3, 0, item(1), b"a")),
        Err(Fault::State)
    );
    let mut parser = stream();
    parser.feed(&complete()[..136]).unwrap();
    assert_eq!(
        parser.feed(&frame(2, 2, item(2), &0u64.to_le_bytes())),
        Err(Fault::State)
    );
}

#[test]
fn rejects_duplicate_begin_job() {
    let mut parser = stream();
    parser.feed(&frame(1, 0, ZERO, &[])).unwrap();
    assert_eq!(parser.feed(&frame(1, 1, ZERO, &[])), Err(Fault::State));
}

#[test]
fn rejects_huge_payload_before_body() {
    let mut bytes = frame(1, 0, ZERO, &[]);
    bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(stream().feed(&bytes), Err(Fault::Length));
}

#[test]
fn rejects_wrong_fixed_payload_lengths_and_empty_data() {
    for (kind, size) in [(1, 1), (2, 9), (3, 0), (4, 7), (5, 2)] {
        assert_eq!(
            stream().feed(&frame(kind, 0, ZERO, &vec![0; size])),
            Err(Fault::Length)
        );
    }
}

#[test]
fn enforces_wire_budget_before_payload() {
    let limits = Limits {
        wire_bytes: 64,
        ..Limits::default()
    };
    let mut parser = Stream::new(JOB, limits).unwrap();
    assert_eq!(
        parser.feed(&frame(2, 0, item(1), &0u64.to_le_bytes())[..64]),
        Err(Fault::Budget)
    );
}

#[test]
fn enforces_header_budget_during_fragmentation() {
    let mut parser = Stream::new(
        JOB,
        Limits {
            wire_bytes: 4,
            ..Limits::default()
        },
    )
    .unwrap();
    parser.feed(b"ZIP2").unwrap();
    assert_eq!(parser.feed(&[2]), Err(Fault::Budget));
}

#[test]
fn enforces_frame_and_item_budgets() {
    let mut parser = Stream::new(
        JOB,
        Limits {
            frames: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    parser.feed(&frame(1, 0, ZERO, &[])).unwrap();
    assert_eq!(parser.feed(&frame(5, 1, ZERO, &[1])), Err(Fault::Budget));
    let mut parser = Stream::new(
        JOB,
        Limits {
            items: 0,
            ..Limits::default()
        },
    )
    .unwrap();
    parser.feed(&frame(1, 0, ZERO, &[])).unwrap();
    assert_eq!(
        parser.feed(&frame(2, 1, item(1), &0u64.to_le_bytes())),
        Err(Fault::Budget)
    );
}

#[test]
fn output_exact_boundary_and_one_byte_under() {
    let mut parser = Stream::new(
        JOB,
        Limits {
            output_bytes: 3,
            ..Limits::default()
        },
    )
    .unwrap();
    parser.feed(&complete()).unwrap();
    let mut parser = Stream::new(
        JOB,
        Limits {
            output_bytes: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(parser.feed(&complete()), Err(Fault::Budget));
}

#[test]
fn rejects_data_over_declared_length_before_body() {
    let mut parser = stream();
    parser.feed(&complete()[..136]).unwrap();
    assert_eq!(
        parser.feed(&frame(3, 2, item(1), b"abcd")[..64]),
        Err(Fault::Length)
    );
}

#[test]
fn rejects_wrong_end_item_actual_or_short_data() {
    for actual in [0u64, 3] {
        let mut parser = stream();
        parser.feed(&complete()[..136]).unwrap();
        assert_eq!(
            parser.feed(&frame(4, 2, item(1), &actual.to_le_bytes())),
            Err(Fault::Length)
        );
    }
    let mut parser = stream();
    parser.feed(&complete()[..203]).unwrap();
    assert_eq!(
        parser.feed(&frame(4, 3, item(1), &2u64.to_le_bytes())),
        Err(Fault::Length)
    );
}

#[test]
fn rejects_terminal_with_active_item_or_non_completed_status() {
    let mut parser = stream();
    parser.feed(&complete()[..136]).unwrap();
    assert_eq!(parser.feed(&frame(5, 2, ZERO, &[1])), Err(Fault::State));
    for status in [0, 2, 3, 255] {
        let mut parser = stream();
        parser.feed(&frame(1, 0, ZERO, &[])).unwrap();
        assert_eq!(
            parser.feed(&frame(5, 1, ZERO, &[status])),
            Err(Fault::State)
        );
    }
}

#[test]
fn rejects_trailing_bytes_in_same_and_later_chunks() {
    let mut bytes = complete();
    bytes.push(0);
    assert_eq!(stream().feed(&bytes), Err(Fault::TrailingData));
    let mut parser = stream();
    parser.feed(&complete()).unwrap();
    assert_eq!(parser.feed(&[0]), Err(Fault::TrailingData));
}

#[test]
fn distinguishes_boundary_header_and_body_eof() {
    for (size, end, fault) in [
        (0, StreamEnd::Boundary, None),
        (1, StreamEnd::PartialHeader, Some(Fault::PartialHeader)),
        (64, StreamEnd::Boundary, None),
        (128, StreamEnd::PartialPayload, Some(Fault::PartialPayload)),
        (130, StreamEnd::PartialPayload, Some(Fault::PartialPayload)),
    ] {
        let mut parser = stream();
        parser.feed(&complete()[..size]).unwrap();
        let summary = parser.finish_input();
        assert_eq!(summary.end, end);
        assert_eq!(summary.fault, fault);
    }
}

#[test]
fn failed_parser_never_recovers_to_completed() {
    let mut parser = stream();
    assert_eq!(parser.feed(&frame(1, 1, ZERO, &[])), Err(Fault::Sequence));
    assert_eq!(parser.feed(&complete()), Err(Fault::Sequence));
    assert!(!parser.summary().completed);
}

#[test]
fn worker_exit_mapping_remains_worker_reported() {
    for (code, reported) in [
        (10, WorkerReport::Unsupported),
        (11, WorkerReport::UnsupportedEntry),
        (12, WorkerReport::Integrity),
        (13, WorkerReport::Budget),
        (14, WorkerReport::Protocol),
        (15, WorkerReport::ObservedUnexpectedEof),
        (16, WorkerReport::UnknownEngineCause),
        (17, WorkerReport::DuplicateName),
        (18, WorkerReport::MetadataMismatch),
    ] {
        let report = Harness::new().natural(&frame(1, 0, ZERO, &[]), code);
        assert_eq!(
            report.outcome,
            Outcome::Failed(Cause::WorkerReported(reported))
        );
        assert!(!report.stop_required);
    }
}

#[test]
fn unknown_exit_and_missing_completion_are_failures() {
    assert_eq!(
        Harness::new().natural(&[], 99).outcome,
        Outcome::Failed(Cause::UnknownExit(99))
    );
    assert_eq!(
        Harness::new().natural(&[], 0).outcome,
        Outcome::Failed(Cause::MissingCompletion)
    );
}

#[test]
fn completed_nonzero_exit_is_contradiction() {
    assert_eq!(
        Harness::new().natural(&complete(), 12).outcome,
        Outcome::Failed(Cause::CompletionExitContradiction)
    );
}

#[test]
fn partial_frame_prevents_worker_integrity_mapping() {
    assert_eq!(
        Harness::new().natural(&complete()[..130], 12).outcome,
        Outcome::Failed(Cause::RawFrame(Fault::PartialPayload))
    );
}

#[test]
fn malformed_frame_wins_over_worker_exit() {
    assert_eq!(
        Harness::new().natural(&frame(1, 9, ZERO, &[]), 12).outcome,
        Outcome::Failed(Cause::RawFrame(Fault::Sequence))
    );
}

#[test]
fn first_host_cause_survives_cleanup_io_and_exit() {
    for cause in [
        HostCause::Cancelled,
        HostCause::Deadline,
        HostCause::Budget,
        HostCause::Io,
    ] {
        let mut h = Harness::new();
        h.event(Event::HostFailure(cause));
        h.event(Event::HostFailure(HostCause::Io));
        h.event(Event::StdoutEof);
        h.event(Event::NaturalExit(12));
        h.settle();
        assert_eq!(
            h.session.report().outcome,
            Outcome::Failed(Cause::SupervisorHost(cause))
        );
        assert_eq!(h.session.report().exit, Exit::AfterHostStop(12));
    }
}

#[test]
fn cancellation_and_deadline_follow_observed_order() {
    for (first, second) in [
        (HostCause::Cancelled, HostCause::Deadline),
        (HostCause::Deadline, HostCause::Cancelled),
    ] {
        let mut h = Harness::new();
        h.event(Event::HostFailure(first));
        h.event(Event::HostFailure(second));
        assert_eq!(
            h.session.report().outcome,
            Outcome::Failed(Cause::SupervisorHost(first))
        );
    }
}

#[test]
fn host_failure_cannot_become_completion() {
    let mut h = Harness::new();
    h.event(Event::HostFailure(HostCause::Cancelled));
    let report = h.natural(&complete(), 0);
    assert_eq!(
        report.outcome,
        Outcome::Failed(Cause::SupervisorHost(HostCause::Cancelled))
    );
}

#[test]
fn cleanup_failure_blocks_without_erasing_worker_cause() {
    let mut h = Harness::new();
    h.natural(&[], 12);
    h.event(Event::CleanupFailed);
    let report = h.session.report();
    assert!(report.stop_required && report.cleanup_failed);
    assert_eq!(
        report.outcome,
        Outcome::Failed(Cause::WorkerReported(WorkerReport::Integrity))
    );
}

#[test]
fn cleanup_failure_is_sticky_after_cleanup_complete() {
    let mut h = Harness::new();
    h.event(Event::CleanupFailed);
    let report = h.natural(&complete(), 0);
    assert!(report.stop_required);
    assert_ne!(report.outcome, Outcome::ProtocolCompletionObserved);
}

#[test]
fn eof_without_exit_remains_blocked() {
    let mut h = Harness::new();
    h.event(Event::Stdout(&complete()));
    h.event(Event::StdoutEof);
    h.event(Event::IoSettled);
    assert!(h.session.report().stop_required);
    assert_eq!(h.session.report().outcome, Outcome::PendingOrUnconfirmed);
}

#[test]
fn exit_without_eof_remains_blocked() {
    let mut h = Harness::new();
    h.event(Event::Stdout(&complete()));
    h.event(Event::NaturalExit(0));
    h.settle();
    assert!(h.session.report().stop_required);
    assert_ne!(
        h.session.report().outcome,
        Outcome::ProtocolCompletionObserved
    );
}

#[test]
fn exit_before_eof_can_resolve_after_pipe_drains() {
    let mut h = Harness::new();
    h.event(Event::NaturalExit(12));
    h.event(Event::Stdout(&frame(1, 0, ZERO, &[])));
    h.event(Event::StdoutEof);
    h.settle();
    assert_eq!(
        h.session.report().outcome,
        Outcome::Failed(Cause::WorkerReported(WorkerReport::Integrity))
    );
}

#[test]
fn missing_io_or_cleanup_confirmation_blocks() {
    let mut h = Harness::new();
    h.event(Event::Stdout(&complete()));
    h.event(Event::StdoutEof);
    h.event(Event::NaturalExit(0));
    assert!(h.session.report().stop_required);
    h.event(Event::IoSettled);
    assert!(h.session.report().stop_required);
    h.event(Event::CleanupComplete);
    assert!(!h.session.report().stop_required);
}

#[test]
fn forced_exit_is_not_worker_reported() {
    let mut h = Harness::new();
    h.event(Event::StdoutEof);
    h.event(Event::ForcedOrSignalledExit);
    h.settle();
    assert_eq!(
        h.session.report().outcome,
        Outcome::Failed(Cause::ForcedOrSignalledExit)
    );
}

#[test]
fn worker_cause_survives_later_host_cleanup_error() {
    let mut h = Harness::new();
    h.event(Event::StdoutEof);
    h.event(Event::NaturalExit(15));
    h.event(Event::HostFailure(HostCause::Io));
    assert_eq!(
        h.session.report().outcome,
        Outcome::Failed(Cause::WorkerReported(WorkerReport::ObservedUnexpectedEof))
    );
}

#[test]
fn rejects_supervisor_job_sequence_and_duplicates() {
    for wrong_job in [false, true] {
        let mut h = Harness::new();
        h.session.observe(
            if wrong_job { ZERO } else { JOB },
            if wrong_job { 0 } else { 1 },
            Event::StdoutEof,
        );
        let report = h.session.report();
        assert!(report.stop_required && report.supervisor_invalid);
    }
    let mut h = Harness::new();
    h.event(Event::StdoutEof);
    h.event(Event::StdoutEof);
    assert!(h.session.report().supervisor_invalid);
}

#[test]
fn rejects_data_after_io_settled_and_duplicate_exit() {
    let mut h = Harness::new();
    h.event(Event::IoSettled);
    h.event(Event::Stdout(&[0]));
    assert!(h.session.report().supervisor_invalid);
    let mut h = Harness::new();
    h.event(Event::NaturalExit(0));
    h.event(Event::NaturalExit(0));
    assert!(h.session.report().supervisor_invalid);
}

#[test]
fn rejects_early_cleanup_confirmation() {
    let mut h = Harness::new();
    h.event(Event::CleanupComplete);
    assert!(h.session.report().supervisor_invalid);
}

#[test]
fn binding_is_required_and_hash_mismatch_is_rejected() {
    let mut h = Harness {
        session: Session::new(JOB, Limits::default(), None).unwrap(),
        sequence: 0,
    };
    let report = h.natural(&complete(), 0);
    assert_eq!(report.outcome, Outcome::PendingOrUnconfirmed);
    assert!(report.stop_required);
    let mut bad = binding();
    bad.observed_sha256 = [3; 32];
    assert!(Session::new(JOB, Limits::default(), Some(bad)).is_err());
}

#[test]
fn rejects_wrong_profile_source_invocation_and_pid() {
    for field in 0..3 {
        let mut bad = binding();
        match field {
            0 => bad.source_head = "other".into(),
            1 => bad.invocation = "--list".into(),
            _ => bad.pid = 0,
        }
        assert!(Session::new(JOB, Limits::default(), Some(bad)).is_err());
    }
}

#[test]
fn rejects_invalid_limits_and_zero_job() {
    assert!(Stream::new(ZERO, Limits::default()).is_err());
    assert!(
        Stream::new(
            JOB,
            Limits {
                frames: u64::MAX,
                ..Limits::default()
            }
        )
        .is_err()
    );
}

#[test]
fn raw_capture_end_is_not_process_evidence() {
    let mut parser = stream();
    parser.feed(&complete()).unwrap();
    assert!(parser.finish_input().completed);
    let mut h = Harness::new();
    h.event(Event::Stdout(&complete()));
    assert_eq!(h.session.report().outcome, Outcome::PendingOrUnconfirmed);
    assert!(h.session.report().stop_required);
}

#[test]
fn rejects_data_after_attested_eof() {
    let mut h = Harness::new();
    h.event(Event::StdoutEof);
    h.event(Event::NaturalExit(12));
    h.event(Event::Stdout(&[0]));
    let report = h.session.report();
    assert!(report.stop_required && report.supervisor_invalid);
    assert_eq!(
        report.outcome,
        Outcome::Failed(Cause::WorkerReported(WorkerReport::Integrity))
    );
}

#[test]
fn accepts_maximum_data_payload_without_retaining_body() {
    let bytes = [
        frame(1, 0, ZERO, &[]),
        frame(2, 1, item(1), &65_536u64.to_le_bytes()),
        frame(3, 2, item(1), &vec![7; 65_536]),
        frame(4, 3, item(1), &65_536u64.to_le_bytes()),
        frame(5, 4, ZERO, &[1]),
    ]
    .concat();
    let mut parser = stream();
    for chunk in bytes.chunks(8192) {
        parser.feed(chunk).unwrap();
    }
    assert_eq!(parser.finish_input().output_bytes, 65_536);
}

#[test]
fn cumulative_output_budget_applies_across_items() {
    let mut parser = Stream::new(
        JOB,
        Limits {
            output_bytes: 3,
            ..Limits::default()
        },
    )
    .unwrap();
    parser.feed(&complete()[..275]).unwrap();
    assert_eq!(
        parser.feed(&frame(2, 4, item(2), &1u64.to_le_bytes())),
        Err(Fault::Budget)
    );
}

#[test]
fn rejects_huge_declared_length_without_allocation() {
    let mut parser = stream();
    parser.feed(&frame(1, 0, ZERO, &[])).unwrap();
    assert_eq!(
        parser.feed(&frame(2, 1, item(1), &u64::MAX.to_le_bytes())),
        Err(Fault::Budget)
    );
}

#[test]
fn rejects_oversized_supervisor_chunk() {
    let mut h = Harness::new();
    h.event(Event::Stdout(&vec![0; 65_537]));
    assert!(h.session.report().stop_required);
    assert_eq!(
        h.session.report().outcome,
        Outcome::Failed(Cause::RawFrame(Fault::Budget))
    );
}

#[test]
fn first_raw_failure_survives_cancellation() {
    let mut h = Harness::new();
    h.event(Event::Stdout(&frame(1, 8, ZERO, &[])));
    h.event(Event::HostFailure(HostCause::Cancelled));
    assert_eq!(
        h.session.report().outcome,
        Outcome::Failed(Cause::RawFrame(Fault::Sequence))
    );
}
