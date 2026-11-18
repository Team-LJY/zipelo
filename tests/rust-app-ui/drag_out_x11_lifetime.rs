// X11 끌기 스레드의 종료 조건 시험. drag_out_x11.rs의 `tests` 모듈로 포함됨.
use super::*;

#[test]
fn x11_drag_grace_starts_on_drop_when_files_are_already_ready() {
    let start = Instant::now();
    let mut grace_started_at = None;
    let mut ended = |grabbing, elapsed| {
        session_over(
            grabbing,
            !grabbing,
            || true,
            &mut grace_started_at,
            false,
            start + Duration::from_secs(elapsed),
        )
    };

    assert!(!ended(true, 0));
    assert!(!ended(true, 61));
    // 파일이 준비된 뒤 61초가 지나 놓아도 첫 요청을 받을 수 있음.
    assert!(!ended(false, 61));
    assert!(!ended(false, 120));
    assert!(ended(false, 121));
}

#[test]
fn x11_drag_grace_starts_on_ready_when_already_dropped() {
    let start = Instant::now();
    let mut grace_started_at = None;
    let mut ended = |ready, waiting, elapsed| {
        session_over(
            false,
            true,
            || ready,
            &mut grace_started_at,
            waiting,
            start + Duration::from_secs(elapsed),
        )
    };

    assert!(!ended(false, false, 0));
    assert!(!ended(false, false, 64));
    assert!(!ended(false, true, 64));
    // 놓고 65초가 지나서야 파일 준비가 끝났으므로 이때부터 기다림.
    assert!(!ended(true, false, 65));
    assert!(!ended(true, false, 124));
    assert!(!ended(true, true, 125));
    assert!(ended(true, false, 125));
}

#[test]
fn x11_drag_cancelled_without_drop_ends_immediately() {
    let mut grace_started_at = None;
    assert!(session_over(
        false,
        false,
        || false,
        &mut grace_started_at,
        false,
        Instant::now(),
    ));
}
