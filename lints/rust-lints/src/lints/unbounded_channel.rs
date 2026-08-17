use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::Expr;
use rustc_lint::{LateContext, LateLintPass};

use rustc_data_structures::fx::FxHashSet;

use super::call_matching::{build_path_list, is_in_suppression_zone, match_call_path};
use crate::config::SubLintConfig;

rustc_session::declare_lint! {
    /// Flags creation of unbounded channels, which can exhaust memory
    /// under backpressure.
    pub UNBOUNDED_CHANNEL,
    Deny,
    "unbounded channel created \u{2014} can exhaust memory under backpressure"
}

const DEFAULT_PATHS: &[&str] = &[
    // std (unbounded by default — no capacity parameter)
    "std::sync::mpsc::channel",
    // tokio
    "tokio::sync::mpsc::unbounded_channel",
    // flume
    "flume::unbounded",
    // crossbeam
    "crossbeam_channel::unbounded",
    "crossbeam::channel::unbounded",
];

const HELP: &str = "use a bounded channel with an explicit capacity to enable backpressure \
                     (e.g., `mpsc::channel(1000)`)";

pub struct UnboundedChannel {
    paths: FxHashSet<String>,
}

impl UnboundedChannel {
    pub fn new() -> Self {
        let config: SubLintConfig = dylint_linting::config_or_default("unbounded_channel");

        Self {
            paths: build_path_list(DEFAULT_PATHS, &config),
        }
    }
}

rustc_session::impl_lint_pass!(UnboundedChannel => [UNBOUNDED_CHANNEL]);

impl<'tcx> LateLintPass<'tcx> for UnboundedChannel {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        // Skip macro-generated code.
        if expr.span.from_expansion() {
            return;
        }

        let Some(matched_path) = match_call_path(cx, expr, &self.paths) else {
            return;
        };

        if is_in_suppression_zone(cx, expr) {
            return;
        }

        span_lint_and_help(
            cx,
            UNBOUNDED_CHANNEL,
            expr.span,
            format!("`{matched_path}()` creates an unbounded channel — no backpressure"),
            None,
            HELP,
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ui_unbounded_channel() {
        crate::testing::run_ui_test("unbounded_channel", None, &[]);
    }
}
