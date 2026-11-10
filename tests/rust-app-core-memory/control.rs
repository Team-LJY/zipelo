use app_core::Operation;

#[test]
fn ids_are_nonzero_distinct_and_bound_to_handles() {
    let (first, first_cancel) = Operation::start().unwrap();
    let (second, second_cancel) = Operation::start().unwrap();
    assert_ne!(first.id(), second.id());
    assert_ne!(first.id().value(), 0);
    assert_ne!(second.id().value(), 0);
    assert_eq!(first.id(), first_cancel.operation());
    assert_eq!(second.id(), second_cancel.operation());
}

#[test]
fn cancellation_is_shared_monotonic_and_operation_local() {
    let (_, first) = Operation::start().unwrap();
    let clone = first.clone();
    let (_, second) = Operation::start().unwrap();
    assert!(!first.is_requested());
    assert!(clone.request());
    assert!(first.is_requested());
    assert!(!first.request());
    assert!(!clone.request());
    assert!(!second.is_requested());
}
