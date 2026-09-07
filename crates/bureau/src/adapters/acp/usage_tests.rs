use agent_client_protocol::schema::v1::{Cost, SessionNotification, SessionUpdate, UsageUpdate};
use agent_client_protocol::{Client, ConnectionTo, Result, UntypedMessage};

use super::test_peer::{Case, run};

const REPORTS: &[(&str, Option<f64>)] = &[
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100}}"#,
        Some(0.42),
    ),
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100,"_meta":{"rateLimit":{"remaining":2}}}}"#,
        Some(0.42),
    ),
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100,"cost":null}}"#,
        None,
    ),
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100,"cost":"invalid"}}"#,
        None,
    ),
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100,"cost":{}}}"#,
        None,
    ),
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100,"cost":{"amount":"invalid","currency":"USD"}}}"#,
        None,
    ),
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100,"cost":{"amount":null,"currency":"USD"}}}"#,
        None,
    ),
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100,"cost":{"amount":-1,"currency":"USD"}}}"#,
        None,
    ),
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100,"cost":{"amount":2,"currency":"EUR"}}}"#,
        None,
    ),
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100,"cost":{"amount":0.65,"currency":"USD"}}}"#,
        Some(0.65),
    ),
    (
        r#"{"sessionId":"session","update":{"sessionUpdate":"usage_update","used":45,"size":100,"cost":{"amount":0,"currency":"USD"}}}"#,
        Some(0.0),
    ),
    (
        r#"{"sessionId":"foreign","update":{"sessionUpdate":"usage_update","used":45,"size":100,"cost":null}}"#,
        Some(0.42),
    ),
];

pub(super) fn send(connection: &ConnectionTo<Client>, report: &str) -> Result<()> {
    connection.send_notification(SessionNotification::new(
        "session",
        SessionUpdate::UsageUpdate(UsageUpdate::new(40, 100).cost(Cost::new(0.42, "USD"))),
    ))?;
    let params: serde_json::Value = serde_json::from_str(report).expect("usage fixture");
    connection.send_notification(UntypedMessage::new("session/update", params)?)?;
    connection.send_notification(SessionNotification::new(
        "session",
        SessionUpdate::UsageUpdate(UsageUpdate::new(50, 100)),
    ))
}

#[tokio::test]
async fn wire_usage_omissions_preserve_cost_but_invalid_reports_clear_it() {
    for (report, expected) in REPORTS {
        let (completion, _, usage) =
            tokio::time::timeout(std::time::Duration::from_secs(3), run(Case::Usage(report)))
                .await
                .expect("bounded peer");
        completion.expect("ACP exchange");
        assert_eq!(
            (usage.cost_usd, usage.cost_basis.as_deref()),
            (*expected, expected.map(|_| "acp_cumulative_session_usd")),
            "{report}"
        );
    }
}
