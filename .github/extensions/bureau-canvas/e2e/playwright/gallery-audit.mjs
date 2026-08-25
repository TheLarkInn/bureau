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
 * Every render name a declared twin mentions.
 *
 * Used to decide which renders are worth filing a full signature for: only a
 * pair the registry has made a claim about is ever asked what it differs in.
 */
export function twinParticipants(twins) {
  return new Set(twins.flatMap((twin) => keysFor(twin).flat()));
}

/**
 * States whose renders are the same screen on purpose, and why.
 *
 * A twin is a claim in both directions. If the two stop matching, the claim was
 * wrong or the screen regressed, and either way it needs re-reading — so a
 * declared twin that no longer holds is reported exactly like an undeclared one
 * that does. A twin whose renders are *absent* is reported too, and separately:
 * a partial run would otherwise return clean for twins it never compared, which
 * is the audit quietly saying "checked" about work it did not do.
 *
 * `records` is one entry per render — `{ signature, settled, detail }` — rather
 * than a bare digest map, because a digest can only ever answer "do these two
 * draw the same screen". The two things this audit could not previously say are
 * both in the rest of the record: whether a render was proved to have stopped
 * changing, and, when a claim really has broken, what it broke in.
 *
 * A pair that differs is evidence of a difference only when both sides were
 * proved settled; otherwise one of them is a frame a contended worker happened
 * to be drawing, and calling that a broken claim points a reviewer at a screen
 * that is not the product. A record without a `settled` field reads as proved,
 * so an artefact from a run that filed no settle-proof reads exactly as it did
 * before: absence of a record is not evidence of doubt.
 */
export function auditTwins(records, twins) {
  const byName = new Map(Object.entries(records));
  const declared = new Set(twins.flatMap((twin) => keysFor(twin).map(pairKey)));
  const groups = new Map();
  for (const [name, record] of byName) {
    // Grouped per viewport, because that is as far as a geometry-free
    // signature may be compared. The two layouts draw the same DOM for most
    // states on purpose — `Loading…` at 760px and at 1280px is one screen
    // described twice, not a coincidence worth reporting — so a cross-viewport
    // match says nothing, and five of the first run's findings were exactly
    // that.
    const key = `${viewportOf(name)}::${record.signature}`;
    groups.set(key, [...(groups.get(key) ?? []), name]);
  }
  return [...undeclared(groups, declared), ...broken(twins, byName)];
}

/** Every render this run could not prove had stopped changing. */
export function auditSettled(records) {
  return Object.keys(records).filter((name) => records[name]?.settled === false).sort();
}

function proved(record) {
  return record?.settled !== false;
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

/**
 * One finding per identical-signature group rather than per pair.
 *
 * Per-pair is quadratic in the group, and the run where that matters is the run
 * where the report matters most: a catastrophe in which every state draws one
 * "Loading…" screen produces C(250,2) = 31,125 findings per viewport, and the
 * banner carrying them into `index.html` is megabytes of unreadable text. A
 * group states the same fact once and stays legible.
 */
function undeclared(groups, declared) {
  const findings = [];
  for (const group of groups.values()) {
    const names = group.sort();
    const undeclaredPairs = pairsOf(names).filter((pair) => !declared.has(pairKey(pair)));
    if (!undeclaredPairs.length) {
      continue;
    }
    findings.push({
      kind: "undeclared-twin",
      detail: names.length > 2
        ? `${names.length} renders draw the same screen (${names.slice(0, 4).join(", ")}${names.length > 4 ? ", …" : ""}); declare the twins and say why, or make the states differ`
        : `${names[0]} and ${names[1]} draw the same screen; declare the twin and say why, or make the states differ`,
    });
  }
  return findings;
}

function broken(twins, byName) {
  const findings = [];
  for (const twin of twins) {
    for (const [one, other] of keysFor(twin)) {
      const held = byName.has(one) && byName.has(other);
      if (!held) {
        findings.push({
          kind: "unchecked-twin",
          detail: `${one} and ${other} are declared to draw the same screen (${twin.why}) and this run rendered neither or only one, so the claim was not tested`,
        });
      } else if (byName.get(one).signature !== byName.get(other).signature) {
        findings.push(parted(twin, one, other, byName));
      }
    }
  }
  return findings;
}

/**
 * What a mismatched pair means, which depends on whether both renders settled.
 *
 * Proved on both sides, the two screens really differ and the declaration is
 * wrong or the UI regressed — and the finding then carries the first line the
 * two signatures disagree on, because a reviewer sent to compare two
 * screenshots and a hash has been told there is a difference and given no way
 * to find it.
 *
 * With either side unproved, the mismatch is a frame rather than a screen, and
 * saying "no longer draw the same screen" would send a reviewer looking for a
 * difference that is not in the product.
 */
function parted(twin, one, other, byName) {
  const unproved = [one, other].filter((name) => !proved(byName.get(name)));
  if (unproved.length) {
    return {
      kind: "unproven-twin",
      detail: `${one} and ${other} are declared to draw the same screen (${twin.why}) and differ, but ${unproved.join(" and ")} never stopped changing inside the settle budget, so the difference is a frame rather than a finding`,
    };
  }
  return {
    kind: "broken-twin",
    detail: `${one} and ${other} are declared to draw the same screen (${twin.why}) and no longer do${difference(byName.get(one).detail, byName.get(other).detail)}`,
  };
}

/**
 * The first line two signatures disagree on, as a sentence.
 *
 * Says nothing when the run filed no detail for either render — only twin
 * participants carry one — rather than inventing a difference it cannot see.
 */
export function difference(one, other) {
  if (typeof one !== "string" || typeof other !== "string") {
    return "";
  }
  const left = one.split("\n");
  const right = other.split("\n");
  const at = left.findIndex((line, index) => line !== right[index]);
  if (at === -1) {
    return left.length === right.length ? "" : `; they differ in length alone (${left.length} vs ${right.length} elements)`;
  }
  return `; first difference at element ${at + 1} of ${Math.max(left.length, right.length)}: ${JSON.stringify(left[at] ?? null)} vs ${JSON.stringify(right[at] ?? null)}`;
}

function pairsOf(group) {
  return group.flatMap((one, index) => group.slice(index + 1).map((other) => [one, other]));
}
