// Whether a published gallery is the whole matrix, and whether any two states
// in it draw the same screen.
//
// Both questions are asked here because both are claims the gallery makes and
// neither could previously be contradicted:
//
//   completeness — the index links a figure for every state in the registry
//                  whether or not the render behind it was ever written, so a
//                  run that rendered 300 of 500 states published an artefact
//                  that reads as the complete gallery with 200 broken images.
//                  A reviewer scrolling it sees states they believe they have
//                  reviewed.
//
//   distinctness — two states that draw the identical screen are either a
//                  finding worth stating (a control that stopped varying, an
//                  entry operation that has become a no-op) or a fact worth
//                  declaring (dismissing a refusal really does return you to
//                  the screen you started on). Undeclared, they are neither:
//                  the registry says the states differ and the renders say
//                  they do not, and nothing reconciles the two.
//
// Distinctness is judged on the DOM signature rather than on the screenshot.
// Two runs of one state produce different bytes — a handful of antialiased
// glyphs — so pixels cannot tell "these two states draw the same screen" from
// "these two files were written by different runs". `checks.mjs` gathers the
// signature in the same pass as everything else it measures.

/** The shot name for a state at a viewport. One namer, so nothing drifts. */
export function shotName(stateId, viewportId) {
  return `${viewportId}--${stateId.replace(/[^a-z0-9]+/giu, "_")}.png`;
}

/** Every render the registry implies, in a stable order. */
export function expectedShots(states, viewports) {
  return states.flatMap((state) => viewports.map((viewport) => shotName(state.id, viewport.id)));
}

/**
 * What a published file list says about the run that wrote it.
 *
 * `stray` matters as much as `missing`: a render left behind by a registry that
 * no longer holds that state invites a reviewer to sign off on a screen the
 * product does not have.
 */
export function auditNames(expected, present) {
  const held = new Set(present);
  const wanted = new Set(expected);
  return {
    missing: expected.filter((name) => !held.has(name)),
    stray: present.filter((name) => name.endsWith(".png") && !wanted.has(name)),
  };
}

/**
 * States whose renders are the same screen on purpose, and why.
 *
 * A twin is a claim in both directions. If the two stop matching, the claim was
 * wrong or the screen regressed, and either way it needs re-reading — so a
 * declared twin that no longer holds is reported exactly like an undeclared one
 * that does.
 */
export function auditTwins(signatures, twins) {
  const byName = new Map(Object.entries(signatures));
  const declared = new Set(twins.flatMap((twin) => keysFor(twin).map(pairKey)));
  const groups = new Map();
  for (const [name, digest] of byName) {
    // Grouped per viewport, because that is as far as a geometry-free
    // signature may be compared. The two layouts draw the same DOM for most
    // states on purpose — `Loading…` at 760px and at 1280px is one screen
    // described twice, not a coincidence worth reporting — so a cross-viewport
    // match says nothing, and five of the first run's findings were exactly
    // that.
    const key = `${viewportOf(name)}::${digest}`;
    groups.set(key, [...(groups.get(key) ?? []), name]);
  }
  return [...undeclared(groups, declared), ...broken(twins, byName)];
}

function viewportOf(name) {
  return name.slice(0, name.indexOf("--"));
}

/** Every ordered-insensitive pair a twin declares, one per viewport. */
function keysFor(twin) {
  return twin.viewports.map((viewport) => [shotName(twin.a, viewport), shotName(twin.b, viewport)]);
}

function pairKey([one, other]) {
  return [one, other].sort().join(" == ");
}

function undeclared(groups, declared) {
  const findings = [];
  for (const group of groups.values()) {
    for (const pair of pairsOf(group.sort())) {
      if (!declared.has(pairKey(pair))) {
        findings.push({
          kind: "undeclared-twin",
          detail: `${pair[0]} and ${pair[1]} draw the same screen; declare the twin and say why, or make the states differ`,
        });
      }
    }
  }
  return findings;
}

function broken(twins, byName) {
  const findings = [];
  for (const twin of twins) {
    for (const [one, other] of keysFor(twin)) {
      const held = byName.has(one) && byName.has(other) && byName.get(one) === byName.get(other);
      if (!held && byName.has(one) && byName.has(other)) {
        findings.push({
          kind: "broken-twin",
          detail: `${one} and ${other} are declared to draw the same screen (${twin.why}) and no longer do`,
        });
      }
    }
  }
  return findings;
}

function pairsOf(group) {
  return group.flatMap((one, index) => group.slice(index + 1).map((other) => [one, other]));
}
