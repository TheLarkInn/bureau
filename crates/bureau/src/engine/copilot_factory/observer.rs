//! Admission replies are fenced before callers wake, including cancelled callers.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

use crate::adapters::copilot_factory::artifacts::Artifacts;
use crate::adapters::copilot_factory::launch_provider;
use crate::adapters::copilot_factory::rpc::{CallbackRequest, Handler, RpcFault};
use crate::adapters::copilot_factory::types::{FactoryResumeResult, FactoryRunResult};
use crate::runlog::copilot_factory::{Data, Operation};

use super::journal::Journal;

const fn fault(message: String) -> RpcFault {
    RpcFault {
        code: -32602,
        message,
        data: None,
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> io::Result<T> {
    serde_json::from_value(value.clone()).map_err(io::Error::other)
}

fn rejected(error: &RpcFault) -> bool {
    matches!(
        error.data_code(),
        Some(
            "agent_factories_unavailable"
                | "factory_invalid_args"
                | "factory_invalid_limit"
                | "factory_limits_invalid"
                | "factory_not_found"
                | "invalid_params"
        )
    )
}

fn permission(params: &Value) -> io::Result<Vec<CallbackRequest>> {
    if params["event"]["data"]["resolvedByHook"] == true {
        return Ok(Vec::new());
    }
    let session = params["sessionId"]
        .as_str()
        .ok_or_else(|| io::Error::other("permission session missing"))?;
    let request = params["event"]["data"]["requestId"]
        .as_str()
        .ok_or_else(|| io::Error::other("permission request ID missing"))?;
    Ok(vec![CallbackRequest {
        method: "session.permissions.handlePendingPermissionRequest".to_owned(),
        params: json!({"sessionId": session, "requestId": request,
            "result": {"kind": "reject", "feedback": "Denied by Bureau policy."}}),
    }])
}

pub(super) struct Observer {
    pub(super) journal: Journal,
    pub(super) session: String,
    pub(super) artifacts: Arc<Artifacts>,
    pub(super) request_file: PathBuf,
    pub(super) agent: String,
    pub(super) launched: Arc<AtomicBool>,
    pub(super) allow_provider: bool,
}

impl Observer {
    fn admission(&self, method: &str, value: &Value) -> io::Result<()> {
        let run: FactoryRunResult = if method == "session.factory.resume" {
            let response: FactoryResumeResult = decode(value)?;
            if response.factory_name != self.artifacts.definition.name {
                return Err(io::Error::other("resumed a different factory name"));
            }
            response.run
        } else {
            decode(value)?
        };
        let attempt = run
            .attempt
            .ok_or_else(|| io::Error::other("qualified factory reply lacks attempt"))?;
        self.journal.append(Data::Accepted {
            session_id: self.session.clone(),
            run_id: run.run_id,
            attempt,
        })
    }

    fn accepted_session(&self, value: &Value) -> io::Result<()> {
        if value["sessionId"].as_str() != Some(&self.session) {
            return Err(io::Error::other(
                "SDK acknowledged a different runtime session ID",
            ));
        }
        self.journal.append(Data::SessionAccepted {
            session_id: self.session.clone(),
        })
    }

    fn permission_ack(value: &Value) -> io::Result<()> {
        match value["success"].as_bool() {
            Some(true | false) => Ok(()),
            None => Err(io::Error::other(
                "permission denial acknowledgement lacks boolean success",
            )),
        }
    }

    fn success(&self, method: &str, value: &Value) -> io::Result<()> {
        match method {
            "session.create" | "session.resume" => self.accepted_session(value),
            "session.factory.run" | "session.factory.resume" => self.admission(method, value),
            "session.permissions.handlePendingPermissionRequest" => Self::permission_ack(value),
            _ => Ok(()),
        }
    }

    fn failure(&self, method: &str, error: &RpcFault) -> io::Result<()> {
        if method == "session.factory.run" && rejected(error) {
            self.journal.append(Data::Rejected {
                session_id: self.session.clone(),
                message: format!(
                    "{}: {}",
                    error.data_code().unwrap_or("unknown"),
                    error.message
                ),
            })?;
        }
        if method == "session.permissions.handlePendingPermissionRequest" {
            return Err(io::Error::other(format!(
                "permission rejection failed: {}",
                error.message
            )));
        }
        Ok(())
    }
}

impl Handler for Observer {
    fn request(&mut self, method: &str, params: &Value) -> Result<Value, RpcFault> {
        self.journal
            .check()
            .map_err(|error| fault(error.to_string()))?;
        if method != "extensionLaunchProvider.resolve" {
            return Err(RpcFault {
                code: -32601,
                message: "Method not found".into(),
                data: None,
            });
        }
        if !self.allow_provider {
            return Ok(json!({}));
        }
        let result =
            launch_provider::resolve(&self.artifacts, params, &self.request_file, &self.agent)
                .map_err(|error| fault(error.into()))?;
        if result.get("launch").is_some() {
            self.launched.store(true, Ordering::Release);
        }
        Ok(result)
    }

    fn notification(&mut self, method: &str, params: &Value) -> io::Result<Vec<CallbackRequest>> {
        if method != "session.event" {
            return Ok(Vec::new());
        }
        let kind = params["event"]["type"].as_str().unwrap_or("");
        if kind.starts_with("factory.") || kind.starts_with("permission.") {
            self.journal.append(Data::Notification {
                session_id: self.session.clone(),
                notification: params.clone(),
            })?;
        }
        if kind == "permission.requested" {
            return permission(params);
        }
        Ok(Vec::new())
    }

    fn response(
        &mut self,
        method: &str,
        _params: &Value,
        result: &Result<Value, RpcFault>,
    ) -> io::Result<()> {
        match result {
            Ok(value) => self.success(method, value),
            Err(error) => self.failure(method, error),
        }
    }

    fn before_request(&mut self, method: &str, _params: &Value) -> io::Result<()> {
        self.journal.check()?;
        let operation = match method {
            "session.factory.run" => Operation::Start,
            "session.factory.resume" => Operation::Resume,
            "session.factory.pause" => Operation::Pause,
            "session.factory.cancel" => Operation::Cancel,
            _ => return Ok(()),
        };
        self.artifacts.verify().map_err(io::Error::other)?;
        self.journal.append(Data::Dispatch {
            session_id: self.session.clone(),
            operation,
        })
    }
}
