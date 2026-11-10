use crate::support::*;
use publication::{CancelDecision, Limits, Outcome, Publication, Request, Stage, Stop};
use std::{ffi::OsStr, fs, os::unix::fs::MetadataExt};

fn replace_parent(stage: Stage) {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let before = fs::metadata(&parent).unwrap();
    let moved = sandbox.path("moved");
    let mut opening = 0;
    let mut changed = false;
    let (task, _) = Publication::new();
    let report = task.publish(
        Request {
            parent: &parent,
            leaf: OsStr::new("output"),
            bytes: b"new-output",
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::Opening {
                opening += 1;
            }
            if !changed && point.stage == stage && (stage != Stage::Opening || opening == 2) {
                fs::rename(&parent, &moved).unwrap();
                sandbox.dir("parent");
                sandbox.file("parent/output", b"replacement-parent");
                changed = true;
            }
            Ok::<_, Stop<()>>(())
        },
    );
    assert!(changed);
    assert_eq!(
        (
            fs::metadata(&moved).unwrap().dev(),
            fs::metadata(&moved).unwrap().ino()
        ),
        (before.dev(), before.ino())
    );
    assert_eq!(
        fs::read(parent.join("output")).unwrap(),
        b"replacement-parent"
    );
    committed(report, &moved, OsStr::new("output"), b"new-output");
    sandbox.finish();
}

#[test]
fn parent_replaced_after_open_commits_to_held_directory() {
    replace_parent(Stage::Opening);
}

#[test]
fn parent_replaced_before_commit_keeps_receipt_on_old_parent() {
    replace_parent(Stage::BeforeCommit);
}

#[test]
fn destination_created_at_last_checkpoint_is_preserved() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let (task, _) = Publication::new();
    let mut inserted = false;
    let report = task.publish(
        Request {
            parent: &parent,
            leaf: OsStr::new("output"),
            bytes: b"new",
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::BeforeCommit {
                sandbox.file("parent/output", b"late-existing");
                inserted = true;
            }
            Ok::<_, Stop<()>>(())
        },
    );
    assert!(inserted);
    conflict(report, 3);
    assert_eq!(fs::read(parent.join("output")).unwrap(), b"late-existing");
    sandbox.finish();
}

#[test]
fn removed_parent_returns_uncertain_original_errno_without_receipt() {
    let sandbox = Sandbox::new();
    let parent = sandbox.parent();
    let (task, cancel) = Publication::new();
    let mut removed = false;
    let report = task.publish(
        Request {
            parent: &parent,
            leaf: OsStr::new("output"),
            bytes: b"abc",
            limits: Limits::default(),
        },
        |point| {
            if point.stage == Stage::BeforeCommit {
                assert!(names(&parent).is_empty());
                fs::remove_dir(&parent).unwrap();
                removed = true;
            }
            Ok::<_, Stop<()>>(())
        },
    );
    assert!(removed);
    match report.outcome {
        Outcome::CommitUncertain { source } => {
            assert_eq!(source.raw_os_error(), Some(libc::ENOENT))
        }
        Outcome::NotCommitted(error) => panic!("uncertainty erased: {error:?}"),
        Outcome::Committed { .. } => panic!("unexpected receipt"),
    }
    full_usage(report.usage, 3, false);
    cleanup(&report.cleanup, 4);
    assert_eq!(cancel.request(), CancelDecision::Finished);
    assert!(!parent.exists());
    assert!(names(&sandbox.root).is_empty());
    sandbox.finish();
}
