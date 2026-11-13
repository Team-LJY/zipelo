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
pub(crate) fn aes_decrypt(
    packed: &[u8],
    password: &[u8],
    check: &mut impl crate::resources::Monitor,
) -> Result<Zeroizing<Vec<u8>>> {
    if packed.len() < 28 {
        return Err(ErrorCode::Integrity);
    }
    check.check()?;
    let key = keys(password, &packed[..16]);
    if !bool::from(key[64..].ct_eq(&packed[16..18])) {
        return Err(ErrorCode::WrongPassword);
    }
    let end = packed.len() - 10;
    // 인증이 성공하기 전에는 복호화나 압축 해제를 시작하지 않음.
    authenticate(&key[32..64], &packed[18..end], check)?
        .verify_truncated_left(&packed[end..])
        .map_err(|_| ErrorCode::Integrity)?;
    let mut bytes = Zeroizing::new(packed[18..end].to_vec());
    ctr(&mut bytes, &key[..32], check)?;
    Ok(bytes)
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
pub(crate) fn zipcrypto_decrypt(
    packed: &[u8],
    password: &[u8],
    verifier: u8,
    check: &mut impl crate::resources::Monitor,
) -> Result<Zeroizing<Vec<u8>>> {
    if packed.len() < 12 {
        return Err(ErrorCode::Integrity);
    }
    let mut key = Zeroizing::new([0x12345678u32, 0x23456789, 0x34567890]);
    for &byte in password {
        zip_update(&mut key, byte);
    }
    let mut out = Zeroizing::new(Vec::with_capacity(packed.len() - 12));
    for (index, &byte) in packed.iter().enumerate() {
        if index % 65536 == 0 {
            check.check()?;
        }
        let temp = key[2] | 2;
        let plain = byte ^ ((temp.wrapping_mul(temp ^ 1) >> 8) as u8);
        zip_update(&mut key, plain);
        if index == 11 && plain != verifier {
            return Err(ErrorCode::WrongPassword);
        }
        if index >= 12 {
            out.push(plain);
        }
    }
    Ok(out)
}

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
                check.activity()?;
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
        check.activity()?;
    }
    output
        .write_all(&mac.finalize().into_bytes()[..10])
        .map_err(crate::space::io_error)?;
    Ok(())
}
