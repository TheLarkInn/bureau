//! Offline ADO pull-request status observation.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;

use bureau::forge::ado::AdoForge;
use bureau::forge::{Forge as _, PrStatus};
use bureau::process::Secret;

#[tokio::test]
async fn pr_status_reports_the_completed_commit() {
    let (base, request) = server();
    let forge = AdoForge::new(base, Secret::new("token"));
    let status = forge.pr_status("Odsp/odsp-web", 9).await.expect("status");
    let path = request.recv().expect("request");
    let expected = PrStatus::Merged {
        commit: Some("abc123".to_owned()),
    };
    assert_eq!((status, path), (expected, status_path().to_owned()));
}

fn server() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || respond(&listener, &send));
    (base, receive)
}

fn respond(listener: &TcpListener, send: &mpsc::Sender<String>) {
    let (mut stream, _) = listener.accept().expect("accept");
    send.send(request_path(&stream)).expect("send request");
    let body =
        r#"{"pullRequestId":9,"status":"completed","lastMergeCommit":{"commitId":"abc123"}}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).expect("response");
}

fn request_path(stream: &TcpStream) -> String {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first).expect("request line");
    first
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned()
}

const fn status_path() -> &'static str {
    "/Odsp/_apis/git/repositories/odsp-web/pullrequests/9?api-version=7.1"
}
