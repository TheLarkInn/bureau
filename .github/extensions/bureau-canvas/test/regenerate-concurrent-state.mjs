// Rebuilds `web/statelab/concurrent-state.mjs` from the committed validate
// payload beside it.
//
// The state lab needs one pipeline whose middle step is a concurrent group, so
// the run viewer's group card — its member rows and the control that folds them
// away — has something to draw on. The bundled sample has no concurrent step,
// and a group card's geometry belongs to `lib/layout.mjs`, which the two-host
// bundle boundary keeps out of the browser. So the payload is built once, here,
// by the host itself, and committed.
//
// `test/statelab.test.mjs` runs the same build and fails if the committed file
// differs, which is what stops it drifting from what the host would serve.
//
//   node test/regenerate-concurrent-state.mjs

import { writeFile } from "node:fs/promises";
import { buildConcurrentState, MODULE_URL, PROJECTED_FIELDS } from "./support/concurrent-state.mjs";

const header = `// The served state for a pipeline whose middle step is a concurrent group.
//
// Generated, not written. \`test/fixtures/concurrent-payload.json\` is a
// \`bureau validate --json\` answer; this is what \`extension.mjs\` builds from
// it — the same layout projector, the same handles, the same relation graph the
// host would serve. \`test/statelab.test.mjs\` rebuilds it and fails on any
// difference, so it cannot drift from the host.
//
// The ${PROJECTED_FIELDS.length} host-owned fields (${PROJECTED_FIELDS.map((field) => `\`${field}\``).join(", ")}) are absent
// on purpose: the fixture takes those from the payload it projects over, so the
// same committed shape renders on every machine.
//
// To regenerate: node test/regenerate-concurrent-state.mjs

export const CONCURRENT_STATE = `;

const state = await buildConcurrentState();
await writeFile(MODULE_URL, `${header}${JSON.stringify(state, null, 2)};\n`);
process.stdout.write(`rebuilt ${MODULE_URL.pathname}\n`);
