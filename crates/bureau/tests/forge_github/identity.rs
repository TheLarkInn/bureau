//! Who a resolved credential is, according to GitHub: `GET /user`, and
//! the read-only fallback that settles the 403 GitHub answers for a
//! valid GitHub App installation token. Offline: a loopback server.

use bureau::forge::identity::{Check, Expected, IdentityError, Reported, verify};
use bureau::forge::{Error, Forge as _, Identity};
use bureau::process::Secret;

use super::{TestServer, decoded_target, ok_json, only_request, response};

const USER: &str = r#"{"login":"bureau-bot"}"#;
const REPOS: &str = r#"{"total_count":0,"repositories":[]}"#;
const DENIED: &str = r#"{"message":"Resource not accessible by integration"}"#;
const INSTALLATION_PATH: &str = "/installation/repositories?per_page=1";

/// One credential to check, with or without an expected account.
const fn check<'a>(credential: &'a Secret, expected: Option<&'a str>) -> Check<'a> {
    Check {
        reference: "gh-main",
        credential,
        expected,
        expectation: Expected::Declared,
    }
}

/// The identity call: `GET /user`, signed with the credential passed in
/// rather than the client's own, mapped to the account's login.
#[tokio::test]
async fn identity_reports_the_login_for_the_passed_credential() {
    let server = TestServer::start(|_| vec![ok_json(USER)]);
    let reported = server
        .forge()
        .identity(&Secret::new("other-token"))
        .await
        .expect("identity");
    let (request, lower) = only_request(server);
    let got = (
        reported,
        decoded_target(&request) == "/user",
        lower.contains("authorization: bearer other-token"),
    );
    assert_eq!(
        got,
        (Reported::Account(Identity::new("bureau-bot")), true, true)
    );
}

/// A refused token is an API error carrying the status, which is what
/// lets the caller say "invalid or expired" rather than "unreachable".
#[tokio::test]
async fn a_refused_credential_is_an_api_error() {
    let server = TestServer::start(|_| vec![response(401, r#"{"message":"Bad"}"#, "")]);
    let error = server
        .forge()
        .identity(&Secret::new("test-token"))
        .await
        .expect_err("must fail");
    assert!(
        matches!(error, Error::Api { status: 401, .. }),
        "got {error:?}"
    );
}

/// `/user` is not served to a GitHub App installation token, so its 403
/// proves nothing; one read-only installation listing settles that the
/// value works, and it stays nameless.
#[tokio::test]
async fn a_forbidden_user_call_falls_back_to_the_installation_listing() {
    let server = TestServer::start(|_| vec![response(403, DENIED, ""), ok_json(REPOS)]);
    let reported = server
        .forge()
        .identity(&Secret::new("ghs-installation"))
        .await
        .expect("identity");
    let targets: Vec<String> = server
        .requests()
        .iter()
        .map(|r| decoded_target(r))
        .collect();
    assert_eq!(
        (reported, targets),
        (
            Reported::Unnamed,
            vec!["/user".to_owned(), INSTALLATION_PATH.to_owned()]
        )
    );
}

/// A nameless acceptance is enough when nothing is declared and never
/// enough when something is: an installation token has no account, so a
/// declared identity fails as unverifiable rather than pass unchecked.
#[tokio::test]
async fn an_unnamed_token_passes_undeclared_and_fails_a_declared_identity() {
    let replies = vec![
        response(403, DENIED, ""),
        ok_json(REPOS),
        response(403, DENIED, ""),
        ok_json(REPOS),
    ];
    let server = TestServer::start(|_| replies);
    let forge = server.forge();
    let credential = Secret::new("ghs-installation");
    let permissive = verify(&forge, &check(&credential, None)).await;
    let declared = verify(&forge, &check(&credential, Some("bureau-bot"))).await;
    assert_eq!(
        (
            permissive.ok(),
            matches!(declared, Err(IdentityError::Unverifiable { .. })),
        ),
        (Some(Reported::Unnamed), true)
    );
}

/// A 403 the fallback never confirmed leaves the value unproven, so it
/// is unverifiable — never "invalid or expired", which would send an
/// operator rotating a credential that may be fine.
#[tokio::test]
async fn an_unconfirmed_forbidden_is_not_invalid_or_expired() {
    let server = TestServer::start(|_| vec![response(403, DENIED, ""), response(403, DENIED, "")]);
    let credential = Secret::new("mystery-token");
    let error = verify(&server.forge(), &check(&credential, None))
        .await
        .expect_err("must fail");
    let message = error.to_string();
    assert_eq!(
        (
            matches!(error, IdentityError::Unverifiable { .. }),
            message.contains("invalid or expired"),
            message.contains("mystery-token"),
        ),
        (true, false, false)
    );
}
