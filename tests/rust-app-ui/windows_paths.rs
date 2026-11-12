use zipelo_ui_memory_app::archive_path::windows_path;

#[test]
fn ordinary_verbatim_drives_are_converted_without_changing_components() {
    for (input, expected) in [
        (r"\\?\C:\Users\사용자\one.txt", r"C:\Users\사용자\one.txt"),
        (r"\\?\C:\", r"C:\"),
        (r"D:\one.txt", r"D:\one.txt"),
    ] {
        assert_eq!(windows_path(input), expected);
    }
}

#[test]
fn unc_devices_and_ambiguous_components_remain_blocked() {
    for input in [
        r"\\?\UNC\server\share\one.txt",
        r"\\server\share\one.txt",
        r"\\.\C:\one.txt",
        r"\\?\GLOBALROOT\Device\one",
        r"\\?\C:\a\..\one",
        r"\\?\C:\a\.\one",
        r"\\?\C:\a\name.",
        r"\\?\C:\a\name ",
        r"\\?\C:\a\NUL",
        r"\\?\C:\a\one:stream",
        r"\\?\C:\a\\one",
        r"\\?\C:\a/one",
        r"\\?\C:one",
    ] {
        assert_eq!(windows_path(input), input);
    }
}

#[test]
fn creation_messages_use_save_vocabulary_and_preserve_warnings() {
    use zipelo_ui_memory_app::archive_present::*;
    for text in [
        UNCERTAIN,
        PARTIAL,
        LEFTOVERS,
        SYNC_FAILED,
        CLEANUP_WARNING,
        UNCONFIRMED_FINISH,
        error_text(zipelo_ui_memory_app::archive_ui::ErrorCode::InvalidRequest),
    ] {
        assert_eq!(operation_text(text, false), text);
        assert!(!operation_text(text, true).contains("풀 위치"));
    }
    assert!(operation_text(UNCERTAIN, true).contains("확인하지 못했습니다"));
}

#[cfg(windows)]
#[test]
fn windows_native_contract_accepts_drives_and_still_rejects_unc() {
    use std::path::Path;
    use worker_protocol::protocol::NativePath;
    use zipelo_ui_memory_app::archive_path::worker_path;
    let ordinary = worker_path(Path::new(r"\\?\C:\Users\one.txt"));
    assert_eq!(
        NativePath::from_path(&ordinary).to_path().unwrap(),
        Path::new(r"C:\Users\one.txt")
    );
    for text in [
        r"\\?\UNC\server\share\one.txt",
        r"\\server\share\one.txt",
        r"\\?\C:\a\..\one",
    ] {
        assert!(
            NativePath::from_path(&worker_path(Path::new(text)))
                .to_path()
                .is_err()
        );
    }
}
