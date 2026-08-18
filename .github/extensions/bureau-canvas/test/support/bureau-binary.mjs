// Some tests validate scaffolds by running the real `bureau validate --json`,
// which is far stronger than asserting field shapes — it is how the flow-style
// and unreachable-step bugs were found.
//
// That needs a built binary. CI builds one before the suite runs; a developer
// iterating on JavaScript alone may not have. Rather than fail for an
// environmental reason, those tests skip with a reason that says what to do.

import { access } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const REPO_ROOT = new URL("../../../../../", import.meta.url);

export async function bureauBinary() {
  const explicit = process.env.BUREAU_CANVAS_BUREAU;
  const candidates = [
    ...(explicit ? [explicit] : []),
    fileURLToPath(new URL("target/debug/bureau", REPO_ROOT)),
    fileURLToPath(new URL("target/debug/bureau.exe", REPO_ROOT)),
  ];
  for (const candidate of candidates) {
    if (await exists(candidate)) {
      return candidate;
    }
  }
  return null;
}

/** `false` when the tests can run, otherwise the reason they cannot. */
export async function skipWithoutBureau() {
  return (await bureauBinary()) ? false : "needs a built bureau: cargo build --bin bureau";
}

async function exists(path) {
  return access(path).then(() => true, () => false);
}
