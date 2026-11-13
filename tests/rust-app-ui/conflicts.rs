use super::*;
use std::collections::BTreeSet;
use zipelo_ui_memory_app::archive_present as present;

fn waiting() -> (Model, FakeFactory) {
    let mut factory = FakeFactory::default();
    let mut model = Model::default();
    list(&mut model, &mut factory);
    factory.state.borrow_mut().manual = true;
    assert!(model.extract(path("output"), false, None, &mut factory));
    factory
        .state
        .borrow_mut()
        .frame(Event::Accepted { encryption: false });
    ask(&mut model, &factory, 29);
    (model, factory)
}

fn ask(model: &mut Model, factory: &FakeFactory, challenge_id: u64) {
    factory
        .state
        .borrow_mut()
        .frame(Event::ConflictRequired(Conflict {
            challenge_id,
            entry_id: 0,
            relative_name: "읽어주세요.txt".into(),
            incoming_size: 31,
            existing_size: 128,
        }));
    for _ in 0..8 {
        model.poll();
    }
}

#[test]
fn each_choice_sends_one_scoped_reply_and_does_not_claim_a_saved_file() {
    for choice in [
        ConflictChoice::Overwrite,
        ConflictChoice::Skip,
        ConflictChoice::Rename,
    ] {
        for apply in [false, true] {
            let (mut model, factory) = waiting();
            assert_eq!(model.conflict.as_ref().unwrap().existing_size, 128);
            assert!(model.resolve_conflict(choice, apply));
            assert!(model.conflict.is_none());
            assert!(model.outputs.is_empty());
            assert!(!model.resolve_conflict(choice, apply));
            assert_eq!(
                factory.state.borrow().conflict_replies,
                vec![(29, choice, apply)]
            );
            assert_eq!(
                factory.state.borrow().requests.last(),
                Some(&(1, "conflict"))
            );
        }
    }
}

#[test]
fn cancellation_clears_question_and_rejects_a_late_click() {
    let (mut model, factory) = waiting();
    model.cancel();
    assert!(model.conflict.is_none());
    assert!(!model.resolve_conflict(ConflictChoice::Overwrite, true));
    ask(&mut model, &factory, 30);
    assert!(model.conflict.is_none());
    assert!(factory.state.borrow().conflict_replies.is_empty());
}

#[test]
fn duplicate_challenge_and_duplicate_resolution_are_rejected() {
    let (mut model, factory) = waiting();
    assert!(model.resolve_conflict(ConflictChoice::Skip, false));
    ask(&mut model, &factory, 29);
    assert!(model.error.is_some());
    let (mut model, factory) = waiting();
    let resolution = Resolution {
        entry_id: 0,
        relative_name: "읽어주세요.txt".into(),
        choice: ConflictChoice::Skip,
    };
    for _ in 0..2 {
        factory
            .state
            .borrow_mut()
            .frame(Event::ConflictResolved(resolution.clone()));
    }
    for _ in 0..8 {
        model.poll();
    }
    assert!(model.error.is_some());
}

fn completed(created_count: u64) -> Terminal {
    Terminal {
        outcome: Outcome::Completed,
        error: None,
        publication: if created_count == 0 {
            Publication::None
        } else {
            Publication::Complete
        },
        created_count,
        retained_partial_count: 0,
        durability: if created_count == 0 {
            Durability::NotApplicable
        } else {
            Durability::Synced
        },
        cleanup: Cleanup::Complete,
    }
}

#[test]
fn auto_applied_results_adjust_targets_but_created_reports_confirm_saves() {
    for choice in [
        ConflictChoice::Overwrite,
        ConflictChoice::Skip,
        ConflictChoice::Rename,
    ] {
        let (mut model, factory) = waiting();
        assert!(model.resolve_conflict(choice, true));
        let name = if choice == ConflictChoice::Rename {
            "읽어주세요 (1).txt"
        } else {
            "읽어주세요.txt"
        };
        factory
            .state
            .borrow_mut()
            .frame(Event::ConflictResolved(Resolution {
                entry_id: 0,
                relative_name: name.into(),
                choice,
            }));
        for _ in 0..8 {
            model.poll();
        }
        assert!(model.outputs.is_empty());
        let targets = ["읽어주세요.txt".to_owned()].into_iter().collect();
        let targets =
            present::resolved_targets(Some(&targets), &model.entries, None, &model.resolutions)
                .unwrap();
        assert_eq!(targets.len(), usize::from(choice != ConflictChoice::Skip));
        let count = u64::from(choice != ConflictChoice::Skip);
        let terminal = completed(count);
        if count != 0 {
            assert!(
                present::publication_warnings(
                    Some(&terminal),
                    true,
                    Some(&targets),
                    &BTreeSet::new(),
                    &[]
                )
                .contains(&present::UNCERTAIN)
            );
            factory.state.borrow_mut().frame(Event::Output {
                entry_id: Some(0),
                relative_name: name.into(),
                disposition: Disposition::Created,
            });
        }
        factory
            .state
            .borrow_mut()
            .frame(Event::Terminal(terminal.clone()));
        factory.state.borrow_mut().queue.extend([
            HostEvent::StdoutEof,
            HostEvent::StderrEof,
            HostEvent::Exited { success: true },
        ]);
        pump(&mut model);
        assert!(model.confirmed);
        assert!(model.conflict.is_none());
        assert!(
            present::publication_warnings(
                Some(&terminal),
                model.confirmed,
                Some(&targets),
                &BTreeSet::new(),
                &model.outputs
            )
            .is_empty()
        );
    }
}

#[test]
fn skipped_files_still_allow_created_directories_to_be_counted() {
    let terminal = completed(1);
    assert!(
        present::publication_warnings(
            Some(&terminal),
            true,
            Some(&BTreeSet::new()),
            &["사진".into()].into_iter().collect(),
            &[("사진".into(), Disposition::Created)],
        )
        .is_empty()
    );
}

#[cfg(all(feature = "ui-fixture", debug_assertions))]
#[test]
fn conflict_modal_buttons_and_escape_use_the_pending_question() {
    use eframe::egui::{self, epaint::Shape};
    use zipelo_ui_memory_app::archive_screen::{ArchiveApp, scenarios};
    for label in [
        "덮어쓰기",
        "건너뛰기",
        "이름 바꿔 저장",
        "작업 취소",
        "escape",
    ] {
        let ctx = egui::Context::default();
        let mut app = scenarios::parse("conflict-choice").unwrap().build();
        let frame = |app: &mut ArchiveApp, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 540.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.show(ui),
            );
            output.textures_delta.clear();
            output
        };
        fn labels(shape: &Shape, output: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                Shape::Text(t) => {
                    output.push((t.galley.text().into(), t.pos + t.galley.rect.size() / 2.0))
                }
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        labels(shape, output);
                    }
                }
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for _ in 0..3 {
            texts.clear();
            for shape in frame(&mut app, vec![]).shapes {
                labels(&shape.shape, &mut texts);
            }
        }
        if label == "escape" {
            frame(
                &mut app,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        } else {
            let point = texts.iter().find(|(text, _)| text == label).unwrap().1;
            for pressed in [true, false] {
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        }
        assert!(app.model.conflict.is_none(), "{label}");
        assert!(app.model.busy());
        assert!(app.model.outputs.is_empty());
        assert_eq!(
            app.model.cancellation_requested(),
            matches!(label, "작업 취소" | "escape")
        );
    }
}
