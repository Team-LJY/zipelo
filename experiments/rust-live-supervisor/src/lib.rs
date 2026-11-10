#![forbid(unsafe_code)]

#[cfg(not(target_os = "linux"))]
compile_error!("This candidate is Linux-only.");

pub mod launch;
pub mod range;
pub mod reactor;

pub const LIVE_SUPERVISOR_IMPLEMENTED: bool = false;
