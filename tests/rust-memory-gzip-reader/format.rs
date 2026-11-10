use crate::support::*;
use zipelo_memory_gzip_reader::{Error, Failure, InflateError, Stage};

fn truncated_prefixes(name: &str, selected: bool, header: usize, raw: usize) {
    let input = fixture(name);
    for length in 0..input.len() {
        let mut budget = budget();
        let mut reached_return = false;
        let result = zipelo_memory_gzip_reader::read(
            request(&input[..length], selected),
            &mut budget,
            |point| {
                reached_return |= point.stage == Stage::BeforeReturn;
                Ok::<_, zipelo_memory_gzip_reader::Stop<()>>(())
            },
        );
        assert!(!reached_return, "{name}, prefix {length}");
        if length <= header || length >= header + raw {
            reader_error(result, Failure::Truncated);
        } else {
            assert!(
                matches!(
                    result,
                    Err(Error::Reader(Failure::Truncated))
                        | Err(Error::Backend(InflateError::DataError))
                ),
                "{name}, prefix {length}: {result:?}"
            );
        }
        if length < header {
            assert_eq!(budget.usage().processed, length as u64);
            assert_eq!(budget.usage().metadata, length as u64 + 1);
        }
        if !selected {
            assert_eq!(budget.usage().output, 0);
        }
    }
}

fn corrupt(offset: usize, selected: bool, expected: Failure) {
    let mut input = normal();
    input[offset] ^= 1;
    reader_error(read(&input, selected, &mut budget()), expected);
}

fn suffix(suffix: &[u8], selected: bool) {
    let mut input = normal();
    input.extend_from_slice(suffix);
    let mut budget = budget();
    reader_error(read(&input, selected, &mut budget), Failure::TrailingData);
    assert_eq!(budget.usage().processed, input.len() as u64 + 9);
    assert_eq!(budget.usage().output, if selected { 9 } else { 0 });
}

#[test]
fn flags_00_selected() {
    assert_valid("flags_00", true, b"123456789", 10, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_00_selected() {
    truncated_prefixes("flags_00", true, 10, 11);
}

#[test]
fn flags_00_unselected() {
    assert_valid("flags_00", false, b"123456789", 10, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_00_unselected() {
    truncated_prefixes("flags_00", false, 10, 11);
}

#[test]
fn flags_01_selected() {
    assert_valid("flags_01", true, b"123456789", 10, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_01_selected() {
    truncated_prefixes("flags_01", true, 10, 11);
}

#[test]
fn flags_01_unselected() {
    assert_valid("flags_01", false, b"123456789", 10, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_01_unselected() {
    truncated_prefixes("flags_01", false, 10, 11);
}

#[test]
fn flags_02_selected() {
    assert_valid("flags_02", true, b"123456789", 12, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_02_selected() {
    truncated_prefixes("flags_02", true, 12, 11);
}

#[test]
fn flags_02_unselected() {
    assert_valid("flags_02", false, b"123456789", 12, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_02_unselected() {
    truncated_prefixes("flags_02", false, 12, 11);
}

#[test]
fn flags_03_selected() {
    assert_valid("flags_03", true, b"123456789", 12, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_03_selected() {
    truncated_prefixes("flags_03", true, 12, 11);
}

#[test]
fn flags_03_unselected() {
    assert_valid("flags_03", false, b"123456789", 12, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_03_unselected() {
    truncated_prefixes("flags_03", false, 12, 11);
}

#[test]
fn flags_04_selected() {
    assert_valid("flags_04", true, b"123456789", 15, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_04_selected() {
    truncated_prefixes("flags_04", true, 15, 11);
}

#[test]
fn flags_04_unselected() {
    assert_valid("flags_04", false, b"123456789", 15, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_04_unselected() {
    truncated_prefixes("flags_04", false, 15, 11);
}

#[test]
fn flags_05_selected() {
    assert_valid("flags_05", true, b"123456789", 15, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_05_selected() {
    truncated_prefixes("flags_05", true, 15, 11);
}

#[test]
fn flags_05_unselected() {
    assert_valid("flags_05", false, b"123456789", 15, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_05_unselected() {
    truncated_prefixes("flags_05", false, 15, 11);
}

#[test]
fn flags_06_selected() {
    assert_valid("flags_06", true, b"123456789", 17, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_06_selected() {
    truncated_prefixes("flags_06", true, 17, 11);
}

#[test]
fn flags_06_unselected() {
    assert_valid("flags_06", false, b"123456789", 17, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_06_unselected() {
    truncated_prefixes("flags_06", false, 17, 11);
}

#[test]
fn flags_07_selected() {
    assert_valid("flags_07", true, b"123456789", 17, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_07_selected() {
    truncated_prefixes("flags_07", true, 17, 11);
}

#[test]
fn flags_07_unselected() {
    assert_valid("flags_07", false, b"123456789", 17, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_07_unselected() {
    truncated_prefixes("flags_07", false, 17, 11);
}

#[test]
fn flags_08_selected() {
    assert_valid("flags_08", true, b"123456789", 23, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_08_selected() {
    truncated_prefixes("flags_08", true, 23, 11);
}

#[test]
fn flags_08_unselected() {
    assert_valid("flags_08", false, b"123456789", 23, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_08_unselected() {
    truncated_prefixes("flags_08", false, 23, 11);
}

#[test]
fn flags_09_selected() {
    assert_valid("flags_09", true, b"123456789", 23, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_09_selected() {
    truncated_prefixes("flags_09", true, 23, 11);
}

#[test]
fn flags_09_unselected() {
    assert_valid("flags_09", false, b"123456789", 23, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_09_unselected() {
    truncated_prefixes("flags_09", false, 23, 11);
}

#[test]
fn flags_10_selected() {
    assert_valid("flags_10", true, b"123456789", 25, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_10_selected() {
    truncated_prefixes("flags_10", true, 25, 11);
}

#[test]
fn flags_10_unselected() {
    assert_valid("flags_10", false, b"123456789", 25, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_10_unselected() {
    truncated_prefixes("flags_10", false, 25, 11);
}

#[test]
fn flags_11_selected() {
    assert_valid("flags_11", true, b"123456789", 25, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_11_selected() {
    truncated_prefixes("flags_11", true, 25, 11);
}

#[test]
fn flags_11_unselected() {
    assert_valid("flags_11", false, b"123456789", 25, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_11_unselected() {
    truncated_prefixes("flags_11", false, 25, 11);
}

#[test]
fn flags_12_selected() {
    assert_valid("flags_12", true, b"123456789", 28, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_12_selected() {
    truncated_prefixes("flags_12", true, 28, 11);
}

#[test]
fn flags_12_unselected() {
    assert_valid("flags_12", false, b"123456789", 28, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_12_unselected() {
    truncated_prefixes("flags_12", false, 28, 11);
}

#[test]
fn flags_13_selected() {
    assert_valid("flags_13", true, b"123456789", 28, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_13_selected() {
    truncated_prefixes("flags_13", true, 28, 11);
}

#[test]
fn flags_13_unselected() {
    assert_valid("flags_13", false, b"123456789", 28, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_13_unselected() {
    truncated_prefixes("flags_13", false, 28, 11);
}

#[test]
fn flags_14_selected() {
    assert_valid("flags_14", true, b"123456789", 30, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_14_selected() {
    truncated_prefixes("flags_14", true, 30, 11);
}

#[test]
fn flags_14_unselected() {
    assert_valid("flags_14", false, b"123456789", 30, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_14_unselected() {
    truncated_prefixes("flags_14", false, 30, 11);
}

#[test]
fn flags_15_selected() {
    assert_valid("flags_15", true, b"123456789", 30, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_15_selected() {
    truncated_prefixes("flags_15", true, 30, 11);
}

#[test]
fn flags_15_unselected() {
    assert_valid("flags_15", false, b"123456789", 30, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_15_unselected() {
    truncated_prefixes("flags_15", false, 30, 11);
}

#[test]
fn flags_16_selected() {
    assert_valid("flags_16", true, b"123456789", 15, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_16_selected() {
    truncated_prefixes("flags_16", true, 15, 11);
}

#[test]
fn flags_16_unselected() {
    assert_valid("flags_16", false, b"123456789", 15, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_16_unselected() {
    truncated_prefixes("flags_16", false, 15, 11);
}

#[test]
fn flags_17_selected() {
    assert_valid("flags_17", true, b"123456789", 15, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_17_selected() {
    truncated_prefixes("flags_17", true, 15, 11);
}

#[test]
fn flags_17_unselected() {
    assert_valid("flags_17", false, b"123456789", 15, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_17_unselected() {
    truncated_prefixes("flags_17", false, 15, 11);
}

#[test]
fn flags_18_selected() {
    assert_valid("flags_18", true, b"123456789", 17, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_18_selected() {
    truncated_prefixes("flags_18", true, 17, 11);
}

#[test]
fn flags_18_unselected() {
    assert_valid("flags_18", false, b"123456789", 17, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_18_unselected() {
    truncated_prefixes("flags_18", false, 17, 11);
}

#[test]
fn flags_19_selected() {
    assert_valid("flags_19", true, b"123456789", 17, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_19_selected() {
    truncated_prefixes("flags_19", true, 17, 11);
}

#[test]
fn flags_19_unselected() {
    assert_valid("flags_19", false, b"123456789", 17, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_19_unselected() {
    truncated_prefixes("flags_19", false, 17, 11);
}

#[test]
fn flags_20_selected() {
    assert_valid("flags_20", true, b"123456789", 20, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_20_selected() {
    truncated_prefixes("flags_20", true, 20, 11);
}

#[test]
fn flags_20_unselected() {
    assert_valid("flags_20", false, b"123456789", 20, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_20_unselected() {
    truncated_prefixes("flags_20", false, 20, 11);
}

#[test]
fn flags_21_selected() {
    assert_valid("flags_21", true, b"123456789", 20, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_21_selected() {
    truncated_prefixes("flags_21", true, 20, 11);
}

#[test]
fn flags_21_unselected() {
    assert_valid("flags_21", false, b"123456789", 20, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_21_unselected() {
    truncated_prefixes("flags_21", false, 20, 11);
}

#[test]
fn flags_22_selected() {
    assert_valid("flags_22", true, b"123456789", 22, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_22_selected() {
    truncated_prefixes("flags_22", true, 22, 11);
}

#[test]
fn flags_22_unselected() {
    assert_valid("flags_22", false, b"123456789", 22, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_22_unselected() {
    truncated_prefixes("flags_22", false, 22, 11);
}

#[test]
fn flags_23_selected() {
    assert_valid("flags_23", true, b"123456789", 22, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_23_selected() {
    truncated_prefixes("flags_23", true, 22, 11);
}

#[test]
fn flags_23_unselected() {
    assert_valid("flags_23", false, b"123456789", 22, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_23_unselected() {
    truncated_prefixes("flags_23", false, 22, 11);
}

#[test]
fn flags_24_selected() {
    assert_valid("flags_24", true, b"123456789", 28, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_24_selected() {
    truncated_prefixes("flags_24", true, 28, 11);
}

#[test]
fn flags_24_unselected() {
    assert_valid("flags_24", false, b"123456789", 28, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_24_unselected() {
    truncated_prefixes("flags_24", false, 28, 11);
}

#[test]
fn flags_25_selected() {
    assert_valid("flags_25", true, b"123456789", 28, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_25_selected() {
    truncated_prefixes("flags_25", true, 28, 11);
}

#[test]
fn flags_25_unselected() {
    assert_valid("flags_25", false, b"123456789", 28, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_25_unselected() {
    truncated_prefixes("flags_25", false, 28, 11);
}

#[test]
fn flags_26_selected() {
    assert_valid("flags_26", true, b"123456789", 30, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_26_selected() {
    truncated_prefixes("flags_26", true, 30, 11);
}

#[test]
fn flags_26_unselected() {
    assert_valid("flags_26", false, b"123456789", 30, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_26_unselected() {
    truncated_prefixes("flags_26", false, 30, 11);
}

#[test]
fn flags_27_selected() {
    assert_valid("flags_27", true, b"123456789", 30, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_27_selected() {
    truncated_prefixes("flags_27", true, 30, 11);
}

#[test]
fn flags_27_unselected() {
    assert_valid("flags_27", false, b"123456789", 30, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_27_unselected() {
    truncated_prefixes("flags_27", false, 30, 11);
}

#[test]
fn flags_28_selected() {
    assert_valid("flags_28", true, b"123456789", 33, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_28_selected() {
    truncated_prefixes("flags_28", true, 33, 11);
}

#[test]
fn flags_28_unselected() {
    assert_valid("flags_28", false, b"123456789", 33, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_28_unselected() {
    truncated_prefixes("flags_28", false, 33, 11);
}

#[test]
fn flags_29_selected() {
    assert_valid("flags_29", true, b"123456789", 33, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_29_selected() {
    truncated_prefixes("flags_29", true, 33, 11);
}

#[test]
fn flags_29_unselected() {
    assert_valid("flags_29", false, b"123456789", 33, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_29_unselected() {
    truncated_prefixes("flags_29", false, 33, 11);
}

#[test]
fn flags_30_selected() {
    assert_valid("flags_30", true, b"123456789", 35, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_30_selected() {
    truncated_prefixes("flags_30", true, 35, 11);
}

#[test]
fn flags_30_unselected() {
    assert_valid("flags_30", false, b"123456789", 35, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_30_unselected() {
    truncated_prefixes("flags_30", false, 35, 11);
}

#[test]
fn flags_31_selected() {
    assert_valid("flags_31", true, b"123456789", 35, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_31_selected() {
    truncated_prefixes("flags_31", true, 35, 11);
}

#[test]
fn flags_31_unselected() {
    assert_valid("flags_31", false, b"123456789", 35, 11, 0xcbf43926);
}

#[test]
fn truncate_flags_31_unselected() {
    truncated_prefixes("flags_31", false, 35, 11);
}

#[test]
fn empty_fixed_selected() {
    assert_valid("empty_fixed", true, b"", 10, 2, 0);
}

#[test]
fn empty_stored_selected() {
    assert_valid("empty_stored", true, b"", 10, 5, 0);
}

#[test]
fn zeros_three_selected() {
    assert_valid("zeros_three", true, &[0; 3], 10, 5, 4282505490);
}

#[test]
fn byte_a_selected() {
    assert_valid("byte_a", true, b"A", 10, 6, 3554254475);
}

#[test]
fn opaque_name_selected() {
    assert_valid("opaque_name", true, b"123456789", 20, 11, 3421780262);
}

#[test]
fn empty_options_selected() {
    assert_valid("empty_options", true, b"123456789", 16, 11, 3421780262);
}

#[test]
fn large_output_selected() {
    assert_valid(
        "large_output",
        true,
        &vec![b'A'; 200003],
        10,
        212,
        1109513937,
    );
}

#[test]
fn multiple_input_selected() {
    assert_valid(
        "multiple_input",
        true,
        &(0..65537).map(|i| (i % 251) as u8).collect::<Vec<_>>(),
        10,
        65547,
        2848747123,
    );
}

#[test]
fn invalid_header_0_selected() {
    corrupt(0, true, Failure::Header);
}

#[test]
fn invalid_header_1_selected() {
    corrupt(1, true, Failure::Header);
}

#[test]
fn invalid_header_2_selected() {
    corrupt(2, true, Failure::Header);
}

#[test]
fn reserved_flag_32_selected() {
    let mut input = normal();
    input[3] = 32;
    reader_error(read(&input, true, &mut budget()), Failure::Header);
}

#[test]
fn reserved_flag_64_selected() {
    let mut input = normal();
    input[3] = 64;
    reader_error(read(&input, true, &mut budget()), Failure::Header);
}

#[test]
fn reserved_flag_128_selected() {
    let mut input = normal();
    input[3] = 128;
    reader_error(read(&input, true, &mut budget()), Failure::Header);
}

#[test]
fn invalid_crc_0_selected() {
    corrupt(21, true, Failure::Crc);
}

#[test]
fn invalid_crc_1_selected() {
    corrupt(22, true, Failure::Crc);
}

#[test]
fn invalid_crc_2_selected() {
    corrupt(23, true, Failure::Crc);
}

#[test]
fn invalid_crc_3_selected() {
    corrupt(24, true, Failure::Crc);
}

#[test]
fn invalid_size_0_selected() {
    corrupt(25, true, Failure::Size);
}

#[test]
fn invalid_size_1_selected() {
    corrupt(26, true, Failure::Size);
}

#[test]
fn invalid_size_2_selected() {
    corrupt(27, true, Failure::Size);
}

#[test]
fn invalid_size_3_selected() {
    corrupt(28, true, Failure::Size);
}

#[test]
fn fhcrc_corrupt_33_selected() {
    let mut input = fixture("flags_31");
    input[33] ^= 1;
    reader_error(read(&input, true, &mut budget()), Failure::Header);
}

#[test]
fn fhcrc_corrupt_34_selected() {
    let mut input = fixture("flags_31");
    input[34] ^= 1;
    reader_error(read(&input, true, &mut budget()), Failure::Header);
}

#[test]
fn fhcrc_corrupt_16_selected() {
    let mut input = fixture("flags_31");
    input[16] ^= 1;
    reader_error(read(&input, true, &mut budget()), Failure::Header);
}

#[test]
fn trailing_zero_selected() {
    suffix(&[0], true);
}

#[test]
fn trailing_ff_selected() {
    suffix(&[255], true);
}

#[test]
fn trailing_padding_selected() {
    suffix(&[0; 512], true);
}

#[test]
fn trailing_member_selected() {
    suffix(&normal(), true);
}

#[test]
fn trailing_empty_member_selected() {
    suffix(&fixture("empty_fixed"), true);
}

#[test]
fn trailing_magic_selected() {
    suffix(&[0x1f, 0x8b], true);
}

#[test]
fn crc_before_size_and_trailing_selected() {
    let mut input = normal();
    input[21] ^= 1;
    input[25] ^= 1;
    input.push(0);
    reader_error(read(&input, true, &mut budget()), Failure::Crc);
}
#[test]
fn size_before_trailing_selected() {
    let mut input = normal();
    input[25] ^= 1;
    input.push(0);
    reader_error(read(&input, true, &mut budget()), Failure::Size);
}
#[test]
fn reserved_deflate_block_selected() {
    let mut input = fixture("empty_fixed");
    input[10] = 7;
    assert!(matches!(
        read(&input, true, &mut budget()),
        Err(Error::Backend(InflateError::DataError))
    ));
}
#[test]
fn input_without_deflate_selected() {
    reader_error(
        read(&normal()[..10], true, &mut budget()),
        Failure::Truncated,
    );
}

#[test]
fn empty_fixed_unselected() {
    assert_valid("empty_fixed", false, b"", 10, 2, 0);
}

#[test]
fn empty_stored_unselected() {
    assert_valid("empty_stored", false, b"", 10, 5, 0);
}

#[test]
fn zeros_three_unselected() {
    assert_valid("zeros_three", false, &[0; 3], 10, 5, 4282505490);
}

#[test]
fn byte_a_unselected() {
    assert_valid("byte_a", false, b"A", 10, 6, 3554254475);
}

#[test]
fn opaque_name_unselected() {
    assert_valid("opaque_name", false, b"123456789", 20, 11, 3421780262);
}

#[test]
fn empty_options_unselected() {
    assert_valid("empty_options", false, b"123456789", 16, 11, 3421780262);
}

#[test]
fn large_output_unselected() {
    assert_valid(
        "large_output",
        false,
        &vec![b'A'; 200003],
        10,
        212,
        1109513937,
    );
}

#[test]
fn multiple_input_unselected() {
    assert_valid(
        "multiple_input",
        false,
        &(0..65537).map(|i| (i % 251) as u8).collect::<Vec<_>>(),
        10,
        65547,
        2848747123,
    );
}

#[test]
fn invalid_header_0_unselected() {
    corrupt(0, false, Failure::Header);
}

#[test]
fn invalid_header_1_unselected() {
    corrupt(1, false, Failure::Header);
}

#[test]
fn invalid_header_2_unselected() {
    corrupt(2, false, Failure::Header);
}

#[test]
fn reserved_flag_32_unselected() {
    let mut input = normal();
    input[3] = 32;
    reader_error(read(&input, false, &mut budget()), Failure::Header);
}

#[test]
fn reserved_flag_64_unselected() {
    let mut input = normal();
    input[3] = 64;
    reader_error(read(&input, false, &mut budget()), Failure::Header);
}

#[test]
fn reserved_flag_128_unselected() {
    let mut input = normal();
    input[3] = 128;
    reader_error(read(&input, false, &mut budget()), Failure::Header);
}

#[test]
fn invalid_crc_0_unselected() {
    corrupt(21, false, Failure::Crc);
}

#[test]
fn invalid_crc_1_unselected() {
    corrupt(22, false, Failure::Crc);
}

#[test]
fn invalid_crc_2_unselected() {
    corrupt(23, false, Failure::Crc);
}

#[test]
fn invalid_crc_3_unselected() {
    corrupt(24, false, Failure::Crc);
}

#[test]
fn invalid_size_0_unselected() {
    corrupt(25, false, Failure::Size);
}

#[test]
fn invalid_size_1_unselected() {
    corrupt(26, false, Failure::Size);
}

#[test]
fn invalid_size_2_unselected() {
    corrupt(27, false, Failure::Size);
}

#[test]
fn invalid_size_3_unselected() {
    corrupt(28, false, Failure::Size);
}

#[test]
fn fhcrc_corrupt_33_unselected() {
    let mut input = fixture("flags_31");
    input[33] ^= 1;
    reader_error(read(&input, false, &mut budget()), Failure::Header);
}

#[test]
fn fhcrc_corrupt_34_unselected() {
    let mut input = fixture("flags_31");
    input[34] ^= 1;
    reader_error(read(&input, false, &mut budget()), Failure::Header);
}

#[test]
fn fhcrc_corrupt_16_unselected() {
    let mut input = fixture("flags_31");
    input[16] ^= 1;
    reader_error(read(&input, false, &mut budget()), Failure::Header);
}

#[test]
fn trailing_zero_unselected() {
    suffix(&[0], false);
}

#[test]
fn trailing_ff_unselected() {
    suffix(&[255], false);
}

#[test]
fn trailing_padding_unselected() {
    suffix(&[0; 512], false);
}

#[test]
fn trailing_member_unselected() {
    suffix(&normal(), false);
}

#[test]
fn trailing_empty_member_unselected() {
    suffix(&fixture("empty_fixed"), false);
}

#[test]
fn trailing_magic_unselected() {
    suffix(&[0x1f, 0x8b], false);
}

#[test]
fn crc_before_size_and_trailing_unselected() {
    let mut input = normal();
    input[21] ^= 1;
    input[25] ^= 1;
    input.push(0);
    reader_error(read(&input, false, &mut budget()), Failure::Crc);
}
#[test]
fn size_before_trailing_unselected() {
    let mut input = normal();
    input[25] ^= 1;
    input.push(0);
    reader_error(read(&input, false, &mut budget()), Failure::Size);
}
#[test]
fn reserved_deflate_block_unselected() {
    let mut input = fixture("empty_fixed");
    input[10] = 7;
    assert!(matches!(
        read(&input, false, &mut budget()),
        Err(Error::Backend(InflateError::DataError))
    ));
}
#[test]
fn input_without_deflate_unselected() {
    reader_error(
        read(&normal()[..10], false, &mut budget()),
        Failure::Truncated,
    );
}

#[test]
fn read_ahead_is_not_recharged() {
    let input = normal();
    let mut budget = budget();
    let done = read(&input, true, &mut budget).unwrap();
    assert_eq!(done.boundary.read_ahead_bytes, 8);
    assert_eq!(done.boundary.offered_bytes, 19);
    assert_eq!(done.usage.processed, 38);
}

#[test]
fn header_truncation_charges_attempt_before_eof() {
    let mut budget = budget();
    reader_error(read(&[], false, &mut budget), Failure::Truncated);
    assert_eq!(budget.usage().metadata, 1);
    assert_eq!(budget.usage().processed, 0);
}
