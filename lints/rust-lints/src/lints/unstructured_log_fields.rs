use clippy_utils::diagnostics::span_lint_and_help;
use rustc_data_structures::fx::FxHashSet;
use rustc_hir::Expr;
use rustc_lint::{LateContext, LateLintPass, LintContext as _};
use rustc_span::{ExpnKind, Span};

use crate::lints::suppression::is_in_test_zone;

/// Split a tracing macro invocation into the tokens before the format string
/// and the format string content itself.
///
/// Returns `(before_format_string, format_string_content)`.
#[expect(
    clippy::indexing_slicing,
    reason = "all `bytes[i]` accesses are guarded by `i < bytes.len()` loop conditions"
)]
fn split_at_format_string(snippet: &str) -> Option<(&str, &str)> {
    // Find the opening delimiter after the macro name.
    let args_start = snippet.find('(')?;
    let args = &snippet[args_start + 1..];

    // Walk tokens at depth 0 to find the first string literal that is the actual
    // format string. Leading tracing directives (`target:`, `parent:`) take a
    // value that may itself be a string literal (e.g. `target: "net"`); those
    // are not the format string, so skip them.
    let bytes = args.as_bytes();
    let mut i = 0;
    let mut depth: u32 = 0;
    // Start of the current top-level argument segment (after the last `,` at
    // depth 0). Used to detect a leading `target:` / `parent:` directive name.
    let mut seg_start = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => {
                if depth == 0 {
                    break; // closing paren of macro
                }
                depth -= 1;
            }
            b',' if depth == 0 => seg_start = i + 1,
            b'"' if depth == 0 => {
                // Is this string the value of a leading tracing directive
                // (`target: "..."` / `parent: "..."`)? If so, it is not the
                // format string — skip past it and keep scanning.
                let segment = args[seg_start..i].trim();
                let is_directive_value = segment
                    .strip_suffix(':')
                    .map(str::trim_end)
                    .is_some_and(|name| matches!(name, "target" | "parent"));

                let before = &args[..i];
                // Walk past the string literal content.
                let str_start = i + 1;
                i = str_start;
                let mut terminated = false;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'"' {
                        terminated = true;
                        break;
                    }
                    i += 1;
                }
                if !terminated {
                    return None; // unterminated string
                }
                if !is_directive_value {
                    let str_content = &args[str_start..i];
                    return Some((before.trim(), str_content));
                }
                // Skip this directive value string and continue scanning.
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Check if a format string contains placeholders like `{}`, `{:?}`, `{name}`.
/// Escaped braces `{{` are ignored.
#[expect(
    clippy::indexing_slicing,
    reason = "all `bytes[i]` accesses are guarded by `i < bytes.len()` loop condition"
)]
fn has_format_placeholders(fmt: &str) -> bool {
    let bytes = fmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                i += 2;
                continue;
            }
            return true;
        }
        i += 1;
    }
    false
}

/// Byte index of the first top-level `,` in `s`, skipping over `"…"` string
/// literals whose contents may themselves contain commas. Returns `None` when
/// there is no top-level comma.
#[expect(
    clippy::indexing_slicing,
    reason = "all `bytes[i]` accesses are guarded by `i < bytes.len()` loop conditions"
)]
fn find_top_level_comma(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                // Skip the string literal, honoring `\"` escapes.
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
            }
            b',' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Strip a leading tracing directive (`target: <value>` / `parent: <value>`)
/// from the tokens before the format string. Directives are not structured
/// fields, so they must not count as structuring. Returns the remaining tokens
/// and whether at least one directive was stripped.
fn strip_leading_directives(before: &str) -> (&str, bool) {
    let mut rest = before.trim();
    let mut stripped = false;
    loop {
        let Some(name_end) = rest.find(':') else {
            return (rest, stripped);
        };
        let name = rest[..name_end].trim();
        if !matches!(name, "target" | "parent") {
            return (rest, stripped);
        }
        stripped = true;
        // Drop everything up to and including the directive's trailing comma.
        // The comma search skips string literals so a comma *inside* the value
        // (e.g. `target: "a, b"`) is not mistaken for the directive separator.
        let value = &rest[name_end + 1..];
        match find_top_level_comma(value) {
            Some(comma) => rest = value[comma + 1..].trim(),
            None => return ("", true), // directive with no following arg
        }
    }
}

/// Returns `Some(has_directive)` when the macro invocation snippet has format
/// placeholders in its format string but no structured tracing fields
/// (`key = value`, `?field`, `%field`, or bare identifier) before it.
///
/// `has_directive` reports whether the invocation began with a tracing directive
/// (`target:` / `parent:`). With a directive the macro expands through a
/// different arm whose call-site span covers the trailing `;`, so the diagnostic
/// span must be extended to match.
fn format_only_call(snippet: &str) -> Option<bool> {
    let (before_fmt, fmt_str) = split_at_format_string(snippet)?;
    let (after_directives, has_directive) = strip_leading_directives(before_fmt);
    if has_format_placeholders(fmt_str)
        && after_directives.trim().trim_end_matches(',').trim().is_empty()
    {
        Some(has_directive)
    } else {
        None
    }
}

/// Walk up the macro expansion chain to find a tracing level macro defined in
/// the `tracing` crate. Returns the display label (e.g. `"tracing::info"`) and
/// the call site span.
fn find_tracing_macro_callsite(
    cx: &LateContext<'_>,
    span: Span,
) -> Option<(&'static str, Span)> {
    let mut current = span;
    while current.from_expansion() {
        let expn = current.ctxt().outer_expn_data();
        if let ExpnKind::Macro(_, name) = &expn.kind
            && let Some(def_id) = expn.macro_def_id
            && cx.tcx.crate_name(def_id.krate).as_str() == "tracing"
        {
            // The expansion name may be the bare ident (`info`) or a
            // crate-qualified path (`tracing::info`), depending on how the
            // macro was invoked. Accept both.
            let base = name.as_str();
            let base = base.strip_prefix("tracing::").unwrap_or(base);
            let label = match base {
                "info" => Some("tracing::info"),
                "warn" => Some("tracing::warn"),
                "debug" => Some("tracing::debug"),
                "error" => Some("tracing::error"),
                "trace" => Some("tracing::trace"),
                _ => None,
            };
            if let Some(label) = label {
                return Some((label, expn.call_site));
            }
        }
        current = expn.call_site;
    }
    None
}

rustc_session::declare_lint! {
    /// Flags `tracing` macro invocations where all captured values are positional
    /// format arguments and none are structured key-value fields.
    pub UNSTRUCTURED_LOG_FIELDS,
    Warn,
    "`tracing` macro uses format args instead of structured fields"
}

pub struct UnstructuredLogFields {
    seen_callsites: FxHashSet<Span>,
}

impl UnstructuredLogFields {
    pub fn new() -> Self {
        Self {
            seen_callsites: FxHashSet::default(),
        }
    }
}

rustc_session::impl_lint_pass!(UnstructuredLogFields => [UNSTRUCTURED_LOG_FIELDS]);

impl<'tcx> LateLintPass<'tcx> for UnstructuredLogFields {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if !expr.span.from_expansion() {
            return;
        }

        let Some((macro_label, call_site)) = find_tracing_macro_callsite(cx, expr.span) else {
            return;
        };

        if !self.seen_callsites.insert(call_site) {
            return;
        }

        if is_in_test_zone(cx, expr) {
            return;
        }

        let sm = cx.sess().source_map();
        let Ok(snippet) = sm.span_to_snippet(call_site) else {
            return;
        };

        let Some(has_directive) = format_only_call(&snippet) else {
            return;
        };

        // A directive-prefixed invocation (`target:` / `parent:`) expands
        // through a macro arm whose call-site span swallows the statement's
        // trailing `;`; extend the diagnostic span to cover it.
        let lint_span = if has_directive {
            sm.span_extend_while(call_site, |c| c == ';')
                .unwrap_or(call_site)
        } else {
            call_site
        };

        span_lint_and_help(
            cx,
            UNSTRUCTURED_LOG_FIELDS,
            lint_span,
            format!("`{macro_label}!` uses format args instead of structured fields"),
            None,
            "use structured fields: `tracing::info!(key, \"message\")` instead of \
             `tracing::info!(\"msg {}\", key)`",
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ui_unstructured_log_fields() {
        crate::testing::run_ui_test("unstructured_log_fields", None, &[]);
    }
}
