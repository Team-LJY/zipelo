// X11 끌기 스레드의 종료 조건 시험. drag_out_x11.rs의 `tests` 모듈로 포함됨.
use super::*;

#[test]
fn x11_drag_waits_for_slow_files_after_drop() {
    // 끌고 있는 동안에는 끝내지 않음.
    assert!(!session_over(true, false, None, false));
    // 놓지 않고 끝났으면 바로 끝냄.
    assert!(session_over(false, false, None, false));
    // 놓은 뒤 파일 준비가 60초를 넘어도(아직 준비 전) 끝내지 않음.
    assert!(!session_over(false, true, None, true));
    assert!(!session_over(false, true, None, false));
    // 준비된 뒤에도 늦은 요청을 위해 잠시 남음.
    assert!(!session_over(
        false,
        true,
        Some(Duration::from_secs(5)),
        false
    ));
    // 기다리는 요청이 있으면 끝내지 않음.
    assert!(!session_over(false, true, Some(READY_GRACE * 2), true));
    // 준비되고 요청을 다 처리한 뒤 유예 시간이 지나면 끝냄.
    assert!(session_over(false, true, Some(READY_GRACE), false));
}
