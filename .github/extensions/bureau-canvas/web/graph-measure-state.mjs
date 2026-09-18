export function hasNodeMeasurement(node) {
  return Boolean(node && (node.hidden || (node.internals?.handleBounds
    && Number.isFinite(node.measured?.width) && node.measured.width > 0
    && Number.isFinite(node.measured?.height) && node.measured.height > 0)));
}

// A missing target is not permission to frame the measured subset.
export function measuredGraphNodes(ids, getNode) {
  const nodes = ids.map((id) => getNode(id));
  return nodes.every(hasNodeMeasurement) ? nodes.filter((node) => !node.hidden) : null;
}
