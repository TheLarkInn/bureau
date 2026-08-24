use std::time::Duration;

use bureau::forge::PrStatus;
use bureau::setup::{Settings, ValidatedConfig};

use super::access;
use super::proposal::{self, Proposal};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
fn completed(status: PrStatus) -> anyhow::Result<Option<String>> {
    match status {
        PrStatus::Open => Ok(None),
        PrStatus::Closed => anyhow::bail!("config pull request closed without merge"),
        PrStatus::Merged {
            commit: Some(commit),
        } => Ok(Some(commit)),
        PrStatus::Merged { commit: None } => {
            anyhow::bail!("forge did not report the exact merged config commit")
        }
    }
}

pub(super) async fn wait(settings: &Settings, proposal: &Proposal) -> anyhow::Result<String> {
    if proposal.is_premerged() {
        crate::cli::out::line(format_args!("config already merged at {}", proposal.commit));
        return Ok(proposal.commit.clone());
    }
    proposal::display(proposal);
    let access = access::verified(settings).await?;
    anyhow::ensure!(
        access.repo == proposal.repo,
        "config pull request repository changed"
    );
    loop {
        let status = access
            .forge
            .pr_status(&proposal.repo, proposal.number)
            .await?;
        if let Some(commit) = completed(status)? {
            return Ok(commit);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

pub(super) async fn validate(
    layout: &bureau::home::Layout,
    settings: &Settings,
    commit: &str,
) -> anyhow::Result<ValidatedConfig> {
    let credential = access::credential(settings).await?;
    let source = bureau::config::GitSource::new(
        settings.config.remote().to_owned(),
        commit.to_owned(),
        settings.config.subdirectory().to_path_buf(),
        &layout.config_cache().join("init-validated"),
        credential,
    );
    let active = source.load().await?;
    anyhow::ensure!(
        active.commit == commit,
        "validated a different config commit"
    );
    Ok(ValidatedConfig {
        source: settings.config.clone(),
        commit: active.commit,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::{access, proposal::Proposal};
    use super::{completed, wait};
    use bureau::forge::PrStatus;
    use bureau::setup::{ConfigSource, Settings};

    #[test]
    fn completion_requires_an_exact_merge_commit() {
        let values = [
            completed(PrStatus::Open).is_ok_and(|value| value.is_none()),
            completed(PrStatus::Closed).is_err(),
            completed(PrStatus::Merged { commit: None }).is_err(),
            completed(PrStatus::Merged {
                commit: Some("abc".to_owned()),
            })
            .is_ok_and(|value| value.as_deref() == Some("abc")),
        ];
        assert_eq!(values, [true; 4]);
    }

    #[tokio::test]
    async fn premerged_proposal_skips_the_merge_wait() {
        let settings = Settings {
            config: ConfigSource::SingleRepository {
                remote: "https://github.com/o/r.git".to_owned(),
                reference: "main".to_owned(),
            },
            credentials: std::collections::BTreeMap::default(),
            plugin: bureau::setup::PluginSettings::default(),
            migration: bureau::setup::MigrationSettings::default(),
        };
        let access = access::Access {
            forge: Arc::new(bureau::forge::fake::FakeForge::default()),
            git: bureau::git::credential_for(
                bureau::config::ForgeKind::Github,
                bureau::process::Secret::new("unused"),
            ),
            repo: "o/r".to_owned(),
        };
        let proposal = Proposal::premerged(&access, "main", "abc".to_owned());
        let commit = wait(&settings, &proposal).await.expect("commit");
        assert_eq!(commit, "abc");
    }
}
