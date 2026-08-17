use clippy_utils::diagnostics::span_lint_and_help;
use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_hir::definitions::DefPathData;
use rustc_hir::{Expr, HirId, Item};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;
use rustc_span::{Span, Symbol};

use super::hir_refs;
use crate::config::ModuleDependenciesConfig;

rustc_session::declare_lint! {
    /// Flags cross-module dependencies not declared in the allowlist.
    ///
    /// Each top-level module declares which other top-level modules it may
    /// depend on via `[module_dependencies.allow]` in `dylint.toml`. Any
    /// reference to an item in an undeclared module is a compile-time error.
    pub MODULE_DEPENDENCIES,
    Deny,
    "cross-module dependency not declared in allowlist"
}

rustc_session::declare_lint! {
    /// Flags modules not listed in the config when exhaustive mode is enabled.
    pub MODULE_DEPENDENCIES_UNLISTED,
    Deny,
    "module not listed in module_dependencies config (exhaustive mode)"
}

rustc_session::declare_lint! {
    /// Flags edges declared in the allowlist that have no corresponding
    /// dependency in code. Stale edges make the config lie.
    pub MODULE_DEPENDENCIES_DEAD_EDGE,
    Warn,
    "allowlist edge has no corresponding dependency in code"
}

/// Extracts the top-level module name for a local `DefId`.
///
/// Returns `None` for items at the crate root or external crate items.
fn top_level_module(tcx: TyCtxt<'_>, def_id: DefId) -> Option<Symbol> {
    if !def_id.is_local() {
        return None;
    }
    tcx.def_path(def_id).data.iter().find_map(|d| {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "only the TypeNs variant names a module"
        )]
        match d.data {
            DefPathData::TypeNs(sym) => Some(sym),
            _ => None,
        }
    })
}

pub struct ModuleDependencies {
    exhaustive: bool,
    /// Raw config entries: `(crate scope, module, deps)`. A `None` scope
    /// applies to every crate; `Some(krate)` only to that crate, so one
    /// workspace-wide `dylint.toml` can declare distinct module graphs.
    entries: Vec<(Option<Symbol>, Symbol, Vec<Symbol>)>,
    allow: FxHashMap<Symbol, FxHashSet<Symbol>>,
    all_modules: FxHashSet<Symbol>,
    /// Whether `allow`/`all_modules` have been filtered to the current crate.
    scoped: bool,
    /// Tracks which declared edges were actually observed in code.
    used_edges: FxHashSet<(Symbol, Symbol)>,
    /// Top-level modules observed in this crate as a reference source or
    /// target. Dead edges are only flagged when both endpoint modules are
    /// present in this crate.
    seen_modules: FxHashSet<Symbol>,
    warned_unlisted: FxHashSet<Symbol>,
}

impl ModuleDependencies {
    pub fn new() -> Self {
        let config: ModuleDependenciesConfig =
            dylint_linting::config_or_default("module_dependencies");

        let entries = config
            .allow
            .iter()
            .map(|(module, deps)| {
                let (scope, name) = module
                    .split_once(':')
                    .map_or((None, module.as_str()), |(krate, name)| {
                        (Some(Symbol::intern(krate)), name)
                    });
                (
                    scope,
                    Symbol::intern(name),
                    deps.iter().map(|d| Symbol::intern(d)).collect(),
                )
            })
            .collect();

        Self {
            exhaustive: config.exhaustive,
            entries,
            allow: FxHashMap::default(),
            all_modules: FxHashSet::default(),
            scoped: false,
            used_edges: FxHashSet::default(),
            seen_modules: FxHashSet::default(),
            warned_unlisted: FxHashSet::default(),
        }
    }

    /// Filters the workspace-wide config to the crate being linted. Runs once
    /// per crate, on the first lint callback.
    fn scope_to_crate(&mut self, tcx: TyCtxt<'_>) {
        if self.scoped {
            return;
        }
        self.scoped = true;
        let crate_name = tcx.crate_name(rustc_span::def_id::LOCAL_CRATE);
        for (scope, module, deps) in &self.entries {
            if scope.is_some_and(|krate| krate != crate_name) {
                continue;
            }
            let dep_set: FxHashSet<Symbol> = deps.iter().copied().collect();
            for &dep in &dep_set {
                self.all_modules.insert(dep);
            }
            self.all_modules.insert(*module);
            self.allow.insert(*module, dep_set);
        }
    }

    fn is_configured(&self) -> bool {
        !self.entries.is_empty() || self.exhaustive
    }

    fn warn_unlisted_once(&mut self, cx: &LateContext<'_>, module: Symbol, span: Span) -> bool {
        if self.all_modules.contains(&module) || !self.warned_unlisted.insert(module) {
            return false;
        }
        span_lint_and_help(
            cx,
            MODULE_DEPENDENCIES_UNLISTED,
            span,
            format!("module `{module}` is not listed in module_dependencies config"),
            None,
            "add this module to [module_dependencies.allow] in dylint.toml",
        );
        true
    }

    fn check_dependency(&mut self, cx: &LateContext<'_>, def_id: DefId, hir_id: HirId, span: Span) {
        if !self.is_configured() || hir_refs::should_skip_ref(cx, def_id, hir_id, span) {
            return;
        }
        self.scope_to_crate(cx.tcx);

        let source_mod = cx.tcx.parent_module(hir_id);
        let source_top = top_level_module(cx.tcx, source_mod.to_def_id());
        if let Some(source) = source_top {
            self.seen_modules.insert(source);
        }
        let target_top = top_level_module(cx.tcx, def_id);
        if let Some(target) = target_top {
            self.seen_modules.insert(target);
        }

        let (Some(source), Some(target)) = (source_top, target_top) else {
            return;
        };

        if source == target {
            return;
        }

        if self.exhaustive
            && (self.warn_unlisted_once(cx, source, span)
                || self.warn_unlisted_once(cx, target, span))
        {
            return;
        }

        // In non-exhaustive mode, unconfigured source modules are silently allowed.
        let Some(allowed) = self.allow.get(&source) else {
            return;
        };

        self.used_edges.insert((source, target));

        if allowed.contains(&target) {
            return;
        }

        let mut allowed_list: Vec<_> = allowed.iter().map(|s| s.as_str().to_owned()).collect();
        allowed_list.sort();
        let allowed_str = if allowed_list.is_empty() {
            "none".to_owned()
        } else {
            allowed_list.join(", ")
        };

        span_lint_and_help(
            cx,
            MODULE_DEPENDENCIES,
            span,
            format!("`{source}` depends on `{target}`, which is not in its allowlist"),
            None,
            format!(
                "if this dependency is architecturally correct, add \"{target}\" to the \
                 `{source}` allowlist in dylint.toml under [module_dependencies.allow]\n\
                 if not, move the item to a module that `{source}` is allowed to depend on \
                 (currently: {allowed_str})"
            ),
        );
    }
}

rustc_session::impl_lint_pass!(ModuleDependencies => [MODULE_DEPENDENCIES, MODULE_DEPENDENCIES_UNLISTED, MODULE_DEPENDENCIES_DEAD_EDGE]);

impl<'tcx> LateLintPass<'tcx> for ModuleDependencies {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if let Some((def_id, hir_id, span)) = hir_refs::resolve_expr_def_id(cx, expr) {
            self.check_dependency(cx, def_id, hir_id, span);
        }
    }

    fn check_ty(
        &mut self,
        cx: &LateContext<'tcx>,
        ty: &'tcx rustc_hir::Ty<'tcx, rustc_hir::AmbigArg>,
    ) {
        if let Some((def_id, hir_id, span)) = hir_refs::resolve_ty_def_id(cx, ty) {
            self.check_dependency(cx, def_id, hir_id, span);
        }
    }

    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        hir_refs::for_each_use_def_id(item, |def_id, hir_id, span| {
            self.check_dependency(cx, def_id, hir_id, span);
        });
    }

    fn check_crate_post(&mut self, cx: &LateContext<'tcx>) {
        if !self.is_configured() {
            return;
        }
        self.scope_to_crate(cx.tcx);

        for (source, deps) in &self.allow {
            // A workspace-wide config lists every crate's modules; only flag
            // dead edges when both endpoint modules were observed in the
            // crate just linted.
            if !self.seen_modules.contains(source) {
                continue;
            }
            for target in deps {
                if !self.seen_modules.contains(target) {
                    continue;
                }
                if !self.used_edges.contains(&(*source, *target)) {
                    span_lint_and_help(
                        cx,
                        MODULE_DEPENDENCIES_DEAD_EDGE,
                        rustc_span::DUMMY_SP,
                        format!(
                            "allowlist declares `{source}` \u{2192} `{target}`, \
                             but no such dependency exists in code"
                        ),
                        None,
                        format!(
                            "remove \"{target}\" from the `{source}` allowlist \
                             in dylint.toml, or this edge is stale"
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    const TOML: &str = "\
[module_dependencies]\n\
exhaustive = false\n\
\n\
[module_dependencies.allow]\n\
types = []\n\
errors = [\"types\"]\n\
utils = [\"types\", \"errors\"]\n\
payments = [\"types\", \"errors\", \"utils\"]\n\
server = [\"types\", \"errors\", \"utils\"]\n\
";

    #[test]
    fn ui_module_dependencies() {
        crate::testing::run_ui_test("module_dependencies", Some(TOML), &[]);
    }
}
