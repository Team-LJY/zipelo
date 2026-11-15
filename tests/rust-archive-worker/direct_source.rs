use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Source(std::path::PathBuf);
impl Source {
    fn new() -> Self {
        let root = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/work/tmp")
            });
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "direct-source-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("input"), vec![42; 1024 * 1024]).unwrap();
        Self(path)
    }
    fn path(&self) -> std::path::PathBuf {
        self.0.join("input")
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn direct_source_matches_copied_identity_length_and_hash() {
    let source = Source::new();
    let (direct, mut guard) =
        Snapshot::open_direct(&source.path(), u64::MAX, &mut |_| Ok(())).unwrap();
    let copied = Snapshot::open_reporting(&source.path(), u64::MAX, &mut |_| Ok(())).unwrap();
    assert_eq!(direct.object, copied.object);
    assert_eq!(direct.len, copied.len);
    assert_eq!(direct.hash, copied.hash);
    guard.verify(&mut |_| Ok(())).unwrap();
}

#[cfg(unix)]
#[test]
fn direct_source_detects_same_size_and_restored_mtime_changes() {
    let source = Source::new();
    let (_, mut guard) = Snapshot::open_direct(&source.path(), u64::MAX, &mut |_| Ok(())).unwrap();
    let modified = std::fs::metadata(source.path())
        .unwrap()
        .modified()
        .unwrap();
    let mut file = OpenOptions::new().write(true).open(source.path()).unwrap();
    file.seek(SeekFrom::Start(512)).unwrap();
    file.write_all(b"x").unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert!(matches!(
        guard.verify(&mut |_| Ok(())),
        Err(ErrorCode::SourceChanged)
    ));
}

#[cfg(unix)]
#[test]
fn direct_source_detects_named_replacement_even_when_open_handle_survives() {
    let source = Source::new();
    let (_, mut guard) = Snapshot::open_direct(&source.path(), u64::MAX, &mut |_| Ok(())).unwrap();
    std::fs::rename(source.path(), source.0.join("old")).unwrap();
    std::fs::write(source.path(), vec![42; 1024 * 1024]).unwrap();
    assert!(matches!(
        guard.verify(&mut |_| Ok(())),
        Err(ErrorCode::SourceChanged)
    ));
}

#[cfg(windows)]
#[test]
fn direct_source_allows_existing_readers_and_blocks_writers_until_closed() {
    use std::os::windows::fs::OpenOptionsExt;
    let source = Source::new();
    let reader = OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(source.path())
        .unwrap();
    let (snapshot, guard) =
        Snapshot::open_direct(&source.path(), u64::MAX, &mut |_| Ok(())).unwrap();
    assert!(OpenOptions::new().read(true).open(source.path()).is_ok());
    assert!(OpenOptions::new().write(true).open(source.path()).is_err());
    drop((snapshot, guard, reader));
    assert!(OpenOptions::new().write(true).open(source.path()).is_ok());
}

#[cfg(unix)]
#[test]
fn direct_source_detects_transient_change_even_after_bytes_and_mtime_are_restored() {
    let source = Source::new();
    let (_, mut guard) = Snapshot::open_direct(&source.path(), u64::MAX, &mut |_| Ok(())).unwrap();
    let modified = std::fs::metadata(source.path())
        .unwrap()
        .modified()
        .unwrap();
    let mut file = OpenOptions::new().write(true).open(source.path()).unwrap();
    file.write_all(b"x").unwrap();
    file.rewind().unwrap();
    file.write_all(b"*").unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert!(matches!(
        guard.verify(&mut |_| Ok(())),
        Err(ErrorCode::SourceChanged)
    ));
}
