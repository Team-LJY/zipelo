use std::time::Duration;
use zipelo_rust_live_supervisor_model::{
    LIVE_SUPERVISOR_IMPLEMENTED,
    launch::{self, ImageDecision},
    range::{Config, MAX_REQUESTS, MAX_SERVED, RangeFault, RangeLimits, RangeMachine},
    reactor::{Deadlines, Phase, Reactor, Reason, WaitObservation},
};

const JOB: [u8; 16] = [1; 16];

fn config() -> Config {
    Config {
        job: JOB,
        processed: 128 * 1024 * 1024,
        output: 16 * 1024 * 1024,
        entries: 1024,
        duration: Duration::from_secs(1),
    }
}

fn range_with(limits: RangeLimits) -> RangeMachine {
    let mut range = RangeMachine::new(
        (0u8..128).collect::<Vec<_>>().into_boxed_slice(),
        config(),
        limits,
    )
    .unwrap();
    range.written(64).unwrap();
    range
}

fn request(sequence: u64, offset: u64, count: u32) -> [u8; 48] {
    let mut bytes = [0; 48];
    bytes[..4].copy_from_slice(b"ZXR2");
    bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
    bytes[8..24].copy_from_slice(&JOB);
    bytes[24..32].copy_from_slice(&sequence.to_le_bytes());
    bytes[32..40].copy_from_slice(&offset.to_le_bytes());
    bytes[40..44].copy_from_slice(&count.to_le_bytes());
    bytes
}

fn flush(range: &mut RangeMachine, step: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    while let Some(next) = range.outbound() {
        let count = next.len().min(step);
        bytes.extend_from_slice(&next[..count]);
        range.written(count).unwrap();
    }
    bytes
}

fn reactor() -> Reactor {
    Reactor::new(
        vec![0; 22].into_boxed_slice(),
        config(),
        RangeLimits::default(),
        Deadlines {
            launch: Duration::from_millis(100),
            operation: Duration::from_secs(1),
            cleanup: Duration::from_millis(200),
        },
    )
    .unwrap()
}

fn running() -> Reactor {
    let mut reactor = reactor();
    reactor.begin_turn(Duration::ZERO, false);
    reactor.exec_status_eof();
    reactor.stdin_written(64);
    reactor
}

fn complete() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (sequence, kind, payload) in [(0u64, 1u16, vec![]), (1, 5, vec![1])] {
        let mut header = [0; 64];
        header[..4].copy_from_slice(b"ZIP2");
        header[4..6].copy_from_slice(&2u16.to_le_bytes());
        header[6..8].copy_from_slice(&kind.to_le_bytes());
        header[8..12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        header[16..32].copy_from_slice(&JOB);
        header[48..56].copy_from_slice(&sequence.to_le_bytes());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&payload);
    }
    bytes
}

#[test]
fn configuration_has_exact_wire_layout() {
    let bytes = config().encode(22).unwrap();
    assert_eq!(&bytes[..4], b"ZXE2");
    assert_eq!(&bytes[4..8], &2u32.to_le_bytes());
    assert_eq!(&bytes[8..24], &JOB);
    assert_eq!(&bytes[24..32], &22u64.to_le_bytes());
    assert_eq!(&bytes[32..40], &(128u64 * 1024 * 1024).to_le_bytes());
    assert_eq!(&bytes[56..64], &10_000_000u64.to_le_bytes());
}

#[test]
fn configuration_rejects_invalid_caps_and_zero_fields() {
    for length in [0, 21, 16 * 1024 * 1024 + 1] {
        assert_eq!(config().encode(length), Err(RangeFault::Config));
    }
    for field in 0..5 {
        let mut bad = config();
        match field {
            0 => bad.job = [0; 16],
            1 => bad.processed = 0,
            2 => bad.output = u64::MAX,
            3 => bad.entries = 1025,
            _ => bad.duration = Duration::from_nanos(99),
        }
        assert_eq!(bad.encode(22), Err(RangeFault::Config));
    }
}

#[test]
fn partial_configuration_write_blocks_requests() {
    let mut range = RangeMachine::new(
        vec![0; 22].into_boxed_slice(),
        config(),
        RangeLimits::default(),
    )
    .unwrap();
    assert_eq!(range.request_capacity(), 0);
    range.written(1).unwrap();
    assert_eq!(range.outbound().unwrap().len(), 63);
    range.written(63).unwrap();
    assert_eq!(range.request_capacity(), 48);
}

#[test]
fn all_request_splits_echo_exact_header_and_snapshot() {
    let header = request(0, 7, 9);
    for split in 0..=48 {
        let mut range = range_with(RangeLimits::default());
        range.feed_request(&header[..split]).unwrap();
        range.feed_request(&header[split..]).unwrap();
        let reply = flush(&mut range, 3);
        assert_eq!(&reply[..48], &header);
        assert_eq!(&reply[48..], &[7, 8, 9, 10, 11, 12, 13, 14, 15]);
        assert_eq!(range.stats().reserved_bytes, 9);
        assert_eq!(range.stats().sent_bytes, 9);
        assert_eq!(range.request_capacity(), 48);
    }
}

#[test]
fn bytewise_requests_and_writes_preserve_single_reply() {
    let mut range = range_with(RangeLimits::default());
    for byte in request(0, 0, 1) {
        range.feed_request(&[byte]).unwrap();
    }
    assert_eq!(range.request_capacity(), 0);
    let reply = flush(&mut range, 1);
    assert_eq!(reply.len(), 49);
    assert_eq!(range.stats().sent_bytes, 1);
}

#[test]
fn queued_second_request_is_rejected() {
    let mut range = range_with(RangeLimits::default());
    range.feed_request(&request(0, 0, 1)).unwrap();
    assert_eq!(
        range.feed_request(&request(1, 0, 1)),
        Err(RangeFault::OutstandingResponse)
    );
}

#[test]
fn coalesced_headers_are_rejected_without_allocation() {
    let mut range = range_with(RangeLimits::default());
    let bytes = [request(0, 0, 1), request(1, 0, 1)].concat();
    assert_eq!(
        range.feed_request(&bytes),
        Err(RangeFault::OutstandingResponse)
    );
}

#[test]
fn rejects_header_magic_version_and_reserved() {
    for offset in [0, 4, 44] {
        let mut range = range_with(RangeLimits::default());
        let mut header = request(0, 0, 1);
        header[offset] = 99;
        assert_eq!(range.feed_request(&header), Err(RangeFault::Header));
    }
}

#[test]
fn rejects_wrong_job_and_sequence() {
    let mut range = range_with(RangeLimits::default());
    let mut header = request(0, 0, 1);
    header[8] = 4;
    assert_eq!(range.feed_request(&header), Err(RangeFault::Job));
    assert_eq!(
        range_with(RangeLimits::default()).feed_request(&request(1, 0, 1)),
        Err(RangeFault::Sequence)
    );
}

#[test]
fn rejects_zero_huge_out_of_range_and_overflow_counts() {
    for (offset, count) in [(0, 0), (0, 65_537), (128, 1), (127, 2), (u64::MAX, 2)] {
        assert_eq!(
            range_with(RangeLimits::default()).feed_request(&request(0, offset, count)),
            Err(RangeFault::Bounds)
        );
    }
}

#[test]
fn request_budget_exact_boundary() {
    let mut range = range_with(RangeLimits {
        requests: 2,
        served: 10,
    });
    for sequence in 0..2 {
        range.feed_request(&request(sequence, 0, 1)).unwrap();
        flush(&mut range, 48);
    }
    assert_eq!(
        range.feed_request(&request(2, 0, 1)),
        Err(RangeFault::Budget)
    );
}

#[test]
fn served_budget_reserved_before_write_and_exact_boundary() {
    let mut range = range_with(RangeLimits {
        requests: 3,
        served: 3,
    });
    range.feed_request(&request(0, 0, 2)).unwrap();
    assert_eq!(range.stats().reserved_bytes, 2);
    assert_eq!(range.stats().sent_bytes, 0);
    flush(&mut range, 48);
    range.feed_request(&request(1, 0, 1)).unwrap();
    flush(&mut range, 48);
    assert_eq!(range.stats().sent_bytes, 3);
    assert_eq!(
        range.feed_request(&request(2, 0, 1)),
        Err(RangeFault::Budget)
    );
}

#[test]
fn hard_request_cap_allows_65536_not_65537() {
    let mut range = range_with(RangeLimits::default());
    for sequence in 0..MAX_REQUESTS {
        range.feed_request(&request(sequence, 0, 1)).unwrap();
        flush(&mut range, 48);
    }
    assert_eq!(
        range.feed_request(&request(MAX_REQUESTS, 0, 1)),
        Err(RangeFault::Budget)
    );
}

#[test]
fn hard_byte_cap_allows_48_mib_exactly() {
    let mut range = RangeMachine::new(
        vec![3; 65_536].into_boxed_slice(),
        config(),
        RangeLimits::default(),
    )
    .unwrap();
    range.written(64).unwrap();
    for sequence in 0..MAX_SERVED / 65_536 {
        range.feed_request(&request(sequence, 0, 65_536)).unwrap();
        while let Some(bytes) = range.outbound() {
            let count = bytes.len();
            range.written(count).unwrap();
        }
    }
    assert_eq!(range.stats().sent_bytes, MAX_SERVED);
    assert_eq!(
        range.feed_request(&request(MAX_SERVED / 65_536, 0, 1)),
        Err(RangeFault::Budget)
    );
}

#[test]
fn partial_and_pending_response_eof_are_distinct() {
    let mut range = range_with(RangeLimits::default());
    range.feed_request(&request(0, 0, 1)[..47]).unwrap();
    assert_eq!(range.request_eof(), Err(RangeFault::PartialHeader));
    let mut range = range_with(RangeLimits::default());
    range.feed_request(&request(0, 0, 1)).unwrap();
    assert_eq!(range.request_eof(), Err(RangeFault::OutstandingResponse));
}

#[test]
fn boundary_eof_prevents_further_requests() {
    let mut range = range_with(RangeLimits::default());
    range.request_eof().unwrap();
    assert!(range.request_eof_seen());
    assert_eq!(
        range.feed_request(&request(0, 0, 1)),
        Err(RangeFault::Closed)
    );
}

#[test]
fn invalid_write_progress_does_not_claim_sent_bytes() {
    for count in [0, 49] {
        let mut range = range_with(RangeLimits::default());
        range.feed_request(&request(0, 0, 1)).unwrap();
        assert_eq!(range.written(count), Err(RangeFault::WriteProgress));
        assert_eq!(range.stats().sent_bytes, 0);
    }
}

#[test]
fn aborted_response_is_not_sent_or_eof() {
    let mut range = range_with(RangeLimits::default());
    range.feed_request(&request(0, 0, 9)).unwrap();
    range.written(48).unwrap();
    range.written(2).unwrap();
    range.abort();
    assert_eq!(range.stats().sent_bytes, 2);
    assert!(!range.request_eof_seen());
    assert!(range.outbound().is_none());
}

#[test]
fn range_failure_stays_latched() {
    let mut range = range_with(RangeLimits::default());
    assert_eq!(
        range.feed_request(&request(9, 0, 1)),
        Err(RangeFault::Sequence)
    );
    assert_eq!(
        range.feed_request(&request(0, 0, 1)),
        Err(RangeFault::Sequence)
    );
}

#[test]
fn starting_stop_flag_is_not_a_kill_command() {
    let mut reactor = reactor();
    assert!(reactor.report().protocol.stop_required);
    assert!(!reactor.begin_turn(Duration::ZERO, false).request_kill);
}

#[test]
fn launch_and_operation_deadlines_are_separate() {
    let mut reactor = reactor();
    let plan = reactor.begin_turn(Duration::from_millis(100), false);
    assert!(plan.request_kill);
    assert_eq!(
        reactor.report().first,
        Some(Reason::Deadline(Phase::Launching))
    );
    let mut reactor = running();
    reactor.begin_turn(Duration::from_millis(999), false);
    assert_eq!(reactor.report().first, None);
    reactor.begin_turn(Duration::from_secs(1), false);
    assert_eq!(
        reactor.report().first,
        Some(Reason::Deadline(Phase::Running))
    );
}

#[test]
fn same_turn_cancel_precedes_deadline_and_stays_first() {
    let mut reactor = running();
    reactor.begin_turn(Duration::from_secs(1), true);
    assert_eq!(reactor.report().first, Some(Reason::Cancelled));
    reactor.io_failure();
    assert_eq!(reactor.report().first, Some(Reason::Cancelled));
}

#[test]
fn earlier_deadline_is_not_overwritten_by_cancel() {
    let mut reactor = running();
    reactor.begin_turn(Duration::from_secs(1), false);
    reactor.begin_turn(Duration::from_millis(1001), true);
    assert_eq!(
        reactor.report().first,
        Some(Reason::Deadline(Phase::Running))
    );
}

#[test]
fn monotonic_clock_regression_is_failure() {
    let mut reactor = running();
    reactor.begin_turn(Duration::from_millis(1), false);
    reactor.begin_turn(Duration::ZERO, false);
    assert_eq!(reactor.report().first, Some(Reason::ReactorOrder));
}

#[test]
fn raw_frame_validation_uses_pr30() {
    let mut reactor = running();
    let mut bytes = complete();
    bytes[16] = 2;
    reactor.stdout_bytes(&bytes);
    assert!(matches!(reactor.report().first, Some(Reason::Raw(_))));
    assert!(reactor.plan().request_kill);
}

#[test]
fn stderr_junk_stops_before_signal_and_response() {
    let mut reactor = running();
    reactor.stderr_bytes(&[b'x'; 48]);
    assert_eq!(
        reactor.report().first,
        Some(Reason::Range(RangeFault::Header))
    );
    assert!(reactor.plan().request_kill);
    assert!(reactor.stdin_bytes().is_empty());
}

#[test]
fn complete_stdout_without_exit_still_hits_deadline() {
    let mut reactor = running();
    reactor.stdout_bytes(&complete());
    reactor.stdout_eof();
    assert!(!reactor.report().owned_child_reaped);
    reactor.begin_turn(Duration::from_secs(1), false);
    assert!(reactor.plan().request_kill);
}

#[test]
fn exit_before_eof_enters_bounded_drain() {
    let mut reactor = running();
    reactor.wait_observation(WaitObservation::Exited(0));
    assert_eq!(reactor.report().phase, Phase::Draining);
    assert!(!reactor.plan().request_kill);
    reactor.begin_turn(Duration::from_millis(200), false);
    assert!(reactor.report().cleanup_failed);
}

#[test]
fn eof_and_exit_both_orders_settle_only_the_model() {
    for exit_first in [false, true] {
        let mut reactor = running();
        reactor.stdout_bytes(&complete());
        if exit_first {
            reactor.wait_observation(WaitObservation::Exited(0));
        }
        reactor.stdout_eof();
        reactor.stderr_eof();
        if !exit_first {
            reactor.wait_observation(WaitObservation::Exited(0));
        }
        reactor.stdin_closed();
        reactor.fds_closed();
        let report = reactor.report();
        assert_eq!(report.phase, Phase::SettledModel);
        assert!(!report.live_verified && !report.publication && report.stop_required);
        assert!(!report.protocol.binding_attested);
    }
}

#[test]
fn kill_acceptance_does_not_prove_reap_or_cleanup() {
    let mut reactor = running();
    reactor.begin_turn(Duration::ZERO, true);
    reactor.kill_result(true);
    assert!(!reactor.report().owned_child_reaped);
    assert!(!reactor.report().protocol.cleanup_confirmed_attested);
    reactor.begin_turn(Duration::from_millis(200), false);
    assert!(reactor.report().cleanup_failed);
    assert_eq!(reactor.report().first, Some(Reason::Cancelled));
}

#[test]
fn kill_and_reap_failures_block_followup() {
    let mut reactor = running();
    reactor.io_failure();
    reactor.kill_result(false);
    assert_eq!(reactor.report().phase, Phase::Blocked);
    assert_eq!(reactor.report().first, Some(Reason::Io));
    let mut reactor = running();
    reactor.reap_failure();
    assert!(reactor.report().cleanup_failed);
}

#[test]
fn closing_readers_is_not_eof() {
    let mut reactor = running();
    reactor.io_failure();
    reactor.stdin_closed();
    reactor.fds_closed();
    assert!(reactor.report().cleanup_failed);
    assert!(!reactor.report().protocol.stdout_eof_attested);
}

#[test]
fn split_exec_errno_stays_first() {
    let mut reactor = reactor();
    reactor.begin_turn(Duration::ZERO, false);
    for byte in 13i32.to_le_bytes() {
        reactor.exec_status_bytes(&[byte]);
    }
    assert_eq!(reactor.report().first, Some(Reason::ExecErrno(13)));
    reactor.exec_status_eof();
    assert_eq!(reactor.report().first, Some(Reason::ExecErrno(13)));
}

#[test]
fn partial_exec_status_eof_fails_closed() {
    let mut reactor = reactor();
    reactor.begin_turn(Duration::ZERO, false);
    reactor.exec_status_bytes(&[1, 2]);
    reactor.exec_status_eof();
    assert_eq!(reactor.report().first, Some(Reason::ExecStatus));
}

#[test]
fn duplicate_wait_is_rejected() {
    let mut reactor = running();
    reactor.wait_observation(WaitObservation::Exited(1));
    reactor.wait_observation(WaitObservation::Exited(1));
    assert_eq!(reactor.report().first, Some(Reason::ReactorOrder));
}

#[test]
fn per_turn_write_budget_and_backpressure_are_bounded() {
    let mut reactor = running();
    reactor.stderr_bytes(&request(0, 0, 22));
    assert_eq!(reactor.plan().stderr_max, 0);
    assert_eq!(reactor.stdin_bytes().len(), 48);
    reactor.stdin_written(1);
    assert_eq!(reactor.stdin_bytes().len(), 47);
    reactor.stdin_written(47);
    assert_eq!(reactor.stdin_bytes().len(), 22);
    reactor.stdin_written(22);
    reactor.begin_turn(Duration::ZERO, false);
    assert_eq!(reactor.plan().stderr_max, 48);
}

#[test]
fn stdout_quota_prevents_monopolizing_control_turn() {
    let mut reactor = running();
    let plan = reactor.plan();
    assert_eq!(plan.stdout_max, 16_384);
    reactor.stdout_bytes(&vec![0; 16_385]);
    assert_eq!(reactor.report().first, Some(Reason::ReactorOrder));
    reactor.begin_turn(Duration::ZERO, true);
    assert_eq!(reactor.report().first, Some(Reason::ReactorOrder));
}

#[test]
fn stalled_stdin_reaches_operation_deadline() {
    let mut reactor = reactor();
    reactor.begin_turn(Duration::ZERO, false);
    reactor.exec_status_eof();
    assert_eq!(reactor.stdin_bytes().len(), 64);
    reactor.begin_turn(Duration::from_secs(1), false);
    assert_eq!(
        reactor.report().first,
        Some(Reason::Deadline(Phase::Running))
    );
}

#[test]
fn image_digest_mismatch_never_grants_execution() {
    assert_eq!(
        launch::inspect_image_candidate(b"not an approved worker"),
        ImageDecision::DigestMismatch
    );
    assert_eq!(launch::inspect_image_candidate(&[]), ImageDecision::Empty);
    assert!(!launch::live_launch_available());
    assert!(!LIVE_SUPERVISOR_IMPLEMENTED);
}

#[test]
fn launch_contract_requires_all_four_seals_and_fd_exec() {
    assert_eq!(
        launch::required_seals(),
        libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL
    );
    assert_eq!(
        launch::required_memfd_flags(),
        libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING
    );
    assert_eq!(launch::required_exec_flags(), libc::AT_EMPTY_PATH);
}

#[test]
fn reap_with_pending_reply_enters_failure_drain() {
    let mut reactor = running();
    reactor.stderr_bytes(&request(0, 0, 1));
    reactor.wait_observation(WaitObservation::Exited(12));
    assert_eq!(
        reactor.report().first,
        Some(Reason::Range(RangeFault::OutstandingResponse))
    );
    reactor.begin_turn(Duration::ZERO, false);
    assert!(reactor.plan().stderr_max > 0);
    assert!(!reactor.plan().request_kill);
}

#[test]
fn nonzero_exit_without_binding_never_becomes_live_success() {
    let mut reactor = running();
    reactor.stdout_eof();
    reactor.stderr_eof();
    reactor.wait_observation(WaitObservation::Exited(12));
    reactor.stdin_closed();
    reactor.fds_closed();
    assert_eq!(
        reactor.report().protocol.exit,
        zipelo_rust_host_errors::Exit::Natural(12)
    );
    assert!(reactor.report().stop_required);
    assert!(!reactor.report().live_verified);
}

#[test]
fn raw_frames_accept_every_split_through_pr30() {
    let bytes = complete();
    for split in 0..=bytes.len() {
        let mut reactor = running();
        for chunk in [&bytes[..split], &bytes[split..]] {
            if !chunk.is_empty() {
                reactor.stdout_bytes(chunk);
            }
        }
        assert!(reactor.report().protocol.stream.completed);
        assert_eq!(reactor.report().first, None);
        assert!(!reactor.report().live_verified);
    }
}

#[test]
fn completed_frame_and_nonzero_exit_are_contradictory() {
    let mut reactor = running();
    reactor.stdout_bytes(&complete());
    reactor.stdout_eof();
    reactor.wait_observation(WaitObservation::Exited(12));
    assert_eq!(
        reactor.report().first,
        Some(Reason::Raw(
            zipelo_rust_host_errors::Cause::CompletionExitContradiction
        ))
    );
    reactor.begin_turn(Duration::ZERO, true);
    assert_eq!(
        reactor.report().first,
        Some(Reason::Raw(
            zipelo_rust_host_errors::Cause::CompletionExitContradiction
        ))
    );
}

#[test]
fn partial_raw_frame_eof_precedes_cleanup_failure() {
    let mut reactor = running();
    reactor.stdout_bytes(&complete()[..63]);
    reactor.stdout_eof();
    let first = reactor.report().first;
    assert!(matches!(first, Some(Reason::Raw(_))));
    reactor.reap_failure();
    assert_eq!(reactor.report().first, first);
    assert!(reactor.report().cleanup_failed);
}

#[test]
fn buffered_range_after_reap_cannot_create_unsendable_reply() {
    let mut reactor = running();
    reactor.wait_observation(WaitObservation::Exited(12));
    reactor.stderr_bytes(&request(0, 0, 1));
    assert_eq!(
        reactor.report().first,
        Some(Reason::Range(RangeFault::OutstandingResponse))
    );
    assert_eq!(reactor.report().range.requests, 0);
    assert_eq!(reactor.plan().stdin_max, 0);
    assert_eq!(reactor.report().phase, Phase::Stopping);
}

const SESSION_EVENT_LIMIT: usize = 131_072;

// 유효한 6프레임을 나눠 실제 Session 이벤트 예산을 소진함.
fn completed_with_stdout_events(count: usize) -> Reactor {
    let mut bytes = Vec::new();
    let mut item = [0; 16];
    item[0] = 1;
    let frames = [
        (1u16, [0; 16], vec![]),
        (2, item, 131_072u64.to_le_bytes().to_vec()),
        (3, item, vec![0; 65_536]),
        (3, item, vec![0; 65_536]),
        (4, item, 131_072u64.to_le_bytes().to_vec()),
        (5, [0; 16], vec![1]),
    ];
    for (sequence, (kind, item, body)) in frames.into_iter().enumerate() {
        let mut header = [0; 64];
        header[..4].copy_from_slice(b"ZIP2");
        header[4..6].copy_from_slice(&2u16.to_le_bytes());
        header[6..8].copy_from_slice(&kind.to_le_bytes());
        header[8..12].copy_from_slice(&(body.len() as u32).to_le_bytes());
        header[16..32].copy_from_slice(&JOB);
        header[32..48].copy_from_slice(&item);
        header[48..56].copy_from_slice(&(sequence as u64).to_le_bytes());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&body);
    }
    assert!((SESSION_EVENT_LIMIT - 8..=SESSION_EVENT_LIMIT).contains(&count));
    let mut reactor = running();
    for byte in &bytes[..count - 1] {
        reactor.begin_turn(Duration::ZERO, false);
        reactor.stdout_bytes(&[*byte]);
    }
    reactor.begin_turn(Duration::ZERO, false);
    reactor.stdout_bytes(&bytes[count - 1..]);
    let report = reactor.report();
    assert_eq!(report.first, None);
    assert!(report.protocol.stream.completed);
    assert_eq!(report.protocol.stream.frames, 6);
    assert!(!report.protocol.supervisor_invalid);
    reactor
}

fn finish_model_observations(reactor: &mut Reactor) {
    reactor.stdout_eof();
    reactor.stderr_eof();
    reactor.wait_observation(WaitObservation::Exited(0));
    reactor.stdin_closed();
    reactor.fds_closed();
}

fn assert_evidence_rejected(reactor: &Reactor, first: Reason) {
    let report = reactor.report();
    assert_eq!(report.phase, Phase::Blocked);
    assert_eq!(report.first, Some(first));
    assert!(report.cleanup_failed);
    assert!(report.protocol.supervisor_invalid);
    assert!(!report.protocol.cleanup_confirmed_attested);
    assert!(report.stop_required);
    assert!(!report.live_verified && !report.publication);
}

fn supervisor_order() -> Reason {
    Reason::Raw(zipelo_rust_host_errors::Cause::SupervisorOrder)
}

#[test]
fn session_cap_rejecting_io_settled_blocks_model_settlement() {
    let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT - 2);
    finish_model_observations(&mut reactor);
    assert_evidence_rejected(&reactor, supervisor_order());
    assert!(!reactor.report().protocol.io_settled_attested);
    assert!(reactor.report().owned_child_reaped);
}

#[test]
fn session_cap_rejecting_cleanup_complete_preserves_io_confirmation() {
    let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT - 3);
    finish_model_observations(&mut reactor);
    assert_evidence_rejected(&reactor, supervisor_order());
    assert!(reactor.report().protocol.io_settled_attested);
}

#[test]
fn exact_session_cap_accepts_both_cleanup_events() {
    let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT - 4);
    finish_model_observations(&mut reactor);
    let report = reactor.report();
    assert_eq!(report.phase, Phase::SettledModel);
    assert_eq!(report.first, None);
    assert!(!report.cleanup_failed && !report.protocol.supervisor_invalid);
    assert!(report.protocol.io_settled_attested);
    assert!(report.protocol.cleanup_confirmed_attested);
    assert!(report.stop_required && !report.live_verified && !report.publication);
}

#[test]
fn cancelled_first_survives_each_cleanup_event_rejection() {
    for reserve in [3, 4] {
        let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT - reserve);
        reactor.begin_turn(Duration::ZERO, true);
        finish_model_observations(&mut reactor);
        assert_evidence_rejected(&reactor, Reason::Cancelled);
        assert_eq!(reactor.report().protocol.io_settled_attested, reserve == 4);
    }
}

#[test]
fn io_first_survives_each_cleanup_event_rejection() {
    for reserve in [3, 4] {
        let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT - reserve);
        reactor.io_failure();
        finish_model_observations(&mut reactor);
        assert_evidence_rejected(&reactor, Reason::Io);
        assert_eq!(reactor.report().protocol.io_settled_attested, reserve == 4);
    }
}

#[test]
fn raw_failure_survives_cleanup_evidence_rejection() {
    let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT - 4);
    reactor.stdout_bytes(&[0]);
    let first = reactor.report().first.unwrap();
    assert!(matches!(first, Reason::Raw(_)));
    finish_model_observations(&mut reactor);
    assert_evidence_rejected(&reactor, first);
}

#[test]
fn stdout_event_rejection_marks_cleanup_unconfirmed() {
    let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT);
    reactor.stdout_bytes(&[0]);
    assert_evidence_rejected(&reactor, supervisor_order());
}

#[test]
fn stdout_eof_event_rejection_marks_cleanup_unconfirmed() {
    let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT);
    reactor.stdout_eof();
    assert_evidence_rejected(&reactor, supervisor_order());
    assert!(!reactor.report().protocol.stdout_eof_attested);
}

#[test]
fn natural_exit_event_rejection_marks_cleanup_unconfirmed() {
    let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT);
    reactor.wait_observation(WaitObservation::Exited(0));
    assert_evidence_rejected(&reactor, supervisor_order());
    assert_eq!(
        reactor.report().protocol.exit,
        zipelo_rust_host_errors::Exit::Unknown
    );
    assert!(reactor.report().owned_child_reaped);
}

#[test]
fn signalled_exit_event_rejection_marks_cleanup_unconfirmed() {
    let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT);
    reactor.wait_observation(WaitObservation::Signalled);
    assert_evidence_rejected(&reactor, supervisor_order());
    assert_eq!(
        reactor.report().protocol.exit,
        zipelo_rust_host_errors::Exit::Unknown
    );
}

#[test]
fn rejected_host_failure_keeps_original_host_cause() {
    let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT);
    reactor.io_failure();
    assert_evidence_rejected(&reactor, Reason::Io);
}

#[test]
fn rejected_cleanup_failure_keeps_original_confirmation_failure() {
    let mut reactor = completed_with_stdout_events(SESSION_EVENT_LIMIT);
    reactor.fds_closed();
    assert_evidence_rejected(&reactor, Reason::CleanupUnconfirmed);
}
