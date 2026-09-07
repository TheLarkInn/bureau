# Automated releases

`CI` is the entry point for pull requests, pushes to `main`, manual recovery,
and a daily recovery run. A release requires all of:

- The complete `scripts/lint.sh` gate, including offline canvas/script tests,
  the Playwright PR suite, repository policy, rustfmt, clippy, and dylint.
- The exhaustive Canvas state matrix and approved visual snapshots.
- Offline Rust unit/integration tests on native Linux x86-64 and ARM64
  runners, including the fake pipeline end-to-end tests.
- Locked, static-musl release builds on both architectures and an
  extraction/CLI smoke test for each packaged archive.

Browser dependencies are installed in CI so lint's local optional-browser
behavior cannot silently skip the PR suite. Each build/test job has read-only
repository permissions and checks out the same immutable commit.

## Release sequence

After the initial release, a successful main run lets release-plz prepare a version/changelog-only
release PR. CI explicitly dispatches another run for its exact head SHA.
That run executes every gate above, reports `All checks passed` on that SHA,
and merges the PR using GitHub's SHA-conditional squash-merge API.
It never uses an administrator bypass or suppresses a required review.

The merged release commit gets another complete main CI run. Only then does
release-plz create its tag and a **draft** GitHub release. CI downloads the
artifacts from that same run, verifies that every supported target and
checksum is present, uploads all assets, and finally publishes the release.
A failed upload leaves a draft, never an apparently complete public release.

The automation uses the repository's built-in `GITHUB_TOKEN`; no PAT, GitHub
App, crates.io account, or crates.io publishing token is needed. Token-created
PRs do not provide unattended ordinary PR runs (GitHub may leave those runs
awaiting approval), and token-created merge/tag/release events do **not**
start ordinary GitHub Actions workflows. The explicit `workflow_dispatch` calls after PR creation
and merge are essential, as is publishing assets in the same workflow rather
than depending on a `release: published` event.

Only an open, same-repository release-plz PR authored by `github-actions[bot]`,
labeled `release`, and based on current `main` is eligible for automatic
merging. Its diff must be exactly one commit touching only the known version,
lockfile, and changelog files. Both its head and main's head are rechecked
after validation. Ordinary feature PRs are never auto-merged by this pipeline.

## Version intent

Squash-merge feature PRs. CI requires a Conventional Commit PR title; preserve
that title as the squash commit's subject:

| Title | Version intent |
|---|---|
| `fix: ...`, `perf: ...` | Patch |
| `feat: ...` | Minor, including before 1.0 |
| `feat!: ...`, `fix!: ...` | Breaking change: next minor before 1.0, next major after 1.0 |
| `docs: ...`, `test: ...`, `ci: ...`, `chore: ...` | Maintenance changes; release-plz determines whether the shipped package changed |

A `BREAKING CHANGE:` footer in the squash commit body also records breaking
intent. Version selection is deterministic commit metadata, not an AI guess:
authors still need to identify breaking CLI/config behavior correctly.
The first successful main run releases the current `0.1.0` directly to
establish a baseline: there is no previous release to compare. Subsequent
version changes arrive through release-plz PRs. `release_always = true`
keeps tagging on the exact gated commit rather than letting release-plz
select an older PR commit; existing version tags prevent duplicate releases.
Internal crates share the workspace version;
only `bureau` owns a public tag and GitHub release. Nothing is sent to crates.io.

## One-time repository setup

The workflows must first land on `main`. In **Settings > Actions > General**,
enable **Allow GitHub Actions to create and approve pull requests**. The
automation needs the *create* permission; it does not approve its own PRs.
Default workflow permissions can remain read-only because write permissions
are scoped to specific automation jobs.

Create the `release` label used by release-plz and the release-PR identity
guard. Keep only squash merging enabled, use the PR title as the default squash commit
title, and retain PR bodies for breaking-change footers. Branch protection
must require **All checks passed** from GitHub Actions and **Require branches
to be up to date before merging**, including administrators. The latter is
the server-side protection against main advancing between CI's final base
check and the merge request; the REST `sha` parameter only protects the PR
head. Do not require a human
approval for release PRs if releases must be unattended. An approval
requirement applying to every PR will intentionally block the merge; this
pipeline does not evade it. GitHub's repository-level **Allow auto-merge**
setting is not necessary: CI invokes the ordinary merge API after its gates.

Before each release-plz invocation, CI attaches a local `main` branch to the
immutable tested SHA and sets its upstream. A SHA checkout alone leaves
detached HEAD, which release-plz rejects. This does not switch to newer
untested remote-main content.

The release-head check is posted explicitly because a dispatched workflow
otherwise associates its checks with main's commit rather than the release
PR commit it actually tested. Restrict pushes to main in your repository
rules if direct pushes must not bypass the normal PR-title/review policy.

## Platforms

| Asset target | Where it runs |
|---|---|
| `x86_64-unknown-linux-musl` | x86-64 Linux and x86-64 WSL2 |
| `aarch64-unknown-linux-musl` | ARM64 Linux and ARM64 WSL2; ARM64 Linux VMs on Apple Silicon |

These are static Linux binaries, not native Windows/macOS executables.
Native ports require equivalent process-tree isolation, signal handling,
file identity/permission semantics, and offline contract tests. Simply
cross-compiling, dropping the Linux sandbox, or publishing a binary that
cannot execute a pipeline would misrepresent support.

Archives contain `bureau`, the license, and the README. Git, `unshare`, Node.js
for the dashboard, and agent CLIs remain explicit runtime dependencies.
Linux user namespaces must be enabled; CI enables them on disposable hosted
runners before executing the process tests.

## Recovery

Re-run a failed CI run to retry transient failures. Upload retries can replace
partial assets on the matching draft, but never overwrite published assets.
A draft's tag must resolve to the exact tested commit; a newer commit's
binary is never attached to an older release.

To refresh a stale release PR, dispatch `CI` on `main` with `release_pr` empty.
To retry its checks without a new version calculation, dispatch with the PR
number. Main advancing or the PR head changing during validation blocks
merging; a subsequent main run refreshes it. An unchanged, still-open release
PR is also dispatched again rather than being left waiting for an event that
will never arrive.

The daily main run recovers a missed dispatch or partial draft. A pending
draft takes priority: CI resolves its tag, requires that it belongs to main's
history, reruns every gate on that exact commit, rebuilds its artifacts, and
finishes publication. It then dispatches current main to resume new releases.
Unrelated or multiple pending drafts fail closed rather than guessing which
version to ship.
