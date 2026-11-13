use super::*;

fn list(start: &mut Start, s: &Scratch) {
    start.action = Action::List {
        archive: s.path("input"),
    };
}

#[test]
fn compressed_header_empty_files_match_plain_and_legacy_at_one_byte_limit() {
    for encoded in [false, true] {
        for extract in [false, true] {
            for mode in 0..3 {
                let s = setup();
                generate(
                    &s,
                    if encoded {
                        "header-budget-encoded-0"
                    } else {
                        "header-budget-plain-0"
                    },
                );
                let mut start = request(&s, 1, if mode == 2 { 2 } else { 1 });
                start.ceiling.entries = start.start.limits.entries;
                if !extract {
                    list(&mut start.start, &s);
                } else {
                    let mut probe = start.start.clone();
                    list(&mut probe, &s);
                    let events = execute(probe);
                    let id = events
                        .into_iter()
                        .find_map(|m| match m {
                            Message::ListEnd { list_id, .. } => Some(list_id),
                            _ => None,
                        })
                        .unwrap();
                    if let Action::Extract {
                        selection, list_id, ..
                    } = &mut start.start.action
                    {
                        *selection = Selection::EntryIds((0..8).collect());
                        *list_id = Some(id);
                    }
                }
                let events = if mode == 0 {
                    execute(start.start)
                } else {
                    run(&s, start, approve)
                };
                let result = terminal(&events);
                assert_eq!(
                    result.outcome,
                    Outcome::Completed,
                    "encoded={encoded}, extract={extract}, mode={mode}: {events:?}"
                );
                assert_eq!(result.error, ErrorCode::None);
                assert_eq!(result.cleanup, Cleanup::Complete);
                assert_eq!(result.retained_partial_count, 0);
                assert_eq!(
                    events
                        .iter()
                        .filter(|m| matches!(m, Message::ResourceRequired { .. }))
                        .count(),
                    usize::from(mode == 2)
                );
                if extract {
                    assert_eq!(result.created_count, 8);
                    let files = fs::read_dir(s.0.join("output"))
                        .unwrap()
                        .collect::<std::io::Result<Vec<_>>>()
                        .unwrap();
                    assert_eq!(files.len(), 8);
                    assert!(files.iter().all(|file| file.metadata().unwrap().is_file()
                        && file.metadata().unwrap().len() == 0));
                } else {
                    assert_eq!(result.created_count, 0);
                    assert_eq!(
                        events
                            .iter()
                            .filter(|m| matches!(m, Message::Entry(_)))
                            .count(),
                        1000
                    );
                    assert!(events.iter().any(|m| matches!(
                        m,
                        Message::ListEnd {
                            entry_count: 1000,
                            ..
                        }
                    )));
                }
            }
        }
    }
}

#[test]
fn compressed_header_keeps_actual_file_output_boundary() {
    for encoded in [false, true] {
        for size in [1, 2] {
            let s = setup();
            generate(
                &s,
                &format!(
                    "header-budget-{}-{size}",
                    if encoded { "encoded" } else { "plain" }
                ),
            );
            let mut start = request(&s, 1, 1);
            start.ceiling.entries = start.start.limits.entries;
            let probe = execute(Start {
                action: Action::List {
                    archive: s.path("input"),
                },
                limits: Limits::default(),
            });
            let id = probe
                .into_iter()
                .find_map(|m| match m {
                    Message::ListEnd { list_id, .. } => Some(list_id),
                    _ => None,
                })
                .unwrap();
            if let Action::Extract {
                selection, list_id, ..
            } = &mut start.start.action
            {
                *selection = Selection::EntryIds(vec![1000]);
                *list_id = Some(id);
            }
            let events = run(&s, start, approve);
            let result = terminal(&events);
            assert_eq!(result.cleanup, Cleanup::Complete, "{events:?}");
            assert_eq!(result.retained_partial_count, 0);
            assert!(
                !events
                    .iter()
                    .any(|m| matches!(m, Message::ResourceRequired { .. }))
            );
            if size == 1 {
                assert_eq!(result.outcome, Outcome::Completed, "{events:?}");
                assert_eq!(result.created_count, 1);
                assert_eq!(fs::read(s.0.join("output/payload")).unwrap(), b"x");
            } else {
                assert_eq!(result.outcome, Outcome::Failed, "{events:?}");
                assert_eq!(result.error, ErrorCode::Limit);
                assert_eq!(result.created_count, 0);
                assert!(fs::read_dir(s.0.join("output")).unwrap().next().is_none());
            }
        }
    }
}

#[test]
fn zip_and_tar_header_scans_keep_the_logical_output_budget() {
    for format in ["zip", "tar"] {
        let s = setup();
        generate(&s, &format!("header-budget-{format}"));
        let mut start = request(&s, 1, 1);
        start.ceiling.entries = start.start.limits.entries;
        list(&mut start.start, &s);
        for events in [execute(start.start.clone()), run(&s, start, approve)] {
            assert_eq!(
                terminal(&events).outcome,
                Outcome::Completed,
                "{format}: {events:?}"
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|m| matches!(m, Message::Entry(_)))
                    .count(),
                1000
            );
            assert!(
                !events
                    .iter()
                    .any(|m| matches!(m, Message::ResourceRequired { .. }))
            );
        }
    }
}
