use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/rust-archive-worker/runs");
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "sources-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn source(&self, name: &str) -> Source {
        Source {
            path: NativePath::from_path(&self.0.join(name)),
            archive_name: name.into(),
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn recursive_counts_and_source_bytes_honor_exact_quotas() {
    let s = Scratch::new();
    fs::create_dir(s.0.join("folder")).unwrap();
    fs::create_dir(s.0.join("folder/empty")).unwrap();
    fs::write(s.0.join("folder/file"), b"abc").unwrap();
    for (entries, output, expected) in [
        (3, 3, None),
        (2, 3, Some(ErrorCode::Limit)),
        (3, 2, Some(ErrorCode::Limit)),
    ] {
        let result = Plan::new(
            &[s.source("folder")],
            Limits {
                entries,
                output,
                ..Limits::default()
            },
            &mut || Ok(()),
        );
        assert_eq!(result.as_ref().err().copied(), expected);
        if let Ok(plan) = result {
            assert_eq!(plan.items.len(), 3);
        }
    }
}
#[test]
fn replaced_source_is_rejected_before_capture() {
    let s = Scratch::new();
    fs::write(s.0.join("file"), b"original").unwrap();
    let plan = Plan::new(&[s.source("file")], Limits::default(), &mut || Ok(())).unwrap();
    fs::rename(s.0.join("file"), s.0.join("previous")).unwrap();
    fs::write(s.0.join("file"), b"different").unwrap();
    assert!(matches!(
        plan.items[0].open(),
        Err(ErrorCode::SourceChanged)
    ));
}
#[test]
fn cancellation_stops_directory_enumeration() {
    let s = Scratch::new();
    fs::create_dir(s.0.join("folder")).unwrap();
    for n in 0..20 {
        fs::write(s.0.join(format!("folder/{n}")), b"x").unwrap();
    }
    let mut calls = 0;
    let result = Plan::new(&[s.source("folder")], Limits::default(), &mut || {
        calls += 1;
        if calls == 5 {
            Err(ErrorCode::None)
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(ErrorCode::None)));
}
#[cfg(target_os = "linux")]
#[test]
fn pinned_directory_enumeration_does_not_follow_replacement_link() {
    let s = Scratch::new();
    fs::create_dir(s.0.join("folder")).unwrap();
    fs::create_dir(s.0.join("outside")).unwrap();
    fs::write(s.0.join("folder/original"), b"original").unwrap();
    fs::write(s.0.join("outside/secret"), b"secret").unwrap();
    let path = s.0.join("folder");
    let input = Input::open(&path).unwrap();
    fs::rename(&path, s.0.join("moved")).unwrap();
    std::os::unix::fs::symlink(s.0.join("outside"), &path).unwrap();
    let names = input
        .children(&path)
        .unwrap()
        .map(|v| v.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, vec![std::ffi::OsString::from("original")]);
    assert!(input.open_child(&path, "original").is_ok());
    assert!(input.open_child(&path, "secret").is_err());
    assert!(matches!(Input::open(&path), Err(ErrorCode::UnsafePath)));
}
