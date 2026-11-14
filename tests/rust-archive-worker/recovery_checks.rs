use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
struct Scratch(PathBuf);
static NEXT: AtomicU64 = AtomicU64::new(0);
impl Scratch {
    fn new(bytes: &[u8]) -> Self {
        let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "rec-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("source.zip"), bytes).unwrap();
        Self(dir)
    }
    fn path(&self) -> PathBuf {
        self.0.join("source.zip")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[cfg(unix)]
#[test]
fn publication_recheck_rejects_content_metadata_and_object_changes() {
    let bytes = include_bytes!("../zip-recovery-fixtures/missing-directory.zip");
    for mode in 0..3 {
        let s = Scratch::new(bytes);
        let mut recovery = Recovery::open(&s.path(), Limits::default(), &mut || Ok(())).unwrap();
        match mode {
            0 => {
                let mut changed = bytes.to_vec();
                changed[40] ^= 1;
                std::fs::write(s.path(), changed).unwrap();
            }
            1 => {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(s.path())
                    .unwrap()
                    .set_len(10)
                    .unwrap();
            }
            _ => {
                std::fs::rename(s.path(), s.0.join("old")).unwrap();
                std::fs::write(s.path(), bytes).unwrap();
            }
        }
        assert_eq!(
            recovery.verify_source(&mut || Ok(())),
            Err(ErrorCode::SourceChanged)
        );
    }
}
#[test]
fn unchanged_source_recheck_succeeds_without_rewriting_original() {
    let bytes = include_bytes!("../zip-recovery-fixtures/broken-local-header.zip");
    let s = Scratch::new(bytes);
    let mut recovery = Recovery::open(&s.path(), Limits::default(), &mut || Ok(())).unwrap();
    recovery.verify_source(&mut || Ok(())).unwrap();
    assert_eq!(std::fs::read(s.path()).unwrap(), bytes);
}
#[test]
fn four_gibibyte_input_cap_is_checked_before_capture() {
    let s = Scratch::new(&[]);
    std::fs::OpenOptions::new()
        .write(true)
        .open(s.path())
        .unwrap()
        .set_len(INPUT_CAP + 1)
        .unwrap();
    assert!(matches!(
        Recovery::open(&s.path(), Limits::default(), &mut || Ok(())),
        Err(ErrorCode::UnsupportedFeature)
    ));
}
#[test]
fn failed_crc_still_consumes_decoded_byte_budget() {
    struct Usage(u64);
    impl Monitor for Usage {
        fn check(&mut self) -> Result<()> {
            Ok(())
        }
        fn observe(&mut self, usage: ResourceUsage, _storage: Option<&File>) -> Result<()> {
            self.0 = self.0.max(usage.actual_bytes.unwrap_or(0));
            Ok(())
        }
    }
    let s = Scratch::new(include_bytes!("../zip-recovery-fixtures/body-corrupt.zip"));
    let mut usage = Usage(0);
    let recovery = Recovery::open(&s.path(), Limits::default(), &mut usage).unwrap();
    assert_eq!(usage.0, 1521);
    assert_eq!(recovery.data.metadata().unwrap().len(), 497);
    assert_eq!(recovery.summary.lost_entries, 1);
}
#[test]
fn deflate_truncation_and_trailing_bytes_are_not_verified() {
    for length in [36, 38] {
        let mut input = spool::temporary().unwrap();
        input
            .write_all(include_bytes!("../zip-recovery-fixtures/control.zip"))
            .unwrap();
        input.seek(SeekFrom::Start(112)).unwrap();
        let mut output = spool::temporary().unwrap();
        let mut used = 0;
        assert!(matches!(
            decode(
                &mut input,
                length,
                8,
                432,
                &mut output,
                &mut used,
                Limits::default(),
                &mut || Ok(())
            ),
            Err(ErrorCode::Integrity)
        ));
    }
}
#[test]
fn cooperative_cancel_before_decode_preserves_source() {
    let original = include_bytes!("../zip-recovery-fixtures/missing-directory.zip");
    let s = Scratch::new(original);
    let mut count = 0;
    assert!(matches!(
        Recovery::open(&s.path(), Limits::default(), &mut || {
            count += 1;
            if count > 15 {
                Err(ErrorCode::None)
            } else {
                Ok(())
            }
        }),
        Err(ErrorCode::None)
    ));
    assert_eq!(std::fs::read(s.path()).unwrap(), original);
}
#[test]
fn signed_and_unsigned_descriptors_require_exact_central_evidence() {
    let source = include_bytes!("../zip-recovery-fixtures/control.zip");
    for signed in [false, true] {
        let mut bytes = source[..65].to_vec();
        bytes[..4].copy_from_slice(b"BAD!");
        bytes[6..8].copy_from_slice(&8u16.to_le_bytes());
        bytes[14..26].fill(0);
        if signed {
            bytes.extend(b"PK\x07\x08");
        }
        bytes.extend(&source[14..26]);
        let cd = bytes.len() as u32;
        let mut central = source[1372..1427].to_vec();
        central[8..10].copy_from_slice(&8u16.to_le_bytes());
        bytes.extend(&central);
        let mut end = source[source.len() - 22..].to_vec();
        end[8..10].copy_from_slice(&1u16.to_le_bytes());
        end[10..12].copy_from_slice(&1u16.to_le_bytes());
        end[12..16].copy_from_slice(&55u32.to_le_bytes());
        end[16..20].copy_from_slice(&cd.to_le_bytes());
        bytes.extend(&end);
        let s = Scratch::new(&bytes);
        let recovery = Recovery::open(&s.path(), Limits::default(), &mut || Ok(())).unwrap();
        assert_eq!(recovery.summary.verified_entries, 1);
        assert!(recovery.summary.complete());
        assert_eq!(recovery.findings[0].reason, RecoveryReason::HeaderRebuilt);
    }
}

#[test]
fn decoder_error_bytes_are_charged_before_reporting_loss() {
    let original = &include_bytes!("../zip-recovery-fixtures/control.zip")[112..149];
    let mut probe = None;
    'search: for at in 0..original.len() {
        for bit in 0..8 {
            let mut damaged = original.to_vec();
            damaged[at] ^= 1 << bit;
            let mut stream = Inflate::new(false, 15);
            let mut plain = [0; 65536];
            if stream
                .decompress(&damaged, &mut plain, InflateFlush::NoFlush)
                .is_err()
                && stream.total_out() > 0
            {
                probe = Some((damaged, stream.total_out()));
                break 'search;
            }
        }
    }
    let (bytes, count) = probe.expect("오류 전에 디코딩된 표본이 필요함");
    let mut input = spool::temporary().unwrap();
    input.write_all(&bytes).unwrap();
    input.rewind().unwrap();
    let mut output = spool::temporary().unwrap();
    let mut used = 0;
    assert!(matches!(
        decode(
            &mut input,
            bytes.len() as u64,
            8,
            65536,
            &mut output,
            &mut used,
            Limits::default(),
            &mut || Ok(())
        ),
        Err(ErrorCode::Integrity)
    ));
    assert_eq!(used, count);
}

#[test]
fn zip64_locator_after_central_directory_remains_unsupported() {
    let source = include_bytes!("../zip-recovery-fixtures/broken-local-header.zip");
    let mut input = source[..source.len() - 22].to_vec();
    input.extend(b"PK\x06\x07");
    input.extend(0u32.to_le_bytes());
    input.extend(0u64.to_le_bytes());
    input.extend(1u32.to_le_bytes());
    input.extend(&source[source.len() - 22..]);
    let s = Scratch::new(&input);
    assert!(matches!(
        Recovery::open(&s.path(), Limits::default(), &mut || Ok(())),
        Err(ErrorCode::UnsupportedFeature)
    ));
    assert_eq!(std::fs::read(s.path()).unwrap(), input);
}
