use crate::fixtures::crc32;
use job::{
    Checkpoint, Event, FileJob, Limits, Outcome, Report, Request, Stop, Usage, gzip, publication,
    snapshot,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);

pub struct Sandbox {
    pub root: PathBuf,
    identity: (u64, u64),
}

impl Sandbox {
    pub fn new() -> Self {
        let runs = Path::new(env!("CARGO_MANIFEST_DIR")).join("runs");
        let info = fs::symlink_metadata(&runs).expect("runner가 전용 runs를 준비해야 함");
        assert!(info.is_dir() && !info.file_type().is_symlink());
        assert_eq!(info.mode() & 0o777, 0o700);
        assert_eq!(info.uid(), unsafe { libc::geteuid() });
        assert_eq!(runs.canonicalize().unwrap(), runs);
        let root = runs.join(format!(
            "case-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let info = fs::symlink_metadata(&root).unwrap();
        let result = Self {
            root,
            identity: (info.dev(), info.ino()),
        };
        result.dir("parent");
        result
    }

    pub fn path(&self, name: impl AsRef<Path>) -> PathBuf {
        let name = name.as_ref();
        assert!(
            name.components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        );
        self.root.join(name)
    }

    pub fn parent(&self) -> PathBuf {
        self.path("parent")
    }

    pub fn dir(&self, name: impl AsRef<Path>) -> PathBuf {
        let path = self.path(name);
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        path
    }

    pub fn file(&self, name: impl AsRef<Path>, bytes: &[u8]) -> PathBuf {
        let path = self.path(name);
        let mut handle = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        handle.write_all(bytes).unwrap();
        handle.sync_all().unwrap();
        path
    }

    pub fn finish(self) {
        assert!(
            owned_descriptors(&self.root).is_empty(),
            "소유 fd가 남아 있음"
        );
        let info = fs::symlink_metadata(&self.root).unwrap();
        assert!(info.is_dir() && !info.file_type().is_symlink());
        assert_eq!((info.dev(), info.ino()), self.identity);
        fs::remove_dir_all(&self.root).unwrap();
        assert!(!self.root.exists());
    }
}

pub fn owned_descriptors(root: &Path) -> Vec<u32> {
    let mut result = Vec::new();
    for item in fs::read_dir("/proc/self/fd").unwrap() {
        let item = item.unwrap();
        if let Ok(target) = fs::read_link(item.path()) {
            if target.starts_with(root) {
                result.push(item.file_name().to_str().unwrap().parse().unwrap());
            }
        }
    }
    result.sort();
    result
}

pub fn names(parent: &Path) -> Vec<OsString> {
    let mut result: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    result.sort();
    result
}

pub fn identity(path: &Path) -> (u64, u64, u64, u32, i64, i64, i64, i64) {
    let m = fs::symlink_metadata(path).unwrap();
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mode(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}

pub fn request<'a>(input: &'a Path, parent: &'a Path, limits: Limits) -> Request<'a> {
    Request {
        input,
        output_parent: parent,
        output_leaf: OsStr::new("output"),
        limits,
    }
}

#[derive(Default)]
pub struct Trace {
    pub points: Vec<Checkpoint>,
}

fn counters(usage: Usage) -> [u64; 18] {
    let s = usage.snapshot;
    let g = usage.gzip;
    let p = usage.publication;
    [
        s.open_calls,
        s.metadata_calls,
        s.read_calls,
        s.reserved_read_bytes,
        s.actual_read_bytes,
        s.compared_bytes,
        g.processed,
        g.output,
        g.metadata,
        p.open_calls,
        p.metadata_calls,
        p.sync_calls,
        p.link_calls,
        p.read_calls,
        p.write_calls,
        p.reserved_io_bytes,
        p.actual_read_bytes,
        p.actual_written_bytes,
    ]
}

pub fn phase(event: Event) -> u8 {
    match event {
        Event::Snapshot(_) => 0,
        Event::Decode(_) => 1,
        Event::Publication(_) => 2,
    }
}

impl Trace {
    pub fn push(&mut self, point: Checkpoint) {
        if let Some(before) = self.points.last() {
            assert!(phase(before.event) <= phase(point.event));
            for (a, b) in counters(before.usage)
                .into_iter()
                .zip(counters(point.usage))
            {
                assert!(a <= b);
            }
        }
        match point.event {
            Event::Snapshot(native) => {
                assert_eq!(point.usage.snapshot, native.usage);
                assert_eq!(point.usage.gzip, gzip::Usage::default());
                assert_eq!(point.usage.publication, publication::Usage::default());
            }
            Event::Decode(native) => {
                assert_eq!(point.usage.gzip, native.usage);
                assert_eq!(point.usage.publication, publication::Usage::default());
            }
            Event::Publication(native) => {
                assert_eq!(point.usage.publication, native.usage);
                assert_eq!(native.usage.link_calls, 0);
            }
        }
        self.points.push(point);
    }

    pub fn finish<E>(&self, report: &Report<E>) {
        if let Some(last) = self.points.last() {
            for (a, b) in counters(last.usage).into_iter().zip(counters(report.usage)) {
                assert!(a <= b);
            }
        }
        match &report.outcome {
            Outcome::SnapshotFailed(failure) => {
                assert_eq!(report.usage.snapshot, failure.usage);
                assert_eq!(report.usage.gzip, gzip::Usage::default());
                assert_eq!(report.usage.publication, publication::Usage::default());
                assert!(report.snapshot.is_none() && report.decoded.is_none());
                assert!(self.points.iter().all(|p| phase(p.event) == 0));
            }
            Outcome::DecodeFailed(_) => {
                assert!(report.snapshot.is_some() && report.decoded.is_none());
                assert_eq!(report.usage.publication, publication::Usage::default());
                assert!(self.points.iter().all(|p| phase(p.event) <= 1));
            }
            Outcome::Publication(native) => {
                assert!(report.snapshot.is_some() && report.decoded.is_some());
                assert_eq!(report.usage.publication, native.usage);
            }
            Outcome::AdapterFailed(_) => panic!("정상 decoder에서 adapter 실패가 발생함"),
        }
    }
}

pub fn run(input: &Path, parent: &Path, limits: Limits) -> (Report<()>, Trace) {
    let (task, cancel) = FileJob::new();
    let mut trace = Trace::default();
    let report = task.run(request(input, parent, limits), |point| {
        trace.push(point);
        Ok::<_, Stop<()>>(())
    });
    trace.finish(&report);
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    (report, trace)
}

pub fn snapshot_usage(usage: snapshot::Usage, length: usize) {
    assert_eq!(
        usage,
        snapshot::Usage {
            open_calls: 4,
            metadata_calls: 5,
            read_calls: 2 * (length as u64).div_ceil(65_536) + 2,
            reserved_read_bytes: 2 * length as u64 + 2,
            actual_read_bytes: 2 * length as u64,
            compared_bytes: length as u64,
        }
    );
}

pub fn gzip_usage(usage: gzip::Usage, archive: &[u8], output: &[u8], header: usize) {
    assert_eq!(
        usage,
        gzip::Usage {
            processed: (archive.len() + output.len()) as u64,
            output: output.len() as u64,
            metadata: header as u64,
        }
    );
}

pub fn cleanup(value: &publication::Cleanup, attempts: u8) {
    assert_eq!(value.attempted, attempts);
    assert!(value.all_close_calls_succeeded());
    assert!(value.failures.iter().all(Option::is_none));
}

pub fn publication_usage(usage: publication::Usage, length: usize, linked: bool) {
    let chunks = (length as u64).div_ceil(65_536);
    assert_eq!(
        usage,
        publication::Usage {
            open_calls: 4,
            metadata_calls: 5,
            sync_calls: if linked { 2 } else { 1 },
            link_calls: 1,
            read_calls: chunks + 1,
            write_calls: chunks,
            reserved_io_bytes: 2 * length as u64 + 1,
            actual_read_bytes: length as u64,
            actual_written_bytes: length as u64,
        }
    );
}

pub fn observations<E>(
    report: &Report<E>,
    source_id: (u64, u64),
    archive: &[u8],
    output: &[u8],
    header: usize,
) {
    let captured = report.snapshot.as_ref().expect("snapshot 관측값 누락");
    assert_eq!(
        (captured.identity().device(), captured.identity().inode()),
        source_id
    );
    assert_eq!(captured.identity().length(), archive.len() as u64);
    assert_eq!(captured.sha256(), <[u8; 32]>::from(Sha256::digest(archive)));
    let boundary = report
        .decoded
        .as_ref()
        .expect("decode 관측값 누락")
        .boundary();
    assert_eq!(boundary.header_bytes, header);
    assert_eq!(boundary.member_bytes, archive.len());
    assert_eq!(boundary.deflate_bytes as usize, archive.len() - header - 8);
    assert_eq!(boundary.inflated_bytes, output.len() as u64);
    assert_eq!(boundary.crc32, crc32(output));
    snapshot_usage(report.usage.snapshot, archive.len());
    gzip_usage(report.usage.gzip, archive, output, header);
}

pub fn committed<E>(report: &Report<E>, parent: &Path, output: &[u8]) {
    let native = match &report.outcome {
        Outcome::Publication(native) => native,
        _ => panic!("게시 보고서 누락"),
    };
    assert_eq!(native.usage, report.usage.publication);
    publication_usage(native.usage, output.len(), true);
    cleanup(&native.cleanup, 4);
    let (receipt, durability) = match &native.outcome {
        publication::Outcome::Committed {
            receipt,
            durability,
        } => (receipt, durability),
        _ => panic!("확정된 게시가 아님"),
    };
    assert!(matches!(
        durability,
        publication::Durability::FileAndParentSynced
    ));
    let m = fs::symlink_metadata(parent.join("output")).unwrap();
    let p = fs::metadata(parent).unwrap();
    assert!(m.is_file() && !m.file_type().is_symlink());
    assert_eq!(m.nlink(), 1);
    assert_eq!(m.mode() & 0o777, 0o600);
    assert_eq!(receipt.length(), output.len() as u64);
    assert_eq!(
        (
            receipt.parent_identity().device(),
            receipt.parent_identity().inode()
        ),
        (p.dev(), p.ino())
    );
    assert_eq!(
        (
            receipt.file_identity().device(),
            receipt.file_identity().inode()
        ),
        (m.dev(), m.ino())
    );
    assert_eq!(fs::read(parent.join("output")).unwrap(), output);
    assert_eq!(names(parent), vec![OsString::from("output")]);
}

pub fn conflict<E>(report: &Report<E>, length: usize) {
    let native = match &report.outcome {
        Outcome::Publication(native) => native,
        _ => panic!("게시 보고서 누락"),
    };
    match &native.outcome {
        publication::Outcome::NotCommitted(publication::Error::Io { operation, source }) => {
            assert_eq!(*operation, "linkat_conflict");
            assert_eq!(source.raw_os_error(), Some(libc::EEXIST));
        }
        _ => panic!("명확한 충돌 오류가 아님"),
    }
    assert_eq!(report.usage.publication, native.usage);
    publication_usage(native.usage, length, false);
    cleanup(&native.cleanup, 4);
}
