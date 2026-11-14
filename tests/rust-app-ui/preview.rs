use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn setup() -> (Model, FakeFactory) {
    let mut model = Model::default();
    let mut factory = FakeFactory::default();
    list(&mut model, &mut factory);
    model.selected.insert(0);
    factory.state.borrow_mut().manual = true;
    assert!(model.begin_preview(&mut factory));
    factory
        .state
        .borrow_mut()
        .frame(Event::Accepted { encryption: false });
    (model, factory)
}

fn info(length: u64) -> PreviewInfo {
    PreviewInfo {
        entry_id: 0,
        format: PreviewFormat::Utf8Text,
        total_bytes: length,
        width: 0,
        height: 0,
    }
}

fn transfer(factory: &FakeFactory, bytes: &[u8]) {
    let mut state = factory.state.borrow_mut();
    state.frame(Event::PreviewBegin(info(bytes.len() as u64)));
    for (offset, chunk) in bytes.chunks(32 * 1024).enumerate() {
        state.frame(Event::PreviewChunk {
            entry_id: 0,
            offset: (offset * 32 * 1024) as u64,
            data: chunk.to_vec(),
        });
    }
    state.frame(Event::PreviewEnd {
        entry_id: 0,
        total_bytes: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    });
}

fn finish(factory: &FakeFactory) {
    let mut state = factory.state.borrow_mut();
    state.frame(Event::Terminal(Terminal {
        outcome: Outcome::Completed,
        error: None,
        publication: Publication::None,
        created_count: 0,
        retained_partial_count: 0,
        durability: Durability::NotApplicable,
        cleanup: Cleanup::Complete,
    }));
    state.queue.push_back(HostEvent::StdoutEof);
    state.queue.push_back(HostEvent::StderrEof);
    state.queue.push_back(HostEvent::Exited { success: true });
}

#[test]
fn preview_waits_for_verified_data_terminal_eofs_and_reap() {
    for bytes in [
        Vec::new(),
        "가".repeat(20_000).into_bytes(),
        vec![b'a'; 1024 * 1024],
    ] {
        let (mut model, factory) = setup();
        transfer(&factory, &bytes);
        for _ in 0..100 {
            model.poll();
            assert!(model.preview.is_none());
        }
        finish(&factory);
        while model.busy() {
            assert!(model.preview.is_none());
            model.poll();
        }
        assert!(model.confirmed);
        assert_eq!(model.preview.as_ref().unwrap().data, bytes);
        assert_eq!(model.selected, BTreeSet::from([0]));
        assert!(matches!(
            factory.state.borrow().starts.last(),
            Some(Start::Preview { entry_id: 0, .. })
        ));
    }
}

#[test]
fn preview_rejects_bad_dimensions_and_text_bounds_before_allocation() {
    for metadata in [
        info(1024 * 1024 + 1),
        PreviewInfo {
            width: 1,
            ..info(1)
        },
        PreviewInfo {
            format: PreviewFormat::Rgba8,
            width: 8193,
            height: 1,
            total_bytes: 8193 * 4,
            ..info(0)
        },
        PreviewInfo {
            format: PreviewFormat::Rgba8,
            width: 4001,
            height: 4000,
            total_bytes: 4001 * 4000 * 4,
            ..info(0)
        },
        PreviewInfo {
            format: PreviewFormat::Rgba8,
            width: 1,
            height: 1,
            total_bytes: 3,
            ..info(0)
        },
        PreviewInfo {
            entry_id: 8,
            ..info(1)
        },
    ] {
        let (mut model, factory) = setup();
        factory
            .state
            .borrow_mut()
            .frame(Event::PreviewBegin(metadata));
        finish(&factory);
        pump(&mut model);
        assert!(model.preview.is_none());
        assert!(model.error.is_some());
    }
}

#[test]
fn preview_rejects_offsets_duplicates_oversized_chunks_and_hash() {
    for case in 0..8 {
        let (mut model, factory) = setup();
        {
            let mut state = factory.state.borrow_mut();
            if case != 0 {
                state.frame(Event::PreviewBegin(info(4)));
            }
            state.frame(Event::PreviewChunk {
                entry_id: if case == 1 { 9 } else { 0 },
                offset: if case == 2 { u64::MAX } else { 0 },
                data: if case == 3 {
                    vec![0; 32769]
                } else if case == 4 {
                    vec![]
                } else if case == 7 {
                    vec![255; 4]
                } else {
                    b"abcd".to_vec()
                },
            });
            if case == 5 {
                state.frame(Event::PreviewBegin(info(4)));
            }
            state.frame(Event::PreviewEnd {
                entry_id: 0,
                total_bytes: 4,
                sha256: if case == 6 {
                    [0; 32]
                } else if case == 7 {
                    Sha256::digest([255; 4]).into()
                } else {
                    Sha256::digest(b"abcd").into()
                },
            });
        }
        finish(&factory);
        pump(&mut model);
        assert!(model.preview.is_none(), "case {case}");
        assert!(model.error.is_some(), "case {case}");
    }
}

#[test]
fn preview_cancel_discards_late_success_even_when_too_late() {
    for selection_change in [false, true] {
        let (mut model, factory) = setup();
        factory
            .state
            .borrow_mut()
            .frame(Event::PreviewBegin(info(4)));
        model.poll();
        model.poll();
        if selection_change {
            model.selected.clear();
            model.poll();
        } else {
            model.dismiss_preview();
        }
        assert!(model.busy());
        {
            let mut state = factory.state.borrow_mut();
            state.frame(Event::CancelAck {
                request_sequence: 1,
                status: CancelStatus::TooLate,
            });
        }
        transfer(&factory, b"late");
        finish(&factory);
        pump(&mut model);
        assert!(model.preview.is_none());
        assert!(!model.can_retry_expanded());
        assert!(
            factory
                .state
                .borrow()
                .requests
                .iter()
                .any(|(_, c)| *c == "cancel")
        );
    }
}

#[test]
fn preview_requires_one_regular_file_and_clears_previous_result() {
    let mut factory = FakeFactory::default();
    let mut model = Model::default();
    list(&mut model, &mut factory);
    assert!(!model.can_preview());
    model.selected.extend([0, 1]);
    assert!(!model.can_preview());
    model.selected = BTreeSet::from([0]);
    for kind in [Kind::Directory, Kind::Link, Kind::Other] {
        model.entries[0].kind = kind;
        assert!(!model.can_preview());
    }
    model.entries[0].kind = Kind::File;
    assert!(model.can_preview());
    let (mut model, factory) = setup();
    transfer(&factory, b"abc");
    finish(&factory);
    pump(&mut model);
    assert!(model.preview.is_some());
    model.dismiss_preview();
    assert!(model.preview.is_none());
}
