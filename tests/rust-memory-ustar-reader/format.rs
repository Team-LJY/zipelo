use crate::support::*;
use zipelo_memory_ustar_reader::{ContentVerification, Failure, Kind};

#[test]
fn empty_archive() {
    let bytes = vec![0; 1024];
    let result = run(bytes.clone(), &[]);
    assert!(result.entries.is_empty() && result.files.is_empty());
    assert_eq!(result.usage.entries, 0);
    assert_eq!(result.usage.processed, 1024);
    assert_eq!(result.input_sha256, digest(&bytes));
    assert_eq!(
        result.content_verification,
        ContentVerification::Unavailable
    );
}

#[test]
fn list_validates_all_entries() {
    let bytes = normal();
    let result = run(bytes.clone(), &[]);
    assert_eq!(
        result
            .entries
            .iter()
            .map(|e| (e.id.as_str(), e.name.as_str(), e.kind, e.size))
            .collect::<Vec<_>>(),
        vec![
            ("0", "folder", Kind::Directory, 0),
            ("1", "folder/a.txt", Kind::File, 5),
            ("2", "empty", Kind::File, 0),
            ("3", "b.bin", Kind::File, 140_000)
        ]
    );
    assert!(result.files.is_empty());
    assert_eq!(result.usage.entries, 4);
    assert_eq!(result.usage.processed, bytes.len() as u64);
    assert_eq!(result.usage.output, 0);
    assert_eq!(result.input_sha256, digest(&bytes));
    assert_eq!(
        result.content_verification,
        ContentVerification::Unavailable
    );
}

#[test]
fn zero_trailing_blocks_hashed() {
    let mut bytes = normal();
    bytes.extend_from_slice(&vec![0; 1024]);
    let result = run(bytes.clone(), &["1"]);
    assert_eq!(result.usage.processed, bytes.len() as u64 * 2);
    assert_eq!(result.input_sha256, digest(&bytes));
}

#[test]
fn payload_is_not_authenticated_by_header_checksum() {
    let original = archive(&[Fixture::file("a", b"one")]);
    let mut changed = original.clone();
    changed[512] ^= 1;
    let first = run(original, &["0"]);
    let second = run(changed.clone(), &["0"]);
    assert_ne!(first.input_sha256, second.input_sha256);
    assert_eq!(second.input_sha256, digest(&changed));
    assert_eq!(second.files[0].bytes, b"nne");
    assert_eq!(
        second.content_verification,
        ContentVerification::Unavailable
    );
}

#[test]
fn nul_regular_file_type() {
    let mut entry = Fixture::file("a", &[7, 8, 9]);
    entry.kind = 0;
    let result = run(archive(&[entry]), &["0"]);
    assert_eq!(result.entries[0].kind, Kind::File);
    assert_eq!(result.files[0].bytes, [7, 8, 9]);
}

#[test]
fn checksum_mismatch() {
    let mut bytes = normal();
    bytes[0] ^= 1;
    reject(bytes, Failure::Integrity);
}

#[test]
fn truncated_header() {
    reject(normal()[..511].to_vec(), Failure::Integrity);
}

#[test]
fn truncated_body() {
    reject(
        archive(&[Fixture::file("a", &vec![0; 1024])])[..1024].to_vec(),
        Failure::Integrity,
    );
}

#[test]
fn missing_end() {
    reject(
        archive(&[Fixture::file("a", &[])])[..512].to_vec(),
        Failure::Integrity,
    );
}

#[test]
fn single_end() {
    reject(
        archive(&[Fixture::file("a", &[])])[..1024].to_vec(),
        Failure::Integrity,
    );
}

#[test]
fn trailing_partial() {
    reject(
        {
            let mut b = normal();
            b.push(0);
            b
        },
        Failure::Integrity,
    );
}

#[test]
fn second_end_nonzero() {
    reject(
        {
            let mut b = vec![0; 1024];
            b[512] = 1;
            b
        },
        Failure::Integrity,
    );
}

#[test]
fn trailing_nonzero() {
    reject(
        {
            let mut b = normal();
            b.extend_from_slice(&[1; 512]);
            b
        },
        Failure::Integrity,
    );
}

#[test]
fn padding_nonzero() {
    reject(
        {
            let mut b = archive(&[Fixture::file("a", &[1])]);
            b[513] = 1;
            b
        },
        Failure::Integrity,
    );
}

#[test]
fn size_huge_octal() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 124, 12, b"77777777777\0"), Failure::Integrity);
}

#[test]
fn directory_nonzero() {
    let bytes = archive(&[Fixture::directory("a/")]);
    reject(field(bytes, 124, 12, b"00000000001\0"), Failure::Integrity);
}

#[test]
fn version() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 263, 2, b"01"), Failure::Unsupported);
}

#[test]
fn gnu_magic() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 257, 6, b"ustar "), Failure::Unsupported);
}

#[test]
fn linkname_on_file() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 157, 100, b"x"), Failure::Integrity);
}

#[test]
fn reserved_bytes() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 500, 12, &[1]), Failure::Integrity);
}

#[test]
fn name_hidden_after_nul() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 0, 100, b"a\0b"), Failure::Integrity);
}

#[test]
fn invalid_utf8_name() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 0, 100, &[255]), Failure::Integrity);
}

#[test]
fn invalid_utf8_prefix() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 345, 155, &[255]), Failure::Integrity);
}

#[test]
fn invalid_utf8_uname() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 265, 32, &[255]), Failure::Integrity);
}

#[test]
fn invalid_utf8_gname() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 297, 32, &[255]), Failure::Integrity);
}

#[test]
fn prefix_hidden_after_nul() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 345, 155, b"a\0b"), Failure::Integrity);
}

#[test]
fn uname_hidden_after_nul() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 265, 32, b"a\0b"), Failure::Integrity);
}

#[test]
fn gname_hidden_after_nul() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 297, 32, b"a\0b"), Failure::Integrity);
}

#[test]
fn device_major_nonzero() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 329, 8, b"0000001\0"), Failure::Integrity);
}

#[test]
fn device_minor_nonzero() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    reject(field(bytes, 337, 8, b"0000001\0"), Failure::Integrity);
}

#[test]
fn size_base256_valid_control() {
    let data = vec![0; 0];
    let result = run(archive(&[Fixture::file("number.bin", &data)]), &["0"]);
    assert_eq!(result.entries[0].size, data.len() as u64);
    assert_eq!(result.files[0].bytes, data);
}

#[test]
fn size_base256() {
    let bytes = archive(&[Fixture::file("number.bin", &vec![0; 0])]);
    reject(
        field(bytes, 124, 12, &[128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Failure::Integrity,
    );
}

#[test]
fn size_negative_valid_control() {
    let data = vec![0; 0];
    let result = run(archive(&[Fixture::file("number.bin", &data)]), &["0"]);
    assert_eq!(result.entries[0].size, data.len() as u64);
    assert_eq!(result.files[0].bytes, data);
}

#[test]
fn size_negative() {
    let bytes = archive(&[Fixture::file("number.bin", &vec![0; 0])]);
    reject(field(bytes, 124, 12, b"-0000000000\0"), Failure::Integrity);
}

#[test]
fn size_invalid_digit_valid_control() {
    let data = vec![0; 8];
    let result = run(archive(&[Fixture::file("number.bin", &data)]), &["0"]);
    assert_eq!(result.entries[0].size, data.len() as u64);
    assert_eq!(result.files[0].bytes, data);
}

#[test]
fn size_invalid_digit() {
    let bytes = archive(&[Fixture::file("number.bin", &vec![0; 8])]);
    reject(field(bytes, 124, 12, b"00000000008\0"), Failure::Integrity);
}

#[test]
fn size_internal_terminator_valid_control() {
    let data = vec![0; 1];
    let result = run(archive(&[Fixture::file("number.bin", &data)]), &["0"]);
    assert_eq!(result.entries[0].size, data.len() as u64);
    assert_eq!(result.files[0].bytes, data);
}

#[test]
fn size_internal_terminator() {
    let bytes = archive(&[Fixture::file("number.bin", &vec![0; 1])]);
    reject(
        field(bytes, 124, 12, b"000000000\x001\0"),
        Failure::Integrity,
    );
}

#[test]
fn type_pax_local() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'x';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_pax_global() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'g';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_gnu_long_name() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'L';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_gnu_long_link() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'K';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_gnu_sparse() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'S';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_hard_link() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'1';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_symbolic_link() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'2';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_character_device() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'3';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_block_device() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'4';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_fifo() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'6';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_contiguous() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'7';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_volume() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'V';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn type_unknown() {
    let mut entry = Fixture::file("a", &[]);
    entry.kind = b'?';
    reject(archive(&[entry]), Failure::Unsupported);
}

#[test]
fn numeric_fields_require_digits() {
    for (offset, width) in [(100, 8), (108, 8), (116, 8), (124, 12), (136, 12)] {
        reject(
            field(archive(&[Fixture::file("a", &[])]), offset, width, b""),
            Failure::Integrity,
        );
    }
}

#[test]
fn numeric_fields_reject_terminator_then_digit() {
    for (offset, width) in [
        (100, 8),
        (108, 8),
        (116, 8),
        (124, 12),
        (136, 12),
        (329, 8),
        (337, 8),
    ] {
        reject(
            field(archive(&[Fixture::file("a", &[])]), offset, width, b"0 1"),
            Failure::Integrity,
        );
    }
}

#[test]
fn numeric_fields_allow_leading_space_and_zero_device() {
    let mut bytes = archive(&[Fixture::file("a", &[])]);
    for (offset, width) in [
        (100, 8),
        (108, 8),
        (116, 8),
        (124, 12),
        (136, 12),
        (329, 8),
        (337, 8),
    ] {
        bytes = field(bytes, offset, width, b" 0 \0 ");
    }
    assert_eq!(run(bytes, &["0"]).files[0].bytes.len(), 0);
}

#[test]
fn device_fields_allow_empty() {
    let bytes = archive(&[Fixture::file("a", &[])]);
    assert_eq!(run(bytes, &["0"]).entries.len(), 1);
}

#[test]
fn numeric_fields_allow_full_width_octal() {
    let mut bytes = archive(&[Fixture::file("a", &[])]);
    for (offset, width) in [(100, 8), (108, 8), (116, 8), (136, 12)] {
        bytes = field(bytes, offset, width, &vec![b'7'; width]);
    }
    assert_eq!(run(bytes, &["0"]).entries[0].size, 0);
}

#[test]
fn checksum_grammar_rejects_internal_terminator() {
    let mut bytes = archive(&[Fixture::file("a", &[])]);
    bytes[148..156].copy_from_slice(b"00\x000001 ");
    reject(bytes, Failure::Integrity);
}

#[test]
fn unselected_damage_blocks_selection() {
    let mut bytes = archive(&[Fixture::file("ok", &[1]), Fixture::file("bad", &[2])]);
    bytes[1024] ^= 1;
    reject(bytes, Failure::Integrity);
}
