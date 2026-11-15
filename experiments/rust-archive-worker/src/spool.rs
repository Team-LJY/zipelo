use crate::{filesystem, protocol::*};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

type Result<T> = std::result::Result<T, ErrorCode>;

thread_local! {
    // 작업 스레드가 지정한 임시 파일 위치. 없으면 시스템 임시 폴더를 사용함.
    static LOCATION: std::cell::RefCell<Option<filesystem::Dir>> =
        const { std::cell::RefCell::new(None) };
    // 출력 폴더에 이름 있는 임시 파일을 만든 뒤 이름을 지우지 못했는지 기록함.
    static LEFTOVER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 해제 작업의 임시 파일을 출력 폴더에 두도록 지정함. 반환값이 사라지면 해제됨.
/// 작업은 한 스레드에서 실행되므로 다른 작업이나 시험에 섞이지 않음.
pub(crate) struct Location(());
impl Drop for Location {
    fn drop(&mut self) {
        LOCATION.with(|location| *location.borrow_mut() = None);
    }
}
pub(crate) fn place_in(dir: &filesystem::Dir) -> Result<Location> {
    let dir = dir.duplicate()?;
    LOCATION.with(|location| *location.borrow_mut() = Some(dir));
    LEFTOVER.with(|leftover| leftover.set(false));
    Ok(Location(()))
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

pub(crate) fn random_name() -> Result<String> {
    let mut random = [0; 16];
    crate::crypto::random(&mut random)?;
    Ok(random.iter().map(|b| format!("{b:02x}")).collect())
}

/// 이름 없는 임시 파일을 만듦. 해제 작업에서는 출력 폴더에, 그 외에는 시스템 임시 폴더에 둠.
/// 출력 폴더에 두면 공간 판단이 실제 출력 디스크와 같아지고 /tmp 크기나 할당량에 묶이지 않음.
/// 출력 폴더에 만들지 못하면(읽기 전용, 공간 부족 등) 이 작업의 남은 임시 파일은 모두
/// 기존 시스템 임시 폴더에 둠. 그러면 동작과 오류 우선순위가 이전과 같아지며, 출력 폴더
/// 문제는 원래의 출력 확인 지점에서 보고됨.
pub(crate) fn temporary() -> Result<File> {
    let placed =
        LOCATION.with(|location| location.borrow().as_ref().map(filesystem::Dir::temporary));
    match placed {
        Some(Ok(file)) => return Ok(file),
        Some(Err(_)) => LOCATION.with(|location| *location.borrow_mut() = None),
        None => {}
    }
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
