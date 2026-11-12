use std::{collections::BTreeSet, path::Path};
use zipelo_ui_memory_app::{
    archive_dialog::archive_filters,
    archive_present::{gzip_output_name, valid_output_leaf},
};

#[test]
fn archive_filter_contains_every_case_variant_and_all_files_option() {
    let filters = archive_filters();
    assert_eq!(filters.len(), 2);
    assert_eq!(filters[0].0, "압축파일");
    let expected = [
        "zip", "ziP", "zIp", "zIP", "Zip", "ZiP", "ZIp", "ZIP", "7z", "7Z", "tar", "taR", "tAr",
        "tAR", "Tar", "TaR", "TAr", "TAR", "gz", "gZ", "Gz", "GZ", "tgz", "tgZ", "tGz", "tGZ",
        "Tgz", "TgZ", "TGz", "TGZ",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    assert_eq!(
        filters[0].1.iter().cloned().collect::<BTreeSet<_>>(),
        expected
    );
    assert_eq!(filters[0].1.len(), expected.len());
    assert_eq!(filters[1], ("모든 파일", vec!["*".to_owned()]));
}

#[test]
fn gzip_suggested_names_are_portable_and_preserve_valid_stems() {
    for name in [
        ".gz",
        ".GZ",
        "report:2026.gz",
        "report?2026.gz",
        "CON.gz",
        "COM1.gz",
        "name..gz",
        "name .gz",
    ] {
        assert_eq!(
            gzip_output_name(Path::new(name)),
            "압축 해제 파일",
            "{name}"
        );
    }
    for (name, expected) in [
        ("report.gz", "report"),
        ("report.GZ", "report"),
        ("report.Gz", "report"),
        ("한글.gz", "한글"),
        ("input.archive", "input.archive"),
    ] {
        assert_eq!(gzip_output_name(Path::new(name)), expected);
    }
    for name in [
        "",
        ".",
        "..",
        "../file",
        "dir/file",
        "dir\\file",
        "a:b",
        "a*",
        "a?",
        "a|b",
        "a<b",
        "a>b",
        "a\"b",
        "NUL.txt",
        "name.",
        "name ",
        "control\n",
    ] {
        assert!(!valid_output_leaf(name), "{name:?}");
    }
    assert!(valid_output_leaf("사용자 이름.txt"));
}
