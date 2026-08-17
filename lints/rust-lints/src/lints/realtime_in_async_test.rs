//! Flags two clock-correctness issues in async tests using Tokio time:
//!
//! 1. `tokio::time::sleep` / `timeout` / `interval` / `sleep_until` without
//!    `start_paused = true` — these wait on real time, slowing CI.
//! 2. `std::time::Instant::now()` inside a test with `start_paused = true` —
//!    it doesn't respect Tokio's paused clock (`tokio::time::Instant` should
//!    be used instead).
//!
//! # Detection approach
//!
//! `#[tokio::test]` is a proc macro that expands into a `#[test]` fn wrapping
//! the user's async body in a tokio runtime. The `start_paused = true` variant
//! generates a `.start_paused(true)` call on the runtime builder.
//!
//! We detect this from the expanded code:
//! 1. Find test functions (via `rustc_test_marker` / `is_in_test`).
//! 2. Walk the body for time-related calls (`tokio::time::sleep`, etc.).
//! 3. Walk the body for `std::time::Instant::now()` calls.
//! 4. Walk the body for `.start_paused(true)` — present when the user wrote
//!    `#[tokio::test(start_paused = true)]`.
//! 5. Fire if time calls found but no `start_paused(true)`.
//! 6. Fire if `std::time::Instant::now()` found with `start_paused(true)`.
//!
//! This avoids depending on the proc macro's attribute syntax (consumed before
//! HIR) and instead observes the generated code.
//!
//! # Scope
//!
//! - Only fires inside test functions (not production async code).
//! - Only fires on `tokio::time::*` calls, not `std::thread::sleep`
//!   (which is a different problem — see `blocking_in_async`).
//! - Suppressed by `#[allow]`.
//! - Does NOT fire on `tokio::time::advance` (that's the solution, not the
//!   problem).

use std::ops::ControlFlow;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::fn_def_id;
use clippy_utils::is_test_function;
use clippy_utils::visitors::for_each_expr;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{Body, Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::hir::nested_filter;

use rustc_data_structures::fx::FxHashSet;

use super::call_matching::{build_path_list, find_matching_path, resolve_callee_def_id_with_typeck};
use crate::config::SubLintConfig;

rustc_session::declare_lint! {
    /// Flags real-time waits in async tests that should use a paused clock.
    pub REALTIME_IN_ASYNC_TEST,
    Warn,
    "real-time wait in async test \u{2014} use `#[tokio::test(start_paused = true)]`"
}

const DEFAULT_TIME_PATHS: &[&str] = &[
    "tokio::time::sleep",
    "tokio::time::sleep_until",
    "tokio::time::timeout",
    "tokio::time::timeout_at",
    "tokio::time::interval",
    "tokio::time::interval_at",
];

const STD_INSTANT_NOW: &str = "std::time::Instant::now";

const HELP: &str = "switch to `#[tokio::test(start_paused = true)]` to resolve \
                     sleeps instantly; use `tokio::time::advance()` for precise control";

const STD_INSTANT_HELP: &str = "use `tokio::time::Instant::now()` instead; \
                                it advances with `tokio::time::advance()` and auto-advance";

/// Returns `true` if `expr` is a call to `tokio::runtime::Builder::start_paused(true)`.
///
/// The `method.ident` prefilter keeps this cheap on the visitor's hot path —
/// the `def_path_str` lookup only runs on method calls literally named
/// `start_paused`.
fn is_start_paused_true(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    if let ExprKind::MethodCall(method, _receiver, [arg], _span) = &expr.kind
        && method.ident.as_str() == "start_paused"
        && let ExprKind::Lit(lit) = &arg.kind
        && matches!(lit.node, rustc_ast::LitKind::Bool(true))
        && let Some(def_id) = fn_def_id(cx, expr)
    {
        return cx.tcx.def_path_str(def_id) == "tokio::runtime::Builder::start_paused";
    }
    false
}

/// Walks a function body looking for tokio time calls and `start_paused(true)`.
/// Collects local callees so the caller can check them transitively.
struct TimeCallVisitor<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    time_paths: &'a FxHashSet<String>,
    /// Span of the first tokio time call found (for diagnostic pointing).
    first_time_call_span: Option<rustc_span::Span>,
    /// Span of the first `std::time::Instant::now()` call found.
    std_instant_now_span: Option<rustc_span::Span>,
    /// Whether `.start_paused(true)` was found in the body.
    has_start_paused_true: bool,
    /// Local functions called from this body (checked transitively after visit).
    local_callees: Vec<(LocalDefId, rustc_span::Span)>,
}

impl<'tcx> Visitor<'tcx> for TimeCallVisitor<'_, 'tcx> {
    type NestedFilter = nested_filter::OnlyBodies;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.cx.tcx
    }

    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        // Short-circuit: once we have all signals, the outcome is determined.
        if self.first_time_call_span.is_some()
            && self.has_start_paused_true
            && self.std_instant_now_span.is_some()
        {
            return;
        }

        let needs_time_check = self.first_time_call_span.is_none();
        let needs_instant_check = self.std_instant_now_span.is_none();

        if (needs_time_check || needs_instant_check)
            && let Some(def_id) = fn_def_id(self.cx, expr)
        {
            let callee_path = self.cx.tcx.def_path_str(def_id);

            if needs_time_check && find_matching_path(&callee_path, self.time_paths).is_some() {
                self.first_time_call_span = Some(expr.span);
            }
            if needs_instant_check && callee_path == STD_INSTANT_NOW {
                self.std_instant_now_span = Some(expr.span);
            }
            if needs_time_check
                && let Some(local_id) = def_id.as_local()
            {
                self.local_callees.push((local_id, expr.span));
            }
        }

        if !self.has_start_paused_true && is_start_paused_true(self.cx, expr) {
            self.has_start_paused_true = true;
        }

        intravisit::walk_expr(self, expr);
    }
}

/// Checks whether a local function (transitively) calls any tokio time path.
/// Does NOT check `std::time::Instant::now()` — that's a separate concern
/// (Case 2) handled only for direct calls in the test body.
/// Uses the callee's own typeck results so it is safe to call on any
/// `LocalDefId`. Recurses into local callees with cycle detection via `visited`.
fn has_transitive_time_call(
    cx: &LateContext<'_>,
    local_id: LocalDefId,
    time_paths: &FxHashSet<String>,
    visited: &mut FxHashSet<LocalDefId>,
) -> bool {
    if !visited.insert(local_id) {
        return false;
    }
    let Some(body) = cx.tcx.hir_maybe_body_owned_by(local_id) else {
        return false;
    };
    let typeck = cx.tcx.typeck(local_id);

    // `for_each_expr` walks into async blocks (which share the parent's
    // TypeckResults) and closures. For closures the typeck lookup may
    // return None — that's a harmless false negative, not a false positive.
    for_each_expr(cx, body, |expr| {
        if let Some(def_id) = resolve_callee_def_id_with_typeck(typeck, expr) {
            // Local functions can't match external tokio paths — recurse directly.
            if let Some(callee_local) = def_id.as_local() {
                if has_transitive_time_call(cx, callee_local, time_paths, visited) {
                    return ControlFlow::Break(());
                }
            } else {
                let callee_path = cx.tcx.def_path_str(def_id);
                if find_matching_path(&callee_path, time_paths).is_some() {
                    return ControlFlow::Break(());
                }
            }
        }
        ControlFlow::Continue(())
    })
    .is_some()
}

pub struct RealtimeInAsyncTest {
    time_paths: FxHashSet<String>,
}

impl RealtimeInAsyncTest {
    pub fn new() -> Self {
        let config: SubLintConfig = dylint_linting::config_or_default("realtime_in_async_test");
        Self {
            time_paths: build_path_list(DEFAULT_TIME_PATHS, &config),
        }
    }
}

rustc_session::impl_lint_pass!(RealtimeInAsyncTest => [REALTIME_IN_ASYNC_TEST]);

impl<'tcx> LateLintPass<'tcx> for RealtimeInAsyncTest {
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        kind: rustc_hir::intravisit::FnKind<'tcx>,
        _decl: &'tcx rustc_hir::FnDecl<'tcx>,
        body: &'tcx Body<'tcx>,
        _span: rustc_span::Span,
        def_id: rustc_hir::def_id::LocalDefId,
    ) {
        // Only top-level test functions (skip closures, async blocks, helpers).
        // `is_in_test` is too broad — it matches any function inside a
        // `#[cfg(test)]` module. `is_test_function` checks that this specific
        // function has the `#[test]` attribute (which `#[tokio::test]` expands
        // to include).
        if !matches!(kind, rustc_hir::intravisit::FnKind::ItemFn(..))
            || !is_test_function(cx.tcx, def_id)
        {
            return;
        }

        // Walk the entire body (including proc-macro-generated runtime setup)
        // collecting two signals.
        let mut visitor = TimeCallVisitor {
            cx,
            time_paths: &self.time_paths,
            first_time_call_span: None,
            std_instant_now_span: None,
            has_start_paused_true: false,
            local_callees: Vec::new(),
        };
        intravisit::walk_body(&mut visitor, body);

        // If no direct time call was found and the clock isn't paused (Case 1
        // would be suppressed anyway), check local callees transitively.
        if visitor.first_time_call_span.is_none() && !visitor.has_start_paused_true {
            let mut visited = FxHashSet::default();
            for &(callee_id, call_span) in &visitor.local_callees {
                if has_transitive_time_call(cx, callee_id, &self.time_paths, &mut visited) {
                    visitor.first_time_call_span = Some(call_span);
                    break;
                }
            }
        }

        // Case 1: tokio time call without paused clock.
        if let Some(time_span) = visitor.first_time_call_span {
            if !visitor.has_start_paused_true {
                span_lint_and_help(
                    cx,
                    REALTIME_IN_ASYNC_TEST,
                    time_span,
                    "real-time wait in async test without paused clock",
                    None,
                    HELP,
                );
            }
        }

        // Case 2: std::time::Instant::now() with paused clock.
        if let Some(instant_span) = visitor.std_instant_now_span {
            if visitor.has_start_paused_true {
                span_lint_and_help(
                    cx,
                    REALTIME_IN_ASYNC_TEST,
                    instant_span,
                    "`std::time::Instant::now()` does not respect Tokio's paused clock",
                    None,
                    STD_INSTANT_HELP,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ui_realtime_in_async_test() {
        crate::testing::run_ui_test("realtime_in_async_test", None, &["--test"]);
    }
}
