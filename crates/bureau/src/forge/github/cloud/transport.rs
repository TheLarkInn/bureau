use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;

use super::Error;

pub(super) const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// The injected HTTP boundary; offline tests never open a connection.
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, request: reqwest::Request) -> Result<Response, Error>;
}

fn response_headers(response: &reqwest::Response) -> Result<BTreeMap<String, String>, Error> {
    let names = [
        "link",
        "retry-after",
        "x-ratelimit-remaining",
        "x-ratelimit-reset",
    ];
    let mut headers = BTreeMap::new();
    for name in names {
        if let Some(value) = response.headers().get(name) {
            let value = value
                .to_str()
                .map_err(|error| Error::Response(error.to_string()))?;
            headers.insert(name.to_owned(), value.to_owned());
        }
    }
    Ok(headers)
}

fn extend_body(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), Error> {
    if chunk.len() > MAX_BODY_BYTES.saturating_sub(body.len()) {
        return Err(Error::Incomplete(
            "response exceeds the 16 MiB client limit".to_owned(),
        ));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

async fn response_body(mut response: reqwest::Response) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| Error::Transport(error.to_string()))?
    {
        extend_body(&mut body, &chunk)?;
    }
    Ok(body)
}

async fn read_response(response: reqwest::Response, submission: bool) -> Result<Response, Error> {
    let status = response.status().as_u16();
    if submission && response.status().is_success() {
        return Ok(Response {
            status,
            headers: BTreeMap::new(),
            body: Vec::new(),
        });
    }
    let headers = response_headers(&response)?;
    let body = response_body(response).await?;
    Ok(Response {
        status,
        headers,
        body,
    })
}

pub(super) struct Http {
    client: reqwest::Client,
}

impl Http {
    pub(super) fn new() -> Result<Self, Error> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| Error::Transport(error.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Transport for Http {
    async fn send(&self, request: reqwest::Request) -> Result<Response, Error> {
        let submission = request.method() == reqwest::Method::POST;
        let response = self
            .client
            .execute(request)
            .await
            .map_err(|error| Error::Transport(error.to_string()))?;
        read_response(response, submission).await
    }
}
