use std::io::{self, ErrorKind};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, BufReader, BufWriter, DuplexStream};

use super::{MAX_BODY_BYTES, MAX_HEADER_BYTES, read_frame, write_frame};

const INVALID_HEADERS: &[&[u8]] = &[
    b"\r\n",
    b"Content-Type: application/json\r\n\r\n{}",
    b"Content-Length 2\r\n\r\n{}",
    b" Content-Length: 2\r\n\r\n{}",
    b"Content-Length : 2\r\n\r\n{}",
    b"Content-Length:\r\n\r\n",
    b"Content-Length: +2\r\n\r\n{}",
    b"Content-Length: -2\r\n\r\n{}",
    b"Content-Length: 2.0\r\n\r\n{}",
    b"Content-Length: 2,2\r\n\r\n{}",
    b"Content-Length: \xc2\xb2\r\n\r\n{}",
    b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
    b"Content-Length: 2\r\ncOnTeNt-LeNgTh: 2\r\n\r\n{}",
    b"Content-Length: 2\n\n{}",
    b"Content-Length: 2\r\n\n{}",
    b"Content-Length: 2\r\nX-Test: invalid\0value\r\n\r\n{}",
    b"Content-Length: 2\r\nX-Test: \xff\r\n\r\n{}",
    b"Content-Length: 184467440737095516160\r\n\r\n",
];

async fn input(bytes: &[u8]) -> io::Result<BufReader<DuplexStream>> {
    let (mut writer, reader) = tokio::io::duplex(bytes.len().max(1));
    writer.write_all(bytes).await?;
    drop(writer);
    Ok(BufReader::new(reader))
}

async fn decode(bytes: &[u8]) -> io::Result<Value> {
    read_frame(&mut input(bytes).await?).await
}

async fn send_chunks(mut writer: DuplexStream, chunks: &[&[u8]]) -> io::Result<()> {
    for chunk in chunks {
        writer.write_all(chunk).await?;
    }
    writer.shutdown().await
}

async fn decode_chunks(chunks: &[&[u8]]) -> io::Result<Value> {
    let (writer, reader) = tokio::io::duplex(1);
    let mut reader = BufReader::with_capacity(1, reader);
    let (decoded, sent) = tokio::join!(read_frame(&mut reader), send_chunks(writer, chunks));
    sent?;
    decoded
}

async fn round_trip(message: &Value) -> io::Result<Value> {
    let (mut writer, reader) = tokio::io::duplex(1);
    let mut reader = BufReader::with_capacity(1, reader);
    let (sent, decoded) = tokio::join!(write_frame(&mut writer, message), read_frame(&mut reader));
    sent?;
    decoded
}

#[tokio::test]
async fn malformed_headers_fail() {
    for bytes in INVALID_HEADERS {
        assert_eq!(
            decode(bytes).await.expect_err("invalid header").kind(),
            ErrorKind::InvalidData,
            "{bytes:?}"
        );
    }
}

#[tokio::test]
async fn invalid_json_and_utf8_fail() {
    for bytes in [
        b"Content-Length: 0\r\n\r\n".as_slice(),
        b"Content-Length: 1\r\n\r\n{",
        b"Content-Length: 2\r\n\r\n\xff\xfe",
        b"Content-Length: 3\r\n\r\n{}\0",
        b"Content-Length: 4\r\n\r\n{}{}",
        b"Content-Length: 4\r\n\r\n\"\xffx\"",
        b"Content-Length: 2\r\n\r\n \t",
    ] {
        assert_eq!(
            decode(bytes).await.expect_err("invalid JSON").kind(),
            ErrorKind::InvalidData
        );
    }
}

#[tokio::test]
async fn premature_eof_fails() {
    for bytes in [
        b"".as_slice(),
        b"Content-Length",
        b"Content-Length: 2\r",
        b"Content-Length: 2\r\n",
        b"Content-Length: 2\r\n\r",
        b"Content-Length: 2\r\n\r\n",
        b"Content-Length: 2\r\n\r\n{",
    ] {
        assert_eq!(
            decode(bytes).await.expect_err("incomplete frame").kind(),
            ErrorKind::UnexpectedEof
        );
    }
}

#[tokio::test]
async fn header_names_and_optional_headers_are_supported() {
    let bytes = b"Content-Type: application/json\r\ncOnTeNt-LeNgTh:\t0002 \t\r\n\r\n{}";
    assert_eq!(decode(bytes).await.expect("valid headers"), json!({}));
}

#[tokio::test]
async fn split_headers_and_unicode_code_points_are_preserved() {
    let chunks: &[&[u8]] = &[
        b"Con",
        b"tent-Leng",
        b"th: 6\r",
        b"\n\r",
        b"\n\"",
        b"\xf0\x9f",
        b"\x99\x82",
        b"\"",
    ];
    let decoded = tokio::time::timeout(Duration::from_secs(2), decode_chunks(chunks))
        .await
        .expect("split frame must finish")
        .expect("split frame");
    assert_eq!(decoded, json!("🙂"));
}

#[tokio::test]
async fn coalesced_frames_remain_separate() {
    let mut reader = input("Content-Length: 2\r\n\r\n{}Content-Length: 6\r\n\r\n\"🙂\"".as_bytes())
        .await
        .expect("input");
    let first = read_frame(&mut reader).await.expect("first frame");
    let second = read_frame(&mut reader).await.expect("second frame");
    assert_eq!((first, second), (json!({}), json!("🙂")));
}

#[tokio::test]
async fn writing_uses_utf8_byte_lengths_and_exact_crlf() {
    let (mut writer, mut reader) = tokio::io::duplex(64);
    write_frame(&mut writer, &json!("é🙂"))
        .await
        .expect("write");
    drop(writer);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.expect("read bytes");
    assert_eq!(bytes, "Content-Length: 8\r\n\r\n\"é🙂\"".as_bytes());
}

#[tokio::test]
async fn buffered_writers_flush_the_complete_frame() {
    let (writer, reader) = tokio::io::duplex(64);
    let mut writer = BufWriter::with_capacity(64, writer);
    write_frame(&mut writer, &json!({})).await.expect("write");
    let mut reader = BufReader::new(reader);
    let decoded = tokio::time::timeout(Duration::from_secs(2), read_frame(&mut reader))
        .await
        .expect("frame must be flushed")
        .expect("frame");
    assert_eq!(decoded, json!({}));
}

#[tokio::test]
async fn small_buffers_round_trip_arbitrary_json() {
    for message in [json!({"text": "é🙂"}), json!(null), json!([1, true, "x"])] {
        let decoded = tokio::time::timeout(Duration::from_secs(2), round_trip(&message))
            .await
            .expect("round trip must finish")
            .expect("round trip");
        assert_eq!(decoded, message);
    }
}

#[tokio::test]
async fn oversized_header_fails_without_waiting_for_eof() {
    let (mut writer, reader) = tokio::io::duplex(MAX_HEADER_BYTES + 1);
    writer
        .write_all(&vec![b'X'; MAX_HEADER_BYTES + 1])
        .await
        .expect("input");
    let mut reader = BufReader::new(reader);
    let error = tokio::time::timeout(Duration::from_secs(2), read_frame(&mut reader))
        .await
        .expect("bounded header read")
        .expect_err("oversized header");
    assert_eq!(error.kind(), ErrorKind::InvalidData);
}

#[tokio::test]
async fn complete_header_at_exact_bound_is_accepted() {
    let mut bytes = b"Content-Length: 2\r\nPadding: ".to_vec();
    bytes.resize(MAX_HEADER_BYTES - 4, b'x');
    bytes.extend_from_slice(b"\r\n\r\n{}");
    assert_eq!(decode(&bytes).await.expect("bounded header"), json!({}));
}

#[tokio::test]
async fn cumulative_header_size_is_bounded() {
    let bytes = format!(
        "Content-Length: 2\r\n{}\r\n{{}}",
        "X: y\r\n".repeat(MAX_HEADER_BYTES / 6)
    );
    assert_eq!(
        decode(bytes.as_bytes())
            .await
            .expect_err("header bound")
            .kind(),
        ErrorKind::InvalidData
    );
}

#[tokio::test]
async fn oversized_body_is_rejected_before_reading_it() {
    let bytes = format!("Content-Length: {}\r\n\r\n", MAX_BODY_BYTES + 1);
    assert_eq!(
        decode(bytes.as_bytes())
            .await
            .expect_err("body bound")
            .kind(),
        ErrorKind::InvalidData
    );
}

#[tokio::test]
async fn oversized_outgoing_body_writes_nothing() {
    let (mut writer, mut reader) = tokio::io::duplex(1);
    let message = json!("x".repeat(MAX_BODY_BYTES));
    let error = tokio::time::timeout(Duration::from_secs(2), write_frame(&mut writer, &message))
        .await
        .expect("bounded outgoing frame")
        .expect_err("body bound");
    drop(writer);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.expect("read");
    assert_eq!((error.kind(), bytes), (ErrorKind::InvalidInput, Vec::new()));
}

#[tokio::test]
async fn closed_writer_peer_is_reported() {
    let (mut writer, reader) = tokio::io::duplex(1);
    drop(reader);
    let error = write_frame(&mut writer, &json!({}))
        .await
        .expect_err("closed reader");
    assert_eq!(error.kind(), ErrorKind::BrokenPipe);
}
