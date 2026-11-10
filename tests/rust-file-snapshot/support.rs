use snapshot::{Error, Failure, FileSnapshot, Limits, Request, Stop, capture};
use std::{
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
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("runs");
        let metadata = fs::symlink_metadata(&parent).expect("runner가 runs 디렉터리를 준비해야 함");
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(metadata.mode() & 0o777, 0o700);
        assert_eq!(parent.canonicalize().unwrap(), parent);
        let root = parent.join(format!(
            "case-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let metadata = fs::symlink_metadata(&root).unwrap();
        Self {
            root,
            identity: (metadata.dev(), metadata.ino()),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
        path
    }

    pub fn dir(&self, name: &str) -> PathBuf {
        let path = self.path(name);
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        path
    }

    pub fn finish(self) {
        let metadata = fs::symlink_metadata(&self.root).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!((metadata.dev(), metadata.ino()), self.identity);
        fs::remove_dir_all(&self.root).unwrap();
        assert!(!self.root.exists());
    }
}

pub fn pattern(length: usize) -> Vec<u8> {
    (0..length).map(|i| ((i * 37 + 11) % 251) as u8).collect()
}

pub fn get(path: &Path, limits: Limits) -> Result<FileSnapshot, Failure<()>> {
    capture(Request { path, limits }, |_| Ok::<_, Stop<()>>(()))
}

pub fn expect_usage(snapshot: &FileSnapshot, length: usize) {
    let usage = snapshot.usage();
    assert_eq!(usage.open_calls, 4);
    assert_eq!(usage.metadata_calls, 5);
    assert_eq!(usage.read_calls, 2 * (length as u64).div_ceil(65_536) + 2);
    assert_eq!(usage.reserved_read_bytes, 2 * length as u64 + 2);
    assert_eq!(usage.actual_read_bytes, 2 * length as u64);
    assert_eq!(usage.compared_bytes, length as u64);
}

pub fn changed(failure: Failure<()>, expected: snapshot::Changed) -> snapshot::Usage {
    assert!(matches!(failure.cause, Error::Changed(value) if value == expected));
    failure.usage
}
