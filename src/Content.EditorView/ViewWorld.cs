using System.Numerics;
using Content.Client.Decals;
using Content.Client.Decals.Overlays;
using Content.Client.Markers;
#if FORK_PLUMBING
using Content.Shared._Starlight.Plumbing;
#endif
using Content.Shared.Atmos;
using Content.Shared.Atmos.Components;
using Content.Shared.Atmos.EntitySystems;
using Content.Shared.Atmos.Piping;
using Content.Shared.Decals;
using Content.Shared.Wires;
using Robust.Client.GameObjects;
using Robust.Client.Graphics;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;
using Robust.Shared.Log;
using Robust.Shared.Map;
using Robust.Shared.Map.Components;
using Robust.Shared.Maths;
using Robust.Shared.Prototypes;
using SS14.EditorKernel;

namespace Content.EditorView;

/// <summary>
/// The view's copy of each open document, built from the scene the kernel reports: a map, its
/// grids with their tiles, and one client entity per document entity, spawned from the same
/// prototype in the same place. The game's own client systems then decide how it all looks.
/// Nothing here is saved. The document itself lives in the kernel.
/// </summary>
public sealed partial class ViewWorld
{
    private readonly IEntityManager _entMan = IoCManager.Resolve<IEntityManager>();
    private readonly ITileDefinitionManager _tileDefs = IoCManager.Resolve<ITileDefinitionManager>();
    private readonly IPrototypeManager _protos = IoCManager.Resolve<IPrototypeManager>();
    private readonly ISawmill _log = Logger.GetSawmill("editor.view");
    private readonly SharedMapSystem _map;
    private readonly SharedTransformSystem _xform;
    private readonly Dictionary<int, ViewDocument> _documents = new();

    public ViewWorld()
    {
        _map = _entMan.System<SharedMapSystem>();
        _xform = _entMan.System<SharedTransformSystem>();
    }

    private sealed class ViewDocument
    {
        public required EntityUid Map;
        public required MapId MapId;
        /// <summary>Document uid to view entity, for the map, its grids and every shown entity.</summary>
        public readonly Dictionary<int, EntityUid> Entities = new();

        /// <summary>View entity back to document uid, for shown entities only.</summary>
        public readonly Dictionary<EntityUid, int> DocUids = new();

        /// <summary>Document uids of the grids, to report their placement with a draw list.</summary>
        public readonly HashSet<int> Grids = new();

        // What the next draw list has to cover: everything, or these entities.
        public bool RecordAll = true;
        public readonly HashSet<int> Dirty = new();
        public readonly HashSet<int> Gone = new();

        // Tiles are recorded a 16x16 chunk at a time, and a grid's decals all together. Each
        // gets a negative item id, to keep them apart from entity uids.
        public readonly Dictionary<(int Grid, Vector2i Chunk), int> ChunkIds = new();
        public readonly HashSet<(int Grid, Vector2i Chunk)> DirtyChunks = new();
        public readonly HashSet<int> DirtyDecals = new();

        /// <summary>The looks the kernel sent last, by document uid, for a view entity made again.</summary>
        public readonly Dictionary<int, SceneVisuals> Visuals = new();

        /// <summary>The saved data the kernel sent last, by document uid, that the entity is made with (ViewWorld.Saved.cs).</summary>
        public readonly Dictionary<int, IReadOnlyList<SavedComponent>> Saved = new();

        /// <summary>Where each shown entity was last placed, to make it again when its saved data changes.</summary>
        public readonly Dictionary<int, SceneEntity> Placed = new();

        /// <summary>Shown entities whose saved data named one the view had not made yet.</summary>
        public readonly HashSet<int> Unresolved = new();
    }

    // ---- layer visibility ----
    //
    // The editor's layer switches hide entities by draw depth. An entity hidden here remembers
    // whether the game had it visible, so switching the layer back on restores exactly that
    // (something the game itself hides, such as a pipe under a floor tile, stays hidden).

    private bool _showEntities = true;
    private IReadOnlyList<int> _hiddenDepths = [];
    private readonly Dictionary<EntityUid, bool> _hiddenByLayer = new();

    public void SetLayers(bool entities, bool markers, bool decals, IReadOnlyList<int> hiddenDepths)
    {
        _showEntities = entities;
        _hiddenDepths = hiddenDepths;

        _entMan.System<MarkerSystem>().MarkersVisible = entities && markers;

        var overlays = IoCManager.Resolve<IOverlayManager>();
        if (overlays.HasOverlay<DecalOverlay>() != decals)
            _entMan.System<DecalSystem>().ToggleOverlay();

        foreach (var doc in _documents.Values)
        {
            foreach (var uid in doc.Entities.Values)
                ApplyLayer(uid);
            doc.RecordAll = true;
        }
    }

    /// <summary>Marks every document for a complete new draw list (a view-wide setting changed).</summary>
    public void InvalidateDrawLists()
    {
        foreach (var doc in _documents.Values)
            doc.RecordAll = true;
    }

    private void ApplyLayer(EntityUid uid)
    {
        // Markers follow the game's own marker switch.
        if (!_entMan.TryGetComponent(uid, out SpriteComponent? sprite) || _entMan.HasComponent<MarkerComponent>(uid))
            return;

        var hide = !_showEntities;
        for (var i = 0; !hide && i + 1 < _hiddenDepths.Count; i += 2)
            hide = sprite.DrawDepth >= _hiddenDepths[i] && sprite.DrawDepth <= _hiddenDepths[i + 1];

        var sprites = _entMan.System<SpriteSystem>();
        if (hide && !_hiddenByLayer.ContainsKey(uid))
        {
            _hiddenByLayer[uid] = sprite.Visible;
            sprites.SetVisible((uid, sprite), false);
        }
        else if (!hide && _hiddenByLayer.Remove(uid, out var wasVisible))
        {
            sprites.SetVisible((uid, sprite), wasVisible);
        }
    }

    public IReadOnlyCollection<int> DocumentIds => _documents.Keys.ToArray();

    public int EntityCount(int document) =>
        _documents.TryGetValue(document, out var doc) ? doc.Entities.Count : 0;

    public void ApplyScene(Scene scene)
    {
        Close(scene.Document.Id);

        var mapUid = _map.CreateMap(out var mapId, runMapInit: false);
        _map.SetPaused(mapId, true);
        var doc = new ViewDocument { Map = mapUid, MapId = mapId };
        doc.Entities[scene.Document.MapUid] = mapUid;
        _documents[scene.Document.Id] = doc;

        foreach (var grid in scene.Grids)
            UpsertGrid(doc, grid);

        foreach (var saved in scene.Saved)
            doc.Saved[saved.Uid] = saved.Components;

        var failed = 0;
        foreach (var entity in scene.Entities)
        {
            if (!Upsert(doc, entity))
                failed++;
        }

        if (failed > 0)
            _log.Warning($"{failed} of {scene.Entities.Count} entities could not be shown.");

        ResolveSaved(doc);

        ApplyVisuals(doc, scene.Visuals);
    }

    public void ApplyDelta(SceneDelta delta)
    {
        if (!_documents.TryGetValue(delta.Document, out var doc))
            return;

        foreach (var grid in delta.GridsChanged)
            UpsertGrid(doc, grid);

        // Tiles before entities: an entity can only be anchored to a tile that exists.
        foreach (var byGrid in delta.Tiles.GroupBy(tile => tile.Grid))
        {
            if (!doc.Entities.TryGetValue(byGrid.Key, out var gridUid) || !_entMan.TryGetComponent(gridUid, out MapGridComponent? grid))
                continue;

            var tiles = new List<(Vector2i, Tile)>();
            foreach (var change in byGrid)
                tiles.Add((new Vector2i(change.X, change.Y), ToTile(change.Tile, change.Variant, change.RotationMirroring)));
            _map.SetTiles(gridUid, grid, tiles);

            // What is on or next to a changed tile can look different (things under floors),
            // and so can the tiles around it (edge sprites).
            foreach (var (index, _) in tiles)
            {
                MarkAround(doc, gridUid, grid, index);
                MarkTiles(doc, byGrid.Key, index);
            }
        }

        foreach (var change in delta.Decals)
        {
            if (!doc.Entities.TryGetValue(change.Grid, out var gridUid) || !_entMan.EntityExists(gridUid))
                continue;

            var decals = _entMan.EnsureComponent<DecalGridComponent>(gridUid);
            RemoveDecal(decals, (uint) change.Id);
            if (change.Decal != null)
                PutDecal(decals, change.Decal);
            doc.DirtyDecals.Add(change.Grid);
        }

        foreach (var removed in delta.Removed)
        {
            if (doc.Entities.Remove(removed, out var uid) && _entMan.EntityExists(uid))
            {
                MarkAround(doc, uid);
                doc.DocUids.Remove(uid);
                doc.Dirty.Remove(removed);
                doc.Gone.Add(removed);
                doc.Visuals.Remove(removed);
                _hiddenByLayer.Remove(uid);
                _entMan.DeleteEntity(uid);
            }

            doc.Saved.Remove(removed);
            doc.Placed.Remove(removed);
        }

        // An entity whose saved data changed is made again with it, where the delta puts it
        // or where it stood.
        var again = TakeSaved(doc, delta.Saved);
        foreach (var entity in delta.Entities)
            Upsert(doc, entity);
        foreach (var uid in again)
        {
            if (!doc.Entities.ContainsKey(uid) && doc.Placed.TryGetValue(uid, out var placed))
                Upsert(doc, placed);
        }

        ResolveSaved(doc);

        // An older kernel's delta, read from text, has none.
        ApplyVisuals(doc, delta.Visuals ?? []);
    }

    /// <summary>
    /// Sets the looks the kernel worked out as the game's server would, with the client's own
    /// appearance system, as if the server had sent them. The client's visualizers then dress
    /// the sprites in the frames that follow (ViewKernel.Settle). Only the entity itself looks
    /// different, so only it is recorded again.
    /// </summary>
    private void ApplyVisuals(ViewDocument doc, IReadOnlyList<SceneVisuals> visuals)
    {
        foreach (var record in visuals)
        {
            doc.Visuals[record.Uid] = record;
            if (doc.Entities.TryGetValue(record.Uid, out var uid) && _entMan.EntityExists(uid))
            {
                SetVisuals(uid, record);
                doc.Dirty.Add(record.Uid);
            }
        }
    }

    private void SetVisuals(EntityUid uid, SceneVisuals record)
    {
        if (!_entMan.TryGetComponent(uid, out AppearanceComponent? appearance))
            return;

        var system = _entMan.System<SharedAppearanceSystem>();
        if (record.CableMask is { } mask)
            system.SetData(uid, WireVisVisuals.ConnectedMask, (WireVisDirFlags) mask, appearance);
        // The layer first, as the server sets it on start: it changes the sprite's files.
        if (record.PipeLayer is { } layer && _entMan.TryGetComponent(uid, out AtmosPipeLayersComponent? layers))
            _entMan.System<SharedAtmosPipeLayersSystem>().SetPipeLayer((uid, layers), (AtmosPipeLayer) layer);
        if (record.PipeState is { } state)
            system.SetData(uid, PipeVisuals.VisualState, state, appearance);
#if FORK_PLUMBING
        // Only a fork with Starlight's plumbing has its looks (src/Directory.Build.props).
        foreach (var value in record.Plumbing ?? [])
        {
            // Two of the keys are a yes or no to the client's PlumbingConnectorAppearanceSystem.
            if (!Enum.TryParse<PlumbingVisuals>(value.Key, out var key))
                continue;
            if (key is PlumbingVisuals.ManifoldMode or PlumbingVisuals.CoveredByFloor)
                system.SetData(uid, key, value.Value != 0, appearance);
            else
                system.SetData(uid, key, value.Value, appearance);
        }
#endif

        if (record.PipeColor is { } color)
            system.SetData(uid, PipeColorVisuals.Color, Color.FromHex(color), appearance);
        if (record.Appearance is { } values)
            SetAppearance(uid, appearance, values);
    }

    public void Close(int document)
    {
        if (!_documents.Remove(document, out var doc))
            return;

        if (_entMan.EntityExists(doc.Map))
            _entMan.DeleteEntity(doc.Map);
    }

    /// <summary>
    /// The map a document is shown on, and where a point given in one of its grids' own
    /// coordinates is in the world, with that grid's rotation.
    /// </summary>
    public bool TryLocate(int document, int grid, Vector2 local, out MapCoordinates position, out Angle rotation)
    {
        position = default;
        rotation = default;
        if (!_documents.TryGetValue(document, out var doc))
            return false;

        if (doc.Entities.TryGetValue(grid, out var gridUid) && _entMan.EntityExists(gridUid))
        {
            var (_, worldRotation, matrix) = _xform.GetWorldPositionRotationMatrix(gridUid);
            position = new MapCoordinates(Vector2.Transform(local, matrix), doc.MapId);
            rotation = worldRotation;
            return true;
        }

        position = new MapCoordinates(local, doc.MapId);
        return true;
    }

    private void UpsertGrid(ViewDocument doc, SceneGrid scene)
    {
        if (!doc.Entities.TryGetValue(scene.Uid, out var uid) || !_entMan.EntityExists(uid))
        {
#if ENGINE_MAP_SYSTEM_GRIDS
            uid = _map.CreateGridEntity(doc.MapId).Owner;
#else
            // An engine without grids made by the map system makes them with the map manager.
            uid = IoCManager.Resolve<IMapManager>().CreateGridEntity(doc.MapId).Owner;
#endif
            doc.Entities[scene.Uid] = uid;
            doc.Grids.Add(scene.Uid);
        }

        doc.RecordAll = true;

        // The map can be a grid itself, as a planet's is. It is made one here too, and stays
        // where a map is.
        if (uid == doc.Map)
        {
            doc.Grids.Add(scene.Uid);
        }
        else
        {
            // Placed at once. The client's ordinary setters start a smoothing animation from the
            // old position to the new one, which is meant for movement arriving from a server and,
            // with no server to finish it, would leave the grid sliding back and forth for ever.
            var xform = _entMan.GetComponent<TransformComponent>(uid);
            _xform.SetLocalPositionNoLerp(uid, new Vector2(scene.X, scene.Y), xform);
            _xform.SetLocalRotationNoLerp(uid, new Angle(scene.Rotation), xform);
        }

        var grid = _entMan.EnsureComponent<MapGridComponent>(uid);
        var tiles = new List<(Vector2i, Tile)>(scene.Tiles.Count / 5);
        for (var i = 0; i + 4 < scene.Tiles.Count; i += 5)
        {
            var id = scene.TileIds[scene.Tiles[i + 2]];
            tiles.Add((new Vector2i(scene.Tiles[i], scene.Tiles[i + 1]), ToTile(id, scene.Tiles[i + 3], scene.Tiles[i + 4])));
        }

        _map.SetTiles(uid, grid, tiles);

        // The client draws decals straight from this component, so filling it is all it takes.
        var decals = _entMan.EnsureComponent<DecalGridComponent>(uid);
        decals.ChunkCollection.ChunkCollection.Clear();
        decals.DecalIndex.Clear();
        foreach (var decal in scene.Decals)
            PutDecal(decals, decal);
    }

#if FORK_DECAL_CHUNK_ENTITIES
    // A fork that keeps its decals on chunk entities gives none from the kernel
    // (Editor.Kernel/Documents.cs), so there are none to put here.
    private static void PutDecal(DecalGridComponent comp, SceneDecal scene)
    {
    }

    private static void RemoveDecal(DecalGridComponent comp, uint id)
    {
    }
#else
    private static void PutDecal(DecalGridComponent comp, SceneDecal scene)
    {
        Color? color = scene.Color == null ? null : Color.FromHex(scene.Color);
        var decal = new Decal(new Vector2(scene.X, scene.Y), scene.Prototype, color, new Angle(scene.Angle), scene.ZIndex, scene.Cleanable);
        var indices = SharedDecalSystem.GetChunkIndices(decal.Coordinates);
        var chunks = comp.ChunkCollection.ChunkCollection;
        if (!chunks.TryGetValue(indices, out var chunk))
            chunks[indices] = chunk = new DecalGridComponent.DecalChunk();

        chunk.Decals[(uint) scene.Id] = decal;
        comp.DecalIndex[(uint) scene.Id] = indices;
    }

    private static void RemoveDecal(DecalGridComponent comp, uint id)
    {
        if (comp.DecalIndex.Remove(id, out var indices) && comp.ChunkCollection.ChunkCollection.TryGetValue(indices, out var chunk))
            chunk.Decals.Remove(id);
    }
#endif

    private Tile ToTile(string id, int variant, int rotationMirroring)
    {
        if (!_tileDefs.TryGetDefinition(id, out var definition) || definition.TileId == Tile.Empty.TypeId)
            return Tile.Empty;

        return new Tile(definition.TileId, 0, (byte) variant, (byte) rotationMirroring);
    }

    private bool Upsert(ViewDocument doc, SceneEntity scene)
    {
        var known = doc.Entities.TryGetValue(scene.Uid, out var uid) && _entMan.EntityExists(uid);

        // Things inside containers are not drawn by the game either.
        if (scene.Contained || scene.Prototype.Length == 0 || !doc.Entities.TryGetValue(scene.Parent, out var parent))
        {
            if (known)
            {
                MarkAround(doc, uid);
                doc.DocUids.Remove(uid);
                doc.Gone.Add(scene.Uid);
                _entMan.DeleteEntity(uid);
                doc.Entities.Remove(scene.Uid);
                doc.Placed.Remove(scene.Uid);
            }

            return true;
        }

        var coordinates = new EntityCoordinates(parent, scene.X, scene.Y);
        var rotation = new Angle(scene.Rotation);
        try
        {
            TransformComponent xform;
            if (!known)
            {
                if (!_protos.HasIndex<EntityPrototype>(scene.Prototype))
                    return false;

                uid = Spawn(doc, scene, coordinates, rotation);
                doc.Entities[scene.Uid] = uid;
                doc.DocUids[uid] = scene.Uid;
                doc.Gone.Remove(scene.Uid);
                xform = _entMan.GetComponent<TransformComponent>(uid);
            }
            else
            {
                xform = _entMan.GetComponent<TransformComponent>(uid);
                // Its old neighbours may look different once it has gone (wall smoothing).
                MarkAround(doc, uid);
                if (xform.Anchored)
                    _xform.Unanchor(uid, xform);
                _xform.SetCoordinates(uid, xform, coordinates, rotation);
            }

            if (scene.Anchored && !xform.Anchored)
                _xform.AnchorEntity(uid, xform);
            else if (!scene.Anchored && xform.Anchored)
                _xform.Unanchor(uid, xform);

            if (!known && (!_showEntities || _hiddenDepths.Count > 0))
                ApplyLayer(uid);

            // Made again, as after it was in a container: the kernel sends looks only when they change.
            if (!known && doc.Visuals.TryGetValue(scene.Uid, out var visuals))
                SetVisuals(uid, visuals);

            doc.Dirty.Add(scene.Uid);
            doc.Placed[scene.Uid] = scene;
            MarkAround(doc, uid);

            return true;
        }
        catch (Exception e)
        {
            _log.Error($"Could not show {scene.Prototype} ({scene.Uid}): {e.Message}");
            return false;
        }
    }
}
