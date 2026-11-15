use crate::{filesystem, protocol::*};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

type Result<T> = std::result::Result<T, ErrorCode>;

enum Placement {
    Fixed(filesystem::Dir),
    Fallback {
        source_parent: std::path::PathBuf,
        next: usize,
        other_error: Option<ErrorCode>,
    },
}

thread_local! {
    // 작업 스레드의 임시 파일 정책만 보관함.
    static LOCATION: std::cell::RefCell<Option<Placement>> =
        const { std::cell::RefCell::new(None) };
    // 출력 폴더에 이름 있는 임시 파일을 만든 뒤 이름을 지우지 못했는지 기록함.
    static LEFTOVER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) struct Location(Option<Placement>);
impl Drop for Location {
    fn drop(&mut self) {
        LOCATION.with(|location| *location.borrow_mut() = self.0.take());
    }
}
pub(crate) fn place_in(dir: &filesystem::Dir) -> Result<Location> {
    let placement = Placement::Fixed(dir.duplicate()?);
    LEFTOVER.with(|leftover| leftover.set(false));
    Ok(LOCATION.with(|location| Location(location.replace(Some(placement)))))
}
pub(crate) fn fallback_for(path: &Path) -> Location {
    LEFTOVER.with(|leftover| leftover.set(false));
    let placement = Placement::Fallback {
        source_parent: path.parent().unwrap_or(Path::new("")).to_path_buf(),
        next: 0,
        other_error: None,
    };
    LOCATION.with(|location| Location(location.replace(Some(placement))))
}

#[cfg(target_os = "linux")]
fn private_directory(path: &Path) -> Result<filesystem::Dir> {
    if !path.is_absolute() {
        return Err(ErrorCode::Io);
    }
    match filesystem::Dir::root(path) {
        Ok(dir) => Ok(dir),
        Err(error) => {
            if !matches!(std::fs::symlink_metadata(path), Err(ref e) if e.kind() == std::io::ErrorKind::NotFound)
            {
                return Err(error);
            }
            let parent = private_directory(path.parent().ok_or(ErrorCode::Io)?)?;
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(ErrorCode::Io)?;
            parent.child(name, true)?.ok_or(ErrorCode::Io)
        }
    }
}
fn cache_directory() -> Result<filesystem::Dir> {
    #[cfg(target_os = "linux")]
    {
        let base = match std::env::var_os("XDG_CACHE_HOME").filter(|p| !p.is_empty()) {
            Some(path) => std::path::PathBuf::from(path),
            None => std::path::PathBuf::from(std::env::var_os("HOME").ok_or(ErrorCode::Io)?)
                .join(".cache"),
        };
        private_directory(&base)?
            .child("zipelo", true)?
            .ok_or(ErrorCode::Io)
    }
    #[cfg(windows)]
    {
        let base = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(ErrorCode::Io)?);
        if !base.is_absolute() {
            return Err(ErrorCode::Io);
        }
        filesystem::Dir::root(&base)?
            .child("Zipelo", true)?
            .ok_or(ErrorCode::Io)?
            .child("Temp", true)?
            .ok_or(ErrorCode::Io)
    }
}
pub(crate) fn next_after_full() -> Result<bool> {
    LOCATION.with(|location| {
        let mut location = location.borrow_mut();
        match location.as_mut() {
            Some(Placement::Fallback {
                next, other_error, ..
            }) => {
                *next += 1;
                if *next >= 3 {
                    Err(other_error.unwrap_or(ErrorCode::DiskFull))
                } else {
                    Ok(true)
                }
            }
            _ => Ok(false),
        }
    })
}

/// 출력 폴더에 지우지 못한 임시 파일이 남았음을 기록함. Windows 임시 파일은 닫으면 지워지므로
/// Linux 대체 이름 경로에서만 사용함.
#[cfg(target_os = "linux")]
pub(crate) fn note_leftover() {
    LEFTOVER.with(|leftover| leftover.set(true));
}

/// 작업 중 출력 폴더에 임시 파일이 남았는지 확인하고 기록을 비움.
pub(crate) fn take_leftover() -> bool {
    LEFTOVER.with(|leftover| leftover.replace(false))
}

/// 실패한 단계의 임시 파일이 닫힌 뒤 다음 위치에서 다시 시도함.
pub(crate) fn retry_full<T>(mut operation: impl FnMut() -> Result<T>) -> Result<T> {
    loop {
        match operation() {
            Err(ErrorCode::DiskFull) if next_after_full()? => continue,
            result => return result,
        }
    }
}

pub(crate) fn random_name() -> Result<String> {
    let mut random = [0; 16];
    crate::crypto::random(&mut random)?;
    Ok(random.iter().map(|b| format!("{b:02x}")).collect())
}

/// 지정한 정책에 따라 이름 없는 임시 파일을 만듦.
pub(crate) fn temporary() -> Result<File> {
    let placed = LOCATION.with(|location| {
        let mut location = location.borrow_mut();
        match location.as_mut() {
            Some(Placement::Fixed(dir)) => match dir.temporary() {
                Ok(file) => Some(Ok(file)),
                Err(error) => {
                    *location = None;
                    Some(system_temporary().map_err(|fallback| {
                        if error == ErrorCode::DiskFull {
                            ErrorCode::DiskFull
                        } else {
                            fallback
                        }
                    }))
                }
            },
            Some(Placement::Fallback {
                source_parent,
                next,
                other_error,
            }) => {
                while *next < 3 {
                    let file = match *next {
                        0 => cache_directory().and_then(|dir| dir.temporary()),
                        1 => filesystem::Dir::root(source_parent).and_then(|dir| dir.temporary()),
                        _ => system_temporary(),
                    };
                    match file {
                        Ok(file) => return Some(Ok(file)),
                        Err(error) => {
                            if error != ErrorCode::DiskFull {
                                *other_error = Some(ErrorCode::Io);
                            }
                            *next += 1;
                        }
                    }
                }
                Some(Err(other_error.unwrap_or(ErrorCode::DiskFull)))
            }
            None => None,
        }
    });
    placed.unwrap_or_else(system_temporary)
}

fn system_temporary() -> Result<File> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // 이름 없는 임시 파일은 마지막 핸들을 닫으면 제거됨.
        match OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(0x410000 | 0x80)
            .mode(0o600)
            .open(std::env::temp_dir())
        {
            Ok(file) => return Ok(file),
            Err(error) => {
                if crate::space::io_error(error) == ErrorCode::DiskFull {
                    return Err(ErrorCode::DiskFull);
                }
            }
        }
        let name = random_name()?;
        let path = std::env::temp_dir().join(format!("zipelo-{name}.tmp"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(0x20000)
            .open(&path)
            .map_err(crate::space::io_error)?;
        std::fs::remove_file(path).map_err(|_| ErrorCode::Io)?;
        use std::os::unix::fs::MetadataExt;
        if file.metadata().map_err(|_| ErrorCode::Io)?.nlink() != 0 {
            return Err(ErrorCode::Io);
        }
        Ok(file)
    }
    #[cfg(windows)]
    {
        windows_temporary(&std::env::temp_dir(), "zipelo-", false)
    }
}

/// 닫으면 지워지는 Windows 임시 파일을 지정한 폴더에 만듦. 출력 폴더에서는 숨김 속성을 붙임.
#[cfg(windows)]
pub(crate) fn windows_temporary(parent: &Path, prefix: &str, hidden: bool) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    let name = random_name()?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(0)
        .access_mode(0xc0010000)
        .custom_flags(0x04000000 | 0x00200000)
        .attributes(if hidden { 0x100 | 0x2 } else { 0x100 })
        .open(parent.join(format!("{prefix}{name}.tmp")))
        .map_err(crate::space::io_error)
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    Check,
    Activity,
    Read { bytes: u64, total: u64 },
}

struct Progress<'a, F>(&'a mut F);
impl<F: FnMut(Step) -> Result<()>> crate::resources::Monitor for Progress<'_, F> {
    fn check(&mut self) -> Result<()> {
        self.0(Step::Check)
    }
    fn activity(&mut self) -> Result<()> {
        self.0(Step::Activity)
    }
}
fn completed<T>(result: Result<T>, step: &mut impl FnMut(Step) -> Result<()>) -> Result<T> {
    crate::resources::completed(result, &mut Progress(step))
}

pub(crate) struct Snapshot {
    pub file: File,
    pub len: u64,
    pub object: SourceObject,
    pub hash: [u8; 32],
}
impl Snapshot {
    /// 원본을 임시 파일로 복사하는 동안 읽은 바이트와 전체 크기를 함께 알림.
    pub fn open_reporting(
        path: &Path,
        limit: u64,
        step: &mut impl FnMut(Step) -> Result<()>,
    ) -> Result<Self> {
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
        let mut source = completed(options.open(path).map_err(|_| ErrorCode::Io), step)?;
        let snapshot = Self::capture_reporting(&mut source, limit, step)?;
        let named = completed(
            options.open(path).map_err(|_| ErrorCode::SourceChanged),
            step,
        )?;
        if filesystem::object(&named)? != snapshot.object
            || !named.metadata().map_err(|_| ErrorCode::Io)?.is_file()
        {
            return Err(ErrorCode::SourceChanged);
        }
        completed(Ok(snapshot), step)
    }
    pub(crate) fn capture(
        source: &mut File,
        limit: u64,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<Self> {
        Self::capture_reporting(source, limit, &mut |step| match step {
            Step::Check => check.check(),
            Step::Activity => crate::resources::completed(Ok(()), check),
            Step::Read { .. } => check.check(),
        })
    }
    pub(crate) fn capture_reporting(
        source: &mut File,
        limit: u64,
        step: &mut impl FnMut(Step) -> Result<()>,
    ) -> Result<Self> {
        let original = source.metadata().map_err(|_| ErrorCode::Io)?;
        loop {
            step(Step::Check)?;
            let current = source.metadata().map_err(|_| ErrorCode::Io)?;
            if current.len() != original.len()
                || current.modified().ok() != original.modified().ok()
            {
                return Err(ErrorCode::SourceChanged);
            }
            match Self::capture_once(source, limit, step) {
                Err(ErrorCode::DiskFull) if next_after_full()? => continue,
                result => return result,
            }
        }
    }
    fn capture_once(
        source: &mut File,
        limit: u64,
        step: &mut impl FnMut(Step) -> Result<()>,
    ) -> Result<Self> {
        let before = completed(source.metadata().map_err(|_| ErrorCode::Io), step)?;
        if !before.is_file() {
            return Err(ErrorCode::InvalidRequest);
        }
        if before.len() > limit {
            return Err(ErrorCode::Limit);
        }
        source.rewind().map_err(|_| ErrorCode::Io)?;
        let object = filesystem::object(&source)?;
        let mut file = completed(temporary(), step)?;
        completed(crate::space::ensure(&file, before.len()), step)?;
        let mut buffer = zeroize::Zeroizing::new([0; 65536]);
        let mut hash = Sha256::new();
        let mut len = 0u64;
        let total = before.len();
        let mut reported = std::time::Instant::now();
        loop {
            step(Step::Check)?;
            let n = completed(
                source.read(&mut buffer[..]).map_err(|_| ErrorCode::Io),
                step,
            )?;
            if n == 0 {
                break;
            }
            len = len
                .checked_add(n as u64)
                .filter(|n| *n <= limit)
                .ok_or(ErrorCode::Limit)?;
            completed(
                file.write_all(&buffer[..n]).map_err(crate::space::io_error),
                step,
            )?;
            hash.update(&buffer[..n]);
            // 큰 파일에서도 진행이 보이도록 일정 간격으로만 알림.
            if reported.elapsed() >= std::time::Duration::from_millis(200) {
                step(Step::Read { bytes: len, total })?;
                reported = std::time::Instant::now();
            }
        }
        step(Step::Read { bytes: len, total })?;
        let after = source.metadata().map_err(|_| ErrorCode::Io)?;
        if filesystem::object(&source)? != object
            || before.len() != len
            || after.len() != len
            || before.modified().ok() != after.modified().ok()
        {
            return Err(ErrorCode::SourceChanged);
        }
        completed(
            file.seek(SeekFrom::Start(0)).map_err(|_| ErrorCode::Io),
            step,
        )?;
        Ok(Self {
            file,
            len,
            object,
            hash: hash.finalize().into(),
        })
    }
}
