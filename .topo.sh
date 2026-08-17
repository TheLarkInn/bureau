#!/bin/bash
cd /home/selarkin/src/copilot-worktrees/bureau/thelarkinn-super-funicular
awk '/^warning: items are not in topological order|^warning: `impl/{f=""} /-->/{if ($2 != "") f=$2} {print}' .dylint-run.txt > /dev/null
# simpler: split output into per-finding blocks
python3 - <<'EOF'
import re
text = open('.dylint-run.txt').read()
blocks = re.split(r'\n(?=warning: )', text)
out = {}
for b in blocks:
    m = re.search(r'--> (crates/\S+):(\d+)', b)
    if not m: continue
    f = m.group(1)
    if '/cli' not in f: continue
    kind = b.split('\n')[0]
    if 'topological' not in kind and 'separated' not in kind: continue
    refs = re.findall(r'`([^`]+)` references `([^`]+)` but appears before it', b)
    defs = re.findall(r'`([^`]+)` defined here', b)
    out.setdefault(f, []).append((kind[:60], refs, defs))
for f, items in sorted(out.items()):
    print(f'### {f}')
    for kind, refs, defs in items:
        print(' ', kind)
        for a, b in refs:
            print(f'    {a} -> {b}')
EOF