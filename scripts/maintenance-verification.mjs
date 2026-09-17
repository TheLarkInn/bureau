const INPUT_NAMES = ["Cargo.toml", "Cargo.lock", "package.json", "package-lock.json",
  "npm-shrinkwrap.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb",
  "build.rs", "rust-toolchain", "rust-toolchain.toml", "clippy.toml", ".clippy.toml",
  "rustfmt.toml", ".rustfmt.toml", ".gitattributes", ".gitignore"];

export const VERIFICATION_INPUTS = [
  ...INPUT_NAMES.map((name) => `:(glob)**/${name}`),
  ":(glob)**/*.lock", ":(glob)**/*.lockb", ":(glob)**/*-lock.json",
  ":(glob)**/*-lock.yaml", ":(glob)**/*-lock.yml", ":(glob)**/.cargo/**",
];

export function verificationInput(path) {
  const parts = path.split("/");
  const name = parts.at(-1);
  return parts.includes(".cargo") || INPUT_NAMES.includes(name)
    || /\.lockb?$/u.test(name) || /-lock\.(?:json|ya?ml)$/u.test(name);
}
