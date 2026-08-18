const [, , command, dir, jsonFlag] = process.argv;

if (command !== "validate" || jsonFlag !== "--json") {
  process.stderr.write("expected: validate <dir> --json\n");
  process.exit(64);
}

if (dir.includes("findings-crash")) {
  process.stderr.write("simulated validate crash\n");
  process.exit(101);
}

const payload = dir.includes("findings-invalid") ? invalidPayload(dir) : validPayload(dir);
process.stdout.write(`${JSON.stringify(payload)}\n`);
process.exit(payload.ok ? 0 : 1);

function validPayload(dir) {
  return {
    ok: true,
    dir,
    errors: [],
    config: {
      repos: {},
      roles: {},
      assignments: {},
      pipelines: {},
    },
  };
}

function invalidPayload(dir) {
  return {
    ok: false,
    dir,
    errors: [
      {
        path: "pipelines/bad-edge.yaml",
        message: "pipeline `bad-edge` step `verify`: unknown next target `missing`",
      },
      {
        path: "pipelines/bad-edge.yaml",
        message: "pipeline `bad-edge`: unreachable step `review`",
      },
      {
        path: "notes.txt",
        message: "notes are not part of config",
      },
    ],
    config: null,
  };
}
