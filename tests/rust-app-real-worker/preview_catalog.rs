use super::*;

fn finish_preview_catalog(model: &mut Model, allow: bool) -> usize {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut questions = 0;
    while model.busy() {
        model.poll();
        if let Some(question) = model.resource.clone() {
            assert!(question.scope.entries);
            assert!(!question.scope.ratio);
            assert_eq!(question.action, ActionKind::Preview);
            assert_eq!(model.estimated_usage().estimated_entries, Some(10_001));
            assert!(model.resource_reply(model.job_id().unwrap(), question.id, allow));
            questions += 1;
        }
        assert!(Instant::now() < deadline, "{:?}", model.error);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(model.error.is_none(), "{:?}", model.error);
    questions
}

fn preview_catalog(count: usize, bytes: usize, allow: bool, image: bool) {
    let name = if image { "small.png" } else { "small.txt" };
    let scratch = Scratch::new();
    let archive = scratch.path("many.zip");
    let generated = std::process::Command::new("python3")
        .arg("-c")
        .arg("import sys,zipfile; z=zipfile.ZipFile(sys.argv[1], 'w'); z.writestr(sys.argv[4], b'x'*int(sys.argv[3])); [z.writestr(f'{i}.bin', b'') for i in range(int(sys.argv[2])-1)]; z.close()")
        .arg(&archive)
        .arg(count.to_string())
        .arg(bytes.to_string())
        .arg(name)
        .output()
        .unwrap();
    assert!(generated.status.success());
    let mut factory = ProcessFactory::new(binary("zipelo-archive-worker"));
    let mut model = Model::default();
    assert!(model.begin(Start::List { archive }, &mut factory));
    wait(&mut model);
    if count > 10_000 {
        assert_eq!(
            model.terminal.as_ref().unwrap().error,
            Some(ErrorCode::Limit)
        );
        assert!(model.retry_expanded(&mut factory));
        let deadline = Instant::now() + Duration::from_secs(30);
        while model.busy() {
            model.poll();
            if let Some(question) = model.resource.clone() {
                assert!(model.resource_reply(model.job_id().unwrap(), question.id, true));
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    completed(&model);
    assert_eq!(model.entries.len(), count);
    let id = model
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .unwrap()
        .id;
    model.selected.insert(id);
    assert!(model.begin_preview(&mut factory));
    let questions = finish_preview_catalog(&mut model, allow);
    assert_eq!(questions, usize::from(count > 10_000));
    if !allow {
        assert!(model.preview.is_none());
        assert_ne!(model.terminal.as_ref().unwrap().outcome, Outcome::Completed);
    } else if bytes > if image { 8 * 1024 * 1024 } else { 1024 * 1024 } {
        assert_eq!(
            model.terminal.as_ref().unwrap().error,
            Some(ErrorCode::Limit)
        );
        assert!(model.preview.is_none());
        assert!(!model.can_retry_expanded());
    } else {
        completed(&model);
        assert_eq!(model.preview.as_ref().unwrap().data, vec![b'x'; bytes]);
    }
    assert!(model.outputs.is_empty());
    assert_eq!(model.selected, [id].into_iter().collect());
}

#[test]
fn preview_catalog_at_default_entry_limit_needs_no_approval() {
    preview_catalog(10_000, 7, true, false);
}

#[test]
fn preview_catalog_over_default_entry_limit_requires_new_approval() {
    preview_catalog(10_001, 7, true, false);
}

#[test]
fn preview_catalog_approval_does_not_raise_text_size_limit() {
    preview_catalog(10_001, 1024 * 1024 + 1, true, false);
}

#[test]
fn preview_catalog_denial_does_not_show_data() {
    preview_catalog(10_001, 7, false, false);
}

#[test]
fn preview_catalog_approval_does_not_raise_image_size_limit() {
    preview_catalog(10_001, 8 * 1024 * 1024 + 1, true, true);
}
