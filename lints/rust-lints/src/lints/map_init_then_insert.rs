use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::{fn_def_id, is_expr_default};
use rustc_hir::def::Res;
use rustc_hir::{Block, Expr, ExprKind, HirId, PatKind, Path, QPath, Stmt, StmtKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::{self, Ty};
use rustc_span::{Symbol, sym};

rustc_session::declare_lint! {
    /// Warns when a `HashMap`, `BTreeMap`, `IndexMap`, `FxHashMap`, `AHashMap`,
    /// or similar map is created empty and then immediately populated with
    /// sequential `.insert()` calls.
    ///
    /// Suggests using `Type::from([...])` instead.
    pub MAP_INIT_THEN_INSERT,
    Warn,
    "immediately inserting into a newly created map \u{2014} consider using `Type::from([..])`"
}

/// Returns `true` if `stmt` is `<binding>.insert(k, v)` — a semicolon
/// expression statement calling the `insert` method on the given binding.
///
/// The receiver must be the bare local binding with no projections: an
/// indexing or field projection such as `m[i].insert(..)` or
/// `m.inner.insert(..)` targets the projected value, not the map itself, and
/// can't be rewritten to `Type::from([..])`.
fn is_insert_on_binding(stmt: &Stmt<'_>, binding_id: HirId) -> bool {
    let StmtKind::Semi(expr) = &stmt.kind else {
        return false;
    };

    if stmt.span.from_expansion() {
        return false;
    }

    let ExprKind::MethodCall(method, receiver, args, _) = &expr.kind else {
        return false;
    };

    if method.ident.as_str() != "insert" || args.len() != 2 {
        return false;
    }

    matches!(
        receiver.kind,
        ExprKind::Path(QPath::Resolved(
            _,
            Path {
                res: Res::Local(local),
                ..
            },
        )) if *local == binding_id
    )
}

/// Counts how many consecutive statements are `.insert(k, v)` calls on the
/// binding identified by `binding_id`. Stops at the first non-insert statement.
fn count_consecutive_inserts(stmts: &[Stmt<'_>], binding_id: HirId) -> usize {
    stmts
        .iter()
        .take_while(|stmt| is_insert_on_binding(stmt, binding_id))
        .count()
}

/// Extracts the type name segment written by the user from a `Type::method`
/// callee expression. Returns `Some("FxHashMap")` for `FxHashMap::new`,
/// `Some("HashMap")` for `HashMap::new`, etc. Returns `None` for bare
/// `Default::default()` calls where no explicit type is written on the callee.
fn callee_type_name(callee: &Expr<'_>) -> Option<rustc_span::Symbol> {
    let ExprKind::Path(QPath::TypeRelative(ty, _)) = &callee.kind else {
        return None;
    };
    let rustc_hir::TyKind::Path(QPath::Resolved(_, path)) = &ty.kind else {
        return None;
    };
    path.segments.last().map(|seg| seg.ident.name)
}

/// Returns `true` if the call expression is a recognized map constructor:
/// `Default::default()` (via the `default_fn` diagnostic item) or an
/// inherent `new`/`with_capacity` (matched by item name, since those are
/// standard constructor names shared across all map types).
fn is_map_constructor<'tcx>(cx: &LateContext<'tcx>, call: &'tcx Expr<'tcx>) -> bool {
    if is_expr_default(cx, call) {
        return true;
    }

    let Some(def_id) = fn_def_id(cx, call) else {
        return false;
    };

    matches!(cx.tcx.item_name(def_id).as_str(), "new" | "with_capacity")
}

/// Minimum number of consecutive `.insert()` calls required to fire the lint.
/// A single insert isn't worth rewriting.
const MIN_INSERTS: usize = 2;

pub struct MapInitThenInsert {
    // Cached symbols for third-party map detection (no diagnostic items exist).
    // Interned once in `new()` to avoid per-statement re-interning.
    sym_indexmap_crate: Symbol,
    sym_indexmap_type: Symbol,
    sym_ahash_crate: Symbol,
    sym_ahashmap_type: Symbol,
}

impl MapInitThenInsert {
    pub fn new() -> Self {
        Self {
            sym_indexmap_crate: Symbol::intern("indexmap"),
            sym_indexmap_type: Symbol::intern("IndexMap"),
            sym_ahash_crate: Symbol::intern("ahash"),
            sym_ahashmap_type: Symbol::intern("AHashMap"),
        }
    }

    /// Returns the display name if `ty` is a recognized map type, `None` otherwise.
    ///
    /// HashMap/BTreeMap use `is_diagnostic_item` (robust, compiler-provided).
    /// `IndexMap` uses `crate_name` + `item_name` matching (no diagnostic item
    /// exists for third-party crates).
    fn recognized_map_type<'tcx>(
        &self,
        cx: &LateContext<'tcx>,
        ty: Ty<'tcx>,
    ) -> Option<&'static str> {
        let ty::Adt(adt, _) = ty.kind() else {
            return None;
        };
        let def_id = adt.did();

        if cx.tcx.is_diagnostic_item(sym::HashMap, def_id) {
            Some("HashMap")
        } else if cx.tcx.is_diagnostic_item(sym::BTreeMap, def_id) {
            Some("BTreeMap")
        } else if cx.tcx.crate_name(def_id.krate) == self.sym_indexmap_crate
            && cx.tcx.item_name(def_id) == self.sym_indexmap_type
        {
            Some("IndexMap")
        } else if cx.tcx.crate_name(def_id.krate) == self.sym_ahash_crate
            && cx.tcx.item_name(def_id) == self.sym_ahashmap_type
        {
            Some("AHashMap")
        } else {
            None
        }
    }

    /// If `stmt` is `let [mut] <name> = <MapType>::new()` (or `::default()` or
    /// `::with_capacity(_)`), returns the binding's `HirId` along with the
    /// info needed to render a display name on demand.
    ///
    /// The display name is resolved lazily (see [`Self::format_type_name`])
    /// because rendering allocates and the vast majority of `let` bindings
    /// don't trigger the lint.
    fn map_init_binding<'tcx>(
        &self,
        cx: &LateContext<'tcx>,
        stmt: &'tcx Stmt<'tcx>,
    ) -> Option<(HirId, &'tcx Expr<'tcx>, &'static str)> {
        let StmtKind::Let(local) = &stmt.kind else {
            return None;
        };
        let init = local.init?;

        if stmt.span.from_expansion() {
            return None;
        }

        let ExprKind::Call(callee, _args) = &init.kind else {
            return None;
        };

        let ty = cx.typeck_results().expr_ty(init);
        let fallback_name = self.recognized_map_type(cx, ty)?;

        if !is_map_constructor(cx, init) {
            return None;
        }

        let PatKind::Binding(_, hir_id, _, _) = local.pat.kind else {
            return None;
        };

        Some((hir_id, callee, fallback_name))
    }

    /// Renders the display name for a map binding. Preferred: the name the
    /// user wrote on the callee (so that type aliases like `FxHashMap` render
    /// as `FxHashMap::from([..])`). Falls back to the resolved type name when
    /// the callee is a plain `Default::default()` without a type qualifier.
    fn format_type_name(callee: &Expr<'_>, fallback: &'static str) -> String {
        callee_type_name(callee).map_or_else(|| fallback.to_owned(), |s| s.to_string())
    }
}

rustc_session::impl_lint_pass!(MapInitThenInsert => [MAP_INIT_THEN_INSERT]);

impl<'tcx> LateLintPass<'tcx> for MapInitThenInsert {
    #[expect(
        clippy::indexing_slicing,
        reason = "indices are bounds-checked by the while condition"
    )]
    fn check_block(&mut self, cx: &LateContext<'tcx>, block: &'tcx Block<'tcx>) {
        let stmts = block.stmts;
        let mut i = 0;

        while i < stmts.len() {
            let Some((binding_id, callee, fallback_name)) =
                self.map_init_binding(cx, &stmts[i])
            else {
                i += 1;
                continue;
            };

            let insert_start = i + 1;
            let insert_count = count_consecutive_inserts(&stmts[insert_start..], binding_id);

            if insert_count >= MIN_INSERTS {
                let init_span = stmts[i].span;
                let last_insert_span = stmts[insert_start + insert_count - 1].span;
                let full_span = init_span.to(last_insert_span);
                let map_type_name = Self::format_type_name(callee, fallback_name);

                span_lint_and_help(
                    cx,
                    MAP_INIT_THEN_INSERT,
                    full_span,
                    format!(
                        "immediately inserting into a newly created map \
                         \u{2014} consider using `{map_type_name}::from([..])`"
                    ),
                    None,
                    format!(
                        "use `let m = {map_type_name}::from([..])` to initialize the map inline"
                    ),
                );
            }

            i = insert_start + insert_count;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ui_map_init_then_insert() {
        crate::testing::run_ui_test("map_init_then_insert", None, &[]);
    }
}
