use agent_client_protocol::schema::v1::SessionNotification;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Debug, Serialize, agent_client_protocol::JsonRpcNotification)]
#[notification(method = "session/update")]
pub(super) struct Notification {
    #[serde(flatten)]
    pub(super) session: SessionNotification,
    #[serde(skip)]
    pub(super) cost_reported: bool,
}

impl<'de> Deserialize<'de> for Notification {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let params = serde_json::Value::deserialize(deserializer)?;
        // The SDK defaults malformed optional costs to None. Retain presence so
        // only omission, not an invalid/null report, preserves earlier usage.
        let cost_reported = params.pointer("/update/cost").is_some();
        let session = SessionNotification::deserialize(params).map_err(serde::de::Error::custom)?;
        Ok(Self {
            session,
            cost_reported,
        })
    }
}
