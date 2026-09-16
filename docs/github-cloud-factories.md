# GitHub cloud factories

Bureau can experimentally list existing GitHub cloud automations, submit them,
and monitor an explicitly selected task. Availability is limited. These are
**not** local Copilot SDK factories or Bureau agent steps.

The integration calls internal CMC endpoints on `api.github.com`. This is not
a supported public GitHub REST factory API. Bureau uses its own user agent and
authenticates with the selected repository's declared credential via the
`Bearer` HTTP scheme. Client compatibility, token entitlement and enterprise
support are not established by offline tests; no live entitlement proof is
claimed. Authentication and eligibility errors are surfaced without fallbacks
or rollout bypasses. No first-party application identity or application token
is used.

## Before using it

The repository must already be registered in committed Bureau configuration
as a dotcom GitHub repository, with a credential reference declared in local
settings. Initial network commands require `--expected-login`; Bureau checks
the authenticated user rather than assuming that a resolved token is the
intended credential. Later refreshes verify the recorded numeric user ID,
so credential rotation or a login rename cannot select a different account.

Dispatch requires reviewed registry `access: push`. `read` and `pr` entries
can be inspected but cannot dispatch. This is a conservative admission rule,
not enforcement of the remote automation's tool permissions. Review the
automation's prompt, tools, permissions, and repository before submitting it.
No command automatically enables an automation or changes reviewed grants.

Replace the example registry name, login, and IDs below with exact values.
Use `--settings` and `--config-cache` for the existing local-path overrides.
`--json` emits compact structured output; default output is readable JSON.

## Inspect existing automations

```sh
bureau list --github-cloud --repo code --expected-login YOUR_LOGIN --json
bureau show --github-cloud --repo code --expected-login YOUR_LOGIN \
  --automation AUTOMATION_ID --json
```

Inventory is repository-scoped. Definition lookup first verifies membership
in that inventory; a missing repository field on the definition is never
replaced with fabricated identity.

Run now supports only an empty trigger map (`manual`) or exactly one
`interval`/`schedule` key (`interval`). Mixed or event-driven triggers are
rejected. Bureau also conservatively rejects disabled automations because
their manual-dispatch behavior is not established. It never enables them.

## Submit once and keep the receipt

```sh
bureau run --github-cloud --repo code --expected-login YOUR_LOGIN \
  --dispatch-automation AUTOMATION_ID --request-id review-request-42 --json
```

**Acceptance is not task completion, and does not identify a task.** Bureau's
HTTP client treats a 2xx status as submission acceptance and discards the body.
It assumes no response-task-ID, task-correlation or server idempotency contract.

`--request-id` is a stable local key, not a cloud task ID. Use ASCII letters,
digits, hyphens, or underscores, at most 128 bytes. Bureau persists a send
intent before the POST. Repeating a matching key returns the existing receipt
without another POST. Reusing it for a different target or principal fails.

If delivery or durable outcome recording is uncertain, the receipt remains
uncertain. Do not respond by inventing a new key and submitting again:
that can create duplicate paid work. Inspect the remote history and decide
explicitly what to do. Bureau never matches a receipt to the newest task.

```sh
bureau show --github-cloud review-request-42 --json
```

This reads the durable local record and requires no network credentials.
`prepared` in the underlying record means that a send may have occurred;
the user-facing submission status is `uncertain`.

## Select an exact task for monitoring

```sh
bureau list --github-cloud --repo code --expected-login YOUR_LOGIN \
  --automation AUTOMATION_ID --json

bureau run --github-cloud --repo code --expected-login YOUR_LOGIN \
  --automation AUTOMATION_ID --track-task TASK_ID \
  --request-id review-request-42 --json
```

The second command does **not** submit work. It records an operator-selected
task after checking its exact task ID, automation attribution, and session
task IDs. A new local key can monitor an existing task without any preceding
submission. An existing selection cannot silently change to a different
task, and archived tasks cannot be newly attached.

The association remains `operator_selected_unproven`: it is not proof that
the submission created the task.

```sh
bureau show --github-cloud review-request-42 --refresh --json
bureau show --github-cloud review-request-42 --refresh --events --json
```

Refresh fetches and records observations of the exact selected task.
Without `--refresh`, `show` reports the last stored observation with its
timestamp. Requesting events before an event snapshot was fetched is an
explicit error, not an empty successful result.

Task `state` and `status` are displayed independently. Session IDs remain
distinct from task and automation IDs. Unknown states and event types are
preserved. With event refresh, Bureau re-reads the task afterward; old
transcript completion cannot overwrite a newer `waiting_for_user` state.
There is no snapshot-isolation guarantee across paginated reads.

Artifacts are remote JSON references, not downloaded files. Bureau does not
follow their URLs with credentials, interpret their contents as executable
instructions, or parse embedded `v2` JSON as a pipeline result.

## Controls that are deliberately unavailable

Remote cancellation, pause, resumption, retries, approvals, and feedback are
unsupported: Bureau has no supported, authorized HTTP contract for them.
For example:

```sh
bureau cancel --github-cloud review-request-42 --json
bureau resume --github-cloud review-request-42 --json
```

These exit nonzero and explicitly report `request_sent: false`. They do not
write pipeline control markers or send steering requests. A remote
`remote_steerable: true` field does not grant Bureau authorization. Use an
independently authorized cloud interface for controls it supports.

Stopping Bureau or refreshing monitoring does not stop or resume the remote
workload. Remote execution owns its environment, permissions, billing, and
lifetime. Local pipeline budgets, deadlines, worktrees, plugins, and
credentials are not inherited by the cloud task. Unreported cost is unknown,
not zero.

## Storage, limits, and exit behavior

Cloud receipts live at
`BUREAU_HOME/github-cloud-runs/<request-id>/events.jsonl`, separate from
pipeline runs. They use checked, scrubbed, lease-fenced appends and strict
replay; no `state.json` cache is required. No task worktree or artifact
directory is created. Use the same state database and record root for all
cooperating processes; `--runs` and `--state` are explicit cloud-operation
overrides, not remote identity settings.

Requests have a 30-second timeout and do not follow redirects. Reads request
100 items per page and stop with an explicit error when more data remains
after 100 pages. Client response/read limits are 16 MiB per response and
64 MiB per paginated operation. These are Bureau limits, not claims about
server maximums. Continuations cannot change origin, repository, or query
scope. Known remote identities are deduplicated; unidentified raw events are
preserved rather than assigned invented IDs.

Exit 0 means the requested inspection, recorded observation, or submission
acceptance was handled, **not that a task succeeded**. Dispatch exits 0 only
with recorded acceptance; every other submission state exits 1. Replaying a
creation-only or tracking-only receipt reports `not_submitted`, exits 1, and
sends no POST. Tracking and read-only inspection of those receipts still exit
0. Invalid selections, unsupported controls,
ownership failures, and unrecorded errors exit 2. A failed task can still be
read successfully; its remote failure remains visible in the data.

The source-level client is `forge::github::cloud`, reached through
`GitHubForge::cloud()`. The durable public operations are in `github_cloud`.
Neither extends the cross-forge `Forge` trait or changes ordinary GitHub/ADO,
ACP, pipeline, watch, or canvas behavior.
