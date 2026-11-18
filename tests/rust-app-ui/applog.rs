// 앱 로그 단계와 Debug 요약 시험. applog.rs의 `tests` 모듈로 포함됨.
use super::*;

#[test]
fn release_builds_log_errors_only_and_debug_builds_log_everything() {
    let expected = if cfg!(debug_assertions) {
        Level::Trace
    } else {
        Level::Error
    };
    assert_eq!(Level::default_for_build(), expected);
    assert_eq!(Level::parse(" Debug "), Some(Level::Debug));
    assert_eq!(Level::parse("off"), Some(Level::Off));
    assert_eq!(Level::parse("loud"), None);
    assert!(Level::Error < Level::Trace);
}

#[test]
fn brief_stops_formatting_large_values() {
    let data = vec![7u8; 100_000];
    let text = brief(&data, 20);
    assert_eq!(text.chars().count(), 21);
    assert!(text.ends_with('…'));
    assert_eq!(brief(&"짧음", 20), "\"짧음\"");
}
