//! Who a resolved credential is, according to Azure DevOps: the
//! organization-level `connectionData` call, offline against the stub.

use bureau::forge::identity::Reported;
use bureau::forge::{Forge as _, Identity};
use bureau::process::Secret;

use super::stub;

const CONNECTION_PATH: &str = "/_apis/connectionData?api-version=7.1";
const CONNECTED: &str = concat!(
    r#"{"authenticatedUser":{"providerDisplayName":"Bureau Bot","#,
    r#""properties":{"Account":{"$type":"System.String","$value":"bot@example.invalid"}}}}"#,
);
const DISPLAY_ONLY: &str = r#"{"authenticatedUser":{"providerDisplayName":"Bureau Bot"}}"#;
const NO_USER: &str = "{}";
const EMPTY_USER: &str = r#"{"authenticatedUser":{"providerDisplayName":"  "}}"#;

/// The account property answers first; the display name is the fallback.
/// Both requests carry the credential passed in, not the client's own.
#[tokio::test]
async fn identity_reports_the_connected_account() {
    let stub = stub(&[("200 OK", CONNECTED), ("200 OK", DISPLAY_ONLY)]);
    let forge = stub.forge();
    let account = forge
        .identity(&Secret::new("hunter2"))
        .await
        .expect("account");
    let display = forge
        .identity(&Secret::new("hunter2"))
        .await
        .expect("display");
    let requests = stub.requests();
    assert_eq!(
        (
            account,
            display,
            requests[0].path.as_str(),
            requests[0].authorization.as_str(),
        ),
        (
            Reported::Account(Identity::new("bot@example.invalid")),
            Reported::Account(Identity::new("Bureau Bot")),
            CONNECTION_PATH,
            super::AUTH,
        )
    );
}

/// A body with no authenticated user, and one whose account and display
/// name are both blank, are unexpected responses — never a verified
/// empty identity, which would match a declared account by accident.
#[tokio::test]
async fn a_connection_without_an_account_is_an_unexpected_response() {
    let stub = stub(&[("200 OK", NO_USER), ("200 OK", EMPTY_USER)]);
    let forge = stub.forge();
    let missing = forge.identity(&Secret::new("hunter2")).await;
    let empty = forge.identity(&Secret::new("hunter2")).await;
    let kinds = [missing, empty].map(|answer| {
        matches!(
            answer.expect_err("must fail"),
            bureau::forge::Error::Parse(_)
        )
    });
    assert_eq!(kinds, [true, true]);
}

/// A refused personal access token is an API error carrying the status,
/// and the credential value never reaches the message.
#[tokio::test]
async fn a_refused_credential_is_an_api_error_without_the_value() {
    let stub = stub(&[("401 Unauthorized", "access denied")]);
    let error = stub
        .forge()
        .identity(&Secret::new("hunter2"))
        .await
        .expect_err("must fail");
    let summary = match error {
        bureau::forge::Error::Api { status, message } => (status, message.contains("hunter2")),
        other => panic!("wrong error kind: {other:?}"),
    };
    assert_eq!(summary, (401, false));
}
