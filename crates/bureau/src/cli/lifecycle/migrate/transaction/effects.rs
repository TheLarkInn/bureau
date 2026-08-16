use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use super::{Marker, Phase, Recovery, remove_marker, remove_stage, set_phase, validate_stage};

pub fn move_current(layout: &bureau::home::Layout) -> anyhow::Result<()> {
    let path = layout.root().join(super::MARKER);
    let marker = super::read_marker(&path)?
        .ok_or_else(|| anyhow::anyhow!("migration marker disappeared before data move"))?;
    move_data(layout, &marker)
}

pub fn complete_commit(layout: &bureau::home::Layout, marker: &Marker) -> anyhow::Result<Recovery> {
    move_data(layout, marker)?;
    set_phase(layout, Phase::EffectsDataMoved)?;
    finalize(layout, marker)
}

pub fn move_data(layout: &bureau::home::Layout, marker: &Marker) -> anyhow::Result<()> {
    validate_stage(&marker.stage)?;
    let stage = layout.root().join(&marker.stage);
    let metadata = fs::symlink_metadata(&stage)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "migration stage is unsafe"
    );
    complete_state_move(&stage, layout, marker.state_expected)?;
    complete_runs_move(&stage, layout, marker)?;
    fs::File::open(layout.root())?.sync_all()?;
    Ok(())
}

pub fn finalize(layout: &bureau::home::Layout, marker: &Marker) -> anyhow::Result<Recovery> {
    let settings: bureau::setup::Settings = serde_yaml_ng::from_slice(&marker.settings)?;
    bureau::setup::save_settings(layout.settings(), &settings)?;
    remove_stage(layout.root(), &marker.stage)?;
    remove_marker(layout.root())?;
    Ok(Recovery::Completed)
}

fn complete_state_move(
    stage: &Path,
    layout: &bureau::home::Layout,
    expected: bool,
) -> anyhow::Result<()> {
    move_if_needed(
        &stage.join("state.db"),
        layout.state_db(),
        expected,
        false,
        false,
    )
}

fn complete_runs_move(
    stage: &Path,
    layout: &bureau::home::Layout,
    marker: &Marker,
) -> anyhow::Result<()> {
    move_if_needed(
        &stage.join("runs"),
        layout.runs(),
        marker.runs_expected,
        marker.target_runs_existed,
        true,
    )
}

fn move_if_needed(
    source: &Path,
    target: &Path,
    expected: bool,
    target_may_be_empty: bool,
    directory: bool,
) -> anyhow::Result<()> {
    let source_exists = safe_present(source, directory)?;
    let target_exists = safe_present(target, directory)?;
    if !expected {
        let target_ok =
            !target_exists || (target_may_be_empty && std::fs::read_dir(target)?.next().is_none());
        anyhow::ensure!(
            !source_exists && target_ok,
            "unexpected migration durable path"
        );
        return Ok(());
    }
    match (source_exists, target_exists) {
        (true, false) => std::fs::rename(source, target).map_err(Into::into),
        (true, true) if target_may_be_empty && std::fs::read_dir(target)?.next().is_none() => {
            std::fs::remove_dir(target)?;
            std::fs::rename(source, target)?;
            Ok(())
        }

        (false, true) => Ok(()),
        _ => anyhow::bail!("migration durable path has an ambiguous commit state"),
    }
}

fn safe_present(path: &Path, directory: bool) -> anyhow::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let safe = !metadata.file_type().is_symlink()
        && if directory {
            metadata.is_dir()
        } else {
            metadata.is_file() && metadata.nlink() == 1
        };
    anyhow::ensure!(safe, "migration durable path is unsafe");
    Ok(true)
}
