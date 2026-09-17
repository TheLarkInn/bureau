# Single-owner maintenance deployment

Run **one Bureau engine** on one deliberately provisioned Linux machine, VM, or
dev container. Keep its SQLite database, run logs, checkout cache and ownership
lock on the same local, persistent native filesystem. Three assignments are
three kinds of desired work, not three daemons. An idle development machine is
an optional manually chosen host, not a host-matching or dispatch system.

The shipped files are reviewed samples, **not an enabled deployment**. Source
issues [#131](https://github.com/TheLarkInn/bureau/issues/131) (chaos),
[#132](https://github.com/TheLarkInn/bureau/issues/132) (accessibility) and
[#130](https://github.com/TheLarkInn/bureau/issues/130) (responsive) were created
with category labels only. Startup approval and the workflow activation variable
remain off. The existing development Linux root was observed nearly full; that
host must fail admission even when its Windows backing volume has ample room.
A successful isolated test campaign is not proof of durable deployment readiness.
Do not touch, migrate, or clear an existing Bureau home to make these samples run.

## Reviewed code and configuration

The maintenance-only config root is **`.bureau/maintenance`**, containing its own
registry, assignments, pipelines, roles and direct agents. It does not copy or
select the ordinary `agent-eligible` or design-audit assignments in `.bureau`.
The existing `reconcile --config-subdir` selects this committed directory.
Never launch a bare `bureau reconcile` from these samples: local settings retain
the ordinary single-repository schema, whose default subdirectory is `.bureau`.
The launcher always supplies the explicit maintenance override.

`deployment/maintenance-policy.json` pins the exact source issue numbers,
maintainer login and numeric issuer ID. It also declares the disposable Cargo
cache, prepared browser tools, and any backing-volume admission paths. Changing
those paths, identities, resource limits, agents or pipeline grants requires
normal config/code PR review. No runtime generates a second config or writes
status into Git. The service pins an explicitly reviewed full config SHA.

Before activation, provision and qualify:

1. A dedicated non-root Linux account, a **new** private persistent
   `/var/lib/bureau-maintenance`, a separate runtime home, and a disposable
   `/var/cache/bureau-maintenance/cargo` with an 8 GiB ceiling. State must be
   native ext4, XFS or Btrfs, not tmpfs, a Windows mount, or a network share.
   Independently guarantee persistent volume backing; filesystem type alone
   does not prove that a VM disk survives deletion of its host.
2. A read-only `/opt/bureau/source` at the reviewed commit, its matching built
   Bureau executable in `/opt/bureau/bin`, Node 24, Git, Bash, `flock`, `unshare`,
   `getconf`, the repository's Rust toolchains/lint tools, and offline Cargo dependencies.
   Node must provide Linux `process.execve`; missing replacement support fails
   closed rather than substituting a detached command.
   Qualify unprivileged user/PID isolation under the actual service/container
   policy. Do not disable the command sandbox, run privileged, auto-install
   tools, or use a personal home/SSH agent/container socket to bypass a refusal.
3. Prepared read-only browser tools at `policy.site_tools`: the `site/` package
   and the existing canvas Playwright package at their ordinary relative paths,
   each with its reviewed manifests, lockfiles and installed `node_modules`.
   Prepare Chromium separately at `policy.browser_path`. The site resolver
   compares both manifest/lock pairs against the checked-out code and confines
   modules to the explicit tool root. Missing/mismatched tools fail; there is
   no runtime install or ambient fallback. Mount tools and browser executables
   read-only to prevent a fixer from rewriting ignored dependencies.
4. Private settings based on `deployment/settings.yaml.example` and the
   `github-maintenance` credential reference; never put values in Git. Qualify
   the declared adapter's model authentication in the dedicated runtime home,
   without mounting a developer's authenticated home. Reporter grants require
   issue reads/writes and model use; fixers receive no forge/push/merge grant.
   The current adapter needs `repo:write` to invoke any shell, including the
   reporter helper. Native grants are not token attenuation: provision the
   narrowest actual credential and sandbox policy the operation needs.
5. Measurable adapter USD usage. Each assignment caps daily cost at $10;
   **unknown cost is not zero**. An unmeasured model step fails closed and may
   leave an inert draft requiring inspection. Do not remove the cost cap to
   disguise missing accounting. No automatic merges are authorized.

Use `deployment/bureau-maintenance.service` for a provisioned Linux host.
`deployment/compose.yaml` is an alternative for an independently qualified,
digest-pinned image with native persistent named volumes. Choose **one**, not
both. Container startup must already have private state/settings, cache/tooling,
read-only dependencies, and working user/PID isolation; default runtime policies
may refuse `unshare`. The sample intentionally cannot install or relax policy.
Do not use a Windows bind mount for SQLite. Do not remove durable volumes during
an update. No host or container port is publicly published.

Both launch paths take the same nonblocking `flock` on the state root before
entering `run-owner.sh`. The lock is held for the daemon's entire lifetime.
Do not unlink that file, bypass the launcher, run `reconcile --now` alongside it,
or point a second host with separate state at the same work. A filesystem lock
and local SQLite CAS do not provide separate-host coordination.

## Resource admission and shutdown

The startup check examines Linux `/`, runtime home, work, temporary storage,
durable state, Cargo cache, tools and every declared backing filesystem. Each
must expose at least **4 GiB available to the process**. Under WSL an explicit
backing-volume path is mandatory in `backing_paths`; free virtual-disk space
alone is not a host-volume capacity check. If host automount is disabled, an
administrator may expose only a dedicated read-only capacity-check directory
on the backing filesystem. Never auto-mount a whole host home or drive for this.
This repository's reviewed path is `/mnt/bureau-q-capacity`, a read-only bind of
the empty `Q:\WSL\BureauOperationsTelemetry` directory. Provision that narrow
mount before the service starts; no broad Q: mount stays exposed. The guard
requires a distinct backing filesystem and a read-only mount, not an ordinary
empty directory on the Linux root. Other hosts need a reviewed path adjustment.
Unobservable capacity, the nearly-full native root, or insufficient backing
space is a refusal, not a cleanup request.
Filesystem proof uses an existing, non-symlink directory opened with Linux
`O_PATH`. Its `/proc/self/fdinfo` mount ID selects the exact mountinfo record;
row order and equal-path hidden mounts cannot establish read-only status.
Capacity and device identity are read through that same live descriptor.
Missing/inaccessible paths, malformed metadata and unknown mount IDs fail;
there is no fallback to a read-only ancestor.

The systemd sample bounds the whole service to 8 GiB RAM, zero extra swap,
two CPUs and 256 PIDs. It restarts at most three times in fifteen minutes.
Container samples use the same hard bounds but do not automatically restart.
Deterministic helpers additionally reserve the complete allowed child RSS plus
1 GiB headroom, independently on the host and every cgroup ancestor, and require
16 PID slots before spawn. Hierarchical cgroup limits are checked,
not just host RAM. Checks share one command lock, waiting only within their
existing deadline so concurrently selected assignments do not immediately fail
each other. Waiting retains the disk/PID/output guards and a 256 MiB memory
floor; full child RSS plus headroom is checked again after acquisition and
before execution. The lock holder waits for the original process, which replaces
itself with `unshare` rather than leaving an intermediate command parent.
Executed checks are never retried by the lock helper. Checks have one bounded
child, a clean explicit environment, combined stdout/stderr <=1 MiB,
and <=128 MiB owned scratch. Runtime probes recheck disk, cgroup headroom,
group RSS/PIDs, at least 256 MiB remaining memory, scratch and the bounded reusable
Cargo cache. Other users can still consume resources after admission; these
checks refuse known insufficiency rather than promising an absolute OOM guarantee.
Process identity, parent, start time, group and resident-page counters come
from the same `/proc/PID/stat` record, multiplied by the observed, validated
`getconf PAGESIZE` result. Accounting follows descendants across `setsid` and
reparenting to the child PID namespace's init; a detached browser cannot escape
the check's RSS/process ceilings by changing its group. The observation table
is capped at 8,192 entries and incompatible parent identities fail closed. No
earlier process state is combined with a later optional `VmRSS` field. A numeric
zero is an observation; missing, malformed or unreadable live counters fail.
Scratch/cache walks tolerate only explicitly vanished non-root children
(`ENOENT`/`ESRCH`), retaining already observed bytes conservatively. Attempted
entries still count toward the 10,000-entry bound. Root identity is pinned
before execution and rechecked during and after enumeration; missing/replaced
roots, permission errors, malformed sizes and escaping names remain failures.

Browser audits have a 150-second outer deadline; short chaos checks have five
minutes including an offline, single-job rebuild. Full repository verification
has fifteen minutes and a 4 GiB group RSS ceiling; ordinary checks have 2 GiB.
Run Node test files serially with `--test-concurrency=1`, as the integrated lint
entry point does. CPU quota throttles execution but does not constrain Node's
reported available parallelism or its default number of test-file processes.
`unshare` provides a PID-namespace init inside a dedicated process group.
Timeout/cancellation kills the group and namespace, including descendants which
create new sessions. Required isolation failure is infrastructure failure, not
a test result. The limits are safeguards, not permission to fill a disk or RAM.
Normal retained history still needs explicit operator capacity review; the
launcher never truncates logs, resets SQLite, deletes caches or reclaims history.

First SIGINT/SIGTERM stops claims and drains active runs while renewing leases.
The service uses `KillMode=mixed` so the first signal reaches the engine, not
all its children. It allows seventy minutes, longer than the one-hour complete
run limit. A second signal to the engine requests hard process-group cancellation.
If graceful shutdown exceeds the service deadline, systemd/container termination
is a hard stop: retain history and inspect recovery on restart. Never interpret
that as successful completion. Removing an assignment or role drains rather
than killing work. Inspect before explicitly retrying a failed/indeterminate run.

## Detection, reporting and actual fixes

| Assignment | Deterministic detector | Automatic patch scope |
|---|---|---|
| `maintenance-chaos` | `maintenance_chaos` / `seeded_offline_invariants`, fake agents/forges, seed in evidence | `crates/` and required `dylint.toml` edges; invariant suites and build inputs protected |
| `maintenance-site-accessibility` | `node site/check.mjs --kind accessibility --json`, real loopback browser/axe | `site/src/**` only |
| `maintenance-site-responsive` | `node site/check.mjs --kind responsive --json`, real viewport measurements | `site/src/**` only |

Every assignment permits one active run, two runs/hour, six/day, two open PRs
and one hour per run. At most three assignments can have active work at once;
the service bounds their aggregate resources and deterministic command admission
is serial. A full open-PR cap also pauses scans: review outstanding proposals,
do not quietly raise the cap. Rate/cost limits include reporting and fixes.

A canonical persistent source has exactly one category marker and intent block:

```text
<!-- bureau-maintenance-source:chaos -->

Human-maintained purpose and scope remain outside the block.

<!-- bureau-maintenance-intent
{"schema":"bureau-maintenance-intent-v1","category":"chaos","commit":"FULL_40_HEX_REVIEWED_COMMIT","cycle":"2026-09-17T12:00:00Z"}
-->
```

The placeholder is not valid configuration; use a real committed SHA.
Substitute the other exact category for the other two issues. The configured
issuer ID, issue number, exact repository URL, unique marker, exclusive category,
open state, `bureau:maintenance-approved` and `bureau:maintenance-ready` are
all checked. The updater may reopen an explicitly approved persistent source,
but never creates one or grants either approval/ready label.

`.github/workflows/maintenance.yml` runs only on trusted default-branch schedule
or manual dispatch, and only when the repository variable
`BUREAU_MAINTENANCE_ENABLED=true`. It changes a bounded intent block to the
committed SHA and a stable six-hour UTC cycle. It preserves human prose and
unrelated labels, re-arms `bureau:maintenance-scan`, and clears only the previous
`bureau:maintenance-reported` marker. Same-cycle completed work stays completed;
partially observed label updates can recover without creating another ticket.
Terminal failed/needs-human sources require human review before re-arming.
The workflow has no Bureau command, daemon credentials, queue or run database.
Missed events do not lose pending forge intent; the next poll still sees it.

The pipeline records the exact clean source commit and rejects a checkout that
has moved from the requested SHA. If main advances before a scan starts, refresh
intent and explicitly review/clear the terminal diagnostic; never silently audit
one revision while reporting another. Incomplete/skipped checks, missing tooling,
resource refusals and malformed evidence escalate without manufacturing findings.
Executed checks retain their bounded raw stdout/stderr as failure artifacts,
including incomplete accessibility reports needing manual review. Failure to
persist those artifacts is reported explicitly, never converted to a clean scan.

With real findings, the model-facing reporter invokes the bounded helper to
create or adopt **one inert finding issue**. Its body is canonical deterministic
evidence, not a model-authored success claim. The fingerprint includes category,
source issue, source commit and finding identities, but not the recurring cycle.
Closed/rejected findings are not reopened. An ambiguous create is followed only
by an exact bounded observation; failure to identify a unique issue blocks and
never repeats POST. Reads have ten-second deadlines, 2 MiB response bounds,
no redirects and at most five pages; rate limiting is an explicit failure.

A separate deterministic step re-reads the live source, issue, numeric issuer,
full evidence, dedup and publication identity before any handoff. Only then can
the reporter record an exact source report and add the category-specific
`bureau:maintenance-fix` plus `bureau:maintenance-ready` labels. Another
deterministic step verifies those observed effects. Ordinary `agent-eligible`
and design-scan labels are forbidden. Zero findings likewise require a verified
source report, not simply an agent saying "no work".
Same-cycle draft evidence must exactly match the detector. Reuse of an older
cycle requires a canonical source report whose forge-created and last-edited
timestamps both precede the source's forge timestamp observed at deterministic
intake (not the host clock); the reporting agent cannot create
a new "previous report" to justify replacing the current scan's evidence.

The same assignment picks up that derived finding on a later pass. It rechecks
the canonical source's **current** approval, the issue's readiness, configured
issuer, exact source report and finding provenance, then reproduces the failure
before authorizing a local patch. It allows one proposal and at most one repair.
Reproduction and validation retain the original finding's seed even when the
current checkout is a later descendant of its source commit.
It re-runs the real detector, checks paths, pins verification code against the
initial source even across checkpoint commits, and runs all repository gates
before reaching the existing engine's lease-fenced PR publication.
Rust/Node manifests and locks at every depth, `build.rs`, `.cargo`, toolchain,
formatting and Git-filter inputs are not automatic patch scope. The reviewed
bootstrap and the command adapter both pin those inputs before any Cargo
invocation; tracked files and protected invariant/accounting/migration proofs
are compared as raw bytes with the initial source.
Ignored/untracked additions and index-hiding flags cannot redirect the exact
test target. Such changes require a separately reviewed human patch.
Deterministic `inputs_from` entries come last so agent outputs cannot overwrite
the source/evidence pins. Checks and model text do not replace human PR review.

Remove `bureau:maintenance-approved` from a source to stop future handoffs and
finding pre-write checks. Remove `bureau:maintenance-ready` from an active finding
as well to use the engine's every-boundary/publication approval fence; source
revocation alone is not an instantaneous cancellation signal for a running model.
No model step gets push/merge authority. Source evidence is verified again before
and after patch checks and full gates. Agent-provided cost or claimed results
never authorize another run.

## Offline checks and a reusable 25-minute campaign

Fast helper/lifecycle tests need only Node and injected fake forge state:

```sh
node --test --test-concurrency=1 scripts/maintenance*.test.mjs
```

For a long campaign, obtain a resource slot first. Build the dedicated Rust
integration test **once**, offline, at a clean reviewed commit, with one Cargo
job and a bounded disposable target directory. Its fixture root must be native
Linux; passing a Windows mount to permission-sensitive tests is not equivalent.
Use the exact executable reported by Cargo's `--no-run` output, not a guessed
glob or the newest unrelated binary:

```sh
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/var/cache/bureau-maintenance/cargo \
  cargo test --offline --test maintenance_chaos --no-run
node scripts/maintenance-suite.mjs \
  --binary /absolute/path/reported/by/cargo/maintenance_chaos-HASH \
  --output /absolute/campaign/suite.json
node scripts/maintenance-soak.mjs \
  --suite /absolute/campaign/suite.json --scratch /absolute/campaign/scratch \
  --seed 0 --minutes 25 --max-iterations 1000
```

Under WSL add `--backing /absolute/read-only/backing-capacity-directory`.
All named parent directories must already exist on an admitted filesystem.
Record the manifest only after verifying the selected executable was built
from that source; its digest detects later modification, not a dishonest build.
Manifest creation is exclusive and never overwrites earlier campaign evidence.
Keep the prebuilt executable immutable for the campaign. The runner checks
device/inode, size and modification/change timestamps around hashing and every
repetition, so a concurrent rebuild fails explicitly instead of silently testing
different code under the old source/binary receipt.

The campaign invokes only the prebuilt exact test, with
`BUREAU_CHAOS_SEED=<u32>` and its private `TMPDIR`. No Cargo, browser, API or model
is called between iterations. There is one child at a time, 30 seconds/iteration,
512 MiB group RSS, 64 group PIDs, 1 MiB output and 128 MiB scratch ceilings.
Seeds advance deterministically; pacing avoids a busy loop. Maximum duration is
thirty minutes and maximum repetitions are 2,000. Hitting a repetition ceiling
before the requested duration is **incomplete**, not a passed soak.

Stdout ends with one `bureau-chaos-soak-v1` JSON result containing source/binary
pins, initial seed, elapsed time, iterations and the failing seed/iteration with
bounded evidence when applicable. Exit is nonzero on assertions, missing or
ignored tests, timeout, infrastructure/resource refusal or incomplete execution.
Keep that small JSON as the campaign record; reproduce a failing seed with the
same manifest. Only each explicitly owned temporary fixture directory is
removed. The campaign never accumulates per-iteration builds or deletes Bureau
run history. A separate approved, read-only validation environment may provide
its own independently enforced execution adapter to the exported `runSoak`
function; that does not waive or emulate production admission.

## Private inspection and updates

Run local inspection as the dedicated `bureau` account, with the reviewed service
PATH and the same runtime home and config, never from a personal authenticated home:

```sh
export BUREAU_HOME=/var/lib/bureau-maintenance
export HOME=/var/lib/bureau-maintenance-runtime
/opt/bureau/bin/bureau validate /opt/bureau/source/.bureau/maintenance
/opt/bureau/bin/bureau doctor --json
/opt/bureau/bin/bureau watch
BUREAU_CANVAS_READ_ONLY=1 /opt/bureau/bin/bureau dashboard \
  --dir /opt/bureau/source/.bureau/maintenance --no-open --port 7331
```

`doctor` is offline and reads this home's active config cache, populated by the
explicit maintenance reconcile path. It has no subdirectory flag; before the
first activation it can honestly report that no committed snapshot is cached.
The reviewed launcher, not bare CLI defaults, selects execution. Use the matching
integrated binary whose backend-enforced `BUREAU_CANVAS_READ_ONLY=1` mode has
been qualified. Confirm the State/Operations response reports
`access.mode=read-only` and an explicit reason before forwarding the dashboard;
setting an environment variable on an older binary is not proof of enforcement.
Merely selecting `--dir /opt/bureau/source/.bureau/maintenance` or canvas `dir=.bureau/maintenance`
does not bind execution: a manual bare `reconcile --now` can select root `.bureau`
and bypass the owner lock. Managed inspection must reject run/reconcile/retry,
state controls, and config writes with an explicit reason, not just gray a button.
Keep editable authoring canvases in the separate workspace with the normal PR
review path. Use `watch`, `list` and `show` instead if the dashboard guard has not
been qualified. The read-only mode is immutable for the server lifetime; invalid
values fail closed and request bodies cannot relax it.

Keep the dashboard on its existing loopback bind. Use an existing authenticated
private forwarding channel, for example an SSH local forward from the developer
machine to the Linux VM's `127.0.0.1:7331`. For a dev container, use the existing
private container-loopback forwarding facility; if it cannot target loopback,
do not replace that with a public bind or published port. Never put credentials,
state or daemon access on the public static Pages site. Bureau adds no login,
public authorization service, inbound webhook server or network control plane.

For an update: drain the one owner; retain its complete durable home; qualify
the new reviewed executable/config/tools and available disk; update the explicit
commit pin; inspect recovery; restart that same owner. Back up closed SQLite
and its complete run history coherently before a deliberate migration. Do not
copy an active SQLite file to another independent host and claim its leases
coordinate. No sample performs migration, installation, publishing or activation
without an explicit operator action.
