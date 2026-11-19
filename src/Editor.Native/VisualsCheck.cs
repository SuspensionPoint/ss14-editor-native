using System.Buffers.Binary;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.RegularExpressions;
using Robust.Shared.Maths;
using SS14.EditorKernel;
using YamlDotNet.RepresentationModel;
using static SS14.EditorNative.CheckKit;

namespace SS14.EditorNative;

/// <summary>
/// Checks the looks the kernel works out the way the game's server would (Visuals.cs), and that
/// the view draws by them.
///
/// On each map given, every pipe's colour sent must match the colour the file saves. Every cable
/// whose neighbours hold only cables of its own kind, and no terminal, must join the neighbours
/// that hold one, checked as straight runs, corners and Ts. A terminal placed on a straight run
/// splits the two cables it sits between, and undo joins them again. A cable laid beside another
/// joins it, both looks change, and undo restores the first one's look. Then, with what lies under
/// the floor shown, a cable must be drawn with the state of its joins and a pipe in its saved
/// colour. On a cell with both a pipe and a cable, the cable is drawn over the pipe, as their
/// depths from before the floor was seen through say.
/// </summary>
public static partial class VisualsCheck
{
    private const string Terminal = "CableTerminal";

    /// <returns>0 if every map passed.</returns>
    public static int Run(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, Action<string> say)
    {
        var session = EngineSession.Booted(forkRepo, kernelPack);
        var code = Run(session, forkRepo, maps, say);
        CheckTimes.Report("visuals", say);
        CheckTimes.Summary(say);
        return code;
    }

    /// <summary>The check on an engine that is up already.</summary>
    public static int Run(EngineSession session, string forkRepo, IReadOnlyList<string> maps, Action<string> say) =>
        OnEach(maps, say, "Cables joined and pipes took their colours as the game's server would have them.", (map, tally) =>
        {
            Check(session, forkRepo, map, say, tally);
            return true;
        });

    private static void Check(EngineSession session, string forkRepo, string map, Action<string> say, Tally tally)
    {
        var yaml = File.ReadAllText(Path.Combine(forkRepo, "Resources", map));
        var scene = Opened(session, CheckTimes.Open(map), yaml, map);
        var doc = scene.Document.Id;
        try
        {
            var looks = scene.Visuals.ToDictionary(look => look.Uid);
            GridsSent(scene, yaml, say, tally);
            Colours(scene, yaml, looks, say, tally);
            var world = new World(scene, looks);
            Joins(world, say, tally);
            var pipes = new Pipes(world, PipeFactsOf(session, scene.Entities.Select(entity => entity.Prototype)), SavedLayers(yaml));
            Connectors(pipes, say, tally);
            LayersSent(pipes, say, tally);
            PlumbingConnectors(pipes, say, tally);
            // First, while the list of the open, with every texture, is the latest.
            Drawn(session, doc, world, pipes, say, tally);
            TerminalParts(session, doc, world, say, tally);
            Laid(session, doc, world, say, tally);
            PipeLaid(session, doc, pipes, say, tally);
            DuctLaid(session, doc, pipes, say, tally);
        }
        finally
        {
            session.ShowSubFloor(doc, false);
            session.Close(doc);
        }
    }

    // ---- grids ----

    /// <summary>
    /// Every grid the file lists is sent as a grid, the map itself too when it is one, as a
    /// planet's is. A grid left out has no tiles in the view, and nothing on it is anchored.
    /// </summary>
    private static void GridsSent(Scene scene, string yaml, Action<string> say, Tally tally)
    {
        var stream = new YamlStream();
        stream.Load(new StringReader(yaml));
        var root = (YamlMappingNode) stream.Documents[0].RootNode;
        if (!root.Children.TryGetValue("grids", out var listed) || listed is not YamlSequenceNode { Children.Count: > 0 } grids)
        {
            CheckSkips.Skip(say, "grids sent: the file lists no grids");
            return;
        }

        var inFile = grids.Children.Select(grid => int.Parse(((YamlScalarNode) grid).Value!)).ToHashSet();
        var sent = scene.Grids.Select(grid => grid.YamlUid).ToHashSet();
        var maps = root.Children.TryGetValue("maps", out var mapList) && mapList is YamlSequenceNode mapNodes
            ? mapNodes.Children.Select(node => int.Parse(((YamlScalarNode) node).Value!)).ToHashSet()
            : [];
        tally.Expect(inFile.SetEquals(sent),
            $"every grid the file lists is sent: {inFile.Count} listed{(inFile.Overlaps(maps) ? ", the map among them" : "")}, {sent.Count} sent{(inFile.SetEquals(sent) ? "" : $", missing {string.Join(", ", inFile.Except(sent))}")}");
    }

    // ---- pipe colours ----

    private static void Colours(Scene scene, string yaml, Dictionary<int, SceneVisuals> looks, Action<string> say, Tally tally)
    {
        var saved = SavedColours(yaml);
        if (saved.Count == 0)
        {
            CheckSkips.Skip(say, "pipe colours: the map saves none");
            return;
        }

        var wrong = new List<string>();
        var compared = 0;
        foreach (var entity in scene.Entities)
        {
            if (!saved.TryGetValue(entity.YamlUid, out var colour))
                continue;

            compared++;
            if (!looks.TryGetValue(entity.Uid, out var look) || look.PipeColor == null || Color.FromHex(look.PipeColor) != Color.FromHex(colour))
                wrong.Add($"{entity.Prototype} {entity.YamlUid}: saved {colour}, sent {(looks.TryGetValue(entity.Uid, out var sent) ? sent.PipeColor : "nothing")}");
        }

        tally.Expect(compared == saved.Count && wrong.Count == 0,
            $"the colour sent of each of {compared} pipes is its saved colour (of {saved.Count} saved){(wrong.Count > 0 ? ": " + string.Join("; ", wrong.Take(4)) : "")}");
    }

    /// <summary>The AtmosPipeColor of each entity of the file that saves one, by its uid in the file.</summary>
    private static Dictionary<int, string> SavedColours(string yaml)
    {
        var stream = new YamlStream();
        stream.Load(new StringReader(yaml));
        var root = (YamlMappingNode) stream.Documents[0].RootNode;
        var colours = new Dictionary<int, string>();
        foreach (var entity in ((YamlSequenceNode) root["entities"]).Children
                     .SelectMany(prototype => ((YamlSequenceNode) prototype["entities"]).Children.Cast<YamlMappingNode>()))
        {
            if (!entity.Children.TryGetValue("components", out var components))
                continue;

            foreach (var component in ((YamlSequenceNode) components).Children.Cast<YamlMappingNode>())
            {
                if (((YamlScalarNode) component["type"]).Value == "AtmosPipeColor" && component.Children.TryGetValue("color", out var colour))
                    colours[int.Parse(((YamlScalarNode) entity["uid"]).Value!)] = ((YamlScalarNode) colour).Value!;
            }
        }

        return colours;
    }

    // ---- cable joins ----

    /// <summary>What stands on each cell of each grid, as the scene says.</summary>
    private sealed class World
    {
        public readonly Dictionary<(int Grid, Vector2i Cell), List<SceneEntity>> Cells = new();
        public readonly Dictionary<int, HashSet<Vector2i>> Tiles = new();
        public readonly Dictionary<int, SceneVisuals> Looks;
        public readonly List<SceneEntity> Cables = new();
        public readonly IReadOnlyList<SceneGrid> Grids;

        public World(Scene scene, Dictionary<int, SceneVisuals> looks)
        {
            Looks = looks;
            Grids = scene.Grids;
            var grids = scene.Grids.Select(grid => grid.Uid).ToHashSet();
            foreach (var grid in scene.Grids)
            {
                var tiles = Tiles[grid.Uid] = new HashSet<Vector2i>();
                for (var i = 0; i + 4 < grid.Tiles.Count; i += 5)
                    tiles.Add(new Vector2i(grid.Tiles[i], grid.Tiles[i + 1]));
            }

            foreach (var entity in scene.Entities)
            {
                if (!entity.Anchored || !grids.Contains(entity.Parent))
                    continue;

                var key = (entity.Parent, CellOf(entity));
                if (!Cells.TryGetValue(key, out var here))
                    Cells[key] = here = new List<SceneEntity>();
                here.Add(entity);
                if (looks.TryGetValue(entity.Uid, out var look) && look.CableMask != null)
                    Cables.Add(entity);
            }
        }

        public static Vector2i CellOf(SceneEntity entity) => new((int) MathF.Floor(entity.X), (int) MathF.Floor(entity.Y));

        public IReadOnlyList<SceneEntity> On(int grid, Vector2i cell) => Cells.TryGetValue((grid, cell), out var here) ? here : [];

        public bool IsCable(SceneEntity entity) => Looks.TryGetValue(entity.Uid, out var look) && look.CableMask != null;

        /// <summary>
        /// The joins a cable must have, told from the neighbours alone: a bit for each side
        /// whose cell holds a cable of its prototype. Null if that cannot be told so plainly:
        /// a terminal on its cell or beside it, or a cable of another prototype beside it,
        /// which may be of its network or not.
        /// </summary>
        public int? Expected(SceneEntity cable, int? extraFrom = null)
        {
            var cell = CellOf(cable);
            var mask = 0;
            if (On(cable.Parent, cell).Any(entity => entity.Prototype == Terminal))
                return null;

            foreach (var (bit, offset) in Sides)
            {
                var here = On(cable.Parent, cell + offset);
                if (here.Any(entity => entity.Prototype == Terminal || IsCable(entity) && entity.Prototype != cable.Prototype))
                    return null;
                if (here.Any(entity => entity.Prototype == cable.Prototype))
                    mask |= bit;
            }

            return mask;
        }
    }

    private static readonly (int Bit, Vector2i Offset)[] Sides = [(1, new Vector2i(0, 1)), (2, new Vector2i(0, -1)), (4, new Vector2i(1, 0)), (8, new Vector2i(-1, 0))];

    private static int Bit(Vector2i offset) => Sides.First(side => side.Offset == offset).Bit;

    private static void Joins(World world, Action<string> say, Tally tally)
    {
        if (world.Cables.Count == 0)
        {
            CheckSkips.Skip(say, "cable joins: the map has no cables");
            return;
        }

        var kinds = new Dictionary<string, int> { ["straight"] = 0, ["corner"] = 0, ["T"] = 0, ["other"] = 0 };
        var wrong = new List<string>();
        foreach (var cable in world.Cables)
        {
            if (world.Expected(cable) is not { } expected)
                continue;

            var kind = expected switch
            {
                3 or 12 => "straight",
                5 or 6 or 9 or 10 => "corner",
                7 or 11 or 13 or 14 => "T",
                _ => "other",
            };
            kinds[kind]++;
            var sent = world.Looks[cable.Uid].CableMask;
            if (sent != expected)
                wrong.Add($"{cable.Prototype} {cable.Uid} at {World.CellOf(cable)}: expected {expected}, sent {sent}");
        }

        tally.Expect(wrong.Count == 0,
            $"every plain cable joins its neighbours: {kinds["straight"]} straight, {kinds["corner"]} corners, {kinds["T"]} Ts, {kinds["other"]} others, of {world.Cables.Count} cables{(wrong.Count > 0 ? ": " + string.Join("; ", wrong.Take(4)) : "")}");
        foreach (var kind in new[] { "straight", "corner", "T" })
        {
            if (kinds[kind] == 0)
                CheckSkips.Skip(say, $"cable joins: no plain {kind} on the map");
        }
    }

    /// <summary>A plain straight run: a cable and the cable beside it that it joins.</summary>
    private static (SceneEntity Cable, SceneEntity Next, Vector2i Offset)? StraightRun(World world)
    {
        foreach (var cable in world.Cables)
        {
            if (world.Expected(cable) is not (3 or 12) || world.Looks[cable.Uid].CableMask is not (3 or 12))
                continue;

            var offset = world.Looks[cable.Uid].CableMask == 3 ? new Vector2i(0, 1) : new Vector2i(1, 0);
            var next = world.On(cable.Parent, World.CellOf(cable) + offset).FirstOrDefault(entity => entity.Prototype == cable.Prototype);
            if (next != null && world.Expected(next) != null)
                return (cable, next, offset);
        }

        return null;
    }

    private static int? MaskIn(SceneDelta delta, int uid) => delta.Visuals.FirstOrDefault(look => look.Uid == uid)?.CableMask;

    private static void TerminalParts(EngineSession session, int doc, World world, Action<string> say, Tally tally)
    {
        if (StraightRun(world) is not { } run)
        {
            CheckSkips.Skip(say, "a terminal on a cable: no plain straight run");
            return;
        }

        var (cable, next, offset) = run;
        // A terminal faces the way its rotation turns south (CableNode.cs:85-101).
        var facing = offset == new Vector2i(0, 1) ? Direction.North : Direction.East;
        var cell = World.CellOf(cable);
        var before = (world.Looks[cable.Uid].CableMask!.Value, world.Looks[next.Uid].CableMask!.Value);
        var placed = Edit(session, doc, "Terminal", new EditOp("spawn", Prototype: Terminal, Parent: cable.Parent, X: cell.X + 0.5f, Y: cell.Y + 0.5f, Rotation: facing.ToAngle().Theta));
        var parted = (MaskIn(placed, cable.Uid), MaskIn(placed, next.Uid));
        tally.Expect(parted == (before.Item1 & ~Bit(offset), before.Item2 & ~Bit(-offset)),
            $"a terminal on {cable.Prototype} {cable.Uid} facing its neighbour parts the two: {before} became {parted}");

        var undone = Undo(session, doc);
        tally.Expect((MaskIn(undone, cable.Uid), MaskIn(undone, next.Uid)) == (before.Item1, before.Item2),
            $"undo joins them again: {(MaskIn(undone, cable.Uid), MaskIn(undone, next.Uid))}");
    }

    private static void Laid(EngineSession session, int doc, World world, Action<string> say, Tally tally)
    {
        foreach (var cable in world.Cables)
        {
            if (world.Expected(cable) is not { } mask || world.Looks[cable.Uid].CableMask != mask)
                continue;

            foreach (var (bit, offset) in Sides)
            {
                var cell = World.CellOf(cable) + offset;
                if ((mask & bit) != 0 || !world.Tiles[cable.Parent].Contains(cell) || world.On(cable.Parent, cell).Count > 0)
                    continue;

                // What the new cable must join, its neighbours told plainly.
                var expected = 0;
                var plain = true;
                foreach (var (otherBit, otherOffset) in Sides)
                {
                    var here = world.On(cable.Parent, cell + otherOffset);
                    plain &= !here.Any(entity => entity.Prototype == Terminal || world.IsCable(entity) && entity.Prototype != cable.Prototype);
                    if (here.Any(entity => entity.Prototype == cable.Prototype))
                        expected |= otherBit;
                }

                if (!plain)
                    continue;

                var laid = Edit(session, doc, "Cable", new EditOp("spawn", Prototype: cable.Prototype, Parent: cable.Parent, X: cell.X + 0.5f, Y: cell.Y + 0.5f));
                var made = laid.Created.Single();
                tally.Expect(MaskIn(laid, cable.Uid) == (mask | bit) && MaskIn(laid, made) == expected,
                    $"a {cable.Prototype} laid beside {cable.Uid} joins it: its joins {mask} became {MaskIn(laid, cable.Uid)}, the new one's are {MaskIn(laid, made)} of {expected}");

                var undone = Undo(session, doc);
                tally.Expect(MaskIn(undone, cable.Uid) == mask && undone.Removed.Contains(made),
                    $"undo takes the new cable away and gives back the joins {mask}: {MaskIn(undone, cable.Uid)}");
                return;
            }
        }

        CheckSkips.Skip(say, "a cable laid beside another: no plain cable with an empty floor beside it");
    }

    // ---- what the view draws ----

    private static void Drawn(EngineSession session, int doc, World world, Pipes pipes, Action<string> say, Tally tally)
    {
        // The textures of the whole list came with the list of the open. Each list after it
        // names only the new ones.
        var list = DrawList.Read(session, doc, null);
        TilesDrawn(session, world, list, say, tally);
        session.ShowSubFloor(doc, true);
        list = DrawList.Read(session, doc, list.Textures);

        var cable = world.Cables.FirstOrDefault(entity => world.Looks[entity.Uid].CableMask > 0 && list.Items.ContainsKey(entity.Uid));
        if (cable == null)
        {
            CheckSkips.Skip(say, "a cable drawn: none is drawn");
        }
        else
        {
            var mask = world.Looks[cable.Uid].CableMask;
            var states = list.Items[cable.Uid].Quads.Select(quad => list.Textures.GetValueOrDefault(quad.Texture)?.State).ToList();
            tally.Expect(states.Count > 0 && Regex.Match(states[0] ?? "", @"^[a-z]+_(\d+)$") is { Success: true } state && state.Groups[1].Value == mask.ToString(),
                $"{cable.Prototype} {cable.Uid} is drawn with the state of its joins {mask}: {string.Join(", ", states)}");
        }

        var pipe = list.Items.Keys.FirstOrDefault(uid => world.Looks.TryGetValue(uid, out var look) && look.PipeColor != null && Color.FromHex(look.PipeColor) != Color.White);
        if (pipe == 0)
        {
            CheckSkips.Skip(say, "a coloured pipe drawn: none is drawn");
        }
        else
        {
            var colour = Color.FromHex(world.Looks[pipe].PipeColor!);
            var tints = list.Items[pipe].Quads.Select(quad => quad.Color).ToList();
            tally.Expect(tints.Any(tint => Near(tint.R, colour.R) && Near(tint.G, colour.G) && Near(tint.B, colour.B)),
                $"pipe {pipe} is drawn in its saved colour {colour.ToHex()}: {string.Join(", ", tints.Select(tint => tint.ToHex()))}");
        }

        // A gas pipe with connectors draws one connector for each, as the client's
        // AtmosPipeAppearanceSystem shows them from the state.
        var joined = list.Items.Keys.FirstOrDefault(uid => world.Looks.TryGetValue(uid, out var look) && look.PipeState > 0
                                                           && world.Cells.Values.SelectMany(here => here).FirstOrDefault(entity => entity.Uid == uid) is { } entity
                                                           && pipes.Of(entity) is { Plumbing: false, Nodes.Count: 1 });
        if (joined == 0)
        {
            CheckSkips.Skip(say, "a pipe drawn with connectors: none is drawn");
        }
        else
        {
            var state = world.Looks[joined].PipeState!.Value;
            var states = list.Items[joined].Quads.Select(quad => list.Textures.GetValueOrDefault(quad.Texture)?.State).ToList();
            var connectors = states.Count(name => name?.StartsWith("pipeConnector", StringComparison.Ordinal) == true);
            tally.Expect(connectors == System.Numerics.BitOperations.PopCount((uint) state),
                $"pipe {joined} with the connectors {state} is drawn with {connectors} of them: {string.Join(", ", states)}");
        }

        LayerDrawn(list, pipes, say, tally);
        PlumbingLayer(list, pipes, say, tally);
        DisposalsLayer(list, world, say, tally);

        // A cell with a pipe and a cable, both drawn through the floor.
        foreach (var (key, here) in world.Cells)
        {
            var wire = here.FirstOrDefault(entity => world.IsCable(entity) && list.Items.ContainsKey(entity.Uid));
            var gas = here.FirstOrDefault(entity => world.Looks.TryGetValue(entity.Uid, out var look) && look.PipeColor != null && list.Items.ContainsKey(entity.Uid) && list.Items[entity.Uid].Kind == 4);
            if (wire == null || gas == null)
                continue;

            var (a, b) = (list.Items[gas.Uid], list.Items[wire.Uid]);
            var drawnOver = a.Depth == b.Depth && a.Order == b.Order && a.Sort == b.Sort && a.Tie < b.Tie;
            tally.Expect(drawnOver,
                $"on {key.Cell} the cable {wire.Uid} is drawn over the pipe {gas.Uid}: depths {a.Depth} and {b.Depth}, sort keys {a.Sort} and {b.Sort}, depths before {a.Tie} and {b.Tie}");
            return;
        }

        CheckSkips.Skip(say, "a pipe and a cable on one cell: none drawn");
    }

    /// <summary>
    /// Every tile of the map whose prototype has a picture is drawn with it. A tile the view
    /// does not know is left out of the list without a word, so a whole map can lose its floors.
    /// </summary>
    private static void TilesDrawn(EngineSession session, World world, DrawList list, Action<string> say, Tally tally)
    {
        using var known = JsonDocument.Parse(session.Command("tiles", "{}"));
        var sprites = known.RootElement.EnumerateArray()
            .Where(tile => tile.TryGetProperty("sprite", out var sprite) && sprite.ValueKind == JsonValueKind.String)
            .ToDictionary(tile => tile.GetProperty("id").GetString()!, tile => tile.GetProperty("sprite").GetString()!);

        var wanted = new Dictionary<string, int>();
        foreach (var grid in world.Grids)
        {
            for (var i = 0; i + 4 < grid.Tiles.Count; i += 5)
            {
                if (sprites.TryGetValue(grid.TileIds[grid.Tiles[i + 2]], out var sprite))
                    wanted[sprite] = wanted.GetValueOrDefault(sprite) + 1;
            }
        }

        if (wanted.Count == 0)
        {
            CheckSkips.Skip(say, "tiles drawn: the map has no tile with a picture");
            return;
        }

        var drawn = new Dictionary<string, int>();
        foreach (var item in list.Items.Values.Where(item => item.Kind == 1))
        foreach (var (texture, _) in item.Quads)
        {
            if (list.Textures.TryGetValue(texture, out var source))
                drawn[source.Path] = drawn.GetValueOrDefault(source.Path) + 1;
        }

        // Edges of a neighbour can use a tile's own picture too, so more is fine and fewer is not.
        var missing = wanted.Where(pair => drawn.GetValueOrDefault(pair.Key) < pair.Value).ToList();
        tally.Expect(missing.Count == 0,
            $"every one of {wanted.Values.Sum()} tiles with a picture is drawn, {drawn.Values.Sum()} tile images in all{(missing.Count > 0 ? ": " + string.Join("; ", missing.Take(4).Select(pair => $"{pair.Key} {drawn.GetValueOrDefault(pair.Key)} of {pair.Value}")) : "")}");
    }

    private static bool Near(float a, float b) => MathF.Abs(a - b) < 1.5f / 255;

    /// <summary>A draw-list message read back, enough to tell what each entity was drawn with.</summary>
    private sealed record DrawList(Dictionary<int, TextureSourceInfo> Textures, Dictionary<int, DrawItem> Items)
    {
        public static DrawList Read(EngineSession session, int doc, Dictionary<int, TextureSourceInfo>? known)
        {
            var (address, length) = session.LatestDrawList(doc);
            var bytes = new byte[length];
            Marshal.Copy(address, bytes, 0, length);
            var headerLength = BinaryPrimitives.ReadInt32LittleEndian(bytes.AsSpan(4));
            using var header = JsonDocument.Parse(bytes.AsMemory(8, headerLength));
            var textures = known ?? new Dictionary<int, TextureSourceInfo>();
            foreach (var texture in header.RootElement.GetProperty("textures").EnumerateArray())
            {
                var state = texture.GetProperty("state");
                int? next = texture.TryGetProperty("next", out var link) && link.ValueKind == JsonValueKind.Number ? link.GetInt32() : null;
                var delay = texture.TryGetProperty("delay", out var time) && time.ValueKind == JsonValueKind.Number ? time.GetSingle() : 0;
                textures[texture.GetProperty("id").GetInt32()] = new TextureSourceInfo(texture.GetProperty("path").GetString()!, state.ValueKind == JsonValueKind.Null ? null : state.GetString(), next, delay);
            }

            var items = new Dictionary<int, DrawItem>();
            var offset = 8 + headerLength;
            foreach (var item in header.RootElement.GetProperty("items").EnumerateArray())
            {
                var count = item[4].GetInt32();
                var quads = new List<(int, Color)>();
                for (var i = 0; i < count; i++)
                {
                    float At(int index) => BinaryPrimitives.ReadSingleLittleEndian(bytes.AsSpan(offset + index * 4));
                    quads.Add(((int) At(12), new Color(At(8), At(9), At(10), At(11))));
                    offset += 17 * 4;
                }

                var depth = item[1].GetInt32();
                items[item[0].GetInt32()] = new DrawItem(depth, item[2].GetInt32(), item[3].GetSingle(), item[5].GetInt32(), item.GetArrayLength() > 6 ? item[6].GetInt32() : depth, quads);
            }

            return new DrawList(textures, items);
        }
    }

    private sealed record TextureSourceInfo(string Path, string? State, int? Next = null, float Delay = 0);

    private sealed record DrawItem(int Depth, int Order, float Sort, int Kind, int Tie, List<(int Texture, Color Color)> Quads);
}
