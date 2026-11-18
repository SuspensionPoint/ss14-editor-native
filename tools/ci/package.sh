#!/usr/bin/env bash
# Puts the editor, its engine side and its start script in one folder and zips it. Used by CI
# (.github/workflows/package.yml, docs/ci.md).
#
#   tools/ci/package.sh <editor program> <engine side folder> <zip path>
#
# The zip holds one folder named as the zip:
#
#   ss14-editor(.exe)           the editor
#   engine/                     the engine side, stamped with its code fingerprint
#   src/, patches/engine/, ...  the editor's C# sources and engine patches, which the editor
#                               builds an engine side for a fork's own code from
#                               (editor-core forks/engines.rs, Sources)
#   start.cmd or start.sh       starts the editor on its start screen
#   LICENSE                     the editor's licence
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
editor="${1:?usage: $0 <editor program> <engine side folder> <zip path>}"
engine="${2:?usage: $0 <editor program> <engine side folder> <zip path>}"
zip="${3:?usage: $0 <editor program> <engine side folder> <zip path>}"
mkdir -p "$(dirname "$zip")"
zip="$(cd "$(dirname "$zip")" && pwd)/$(basename "$zip")"
name="$(basename "$zip" .zip)"

[ -f "$engine/code-fingerprint.txt" ] || { echo "$engine has no code fingerprint: stamp it with --stamp-engine first" >&2; exit 1; }

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
pkg="$stage/$name"
mkdir -p "$pkg"

cp "$editor" "$pkg/"
cp "$ROOT/LICENSE" "$pkg/"
cp -R "$engine" "$pkg/engine"

# The files engines.rs builds an engine side from, as git has them.
(cd "$ROOT" && git ls-files -z -- \
  src/Editor.Kernel src/Editor.Native src/Editor.ViewKernel src/Content.EditorView \
  src/Directory.Build.props src/Directory.Build.targets Directory.Packages.props nuget.config \
  patches/engine | tar --null -T - -cf -) | tar -xf - -C "$pkg"

case "$editor" in
  *.exe) cp "$ROOT/tools/package/start.cmd" "$pkg/" ;;
  *)
    cp "$ROOT/tools/package/start.sh" "$pkg/"
    chmod +x "$pkg/start.sh" "$pkg/$(basename "$editor")"
    ;;
esac

rm -f "$zip"
if command -v zip >/dev/null 2>&1; then
  # Keeps the programs' executable bits.
  (cd "$stage" && zip -qr -y "$zip" "$name")
elif command -v 7z >/dev/null 2>&1; then
  (cd "$stage" && 7z a -tzip -bd -bso0 "$zip" "$name")
else
  (cd "$stage" && powershell -NoProfile -Command "Compress-Archive -Path '$name' -DestinationPath '$(cygpath -w "$zip")'")
fi
echo "packed $zip ($(du -h "$zip" | cut -f1))"
