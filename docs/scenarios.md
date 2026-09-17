# Bureau scenario setups

These ten setups adapt useful patterns from other orchestrators to Bureau's
existing roles, assignments, label rules, and pipelines. There is no new
language, compiler, scheduler, or execution engine. The YAML you review is
the YAML Bureau loads.

Use examples from the same tag or commit as your Bureau binary; check
`bureau --version`. The small
[`catalog.json`](../examples/scenarios/catalog.json) is a documentation index,
not executable configuration. Its `id`, relative `path`, `docs_anchor`,
`execution`, and `readiness` fields let other surfaces link to these setups
without copying their configuration.

## Choose a first scenario

Every directory below passes Bureau's real config loader. That proves shape
and references, not credentials, adapter compatibility, test-tool availability,
model quality, runtime qualification, or cloud entitlement.

| Setup | Purpose | Additional prerequisite |
|---|---|---|
| [Design review](#design-review) | Propose a review document, not an implementation | Python 3 |
| [Issue intake](#issue-intake) | Dependency-based label graduation, no model | GitHub issue-label access |
| [Issue triage](#issue-triage) | Read-only diagnosis for a human | Python 3 |
| [Customer feedback](#customer-feedback) | Assessment, human bug triage, separately approved fix | Python 3; Rust tests in the sample fix |
| [Failing-test repair](#failing-test-repair) | Reproduce, repair with a limit, verify | Python 3; Rust tests |
| [Multi-repo fix](#multi-repo-fix) | Fix the primary using a pinned public contract | Reviewed secondary commit; Python 3; Rust tests |
| [Azure DevOps](#azure-devops) | Approved WIQL-selected bugs | ADO PAT; provisioned .NET dependencies |
| [Local SDK factory](#local-sdk-factory) | Qualified local factory review | Deliberately blocked until qualification |
| [Cloud automation](#cloud-automation) | Inspect and explicitly submit an existing automation | Experimental service eligibility; inspection-only default |
| [Recurring maintenance](#recurring-maintenance) | Maintain desired state with concurrent evidence | Rust toolchain; open approved forge work |

The runtime prerequisites are still Linux, git, `unshare`, an isolated
container, and a compatible authenticated adapter for ordinary agent steps.
Keep state on a native Linux filesystem. Do not mount a host home or forward
a host SSH agent. Provision tools and dependency caches before running;
Bureau does not infer a toolchain or install missing dependencies during a run.
Replace every sample test command with the repository's real deterministic
check, including every repeated occurrence in the pipeline and assignment.

## Try the zero-cost offline example

From a source checkout, with the ordinary Rust build prerequisites and
dependencies already cached:

```sh
cargo test --offline -p bureau --test engine \
  scenario_offline::design_review_demo_runs_offline_end_to_end -- --exact
```

This is a complete deterministic engine run, not just YAML validation.
It loads the design-review setup, replaces only the external effects with
the existing fake adapter and in-memory forge, creates a temporary local git
repository, renders and verifies the review, pushes to that local repository,
and records a fake PR. The test checks the exact step sequence and zero
measured cost. It uses no real credentials, model, or forge network call and
cleans up its temporary state. It does not test a live adapter's permissions,
model quality, cloud eligibility, or local-factory qualification.

If you only have the installed binary and the example files:

```sh
bureau fake replay examples/scenarios/design-review/fixtures/analyze.json
```

That second command is only a transcript replay. Its explicitly fake result
is useful for learning the v2 output contract, but it is not the full engine
demonstration and is not evidence that any real service ran.

## Author, validate, review, then run

First inspect `bureau init --print-template`. Saving its stdout writes only
the file you choose; printing itself does not initialize Bureau. Fill in
the request and follow [first-time setup](getting-started.md#first-time-setup-bureau-init).
`init --from` may install a requested plugin, creates a config PR, waits for
merge, and runs one pass against the validated merged commit.

These scenario directories are **config trees, not init requests**. Do not
pass their `repos.yaml` or catalog to `init --from`.

For a first installation, either finish the reference initialization with
no eligible approved work and then propose a scenario config change, or
replace the draft files in the initial config PR while `init` waits for its
review. In the latter case, include all credential references needed by the
final setup in the initial local request. Keep that flow running through
merge: restarting `init` reconstructs its original draft, not your later
manual edits. Inspect existing PRs and committed configuration before
restarting an interrupted, manually revised initialization.

Copy **one whole setup tree**, including `agents/` when present, into a new
authoring checkout: `.bureau/` for a single work repository, or the root of
a separate config repository. Do not overwrite an existing configuration
blindly. In the config PR, deliberately remove or retain prior assignments;
adding a sample alongside them does not disable their work.

Customize the repository URLs, work sources, native queries, approval and
terminal labels, test commands, branch prefixes, and budgets. Keep queries
unmatched until intended activation. With label rules, withholding an
assignment approval label is insufficient: their reviewed filter directly
authorizes their bounded label changes.

```sh
bureau validate /path/to/config-checkout --json
```

For single-repository mode, pass `/path/to/work-repo/.bureau` instead.
Authoring and validation neither approve nor execute work. Review and merge
the exact files into the configured remote/ref before execution.

[`settings.yaml.example`](../examples/scenarios/settings.yaml.example) shows
the existing local credential-source schema. Copy it outside the repository,
retain only the references you use, and customize the source. `config` names
access to the config repository; `github-work`, `ado-work`, and
`github-context-read` name separately intended work-repository credentials.
Credential values never belong in any example or config PR. A local factory
additionally needs the explicitly model-authorized `copilot-model` reference.

For an **already initialized** installation that needs a source or credential
change:

```sh
bureau setup --from /path/to/private/settings.yaml
bureau doctor --json
```

`setup` refuses a missing initial installation. It is not a shortcut around
`init` or config review. Its optional plugin installation is an explicit
local effect; the sample settings leave it off. Roles referring to
`/bureau:implementer` or `/bureau:reviewer` need the installed Bureau plugin;
scenario-specific direct agents are included in their own trees. Runs do
not auto-install missing resources.

After the intended config PR is merged and the local prerequisites are ready:

```sh
bureau reconcile --now
bureau list
bureau show RUN_ID --json
```

`--now` can start paid local work and waits for its outcomes. It is not a
preview. For label-only setups it may mutate labels; the cloud setup has no
assignments or label rules and cannot dispatch a cloud automation this way.

## Shared safety and operating contract

Every pipeline assignment has an explicit approval label, distinct abort
and escalation labels, and all six assignment limits. Limits apply per
assignment, not globally; combining setups can add their spending ceilings.
Missing measured model cost fails a configured daily cost gate rather than
being treated as zero.

Approval is separate from eligibility and model output. A maintainer reviews
the item before applying its approval label. Bureau checks approval before
claiming, at step boundaries, and before publication; removal blocks work
and requires explicit retry. Deterministic validation of model output does
not promote its trust. Agent results remain `derived`; deterministic results
retain their request's trust floor.

Writing agents receive local read/edit and model grants, not push, issue-write,
PR-write, or merge grants. Review and assessment roles have no edit grant.
The engine performs its own lease-fenced publication and terminal labeling
using the registered credentials. Grant names do not attenuate an overbroad
token: independently scope the actual credentials and retain both isolation
boundaries.

Every outcome has an explicit route except concurrent members, which route
only through their group. A writing agent's `no-work` still goes to machine
checks. Repeated failures are bounded with `max_attempts`; reaching the limit
escalates. `blocked` is a human/prerequisite stop, not permission to retry a
model indefinitely. Terminal labels are excluded from the example work
queries to avoid repeatedly claiming unresolved failures.

Inspect the run and fix its cause before using `bureau retry RUN_ID`.
Do not clear terminal labels as an unattended requeue mechanism. A local
factory has stricter same-run recovery rules described below; `retry` must
not replace an indeterminate factory run.

## Design review

Source: [`design-review/`](../examples/scenarios/design-review).

Select approved `design-review` issues whose bodies describe a concrete
proposal. A read-only agent supplies structured alternatives, risks, and
evidence. Code checks their shape and writes only
`docs/reviews/design-review.json`; a second read-only role reviews the
proposal, then deterministic verification restricts the final diff to that
report and checks its JSON.

The verifier compares against the pre-agent Git commit, not just the
currently clean working tree: Bureau checkpoints between steps. It therefore
catches code edits that were already checkpointed. A structural check is not
proof that a recommendation is correct. The output is a proposed review
document in a PR; a human still accepts, revises, or rejects the design.
No implementation is authorized by a model's `accept` recommendation.

Customize the report destination in both assignment and pipeline if the
repository has an established design-record location. Initial `no-work`
must still include the complete existing report; it is checked like any
other result. An unchanged checked document produces no new proposal.

## Issue intake

Source: [`issue-intake/`](../examples/scenarios/issue-intake).

A human selects intake work with `agent-blocked` and records blocking
relationships through GitHub's dependency API. On each reconcile pass,
`graduate-unblocked` changes only `agent-blocked` to `agent-eligible` once
every dependency is closed. **An empty dependency set also satisfies this
condition.** It is not an AI classifier or a guarantee that the issue is
ready to implement.

There are no roles, assignments, pipelines, model calls, worktrees, or PRs.
The registry's `access: read` concerns repository content; the declared
credential still needs issue-label mutation access for this reviewed rule.
The rule never adds an approval label. Pairing it with a fix assignment
still requires a separate maintainer approval.

The hourly limit counts attempted mutations, including failures. Audit
events record started/applied/failed updates. Open dependencies are revisited
on later passes; interrupted label deltas remain recoverable by item identity.
This setup requires GitHub: do not translate it into invented ADO dependency
semantics.

## Issue triage

Source: [`issue-triage/`](../examples/scenarios/issue-triage).

Use `needs-triage` plus the separate `bureau:triage-approved` admission label.
The read-only triager classifies the item as `bug`, `duplicate`,
`needs-information`, or `not-a-bug`, with a summary and evidence. It can
accept low-trust issue/log content without receiving edit or forge-write grants.

Code verifies the repository is unchanged from the pre-agent baseline and
checks the output contract. A completed assessment deliberately becomes
`blocked`/`escalate`, preserving the diagnosis in the run log and the
engine's human-attention comment. It does not label, close, approve, or fix
the issue itself. A human performs that triage in the forge.

The loader currently requires a publication-capable primary for every
assignment, so this registry uses `push` even though this pipeline never
routes to publication. That registry prerequisite is not an agent push grant.

## Customer feedback

Source: [`customer-feedback/`](../examples/scenarios/customer-feedback).

This tree contains two distinct assignments with separate limits and approval:
`assess-feedback` selects `customer-feedback` plus
`bureau:triage-approved`; `fix-feedback-bug` selects `feedback-bug` and
`agent-eligible` plus **`bureau:fix-approved`**.

The first assignment reads feedback without editing, checks its classification
and reproduction description, verifies an unchanged repository, and escalates.
Its output is evidence, not a newly trusted work item or automatic bug creation.
A human decides whether it is a bug, authors or revises the issue to state
the intended fix and regression test, removes it from the feedback-assessment
filter, and separately approves implementation.

Only the second assignment edits code. Tests run before and after read-only
review; even a writing agent's `no-work` must pass them. The sample uses
`cargo test --offline`; replace it everywhere for another toolchain.
Neither a `bug` classification nor the triage approval label can admit the fix.

## Failing-test repair

Source: [`failing-test-repair/`](../examples/scenarios/failing-test-repair).

Select approved `failing-test` issues. The deterministic reproduction command
runs the tests before invoking a model. Passing tests produce `no-work`.
A failing command supplies its exit code, command, and bounded output to a
local-edit-only repairer; a truncation field identifies shortened output.
Missing executables fail explicitly rather than masquerading as a reproduced
test failure.

After the first proposal, verification can route to at most two repair
entries. Each repair receives the latest captured verification evidence.
The verifier has three entries available; the complete-run deadline and
assignment budget remain additional ceilings. Read-only review and a final
real test command are required before `done`.

Replace all three forms of the test command together: the reproduction
argument array, the capture-verification argument array, and the final
command/assignment `verify`. Prepare dependencies in advance. Passing tests
alone cannot prove that an agent did not weaken a regression test; human
diff review remains necessary.

## Multi-repo fix

Source: [`multi-repo-fix/`](../examples/scenarios/multi-repo-fix).

The first registered repository is the only publication target. `contracts`
is independently registered `read`. Registry entries do **not** automatically
materialize secondary checkouts in the current engine.

This concrete setup instead reads one file from an explicitly pinned
**public** secondary repository. Customize the static repository URL,
`api/contract.json` path, and `REPLACE_WITH_REVIEWED_CONTEXT_COMMIT` in
`read-context`; keep the registry URL consistent. The placeholder fails
before any fetch. A valid reviewed 40-digit commit is fetched anonymously
with credential helpers and prompting disabled, its exact identity checked,
and the file bounded to 64 KiB before reading. A temporary bare repository
is removed before the agent receives the provenance and text in its inputs.

No model/work-item input chooses a URL, revision, or path. The agent edits
the primary against that snapshot, then tests and read-only review check
the proposal. There is no fallback to the latest secondary branch or a
primary write token. The named read credential does not get passed to this
deterministic fetch. Private-context transport is not implemented by this
example; do not put credentials in its URL or replace a failure with an
unreviewed host checkout.

## Azure DevOps

Source: [`azure-devops/`](../examples/scenarios/azure-devops).

Replace the organization, project, repository, and WIQL predicate. `source`
is `ExampleProject/example-repo`; explicit item IDs use `ExampleProject/12345`.
The filter is a native WIQL `WHERE` fragment, not a Bureau expression.
Provision the `ado-work` PAT for the intended Code and Work Items operations.

ADO items begin `untrusted`. The separate `agent-approved` tag is required
for the maintainer-trust implementer, and removing it stops further work.
The sample proposes a .NET change, runs `dotnet test --no-restore`, requests
read-only review, and repeats verification before publishing.
Restore the appropriate dependency cache before execution.

The config repository can remain on GitHub: its `config` credential and
`settings.config.remote` are independent of the ADO work source and PAT.
No GitHub-only label rule is included.

## Local SDK factory

Source: [`local-sdk-factory/`](../examples/scenarios/local-sdk-factory).

**The shipped runnable pipeline is deliberately blocked.** Its deterministic
`require-qualification` step reports what is missing and escalates; it does
not call a factory, substitute ACP, or claim a successful demonstration.
No runtime/provider hashes or generally available release mapping are invented.

The inactive [`provider/`](../examples/scenarios/local-sdk-factory/provider)
directory contains a concrete standard SDK provider: `extension.mjs` imports
`defineFactory` and `joinSession` through the stock SDK resolver, loads the
same `factory.json` metadata Bureau validates, and registers `review`.
It is **not** in an autodiscovered extension directory and is not enabled
by copying or validating the scenario config tree.

Its implementation makes at most two logical `ctx.agent` calls: a read-only
assessment using `bureau-io.get_step_context`, followed by independent
verification. The SDK may retry each structured response once, so these
two calls can consume four direct admissions. The code has no delegation
loop or nested factory, asks for no sensitive environment or extra tools,
checks null/invalid child responses and data-size bounds, propagates hard
SDK errors and cancellation, and returns a complete Derived v2 result.
A missing prerequisite returns `blocked`; unsupported claims or malformed
responses return `failure`. A durable `ctx.step` records the final result.
Child publication never substitutes for the factory return.

The provider metadata intentionally declares no guessed native limits.
The `.example` declaration's invocation ceilings are editable policy
examples, not cost estimates; choose explicit approved values after
qualification, including the native accounting of descendants.

Check its pure control flow without importing the SDK or registering a provider:

```sh
node --test examples/scenarios/local-sdk-factory/fixtures/provider.test.mjs
cargo test --offline -p bureau --test scenario_catalog prerequisites::
```

The Node tests use stub contexts and recorded child values only. The Rust
tests additionally validate the actual metadata with Bureau's complete
argument-schema validator and parse the provider's stubbed return through
the real v2 result decoder. Neither proves live eligibility, runtime
conformance, accounting, or child-context propagation.

The adjacent `qualified-pipeline.yaml.example` is an incomplete ordinary
pipeline declaration, not loaded while it has the `.example` suffix.
Its placeholder digests intentionally fail validation if adopted unchanged.
Before replacing the guard in a reviewed config PR, an operator must provide:

- An actually eligible account and model credential; offline tests, protocol
  version 3, `--experimental`, or an installed CLI do not establish eligibility.
- A reviewed, self-contained runtime/SDK bundle implementing Bureau's
  `copilot-sdk-factory-v1` contract and its exact expected `connect.version`.
- The actual `project:review` provider with standard SDK
  `defineFactory`/`joinSession` registration and pinned `factory.json` metadata.
- Independent canonical provider and runtime tree digests, calculated with
  `bureau_plugin::tree_digest`'s path/byte/mode algorithm, not an archive hash.
- Arguments satisfying the metadata's full `argsSchema`, the separately
  declared `copilot-model` source, and explicitly reviewed native ceilings.

Only after that review, copy the provider's three files into
`.github/extensions/review/` in the managed work repository, review them
there, and obtain the canonical digest of the final bytes and file modes.
Do not run a standalone wrapper, auto-install another SDK, reload active
extensions as an onboarding shortcut, or invent a pin from a version label.
The qualified runtime must launch this provider through its stock bootstrap.

There is no one-command qualification shortcut in this guide. If any item is
unavailable, keep the guard and do not approve a live factory run. The example
requires the factory itself to return a complete v2 result with a nonempty
`outputs.review` string; child publications and previews do not finish it.
Deterministic checks then escalate that review for a human.

Provider initialization and factory JavaScript are trusted executable host
code, not sandboxed by child tool grants. Committed config is the admission
authority for direct SDK calls. Extra permissions and ambient plugins are
not automatically approved. Model authorization does not add forge grants.
Native Git/gh credential injection remains disabled under the qualified policy.

Preserve original worktree, pins, native identity, and opaque SDK state on
interruption. A lost start acknowledgement is indeterminate, not retryable
permission to start again. Inspect `bureau show RUN_ID --json` for Bureau's
continuation decision. Native ceilings persist across eligible same-run
resume; unknown accounting is not zero. See
[the full local-factory contract](getting-started.md#opt-in-local-copilot-factories).

## Cloud automation

Source: [`cloud-automation/`](../examples/scenarios/cloud-automation).

This tree contains **only a read-only registry**. It has no pipeline,
assignment, label rule, or daemon-dispatch behavior. Select an automation
that already exists in the registered repository:

```sh
bureau list --github-cloud --repo code --expected-login YOUR_LOGIN --json
bureau show --github-cloud --repo code --expected-login YOUR_LOGIN \
  --automation AUTOMATION_ID --json
```

These experimental controls use internal CMC endpoints, not a supported
public factory REST API. No live entitlement is claimed. Authentication or
eligibility failures stop the operation, with no alternate account, token,
authentication scheme, or first-party identity fallback.

Inspection does not authorize dispatch. After separately reviewing the
automation's environment, tools, permissions, billing, and trigger eligibility,
a human must approve a config change from registry `read` to `push` before
explicit submission. Only enabled automations with the supported manual or
single interval/schedule trigger shape can be submitted:

```sh
bureau run --github-cloud --repo code --expected-login YOUR_LOGIN \
  --dispatch-automation AUTOMATION_ID --request-id review-request-42 --json
```

This can start paid remote work. Acceptance does not identify a task or mean
success. Preserve the local request key; uncertainty is not a reason to
invent a new key and submit again. Inspect exact task IDs and attribute one
explicitly, never by "latest task":

```sh
bureau list --github-cloud --repo code --expected-login YOUR_LOGIN \
  --automation AUTOMATION_ID --json
bureau run --github-cloud --repo code --expected-login YOUR_LOGIN \
  --automation AUTOMATION_ID --track-task TASK_ID \
  --request-id review-request-42 --json
bureau show --github-cloud review-request-42 --refresh --json
```

Tracking is an operator-selected association, not proof of which submission
created the task. Cloud execution does not inherit local pipeline budgets,
permissions, worktrees, deadlines, or cancellation. Unknown cost remains
unknown. Remote pause, cancel, resume, retry, approval, and feedback are
unsupported here. See [cloud controls](github-cloud-factories.md) before
attempting any submission.

## Recurring maintenance

Source: [`recurring-maintenance/`](../examples/scenarios/recurring-maintenance).

Represent a concrete desired maintenance change as an open forge issue with
`maintenance`, `agent-eligible`, and separate maintainer approval. Examples
include a reviewed dependency update or removing a deprecated API. State the
acceptance check in the item; "do maintenance" is not a bounded request.

The implementer proposes the change. A concurrent evidence group runs
format checks, tests, and read-only review on identical detached snapshots,
with at most two active members. Member edits are discarded; any failure
or blocked member prevents publication and escalates. The assignment limits
active work items independently of that evidence-group limit.

```sh
bureau reconcile --interval 5m
```

The interval controls **observation**, not a schedule that creates tasks.
Each pass re-queries current forge state and uses the existing leases,
open-PR observations, deduplication, and budgets. Resolve the issue when its
desired change lands; a human or an already authorized forge process can
create/revise the next concrete maintenance item when intent changes.
There is no cron DSL, timestamp trigger, queue table, or automatic cloud
submission in this setup.
