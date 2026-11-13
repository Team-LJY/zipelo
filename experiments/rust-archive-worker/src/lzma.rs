use crate::protocol::ErrorCode;
use std::io::Read;
use zeroize::Zeroize;
// 최근 8 MiB만 메모리에 두고 더 먼 사전 참조는 임시 파일에서 읽음.
struct History<'a> {
    file: &'a mut std::fs::File,
    base: u64,
    len: u64,
    cache: zeroize::Zeroizing<Vec<u8>>,
    pending: zeroize::Zeroizing<Vec<u8>>,
}
impl<'a> History<'a> {
    fn new(file: &'a mut std::fs::File, dictionary: usize) -> Result<Self> {
        use std::io::Seek;
        let base = file.stream_position().map_err(crate::space::io_error)?;
        Ok(Self {
            file,
            base,
            len: 0,
            cache: zeroize::Zeroizing::new(vec![0; dictionary.min(8 * 1024 * 1024)]),
            pending: zeroize::Zeroizing::new(Vec::with_capacity(65536)),
        })
    }
    fn len(&self) -> u64 {
        self.len
    }
    fn flush(&mut self) -> Result<()> {
        use std::io::Write;
        crate::space::ensure(self.file, self.pending.len() as u64)?;
        self.file
            .write_all(&self.pending)
            .map_err(crate::space::io_error)?;
        self.pending.as_mut_slice().zeroize();
        self.pending.clear();
        Ok(())
    }
    fn extend(&mut self, mut bytes: &[u8]) -> Result<()> {
        while !bytes.is_empty() {
            let at = (self.len % self.cache.len() as u64) as usize;
            let n = bytes
                .len()
                .min(self.cache.len() - at)
                .min(65536 - self.pending.len());
            self.cache[at..at + n].copy_from_slice(&bytes[..n]);
            self.pending.extend_from_slice(&bytes[..n]);
            self.len = self.len.checked_add(n as u64).ok_or(ErrorCode::Limit)?;
            bytes = &bytes[n..];
            if self.pending.len() == 65536 {
                self.flush()?;
            }
        }
        Ok(())
    }
    fn push(&mut self, byte: u8) -> Result<()> {
        self.extend(&[byte])
    }
    fn read_back(&mut self, distance: usize, bytes: &mut [u8]) -> Result<()> {
        use std::io::{Read, Seek, SeekFrom};
        if distance as u64 > self.len || bytes.len() > distance {
            return Err(ErrorCode::Integrity);
        }
        if distance <= self.cache.len() {
            let at = ((self.len - distance as u64) % self.cache.len() as u64) as usize;
            let n = bytes.len().min(self.cache.len() - at);
            bytes[..n].copy_from_slice(&self.cache[at..at + n]);
            let rest = bytes.len() - n;
            bytes[n..].copy_from_slice(&self.cache[..rest]);
        } else {
            self.flush()?;
            self.file
                .seek(SeekFrom::Start(self.base + self.len - distance as u64))
                .map_err(crate::space::io_error)?;
            self.file
                .read_exact(bytes)
                .map_err(crate::space::io_error)?;
            self.file
                .seek(SeekFrom::Start(self.base + self.len))
                .map_err(crate::space::io_error)?;
        }
        Ok(())
    }
    fn previous(&mut self, history: u64, distance: u32, dictionary: usize) -> Result<u8> {
        let distance = distance as usize + 1;
        if distance > dictionary || distance as u64 > self.len - history {
            return Err(ErrorCode::Integrity);
        }
        let mut value = [0];
        self.read_back(distance, &mut value)?;
        Ok(value[0])
    }
    fn copy(
        &mut self,
        history: u64,
        distance: u32,
        dictionary: usize,
        length: usize,
    ) -> Result<()> {
        let distance = distance as usize + 1;
        if distance > dictionary || distance as u64 > self.len - history {
            return Err(ErrorCode::Integrity);
        }
        let mut buffer = zeroize::Zeroizing::new([0; 273]);
        if distance == 1 {
            let byte = self.previous(history, 0, dictionary)?;
            buffer[..length].fill(byte);
            return self.extend(&buffer[..length]);
        }
        let mut remaining = length;
        while remaining != 0 {
            let n = remaining.min(distance).min(buffer.len());
            self.read_back(distance, &mut buffer[..n])?;
            self.extend(&buffer[..n])?;
            remaining -= n;
        }
        Ok(())
    }
}
type Result<T> = std::result::Result<T, ErrorCode>;
const MAX_DICT: usize = 64 * 1024 * 1024;
struct Range<'a> {
    input: &'a mut dyn Read,
    at: u64,
    length: u64,
    range: u32,
    code: u32,
}
impl<'a> Range<'a> {
    fn new(input: &'a mut dyn Read, length: u64) -> Result<Self> {
        let mut prefix = [0; 5];
        input
            .read_exact(&mut prefix)
            .map_err(|_| ErrorCode::Integrity)?;
        if length < 5 || prefix[0] != 0 {
            return Err(ErrorCode::Integrity);
        }
        Ok(Self {
            input,
            at: 5,
            length,
            range: u32::MAX,
            code: u32::from_be_bytes(prefix[1..5].try_into().unwrap()),
        })
    }
    fn normalize(&mut self) -> Result<()> {
        if self.range < 1 << 24 {
            self.range <<= 8;
            if self.at >= self.length {
                return Err(ErrorCode::Integrity);
            }
            let mut byte = [0];
            self.input
                .read_exact(&mut byte)
                .map_err(|_| ErrorCode::Integrity)?;
            self.code = (self.code << 8) | byte[0] as u32;
            self.at += 1;
        }
        Ok(())
    }
    fn bit(&mut self, probability: &mut u16) -> Result<usize> {
        let bound = (self.range >> 11) * *probability as u32;
        let bit = if self.code < bound {
            self.range = bound;
            *probability += (2048 - *probability) >> 5;
            0
        } else {
            self.range -= bound;
            self.code -= bound;
            *probability -= *probability >> 5;
            1
        };
        self.normalize()?;
        Ok(bit)
    }
    fn direct(&mut self, count: usize) -> Result<u32> {
        let mut value = 0;
        for _ in 0..count {
            self.range >>= 1;
            let bit = u32::from(self.code >= self.range);
            if bit != 0 {
                self.code -= self.range;
            }
            value = (value << 1) | bit;
            self.normalize()?;
        }
        Ok(value)
    }
    fn tree(&mut self, p: &mut [u16], bits: usize) -> Result<usize> {
        let mut symbol = 1;
        for _ in 0..bits {
            symbol = (symbol << 1) | self.bit(&mut p[symbol])?;
        }
        Ok(symbol - (1 << bits))
    }
    fn reverse(&mut self, p: &mut [u16], base: isize, bits: usize) -> Result<u32> {
        let mut symbol = 1;
        let mut value = 0;
        for i in 0..bits {
            let index =
                usize::try_from(base + symbol as isize).map_err(|_| ErrorCode::Integrity)?;
            let bit = self.bit(p.get_mut(index).ok_or(ErrorCode::Integrity)?)?;
            symbol = (symbol << 1) | bit;
            value |= (bit as u32) << i;
        }
        Ok(value)
    }
    fn finished(&self) -> bool {
        self.code == 0 && self.at == self.length
    }
}
struct Length {
    choice: [u16; 2],
    low: [[u16; 8]; 16],
    middle: [[u16; 8]; 16],
    high: [u16; 256],
}
impl Length {
    fn new() -> Self {
        Self {
            choice: [1024; 2],
            low: [[1024; 8]; 16],
            middle: [[1024; 8]; 16],
            high: [1024; 256],
        }
    }
    fn read(&mut self, r: &mut Range<'_>, position: usize) -> Result<usize> {
        if r.bit(&mut self.choice[0])? == 0 {
            r.tree(&mut self.low[position], 3)
        } else if r.bit(&mut self.choice[1])? == 0 {
            Ok(8 + r.tree(&mut self.middle[position], 3)?)
        } else {
            Ok(16 + r.tree(&mut self.high, 8)?)
        }
    }
}
struct Model {
    lc: usize,
    lp: usize,
    pb: usize,
    state: usize,
    reps: [u32; 4],
    literal: Vec<u16>,
    matches: [[u16; 16]; 12],
    rep: [u16; 12],
    g0: [u16; 12],
    g1: [u16; 12],
    g2: [u16; 12],
    short: [[u16; 16]; 12],
    slots: [[u16; 64]; 4],
    special: [u16; 114],
    align: [u16; 16],
    length: Length,
    rep_length: Length,
}
impl Model {
    fn new(property: u8) -> Result<Self> {
        if property >= 225 {
            return Err(ErrorCode::Integrity);
        }
        let lc = property as usize % 9;
        let lp = (property as usize / 9) % 5;
        let pb = property as usize / 45;
        if lc + lp > 4 {
            return Err(ErrorCode::UnsupportedFeature);
        }
        Ok(Self {
            lc,
            lp,
            pb,
            state: 0,
            reps: [0; 4],
            literal: vec![1024; 0x300 << (lc + lp)],
            matches: [[1024; 16]; 12],
            rep: [1024; 12],
            g0: [1024; 12],
            g1: [1024; 12],
            g2: [1024; 12],
            short: [[1024; 16]; 12],
            slots: [[1024; 64]; 4],
            special: [1024; 114],
            align: [1024; 16],
            length: Length::new(),
            rep_length: Length::new(),
        })
    }
    fn reset(&mut self) -> Result<()> {
        *self = Self::new(((self.pb * 5 + self.lp) * 9 + self.lc) as u8)?;
        Ok(())
    }
    fn symbol(
        &mut self,
        r: &mut Range<'_>,
        out: &mut History<'_>,
        history: u64,
        dictionary: usize,
        target: u64,
    ) -> Result<bool> {
        let pos = out.len() - history;
        let position = pos as usize & ((1 << self.pb) - 1);
        let state = self.state;
        if r.bit(&mut self.matches[state][position])? == 0 {
            if out.len() >= target {
                return Err(ErrorCode::Integrity);
            }
            let previous = if pos == 0 {
                0
            } else {
                out.previous(history, 0, dictionary)?
            };
            let context = (((pos as usize) & ((1 << self.lp) - 1)) << self.lc)
                + (previous as usize >> (8 - self.lc));
            let p = &mut self.literal[context * 0x300..(context + 1) * 0x300];
            let mut symbol = 1;
            if state >= 7 {
                let mut matched = out.previous(history, self.reps[0], dictionary)?;
                while symbol < 256 {
                    let m = (matched >> 7) as usize;
                    matched <<= 1;
                    let bit = r.bit(&mut p[((1 + m) << 8) + symbol])?;
                    symbol = (symbol << 1) | bit;
                    if bit != m {
                        break;
                    }
                }
            }
            while symbol < 256 {
                symbol = (symbol << 1) | r.bit(&mut p[symbol])?;
            }
            out.push(symbol as u8)?;
            self.state = if state < 4 {
                0
            } else if state < 10 {
                state - 3
            } else {
                state - 6
            };
            return Ok(false);
        }
        let length;
        if r.bit(&mut self.rep[state])? != 0 {
            if r.bit(&mut self.g0[state])? == 0 {
                if r.bit(&mut self.short[state][position])? == 0 {
                    self.state = if state < 7 { 9 } else { 11 };
                    if out.len() >= target {
                        return Err(ErrorCode::Integrity);
                    }
                    let byte = out.previous(history, self.reps[0], dictionary)?;
                    out.push(byte)?;
                    return Ok(false);
                }
            } else {
                let index = if r.bit(&mut self.g1[state])? == 0 {
                    1
                } else if r.bit(&mut self.g2[state])? == 0 {
                    2
                } else {
                    3
                };
                let distance = self.reps[index];
                for i in (1..=index).rev() {
                    self.reps[i] = self.reps[i - 1];
                }
                self.reps[0] = distance;
            }
            length = self.rep_length.read(r, position)? + 2;
            self.state = if state < 7 { 8 } else { 11 };
        } else {
            length = self.length.read(r, position)? + 2;
            self.state = if state < 7 { 7 } else { 10 };
            let slot = r.tree(&mut self.slots[(length - 2).min(3)], 6)?;
            let distance = if slot < 4 {
                slot as u32
            } else {
                let bits = (slot >> 1) - 1;
                let base = ((2 | (slot & 1)) as u32) << bits;
                if slot < 14 {
                    base + r.reverse(&mut self.special, base as isize - slot as isize - 1, bits)?
                } else {
                    base + (r.direct(bits - 4)? << 4) + r.reverse(&mut self.align, 0, 4)?
                }
            };
            if distance == u32::MAX {
                if length != 2 {
                    return Err(ErrorCode::Integrity);
                }
                return Ok(true);
            }
            self.reps.rotate_right(1);
            self.reps[0] = distance;
        }
        if out
            .len()
            .checked_add(length as u64)
            .is_none_or(|n| n > target)
        {
            return Err(ErrorCode::Integrity);
        }
        out.copy(history, self.reps[0], dictionary, length)?;
        Ok(false)
    }
    fn decode(
        &mut self,
        input: &mut dyn Read,
        length: u64,
        out: &mut History<'_>,
        history: u64,
        dictionary: usize,
        target: u64,
        eos: bool,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<()> {
        let mut range = Range::new(input, length)?;
        let mut next_check = out.len();
        while out.len() < target {
            if out.len() >= next_check {
                check.check()?;
                if out.len() > next_check.saturating_sub(65536) {
                    crate::resources::completed(Ok(()), check)?;
                }
                check.observe(
                    crate::protocol::ResourceUsage {
                        actual_bytes: Some(
                            out.base.checked_add(out.len()).ok_or(ErrorCode::Limit)?,
                        ),
                        ..crate::protocol::ResourceUsage::default()
                    },
                    Some(out.file),
                )?;
                next_check = out.len() + 65536;
            }
            if self.symbol(&mut range, out, history, dictionary, target)? {
                return Err(ErrorCode::Integrity);
            }
        }
        if !range.finished() {
            if !eos
                || !self.symbol(&mut range, out, history, dictionary, target)?
                || !range.finished()
            {
                return Err(ErrorCode::Integrity);
            }
        }
        Ok(())
    }
}
fn dictionary(size: u64) -> Result<usize> {
    if size > MAX_DICT as u64 {
        return Err(ErrorCode::Limit);
    }
    Ok((size as usize).max(4096))
}
pub(crate) fn decode_stream(
    input: &mut impl std::io::Read,
    length: u64,
    properties: &[u8],
    lzma2: bool,
    size: u64,
    output: &mut std::fs::File,
    check: &mut impl crate::resources::Monitor,
) -> Result<()> {
    use std::io::{BufReader, Read};
    crate::space::ensure(output, size)?;
    let dict = if !lzma2 {
        if properties.len() != 5 {
            return Err(ErrorCode::Integrity);
        }
        dictionary(u32::from_le_bytes(properties[1..5].try_into().unwrap()) as u64)?
    } else {
        if properties.len() != 1 || properties[0] > 40 {
            return Err(ErrorCode::Integrity);
        }
        let prop = properties[0] as u32;
        dictionary(if prop == 40 {
            u32::MAX as u64
        } else {
            ((2 | (prop & 1)) as u64) << (prop / 2 + 11)
        })?
    };
    let mut out = History::new(output, dict)?;
    let mut reader = BufReader::with_capacity(65536, input.take(length));
    if !lzma2 {
        Model::new(properties[0])?.decode(
            &mut reader,
            length,
            &mut out,
            0,
            dict,
            size,
            true,
            check,
        )?;
        return out.flush();
    }
    let byte = |r: &mut dyn Read| -> Result<u8> {
        let mut b = [0];
        r.read_exact(&mut b).map_err(|_| ErrorCode::Integrity)?;
        Ok(b[0])
    };
    let word = |r: &mut dyn Read| -> Result<usize> {
        let mut b = [0; 2];
        r.read_exact(&mut b).map_err(|_| ErrorCode::Integrity)?;
        Ok(u16::from_be_bytes(b) as usize + 1)
    };
    let mut history = 0;
    let mut have_dictionary = false;
    let mut need_state = true;
    let mut model: Option<Model> = None;
    loop {
        check.check()?;
        let control = byte(&mut reader)?;
        crate::resources::completed(Ok(()), check)?;
        if control == 0 {
            let mut extra = [0];
            if reader.read(&mut extra).map_err(crate::space::io_error)? != 0 || out.len() != size {
                return Err(ErrorCode::Integrity);
            }
            return out.flush();
        }
        if control == 1 || control >= 0xe0 {
            history = out.len();
            have_dictionary = true;
            model = None;
        }
        if !have_dictionary {
            return Err(ErrorCode::Integrity);
        }
        let unpacked = word(&mut reader)?;
        if control <= 2 {
            let target = out
                .len()
                .checked_add(unpacked as u64)
                .filter(|n| *n <= size)
                .ok_or(ErrorCode::Integrity)?;
            let mut buffer = zeroize::Zeroizing::new([0; 65536]);
            reader
                .read_exact(&mut buffer[..unpacked])
                .map_err(|_| ErrorCode::Integrity)?;
            out.extend(&buffer[..unpacked])?;
            debug_assert_eq!(out.len(), target);
            need_state = true;
        } else if control >= 0x80 {
            let unpacked = unpacked + ((control as usize & 31) << 16);
            let packed = word(&mut reader)? as u64;
            if control >= 0xc0 {
                model = Some(Model::new(byte(&mut reader)?)?);
            }
            let model = model.as_mut().ok_or(ErrorCode::Integrity)?;
            if control >= 0xa0 {
                model.reset()?;
            } else if need_state {
                return Err(ErrorCode::Integrity);
            }
            let target = out
                .len()
                .checked_add(unpacked as u64)
                .filter(|n| *n <= size)
                .ok_or(ErrorCode::Integrity)?;
            model.decode(
                &mut (&mut reader).take(packed),
                packed,
                &mut out,
                history,
                dict,
                target,
                false,
                check,
            )?;
            need_state = false;
        } else {
            return Err(ErrorCode::Integrity);
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/rust-archive-worker/lzma_streaming.rs"]
mod tests;
