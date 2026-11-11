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
fn ctr(bytes: &mut [u8], key: &[u8], check: &mut impl FnMut() -> Result<()>) -> Result<()> {
    let cipher = Aes256::new_from_slice(key).map_err(|_| ErrorCode::Internal)?;
    for (index, chunk) in bytes.chunks_mut(16).enumerate() {
        if index % 4096 == 0 {
            check()?;
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
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Sha1Mac> {
    let mut mac = <Sha1Mac as Mac>::new_from_slice(key).map_err(|_| ErrorCode::Internal)?;
    for chunk in bytes.chunks(65536) {
        check()?;
        mac.update(chunk);
    }
    Ok(mac)
}
pub(crate) fn aes_decrypt(
    packed: &[u8],
    password: &[u8],
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Zeroizing<Vec<u8>>> {
    if packed.len() < 28 {
        return Err(ErrorCode::Integrity);
    }
    check()?;
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
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    check()?;
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
fn random(bytes: &mut [u8]) -> Result<()> {
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
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Zeroizing<Vec<u8>>> {
    if packed.len() < 12 {
        return Err(ErrorCode::Integrity);
    }
    let mut key = Zeroizing::new([0x12345678u32, 0x23456789, 0x34567890]);
    fn update(key: &mut [u32; 3], byte: u8) {
        key[0] = (key[0] >> 8) ^ CRC_TABLE[((key[0] as u8) ^ byte) as usize];
        key[1] = key[1]
            .wrapping_add(key[0] & 255)
            .wrapping_mul(134775813)
            .wrapping_add(1);
        key[2] = (key[2] >> 8) ^ CRC_TABLE[((key[2] as u8) ^ ((key[1] >> 24) as u8)) as usize];
    }
    for &byte in password {
        update(&mut key, byte);
    }
    let mut out = Zeroizing::new(Vec::with_capacity(packed.len() - 12));
    for (index, &byte) in packed.iter().enumerate() {
        if index % 65536 == 0 {
            check()?;
        }
        let temp = key[2] | 2;
        let plain = byte ^ ((temp.wrapping_mul(temp ^ 1) >> 8) as u8);
        update(&mut key, plain);
        if index == 11 && plain != verifier {
            return Err(ErrorCode::WrongPassword);
        }
        if index >= 12 {
            out.push(plain);
        }
    }
    Ok(out)
}
