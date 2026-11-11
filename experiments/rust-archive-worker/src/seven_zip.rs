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
    fn bounds<'a>(&self, input: &'a [u8], data_end: usize) -> Result<Vec<&'a [u8]>> {
        let mut at = 32usize
            .checked_add(self.position)
            .ok_or(ErrorCode::Integrity)?;
        let mut slices = Vec::new();
        for (size, expected) in self.packed.iter().zip(&self.packed_crcs) {
            let end = at
                .checked_add(*size)
                .filter(|end| *end <= data_end)
                .ok_or(ErrorCode::Integrity)?;
            let bytes = input.get(at..end).ok_or(ErrorCode::Integrity)?;
            if expected.is_some_and(|crc| crc32(0, bytes) != crc) {
                return Err(ErrorCode::Integrity);
            }
            slices.push(bytes);
            at = end;
        }
        Ok(slices)
    }
    fn decode(
        &self,
        packed: &[&[u8]],
        check: &mut impl FnMut() -> Result<()>,
    ) -> Result<Vec<Vec<u8>>> {
        self.folders
            .iter()
            .zip(packed)
            .map(|(f, bytes)| {
                check()?;
                let out = match f.method.as_slice() {
                    [0] => {
                        if !f.properties.is_empty() || bytes.len() != f.size {
                            return Err(ErrorCode::Integrity);
                        }
                        bytes.to_vec()
                    }
                    [3, 1, 1] => crate::lzma::decode(bytes, &f.properties, false, f.size, check)?,
                    [0x21] => crate::lzma::decode(bytes, &f.properties, true, f.size, check)?,
                    _ => return Err(ErrorCode::UnsupportedFeature),
                };
                if f.crc.is_some_and(|crc| crc32(0, &out) != crc) {
                    return Err(ErrorCode::Integrity);
                }
                let mut at = 0;
                for (size, crc) in f.sizes.iter().zip(&f.crcs) {
                    let end = at + *size;
                    if crc.is_some_and(|crc| crc32(0, &out[at..end]) != crc) {
                        return Err(ErrorCode::Integrity);
                    }
                    at = end;
                }
                Ok(out)
            })
            .collect()
    }
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
pub(crate) fn parse(
    input: &[u8],
    limits: Limits,
    decode: bool,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<Member>> {
    check()?;
    let header = input.get(..32).ok_or(ErrorCode::Integrity)?;
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
    let start = usize::try_from(offset)
        .ok()
        .and_then(|n| n.checked_add(32))
        .ok_or(ErrorCode::Integrity)?;
    let end = start
        .checked_add(len as usize)
        .filter(|n| *n == input.len())
        .ok_or(ErrorCode::Integrity)?;
    let bytes = input.get(start..end).ok_or(ErrorCode::Integrity)?;
    if crc32(0, bytes) != u32::from_le_bytes(header[28..32].try_into().unwrap()) {
        return Err(ErrorCode::Integrity);
    }
    if bytes.is_empty() {
        return if start == 32 {
            Ok(Vec::new())
        } else {
            Err(ErrorCode::Integrity)
        };
    }
    let mut data_end = start;
    let decoded_header;
    let bytes = if bytes[0] == 23 {
        let mut r = Reader::new(&bytes[1..]);
        let mut header_limits = limits;
        header_limits.output = MAX_METADATA_BYTES as u64;
        let streams = Streams::read(&mut r, header_limits)?;
        r.end()?;
        if streams.folders.len() != 1 || streams.folders[0].sizes.len() != 1 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let packed = streams.bounds(input, data_end)?;
        decoded_header = streams
            .decode(&packed, check)?
            .pop()
            .ok_or(ErrorCode::Integrity)?;
        data_end = 32 + streams.position;
        &decoded_header
    } else {
        bytes
    };
    let mut r = Reader::new(bytes);
    r.expect(1)?;
    let mut streams = None;
    let mut tag = r.byte()?;
    if tag == 2 {
        while {
            let tag = r.byte()?;
            tag != 0
        } {
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
    let (mut members, stream_indices) = if tag == 5 {
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
    if let Some(streams) = streams {
        if streams.folders.iter().map(|f| f.sizes.len()).sum::<usize>() != stream_indices.len() {
            return Err(ErrorCode::Integrity);
        }
        let packed = streams.bounds(input, data_end)?;
        if decode {
            let output = streams.decode(&packed, check)?;
            let mut stream_files = stream_indices.into_iter();
            for (folder, bytes) in streams.folders.iter().zip(output) {
                let mut at = 0;
                for size in &folder.sizes {
                    let member = &mut members[stream_files.next().ok_or(ErrorCode::Integrity)?];
                    if member.entry.size as usize != *size {
                        return Err(ErrorCode::Integrity);
                    }
                    member.data = bytes[at..at + size].to_vec();
                    at += size;
                }
            }
        }
    }
    Ok(members)
}
