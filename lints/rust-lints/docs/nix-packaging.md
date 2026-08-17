# Packaging rust-lints as a Nix Flake Output

## Problem

Consumers add our lints via:

```toml
[workspace.metadata.dylint]
libraries = [{ git = "https://github.com/li-kai/rust-lints" }]
```

This causes `cargo-dylint` to clone and build the library from source on every
consumer machine. This has two costs:

1. **Nix consumers can't build it.** The build requires `dylint-link` as a
   custom linker, which calls `rustup` internally. Nix environments don't have
   rustup. Our `flake.nix` solves this for _developers_ of this repo (via shims
   and wrappers), but consumers of the library don't get those shims.

2. **Every consumer rebuilds from source.** Building requires the exact pinned
   nightly with `rustc-dev`, plus `dylint-link`. This
   is slow and fragile.

## Background: how dylint loads and runs lints

Understanding dylint's architecture clarifies the solution.

### The driver model

Dylint does **not** load lint dylibs into the consumer's rustc. Instead, it uses
its own **driver** — a small binary linked against `rustc_driver` from the
_library's_ toolchain. The flow:

```
cargo dylint --all
  ↓
cargo-dylint reads DYLINT_LIBRARY_PATH (or workspace.metadata.dylint)
  ↓
Groups libraries by toolchain (parsed from the @toolchain filename tag)
  ↓
For each toolchain group:
  - Finds or builds a dylint-driver linked against that toolchain's rustc_driver
  - Sets RUSTC_WORKSPACE_WRAPPER to that driver
  - Runs cargo check, which invokes the driver instead of rustc
  ↓
The driver loads the dylib and calls register_lints()
  ↓
Lint passes run during compilation
```

**Key insight:** The packaged driver and library are tied to the _library's_
toolchain, and the `cargo dylint` invocation must run in an environment that is
compatible with that toolchain. A consumer project can still use stable Rust for
normal development, but the supported Nix interface should provide the matching
Dylint runtime environment instead of asking downstream repos to reconstruct it.

### The rustup problem (for nix)

Normally, when dylint encounters a library tagged `@nightly-2026-01-22-<triple>`
and has no matching driver, it uses **rustup** to:

1. Install that nightly toolchain (if missing)
2. Build a driver binary linked against that toolchain's `rustc_driver`
3. Cache the driver for future use

In a nix environment without rustup, step 1 fails. This is the core problem.

### The solution: ship both the dylib and the driver

Dylint supports two environment variables:

- **`DYLINT_LIBRARY_PATH`** — directories containing pre-built
  `lib<name>@<toolchain>.<ext>` files
- **`DYLINT_DRIVER_PATH`** — directory containing pre-built driver binaries,
  structured as `<toolchain>/dylint-driver`

If both are set, dylint does not need rustup to discover or build a driver. It
finds the pre-built library, matches it to the pre-built driver, and runs. In
practice, the supported consumer interface should still provide the matching
toolchain and `cargo-dylint` invocation environment.

## Solution: ship dylib + driver as a Nix flake package

We add a `packages.default` output to our `flake.nix` that produces two
artifacts:

```
$out/
  lib/
    librust_lints@nightly-2026-01-22-x86_64-unknown-linux-gnu.so
  drivers/
    nightly-2026-01-22-x86_64-unknown-linux-gnu/
      dylint-driver
```

### Consumer usage

The supported Nix interface is a public shell helper:

```nix
{
  inputs = {
    rust-lints.url = "github:li-kai/rust-lints";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, rust-lints, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        devShells.default = rust-lints.lib.mkDevShell {
          inherit pkgs;
          packages = [ pkgs.just ];
        };
      }
    );
}
```

The consumer then runs `cargo dylint --all` as normal. The helper shell provides
the matching `cargo-dylint`, toolchain, `rustup` shim, and Dylint env vars. The
consumer project can keep its own normal Rust workflow outside that shell.

Configuration via `dylint.toml` works exactly as before — it's read at lint
time, not build time.

For advanced consumers, the flake still exposes:

```nix
rust-lints.lib.dylint.version
rust-lints.lib.dylint.forSystem system
rust-lints.packages.${system}.default
```

Those are lower-level interfaces. `mkDevShell` is the supported path.

### Naming convention

Dylint discovers libraries and drivers by filename/directory convention:

| Artifact | Convention | Example |
|---|---|---|
| Library | `<DLL_PREFIX><name>@<toolchain><DLL_SUFFIX>` | `librust_lints@nightly-2026-01-22-x86_64-unknown-linux-gnu.so` |
| Driver | `<toolchain>/dylint-driver` | `nightly-2026-01-22-x86_64-unknown-linux-gnu/dylint-driver` |

The `@toolchain` tag in the library filename is how dylint knows which driver to
pair it with. `dylint-link` (our custom linker) produces this tag automatically
during the build.

## What we need to build

The `packages.default` derivation must produce two things:

### 1. The lint library (cdylib)

- Compile `rust-lints` as a cdylib using the pinned nightly from fenix.
- Use `dylint-link` as the linker (required by `.cargo/config.toml`).
- `dylint-link` produces the `@toolchain`-tagged filename automatically.
- Output: `$out/lib/librust_lints@<toolchain>.<ext>`

### 2. The dylint driver

- Build the `dylint-driver` binary from a local shim project
  (`nix/dylint-driver/`) that wraps the `dylint_driver` library crate from
  crates.io. The binary doesn't exist as a published crate — `cargo-dylint`
  normally synthesizes it at runtime; we replicate that offline for Nix.
- The driver binary dynamically links to `librustc_driver` — the sysroot
  libraries must be reachable at runtime (via `-rpath` baked in at link time,
  or via `LD_LIBRARY_PATH`).
- `dylint-driver` has internal version coupling with `cargo-dylint` — version
  mismatches produce silent failures or cryptic errors. Pin both to the same
  version.
- Output: `$out/drivers/<toolchain>/dylint-driver`

### Build-time dependencies (not needed by consumers)

| Dependency | Why | Source |
|---|---|---|
| Rust nightly (`nightly-2026-01-22`) | Compile the cdylib and driver against `rustc_private` APIs | fenix (already in our flake) |
| `rustc-dev` | Provide `rustc_driver` and compiler internals for linking | fenix toolchain components |
| `dylint-link` | Custom linker that produces `@toolchain`-tagged output | Built from crates.io |
| `rustup` shim | `dylint-link` calls `rustup which rustc` internally | Already in our flake |
| Prefetched clippy source | `dylint_driver`'s build script clones rust-clippy to extract symbols from `clippy_utils/src/sym.rs`; Nix builds have no network access | `fetchFromGitHub` at the rev from `Cargo.toml` |
| Git clippy wrapper | Intercepts `git clone`/`checkout` of rust-clippy and serves the prefetched source | `nix/git-clippy-wrapper.sh` |

### Implementation

See `flake.nix` for the full implementation using `crane` with the fenix
toolchain. The key derivations are `dylintLink`, `dylintDriver`, `rustLintsLib`,
and the final `rustLints` symlinkJoin.

### Runtime linking: the `-rpath` detail

The dylint driver dynamically links against `librustc_driver` from the nightly
toolchain's sysroot. Normally this works because rustup sets up `LD_LIBRARY_PATH`
(or `DYLD_LIBRARY_PATH` on macOS) to point at the sysroot.

In Nix, the sysroot lives in `/nix/store/...`. We must bake this path into the
driver binary at build time using `-rpath`, so the driver can find
`librustc_driver` without any environment variable setup. This is standard
practice for Nix — it's how most dynamically-linked binaries work in nixpkgs.

If `-rpath` proves difficult, an alternative is to wrap the driver binary in a
shell script that sets `LD_LIBRARY_PATH` before exec. **Caveat:** the wrapper
changes the binary path, which may confuse dylint's driver discovery (it expects
`<toolchain>/dylint-driver`). If wrapping, ensure the wrapper script itself is
named `dylint-driver` and placed at the expected path:

```nix
dylintDriverWrapper = pkgs.writeShellScriptBin "dylint-driver" ''
  export LD_LIBRARY_PATH="${rustToolchain}/lib:$LD_LIBRARY_PATH"
  exec ${dylintDriver}/bin/dylint-driver "$@"
'';
```

## Multi-platform support

The dylib and driver are both platform-specific. Our flake already uses
`eachDefaultSystem`, so each system produces its own `packages.default` with the
correct target triple. Consumers referencing
`rust-lints.packages.${system}.default` automatically get their platform's
artifacts.

## What this does NOT solve

- **Non-nix consumers.** Teams using rustup can continue with `{ git = "..." }`
  — dylint builds from source and handles the driver automatically. For pre-built
  binaries outside nix, GitHub release artifacts are a separate concern.
- **Pure Nix packaging of `cargo-dylint`.** The supported consumer interface is
  the shell helper. Packaging `cargo-dylint` itself as a standalone derivation is
  a separate concern.

