use super::*;
use crate::resources::{Budget, Monitor};

struct Policy(MetadataLimits);
impl Monitor for Policy {
    fn check(&mut self) -> Result<()> {
        Ok(())
    }
    fn metadata_limits(&self) -> MetadataLimits {
        self.0
    }
}
fn approved(bytes: usize, nodes: usize) -> MetadataLimits {
    MetadataLimits {
        bytes,
        nodes,
        ..MetadataLimits::approved()
    }
}
fn member(name: &str) -> Member {
    Member {
        entry: Entry {
            entry_id: 0,
            name: name.into(),
            kind: EntryKind::File,
            size: 0,
            compressed_size: 0,
            encrypted: false,
        },
        data: Vec::new(),
    }
}

#[test]
fn only_a_grant_selects_expanded_metadata_policy() {
    let soft = Limits::default();
    let mut b = Budget {
        soft,
        ceiling: Limits {
            entries: 200_000,
            output: 64 * 1024 * 1024 * 1024,
            ..soft
        },
        granted: ResourceScope::default(),
    };
    assert_eq!(b.metadata_limits(), MetadataLimits::default());
    for scope in [
        ResourceScope {
            output: true,
            ..ResourceScope::default()
        },
        ResourceScope {
            entries: true,
            ..ResourceScope::default()
        },
        ResourceScope {
            ratio: true,
            ..ResourceScope::default()
        },
    ] {
        b.granted = ResourceScope::default();
        b.grant(scope);
        assert_eq!(b.metadata_limits(), MetadataLimits::approved());
    }
    assert_eq!(MetadataLimits::default().bytes, 8 * 1024 * 1024);
    assert_eq!(MetadataLimits::default().nodes, 32_768);
    assert_eq!(MetadataLimits::approved().bytes, 64 * 1024 * 1024);
    assert_eq!(MetadataLimits::approved().nodes, 262_144);
    assert_eq!(
        Policy(MetadataLimits::default()).metadata_limits(),
        MetadataLimits::default()
    );
    assert_eq!((|| Ok(())).metadata_limits(), MetadataLimits::default());
}

#[test]
fn scaled_byte_and_node_caps_are_independent_and_atomic() {
    let limits = approved(524, 2);
    let mut b = MetadataBudget::default();
    b.charge_with("a", limits).unwrap();
    b.charge_with("bb", limits).unwrap();
    assert_eq!((b.bytes, b.nodes), (524, 2));
    assert_eq!(b.charge_with("", limits), Err(ErrorCode::Limit));
    assert_eq!((b.bytes, b.nodes), (524, 2));
    let mut b = MetadataBudget::default();
    b.charge_with("a", approved(1000, 1)).unwrap();
    assert_eq!(b.charge_with("b", approved(1000, 1)), Err(ErrorCode::Limit));
    assert_eq!(b.charge_with("aa", approved(263, 2)), Err(ErrorCode::Limit));
    assert_eq!((b.bytes, b.nodes), (260, 1));
}

#[test]
fn approved_source_path_copies_consume_bytes_without_duplicate_nodes() {
    let mut b = MetadataBudget::default();
    let limits = approved(300, 1);
    b.charge_with("a", limits).unwrap();
    b.charge_copy("/source/a", limits).unwrap();
    b.charge_copy("a", limits).unwrap();
    assert_eq!((b.bytes, b.nodes), (300, 1));
    assert_eq!(b.charge_copy("b", limits), Err(ErrorCode::Limit));
    let mut legacy = MetadataBudget::default();
    legacy.charge_with("a", MetadataLimits::default()).unwrap();
    legacy
        .charge_copy("/source/a", MetadataLimits::default())
        .unwrap();
    assert_eq!(legacy.nodes, 2);
}

#[test]
fn directory_planning_counts_implicit_parents_in_its_budget() {
    let members = vec![member("a/b")];
    assert_eq!(
        output_directories(&members, &mut Policy(approved(528, 2)))
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        vec!["a"]
    );
    assert_eq!(
        output_directories(&members, &mut Policy(approved(527, 2))),
        Err(ErrorCode::Limit)
    );
    assert_eq!(
        output_directories(&members, &mut Policy(approved(528, 1))),
        Err(ErrorCode::Limit)
    );
    let conflict = vec![member("a"), member("a/b")];
    assert_eq!(
        validate_members_with(&conflict, &mut Policy(MetadataLimits::approved())),
        Err(ErrorCode::Conflict)
    );
}

#[test]
fn source_plan_uses_the_selected_metadata_and_entry_caps() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/rust-archive-worker/fixtures")
        .canonicalize()
        .unwrap();
    let sources = ["sample.gz", "sample.tar"]
        .into_iter()
        .enumerate()
        .map(|(i, name)| Source {
            path: NativePath::from_path(&root.join(name)),
            archive_name: format!("source-{i}"),
        })
        .collect::<Vec<_>>();
    let limits = Limits {
        entries: 2,
        ..Limits::default()
    };
    assert!(
        crate::create_sources::Plan::new(&sources, limits, &mut Policy(approved(10000, 2))).is_ok()
    );
    assert!(matches!(
        crate::create_sources::Plan::new(&sources, limits, &mut Policy(approved(10000, 1))),
        Err(ErrorCode::Limit)
    ));
    assert!(matches!(
        crate::create_sources::Plan::new(&sources, limits, &mut Policy(approved(511, 2))),
        Err(ErrorCode::Limit)
    ));
    assert!(matches!(
        crate::create_sources::Plan::new(
            &sources,
            Limits {
                entries: 1,
                ..limits
            },
            &mut Policy(approved(10000, 2))
        ),
        Err(ErrorCode::Limit)
    ));
}
