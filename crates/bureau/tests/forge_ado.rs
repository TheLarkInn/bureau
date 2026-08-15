//! Offline ADO forge tests (DESIGN.md section 12): loopback TCP stub.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use bureau::contract::Trust;
use bureau::forge::ado::AdoForge;
use bureau::forge::{Error, Forge, Item, PrRequest};
use bureau::process::Secret;

/// `Basic` of `:hunter2` — the empty-user PAT form ADO expects.
const AUTH: &str = "Basic Omh1bnRlcjI=";
const WIQL: &str = "SELECT [System.Id] FROM WorkItems WHERE [System.Tags] CONTAINS 'agent'";
const REGISTRY_URL: &str = "https://dev.azure.com/microsoft/Odsp/_git/odsp-web";
const WIQL_PATH: &str = "/Odsp/_apis/wit/wiql?api-version=7.1";
const HYDRATE_PATH: &str = "/Odsp/_apis/wit/workitems?ids=1,2&fields=System.Id,System.Title,System.Description,System.Tags&api-version=7.1";
const PRS_PATH: &str = "/Odsp/_apis/git/repositories/odsp-web/pullrequests?searchCriteria.status=active&api-version=7.1";
const CREATE_PATH: &str = "/Odsp/_apis/git/repositories/odsp-web/pullrequests?api-version=7.1";
const COMMENT_PATH: &str = "/Odsp/_apis/wit/workItems/42/comments?api-version=7.1-preview.3";
const LABELS_PATH: &str = "/Odsp/_apis/wit/workitems/42?api-version=7.1";

const WIQL_REPLY: &str = r#"{"workItems":[{"id":1},{"id":2}]}"#;
const ITEMS_REPLY: &str = concat!(
    r#"{"value":[{"id":1,"fields":{"System.Title":"Crash on save","#,
    r#""System.Description":"Null handle","System.Tags":"crash; ui"},"#,
    r#""_links":{"html":{"href":"https://example/1"}}},{"id":2,"#,
    r#""fields":{"System.Title":"Slow boot","System.Description":"Ten minutes"}}]}"#,
);
const PRS_REPLY: &str = concat!(
    r#"{"value":[{"pullRequestId":7,"sourceRefName":"refs/heads/bureau/fix-1","#,
    r#""title":"Fix one","url":"https://example/pr/7"},{"pullRequestId":8,"#,
    r#""sourceRefName":"refs/heads/other/work","title":"Other","url":"https://example/pr/8"}]}"#,
);
const CREATED_REPLY: &str = r#"{"pullRequestId":9,"sourceRefName":"refs/heads/bureau/fix","title":"Fix","url":"https://example/pr/9"}"#;

/// A request exactly as the stub received it.
#[derive(Clone)]
struct Request {
    method: String,
    path: String,
    authorization: String,
    body: String,
}

/// A loopback server answering each canned `(status, body)` once.
struct Stub {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl Stub {
    fn forge(&self) -> AdoForge {
        AdoForge::new(self.url.clone(), Secret::new("hunter2"))
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().expect("requests lock").clone()
    }
}

fn stub(responses: &[(&str, &str)]) -> Stub {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    let requests: Arc<Mutex<Vec<Request>>> = Arc::default();
    let seen = Arc::clone(&requests);
    let replies: Vec<(String, String)> = responses
        .iter()
        .map(|(status, body)| ((*status).to_owned(), (*body).to_owned()))
        .collect();
    std::thread::spawn(move || {
        for (status, body) in replies {
            serve_once(&listener, &seen, &status, &body);
        }
    });
    Stub { url, requests }
}

fn serve_once(listener: &TcpListener, seen: &Mutex<Vec<Request>>, status: &str, body: &str) {
    let (mut stream, _) = listener.accept().expect("accept");
    seen.lock().expect("seen lock").push(read_request(&stream));
    let head = format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\n");
    let response = format!(
        "{head}content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).expect("write");
}

fn read_request(stream: &TcpStream) -> Request {
    let mut reader = BufReader::new(stream);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).expect("header line");
        if read == 0 || line == "\r\n" {
            break;
        }
        head.push_str(&line);
    }
    let mut body = vec![0; content_length(&head)];
    reader.read_exact(&mut body).expect("body");
    parse_request(&head, &body)
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .map(str::to_owned)
        })
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn parse_request(head: &str, body: &[u8]) -> Request {
    let mut start = head.lines().next().unwrap_or_default().split_whitespace();
    Request {
        method: start.next().unwrap_or_default().to_owned(),
        path: start.next().unwrap_or_default().to_owned(),
        authorization: header(head, "authorization").to_owned(),
        body: String::from_utf8_lossy(body).into_owned(),
    }
}

fn header<'a>(head: &'a str, name: &str) -> &'a str {
    head.lines()
        .find(|line| line.len() > name.len() && line[..name.len()].eq_ignore_ascii_case(name))
        .map_or("", |line| line[name.len() + 1..].trim())
}

fn expected_items() -> Vec<Item> {
    let full = Item {
        external_id: "Odsp/1".to_owned(),
        title: "Crash on save".to_owned(),
        body: "Null handle".to_owned(),
        url: "https://example/1".to_owned(),
        labels: vec!["crash".to_owned(), "ui".to_owned()],
        trust: Trust::Untrusted,
    };
    let sparse = Item {
        external_id: "Odsp/2".to_owned(),
        title: "Slow boot".to_owned(),
        body: "Ten minutes".to_owned(),
        url: String::new(),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    };
    vec![full, sparse]
}

#[tokio::test]
async fn query_passes_wiql_verbatim_and_maps_items() {
    let stub = stub(&[("200 OK", WIQL_REPLY), ("200 OK", ITEMS_REPLY)]);
    let items = stub
        .forge()
        .query("Odsp/odsp-web", WIQL)
        .await
        .expect("query");
    let requests = stub.requests();
    let (wiql, hydrate) = (&requests[0], &requests[1]);
    assert_eq!(
        (
            wiql.method.as_str(),
            wiql.path.as_str(),
            wiql.authorization.as_str()
        ),
        ("POST", WIQL_PATH, AUTH),
    );
    assert_eq!(
        (
            hydrate.method.as_str(),
            wiql.body.contains(WIQL),
            hydrate.path.as_str()
        ),
        ("GET", true, HYDRATE_PATH),
    );
    assert_eq!(items, expected_items());
}

#[tokio::test]
async fn open_prs_filters_by_branch_prefix() {
    let stub = stub(&[("200 OK", PRS_REPLY)]);
    let prs = stub
        .forge()
        .open_prs(REGISTRY_URL, "bureau/")
        .await
        .expect("open_prs");
    let requests = stub.requests();
    assert_eq!(
        (requests[0].method.as_str(), requests[0].path.as_str()),
        ("GET", PRS_PATH),
    );
    assert_eq!(
        (
            prs.len(),
            prs[0].number,
            prs[0].repo.as_str(),
            prs[0].branch.as_str(),
            prs[0].item_id.as_deref()
        ),
        (1, 7, "Odsp/odsp-web", "bureau/fix-1", None),
    );
}

#[tokio::test]
async fn create_pr_prefixes_refs() {
    let stub = stub(&[("200 OK", CREATED_REPLY)]);
    let request = PrRequest {
        repo: "Odsp/odsp-web".to_owned(),
        branch: "bureau/fix".to_owned(),
        base: "main".to_owned(),
        title: "Fix".to_owned(),
        body: "Does it".to_owned(),
        item_id: None,
    };
    let pr = stub.forge().create_pr(&request).await.expect("create_pr");
    let requests = stub.requests();
    let sent = &requests[0];
    let refs = (
        sent.body
            .contains(r#""sourceRefName":"refs/heads/bureau/fix""#),
        sent.body.contains(r#""targetRefName":"refs/heads/main""#),
    );
    assert_eq!(
        (sent.method.as_str(), sent.path.as_str()),
        ("POST", CREATE_PATH)
    );
    assert_eq!(refs, (true, true));
    assert_eq!((pr.number, pr.branch.as_str()), (9, "bureau/fix"));
}

#[tokio::test]
async fn comment_posts_text_to_work_item() {
    let stub = stub(&[("200 OK", "{}")]);
    stub.forge()
        .comment("Odsp/42", "looking at this")
        .await
        .expect("comment");
    let requests = stub.requests();
    let sent = &requests[0];
    assert_eq!(
        (sent.method.as_str(), sent.path.as_str(), sent.body.as_str()),
        ("POST", COMMENT_PATH, r#"{"text":"looking at this"}"#),
    );
}

#[tokio::test]
async fn set_labels_replaces_tags() {
    let stub = stub(&[("200 OK", "{}")]);
    let labels = vec!["x".to_owned(), "y".to_owned()];
    stub.forge()
        .set_labels("Odsp/42", &labels)
        .await
        .expect("set_labels");
    let requests = stub.requests();
    let sent = &requests[0];
    assert_eq!(
        (sent.method.as_str(), sent.path.as_str(), sent.body.as_str()),
        (
            "PATCH",
            LABELS_PATH,
            r#"[{"op":"add","path":"/fields/System.Tags","value":"x; y"}]"#
        ),
    );
}

#[tokio::test]
async fn rejected_status_is_api_error_without_token() {
    let stub = stub(&[("401 Unauthorized", "access denied")]);
    let error = stub
        .forge()
        .comment("Odsp/42", "hi")
        .await
        .expect_err("must fail");
    let summary = match error {
        Error::Api { status, message } => (status, message.len(), message.contains("hunter2")),
        other => panic!("wrong error kind: {other:?}"),
    };
    assert_eq!(summary, (401, 13, false));
}

#[tokio::test]
async fn malformed_json_is_parse_error() {
    let stub = stub(&[("200 OK", "not json")]);
    let error = stub
        .forge()
        .comment("Odsp/42", "hi")
        .await
        .expect_err("must fail");
    assert!(matches!(error, Error::Parse(_)));
}

#[tokio::test]
async fn bad_item_id_is_parse_error() {
    let forge = AdoForge::new("http://127.0.0.1:1".to_owned(), Secret::new("hunter2"));
    let error = forge.comment("42", "hi").await.expect_err("must fail");
    assert!(matches!(error, Error::Parse(_)));
}
