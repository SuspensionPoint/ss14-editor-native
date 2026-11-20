#if FORK_DECAL_CHUNK_ENTITIES
using Content.Shared.Decals;
using Robust.Shared.EntitySerialization.Components;
using Robust.Shared.GameObjects;
using Robust.Shared.GameStates;
using Robust.Shared.Maths;

namespace SS14.EditorKernel;

// Decals as the Space Wizards keep them, for a fork built with FORK_DECAL_CHUNK_ENTITIES
// (src/Directory.Build.props). A grid's decals are on chunk entities rooted on the grid
// (Robust.Shared/GameStates/ChunkEntitySystem.cs), one for each chunk of 16 by 16 tiles that has
// any. Each holds the decals of its chunk by an id of its own, and the highest id it has given
// (Content.Shared/Decals/DecalChunkComponent.cs).
//
// The game changes them through its decal system (Content.Server/Decals/DecalSystem.cs), which
// this kernel does not run. The lines here follow its rules:
//   - A new decal goes into the chunk entity of the chunk its corner is in, made if there is
//     none (GetOrCreateDecalChunk), with the lowest id the chunk has free, or the next after
//     the highest it has given (TryAllocateDecalId).
//   - A chunk entity left with no decals goes (DirtyChunk).
//   - A decal moved into another chunk leaves its own and is added to the other as a new one
//     there (SetDecalPosition). Any other change keeps it where it is (ModifyDecal).
//
// One rule is the editor's own. The game's SetDecalPosition takes a decal out and adds it again
// on any move, so a decal moved within its chunk gets the lowest free id there, and the chunk
// entity of a chunk with only that decal goes and is made anew. The editor keeps such a decal
// under its id, as a change of colour keeps it. A mapper's nudge then changes one line of the
// file and not the decal's id and its chunk entity's number, and the file is one the game loads
// just the same.
//   - As a map loads, each chunk's highest id is raised to its highest decal
//     (SharedDecalSystem.RebuildFreeDecalIds), and a grid that still keeps its decals the old
//     way, in its DecalGridComponent, has them moved onto chunk entities
//     (MigrateLegacyDecalGrid). A save of such a map has them on chunk entities, as the game's.
//
// The ids in a chunk change as decals go from one chunk to another, so the editor is given a
// number of its own for each decal (DecalNumbers). It stays the same for as long as the
// document is open, whichever chunk the decal is in.
public sealed partial class DocumentManager
{
    /// <summary>
    /// A decal changed, added or removed, with what undo needs to put its chunks back as they
    /// were, to the byte.
    /// </summary>
    /// <param name="From">Its chunk and its id there before, if it was there.</param>
    /// <param name="To">Its chunk and its id there after, if it is there. The same as From for a decal changed in its chunk.</param>
    /// <param name="ToMax">The highest id the chunk it went to had given before it came.</param>
    /// <param name="ToMade">Whether the chunk it went to had no decals of its own before: no chunk entity, or one without them.</param>
    /// <param name="ToEntityMade">Whether the chunk entity of the chunk it went to had to be made for it. One the file has with no decals is kept.</param>
    /// <param name="FromGone">The chunk it left, if that went for being empty: the highest id it had given and its number in the file (0 for none).</param>
    private sealed record DecalEdit(
        EntityUid Grid,
        uint Id,
        Decal? Old,
        Decal? New,
        DecalIndex? From,
        DecalIndex? To,
        ushort ToMax,
        bool ToMade,
        bool ToEntityMade,
        (ushort Max, int YamlUid)? FromGone) : Change;

    /// <summary>The editor's numbers for the decals of one grid, and where each decal is.</summary>
    private sealed class DecalNumbers
    {
        public readonly Dictionary<uint, DecalIndex> Places = new();
        public readonly Dictionary<DecalIndex, uint> Numbers = new();
        public uint Next;

        /// <summary>Puts a number at a place, or with no place takes it away.</summary>
        public void Set(uint number, DecalIndex? place)
        {
            if (Places.Remove(number, out var was) && Numbers.TryGetValue(was, out var there) && there == number)
                Numbers.Remove(was);

            if (place is not { } now)
                return;

            Places[number] = now;
            Numbers[now] = number;
        }
    }

    private readonly Dictionary<EntityUid, DecalNumbers> _decalNumbers = new();

    private ChunkEntitySystem? _chunkEntities;

    private ChunkEntitySystem ChunkEntities => _chunkEntities ??= _entMan.System<ChunkEntitySystem>();

    /// <summary>
    /// Where the game draws a decal among those of its z-index (SceneDecal.Order): by chunk x,
    /// then chunk y, then id (Content.Client/Decals/Overlays/DecalOverlay.cs CompareDecalIndex).
    /// Each chunk coordinate takes 23 bits, which is room for four million chunks either way.
    /// </summary>
    private static long OrderOf(DecalIndex place) =>
        ((long) (place.Chunk.X + (1 << 22)) << 39) | ((long) (place.Chunk.Y + (1 << 22)) << 16) | place.Id;

    /// <summary>
    /// What the game does to decals as a map loads (SharedDecalSystem.RebuildFreeDecalIds and the
    /// server's MigrateLegacyDecalGrid), on every grid of the map. A document gets it as it
    /// opens, and so does a map the kernel loads only to save again (EditorKernel.Resave).
    /// </summary>
    /// <remarks>
    /// After a grid's old decals are moved onto chunk entities, the game gives a new decal of
    /// such a chunk the id after the highest until the map is loaded again: its list of free ids
    /// is not saved, and is made only as a chunk starts, before the decals are moved in. The
    /// editor gives the lowest free id (FreeId), as the game does once the map is loaded again.
    /// Either way the file is one the game loads.
    /// </remarks>
    internal void StartDecals(EntityUid map)
    {
        bool OnMap(EntityUid uid) => uid == map || _entMan.GetComponent<TransformComponent>(uid).MapUid == map;

        var chunks = _entMan.AllEntityQueryEnumerator<ChunkEntityComponent, DecalChunkComponent>();
        while (chunks.MoveNext(out var chunk, out var decals))
        {
            if (!_entMan.EntityExists(chunk.Root) || !OnMap(chunk.Root))
                continue;

            foreach (var id in decals.Decals.Keys)
            {
                if (id <= DecalChunkComponent.MaxServerDecalId)
                    decals.MaxDecalId = Math.Max(decals.MaxDecalId, id);
            }
        }

#pragma warning disable CS0618 // The game's own migration reads the old component too.
        var legacy = new List<(EntityUid Grid, DecalGridComponent Decals)>();
        var grids = _entMan.AllEntityQueryEnumerator<DecalGridComponent>();
        while (grids.MoveNext(out var grid, out var decals))
        {
            if (OnMap(grid))
                legacy.Add((grid, decals));
        }

        foreach (var (grid, decals) in legacy)
        {
            foreach (var chunk in decals.ChunkCollection.ChunkCollection.Values)
            {
                foreach (var (id, decal) in chunk.Decals)
                {
                    var target = MakeChunk(grid, ChunkEntitySystem.GetChunkIndices(decal.Coordinates), out _, out _);
                    PutDecal(target, id, decal);
                }
            }

            _entMan.RemoveComponent<DecalGridComponent>(grid);
        }
#pragma warning restore CS0618
    }

    /// <summary>A grid's decals, each with its number, in the order the game draws them after z-index.</summary>
    private IEnumerable<SceneDecal> ChunkDecals(EntityUid grid)
    {
        var stored = Stored(grid);
        var numbers = Numbered(grid, stored);
        return stored.Select(pair => DescribeDecal(numbers.Numbers[pair.Place], pair.Decal, OrderOf(pair.Place))).ToList();
    }

    /// <summary>The grid's numbers, given the first time the grid is asked about.</summary>
    private DecalNumbers NumbersOf(EntityUid grid) =>
        _decalNumbers.TryGetValue(grid, out var numbers) ? numbers : Numbered(grid, Stored(grid));

    /// <summary>
    /// The grid's numbers made to fit its decals: any decal without one gets the next, in the
    /// order the game draws them, and a number whose decal is gone is dropped.
    /// </summary>
    private DecalNumbers Numbered(EntityUid grid, List<(DecalIndex Place, Decal Decal)> stored)
    {
        if (!_decalNumbers.TryGetValue(grid, out var numbers))
            _decalNumbers[grid] = numbers = new DecalNumbers();

        foreach (var (place, _) in stored)
        {
            if (!numbers.Numbers.ContainsKey(place))
                numbers.Set(numbers.Next++, place);
        }

        if (numbers.Places.Count != stored.Count)
        {
            var there = stored.Select(pair => pair.Place).ToHashSet();
            foreach (var (number, place) in numbers.Places.ToArray())
            {
                if (!there.Contains(place))
                    numbers.Set(number, null);
            }
        }

        return numbers;
    }

    /// <summary>The numbers of the grids that are gone are let go of.</summary>
    private void ForgetDecalNumbers()
    {
        foreach (var grid in _decalNumbers.Keys.ToArray())
        {
            if (!_entMan.EntityExists(grid))
                _decalNumbers.Remove(grid);
        }
    }

    /// <summary>Every decal of the grid with its place, in the order the game draws them after z-index.</summary>
    private List<(DecalIndex Place, Decal Decal)> Stored(EntityUid grid)
    {
        var stored = new List<(DecalIndex Place, Decal Decal)>();
        foreach (var chunk in ChunkEntities.GetChunks(grid))
        {
            if (!_entMan.TryGetComponent(chunk.Owner, out DecalChunkComponent? decals))
                continue;

            foreach (var (id, decal) in decals.Decals)
                stored.Add((new DecalIndex(chunk.Comp.Chunk, id), decal));
        }

        stored.Sort((a, b) => OrderOf(a.Place).CompareTo(OrderOf(b.Place)));
        return stored;
    }

    private Decal? FindDecal(EntityUid grid, uint id)
    {
        if (!NumbersOf(grid).Places.TryGetValue(id, out var place) || DecalChunk(grid, place.Chunk) is not { } chunk)
            return null;

        return chunk.Comp2.Decals.GetValueOrDefault(place.Id);
    }

    private uint NewDecalId(EntityUid grid) => NumbersOf(grid).Next++;

    /// <summary>
    /// Changes, adds or (with no <paramref name="decal"/>) removes a decal by the game's rules,
    /// and returns the change for undo.
    /// </summary>
    private DecalEdit EditDecal(EntityUid grid, uint id, Decal? old, Decal? decal, List<DecalChange> report)
    {
        var numbers = NumbersOf(grid);
        DecalIndex? from = old == null ? null : numbers.Places[id];
        DecalIndex? to = null;
        ushort toMax = 0;
        var (made, entityMade) = (false, false);
        (ushort, int)? gone = null;

        if (decal != null && from is { } here && ChunkEntitySystem.GetChunkIndices(decal.Coordinates) == here.Chunk)
        {
            // Changed where it is, as the game's SetDecalColor and the like change one. A move
            // within the chunk is changed so too, by the editor's own rule (see the top of this
            // file): the game would take it out and add it again.
            var chunk = DecalChunk(grid, here.Chunk)!.Value;
            toMax = chunk.Comp2.MaxDecalId;
            chunk.Comp2.Decals[here.Id] = decal;
            to = here;
        }
        else
        {
            var indices = decal == null ? default : ChunkEntitySystem.GetChunkIndices(decal.Coordinates);
            // Refused before anything changes, so a refusal leaves the decal where it was.
            if (decal != null && DecalChunk(grid, indices) is { } full && FreeId(full.Comp2) == null)
                throw new KernelException("This part of the grid already has as many decals as the game allows.");

            if (from is { } place)
                gone = TakeDecal(grid, place);

            if (decal != null)
            {
                var chunk = MakeChunk(grid, indices, out entityMade, out made);
                toMax = chunk.Comp2.MaxDecalId;
                to = new DecalIndex(indices, FreeId(chunk.Comp2)!.Value);
                PutDecal(chunk, to.Value.Id, decal);
            }
        }

        var change = new DecalEdit(grid, id, old, decal, from, to, toMax, made, entityMade, gone);
        numbers.Set(id, to);
        report.Add(new DecalChange(grid.Id, (int) id, decal == null || to == null ? null : DescribeDecal(id, decal, OrderOf(to.Value))));
        return change;
    }

    /// <summary>A decal change done again, or undone, as it was done the first time.</summary>
    private void ApplyDecal(DecalEdit change, bool forward, List<DecalChange> report)
    {
        var grid = change.Grid;
        if (change.From is { } here && change.To == here)
        {
            DecalChunk(grid, here.Chunk)!.Value.Comp2.Decals[here.Id] = forward ? change.New! : change.Old!;
        }
        else if (forward)
        {
            if (change.From is { } from)
                TakeDecal(grid, from);

            if (change.To is { } to)
                PutDecal(MakeChunk(grid, to.Chunk, out _, out _), to.Id, change.New!);
        }
        else
        {
            if (change.To is { } to && DecalChunk(grid, to.Chunk) is { } chunk)
            {
                chunk.Comp2.Decals.Remove(to.Id);
                chunk.Comp2.MaxDecalId = change.ToMax;
                // Only what the change made goes: a chunk entity the file had with no decals stays.
                if (change.ToMade && chunk.Comp2.Decals.Count == 0)
                {
                    if (change.ToEntityMade)
                    {
                        DropChunk(chunk);
                    }
                    else
                    {
                        _entMan.RemoveComponent<DecalChunkComponent>(chunk.Owner);
                        Untrack(chunk.Owner);
                    }
                }
            }

            if (change.From is { } from)
            {
                var back = MakeChunk(grid, from.Chunk, out _, out var made);
                if (made && change.FromGone is { } gone)
                {
                    back.Comp2.MaxDecalId = gone.Max;
                    if (gone.YamlUid != 0)
                        _entMan.EnsureComponent<YamlUidComponent>(back.Owner).Uid = gone.YamlUid;
                }

                PutDecal(back, from.Id, change.Old!);
            }
        }

        var now = forward ? change.To : change.From;
        var decal = forward ? change.New : change.Old;
        NumbersOf(grid).Set(change.Id, now);
        report.Add(new DecalChange(grid.Id, (int) change.Id, decal == null || now == null ? null : DescribeDecal(change.Id, decal, OrderOf(now.Value))));
    }

    /// <summary>A chunk's decals, if it has a chunk entity with any.</summary>
    private Entity<ChunkEntityComponent, DecalChunkComponent>? DecalChunk(EntityUid grid, Vector2i indices)
    {
        if (!ChunkEntities.TryGetChunk(grid, indices, out var chunk) || !_entMan.TryGetComponent(chunk.Value.Owner, out DecalChunkComponent? decals))
            return null;

        return (chunk.Value.Owner, chunk.Value.Comp, decals);
    }

    /// <summary>
    /// A chunk's decals, with its chunk entity made if it has none (DecalSystem.GetOrCreateDecalChunk).
    /// </summary>
    /// <param name="entityMade">Whether the chunk had no chunk entity before.</param>
    /// <param name="made">Whether the chunk had no decals of its own before: no chunk entity, or one without them.</param>
    private Entity<ChunkEntityComponent, DecalChunkComponent> MakeChunk(EntityUid grid, Vector2i indices, out bool entityMade, out bool made)
    {
        entityMade = !ChunkEntities.TryGetChunk(grid, indices, out _);
        var chunk = ChunkEntities.GetOrCreateChunk(grid, indices);
        made = !_entMan.HasComponent<DecalChunkComponent>(chunk.Owner);
        var decals = _entMan.EnsureComponent<DecalChunkComponent>(chunk.Owner);
        Untrack(chunk.Owner);
        return (chunk.Owner, chunk.Comp, decals);
    }

    /// <summary>
    /// The id a new decal of the chunk gets (DecalSystem.TryAllocateDecalId): the lowest free one
    /// up to the highest given, else the one after it. Null when the chunk has given all ids the
    /// server may give.
    /// </summary>
    private static ushort? FreeId(DecalChunkComponent chunk)
    {
        for (var id = 0; id <= chunk.MaxDecalId; id++)
        {
            if (!chunk.Decals.ContainsKey((ushort) id))
                return (ushort) id;
        }

        for (var next = chunk.MaxDecalId + 1; next <= DecalChunkComponent.MaxServerDecalId; next++)
        {
            if (!chunk.Decals.ContainsKey((ushort) next))
                return (ushort) next;
        }

        return null;
    }

    /// <summary>Puts a decal into a chunk under an id (DecalSystem.AddDecalWithId).</summary>
    private static void PutDecal(Entity<ChunkEntityComponent, DecalChunkComponent> chunk, ushort id, Decal decal)
    {
        chunk.Comp2.Decals[id] = decal;
        if (id <= DecalChunkComponent.MaxServerDecalId)
            chunk.Comp2.MaxDecalId = Math.Max(chunk.Comp2.MaxDecalId, id);
    }

    /// <summary>
    /// Takes a decal out of its chunk (DecalSystem.RemoveDecalInternal). A chunk left with none
    /// goes, and what undo needs to make it again is returned.
    /// </summary>
    private (ushort Max, int YamlUid)? TakeDecal(EntityUid grid, DecalIndex place)
    {
        var chunk = DecalChunk(grid, place.Chunk) ?? throw new KernelException($"The decal's chunk {place.Chunk} has no decals.");
        chunk.Comp2.Decals.Remove(place.Id);
        if (chunk.Comp2.Decals.Count > 0)
            return null;

        var gone = (chunk.Comp2.MaxDecalId, _entMan.TryGetComponent(chunk.Owner, out YamlUidComponent? yaml) ? yaml.Uid : 0);
        DropChunk(chunk);
        return gone;
    }

    /// <summary>
    /// A chunk with no decals left loses them, and its chunk entity goes unless it holds other
    /// things (DecalSystem.DirtyChunk, ChunkEntitySystem.TryRemoveChunk). The file's number for
    /// it does not count as another thing: a map the game loads for play has none.
    /// </summary>
    private void DropChunk(Entity<ChunkEntityComponent, DecalChunkComponent> chunk)
    {
        _entMan.RemoveComponent<DecalChunkComponent>(chunk.Owner);
        var others = _entMan.GetComponents(chunk.Owner)
            .Count(component => component is not (MetaDataComponent or TransformComponent or ChunkEntityComponent or YamlUidComponent or DecalChunkComponent) && !component.Deleted);
        if (others == 0)
            _entMan.DeleteEntity(chunk.Owner);
        Untrack(chunk.Owner);
    }

    /// <summary>A chunk entity is no part of the scene: its making and going are not reported.</summary>
    private void Untrack(EntityUid uid)
    {
        _touched.Remove(uid);
        _destroyed.Remove(uid.Id);
        _before.Remove(uid);
    }
}
#endif
