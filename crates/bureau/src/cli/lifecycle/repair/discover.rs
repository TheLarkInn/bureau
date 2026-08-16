//! Read-only discovery of conservative repair candidates.

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use bureau::home::{Directory, Layout};
use bureau::repair::{
    CacheState, Candidate, DerivedState, DirectoryState, DisposableCache, Ownership,
    OwnershipState, PluginActivationState, WorktreeState,
};

pub(super) fn candidates(
    layout: &Layout,
    checkout: bool,
    config: bool,
) -> anyhow::Result<Vec<Candidate>> {
    let mut found: Vec<_> = Directory::ALL
        .into_iter()
        .map(|directory| directory_state(layout, directory))
        .collect();
    add_caches(&mut found, checkout, config);
    add_runs(&mut found, layout)?;
    add_ownership(&mut found, layout)?;
    Ok(found)
}

fn directory_state(layout: &Layout, directory: Directory) -> Candidate {
    let metadata = std::fs::symlink_metadata(layout.directory(directory)).ok();
    let exists = metadata
        .as_ref()
        .is_some_and(|value| value.is_dir() && !value.file_type().is_symlink());
    let permissions_ok =
        metadata.is_some_and(|value| value.permissions().mode().trailing_zeros() >= 6);
    Candidate::Directory(DirectoryState {
        directory,
        exists,
        permissions_ok,
    })
}

fn add_caches(found: &mut Vec<Candidate>, checkout: bool, config: bool) {
    let caches = [
        (checkout, DisposableCache::Checkout),
        (config, DisposableCache::Config),
    ];
    found.extend(caches.into_iter().filter_map(|(requested, cache)| {
        requested.then_some(Candidate::Cache(CacheState {
            cache,
            in_use: false,
        }))
    }));
}

fn add_runs(found: &mut Vec<Candidate>, layout: &Layout) -> anyhow::Result<()> {
    let entries = match std::fs::read_dir(layout.runs()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        add_run(found, layout, &entry?.path())?;
    }
    Ok(())
}

fn add_run(found: &mut Vec<Candidate>, layout: &Layout, path: &Path) -> anyhow::Result<()> {
    let run_id = safe_run_id(layout.runs(), path)?;
    if !path.join(bureau::runlog::EVENTS_FILE).is_file() {
        found.push(Candidate::Worktree(WorktreeState {
            run_id,
            run_exists: false,
            ownership_active: false,
        }));
        return Ok(());
    }
    let state = bureau::doctor::replay_run_read_only(path).map_err(anyhow::Error::msg)?;
    let live = bureau::doctor::active_lease_count(layout.state_db(), Some(&run_id))
        .map_err(anyhow::Error::msg)?
        > 0;
    add_derived(found, path, &run_id, &state, live);
    add_plugin(found, path, &run_id, live)
}

fn safe_run_id(root: &Path, path: &Path) -> anyhow::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "unsafe run directory {}",
        path.display()
    );
    let root = std::fs::canonicalize(root)?;
    anyhow::ensure!(
        std::fs::canonicalize(path)?.starts_with(root),
        "run escapes root"
    );
    let Some(run_id) = path.file_name().and_then(|name| name.to_str()) else {
        anyhow::bail!("run path has no UTF-8 name");
    };
    Ok(run_id.to_owned())
}

fn add_derived(
    found: &mut Vec<Candidate>,
    path: &Path,
    run_id: &str,
    state: &bureau::runlog::RunState,
    live: bool,
) {
    found.push(Candidate::DerivedState(DerivedState {
        run_id: run_id.to_owned(),
        durable_history_exists: true,
        needs_rebuild: !derived_matches(path, state),
        run_active: live,
    }));
}

fn add_plugin(
    found: &mut Vec<Candidate>,
    path: &Path,
    run_id: &str,
    live: bool,
) -> anyhow::Result<()> {
    for info in bureau::plugin::restoration_infos(path)? {
        found.push(Candidate::PluginActivation(PluginActivationState {
            activation_id: info.activation_id,
            run_id: run_id.to_owned(),
            plugin: info.plugin,
            installed_version: info.installed_version,
            recorded_version: info.recorded_version,
            stale: true,
            run_active: live,
        }));
    }
    Ok(())
}

fn derived_matches(path: &Path, expected: &bureau::runlog::RunState) -> bool {
    std::fs::read(path.join(bureau::runlog::STATE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<bureau::runlog::RunState>(&bytes).ok())
        .is_some_and(|state| state == *expected)
}

fn add_ownership(found: &mut Vec<Candidate>, layout: &Layout) -> anyhow::Result<()> {
    if !layout.state_db().is_file() {
        return Ok(());
    }
    let connection = rusqlite::Connection::open_with_flags(
        layout.state_db(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut statement = connection.prepare(
        "SELECT assignment, forge, external_id, run_id, owner_id, expires_at_ms FROM leases",
    )?;
    let rows = statement.query_map([], ownership)?;
    let observed_at_ms = now_millis();
    found.extend(rows.filter_map(Result::ok).map(|ownership| {
        Candidate::Ownership(OwnershipState {
            ownership,
            observed_at_ms,
        })
    }));
    Ok(())
}

fn ownership(row: &rusqlite::Row<'_>) -> rusqlite::Result<Ownership> {
    let expires: i64 = row.get(5)?;
    Ok(Ownership {
        assignment: row.get(0)?,
        forge: row.get(1)?,
        external_id: row.get(2)?,
        run_id: row.get(3)?,
        owner_id: row.get(4)?,
        expires_at_ms: u64::try_from(expires).unwrap_or(0),
    })
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}
