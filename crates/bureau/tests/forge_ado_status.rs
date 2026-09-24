//! Offline ADO pull-request status and single work-item reads.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;

use bureau::forge::ado::AdoForge;
use bureau::forge::{Forge as _, PrStatus};
use bureau::process::Secret;

const STATUS_BODY: &str =
    r#"{"pullRequestId":9,"status":"completed","lastMergeCommit":{"commitId":"abc123"}}"#;

const ITEM_BODY: &str = r#"{"value":[{"id":5,"rev":3,"fields":{"System.Title":"T","System.Tags":"ready; bureau:reported"},"_links":{"html":{"href":"https://x/5"}}}]}"#;

fn server(bodies: Vec<&'static str>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        for body in bodies {
            respond(&listener, &send, body);
        }
    });
    (base, receive)
}

fn respond(listener: &TcpListener, send: &mpsc::Sender<String>, body: &str) {
    let (mut stream, _) = listener.accept().expect("accept");
    send.send(request_path(&stream)).expect("send request");
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

const fn item_path() -> &'static str {
    "/Odsp/_apis/wit/workitems?ids=5&fields=System.Id,System.Title,System.Description,System.Tags&api-version=7.1"
}

#[tokio::test]
async fn pr_status_reports_the_completed_commit() {
    let (base, request) = server(vec![STATUS_BODY]);
    let forge = AdoForge::new(base, Secret::new("token"));
    let status = forge.pr_status("Odsp/odsp-web", 9).await.expect("status");
    let path = request.recv().expect("request");
    let expected = PrStatus::Merged {
        commit: Some("abc123".to_owned()),
    };
    assert_eq!((status, path), (expected, status_path().to_owned()));
}

#[tokio::test]
async fn item_reads_one_work_item_and_refuses_an_absent_one() {
    let (base, request) = server(vec![ITEM_BODY, r#"{"value":[]}"#]);
    let forge = AdoForge::new(base, Secret::new("token"));
    let mut seen = Vec::new();
    for _ in 0..2 {
        let read = forge.item("Odsp/5").await;
        let path = request.recv().expect("request");
        seen.push((
            read.map(|item| item.labels).map_err(|e| e.to_string()),
            path,
        ));
    }
    let labels = vec!["ready".to_owned(), "bureau:reported".to_owned()];
    let missing = "unexpected forge response: work item `Odsp/5` not found".to_owned();
    let expected = vec![
        (Ok(labels), item_path().to_owned()),
        (Err(missing), item_path().to_owned()),
    ];
    assert_eq!(seen, expected);
}
