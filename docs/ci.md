# CI

There are three workflows in `.github/workflows/`.

- `pr.yml` runs on every pull request into `main`, on every push to `main`, and by hand from
  the Actions tab. A new push to a pull request cancels the run before it.
- `release.yml` runs when a `v*` tag is pushed, and makes a GitHub Release.
- `package.yml` builds the packages. Both of the others call it.

## The jobs of a pull request

| Job | Runs on | What it does | Time, cold cache | Time, warm |
|---|---|---|---|---|
| `checks` | Linux | `cargo fmt` (report only), `cargo clippy` on the whole workspace (warnings counted, not failed), `cargo test -p editor-core --release`, and a C# build of `src/Editor.Native` against the pinned Starlight with every engine patch unit on | 20 to 25 min | about 10 min |
| `engine-checks` | Linux | Publishes the engine side for linux-x64 against the pinned Starlight, packs its prototypes and runs `Editor.Native quick` (nine checks on two shuttles) | 15 to 20 min | about 12 min |
| `package` | Windows, Linux, macOS (Apple silicon and Intel) | Builds the editor and an engine side from upstream Space Wizards, stamps it, zips it with the start script and uploads the zip | 20 to 25 min | about 12 min |

The jobs run in parallel. Standard GitHub runners are free for a public repository, macOS
ones included.

- **Formatting.** The code isn't formatted with rustfmt yet. The step reports how many places
  and files would change, and passes. Once the code is formatted, drop the step's `if` so it
  fails on unformatted code.
- **Clippy** runs with `--cap-lints warn`, so only a build error fails it. The warning count is
  in the run's summary.
- **The fork's code.** `tools/ci/fork-checkout.sh` fetches a fork at one commit, shallow. For a
  build it checks out only the code (the Content projects, what they refer to, MSBuild), the
  same way `forks/engines.rs` copies a fork. Only `engine-checks` checks out the whole fork,
  because the checks read its maps, prototypes, sprites and text. The engine (RobustToolbox) is
  fetched whole at the fork's commit, with its submodules. The `upstream/starlight` submodule is
  never cloned whole.
- **Caches.** Cargo's registry and `native/target` are cached per job and platform, keyed by
  `Cargo.lock`. NuGet packages are cached per platform and fork commit, keyed by the editor's
  project files.

## What a package is

The engine side that ships is built from upstream Space Wizards (MIT throughout), pinned in
`package.yml` as `WIZDEN_COMMIT`. Every other fork, Starlight included, gets its engine side
built by the editor the first time it opens, which needs the .NET 10 SDK. Fork content
(prototypes, textures) is never shipped.

`tools/ci/engine-side.sh` builds the engine side the way `forks/engines.rs` does. First the
native engine patch units go on (a unit the engine already has is skipped, otherwise the first
patch that fits is applied). Then `dotnet publish` runs with ReadyToRun, and the engine's
Resources are put beside the result. The editor's `--stamp-engine` then writes its code
fingerprint. `tools/ci/package.sh` makes the zip:

```
ss14-editor-<platform>-<version>/
  ss14-editor(.exe)           the editor
  engine/                     the engine side, stamped
  src/, patches/engine/, ...  the sources the editor builds a fork's engine side from
  start.cmd or start.sh       starts the editor on its start screen (tools/package/)
  LICENSE                     the editor's licence
```

The version is the commit's first seven characters for a pull request, or the tag for a
release. The start scripts in `tools/` (`run.ps1`, `run.sh`) only work inside a clone of this
repository, so the package has its own in `tools/package/`. The stamp identifies its build by
the size and a hash of `engine/Editor.Native.dll` instead of a timestamp, so unzipping doesn't
invalidate it.

### Bumping the Space Wizards commit

`WIZDEN_COMMIT` is the newest commit the editor builds against: master's `a3f6b78` from
2026-11-17. The editor's C# keeps up with "Self-predicting popups" (#44524) and the device
network refactor (#44346, #44990) through the switches `FORK_PREDICTED_POPUPS`,
`FORK_SEALED_DEVICE_LINK_SYSTEM` and `FORK_LINKED_DEVICE_NETWORK` (`src/Directory.Build.props`).
To bump it, first check the new commit with `tools/engine-patches.sh fit <engine> --native` and
a publish against it, then change `WIZDEN_COMMIT`.

## Downloading a pull request's build

On the pull request, open **Checks**, then the `pr` run, then **Summary**. The zips are under
**Artifacts** at the bottom, one per platform, named `ss14-editor-<platform>-<commit>`. They
are kept for 30 days, and you need to be signed in to GitHub to download them.

Or use the GitHub CLI:

```
gh run list --workflow pr --branch <branch>
gh run download <run id> --name ss14-editor-windows-x64-<commit>
```

Each artifact is a zip holding the package's zip, which keeps the programs' executable bits.
Unzip both. Then run `start.cmd` on Windows or `./start.sh` on Linux and macOS. Not many
people have tried the Linux build yet, so treat it as experimental. On macOS, first run
`xattr -dr com.apple.quarantine .` in the package's folder, because the build isn't signed.

## Making a release

1. Set the version in `native/Cargo.toml` (`[workspace.package]`) and in
   `src/Directory.Build.props` (`Version`), and merge that.
2. Tag the commit on `main` and push the tag:

   ```
   git tag v0.1.0
   git push origin v0.1.0
   ```

`release.yml` checks that the tag matches the version in `native/Cargo.toml`, builds the four
packages, and makes a GitHub Release with them attached. GitHub writes the notes from the
merged pull requests. You can edit them on the release page afterwards.

The builds aren't signed. Windows SmartScreen warns about them, and macOS Gatekeeper holds them
back until the quarantine flag is removed (see above).
