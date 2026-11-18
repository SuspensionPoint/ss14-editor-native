#!/usr/bin/env bash
# Writes the engine checkout's current changes to a variant of an engine patch unit.
#   tools/export-engine-patch.sh <unit> <variant> <path> [path ...]
# as: tools/export-engine-patch.sh 0004-rsi-layout-without-images starlight Robust.Client/...
# Paths are relative to the engine checkout. New files are included. A new unit also needs its
# unit.txt (see tools/engine-patches.sh).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENGINE="$ROOT/upstream/starlight/RobustToolbox"
unit="$1"; variant="$2"; shift 2
mkdir -p "$ROOT/patches/engine/$unit"
out="$ROOT/patches/engine/$unit/$variant.patch"
git -C "$ENGINE" add -N -- "$@"
git -C "$ENGINE" diff -- "$@" > "$out"
git -C "$ENGINE" reset -q -- "$@"
echo "wrote patches/engine/$unit/$variant.patch ($(grep -c '^[+-]' "$out") changed lines)"
