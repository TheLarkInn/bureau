//! GitHub REST forge; registry arguments accept URLs or bare `owner/name`.

mod status;

use async_trait::async_trait;
use reqwest::{Method, RequestBuilder};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::{Error, Forge, Item, Pr, PrRequest, PrStatus};
use crate::contract::Trust;
use crate::process::Secret;

/// `rel="next"` page limit per list call (no unbounded loops).
const MAX_PAGES: u32 = 3;

/// GitHub API client; see module docs for the argument forms.
pub struct GitHubForge {
    /// API root: `https://api.github.com`, or a local server in tests.
    pub base_url: String,
    token: Secret,
    client: reqwest::Client,
}

impl GitHubForge {
    /// A client against `https://api.github.com`.
    #[must_use]
    pub fn new(token: Secret) -> Self {
        Self {
            base_url: "https://api.github.com".to_owned(),
            token,
            client: reqwest::Client::new(),
        }
    }

    /// Points the client at another API root (tests, GitHub Enterprise).
    #[must_use]
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    /// A request carrying the headers every GitHub call needs.
    fn request(&self, method: Method, url: &str) -> RequestBuilder {
        self.client
            .request(method, url)
            .bearer_auth(self.token.expose())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "bureau")
    }

    /// GETs `first`, following up to [`MAX_PAGES`] `rel="next"` links.
    async fn get_pages<T: Send, P: DeserializeOwned>(
        &self,
        first: RequestBuilder,
        into_items: impl Fn(P) -> Vec<T> + Send + Sync,
    ) -> Result<Vec<T>, Error> {
        let mut items = Vec::new();
        let mut next = Some(first);
        for _ in 0..MAX_PAGES {
            let Some(req) = next.take() else { break };
            let resp = req.send().await?;
            next = next_page(&resp).map(|url| self.request(Method::GET, &url));
            items.extend(into_items(json_body(resp).await?));
        }
        Ok(items)
    }
}

/// `owner/name` parsed from a registry URL or a bare `owner/name`.
fn repo_name(source: &str) -> Result<String, Error> {
    let trimmed = source.trim_end_matches('/').trim_end_matches(".git");
    let mut segments = trimmed.rsplit('/');
    let (name, owner) = (segments.next(), segments.next());
    match (owner, name) {
        (Some(owner), Some(name)) if !owner.is_empty() && !name.is_empty() => {
            Ok(format!("{owner}/{name}"))
        }
        _ => Err(Error::Parse(format!(
            "expected a registry URL or owner/name, got {source:?}"
        ))),
    }
}

/// Reads a JSON body; a non-2xx status becomes [`Error::Api`].
async fn json_body<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, Error> {
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        return Err(api_error(status, &bytes));
    }
    serde_json::from_slice(&bytes).map_err(|e| Error::Parse(e.to_string()))
}

/// Checks the status of a call whose body carries no information.
async fn ensure_ok(resp: reqwest::Response) -> Result<(), Error> {
    if resp.status().is_success() {
        return Ok(());
    }
    Err(api_error(resp.status(), &resp.bytes().await?))
}

/// [`Error::Api`] from a status and body, truncated to 300 chars.
fn api_error(status: reqwest::StatusCode, bytes: &[u8]) -> Error {
    let message: String = String::from_utf8_lossy(bytes).chars().take(300).collect();
    Error::Api {
        status: status.as_u16(),
        message,
    }
}

/// The `rel="next"` URL from a `Link` header, when present.
fn next_page(resp: &reqwest::Response) -> Option<String> {
    let link = resp.headers().get("link")?.to_str().ok()?;
    link.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.trim()
            .eq_ignore_ascii_case("rel=\"next\"")
            .then(|| url.trim().trim_matches(|c| c == '<' || c == '>').to_owned())
    })
}

/// Trust grade from GitHub's `author_association` (DESIGN.md layer 9).
fn trust(association: &str) -> Trust {
    match association {
        "OWNER" | "MEMBER" | "COLLABORATOR" => Trust::Maintainer,
        _ => Trust::Untrusted,
    }
}

/// The work item a PR body closes: the first `Closes #N` (any case).
fn closes_item(body: Option<&str>, repo: &str) -> Option<String> {
    let body = body?.to_lowercase();
    let start = body.find("closes #")? + "closes #".len();
    let digits: String = body[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then(|| format!("{repo}#{digits}"))
}

fn issue_number(item_id: &str) -> &str {
    item_id.rsplit('#').next().unwrap_or(item_id)
}

/// `owner/name#number` split into its halves.
fn split_item_id(item_id: &str) -> Result<(String, u64), Error> {
    let parsed = || {
        let (repo, number) = item_id.rsplit_once('#')?;
        Some((repo.to_owned(), number.parse().ok()?))
    };
    parsed().ok_or_else(|| Error::Parse(format!("expected owner/name#number, got {item_id:?}")))
}

#[derive(Deserialize)]
struct SearchPage {
    items: Vec<Issue>,
}

#[derive(Deserialize)]
struct Issue {
    number: u64,
    title: String,
    body: Option<String>,
    html_url: String,
    labels: Vec<Label>,
    author_association: String,
}

impl Issue {
    fn into_item(self, repo: &str) -> Item {
        Item {
            external_id: format!("{repo}#{}", self.number),
            title: self.title,
            body: self.body.unwrap_or_default(),
            url: self.html_url,
            labels: self.labels.into_iter().map(|label| label.name).collect(),
            trust: trust(&self.author_association),
        }
    }
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
struct Pull {
    number: u64,
    title: String,
    html_url: String,
    body: Option<String>,
    head: Head,
}

impl Pull {
    fn into_pr(self, repo: &str) -> Pr {
        Pr {
            number: self.number,
            repo: repo.to_owned(),
            branch: self.head.branch,
            title: self.title,
            url: self.html_url,
            item_id: closes_item(self.body.as_deref(), repo),
        }
    }
}

#[derive(Deserialize)]
struct Head {
    #[serde(rename = "ref")]
    branch: String,
}

#[async_trait]
impl Forge for GitHubForge {
    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>, Error> {
        let repo = repo_name(source)?;
        let q = format!("{filter} repo:{repo}");
        let url = format!("{}/search/issues", self.base_url);
        let first = self
            .request(Method::GET, &url)
            .query(&[("q", q.as_str()), ("per_page", "100")]);
        self.get_pages(first, |page: SearchPage| {
            page.items
                .into_iter()
                .map(|issue| issue.into_item(&repo))
                .collect()
        })
        .await
    }

    async fn open_prs(&self, repo: &str, branch_prefix: &str) -> Result<Vec<Pr>, Error> {
        let repo = repo_name(repo)?;
        let url = format!("{}/repos/{repo}/pulls", self.base_url);
        let first = self
            .request(Method::GET, &url)
            .query(&[("state", "open"), ("per_page", "100")]);
        let prs = self
            .get_pages(first, |page: Vec<Pull>| {
                page.into_iter().map(|pull| pull.into_pr(&repo)).collect()
            })
            .await?;
        Ok(prs
            .into_iter()
            .filter(|pr| pr.branch.starts_with(branch_prefix))
            .collect())
    }

    async fn create_pr(&self, req: &PrRequest) -> Result<Pr, Error> {
        let repo = repo_name(&req.repo)?;
        let body = req.item_id.as_ref().map_or_else(
            || req.body.clone(),
            |item_id| format!("{}\n\nCloses #{}", req.body, issue_number(item_id)),
        );
        let url = format!("{}/repos/{repo}/pulls", self.base_url);
        let payload = serde_json::json!({
            "title": &req.title,
            "head": &req.branch,
            "base": &req.base,
            "body": body,
        });
        let resp = self
            .request(Method::POST, &url)
            .json(&payload)
            .send()
            .await?;
        Ok(json_body::<Pull>(resp).await?.into_pr(&repo))
    }

    async fn pr_status(&self, repo: &str, number: u64) -> Result<PrStatus, Error> {
        status::get(self, repo, number).await
    }

    async fn comment(&self, item_id: &str, body: &str) -> Result<(), Error> {
        let (repo, number) = split_item_id(item_id)?;
        let url = format!("{}/repos/{repo}/issues/{number}/comments", self.base_url);
        let resp = self
            .request(Method::POST, &url)
            .json(&serde_json::json!({ "body": body }))
            .send()
            .await?;
        ensure_ok(resp).await
    }

    async fn set_labels(&self, item_id: &str, labels: &[String]) -> Result<(), Error> {
        let (repo, number) = split_item_id(item_id)?;
        let url = format!("{}/repos/{repo}/issues/{number}/labels", self.base_url);
        let resp = self
            .request(Method::PUT, &url)
            .json(&serde_json::json!({ "labels": labels }))
            .send()
            .await?;
        ensure_ok(resp).await
    }
}
