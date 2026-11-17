//! 압축파일 안 항목을 창 밖(파일 관리자, 브라우저 업로드 영역 등)으로 끌어 놓는 기능의
//! 운영체제 독립 부분.
//!
//! 끌기를 시작하면 앱은 선택 항목을 임시 폴더에 풀고, 받는 쪽이 데이터를 요청하면 풀린
//! 파일의 경로를 넘김. 실제 복사(폴더 위에 놓으면 그 폴더 안, 빈 곳이면 열린 폴더)와
//! 같은 이름 처리는 받는 프로그램이 함. 운영체제별 끌기 시작은 [`DragOut`] 구현이 맡음.

use std::path::{Path, PathBuf};

/// 운영체제 끌기에서 앱이 알아야 하는 사건.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DragEvent {
    /// 받는 쪽이 놓기를 받아들임. 이제 데이터를 요청하거나 이미 요청함.
    Dropped,
    /// 받는 쪽이 데이터를 다 받음.
    Finished,
    /// 놓지 않고 취소했거나 받는 쪽이 거절함.
    Cancelled,
}

/// 운영체제별 끌어 놓기 시작 구현.
pub trait DragOut {
    /// 지금 누르고 있는 마우스 버튼으로 운영체제 끌기를 시작함. 시작하지 못하면 false.
    fn start(&mut self) -> bool;
    /// 지난 화면 이후 들어온 끌기 사건.
    fn poll(&mut self) -> Option<DragEvent>;
    /// 풀기가 끝난 파일과 폴더의 절대 경로를 넘김. 이미 들어온 요청과 이후 요청에 씀.
    fn provide(&mut self, paths: &[PathBuf]);
    /// 풀기에 실패함. 기다리는 요청에는 빈 목록을 보내고 끌기를 끝냄.
    fn fail(&mut self);
}

/// RFC 2483 text/uri-list 본문을 만듦. 경로의 각 바이트를 file URI 규칙으로 바꿈.
pub fn uri_list(paths: &[PathBuf]) -> String {
    let mut text = String::new();
    for path in paths {
        text.push_str(&file_uri(path));
        text.push_str("\r\n");
    }
    text
}

/// 절대 경로를 file:// URI로 바꿈. 영문, 숫자, '-', '.', '_', '~', '/'만 그대로 두고
/// 나머지 바이트(공백, 한글 UTF-8 등)는 %XX로 씀.
pub fn file_uri(path: &Path) -> String {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes = path.to_string_lossy().replace('\\', "/").into_bytes();
    let mut uri = String::from("file://");
    #[cfg(not(unix))]
    uri.push('/');
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn uri_list_escapes_spaces_hangul_and_reserved_bytes() {
        let paths = [
            PathBuf::from("/tmp/여행 자료/a#b.txt"),
            PathBuf::from("/tmp/x/폴더"),
        ];
        assert_eq!(
            uri_list(&paths),
            "file:///tmp/%EC%97%AC%ED%96%89%20%EC%9E%90%EB%A3%8C/a%23b.txt\r\n\
             file:///tmp/x/%ED%8F%B4%EB%8D%94\r\n"
        );
    }
}
