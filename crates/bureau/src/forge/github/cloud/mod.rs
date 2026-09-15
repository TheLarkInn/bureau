//! Experimental CMC controls, distinct from local Copilot runtime factories.

mod automation;
mod client;
mod error;
mod ids;
mod pages;
mod read;
mod repository;
mod request;
mod task;
mod transport;

pub use automation::{Definition, DisabledState, DispatchEvent, McpServer, Summary, Trigger, User};
pub use client::Client;
pub use error::Error;
pub use ids::{AutomationId, SessionId, TaskId};
pub use repository::{Repository, RepositoryRef};
pub use task::{Events, Principal, Session, Task};
pub use transport::{Response, Transport};

impl super::GitHubForge {
    /// Builds the separate experimental dotcom cloud control client.
    ///
    /// # Errors
    /// Rejects enterprise roots and HTTP-client construction failures.
    pub fn cloud(&self) -> Result<Client, Error> {
        if self.base_url != repository::ORIGIN {
            return Err(Error::Unsupported(
                "cloud controls support dotcom only".to_owned(),
            ));
        }
        Client::new(self.token.clone())
    }
}
