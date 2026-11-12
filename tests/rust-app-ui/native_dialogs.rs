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

#[cfg(target_os = "linux")]
#[test]
fn portal_response_distinguishes_cancellation_from_backend_failure() {
    use ashpd::{Error, PortalError, desktop::ResponseError};
    use zipelo_ui_memory_app::archive_dialog::{DIALOG_UNAVAILABLE, portal_response};
    for error in [
        Error::Response(ResponseError::Cancelled),
        Error::Portal(PortalError::Cancelled("cancelled".into())),
    ] {
        assert_eq!(portal_response(Err(error)), Ok(None));
    }
    for error in [
        Error::Response(ResponseError::Other),
        Error::Portal(PortalError::Failed("failed".into())),
        Error::Portal(PortalError::WindowDestroyed("destroyed".into())),
        Error::NoResponse,
        Error::PortalNotFound("org.freedesktop.portal.FileChooser".try_into().unwrap()),
        Error::IO(std::io::Error::other("backend unavailable")),
        Error::ParseError("invalid URI"),
    ] {
        assert_eq!(portal_response(Err(error)), Err(DIALOG_UNAVAILABLE.into()));
    }
    assert_eq!(portal_response(Ok(vec![])), Err(DIALOG_UNAVAILABLE.into()));
    let path = Path::new("/selected/archive.zip").to_path_buf();
    assert_eq!(
        portal_response(Ok(vec![path.clone()])),
        Ok(Some(vec![path]))
    );
}

#[cfg(target_os = "linux")]
#[test]
fn missing_portal_returns_errors_for_every_dialog() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    use zipelo_ui_memory_app::archive_dialog::{
        DIALOG_UNAVAILABLE, DialogRequest, Dialogs, NativeDialogs, Purpose,
    };
    if std::env::var_os("ZIPELO_NO_PORTAL_CHILD").is_some() {
        let mut dialogs = NativeDialogs;
        for purpose in [
            Purpose::Open,
            Purpose::Files,
            Purpose::Folder,
            Purpose::Destination,
            Purpose::Retry,
            Purpose::SaveZip,
            Purpose::SaveGzip,
            Purpose::RetryGzip,
        ] {
            let result = dialogs.choose(DialogRequest {
                purpose,
                directory: std::env::current_dir().unwrap(),
                file_name: "output.zip".into(),
            });
            assert_eq!(result, Err(DIALOG_UNAVAILABLE.into()), "{purpose:?}");
        }
        return;
    }
    let endpoint = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../tests/rust-app-ui/runs/no-bus-{}",
        std::process::id()
    ));
    assert!(!endpoint.exists());
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "native_dialogs::missing_portal_returns_errors_for_every_dialog",
            "--nocapture",
        ])
        .env("ZIPELO_NO_PORTAL_CHILD", "1")
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", endpoint.display()),
        )
        .env_remove("DBUS_STARTER_ADDRESS")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("portal 실패 응답 시간 초과");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
