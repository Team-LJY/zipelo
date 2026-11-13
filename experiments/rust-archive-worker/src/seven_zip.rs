use crate::{
    archive::{MAX_METADATA_BYTES, Member, MetadataBudget, valid_name, validate_members},
    protocol::*,
};
use std::collections::BTreeSet;
use zlib_rs::crc32::crc32;
type Result<T> = std::result::Result<T, ErrorCode>;
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(ErrorCode::Integrity)?;
        let bytes = self.bytes.get(self.at..end).ok_or(ErrorCode::Integrity)?;
        self.at = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn expect(&mut self, id: u8) -> Result<()> {
        if self.byte()? == id {
            Ok(())
        } else {
            Err(ErrorCode::Integrity)
        }
    }
    fn integer(&mut self) -> Result<u64> {
        let first = self.byte()?;
        let mut mask = 0x80;
        let mut value = 0;
        for i in 0..8 {
            if first & mask == 0 {
                return Ok(value | (((first & (mask - 1)) as u64) << (8 * i)));
            }
            value |= (self.byte()? as u64) << (8 * i);
            mask >>= 1;
        }
        Ok(value)
    }
    fn number(&mut self, limit: usize) -> Result<usize> {
        usize::try_from(self.integer()?)
            .ok()
            .filter(|n| *n <= limit)
            .ok_or(ErrorCode::Limit)
    }
    fn bits(&mut self, count: usize) -> Result<Vec<bool>> {
        let bytes = self.take(count.div_ceil(8))?;
        Ok((0..count)
            .map(|i| bytes[i / 8] & (0x80 >> (i % 8)) != 0)
            .collect())
    }
    fn defined(&mut self, count: usize) -> Result<Vec<bool>> {
        match self.byte()? {
            0 => self.bits(count),
            1 => Ok(vec![true; count]),
            _ => Err(ErrorCode::Integrity),
        }
    }
    fn digests(&mut self, count: usize) -> Result<Vec<Option<u32>>> {
        self.defined(count)?
            .into_iter()
            .map(|set| {
                if set {
                    Ok(Some(u32::from_le_bytes(self.take(4)?.try_into().unwrap())))
                } else {
                    Ok(None)
                }
            })
            .collect()
    }
    fn end(&self) -> Result<()> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(ErrorCode::Integrity)
        }
    }
}
struct Folder {
    method: Vec<u8>,
    properties: Vec<u8>,
    size: usize,
    crc: Option<u32>,
    sizes: Vec<usize>,
    crcs: Vec<Option<u32>>,
}
struct Streams {
    position: usize,
    packed: Vec<usize>,
    packed_crcs: Vec<Option<u32>>,
    folders: Vec<Folder>,
}
impl Streams {
    fn read(r: &mut Reader<'_>, limits: Limits) -> Result<Self> {
        r.expect(6)?;
        let position = r.number(limits.input as usize)?;
        let count = r.number(limits.entries as usize)?;
        r.expect(9)?;
        let packed = (0..count)
            .map(|_| r.number(limits.input as usize))
            .collect::<Result<Vec<_>>>()?;
        let mut tag = r.byte()?;
        let packed_crcs = if tag == 10 {
            tag = 255;
            r.digests(count)?
        } else {
            vec![None; count]
        };
        if tag == 255 {
            tag = r.byte()?;
        }
        if tag != 0 {
            return Err(ErrorCode::Integrity);
        }
        r.expect(7)?;
        r.expect(11)?;
        let num = r.number(limits.entries as usize)?;
        if num != count {
            return Err(ErrorCode::UnsupportedFeature);
        }
        if r.byte()? != 0 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let mut folders = Vec::new();
        for _ in 0..num {
            if r.integer()? != 1 {
                return Err(ErrorCode::UnsupportedFeature);
            }
            let flags = r.byte()?;
            let len = (flags & 15) as usize;
            if flags & 0xc0 != 0 || len == 0 {
                return Err(ErrorCode::UnsupportedFeature);
            }
            let method = r.take(len)?.to_vec();
            if flags & 0x10 != 0 && (r.integer()? != 1 || r.integer()? != 1) {
                return Err(ErrorCode::UnsupportedFeature);
            }
            let properties = if flags & 0x20 != 0 {
                let n = r.number(64)?;
                r.take(n)?.to_vec()
            } else {
                Vec::new()
            };
            if !matches!(method.as_slice(), [0] | [3, 1, 1] | [0x21]) {
                return Err(ErrorCode::UnsupportedFeature);
            }
            folders.push(Folder {
                method,
                properties,
                size: 0,
                crc: None,
                sizes: Vec::new(),
                crcs: Vec::new(),
            });
        }
        r.expect(12)?;
        let mut total = 0usize;
        for folder in &mut folders {
            folder.size = r.number(limits.output as usize)?;
            total = total
                .checked_add(folder.size)
                .filter(|n| *n <= limits.output as usize)
                .ok_or(ErrorCode::Limit)?;
        }
        let mut tag = r.byte()?;
        if tag == 10 {
            for (f, crc) in folders.iter_mut().zip(r.digests(num)?) {
                f.crc = crc;
            }
            tag = r.byte()?;
        }
        if tag != 0 {
            return Err(ErrorCode::Integrity);
        }
        let mut counts = vec![1usize; num];
        let mut supplied_sizes = false;
        let mut tag = r.byte()?;
        if tag == 8 {
            tag = r.byte()?;
            if tag == 13 {
                let mut total = 0usize;
                for n in &mut counts {
                    *n = r.number(limits.entries as usize)?;
                    total = total
                        .checked_add(*n)
                        .filter(|n| *n <= limits.entries as usize)
                        .ok_or(ErrorCode::Limit)?;
                }
                tag = r.byte()?;
            }
            if tag == 9 {
                supplied_sizes = true;
                for (folder, count) in folders.iter_mut().zip(&counts) {
                    let mut total = 0usize;
                    for _ in 1..*count {
                        let n = r.number(folder.size)?;
                        total = total
                            .checked_add(n)
                            .filter(|n| *n <= folder.size)
                            .ok_or(ErrorCode::Integrity)?;
                        folder.sizes.push(n);
                    }
                    if *count > 0 {
                        folder.sizes.push(folder.size - total);
                    }
                }
                tag = r.byte()?;
            }
            let digest_count = folders
                .iter()
                .zip(&counts)
                .map(|(f, n)| if *n == 1 && f.crc.is_some() { 0 } else { *n })
                .sum();
            let mut digests = if tag == 10 {
                let d = r.digests(digest_count)?;
                tag = r.byte()?;
                d.into_iter()
            } else {
                vec![None; digest_count].into_iter()
            };
            for (f, n) in folders.iter_mut().zip(&counts) {
                f.crcs = if *n == 1 && f.crc.is_some() {
                    vec![f.crc]
                } else {
                    (0..*n).map(|_| digests.next().unwrap()).collect()
                };
            }
            if tag != 0 {
                return Err(ErrorCode::Integrity);
            }
            tag = r.byte()?;
        } else {
            for f in &mut folders {
                f.crcs.push(f.crc);
            }
        }
        if tag != 0 {
            return Err(ErrorCode::Integrity);
        }
        for (f, n) in folders.iter_mut().zip(&counts) {
            if !supplied_sizes {
                if *n > 1 {
                    return Err(ErrorCode::Integrity);
                }
                if *n == 1 {
                    f.sizes.push(f.size);
                }
            }
            if *n == 0 && f.size != 0 {
                return Err(ErrorCode::Integrity);
            }
        }
        Ok(Self {
            position,
            packed,
            packed_crcs,
            folders,
        })
    }
    fn ranges(
        &self,
        input: &mut std::fs::File,
        data_end: u64,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<Vec<(u64, u64)>> {
        let mut at = 32u64
            .checked_add(self.position as u64)
            .ok_or(ErrorCode::Integrity)?;
        let mut ranges = Vec::new();
        for (size, expected) in self.packed.iter().zip(&self.packed_crcs) {
            let size = *size as u64;
            let end = at
                .checked_add(size)
                .filter(|n| *n <= data_end)
                .ok_or(ErrorCode::Integrity)?;
            if let Some(expected) = expected {
                if digest(input, at, size, check)? != *expected {
                    return Err(ErrorCode::Integrity);
                }
            }
            ranges.push((at, size));
            at = end;
        }
        Ok(ranges)
    }
    fn decode_file(
        &self,
        input: &mut std::fs::File,
        ranges: &[(u64, u64)],
        output: &mut std::fs::File,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<()> {
        use std::io::{Read, Seek, SeekFrom, Write};
        let total = self.folders.iter().try_fold(0u64, |sum, f| {
            sum.checked_add(f.size as u64).ok_or(ErrorCode::Limit)
        })?;
        crate::space::ensure(output, total)?;
        for (f, (start, size)) in self.folders.iter().zip(ranges) {
            check.check()?;
            let base = output.stream_position().map_err(crate::space::io_error)?;
            check.observe(
                ResourceUsage {
                    compressed_bytes: Some(*size),
                    ratio_output_bytes: Some(f.size as u64),
                    ..ResourceUsage::default()
                },
                Some(output),
            )?;
            input
                .seek(SeekFrom::Start(*start))
                .map_err(crate::space::io_error)?;
            match f.method.as_slice() {
                [0] => {
                    if !f.properties.is_empty() || *size != f.size as u64 {
                        return Err(ErrorCode::Integrity);
                    }
                    let mut buffer = zeroize::Zeroizing::new([0; 65536]);
                    let mut remaining = *size;
                    while remaining != 0 {
                        check.check()?;
                        let n = remaining.min(buffer.len() as u64) as usize;
                        input
                            .read_exact(&mut buffer[..n])
                            .map_err(|_| ErrorCode::Integrity)?;
                        crate::space::ensure(output, n as u64)?;
                        output
                            .write_all(&buffer[..n])
                            .map_err(crate::space::io_error)?;
                        remaining -= n as u64;
                        check.activity()?;
                    }
                }
                [3, 1, 1] | [0x21] => crate::lzma::decode_stream(
                    input,
                    *size,
                    &f.properties,
                    f.method == [0x21],
                    f.size as u64,
                    output,
                    check,
                )?,
                _ => return Err(ErrorCode::UnsupportedFeature),
            }
            let single = f.sizes.len() == 1 && f.sizes[0] == f.size;
            let folder_crc = if f.crc.is_some() || single && f.crcs[0].is_some() {
                Some(digest(output, base, f.size as u64, check)?)
            } else {
                None
            };
            if f.crc.is_some() && f.crc != folder_crc {
                return Err(ErrorCode::Integrity);
            }
            let mut at = base;
            for (size, expected) in f.sizes.iter().zip(&f.crcs) {
                if let Some(expected) = expected {
                    let actual = if single {
                        folder_crc.ok_or(ErrorCode::Internal)?
                    } else {
                        digest(output, at, *size as u64, check)?
                    };
                    if actual != *expected {
                        return Err(ErrorCode::Integrity);
                    }
                }
                at = at.checked_add(*size as u64).ok_or(ErrorCode::Integrity)?;
            }
            output
                .seek(SeekFrom::Start(base + f.size as u64))
                .map_err(crate::space::io_error)?;
        }
        Ok(())
    }
}
fn digest(
    file: &mut std::fs::File,
    start: u64,
    size: u64,
    check: &mut impl crate::resources::Monitor,
) -> Result<u32> {
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(start))
        .map_err(crate::space::io_error)?;
    let mut crc = 0;
    let mut remaining = size;
    let mut buffer = zeroize::Zeroizing::new([0; 65536]);
    while remaining != 0 {
        check.check()?;
        let n = remaining.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..n])
            .map_err(crate::space::io_error)?;
        crc = crc32(crc, &buffer[..n]);
        remaining -= n as u64;
        check.activity()?;
    }
    Ok(crc)
}
fn files(
    r: &mut Reader<'_>,
    limits: Limits,
    streams: Option<&Streams>,
) -> Result<(Vec<Member>, Vec<usize>)> {
    let count = r.number(limits.entries as usize)?;
    let mut empty = vec![false; count];
    let mut empty_file = Vec::new();
    let mut names = None;
    let mut attrs = vec![None; count];
    let mut seen = BTreeSet::new();
    loop {
        let property = r.byte()?;
        if property == 0 {
            break;
        }
        if !seen.insert(property) {
            return Err(ErrorCode::Integrity);
        }
        let len = r.number(MAX_METADATA_BYTES)?;
        let mut value = Reader::new(r.take(len)?);
        match property {
            14 => {
                empty = value.bits(count)?;
            }
            15 => {
                empty_file = value.bits(empty.iter().filter(|e| **e).count())?;
            }
            16 => {
                if value
                    .bits(empty.iter().filter(|e| **e).count())?
                    .iter()
                    .any(|b| *b)
                {
                    return Err(ErrorCode::UnsupportedFeature);
                }
            }
            17 => {
                if value.byte()? != 0 {
                    return Err(ErrorCode::UnsupportedFeature);
                }
                let mut decoded = Vec::new();
                let mut budget = MetadataBudget::default();
                for _ in 0..count {
                    let mut units = Vec::new();
                    loop {
                        let n = u16::from_le_bytes(value.take(2)?.try_into().unwrap());
                        if n == 0 {
                            break;
                        }
                        if units.len() >= 4096 {
                            return Err(ErrorCode::Limit);
                        }
                        units.push(n);
                    }
                    let name = String::from_utf16(&units).map_err(|_| ErrorCode::Integrity)?;
                    budget.charge(&name)?;
                    decoded.push(name);
                }
                names = Some(decoded);
            }
            21 => {
                let defined = value.defined(count)?;
                if value.byte()? != 0 {
                    return Err(ErrorCode::UnsupportedFeature);
                }
                for (attr, set) in attrs.iter_mut().zip(defined) {
                    if set {
                        *attr = Some(u32::from_le_bytes(value.take(4)?.try_into().unwrap()));
                    }
                }
            }
            18..=20 => {
                let defined = value.defined(count)?;
                if value.byte()? != 0 {
                    return Err(ErrorCode::UnsupportedFeature);
                }
                value.take(defined.iter().filter(|b| **b).count() * 8)?;
            }
            25 => {
                if value.take(len)?.iter().any(|b| *b != 0) {
                    return Err(ErrorCode::Integrity);
                }
            }
            _ => return Err(ErrorCode::UnsupportedFeature),
        }
        value.end()?;
    }
    let names = names.unwrap_or_default();
    if names.len() != count {
        return Err(ErrorCode::Integrity);
    }
    if !empty_file.is_empty() && empty_file.len() != empty.iter().filter(|e| **e).count() {
        return Err(ErrorCode::Integrity);
    }
    let mut descriptions = Vec::new();
    if let Some(streams) = streams {
        for (folder, compressed) in streams.folders.iter().zip(&streams.packed) {
            for (i, size) in folder.sizes.iter().enumerate() {
                descriptions.push((*size, if i == 0 { *compressed } else { 0 }));
            }
        }
    }
    let mut descriptions = descriptions.into_iter();
    let mut empty_flags = empty_file.into_iter();
    let mut members = Vec::new();
    let mut stream_indices = Vec::new();
    for (index, ((name, empty), attr)) in names.into_iter().zip(empty).zip(attrs).enumerate() {
        let kind = if empty && !empty_flags.next().unwrap_or(false) {
            EntryKind::Directory
        } else {
            EntryKind::File
        };
        if let Some(attr) = attr {
            let mode = attr >> 16;
            if attr & 0x400 != 0
                || (attr & 0x8000 != 0 && !matches!(mode & 0xf000, 0 | 0x4000 | 0x8000))
            {
                return Err(ErrorCode::UnsafePath);
            }
            if (attr & 0x10 != 0 || attr & 0x8000 != 0 && mode & 0xf000 == 0x4000)
                && kind != EntryKind::Directory
            {
                return Err(ErrorCode::Integrity);
            }
        }
        let name = valid_name(&name, kind == EntryKind::Directory)?;
        if !empty {
            stream_indices.push(index);
        }
        let (size, compressed_size) = if empty {
            (0, 0)
        } else {
            descriptions.next().ok_or(ErrorCode::Integrity)?
        };
        members.push(Member {
            entry: Entry {
                entry_id: index as u32,
                name,
                kind,
                size: size as u64,
                compressed_size: compressed_size as u64,
                encrypted: false,
            },
            data: Vec::new(),
        });
    }
    if descriptions.next().is_some() {
        return Err(ErrorCode::Integrity);
    }
    validate_members(&members)?;
    Ok((members, stream_indices))
}
pub(crate) fn open(
    input: &mut std::fs::File,
    length: u64,
    limits: Limits,
    decode: bool,
    check: &mut impl crate::resources::Monitor,
) -> Result<crate::streaming::Catalog> {
    use std::io::{Read, Seek, SeekFrom};
    check.check()?;
    if length > limits.input {
        return Err(ErrorCode::Limit);
    }
    input.rewind().map_err(crate::space::io_error)?;
    let mut header = [0; 32];
    input
        .read_exact(&mut header)
        .map_err(|_| ErrorCode::Integrity)?;
    if &header[..6] != b"7z\xbc\xaf\x27\x1c" {
        return Err(ErrorCode::Integrity);
    }
    if header[6] != 0 || header[7] > 4 {
        return Err(ErrorCode::UnsupportedFeature);
    }
    if crc32(0, &header[12..32]) != u32::from_le_bytes(header[8..12].try_into().unwrap()) {
        return Err(ErrorCode::Integrity);
    }
    let offset = u64::from_le_bytes(header[12..20].try_into().unwrap());
    let len = u64::from_le_bytes(header[20..28].try_into().unwrap());
    if len > MAX_METADATA_BYTES as u64 {
        return Err(ErrorCode::Limit);
    }
    let start = offset.checked_add(32).ok_or(ErrorCode::Integrity)?;
    if start.checked_add(len) != Some(length) {
        return Err(ErrorCode::Integrity);
    }
    input
        .seek(SeekFrom::Start(start))
        .map_err(crate::space::io_error)?;
    let mut bytes = vec![0; len as usize];
    input
        .read_exact(&mut bytes)
        .map_err(|_| ErrorCode::Integrity)?;
    if crc32(0, &bytes) != u32::from_le_bytes(header[28..32].try_into().unwrap()) {
        return Err(ErrorCode::Integrity);
    }
    let mut data = crate::spool::temporary()?;
    if bytes.is_empty() {
        if start != 32 {
            return Err(ErrorCode::Integrity);
        }
        return Ok(crate::streaming::Catalog {
            format: Format::SevenZip,
            members: Vec::new(),
            offsets: Vec::new(),
            data,
        });
    }
    let mut data_end = start;
    if bytes[0] == 23 {
        let mut r = Reader::new(&bytes[1..]);
        let header_limits = Limits {
            output: MAX_METADATA_BYTES as u64,
            ..limits
        };
        let streams = Streams::read(&mut r, header_limits)?;
        r.end()?;
        if streams.folders.len() != 1 || streams.folders[0].sizes.len() != 1 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let ranges = streams.ranges(input, data_end, check)?;
        streams.decode_file(input, &ranges, &mut data, check)?;
        data_end = 32u64
            .checked_add(streams.position as u64)
            .ok_or(ErrorCode::Integrity)?;
        data.rewind().map_err(crate::space::io_error)?;
        bytes.clear();
        data.read_to_end(&mut bytes)
            .map_err(crate::space::io_error)?;
        data = crate::spool::temporary()?;
    }
    let mut r = Reader::new(&bytes);
    r.expect(1)?;
    let mut streams = None;
    let mut tag = r.byte()?;
    if tag == 2 {
        while r.byte()? != 0 {
            let len = r.number(MAX_METADATA_BYTES)?;
            r.take(len)?;
        }
        tag = r.byte()?;
    }
    if tag == 3 {
        return Err(ErrorCode::UnsupportedFeature);
    }
    if tag == 4 {
        streams = Some(Streams::read(&mut r, limits)?);
        tag = r.byte()?;
    }
    let (members, stream_indices) = if tag == 5 {
        let f = files(&mut r, limits, streams.as_ref())?;
        tag = r.byte()?;
        f
    } else {
        (Vec::new(), Vec::new())
    };
    if tag != 0 {
        return Err(ErrorCode::Integrity);
    }
    r.end()?;
    let total = members.iter().try_fold(0u64, |n, m| {
        n.checked_add(m.entry.size).ok_or(ErrorCode::Limit)
    })?;
    let packed = streams
        .as_ref()
        .map(|s| {
            s.packed.iter().try_fold(0u64, |sum, size| {
                sum.checked_add(*size as u64).ok_or(ErrorCode::Integrity)
            })
        })
        .transpose()?;
    check.observe(
        ResourceUsage {
            estimated_bytes: Some(total),
            estimated_entries: Some(members.len() as u32),
            compressed_bytes: packed,
            ratio_output_bytes: Some(total),
            ..ResourceUsage::default()
        },
        Some(&data),
    )?;
    check.activity()?;
    let mut offsets = vec![0; members.len()];
    if let Some(streams) = streams {
        if streams.folders.iter().map(|f| f.sizes.len()).sum::<usize>() != stream_indices.len() {
            return Err(ErrorCode::Integrity);
        }
        let ranges = streams.ranges(input, data_end, check)?;
        let mut at = 0u64;
        let mut ids = stream_indices.into_iter();
        for f in &streams.folders {
            for size in &f.sizes {
                offsets[ids.next().ok_or(ErrorCode::Integrity)?] = at;
                at = at.checked_add(*size as u64).ok_or(ErrorCode::Limit)?;
            }
        }
        if at > limits.output {
            return Err(ErrorCode::Limit);
        }
        if decode {
            streams.decode_file(input, &ranges, &mut data, check)?;
        }
    }
    Ok(crate::streaming::Catalog {
        format: Format::SevenZip,
        members,
        offsets,
        data,
    })
}
pub(crate) fn parse(
    input: &[u8],
    limits: Limits,
    decode: bool,
    check: &mut impl crate::resources::Monitor,
) -> Result<Vec<Member>> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut source = crate::spool::temporary()?;
    crate::space::ensure(&source, input.len() as u64)?;
    source.write_all(input).map_err(crate::space::io_error)?;
    let mut catalog = open(&mut source, input.len() as u64, limits, decode, check)?;
    if decode {
        for (member, offset) in catalog.members.iter_mut().zip(catalog.offsets) {
            catalog
                .data
                .seek(SeekFrom::Start(offset))
                .map_err(crate::space::io_error)?;
            member
                .data
                .try_reserve_exact(member.entry.size as usize)
                .map_err(|_| ErrorCode::Limit)?;
            (&mut catalog.data)
                .take(member.entry.size)
                .read_to_end(&mut member.data)
                .map_err(crate::space::io_error)?;
        }
    }
    Ok(catalog.members)
}
