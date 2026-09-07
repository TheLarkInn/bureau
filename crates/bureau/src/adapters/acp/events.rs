//! Per-attempt ACP evidence and rejection of additional permissions.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use agent_client_protocol::schema::v1::{
    ContentBlock, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionId, SessionNotification,
    SessionUpdate, UsageUpdate,
};
use agent_client_protocol::{Error, Result};

use super::capture::Capture;
use super::selection;
use crate::adapters::Usage;
use crate::process::{ScrubWriter, Secret, SharedLog};

fn writer(writer: &mut Option<ScrubWriter<Capture>>) -> Result<&mut ScrubWriter<Capture>> {
    writer
        .as_mut()
        .ok_or_else(|| selection::error("ACP event after capture finished"))
}

pub(super) type SharedEvents = Arc<Mutex<Events>>;

pub(super) struct Events {
    pub(super) session: Option<SessionId>,
    pub(super) agent: Option<String>,
    pub(super) usage: Usage,
    pub(super) cancel: Option<PathBuf>,
    failure: Option<Error>,
    response: Option<ScrubWriter<Capture>>,
    progress: Option<ScrubWriter<Capture>>,
}

impl Events {
    pub(super) fn new(
        provider: &str,
        secrets: &[Secret],
        log: Option<SharedLog>,
        cancel: Option<PathBuf>,
    ) -> SharedEvents {
        Arc::new(Mutex::new(Self {
            session: None,
            agent: None,
            usage: Usage::unknown(provider),
            cancel,
            failure: None,
            response: Some(Capture::scrubbed(log.clone(), secrets)),
            progress: Some(Capture::scrubbed(log, secrets)),
        }))
    }

    pub(super) fn receive(&mut self, notification: SessionNotification) {
        // The SDK logs notification-handler errors rather than ending the exchange.
        if let Err(error) = self.update(notification) {
            self.failure.get_or_insert(error);
        }
    }

    pub(super) fn check(&self) -> Result<()> {
        self.failure.clone().map_or(Ok(()), Err)
    }

    pub(super) fn update(&mut self, notification: SessionNotification) -> Result<()> {
        if self.session.as_ref() != Some(&notification.session_id) {
            return Ok(());
        }
        self.apply(notification.update)
    }

    fn apply(&mut self, update: SessionUpdate) -> Result<()> {
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => self.message(chunk.content),
            SessionUpdate::AgentThoughtChunk(chunk) => self.thought(chunk.content),
            SessionUpdate::ToolCall(tool) => self.progress(&format!("{}\n", tool.title)),
            SessionUpdate::UsageUpdate(update) => {
                self.measure(&update);
                Ok(())
            }
            SessionUpdate::ConfigOptionUpdate(update) => self.configuration(&update.config_options),
            _ => self.progress("ACP session progress\n"),
        }
    }

    fn thought(&mut self, content: ContentBlock) -> Result<()> {
        if let ContentBlock::Text(text) = content {
            self.progress(&text.text)?;
        }
        Ok(())
    }

    fn message(&mut self, content: ContentBlock) -> Result<()> {
        if let ContentBlock::Text(text) = content {
            writer(&mut self.response)?
                .write_all(text.text.as_bytes())
                .map_err(Error::into_internal_error)?;
        }
        Ok(())
    }

    fn configuration(
        &self,
        options: &[agent_client_protocol::schema::v1::SessionConfigOption],
    ) -> Result<()> {
        if let Some(agent) = &self.agent {
            selection::selected(options, agent)?;
        }
        Ok(())
    }

    fn measure(&mut self, update: &UsageUpdate) {
        self.usage.cost_usd = update
            .cost
            .as_ref()
            .filter(|cost| cost.currency == "USD")
            .map(|cost| cost.amount)
            .filter(|amount| amount.is_finite() && *amount >= 0.0);
        self.usage.cost_basis = self
            .usage
            .cost_usd
            .map(|_| "acp_cumulative_session_usd".to_owned());
    }

    pub(super) fn progress(&mut self, text: &str) -> Result<()> {
        writer(&mut self.progress)?
            .write_all(text.as_bytes())
            .map_err(Error::into_internal_error)
    }

    pub(super) fn finish(&mut self) -> Result<Vec<u8>> {
        let progress = self
            .progress
            .take()
            .ok_or_else(|| selection::error("ACP capture finished twice"))?;
        progress.finish().map_err(Error::into_internal_error)?;
        let response = self
            .response
            .take()
            .ok_or_else(|| selection::error("ACP capture finished twice"))?;
        Ok(response
            .finish()
            .map_err(Error::into_internal_error)?
            .bytes()
            .to_vec())
    }
}

pub(super) fn lock(events: &SharedEvents) -> Result<MutexGuard<'_, Events>> {
    events
        .lock()
        .map_err(|_| selection::error("ACP event lock poisoned"))
}

pub(super) fn permission(
    events: &SharedEvents,
    request: &RequestPermissionRequest,
) -> Result<RequestPermissionResponse> {
    let mut state = lock(events)?;
    state.progress("ACP additional permission rejected; native role grants remain unchanged\n")?;
    if state.cancel.as_deref().is_some_and(std::path::Path::exists) {
        return Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ));
    }
    drop(state);
    let option = request
        .options
        .iter()
        .find(|option| option.kind == PermissionOptionKind::RejectOnce)
        .ok_or_else(|| {
            selection::error("ACP permission request supplied no per-request rejection option")
        })?;
    Ok(RequestPermissionResponse::new(
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            option.option_id.clone(),
        )),
    ))
}
