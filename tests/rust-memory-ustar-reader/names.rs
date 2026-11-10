use crate::support::*;
use std::convert::Infallible;
use zipelo_memory_ustar_reader::{Failure, Kind, NamePolicy, read};

#[test]
fn reject_parent() {
    reject(archive(&[Fixture::file("../a", &[])]), Failure::UnsafePath);
}

#[test]
fn reject_absolute() {
    reject(archive(&[Fixture::file("/root", &[])]), Failure::UnsafePath);
}

#[test]
fn reject_empty_segment() {
    reject(archive(&[Fixture::file("a//b", &[])]), Failure::UnsafePath);
}

#[test]
fn reject_dot() {
    reject(archive(&[Fixture::file("a/./b", &[])]), Failure::UnsafePath);
}

#[test]
fn reject_parent_segment() {
    reject(
        archive(&[Fixture::file("a/../b", &[])]),
        Failure::UnsafePath,
    );
}

#[test]
fn reject_backslash() {
    reject(archive(&[Fixture::file("a\\b", &[])]), Failure::UnsafePath);
}

#[test]
fn reject_empty() {
    reject(archive(&[Fixture::file("", &[])]), Failure::Integrity);
}

#[test]
fn reject_file_trailing_slash() {
    reject(archive(&[Fixture::file("a/", &[])]), Failure::UnsafePath);
}

#[test]
fn directory_removes_one_trailing_slash() {
    let result = run(archive(&[Fixture::directory("a/")]), &["0"]);
    assert_eq!(result.entries[0].name, "a");
    assert_eq!(result.entries[0].kind, Kind::Directory);
    assert!(result.files.is_empty());
}

#[test]
fn directory_rejects_repeated_trailing_slash() {
    reject(archive(&[Fixture::directory("a//")]), Failure::UnsafePath);
}

#[test]
fn directory_without_trailing_slash() {
    assert_eq!(
        run(archive(&[Fixture::directory("a")]), &[]).entries[0].name,
        "a"
    );
}

#[test]
fn linux_case_distinct() {
    let result = run(
        archive(&[Fixture::file("A", &[]), Fixture::file("a", &[])]),
        &["1", "0"],
    );
    assert_eq!(
        result
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "a"]
    );
}

#[test]
fn korean_and_jamo_original_bytes() {
    let names = ["한글.txt", "가", "가", "폴더/파일"];
    let fixtures = names
        .iter()
        .map(|name| Fixture::file(name, name.as_bytes()))
        .collect::<Vec<_>>();
    let result = run(archive(&fixtures), &["3", "2", "1", "0"]);
    for (i, name) in names.iter().enumerate() {
        assert_eq!(result.entries[i].name, *name);
        assert_eq!(result.files[i].bytes, name.as_bytes());
    }
}

#[test]
fn linux_accepts_windows_reserved_and_trailing_characters() {
    let names = ["CON.txt", "C:/a", "a.", "a ", " ", "x:y", "line\nfeed"];
    let fixtures = names
        .iter()
        .map(|name| Fixture::file(name, &[]))
        .collect::<Vec<_>>();
    let result = run(archive(&fixtures), &[]);
    assert_eq!(
        result
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        names
    );
}

#[test]
fn windows_policy_unsupported() {
    let mut input = request(archive(&[Fixture::file("a", &[])]));
    input.policy = NamePolicy::Windows;
    let mut selected = false;
    failure(
        read(
            input,
            |_| {
                selected = true;
                Ok::<_, Infallible>(vec![])
            },
            |_| Ok(()),
        ),
        Failure::Unsupported,
    );
    assert!(!selected);
}

#[test]
fn prefix_join() {
    let prefix = format!("{}/{}", "p".repeat(80), "q".repeat(60));
    let result = run(
        archive(&[Fixture::file("a", b"ok").prefix(&prefix)]),
        &["0"],
    );
    assert_eq!(result.entries[0].name, format!("{prefix}/a"));
    assert_eq!(result.files[0].bytes, b"ok");
}

#[test]
fn maximum_header_name_and_prefix_bytes() {
    let leaf = "a".repeat(100);
    let prefix = "p".repeat(155);
    let result = run(archive(&[Fixture::file(&leaf, &[]).prefix(&prefix)]), &[]);
    assert_eq!(result.entries[0].name, format!("{prefix}/{leaf}"));
    assert_eq!(result.entries[0].name.len(), 256);
    assert_eq!(result.entries[0].name.encode_utf16().count(), 256);
}

#[test]
fn utf8_header_field_boundaries_preserve_text() {
    let leaf = format!("{}a", "한".repeat(33));
    let prefix = format!("{}ab", "ᄀ".repeat(51));
    assert_eq!(leaf.len(), 100);
    assert_eq!(prefix.len(), 155);
    let result = run(archive(&[Fixture::file(&leaf, &[]).prefix(&prefix)]), &[]);
    assert_eq!(
        result.entries[0].name.as_bytes(),
        format!("{prefix}/{leaf}").as_bytes()
    );
}

#[test]
fn supplementary_characters_preserve_utf16_units() {
    let leaf = "\u{10400}".repeat(25);
    let result = run(archive(&[Fixture::file(&leaf, &[])]), &[]);
    assert_eq!(result.entries[0].name.len(), 100);
    assert_eq!(result.entries[0].name.encode_utf16().count(), 50);
    assert_eq!(result.entries[0].name, leaf);
}

#[test]
fn utf8_split_at_name_boundary() {
    let bytes = archive(&[Fixture::file(&"a".repeat(100), &[])]);
    reject(field(bytes, 99, 1, &[0xe3]), Failure::Integrity);
}

#[test]
fn utf8_split_at_prefix_boundary() {
    let bytes = archive(&[Fixture::file("a", &[]).prefix(&"p".repeat(155))]);
    reject(field(bytes, 499, 1, &[0xe3]), Failure::Integrity);
}

#[test]
fn utf8_rejects_overlong_surrogate_and_out_of_range() {
    for invalid in [
        &[0xc0, 0xaf][..],
        &[0xed, 0xa0, 0x80][..],
        &[0xf4, 0x90, 0x80, 0x80][..],
    ] {
        reject(
            field(archive(&[Fixture::file("a", &[])]), 0, 100, invalid),
            Failure::Integrity,
        );
    }
}

#[test]
fn unselected_unsafe_path_blocks_selection() {
    reject(
        archive(&[Fixture::file("ok", &[1]), Fixture::file("../bad", &[])]),
        Failure::UnsafePath,
    );
}

#[test]
fn conflict_duplicate() {
    reject(
        archive(&[Fixture::file("a", &[]), Fixture::file("a", &[])]),
        Failure::Conflict,
    );
}

#[test]
fn conflict_directory_file_same() {
    reject(
        archive(&[Fixture::directory("a/"), Fixture::file("a", &[])]),
        Failure::Conflict,
    );
}

#[test]
fn conflict_parent_first() {
    reject(
        archive(&[Fixture::file("a", &[]), Fixture::file("a/b", &[])]),
        Failure::Conflict,
    );
}

#[test]
fn conflict_child_first() {
    reject(
        archive(&[Fixture::file("a/b", &[]), Fixture::file("a", &[])]),
        Failure::Conflict,
    );
}

#[test]
fn directory_first() {
    assert_eq!(
        run(
            archive(&[Fixture::directory("a/"), Fixture::file("a/b", &[1])]),
            &[]
        )
        .entries
        .len(),
        2
    );
}

#[test]
fn directory_after_child() {
    assert_eq!(
        run(
            archive(&[Fixture::file("a/b", &[1]), Fixture::directory("a/")]),
            &[]
        )
        .entries
        .len(),
        2
    );
}

#[test]
fn prefix_siblings() {
    assert_eq!(
        run(
            archive(&[Fixture::file("a", &[1]), Fixture::file("ab/c", &[2])]),
            &[]
        )
        .entries
        .len(),
        2
    );
}

#[test]
fn long_conflict_exact() {
    let prefix = format!("{}/{}", "p".repeat(70), "q".repeat(70));
    let leaf = "a".repeat(80);
    reject(
        archive(&[
            Fixture::file(&leaf, &[]).prefix(&prefix),
            Fixture::file(&leaf, &[]).prefix(&prefix),
        ]),
        Failure::Conflict,
    );
}

#[test]
fn long_conflict_parent_first() {
    let prefix = format!("{}/{}", "p".repeat(70), "q".repeat(70));
    let leaf = "a".repeat(80);
    let child = format!("{leaf}/child");
    reject(
        archive(&[
            Fixture::file(&leaf, &[]).prefix(&prefix),
            Fixture::file(&child, &[]).prefix(&prefix),
        ]),
        Failure::Conflict,
    );
}

#[test]
fn long_conflict_child_first() {
    let prefix = format!("{}/{}", "p".repeat(70), "q".repeat(70));
    let leaf = "a".repeat(80);
    let child = format!("{leaf}/child");
    reject(
        archive(&[
            Fixture::file(&child, &[]).prefix(&prefix),
            Fixture::file(&leaf, &[]).prefix(&prefix),
        ]),
        Failure::Conflict,
    );
}

#[test]
fn long_conflict_directory_file_same() {
    let prefix = format!("{}/{}", "p".repeat(70), "q".repeat(70));
    let leaf = "a".repeat(80);
    let directory = format!("{leaf}/");
    reject(
        archive(&[
            Fixture::directory(&directory).prefix(&prefix),
            Fixture::file(&leaf, &[]).prefix(&prefix),
        ]),
        Failure::Conflict,
    );
}

#[test]
fn long_siblings_directories_false() {
    let prefix = format!("{}/{}", "p".repeat(70), "q".repeat(70));
    let leaf = "a".repeat(80);
    let sibling = format!("{leaf}x");
    let directory = format!("{leaf}-dir/");
    let child = format!("{leaf}-dir/child");
    let mut entries = vec![
        Fixture::file(&leaf, &[1]).prefix(&prefix),
        Fixture::file(&sibling, &[2]).prefix(&prefix),
        Fixture::directory(&directory).prefix(&prefix),
        Fixture::file(&child, &[3]).prefix(&prefix),
    ];
    if false {
        entries.reverse();
    }
    let result = run(archive(&entries), &["0", "1", "2", "3"]);
    assert_eq!(result.entries.len(), 4);
    assert_eq!(result.files.len(), 3);
    assert_eq!(result.usage.output, 3);
}

#[test]
fn long_case_parent_distinct_false() {
    let prefix = format!("{}/{}", "p".repeat(70), "q".repeat(70));
    let upper = prefix.to_uppercase();
    let leaf = "a".repeat(80);
    let parent = leaf.to_uppercase();
    let child = format!("{leaf}/child");
    let mut entries = vec![
        Fixture::file(&parent, &[]).prefix(&upper),
        Fixture::file(&child, &[]).prefix(&prefix),
    ];
    if false {
        entries.reverse();
    }
    assert_eq!(run(archive(&entries), &[]).entries.len(), 2);
}

#[test]
fn long_siblings_directories_true() {
    let prefix = format!("{}/{}", "p".repeat(70), "q".repeat(70));
    let leaf = "a".repeat(80);
    let sibling = format!("{leaf}x");
    let directory = format!("{leaf}-dir/");
    let child = format!("{leaf}-dir/child");
    let mut entries = vec![
        Fixture::file(&leaf, &[1]).prefix(&prefix),
        Fixture::file(&sibling, &[2]).prefix(&prefix),
        Fixture::directory(&directory).prefix(&prefix),
        Fixture::file(&child, &[3]).prefix(&prefix),
    ];
    if true {
        entries.reverse();
    }
    let result = run(archive(&entries), &["0", "1", "2", "3"]);
    assert_eq!(result.entries.len(), 4);
    assert_eq!(result.files.len(), 3);
    assert_eq!(result.usage.output, 3);
}

#[test]
fn long_case_parent_distinct_true() {
    let prefix = format!("{}/{}", "p".repeat(70), "q".repeat(70));
    let upper = prefix.to_uppercase();
    let leaf = "a".repeat(80);
    let parent = leaf.to_uppercase();
    let child = format!("{leaf}/child");
    let mut entries = vec![
        Fixture::file(&parent, &[]).prefix(&upper),
        Fixture::file(&child, &[]).prefix(&prefix),
    ];
    if true {
        entries.reverse();
    }
    assert_eq!(run(archive(&entries), &[]).entries.len(), 2);
}

#[test]
fn long_case_exact_distinct() {
    let prefix = format!("{}/{}", "p".repeat(70), "q".repeat(70));
    let upper = prefix.to_uppercase();
    let leaf = "a".repeat(80);
    let upper_leaf = leaf.to_uppercase();
    assert_eq!(
        run(
            archive(&[
                Fixture::file(&leaf, &[]).prefix(&prefix),
                Fixture::file(&upper_leaf, &[]).prefix(&upper)
            ]),
            &[]
        )
        .entries
        .len(),
        2
    );
}

#[test]
fn long_maximum_siblings() {
    let prefix = format!("{}/{}", "p".repeat(70), "q".repeat(70));
    let names = (0..1024)
        .map(|i| format!("{}{i:04}", "a".repeat(80)))
        .collect::<Vec<_>>();
    let fixtures = names
        .iter()
        .enumerate()
        .map(|(i, name)| Fixture::file(name, &[i as u8]).prefix(&prefix))
        .collect::<Vec<_>>();
    let bytes = archive(&fixtures);
    let len = bytes.len() as u64;
    let result = run(bytes, &["1023"]);
    assert_eq!(result.entries.len(), 1024);
    assert_eq!(result.usage.entries, 1024);
    assert_eq!(result.usage.processed, len * 2);
    assert_eq!(result.usage.output, 1);
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].id, "1023");
    assert_eq!(result.files[0].bytes, [255]);
}
