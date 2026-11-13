use crate::{
    protocol::{ErrorCode, ResourceUsage},
    space, spool,
};
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

impl<F: crate::resources::Monitor> Read for CheckedInput<'_, F> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.check
            .borrow_mut()
            .check()
            .map_err(|error| io::Error::other(InputError(error)))?;
        let count = buffer.len().min(CHUNK);
        let n = self
            .file
            .read(&mut buffer[..count])
            .map_err(|_| io::Error::other(InputError(ErrorCode::Io)))?;
        if n != 0 {
            self.check
                .borrow_mut()
                .activity()
                .map_err(|e| io::Error::other(InputError(e)))?;
        }
        Ok(n)
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
    check: &mut impl crate::resources::Monitor,
) -> Result<(File, u64)> {
    check.check()?;
    let expected = if matches!(compression, Compression::Xz) {
        Some(xz_layout(source, limit, check)?)
    } else {
        None
    };
    let packed_length = source.metadata().map_err(space::io_error)?.len();
    if let Some(expected) = expected {
        check.observe(
            ResourceUsage {
                estimated_bytes: Some(expected),
                compressed_bytes: Some(packed_length),
                ratio_output_bytes: Some(expected),
                ..ResourceUsage::default()
            },
            Some(source),
        )?;
    }
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
        check.borrow_mut().check()?;
        let count = reader.read(&mut buffer[..]).map_err(decode_error)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .filter(|total| *total <= limit)
            .ok_or(ErrorCode::Limit)?;
        check.borrow_mut().observe(
            ResourceUsage {
                actual_bytes: Some(total),
                compressed_bytes: Some(packed_length),
                ratio_output_bytes: Some(total),
                ..ResourceUsage::default()
            },
            Some(&output),
        )?;
        check.borrow_mut().activity()?;
        // 전체 검증 전에는 비공개 임시 파일에만 기록함.
        space::ensure(&output, count as u64)?;
        output
            .write_all(&buffer[..count])
            .map_err(space::io_error)?;
    }
    check.borrow_mut().check()?;
    if expected.is_some_and(|expected| expected != total) {
        return Err(ErrorCode::Integrity);
    }
    Ok((output, total))
}

fn read_at(
    source: &mut File,
    offset: u64,
    bytes: &mut [u8],
    check: &mut impl crate::resources::Monitor,
) -> Result<()> {
    check.check()?;
    source
        .seek(std::io::SeekFrom::Start(offset))
        .map_err(space::io_error)?;
    source.read_exact(bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            ErrorCode::Integrity
        } else {
            ErrorCode::Io
        }
    })?;
    check.activity()
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

fn lzma2_size(
    source: &mut File,
    mut at: u64,
    end: u64,
    expected: u64,
    check: &mut impl crate::resources::Monitor,
) -> Result<()> {
    let mut size = 0u64;
    while at < end {
        let mut control = [0];
        read_at(source, at, &mut control, check)?;
        at += 1;
        if control[0] == 0 {
            return if at == end && size == expected {
                Ok(())
            } else {
                Err(ErrorCode::Integrity)
            };
        }
        let header_len = match control[0] {
            1 | 2 => 2,
            128..=255 => 4 + usize::from(control[0] >= 192),
            _ => return Err(ErrorCode::Integrity),
        };
        if end - at < header_len as u64 {
            return Err(ErrorCode::Integrity);
        }
        let mut header = [0; 5];
        read_at(source, at, &mut header[..header_len], check)?;
        at += header_len as u64;
        let unpacked = u64::from(u16::from_be_bytes(header[..2].try_into().unwrap())) + 1;
        let (packed, unpacked) = if control[0] >= 128 {
            (
                u64::from(u16::from_be_bytes(header[2..4].try_into().unwrap())) + 1,
                (u64::from(control[0] & 31) << 16) + unpacked,
            )
        } else {
            (unpacked, unpacked)
        };
        at = at
            .checked_add(packed)
            .filter(|at| *at <= end)
            .ok_or(ErrorCode::Integrity)?;
        size = size
            .checked_add(unpacked)
            .filter(|size| *size <= expected)
            .ok_or(ErrorCode::Integrity)?;
    }
    Err(ErrorCode::Integrity)
}

fn xz_block(
    source: &mut File,
    start: u64,
    unpadded: u64,
    size: u64,
    check_size: u64,
    check: &mut impl crate::resources::Monitor,
) -> Result<()> {
    let mut header = [0; 1024];
    read_at(source, start, &mut header[..1], check)?;
    if header[0] == 0 {
        return Err(ErrorCode::Integrity);
    }
    let header_len = (usize::from(header[0]) + 1) * 4;
    let packed = unpadded
        .checked_sub(header_len as u64 + check_size)
        .filter(|packed| *packed != 0)
        .ok_or(ErrorCode::Integrity)?;
    read_at(source, start, &mut header[..header_len], check)?;
    let data = &header[..header_len - 4];
    if zlib_rs::crc32::crc32(0, data)
        != u32::from_le_bytes(header[header_len - 4..header_len].try_into().unwrap())
    {
        return Err(ErrorCode::Integrity);
    }
    let flags = header[1];
    if flags & 0x3c != 0 {
        return Err(ErrorCode::UnsupportedFeature);
    }
    let mut cursor = 2;
    if flags & 0x40 != 0 && vli(data, &mut cursor)? != packed {
        return Err(ErrorCode::Integrity);
    }
    if flags & 0x80 != 0 && vli(data, &mut cursor)? != size {
        return Err(ErrorCode::Integrity);
    }
    let filters = usize::from(flags & 3) + 1;
    for number in 0..filters {
        let id = vli(data, &mut cursor)?;
        // 지원 필터는 길이를 보존하며 마지막 필터는 LZMA2임.
        let supported = if number + 1 == filters {
            id == 0x21
        } else {
            (0x03..=0x0b).contains(&id)
        };
        if !supported {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let properties = vli(data, &mut cursor)?;
        cursor = cursor
            .checked_add(usize::try_from(properties).map_err(|_| ErrorCode::Integrity)?)
            .filter(|cursor| *cursor <= data.len())
            .ok_or(ErrorCode::Integrity)?;
    }
    if data[cursor..].iter().any(|byte| *byte != 0) {
        return Err(ErrorCode::Integrity);
    }
    let data_start = start
        .checked_add(header_len as u64)
        .ok_or(ErrorCode::Integrity)?;
    let data_end = data_start.checked_add(packed).ok_or(ErrorCode::Integrity)?;
    // 청크 경계와 해제량을 블록별로 대조한 뒤 기존 디코더가 내용과 체크섬을 검증함.
    lzma2_size(source, data_start, data_end, size, check)
}

// 역방향 인덱스 검사는 디코더의 할당과 실제 해제량 검사보다 먼저 수행함.
fn xz_layout(
    source: &mut File,
    limit: u64,
    check: &mut impl crate::resources::Monitor,
) -> Result<u64> {
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
        let mut records = Vec::new();
        for _ in 0..count {
            check.check()?;
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
            records.push((unpadded, size));
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
        let check_size = match header[7] {
            0 => 0,
            1 => 4,
            4 => 8,
            10 => 32,
            _ => return Err(ErrorCode::UnsupportedFeature),
        };
        let mut block_at = start + 12;
        for (unpadded, size) in records {
            xz_block(source, block_at, unpadded, size, check_size, check)?;
            block_at += (unpadded + 3) & !3;
        }
        end = start;
    }
    if streams == 0 {
        return Err(ErrorCode::Integrity);
    }
    Ok(total)
}
