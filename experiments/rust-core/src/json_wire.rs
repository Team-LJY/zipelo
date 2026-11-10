use std::{
    io::{Cursor, Read, Write},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use crate::{
    budget::{Budget, Limits, MAX_INPUT, Source},
    error::{Failure, Result},
    wire::parse_id,
    zip_adapter,
};

const MAX_FRAME: usize = 32 * 1024 * 1024;
const EMPTY_ID: &str = "00000000-0000-0000-0000-000000000000";

fn positive(value: &Value, field: &str) -> Result<u64> {
    value
        .get(field)
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .map(|n| n as u64)
        .ok_or(Failure::Protocol)
}

pub fn duration(text: &str) -> Result<Duration> {
    // 5분 이하의 .NET 상수 형식만 허용함.
    let parts: Vec<_> = text.split(':').collect();
    if parts.len() != 3 || parts[0] != "00" || parts[1].len() != 2 {
        return Err(Failure::Protocol);
    }
    let (seconds, fraction) = parts[2].split_once('.').unwrap_or((parts[2], ""));
    if seconds.len() != 2
        || fraction.len() > 7
        || !parts[1]
            .bytes()
            .chain(seconds.bytes())
            .chain(fraction.bytes())
            .all(|c| c.is_ascii_digit())
    {
        return Err(Failure::Protocol);
    }
    let minutes = parts[1].parse::<u64>().map_err(|_| Failure::Protocol)?;
    let seconds = seconds.parse::<u64>().map_err(|_| Failure::Protocol)?;
    if minutes > 5 || seconds >= 60 {
        return Err(Failure::Protocol);
    }
    let mut ticks = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().map_err(|_| Failure::Protocol)?
    };
    for _ in fraction.len()..7 {
        ticks *= 10;
    }
    ticks += (minutes * 60 + seconds) * 10_000_000;
    if ticks == 0 || ticks > 3_000_000_000 {
        return Err(Failure::Unsupported);
    }
    Ok(Duration::from_nanos(ticks * 100))
}

pub fn decode_input(encoded: &str) -> Result<Vec<u8>> {
    let bytes = encoded.as_bytes();
    if bytes.len() % 4 != 0 || bytes.len() > 4 * (MAX_INPUT as usize).div_ceil(3) {
        return Err(Failure::Budget);
    }
    let padding = bytes.iter().rev().take_while(|&&b| b == b'=').count();
    if padding > 2 {
        return Err(Failure::Protocol);
    }
    let size = (bytes.len() / 4 * 3)
        .checked_sub(padding)
        .ok_or(Failure::Protocol)?;
    if size > MAX_INPUT as usize {
        return Err(Failure::Budget);
    }
    let mut output = vec![0; size];
    let written = STANDARD
        .decode_slice(bytes, &mut output)
        .map_err(|_| Failure::Protocol)?;
    if written != size {
        return Err(Failure::Protocol);
    }
    Ok(output)
}

fn handle(request: &Value) -> Result<Vec<Value>> {
    let action = request
        .get("Action")
        .and_then(Value::as_i64)
        .ok_or(Failure::Protocol)?;
    if action != 0 {
        return Err(Failure::Unsupported);
    }
    let inputs = request
        .get("Inputs")
        .and_then(Value::as_array)
        .ok_or(Failure::Protocol)?;
    let names = request
        .get("Names")
        .and_then(Value::as_array)
        .ok_or(Failure::Protocol)?;
    if inputs.len() != 1 || !names.is_empty() {
        return Err(Failure::Unsupported);
    }
    let limits = request.get("Limits").ok_or(Failure::Protocol)?;
    if positive(limits, "MaxPasswordAttempts")? > i32::MAX as u64 {
        return Err(Failure::Protocol);
    }
    let limits = Limits {
        entries: positive(limits, "MaxEntries")?,
        processed: positive(limits, "MaxProcessedBytes")?,
        output: positive(limits, "MaxOutputBytes")?,
        duration: duration(
            limits
                .get("MaxDuration")
                .and_then(Value::as_str)
                .ok_or(Failure::Protocol)?,
        )?,
    }
    .validate()?;
    let budget = Budget::new(limits)?;
    let data = decode_input(inputs[0].as_str().ok_or(Failure::Protocol)?)?;
    let size = data.len() as u64;
    let mut source = Source::new(Cursor::new(data), size, budget.clone())?;
    source.list_accounting()?;
    zip_adapter::list(source, &budget)
}

pub fn run(input: &mut impl Read, output: &mut impl Write) -> Result<()> {
    let mut id = EMPTY_ID.to_owned();
    let result = (|| {
        let mut header = [0; 4];
        input.read_exact(&mut header)?;
        let length = i32::from_le_bytes(header);
        if length <= 0 || length as usize > MAX_FRAME {
            return Err(Failure::Budget);
        }
        let mut bytes = vec![0; length as usize];
        input.read_exact(&mut bytes)?;
        let mut suffix = [0];
        if input.read(&mut suffix)? != 0 {
            return Err(Failure::Protocol);
        }
        let request: Value = serde_json::from_slice(&bytes).map_err(|_| Failure::Protocol)?;
        let value = request
            .get("JobId")
            .and_then(Value::as_str)
            .ok_or(Failure::Protocol)?;
        parse_id(value)?;
        id = value.to_ascii_lowercase();
        handle(&request)
    })();
    let (state, error, entries, detail) = match result {
        Ok(entries) => (4, 0, entries, Value::Null),
        Err(failure) => (
            7,
            failure.archive_code(),
            Vec::new(),
            json!(failure.label()),
        ),
    };
    let response = json!({
        "Job": { "JobId": id, "State": state, "Entries": entries, "Error": error },
        "CreatedZip": null, "ExtractedEntries": null,
        "Probe": {
            "Failure": detail, "ContentCrcVerified": false,
            "MetadataPolicy": "strict-zip32-local-central-v1",
            "DuplicateNames": "reject-before-crate",
            "DeflateTailPolicy": "codec-behavior-requires-comparison"
        }
    });
    let bytes = serde_json::to_vec(&response).map_err(|_| Failure::Engine)?;
    if bytes.len() > MAX_FRAME {
        return Err(Failure::Budget);
    }
    output.write_all(&(bytes.len() as i32).to_le_bytes())?;
    output.write_all(&bytes)?;
    output.flush()?;
    Ok(())
}
