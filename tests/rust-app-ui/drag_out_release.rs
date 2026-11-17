// 끌던 버튼을 놓을 때의 처리 판단 시험. drag_out.rs의 `release_tests` 모듈로 포함됨.
use super::*;

#[test]
fn release_over_own_window_cancels_without_waiting() {
    // 아직 푸는 중이어도 기다리지 않고 바로 취소함.
    assert_eq!(release_action(None, true), ReleaseAction::Cancel);
    // 이미 다 풀렸어도 자기 창이면 받을 곳이 없으므로 취소함.
    assert_eq!(release_action(Some(true), true), ReleaseAction::Cancel);
    assert_eq!(release_action(Some(false), true), ReleaseAction::Cancel);
}

#[test]
fn release_over_other_window_keeps_drop_and_wait() {
    assert_eq!(release_action(None, false), ReleaseAction::Wait);
    assert_eq!(release_action(Some(true), false), ReleaseAction::Drop);
    assert_eq!(release_action(Some(false), false), ReleaseAction::Cancel);
}

#[test]
fn own_window_is_decided_by_owner_process() {
    assert!(is_own_window(Some(4242), 4242));
    assert!(!is_own_window(Some(100), 4242));
    // 커서 아래에 창이 없거나 소유자를 알 수 없으면 자기 창이 아님.
    assert!(!is_own_window(None, 4242));
}
