// 끌어 놓기 file URI 목록 시험. drag_out.rs의 `tests` 모듈로 포함됨.
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
