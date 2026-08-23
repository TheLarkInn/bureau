//! The ADO tests' loopback stub: canned replies plus recorded requests.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use bureau::forge::ado::AdoForge;
use bureau::process::Secret;

#[derive(Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub authorization: String,
    pub body: String,
}

pub struct Stub {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl Stub {
    pub fn forge(&self) -> AdoForge {
        AdoForge::new(self.url.clone(), Secret::new("hunter2"))
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().expect("requests lock").clone()
    }
}

pub fn stub(responses: &[(&str, &str)]) -> Stub {
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
