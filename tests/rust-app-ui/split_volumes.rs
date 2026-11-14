use super::*;

fn split_id() -> ListId {
    ListId {
        catalog_version: 2,
        format: Format::SevenZip,
        source_object: SourceObject::SplitSevenZip {
            manifest_sha256: [19; 32],
        },
        archive_bytes: 900,
        archive_sha256: [23; 32],
    }
}
fn setup() -> (Model, FakeFactory) {
    let mut model = Model::default();
    let mut factory = FakeFactory::manual();
    assert!(model.begin(
        Start::List {
            archive: path("자료.7z.001")
        },
        &mut factory
    ));
    factory
        .state
        .borrow_mut()
        .frame(Event::Accepted { encryption: false });
    (model, factory)
}
fn problem(reason: VolumeProblemReason) -> VolumeProblem {
    VolumeProblem {
        reason,
        volume: NativePath::Unix("자료.7z.003".as_bytes().to_vec()),
    }
}
fn terminal(factory: &FakeFactory, outcome: Outcome, error: Option<ErrorCode>) {
    factory.state.borrow_mut().frame(Event::Terminal(Terminal {
        outcome,
        error,
        publication: Publication::None,
        created_count: 0,
        retained_partial_count: 0,
        durability: Durability::NotApplicable,
        cleanup: Cleanup::Complete,
    }));
}
fn exited(factory: &FakeFactory) {
    let mut state = factory.state.borrow_mut();
    state.queue.push_back(HostEvent::StdoutEof);
    state.queue.push_back(HostEvent::StderrEof);
    state.queue.push_back(HostEvent::Exited { success: true });
}
#[test]
fn split_list_id_is_preserved_for_extract_and_preview_cannot_start() {
    let (mut model, mut factory) = setup();
    {
        let mut state = factory.state.borrow_mut();
        state.frame(Event::Entry(Entry {
            id: 0,
            name: "small.txt".into(),
            kind: Kind::File,
            bytes: Some(7),
            packed: Some(9),
            encrypted: false,
        }));
        state.frame(Event::ListEnd {
            entry_count: 1,
            list_id: split_id(),
        });
    }
    terminal(&factory, Outcome::Completed, None);
    exited(&factory);
    pump(&mut model);
    assert_eq!(model.list_id, Some(split_id()));
    assert!(model.can_extract());
    model.selected.insert(0);
    assert!(!model.can_preview());
    assert!(!model.begin_preview(&mut factory));
    assert!(!model.begin(
        Start::Preview {
            archive: model.archive.clone().unwrap(),
            entry_id: 0,
            list_id: split_id()
        },
        &mut factory
    ));
    assert!(model.extract(path("output"), true, None, &mut factory));
    assert!(
        matches!(factory.state.borrow().starts.last(), Some(Start::Extract { list_id, .. }) if list_id == &split_id())
    );
}
#[test]
fn split_list_id_rejects_invalid_version_format_and_source_combinations() {
    for id in [
        ListId {
            catalog_version: 1,
            ..split_id()
        },
        ListId {
            format: Format::Zip,
            ..split_id()
        },
        ListId {
            source_object: SourceObject::Unix {
                device: 1,
                inode: 2,
            },
            ..split_id()
        },
    ] {
        let (mut model, factory) = setup();
        factory.state.borrow_mut().frame(Event::ListEnd {
            entry_count: 0,
            list_id: id,
        });
        terminal(&factory, Outcome::Completed, None);
        exited(&factory);
        pump(&mut model);
        assert!(model.error.is_some());
        assert!(model.list_id.is_none());
    }
}
#[test]
fn volume_problem_waits_for_terminal_eofs_and_reap_and_clears_on_new_job() {
    let (mut model, mut factory) = setup();
    factory
        .state
        .borrow_mut()
        .frame(Event::VolumeProblem(problem(
            VolumeProblemReason::MissingVolume,
        )));
    terminal(&factory, Outcome::Failed, Some(ErrorCode::MissingVolume));
    for _ in 0..32 {
        model.poll();
        assert!(model.volume_problem.is_none());
    }
    assert!(model.busy());
    exited(&factory);
    pump(&mut model);
    assert!(model.confirmed);
    assert_eq!(
        model.volume_problem,
        Some(problem(VolumeProblemReason::MissingVolume))
    );
    assert!(model.begin(
        Start::List {
            archive: path("next.zip")
        },
        &mut factory
    ));
    assert!(model.volume_problem.is_none());
}
#[test]
fn volume_problem_reason_requires_matching_failed_terminal() {
    for reason in [
        VolumeProblemReason::FirstVolumeRequired,
        VolumeProblemReason::MissingVolume,
        VolumeProblemReason::IncompleteSet,
    ] {
        for valid in [true, false] {
            let (mut model, factory) = setup();
            factory
                .state
                .borrow_mut()
                .frame(Event::VolumeProblem(problem(reason)));
            let error = match reason {
                VolumeProblemReason::FirstVolumeRequired => ErrorCode::InvalidRequest,
                VolumeProblemReason::MissingVolume => ErrorCode::MissingVolume,
                VolumeProblemReason::IncompleteSet => ErrorCode::Integrity,
            };
            terminal(
                &factory,
                if valid {
                    Outcome::Failed
                } else {
                    Outcome::Completed
                },
                Some(error),
            );
            exited(&factory);
            pump(&mut model);
            assert_eq!(model.confirmed, valid);
            assert_eq!(model.volume_problem.is_some(), valid);
        }
    }
    let (mut model, factory) = setup();
    factory
        .state
        .borrow_mut()
        .frame(Event::VolumeProblem(problem(
            VolumeProblemReason::MissingVolume,
        )));
    terminal(&factory, Outcome::Failed, Some(ErrorCode::Integrity));
    exited(&factory);
    pump(&mut model);
    assert!(!model.confirmed);
    assert!(model.volume_problem.is_none());
}
#[test]
fn duplicate_or_unsafe_volume_names_are_rejected() {
    for duplicate in [false, true] {
        let (mut model, factory) = setup();
        let mut value = problem(VolumeProblemReason::MissingVolume);
        if !duplicate {
            value.volume = NativePath::Unix(b"../outside.7z.003".to_vec());
        }
        factory
            .state
            .borrow_mut()
            .frame(Event::VolumeProblem(value.clone()));
        if duplicate {
            factory
                .state
                .borrow_mut()
                .frame(Event::VolumeProblem(value));
        }
        terminal(&factory, Outcome::Failed, Some(ErrorCode::MissingVolume));
        exited(&factory);
        pump(&mut model);
        assert!(!model.confirmed);
        assert!(model.volume_problem.is_none());
    }
}
#[test]
fn cancellation_discards_late_volume_problem() {
    let (mut model, factory) = setup();
    for _ in 0..4 {
        model.poll();
    }
    model.cancel();
    factory
        .state
        .borrow_mut()
        .frame(Event::VolumeProblem(problem(
            VolumeProblemReason::MissingVolume,
        )));
    factory.state.borrow_mut().frame(Event::CancelAck {
        request_sequence: 1,
        status: CancelStatus::Requested,
    });
    terminal(&factory, Outcome::Cancelled, None);
    exited(&factory);
    pump(&mut model);
    assert!(model.confirmed, "{:?}", model.error);
    assert!(model.volume_problem.is_none());
}
#[test]
fn volume_filename_display_preserves_native_units_and_escapes_controls() {
    use zipelo_ui_memory_app::archive_present::volume_problem_text;
    let mut value = problem(VolumeProblemReason::MissingVolume);
    value.volume = NativePath::Unix(b"a\n\xff.7z.003".to_vec());
    let text = volume_problem_text(&value);
    assert!(text.contains("a\\u{a}\\xff.7z.003"), "{text}");
    assert!(!text.contains('\n'));
    value.volume = NativePath::Windows(vec![0xd800, 0x202e, 46, 55, 122, 46, 48, 48, 51]);
    let text = volume_problem_text(&value);
    assert!(text.contains("\\u{d800}\\u{202e}.7z.003"), "{text}");
}

#[test]
fn wrong_job_volume_problem_is_not_presented() {
    let (mut model, factory) = setup();
    factory
        .state
        .borrow_mut()
        .frame(Event::VolumeProblem(problem(
            VolumeProblemReason::MissingVolume,
        )));
    if let Some(HostEvent::Frame { job_id, .. }) = factory.state.borrow_mut().queue.back_mut() {
        *job_id += 1;
    }
    terminal(&factory, Outcome::Failed, Some(ErrorCode::MissingVolume));
    exited(&factory);
    pump(&mut model);
    assert!(!model.confirmed);
    assert!(model.volume_problem.is_none());
}
