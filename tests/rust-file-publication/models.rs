use publication::{
    Cancellation, Cleanup, CloseFailure, FileCommitReceipt, Publication, Report, Resource,
};
use std::io;

#[test]
fn constructed_cleanup_failures_are_never_all_successful() {
    let cleanup = Cleanup {
        attempted: 2,
        failures: [
            Some(CloseFailure {
                resource: Resource::TemporaryFile,
                source: io::Error::from_raw_os_error(libc::EIO),
            }),
            Some(CloseFailure {
                resource: Resource::ParentDirectory,
                source: io::Error::from_raw_os_error(libc::EINTR),
            }),
            None,
            None,
        ],
    };
    assert!(!cleanup.all_close_calls_succeeded());
    assert_eq!(cleanup.attempted, 2);
    assert_eq!(
        cleanup.failures[0].as_ref().unwrap().resource,
        Resource::TemporaryFile
    );
    assert_eq!(
        cleanup.failures[0].as_ref().unwrap().source.raw_os_error(),
        Some(libc::EIO)
    );
    assert_eq!(
        cleanup.failures[1].as_ref().unwrap().source.raw_os_error(),
        Some(libc::EINTR)
    );
    let empty = Cleanup {
        attempted: 0,
        failures: [None, None, None, None],
    };
    assert!(empty.all_close_calls_succeeded());
    assert_eq!(empty.attempted, 0);
}

#[test]
fn public_types_support_transfer_without_forging_receipts() {
    fn send<T: Send>() {}
    send::<Publication>();
    send::<Cancellation>();
    send::<Report<()>>();
    send::<FileCommitReceipt>();
}
