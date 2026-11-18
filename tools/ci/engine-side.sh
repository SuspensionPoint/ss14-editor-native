#!/usr/bin/env bash
# Builds the engine side (src/Editor.Native) against a fork checkout, as editor-core's
# forks/engines.rs builds one for a mapper's fork. Used by CI (.github/workflows/pr.yml and package.yml).
#
#   tools/ci/engine-side.sh <fork folder> <runtime> <output folder> [--patched]
#
# The engine patch units native needs go on the fork's RobustToolbox first, by the rules of
# engines.rs `apply_units`: a unit the engine has already is left out, else the first of its
# patches that git takes goes on, and it must then have what its unit.txt looks for. With
# --patched the engine has its units already (tools/engine-patches.sh apply) and none go on.
#
# Then the engine side is published with ReadyToRun for the runtime (win-x64, linux-x64,
# osx-arm64, osx-x64) and the engine's Resources are put beside it. The code fingerprint is
# written after, by the editor: <editor> --stamp-engine <output folder> <fork folder>.
set -euo pipefail
export LC_ALL=C

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
fork="$(cd "${1:?usage: $0 <fork folder> <runtime> <output folder> [--patched]}" && pwd)"
runtime="${2:?usage: $0 <fork folder> <runtime> <output folder> [--patched]}"
out="${3:?usage: $0 <fork folder> <runtime> <output folder> [--patched]}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
engine="$fork/RobustToolbox"

# Whether the engine has a unit's ability: one of its has lines holds.
has_unit() {
  local unit="$1" line file text
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

if [ "${4:-}" != --patched ]; then
  echo "== Applying the native engine patch units to $(git -C "$engine" rev-parse --short HEAD)"
  for unit in "$ROOT"/patches/engine/*/; do
    unit="${unit%/}"
    [ -f "$unit/unit.txt" ] || continue
    grep -q '^native: *yes' "$unit/unit.txt" || continue
    name="$(basename "$unit")"
    for_file="$(sed -n 's/^for: *//p' "$unit/unit.txt" | tr -d '\r' | head -1)"
    if [ -n "$for_file" ] && [ ! -f "$engine/$for_file" ]; then
      echo "ok      $name not for this engine"
      continue
    fi
    if has_unit "$unit"; then
      echo "ok      $name had it"
      continue
    fi
    chosen=""
    for variant in "$unit"/*.patch; do
      if git -C "$engine" apply --check --whitespace=nowarn "$variant" 2>/dev/null; then
        chosen="$variant"
        break
      fi
    done
    if [ -z "$chosen" ]; then
      echo "FAILED  $name: no patch of it fits this engine" >&2
      for variant in "$unit"/*.patch; do
        echo "  $(basename "$variant"): $(git -C "$engine" apply --check --whitespace=nowarn "$variant" 2>&1 | head -1)" >&2
      done
      exit 1
    fi
    git -C "$engine" apply --whitespace=nowarn "$chosen"
    has_unit "$unit" || { echo "FAILED  $name: $(basename "$chosen") went on, but the engine lacks what unit.txt looks for" >&2; exit 1; }
    echo "ok      $name with $(basename "$chosen")"
  done
fi

echo "== Publishing the engine side for $runtime"
cd "$ROOT"
# dotnet on Windows takes C:/ paths, not Git Bash's /c/ ones.
fork_root="$fork"
out_dir="$out"
if command -v cygpath >/dev/null 2>&1; then
  fork_root="$(cygpath -m "$fork")"
  out_dir="$(cygpath -m "$out")"
fi
# Space Wizards' Content.Server has a server_config.toml as Robust.Server has, which a publish
# refuses unless told. engines.rs does not pass this yet.
export MSBUILDDISABLENODEREUSE=1 DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
log="$(mktemp)"
publish() {
  dotnet publish src/Editor.Native -c Release -r "$runtime" --self-contained false \
    -p:PublishReadyToRun=true -p:TreatWarningsAsErrors=false "-p:ForkRoot=$fork_root/" \
    -p:ErrorOnDuplicatePublishOutputFiles=false \
    -o "$out_dir" -nologo -v:m -nodeReuse:false -clp:NoSummary 2>&1 | tee "$log"
  return "${PIPESTATUS[0]}"
}
# MSBuild may build a project twice at once and trip over its own files, as engines.rs knows.
# Once more mends it.
if ! publish; then
  grep -q "being used by another process" "$log" || exit 1
  echo "== Publishing again: a file was in use"
  publish
fi
rm -f "$log"

# The engine's Resources, for a checkout whose engine submodule is not set up
# (ResourcePack.EngineResources).
rm -rf "$out/RobustToolbox"
mkdir -p "$out/RobustToolbox"
cp -R "$engine/Resources" "$out/RobustToolbox/Resources"
echo "engine side in $out"
