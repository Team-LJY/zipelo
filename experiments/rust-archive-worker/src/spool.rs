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

pub(crate) struct Snapshot {
    pub file: File,
    pub len: u64,
    pub object: SourceObject,
    pub hash: [u8; 32],
}
impl Snapshot {
    pub fn open(path: &Path, limit: u64, check: &mut impl FnMut() -> Result<()>) -> Result<Self> {
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
        let before = source.metadata().map_err(|_| ErrorCode::Io)?;
        if !before.is_file() {
            return Err(ErrorCode::InvalidRequest);
        }
        let mut prefix = [0; 6];
        let prefix_len = before.len().min(prefix.len() as u64) as usize;
        source
            .read_exact(&mut prefix[..prefix_len])
            .map_err(|_| ErrorCode::SourceChanged)?;
        let limit = if prefix == *b"7z\xbc\xaf\x27\x1c" {
            limit.min(64 * 1024 * 1024)
        } else {
            limit
        };
        if before.len() > limit {
            return Err(ErrorCode::Limit);
        }
        source.rewind().map_err(|_| ErrorCode::Io)?;
        let object = filesystem::object(&source)?;
        let mut file = temporary()?;
        crate::space::ensure(&file, before.len())?;
        let mut buffer = [0; 65536];
        let mut hash = Sha256::new();
        let mut len = 0u64;
        loop {
            check()?;
            let n = source.read(&mut buffer).map_err(|_| ErrorCode::Io)?;
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
        }
        let after = source.metadata().map_err(|_| ErrorCode::Io)?;
        let named = options.open(path).map_err(|_| ErrorCode::SourceChanged)?;
        if filesystem::object(&named)? != object
            || filesystem::object(&source)? != object
            || before.len() != len
            || after.len() != len
            || !named.metadata().map_err(|_| ErrorCode::Io)?.is_file()
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
