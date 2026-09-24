# Opt-in Windows/WSL lifetime supervision

This is a separately installed deployment profile, **not an enabled runner**.
The ordinary Linux service and its one engine command remain unchanged. Do not
install this profile on an ordinary Linux deployment merely because these files
exist. It adds no dispatcher, host selection, second engine or separate durable
state owner. Assignment quotas are unchanged.

WSL systemd services do not by themselves keep a distribution alive
([Microsoft's systemd announcement](https://devblogs.microsoft.com/commandline/systemd-support-is-now-available-in-wsl/)).
Conversely, an unrelated terminal can keep a distribution alive after a Windows
monitor exits. Therefore neither starting a service once nor killing `wsl.exe`
is a lifetime contract.

## Ownership and refusal

The Windows foreground supervisor holds one machine-wide mutex before launching
anything in WSL. It selects one exact distribution from the current Windows
user's registration; `BureauOperations` is the example, not a discovery fallback.
Its sole long-lived `wsl.exe` call stays attached, with redirected stdin/stdout.
It never terminates WSL, starts another distribution, copies desktop login state
or stores a Windows password.

The native entry takes a second nonblocking `flock` through the complete
transaction and cgroup drain. It verifies root-protected read-only reviewed
source, exact installed service/drop-in bytes, loaded systemd properties and
explicit deployment approval. The additional non-secret
`/etc/bureau/windows-supervision.json` binds the reviewed source commit to the
qualified immutable Bureau binary's SHA-256 and the canonical Node 24 path and
SHA-256; its example is deliberately unapproved. Bootstrap and the unprivileged
pre-start use the existing service's fixed tool `PATH`, not an assumed
`/usr/bin/node` or an inherited executable override. The bootstrap's actual
interpreter must match that explicit pin and Node 24 before a unit is created;
the heartbeat then uses that qualified canonical executable. Startup hashes
the actual bounded executable files without running the Bureau binary.
Existing `run-owner.sh` still verifies the config
commit, private durable state, immutable Rust/runtime provenance, Linux resource
admission and maintenance configuration before executing the **original**
`bureau-maintenance.service` command. The original durable-state owner lock
remains in effect.

Exactly two installed drop-ins are admitted: `windows-supervision.conf` and the
required `20-reporter-credential.conf`. The latter's canonical source template
preserves the approved narrow reporter binding to
`/var/lib/bureau-maintenance/credentials/github-reporter.env`; do not remove it
or expand the drop-in allowlist. Both `EnvironmentFile` bindings must be required
and match the loaded unit. Startup checks the reporter file's canonical path,
regular-file type, private ownership/permissions and bounded size, **not its
credential contents**. The reporter file's intended mode/owner is
`root:root 0600`, but its writable `bureau`-owned ancestors do not make its
pathname root-protected. Therefore no command in the credential-bearing engine
unit receives a root privilege prefix. Clearing an environment after an ELF
loader has already run would not protect a privileged command.

Only the heartbeat is a transient unit. The opt-in
`deployment/windows-supervision.conf` binds the existing engine to it and orders
the engine after its readiness. The heartbeat establishes stdin ownership first,
publishes a fresh startup lease, then sends `READY=1`. It does not synchronously
wait for the engine before becoming ready; that would deadlock the ordering.

The engine's pre-start stays under `User=bureau` and asks the clean root heartbeat
for admission through a root-owned, group-connect-only Unix socket. Its parent
directory is root-owned mode `0750`; the lease and one-use grant remain root-only.
The service user cannot rewrite those files. A small Python 3 standard-library
helper supplies Linux `SO_PEERCRED`, which Node 24's public socket API does not
expose. It runs in the same bounded heartbeat cgroup with isolated Python startup,
one connection and bounded frames/read/reply deadlines. There are no Python
packages or runtime installations. Missing Python 3 or kernel support refuses
startup.

Authorization binds the kernel peer PID/UID to the actual service `ControlPID`,
invocation, process starttime, cgroup and qualified interpreter; a PID in JSON is
not peer authentication. The root heartbeat, not the service user, atomically
consumes exactly one grant. A separate service-user process cannot impersonate
the pre-start using its public PID. A failed start consumes a granted permit too.
`RefuseManualStart=yes`, `Restart=no`, and the one-use grant prevent a direct
start/restart, boot enablement, or an indirect replacement from bypassing the
profile. An operator must not add another start dependency, change these units
under a running transaction, or override the protected admission program.
Privileged administrators can rewrite policy; this is not isolation from root.

This matters for `BindsTo`: checking an old invocation and then stopping a unit
by name could kill a replacement. Production code never performs that
check-then-name-stop. Replacement admission is prohibited for the lifetime of
the dependency, and the native transaction refuses observed identity drift.
Each running observation binds the real systemd invocation, main PID, Linux
process starttime, cgroup membership and cgroup device/inode. A running service
with empty or unknown identity is a refusal.
A service cgroup must be exactly `<root>/system.slice/<unit>.service`, where
`<root>` is systemd's own root: PID 1's single cgroup v2 line
`0::<root>/init.scope` without `/init.scope`. It is empty on ordinary hosts;
WSL runs systemd under a per-boot `/wsl-user/distro-<N>/systemd`. Another
prefix, slice, traversal or malformed `/proc/1/cgroup` refuses.
Startup without a PID is allowed only before running, under an absolute
deadline; it is not reported as admission.

Every Windows response answers a new random native challenge after a fresh host
sample. Frames are strict bounded JSON, not an accumulating heartbeat queue.
There is one outstanding response, a six-second native read deadline, and a
twelve-second systemd watchdog enforced outside the native event loop. That is
an aggregate freshness bound, not permission to add the individual command
timeouts together. An overloaded monitor that cannot finish an exchange within
that bound must fail closed too; host qualification must exercise these deadlines
under the intended load. Watchdog renewal requires a fresh ownership exchange,
not an independent timer that could keep stale ownership alive.
A frozen Windows sampler, EOF,
process loss, malformed input, stale reply, identity change or resource refusal
cannot renew ownership. A frozen native event loop cannot renew the watchdog.
Inherited credential, SSH, proxy and executable-override environments are not
forwarded into the native bootstrap or its command probes.

The opted-in engine has a **five-second stop timeout**, followed by cgroup-wide
forced termination, rather than the ordinary seventy-minute drain allowance.
The heartbeat has a two-second stop bound. The attached transaction waits for
the engine to become inactive/failed with no main PID and an empty cgroup before
acknowledging `drained`. Killing only the client is never a drain receipt.
During shutdown, a disappearing or already-exited matching leader is only
progress, not proof of drain. Permission failures, corrupt observations and
changed ownership still refuse; terminal service and cgroup evidence remain
required. Live monitoring never accepts an absent or exited leader.
The verifier retains the owned cgroup path even after systemd clears its
metadata. A missing population counter is not emptiness: the directory must
be demonstrably removed under an observable parent, and terminal ownership is
rechecked after the cgroup observation.
A refusing heartbeat prints its bounded reason; after the drain proof the
transaction repeats at most 512 sanitized bytes of that native diagnostic.
Refusal does not restart or retry. Retain run history and inspect interrupted
work before an explicit new admission.

This intentionally changes **process-exit recovery**, not the engine's
continuing-pass retries. The released CLI retries failures inside `continuous()`,
but `finish_startup()` drains and returns an initial-pass error. Under this
profile an initial config/forge failure or any actual process exit therefore
requires explicit re-admission, instead of the ordinary Linux unit's
sixty-second restart. The one-use grant is not renewed to recover a failed start.

An operator can stop the owned engine or heartbeat using the service manager.
First stop closes admission; the short profile timeout still applies. For a
long planned drain, stop new work and let it finish **before** stopping this
transaction. Do not replace the heartbeat or restore the ordinary long grace
period while the supervised transaction is running.

## Host limits are not extra capacity

The Windows configuration must retain or tighten these supplied limits:

| Observation | Limit |
|---|---:|
| VHD stop threshold | 15,032,385,536 bytes |
| Backing-volume free minimum | 8,589,934,592 bytes (8 GiB) |
| Windows available physical memory minimum | 1,073,741,824 bytes (1 GiB) |

Every startup additionally requires **1 GiB working reserve below the VHD stop**.
At the supplied stop value, admission requires both observed VHD sizes to be at
most **13,958,643,712 bytes**. The supervisor rechecks this on every start,
including a later logon task launch with already-approved configuration. The
continuous stop threshold remains unchanged; approval is not a cached resource
admission.

Startup RAM admission is also stronger than the continuous emergency floor.
Windows must freshly observe the reviewed **8 GiB engine plus 128 MiB guardian
allocation plus the configured Windows free-memory floor**: at the default
floor, **9,797,894,144 bytes**. The shared, reviewed
`deployment/windows/native-budget.json` supplies both allocations; native
admission checks the loaded engine bound and constructs the guardian bound from
that same contract. The Windows bundle pins it with the other reviewed bytes.
Recheck before launch and before initial heartbeat authorization. Guest Linux
`MemAvailable` and guest cgroup headroom cannot substitute for Windows physical
RAM. These are additional startup admission margins, not higher service ceilings or
a raised continuous stop floor.

Observe both the VHD file's logical end-of-file and its physical allocation.
Neither is the guest filesystem's virtual capacity. Pin the registered
distribution, canonical VHD path, file identity and backing volume; a changed
identity, inaccessible counter, stale sample or API failure is not zero usage
and not a safe reading. Keep the existing Linux filesystem/cgroup/output/cache
guards; Windows free space is not a substitute for Linux free space.

For perspective, a 14,877,196,288-byte VHD has only **155,189,248 bytes** below
this stop threshold. It is **ineligible for startup** under the required 1 GiB
reserve. That observation does not qualify this host for a build,
authorize increasing the cap, or request compaction or cleanup. Sampling does
not reserve space or memory and cannot prevent other users from exhausting a
shared host between samples. Separate qualification must establish adequate
workload headroom and service hard limits; otherwise keep approval off.

## Separate installation and qualification

No repository script registers or starts a Scheduled Task. Preparation emits a
disabled, reviewable task document. Qualification and installation remain
explicit operator actions outside ordinary source work.

Before opting in, independently provision and qualify the native deployment in
[deployment.md](deployment.md), its existing credentials and runtime, and the
Windows bundle's reviewed digests and explicit non-secret configuration. Use a
dedicated protected copy, not a changing development worktree. Keep unapproved
configuration inactive. This profile additionally requires the root-protected
system `/usr/bin/python3` (3.8 or newer) with Linux `SO_PEERCRED`, solely for that
kernel credential bridge; it never installs this prerequisite.

With the ordinary service stopped and drained, a separately authorized operator
installs the exact reviewed `bureau-maintenance.service` at
`/etc/systemd/system/bureau-maintenance.service` and the opt-in drop-in at
`/etc/systemd/system/bureau-maintenance.service.d/windows-supervision.conf`.
Preserve the already-required reporter drop-in at
`/etc/systemd/system/bureau-maintenance.service.d/20-reporter-credential.conf`,
matching the reviewed template byte-for-byte. Native base environment, binary,
settings and credential contents are not rewritten by these launchers.
Remove ordinary boot enablement, reload the reviewed configuration, and verify
the effective profile before qualification. Install the source at the existing
read-only `/opt/bureau/source` and leave the approval/config SHA pin explicit.
Provision the root-protected native approval from
`deployment/windows-supervision.json.example` only after qualifying the exact
binary/source pair; source checkout identity alone is not binary provenance.
Do not start the engine directly. The exact-distro foreground entry invokes
`deployment/windows-entry.sh` with the reviewed commit; it creates no alternative
engine command and never provisions missing prerequisites.

Any prepared task uses the explicitly selected Windows user's **logon**
identity and interactive token, because WSL distribution registration is
per-user. It is independent of the Copilot session, **not a qualified boot-time
service before login**, and does not promise survival of logout or Windows
shutdown. No `SYSTEM`, S4U, password-storage or hidden keepalive fallback is
authorized. Task registration, enablement, first start, and native installation
must each remain separate approved operator work.

### Windows bundle and preparation commands

Use 64-bit PowerShell 7.4 or newer. Copy the reviewed `deployment/windows`
directory to a protected, stable location and author a separate non-secret
configuration from `config.example.json`. Keep `approved` and `qualified` false
until independent qualification is complete. Supply the selected Windows SID,
exact HKCU registration GUID/name, reviewed commit, canonical VHD path and
by-handle volume/file pins. `sourceSha256` must cover all nine named bundle files,
including `native-budget.json`. The configuration and verified source files are
held open against replacement while the foreground supervisor runs.

The separately authorized foreground entry is:

```powershell
& 'C:\Program Files\PowerShell\7\pwsh.exe' -NoProfile -NonInteractive `
  -File 'C:\Bureau\source\deployment\windows\supervise.ps1' `
  -ConfigPath 'C:\Bureau\supervision.json'
```

Preparation alone writes disabled XML; it never registers or starts a task:

```powershell
& 'C:\Program Files\PowerShell\7\pwsh.exe' -NoProfile -NonInteractive `
  -File 'C:\Bureau\source\deployment\windows\prepare-task.ps1' `
  -UserSid '<selected-Windows-user-SID>' `
  -PwshPath 'C:\Program Files\PowerShell\7\pwsh.exe' -PwshSha256 '<reviewed-sha256>' `
  -SupervisorPath 'C:\Bureau\source\deployment\windows\supervise.ps1' `
  -SupervisorSha256 '<reviewed-sha256>' `
  -ConfigPath 'C:\Bureau\supervision.json' -ConfigSha256 '<reviewed-sha256>' `
  -OutputPath 'C:\Bureau\disabled-logon.xml'
```

The placeholders deliberately refuse execution. Preparation pins the three
named inputs, requires the same selected SID as the configuration, refuses
reparse paths and existing output files, and emits no password or registration
operation. Those observations do not replace protected installation or future
qualification. A new logon always performs fresh resource admission.

## Evidence and remaining qualification

Bounded Node tests execute the real native heartbeat loop, framing, one-use
admission, process-identity parsing, foreground subprocess transaction and drain
logic. PowerShell tests execute the real Windows supervision logic with harmless
foreground fixtures and injected host observations. They do not call WSL, use
operator secrets, reconcile work, contact a model or contact a forge.

The required hosted supervision gate separately exercises actual systemd
start/readiness/dependency/stop behavior. It uses unique test-owned dummy units,
the production drop-in and supervision logic, private networking, finite
memory/CPU/task/runtime limits and explicit cgroup drain. Cases include initial
EOF/no ownership, partial startup, failed engine start, healthy ownership,
manual-restart refusal, post-admission EOF/oversize input, a frozen Windows
writer, native process loss and native watchdog expiry. It never launches the
Bureau engine. The actual pre-start runs as an unprivileged dummy user, cannot
write the grant directory or read the root lease, and rejects a forged
`ControlPID` request from a second process before admitting the real pre-start.
Failed fixture cleanup fails the job.

Those gates are evidence for this source contract, not certification of a
particular Windows user, WSL installation, mounted VHD, service account, model
authentication, shutdown/sleep behavior or real maintenance workload. Those
remain separately controlled deployment qualification. Never treat a mock,
static unit-file assertion, skipped test or green Rust suite as live systemd
or host qualification.
