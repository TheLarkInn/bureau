//! Offline tests for the GitHub forge: a canned-response HTTP server on
//! 127.0.0.1 stands in for `api.github.com` (DESIGN.md layer 7).

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use bureau::contract::Trust;
use bureau::forge::github::GitHubForge;
use bureau::forge::{Error, Forge as _, Item, Pr};
use bureau::process::Secret;

/// A canned-response HTTP server that records each raw request.
struct TestServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    join: JoinHandle<()>,
}

impl TestServer {
    /// Serves `responses(addr)`, one per connection, then exits.
    fn start(responses: impl FnOnce(SocketAddr) -> Vec<String> + Send) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let addr = listener.local_addr().expect("local addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&requests);
        let responses = responses(addr);
        let join = std::thread::spawn(move || serve(&listener, &responses, &sink));
        Self {
            addr,
            requests,
            join,
        }
    }

    /// A forge pointed at this server.
    fn forge(&self) -> GitHubForge {
        GitHubForge::new(Secret::new("test-token")).with_base_url(format!("http://{}", self.addr))
    }

    /// The recorded requests; joins the server thread first.
    fn requests(self) -> Vec<String> {
        self.join.join().expect("server thread");
        self.requests.lock().expect("requests lock").clone()
    }
}

/// Answers each connection with the next canned response.
fn serve(listener: &TcpListener, responses: &[String], sink: &Mutex<Vec<String>>) {
    for response in responses {
        let mut stream = accept(listener);
        sink.lock().expect("sink lock").push(read_request(&stream));
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    }
}

/// The next connection; the deadline turns a request-count mismatch
/// into a failed test instead of a hung one.
fn accept(listener: &TcpListener) -> TcpStream {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok((stream, _)) = listener.accept() {
            stream
                .set_read_timeout(Some(Duration::from_secs(15)))
                .expect("read timeout");
            return stream;
        }
        assert!(std::time::Instant::now() < deadline, "accept timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Reads one request head plus its `Content-Length` body.
fn read_request(stream: &TcpStream) -> String {
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut head = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read line");
        if line == "\r\n" || line.is_empty() {
            break;
        }
        head.push_str(&line);
    }
    let mut body = vec![0; content_length(&head)];
    reader.read_exact(&mut body).expect("read body");
    format!("{head}{}", String::from_utf8_lossy(&body))
}

/// The `Content-Length` header value, or 0.
fn content_length(head: &str) -> usize {
    for line in head.lines() {
        if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
            return value.trim().parse().unwrap_or(0);
        }
    }
    0
}

/// A complete HTTP response with a JSON body.
fn response(status: u16, body: &str, extra_head: &str) -> String {
    let head = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n{extra_head}");
    format!(
        "{head}content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// A 200 JSON response.
fn ok_json(body: &str) -> String {
    response(200, body, "")
}

/// A 200 JSON response carrying a `Link: rel="next"` header.
fn ok_json_linked(body: &str, next: &str) -> String {
    response(200, body, &format!("link: <{next}>; rel=\"next\"\r\n"))
}

/// The request target, percent-decoded enough for assertions.
fn decoded_target(request: &str) -> String {
    let target = request.split(' ').nth(1).unwrap_or("");
    target
        .replace("%3A", ":")
        .replace("%2F", "/")
        .replace('+', " ")
}

/// The first recorded request and its lowercase form for header checks.
fn only_request(server: TestServer) -> (String, String) {
    let request = server.requests().into_iter().next().expect("a request");
    let lower = request.to_lowercase();
    (request, lower)
}

/// Two in-repo issues plus one cross-repo result a malicious filter exposed.
const QUERY_BODY: &str = concat!(
    r#"{"items":[{"number":1,"repository_url":"https://api.github.com/repos/o/r","title":"Fix bug","body":"details","#,
    r#""html_url":"https://x/1","labels":[{"name":"bug"}],"author_association":"OWNER"},"#,
    r#"{"number":2,"repository_url":"https://api.github.com/repos/o/r","title":"Idea","body":null,"html_url":"https://x/2","#,
    r#""labels":[],"author_association":"NONE"},"#,
    r#"{"number":1,"repository_url":"https://api.github.com/repos/other/repo","title":"Wrong repo","#,
    r#""body":null,"html_url":"https://x/3","labels":[],"author_association":"OWNER"}]}"#,
);

#[tokio::test]
async fn query_maps_items_and_grades_trust() {
    let server = TestServer::start(|_| vec![ok_json(QUERY_BODY)]);
    let items = server
        .forge()
        .query("https://github.com/o/r", "is:issue label:agent-ready")
        .await
        .expect("query");
    assert_mapped_items(&items);
    assert_query_request(server);
}

/// Item mapping: `external_id`/`body`/`trust`, then `title`/`url`/`labels`.
fn assert_mapped_items(items: &[Item]) {
    let seen: Vec<_> = items
        .iter()
        .map(|i| (i.external_id.as_str(), i.body.as_str(), i.trust))
        .collect();
    let expected = [
        ("o/r#1", "details", Trust::Maintainer),
        ("o/r#2", "", Trust::Untrusted),
    ];
    assert_eq!(seen, expected);
    let extra: Vec<_> = items
        .iter()
        .map(|i| (i.title.as_str(), i.url.as_str(), i.labels.clone()))
        .collect();
    let expected_extra = [
        ("Fix bug", "https://x/1", vec!["bug".to_owned()]),
        ("Idea", "https://x/2", Vec::new()),
    ];
    assert_eq!(extra, expected_extra);
}

/// The query request: verbatim filter plus repo scope, auth headers.
fn assert_query_request(server: TestServer) {
    let (request, lower) = only_request(server);
    let got = (
        decoded_target(&request)
            == "/search/issues?q=is:issue label:agent-ready repo:o/r&per_page=100",
        lower.contains("authorization: bearer test-token"),
        lower.contains("x-github-api-version: 2026-03-10"),
    );
    assert_eq!(got, (true, true, true));
}

/// Two PRs: one on a `bureau/` branch closing #9, one on another branch.
const PULLS_BODY: &str = concat!(
    r#"[{"number":10,"title":"A","html_url":"https://x/10","#,
    r#""body":"work\n\nCloses #9","head":{"ref":"bureau/fix-9"}},"#,
    r#"{"number":11,"title":"B","html_url":"https://x/11","#,
    r#""body":null,"head":{"ref":"misc/other"}}]"#,
);

#[tokio::test]
async fn open_prs_filters_by_branch_prefix() {
    let server = TestServer::start(|_| vec![ok_json(PULLS_BODY)]);
    let prs = server
        .forge()
        .open_prs("https://github.com/o/r", "bureau/")
        .await
        .expect("open prs");
    assert_eq!(prs, [expected_pr()]);
    assert_pulls_request(server);
}

/// The one PR matching the `bureau/` prefix.
fn expected_pr() -> Pr {
    Pr {
        number: 10,
        repo: "o/r".to_owned(),
        branch: "bureau/fix-9".to_owned(),
        title: "A".to_owned(),
        url: "https://x/10".to_owned(),
        item_id: Some("o/r#9".to_owned()),
    }
}

/// The pulls request: open state, page size, JSON accept header.
fn assert_pulls_request(server: TestServer) {
    let (request, lower) = only_request(server);
    let got = (
        decoded_target(&request) == "/repos/o/r/pulls?state=open&per_page=100",
        lower.contains("accept: application/vnd.github+json"),
    );
    assert_eq!(got, (true, true));
}

#[tokio::test]
async fn malformed_json_becomes_error_parse() {
    let server = TestServer::start(|_| vec![ok_json("not json")]);
    let err = server
        .forge()
        .query("o/r", "anything")
        .await
        .expect_err("must fail");
    assert!(matches!(err, Error::Parse(_)), "got {err:?}");
}

#[tokio::test]
async fn query_follows_next_links() {
    let page1 = r#"{"items":[{"number":1,"repository_url":"https://api.github.com/repos/o/r","title":"A","body":null,"html_url":"https://x/1","labels":[],"author_association":"NONE"}]}"#;
    let page2 = r#"{"items":[{"number":2,"repository_url":"https://api.github.com/repos/o/r","title":"B","body":null,"html_url":"https://x/2","labels":[],"author_association":"OWNER"}]}"#;
    let server = TestServer::start(|addr| {
        vec![
            ok_json_linked(page1, &format!("http://{addr}/page2")),
            ok_json(page2),
        ]
    });
    let items = server.forge().query("o/r", "f").await.expect("query");
    let count = (items.len(), server.requests().len());
    assert_eq!(count, (2, 2));
}
