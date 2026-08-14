# bureau

Organizing your agents

## Rust quality gates

All workspace crates inherit deny-level Rust and Clippy lints. Clippy limits
cognitive complexity to 4 and functions to 25 lines. The CI workflow also
rejects Rust source files over 300 lines and lint-suppression attributes,
including `#[allow(...)]` and `#[expect(...)]`.

Custom lints from [`li-kai/rust-lints`](https://github.com/li-kai/rust-lints)
run through Dylint and are promoted to errors in CI.
