#![cfg(target_os = "linux")]

#[path = "../../experiments/rust-ui-memory-app/src/system_font.rs"]
mod system_font;

use std::{fs, os::unix::fs::MetadataExt};

const FONT: &str = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc";
const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;

fn identity(value: &fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        value.dev(),
        value.ino(),
        value.len(),
        value.mtime(),
        value.mtime_nsec(),
        value.ctime(),
        value.ctime_nsec(),
    )
}

#[test]
fn linux_loader_preserves_installed_bytes_and_korean_face_index() {
    let metadata_before = fs::metadata(FONT).expect("설치된 Noto 글꼴 확인 실패");
    assert!(metadata_before.is_file(), "일반 글꼴 파일이어야 함");
    assert!(
        (1..=MAX_FONT_BYTES).contains(&metadata_before.len()),
        "글꼴 입력 상한 초과 또는 빈 파일임"
    );
    let before = fs::read(FONT).expect("기준 글꼴 읽기 실패");
    assert_eq!(before.len() as u64, metadata_before.len());

    let loaded = system_font::load().expect("생산 Linux 로더 실패");
    assert_eq!(loaded.face_index, 1);
    assert_eq!(loaded.bytes.len(), before.len());
    assert!(loaded.bytes == before, "생산 로더와 설치 글꼴 bytes가 다름");

    let after = fs::read(FONT).expect("글꼴 보존 확인 실패");
    let metadata_after = fs::metadata(FONT).expect("글꼴 identity 확인 실패");
    assert!(after == before, "시험 중 설치 글꼴 bytes가 변경됨");
    assert_eq!(identity(&metadata_before), identity(&metadata_after));
    println!(
        "font_bytes={} face_index={} input_unchanged=true",
        loaded.bytes.len(),
        loaded.face_index
    );
}
