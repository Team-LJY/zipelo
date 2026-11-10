#![forbid(unsafe_code)]

#[path = "../../rust-core/src/budget.rs"]
mod budget;
mod error;
#[path = "../../rust-core/src/json_wire.rs"]
mod json_wire;
mod typed_deflate;
#[path = "../../rust-core/src/wire.rs"]
mod wire;
mod zip_adapter;

use std::{io, process::ExitCode};

use budget::{Budget, Source};
use typed_deflate::Error;
use wire::{Frames, Remote};

fn streaming() -> Result<(), Error> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut input = stdin.lock();
    let config = wire::read_config(&mut input)?;
    let budget = Budget::new(config.limits)?;
    let remote = Remote::new(input, stderr.lock(), &config);
    let source = Source::new(remote, config.length, budget.clone())?;
    let mut frames = Frames::new(stdout.lock(), config.id, budget.clone())?;
    frames.begin()?;
    // stderr는 범위 요청 전용임. 실패는 종료 코드로 구별함.
    zip_adapter::extract_with_evidence(source, &budget, &mut frames)
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).take(2).collect();
    let result = if args.is_empty() {
        json_wire::run(&mut io::stdin().lock(), &mut io::stdout().lock()).map_err(Error::Host)
    } else if args.len() == 1 && args[0] == "--extract-stream-v2" {
        streaming()
    } else {
        return ExitCode::from(2);
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => ExitCode::from(failure.failure().exit_code()),
    }
}
