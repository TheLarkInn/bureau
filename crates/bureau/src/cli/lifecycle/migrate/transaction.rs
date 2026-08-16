use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

mod effects;
#[cfg(test)]
mod tests;

const MARKER: &str = "migration.json";
const TEMPORARY: &str = "migration.json.tmp";

pub use effects::move_current as move_data;

#[derive(Debug, PartialEq, Eq)]
pub enum Recovery {
    None,
    Completed,
    RolledBack,
    Resume(Resume),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Resume {
    pub stage: PathBuf,
    pub source: PathBuf,
    pub target_runs_existed: bool,
    pub settings: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    stage: String,
    target_runs_existed: bool,
    settings: Vec<u8>,
    source: PathBuf,
    state_expected: bool,
    runs_expected: bool,
    phase: Phase,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    EffectsRunning,
    EffectsCommitting,
    DataMoved,
    EffectsDataMoved,
}

pub fn begin(
    layout: &bureau::home::Layout,
    stage: &Path,
    settings: &bureau::setup::Settings,
    source: &Path,
    target_runs_existed: bool,
) -> anyhow::Result<()> {
    let state_expected = stage.join("state.db").exists();
    let runs_expected = stage.join("runs").exists();
    let stage = stage_name(stage)?;
    let marker = Marker {
        stage,
        target_runs_existed,
        settings: serde_yaml_ng::to_string(settings)?.into_bytes(),
        source: source.to_path_buf(),
        state_expected,
        runs_expected,
        phase: Phase::Prepared,
    };
    let path = layout.root().join(MARKER);
    anyhow::ensure!(!path.exists(), "another migration transaction is pending");
    write_marker(&path, &marker)
}

pub fn effects_running(layout: &bureau::home::Layout) -> anyhow::Result<()> {
    set_phase(layout, Phase::EffectsRunning)
}

pub fn start_commit(layout: &bureau::home::Layout) -> anyhow::Result<bool> {
    let path = layout.root().join(MARKER);
    let marker = read_marker(&path)?
        .ok_or_else(|| anyhow::anyhow!("migration marker disappeared before commit"))?;
    if matches!(marker.phase, Phase::EffectsRunning) {
        let mut marker = marker;
        let stage = layout.root().join(&marker.stage);
        marker.state_expected = stage.join("state.db").exists();
        marker.runs_expected = stage.join("runs").exists();
        marker.phase = Phase::EffectsCommitting;
        write_marker(&path, &marker)?;
        Ok(true)
    } else if matches!(marker.phase, Phase::Prepared) {
        Ok(false)
    } else {
        anyhow::bail!("migration transaction is not ready to commit")
    }
}

pub fn data_moved(layout: &bureau::home::Layout, effects: bool) -> anyhow::Result<()> {
    let phase = if effects {
        Phase::EffectsDataMoved
    } else {
        Phase::DataMoved
    };
    set_phase(layout, phase)
}

fn set_phase(layout: &bureau::home::Layout, phase: Phase) -> anyhow::Result<()> {
    let path = layout.root().join(MARKER);
    let mut marker = read_marker(&path)?
        .ok_or_else(|| anyhow::anyhow!("migration marker disappeared before commit"))?;
    marker.phase = phase;
    write_marker(&path, &marker)
}

pub fn finish(layout: &bureau::home::Layout) -> anyhow::Result<()> {
    remove_marker(layout.root())?;
    cleanup_orphans(layout.root())
}

pub fn recover(layout: &bureau::home::Layout) -> anyhow::Result<Recovery> {
    if !layout.root().exists() {
        return Ok(Recovery::None);
    }
    let path = layout.root().join(MARKER);
    let Some(marker) = read_marker(&path)? else {
        cleanup_orphans(layout.root())?;
        return Ok(Recovery::None);
    };
    recover_marker(layout, &marker)
}

fn recover_marker(layout: &bureau::home::Layout, marker: &Marker) -> anyhow::Result<Recovery> {
    match marker.phase {
        Phase::Prepared => rollback(layout, marker),
        Phase::EffectsRunning => resume(layout, marker),
        Phase::EffectsCommitting => effects::complete_commit(layout, marker),
        Phase::DataMoved => recover_moved(layout, marker),
        Phase::EffectsDataMoved => effects::finalize(layout, marker),
    }
}

fn recover_moved(layout: &bureau::home::Layout, marker: &Marker) -> anyhow::Result<Recovery> {
    let installed = match fs::read(layout.settings()) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if installed.as_deref() == Some(marker.settings.as_slice()) {
        remove_stage(layout.root(), &marker.stage)?;
        remove_marker(layout.root())?;
        return Ok(Recovery::Completed);
    }
    rollback(layout, marker)
}

fn resume(layout: &bureau::home::Layout, marker: &Marker) -> anyhow::Result<Recovery> {
    validate_stage(&marker.stage)?;
    let stage = layout.root().join(&marker.stage);
    let metadata = fs::symlink_metadata(&stage)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "migration stage is unsafe"
    );
    Ok(Recovery::Resume(Resume {
        stage,
        source: marker.source.clone(),
        target_runs_existed: marker.target_runs_existed,
        settings: marker.settings.clone(),
    }))
}

fn rollback(layout: &bureau::home::Layout, marker: &Marker) -> anyhow::Result<Recovery> {
    rollback_paths(layout, marker.target_runs_existed)?;
    remove_stage(layout.root(), &marker.stage)?;
    remove_marker(layout.root())?;
    Ok(Recovery::RolledBack)
}

fn rollback_paths(layout: &bureau::home::Layout, target_runs_existed: bool) -> anyhow::Result<()> {
    remove_file(layout.state_db())?;
    remove_directory(layout.runs())?;
    if target_runs_existed {
        fs::create_dir_all(layout.runs())?;
    }
    Ok(())
}

fn read_marker(path: &Path) -> anyhow::Result<Option<Marker>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1,
        "migration marker is unsafe"
    );
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}

fn write_marker(path: &Path, marker: &Marker) -> anyhow::Result<()> {
    let temporary = path.with_file_name(TEMPORARY);
    let _removed = fs::remove_file(&temporary);
    let bytes = serde_json::to_vec_pretty(marker)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("migration marker has no parent"))?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn remove_marker(root: &Path) -> anyhow::Result<()> {
    remove_file(&root.join(MARKER))?;
    remove_file(&root.join(TEMPORARY))?;
    fs::File::open(root)?.sync_all()?;
    Ok(())
}

fn cleanup_orphans(root: &Path) -> anyhow::Result<()> {
    remove_file(&root.join(TEMPORARY))?;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        if name
            .to_str()
            .is_some_and(|value| value.starts_with(".migration-"))
        {
            remove_stage(root, &name.to_string_lossy())?;
        }
    }
    Ok(())
}

fn remove_stage(root: &Path, name: &str) -> anyhow::Result<()> {
    validate_stage(name)?;
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "migration stage is unsafe"
            );
            fs::remove_dir_all(path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn remove_file(path: &Path) -> anyhow::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn remove_directory(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path)?;
            Ok(())
        }
        Ok(_) => Err(anyhow::anyhow!("migration target directory is unsafe")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn stage_name(path: &Path) -> anyhow::Result<String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("migration stage has no UTF-8 name"))?;
    validate_stage(name).map(|()| name.to_owned())
}

fn validate_stage(name: &str) -> anyhow::Result<()> {
    let path = PathBuf::from(name);
    let mut components = path.components();
    let one =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    anyhow::ensure!(
        one && name.starts_with(".migration-"),
        "invalid migration stage"
    );
    Ok(())
}
