use crate::{protocol::*, resources::Monitor};
use std::io::{self, BufRead, Cursor, Read, Seek, SeekFrom};
use zlib_rs::crc32::crc32;
use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};

type Result<T> = std::result::Result<T, ErrorCode>;

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Text,
    Png,
    Jpeg,
}
impl Kind {
    pub fn from_name(name: &str) -> Result<Self> {
        match name
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("txt" | "md" | "log" | "csv" | "json") => Ok(Self::Text),
            Some("png") => Ok(Self::Png),
            Some("jpg" | "jpeg") => Ok(Self::Jpeg),
            _ => Err(ErrorCode::UnsupportedFeature),
        }
    }
    pub fn limit(self) -> u64 {
        match self {
            Self::Text => MAX_PREVIEW_TEXT_BYTES,
            Self::Png | Self::Jpeg => MAX_PREVIEW_IMAGE_BYTES,
        }
    }
}
fn rgba_info(entry_id: u32, width: u32, height: u32) -> Result<PreviewInfo> {
    if width == 0 || height == 0 {
        return Err(ErrorCode::Integrity);
    }
    let total_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(4))
        .ok_or(ErrorCode::Limit)?;
    let info = PreviewInfo {
        entry_id,
        format: PreviewFormat::Rgba8,
        total_bytes,
        width,
        height,
    };
    if !info.valid() {
        return Err(ErrorCode::Limit);
    }
    Ok(info)
}
fn buffer(length: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| ErrorCode::Limit)?;
    bytes.resize(length, 0);
    Ok(bytes)
}
pub(crate) fn decode(
    kind: Kind,
    entry_id: u32,
    mut source: Vec<u8>,
    check: &mut impl Monitor,
) -> Result<(PreviewInfo, Vec<u8>)> {
    check.check()?;
    if source.len() as u64 > kind.limit() {
        return Err(ErrorCode::Limit);
    }
    let result = match kind {
        Kind::Text => {
            std::str::from_utf8(&source).map_err(|_| ErrorCode::Integrity)?;
            if source.starts_with(&[0xef, 0xbb, 0xbf]) {
                source.drain(..3);
            }
            Ok((
                PreviewInfo {
                    entry_id,
                    format: PreviewFormat::Utf8Text,
                    total_bytes: source.len() as u64,
                    width: 0,
                    height: 0,
                },
                source,
            ))
        }
        Kind::Png => png(entry_id, &source, check),
        Kind::Jpeg => jpeg(entry_id, &source, check),
    };
    check.activity()?;
    result
}
struct Input<'a, M> {
    bytes: Cursor<&'a [u8]>,
    check: &'a mut M,
}
impl<M: Monitor> Read for Input<'_, M> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.check
            .check()
            .map_err(|_| io::Error::other("preview interrupted"))?;
        let length = self.bytes.read(bytes)?;
        if length != 0 {
            self.check
                .activity()
                .map_err(|_| io::Error::other("preview interrupted"))?;
        }
        Ok(length)
    }
}
impl<M: Monitor> BufRead for Input<'_, M> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.check
            .check()
            .map_err(|_| io::Error::other("preview interrupted"))?;
        self.bytes.fill_buf()
    }
    fn consume(&mut self, length: usize) {
        self.bytes.consume(length);
    }
}
impl<M: Monitor> Seek for Input<'_, M> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.check
            .check()
            .map_err(|_| io::Error::other("preview interrupted"))?;
        self.bytes.seek(position)
    }
}
fn png(entry_id: u32, source: &[u8], check: &mut impl Monitor) -> Result<(PreviewInfo, Vec<u8>)> {
    if !source.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(ErrorCode::UnsupportedFormat);
    }
    let mut at = 8usize;
    let mut ended = false;
    while at < source.len() {
        check.check()?;
        let header = source.get(at..at + 8).ok_or(ErrorCode::Integrity)?;
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let end = at
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            .filter(|n| *n <= source.len())
            .ok_or(ErrorCode::Integrity)?;
        if crc32(0, &source[at + 4..end - 4])
            != u32::from_be_bytes(source[end - 4..end].try_into().unwrap())
        {
            return Err(ErrorCode::Integrity);
        }
        if at == 8 && (&header[4..] != b"IHDR" || length != 13) {
            return Err(ErrorCode::Integrity);
        }
        if matches!(&header[4..], b"acTL" | b"fcTL" | b"fdAT") {
            return Err(ErrorCode::UnsupportedFeature);
        }
        if &header[4..] == b"IEND" {
            if length != 0 || end != source.len() {
                return Err(ErrorCode::Integrity);
            }
            ended = true;
        }
        at = end;
        check.activity()?;
    }
    if !ended {
        return Err(ErrorCode::Integrity);
    }
    let mut decoder = png::Decoder::new_with_limits(
        Input {
            bytes: Cursor::new(source),
            check,
        },
        png::Limits {
            bytes: 128 * 1024 * 1024,
        },
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let header = decoder
        .read_header_info()
        .map_err(|_| ErrorCode::Integrity)?;
    let info = rgba_info(entry_id, header.width, header.height)?;
    let mut reader = decoder.read_info().map_err(|_| ErrorCode::Integrity)?;
    let length = reader
        .output_buffer_size()
        .filter(|n| *n as u64 <= info.total_bytes)
        .ok_or(ErrorCode::Limit)?;
    let mut raw = buffer(length)?;
    let decoded = reader
        .next_frame(&mut raw)
        .map_err(|_| ErrorCode::Integrity)?;
    reader.finish().map_err(|_| ErrorCode::Integrity)?;
    drop(reader);
    check.activity()?;
    if decoded.width != info.width
        || decoded.height != info.height
        || decoded.bit_depth != png::BitDepth::Eight
    {
        return Err(ErrorCode::Integrity);
    }
    raw.truncate(decoded.buffer_size());
    let channels = match decoded.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => return Err(ErrorCode::UnsupportedFeature),
    };
    if raw.len() != info.width as usize * info.height as usize * channels {
        return Err(ErrorCode::Integrity);
    }
    if channels == 4 {
        return Ok((info, raw));
    }
    let mut rgba = buffer(info.total_bytes as usize)?;
    for (source, target) in raw
        .chunks_exact(info.width as usize * channels)
        .zip(rgba.chunks_exact_mut(info.width as usize * 4))
    {
        check.activity()?;
        for (pixel, out) in source
            .chunks_exact(channels)
            .zip(target.chunks_exact_mut(4))
        {
            match channels {
                1 => out.copy_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
                2 => out.copy_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]),
                3 => out.copy_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
                _ => unreachable!(),
            }
        }
    }
    Ok((info, rgba))
}
fn jpeg_info(entry_id: u32, source: &[u8], check: &mut impl Monitor) -> Result<PreviewInfo> {
    let mut at = 2usize;
    let mut scanning = false;
    let mut info = None;
    let mut seen_scan = false;
    let mut components = Vec::new();
    let mut decoded_components = 0u8;
    let mut progressive = false;
    while at < source.len() {
        check.check()?;
        if scanning {
            at += source[at..]
                .iter()
                .position(|byte| *byte == 0xff)
                .ok_or(ErrorCode::Integrity)?;
        }
        if source[at] != 0xff {
            return Err(ErrorCode::Integrity);
        }
        while source.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *source.get(at).ok_or(ErrorCode::Integrity)?;
        at += 1;
        match marker {
            0 | 0xd0..=0xd7 if scanning => continue,
            0xd9 if at == source.len() && seen_scan => {
                if decoded_components != (1 << components.len()) - 1 {
                    return Err(ErrorCode::Integrity);
                }
                return info.ok_or(ErrorCode::Integrity);
            }
            0 | 0xd0..=0xd9 => return Err(ErrorCode::Integrity),
            1 => continue,
            _ => {}
        }
        let length = source.get(at..at + 2).ok_or(ErrorCode::Integrity)?;
        let length = u16::from_be_bytes(length.try_into().unwrap()) as usize;
        let end = at
            .checked_add(length)
            .filter(|n| length >= 2 && *n <= source.len())
            .ok_or(ErrorCode::Integrity)?;
        let body = &source[at + 2..end];
        if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            if info.is_some() || body.len() < 6 {
                return Err(ErrorCode::Integrity);
            }
            if !matches!(marker, 0xc0 | 0xc2) || body[0] != 8 {
                return Err(ErrorCode::UnsupportedFeature);
            }
            progressive = marker == 0xc2;
            let count = usize::from(body[5]);
            if !(1..=4).contains(&count) || body.len() != 6 + count * 3 {
                return Err(ErrorCode::Integrity);
            }
            for component in body[6..].chunks_exact(3) {
                if components.contains(&component[0]) {
                    return Err(ErrorCode::Integrity);
                }
                components.push(component[0]);
            }
            info = Some(rgba_info(
                entry_id,
                u32::from(u16::from_be_bytes(body[3..5].try_into().unwrap())),
                u32::from(u16::from_be_bytes(body[1..3].try_into().unwrap())),
            )?);
        }
        if marker == 0xe2 && body.starts_with(b"MPF\0") {
            return Err(ErrorCode::UnsupportedFeature);
        }
        scanning = marker == 0xda;
        if scanning {
            let count = usize::from(*body.first().ok_or(ErrorCode::Integrity)?);
            if info.is_none()
                || count == 0
                || count > components.len()
                || body.len() != 4 + count * 2
            {
                return Err(ErrorCode::Integrity);
            }
            let mut scan_components = 0u8;
            for component in body[1..1 + count * 2].chunks_exact(2) {
                let index = components
                    .iter()
                    .position(|id| *id == component[0])
                    .ok_or(ErrorCode::Integrity)?;
                let bit = 1 << index;
                if scan_components & bit != 0 {
                    return Err(ErrorCode::Integrity);
                }
                scan_components |= bit;
            }
            if !progressive
                && (scan_components & decoded_components != 0
                    || body[1 + count * 2..] != [0, 63, 0])
            {
                return Err(ErrorCode::Integrity);
            }
            // 모든 색상 성분의 최초 DC 스캔이 있어야 완전한 그림으로 처리함.
            if body[1 + count * 2] == 0 && body[3 + count * 2] >> 4 == 0 {
                decoded_components |= scan_components;
            }
        }
        seen_scan |= scanning;
        at = end;
        check.activity()?;
    }
    Err(ErrorCode::Integrity)
}
fn jpeg_error(error: zune_jpeg::errors::DecodeErrors) -> ErrorCode {
    match error {
        zune_jpeg::errors::DecodeErrors::LargeDimensions(_) => ErrorCode::Limit,
        zune_jpeg::errors::DecodeErrors::Unsupported(_) => ErrorCode::UnsupportedFeature,
        _ => ErrorCode::Integrity,
    }
}
fn jpeg(entry_id: u32, source: &[u8], check: &mut impl Monitor) -> Result<(PreviewInfo, Vec<u8>)> {
    if !source.starts_with(&[0xff, 0xd8]) {
        return Err(ErrorCode::UnsupportedFormat);
    }
    if !source.ends_with(&[0xff, 0xd9]) {
        return Err(ErrorCode::Integrity);
    }
    let expected = jpeg_info(entry_id, source, check)?;
    let options = DecoderOptions::new_safe()
        .set_strict_mode(true)
        .set_max_width(MAX_PREVIEW_DIMENSION as usize)
        .set_max_height(MAX_PREVIEW_DIMENSION as usize)
        .jpeg_set_max_scans(100)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(source), options);
    decoder.decode_headers().map_err(jpeg_error)?;
    let header = decoder.info().ok_or(ErrorCode::Integrity)?;
    let info = rgba_info(entry_id, u32::from(header.width), u32::from(header.height))?;
    let rgb_bytes = (info.total_bytes / 4 * 3) as usize;
    if info != expected || decoder.output_buffer_size() != Some(rgb_bytes) {
        return Err(ErrorCode::Integrity);
    }
    let mut rgba = buffer(info.total_bytes as usize)?;
    decoder
        .decode_into(&mut rgba[..rgb_bytes])
        .map_err(jpeg_error)?;
    // CMYK의 RGBA 경로를 피하고 같은 버퍼 안에서 불투명 알파를 채움.
    for row in (0..info.height as usize).rev() {
        check.activity()?;
        for column in (0..info.width as usize).rev() {
            let pixel = row * info.width as usize + column;
            let rgb = [
                rgba[pixel * 3],
                rgba[pixel * 3 + 1],
                rgba[pixel * 3 + 2],
                255,
            ];
            rgba[pixel * 4..pixel * 4 + 4].copy_from_slice(&rgb);
        }
    }
    Ok((info, rgba))
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/preview_decode.rs"]
mod tests;
