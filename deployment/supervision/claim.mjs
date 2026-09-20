import { requestAdmission } from "./admission.mjs";

try {
  await requestAdmission("/run/bureau-windows-supervision/admission.sock",
    { invocation: process.env.INVOCATION_ID, pid: process.pid });
} catch {
  console.error("Windows supervision startup grant refused");
  process.exitCode = 1;
}
