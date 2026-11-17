//! 열어 둔 폴더 핸들에 묶어서 폴더 안의 내용을 지움.
//! 경로를 다시 해석하지 않으므로 확인한 뒤 경로가 바뀌어도 다른 폴더를 지우지 않음.

use std::fs::File;
use std::io;
use std::path::Path;

/// `dir` 핸들이 가리키는 폴더 안의 모든 항목을 지움. 폴더 자체는 지우지 않음.
/// Linux는 핸들 기준 `unlinkat`으로 지우며 `path`는 쓰지 않음.
/// Windows는 호출한 쪽이 공유 삭제를 허용하지 않고 `dir`을 열어 두어 폴더 이름이
/// 바뀌지 않게 한 상태에서만 `path`를 씀.
pub fn remove_contents(dir: &File, path: &Path) -> io::Result<()> {
    platform::remove_contents(dir, path)
}

/// `dir` 핸들이 가리키는 폴더가 비어 있는지 확인함. 새로 만든 폴더를 핸들로 잡은 직후 쓰임.
pub fn is_empty(dir: &File, path: &Path) -> io::Result<bool> {
    platform::is_empty(dir, path)
}

#[cfg(target_os = "linux")]
mod platform {
    use std::ffi::{CStr, CString};
    use std::fs::File;
    use std::io;
    use std::os::unix::{
        fs::MetadataExt,
        io::{AsRawFd, FromRawFd},
    };
    use std::path::Path;

    #[cfg(any(target_arch = "aarch64", target_arch = "arm"))]
    const NOFOLLOW_DIRECTORY: i32 = 0x8000 | 0x4000;
    #[cfg(not(any(target_arch = "aarch64", target_arch = "arm")))]
    const NOFOLLOW_DIRECTORY: i32 = 0x20000 | 0x10000;
    const CLOEXEC: i32 = 0x80000;
    const AT_REMOVEDIR: i32 = 0x200;
    const ENOENT: i32 = 2;
    const EPERM: i32 = 1;
    const EISDIR: i32 = 21;
    /// 깊게 중첩된 폴더에서 파일 디스크립터가 바닥나지 않게 하는 한도.
    const MAX_DEPTH: usize = 256;

    #[repr(C)]
    struct Dirent64 {
        inode: u64,
        offset: i64,
        length: u16,
        kind: u8,
        name: [i8; 256],
    }

    unsafe extern "C" {
        fn openat(fd: i32, path: *const i8, flags: i32, ...) -> i32;
        fn unlinkat(fd: i32, path: *const i8, flags: i32) -> i32;
        fn fdopendir(fd: i32) -> *mut std::ffi::c_void;
        fn readdir64(stream: *mut std::ffi::c_void) -> *mut Dirent64;
        fn rewinddir(stream: *mut std::ffi::c_void);
        fn closedir(stream: *mut std::ffi::c_void) -> i32;
    }

    pub fn remove_contents(dir: &File, _path: &Path) -> io::Result<()> {
        let device = dir.metadata()?.dev();
        remove_children(dir, device, 0)
    }

    pub fn is_empty(dir: &File, _path: &Path) -> io::Result<bool> {
        Ok(names(dir)?.is_empty())
    }

    fn names(dir: &File) -> io::Result<Vec<CString>> {
        let copy = dir.try_clone()?;
        let fd = std::os::unix::io::IntoRawFd::into_raw_fd(copy);
        // 복제한 디스크립터의 소유권은 폴더 스트림이 가져가며 closedir이 닫음.
        let stream = unsafe { fdopendir(fd) };
        if stream.is_null() {
            let error = io::Error::last_os_error();
            // 스트림을 만들지 못했으므로 복제한 디스크립터를 직접 돌려받아 닫음.
            drop(unsafe { File::from_raw_fd(fd) });
            return Err(error);
        }
        // 복제한 디스크립터는 원본과 읽기 위치를 공유하므로 처음부터 읽음.
        unsafe { rewinddir(stream) };
        let mut found = Vec::new();
        loop {
            let entry = unsafe { readdir64(stream) };
            if entry.is_null() {
                break;
            }
            // 반환된 항목은 다음 readdir64 호출 전까지 유효하며 바로 복사함.
            let name = unsafe { CStr::from_ptr((*entry).name.as_ptr()) };
            let bytes = name.to_bytes();
            if bytes != b"." && bytes != b".." {
                found.push(name.to_owned());
            }
        }
        unsafe { closedir(stream) };
        Ok(found)
    }

    fn unlink(dir: &File, name: &CString, flags: i32) -> io::Result<()> {
        // 부모 핸들 기준으로 이름 하나만 지움.
        if unsafe { unlinkat(dir.as_raw_fd(), name.as_ptr(), flags) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn open_directory(dir: &File, name: &CString) -> io::Result<File> {
        // 심볼릭 링크를 따르지 않고 폴더만 부모 핸들 기준으로 엶.
        let fd = unsafe {
            openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                NOFOLLOW_DIRECTORY | CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn remove_children(dir: &File, device: u64, depth: usize) -> io::Result<()> {
        let mut failure = None;
        for name in names(dir)? {
            if let Err(error) = remove_entry(dir, &name, device, depth) {
                if error.raw_os_error() != Some(ENOENT) {
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    fn remove_entry(dir: &File, name: &CString, device: u64, depth: usize) -> io::Result<()> {
        match unlink(dir, name, 0) {
            Ok(()) => return Ok(()),
            Err(error) if matches!(error.raw_os_error(), Some(EISDIR | EPERM)) => {}
            Err(error) => return Err(error),
        }
        if depth >= MAX_DEPTH {
            return Err(io::Error::other("폴더가 너무 깊음"));
        }
        let child = match open_directory(dir, name) {
            Ok(child) => child,
            // 그 사이 폴더가 아닌 항목으로 바뀌었으면 항목 자체를 지움.
            Err(_) => return unlink(dir, name, 0),
        };
        // 다른 파일 시스템이 연결된 폴더 안은 지우지 않음.
        if child.metadata()?.dev() != device {
            return Err(io::Error::other("다른 파일 시스템의 폴더"));
        }
        remove_children(&child, device, depth + 1)?;
        drop(child);
        unlink(dir, name, AT_REMOVEDIR)
    }
}

#[cfg(windows)]
mod platform {
    use std::fs::{self, File};
    use std::io;
    use std::path::Path;

    pub fn is_empty(_dir: &File, path: &Path) -> io::Result<bool> {
        // 호출한 쪽의 핸들이 이름 변경을 막고 있으므로 경로가 같은 폴더를 가리킴.
        Ok(fs::read_dir(path)?.next().is_none())
    }

    pub fn remove_contents(_dir: &File, path: &Path) -> io::Result<()> {
        let mut failure = None;
        // 호출한 쪽의 핸들이 폴더 이름 변경을 막고 있으므로 경로가 같은 폴더를 가리킴.
        for entry in fs::read_dir(path)? {
            let result = entry.and_then(|entry| {
                let path = entry.path();
                let kind = fs::symlink_metadata(&path)?.file_type();
                if kind.is_symlink() {
                    // 링크는 따라가지 않고 링크 자체만 지움.
                    fs::remove_file(&path).or_else(|_| fs::remove_dir(&path))
                } else if kind.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                }
            });
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}
