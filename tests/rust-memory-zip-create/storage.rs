use super::Bounded;
use crate::{Checkpoint, Failure, Gate, Stop};
use std::{
    cell::{Cell, RefCell},
    io::{Seek, SeekFrom, Write},
    time::{Duration, Instant},
};

fn gate<C: FnMut(Checkpoint) -> Result<(), Stop>>(control: C) -> Gate<C> {
    Gate {
        control: RefCell::new(control),
        deadline: Instant::now() + Duration::from_secs(300),
        first: Cell::new(None),
    }
}

fn cause(error: std::io::Error) -> Failure {
    *error.get_ref().unwrap().downcast_ref::<Failure>().unwrap()
}

#[test]
fn memory_storage_patch_does_not_double_charge_extent() {
    let gate = gate(|_| Ok(()));
    let mut output = Bounded::new(&gate, 4, 7, 3);
    output.write_all(b"abcd").unwrap();
    output.seek(SeekFrom::Start(1)).unwrap();
    output.write_all(b"XY").unwrap();
    output.seek(SeekFrom::End(0)).unwrap();
    output.flush().unwrap();
    assert_eq!(output.into_bytes(), b"aXYd");
}

#[test]
fn memory_storage_budget_failure_precedes_allocation_and_is_sticky() {
    let calls = Cell::new(0);
    let gate = gate(|_| {
        calls.set(calls.get() + 1);
        Ok(())
    });
    let mut output = Bounded::new(&gate, 4, 7, 3);
    output.write_all(b"abcd").unwrap();
    let before = (
        output.bytes.clone(),
        output.position,
        output.bytes.capacity(),
    );
    assert_eq!(cause(output.write(b"e").unwrap_err()), Failure::Budget);
    let count = calls.get();
    assert_eq!(
        cause(output.seek(SeekFrom::Start(0)).unwrap_err()),
        Failure::Budget
    );
    assert_eq!(cause(output.write(b"Z").unwrap_err()), Failure::Budget);
    assert_eq!(cause(output.flush().unwrap_err()), Failure::Budget);
    assert_eq!(
        cause(output.seek(SeekFrom::Current(i64::MIN)).unwrap_err()),
        Failure::Budget
    );
    assert_eq!(
        (
            output.bytes.clone(),
            output.position,
            output.bytes.capacity()
        ),
        before
    );
    assert_eq!(calls.get(), count);
}

#[test]
fn memory_storage_processed_failure_precedes_extension() {
    let gate = gate(|_| Ok(()));
    let mut output = Bounded::new(&gate, 100, 5, 3);
    output.write_all(b"ab").unwrap();
    let before = (output.bytes.clone(), output.bytes.capacity());
    assert_eq!(cause(output.write(b"c").unwrap_err()), Failure::Budget);
    assert_eq!((output.bytes.clone(), output.bytes.capacity()), before);
}

#[test]
fn memory_storage_checked_processed_sum_rejects_overflow() {
    let gate = gate(|_| Ok(()));
    let mut output = Bounded::new(&gate, u64::MAX, u64::MAX, u64::MAX);
    assert_eq!(cause(output.write(b"x").unwrap_err()), Failure::Budget);
    assert!(output.bytes.is_empty());
    assert_eq!(output.bytes.capacity(), 0);
}

#[test]
fn memory_storage_seek_modes_preserve_bytes_and_reject_holes() {
    for invalid in [
        SeekFrom::Start(5),
        SeekFrom::Current(i64::MIN),
        SeekFrom::End(i64::MAX),
    ] {
        let gate = gate(|_| Ok(()));
        let mut output = Bounded::new(&gate, 4, 4, 0);
        output.write_all(b"abcd").unwrap();
        assert_eq!(output.seek(SeekFrom::Start(0)).unwrap(), 0);
        assert_eq!(output.seek(SeekFrom::Current(2)).unwrap(), 2);
        assert_eq!(output.seek(SeekFrom::End(-1)).unwrap(), 3);
        assert_eq!(cause(output.seek(invalid).unwrap_err()), Failure::Engine);
        assert_eq!(output.position, 3);
        assert_eq!(output.bytes, b"abcd");
        assert_eq!(cause(output.write(b"X").unwrap_err()), Failure::Engine);
    }
}

#[test]
fn memory_storage_cancelled_patch_is_atomic_and_latched() {
    let armed = Cell::new(false);
    let gate = gate(|_| {
        if armed.get() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    });
    let mut output = Bounded::new(&gate, 4, 4, 0);
    output.write_all(b"abcd").unwrap();
    output.seek(SeekFrom::Start(1)).unwrap();
    let before = (output.bytes.clone(), output.position);
    armed.set(true);
    assert_eq!(cause(output.write(b"XY").unwrap_err()), Failure::Cancelled);
    armed.set(false);
    assert_eq!(cause(output.flush().unwrap_err()), Failure::Cancelled);
    assert_eq!(
        cause(output.seek(SeekFrom::Start(0)).unwrap_err()),
        Failure::Cancelled
    );
    assert_eq!((output.bytes.clone(), output.position), before);
}

#[test]
fn memory_storage_expired_real_clock_rejects_patch_without_callback() {
    let calls = Cell::new(0);
    let mut gate = gate(|_| {
        calls.set(calls.get() + 1);
        Ok(())
    });
    gate.deadline = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    let mut output = Bounded::new(&gate, 4, 4, 0);
    // 만료 시점의 저장기 상태를 직접 구성함. 실제 대기 경과 시험은 아님.
    output.bytes = b"abcd".to_vec();
    output.position = 1;
    assert_eq!(cause(output.write(b"XY").unwrap_err()), Failure::Deadline);
    assert_eq!(cause(output.flush().unwrap_err()), Failure::Deadline);
    assert_eq!(
        cause(output.seek(SeekFrom::Start(0)).unwrap_err()),
        Failure::Deadline
    );
    assert_eq!(output.bytes, b"abcd");
    assert_eq!(output.position, 1);
    assert_eq!(calls.get(), 0);
}

#[test]
fn memory_storage_injected_deadline_remains_after_callback_recovers() {
    let armed = Cell::new(false);
    let gate = gate(|_| {
        if armed.get() {
            Err(Stop::Deadline)
        } else {
            Ok(())
        }
    });
    let mut output = Bounded::new(&gate, 4, 4, 0);
    output.write_all(b"abcd").unwrap();
    armed.set(true);
    assert_eq!(
        cause(output.seek(SeekFrom::Start(0)).unwrap_err()),
        Failure::Deadline
    );
    armed.set(false);
    assert_eq!(cause(output.write(b"X").unwrap_err()), Failure::Deadline);
    assert_eq!(output.bytes, b"abcd");
    assert_eq!(output.position, 4);
}

#[test]
fn memory_storage_first_failure_dominates_later_clock_and_callback() {
    let calls = Cell::new(0);
    let mut gate = gate(|_| {
        calls.set(calls.get() + 1);
        Err(Stop::Cancelled)
    });
    assert_eq!(gate.fail(Failure::Engine), Failure::Engine);
    assert_eq!(gate.fail(Failure::Allocation), Failure::Engine);
    gate.deadline = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    let mut output = Bounded::new(&gate, 0, 0, 0);
    assert_eq!(cause(output.write(b"x").unwrap_err()), Failure::Engine);
    assert_eq!(cause(output.flush().unwrap_err()), Failure::Engine);
    assert_eq!(calls.get(), 0);
    assert!(output.bytes.is_empty());
}

#[test]
fn memory_storage_typed_failure_survives_io_and_zip_wrapping() {
    for failure in [
        Failure::Budget,
        Failure::Cancelled,
        Failure::Deadline,
        Failure::Allocation,
        Failure::Engine,
    ] {
        assert_eq!(crate::io_failure(std::io::Error::other(failure)), failure);
        assert_eq!(
            crate::zip_failure(zip::result::ZipError::Io(std::io::Error::other(failure))),
            failure
        );
    }
    assert_eq!(
        crate::io_failure(std::io::Error::other("synthetic")),
        Failure::Engine
    );
}

#[test]
fn memory_storage_drop_cannot_clear_preexisting_failure() {
    for method in [
        zip::CompressionMethod::Stored,
        zip::CompressionMethod::Deflated,
    ] {
        let gate = gate(|_| Ok(()));
        let mut output = Bounded::new(&gate, 4096, 8192, 0);
        let mut writer = zip::ZipWriter::new(&mut output);
        let options = zip::write::SimpleFileOptions::default().compression_method(method);
        writer.start_file("a", options).unwrap();
        writer.write_all(b"123456789").unwrap();
        gate.fail(Failure::Cancelled);
        drop(writer);
        assert_eq!(gate.first.get(), Some(Failure::Cancelled));
        assert_eq!(cause(output.write(b"x").unwrap_err()), Failure::Cancelled);
        assert_eq!(cause(output.flush().unwrap_err()), Failure::Cancelled);
    }
}
