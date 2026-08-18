// A `bureau` that predates `validate --json`, as an older install on PATH
// would be. Mirrors clap's message so the detection is tested against the real
// wording rather than an invented one.

const [, , command, , jsonFlag] = process.argv;

if (command === "validate" && jsonFlag === "--json") {
  process.stderr.write("error: unexpected argument '--json' found\n\nUsage: bureau validate <DIR>\n");
  process.exit(2);
}

process.stdout.write("config ok: 0 repos, 0 roles, 0 assignments\n");
process.exit(0);
