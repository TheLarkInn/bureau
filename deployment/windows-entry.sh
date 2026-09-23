#!/usr/bin/env bash
set -euo pipefail

[[ $# == 1 && "$1" =~ ^[a-f0-9]{40}$ ]] || { echo "reviewed commit required" >&2; exit 1; }
[[ $EUID == 0 ]] || { echo "supervision requires the provisioned root entry point" >&2; exit 1; }
# /run is root-owned. The lock survives exec and covers startup through cgroup drain.
exec /usr/bin/flock --nonblock --no-fork /run/bureau-windows-owner.lock \
  /usr/bin/env -i PATH=/opt/bureau/rust/bin:/opt/bureau/bin:/usr/local/bin:/usr/bin:/bin \
  node /opt/bureau/source/deployment/supervision/transaction.mjs "$1"
