use std::collections::BTreeMap;
use std::sync::Arc;

use bureau::forge::fake::FakeForge;
use bureau::forge::{Forge as _, PrRequest};

use super::{open_pr_count, prepare_item};
use crate::cli::run::tests::{assignment, config, item};

async fn populate(forge: &FakeForge) {
    for (repo, branch) in [
        ("fake://repo", "bureau/current"),
        ("fake://other", "bureau/unrelated"),
        ("fake://repo", "other/current"),
    ] {
        forge.create_pr(&PrRequest {
            repo: repo.to_owned(),
            branch: branch.to_owned(),
            base: "main".to_owned(),
            title: "Offline change".to_owned(),
            body: String::new(),
            item_id: Some("prior".to_owned()),
        }).await.expect("fake PR");
    }
}

#[tokio::test]
async fn explicit_preparation_carries_the_scoped_forge_pr_count() {
    let forge = Arc::new(FakeForge::new(vec![item("1")]));
    populate(&forge).await;
    let prepared = prepare_item(&config(), &assignment(), "1", forge, BTreeMap::new())
        .await.expect("observation").expect("eligible item");
    assert_eq!(
        (prepared.item.external_id.as_str(), prepared.open_prs, prepared.credentials.len()),
        ("1", 1, 0)
    );
}

#[tokio::test]
async fn an_unknown_primary_repo_does_not_default_to_zero_prs() {
    let mut config = config();
    config.repos.clear();
    let result = open_pr_count(&config, &assignment(), &FakeForge::default()).await;
    assert!(result.expect_err("unobservable primary").to_string()
        .contains("assignment has no primary repo"));
}
