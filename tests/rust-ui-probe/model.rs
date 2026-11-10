use zipelo_rust_ui_probe::{
    Job, ModelError, Navigation, Phase, ROW_COUNT, Selection, row_id, row_name,
};

#[test]
fn empty_list_has_no_selection() {
    let mut s = Selection::new(0);
    assert!(!s.select(0));
    assert_eq!(s.navigate(Navigation::Last), None);
}

#[test]
fn hundred_thousand_rows_keep_stable_unique_ids() {
    for i in 0..ROW_COUNT {
        assert_eq!(row_id(i), Some(i as u64 + 1));
    }
    assert_eq!(row_id(ROW_COUNT), None);
    assert_eq!(
        row_name(0).as_deref(),
        Some("자료/000001_한글 합성 문서.txt")
    );
}

#[test]
fn navigation_reaches_and_clamps_both_ends() {
    let mut s = Selection::new(ROW_COUNT);
    assert_eq!(s.navigate(Navigation::Next), Some(0));
    assert_eq!(s.navigate(Navigation::Previous), Some(0));
    assert_eq!(s.navigate(Navigation::Last), Some(99_999));
    assert_eq!(s.navigate(Navigation::Next), Some(99_999));
    assert_eq!(s.navigate(Navigation::First), Some(0));
}

#[test]
fn page_navigation_saturates_without_overflow() {
    let mut s = Selection::new(ROW_COUNT);
    assert_eq!(s.navigate(Navigation::PageDown(usize::MAX)), Some(99_999));
    assert_eq!(s.navigate(Navigation::PageUp(usize::MAX)), Some(0));
    assert!(s.select(400));
    assert_eq!(s.navigate(Navigation::PageDown(20)), Some(420));
    assert_eq!(s.navigate(Navigation::PageUp(20)), Some(400));
}

#[test]
fn invalid_selection_preserves_previous_row() {
    let mut s = Selection::new(ROW_COUNT);
    assert!(s.select(50));
    assert!(!s.select(ROW_COUNT));
    assert_eq!(s.selected(), Some(50));
}

#[test]
fn zero_work_is_rejected_without_starting() {
    let mut j = Job::default();
    assert_eq!(j.start(0), Err(ModelError::EmptyWork));
    assert_eq!(j.phase(), Phase::Idle);
}

#[test]
fn active_job_rejects_duplicate_start() {
    let mut j = Job::default();
    let id = j.start(10).unwrap();
    assert_eq!(j.start(20), Err(ModelError::Busy));
    assert_eq!((j.id(), j.total()), (id, 10));
}

#[test]
fn progress_is_monotonic_and_bounded() {
    let mut j = Job::default();
    let id = j.start(10).unwrap();
    assert!(j.progress(id, 7));
    assert!(!j.progress(id, 6));
    assert!(!j.progress(id, 11));
    assert!(!j.progress(id + 1, 9));
    assert_eq!(j.done(), 7);
}

#[test]
fn completion_requires_all_progress() {
    let mut j = Job::default();
    let id = j.start(10).unwrap();
    assert!(!j.complete(id));
    assert!(j.progress(id, 10));
    assert!(j.complete(id));
    assert_eq!(j.phase(), Phase::MockCompleted);
    assert!(!j.request_cancel());
}

#[test]
fn cancellation_waits_for_explicit_mock_ack() {
    let mut j = Job::default();
    let id = j.start(10).unwrap();
    assert!(j.request_cancel());
    assert_eq!(j.phase(), Phase::CancelRequested);
    assert!(!j.progress(id, 10));
    assert!(!j.complete(id));
    assert_eq!(j.start(1), Err(ModelError::Busy));
    assert!(j.acknowledge_mock_cancel(id));
    assert_eq!(j.phase(), Phase::MockCancelled);
}

#[test]
fn cancelled_job_cannot_accept_late_success() {
    let mut j = Job::default();
    let old = j.start(10).unwrap();
    j.request_cancel();
    j.acknowledge_mock_cancel(old);
    assert!(!j.complete(old));
    let new = j.start(20).unwrap();
    assert_ne!(old, new);
    assert!(!j.progress(old, 10));
    assert!(!j.complete(old));
    assert!(!j.fail(old));
    assert_eq!((j.phase(), j.done()), (Phase::Running, 0));
}

#[test]
fn error_is_terminal_and_does_not_become_success() {
    let mut j = Job::default();
    let id = j.start(10).unwrap();
    assert!(j.fail(id));
    assert!(!j.progress(id, 10));
    assert!(!j.complete(id));
    assert!(!j.acknowledge_mock_cancel(id));
    assert_eq!(j.phase(), Phase::MockFailed);
}

#[test]
fn error_during_cancel_preserves_failure() {
    let mut j = Job::default();
    let id = j.start(10).unwrap();
    j.request_cancel();
    assert!(j.fail(id));
    assert!(!j.acknowledge_mock_cancel(id));
    assert_eq!(j.phase(), Phase::MockFailed);
}

#[test]
fn wrong_cancel_ack_cannot_settle_job() {
    let mut j = Job::default();
    let id = j.start(10).unwrap();
    assert!(!j.acknowledge_mock_cancel(id));
    j.request_cancel();
    assert!(!j.acknowledge_mock_cancel(id + 1));
    assert_eq!(j.phase(), Phase::CancelRequested);
}
