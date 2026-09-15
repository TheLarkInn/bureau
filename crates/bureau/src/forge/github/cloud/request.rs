use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue, USER_AGENT};
use serde_json::Value;

use crate::process::Secret;

use super::repository::ORIGIN;
use super::{Error, Response};

pub(super) fn url(segments: &[&str], internal: bool) -> Result<reqwest::Url, Error> {
    let mut url =
        reqwest::Url::parse(ORIGIN).map_err(|error| Error::Response(error.to_string()))?;
    let mut path = url
        .path_segments_mut()
        .map_err(|()| Error::Response("invalid API origin".to_owned()))?;
    if internal {
        path.extend(["cmc_internal", "api"]);
    }
    path.extend(segments);
    drop(path);
    Ok(url)
}

fn authorization(secret: &Secret) -> Result<HeaderValue, Error> {
    if secret.expose().is_empty() {
        return Err(Error::Unsupported(
            "a declared nonempty GitHub credential is required".to_owned(),
        ));
    }
    let mut value = HeaderValue::from_str(&format!("Bearer {}", secret.expose()))
        .map_err(|_| Error::Unsupported("credential is not a valid HTTP credential".to_owned()))?;
    value.set_sensitive(true);
    Ok(value)
}

pub(super) fn build(
    secret: &Secret,
    method: reqwest::Method,
    url: reqwest::Url,
    body: Option<&Value>,
) -> Result<reqwest::Request, Error> {
    let mut request = reqwest::Request::new(method, url);
    let headers = request.headers_mut();
    headers.insert(AUTHORIZATION, authorization(secret)?);
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(USER_AGENT, HeaderValue::from_static("bureau"));
    if let Some(body) = body {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let bytes = serde_json::to_vec(body).map_err(|error| Error::Response(error.to_string()))?;
        *request.body_mut() = Some(bytes.into());
    }
    Ok(request)
}

pub(super) fn page(url: &reqwest::Url, number: u32) -> reqwest::Url {
    let mut url = url.clone();
    url.query_pairs_mut()
        .append_pair("page", &number.to_string())
        .append_pair("per_page", "100");
    url
}

pub(super) fn check_response(response: &Response) -> Result<(), Error> {
    if (200..300).contains(&response.status) {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&response.body)
        .chars()
        .take(300)
        .collect();
    let retry_after_secs = response
        .headers
        .get("retry-after")
        .and_then(|value| value.parse().ok());
    Err(Error::Api {
        status: response.status,
        message,
        retry_after_secs,
    })
}
