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
  return [...undeclared(groups, declared, byName), ...broken(twins, byName)];
}

/**
 * Whether a finding describes this harness's own drift rather than the product.
 *
 * The two kinds of news this audit produces read the same in a list and mean
 * opposite things: one asks a reviewer to go and look at the UI, the other says
 * there is nothing in the UI to look at. Splitting them by name here — rather
 * than at each of the places that reports — is what lets `global-teardown.mjs`
 * keep the alarm for claims about the product and put drift in the note beside
 * the unsettled count, where it belongs and where it cannot cry wolf.
 */
export function isDrift(finding) {
  return finding.kind.startsWith("unproven-");
}

/**
 * Which of this audit's findings may gate a run, and which may only be reported.
 *
 * The distinction is not severity, it is what the finding is computed *from*.
 * `unchecked` — a declared twin whose renders this run did not both produce —
 * is arithmetic over a file list, exactly like a missing render: it says the run
 * did not do the work, it cannot come out differently on a contended machine,
 * and a run that reports it has published an artefact that lies about its own
 * extent. `claims` compares two renders against each other, and that comparison
 * still drifts — some content arrives after a surface has held still for a poll
 * interval — so gating on it would fail runs at random, which this repository
 * treats as worse than not gating at all. `drift` is the subset that has already
 * said in its own words that the difference is a frame.
 *
 * Pure and total: every finding lands in exactly one of the three, so a kind
 * added later cannot fall out of all of them and quietly stop being reported.
 */
export function partitionFindings(findings) {
  return {
    unchecked: findings.filter((finding) => finding.kind === "unchecked-twin"),
    claims: findings.filter((finding) => !isDrift(finding) && finding.kind !== "unchecked-twin"),
    drift: findings.filter(isDrift),
  };
}

/** Every render this run could not prove had stopped changing. */
export function auditSettled(records) {
  return Object.keys(records).filter((name) => records[name]?.settled === false).sort();
}

/**
 * The unsettled renders no state asked to be unsettled, and the declared ones
 * that came to rest anyway. Both are findings; neither was one before.
 *
 * `auditSettled` counts motion, and a count is not a correspondence: two
 * unsettled renders reported as a note read exactly the same whether they are
 * the two `transport:playing` figures — which advance on a 100ms interval and
 * are supposed to move — or two ordinary screens that have quietly become
 * nondeterministic. The note said "2" in both worlds, so the gallery could
 * publish a screenshot that differed run to run and still describe itself as
 * complete.
 *
 * So the registry is asked which renders are entitled to move. `stray` is a
 * render that moved without saying it would; `still` is one that said it would
 * and did not, which is how a declaration goes stale — Play stops advancing the
 * run and the exemption silently absorbs the regression. `state-matrix.spec.mjs`
 * fails per render on both, and this is the same claim made over the published
 * artefact, where a partial or reordered run cannot hide it.
 */
export function auditMotion(records, moving) {
  const declared = new Set(moving);
  const names = Object.keys(records).filter((name) => typeof records[name]?.settled === "boolean");
  return {
    stray: names.filter((name) => records[name].settled === false && !declared.has(name)).sort(),
    still: names.filter((name) => records[name].settled === true && declared.has(name)).sort(),
  };
}

/** The renders whose state declares itself in motion, as published names. */
export function movingShots(states, viewports) {
  return states
    .filter((state) => state.expect?.settles === false)
    .flatMap((state) => viewports.map((viewport) => shotName(state.id, viewport.id)));
}

/**
 * Renders the gallery published and knows nothing about.
 *
 * A render files its record *after* its screenshot, so a worker killed between
 * the two leaves a PNG with no record at all — the same accident `unreadable`
 * already exists for, one instruction earlier. `unreadable` only sees a record
 * that exists and will not parse; a record that was never written is invisible
 * to it, and a render absent from `records` is absent from every audit that
 * reads them: it is not in `auditSettled`, so nothing marks its figure, and it
 * is not in the twin groups, so nothing compares it.
 *
 * The reviewer therefore gets a figure indexed and captioned exactly like a
 * proved one, for a render this run cannot say anything about — which is the
 * one claim this gallery is not allowed to make. Named here so it can be
 * reported and marked like any other render that may not be believed.
 *
 * Only over renders the registry expects and this run published: a PNG that is
 * missing is `auditNames().missing`, and one belonging to no state is
 * `.stray`. `unreadable` is excluded too — a record that exists and will not
 * parse is already reported, in its own words, and a render answered twice with
 * "could not read its record" and "filed no record at all" is a partition that
 * contradicts itself. Each render lands in exactly one of the four.
 */
export function auditUnaudited(expected, published, records, unreadable = []) {
  const held = new Set(published);
  const known = new Set([...Object.keys(records), ...unreadable]);
  return expected.filter((name) => held.has(name) && !known.has(name)).sort();
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
 *
 * Settle-proof is read here as well as in `parted`, and the reason is symmetry:
 * a pair that *matches* is evidence of sameness only when both sides were
 * proved, exactly as a pair that differs is evidence of a difference only then.
 * A render captured a beat early is missing whatever had not arrived yet, and
 * two states that are each missing the same late region collide on a signature
 * neither of them will still have a moment later. Left alone, that path
 * published the harness's own contention as "declare the twins and say why" —
 * the same false finding this audit stopped making in the other direction.
 *
 * Read per pair rather than per group, because a group is not all one claim.
 * One unsettled render joining a group would otherwise take the whole group's
 * news with it: with `a` unsettled and `b` and `c` both proved and both
 * undeclared, the real finding about `b` and `c` is one this harness owes a
 * reviewer, and answering it with "a never stopped changing" drops a defect on
 * the floor. Each half is still reported once, so the quadratic group stays a
 * single line.
 */
function undeclared(groups, declared, byName) {
  const findings = [];
  for (const group of groups.values()) {
    const undeclaredPairs = pairsOf(group.sort()).filter((pair) => !declared.has(pairKey(pair)));
    const held = undeclaredPairs.filter((pair) => pair.every((name) => proved(byName.get(name))));
    const framed = undeclaredPairs.filter((pair) => !pair.every((name) => proved(byName.get(name))));
    if (held.length) {
      findings.push(matched(namesIn(held)));
    }
    if (framed.length) {
      findings.push(framedMatch(namesIn(framed), namesIn(framed).filter((name) => !proved(byName.get(name)))));
    }
  }
  return findings;
}

/** Every render a set of pairs names, once each, in a stable order. */
function namesIn(pairs) {
  return [...new Set(pairs.flat())].sort();
}

function matched(names) {
  return {
    kind: "undeclared-twin",
    detail: names.length > 2
      ? `${names.length} renders draw the same screen (${names.slice(0, 4).join(", ")}${names.length > 4 ? ", …" : ""}); declare the twins and say why, or make the states differ`
      : `${names[0]} and ${names[1]} draw the same screen; declare the twin and say why, or make the states differ`,
  };
}

function framedMatch(names, unproved) {
  return {
    kind: "unproven-match",
    detail: `${names.join(" and ")} drew the same screen, but ${unproved.join(" and ")} never stopped changing inside the settle budget, so the match is a frame rather than a finding`,
  };
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
