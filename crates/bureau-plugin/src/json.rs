//! Small JSONC reader used for Copilot settings and plugin catalogs.

use std::fs;
use std::path::Path;

use serde_json::Value;

use super::Error;

pub fn read(path: &Path) -> Result<Value, Error> {
    let bytes = read_bytes(path)?;
    parse(&bytes, path)
}

pub fn read_optional(path: &Path) -> Result<Option<Value>, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => read(path).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::io("inspect", path, error)),
    }
}

pub fn parse(bytes: &[u8], path: &Path) -> Result<Value, Error> {
    let stripped = Stripper::new(bytes)
        .strip()
        .map_err(|message| Error::invalid(path, message))?;
    serde_json::from_slice(&stripped).map_err(|error| Error::invalid(path, error))
}

pub fn format(value: &Value, path: &Path) -> Result<Vec<u8>, Error> {
    let mut bytes =
        serde_json::to_vec_pretty(value).map_err(|error| Error::invalid(path, error))?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn read_bytes(path: &Path) -> Result<Vec<u8>, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|error| Error::io("inspect", path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Error::invalid(
            path,
            "expected a regular file, not a symlink",
        ));
    }
    fs::read(path).map_err(|error| Error::io("read", path, error))
}

struct Stripper<'a> {
    input: &'a [u8],
    index: usize,
    output: Vec<u8>,
}

impl<'a> Stripper<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            index: 0,
            output: Vec::with_capacity(input.len()),
        }
    }

    fn strip(mut self) -> Result<Vec<u8>, &'static str> {
        while self.index < self.input.len() {
            self.copy_next()?;
        }
        Ok(self.output)
    }

    fn copy_next(&mut self) -> Result<(), &'static str> {
        match (self.current(), self.next()) {
            (Some(b'"'), _) => self.copy_string(),
            (Some(b'/'), Some(b'/')) => {
                self.skip_line();
                Ok(())
            }
            (Some(b'/'), Some(b'*')) => self.skip_block(),
            (Some(byte), _) => {
                self.output.push(byte);
                self.index += 1;
                Ok(())
            }
            (None, _) => Ok(()),
        }
    }

    fn copy_string(&mut self) -> Result<(), &'static str> {
        self.copy_byte();
        while let Some(byte) = self.current() {
            self.copy_byte();
            if byte == b'\\' {
                self.copy_byte();
            } else if byte == b'"' {
                return Ok(());
            }
        }
        Err("unterminated string")
    }

    fn skip_line(&mut self) {
        self.replace_byte();
        self.replace_byte();
        while self.current().is_some_and(|byte| byte != b'\n') {
            self.replace_byte();
        }
    }

    fn skip_block(&mut self) -> Result<(), &'static str> {
        self.replace_byte();
        self.replace_byte();
        while (self.current(), self.next()) != (Some(b'*'), Some(b'/')) {
            if self.current().is_none() {
                return Err("unterminated comment");
            }
            self.replace_byte();
        }
        self.replace_byte();
        self.replace_byte();
        Ok(())
    }

    fn current(&self) -> Option<u8> {
        self.input.get(self.index).copied()
    }

    fn next(&self) -> Option<u8> {
        self.input.get(self.index + 1).copied()
    }

    fn copy_byte(&mut self) {
        if let Some(byte) = self.current() {
            self.output.push(byte);
            self.index += 1;
        }
    }

    fn replace_byte(&mut self) {
        if let Some(byte) = self.current() {
            self.output.push(if matches!(byte, b'\n' | b'\r') {
                byte
            } else {
                b' '
            });
            self.index += 1;
        }
    }
}
