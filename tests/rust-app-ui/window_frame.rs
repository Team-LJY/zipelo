// GNOME Wayland처럼 운영체제가 창 테두리를 그리지 않을 때 쓰는 자체 제목 표시줄 회귀 시험.

fn frame_output(
    ctx: &egui::Context,
    app: &mut ArchiveApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1060.0, 720.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show(ui),
    );
    output.textures_delta.clear();
    output
}

fn root_commands(output: &egui::FullOutput) -> Vec<egui::ViewportCommand> {
    output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|viewport| viewport.commands.clone())
        .unwrap_or_default()
}

fn press_release(
    ctx: &egui::Context,
    app: &mut ArchiveApp,
    point: egui::Pos2,
) -> Vec<egui::ViewportCommand> {
    frame_output(ctx, app, vec![egui::Event::PointerMoved(point)]);
    let mut commands = Vec::new();
    for pressed in [true, false] {
        let output = frame_output(
            ctx,
            app,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        commands.extend(root_commands(&output));
    }
    commands
}

fn window_commands(commands: &[egui::ViewportCommand]) -> Vec<egui::ViewportCommand> {
    // 제목 갱신은 매 프레임 나가므로 창 조작 명령만 비교함.
    commands
        .iter()
        .filter(|command| !matches!(command, egui::ViewportCommand::Title(_)))
        .cloned()
        .collect()
}

#[test]
fn custom_window_frame_shows_title_and_sends_window_commands() {
    use zipelo_ui_memory_app::window_frame::TITLE_HEIGHT;
    // 제목 표시줄 버튼 중심: 오른쪽 여백 6, 버튼 폭 34, 간격 4.
    let close = egui::pos2(1060.0 - 6.0 - 17.0, TITLE_HEIGHT / 2.0);
    let maximize = close - egui::vec2(38.0, 0.0);
    let minimize = maximize - egui::vec2(38.0, 0.0);
    for (point, expected) in [
        (close, egui::ViewportCommand::Close),
        (maximize, egui::ViewportCommand::Maximized(true)),
        (minimize, egui::ViewportCommand::Minimized(true)),
    ] {
        let scratch = Folder::new();
        let factory = FakeFactory::default();
        let mut app = loaded(&scratch, &factory);
        app.set_custom_frame(true);
        let ctx = egui::Context::default();
        let shown = labels(&frame_output(&ctx, &mut app, vec![]).shapes);
        assert!(
            shown
                .iter()
                .any(|(text, pos)| text.ends_with("sample.zip — Zipelo") && pos.y < TITLE_HEIGHT),
            "제목 표시줄에 창 제목이 없음: {shown:?}"
        );
        assert_eq!(
            window_commands(&press_release(&ctx, &mut app, point)),
            [expected]
        );
    }

    // 제목 표시줄을 끌면 운영체제 창 이동을 시작함.
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    app.set_custom_frame(true);
    let ctx = egui::Context::default();
    let start = egui::pos2(300.0, TITLE_HEIGHT / 2.0);
    frame_output(&ctx, &mut app, vec![egui::Event::PointerMoved(start)]);
    let mut commands = root_commands(&frame_output(
        &ctx,
        &mut app,
        vec![egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    ));
    for step in 1..=4 {
        commands.extend(root_commands(&frame_output(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(
                start + egui::vec2(6.0 * step as f32, 0.0),
            )],
        )));
    }
    assert_eq!(
        window_commands(&commands),
        [egui::ViewportCommand::StartDrag]
    );
}

#[test]
fn default_window_frame_draws_no_title_bar_or_window_commands() {
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    let ctx = egui::Context::default();
    let shown = labels(&frame_output(&ctx, &mut app, vec![]).shapes);
    assert!(
        !shown
            .iter()
            .any(|(text, _)| text.ends_with(" — Zipelo")),
        "운영체제가 테두리를 그릴 때는 제목 표시줄을 그리지 않아야 함"
    );
    let close = egui::pos2(1060.0 - 6.0 - 17.0, 19.0);
    assert!(window_commands(&press_release(&ctx, &mut app, close)).is_empty());
}

fn drag_without_release(
    ctx: &egui::Context,
    app: &mut ArchiveApp,
    start: egui::Pos2,
) -> Vec<egui::ViewportCommand> {
    frame_output(ctx, app, vec![egui::Event::PointerMoved(start)]);
    let mut commands = root_commands(&frame_output(
        ctx,
        app,
        vec![egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    ));
    for step in 1..=4 {
        commands.extend(root_commands(&frame_output(
            ctx,
            app,
            vec![egui::Event::PointerMoved(
                start + egui::vec2(6.0 * step as f32, 0.0),
            )],
        )));
    }
    window_commands(&commands)
}

#[test]
fn title_bar_drag_starts_again_when_the_system_swallowed_the_release() {
    use zipelo_ui_memory_app::window_frame::TITLE_HEIGHT;
    let scratch = Folder::new();
    let factory = FakeFactory::default();
    let mut app = loaded(&scratch, &factory);
    app.set_custom_frame(true);
    let ctx = egui::Context::default();
    // GNOME Wayland는 창 이동이 끝나도 버튼 놓기를 앱에 보내지 않음.
    let first = drag_without_release(&ctx, &mut app, egui::pos2(300.0, TITLE_HEIGHT / 2.0));
    assert_eq!(first, [egui::ViewportCommand::StartDrag]);
    let second = drag_without_release(&ctx, &mut app, egui::pos2(500.0, TITLE_HEIGHT / 2.0));
    assert_eq!(second, [egui::ViewportCommand::StartDrag]);

    // 이동을 넘긴 뒤 다음 프레임 입력에 버튼 놓기를 넣어 눌림 상태를 풀어야 함.
    let mut raw = egui::RawInput::default();
    eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
    assert!(raw.events.iter().any(|event| matches!(
        event,
        egui::Event::PointerButton {
            button: egui::PointerButton::Primary,
            pressed: false,
            ..
        }
    )));
    frame_output(&ctx, &mut app, raw.events);
    assert!(!ctx.input(|i| i.pointer.any_down()));
    let mut again = egui::RawInput::default();
    eframe::App::raw_input_hook(&mut app, &ctx, &mut again);
    assert!(again.events.is_empty(), "놓기는 이동 한 번에 한 번만 넣어야 함");
}
