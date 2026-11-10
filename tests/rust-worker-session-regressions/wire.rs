use crate::support::*;

#[test]
fn all_header_kinds_accept_only_canonical_lengths() {
    for (kind, good, bad) in [
        (Kind::RequestBegin, vec![16], vec![0, 15, 17, u32::MAX]),
        (Kind::RequestData, vec![1, 65536], vec![0, 65537, u32::MAX]),
        (Kind::RequestEnd, vec![0], vec![1, u32::MAX]),
        (Kind::ResultBegin, vec![8], vec![0, 7, 9, u32::MAX]),
        (Kind::ResultData, vec![1, 65536], vec![0, 65537, u32::MAX]),
        (Kind::ResultEnd, vec![1], vec![0, 2, u32::MAX]),
    ] {
        for length in good {
            let h = Header::inspect(&encode_header(kind, id(), u64::MAX, length)).unwrap();
            assert_eq!(h.kind(), kind);
            assert_eq!(h.payload_length(), length as usize);
            assert_eq!(h.sequence(), u64::MAX);
            assert_eq!(h.id().bytes(), [19; 16]);
        }
        for length in bad {
            assert_eq!(
                Header::inspect(&encode_header(kind, id(), 0, length)),
                Err(ProtocolError::InvalidLength)
            );
        }
    }
}
#[test]
fn every_reserved_byte_and_magic_version_byte_is_checked() {
    let h = encode_header(Kind::ResultBegin, id(), 0, 8);
    for index in (0..6).chain(12..16).chain(40..64) {
        let mut bad = h;
        bad[index] ^= 1;
        assert_eq!(Header::inspect(&bad), Err(ProtocolError::InvalidHeader));
    }
}
#[test]
fn unknown_kind_and_zero_wire_identity_are_rejected() {
    for kind in [0_u16, 4, 15, 19, u16::MAX] {
        let mut h = encode_header(Kind::ResultBegin, id(), 0, 8);
        h[6..8].copy_from_slice(&kind.to_le_bytes());
        assert_eq!(Header::inspect(&h), Err(ProtocolError::InvalidHeader));
    }
    let mut h = encode_header(Kind::ResultBegin, id(), 0, 8);
    h[16..32].fill(0);
    assert_eq!(Header::inspect(&h), Err(ProtocolError::InvalidHeader));
}
#[test]
fn begin_before_request_end_is_rejected_with_cost() {
    let mut m = model();
    assert_eq!(
        frame(&mut m, Kind::ResultBegin, 0, &0_u64.to_le_bytes()),
        Err(ApplyError::Fault(Fault::Protocol(
            ProtocolError::UnexpectedFrame
        )))
    );
    assert_eq!(m.report().usage.result_wire_bytes, 72);
    assert_eq!(m.report().usage.result_frames, 1);
    protocol(&m, ProtocolError::UnexpectedFrame);
}
#[test]
fn supplied_body_length_mismatch_charges_only_wire() {
    for payload in [&[0_u8; 7][..], &[0_u8; 9][..]] {
        let mut m = model();
        sent(&mut m);
        assert!(
            raw(
                &mut m,
                &encode_header(Kind::ResultBegin, id(), 0, 8),
                payload
            )
            .is_err()
        );
        protocol(&m, ProtocolError::InvalidLength);
        assert_eq!(
            m.report().usage.result_wire_bytes,
            64 + payload.len() as u64
        );
        assert_eq!(m.report().usage.result_frames, 0);
    }
}
#[test]
fn duplicate_begin_preserves_initial_declaration_and_fault() {
    let mut m = model();
    sent(&mut m);
    begin(&mut m, 0);
    assert!(frame(&mut m, Kind::ResultBegin, 1, &9_u64.to_le_bytes()).is_err());
    protocol(&m, ProtocolError::UnexpectedFrame);
    assert_eq!(m.report().usage.result_wire_bytes, 144);
    assert_eq!(m.report().usage.result_frames, 2);
    assert_eq!(m.report().usage.result_data_bytes, 0);
}
#[test]
fn data_and_terminal_before_begin_are_rejected() {
    for kind in [Kind::ResultData, Kind::ResultEnd] {
        let mut m = model();
        sent(&mut m);
        assert!(frame(&mut m, kind, 0, &[1]).is_err());
        protocol(&m, ProtocolError::UnexpectedFrame);
        assert_eq!(m.report().usage.result_frames, 1);
    }
}
#[test]
fn wrong_identity_or_sequence_charges_wire_not_frame() {
    for (other, seq, error) in [
        (
            RequestId::new([20; 16]).unwrap(),
            0,
            ProtocolError::IdMismatch,
        ),
        (id(), u64::MAX, ProtocolError::SequenceMismatch),
    ] {
        let mut m = model();
        sent(&mut m);
        assert!(
            raw(
                &mut m,
                &encode_header(Kind::ResultBegin, other, seq, 8),
                &0_u64.to_le_bytes()
            )
            .is_err()
        );
        protocol(&m, error);
        assert_eq!(m.report().usage.result_wire_bytes, 72);
        assert_eq!(m.report().usage.result_frames, 0);
    }
}
#[test]
fn all_request_kinds_on_result_channel_are_rejected() {
    for (kind, bytes) in [
        (Kind::RequestBegin, vec![0; 16]),
        (Kind::RequestData, vec![1]),
        (Kind::RequestEnd, vec![]),
    ] {
        let mut m = model();
        sent(&mut m);
        assert!(frame(&mut m, kind, 0, &bytes).is_err());
        protocol(&m, ProtocolError::UnexpectedFrame);
    }
}
#[test]
fn unknown_worker_status_never_becomes_completion() {
    for status in [0, 4, 255] {
        let mut m = model();
        sent(&mut m);
        begin(&mut m, 0);
        assert!(frame(&mut m, Kind::ResultEnd, 1, &[status]).is_err());
        protocol(&m, ProtocolError::InvalidHeader);
        assert_eq!(m.report().worker_report, None);
    }
}
#[test]
fn explicit_truncation_and_eof_without_terminal_latch_truncated() {
    for explicit in [false, true] {
        let mut m = model();
        sent(&mut m);
        begin(&mut m, 0);
        let result = apply(
            &mut m,
            ZERO,
            if explicit {
                Observation::TruncatedFrame
            } else {
                Observation::ResultEof
            },
        );
        assert_eq!(result.is_err(), explicit);
        protocol(&m, ProtocolError::Truncated);
        assert_eq!(m.report().missing.result_eof, explicit);
    }
}
#[test]
fn trailing_frame_after_terminal_does_not_add_wire_or_frames() {
    let mut m = model();
    complete(&mut m);
    let before = m.report().usage;
    assert!(frame(&mut m, Kind::ResultData, 2, b"x").is_err());
    protocol(&m, ProtocolError::TrailingData);
    let after = m.report().usage;
    assert_eq!(before.result_wire_bytes, after.result_wire_bytes);
    assert_eq!(before.result_frames, after.result_frames);
    assert_eq!(after.observations, before.observations + 1);
}
#[test]
fn full_chunk_and_final_single_byte_accumulate_exactly() {
    let mut m = model();
    sent(&mut m);
    begin(&mut m, 65537);
    frame(&mut m, Kind::ResultData, 1, &[9; CHUNK_BYTES]).unwrap();
    frame(&mut m, Kind::ResultData, 2, &[8]).unwrap();
    frame(&mut m, Kind::ResultEnd, 3, &[1]).unwrap();
    cleanup(&mut m, 0);
    assert_eq!(m.report().usage.result_data_bytes, 65537);
    assert_eq!(m.report().usage.result_wire_bytes, 65537 + 8 + 1 + 4 * 64);
    assert_eq!(m.report().outcome, Outcome::ProtocolCompletionObserved);
}
#[test]
fn failed_and_cancelled_reports_may_end_partial_output_without_authority() {
    for status in [WorkerStatus::Failed, WorkerStatus::Cancelled] {
        let mut m = model();
        sent(&mut m);
        begin(&mut m, 100);
        frame(&mut m, Kind::ResultData, 1, b"x").unwrap();
        frame(&mut m, Kind::ResultEnd, 2, &[status as u8]).unwrap();
        cleanup(&mut m, 7);
        assert_eq!(m.report().outcome, Outcome::WorkerReported(status));
        assert_eq!(m.report().usage.result_data_bytes, 1);
    }
}

#[test]
fn terminal_header_and_supplied_body_lengths_are_validated_separately() {
    for (declared, payload, charged, frames) in [
        (0, vec![], 64, 1),
        (2, vec![1, 0], 66, 1),
        (1, vec![], 64, 1),
        (1, vec![1, 0], 66, 1),
    ] {
        let mut m = model();
        sent(&mut m);
        begin(&mut m, 0);
        let header = encode_header(Kind::ResultEnd, id(), 1, declared);
        assert!(raw(&mut m, &header, &payload).is_err());
        protocol(&m, ProtocolError::InvalidLength);
        assert_eq!(m.report().worker_report, None);
        assert_eq!(m.report().usage.result_wire_bytes, 72 + charged);
        assert_eq!(m.report().usage.result_frames, frames);
        assert_eq!(m.report().usage.result_data_bytes, 0);
    }
}

#[test]
fn malformed_frame_after_valid_data_keeps_cumulative_cost_and_first_fault() {
    let mut m = model();
    sent(&mut m);
    begin(&mut m, 2);
    frame(&mut m, Kind::ResultData, 1, b"a").unwrap();
    let mut bad = encode_header(Kind::ResultData, id(), 2, 1);
    bad[40] = 1;
    assert!(raw(&mut m, &bad, b"b").is_err());
    protocol(&m, ProtocolError::InvalidHeader);
    let before = m.report().usage;
    assert_eq!(before.result_wire_bytes, 72 + 65 + 65);
    assert_eq!(before.result_frames, 2);
    assert_eq!(before.result_data_bytes, 1);
    assert!(frame(&mut m, Kind::ResultEnd, 2, &[1]).is_err());
    apply(&mut m, ZERO, Observation::Cancel).unwrap();
    protocol(&m, ProtocolError::InvalidHeader);
    let after = m.report().usage;
    assert_eq!(after.result_wire_bytes, before.result_wire_bytes);
    assert_eq!(after.result_frames, before.result_frames);
    assert_eq!(after.result_data_bytes, before.result_data_bytes);
    assert_eq!(after.observations, before.observations + 2);
}
