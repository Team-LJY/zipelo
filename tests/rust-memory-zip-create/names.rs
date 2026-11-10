use super::support::*;
use zipelo_rust_memory_zip_create::{Failure, Method};

fn rejected(name: &str) {
    fail_before_output(
        request(Method::Stored, vec![entry(name, vec![])]),
        Failure::UnsafeName,
    );
}

#[test]
fn rejects_root_drive_backslash_and_dot_components() {
    for name in [
        "",
        "/root",
        "trailing/",
        "a//b",
        ".",
        "..",
        "a/./b",
        "a/../b",
        "C:/file",
        "C:file",
        "back\\slash",
        "\\\\server\\file",
    ] {
        rejected(name);
    }
}

#[test]
fn rejects_every_ascii_control_and_windows_dangerous_character() {
    for byte in (0u8..32).chain([127]) {
        rejected(&format!("a{}b", char::from(byte)));
    }
    for character in ['<', '>', ':', '"', '|', '?', '*'] {
        rejected(&format!("a{character}b"));
    }
}

#[test]
fn rejects_reserved_stems_case_extensions_and_trimmed_spaces() {
    let mut reserved = vec![
        "CON".to_owned(),
        "PRN".to_owned(),
        "AUX".to_owned(),
        "NUL".to_owned(),
        "CONIN$".to_owned(),
        "CONOUT$".to_owned(),
    ];
    for index in 1..=9 {
        reserved.push(format!("COM{index}"));
        reserved.push(format!("LPT{index}"));
    }
    for stem in reserved {
        for name in [
            stem.clone(),
            stem.to_ascii_lowercase(),
            format!("dir/{stem}.txt"),
            format!("{stem} .txt"),
        ] {
            rejected(&name);
        }
    }
}

#[test]
fn rejects_trailing_space_dot_and_space_only_components() {
    for name in ["a ", "a.", "a /b", "a./b", " ", "a/   /b"] {
        rejected(name);
    }
}

#[test]
fn rejects_unicode_including_korean_and_decomposed_names() {
    for name in ["한글.txt", "한.txt", "é.txt", "e\u{301}.txt", "COM¹.txt"] {
        rejected(name);
    }
}

#[test]
fn component_255_accepted_256_rejected() {
    let accepted = "a".repeat(255);
    assert_zip32(
        &run(request(Method::Stored, vec![entry(accepted, vec![])])),
        1,
    );
    rejected(&"a".repeat(256));
}

#[test]
fn full_name_1024_accepted_1025_rejected() {
    let name = [
        "a".repeat(255),
        "b".repeat(255),
        "c".repeat(255),
        "d".repeat(255),
    ]
    .join("/")
        + "/x";
    assert_eq!(name.len(), 1025);
    let accepted = [
        "a".repeat(255),
        "b".repeat(255),
        "c".repeat(255),
        "d".repeat(254),
        "x".to_owned(),
    ]
    .join("/");
    assert_eq!(accepted.len(), 1024);
    assert_zip32(
        &run(request(Method::Stored, vec![entry(accepted, vec![])])),
        1,
    );
    rejected(&name);
}

#[test]
fn safe_ascii_punctuation_and_nonreserved_stems_remain_accepted() {
    for name in [
        "a-b_c.txt",
        "dir/a b",
        ".hidden",
        "COM0",
        "COM10",
        "LPT0",
        "CONSOLE",
        " leading",
    ] {
        assert_zip32(&run(request(Method::Stored, vec![entry(name, vec![])])), 1);
    }
}

#[test]
fn exact_and_ascii_case_duplicates_conflict() {
    for pair in [
        ["same", "same"],
        ["Readme.txt", "README.TXT"],
        ["A/x", "a/X"],
    ] {
        fail_before_output(
            request(
                Method::Stored,
                pair.into_iter().map(|name| entry(name, vec![])).collect(),
            ),
            Failure::Conflict,
        );
    }
}

#[test]
fn ancestor_collision_is_rejected_in_both_orders_and_cases() {
    for pair in [
        ["file", "file/child"],
        ["file/child", "file"],
        ["FILE", "file/child"],
        ["file/child", "FILE"],
    ] {
        fail_before_output(
            request(
                Method::Stored,
                pair.into_iter().map(|name| entry(name, vec![])).collect(),
            ),
            Failure::Conflict,
        );
    }
}

#[test]
fn common_prefix_without_separator_is_not_a_collision() {
    let entries = ["file", "file2", "file2/child.txt"];
    let bytes = run(request(
        Method::Stored,
        entries[..2]
            .iter()
            .map(|name| entry(*name, vec![]))
            .collect(),
    ));
    assert_zip32(&bytes, 2);
    let bytes = run(request(
        Method::Stored,
        vec![entry("file", vec![]), entry(entries[2], vec![])],
    ));
    assert_zip32(&bytes, 2);
}

#[test]
fn sibling_directory_case_spelling_is_preserved() {
    let bytes = run(request(
        Method::Stored,
        vec![entry("A/x", vec![]), entry("a/y", vec![])],
    ));
    assert_files(&bytes, Method::Stored, &[("A/x", b"", 0), ("a/y", b"", 0)]);
}
