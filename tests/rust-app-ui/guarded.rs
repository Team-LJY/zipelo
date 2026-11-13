use super::*;

fn waiting() -> (Model, FakeFactory) {
    let mut f = FakeFactory::manual();
    let mut m = Model::default();
    assert!(m.begin(
        Start::List {
            archive: path("large.zip")
        },
        &mut f
    ));
    f.state
        .borrow_mut()
        .frame(Event::Accepted { encryption: true });
    for _ in 0..4 {
        m.poll();
    }
    (m, f)
}
fn question(id: u64) -> ResourceChallenge {
    ResourceChallenge {
        id,
        action: ActionKind::List,
        scope: ResourceScope {
            ratio: true,
            ..Default::default()
        },
        usage: ResourceUsage::default(),
        soft_limits: Limits::default(),
        ceiling: Limits::default(),
    }
}
fn receive(m: &mut Model, f: &FakeFactory, event: Event) {
    f.state.borrow_mut().frame(event);
    for _ in 0..4 {
        m.poll();
    }
}
fn fail(
    m: &mut Model,
    f: &FakeFactory,
    publication: Publication,
    created: u64,
    partial: u64,
    cleanup: Cleanup,
) {
    let mut s = f.state.borrow_mut();
    s.frame(Event::Terminal(Terminal {
        outcome: Outcome::Failed,
        error: Some(ErrorCode::Limit),
        publication,
        created_count: created,
        retained_partial_count: partial,
        durability: Durability::NotApplicable,
        cleanup,
    }));
    s.queue.extend([
        HostEvent::StdoutEof,
        HostEvent::StderrEof,
        HostEvent::Exited { success: true },
    ]);
    drop(s);
    pump(m);
}
#[test]
fn reply_is_bound_to_job_question_scope_and_once_only() {
    let (mut m, f) = waiting();
    receive(&mut m, &f, Event::ResourceRequired(question(1)));
    assert!(!m.resource_reply(99, 1, true));
    assert!(!m.resource_reply(m.job_id().unwrap(), 2, true));
    assert!(m.resource_reply(m.job_id().unwrap(), 1, true));
    assert!(!m.resource_reply(m.job_id().unwrap(), 1, true));
    assert_eq!(f.state.borrow().resources, [(1, question(1).scope, true)]);
}
#[test]
fn cancel_during_question_never_grants_and_clears_prompt() {
    let (mut m, f) = waiting();
    receive(&mut m, &f, Event::ResourceRequired(question(1)));
    let job = m.job_id().unwrap();
    m.cancel();
    assert!(m.resource.is_none());
    assert!(!m.resource_reply(job, 1, true));
    f.state
        .borrow_mut()
        .finish(Outcome::Cancelled, Publication::None);
    pump(&mut m);
    assert!(f.state.borrow().resources.is_empty());
    assert!(m.confirmed);
}
#[test]
fn queued_question_after_cancel_does_not_turn_cancel_into_transport_failure() {
    let (mut m, f) = waiting();
    m.cancel();
    receive(&mut m, &f, Event::ResourceRequired(question(1)));
    assert!(m.resource.is_none());
    assert!(m.error.is_none());
}
#[test]
fn wrong_action_limits_empty_scope_and_overlapping_questions_are_rejected() {
    for mode in 0..6 {
        let (mut m, f) = waiting();
        let mut q = question(1);
        match mode {
            0 => q.action = ActionKind::Extract,
            1 => q.ceiling.output += 1,
            2 => q.scope = ResourceScope::default(),
            3 => q.id = 0,
            4 => receive(
                &mut m,
                &f,
                Event::PasswordRequired {
                    challenge_id: 1,
                    attempt: 1,
                    encrypt: false,
                },
            ),
            _ => receive(&mut m, &f, Event::ResourceRequired(q.clone())),
        }
        receive(&mut m, &f, Event::ResourceRequired(q));
        assert!(m.error.is_some(), "{mode}");
    }
}
#[test]
fn old_challenge_and_repeated_scope_are_rejected_after_reply() {
    for reused_id in [true, false] {
        let (mut m, f) = waiting();
        receive(&mut m, &f, Event::ResourceRequired(question(1)));
        assert!(m.resource_reply(m.job_id().unwrap(), 1, true));
        receive(
            &mut m,
            &f,
            Event::ResourceRequired(question(if reused_id { 1 } else { 2 })),
        );
        assert!(m.error.is_some());
    }
}
#[test]
fn only_confirmed_clean_empty_failure_offers_expanded_retry_once() {
    for (publication, count, partial, cleanup, retry) in [
        (Publication::None, 0, 0, Cleanup::Complete, true),
        (Publication::None, 1, 0, Cleanup::Complete, false),
        (Publication::Partial, 1, 0, Cleanup::Complete, false),
        (Publication::Uncertain, 0, 0, Cleanup::Complete, false),
        (Publication::None, 0, 1, Cleanup::Complete, false),
        (Publication::None, 0, 0, Cleanup::Unconfirmed, false),
    ] {
        let (mut m, mut f) = waiting();
        fail(&mut m, &f, publication, count, partial, cleanup);
        assert_eq!(m.can_retry_expanded(), retry);
        assert_eq!(m.retry_expanded(&mut f), retry);
        if retry {
            assert!(f.state.borrow().expanded);
            receive(&mut m, &f, Event::Accepted { encryption: true });
            fail(&mut m, &f, Publication::None, 0, 0, Cleanup::Complete);
            assert!(!m.can_retry_expanded());
        }
    }
}
#[test]
fn known_extract_size_requests_expansion_before_processing_and_estimates_unknown_separately() {
    let mut m = Model::default();
    let mut f = FakeFactory::default();
    list(&mut m, &mut f);
    m.entries[0].bytes = Some(Limits::default().output + 1);
    f.state.borrow_mut().manual = true;
    assert!(m.extract(path("out"), false, None, &mut f));
    assert!(f.state.borrow().expanded);
    assert_eq!(
        m.estimated_usage().estimated_bytes,
        Some(Limits::default().output + 1)
    );
    assert_eq!(m.estimated_usage().actual_bytes, None);
}
#[test]
fn entries_over_one_hundred_thousand_are_accepted_only_after_entry_approval() {
    for approved in [false, true] {
        let (mut m, f) = waiting();
        if approved {
            fail(&mut m, &f, Publication::None, 0, 0, Cleanup::Complete);
            let mut factory = f.clone();
            assert!(m.retry_expanded(&mut factory));
            receive(&mut m, &f, Event::Accepted { encryption: true });
            let mut q = question(1);
            q.scope = ResourceScope {
                output: true,
                entries: true,
                ratio: false,
            };
            q.ceiling = Limits {
                input: u64::MAX,
                output: 64 * 1024 * 1024 * 1024,
                entries: 200_000,
            };
            receive(&mut m, &f, Event::ResourceRequired(q));
            assert!(m.resource_reply(m.job_id().unwrap(), 1, true));
        }
        let cap = if approved { 200_000 } else { 100_000 };
        for id in 0..=cap {
            assert!(m.error.is_none(), "{id}");
            receive(
                &mut m,
                &f,
                Event::Entry(Entry {
                    id,
                    name: format!("{id}"),
                    kind: Kind::File,
                    bytes: Some(0),
                    packed: Some(0),
                    encrypted: false,
                }),
            );
        }
        assert!(m.error.is_some());
    }
}

#[test]
fn output_history_ceiling_expands_only_after_entry_permission() {
    for approved in [false, true] {
        let mut m = Model::default();
        let mut f = FakeFactory::manual();
        assert!(m.begin(
            Start::CreateZip {
                sources: vec![(path("source"), "source".into())],
                output_file: path("new.zip"),
                method: Method::Store,
                aes256: false
            },
            &mut f
        ));
        receive(&mut m, &f, Event::Accepted { encryption: true });
        if approved {
            fail(&mut m, &f, Publication::None, 0, 0, Cleanup::Complete);
            assert!(m.retry_expanded(&mut f));
            receive(&mut m, &f, Event::Accepted { encryption: true });
            let mut q = question(1);
            q.action = ActionKind::CreateZip;
            q.scope = ResourceScope {
                output: true,
                entries: true,
                ratio: false,
            };
            q.ceiling = Limits {
                input: u64::MAX,
                output: 64 * 1024 * 1024 * 1024,
                entries: 200_000,
            };
            receive(&mut m, &f, Event::ResourceRequired(q));
            assert!(m.resource_reply(m.job_id().unwrap(), 1, true));
        }
        let cap = if approved { 200_000 } else { 100_000 };
        for n in 0..=cap {
            assert!(m.error.is_none(), "{n}");
            receive(
                &mut m,
                &f,
                Event::Output {
                    entry_id: None,
                    relative_name: format!("{n}"),
                    disposition: Disposition::Created,
                },
            );
        }
        assert_eq!(m.outputs.len(), cap);
        assert!(m.error.is_some());
    }
}
