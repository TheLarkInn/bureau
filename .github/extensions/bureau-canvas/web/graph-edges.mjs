// What a React Flow surface can be expected to draw, for the surfaces here to
// publish about themselves.
//
// Separate from `graph-measure.mjs` because that module is a React component
// and this is a pure count: the offline suite holds the rule without a browser
// and without a renderer.

/**
 * How many of these edges React Flow can be expected to draw.
 *
 * Published by each graph as `data-graph-edges`, where the state matrix's
 * settle rule reads it: a render is finished only once every graph on it has
 * drawn the edges it declared. That is what stops the gallery capturing a graph
 * of disconnected boxes — React Flow lays edges out in a pass after it has
 * measured the nodes, and the lull in between is long enough to look settled.
 *
 * Drawable rather than declared, because React Flow draws nothing at all for an
 * edge whose source or target is not on the graph. A config can hold such a
 * reference honestly, so counting it would leave the barrier waiting for a line
 * that was never coming — turning "has this graph finished drawing" into a
 * question with no answer for exactly the configs most worth looking at.
 *
 * **Call this on the surface's own model, never on the arrays it hands React
 * Flow.** While the count was only a timing barrier, "the count handed to the
 * renderer" was the right source: the question was whether the draw pass had
 * happened, and both sides of it moved together by construction. `undrawn-graph`
 * changed what the number is for. It fails a render whose graph never drew the
 * edges it declared — a claim about the *screen* — and a projection that dropped
 * every edge answered that claim by lowering the bar: it declared 0, drew 0, and
 * satisfied both `graphsDrawn` and `undrawnGraphs`. Counting the config instead
 * makes the two numbers independent, so the same failure that was written to
 * catch a graph mid-draw now catches a graph projected wrong. `test/graph-edges.
 * test.mjs` holds the independence at each of the three surfaces by reading how
 * they compute the attribute, because it is a property of the call site and
 * nothing inside this function can enforce it.
 */
export function drawableEdges(nodes, edges) {
  const present = new Set(nodes.map((node) => node.id));
  return edges.filter((edge) => present.has(edge.source) && present.has(edge.target)).length;
}
