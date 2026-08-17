#![allow(
    clippy::indexing_slicing,
    reason = "graph algorithm indices are always in-bounds"
)]

use clippy_utils::diagnostics::span_lint_and_then;
use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_hir::def::DefKind;
use rustc_hir::{Expr, HirId, Item};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;
use rustc_span::{Span, Symbol};

use super::hir_refs;

rustc_session::declare_lint! {
    /// Flags cyclic dependencies between sibling modules at any depth.
    ///
    /// An acyclic module graph forces shared concepts into explicit, stable
    /// layers.  This lint builds a sibling dependency graph at every level of
    /// the module hierarchy and reports any cycle it finds.
    ///
    /// No configuration required — uses `#[expect(acyclic_modules)]` for
    /// per-site opt-out.
    pub ACYCLIC_MODULES,
    Deny,
    "cyclic dependency between sibling modules"
}

struct Edge {
    source: Vec<Symbol>,
    target: Vec<Symbol>,
    span: Span,
}

/// Returns the `DefId` of the nearest module ancestor for a local item.
///
/// If `def_id` is itself a module, returns it unchanged.
fn item_module_def_id(tcx: TyCtxt<'_>, def_id: DefId) -> DefId {
    let mut current = def_id;
    while tcx.def_kind(current) != DefKind::Mod {
        current = tcx.parent(current);
    }
    current
}

/// A directed edge between two sibling module names, with the span that caused it.
struct SiblingEdge {
    from: Symbol,
    to: Symbol,
    span: Span,
}

/// Builds sibling dependency graphs for every parent module that has children.
///
/// For each recorded edge, finds the first point where source and target paths
/// diverge.  The common prefix identifies the parent module; the diverging
/// components identify which sibling children are involved.  Parent-child
/// edges (one path is a prefix of the other) are excluded by construction.
fn build_sibling_graphs(edges: &[Edge]) -> FxHashMap<Vec<Symbol>, Vec<SiblingEdge>> {
    let mut graphs: FxHashMap<Vec<Symbol>, Vec<SiblingEdge>> = FxHashMap::default();

    for edge in edges {
        let common_len = edge
            .source
            .iter()
            .zip(edge.target.iter())
            .take_while(|(a, b)| a == b)
            .count();

        // One path is a prefix of the other → parent-child, not sibling.
        if common_len >= edge.source.len() || common_len >= edge.target.len() {
            continue;
        }

        let parent = edge.source[..common_len].to_vec();
        let from = edge.source[common_len];
        let to = edge.target[common_len];

        graphs.entry(parent).or_default().push(SiblingEdge {
            from,
            to,
            span: edge.span,
        });
    }

    graphs
}

/// A cycle is a sequence of module names forming a loop, e.g. `[A, B, C]`
/// means `A → B → C → A`.
type Cycle = Vec<Symbol>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Gray,
    Black,
}

fn dfs(
    node: Symbol,
    adj: &FxHashMap<Symbol, Vec<Symbol>>,
    color: &mut FxHashMap<Symbol, Color>,
    path: &mut Vec<Symbol>,
    cycles: &mut Vec<Cycle>,
) {
    color.insert(node, Color::Gray);
    path.push(node);

    if let Some(neighbors) = adj.get(&node) {
        for &next in neighbors {
            match color.get(&next) {
                None => dfs(next, adj, color, path, cycles),
                Some(Color::Gray) => {
                    // Back edge → extract the cycle from the path.
                    #[expect(
                        clippy::unwrap_used,
                        reason = "Gray node is guaranteed to be on the current DFS path"
                    )]
                    let start = path.iter().position(|&n| n == next).unwrap();
                    cycles.push(path[start..].to_vec());
                }
                Some(Color::Black) => {}
            }
        }
    }

    path.pop();
    color.insert(node, Color::Black);
}

/// Rotates a non-empty cycle so it starts at the lexicographically smallest
/// module name.
fn normalize_cycle(cycle: &[Symbol]) -> Cycle {
    let min_pos = cycle
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.as_str().cmp(b.as_str()))
        .map_or(0, |(i, _)| i);
    let mut result = Vec::with_capacity(cycle.len());
    result.extend_from_slice(&cycle[min_pos..]);
    result.extend_from_slice(&cycle[..min_pos]);
    result
}

/// Runs DFS on a sibling graph to find all cycles.
///
/// Uses Gray (on stack) / Black (finished) coloring with absence meaning
/// unvisited.  Returns cycles normalized and deduplicated.
fn detect_cycles(graph: &[SiblingEdge]) -> Vec<Cycle> {
    let mut adj: FxHashMap<Symbol, Vec<Symbol>> = FxHashMap::default();
    let mut node_set: FxHashSet<Symbol> = FxHashSet::default();
    let mut seen_edges: FxHashSet<(Symbol, Symbol)> = FxHashSet::default();

    for edge in graph {
        node_set.insert(edge.from);
        node_set.insert(edge.to);
        if seen_edges.insert((edge.from, edge.to)) {
            adj.entry(edge.from).or_default().push(edge.to);
        }
    }

    // Sort nodes and adjacency lists for deterministic output.
    let mut nodes: Vec<Symbol> = node_set.into_iter().collect();
    nodes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    for neighbors in adj.values_mut() {
        neighbors.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    }

    let mut cycles = Vec::new();
    let mut color: FxHashMap<Symbol, Color> = FxHashMap::default();

    for &node in &nodes {
        if !color.contains_key(&node) {
            let mut path = Vec::new();
            dfs(node, &adj, &mut color, &mut path, &mut cycles);
        }
    }

    let mut result: Vec<Cycle> = cycles.into_iter().map(|c| normalize_cycle(&c)).collect();
    result.sort_by(|a, b| {
        a.iter()
            .map(Symbol::as_str)
            .cmp(b.iter().map(Symbol::as_str))
    });
    result.dedup();
    result
}

/// Emits a diagnostic for a single cycle found under a parent module.
///
/// Shows the cycle path and the source locations of the edges that form it,
/// following the format specified in the design doc.
fn emit_cycle_diagnostic(
    cx: &LateContext<'_>,
    parent: &[Symbol],
    cycle: &Cycle,
    sibling_edges: &[SiblingEdge],
) {
    let parent_name = if parent.is_empty() {
        "crate".to_owned()
    } else {
        parent
            .iter()
            .map(Symbol::as_str)
            .collect::<Vec<_>>()
            .join("::")
    };

    let cycle_str = cycle
        .iter()
        .chain(core::iter::once(&cycle[0]))
        .map(|s| format!("`{s}`"))
        .collect::<Vec<_>>()
        .join(" \u{2192} ");

    // Build a lookup for the first span witnessing each directed sibling edge.
    let mut span_map: FxHashMap<(Symbol, Symbol), Span> = FxHashMap::default();
    for e in sibling_edges {
        span_map.entry((e.from, e.to)).or_insert(e.span);
    }

    let edge_spans: Vec<(Symbol, Symbol, Span)> = cycle
        .windows(2)
        .map(|w| (w[0], w[1]))
        .chain(core::iter::once((cycle[cycle.len() - 1], cycle[0])))
        .filter_map(|(from, to)| span_map.get(&(from, to)).map(|&span| (from, to, span)))
        .collect();

    let primary_span = edge_spans.first().map_or(rustc_span::DUMMY_SP, |e| e.2);

    span_lint_and_then(
        cx,
        ACYCLIC_MODULES,
        primary_span,
        format!(
            "cyclic dependency between sibling modules under `{parent_name}`:\n\
             \x20      {cycle_str}"
        ),
        |diag| {
            for &(from, to, span) in &edge_spans {
                diag.span_label(span, format!("`{from}` \u{2192} `{to}`"));
            }

            if cycle.len() == 2 {
                diag.help(format!(
                    "break this cycle by moving shared items to a module that both \
                     `{}` and `{}` can depend on, or restructure so the dependency \
                     flows in one direction",
                    cycle[0], cycle[1]
                ));
            } else {
                let names = cycle
                    .iter()
                    .map(|s| format!("`{s}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                diag.help(format!(
                    "break this cycle by extracting shared items into a common \
                     module that {names} can all depend on"
                ));
            }
        },
    );
}

pub struct AcyclicModules {
    edges: Vec<Edge>,
}

impl AcyclicModules {
    pub const fn new() -> Self {
        Self { edges: Vec::new() }
    }

    /// Record an edge if the source and target belong to different module
    /// subtrees.  Skips external crate items, test code, and macro expansions.
    fn record_edge(&mut self, cx: &LateContext<'_>, def_id: DefId, hir_id: HirId, span: Span) {
        if hir_refs::should_skip_ref(cx, def_id, hir_id, span) {
            return;
        }

        let source_mod_def_id = cx.tcx.parent_module(hir_id).to_def_id();
        let target_mod_def_id = item_module_def_id(cx.tcx, def_id);

        // Fast path: skip intra-module references without allocating paths.
        if source_mod_def_id == target_mod_def_id {
            return;
        }

        let source_path = hir_refs::def_path_segments(cx.tcx, source_mod_def_id);
        let target_path = hir_refs::def_path_segments(cx.tcx, target_mod_def_id);

        self.edges.push(Edge {
            source: source_path,
            target: target_path,
            span,
        });
    }
}

rustc_session::impl_lint_pass!(AcyclicModules => [ACYCLIC_MODULES]);

impl<'tcx> LateLintPass<'tcx> for AcyclicModules {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if let Some((def_id, hir_id, span)) = hir_refs::resolve_expr_def_id(cx, expr) {
            self.record_edge(cx, def_id, hir_id, span);
        }
    }

    fn check_ty(
        &mut self,
        cx: &LateContext<'tcx>,
        ty: &'tcx rustc_hir::Ty<'tcx, rustc_hir::AmbigArg>,
    ) {
        if let Some((def_id, hir_id, span)) = hir_refs::resolve_ty_def_id(cx, ty) {
            self.record_edge(cx, def_id, hir_id, span);
        }
    }

    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        hir_refs::for_each_use_def_id(item, |def_id, hir_id, span| {
            self.record_edge(cx, def_id, hir_id, span);
        });
    }

    fn check_crate_post(&mut self, cx: &LateContext<'tcx>) {
        let sibling_graphs = build_sibling_graphs(&self.edges);

        let mut parents: Vec<_> = sibling_graphs.keys().collect();
        parents.sort_by(|a, b| {
            a.iter()
                .map(Symbol::as_str)
                .cmp(b.iter().map(Symbol::as_str))
        });

        for parent in parents {
            let sibling_edges = &sibling_graphs[parent];
            let cycles = detect_cycles(sibling_edges);
            for cycle in &cycles {
                emit_cycle_diagnostic(cx, parent, cycle, sibling_edges);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ui_acyclic_modules() {
        crate::testing::run_ui_test("acyclic_modules", None, &[]);
    }
}
