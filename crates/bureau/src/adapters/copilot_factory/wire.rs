//! Byte-counted SDK framing, not ACP newline-delimited JSON.
//!
//! The bounds below protect transport memory; they are not factory resource
//! ceilings. Message shape, deadlines, and connection recovery belong to callers.
//! Discard the stream after an error or cancellation partway through a frame.

use std::io;

use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt as _, AsyncReadExt as _};
use tokio::io::{AsyncWrite, AsyncWriteExt as _};

/// Maximum complete header section, including its terminating CRLF pair.
pub const MAX_HEADER_BYTES: usize = 8 * 1024;
/// Maximum encoded JSON body accepted or emitted by this transport.
pub const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

fn invalid(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn incomplete_line(line: &[u8]) -> io::Error {
    if line.ends_with(b"\n") {
        invalid("SDK frame headers must end with CRLF")
    } else {
        io::Error::new(io::ErrorKind::UnexpectedEof, "incomplete SDK frame header")
    }
}

fn validate_line(line: &[u8], remaining: usize) -> io::Result<()> {
    if line.len() > remaining {
        return Err(invalid("SDK frame headers exceed the byte bound"));
    }
    line.strip_suffix(b"\r\n")
        .ok_or_else(|| incomplete_line(line))?;
    Ok(())
}

async fn read_line(
    reader: &mut (impl AsyncBufRead + Unpin),
    remaining: usize,
) -> io::Result<Vec<u8>> {
    let limit = u64::try_from(remaining).map_err(io::Error::other)? + 1;
    let mut line = Vec::new();
    reader.take(limit).read_until(b'\n', &mut line).await?;
    validate_line(&line, remaining)?;
    Ok(line)
}

fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

const fn value_byte(byte: u8) -> bool {
    byte == b'\t' || (byte >= b' ' && byte <= b'~')
}

fn header(line: &[u8]) -> io::Result<(&str, &str)> {
    let text = std::str::from_utf8(line).map_err(invalid)?;
    let text = text
        .strip_suffix("\r\n")
        .ok_or_else(|| invalid("SDK frame headers must end with CRLF"))?;
    let (name, value) = text
        .split_once(':')
        .ok_or_else(|| invalid("SDK frame header is missing its colon"))?;
    if name.is_empty() || !name.bytes().all(name_byte) || !value.bytes().all(value_byte) {
        return Err(invalid("invalid SDK frame header name or value"));
    }
    Ok((name, value.trim_matches([' ', '\t'])))
}

fn parse_length(value: &str) -> io::Result<usize> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid(
            "Content-Length must be an unsigned decimal byte count",
        ));
    }
    let length: usize = value.parse().map_err(invalid)?;
    if length > MAX_BODY_BYTES {
        return Err(invalid("SDK frame body exceeds the byte bound"));
    }
    Ok(length)
}

fn record_length(line: &[u8], length: &mut Option<usize>) -> io::Result<()> {
    let (name, value) = header(line)?;
    if !name.eq_ignore_ascii_case("Content-Length") {
        return Ok(());
    }
    if length.is_some() {
        return Err(invalid("duplicate Content-Length header"));
    }
    *length = Some(parse_length(value)?);
    Ok(())
}

async fn read_length(reader: &mut (impl AsyncBufRead + Unpin)) -> io::Result<usize> {
    let mut remaining = MAX_HEADER_BYTES;
    let mut length = None;
    loop {
        let line = read_line(reader, remaining).await?;
        remaining -= line.len();
        if line == b"\r\n" {
            return length.ok_or_else(|| invalid("missing Content-Length header"));
        }
        record_length(&line, &mut length)?;
    }
}

/// Reads exactly one UTF-8 JSON body, preserving any following buffered frame.
///
/// JSON-RPC envelope validation is deliberately separate from framing.
///
/// # Errors
///
/// Returns `InvalidData` for malformed headers, exceeded transport bounds, or
/// invalid JSON/UTF-8; `UnexpectedEof` for incomplete headers or bodies; and
/// propagates underlying reader errors.
pub async fn read_frame(reader: &mut (impl AsyncBufRead + Unpin)) -> io::Result<Value> {
    let length = read_length(reader).await?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map_err(invalid)
}

fn encode(message: &Value) -> io::Result<Vec<u8>> {
    let body = serde_json::to_vec(message).map_err(invalid)?;
    if body.len() > MAX_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SDK frame body exceeds the byte bound",
        ));
    }
    Ok(body)
}

/// Writes and flushes one JSON frame using its encoded UTF-8 byte length.
///
/// # Errors
///
/// Returns `InvalidInput` if the encoded body exceeds the transport bound,
/// `InvalidData` for serialization errors, and propagates write/flush errors.
pub async fn write_frame(
    writer: &mut (impl AsyncWrite + Unpin),
    message: &Value,
) -> io::Result<()> {
    let body = encode(message)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(&body).await?;
    writer.flush().await
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
