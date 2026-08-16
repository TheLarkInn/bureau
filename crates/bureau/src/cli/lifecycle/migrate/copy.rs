use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

use super::validate::Source;

pub(super) fn durable_state(source: &Source, stage: &Path) -> anyhow::Result<()> {
    if let Some(state) = &source.state {
        copy_file(state, &stage.join("state.db"))?;
    }
    if let Some(runs) = &source.runs {
        copy_runs(runs, &stage.join("runs"))?;
    }
    Ok(())
}

fn copy_runs(source: &Path, target: &Path) -> anyhow::Result<()> {
    create_directory(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        anyhow::ensure!(
            metadata.is_dir() && !entry.file_type()?.is_symlink(),
            "migration run entries must be safe directories"
        );
        copy_tree(&entry.path(), &target.join(entry.file_name()), true)?;
    }
    fs::File::open(target)?.sync_all()?;
    Ok(())
}

fn copy_tree(source: &Path, target: &Path, run_root: bool) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "migration source tree is unsafe"
    );
    create_directory(target)?;
    for entry in fs::read_dir(source)? {
        copy_entry(&entry?, target, run_root)?;
    }
    fs::File::open(target)?.sync_all()?;
    Ok(())
}

fn copy_entry(entry: &fs::DirEntry, target: &Path, run_root: bool) -> anyhow::Result<()> {
    let name = entry.file_name();
    let disposable = name == "wt"
        || (run_root && name == "activations")
        || name
            .to_str()
            .is_some_and(|value| value.starts_with(".concurrent-index-"));
    if disposable {
        return Ok(());
    }
    let source = entry.path();
    let destination = target.join(entry.file_name());
    let metadata = fs::symlink_metadata(&source)?;
    anyhow::ensure!(
        !metadata.file_type().is_symlink(),
        "migration source contains a symlink"
    );
    if metadata.is_dir() {
        copy_tree(&source, &destination, false)
    } else {
        anyhow::ensure!(
            metadata.is_file() && metadata.nlink() == 1,
            "migration source contains an unsafe file"
        );
        copy_file(&source, &destination)
    }
}

fn copy_file(source: &Path, target: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1,
        "migration source file is unsafe"
    );
    let bytes = fs::read(source)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    file.write_all(&bytes)?;
    let mode = 0o600 | (metadata.permissions().mode() & 0o100);
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.sync_all()?;
    Ok(())
}

fn create_directory(target: &Path) -> anyhow::Result<()> {
    fs::create_dir(target)?;
    fs::set_permissions(target, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
