/** Chooses the next open assignment without closing an editor by surprise. */
export function nextExpandedAssignment(current, requested, canClose) {
  if (current && !canClose()) {
    return current;
  }
  return current === requested ? null : requested;
}
