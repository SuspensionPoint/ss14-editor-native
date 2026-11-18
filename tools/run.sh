#!/usr/bin/env bash
# Sets up and runs the editor from a clone of this repository, on macOS or Linux. The first
# run does everything a fresh clone needs. Later runs only redo what is missing or out of date.
#
#   tools/run.sh                                       the start screen, to choose a fork
#   tools/run.sh Maps/_Starlight/Stations/Cork.yml     open a map of upstream/starlight by its path
#   tools/run.sh --rebuild Maps/...                    build the engine side again
#
# From the start screen, the editor makes each fork's pack and textures itself and keeps them
# in its cache. Given a map, it runs on upstream/starlight instead, with a pack and textures
# this script makes once in native/cache. The engine side's `pack` makes the pack and the
# editor's `--bake` makes the textures. Every publish of the engine side writes the code
# fingerprint of upstream/starlight next to it, and the editor checks a fork against that
# before opening it.
#
# You need git, the .NET 10 SDK (https://dotnet.microsoft.com/download) and Rust
# (https://rustup.rs). On Linux you also need the system libraries listed as LINUX_PACKAGES in
# .github/workflows/pr.yml. The first run takes a while, since it fetches the fork and builds
# the engine.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

REBUILD=0
while [ $# -gt 0 ]; do
  case "$1" in
    --rebuild) REBUILD=1; shift ;;
    *) break ;;
  esac
done
MAP="${1:-}"

say() { printf '\n== %s\n' "$1"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "$1 was not found. $2" >&2; exit 1; }; }
need git "Install git (on macOS: xcode-select --install)"
need dotnet "Install the .NET 10 SDK from https://dotnet.microsoft.com/download"
need cargo "Install Rust from https://rustup.rs, then open a new terminal"
# The fork builds with the .NET 10 SDK, and global.json here and in the fork pin it, so a
# newer SDK installed beside it is not used.
if ! dotnet --list-sdks | grep -q '^10\.'; then
  echo "The .NET 10 SDK is needed. Found: $(dotnet --list-sdks | tr '\n' ' ')" >&2
  exit 1
fi

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) RID=osx-arm64 ;;
  Darwin-*) RID=osx-x64 ;;
  Linux-aarch64) RID=linux-arm64 ;;
  *) RID=linux-x64 ;;
esac

# The fork, at the commit this repository pins, with the engine inside it.
FORK="$ROOT/upstream/starlight"
ENGINE="$FORK/RobustToolbox"
if [ ! -f "$ENGINE/Robust.Shared/Robust.Shared.csproj" ]; then
  say "Fetching the fork (several GB, once)"
  git submodule update --init --recursive upstream/starlight
fi

# The engine's patch queue. A checkout with no changes of its own gets the patches.
if [ -z "$(git -C "$ENGINE" status --porcelain)" ]; then
  say "Applying the engine patches"
  tools/engine-patches.sh apply
fi

# The engine side, published for this machine. It is published again whenever its code changed
# since the last publish: the stamp is git's hash of the src folder and the patch queue.
BIN="$ROOT/src/Editor.Native/bin/r2r"
STAMP="$BIN/.source-stamp"
SOURCE="$(git rev-parse HEAD:src HEAD:patches 2>/dev/null | tr '\n' ' ')$(git status --porcelain -- src patches | git hash-object --stdin)"
PUBLISHED=0
if [ ! -f "$BIN/Editor.Native.dll" ] || [ "$REBUILD" = 1 ] || [ "$(cat "$STAMP" 2>/dev/null)" != "$SOURCE" ]; then
  say "Building the engine side for $RID"
  dotnet publish src/Editor.Native -c Release -r "$RID" --self-contained false \
    -p:PublishReadyToRun=true -p:TreatWarningsAsErrors=false -o "$BIN" -nologo -v q -clp:ErrorsOnly
  printf '%s' "$SOURCE" > "$STAMP"
  PUBLISHED=1
fi

# The editor itself. Cargo builds again only what changed.
say "Building the editor"
(cd native && cargo build --release -p ss14-editor)
EDITOR="native/target/release/ss14-editor"

# The code fingerprint of the fork the engine side was just built from, written by the editor.
if [ "$PUBLISHED" = 1 ] || [ ! -f "$BIN/code-fingerprint.txt" ]; then
  "$EDITOR" --stamp-engine "$BIN" "$FORK"
fi

if [ -z "$MAP" ]; then
  say "Starting the editor"
  exec "$EDITOR" "$BIN"
fi

# The prototypes, parsed once by the engine and packed for a quick start, and every texture
# of the fork in one file. Both are made once.
PACK="$ROOT/native/cache/kernel-resources.zip"
TEXTURES="$ROOT/native/cache/textures.sstx"
mkdir -p "$ROOT/native/cache"
if [ ! -f "$PACK" ] || [ "$REBUILD" = 1 ]; then
  say "Packing the fork's prototypes"
  dotnet "$BIN/Editor.Native.dll" pack "$FORK" "$PACK"
fi
if [ ! -f "$TEXTURES" ]; then
  say "Baking the fork's textures (once)"
  "$EDITOR" --bake "$FORK" "$TEXTURES"
fi

say "Starting the editor"
exec "$EDITOR" "$BIN" "$FORK" "$PACK" "$TEXTURES" "$FORK/Resources/$MAP"
