use crate::support::*;
use publication::{Error, Limits, Publication, Request, Stop, Usage};
use std::{
    ffi::{CString, OsStr, OsString},
    fs,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{FileTypeExt, MetadataExt, symlink},
        net::UnixListener,
    },
    path::{Path, PathBuf},
};

fn invalid(parent: &Path, leaf: &OsStr) {
    let (task, _) = Publication::new();
    let (error, usage, closed) = not_committed(task.publish(
        Request {
            parent,
            leaf,
            bytes: b"abc",
            limits: Limits::default(),
        },
        |_| -> Result<(), Stop<()>> { panic!("invalid path callback") },
    ));
    assert!(matches!(error, Error::InvalidPath));
    assert_eq!(usage, Usage::default());
    cleanup(&closed, 0);
}

#[test]
fn invalid_parent_forms_rejected_before_callback() {
    for value in [
        "",
        "relative",
        "/owned/./x",
        "/owned/../x",
        "/owned//x",
        "/owned/",
        "/owned/\0x",
    ] {
        invalid(Path::new(value), OsStr::new("output"));
    }
    invalid(
        &PathBuf::from(format!("/{}", "x".repeat(4095))),
        OsStr::new("output"),
    );
}

#[test]
fn invalid_leaf_forms_rejected_before_callback() {
    for value in ["", ".", "..", "a/b", "/absolute", "nul\0byte"] {
        invalid(Path::new("/owned"), OsStr::new(value));
    }
    invalid(Path::new("/owned"), OsStr::new(&"x".repeat(256)));
}

#[test]
fn root_parent_is_lexically_valid_without_opening_root() {
    let (task, _) = Publication::new();
    let (error, usage, closed) = not_committed(task.publish(
        Request {
            parent: Path::new("/"),
            leaf: OsStr::new("unused"),
            bytes: &[],
            limits: Limits::default(),
        },
        |_| Err(Stop::<()>::Cancelled),
    ));
    assert!(matches!(error, Error::Cancelled));
    assert_eq!(usage, Usage::default());
    cleanup(&closed, 0);
}

fn open_error(parent: &Path, expected: i32) {
    let (error, usage, closed) = not_committed(publish(
        parent,
        OsStr::new("output"),
        b"abc",
        Limits::default(),
    ));
    match error {
        Error::Io { operation, source } => {
            assert_eq!(operation, "openat2");
            assert_eq!(source.raw_os_error(), Some(expected));
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert_eq!(usage.open_calls, 1);
    assert_eq!(
        usage.metadata_calls + usage.link_calls + usage.write_calls + usage.read_calls,
        0
    );
    cleanup(&closed, 0);
}

#[test]
fn missing_and_regular_parent_preserve_errno() {
    let sandbox = Sandbox::new();
    open_error(&sandbox.path("missing"), libc::ENOENT);
    let regular = sandbox.file("regular", b"untouched");
    open_error(&regular, libc::ENOTDIR);
    assert_eq!(fs::read(regular).unwrap(), b"untouched");
    assert!(names(&sandbox.parent()).is_empty());
    sandbox.finish();
}

#[test]
fn parent_and_ancestor_symlinks_rejected() {
    let sandbox = Sandbox::new();
    let link = sandbox.path("alias");
    symlink(sandbox.parent(), &link).unwrap();
    open_error(&link, libc::ELOOP);
    let ancestor = sandbox.path("ancestor");
    symlink(&sandbox.root, &ancestor).unwrap();
    open_error(&ancestor.join("parent"), libc::ELOOP);
    assert!(names(&sandbox.parent()).is_empty());
    sandbox.finish();
}

#[test]
fn non_utf8_leaf_and_parent_are_supported() {
    let sandbox = Sandbox::new();
    let parent_name = OsString::from_vec(vec![b'p', 0xfe]);
    let parent = sandbox.dir(Path::new(&parent_name));
    let leaf = OsString::from_vec(vec![b'o', 0xff]);
    committed(
        publish(&parent, &leaf, b"abc", Limits::default()),
        &parent,
        &leaf,
        b"abc",
    );
    sandbox.finish();
}

#[test]
fn maximum_leaf_length_is_supported() {
    let sandbox = Sandbox::new();
    let leaf = OsString::from("x".repeat(255));
    let parent = sandbox.parent();
    committed(
        publish(&parent, &leaf, b"abc", Limits::default()),
        &parent,
        &leaf,
        b"abc",
    );
    sandbox.finish();
}

#[test]
fn linux_backslash_and_colon_are_leaf_bytes() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let leaf = OsStr::new("a\\b:c");
    committed(
        publish(&parent, leaf, b"abc", Limits::default()),
        &parent,
        leaf,
        b"abc",
    );
    sandbox.finish();
}

#[derive(Clone, Copy)]
enum Existing {
    Regular,
    Directory,
    Symlink,
    Dangling,
    HardLink,
    Fifo,
    Socket,
}

fn bind_socket(output: &Path) -> UnixListener {
    // Cargo 시험의 현재 디렉터리 기준으로 소켓 주소만 짧게 지정함.
    let current = std::env::current_dir().unwrap();
    let relative = output.strip_prefix(&current).unwrap();
    UnixListener::bind(relative).unwrap()
}

#[test]
fn socket_conflict_preserves_output_beyond_absolute_address_limit() {
    let sandbox = Sandbox::new();
    let padding = 108usize
        .saturating_sub(sandbox.root.as_os_str().as_bytes().len() + "/output".len())
        .max(1);
    let parent = sandbox.dir("p".repeat(padding));
    let output = parent.join("output");
    assert!(output.as_os_str().as_bytes().len() >= 108);
    let listener = bind_socket(&output);
    let before = fs::symlink_metadata(&output).unwrap();
    assert!(before.file_type().is_socket());
    conflict(
        publish(
            &parent,
            OsStr::new("output"),
            b"replacement",
            Limits::default(),
        ),
        11,
    );
    let after = fs::symlink_metadata(&output).unwrap();
    assert!(after.file_type().is_socket());
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    assert_eq!(names(&parent), vec![OsString::from("output")]);
    drop(listener);
    sandbox.finish();
}

fn existing(kind: Existing) {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let leaf = OsStr::new("output");
    let output = parent.join(leaf);
    let sentinel = sandbox.file("sentinel", b"untouched");
    let mut listener = None;
    match kind {
        Existing::Regular => {
            sandbox.file("parent/output", b"existing");
        }
        Existing::Directory => {
            sandbox.dir("parent/output");
            sandbox.file("parent/output/child", b"child");
        }
        Existing::Symlink => symlink(&sentinel, &output).unwrap(),
        Existing::Dangling => symlink(sandbox.path("missing"), &output).unwrap(),
        Existing::HardLink => fs::hard_link(&sentinel, &output).unwrap(),
        Existing::Fifo => {
            let raw = CString::new(output.as_os_str().as_bytes()).unwrap();
            // 작업 전용 이름의 합성 FIFO만 생성함.
            assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
        }
        Existing::Socket => {
            listener = Some(bind_socket(&output));
        }
    }
    let before = fs::symlink_metadata(&output).unwrap();
    conflict(
        publish(&parent, leaf, b"replacement", Limits::default()),
        11,
    );
    let after = fs::symlink_metadata(&output).unwrap();
    assert_eq!(
        (
            before.dev(),
            before.ino(),
            before.mode(),
            before.nlink(),
            before.len()
        ),
        (
            after.dev(),
            after.ino(),
            after.mode(),
            after.nlink(),
            after.len()
        )
    );
    assert_eq!(fs::read(&sentinel).unwrap(), b"untouched");
    match kind {
        Existing::Regular => assert_eq!(fs::read(&output).unwrap(), b"existing"),
        Existing::Directory => {
            assert_eq!(names(&output), vec![OsString::from("child")]);
            assert_eq!(fs::read(output.join("child")).unwrap(), b"child");
        }
        Existing::Symlink => assert_eq!(fs::read_link(&output).unwrap(), sentinel),
        Existing::Dangling => assert_eq!(fs::read_link(&output).unwrap(), sandbox.path("missing")),
        Existing::HardLink => assert_eq!(fs::read(&output).unwrap(), b"untouched"),
        Existing::Fifo | Existing::Socket => {}
    }
    assert_eq!(names(&parent), vec![leaf.to_os_string()]);
    drop(listener);
    sandbox.finish();
}

#[test]
fn existing_regular_file_preserved() {
    existing(Existing::Regular);
}
#[test]
fn existing_directory_and_child_preserved() {
    existing(Existing::Directory);
}
#[test]
fn existing_symlink_and_target_preserved() {
    existing(Existing::Symlink);
}
#[test]
fn dangling_symlink_preserved() {
    existing(Existing::Dangling);
}
#[test]
fn existing_hard_links_preserved() {
    existing(Existing::HardLink);
}
#[test]
fn existing_fifo_preserved_without_opening_it() {
    existing(Existing::Fifo);
}
#[test]
fn existing_socket_preserved_without_accepting_connections() {
    existing(Existing::Socket);
}
