using System.Diagnostics;
using System.Numerics;
using Content.Shared.Atmos;
using Content.Shared.Atmos.Components;
using Content.Shared.Decals;
using Content.Shared.DeviceLinking;
using Content.Shared.DeviceNetwork.Components;
using Content.Shared.Item;
using Content.Shared.Storage;
using Robust.Shared.Containers;
using Robust.Shared.EntitySerialization;
using Robust.Shared.EntitySerialization.Components;
using Robust.Shared.EntitySerialization.Systems;
using Robust.Shared.GameObjects;
using Robust.Shared.Map;
using Robust.Shared.Map.Components;
using Robust.Shared.Map.Enumerators;
using Robust.Shared.Map.Events;
using Robust.Shared.Maths;
using Robust.Shared.Physics;
using Robust.Shared.Prototypes;
using Robust.Shared.Serialization.Markdown;
using Robust.Shared.Serialization.Markdown.Mapping;
using Robust.Shared.Serialization.Markdown.Value;
using ComponentSnapshot = (string Name, Robust.Shared.Serialization.Markdown.Mapping.MappingDataNode? Before, Robust.Shared.Serialization.Markdown.Mapping.MappingDataNode? After);

namespace SS14.EditorKernel;

/// <summary>
/// Open documents: map and grid files held as live engine entities for the whole editing session.
/// Every edit is an engine operation, the scene the interface shows is read back from the engine,
/// and saving uses the game's own serializer. Nothing here builds or parses map YAML by hand.
/// </summary>
public sealed class DocumentManager
{
    private readonly IEntityManager _entMan;
    private readonly ITileDefinitionManager _tiles;
    private readonly IPrototypeManager _protos;
    private readonly MapLoaderSystem _loader;
    private readonly ComponentEditor _components;
    private readonly SharedMapSystem _map;
    private readonly SharedTransformSystem _xform;
    private readonly MetaDataSystem _meta;
    private readonly SharedContainerSystem _containers;
    private readonly EditorDeviceLinkSystem _links;
    private readonly VisualsReader _visuals;
    private readonly CollectingLogHandler _log;
#if ENGINE_MAP_SYSTEM_GRIDS
    private Entity<MapGridComponent> CreateGrid(MapId map) => _map.CreateGridEntity(map);
    private Entity<MapGridComponent> CreateGrid(EntityUid map) => _map.CreateGridEntity(map);
#else
    // Engines without grids made by the map system make them with the map manager.
    private readonly IMapManager _mapManager;
    private Entity<MapGridComponent> CreateGrid(MapId map) => _mapManager.CreateGridEntity(map);
    private Entity<MapGridComponent> CreateGrid(EntityUid map) => _mapManager.CreateGridEntity(map);
#endif

    private readonly Dictionary<int, Document> _documents = new();
    private readonly Dictionary<EntityUid, Document> _byMap = new();
    private int _nextId = 1;

    /// <summary>
    /// Deleted entities are parked on this map instead of being destroyed, so undo can put the
    /// very same entity back. A map save only writes the map's own children, so parked entities
    /// never reach a file.
    /// </summary>
    private EntityUid _parking;

    /// <summary>Removed grids are parked on this map, apart from the parked entities.</summary>
    private EntityUid _gridParking;

    // Entities the engine reported as created, moved or destroyed since the last delta.
    private readonly HashSet<EntityUid> _touched = new();
    private readonly HashSet<int> _destroyed = new();

    // Whose links the next delta reports. Those whose link data a command changed are
    // reported whatever they have left. Those that came into the document, new or put back,
    // are reported if they have any.
    private readonly HashSet<EntityUid> _linksChanged = new();
    private readonly HashSet<EntityUid> _linksEntered = new();

    // The grids whose stored air the current command put in place, for the next delta.
    private readonly HashSet<EntityUid> _airChanged = new();

    /// <summary>
    /// Where each entity the engine moved or re-anchored during the current edit was before it.
    /// An edit can move entities it never named (erasing a tile detaches what was anchored to
    /// it), and undo has to put those back as well.
    /// </summary>
    private readonly Dictionary<EntityUid, Placement> _before = new();

    // While an edit sets tiles: the grid, and what the engine takes off it, in the order it goes.
    private EntityUid _leavingGrid;
    private List<EntityUid>? _leaving;

    // MetaDataComponent keeps "no name of its own" as null in an internal field, and the public
    // setter can only store a string. Undoing a rename has to restore the null, or the save would
    // gain a name override the file never had.
    private static readonly System.Reflection.FieldInfo? NameOverrideField =
        typeof(MetaDataComponent).GetField("_entityName", System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic);

    internal DocumentManager(
        IEntityManager entMan,
        ITileDefinitionManager tiles,
        IPrototypeManager protos,
        MapLoaderSystem loader,
        ComponentEditor components,
        CollectingLogHandler log)
    {
        _entMan = entMan;
        _tiles = tiles;
        _protos = protos;
        _components = components;
        _loader = loader;
        _log = log;
        _map = entMan.System<SharedMapSystem>();
        _xform = entMan.System<SharedTransformSystem>();
        _meta = entMan.System<MetaDataSystem>();
        _containers = entMan.System<SharedContainerSystem>();
        _links = entMan.System<EditorDeviceLinkSystem>();
        _visuals = new VisualsReader(entMan, _map, tiles);
#if !ENGINE_MAP_SYSTEM_GRIDS
        _mapManager = entMan.EntitySysManager.DependencyCollection.Resolve<IMapManager>();
#endif

        // The scene the interface mirrors is driven by what the engine says happened, not by
        // what a command meant to do, so side effects of an edit are reported too.
        _entMan.EntityAdded += entity => _touched.Add(entity.Owner);
        _entMan.EntityDeleted += entity => _destroyed.Add(entity.Owner.Id);
        _xform.OnGlobalMoveEvent += (ref MoveEvent ev) =>
        {
            _touched.Add(ev.Sender);
            // An un-anchoring is always announced before the move it allows, so if this entity
            // has no record yet its anchoring has not changed during this edit.
            _before.TryAdd(ev.Sender, new Placement(ev.OldPosition.EntityId, ev.OldPosition.Position, ev.OldRotation, ev.Component.Anchored));
            if (_leaving != null && ev.ParentChanged && ev.OldPosition.EntityId == _leavingGrid)
                _leaving.Add(ev.Sender);
        };
        entMan.System<DocumentTrackingSystem>().AnchorChanged += (ref AnchorStateChangedEvent ev) =>
        {
            _touched.Add(ev.Entity);
            var xform = ev.Transform;
            _before.TryAdd(ev.Entity, new Placement(xform.ParentUid, xform.LocalPosition, xform.LocalRotation, !ev.Anchored));
        };
    }

    private sealed class Document
    {
        public required int Id;
        public required string Source;
        public required FileCategory Category;
        public required EntityUid Map;
        public int FormatVersion;
        public string? EngineVersion;
        public long LoadMilliseconds;
        public int Revision;
        public readonly List<Step> Undo = new();
        public readonly List<Step> Redo = new();
        public readonly HashSet<EntityUid> Parked = new();
        /// <summary>The looks last sent of each entity (Visuals.cs).</summary>
        public readonly Dictionary<EntityUid, VisualsReader.Sent> Visuals = new();
    }

    /// <summary>One undoable user action.</summary>
    private sealed record Step(string Label, List<Change> Changes);

    private abstract record Change;
    /// <param name="Left">What the engine took off the grid as the tiles went, in the order it did.</param>
    private sealed record TilesChange(EntityUid Grid, List<(Vector2i Index, Tile Old, Tile New)> Tiles, List<EntityUid> Left) : Change;
    /// <param name="Held">The container it is in while it is there, if it is in one.</param>
    private sealed record PresenceChange(EntityUid Uid, Placement Placement, bool Created, Held? Held = null) : Change;
    private sealed record PlacementChange(EntityUid Uid, Placement Old, Placement New) : Change;
    private sealed record NameChange(EntityUid Uid, string? Old, string? New) : Change;
    private sealed record DecalEdit(EntityUid Grid, uint Id, Decal? Old, Decal? New) : Change;
    private sealed record ComponentsChange(EntityUid Uid, List<ComponentSnapshot> Components) : Change;
    /// <summary>The Joint component of one body, whole, around the leaving that cleared its joints.</summary>
    private sealed record JointsChange(EntityUid Uid, MappingDataNode Before, MappingDataNode? After) : Change;
    /// <summary>The air a grid stores by cell, whole and in its order, before and after air moved on it.</summary>
    private sealed record AirChange(EntityUid Grid, KeyValuePair<Vector2i, TileAtmosphere>[] Before, KeyValuePair<Vector2i, TileAtmosphere>[] After) : Change;
    /// <summary>An atmosphere a grid was given, as the game gives one, and whether a gas overlay came with it.</summary>
    private sealed record AtmosphereAdded(EntityUid Grid, bool Overlay) : Change;

    private readonly record struct Placement(EntityUid Parent, Vector2 Position, Angle Rotation, bool Anchored);

    /// <summary>A container of an entity's parent, and the entity's place in that container's list.</summary>
    private sealed record Held(string Container, int Index);

    // ---- opening, creating, closing ----

    /// <summary>Loads a map or grid file with the game's loader and keeps it open.</summary>
    public Scene Open(string yaml, string source)
    {
        _log.Drain();
        var sw = Stopwatch.StartNew();

        // Same options as the game's own `resave` command (Content.Server/Maps/ResaveCommand.cs).
        var deserialization = DeserializationOptions.Default with
        {
            StoreYamlUids = true,
            LogOrphanedGrids = false,
        };

        var (category, version, engineVersion) = PeekHeader(yaml);
        EntityUid mapUid;

        if (category != FileCategory.Grid)
        {
            using var reader = new StringReader(yaml);
            var options = MapLoadOptions.Default with { DeserializationOptions = deserialization };
            if (!_loader.TryLoadGeneric(reader, source, out var result, options))
                throw new KernelException("The engine's map loader rejected the file.", _log.Drain());

            category = result.Category;
            version = result.Version;
            engineVersion = result.EngineVersion;

            if (category == FileCategory.Map && result.Maps.Count == 1)
            {
                mapUid = result.Maps.First().Owner;
            }
            else
            {
                // Old grid files do not say what they are; the loader only works it out while
                // loading. Drop this attempt and load it again the way the game loads grids.
                _loader.Delete(result);
                if (category != FileCategory.Grid)
                {
                    throw new KernelException(
                        $"Only map and grid files can be edited. The engine read this file as: {category} " +
                        $"({result.Maps.Count} maps, {result.Grids.Count} grids).", _log.Drain());
                }

                mapUid = default;
            }
        }
        else
        {
            mapUid = default;
        }

        if (category == FileCategory.Grid)
        {
            // A grid file is loaded onto a map, as the game's `loadgrid` does. The map is left
            // uninitialized so that nothing on the grid runs its map-init logic.
            mapUid = _map.CreateMap(out var mapId, runMapInit: false);
            using var reader = new StringReader(yaml);
            if (!_loader.TryLoadGrid(mapId, reader, source, out _, deserialization))
            {
                _entMan.DeleteEntity(mapUid);
                throw new KernelException("The engine's map loader rejected the grid file.", _log.Drain());
            }
        }

        var document = new Document
        {
            Id = _nextId++,
            Source = source,
            Category = category,
            Map = mapUid,
            FormatVersion = version,
            EngineVersion = engineVersion,
            LoadMilliseconds = sw.ElapsedMilliseconds,
        };
        return Register(document);
    }

    /// <summary>Creates an empty document: a map with one empty grid, or a lone grid.</summary>
    public Scene New(string category, string source)
    {
        _log.Drain();
        var kind = category == "Grid" ? FileCategory.Grid : FileCategory.Map;
        var mapUid = _map.CreateMap(out var mapId, runMapInit: false);
        CreateGrid(mapId);

        return Register(new Document
        {
            Id = _nextId++,
            Source = source,
            Category = kind,
            Map = mapUid,
        });
    }

    private Scene Register(Document document)
    {
        _documents[document.Id] = document;
        _byMap[document.Map] = document;
        _touched.Clear();
        _destroyed.Clear();
        return GetScene(document.Id);
    }

    public void Close(int documentId)
    {
        var document = Get(documentId);
        foreach (var uid in document.Parked)
        {
            if (_entMan.EntityExists(uid))
                _entMan.DeleteEntity(uid);
        }

        _entMan.DeleteEntity(document.Map);
        _entMan.CullRemovedComponents();
        _documents.Remove(documentId);
        _byMap.Remove(document.Map);
        _touched.Clear();
        _destroyed.Clear();
    }

    public IReadOnlyList<int> OpenDocuments => _documents.Keys.ToArray();

    /// <summary>
    /// Which backdrop the game draws behind this map (the map's "parallax", or the game's
    /// default), whether it draws one at all (a planet's map has its biome instead), and
    /// whether the map names one itself or is left to the game's default.
    /// </summary>
    public (string Name, bool Drawn, bool Chosen) Backdrop(int documentId)
    {
        var document = Get(documentId);
        var chosen = _entMan.TryGetComponent(document.Map, out Content.Shared.Parallax.ParallaxComponent? parallax);
        return (chosen ? parallax!.Parallax : "Default", !IsPlanet(document.Map), chosen);
    }

    private bool IsPlanet(EntityUid map) => _entMan.HasComponent<Content.Shared.Parallax.Biomes.BiomeComponent>(map);

    /// <summary>
    /// Refuses a change to the backdrop of a map that cannot keep one: a grid file saves only
    /// its grid, and a planet's map draws its biome in place of a backdrop.
    /// </summary>
    private void CheckBackdropChange(Document document, ComponentsChange change)
    {
        if (change.Uid != document.Map || change.Components.All(component => component.Name != "Parallax"))
            return;

        if (document.Category == FileCategory.Grid)
            throw new KernelException("A grid file has no backdrop of its own. The game saves only its grid.");
        if (IsPlanet(document.Map))
            throw new KernelException("A planet's map draws its biome, and no backdrop.");
    }

    // ---- reading the scene ----

    public Scene GetScene(int documentId)
    {
        var document = Get(documentId);
        var grids = new List<SceneGrid>();
        var entities = new List<SceneEntity>();
        // A map can be a grid itself, as a planet's is, with its ground as its tiles.
        if (_entMan.TryGetComponent(document.Map, out MapGridComponent? ground))
            grids.Add(DescribeGrid(document.Map, ground));
        Collect(document.Map, grids, entities);

        var info = new DocumentInfo(
            document.Id,
            document.Source,
            document.Category.ToString(),
            document.FormatVersion,
            document.EngineVersion,
            document.Map.Id,
            entities.Count + grids.Count + (ground == null ? 1 : 0),
            document.LoadMilliseconds,
            _log.Drain());
        return new Scene(info, grids, entities, History(document), _visuals.All(document.Map, document.Visuals));
    }

    private void Collect(EntityUid parent, List<SceneGrid> grids, List<SceneEntity> entities)
    {
        var children = _entMan.GetComponent<TransformComponent>(parent).ChildEnumerator;
        while (children.MoveNext(out var child))
        {
            if (_entMan.TryGetComponent(child, out MapGridComponent? grid))
                grids.Add(DescribeGrid(child, grid));
            else
                entities.Add(DescribeEntity(child));

            Collect(child, grids, entities);
        }
    }

    private SceneGrid DescribeGrid(EntityUid uid, MapGridComponent grid)
    {
        var xform = _entMan.GetComponent<TransformComponent>(uid);
        var ids = new List<string> { TileId(Tile.Empty) };
        var indexOf = new Dictionary<int, int> { [Tile.Empty.TypeId] = 0 };
        var tiles = new List<int>();

        foreach (var tileRef in _map.GetAllTiles(uid, grid))
        {
            var tile = tileRef.Tile;
            if (!indexOf.TryGetValue(tile.TypeId, out var index))
            {
                index = ids.Count;
                indexOf[tile.TypeId] = index;
                ids.Add(TileId(tile));
            }

            tiles.Add(tileRef.X);
            tiles.Add(tileRef.Y);
            tiles.Add(index);
            tiles.Add(tile.Variant);
            tiles.Add(tile.RotationMirroring);
        }

        var decals = new List<SceneDecal>();
        if (_entMan.TryGetComponent(uid, out DecalGridComponent? decalGrid))
        {
            foreach (var chunk in decalGrid.ChunkCollection.ChunkCollection.Values)
            foreach (var (id, decal) in chunk.Decals)
                decals.Add(DescribeDecal(id, decal));
            decals.Sort((a, b) => a.Id.CompareTo(b.Id));
        }

        return new SceneGrid(
            uid.Id,
            YamlUid(uid),
            _entMan.GetComponent<MetaDataComponent>(uid).EntityName,
            xform.LocalPosition.X,
            xform.LocalPosition.Y,
            xform.LocalRotation.Theta,
            ids,
            tiles,
            decals);
    }

    private static SceneDecal DescribeDecal(uint id, Decal decal) => new(
        (int) id,
        decal.Id,
        decal.Coordinates.X,
        decal.Coordinates.Y,
        decal.Color?.ToHex(),
        decal.Angle.Theta,
        decal.ZIndex,
        decal.Cleanable);

    // ---- decals ----
    //
    // Decals live in a component on the grid, and the game changes them through its decal
    // system, which this kernel does not run (it is tied to players, admin rights and network
    // state). These few lines are that system's TryAddDecal and RemoveDecal with the same rules:
    // a known decal prototype, on a tile that is not space, filed under its 32-tile chunk.
    //
    // A fork that keeps its decals on chunk entities of the grid (src/Directory.Build.props) has
    // no decals in that component, so none are shown, and an edit of one is refused. The chunk
    // entities are loaded and saved as they are, with their decals.

#if FORK_DECAL_CHUNK_ENTITIES
    private const string DecalsOnChunks = "This fork keeps its decals on chunk entities of the grid, which the editor cannot edit yet. They are kept as they are when the map is saved.";

    private static Decal? FindDecal(EntityUid grid, uint id) => throw new KernelException(DecalsOnChunks);

    private static void SetDecal(EntityUid grid, uint id, Decal? decal) => throw new KernelException(DecalsOnChunks);
#else
    private Decal? FindDecal(EntityUid grid, uint id)
    {
        if (!_entMan.TryGetComponent(grid, out DecalGridComponent? comp))
            return null;

        foreach (var chunk in comp.ChunkCollection.ChunkCollection.Values)
        {
            if (chunk.Decals.TryGetValue(id, out var decal))
                return decal;
        }

        return null;
    }

    private void SetDecal(EntityUid grid, uint id, Decal? decal)
    {
        var comp = _entMan.EnsureComponent<DecalGridComponent>(grid);
        var chunks = comp.ChunkCollection.ChunkCollection;
        foreach (var (index, chunk) in chunks)
        {
            if (!chunk.Decals.Remove(id))
                continue;

            if (chunk.Decals.Count == 0)
                chunks.Remove(index);
            break;
        }

        comp.DecalIndex.Remove(id);
        if (decal == null)
            return;

        var indices = SharedDecalSystem.GetChunkIndices(decal.Coordinates);
        if (!chunks.TryGetValue(indices, out var target))
            chunks[indices] = target = new DecalGridComponent.DecalChunk();

        target.Decals[id] = decal;
        comp.DecalIndex[id] = indices;
    }
#endif

    /// <param name="kept">The decal being changed, if the op changes one.</param>
    private Decal BuildDecal(EntityUid grid, EditOp op, Decal? kept)
    {
        // A map can hold a decal of a prototype the game does not have. Such a decal may be
        // moved and keeps the prototype it has. A new decal, or another prototype, must be known.
        if (op.Prototype == null || (op.Prototype != kept?.Id && !_protos.HasIndex<DecalPrototype>(op.Prototype)))
            throw new KernelException($"Unknown decal '{op.Prototype}'.");

        var position = new Vector2(op.X, op.Y);
        var gridComp = _entMan.GetComponent<MapGridComponent>(grid);
        if (_map.GetTileRef(grid, gridComp, new EntityCoordinates(grid, position)).Tile.IsEmpty)
            throw new KernelException("A decal needs a tile under it.");

        Color? color = op.Color == null ? null : Color.FromHex(op.Color);
        return new Decal(position, op.Prototype, color, new Angle(op.Rotation), op.ZIndex, op.Cleanable);
    }

    private SceneEntity DescribeEntity(EntityUid uid)
    {
        var xform = _entMan.GetComponent<TransformComponent>(uid);
        var meta = _entMan.GetComponent<MetaDataComponent>(uid);
        var parent = xform.ParentUid;
        var contained = parent.IsValid()
                        && !_entMan.HasComponent<MapGridComponent>(parent)
                        && !_entMan.HasComponent<MapComponent>(parent);
        var prototype = meta.EntityPrototype;

        return new SceneEntity(
            uid.Id,
            YamlUid(uid),
            prototype?.ID ?? "",
            parent.Id,
            xform.LocalPosition.X,
            xform.LocalPosition.Y,
            xform.LocalRotation.Theta,
            xform.Anchored,
            contained,
            prototype != null && meta.EntityName == prototype.Name ? null : meta.EntityName);
    }

    /// <summary>Component data for the given entities, as a map file would store it.</summary>
    public IReadOnlyList<EntityDetails> Describe(int documentId, IReadOnlyList<int> uids)
    {
        var document = Get(documentId);
        var result = new List<EntityDetails>(uids.Count);
        foreach (var id in uids)
        {
            var uid = new EntityUid(id);
            if (_entMan.TryGetComponent(uid, out TransformComponent? xform) && (uid == document.Map || xform.MapUid == document.Map))
                result.Add(new EntityDetails(id, _components.Describe(uid)));
        }

        return result;
    }

    /// <summary>The data fields of an entity's components, or of the named ones, for typed forms (ComponentFields.cs).</summary>
    public FieldsAnswer Fields(int documentId, int id, IReadOnlyList<string>? names, IReadOnlySet<string> known)
    {
        var document = Get(documentId);
        var uid = new EntityUid(id);
        if (!_entMan.TryGetComponent(uid, out TransformComponent? xform) || !(uid == document.Map || xform.MapUid == document.Map))
            throw new KernelException($"The document has no entity {id}.");

        return _components.Fields(uid, names, known);
    }

    private ComponentsChange SetComponents(EntityUid uid, string json)
    {
        var snapshots = new Dictionary<string, MappingDataNode?>();
        var changed = _components.SetComponents(uid, json, name => snapshots[name] = _components.Snapshot(uid, name));
        var list = new List<ComponentSnapshot>(changed.Count);
        foreach (var name in changed)
            list.Add((name, snapshots.GetValueOrDefault(name), _components.Snapshot(uid, name)));
        return new ComponentsChange(uid, list);
    }

    private void ApplyComponents(ComponentsChange change, bool forward)
    {
        foreach (var (name, before, after) in change.Components)
            _components.Restore(change.Uid, name, forward ? after : before);
        MarkLinks(change);
    }

    /// <summary>Has the next delta report this entity's links, if the change is to where it keeps them.</summary>
    private void MarkLinks(ComponentsChange change)
    {
        foreach (var (name, _, _) in change.Components)
        {
            if (name is "DeviceLinkSource" or "DeviceList")
                _linksChanged.Add(change.Uid);
        }
    }

    // ---- device links ----

    /// <summary>Every device link and device list entry of the document.</summary>
    public LinkScene Links(int documentId)
    {
        var document = Get(documentId);
        var links = new List<SceneLink>();
        var sources = _entMan.AllEntityQueryEnumerator<DeviceLinkSourceComponent, TransformComponent>();
        while (sources.MoveNext(out var uid, out var source, out var xform))
        {
            if (xform.MapUid == document.Map)
                AddLinks(uid, source, links);
        }

        var listed = new List<ListedDevice>();
        var lists = _entMan.AllEntityQueryEnumerator<DeviceListComponent, TransformComponent>();
        while (lists.MoveNext(out var uid, out var list, out var xform))
        {
            if (xform.MapUid != document.Map)
                continue;

            foreach (var device in list.Devices)
                listed.Add(new ListedDevice(uid.Id, device.Id));
        }

        // By entity, and within one entity as its component keeps them.
        return new LinkScene(links.OrderBy(link => link.Source).ToArray(), listed.OrderBy(entry => entry.List).ToArray());
    }

    // ---- air ----

    /// <summary>The air stored on the document's grids, or on those of them named (Air.cs). It changes nothing.</summary>
    public AirScene Air(int documentId, IReadOnlyCollection<int>? grids) => AirReader.Read(_entMan, _map, _tiles, Get(documentId).Map, grids);

    private static void AddLinks(EntityUid uid, DeviceLinkSourceComponent source, List<SceneLink> into)
    {
        foreach (var (sink, ports) in source.LinkedPorts)
        foreach (var (from, to) in ports)
            into.Add(new SceneLink(uid.Id, from, sink.Id, to));
    }

    private EntityLinks LinksOf(EntityUid uid)
    {
        var links = new List<SceneLink>();
        if (_entMan.TryGetComponent(uid, out DeviceLinkSourceComponent? source))
            AddLinks(uid, source, links);

        var devices = _entMan.TryGetComponent(uid, out DeviceListComponent? list) ? list.Devices.Select(device => device.Id).ToArray() : [];
        return new EntityLinks(uid.Id, links, devices);
    }

    /// <summary>The ports the given entities have now, and whether they keep or can join a device list.</summary>
    public IReadOnlyList<EntityPorts> Ports(int documentId, IReadOnlyList<int> uids)
    {
        var document = Get(documentId);
        var result = new List<EntityPorts>(uids.Count);
        foreach (var id in uids)
        {
            var uid = Owned(document, id);
            _entMan.TryGetComponent(uid, out DeviceLinkSourceComponent? source);
            _entMan.TryGetComponent(uid, out DeviceLinkSinkComponent? sink);
            result.Add(new EntityPorts(
                id,
                source == null ? [] : source.Ports.Select(port => port.Id).OrderBy(port => port, StringComparer.Ordinal).ToArray(),
                sink == null ? [] : sink.Ports.Select(port => port.Id).OrderBy(port => port, StringComparer.Ordinal).ToArray(),
                source?.Range ?? 0,
                _entMan.HasComponent<DeviceListComponent>(uid),
                _entMan.HasComponent<DeviceNetworkComponent>(uid)));
        }

        return result;
    }

    /// <summary>
    /// Makes or takes away one device link with the game's device link system. ToggleLink is
    /// the call the game's network configurator makes for a mapper, so the game's rules hold:
    /// both ports must be there, and the two must be within the source's range. Links are
    /// kept in the source's component alone, which is recorded whole for undo.
    /// </summary>
    private ComponentsChange? Link(Document document, EditOp op)
    {
        var source = Owned(document, op.Uid);
        var sink = Owned(document, op.Target);
        if (!_entMan.TryGetComponent(source, out DeviceLinkSourceComponent? sourceComp))
            throw new KernelException($"Entity {op.Uid} sends no signals: it has no DeviceLinkSource.");
        if (!_entMan.TryGetComponent(sink, out DeviceLinkSinkComponent? sinkComp))
            throw new KernelException($"Entity {op.Target} takes no signals: it has no DeviceLinkSink.");

        ProtoId<SourcePortPrototype> from = op.Port ?? "";
        ProtoId<SinkPortPrototype> to = op.TargetPort ?? "";
        var linked = sourceComp.LinkedPorts.TryGetValue(sink, out var ports) && ports.Contains((from, to));
        if (linked == (op.Kind == "link"))
            return null;

        if (!linked)
        {
            // The game refuses these too, without a word. Said here so the user knows why.
            if (!sourceComp.Ports.Contains(from))
                throw new KernelException($"Entity {op.Uid} has no source port '{from}'. It has: {string.Join(", ", sourceComp.Ports)}.");
            if (!sinkComp.Ports.Contains(to))
                throw new KernelException($"Entity {op.Target} has no sink port '{to}'. It has: {string.Join(", ", sinkComp.Ports)}.");
        }

        var before = _components.Snapshot(source, "DeviceLinkSource");
        if (!_links.ToggleLink(null, source, sink, from, to, sourceComp, sinkComp))
            throw new KernelException($"The game refused to link entity {op.Uid} to entity {op.Target}. A link reaches {sourceComp.Range} tiles from this source.");

        return new ComponentsChange(source, [("DeviceLinkSource", before, _components.Snapshot(source, "DeviceLinkSource"))]);
    }

    /// <summary>
    /// Puts a device on an entity's device list or takes it off. The game does this in
    /// DeviceListSystem.UpdateDeviceList, a system of the server that the kernel cannot run:
    /// it needs the network configurator and all behind it, and it rewrites lists as a map
    /// is saved. These lines are that method for one device, with its rules: the device must
    /// be a network device, the list has a limit, and the device keeps the list in turn.
    /// </summary>
    private List<ComponentsChange> ListDevice(Document document, EditOp op)
    {
        var owner = Owned(document, op.Uid);
        var device = Owned(document, op.Target);
        if (!_entMan.TryGetComponent(owner, out DeviceListComponent? list))
            throw new KernelException($"Entity {op.Uid} keeps no device list.");

        var add = op.Kind == "list.add";
        if (list.Devices.Contains(device) == add)
            return [];

        if (add && !_entMan.HasComponent<DeviceNetworkComponent>(device))
            throw new KernelException($"Entity {op.Target} is no network device and can be on no device list.");
        if (add && list.Devices.Count + 1 > list.DeviceLimit)
            throw new KernelException($"The device list of entity {op.Uid} is full: it holds {list.DeviceLimit}.");

        // Where the device keeps its lists: its DeviceNetwork, or a LinkedDeviceNetwork the
        // game's network configurator gives it when it first stores it (src/Directory.Build.props).
        _entMan.TryGetComponent(device, out ListedDeviceComponent? listed);
        var listBefore = _components.Snapshot(owner, "DeviceList");
        var deviceBefore = listed == null ? null : _components.Snapshot(device, ForkCompat.ListedDevice);
        if (add)
        {
            list.Devices.Add(device);
            listed ??= _entMan.EnsureComponent<ListedDeviceComponent>(device);
            listed.DeviceLists.Add(owner);
        }
        else
        {
            list.Devices.Remove(device);
            listed?.DeviceLists.Remove(owner);
        }

        var changes = new List<ComponentsChange> { new(owner, [("DeviceList", listBefore, _components.Snapshot(owner, "DeviceList"))]) };
        if (listed != null)
            changes.Add(new ComponentsChange(device, [(ForkCompat.ListedDevice, deviceBefore, _components.Snapshot(device, ForkCompat.ListedDevice))]));
        return changes;
    }

    private int YamlUid(EntityUid uid) =>
        _entMan.TryGetComponent(uid, out YamlUidComponent? yaml) ? yaml.Uid : 0;

    // ---- copying ----

    /// <summary>
    /// Writes entities of one grid, with all they hold, as the game's own entity text. It is a
    /// question and not an edit: nothing in the document changes and nothing is recorded. An
    /// entity inside another of those given is copied once, with what holds it. The lists in
    /// which the copied name other entities are cut down to the copy while the serializer
    /// runs, so the text names nothing outside it.
    /// </summary>
    public CopyResult Copy(int documentId, IReadOnlyList<int> uids)
    {
        var document = Get(documentId);
        _log.Drain();

        var given = new HashSet<EntityUid>();
        foreach (var id in uids)
            given.Add(Owned(document, id));

        var roots = new HashSet<EntityUid>();
        var grid = EntityUid.Invalid;
        foreach (var uid in given)
        {
            var parent = _entMan.GetComponent<TransformComponent>(uid).ParentUid;
            if (Inside(parent, given))
                continue;

            if (!_entMan.HasComponent<MapGridComponent>(parent) || (grid.IsValid() && parent != grid))
                throw new KernelException("Only entities that stand on one grid can be copied.");

            grid = parent;
            roots.Add(uid);
        }

        // As the serializer gathers them: nothing the game does not save, nor what such a thing holds.
        var all = new HashSet<EntityUid>();
        void Gather(EntityUid uid)
        {
            if (_entMan.GetComponent<MetaDataComponent>(uid).EntityPrototype?.MapSavable == false)
                return;

            all.Add(uid);
            var children = _entMan.GetComponent<TransformComponent>(uid).ChildEnumerator;
            while (children.MoveNext(out var child))
                Gather(child);
        }

        foreach (var root in roots)
            Gather(root);
        roots.IntersectWith(all);
        if (roots.Count == 0)
            throw new KernelException("Nothing to copy.");

        var (fromX, fromY) = (int.MaxValue, int.MaxValue);
        foreach (var root in roots)
        {
            var position = _entMan.GetComponent<TransformComponent>(root).LocalPosition;
            fromX = Math.Min(fromX, (int) MathF.Floor(position.X));
            fromY = Math.Min(fromY, (int) MathF.Floor(position.Y));
        }

        _entMan.CullRemovedComponents();
        using var writer = new StringWriter { NewLine = "\n" };
        // What is left out is not pulled in and not complained of. The lists are cut down first.
        var options = SerializationOptions.Default with { MissingEntityBehaviour = MissingEntityBehaviour.Ignore };
        var putBack = _components.Trim(all);
        bool written;
        try
        {
            written = _loader.TrySaveGeneric(roots, writer, out _, options);
        }
        finally
        {
            putBack();
        }

        if (!written)
            throw new KernelException("The engine could not write the copy.", _log.Drain());

        // The engine writes platform line endings regardless of the writer's setting.
        return new CopyResult(writer.ToString().Replace("\r\n", "\n"), roots.Count, all.Count, fromX, fromY, _log.Drain());
    }

    /// <summary>Whether this entity, or anything it is inside, is one of these.</summary>
    private bool Inside(EntityUid uid, HashSet<EntityUid> these)
    {
        while (uid.IsValid())
        {
            if (these.Contains(uid))
                return true;

            uid = _entMan.GetComponent<TransformComponent>(uid).ParentUid;
        }

        return false;
    }

    private string TileId(Tile tile) => _tiles[tile.TypeId].ID;

    // ---- editing ----

    /// <summary>
    /// Applies a list of changes as one undoable step and reports what the engine did. With
    /// <paramref name="merge"/> the changes join the step before them instead: one stroke of a
    /// brush, or one drag, arrives as many edits and is undone as one.
    /// </summary>
    public SceneDelta Edit(int documentId, string label, IReadOnlyList<EditOp> ops, bool merge = false)
    {
        var document = Get(documentId);
        _log.Drain();
        _touched.Clear();
        _destroyed.Clear();
        _before.Clear();
        _linksChanged.Clear();
        _linksEntered.Clear();
        _airChanged.Clear();

        var changes = new List<Change>();
        var decalChanges = new List<DecalChange>();
        var createdDecals = new List<int>();
        var detailed = new HashSet<EntityUid>();
        var createdUids = new List<EntityUid>();
        var parked = new HashSet<EntityUid>();
        var pasted = new HashSet<EntityUid>();
        var tileChanges = new List<TileChange>();
        var pendingTiles = new Dictionary<EntityUid, List<(Vector2i Index, Tile Old, Tile New)>>();
        var pendingAir = new Dictionary<EntityUid, List<(Vector2i From, Vector2i? To)>>();
        var pendingFix = new List<EntityUid>();

        try
        {
        foreach (var op in ops)
        {
            switch (op.Kind)
            {
                case "tile":
                {
                    var gridUid = Owned(document, op.Grid);
                    var grid = _entMan.GetComponent<MapGridComponent>(gridUid);
                    var index = new Vector2i((int) MathF.Floor(op.X), (int) MathF.Floor(op.Y));
                    var definition = _tiles[op.Tile ?? TileId(Tile.Empty)];
                    var tile = definition.TileId == Tile.Empty.TypeId
                        ? Tile.Empty
                        : new Tile(definition.TileId, 0, (byte) op.Variant, (byte) op.RotationMirroring);
                    var old = _map.GetTileRef(gridUid, grid, index).Tile;
                    if (old == tile)
                        break;

                    if (!pendingTiles.TryGetValue(gridUid, out var list))
                        pendingTiles[gridUid] = list = new List<(Vector2i, Tile, Tile)>();
                    list.Add((index, old, tile));
                    break;
                }
                case "air":
                {
                    // Gathered, and moved all at once at the end (MoveAir).
                    var gridUid = Owned(document, op.Grid);
                    if (!_entMan.HasComponent<MapGridComponent>(gridUid))
                        throw new KernelException($"Entity {op.Grid} is no grid.");

                    if (!pendingAir.TryGetValue(gridUid, out var moves))
                        pendingAir[gridUid] = moves = new List<(Vector2i, Vector2i?)>();
                    // One with Clear has no destination: the air goes nowhere.
                    Vector2i? to = op.Clear ? null : new Vector2i((int) MathF.Floor(op.X), (int) MathF.Floor(op.Y));
                    moves.Add((new Vector2i(op.FromX, op.FromY), to));
                    break;
                }
                case "air.add":
                {
                    var gridUid = Owned(document, op.Grid);
                    if (AirFixer.AddRefusal(_entMan, gridUid) is { } refusal)
                        throw new KernelException(refusal);

                    changes.Add(AddAtmosphere(gridUid));
                    break;
                }
                case "air.fix":
                {
                    // Done at the end, after the air ops, each grid once (FixAir).
                    var gridUid = Owned(document, op.Grid);
                    if (AirFixer.Refusal(_entMan, gridUid) is { } refusal)
                        throw new KernelException(refusal);

                    if (!pendingFix.Contains(gridUid))
                        pendingFix.Add(gridUid);
                    break;
                }
                case "spawn":
                {
                    FlushTiles(pendingTiles, changes, tileChanges);
                    var parent = Owned(document, op.Parent);
                    // The same two calls the game server makes when a mapper places an entity.
                    var uid = _entMan.SpawnEntity(op.Prototype, new EntityCoordinates(parent, op.X, op.Y));
                    _xform.SetLocalRotation(uid, new Angle(op.Rotation));
                    createdUids.Add(uid);
                    // Applied to an entity this same step creates, so undoing the step takes
                    // the data away with the entity and no separate record is needed.
                    if (op.Components != null)
                        SetComponents(uid, op.Components);
                    break;
                }
                case "paste":
                {
                    FlushTiles(pendingTiles, changes, tileChanges);
                    var gridUid = Owned(document, op.Grid);
                    if (!_entMan.HasComponent<MapGridComponent>(gridUid))
                        throw new KernelException("A paste goes onto a grid.");

                    // Each entity of the grid is recorded as a spawned one is. Undo parks it
                    // with what it holds and redo puts it back.
                    createdUids.AddRange(Paste(gridUid, op, pasted));
                    break;
                }
                case "components":
                {
                    var uid = Owned(document, op.Uid);
                    var change = SetComponents(uid, op.Components ?? "[]");
                    if (change.Components.Count > 0)
                    {
                        changes.Add(change);
                        detailed.Add(uid);
                        MarkLinks(change);
                        // After it is listed, so that a refusal puts it back.
                        CheckBackdropChange(document, change);
                    }

                    break;
                }
                case "link":
                case "unlink":
                {
                    if (Link(document, op) is { } change)
                    {
                        changes.Add(change);
                        detailed.Add(change.Uid);
                        MarkLinks(change);
                    }

                    break;
                }
                case "list.add":
                case "list.remove":
                {
                    foreach (var change in ListDevice(document, op))
                    {
                        changes.Add(change);
                        detailed.Add(change.Uid);
                        MarkLinks(change);
                    }

                    break;
                }
                case "grid.add":
                {
                    FlushTiles(pendingTiles, changes, tileChanges);
                    if (document.Category == FileCategory.Grid)
                        throw new KernelException("A grid file holds exactly one grid.");

                    // The game's own call, as for the grid of a new document. The engine names
                    // it "grid". Undo parks it like any entity this step creates.
                    var grid = CreateGrid(document.Map).Owner;
                    createdUids.Add(grid);
                    _xform.SetLocalPosition(grid, new Vector2(op.X, op.Y));
                    if (op.Rotation != 0)
                        _xform.SetLocalRotation(grid, new Angle(op.Rotation));
                    if (!string.IsNullOrEmpty(op.Name))
                        _meta.SetEntityName(grid, op.Name);
                    break;
                }
                case "grid.remove":
                {
                    FlushTiles(pendingTiles, changes, tileChanges);
                    var grid = Owned(document, op.Grid);
                    if (!_entMan.HasComponent<MapGridComponent>(grid))
                        throw new KernelException($"Entity {op.Grid} is no grid.");
                    if (grid == document.Map)
                        throw new KernelException("The map's own ground is a grid of the map itself, and cannot be removed.");
                    if (document.Category == FileCategory.Grid)
                        throw new KernelException("A grid file holds exactly one grid.");

                    // Parked whole, with all on it, as a deleted entity is with what it holds.
                    var placement = _before.TryGetValue(grid, out var earlier) ? earlier : Capture(grid);
                    Leave(document, grid, changes);
                    parked.Add(grid);
                    changes.Add(new PresenceChange(grid, placement, Created: false));
                    break;
                }
                case "contain":
                {
                    FlushTiles(pendingTiles, changes, tileChanges);
                    var owner = Owned(document, op.Parent);
                    var id = op.Container ?? "";
                    if (!_entMan.TryGetComponent(owner, out ContainerManagerComponent? manager) || !manager.Containers.ContainsKey(id))
                    {
                        var has = manager == null ? "" : string.Join(", ", manager.Containers.Keys);
                        throw new KernelException($"Entity {op.Parent} has no container '{id}'. It has: {(has.Length > 0 ? has : "none")}.");
                    }

                    if (op.Prototype == null || !_protos.HasIndex<EntityPrototype>(op.Prototype))
                        throw new KernelException($"Unknown entity prototype '{op.Prototype}'.");

                    // The game's own call. It makes the entity as the map is, started or not,
                    // and deletes it again if the container will not have it.
                    if (!_entMan.TrySpawnInContainer(op.Prototype, owner, id, out var made, manager))
                        throw new KernelException($"The game would not put a {op.Prototype} into '{id}' of entity {op.Parent}.");

                    createdUids.Add(made.Value);
                    if (op.Components != null)
                        SetComponents(made.Value, op.Components);
                    if (id == StorageComponent.ContainerId && _entMan.TryGetComponent(owner, out StorageComponent? storage)
                        && !storage.StoredItems.ContainsKey(made.Value))
                    {
                        // The game would take it out again (SharedStorageSystem.OnEntInserted).
                        if (FreePlace(storage, made.Value) is not { } place)
                            throw new KernelException($"There is no room for a {op.Prototype} in '{id}' of entity {op.Parent}.");

                        var before = _components.Snapshot(owner, "Storage");
                        storage.StoredItems[made.Value] = place;
                        changes.Add(new ComponentsChange(owner, [("Storage", before, _components.Snapshot(owner, "Storage"))]));
                    }

                    detailed.Add(owner);
                    break;
                }
                case "delete":
                case "uncontain":
                {
                    FlushTiles(pendingTiles, changes, tileChanges);
                    var uid = Owned(document, op.Uid);
                    if (_entMan.HasComponent<MapGridComponent>(uid) || uid == document.Map)
                        throw new KernelException("Grids and maps cannot be deleted as entities.");

                    var held = HeldOf(uid);
                    if (held == null && op.Kind == "uncontain")
                        throw new KernelException($"Entity {op.Uid} is in no container.");

                    // Where it was when this edit began, if an earlier part of the edit moved it.
                    var placement = _before.TryGetValue(uid, out var earlier) ? earlier : Capture(uid);
                    if (held != null)
                    {
                        // Out of the container by the game's own handler, as it leaves its parent.
                        placement = Capture(uid);
                        detailed.Add(placement.Parent);
                    }

                    Leave(document, uid, changes);
                    parked.Add(uid);
                    changes.Add(new PresenceChange(uid, placement, Created: false, held));
                    break;
                }
                case "move":
                {
                    FlushTiles(pendingTiles, changes, tileChanges);
                    var uid = Owned(document, op.Uid);
                    var old = Capture(uid);
                    var parent = op.Parent == 0 ? old.Parent : Owned(document, op.Parent);
                    Place(uid, old with { Parent = parent, Position = new Vector2(op.X, op.Y) });
                    break;
                }
                case "anchor":
                {
                    // Anchors an entity to the cell of its grid it stands on, as a wrench does in
                    // the game. Validate Map's Lay plating uses it: a wall left on a cell with no
                    // tile comes loose, and stays loose when a tile is laid under it. Tiles set
                    // earlier in the edit go first, so the cell has its tile.
                    FlushTiles(pendingTiles, changes, tileChanges);
                    var uid = Owned(document, op.Uid);
                    var xform = _entMan.GetComponent<TransformComponent>(uid);
                    if (xform.Anchored)
                        break;

                    if (!_entMan.HasComponent<MapGridComponent>(xform.ParentUid))
                        throw new KernelException($"Entity {op.Uid} is on no grid, so it can't be anchored.");

                    _xform.AnchorEntity(uid, xform);
                    if (!xform.Anchored)
                        throw new KernelException($"Entity {op.Uid} can't be anchored. Its cell may have no tile.");
                    break;
                }
                case "rotate":
                {
                    var uid = Owned(document, op.Uid);
                    _xform.SetLocalRotation(uid, new Angle(op.Rotation));
                    break;
                }
                case "rename":
                {
                    var uid = Owned(document, op.Uid);
                    var meta = _entMan.GetComponent<MetaDataComponent>(uid);
                    var old = NameOverride(meta);
                    _meta.SetEntityName(uid, op.Name ?? "", meta);
                    _touched.Add(uid);
                    changes.Add(new NameChange(uid, old, NameOverride(meta)));
                    break;
                }
#if FORK_DECAL_CHUNK_ENTITIES
                case "decal":
                case "decal.update":
                case "decal.remove":
                    throw new KernelException(DecalsOnChunks);
#else
                case "decal":
                case "decal.update":
                {
                    FlushTiles(pendingTiles, changes, tileChanges);
                    var gridUid = Owned(document, op.Grid);
                    var id = (uint) op.Uid;
                    var old = op.Kind == "decal" ? null : FindDecal(gridUid, id) ?? throw new KernelException($"Decal {id} does not exist.");
                    var decal = BuildDecal(gridUid, op, old);
                    if (op.Kind == "decal")
                    {
                        id = _entMan.EnsureComponent<DecalGridComponent>(gridUid).ChunkCollection.NextDecalId++;
                        createdDecals.Add((int) id);
                    }

                    var change = new DecalEdit(gridUid, id, old, decal);
                    ApplyDecal(change, forward: true, decalChanges);
                    changes.Add(change);
                    break;
                }
                case "decal.remove":
                {
                    var gridUid = Owned(document, op.Grid);
                    var id = (uint) op.Uid;
                    var old = FindDecal(gridUid, id) ?? throw new KernelException($"Decal {id} does not exist.");
                    var change = new DecalEdit(gridUid, id, old, null);
                    ApplyDecal(change, forward: true, decalChanges);
                    changes.Add(change);
                    break;
                }
#endif
                default:
                    throw new KernelException($"Unknown edit kind '{op.Kind}'.");
            }
        }

        FlushTiles(pendingTiles, changes, tileChanges);
        GainAtmospheres(changes);
        foreach (var (gridUid, moves) in pendingAir)
        {
            if (MoveAir(gridUid, moves) is { } air)
                changes.Add(air);
        }

        foreach (var gridUid in pendingFix)
        {
            if (FixAir(gridUid) is { } air)
                changes.Add(air);
        }

        Unlist(document, parked, changes, detailed);
        }
        catch
        {
            // An edit is all or nothing: put back whatever part of it had already happened.
            Rollback(document, changes, createdUids);
            throw;
        }

        foreach (var uid in createdUids)
        {
            if (_entMan.EntityExists(uid))
                changes.Add(new PresenceChange(uid, Capture(uid), Created: true, HeldOf(uid)));
        }

        // What was made may have come with links: a paste, or a spawn given component data.
        _linksEntered.UnionWith(createdUids);
        _linksEntered.UnionWith(pasted);

        // Everything else the engine moved, whether an op asked for it or it was a side effect.
        foreach (var (uid, before) in _before)
        {
            if (parked.Contains(uid) || pasted.Contains(uid) || createdUids.Contains(uid) || !_entMan.EntityExists(uid))
                continue;

            var after = Capture(uid);
            if (after != before)
                changes.Add(new PlacementChange(uid, before, after));
        }

        if (changes.Count > 0)
        {
            if (merge && document.Undo.Count > 0)
                document.Undo[^1].Changes.AddRange(changes);
            else
                document.Undo.Add(new Step(label, changes));
            document.Redo.Clear();
            document.Revision++;
        }

        return Delta(document, tileChanges, createdUids.Select(uid => uid.Id).ToArray(), decalChanges, createdDecals, detailed);
    }

    /// <summary>
    /// Makes new entities on a grid of the text a copy wrote, and returns those of the grid.
    /// The game's loader will not do it: it leaves such entities with no parent, or unanchors
    /// each one. So the engine's deserializer is driven as the loader drives it, and between
    /// making the entities and starting them each one of the grid is given the grid as its
    /// parent and its place, still anchored. It anchors itself to the grid as it starts.
    /// Whatever goes wrong, nothing is left behind.
    /// </summary>
    /// <param name="made">Every entity made is added to these, the held ones too.</param>
    private List<EntityUid> Paste(EntityUid grid, EditOp op, HashSet<EntityUid> made)
    {
        MappingDataNode data;
        try
        {
            using var reader = new StringReader(op.Text ?? "");
            data = (MappingDataNode) DataNodeParser.ParseYamlStream(reader).First().Root;
        }
        catch (Exception e)
        {
            throw new KernelException($"The text to paste is not entity text. {e.GetType().Name}: {e.Message}");
        }

        // The numbers of the text are put on the copies only to be taken off again below. It
        // is the sequence the engine check proved.
        var options = DeserializationOptions.Default with { StoreYamlUids = true };
        // Prototype renames from migration.yml, as the loader asks for them.
        var read = new BeforeEntityReadEvent();
        _entMan.EventBus.RaiseEvent(EventSource.Local, read);
        var deserializer = new EntityDeserializer(_entMan.EntitySysManager.DependencyCollection, data, options, read.RenamedPrototypes, read.DeletedPrototypes);
        var result = deserializer.Result;
        try
        {
            if (!deserializer.TryProcessData())
                throw new KernelException("The engine refused the text to paste.", _log.Drain());

            deserializer.CreateEntities();
            if (result.Orphans.Count == 0 || result.Maps.Count > 0 || result.Grids.Count > 0 || result.NullspaceEntities.Count > 0)
                throw new KernelException("Only a copy of entities of a grid can be pasted.");

            // In the order of the text, which is the order they were made in.
            var roots = result.Orphans.OrderBy(uid => uid.Id).ToList();
            var gridComp = _entMan.GetComponent<MapGridComponent>(grid);
            var quarters = ((op.Quarters % 4) + 4) % 4;
            var places = new List<(Vector2 Position, Angle Rotation)>(roots.Count);
            var noTile = 0;
            foreach (var root in roots)
            {
                var xform = _entMan.GetComponent<TransformComponent>(root);
                Vector2 position;
                var rotation = xform.LocalRotation.Theta;
                if (quarters == 0)
                {
                    position = xform.LocalPosition + new Vector2(op.X - op.FromX, op.Y - op.FromY);
                }
                else
                {
                    // About the middle of the cell the copy is measured from, against the clock.
                    var (dx, dy) = (xform.LocalPosition.X - (op.FromX + 0.5), xform.LocalPosition.Y - (op.FromY + 0.5));
                    for (var i = 0; i < quarters; i++)
                    {
                        (dx, dy) = (-dy, dx);
                        rotation = QuarterOn(rotation);
                    }

                    position = new Vector2((float) (op.X + 0.5 + dx), (float) (op.Y + 0.5 + dy));
                }

                places.Add((position, new Angle(rotation)));
                var cell = new Vector2i((int) MathF.Floor(position.X), (int) MathF.Floor(position.Y));
                if (_map.GetTileRef(grid, gridComp, cell).Tile.IsEmpty)
                    noTile++;
            }

            // Such an entity would leave the grid for the map and lose its anchoring. Leaving
            // it out would cut the lists of the rest, so the whole paste is refused.
            if (noTile > 0)
                throw new KernelException($"Nothing pasted: {noTile} of the {roots.Count} entities would be on a cell with no tile.");

            var target = _entMan.GetComponent<TransformComponent>(grid);
            for (var i = 0; i < roots.Count; i++)
            {
                var xform = _entMan.GetComponent<TransformComponent>(roots[i]);
                // Not unanchored: what is to be anchored anchors itself to the grid as it starts.
                // The engine keeps the facing of what it never turns.
                _xform.SetCoordinates(
                    (roots[i], xform, _entMan.GetComponent<MetaDataComponent>(roots[i])),
                    new EntityCoordinates(grid, places[i].Position),
                    rotation: places[i].Rotation,
                    unanchor: false,
                    newParent: target);
            }

            // They have their parent now. Left listed, the engine would start them as loose.
            result.Orphans.Clear();
            deserializer.StartEntities();

            foreach (var uid in result.Entities)
            {
                // These are the numbers the originals have in their file. Kept, the two would
                // contend for them at the next save.
                _entMan.RemoveComponent<YamlUidComponent>(uid);
                made.Add(uid);
            }

            return roots;
        }
        catch (Exception e)
        {
            _loader.Delete(result);
            if (e is KernelException)
                throw;

            throw new KernelException($"The paste failed. {e.GetType().Name}: {e.Message}", _log.Drain());
        }
    }

    /// <summary>
    /// An angle a quarter turn on, against the clock. The count of quarters stays within the
    /// four the angle is already in, so four turns come back to the very number. It is the
    /// editor's rule (quarter_on in tools/select.rs, ByQuarters in RegionCheck.cs).
    /// </summary>
    private static double QuarterOn(double angle)
    {
        const double quarter = Math.PI / 2;
        var quarters = Math.Floor(angle / quarter + 0.5);
        var rest = angle - quarters * quarter;
        var first = 4 * Math.Floor((quarters + 1) / 4) - 1;
        var next = quarters + 1;
        if (next > first + 3)
            next -= 4;
        return next * quarter + rest;
    }

    private void ApplyDecal(DecalEdit change, bool forward, List<DecalChange> report)
    {
        var now = forward ? change.New : change.Old;
        SetDecal(change.Grid, change.Id, now);
        report.Add(new DecalChange(change.Grid.Id, (int) change.Id, now == null ? null : DescribeDecal(change.Id, now)));
    }

    private void Rollback(Document document, List<Change> changes, List<EntityUid> created)
    {
        var discard = new List<TileChange>();
        var discardDecals = new List<DecalChange>();
        changes.Reverse();
        foreach (var tiles in changes.OfType<TilesChange>())
            ApplyTiles(tiles, forward: false, discard);
        foreach (var decal in changes.OfType<DecalEdit>())
            ApplyDecal(decal, forward: false, discardDecals);
        foreach (var air in changes.OfType<AirChange>())
            SetAir(air.Grid, air.Before);
        foreach (var added in changes.OfType<AtmosphereAdded>())
            RemoveAtmosphere(added);
        foreach (var uid in created)
        {
            if (_entMan.EntityExists(uid))
                _entMan.DeleteEntity(uid);
        }

        foreach (var presence in changes.OfType<PresenceChange>())
            Unpark(document, presence.Uid, presence.Placement, presence.Held);
        foreach (var (uid, before) in _before.ToArray())
        {
            if (_entMan.EntityExists(uid) && !created.Contains(uid))
                Place(uid, before);
        }

        foreach (var joints in changes.OfType<JointsChange>())
            _components.Restore(joints.Uid, "Joint", joints.Before);
        foreach (var name in changes.OfType<NameChange>())
            SetNameOverride(name.Uid, name.Old);
        foreach (var components in changes.OfType<ComponentsChange>())
            ApplyComponents(components, forward: false);

        _touched.Clear();
        _destroyed.Clear();
        _before.Clear();
        _linksChanged.Clear();
        _linksEntered.Clear();
    }

    private void FlushTiles(
        Dictionary<EntityUid, List<(Vector2i Index, Tile Old, Tile New)>> pending,
        List<Change> changes,
        List<TileChange> report)
    {
        foreach (var (gridUid, list) in pending)
        {
            var change = new TilesChange(gridUid, list, new List<EntityUid>());
            (_leavingGrid, _leaving) = (gridUid, change.Left);
            try
            {
                ApplyTiles(change, forward: true, report);
            }
            finally
            {
                _leaving = null;
            }

            changes.Add(change);
        }

        pending.Clear();
    }

    private void ApplyTiles(TilesChange change, bool forward, List<TileChange> report)
    {
        var grid = _entMan.GetComponent<MapGridComponent>(change.Grid);
        var tiles = new List<(Vector2i GridIndices, Tile Tile)>(change.Tiles.Count);
        foreach (var (index, old, @new) in change.Tiles)
            tiles.Add((index, forward ? @new : old));
        if (!forward)
            tiles = InChunkOrderBackwards(grid, change.Tiles, tiles);

        _map.SetTiles(change.Grid, grid, tiles);

        foreach (var (index, _, _) in change.Tiles)
        {
            // Report what the grid now holds, in case the engine adjusted it.
            var now = _map.GetTileRef(change.Grid, grid, index).Tile;
            report.Add(new TileChange(change.Grid.Id, index.X, index.Y, TileId(now), now.Variant, now.RotationMirroring));
        }
    }

    /// <summary>
    /// The tiles of a change being taken back, in the order that leaves the grid's chunks where
    /// they were. A grid keeps its chunks in a dictionary and saves them in the order the
    /// dictionary holds them. A chunk that is made takes the place of the chunk dropped last,
    /// or comes after all the others. So taking a change back has to make chunks in the reverse
    /// of the order they were dropped in, and drop them in the reverse of the order they were
    /// made in, and then the dictionary is as it was, for redo as well. The engine makes a chunk
    /// at its first tile in the list that is not space, and drops the emptied ones in the order
    /// of their first tile that went between space and not space (SharedMapSystem.SetTiles).
    /// Hence all the tiles of one chunk are given together, and the chunks in the reverse of
    /// the order of those first tiles. The tiles themselves are the same in any order.
    /// </summary>
    private List<(Vector2i GridIndices, Tile Tile)> InChunkOrderBackwards(
        MapGridComponent grid,
        List<(Vector2i Index, Tile Old, Tile New)> change,
        List<(Vector2i GridIndices, Tile Tile)> tiles)
    {
        var first = new Dictionary<Vector2i, int>();
        for (var i = 0; i < change.Count; i++)
        {
            if (change[i].Old.IsEmpty != change[i].New.IsEmpty)
                first.TryAdd(_map.GridTileToChunkIndices(grid, change[i].Index), i);
        }

        // A stable sort. Chunks none of whose tiles came or went are neither made nor dropped.
        return tiles
            .OrderByDescending(tile => first.GetValueOrDefault(_map.GridTileToChunkIndices(grid, tile.GridIndices), -1))
            .ToList();
    }

    /// <summary>
    /// Carries the air a grid stores for some of its cells to other cells of it, as a region of
    /// tiles moves or turns. Each destination gets a copy of what its source stored, or nothing
    /// if its source stored nothing, in place of what it had. A source that is no destination
    /// keeps no air. Its tile went too, and the game's rebuild gives an empty cell none. Every
    /// source is read before any destination is written, so a region that overlaps itself moves
    /// whole. A grid that stores no air is left alone. The game's own atmosphere system is not
    /// run: the component's data is written directly. A move with no destination only takes the
    /// air of its source away, as when a region is deleted.
    ///
    /// The save writes the chunks of four by four cells in the order their first cell is held,
    /// and the mixes in the order they are first used. So what stays keeps its place, and a
    /// destination that stored air before stores the new air in the same place. One that stored
    /// none goes after the last cell held of its chunk, or with its chunk after all the others,
    /// which keeps a chunk's cells together as the loader holds them.
    /// </summary>
    private AirChange? MoveAir(EntityUid grid, List<(Vector2i From, Vector2i? To)> moves)
    {
        if (!_entMan.TryGetComponent(grid, out GridAtmosphereComponent? atmosphere))
            return null;

        var before = atmosphere.Tiles.ToArray();
        var arriving = new Dictionary<Vector2i, TileAtmosphere?>();
        foreach (var (from, destination) in moves)
        {
            if (destination is { } to)
                arriving[to] = atmosphere.Tiles.TryGetValue(from, out var tile) ? new TileAtmosphere(tile) { GridIndices = to } : null;
        }
        var leaving = moves.Select(move => move.From).Where(cell => !arriving.ContainsKey(cell)).ToHashSet();

        var kept = new List<KeyValuePair<Vector2i, TileAtmosphere>>(before.Length + arriving.Count);
        var placed = new HashSet<Vector2i>();
        foreach (var (cell, tile) in before)
        {
            if (leaving.Contains(cell))
                continue;

            if (!arriving.TryGetValue(cell, out var arrived))
            {
                kept.Add(new(cell, tile));
                continue;
            }

            placed.Add(cell);
            if (arrived != null)
                kept.Add(new(cell, arrived));
        }

        // The new cells by chunk, the chunks in the order their first cell arrives.
        static Vector2i ChunkOf(Vector2i cell) => SharedMapSystem.GetChunkIndices(cell, 4);
        var joining = new Dictionary<Vector2i, List<KeyValuePair<Vector2i, TileAtmosphere>>>();
        var newChunks = new List<Vector2i>();
        foreach (var (_, destination) in moves)
        {
            if (destination is not { } to || !placed.Add(to) || arriving[to] is not { } arrived)
                continue;

            var chunk = ChunkOf(to);
            if (!joining.TryGetValue(chunk, out var cells))
            {
                joining[chunk] = cells = new List<KeyValuePair<Vector2i, TileAtmosphere>>();
                newChunks.Add(chunk);
            }

            cells.Add(new(to, arrived));
        }

        var last = new Dictionary<Vector2i, int>();
        for (var i = 0; i < kept.Count; i++)
            last[ChunkOf(kept[i].Key)] = i;

        var after = new List<KeyValuePair<Vector2i, TileAtmosphere>>(kept.Count + placed.Count);
        for (var i = 0; i < kept.Count; i++)
        {
            after.Add(kept[i]);
            var chunk = ChunkOf(kept[i].Key);
            if (last[chunk] == i && joining.Remove(chunk, out var cells))
                after.AddRange(cells);
        }

        foreach (var chunk in newChunks)
        {
            if (joining.TryGetValue(chunk, out var cells))
                after.AddRange(cells);
        }

        var change = new AirChange(grid, before, after.ToArray());
        SetAir(grid, change.After);
        return change;
    }

    /// <summary>
    /// Rebuilds a grid's stored air as the game's fixgridatmos does (AirFix.cs). Every tile
    /// gets new air, so what the step keeps for undo is never changed after. None if the grid's
    /// air is already what a fix would make, to the order it is held in.
    /// </summary>
    private AirChange? FixAir(EntityUid grid)
    {
        var atmosphere = _entMan.GetComponent<GridAtmosphereComponent>(grid);
        var before = atmosphere.Tiles.ToArray();
        var after = AirFixer.Rebuild(_entMan, _map, _tiles, grid, _entMan.GetComponent<MapGridComponent>(grid));
        if (AirFixer.SameAir(before, after))
            return null;

        var change = new AirChange(grid, before, after);
        SetAir(grid, after);
        return change;
    }

    /// <summary>
    /// Gives a grid an atmosphere as the game's addatmos and atmos:add do: the game's
    /// GridAtmosphere through the entity manager (AddAtmosCommand.cs 47). The game's atmosphere
    /// system gives it a gas overlay as the component starts (AtmosphereSystem.GridAtmosphere.cs
    /// 44-46). That system does not run here, so the overlay is added as it adds it. Nothing
    /// else of the running game follows: no tile is made or filled until a fix.
    /// </summary>
    private AtmosphereAdded AddAtmosphere(EntityUid grid)
    {
        var added = new AtmosphereAdded(grid, !_entMan.HasComponent<GasTileOverlayComponent>(grid));
        PutAtmosphere(added);
        return added;
    }

    private void PutAtmosphere(AtmosphereAdded added)
    {
        _entMan.AddComponent<GridAtmosphereComponent>(added.Grid);
        if (added.Overlay)
            _entMan.AddComponent<GasTileOverlayComponent>(added.Grid);
        _airChanged.Add(added.Grid);
    }

    /// <summary>Takes away exactly what <see cref="PutAtmosphere"/> added, the last added first.</summary>
    private void RemoveAtmosphere(AtmosphereAdded added)
    {
        if (added.Overlay)
            _entMan.RemoveComponent<GasTileOverlayComponent>(added.Grid);
        _entMan.RemoveComponent<GridAtmosphereComponent>(added.Grid);
        _airChanged.Add(added.Grid);
    }

    /// <summary>
    /// After the tiles of an edit: each grid that gained or lost a tile, and so changed its mass
    /// in the game, gets an atmosphere if the game's AutomaticAtmosSystem would give it one
    /// (<see cref="AirFixer.WouldGainAtmosphere"/>). It is part of the same step.
    /// </summary>
    private void GainAtmospheres(List<Change> changes)
    {
        var grids = new List<EntityUid>();
        foreach (var tiles in changes.OfType<TilesChange>())
        {
            if (!grids.Contains(tiles.Grid) && tiles.Tiles.Any(tile => tile.Old.IsEmpty != tile.New.IsEmpty))
                grids.Add(tiles.Grid);
        }

        foreach (var grid in grids)
        {
            if (_entMan.EntityExists(grid) && AirFixer.WouldGainAtmosphere(_entMan, grid))
                changes.Add(AddAtmosphere(grid));
        }
    }

    /// <summary>What fixing the air of each grid of a document would change, or why it cannot be fixed. Changes nothing.</summary>
    public IReadOnlyList<AirFixPreview> AirFixPreview(int documentId, IReadOnlyCollection<int>? grids)
    {
        var document = Get(documentId);
        var previews = new List<AirFixPreview>();
        var query = _entMan.AllEntityQueryEnumerator<MapGridComponent, TransformComponent>();
        while (query.MoveNext(out var uid, out var grid, out var xform))
        {
            if (xform.MapUid != document.Map || (grids != null && !grids.Contains(uid.Id)))
                continue;

            // A grid with no atmosphere that may get one is counted as if it had an empty one.
            var addable = AirFixer.Addable(_entMan, uid);
            var refusal = AirFixer.Refusal(_entMan, uid);
            if (refusal != null && !addable)
            {
                if (uid == document.Map)
                    refusal = "It is the map's own ground, a planet's, whose air is the map's. The game gives it no atmosphere.";
                previews.Add(new AirFixPreview(uid.Id, refusal, 0, 0));
                continue;
            }

            var stored = _entMan.TryGetComponent(uid, out GridAtmosphereComponent? atmosphere) ? atmosphere.Tiles : new Dictionary<Vector2i, TileAtmosphere>();
            try
            {
                var (tiles, outside) = AirFixer.Changes(_map, uid, grid, stored, AirFixer.Rebuild(_entMan, _map, _tiles, uid, grid));
                previews.Add(new AirFixPreview(uid.Id, refusal, tiles, outside, addable));
            }
            catch (KernelException e)
            {
                previews.Add(new AirFixPreview(uid.Id, e.Message, 0, 0));
            }
        }

        return previews;
    }

    /// <summary>
    /// Puts a grid's stored air back as it was held, in the same order. A dictionary that is
    /// cleared and filled again holds its entries in the order they were added.
    /// </summary>
    private void SetAir(EntityUid grid, KeyValuePair<Vector2i, TileAtmosphere>[] tiles)
    {
        if (!_entMan.TryGetComponent(grid, out GridAtmosphereComponent? atmosphere))
            return;

        atmosphere.Tiles.Clear();
        foreach (var (cell, tile) in tiles)
            atmosphere.Tiles.Add(cell, tile);
        _airChanged.Add(grid);
    }

    public SceneDelta Undo(int documentId) => Travel(documentId, undo: true);

    public SceneDelta Redo(int documentId) => Travel(documentId, undo: false);

    private SceneDelta Travel(int documentId, bool undo)
    {
        var document = Get(documentId);
        _log.Drain();
        _touched.Clear();
        _destroyed.Clear();
        _linksChanged.Clear();
        _linksEntered.Clear();
        _airChanged.Clear();

        var from = undo ? document.Undo : document.Redo;
        var to = undo ? document.Redo : document.Undo;
        var tileChanges = new List<TileChange>();
        var decalChanges = new List<DecalChange>();
        if (from.Count == 0)
            return Delta(document, tileChanges, []);

        var step = from[^1];
        from.RemoveAt(from.Count - 1);
        var ordered = (undo ? Enumerable.Reverse(step.Changes) : step.Changes).ToList();

        // A step records where everything ended up, not the order things happened in, so it is
        // replayed by kind. Tiles go first because an entity can only be put back on, and
        // anchored to, a tile that exists.
        if (!undo)
            LeaveInOrder(document, ordered);

        foreach (var tiles in ordered.OfType<TilesChange>())
            ApplyTiles(tiles, forward: !undo, tileChanges);

        foreach (var decal in ordered.OfType<DecalEdit>())
            ApplyDecal(decal, forward: !undo, decalChanges);

        // An atmosphere comes before the air it holds and goes after it.
        if (!undo)
        {
            foreach (var added in ordered.OfType<AtmosphereAdded>())
                PutAtmosphere(added);
        }

        foreach (var air in ordered.OfType<AirChange>())
            SetAir(air.Grid, undo ? air.Before : air.After);

        if (undo)
        {
            foreach (var added in ordered.OfType<AtmosphereAdded>())
                RemoveAtmosphere(added);
        }

        var detailed = new HashSet<EntityUid>();
        foreach (var presence in ordered.OfType<PresenceChange>())
        {
            // Undoing a creation takes the entity away. A deletion redone has taken it already.
            if (presence.Created == undo)
            {
                if (undo)
                    Park(document, presence.Uid);
            }
            else
            {
                Unpark(document, presence.Uid, presence.Placement, presence.Held);
                // Taking it away reported all it holds as removed. The engine moves only the
                // entity itself, so what it holds is named here, or the delta would leave it out.
                TouchDescendants(presence.Uid);
            }

            // What a container holds is part of its owner's component data.
            if (presence.Held != null)
                detailed.Add(presence.Placement.Parent);
        }

        foreach (var placement in ordered.OfType<PlacementChange>())
            Place(placement.Uid, undo ? placement.Old : placement.New);

        // After the entities are back on the map, or off it again. The engine clears the
        // joints of whatever changes map, so on redo this finds them cleared already.
        foreach (var joints in ordered.OfType<JointsChange>())
            _components.Restore(joints.Uid, "Joint", undo ? joints.Before : joints.After);

        foreach (var name in ordered.OfType<NameChange>())
        {
            SetNameOverride(name.Uid, undo ? name.Old : name.New);
            _touched.Add(name.Uid);
        }

        foreach (var components in ordered.OfType<ComponentsChange>())
        {
            ApplyComponents(components, forward: !undo);
            detailed.Add(components.Uid);
        }

        to.Add(step);
        document.Revision++;
        return Delta(document, tileChanges, [], decalChanges, null, detailed);
    }

    /// <summary>
    /// Before a step is redone: takes off their grids the entities the step took off them, in
    /// the order they went the first time. An entity saved for the first time is numbered by
    /// its place in its parent's list of children, and a child that joins takes the place of
    /// the child that left last. Left to the engine, the tiles that go would shed what stands
    /// on them in the order it finds them, which is not the order of the first time, and the
    /// entities made after that would be saved under each other's numbers. The deleted are
    /// parked here for the same reason: the tiles of the step would shed them first.
    /// </summary>
    private void LeaveInOrder(Document document, List<Change> changes)
    {
        foreach (var change in changes)
        {
            if (change is PresenceChange { Created: false } presence)
            {
                Park(document, presence.Uid);
                continue;
            }

            if (change is not TilesChange tiles || tiles.Left.Count == 0)
                continue;

            var map = _entMan.GetComponent<TransformComponent>(tiles.Grid).MapUid;
            if (map == null)
                continue;

            var emptied = new HashSet<Vector2i>();
            foreach (var (index, _, @new) in tiles.Tiles)
            {
                if (@new.IsEmpty)
                    emptied.Add(index);
            }

            foreach (var uid in tiles.Left)
            {
                // Only what the engine would take off itself: on this grid, on a tile that goes.
                if (!_entMan.TryGetComponent(uid, out TransformComponent? xform) || xform.ParentUid != tiles.Grid)
                    continue;

                var cell = new Vector2i((int) MathF.Floor(xform.LocalPosition.X), (int) MathF.Floor(xform.LocalPosition.Y));
                if (emptied.Contains(cell))
                    _xform.SetParent(uid, xform, map.Value);
            }
        }
    }

    private static string? NameOverride(MetaDataComponent meta) =>
        NameOverrideField != null ? (string?) NameOverrideField.GetValue(meta) : meta.EntityName;

    private void SetNameOverride(EntityUid uid, string? value)
    {
        var meta = _entMan.GetComponent<MetaDataComponent>(uid);
        if (value == null && NameOverrideField != null)
        {
            // Go through the setter first so the engine raises its events, then clear the field.
            _meta.SetEntityName(uid, meta.EntityPrototype?.Name ?? "", meta);
            NameOverrideField.SetValue(meta, null);
            return;
        }

        _meta.SetEntityName(uid, value ?? "", meta);
    }

    private Placement Capture(EntityUid uid)
    {
        var xform = _entMan.GetComponent<TransformComponent>(uid);
        return new Placement(xform.ParentUid, xform.LocalPosition, xform.LocalRotation, xform.Anchored);
    }

    private void Place(EntityUid uid, Placement placement)
    {
        var xform = _entMan.GetComponent<TransformComponent>(uid);
        if (xform.Anchored)
            _xform.Unanchor(uid, xform);

        _xform.SetCoordinates(uid, xform, new EntityCoordinates(placement.Parent, placement.Position), placement.Rotation);
        if (placement.Anchored)
            _xform.AnchorEntity(uid, xform);
    }

    /// <summary>
    /// Takes the entities an edit has parked, and all they hold, out of the lists of the
    /// entities left on the map that name them, as the game does when an entity is deleted.
    /// Left there, each would be saved as "invalid", and a button linked to two of them could
    /// not be saved at all: its links are a dictionary, and both keys would read "invalid".
    /// Each component changed is recorded whole, so undo puts every name back where it was.
    /// </summary>
    private void Unlist(Document document, HashSet<EntityUid> parked, List<Change> changes, HashSet<EntityUid> detailed)
    {
        if (parked.Count == 0)
            return;

        var gone = new HashSet<EntityUid>(parked);
        var held = new HashSet<int>();
        foreach (var uid in parked)
            AddDescendants(uid, held);
        foreach (var id in held)
            gone.Add(new EntityUid(id));

        foreach (var group in _components.Listers(gone).GroupBy(lister => lister.Uid))
        {
            var uid = group.Key;
            if (gone.Contains(uid) || _entMan.GetComponent<TransformComponent>(uid).MapUid != document.Map)
                continue;

            var list = new List<ComponentSnapshot>();
            foreach (var (_, name) in group)
            {
                var before = _components.Snapshot(uid, name);
                if (before == null)
                    continue;

                var after = _components.Without(name, before, gone);
                _components.Restore(uid, name, after);
                list.Add((name, before, after));
            }

            var change = new ComponentsChange(uid, list);
            changes.Add(change);
            detailed.Add(uid);
            MarkLinks(change);
        }
    }

    /// <summary>
    /// Parks an entity an op takes off the map, and records the joints that go as it does.
    /// The engine clears the joints of everything that changes map, on both bodies of each
    /// joint (SharedPhysicsSystem.RecursiveMapUpdate). The weld between two docked grids is
    /// one, and removing either grid clears it on both. That is right for the save, and the
    /// game does the same when a docked grid is deleted. But the entity is only parked, so
    /// the Joint component of each body is recorded whole, and undo puts the joints back.
    /// </summary>
    private void Leave(Document document, EntityUid uid, List<Change> changes)
    {
        var bodies = new HashSet<EntityUid>();
        var jointed = _entMan.AllEntityQueryEnumerator<JointComponent>();
        while (jointed.MoveNext(out var body, out var joints))
        {
            if (joints.JointCount == 0)
                continue;

            // Only the joints of the entity and of what is on or in it.
            var up = body;
            while (up.IsValid() && up != uid)
                up = _entMan.GetComponent<TransformComponent>(up).ParentUid;
            if (up != uid)
                continue;

            bodies.Add(body);
            foreach (var joint in joints.GetJoints.Values)
            {
                bodies.Add(joint.BodyAUid);
                bodies.Add(joint.BodyBUid);
            }
        }

        var before = bodies.Select(body => (Uid: body, Joints: _components.Snapshot(body, "Joint"))).ToList();
        Park(document, uid);
        foreach (var (body, joints) in before)
        {
            if (joints == null)
                continue;

            var after = _components.Snapshot(body, "Joint");
            if (!joints.Equals(after))
                changes.Add(new JointsChange(body, joints, after));
        }
    }

    private void Park(Document document, EntityUid uid)
    {
        if (!_parking.IsValid() || !_entMan.EntityExists(_parking))
        {
            _parking = _map.CreateMap(runMapInit: false);
            // The parking map is the kernel's own and no part of any document.
            _touched.Remove(_parking);
        }

        var xform = _entMan.GetComponent<TransformComponent>(uid);
        if (_entMan.HasComponent<MapGridComponent>(uid))
        {
            // A grid keeps its place and goes to a map of its own kind. On the map of the
            // parked entities, the next one parked would land on its tiles and the engine
            // would make it a child of the grid.
            if (!_gridParking.IsValid() || !_entMan.EntityExists(_gridParking))
            {
                _gridParking = _map.CreateMap(runMapInit: false);
                _touched.Remove(_gridParking);
            }

            _xform.SetCoordinates(uid, xform, new EntityCoordinates(_gridParking, xform.LocalPosition), xform.LocalRotation);
            document.Parked.Add(uid);
            return;
        }

        if (xform.Anchored)
            _xform.Unanchor(uid, xform);

        _xform.SetCoordinates(uid, xform, new EntityCoordinates(_parking, Vector2.Zero));
        document.Parked.Add(uid);
    }

    private void Unpark(Document document, EntityUid uid, Placement placement, Held? held = null)
    {
        if (held == null || !Hold(uid, placement, held))
            Place(uid, placement);
        document.Parked.Remove(uid);
        EnterLinks(uid);
    }

    /// <summary>An entity is back in the document with all it holds, and with it whatever links they have.</summary>
    private void EnterLinks(EntityUid uid)
    {
        _linksEntered.Add(uid);
        var children = _entMan.GetComponent<TransformComponent>(uid).ChildEnumerator;
        while (children.MoveNext(out var child))
            EnterLinks(child);
    }

    /// <summary>The container an entity is in, and its place in that container's list.</summary>
    private Held? HeldOf(EntityUid uid)
    {
        if (!_containers.TryGetContainingContainer((uid, null, null), out var container))
            return null;

        var list = container.ContainedEntities;
        for (var i = 0; i < list.Count; i++)
        {
            if (list[i] == uid)
                return new Held(container.ID, i);
        }

        return null;
    }

    /// <summary>
    /// Puts an entity back into the container it was in, with the game's container system.
    /// The game adds at the end of a container's list, and a map file lists what a container
    /// holds in that order. So those that were behind the entity are taken out and put in
    /// again, where they are, and the list is as it was. The engine puts each at 0,0 unturned,
    /// so each is given back its own place and turn.
    /// </summary>
    private bool Hold(EntityUid uid, Placement placement, Held held)
    {
        if (!_containers.TryGetContainer(placement.Parent, held.Container, out var container))
            return false;

        if (!_containers.Insert(uid, container, force: true))
            return false;

        Settle(uid, placement.Position, placement.Rotation);
        var behind = new List<(EntityUid Uid, Vector2 Position, Angle Rotation)>();
        var list = container.ContainedEntities;
        for (var i = held.Index; i < list.Count; i++)
        {
            if (list[i] == uid)
                continue;

            var xform = _entMan.GetComponent<TransformComponent>(list[i]);
            behind.Add((list[i], xform.LocalPosition, xform.LocalRotation));
        }

        foreach (var (other, position, rotation) in behind)
        {
            _containers.Remove(other, container, reparent: false, force: true);
            _containers.Insert(other, container, force: true);
            Settle(other, position, rotation);
        }

        return true;

        // The same parent, so it stays in the container.
        void Settle(EntityUid thing, Vector2 position, Angle rotation) =>
            _xform.SetCoordinates(thing, _entMan.GetComponent<TransformComponent>(thing), new EntityCoordinates(container.Owner, position), rotation);
    }

    // ---- a place in a bag ----
    // The kernel runs no storage system, so how the game finds a thing its place in a bag is
    // mirrored here step for step (SharedStorageSystem and SharedItemSystem).

    /// <summary>
    /// The first free place in a bag's grid of cells for a thing, as the game finds it when the
    /// thing goes in (TryGetAvailableGridSpace). Null if the thing is no item or has no room.
    /// </summary>
    private ItemStorageLocation? FreePlace(StorageComponent storage, EntityUid item)
    {
        if (!_entMan.TryGetComponent(item, out ItemComponent? itemComp))
            return null;

        // As UpdateOccupied fills it: outside the bag's cells is taken, then each thing's own.
        var occupied = new Dictionary<Vector2i, ulong>();
        MarkCells(storage.Grid, occupied, taken: false);
        foreach (var (stored, location) in storage.StoredItems)
        {
            if (_entMan.TryGetComponent(stored, out ItemComponent? storedItem))
                MarkCells(ItemShape(storedItem, location.Rotation, location.Position), occupied, taken: true);
        }

        var storageBounding = storage.Grid.GetBoundingBox();

        // A place saved for things of its name comes first (FindSavedLocation).
        if (storage.SavedLocations.TryGetValue(_entMan.GetComponent<MetaDataComponent>(item).EntityName, out var saved))
        {
            foreach (var location in saved)
            {
                if (storageBounding.Contains(location.Position) && Fits(occupied, ItemShape(itemComp, location.Rotation, location.Position)))
                    return location;
            }
        }

        Angle startAngle;
        if (storage.DefaultStorageOrientation == null)
        {
            startAngle = Angle.Zero;
        }
        else if (storageBounding.Width < storageBounding.Height)
        {
            startAngle = storage.DefaultStorageOrientation == StorageDefaultOrientation.Horizontal
                ? Angle.Zero
                : Angle.FromDegrees(90);
        }
        else
        {
            startAngle = storage.DefaultStorageOrientation == StorageDefaultOrientation.Vertical
                ? Angle.Zero
                : Angle.FromDegrees(90);
        }

        var itemShape = itemComp.Shape ?? _protos.Index(itemComp.Size).DefaultShape;
        var fastPath = itemShape.Count == 1 && itemShape[0].Contains(Vector2i.Zero);
        var angles = new List<Angle>();
        if (itemShape.Count != 1)
        {
            for (var angle = startAngle; angle <= Angle.FromDegrees(360 - startAngle); angle += Math.PI / 2f)
                angles.Add(angle);
        }
        else
        {
            angles.Add(startAngle);
            if (itemShape[0].Width != itemShape[0].Height)
                angles.Add(startAngle + Angle.FromDegrees(90));
        }

        var chunks = new ChunkIndicesEnumerator(storageBounding, StorageComponent.ChunkSize);
        while (chunks.MoveNext(out var storageChunk))
        {
            var storageChunkOrigin = storageChunk.Value * StorageComponent.ChunkSize;
            var left = Math.Max(storageChunkOrigin.X, storageBounding.Left);
            var bottom = Math.Max(storageChunkOrigin.Y, storageBounding.Bottom);
            var top = Math.Min(storageChunkOrigin.Y + StorageComponent.ChunkSize - 1, storageBounding.Top);
            var right = Math.Min(storageChunkOrigin.X + StorageComponent.ChunkSize - 1, storageBounding.Right);
            if (!occupied.TryGetValue(storageChunkOrigin, out var taken))
                continue;

            for (var y = bottom; y <= top; y++)
            {
                for (var x = left; x <= right; x++)
                {
                    foreach (var angle in angles)
                    {
                        var position = new Vector2i(x, y);
                        if (fastPath)
                        {
                            var flag = SharedMapSystem.ToBitmask(SharedMapSystem.GetChunkRelative(position, StorageComponent.ChunkSize), StorageComponent.ChunkSize);
                            if ((taken & flag) == flag)
                                continue;
                        }

                        if (Fits(occupied, ItemShape(itemComp, angle, position)))
                            return new ItemStorageLocation(angle, position);
                    }
                }
            }
        }

        return null;
    }

    /// <summary>An item's cells turned and moved to a place (SharedItemSystem.GetAdjustedItemShape).</summary>
    private List<Box2i> ItemShape(ItemComponent item, Angle rotation, Vector2i position)
    {
        var shapes = item.Shape ?? _protos.Index(item.Size).DefaultShape;
        var boundingShape = shapes.GetBoundingBox();
        var boundingCenter = ((Box2) boundingShape).Center;
        var matty = Matrix3Helpers.CreateTransform(boundingCenter, rotation);
        var drift = boundingShape.BottomLeft - matty.TransformBox(boundingShape).BottomLeft;
        var adjusted = new List<Box2i>(shapes.Count);
        foreach (var shape in shapes)
        {
            var transformed = matty.TransformBox(shape).Translated(drift);
            var floored = new Box2i(transformed.BottomLeft.Floored(), transformed.TopRight.Floored());
            adjusted.Add(floored.Translated(position));
        }

        return adjusted;
    }

    /// <summary>Whether none of these cells is taken. A chunk with no data is all taken (ItemFitsInGridLocation).</summary>
    private static bool Fits(Dictionary<Vector2i, ulong> occupied, List<Box2i> itemShape)
    {
        foreach (var box in itemShape)
        {
            var chunks = new ChunkIndicesEnumerator(box, StorageComponent.ChunkSize);
            while (chunks.MoveNext(out var chunk))
            {
                var chunkOrigin = chunk.Value * StorageComponent.ChunkSize;
                var left = Math.Max(chunkOrigin.X, box.Left);
                var bottom = Math.Max(chunkOrigin.Y, box.Bottom);
                var right = Math.Min(chunkOrigin.X + StorageComponent.ChunkSize - 1, box.Right);
                var top = Math.Min(chunkOrigin.Y + StorageComponent.ChunkSize - 1, box.Top);
                if (!occupied.TryGetValue(chunkOrigin, out var occupiedMask))
                    return false;

                for (var x = left; x <= right; x++)
                {
                    for (var y = bottom; y <= top; y++)
                    {
                        var flag = SharedMapSystem.ToBitmask(SharedMapSystem.GetChunkRelative(new Vector2i(x, y), StorageComponent.ChunkSize), StorageComponent.ChunkSize);
                        if ((occupiedMask & flag) == flag)
                            return false;
                    }
                }
            }
        }

        return true;
    }

    /// <summary>
    /// Marks cells taken, as AddOccupied does, or free, as RemoveOccupied does. A chunk first
    /// freed starts all taken, so what lies outside the bag's own cells stays taken.
    /// </summary>
    private static void MarkCells(IReadOnlyList<Box2i> boxes, Dictionary<Vector2i, ulong> occupied, bool taken)
    {
        foreach (var box in boxes)
        {
            var chunks = new ChunkIndicesEnumerator(box, StorageComponent.ChunkSize);
            while (chunks.MoveNext(out var chunk))
            {
                var chunkOrigin = chunk.Value * StorageComponent.ChunkSize;
                var existing = taken ? occupied.GetValueOrDefault(chunkOrigin) : occupied.GetValueOrDefault(chunkOrigin, ulong.MaxValue);
                var left = Math.Max(chunkOrigin.X, box.Left);
                var bottom = Math.Max(chunkOrigin.Y, box.Bottom);
                var right = Math.Min(chunkOrigin.X + StorageComponent.ChunkSize - 1, box.Right);
                var top = Math.Min(chunkOrigin.Y + StorageComponent.ChunkSize - 1, box.Top);
                for (var x = left; x <= right; x++)
                {
                    for (var y = bottom; y <= top; y++)
                    {
                        var flag = SharedMapSystem.ToBitmask(SharedMapSystem.GetChunkRelative(new Vector2i(x, y), StorageComponent.ChunkSize), StorageComponent.ChunkSize);
                        existing = taken ? existing | flag : existing & ~flag;
                    }
                }

                occupied[chunkOrigin] = existing;
            }
        }
    }

    /// <summary>The containers of the given entities, with what each holds.</summary>
    public IReadOnlyList<EntityContainers> Containers(int documentId, IReadOnlyList<int> uids)
    {
        var document = Get(documentId);
        var result = new List<EntityContainers>(uids.Count);
        foreach (var id in uids)
        {
            var uid = Owned(document, id);
            var containers = new List<ContainerInfo>();
            if (_entMan.TryGetComponent(uid, out ContainerManagerComponent? manager))
            {
                foreach (var (name, container) in manager.Containers)
                {
                    containers.Add(new ContainerInfo(
                        name,
                        container.GetType().Name,
                        container.ContainedEntities.Select(held => held.Id).ToArray(),
                        container is not ContainerSlot || container.Count == 0));
                }
            }

            result.Add(new EntityContainers(id, containers));
        }

        return result;
    }

    private SceneDelta Delta(
        Document document,
        List<TileChange> tiles,
        IReadOnlyList<int> created,
        IReadOnlyList<DecalChange>? decals = null,
        IReadOnlyList<int>? createdDecals = null,
        IReadOnlyCollection<EntityUid>? detailed = null)
    {
        var details = new List<EntityDetails>();
        foreach (var uid in detailed ?? [])
        {
            if (_entMan.EntityExists(uid))
                details.Add(new EntityDetails(uid.Id, _components.Describe(uid)));
        }

        var entities = new List<SceneEntity>();
        // In order: what a thing holds, or a grid carries, is named before the thing itself.
        var removed = new List<int>(_destroyed);
        var grids = new List<SceneGrid>();

        foreach (var uid in _touched)
        {
            if (!_entMan.EntityExists(uid))
                continue;

            var xform = _entMan.GetComponent<TransformComponent>(uid);
            if (xform.MapUid != document.Map)
            {
                // Parked, or otherwise no longer part of this document.
                AddInnermostFirst(uid, removed);
                continue;
            }

            // The map itself is reported only when it is a grid too, as a planet's is.
            if (uid == document.Map && !_entMan.HasComponent<MapGridComponent>(uid))
                continue;

            if (_entMan.TryGetComponent(uid, out MapGridComponent? grid))
                grids.Add(DescribeGrid(uid, grid));
            else
                entities.Add(DescribeEntity(uid));
        }

        var linked = new List<EntityLinks>();
        foreach (var uid in _linksChanged.Union(_linksEntered))
        {
            // One that is gone is in the removed, and its links went with it.
            if (!_entMan.TryGetComponent(uid, out TransformComponent? xform) || xform.MapUid != document.Map)
                continue;

            var links = LinksOf(uid);
            if (links.Links.Count > 0 || links.Devices.Count > 0 || _linksChanged.Contains(uid))
                linked.Add(links);
        }

        // Looks: of what this command touched, where it stood before and where it stands now.
        var cells = tiles.Select(tile => (new EntityUid(tile.Grid), new Vector2i(tile.X, tile.Y))).ToList();
        foreach (var before in _before.Values)
        {
            if (_visuals.CellOf(before.Parent, before.Position) is { } cell)
                cells.Add(cell);
        }

        var visuals = _visuals.Changed(document.Map, document.Visuals, _touched.Concat(detailed ?? []), cells, removed);

        _touched.Clear();
        _destroyed.Clear();
        _before.Clear();
        _linksChanged.Clear();
        _linksEntered.Clear();
        var air = _airChanged.Where(_entMan.EntityExists).Select(uid => uid.Id).Order().ToArray();
        _airChanged.Clear();
        return new SceneDelta(document.Id, tiles, entities, removed.Distinct().ToArray(), grids, History(document), created, decals ?? [], createdDecals ?? [], details, linked, air, _log.Drain(), visuals);
    }

    private void TouchDescendants(EntityUid uid)
    {
        var children = _entMan.GetComponent<TransformComponent>(uid).ChildEnumerator;
        while (children.MoveNext(out var child))
        {
            _touched.Add(child);
            TouchDescendants(child);
        }
    }

    private void AddInnermostFirst(EntityUid uid, List<int> into)
    {
        var children = _entMan.GetComponent<TransformComponent>(uid).ChildEnumerator;
        while (children.MoveNext(out var child))
            AddInnermostFirst(child, into);
        into.Add(uid.Id);
    }

    private void AddDescendants(EntityUid uid, HashSet<int> into)
    {
        var children = _entMan.GetComponent<TransformComponent>(uid).ChildEnumerator;
        while (children.MoveNext(out var child))
        {
            into.Add(child.Id);
            AddDescendants(child, into);
        }
    }

    private static HistoryState History(Document document) => new(
        document.Undo.Count > 0,
        document.Redo.Count > 0,
        document.Revision,
        document.Undo.Count > 0 ? document.Undo[^1].Label : null,
        document.Redo.Count > 0 ? document.Redo[^1].Label : null);

    // ---- saving ----

    /// <summary>Writes the document with the game's own serializer.</summary>
    public SaveResult Save(int documentId)
    {
        var document = Get(documentId);
        _log.Drain();
        var sw = Stopwatch.StartNew();
        _entMan.CullRemovedComponents();

        using var writer = new StringWriter { NewLine = "\n" };
        bool saved;
        try
        {
            if (document.Category == FileCategory.Grid)
            {
                var grids = new List<SceneGrid>();
                Collect(document.Map, grids, []);
                if (grids.Count != 1)
                    return new SaveResult(false, $"A grid file holds exactly one grid; this document has {grids.Count}.", null, sw.ElapsedMilliseconds, _log.Drain());

                saved = _loader.TrySaveGrid(new EntityUid(grids[0].Uid), writer);
            }
            else
            {
                saved = _loader.TrySaveMap(document.Map, writer);
            }
        }
        catch (Exception e)
        {
            return new SaveResult(false, $"{e.GetType().Name}: {e.Message}", null, sw.ElapsedMilliseconds, _log.Drain());
        }

        if (!saved)
            return new SaveResult(false, "The engine failed to save the document.", null, sw.ElapsedMilliseconds, _log.Drain());

        // The engine writes platform line endings regardless of the writer's setting.
        return new SaveResult(true, null, writer.ToString().Replace("\r\n", "\n"), sw.ElapsedMilliseconds, _log.Drain());
    }

    // ---- helpers ----

    private Document Get(int documentId) =>
        _documents.TryGetValue(documentId, out var document)
            ? document
            : throw new KernelException($"No open document with id {documentId}.");

    /// <summary>Resolves a uid from the interface, refusing entities outside the document.</summary>
    private EntityUid Owned(Document document, int id)
    {
        var uid = new EntityUid(id);
        if (!_entMan.TryGetComponent(uid, out TransformComponent? xform))
            throw new KernelException($"Entity {id} does not exist.");

        if (uid != document.Map && xform.MapUid != document.Map)
            throw new KernelException($"Entity {id} is not part of document {document.Id}.");

        return uid;
    }

    /// <summary>Reads the file kind from the header without loading anything.</summary>
    private static (FileCategory Category, int Version, string? EngineVersion) PeekHeader(string yaml)
    {
        try
        {
            using var reader = new StringReader(yaml);
            var root = DataNodeParser.ParseYamlStream(reader).FirstOrDefault()?.Root as MappingDataNode;
            if (root == null || !root.TryGet("meta", out MappingDataNode? meta))
                return (FileCategory.Unknown, 0, null);

            var category = FileCategory.Unknown;
            if (meta.TryGet("category", out ValueDataNode? categoryNode))
                Enum.TryParse(categoryNode.Value, out category);

            var version = 0;
            if (meta.TryGet("format", out ValueDataNode? formatNode))
                int.TryParse(formatNode.Value, out version);

            meta.TryGet("engineVersion", out ValueDataNode? engineNode);
            return (category, version, engineNode?.Value);
        }
        catch
        {
            return (FileCategory.Unknown, 0, null);
        }
    }
}

/// <summary>A failure the caller should show to the user, with whatever the engine logged.</summary>
public sealed class KernelException(string message, IReadOnlyList<KernelLogEntry>? log = null) : Exception(message)
{
    public IReadOnlyList<KernelLogEntry> Log { get; } = log ?? [];
}
