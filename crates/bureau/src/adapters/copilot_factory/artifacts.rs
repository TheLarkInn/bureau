//! Per-invocation executable and session-provider pins, outside the worktree.

mod paths;

pub use paths::Paths;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use bureau_plugin::TreeSnapshot;
use serde::{Deserialize, Serialize};

use super::SetupError;
use super::definition::Definition;
use crate::config::CopilotFactory;

fn project_source(factory: &CopilotFactory, worktree: &Path) -> Result<PathBuf, String> {
    let name = factory
        .extension
        .strip_prefix("project:")
        .ok_or("factory provider source must be project-qualified")?;
    if name.is_empty() || name.contains(['/', '\\']) || matches!(name, "." | "..") {
        return Err("factory provider source is not a single project directory".to_owned());
    }
    let worktree = fs::canonicalize(worktree).map_err(|error| error.to_string())?;
    let source = worktree.join(".github/extensions").join(name);
    let canonical = fs::canonicalize(&source).map_err(|error| error.to_string())?;
    if canonical != source {
        return Err("factory provider source may not traverse symlinked directories".to_owned());
    }
    Ok(source)
}

/// Source provenance and actual provider ownership are intentionally distinct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    /// Approved original project source, even if later edited or deleted.
    pub source_extension_id: String,
    /// Session-scoped provider actually registered with the SDK.
    pub runtime_extension_id: String,
    /// Directory name presented by extension discovery.
    pub runtime_extension_name: String,
    /// Complete approved provider code digest.
    pub provider_digest: String,
    /// Complete qualified runtime and SDK digest.
    pub runtime_digest: String,
}

impl Identity {
    fn new(factory: &CopilotFactory, session_id: &str) -> Self {
        Self {
            source_extension_id: factory.extension.clone(),
            runtime_extension_id: format!("session:{session_id}:{session_id}"),
            runtime_extension_name: session_id.to_owned(),
            provider_digest: factory.extension_digest.clone(),
            runtime_digest: factory.runtime.digest.clone(),
        }
    }
}

fn executable(snapshot: &TreeSnapshot, relative: &Path) -> Result<PathBuf, String> {
    let path = snapshot.path(relative).map_err(|error| error.to_string())?;
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err("qualified runtime executable is not an executable plain file".to_owned());
    }
    Ok(path)
}

fn checked_file(snapshot: &TreeSnapshot, relative: &Path) -> Result<PathBuf, String> {
    snapshot.read(relative).map_err(|error| error.to_string())?;
    Ok(snapshot.directory().join(relative).components().collect())
}

/// Exact stock launcher paths within the qualified artifact, not guessed from cwd.
#[derive(Debug, Clone)]
pub struct Launch {
    /// Actual host runtime image, matching the stock launcher's process.execPath.
    pub executable: PathBuf,
    /// Optional CLI entrypoint for a non-embedded image.
    pub cli: Option<PathBuf>,
    /// Verified CLI distribution directory.
    pub dist: PathBuf,
    /// SDK root containing index.js and extension.js directly.
    pub sdk: PathBuf,
    /// Stock extension bootstrap; passed as the sole extension-process argument.
    pub bootstrap: PathBuf,
}

impl Launch {
    fn new(factory: &CopilotFactory, runtime: &TreeSnapshot) -> Result<Self, String> {
        let config = &factory.runtime;
        let dist = runtime
            .path(&config.dist)
            .map_err(|error| error.to_string())?;
        let sdk = config.dist.join("copilot-sdk");
        checked_file(runtime, &sdk.join("index.js"))?;
        checked_file(runtime, &sdk.join("extension.js"))?;
        checked_file(
            runtime,
            &config.dist.join("preloads/extension_sdk_resolver.mjs"),
        )?;
        Ok(Self {
            executable: executable(runtime, &config.executable)?,
            cli: config
                .cli
                .as_ref()
                .map(|path| checked_file(runtime, path))
                .transpose()?,
            bootstrap: checked_file(
                runtime,
                &config.dist.join("preloads/extension_bootstrap.mjs"),
            )?,
            sdk: dist.join("copilot-sdk"),
            dist,
        })
    }
}

#[derive(Debug, Clone)]
struct Code {
    runtime: TreeSnapshot,
    provider: TreeSnapshot,
}

impl Code {
    fn verify(&self) -> Result<(), String> {
        self.runtime.verify().map_err(|error| error.to_string())?;
        self.provider.verify().map_err(|error| error.to_string())
    }
}

/// Fully checked executable material; this value grants no factory or child-tool permission.
#[derive(Debug, Clone)]
pub struct Artifacts {
    /// Private paths, retained independently of the worktree.
    pub paths: Paths,
    /// Original source and actual runtime provider identities.
    pub identity: Identity,
    /// Qualified stock launcher.
    pub launch: Launch,
    /// Full metadata read from the pinned provider.
    pub definition: Definition,
    code: Code,
}

impl Artifacts {
    fn load(factory: &CopilotFactory, paths: Paths, session_id: &str) -> Result<Self, String> {
        paths.verify_private()?;
        let runtime = TreeSnapshot::open(&paths.runtime, &factory.runtime.digest)
            .map_err(|error| error.to_string())?;
        let provider = TreeSnapshot::open(&paths.provider, &factory.extension_digest)
            .map_err(|error| error.to_string())?;
        checked_file(&provider, Path::new("extension.mjs"))?;
        let metadata = provider
            .read(&factory.metadata)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            launch: Launch::new(factory, &runtime)?,
            definition: Definition::parse(&metadata, &factory.name, &factory.args)?,
            identity: Identity::new(factory, session_id),
            paths,
            code: Code { runtime, provider },
        })
    }

    fn pin_inner(
        factory: &CopilotFactory,
        worktree: &Path,
        root: &Path,
        session_id: &str,
    ) -> Result<Self, String> {
        let paths = Paths::new(root, worktree, session_id)?;
        paths.create_private()?;
        TreeSnapshot::pin(
            &factory.runtime.directory,
            &paths.runtime,
            &factory.runtime.digest,
        )
        .map_err(|error| error.to_string())?;
        let source = project_source(factory, worktree)?;
        TreeSnapshot::pin(&source, &paths.provider, &factory.extension_digest)
            .map_err(|error| error.to_string())?;
        Self::load(factory, paths, session_id)
    }

    /// Pins approved source once. No executable or provider is run.
    ///
    /// # Errors
    /// Rejects invalid material, confinement, metadata, or argument constraints.
    pub fn pin(
        factory: &CopilotFactory,
        worktree: &Path,
        root: &Path,
        session_id: &str,
    ) -> Result<Self, SetupError> {
        Self::pin_inner(factory, worktree, root, session_id).map_err(SetupError::Material)
    }

    /// Loads only the original pinned copies; never resolves current project provider bytes.
    ///
    /// # Errors
    /// Rejects lost or changed pins rather than restoring from ambient sources.
    pub fn resume(
        factory: &CopilotFactory,
        worktree: &Path,
        root: &Path,
        session_id: &str,
    ) -> Result<Self, SetupError> {
        Self::load(factory, Paths::new(root, worktree, session_id)?, session_id)
            .map_err(SetupError::Material)
    }

    /// Revalidates both executable trees before every provider launch or reload.
    ///
    /// # Errors
    /// Rejects missing or changed approved code.
    pub fn verify(&self) -> Result<(), SetupError> {
        self.paths.verify_private().map_err(SetupError::Material)?;
        self.code.verify().map_err(SetupError::Material)
    }

    /// The actual approved entrypoint, independent of the original project source.
    #[must_use]
    pub fn entrypoint(&self) -> PathBuf {
        self.code.provider.directory().join("extension.mjs")
    }
}
