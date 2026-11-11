use super::*;
use std::{
    ffi::{CStr, CString},
    fs::{File, Metadata},
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd, RawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
};

const NO_SYMLINKS: u64 = 0x04;
const BENEATH: u64 = 0x08;
const NO_XDEV: u64 = 0x01;

#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

#[derive(Default)]
struct Handles {
    parent: Option<File>,
    temp: Option<File>,
    proc_root: Option<File>,
    proc_fds: Option<File>,
}

impl Handles {
    fn close_all(&mut self) -> Cleanup {
        let mut cleanup = Cleanup::empty();
        for (resource, file) in [
            (Resource::TemporaryFile, self.temp.take()),
            (Resource::ProcFdDirectory, self.proc_fds.take()),
            (Resource::ProcRoot, self.proc_root.take()),
            (Resource::ParentDirectory, self.parent.take()),
        ] {
            if let Some(file) = file {
                let fd = file.into_raw_fd();
                let index = cleanup.attempted as usize;
                cleanup.attempted += 1;
                // 소유 fd를 한 번만 닫으며 Linux close 오류를 재시도하지 않음.
                if unsafe { libc::close(fd) } != 0 {
                    cleanup.failures[index] = Some(CloseFailure {
                        resource,
                        source: io::Error::last_os_error(),
                    });
                }
            }
        }
        cleanup
    }
}

pub(super) fn publish<E>(
    request: Request<'_>,
    run: &mut Run<'_>,
    callback: &mut impl FnMut(Checkpoint) -> Result<(), Stop<E>>,
) -> Report<E> {
    let mut handles = Handles::default();
    let outcome = match perform(request, run, callback, &mut handles) {
        Ok(outcome) => outcome,
        Err(error) => Outcome::NotCommitted(error),
    };
    let cleanup = handles.close_all();
    Report {
        outcome,
        usage: run.usage,
        cleanup,
    }
}

fn io_error<E>(operation: &'static str, source: io::Error) -> Error<E> {
    Error::Io { operation, source }
}

fn opened<E>(fd: libc::c_long, operation: &'static str) -> Result<File, Error<E>> {
    if fd < 0 {
        return Err(io_error(operation, io::Error::last_os_error()));
    }
    // 성공한 open 계열 호출의 새 fd 소유권을 정확히 한 번 인수함.
    Ok(unsafe { File::from_raw_fd(fd as RawFd) })
}

fn open2<E>(
    dir: RawFd,
    path: &CStr,
    flags: i32,
    resolve: u64,
    run: &mut Run<'_>,
) -> Result<File, Error<E>> {
    let how = OpenHow {
        flags: flags as u64,
        mode: 0,
        resolve,
    };
    run.usage.open_calls += 1;
    // NUL 종료 경로와 초기화한 구조체를 호출 동안 유지함.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            dir,
            path.as_ptr(),
            &how as *const OpenHow,
            std::mem::size_of::<OpenHow>(),
        )
    };
    opened(fd, "openat2")
}

fn metadata<E>(file: &File, run: &mut Run<'_>) -> Result<Metadata, Error<E>> {
    run.usage.metadata_calls += 1;
    file.metadata().map_err(|error| io_error("fstat", error))
}

fn filesystem<E>(file: &File, run: &mut Run<'_>) -> Result<u64, Error<E>> {
    let mut value = MaybeUninit::<libc::statfs>::uninit();
    run.usage.metadata_calls += 1;
    // 성공한 fstatfs만 초기화된 구조체로 읽음.
    if unsafe { libc::fstatfs(file.as_raw_fd(), value.as_mut_ptr()) } != 0 {
        return Err(io_error("fstatfs", io::Error::last_os_error()));
    }
    Ok(unsafe { value.assume_init() }.f_type as u64)
}

fn identity(value: &Metadata) -> Identity {
    Identity {
        device: value.dev(),
        inode: value.ino(),
    }
}

fn paths<E>(request: &Request<'_>) -> Result<(CString, CString), Error<E>> {
    let parent = request.parent.as_os_str().as_bytes();
    let leaf = request.leaf.as_bytes();
    if parent.first() != Some(&b'/')
        || parent.len() > 4095
        || (parent != b"/"
            && parent[1..]
                .split(|v| *v == b'/')
                .any(|v| v.is_empty() || v == b"." || v == b".."))
        || leaf.is_empty()
        || leaf.len() > 255
        || leaf == b"."
        || leaf == b".."
        || leaf.contains(&b'/')
    {
        return Err(Error::InvalidPath);
    }
    Ok((
        CString::new(parent).map_err(|_| Error::InvalidPath)?,
        CString::new(leaf).map_err(|_| Error::InvalidPath)?,
    ))
}

fn perform<E>(
    request: Request<'_>,
    run: &mut Run<'_>,
    callback: &mut impl FnMut(Checkpoint) -> Result<(), Stop<E>>,
    handles: &mut Handles,
) -> Result<Outcome<E>, Error<E>> {
    let (parent_path, leaf) = paths(&request)?;
    run.check(Stage::Opening, 0, callback)?;
    handles.parent = Some(open2(
        libc::AT_FDCWD,
        &parent_path,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NONBLOCK | libc::O_NOCTTY,
        NO_SYMLINKS,
        run,
    )?);
    let parent = handles.parent.as_ref().expect("owned parent");
    let parent_meta = metadata(parent, run)?;
    // 유효 UID를 읽을 뿐 권한이나 신원을 변경하지 않음.
    let uid = unsafe { libc::geteuid() };
    if !parent_meta.is_dir() || parent_meta.uid() != uid || parent_meta.mode() & 0o022 != 0 {
        return Err(Error::UntrustedParent);
    }
    match filesystem(parent, run)? {
        0xef53 | 0x58465342 => {}
        _ => return Err(Error::UnsupportedFilesystem),
    }
    run.check(Stage::Opening, 0, callback)?;
    run.usage.open_calls += 1;
    // 고정 부모 아래 이름 없는 파일을 만들며 O_EXCL을 사용하지 않음.
    handles.temp = Some(opened(
        unsafe {
            libc::openat(
                parent.as_raw_fd(),
                c".".as_ptr(),
                libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC,
                0o600 as libc::mode_t,
            )
        } as libc::c_long,
        "openat_tmpfile",
    )?);
    let temp = handles.temp.as_mut().expect("owned temporary file");
    let initial = metadata(temp, run)?;
    if !initial.is_file()
        || initial.nlink() != 0
        || initial.uid() != uid
        || initial.dev() != parent_meta.dev()
    {
        return Err(Error::InvalidTemporaryFile);
    }
    let mut offset = 0;
    while offset < request.bytes.len() {
        run.check(Stage::Writing, offset, callback)?;
        let end = (offset + CHUNK_BYTES).min(request.bytes.len());
        run.reserve(end - offset, true)?;
        // 읽기 전용 입력 슬라이스의 유효 범위만 커널에 전달함.
        let result = unsafe {
            libc::write(
                temp.as_raw_fd(),
                request.bytes[offset..end].as_ptr().cast(),
                end - offset,
            )
        };
        if result < 0 {
            return Err(io_error("write", io::Error::last_os_error()));
        }
        let count = result as usize;
        run.usage.actual_written_bytes += count as u64;
        if count == 0 {
            return Err(io_error("write", io::Error::from(io::ErrorKind::WriteZero)));
        }
        offset += count;
    }
    run.check(Stage::Verifying, 0, callback)?;
    run.usage.sync_calls += 1;
    sync(temp).map_err(|error| io_error("fsync_file", error))?;
    // 소유한 임시 파일의 읽기 위치만 이동함.
    if unsafe { libc::lseek(temp.as_raw_fd(), 0, libc::SEEK_SET) } < 0 {
        return Err(io_error("lseek", io::Error::last_os_error()));
    }
    let mut scratch = [0_u8; CHUNK_BYTES];
    offset = 0;
    while offset < request.bytes.len() {
        run.check(Stage::Verifying, offset, callback)?;
        let count = CHUNK_BYTES.min(request.bytes.len() - offset);
        run.reserve(count, false)?;
        let read =
            read_once(temp, &mut scratch[..count]).map_err(|error| io_error("read", error))?;
        run.usage.actual_read_bytes += read as u64;
        if read == 0 || scratch[..read] != request.bytes[offset..offset + read] {
            return Err(Error::VerificationMismatch);
        }
        offset += read;
    }
    run.check(Stage::Verifying, offset, callback)?;
    run.reserve(1, false)?;
    let extra = read_once(temp, &mut scratch[..1]).map_err(|error| io_error("read_eof", error))?;
    run.usage.actual_read_bytes += extra as u64;
    if extra != 0 {
        return Err(Error::VerificationMismatch);
    }
    let final_meta = metadata(temp, run)?;
    if identity(&final_meta) != identity(&initial)
        || !final_meta.is_file()
        || final_meta.nlink() != 0
        || final_meta.len() != request.bytes.len() as u64
    {
        return Err(Error::VerificationMismatch);
    }
    // procfs 연결 방식은 파일 계층용이며 worker 실행 우회가 아님.
    handles.proc_root = Some(open2(
        libc::AT_FDCWD,
        c"/proc",
        libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        NO_SYMLINKS,
        run,
    )?);
    let proc_root = handles.proc_root.as_ref().expect("owned proc root");
    if filesystem(proc_root, run)? != 0x9fa0 {
        return Err(Error::UnsupportedFilesystem);
    }
    // 현재 프로세스의 숫자 경로를 사용해 self 심볼릭 링크를 피함.
    let pid = unsafe { libc::getpid() };
    let proc_path = CString::new(format!("{pid}/fd")).map_err(|_| Error::InvalidPath)?;
    handles.proc_fds = Some(open2(
        proc_root.as_raw_fd(),
        &proc_path,
        libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
        BENEATH | NO_SYMLINKS | NO_XDEV,
        run,
    )?);
    let proc_fds = handles.proc_fds.as_ref().expect("owned proc fd directory");
    let fd_name = CString::new(temp.as_raw_fd().to_string()).map_err(|_| Error::InvalidPath)?;
    run.check(Stage::BeforeCommit, offset, callback)?;
    run.begin_commit()?;
    run.usage.link_calls += 1;
    // CAS 이후 callback 없이 소유 fd와 고정 부모 사이의 원자적 링크를 한 번 시도함.
    let linked = unsafe {
        libc::linkat(
            proc_fds.as_raw_fd(),
            fd_name.as_ptr(),
            parent.as_raw_fd(),
            leaf.as_ptr(),
            libc::AT_SYMLINK_FOLLOW,
        )
    };
    if linked != 0 {
        let source = io::Error::last_os_error();
        if source.raw_os_error() == Some(libc::EEXIST) {
            return Err(io_error("linkat_conflict", source));
        }
        return Ok(Outcome::CommitUncertain { source });
    }
    let receipt = FileCommitReceipt {
        parent: identity(&parent_meta),
        file: identity(&initial),
        length: run.length,
    };
    run.usage.sync_calls += 1;
    let durability = match sync(parent) {
        Ok(()) => Durability::FileAndParentSynced,
        Err(error) => Durability::ParentSyncFailed(error),
    };
    Ok(Outcome::Committed {
        receipt,
        durability,
    })
}

fn read_once(file: &File, buffer: &mut [u8]) -> io::Result<usize> {
    // 독점 대여한 버퍼의 길이 안에서만 읽으며 EINTR도 원형으로 반환함.
    let result = unsafe { libc::read(file.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result as usize)
    }
}

fn sync(file: &File) -> io::Result<()> {
    // 소유 fd에 한 번만 fsync를 호출함.
    if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
