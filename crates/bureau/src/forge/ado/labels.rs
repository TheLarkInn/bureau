//! Optimistic read-modify-write updates for ADO work-item tags.

use super::{AdoForge, Error, RawWorkItem, decode, item_parts};

pub(super) fn parse(tags: &str) -> Vec<String> {
    tags.split(';')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

fn changed(current: &[String], add: &[String], remove: &[String]) -> Vec<String> {
    let mut labels: Vec<String> = current
        .iter()
        .filter(|label| !remove.contains(label))
        .cloned()
        .collect();
    for label in add {
        if !labels.contains(label) {
            labels.push(label.clone());
        }
    }
    labels
}

async fn current(forge: &AdoForge, item_id: &str) -> Result<RawWorkItem, Error> {
    let (project, id) = item_parts(item_id)?;
    let url = forge.url(&format!(
        "/{project}/_apis/wit/workitems/{id}?fields=System.Tags&api-version=7.1"
    ));
    decode(forge.get(&url).send().await?).await
}

async fn replace(
    forge: &AdoForge,
    item_id: &str,
    rev: u64,
    labels: &[String],
) -> Result<(), Error> {
    let (project, id) = item_parts(item_id)?;
    let url = forge.url(&format!(
        "/{project}/_apis/wit/workitems/{id}?api-version=7.1"
    ));
    let patch = serde_json::json!([
        {"op": "test", "path": "/rev", "value": rev},
        {"op": "add", "path": "/fields/System.Tags", "value": labels.join("; ")},
    ]);
    let request = forge
        .request(reqwest::Method::PATCH, &url)
        .header("content-type", "application/json-patch+json")
        .body(patch.to_string());
    let _: serde_json::Value = decode(request.send().await?).await?;
    Ok(())
}

pub(super) async fn set(forge: &AdoForge, item_id: &str, labels: &[String]) -> Result<(), Error> {
    let (project, id) = item_parts(item_id)?;
    let url = forge.url(&format!(
        "/{project}/_apis/wit/workitems/{id}?api-version=7.1"
    ));
    let patch = serde_json::json!([{
        "op": "add",
        "path": "/fields/System.Tags",
        "value": labels.join("; "),
    }]);
    let request = forge
        .request(reqwest::Method::PATCH, &url)
        .header("content-type", "application/json-patch+json")
        .body(patch.to_string());
    let _: serde_json::Value = decode(request.send().await?).await?;
    Ok(())
}

pub(super) async fn update(
    forge: &AdoForge,
    item_id: &str,
    add: &[String],
    remove: &[String],
) -> Result<(), Error> {
    let raw = current(forge, item_id).await?;
    let next = changed(&parse(&raw.fields.tags), add, remove);
    replace(forge, item_id, raw.rev, &next).await
}
