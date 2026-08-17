mod copy;
mod transaction;
mod validate;

use std::path::{Path, PathBuf};

fn final_settings(settings: &bureau::setup::Settings) -> bureau::setup::Settings {
    let mut settings = settings.clone();
    settings.migration.source = None;
    settings
}

/// Nanoseconds since the Unix epoch. The process clock boundary:
/// bound once as a function pointer so this stays the single read site.
fn now() -> u128 {
    let now = std::time::SystemTime::now;
    now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

struct Stage {
    path: PathBuf,
    cleanup_on_drop: bool,
}

impl Stage {
    fn new(root: &Path) -> anyhow::Result<Self> {
        use std::os::unix::fs::PermissionsExt as _;

        std::fs::create_dir_all(root)?;
        let name = format!(".migration-{}-{}", std::process::id(), now());
        let path = root.join(name);
        std::fs::create_dir(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            path,
            cleanup_on_drop: true,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn existing(path: PathBuf) -> anyhow::Result<Self> {
        let metadata = std::fs::symlink_metadata(&path)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "pending migration stage is unsafe"
        );
        Ok(Self {
            path,
            cleanup_on_drop: false,
        })
    }

    const fn preserve(&mut self) {
        self.cleanup_on_drop = false;
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let _removed = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn validate_resume_source(
    settings: Option<&bureau::setup::Settings>,
    resume: &transaction::Resume,
) -> anyhow::Result<()> {
    let settings =
        settings.ok_or_else(|| anyhow::anyhow!("pending migration requires init or setup"))?;
    let final_bytes = serde_yaml_ng::to_string(&final_settings(settings))?.into_bytes();
    anyhow::ensure!(
        final_bytes == resume.settings,
        "pending migration settings changed"
    );
    let requested = settings
        .migration
        .source
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("pending migration requires init or setup"))?;
    let requested = std::fs::canonicalize(requested)?;
    anyhow::ensure!(
        requested == resume.source,
        "pending migration source changed"
    );
    Ok(())
}

pub(super) fn recover_pending(
    layout: &bureau::home::Layout,
    settings: Option<&mut bureau::setup::Settings>,
) -> anyhow::Result<()> {
    match transaction::recover(layout)? {
        transaction::Recovery::Completed => {
            if let Some(settings) = settings {
                settings.migration.source = None;
            }
        }
        transaction::Recovery::Resume(resume) => {
            validate_resume_source(settings.as_deref(), &resume)?;
        }
        transaction::Recovery::None | transaction::Recovery::RolledBack => {}
    }
    Ok(())
}

pub(super) struct Prepared {
    stage: Stage,
    _source: validate::Source,
}

impl Prepared {
    pub(super) fn state_path(&self) -> PathBuf {
        self.stage.path().join("state.db")
    }

    pub(super) fn runs_path(&self) -> PathBuf {
        self.stage.path().join("runs")
    }

    pub(super) fn commit(layout: &bureau::home::Layout) -> anyhow::Result<()> {
        let effects = transaction::start_commit(layout)?;
        let committed = transaction::move_current(layout)
            .and_then(|()| transaction::data_moved(layout, effects));
        if let Err(commit) = committed {
            return match transaction::recover(layout) {
                Ok(transaction::Recovery::Completed) if effects => Ok(()),
                Ok(_) => Err(commit),
                Err(rollback) => Err(anyhow::anyhow!(
                    "{commit}; migration rollback failed: {rollback}"
                )),
            };
        }
        Ok(())
    }

    pub(super) fn before_effects(layout: &bureau::home::Layout) -> anyhow::Result<()> {
        transaction::effects_running(layout)
    }
}

fn settle_settings(
    migration: &mut Option<Prepared>,
    layout: &bureau::home::Layout,
    result: Result<(), bureau::setup::FileError>,
) -> anyhow::Result<()> {
    if migration.is_none() {
        return result.map_err(Into::into);
    }
    match result {
        Ok(()) => transaction::finish(layout)?,
        Err(error) => {
            let recovery = transaction::recover(layout)?;
            if matches!(recovery, transaction::Recovery::Completed) {
                *migration = None;
                return Ok(());
            }
            return Err(error.into());
        }
    }
    *migration = None;
    Ok(())
}

fn new_prepared(
    layout: &bureau::home::Layout,
    settings: &bureau::setup::Settings,
) -> anyhow::Result<Prepared> {
    let source_path = settings
        .migration
        .source
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("migration source is missing"))?;
    let source = validate::source(layout, source_path)?;
    let mut stage = Stage::new(layout.root())?;
    copy::durable_state(&source, stage.path())?;
    let final_settings = final_settings(settings);
    transaction::begin(
        layout,
        stage.path(),
        &final_settings,
        source.root(),
        source.target_runs_existed,
    )?;
    stage.preserve();
    Ok(Prepared {
        stage,
        _source: source,
    })
}

fn resume_prepared(
    layout: &bureau::home::Layout,
    settings: &bureau::setup::Settings,
    resume: transaction::Resume,
) -> anyhow::Result<Prepared> {
    let final_bytes = serde_yaml_ng::to_string(&final_settings(settings))?.into_bytes();
    anyhow::ensure!(
        final_bytes == resume.settings,
        "pending migration settings changed"
    );
    let requested = settings
        .migration
        .source
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("pending migration requires its original source"))?;
    let source = validate::source(layout, requested)?;
    anyhow::ensure!(
        source.root() == resume.source,
        "pending migration source changed"
    );
    anyhow::ensure!(
        source.target_runs_existed == resume.target_runs_existed,
        "pending migration target changed"
    );
    Ok(Prepared {
        stage: Stage::existing(resume.stage)?,
        _source: source,
    })
}

pub(super) fn prepare(
    layout: &bureau::home::Layout,
    settings: &bureau::setup::Settings,
) -> anyhow::Result<Option<Prepared>> {
    match transaction::recover(layout)? {
        transaction::Recovery::Completed => return Ok(None),
        transaction::Recovery::Resume(resume) => {
            return resume_prepared(layout, settings, resume).map(Some);
        }
        transaction::Recovery::None | transaction::Recovery::RolledBack => {}
    }
    new_prepared(layout, settings).map(Some)
}

pub(super) fn save_settings(
    migration: &mut Option<Prepared>,
    layout: &bureau::home::Layout,
    settings: &bureau::setup::Settings,
) -> anyhow::Result<()> {
    if migration.is_some() {
        Prepared::commit(layout)?;
    }
    let result = bureau::setup::save_settings(layout.settings(), settings);
    settle_settings(migration, layout, result)
}
