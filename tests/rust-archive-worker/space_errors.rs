use super::*;
use std::io::{Error, ErrorKind};

#[test]
fn storage_full_is_distinct_from_other_io_errors() {
    assert_eq!(io_error(ErrorKind::StorageFull.into()), ErrorCode::DiskFull);
    for kind in [
        ErrorKind::PermissionDenied,
        ErrorKind::Other,
        ErrorKind::NotFound,
    ] {
        assert_eq!(io_error(kind.into()), ErrorCode::Io);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn native_linux_enospc_is_disk_full() {
    assert_eq!(io_error(Error::from_raw_os_error(28)), ErrorCode::DiskFull);
    for code in [5, 13, 39, 112] {
        assert_eq!(io_error(Error::from_raw_os_error(code)), ErrorCode::Io);
    }
}

#[cfg(windows)]
#[test]
fn native_windows_disk_full_codes_are_distinct_from_access_denied() {
    for code in [39, 112] {
        assert_eq!(
            io_error(Error::from_raw_os_error(code)),
            ErrorCode::DiskFull
        );
    }
    assert_eq!(io_error(Error::from_raw_os_error(5)), ErrorCode::Io);
}
