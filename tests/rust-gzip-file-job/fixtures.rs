pub fn pattern(length: usize) -> Vec<u8> {
    (0..length).map(|i| ((i * 37 + 11) % 251) as u8).collect()
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut value = !0_u32;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(value & 1));
        }
    }
    !value
}

pub fn stored(bytes: &[u8]) -> Vec<u8> {
    assert!(bytes.len() <= 65_537);
    let mut archive = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    let blocks = bytes.len().max(1).div_ceil(65_535);
    for index in 0..blocks {
        let start = index * 65_535;
        let end = (start + 65_535).min(bytes.len());
        let length = (end - start) as u16;
        archive.push(u8::from(index + 1 == blocks));
        archive.extend_from_slice(&length.to_le_bytes());
        archive.extend_from_slice(&(!length).to_le_bytes());
        archive.extend_from_slice(&bytes[start..end]);
    }
    archive.extend_from_slice(&crc32(bytes).to_le_bytes());
    archive.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    archive
}

pub fn named(bytes: &[u8]) -> Vec<u8> {
    let mut archive = stored(bytes);
    archive[3] = 8 | 16;
    archive.splice(10..10, b"../header-name\0../comment-name\0".iter().copied());
    archive
}
