#!/bin/sh
# Starts the SS14 map editor from its unzipped package, on Linux or macOS. It opens on the
# start screen, where you choose a fork. It needs the .NET 10 runtime, and the .NET 10 SDK to
# build an engine side for a fork's code the first time you open that fork. The SDK includes
# the runtime (https://dotnet.microsoft.com/download).
#
# macOS holds back unsigned downloads. Run this once in this folder to release it:
#   xattr -dr com.apple.quarantine .
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
exec "$HERE/ss14-editor" "$HERE/engine"
