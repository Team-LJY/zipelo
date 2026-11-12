fn narrow_frame(
    ctx: &egui::Context,
    app: &mut ArchiveApp,
    time: f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 540.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| app.show(ui),
    );
    output.textures_delta.clear();
    output
}

fn layout_text(output: &egui::FullOutput) -> Vec<(String, egui::Rect, bool)> {
    fn walk(shape: &Shape, result: &mut Vec<(String, egui::Rect, bool)>) {
        match shape {
            Shape::Text(text) => result.push((
                text.galley.job.text.clone(),
                text.galley.rect.translate(text.pos.to_vec2()),
                text.galley.elided,
            )),
            Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, result);
                }
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    for shape in &output.shapes {
        walk(&shape.shape, &mut result);
    }
    result
}

fn assert_narrow_text(output: &egui::FullOutput) {
    for (text, rect, _) in layout_text(output) {
        assert!(
            rect.left() >= -1.0 && rect.right() <= 761.0,
            "{text:?}: {rect:?}"
        );
    }
}

fn assert_full_tooltip(ctx: &egui::Context, app: &mut ArchiveApp, point: egui::Pos2, full: &str) {
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.interaction.tooltip_delay = 0.0
    });
    let mut output = narrow_frame(ctx, app, 10.0, vec![egui::Event::PointerMoved(point)]);
    for time in [11.0, 12.0, 13.0] {
        output = narrow_frame(ctx, app, time, vec![]);
    }
    assert!(
        layout_text(&output)
            .iter()
            .any(|(text, _, elided)| text == full && !elided),
        "전체 툴팁 없음: {full} point={point:?} shown={:?}",
        layout_text(&output)
    );
    assert_narrow_text(&output);
}

#[test]
fn narrow_archive_keeps_columns_and_virtualizes_thousands_of_long_names() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let name = format!("{}.txt", "W".repeat(220));
    app.model.entries = (0..5000)
        .map(|id| Entry {
            id,
            name: format!("{id:04}-{name}"),
            kind: Kind::File,
            bytes: Some(31),
            packed: None,
            encrypted: true,
        })
        .collect();
    app.model.selected = (0..5000).collect();
    app.model.archive = Some(
        scratch
            .0
            .join("deep-folder/".repeat(30))
            .join(format!("{name}.zip")),
    );
    let full_title = format!("{} — Zipelo", app.model.archive.as_ref().unwrap().display());
    let ctx = egui::Context::default();
    narrow_frame(&ctx, &mut app, 0.0, vec![]);
    let output = narrow_frame(&ctx, &mut app, 1.0, vec![]);
    assert_narrow_text(&output);
    let text = layout_text(&output);
    assert!(text.len() < 100);
    // 암호가 걸린 항목은 이름 앞에 *가 붙음.
    let first = text
        .iter()
        .find(|(text, _, _)| text.trim_start_matches('*') == format!("0000-{name}"))
        .unwrap();
    assert!(first.2);
    // 크기는 스크롤 막대 자리(오른쪽 24px)를 침범하지 않는 행 안쪽에 남음.
    assert!(
        text.iter()
            .any(|(text, rect, _)| text == "31 B" && rect.right() <= 736.0)
    );
    for expected in ["5000개 항목", "선택 5000개"] {
        assert!(text.iter().any(|(text, _, _)| text == expected));
    }
    // 열 제목 줄(이름, 크기, 암호)은 없고 암호는 이름 앞의 *로만 표시함.
    for removed in ["이름", "크기", "암호"] {
        assert!(!text.iter().any(|(text, _, _)| text == removed));
    }
    let title = output
        .viewport_output
        .values()
        .flat_map(|v| &v.commands)
        .find_map(|cmd| match cmd {
            egui::ViewportCommand::Title(title) => Some(title),
            _ => None,
        })
        .unwrap();
    assert!(title.contains('…') && title.ends_with(" — Zipelo"));
    assert!(title.chars().count() < full_title.chars().count());
    // 툴팁은 표시용 *를 뺀 실제 이름을 보여 줌.
    assert_full_tooltip(
        &ctx,
        &mut app,
        first.1.center(),
        first.0.trim_start_matches('*'),
    );
    let output = narrow_frame(&ctx, &mut app, 14.0, vec![egui::Event::PointerGone]);
    let point = layout_text(&output)
        .into_iter()
        .find(|(text, _, _)| text == "열기")
        .unwrap()
        .1
        .center();
    assert_full_tooltip(&ctx, &mut app, point, &full_title);
}

#[test]
fn narrow_deep_folder_path_is_elided_without_displacing_search() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let folder = "deep-folder/".repeat(40).trim_end_matches('/').to_owned();
    app.model.entries[0].name = format!("{folder}/file.txt");
    app.browse_folder(&folder);
    let ctx = egui::Context::default();
    narrow_frame(&ctx, &mut app, 0.0, vec![]);
    let output = narrow_frame(&ctx, &mut app, 1.0, vec![]);
    assert_narrow_text(&output);
    let text = layout_text(&output);
    let full = format!("/{folder}");
    let path = text.iter().find(|(text, _, _)| text == &full).unwrap();
    assert!(path.2);
    assert!(
        text.iter()
            .any(|(text, rect, _)| text == "파일 검색" && rect.right() < 760.0)
    );
    assert_full_tooltip(&ctx, &mut app, path.1.center(), &full);
}

#[test]
fn narrow_create_sources_keep_remove_and_footer_actions_visible() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let name = format!("{}.txt", "W".repeat(220));
    let path = scratch.0.join(&name);
    fs::write(&path, b"source").unwrap();
    app.add_sources(vec![path]);
    app.show_create(false);
    let ctx = egui::Context::default();
    narrow_frame(&ctx, &mut app, 0.0, vec![]);
    let output = narrow_frame(&ctx, &mut app, 1.0, vec![]);
    assert_narrow_text(&output);
    let text = layout_text(&output);
    assert!(
        text.iter()
            .any(|(text, _, elided)| text == &name && *elided)
    );
    for expected in ["빼기", "취소", "압축하기"] {
        assert!(
            text.iter()
                .any(|(text, rect, _)| text == expected && rect.bottom() <= 540.0)
        );
    }
}

#[test]
fn narrow_result_path_is_elided_and_full_path_remains_in_tooltip() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let output_path = scratch.0.join("nested-output/".repeat(30));
    fs::create_dir_all(&output_path).unwrap();
    app.choose_destination(output_path.clone());
    assert!(app.begin_extract());
    pump(&mut app.model);
    let ctx = egui::Context::default();
    for frame in 0..8 {
        narrow_frame(&ctx, &mut app, frame as f64 / 10.0, vec![]);
    }
    let output = narrow_frame(&ctx, &mut app, 1.0, vec![]);
    assert_narrow_text(&output);
    let text = layout_text(&output);
    let full = format!("{} 폴더에 저장했습니다.", output_path.display());
    let path = text.iter().find(|(text, _, _)| text == &full).unwrap();
    assert!(path.2);
    for expected in ["확인", "폴더 열기"] {
        assert!(text.iter().any(|(text, _, _)| text == expected));
    }
    assert_full_tooltip(&ctx, &mut app, path.1.center(), &full);
}
