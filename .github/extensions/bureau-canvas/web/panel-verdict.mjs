// What the pipeline panel says when *this pipeline* has no findings — and it is
// three different statements, not two.
//
// "Clean" is a *result*, and a result requires a run. In the `fixture` state
// there is no bureau binary, `bureau validate` was never invoked, and the
// findings list is empty because nothing ever filled it. The panel claimed the
// pass anyway, on the same screen whose header reads "Showing bundled sample;
// bureau binary not available." — two statements a reader cannot reconcile,
// and the panel's is the one taken as the verdict, because it is the section
// headed "Validation".
//
// The third statement is the one that reading `state` alone cannot make.
// `validation.state` is `"validated"` whenever the CLI ran *and returned JSON*
// — accepted or rejected alike (`lib/findings.mjs` `validatedState` sets it
// unconditionally and records the verdict separately in `ok`). Meanwhile the
// panel's list is scoped to the pipeline on screen, and a validate finding
// routinely names something else: a role, an assignment, `repos.yaml`, or a
// different pipeline. So a rejected config whose errors all sit elsewhere
// leaves this pipeline with an empty list, and a verdict read off `state`
// announced "clean — bureau validate would pass" while the header on the same
// screen read "Validation findings". That is the identical defect one branch
// over: a mark standing in for a check nobody made, and this time asserting the
// opposite of the command's actual answer.
//
// An empty findings list therefore means one of three things, and only
// `state` *and* `ok` together tell them apart:
//
//   not validated      nothing ran, so there is nothing to report
//   validated  &&  ok  the command ran and accepted the config
//   validated && !ok   the command ran and rejected it, for reasons that are
//                      real and are simply not on this pipeline
//
// The third sentence says "no findings for this pipeline" rather than anything
// resembling a pass, because the pipeline being unimplicated is a fact about
// the pipeline and the config being rejected is a fact about the config, and a
// reader who is about to run `bureau validate` needs the second one.
//
// This lives in `web/` beside `step-refs.mjs` and for its reason: `web/` is the
// only tree the browser can reach, and a rule written here can be imported by
// the page, by the state registry that asserts the page, and by the offline
// suite. `app.mjs` cannot be imported without a browser — it takes React and
// `@xyflow/react` through bare specifiers — so a verdict spelled inside it is a
// rule the offline suite can only read. Pure, with no imports of its own.

/** Nothing ran, so the empty list is an absence of evidence. */
export const PANEL_UNCHECKED = "not checked — bureau validate did not run";

/** The command ran and accepted the config: evidence of absence. */
export const PANEL_CLEAN = "clean — bureau validate would pass";

/** The command ran and rejected it, naming nothing on this pipeline. */
export const PANEL_ELSEWHERE = "no findings for this pipeline — bureau validate rejected the config elsewhere";

/**
 * The sentence an empty pipeline findings list carries.
 *
 * Takes the validation record rather than the whole app state so the rule can
 * be asked its question directly, without standing a page up around it.
 */
export function emptyVerdict(validation) {
  if (validation?.state !== "validated") {
    return PANEL_UNCHECKED;
  }
  return validation.ok ? PANEL_CLEAN : PANEL_ELSEWHERE;
}
