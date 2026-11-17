use super::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn check_case(case: &str, safe: bool) {
    run_case(case, safe, false);
}

pub(super) fn check_extraction(case: &str, safe: bool) {
    run_case(case, safe, true);
}

fn run_case(case: &str, safe: bool, extraction: bool) {
    run_case_with_policy(case, safe, extraction, None);
}

fn run_case_with_policy(case: &str, safe: bool, extraction: bool, policy: Option<&str>) {
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
        "ldap-accounts" => "passwd: files ldap\n",
        "winbind-accounts" => "passwd: files winbind\n",
        "unknown-accounts" => "passwd: files unknown\n",
        "short-circuit-policy" => "passwd: files [SUCCESS=return] systemd\n",
        "duplicate-policy" => "passwd: files\npasswd: files\n",
        "missing-policy" => "group: files\n",
        _ if case.starts_with("sss-") || case == "systemd-remote" => "passwd: files systemd sss\n",
        _ if case.starts_with("systemd-") || case.starts_with("userdb-") => {
            "passwd: files systemd\n"
        }
        _ => "# fixture\npasswd: files # local accounts\n",
    };
    let nss = if case == "missing-policy" {
        nss.to_owned()
    } else {
        format!("{nss}group: files\n")
    };
    let nss_path = scratch.0.join("nsswitch.conf");
    fs::write(&nss_path, policy.unwrap_or(&nss)).unwrap();
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
    let userdb = scratch.0.join("userdb");
    let _services = FakeUserdb::new(&userdb, case, uid, gid);
    let library = shim();
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
        .env("ZIPELO_GROUP_USERDB", &userdb)
        .env("ZIPELO_GROUP_OUTPUT", &output)
        .env("ZIPELO_GROUP_SAFE", if safe { "1" } else { "0" })
        .env("ZIPELO_GROUP_EXTRACT", if extraction { "1" } else { "0" })
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{case} policy={policy:?}: {} {}",
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
fn private_group_checks_varlink_records_and_completion() {
    for case in [
        "systemd-personal",
        "systemd-unrelated",
        "systemd-large-record",
        "systemd-ignored-relays",
        "systemd-multipart-empty",
        "systemd-fragmented",
    ] {
        check_extraction(case, true);
    }
    for case in [
        "systemd-dynamic-shared",
        "systemd-homed-shared",
        "systemd-large-shared",
        "systemd-partial-error",
        "systemd-group-error",
        "systemd-membership-error",
        "systemd-enumeration-error",
        "systemd-disconnect",
        "systemd-invalid-json",
        "systemd-duplicate-key",
        "systemd-missing-gid",
        "systemd-wrong-type",
        "systemd-group-member",
        "systemd-membership-member",
        "systemd-user-member",
        "systemd-incomplete",
        "systemd-overrides",
        "systemd-non-socket",
        "systemd-connect-error",
        "systemd-buffer-limit",
        "systemd-final-shared",
        "systemd-unknown-membership",
        "systemd-malformed-membership",
        "systemd-second-service-error",
        "systemd-socket-symlink",
        "systemd-deep-json",
        "systemd-invalid-utf8",
        "systemd-negative-gid",
        "systemd-float-gid",
        "systemd-nested-duplicate",
        "systemd-bad-continues",
    ] {
        check_extraction(case, false);
    }
}

#[test]
fn private_group_checks_systemd_in_group_and_initgroups_policies() {
    for policy in [
        "passwd: files\ngroup: files systemd\n",
        "passwd: files\ngroup: files systemd\ninitgroups: files\n",
        "passwd: files\ngroup: files\ninitgroups: files systemd\n",
    ] {
        for case in ["systemd-personal", "systemd-unrelated"] {
            run_case_with_policy(case, true, true, Some(policy));
        }
        for case in [
            "systemd-dynamic-shared",
            "systemd-group-member",
            "systemd-membership-member",
            "systemd-user-member",
            "systemd-partial-error",
            "userdb-d0",
        ] {
            run_case_with_policy(case, false, true, Some(policy));
        }
    }
}

#[test]
fn private_group_checks_sss_in_group_and_initgroups_policies() {
    for policy in [
        "passwd: files\ngroup: files sss\n",
        "passwd: files\ngroup: files sss\ninitgroups: files\n",
        "passwd: files\ngroup: files\ninitgroups: files sss\n",
    ] {
        run_case_with_policy("sss-empty", true, true, Some(policy));
        for case in [
            "sss-socket",
            "sss-passwd-cache",
            "sss-group-cache",
            "sss-initgr-cache",
            "sss-probe-error",
        ] {
            run_case_with_policy(case, false, true, Some(policy));
        }
    }
}

#[test]
fn private_group_rejects_incomplete_group_and_initgroups_policies() {
    for policy in [
        "passwd: files\n",
        "passwd: files\ngroup: files\ngroup: files\n",
        "passwd: files\ngroup: files\ninitgroups: files\ninitgroups: files\n",
    ] {
        run_case_with_policy("personal", false, true, Some(policy));
    }
    for source in [
        "",
        "files ldap",
        "files winbind",
        "files unknown",
        "files [SUCCESS=return] systemd",
        "files systemd [UNAVAIL=continue]",
        "files files",
    ] {
        for policy in [
            format!("passwd: files\ngroup: {source}\n"),
            format!("passwd: files\ngroup: files\ninitgroups: {source}\n"),
        ] {
            run_case_with_policy("personal", false, true, Some(&policy));
        }
    }
    run_case_with_policy(
        "personal",
        true,
        true,
        Some("passwd: files\ngroup: files\ninitgroups: files # local groups\n"),
    );
}

#[test]
fn private_group_rejects_consumed_systemd_record_on_erange() {
    // NSS 버퍼 재시도 대신 완전한 varlink 레코드를 직접 검사함.
    check_extraction("systemd-large-record", true);
    check_extraction("systemd-large-shared", false);
}

#[test]
fn private_group_rejects_userdb_dropins_and_probe_failures() {
    for case in ["userdb-empty", "userdb-missing"] {
        check_extraction(case, true);
    }
    for case in [
        "userdb-d0",
        "userdb-d1",
        "userdb-host",
        "userdb-d3",
        "userdb-d4",
        "userdb-probe-error",
        "userdb-symlink",
        "userdb-invalid-parent",
    ] {
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

fn shim() -> &'static PathBuf {
    static LIBRARY: std::sync::OnceLock<(Scratch, PathBuf)> = std::sync::OnceLock::new();
    &LIBRARY
        .get_or_init(|| {
            let scratch = Scratch::new();
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
            (scratch, library)
        })
        .1
}

struct FakeUserdb {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl FakeUserdb {
    fn new(root: &std::path::Path, case: &str, uid: u32, gid: u32) -> Self {
        use std::os::fd::AsRawFd;
        let sockets = root.join("s");
        fs::create_dir_all(&sockets).unwrap();
        for d in ["d0", "d1", "host/userdb", "d3", "d4"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        if case == "userdb-missing" {
            fs::remove_dir_all(root.join("host")).unwrap();
        }
        if let Some(d) = case.strip_prefix("userdb-") {
            let d = if d == "host" { "host/userdb" } else { d };
            if root.join(d).is_dir() {
                fs::write(root.join(d).join("fixture.user"), b"{}").unwrap();
            }
        }
        if case == "userdb-symlink" {
            fs::remove_dir(root.join("d0")).unwrap();
            std::os::unix::fs::symlink("missing", root.join("d0")).unwrap();
        }
        if case == "userdb-invalid-parent" {
            fs::remove_dir(root.join("d0")).unwrap();
            fs::write(root.join("d0"), b"").unwrap();
        }
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut result = Self {
            stop: stop.clone(),
            thread: None,
        };
        if case == "systemd-non-socket" {
            fs::write(sockets.join("io.zipelo.Fixture"), b"").unwrap();
            return result;
        }
        let parent = fs::File::open(&sockets).unwrap();
        let path = format!("/proc/self/fd/{}/io.zipelo.Fixture", parent.as_raw_fd());
        let listener = std::os::unix::net::UnixListener::bind(path).unwrap();
        if case == "systemd-connect-error" {
            return result;
        }
        if case == "systemd-second-service-error" {
            drop(
                std::os::unix::net::UnixListener::bind(format!(
                    "/proc/self/fd/{}/io.zipelo.Failed",
                    parent.as_raw_fd()
                ))
                .unwrap(),
            );
        }
        if case == "systemd-socket-symlink" {
            std::os::unix::fs::symlink("io.zipelo.Fixture", sockets.join("io.zipelo.Link"))
                .unwrap();
        }
        let _relays = if case == "systemd-ignored-relays" {
            Some(
                ["io.systemd.Multiplexer", "io.systemd.NameServiceSwitch"].map(|v| {
                    std::os::unix::net::UnixListener::bind(format!(
                        "/proc/self/fd/{}/{v}",
                        parent.as_raw_fd()
                    ))
                    .unwrap()
                }),
            )
        } else {
            None
        };
        // 중계 소켓은 연결 불가 상태로 남겨 실제로 건너뛰는지 확인함.
        drop(_relays);
        listener.set_nonblocking(true).unwrap();
        let case = case.to_owned();
        result.thread = Some(std::thread::spawn(move || {
            use serde_json::{Value, json};
            use std::io::{Read, Write};
            while !stop.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                loop {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    if byte == [0] {
                        break;
                    }
                    request.push(byte[0]);
                }
                let request: Value = serde_json::from_slice(&request).unwrap();
                assert_eq!(request["parameters"]["service"], "io.zipelo.Fixture");
                assert_eq!(request["more"], true);
                let method = request["method"]
                    .as_str()
                    .unwrap()
                    .rsplit('.')
                    .next()
                    .unwrap();
                let no_record = json!({"error":"io.systemd.UserDatabase.NoRecordFound"});
                let failure = json!({"error":"io.systemd.UserDatabase.ServiceNotAvailable"});
                let mut reply = match method {
                    "GetGroupRecord" => {
                        json!({"parameters":{"record":{"groupName":"test-personal","gid":gid,"members":[]}}})
                    }
                    "GetUserRecord" => {
                        json!({"parameters":{"record":{"userName":"other","uid":uid+1,"gid":gid+1}}})
                    }
                    "GetMemberships" => no_record.clone(),
                    _ => panic!("unexpected method"),
                };
                if method == "GetGroupRecord" {
                    if case == "systemd-group-member" {
                        reply["parameters"]["record"]["members"] = json!(["other"]);
                    }
                    if case == "systemd-group-error" {
                        reply = failure.clone();
                    }
                }
                if method == "GetMemberships" {
                    if case == "systemd-membership-member" {
                        reply =
                            json!({"parameters":{"userName":"other","groupName":"test-personal"}});
                    }
                    if case == "systemd-unknown-membership" {
                        reply = json!({"parameters":{"userName":"other","groupName":"unknown"}});
                    }
                    if case == "systemd-malformed-membership" {
                        reply = json!({"parameters":{"userName":123,"groupName":"test-personal"}});
                    }
                    if case == "systemd-membership-error" {
                        reply = failure.clone();
                    }
                }
                if method == "GetUserRecord" {
                    if [
                        "systemd-dynamic-shared",
                        "systemd-homed-shared",
                        "systemd-large-shared",
                    ]
                    .contains(&case.as_str())
                    {
                        reply["parameters"]["record"]["gid"] = json!(gid);
                    }
                    if case == "systemd-large-record" || case == "systemd-large-shared" {
                        reply["parameters"]["record"]["realName"] = json!("x".repeat(32768));
                    }
                    if case == "systemd-buffer-limit" {
                        reply["parameters"]["record"]["realName"] = json!("x".repeat(1024 * 1024));
                    }
                    if case == "systemd-incomplete" {
                        reply["parameters"]["incomplete"] = json!(true);
                    }
                    if case == "systemd-overrides" {
                        reply["parameters"]["record"]["binding"] = json!({"machine":{"gid":gid}});
                    }
                    if case == "systemd-missing-gid" {
                        reply["parameters"]["record"]
                            .as_object_mut()
                            .unwrap()
                            .remove("gid");
                    }
                    if case == "systemd-wrong-type" {
                        reply["parameters"]["record"]["gid"] = json!("1000");
                    }
                    if case == "systemd-user-member" {
                        reply["parameters"]["record"]["memberOf"] = json!(["test-personal"]);
                    }
                    if case == "systemd-personal" {
                        reply = no_record.clone();
                    }
                    if case == "systemd-enumeration-error" {
                        reply = failure.clone();
                    }
                    if [
                        "systemd-partial-error",
                        "systemd-disconnect",
                        "systemd-final-shared",
                    ]
                    .contains(&case.as_str())
                    {
                        reply["continues"] = json!(true);
                    }
                    if case == "systemd-invalid-json" {
                        let _ = stream.write_all(b"{not-json}\0");
                        continue;
                    }
                    if case == "systemd-duplicate-key" {
                        let _ = stream.write_all(b"{\"error\":\"failure\",\"error\":\"io.systemd.UserDatabase.NoRecordFound\"}\0");
                        continue;
                    }
                }
                if method == "GetUserRecord" {
                    if case == "systemd-negative-gid" {
                        reply["parameters"]["record"]["gid"] = json!(-1);
                    }
                    if case == "systemd-float-gid" {
                        reply["parameters"]["record"]["gid"] = json!(1000.0);
                    }
                    if case == "systemd-bad-continues" {
                        reply["continues"] = json!("true");
                    }
                    if case == "systemd-multipart-empty" {
                        reply["continues"] = json!(true);
                    }
                    if case == "systemd-invalid-utf8" {
                        let _ = stream.write_all(b"\xff\0");
                        continue;
                    }
                    if case == "systemd-nested-duplicate" {
                        let _ = stream.write_all(b"{\"parameters\":{\"record\":{\"userName\":\"other\",\"uid\":1001,\"gid\":1000,\"gid\":1001}}}\0");
                        continue;
                    }
                    if case == "systemd-deep-json" {
                        let data = format!("{}0{}\0", "[".repeat(140), "]".repeat(140));
                        let _ = stream.write_all(data.as_bytes());
                        continue;
                    }
                }
                let mut bytes = serde_json::to_vec(&reply).unwrap();
                bytes.push(0);
                if method == "GetUserRecord" && case == "systemd-partial-error" {
                    bytes.extend(serde_json::to_vec(&failure).unwrap());
                    bytes.push(0);
                }
                if method == "GetUserRecord" && case == "systemd-final-shared" {
                    reply["parameters"]["record"]["gid"] = json!(gid);
                    reply["continues"] = json!(false);
                    bytes.extend(serde_json::to_vec(&reply).unwrap());
                    bytes.push(0);
                }
                if method == "GetUserRecord" && case == "systemd-multipart-empty" {
                    bytes.extend(serde_json::to_vec(&no_record).unwrap());
                    bytes.push(0);
                }
                if case == "systemd-fragmented" {
                    for byte in bytes {
                        if stream.write_all(&[byte]).is_err() {
                            break;
                        }
                    }
                } else {
                    let _ = stream.write_all(&bytes);
                }
            }
        }));
        result
    }
}
impl Drop for FakeUserdb {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
