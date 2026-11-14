use super::*;

#[test]
fn uint64_boundaries_match_the_seven_zip_encoding() {
    for (value, expected) in [
        (0, vec![0]),
        (127, vec![0x7f]),
        (128, vec![0x80, 0x80]),
        (16_383, vec![0xbf, 0xff]),
        (16_384, vec![0xc0, 0, 0x40]),
        (u32::MAX as u64, vec![0xf0, 0xff, 0xff, 0xff, 0xff]),
        (1u64 << 32, vec![0xf1, 0, 0, 0, 0]),
        (1u64 << 56, vec![0xff, 0, 0, 0, 0, 0, 0, 0, 1]),
        (u64::MAX, vec![0xff; 9]),
    ] {
        let mut header = Header::new(9).unwrap();
        header.integer(value);
        assert_eq!(header.0, expected, "{value}");
    }
}

#[test]
fn bitmap_boundaries_keep_only_the_empty_stream_subset() {
    for (count, expected) in [(7, vec![0xaa]), (8, vec![0xaa]), (9, vec![0xaa, 0x80])] {
        let mut header = Header::new(2).unwrap();
        header.bits((0..count).map(|n| n % 2 == 0));
        assert_eq!(header.0, expected);
    }
    let mut header = Header::new(2).unwrap();
    header.bits([true, false, false, true, true, false, true, false, true].into_iter());
    assert_eq!(header.0, [0x9a, 0x80]);
}

#[test]
fn encoder_dictionary_memory_and_packed_bound_are_checked() {
    let settings = options().unwrap();
    assert_eq!(settings.lzma_options.dict_size, 1_048_576);
    assert!(settings.lzma_options.get_memory_usage() <= 32 * 1024);
    assert_eq!(
        (2 | (DICTIONARY_PROPERTY as u32 & 1)) << (DICTIONARY_PROPERTY / 2 + 11),
        DICTIONARY
    );
    assert_eq!(packed_bound(0), Ok(1));
    assert_eq!(packed_bound(65_536), Ok(262_145));
    assert_eq!(packed_bound(u64::MAX / 4), Ok(u64::MAX - 2));
    assert_eq!(packed_bound(u64::MAX / 4 + 1), Err(ErrorCode::Limit));
}

#[test]
fn encoding_preserves_source_crc_and_respects_stream_boundaries() {
    use lzma_rust2::Lzma2Reader;
    for size in [1, 65_535, 65_536, 65_537, 131_081] {
        let expected: Vec<u8> = (0..size).map(|n| ((n * 73 + n / 17) % 251) as u8).collect();
        let mut input = spool::temporary().unwrap();
        input.write_all(&expected).unwrap();
        let mut output = spool::temporary().unwrap();
        output.write_all(&[0; 32]).unwrap();
        let stream = pack(&mut input, size as u64, &mut output, 0, &mut || Ok(())).unwrap();
        assert_eq!(stream.size, size as u64);
        assert_eq!(stream.crc, crc32(0, &expected));
        assert!(stream.packed <= packed_bound(size as u64).unwrap());
        output.seek(SeekFrom::Start(32)).unwrap();
        let mut actual = Vec::new();
        Lzma2Reader::new(output.take(stream.packed), DICTIONARY, None)
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn monitored_encoder_output_preserves_cancellation_error() {
    let mut file = spool::temporary().unwrap();
    let mut check = || Err(ErrorCode::None);
    let mut sink = Sink {
        file: &mut file,
        check: &mut check,
        error: None,
        written: 0,
        bound: 1,
        crc: 0,
        base: 0,
        entries: 0,
        reported: Instant::now(),
    };
    assert!(sink.write_all(b"x").is_err());
    assert_eq!(sink.error, Some(ErrorCode::None));
    assert_eq!(sink.written, 0);
}

#[test]
fn every_encoding_checkpoint_preserves_cooperative_cancellation() {
    struct Stop {
        at: usize,
        seen: usize,
    }
    impl Monitor for Stop {
        fn check(&mut self) -> Result<()> {
            self.seen += 1;
            if self.at == self.seen {
                Err(ErrorCode::None)
            } else {
                Ok(())
            }
        }
    }
    let mut input = spool::temporary().unwrap();
    input.write_all(b"checkpoint").unwrap();
    let mut maximum = Stop {
        at: usize::MAX,
        seen: 0,
    };
    let mut output = spool::temporary().unwrap();
    output.write_all(&[0; 32]).unwrap();
    pack(&mut input, 10, &mut output, 0, &mut maximum).unwrap();
    for at in 1..=maximum.seen {
        let mut output = spool::temporary().unwrap();
        output.write_all(&[0; 32]).unwrap();
        let result = pack(&mut input, 10, &mut output, 0, &mut Stop { at, seen: 0 });
        assert!(matches!(result, Err(ErrorCode::None)), "checkpoint {at}");
    }
}

#[test]
fn header_metadata_is_charged_before_source_capture() {
    use std::path::PathBuf;
    let root = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/work/tmp"));
    std::fs::create_dir_all(&root).unwrap();
    let root = root
        .canonicalize()
        .unwrap()
        .join(format!("7zc-budget-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("empty");
    std::fs::write(&path, b"").unwrap();
    let mut plan = Plan::new(
        &[Source {
            path: NativePath::from_path(&path),
            archive_name: "empty".into(),
        }],
        Limits::default(),
        &mut || Ok(()),
    )
    .unwrap();
    struct Tiny;
    impl Monitor for Tiny {
        fn metadata_limits(&self) -> crate::archive::MetadataLimits {
            crate::archive::MetadataLimits {
                bytes: 1,
                ..Default::default()
            }
        }
        fn check(&mut self) -> Result<()> {
            Ok(())
        }
        fn progress(&mut self, _: Stage, _: u64, _: Option<u64>, _: u32) -> Result<()> {
            panic!("헤더 예산 검사 전 캡처가 시작됨")
        }
    }
    assert!(matches!(
        archive(&mut plan, Limits::default(), &mut Tiny),
        Err(ErrorCode::Limit)
    ));
    std::fs::remove_dir_all(root).unwrap();
}
