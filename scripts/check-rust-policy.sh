#!/usr/bin/env bash
set -euo pipefail

max_file_lines=300
failures=0

while IFS= read -r -d '' source_file; do
    line_count=$(wc -l < "$source_file")
    if ((line_count > max_file_lines)); then
        printf '%s has %s lines; Rust source files may contain at most %s lines.\n' \
            "$source_file" "$line_count" "$max_file_lines" >&2
        failures=$((failures + 1))
    fi

    if grep -zEq '#!?[[:space:]]*\[[[:space:]]*(allow|expect)[[:space:]]*\(' "$source_file"; then
        printf '%s contains a forbidden lint-suppression attribute.\n' "$source_file" >&2
        failures=$((failures + 1))
    fi
done < <(find . -type f -name '*.rs' -not -path './.git/*' -not -path './target/*' -print0)

if ((failures > 0)); then
    exit 1
fi
