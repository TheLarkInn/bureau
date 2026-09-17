const INPUT_NAMES = ["Cargo.toml", "Cargo.lock", "package.json", "package-lock.json",
  "npm-shrinkwrap.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb",
  "build.rs", "rust-toolchain", "rust-toolchain.toml", "clippy.toml", ".clippy.toml",
  "rustfmt.toml", ".rustfmt.toml", ".gitattributes", ".gitignore"];

const VERIFIER_ROOTS = ["crates/bureau/tests/maintenance_chaos", "crates/bureau/tests/rate_admission",
  "crates/bureau/tests/runlog_framing", "crates/bureau/tests/migration_cli",
  "crates/bureau/tests/edge/testdir", "crates/bureau/src/cli/run/tests",
  "crates/bureau/src/cli/run/claim/tests", "crates/bureau/src/cli/run/observe/tests",
  "crates/bureau/src/state/accounting/tests", "crates/bureau/src/state/claim/fresh/quota/tests"];

export const VERIFICATION_INPUTS = [
  ...INPUT_NAMES.map((name) => `:(glob)**/${name}`),
  ":(glob)**/*.lock", ":(glob)**/*.lockb", ":(glob)**/*-lock.json",
  ":(glob)**/*-lock.yaml", ":(glob)**/*-lock.yml", ":(glob)**/.cargo/**",
  ...VERIFIER_ROOTS.flatMap((root) => [`:(literal)${root}.rs`, `:(glob)${root}/**`]),
];

export function verificationInput(path) {
  const parts = path.split("/");
  const name = parts.at(-1);
  return VERIFIER_ROOTS.some((root) => path === `${root}.rs` || path.startsWith(`${root}/`))
    || parts.includes(".cargo") || INPUT_NAMES.includes(name)
    || /\.lockb?$/u.test(name) || /-lock\.(?:json|ya?ml)$/u.test(name);
}
