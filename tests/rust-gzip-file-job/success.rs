use crate::{fixtures::*, support::*};
use job::{Event, FileJob, Limits, Stop, gzip, publication};
use std::fs;

fn boundary(length: usize) {
    let sandbox = Sandbox::new();
    let bytes = pattern(length);
    let archive = stored(&bytes);
    let input = sandbox.file("input.gz", &archive);
    let before = identity(&input);
    let parent = sandbox.parent();
    let (report, trace) = run(&input, &parent, Limits::default());
    observations(&report, (before.0, before.1), &archive, &bytes, 10);
    committed(&report, &parent, &bytes);
    assert_eq!(identity(&input), before);
    assert_eq!(fs::read(&input).unwrap(), archive);
    assert!(
        matches!(trace.points.last().unwrap().event, Event::Publication(point) if point.stage == publication::Stage::BeforeCommit)
    );
    assert!(owned_descriptors(&sandbox.root).is_empty());
    sandbox.finish();
}

#[test]
fn empty_output() {
    boundary(0);
}
#[test]
fn one_byte_output() {
    boundary(1);
}
#[test]
fn known_bytes_output() {
    boundary(3);
}
#[test]
fn chunk_minus_one_output() {
    boundary(65_535);
}
#[test]
fn chunk_output() {
    boundary(65_536);
}
#[test]
fn chunk_plus_one_output() {
    boundary(65_537);
}

#[test]
fn header_name_and_comment_never_select_destination() {
    let sandbox = Sandbox::new();
    let bytes = b"abc";
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    let archive = named(bytes);
    let header = 10 + b"../header-name\0../comment-name\0".len();
    let input = sandbox.file("input.gz", &archive);
    let before = identity(&input);
    let parent = sandbox.parent();
    let (report, _) = run(&input, &parent, Limits::default());
    observations(&report, (before.0, before.1), &archive, bytes, header);
    committed(&report, &parent, bytes);
    assert_eq!(
        names(&sandbox.root),
        vec![
            std::ffi::OsString::from("input.gz"),
            std::ffi::OsString::from("parent")
        ]
    );
    assert_eq!(identity(&input), before);
    sandbox.finish();
}

#[test]
fn replacing_input_after_snapshot_uses_captured_owned_bytes() {
    let sandbox = Sandbox::new();
    let archive = stored(b"original");
    let replacement = stored(b"replacement");
    let input = sandbox.file("input.gz", &archive);
    let before = identity(&input);
    let parent = sandbox.parent();
    let (task, cancel) = FileJob::new();
    let mut changed = false;
    let mut trace = Trace::default();
    let report = task.run(request(&input, &parent, Limits::default()), |point| {
        trace.push(point);
        if matches!(point.event, Event::Decode(native) if native.stage == gzip::Stage::Start) {
            fs::rename(&input, sandbox.path("old.gz")).unwrap();
            sandbox.file("input.gz", &replacement);
            changed = true;
        }
        Ok::<_, Stop<()>>(())
    });
    assert!(changed);
    trace.finish(&report);
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    observations(&report, (before.0, before.1), &archive, b"original", 10);
    committed(&report, &parent, b"original");
    assert_eq!(fs::read(&input).unwrap(), replacement);
    assert_eq!(fs::read(sandbox.path("old.gz")).unwrap(), archive);
    assert_ne!(
        (identity(&input).0, identity(&input).1),
        (before.0, before.1)
    );
    sandbox.finish();
}
