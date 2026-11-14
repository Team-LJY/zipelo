use crate::support::*;
use snapshot::{Error, IoOperation, Limits, Unsupported, Usage};
use std::{
    ffi::{CString, OsString},
    fs,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::symlink,
        net::UnixListener,
    },
    path::{Path, PathBuf},
};

fn invalid(path: &Path) {
    let failure = get(path, Limits::default()).unwrap_err();
    assert!(matches!(failure.cause, Error::InvalidPath));
    assert_eq!(failure.usage, Usage::default());
}

#[test]
fn empty_path() {
    invalid(Path::new(""));
}

#[test]
fn relative_path() {
    invalid(Path::new("relative"));
}

#[test]
fn root_only() {
    invalid(Path::new("/"));
}

#[test]
fn dot_component() {
    invalid(Path::new("/owned/./input"));
}

#[test]
fn parent_component() {
    invalid(Path::new("/owned/../input"));
}

#[test]
fn repeated_slash() {
    invalid(Path::new("/owned//input"));
}

#[test]
fn trailing_slash() {
    invalid(Path::new("/owned/input/"));
}

#[test]
fn nul_byte() {
    invalid(Path::new("/owned/in\0put"));
}

#[test]
fn component_over_255() {
    invalid(&PathBuf::from(format!("/{}", "x".repeat(256))));
}

#[test]
fn path_over_4096() {
    invalid(&PathBuf::from(format!("/{}", "x/".repeat(2049))));
}

fn pin_error(path: &Path, code: i32) {
    let failure = get(path, Limits::default()).unwrap_err();
    match failure.cause {
        Error::Io { operation, source } => {
            assert_eq!(operation, IoOperation::PinPath);
            assert_eq!(source.raw_os_error(), Some(code));
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert_eq!(failure.usage.open_calls, 1);
    assert_eq!(failure.usage.metadata_calls, 0);
    assert_eq!(failure.usage.read_calls, 0);
}

#[test]
fn missing_path_preserves_enoent_and_stage() {
    let sandbox = Sandbox::new();
    pin_error(&sandbox.path("missing"), libc::ENOENT);
    sandbox.finish();
}

#[test]
fn regular_parent_component_preserves_enotdir() {
    let sandbox = Sandbox::new();
    let file = sandbox.file("parent", b"unchanged");
    pin_error(&file.join("input"), libc::ENOTDIR);
    assert_eq!(fs::read(file).unwrap(), b"unchanged");
    sandbox.finish();
}

#[test]
fn leaf_symlink_rejected_without_reading_target() {
    let sandbox = Sandbox::new();
    let target = sandbox.file("target", b"unchanged");
    let path = sandbox.path("link");
    symlink(&target, &path).unwrap();
    pin_error(&path, libc::ELOOP);
    assert_eq!(fs::read(target).unwrap(), b"unchanged");
    sandbox.finish();
}

#[test]
fn ancestor_symlink_rejected() {
    let sandbox = Sandbox::new();
    let parent = sandbox.dir("parent");
    sandbox.file("parent/input", b"unchanged");
    let link = sandbox.path("alias");
    symlink(parent, &link).unwrap();
    pin_error(&link.join("input"), libc::ELOOP);
    sandbox.finish();
}

#[test]
fn dangling_symlink_preserves_eloop() {
    let sandbox = Sandbox::new();
    let path = sandbox.path("link");
    symlink(sandbox.path("missing"), &path).unwrap();
    pin_error(&path, libc::ELOOP);
    sandbox.finish();
}

fn unsupported(path: &Path, expected: Unsupported) {
    let failure = get(path, Limits::default()).unwrap_err();
    assert!(matches!(failure.cause, Error::Unsupported(value) if value == expected));
    assert_eq!(failure.usage.open_calls, 1);
    assert_eq!(failure.usage.metadata_calls, 1);
    assert_eq!(failure.usage.read_calls, 0);
}

#[test]
fn directory_rejected_before_data_open() {
    let sandbox = Sandbox::new();
    unsupported(&sandbox.dir("directory"), Unsupported::FileType);
    sandbox.finish();
}

#[test]
fn fifo_without_writer_rejected_before_data_open() {
    let sandbox = Sandbox::new();
    let path = sandbox.path("fifo");
    let raw = CString::new(path.as_os_str().as_bytes()).unwrap();
    // 작업 전용 경로에 합성 FIFO만 생성함.
    assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
    unsupported(&path, Unsupported::FileType);
    sandbox.finish();
}

#[test]
fn unix_socket_rejected_before_data_open() {
    let sandbox = Sandbox::new();
    let path = sandbox.path("socket");
    use std::os::fd::AsRawFd;
    let directory = fs::File::open(path.parent().unwrap()).unwrap();
    let socket =
        UnixListener::bind(format!("/proc/self/fd/{}/socket", directory.as_raw_fd())).unwrap();
    unsupported(&path, Unsupported::FileType);
    drop(socket);
    sandbox.finish();
}

#[test]
fn multiple_hard_links_rejected_and_contents_preserved() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"unchanged");
    let link = sandbox.path("hardlink");
    fs::hard_link(&path, &link).unwrap();
    unsupported(&path, Unsupported::MultipleLinks);
    assert_eq!(fs::read(path).unwrap(), b"unchanged");
    assert_eq!(fs::read(link).unwrap(), b"unchanged");
    sandbox.finish();
}

#[test]
fn non_utf8_linux_name_is_supported() {
    let sandbox = Sandbox::new();
    let path = sandbox.root.join(OsString::from_vec(vec![b'i', 0xff]));
    fs::write(&path, b"abc").unwrap();
    assert_eq!(get(&path, Limits::default()).unwrap().bytes(), b"abc");
    sandbox.finish();
}

#[test]
fn linux_backslash_and_colon_are_name_bytes() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("a\\b:c", b"abc");
    assert_eq!(get(&path, Limits::default()).unwrap().bytes(), b"abc");
    sandbox.finish();
}

#[test]
fn component_255_bytes_is_supported() {
    let sandbox = Sandbox::new();
    let path = sandbox.file(&"x".repeat(255), b"abc");
    assert_eq!(get(&path, Limits::default()).unwrap().bytes(), b"abc");
    sandbox.finish();
}
