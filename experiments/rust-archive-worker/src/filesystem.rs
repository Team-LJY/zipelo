use crate::archive::valid_name;
use crate::protocol::*;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
#[cfg(windows)]
use std::path::PathBuf;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

type Result<T> = std::result::Result<T, ErrorCode>;
static TEMP: AtomicU64 = AtomicU64::new(0);
pub struct Snapshot {
    pub bytes: Vec<u8>,
    pub object: SourceObject,
    pub hash: [u8; 32],
}
#[cfg(unix)]
pub fn object(file: &File) -> Result<SourceObject> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata().map_err(|_| ErrorCode::Io)?;
    Ok(SourceObject::Unix {
        device: m.dev(),
        inode: m.ino(),
    })
}
#[cfg(windows)]
pub fn object(file: &File) -> Result<SourceObject> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct Info {
        volume: u64,
        id: [u8; 16],
    }
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(
            h: *mut std::ffi::c_void,
            class: u32,
            info: *mut std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    let mut info = Info {
        volume: 0,
        id: [0; 16],
    };
    // 열린 핸들에 대해 고정 크기의 파일 식별 정보를 조회함.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            18,
            (&mut info as *mut Info).cast(),
            size_of::<Info>() as u32,
        )
    } == 0
    {
        return Err(ErrorCode::UnsupportedFeature);
    }
    Ok(SourceObject::Windows {
        volume_serial: info.volume,
        file_id: info.id,
    })
}
pub fn snapshot(
    path: &Path,
    limit: u64,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Snapshot> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x20000 | 0x800);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(path).map_err(|_| ErrorCode::Io)?;
    let before = file.metadata().map_err(|_| ErrorCode::Io)?;
    if !before.is_file() {
        return Err(ErrorCode::InvalidRequest);
    }
    if before.len() > limit {
        return Err(ErrorCode::Limit);
    }
    let identity = object(&file)?;
    let mut bytes = Vec::new();
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        check()?;
        let n = file.read(&mut buffer).map_err(|_| ErrorCode::Io)?;
        if n == 0 {
            break;
        }
        if bytes.len() as u64 + n as u64 > limit {
            return Err(ErrorCode::Limit);
        }
        bytes.try_reserve(n).map_err(|_| ErrorCode::Limit)?;
        bytes.extend_from_slice(&buffer[..n]);
        hash.update(&buffer[..n]);
    }
    let after = file.metadata().map_err(|_| ErrorCode::Io)?;
    let named = options.open(path).map_err(|_| ErrorCode::SourceChanged)?;
    if !named
        .metadata()
        .map_err(|_| ErrorCode::SourceChanged)?
        .is_file()
        || object(&named)? != identity
        || object(&file)? != identity
        || before.len() != after.len()
        || after.len() != bytes.len() as u64
        || before.modified().ok() != after.modified().ok()
    {
        return Err(ErrorCode::SourceChanged);
    }
    close(file).map_err(|_| ErrorCode::Io)?;
    Ok(Snapshot {
        bytes,
        object: identity,
        hash: hash.finalize().into(),
    })
}
#[cfg(unix)]
fn close(file: File) -> std::io::Result<()> {
    use std::os::unix::io::IntoRawFd;
    unsafe extern "C" {
        fn close(fd: i32) -> i32;
    }
    // 소유한 파일 디스크립터를 한 번만 닫음.
    if unsafe { close(file.into_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
#[cfg(windows)]
fn close(file: File) -> std::io::Result<()> {
    use std::os::windows::io::IntoRawHandle;
    unsafe extern "system" {
        fn CloseHandle(h: *mut std::ffi::c_void) -> i32;
    }
    // 소유한 핸들을 한 번만 닫음.
    if unsafe { CloseHandle(file.into_raw_handle()) } != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawFd, FromRawFd},
    };
    unsafe extern "C" {
        fn openat(fd: i32, path: *const i8, flags: i32, ...) -> i32;
        fn mkdirat(fd: i32, path: *const i8, mode: u32) -> i32;
        fn linkat(fd: i32, path: *const i8, newfd: i32, newpath: *const i8, flags: i32) -> i32;
        fn unlinkat(fd: i32, path: *const i8, flags: i32) -> i32;
        fn geteuid() -> u32;
    }
    pub struct Dir {
        file: File,
    }
    fn name(s: &std::ffi::OsStr) -> Result<CString> {
        CString::new(s.as_bytes()).map_err(|_| ErrorCode::UnsafePath)
    }
    fn open(parent: &File, s: &std::ffi::OsStr, flags: i32) -> std::io::Result<File> {
        let n = name(s).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        // 부모 핸들 기준으로 심볼릭 링크를 따르지 않고 열음.
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                n.as_ptr(),
                flags | 0x80000 | 0x20000,
                0o600u32,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    impl Dir {
        pub fn root(path: &Path) -> Result<Self> {
            if !path.is_absolute() {
                return Err(ErrorCode::InvalidRequest);
            }
            let mut d = Self {
                file: File::open("/").map_err(|_| ErrorCode::Io)?,
            };
            for c in path.components() {
                match c {
                    Component::RootDir => {}
                    Component::Normal(n) => {
                        d = Self {
                            file: open(&d.file, n, 0x10000).map_err(|_| ErrorCode::UnsafePath)?,
                        }
                    }
                    _ => return Err(ErrorCode::UnsafePath),
                }
            }
            d.trusted()?;
            Ok(d)
        }
        fn trusted(&self) -> Result<()> {
            let m = self.file.metadata().map_err(|_| ErrorCode::Io)?;
            // 다른 계정이 바꿀 수 있는 출력 폴더를 거부함.
            if !m.is_dir() || m.uid() != unsafe { geteuid() } || m.mode() & 0o022 != 0 {
                return Err(ErrorCode::UnsafePath);
            }
            Ok(())
        }
        pub fn child(&self, n: &str, create: bool) -> Result<Option<Self>> {
            match open(&self.file, std::ffi::OsStr::new(n), 0x10000) {
                Ok(file) => {
                    let d = Self { file };
                    d.trusted()?;
                    Ok(Some(d))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if !create {
                        return Ok(None);
                    }
                    let n = CString::new(n).map_err(|_| ErrorCode::UnsafePath)?;
                    // 고정 부모 안에 새 전용 디렉터리를 생성함.
                    if unsafe { mkdirat(self.file.as_raw_fd(), n.as_ptr(), 0o700) } != 0 {
                        return Err(ErrorCode::Io);
                    }
                    let file = open(
                        &self.file,
                        std::ffi::OsStr::new(n.to_str().unwrap()),
                        0x10000,
                    )
                    .map_err(|_| ErrorCode::UnsafePath)?;
                    let d = Self { file };
                    d.trusted()?;
                    Ok(Some(d))
                }
                Err(_) => Err(ErrorCode::UnsafePath),
            }
        }
        pub(crate) fn handle(&self) -> &File {
            &self.file
        }
        pub fn duplicate(&self) -> Result<Self> {
            Ok(Self {
                file: self.file.try_clone().map_err(|_| ErrorCode::Io)?,
            })
        }
        pub fn absent(&self, n: &str) -> Result<bool> {
            match open(&self.file, std::ffi::OsStr::new(n), 0x200000) {
                Ok(_) => Ok(false),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
                Err(_) => Err(ErrorCode::Io),
            }
        }
        pub fn create(&self, n: &str) -> std::io::Result<File> {
            open(&self.file, std::ffi::OsStr::new(n), 1 | 0x40 | 0x80)
        }
        pub fn link(&self, from: &str, to: &str) -> std::io::Result<()> {
            let a = CString::new(from).unwrap();
            let b = CString::new(to).unwrap();
            // 배타적 hard link 게시로 기존 이름을 보존함.
            if unsafe {
                linkat(
                    self.file.as_raw_fd(),
                    a.as_ptr(),
                    self.file.as_raw_fd(),
                    b.as_ptr(),
                    0,
                )
            } == 0
            {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        }
        pub fn remove(&self, n: &str) -> bool {
            let n = CString::new(n).unwrap();
            unsafe { unlinkat(self.file.as_raw_fd(), n.as_ptr(), 0) == 0 }
        }
        pub fn sync(&self) -> Durability {
            if self.file.sync_all().is_ok() {
                Durability::Synced
            } else {
                Durability::Failed
            }
        }
    }
}
#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    pub struct Dir {
        path: PathBuf,
        locks: Vec<File>,
    }
    fn lock(path: &Path) -> Result<File> {
        let f = OpenOptions::new()
            .access_mode(0x80)
            .share_mode(3)
            .custom_flags(0x02000000 | 0x00200000)
            .open(path)
            .map_err(|_| ErrorCode::UnsafePath)?;
        let m = f.metadata().map_err(|_| ErrorCode::Io)?;
        if !m.is_dir() || m.file_attributes() & 0x400 != 0 {
            return Err(ErrorCode::UnsafePath);
        }
        Ok(f)
    }
    impl Dir {
        pub fn root(path: &Path) -> Result<Self> {
            let mut p = PathBuf::new();
            let mut locks = Vec::new();
            for c in path.components() {
                match c {
                    Component::Prefix(_) | Component::RootDir => p.push(c.as_os_str()),
                    Component::Normal(n) => {
                        if locks.is_empty() {
                            locks.push(lock(&p)?);
                        }
                        p.push(n);
                        locks.push(lock(&p)?);
                    }
                    _ => return Err(ErrorCode::UnsafePath),
                }
            }
            if locks.is_empty() {
                locks.push(lock(&p)?);
            }
            Ok(Self { path: p, locks })
        }
        pub(crate) fn handle(&self) -> &File {
            self.locks.last().unwrap()
        }
        pub fn duplicate(&self) -> Result<Self> {
            Ok(Self {
                path: self.path.clone(),
                locks: self
                    .locks
                    .iter()
                    .map(|f| f.try_clone().map_err(|_| ErrorCode::Io))
                    .collect::<Result<_>>()?,
            })
        }
        pub fn child(&self, n: &str, create: bool) -> Result<Option<Self>> {
            let p = self.path.join(n);
            match std::fs::symlink_metadata(&p) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if !create {
                        return Ok(None);
                    }
                    std::fs::create_dir(&p).map_err(|_| ErrorCode::Io)?;
                }
                Err(_) => return Err(ErrorCode::Io),
            }
            let mut d = self.duplicate()?;
            d.locks.push(lock(&p)?);
            d.path = p;
            Ok(Some(d))
        }
        pub fn absent(&self, n: &str) -> Result<bool> {
            match std::fs::symlink_metadata(self.path.join(n)) {
                Ok(_) => Ok(false),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
                Err(_) => Err(ErrorCode::Io),
            }
        }
        pub fn create(&self, n: &str) -> std::io::Result<File> {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .share_mode(0)
                .custom_flags(0x00200000)
                .open(self.path.join(n))
        }
        pub fn link(&self, a: &str, b: &str) -> std::io::Result<()> {
            std::fs::hard_link(self.path.join(a), self.path.join(b))
        }
        pub fn remove(&self, n: &str) -> bool {
            std::fs::remove_file(self.path.join(n)).is_ok()
        }
        pub fn sync(&self) -> Durability {
            Durability::Unconfirmed
        }
    }
}
#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("Archive worker supports Linux and Windows only");
pub use platform::Dir;
fn parent(root: &Dir, name: &str, create: bool) -> Result<Option<(Dir, String)>> {
    let mut parts = name.split('/').collect::<Vec<_>>();
    let leaf = parts.pop().ok_or(ErrorCode::UnsafePath)?;
    let mut d = root.duplicate()?;
    for p in parts {
        match d.child(p, create)? {
            Some(child) => d = child,
            None => return Ok(None),
        }
    }
    Ok(Some((d, leaf.to_owned())))
}
pub fn preflight(root: &Dir, names: &[(String, EntryKind)]) -> Result<()> {
    for (name, kind) in names {
        if let Some((d, leaf)) = parent(root, name, false)? {
            if *kind == EntryKind::Directory {
                let _ = d.child(&leaf, false)?;
            } else if !d.absent(&leaf)? {
                return Err(ErrorCode::Conflict);
            }
        }
    }
    Ok(())
}
pub(crate) fn preflight_space(
    root: &Dir,
    files: &[(String, u64)],
    staging: Option<(&File, u64)>,
) -> Result<()> {
    let mut volumes = std::collections::BTreeMap::<u64, (File, u64)>::new();
    let mut charge = |file: &File, bytes: u64| -> Result<()> {
        crate::space::ensure(file, bytes)?;
        let volume = crate::space::volume(file)?;
        if let Some((_, total)) = volumes.get_mut(&volume) {
            *total = total.checked_add(bytes).ok_or(ErrorCode::Limit)?;
        } else {
            volumes.insert(
                volume,
                (file.try_clone().map_err(|_| ErrorCode::Io)?, bytes),
            );
        }
        Ok(())
    };
    if let Some((file, bytes)) = staging {
        charge(file, bytes)?;
    }
    for (name, size) in files {
        valid_name(name, false)?;
        let mut parent = root.duplicate()?;
        let mut parts = name.split('/').collect::<Vec<_>>();
        parts.pop();
        for part in parts {
            match parent.child(part, false)? {
                Some(child) => parent = child,
                None => break,
            }
        }
        charge(parent.handle(), *size)?;
    }
    for (file, bytes) in volumes.values() {
        crate::space::ensure(file, *bytes)?;
    }
    Ok(())
}
pub fn directory(root: &Dir, name: &str) -> Result<(bool, Durability)> {
    let (d, leaf) = parent(root, name, false)?.ok_or(ErrorCode::Io)?;
    if d.child(&leaf, false)?.is_some() {
        return Ok((false, Durability::NotApplicable));
    }
    d.child(&leaf, true)?.ok_or(ErrorCode::Io)?;
    Ok((true, d.sync()))
}
pub struct Published {
    pub created: bool,
    pub uncertain: bool,
    pub error: Option<ErrorCode>,
    pub durability: Durability,
    pub cleanup: Cleanup,
    pub retained: u64,
    pub retained_name: Option<String>,
}
pub fn publish(
    root: &Dir,
    name: &str,
    data: &[u8],
    check: &mut impl FnMut(bool) -> Result<()>,
) -> Published {
    publish_reader(
        root,
        name,
        &mut std::io::Cursor::new(data),
        data.len() as u64,
        check,
    )
}

pub fn publish_reader(
    root: &Dir,
    name: &str,
    data: &mut impl Read,
    length: u64,
    check: &mut impl FnMut(bool) -> Result<()>,
) -> Published {
    let mut result = Published {
        created: false,
        uncertain: false,
        error: None,
        durability: Durability::NotApplicable,
        cleanup: Cleanup::Complete,
        retained: 0,
        retained_name: None,
    };
    let prepared = (|| {
        valid_name(name, false)?;
        let (dir, leaf) = parent(root, name, true)?.ok_or(ErrorCode::Io)?;
        let temp = format!(
            ".zipelo-{}-{}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        );
        let mut file = dir.create(&temp).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                ErrorCode::Conflict
            } else {
                ErrorCode::Io
            }
        })?;
        let written = (|| {
            crate::space::ensure(&file, length)?;
            let mut buffer = zeroize::Zeroizing::new([0; 65536]);
            let mut remaining = length;
            while remaining != 0 {
                check(false)?;
                let count = remaining.min(buffer.len() as u64) as usize;
                let n = data.read(&mut buffer[..count]).map_err(|_| ErrorCode::Io)?;
                if n == 0 {
                    return Err(ErrorCode::Io);
                }
                file.write_all(&buffer[..n]).map_err(|_| ErrorCode::Io)?;
                remaining -= n as u64;
            }
            file.sync_all().map_err(|_| ErrorCode::Io)
        })();
        let closed = close(file).map_err(|_| ErrorCode::Io);
        let mut finish = written.and(closed).and_then(|_| check(true));
        if finish.is_ok() {
            match dir.link(&temp, &leaf) {
                Ok(()) => {
                    result.created = true;
                    result.durability = dir.sync();
                    if result.durability == Durability::Failed {
                        finish = Err(ErrorCode::Io);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    finish = Err(ErrorCode::Conflict)
                }
                Err(_) => {
                    result.uncertain = true;
                    result.durability = Durability::Unconfirmed;
                    finish = Err(ErrorCode::Io);
                }
            }
        }
        if !dir.remove(&temp) {
            result.cleanup = Cleanup::Failed;
            result.retained = 1;
            result.retained_name = Some(match name.rsplit_once('/') {
                Some((prefix, _)) => format!("{prefix}/{temp}"),
                None => temp.clone(),
            });
            if finish.is_ok() {
                finish = Err(ErrorCode::Io);
            }
        }
        if result.created && dir.sync() == Durability::Failed {
            result.durability = Durability::Failed;
            finish = Err(ErrorCode::Io);
        }
        finish
    })();
    result.error = prepared.err();
    result
}
