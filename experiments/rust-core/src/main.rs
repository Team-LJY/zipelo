#![forbid(unsafe_code)]

mod budget;
mod error;
mod json_wire;
mod wire;
mod zip_adapter;

use std::{io, process::ExitCode};

use budget::{Budget, Source};
use error::Result;
use wire::{Frames, Remote};

fn streaming() -> Result<()> {
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
    zip_adapter::extract(source, &budget, &mut frames)
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).take(2).collect();
    let result = if args.is_empty() {
        json_wire::run(&mut io::stdin().lock(), &mut io::stdout().lock())
    } else if args.len() == 1 && args[0] == "--extract-stream-v2" {
        streaming()
    } else {
        return ExitCode::from(2);
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => ExitCode::from(failure.exit_code()),
    }
}
