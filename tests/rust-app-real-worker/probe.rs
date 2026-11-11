use std::{io::Write, path::PathBuf, time::Duration};
use worker_protocol::{
    protocol::*,
    wire::{Reader, Writer},
};

fn main() {
    let executable = PathBuf::from(std::env::args_os().next().unwrap());
    let mode = executable.file_stem().unwrap().to_string_lossy();
    if mode.starts_with("hang") {
        let parent = executable.parent().unwrap();
        std::fs::write(parent.join("probe.pid.tmp"), std::process::id().to_string()).unwrap();
        std::fs::rename(parent.join("probe.pid.tmp"), parent.join("probe.pid")).unwrap();
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let mut reader = Reader::new(std::io::stdin());
    let (job, _, _) = reader.read().unwrap().unwrap();
    if mode.starts_with("wrong-action") {
        Writer::new(std::io::stdout(), job)
            .send(&Message::Accepted {
                action: ActionKind::Extract,
                encryption: false,
            })
            .unwrap();
    } else if !mode.starts_with("early-exit") {
        std::io::stdout().write_all(b"ZAW1").unwrap();
    }
}
