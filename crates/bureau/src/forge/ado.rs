//! Azure DevOps forge: REST over reqwest with rustls; never `az`.
//!
//! `query` takes `project/repo`; `open_prs`/`create_pr` take the registry
//! URL or bare `project/repo`; `comment`/`set_labels` take `{project}/{id}`.

use async_trait::async_trait;
use serde::Deserialize;

use super::{Error, Forge, Item, Pr, PrRequest};
use crate::contract::Trust;
use crate::process::Secret;

const BATCH: usize = 200;
const MAX_BATCHES: usize = 2;
const FIELDS: &str = "System.Id,System.Title,System.Description,System.Tags";

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct WiqlResult {
    work_items: Vec<WiqlRef>,
}

#[derive(Deserialize)]
struct WiqlRef {
    id: u64,
}

#[derive(Deserialize)]
struct List<T> {
    #[serde(default)]
    value: Vec<T>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawWorkItem {
    id: u64,
    fields: WorkItemFields,
    #[serde(rename = "_links")]
    links: serde_json::Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct WorkItemFields {
    #[serde(rename = "System.Title")]
    title: String,
    #[serde(rename = "System.Description")]
    description: String,
    #[serde(rename = "System.Tags")]
    tags: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawPr {
    pull_request_id: u64,
    source_ref_name: String,
    title: String,
    url: String,
}

/// Azure DevOps API client. `source` uses `project/repo` form; `filter`
/// is a WIQL query passed through verbatim, never parsed.
pub struct AdoForge {
    /// API root, e.g. `https://dev.azure.com/microsoft`; a local test
    /// server in tests.
    pub base_url: String,
    token: Secret,
    client: reqwest::Client,
}

impl AdoForge {
    /// A client against an ADO organization root.
    #[must_use]
    pub fn new(base_url: String, token: Secret) -> Self {
        Self {
            base_url,
            token,
            client: reqwest::Client::new(),
        }
    }

    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, url)
            .header("authorization", basic_auth(&self.token))
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, url)
    }

    fn post(&self, url: &str, body: &serde_json::Value) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::POST, url).json(body)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    async fn wiql_ids(&self, project: &str, filter: &str) -> Result<Vec<u64>, Error> {
        let url = self.url(&format!("/{project}/_apis/wit/wiql?api-version=7.1"));
        let body = serde_json::json!({"query": filter});
        let result: WiqlResult = decode(self.post(&url, &body).send().await?).await?;
        Ok(result.work_items.into_iter().map(|item| item.id).collect())
    }

    async fn hydrate(&self, project: &str, ids: &[u64]) -> Result<Vec<Item>, Error> {
        let csv = ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
        let url = self.url(&format!(
            "/{project}/_apis/wit/workitems?ids={csv}&fields={FIELDS}&api-version=7.1"
        ));
        let list: List<RawWorkItem> = decode(self.get(&url).send().await?).await?;
        let mut items = Vec::new();
        for raw in list.value {
            items.push(work_item(project, raw));
        }
        Ok(items)
    }
}

#[async_trait]
impl Forge for AdoForge {
    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>, Error> {
        let (project, _) = repo_parts(source)?;
        let ids = self.wiql_ids(&project, filter).await?;
        let mut items = Vec::new();
        for chunk in ids.chunks(BATCH).take(MAX_BATCHES) {
            items.extend(self.hydrate(&project, chunk).await?);
        }
        Ok(items)
    }

    async fn open_prs(&self, repo: &str, branch_prefix: &str) -> Result<Vec<Pr>, Error> {
        let (project, repo) = repo_parts(repo)?;
        let path = format!("/{project}/_apis/git/repositories/{repo}/pullrequests");
        let url = self.url(&format!(
            "{path}?searchCriteria.status=active&api-version=7.1"
        ));
        let list: List<RawPr> = decode(self.get(&url).send().await?).await?;
        let want = format!("refs/heads/{branch_prefix}");
        let mut prs = Vec::new();
        for pr in list.value {
            if pr.source_ref_name.starts_with(&want) {
                prs.push(pull_request(&project, &repo, pr));
            }
        }
        Ok(prs)
    }

    async fn create_pr(&self, req: &PrRequest) -> Result<Pr, Error> {
        let (project, repo) = repo_parts(&req.repo)?;
        let url = self.url(&format!(
            "/{project}/_apis/git/repositories/{repo}/pullrequests?api-version=7.1"
        ));
        let body = serde_json::json!({
            "sourceRefName": format!("refs/heads/{}", req.branch),
            "targetRefName": format!("refs/heads/{}", req.base),
            "title": req.title.as_str(),
            "description": req.body.as_str(),
        });
        let raw: RawPr = decode(self.post(&url, &body).send().await?).await?;
        Ok(pull_request(&project, &repo, raw))
    }

    async fn comment(&self, item_id: &str, body: &str) -> Result<(), Error> {
        let (project, id) = item_parts(item_id)?;
        let url = self.url(&format!(
            "/{project}/_apis/wit/workItems/{id}/comments?api-version=7.1-preview.3"
        ));
        let body = serde_json::json!({"text": body});
        let _: serde_json::Value = decode(self.post(&url, &body).send().await?).await?;
        Ok(())
    }

    async fn set_labels(&self, item_id: &str, labels: &[String]) -> Result<(), Error> {
        let (project, id) = item_parts(item_id)?;
        let url = self.url(&format!(
            "/{project}/_apis/wit/workitems/{id}?api-version=7.1"
        ));
        // SET replaces: the patch value is exactly `labels`, not a union.
        let patch = serde_json::json!([{
            "op": "add",
            "path": "/fields/System.Tags",
            "value": labels.join("; "),
        }]);
        let request = self
            .request(reqwest::Method::PATCH, &url)
            .header("content-type", "application/json-patch+json")
            .body(patch.to_string());
        let _: serde_json::Value = decode(request.send().await?).await?;
        Ok(())
    }
}

/// ADO Basic credential: empty user, PAT as password; never logged.
fn basic_auth(token: &Secret) -> String {
    format!(
        "Basic {}",
        base64(format!(":{}", token.expose()).as_bytes())
    )
}

/// RFC 4648 base64; a local twin of `git.rs`'s private encoder.
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    data.chunks(3)
        .flat_map(|chunk| {
            let bits = chunk
                .iter()
                .fold(0usize, |acc, &byte| (acc << 8) | usize::from(byte))
                << (8 * (3 - chunk.len()));
            (0..4).map(move |i| {
                if i <= chunk.len() {
                    char::from(TABLE[(bits >> (18 - 6 * i)) & 63])
                } else {
                    '='
                }
            })
        })
        .collect()
}

/// Splits a registry URL (`.../{project}/_git/{repo}`) or bare `project/repo`.
fn repo_parts(input: &str) -> Result<(String, String), Error> {
    let trimmed = input.trim_matches('/');
    let parts = trimmed
        .rsplit_once("/_git/")
        .map(|(head, repo)| (head.rsplit('/').next().unwrap_or_default(), repo))
        .or_else(|| trimmed.split_once('/'));
    match parts {
        Some((project, repo)) if !project.is_empty() && !repo.is_empty() && !repo.contains('/') => {
            Ok((project.to_owned(), repo.to_owned()))
        }
        _ => Err(Error::Parse(format!("bad repo reference: {input}"))),
    }
}

fn item_parts(item_id: &str) -> Result<(String, u64), Error> {
    let bad = || Error::Parse(format!("bad work item id: {item_id}"));
    let (project, id) = item_id.split_once('/').ok_or_else(bad)?;
    let id = id.parse::<u64>().map_err(|_| bad())?;
    Ok((project.to_owned(), id))
}

/// Non-2xx becomes [`Error::Api`] (≤300 chars); shape mismatch [`Error::Parse`].
async fn decode<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T, Error> {
    let status = response.status();
    if status.is_success() {
        return response
            .json::<T>()
            .await
            .map_err(|error| Error::Parse(error.to_string()));
    }
    let text = response.text().await.unwrap_or_default();
    Err(Error::Api {
        status: status.as_u16(),
        message: text.chars().take(300).collect(),
    })
}

/// Maps a hydrated work item. Trust is `Maintainer`: creating an ADO
/// work item requires write access on the project, so a human with
/// maintainer-grade authority authored the body (DESIGN.md section 9).
fn work_item(project: &str, raw: RawWorkItem) -> Item {
    let url = raw.links["html"]["href"].as_str().unwrap_or_default();
    Item {
        external_id: format!("{project}/{}", raw.id),
        title: raw.fields.title,
        body: raw.fields.description,
        url: url.to_owned(),
        labels: raw
            .fields
            .tags
            .split(';')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(str::to_owned)
            .collect(),
        trust: Trust::Maintainer,
    }
}

/// Maps a pull request. `item_id` stays `None`: ADO's work-item links
/// live behind a separate relations API the runner does not call.
fn pull_request(project: &str, repo: &str, raw: RawPr) -> Pr {
    let branch = raw
        .source_ref_name
        .trim_start_matches("refs/heads/")
        .to_owned();
    Pr {
        number: raw.pull_request_id,
        repo: format!("{project}/{repo}"),
        branch,
        title: raw.title,
        url: raw.url,
        item_id: None,
    }
}
