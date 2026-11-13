#![cfg(debug_assertions)]

use eframe::egui;
use std::{collections::BTreeSet, ffi::OsString};
use zipelo_ui_memory_app::archive_screen::scenarios::{self, Launch, SCENARIOS};

#[test]
fn every_name_round_trips_and_listing_has_no_duplicates() {
    let names: BTreeSet<_> = SCENARIOS.iter().map(|s| s.name).collect();
    assert_eq!(names.len(), SCENARIOS.len());
    for scenario in SCENARIOS {
        assert_eq!(scenarios::parse(scenario.name), Ok(*scenario));
        assert!(scenarios::listing().contains(scenario.description));
        assert!(
            include_str!("../../docs/ui-scenario-guide.md")
                .contains(&format!("- {}:", scenario.name))
        );
        assert_eq!(
            scenarios::launch(vec!["--ui-scenario".into(), scenario.name.into()]),
            Ok(Launch::Scenario(*scenario)),
        );
    }
    assert_eq!(
        scenarios::launch(vec!["--list-ui-scenarios".into()]),
        Ok(Launch::List)
    );
}

#[test]
fn unknown_missing_and_conflicting_arguments_are_rejected_with_catalog() {
    for args in [
        vec!["--ui-scenario", "missing"],
        vec!["--ui-scenario"],
        vec!["--ui-scenario", "archive", "file.zip"],
        vec!["--list-ui-scenarios", "file.zip"],
        vec!["--ui-fixture", "--ui-scenario", "archive"],
    ] {
        let error = scenarios::launch(args.into_iter().map(OsString::from).collect()).unwrap_err();
        for scenario in SCENARIOS {
            assert!(error.contains(scenario.name));
        }
    }
    let original = vec![OsString::from("압축 자료.zip")];
    assert_eq!(
        scenarios::launch(original.clone()),
        Ok(Launch::Normal(original))
    );
}

fn text(shapes: &[egui::epaint::ClippedShape]) -> String {
    fn collect(shape: &egui::Shape, result: &mut String) {
        match shape {
            egui::Shape::Text(t) => {
                result.push_str(t.galley.text());
                result.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, result);
                }
            }
            _ => {}
        }
    }
    let mut result = String::new();
    for shape in shapes {
        collect(&shape.shape, &mut result);
    }
    result
}

#[test]
fn every_listed_scenario_draws_and_keeps_its_expected_screen() {
    for scenario in SCENARIOS {
        for size in [egui::vec2(1060.0, 720.0), egui::vec2(760.0, 540.0)] {
            let ctx = egui::Context::default();
            let mut app = scenario.build();
            let mut labels = String::new();
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    },
                    |ui| app.show(ui),
                );
                assert!(!output.shapes.is_empty(), "{}", scenario.name);
                labels = text(&output.shapes);
                output.textures_delta.clear();
            }
            assert!(
                app.model.error.is_none(),
                "{}: {:?}",
                scenario.name,
                app.model.error
            );
            assert!(app.model.input_error.is_none(), "{}", scenario.name);
            let expected = match scenario.name {
                "conflict-choice" | "conflict-apply" | "gzip-conflict-choice" => {
                    "나머지 파일에도 같게 적용"
                }
                "extract-skipped" => "모든 파일을 건너뛰었습니다",
                "extract-rename-failed" => "아무 파일도 풀지 않았습니다",
                "extract-renamed" => "이름 바꿔 저장: 읽어주세요 (1).txt",
                "empty" => "압축파일",
                "folder-empty" => "이 폴더는 비어 있습니다",
                "dialog-error" => "파일 선택 창을 열 수 없습니다",
                "extract-partial" => "일부 파일만 풀렸습니다",
                "extract-uncertain" => "저장이 끝났는지 확인하지 못했습니다",
                "extract-unsynced" => "파일을 디스크에 완전히 저장했는지 확인하지 못했습니다",
                "source-changed" => "압축파일 다시 열기",
                "gzip-conflict" => "다른 이름으로 저장",
                "create-error" => "ZIP 파일을 만들지 못했습니다",
                "password-create" => "암호 확인",
                "folder" => "바다.jpg",
                "search-empty" => "일치하는 항목이 없습니다",
                "password" | "password-retry" => "암호",
                "extract-complete" => "압축을 풀었습니다",
                "extract-cancelled" => "작업을 취소했습니다",
                "create-complete" => "ZIP 파일을 만들었습니다",
                "create" | "create-encrypted" => "암호로 보호하기",
                "unsupported" | "damaged" => "압축파일을 열지 못했습니다",
                "conflict" => "압축을 풀지 못했습니다",
                "close-confirm" => "닫",
                _ => "읽어주세요.txt",
            };
            assert!(labels.contains(expected), "{}: {labels}", scenario.name);
            if scenario.name == "extract-rename-failed" {
                assert!(labels.contains("파일을 읽거나 쓰지 못했습니다"), "{labels}");
                assert!(!labels.contains("이름 바꿔 저장:"), "{labels}");
            }
            if scenario.name == "many-items" {
                assert_eq!(app.model.entries.len(), 505);
            }
            if matches!(scenario.name, "selection" | "marquee") {
                assert_eq!(app.model.selected.len(), 4);
                assert!(labels.contains("선택 2개"));
            }
            if scenario.name == "password-retry" {
                assert_eq!(app.model.challenge.unwrap().1, 2);
            }
        }
    }
}

#[test]
fn command_line_catalog_and_errors_work_without_a_display() {
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_zipelo-ui-memory-app"))
            .args(args)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap()
    };
    let listed = run(&["--list-ui-scenarios"]);
    assert!(listed.status.success());
    assert_eq!(
        String::from_utf8(listed.stdout).unwrap(),
        scenarios::listing()
    );
    for args in [vec!["--ui-scenario", "unknown"], vec!["--ui-scenario"]] {
        let failed = run(&args);
        assert!(!failed.status.success());
        let stderr = String::from_utf8_lossy(&failed.stderr);
        for scenario in SCENARIOS {
            assert!(stderr.contains(scenario.name));
        }
    }
}

#[test]
fn preview_ignores_password_submission() {
    let ctx = egui::Context::default();
    let mut preview = scenarios::Preview::new(scenarios::parse("password").unwrap().build(), &ctx);
    for _ in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1060.0, 720.0),
                )),
                events: vec![
                    egui::Event::Text("화면 검토".into()),
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ui| preview.0.show(ui),
        );
        output.textures_delta.clear();
    }
    assert_eq!(preview.0.model.challenge, Some((17, 1, false)));
    assert!(preview.0.model.busy());
}

fn result_labels(
    app: &mut zipelo_ui_memory_app::archive_screen::ArchiveApp,
    size: egui::Vec2,
) -> String {
    let ctx = egui::Context::default();
    let mut labels = String::new();
    for _ in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ui| app.show(ui),
        );
        labels = text(&output.shapes);
        output.textures_delta.clear();
    }
    labels
}

#[test]
fn rename_without_created_output_never_claims_saved_name() {
    use zipelo_ui_memory_app::archive_ui::*;
    for size in [egui::vec2(1060.0, 720.0), egui::vec2(760.0, 540.0)] {
        for disposition in [
            None,
            Some(Disposition::PartialRetained),
            Some(Disposition::Removed),
            Some(Disposition::CommitUncertain),
        ] {
            let mut app = scenarios::parse("extract-renamed").unwrap().build();
            app.model.outputs = disposition
                .into_iter()
                .map(|d| ("읽어주세요 (1).txt".into(), d))
                .collect();
            app.model.terminal = Some(Terminal {
                outcome: Outcome::Failed,
                error: Some(ErrorCode::Io),
                publication: if disposition.is_none() {
                    Publication::None
                } else {
                    Publication::Uncertain
                },
                created_count: 0,
                retained_partial_count: 0,
                durability: Durability::NotApplicable,
                cleanup: Cleanup::Complete,
            });
            let labels = result_labels(&mut app, size);
            assert!(labels.contains("파일을 읽거나 쓰지 못했습니다"), "{labels}");
            if disposition.is_none() {
                assert!(labels.contains("아무 파일도 풀지 않았습니다"), "{labels}");
            }
            assert!(
                !labels.contains("이름 바꿔 저장: 읽어주세요 (1).txt"),
                "{labels}"
            );
        }
    }
}

#[test]
fn rename_saved_before_later_failure_remains_reported() {
    use zipelo_ui_memory_app::archive_ui::*;
    for size in [egui::vec2(1060.0, 720.0), egui::vec2(760.0, 540.0)] {
        let mut app = scenarios::parse("extract-renamed").unwrap().build();
        app.model.resolutions.push(Resolution {
            entry_id: 2,
            relative_name: "여행 일정 (1).pdf".into(),
            choice: ConflictChoice::Rename,
        });
        app.model.terminal.as_mut().unwrap().outcome = Outcome::Failed;
        app.model.terminal.as_mut().unwrap().error = Some(ErrorCode::Io);
        app.model.terminal.as_mut().unwrap().publication = Publication::Partial;
        let labels = result_labels(&mut app, size);
        assert!(
            labels.contains("이름 바꿔 저장: 읽어주세요 (1).txt"),
            "{labels}"
        );
        assert!(
            !labels.contains("이름 바꿔 저장: 여행 일정 (1).pdf"),
            "{labels}"
        );
        assert!(labels.contains("파일을 읽거나 쓰지 못했습니다"), "{labels}");
    }
}
