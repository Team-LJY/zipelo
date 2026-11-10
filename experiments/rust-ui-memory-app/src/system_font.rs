use std::io;

pub(super) struct RuntimeFont {
    pub bytes: Vec<u8>,
    pub face_index: u32,
}

pub(super) fn load() -> io::Result<RuntimeFont> {
    platform::load()
}

#[cfg(target_os = "linux")]
mod platform {
    use super::RuntimeFont;
    use std::io;

    pub(super) fn load() -> io::Result<RuntimeFont> {
        Ok(RuntimeFont {
            bytes: std::fs::read("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc")?,
            face_index: 1,
        })
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::RuntimeFont;
    use std::{io, path::PathBuf};

    pub(super) fn load() -> io::Result<RuntimeFont> {
        let root = std::env::var_os("SystemRoot").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Windows system directory is unavailable",
            )
        })?;
        let root = PathBuf::from(root);
        if !root.is_absolute()
            || !matches!(root.components().next(), Some(std::path::Component::Prefix(prefix))
                if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows system directory must be a local absolute drive path",
            ));
        }
        Ok(RuntimeFont {
            bytes: std::fs::read(root.join("Fonts").join("malgun.ttf"))?,
            face_index: 0,
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod platform {
    use super::RuntimeFont;
    use std::io;

    pub(super) fn load() -> io::Result<RuntimeFont> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "No Korean system font provider is available for this platform",
        ))
    }
}
