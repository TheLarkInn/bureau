# bureau

A local runner that uses AI agents and shell commands to turn GitHub issues
or Azure DevOps work items into pull requests. It polls for work, avoids
duplicate claims, and runs pipelines in isolated Git worktrees.

## Install

**Linux only**, including WSL2 or a Linux VM. Install Git and `unshare`.
Keep state and worktrees on a Linux filesystem, not a Windows-mounted drive.
The browser dashboard also needs Node.js.

Download a Linux x86-64 or ARM64 archive and its `.sha256` file from
[Releases](https://github.com/TheLarkInn/bureau/releases/latest).
Verify with `sha256sum --check <archive>.sha256`, extract, and put `bureau`
on your `PATH`. No Rust required.

Or build from a source checkout:

```sh
cargo install --path crates/bureau
```

## Start

Create `init.yaml` using the
[setup guide](https://github.com/TheLarkInn/bureau/blob/main/docs/getting-started.md#first-time-setup-bureau-init), then:

```sh
bureau init --from init.yaml
bureau reconcile
```

`init` opens a config PR and waits for its merge before running work.
Local state lives in `~/.bureau`; `BUREAU_HOME` overrides it.

## Everyday commands

```sh
bureau validate .bureau          # check local config
bureau reconcile --now          # one pass; wait for its runs
bureau list                     # list runs
bureau show <run-id>             # inspect a run
bureau watch                    # live terminal view
bureau dashboard                # browser editor and run viewer
bureau pause <run-id>            # pause at a step boundary
bureau resume <run-id>           # allow run re-entry or reconcile to continue
bureau cancel <run-id>           # request cancellation
bureau retry <run-id>            # start a new run for that item
bureau doctor --json             # offline diagnostics
bureau repair                   # preview and confirm repairs
```

For one item: `bureau run <pipeline> --item owner/repo#42`
(GitHub) or `--item Project/42` (Azure DevOps).

## Before real runs

Config review is authorization. **Never commit credential values.**
Install agents and plugins before running; Bureau does not install them
during a run. Read the
[agent permission and cost caveats](https://github.com/TheLarkInn/bureau/blob/main/docs/getting-started.md#agent-transport).

## Known deltas

- The `join` terminal is unsupported.
- `bureau run` uses the primary repo's credential for the work forge.
- Duplicate YAML keys use the last value. Review config diffs carefully.
- Step stdout/stderr share `stream: "combined"`; run messages use `"run"`.

## Development

Tests use fake agents and forges, with no model calls.

```sh
cargo fmt --all
bash scripts/lint.sh
cargo test --offline
```

See [contributor rules](https://github.com/TheLarkInn/bureau/blob/main/AGENTS.md)
for tool setup and enforced limits.

[Setup](https://github.com/TheLarkInn/bureau/blob/main/docs/getting-started.md) |
[Canvas](https://github.com/TheLarkInn/bureau/blob/main/.github/extensions/bureau-canvas/README.md) |
[Design](https://github.com/TheLarkInn/bureau/blob/main/DESIGN.md) |
[Release process](https://github.com/TheLarkInn/bureau/blob/main/docs/releases.md)
