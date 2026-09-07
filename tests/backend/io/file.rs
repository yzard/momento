use std::collections::HashSet;

use momento_api::io::file::{
    NormalizedStoragePath, PathClaim, PathClaimMode, PathClaimScope, StorageRootId,
    StorageRootRegistry, MAX_STORAGE_PATH_COMPONENTS,
};

#[test]
fn storage_roots_are_closed_unique_and_counted() {
    assert_eq!(StorageRootId::ALL.len(), StorageRootId::COUNT);
    let names = StorageRootId::ALL
        .into_iter()
        .map(StorageRootId::as_str)
        .collect::<HashSet<_>>();
    assert_eq!(names.len(), StorageRootId::COUNT);
    for root in StorageRootId::ALL {
        assert_eq!(StorageRootId::try_from(root.as_str()), Ok(root));
        assert!(!root.directory_name().is_empty());
    }
}

#[test]
fn canonical_keys_are_component_delimited_and_preserve_ancestors() {
    let first = NormalizedStoragePath::parse("ab/c").expect("first path");
    let second = NormalizedStoragePath::parse("a/bc").expect("second path");
    assert_ne!(first.path_key(), second.path_key());
    assert_eq!(first.ancestor_keys().len(), 2);
    assert_eq!(first.ancestor_keys()[1], first.path_key());

    let descendant = NormalizedStoragePath::parse("ab/c/d").expect("descendant path");
    assert!(descendant.path_key().starts_with(first.path_key()));
    assert!(descendant.path_key() < first.subtree_upper_bound().as_slice());
}

#[test]
fn storage_paths_reject_absolute_traversal_empty_and_overbounded_components() {
    for invalid in ["", "/absolute", ".", "..", "a//b", "a/./b", "a/../b"] {
        assert!(
            NormalizedStoragePath::parse(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }
    assert!(NormalizedStoragePath::parse(&"a".repeat(256)).is_err());
    let too_many_components = std::iter::repeat_n("a", MAX_STORAGE_PATH_COMPONENTS + 1)
        .collect::<Vec<_>>()
        .join("/");
    assert!(NormalizedStoragePath::parse(&too_many_components).is_err());
}

#[test]
fn path_claim_conflicts_are_component_scoped_and_read_sharing_is_allowed() {
    let claim = |path: &str, mode, scope| PathClaim {
        storage_root: StorageRootId::Originals,
        path: NormalizedStoragePath::parse(path).expect("claim path"),
        mode,
        scope,
    };
    let subtree_write = claim("album", PathClaimMode::Write, PathClaimScope::Subtree);
    let descendant_read = claim(
        "album/photo.jpg",
        PathClaimMode::Read,
        PathClaimScope::Exact,
    );
    let sibling_write = claim("album-2", PathClaimMode::Write, PathClaimScope::Exact);
    assert!(subtree_write.conflicts_with(&descendant_read));
    assert!(!subtree_write.conflicts_with(&sibling_write));

    let exact_read = claim(
        "album/photo.jpg",
        PathClaimMode::Read,
        PathClaimScope::Exact,
    );
    assert!(!exact_read.conflicts_with(&descendant_read));
    let other_root = PathClaim {
        storage_root: StorageRootId::Previews,
        path: NormalizedStoragePath::parse("album/photo.jpg").expect("other root path"),
        mode: PathClaimMode::Write,
        scope: PathClaimScope::Exact,
    };
    assert!(!exact_read.conflicts_with(&other_root));
}

#[test]
fn storage_root_registry_opens_fixed_directories_and_rejects_symlinks() {
    let directory = tempfile::tempdir().expect("data directory");
    for root in StorageRootId::ALL {
        if root != StorageRootId::Static {
            std::fs::create_dir(directory.path().join(root.directory_name()))
                .expect("storage root");
        }
    }
    let registry =
        StorageRootRegistry::open_existing(directory.path(), None).expect("storage registry");
    assert!(registry.is_available(StorageRootId::Originals));
    assert!(!registry.is_available(StorageRootId::Static));

    std::fs::remove_dir(directory.path().join("imports")).expect("remove imports root");
    std::os::unix::fs::symlink(
        directory.path().join("originals"),
        directory.path().join("imports"),
    )
    .expect("symlink imports root");
    assert!(StorageRootRegistry::open_existing(directory.path(), None).is_err());
}
