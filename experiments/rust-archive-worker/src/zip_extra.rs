use crate::protocol::ErrorCode;

type Result<T> = std::result::Result<T, ErrorCode>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Policy {
    Streaming,
    Classic,
    Recovery,
    Names,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AesExtra {
    pub version: u16,
    pub method: u16,
}

#[derive(Default, Debug)]
pub(crate) struct Extra<'a> {
    pub aes: Option<AesExtra>,
    pub zip64: Option<&'a [u8]>,
    pub unicode: Option<&'a [u8]>,
}

fn u16at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}
fn u32at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn u64at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn aes(value: &[u8]) -> Result<AesExtra> {
    if value.len() < 7 {
        return Err(ErrorCode::Integrity);
    }
    let version = u16at(value, 0);
    let method = u16at(value, 5);
    if !matches!(version, 1 | 2)
        || &value[2..4] != b"AE"
        || value[4] != 3
        || !matches!(method, 0 | 8)
    {
        return Err(ErrorCode::UnsupportedFeature);
    }
    Ok(AesExtra { version, method })
}

pub(crate) fn parse(bytes: &[u8], policy: Policy) -> Result<Extra<'_>> {
    let mut found = Extra::default();
    let mut deferred_aes = None;
    let mut duplicate_aes = false;
    let mut at = 0;
    while at < bytes.len() {
        if bytes.len() - at < 4 {
            return Err(ErrorCode::Integrity);
        }
        let tag = u16at(bytes, at);
        let length = u16at(bytes, at + 2) as usize;
        // 복구 경로는 확장 크기와 강한 암호화를 해석하지 않음.
        if policy == Policy::Recovery && matches!(tag, 1 | 0x0017 | 0x0018) {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let end = at.checked_add(4 + length).ok_or(ErrorCode::Integrity)?;
        let value = bytes.get(at + 4..end).ok_or(ErrorCode::Integrity)?;
        match (policy, tag) {
            (Policy::Classic, 1) => return Err(ErrorCode::UnsupportedFeature),
            (Policy::Streaming, 1) => {
                if found.zip64.replace(value).is_some() {
                    return Err(ErrorCode::Integrity);
                }
            }
            (Policy::Classic, 0x9901) => {
                if found.aes.is_some() {
                    return Err(ErrorCode::Integrity);
                }
                found.aes = Some(aes(value)?);
            }
            (Policy::Streaming, 0x9901) => {
                if deferred_aes.is_some() {
                    duplicate_aes = true;
                } else {
                    deferred_aes = Some(value);
                }
            }
            (Policy::Names, 0x7075) => {
                if found.unicode.is_some() || length < 5 {
                    return Err(ErrorCode::Integrity);
                }
                found.unicode = Some(value);
            }
            _ => {}
        }
        at = end;
    }
    // 스트리밍은 전체 필드 경계를 확인한 뒤 AES 지원 여부를 판단함.
    if let Some(value) = deferred_aes {
        found.aes = Some(aes(value)?);
        if duplicate_aes {
            return Err(ErrorCode::Integrity);
        }
    }
    Ok(found)
}

pub(crate) fn large_value(value: u32, extra: &mut &[u8]) -> Result<u64> {
    if value != u32::MAX {
        return Ok(value as u64);
    }
    if extra.len() < 8 {
        return Err(ErrorCode::Integrity);
    }
    let result = u64at(extra, 0);
    *extra = &extra[8..];
    Ok(result)
}
pub(crate) fn expanded(value: u64, maximum: u64, large: u64) -> Result<u64> {
    if value == maximum || value == large {
        Ok(large)
    } else {
        Err(ErrorCode::Integrity)
    }
}
pub(crate) fn disk_start(value: u16, extra: &mut &[u8]) -> Result<u32> {
    if value != u16::MAX {
        return Ok(value as u32);
    }
    if extra.len() < 4 {
        return Err(ErrorCode::Integrity);
    }
    let result = u32at(extra, 0);
    *extra = &extra[4..];
    Ok(result)
}
pub(crate) fn local_sizes(size: u32, compressed: u32, extra: Option<&[u8]>) -> Result<(u64, u64)> {
    if let Some(extra) = extra {
        // 로컬 ZIP64 추가 필드에는 두 크기가 함께 들어감.
        if extra.len() < 16 {
            return Err(ErrorCode::Integrity);
        }
        Ok((
            expanded(size as u64, u32::MAX as u64, u64at(extra, 0))?,
            expanded(compressed as u64, u32::MAX as u64, u64at(extra, 8))?,
        ))
    } else if size == u32::MAX || compressed == u32::MAX {
        Err(ErrorCode::Integrity)
    } else {
        Ok((size as u64, compressed as u64))
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/zip_extra.rs"]
mod tests;
