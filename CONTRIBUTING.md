# Contributing

Thanks for helping out. This page gets you from a fresh clone to a tested change. When you
know which files you want to touch, read their part of
[docs/architecture.md](docs/architecture.md) first. It says where everything lives.

Bug reports and ideas are welcome as issues. For a bigger change, open an issue first so we can
agree on the approach before you sink time into it.

## The parts

- `native/editor-dioxus` is the interface: the window, the panels and the start screen, built
  with Dioxus Native. The crate and the program are both called `ss14-editor`.
- `native/editor-core` is the rest of the Rust side: the session and its tools, the mirror of
  the open map, forks and their cache, and the GPU renderer. Nearly all the logic lives here,
  and its tests run without an engine.
- `src/` is the engine side, in C#. `Editor.Kernel` holds the open maps as live game entities
  and makes every edit. `Editor.Native` hosts it inside the editor's process and holds the
  engine checks.
- `patches/engine` holds the small patches the engine side needs on top of RobustToolbox.
- `upstream/starlight` is a submodule with the Starlight fork at a pinned commit. The engine
  side builds against it by default, and the engine checks open its maps.

## Building

You need [Rust](https://rustup.rs), the [.NET 10 SDK](https://dotnet.microsoft.com/download)
and git. On Linux, add the libraries listed as `LINUX_PACKAGES` in `.github/workflows/pr.yml`.
On Windows, run the `.sh` scripts from Git Bash.

On macOS and Linux, `tools/run.sh` does everything below and then starts the editor. It
fetches the submodule too.

By hand, from the repository root:

```
git submodule update --init --recursive upstream/starlight
bash tools/engine-patches.sh apply
cd native
cargo build --release -p ss14-editor
cd ..
```

Then publish the engine side and start the editor. On Windows:

```
.\tools\run.ps1 -Publish
.\tools\run.ps1
```

To build the engine side against another fork's checkout, pass its folder:
`dotnet publish src/Editor.Native ... -p:ForkRoot=<fork folder>`. That fork's RobustToolbox
needs the patch units on it first (see below).

Two environment variables help when you work on the interface:

- `SS14_EDITOR_START_DEMO` set to `jobs`, `errors` or `jobs,errors` fills the start screen
  with fake jobs and errors so you can see how they look. Point `LOCALAPPDATA` (or your
  platform's data folder) at an empty folder as well, so your real settings stay untouched.
- `SS14_EDITOR_TAB` set to `favourites`, `tiles`, `entities`, `decals`, `prefabs` or `grids`
  opens the side panel on that tab.

## Testing a change

### The Rust tests

```
cd native
cargo test -p editor-core --release
```

They take seconds and need no engine, so run them after any change on the Rust side. Tool
tests drive a session with the mouse and keys and live in
`editor-core/src/session/tools/tests/`. If you change a tool or a rule, add a test with it.

A few tests are marked `#[ignore]` because they need a published engine side or build one.
Their comments say how to run them.

### The engine checks

After changing C#, publish the engine side again and run the `quick` check from the
repository root. Make the prototype pack once before the first run:

```
dotnet src/Editor.Native/bin/r2r/Editor.Native.dll pack upstream/starlight native/cache/kernel-resources.zip
dotnet src/Editor.Native/bin/r2r/Editor.Native.dll quick
```

`quick` runs nine checks on two small Starlight shuttles in about a minute. Each one opens a
map, edits it through the kernel, saves it and reads the save back. A line starting with `skip`
means the maps have nothing for that step. It isn't a failure. The
[architecture page](docs/architecture.md#checks) has a table of which check covers which part
of the kernel, and shows how to run the checks on the full stations.

### CI

Every pull request runs the Rust tests, clippy, a C# build and the `quick` check, and builds
the packages for every platform ([docs/ci.md](docs/ci.md)). You can download your pull
request's build from its checks and try it yourself.

## Engine patch units

The engine side needs a few things RobustToolbox doesn't offer, such as starting from
pre-parsed prototypes. Each one is a patch unit in `patches/engine/<number>-<name>/`:

- `unit.txt` says what the unit adds, whether the native engine side needs it, and how to
  tell that an engine already has it (`has: <file> <text>`).
- One patch per shape of engine, tried in name order (`starlight.patch`, `wizden-264.patch`,
  `wizden.patch` and so on). The first one that fits is applied.

The scripts:

- `tools/engine-patches.sh apply` resets the pinned engine and applies every unit.
- `tools/engine-patches.sh status` and `check` tell you whether the units still fit.
- `tools/engine-patches.sh fit <engine folder> --native` shows how each unit would go on
  another engine, without changing it.
- `tools/export-engine-patch.sh` writes a patch file from changes in an engine checkout, for a
  new unit or a new variant of one.

The editor applies the same units itself when it builds an engine side for a mapper's fork
(`native/editor-core/src/forks/engines.rs`).

## Rules

- Never commit changes inside `upstream/`. The patch files in `patches/engine` are the source
  of truth.
- An ordinary edit must read and write a map exactly as before. If your change alters what a
  saved map looks like, say so in the pull request.
- Keep each file's line endings.

## Style

- Comments, docs and commit messages use plain, short sentences. No semicolons and no em
  dashes. Say what the code does and why.
- Each Rust file starts with a `//!` comment saying what it holds.
- Tests live apart from the code, in `src/tests/` at the same path, declared with
  `#[cfg(test)] #[path = "..."] mod tests;`.
- `cargo fmt` isn't enforced yet. Format the lines you touch to match the code around them.
- Keep a pull request to one change. Small ones get reviewed faster.

By contributing, you agree that your contribution is licensed under the [MIT licence](LICENSE).
