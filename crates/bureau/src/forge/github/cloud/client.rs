use std::sync::Arc;

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::process::{Secret, scrub_json};

use super::pages::{Collection, Endpoint, MAX_PAGES};
use super::transport::{Http, MAX_BODY_BYTES};
use super::{Error, Principal, Response, Transport, request};

pub struct Client {
    token: Secret,
    transport: Arc<dyn Transport>,
}

impl Client {
    /// Uses Bureau's existing Bearer authentication; CMC compatibility is experimental.
    ///
    /// # Errors
    /// Returns HTTP client construction errors.
    pub fn new(token: Secret) -> Result<Self, Error> {
        Ok(Self::with_transport(token, Arc::new(Http::new()?)))
    }

    /// Injects an HTTP boundary without changing API origin or identity headers.
    #[must_use]
    pub fn with_transport(token: Secret, transport: Arc<dyn Transport>) -> Self {
        Self { token, transport }
    }

    /// Redacts this client's credential from output before it leaves the boundary.
    pub fn scrub(&self, value: &mut Value) {
        scrub_json(value, std::slice::from_ref(&self.token));
    }

    pub(crate) fn credential(&self) -> Secret {
        self.token.clone()
    }

    pub(crate) fn matches_credential(&self, secret: &Secret) -> bool {
        self.token.expose() == secret.expose()
    }

    pub(super) fn redact(&self, error: Error) -> Error {
        error.redacted(&self.token)
    }

    async fn send(&self, request: reqwest::Request) -> Result<Response, Error> {
        let submission = request.method() == Method::POST;
        let mut response = self
            .transport
            .send(request)
            .await
            .map_err(|error| self.redact(error))?;
        request::check_response(&response).map_err(|error| self.redact(error))?;
        if submission {
            response.body.clear();
        }
        if response.body.len() > MAX_BODY_BYTES {
            return Err(Error::Incomplete(
                "response exceeds the 16 MiB client limit".to_owned(),
            ));
        }
        Ok(response)
    }

    pub(super) async fn get(&self, url: reqwest::Url) -> Result<Response, Error> {
        let request = request::build(&self.token, Method::GET, url, None)?;
        self.send(request).await
    }

    pub(super) async fn post(&self, url: reqwest::Url, body: &Value) -> Result<(), Error> {
        let request = request::build(&self.token, Method::POST, url, Some(body))?;
        self.send(request).await?;
        Ok(())
    }

    pub(super) async fn json<T: DeserializeOwned>(&self, url: reqwest::Url) -> Result<T, Error> {
        let response = self.get(url).await?;
        serde_json::from_slice(&response.body)
            .map_err(|error| self.redact(Error::Response(error.to_string())))
    }

    pub(super) async fn collect<T: DeserializeOwned>(
        &self,
        endpoint: &Endpoint,
    ) -> Result<Collection<T>, Error> {
        let mut collection = Collection::new();
        for number in 1..=MAX_PAGES {
            let response = self.get(request::page(&endpoint.url, number)).await?;
            let count = collection
                .extend(&response, endpoint.field)
                .map_err(|error| self.redact(error))?;
            if !endpoint.more(&response, number, count)? {
                return Ok(collection);
            }
        }
        Err(Error::Incomplete(
            "the 100-page client limit was reached with more data".to_owned(),
        ))
    }

    /// Verifies the selected user, not CMC entitlement.
    ///
    /// # Errors
    /// Rejects failed authentication, an invalid user response, or a different user.
    pub async fn authenticate(&self, expected_login: &str) -> Result<Principal, Error> {
        if expected_login.trim().is_empty() {
            return Err(Error::Identity(
                "an explicit expected GitHub login is required".to_owned(),
            ));
        }
        let principal: Principal = self.json(request::url(&["user"], false)?).await?;
        principal.check(expected_login)?;
        Ok(principal)
    }

    /// Rechecks the recorded numeric principal, permitting a login rename.
    ///
    /// # Errors
    /// Rejects failed authentication or a different/invalid principal.
    pub async fn authenticate_id(&self, expected_id: u64) -> Result<Principal, Error> {
        if expected_id == 0 {
            return Err(Error::Identity(
                "a positive recorded GitHub user ID is required".to_owned(),
            ));
        }
        let principal: Principal = self.json(request::url(&["user"], false)?).await?;
        if principal.id != expected_id || principal.login.trim().is_empty() {
            return Err(Error::Identity(
                "credential no longer identifies the recorded GitHub user".to_owned(),
            ));
        }
        Ok(principal)
    }
}
