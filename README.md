# bureau

See it work in 5 seconds, offline:

```sh
cargo test --offline    # 209 tests, ~4s, no network, no model calls
```

bureau is a CI runner whose step body can be an LLM agent instead of a
shell script. It is level-triggered: each pass compares desired state
(every matching work item should have an open PR) with observed state
(what the forge shows) and closes the gap by running agent pipelines in
git worktrees. Work is claimed off a backlog by lease, never pushed.
`DESIGN.md` is the authoritative spec.

## Run one pipeline

1. Write the config repo (its PR review is the entire authorization
   model):

   ```
   runner-config/
     repos.yaml                        # every repo, with an access level
     roles/implementer.yaml            # agent reference + adapter + permissions + min_trust
     assignments/fix-flaky-tests.yaml  # work source + repos + pipeline + role + limits
     pipelines/fix-failing-test.yaml   # the step state machine
   ```

2. Check it — every error in one pass, exit 1 if any:

   ```sh
   bureau validate runner-config
   ```

3. Run once for one work item:

   ```sh
   bureau run fix-failing-test --item 42
   ```

   Exit `0` on success or no-work, `1` on failure/blocked/claim-lost,
   `2` on setup errors. A missing credential exits `2` before any
   subprocess spawns and names the credential.

## Credentials

Config names a reference (`credential: ado-main`); the value is never in
git. At spawn, bureau checks, in order:

1. `BUREAU_CREDENTIAL_<NAME>` — reference uppercased, `-` → `_`
   (`ado-main` → `BUREAU_CREDENTIAL_ADO_MAIN`)
2. a file named `<reference>` under `$BUREAU_CREDENTIALS_DIR`

Values are scrubbed from everything written to the run log.

## Inspect and control runs

```sh
bureau list                  # every run
bureau show <run-id>         # replayed state of one run
bureau cancel <run-id>       # write the run's CANCEL marker
bureau retry <run-id>        # new run for the item an earlier run targeted
```

Filesystem roots and their defaults: `--config runner-config`,
`--runs runs`, `--state state.db`, `--cache checkout-cache`. `list`,
`show`, and `cancel` take only `--runs`.

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
| 0–3 | Process contract · fake adapter · step contract · run log | `src/process/`, `src/adapters/`, `src/contract.rs`, `src/runlog/` |
| 4 | Engine: the step state machine | `src/engine/` |
| 5 | Durable state: SQLite leases, budget, dedup | `src/state/` |
| 6 | Git: mirror cache, one worktree per run | `src/git.rs` |
| 7 | Forges: GitHub, ADO, in-memory fake | `src/forge/` |
| 8 | Reconcile loop: desired − observed − in-flight, claimed by CAS | `src/reconcile.rs` |

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

Internal departures (no behavior change):

- `engine::log::Appender` mirrors `RunLog`'s wire format; resume needs
  open-for-append, a future `RunLog::open` cleanup.
- Run-log `output` events carry `stream: "combined"` — layer 0
  multiplexes stdout and stderr into one sink.
- `Reconciler.forges` is a `Vec`, not a map (`ForgeKind` lacks `Ord`).
