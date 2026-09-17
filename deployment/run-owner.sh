#!/usr/bin/env bash
set -euo pipefail

: "${BUREAU_HOME:?set an explicit dedicated state root}"
: "${BUREAU_CONFIG_COMMIT:?set the reviewed full config commit}"
: "${BUREAU_DEPLOYMENT_APPROVED:?explicit deployment approval is required}"
[[ "$BUREAU_DEPLOYMENT_APPROVED" == true ]] || { echo "deployment is not approved" >&2; exit 1; }
[[ "$BUREAU_CONFIG_COMMIT" =~ ^[a-f0-9]{40}$ ]] || { echo "config commit must be a full SHA" >&2; exit 1; }
[[ "$(git rev-parse HEAD)" == "$BUREAU_CONFIG_COMMIT" ]] || { echo "installed source differs from the reviewed commit" >&2; exit 1; }

node deployment/check.mjs --home "$BUREAU_HOME"
/opt/bureau/bin/bureau validate .bureau/maintenance

# All supported service/container starts take the same external flock. Never
# replace the lock file or start another reconcile/run path around that lock.
exec /opt/bureau/bin/bureau reconcile \
  --settings "$BUREAU_HOME/settings.yaml" \
  --config-remote https://github.com/TheLarkInn/bureau.git \
  --config-ref "$BUREAU_CONFIG_COMMIT" \
  --config-subdir .bureau/maintenance \
  --config-cache "$BUREAU_HOME/config-cache" \
  --runs "$BUREAU_HOME/runs" \
  --state "$BUREAU_HOME/state.db" \
  --cache "$BUREAU_HOME/checkout-cache" \
  --interval 5m
