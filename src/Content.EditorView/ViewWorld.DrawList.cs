using System.Buffers.Binary;
using System.Numerics;
using System.Text.Json;
using Content.Client.Markers;
using Content.Shared.Decals;
#if FORK_DISPOSAL_EXITS
using Content.Shared.Disposal.Tube;
#endif
using Content.Shared.SubFloor;
using Robust.Client.GameObjects;
using Robust.Client.ResourceManagement;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;
using Robust.Shared.Map;
using Robust.Shared.Map.Components;
using Robust.Shared.Maths;
using Robust.Shared.Prototypes;

namespace Content.EditorView;

// Draw lists: the document as plain drawing data.
//
// For every shown entity, the engine's sprite code is run against a recording handle, and what
// it asked to draw is kept: textured rectangles in world space, in the order the renderer would
// have drawn them. Tiles and decals are added the way the renderer and the game's decal overlay
// draw them. A draw list is sent once in full and then as changes, so a page can draw the map
// itself and only hears from the engine when the map changes.
public sealed partial class ViewWorld
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);

    /// <summary>Tiles are drawn first, then decals, then entities by their own draw depth.</summary>
    private const int TileDepth = -2_000_000_000;
    private const int DecalDepth = -1_000_000_000;
    private const int ChunkSize = 16;
    private const int TilePixels = 32;
    /// <summary>The engine's image for a tile that has none of its own.</summary>
    private const string NoTile = "/Textures/noTile.png";

    private readonly TextureRegistry _textures = new();
    private readonly IResourceCache _resources = IoCManager.Resolve<IResourceCache>();
    private DrawRecorder? _recorder;

    public IEnumerable<int> DocumentsToRecord =>
        _documents.Where(pair => NeedsRecording(pair.Value)).Select(pair => pair.Key).ToArray();

    /// <summary>True if some document has changes that a draw list has not reported yet.</summary>
    public bool HasUnrecordedChanges => _documents.Values.Any(NeedsRecording);

    private static bool NeedsRecording(ViewDocument doc) =>
        doc.RecordAll || doc.Dirty.Count > 0 || doc.Gone.Count > 0 || doc.DirtyChunks.Count > 0 || doc.DirtyDecals.Count > 0;

    /// <summary>
    /// Builds a draw-list message for a document: everything (<paramref name="full"/>, or when
    /// the whole document is due), or only what changed since the last one. Returns null when
    /// there is nothing to report.
    ///
    /// Layout: "DRW1", the byte length of a JSON header, the header, padding to a multiple of
    /// four, then 32-bit floats, <see cref="DrawRecorder.FloatsPerQuad"/> per rectangle, in the
    /// order of the header's item list. An item is one entity (its id is the document's uid for
    /// it), one chunk of tiles or one grid's decals (negative ids), given as
    /// [id, draw depth, render order, vertical sort key, rectangle count, kind, tie]. The kind says
    /// what the item is, for an interface that hides layers itself: 0 an entity, 1 tiles,
    /// 2 decals, 3 a marker entity, 4 an entity that lives under the floor (pipes, cables),
    /// 5 a piece of plumbing, which the kernel names (SceneVisuals.PlumbingLayer), 6 a disposal
    /// tube under the floor. The last three cannot be told by draw depth: the game draws such things above doors while
    /// they are being shown through the floor. Items are drawn by depth, render order and sort
    /// key, then by the tie, then by id. The tie is the depth an item had before it was shown
    /// through the floor, and its draw depth otherwise.
    /// </summary>
    public byte[]? BuildDrawList(int document, bool full)
    {
        if (!_documents.TryGetValue(document, out var doc))
            return null;

        var everything = full || doc.RecordAll;
        if (!everything && !NeedsRecording(doc))
            return null;

        var sprites = _entMan.System<SpriteSystem>();
        _recorder ??= new DrawRecorder(_textures, sprites);

        var quads = new List<float>();
        var items = new List<object[]>();
        var skipped = 0;

        void Finish(int id, int depth, uint order, float sort, int kind, int? tie = null)
        {
            items.Add([id, depth, order, sort, _recorder.Quads.Count / DrawRecorder.FloatsPerQuad, kind, tie ?? depth]);
            quads.AddRange(_recorder.Quads);
            skipped += _recorder.Skipped;
        }

        // Tiles and decals.
        foreach (var gridDocUid in doc.Grids)
        {
            if (!doc.Entities.TryGetValue(gridDocUid, out var gridUid) || !_entMan.TryGetComponent(gridUid, out MapGridComponent? grid))
                continue;

            var matrix = _xform.GetWorldMatrix(gridUid);
            var existing = new HashSet<Vector2i>();
            foreach (var tile in _map.GetAllTiles(gridUid, grid))
                existing.Add(ChunkOf(tile.GridIndices));

            // A chunk that lost its last tile still has to be reported, as empty.
            var chunks = everything
                ? existing.Union(doc.ChunkIds.Keys.Where(key => key.Grid == gridDocUid).Select(key => key.Chunk)).ToArray()
                : doc.DirtyChunks.Where(key => key.Grid == gridDocUid).Select(key => key.Chunk).ToArray();

            foreach (var chunk in chunks)
            {
                if (!doc.ChunkIds.TryGetValue((gridDocUid, chunk), out var id))
                    doc.ChunkIds[(gridDocUid, chunk)] = id = -1 - doc.ChunkIds.Count;

                _recorder.Clear();
                _recorder.Skipped = 0;
                if (existing.Contains(chunk))
                    RecordTiles(gridUid, grid, chunk, existing, matrix);
                Finish(id, TileDepth, 0, 0, KindTiles);
            }

            if (everything || doc.DirtyDecals.Contains(gridDocUid))
            {
                _recorder.Clear();
                _recorder.Skipped = 0;
                RecordDecals(gridUid, matrix, sprites);
                Finish(-1_000_000 - gridDocUid, DecalDepth, 0, 0, KindDecals);
            }
        }

        // Entities.
        IEnumerable<int> targets = everything ? doc.DocUids.Values : doc.Dirty;
        foreach (var docUid in targets.ToArray())
        {
            if (!doc.Entities.TryGetValue(docUid, out var uid) || !_entMan.EntityExists(uid))
                continue;

            _recorder.Clear();
            _recorder.Skipped = 0;
            var kind = doc.Visuals.TryGetValue(docUid, out var looks) && looks.PlumbingLayer == true ? KindPlumbing : KindOf(uid);
            if (!_entMan.TryGetComponent(uid, out SpriteComponent? sprite) || !sprite.Visible)
            {
                // Nothing to draw, but say so: the page may be holding an older picture of it.
                Finish(docUid, 0, 0, 0, kind);
                continue;
            }

            _textures.RegisterLayers(sprite, _resources);
            var (position, rotation) = _xform.GetWorldPositionRotation(uid);
            try
            {
                sprites.RenderSprite((uid, sprite), _recorder, Angle.Zero, rotation, position);
            }
            catch (Exception e)
            {
                // A layer that hands its texture to a shader reaches for the renderer itself,
                // which a client with no window does not have. One such entity must not cost
                // the whole list: it is reported as drawn with nothing.
                _log.Warning($"{_entMan.ToPrettyString(uid)} could not be recorded: {e.Message}");
                _recorder.Clear();
                Finish(docUid, 0, 0, 0, kind);
                continue;
            }

            // The renderer draws sprites of equal depth from the top of the screen down, by
            // the lower edge of their bounds, so that things nearer the viewer overlap.
            var bounds = sprites.CalculateBounds((uid, sprite), position, rotation, Angle.Zero).CalcBoundingBox();
            var sort = -bounds.Bottom;

            // Shown through the floor, the game draws what lies under it above doors, all at one
            // depth (SubFloorHideSystem.cs:105-117). Its renderer then sorts by the lower edge,
            // and last by entity uid (Clyde.Sprite.cs:275-293), which follows the order the
            // client heard of things in and is no order a map can count on. On one cell that
            // is all it has, so the depth each had before is taken in its place: thick pipes
            // and disposals under thin pipes, those under cables (DrawDepth.cs:18-26). The
            // lower edge is rounded so that the turning of a sprite does not part what stands
            // on one cell.
            int? tie = null;
            if (_entMan.TryGetComponent(uid, out SubFloorHideComponent? subFloor) && subFloor.OriginalDrawDepth is { } original)
            {
                tie = original;
                sort = MathF.Round(sort * 1024) / 1024;
            }

            Finish(docUid, sprite.DrawDepth, sprite.RenderOrder, sort, kind, tie);
        }

        var grids = new List<object>();
        foreach (var gridDocUid in doc.Grids)
        {
            if (!doc.Entities.TryGetValue(gridDocUid, out var gridUid) || !_entMan.TryGetComponent(gridUid, out TransformComponent? gridXform))
                continue;

            grids.Add(new { uid = gridDocUid, x = gridXform.LocalPosition.X, y = gridXform.LocalPosition.Y, rotation = gridXform.LocalRotation.Theta });
        }

        var header = JsonSerializer.SerializeToUtf8Bytes(new
        {
            doc = document,
            full = everything,
            // A client that asked for everything is starting from nothing, textures included.
            textures = full ? _textures.All : _textures.TakeNew(),
            items,
            removed = everything ? [] : doc.Gone.ToArray(),
            grids,
            skipped,
        }, Json);
        if (full)
            _textures.TakeNew();

        doc.RecordAll = false;
        doc.Dirty.Clear();
        doc.Gone.Clear();
        doc.DirtyChunks.Clear();
        doc.DirtyDecals.Clear();

        return Pack(header, quads);
    }

    private const int KindEntity = 0;
    private const int KindTiles = 1;
    private const int KindDecals = 2;
    private const int KindMarker = 3;
    private const int KindSubFloor = 4;
    private const int KindPlumbing = 5;
    private const int KindDisposals = 6;

    private int KindOf(EntityUid uid)
    {
        if (_entMan.HasComponent<MarkerComponent>(uid))
            return KindMarker;

        if (!_entMan.HasComponent<SubFloorHideComponent>(uid))
            return KindEntity;

        // Disposal units stand on the floor and stay with the structures, as the game draws them.
#if FORK_DISPOSAL_EXITS
        return _entMan.HasComponent<DisposalTubeComponent>(uid) ? KindDisposals : KindSubFloor;
#else
        // A fork whose tube component is the server's alone has nothing the client can tell a
        // tube by, so its tubes stay with the rest under the floor (src/Directory.Build.props).
        return KindSubFloor;
#endif
    }

    /// <summary>
    /// How entity prototypes look, for palettes and for the outline of what is about to be
    /// placed: each is spawned nowhere, as the game's own spawn menu does, drawn at the origin,
    /// and deleted again. The answer is a draw-list message for document -1 whose item ids are
    /// the request's key times four plus the direction: 0 as placed, and, when
    /// <paramref name="directions"/> is set, 1 to 3 for each further quarter turn.
    /// </summary>
    public byte[] BuildPreviews(IReadOnlyList<(int Key, string Prototype)> requests, bool directions)
    {
        var sprites = _entMan.System<SpriteSystem>();
        _recorder ??= new DrawRecorder(_textures, sprites);

        var quads = new List<float>();
        var items = new List<object[]>();
        var skipped = 0;
        foreach (var (key, prototype) in requests)
        {
            var turns = directions ? 4 : 1;
            var recorded = 0;
            try
            {
                if (_protos.HasIndex<EntityPrototype>(prototype))
                {
                    var uid = _entMan.SpawnEntity(prototype, MapCoordinates.Nullspace);
                    try
                    {
                        if (_entMan.TryGetComponent(uid, out SpriteComponent? sprite))
                        {
                            _textures.RegisterLayers(sprite, _resources);
                            var kind = KindOf(uid);
                            for (; recorded < turns; recorded++)
                            {
                                _recorder.Clear();
                                _recorder.Skipped = 0;
                                sprites.RenderSprite((uid, sprite), _recorder, Angle.Zero, new Angle(recorded * Math.PI / 2), Vector2.Zero);
                                items.Add([key * 4 + recorded, sprite.DrawDepth, 0, 0, _recorder.Quads.Count / DrawRecorder.FloatsPerQuad, kind]);
                                quads.AddRange(_recorder.Quads);
                                skipped += _recorder.Skipped;
                            }
                        }
                    }
                    finally
                    {
                        _entMan.DeleteEntity(uid);
                    }
                }
            }
            catch (Exception e)
            {
                _log.Warning($"No preview of {prototype}: {e.Message}");
            }

            // Whatever could not be drawn is still answered, as nothing.
            for (; recorded < turns; recorded++)
                items.Add([key * 4 + recorded, 0, 0, 0, 0, KindEntity]);
        }

        var header = JsonSerializer.SerializeToUtf8Bytes(new
        {
            doc = -1,
            full = false,
            textures = _textures.TakeNew(),
            items,
            removed = Array.Empty<int>(),
            grids = Array.Empty<object>(),
            skipped,
        }, Json);
        return Pack(header, quads);
    }

    private static byte[] Pack(byte[] header, List<float> quads)
    {
        var headerLength = (header.Length + 3) & ~3;
        var message = new byte[8 + headerLength + quads.Count * 4];
        "DRW1"u8.CopyTo(message);
        BinaryPrimitives.WriteInt32LittleEndian(message.AsSpan(4), headerLength);
        header.CopyTo(message, 8);
        for (var i = header.Length; i < headerLength; i++)
            message[8 + i] = (byte) ' ';

        var floats = message.AsSpan(8 + headerLength);
        for (var i = 0; i < quads.Count; i++)
            BinaryPrimitives.WriteSingleLittleEndian(floats[(i * 4)..], quads[i]);

        return message;
    }

    private static Vector2i ChunkOf(Vector2i tile) =>
        new((int) MathF.Floor(tile.X / (float) ChunkSize), (int) MathF.Floor(tile.Y / (float) ChunkSize));

    /// <summary>
    /// One chunk of tiles, as the renderer builds it (Clyde.GridRendering): each tile's own
    /// image for its variant, turned and mirrored as the tile says, and then the edge images of
    /// neighbouring tiles that take priority, drawn over this tile.
    /// </summary>
    private void RecordTiles(EntityUid gridUid, MapGridComponent grid, Vector2i chunk, HashSet<Vector2i> existing, Matrix3x2 matrix)
    {
        var origin = chunk * ChunkSize;
        var edges = new List<(Vector2i Cell, string Path, int Turns)>();

        // One cell beyond the chunk on every side, for edges that fall on cells no chunk holds.
        for (var x = -1; x <= ChunkSize; x++)
        for (var y = -1; y <= ChunkSize; y++)
        {
            var cell = origin + new Vector2i(x, y);
            var owner = ChunkOf(cell);
            if (owner != chunk && existing.Contains(owner))
                continue;

            if (!_map.TryGetTile(grid, cell, out var tile))
                tile = Tile.Empty;
            _tileDefs.TryGetDefinition(tile.TypeId, out var definition);

            if (owner == chunk && !tile.IsEmpty)
            {
                // The game draws its missing tile image for a tile with no image, or a variant
                // its image does not have (Clyde.GridRendering.cs, ClydeTileDefinitionManager).
                var id = definition?.Sprite is { } sprite && tile.Variant < definition.Variants
                    ? _textures.File(sprite.ToString(), tile.Variant, TilePixels, TilePixels)
                    : _textures.File(NoTile, 0, TilePixels, TilePixels);
                AddTile(id, cell, definition is { AllowRotationMirror: true } ? tile.RotationMirroring : 0, matrix);
            }

            if (definition == null)
                continue;

            for (var nx = -1; nx <= 1; nx++)
            for (var ny = -1; ny <= 1; ny++)
            {
                if (nx == 0 && ny == 0)
                    continue;

                if (!_map.TryGetTile(grid, cell + new Vector2i(nx, ny), out var neighbour))
                    neighbour = Tile.Empty;
                if (!_tileDefs.TryGetDefinition(neighbour.TypeId, out var neighbourDefinition))
                    continue;

                if (tile.TypeId == neighbour.TypeId
                    || neighbourDefinition.EdgeSprites.Count == 0
                    || neighbourDefinition.EdgeSpritePriority <= definition.EdgeSpritePriority)
                {
                    continue;
                }

                var direction = new Vector2i(nx, ny).AsDirection().GetOpposite();
                if (neighbourDefinition.EdgeSprites.TryGetValue(direction, out var edge))
                    edges.Add((cell, edge.ToString(), EdgeTurns(direction)));
            }
        }

        // Edges go over every tile of the chunk, as the renderer draws them in a second pass.
        foreach (var (cell, path, turns) in edges)
            AddTile(_textures.File(path, 0, TilePixels, TilePixels), cell, turns, matrix);
    }

    /// <summary>
    /// The quarter turns an edge image takes for its direction. The game draws each edge image
    /// for the south side, or the south-east corner, and turns it to face the others when it
    /// loads the tile atlas (ClydeTileDefinitionManager): a quarter for east and north-east,
    /// a half for north and north-west, three quarters for west and south-west. One turn in
    /// <see cref="AddTile"/> takes the image's lower side to the cell's right, as east needs.
    /// </summary>
    private static int EdgeTurns(Direction direction) => direction switch
    {
        Direction.East or Direction.NorthEast => 1,
        Direction.North or Direction.NorthWest => 2,
        Direction.West or Direction.SouthWest => 3,
        _ => 0,
    };

    private void AddTile(int textureId, Vector2i cell, int rotationMirroring, Matrix3x2 matrix)
    {
        // Which corner of the image lands on each corner of the cell. The renderer turns the
        // image a quarter at a time and then mirrors it; the same steps on corner names.
        // Order throughout: bottom-left, bottom-right, top-right, top-left.
        Span<int> image = [0, 1, 2, 3];
        for (var turn = 0; turn < rotationMirroring % 4; turn++)
            (image[0], image[1], image[2], image[3]) = (image[3], image[0], image[1], image[2]);

        if (rotationMirroring >= 4)
        {
            // Swap left and right, or top and bottom, of whichever image corner is at each cell corner.
            ReadOnlySpan<int> swap = rotationMirroring % 2 == 0 ? [1, 0, 3, 2] : [3, 2, 1, 0];
            for (var i = 0; i < 4; i++)
                image[i] = swap[image[i]];
        }

        Span<Vector2> cellCorners =
        [
            new Vector2(cell.X, cell.Y), new Vector2(cell.X + 1, cell.Y), new Vector2(cell.X + 1, cell.Y + 1), new Vector2(cell.X, cell.Y + 1),
        ];

        // The recorder wants where each image corner goes, so turn the mapping around.
        Span<Vector2> byImageCorner = stackalloc Vector2[4];
        for (var i = 0; i < 4; i++)
            byImageCorner[image[i]] = Vector2.Transform(cellCorners[i], matrix);

        _recorder!.AddRaw(textureId, byImageCorner[0], byImageCorner[1], byImageCorner[2], byImageCorner[3], TilePixels, TilePixels);
    }

    /// <summary>A grid's decals, drawn as the game's decal overlay draws them.</summary>
    private void RecordDecals(EntityUid gridUid, Matrix3x2 matrix, SpriteSystem sprites)
    {
        if (!_entMan.TryGetComponent(gridUid, out DecalGridComponent? decalGrid))
            return;

        var decals = new List<(uint Id, Decal Decal)>();
        foreach (var chunk in decalGrid.ChunkCollection.ChunkCollection.Values)
        foreach (var (id, decal) in chunk.Decals)
            decals.Add((id, decal));

        decals.Sort((a, b) =>
        {
            var byZ = a.Decal.ZIndex.CompareTo(b.Decal.ZIndex);
            return byZ != 0 ? byZ : a.Id.CompareTo(b.Id);
        });

        _recorder!.SetTransform(matrix);
        var (_, gridRotation) = _xform.GetWorldPositionRotation(gridUid);
        foreach (var (_, decal) in decals)
        {
            if (!_protos.TryIndex<DecalPrototype>(decal.Id, out var prototype))
                continue;

            _textures.RegisterSpecifier(prototype.Sprite, sprites, _resources);
            var texture = sprites.Frame0(prototype.Sprite);
            var cardinal = prototype.SnapCardinals ? gridRotation.GetCardinalDir().ToAngle() : Angle.Zero;
            var angle = decal.Angle - cardinal;

            if (angle.Equals(Angle.Zero))
                _recorder.DrawTexture(texture, decal.Coordinates, decal.Color);
            else
                _recorder.DrawTexture(texture, decal.Coordinates, angle, decal.Color);
        }

        _recorder.SetTransform(Matrix3x2.Identity);
    }

    /// <summary>Marks the chunks holding a tile and its neighbours for re-recording.</summary>
    private static void MarkTiles(ViewDocument doc, int gridDocUid, Vector2i tile)
    {
        for (var dx = -1; dx <= 1; dx++)
        for (var dy = -1; dy <= 1; dy++)
            doc.DirtyChunks.Add((gridDocUid, ChunkOf(tile + new Vector2i(dx, dy))));
    }

    /// <summary>Marks what is anchored on and around an entity's tile for re-recording.</summary>
    private void MarkAround(ViewDocument doc, EntityUid uid)
    {
        if (!_entMan.TryGetComponent(uid, out TransformComponent? xform) || xform.GridUid is not { } gridUid)
            return;

        if (!_entMan.TryGetComponent(gridUid, out MapGridComponent? grid))
            return;

        MarkAround(doc, gridUid, grid, _map.TileIndicesFor(gridUid, grid, xform.Coordinates));
    }

    private void MarkAround(ViewDocument doc, EntityUid gridUid, MapGridComponent grid, Vector2i tile)
    {
        for (var dx = -1; dx <= 1; dx++)
        for (var dy = -1; dy <= 1; dy++)
        {
            var anchored = _map.GetAnchoredEntitiesEnumerator(gridUid, grid, tile + new Vector2i(dx, dy));
            while (anchored.MoveNext(out var neighbour))
            {
                if (doc.DocUids.TryGetValue(neighbour.Value, out var docUid))
                    doc.Dirty.Add(docUid);
            }
        }
    }
}
