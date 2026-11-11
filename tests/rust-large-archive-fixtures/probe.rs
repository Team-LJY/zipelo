use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
use zipelo_archive_worker::{
    protocol::*,
    wire::{Reader, Writer},
};

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    assert_eq!(
        args.len(),
        9,
        "worker archive destination input_limit output_limit entries outcome payload_size gzip"
    );
    let source = PathBuf::from(&args[1]);
    let destination = PathBuf::from(&args[2]);
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    let failed = args[6] == "Failed";
    assert!(failed || args[6] == "Completed");
    let input_limit = args[3].to_str().unwrap().parse().unwrap();
    let output_limit = args[4].to_str().unwrap().parse().unwrap();
    let entries_limit = args[5].to_str().unwrap().parse().unwrap();
    let payload_size: u64 = args[7].to_str().unwrap().parse().unwrap();
    let start = Start {
        action: Action::Extract {
            archive: NativePath::from_path(&source),
            destination: NativePath::from_path(&destination),
            selection: Selection::All,
            list_id: None,
            gzip_output_name: (args[8] == "Gzip").then(|| "payload.bin".to_owned()),
        },
        limits: Limits {
            input: input_limit,
            output: output_limit,
            entries: entries_limit,
        },
    };
    let mut request = Vec::new();
    Writer::new(&mut request, 17)
        .send(&Message::Start(start))
        .unwrap();
    let mut child = Command::new(&args[0])
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
    let mut terminals = 0;
    let mut outputs = 0;
    while let Some((job, _, message)) = reader.read().unwrap() {
        assert_eq!(job, 17);
        assert_eq!(terminals, 0, "Terminal 뒤 이벤트가 있음");
        match message {
            Message::Output { .. } => outputs += 1,
            Message::Terminal(t) => {
                terminals += 1;
                assert_eq!(
                    t.outcome,
                    if failed {
                        Outcome::Failed
                    } else {
                        Outcome::Completed
                    }
                );
                assert_eq!(
                    t.error,
                    if failed {
                        ErrorCode::Limit
                    } else {
                        ErrorCode::None
                    }
                );
                assert_eq!(
                    t.publication,
                    if failed {
                        Publication::None
                    } else {
                        Publication::Complete
                    }
                );
                assert_eq!(t.created_count, if failed { 0 } else { 1 });
                assert_eq!(t.retained_partial_count, 0);
                assert_eq!(t.cleanup, Cleanup::Complete);
            }
            _ => {}
        }
    }
    assert_eq!(terminals, 1);
    assert_eq!(outputs, if failed { 0 } else { 1 });
    assert_eq!(
        fs::read_dir(&destination).unwrap().count(),
        if failed { 0 } else { 1 }
    );
    if !failed {
        assert_eq!(
            fs::metadata(destination.join("payload.bin")).unwrap().len(),
            payload_size
        );
    }
}
