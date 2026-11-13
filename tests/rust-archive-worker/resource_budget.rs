use super::*;

fn budget() -> Budget {
    Budget {
        soft: Limits::default(),
        ceiling: Limits {
            input: u64::MAX,
            output: 64 * 1024 * 1024 * 1024,
            entries: 200_000,
        },
        granted: ResourceScope::default(),
    }
}
#[test]
fn output_boundaries_and_absolute_ceiling() {
    let mut b = budget();
    let soft = b.soft.output;
    for n in [soft - 1, soft] {
        assert!(
            !b.required(ResourceUsage {
                actual_bytes: Some(n),
                ..ResourceUsage::default()
            })
            .unwrap()
            .any()
        );
    }
    let usage = ResourceUsage {
        actual_bytes: Some(soft + 1),
        ..ResourceUsage::default()
    };
    let scope = b.required(usage).unwrap();
    assert_eq!(
        scope,
        ResourceScope {
            output: true,
            ..ResourceScope::default()
        }
    );
    b.grant(scope);
    assert!(!b.required(usage).unwrap().any());
    assert!(
        !b.required(ResourceUsage {
            actual_bytes: Some(b.ceiling.output),
            ..ResourceUsage::default()
        })
        .unwrap()
        .any()
    );
    assert_eq!(
        b.required(ResourceUsage {
            actual_bytes: Some(b.ceiling.output + 1),
            ..ResourceUsage::default()
        }),
        Err(ErrorCode::Limit)
    );
}
#[test]
fn entries_need_their_own_grant_and_keep_absolute_ceiling() {
    let mut b = budget();
    b.grant(ResourceScope {
        output: true,
        ..ResourceScope::default()
    });
    assert!(
        !b.required(ResourceUsage {
            estimated_entries: Some(10_000),
            ..ResourceUsage::default()
        })
        .unwrap()
        .any()
    );
    let usage = ResourceUsage {
        estimated_entries: Some(10_001),
        ..ResourceUsage::default()
    };
    assert!(b.required(usage).unwrap().entries);
    b.grant(ResourceScope {
        entries: true,
        ..ResourceScope::default()
    });
    assert!(
        !b.required(ResourceUsage {
            estimated_entries: Some(200_000),
            ..ResourceUsage::default()
        })
        .unwrap()
        .any()
    );
    assert_eq!(
        b.required(ResourceUsage {
            actual_entries: Some(200_001),
            ..ResourceUsage::default()
        }),
        Err(ErrorCode::Limit)
    );
}
#[test]
fn ratio_floor_boundary_unknown_and_zero_input() {
    let b = budget();
    let usage = |output, input| ResourceUsage {
        ratio_output_bytes: Some(output),
        compressed_bytes: input,
        ..ResourceUsage::default()
    };
    assert!(
        !b.required(usage(256 * 1024 * 1024 - 1, Some(1)))
            .unwrap()
            .ratio
    );
    assert!(!b.required(usage(300_000_000, Some(300_000))).unwrap().ratio);
    assert!(b.required(usage(300_000_001, Some(300_000))).unwrap().ratio);
    assert!(b.required(usage(256 * 1024 * 1024, Some(0))).unwrap().ratio);
    assert!(!b.required(usage(256 * 1024 * 1024, None)).unwrap().ratio);
    assert!(!b.required(ResourceUsage::default()).unwrap().any());
}
#[test]
fn estimated_and_actual_totals_are_both_enforced() {
    let b = budget();
    for usage in [
        ResourceUsage {
            estimated_bytes: Some(b.soft.output + 1),
            actual_bytes: Some(0),
            ..ResourceUsage::default()
        },
        ResourceUsage {
            estimated_bytes: Some(0),
            actual_bytes: Some(b.soft.output + 1),
            ..ResourceUsage::default()
        },
    ] {
        assert!(b.required(usage).unwrap().output);
    }
}
#[test]
fn idle_timeout_and_activity_use_virtual_time() {
    let mut w = Watchdog::new();
    assert!(w.check(Duration::from_secs(300)).is_ok());
    assert_eq!(w.check(Duration::from_secs(301)), Err(ErrorCode::Limit));
    w.resume(Duration::from_secs(299), true);
    assert!(w.check(Duration::from_secs(358)).is_ok());
    assert_eq!(w.check(Duration::from_secs(359)), Err(ErrorCode::Limit));
    w.activity(Duration::from_secs(350)).unwrap();
    assert!(w.check(Duration::from_secs(409)).is_ok());
    assert_eq!(w.check(Duration::from_secs(410)), Err(ErrorCode::Limit));
}
#[test]
fn approved_progress_can_run_longer_than_five_minutes() {
    let mut w = Watchdog::new();
    w.resume(Duration::ZERO, true);
    for t in (1..=3600).step_by(30) {
        w.activity(Duration::from_secs(t)).unwrap();
    }
    assert!(w.check(Duration::from_secs(3600)).is_ok());
    w.resume(Duration::from_secs(3900), false);
    assert!(w.check(Duration::from_secs(3959)).is_ok());
    assert_eq!(w.check(Duration::from_secs(3960)), Err(ErrorCode::Limit));
}

#[test]
fn granted_entry_policy_keeps_independent_metadata_budget() {
    let mut b = budget();
    b.grant(ResourceScope {
        entries: true,
        ..ResourceScope::default()
    });
    assert!(
        !b.required(ResourceUsage {
            estimated_entries: Some(200_000),
            ..ResourceUsage::default()
        })
        .unwrap()
        .any()
    );
    let mut metadata = crate::archive::MetadataBudget::default();
    for _ in 0..32768 {
        metadata.charge("").unwrap();
    }
    assert_eq!(metadata.charge(""), Err(ErrorCode::Limit));
}
