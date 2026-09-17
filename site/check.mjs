import { parseArgs } from "node:util";
import { audit, auditKinds, incompleteReport, reportStatus } from "./audit.mjs";
import { isMain } from "./paths.mjs";

export function checkOptions(args) {
  const { values } = parseArgs({ args, options: {
    kind: { type: "string" }, json: { type: "boolean", default: false },
  } });
  if (!auditKinds.includes(values.kind)) throw new Error("Use --kind accessibility or --kind responsive.");
  return values;
}

export function encodeReport(report) {
  const json = JSON.stringify(report);
  if (Buffer.byteLength(json) > 128 * 1024) {
    return JSON.stringify(incompleteReport(report.kind, "Audit output exceeded 128 KiB.", report.checks));
  }
  return json;
}

if (isMain(import.meta.url)) {
  let report;
  let kind = "unknown";
  try {
    const options = checkOptions(process.argv.slice(2));
    kind = options.kind;
    report = await audit(options.kind);
  } catch (error) {
    report = incompleteReport(kind, error.message);
  }
  const json = encodeReport(report);
  console.log(json);
  process.exitCode = reportStatus(JSON.parse(json));
}
