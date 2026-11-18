#!/usr/bin/env bash
# Checks out a fork at one commit, shallow, with its engine (RobustToolbox) and the engine's
# submodules. Used by CI (.github/workflows/pr.yml and package.yml, docs/ci.md).
#
#   tools/ci/fork-checkout.sh <url> <commit> <folder> [--code|--full]
#
# --code (the default) checks out only what the engine side is built from, as editor-core's
# forks/engines.rs copies a fork: the three Content projects, the fork's Content.<name>.Server
# and Content.<name>.Client modules, every project they refer to, the MSBuild folder and the
# files of the top folder. Files outside these are never fetched.
# --full checks out the whole fork, Resources too.
#
# The engine is fetched whole at the commit the fork names, as the build needs its Resources.
# The folder may exist empty, as a submodule's folder does before it is set up.
set -euo pipefail

url="${1:?usage: $0 <url> <commit> <folder> [--code|--full]}"
commit="${2:?usage: $0 <url> <commit> <folder> [--code|--full]}"
dest="${3:?usage: $0 <url> <commit> <folder> [--code|--full]}"
mode="${4:---code}"

say() { printf '== %s\n' "$1"; }

# Fetches one commit into a new repository in a folder, shallow. Extra arguments go to fetch.
fetch_commit() {
  local folder="$1" from="$2" at="$3"
  shift 3
  mkdir -p "$folder"
  git init -q "$folder"
  git -C "$folder" config core.longpaths true
  git -C "$folder" remote add origin "$from"
  git -C "$folder" fetch -q --depth 1 "$@" origin "$at"
}

# The top folders of the projects a set of top folders' project files refer to.
referred() {
  local folder project reference
  for folder in "$@"; do
    for project in "$dest/$folder"/*.csproj; do
      [ -f "$project" ] || continue
      grep -o 'ProjectReference Include="[^"]*"' "$project" | sed 's/^ProjectReference Include="//; s/"$//' | tr '\\' '/' |
        while IFS= read -r reference; do
          # Relative to the project's folder, which is one below the top.
          case "$reference" in
            ../*) reference="${reference#../}" ;;
            *) reference="$folder/$reference" ;;
          esac
          echo "${reference%%/*}"
        done
    done
  done
}

say "Fetching $url at $commit ($mode)"
if [ "$mode" = --full ]; then
  fetch_commit "$dest" "$url" "$commit"
else
  fetch_commit "$dest" "$url" "$commit" --filter=blob:none
  # Trees only so far. The sparse set is written before the checkout, so that only its files
  # are fetched.
  modules="$(git -C "$dest" ls-tree -d --name-only FETCH_HEAD | grep -E '^Content\.[^.]+\.(Server|Client)$' || true)"
  folders="$(printf '%s\n' Content.Shared Content.Server Content.Client MSBuild $modules | sort -u)"
  git -C "$dest" sparse-checkout set --cone $folders
fi
git -C "$dest" -c advice.detachedHead=false checkout -q --detach FETCH_HEAD

if [ "$mode" != --full ]; then
  # Follow the project references until no new folder turns up.
  while true; do
    more="$(referred $folders | grep -v -e '^RobustToolbox$' -e '^\.\.$' -e '^$' -e '\$' | sort -u || true)"
    grown="$(printf '%s\n' $folders $more | sort -u)"
    [ "$grown" = "$folders" ] && break
    folders="$grown"
    git -C "$dest" sparse-checkout set --cone $folders
  done
  echo "checked out: $(echo $folders)"
fi

# The engine at the commit the fork names, with its submodules.
engine_commit="$(git -C "$dest" ls-tree FETCH_HEAD RobustToolbox | awk '$2 == "commit" { print $3 }')"
[ -n "$engine_commit" ] || { echo "$url names no commit of RobustToolbox" >&2; exit 1; }
engine_url="$(git -C "$dest" config -f .gitmodules --get submodule.RobustToolbox.url)"
case "$engine_url" in
  ./* | ../*) echo "a relative engine URL ($engine_url) is not handled here" >&2; exit 1 ;;
esac
say "Fetching the engine $engine_url at $engine_commit"
rm -rf "$dest/RobustToolbox"
fetch_commit "$dest/RobustToolbox" "$engine_url" "$engine_commit"
git -C "$dest/RobustToolbox" -c advice.detachedHead=false checkout -q --detach FETCH_HEAD
git -C "$dest/RobustToolbox" submodule update -q --init --recursive --depth 1
echo "fork $(git -C "$dest" rev-parse --short HEAD), engine $(git -C "$dest/RobustToolbox" rev-parse --short HEAD)"
