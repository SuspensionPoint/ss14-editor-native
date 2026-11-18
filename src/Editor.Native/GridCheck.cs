using System.Text.Json;
using System.Text.Json.Nodes;
using SS14.EditorKernel;
using YamlDotNet.RepresentationModel;
using static SS14.EditorNative.CheckKit;

namespace SS14.EditorNative;

/// <summary>
/// Checks the kernel's "grid.add" and "grid.remove".
///
/// On a new map, a second grid is added and given tiles, a thing standing on them, a door that a
/// button on the first grid is linked to, and a new name. The two grids are docked: a dock on each
/// names the other, and both grids have the joint. The map is saved, reopened and saved as the
/// same file. Then the new grid is removed. That must give back the file from before it, leave the
/// button linked to nothing and the dock docked to nothing, and undo must restore the file with
/// the grid, joint included.
///
/// On each map given, the air the kernel reads on the largest grid must be the air the file
/// stores.
///
/// On each map given that is a map file, the largest grid is removed with everything on it, and
/// undo must put back every tile, entity and link, exactly. Then the map's backdrop is set and
/// reset to the game's default, each change saved, reopened and undone. A shuttle is a grid file,
/// which holds one grid, so it must refuse both ops and a backdrop. If none of the given maps is
/// a map file, a small one from the fork stands in.
/// </summary>
public static class GridCheck
{
    private const string Floor = "Plating";
    private const string Item = "Crowbar";
    private const string Door = "Airlock";
    private const string Button = "SignalButton";

    /// <summary>A door that docks, and the name of the joint between two docked grids.</summary>
    private const string Dock = "AirlockShuttle";
    private const string Weld = "docking1";

    /// <summary>A small map file, for when every map given is a grid file.</summary>
    private const string SmallMap = "Maps/Lavaland/wrecked_outpost.yml";

    /// <returns>0 if every map passed.</returns>
    public static int Run(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        var session = EngineSession.Booted(forkRepo, kernelPack);
        var code = Run(session, forkRepo, maps, outFolder, say);
        CheckTimes.Report("grids", say);
        CheckTimes.Summary(say);
        return code;
    }

    /// <summary>The check on an engine that is up already.</summary>
    public static int Run(EngineSession session, string forkRepo, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        Directory.CreateDirectory(outFolder);
        var onNew = new Tally(say);
        say("a new map");
        try
        {
            NewMap(session, outFolder, say, onNew);
        }
        catch (Exception e)
        {
            onNew.Expect(false, $"stopped: {e.Message}");
        }

        var mapFiles = 0;
        bool Each(string map, Tally tally)
        {
            if (Check(session, forkRepo, map, outFolder, say, tally))
                mapFiles++;
            return true;
        }

        var code = OnEach(maps, say, "Grids came and went with all on them, and undo gave back the same file each time.", Each);
        if (mapFiles == 0)
        {
            if (File.Exists(Path.Combine(forkRepo, "Resources", SmallMap)))
                code |= OnEach([SmallMap], say, "The small map stood in for a map file.", Each);
            else
                CheckSkips.Skip(say, "a grid of a map removed with all on it: no map given is a map file");
        }

        return onNew.Failed == 0 ? code : 1;
    }

    private static void NewMap(EngineSession session, string outFolder, Action<string> say, Tally tally)
    {
        string Out(string step) => Path.Combine(outFolder, $"new.{step}.yml");

        var scene = session.New().Scene;
        var document = scene.Document.Id;
        try
        {
            // A floor on the grid a new map comes with, and a button on it.
            var home = scene.Grids[0].Uid;
            var steps = 0;
            SceneDelta Do(string label, params EditOp[] ops)
            {
                steps++;
                return Edit(session, document, label, ops);
            }

            EditOp[] Tiles(int grid) => (from x in Enumerable.Range(0, 3) from y in Enumerable.Range(0, 3) select new EditOp("tile", Grid: grid, X: x, Y: y, Tile: Floor)).ToArray();
            Do("Tiles", Tiles(home));
            var button = Do("Place", new EditOp("spawn", Prototype: Button, Parent: home, X: 0.5f, Y: 0.5f)).Created[0];
            // A dock on it too, and the component a grid keeps its joints in, with none yet.
            // A grid that loses its last joint keeps the component, so the file from before
            // the second grid has to have it.
            var homeDock = Do("Place", new EditOp("spawn", Prototype: Dock, Parent: home, X: 2.5f, Y: 1.5f)).Created[0];
            Do("Joints", new EditOp("components", Uid: home, Components: With(session, document, home, new JsonObject { ["type"] = "Joint" })));
            var before = Save(session, document, Out("before"));
            steps = 0;

            // ---- a second grid ----
            var (x, y) = (12.25f, -3.5f);
            var added = Do("Add grid", new EditOp("grid.add", X: x, Y: y, Name: "Check grid"));
            var made = added.Created.Count == 1 ? added.Created[0] : 0;
            var reported = added.GridsChanged.FirstOrDefault(grid => grid.Uid == made);
            tally.Expect(reported is { Name: "Check grid" } && reported.X == x && reported.Y == y && reported.Tiles.Count == 0,
                "a grid added is reported as made, empty, with its name and its place");

            var tiled = Do("Tiles", Tiles(made));
            var stood = Do("Place", new EditOp("spawn", Prototype: Item, Parent: made, X: 1.5f, Y: 1.5f), new EditOp("spawn", Prototype: Door, Parent: made, X: 0.5f, Y: 0.5f));
            var (thing, door) = stood.Created.Count == 2 ? (stood.Created[0], stood.Created[1]) : (0, 0);
            var renamed = Do("Rename", new EditOp("rename", Uid: made, Name: "Check grid two"));
            tally.Expect(tiled.Tiles.Count == 9 && stood.Entities.Count(entity => entity.Parent == made) == 2 && renamed.GridsChanged.Any(grid => grid.Uid == made && grid.Name == "Check grid two"),
                "it takes nine tiles and two things to stand on them, and \"rename\" names it");

            var link = new SceneLink(button, "Pressed", door, "Toggle");
            var linked = Do("Link", new EditOp("link", Uid: button, Port: link.SourcePort, Target: door, TargetPort: link.SinkPort));
            tally.Expect(linked.Linked.Any(entity => entity.Uid == button && entity.Links.Contains(link)), "the button on the first grid links to the door on the new one");

            // The two grids docked, as a map saved with a shuttle at a dock has them: each dock
            // names the other and the joint, and each grid has the joint that welds the two.
            var dock = Do("Place", new EditOp("spawn", Prototype: Dock, Parent: made, X: 0.5f, Y: 1.5f)).Created[0];
            JsonObject Docked(int with) => new() { ["type"] = "Docking", ["dockedWith"] = with, ["dockJointId"] = Weld };
            JsonObject Welded() => new()
            {
                ["type"] = "Joint",
                ["joints"] = new JsonObject
                {
                    [Weld] = new JsonObject { ["_ss14Tag"] = "WeldJoint", ["id"] = Weld, ["bodyA"] = home, ["bodyB"] = made, ["localAnchorA"] = "3,1.5", ["localAnchorB"] = "0,1.5" },
                },
            };
            Do("Dock",
                new EditOp("components", Uid: homeDock, Components: With(session, document, homeDock, Docked(dock))),
                new EditOp("components", Uid: dock, Components: With(session, document, dock, Docked(homeDock))),
                new EditOp("components", Uid: home, Components: With(session, document, home, Welded())),
                new EditOp("components", Uid: made, Components: With(session, document, made, Welded())));

            var withGrid = Save(session, document, Out("added"));
            tally.Expect(Invalid(withGrid) == Invalid(before), "the save names nothing that is not in it");
            tally.Expect(withGrid.Count(line => line.Contains($"{Weld}: !type:WeldJoint")) == 2 && withGrid.Count(line => line.Contains($"dockJointId: {Weld}")) == 2,
                "the two grids are docked: the save has the joint on both and two docks that name it");
            Reopen(session, tally, say, Out("added"), withGrid, [], scene.Entities.Count + 5, reopened =>
            {
                var grid = reopened.Grids.FirstOrDefault(found => found.Name == "Check grid two");
                tally.Expect(reopened.Grids.Count == 2 && grid != null && grid.Tiles.Count == 9 * 5 && grid.X == x && grid.Y == y
                             && reopened.Entities.Count(entity => entity.Parent == grid.Uid) == 3 && Links(session, reopened.Document.Id) == 1,
                    $"opened again it has {reopened.Grids.Count} grids, the new one with its name, its place, nine tiles and three things on it, and the link");
            });

            // ---- the new grid removed ----
            var removed = Do("Remove grid", new EditOp("grid.remove", Grid: made));
            tally.Expect(removed.Removed.Count == 4 && removed.Removed[^1] == made && removed.Removed.Contains(thing) && removed.Removed.Contains(door) && removed.Removed.Contains(dock),
                "the grid removed is reported gone, after the things that stood on it");
            tally.Expect(removed.Linked.Any(entity => entity.Uid == button && entity.Links.Count == 0) && Links(session, document) == 0,
                "the button's link to the door went with it, and the delta says so");
            // The dock left behind is docked with nothing, and the joint is gone from its grid.
            tally.Same(before, Save(session, document, Out("added-removed")), "with the new grid removed the save is the file from before it");

            var back = Undo(session, document);
            tally.Expect(back.GridsChanged.Any(grid => grid.Uid == made && grid.Tiles.Count == 9 * 5) && back.Entities.Count(entity => entity.Parent == made) == 3
                         && back.Linked.Any(entity => entity.Uid == button && entity.Links.Contains(link)),
                "undo reports the grid with its tiles, the things on it and the button's link");
            tally.Same(withGrid, Save(session, document, Out("added-removed-undone")), "undone, the save is the file with the grid, docked as it was");
            Redo(session, document);
            tally.Same(before, Save(session, document, Out("added-removed-redone")), "redone, the save is the file from before it");
            Undo(session, document);
            tally.Same(withGrid, Save(session, document, Out("added-removed-undone-again")), "undone again, the save is the file with the grid");
            steps--;

            // ---- what the kernel must refuse ----
            var refusals = new (string What, string? Said)[]
            {
                ("\"grid.remove\" of a thing that is no grid", Refusal(session, document, new EditOp("grid.remove", Grid: button))),
                ("\"delete\" of a grid", Refusal(session, document, new EditOp("delete", Uid: made))),
            };
            foreach (var (what, said) in refusals)
                tally.Expect(said != null, $"refused: {what} ({said})");

            for (; steps > 0; steps--)
                Undo(session, document);
            tally.Same(before, Save(session, document, Out("after")), "with all of it undone the save is the file from before it");
        }
        finally
        {
            session.Close(document);
        }
    }

    /// <returns>Whether the map is a map file, on which a grid could be removed.</returns>
    private static bool Check(EngineSession session, string forkRepo, string map, string outFolder, Action<string> say, Tally tally)
    {
        var name = Path.GetFileNameWithoutExtension(map);
        string Out(string step) => Path.Combine(outFolder, $"{name}.{step}.yml");

        var scene = Opened(session, CheckTimes.Open(map), File.ReadAllText(Path.Combine(forkRepo, "Resources", map)), map);
        var document = scene.Document.Id;
        try
        {
            Air(session, File.ReadAllText(Path.Combine(forkRepo, "Resources", map)), scene, say, tally);
            AirtightPieces(session, scene, tally);

            if (scene.Document.Category != "Map")
            {
                var second = Refusal(session, document, new EditOp("grid.add", X: 300, Y: 0));
                var none = Refusal(session, document, new EditOp("grid.remove", Grid: scene.Grids[0].Uid));
                tally.Expect(second != null && none != null, $"it is a grid file and refuses a second grid and the loss of its one ({second})");
                var mapUid = scene.Document.MapUid;
                var backdrop = Refusal(session, document, new EditOp("components", Uid: mapUid, Components: With(session, document, mapUid, "Parallax", Parallax("Water"))));
                tally.Expect(backdrop != null && backdrop.Contains("grid file", StringComparison.Ordinal), $"and it refuses a backdrop ({backdrop})");
                return false;
            }

            var before = Save(session, document, Out("before"));
            var firstLog = scene.Document.Log.Select(entry => entry.Message).ToHashSet();
            var linksBefore = Links(session, document);

            var parentOf = scene.Entities.ToDictionary(entity => entity.Uid, entity => entity.Parent);
            int GridOf(int uid)
            {
                while (parentOf.TryGetValue(uid, out var parent))
                    uid = parent;
                return uid;
            }

            // A map that is a grid itself, as a planet's is, keeps that grid.
            var mapEntity = scene.Document.MapUid;
            if (scene.Grids.Any(grid => grid.Uid == mapEntity))
            {
                var kept = Refusal(session, document, new EditOp("grid.remove", Grid: mapEntity));
                tally.Expect(kept != null, $"the map's own ground is a grid that is not removed ({kept})");
            }

            var removable = scene.Grids.Where(grid => grid.Uid != mapEntity).ToList();
            if (removable.Count == 0)
            {
                CheckSkips.Skip(say, "a grid removed: the map has no grid but its own ground");
                Backdrops(session, scene, before, Out, tally);
                return true;
            }

            var old = removable.OrderByDescending(grid => grid.Tiles.Count).First();
            var onIt = scene.Entities.Where(entity => GridOf(entity.Uid) == old.Uid).Select(entity => entity.Uid).ToHashSet();
            var emptied = Edit(session, document, "Remove grid", new EditOp("grid.remove", Grid: old.Uid));
            tally.Expect(emptied.Removed.Count == onIt.Count + 1 && emptied.Removed[^1] == old.Uid && emptied.Removed.Take(onIt.Count).All(onIt.Contains),
                $"the largest of its {scene.Grids.Count} grids removed is reported gone with the {onIt.Count} entities on it, itself last");
            var without = Save(session, document, Out("emptied"));
            tally.Expect(Invalid(without) <= Invalid(before), $"the save has no \"invalid\" the first file had not ({Invalid(without)} lines with the word, {Invalid(before)} before)");
            Reopen(session, tally, say, Out("emptied"), without, firstLog, scene.Entities.Count - onIt.Count, reopened =>
                tally.Expect(reopened.Grids.Count == scene.Grids.Count - 1, $"opened again it has {reopened.Grids.Count} grids"));

            var restored = Undo(session, document);
            var grid = restored.GridsChanged.FirstOrDefault(found => found.Uid == old.Uid);
            tally.Expect(grid != null && grid.Tiles.SequenceEqual(old.Tiles) && grid.Decals.Count == old.Decals.Count && restored.Entities.Select(entity => entity.Uid).ToHashSet().SetEquals(onIt),
                $"undo reports the grid with its {old.Tiles.Count / 5} tiles and {old.Decals.Count} decals, and every entity that was on it");
            tally.Expect(Links(session, document) == linksBefore, $"its {linksBefore} links and listed devices are back");
            tally.Same(before, Save(session, document, Out("emptied-undone")), "undone, the save is the first file");
            Redo(session, document);
            tally.Same(without, Save(session, document, Out("emptied-redone")), "redone, the save is the file without the grid");
            Undo(session, document);
            tally.Same(before, Save(session, document, Out("after")), "undone again, the save is the first file");

            Backdrops(session, scene, before, Out, tally);
            return true;
        }
        finally
        {
            session.Close(document);
        }
    }

    // ---- the air ----

    /// <summary>A mix as a map file writes it: its temperature, its moles by gas name, its volume and whether it is the map's own.</summary>
    internal sealed record FileMix(float Temperature, Dictionary<string, float> Moles, float Volume, bool Immutable);

    /// <summary>
    /// The air the kernel reads on the largest grid against the map file: as many tiles hold
    /// air, each holds the mix the file gives it, and the file's mixes are the kernel's. Then a
    /// floor painted on an empty cell beside the grid is a floor with no air, and undone it is gone.
    /// </summary>
    private static void Air(EngineSession session, string yaml, Scene scene, Action<string> say, Tally tally)
    {
        var document = scene.Document.Id;
        var grid = scene.Grids.OrderByDescending(found => found.Tiles.Count).First();
        var file = FileAir(yaml, grid.YamlUid);
        if (file == null)
        {
            CheckSkips.Skip(say,$"the air of {grid.Name}: the file stores none, or in an older form");
            return;
        }

        var (fileMixes, fileTiles) = file.Value;
        var asked = System.Diagnostics.Stopwatch.StartNew();
        using var answer = Ask(session, "air", $"{{\"doc\":{document},\"grids\":[{grid.Uid}]}}");
        var readMs = asked.ElapsedMilliseconds;
        var gases = answer.RootElement.GetProperty("gases").EnumerateArray().Select(gas => gas.GetString() ?? "").ToArray();
        var read = answer.RootElement.GetProperty("grids").EnumerateArray().Single();
        var mixes = read.GetProperty("mixes").EnumerateArray().ToArray();
        var tiles = read.GetProperty("tiles").EnumerateArray().Select(number => number.GetInt32()).ToArray();
        var mixOf = new Dictionary<(int, int), int>();
        for (var at = 0; at + 2 < tiles.Length; at += 3)
            mixOf[(tiles[at], tiles[at + 1])] = tiles[at + 2];

        bool Same(JsonElement mix, FileMix expected)
        {
            var moles = mix.GetProperty("moles").EnumerateArray().Select(number => number.GetSingle()).ToArray();
            bool Near(float a, float b) => MathF.Abs(a - b) <= 1e-4f * MathF.Max(1, MathF.Abs(b));
            return Near(mix.GetProperty("temperature").GetSingle(), expected.Temperature)
                   && gases.Select((gas, index) => Near(moles[index], expected.Moles.GetValueOrDefault(gas))).All(held => held);
        }

        var wrong = fileTiles.Count(tile => !mixOf.TryGetValue(tile.Key, out var index) || !Same(mixes[index], fileMixes[tile.Value]));
        tally.Expect(mixOf.Count == fileTiles.Count && wrong == 0,
            $"the air of {grid.Name}, read in {readMs} ms: {mixOf.Count} tiles hold air, as the file's {fileTiles.Count} do, {wrong} with another mix than the file's");
        var used = fileTiles.Values.Distinct().Count();
        tally.Expect(mixes.Length == used, $"its {mixes.Length} mixes are the {used} the file's tiles use");

        int Pairs(JsonElement of, string name) => of.GetProperty(name).GetArrayLength() / 2;
        var walls = Pairs(read, "blocked");
        tally.Expect(walls > 0, $"{walls} of its tiles are walls that hold no air, and {Pairs(read, "unfilled")} are floors that hold none");

        // The game's own last fix of the air against the kernel's airtight rules: a tile they
        // call blocked on all sides with no air should store none. A tile open to the map's
        // atmosphere keeps the map's air under a wall too.
        var flags = Cells(read);
        bool Wall(int cell) => (cell & (Blocks | Open)) == Blocks;
        // A few may: a wall placed after the air was last fixed keeps it until the game's
        // first tick. Pursuit has none, Bagel and Cork none, the medical shuttle one.
        var held = flags.Count(cell => Wall(cell.Value) && mixOf.ContainsKey(cell.Key));
        var sides = flags.Count(cell => (cell.Value & 15) is not (0 or 15) && (cell.Value & Open) == 0);
        tally.Expect(flags.Count == grid.Tiles.Count / 5 && held <= 2,
            $"the airtight rules name each of its {flags.Count} tiles. {flags.Count(cell => Wall(cell.Value))} are walls with no air, of which the file stores air on {held}, "
            + $"{flags.Count(cell => (cell.Value & (Blocks | Open)) == 15)} are blocked on all sides and keep their air, {sides} are blocked on some sides, and {flags.Count(cell => (cell.Value & Open) != 0)} are open to the map's atmosphere");

        // An empty cell beside a tile of the grid that stores no air either.
        var occupied = new HashSet<(int, int)>();
        for (var at = 0; at + 4 < grid.Tiles.Count; at += 5)
            occupied.Add((grid.Tiles[at], grid.Tiles[at + 1]));
        var beside = occupied.Select(cell => (cell.Item1 + 1, cell.Item2)).Where(cell => !occupied.Contains(cell) && !mixOf.ContainsKey(cell)).ToArray();
        if (beside.Length == 0)
        {
            CheckSkips.Skip(say, $"a floor painted beside {grid.Name}: every empty cell beside it stores air");
            return;
        }

        var empty = beside[0];

        bool Unfilled()
        {
            using var again = Ask(session, "air", $"{{\"doc\":{document},\"grids\":[{grid.Uid}]}}");
            var cells = again.RootElement.GetProperty("grids")[0].GetProperty("unfilled").EnumerateArray().Select(number => number.GetInt32()).ToArray();
            return Enumerable.Range(0, cells.Length / 2).Any(pair => (cells[pair * 2], cells[pair * 2 + 1]) == empty);
        }

        Edit(session, document, "Tiles", new EditOp("tile", Grid: grid.Uid, X: empty.Item1, Y: empty.Item2, Tile: Floor));
        var painted = Unfilled();
        Undo(session, document);
        tally.Expect(painted && !Unfilled(), $"a floor painted at {empty} is a floor with no air, and undone it is gone");
    }

    /// <summary>A cell's flags for a wall: all four sides blocked (15) and no air (16).</summary>
    private const int Blocks = 31;

    /// <summary>A cell's flag for a tile open to the map's atmosphere.</summary>
    private const int Open = 32;

    /// <summary>The flags of each tile of a grid in the kernel's air answer: x, y and the flags, three numbers a tile.</summary>
    private static Dictionary<(int, int), int> Cells(JsonElement grid)
    {
        var numbers = grid.GetProperty("cells").EnumerateArray().Select(number => number.GetInt32()).ToArray();
        var cells = new Dictionary<(int, int), int>();
        for (var at = 0; at + 2 < numbers.Length; at += 3)
            cells[(numbers[at], numbers[at + 1])] = numbers[at + 2];
        return cells;
    }

    /// <summary>
    /// Known pieces, each placed alone on a floor, and the sides of its cell it must block as a
    /// round starts: north 1, south 2, east 4, west 8, and 16 for no air when all four are.
    /// Each is airtight, which fixes vacuum, 64, whether it blocks or not. The turn is in
    /// quarters against the clock.
    /// </summary>
    private static readonly (string Prototype, int Quarters, int Flags, string What)[] Pieces =
    [
        ("WallSolid", 0, 31 | FixVacuum, "a wall blocks all sides and holds no air"),
        ("Airlock", 0, 15 | FixVacuum, "a closed airlock blocks all sides and keeps its air"),
        ("Firelock", 0, FixVacuum, "a firelock is open and blocks nothing, and still fixes vacuum"),
        ("BlastDoor", 0, 31 | FixVacuum, "a closed blast door blocks all sides, though its data says it blocks nothing"),
        ("BlastDoorOpen", 0, FixVacuum, "an open blast door blocks nothing"),
        ("WindowDirectional", 0, 2 | FixVacuum, "a thin window blocks south"),
        ("WindowDirectional", 1, 4 | FixVacuum, "turned a quarter, east"),
        ("WindowDirectional", 2, 1 | FixVacuum, "turned a half, north"),
        ("WindowDirectional", 3, 8 | FixVacuum, "turned three quarters, west"),
        ("Windoor", 0, 2 | FixVacuum, "a windoor blocks south"),
        ("Windoor", 2, 1 | FixVacuum, "turned a half, north"),
        ("WindowDiagonal", 0, 6 | FixVacuum, "a diagonal window blocks south and east"),
        ("WindowDiagonal", 1, 5 | FixVacuum, "turned a quarter, east and north"),
    ];

    /// <summary>A cell's flag for an airtight thing, which the game lets fill a floor with no air from its neighbours.</summary>
    private const int FixVacuum = 64;

    /// <summary>
    /// The airtight rules on known pieces: a row of floors below the grid, joined to it, one
    /// piece on each, turned as given. The kernel's air answer must tell the sides each blocks.
    /// One edit, undone after.
    /// </summary>
    private static void AirtightPieces(EngineSession session, Scene scene, Tally tally)
    {
        var document = scene.Document.Id;
        var grid = scene.Grids.OrderByDescending(found => found.Tiles.Count).First();
        var cells = Enumerable.Range(0, grid.Tiles.Count / 5).Select(at => (X: grid.Tiles[at * 5], Y: grid.Tiles[at * 5 + 1])).ToArray();
        var bottom = cells.Min(cell => cell.Y);
        var left = cells.Where(cell => cell.Y == bottom).Min(cell => cell.X);
        var row = Enumerable.Range(0, Pieces.Length).Select(at => (X: left + at, Y: bottom - 1)).ToArray();

        var ops = row.Select(cell => new EditOp("tile", Grid: grid.Uid, X: cell.X, Y: cell.Y, Tile: Floor)).ToList();
        ops.AddRange(Pieces.Select((piece, at) => new EditOp("spawn", Prototype: piece.Prototype, Parent: grid.Uid, X: row[at].X + 0.5f, Y: row[at].Y + 0.5f, Rotation: piece.Quarters * Math.PI / 2)));
        Edit(session, document, "Pieces", ops.ToArray());
        try
        {
            using var answer = Ask(session, "air", $"{{\"doc\":{document},\"grids\":[{grid.Uid}]}}");
            var flags = Cells(answer.RootElement.GetProperty("grids")[0]);
            var wrong = Pieces.Select((piece, at) => (piece, Told: flags.GetValueOrDefault(row[at], -1))).Where(pair => pair.Told != pair.piece.Flags).ToList();
            tally.Expect(wrong.Count == 0,
                wrong.Count == 0
                    ? $"the airtight rules on {Pieces.Length} known pieces: {string.Join(", ", Pieces.Select(piece => piece.What))}"
                    : $"the airtight rules on known pieces: {string.Join(", ", wrong.Select(pair => $"{pair.piece.Prototype} turned {pair.piece.Quarters} quarters gives {pair.Told}, not {pair.piece.Flags}"))}");
        }
        finally
        {
            Undo(session, document);
        }
    }

    /// <summary>
    /// The air a map file stores on one grid, as its mixes and the mix of each tile, or null if
    /// it stores none or in a form older than version 2.
    /// </summary>
    internal static (List<FileMix> Mixes, Dictionary<(int, int), int> Tiles)? FileAir(string yaml, int yamlUid)
    {
        var stream = new YamlStream();
        stream.Load(new StringReader(yaml));
        var root = (YamlMappingNode) stream.Documents[0].RootNode;
        var atmosphere = ((YamlSequenceNode) root["entities"]).Children
            .SelectMany(prototype => ((YamlSequenceNode) prototype["entities"]).Children.Cast<YamlMappingNode>())
            .Where(entity => ((YamlScalarNode) entity["uid"]).Value == yamlUid.ToString())
            .SelectMany(entity => entity.Children.TryGetValue("components", out var components) ? ((YamlSequenceNode) components).Children.Cast<YamlMappingNode>() : [])
            .FirstOrDefault(component => ((YamlScalarNode) component["type"]).Value == "GridAtmosphere");
        if (atmosphere == null || !atmosphere.Children.TryGetValue("version", out var version) || ((YamlScalarNode) version).Value != "2")
            return null;

        var data = (YamlMappingNode) atmosphere["data"];
        string Text(YamlNode node) => ((YamlScalarNode) node).Value ?? "";
        float Number(YamlNode node) => float.Parse(Text(node), System.Globalization.CultureInfo.InvariantCulture);
        var mixes = new List<FileMix>();
        if (data.Children.TryGetValue("uniqueMixes", out var unique))
        {
            foreach (var mix in ((YamlSequenceNode) unique).Children.Cast<YamlMappingNode>())
            {
                // A mix that names no temperature has the game's default, that of space.
                var temperature = mix.Children.TryGetValue("temperature", out var told) ? Number(told) : Content.Shared.Atmos.Atmospherics.TCMB;
                // Moles by gas name, or in an older file a list in the order of the gases.
                mix.Children.TryGetValue("moles", out var gases);
                var moles = gases switch
                {
                    YamlMappingNode named => named.Children.ToDictionary(gas => Text(gas.Key), gas => Number(gas.Value)),
                    YamlSequenceNode listed => listed.Children.Select((amount, gas) => (Name: ((Content.Shared.Atmos.Gas) gas).ToString(), Amount: Number(amount))).ToDictionary(gas => gas.Name, gas => gas.Amount),
                    _ => new Dictionary<string, float>(),
                };
                var volume = mix.Children.TryGetValue("volume", out var litres) ? Number(litres) : 0;
                var immutable = mix.Children.TryGetValue("immutable", out var marked) && Text(marked) == "True";
                mixes.Add(new FileMix(temperature, moles, volume, immutable));
            }
        }

        var chunkSize = data.Children.TryGetValue("chunkSize", out var size) ? int.Parse(Text(size)) : 4;
        var tiles = new Dictionary<(int, int), int>();
        // A grid that stores no air has no tiles.
        IEnumerable<KeyValuePair<YamlNode, YamlNode>> chunks = data.Children.TryGetValue("tiles", out var stored) ? ((YamlMappingNode) stored).Children : [];
        foreach (var (key, chunk) in chunks)
        {
            var origin = Text(key).Split(',').Select(int.Parse).ToArray();
            foreach (var (index, mask) in ((YamlMappingNode) chunk).Children)
            {
                var bits = uint.Parse(Text(mask));
                for (var bit = 0; bit < chunkSize * chunkSize; bit++)
                {
                    if ((bits & (1u << bit)) != 0)
                        tiles[(origin[0] * chunkSize + bit % chunkSize, origin[1] * chunkSize + bit / chunkSize)] = int.Parse(Text(index));
                }
            }
        }

        return (mixes, tiles);
    }

    // ---- the map's backdrop ----

    /// <summary>
    /// A backdrop set on the map, saved, opened again and read back, and undone. Then the
    /// game's default, which takes the map's own away: on the map if it names one, and else on
    /// the save that does. Last, the map made a planet's, which refuses a backdrop.
    /// </summary>
    private static void Backdrops(EngineSession session, Scene scene, string[] first, Func<string, string> Out, Tally tally)
    {
        var (document, map) = (scene.Document.Id, scene.Document.MapUid);
        var names = session.BackdropNames();
        tally.Expect(names.Contains("Default") && names.SequenceEqual(names.Order(StringComparer.OrdinalIgnoreCase)),
            $"the fork has {names.Count} backdrops, listed in order, the game's default among them");
        var (own, _, chosen) = session.OnDocumentKernel(kernel => kernel.Documents.Backdrop(document));
        var other = names.First(name => name != own && name != "Default");

        var set = BackdropRoundTrip(session, document, map, other, first, Out("backdrop"), tally);
        if (chosen)
        {
            BackdropRoundTrip(session, document, map, null, first, Out("backdrop-default"), tally);
        }
        else
        {
            var path = Out("backdrop");
            var named = Opened(session, CheckTimes.Reopen, File.ReadAllText(path), Path.GetFileName(path));
            try
            {
                var left = BackdropRoundTrip(session, named.Document.Id, named.Document.MapUid, null, set, Out("backdrop-default"), tally);
                tally.Same(first, left, "the save of it with the game's default is the first file");
            }
            finally
            {
                session.Close(named.Document.Id);
            }
        }

        var planet = Edit(session, document, "Planet", new EditOp("components", Uid: map, Components: With(session, document, map, new JsonObject { ["type"] = "Biome", ["enabled"] = false })));
        var refused = Refusal(session, document, new EditOp("components", Uid: map, Components: With(session, document, map, "Parallax", Parallax(other))));
        tally.Expect(planet.Details.Any(details => details.Uid == map) && refused != null && refused.Contains("planet", StringComparison.Ordinal), $"made a planet's, the map refuses a backdrop ({refused})");
        Undo(session, document);
        tally.Same(first, Save(session, document, Out("backdrop-planet-undone")), "undone, the save is the first file");
    }

    /// <summary>
    /// Sets the map's backdrop, or with no name the game's default, and checks the save, the
    /// save opened again, and undo.
    /// </summary>
    /// <returns>The save with the backdrop set.</returns>
    private static string[] BackdropRoundTrip(EngineSession session, int document, int map, string? name, string[] first, string path, Tally tally)
    {
        var what = name == null ? "the game's default" : $"\"{name}\"";
        var changed = Edit(session, document, "Set backdrop", new EditOp("components", Uid: map, Components: With(session, document, map, "Parallax", name == null ? null : Parallax(name))));
        tally.Expect(changed.Details.Any(details => details.Uid == map) && changed.History.UndoLabel == "Set backdrop", $"{what} set on the map is one undo step that reports the map's components");
        var saved = Save(session, document, path);
        tally.Expect(BackdropOf(saved) == name, $"the save names {what} ({BackdropOf(saved) ?? "none"})");
        tally.Same(WithoutBackdrop(first), WithoutBackdrop(saved), "apart from the backdrop the save is the file before");

        var reopened = Opened(session, CheckTimes.Reopen, File.ReadAllText(path), Path.GetFileName(path));
        try
        {
            var (read, drawn, chosen) = session.OnDocumentKernel(kernel => kernel.Documents.Backdrop(reopened.Document.Id));
            tally.Expect(read == (name ?? "Default") && chosen == (name != null) && drawn, $"opened again its backdrop is {what} (\"{read}\", named by the map: {chosen})");
        }
        finally
        {
            session.Close(reopened.Document.Id);
        }

        Undo(session, document);
        tally.Same(first, Save(session, document, Path.ChangeExtension(path, ".undone.yml")), "undone, the save is the file before");
        return saved;
    }

    private static JsonObject Parallax(string name) => new() { ["type"] = "Parallax", ["parallax"] = name };

    /// <summary>The backdrop a save names on its map, or null.</summary>
    private static string? BackdropOf(string[] lines)
    {
        var at = Array.FindIndex(lines, line => line.Trim() == "- type: Parallax");
        if (at < 0)
            return null;

        var next = at + 1 < lines.Length ? lines[at + 1].Trim() : "";
        return next.StartsWith("parallax: ", StringComparison.Ordinal) ? next["parallax: ".Length..] : "Default";
    }

    /// <summary>A save with its map's Parallax component left out.</summary>
    private static string[] WithoutBackdrop(string[] lines)
    {
        var at = Array.FindIndex(lines, line => line.Trim() == "- type: Parallax");
        if (at < 0)
            return lines;

        var length = at + 1 < lines.Length && lines[at + 1].Trim().StartsWith("parallax: ", StringComparison.Ordinal) ? 2 : 1;
        return [.. lines[..at], .. lines[(at + length)..]];
    }

    /// <summary>An entity's component data, as "details" gives it, with this component in place of its own of the kind.</summary>
    private static string With(EngineSession session, int document, int uid, JsonObject component) =>
        With(session, document, uid, component["type"]!.GetValue<string>(), component);

    /// <summary>The same, with none of the kind if <paramref name="component"/> is null.</summary>
    private static string With(EngineSession session, int document, int uid, string type, JsonObject? component)
    {
        using var details = Ask(session, "details", $"{{\"doc\":{document},\"uids\":[{uid}]}}");
        var components = JsonNode.Parse(details.RootElement[0].GetProperty("components").GetString() ?? "[]")!.AsArray();
        foreach (var old in components.OfType<JsonObject>().Where(old => old["type"]?.GetValue<string>() == type).ToArray())
            components.Remove(old);
        if (component != null)
            components.Add(component);
        return components.ToJsonString();
    }

    /// <summary>How many links and listed devices the document has.</summary>
    private static int Links(EngineSession session, int document)
    {
        using var answer = Ask(session, "links", $"{{\"doc\":{document}}}");
        return answer.RootElement.GetProperty("links").GetArrayLength() + answer.RootElement.GetProperty("lists").GetArrayLength();
    }

    /// <summary>
    /// Opens a save again: nothing may be logged that the map did not log when first opened,
    /// the count of entities must be right, and saved again it must be the same file.
    /// </summary>
    private static void Reopen(EngineSession session, Tally tally, Action<string> say, string path, string[] saved, HashSet<string> firstLog, int entities, Action<Scene> look)
    {
        var reopened = Opened(session, CheckTimes.Reopen, File.ReadAllText(path), Path.GetFileName(path));
        try
        {
            var logged = reopened.Document.Log.Where(entry => !firstLog.Contains(entry.Message)).ToArray();
            if (!tally.Expect(logged.Length == 0 && reopened.Entities.Count == entities, $"opened again it has {reopened.Entities.Count} entities (expected {entities}) and logs {logged.Length} new messages"))
            {
                foreach (var entry in logged.Take(6))
                    say($"        {entry.Level} {entry.Sawmill}: {entry.Message}");
            }

            look(reopened);
            tally.Same(saved, Save(session, reopened.Document.Id, Path.ChangeExtension(path, ".again.yml")), "saved again it is the same file");
        }
        finally
        {
            session.Close(reopened.Document.Id);
        }
    }
}
