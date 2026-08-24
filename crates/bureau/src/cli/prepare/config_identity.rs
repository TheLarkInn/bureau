//! The reserved config credential's declared identity, verified before
//! anything fetches the reviewed config with it.
//!
//! That fetch happens before assignment config is loaded, so no
//! registry is available to authorize a host for this reference: the
//! config remote is the only host that can answer for it there. `run`,
//! `retry`, the reconcile daemon, and `init` all check it here before
//! their first call; `validate` leaves the declaration exempt from the
//! orphan rule because this check — and `doctor`'s read-only twin — is
//! what enforces it. A registered repo may still name the same
//! reference, and then it authorizes its own host for a run's check,
//! exactly as it does for any other credential.

use anyhow::Context as _;

use bureau::config::{CONFIG_CREDENTIAL, ForgeKind, config_forge, config_repo};
use bureau::forge::Forge;
use bureau::forge::identity::{Check, Expected, verify};
use bureau::process::Secret;
use bureau::setup::Settings;

use super::repo_forge;

/// The config credential a fetch is about to use: the reference
/// actually configured, and the remote it will be sent to.
pub struct ConfigRemote<'a> {
    /// Credential reference, named in every failure.
    pub reference: &'a str,
    /// Remote the reviewed config is fetched from.
    pub remote: &'a str,
    /// The forge that remote is addressed on.
    pub forge: ForgeKind,
}

/// A remote this runner cannot address, said without echoing the URL:
/// a remote may carry a value in its own userinfo.
fn unaddressable(reference: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "credential `{reference}` could not be verified: the configured config remote is not a supported forge reference"
    )
}

/// Checks one resolved value against one client, secret-free.
async fn against(
    client: &dyn Forge,
    reference: &str,
    credential: &Secret,
    declared: &str,
) -> anyhow::Result<()> {
    let check = Check {
        reference,
        credential,
        expected: Some(declared),
        expectation: Expected::Declared,
    };
    verify(client, &check).await?;
    Ok(())
}

/// Verifies the config credential against the forge root its remote
/// implies, before anything fetches with it. Fails when the forge
/// refuses the value, cannot answer for it, or names another account —
/// including a token it accepts without naming. Neither the value nor a
/// forge response body enters the error.
async fn verify_declared(
    target: &ConfigRemote<'_>,
    credential: &Secret,
    declared: &str,
) -> anyhow::Result<()> {
    let repo = config_repo(target.remote, target.forge);
    let client =
        repo_forge(&repo, credential.clone()).map_err(|_| unaddressable(target.reference))?;
    against(client.as_ref(), target.reference, credential, declared)
        .await
        .with_context(|| {
            format!(
                "verifying credential `{}` against the config remote",
                target.reference
            )
        })
}

/// The one rule every fetch path shares: a configured config credential
/// declaring an identity must be that account before the fetch, and one
/// declaring none leaves the fetch its own validity check.
async fn verify_configured(
    settings: &Settings,
    target: &ConfigRemote<'_>,
    credential: &Secret,
) -> anyhow::Result<()> {
    let Some(declared) = settings.declared_identity(target.reference) else {
        return Ok(());
    };
    verify_declared(target, credential, declared).await
}

/// The same rule where settings may be absent: the reconcile daemon can
/// run without them, and then there is no declaration to enforce.
///
/// # Errors
/// Propagates the verification failure, secret-free.
pub async fn verify_optional(
    settings: Option<&Settings>,
    target: &ConfigRemote<'_>,
    credential: &Secret,
) -> anyhow::Result<()> {
    let Some(settings) = settings else {
        return Ok(());
    };
    verify_configured(settings, target, credential).await
}

/// The reserved config credential these settings declare: resolved
/// once, and proved to be the account its declaration names before the
/// caller signs a clone, a push, or a pull request with it.
///
/// # Errors
/// Propagates resolution and verification failures, secret-free.
pub async fn verified_secret(settings: &Settings) -> anyhow::Result<(ForgeKind, Secret)> {
    let remote = settings.config.remote();
    let forge = config_forge(remote);
    let secret = bureau::credential::resolve(settings, CONFIG_CREDENTIAL)?;
    let target = ConfigRemote {
        reference: CONFIG_CREDENTIAL,
        remote,
        forge,
    };
    verify_configured(settings, &target, &secret).await?;
    Ok((forge, secret))
}

#[cfg(test)]
mod tests {
    use super::{Settings, against};
    use bureau::forge::fake::FakeForge;
    use bureau::process::Secret;

    const SETTINGS_YAML: &str = concat!(
        "config:\n  kind: separate_repository\n",
        "  remote: https://github.com/acme/config.git\n  reference: main\n",
        "credentials:\n",
        "  config:\n    source: environment\n",
        "    variable: BUREAU_CONFIG_TOKEN\n    identity: bureau-bot\n",
        "  other:\n    source: file\n    path: /run/credentials/other\n",
    );

    fn settings() -> Settings {
        serde_yaml_ng::from_str(SETTINGS_YAML).expect("settings parse")
    }

    /// The declaration is read from the credential the fetch will use,
    /// and a credential declaring nothing is left to the fetch itself.
    #[test]
    fn only_a_declared_identity_is_enforced() {
        let settings = settings();
        let found = (
            settings.declared_identity("config"),
            settings.declared_identity("other"),
            settings.declared_identity("absent"),
        );
        assert_eq!(found, (Some("bureau-bot"), None, None));
    }

    /// A value belonging to another account fails, naming the
    /// reference and both accounts and never the value itself.
    #[tokio::test]
    async fn a_wrong_account_fails_without_echoing_the_value() {
        let forge = FakeForge::default();
        forge.verify_identity_as("someone-else");
        let credential = Secret::new("config-token-value");
        let error = against(&forge, "config", &credential, "bureau-bot")
            .await
            .expect_err("a wrong account must fail");
        let message = format!("{error:#}");
        assert_eq!(
            (
                message.contains("config") && message.contains("bureau-bot"),
                message.contains("someone-else"),
                message.contains("config-token-value"),
            ),
            (true, true, false)
        );
    }

    /// A token the forge accepts without naming — a GitHub App
    /// installation token — cannot satisfy a declared identity.
    #[tokio::test]
    async fn an_unnamed_token_cannot_satisfy_the_declaration() {
        let forge = FakeForge::default();
        forge.accept_identity_unnamed();
        let error = against(&forge, "config", &Secret::new("ghs-x"), "bureau-bot")
            .await
            .expect_err("an unnamed token must fail");
        assert!(
            format!("{error:#}").contains("names no account"),
            "{error:#}"
        );
    }

    /// The forge that answers is the one the config remote implies, and
    /// it is asked about exactly the value the fetch would sign with.
    #[tokio::test]
    async fn the_declared_account_passes_and_is_asked_once() {
        let forge = FakeForge::default();
        forge.verify_identity_as("bureau-bot");
        let passed = against(&forge, "config", &Secret::new("value"), "bureau-bot").await;
        assert_eq!((passed.is_ok(), forge.identified().len()), (true, 1));
    }
}
