use crate::support::*;
use snapshot::{Changed, Error, Limits, Request, Stage, Stop, Unsupported, capture};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::MetadataExt,
};

fn alter(
    stage: Stage,
    offset: u64,
    action: impl FnOnce(&std::path::Path),
) -> snapshot::Failure<()> {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", &pattern(32));
    let mut action = Some(action);
    let result = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |point| {
            if point.stage == stage && point.offset == offset {
                if let Some(action) = action.take() {
                    action(&path);
                }
            }
            Ok::<_, Stop<()>>(())
        },
    )
    .unwrap_err();
    assert!(action.is_none());
    sandbox.finish();
    result
}

fn append(path: &std::path::Path) {
    OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(b"x")
        .unwrap();
}

fn truncate(path: &std::path::Path) {
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(0)
        .unwrap();
}

#[test]
fn append_after_pinning_detected_before_data_read() {
    let usage = changed(alter(Stage::Pinned, 0, append), Changed::Length);
    assert_eq!(usage.read_calls, 0);
}

#[test]
fn truncate_after_pinning_detected_before_data_read() {
    let usage = changed(alter(Stage::Pinned, 0, truncate), Changed::Length);
    assert_eq!(usage.read_calls, 0);
}

#[test]
fn append_after_copy_detected_by_first_eof_probe() {
    let usage = changed(alter(Stage::Copying, 32, append), Changed::Length);
    assert_eq!(usage.read_calls, 2);
    assert_eq!(usage.actual_read_bytes, 33);
    assert_eq!(usage.compared_bytes, 0);
}

#[test]
fn truncate_before_copy_returns_early_eof() {
    let usage = changed(alter(Stage::Copying, 0, truncate), Changed::Length);
    assert_eq!(usage.read_calls, 1);
    assert_eq!(usage.actual_read_bytes, 0);
    assert_eq!(usage.reserved_read_bytes, 32);
}

#[test]
fn positive_short_read_is_accumulated_before_later_eof() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", &pattern(65_537));
    let mut altered = false;
    let mut saw_short = false;
    let failure = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::Copying && !altered {
                OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_len(1)
                    .unwrap();
                altered = true;
            }
            if point.stage == Stage::Copying && point.offset == 1 {
                assert_eq!(point.usage.actual_read_bytes, 1);
                saw_short = true;
            }
            Ok::<_, Stop<()>>(())
        },
    )
    .unwrap_err();
    let usage = changed(failure, Changed::Length);
    assert!(altered && saw_short);
    assert_eq!(usage.read_calls, 2);
    assert_eq!(usage.reserved_read_bytes, 131_072);
    assert_eq!(usage.actual_read_bytes, 1);
    sandbox.finish();
}

#[test]
fn same_size_overwrite_before_comparison_returns_content_change() {
    let usage = changed(
        alter(Stage::Comparing, 0, |path| {
            fs::write(path, vec![99; 32]).unwrap()
        }),
        Changed::Contents,
    );
    assert_eq!(usage.actual_read_bytes, 64);
    assert_eq!(usage.compared_bytes, 32);
}

#[test]
fn truncate_before_comparison_returns_early_eof() {
    let usage = changed(alter(Stage::Comparing, 0, truncate), Changed::Length);
    assert_eq!(usage.actual_read_bytes, 32);
    assert_eq!(usage.compared_bytes, 0);
    assert_eq!(usage.read_calls, 3);
}

#[test]
fn append_after_comparison_detected_by_second_eof_probe() {
    let usage = changed(alter(Stage::Comparing, 32, append), Changed::Length);
    assert_eq!(usage.actual_read_bytes, 65);
    assert_eq!(usage.compared_bytes, 32);
    assert_eq!(usage.read_calls, 4);
}

#[test]
fn finalizing_append_detected_by_final_metadata() {
    let usage = changed(alter(Stage::Finalizing, 32, append), Changed::Length);
    assert_eq!(usage.actual_read_bytes, 64);
    assert_eq!(usage.metadata_calls, 5);
}

#[test]
fn hard_link_added_at_finalizing_rejected() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"abc");
    let failure = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::Finalizing {
                fs::hard_link(&path, sandbox.path("alias")).unwrap();
            }
            Ok::<_, Stop<()>>(())
        },
    )
    .unwrap_err();
    assert!(matches!(
        failure.cause,
        Error::Unsupported(Unsupported::MultipleLinks)
    ));
    assert_eq!(failure.usage.actual_read_bytes, 6);
    sandbox.finish();
}

#[test]
fn replaced_leaf_never_captures_replacement_bytes() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"old");
    let replacement = sandbox.file("replacement", b"new");
    let failure = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::Pinned {
                fs::rename(&replacement, &path).unwrap();
            }
            Ok::<_, Stop<()>>(())
        },
    )
    .unwrap_err();
    assert!(matches!(
        failure.cause,
        Error::Unsupported(Unsupported::MultipleLinks)
    ));
    assert_eq!(failure.usage.read_calls, 0);
    assert_eq!(fs::read(&path).unwrap(), b"new");
    sandbox.finish();
}

#[test]
fn replaced_parent_keeps_pinned_inode_not_new_path_contents() {
    let sandbox = Sandbox::new();
    let parent = sandbox.dir("parent");
    let path = sandbox.file("parent/input", b"old");
    let original = fs::metadata(&path).unwrap();
    let moved = sandbox.path("moved");
    let result = capture(
        Request {
            path: &path,
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::Pinned {
                fs::rename(&parent, &moved).unwrap();
                sandbox.dir("parent");
                sandbox.file("parent/input", b"new");
            }
            Ok::<_, Stop<()>>(())
        },
    )
    .unwrap();
    assert_eq!(result.bytes(), b"old");
    assert_eq!(result.identity().inode(), original.ino());
    assert_eq!(result.identity().device(), original.dev());
    assert_eq!(fs::read(&path).unwrap(), b"new");
    assert_eq!(fs::read(moved.join("input")).unwrap(), b"old");
    sandbox.finish();
}

#[test]
fn returned_snapshot_owns_bytes_after_source_changes() {
    let sandbox = Sandbox::new();
    let path = sandbox.file("input", b"old");
    let result = get(&path, Limits::default()).unwrap();
    let digest = result.sha256();
    fs::write(&path, b"new").unwrap();
    assert_eq!(result.bytes(), b"old");
    assert_eq!(result.sha256(), digest);
    assert_eq!(result.into_bytes(), b"old");
    sandbox.finish();
}
