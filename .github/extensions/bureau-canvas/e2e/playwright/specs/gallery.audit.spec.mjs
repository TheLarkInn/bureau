// The gallery's own completeness, asserted where a failure counts.
//
// This runs in the `gallery` teardown project, so it starts only once every
// worker in the matrix project has finished — the same vantage point
// `globalTeardown` has, and the only one from which the whole published
// directory can be seen, because a render is written by whichever worker drew
// it and no test can see another worker's files.
//
// It is a spec rather than a hook because the audit should be a *named check*.
// For as long as it lived in `globalTeardown` it produced findings that nothing
// was answerable for: a run could print `This gallery is not the whole matrix`
// in red, at length, and the only things carrying that news were a console line
// and a banner nobody is gated on. Throwing from the hook does fail the run —
// measured, not assumed — but it fails it as an error belonging to no test, so
// the reporter says "1 error was not a part of any test" and the run's own
// record of what it checked does not contain the check. An audit that cannot be
// found in the list of checks is halfway to the defect this branch removes.
//
// Both halves are asserted. `auditGallery` splits its findings by what they are
// computed from: arithmetic over a file list on one side — renders never
// written, renders belonging to no state, records that will not parse, declared
// twins the run never rendered both sides of — and comparisons between two
// renders on the other. Both gate, in two assertions rather than one, so a run
// says which kind of thing it got wrong.
//
// The comparisons gate because the drift they were once excused for cannot
// reach them: a claim requires both renders to have *proved* they stopped
// changing, and every other pair is routed to `unproven-*` and reported as
// drift. `global-teardown.mjs` carries the measurement — two full runs over one
// tree agree on 511 of 512 signatures, and the one that moves is the single
// state the registry declares in motion, which is unproved by construction.

import { expect, test } from "@playwright/test";

import { runAudit } from "../global-teardown.mjs";

/**
 * Tagged `@matrix` for the reporter's sake, not for filtering: Playwright does
 * not apply `--grep` to a teardown project, so this runs on `test:pr` and
 * `test:visual` too, where nothing was rendered and there is no gallery to
 * audit. It says so and skips, rather than failing a suite it has no business
 * judging — those runs leave a reviewer's gallery exactly as they found it,
 * which is `gallery.mjs`'s rule and not this one's.
 *
 * The skip is on `ran`, and `ran` is the vacuity guard rather than a
 * convenience. Without it the assertion below would be green for a run that
 * published nothing and therefore found nothing, which is the shape of pass
 * this whole suite is written against — so the two are kept apart: no gallery
 * is a *skip*, a gallery with holes in it is a *failure*, and neither can be
 * mistaken for the other in a run's record.
 */
test("@matrix the published gallery is the whole matrix", async () => {
  const audit = await runAudit();

  test.skip(!audit.ran, audit.reason ?? "no gallery was published by this run");
  expect(audit.incomplete, "renders the gallery claims and does not hold").toEqual([]);
  expect(audit.claims, "states that draw one screen without declaring they do, and declared twins that parted").toEqual([]);
});
