use sha2::{Digest, Sha256};

pub const WORKER_SOURCE: &str = "9626abbf22f3a4c66a996e64601694ed5fe98763";
pub const APPROVED_SHA256: [u8; 32] = [
    0x62, 0xe1, 0x57, 0xe3, 0x04, 0xb0, 0x1c, 0xac, 0x7c, 0x99, 0xa8, 0xeb, 0x60, 0xa1, 0x47, 0x06,
    0x68, 0x77, 0xcf, 0x14, 0x1b, 0x03, 0x14, 0xdb, 0x5c, 0xf1, 0xa7, 0x6a, 0xab, 0x55, 0xa5, 0xf7,
];
pub const MAX_IMAGE: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageDecision {
    TooLarge,
    Empty,
    DigestMismatch,
    RequiresSealedFdAndOsLauncher,
}

// 메모리 바이트 비교만 수행함. 봉인이나 실행 identity 증명이 아님.
pub fn inspect_image_candidate(bytes: &[u8]) -> ImageDecision {
    if bytes.is_empty() {
        return ImageDecision::Empty;
    }
    if bytes.len() > MAX_IMAGE {
        return ImageDecision::TooLarge;
    }
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    if digest != APPROVED_SHA256 {
        ImageDecision::DigestMismatch
    } else {
        ImageDecision::RequiresSealedFdAndOsLauncher
    }
}

pub fn required_seals() -> libc::c_int {
    libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL
}

pub fn required_memfd_flags() -> libc::c_uint {
    libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING
}

pub fn required_exec_flags() -> libc::c_int {
    libc::AT_EMPTY_PATH
}

// pathname 실행 대체 경로와 외부 lifecycle 증명 옵션은 제공하지 않음.
pub fn live_launch_available() -> bool {
    false
}
