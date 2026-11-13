use super::*;

fn phase_setup(phase: &str) -> (Scratch, GuardedStart) {
    let s = setup();
    let directories = matches!(phase, "preflight" | "directory" | "mkdir");
    generate(
        &s,
        if directories {
            "phase-directories"
        } else {
            "phase-files"
        },
    );
    if matches!(phase, "preflight" | "directory") {
        for id in 0..8 {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .mode(0o700)
                .create(s.0.join(format!("output/entry-{id}")))
                .unwrap();
        }
    }
    let mut start = request(&s, 64, 128);
    match phase {
        "source" | "header" => {
            start = create_progress_request(
                &s,
                if phase == "header" {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                },
                if phase == "header" { 0 } else { 1 },
                Method::Store,
            );
        }
        "list" => {
            start.start.action = Action::List {
                archive: s.path("input"),
            }
        }
        "rename" => {
            let members = [archive::Member {
                entry: Entry {
                    entry_id: 0,
                    name: "file.txt".into(),
                    kind: EntryKind::File,
                    size: 1,
                    compressed_size: 1,
                    encrypted: false,
                },
                data: b"x".to_vec(),
            }];
            fs::write(
                s.0.join("input"),
                archive::create_zip(&members, Method::Store, Limits::default(), &mut || Ok(()))
                    .unwrap(),
            )
            .unwrap();
            fs::write(s.0.join("output/file.txt"), b"old").unwrap();
            for id in 1..=8 {
                fs::write(s.0.join(format!("output/file ({id}).txt")), b"old").unwrap();
            }
            start.interactive = true;
        }
        "overwrite" => {
            for id in 0..8 {
                fs::write(s.0.join(format!("output/entry-{id}")), b"old").unwrap();
            }
            start.interactive = true;
        }
        _ => {}
    }
    (s, start)
}

fn phase_reply(event: &Message, phase: &str) -> Option<Vec<Message>> {
    if let Message::ConflictRequired { challenge_id, .. } = event {
        return Some(vec![Message::ConflictReply {
            challenge_id: *challenge_id,
            choice: if phase == "rename" {
                ConflictChoice::Rename
            } else {
                ConflictChoice::Overwrite
            },
            apply_to_remaining: true,
        }]);
    }
    approve(event)
}

#[test]
fn existing_directory_lookups_refresh_progress_after_permission() {
    for approved in [false, true] {
        let (s, mut start) = phase_setup("directory");
        if !approved {
            start.ceiling = start.start.limits;
        }
        let events = run_fault(&s, start, Some("phase-directory"), approve);
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{approved}: {events:?}"
        );
        assert_eq!(terminal(&events).created_count, 0);
        assert_eq!(
            fs::read_to_string(s.0.join("marker"))
                .unwrap()
                .lines()
                .count(),
            8
        );
    }
}

#[test]
fn every_processing_phase_records_completed_items_with_a_slow_clock() {
    for phase in [
        "preflight",
        "mkdir",
        "space",
        "create",
        "sync",
        "list",
        "catalog",
        "source",
        "header",
        "rename",
        "overwrite",
        "write",
        "publish",
        "cleanup",
        "all",
    ] {
        let (s, start) = phase_setup(phase);
        let events = run_fault(&s, start, Some(&format!("phase-{phase}")), |event| {
            phase_reply(event, phase)
        });
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Completed,
            "{phase}: {events:?}"
        );
        assert_eq!(terminal(&events).cleanup, Cleanup::Complete);
        let steps = fs::read_to_string(s.0.join("marker"))
            .unwrap_or_else(|error| panic!("{phase}: {error}: {events:?}"));
        assert!(steps.lines().count() >= 8, "{phase}: {steps}");
        if phase == "rename" {
            assert_eq!(fs::read(s.0.join("output/file (9).txt")).unwrap(), b"x");
            assert_eq!(fs::read(s.0.join("output/file.txt")).unwrap(), b"old");
        } else if phase == "overwrite" || phase == "all" {
            for id in 0..8 {
                assert_eq!(
                    fs::read(s.0.join(format!("output/entry-{id}"))).unwrap(),
                    b"x"
                );
            }
        }
    }
}

#[test]
fn a_single_stalled_phase_keeps_publication_accounting_and_cleanup() {
    for (phase, created) in [
        ("preflight", 0),
        ("directory", 0),
        ("mkdir", 1),
        ("write", 0),
        ("publish", 1),
        ("cleanup", 1),
    ] {
        let (s, start) = phase_setup(phase);
        let events = run_fault(&s, start, Some(&format!("phase-{phase}-stall")), approve);
        assert_eq!(
            terminal(&events).error,
            ErrorCode::Limit,
            "{phase}: {events:?}"
        );
        assert_eq!(
            terminal(&events).outcome,
            Outcome::Failed,
            "{phase}: {events:?}"
        );
        assert_eq!(
            terminal(&events).created_count,
            created,
            "{phase}: {events:?}"
        );
        assert_eq!(
            terminal(&events).cleanup,
            Cleanup::Complete,
            "{phase}: {events:?}"
        );
        assert_eq!(terminal(&events).retained_partial_count, 0);
        assert_eq!(
            terminal(&events).publication,
            if created == 0 {
                Publication::None
            } else {
                Publication::Partial
            }
        );
        assert!(fs::read_dir(s.0.join("output")).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".zipelo-")
        }));
    }
}
