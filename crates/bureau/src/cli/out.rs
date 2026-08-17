//! CLI output boundary — the only place user-facing text is written.
//!
//! `println!`/`eprintln!` are banned by the `debug_remnants` lint (no fn main
//! exemption); user-facing output goes through these two functions, which
//! write via `std::io::Write` (not a flagged macro) and ignore broken-pipe
//! errors so piping into `head` exits quietly.

use std::fmt::Arguments;
use std::io::Write as _;

/// Writes one line to stdout.
pub fn line(args: Arguments<'_>) {
    let _written = writeln!(std::io::stdout(), "{args}");
}

/// Writes one line to stderr.
pub fn error(args: Arguments<'_>) {
    let _written = writeln!(std::io::stderr(), "{args}");
}
