use crate::archive::valid_name;
use crate::protocol::*;
use crate::resources::{Monitor, completed};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
#[cfg(windows)]
use std::path::PathBuf;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

type Result<T> = std::result::Result<T, ErrorCode>;
static TEMP: AtomicU64 = AtomicU64::new(0);
#[derive(Debug, PartialEq, Eq)]
pub struct Existing {
    parent: SourceObject,
    object: SourceObject,
    pub size: u64,
    modified: std::time::SystemTime,
}
fn existing_file(parent: &File, file: File) -> Result<Existing> {
    let metadata = file.metadata().map_err(|_| ErrorCode::Io)?;
    if !metadata.is_file() {
        return Err(ErrorCode::UnsafePath);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(ErrorCode::UnsafePath);
        }
    }
    Ok(Existing {
        parent: object(parent)?,
        object: object(&file)?,
        size: metadata.len(),
        modified: metadata.modified().map_err(|_| ErrorCode::Io)?,
    })
}
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
    check: &mut impl crate::resources::Monitor,
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
        check.check()?;
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
        completed(Ok(()), check)?;
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
        fn renameat(fd: i32, path: *const i8, newfd: i32, newpath: *const i8) -> i32;
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
                        return Err(crate::space::io_error(std::io::Error::last_os_error()));
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
        pub fn existing(&self, n: &str) -> Result<Option<Existing>> {
            match open(&self.file, std::ffi::OsStr::new(n), 0x200000) {
                Ok(file) => existing_file(&self.file, file).map(Some),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(ErrorCode::Io),
            }
        }
        pub fn replace(&self, from: &str, to: &str) -> std::io::Result<()> {
            let from = CString::new(from).unwrap();
            let to = CString::new(to).unwrap();
            // 고정 부모 안에서 완성된 임시 파일로 원본 이름을 교체함.
            if unsafe {
                renameat(
                    self.file.as_raw_fd(),
                    from.as_ptr(),
                    self.file.as_raw_fd(),
                    to.as_ptr(),
                )
            } == 0
            {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
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
            self.sync_checked().unwrap_or(Durability::Failed)
        }
        pub(crate) fn sync_checked(&self) -> Result<Durability> {
            self.file.sync_all().map_err(crate::space::io_error)?;
            Ok(Durability::Synced)
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
                    std::fs::create_dir(&p).map_err(crate::space::io_error)?;
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
        pub fn existing(&self, n: &str) -> Result<Option<Existing>> {
            match OpenOptions::new()
                .access_mode(0x80)
                .share_mode(7)
                .custom_flags(0x02000000 | 0x00200000)
                .open(self.path.join(n))
            {
                Ok(file) => existing_file(self.handle(), file).map(Some),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(ErrorCode::Io),
            }
        }
        pub fn replace(&self, from: &str, to: &str) -> std::io::Result<()> {
            use std::os::windows::ffi::OsStrExt;
            unsafe extern "system" {
                fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
            }
            let extended = |name: &str| {
                [92u16, 92, 63, 92]
                    .into_iter()
                    .chain(
                        self.path
                            .join(name)
                            .as_os_str()
                            .encode_wide()
                            .map(|unit| if unit == 47 { 92 } else { unit }),
                    )
                    .chain(Some(0))
                    .collect::<Vec<_>>()
            };
            let from = extended(from);
            let to = extended(to);
            // 같은 부모의 이름만 교체하고 복사나 사전 삭제는 하지 않음.
            if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1) } != 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
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
        pub(crate) fn sync_checked(&self) -> Result<Durability> {
            Ok(Durability::Unconfirmed)
        }
    }
}
#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("Archive worker supports Linux and Windows only");
pub use platform::Dir;
fn parent_with(
    root: &Dir,
    name: &str,
    create: bool,
    check: &mut impl Monitor,
) -> Result<Option<(Dir, String)>> {
    let mut parts = name.split('/').collect::<Vec<_>>();
    let leaf = parts.pop().ok_or(ErrorCode::UnsafePath)?;
    let mut d = root.duplicate()?;
    for p in parts {
        match completed(d.child(p, create), check)? {
            Some(child) => d = child,
            None => return Ok(None),
        }
    }
    Ok(Some((d, leaf.to_owned())))
}
pub fn existing(root: &Dir, name: &str) -> Result<Option<Existing>> {
    existing_with(root, name, &mut || Ok(()))
}
pub(crate) fn existing_with(
    root: &Dir,
    name: &str,
    check: &mut impl Monitor,
) -> Result<Option<Existing>> {
    valid_name(name, false)?;
    let result = match parent_with(root, name, false, check)? {
        Some((dir, leaf)) => dir.existing(&leaf),
        None => Ok(None),
    };
    completed(result, check)
}
pub fn unused(root: &Dir, name: &str) -> Result<bool> {
    unused_with(root, name, &mut || Ok(()))
}
pub(crate) fn unused_with(root: &Dir, name: &str, check: &mut impl Monitor) -> Result<bool> {
    valid_name(name, false)?;
    let result = match parent_with(root, name, false, check)? {
        Some((dir, leaf)) => dir.absent(&leaf),
        None => Ok(true),
    };
    completed(result, check)
}
pub fn preflight(root: &Dir, names: &[(String, EntryKind)]) -> Result<()> {
    preflight_mode(root, names, false)
}
pub fn preflight_mode(root: &Dir, names: &[(String, EntryKind)], interactive: bool) -> Result<()> {
    preflight_mode_with(root, names, interactive, &mut || Ok(()))
}
pub(crate) fn preflight_mode_with(
    root: &Dir,
    names: &[(String, EntryKind)],
    interactive: bool,
    check: &mut impl Monitor,
) -> Result<()> {
    for (name, kind) in names {
        check.check()?;
        if let Some((d, leaf)) = parent_with(root, name, false, check)? {
            if *kind == EntryKind::Directory {
                let _ = d.child(&leaf, false)?;
            } else if interactive {
                d.existing(&leaf)?;
            } else if !d.absent(&leaf)? {
                return Err(ErrorCode::Conflict);
            }
        }
        completed(Ok(()), check)?;
    }
    Ok(())
}
pub(crate) fn preflight_space_with(
    root: &Dir,
    files: &[(String, u64)],
    staging: Option<(&File, u64)>,
    check: &mut impl Monitor,
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
        completed(charge(file, bytes), check)?;
    }
    for (name, size) in files {
        valid_name(name, false)?;
        let mut parent = root.duplicate()?;
        let mut parts = name.split('/').collect::<Vec<_>>();
        parts.pop();
        for part in parts {
            match completed(parent.child(part, false), check)? {
                Some(child) => parent = child,
                None => break,
            }
        }
        completed(charge(parent.handle(), *size), check)?;
    }
    for (file, bytes) in volumes.values() {
        completed(crate::space::ensure(file, *bytes), check)?;
    }
    Ok(())
}
pub fn directory(root: &Dir, name: &str) -> Result<(bool, Durability, Option<ErrorCode>)> {
    directory_with(root, name, &mut || Ok(()))
}
pub(crate) fn directory_with(
    root: &Dir,
    name: &str,
    check: &mut impl Monitor,
) -> Result<(bool, Durability, Option<ErrorCode>)> {
    directory_steps(root, name, &mut |step| match step {
        PublishStep::Check | PublishStep::Commit => check.check(),
        PublishStep::Activity => check.activity(),
    })
}
pub(crate) fn directory_steps(
    root: &Dir,
    name: &str,
    check: &mut impl FnMut(PublishStep) -> Result<()>,
) -> Result<(bool, Durability, Option<ErrorCode>)> {
    let (d, leaf) =
        parent_with(root, name, false, &mut PublicationMonitor(check))?.ok_or(ErrorCode::Io)?;
    if completed(d.child(&leaf, false), &mut PublicationMonitor(check))?.is_some() {
        return Ok((false, Durability::NotApplicable, None));
    }
    check(PublishStep::Commit)?;
    d.child(&leaf, true)?.ok_or(ErrorCode::Io)?;
    let progress_error = completed(Ok(()), &mut PublicationMonitor(check)).err();
    Ok(match d.sync_checked() {
        Ok(durability) => (
            true,
            durability,
            progress_error.or_else(|| completed(Ok(()), &mut PublicationMonitor(check)).err()),
        ),
        Err(error) => (true, Durability::Failed, Some(error)),
    })
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
    publish_reader_mode(root, name, data, length, None, check)
}
pub fn publish_reader_mode(
    root: &Dir,
    name: &str,
    data: &mut impl Read,
    length: u64,
    replace: Option<&Existing>,
    check: &mut impl FnMut(bool) -> Result<()>,
) -> Published {
    publish_reader_steps(root, name, data, length, replace, &mut |step| match step {
        PublishStep::Check => check(false),
        PublishStep::Commit => check(true),
        PublishStep::Activity => Ok(()),
    })
}
pub(crate) enum PublishStep {
    Check,
    Activity,
    Commit,
}
struct PublicationMonitor<'a, F>(&'a mut F);
impl<F: FnMut(PublishStep) -> Result<()>> Monitor for PublicationMonitor<'_, F> {
    fn check(&mut self) -> Result<()> {
        self.0(PublishStep::Check)
    }
    fn activity(&mut self) -> Result<()> {
        self.0(PublishStep::Activity)
    }
}
fn publication_activity(
    finish: &mut Result<()>,
    check: &mut impl FnMut(PublishStep) -> Result<()>,
) {
    let progress = completed(Ok(()), &mut PublicationMonitor(check));
    if finish.is_ok() {
        *finish = progress;
    }
}
pub(crate) fn publish_reader_steps(
    root: &Dir,
    name: &str,
    data: &mut impl Read,
    length: u64,
    replace: Option<&Existing>,
    check: &mut impl FnMut(PublishStep) -> Result<()>,
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
        let (dir, leaf) =
            parent_with(root, name, true, &mut PublicationMonitor(check))?.ok_or(ErrorCode::Io)?;
        let temp = format!(
            ".zipelo-{}-{}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        );
        let mut file = dir.create(&temp).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                ErrorCode::Conflict
            } else {
                crate::space::io_error(e)
            }
        })?;
        let written = (|| {
            completed(Ok(()), &mut PublicationMonitor(check))?;
            completed(
                crate::space::ensure(&file, length),
                &mut PublicationMonitor(check),
            )?;
            let mut buffer = zeroize::Zeroizing::new([0; 65536]);
            let mut remaining = length;
            while remaining != 0 {
                check(PublishStep::Check)?;
                let count = remaining.min(buffer.len() as u64) as usize;
                let n = data.read(&mut buffer[..count]).map_err(|_| ErrorCode::Io)?;
                if n == 0 {
                    return Err(ErrorCode::Io);
                }
                completed(Ok(()), &mut PublicationMonitor(check))?;
                completed(
                    crate::space::ensure(&file, n as u64),
                    &mut PublicationMonitor(check),
                )?;
                completed(
                    file.write_all(&buffer[..n]).map_err(crate::space::io_error),
                    &mut PublicationMonitor(check),
                )?;
                remaining -= n as u64;
            }
            completed(
                file.sync_all().map_err(crate::space::io_error),
                &mut PublicationMonitor(check),
            )
        })();
        let closed = completed(
            close(file).map_err(crate::space::io_error),
            &mut PublicationMonitor(check),
        );
        let mut finish = written.and(closed).and_then(|_| {
            if let Some(expected) = replace {
                if completed(dir.existing(&leaf), &mut PublicationMonitor(check))?.as_ref()
                    != Some(expected)
                {
                    return Err(ErrorCode::Conflict);
                }
            }
            check(PublishStep::Commit)
        });
        let mut temp_consumed = false;
        if finish.is_ok() {
            let published = if replace.is_some() {
                dir.replace(&temp, &leaf)
            } else {
                dir.link(&temp, &leaf)
            };
            match published {
                Ok(()) => {
                    temp_consumed = replace.is_some();
                    result.created = true;
                    publication_activity(&mut finish, check);
                    match dir.sync_checked() {
                        Ok(durability) => {
                            result.durability = durability;
                            publication_activity(&mut finish, check);
                        }
                        Err(error) => {
                            result.durability = Durability::Failed;
                            finish = Err(error);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    finish = Err(ErrorCode::Conflict)
                }
                Err(error) => {
                    result.uncertain = true;
                    result.durability = Durability::Unconfirmed;
                    finish = Err(crate::space::io_error(error));
                }
            }
        }
        if !temp_consumed {
            if dir.remove(&temp) {
                publication_activity(&mut finish, check);
            } else {
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
        }
        if result.created {
            if let Err(error) = dir.sync_checked() {
                result.durability = Durability::Failed;
                finish = Err(error);
            } else {
                publication_activity(&mut finish, check);
            }
        }
        finish
    })();
    result.error = prepared.err();
    result
}
