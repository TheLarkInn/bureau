use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::fn_def_id;
use rustc_data_structures::fx::FxHashSet;
use rustc_hir::Expr;
use rustc_lint::{LateContext, LateLintPass, Lint};

use super::call_matching::{build_path_list, find_matching_path, is_in_suppression_zone};
use crate::config::{GlobalSideEffectConfig, SubLintConfig};

rustc_session::declare_lint! {
    /// Flags direct calls to wall-clock or monotonic time functions.
    pub GLOBAL_SIDE_EFFECT_TIME,
    Warn,
    "direct call to a time function \u{2014} accept a time parameter instead"
}

rustc_session::declare_lint! {
    /// Flags direct calls to random number generation functions.
    pub GLOBAL_SIDE_EFFECT_RANDOMNESS,
    Warn,
    "direct call to a random function \u{2014} accept an `impl Rng` parameter instead"
}

rustc_session::declare_lint! {
    /// Flags direct calls to environment variable or CLI argument functions.
    pub GLOBAL_SIDE_EFFECT_ENV,
    Warn,
    "direct call to an environment function \u{2014} pass the value as a parameter instead"
}

rustc_session::declare_lint! {
    /// Flags global tracing subscriber initialization outside `main()`.
    pub GLOBAL_SIDE_EFFECT_LOGGING_INIT,
    Deny,
    "global tracing subscriber initialization outside `main()`"
}

const DEFAULT_TIME_PATHS: &[&str] = &[
    "std::time::SystemTime::now",
    "std::time::Instant::now",
    "chrono::Utc::now",
    "chrono::Local::now",
    "chrono::offset::Utc::now",
    "chrono::offset::Local::now",
    "time::OffsetDateTime::now_utc",
    "time::OffsetDateTime::now_local",
    "time::Instant::now",
    "jiff::Zoned::now",
    "jiff::Timestamp::now",
    "tokio::time::Instant::now",
];

const DEFAULT_RANDOMNESS_PATHS: &[&str] = &[
    "std::random::random",
    // rand 0.8
    "rand::thread_rng",
    // rand 0.9+
    "rand::rng",
    "rand::random",
    "rand::random_range",
    "rand::rngs::OsRng::new",
    "rand::rngs::StdRng::from_os_rng",
    // getrandom
    "getrandom::getrandom",
    // fastrand
    "fastrand::bool",
    "fastrand::u8",
    "fastrand::u16",
    "fastrand::u32",
    "fastrand::u64",
    "fastrand::u128",
    "fastrand::usize",
    "fastrand::i8",
    "fastrand::i16",
    "fastrand::i32",
    "fastrand::i64",
    "fastrand::i128",
    "fastrand::isize",
    "fastrand::f32",
    "fastrand::f64",
    "fastrand::char",
    "fastrand::Rng::new",
];

const DEFAULT_ENV_PATHS: &[&str] = &[
    // std
    "std::env::var",
    "std::env::var_os",
    "std::env::vars",
    "std::env::vars_os",
    "std::env::args",
    "std::env::args_os",
    // dotenvy
    "dotenvy::dotenv",
    "dotenvy::dotenv_override",
    "dotenvy::from_filename",
    "dotenvy::var",
    "dotenvy::vars",
    // dotenv (unmaintained predecessor)
    "dotenv::dotenv",
    "dotenv::var",
    "dotenv::vars",
];

const DEFAULT_LOGGING_INIT_PATHS: &[&str] = &[
    "tracing_subscriber::fmt::init",
    "tracing_subscriber::fmt::try_init",
    "tracing_subscriber::fmt::SubscriberBuilder::init",
    "tracing_subscriber::fmt::SubscriberBuilder::try_init",
    "tracing_subscriber::util::SubscriberInitExt::init",
    "tracing_subscriber::util::SubscriberInitExt::try_init",
    "tracing::subscriber::set_global_default",
];

const HELP_TIME: &str =
    "accept a time parameter or use a clock trait so callers can control the time source in tests";
const HELP_RANDOMNESS: &str = "accept an `impl Rng` parameter so callers can inject a seeded RNG";
const HELP_ENV: &str =
    "move this to your application's entry point and pass the value as a parameter";
const HELP_LOGGING_INIT: &str = "move global tracing subscriber initialization to `main()` so library code does not mutate process-global state";

/// One category's rule: lint to emit, path set to match against, help text.
struct Sublint {
    lint: &'static Lint,
    paths: FxHashSet<String>,
    help: &'static str,
}

impl Sublint {
    fn new(
        lint: &'static Lint,
        defaults: &[&str],
        config: &SubLintConfig,
        help: &'static str,
    ) -> Self {
        Self {
            lint,
            paths: build_path_list(defaults, config),
            help,
        }
    }
}

/// Single pass over four categories — one HIR traversal, four path-set lookups
/// per call expression.
pub struct GlobalSideEffect {
    sublints: [Sublint; 4],
}

impl GlobalSideEffect {
    pub fn new() -> Self {
        let config: GlobalSideEffectConfig =
            dylint_linting::config_or_default("global_side_effect");

        Self {
            sublints: [
                Sublint::new(
                    GLOBAL_SIDE_EFFECT_TIME,
                    DEFAULT_TIME_PATHS,
                    &config.time,
                    HELP_TIME,
                ),
                Sublint::new(
                    GLOBAL_SIDE_EFFECT_RANDOMNESS,
                    DEFAULT_RANDOMNESS_PATHS,
                    &config.randomness,
                    HELP_RANDOMNESS,
                ),
                Sublint::new(
                    GLOBAL_SIDE_EFFECT_ENV,
                    DEFAULT_ENV_PATHS,
                    &config.env,
                    HELP_ENV,
                ),
                Sublint::new(
                    GLOBAL_SIDE_EFFECT_LOGGING_INIT,
                    DEFAULT_LOGGING_INIT_PATHS,
                    &config.logging_init,
                    HELP_LOGGING_INIT,
                ),
            ],
        }
    }
}

rustc_session::impl_lint_pass!(GlobalSideEffect => [
    GLOBAL_SIDE_EFFECT_TIME,
    GLOBAL_SIDE_EFFECT_RANDOMNESS,
    GLOBAL_SIDE_EFFECT_ENV,
    GLOBAL_SIDE_EFFECT_LOGGING_INIT,
]);

impl<'tcx> LateLintPass<'tcx> for GlobalSideEffect {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }

        let Some(def_id) = fn_def_id(cx, expr) else {
            return;
        };

        let callee_path = cx.tcx.def_path_str(def_id);
        let Some((sublint, matched_path)) = self
            .sublints
            .iter()
            .find_map(|s| find_matching_path(&callee_path, &s.paths).map(|p| (s, p)))
        else {
            return;
        };

        // Suppression check runs only after a match, since most expressions
        // are not flagged and the HIR parent walk is the expensive step.
        if is_in_suppression_zone(cx, expr) {
            return;
        }

        span_lint_and_help(
            cx,
            sublint.lint,
            expr.span,
            format!("direct call to `{matched_path}()`"),
            None,
            sublint.help,
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ui_global_side_effect() {
        crate::testing::run_ui_test("global_side_effect", None, &[]);
    }
}
