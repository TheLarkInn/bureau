#!/usr/bin/env bash
set -euo pipefail

target="${1:?expected a Rust target}"
destination="${2:?expected an artifact directory}"
case "$target" in
    x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;;
    *) echo "unsupported release target: $target" >&2; exit 1 ;;
esac

binary="target/$target/release/bureau"
version="$("$binary" --version)"
version="${version#bureau }"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "unexpected bureau version: $version" >&2
    exit 1
fi

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
install -m 755 "$binary" "$stage/bureau"
install -m 644 LICENSE README.md "$stage/"
mkdir -p "$destination"
archive="bureau-v$version-$target.tar.gz"
tar -czf "$destination/$archive" -C "$stage" bureau LICENSE README.md
(cd "$destination" && sha256sum "$archive" > "$archive.sha256")

mkdir "$stage/unpacked"
tar -xzf "$destination/$archive" -C "$stage/unpacked"
test "$("$stage/unpacked/bureau" --version)" = "bureau $version"
"$stage/unpacked/bureau" --help > /dev/null
