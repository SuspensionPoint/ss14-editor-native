using System.Globalization;
using System.Text.Json;
using SS14.EditorKernel;

namespace SS14.EditorNative;

/// <summary>
/// Checks that a region of tiles, with everything standing on it, can be moved, deleted and turned
/// as one edit and one undo step, and that the saved file comes out right.
///
/// The kernel has no region op. A region edit is a list of tile, move, rotate and decal ops in one
/// edit, and their order decides whether anchored things stay anchored. The ops are built here the
/// way the editor builds them from its mirror, from the scene the kernel reports and nothing else.
/// The order of a move is:
///   1. tile ops for the cells the region goes to, with variant and rotation byte
///   2. move ops for every entity standing on the region
///   3. decal.update ops for every decal on the region, with every field sent again
///   4. tile ops that empty the cells the region left and didn't also move into
///   5. air ops that carry each cell's stored air to the cell its tile went to
/// Each edit is saved and compared with the first save, line by line. Lines are matched by where
/// they are in the file (tile chunks, an entity's position, decals), and every line that differs
/// when it shouldn't is printed.
///
/// Lines starting with "found" note where the engine behaves differently than one might expect.
/// They don't fail the check. Lines starting with "FAIL" do.
/// </summary>
public static partial class RegionCheck
{
    private const int Width = 12;
    private const int Height = 8;
    private const double Quarter = Math.PI / 2;

    /// <returns>0 if every map passed.</returns>
    public static int Run(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        var session = EngineSession.Booted(forkRepo, kernelPack);
        var code = Run(session, forkRepo, maps, outFolder, say);
        CheckTimes.Report("regions", say);
        CheckTimes.Summary(say);
        return code;
    }

    /// <summary>The check on an engine that is up already.</summary>
    public static int Run(EngineSession session, string forkRepo, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        Directory.CreateDirectory(outFolder);

        // The kernel says of each tile whether it can be turned (TileInfo.AllowsRotation).
        using var tileKinds = JsonDocument.Parse(session.Command("tiles", "{}"));
        var turning = tileKinds.RootElement.EnumerateArray()
            .Where(tile => tile.TryGetProperty("allowsRotation", out var allows) && allows.ValueKind == JsonValueKind.True)
            .Select(tile => tile.GetProperty("id").GetString() ?? "")
            .ToHashSet();
        say($"tiles the game draws by their rotation byte ({turning.Count}): {string.Join(", ", turning.Order(StringComparer.Ordinal))}");
        // A fork may have no such tile at all, as Space Wizards' master has none. Its prototypes say.
        var untold = turning.Count == 0 && TurningInFiles(forkRepo);
        if (untold)
            say("  FAIL  the kernel names no tile that can be turned, and the fork's tile prototypes have some");
        else if (turning.Count == 0)
            CheckSkips.Skip(say, "tiles drawn by their rotation byte: the fork's tile prototypes have none");

        var miscounted = QuarterCases.Where(pair => ByQuarters(pair.Angle) != pair.On).ToList();
        say(miscounted.Count == 0
            ? $"  ok    an angle a quarter turn on is the very number expected, in all {QuarterCases.Length} cases"
            : $"  FAIL  an angle a quarter turn on is not the number expected in {miscounted.Count} of {QuarterCases.Length} cases, the first {miscounted[0].Angle}: {ByQuarters(miscounted[0].Angle)}, not {miscounted[0].On}");
        untold |= miscounted.Count > 0;

        using var listed = JsonDocument.Parse(session.Command("decals", "{}"));
        var decalKinds = listed.RootElement.EnumerateArray().Select(decal => decal.GetProperty("id").GetString() ?? "").ToHashSet();

        var skipsBefore = CheckSkips.Count;
        var failed = 0;
        foreach (var map in maps)
        {
            say(map);
            var check = new Check(session, forkRepo, map, outFolder, turning, decalKinds, say);
            try
            {
                check.Run();
            }
            catch (Exception e)
            {
                check.Fail(e.Message);
            }
            finally
            {
                check.Close();
            }

            if (check.Failures > 0)
                failed++;
        }

        say(failed == 0 && !untold ? "\nA region moved, went and turned as one step, and the files were right." : $"\n{failed} of {maps.Count} maps failed.");
        CheckSkips.Report(say, skipsBefore);
        return failed == 0 && !untold ? 0 : 1;
    }

    /// <summary>Whether any of the fork's tile prototypes may be turned (allowRotationMirror), read from its files.</summary>
    private static bool TurningInFiles(string forkRepo) =>
        Directory.EnumerateFiles(Path.Combine(forkRepo, "Resources", "Prototypes"), "*.yml", SearchOption.AllDirectories)
            .Any(file => File.ReadAllText(file).Contains("allowRotationMirror: true", StringComparison.OrdinalIgnoreCase));

    // ---- the saved file, line by line ----

    /// <summary>
    /// A save as lines, each with the file uid of the entity it belongs to (0 for none) and the
    /// part of the file it is in: "head", "group" (a prototype's heading), "entity", or the
    /// name of the component.
    /// </summary>
    private sealed record Saved(string[] Lines, int[] Uid, string[] Part)
    {
        public static Saved Of(string[] lines)
        {
            lines = (string[]) lines.Clone();
            var uids = new int[lines.Length];
            var parts = new string[lines.Length];
            var (uid, part) = (0, "head");
            for (var i = 0; i < lines.Length; i++)
            {
                var line = lines[i];
                // The file's heading says when it was saved.
                if (i < 12 && line.TrimStart().StartsWith("time:", StringComparison.Ordinal))
                    lines[i] = "  time:";
                else if (line.StartsWith("- proto:", StringComparison.Ordinal) || line == "  entities:")
                    (uid, part) = (0, "group");
                // The end of the file belongs to no entity, though the last one is above it.
                else if (line == "...")
                    (uid, part) = (0, "head");
                else if (line.StartsWith("  - uid: ", StringComparison.Ordinal))
                    (uid, part) = (int.Parse(line.AsSpan(9), CultureInfo.InvariantCulture), "entity");
                else if (uid != 0 && line.StartsWith("    - type: ", StringComparison.Ordinal))
                    part = line[12..].Trim();

                uids[i] = uid;
                parts[i] = part;
            }

            return new Saved(lines, uids, parts);
        }

        /// <summary>What kind of line this is, for counting the lines that differ.</summary>
        public string Kind(int line)
        {
            var text = Lines[line].TrimStart();
            switch (Part[line])
            {
                case "head":
                    return text.StartsWith("entityCount:", StringComparison.Ordinal) ? "entity count" : "other";
                case "group":
                    return "group heading";
                case "MapGrid":
                    return "tile chunk";
                case "GridAtmosphere":
                    return "air";
                case "DecalGrid" or "DecalChunk":
                    return text.Length > 0 && char.IsAsciiDigit(text[0]) ? "decal coordinate" : "decal group";
                // A fork's chunk entities of decals, and the lists of them a grid or map keeps.
                case "ChunkEntity" or "ChunkContainer":
                    return "decal chunk";
                case "Transform":
                    return text.StartsWith("pos:", StringComparison.Ordinal) ? "pos"
                        : text.StartsWith("rot:", StringComparison.Ordinal) ? "rot"
                        : text.StartsWith("parent:", StringComparison.Ordinal) ? "parent"
                        : "other";
                // The components in which one entity lists others (ComponentEditor.Listing).
                case "DeviceLinkSource" or "DeviceList" or "DeviceNetwork" or "LinkedDeviceNetwork":
                    return "list of entities";
                default:
                    return "other";
            }
        }

        /// <summary>An entity's transform as the file has it.</summary>
        public string Transform(int uid)
        {
            var lines = new List<string>();
            for (var i = 0; i < Lines.Length; i++)
            {
                if (Uid[i] == uid && Part[i] == "Transform" && !Lines[i].StartsWith("    - type:", StringComparison.Ordinal))
                    lines.Add(Lines[i].Trim());
            }

            return lines.Count == 0 ? "(not in the file)" : string.Join(", ", lines);
        }

        /// <summary>The same save with these entities cut out, and the headings left with none.</summary>
        public Saved Without(HashSet<int> gone)
        {
            var kept = new List<string>();
            for (var i = 0; i < Lines.Length; i++)
            {
                if (Uid[i] == 0 || !gone.Contains(Uid[i]))
                    kept.Add(Lines[i]);
            }

            var result = new List<string>();
            for (var i = 0; i < kept.Count; i++)
            {
                if (kept[i].StartsWith("- proto:", StringComparison.Ordinal) && i + 1 < kept.Count && kept[i + 1] == "  entities:"
                    && (i + 2 >= kept.Count || !kept[i + 2].StartsWith("  - uid: ", StringComparison.Ordinal)))
                {
                    i++;
                    continue;
                }

                result.Add(kept[i]);
            }

            return Of(result.ToArray());
        }
    }

    /// <summary>The lines two saves differ in: those only the first has and those only the second has.</summary>
    private sealed class Differences
    {
        public readonly List<(string Kind, int Uid, string Text, bool Added)> Lines = new();

        public bool None => Lines.Count == 0;

        public int Removed(string kind) => Lines.Count(line => !line.Added && line.Kind == kind);

        public int Added(string kind) => Lines.Count(line => line.Added && line.Kind == kind);

        public bool Only(params string[] kinds) => Lines.All(line => kinds.Contains(line.Kind));

        /// <summary>How many lines of a kind differ: one number if as many went as came, else both.</summary>
        public string Count(string kind)
        {
            var (removed, added) = (Removed(kind), Added(kind));
            return removed == added ? $"{removed}" : $"{removed} out and {added} in";
        }

        /// <summary>
        /// Whether the saves have the same lines and only tile chunks are in another place. A grid
        /// writes its chunks in the order it holds them, which is not the order of their places.
        /// </summary>
        public bool OnlyChunkOrder => !None && Only("tile chunk")
            && Lines.Where(line => !line.Added).Select(line => line.Text).Order(StringComparer.Ordinal)
                .SequenceEqual(Lines.Where(line => line.Added).Select(line => line.Text).Order(StringComparer.Ordinal));

        /// <summary>The same differences but for the lines of one kind.</summary>
        public Differences Except(string kind)
        {
            var rest = new Differences();
            rest.Lines.AddRange(Lines.Where(line => line.Kind != kind));
            return rest;
        }

        public string Describe() => None
            ? "no lines"
            : string.Join(", ", Lines.Select(line => line.Kind).Distinct().Order(StringComparer.Ordinal).Select(kind => $"{kind} lines {Count(kind)}"));

        public IEnumerable<string> Examples(Func<string, bool> kind, int take) => Lines
            .Where(line => kind(line.Kind))
            .Take(take)
            .Select(line => $"        {(line.Added ? "+" : "-")} {line.Kind}{(line.Uid == 0 ? "" : $" of uid {line.Uid}")}: {(line.Text.Length > 110 ? line.Text[..110] + "..." : line.Text)}");
    }

    private static bool Same(Saved a, Saved b)
    {
        using var timed = CheckTimes.In(CheckTimes.Compare);
        return a.Lines.AsSpan().SequenceEqual(b.Lines);
    }

    private static Differences Compare(Saved a, Saved b)
    {
        using var timed = CheckTimes.In(CheckTimes.Compare);
        var ids = new Dictionary<string, int>(StringComparer.Ordinal);
        int[] Numbered(string[] lines)
        {
            var numbers = new int[lines.Length];
            for (var i = 0; i < lines.Length; i++)
            {
                if (!ids.TryGetValue(lines[i], out var id))
                    ids[lines[i]] = id = ids.Count;
                numbers[i] = id;
            }

            return numbers;
        }

        var (x, y) = (Numbered(a.Lines), Numbered(b.Lines));
        var start = 0;
        while (start < x.Length && start < y.Length && x[start] == y[start])
            start++;
        var (endA, endB) = (x.Length, y.Length);
        while (endA > start && endB > start && x[endA - 1] == y[endB - 1])
        {
            endA--;
            endB--;
        }

        var (removed, added) = ShortestEdit(x, start, endA - start, y, start, endB - start);
        var differences = new Differences();
        foreach (var line in removed)
            differences.Lines.Add((a.Kind(line), a.Uid[line], a.Lines[line].Trim(), false));
        foreach (var line in added)
            differences.Lines.Add((b.Kind(line), b.Uid[line], b.Lines[line].Trim(), true));
        return differences;
    }

    /// <summary>
    /// The fewest lines to take out of one text and put into it to get the other (Myers'
    /// algorithm). A cruder comparison would be fooled by the many lines every entity has alike.
    /// </summary>
    private static (List<int> Removed, List<int> Added) ShortestEdit(int[] a, int a0, int n, int[] b, int b0, int m)
    {
        var removed = new List<int>();
        var added = new List<int>();
        if (n == 0 || m == 0)
        {
            for (var i = 0; i < n; i++)
                removed.Add(a0 + i);
            for (var j = 0; j < m; j++)
                added.Add(b0 + j);
            return (removed, added);
        }

        var max = n + m;
        var v = new int[2 * max + 2];
        var rounds = new List<int[]>();
        var found = -1;
        for (var d = 0; d <= max && found < 0; d++)
        {
            if (d > 8000)
                throw new InvalidOperationException("the two saves differ in too many lines to compare");

            for (var k = -d; k <= d; k += 2)
            {
                var x = k == -d || (k != d && v[max + k - 1] < v[max + k + 1]) ? v[max + k + 1] : v[max + k - 1] + 1;
                var y = x - k;
                while (x < n && y < m && a[a0 + x] == b[b0 + y])
                {
                    x++;
                    y++;
                }

                v[max + k] = x;
                if (x >= n && y >= m)
                {
                    found = d;
                    break;
                }
            }

            var round = new int[2 * d + 1];
            Array.Copy(v, max - d, round, 0, round.Length);
            rounds.Add(round);
        }

        var (px, py) = (n, m);
        for (var d = found; d > 0; d--)
        {
            var round = rounds[d - 1];
            var k = px - py;
            var from = k == -d || (k != d && round[k - 1 + d - 1] < round[k + 1 + d - 1]) ? k + 1 : k - 1;
            var fromX = round[from + d - 1];
            var fromY = fromX - from;
            while (px > fromX && py > fromY)
            {
                px--;
                py--;
            }

            if (px == fromX)
                added.Add(b0 + fromY);
            else
                removed.Add(a0 + fromX);
            (px, py) = (fromX, fromY);
        }

        removed.Sort();
        added.Sort();
        return (removed, added);
    }

    // ---- a region and what stands on it ----

    private readonly record struct TileOn(string Id, int Variant, int Turn);

    /// <summary>What the scene has on a set of cells of one grid.</summary>
    private sealed record Standing(
        Dictionary<(int X, int Y), TileOn> Tiles,
        List<SceneEntity> Entities,
        List<SceneDecal> Decals);

    /// <summary>Ops for one edit, and what the engine should hold after it.</summary>
    private sealed record Plan(
        List<EditOp> Ops,
        Dictionary<(int X, int Y), TileOn> Tiles,
        List<(int X, int Y)> Emptied,
        List<(SceneEntity Was, float X, float Y, double Rotation)> Entities,
        List<(SceneDecal Was, float X, float Y, double Angle)> Decals,
        /// <summary>The decals a turn could not turn as one piece with the region (TurnPlan).</summary>
        IReadOnlyList<int> Fallback,
        /// <summary>Each cell of the region whose air goes, and the cell it goes to.</summary>
        List<((int X, int Y) From, (int X, int Y) To)> Air);

    private static (int X, int Y) Cell(float x, float y) => ((int) MathF.Floor(x), (int) MathF.Floor(y));

    private static Dictionary<(int X, int Y), TileOn> TilesOf(SceneGrid grid)
    {
        var tiles = new Dictionary<(int X, int Y), TileOn>();
        for (var i = 0; i + 4 < grid.Tiles.Count; i += 5)
            tiles[(grid.Tiles[i], grid.Tiles[i + 1])] = new TileOn(grid.TileIds[grid.Tiles[i + 2]], grid.Tiles[i + 3], grid.Tiles[i + 4]);
        return tiles;
    }

    /// <summary>
    /// As the editor will read its mirror: the tiles of the cells, the entities that are
    /// children of the grid, not inside anything, with their position in one of the cells, and
    /// the decals whose corner is in one of the cells, of a kind the game has or not.
    /// </summary>
    private static Standing On(Scene scene, int gridUid, IReadOnlySet<(int X, int Y)> cells)
    {
        var grid = scene.Grids.First(g => g.Uid == gridUid);
        return new Standing(
            TilesOf(grid).Where(pair => cells.Contains(pair.Key)).ToDictionary(pair => pair.Key, pair => pair.Value),
            scene.Entities.Where(entity => entity.Parent == gridUid && !entity.Contained && cells.Contains(Cell(entity.X, entity.Y))).ToList(),
            grid.Decals.Where(decal => cells.Contains(Cell(decal.X, decal.Y))).ToList());
    }

    private static EditOp TileOp(int grid, (int X, int Y) cell, TileOn? tile) => tile is { } t
        ? new EditOp("tile", Grid: grid, X: cell.X, Y: cell.Y, Tile: t.Id, Variant: t.Variant, RotationMirroring: t.Turn)
        : new EditOp("tile", Grid: grid, X: cell.X, Y: cell.Y);

    private static EditOp DecalOp(int grid, SceneDecal decal, float x, float y, double angle) => new(
        "decal.update", Uid: decal.Id, Grid: grid, X: x, Y: y, Prototype: decal.Prototype, Color: decal.Color,
        Rotation: angle, ZIndex: decal.ZIndex, Cleanable: decal.Cleanable);

    /// <summary>
    /// The air ops of a move or a turn, after all its other ops: each cell of the region that
    /// has a tile carries its air to the cell its tile goes to.
    /// </summary>
    private static List<((int X, int Y) From, (int X, int Y) To)> AirOps(int grid, IEnumerable<((int X, int Y) From, (int X, int Y) To)> cells, List<EditOp> ops)
    {
        var air = cells.ToList();
        foreach (var (from, to) in air)
            ops.Add(new EditOp("air", Grid: grid, FromX: from.X, FromY: from.Y, X: to.X, Y: to.Y));
        return air;
    }

    private static Plan MovePlan(int grid, Standing on, int dx, int dy)
    {
        var ops = new List<EditOp>();
        var tiles = on.Tiles.ToDictionary(pair => (pair.Key.X + dx, pair.Key.Y + dy), pair => pair.Value);
        foreach (var (cell, tile) in tiles)
            ops.Add(TileOp(grid, cell, tile));

        var entities = on.Entities.Select(entity => (entity, entity.X + dx, entity.Y + dy, entity.Rotation)).ToList();
        foreach (var (entity, x, y, _) in entities)
            ops.Add(new EditOp("move", Uid: entity.Uid, X: x, Y: y));

        // As the editor sends them (area.rs decal_move_order): the chunks the move goes towards
        // first, so they are left before the decals behind them come in, each in draw order.
        var decals = on.Decals
            .OrderBy(decal => -Math.Sign(dx) * ChunkOf(decal.X, decal.Y).X)
            .ThenBy(decal => -Math.Sign(dy) * ChunkOf(decal.X, decal.Y).Y)
            .ThenBy(decal => decal.Order)
            .ThenBy(decal => decal.Id)
            .Select(decal => (decal, decal.X + dx, decal.Y + dy, decal.Angle)).ToList();
        foreach (var (decal, x, y, angle) in decals)
            ops.Add(DecalOp(grid, decal, x, y, angle));

        var emptied = on.Tiles.Keys.Where(cell => !tiles.ContainsKey(cell)).ToList();
        foreach (var cell in emptied)
            ops.Add(TileOp(grid, cell, null));

        var air = AirOps(grid, on.Tiles.Keys.Select(cell => (cell, (cell.X + dx, cell.Y + dy))), ops);
        return new Plan(ops, tiles, emptied, entities, decals, [], air);
    }

    private static Plan DeletePlan(int grid, Standing on)
    {
        var ops = new List<EditOp>();
        foreach (var entity in on.Entities)
            ops.Add(new EditOp("delete", Uid: entity.Uid));
        foreach (var decal in on.Decals)
            ops.Add(new EditOp("decal.remove", Grid: grid, Uid: decal.Id));
        foreach (var cell in on.Tiles.Keys)
            ops.Add(TileOp(grid, cell, null));
        // Last the air of each emptied cell goes, with no destination.
        foreach (var cell in on.Tiles.Keys)
            ops.Add(new EditOp("air", Grid: grid, FromX: cell.X, FromY: cell.Y, Clear: true));
        return new Plan(ops, new(), on.Tiles.Keys.ToList(), new(), new(), [], []);
    }

    /// <summary>The cell a cell goes to in a quarter turn against the clock about a point.</summary>
    private static (int X, int Y) Turned((int X, int Y) cell, double px, double py) =>
        ((int) Math.Floor(px - (cell.Y + 0.5 - py)), (int) Math.Floor(py + (cell.X + 0.5 - px)));

    /// <summary>
    /// Where a decal's corner is within its cell when the decal turns with that cell alone and
    /// not as one piece with the region. It is what a turn falls back on for a decal whose
    /// corner would otherwise be on a cell with no tile, where the kernel refuses a decal
    /// (TurnPlan). The cell goes where the turn takes it, and the corner stays in it.
    /// The corner's offset in the cell, x and y from 0 up to 1, is turned about the cell's middle:
    ///   against the clock   (x, y) to (1 - y, x)
    ///   with the clock      (x, y) to (y, 1 - x)
    /// The decal's own square turns with it, so a 1 that would come of a 0 is 0, and a decal
    /// drawn exactly on its cell is exactly on the turned cell. The two directions undo each
    /// other, and four turns one way bring the offset back.
    /// </summary>
    private static (float X, float Y) TurnedOffset(float x, float y, bool clockwise) => clockwise
        ? (y, x == 0 ? 0 : 1 - x)
        : (y == 0 ? 0 : 1 - y, x);

    /// <summary>A cell's number and an offset in it as one number, which is still in the cell.</summary>
    private static float Within(int cell, float offset) => MathF.Min(cell + offset, MathF.BitDecrement(cell + 1f));

    /// <summary>
    /// A quarter turn against the clock about a point: tiles to their turned cells, with the
    /// rotation byte one further for the tiles drawn by it (its low two bits count quarter
    /// turns, the bit above them is the mirroring and stays). Entities are moved and rotated.
    /// Angles are counted in quarters (ByQuarters).
    /// A decal turns as one piece with the region: its middle, half a cell from its corner,
    /// goes about the point, and its angle a quarter on. Only where its corner would then be
    /// on a cell that holds no tile after the edit does it fall back onto the cell its own
    /// cell turns to, and keep its place in that cell, turned (TurnedOffset). A decal with no
    /// tile under it either way gets no op.
    /// </summary>
    /// <param name="all">Every tile of the grid before the turn.</param>
    private static Plan TurnPlan(int grid, Standing on, Dictionary<(int X, int Y), TileOn> all, double px, double py, IReadOnlySet<string> turning)
    {
        var ops = new List<EditOp>();
        var tiles = on.Tiles.ToDictionary(
            pair => Turned(pair.Key, px, py),
            pair => turning.Contains(pair.Value.Id) ? pair.Value with { Turn = (pair.Value.Turn & 4) | ((pair.Value.Turn + 1) & 3) } : pair.Value);
        foreach (var (cell, tile) in tiles)
            ops.Add(TileOp(grid, cell, tile));

        var entities = on.Entities
            .Select(entity => (entity, (float) (px - (entity.Y - py)), (float) (py + (entity.X - px)), ByQuarters(entity.Rotation)))
            .ToList();
        foreach (var (entity, x, y, rotation) in entities)
        {
            ops.Add(new EditOp("move", Uid: entity.Uid, X: x, Y: y));
            ops.Add(new EditOp("rotate", Uid: entity.Uid, Rotation: rotation));
        }

        // A cell holds a tile after the edit if one is written there, or if it had one and is
        // not a cell the region leaves.
        bool Tiled((int X, int Y) cell) => tiles.ContainsKey(cell) || (all.ContainsKey(cell) && !on.Tiles.ContainsKey(cell));

        var fallback = new List<int>();
        var decals = new List<(SceneDecal, float, float, double)>();
        foreach (var decal in on.Decals)
        {
            var (mx, my) = (decal.X + 0.5, decal.Y + 0.5);
            var (x, y) = ((float) (px - (my - py) - 0.5), (float) (py + (mx - px) - 0.5));
            if (!Tiled(Cell(x, y)))
            {
                var from = Cell(decal.X, decal.Y);
                var to = Turned(from, px, py);
                if (!Tiled(to))
                    continue;

                var (ox, oy) = TurnedOffset(decal.X - from.X, decal.Y - from.Y, clockwise: false);
                (x, y) = (Within(to.X, ox), Within(to.Y, oy));
                fallback.Add(decal.Id);
            }

            decals.Add((decal, x, y, ByQuarters(decal.Angle)));
            ops.Add(DecalOp(grid, decal, x, y, ByQuarters(decal.Angle)));
        }

        var emptied = on.Tiles.Keys.Where(cell => !tiles.ContainsKey(cell)).ToList();
        foreach (var cell in emptied)
            ops.Add(TileOp(grid, cell, null));

        // Only the places turn. The air itself has no facing.
        var air = AirOps(grid, on.Tiles.Keys.Select(cell => (cell, Turned(cell, px, py))), ops);
        return new Plan(ops, tiles, emptied, entities, decals, fallback, air);
    }

    /// <summary>
    /// Angles, and what each is a quarter turn on. The editor has the same rule and the same
    /// cases (quarter_on in tools/select.rs, tried in tools/tests/area.rs).
    /// </summary>
    private static readonly (double Angle, double On)[] QuarterCases =
    [
        (0, Quarter), (Quarter, Math.PI), (Math.PI, -Quarter), (-Quarter, 0),
        (3 * Quarter, Math.Tau), (Math.Tau, 5 * Quarter), (5 * Quarter, 3 * Math.PI), (3 * Math.PI, 3 * Quarter),
        (-Math.PI, -5 * Quarter), (-5 * Quarter, -Math.Tau),
        (0.75 * Math.PI, -0.75 * Math.PI), (-0.75 * Math.PI, -0.25 * Math.PI), (-0.25 * Math.PI, 0.25 * Math.PI), (0.25 * Math.PI, 0.75 * Math.PI),
    ];

    /// <summary>
    /// An angle a quarter turn on. The angle is taken as so many whole quarter turns and a
    /// rest. One quarter is added, and the count stays within the four that begin just after
    /// minus a half turn, or the same four any number of whole turns away, whichever the angle
    /// is already in. So four turns come back to the very number, whatever turn the file wrote
    /// the angle in. Adding a quarter and leaving the sum, or bringing it into 0 to 2 pi or
    /// into -pi to pi, does not: the file's rotation and decal angle lines come back changed.
    /// An angle half way between two quarters counts as the upper one, each time. Rounded to
    /// the even one, as Math.Round does, such an angle came back a whole turn away.
    /// </summary>
    private static double ByQuarters(double angle)
    {
        var quarters = Math.Floor(angle / Quarter + 0.5);
        var rest = angle - quarters * Quarter;
        var first = 4 * Math.Floor((quarters + 1) / 4) - 1;
        var next = quarters + 1;
        if (next > first + 3)
            next -= 4;
        return next * Quarter + rest;
    }

    // ---- one map ----

    private sealed class Check(EngineSession session, string forkRepo, string map, string outFolder, IReadOnlySet<string> turning, IReadOnlySet<string> decalKinds, Action<string> say)
    {
        private readonly string _name = Path.GetFileNameWithoutExtension(map);
        private int _document;
        private int _grid;
        private int _gridYaml;
        private int _mapUid;
        private bool _canUndo;
        private Saved _before = null!;
        private (long EngineMs, long TotalMs, int Ops, string Label) _largest;

        public int Failures { get; private set; }

        public void Ok(string text) => say($"  ok    {text}");

        public void Found(string text) => say($"  found {text}");

        public void Info(string text) => say($"  info  {text}");

        private void Skip(string text) => CheckSkips.Skip(say, text);

        public void Fail(string text)
        {
            say($"  FAIL  {text}");
            Failures++;
        }

        private void Expect(bool holds, string ok, string fail)
        {
            if (holds)
                Ok(ok);
            else
                Fail(fail);
        }

        public void Close()
        {
            if (_document != 0)
                session.Close(_document);
        }

        private Scene Opened(string stage, string yaml, string source)
        {
            using var timed = CheckTimes.In(stage);
            return session.Open(yaml, source).Scene;
        }

        private Saved Save(string tag) => Save(_document, tag);

        private Saved Save(int document, string tag)
        {
            var saved = Saving(document);
            if (!saved.Ok || saved.Yaml == null)
                throw new InvalidOperationException($"not saved: {saved.Failure}");

            return Written(tag, saved.Yaml);
        }

        private SaveResult Saving(int document)
        {
            using var timed = CheckTimes.In(CheckTimes.Save);
            return session.Save(document);
        }

        private Saved Written(string tag, string yaml)
        {
            using var timed = CheckTimes.In(CheckTimes.Text);
            File.WriteAllText(Path.Combine(outFolder, $"{_name}.{tag}.yml"), yaml);
            return Saved.Of(yaml.Split('\n'));
        }

        private (SceneDelta Delta, Shown Time) Edit(string label, IReadOnlyList<EditOp> ops)
        {
            using var timed = CheckTimes.In(CheckTimes.Edit);
            var (delta, time) = session.Edit(_document, label, ops);
            _canUndo = delta.History.CanUndo;
            if (time.DocumentMs >= _largest.EngineMs)
                _largest = (time.DocumentMs, time.TotalMs, ops.Count, label);
            return (delta, time);
        }

        private (SceneDelta Delta, Shown Time) Undo()
        {
            using var timed = CheckTimes.In(CheckTimes.Edit);
            var (delta, time) = session.Undo(_document);
            _canUndo = delta.History.CanUndo;
            return (delta, time);
        }

        private SceneDelta Redo()
        {
            using var timed = CheckTimes.In(CheckTimes.Edit);
            var (delta, _) = session.Redo(_document);
            _canUndo = delta.History.CanUndo;
            return delta;
        }

        /// <summary>Whether a save is the first save, said either way, with the lines that differ.</summary>
        private bool Equal(Saved saved, Saved wanted, string what, string than)
        {
            if (Same(saved, wanted))
            {
                Ok($"{what}: the save is {than}");
                return true;
            }

            var differences = Compare(wanted, saved);
            if (differences.OnlyChunkOrder)
            {
                Fail($"{what}: the save is not {than}. Every line is the same, but {differences.Removed("tile chunk")} tile chunk lines are in another place: the grid writes its chunks in the order it holds them, and chunks that were dropped and made again are held in another order");
                return false;
            }

            Fail($"{what}: the save is not {than}. They differ in {differences.Describe()}");
            foreach (var line in differences.Examples(_ => true, 8))
                say(line);
            return false;
        }

        /// <summary>One experiment. Whatever it leaves done is undone, so the next begins from the first save.</summary>
        private void Try(string what, Action experiment)
        {
            try
            {
                experiment();
            }
            catch (Exception e)
            {
                Fail($"{what}: {e.Message}");
            }

            try
            {
                while (_canUndo)
                    Undo();
            }
            catch (Exception e)
            {
                Fail($"{what}, undoing: {e.Message}");
                _canUndo = false;
            }
        }

        private string Place(SceneEntity entity) =>
            $"{(entity.Parent == _grid ? "child of the grid" : entity.Parent == _mapUid ? "child of the map" : $"child of entity {entity.Parent}")}, {(entity.Anchored ? "anchored" : "not anchored")}, at {entity.X.ToString(CultureInfo.InvariantCulture)},{entity.Y.ToString(CultureInfo.InvariantCulture)}";

        public void Run()
        {
            var scene = Opened(CheckTimes.Open(map), File.ReadAllText(Path.Combine(forkRepo, "Resources", map)), map);
            _document = scene.Document.Id;
            _mapUid = scene.Document.MapUid;
            _before = Save("before");

            var grid = scene.Grids.OrderByDescending(g => g.Tiles.Count).First();
            _grid = grid.Uid;
            _gridYaml = grid.YamlUid;
            var tiles = TilesOf(grid);
            var holders = scene.Entities.Where(entity => entity.Contained).GroupBy(entity => entity.Parent).ToDictionary(group => group.Key, group => group.Count());
            var alarms = scene.Entities
                .Where(entity => !entity.Contained && entity.Parent == _grid)
                .Where(entity => entity.Prototype.Contains("AirAlarm", StringComparison.Ordinal) || entity.Prototype.Contains("FireAlarm", StringComparison.Ordinal))
                .ToArray();
            var linked = MoveCheck.Devices(session, _document, alarms, scene.Entities.ToDictionary(entity => entity.Uid))
                .Where(pair => pair.Devices.Count > 0)
                .Select(pair => (pair.Alarm, Devices: pair.Devices.Where(device => device.Parent == _grid && !device.Contained).ToList()))
                .ToList();

            Scan(scene, grid, tiles);

            var (left, bottom) = Pick(scene, grid, tiles, holders, linked);
            var rectangle = new HashSet<(int X, int Y)>();
            for (var x = left; x < left + Width; x++)
            for (var y = bottom; y < bottom + Height; y++)
            {
                if (tiles.ContainsKey((x, y)))
                    rectangle.Add((x, y));
            }

            var on = On(scene, _grid, rectangle);
            var inside = on.Entities.Select(entity => entity.Uid).ToHashSet();
            var walls = on.Entities.Where(entity => entity.Anchored && entity.Prototype.StartsWith("Wall", StringComparison.Ordinal)).ToList();
            var containers = on.Entities.Where(entity => holders.ContainsKey(entity.Uid)).OrderByDescending(entity => holders[entity.Uid]).ToList();
            var loose = on.Entities.Where(entity => !entity.Anchored && !holders.ContainsKey(entity.Uid)).ToList();
            var alarmsInside = linked.Where(pair => inside.Contains(pair.Alarm.Uid)).ToList();
            say($"  region {Width} by {Height} cells from {left},{bottom} on the grid \"{grid.Name}\": {on.Tiles.Count} tiles of {on.Tiles.Values.Select(tile => tile.Id).Distinct().Count()} kinds, "
                + $"{on.Entities.Count} entities ({on.Entities.Count(entity => entity.Anchored)} anchored, {walls.Count} of them walls, and {on.Entities.Count(entity => !entity.Anchored)} loose), "
                + $"{containers.Count} containers holding {containers.Sum(entity => holders[entity.Uid])} things, "
                + $"{alarmsInside.Count} alarms with {alarmsInside.Sum(pair => pair.Devices.Count(device => inside.Contains(device.Uid)))} of their {alarmsInside.Sum(pair => pair.Devices.Count)} linked devices inside, "
                + $"{on.Decals.Count} decals{(Strangers(on) > 0 ? $", {Strangers(on)} of them of a kind the game does not have" : "")}");

            Air(grid, on);

            // Far enough to the right that nothing is there, with room around to turn in.
            var far = tiles.Keys.Max(cell => cell.X) + 8 - left;

            Try("move far", () => Move("moved far", "far", rectangle, far, 0, tiles, scene));
            Try("move near", () => Move("moved one cell", "near", rectangle, 1, 0, tiles, scene));
            Try("delete", () => Delete(scene, rectangle));
            Try("two things one source is linked to, deleted", () => Links(scene));
            Try("turn", () => Turn(rectangle, far, left + Width / 2.0, bottom + Height / 2.0));
            Try("a tile emptied under a wall", () => Emptied("an anchored wall", walls.FirstOrDefault(), scene));
            Try("a tile emptied under a loose thing", () => Emptied("a loose thing", loose.FirstOrDefault(entity => !entity.Prototype.Contains("Spawn", StringComparison.Ordinal)) ?? loose.FirstOrDefault(), scene));
            Try("a tile emptied under a container", () => Emptied("a container with contents", containers.FirstOrDefault(), scene));
            Try("an anchored entity onto space", () => Anchored(walls, (left + far + 40, bottom)));
            Try("decals placed, changed, moved and removed", () => Decals(grid, tiles));
            Try("a decal of a kind the game does not have", () => Stranger(grid, tiles));
            Try("a large region", () => Large(scene, grid, tiles, left + Width / 2 - 25, bottom + Height / 2 - 25));

            Equal(Save("end"), _before, "at the end, with everything undone", "the first save");
            Info($"the edit the engine took longest over: \"{_largest.Label}\", {_largest.Ops} ops, {_largest.EngineMs} ms in the document kernel, {_largest.TotalMs} ms with the picture redrawn");
        }

        /// <summary>How many of the decals are of a kind the game does not have. A map can hold such.</summary>
        private int Strangers(Standing on) => on.Decals.Count(decal => !decalKinds.Contains(decal.Prototype));

        /// <summary>The grid as the scene has it now.</summary>
        private SceneGrid GridNow() => session.Scene(_document).Grids.First(g => g.Uid == _grid);

        /// <summary>
        /// Two saves to compare line by line. On a fork that keeps its decals on chunk entities,
        /// without the decals' storage, which DecalsAsGame checks apart.
        /// </summary>
        private static Differences Lines(Saved was, Saved now) => Chunked ? Compare(Plainly(was), Plainly(now)) : Compare(was, now);

        /// <summary>
        /// On a fork that keeps its decals on chunk entities: whether the save's decal chunks are
        /// what the game's decal system makes of the edit's decal ops on those of the save before,
        /// and each decal of the scene is where the kernel's numbers say. Said either way.
        /// </summary>
        /// <param name="places">Each decal's chunk and id by its number before the edit.</param>
        private void DecalsAsGame(string what, Saved was, Saved now, IEnumerable<EditOp> ops, Dictionary<int, Place> places)
        {
            if (!Chunked)
                return;

            var want = AsGame(ModelOf(was, _gridYaml), ops, _grid, new Dictionary<int, Place>(places));
            var got = ModelOf(now, _gridYaml);
            var unlike = Unlike(want, got);
            var misplaced = Misplaced(GridNow(), got);
            Expect(unlike.Count == 0 && misplaced == 0,
                $"{what}: the save's decal chunks are what the game's decal system makes of the decal ops: {got.Decals.Count} decals in {got.Max.Count} chunk entities, each with the id and highest id given expected, and each decal of the scene is in the chunk and under the id its number says",
                $"{what}: the save's decal chunks are not what the game's decal system makes of the decal ops. {unlike.Count} things differ, and {misplaced} decals of the scene are not where the kernel's numbers say");
            foreach (var line in unlike.Take(6))
                say($"        {line}");
        }

        /// <summary>
        /// On a fork that keeps its decals on the grid: whether the save has each decal of the
        /// scene under its number, as the scene has it, and no other. Said either way.
        /// </summary>
        private void DecalsSaved(string what, Saved now)
        {
            if (Chunked)
                return;

            var saved = GridDecals(now, _gridYaml);
            var scene = GridNow().Decals;
            var wrong = scene.Count(decal => saved.GetValueOrDefault(decal.Id) != ModelDecal.Of(decal)) + saved.Keys.Count(id => scene.All(decal => decal.Id != id));
            Expect(wrong == 0,
                $"{what}: the save has each of the grid's {scene.Count} decals under its number, as the scene has it",
                $"{what}: {wrong} decals of the save and the scene are not alike");
        }

        /// <summary>
        /// The decals of a grid as a save has them: for each its coordinates, and the lines of
        /// the group it is written under (kind, colour, angle and the rest).
        /// </summary>
        private static Dictionary<int, (string Group, string At)> DecalLines(Saved saved, int gridYamlUid)
        {
            var decals = new Dictionary<int, (string Group, string At)>();
            var group = new List<string>();
            var text = "";
            for (var i = 0; i < saved.Lines.Length; i++)
            {
                if (saved.Uid[i] != gridYamlUid || saved.Part[i] != "DecalGrid")
                    continue;

                var line = saved.Lines[i].Trim();
                if (line == "- node:")
                    group.Clear();
                else if (line == "decals:")
                    text = string.Join(", ", group);
                else if (saved.Kind(i) == "decal coordinate")
                    decals[int.Parse(line.AsSpan(0, line.IndexOf(':')), CultureInfo.InvariantCulture)] = (text, line[(line.IndexOf(':') + 1)..].Trim());
                else
                    group.Add(line);
            }

            return decals;
        }

        /// <summary>What on the grid a region edit could trip over.</summary>
        private void Scan(Scene scene, SceneGrid grid, Dictionary<(int X, int Y), TileOn> tiles)
        {
            var children = scene.Entities.Where(entity => entity.Parent == _grid && !entity.Contained).ToList();
            static bool Whole(float value) => MathF.Abs(value - MathF.Round(value)) < 1e-4f;
            static bool Middle(float value) => MathF.Abs(value - MathF.Floor(value) - 0.5f) < 1e-3f;
            var edge = children.Count(entity => Whole(entity.X) || Whole(entity.Y));
            var edgeAnchored = children.Count(entity => entity.Anchored && (Whole(entity.X) || Whole(entity.Y)));
            var offCentre = children.Count(entity => entity.Anchored && !(Middle(entity.X) && Middle(entity.Y)));
            var overSpace = children.Count(entity => !tiles.ContainsKey(Cell(entity.X, entity.Y)));
            var decalsOffCell = grid.Decals.Count(decal => Cell(decal.X, decal.Y) != Cell(decal.X + 0.5f, decal.Y + 0.5f));
            Info($"the grid has {tiles.Count} tiles, {tiles.Values.Count(tile => turning.Contains(tile.Id))} of a kind drawn by its rotation byte and {tiles.Values.Count(tile => tile.Turn != 0)} with that byte not 0; "
                + $"{children.Count} entities stand on it: {edge} exactly on a cell's edge ({edgeAnchored} of them anchored), {offCentre} anchored off a cell's middle, {overSpace} over space; "
                + $"{grid.Decals.Count} decals, {decalsOffCell} with the middle in another cell than the corner");
        }

        /// <summary>The rectangle that holds the most kinds of thing, tried around every alarm with devices and every container.</summary>
        private (int Left, int Bottom) Pick(
            Scene scene,
            SceneGrid grid,
            Dictionary<(int X, int Y), TileOn> tiles,
            Dictionary<int, int> holders,
            List<(SceneEntity Alarm, List<SceneEntity> Devices)> linked)
        {
            var byCell = new Dictionary<(int X, int Y), List<SceneEntity>>();
            foreach (var entity in scene.Entities)
            {
                if (entity.Parent != _grid || entity.Contained)
                    continue;

                var cell = Cell(entity.X, entity.Y);
                if (!byCell.TryGetValue(cell, out var list))
                    byCell[cell] = list = new List<SceneEntity>();
                list.Add(entity);
            }

            var decals = grid.Decals.GroupBy(decal => Cell(decal.X, decal.Y)).ToDictionary(group => group.Key, group => group.Count());
            var around = linked.Select(pair => Cell(pair.Alarm.X, pair.Alarm.Y))
                .Concat(scene.Entities.Where(entity => holders.ContainsKey(entity.Uid) && entity.Parent == _grid && !entity.Contained).Take(300).Select(entity => Cell(entity.X, entity.Y)))
                .Distinct()
                .ToList();
            if (around.Count == 0)
                around.Add(tiles.Keys.First());

            var best = (Score: -1L, Left: 0, Bottom: 0);
            var tried = new HashSet<(int, int)>();
            foreach (var (ax, ay) in around)
            for (var left = ax - Width + 1; left <= ax; left++)
            for (var bottom = ay - Height + 1; bottom <= ay; bottom++)
            {
                if (!tried.Add((left, bottom)))
                    continue;

                var (floor, walls, loose, containers, links, marks) = (0, 0, 0, 0, 0, 0);
                for (var x = left; x < left + Width; x++)
                for (var y = bottom; y < bottom + Height; y++)
                {
                    if (!tiles.ContainsKey((x, y)))
                        continue;

                    floor++;
                    marks += decals.GetValueOrDefault((x, y));
                    foreach (var entity in byCell.GetValueOrDefault((x, y)) ?? [])
                    {
                        if (holders.ContainsKey(entity.Uid))
                            containers++;
                        else if (!entity.Anchored)
                            loose++;
                        else if (entity.Prototype.StartsWith("Wall", StringComparison.Ordinal))
                            walls++;
                    }
                }

                bool Inside(SceneEntity entity) => entity.X >= left && entity.X < left + Width && entity.Y >= bottom && entity.Y < bottom + Height;
                foreach (var (alarm, devices) in linked)
                {
                    if (Inside(alarm))
                        links += devices.Count(Inside);
                }

                var kinds = (walls > 0 ? 1 : 0) + (loose > 0 ? 1 : 0) + (containers > 0 ? 1 : 0) + (links > 0 ? 1 : 0) + (marks > 0 ? 1 : 0);
                var score = kinds * 1_000_000L + floor * 1000 + Math.Min(walls, 20) + Math.Min(loose, 20) * 3 + Math.Min(containers, 5) * 10 + Math.Min(links, 10) * 10 + Math.Min(marks, 30);
                if (score > best.Score)
                    best = (score, left, bottom);
            }

            return (best.Left, best.Bottom);
        }

        /// <summary>
        /// The air a save stores on the grid, by cell, each cell with the text of its mix. So two
        /// saves compare by what each cell holds, whatever order they number their mixes in.
        /// Null if the grid stores none.
        /// </summary>
        private Dictionary<(int X, int Y), string>? AirOf(Saved saved)
        {
            var mixes = new List<string>();
            var cells = new Dictionary<(int X, int Y), string>();
            var (found, inMixes, inTiles) = (false, false, false);
            var chunk = (X: 0, Y: 0);
            for (var i = 0; i < saved.Lines.Length; i++)
            {
                if (saved.Uid[i] != _gridYaml || saved.Part[i] != "GridAtmosphere")
                    continue;

                found = true;
                var line = saved.Lines[i];
                if (line == "        uniqueMixes:" || line == "        tiles:")
                {
                    (inMixes, inTiles) = (line.EndsWith("uniqueMixes:", StringComparison.Ordinal), line.EndsWith("tiles:", StringComparison.Ordinal));
                    continue;
                }

                if (inMixes && line.StartsWith("        - ", StringComparison.Ordinal))
                {
                    mixes.Add(line[10..]);
                    continue;
                }

                if (inMixes && line.StartsWith("          ", StringComparison.Ordinal))
                {
                    mixes[^1] += "; " + line.Trim();
                    continue;
                }

                if (inTiles && line.StartsWith("          ", StringComparison.Ordinal))
                {
                    var text = line.Trim();
                    var colon = text.IndexOf(':');
                    if (text.EndsWith(':'))
                    {
                        var parts = text[..colon].Split(',');
                        chunk = (int.Parse(parts[0], CultureInfo.InvariantCulture), int.Parse(parts[1], CultureInfo.InvariantCulture));
                        continue;
                    }

                    var mix = mixes[int.Parse(text[..colon], CultureInfo.InvariantCulture)];
                    var mask = uint.Parse(text[(colon + 1)..].Trim(), CultureInfo.InvariantCulture);
                    for (var bit = 0; bit < 16; bit++)
                    {
                        if ((mask & (1u << bit)) != 0)
                            cells[(chunk.X * 4 + bit % 4, chunk.Y * 4 + bit / 4)] = mix;
                    }

                    continue;
                }

                (inMixes, inTiles) = (false, false);
            }

            return found ? cells : null;
        }

        /// <summary>Whether two saves store the same air in every cell of the grid.</summary>
        private bool SameAir(Saved a, Saved b)
        {
            var (x, y) = (AirOf(a), AirOf(b));
            return x == null || y == null ? x == y : x.Count == y.Count && x.All(pair => y.TryGetValue(pair.Key, out var mix) && mix == pair.Value);
        }

        /// <summary>
        /// The air of a save after a region moved or turned, against the air of the save before:
        /// each cell the region went to stores what its own cell stored, or nothing if that
        /// stored nothing, each cell it left stores nothing, and every other cell is as it was.
        /// Then the kernel's air question, which the air overlay draws from, against the save.
        /// </summary>
        private void AirCarried(string what, Saved was, Saved now, Plan plan)
        {
            var before = AirOf(was);
            if (before == null)
            {
                Skip($"{what}: the air moved with it. The grid stores no air");
                return;
            }

            var after = AirOf(now) ?? new Dictionary<(int X, int Y), string>();
            var expected = new Dictionary<(int X, int Y), string>(before);
            foreach (var (from, _) in plan.Air)
                expected.Remove(from);
            foreach (var (from, to) in plan.Air)
            {
                if (before.TryGetValue(from, out var mix))
                    expected[to] = mix;
                else
                    expected.Remove(to);
            }

            var wrong = expected.Keys.Union(after.Keys)
                .Where(cell => expected.GetValueOrDefault(cell) != after.GetValueOrDefault(cell))
                .OrderBy(cell => cell)
                .ToList();
            var gone = plan.Air.Select(pair => pair.To).ToHashSet();
            var carried = plan.Air.Count(pair => before.ContainsKey(pair.From));
            var left = plan.Air.Count(pair => !gone.Contains(pair.From) && before.ContainsKey(pair.From));
            var differ = plan.Air.Count(pair => before.GetValueOrDefault(pair.From) != before.GetValueOrDefault(pair.To));
            Expect(wrong.Count == 0 && carried > 0,
                $"{what}: the air went with the tiles. {carried} of the region's {plan.Air.Count} cells stored air and the cells they went to store the same, {differ} of them other air than was there. The {left} cells left behind that stored air store none, and the grid's other {after.Count - carried} cells with air are as they were",
                $"{what}: the air did not go with the tiles. {wrong.Count} cells store other air than expected, of {plan.Air.Count} cells carried, {carried} of them with air");
            foreach (var cell in wrong.Take(4))
                say($"        cell {cell.X},{cell.Y}: stores {after.GetValueOrDefault(cell) ?? "nothing"}, expected {expected.GetValueOrDefault(cell) ?? "nothing"}");

            // The kernel reads its own component, which the save was written from.
            using var answer = JsonDocument.Parse(session.Command("air", $"{{\"doc\":{_document},\"grids\":[{_grid}]}}"));
            var numbers = answer.RootElement.GetProperty("grids")[0].GetProperty("tiles").EnumerateArray().Select(number => number.GetInt32()).ToArray();
            var mixOf = new Dictionary<(int X, int Y), int>();
            for (var at = 0; at + 2 < numbers.Length; at += 3)
                mixOf[(numbers[at], numbers[at + 1])] = numbers[at + 2];
            var textOf = new Dictionary<int, string>();
            var numberOf = new Dictionary<string, int>(StringComparer.Ordinal);
            var unlike = 0;
            foreach (var (cell, number) in mixOf)
            {
                if (!after.TryGetValue(cell, out var text) || textOf.GetValueOrDefault(number, text) != text || numberOf.GetValueOrDefault(text, number) != number)
                    unlike++;
                textOf[number] = text ?? "";
                numberOf[text ?? ""] = number;
            }

            Expect(mixOf.Count == after.Count && unlike == 0,
                $"{what}: the kernel's air answer, which the air overlay draws, has the {after.Count} cells with air the save has, and the same mix in each",
                $"{what}: the kernel's air answer has {mixOf.Count} cells with air, the save {after.Count}, and {unlike} cells hold another mix than the save's");
        }

        /// <summary>
        /// The air of a save after a region was deleted, against the air of the first save: each
        /// cell emptied stores none, and every other cell is as it was.
        /// </summary>
        private void AirCleared(Saved now, Plan plan)
        {
            var before = AirOf(_before);
            var emptied = plan.Emptied.ToHashSet();
            var held = before?.Keys.Count(emptied.Contains) ?? 0;
            if (before == null || held == 0)
            {
                Skip("deleted: the air of the emptied cells went. None of them stored air");
                return;
            }

            var after = AirOf(now) ?? new Dictionary<(int X, int Y), string>();
            var wrong = before.Keys.Union(after.Keys)
                .Where(cell => (emptied.Contains(cell) ? null : before.GetValueOrDefault(cell)) != after.GetValueOrDefault(cell))
                .OrderBy(cell => cell)
                .ToList();
            Expect(wrong.Count == 0,
                $"deleted: the {held} emptied cells that stored air store none, and the grid's other {after.Count} cells with air are as they were",
                $"deleted: {wrong.Count} cells store other air than expected. Of the {emptied.Count} cells emptied {held} stored air");
            foreach (var cell in wrong.Take(4))
                say($"        cell {cell.X},{cell.Y}: stores {after.GetValueOrDefault(cell) ?? "nothing"}, expected {(emptied.Contains(cell) ? "nothing" : before.GetValueOrDefault(cell) ?? "nothing")}");
        }

        /// <summary>
        /// How many of the region's cells have air of their own in what the grid stores
        /// (GridAtmosphere, read from the first save: mixes by number, and for each block of four
        /// by four cells a mask of the cells that have each mix). A move or a turn of the region
        /// carries it with the tiles (air ops).
        /// </summary>
        private void Air(SceneGrid grid, Standing on)
        {
            var mixOf = new Dictionary<(int X, int Y), int>();
            var lines = _before.Lines;
            var at = -1;
            for (var i = 0; i < lines.Length && at < 0; i++)
            {
                if (_before.Uid[i] == grid.YamlUid && _before.Part[i] == "GridAtmosphere" && lines[i] == "        tiles:")
                    at = i;
            }

            if (at < 0)
            {
                Info("air: the grid stores no air by cell");
                return;
            }

            var block = (X: 0, Y: 0);
            for (var i = at + 1; i < lines.Length && lines[i].StartsWith("          ", StringComparison.Ordinal); i++)
            {
                var text = lines[i].Trim();
                var colon = text.IndexOf(':');
                if (text.EndsWith(':'))
                {
                    var parts = text[..colon].Split(',');
                    block = (int.Parse(parts[0], CultureInfo.InvariantCulture), int.Parse(parts[1], CultureInfo.InvariantCulture));
                    continue;
                }

                var mix = int.Parse(text[..colon], CultureInfo.InvariantCulture);
                var mask = uint.Parse(text[(colon + 1)..].Trim(), CultureInfo.InvariantCulture);
                for (var bit = 0; bit < 16; bit++)
                {
                    if ((mask & (1u << bit)) != 0)
                        mixOf[(block.X * 4 + bit % 4, block.Y * 4 + bit / 4)] = mix;
                }
            }

            var common = mixOf.Values.GroupBy(mix => mix).OrderByDescending(group => group.Count()).First().Key;
            var here = on.Tiles.Keys.Where(mixOf.ContainsKey).ToList();
            var byMix = here.GroupBy(cell => mixOf[cell]).OrderBy(group => group.Key).Select(group => $"mix {group.Key}: {group.Count()}");
            Info($"air: {here.Count} of the region's {on.Tiles.Count} cells have air stored for them ({string.Join(", ", byMix)}), "
                + $"{here.Count(cell => mixOf[cell] != common)} of them a mix other than the grid's commonest (mix {common}). The grid stores air for {mixOf.Count} cells in {mixOf.Values.Distinct().Count()} mixes");
        }

        /// <summary>What the engine holds after an edit that should not be so.</summary>
        /// <param name="unturned">Entities sent a new rotation that kept the one they had.</param>
        private List<string> Wrong(Plan plan, out int unturned)
        {
            unturned = 0;
            var wrong = new List<string>();
            var scene = session.Scene(_document);
            var tiles = TilesOf(scene.Grids.First(g => g.Uid == _grid));
            foreach (var (cell, tile) in plan.Tiles)
            {
                if (!tiles.TryGetValue(cell, out var has) || has != tile)
                    wrong.Add($"cell {cell.X},{cell.Y} holds {(tiles.ContainsKey(cell) ? has.ToString() : "space")}, not {tile}");
            }

            foreach (var cell in plan.Emptied)
            {
                if (tiles.ContainsKey(cell))
                    wrong.Add($"cell {cell.X},{cell.Y} is not space");
            }

            var byUid = scene.Entities.ToDictionary(entity => entity.Uid);
            foreach (var (was, x, y, rotation) in plan.Entities)
            {
                if (!byUid.TryGetValue(was.Uid, out var now))
                {
                    wrong.Add($"{was.Prototype} {was.Uid} is gone");
                    continue;
                }

                // Anchoring puts an entity on the middle of its cell.
                var (wantX, wantY) = was.Anchored ? (MathF.Floor(x) + 0.5f, MathF.Floor(y) + 0.5f) : (x, y);
                var turned = Math.Abs(now.Rotation - rotation) <= 1e-9;
                if (!turned && now.Rotation == was.Rotation)
                    unturned++;
                if (now.Parent != _grid || now.Anchored != was.Anchored || MathF.Abs(now.X - wantX) > 1e-3f || MathF.Abs(now.Y - wantY) > 1e-3f || (!turned && now.Rotation != was.Rotation))
                    wrong.Add($"{was.Prototype} {was.Uid} (was {Place(was)}) is {Place(now)} turned {now.Rotation}, not at {wantX},{wantY} turned {rotation}");
            }

            var decals = scene.Grids.First(g => g.Uid == _grid).Decals.ToDictionary(decal => decal.Id);
            foreach (var (was, x, y, angle) in plan.Decals)
            {
                if (!decals.TryGetValue(was.Id, out var now) || now.X != x || now.Y != y || now.Angle != angle || now.Prototype != was.Prototype || now.Color != was.Color || now.ZIndex != was.ZIndex || now.Cleanable != was.Cleanable)
                    wrong.Add($"decal {was.Id} is {(now == null ? "gone" : $"at {now.X},{now.Y} turned {now.Angle}")}, not at {x},{y} turned {angle}");
            }

            return wrong;
        }

        /// <summary>The delta of an edit that only moves things: nothing gone, nothing new, and what was anchored still is.</summary>
        private void Kept(string what, SceneDelta delta, Plan plan)
        {
            Expect(delta.Removed.Count == 0 && delta.Created.Count == 0,
                $"{what}: the delta removes and creates nothing",
                $"{what}: the delta removes {delta.Removed.Count} entities and creates {delta.Created.Count}");

            var reported = delta.Entities.ToDictionary(entity => entity.Uid);
            var named = plan.Entities.Select(entity => entity.Was.Uid).ToHashSet();
            var anchored = plan.Entities.Where(entity => entity.Was.Anchored).ToList();
            var lost = anchored.Where(entity => !reported.TryGetValue(entity.Was.Uid, out var now) || !now.Anchored || now.Parent != _grid).ToList();
            var others = delta.Entities.Where(entity => !named.Contains(entity.Uid)).ToList();
            var wrong = Wrong(plan, out var unturned);
            if (unturned > 0)
                Found($"{what}: expected every entity to take its new rotation. {unturned} of {plan.Entities.Count} kept the rotation they had: their prototype says they are never rotated (noRot, as cables and lockers are), and the engine ignores a rotation for them. They are moved like the rest");
            if (lost.Count == 0 && wrong.Count == 0 && others.Count == 0)
            {
                Ok($"{what}: all {anchored.Count} anchored entities are still anchored children of the grid, and the engine holds every tile ({plan.Tiles.Count}), entity ({plan.Entities.Count}) and decal ({plan.Decals.Count}) where it was sent");
                return;
            }

            Fail($"{what}: {lost.Count} of {anchored.Count} anchored entities are not reported anchored on the grid, {wrong.Count} things are not as sent, and the delta reports {others.Count} entities no op named");
            foreach (var entity in lost.Take(4))
                say($"        {entity.Was.Prototype} {entity.Was.Uid}: {(reported.TryGetValue(entity.Was.Uid, out var now) ? Place(now) : "not in the delta")}");
            foreach (var line in wrong.Take(6))
                say($"        {line}");
            foreach (var entity in others.Take(4))
                say($"        not named: {entity.Prototype} {entity.Uid}, {Place(entity)}");
        }

        /// <summary>
        /// Of the positions and decal corners that differ, how many differ only in a number's last
        /// digits (they went through sums and back), and how many by more. Null if a line that
        /// differs has no partner for the same entity or decal.
        /// </summary>
        /// <param name="apartFrom">Decals that are left out of the count.</param>
        private static (int Close, double Closest, int Apart, double Furthest)? Shifts(Differences differences, IReadOnlySet<int>? apartFrom = null)
        {
            var (close, closest, apart, furthest) = (0, 0.0, 0, 0.0);
            foreach (var group in differences.Lines
                         .Where(line => line.Kind is "pos" or "decal coordinate")
                         .GroupBy(line => line.Kind == "pos" ? $"e{line.Uid}" : $"d{line.Text[..line.Text.IndexOf(':')]}"))
            {
                if (apartFrom != null && group.Key[0] == 'd' && apartFrom.Contains(int.Parse(group.Key.AsSpan(1), CultureInfo.InvariantCulture)))
                    continue;

                var pair = group.ToList();
                if (pair.Count != 2 || pair[0].Added == pair[1].Added)
                    return null;

                double[] Numbers(string text) => text[(text.IndexOf(':') + 1)..].Split(',').Select(part => double.Parse(part, CultureInfo.InvariantCulture)).ToArray();
                var (a, b) = (Numbers(pair[0].Text), Numbers(pair[1].Text));
                var by = Math.Max(Math.Abs(a[0] - b[0]), Math.Abs(a[1] - b[1]));
                if (by < 1e-3)
                    (close, closest) = (close + 1, Math.Max(closest, by));
                else
                    (apart, furthest) = (apart + 1, Math.Max(furthest, by));
            }

            return (close, closest, apart, furthest);
        }

        private void Move(string what, string tag, HashSet<(int X, int Y)> cells, int dx, int dy, Dictionary<(int X, int Y), TileOn> tilesBefore, Scene sceneBefore)
        {
            var current = session.Scene(_document);
            var places = PlacesOf(current.Grids.First(g => g.Uid == _grid));
            var on = On(current, _grid, cells);
            var plan = MovePlan(_grid, on, dx, dy);
            var covered = plan.Tiles.Keys.Where(cell => !cells.Contains(cell) && tilesBefore.ContainsKey(cell)).ToHashSet();
            var (delta, time) = Edit("Move region", plan.Ops);
            var moved = Save(tag);
            Info($"{what} (by {dx},{dy}): one edit of {plan.Ops.Count} ops ({plan.Tiles.Count} tiles written, {plan.Entities.Count} entities, {plan.Decals.Count} decals, {plan.Emptied.Count} cells emptied), {time.DocumentMs} ms in the document kernel; {covered.Count} of the cells it went to had a tile already");
            Kept(what, delta, plan);

            var differences = Lines(_before, moved);
            var wanted = on.Entities.Select(entity => entity.YamlUid).ToHashSet();
            var positions = differences.Lines.Where(line => line.Kind == "pos").Select(line => line.Uid).ToHashSet();
            // Where decals are kept on chunk entities, their lines are left out and checked apart.
            var decalLines = Chunked ? 0 : on.Decals.Count;
            if (differences.Only("tile chunk", "pos", "decal coordinate", "air")
                && positions.SetEquals(wanted) && differences.Removed("pos") == wanted.Count && differences.Added("pos") == wanted.Count
                && differences.Removed("decal coordinate") == decalLines && differences.Added("decal coordinate") == decalLines)
            {
                var decalsSaid = Chunked
                    ? "and but for the decals' chunk entities"
                    : $"and {on.Decals.Count} decal coordinate lines, one for each decal{(Strangers(on) > 0 ? $" ({Strangers(on)} of them of a kind the game does not have)" : "")}, and";
                Ok($"{what}: the saved file differs in tile chunk lines ({differences.Count("tile chunk")}), air lines ({differences.Count("air")}), {wanted.Count} position lines, one for each entity moved, {decalsSaid} in nothing else ({_before.Lines.Length} lines)");
            }
            else
            {
                Fail($"{what}: the saved file differs in {differences.Describe()}; {wanted.Count} entities and {on.Decals.Count} decals were moved, and position lines changed for {positions.Count} entities ({positions.Count(uid => !wanted.Contains(uid))} of them not moved)");
                foreach (var line in differences.Examples(kind => kind is not ("tile chunk" or "pos" or "decal coordinate" or "air"), 10))
                    say(line);
            }

            DecalsAsGame(what, _before, moved, plan.Ops, places);
            AirCarried(what, _before, moved, plan);

            Undo();
            Equal(Save($"{tag}.undone"), _before, $"{what}, undone", "the first save");
            Redo();
            Equal(Save($"{tag}.redone"), moved, $"{what}, redone", "the edited save");

            // Back again, as a second edit built from the scene as it now is.
            var there = plan.Tiles.Keys.ToHashSet();
            var movedScene = session.Scene(_document);
            var back = MovePlan(_grid, On(movedScene, _grid, there), -dx, -dy);
            Edit("Move region", back.Ops);
            var backSaved = Save($"{tag}.back");
            // A decal that left its chunk and came back has the id the chunk gave it then.
            DecalsAsGame($"{what}, then moved back as a second edit", moved, backSaved, back.Ops, PlacesOf(movedScene.Grids.First(g => g.Uid == _grid)));
            var returned = Lines(_before, backSaved);
            var airLines = returned.Lines.Count(line => line.Kind == "air");
            if (airLines > 0 && covered.Count == 0 && SameAir(_before, backSaved))
            {
                Found($"{what}, then moved back as a second edit: expected the first save. Every cell stores the air it did, but {airLines} air lines differ in their order. A cell whose air went and came back is held after the other cells of its chunk of four by four, and a chunk whose air all went after the other chunks, and the save writes chunks and mixes in the order they are held. Only undo puts them back in their place");
                returned = returned.Except("air");
            }

            if (returned.None)
            {
                Ok($"{what}, then moved back as a second edit: the save is the first save");
            }
            else if (returned.Only("pos", "decal coordinate") && Shifts(returned) is { Apart: 0 } shifts)
            {
                Found($"{what}, then moved back as a second edit: expected the first save. {shifts.Close} positions that are not on a half cell differ in their last digits (by at most {shifts.Closest:0.#######}): a position is a 32 bit number, and the sum there and back does not always give the same one. Nothing else differs");
                foreach (var line in returned.Examples(_ => true, 4))
                    say(line);
            }
            else if (covered.Count > 0)
            {
                var stood = sceneBefore.Entities.Count(entity => entity.Parent == _grid && !entity.Contained && covered.Contains(Cell(entity.X, entity.Y)));
                Found($"{what}, then moved back as a second edit: expected the first save. The move wrote over {covered.Count} cells that had a tile, with {stood} entities standing on them, and moving back leaves those cells empty: only undo brings back what a move covered. The saves differ in {returned.Describe()}");
                foreach (var line in returned.Examples(kind => kind is not ("tile chunk" or "pos"), 6))
                    say(line);
            }
            else
            {
                Fail($"{what}, then moved back as a second edit: the save is not the first save. They differ in {returned.Describe()}");
                foreach (var line in returned.Examples(_ => true, 8))
                    say(line);
            }

            Undo();
            Reopened(what, tag, moved, sceneBefore.Entities.Count);
        }

        /// <summary>Whatever was only half true in the file would show when it is read again.</summary>
        private void Reopened(string what, string tag, Saved saved, int entities)
        {
            var file = $"{_name}.{tag}.yml";
            var reopened = Opened(CheckTimes.Reopen, File.ReadAllText(Path.Combine(outFolder, file)), file);
            try
            {
                var again = Save(reopened.Document.Id, $"{tag}.again");
                if (reopened.Entities.Count == entities && Same(saved, again))
                {
                    Ok($"{what}, opened again: it has the {entities} entities expected, and saved again it is the same file");
                    return;
                }

                var differences = Compare(saved, again);
                Fail($"{what}, opened again: {reopened.Entities.Count} entities ({entities} expected); saved again it differs in {differences.Describe()}");
                foreach (var line in differences.Examples(_ => true, 8))
                    say(line);
            }
            finally
            {
                session.Close(reopened.Document.Id);
            }
        }

        private void Delete(Scene scene, HashSet<(int X, int Y)> cells)
        {
            var current = session.Scene(_document);
            var places = PlacesOf(current.Grids.First(g => g.Uid == _grid));
            var on = On(current, _grid, cells);
            var plan = DeletePlan(_grid, on);
            var (delta, time) = Edit("Delete region", plan.Ops);
            var deleted = Save("deleted");

            // Everything inside a deleted entity goes with it.
            var named = on.Entities.Select(entity => entity.Uid).ToHashSet();
            var gone = new HashSet<int>(named);
            for (var grew = true; grew;)
            {
                grew = false;
                foreach (var entity in scene.Entities)
                    grew |= gone.Contains(entity.Parent) && gone.Add(entity.Uid);
            }

            var held = gone.Count - named.Count;
            var byUid = scene.Entities.ToDictionary(entity => entity.Uid);
            Info($"deleted: one edit of {plan.Ops.Count} ops ({named.Count} entities holding {held} more, {on.Decals.Count} decals, {plan.Emptied.Count} cells emptied), {time.DocumentMs} ms in the document kernel");
            Expect(delta.Created.Count == 0 && gone.SetEquals(delta.Removed),
                $"deleted: the delta removes the {named.Count} entities the ops named and the {held} things they held, and nothing else",
                $"deleted: the delta removes {delta.Removed.Count} entities ({delta.Removed.Count(uid => !gone.Contains(uid))} of them neither named nor held by one that was) and creates {delta.Created.Count}. {gone.Count} were expected");
            foreach (var uid in delta.Removed.Where(uid => !gone.Contains(uid)).Take(4))
                say($"        not the region's: {(byUid.TryGetValue(uid, out var other) ? $"{other.Prototype} {uid}, {Place(other)}" : $"{uid}, which the scene never had")}");

            var wrong = Wrong(plan, out _);
            var now = session.Scene(_document);
            var decalsLeft = now.Grids.First(g => g.Uid == _grid).Decals.Count(decal => on.Decals.Any(was => was.Id == decal.Id));
            Expect(wrong.Count == 0 && decalsLeft == 0 && delta.Entities.Count == 0,
                $"deleted: the region's {plan.Emptied.Count} cells are space, its decals are gone, and the delta moved nothing else",
                $"deleted: {wrong.Count} cells are not space, {decalsLeft} decals are left, and the delta reports {delta.Entities.Count} other entities changed{(delta.Entities.Count > 0 ? $", the first {delta.Entities[0].Prototype} {delta.Entities[0].Uid}: {Place(delta.Entities[0])}" : "")}");

            // "Nothing else": the first save with those entities' blocks cut out of it must be
            // the new save but for tile chunks, decals and the count in the heading.
            var goneInFile = gone.Where(byUid.ContainsKey).Select(uid => byUid[uid].YamlUid).ToHashSet();
            var cut = _before.Without(goneInFile);
            var differences = Lines(cut, deleted);

            // What is left behind may have named what is gone: an alarm its devices, a device its
            // alarms, a button what it is linked to. The kernel takes those names out, as the game
            // does, and the lines of those lists are the only other lines that may differ.
            var keepers = differences.Lines.Where(line => line.Kind == "list of entities").Select(line => line.Uid).ToHashSet();
            var invalid = NewlyInvalid(deleted);
            Expect(differences.Only("tile chunk", "decal coordinate", "decal group", "entity count", "list of entities", "air")
                    && differences.Added("decal coordinate") == 0 && differences.Removed("decal coordinate") == (Chunked ? 0 : on.Decals.Count)
                    && invalid.Count == 0,
                $"deleted: the map saves, and the file is the first save without the {gone.Count} entities' own lines ({_before.Lines.Length - cut.Lines.Length}), but for tile chunk lines ({differences.Count("tile chunk")}), air lines ({differences.Count("air")}), {differences.Removed("decal coordinate")} decal lines gone with {differences.Removed("decal group")} lines of groups left empty, the entity count, and the lists of {keepers.Count} entities left behind that named a deleted one (lines {differences.Count("list of entities")}). No line reads \"invalid\" that did not before",
                $"deleted: against the first save without the {gone.Count} entities' own lines, the saved file differs in {differences.Describe()}, and {invalid.Count} lines read \"invalid\" that did not before{(invalid.Count > 0 ? $" (the first: {invalid[0]})" : "")}");
            foreach (var line in differences.Examples(kind => kind is "other" or "pos" or "rot" or "parent" or "group heading", 10))
                say(line);

            DecalsAsGame("deleted", _before, deleted, plan.Ops, places);
            AirCleared(deleted, plan);

            // A mirror keeps component data too, and has it right only if the delta brings it.
            var byYamlUid = scene.Entities.Where(entity => entity.YamlUid != 0).GroupBy(entity => entity.YamlUid).ToDictionary(group => group.Key, group => group.First().Uid);
            var listers = keepers.Where(byYamlUid.ContainsKey).Select(uid => byYamlUid[uid]).ToHashSet();
            bool Details(SceneDelta of) => listers.SetEquals(of.Details.Select(details => details.Uid));
            Expect(listers.Count == keepers.Count && Details(delta),
                $"deleted: the delta has the components of the {listers.Count} entities whose lists changed, and of no other",
                $"deleted: the lists of {keepers.Count} entities changed in the file, and the delta has the components of {delta.Details.Count} entities ({delta.Details.Count(details => listers.Contains(details.Uid))} of those)");

            Reopened("deleted", "deleted", deleted, scene.Entities.Count - gone.Count);

            var (undone, _) = Undo();
            Equal(Save("deleted.undone"), _before, "deleted, undone", "the first save");
            Expect(Details(undone),
                $"deleted, undone: the delta has the components of the same {listers.Count} entities again",
                $"deleted, undone: the delta has the components of {undone.Details.Count} entities, not of the {listers.Count} whose lists changed back");
            // A mirror kept from deltas has only what they name: all that the delete removed must come back.
            var back = undone.Entities.Select(entity => entity.Uid).ToHashSet();
            var (namedBack, heldBack) = (named.Count(back.Contains), gone.Count(uid => !named.Contains(uid) && back.Contains(uid)));
            var wasRemoved = delta.Removed.Where(byUid.ContainsKey).ToList();
            Expect(namedBack == named.Count && heldBack == held && wasRemoved.All(back.Contains) && undone.Removed.Count == 0,
                $"deleted, undone: the delta brings back all {named.Count} entities and the {held} things they held, each as the scene had it",
                $"deleted, undone: the delta brings back {namedBack} of the {named.Count} entities and {heldBack} of the {held} things they held, and removes {undone.Removed.Count}: a mirror kept from deltas has lost {wasRemoved.Count(uid => !back.Contains(uid))}");
            var changed = undone.Entities.Where(entity => byUid.TryGetValue(entity.Uid, out var was) && was != entity).ToList();
            if (changed.Count > 0)
                Fail($"deleted, undone: {changed.Count} entities come back other than they were, the first {changed[0].Prototype} {changed[0].Uid}: {Place(changed[0])}, was {Place(byUid[changed[0].Uid])}");

            var redone = Redo();
            Equal(Save("deleted.redone"), deleted, "deleted, redone", "the edited save");
            Expect(redone.Entities.Count == 0 && gone.SetEquals(redone.Removed) && Details(redone),
                $"deleted, redone: the delta removes the same {gone.Count} entities again, and has the components of the same {listers.Count}",
                $"deleted, redone: the delta removes {redone.Removed.Count} entities ({gone.Count(uid => !redone.Removed.Contains(uid))} of the {gone.Count} missing) and reports {redone.Entities.Count} changed");
        }

        /// <summary>The lines of a save that name an entity that is not there, each with how often it is written.</summary>
        private static Dictionary<string, int> Invalid(Saved saved)
        {
            var lines = new Dictionary<string, int>(StringComparer.Ordinal);
            for (var i = 0; i < saved.Lines.Length; i++)
            {
                var text = saved.Lines[i].Trim();
                if (text is "- invalid" or "invalid:" || text.EndsWith(": invalid", StringComparison.Ordinal))
                {
                    var line = $"uid {saved.Uid[i]}, {saved.Part[i]}: {text}";
                    lines[line] = lines.GetValueOrDefault(line) + 1;
                }
            }

            return lines;
        }

        /// <summary>The lines that read "invalid" in a save more often than in the first save.</summary>
        private List<string> NewlyInvalid(Saved saved)
        {
            var before = Invalid(_before);
            return Invalid(saved).Where(pair => pair.Value > before.GetValueOrDefault(pair.Key)).Select(pair => pair.Key).ToList();
        }

        /// <summary>
        /// One source linked to two sinks, and both sinks deleted, in one edit and in two. A source
        /// names its sinks as the keys of a dictionary (DeviceLinkSource.linkedPorts). If the two
        /// were left there, both keys would be written "invalid" and the save would fail.
        /// </summary>
        private void Links(Scene scene)
        {
            // The first save says who is linked to whom: each key under a source's linkedPorts is a sink.
            var sinksOf = new Dictionary<int, List<int>>();
            var listing = false;
            for (var i = 0; i < _before.Lines.Length; i++)
            {
                var line = _before.Lines[i];
                if (_before.Part[i] != "DeviceLinkSource")
                    listing = false;
                else if (line.Length > 6 && line.StartsWith("      ", StringComparison.Ordinal) && line[6] != ' ')
                    listing = line == "      linkedPorts:";
                else if (listing && line.StartsWith("        ", StringComparison.Ordinal) && line.EndsWith(':') && int.TryParse(line.AsSpan(8, line.Length - 9), out var sink))
                {
                    if (!sinksOf.TryGetValue(_before.Uid[i], out var sinks))
                        sinksOf[_before.Uid[i]] = sinks = new List<int>();
                    sinks.Add(sink);
                }
            }

            var byYamlUid = scene.Entities.Where(entity => entity.YamlUid != 0).GroupBy(entity => entity.YamlUid).ToDictionary(group => group.Key, group => group.First());
            var found = sinksOf
                .Where(pair => byYamlUid.ContainsKey(pair.Key))
                .Select(pair => (Source: byYamlUid[pair.Key], Sinks: pair.Value.Where(byYamlUid.ContainsKey).Select(uid => byYamlUid[uid]).Where(sink => !sink.Contained).ToList()))
                .Where(pair => pair.Sinks.Count >= 2)
                .OrderBy(pair => pair.Source.YamlUid)
                .FirstOrDefault();
            if (found.Source == null)
            {
                Skip($"two things one source is linked to, deleted: of the {sinksOf.Count} sources of the map none is linked to two");
                return;
            }

            var (source, first, second) = (found.Source, found.Sinks[0], found.Sinks[1]);
            var what = $"two things one source is linked to ({source.Prototype} {source.YamlUid} in the file, linked to {found.Sinks.Count}: {first.Prototype} {first.YamlUid} and {second.Prototype} {second.YamlUid}), deleted";

            // The source's own lines in a save, without those of the two sinks.
            var keys = new[] { $"{first.YamlUid}:", $"{second.YamlUid}:" };
            bool Names(Saved saved) => Enumerable.Range(0, saved.Lines.Length).Any(i => saved.Uid[i] == source.YamlUid && saved.Part[i] == "DeviceLinkSource" && keys.Contains(saved.Lines[i].Trim()));
            bool Told(SceneDelta delta) => delta.Details.Any(details => details.Uid == source.Uid && !details.Components.Contains($"\"{first.Uid}\":", StringComparison.Ordinal) && !details.Components.Contains($"\"{second.Uid}\":", StringComparison.Ordinal));
            bool Retold(SceneDelta delta) => delta.Details.Any(details => details.Uid == source.Uid && details.Components.Contains($"\"{first.Uid}\":", StringComparison.Ordinal) && details.Components.Contains($"\"{second.Uid}\":", StringComparison.Ordinal));

            Saved? Saves(string how, string tag)
            {
                var saved = Saving(_document);
                if (saved.Ok && saved.Yaml != null)
                    return Written(tag, saved.Yaml);

                Fail($"{what} {how}: the map does not save (\"{saved.Failure}\"{(saved.Log.Count > 0 ? $", and the engine's log says \"{saved.Log[0].Message.Split('\n')[0]}\"" : "")})");
                return null;
            }

            var (delta, _) = Edit("Delete", [new EditOp("delete", Uid: first.Uid), new EditOp("delete", Uid: second.Uid)]);
            var once = Saves("in one edit", "links");
            if (once != null)
            {
                var invalid = NewlyInvalid(once);
                Expect(invalid.Count == 0 && !Names(once) && Told(delta),
                    $"{what} in one edit: the map saves, no line of it reads \"invalid\" that did not before, the source names neither any more, and the delta says so with the source's components",
                    $"{what} in one edit: the map saves, but {invalid.Count} lines read \"invalid\" that did not before{(invalid.Count > 0 ? $" (the first: {invalid[0]})" : "")}, the source {(Names(once) ? "still names one" : "names neither")}, and the delta {(Told(delta) ? "has" : "does not have")} the source's components without them");
            }

            var (undone, _) = Undo();
            Equal(Save("links.undone"), _before, $"{what} in one edit, undone", "the first save");
            Expect(Retold(undone),
                $"{what} in one edit, undone: the delta has the source's components naming both again",
                $"{what} in one edit, undone: the delta does not have the source's components naming both again");
            var redone = Redo();
            var again = Saves("in one edit, redone", "links.redone");
            if (once != null && again != null)
                Equal(again, once, $"{what} in one edit, redone", "the edited save");
            Expect(Told(redone),
                $"{what} in one edit, redone: the delta has the source's components without them",
                $"{what} in one edit, redone: the delta does not have the source's components without them");
            Undo();

            Edit("Delete", [new EditOp("delete", Uid: first.Uid)]);
            Edit("Delete", [new EditOp("delete", Uid: second.Uid)]);
            var twice = Saves("in two edits", "links.twice");
            if (once != null && twice != null)
                Equal(twice, once, $"{what} in two edits", "the save of the one edit");
            Undo();
            Undo();
            Equal(Save("links.twice.undone"), _before, $"{what} in two edits, both undone", "the first save");
        }

        private void Turn(HashSet<(int X, int Y)> cells, int far, double px, double py)
        {
            // Out to where there is room, so that the cells it turns onto are free.
            var plan = MovePlan(_grid, On(session.Scene(_document), _grid, cells), far, 0);
            Edit("Move region", plan.Ops);
            var region = plan.Tiles.Keys.ToHashSet();
            px += far;

            // The rotation byte of a tile is rare on a map, so some are painted to be turned.
            var kind = turning.Order(StringComparer.Ordinal).FirstOrDefault();
            var painted = kind == null ? new List<(int X, int Y)>() : region.OrderBy(cell => cell.Y).ThenBy(cell => cell.X).Take(8).ToList();
            if (kind != null)
                Edit("Paint", painted.Select((cell, index) => TileOp(_grid, cell, new TileOn(kind, 0, index))).ToList());

            var start = Save("turn.start");
            var steps = 1 + (kind != null ? 1 : 0);
            Info($"turned: the region is first moved out by {far},0 and {painted.Count} of its tiles painted {kind} with rotation bytes 0 to 7. It is turned there, a quarter against the clock about {px},{py}, a cell's corner, onto cells that are free");

            var first = On(session.Scene(_document), _grid, region);
            static bool WholeCell(SceneDecal decal) => decal.X == MathF.Floor(decal.X) && decal.Y == MathF.Floor(decal.Y);
            var whole = first.Decals.Where(WholeCell).ToList();
            var others = first.Decals.Where(decal => !WholeCell(decal)).ToList();

            // The rule for a decal is stated for both directions: each must undo the other.
            var unlike = first.Decals.Count(decal =>
            {
                var (x, y) = (decal.X - MathF.Floor(decal.X), decal.Y - MathF.Floor(decal.Y));
                var there = TurnedOffset(x, y, clockwise: false);
                var back = TurnedOffset(there.X, there.Y, clockwise: true);
                var other = TurnedOffset(x, y, clockwise: true);
                var again = TurnedOffset(other.X, other.Y, clockwise: false);
                var by = MathF.Max(MathF.Max(MathF.Abs(back.X - x), MathF.Abs(back.Y - y)), MathF.Max(MathF.Abs(again.X - x), MathF.Abs(again.Y - y)));
                return WholeCell(decal) ? by != 0 : by > 1e-6f;
            });
            Expect(unlike == 0,
                $"turned: a decal's place in its cell, turned one way and then the other, is where it was, for all {first.Decals.Count} decals",
                $"turned: for {unlike} of {first.Decals.Count} decals the place in the cell, turned one way and then the other, is not where it was");

            var current = region;
            var cost = "";
            // The decal chunks the turns should leave, by the game's rules, turn by turn.
            var model = ModelOf(start, _gridYaml);
            // The decals that in some turn could not turn as one piece with the region, how many
            // in each turn, how many got no op at all, and how many were then on no tile.
            var fallen = new HashSet<int>();
            var falls = new List<int>();
            var (left, bare) = (0, 0);
            for (var turn = 1; turn <= 4; turn++)
            {
                var scene = session.Scene(_document);
                var on = On(scene, _grid, current);
                var turned = TurnPlan(_grid, on, TilesOf(scene.Grids.First(g => g.Uid == _grid)), px, py, turning);
                var places = PlacesOf(scene.Grids.First(g => g.Uid == _grid));
                if (Chunked)
                    model = AsGame(model, turned.Ops, _grid, new Dictionary<int, Place>(places));
                fallen.UnionWith(turned.Fallback);
                falls.Add(turned.Fallback.Count);
                left += on.Decals.Count - turned.Decals.Count;
                var (delta, time) = Edit("Turn region", turned.Ops);
                current = turned.Tiles.Keys.ToHashSet();
                var tilesNow = TilesOf(session.Scene(_document).Grids.First(g => g.Uid == _grid));
                bare += turned.Decals.Count(decal => !tilesNow.ContainsKey(Cell(decal.X, decal.Y)));
                if (turn > 1)
                    continue;

                var one = Save("turn.once");
                DecalsAsGame("turned once", start, one, turned.Ops, places);
                var differences = Lines(start, one);
                cost = differences.Describe();
                Info($"turned: one edit of {turned.Ops.Count} ops ({turned.Tiles.Count} tiles written, {turned.Entities.Count} entities moved and rotated, {turned.Decals.Count} decals, {turned.Emptied.Count} cells emptied), {time.DocumentMs} ms in the document kernel");
                Kept("turned once", delta, turned);
                var wanted = on.Entities.Select(entity => entity.YamlUid).ToHashSet();
                var touched = differences.Lines.Where(line => line.Kind is "pos" or "rot").Select(line => line.Uid).ToHashSet();
                Expect(differences.Only("tile chunk", "pos", "rot", "decal coordinate", "decal group", "air") && touched.IsSubsetOf(wanted),
                    $"turned once: the saved file differs only in tile chunk lines, position and rotation lines of the entities turned, decal lines and air lines ({differences.Describe()})",
                    $"turned once: the saved file differs in {differences.Describe()}; position or rotation lines changed for {touched.Count(uid => !wanted.Contains(uid))} entities that were not turned");
                foreach (var line in differences.Examples(k => k is "other" or "parent" or "group heading" or "entity count", 10))
                    say(line);
                AirCarried("turned once", start, one, turned);

                Undo();
                Equal(Save("turn.undone"), start, "turned once, undone", "the save before the turn");
                Redo();
                Equal(Save("turn.redone"), one, "turned once, redone", "the edited save");
            }

            Expect(bare == 0 && left == 0,
                $"turned four times: after every turn each of the {first.Decals.Count} decals has a tile under its corner. A decal turns as one piece with the region, {others.Count} of them not on whole cells. Where its corner would be on a cell with no tile it falls back onto the cell its own cell turns to: {string.Join(", ", falls)} decals in the four turns, {fallen.Count} different ones",
                $"turned four times: {bare} times a decal was sent to a cell with no tile, and {left} times one had no tile to go to and was left");

            var fourSaved = Save("turn.four");
            if (Chunked)
            {
                var chunksUnlike = Unlike(model, ModelOf(fourSaved, _gridYaml));
                Expect(chunksUnlike.Count == 0,
                    "turned four times: the save's decal chunks are what the game's decal system makes of the four turns",
                    $"turned four times: the save's decal chunks are not what the game's decal system makes of the four turns. {chunksUnlike.Count} things differ");
                foreach (var line in chunksUnlike.Take(6))
                    say($"        {line}");
            }

            var four = Lines(start, fourSaved);
            var airLines = four.Lines.Count(line => line.Kind == "air");
            if (airLines > 0)
            {
                Expect(SameAir(start, fourSaved),
                    $"turned four times: every cell stores the air it did before the turns. {airLines} air lines are in another order: a cell whose air went and came back is held after the others of its chunk, or with its chunk after the other chunks",
                    $"turned four times: the air is not as before the turns. {airLines} air lines differ");
                four = four.Except("air");
            }

            var angles = four.Lines.Count(line => line.Kind is "rot" or "decal group");
            Expect(angles == 0,
                $"turned four times, each angle counted in whole quarters and kept in the turn it is in: every rotation line and every decal's angle in the save is as before the turns. One turn changes {cost}",
                $"turned four times: the angles in the save are not as before the turns. The saves differ in {four.Describe()}");
            foreach (var example in four.Examples(k => k is "rot" or "decal group", 6))
                say(example);

            if (four.None)
            {
                Ok("turned four times: the save is the save before the turns");
            }
            else if (four.Only("pos", "rot", "decal coordinate", "decal group") && Shifts(four, fallen) is { Apart: 0 } shifts)
            {
                Found($"turned four times: expected the save before the turns. {shifts.Close} positions that are not on a half cell differ in their last digits (by at most {shifts.Closest:0.#######}): a position is a 32 bit number, and four turns do not always bring back the same one. No position differs by more, but for the {fallen.Count} decals that fell back onto a cell in some turn, which need not come back");
                foreach (var example in four.Examples(k => k is "pos" or "decal coordinate", 4))
                    say(example);
            }
            else
            {
                Fail($"turned four times: the save is not the save before the turns. They differ in {four.Describe()}");
                foreach (var example in four.Examples(k => k is not ("rot" or "decal group"), 8))
                    say(example);
            }

            var now = session.Scene(_document).Grids.First(g => g.Uid == _grid).Decals.ToDictionary(decal => decal.Id);
            // Where the game draws it may change on the way: a chunk gives a decal back another id.
            var notBack = whole.Count(decal => !now.TryGetValue(decal.Id, out var has) || has with { Order = decal.Order } != decal);
            var otherAngle = first.Decals.Count(decal => !now.TryGetValue(decal.Id, out var has) || has.Angle != decal.Angle);
            float Away(SceneDecal decal) => now.TryGetValue(decal.Id, out var has) ? MathF.Max(MathF.Abs(has.X - decal.X), MathF.Abs(has.Y - decal.Y)) : float.PositiveInfinity;
            var rigid = others.Where(decal => !fallen.Contains(decal.Id)).ToList();
            var largest = rigid.Select(Away).DefaultIfEmpty(0).Max();
            var furthest = first.Decals.Where(decal => fallen.Contains(decal.Id)).Select(Away).DefaultIfEmpty(0).Max();
            Expect(notBack == 0 && otherAngle == 0 && largest < 1e-4f,
                $"turned four times: the {whole.Count} decals on whole cells are back to the last digit, every decal has the very angle it had, and the {rigid.Count} others that turned as one piece with the region all four times are back to within {largest:0.#######} of a cell. The {fallen.Count} that fell back in some turn are up to {furthest:0.###} of a cell from where they were",
                $"turned four times: {notBack} of the {whole.Count} decals on whole cells are not as they were, {otherAngle} decals have another angle, and the {rigid.Count} others that turned as one piece with the region all four times are up to {largest:0.#######} of a cell from where they were");

            for (var turn = 0; turn < 4; turn++)
                Undo();

            Equal(Save("turn.end"), start, "every turn undone", "the save before the turns");
            for (var step = 0; step < steps; step++)
                Undo();
        }

        /// <summary>One tile emptied under something, with no other op: what the engine does by itself.</summary>
        private void Emptied(string what, SceneEntity? target, Scene scene)
        {
            if (target == null)
            {
                Skip($"a tile emptied under {what}: the region has none");
                return;
            }

            var cell = Cell(target.X, target.Y);
            var contents = scene.Entities.Where(entity => entity.Parent == target.Uid).ToList();
            var (delta, _) = Edit("Erase tile", [TileOp(_grid, cell, null)]);
            var saved = Save($"emptied.{what.Split(' ')[^1]}");
            var differences = Compare(_before, saved);
            var now = delta.Entities.FirstOrDefault(entity => entity.Uid == target.Uid);
            var others = delta.Entities.Where(entity => entity.Uid != target.Uid).ToList();
            var inFile = differences.Lines.Where(line => line.Uid != 0).Select(line => line.Uid).Distinct().Count();
            Info($"a tile emptied under {what} ({target.Prototype} at {cell.X},{cell.Y}, {Place(target)}{(contents.Count > 0 ? $", holding {contents.Count} things" : "")}): "
                + $"the delta says it is {(now == null ? "unchanged" : Place(now))}, and {others.Count} other entities with it ({others.Count(entity => entity.Parent == _mapUid)} now children of the map, {others.Count(entity => contents.Any(held => held.Uid == entity.Uid))} of them things it holds); it removes {delta.Removed.Count}");
            Info($"    the file: its transform was \"{_before.Transform(target.YamlUid)}\" and is \"{saved.Transform(target.YamlUid)}\". The save differs in {differences.Describe()}, for {inFile} entities, {differences.Lines.Count(line => contents.Any(held => held.YamlUid == line.Uid))} lines of them in things it holds");
            foreach (var line in differences.Examples(kind => kind is "other" or "group heading", 6))
                say(line);

            var (undone, _) = Undo();
            var back = undone.Entities.FirstOrDefault(entity => entity.Uid == target.Uid);
            Equal(Save("emptied.undone"), _before, $"a tile emptied under {what}, undone ({(back == null ? "the delta does not name it" : $"the delta says it is {Place(back)}")})", "the first save");
        }

        private void Anchored(List<SceneEntity> walls, (int X, int Y) space)
        {
            if (walls.Count == 0)
            {
                Skip("an anchored entity moved onto space: the region has no wall");
                return;
            }

            var wall = walls[0];
            var (delta, _) = Edit("Move", [new EditOp("move", Uid: wall.Uid, X: space.X + 0.5f, Y: space.Y + 0.5f)]);
            var now = delta.Entities.FirstOrDefault(entity => entity.Uid == wall.Uid);
            var saved = Save("anchored.space");
            Info($"an anchored entity ({wall.Prototype}) moved onto a space cell ({space.X},{space.Y}): the delta says it is {(now == null ? "unchanged" : Place(now))}. The file: \"{saved.Transform(wall.YamlUid)}\", and the save differs in {Compare(_before, saved).Describe()}");
            Undo();
            Equal(Save("anchored.undone"), _before, "an anchored entity moved onto space, undone", "the first save");

            var other = walls.Skip(1).FirstOrDefault();
            if (other == null)
            {
                Skip("an anchored entity moved onto an occupied cell: the region has no second wall");
                return;
            }

            (delta, _) = Edit("Move", [new EditOp("move", Uid: wall.Uid, X: other.X, Y: other.Y)]);
            now = delta.Entities.FirstOrDefault(entity => entity.Uid == wall.Uid);
            saved = Save("anchored.occupied");
            Info($"an anchored entity ({wall.Prototype}) moved onto the cell of another anchored entity ({other.Prototype} at {other.X},{other.Y}): the delta says it is {(now == null ? "unchanged" : Place(now))}, and names {delta.Entities.Count - (now == null ? 0 : 1)} others. The save differs in {Compare(_before, saved).Describe()}");
            Undo();
            Equal(Save("anchored.undone"), _before, "an anchored entity moved onto an occupied cell, undone", "the first save");
        }

        /// <summary>
        /// Decals placed, changed in every setting, moved into another chunk, and removed, each
        /// as one edit that is undone to the save before it and done again. The delta has each
        /// decal as it was sent. On a fork that keeps its decals on chunk entities the save is
        /// what the game's decal system makes of the ops: a decal placed where its chunk has
        /// none makes a chunk entity, a chunk left with none loses its entity, and ids freed in a
        /// chunk are given again, lowest first. On a fork that keeps them on the grid, the save
        /// has each decal under its number.
        /// </summary>
        private void Decals(SceneGrid grid, Dictionary<(int X, int Y), TileOn> tiles)
        {
            var kind = decalKinds.Contains("Arrows") ? "Arrows" : decalKinds.Order(StringComparer.Ordinal).FirstOrDefault();
            var other = decalKinds.Where(id => id != kind).Order(StringComparer.Ordinal).FirstOrDefault();
            var known = grid.Decals.Where(decal => decalKinds.Contains(decal.Prototype) && tiles.ContainsKey(Cell(decal.X, decal.Y))).ToList();
            if (kind == null || other == null || known.Count < 3)
            {
                Skip("decals placed, changed, moved and removed: the grid has fewer than three decals of a kind the game has");
                return;
            }

            EditOp Placed((int X, int Y) cell, float within) => new("decal", Grid: _grid, X: cell.X + within, Y: cell.Y + within, Prototype: kind, Color: "#FF0000FF", Rotation: Quarter, ZIndex: 2, Cleanable: true);
            EditOp Updated(SceneDecal decal) => DecalOp(_grid, decal, decal.X, decal.Y, decal.Angle);
            var busy = Cell(known[0].X, known[0].Y);
            var withDecals = grid.Decals.Select(decal => ChunkOf(decal.X, decal.Y)).ToHashSet();
            var fresh = tiles.Keys.Where(cell => !withDecals.Contains(ChunkOf(cell.X, cell.Y))).Order().ToList();

            var placing = new List<EditOp> { Placed(busy, 0), Placed(busy, 0.25f) };
            if (fresh.Count > 0)
                placing.Add(Placed(fresh[0], 0));
            else
                Skip("a decal placed on a chunk of the grid that has none: every chunk with a tile has decals");
            var was = DecalStep("decals placed", "decals.placed", _before, placing);

            // Every setting at once, where it is.
            var changed = known[1];
            was = DecalStep("a decal changed in kind, colour, angle, z-index and cleanable", "decals.changed", was,
                [Updated(changed with { Prototype = other, Color = "#00FF0080", Angle = changed.Angle + Quarter, ZIndex = changed.ZIndex + 3, Cleanable = !changed.Cleanable })]);

            // Into another chunk: one that had none before the decals placed, if there is one.
            var moving = known[2];
            var to = fresh.Count > 0 ? fresh[0] : tiles.Keys.Where(cell => ChunkOf(cell.X, cell.Y) != ChunkOf(moving.X, moving.Y)).Order().FirstOrDefault(Cell(moving.X, moving.Y));
            was = DecalStep("a decal moved into another chunk", "decals.moved", was,
                [Updated(moving with { X = to.X + 0.5f, Y = to.Y + 0.5f })]);

            // A chunk left with none: the one with the fewest decals.
            var now = GridNow();
            var fewest = now.Decals.GroupBy(decal => ChunkOf(decal.X, decal.Y)).OrderBy(group => group.Count()).ThenBy(group => group.Key).First();
            was = DecalStep($"the {fewest.Count()} decals of the chunk with the fewest removed", "decals.removed", was,
                fewest.Select(decal => new EditOp("decal.remove", Grid: _grid, Uid: decal.Id)).ToList());

            // Two removed from the chunk of the first decal and two placed there in one edit.
            var two = GridNow().Decals.Where(decal => ChunkOf(decal.X, decal.Y) == ChunkOf(busy.X, busy.Y)).OrderBy(decal => decal.Order).Take(2).ToList();
            DecalStep("two decals removed and two placed on their chunk", "decals.again", was,
                [.. two.Select(decal => new EditOp("decal.remove", Grid: _grid, Uid: decal.Id)), Placed(busy, 0.5f), Placed(busy, 0.125f)]);
        }

        /// <summary>
        /// One edit of decal ops: the delta has each decal as sent, the save is checked, and the
        /// edit is undone to the save before and done again.
        /// </summary>
        /// <returns>The save with the edit done.</returns>
        private Saved DecalStep(string what, string tag, Saved was, List<EditOp> ops)
        {
            var places = PlacesOf(GridNow());
            var (delta, _) = Edit("Decals", ops);
            var now = Save(tag);

            var told = delta.Decals.Where(change => change.Grid == _grid).ToLookup(change => change.Id);
            var created = new Queue<int>(delta.CreatedDecals);
            var wrong = 0;
            foreach (var op in ops)
            {
                var id = op.Kind == "decal" ? created.Count > 0 ? created.Dequeue() : -1 : op.Uid;
                var decal = told[id].LastOrDefault()?.Decal;
                if (op.Kind == "decal.remove" ? decal != null || !told[id].Any()
                    : decal == null || decal.Prototype != op.Prototype || decal.Color != op.Color || decal.Angle != op.Rotation || decal.ZIndex != op.ZIndex || decal.Cleanable != op.Cleanable || decal.X != op.X || decal.Y != op.Y)
                    wrong++;
            }

            Expect(wrong == 0,
                $"{what}: the delta has each of the {ops.Count} decals as sent",
                $"{what}: the delta has {wrong} of the {ops.Count} decals other than sent");
            DecalsAsGame(what, was, now, ops, places);
            DecalsSaved(what, now);

            Undo();
            Equal(Save($"{tag}.undone"), was, $"{what}, undone", "the save before");
            Redo();
            Equal(Save($"{tag}.redone"), now, $"{what}, redone", "the edited save");
            return now;
        }

        /// <summary>
        /// A map can hold a decal of a kind the game does not have. It can be moved and stays the
        /// kind it is. One cannot be made, and no decal can be made into one.
        /// </summary>
        private void Stranger(SceneGrid grid, Dictionary<(int X, int Y), TileOn> tiles)
        {
            var strangers = grid.Decals.Where(decal => !decalKinds.Contains(decal.Prototype) && tiles.ContainsKey(Cell(decal.X, decal.Y))).ToList();
            var known = grid.Decals.FirstOrDefault(decal => decalKinds.Contains(decal.Prototype) && tiles.ContainsKey(Cell(decal.X, decal.Y)));
            if (strangers.Count == 0 || known == null)
            {
                Skip("a decal of a kind the game does not have: the grid has none");
                return;
            }

            var stranger = strangers[0];
            var what = $"a decal of a kind the game does not have (\"{stranger.Prototype}\", one of {strangers.Count} on the grid)";
            string? Refused(EditOp op)
            {
                try
                {
                    Edit("Decal", [op]);
                    return null;
                }
                catch (Exception e)
                {
                    return e.Message;
                }
            }

            SceneDelta delta;
            try
            {
                (delta, _) = Edit("Move decal", [DecalOp(_grid, stranger, stranger.X, stranger.Y, stranger.Angle)]);
            }
            catch (Exception e)
            {
                Fail($"{what}, sent again as it is: the kernel refuses the edit (\"{e.Message}\")");
                return;
            }

            Expect(delta.Decals.Count == 1 && delta.Decals[0].Decal == stranger && Same(Save("stranger"), _before),
                $"{what}, sent again as it is: the edit is taken, the delta has the decal as it was, and the save is the first save",
                $"{what}, sent again as it is: the edit is taken, but the decal or the save is not as it was");
            Undo();

            var made = Refused(new EditOp("decal", Grid: _grid, X: stranger.X, Y: stranger.Y, Prototype: stranger.Prototype, Color: stranger.Color, Rotation: stranger.Angle, ZIndex: stranger.ZIndex, Cleanable: stranger.Cleanable));
            var into = Refused(DecalOp(_grid, known with { Prototype = stranger.Prototype }, known.X, known.Y, known.Angle));
            var other = Refused(DecalOp(_grid, stranger with { Prototype = "no such decal" }, stranger.X, stranger.Y, stranger.Angle));
            Expect(made != null && into != null && other != null && Same(Save("stranger.refused"), _before),
                $"a new decal of that kind, a decal of a kind the game has made into that kind, and that decal made into another kind the game does not have are each refused (\"{other}\"), and the save is the first save",
                $"of a new decal of that kind, a decal of a kind the game has made into that kind, and that decal made into another kind the game does not have, the kernel takes {new[] { made, into, other }.Count(refusal => refusal == null)}");
        }

        private void Large(Scene scene, SceneGrid grid, Dictionary<(int X, int Y), TileOn> tiles, int left, int bottom)
        {
            var cells = new HashSet<(int X, int Y)>();
            for (var x = left; x < left + 50; x++)
            for (var y = bottom; y < bottom + 50; y++)
            {
                if (tiles.ContainsKey((x, y)))
                    cells.Add((x, y));
            }

            const string what = "a region of 50 by 50 cells moved far";
            var on = On(scene, _grid, cells);
            var by = tiles.Keys.Max(cell => cell.X) + 8 - left;
            var plan = MovePlan(_grid, on, by, 0);
            var (delta, time) = Edit("Move region", plan.Ops);
            var moved = Save("large");
            Info($"a region of 50 by 50 cells from {left},{bottom} moved far: one edit of {plan.Ops.Count} ops ({plan.Tiles.Count} tiles written, {plan.Entities.Count} entities, {plan.Decals.Count} decals, {Strangers(on)} of them of a kind the game does not have, {plan.Emptied.Count} cells emptied), {time.DocumentMs} ms in the document kernel, {time.TotalMs} ms with the picture redrawn");
            Kept(what, delta, plan);

            // Too many lines differ for the saves to be compared whole. The decals are read out of each.
            if (Chunked)
            {
                DecalsAsGame(what, _before, moved, plan.Ops, PlacesOf(grid));
            }
            else
            {
                var (was, now) = (DecalLines(_before, grid.YamlUid), DecalLines(moved, grid.YamlUid));
                var sent = on.Decals.Select(decal => decal.Id).ToHashSet();
                var wrong = was.Keys.Count(id => !now.TryGetValue(id, out var has) || has.Group != was[id].Group || has.At != was[id].At != sent.Contains(id))
                    + now.Keys.Count(id => !was.ContainsKey(id));
                Expect(wrong == 0 && sent.All(was.ContainsKey),
                    $"{what}: in the saved file each of the {sent.Count} decals has other coordinates and is written under the kind, colour and angle it was, and none of the other {was.Count - sent.Count} decals differs",
                    $"{what}: in the saved file {wrong} decals are not as expected: a decal moved is to have other coordinates under the kind, colour and angle it had, and any other is to be as it was");
            }

            var (_, undo) = Undo();
            Equal(Save("large.undone"), _before, $"{what}, undone ({undo.DocumentMs} ms in the document kernel)", "the first save");
            Redo();
            Equal(Save("large.redone"), moved, $"{what}, redone", "the edited save");

            // A second edit that leaves whole chunks empty, and the two undone and redone in a row.
            var further = MovePlan(_grid, On(session.Scene(_document), _grid, plan.Tiles.Keys.ToHashSet()), 0, 64);
            Edit("Move region", further.Ops);
            var twice = Save("large.twice");
            Undo();
            Undo();
            Equal(Save("large.twice.undone"), _before, $"{what} and then 64 cells up, both undone", "the first save");
            Redo();
            Redo();
            Equal(Save("large.twice.redone"), twice, $"{what} and then 64 cells up, both redone", "the edited save");
            Undo();
            Equal(Save("large.once"), moved, $"{what} and then 64 cells up, the second undone again", "the save after the first move");
        }
    }
}
