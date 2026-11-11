fn main() {
    if zipelo_archive_worker::runtime::serve(std::io::stdin(), std::io::stdout()).is_err() {
        std::process::exit(2);
    }
}
