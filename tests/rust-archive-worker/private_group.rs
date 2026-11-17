use super::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn check_case(case: &str, safe: bool) {
    run_case(case, safe, false);
}

pub(super) fn check_extraction(case: &str, safe: bool) {
    run_case(case, safe, true);
}

fn run_case(case: &str, safe: bool, extraction: bool) {
    let scratch = Scratch::new();
    let output = scratch.0.join("output");
    fs::create_dir(&output).unwrap();
    let mode = if case == "world-writable" {
        0o777
    } else {
        0o775
    };
    fs::set_permissions(&output, fs::Permissions::from_mode(mode)).unwrap();
    if extraction {
        fs::write(
            scratch.0.join("input"),
            include_bytes!("fixtures/store.zip"),
        )
        .unwrap();
    }
    let metadata = fs::metadata(&output).unwrap();
    let uid = metadata.uid();
    let gid = metadata.gid();
    let mut passwd = format!("owner:x:{uid}:{gid}::/:/bin/sh\n");
    match case {
        "shared-primary" => passwd.push_str(&format!(
            "other:x:{}:{gid}::/:/bin/sh\n",
            uid.wrapping_add(1)
        )),
        "unrelated-primary" => passwd.push_str(&format!(
            "other:x:{}:{}::/:/bin/sh\n",
            uid.wrapping_add(1),
            gid.wrapping_add(1)
        )),
        "missing-owner" => passwd.clear(),
        "malformed-passwd" => passwd.push_str("broken:x:not-a-uid:1000::/:/bin/sh\n"),
        "nis-passwd" => passwd.push_str("+::::::\n"),
        _ => {}
    }
    let passwd_path = scratch.0.join("passwd");
    fs::write(&passwd_path, passwd).unwrap();
    if case == "passwd-unavailable" {
        fs::remove_file(&passwd_path).unwrap();
    }
    let nss = match case {
        "remote-accounts" | "sss-files-only" => "passwd: files sss\n",
        "systemd-personal"
        | "systemd-dynamic-shared"
        | "systemd-homed-shared"
        | "systemd-unrelated"
        | "systemd-large-record"
        | "systemd-large-shared"
        | "systemd-buffer-limit"
        | "systemd-enumeration-error"
        | "systemd-set-error"
        | "systemd-load-error"
        | "systemd-symbol-error"
        | "systemd-partial-error" => "passwd: files systemd\n",
        "ldap-accounts" => "passwd: files ldap\n",
        "winbind-accounts" => "passwd: files winbind\n",
        "systemd-remote" => "passwd: files systemd sss\n",
        _ if case.starts_with("sss-") => "passwd: files systemd sss\n",
        "unknown-accounts" => "passwd: files unknown\n",
        "short-circuit-policy" => "passwd: files [SUCCESS=return] systemd\n",
        "duplicate-policy" => "passwd: files\npasswd: files\n",
        "missing-policy" => "group: files\n",
        _ => "# fixture\npasswd: files # local accounts\n",
    };
    let nss_path = scratch.0.join("nsswitch.conf");
    fs::write(&nss_path, nss).unwrap();
    if case == "nss-unavailable" {
        fs::remove_file(&nss_path).unwrap();
    }
    let sss = scratch.0.join("sss");
    fs::create_dir_all(sss.join("pipes")).unwrap();
    fs::create_dir_all(sss.join("mc")).unwrap();
    let _socket = match case {
        "sss-socket" | "remote-accounts" | "systemd-remote" => {
            use std::os::fd::AsRawFd;
            let pipes = fs::File::open(sss.join("pipes")).unwrap();
            let path = format!("/proc/self/fd/{}/nss", pipes.as_raw_fd());
            Some(std::os::unix::net::UnixListener::bind(path).unwrap())
        }
        _ => None,
    };
    match case {
        "sss-passwd-cache" => fs::write(sss.join("mc/passwd"), b"").unwrap(),
        "sss-group-cache" => fs::write(sss.join("mc/group"), b"").unwrap(),
        "sss-initgr-cache" => fs::write(sss.join("mc/initgr"), b"").unwrap(),
        "sss-dangling-socket" => {
            std::os::unix::fs::symlink("missing", sss.join("pipes/nss")).unwrap();
        }
        "sss-dangling-cache" => {
            std::os::unix::fs::symlink("missing", sss.join("mc/passwd")).unwrap();
        }
        "sss-cache-symlink" => {
            fs::remove_dir(sss.join("mc")).unwrap();
            std::os::unix::fs::symlink("missing", sss.join("mc")).unwrap();
        }
        "sss-invalid-parent" => {
            fs::remove_dir(sss.join("mc")).unwrap();
            fs::write(sss.join("mc"), b"").unwrap();
        }
        "sss-missing-directories" => fs::remove_dir_all(&sss).unwrap(),
        _ => {}
    }
    let library = scratch.0.join("private-group.so");
    assert!(
        Command::new("cc")
            .args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"])
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/rust-archive-worker/private_group_nss.c")
            )
            .args(["-ldl", "-o"])
            .arg(&library)
            .status()
            .unwrap()
            .success()
    );
    let result = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "private_group::probe_child",
            "--test-threads=1",
            "--nocapture",
        ])
        .env("LD_PRELOAD", library)
        .env("ZIPELO_GROUP_CASE", case)
        .env("ZIPELO_GROUP_PASSWD", passwd_path)
        .env("ZIPELO_GROUP_NSS", nss_path)
        .env("ZIPELO_GROUP_SSS", sss)
        .env("ZIPELO_GROUP_OUTPUT", &output)
        .env("ZIPELO_GROUP_SAFE", if safe { "1" } else { "0" })
        .env("ZIPELO_GROUP_EXTRACT", if extraction { "1" } else { "0" })
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{case}: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    if extraction && safe {
        assert_eq!(fs::read(output.join("folder/hello.txt")).unwrap(), PAYLOAD);
    } else {
        assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    }
    assert_eq!(fs::metadata(&output).unwrap().mode() & 0o777, mode);
}

#[test]
fn probe_child() {
    let Some(path) = std::env::var_os("ZIPELO_GROUP_OUTPUT") else {
        return;
    };
    let path = std::path::Path::new(&path);
    let actual = zipelo_archive_worker::filesystem::Dir::root(path);
    if std::env::var("ZIPELO_GROUP_SAFE").unwrap() == "1" {
        assert!(actual.is_ok());
    } else {
        assert!(matches!(actual, Err(ErrorCode::UnsafePath)));
    }
    if std::env::var("ZIPELO_GROUP_EXTRACT").unwrap() == "1" {
        let events = execute(Start {
            action: Action::Extract {
                archive: NativePath::from_path(&path.parent().unwrap().join("input")),
                destination: NativePath::from_path(path),
                selection: Selection::All,
                list_id: None,
                gzip_output_name: None,
            },
            limits: Limits::default(),
        });
        if std::env::var("ZIPELO_GROUP_SAFE").unwrap() == "1" {
            assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        } else {
            assert_eq!(terminal(&events).error, ErrorCode::UnsafePath, "{events:?}");
        }
    }
}

#[test]
fn private_group_accepts_only_current_primary_member() {
    for case in ["personal", "unrelated-primary"] {
        check_case(case, true);
    }
}

#[test]
fn private_group_rejects_another_primary_member() {
    check_case("shared-primary", false);
}

#[test]
fn private_group_rejects_incomplete_account_information() {
    for case in [
        "missing-owner",
        "malformed-passwd",
        "nis-passwd",
        "passwd-unavailable",
        "nss-unavailable",
        "remote-accounts",
        "unknown-accounts",
        "ldap-accounts",
        "winbind-accounts",
        "systemd-remote",
        "short-circuit-policy",
        "duplicate-policy",
        "missing-policy",
        "group-error",
        "group-missing",
        "group-null-members",
        "group-wrong-gid",
        "supplementary-member",
    ] {
        check_case(case, false);
    }
}

#[test]
fn private_group_checks_systemd_enumerated_accounts() {
    for case in ["systemd-personal", "systemd-unrelated"] {
        check_extraction(case, true);
    }
    for case in [
        "systemd-dynamic-shared",
        "systemd-homed-shared",
        "systemd-buffer-limit",
        "systemd-enumeration-error",
        "systemd-set-error",
        "systemd-load-error",
        "systemd-symbol-error",
        "systemd-partial-error",
    ] {
        check_extraction(case, false);
    }
    check_extraction("remote-accounts", false);
    check_extraction("systemd-remote", false);
}

#[test]
fn private_group_rejects_consumed_systemd_record_on_erange() {
    for case in ["systemd-large-record", "systemd-large-shared"] {
        check_extraction(case, false);
    }
}

#[test]
fn private_group_accepts_sss_only_without_socket_and_caches() {
    for case in ["sss-empty", "sss-files-only", "sss-missing-directories"] {
        check_extraction(case, true);
    }
    for case in [
        "sss-socket",
        "sss-passwd-cache",
        "sss-group-cache",
        "sss-initgr-cache",
        "sss-dangling-socket",
        "sss-dangling-cache",
        "sss-cache-symlink",
        "sss-invalid-parent",
        "sss-probe-error",
    ] {
        check_extraction(case, false);
    }
}
