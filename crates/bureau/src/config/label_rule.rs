//! `label_rules/<name>.yaml` — bounded forge-label reconciliation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::files::{Named, Repo};
use crate::forge::ForgeKind;

/// A forge work-item query for a label rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelRuleWork {
    /// Which forge holds the work items.
    pub forge: ForgeKind,
    /// Forge-specific source, such as `owner/repo`.
    pub source: String,
    /// Forge-native query, passed through verbatim.
    pub filter: String,
}

/// The condition that makes a label rule apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelRuleCondition {
    /// Every issue listed by the forge as blocking the item is closed.
    DependenciesClosed,
}

/// Mutation limits for one label rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelRuleLimits {
    /// Label-update attempts allowed in a rolling hour.
    pub max_updates_per_hour: u32,
}

fn normalized_repo(value: &str) -> Option<String> {
    crate::forge::repository::parse(value).map(|location| location.identity())
}

/// A repeatedly evaluated rule that reconciles forge-owned labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelRule {
    /// Must match the file stem.
    pub name: String,
    /// Work items evaluated on every reconcile pass.
    pub work: LabelRuleWork,
    /// Condition that permits the label update.
    pub when: LabelRuleCondition,
    /// Labels added when the condition is satisfied.
    pub add_labels: Vec<String>,
    /// Labels removed when the condition is satisfied.
    pub remove_labels: Vec<String>,
    /// Hard mutation bound.
    pub limits: LabelRuleLimits,
}

impl LabelRule {
    /// Stable, case-insensitive repository identity for durable audits.
    #[must_use]
    pub fn source_identity(&self) -> String {
        normalized_repo(&self.work.source)
            .unwrap_or_else(|| self.work.source.trim().to_ascii_lowercase())
    }

    /// Registry repo whose credential authorizes this rule's source.
    #[must_use]
    pub fn source_repo<'a>(&self, repos: &'a BTreeMap<String, Repo>) -> Option<&'a Repo> {
        let source = normalized_repo(&self.work.source)?;
        repos.values().find(|repo| {
            repo.forge == self.work.forge
                && normalized_repo(&repo.url).is_some_and(|candidate| candidate == source)
        })
    }
}

impl Named for LabelRule {
    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{LabelRule, LabelRuleCondition, LabelRuleLimits, LabelRuleWork, normalized_repo};
    use crate::config::{Access, Repo};
    use crate::forge::ForgeKind;

    #[test]
    fn bare_and_dot_com_urls_share_an_identity() {
        let bare = normalized_repo("TheLarkInn/bureau");
        let url = normalized_repo("https://github.com/TheLarkInn/bureau");
        let expected = Some("github.com:443/thelarkinn/bureau".to_owned());
        assert_eq!((bare, url), (expected.clone(), expected));
    }

    #[test]
    fn source_selects_the_matching_registered_repo() {
        let rule = LabelRule {
            name: "graduate".to_owned(),
            work: LabelRuleWork {
                forge: ForgeKind::Github,
                source: "TheLarkInn/bureau".to_owned(),
                filter: "is:issue".to_owned(),
            },
            when: LabelRuleCondition::DependenciesClosed,
            add_labels: vec!["ready".to_owned()],
            remove_labels: vec!["blocked".to_owned()],
            limits: LabelRuleLimits {
                max_updates_per_hour: 20,
            },
        };
        let repo = Repo {
            url: "https://github.com/TheLarkInn/bureau".to_owned(),
            forge: ForgeKind::Github,
            access: Access::Read,
            credential: "github".to_owned(),
        };
        assert!(
            rule.source_repo(&BTreeMap::from([("bureau".to_owned(), repo)]))
                .is_some()
        );
    }
}
