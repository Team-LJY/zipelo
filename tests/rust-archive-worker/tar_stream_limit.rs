use super::*;

fn fixture(kind: &str, joined: bool) -> (Scratch, u64) {
    let s = setup();
    generate(
        &s,
        &format!(
            "tar-stream-limit-{kind}-{}",
            if joined { "multi" } else { "single" }
        ),
    );
    let stream: u64 = fs::read_to_string(s.0.join("stream-size"))
        .unwrap()
        .parse()
        .unwrap();
    let payload = fs::metadata(s.0.join("expected")).unwrap().len();
    assert!(stream > 65_536 && payload < stream - 2);
    (s, stream)
}

fn start(s: &Scratch, extract: bool, soft: u64, ceiling: u64) -> GuardedStart {
    let mut start = request(s, soft, ceiling);
    start.ceiling.entries = start.start.limits.entries;
    if !extract {
        start.start.action = Action::List {
            archive: s.path("input"),
        };
    }
    start
}

fn result(s: &Scratch, events: &[Message], extract: bool, fits: bool) {
    let result = terminal(events);
    assert_eq!(result.cleanup, Cleanup::Complete, "{events:?}");
    assert_eq!(result.retained_partial_count, 0);
    if !fits {
        assert_eq!(result.outcome, Outcome::Failed, "{events:?}");
        assert_eq!(result.error, ErrorCode::Limit);
        assert_eq!(result.created_count, 0);
        assert_eq!(result.publication, Publication::None);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Message::Output { .. }))
        );
        assert!(fs::read_dir(s.0.join("output")).unwrap().next().is_none());
        return;
    }
    assert_eq!(result.outcome, Outcome::Completed, "{events:?}");
    assert_eq!(result.error, ErrorCode::None);
    if extract {
        assert_eq!(result.created_count, 1);
        assert_eq!(
            fs::read(s.0.join("output/payload")).unwrap(),
            fs::read(s.0.join("expected")).unwrap()
        );
    } else {
        assert_eq!(result.created_count, 0);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Message::ListEnd { entry_count: 1, .. }))
        );
        assert!(fs::read_dir(s.0.join("output")).unwrap().next().is_none());
    }
}

#[test]
fn normal_tar_stream_limit_keeps_exact_boundary_for_both_request_types() {
    for kind in ["gz", "bz2", "xz"] {
        for joined in [false, true] {
            for extract in [false, true] {
                for delta in [-1i64, 0, 1] {
                    for guarded in [false, true] {
                        let (s, stream) = fixture(kind, joined);
                        let limit = stream.checked_add_signed(delta).unwrap();
                        let start = start(&s, extract, limit, limit);
                        let events = if guarded {
                            run(&s, start, |event| {
                                assert!(!matches!(event, Message::ResourceRequired { .. }));
                                None
                            })
                        } else {
                            execute(start.start)
                        };
                        result(&s, &events, extract, delta >= 0);
                    }
                }
            }
        }
    }
}

#[test]
fn approved_tar_stream_limit_uses_ceiling_including_exact_boundary() {
    for kind in ["gz", "bz2", "xz"] {
        for joined in [false, true] {
            for extract in [false, true] {
                for delta in [-1i64, 0, 1] {
                    let (s, stream) = fixture(kind, joined);
                    let mut questions = 0;
                    let events = run(
                        &s,
                        start(
                            &s,
                            extract,
                            stream - 2,
                            stream.checked_add_signed(delta).unwrap(),
                        ),
                        |event| {
                            if let Message::ResourceRequired { scope, .. } = event {
                                questions += 1;
                                assert!(scope.output && !scope.entries && !scope.ratio);
                                assert!(fs::read_dir(s.0.join("output")).unwrap().next().is_none());
                            }
                            approve(event)
                        },
                    );
                    assert_eq!(questions, 1);
                    result(&s, &events, extract, delta >= 0);
                }
            }
        }
    }
}

#[test]
fn tar_stream_expansion_requires_permission_before_any_publication() {
    for kind in ["gz", "bz2", "xz"] {
        for extract in [false, true] {
            for cancel in [false, true] {
                let (s, stream) = fixture(kind, true);
                let mut questions = 0;
                let events = run(&s, start(&s, extract, stream - 1, stream), |event| {
                    if let Message::ResourceRequired {
                        challenge_id,
                        scope,
                        ..
                    } = event
                    {
                        questions += 1;
                        assert!(scope.output && !scope.entries && !scope.ratio);
                        assert!(fs::read_dir(s.0.join("output")).unwrap().next().is_none());
                        return Some(vec![if cancel {
                            Message::Cancel
                        } else {
                            Message::ResourceReply {
                                challenge_id: *challenge_id,
                                scope: *scope,
                                allow: false,
                            }
                        }]);
                    }
                    None
                });
                assert_eq!(questions, 1);
                let result = terminal(&events);
                assert_eq!(result.outcome, Outcome::Cancelled, "{events:?}");
                assert_eq!(result.publication, Publication::None);
                assert_eq!(result.created_count, 0);
                assert_eq!(result.cleanup, Cleanup::Complete);
                assert!(
                    !events
                        .iter()
                        .any(|event| matches!(event, Message::Output { .. }))
                );
                assert!(fs::read_dir(s.0.join("output")).unwrap().next().is_none());
            }
        }
    }
}
