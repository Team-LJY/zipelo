use crate::protocol::*;
use std::fs::File;

type Result<T> = std::result::Result<T, ErrorCode>;

pub(crate) fn volume(file: &File) -> Result<u64> {
    Ok(match crate::filesystem::object(file)? {
        SourceObject::Unix { device, .. } => device,
        SourceObject::Windows { volume_serial, .. } => volume_serial,
        SourceObject::SplitSevenZip { .. } => return Err(ErrorCode::Protocol),
    })
}

pub(crate) fn ensure(file: &File, bytes: u64) -> Result<()> {
    if bytes == 0 || bytes as u128 <= available(file)? {
        Ok(())
    } else {
        Err(ErrorCode::DiskFull)
    }
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
pub(crate) fn available(file: &File) -> Result<u128> {
    use std::os::fd::AsRawFd;
    #[repr(C)]
    #[derive(Default)]
    struct Vfs {
        block_size: u64,
        fragment_size: u64,
        blocks: u64,
        free_blocks: u64,
        available_blocks: u64,
        files: u64,
        free_files: u64,
        available_files: u64,
        filesystem_id: u64,
        flags: u64,
        name_max: u64,
        reserved: [i32; 6],
    }
    unsafe extern "C" {
        fn fstatvfs(fd: i32, value: *mut Vfs) -> i32;
    }
    let mut value = Vfs::default();
    if unsafe { fstatvfs(file.as_raw_fd(), &mut value) } != 0 || value.fragment_size == 0 {
        return Err(ErrorCode::Io);
    }
    Ok(value.available_blocks as u128 * value.fragment_size as u128)
}

#[cfg(all(target_os = "linux", not(target_pointer_width = "64")))]
pub(crate) fn available(_: &File) -> Result<u128> {
    Err(ErrorCode::UnsupportedFeature)
}

#[cfg(windows)]
pub(crate) fn available(file: &File) -> Result<u128> {
    use std::{
        ffi::OsString,
        os::windows::{
            ffi::{OsStrExt, OsStringExt},
            io::AsRawHandle,
        },
        path::PathBuf,
    };
    unsafe extern "system" {
        fn GetFinalPathNameByHandleW(
            handle: *mut std::ffi::c_void,
            path: *mut u16,
            capacity: u32,
            flags: u32,
        ) -> u32;
        fn GetDiskFreeSpaceExW(
            path: *const u16,
            available: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> i32;
    }
    let mut buffer = vec![0u16; 32768];
    let mut length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    };
    if length == 0 {
        length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                1,
            )
        };
    }
    if length == 0 || length as usize >= buffer.len() {
        return Err(ErrorCode::Io);
    }
    let mut path = PathBuf::from(OsString::from_wide(&buffer[..length as usize]));
    if !file.metadata().map_err(|_| ErrorCode::Io)?.is_dir() && !path.pop() {
        return Err(ErrorCode::Io);
    }
    let mut wide: Vec<_> = path.as_os_str().encode_wide().collect();
    if wide.last() != Some(&92) {
        wide.push(92);
    }
    wide.push(0);
    let mut free = 0;
    if unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(ErrorCode::Io);
    }
    Ok(free as u128)
}

// 공간 부족만 전용 오류로 변환함.
pub(crate) fn io_error(error: std::io::Error) -> ErrorCode {
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(39 | 112)) {
        return ErrorCode::DiskFull;
    }
    if error.kind() == std::io::ErrorKind::StorageFull {
        ErrorCode::DiskFull
    } else {
        ErrorCode::Io
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/space_errors.rs"]
mod tests;
