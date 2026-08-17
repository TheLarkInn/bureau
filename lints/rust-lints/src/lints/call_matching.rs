//! Shared utilities for lints that match function calls against configured path lists.

use std::borrow::Cow;

use clippy_utils::{fn_def_id, is_entrypoint_fn};
use rustc_data_structures::fx::FxHashSet;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::def_id::DefId;
use rustc_hir::{Expr, ExprKind};
use rustc_lint::LateContext;

use super::suppression::is_in_test_zone;
use crate::config::SubLintConfig;

/// Typeck-parameterized equivalent of [`clippy_utils::fn_def_id`] — resolves
/// the callee `DefId` of a `Call` or `MethodCall` against explicit
/// `TypeckResults` rather than `cx.typeck_results()`. Needed when walking a
/// function body other than the one currently being lint-checked.
pub fn resolve_callee_def_id_with_typeck(
    typeck: &rustc_middle::ty::TypeckResults<'_>,
    expr: &Expr<'_>,
) -> Option<DefId> {
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "ExprKind has many variants; only Call and MethodCall are relevant"
    )]
    match &expr.kind {
        ExprKind::Call(
            Expr {
                kind: ExprKind::Path(qpath),
                hir_id: path_hir_id,
                ..
            },
            ..,
        ) => match typeck.qpath_res(qpath, *path_hir_id) {
            Res::Def(DefKind::Fn | DefKind::Ctor(..) | DefKind::AssocFn, id) => Some(id),
            _ => None,
        },
        ExprKind::MethodCall(..) => typeck.type_dependent_def_id(expr.hir_id),
        _ => None,
    }
}

/// Returns `true` if the expression is inside a suppression zone:
///
/// - **Test zone** — test crate, `#[test]` function, or `#[cfg(test)]` module
///   (see `suppression::is_in_test_zone`).
/// - **`fn main()`** — the composition root, where wiring up real
///   dependencies is expected.
pub fn is_in_suppression_zone(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    if is_in_test_zone(cx, expr) {
        return true;
    }

    // `hir_get_parent_item` walks out to the enclosing item, treating closures
    // as transparent (they are bodies, not items). So a call inside a closure
    // nested in `fn main()` still resolves to `fn main`, keeping the entrypoint
    // recognized as the composition root — unlike `hir_enclosing_body_owner`,
    // which stops at the innermost closure.
    let enclosing_item = cx.tcx.hir_get_parent_item(expr.hir_id).to_def_id();
    is_entrypoint_fn(cx, enclosing_item)
}

fn strip_generic_args(path: &str) -> Cow<'_, str> {
    if !path.contains("::<") {
        return Cow::Borrowed(path);
    }
    let mut normalized = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    let mut generic_depth = 0_usize;

    while let Some(ch) = chars.next() {
        if generic_depth == 0 && ch == ':' && chars.peek() == Some(&':') {
            let mut lookahead = chars.clone();
            lookahead.next();
            if lookahead.peek() == Some(&'<') {
                chars.next();
                chars.next();
                generic_depth = 1;
                continue;
            }
        }

        if generic_depth > 0 {
            match ch {
                '<' => generic_depth += 1,
                '>' => generic_depth -= 1,
                _ => {}
            }
            continue;
        }

        normalized.push(ch);
    }

    Cow::Owned(normalized)
}

/// Checks if `callee_path` (from `def_path_str`) matches any configured path.
/// Returns the matched path string for use in the diagnostic message.
pub fn find_matching_path<'a>(callee_path: &str, paths: &'a FxHashSet<String>) -> Option<&'a str> {
    let normalized = strip_generic_args(callee_path);
    paths.get(normalized.as_ref()).map(String::as_str)
}

/// Resolves the callee of `expr` and returns the matching configured path,
/// if any. Combines [`fn_def_id`], [`LateContext::tcx.def_path_str`], and
/// [`find_matching_path`] into a single helper for single-set lints.
///
/// For lints that check a call against *multiple* path sets, call
/// `cx.tcx.def_path_str` once and then invoke [`find_matching_path`] directly
/// for each set to avoid recomputing the callee path.
pub fn match_call_path<'a>(
    cx: &LateContext<'_>,
    expr: &Expr<'_>,
    paths: &'a FxHashSet<String>,
) -> Option<&'a str> {
    let def_id = fn_def_id(cx, expr)?;
    let callee_path = cx.tcx.def_path_str(def_id);
    find_matching_path(&callee_path, paths)
}

/// Builds the effective path set from defaults and config overrides.
/// If `config.paths` is `Some`, it replaces defaults entirely.
/// Otherwise, defaults are merged with `config.additional_paths`.
pub fn build_path_list(defaults: &[&str], config: &SubLintConfig) -> FxHashSet<String> {
    if let Some(ref overrides) = config.paths {
        overrides.iter().cloned().collect()
    } else {
        let mut merged: FxHashSet<String> = defaults.iter().map(|&s| s.to_owned()).collect();
        merged.extend(config.additional_paths.iter().cloned());
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::strip_generic_args;

    #[test]
    fn strip_generic_args_removes_turbofish_segments() {
        assert_eq!(
            strip_generic_args(
                "tracing_subscriber::fmt::SubscriberBuilder::<N, E, F, W>::try_init"
            ),
            "tracing_subscriber::fmt::SubscriberBuilder::try_init"
        );
        assert_eq!(
            strip_generic_args("foo::Bar::<Baz::<Qux>>::quux"),
            "foo::Bar::quux"
        );
        assert_eq!(strip_generic_args("std::env::var"), "std::env::var");
    }
}
