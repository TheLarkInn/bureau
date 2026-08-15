# bureau

A local agent work runner: a CI runner with a work queue and a reconcile
loop, where a step body can be an agent instead of a shell script. One
binary, one process, one machine.

bureau is level-triggered. Each pass compares two things:

- **desired state** — every work item matching an assignment's filter
  should have an open PR
- **observed state** — what the forge (GitHub/ADO) actually shows

and closes the gap by running agent-driven pipelines against git
worktrees. Work is claimed off a backlog by lease, never triggered by a
push. There is no queue table: pending work is a query, and a crash
mid-run is re-observed and resumed on the next pass. `DESIGN.md` is the
authoritative spec.

## Layers

| Layer | What it is | Code |
|---|---|---|
| 0 | Process contract: explicit child env, process-group timeout kill, streamed capture, secrets scrubbed on write | `src/process/` |
| 1 | `fake` adapter: replays recorded transcripts; `record` writes them | `src/adapters/` |
| 2 | Step contract: versioned JSON on stdin/stdout | `src/contract.rs` |
| 3 | Run log: append-only `events.jsonl` per run, the only source of truth for resume | `src/runlog/` |
| 4 | Engine: the step state machine (`deterministic`, `agent`, `decision`) | `src/engine/` |
| 5 | Durable state: SQLite leases, budget counters, dedup | `src/state/` |
| 6 | Git: bare-mirror checkout cache, one worktree per run | `src/git.rs` |
| 7 | Forges: GitHub, ADO, and an in-memory fake behind one `Forge` trait | `src/forge/` |
| 8 | Reconcile loop: desired minus observed minus in-flight, claimed by lease compare-and-swap | `src/reconcile.rs` |

## Quickstart

Config lives in a separate git repository. PR review of that repo is the
entire authorization model.

```
runner-config/
  repos.yaml                        # registry: every repo, with an access level
  roles/implementer.yaml            # agent reference + adapter + permissions + min_trust
  assignments/fix-flaky-tests.yaml  # work source + repos + pipeline + role + limits
  pipelines/fix-failing-test.yaml   # the step state machine
```

Check the whole config in one pass (reports every error, not the first):

```sh
bureau validate [dir]        # default: runner-config
```

Run a pipeline once for one work item:

```sh
bureau run fix-failing-test --item 42
bureau retry <run-id>        # new run for the item an earlier run targeted
```

Inspect and control runs:

```sh
bureau list                  # every run
bureau show <run-id>         # replayed state of one run
bureau cancel <run-id>       # write the run's CANCEL marker
```

The run-side verbs share four filesystem roots with these defaults:
`--config runner-config`, `--runs runs`, `--state state.db`,
`--cache checkout-cache`. `list`, `show`, and `cancel` take only
`--runs`. Exit codes for `run`/`retry`: `0` on success or no-work, `1`
on failure, blocked, claim lost, or already finished, `2` on setup and
usage errors (unknown pipeline, missing credential, item or run not
found). A missing credential fails before any subprocess spawns.

Each run gets a directory under `runs/<run-id>/`: `events.jsonl`
(append-only, fsync'd, secret-scrubbed), a derived `state.json` cache,
`artifacts/`, and the worktree `wt/`.

## Credentials

Config names a credential reference (`credential: ado-main`); the value
is never in git. Resolution order at spawn time:

1. `BUREAU_CREDENTIAL_<NAME>` — the reference uppercased, `-` becomes
   `_` (`ado-main` → `BUREAU_CREDENTIAL_ADO_MAIN`)
2. a file named `<reference>` under `$BUREAU_CREDENTIALS_DIR`

Values are scrubbed from everything written to the run log.

## Testing seams

Everything above layer 1 is testable offline, in milliseconds:

- The `fake` adapter replays a recorded transcript fixture. Record one
  from a real command: `bureau fake record <fixture> -- <argv...>`;
  replay it with `bureau fake replay <fixture>`.
- `FakeForge` is an in-memory forge driven by construction-time state.
- `tests/pipeline_e2e.rs` runs the reference pipeline (claim, reproduce,
  propose, apply, review, verify, push, PR) end to end under both fakes.

## Rust quality gates

All workspace crates inherit deny-level Rust and Clippy lints. Clippy limits
cognitive complexity to 4 and functions to 25 lines. The CI workflow also
rejects Rust source files over 300 lines and lint-suppression attributes,
including `#[allow(...)]` and `#[expect(...)]`.

Custom lints from [`li-kai/rust-lints`](https://github.com/li-kai/rust-lints)
run through Dylint and are promoted to errors in CI.

## Known deltas

Places where the code departs from the spec as written, each with its
reason:

- `engine::log::Appender` mirrors `RunLog`'s wire format (same event
  form, scrub-on-write, fsync per append) because `RunLog` cannot open
  an existing log for append, which resume needs — a future cleanup is
  `RunLog::open`.
- Run-log `output` events carry `stream: "combined"` because layer 0
  multiplexes stdout and stderr into the single spawn sink, so per-chunk
  stream attribution is impossible.
- `Reconciler.forges` is a `Vec<(ForgeKind, Arc<dyn Forge>)>`, not a
  map, because `ForgeKind` derives neither `Ord` nor `Hash`.
- The forge token for `bureau run` comes from the primary repo's
  credential — v0 assumes the work forge shares that credential.
- The `join` terminal is reserved and rejected at config validation in
  v0 — no fan-out.
- Duplicate YAML mapping keys in config files are last-write-wins:
  `serde_yaml_ng` has no duplicate-key rejection, so a repeated key
  (e.g. a second `odsp-web:` in `repos.yaml`) silently shadows the
  earlier entry — review config diffs carefully.
