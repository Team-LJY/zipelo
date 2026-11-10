use publication::{
    Cleanup, Durability, Error, Limits, Outcome, Publication, Report, Request, Stop, Usage,
};
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
        let sandbox = Self {
            root,
            identity: (info.dev(), info.ino()),
        };
        sandbox.dir("parent");
        sandbox
    }

    pub fn path(&self, name: impl AsRef<Path>) -> PathBuf {
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
            "작업 디렉터리의 열린 fd가 남아 있음"
        );
        let info = fs::symlink_metadata(&self.root).unwrap();
        assert!(info.is_dir() && !info.file_type().is_symlink());
        assert_eq!((info.dev(), info.ino()), self.identity);
        fs::remove_dir_all(&self.root).unwrap();
        assert!(!self.root.exists());
    }
}

pub fn pattern(length: usize) -> Vec<u8> {
    (0..length).map(|i| ((i * 37 + 11) % 251) as u8).collect()
}

pub fn names(parent: &Path) -> Vec<OsString> {
    let mut result: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    result.sort();
    result
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

pub fn publish(parent: &Path, leaf: &OsStr, bytes: &[u8], limits: Limits) -> Report<()> {
    let (task, cancel) = Publication::new();
    let report = task.publish(
        Request {
            parent,
            leaf,
            bytes,
            limits,
        },
        |_| Ok::<_, Stop<()>>(()),
    );
    assert_eq!(cancel.request(), publication::CancelDecision::Finished);
    report
}

pub fn not_committed<E>(report: Report<E>) -> (Error<E>, Usage, Cleanup) {
    let Report {
        outcome,
        usage,
        cleanup,
    } = report;
    match outcome {
        Outcome::NotCommitted(error) => (error, usage, cleanup),
        Outcome::CommitUncertain { source } => panic!("unexpected uncertain commit: {source}"),
        Outcome::Committed { .. } => panic!("unexpected committed result"),
    }
}

pub fn cleanup(cleanup: &Cleanup, attempts: u8) {
    assert_eq!(cleanup.attempted, attempts);
    assert!(cleanup.all_close_calls_succeeded(), "{cleanup:?}");
    assert!(cleanup.failures.iter().all(Option::is_none));
}

pub fn full_usage(usage: Usage, length: usize, linked: bool) {
    let chunks = (length as u64).div_ceil(65_536);
    assert_eq!(usage.open_calls, 4);
    assert_eq!(usage.metadata_calls, 5);
    assert_eq!(usage.write_calls, chunks);
    assert_eq!(usage.read_calls, chunks + 1);
    assert_eq!(usage.actual_written_bytes, length as u64);
    assert_eq!(usage.actual_read_bytes, length as u64);
    assert_eq!(usage.reserved_io_bytes, 2 * length as u64 + 1);
    assert_eq!(usage.link_calls, 1);
    assert_eq!(usage.sync_calls, if linked { 2 } else { 1 });
}

pub fn committed(report: Report<()>, parent: &Path, leaf: &OsStr, bytes: &[u8]) {
    cleanup(&report.cleanup, 4);
    full_usage(report.usage, bytes.len(), true);
    let (receipt, durability) = match report.outcome {
        Outcome::Committed {
            receipt,
            durability,
        } => (receipt, durability),
        Outcome::NotCommitted(error) => panic!("unexpected noncommit: {error:?}"),
        Outcome::CommitUncertain { source } => panic!("unexpected uncertain commit: {source}"),
    };
    assert!(matches!(durability, Durability::FileAndParentSynced));
    let parent_meta = fs::metadata(parent).unwrap();
    let path = parent.join(leaf);
    let file_meta = fs::symlink_metadata(&path).unwrap();
    assert!(file_meta.is_file() && !file_meta.file_type().is_symlink());
    assert_eq!(file_meta.nlink(), 1);
    assert_eq!(file_meta.mode() & 0o077, 0);
    assert_eq!(file_meta.uid(), unsafe { libc::geteuid() });
    assert_eq!(receipt.length(), bytes.len() as u64);
    assert_eq!(receipt.parent_identity().device(), parent_meta.dev());
    assert_eq!(receipt.parent_identity().inode(), parent_meta.ino());
    assert_eq!(receipt.file_identity().device(), file_meta.dev());
    assert_eq!(receipt.file_identity().inode(), file_meta.ino());
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert_eq!(names(parent), vec![leaf.to_os_string()]);
}

pub fn conflict(report: Report<()>, length: usize) {
    let (error, usage, closed) = not_committed(report);
    match error {
        Error::Io { operation, source } => {
            assert_eq!(operation, "linkat_conflict");
            assert_eq!(source.raw_os_error(), Some(libc::EEXIST));
        }
        other => panic!("unexpected: {other:?}"),
    }
    full_usage(usage, length, false);
    cleanup(&closed, 4);
}
