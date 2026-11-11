use crate::protocol::ErrorCode;
type Result<T> = std::result::Result<T, ErrorCode>;

#[cfg(target_os = "linux")]
pub(crate) struct Parent(i32);
#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn getppid() -> i32;
}
#[cfg(target_os = "linux")]
impl Parent {
    pub fn capture() -> Result<Self> {
        let id = unsafe { getppid() };
        if id <= 1 {
            return Err(ErrorCode::Io);
        }
        Ok(Self(id))
    }
    pub fn check(&self) -> Result<()> {
        if unsafe { getppid() } == self.0 {
            Ok(())
        } else {
            Err(ErrorCode::Io)
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    type Handle = *mut c_void;
    #[repr(C)]
    struct Entry {
        size: u32,
        usage: u32,
        pid: u32,
        heap: usize,
        module: u32,
        threads: u32,
        parent: u32,
        priority: i32,
        flags: u32,
        executable: [u16; 260],
    }
    #[repr(C)]
    #[derive(Default)]
    struct Time {
        low: u32,
        high: u32,
    }
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        fn Process32FirstW(snapshot: Handle, entry: *mut Entry) -> i32;
        fn Process32NextW(snapshot: Handle, entry: *mut Entry) -> i32;
        fn GetCurrentProcessId() -> u32;
        fn GetCurrentProcess() -> Handle;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn GetProcessTimes(
            handle: Handle,
            creation: *mut Time,
            exit: *mut Time,
            kernel: *mut Time,
            user: *mut Time,
        ) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    fn created(handle: Handle) -> Result<u64> {
        let (mut c, mut e, mut k, mut u) = (
            Time::default(),
            Time::default(),
            Time::default(),
            Time::default(),
        );
        if unsafe { GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u) } == 0 {
            return Err(ErrorCode::Io);
        }
        Ok((c.high as u64) << 32 | c.low as u64)
    }
    pub(crate) struct Parent(Handle);
    impl Parent {
        pub fn capture() -> Result<Self> {
            let snapshot = unsafe { CreateToolhelp32Snapshot(2, 0) };
            if snapshot as isize == -1 {
                return Err(ErrorCode::Io);
            }
            let mut entry: Entry = unsafe { std::mem::zeroed() };
            entry.size = size_of::<Entry>() as u32;
            let pid = unsafe { GetCurrentProcessId() };
            let mut valid = unsafe { Process32FirstW(snapshot, &mut entry) };
            let mut parent = 0;
            while valid != 0 {
                if entry.pid == pid {
                    parent = entry.parent;
                    break;
                }
                valid = unsafe { Process32NextW(snapshot, &mut entry) };
            }
            unsafe {
                CloseHandle(snapshot);
            }
            if parent == 0 {
                return Err(ErrorCode::Io);
            }
            let handle = unsafe { OpenProcess(0x100000 | 0x1000, 0, parent) };
            if handle.is_null() {
                return Err(ErrorCode::Io);
            }
            let guard = Self(handle);
            // 부모 PID가 재사용된 경우 새 프로세스에 연결하지 않음.
            if created(handle)? > created(unsafe { GetCurrentProcess() })? {
                return Err(ErrorCode::Io);
            }
            guard.check()?;
            Ok(guard)
        }
        pub fn check(&self) -> Result<()> {
            if unsafe { WaitForSingleObject(self.0, 0) } == 258 {
                Ok(())
            } else {
                Err(ErrorCode::Io)
            }
        }
    }
    impl Drop for Parent {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
#[cfg(windows)]
pub(crate) use windows::Parent;
