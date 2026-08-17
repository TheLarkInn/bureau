# Binary Cache via Cachix

Prerequisite: the Nix flake package must be building correctly first. See
[docs/nix-packaging.md](./nix-packaging.md).

## Setup

We already have a Cachix cache (`li-kai.cachix.org`) used by
[treetok](https://github.com/li-kai/treetok). We reuse the same cache and the
same CI pattern here.

### `nixConfig` block

Copy from treetok's `flake.nix` — identical config:

```nix
{
  nixConfig = {
    extra-substituters = [ "https://li-kai.cachix.org" ];
    extra-trusted-public-keys = [
      "li-kai.cachix.org-1:hT/YtROuqsBhfSx1YDcMrFxBbnZLoyu+WA1CnhiUgWM="
    ];
  };

  # ... rest of flake
}
```

## CI: build and push

The CI workflow builds and then explicitly pushes only the output store path.
`cachix-action` is configured with `skipPush: true` so the daemon does not
auto-push everything (which would include the fenix toolchain). Instead, the
explicit `cachix push` targets just our output:

See [`.github/workflows/nix.yml`](../.github/workflows/nix.yml) for the full
workflow.

Without cachix, consumers referencing our flake output build from source —
downloading the nightly toolchain and compiling everything. With cachix, they
download pre-built artifacts directly.

### Version pinning and CI ordering

When you bump the nightly toolchain, consumers with a cached `flake.lock` still
reference the old revision — that's fine. But when a consumer updates their lock,
the new dylib + driver must already be in cachix, or they fall back to building
from source — which fails on Nix (the original problem).

**CI must build and push to cachix before any consumer could resolve the new
revision.** In practice: the cachix push job should run on every push to `main`,
and you should not tag a release until CI confirms the cachix push succeeded.

## Storage budget

Our artifacts are small, but the transitive closure is not:

| What | Size estimate | Per platform |
|---|---|---|
| `librust_lints@<toolchain>.<ext>` | ~5–15 MB | yes |
| `dylint-driver` binary | ~10–20 MB | yes |
| **Our outputs total** | **~15–35 MB** | × 2–4 platforms |
| fenix nightly toolchain (with `rustc-dev`) | ~500 MB – 1 GB | × 2–4 platforms |

**Push only our output store path, not the full closure.** Capture the
output path from `nix build --print-out-paths` and push just that (not
`--watch-store` or `nix-store -qR`). This avoids caching the fenix
toolchain through our cache. Consumers pull the toolchain from
[fenix's own cache](https://fenix.cachix.org) instead.
