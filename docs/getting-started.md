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
- `git` on `PATH`.
- An agent CLI for agent steps: GitHub Copilot CLI (`copilot`) or Claude
  Code (`claude`). You can also try everything offline with the `fake`
  adapter — see [Try it offline first](#try-it-offline-first).
- A token for each forge you use:
  - **GitHub**: a token with repo and issues/PR access to the repositories
    involved (a PAT or a GitHub App token).
  - **Azure DevOps**: a PAT with Code (read/write) and Work Items
    (read/write) scopes for the organization.

## Install

From a source checkout:

```sh
cargo install --path crates/bureau
bureau version
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

## The four config concepts

All reviewed configuration is four small YAML shapes:

| File | What it declares |
|---|---|
| `repos.yaml` | The repo registry: every repository bureau may touch, with a per-repo access level (`read`/`pr`/`push`) and a credential *reference*. |
| `roles/<name>.yaml` | An agent reference (`/bureau:implementer` or a path to an agent `.md`), the adapter that runs it (`copilot`/`claude`/`fake`), credential-grant permissions, and a minimum input trust. |
| `assignments/<name>.yaml` | The standing arrangement: which work source to watch, which repos to touch, which pipeline and role to use, the `verify` command, the branch prefix, and budget limits. |
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
  pipelines/fix-failing-test.yaml

runner-config/                   # separate-repository mode, same schema at the root
  repos.yaml
  roles/...
  assignments/...
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
```

- `filter` is GitHub issue-search syntax. bureau appends `repo:acme/web`
  itself — do not include a `repo:` term.
- Work item ids are `acme/web#42`; `bureau run ... --item` takes that form.
- Trust: items opened by an owner, member, or collaborator grade
  `maintainer`; everyone else's grade `untrusted`. Roles that write code
  require `maintainer` or better, so outside-contributor items are skipped
  by data-flow control, not by a blocklist.

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
bureau list                       # every run
bureau show <run-id>              # replayed state of one run
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

`bureau run` exit codes: `0` success or no-work, `1` failure/blocked/
claim-lost, `2` setup errors (e.g. a missing credential, named in the
error).

Every run writes `~/.bureau/runs/<run-id>/`:

- `events.jsonl` — append-only, fsync'd, secret-scrubbed; **the** source of
  truth. Killing the daemon mid-run and restarting resumes from it.
- `state.json` — a derived cache, reconstructible by replaying the log.
- `artifacts/` — files steps published.
- `wt/` — the run's git worktree, on branch `<branch_prefix><pipeline>/<run-id>`.

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

- `DESIGN.md` — the authoritative spec (control model, trust, limits).
- `README.md` — command summary and the layer map.
- `bureau doctor` — when anything in this guide misbehaves, start there.
