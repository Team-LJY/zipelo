use crate::{fixtures::*, support::*};
use job::{Limits, Outcome, gzip};

fn rejected(archive: Vec<u8>, expected: Option<gzip::Failure>) {
    let sandbox = Sandbox::new();
    let input = sandbox.file("input.gz", &archive);
    let before = identity(&input);
    let parent = sandbox.parent();
    let (report, trace) = run(&input, &parent, Limits::default());
    match (&report.outcome, expected) {
        (Outcome::DecodeFailed(gzip::Error::Reader(actual)), Some(expected)) => {
            assert_eq!(*actual, expected)
        }
        (Outcome::DecodeFailed(gzip::Error::Backend(_)), None) => {}
        _ => panic!("원래 GZip 실패 종류가 보존되지 않음"),
    }
    trace.finish(&report);
    snapshot_usage(report.usage.snapshot, archive.len());
    assert_eq!(
        report.snapshot.as_ref().unwrap().identity().inode(),
        before.1
    );
    assert!(names(&parent).is_empty());
    assert_eq!(identity(&input), before);
    assert_eq!(std::fs::read(input).unwrap(), archive);
    sandbox.finish();
}

#[test]
fn invalid_magic_prevents_publication() {
    let mut archive = stored(b"abc");
    archive[0] = 0;
    rejected(archive, Some(gzip::Failure::Header));
}
#[test]
fn invalid_deflate_preserves_backend_error() {
    let mut archive = stored(b"abc");
    archive[10] = 7;
    rejected(archive, None);
}
#[test]
fn bad_crc_prevents_publication() {
    let mut archive = stored(b"abc");
    let index = archive.len() - 8;
    archive[index] ^= 1;
    rejected(archive, Some(gzip::Failure::Crc));
}
#[test]
fn bad_size_prevents_publication() {
    let mut archive = stored(b"abc");
    let index = archive.len() - 4;
    archive[index] ^= 1;
    rejected(archive, Some(gzip::Failure::Size));
}
#[test]
fn concatenated_members_prevent_publication() {
    let mut archive = stored(b"abc");
    archive.extend(stored(b"def"));
    rejected(archive, Some(gzip::Failure::TrailingData));
}
#[test]
fn trailing_byte_prevents_publication() {
    let mut archive = stored(b"abc");
    archive.push(0);
    rejected(archive, Some(gzip::Failure::TrailingData));
}
#[test]
fn truncated_trailer_prevents_publication() {
    let mut archive = stored(b"abc");
    archive.pop();
    rejected(archive, Some(gzip::Failure::Truncated));
}
