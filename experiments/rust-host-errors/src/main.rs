use std::{env, fs::File, io::Read, process::ExitCode};
use zipelo_rust_host_errors::{Limits, Stream};

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: zipelo-rust-host-errors CAPTURE JOB_WIRE_HEX_32".into());
    }
    let text = args[1].to_str().ok_or("job must be ASCII hex")?;
    if text.len() != 32 || !text.is_ascii() {
        return Err("job must contain exactly 32 wire-order hex digits".into());
    }
    let mut job = [0u8; 16];
    for (index, byte) in job.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| "invalid job hex")?;
    }
    let mut file = File::open(&args[0]).map_err(|_| "capture open failed")?;
    let metadata = file.metadata().map_err(|_| "capture metadata failed")?;
    let limits = Limits::default();
    if !metadata.is_file() || metadata.len() > limits.wire_bytes {
        return Err("capture must be a bounded regular file".into());
    }
    let mut stream = Stream::new(job, limits).map_err(|fault| format!("{fault:?}"))?;
    let mut buffer = [0u8; 8192];
    loop {
        let count = file.read(&mut buffer).map_err(|_| "capture read failed")?;
        if count == 0 {
            break;
        }
        stream
            .feed(&buffer[..count])
            .map_err(|fault| format!("raw-frame fault: {fault:?}"))?;
    }
    let summary = stream.finish_input();
    println!("source=offline-capture publication=false process_evidence=unconfirmed");
    println!("stop_required=true capture_summary={summary:?}");
    if let Some(fault) = summary.fault {
        return Err(format!("raw-frame fault: {fault:?}"));
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        // 파일 EOF는 파이프 EOF나 프로세스 종료 증거가 아님.
        Ok(()) => ExitCode::from(2),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
