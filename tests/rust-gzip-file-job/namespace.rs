use crate::{fixtures::*, support::*};
use job::{Event, FileJob, Limits, Outcome, Stop, publication, snapshot};
use std::{ffi::OsStr, fs, os::unix::fs::symlink};

#[test]
fn existing_file_bytes_and_identity_survive_conflict() {
    let sandbox = Sandbox::new();
    let input = sandbox.file("input.gz", &stored(b"abc"));
    let existing = sandbox.file("parent/output", b"preserve");
    let before = identity(&existing);
    let (report, _) = run(&input, &sandbox.parent(), Limits::default());
    conflict(&report, 3);
    assert_eq!(fs::read(&existing).unwrap(), b"preserve");
    assert_eq!(identity(&existing), before);
    assert_eq!(
        names(&sandbox.parent()),
        vec![std::ffi::OsString::from("output")]
    );
    sandbox.finish();
}

#[test]
fn existing_directory_and_child_survive_conflict() {
    let sandbox = Sandbox::new();
    let input = sandbox.file("input.gz", &stored(b"abc"));
    let existing = sandbox.dir("parent/output");
    let child = sandbox.file("parent/output/child", b"preserve");
    let before = identity(&existing);
    let child_before = identity(&child);
    let (report, _) = run(&input, &sandbox.parent(), Limits::default());
    conflict(&report, 3);
    assert_eq!(identity(&existing), before);
    assert_eq!(identity(&child), child_before);
    assert_eq!(fs::read(child).unwrap(), b"preserve");
    assert_eq!(names(&existing), vec![std::ffi::OsString::from("child")]);
    sandbox.finish();
}

#[test]
fn existing_symlink_and_target_survive_conflict() {
    let sandbox = Sandbox::new();
    let input = sandbox.file("input.gz", &stored(b"abc"));
    let victim = sandbox.file("victim", b"preserve");
    let existing = sandbox.parent().join("output");
    symlink(&victim, &existing).unwrap();
    let before = identity(&existing);
    let victim_before = identity(&victim);
    let (report, _) = run(&input, &sandbox.parent(), Limits::default());
    conflict(&report, 3);
    assert_eq!(identity(&existing), before);
    assert_eq!(identity(&victim), victim_before);
    assert_eq!(fs::read_link(existing).unwrap(), victim);
    assert_eq!(fs::read(victim).unwrap(), b"preserve");
    sandbox.finish();
}

#[test]
fn identical_input_and_output_name_preserves_archive() {
    let sandbox = Sandbox::new();
    let archive = stored(b"abc");
    let input = sandbox.file("input.gz", &archive);
    let before = identity(&input);
    let (task, cancel) = FileJob::new();
    let mut trace = Trace::default();
    let report = task.run(
        job::Request {
            input: &input,
            output_parent: &sandbox.root,
            output_leaf: OsStr::new("input.gz"),
            limits: Limits::default(),
        },
        |point| {
            trace.push(point);
            Ok::<_, Stop<()>>(())
        },
    );
    trace.finish(&report);
    conflict(&report, 3);
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    assert_eq!(identity(&input), before);
    assert_eq!(fs::read(input).unwrap(), archive);
    sandbox.finish();
}

#[test]
fn missing_owned_input_preserves_native_open_error() {
    let sandbox = Sandbox::new();
    let (report, trace) = run(
        &sandbox.path("missing.gz"),
        &sandbox.parent(),
        Limits::default(),
    );
    match &report.outcome {
        Outcome::SnapshotFailed(snapshot::Failure {
            cause: snapshot::Error::Io { operation, source },
            usage,
        }) => {
            assert_eq!(*operation, snapshot::IoOperation::PinPath);
            assert_eq!(source.raw_os_error(), Some(libc::ENOENT));
            assert_eq!(
                *usage,
                snapshot::Usage {
                    open_calls: 1,
                    ..snapshot::Usage::default()
                }
            );
        }
        _ => panic!("원래 입력 open 오류가 아님"),
    }
    trace.finish(&report);
    assert!(names(&sandbox.parent()).is_empty());
    sandbox.finish();
}

#[test]
fn replaced_parent_path_does_not_redirect_pinned_commit() {
    let sandbox = Sandbox::new();
    let input = sandbox.file("input.gz", &stored(b"abc"));
    let parent = sandbox.parent();
    let (task, cancel) = FileJob::new();
    let mut trace = Trace::default();
    let mut replaced = false;
    let report = task.run(request(&input, &parent, Limits::default()), |point| {
        trace.push(point);
        if matches!(point.event, Event::Publication(native) if native.stage == publication::Stage::BeforeCommit) {
            fs::rename(&parent, sandbox.path("old-parent")).unwrap();
            sandbox.dir("parent");
            replaced = true;
        }
        Ok::<_, Stop<()>>(())
    });
    assert!(replaced);
    trace.finish(&report);
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    committed(&report, &sandbox.path("old-parent"), b"abc");
    assert!(names(&parent).is_empty());
    sandbox.finish();
}
