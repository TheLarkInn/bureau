# Getting started with bureau

bureau is a local agent work runner: a single binary that continuously
compares desired state ("every work item matching this filter should have an
open PR") with observed state (what the forge actually shows) and closes the
gap by running agent-driven pipelines in git worktrees. Work is claimed off
the work source by lease, never pushed; every run is recorded in an
append-only run log you can replay.

This guide takes you from zero to a running reconcile loop, for both
configuration layouts (config inside the work repository, or in a separate
config repository) and both forges (GitHub and Azure DevOps).

## Prerequisites

- A Linux environment (a dev container is the intended sandbox boundary).
- `git` and `unshare` on `PATH`. Keep local state and worktrees on a Linux
  filesystem, not a Windows-mounted drive.
- Node.js on `PATH` when using the optional browser dashboard.
- An ACP agent for agent steps: GitHub Copilot CLI (`copilot --acp --stdio`)
  or the public Claude adapter (`claude-agent-acp`, installed with
  `npm install --global @agentclientprotocol/claude-agent-acp@0.75.1`).
  Both must advertise the role's custom agent through session config options.
  You can also try everything offline with the `fake`
  adapter — see [Try it offline first](#try-it-offline-first).
- A token for each forge you use:
  - **GitHub**: a token with repo and issues/PR access to the repositories
    involved (a PAT or a GitHub App token).
  - **Azure DevOps**: a PAT with Code (read/write) and Work Items
    (read/write) scopes for the organization.

For optional, experimental, limited-availability controls over existing
GitHub cloud automations, see [GitHub cloud factories](github-cloud-factories.md).
They do not run as Bureau pipeline steps or use a supported public REST
factory API. Ordinary GitHub token support above does not establish
entitlement to the internal cloud API.

## Install

From a source checkout:

```sh
cargo install --path crates/bureau
bureau --version
```

Local state lives in `~/.bureau` (set `BUREAU_HOME` to move it):

```text
~/.bureau/
  settings.yaml      # non-secret local settings, written by init/setup
  credentials/       # credential values, never in git
  state.db           # leases, budget counters, dedup markers
  runs/              # one directory per run (the run logs)
  checkout-cache/    # bare mirrors, keyed by remote URL hash
  config-cache/      # disposable cache of the committed config
```

## The five config concepts

All reviewed configuration is five small YAML shapes:

| File | What it declares |
|---|---|
| `repos.yaml` | The repo registry: every repository bureau may touch, with a per-repo access level (`read`/`pr`/`push`) and a credential *reference*. |
| `roles/<name>.yaml` | An agent reference (`/bureau:implementer` or a path to an agent `.md`), the adapter that runs it (`copilot`/`claude`/`fake`), credential-grant permissions, and a minimum input trust. |
| `assignments/<name>.yaml` | The standing arrangement: which work source to watch, which repos to touch, which pipeline and role to use, the `verify` command, the branch prefix, and budget limits. |
| `label_rules/<name>.yaml` | A bounded rule that updates forge labels when every blocking GitHub issue is closed. |
| `pipelines/<name>.yaml` | The step state machine: `deterministic` steps run shell, `agent` steps run an adapter, `decision` steps branch on an earlier step's outcome. |

Where these files live depends on your layout — that is the next decision.

## Pick your layout

| | Single-repository | Separate config repository |
|---|---|---|
| Config lives in | the work repository, under `.bureau/` | its own repository, at the root |
| Use when | one work repository | several work repositories, or repos you cannot commit config to |
| Authorization | PR review of the work repo | PR review of the config repo |
| settings.yaml `config.kind` | `single_repository` | `separate_repository` |

The file schemas are identical; only the location differs:

```text
my-repo/.bureau/                 # single-repository mode
  repos.yaml
  roles/implementer.yaml
  roles/reviewer.yaml
  assignments/fix-flaky-tests.yaml
  label_rules/graduate-unblocked.yaml
  pipelines/fix-failing-test.yaml

runner-config/                   # separate-repository mode, same schema at the root
  repos.yaml
  roles/...
  assignments/...
  label_rules/...
  pipelines/...
```

A local checkout of config is always a disposable cache. Reconcile reads the
*committed* config from the configured remote/ref, validates the complete
snapshot, and adopts it atomically (retaining last-known-good on failure).
`bureau validate` is the exception: it inspects uncommitted local files so
you can check authoring changes before opening the config PR.

## Credentials

Config names a *reference* (`credential: github-main`); the value is never in
git. `settings.yaml` declares where each reference resolves — one of three
sources per reference:

```yaml
credentials:
  github-main:
    source: environment        # read the value from one environment variable
    variable: GH_TOKEN
  ado-main:
    source: file               # read the value from one exact file
    path: /run/secrets/ado-pat
  shared:
    source: directory          # read credentials/shared from this directory
    path: /home/me/.bureau/credentials
```

Values are injected into step environments scoped by the role's permissions
and are scrubbed from everything written to the run log. A step missing a
required credential fails *before* spawn, naming the reference.

### No token in git, no clicking

The reference lives in `settings.yaml` under `BUREAU_HOME` (`~/.bureau`), which
is **not** your repo — so the credential configuration is never committed. The
token value likewise never touches the repo. To reuse the GitHub CLI token you
already have (one `gh auth login` device flow, once per machine), point the
reference at it instead of hand-making a PAT:

```yaml
credentials:
  github-main:
    source: file
    path: /home/you/.config/bureau/github-main
```

```sh
mkdir -p ~/.config/bureau
gh auth token > ~/.config/bureau/github-main   # one-time, non-interactive
chmod 600 ~/.config/bureau/github-main
```

Or skip the file entirely and read it straight from the environment:

```yaml
credentials:
  github-main:
    source: environment
    variable: GH_TOKEN                  # export GH_TOKEN="$(gh auth token)"
```

## First-time setup: `bureau init`

`init` is driven by one YAML file. It previews and validates the config it
generates, opens a config PR, waits for you to merge it, validates the exact
merged commit, runs one foreground reconcile pass, and only then marks the
install initialized. It never runs unmerged config.

Re-running `init` after an interruption is safe: when the committed config
at the tracked ref already matches the generated draft byte for byte, no new
config PR is opened — the flow resumes by validating that exact commit.

### init.yaml — every field

```yaml
settings:
  config:
    kind: single_repository          # or separate_repository
    remote: https://github.com/acme/web.git   # repo holding the config
    reference: main                  # ref reconcile tracks
  credentials:
    github-main:
      source: environment
      variable: GH_TOKEN
  plugin:
    install_user_global: true        # install the bundled bureau plugin now

repositories:                        # becomes repos.yaml
  web:
    url: https://github.com/acme/web.git
    forge: github
    access: push
    credential: github-main

assignment:                          # becomes assignments/<name>.yaml
  name: fix-failing-tests
  work:
    forge: github
    source: acme/web                 # owner/name
    filter: "is:open label:agent-eligible"   # forge-native query (see below)
    abort_label: bureau:failed
    escalate_label: bureau:needs-human
  primary_repo: web
  context_repos: []                  # extra read-only context repos
  verify: "cargo test --workspace"   # run by a deterministic step
  branch_prefix: bureau/
  adapter: copilot                   # copilot | claude | fake
  limits:
    max_concurrent: 2
    max_runs_per_hour: 6
    max_cost_per_day_usd: 25

first_pipeline:
  kind: fixed                        # bundled reference pipeline
  # kind: ai_authored                # or let the bureau:pipeline-author
  # request: "Prioritize flaky tests"  # skill draft it (needs `copilot`)
```

Then:

```sh
bureau init --from init.yaml
```

The generated first config wires two roles (`implementer` and `reviewer`,
referencing the bundled plugin agents) and a three-step pipeline:
`implement` (agent) → `verify` (deterministic, your `verify` command) →
`review` (agent). Review it in the config PR like any other change — merging
it is what authorizes bureau to act.

## Forge specifics

### GitHub

```yaml
# repos.yaml
repos:
  web:
    url: https://github.com/acme/web.git
    forge: github
    access: push
    credential: github-main
```

```yaml
# assignments/fix-failing-tests.yaml
work:
  forge: github
  source: acme/web            # owner/name (a URL also works)
  filter: "is:open label:agent-eligible"   # GitHub issue-search syntax
  abort_label: bureau:failed
  escalate_label: bureau:needs-human
```

- `filter` is GitHub issue-search syntax. bureau appends `repo:acme/web`
  itself — do not include a `repo:` term.
- Work item ids are `acme/web#42`; `bureau run ... --item` takes that form.
- Trust: items opened by an owner, member, or collaborator grade
  `maintainer`; everyone else's grade `untrusted`. Roles that write code
  require `maintainer` or better, so outside-contributor items are skipped
  by data-flow control, not by a blocklist.

### Dependency-driven labels

Label rules are deterministic reconcile work, not pipelines: they create no
worktree or pull request and do not invoke a model. The source must match a
GitHub repo in `repos.yaml` so Bureau can resolve its credential.

```yaml
# label_rules/graduate-unblocked.yaml
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

Each attempted update records durable `update_started` and
`update_applied`/`update_failed` audit events. The hourly limit counts attempts,
including failures. Items whose dependencies remain open are reconsidered on
the next pass without consuming update headroom. Failed or interrupted partial
updates are verified and retried by item identity even when removing a label
makes the item leave the original filter. A deleted item records
`update_abandoned` without blocking the rest of its rule.

### Azure DevOps

```yaml
# repos.yaml
repos:
  odsp-web:
    url: https://dev.azure.com/microsoft/Odsp/_git/odsp-web
    forge: ado
    access: push
    credential: ado-main
```

```yaml
# assignments/fix-flaky-tests.yaml
work:
  forge: ado
  source: "Odsp/odsp-web"     # project/repo
  filter: |                   # WIQL, passed through verbatim
    [System.WorkItemType] = 'Bug'
      AND [System.Tags] CONTAINS 'agent-eligible'
      AND [System.State] = 'Active'
  approval_label: agent-approved
  abort_label: bureau:failed
  escalate_label: bureau:needs-human
```

- `filter` is a WIQL `WHERE` fragment; bureau never parses it.
- Work item ids are `Odsp/12345` (`project/id`).
- **ADO items are always `untrusted` until they carry `approval_label`.**
  Because the bundled `implementer` role requires `maintainer` input, an ADO
  assignment whose agent steps need more than `untrusted` trust must set
  `approval_label` — validation refuses the config otherwise. Removing the
  label blocks the active run and requires an explicit `bureau retry`.

### Mixing forges

Config forge and work forge are independent settings. Config in a GitHub
repository (`settings.config.remote` on github.com) with work items in ADO
(`work.forge: ado`) is a valid, expected configuration.

## Multi-repository operation

Point `settings.config` at the config repository and register every work
repository in `repos.yaml` with its own access level:

```yaml
repos:
  odsp-web:
    url: https://dev.azure.com/microsoft/Odsp/_git/odsp-web
    forge: ado
    access: push               # the branch lands here
    credential: ado-main
  augloop:
    url: https://dev.azure.com/office/Augmentation/_git/augloop
    forge: ado
    access: read               # read-only context from another org
    credential: ado-main
```

An assignment lists `repos: [odsp-web, augloop]` — the first entry is
primary (the branch and PR land there); the rest are read-only context. A
run receives a token that can push to `odsp-web` and a token that can only
read `augloop`.

Each assignment is independent: one can watch ADO bugs, another GitHub
issues, each with its own pipeline, role, and limits.

## Day two

```sh
bureau validate runner-config     # check a config checkout; every error in one pass
bureau run fix-failing-test --item acme/web#42   # one item, once, foreground
bureau reconcile                  # the continuous loop (default 5m interval)
bureau reconcile --now            # one pass; start eligible work and wait
bureau watch                      # live dashboard: runs, budget, latest events
bureau dashboard                  # browser drafting table and run visualization
bureau list                       # every run
bureau show <run-id>              # replayed state of one run
bureau pause <run-id>             # pause at the next step boundary
bureau resume <run-id>            # allow re-entry or reconcile to continue
bureau cancel <run-id>            # cooperative stop between steps
bureau retry <run-id>             # new run for the item an earlier run targeted
bureau doctor --json              # read-only diagnostics (offline)
bureau repair                     # preview, then confirm, reversible repairs
```

While the daemon runs, `bureau watch` is the standing answer to "what is
it doing right now": a self-refreshing terminal view of the adopted
config commit, active leases, every run's current step and cost, and the
per-assignment budget headroom. It reads `~/.bureau` without ever
writing or locking it.

It refreshes once a second. Arrow keys select a run; `q`, `Esc`, or `Ctrl-C`
exits. Piped output is one plain-text snapshot. Budget headroom excludes the
open-PR limit, which requires forge state.

`bureau dashboard` is the browser counterpart. It serves the same config,
pipeline editor, run overlays, transcripts, and controls as the GitHub Copilot
app canvas on loopback only. Use `--no-open` for SSH port forwarding or
`--dev` from a trusted source checkout while changing
`.github/extensions/bureau-canvas/web/`; development reloads preserve the
selected pipeline mode, run, and step. The web bundle is embedded in the
binary, so normal dashboard use does not depend on the source checkout.

`bureau run` exit codes: `0` success or no-work, `1` failure/blocked/
claim-lost, `2` setup errors (e.g. a missing credential, named in the
error).

Every run writes `~/.bureau/runs/<run-id>/`:

- `events.jsonl` — append-only, fsync'd, secret-scrubbed; **the** source of
  truth. Killing the daemon mid-run and restarting resumes from it.
- `state.json` — a derived cache, reconstructible by replaying the log.
- `artifacts/` — files steps published.
- `wt/` — the run's git worktree, on branch `<branch_prefix><pipeline>/<run-id>`.

### Change local settings

```sh
bureau setup --from settings.yaml
```

An explicit state migration copies durable state and run history, not
credentials, worktrees, activation records, or disposable caches. It rejects
overlapping paths, active leases, symlinks/hard links, corrupt or newer database
schemas, and non-empty targets. A durable marker blocks normal workers while
migration is incomplete; retries resume or roll back after interruption.

## The agent plugin

The installable `bureau` plugin provides the public agent resources:
`/bureau:implementer`, `/bureau:reviewer`, `/bureau:pipeline-author`, and
`/bureau:run-inspector`. Agent files own their model, instructions, and
tools; roles reference them and add only adapter, permissions, and trust.

- `bureau init`/`bureau setup` with `plugin.install_user_global: true`
  installs the plugin user-globally.
- A work repository may ship its own copy through a local marketplace under
  `.github/`; target-repository plugins intentionally override the global
  ones. Reviewed config still controls adapter, permissions, trust, limits.
- Runs never auto-install: a missing plugin fails before spawn with the
  install action in the error.
- Every run log pins the resolved plugin's source, version, and digest.

A role may also reference a plain agent file (`agent: agents/reviewer.md`)
instead of a plugin invocation; the bytes are pinned into the run log at
config-adoption time.

### Agent transport

By default, both production adapters use the official Rust ACP client and stable protocol
v1 over supervised stdio: `copilot --acp --stdio` or `claude-agent-acp`.
Each attempt opens a fresh session, selects the exact advertised custom agent,
and supplies `bureau-io` through MCP. A missing selector fails before prompting;
hidden session state does not pass between steps.

Role grants authorize normal work. Additional permission requests, including
sandbox bypass, are denied. Copilot retains its sandbox and native allow/deny
rules; Claude receives tool grants through its public session metadata.
Bureau controls the cleared environment, process cleanup, deadlines, and
secret-scrubbed logs. Provider permission enforcement remains a provider
responsibility; [github/copilot-cli#4537](https://github.com/github/copilot-cli/issues/4537)
is a known compatibility risk. Protocol tests do not establish sandbox safety.

`end_turn` alone is not success: an agent must publish through MCP or return
a valid `v2` result in agent-message text. Context-only usage updates preserve
the latest cumulative USD measurement; explicit unusable cost reports clear it.
Assignments with cost limits fail closed without usable measured USD cost.

### Opt-in local Copilot factories

For a reviewed runtime factory, add `copilot_factory` to an existing
Copilot-role agent step. This invokes the real local SDK factory API, not a
`/factory` prompt and not the [cloud automation controls](github-cloud-factories.md).
Ordinary steps keep their existing ACP behavior.

```yaml
- name: review
  type: agent
  role: reviewer
  inputs_from: [verify]
  copilot_factory:
    name: review
    extension: project:review
    extension_digest: "tree-sha256:<64 lowercase hex digits>"
    model_credential: copilot-model
    metadata: factory.json
    runtime:
      profile: copilot-sdk-factory-v1
      directory: /opt/qualified-copilot
      digest: "tree-sha256:<64 lowercase hex digits>"
      version: "<exact expected connect.version>"
      executable: bin/node
      cli: dist/index.js
      dist: dist
    args: { format: detailed }
    limits:
      max_concurrent_subagents: 2
      max_total_subagents: 4
      timeout_seconds: 120
      max_ai_credits: 1.5
  next: done
```

The placeholders must be replaced; this is not a ready-to-run configuration.
The literal `runtime.profile: copilot-sdk-factory-v1` names **Bureau's SDK
capability contract**, not an SDK release, version, or generally available
feature mapping. No compatibility alias is accepted. `runtime.version`
remains the exact expected `connect.version`.

Provision and review an operator-qualified, self-contained bundle with the
actual executable, CLI distribution, stock extension bootstrap/resolver and
bundled SDK. Both the bundle and provider require exact canonical tree
digests. `tree-sha256:` identities use Bureau's `bureau_plugin::tree_digest`
byte/path/mode algorithm, not an archive hash.
There is no verified mapping to a generally available SDK or CLI release,
and offline checks do not prove live entitlement. Protocol 3, a version
string alone, or `--experimental` is insufficient. Actual eligibility,
including staff rollout and eligible GitHub token-based billing, is an access
prerequisite. Bureau never fabricates eligibility or bypasses authentication
or rollout checks.

The original provider is `.github/extensions/review/extension.mjs` in the
work repository. It must use the runtime's real SDK
`defineFactory`/`joinSession` registration and provide the standard
`FactoryMeta` JSON at `factory.json` (or `metadata`). Keep that metadata and
its full `argsSchema` inside the reviewed provider tree. Bureau pins the
provider and bundle privately before executing either. Schema validation
does not retrieve external URLs or files.

The role must grant `model:invoke`. `model_credential` is a required
reference, not a token or a fallback to a repo credential. Declare its source
in existing local `settings.yaml`, for example:

```yaml
credentials:
  copilot-model:
    source: environment
    variable: BUREAU_COPILOT_MODEL_TOKEN
```

Provision that source separately with an eligible token authorized for
Copilot model access. Bureau never copies an ambient Copilot login or
credential store. The reference is retained in the run snapshot and resolved
again on cold recovery; credential values are not persisted there or granted
to forge tools by this declaration.
During daemon reconciliation, an unavailable model source blocks only the
assignments that need it; ordinary work and independent label rules continue.
Each affected assignment is diagnosed, and an otherwise idle pass reports the
failure. Cold daemon recovery applies the same rule to a known preserved
factory run: keep its reservation and original workspace, report the run and
credential reference, and continue independent recovery and work. Corrupt
evidence, changed identities and lost ownership still fail explicitly.
Deferral does not make an indeterminate or nonresumable factory resumable.
Explicit run/resume commands remain strict about their declared sources.

Bureau selects and registers the canonical `COPILOT_GITHUB_TOKEN` carrier
on every runtime launch; it does not export additional model-token aliases.
Mixed roles retain a separately authorized `GH_TOKEN` from Bureau's existing
`FORGE_GRANTS` policy, even when both credentials intentionally use the same
value. This channel is never an implicit model-authentication fallback;
model-only roles still receive no forge token. Prefer independently scoped
tokens: the model reference neither reduces token rights nor adds forge grants.
Both session shell credentials and private sandbox Git/gh credential
injection are disabled. The generated private SDK settings are checked
byte-for-byte before startup. Missing or changed policy, malformed legacy
configuration, or a legacy sandbox-policy override blocks execution instead
of falling back to native defaults. Do not hand-edit the run's private policy.
Managed deny-wins is respected without disabling managed policy or changing
sandbox enablement/filesystem/network rules.

The supported context excludes unapproved executable configuration and LSP
operations, including for nested children under the qualified tool policy.
The broker's command/argument/environment configuration must not contain
`$` expansion; this restriction does not reinterpret opaque factory `args`.
Environment filtering is not OS isolation or a sandbox for approved provider
JavaScript, and it does not attenuate children's authorized model access.

**Committed configuration
authorizes executable provider code**, arguments and ceilings: direct SDK
calls do not show the model tool's separate factory approval dialog.
Factory-host JavaScript is trusted code, not sandboxed by child tool grants.
Additional tool or sensitive-environment permission requests are denied.
Saving an editor draft neither authorizes uncommitted execution nor launches
a factory.

`args` stays an object or null; omission means null. Argument array order and
repeated values survive editor saves exactly. No dynamic inputs are
injected into it. `inputs_from` still supplies the v2 `StepRequest` through
the restricted `bureau-io` context tool. The factory itself must return a
complete v2 `StepResult`; child publications and result previews do not
finish the step. Native ceilings are independently optional and soft where
the runtime says so. Leaving one out retains native policy; resume does not
reset or raise it. An explicit `max_concurrent_subagents` must be between
1 and 500 under this SDK capability contract; that is not a total-subagent limit.

This mode deliberately disables ambient configuration discovery and passes
only approved pinned resources. The real settings key is
`extraKnownMarketplaces`, not `extraKnownMarketplace`. `enabledPlugins` alone
does not prove installation, trust, activation or child-tool availability.
Do not expect all app/global/repository plugins to appear automatically.
Cold resume uses the same stored cwd/context, not a new discovery pass.

Use `bureau show <run-id>` and `--events --json` to inspect the distinct SDK
session, native run/attempt, usage and preserved workspace. Existing
`pause`, `cancel` and `resume` commands remain Bureau control intents.
Native pause is orderly/resumable; cancel is not. A lost start reply is
indeterminate and cannot be retried or resolved by choosing the newest
same-named run. Hard-killed/interrupted native runs cannot resume, and live
ownership may remain visible until the runtime lease expires.
A definite SDK admission rejection with verified clean shutdown follows the
step's configured failure route. It does not manufacture a pause; an actual
operator pause still takes precedence.

`bureau show <run-id> --json` reports structured state and Bureau's
local-factory continuation decision. Canvas Resume uses that same decision;
an unavailable or older CLI cannot authorize continuation from raw browser
replay. `resume` clears an eligible pause for the reconcile loop's same-run
recovery; it does not itself execute a factory or create a new Bureau run.

Lease release or expiry does not make unfinished factory work available
again. Reconcile, `run` and `retry` refuse a fresh run for the same assignment,
forge and item while its original factory work remains preserved. Inspect
and, only when eligible, resume that original run; do not use `retry` as a
replacement for an indeterminate start.

Keep the original worktree, private pins and complete opaque SDK-owned
external state together. Before restarting the SDK, Bureau validates the
saved session/workspace metadata: exact session ID, canonical absolute cwd
and retained worktree filesystem identity. Missing, malformed or changed
metadata blocks recovery; it is not repaired or replaced with a new cwd.
Deleting Bureau's derived `state.json` is recoverable; losing SDK state or
recreating the worktree is not. SDK step replay is at-least-once around
external effects, not filesystem checkpoints. Measured credits use an
explicit $0.01-per-credit normalization, not an invoice; incomplete
accounting is reported as a floor rather than zero. These constraints apply
only to this opt-in mode; see [DESIGN section 17](../DESIGN.md#17-local-copilot-runtime-factories).

## Try it offline first

No forge, no model, no network:

```sh
bureau fake record fixture.json -- python3 -m pytest -q   # capture a real run
bureau fake replay fixture.json                            # replay it
```

The `fake` adapter replays recorded transcripts; config validation allows
`fixture:` paths only on roles using it. The repository's own test suite is
the standing proof the whole stack runs offline:

```sh
cargo test --offline    # engine, reconcile, plugins, forges — all fake-backed
```

## Where to go next

- [DESIGN.md](../DESIGN.md) — architecture, control model, trust, and limits.
- [README.md](../README.md) — quick start and everyday commands.
- `bureau doctor` — when anything in this guide misbehaves, start there.
