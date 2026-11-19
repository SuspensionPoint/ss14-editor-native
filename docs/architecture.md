# Architecture

How the editor fits together, and where each part lives. Skim it before a change so you can
go straight to the right file. [CONTRIBUTING.md](../CONTRIBUTING.md) covers building and testing.

## The shape

The engine owns the map. The Rust side asks it to make edits and draws whatever it reports
back.

```
the window and its panels, made with Dioxus Native (native/editor-dioxus)
   -> Session, tools, mirror of the document (native/editor-core)
      -> the .NET runtime in the same process (src/Editor.Native)
         -> the kernel: documents, edits, undo, save (src/Editor.Kernel)
            -> the game engine and the fork's content (a fork checkout, never changed)
```

An engine side is built against one fork's code. The editor ships with one built from
upstream Space Wizards. When a mapper opens a fork whose code differs, the editor builds an
engine side for that fork into its cache, once (`forks/engines.rs`). For development and for
the checks, the repository pins Starlight as a submodule (`upstream/starlight`).

Line counts below are rough, to give a sense of size.

## Engine side (C#)

| File | Lines | What it holds |
|---|---|---|
| `src/Editor.Kernel/Documents.cs` | 2490 | Open maps as live entities. Every edit op, undo, redo, rollback, save, copy and paste. Start here for anything that changes map data. The `anchor` op fixes a loose entity to the tile under it, as a wrench does. Validate Map's Lay plating uses it, since a tile laid under a loose wall leaves it loose. |
| `src/Editor.Kernel/EditorKernel.cs` | 510 | The headless engine with the fork's content loaded, and its start-up. |
| `src/Editor.Kernel/ComponentEditor.cs` | 640 | Reads and writes component data in map-file form, and trims device lists down for a copy. Also the fixed table of components whose references the delete cleanup clears (`Listing`, `Listers`), plus the ones from that table the fork lacks (`UnknownListing`). The kernel warns about those at start. |
| `src/Editor.Kernel/ComponentFields.cs` | 240 | The `component.fields` question, which the typed forms are built from. It finds each data field of an entity's components by reflection on `DataField`, over fields and properties of the type and its bases, with the tag as the engine makes it. For each field it gives the kind, the C# type, the current value and the prototype's, a read-only flag, the names of an enum or flag set, and the kind of a prototype id. The ids of each prototype kind come once. A field the reflection misses (an included data definition) comes back as `other`. It changes nothing. |
| `src/Editor.Kernel/Contract.cs` | 370 | The message types: `EditOp`, deltas, document info. The Rust side reads them as JSON, so change both sides together. |
| `src/Editor.Kernel/KernelApi.cs` | 130 | The kernel's commands by name, with JSON in and out. |
| `src/Editor.Kernel/Air.cs` | 190 | The `air` question. For each grid it gives the stored mixes, the mix of each tile, and the tiles with no air, walls and floors apart. For every tile (`cells`) it gives the sides that block air, whether it holds no air because all four do, whether it is open to the map's atmosphere, and whether any airtight thing stands on it at all. That last one matters because the game lets a floor with no air under such a thing fill from its neighbours. It also says whether the map's atmosphere is space and whether each grid has an atmosphere. The airtight rules are the game's, worked out from the components, since no atmosphere or door system runs. It changes nothing. |
| `src/Editor.Kernel/AirFix.cs` | 310 | Fix grid atmos. This is the game's `RebuildGridAtmosphere` applied to a grid's data, step by step with the game's lines cited, as on the paused map a mapper fixes and saves. The `air.fix` op of `Documents.Edit` uses it (`FixAir`, one `AirChange` per grid). The `air.fix.preview` question counts what a fix would change, and for a grid with no atmosphere it counts as if one were added first (`addable`). It also does the game's addatmos. The `air.add` op gives a grid an empty GridAtmosphere and a GasTileOverlay (`AtmosphereAdded`, undone exactly). An edit whose tiles bring a grid without one up to the weight of seven tiles gets one too, as the game's AutomaticAtmosSystem does (`WouldGainAtmosphere`). A planet's own ground is never offered one. When the fork is bumped, compare this file with the fork's `AtmosphereSystem.Commands.cs`, `AddAtmosCommand.cs` and `AutomaticAtmosSystem.cs`. |
| `src/Editor.Kernel/Visuals.cs` | 290 | The looks the game's server would send its clients as appearance data, worked out from the components with the game's lines cited. That means a cable's joins (`CableNode.GetReachableNodes` with its terminals, `CableVisSystem`) and a pipe's saved colour (`AtmosPipeColorSystem`). A scene carries all of them (`Scene.Visuals`). After an edit, undo or redo, the cells it touched and their neighbours are checked again, along with those beside a plumbing machine, and the delta carries only the looks that changed (`SceneDelta.Visuals`). The view applies them through the client's appearance system (`ViewWorld.ApplyVisuals`), so a change of looks alone doesn't move the entity in the view. Whether a node can connect is written out here (`Connectable`) instead of asking the game's node objects. |
| `src/Editor.Kernel/SavedState.cs` | 180 | The saved data the view is sent of each entity it shows (`Scene.Saved`, `SceneDelta.Saved`), so that it looks as the map saves it and not as its prototype. In the game's mapping mode the server sends each client its copy of every networked component, and the client's systems dress the entity by it, as SharedDoorSystem does from a door's state and SharedGasValveSystem from a valve's. Here it is the data of the networked components that differs from the prototype (`ComponentEditor.Sent`), all but placement, contents and physics (`NotSent`). On opening, only the components the file saves of each entity are read (`Listed`), since reading all of them costs about as much as a save. After an edit, the entities whose components changed and those new to the view are read again, and only what changed is sent. It goes to the view as data in the same process and is left out of the JSON. |
| `src/Editor.Kernel/VisualsDevices.cs` | 170 | Appearance values the game's server sets as a map loads and a client cannot work out from the saved data (`SceneVisuals.Appearance`, keys and values as text by enum type and kind). A door's state, which the client's DoorSystem never sets itself. Whether a gas filter, mixer, pressure-controlled valve or pressure regulator is on, which only the server's systems set. Whether a cabinet holds something, since the view shows nothing inside a container. What depends on power, the station, the date or chance is left as the client has it. |
| `src/Editor.Kernel/VisualsPipes.cs` | 240 | A pipe's connectors (`PipeVisuals.VisualState`, four bits per pipe layer), following `PipeNode.LinkableNodesInDirection` and `AtmosPipeAppearanceSystem.UpdateAppearance`. No node system runs, so each node's direction is turned by the entity's rotation here, and its layer comes from AtmosPipeLayers. Joins are made undirected the way `NodeGroupSystem` makes them (`Reachable`). Also a pipe's layer when its map data sets one different from its prototype's (`PipeLayer`), which the view applies with the game's `SetPipeLayer`. |
| `src/Directory.Build.props`, `Directory.Build.targets` | 190 | Which fork every project builds against: `ForkRoot`, which is upstream/starlight unless `-p:ForkRoot=` names another. Where the fork and engine keep what the projects build on: `EngineSimulationProject` (the test simulation in Robust.Server.Testing, or in Robust.UnitTesting on engines 264 and 268), `EngineImports` and `ForkContentProps`. And the switches for code that only some forks or engines have. Each switch is detected from the fork's or engine's own source and named for what it has: `FORK_PLUMBING`, `FORK_DISPOSAL_EXITS`, `FORK_ANIMATED_BACKDROPS`, `FORK_SERVER_ATMOSPHERE`, `FORK_CLIENT_SERVICES_AT_INIT`, `FORK_PREDICTED_POPUPS`, `FORK_DECAL_CHUNK_ENTITIES` (Space Wizards' decals on chunk entities, which are shown and cannot be edited yet: `ChunkDecals` in `Documents.cs`), `FORK_SEALED_DEVICE_LINK_SYSTEM`, `FORK_LINKED_DEVICE_NETWORK`, plus an `ENGINE_*` switch for each place the engine's API differs (grids by the map system, partial prototypes, tags on keys, drawing calls, the map loader's text overloads, bulk jobs and more). Code under a switch only compiles where it fits. The switches are set when the engine side is built, from the engine with the patch units on it. The root `Directory.Packages.props` also takes the fork's package versions through `ForkRoot`. |
| `src/Editor.Kernel/ForkCompat.cs`, `VisualsNoPlumbing.cs` | 150 | Stand-ins for forks that lack something the kernel uses. A prototype id as text or as ProtoId. `TryComp` on an engine that calls it `TryGetComponent`. A grid's atmosphere types where a fork keeps them in Content.Server (Goob Station). Loading and saving a map from text on an engine that only does it by path (RMC-14's 264). The device link and device list components where the Space Wizards' device network refactor moved them (`ListedDeviceComponent`, `ListedDevice`, the component a device keeps its lists in). And no plumbing at all. |
| `src/Editor.Kernel/HostingShared.cs` `ContentAssemblies` | 80 | The content assemblies each kernel loads, the same way the game does. That is Content.Shared with Content.Server or Content.Client, plus a fork's modules beside them (Goob Station's Content.Goobstation.* and Content.Common). Any Content assembly in the engine side's folder with an entry point counts, and the engine they refer to tells them apart. Also the prototype kinds that only the other side has, which the document kernel ignores. Editor.Kernel and Editor.Native refer to the fork's `Content.*.Server` and `Content.*.Client` projects by wildcard so the modules get built. Starlight has none. |
| `src/Editor.Kernel/VisualsPlumbing.cs` | 320 | Starlight's plumbing, built only with `FORK_PLUMBING`. It follows `PlumbingNode.GetReachableNodes`: a machine picks one duct per side and takes its layer, and a duct joins the machine that picked it. The machines' connectors from `PlumbingConnectorAppearanceSystem` are sent as keys and values named after `PlumbingVisuals` (`SceneVisuals.Plumbing`). It also decides what belongs on the editor's Plumbing layer (`OnPlumbingLayer`: SubFloorHide plus a plumbing node, so ducts and plumbing machines, but not the chem master or dispensers). |
| `src/Editor.Kernel/PrototypeCache.cs`, `ResourcePack.cs`, `ZipContentRoot.cs` | | Loading content from the kernel pack. Rarely touched. `ResourcePack.EngineResources` finds the engine's Resources: in the fork's RobustToolbox, or beside an engine side the editor built for a checkout whose engine submodule isn't set up. |
| `src/Editor.Native/NativeApi.cs` | 230 | The commands the Rust side calls. It wraps the kernel and the view kernel. `air { doc, grids }` gives the stored air (Air.cs). `air.fix.preview { doc, grids }` gives what a fix would change (AirFix.cs). `backdrops` lists the fork's backdrops, and `backdrop.named` gives one by id, painted and cached the same as a map's. A map's backdrop is set with the `components` op on the map entity. |
| `src/Editor.Native/NativeExports.cs` | 200 | The C functions the Rust side loads. |
| `src/Editor.Native/EngineSession.cs` | 390 | One open session: documents, draw lists, backdrops. |
| `src/Editor.Native/Program.cs` | 230 | Dispatches the checks below, and `pack <fork> <zip>`, which the editor runs to make a fork's pack for its cache. |
| `src/Editor.Native/KeepsCheck.cs` | 170 | `keeps` opens each map, saves it with no edit, and counts every kind of component on the entities in both. If the save has fewer of a kind, the check fails. An entity counts as the same in both by its uid and prototype. Entities that the fork's migrations delete or change at load, and entities made fresh at load (a fork's decal chunk entities) that take a freed uid, are told apart. This is the check to run on an engine side built for another fork. |
| `src/Editor.ViewKernel/`, `src/Content.EditorView/` | | How a document looks: the draw list. Only touch these for drawing bugs. `ViewWorld.Saved.cs` makes an entity with its saved data the way the game's map loader does: made from the prototype, each saved component merged with the prototype's and read onto it, then started. An entity whose saved data changes is made again, since the client's systems read it as the entity starts. A reference to an entity not made yet reads as one that is not there, and the entity is made again once the rest are in (`ResolveSaved`). It also sets the kernel's appearance values with the client's appearance system. An item's seventh number breaks ties after the sort key, so things under the floor that show through it keep their earlier depth order (`ViewWorld.DrawList.cs`, `drawlist.rs`). Item kind 5 is plumbing, as the kernel names it (`SceneVisuals.PlumbingLayer`), and the editor puts it on its Plumbing layer (`renderer.rs` `group_of`, `Layers::plumbing`, and the switches in `shell.rs` `LAYER_NAMES`). |

## Rust side

All of it is under `native/`. Build one package at a time.

| File | Lines | What it holds |
|---|---|---|
| `editor-core/src/session/mod.rs` | 1080 | `Session`: the camera, palettes, selection, layer switches, and whatever is waiting on the engine. `Session::offline` runs the tools with no engine, for unit tests. |
| `editor-core/src/session/engine_thread.rs` | 600 | What the session asks the engine, and what comes back. |
| `editor-core/src/session/zoom.rs` | 70 | Wheel zoom, spread over a few frames so each notch glides. The interface calls `advance_zoom` once a frame and asks for another frame while more zoom is still owed. Tests in `tools/tests/view.rs`. |
| `editor-core/src/session/tools/mod.rs` | 860 | Sends presses, drags and keys to the tools. Its module doc is the rule sheet for adding a tool. |
| `editor-core/src/session/tools/tiles.rs` | 360 | Paint, Erase, Rectangle, Fill. |
| `editor-core/src/session/tools/shapes.rs` | 190 | Line and Circle. |
| `editor-core/src/session/tools/floors.rs` | 240 | Laying a tile under new walls. `needs_floor` is the one rule for what gets a tile: the Wall tag, or anchored by default with the Window tag, the CanBuildWindowOnTop component (grilles) or the Door component. The switch, the tile (Plating by default), the Room tool's wall and its Intersection switch, kept per fork in the settings file (`Flooring`, `ForkPlacing`). The Room tool's wall list order (`room_walls`): WallSolid, WallReinforced, the palette's Structures/Walls, the rest, then rock (`is_rock`, using Validate Map's `ALLOWED_WALLS`). Also the faint ghost of the tiles a press would lay. Tests in `tools/tests/floors.rs`. |
| `editor-core/src/session/tools/room.rs` | 270 | The Room tool: a dragged rectangle that becomes one edit, with the tile in hand inside, the wall tile on the edge, and walls on the edge. `plan_room` decides each cell. With Intersection off, any cell holding a wall (`Prototype::is_wall`) is left alone, so walls never stack and no floor goes under one. With it on, the room joins those it overlaps: walls inside are deleted, edge cells already holding a wall stay, and edge cells on another room's floor stay open. Its ghost (red marks on walls to delete), size label and cancel. Tests in `tools/tests/room.rs`. |
| `editor-core/src/session/tools/place.rs` | 320 | Placing entities. |
| `editor-core/src/session/tools/select.rs` | 650 | Entity Select: find, move, turn and delete entities and decals. |
| `editor-core/src/session/tools/area.rs` | 700 | Area Select: regions of cells. |
| `editor-core/src/session/tools/nudge.rs` | 100 | Nudging. The arrow keys move the selected entities and decals one cell along their grid's directions, or one pixel with Shift when nothing is anchored. They also move the area tool's region one cell (`move_region` in `area.rs`). A press is dropped while the engine hasn't answered the last edit yet (`last_edit`, `last_answered`). `arrows_reach_map` is the rule the interface checks before it passes on an arrow key. Tests in `tools/tests/nudge.rs`. |
| `editor-core/src/session/tools/paste.rs` | 710 | Copy, cut, paste. Escape on the map, and `escape_to`, the rule the interface uses to decide where Escape goes: a menu, the dialogs, a text field or the map. |
| `editor-core/src/session/tools/prefabs.rs` | 145 | Save as Prefab, the session's list of the prefabs folder, choosing a prefab to stamp, import and delete. |
| `editor-core/src/session/tools/pick.rs` | 170 | Pick. |
| `editor-core/src/session/tools/look.rs` | 80 | How the map looks under the mouse for each tool: the colour of the hover outline and the mouse pointer. |
| `editor-core/src/session/tools/decals.rs` | 520 | Decals: the palette's choice and groups, the placement settings, placing with the brushes, the ghost preview, finding decals on the map, and what the decal panel asks for. |
| `editor-core/src/session/tools/infra.rs` | 260 | Cables, and the path the cable and pipe tools drag: the kinds, the current choice, laying and removing cables. |
| `editor-core/src/session/tools/pipes.rs` | 300 | Pipes: the pieces of each network, the fitting a cell calls for, laying and removing with refits. |
| `editor-core/src/session/tools/link.rs` | 470 | The Link tool: source, port chooser, link, unlink, device lists, outlines and the dashed line. Also the selection panel's link rows, Auto-link Room, and a port's English name (`port_name`, `port_shown`, `link_ports_text`). |
| `editor-core/src/session/tools/connections.rs` | 80 | The connections view: every link drawn as a line in the overlay rectangles. |
| `editor-core/src/session/tools/tests/` | | Unit tests, one file per tool. Helpers are in `tests/mod.rs`. Link tests are in `tests/links.rs`. |
| `editor-core/src/session/entity.rs` | 570 | Editing the selected entity. Component text and the typed forms (`component_entries`, requested with the data and again after it changes). Setting or resetting a field as the component's own data, where a value equal to the prototype's is dropped. Adding and removing components. Its containers and what they hold. Tests in `tools/tests/entity.rs`. |
| `editor-core/src/fields.rs` | 200 | The form field kinds as the engine names them, the text each widget shows, turning what was typed back into the component's JSON, flags written the way the game writes them, and when two values count as the same data. Has its own tests. |
| `editor-core/src/session/grids.rs` | 120 | The grid list: centre on one, add, rename, remove. Tests in `tools/tests/grids.rs`. |
| `editor-core/src/session/palettes.rs` | 150 | What is in hand (`InHand`), the reveal a pick asks of the palettes, the entity palette's search, and the entity tree's categories, read on a thread. Tests in `tools/tests/palettes.rs`. |
| `editor-core/src/session/favourites.rs` | 180 | Favourites: tiles, entities and decals in one list, in the mapper's order. They are kept per fork in the settings file and written on every change. Add, remove, move up and down, and pick them the way their own palette does. Number keys 1 to 9 pick one (`favourite_key`, ignored during a drag). A favourite the fork lacks is kept and shown as missing. Tests in `tools/tests/favourites.rs`. |
| `editor-core/src/palette.rs` | 240 | The palettes' order: the common tiles, a prototype file's category and entity ids, the scan of the fork's files, the entity tree, its rows and its search. Has its own tests. |
| `editor-core/src/files.rs` | 70 | Where the system's file dialogs start: the last folder used, Save As's folder and name, and `.yml` added. Has its own tests. |
| `editor-core/src/forks/mod.rs` | 180 | Forks: whether a folder looks like a fork, its name and branch, and `check`, which a fork must pass before it opens. That covers the safety rule (`fingerprint::code_verdict`) and where its pack and textures are. Tests in `tests/forks.rs`, with a small fork in a temporary git checkout that the other fork tests share. |
| `editor-core/src/forks/popular.rs` | 160 | The popular forks on the start screen (`POPULAR_FORKS`: name, repositories with labels, description, licence, own engine fork, colour, upstream). Also the popular fork a URL points at and the name its download gets, a URL's GitHub owner, and a fork's monogram and colour. Tests in `tests/forks/popular.rs`. |
| `editor-core/src/forks/avatars.rs` | 70 | GitHub avatars of the fork owners, fetched once with curl into `avatars/` under the user data folder and kept only if they are PNGs. Tests in `tests/forks/avatars.rs`. |
| `editor-core/src/forks/settings.rs` | 320 | `settings.json` in the user data folder: known forks (with `managed` set for the ones the editor downloaded), recent maps, the last fork, each fork's favourites by its folder (`Favourite`, `set_favourites`), how each fork places walls (`ForkPlacing`, `placing`, `set_placing`), the forks that show the Problems overlay (`problems_shown`), and whether animated sprites move (`animate_sprites`, for every fork). |
| `editor-core/src/forks/download.rs` | 550 | Forks the editor downloads. The forks folder and which folders are in it (`is_in`), and folder names from URLs. Git runs as a child process and its `--progress` output is read into `Progress`. `Stopper` kills git and anything it started. `download` makes a shallow clone with submodules into `<name>.partial` and then moves it into place. `update` fetches, fast-forwards only, and updates submodules. Tests in `tests/forks/download.rs` clone from bare repositories in a temporary folder. |
| `editor-core/src/forks/fingerprint.rs` | 440 | Fingerprints of a checkout's prototypes, textures and code, from git's hashes, or from its files when it isn't a git checkout. The code means the three Content projects, the fork's `Content.*.Server` and `Content.*.Client` modules, and whatever they refer to (`content_projects`). Also the engine's commit, the one the fork names when its submodule isn't set up (`engine_commit`), the engine side's `code-fingerprint.txt` (`EngineStamp`), and the safety rule (`code_verdict`). |
| `editor-core/src/forks/engines.rs` | 890 | The engine side built for a fork's code. `choose` decides which engine side a fork opens with: the editor's own, one in the cache by `key`, or a `BuildJob`. `Sources` finds the editor's sources beside it. `build` makes a shared sparse clone of the checkout with its uncommitted code changes, then clones or fetches the engine at its commit with submodules. Then it applies the engine patch units native needs (`Unit`, `native_units`, `apply_units`). A unit the engine already has is skipped. Otherwise the first variant git accepts goes on, and if none does the build is refused, naming the unit and git's message for each variant. It checks the fork's SDK and takes a newer major one when that SDK is missing (`sdk_check`, `rolled_forward`). Then it runs `dotnet publish` into the cache, showing the first errors on failure, and puts the engine's Resources and the stamp beside it. Tests in `tests/forks/engines.rs` use a git "server" in a temporary folder and small engines for the units. An ignored test builds the engine side of the fork that `EDITOR_BUILD_FORK` names. |
| `patches/engine/<number>-<name>/` | | One engine patch unit each. `unit.txt` holds `adds:`, `native: yes` or `no`, `has: <engine file> <text>` to tell that an engine already has it, and `for: <engine file>` for a unit only engines with that file need (as with 0010 for prototype variants). Next to it is a patch per shape of engine, tried in name order: `starlight.patch`, `wizden-264.patch` (made on RMC-14's 264), `wizden-268.patch`, `wizden-289.patch`, `wizden-296.patch`, and `wizden.patch` (made on Delta-V's 275). The test simulation's units have a second `has` line for engines that keep it in Robust.UnitTesting. `tools/engine-patches.sh` applies every unit to the pinned engine, and `fit <engine folder> [--native]` shows how each would go on another engine without changing it. |
| `editor-core/src/forks/cache.rs` | 160 | The cache of packs, baked textures and built engine sides, keyed by fingerprint. `plan` works out what must be made and the loading screen's text for it. `make_pack` runs `dotnet Editor.Native.dll pack`, and `make_textures` bakes here. Also pruning. An engine side is moved aside before it is deleted, so one in use is never left half deleted. |
| `editor-core/src/shell.rs` | 240 | The command line (`ShellArgs`, `Launch`: a fork with its pack and textures, or just the engine folder for the start screen) and the engine side a fork given there runs on (`choose_engine`). The flags `--stamp-engine`, `--build-engine <engine folder> <fork>` (builds the engine side for a fork's code into the cache, reporting each stage) and `--bake <fork> <textures file>` (bakes a fork's textures, for the start scripts). Also the web editor's colours and the layer switch names. |
| `editor-core/src/session/backdrops.rs` | 310 | The backdrop gallery (Map > Backdrop...): the game's default and the fork's backdrops, which one the map has, which one is under the mouse and previewed behind the map, the pictures painted so far, and the single edit a choice makes. It is refused for a grid file and for a planet. The engine's thread paints the pictures one at a time (`paint_backdrops` in `engine_thread.rs`). Tests in `tools/tests/backdrops.rs`. |
| `editor-core/src/air.rs` | 850 | The air overlay's rules. It mirrors the stored air and each grid's rooms. It holds the Breathable view's categories with the game's thresholds and each one's look (colour, mark, icon). A floor with no air under an open door is shown with the mean of its neighbours' air, as the game gives it. Also the pressure and temperature scales (one cividis-like scale), the Rooms view's colours, the legends with their sample tiles, the hover box's text and icon, the leaks, and `GridsOf`, which finds the grids an edit touched. Has its own tests. |
| `editor-core/src/air_draw.rs` | 600 | Drawing the air overlay: each tile's colour and mark as rectangles for the renderer, the leaks' dashed orange outlines and arrows, room labels and their text, the zoom rules for marks and labels (`mark_strength`, `label_shown`, `uncrowded`), and the legend's sample tiles and icons painted into pixels. Has its own tests. |
| `editor-core/src/air_checks.rs` | 200 | The stored-air faults that Validate Map lists and the Rooms view's hover box reports. Tiles of a sealed room whose air differs from the room's most common mix beyond the tolerances (pressure, gas shares, temperature, each based on what the game acts on). Floors of a sealed room with no air, which the game would turn to vacuum. Walls that store air. Pure functions with tests. They run on the engine's thread along with the rooms (`GridAir::faults`). |
| `editor-core/src/rooms.rs` | 190 | The leak check's rooms. A flood fill over a grid's tiles by the game's rule: two neighbours share air unless either blocks the side between them. For each room it finds the size and the leaks (tiles joined to space, an empty cell or the map's atmosphere) with the way out of each, a colour different from its neighbours', and whether it is the outside (open along most of its edge). Pure functions with tests. The engine's thread finds a grid's rooms when it reads the air (`read_air`), so no frame waits on it. |
| `editor-core/src/session/air.rs` | 140 | The air overlay inside the session. It takes in the engine's answers and rebuilds the rectangles and room labels only when the air, the view, a grid's position or the marks' strength changes (`air_overlay` and `air_labels` take the on-screen width of a tile). Also the hover box with its icon, the legend, and in the Rooms view the room under the mouse. The engine's thread reads the air when a map opens, and again for the grids an edit touched once nothing else is waiting (`read_air`). Tests in `tools/tests/air.rs`. |
| `editor-core/src/session/air_fix.rs` | 310 | Fix grid atmos (Map > Fix grid atmos...). The dialog's choice of all grids or the one under the mouse, and its count of the tiles that will change (`air.fix.preview`). The grids with no atmosphere, and the option (ticked by default) to add one first (`air.add`). Why it can't run, when no grid has an atmosphere or can take one, as on a planet's map. The single edit made of `air.add` and `air.fix` ops, and Validate Map's Fix for one grid (`fix_grid_air`). Afterwards the air is read again, as after any edit that changed it (`Delta::air`). Tests in `tools/tests/air_fix.rs`. |
| `editor-core/src/session/problems.rs` | 180 | The Problems overlay (Layers > Problems): the switch, kept per fork in the settings file, the cheap rules run again after each change the engine answers (`validate::quick`), the badges' rectangles and the box over a badge. Also Validate Map's Remove extras and Lay plating, for one finding or a whole group, as one edit (`fix_finding`, `fix_findings`). Lay plating anchors the loose wall in the same edit. Tests in `tools/tests/problems.rs`. |
| `editor-core/src/problems.rs` | 140 | Drawing the Problems overlay: one badge per cell with findings, in the air overlay's leak orange, half a tile in the cell's corner close up and at least 16 pixels far out (`badge_size`). The badge under the mouse. Has its own tests. |
| `editor-core/src/session/find.rs` | 110 | Jumping to a search result or a finding, and the outline of a found cell. `validation_key` tells Validate Map's list to search again after an edit or a fresh read of the air. |
| `editor-core/src/model.rs` | 630 | The mirror of the open document, as the engine reported it. |
| `editor-core/src/links.rs` | 340 | The mirror of device links and lists (`Doc::links`), what the engine reports about ports including the localisation key of each port's name (`Ports::name_key`), the order ports are offered in, and Auto-link Room's flood fill. Has its own tests. |
| `editor-core/src/locale.rs` | 130 | The fork's English text by localisation key: the plain messages in the `.ftl` files of `Resources/Locale/en-US`, read on a thread as the session starts (`Locale`, kept in `Session::locale`). Used for port names. Has its own tests, plus an ignored one that reads the fork's files. |
| `editor-core/src/prefab.rs` | 530 | The prefab file (`.ss14prefab`): writing it, reading it and refusing bad ones. Also the prefabs folder: listing, search, save, delete, import. Has its own tests. |
| `editor-core/src/search.rs` | 70 | The entity search (Ctrl+F) over the mirror. Has its own tests. |
| `editor-core/src/validate.rs` | 830 | Validate Map: one pure function per rule, the grouping, and the tests. "Stacked walls" finds two walls, windows or doors anchored on one cell, "Duplicates" the same prototype anchored twice on a cell and turned the same way, and "Walls with no floor" a wall, window or door on a cell with no tile or off every grid. What counts as a wall, a window or a door is `blocker`, from tags and components: firelocks, shutters, blast doors, windoors, directional windows and anything diagonal are left out, since the game puts them on a door's or a window's cell on purpose. Markers and spawners may stack. `extras` is what a fix of a stacked cell deletes, all but the first by uid. `quick` runs the five cheap rules of entities and tiles, which the Problems overlay runs after every change. "Open Disposal Ends" (`open_disposals`) joins disposal tubes by the exits the engine reports for each prototype (`Prototype::tube_exits`) and finds where trash would leave with no unit to catch it. It skips a junction's middle side and a trunk on a conveyor, and says whether the floor is torn up by checking which tiles a crowbar lifts (`Tile::can_crowbar`). "Rooms Open To Space" reads the rooms in the air mirror (`open_rooms`, `SMALLEST_ROOM`). "Floors With No Air", "Uneven Air In Rooms" and "Air In Walls" read the faults that `air_checks.rs` found. "Grids With No Atmosphere" finds a grid of floors that has none but can take one. A "Floors With No Air" or "Grids With No Atmosphere" finding offers Fix grid atmos on its grid (`Finding::fixes_air`). An ignored test, `stations`, runs the air rules, the disposal rule and the rules of walls on Bagel, Cork and Starboard through the published engine and prints what they find, which prototypes count as walls, windows and doors, and how long `quick` takes. `EDITOR_ENGINE`, `EDITOR_FORK` and `EDITOR_PACK` point it at another engine side, fork or pack. Another, `plating_anchors_a_loose_wall`, runs Lay plating's edit on Cork with undo and redo. |
| `editor-core/src/renderer.rs`, `drawlist.rs`, `textures.rs`, `backdrop.rs`, `thumbs.rs`, `colour.rs`, `*.wgsl` | | Drawing on the GPU. The map is drawn as the game draws it: textures are sRGB, tints and blending happen in linear light (`colour.rs` holds the conversions), and corners snap to screen pixels. It goes through an sRGB view of the canvas. The editor's own overlays (air, ghosts, grid lines, outlines) go through the plain view, so their colours stay as designed. `backdrop.rs` also reads a backdrop by its id and makes its thumbnail for the gallery. The renderer draws the air overlay (`set_air`) as one batch over the tiles and under everything else, marks, outlines and arrows included. With View > Animate sprites on (`set_animated`), `animate` moves each rectangle of an animated sprite through its state's frames by the session's clock, writing only the rectangles whose frame changed, and says whether one is in view, so the interface draws again only then. With it off, nothing moves and nothing is drawn again for it. Each frame of an animated state comes from the engine with the frame after it and how long it shows (`TextureSource` `next` and `delay`, from the state's folded timeline in `DrawRecorder.cs`). The baked textures file is the same either way. |
| `editor-core/src/animation.rs` | 100 | Which frame of an animated state shows at a time: the ring of frames the engine's links make (`Cycles`), and the frame a given time after another (`Cycle::frame_after`). Has its own tests. |
| `editor-core/src/session/animate.rs` | 40 | The View menu's Animate sprites switch, off until a mapper turns it on, kept in the settings file for every fork. Tests in `tools/tests/animate.rs`. |
| `editor-core/src/engine.rs` | 180 | Loads the .NET runtime into the process. |
| `editor-core/src/loading.rs` | 370 | The loading screen's model: the stages of a load (a fork's pack and bake come first when the cache doesn't have them yet), which are done or failed and when, and how the tiles and the clown move. Has its own tests. The session keeps one in `Session::loading`, fed by the engine's thread. |
| `editor-dioxus/` | 7200 | The interface, made with Dioxus Native and drawn by Blitz, with no browser. The crate and program are `ss14-editor`. See the file list below. |

The files of `editor-dioxus/src/`:

- `main.rs`: the state, the keys and the layout.
- `start.rs`: the start screen's state and actions. It shows when the command line gives only
  the engine folder. It handles downloads of popular forks or a URL, Use a checkout I have,
  Update and Remove for downloaded forks, the fork check on a thread, and the build of an
  engine side for a fork's code. Each job and message is tied to its card by `Place`. It also
  fetches avatars on threads, has the `SS14_EDITOR_START_DEMO` switch that fakes jobs and
  errors for a preview, records maps in the settings, and runs Map > Switch fork..., which
  starts a new process.
- `start_view.rs`: how the start screen looks. The welcome, Your forks as cards, Recent maps,
  Popular forks, Add your own, the first-run steps, the job panel and the messages.
- `run.rs`: starting Dioxus Native, frame timing, and layout before each mouse event.
- `canvas.rs`: the map widget, the mouse, and what is drawn over the map, including the air
  overlay's legend (sample tiles as pictures), its room labels and its hover box. The Problems
  overlay's badges go on top of the map's own rectangles, and its box takes the hover box's place.
- `bars.rs`: the menu, tool and status bars.
- `placing.rs`: the Placement block under the entity palette (lay a tile under walls, and
  which tile) and the Room tool's panel with its Intersection switch. A tile or a wall is chosen from a short list that
  opens under its row, since Blitz has no drop-down lists.
- `palettes.rs`: the side panel and the palettes. The Favourites tab comes first, each row has
  a star (`FavouriteStar`, shown on hover by `.fav-star` in `local.css`), and
  `SS14_EDITOR_TAB` is a developer switch for the tab it opens on.
- `selection.rs`, `components.rs`, `links.rs`: the side panels. `components.rs` has the typed
  forms. An enum with up to four names is a row of choices, a longer one is a dropdown list,
  and a colour is a hex field with a swatch. The layers panel, with the Air switch and its
  views and the Problems switch, is in `selection.rs`.
- `prefabs.rs`: the Prefabs and Grids tabs and their dialogs.
- `dialogs.rs`: the file dialogs, the text field, the dialog windows, and the marks drawn
  without a font (the star is a picture from `thumbs::star_uri`).
- `backdrops.rs`: the backdrop gallery in the Map menu.
- `air_fix.rs`: the Fix grid atmos dialog in the Map menu.
- `find.rs`: the search and Validate Map, with a button on each finding that has a fix (Fix,
  Remove extras, Lay plating or Select) and one on a group that fixes all of it at once. The
  list refreshes while it is open.
- `loading.rs`: the loading screen shown over the editor while the engine starts and a map
  opens, with the clown from `assets/clown.png`. Blitz takes a widget out of its
  `CustomWidgetAttr` only once, so each showing of the screen makes a new widget.
- `web.css` is the web editor's built stylesheet, copied over.
- `assets/` holds the clown and the app icon made from it (credited in `assets/CREDITS.md`).
  `build.rs` gives the icon to the Windows .exe.

## Checks

The Rust unit tests live apart from the code they test. A file's tests are in `src/tests/` at
the same path (`src/validate.rs` in `src/tests/validate.rs`, `src/session/mod.rs` in
`src/tests/session.rs`), declared in the file as `#[cfg(test)] #[path = "..."] mod tests;` so
they can still reach its private items. The tool tests, which drive a `Session::offline` with
the mouse and keys, are in `src/session/tools/tests/`, one file per tool.

The engine checks are commands of `Editor.Native`. They open real maps from the pinned
Starlight with the engine side you built, edit them, save them and read the saves back.

After changing C#, publish the engine side. Use your own runtime: `win-x64`, `linux-x64`,
`osx-arm64` or `osx-x64`.

```
dotnet publish src/Editor.Native -c Release -r win-x64 --self-contained false -p:PublishReadyToRun=true -p:TreatWarningsAsErrors=false -o src/Editor.Native/bin/r2r
```

On Windows, `tools/run.ps1 -Publish` does the same and also writes the code fingerprint of
upstream/starlight next to the engine side (`code-fingerprint.txt`). While an editor is open it
publishes into `src/Editor.Native/bin/r2r-next` instead, and the next `tools/run.ps1` moves it
into place. `tools/run.sh` publishes whenever the C# has changed. The editor refuses a fork
whose code differs from the fingerprint. A publish by hand leaves the previous build's
fingerprint in place, and the editor then only warns.

The checks start faster from a pack of the fork's prototypes. Make it once, from the
repository root:

```
dotnet src/Editor.Native/bin/r2r/Editor.Native.dll pack upstream/starlight native/cache/kernel-resources.zip
```

While you work, run `quick` after each C# change. It runs ten checks (`copies`, `regions`,
`moves`, `facts`, `containers`, `links`, `grids`, `airfix`, `visuals`, `saved`) on two small shuttles in
about a minute. Run it from the repository root with no arguments:

```
dotnet src/Editor.Native/bin/r2r/Editor.Native.dll quick
```

Its saves go into `native/out/quick`. A line starting with `skip` is a step the maps have
nothing for. It is counted, and it is not a pass. Some steps always skip on the shuttles: a
copy of entities on two grids, and a decal of a kind the game doesn't have. Only the stations
have those. `visuals` skips a plain cable T on one shuttle and a cable laid beside another on
the other. On one shuttle or the other it also skips a pump, a pipe on a layer of its own, a
duct beside a plumbing machine, and plumbing under the floor. Both skip the Disposals layer,
since neither shuttle has disposals. So before a large change lands, run the stations too
(below).

| You changed | Run |
|---|---|
| Rust tools or session | `cargo test -p editor-core` from `native/` (seconds, no engine) |
| Copy or paste in the kernel | `copies` |
| Region edits and the air they carry or clear (`air` ops, `Documents.MoveAir`), tile undo and redo | `regions` |
| Moving entities | `moves` |
| What the engine reports about prototypes and tiles, and the components the delete cleanup names | `facts` |
| Putting things into containers and taking them out | `containers` |
| The `component.fields` question (`ComponentFields.cs`) | `containers` (its last step, in `src/Editor.Native/ContainerCheck.Fields.cs`) |
| Device links and device lists (`src/Editor.Kernel/DeviceLinks.cs`) | `links` |
| Adding and removing grids, a map's backdrop, the `air` question and its airtight rules | `grids` |
| Fix grid atmos (`AirFix.cs`, the `air.fix` op) | `airfix` (`src/Editor.Native/AirFixCheck.cs`) |
| The grids a scene sends (a map that is a grid itself, as a planet's is, among them) and the tiles the view draws, cable joins, pipe colours, connectors and layers, plumbing (`Visuals*.cs`), how the view applies them, the draw order of things under the floor, and the kinds on the Plumbing and Disposals layers (Disposals is the view's own: `KindOf` in `ViewWorld.DrawList.cs`) | `visuals` (`src/Editor.Native/VisualsCheck*.cs`). Plumbing with ducts isn't on any quick map, so also run it on `Maps/_Starlight/MedTak/MedTakPointAlpha.yml` |
| The saved data and appearance values the view is sent (`SavedState.cs`, `VisualsDevices.cs`, `ViewWorld.Saved.cs`) | `saved` (`src/Editor.Native/VisualsCheck.Saved.cs`). Every saved field must read the same on the view's component before the entity starts, and every appearance value must be in the view's appearance data. Valves, doors and filters are drawn as their data says, and light fixtures as mapping mode shows them. It also counts the decals a file keeps on chunk entities against those sent and drawn, and checks that each frame of an animated state leads round its state. The shuttles have few of these, so run it on the stations too, with `Maps/_Starlight/Stations/Flotilla.yml` (doors saved welded) and `Box.yml` (a door saved open) |

To run one check on the stations, from the repository root:

```
dotnet src/Editor.Native/bin/r2r/Editor.Native.dll <copies|regions|moves|facts> upstream/starlight --cache native/cache/kernel-resources.zip --out native/out/<name> Maps/_Starlight/Stations/Cork.yml Maps/_Starlight/Stations/Bagel.yml
```

Put `all` in place of the check's name to run all ten in one process. On the two stations
that takes about thirteen minutes and 21 GB of memory. The stations catch what the shuttles
can't, like a belt holding six things or a second grid docked to the first. Every check prints
where its time went (`src/Editor.Native/CheckTimes.cs`). About four fifths of it is saving
whole maps, at roughly three seconds a save. On a shuttle a save takes about a fifth of a
second.

A save of Cork takes about 1.8 s and one of Bagel about 4.4 s. The first save in a process
takes about 4 s more. Nearly all of that is the engine writing every component of every
entity as data nodes to compare with the prototype's. The kernel's own share is under 10 ms.
Use `saves` as the check's name to measure this (`src/Editor.Native/SaveBench.cs`).

Use `airsurvey` as the check's name to fix each given map and report whether its stored air
comes out the way the file had it: identical to the text, in another order, by drift, or
otherwise. It writes nothing.

### Other forks

The editor builds an engine side for another fork into its cache (`forks/engines.rs`). You can
also build one by hand with
`native/target/release/ss14-editor --build-engine src/Editor.Native/bin/r2r <fork>`. Check it
with `keeps` on that fork's maps, run from the engine side in the cache, with a pack made by
that engine side's `pack`. To see whether the editor's engine patches fit a fork's engine
without building anything, run `tools/engine-patches.sh fit <fork>/RobustToolbox --native`.

## Rules

- Never change anything under `upstream/`. Test maps go to `native/out/`.
- An ordinary edit must read and write a map exactly as before, unless the pull request says
  otherwise.
- Comments, commit messages and docs use plain, short sentences. No semicolons, no em dashes.
- Keep each file's line endings.
