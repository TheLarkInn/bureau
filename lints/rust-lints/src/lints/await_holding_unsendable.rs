use clippy_utils::diagnostics::span_lint_and_then;
use rustc_data_structures::fx::FxHashMap;
use rustc_hir::{Closure, ClosureKind, CoroutineDesugaring, CoroutineKind, Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::mir::CoroutineLayout;
use rustc_middle::ty;
use rustc_span::Span;
use serde::Deserialize;

use super::suppression::is_in_test_zone;

rustc_session::declare_lint! {
    /// Flags values of specific types held alive across `.await` points.
    pub AWAIT_HOLDING_UNSENDABLE,
    Deny,
    "type held across `.await` \u{2014} can cause deadlocks, panics, or span corruption"
}

// ── Default types ───────────────────────────────────────────────────

// std::sync guards and std::cell refs are intentionally omitted — Clippy's
// `await_holding_lock` and `await_holding_refcell_ref` already cover them
// with diagnostic-item-based matching that is more robust than def_path_str.
// This lint complements those by covering types Clippy doesn't know about.
const DEFAULT_TYPES: &[(&str, &str)] = &[
    // parking_lot
    ("parking_lot::lock_api::MutexGuard", "deadlock"),
    ("parking_lot::lock_api::FairMutexGuard", "deadlock"),
    ("parking_lot::lock_api::RwLockReadGuard", "deadlock"),
    ("parking_lot::lock_api::RwLockWriteGuard", "deadlock"),
    ("parking_lot::lock_api::RwLockUpgradableReadGuard", "deadlock"),
    ("parking_lot::lock_api::MappedMutexGuard", "deadlock"),
    ("parking_lot::lock_api::MappedFairMutexGuard", "deadlock"),
    ("parking_lot::lock_api::MappedRwLockReadGuard", "deadlock"),
    ("parking_lot::lock_api::MappedRwLockWriteGuard", "deadlock"),
    ("parking_lot::lock_api::ArcMutexGuard", "deadlock"),
    ("parking_lot::lock_api::ArcRwLockReadGuard", "deadlock"),
    ("parking_lot::lock_api::ArcRwLockWriteGuard", "deadlock"),
    ("parking_lot::lock_api::ArcRwLockUpgradableReadGuard", "deadlock"),
    // tracing
    ("tracing::span::Entered", "corrupted span nesting \u{2014} events on other tasks attributed to wrong span"),
    ("tracing::span::EnteredSpan", "corrupted span nesting \u{2014} events on other tasks attributed to wrong span"),
    // crossbeam
    ("crossbeam_epoch::Guard", "delays memory reclamation \u{2014} unbounded memory growth while suspended"),
    // rusqlite
    ("rusqlite::Transaction", "holds exclusive connection lock \u{2014} blocks all other queries while suspended"),
    ("rusqlite::Savepoint", "holds exclusive connection lock \u{2014} blocks all other queries while suspended"),
    // connection pools
    ("r2d2::PooledConnection", "pool starvation \u{2014} connection unavailable to other tasks while suspended"),
    ("diesel::r2d2::PooledConnection", "pool starvation \u{2014} connection unavailable to other tasks while suspended"),
];

// ── Config ──────────────────────────────────────────────────────────

#[derive(Default, Deserialize)]
#[serde(default)]
struct AwaitHoldingUnsendableConfig {
    additional_types: Vec<String>,
    skip_default_types: bool,
}

// ── Lint pass ───────────────────────────────────────────────────────

pub struct AwaitHoldingUnsendable {
    types: FxHashMap<String, &'static str>,
}

impl AwaitHoldingUnsendable {
    pub fn new() -> Self {
        let config: AwaitHoldingUnsendableConfig =
            dylint_linting::config_or_default("await_holding_unsendable");

        let mut types = FxHashMap::default();
        if !config.skip_default_types {
            types.extend(
                DEFAULT_TYPES
                    .iter()
                    .map(|(path, risk)| ((*path).to_owned(), *risk)),
            );
        }
        types.extend(
            config
                .additional_types
                .into_iter()
                .map(|p| (p, "held across `.await`")),
        );

        Self { types }
    }
}

rustc_session::impl_lint_pass!(AwaitHoldingUnsendable => [AWAIT_HOLDING_UNSENDABLE]);

impl<'tcx> LateLintPass<'tcx> for AwaitHoldingUnsendable {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if let ExprKind::Closure(Closure {
            kind:
                ClosureKind::Coroutine(CoroutineKind::Desugared(CoroutineDesugaring::Async, _)),
            def_id,
            ..
        }) = expr.kind
            && !is_in_test_zone(cx, expr)
            && let Some(coroutine_layout) = cx.tcx.mir_coroutine_witnesses(*def_id)
        {
            self.check_interior_types(cx, coroutine_layout);
        }
    }
}

impl AwaitHoldingUnsendable {
    #[expect(
        clippy::indexing_slicing,
        reason = "`variant_fields` and `variant_source_info` are parallel \
                  `IndexVec<VariantIdx, _>`s, so the index from `iter_enumerated` \
                  on one is always valid in the other"
    )]
    fn check_interior_types(&self, cx: &LateContext<'_>, coroutine: &CoroutineLayout<'_>) {
        for (ty_index, ty_cause) in coroutine.field_tys.iter_enumerated() {
            if let ty::Adt(adt, _) = ty_cause.ty.kind() {
                let def_path = cx.tcx.def_path_str(adt.did());

                let Some(risk) = self.types.get(def_path.as_str()) else {
                    continue;
                };

                let short_name = cx.tcx.item_name(adt.did());

                let await_points: Vec<Span> = coroutine
                    .variant_source_info
                    .iter_enumerated()
                    .filter_map(|(variant, source_info)| {
                        coroutine.variant_fields[variant]
                            .raw
                            .contains(&ty_index)
                            .then_some(source_info.span)
                    })
                    .collect();

                span_lint_and_then(
                    cx,
                    AWAIT_HOLDING_UNSENDABLE,
                    ty_cause.source_info.span,
                    format!("`{short_name}` held across `.await` \u{2014} {risk}"),
                    |diag| {
                        diag.help(
                            "scope the guard so it is dropped before the `.await`, \
                             or use an async-aware alternative",
                        );
                        diag.span_note(
                            await_points,
                            "the value is held across these await points",
                        );
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ui_await_holding_unsendable() {
        crate::testing::run_ui_test("await_holding_unsendable", None, &[]);
    }
}
