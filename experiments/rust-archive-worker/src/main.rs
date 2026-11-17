fn main() {
    // 배포 빌드는 패닉 내용을 남기지 않고, 디버그 빌드는 원인을 자세히 남김.
    if cfg!(debug_assertions) {
        eprintln!("[worker {}] 시작", std::process::id());
    } else {
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
    if cfg!(debug_assertions) {
        eprintln!("[worker {}] 정상 종료", std::process::id());
    }
}
