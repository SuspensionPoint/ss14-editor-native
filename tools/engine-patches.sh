#!/usr/bin/env bash
# Manages the engine patches in patches/engine/.
#
# The patches are units, each adding one ability to the engine: a folder (numbered, applied in
# name order) with a manifest, unit.txt, and one patch per shape of engine it was made for. The
# manifest says what the unit adds, whether the native engine side needs it, and how to tell an
# engine has it already ("has: <file> <text>"), and, if only some engines need it, which they are
# ("for: <file>": the engines with that file). A unit the engine has is left out, else the first
# of its patches that fits goes on. The editor's own build for a fork's engine does the same with
# the units native needs (native/editor-core/src/forks/engines.rs).
#
# The engine checkout (upstream/starlight/RobustToolbox) is the fork's pinned commit with every
# unit on it. Nothing is committed inside the engine checkout: the patch files
# in this repo are the source of truth.
#
#   tools/engine-patches.sh status   show whether the engine checkout matches the units
#   tools/engine-patches.sh apply    reset the engine checkout to pristine, then apply every unit
#   tools/engine-patches.sh reset    discard all engine changes (back to the pinned commit)
#   tools/engine-patches.sh check    verify every unit still goes on the pristine checkout
#   tools/engine-patches.sh fit <engine folder> [--native]
#                                    how each unit goes on another engine (a git checkout, at its
#                                    commit), tried in a copy in a temporary folder. --native
#                                    tries only the units the native engine side needs.
set -euo pipefail
export LC_ALL=C

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENGINE="$ROOT/upstream/starlight/RobustToolbox"
PATCHES="$ROOT/patches/engine"

reset_engine() {
  git -C "$ENGINE" reset -q --hard
  git -C "$ENGINE" clean -q -fd -- Robust.Shared Robust.Server Robust.Client Robust.Server.Testing MSBuild Imports
}

units() {
  for unit in "$PATCHES"/*/; do
    [ -f "$unit/unit.txt" ] && echo "${unit%/}"
  done
}

# Whether an engine has a unit's ability: one of the manifest's has lines holds.
has_unit() {
  local engine="$1" unit="$2" line file text
  while IFS= read -r line || [ -n "$line" ]; do
    line="${line%$'\r'}"
    case "$line" in has:*) ;; *) continue ;; esac
    line="${line#has:}"
    line="${line#"${line%%[! ]*}"}"
    file="${line%% *}"
    text="${line#* }"
    [ -f "$engine/$file" ] && grep -qF -- "$text" "$engine/$file" && return 0
  done < "$unit/unit.txt"
  return 1
}

native_unit() {
  grep -q '^native: *yes' "$1/unit.txt"
}

# Whether a unit is for an engine: its manifest names no "for: <file>", or the engine has it.
unit_for() {
  local engine="$1" unit="$2" file
  file="$(sed -n 's/^for: *//p' "$unit/unit.txt" | tr -d '\r' | head -1)"
  [ -z "$file" ] || [ -f "$engine/$file" ]
}

# Puts one unit on an engine and says how: "had it", "not for this engine" or the patch applied.
# Fails, saying what git said of each patch, when none fits.
apply_unit() {
  local engine="$1" unit="$2" variant
  if ! unit_for "$engine" "$unit"; then
    echo "not for this engine"
    return 0
  fi
  if has_unit "$engine" "$unit"; then
    echo "had it"
    return 0
  fi
  for variant in "$unit"/*.patch; do
    if git -C "$engine" apply --check --whitespace=nowarn "$variant" 2>/dev/null; then
      git -C "$engine" apply --whitespace=nowarn "$variant"
      if ! has_unit "$engine" "$unit"; then
        echo "$(basename "$variant") went on, but the engine does not have what unit.txt looks for"
        return 1
      fi
      basename "$variant"
      return 0
    fi
  done
  for variant in "$unit"/*.patch; do
    echo "$(basename "$variant"): $(git -C "$engine" apply --check --whitespace=nowarn "$variant" 2>&1 | head -1)"
  done
  return 1
}

# Every unit on an engine, each said. Stops at the first that does not fit unless asked to go on.
apply_units() {
  local engine="$1" only_native="$2" go_on="$3" unit said failed=0
  while IFS= read -r unit; do
    if [ "$only_native" = 1 ] && ! native_unit "$unit"; then
      continue
    fi
    if said="$(apply_unit "$engine" "$unit")"; then
      printf 'ok      %-45s %s\n' "$(basename "$unit")" "$said"
    else
      printf 'FAILED  %-45s does not fit\n' "$(basename "$unit")"
      printf '%s\n' "$said" | sed 's/^/          /'
      failed=1
      [ "$go_on" = 1 ] || return 1
    fi
  done < <(units)
  return "$failed"
}

case "${1:-status}" in
  status)
    echo "engine at $(git -C "$ENGINE" rev-parse --short HEAD) ($(grep -o '<Version>[^<]*' "$ENGINE/MSBuild/Robust.Engine.Version.props" | cut -d'>' -f2))"
    echo "units: $(units | wc -l)"
    git -C "$ENGINE" status --short
    ;;
  reset)
    reset_engine
    echo "engine checkout is pristine"
    ;;
  apply)
    reset_engine
    apply_units "$ENGINE" 0 0
    ;;
  check)
    scratch="$(mktemp -d)"
    trap 'git -C "$ENGINE" worktree remove --force "$scratch" >/dev/null 2>&1 || true' EXIT
    git -C "$ENGINE" worktree add -q --detach "$scratch" HEAD
    apply_units "$scratch" 0 0
    ;;
  fit)
    other="${2:?usage: $0 fit <engine folder> [--native]}"
    only_native=0
    [ "${3:-}" = "--native" ] && only_native=1
    scratch="$(mktemp -d)"
    trap 'rm -rf "$scratch"' EXIT
    git clone -q --shared --no-checkout -- "$other" "$scratch/engine"
    git -C "$scratch/engine" -c core.longpaths=true -c advice.detachedHead=false checkout -q --detach HEAD
    echo "engine at $(git -C "$scratch/engine" rev-parse --short HEAD) ($(grep -o '<Version>[^<]*' "$scratch/engine/MSBuild/Robust.Engine.Version.props" | cut -d'>' -f2))"
    apply_units "$scratch/engine" "$only_native" 1
    ;;
  *)
    echo "usage: $0 status|apply|reset|check|fit <engine folder> [--native]" >&2
    exit 2
    ;;
esac
