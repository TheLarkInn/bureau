# Build a local agent work runner

> ## ⛔ ENFORCED RULES — READ BEFORE ANY CODE CHANGE ⛔
>
> These rules are **deny-level**: CI fails and the change is rejected if any is
> violated. They are enforced by `bash scripts/lint.sh` (repository policy +
> `cargo fmt --check` + clippy + the vendored dylint lints in
> `lints/rust-lints`, configured by `dylint.toml`).
>
> **Before finishing ANY change, run all gates and fix every finding:**
>
> ```sh
> cargo fmt --all
> bash scripts/lint.sh
> cargo test --offline
> ```
>
> ### Hard limits (deny-level)
>
> | Limit | Value | Enforced by |
> |---|---|---|
> | Function length | 25 lines | clippy `too_many_lines` |
> | Cognitive complexity | 4 — each `assert!` and `.await` costs 1 | clippy `cognitive_complexity` |
> | File length | 300 lines | `scripts/check-rust-policy.sh` |
> | `#[allow]`/`#[expect]` in source | **zero** | policy script + clippy `allow_attributes` |
> | `unsafe` | **zero** | `unsafe_code = "forbid"` |
> | Compiler warnings | **zero** | `warnings = "deny"` |
>
> Clippy groups at deny: `all`, `cargo`, `complexity`, `correctness`,
> `nursery`, `pedantic`, `perf`, `style`, `suspicious`.
>
> Two deliberate lint adjustments (root `Cargo.toml` comments carry the
> reasons): `allow_attributes` is `deny`, not `forbid` — `forbid` breaks
> spec-approved derive macros (clap); `multiple_crate_versions` is `allow` —
> reqwest's own tree requires both syn 2 and syn 3.
>
> ### dylint module architecture (dylint.toml) — MANDATORY
>
> `dylint.toml` is the workspace module architecture and is **exhaustive**:
>
> - Every top-level module of every crate must be listed under
>   `[module_dependencies.allow]`.
> - Every cross-module dependency must be declared there as a
>   `"<crate>:<module>" = [...]` edge.
> - **Adding a module or a cross-module `use` means updating `dylint.toml` in
>   the same change — the lint fails otherwise.** The lint flags both
>   undeclared edges and, within a crate, declared edges no longer present in
>   code.
> - Never add a dependency edge that is not already declared without editing
>   `dylint.toml` in the same commit.
>
> ### Writing tests under these limits
>
> A test function fits at most ~3 assert-family calls. So:
>
> 1. Table-drive: one assert inside a loop over cases.
> 2. Or aggregate values into a tuple/`Vec` and assert once.
> 3. `.expect()` and `?` are free — use them for setup.
>
> Pattern to copy: `crates/bureau/tests/process_contract.rs`.
>
> ---

You are building a new project from scratch in this empty repository. Read this
entire brief before writing anything. It is the authoritative spec and it overrides
conventions you would otherwise assume, including anything you know about a project
called "Goobers" — this is a clean-room rebuild of that idea with different
priorities.

Working name: `bureau`. Placeholder, swappable with one sed. Do not propose a mascot,
an animal, or a themed name.

**Language: Rust, 2021 edition, stable toolchain.** Pin it with `rust-toolchain.toml`.

---

## 1. What this is

A single-binary daemon that continuously compares two things:

- **desired state** — "every work item matching this filter should have an open PR"
- **observed state** — what the forge (GitHub/ADO) actually shows right now

and closes the gap by running agent-driven pipelines against git worktrees.

It is **a CI runner with a work queue and a reconcile loop**. The step body happens
to be an LLM instead of a shell script. That is the only novel part of the execution
model; everything else is ordinary systems engineering and should look ordinary.

It runs in a Linux dev container on one developer's machine. One process. No cluster,
no control plane, no service.

### What it does NOT own

The forge owns these. Consume them via API; never reimplement:

work item storage · assignment · labels · repos · pull requests · review threads ·
merge queues · identity · human authorization

You own exactly four things that no existing runner provides:

1. **Pull-based claiming** — work is claimed off a backlog, not triggered by a push
2. **Durable cross-run state** — leases, budget, dedup survive between runs
3. **Nondeterministic cost control** — a run might cost $12; stop at $50/day
4. **Nondeterministic output handling** — retry with a different model, review gates

---

## 2. Naming law

Violations of this section are build failures, not style notes.

1. **No invented nouns.** If a competent backend engineer cannot guess a term's
   meaning from the word alone, it is the wrong term.
2. **No mascots, no in-jokes, no themed vocabulary.** Not in type names, not in CLI
   verbs, not in log strings, not in module names, not in test fixtures.
3. **Reuse the industry word that already means the thing:** `queue`, `lease`,
   `budget`, `worktree`, `step`, `run`, `role`, `adapter`, `permission`, `trust`,
   `reconcile`, `forge`.
4. **CLI verbs are verbs:** `run`, `list`, `show`, `cancel`, `retry`, `validate`.
5. **Do not import Kubernetes vocabulary.** Take the reconcile *pattern*; refuse the
   words. `operator`, `controller`, `CRD`, `custom resource`, `namespace`,
   `reconciler manager` are all banned. `reconcile`, `desired`, `observed`, `drift`,
   `apply` are ordinary English and are fine.

Standard Rust casing applies: `PascalCase` types, `snake_case` fields and functions,
`SCREAMING_SNAKE_CASE` consts. Serialized field names are `snake_case` except where
this brief shows otherwise.

If you need a term that is not below and is not an ordinary industry word, stop and
ask before using it.

| Concept | Use this | Never |
|---|---|---|
| agent role definition | `role` | goober, persona, character |
| standing config: backlog + repos + policy | `assignment` | gaggle, namespace, project, tenant |
| one execution | `run` | instance, invocation, job (ambiguous) |
| the step state machine | `pipeline` | workflow, DAG, graph |
| one agent CLI integration | `adapter` | harness, driver, provider |
| GitHub/ADO API integration | `forge` | provider, backend, SCM |
| append-only run record | `run log` | journal, ledger, chronicle |
| claim record with expiry | `lease` | claim ledger, reservation |
| branch on a step outcome | `decision` | gate, judge, arbiter |
| concurrent steps | `concurrent group` | fan-out, parallel (noun), scatter |
| where work items come from | `work source` | backlog, hopper |
| bare clone cache | `checkout cache` | workcopies |
| credential grant | `permission` | capability |
| input provenance label | `trust` | integrity, taint |

---

## 3. Non-goals — do not build these

Each exists in the system this replaces, and each is a stub, a mistake, or scope
creep. If you find yourself building one, stop and tell me the spec is wrong.

| Do not build | Why |
|---|---|
| A DSL with a compile/codegen step | The file you edit must be the file that runs. No lock files, no generated execution artifacts. |
| Kubernetes / pod / namespace / identity abstractions | The reference implementation ships these fields and its pod launcher returns `not implemented`. Pure vocabulary tax. |
| A second execution engine (Temporal etc.) | One engine. |
| A content-addressed artifact store | Directories and files. Add CAS when a measured problem demands it. |
| Merge arbitration / "who lands first" election | The forge has a merge queue. Use it. |
| Multi-tenancy, org models, human authz | One developer, one machine. PR review of the config repo IS the authorization model. |
| An issue/PR/comment data model of your own | The forge is the database. |
| A general host-capability matching engine | You are in a container. Provision the environment; do not match against the host. |
| Self-update machinery | `git pull`. |
| A proc-macro crate, a trait-heavy plugin system, or generics for their own sake | Concrete types until duplication proves otherwise. |
| More than 17 CLI verbs | Hard cap; `pause` and `resume` named the last two. |

Target for the complete system, all layers: **under 15,000 lines**. The reference
implementation is 330,000 lines and ~190 CLI verbs for the same feature set. Almost
all of that is surface area, not capability.

---

## 4. The control model — read this twice

This is the decision everything else follows from, and it is the one most likely to
be built wrong by reflex.

**The runner is level-triggered, not edge-triggered.**

| | Edge (webhooks, CI, Actions) | Level (this system) |
|---|---|---|
| Asks | "what just happened?" | "does reality match intent?" |
| Requires an event | yes | no |
| Dropped event | work is lost forever | next pass catches it |
| Duplicate event | duplicate work | no-op |
| Crash mid-run | orphaned | re-observed and resumed |

**Events are a latency optimization and are never a source of truth.**

```
webhook → "wake up and reconcile now"     ✅  payload is DISCARDED
webhook → "start a run for item #42"      ❌  never do this
```

The loop must be fully correct with every webhook unplugged. A webhook only shortens
the interval from minutes to seconds. Because polling works identically on every
forge, your correctness path stays forge-agnostic and only the optimization is
forge-specific.

### Consequence: there is no queue table

Pending work is a **query**, not stored state:

```
pending = query(work_source, filter) − has_open_pr − has_active_lease
```

Do not build queue storage, requeue logic, visibility timeouts, or ordering.
Scheduler state stores exactly two things:

- **leases** — closes the race between observing and acting. Requires
  compare-and-swap. Observation narrows the race window; only CAS closes it.
- **budget counters** — must be checkable cheaply before spawning anything.

Plus run logs and label-rule audit events, which are records, not scheduler state.

---

## 5. Configuration is git

Config is reviewed Git state. Its location follows its scope:

- **one work repository** — config lives in that repository under `.bureau/`;
- **several work repositories** — config lives in a separate repository
  identified by remote URL plus ref.

The multi-repository distinction matters: an assignment may reference repos you
do not own and cannot commit to. A local config checkout is always a disposable
cache, never the system's identity.

```
runner-config/                    # multi-repository mode
  repos.yaml                       # registry: every repo, with an access level
  roles/implementer.yaml
  assignments/fix-flaky-tests.yaml
  label_rules/graduate-unblocked.yaml
  pipelines/fix-failing-test.yaml

work-repo/.bureau/                # single-repository mode, same schema
  repos.yaml
  roles/implementer.yaml
  assignments/fix-flaky-tests.yaml
  label_rules/graduate-unblocked.yaml
  pipelines/fix-failing-test.yaml
```

**PR review of this repo is the entire authorization model.** Granting a role
`pr:merge` requires an approved PR. Do not build a permission system on top of that.

### What is in git vs. never in git

| In git (desired state) | Never in git |
|---|---|
| roles, pipelines, assignments, label rules | leases |
| repo registry + access levels | budget counters |
| filters, budget *limits* | run history and logs |
| credential *references* | dedup markers |
| | label-rule audit events |
| | credential *values* |

Git holds human-written, low-frequency, reviewed intent. Machine-written
high-frequency state goes to SQLite locally and to the forge where it should be
visible to humans (labels, PR state).

**Never write status back to git.** Flux and Argo do not write pod status back to the
repo either. Break this and the config repo becomes a transactional database: a commit
per claim, merge conflicts, API rate limits, and repo bloat.

### Config forge ≠ work forge

The repo holding config and the forge holding work items are independent settings.
Config in GitHub with work items in ADO is a valid and expected configuration. Keep
them separate from the first commit; merging them is a one-line assumption that is
painful to unwind.

---

## 6. Data model

Exact shapes. Do not add fields not listed here without asking.

### repos.yaml — a registry, referenced by many assignments, owned by none

```yaml
repos:
  odsp-web:
    url: https://dev.azure.com/microsoft/Odsp/_git/odsp-web
    forge: ado
    access: push          # push | pr | read
    credential: ado-main  # a REFERENCE, resolved at spawn
  augloop:
    url: https://dev.azure.com/office/Augmentation/_git/augloop
    forge: ado
    access: read
    credential: ado-main
```

`access` is a per-repo grant, not a global one. A run gets a token that can push to
`odsp-web` and a token that can only read `augloop`. This replaces the
"primary repo + siblings" two-tier model, which cannot express the common case of
needing read-only context from repos in other organizations.

### roles/&lt;name&gt;.yaml — a reference plus a grant, NOT a new schema

```yaml
name: implementer
agent: /bureau:implementer            # plugin invocation, or a path to an agent .md
adapter: copilot                      # copilot | claude | fake
permissions: [repo:read, repo:write, repo:push, pr:write]
min_trust: maintainer
```

**Critical:** the agent's name, description, instructions, tools, and model already
have a standard home — the plugin/agent frontmatter format:

```yaml
---
name: codebase-analyzer
description: Analyzes codebase implementation details...
tools: Glob, Grep, Read, LS, Bash
model: opus
---
<the instructions are the file body>
```

Do not re-declare any of that in YAML. Reference it. The role adds only what the
agent format has no opinion about because it lives outside the agent process:
which adapter binary, which credential grants, and minimum input trust. Model
and tools live in the agent resource. Concurrency lives on assignments and
concurrent groups, never roles.

The payoff: the same agent file a developer invokes locally by typing
`/atomic:codebase-analyzer` runs unmodified in automation. Author locally, ship by
naming it. No translation step.

`permissions` and `tools` are different layers and must not be merged.
`tools: Bash` = "may call bash inside the session."
`repo:push` = "gets a credential that can push." Both are needed.

### assignments/&lt;name&gt;.yaml — the standing arrangement

```yaml
name: fix-flaky-tests
work:
  forge: ado
  source: "Odsp/odsp-web"
  filter: |                      # forge-NATIVE query, opaque to the runner
    [System.WorkItemType] = 'Bug'
      AND [System.Tags] CONTAINS 'agent-eligible'
      AND [System.State] = 'Active'
  approval_label: agent-approved # optional; required when ADO input must become maintainer
  abort_label: bureau:failed      # added when the pipeline reaches `abort`
  escalate_label: bureau:needs-human # added when it reaches `escalate`
repos: [odsp-web, spo.core, augloop]   # first is primary; the branch lands there
pipeline: fix-failing-test
role: implementer
verify: "rush test --to odsp-web"
branch_prefix: runner/
limits:
  max_concurrent: 2              # every field is independently optional
  max_runs_per_hour: 6
  max_runs_per_day: 40
  max_open_prs: 5
  max_cost_per_day_usd: 25
  max_run_hours: 24              # omitted uses the system default of 24
```

`filter` is a **forge-native query string passed through verbatim** — WIQL for ADO,
search syntax for GitHub. Do not invent a filter language and do not reduce this to a
label list. A label list cannot express "assigned to me AND not blocked AND in this
area path," and you will hit that limit immediately.

`abort_label` and `escalate_label` are required and must differ. Bureau adds the
matching label without replacing unrelated forge labels, removes the other terminal
label, and clears both after `done`. Label-delivery failures are recorded in the run
message; they never erase the terminal result.

`limits` is a kill switch that stops a runaway loop from costing $400 overnight. It is
not chargeback and not cost accounting — that would imply multi-tenancy, which is a
non-goal.

The `limits` block and every field in it are optional. Omitted concurrency,
rate, open-PR, and cost limits mean unlimited. Omitted `max_run_hours` uses the
system default of 24 hours; an explicit positive value may raise or lower it.

`approval_label` is a hard pre-claim admission check and grants `Maintainer`
trust. Recheck it at every step boundary and before publication. Removing it
blocks the active run and requires explicit retry. An ADO assignment with a
reachable agent step requiring more than `Untrusted` trust must configure it.

### label_rules/&lt;name&gt;.yaml — bounded label reconciliation

```yaml
name: graduate-unblocked
work:
  forge: github
  source: TheLarkInn/bureau
  filter: "is:issue is:open label:agent-blocked"
when: dependencies_closed
add_labels: [agent-eligible]
remove_labels: [agent-blocked]
limits:
  max_updates_per_hour: 20
```

A label rule is not a pipeline. It creates no worktree or PR, invokes no model,
and writes no content-dedup marker. On every pass it evaluates matching work
items through the forge's dependency API. `dependencies_closed` applies when
every blocking issue is closed, including the empty set. Bureau then adds and
removes only the configured labels, preserving unrelated labels.

The source must match a GitHub repo in `repos.yaml`; that registry entry supplies
the credential. A lease prevents two reconcilers from mutating the same item.
`max_updates_per_hour` is required and counts attempted mutations, including
failures. Each attempt durably records `update_started` followed by
`update_applied` or `update_failed`, with a human-readable explanation, the
labels, rule, item, and observed dependency counts. Open dependencies are a
no-op and are reconsidered on later passes. Failed and interrupted deltas remain
recoverable by item identity even after the item leaves the filter; recovery
records `update_abandoned` only when the forge reports the item permanently
missing. All rules share one label-reconcile lease scope, so overlapping rules
cannot mutate the same forge item concurrently.

---

## 7. Build order — strictly enforced

**Rule: layer N is not started until layer N−1 has tests that pass offline, with no
network and no model calls.**

This ordering is the entire point of the project. Every agent framework that gets it
wrong becomes untestable, because each test costs money and returns different bytes.
If the adapter is not fakeable on day one, the project is dead.

### Layer 0 — the process contract

The most important interface in the system.

```rust
pub struct SpawnRequest {
    pub argv: Vec<String>,
    pub dir: PathBuf,                    // always a worktree path, never the daemon's cwd
    pub env: BTreeMap<String, String>,   // the COMPLETE child environment
    pub stdin: Vec<u8>,
    pub timeout: Duration,
    pub secrets: Vec<Secret>,            // values scrubbed from all captured output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnOutcome {
    Exited,       // ran to completion; see exit_code
    Timeout,      // hard-killed after timeout
    Signaled,     // killed externally
    SpawnFailed,  // never started
}

pub struct SpawnResult {
    pub outcome: SpawnOutcome,
    pub exit_code: Option<i32>,   // Some only when outcome == Exited
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub duration: Duration,
    pub error: Option<String>,
}
```

`SpawnResult` is a struct with an enum discriminant rather than a data-carrying enum:
all four outcomes share `duration`, `stdout`, and `stderr`, and the struct serializes
straight into the run log. Do not "improve" this into four variants with duplicated
fields.

`Secret` is a newtype wrapping `String` with a hand-written `Debug` that prints
`Secret(***)` and a `Drop` that zeroes the buffer. A secret must be structurally
impossible to leak through `{:?}` or a panic message.

Requirements, all testable with `/bin/sh` and no model:

- **`env` is complete and explicit.** Call `Command::env_clear()` before `.envs()`.
  There must be no code path that inherits the parent environment. Test it directly by
  setting a sentinel var in the test process and asserting the child cannot see it.
- A missing required credential fails **before spawn**, with the credential name in
  the error.
- **Timeout hard-kills the process group**, so orphaned children die too. Use
  `std::os::unix::process::CommandExt::process_group(0)` at spawn, then signal the
  negated pgid on timeout. Killing only the direct child leaves grandchildren alive —
  test this with a shell that backgrounds a `sleep`.
- **Streamed capture** to the run log as output arrives, not buffered until exit.
- **Secrets scrubbed at the write boundary**, keyed on the values in `secrets`.
  Scrub on write, never on read. The scrubbing writer must retain a tail of
  `max_secret_len − 1` bytes across writes so a secret split across two chunks is
  still caught. Test exactly that case.
- Those four outcomes are genuinely distinct. Do not collapse timeout into an exit
  code, and do not represent "no exit code" as `-1`.

### Layer 1 — the fake adapter

A `fake` adapter that replays a recorded transcript from a fixture file, plus a
`record` mode on real adapters that writes those fixtures.

Every test above this layer uses `fake`. This makes the entire engine, lease, budget,
git, and reconcile logic testable in milliseconds, offline, deterministically, in CI.
**This is the load-bearing decision of the whole project.**

### Layer 2 — step I/O contract

JSON on stdin, JSON on stdout. Steps communicate **only** through this and through
artifact file paths. No shared memory, no ambient globals, no reading each other's
scratch directories.

```rust
pub const SCHEMA_VERSION: &str = "v2";

#[derive(Serialize, Deserialize)]
pub struct WorkItem {
    pub external_id: String,
    pub title: String,
    pub body: String,
    pub url: String,
    pub labels: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct StepRequest {
    pub schema: String,
    pub run_id: String,
    pub step: String,
    pub worktree: PathBuf,
    pub item: WorkItem,
    pub trust: Trust,
    pub inputs: BTreeMap<String, serde_json::Value>,
    pub artifacts: BTreeMap<String, PathBuf>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepOutcome { Success, Failure, Blocked, NoWork }

#[derive(Serialize, Deserialize)]
pub struct StepResult {
    pub schema: String,
    pub outcome: StepOutcome,
    pub outputs: BTreeMap<String, serde_json::Value>,
    pub artifacts: Vec<Artifact>,
    pub trust: Trust,
    pub message: String,
}
```

Wire form, for reference:

```jsonc
{ "schema": "v2", "outcome": "no-work", "outputs": {}, "artifacts": [],
  "trust": "derived", "message": "" }
```

**Version the schema in the first commit.** Reject any payload whose `schema` is not
`SCHEMA_VERSION`, with the received value in the error. The reference implementation
is on its eighth revision of this contract; that churn is the clearest evidence that
it should have been versioned immediately.

`Blocked` and `NoWork` are **not failures** and must not consume retry budget. This
distinction is why the outcome set has four members instead of two.

`item` is the work item the run acts on, carried on every step's request so a step
never has to go looking for its assignment. It is the wire projection of a
`forge::Item`, not a second data model (section 3): the forge remains the database,
and the item's own trust grade is not repeated because `trust` above already holds
the request's provenance floor.

A step answers on stdout. A real agent CLI renders its tool transcript to that same
stream ahead of the final message, so **parse the result as a document embedded in
the output, not as the whole buffer**, searching from the end. Requiring the whole
buffer to be one document silently discards results that were published correctly.
Deterministic steps keep the strict whole-buffer parse: their command output is
arbitrary, and a build that prints a `v2`-shaped object must not seize the outcome.

### Layer 3 — the run log

One directory per run. Append-only `events.jsonl`: fsync'd, sequence-numbered,
secret-scrubbed on write.

```
runs/<run-id>/
  events.jsonl      # append-only, THE source of truth
  state.json        # derived cache; must be reconstructible by replay
  artifacts/
  wt/               # the worktree
```

**The event log is the only source of truth for resume.** Any `state.json` is a
derived cache. Write a test that deletes `state.json`, replays the log, and asserts
identical state. This is the one design decision worth copying verbatim from the
reference implementation.

### Layer 4 — the pipeline engine

A state machine over steps. Exactly two step types: `deterministic` (runs code) and
`agent` (runs an adapter). **Alternating them is the entire value proposition** — the
agent proposes, code verifies.

- Edges are explicit. A step names its successor; a decision names one per outcome.
- **A missing branch fails closed.** Never fall through to a default. Do not write a
  catch-all `_ =>` arm that resumes execution; exhaustive `match` is the point.
- Data flow is explicit (`inputs_from: <step>`). No implicit context accumulation.
- Terminals: `done`, `abort`, `escalate`.

A fourth state kind, `concurrent`, runs a fixed list of evidence-only steps
against identical detached snapshots, then emits one namespaced result and
routes through ordinary outcome edges:

```yaml
- name: inspect-change
  type: concurrent
  steps: [run-tests, review-change, check-policy]
  completion: all              # all | stop_on_failure
  max_concurrent: 3            # optional; defaults to every listed step
  next: evaluate-results
  on_failure: repair
  on_blocked: escalate
  on_no_work: done
```

Each member is exactly one step, belongs to at most one group, has no edges,
cannot be the entry or an ordinary edge target, cannot be concurrent, and
cannot consume a sibling's output. Members may only read repos/forges, read run
evidence, and invoke a model. Repository edits are discarded; outputs and
artifacts survive. `all` is the default. `stop_on_failure` cancels unfinished
members only after `Failure`; `Blocked` and `NoWork` do not cancel siblings.

The aggregate outcome is `Failure` when any member fails, else `Blocked` when
any blocks, else `NoWork` when all report no work, else `Success`. Downstream
inputs are namespaced by group and member.

### Layer 5 — durable state (SQLite)

There is no queue table (§4). Scheduler state has two concerns:

- **leases** — `(assignment, forge, external_id)` unique, with expiry. Expiry-based
  and renewable so a crashed run releases automatically. Enforce single-claim with a
  **unique index inside a transaction**, not an in-process `Mutex`. The reference
  implementation guards this with a process-local mutex, which silently constrains it
  to one daemon forever; put the constraint in the database.
- **budget** — counters checked *before* spawn.

**Dedup lives here.** Content-hash the proposed output; if an identical proposal is
already open or was previously rejected, exit `NoWork`. Without this, a scheduled
pipeline re-proposes the same change forever.

Label-rule events are append-only audit records, not scheduler state. An
`update_started` record also supplies the rolling-hour attempt count used to
enforce `max_updates_per_hour` atomically before a forge mutation.

### Layer 6 — git

- Bare mirror per remote in the checkout cache, keyed by hash of the URL.
- One worktree per run:
  `git worktree add --no-track -b <branch_prefix><pipeline>/<run-id>`
- Read-only steps use `--detach` — git refuses the same branch in two worktrees.
- Every branch carries `branch_prefix` so cleanup is one glob.
- Worktree teardown is idempotent and runs on the unwind path, not only the happy
  path. A `Drop` guard or an explicit `scopeguard`; do not rely on a trailing call.

Shell out to the `git` binary through layer 0. Do not link `git2`/libgit2 in v0.

### Layer 7 — forges

Interface first. GitHub second. ADO third. Nothing else until both are boring.

Call the REST/GraphQL APIs directly. **Do not shell out to `gh` or `az`.**

```rust
#[async_trait]
pub trait Forge: Send + Sync {
    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>>;
    async fn open_prs(&self, repo: &str, branch_prefix: &str) -> Result<Vec<Pr>>;
    async fn create_pr(&self, req: PrRequest) -> Result<Pr>;
    async fn comment(&self, item_id: &str, body: &str) -> Result<()>;
    async fn set_labels(&self, item_id: &str, labels: &[String]) -> Result<()>;
    async fn update_labels(
        &self,
        item_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<()>;
}

#[async_trait]
pub trait LabelForge: Send + Sync {
    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>>;
    async fn item(&self, item_id: &str) -> Result<Item>;
    async fn blocking_dependencies(&self, item_id: &str) -> Result<Vec<Dependency>>;
    async fn update_labels(
        &self,
        item_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<()>;
}
```

`query` passes `filter` through verbatim. The runner never parses it.

---

## 8. The reconcile loop

Replaces the scheduler entirely. There is no separate trigger subsystem.

```rust
loop {
    for a in &assignments {
        let desired  = forge.query(&a.work.source, &a.work.filter).await?;
        let observed = forge.open_prs(a.primary_repo(), &a.branch_prefix).await?;
        let inflight = leases.active(&a.name)?;

        let pending = subtract(desired, observed, inflight);

        let headroom = budget.headroom(a)?;   // 0 when any limit is hit
        for item in pending.into_iter().take(headroom) {
            if !leases.try_claim(&a.name, &item)? { continue; }   // CAS; may lose
            if dedup.seen(&item.content_hash())? {
                leases.release(&a.name, &item)?;
                continue;
            }
            tokio::spawn(engine.clone().run(a.clone(), item));   // teardown always releases
        }
    }

    tokio::select! {
        _ = wake.recv()                        => {}  // webhook or `runner reconcile --now`
        _ = sleep(jitter(cfg.interval))        => {}  // default 5m
    }
}
```

Properties this gives you for free, which you must not undermine:

- **Idempotent by construction.** A loop computing "what is missing" cannot
  double-submit. A cron job that says "do a pass" can.
- **Restart is free.** Kill it mid-run, restart, it re-observes and continues.
- **Multi-host works with no shared queue.** Two daemons on two machines reconciling
  the same forge arbitrate solely through lease CAS.

A panic inside one run must not abort the loop. `tokio::spawn` isolates the task;
join the handle and log the panic, then release the lease.

**Removing an assignment or role drains; it does not kill.** Stop claiming, let
in-flight runs finish. Standard GitOps prune semantics assume disposable resources;
yours are 20-minute jobs holding worktrees.

---

## 9. Trust grading

Label every input with its origin:

| Grade | Source |
|---|---|
| `trusted` | checked into the repo on the default branch |
| `maintainer` | a human with write access wrote it (work item body, PR comment) |
| `untrusted` | a bot, a build log, or an outside contributor produced it |
| `derived` | an agent produced it in an earlier step |

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust { Untrusted, Derived, Maintainer, Trusted }
```

Declaration order defines the ordering, so `min_trust` is a `>=` comparison and
"accepts `maintainer` and above" needs no lookup table. Confirm this ranking with me
before relying on it.

Each step declares the **minimum trust it accepts**. A step that writes code accepts
`Maintainer` and above. A step that reads CI failure output and patches a test accepts
`Untrusted` — that is its whole job — but it must be a **separate step with fewer
permissions**.

This is data-flow control. It is orthogonal to sandboxing, it is the correct answer to
prompt injection for this class of system, and it is nearly free if you build it into
layer 2 rather than bolting it on later.

---

## 10. Sandboxing and permissions

**The container is the sandbox boundary.** State that explicitly; do not build a
per-step sandbox in v0. In exchange, respect it:

- Do not mount the host home directory. Do not forward the host SSH agent.
- The container gets only the credentials the assignment needs, scoped per repo.
- A step that does not need to push never receives a token that can push.

The reference implementation defaults its sandbox to `disabled` while passing
`--allow-all-tools` to `copilot` and `--permission-mode bypassPermissions` to
`claude` — an agent with unrestricted shell on the host by default. Container
isolation plus per-step credential scoping gets the large majority of that value back
on day one.

Permissions are a flat list of strings checked before spawn. Keep it under 15:

```
repo:read  repo:write  repo:push
issues:read  issues:write
pr:read  pr:write  pr:review  pr:merge
runs:read
model:invoke
```

Each maps to a concrete credential that layer 0 will or will not inject. Where the
adapter can enforce it, mirror it in argv:
`--allow-tool='shell(git:*)' --deny-tool='shell(git push)'`.

Toolchain needs (`node@20`, `os=linux`) belong to the container image, not to this
list. Do not build host-capability matching (§3).

### Shell breadth — deliberately loose, for now

A role holding a write grant gets the **whole shell**, not a per-command allowlist.
`git push` remains denied without `repo:push`; denial beats any allow in both
adapters, so the push boundary survives the wider grant. A role without a write
grant is still denied shell outright — the tool grammar matches commands, not
effects, so a read-only shell is not expressible.

This is a deliberate, temporary stance, and the reasoning matters more than the
setting:

- An agent step is instructed to validate its own edit before publishing, and the
  deterministic step remains the authority. An agent that cannot run the repo's
  build or test command cannot follow that instruction: it publishes code it has
  never compiled, and `verify` then fails on errors the agent would have caught
  itself. That was observed, not predicted.
- The grantable unit is the command stem (`shell(cargo:*)` matches every `cargo`
  subcommand), so a "narrow" allowlist would still hand over a tool that runs
  arbitrary code through build scripts, while requiring every repository to
  enumerate its toolchain here — which is host-capability matching (§3) wearing a
  different hat.
- The blast radius is already implied. A write grant plus `--allow-all-paths` lets
  the step edit the worktree, and a later deterministic step executes what it
  wrote. Withholding the shell from the agent does not withhold execution from the
  run.

**The container is doing the work here.** This stance is defensible because of the
boundary stated at the top of this section, and it is exactly as strong as that
boundary is. Running a write-granted role outside a container gives the agent an
unrestricted shell as the invoking user — the same posture this document criticises
the reference implementation for defaulting to. Do not read this section as
endorsing that; read it as the container's rent.

Tighten this once there is evidence: after enough repositories have run, the set of
commands agent steps actually need will be visible in the run logs, and a narrower
default can be derived from that set rather than guessed at now.

**The successor is repo-declared, not bureau-declared.** A repository knows its own
toolchain; bureau does not, and teaching bureau to infer it is host-capability
matching (§3). So when an adapter's CLI can read tool permissions from a file in the
repository, bureau should defer to that file when it exists and fall back to the
loose grant when it does not. That keeps the declaration next to the code it
describes and under that repo's own review, and it costs bureau no new vocabulary.

Note as of this writing the mechanism only half exists. The `copilot` CLI's settings
carry `allowedUrls`, `deniedUrls`, and `trustedFolders` but **no** tool-permission
keys — tool approval is command-line only, so the adapter must pass it and there is
nothing to inherit. Adapter CLIs that do support a repository permissions file
should honour it before this section's default applies. Re-check this when adapter
CLIs change; it is the reason the loose grant is a stance and not a design.

---

## 11. Reference pipeline — build exactly this one

One pipeline, end to end, run for two weeks before a second one is written.

**"Fix a failing test."**

```
1  deterministic  claim next item (lease)                    → no-work exits clean
2  deterministic  create worktree, run failing test, capture output
3  agent          propose a patch      [trust: maintainer, perms: repo:write]
4  deterministic  apply patch, re-run the test
5  decision       passes?   yes → 6   no → 3 (max 3 attempts, then escalate)
6  agent          review the diff      [trust: derived, perms: repo:read]
7  decision       verdict?  pass → 8   needs-changes → 3   fail → escalate
8  deterministic  run the assignment's verify command
9  deterministic  push branch, open PR, release lease
```

**Every rule stated in an agent's instructions must have a matching machine check in a
deterministic step.** If the instructions say "keep files under 250 lines," a
deterministic step must fail the run when a file exceeds 250 lines. An instruction
without a paired check is a suggestion, and agents eventually ignore suggestions.

---

## 12. Scope of THIS session

Build layers 0–3 plus the config loader. Complete, with tests. Then stop.

- [ ] `Cargo.toml`, `rust-toolchain.toml`, module layout, `justfile`
- [ ] **Layer 0** process contract + tests (env isolation via a sentinel var, timeout
      kill, process-**group** kill with a backgrounded grandchild, secret scrubbing
      including a secret split across two writes, all four outcomes,
      missing-credential pre-spawn failure)
- [ ] **Layer 1** `fake` adapter + fixture format + `record` mode stub
- [ ] **Layer 2** `StepRequest`/`StepResult` types, serde round-trip tests, schema
      version constant + rejection of a wrong version, outcome semantics test
- [ ] **Layer 3** run log: append, fsync, sequence, scrub-on-write, and a replay test
      that deletes `state.json` and reconstructs identical state
- [ ] Config loader for `repos.yaml`, `roles/`, `assignments/`, with a
      `runner validate` command that reports every error in one pass (accumulate into
      a `Vec<ConfigError>`; do not bail on the first `?`)
- [ ] `cargo test` passes offline with no network and no model calls, in under 10s
- [ ] `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` both clean
- [ ] No `unsafe` outside the process-group call, and that block carries a comment
      justifying it
- [ ] Commit `DESIGN.md` (this brief, verbatim) and a short `AGENTS.md` pointing at it

Do **not** write: the engine, the reconcile loop, git worktrees, forge clients, or a
CLI beyond `validate` and `version`.

---

## 13. Definition of done for v0 (later sessions, for context only)

- [ ] `runner run fix-failing-test --item <id>` completes §11 against a real repo
- [ ] The same pipeline runs end to end under `fake`, offline, in CI, in under 10s
- [ ] Killing the daemon mid-run and restarting resumes from `events.jsonl`
- [ ] Two daemons started concurrently cannot claim the same item
- [ ] Exceeding an hourly limit blocks the run before any subprocess spawns
- [ ] A step missing a required credential fails before spawn with a clear message
- [ ] No secret appears anywhere under `runs/`
- [ ] Re-reconciling an unchanged item produces no new run
- [ ] Unplugging webhooks changes latency and nothing else

---

## 14. How to work

1. **Do not skip ahead.** If you think a layer is unnecessary, argue it — do not
   silently reorder.
2. **Section 2 is enforced.** Introducing a term outside the table, that is not an
   ordinary industry word, requires stopping and asking.
3. **Section 3 is hard.** If you are building something on that list, the spec is
   wrong; say so.
4. **Every layer ships with offline tests.** No exceptions above layer 1.
5. **No README, no ARCHITECTURE.md, no docs/** until v0 is done. `DESIGN.md` and
   `AGENTS.md` only. Comments explain *why*, never *what*.
6. **Do not add configuration fields** beyond §6 without asking.
7. **Dependencies.** Prefer `std`. Each crate below is pre-approved; anything else
   requires asking first.

   | Crate | For | Notes |
   |---|---|---|
   | `tokio` | process supervision, timeouts, concurrency | features: `rt-multi-thread`, `process`, `time`, `io-util`, `sync`, `macros`. Justified because layer 0 needs concurrent stdout/stderr drain plus a timeout, and §8 needs concurrent runs. |
   | `serde`, `serde_json` | step contract, run log | |
   | a maintained YAML crate | config loading | **`serde_yaml` was archived in 2024 — do not use it.** Pick a maintained fork, tell me which, and pin it. |
   | `rusqlite` | layer 5 only | `bundled` feature; not needed this session |
   | `nix` *or* `libc` | process groups / signals | pick one, not both |
   | `thiserror` | typed errors in library code | |
   | `anyhow` | error context at the binary boundary only | not in library signatures |
   | `async-trait` | `dyn Forge` | layer 7 only |
   | `reqwest` | forge clients | layer 7 only; `rustls-tls`, not native-tls |
   | `clap` | CLI | `derive` feature |

   Not approved: a DI framework, a logging framework, a workflow/state-machine crate,
   `git2`, or a proc-macro crate of your own.

**Start by writing, before any implementation:**

1. The repository layout (crate structure: single binary crate, or a small workspace —
   argue which)
2. Exact Rust types for layers 0 and 2, including the `Secret` newtype and its
   `Debug`/`Drop`
3. The test list for layer 0
4. Anything in this brief you think is wrong, and why

Show me those four things and wait for my response before implementing.

---

## 15. Approved next phase

Sections 12 and 13 record the original build sequence. All layers named there
are complete. This section is the authoritative scope for the next phase and
supersedes earlier examples where they conflict.

### 15.1 Plugins and AI resources

The repository publishes one installable plugin from the standard Copilot
marketplace layout:

```text
.github/plugin/marketplace.json
plugins/bureau/
  plugin.json
  agents/implementer.agent.md
  agents/reviewer.agent.md
  skills/pipeline-author/SKILL.md
  skills/run-inspector/SKILL.md
  .mcp.json
```

The public resources are `/bureau:implementer`, `/bureau:reviewer`,
`/bureau:pipeline-author`, and `/bureau:run-inspector`. Delete the `atomic`
plugin and its analyzer. The implementer uses Opus; the reviewer uses Sonnet.
Agent frontmatter owns model and tools.

The plugin also exposes a `bureau-io` MCP server by invoking hidden
`bureau mcp serve`. Its initial tools are:

- `get_step_context` — read the current immutable request;
- `publish_result` — publish one final result.

Publication is one-shot. Artifact paths must stay inside the step worktree;
copy and hash them into the durable run directory. The MCP process writes no
run-log, lease, budget, or forge state. The engine validates before append.

Copilot and Claude both receive these tools. A real agent step succeeds only
after MCP publication or valid versioned `StepResult` JSON on stdout. Exit zero
plus prose is failure. Remove agent-controlled `cost_usd` from `StepResult` and
advance the wire schema to `v2`. Adapters own usage/cost measurement.

### 15.2 Plugin resolution

Resolve role plugins in this order:

1. target-repository enabled local marketplaces;
2. enabled user-global plugins under `COPILOT_HOME`;
3. `plugins/bureau` only when running from a bureau source checkout.

Target-repository plugin code intentionally may override the role's plugin
implementation. Reviewed config still controls adapter, credential grants,
trust, and limits.

For a run, activate the chosen plugin through the alphabetically first existing
local-directory marketplace. When none exists, create a temporary `.ai`
marketplace named `repo-plugins` and add it to repository Copilot settings.
Save exact originals and restore them before change detection on every exit
path. A conflicting agent edit to activation files escalates. Record plugin
source, version, and digest in the run log.

Never auto-install during a run. A missing plugin fails before spawn with
installation and retry actions.

### 15.3 Reconciliation and committed config

Add:

```text
bureau reconcile
bureau reconcile --interval <duration>
bureau reconcile --now
```

Continuous mode polls. `--now` performs one pass, starts eligible work up to
headroom, waits for those runs, prints outcomes, and exits. Keep `bureau run`
for explicit one-item work.

Before every pass:

1. fetch the configured committed ref;
2. validate the complete config snapshot;
3. adopt it atomically or retain last-known-good;
4. resume unfinished runs;
5. only then observe and claim new work.

Single-repository reconcile reads committed `.bureau/` from the configured
work-repo remote/ref. Multi-repository reconcile reads the separate config
remote/ref. Local config paths are caches only. `bureau validate` may inspect
uncommitted local authoring changes. Committed config loading rejects symlinks
and any path that escapes the exact detached snapshot.

### 15.4 Long-running correctness

Renew every active lease. Losing ownership stops the run. The default complete
run deadline is 24 hours; an assignment may set a different positive
`max_run_hours`. Agent steps without `timeout_secs` inherit the remaining run
deadline. Deterministic steps keep their short default.

Each lease carries both the durable run ID and a unique owner token for the
current supervisor process. Run IDs and owner tokens include operating-system
random entropy so independent hosts cannot collide. Recovery may replace an
owner token only after expiry.
Event appends, commits, pushes, comments, and PR creation fail closed unless the
current token still owns the live lease.

Recheck `approval_label` at every step boundary and before publication. At
deadline or approval revocation, preserve evidence, restore temporary plugin
activation, tear down, release the lease, comment with the exact retry action,
and escalate.

The run log must persist enough to resume the same run ID:

- immutable plan, source commit, item snapshot, pinned direct-agent bytes, and
  SHA-256 plugin digests covering paths, bytes, and file permissions;
- full scrubbed step and concurrent-member results;
- adapter usage;
- internal Git snapshot SHAs;
- pushed commit and created/adopted PR identity;
- terminal outcome, cost, and dedup disposition.

Finalization is idempotent. Before PR creation, observe an exact-head-branch PR
and adopt it. After ambiguous create failure, observe once more. Project
terminal budget/dedup state from the log with unique `run_id` accounting so a
crash cannot double count. Release is idempotent.

First SIGINT/SIGTERM drains while renewing leases. A second signal cancels
process groups, records cancellation, cleans, releases, and exits. Lease loss
uses the same hard process-group cancellation path; detached descendants do not
survive a completed or cancelled step.

### 15.5 Concurrent evidence

At concurrent-group entry, make an internal commit from a separate temporary
index containing the current tracked and untracked non-ignored bytes, excluding
temporary plugin activation. Do not move the branch or real index. Give each
member a detached worktree at that SHA.

Durable events distinguish group start, member start/finish/cancel, and group
finish. Resume skips finished members, reruns interrupted attempts, and starts
pending members under the group limit. Persist the aggregate result under the
group name for `inputs_from`.

Assignment `max_concurrent` limits active work items. Concurrent-group
`max_concurrent` limits active members and defaults to every listed member.
There is no role concurrency.

### 15.6 Local lifecycle

Local state defaults to `~/.bureau` and `BUREAU_HOME` overrides it:

```text
settings.yaml
credentials/
state.db
runs/
checkout-cache/
config-cache/
```

Add four explicit commands:

- `init` — first-time setup only;
- `setup` — change local source/plugin/credential/migration settings;
- `doctor` — read-only human/JSON/offline diagnostics;
- `repair` — confirmed reversible repairs only.

`init` supports single- and multi-repository config, repository registration,
three credential sources, optional user-global plugin installation, and a
fixed or AI-authored first pipeline. It previews and validates config, creates
a config PR, waits for merge, validates the merged commit, runs one reconcile
pass, and waits for its outcomes. It never runs unmerged config.

`doctor` checks local state, config, repos, credentials, adapters, plugins/MCP,
and recovery state. `repair` may restore directories/permissions, disposable
caches, the same plugin version, stale activation, expired ownership, orphan
worktrees, and derived state. It never changes reviewed policy, credentials,
plugin versions, live work, or durable history.

All interactive flows are deterministic state machines over injected effects
and have complete offline transcript tests.

### 15.7 CLI cap and deferrals

Top-level commands after this phase:

```text
validate version run list show cancel retry fake
reconcile init setup doctor repair mcp pause resume
```

Seventeen total, `pause` and `resume` included. `mcp` is hidden from normal
help. Add no other top-level verb.

`pause <run-id>` is a control verb: it writes the run's PAUSE marker, blocking
step transitions at the next step boundary. `resume <run-id>` clears it;
continuation happens through `bureau run` re-entry or the reconcile loop, not
from `resume` itself. This supersedes the mid-run human pause deferral below.

Explicitly defer output-specific gate types, cron/signals, webhook listener,
scratch/sparse workspaces, timeout salvage,
per-step cost caps, and log-derived limit sets. The section 3 non-goals remain
hard.

### 15.8 Delivery order

Deliver in seven tested stages:

1. spec/config cutover and plugin package;
2. MCP and strict agent contract;
3. long-run durability and exactly-once recovery;
4. concurrent evidence groups;
5. reconcile, committed sources, and plugin activation;
6. init/setup/doctor/repair;
7. full offline acceptance.

Each stage passes the four repository gates before the next begins. Every Rust
stage also receives a small-crate modularity review.
