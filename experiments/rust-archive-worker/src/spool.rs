use crate::{filesystem, protocol::*};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

type Result<T> = std::result::Result<T, ErrorCode>;

pub(crate) fn temporary() -> Result<File> {
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
            Err(ref error) if error.kind() == std::io::ErrorKind::StorageFull => {
                return Err(ErrorCode::DiskFull);
            }
            Err(_) => {}
        }
        let mut random = [0; 16];
        crate::crypto::random(&mut random)?;
        let name = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
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
        use std::os::windows::fs::OpenOptionsExt;
        let mut random = [0; 16];
        crate::crypto::random(&mut random)?;
        let name = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(0)
            .access_mode(0xc0010000)
            .custom_flags(0x04000000 | 0x00200000)
            .attributes(0x100)
            .open(std::env::temp_dir().join(format!("zipelo-{name}.tmp")))
            .map_err(crate::space::io_error)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    Check,
    Read { bytes: u64, total: u64 },
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
        let mut source = options.open(path).map_err(|_| ErrorCode::Io)?;
        let snapshot = Self::capture_reporting(&mut source, limit, step)?;
        let named = options.open(path).map_err(|_| ErrorCode::SourceChanged)?;
        if filesystem::object(&named)? != snapshot.object
            || !named.metadata().map_err(|_| ErrorCode::Io)?.is_file()
        {
            return Err(ErrorCode::SourceChanged);
        }
        Ok(snapshot)
    }
    pub(crate) fn capture(
        source: &mut File,
        limit: u64,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<Self> {
        Self::capture_reporting(source, limit, &mut |step| match step {
            Step::Check => check.check(),
            Step::Read { .. } => check.activity(),
        })
    }
    fn capture_reporting(
        source: &mut File,
        limit: u64,
        step: &mut impl FnMut(Step) -> Result<()>,
    ) -> Result<Self> {
        let before = source.metadata().map_err(|_| ErrorCode::Io)?;
        if !before.is_file() {
            return Err(ErrorCode::InvalidRequest);
        }
        if before.len() > limit {
            return Err(ErrorCode::Limit);
        }
        source.rewind().map_err(|_| ErrorCode::Io)?;
        let object = filesystem::object(&source)?;
        let mut file = temporary()?;
        crate::space::ensure(&file, before.len())?;
        let mut buffer = zeroize::Zeroizing::new([0; 65536]);
        let mut hash = Sha256::new();
        let mut len = 0u64;
        let total = before.len();
        let mut reported = std::time::Instant::now();
        loop {
            step(Step::Check)?;
            let n = source.read(&mut buffer[..]).map_err(|_| ErrorCode::Io)?;
            if n == 0 {
                break;
            }
            len = len
                .checked_add(n as u64)
                .filter(|n| *n <= limit)
                .ok_or(ErrorCode::Limit)?;
            file.write_all(&buffer[..n])
                .map_err(crate::space::io_error)?;
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
        file.seek(SeekFrom::Start(0)).map_err(|_| ErrorCode::Io)?;
        Ok(Self {
            file,
            len,
            object,
            hash: hash.finalize().into(),
        })
    }
}
