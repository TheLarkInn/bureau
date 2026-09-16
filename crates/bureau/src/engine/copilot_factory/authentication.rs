//! Private SDK sandbox auth policy and unsupported executable inputs.
//!
//! Supported execution uses the pinned stock CLI and reviewed local directory
//! plugins contributing agents, skills, commands, and the fixed Bureau MCP
//! declaration. Hooks, inline agent MCP, arbitrary MCP, and LSP configurations
//! are unsupported, not deferred to a post-initialization catalogue check.
//! This gate checks private, workspace, and pinned-plugin inputs before each
//! `RuntimeOpened` event; the session tool ceiling also excludes `builtin:lsp`.
//!
//! Stock secret registration filters inherited environments, not launch-profile
//! overrides. Session shell credentials and this private sandbox auth policy
//! independently deny reinjection. Broker strings are checked separately before
//! initialization because direct mode still expands variables. Static factory
//! arguments are opaque and never participate in that broker validation.
//!
//! These controls do not isolate arbitrary approved code from its runtime process
//! or attenuate the model identity inherited by native factory child sessions.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

use crate::adapters::copilot_factory::context_types::PinnedContext;

const SETTINGS: &[u8] = b"{\"sandbox\":{\"auth\":{\"git\":false,\"gh\":false}}}\n";

fn read_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("inspect factory auth policy: {error}")),
    };
    if !metadata.is_file() {
        return Err(format!(
            "factory auth policy is not a plain file: {}",
            path.display()
        ));
    }
    fs::read(path)
        .map(Some)
        .map_err(|error| format!("read factory auth policy: {error}"))
}

fn absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("inspect factory executable configuration: {error}")),
        Ok(_) => Err(format!(
            "unapproved factory executable configuration: {}",
            path.display()
        )),
    }
}

fn legacy(home: &Path) -> Result<(), String> {
    let Some(bytes) = read_file(&home.join("config.json"))? else {
        return Ok(());
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid private native config: {error}"))?;
    let fields = value
        .as_object()
        .ok_or("private native config must be an object")?;
    // Legacy sandbox replaces the whole settings block; even {} or null can mask both denials.
    for key in [
        "sandbox",
        "hooks",
        "extensions",
        "enabledPlugins",
        "extraKnownMarketplaces",
    ] {
        if fields.contains_key(key) {
            return Err(format!(
                "private native config cannot override factory policy: {key}"
            ));
        }
    }
    Ok(())
}

fn sources(context: &PinnedContext) -> Result<(), String> {
    absent(&context.worktree.join(".github/lsp.json"))?;
    for plugin in &context.plugins {
        for path in [
            ".lsp.json",
            "lsp.json",
            ".github/lsp.json",
            "com.github.copilot/lsp.json",
            "com.github.copilot/hooks/hooks.json",
        ] {
            absent(&plugin.directory.join(path))?;
        }
    }
    Ok(())
}

pub(super) fn create(home: &Path) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(home.join("settings.json"))
        .map_err(|error| format!("create private factory auth policy: {error}"))?;
    file.write_all(SETTINGS)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("persist private factory auth policy: {error}"))?;
    fs::File::open(home)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("persist private factory auth directory: {error}"))
}

pub(super) fn verify(home: &Path, context: &PinnedContext) -> Result<(), String> {
    if read_file(&home.join("settings.json"))?.as_deref() != Some(SETTINGS) {
        return Err(
            "private factory auth policy is missing or changed; preserve the session".into(),
        );
    }
    // The SDK contract excludes repo sandbox overrides; managed auth can only become stricter.
    legacy(home)?;
    absent(&home.join("lsp.json"))?;
    sources(context)
}
