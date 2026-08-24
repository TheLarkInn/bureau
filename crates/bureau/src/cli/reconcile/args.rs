//! Reconcile argument parsing and local-default resolution.

use std::path::PathBuf;

use anyhow::Context as _;
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ForgeArg {
    Github,
    Ado,
}

impl From<ForgeArg> for bureau::config::ForgeKind {
    fn from(value: ForgeArg) -> Self {
        match value {
            ForgeArg::Github => Self::Github,
            ForgeArg::Ado => Self::Ado,
        }
    }
}

pub(super) struct ResolvedArgs {
    pub(super) maintenance_guarded: bool,
    pub(super) maintenance_root: PathBuf,
    pub(super) config_remote: String,
    pub(super) config_ref: String,
    pub(super) config_subdir: PathBuf,
    pub(super) config_credential: Option<String>,
    pub(super) config_forge: bureau::config::ForgeKind,
    pub(super) config_cache: PathBuf,
    pub(super) runs: PathBuf,
    pub(super) state: PathBuf,
    pub(super) cache: PathBuf,
    pub(super) settings: Option<bureau::setup::Settings>,
    pub(super) interval: String,
    pub(super) now: bool,
}

#[derive(Debug, clap::Args)]
pub struct Args {
    #[arg(skip)]
    pub maintenance_guarded: bool,
    #[arg(long, hide = true)]
    pub maintenance_root: Option<PathBuf>,
    /// Local settings file override.
    #[arg(long)]
    pub settings: Option<PathBuf>,
    #[arg(long)]
    pub config_remote: Option<String>,
    #[arg(long)]
    pub config_ref: Option<String>,
    #[arg(long)]
    pub config_subdir: Option<PathBuf>,
    #[arg(long)]
    pub config_credential: Option<String>,
    /// Forge the config remote is addressed on; inferred from the
    /// remote when omitted.
    #[arg(long, value_enum)]
    pub config_forge: Option<ForgeArg>,
    #[arg(long)]
    pub config_cache: Option<PathBuf>,
    #[arg(long)]
    pub runs: Option<PathBuf>,
    #[arg(long)]
    pub state: Option<PathBuf>,
    #[arg(long)]
    pub cache: Option<PathBuf>,
    #[arg(long, default_value = "5m")]
    pub interval: String,
    #[arg(long)]
    pub now: bool,
}

impl Args {
    pub(super) fn resolve(self) -> anyhow::Result<ResolvedArgs> {
        if Self::fully_explicit(&self) {
            self.resolve_full()
        } else {
            self.resolve_home()
        }
    }

    fn resolve_full(mut self) -> anyhow::Result<ResolvedArgs> {
        let settings_path = self.settings.take();
        let maintenance_root = self
            .maintenance_root
            .take()
            .unwrap_or_else(|| explicit_root(settings_path.as_deref(), &self));
        let settings = Self::load_optional(settings_path)?;
        Self::explicit(self, settings, maintenance_root)
    }

    fn resolve_home(mut self) -> anyhow::Result<ResolvedArgs> {
        let home = bureau::home::Home::discover()?;
        let maintenance_root = self
            .maintenance_root
            .take()
            .unwrap_or_else(|| home.layout().root().to_path_buf());
        let path = self
            .settings
            .take()
            .unwrap_or_else(|| home.layout().settings().to_path_buf());
        let settings = Self::load_optional(Some(path))?;
        let source = settings.as_ref().map(|settings| &settings.config);
        Self::default_config_credential(&mut self, settings.as_ref());
        let (remote, reference, subdir) = source_args(&mut self, source)?;
        let paths = local_paths(&mut self, home.layout());
        Ok(resolved(
            self,
            remote,
            reference,
            subdir,
            paths,
            settings,
            maintenance_root,
        ))
    }

    const fn fully_explicit(args: &Self) -> bool {
        args.config_remote.is_some()
            && args.config_cache.is_some()
            && args.runs.is_some()
            && args.state.is_some()
            && args.cache.is_some()
    }

    fn explicit(
        mut args: Self,
        settings: Option<bureau::setup::Settings>,
        maintenance_root: PathBuf,
    ) -> anyhow::Result<ResolvedArgs> {
        let remote = args
            .config_remote
            .take()
            .context("explicit config remote disappeared")?;
        let reference = args.config_ref.take().unwrap_or_else(|| "main".to_owned());
        let subdir = args
            .config_subdir
            .take()
            .unwrap_or_else(|| PathBuf::from(".bureau"));
        let paths = (
            args.config_cache.take().context("missing config cache")?,
            args.runs.take().context("missing runs path")?,
            args.state.take().context("missing state path")?,
            args.cache.take().context("missing checkout cache")?,
        );
        Ok(resolved(
            args,
            remote,
            reference,
            subdir,
            paths,
            settings,
            maintenance_root,
        ))
    }

    fn load_optional(path: Option<PathBuf>) -> anyhow::Result<Option<bureau::setup::Settings>> {
        let Some(path) = path else {
            return Ok(None);
        };
        match bureau::setup::load_settings(&path) {
            Ok(settings) => Ok(Some(settings)),
            Err(bureau::setup::FileError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn default_config_credential(args: &mut Self, settings: Option<&bureau::setup::Settings>) {
        let reference = bureau::config::CONFIG_CREDENTIAL;
        let configured = settings.is_some_and(|value| value.credentials.contains_key(reference));
        if args.config_credential.is_none() && configured {
            args.config_credential = Some(reference.to_owned());
        }
    }
}

/// The forge the config remote is addressed on: the explicit flag when
/// given, and otherwise the one the remote itself implies — the same
/// answer `run` and `doctor` derive from `settings.config`.
fn config_forge(flag: Option<ForgeArg>, remote: &str) -> bureau::config::ForgeKind {
    flag.map_or_else(|| bureau::config::config_forge(remote), Into::into)
}

fn resolved(
    args: Args,
    remote: String,
    reference: String,
    subdir: PathBuf,
    paths: (PathBuf, PathBuf, PathBuf, PathBuf),
    settings: Option<bureau::setup::Settings>,
    maintenance_root: PathBuf,
) -> ResolvedArgs {
    ResolvedArgs {
        maintenance_guarded: args.maintenance_guarded,
        maintenance_root,
        config_forge: config_forge(args.config_forge, &remote),
        config_remote: remote,
        config_ref: reference,
        config_subdir: subdir,
        config_credential: args.config_credential,
        config_cache: paths.0,
        runs: paths.1,
        state: paths.2,
        cache: paths.3,
        settings,
        interval: args.interval,
        now: args.now,
    }
}

fn explicit_root(settings: Option<&std::path::Path>, args: &Args) -> PathBuf {
    let path = settings.or(args.config_cache.as_deref());
    path.and_then(std::path::Path::parent)
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

fn source_args(
    args: &mut Args,
    source: Option<&bureau::setup::ConfigSource>,
) -> anyhow::Result<(String, String, PathBuf)> {
    let remote = args
        .config_remote
        .take()
        .or_else(|| source.map(|value| value.remote().to_owned()))
        .context("set --config-remote or run `bureau init`")?;
    let reference = args
        .config_ref
        .take()
        .or_else(|| source.map(|value| value.reference().to_owned()))
        .unwrap_or_else(|| "main".to_owned());
    let subdir = args
        .config_subdir
        .take()
        .or_else(|| source.map(|value| value.subdirectory().to_path_buf()))
        .unwrap_or_else(|| PathBuf::from(".bureau"));
    Ok((remote, reference, subdir))
}

fn local_paths(
    args: &mut Args,
    layout: &bureau::home::Layout,
) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let config = args
        .config_cache
        .take()
        .unwrap_or_else(|| layout.config_cache().to_path_buf());
    let runs = args
        .runs
        .take()
        .unwrap_or_else(|| layout.runs().to_path_buf());
    let state = args
        .state
        .take()
        .unwrap_or_else(|| layout.state_db().to_path_buf());
    let cache = args
        .cache
        .take()
        .unwrap_or_else(|| layout.checkout_cache().to_path_buf());
    (config, runs, state, cache)
}
