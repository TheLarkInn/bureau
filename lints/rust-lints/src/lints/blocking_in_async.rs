use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::fn_def_id;
use rustc_hir::{ClosureKind, CoroutineDesugaring, CoroutineKind, Expr, ExprKind, HirId, Node};
use rustc_lint::{LateContext, LateLintPass};

use rustc_data_structures::fx::FxHashSet;

use super::call_matching::{build_path_list, match_call_path};
use super::suppression::is_in_test_zone;
use crate::config::SubLintConfig;

rustc_session::declare_lint! {
    /// Flags known-blocking operations inside `async fn` or `async {}` blocks.
    pub BLOCKING_IN_ASYNC,
    Deny,
    "blocking call inside async context \u{2014} starves the executor"
}

const DEFAULT_PATHS: &[&str] = &[
    // std::fs
    "std::fs::read",
    "std::fs::read_to_string",
    "std::fs::write",
    "std::fs::read_dir",
    "std::fs::metadata",
    "std::fs::canonicalize",
    "std::fs::copy",
    "std::fs::create_dir",
    "std::fs::create_dir_all",
    "std::fs::remove_file",
    "std::fs::remove_dir",
    "std::fs::remove_dir_all",
    "std::fs::rename",
    // std::net
    "std::net::TcpStream::connect",
    "std::net::TcpListener::bind",
    "std::net::UdpSocket::bind",
    // std::thread
    "std::thread::sleep",
    // std::io — stdin methods are MethodCall, matched by path
    "std::io::Stdin::read_line",
    "std::io::Stdin::read",
    // std::sync
    "std::sync::Mutex::lock",
    "std::sync::RwLock::read",
    "std::sync::RwLock::write",
    // parking_lot — `Mutex`/`RwLock` are `lock_api` types re-exported by
    // parking_lot, so `def_path_str` yields the `lock_api` segment.
    "parking_lot::lock_api::Mutex::lock",
    "parking_lot::lock_api::RwLock::read",
    "parking_lot::lock_api::RwLock::write",
    // std::thread::spawn — bypasses executor
    "std::thread::spawn",
    // tokio::task::block_in_place — risky on single-threaded executors
    "tokio::task::block_in_place",
];

/// Paths that act as "escape hatches" — if the blocking call is inside a
/// closure passed to one of these, it's intentional.
const SPAWN_BLOCKING_PATHS: &[&str] = &[
    "tokio::task::spawn_blocking",
    "async_std::task::spawn_blocking",
];

const HELP: &str = "use an async-aware alternative, or wrap the blocking call \
                     in `tokio::task::spawn_blocking()`";

/// Returns `true` if the closure identified by `closure_hir_id` is invoked
/// synchronously at its definition site, so its body runs in the enclosing
/// execution context rather than being deferred to another thread/task.
///
/// Two shapes qualify: an immediately-invoked closure (`(|| …)()`, where the
/// closure is the call target) and a closure passed as an argument to a method
/// call (`recv.for_each(|…| …)`, `opt.map(|…| …)`, …), which iterator / `Option`
/// / `Result` adapters drive on the spot. A closure passed to a *free function*
/// (e.g. `std::thread::spawn(|| …)`, `spawn_blocking(|| …)`) is the call target's
/// argument but not synchronously invoked, so it is correctly treated as opaque.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "ExprKind has many variants; only call/method-call parents matter"
)]
fn is_synchronously_invoked(cx: &LateContext<'_>, closure_hir_id: HirId) -> bool {
    let Node::Expr(parent) = cx.tcx.parent_hir_node(closure_hir_id) else {
        return false;
    };
    match parent.kind {
        // IIFE: the closure is the thing being called.
        ExprKind::Call(callee, _) => callee.hir_id == closure_hir_id,
        // Iterator / `Option` / `Result` adapter: `recv.method(|…| …)` drives
        // the closure synchronously on the current thread.
        ExprKind::MethodCall(..) => true,
        _ => false,
    }
}

/// Returns `true` if `expr` is syntactically inside an `async fn` or
/// `async {}` block.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "we only care about closures and function boundaries"
)]
fn is_in_async_context(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    for (hir_id, node) in cx.tcx.hir_parent_iter(expr.hir_id) {
        match node {
            Node::Expr(Expr {
                kind: ExprKind::Closure(c),
                ..
            }) => {
                // An async coroutine closure puts us directly on the executor.
                if matches!(
                    c.kind,
                    ClosureKind::Coroutine(CoroutineKind::Desugared(
                        CoroutineDesugaring::Async,
                        _,
                    ))
                ) {
                    return true;
                }
                // A sync closure normally breaks the chain — its body runs
                // wherever it is later invoked, not necessarily on the executor.
                // But a closure that is invoked *synchronously in place* still
                // runs on the executor: an IIFE (`(|| …)()`) or one handed to an
                // iterator / `Option` / `Result` adapter (`.for_each`, `.map`,
                // …). Treat those as transparent and keep walking outward;
                // otherwise the chain is genuinely broken.
                if !is_synchronously_invoked(cx, hir_id) {
                    return false;
                }
            }
            Node::Item(_) | Node::ImplItem(_) | Node::TraitItem(_) => return false,
            _ => {}
        }
    }
    false
}

/// Returns `true` if `expr` is inside a closure passed to
/// `tokio::task::spawn_blocking()` or equivalent escape hatch.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "we only care about closures and function boundaries"
)]
fn is_inside_spawn_blocking(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    for (hir_id, node) in cx.tcx.hir_parent_iter(expr.hir_id) {
        match node {
            Node::Expr(Expr {
                kind: ExprKind::Closure(_),
                ..
            }) => {
                if let Node::Expr(parent) = cx.tcx.hir_node(cx.tcx.parent_hir_id(hir_id))
                    && let Some(def_id) = fn_def_id(cx, parent)
                {
                    let path = cx.tcx.def_path_str(def_id);
                    if SPAWN_BLOCKING_PATHS.iter().any(|&p| p == path) {
                        return true;
                    }
                }
            }
            Node::Item(_) | Node::ImplItem(_) | Node::TraitItem(_) => break,
            _ => {}
        }
    }
    false
}

pub struct BlockingInAsync {
    paths: FxHashSet<String>,
}

impl BlockingInAsync {
    pub fn new() -> Self {
        let config: SubLintConfig = dylint_linting::config_or_default("blocking_in_async");

        Self {
            paths: build_path_list(DEFAULT_PATHS, &config),
        }
    }
}

rustc_session::impl_lint_pass!(BlockingInAsync => [BLOCKING_IN_ASYNC]);

impl<'tcx> LateLintPass<'tcx> for BlockingInAsync {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }

        let Some(matched_path) = match_call_path(cx, expr, &self.paths) else {
            return;
        };

        // Ordered cheap-first: attribute-based test check before HIR parent walks.
        if is_in_test_zone(cx, expr)
            || !is_in_async_context(cx, expr)
            || is_inside_spawn_blocking(cx, expr)
        {
            return;
        }

        span_lint_and_help(
            cx,
            BLOCKING_IN_ASYNC,
            expr.span,
            format!("blocking call to `{matched_path}()` inside async context"),
            None,
            HELP,
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ui_blocking_in_async() {
        crate::testing::run_ui_test("blocking_in_async", None, &[]);
    }
}
