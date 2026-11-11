fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    if zipelo_archive_worker::runtime::serve(std::io::stdin(), std::io::stdout()).is_err() {
        eprintln!("작업자 통신 실패");
        std::process::exit(2);
    }
}
