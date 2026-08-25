// Runs the Bureau canvas as a plain local web page, with no Copilot app.
//
// The canvas is an ordinary loopback HTTP server that renders `/state`; only
// the launcher and the agent-callable actions belong to the app. This entry
// point starts the same server directly, so the view is usable from a browser,
// over SSH, or in CI.
//
// Usage:
//   node .github/extensions/bureau-canvas/serve.mjs [--dir .bureau] [--pipeline <name>] [--dev] [--port <port>] [--open]

process.env.BUREAU_CANVAS_NO_SDK = "1";

import { resolve } from "node:path";

const { openBureauCanvas, closeBureauCanvas } = await import("./extension.mjs");

const INSTANCE = "bureau-standalone";

function parseArgs(argv) {
  const input = {};
  const options = {};
  let open = false;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--open") {
      open = true;
    } else if (arg === "--dev") {
      options.dev = true;
    } else if (arg === "--port") {
      options.port = parsePort(argv[index + 1]);
      index += 1;
    } else if (arg === "--dir" || arg === "--pipeline" || arg === "--bureau") {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        throw new Error(`${arg} needs a value`);
      }
      if (arg === "--bureau") {
        options.binary = resolve(value);
      } else {
        input[arg === "--dir" ? "dir" : "pipeline"] = arg === "--dir" ? resolve(value) : value;
      }
      index += 1;
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return { input, open, options };
}

function parsePort(value) {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
      throw new Error("--port needs an integer from 1 through 65535");
  }
  return port;
}

async function launchBrowser(url) {
  const { spawn } = await import("node:child_process");
  const command = process.platform === "win32"
      ? ["cmd", ["/c", "start", "", url]]
      : process.platform === "darwin" ? ["open", [url]] : ["xdg-open", [url]];
  const child = spawn(command[0], command[1], { detached: true, stdio: "ignore" });
  child.once("error", (error) => {
      process.stderr.write(`Could not open a browser: ${error.message}\n`);
  });
  child.unref();
}

const { input, open, options } = parseArgs(process.argv.slice(2));
const opened = await openBureauCanvas({ instanceId: INSTANCE, input }, options);

process.stdout.write(`Bureau dashboard: ${opened.url}\n`);
process.stdout.write(`Status: ${opened.status}\n`);
process.stdout.write(`Development reload: ${options.dev ? "on" : "off"}\n`);
process.stdout.write("Press Ctrl+C to stop.\n");

if (open) {
  await launchBrowser(opened.url);
}

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    void closeBureauCanvas({ instanceId: INSTANCE }).finally(() => process.exit(0));
  });
}
