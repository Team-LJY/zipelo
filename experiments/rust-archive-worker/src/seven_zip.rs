use crate::{
    archive::{Member, MetadataBudget, valid_name, validate_members_with},
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
const AES: [u8; 4] = [6, 0xf1, 7, 1];
struct Folder {
    method: Vec<u8>,
    properties: Vec<u8>,
    /// 7zAES 코더가 있으면 그 속성. 묶음 스트림을 복호화한 뒤 `method`로 풂.
    encryption: Option<crate::crypto::SevenZipAes>,
    /// 복호화한 뒤 `method` 코더에 넘기는 크기. 암호가 없으면 묶음 크기와 같음.
    inner: usize,
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
    fn read(
        r: &mut Reader<'_>,
        limits: Limits,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<Self> {
        r.expect(6)?;
        let position = r.number(limits.input as usize)?;
        let count = r.number(limits.entries as usize)?;
        r.expect(9)?;
        let packed = (0..count)
            .map(|_| crate::resources::completed(r.number(limits.input as usize), check))
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
        let mut layouts = Vec::new();
        for _ in 0..num {
            // 단일 코더이거나, 7zAES와 Copy/LZMA/LZMA2를 묶은 `7z a -p`의 두 코더 구성만 받음.
            let coders = r.integer()?;
            if !(1..=2).contains(&coders) {
                return Err(ErrorCode::UnsupportedFeature);
            }
            let mut list = Vec::new();
            for _ in 0..coders {
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
                if !matches!(method.as_slice(), [0] | [3, 1, 1] | [0x21]) && method != AES {
                    return Err(ErrorCode::UnsupportedFeature);
                }
                list.push((method, properties));
            }
            let aes = list.iter().position(|(method, _)| *method == AES);
            let main = if coders == 2 {
                // 결합 쌍은 주 코더의 입력을 7zAES의 출력에 잇고, 남은 7zAES 입력이 묶음 스트림임.
                let input = r.integer()?;
                let output = r.integer()?;
                let (Some(aes), main) = (aes, aes.map_or(0, |a| 1 - a)) else {
                    return Err(ErrorCode::UnsupportedFeature);
                };
                if list[main].0 == AES || input != main as u64 || output != aes as u64 {
                    return Err(ErrorCode::UnsupportedFeature);
                }
                main
            } else {
                0
            };
            let encryption = aes
                .map(|index| crate::crypto::SevenZipAes::parse(&list[index].1))
                .transpose()?;
            let (method, properties) = if coders == 1 && encryption.is_some() {
                (vec![0], Vec::new())
            } else {
                list.swap_remove(main)
            };
            layouts.push((coders as usize, main, aes));
            folders.push(Folder {
                method,
                properties,
                encryption,
                inner: 0,
                size: 0,
                crc: None,
                sizes: Vec::new(),
                crcs: Vec::new(),
            });
            crate::resources::completed(Ok(()), check)?;
        }
        r.expect(12)?;
        let mut total = 0usize;
        for (folder, (coders, main, aes)) in folders.iter_mut().zip(&layouts) {
            let mut sizes = [0; 2];
            for size in &mut sizes[..*coders] {
                *size = r.number(limits.output as usize)?;
            }
            folder.size = sizes[*main];
            folder.inner = match aes {
                Some(aes) if *coders == 2 => sizes[*aes],
                _ => folder.size,
            };
            total = total
                .checked_add(folder.size)
                .filter(|n| *n <= limits.output as usize)
                .ok_or(ErrorCode::Limit)?;
            crate::resources::completed(Ok(()), check)?;
        }
        let mut tag = r.byte()?;
        if tag == 10 {
            for (f, crc) in folders.iter_mut().zip(r.digests(num)?) {
                f.crc = crc;
                crate::resources::completed(Ok(()), check)?;
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
                    crate::resources::completed(Ok(()), check)?;
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
                        crate::resources::completed(Ok(()), check)?;
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
                crate::resources::completed(Ok(()), check)?;
            }
            if tag != 0 {
                return Err(ErrorCode::Integrity);
            }
            tag = r.byte()?;
        } else {
            for f in &mut folders {
                f.crcs.push(f.crc);
                crate::resources::completed(Ok(()), check)?;
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
            // 틀린 암호의 출력을 CRC로 가려내야 하므로 CRC가 없는 암호 폴더는 받지 않음.
            if f.encryption.is_some() && f.crc.is_none() && f.crcs.iter().any(|c| c.is_none()) {
                return Err(ErrorCode::UnsupportedFeature);
            }
            crate::resources::completed(Ok(()), check)?;
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
        input: &mut (impl std::io::Read + std::io::Seek),
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
            crate::resources::completed(Ok(()), check)?;
        }
        Ok(ranges)
    }
    fn decode_file(
        &self,
        input: &mut (impl std::io::Read + std::io::Seek),
        ranges: &[(u64, u64)],
        output: &mut std::fs::File,
        selected: Option<usize>,
        keys: &mut Keys<'_>,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<()> {
        use std::io::{Read, Seek, SeekFrom};
        let total = self
            .folders
            .iter()
            .enumerate()
            .filter(|(index, _)| selected.is_none_or(|id| id == *index))
            .try_fold(0u64, |sum, (_, f)| {
                crate::resources::completed(
                    sum.checked_add(f.size as u64).ok_or(ErrorCode::Limit),
                    check,
                )
            })?;
        crate::space::ensure(output, total)?;
        for (index, (f, (start, size))) in self.folders.iter().zip(ranges).enumerate() {
            check.check()?;
            if selected.is_some_and(|id| id != index) {
                crate::resources::completed(Ok(()), check)?;
                continue;
            }
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
            match &f.encryption {
                None => decode_folder(input, *size, f, output, base, check)?,
                Some(aes) => {
                    let key = keys.key(aes, check)?;
                    let mut reader = crate::crypto::SevenZipReader::new(
                        input.by_ref().take(*size),
                        *size,
                        f.inner as u64,
                        aes,
                        &key,
                    )?;
                    // 복호화한 뒤의 해제 실패나 CRC 불일치는 틀린 암호로 보고함. LZMA2는 청크
                    // 머리의 속성도 암호문에서 나오므로 미지원 속성도 틀린 암호로 봄.
                    decode_folder(&mut reader, f.inner as u64, f, output, base, check).map_err(
                        |error| match error {
                            ErrorCode::Integrity => ErrorCode::WrongPassword,
                            ErrorCode::UnsupportedFeature if f.method == [0x21] => {
                                ErrorCode::WrongPassword
                            }
                            error => error,
                        },
                    )?
                }
            }
        }
        Ok(())
    }
}
/// 한 번 여는 동안 받은 암호와, 같은 반복 횟수와 솔트로 이미 유도한 키를 보관함.
pub(crate) struct Keys<'a> {
    password: Option<&'a [u8]>,
    cached: Option<(crate::crypto::SevenZipAes, zeroize::Zeroizing<[u8; 32]>)>,
}
impl<'a> Keys<'a> {
    fn new(password: Option<&'a [u8]>) -> Self {
        Self {
            password,
            cached: None,
        }
    }
    fn key(
        &mut self,
        aes: &crate::crypto::SevenZipAes,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<zeroize::Zeroizing<[u8; 32]>> {
        if let Some((known, key)) = &self.cached {
            if known.same_key(aes) {
                return Ok(key.clone());
            }
        }
        // 암호 없이 암호 폴더를 만나면 호출자가 암호를 물어 다시 열도록 틀린 암호로 보고함.
        let key = aes.key(self.password.ok_or(ErrorCode::WrongPassword)?, check)?;
        self.cached = Some((aes.clone(), key.clone()));
        Ok(key)
    }
}
fn decode_folder(
    input: &mut impl std::io::Read,
    size: u64,
    f: &Folder,
    output: &mut std::fs::File,
    base: u64,
    check: &mut impl crate::resources::Monitor,
) -> Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    match f.method.as_slice() {
        [0] => {
            if !f.properties.is_empty() || size != f.size as u64 {
                return Err(ErrorCode::Integrity);
            }
            let mut buffer = zeroize::Zeroizing::new([0; 65536]);
            let mut remaining = size;
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
                crate::resources::completed(Ok(()), check)?;
            }
        }
        [3, 1, 1] | [0x21] => crate::lzma::decode_stream(
            input,
            size,
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
        crate::resources::completed(Ok(()), check)?;
    }
    crate::resources::completed(
        output
            .seek(SeekFrom::Start(base + f.size as u64))
            .map_err(crate::space::io_error),
        check,
    )?;
    Ok(())
}
fn digest(
    file: &mut (impl std::io::Read + std::io::Seek),
    start: u64,
    size: u64,
    check: &mut impl crate::resources::Monitor,
) -> Result<u32> {
    use std::io::SeekFrom;
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
        crate::resources::completed(Ok(()), check)?;
    }
    Ok(crc)
}
fn files(
    r: &mut Reader<'_>,
    limits: Limits,
    streams: Option<&Streams>,
    check: &mut impl crate::resources::Monitor,
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
        // 7-Zip은 정렬용 kDummy(25)를 한 헤더에 여러 번 쓸 수 있음.
        if property != 25 && !seen.insert(property) {
            return Err(ErrorCode::Integrity);
        }
        let len = r.number(check.metadata_limits().bytes)?;
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
                    check.check()?;
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
                    budget.charge_with(&name, check.metadata_limits())?;
                    decoded.push(name);
                    crate::resources::completed(Ok(()), check)?;
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
                    crate::resources::completed(Ok(()), check)?;
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
                descriptions.push((
                    *size,
                    if i == 0 { *compressed } else { 0 },
                    folder.encryption.is_some(),
                ));
                crate::resources::completed(Ok(()), check)?;
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
        let (size, compressed_size, encrypted) = if empty {
            (0, 0, false)
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
                encrypted,
            },
            data: Vec::new(),
        });
        crate::resources::completed(Ok(()), check)?;
    }
    if descriptions.next().is_some() {
        return Err(ErrorCode::Integrity);
    }
    validate_members_with(&members, check)?;
    Ok((members, stream_indices))
}
pub(crate) fn open(
    input: &mut (impl std::io::Read + std::io::Seek),
    length: u64,
    limits: Limits,
    decode: bool,
    check: &mut impl crate::resources::Monitor,
) -> Result<crate::streaming::Catalog> {
    open_selected(input, length, limits, decode, None, None, check)
}
/// 암호화된 헤더나 항목을 받은 암호로 엶. 암호가 필요한데 없으면 WrongPassword를 돌려줌.
pub(crate) fn open_with_password(
    input: &mut (impl std::io::Read + std::io::Seek),
    length: u64,
    limits: Limits,
    password: Option<&[u8]>,
    check: &mut impl crate::resources::Monitor,
) -> Result<crate::streaming::Catalog> {
    open_selected(input, length, limits, false, password, None, check)
}
pub(crate) fn open_selected(
    input: &mut (impl std::io::Read + std::io::Seek),
    length: u64,
    limits: Limits,
    decode: bool,
    password: Option<&[u8]>,
    selected: Option<u32>,
    check: &mut impl crate::resources::Monitor,
) -> Result<crate::streaming::Catalog> {
    use std::io::{Read, Seek, SeekFrom};
    check.check()?;
    let mut keys = Keys::new(password);
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
    if len > check.metadata_limits().bytes as u64 {
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
            output: check.metadata_limits().bytes as u64,
            ..limits
        };
        let streams = Streams::read(&mut r, header_limits, check)?;
        r.end()?;
        if streams.folders.len() != 1 || streams.folders[0].sizes.len() != 1 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let ranges = streams.ranges(input, data_end, check)?;
        streams.decode_file(
            input,
            &ranges,
            &mut data,
            None,
            &mut keys,
            &mut crate::resources::MetadataMonitor(check),
        )?;
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
            let len = r.number(check.metadata_limits().bytes)?;
            r.take(len)?;
        }
        tag = r.byte()?;
    }
    if tag == 3 {
        return Err(ErrorCode::UnsupportedFeature);
    }
    if tag == 4 {
        streams = Some(Streams::read(&mut r, limits, check)?);
        tag = r.byte()?;
    }
    let (members, stream_indices) = if tag == 5 {
        let f = files(&mut r, limits, streams.as_ref(), check)?;
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
        crate::resources::completed(n.checked_add(m.entry.size).ok_or(ErrorCode::Limit), check)
    })?;
    let packed = streams
        .as_ref()
        .map(|s| {
            s.packed.iter().try_fold(0u64, |sum, size| {
                crate::resources::completed(
                    sum.checked_add(*size as u64).ok_or(ErrorCode::Integrity),
                    check,
                )
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
    crate::resources::completed(Ok(()), check)?;
    let mut offsets = vec![0; members.len()];
    if let Some(streams) = streams {
        if streams.folders.iter().map(|f| f.sizes.len()).sum::<usize>() != stream_indices.len() {
            return Err(ErrorCode::Integrity);
        }
        let ranges = streams.ranges(input, data_end, check)?;
        let mut at = 0u64;
        let mut ids = stream_indices.into_iter();
        let mut selected_folder = None;
        for (folder, f) in streams.folders.iter().enumerate() {
            let base = at;
            for size in &f.sizes {
                let id = ids.next().ok_or(ErrorCode::Integrity)?;
                offsets[id] = if selected.is_some() { at - base } else { at };
                if selected == Some(id as u32) {
                    selected_folder = Some(folder);
                }
                at = at.checked_add(*size as u64).ok_or(ErrorCode::Limit)?;
                crate::resources::completed(Ok(()), check)?;
            }
        }
        if at > limits.output {
            return Err(ErrorCode::Limit);
        }
        if decode && (selected.is_none() || selected_folder.is_some()) {
            streams.decode_file(input, &ranges, &mut data, selected_folder, &mut keys, check)?;
        }
    }
    Ok(crate::streaming::Catalog {
        format: Format::SevenZip,
        members,
        offsets,
        data,
    })
}
