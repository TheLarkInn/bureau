//! Local bureau home resolution and the fixed on-disk layout.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Environment variable that overrides the local bureau home.
pub const BUREAU_HOME: &str = "BUREAU_HOME";
const USER_HOME: &str = "HOME";

/// Failure to resolve the local bureau home.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// `BUREAU_HOME` was present but empty.
    #[error("BUREAU_HOME must not be empty")]
    EmptyOverride,
    /// Neither an override nor the user's home directory was available.
    #[error("HOME is not set; set BUREAU_HOME explicitly")]
    MissingUserHome,
}

/// Read-only environment access, injectable for deterministic callers.
pub trait Environment {
    /// Returns one environment value without Unicode conversion.
    fn value(&self, name: &str) -> Option<OsString>;
}

/// The current process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn value(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}

/// One expected directory in the local layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Directory {
    /// The local bureau home itself.
    Home,
    /// Credential storage.
    Credentials,
    /// Durable run directories.
    Runs,
    /// Disposable bare checkout mirrors.
    CheckoutCache,
    /// Disposable committed config checkouts.
    ConfigCache,
}

impl Directory {
    /// Every expected directory in stable parent-before-child order.
    pub const ALL: [Self; 5] = [
        Self::Home,
        Self::Credentials,
        Self::Runs,
        Self::CheckoutCache,
        Self::ConfigCache,
    ];
}

/// Resolved local bureau home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    layout: Layout,
}

impl Home {
    /// Resolves the home from the current process environment.
    ///
    /// # Errors
    /// Returns an error for an empty override or unavailable user home.
    pub fn discover() -> Result<Self, Error> {
        Self::from_environment(&ProcessEnvironment)
    }

    /// Resolves the home from injected environment access.
    ///
    /// # Errors
    /// Returns an error for an empty override or unavailable user home.
    pub fn from_environment(environment: &impl Environment) -> Result<Self, Error> {
        let root = match environment.value(BUREAU_HOME) {
            Some(value) if value.is_empty() => return Err(Error::EmptyOverride),
            Some(value) => PathBuf::from(value),
            None => default_root(environment)?,
        };
        Ok(Self::new(root))
    }

    /// Creates a resolved home rooted at an explicit path.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            layout: Layout::new(root),
        }
    }

    /// Returns the fixed local layout.
    #[must_use]
    pub const fn layout(&self) -> &Layout {
        &self.layout
    }
}

fn default_root(environment: &impl Environment) -> Result<PathBuf, Error> {
    environment
        .value(USER_HOME)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| path.join(".bureau"))
        .ok_or(Error::MissingUserHome)
}

/// Every fixed path below the local bureau home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    root: PathBuf,
    settings: PathBuf,
    credentials: PathBuf,
    state_db: PathBuf,
    runs: PathBuf,
    checkout_cache: PathBuf,
    config_cache: PathBuf,
}

impl Layout {
    fn new(root: PathBuf) -> Self {
        Self {
            settings: root.join("settings.yaml"),
            credentials: root.join("credentials"),
            state_db: root.join("state.db"),
            runs: root.join("runs"),
            checkout_cache: root.join("checkout-cache"),
            config_cache: root.join("config-cache"),
            root,
        }
    }

    /// Root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Local lifecycle settings file.
    #[must_use]
    pub fn settings(&self) -> &Path {
        &self.settings
    }

    /// Credential storage directory.
    #[must_use]
    pub fn credentials(&self) -> &Path {
        &self.credentials
    }

    /// Durable state database.
    #[must_use]
    pub fn state_db(&self) -> &Path {
        &self.state_db
    }

    /// Durable run directory.
    #[must_use]
    pub fn runs(&self) -> &Path {
        &self.runs
    }

    /// Disposable checkout cache.
    #[must_use]
    pub fn checkout_cache(&self) -> &Path {
        &self.checkout_cache
    }

    /// Disposable committed config cache.
    #[must_use]
    pub fn config_cache(&self) -> &Path {
        &self.config_cache
    }

    /// Resolves one expected directory.
    #[must_use]
    pub fn directory(&self, directory: Directory) -> &Path {
        match directory {
            Directory::Home => &self.root,
            Directory::Credentials => &self.credentials,
            Directory::Runs => &self.runs,
            Directory::CheckoutCache => &self.checkout_cache,
            Directory::ConfigCache => &self.config_cache,
        }
    }
}
