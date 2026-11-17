use crate::protocol::ErrorCode;
use aes::{
    Aes256,
    cipher::{BlockEncrypt, KeyInit},
};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

type Result<T> = std::result::Result<T, ErrorCode>;
type Sha1Mac = Hmac<Sha1>;

fn keys(password: &[u8], salt: &[u8]) -> Zeroizing<[u8; 66]> {
    let mut result = Zeroizing::new([0; 66]);
    pbkdf2::pbkdf2_hmac::<Sha1>(password, salt, 1000, result.as_mut());
    result
}
fn ctr(bytes: &mut [u8], key: &[u8], check: &mut impl crate::resources::Monitor) -> Result<()> {
    let cipher = Aes256::new_from_slice(key).map_err(|_| ErrorCode::Internal)?;
    for (index, chunk) in bytes.chunks_mut(16).enumerate() {
        if index % 4096 == 0 {
            check.check()?;
        }
        // WinZip은 1부터 시작하는 리틀엔디언 카운터를 사용함.
        let mut block = ((index as u128) + 1).to_le_bytes().into();
        cipher.encrypt_block(&mut block);
        for (byte, mask) in chunk.iter_mut().zip(block.iter()) {
            *byte ^= mask;
        }
        block.as_mut_slice().zeroize();
    }
    Ok(())
}
fn authenticate(
    key: &[u8],
    bytes: &[u8],
    check: &mut impl crate::resources::Monitor,
) -> Result<Sha1Mac> {
    let mut mac = <Sha1Mac as Mac>::new_from_slice(key).map_err(|_| ErrorCode::Internal)?;
    for chunk in bytes.chunks(65536) {
        check.check()?;
        mac.update(chunk);
    }
    Ok(mac)
}
pub(crate) fn aes_encrypt(
    packed: &[u8],
    password: &[u8],
    check: &mut impl crate::resources::Monitor,
) -> Result<Vec<u8>> {
    check.check()?;
    let mut salt = [0; 16];
    random(&mut salt)?;
    let key = keys(password, &salt);
    let mut bytes = packed.to_vec();
    ctr(&mut bytes, &key[..32], check)?;
    let mac = authenticate(&key[32..64], &bytes, check)?
        .finalize()
        .into_bytes();
    let mut result = Vec::with_capacity(bytes.len() + 28);
    result.extend(salt);
    result.extend(&key[64..]);
    result.extend(bytes);
    result.extend(&mac[..10]);
    Ok(result)
}
pub(crate) fn random(bytes: &mut [u8]) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(bytes))
            .map_err(|_| ErrorCode::Io)
    }
    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(
                algorithm: *mut core::ffi::c_void,
                buffer: *mut u8,
                length: u32,
                flags: u32,
            ) -> i32;
        }
        let len = u32::try_from(bytes.len()).map_err(|_| ErrorCode::Limit)?;
        if unsafe { BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), len, 2) } >= 0 {
            Ok(())
        } else {
            Err(ErrorCode::Io)
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = bytes;
        Err(ErrorCode::UnsupportedFeature)
    }
}
const CRC_TABLE: [u32; 256] = {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};
pub(crate) enum Decryptor {
    Plain,
    Zip(Zeroizing<[u32; 3]>),
    Aes {
        cipher: Aes256,
        counter: u128,
        mask: [u8; 16],
        used: usize,
    },
}
fn zip_update(key: &mut [u32; 3], byte: u8) {
    key[0] = (key[0] >> 8) ^ CRC_TABLE[((key[0] as u8) ^ byte) as usize];
    key[1] = key[1]
        .wrapping_add(key[0] & 255)
        .wrapping_mul(134775813)
        .wrapping_add(1);
    key[2] = (key[2] >> 8) ^ CRC_TABLE[((key[2] as u8) ^ ((key[1] >> 24) as u8)) as usize];
}
impl Decryptor {
    pub fn prepare(
        file: &mut std::fs::File,
        offset: u64,
        length: u64,
        password: &[u8],
        aes: bool,
        verifier: u8,
        check: &mut impl crate::resources::Monitor,
    ) -> Result<(Self, u64, u64)> {
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| ErrorCode::Io)?;
        if aes {
            if length < 28 {
                return Err(ErrorCode::Integrity);
            }
            let mut header = [0; 18];
            file.read_exact(&mut header)
                .map_err(|_| ErrorCode::Integrity)?;
            let key = keys(password, &header[..16]);
            if !bool::from(key[64..].ct_eq(&header[16..])) {
                return Err(ErrorCode::WrongPassword);
            }
            let mut mac =
                <Sha1Mac as Mac>::new_from_slice(&key[32..64]).map_err(|_| ErrorCode::Internal)?;
            let mut remaining = length - 28;
            let mut buffer = [0; 65536];
            while remaining != 0 {
                check.check()?;
                let n = remaining.min(buffer.len() as u64) as usize;
                file.read_exact(&mut buffer[..n])
                    .map_err(|_| ErrorCode::Integrity)?;
                mac.update(&buffer[..n]);
                remaining -= n as u64;
                crate::resources::completed(Ok(()), check)?;
            }
            let mut tag = [0; 10];
            file.read_exact(&mut tag)
                .map_err(|_| ErrorCode::Integrity)?;
            mac.verify_truncated_left(&tag)
                .map_err(|_| ErrorCode::Integrity)?;
            let cipher = Aes256::new_from_slice(&key[..32]).map_err(|_| ErrorCode::Internal)?;
            Ok((
                Self::Aes {
                    cipher,
                    counter: 1,
                    mask: [0; 16],
                    used: 16,
                },
                offset + 18,
                length - 28,
            ))
        } else {
            if length < 12 {
                return Err(ErrorCode::Integrity);
            }
            let mut key = Zeroizing::new([0x12345678, 0x23456789, 0x34567890]);
            for &b in password {
                zip_update(&mut key, b);
            }
            let mut decoder = Self::Zip(key);
            let mut header = [0; 12];
            file.read_exact(&mut header)
                .map_err(|_| ErrorCode::Integrity)?;
            decoder.apply(&mut header);
            if header[11] != verifier {
                return Err(ErrorCode::WrongPassword);
            }
            header.zeroize();
            Ok((decoder, offset + 12, length - 12))
        }
    }
    pub fn apply(&mut self, bytes: &mut [u8]) {
        match self {
            Self::Plain => {}
            Self::Zip(key) => {
                for byte in bytes {
                    let temp = key[2] | 2;
                    *byte ^= (temp.wrapping_mul(temp ^ 1) >> 8) as u8;
                    zip_update(key, *byte);
                }
            }
            Self::Aes {
                cipher,
                counter,
                mask,
                used,
            } => {
                for byte in bytes {
                    if *used == 16 {
                        let mut block = counter.to_le_bytes().into();
                        cipher.encrypt_block(&mut block);
                        mask.copy_from_slice(&block);
                        block.as_mut_slice().zeroize();
                        *counter += 1;
                        *used = 0;
                    }
                    *byte ^= mask[*used];
                    *used += 1;
                }
            }
        }
    }
}
impl Drop for Decryptor {
    fn drop(&mut self) {
        if let Self::Aes { mask, counter, .. } = self {
            mask.zeroize();
            counter.zeroize();
        }
    }
}

pub(crate) fn aes_encrypt_file(
    input: &mut std::fs::File,
    length: u64,
    output: &mut std::fs::File,
    password: &[u8],
    check: &mut impl crate::resources::Monitor,
) -> Result<()> {
    use std::io::{Read, Write};
    check.check()?;
    let mut salt = [0; 16];
    random(&mut salt)?;
    let key = keys(password, &salt);
    output.write_all(&salt).map_err(crate::space::io_error)?;
    output
        .write_all(&key[64..])
        .map_err(crate::space::io_error)?;
    let mut cipher = Decryptor::Aes {
        cipher: Aes256::new_from_slice(&key[..32]).map_err(|_| ErrorCode::Internal)?,
        counter: 1,
        mask: [0; 16],
        used: 16,
    };
    let mut mac =
        <Sha1Mac as Mac>::new_from_slice(&key[32..64]).map_err(|_| ErrorCode::Internal)?;
    let mut remaining = length;
    let mut buffer = Zeroizing::new([0; 65536]);
    while remaining != 0 {
        check.check()?;
        let n = remaining.min(buffer.len() as u64) as usize;
        input
            .read_exact(&mut buffer[..n])
            .map_err(crate::space::io_error)?;
        cipher.apply(&mut buffer[..n]);
        mac.update(&buffer[..n]);
        crate::space::ensure(output, n as u64)?;
        output
            .write_all(&buffer[..n])
            .map_err(crate::space::io_error)?;
        remaining -= n as u64;
        crate::resources::completed(Ok(()), check)?;
    }
    output
        .write_all(&mac.finalize().into_bytes()[..10])
        .map_err(crate::space::io_error)?;
    Ok(())
}

/// 7-Zip AES 키 유도의 반복 상한(2^24회). 7-Zip 기본값은 2^19회이며, 더 큰 값은
/// 키 유도만으로 오래 걸리게 만드는 입력으로 보고 거부함.
const SEVEN_ZIP_MAX_CYCLES: u8 = 24;

/// 7zAES 코더 속성: 반복 지수, 솔트와 CBC 초기 벡터.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SevenZipAes {
    cycles: u8,
    salt: Vec<u8>,
    iv: [u8; 16],
}
impl SevenZipAes {
    pub fn parse(properties: &[u8]) -> Result<Self> {
        let first = *properties.first().ok_or(ErrorCode::Integrity)?;
        let cycles = first & 0x3f;
        let (salt, iv) = if first & 0xc0 == 0 {
            if properties.len() != 1 {
                return Err(ErrorCode::Integrity);
            }
            (&[][..], &[][..])
        } else {
            let second = *properties.get(1).ok_or(ErrorCode::Integrity)?;
            let salt = ((first >> 7) & 1) as usize + (second >> 4) as usize;
            let iv = ((first >> 6) & 1) as usize + (second & 15) as usize;
            if properties.len() != 2 + salt + iv {
                return Err(ErrorCode::Integrity);
            }
            (&properties[2..2 + salt], &properties[2 + salt..])
        };
        // 0x3f는 해시 없이 암호를 키로 쓰는 방식이므로 반복 상한과 함께 거부함.
        if cycles > SEVEN_ZIP_MAX_CYCLES {
            return Err(ErrorCode::UnsupportedFeature);
        }
        let mut padded = [0; 16];
        padded[..iv.len()].copy_from_slice(iv);
        Ok(Self {
            cycles,
            salt: salt.to_vec(),
            iv: padded,
        })
    }
    /// 같은 키를 유도하는 속성인지. 초기 벡터는 폴더마다 다를 수 있음.
    pub fn same_key(&self, other: &Self) -> bool {
        self.cycles == other.cycles && self.salt == other.salt
    }
    /// SHA-256(솔트 || UTF-16LE 암호 || 8바이트 카운터)를 2^cycles회 이어 계산함.
    pub fn key(
        &self,
        password: &[u8],
        check: &mut impl crate::resources::Monitor,
    ) -> Result<Zeroizing<[u8; 32]>> {
        use sha2::{Digest, Sha256};
        let text = std::str::from_utf8(password).map_err(|_| ErrorCode::WrongPassword)?;
        let mut block = Zeroizing::new(Vec::with_capacity(self.salt.len() + text.len() * 4 + 8));
        block.extend_from_slice(&self.salt);
        for unit in text.encode_utf16() {
            block.extend_from_slice(&unit.to_le_bytes());
        }
        let counter_at = block.len();
        block.extend_from_slice(&[0; 8]);
        let mut hash = Sha256::new();
        for round in 0..1u64 << self.cycles {
            if round % 65536 == 0 {
                crate::resources::completed(Ok(()), check)?;
            }
            block[counter_at..].copy_from_slice(&round.to_le_bytes());
            hash.update(&block[..]);
        }
        let mut key = Zeroizing::new([0; 32]);
        key.copy_from_slice(&hash.finalize());
        Ok(key)
    }
}

/// 7zAES(AES-256-CBC) 묶음 스트림을 읽으며 복호화함. 마지막 블록의 채움 바이트는
/// 코더 출력 크기에서 잘라 냄.
pub(crate) struct SevenZipReader<R> {
    input: R,
    cipher: Aes256,
    previous: [u8; 16],
    buffer: Zeroizing<[u8; 4096]>,
    start: usize,
    end: usize,
    remaining: u64,
}
impl<R: std::io::Read> SevenZipReader<R> {
    /// `packed`는 암호문 크기, `size`는 복호화 뒤 다음 코더에 넘길 크기임.
    pub fn new(
        input: R,
        packed: u64,
        size: u64,
        properties: &SevenZipAes,
        key: &[u8; 32],
    ) -> Result<Self> {
        if packed % 16 != 0 || size > packed {
            return Err(ErrorCode::Integrity);
        }
        Ok(Self {
            input,
            cipher: Aes256::new_from_slice(key).map_err(|_| ErrorCode::Internal)?,
            previous: properties.iv,
            buffer: Zeroizing::new([0; 4096]),
            start: 0,
            end: 0,
            remaining: size,
        })
    }
}
impl<R: std::io::Read> std::io::Read for SevenZipReader<R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        use aes::cipher::BlockDecrypt;
        if self.remaining == 0 || output.is_empty() {
            return Ok(0);
        }
        if self.start == self.end {
            let wanted = self.remaining.min(self.buffer.len() as u64) as usize;
            let length = wanted.div_ceil(16) * 16;
            let mut filled = 0;
            while filled < length {
                match self.input.read(&mut self.buffer[filled..length]) {
                    Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                    Ok(n) => filled += n,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
            for chunk in self.buffer[..length].chunks_exact_mut(16) {
                let mut block = aes::Block::clone_from_slice(chunk);
                let cipher_text: [u8; 16] = block.into();
                self.cipher.decrypt_block(&mut block);
                for ((byte, plain), mask) in chunk.iter_mut().zip(block.iter()).zip(&self.previous)
                {
                    *byte = plain ^ mask;
                }
                block.as_mut_slice().zeroize();
                self.previous = cipher_text;
            }
            self.start = 0;
            self.end = wanted;
        }
        let n = output.len().min(self.end - self.start);
        output[..n].copy_from_slice(&self.buffer[self.start..self.start + n]);
        self.start += n;
        self.remaining -= n as u64;
        Ok(n)
    }
}
impl<R> Drop for SevenZipReader<R> {
    fn drop(&mut self) {
        self.previous.zeroize();
    }
}
