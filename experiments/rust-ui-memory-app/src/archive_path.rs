use std::path::{Path, PathBuf};

pub fn windows_path(text: &str) -> String {
    let Some(path) = text.strip_prefix(r"\\?\") else {
        return text.to_owned();
    };
    let bytes = path.as_bytes();
    // 일반 드라이브의 의미가 바뀌지 않는 경로만 변환함.
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || &bytes[1..3] != b":\\"
        || (path.len() > 3
            && path[3..].split('\\').any(|part| {
                part.is_empty()
                    || worker_protocol::archive::valid_name(part, false).is_err()
                    || part.contains('/')
            }))
    {
        return text.to_owned();
    }
    path.to_owned()
}

pub fn worker_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(text) = path.to_str() {
        return PathBuf::from(windows_path(text));
    }
    path.to_path_buf()
}
