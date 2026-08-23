use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

pub(super) struct Source {
    root: PathBuf,
    pub(super) state: Option<PathBuf>,
    pub(super) runs: Option<PathBuf>,
    pub(super) target_runs_existed: bool,
    _maintenance: bureau::maintenance::Guard,
}

impl Source {
    pub(super) fn root(&self) -> &Path {
        &self.root
    }
}

fn reject_pending_migration(source: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(source.join("migration.json")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => anyhow::bail!("migration source has its own pending migration"),
    }
}

fn safe_directory(path: &Path, create: bool) -> anyhow::Result<PathBuf> {
    if create {
        std::fs::create_dir_all(path)?;
    }
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "unsafe migration directory {}",
        path.display()
    );
    Ok(std::fs::canonicalize(path)?)
}

fn target_empty(layout: &bureau::home::Layout) -> anyhow::Result<bool> {
    anyhow::ensure!(
        !layout.state_db().exists(),
        "target state database already exists"
    );
    let exists = layout.runs().exists();
    if exists {
        let metadata = std::fs::symlink_metadata(layout.runs())?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "target runs path is unsafe"
        );
        anyhow::ensure!(
            std::fs::read_dir(layout.runs())?.next().is_none(),
            "target runs directory is not empty"
        );
    }
    Ok(exists)
}

fn optional_file(path: &Path) -> anyhow::Result<Option<PathBuf>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1,
                "unsafe migration file {}",
                path.display()
            );
            Ok(Some(path.to_path_buf()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn optional_directory(path: &Path) -> anyhow::Result<Option<PathBuf>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "unsafe migration runs path"
            );
            Ok(Some(path.to_path_buf()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Milliseconds since the Unix epoch as `SQLite` sees them. The process
/// clock boundary: bound once as a function pointer so this stays the
/// single read site.
fn now_millis() -> i64 {
    let now = std::time::SystemTime::now;
    let millis = now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    i64::try_from(millis).unwrap_or(i64::MAX)
}

const LEASE_COLUMNS: &[&str] = &[
    "assignment",
    "forge",
    "external_id",
    "run_id",
    "owner_id",
    "expires_at_ms",
];
const RUN_COLUMNS: &[&str] = &["run_id", "assignment", "started_at_ms", "cost_usd"];
const DEDUP_COLUMNS: &[&str] = &["content_hash", "disposition", "at_ms"];
const LABEL_RULE_EVENT_COLUMNS: &[&str] = &[
    "id",
    "attempt_id",
    "rule",
    "source",
    "item",
    "event",
    "message",
    "add_labels",
    "remove_labels",
    "dependency_count",
    "closed_dependency_count",
    "occurred_at_ms",
];

fn table_names(connection: &Connection) -> anyhow::Result<BTreeSet<String>> {
    let sql = "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'";
    let mut statement = connection.prepare(sql)?;
    let names = statement.query_map([], |row| row.get::<_, String>(0))?;
    names.collect::<Result<_, _>>().map_err(Into::into)
}

fn required_columns(table: &str, columns: &BTreeSet<String>) -> anyhow::Result<()> {
    let required: &[&str] = match table {
        "leases" => &["assignment", "forge", "external_id", "expires_at_ms"],
        "runs" => &["assignment", "started_at_ms", "cost_usd"],
        "dedup" => &["content_hash", "disposition", "at_ms"],
        "label_rule_events" => LABEL_RULE_EVENT_COLUMNS,
        _ => anyhow::bail!("unknown migration table `{table}`"),
    };
    let missing = required.iter().find(|name| !columns.contains(**name));
    missing.map_or(Ok(()), |name| {
        Err(anyhow::anyhow!(
            "migration table `{table}` is missing `{name}`"
        ))
    })
}

fn validate_columns(connection: &Connection, table: &str, allowed: &[&str]) -> anyhow::Result<()> {
    if !table_names(connection)?.contains(table) {
        return Ok(());
    }
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    let allowed: BTreeSet<_> = allowed.iter().map(|value| (*value).to_owned()).collect();
    anyhow::ensure!(
        columns.is_subset(&allowed),
        "migration table `{table}` is from a newer schema"
    );
    required_columns(table, &columns)
}

fn reject_active_leases(connection: &Connection, tables: &BTreeSet<String>) -> anyhow::Result<()> {
    if !tables.contains("leases") {
        return Ok(());
    }
    let active: i64 = connection.query_row(
        "SELECT COUNT(*) FROM leases WHERE expires_at_ms > ?1",
        [now_millis()],
        |row| row.get(0),
    )?;
    anyhow::ensure!(active == 0, "migration source has active leases");
    Ok(())
}

fn validate_database(path: &Path) -> anyhow::Result<()> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    anyhow::ensure!(
        integrity == "ok",
        "migration database integrity check failed"
    );
    let tables = table_names(&connection)?;
    let allowed = BTreeSet::from([
        "dedup".to_owned(),
        "label_rule_events".to_owned(),
        "leases".to_owned(),
        "runs".to_owned(),
    ]);
    anyhow::ensure!(
        tables.is_subset(&allowed),
        "migration database is from a newer schema"
    );
    reject_active_leases(&connection, &tables)?;
    validate_columns(&connection, "leases", LEASE_COLUMNS)?;
    validate_columns(&connection, "runs", RUN_COLUMNS)?;
    validate_columns(&connection, "dedup", DEDUP_COLUMNS)?;
    validate_columns(&connection, "label_rule_events", LABEL_RULE_EVENT_COLUMNS)
}

pub(super) fn source(layout: &bureau::home::Layout, source: &Path) -> anyhow::Result<Source> {
    let target = safe_directory(layout.root(), true)?;
    let source = safe_directory(source, false)?;
    anyhow::ensure!(
        !source.starts_with(&target) && !target.starts_with(&source),
        "migration source and target must not overlap"
    );
    let maintenance = bureau::maintenance::exclusive(&source)?;
    reject_pending_migration(&source)?;
    let target_runs_existed = target_empty(layout)?;
    let state = optional_file(&source.join("state.db"))?;
    let runs = optional_directory(&source.join("runs"))?;
    anyhow::ensure!(
        state.is_some() || runs.is_some(),
        "migration source has no durable state"
    );
    if let Some(path) = &state {
        validate_database(path)?;
    }

    Ok(Source {
        root: source,
        state,
        runs,
        target_runs_existed,
        _maintenance: maintenance,
    })
}
