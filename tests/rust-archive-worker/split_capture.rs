use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/work/tmp")
            });
        std::fs::create_dir_all(&root).unwrap();
        let path = root.canonicalize().unwrap().join(format!(
            "zipelo-split-capture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        let mut builder = builder;
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path).unwrap();
        Self(path)
    }
    fn split(&self, count: usize) -> Vec<u8> {
        let bytes = include_bytes!("fixtures/7z/copy.7z");
        for index in (0..count).rev() {
            std::fs::write(
                self.0.join(format!("sample.7z.{:03}", index + 1)),
                &bytes[index * bytes.len() / count..(index + 1) * bytes.len() / count],
            )
            .unwrap();
        }
        bytes.to_vec()
    }
    fn first(&self) -> PathBuf {
        self.0.join("sample.7z.001")
    }
    fn capture(&self) -> Captured {
        open(
            &self.first(),
            None,
            u64::MAX,
            crate::archive::MetadataLimits::default(),
            &mut |_| Ok(()),
        )
        .unwrap()
        .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("분할 조각 임시 폴더 정리에 실패함");
    }
}
fn code<T>(result: Result<T>) -> ErrorCode {
    match result {
        Err(failure) => failure.error,
        Ok(_) => panic!("오류가 필요함"),
    }
}
fn token(capture: &Captured) -> ListId {
    ListId {
        catalog_version: 2,
        format: Format::SevenZip,
        source_object: capture.snapshot.object.clone(),
        archive_bytes: capture.snapshot.len,
        archive_sha256: capture.snapshot.hash,
    }
}

#[test]
fn concatenated_snapshot_and_manifest_include_every_native_component() {
    let s = Scratch::new();
    let original = s.split(11);
    let mut captured = s.capture();
    let mut actual = Vec::new();
    captured.snapshot.file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, original);
    assert_eq!(
        captured.snapshot.hash,
        <[u8; 32]>::from(Sha256::digest(&original))
    );
    let group = &captured.group;
    let mut bytes = b"zipelo.split7z.v1\0".to_vec();
    fn object(bytes: &mut Vec<u8>, object: &SourceObject) {
        match object {
            SourceObject::Unix { device, inode } => {
                bytes.push(1);
                bytes.extend(device.to_le_bytes());
                bytes.extend(inode.to_le_bytes());
            }
            SourceObject::Windows {
                volume_serial,
                file_id,
            } => {
                bytes.push(2);
                bytes.extend(volume_serial.to_le_bytes());
                bytes.extend(file_id);
            }
            _ => panic!("단일 객체가 필요함"),
        }
    }
    object(&mut bytes, &group.object);
    bytes.extend(11u16.to_le_bytes());
    for (index, part) in group.parts.iter().enumerate() {
        bytes.extend(((index + 1) as u16).to_le_bytes());
        match NativePath::from_path(Path::new(&part.name)) {
            NativePath::Unix(name) => {
                bytes.push(1);
                bytes.extend((name.len() as u32).to_le_bytes());
                bytes.extend(name);
            }
            NativePath::Windows(name) => {
                bytes.push(2);
                bytes.extend((name.len() as u32).to_le_bytes());
                for unit in name {
                    bytes.extend(unit.to_le_bytes());
                }
            }
        }
        object(&mut bytes, &part.object);
        bytes.extend(part.length.to_le_bytes());
        bytes.extend(part.hash);
    }
    assert_eq!(
        captured.snapshot.object,
        SourceObject::SplitSevenZip {
            manifest_sha256: Sha256::digest(&bytes).into()
        }
    );
}

#[test]
fn verification_hashes_middle_and_tail_even_when_size_and_time_are_restored() {
    for index in [2, 3] {
        let s = Scratch::new();
        s.split(3);
        let captured = s.capture();
        let path = s.0.join(format!("sample.7z.{index:03}"));
        let file = File::options().read(true).write(true).open(&path).unwrap();
        let modified = file.metadata().unwrap().modified().unwrap();
        let mut changed = std::fs::read(&path).unwrap();
        changed[0] ^= 1;
        std::fs::write(&path, changed).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(
            code(captured.group.verify(&mut |_| Ok(()))),
            ErrorCode::SourceChanged
        );
    }
}

#[test]
fn capture_rechecks_parts_after_the_last_read() {
    let s = Scratch::new();
    s.split(3);
    let mut changed = false;
    let result = open(
        &s.first(),
        None,
        u64::MAX,
        crate::archive::MetadataLimits::default(),
        &mut |step| {
            if matches!(step, Step::Read { .. }) && !changed {
                changed = true;
                let p = s.0.join("sample.7z.002");
                let mut bytes = std::fs::read(&p).unwrap();
                bytes[0] ^= 1;
                std::fs::write(p, bytes).unwrap();
            }
            Ok(())
        },
    );
    assert!(changed);
    assert_eq!(code(result), ErrorCode::SourceChanged);
}

#[test]
fn verification_detects_parent_replacement_and_new_part_names() {
    let s = Scratch::new();
    s.split(3);
    let captured = s.capture();
    std::fs::write(s.0.join("sample.7z.004"), [0]).unwrap();
    assert_eq!(
        code(captured.group.verify(&mut |_| Ok(()))),
        ErrorCode::SourceChanged
    );
    std::fs::remove_file(s.0.join("sample.7z.004")).unwrap();
    let renamed = s.0.with_extension("old");
    #[cfg(unix)]
    {
        std::fs::rename(&s.0, &renamed).unwrap();
        std::fs::create_dir(&s.0).unwrap();
        assert_eq!(
            code(captured.group.verify(&mut |_| Ok(()))),
            ErrorCode::SourceChanged
        );
        std::fs::remove_dir_all(renamed).unwrap();
    }
    #[cfg(windows)]
    {
        // 고정한 디렉터리는 Windows 공유 모드로 이름 변경을 차단함.
        assert!(std::fs::rename(&s.0, &renamed).is_err());
        captured.group.verify(&mut |_| Ok(())).unwrap();
    }
}

#[test]
fn scan_and_rehash_keep_cancellation_and_deadline_errors() {
    for error in [ErrorCode::None, ErrorCode::Limit] {
        let s = Scratch::new();
        s.split(9);
        for at in [1, 10, 50] {
            let mut checks = 0;
            assert_eq!(
                code(open(
                    &s.first(),
                    None,
                    u64::MAX,
                    crate::archive::MetadataLimits::default(),
                    &mut |step| {
                        if matches!(step, Step::Check) {
                            checks += 1;
                            if checks == at {
                                return Err(error);
                            }
                        }
                        Ok(())
                    }
                )),
                error
            );
        }
        let captured = s.capture();
        for at in [1, 10, 40] {
            let mut checks = 0;
            assert_eq!(
                code(captured.group.verify(&mut |step| {
                    if matches!(step, Step::Check) {
                        checks += 1;
                        if checks == at {
                            return Err(error);
                        }
                    }
                    Ok(())
                })),
                error
            );
        }
    }
}

#[test]
fn aggregate_input_and_metadata_limits_are_not_reset_per_part() {
    let s = Scratch::new();
    let bytes = s.split(11);
    assert_eq!(
        code(open(
            &s.first(),
            None,
            bytes.len() as u64 - 1,
            crate::archive::MetadataLimits::default(),
            &mut |_| Ok(())
        )),
        ErrorCode::Limit
    );
    assert!(
        open(
            &s.first(),
            None,
            bytes.len() as u64,
            crate::archive::MetadataLimits::default(),
            &mut |_| Ok(())
        )
        .unwrap()
        .is_some()
    );
    let metadata = crate::archive::MetadataLimits {
        bytes: 512,
        ..Default::default()
    };
    assert_eq!(
        code(open(&s.first(), None, u64::MAX, metadata, &mut |_| Ok(()))),
        ErrorCode::Limit
    );
}

#[cfg(unix)]
#[test]
fn capture_rejects_symlinks_hard_link_aliases_and_special_files() {
    use std::os::unix::fs::symlink;
    let s = Scratch::new();
    s.split(3);
    let first = s.first();
    let second = s.0.join("sample.7z.002");
    std::fs::remove_file(&second).unwrap();
    symlink(&first, &second).unwrap();
    assert_eq!(
        code(open(
            &first,
            None,
            u64::MAX,
            Default::default(),
            &mut |_| Ok(())
        )),
        ErrorCode::UnsafePath
    );
    std::fs::remove_file(&second).unwrap();
    std::fs::hard_link(&first, &second).unwrap();
    assert_eq!(
        code(open(
            &first,
            None,
            u64::MAX,
            Default::default(),
            &mut |_| Ok(())
        )),
        ErrorCode::UnsafePath
    );
    std::fs::remove_file(&second).unwrap();
    std::fs::create_dir(&second).unwrap();
    assert_eq!(
        code(open(
            &first,
            None,
            u64::MAX,
            Default::default(),
            &mut |_| Ok(())
        )),
        ErrorCode::UnsafePath
    );
}

#[test]
fn same_logical_data_with_different_part_boundaries_changes_group_identity() {
    let s = Scratch::new();
    s.split(3);
    let captured = s.capture();
    let expected = token(&captured);
    let first = s.first();
    let second = s.0.join("sample.7z.002");
    let mut a = std::fs::read(&first).unwrap();
    let mut b = std::fs::read(&second).unwrap();
    a.push(b.remove(0));
    std::fs::write(first, a).unwrap();
    std::fs::write(second, b).unwrap();
    let next = s.capture();
    assert_eq!(next.snapshot.hash, captured.snapshot.hash);
    assert_ne!(next.snapshot.object, captured.snapshot.object);
    assert_eq!(
        code(open(
            &s.first(),
            Some(&expected),
            u64::MAX,
            Default::default(),
            &mut |_| Ok(())
        )),
        ErrorCode::SourceChanged
    );
}

#[test]
fn combined_lengths_reject_u64_overflow_without_changing_the_limit() {
    assert_eq!(combined_length(u64::MAX - 1, 1).unwrap(), u64::MAX);
    assert_eq!(code(combined_length(u64::MAX, 1)), ErrorCode::Limit);
    assert_eq!(code(combined_length(u64::MAX - 1, 2)), ErrorCode::Limit);
}

#[cfg(unix)]
#[test]
fn native_non_utf8_part_names_are_preserved_without_lossy_matching() {
    use std::os::unix::ffi::OsStringExt;
    let s = Scratch::new();
    let original = s.split(3);
    for index in 1..=3 {
        let mut name = b"\xff\x01.7z.".to_vec();
        name.extend(format!("{index:03}").bytes());
        std::fs::rename(
            s.0.join(format!("sample.7z.{index:03}")),
            s.0.join(OsString::from_vec(name)),
        )
        .unwrap();
    }
    let path = s.0.join(OsString::from_vec(b"\xff\x01.7z.001".to_vec()));
    let captured = open(&path, None, u64::MAX, Default::default(), &mut |_| Ok(()))
        .unwrap()
        .unwrap();
    assert_eq!(
        captured.snapshot.hash,
        <[u8; 32]>::from(Sha256::digest(original))
    );
    assert_eq!(
        NativePath::from_path(Path::new(&captured.group.parts[0].name)),
        NativePath::Unix(b"\xff\x01.7z.001".to_vec())
    );
}

#[test]
fn disappearing_known_tail_is_a_source_change_before_reopen() {
    let s = Scratch::new();
    s.split(3);
    let captured = s.capture();
    std::fs::remove_file(s.0.join("sample.7z.003")).unwrap();
    assert_eq!(
        code(
            captured
                .group
                .open_part(&captured.group.parts[2].name, &mut |_| Ok(()))
        ),
        ErrorCode::SourceChanged
    );
}

#[test]
fn scratch_uses_tmpdir_and_cleans_all_split_parts() {
    let root = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/work/tmp"));
    let path = {
        let s = Scratch::new();
        let root = root.canonicalize().unwrap();
        assert_eq!(s.0.parent(), Some(root.as_path()));
        s.split(11);
        assert_eq!(std::fs::read_dir(&s.0).unwrap().count(), 11);
        s.0.clone()
    };
    assert!(!path.exists());
}

#[test]
fn scratch_exit_child() {
    if std::env::var_os("ZIPELO_SPLIT_SCRATCH_EXIT_TEST").is_none() {
        return;
    }
    let s = Scratch::new();
    s.split(3);
    // Drop이 실행되지 않는 종료를 재현함.
    std::process::exit(73);
}

#[test]
fn scratch_abrupt_exit_leaves_parts_only_under_tmpdir() {
    let path = {
        let s = Scratch::new();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "split_seven_zip::tests::scratch_exit_child",
                "--test-threads=1",
            ])
            .env("TMPDIR", &s.0)
            .env("ZIPELO_SPLIT_SCRATCH_EXIT_TEST", "1")
            .output()
            .unwrap();
        assert_eq!(status.status.code(), Some(73), "{status:?}");
        let entries = std::fs::read_dir(&s.0)
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(entries.len(), 1);
        let leftover = entries[0].path();
        assert_eq!(leftover.parent(), Some(s.0.as_path()));
        assert!(
            leftover
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("zipelo-split-capture-")
        );
        assert_eq!(std::fs::read_dir(&leftover).unwrap().count(), 3);
        assert_eq!(
            std::fs::read(leftover.join("sample.7z.001")).unwrap().len(),
            include_bytes!("fixtures/7z/copy.7z").len() / 3
        );
        s.0.clone()
    };
    assert!(!path.exists());
}
