// Runs the Bureau canvas as a plain local web page, with no Copilot app.
//
// The canvas is an ordinary loopback HTTP server that renders `/state`; only
// the launcher and the agent-callable actions belong to the app. This entry
// point starts the same server directly, so the view is usable from a browser,
// over SSH, or in CI.
//
// Usage:
//   node .github/extensions/bureau-canvas/serve.mjs [--dir .bureau] [--pipeline <name>] [--open]

process.env.BUREAU_CANVAS_NO_SDK = "1";

const { openBureauCanvas, closeBureauCanvas } = await import("./extension.mjs");

const INSTANCE = "bureau-standalone";

function parseArgs(argv) {
  const input = {};
  let open = false;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--open") {
      open = true;
    } else if (arg === "--dir" || arg === "--pipeline") {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        throw new Error(`${arg} needs a value`);
      }
      input[arg === "--dir" ? "dir" : "pipeline"] = value;
      index += 1;
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return { input, open };
}

async function launchBrowser(url) {
  const { spawn } = await import("node:child_process");
  const command = process.platform === "win32" ? ["cmd", ["/c", "start", "", url]] : ["xdg-open", [url]];
  spawn(command[0], command[1], { detached: true, stdio: "ignore" }).unref();
}

const { input, open } = parseArgs(process.argv.slice(2));
const opened = await openBureauCanvas({ instanceId: INSTANCE, input });

process.stdout.write(`Bureau canvas: ${opened.url}\n`);
process.stdout.write(`Status: ${opened.status}\n`);
process.stdout.write("Press Ctrl+C to stop.\n");

if (open) {
  await launchBrowser(opened.url);
}

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    void closeBureauCanvas({ instanceId: INSTANCE }).finally(() => process.exit(0));
  });
}
