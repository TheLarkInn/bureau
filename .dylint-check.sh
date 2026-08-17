#!/bin/bash
cd /home/selarkin/src/copilot-worktrees/bureau/thelarkinn-super-funicular
export LIBRARY_PATH="$(rustc +nightly-2026-01-22 --print sysroot)/lib"
RUSTFLAGS="--cap-lints warn" cargo dylint --all -- -p bureau > .dylint-run.txt 2>&1
echo "=== by message ==="
grep -E "^warning: |^error: " .dylint-run.txt | sort | uniq -c | sort -rn
echo "=== by file ==="
grep -oE "\-\-> [^ :]+" .dylint-run.txt | sort | uniq -c | sort -rn