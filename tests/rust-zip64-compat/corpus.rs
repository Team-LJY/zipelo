#![cfg(target_os = "linux")]

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use zipelo_archive_worker::{
    protocol::*,
    wire::{Reader, Writer},
};

fn execute(action: Action) -> Vec<Message> {
    let mut request = Vec::new();
    Writer::new(&mut request, 17)
        .send(&Message::Start(Start {
            action,
            limits: Limits::default(),
        }))
        .unwrap();
    let worker = std::env::var_os("ZIPELO_ZIP64_WORKER")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_zip64-test-worker").into());
    let mut child = Command::new(worker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&request).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let mut reader = Reader::new(output.stdout.as_slice());
    let mut events = Vec::new();
    while let Some((job, _, event)) = reader.read().unwrap() {
        assert_eq!(job, 17);
        assert!(!matches!(events.last(), Some(Message::Terminal(_))));
        events.push(event);
    }
    assert!(matches!(events.last(), Some(Message::Terminal(_))));
    events
}

fn terminal(events: &[Message]) -> &Terminal {
    match events.last().unwrap() {
        Message::Terminal(value) => value,
        _ => unreachable!(),
    }
}

fn directory(path: &Path) {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
}

#[test]
fn zip64_tool_and_field_corpus_lists_and_extracts() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let runs = root.join("runs");
    fs::create_dir_all(&runs).unwrap();
    let scratch = runs.join(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    directory(&scratch);
    let corpus = scratch.join("corpus");
    assert!(
        Command::new("python3")
            .arg(root.join("generate.py"))
            .arg(&corpus)
            .status()
            .unwrap()
            .success()
    );
    let mut report = String::from("case\tlist\textract\n");
    let mut failures = Vec::new();
    let manifest = fs::read_to_string(corpus.join("cases.tsv")).unwrap();
    let mut count = 0;
    for line in manifest.lines() {
        let columns: Vec<_> = line.split('\t').collect();
        assert_eq!(columns.len(), 3);
        let (id, name, hex) = (columns[0], columns[1], columns[2]);
        let expected: Vec<_> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let archive = NativePath::from_path(&corpus.join(format!("{id}.zip")));
        let listed = execute(Action::List {
            archive: archive.clone(),
        });
        let list_id = listed.iter().find_map(|event| match event {
            Message::ListEnd {
                entry_count: 1,
                list_id,
            } => Some(list_id.clone()),
            _ => None,
        });
        let entries: Vec<_> = listed
            .iter()
            .filter_map(|event| match event {
                Message::Entry(entry) => Some(entry),
                _ => None,
            })
            .collect();
        let list_ok = terminal(&listed).outcome == Outcome::Completed
            && terminal(&listed).error == ErrorCode::None
            && list_id.is_some()
            && entries.len() == 1
            && entries[0].name == name
            && entries[0].size == expected.len() as u64;
        let destination = scratch.join(id);
        directory(&destination);
        let extracted = execute(Action::Extract {
            archive,
            destination: NativePath::from_path(&destination),
            selection: Selection::All,
            list_id,
            gzip_output_name: None,
        });
        let end = terminal(&extracted);
        let extract_ok = end.outcome == Outcome::Completed
            && end.error == ErrorCode::None
            && end.publication == Publication::Complete
            && end.cleanup == Cleanup::Complete
            && end.created_count == 1
            && end.retained_partial_count == 0
            && fs::read(destination.join(name)).ok().as_deref() == Some(expected.as_slice())
            && fs::read_dir(&destination).unwrap().count() == 1;
        report.push_str(&format!(
            "{id}\t{}:{:?}\t{}:{:?}\n",
            if list_ok { "PASS" } else { "FAIL" },
            terminal(&listed).error,
            if extract_ok { "PASS" } else { "FAIL" },
            end.error
        ));
        if !list_ok || !extract_ok {
            failures.push(id.to_owned());
        }
        count += 1;
    }
    assert_eq!(count, 1066);
    fs::write(scratch.join("results.tsv"), report).unwrap();
    println!(
        "ZIP64 corpus: {count} cases, {} failed; {}",
        failures.len(),
        scratch.display()
    );
    assert!(
        failures.is_empty(),
        "실패 {}개, 앞 12개: {:?}; 전체 결과: {}",
        failures.len(),
        &failures[..failures.len().min(12)],
        scratch.join("results.tsv").display()
    );
}
