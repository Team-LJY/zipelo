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
        fn getegid() -> u32;
        fn fgetxattr(
            fd: i32,
            name: *const std::ffi::c_char,
            value: *mut std::ffi::c_void,
            size: usize,
        ) -> isize;
        fn getgrgid_r(
            gid: u32,
            group: *mut Group,
            buffer: *mut i8,
            length: usize,
            result: *mut *mut Group,
        ) -> i32;
    }
    #[repr(C)]
    struct Group {
        name: *mut i8,
        password: *mut i8,
        gid: u32,
        members: *mut *mut i8,
    }
    // 없는 경로만 허용하며 중간 심볼릭 링크와 확인 오류는 거부함.
    fn path_is_absent(path: &Path) -> bool {
        let mut prefix = std::path::PathBuf::new();
        for component in path.components() {
            prefix.push(component.as_os_str());
            match std::fs::symlink_metadata(&prefix) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return false,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
                Err(_) => return false,
            }
        }
        false
    }

    fn sss_is_unused(socket: &Path, cache: &Path) -> bool {
        path_is_absent(socket)
            && ["passwd", "group", "initgr"]
                .iter()
                .all(|name| path_is_absent(&cache.join(name)))
    }

    // sss는 소켓과 캐시가 모두 없을 때만 빈 소스로 취급함.
    fn primary_group_is_private(gid: u32) -> bool {
        let Ok(policy) = std::fs::read_to_string("/etc/nsswitch.conf") else {
            return false;
        };
        let mut databases = [None, None, None];
        for line in policy.lines() {
            let line = line.split('#').next().unwrap_or_default();
            let Some((database, sources)) = line.split_once(':') else {
                continue;
            };
            let index = match database.trim() {
                "passwd" => 0,
                "group" => 1,
                "initgroups" => 2,
                _ => continue,
            };
            if databases[index]
                .replace(sources.split_whitespace().collect::<Vec<_>>())
                .is_some()
            {
                return false;
            }
        }
        let [Some(passwd), Some(group), initgroups] = databases else {
            return false;
        };
        let mut has_systemd = false;
        let mut has_sss = false;
        // initgroups가 생략되어도 기본값인 group 소스를 검사함.
        for source in [Some(passwd), Some(group), initgroups]
            .into_iter()
            .flatten()
        {
            if !matches!(
                source.as_slice(),
                ["files"] | ["files", "systemd"] | ["files", "sss"] | ["files", "systemd", "sss"]
            ) {
                return false;
            }
            has_systemd |= source.contains(&"systemd");
            has_sss |= source.contains(&"sss");
        }
        if has_sss
            && !sss_is_unused(
                Path::new("/var/lib/sss/pipes/nss"),
                Path::new("/var/lib/sss/mc"),
            )
        {
            return false;
        }
        let Ok(accounts) = std::fs::read_to_string("/etc/passwd") else {
            return false;
        };
        let current_uid = unsafe { geteuid() };
        let mut current_name = None;
        for account in accounts.lines() {
            if account.trim().is_empty() || account.starts_with('#') {
                continue;
            }
            let fields: Vec<_> = account.split(':').collect();
            if fields.len() != 7 || fields[0].is_empty() {
                return false;
            }
            let (Ok(uid), Ok(primary_gid)) = (fields[2].parse::<u32>(), fields[3].parse::<u32>())
            else {
                return false;
            };
            if primary_gid == gid {
                if uid != current_uid {
                    return false;
                }
                if current_name.replace(fields[0]).is_some() {
                    return false;
                }
            }
        }
        let Some(current_name) = current_name else {
            return false;
        };
        !has_systemd
            || crate::systemd_userdb::group_is_private(
                gid,
                current_uid,
                current_name,
                Path::new("/run/systemd/userdb/"),
                &[
                    Path::new("/etc/userdb"),
                    Path::new("/run/userdb"),
                    Path::new("/run/host/userdb"),
                    Path::new("/usr/local/lib/userdb"),
                    Path::new("/usr/lib/userdb"),
                ],
            )
    }

    // 기본 구성원과 보조 구성원을 모두 확인함.
    fn private_group(gid: u32) -> bool {
        if gid != unsafe { getegid() } || !primary_group_is_private(gid) {
            return false;
        }
        let mut group = Group {
            name: std::ptr::null_mut(),
            password: std::ptr::null_mut(),
            gid: 0,
            members: std::ptr::null_mut(),
        };
        let mut result = std::ptr::null_mut();
        let mut buffer = vec![0i8; 16 * 1024];
        let status = unsafe {
            getgrgid_r(
                gid,
                &mut group,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            )
        };
        // 그룹 정보를 읽지 못하면 안전하다고 볼 근거가 없으므로 거부함.
        status == 0
            && !result.is_null()
            && group.gid == gid
            && !group.members.is_null()
            && unsafe { *group.members }.is_null()
    }
    fn access_acl_has_no_named_entries(file: &File) -> bool {
        // 이름 없는 ACL은 소유자, 그룹, other와 선택적 mask의 최대 4항목임.
        let mut bytes = [0u8; 4 + 4 * 8];
        // 경로를 다시 해석하지 않고 판정 대상 폴더의 접근 ACL을 읽음.
        let length = unsafe {
            fgetxattr(
                file.as_raw_fd(),
                c"system.posix_acl_access".as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        if length < 0 {
            // 속성 없음과 미지원 외에는 ERANGE를 포함해 거부함.
            return matches!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(61 | 95)
            );
        }
        let tags: &[u16] = match length {
            28 => &[1, 4, 32],
            36 => &[1, 4, 16, 32],
            _ => return false,
        };
        if u32::from_le_bytes(bytes[..4].try_into().unwrap()) != 2 {
            return false;
        }
        bytes[4..length as usize]
            .chunks_exact(8)
            .zip(tags)
            .all(|(entry, expected)| {
                u16::from_le_bytes(entry[..2].try_into().unwrap()) == *expected
                    && u16::from_le_bytes(entry[2..4].try_into().unwrap()) & !7 == 0
                    && u32::from_le_bytes(entry[4..8].try_into().unwrap()) == u32::MAX
            })
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
            // 그룹 쓰기는 이름 있는 접근 ACL이 없고 개인 그룹일 때만 허용함.
            let group_writable = m.mode() & 0o020 != 0;
            if !m.is_dir()
                || m.uid() != unsafe { geteuid() }
                || m.mode() & 0o002 != 0
                || (group_writable
                    && (!access_acl_has_no_named_entries(&self.file) || !private_group(m.gid())))
            {
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
        /// 이 폴더 안에 이름 없는 읽기/쓰기 임시 파일을 만듦. 마지막 핸들을 닫으면 제거됨.
        pub(crate) fn temporary(&self) -> Result<File> {
            // O_TMPFILE | O_EXCL: 이름이 생기지 않고 나중에 링크할 수도 없음.
            match open(&self.file, std::ffi::OsStr::new("."), 2 | 0x410000 | 0x80) {
                Ok(file) => return Ok(file),
                Err(e) => {
                    if crate::space::io_error(e) == ErrorCode::DiskFull {
                        return Err(ErrorCode::DiskFull);
                    }
                }
            }
            // O_TMPFILE을 지원하지 않는 파일 시스템은 숨김 이름으로 만든 즉시 이름을 지움.
            let n = format!(".zipelo-{}.tmp", crate::spool::random_name()?);
            let file = open(&self.file, std::ffi::OsStr::new(&n), 2 | 0x40 | 0x80)
                .map_err(crate::space::io_error)?;
            if !self.remove(&n) {
                // 핸들을 닫고 한 번 더 지움. 그래도 남으면 정리 실패로 기록해 결과에 알림.
                drop(file);
                if !self.remove(&n) && !self.absent(&n).unwrap_or(false) {
                    crate::spool::note_leftover();
                }
                return Err(ErrorCode::Io);
            }
            if file.metadata().map_err(|_| ErrorCode::Io)?.nlink() != 0 {
                return Err(ErrorCode::Io);
            }
            Ok(file)
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
        /// 이 폴더 안에 닫으면 지워지는 숨김 임시 파일을 만듦.
        pub(crate) fn temporary(&self) -> Result<File> {
            crate::spool::windows_temporary(&self.path, ".zipelo-", true)
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
        PublishStep::Check | PublishStep::Commit(_) => check.check(),
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
    check(PublishStep::Commit(None))?;
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
        PublishStep::Commit(_) => check(true),
        PublishStep::Activity => Ok(()),
    })
}
pub(crate) struct PublicationTemp {
    pub parent: SourceObject,
    pub name: String,
    pub object: SourceObject,
    /// 분할 볼륨처럼 한 번에 게시하는 나머지 임시 파일 (이름, 객체).
    pub others: Vec<(String, SourceObject)>,
}
pub(crate) enum PublishStep {
    Check,
    Activity,
    Commit(Option<PublicationTemp>),
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
        let mut temporary = None;
        let written = (|| {
            temporary = Some(PublicationTemp {
                parent: object(dir.handle())?,
                name: temp.clone(),
                object: object(&file)?,
                others: Vec::new(),
            });
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
        let identity = temporary.as_ref().map(|temp| temp.object.clone());
        let closed = completed(
            close(file).map_err(crate::space::io_error),
            &mut PublicationMonitor(check),
        );
        let mut finish = written.and(closed).and_then(|_| {
            check(PublishStep::Commit(temporary))?;
            // 생성한 임시 파일과 이름이 가리키는 파일이 같은지 확인함.
            if dir.existing(&temp)?.as_ref().map(|file| &file.object) != identity.as_ref() {
                return Err(ErrorCode::Conflict);
            }
            // 긴 원본 재검증 중 바뀐 대상은 교체 직전에 거부함.
            if let Some(expected) = replace {
                if completed(dir.existing(&leaf), &mut PublicationMonitor(check))?.as_ref()
                    != Some(expected)
                {
                    return Err(ErrorCode::Conflict);
                }
            }
            Ok(())
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
            match dir.existing(&temp) {
                Ok(None) => result.cleanup = Cleanup::Unconfirmed,
                Ok(Some(current)) if Some(&current.object) == identity.as_ref() => {
                    if dir.remove(&temp) {
                        publication_activity(&mut finish, check);
                    } else {
                        result.cleanup = Cleanup::Failed;
                        result.retained = 1;
                        result.retained_name = Some(match name.rsplit_once('/') {
                            Some((prefix, _)) => format!("{prefix}/{temp}"),
                            None => temp.clone(),
                        });
                    }
                }
                _ => {
                    // 다른 파일은 보존하고 소유 임시 파일의 위치를 단정하지 않음.
                    result.cleanup = Cleanup::Unconfirmed;
                }
            }
            if result.cleanup != Cleanup::Complete && finish.is_ok() {
                finish = Err(ErrorCode::Io);
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
        if result.created {
            // 이름 기반 게시 뒤에도 결과가 생성한 파일인지 확인함.
            let verified = match dir.existing(&leaf) {
                Ok(Some(current)) if Some(&current.object) == identity.as_ref() => Ok(()),
                Ok(_) => Err(ErrorCode::Conflict),
                Err(error) => Err(error),
            };
            if let Err(error) = verified {
                result.created = false;
                result.uncertain = true;
                result.durability = Durability::Unconfirmed;
                if result.cleanup == Cleanup::Complete {
                    result.cleanup = Cleanup::Unconfirmed;
                }
                finish = Err(error);
            }
        }
        finish
    })();
    result.error = prepared.err();
    result
}

/// 여러 파일을 한 묶음으로 게시한 결과. `created`는 게시된 채 남은 이름.
pub(crate) struct PublishedSet {
    pub created: Vec<String>,
    pub uncertain: bool,
    pub error: Option<ErrorCode>,
    pub durability: Durability,
    pub cleanup: Cleanup,
    pub retained: Vec<String>,
}
struct Staged {
    temp: String,
    object: Option<SourceObject>,
}
/// 분할 볼륨을 같은 폴더에 모두 임시 파일로 쓴 뒤 한 번의 확정 단계로 게시함.
/// 쓰기 중 실패나 취소는 임시 파일만 지우고, 게시 중 실패는 이미 게시한 볼륨을 되돌림.
/// 이름은 `root` 바로 아래 파일 이름이어야 하고 기존 이름은 덮어쓰지 않음.
pub(crate) fn publish_set_steps(
    root: &Dir,
    files: &[(String, u64)],
    data: &mut impl Read,
    check: &mut impl FnMut(PublishStep) -> Result<()>,
) -> PublishedSet {
    let mut result = PublishedSet {
        created: Vec::new(),
        uncertain: false,
        error: None,
        durability: Durability::NotApplicable,
        cleanup: Cleanup::Complete,
        retained: Vec::new(),
    };
    let mut staged: Vec<Staged> = Vec::new();
    let mut linked = 0usize;
    let mut finish = (|| {
        if files.is_empty() {
            return Err(ErrorCode::InvalidRequest);
        }
        for (name, _) in files {
            valid_name(name, false)?;
            if name.contains('/') {
                return Err(ErrorCode::InvalidRequest);
            }
        }
        let parent = object(root.handle())?;
        let mut buffer = zeroize::Zeroizing::new([0; 65536]);
        for (_, length) in files {
            check(PublishStep::Check)?;
            let temp = format!(
                ".zipelo-{}-{}.tmp",
                std::process::id(),
                TEMP.fetch_add(1, Ordering::Relaxed)
            );
            let mut file = root.create(&temp).map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    ErrorCode::Conflict
                } else {
                    crate::space::io_error(e)
                }
            })?;
            staged.push(Staged { temp, object: None });
            let written = (|| {
                staged.last_mut().ok_or(ErrorCode::Internal)?.object = Some(object(&file)?);
                completed(
                    crate::space::ensure(&file, *length),
                    &mut PublicationMonitor(check),
                )?;
                let mut remaining = *length;
                while remaining != 0 {
                    check(PublishStep::Check)?;
                    let count = remaining.min(buffer.len() as u64) as usize;
                    let n = data.read(&mut buffer[..count]).map_err(|_| ErrorCode::Io)?;
                    if n == 0 {
                        return Err(ErrorCode::Io);
                    }
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
            written.and(closed)?;
        }
        let mut owned = staged
            .iter()
            .map(|s| Ok((s.temp.clone(), s.object.clone().ok_or(ErrorCode::Internal)?)))
            .collect::<Result<Vec<_>>>()?;
        let (name, first) = owned.remove(0);
        check(PublishStep::Commit(Some(PublicationTemp {
            parent,
            name,
            object: first,
            others: owned,
        })))?;
        // 생성한 임시 파일과 이름이 가리키는 파일이 같은지 모두 확인함.
        for s in &staged {
            if root.existing(&s.temp)?.as_ref().map(|file| &file.object) != s.object.as_ref() {
                return Err(ErrorCode::Conflict);
            }
        }
        for ((name, _), s) in files.iter().zip(&staged) {
            match root.link(&s.temp, name) {
                Ok(()) => linked += 1,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Err(ErrorCode::Conflict);
                }
                Err(error) => {
                    // 실패한 이름도 되돌리기 대상으로 보고 아래에서 소유 여부를 확인함.
                    linked += 1;
                    return Err(crate::space::io_error(error));
                }
            }
        }
        Ok(())
    })();
    if finish.is_err() {
        // 게시한 볼륨 중 이 작업이 만든 것만 지움. 다른 파일은 보존함.
        for ((name, _), s) in files.iter().zip(&staged).take(linked) {
            match root.existing(name) {
                Ok(Some(current)) if Some(&current.object) == s.object.as_ref() => {
                    if !root.remove(name) {
                        result.created.push(name.clone());
                    }
                }
                Ok(_) => {}
                Err(_) => result.uncertain = true,
            }
        }
    } else {
        result.created = files.iter().map(|(name, _)| name.clone()).collect();
        publication_activity(&mut finish, check);
    }
    for s in &staged {
        match root.existing(&s.temp) {
            Ok(None) => result.cleanup = Cleanup::Unconfirmed,
            Ok(Some(current)) if s.object.as_ref() == Some(&current.object) => {
                if root.remove(&s.temp) {
                    publication_activity(&mut finish, check);
                } else {
                    result.cleanup = Cleanup::Failed;
                    result.retained.push(s.temp.clone());
                }
            }
            // 이름만 만들고 객체를 확인하지 못한 임시 파일이나 다른 파일은 보존함.
            _ => {
                if s.object.is_none() && root.remove(&s.temp) {
                    continue;
                }
                result.cleanup = Cleanup::Unconfirmed;
            }
        }
    }
    if result.cleanup != Cleanup::Complete && finish.is_ok() {
        finish = Err(ErrorCode::Io);
    }
    if !result.created.is_empty() || linked != 0 {
        match root.sync_checked() {
            Ok(durability) => {
                result.durability = durability;
                publication_activity(&mut finish, check);
            }
            Err(error) => {
                result.durability = Durability::Failed;
                if finish.is_ok() {
                    finish = Err(error);
                }
            }
        }
    }
    if finish.is_ok() {
        // 이름 기반 게시 뒤에도 결과가 생성한 파일인지 확인함.
        for ((name, _), s) in files.iter().zip(&staged) {
            let verified = match root.existing(name) {
                Ok(Some(current)) if s.object.as_ref() == Some(&current.object) => Ok(()),
                Ok(_) => Err(ErrorCode::Conflict),
                Err(error) => Err(error),
            };
            if let Err(error) = verified {
                result.created.retain(|created| created != name);
                result.uncertain = true;
                result.durability = Durability::Unconfirmed;
                if finish.is_ok() {
                    finish = Err(error);
                }
            }
        }
    }
    result.error = finish.err();
    result
}
