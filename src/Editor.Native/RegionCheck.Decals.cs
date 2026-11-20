using System.Globalization;
using SS14.EditorKernel;

namespace SS14.EditorNative;

/// <summary>
/// Decals as a fork keeps them on chunk entities of the grid, as Space Wizards do: one entity
/// for each chunk of 16 by 16 tiles that has any, its decals each with an id of its own in the
/// chunk, and the highest id it has given. A save of such a fork is read here into a model of
/// the grid's chunks (ChunkModel), and the decal ops of an edit are played on the model of the
/// save before by the rules of the game's server DecalSystem (AsGame), to say what the save
/// after must hold. The other lines of the two saves are compared with the decals' storage
/// taken out (Plainly), as on a fork that keeps its decals on the grid. The chunk entities of
/// every other grid and map must be as they were (ChunkProblems).
/// </summary>
public static partial class RegionCheck
{
    /// <summary>Whether the fork keeps its decals on chunk entities.</summary>
    private static readonly bool Chunked = ChunkDecalLines.Chunked;

    /// <summary>The game's chunk size for chunk entities (ChunkEntitySystem.ChunkSize).</summary>
    private const int DecalChunkSize = 16;

    /// <summary>The highest id the server gives a decal in a chunk (DecalChunkComponent.MaxServerDecalId).</summary>
    private const int MaxServerDecalId = ushort.MaxValue - 256;

    /// <summary>A decal's chunk and its id there.</summary>
    private readonly record struct Place(int X, int Y, ushort Id)
    {
        public (int X, int Y) Chunk => (X, Y);

        public override string ToString() => $"{X},{Y}:{Id}";
    }

    /// <summary>The decal chunks of one grid in a save.</summary>
    private sealed class ChunkModel
    {
        /// <summary>The chunks that have a chunk entity with decals, each with the highest id it has given.</summary>
        public readonly Dictionary<(int X, int Y), int> Max = new();

        public readonly Dictionary<Place, ModelDecal> Decals = new();

        /// <summary>The chunks with more than one chunk entity with decals, once for each one after the first.</summary>
        public readonly List<(int X, int Y)> Doubled = new();

        public ChunkModel Clone()
        {
            var copy = new ChunkModel();
            foreach (var (chunk, max) in Max)
                copy.Max[chunk] = max;
            foreach (var (place, decal) in Decals)
                copy.Decals[place] = decal;
            copy.Doubled.AddRange(Doubled);
            return copy;
        }
    }

    /// <summary>The chunk a decal's corner is in (ChunkEntitySystem.GetChunkIndices).</summary>
    private static (int X, int Y) ChunkOf(float x, float y) =>
        ((int) MathF.Floor(x / DecalChunkSize), (int) MathF.Floor(y / DecalChunkSize));

    /// <summary>
    /// A decal's chunk and id from its place in the game's draw order, as the kernel packs them
    /// into SceneDecal.Order (Editor.Kernel/ChunkDecals.cs OrderOf).
    /// </summary>
    private static Place PlaceOf(long order) =>
        new((int) (order >> 39) - (1 << 22), (int) ((order >> 16) & ((1L << 23) - 1)) - (1 << 22), (ushort) (order & 0xFFFF));

    /// <summary>Each decal of the scene's grid by its number, with its chunk and id as the kernel says.</summary>
    private static Dictionary<int, Place> PlacesOf(SceneGrid grid) => grid.Decals.ToDictionary(decal => decal.Id, decal => PlaceOf(decal.Order));

    /// <summary>The decals a grid keeps itself (DecalGrid) in a save, by number.</summary>
    private static Dictionary<int, ModelDecal> GridDecals(Saved saved, int gridYamlUid) =>
        ChunkDecalLines.Decals(saved.Lines, "DecalGrid").Where(found => found.Uid == gridYamlUid).ToDictionary(found => (int) found.Id, found => found.Decal);

    /// <summary>
    /// The decal chunks of a grid as a save writes them. A chunk entity with no decals is no
    /// part of it: ChunkProblems tells of those.
    /// </summary>
    private static ChunkModel ModelOf(Saved saved, int gridYamlUid)
    {
        var model = new ChunkModel();
        foreach (var chunk in ChunkDecalLines.Chunks(saved.Lines))
        {
            if (chunk.Root != gridYamlUid || chunk.Empty)
                continue;

            if (!model.Max.TryAdd(chunk.Chunk, chunk.Max))
            {
                model.Doubled.Add(chunk.Chunk);
                model.Max[chunk.Chunk] = Math.Max(model.Max[chunk.Chunk], chunk.Max);
            }

            foreach (var (id, decal) in chunk.Decals)
                model.Decals[new Place(chunk.Chunk.X, chunk.Chunk.Y, id)] = decal;
        }

        return model;
    }

    /// <summary>
    /// What a grid's decal chunks hold after the decal ops of an edit, by the rules of the game's
    /// server DecalSystem, and one rule of the editor's own. By the game's: a decal changed
    /// within its chunk keeps its id there (ModifyDecal), one moved into another chunk leaves its
    /// own and is added to the other (SetDecalPosition), a new one goes into the chunk of its
    /// corner with the lowest id free up to the highest the chunk has given, or the next after it
    /// (TryAllocateDecalId), a chunk entity is made for a chunk that has none
    /// (GetOrCreateDecalChunk), and one left with no decals goes (DirtyChunk). By the editor's: a
    /// decal moved within its chunk keeps its id there too, where the game's SetDecalPosition
    /// would take it out and add it again (Editor.Kernel/ChunkDecals.cs says why).
    /// </summary>
    /// <param name="places">Each decal's chunk and id by its number before the edit. The decals the ops move are moved in it.</param>
    private static ChunkModel AsGame(ChunkModel before, IEnumerable<EditOp> ops, int grid, Dictionary<int, Place> places)
    {
        var model = before.Clone();
        foreach (var op in ops)
        {
            if (op.Grid != grid || op.Kind is not ("decal" or "decal.update" or "decal.remove"))
                continue;

            if (op.Kind == "decal.remove")
            {
                Take(model, places[op.Uid]);
                places.Remove(op.Uid);
                continue;
            }

            var decal = ModelDecal.From(op);
            var chunk = ChunkOf(op.X, op.Y);
            if (op.Kind == "decal.update" && places[op.Uid].Chunk == chunk)
            {
                // Changed in its chunk, moved within it or not: it keeps its id, the editor's rule for a move.
                model.Decals[places[op.Uid]] = decal;
                continue;
            }

            if (op.Kind == "decal.update")
                Take(model, places[op.Uid]);
            var place = Add(model, chunk, decal);
            if (op.Kind == "decal.update")
                places[op.Uid] = place;
        }

        return model;
    }

    private static void Take(ChunkModel model, Place place)
    {
        model.Decals.Remove(place);
        if (!model.Decals.Keys.Any(other => other.Chunk == place.Chunk))
            model.Max.Remove(place.Chunk);
    }

    private static Place Add(ChunkModel model, (int X, int Y) chunk, ModelDecal decal)
    {
        var max = model.Max.GetValueOrDefault(chunk);
        var id = 0;
        while (model.Decals.ContainsKey(new Place(chunk.X, chunk.Y, (ushort) id)) && id <= MaxServerDecalId)
            id++;

        var place = new Place(chunk.X, chunk.Y, (ushort) id);
        model.Decals[place] = decal;
        model.Max[chunk] = Math.Max(max, id);
        return place;
    }

    /// <summary>How two models of a grid's decal chunks differ, each difference said.</summary>
    private static List<string> Unlike(ChunkModel want, ChunkModel got)
    {
        var unlike = new List<string>();
        foreach (var chunk in got.Doubled.Distinct().Order())
            unlike.Add($"chunk {chunk.X},{chunk.Y} has {got.Doubled.Count(other => other == chunk) + 1} chunk entities with decals");
        foreach (var chunk in want.Max.Keys.Union(got.Max.Keys).OrderBy(chunk => chunk))
        {
            var (has, wanted) = (got.Max.TryGetValue(chunk, out var max), want.Max.TryGetValue(chunk, out var wantMax));
            if (has != wanted)
                unlike.Add($"chunk {chunk.X},{chunk.Y} {(has ? "has a chunk entity, and should have none" : "has no chunk entity")}");
            else if (max != wantMax)
                unlike.Add($"chunk {chunk.X},{chunk.Y} has given ids up to {max}, not {wantMax}");
        }

        foreach (var place in want.Decals.Keys.Union(got.Decals.Keys).OrderBy(place => (place.X, place.Y, place.Id)))
        {
            var (has, wanted) = (got.Decals.GetValueOrDefault(place), want.Decals.GetValueOrDefault(place));
            if (has != wanted)
                unlike.Add($"decal {place}: {(has == null ? "none" : has.ToString())}, not {(wanted == null ? "none" : wanted.ToString())}");
        }

        return unlike;
    }

    /// <summary>
    /// The kernel's numbers against a save: each decal of the scene is at the chunk and id its
    /// draw order says, as the save has it there, and the save has no decal the scene lacks.
    /// </summary>
    private static int Misplaced(SceneGrid grid, ChunkModel model)
    {
        var wrong = 0;
        var seen = new HashSet<Place>();
        foreach (var decal in grid.Decals)
        {
            var place = PlaceOf(decal.Order);
            if (!seen.Add(place) || model.Decals.GetValueOrDefault(place) != ModelDecal.Of(decal))
                wrong++;
        }

        return wrong + model.Decals.Keys.Count(place => !seen.Contains(place));
    }

    /// <summary>
    /// Which decal is which, told apart from the kernel's own places for them: each decal of the
    /// grid before the edit by its number, with the ops played on them, against the scene after
    /// by number, and against the decals of the save by what they are.
    /// </summary>
    /// <param name="created">The numbers the edit gave the decals it made, in the order of their ops.</param>
    /// <returns>How many decals of the scene are not what their number should be, and how many decals of the save are not what the ops leave.</returns>
    private static (int Numbers, int Contents) Identity(SceneGrid before, IEnumerable<EditOp> ops, IReadOnlyList<int>? created, SceneGrid now, ChunkModel saved)
    {
        var expected = before.Decals.ToDictionary(decal => decal.Id, ModelDecal.Of);
        var made = new Queue<int>(created ?? []);
        var unnumbered = new List<ModelDecal>();
        foreach (var op in ops)
        {
            if (op.Grid != before.Uid)
                continue;

            if (op.Kind == "decal.remove")
                expected.Remove(op.Uid);
            else if (op.Kind == "decal.update")
                expected[op.Uid] = ModelDecal.From(op);
            else if (op.Kind == "decal" && made.Count > 0)
                expected[made.Dequeue()] = ModelDecal.From(op);
            else if (op.Kind == "decal")
                unnumbered.Add(ModelDecal.From(op));
        }

        var shown = now.Decals.ToDictionary(decal => decal.Id, ModelDecal.Of);
        var numbers = expected.Count(pair => shown.GetValueOrDefault(pair.Key) != pair.Value)
            + shown.Count(pair => !expected.ContainsKey(pair.Key) && !unnumbered.Remove(pair.Value));

        var left = expected.Values.Concat(unnumbered).GroupBy(decal => decal).ToDictionary(group => group.Key, group => group.Count());
        var contents = 0;
        foreach (var decal in saved.Decals.Values)
        {
            if (left.TryGetValue(decal, out var count) && count > 0)
                left[decal] = count - 1;
            else
                contents++;
        }

        return (numbers, contents + left.Values.Sum());
    }

    /// <summary>What each decal of a grid's chunks is, but for where it is, in one order.</summary>
    private static List<string> Kinds(ChunkModel model) =>
        model.Decals.Values.Select(decal => $"{decal.Prototype} turned {decal.Angle.ToString("R", CultureInfo.InvariantCulture)}, colour {decal.Color ?? "none"}, z {decal.ZIndex}{(decal.Cleanable ? ", cleanable" : "")}").Order(StringComparer.Ordinal).ToList();

    /// <summary>A save without the decals' storage (ChunkDecalLines.Without), its lines' parts read again.</summary>
    private static Saved Plainly(Saved saved) => Saved.Of(ChunkDecalLines.Without(saved.Lines));
}

/// <summary>A decal as a save writes it, without its id.</summary>
public sealed record ModelDecal(string Prototype, string? Color, double Angle, int ZIndex, bool Cleanable, float X, float Y)
{
    public static ModelDecal Of(SceneDecal decal) => new(decal.Prototype, decal.Color, decal.Angle, decal.ZIndex, decal.Cleanable, decal.X, decal.Y);

    /// <summary>The decal an op of an edit sends, every field of it.</summary>
    public static ModelDecal From(EditOp op) => new(op.Prototype ?? "", op.Color, op.Rotation, op.ZIndex, op.Cleanable, op.X, op.Y);

    public override string ToString() =>
        $"{Prototype} at {X.ToString(CultureInfo.InvariantCulture)},{Y.ToString(CultureInfo.InvariantCulture)} turned {Angle.ToString(CultureInfo.InvariantCulture)}, colour {Color ?? "none"}, z {ZIndex}{(Cleanable ? ", cleanable" : "")}";
}

/// <summary>
/// The lines of a save that hold the decals of a fork that keeps them on chunk entities, for
/// the checks that compare saves line by line to leave out and check apart.
/// </summary>
public static class ChunkDecalLines
{
    /// <summary>Whether the fork keeps its decals on chunk entities (FORK_DECAL_CHUNK_ENTITIES).</summary>
#if FORK_DECAL_CHUNK_ENTITIES
    public static readonly bool Chunked = true;
#else
    public static readonly bool Chunked = false;
#endif

    /// <summary>One chunk entity of a save.</summary>
    /// <param name="Uid">Its number in the file.</param>
    /// <param name="Root">The number in the file of the grid or map it is a chunk of.</param>
    /// <param name="HasDecalChunk">Whether it has a DecalChunk component.</param>
    /// <param name="Max">The highest id its chunk has given (maxDecalId).</param>
    public sealed record SavedChunk(int Uid, int Root, (int X, int Y) Chunk, bool HasDecalChunk, int Max, List<(ushort Id, ModelDecal Decal)> Decals)
    {
        /// <summary>Whether it holds no decals, which the game does not leave.</summary>
        public bool Empty => !HasDecalChunk || Decals.Count == 0;
    }

    /// <summary>
    /// Goes through a save's lines with the file number of the entity and the kind of component
    /// each is in: "group" for a group's heading, "entity" for an entity's own lines before its
    /// components, and "" outside the entities.
    /// </summary>
    private static IEnumerable<(string Line, int Uid, string Part)> Parts(string[] lines)
    {
        var (uid, part) = (0, "");
        foreach (var line in lines)
        {
            if (line.StartsWith("- proto:", StringComparison.Ordinal) || line == "  entities:")
                (uid, part) = (0, "group");
            else if (line == "...")
                (uid, part) = (0, "");
            else if (line.StartsWith("  - uid: ", StringComparison.Ordinal))
                (uid, part) = (int.Parse(line.AsSpan(9), CultureInfo.InvariantCulture), "entity");
            else if (uid != 0 && line.StartsWith("    - type: ", StringComparison.Ordinal))
                part = line[12..].Trim();

            yield return (line, uid, part);
        }
    }

    /// <summary>
    /// The decals a save writes under one kind of component, as nodes of kind, colour and the
    /// rest, each with its decals by id and coordinates: a grid's DecalGrid, or a chunk
    /// entity's DecalChunk. Each with the file uid of the entity it is on.
    /// </summary>
    public static List<(int Uid, ushort Id, ModelDecal Decal)> Decals(string[] lines, string part)
    {
        var found = new List<(int Uid, ushort Id, ModelDecal Decal)>();
        var fields = new Dictionary<string, string>(StringComparer.Ordinal);
        var listing = false;
        foreach (var (line, uid, inPart) in Parts(lines))
        {
            if (inPart != part)
                continue;

            var text = line.Trim();
            if (text == "- node:" || text.StartsWith("- type:", StringComparison.Ordinal))
            {
                fields.Clear();
                listing = false;
            }
            else if (line == "          decals:")
            {
                listing = true;
            }
            else if (listing && line.StartsWith("            ", StringComparison.Ordinal) && text.Length > 0 && char.IsAsciiDigit(text[0]))
            {
                var colon = text.IndexOf(':');
                var at = text[(colon + 1)..].Trim().Split(',');
                var decal = new ModelDecal(
                    fields.GetValueOrDefault("id", ""),
                    fields.TryGetValue("color", out var color) ? color.Trim('\'') : null,
                    fields.TryGetValue("angle", out var angle) ? double.Parse(angle.Replace("rad", "").Trim(), CultureInfo.InvariantCulture) : 0,
                    fields.TryGetValue("zIndex", out var z) ? int.Parse(z, CultureInfo.InvariantCulture) : 0,
                    fields.TryGetValue("cleanable", out var cleanable) && bool.Parse(cleanable),
                    float.Parse(at[0], CultureInfo.InvariantCulture),
                    float.Parse(at[1], CultureInfo.InvariantCulture));
                found.Add((uid, ushort.Parse(text.AsSpan(0, colon), CultureInfo.InvariantCulture), decal));
            }
            else if (!listing && line.StartsWith("            ", StringComparison.Ordinal) && text.Contains(':'))
            {
                var colon = text.IndexOf(':');
                fields[text[..colon]] = text[(colon + 1)..].Trim();
            }
        }

        return found;
    }

    /// <summary>Every chunk entity of a save, of any grid or map, in the order the save has them.</summary>
    public static List<SavedChunk> Chunks(string[] lines)
    {
        var order = new List<int>();
        var roots = new Dictionary<int, int>();
        var chunks = new Dictionary<int, (int X, int Y)>();
        var maxes = new Dictionary<int, int>();
        var withDecals = new HashSet<int>();
        foreach (var (line, uid, part) in Parts(lines))
        {
            var text = line.Trim();
            if (part == "ChunkEntity" && text == "- type: ChunkEntity")
            {
                order.Add(uid);
            }
            else if (part == "ChunkEntity" && text.StartsWith("root:", StringComparison.Ordinal))
            {
                roots[uid] = int.Parse(text.AsSpan(5).Trim(), CultureInfo.InvariantCulture);
            }
            else if (part == "ChunkEntity" && text.StartsWith("chunk:", StringComparison.Ordinal))
            {
                var at = text[6..].Split(',');
                chunks[uid] = (int.Parse(at[0], CultureInfo.InvariantCulture), int.Parse(at[1], CultureInfo.InvariantCulture));
            }
            else if (part == "DecalChunk" && text == "- type: DecalChunk")
            {
                withDecals.Add(uid);
            }
            else if (part == "DecalChunk" && text.StartsWith("maxDecalId:", StringComparison.Ordinal))
            {
                maxes[uid] = int.Parse(text.AsSpan(11).Trim(), CultureInfo.InvariantCulture);
            }
        }

        var decals = Decals(lines, "DecalChunk").ToLookup(found => found.Uid);
        // A chunk at 0,0 is the field's default, which the save leaves out, and so is a highest id of 0.
        return order.Select(uid => new SavedChunk(uid, roots.GetValueOrDefault(uid), chunks.GetValueOrDefault(uid), withDecals.Contains(uid), maxes.GetValueOrDefault(uid),
            decals[uid].Select(found => (found.Id, found.Decal)).ToList())).ToList();
    }

    /// <summary>The chunk entities each grid or map lists as its own (ChunkContainer), by its number in the file.</summary>
    public static Dictionary<int, List<int>> Containers(string[] lines)
    {
        var listed = new Dictionary<int, List<int>>();
        foreach (var (line, uid, part) in Parts(lines))
        {
            if (part != "ChunkContainer")
                continue;

            if (!listed.TryGetValue(uid, out var list))
                listed[uid] = list = new List<int>();
            if (line.StartsWith("      - ", StringComparison.Ordinal))
                list.Add(int.Parse(line.AsSpan(8).Trim(), CultureInfo.InvariantCulture));
        }

        return listed;
    }

    /// <summary>
    /// The chunk entities of a save without their own numbers, which a save may give anew: for
    /// each, the grid or map it is a chunk of, its chunk, the highest id it has given and each
    /// of its decals by id. One line each, in one order, so two saves compare line by line.
    /// </summary>
    /// <param name="root">Which grids and maps, by their number in the file. All of them with none.</param>
    public static List<string> Model(string[] lines, Func<int, bool>? root = null) =>
        Chunks(lines)
            .Where(chunk => root == null || root(chunk.Root))
            .Select(chunk => $"root {chunk.Root}, chunk {chunk.Chunk.X},{chunk.Chunk.Y}: "
                + (chunk.HasDecalChunk ? $"highest id {chunk.Max}, decals {string.Join("; ", chunk.Decals.OrderBy(pair => pair.Id).Select(pair => $"{pair.Id}: {pair.Decal}"))}" : "no DecalChunk"))
            .Order(StringComparer.Ordinal)
            .ToList();

    /// <summary>
    /// A save without the decals' storage: the chunk entities, the lists of them their grids and
    /// maps keep (ChunkContainer), and their numbers among the file's entities out in nullspace
    /// and in the count in the heading. What is left compares line by line as a save of a fork
    /// that keeps its decals on the grid, but for its decals.
    /// </summary>
    public static string[] Without(string[] lines)
    {
        var chunkEntities = new HashSet<string>(StringComparer.Ordinal);
        var inGroup = false;
        foreach (var line in lines)
        {
            if (line.StartsWith("- proto:", StringComparison.Ordinal) || line == "...")
                inGroup = line == "- proto: ChunkEntity";
            else if (inGroup && line.StartsWith("  - uid: ", StringComparison.Ordinal))
                chunkEntities.Add(line[9..].Trim());
        }

        var kept = new List<string>(lines.Length);
        var (inContainer, entities) = (false, false);
        inGroup = false;
        for (var i = 0; i < lines.Length; i++)
        {
            var line = lines[i];
            if (line.StartsWith("- proto:", StringComparison.Ordinal) || line == "...")
                inGroup = line == "- proto: ChunkEntity";
            if (line == "entities:")
                entities = true;
            if (inGroup)
                continue;

            // A component's lines run from its "- type:" line to the next one or the next entity.
            if (entities && (line.StartsWith("    - type: ", StringComparison.Ordinal) || !line.StartsWith("      ", StringComparison.Ordinal)))
                inContainer = line.TrimEnd() == "    - type: ChunkContainer";
            if (inContainer)
                continue;

            if (!entities && (line == "nullspace:" || line == "nullspace: []"))
            {
                var others = new List<string>();
                while (i + 1 < lines.Length && lines[i + 1].StartsWith("- ", StringComparison.Ordinal))
                {
                    i++;
                    if (!chunkEntities.Contains(lines[i][2..].Trim()))
                        others.Add(lines[i]);
                }

                kept.Add(others.Count == 0 ? "nullspace: []" : "nullspace:");
                kept.AddRange(others);
                continue;
            }

            kept.Add(line);
        }

        for (var i = 0; i < kept.Count && i < 12; i++)
        {
            var text = kept[i].TrimStart();
            if (text.StartsWith("entityCount:", StringComparison.Ordinal))
                kept[i] = kept[i][..kept[i].IndexOf(':')] + ": " + (int.Parse(text.AsSpan(12).Trim(), CultureInfo.InvariantCulture) - chunkEntities.Count).ToString(CultureInfo.InvariantCulture);
        }

        return kept.ToArray();
    }
}
