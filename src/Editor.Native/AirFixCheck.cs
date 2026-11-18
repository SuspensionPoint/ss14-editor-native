using System.Text.Json;
using Content.Shared.Atmos;
using SS14.EditorKernel;
using static SS14.EditorNative.CheckKit;
using FileMix = SS14.EditorNative.GridCheck.FileMix;

namespace SS14.EditorNative;

/// <summary>
/// Checks the kernel's "air.fix", which is the game's fixgridatmos applied to the data (AirFix.cs).
///
/// On a new map, the grid starts with no atmosphere, and the fix alone is refused just as the game
/// refuses it. Given one by "air.add" (as the game's atmos:add does) and fixed in the same step,
/// its floors store standard air, and undo gives back the file from before. Painted up to the
/// weight of seven tiles, it gets an atmosphere by itself, as the game's AutomaticAtmosSystem
/// gives one (NewMap).
///
/// On each map given, the map is first fixed as it is. What the fix changed is reported by kind
/// (floors that had no air, drift from the simulation, air that was set otherwise, cells off the
/// grid) and then undone. Next, one of each of the game's atmos fix markers is placed on floors of
/// the largest grid, and every grid with an atmosphere is fixed in one edit. The save must then
/// store standard air at 20 °C on every floor not under a marker, none on walls, the map's
/// immutable air on space and lattice, none on any cell off the grid, and each marker's own mix
/// under it. The kernel's air answer, which the overlay draws, must match the save's air. What the
/// fix said beforehand it would change must be what changed. Fixing again changes nothing, undo
/// gives back the file from before, and redo gives back the fixed one.
/// </summary>
public static class AirFixCheck
{
    /// <summary>The game's atmos fix markers and the mix each asks for (Resources/Prototypes/Entities/Markers/atmos_blocker.yml).</summary>
    private static readonly (string Prototype, int Mode)[] Markers =
    [
        ("AtmosFixBlockerMarker", 1),
        ("AtmosFixOxygenMarker", 2),
        ("AtmosFixNitrogenMarker", 3),
        ("AtmosFixPlasmaMarker", 4),
        ("AtmosFixInstantPlasmaFireMarker", 5),
        ("AtmosFixFreezerMarker", 6),
        ("AtmosFixVoxMarker", 7),
        ("AtmosFixAirMarker", 8),
    ];

    /// <summary>
    /// The mix of each mode, as the check expects it from the game's numbers: its moles by gas
    /// and its temperature (AtmosphereSystem.Commands.cs 71-107). Written out again here, and
    /// not taken from the kernel, so that the two are compared.
    /// </summary>
    private static (Dictionary<string, float> Moles, float Temperature) Expected(int mode)
    {
        Dictionary<string, float> Of(params (Gas Gas, float Moles)[] gases) => gases.ToDictionary(gas => gas.Gas.ToString(), gas => gas.Moles);
        return mode switch
        {
            0 => (Of((Gas.Oxygen, Atmospherics.OxygenMolesStandard), (Gas.Nitrogen, Atmospherics.NitrogenMolesStandard)), Atmospherics.T20C),
            1 => (Of(), Atmospherics.T20C),
            2 => (Of((Gas.Oxygen, Atmospherics.MolesCellGasMiner)), Atmospherics.T20C),
            3 => (Of((Gas.Nitrogen, Atmospherics.MolesCellGasMiner)), Atmospherics.T20C),
            4 => (Of((Gas.Plasma, Atmospherics.MolesCellGasMiner)), Atmospherics.T20C),
            5 => (Of((Gas.Oxygen, Atmospherics.MolesCellGasMiner), (Gas.Plasma, Atmospherics.MolesCellGasMiner)), 5000f),
            6 => (Of((Gas.Oxygen, Atmospherics.OxygenMolesFreezer), (Gas.Nitrogen, Atmospherics.NitrogenMolesFreezer)), Atmospherics.FreezerTemp),
            7 => (Of((Gas.Nitrogen, Atmospherics.MolesCellStandard)), Atmospherics.T20C),
            8 => (Of((Gas.Oxygen, Atmospherics.OxygenMolesGasMiner), (Gas.Nitrogen, Atmospherics.NitrogenMolesGasMiner)), Atmospherics.T20C),
            _ => throw new ArgumentOutOfRangeException(nameof(mode)),
        };
    }

    // The flags of a cell in the kernel's air answer (Air.cs).
    private const int Sides = 15;
    private const int NoAir = 16;
    private const int Open = 32;

    /// <returns>0 if every map passed.</returns>
    public static int Run(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        var session = EngineSession.Booted(forkRepo, kernelPack);
        var code = Run(session, forkRepo, maps, outFolder, say);
        CheckTimes.Report("airfix", say);
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
            NewMap(session, outFolder, onNew);
        }
        catch (Exception e)
        {
            onNew.Expect(false, $"stopped: {e.Message}");
        }

        var code = OnEach(maps, say, "Every grid was fixed as the game fixes it, a second fix changed nothing, and undo gave back the file before.", (map, tally) =>
        {
            Check(session, forkRepo, map, outFolder, say, tally);
            return true;
        });
        return onNew.Failed == 0 ? code : 1;
    }

    /// <summary>
    /// A new map's grid has no atmosphere: the preview says so and the fix alone is refused.
    /// Six floors painted on it, too few for the game to give it one by itself. Then "air.add"
    /// and "air.fix" in one edit, as the game's atmos:add and fixgridatmos: the save has the
    /// grid's GridAtmosphere and gas overlay, standard air on every floor, and opens again.
    /// Undo gives back the file before to the byte, redo the fixed one. A grid that has an
    /// atmosphere is refused one more. Then the game's own rule: two floors more make the grid
    /// weigh more than seven tiles and the paint gives it an atmosphere in its own step, and a
    /// grid added with "grid.add" has none until it is painted so.
    /// </summary>
    private static void NewMap(EngineSession session, string outFolder, Tally tally)
    {
        string Out(string step) => Path.Combine(outFolder, $"new.{step}.yml");
        var scene = session.New().Scene;
        var document = scene.Document.Id;
        try
        {
            var grid = scene.Grids[0].Uid;
            using (var preview = Ask(session, "air.fix.preview", $"{{\"doc\":{document}}}"))
            {
                var told = preview.RootElement.EnumerateArray().Single();
                var said = told.GetProperty("refused").GetString();
                var refused = Refusal(session, document, new EditOp("air.fix", Grid: grid));
                tally.Expect(said != null && told.GetProperty("addable").GetBoolean() && refused != null && refused.Contains("no atmosphere", StringComparison.Ordinal) && !session.Scene(document).History.CanUndo,
                    $"a new map's grid has no atmosphere, the preview offers to add one, and the fix alone is refused as fixgridatmos refuses it ({refused})");
            }

            // ---- six floors, too few for the game to give the grid an atmosphere ----
            var floors = (from x in Enumerable.Range(0, 2) from y in Enumerable.Range(0, 3) select (X: x, Y: y)).ToArray();
            var painted = Edit(session, document, "Paint", floors.Select(cell => new EditOp("tile", Grid: grid, X: cell.X, Y: cell.Y, Tile: "FloorSteel")).ToArray());
            tally.Expect(painted.Air.Count == 0 && !Grids(session, document)[grid].Atmosphere, "six floors painted, the grid still has no atmosphere, as the game gives one only at the weight of seven tiles");
            var first = Save(session, document, Out("painted"));

            using (var preview = Ask(session, "air.fix.preview", $"{{\"doc\":{document}}}"))
            {
                var told = preview.RootElement.EnumerateArray().Single();
                tally.Expect(told.GetProperty("addable").GetBoolean() && told.GetProperty("tiles").GetInt32() == floors.Length,
                    $"the preview counts the {told.GetProperty("tiles").GetInt32()} floors that adding an atmosphere and fixing would fill");
            }

            // ---- added and fixed as one step ----
            var fixedDelta = Edit(session, document, "Fix grid atmos", new EditOp("air.add", Grid: grid), new EditOp("air.fix", Grid: grid));
            tally.Expect(fixedDelta.History.UndoLabel == "Fix grid atmos" && fixedDelta.Air.Contains(grid) && Grids(session, document)[grid].Atmosphere,
                "\"air.add\" and \"air.fix\" are one step, and the delta names the grid");
            var fixedLines = Save(session, document, Out("fixed"));
            var fixedText = string.Join("\n", fixedLines);
            tally.Expect(fixedLines.Count(line => line.Trim() == "- type: GridAtmosphere") == 1 && fixedLines.Count(line => line.Trim() == "- type: GasTileOverlay") == 1,
                "the save has the grid's GridAtmosphere and GasTileOverlay, as the game's command and atmosphere system give it");

            Scene? reopened = null;
            try
            {
                reopened = session.Open(fixedText, Out("fixed")).Scene;
                var air = GridCheck.FileAir(fixedText, reopened.Grids[0].YamlUid);
                var wrong = air is { } stored ? floors.Count(cell => !stored.Tiles.TryGetValue(cell, out var index) || !Holds(stored.Mixes[index], Expected(0))) : floors.Length;
                tally.Expect(air != null && wrong == 0 && air.Value.Tiles.Count == floors.Length && Grids(session, reopened.Document.Id)[reopened.Grids[0].Uid].Atmosphere,
                    $"the save opens again, and its {floors.Length} floors store standard air at 293.15 K ({wrong} do not), and nothing else does");
            }
            catch (Exception e)
            {
                tally.Expect(false, $"the save opens again: {e.Message.Split('\n')[0]}");
            }
            finally
            {
                if (reopened != null)
                    session.Close(reopened.Document.Id);
            }

            var again = Refusal(session, document, new EditOp("air.add", Grid: grid));
            tally.Expect(again != null && again.Contains("already", StringComparison.Ordinal), $"a grid that has an atmosphere is refused another, as addatmos refuses it ({again})");

            Undo(session, document);
            tally.Expect(!Grids(session, document)[grid].Atmosphere, "undone, the grid has no atmosphere");
            Same(first, Save(session, document, Out("fixed-undone")), "undone, the save is the file before", tally);
            Redo(session, document);
            Same(fixedLines, Save(session, document, Out("fixed-redone")), "redone, the save is the fixed file", tally);
            Undo(session, document);

            // ---- the game's own rule: an atmosphere at the weight of seven tiles ----
            var more = Edit(session, document, "Paint", new EditOp("tile", Grid: grid, X: 0, Y: 3, Tile: "FloorSteel"), new EditOp("tile", Grid: grid, X: 1, Y: 3, Tile: "FloorSteel"));
            var gained = Save(session, document, Out("gained"));
            tally.Expect(more.History.UndoLabel == "Paint" && more.Air.Contains(grid) && Grids(session, document)[grid].Atmosphere && gained.Count(line => line.Trim() == "- type: GridAtmosphere") == 1,
                "two floors more, eight in all, and the paint gives the grid an atmosphere in its own step, as the game's AutomaticAtmosSystem does");
            Undo(session, document);
            Same(first, Save(session, document, Out("gained-undone")), "the paint undone, the atmosphere goes with it and the save is the file before", tally);
            Redo(session, document);
            Same(gained, Save(session, document, Out("gained-redone")), "redone, it comes back", tally);
            Undo(session, document);

            var added = Edit(session, document, "Add grid", new EditOp("grid.add", X: 20, Y: 0));
            var made = added.Created.Count == 1 ? added.Created[0] : 0;
            var madeAir = Grids(session, document).GetValueOrDefault(made);
            tally.Expect(made != 0 && !madeAir.Atmosphere && added.Air.Count == 0, "a grid added has no atmosphere, as a grid the game makes has none until it weighs seven tiles");
            Undo(session, document);
            Undo(session, document);
            tally.Expect(!session.Scene(document).History.CanUndo, "all of it undone, nothing is left to undo");
        }
        finally
        {
            session.Close(document);
        }
    }

    /// <summary>The grids of the kernel's air answer: each one's uid, whether it has an atmosphere, and the flags of its tiles.</summary>
    private static Dictionary<int, (bool Atmosphere, Dictionary<(int, int), int> Cells)> Grids(EngineSession session, int document)
    {
        using var answer = Ask(session, "air", $"{{\"doc\":{document}}}");
        var grids = new Dictionary<int, (bool, Dictionary<(int, int), int>)>();
        foreach (var grid in answer.RootElement.GetProperty("grids").EnumerateArray())
        {
            var numbers = grid.GetProperty("cells").EnumerateArray().Select(number => number.GetInt32()).ToArray();
            var cells = new Dictionary<(int, int), int>();
            for (var at = 0; at + 2 < numbers.Length; at += 3)
                cells[(numbers[at], numbers[at + 1])] = numbers[at + 2];
            grids[grid.GetProperty("grid").GetInt32()] = (grid.GetProperty("atmosphere").GetBoolean(), cells);
        }

        return grids;
    }

    private static void Check(EngineSession session, string forkRepo, string map, string outFolder, Action<string> say, Tally tally)
    {
        var name = Path.GetFileNameWithoutExtension(map);
        string Out(string step) => Path.Combine(outFolder, $"{name}.{step}.yml");

        var scene = Opened(session, CheckTimes.Open(map), File.ReadAllText(Path.Combine(forkRepo, "Resources", map)), map);
        var document = scene.Document.Id;
        try
        {
            var first = Save(session, document, Out("before"));
            var grids = Grids(session, document);
            var fixable = grids.Where(grid => grid.Value.Atmosphere).Select(grid => grid.Key).Order().ToArray();
            if (fixable.Length == 0)
            {
                CheckSkips.Skip(say, $"the fix of {name}: no grid of it has an atmosphere");
                return;
            }

            // ---- the map as it is, fixed, to tell what the fix changes on it ----
            var ops = fixable.Select(grid => new EditOp("air.fix", Grid: grid)).ToArray();
            var plain = Edit(session, document, "Fix grid atmos", ops);
            var plainText = string.Join("\n", Save(session, document, Out("fixed-plain")));
            say($"  what a fix changes on {name}, as it is:");
            Report(scene, fixable, string.Join("\n", first), plainText, say);
            if (plain.History.Revision != scene.History.Revision)
                Undo(session, document);

            // ---- one marker of each kind on floors of the largest grid ----
            // Floors with nothing airtight on them and no marker yet. Cables and pipes may lie there.
            var target = fixable.MaxBy(grid => grids[grid].Cells.Count);
            var taken = scene.Entities.Where(entity => entity.Parent == target && Markers.Any(marker => marker.Prototype == entity.Prototype))
                .Select(entity => ((int) MathF.Floor(entity.X), (int) MathF.Floor(entity.Y))).ToHashSet();
            var floors = grids[target].Cells.Where(cell => cell.Value == 0 && !taken.Contains(cell.Key)).Select(cell => cell.Key).Order().ToArray();
            var steps = 0;
            if (floors.Length < Markers.Length)
            {
                CheckSkips.Skip(say, $"markers on {name}: fewer than {Markers.Length} floors with nothing airtight on them");
            }
            else
            {
                // Spread over the grid, one in every so many of its free floors.
                var stride = floors.Length / Markers.Length;
                var spawns = Markers.Select((marker, at) => new EditOp("spawn", Prototype: marker.Prototype, Parent: target, X: floors[at * stride].Item1 + 0.5f, Y: floors[at * stride].Item2 + 0.5f)).ToArray();
                Edit(session, document, "Markers", spawns);
                steps++;
            }

            var marked = Save(session, document, Out("marked"));
            var markedText = string.Join("\n", marked);
            scene = session.Scene(document);

            // ---- what the fix says it will change ----
            using var preview = Ask(session, "air.fix.preview", $"{{\"doc\":{document}}}");
            var told = preview.RootElement.EnumerateArray().ToDictionary(grid => grid.GetProperty("grid").GetInt32(), grid => (Tiles: grid.GetProperty("tiles").GetInt32(), Outside: grid.GetProperty("outside").GetInt32(), Refused: grid.GetProperty("refused").GetString()));
            tally.Expect(fixable.All(grid => told.TryGetValue(grid, out var said) && said.Refused == null) && grids.Keys.Where(grid => !fixable.Contains(grid)).All(grid => told.TryGetValue(grid, out var said) && said.Refused != null),
                $"the preview names each of its {grids.Count} grids, and refuses only those with no atmosphere");

            // ---- the fix ----
            var asked = System.Diagnostics.Stopwatch.StartNew();
            var fixedDelta = Edit(session, document, "Fix grid atmos", ops);
            var fixMs = asked.ElapsedMilliseconds;
            // A grid whose air is the same but held in another order changes too: the save numbers its mixes otherwise.
            var changedGrids = fixable.Where(grid => told[grid].Tiles + told[grid].Outside > 0).ToArray();
            tally.Expect(changedGrids.All(fixedDelta.Air.Contains) && fixedDelta.Air.All(fixable.Contains) && fixedDelta.History.UndoLabel == "Fix grid atmos" && fixedDelta.Tiles.Count == 0 && fixedDelta.Entities.Count == 0,
                $"the fix of {fixable.Length} grids is one undo step, done in {fixMs} ms, and its delta names the {changedGrids.Length} grids whose air changed, and any held in another order ({string.Join(", ", fixedDelta.Air)})");
            var fixedLines = Save(session, document, Out("fixed"));
            var fixedText = string.Join("\n", fixedLines);

            foreach (var grid in fixable)
                CheckGrid(session, document, scene, grid, grids[grid].Cells, markedText, fixedText, told[grid], say, tally);

            // ---- fixed twice, undone and redone ----
            var again = Edit(session, document, "Fix grid atmos", ops);
            tally.Expect(again.History.Revision == fixedDelta.History.Revision && again.Air.Count == 0, "fixed again, nothing changes and no step is added");
            tally.Same(fixedLines, Save(session, document, Out("fixed-twice")), "the save fixed twice is the save fixed once");

            Undo(session, document);
            Same(marked, Save(session, document, Out("fixed-undone")), "undone, the save is the file before the fix", tally);
            Redo(session, document);
            Same(fixedLines, Save(session, document, Out("fixed-redone")), "redone, the save is the fixed file", tally);
            Undo(session, document);
            for (; steps > 0; steps--)
                Undo(session, document);
            Same(first, Save(session, document, Out("after")), "with the markers undone too, the save is the first file", tally);
        }
        finally
        {
            session.Close(document);
        }
    }

    /// <summary>
    /// Two saves that must be the same file to the byte, but for the line that says when the
    /// file was saved.
    /// </summary>
    private static void Same(string[] expected, string[] actual, string what, Tally tally)
    {
        var differences = Differences(expected, actual);
        var bytes = string.Join("\n", expected.Where(line => !line.TrimStart().StartsWith("time:", StringComparison.Ordinal))) == string.Join("\n", actual.Where(line => !line.TrimStart().StartsWith("time:", StringComparison.Ordinal)));
        tally.Expect(differences.Count == 0 && bytes, differences.Count == 0 && bytes ? $"{what}, to the byte ({expected.Length} lines)" : $"{what}: {differences.Count} differences, {differences.FirstOrDefault()}");
    }

    /// <summary>
    /// One grid after the fix, against the save: each tile's air by what stands on it, the
    /// cells off its edge, the markers, the kernel's air answer, and the preview's counts.
    /// </summary>
    private static void CheckGrid(EngineSession session, int document, Scene scene, int grid, Dictionary<(int, int), int> cells, string markedText, string fixedText,
        (int Tiles, int Outside, string? Refused) told, Action<string> say, Tally tally)
    {
        var sceneGrid = scene.Grids.First(found => found.Uid == grid);
        var label = $"{sceneGrid.Name} ({grid})";
        var file = GridCheck.FileAir(fixedText, sceneGrid.YamlUid);
        if (!tally.Expect(file != null, $"the save stores the air of {label}"))
            return;

        var (mixes, tiles) = file!.Value;
        // The markers anchored on the grid, by cell. Of two on one cell the check expects neither.
        var modes = Markers.ToDictionary(marker => marker.Prototype, marker => marker.Mode);
        var markers = scene.Entities.Where(entity => entity.Parent == grid && entity.Anchored && modes.ContainsKey(entity.Prototype))
            .GroupBy(entity => ((int) MathF.Floor(entity.X), (int) MathF.Floor(entity.Y)))
            .ToDictionary(group => group.Key, group => group.Select(entity => modes[entity.Prototype]).Distinct().Count() == 1 ? modes[group.First().Prototype] : -1);

        var wrong = new List<string>();
        var (floors, walls, open, marked) = (0, 0, 0, 0);
        foreach (var (cell, flags) in cells)
        {
            tiles.TryGetValue(cell, out var index);
            var stored = tiles.ContainsKey(cell) ? mixes[index] : null;
            if ((flags & Open) != 0)
            {
                open++;
                if (stored is not { Immutable: true })
                    wrong.Add($"space or lattice at {cell} stores {Describe(stored)}, not the map's air");
            }
            else if ((flags & Sides) == Sides && (flags & NoAir) != 0)
            {
                walls++;
                if (stored != null)
                    wrong.Add($"the wall at {cell} stores {Describe(stored)}");
            }
            else
            {
                var mode = markers.GetValueOrDefault(cell, 0);
                if (mode < 0)
                    continue;

                if (mode > 0)
                    marked++;
                else
                    floors++;
                if (!Holds(stored, Expected(mode)))
                    wrong.Add($"the {(mode > 0 ? $"marker of mode {mode}" : "floor")} at {cell} stores {Describe(stored)}");
            }
        }

        // No cell off the grid stores air, as on a paused map the game's fix leaves none there.
        var beside = cells.Keys.SelectMany(cell => new[] { (cell.Item1, cell.Item2 + 1), (cell.Item1, cell.Item2 - 1), (cell.Item1 + 1, cell.Item2), (cell.Item1 - 1, cell.Item2) })
            .Where(cell => !cells.ContainsKey(cell)).ToHashSet();
        var offGrid = tiles.Keys.Count(cell => !cells.ContainsKey(cell));
        var immutable = mixes.Count(mix => mix.Immutable);
        tally.Expect(wrong.Count == 0,
            wrong.Count == 0
                ? $"{label}: its {floors} floors store standard air at 293.15 K, its {walls} walls none, its {open} tiles of space and lattice the map's air, and its {marked} marked floors their markers' mixes"
                : $"{label}: {wrong.Count} tiles are wrong, as {string.Join("; ", wrong.Take(4))}");
        tally.Expect(offGrid == 0 && immutable <= 1,
            $"{label}: none of the {beside.Count} cells beside it stores air, as a paused map's fix leaves them, nor any other cell off it ({offGrid} do), and the map's air is one immutable mix ({immutable})");

        // The overlay's answer is the save's air.
        using var answer = Ask(session, "air", $"{{\"doc\":{document},\"grids\":[{grid}]}}");
        var gases = answer.RootElement.GetProperty("gases").EnumerateArray().Select(gas => gas.GetString() ?? "").ToArray();
        var read = answer.RootElement.GetProperty("grids")[0];
        var readMixes = read.GetProperty("mixes").EnumerateArray().Select(mix => new FileMix(
            mix.GetProperty("temperature").GetSingle(),
            mix.GetProperty("moles").EnumerateArray().Select((moles, gas) => (gas, moles.GetSingle())).Where(pair => pair.Item2 != 0).ToDictionary(pair => gases[pair.gas], pair => pair.Item2),
            mix.GetProperty("volume").GetSingle(),
            mix.GetProperty("immutable").GetBoolean())).ToArray();
        var numbers = read.GetProperty("tiles").EnumerateArray().Select(number => number.GetInt32()).ToArray();
        var readTiles = new Dictionary<(int, int), int>();
        for (var at = 0; at + 2 < numbers.Length; at += 3)
            readTiles[(numbers[at], numbers[at + 1])] = numbers[at + 2];
        var disagree = tiles.Count(tile => !readTiles.TryGetValue(tile.Key, out var index) || !SameMix(readMixes[index], mixes[tile.Value]));
        var unfilled = read.GetProperty("unfilled").GetArrayLength() / 2;
        var blocked = read.GetProperty("blocked").GetArrayLength() / 2;
        tally.Expect(readTiles.Count == tiles.Count && disagree == 0 && unfilled == 0 && blocked == walls,
            $"{label}: the overlay's air answer is the save's, {readTiles.Count} cells and {readMixes.Length} mixes, with no floor left with no air ({unfilled}) and {blocked} walls");

        // What the preview said would change is what did.
        var before = GridCheck.FileAir(markedText, sceneGrid.YamlUid);
        var (changedTiles, changedOutside) = (0, 0);
        if (before is { } beforeAir)
        {
            var (beforeMixes, beforeTiles) = beforeAir;
            foreach (var cell in beforeTiles.Keys.Union(tiles.Keys))
            {
                var was = beforeTiles.TryGetValue(cell, out var a) ? beforeMixes[a] : null;
                var now = tiles.TryGetValue(cell, out var b) ? mixes[b] : null;
                if (was == null && now == null || was != null && now != null && SameMix(was, now))
                    continue;
                if (cells.ContainsKey(cell))
                    changedTiles++;
                else
                    changedOutside++;
            }
        }
        else
        {
            (changedTiles, changedOutside) = (tiles.Keys.Count(cells.ContainsKey), tiles.Keys.Count(cell => !cells.ContainsKey(cell)));
        }

        tally.Expect(told.Tiles == changedTiles && told.Outside == changedOutside,
            $"{label}: the preview said {told.Tiles} tiles and {told.Outside} cells off it would change, and {changedTiles} and {changedOutside} did");
    }

    /// <summary>Whether a stored mix is a preset's: its moles and temperature, 2500 litres, mutable.</summary>
    private static bool Holds(FileMix? stored, (Dictionary<string, float> Moles, float Temperature) expected)
    {
        if (stored == null || stored.Immutable || !Near(stored.Volume, Atmospherics.CellVolume) || !Near(stored.Temperature, expected.Temperature))
            return false;

        return stored.Moles.Keys.Union(expected.Moles.Keys).All(gas => Near(stored.Moles.GetValueOrDefault(gas), expected.Moles.GetValueOrDefault(gas)));
    }

    private static bool Near(float a, float b) => MathF.Abs(a - b) <= 1e-5f * MathF.Max(1, MathF.Abs(b));

    /// <summary>Whether two mixes are the same to the bit, as the save tells them apart.</summary>
    private static bool SameMix(FileMix a, FileMix b) =>
        a.Temperature == b.Temperature && a.Volume == b.Volume && a.Immutable == b.Immutable
        && a.Moles.Keys.Union(b.Moles.Keys).All(gas => a.Moles.GetValueOrDefault(gas) == b.Moles.GetValueOrDefault(gas));

    private static float Pressure(FileMix mix) => mix.Volume <= 0 ? 0 : mix.Moles.Values.Sum() * Atmospherics.R * mix.Temperature / mix.Volume;

    /// <summary>A mix in a few words: the map's or not, its pressure and temperature, and its gases by share.</summary>
    private static string Describe(FileMix? mix)
    {
        if (mix == null)
            return "no air";

        var total = mix.Moles.Values.Sum();
        var gases = total <= 0 ? "no gas" : string.Join(" ", mix.Moles.Where(gas => gas.Value > 0).OrderByDescending(gas => gas.Value).Select(gas => $"{gas.Key} {gas.Value / total * 100:0.#}%"));
        return $"{(mix.Immutable ? "the map's air, " : "")}{Pressure(mix):0.#} kPa at {mix.Temperature:0.###} K, {gases}";
    }

    /// <summary>
    /// Whether two mixes differ by less than the game acts on, as the editor's own air checks
    /// tell it (native/editor-core/src/air_checks.rs): both vacuum, or the same gases within
    /// 0.4% each, 4 K and a tenth of an atmosphere.
    /// </summary>
    private static bool Drift(FileMix a, FileMix b)
    {
        var (pa, pb) = (Pressure(a), Pressure(b));
        if (pa <= 20 && pb <= 20)
            return true;
        if (pa <= 20 || pb <= 20)
            return false;

        var (ta, tb) = (a.Moles.Values.Sum(), b.Moles.Values.Sum());
        var shares = a.Moles.Keys.Union(b.Moles.Keys).All(gas => MathF.Abs(a.Moles.GetValueOrDefault(gas) / ta - b.Moles.GetValueOrDefault(gas) / tb) <= 0.004f);
        return shares && MathF.Abs(a.Temperature - b.Temperature) <= 4 && MathF.Abs(pa - pb) <= 0.1f * Atmospherics.OneAtmosphere;
    }

    /// <summary>What the fix changed on each grid, by kind, and the air it replaced that was more than drift.</summary>
    private static void Report(Scene scene, IReadOnlyList<int> grids, string markedText, string fixedText, Action<string> say)
    {
        foreach (var grid in grids)
        {
            var sceneGrid = scene.Grids.First(found => found.Uid == grid);
            if (GridCheck.FileAir(markedText, sceneGrid.YamlUid) is not { } beforeAir || GridCheck.FileAir(fixedText, sceneGrid.YamlUid) is not { } afterAir)
            {
                say($"        {sceneGrid.Name}: the file before stored no air in the form of version 2");
                continue;
            }

            var (beforeMixes, beforeTiles) = beforeAir;
            var (afterMixes, afterTiles) = afterAir;
            var onGrid = new HashSet<(int, int)>();
            for (var at = 0; at + 4 < sceneGrid.Tiles.Count; at += 5)
                onGrid.Add((sceneGrid.Tiles[at], sceneGrid.Tiles[at + 1]));
            var (same, filled, cleared, drift, outsideAdded, outsideGone, outsideChanged) = (0, 0, 0, 0, 0, 0, 0);
            var replaced = new Dictionary<string, int>();
            foreach (var cell in beforeTiles.Keys.Union(afterTiles.Keys))
            {
                var was = beforeTiles.TryGetValue(cell, out var a) ? beforeMixes[a] : null;
                var now = afterTiles.TryGetValue(cell, out var b) ? afterMixes[b] : null;
                if (!onGrid.Contains(cell))
                {
                    if (was == null) outsideAdded++;
                    else if (now == null) outsideGone++;
                    else if (!SameMix(was, now)) outsideChanged++;
                    continue;
                }

                if (was == null && now == null || was != null && now != null && SameMix(was, now))
                    same++;
                else if (was == null)
                    filled++;
                else if (now == null)
                    cleared++;
                else if (Drift(was, now))
                    drift++;
                else
                {
                    var key = $"{Describe(was)} -> {Describe(now)}";
                    replaced[key] = replaced.GetValueOrDefault(key) + 1;
                }
            }

            say($"        {sceneGrid.Name}: {onGrid.Count} tiles. {same} kept their air to the bit, {drift} lost drift of the simulation, {filled} floors with no air were filled, {cleared} lost air they should not store, and {replaced.Values.Sum()} had other air replaced");
            say($"        off its edge: {outsideAdded} cells gained the map's air, {outsideGone} lost air, {outsideChanged} changed");
            foreach (var (what, count) in replaced.OrderByDescending(pair => pair.Value).Take(10))
                say($"          {count,6} x {what}");
        }
    }

    // ---- the fork's maps against a fix ----

    /// <summary>
    /// Opens each map, fixes every grid that has an atmosphere and compares the air the save
    /// stores with the air the file stored: the same text, the same air in another order, or
    /// other air. A map the game had just fixed and saved would come out the same text, which
    /// would show the fix and its order are the game's. Changes no file.
    /// </summary>
    public static int Survey(EngineSession session, string forkRepo, IReadOnlyList<string> maps, Action<string> say)
    {
        var (identical, reordered, drifted, other, none) = (0, 0, 0, 0, 0);
        foreach (var map in maps)
        {
            Scene scene;
            try
            {
                scene = session.Open(File.ReadAllText(Path.Combine(forkRepo, "Resources", map)), map).Scene;
            }
            catch (Exception e)
            {
                say($"  {map}: not opened, {e.Message.Split('\n')[0]}");
                continue;
            }

            var document = scene.Document.Id;
            try
            {
                var original = string.Join("\n", Save(session, document, Path.Combine(Path.GetTempPath(), "editor-air-survey.yml")));
                var grids = Grids(session, document).Where(grid => grid.Value.Atmosphere).Select(grid => grid.Key).Order().ToArray();
                if (grids.Length == 0)
                {
                    none++;
                    continue;
                }

                session.Edit(document, "Fix grid atmos", grids.Select(grid => new EditOp("air.fix", Grid: grid)).ToArray());
                var fixedText = string.Join("\n", Save(session, document, Path.Combine(Path.GetTempPath(), "editor-air-survey.yml")));
                var verdicts = new List<string>();
                foreach (var grid in grids)
                {
                    var sceneGrid = scene.Grids.First(found => found.Uid == grid);
                    var yamlUid = sceneGrid.YamlUid;
                    // How many cells off the grid the file stored, which a paused map's fix leaves out.
                    var onGrid = Enumerable.Range(0, sceneGrid.Tiles.Count / 5).Select(at => (sceneGrid.Tiles[at * 5], sceneGrid.Tiles[at * 5 + 1])).ToHashSet();
                    var stored = GridCheck.FileAir(original, yamlUid) is { } held ? held.Tiles.Keys.Count(cell => !onGrid.Contains(cell)) : 0;
                    var (a, b) = (AtmosphereBlock(original, yamlUid), AtmosphereBlock(fixedText, yamlUid));
                    if (a == b)
                    {
                        verdicts.Add($"same ({stored} off the grid)");
                        continue;
                    }

                    if (GridCheck.FileAir(original, yamlUid) is not { } was || GridCheck.FileAir(fixedText, yamlUid) is not { } now)
                    {
                        verdicts.Add("stored no air before");
                        continue;
                    }

                    var ((m1, t1), (m2, t2)) = (was, now);
                    var (changed, offGrid, driftOnly) = (0, 0, 0);
                    foreach (var cell in t1.Keys.Union(t2.Keys))
                    {
                        var filed = t1.TryGetValue(cell, out var i) ? m1[i] : null;
                        var made = t2.TryGetValue(cell, out var j) ? m2[j] : null;
                        if (filed != null && made != null && SameMix(filed, made))
                            continue;
                        changed++;
                        if (!onGrid.Contains(cell))
                            offGrid++;
                        else if (filed != null && made != null && Drift(filed, made))
                            driftOnly++;
                    }

                    verdicts.Add(changed == 0
                        ? $"same air, other order ({stored} off the grid)"
                        : changed == driftOnly ? $"drift on {changed} cells" : $"{changed} cells differ, {driftOnly} by drift, {offGrid} off the grid ({stored} stored off it before)");
                }

                if (verdicts.All(verdict => verdict.StartsWith("same (", StringComparison.Ordinal)))
                    identical++;
                else if (verdicts.All(verdict => verdict.StartsWith("same", StringComparison.Ordinal)))
                    reordered++;
                else if (verdicts.All(verdict => verdict.StartsWith("same", StringComparison.Ordinal) || verdict.StartsWith("drift", StringComparison.Ordinal)))
                    drifted++;
                else
                    other++;
                say($"  {map}: {string.Join(", ", verdicts)}");
            }
            catch (Exception e)
            {
                say($"  {map}: stopped, {e.Message.Split('\n')[0]}");
            }
            finally
            {
                session.Close(document);
            }
        }

        say($"\n{identical} maps saved the same text after the fix, {reordered} the same air in another order, {drifted} differed only by drift of the simulation, {other} by other air, and {none} had no grid with an atmosphere.");
        return 0;
    }

    /// <summary>The lines of a save that hold one grid's GridAtmosphere, or "" if it has none.</summary>
    private static string AtmosphereBlock(string yaml, int yamlUid)
    {
        var lines = yaml.Split('\n');
        var start = Array.FindIndex(lines, line => line.TrimEnd() == $"  - uid: {yamlUid}");
        if (start < 0)
            return "";

        var end = Array.FindIndex(lines, start + 1, line => line.StartsWith("  - uid: ", StringComparison.Ordinal) || line.StartsWith("- proto: ", StringComparison.Ordinal));
        end = end < 0 ? lines.Length : end;
        var at = Array.FindIndex(lines, start, end - start, line => line.Trim() == "- type: GridAtmosphere");
        if (at < 0)
            return "";

        var indent = lines[at].Length - lines[at].TrimStart().Length;
        var stop = Array.FindIndex(lines, at + 1, end - at - 1, line => line.Length - line.TrimStart().Length <= indent && line.TrimStart().StartsWith("- ", StringComparison.Ordinal));
        return string.Join("\n", lines[at..(stop < 0 ? end : stop)]);
    }
}
