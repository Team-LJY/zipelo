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

fn preview_catalog_bytes(total: u64, allow: bool) {
    let scratch = Scratch::new();
    let archive = scratch.path("catalog-bytes.zip");
    // 선택하지 않은 큰 항목은 해제하지 않고 ZIP64 목록 예산만 검증함.
    let generated = std::process::Command::new("python3")
        .arg("-c")
        .arg(
            r#"
import struct, sys, zlib
central = []
with open(sys.argv[1], 'wb') as f:
    for name, size, compressed, payload in [
        (b'small.txt', 7, 7, b'preview'),
        (b'large.bin', int(sys.argv[2]) - 7, 75 * 1024 * 1024, None),
    ]:
        offset = f.tell()
        crc = zlib.crc32(payload) if payload else 0
        method = 0 if payload else 8
        extra = struct.pack('<HHQQ', 1, 16, size, compressed)
        f.write(struct.pack('<IHHHHHIIIHH', 0x04034b50, 45, 0, method, 0, 0,
                            crc, 0xffffffff, 0xffffffff, len(name), len(extra)))
        f.write(name + extra)
        if payload:
            f.write(payload)
        else:
            f.seek(compressed, 1)
        central.append(struct.pack('<IHHHHHHIIIHHHHHII', 0x02014b50, 45, 45,
                                   0, method, 0, 0, crc, 0xffffffff, 0xffffffff,
                                   len(name), len(extra), 0, 0, 0, 0, offset)
                       + name + extra)
    offset = f.tell()
    data = b''.join(central)
    f.write(data)
    f.write(struct.pack('<IHHHHIIH', 0x06054b50, 0, 0, 2, 2, len(data), offset, 0))
"#,
        )
        .arg(&archive)
        .arg(total.to_string())
        .output()
        .unwrap();
    assert!(generated.status.success(), "{:?}", generated);
    let mut factory = ProcessFactory::new(binary("zipelo-archive-worker"));
    let mut model = Model::default();
    assert!(model.begin(Start::List { archive }, &mut factory));
    wait(&mut model);
    if total > Limits::default().output {
        assert_eq!(
            model.terminal.as_ref().unwrap().error,
            Some(ErrorCode::Limit)
        );
        assert!(model.retry_expanded(&mut factory));
        finish_catalog_bytes(&mut model, true, ActionKind::List);
    }
    completed(&model);
    assert_eq!(model.entries.len(), 2);
    assert_eq!(
        model.entries.iter().map(|e| e.bytes.unwrap()).sum::<u64>(),
        total
    );
    let id = model
        .entries
        .iter()
        .find(|e| e.name == "small.txt")
        .unwrap()
        .id;
    model.selected.insert(id);
    assert!(model.begin_preview(&mut factory));
    assert_eq!(model.estimated_usage().estimated_bytes, Some(total));
    let questions = finish_catalog_bytes(&mut model, allow, ActionKind::Preview);
    assert_eq!(questions, usize::from(total > Limits::default().output));
    if allow {
        completed(&model);
        assert_eq!(model.preview.as_ref().unwrap().data, b"preview");
    } else {
        assert!(model.preview.is_none());
        assert_ne!(model.terminal.as_ref().unwrap().outcome, Outcome::Completed);
    }
    assert!(model.outputs.is_empty());
    assert_eq!(model.selected, [id].into_iter().collect());
}

fn finish_catalog_bytes(model: &mut Model, allow: bool, action: ActionKind) -> usize {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut questions = 0;
    while model.busy() {
        model.poll();
        if let Some(question) = model.resource.clone() {
            assert!(question.scope.output);
            assert!(!question.scope.ratio);
            assert_eq!(question.action, action);
            assert!(model.resource_reply(model.job_id().unwrap(), question.id, allow));
            questions += 1;
        }
        assert!(Instant::now() < deadline, "{:?}", model.error);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(model.error.is_none(), "{:?}", model.error);
    questions
}

#[test]
fn preview_catalog_at_default_byte_limit_needs_no_approval() {
    preview_catalog_bytes(Limits::default().output, true);
}

#[test]
fn preview_catalog_over_default_byte_limit_requires_new_approval() {
    preview_catalog_bytes(Limits::default().output + 1, true);
}

#[test]
fn preview_catalog_byte_approval_denial_does_not_show_data() {
    preview_catalog_bytes(Limits::default().output + 1, false);
}
