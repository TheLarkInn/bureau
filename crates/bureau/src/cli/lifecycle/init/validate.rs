use bureau::setup::ConfigDraft;

use super::files::{self, Temporary};

pub(super) fn preview(draft: &ConfigDraft) {
    for (path, bytes) in &draft.files {
        println!("--- {}", path.display());
        println!("{}", String::from_utf8_lossy(bytes).trim_end());
    }
}

pub(super) fn config(layout: &bureau::home::Layout, draft: &ConfigDraft) -> anyhow::Result<()> {
    let temporary = Temporary::new(layout.config_cache(), "init-preview")?;
    files::materialize(temporary.path(), draft)?;
    bureau::config::Config::load(temporary.path()).map_or_else(
        |errors| {
            let message = errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            anyhow::bail!("generated config is invalid:\n{message}")
        },
        |_| Ok(()),
    )
}
