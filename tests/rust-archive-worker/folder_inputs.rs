use super::*;
use sha2::{Digest, Sha256};
fn request(s: &Scratch, method: Method, encryption: Encryption) -> Start {
    Start {
        action: Action::CreateZip {
            sources: vec![
                Source {
                    path: s.path("tree"),
                    archive_name: "documents".into(),
                },
                Source {
                    path: s.path("loose"),
                    archive_name: "loose.bin".into(),
                },
            ],
            output_file: s.path("created.zip"),
            method,
            encryption,
        },
        limits: Limits::default(),
    }
}
fn seed(s: &Scratch) -> Vec<u8> {
    fs::create_dir_all(s.0.join("tree/nested")).unwrap();
    fs::create_dir(s.0.join("tree/empty")).unwrap();
    let bytes = (0..262157).map(|i| (i % 251) as u8).collect::<Vec<_>>();
    fs::write(s.0.join("tree/nested/한글.bin"), &bytes).unwrap();
    fs::write(s.0.join("tree/zero"), []).unwrap();
    fs::write(s.0.join("loose"), b"loose content").unwrap();
    bytes
}
#[test]
fn mixed_folders_and_files_round_trip_plain_and_aes_with_sha256() {
    for method in [Method::Store, Method::Deflate] {
        for encryption in [Encryption::None, Encryption::Aes256] {
            let s = Scratch::new();
            let original = seed(&s);
            let events = interactive(request(&s, method, encryption), |id, _, _| {
                vec![Message::Password {
                    challenge_id: id,
                    secret: Secret(b"folder-password".to_vec()),
                }]
            });
            assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
            let listed = execute(Start {
                action: Action::List {
                    archive: s.path("created.zip"),
                },
                limits: Limits::default(),
            });
            assert_eq!(terminal(&listed).outcome, Outcome::Completed, "{listed:?}");
            let entries = listed
                .iter()
                .filter_map(|e| {
                    if let Message::Entry(e) = e {
                        Some(e)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(entries.len(), 6);
            assert_eq!(
                entries
                    .iter()
                    .filter(|e| e.kind == EntryKind::Directory)
                    .count(),
                3
            );
            for entry in entries {
                assert_eq!(
                    entry.encrypted,
                    encryption == Encryption::Aes256 && entry.kind == EntryKind::File
                );
            }
            if encryption == Encryption::None {
                assert!(Command::new("python3").arg("-c").arg("import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; assert z.getinfo('documents/empty/').is_dir(); assert z.read('loose.bin') == b'loose content'").arg(s.0.join("created.zip")).status().unwrap().success());
            }
            fs::create_dir(s.0.join("output")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(s.0.join("output"), fs::Permissions::from_mode(0o700)).unwrap();
            }
            let extracted = interactive(
                Start {
                    action: Action::Extract {
                        archive: s.path("created.zip"),
                        destination: s.path("output"),
                        selection: Selection::All,
                        list_id: None,
                        gzip_output_name: None,
                    },
                    limits: Limits::default(),
                },
                |id, _, _| {
                    vec![Message::Password {
                        challenge_id: id,
                        secret: Secret(b"folder-password".to_vec()),
                    }]
                },
            );
            assert_eq!(
                terminal(&extracted).outcome,
                Outcome::Completed,
                "{extracted:?}"
            );
            assert_eq!(
                Sha256::digest(fs::read(s.0.join("output/documents/nested/한글.bin")).unwrap()),
                Sha256::digest(original)
            );
            assert_eq!(
                Sha256::digest(fs::read(s.0.join("output/loose.bin")).unwrap()),
                Sha256::digest(b"loose content")
            );
            assert_eq!(
                fs::read(s.0.join("output/documents/zero")).unwrap(),
                Vec::<u8>::new()
            );
            assert!(s.0.join("output/documents/empty").is_dir());
        }
    }
}
#[test]
fn descendant_name_conflict_is_rejected_before_password() {
    let s = Scratch::new();
    seed(&s);
    let mut start = request(&s, Method::Store, Encryption::Aes256);
    if let Action::CreateZip { sources, .. } = &mut start.action {
        sources[1].archive_name = "documents/zero".into();
    }
    let events = execute(start);
    assert_eq!(terminal(&events).error, ErrorCode::Conflict, "{events:?}");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Message::PasswordRequired { .. }))
    );
    assert!(!s.0.join("created.zip").exists());
}
#[test]
fn empty_root_folder_is_preserved() {
    let s = Scratch::new();
    fs::create_dir(s.0.join("tree")).unwrap();
    let mut start = request(&s, Method::Deflate, Encryption::None);
    if let Action::CreateZip { sources, .. } = &mut start.action {
        sources.truncate(1);
    }
    let events = execute(start);
    assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
    let bytes = fs::read(s.0.join("created.zip")).unwrap();
    let catalog = archive::parse(&bytes, Limits::default(), true, &mut || Ok(())).unwrap();
    assert_eq!(catalog.members.len(), 1);
    assert_eq!(catalog.members[0].entry.kind, EntryKind::Directory);
    assert_eq!(catalog.members[0].entry.name, "documents");
}
#[cfg(target_os = "linux")]
#[test]
fn symlinks_and_special_files_are_not_followed() {
    let s = Scratch::new();
    seed(&s);
    for target in ["loose", "tree", "missing"] {
        std::os::unix::fs::symlink(s.0.join(target), s.0.join("tree/link")).unwrap();
        let events = execute(request(&s, Method::Store, Encryption::Aes256));
        assert_eq!(terminal(&events).error, ErrorCode::UnsafePath, "{events:?}");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Message::PasswordRequired { .. }))
        );
        assert!(!s.0.join("created.zip").exists());
        fs::remove_file(s.0.join("tree/link")).unwrap();
    }
    assert!(
        Command::new("mkfifo")
            .arg(s.0.join("tree/pipe"))
            .status()
            .unwrap()
            .success()
    );
    let events = execute(request(&s, Method::Store, Encryption::None));
    assert_eq!(terminal(&events).error, ErrorCode::UnsafePath, "{events:?}");
}
#[test]
fn source_replacement_while_waiting_for_password_leaves_no_zip() {
    let s = Scratch::new();
    seed(&s);
    let events = interactive(
        request(&s, Method::Store, Encryption::Aes256),
        |id, _, _| {
            fs::rename(s.0.join("tree/nested/한글.bin"), s.0.join("previous")).unwrap();
            fs::write(s.0.join("tree/nested/한글.bin"), b"replacement").unwrap();
            vec![Message::Password {
                challenge_id: id,
                secret: Secret(b"folder-password".to_vec()),
            }]
        },
    );
    assert_eq!(
        terminal(&events).error,
        ErrorCode::SourceChanged,
        "{events:?}"
    );
    assert!(!s.0.join("created.zip").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn invalid_descendant_names_and_case_collisions_fail_before_password() {
    let s = Scratch::new();
    seed(&s);
    for (leaf, expected) in [
        ("CON.txt", ErrorCode::UnsafePath),
        ("ZERO", ErrorCode::Conflict),
    ] {
        fs::write(s.0.join("tree").join(leaf), b"not exported").unwrap();
        let events = execute(request(&s, Method::Store, Encryption::Aes256));
        assert_eq!(terminal(&events).error, expected, "{events:?}");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Message::PasswordRequired { .. }))
        );
        assert!(!s.0.join("created.zip").exists());
        fs::remove_file(s.0.join("tree").join(leaf)).unwrap();
    }
}

#[cfg(target_os = "linux")]
fn readable_sources_under_parent(mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    struct RestoreParent(PathBuf);
    impl Drop for RestoreParent {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let s = Scratch::new();
    let parent = s.0.join("restricted");
    fs::create_dir_all(parent.join("tree/empty")).unwrap();
    fs::write(parent.join("loose"), b"readable file").unwrap();
    fs::write(parent.join("tree/nested"), b"nested content").unwrap();
    let _restore = RestoreParent(parent.clone());
    fs::set_permissions(&parent, fs::Permissions::from_mode(mode)).unwrap();
    if mode == 0o111 {
        assert_eq!(
            fs::read_dir(&parent).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    } else {
        assert!(fs::read_dir(&parent).is_ok());
    }
    assert_eq!(fs::read(parent.join("loose")).unwrap(), b"readable file");
    for (name, encryption) in [("plain", Encryption::None), ("aes", Encryption::Aes256)] {
        let archive_name = format!("{name}.zip");
        let events = interactive(
            Start {
                action: Action::CreateZip {
                    sources: vec![
                        Source {
                            path: NativePath::from_path(&parent.join("loose")),
                            archive_name: "loose.txt".into(),
                        },
                        Source {
                            path: NativePath::from_path(&parent.join("tree")),
                            archive_name: "tree".into(),
                        },
                    ],
                    output_file: s.path(&archive_name),
                    method: Method::Deflate,
                    encryption,
                },
                limits: Limits::default(),
            },
            |id, _, _| {
                vec![Message::Password {
                    challenge_id: id,
                    secret: Secret(b"parent-password".to_vec()),
                }]
            },
        );
        assert_eq!(terminal(&events).outcome, Outcome::Completed, "{events:?}");
        let output = format!("output-{name}");
        fs::create_dir(s.0.join(&output)).unwrap();
        fs::set_permissions(s.0.join(&output), fs::Permissions::from_mode(0o700)).unwrap();
        let extracted = interactive(
            Start {
                action: Action::Extract {
                    archive: s.path(&archive_name),
                    destination: s.path(&output),
                    selection: Selection::All,
                    list_id: None,
                    gzip_output_name: None,
                },
                limits: Limits::default(),
            },
            |id, _, _| {
                vec![Message::Password {
                    challenge_id: id,
                    secret: Secret(b"parent-password".to_vec()),
                }]
            },
        );
        assert_eq!(
            terminal(&extracted).outcome,
            Outcome::Completed,
            "{extracted:?}"
        );
        for (source, destination) in [("loose", "loose.txt"), ("tree/nested", "tree/nested")] {
            assert_eq!(
                Sha256::digest(fs::read(parent.join(source)).unwrap()),
                Sha256::digest(fs::read(s.0.join(&output).join(destination)).unwrap())
            );
        }
        assert!(s.0.join(&output).join("tree/empty").is_dir());
    }
}
#[cfg(target_os = "linux")]
#[test]
fn readable_sources_under_search_only_parent_0111() {
    readable_sources_under_parent(0o111);
}
#[cfg(target_os = "linux")]
#[test]
fn readable_sources_under_readable_parent_0711() {
    readable_sources_under_parent(0o711);
}
