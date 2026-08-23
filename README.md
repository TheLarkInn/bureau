# bureau

See it work in 5 seconds, offline:

```sh
cargo test --offline    # no network, no model calls
```

bureau is a CI runner whose step body can be an LLM agent instead of a
shell script. It is level-triggered: each pass compares desired state
(every matching work item should have an open PR) with observed state
(what the forge shows) and closes the gap by running agent pipelines in
git worktrees. Work is claimed off a backlog by lease, never pushed.
`DESIGN.md` is the authoritative spec. New here?
[docs/getting-started.md](docs/getting-started.md) walks through setup for
single- and multi-repository layouts on both GitHub and Azure DevOps.

## Plugins and agent resources

The installable `bureau` plugin is the primary agent surface:

- `/bureau:implementer`
- `/bureau:reviewer`
- `/bureau:pipeline-author`
- `/bureau:run-inspector`

Roles reference these resources directly. Agent files own their model,
instructions, and tools; bureau owns only pipeline orchestration, permissions,
trust, durable execution, and forge effects.

## Initialize and reconcile

Local state defaults to `~/.bureau`; set `BUREAU_HOME` to override it.

```sh
bureau init --from init.yaml       # first-time reviewed config proposal
bureau setup --from settings.yaml # replace non-secret local settings
bureau doctor --json              # read-only offline diagnostics
bureau repair                     # preview, then confirm reversible repairs
bureau reconcile                  # continuous desired-vs-observed loop
bureau reconcile --now            # one pass, waiting for started runs
```

`init` previews and validates the generated config, opens a config PR, waits
for its forge-owned merge state, validates the exact merged commit, runs one
foreground reconcile pass, then writes `settings.yaml` as the completion
marker. It never executes unmerged config.

`setup` may explicitly migrate a prior local-state root. Migration rejects
overlapping paths, active leases, symlinks, hard links, corrupt or newer
database schemas, and non-empty targets. It imports only durable state and run
history; worktrees, activation records, credentials, and disposable caches are
not copied. A durable migration marker blocks normal workers and makes setup
retries resume or roll back safely after interruption.

## Run one pipeline

1. Review and commit the config (its PR review is the entire authorization
   model):

   ```
   runner-config/
     repos.yaml                        # every repo, with an access level
     roles/implementer.yaml            # agent reference + adapter + permissions + min_trust
     assignments/fix-flaky-tests.yaml  # work source + repos + pipeline + role + limits
     label_rules/graduate-unblocked.yaml # bounded dependency-driven label updates
     pipelines/fix-failing-test.yaml   # the step state machine
   ```

2. Check it — every error in one pass, exit 1 if any:

   ```sh
   bureau validate runner-config
   ```

3. Run once for one work item from the configured committed source:

   ```sh
   bureau run fix-failing-test --item 42
   ```

   Exit `0` on success or no-work, `1` on failure/blocked/claim-lost,
   `2` on setup errors. A missing credential exits `2` before any
   subprocess spawns and names the credential.

## Credentials

Config names a reference (`credential: ado-main`); the value is never in
git. `settings.yaml` declares exactly where each reference resolves:

- one environment variable;
- one exact file; or
- one credential directory containing a file named after the reference.

A credential may also declare the forge `identity` it must authenticate
as. Before a run spawns anything, each credential its repos require is
checked against the forge a repo naming that credential points at — and
never against any other host: a refused value fails the run as invalid or
expired, and a value belonging to another account fails it as a wrong
identity, naming the reference and both identities. Declaring no
`identity` verifies the value and matches it against no name. A GitHub
App installation token can be checked for validity but carries no account
name, so it satisfies no declared `identity`. The result is pinned in the
run log, and a resumed run re-checks its freshly resolved credentials
against that pinned identity, so a value rotated mid-run aborts the
resume rather than continue as somebody else.

Values are scrubbed from everything written to the run log.

## Inspect and control runs

```sh
bureau list                  # every run
bureau show <run-id>         # replayed state of one run
bureau watch                 # live terminal dashboard of local state
bureau cancel <run-id>       # write the run's CANCEL marker
bureau pause <run-id>        # write the run's PAUSE marker
bureau resume <run-id>       # clear it; run re-entry or reconcile continues
bureau retry <run-id>        # new run for the item an earlier run targeted
```

`watch` is read-only: it never writes state.db or run directories and
never takes the maintenance lock, so it is safe alongside a live daemon.
It shows the adopted config commit, live lease and running-run counts,
one row per run (status, latest step, cost so far, age), per-assignment
budget counters (today's cost, runs this hour, headroom — the open-PR
limit is forge state and is excluded), and the selected run's latest
events. It refreshes once a second; `q`, `Esc`, or `Ctrl-C` quits, and
`up`/`down` select a run. Piped instead of a terminal, it prints one
plain-text snapshot and exits.

The fixed home layout contains `settings.yaml`, `credentials/`, `state.db`,
`runs/`, `checkout-cache/`, and `config-cache/`. Explicit path overrides are
available for contained deployments; `list`, `show`, `cancel`, `pause`, and
`resume` take only `--runs`.

Each run writes `runs/<run-id>/`: `events.jsonl` (append-only, fsync'd,
secret-scrubbed — the only source of truth), `state.json` (derived
cache), `artifacts/`, and the worktree `wt/`.

## Test without a forge or a model

- `fake` adapter: record a real command with
  `bureau fake record <fixture> -- <argv...>`, replay it with
  `bureau fake replay <fixture>`.
- `FakeForge`: an in-memory forge driven by construction-time state.
- `tests/pipeline_e2e.rs`: the reference pipeline (claim, reproduce,
  propose, apply, review, verify, push, PR) end to end under both fakes.

## Layer map

| Layer | What it is | Code |
|---|---|---|
| 0–3 | Process contract · fake adapter · step contract · run log | `crates/bureau/src/process/`, `adapters/`, `contract.rs`, `runlog/` |
| 4 | Engine: the step state machine | `crates/bureau/src/engine/` |
| 5 | Durable state: SQLite leases, budget, dedup | `crates/bureau/src/state/` |
| 6 | Git: mirror cache, one worktree per run | `crates/bureau/src/git.rs` |
| 7 | Forges: GitHub, ADO, in-memory fake | `crates/bureau/src/forge/` |
| 8 | Reconcile loop: desired − observed − in-flight, claimed by CAS | `crates/bureau/src/reconcile/` |
| Local lifecycle | Home, settings, init/setup, doctor, repair policy | `crates/bureau-lifecycle/` |
| Plugin runtime | Package, resolution, snapshots, activation, restoration | `crates/bureau-plugin/` |

## Rust quality gates

All workspace crates inherit deny-level Rust and Clippy lints. Clippy limits
cognitive complexity to 4 and functions to 25 lines. The CI workflow also
rejects Rust source files over 300 lines and lint-suppression attributes,
including `#[allow(...)]` and `#[expect(...)]`.

Custom lints from [`li-kai/rust-lints`](https://github.com/li-kai/rust-lints)
run through Dylint and are promoted to errors in CI.

## Known deltas

Behavioral departures from the spec as written, each with its reason:

- `join` terminal: rejected at config validation in v0 (no fan-out).
- Forge token for `bureau run` comes from the primary repo's credential
  (v0 assumes the work forge shares it).
- Duplicate YAML mapping keys are last-write-wins (`serde_yaml_ng` has
  no rejection) — review config diffs carefully.

Run-log step `output` events carry `stream: "combined"` because layer 0
multiplexes a step's stdout and stderr into one scrubbed sink; run-level
messages use `stream: "run"`.
