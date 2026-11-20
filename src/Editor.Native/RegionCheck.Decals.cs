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
/// taken out (Plainly), as on a fork that keeps its decals on the grid.
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

    /// <summary>A decal as a save writes it.</summary>
    private sealed record ModelDecal(string Prototype, string? Color, double Angle, int ZIndex, bool Cleanable, float X, float Y)
    {
        public static ModelDecal Of(SceneDecal decal) => new(decal.Prototype, decal.Color, decal.Angle, decal.ZIndex, decal.Cleanable, decal.X, decal.Y);

        public override string ToString() =>
            $"{Prototype} at {X.ToString(CultureInfo.InvariantCulture)},{Y.ToString(CultureInfo.InvariantCulture)} turned {Angle.ToString(CultureInfo.InvariantCulture)}, colour {Color ?? "none"}, z {ZIndex}{(Cleanable ? ", cleanable" : "")}";
    }

    /// <summary>The decal chunks of one grid in a save.</summary>
    private sealed class ChunkModel
    {
        /// <summary>The chunks that have a chunk entity with decals, each with the highest id it has given.</summary>
        public readonly Dictionary<(int X, int Y), int> Max = new();

        public readonly Dictionary<Place, ModelDecal> Decals = new();

        /// <summary>How many chunk entities the save has with no decals, which the game would not leave.</summary>
        public int Empty;

        public ChunkModel Clone()
        {
            var copy = new ChunkModel { Empty = Empty };
            foreach (var (chunk, max) in Max)
                copy.Max[chunk] = max;
            foreach (var (place, decal) in Decals)
                copy.Decals[place] = decal;
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

    /// <summary>
    /// The decals a save writes under one kind of component, as nodes of kind, colour and the
    /// rest, each with its decals by id and coordinates: a grid's DecalGrid, or a chunk
    /// entity's DecalChunk. Each with the file uid of the entity it is on.
    /// </summary>
    private static List<(int Uid, ushort Id, ModelDecal Decal)> NodeDecals(Saved saved, string part)
    {
        var found = new List<(int Uid, ushort Id, ModelDecal Decal)>();
        var fields = new Dictionary<string, string>(StringComparer.Ordinal);
        var listing = false;
        for (var i = 0; i < saved.Lines.Length; i++)
        {
            if (saved.Part[i] != part)
                continue;

            var line = saved.Lines[i];
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
                found.Add((saved.Uid[i], ushort.Parse(text.AsSpan(0, colon), CultureInfo.InvariantCulture), decal));
            }
            else if (!listing && line.StartsWith("            ", StringComparison.Ordinal) && text.Contains(':'))
            {
                var colon = text.IndexOf(':');
                fields[text[..colon]] = text[(colon + 1)..].Trim();
            }
        }

        return found;
    }

    /// <summary>The decals a grid keeps itself (DecalGrid) in a save, by number.</summary>
    private static Dictionary<int, ModelDecal> GridDecals(Saved saved, int gridYamlUid) =>
        NodeDecals(saved, "DecalGrid").Where(found => found.Uid == gridYamlUid).ToDictionary(found => (int) found.Id, found => found.Decal);

    /// <summary>The decal chunks of a grid as a save writes them.</summary>
    private static ChunkModel ModelOf(Saved saved, int gridYamlUid)
    {
        var roots = new Dictionary<int, int>();
        var chunks = new Dictionary<int, (int X, int Y)>();
        var maxes = new Dictionary<int, int>();
        for (var i = 0; i < saved.Lines.Length; i++)
        {
            var text = saved.Lines[i].Trim();
            if (saved.Part[i] == "ChunkEntity" && text.StartsWith("root:", StringComparison.Ordinal))
            {
                roots[saved.Uid[i]] = int.Parse(text.AsSpan(5).Trim(), CultureInfo.InvariantCulture);
            }
            else if (saved.Part[i] == "ChunkEntity" && text.StartsWith("chunk:", StringComparison.Ordinal))
            {
                var parts = text[6..].Split(',');
                chunks[saved.Uid[i]] = (int.Parse(parts[0], CultureInfo.InvariantCulture), int.Parse(parts[1], CultureInfo.InvariantCulture));
            }
            else if (saved.Part[i] == "DecalChunk" && text.StartsWith("maxDecalId:", StringComparison.Ordinal))
            {
                maxes[saved.Uid[i]] = int.Parse(text.AsSpan(11).Trim(), CultureInfo.InvariantCulture);
            }
        }

        var decals = NodeDecals(saved, "DecalChunk").ToLookup(found => found.Uid);
        var model = new ChunkModel();
        foreach (var (uid, root) in roots)
        {
            if (root != gridYamlUid)
                continue;

            // A chunk at 0,0 is the field's default, which the save leaves out.
            var chunk = chunks.GetValueOrDefault(uid);

            if (!decals[uid].Any())
            {
                model.Empty++;
                continue;
            }

            model.Max[chunk] = maxes.GetValueOrDefault(uid);
            foreach (var (_, id, decal) in decals[uid])
                model.Decals[new Place(chunk.X, chunk.Y, id)] = decal;
        }

        return model;
    }

    /// <summary>
    /// What a grid's decal chunks hold after the decal ops of an edit, by the rules of the game's
    /// server DecalSystem: a decal changed within its chunk keeps its id there (ModifyDecal), one
    /// moved into another chunk leaves its own and is added to the other (SetDecalPosition), a
    /// new one goes into the chunk of its corner with the lowest id free up to the highest the
    /// chunk has given, or the next after it (TryAllocateDecalId), a chunk entity is made for a
    /// chunk that has none (GetOrCreateDecalChunk), and one left with no decals goes (DirtyChunk).
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

            var decal = new ModelDecal(op.Prototype ?? "", op.Color, op.Rotation, op.ZIndex, op.Cleanable, op.X, op.Y);
            var chunk = ChunkOf(op.X, op.Y);
            if (op.Kind == "decal.update" && places[op.Uid].Chunk == chunk)
            {
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
        if (got.Empty > 0)
            unlike.Add($"{got.Empty} chunk entities with no decals are saved");
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

    /// <summary>A save without the decals' storage (ChunkDecalLines.Without), its lines' parts read again.</summary>
    private static Saved Plainly(Saved saved) => Saved.Of(ChunkDecalLines.Without(saved.Lines));
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
