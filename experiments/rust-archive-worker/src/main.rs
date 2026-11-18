fn main() {
    // 정상 동작 중에는 표준 오류에 아무것도 쓰지 않음(부모는 비어 있는 표준 오류를 기대함).
    // 배포 빌드는 패닉 내용을 숨기고, 디버그 빌드는 기본 패닉 출력으로 원인을 자세히 남김.
    if !cfg!(debug_assertions) {
        std::panic::set_hook(Box::new(|_| {}));
    }
    if let Err(error) = zipelo_archive_worker::runtime::serve(std::io::stdin(), std::io::stdout()) {
        if cfg!(debug_assertions) {
            eprintln!("[worker {}] 작업자 통신 실패: {error:?}", std::process::id());
        } else {
            eprintln!("작업자 통신 실패");
        }
        std::process::exit(2);
    }
}
