use std::io::{self, BufRead};

use serde_json::{Map, Value, json};

use crate::contract::StepRequest;

use super::Paths;
use super::protocol::{Failure, Request, parse};
use super::tools;

const PROTOCOL_VERSION: &str = "2025-06-18";

fn optional_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, Failure> {
    object.get(field).map_or(Ok(None), |value| {
        value
            .as_str()
            .map(Some)
            .ok_or_else(|| Failure::invalid_params(format!("{field} must be a string")))
    })
}

fn protocol_version(params: Option<&Value>) -> Result<&str, Failure> {
    let Some(value) = params else {
        return Ok(PROTOCOL_VERSION);
    };
    let object = value
        .as_object()
        .ok_or_else(|| Failure::invalid_params("initialize params must be an object"))?;
    optional_string(object, "protocolVersion").map(|value| value.unwrap_or(PROTOCOL_VERSION))
}

fn initialize(params: Option<&Value>) -> Result<Value, Failure> {
    let protocol_version = protocol_version(params)?;
    Ok(json!({
        "protocolVersion": protocol_version,
        "capabilities": {"tools": {}},
        "serverInfo": {
            "name": "bureau-io",
            "version": env!("CARGO_PKG_VERSION")
        }
    }))
}

struct Server<W> {
    paths: Paths,
    request: StepRequest,
    writer: W,
}

impl<W: io::Write> Server<W> {
    fn handle(&mut self, line: &[u8]) -> io::Result<()> {
        let response = match parse(line) {
            Ok(request) => request.complete(self.dispatch(&request)),
            Err(error) => Some(error),
        };
        if let Some(value) = response {
            self.write(&value)?;
        }
        Ok(())
    }

    fn dispatch(&self, request: &Request) -> Result<Value, Failure> {
        match request.method.as_str() {
            "initialize" => initialize(request.params.as_ref()),
            "notifications/initialized" | "ping" => Ok(json!({})),
            "tools/list" => Ok(tools::list()),
            "tools/call" => {
                tools::call(request.params.as_ref(), &self.request, self.paths.result())
            }
            method => Err(Failure::method_not_found(method)),
        }
    }

    fn write(&mut self, value: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.writer, value).map_err(io::Error::other)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }
}

fn read_messages<R: BufRead, W: io::Write>(
    reader: &mut R,
    server: &mut Server<W>,
) -> io::Result<()> {
    loop {
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        server.handle(&line)?;
    }
}

/// Serves newline-delimited MCP JSON-RPC messages.
///
/// Path validation completes before the first message is read.
///
/// # Errors
/// Returns an error for invalid session paths or stream I/O failures.
pub fn serve<R: BufRead, W: io::Write>(paths: Paths, mut reader: R, writer: W) -> io::Result<()> {
    let request = paths.validate()?;
    let mut server = Server {
        paths,
        request,
        writer,
    };
    read_messages(&mut reader, &mut server)
}

/// Serves MCP using environment-provided paths and process stdio.
///
/// # Errors
/// Returns an error for missing environment, invalid paths, or stdio failures.
pub fn serve_stdio() -> io::Result<()> {
    let paths = Paths::from_env()?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve(paths, stdin.lock(), stdout.lock())
}
