use super::*;
use std::{
    ffi::CString,
    fs::{File, Metadata},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{FileExt, MetadataExt},
        },
    },
};

#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

fn io_error<E>(operation: IoOperation, source: io::Error) -> Error<E> {
    Error::Io { operation, source }
}

fn open<E>(
    directory: i32,
    path: &std::ffi::CStr,
    flags: i32,
    resolve: u64,
    operation: IoOperation,
    run: &mut Run,
) -> Result<File, Error<E>> {
    let how = OpenHow {
        flags: flags as u64,
        mode: 0,
        resolve,
    };
    run.usage.open_calls += 1;
    // 초기화된 구조체와 NUL 종료 문자열을 호출 동안 유지함.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            directory,
            path.as_ptr(),
            &how as *const OpenHow,
            std::mem::size_of::<OpenHow>(),
        )
    };
    if fd < 0 {
        let source = io::Error::last_os_error();
        return Err(io_error(operation, source));
    }
    // 성공한 syscall의 새 fd 소유권을 한 번만 이전함.
    Ok(unsafe { File::from_raw_fd(fd as i32) })
}

fn inspect<E>(file: &File, run: &mut Run) -> Result<Identity, Error<E>> {
    run.usage.metadata_calls += 1;
    let metadata = file
        .metadata()
        .map_err(|e| io_error(IoOperation::Inspect, e))?;
    regular(&metadata)?;
    Ok(Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
        links: metadata.nlink(),
        mode: metadata.mode(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    })
}

fn regular<E>(metadata: &Metadata) -> Result<(), Error<E>> {
    if !metadata.is_file() {
        return Err(Error::Unsupported(Unsupported::FileType));
    }
    if metadata.nlink() != 1 {
        return Err(Error::Unsupported(Unsupported::MultipleLinks));
    }
    Ok(())
}

fn same<E>(before: Identity, after: Identity) -> Result<(), Error<E>> {
    if before.device != after.device || before.inode != after.inode {
        return Err(Error::Changed(Changed::Identity));
    }
    if before.length != after.length {
        return Err(Error::Changed(Changed::Length));
    }
    if before != after {
        return Err(Error::Changed(Changed::Metadata));
    }
    Ok(())
}

fn read<E>(file: &File, output: &mut [u8], offset: u64, run: &mut Run) -> Result<usize, Error<E>> {
    run.reserve_read(output.len())?;
    // EINTR도 원래 오류로 반환하며 자동 재시도로 계수를 숨기지 않음.
    let count = file
        .read_at(output, offset)
        .map_err(|e| io_error(IoOperation::Read, e))?;
    run.usage.actual_read_bytes += count as u64;
    Ok(count)
}

fn eof<E>(file: &File, length: u64, run: &mut Run) -> Result<(), Error<E>> {
    if read(file, &mut [0; 1], length, run)? != 0 {
        return Err(Error::Changed(Changed::Length));
    }
    Ok(())
}

pub(super) fn capture<E>(
    path: &Path,
    run: &mut Run,
    callback: &mut impl FnMut(Checkpoint) -> Result<(), Stop<E>>,
) -> Result<FileSnapshot, Error<E>> {
    run.checkpoint(Stage::Opening, 0, None, callback)?;
    let raw = path.as_os_str().as_bytes();
    if raw.len() < 2
        || raw.len() > 4096
        || raw[0] != b'/'
        || raw[1..]
            .split(|b| *b == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b".." || part.len() > 255)
    {
        return Err(Error::InvalidPath);
    }
    let name = CString::new(raw).map_err(|_| Error::InvalidPath)?;
    let pinned = open(
        libc::AT_FDCWD,
        &name,
        libc::O_PATH | libc::O_CLOEXEC,
        0x04,
        IoOperation::PinPath,
        run,
    )?;
    let identity = inspect(&pinned, run)?;
    if identity.length > run.limits.max_bytes {
        return Err(Error::Limit(LimitKind::Bytes));
    }
    run.checkpoint(Stage::Pinned, 0, Some(identity.length), callback)?;

    // 검증한 procfs 디렉터리에서 보유 regular-file fd만 읽기 모드로 다시 엶.
    let proc = open(
        libc::AT_FDCWD,
        c"/proc",
        libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        0x04,
        IoOperation::OpenProc,
        run,
    )?;
    let mut status = std::mem::MaybeUninit::<libc::statfs>::uninit();
    run.usage.metadata_calls += 1;
    // 성공 반환일 때만 커널이 초기화한 statfs 값을 읽음.
    if unsafe { libc::fstatfs(proc.as_raw_fd(), status.as_mut_ptr()) } != 0 {
        return Err(io_error(
            IoOperation::InspectProc,
            io::Error::last_os_error(),
        ));
    }
    if unsafe { status.assume_init() }.f_type != libc::PROC_SUPER_MAGIC {
        return Err(Error::Unsupported(Unsupported::Procfs));
    }
    let directory_name =
        CString::new(format!("{}/fd", std::process::id())).map_err(|_| Error::InvalidPath)?;
    let descriptors = open(
        proc.as_raw_fd(),
        &directory_name,
        libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        0x0d,
        IoOperation::OpenProcDescriptors,
        run,
    )?;
    let descriptor_name =
        CString::new(pinned.as_raw_fd().to_string()).map_err(|_| Error::InvalidPath)?;
    run.usage.open_calls += 1;
    // 검증한 procfs fd 디렉터리의 보유 fd 링크만 의도적으로 따라감.
    let descriptor = unsafe {
        libc::openat(
            descriptors.as_raw_fd(),
            descriptor_name.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY,
        )
    };
    if descriptor < 0 {
        return Err(io_error(
            IoOperation::ReopenPinned,
            io::Error::last_os_error(),
        ));
    }
    // 새 읽기 fd만 소유하며 원래 경로를 다시 열지 않음.
    let file = unsafe { File::from_raw_fd(descriptor) };
    same(identity, inspect(&file, run)?)?;
    drop(descriptors);
    drop(proc);
    drop(pinned);

    let length = identity.length as usize;
    run.checkpoint(Stage::Copying, 0, Some(identity.length), callback)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(Error::Allocation)?;
    bytes.resize(length, 0);
    let mut offset = 0;
    while offset < length {
        run.checkpoint(
            Stage::Copying,
            offset as u64,
            Some(identity.length),
            callback,
        )?;
        let end = (offset + CHUNK_BYTES).min(length);
        let count = read(&file, &mut bytes[offset..end], offset as u64, run)?;
        if count == 0 {
            return Err(Error::Changed(Changed::Length));
        }
        offset += count;
    }
    run.checkpoint(
        Stage::Copying,
        offset as u64,
        Some(identity.length),
        callback,
    )?;
    eof(&file, identity.length, run)?;
    same(identity, inspect(&file, run)?)?;

    let mut check = [0; CHUNK_BYTES];
    offset = 0;
    while offset < length {
        run.checkpoint(
            Stage::Comparing,
            offset as u64,
            Some(identity.length),
            callback,
        )?;
        let count = read(
            &file,
            &mut check[..(length - offset).min(CHUNK_BYTES)],
            offset as u64,
            run,
        )?;
        if count == 0 {
            return Err(Error::Changed(Changed::Length));
        }
        run.usage.compared_bytes += count as u64;
        if bytes[offset..offset + count] != check[..count] {
            return Err(Error::Changed(Changed::Contents));
        }
        offset += count;
    }
    run.checkpoint(
        Stage::Comparing,
        offset as u64,
        Some(identity.length),
        callback,
    )?;
    eof(&file, identity.length, run)?;
    let sha256 = digest(&bytes);
    run.checkpoint(
        Stage::Finalizing,
        offset as u64,
        Some(identity.length),
        callback,
    )?;
    same(identity, inspect(&file, run)?)?;
    // 마지막 syscall 동안 지난 기한도 확인하며 취소 callback을 재호출하지 않음.
    if run.started.elapsed() >= run.limits.max_duration {
        return Err(Error::Limit(LimitKind::Deadline));
    }
    Ok(FileSnapshot {
        bytes,
        identity,
        sha256,
        usage: run.usage,
    })
}
