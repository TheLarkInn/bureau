//! Structural artifact validation only; pinning and activation belong to the runtime.

use std::path::{Component, Path};

fn valid_digest(digest: &str) -> bool {
    let Some(hex) = digest.strip_prefix("tree-sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub(super) fn check_digest(digest: &str, field: &str, errors: &mut Vec<String>) {
    if !valid_digest(digest) {
        errors.push(format!(
            "`{field}` must be canonical `tree-sha256:` followed by 64 lowercase hexadecimal digits"
        ));
    }
}

fn safe_text(text: &str) -> bool {
    !text.trim().is_empty()
        && !text.contains('\0')
        && text.split(['/', '\\']).all(|component| component != "..")
}

fn drive_prefix(text: &str) -> bool {
    matches!(text.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic())
}

fn relative_text(text: &str) -> bool {
    safe_text(text) && !text.starts_with('\\') && !drive_prefix(text)
}

fn relative_components(path: &Path, allow_current: bool) -> bool {
    let confined = path
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    let named = path
        .components()
        .any(|component| matches!(component, Component::Normal(_)));
    confined && (named || allow_current)
}

fn valid_relative(path: &Path, allow_current: bool) -> bool {
    path.to_str().is_some_and(relative_text) && relative_components(path, allow_current)
}

pub(super) fn check_relative(
    path: &Path,
    field: &str,
    allow_current: bool,
    errors: &mut Vec<String>,
) {
    if !valid_relative(path, allow_current) {
        errors.push(format!(
            "`{field}` must be a safe nonempty relative path without parent or absolute components"
        ));
    }
}

pub(super) fn check_absolute(path: &Path, field: &str, errors: &mut Vec<String>) {
    if !(path.is_absolute() && path.to_str().is_some_and(safe_text)) {
        errors.push(format!(
            "`{field}` must be an absolute path without parent components or NUL bytes"
        ));
    }
}
