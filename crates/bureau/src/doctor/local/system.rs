use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use crate::home::Directory;

use super::LocalEffects;
use crate::doctor::{Observation, Status};

impl LocalEffects {
    pub(super) fn inspect_local_state(&self) -> Result<Observation, String> {
        let mut missing = 0;
        let mut unsafe_paths = 0;
        let mut wrong_mode = 0;
        for directory in Directory::ALL {
            let path = self.layout.directory(directory);
            count_path(path, PathKind::Directory, &mut missing, &mut unsafe_paths)?;
            wrong_mode += usize::from(directory_mode_is_open(path));
        }
        count_path(
            self.layout.settings(),
            PathKind::File,
            &mut missing,
            &mut unsafe_paths,
        )?;
        count_optional_file(self.layout.state_db(), &mut missing, &mut unsafe_paths)?;
        Ok(local_state_observation(missing, unsafe_paths, wrong_mode))
    }

    pub(super) fn inspect_plugin(&self) -> Observation {
        match crate::plugin::inspect_package(&self.plugin_root) {
            Ok(_) => Observation::new(
                Status::Ok,
                "plugin_mcp_ok",
                "bundled plugin and bureau-io MCP definition are available",
            ),
            Err(error) => Observation::new(
                Status::Error,
                "bundled_plugin_incomplete",
                format!("bundled plugin or MCP definition is invalid: {error}"),
            ),
        }
    }

    pub(super) fn binary_available(&self, binary: &str) -> bool {
        self.search_path
            .iter()
            .map(|directory| directory.join(binary))
            .any(|path| executable(&path))
    }
}

#[derive(Clone, Copy)]
enum PathKind {
    Directory,
    File,
}

fn count_path(
    path: &Path,
    kind: PathKind,
    missing: &mut usize,
    unsafe_paths: &mut usize,
) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if expected_kind(&metadata, kind) => {}
        Ok(_) => *unsafe_paths += 1,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => *missing += 1,
        Err(error) => return Err(format!("cannot inspect {}: {error}", path.display())),
    }
    Ok(())
}

fn count_optional_file(
    path: &Path,
    missing: &mut usize,
    unsafe_paths: &mut usize,
) -> Result<(), String> {
    count_path(path, PathKind::File, missing, unsafe_paths)
}

fn expected_kind(metadata: &fs::Metadata, kind: PathKind) -> bool {
    !metadata.file_type().is_symlink()
        && match kind {
            PathKind::Directory => metadata.is_dir(),
            PathKind::File => metadata.is_file(),
        }
}

fn local_state_observation(missing: usize, unsafe_paths: usize, wrong_mode: usize) -> Observation {
    if unsafe_paths > 0 {
        return Observation::new(
            Status::Error,
            "unsafe_local_paths",
            format!("{unsafe_paths} local layout paths have an unsafe type"),
        );
    }
    if missing > 0 {
        return Observation::new(
            Status::Warning,
            "local_paths_missing",
            format!("{missing} local layout paths are not initialized"),
        );
    }
    if wrong_mode > 0 {
        return Observation::new(
            Status::Warning,
            "directory_permissions_open",
            format!("{wrong_mode} local directories permit group or other access"),
        );
    }
    Observation::new(Status::Ok, "local_state_ok", "local layout is available")
}

fn directory_mode_is_open(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.permissions().mode() & 0o077 != 0
    })
}

fn executable(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}
