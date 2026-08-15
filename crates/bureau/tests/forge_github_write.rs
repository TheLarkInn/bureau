//! Offline tests for the GitHub forge's write calls: `create_pr`,
//! `comment`, `set_labels`, and error mapping (DESIGN.md layer 7).

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use bureau::forge::github::GitHubForge;
use bureau::forge::{Error, Forge as _, PrRequest};
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
fn response(status: u16, body: &str) -> String {
    let head = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n");
    format!(
        "{head}content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The first recorded request.
fn only_request(server: TestServer) -> String {
    server.requests().into_iter().next().expect("a request")
}

#[tokio::test]
async fn create_pr_links_the_work_item() {
    let body = concat!(
        r#"{"number":5,"title":"Fix","html_url":"https://x/5","#,
        r#""body":"did work\n\nCloses #7","head":{"ref":"bureau/fix-7"}}"#,
    );
    let server = TestServer::start(|_| vec![response(200, body)]);
    let req = PrRequest {
        repo: "https://github.com/o/r".to_owned(),
        branch: "bureau/fix-7".to_owned(),
        base: "main".to_owned(),
        title: "Fix".to_owned(),
        body: "did work".to_owned(),
        item_id: Some("o/r#7".to_owned()),
    };
    let pr = server.forge().create_pr(&req).await.expect("create pr");
    let got = (pr.number, pr.branch.as_str(), pr.item_id.as_deref());
    assert_eq!(got, (5, "bureau/fix-7", Some("o/r#7")));
    let request = only_request(server);
    let got = (
        request.starts_with("POST /repos/o/r/pulls HTTP/1.1"),
        request.contains(r#""head":"bureau/fix-7""#),
        request.contains(r"did work\n\nCloses #7"),
    );
    assert_eq!(got, (true, true, true));
}

#[tokio::test]
async fn comment_posts_to_the_issues_path() {
    let server = TestServer::start(|_| vec![response(200, "{}")]);
    server
        .forge()
        .comment("o/r#3", "hi there")
        .await
        .expect("comment");
    let request = only_request(server);
    let got = (
        request.starts_with("POST /repos/o/r/issues/3/comments HTTP/1.1"),
        request.contains(r#"{"body":"hi there"}"#),
    );
    assert_eq!(got, (true, true));
}

#[tokio::test]
async fn set_labels_puts_the_label_list() {
    let server = TestServer::start(|_| vec![response(200, "[]")]);
    server
        .forge()
        .set_labels("o/r#4", &["a".to_owned(), "b".to_owned()])
        .await
        .expect("set labels");
    let request = only_request(server);
    let got = (
        request.starts_with("PUT /repos/o/r/issues/4/labels HTTP/1.1"),
        request.contains(r#"{"labels":["a","b"]}"#),
    );
    assert_eq!(got, (true, true));
}

#[tokio::test]
async fn non_2xx_becomes_error_api_without_leaking_the_token() {
    let server = TestServer::start(|_| vec![response(422, r#"{"message":"nope"}"#)]);
    let err = server
        .forge()
        .comment("o/r#3", "hi")
        .await
        .expect_err("must fail");
    let Error::Api { status, message } = err else {
        panic!("expected Error::Api, got {err:?}")
    };
    let got = (
        status,
        message.contains("nope"),
        message.contains("test-token"),
    );
    assert_eq!(got, (422, true, false));
}
