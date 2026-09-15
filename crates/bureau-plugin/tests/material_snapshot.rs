use std::fs;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bureau_plugin::{TreeSnapshot, tree_digest};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    destination: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bureau-code-snapshot-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("source");
        fs::create_dir_all(source.join("nested")).expect("source");
        fs::write(source.join("extension.mjs"), "export {};").expect("entrypoint");
        fs::write(source.join("nested/dependency.mjs"), "export const n = 1;").expect("dependency");
        fs::write(source.join("factory.json"), r#"{"name":"factory"}"#).expect("metadata");
        let destination = root.join("snapshot");
        Self {
            root,
            source,
            destination,
        }
    }

    fn digest(&self) -> String {
        tree_digest(&self.source).expect("source digest")
    }

    fn pin(&self) -> TreeSnapshot {
        TreeSnapshot::pin(&self.source, &self.destination, &self.digest()).expect("pin")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("clean fixture");
    }
}

#[test]
fn snapshot_preserves_entrypoint_dependencies_and_permissions() {
    let fixture = Fixture::new();
    fs::set_permissions(
        fixture.source.join("extension.mjs"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("executable mode");
    let snapshot = fixture.pin();
    let path = snapshot
        .path(Path::new("extension.mjs"))
        .expect("entrypoint");
    let mode = fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(
        (
            snapshot
                .read(Path::new("nested/dependency.mjs"))
                .expect("dependency"),
            mode,
            snapshot.digest() == fixture.digest()
        ),
        (b"export const n = 1;".to_vec(), 0o755, true)
    );
}

#[test]
fn snapshot_container_is_private() {
    let fixture = Fixture::new();
    let _snapshot = fixture.pin();
    let metadata = fs::metadata(&fixture.destination).expect("container");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
}

#[test]
fn current_directory_components_resolve_the_same_pinned_bytes_and_paths() {
    let fixture = Fixture::new();
    let snapshot = fixture.pin();
    for name in ["extension.mjs", "nested/dependency.mjs"] {
        let relative = PathBuf::from(".").join(name);
        assert_eq!(
            (
                snapshot.read(&relative).expect("dot read"),
                snapshot.path(&relative).expect("dot path")
            ),
            (
                snapshot.read(Path::new(name)).expect("read"),
                snapshot.directory().join(name)
            ),
        );
    }
}

#[test]
fn path_normalization_does_not_accept_parent_or_absolute_components() {
    let fixture = Fixture::new();
    let snapshot = fixture.pin();
    for name in [
        "../extension.mjs",
        "nested/../extension.mjs",
        "/extension.mjs",
        "",
    ] {
        assert_eq!(
            (
                snapshot.read(Path::new(name)).is_err(),
                snapshot.path(Path::new(name)).is_err()
            ),
            (true, true),
        );
    }
}

#[test]
fn a_changed_origin_does_not_replace_previously_pinned_code() {
    let fixture = Fixture::new();
    let snapshot = fixture.pin();
    fs::write(fixture.source.join("extension.mjs"), "changed").expect("change origin");
    let reopened = TreeSnapshot::pin(&fixture.source, &fixture.destination, snapshot.digest())
        .expect("reopen");
    assert_eq!(
        reopened
            .read(Path::new("extension.mjs"))
            .expect("pinned entrypoint"),
        b"export {};".to_vec()
    );
}

#[test]
fn pinned_code_remains_available_without_its_original_installation() {
    let fixture = Fixture::new();
    let snapshot = fixture.pin();
    fs::remove_dir_all(&fixture.source).expect("remove original");
    let reopened = TreeSnapshot::pin(&fixture.source, &fixture.destination, snapshot.digest())
        .expect("reopen");
    assert!(reopened.verify().is_ok());
}

#[test]
fn a_wrong_digest_never_publishes_code() {
    let fixture = Fixture::new();
    let result = TreeSnapshot::pin(&fixture.source, &fixture.destination, "tree-sha256:wrong");
    assert_eq!(
        (result.is_err(), fixture.destination.exists()),
        (true, false)
    );
}

#[test]
fn dependency_changes_invalidate_the_complete_snapshot() {
    let fixture = Fixture::new();
    let snapshot = fixture.pin();
    fs::write(
        snapshot.directory().join("nested/dependency.mjs"),
        "changed",
    )
    .expect("tamper");
    let restored = TreeSnapshot::pin(&fixture.source, &fixture.destination, snapshot.digest());
    assert_eq!(
        (
            snapshot.verify().is_err(),
            snapshot.read(Path::new("extension.mjs")).is_err(),
            restored.is_err()
        ),
        (true, true, true)
    );
}

#[test]
fn metadata_changes_invalidate_the_pinned_definition() {
    let fixture = Fixture::new();
    let snapshot = fixture.pin();
    fs::write(
        snapshot.directory().join("factory.json"),
        r#"{"name":"other"}"#,
    )
    .expect("changed metadata");
    assert!(snapshot.verify().is_err());
}

#[test]
fn executable_mode_changes_are_detected() {
    let fixture = Fixture::new();
    let snapshot = fixture.pin();
    fs::set_permissions(
        snapshot.directory().join("extension.mjs"),
        fs::Permissions::from_mode(0o700),
    )
    .expect("change mode");
    assert!(snapshot.verify().is_err());
}

#[test]
fn incomplete_existing_snapshots_are_not_reconstructed() {
    let fixture = Fixture::new();
    fs::create_dir(&fixture.destination).expect("incomplete container");
    let note = fixture.destination.join("retain");
    fs::write(&note, "evidence").expect("evidence");
    let result = TreeSnapshot::pin(&fixture.source, &fixture.destination, &fixture.digest());
    assert_eq!(
        (
            result.is_err(),
            fs::read_to_string(note).expect("preserved evidence")
        ),
        (true, "evidence".to_owned())
    );
}

#[test]
fn source_symlinks_are_not_followed() {
    let fixture = Fixture::new();
    let digest = fixture.digest();
    symlink("nested/dependency.mjs", fixture.source.join("link")).expect("symlink");
    let result = TreeSnapshot::pin(&fixture.source, &fixture.destination, &digest);
    assert_eq!(
        (result.is_err(), fixture.destination.exists()),
        (true, false)
    );
}

#[test]
fn snapshot_container_symlinks_are_rejected() {
    let fixture = Fixture::new();
    let snapshot = fixture.pin();
    let alias = fixture.root.join("alias");
    symlink(&fixture.destination, &alias).expect("alias");
    assert!(TreeSnapshot::open(&alias, snapshot.digest()).is_err());
}

#[test]
fn snapshot_reads_cannot_escape_the_pinned_tree() {
    let fixture = Fixture::new();
    let snapshot = fixture.pin();
    for path in ["../source/extension.mjs", "/etc/passwd", "missing"] {
        assert_eq!(
            (
                snapshot.read(Path::new(path)).is_err(),
                snapshot.path(Path::new(path)).is_err()
            ),
            (true, true)
        );
    }
}

#[test]
fn a_snapshot_cannot_be_created_inside_its_source() {
    let fixture = Fixture::new();
    let destination = fixture.source.join("snapshot");
    let result = TreeSnapshot::pin(&fixture.source, &destination, &fixture.digest());
    assert_eq!((result.is_err(), destination.exists()), (true, false));
}
