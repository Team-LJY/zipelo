use crate::{protocol::ErrorCode, space, spool};
use std::{
    cell::RefCell,
    fs::File,
    io::{self, BufReader, Read, Seek, Write},
};
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, ErrorCode>;
const CHUNK: usize = 65_536;
const XZ_MEMORY_KIB: u32 = 32 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum Compression {
    Bzip2,
    Xz,
}

#[derive(Debug)]
struct InputError(ErrorCode);

impl std::fmt::Display for InputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}", self.0)
    }
}

impl std::error::Error for InputError {}

struct CheckedInput<'a, F> {
    file: &'a mut File,
    check: &'a RefCell<&'a mut F>,
}

impl<F: FnMut() -> Result<()>> Read for CheckedInput<'_, F> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        (self.check.borrow_mut())().map_err(|error| io::Error::other(InputError(error)))?;
        let count = buffer.len().min(CHUNK);
        self.file
            .read(&mut buffer[..count])
            .map_err(|_| io::Error::other(InputError(ErrorCode::Io)))
    }
}

fn decode_error(error: io::Error) -> ErrorCode {
    if let Some(error) = error
        .get_ref()
        .and_then(|error| error.downcast_ref::<InputError>())
    {
        return error.0;
    }
    match error.kind() {
        io::ErrorKind::OutOfMemory => ErrorCode::Limit,
        io::ErrorKind::Unsupported => ErrorCode::UnsupportedFeature,
        _ => ErrorCode::Integrity,
    }
}

pub(crate) fn decode(
    source: &mut File,
    compression: Compression,
    limit: u64,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<(File, u64)> {
    check()?;
    let expected = if matches!(compression, Compression::Xz) {
        Some(xz_layout(source, limit, check)?)
    } else {
        None
    };
    source.rewind().map_err(space::io_error)?;
    let mut output = spool::temporary()?;
    let check = RefCell::new(check);
    let input = CheckedInput {
        file: source,
        check: &check,
    };
    let mut reader: Box<dyn Read + '_> = match compression {
        Compression::Bzip2 => Box::new(bzip2::bufread::MultiBzDecoder::new(
            BufReader::with_capacity(CHUNK, input),
        )),
        Compression::Xz => Box::new(lzma_rust2::XzReader::new_mem_limit(
            input,
            true,
            XZ_MEMORY_KIB,
        )),
    };
    let mut buffer = Zeroizing::new([0; CHUNK]);
    let mut total = 0u64;
    loop {
        (check.borrow_mut())()?;
        let count = reader.read(&mut buffer[..]).map_err(decode_error)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .filter(|total| *total <= limit)
            .ok_or(ErrorCode::Limit)?;
        // 전체 검증 전에는 비공개 임시 파일에만 기록함.
        space::ensure(&output, count as u64)?;
        output
            .write_all(&buffer[..count])
            .map_err(space::io_error)?;
    }
    (check.borrow_mut())()?;
    if expected.is_some_and(|expected| expected != total) {
        return Err(ErrorCode::Integrity);
    }
    Ok((output, total))
}

fn read_at(
    source: &mut File,
    offset: u64,
    bytes: &mut [u8],
    check: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    check()?;
    source
        .seek(std::io::SeekFrom::Start(offset))
        .map_err(space::io_error)?;
    source.read_exact(bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            ErrorCode::Integrity
        } else {
            ErrorCode::Io
        }
    })
}

fn vli(bytes: &[u8], cursor: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for shift in (0..63).step_by(7) {
        let byte = *bytes.get(*cursor).ok_or(ErrorCode::Integrity)?;
        *cursor += 1;
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            if shift != 0 && byte == 0 {
                return Err(ErrorCode::Integrity);
            }
            return Ok(value);
        }
    }
    Err(ErrorCode::Integrity)
}

// 역방향 인덱스 검사는 디코더의 할당과 실제 해제량 검사보다 먼저 수행함.
fn xz_layout(source: &mut File, limit: u64, check: &mut impl FnMut() -> Result<()>) -> Result<u64> {
    use zlib_rs::crc32::crc32;
    let mut end = source.metadata().map_err(space::io_error)?.len();
    let mut total = 0u64;
    let mut streams = 0usize;
    while end != 0 {
        let mut tail = [0; 4];
        loop {
            let at = end.checked_sub(4).ok_or(ErrorCode::Integrity)?;
            read_at(source, at, &mut tail, check)?;
            if tail != [0; 4] {
                break;
            }
            end = at;
        }
        streams += 1;
        if streams > crate::archive::MAX_OUTPUT_NODES {
            return Err(ErrorCode::Limit);
        }
        let footer_at = end.checked_sub(12).ok_or(ErrorCode::Integrity)?;
        let mut footer = [0; 12];
        read_at(source, footer_at, &mut footer, check)?;
        if &footer[10..] != b"YZ"
            || crc32(0, &footer[4..10]) != u32::from_le_bytes(footer[..4].try_into().unwrap())
        {
            return Err(ErrorCode::Integrity);
        }
        let index_len = (u64::from(u32::from_le_bytes(footer[4..8].try_into().unwrap())) + 1) * 4;
        if index_len > crate::archive::MAX_METADATA_BYTES as u64 {
            return Err(ErrorCode::Limit);
        }
        if index_len < 8 {
            return Err(ErrorCode::Integrity);
        }
        let index_at = footer_at
            .checked_sub(index_len)
            .ok_or(ErrorCode::Integrity)?;
        let mut index = vec![0; index_len as usize];
        read_at(source, index_at, &mut index, check)?;
        let data_end = index.len() - 4;
        if index[0] != 0
            || crc32(0, &index[..data_end])
                != u32::from_le_bytes(index[data_end..].try_into().unwrap())
        {
            return Err(ErrorCode::Integrity);
        }
        let mut cursor = 1;
        let count = vli(&index[..data_end], &mut cursor)?;
        if count > crate::archive::MAX_OUTPUT_NODES as u64 {
            return Err(ErrorCode::Limit);
        }
        let mut blocks = 0u64;
        for _ in 0..count {
            check()?;
            let unpadded = vli(&index[..data_end], &mut cursor)?;
            let size = vli(&index[..data_end], &mut cursor)?;
            if unpadded == 0 {
                return Err(ErrorCode::Integrity);
            }
            blocks = blocks
                .checked_add(unpadded.checked_add(3).ok_or(ErrorCode::Integrity)? & !3)
                .ok_or(ErrorCode::Integrity)?;
            total = total
                .checked_add(size)
                .filter(|total| *total <= limit)
                .ok_or(ErrorCode::Limit)?;
        }
        if data_end - cursor > 3 || index[cursor..data_end].iter().any(|byte| *byte != 0) {
            return Err(ErrorCode::Integrity);
        }
        let start = index_at
            .checked_sub(blocks)
            .and_then(|at| at.checked_sub(12))
            .ok_or(ErrorCode::Integrity)?;
        let mut header = [0; 12];
        read_at(source, start, &mut header, check)?;
        if &header[..6] != b"\xfd7zXZ\0"
            || header[6..8] != footer[8..10]
            || crc32(0, &header[6..8]) != u32::from_le_bytes(header[8..].try_into().unwrap())
        {
            return Err(ErrorCode::Integrity);
        }
        end = start;
    }
    if streams == 0 {
        return Err(ErrorCode::Integrity);
    }
    Ok(total)
}
