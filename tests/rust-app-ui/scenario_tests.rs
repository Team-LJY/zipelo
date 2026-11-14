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
                    |ui| scenario.show(&mut app, ui),
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
            if scenario.name == "split-archive" {
                assert!(app.model.list_id.as_ref().unwrap().is_split());
                assert!(!app.model.can_preview());
            }
            let expected = match scenario.name {
                "split-archive" => "읽어주세요.txt",
                "split-first-volume" => "첫 조각 여행.7z.001을 선택해 다시 여세요.",
                "split-missing-volume" => "여행.7z.003 조각이 없습니다",
                "split-incomplete" => "분할 묶음이 불완전하거나 손상됐습니다",
                "split-unsupported" => "분할 ZIP과 RAR",
                "split-source-changed" => "원본 파일이 바뀌었습니다",
                "preview-implicit-folder" => "자료",
                "preview-catalog-approval" | "preview-catalog-bytes-approval" => {
                    "큰 작업을 허용할까요?"
                }
                "preview-loading" => "미리보기를 불러오는 중입니다",
                "preview-text" => "여행 준비 목록",
                "preview-image" => "미리보기",
                "preview-empty" => "빈 텍스트 파일입니다",
                "preview-limit" => "미리보기 크기 또는 처리 한도를 초과했습니다",
                "preview-damaged" => "파일이 손상됐거나 UTF-8 텍스트가 아닙니다",
                "preview-unsupported" => "이 파일 형식은 미리보기를 지원하지 않습니다",
                "resource-approval" => "큰 작업을 허용할까요?",
                "resource-ratio" => "압축률이 매우 높습니다",
                "resource-retry" => "한도를 늘려 다시 시도",
                "resource-partial" => "저장 위치를 확인하세요",
                "uninstall-windows" => "제거 전에 다른 앱을 선택하세요",
                "uninstall-settings-error" => "Windows 설정을 열지 못했습니다",
                "uninstall-ubuntu-missing" => "이전 앱을 복원할 수 없습니다",
                "uninstall-ubuntu-unknown-user" => "사용자 기본 앱 설정은 변경하지 않습니다",
                "conflict-choice" | "conflict-apply" | "gzip-conflict-choice" => {
                    "나머지 파일에도 같게 적용"
                }
                "extract-skipped" => "모든 파일을 건너뛰었습니다",
                "extract-rename-failed" => "아무 파일도 풀지 않았습니다",
                "extract-renamed" => "이름 바꿔 저장: 읽어주세요 (1).txt",
                "empty" => "TAR.BZ2, TAR.XZ",
                "tar-bz2" | "tar-xz" => "읽어주세요.txt",
                "folder-empty" => "이 폴더는 비어 있습니다",
                "dialog-error" => "파일 선택 창을 열 수 없습니다",
                "extract-partial" => "일부 파일만 풀렸습니다",
                "extract-uncertain" => "저장이 끝났는지 확인하지 못했습니다",
                "extract-unsynced" => "파일을 디스크에 완전히 저장했는지 확인하지 못했습니다",
                "source-changed" => "압축파일 다시 열기",
                "gzip-conflict" => "다른 이름으로 저장",
                "create-sevenzip" => "7z는 암호를 지원하지 않습니다",
                "create-sevenzip-progress" => "파일을 저장하는 중",
                "create-sevenzip-complete" => "7z 파일을 만들었습니다",
                "create-sevenzip-error" => "7z 파일을 만들지 못했습니다",
                "create-sevenzip-cancelled" => "작업을 취소했습니다",
                "create-sevenzip-resource" => "큰 작업을 허용할까요?",
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
            if matches!(scenario.name, "tar-bz2" | "tar-xz") {
                assert!(app.model.entries.iter().all(|entry| !entry.encrypted));
                assert_eq!(
                    app.model.list_id.as_ref().unwrap().format,
                    zipelo_ui_memory_app::archive_ui::Format::Tar
                );
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

#[test]
fn uninstall_previews_match_packaged_notices_and_do_not_start_jobs() {
    let installer = include_str!("../../packaging/windows/rust/installer.nsi");
    let prerm = include_str!("../../packaging/ubuntu/rust/prerm");
    let helper = include_str!("../../packaging/ubuntu/rust/default-apps");
    for (name, source, expected) in [
        (
            "uninstall-windows",
            installer,
            "직접 고른 기본 앱은 자동으로 바꾸지 않습니다.",
        ),
        (
            "uninstall-settings-error",
            installer,
            "Windows 설정을 열지 못했습니다.",
        ),
        (
            "uninstall-ubuntu-missing",
            helper,
            "이전 앱을 복원할 수 없습니다:",
        ),
        (
            "uninstall-ubuntu-unknown-user",
            prerm,
            "제거를 요청한 사용자를 확인할 수 없어 사용자 기본 앱 설정은 변경하지 않습니다.",
        ),
    ] {
        assert!(source.contains(expected));
        let scenario = scenarios::parse(name).unwrap();
        let ctx = egui::Context::default();
        let mut preview = scenarios::Preview::new(scenario.build(), &ctx).with_scenario(scenario);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 540.0),
                )),
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| preview.show(ui),
        );
        assert!(text(&output.shapes).contains(expected), "{name}");
        assert!(!preview.0.model.busy());
        assert!(preview.0.model.archive.is_none());
        assert!(output.platform_output.commands.is_empty());
        output.textures_delta.clear();
    }
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

fn resource_frame(
    app: &mut zipelo_ui_memory_app::archive_screen::ArchiveApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> Vec<egui::epaint::ClippedShape> {
    let mut out = ctx.run_ui(
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
    out.textures_delta.clear();
    out.shapes
}
fn find_resource_action(shapes: &[egui::epaint::ClippedShape], label: &str) -> egui::Pos2 {
    fn find(s: &egui::Shape, label: &str) -> Option<egui::Pos2> {
        match s {
            egui::Shape::Text(t) if t.galley.text() == label => {
                Some(t.pos + t.galley.rect.size() / 2.0)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, label)),
            _ => None,
        }
    }
    shapes.iter().find_map(|s| find(&s.shape, label)).unwrap()
}
#[test]
fn resource_dialog_requires_explicit_click_and_escape_cancels() {
    for allow in [false, true] {
        let ctx = egui::Context::default();
        let mut app = scenarios::parse("resource-ratio").unwrap().build();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            shapes = resource_frame(&mut app, &ctx, vec![]);
        }
        assert!(text(&shapes).contains("미확인"));
        resource_frame(
            &mut app,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(app.model.resource.is_some());
        if allow {
            let point = find_resource_action(&shapes, "이 작업 허용");
            resource_frame(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
            );
            resource_frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
            );
            assert!(!app.model.cancellation_requested());
        } else {
            resource_frame(
                &mut app,
                &ctx,
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
            assert!(app.model.cancellation_requested());
        }
        assert!(app.model.resource.is_none());
    }
}
#[test]
fn partial_limit_result_never_offers_expanded_retry() {
    let ctx = egui::Context::default();
    let mut app = scenarios::parse("resource-partial").unwrap().build();
    let mut shapes = Vec::new();
    for _ in 0..3 {
        shapes = resource_frame(&mut app, &ctx, vec![]);
    }
    assert!(!text(&shapes).contains("한도를 늘려 다시 시도"));
    assert!(text(&shapes).contains("풀린 자료"));
}

#[test]
fn preview_escape_discards_content_and_returns_to_list_without_changing_selection() {
    for name in ["preview-text", "preview-image", "preview-loading"] {
        let ctx = egui::Context::default();
        let mut app = scenarios::parse(name).unwrap().build();
        let selected = app.model.selected.clone();
        for _ in 0..3 {
            resource_frame(&mut app, &ctx, vec![]);
        }
        assert!(text(&resource_frame(&mut app, &ctx, vec![])).contains("미리보기"));
        resource_frame(
            &mut app,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        for _ in 0..3 {
            resource_frame(&mut app, &ctx, vec![]);
        }
        assert!(app.model.preview.is_none());
        assert_eq!(app.model.selected, selected);
        if name == "preview-loading" {
            assert!(app.model.cancellation_requested());
        } else {
            assert_eq!(
                ctx.memory(|m| m.focused()),
                Some(egui::Id::unique("archive-list"))
            );
        }
    }
}

#[test]
fn preview_image_respects_renderer_texture_limit() {
    let ctx = egui::Context::default();
    let mut app = scenarios::parse("preview-image").unwrap().build();
    app.model.preview = Some(zipelo_ui_memory_app::archive_ui::Preview {
        info: zipelo_ui_memory_app::archive_ui::PreviewInfo {
            entry_id: 1,
            format: zipelo_ui_memory_app::archive_ui::PreviewFormat::Rgba8,
            width: 2048,
            height: 1,
            total_bytes: 8192,
        },
        data: vec![255; 8192],
    });
    let mut image_seen = false;
    for _ in 0..3 {
        let mut result = ctx.run_ui(
            egui::RawInput {
                max_texture_side: Some(1024),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 540.0),
                )),
                ..Default::default()
            },
            |ui| app.show(ui),
        );
        for (id, delta) in &result.textures_delta.set {
            if *id == egui::TextureId::Managed(0) {
                continue;
            }
            for image in delta {
                assert!(image.image.size().iter().all(|n| *n <= 1024));
            }
            image_seen = true;
        }
        result.textures_delta.clear();
    }
    assert!(image_seen);
}

#[test]
fn split_errors_use_main_modal_without_native_child() {
    for name in [
        "split-first-volume",
        "split-missing-volume",
        "split-incomplete",
        "split-unsupported",
    ] {
        let scenario = scenarios::parse(name).unwrap();
        let mut app = scenario.build();
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        let mut labels = String::new();
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1060.0, 720.0),
                    )),
                    ..Default::default()
                },
                |ui| scenario.show(&mut app, ui),
            );
            labels = text(&output.shapes);
            output.textures_delta.clear();
            assert!(
                output
                    .viewport_output
                    .keys()
                    .all(|id| *id == egui::ViewportId::ROOT),
                "{name}"
            );
        }
        assert!(labels.contains("작업 결과"), "{name}: {labels}");
    }
}

#[test]
fn excluded_split_errors_show_support_guidance_in_main_modal() {
    use zipelo_ui_memory_app::archive_ui::ErrorCode;

    for code in [ErrorCode::UnsupportedFeature, ErrorCode::UnsupportedFormat] {
        for archive in ["sample.zip.001", "sample.z01", "sample.rar", "sample.7z"] {
            let scenario = scenarios::parse("split-unsupported").unwrap();
            let mut app = scenario.build();
            let ctx = egui::Context::default();
            ctx.set_embed_viewports(false);
            let input = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1060.0, 720.0),
                )),
                ..Default::default()
            };
            for _ in 0..3 {
                let mut output = ctx.run_ui(input(), |ui| scenario.show(&mut app, ui));
                output.textures_delta.clear();
            }
            assert_eq!(
                app.model.terminal.as_ref().unwrap().error,
                Some(ErrorCode::UnsupportedFeature)
            );
            app.model.terminal.as_mut().unwrap().error = Some(code);
            app.model.archive = Some(archive.into());
            let mut output = ctx.run_ui(input(), |ui| scenario.show(&mut app, ui));
            let labels = text(&output.shapes);
            output.textures_delta.clear();
            if archive == "sample.7z" {
                assert!(!labels.contains("분할 ZIP과 RAR"));
            } else {
                assert!(
                    labels.contains("분할 ZIP과 RAR"),
                    "{code:?} {archive}: {labels}"
                );
                assert!(
                    output
                        .viewport_output
                        .keys()
                        .all(|id| *id == egui::ViewportId::ROOT)
                );
            }
        }
    }
}
