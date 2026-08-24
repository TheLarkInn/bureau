/** Chooses the next open assignment without closing an editor by surprise. */
export function nextExpandedAssignment(current, requested, canClose) {
  if (current && !canClose()) {
    return current;
  }
  return current === requested ? null : requested;
}

/**
 * The field editors on the config surface that can hold unsaved work.
 *
 * The repo adder is a `.repos-editor` too, so the list covers it without
 * naming it twice.
 */
export const FIELD_EDITORS = [
  ".ws-open",
  ".repos-editor",
  ".limits-editor",
  ".assignment-runtime-editor",
  ".terminal-label-editor",
];

/**
 * What "there is unsaved work on this card" resolves to, as a selector.
 *
 * The predicate matters as much as the list. This used to be the bare editor
 * list, which meant the two controls that navigate away from a card asked
 * whether an editor was *open* and then warned about discarding changes — so
 * opening the repos editor to read it and clicking the pipeline beside it
 * demanded you dismiss a prompt about work you had not done. A prompt that
 * cries wolf is worse than no prompt at all, because the one that matters gets
 * the same reflexive dismissal as the four that did not.
 *
 * Each editor publishes `data-dirty` from the same value it uses to decide
 * whether to offer its own save, so the guard and the save agree by
 * construction.
 *
 * The predicate is applied here by `map` rather than written out per editor,
 * so an editor cannot be added to the list without it — and what
 * `test/assignment-state.test.mjs` holds in place is exactly that
 * construction, not the behaviour. Rewriting this back into a bare
 * `FIELD_EDITORS.join(", ")` fails there. The behavioural half — that each
 * editor really does publish `data-dirty` from its own save gate — is not
 * visible to a DOM-free test at all, and is held by the browser suite's
 * "every field editor reports its own draft the same way".
 */
export const DIRTY_FIELD_EDITORS = FIELD_EDITORS
  .map((editor) => `${editor}[data-dirty="true"]`)
  .join(", ");
