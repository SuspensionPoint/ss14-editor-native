using System.Diagnostics;
using Content.Shared.Decals;
using Content.Shared.DeviceLinking;
using Content.Shared.Maps;
using Content.Shared.Tag;
using Robust.Shared.ContentPack;
using Robust.Shared.EntitySerialization;
using Robust.Shared.EntitySerialization.Components;
using Robust.Shared.EntitySerialization.Systems;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;
using Robust.Shared.Log;
using Robust.Shared.Map;
using Robust.Shared.Prototypes;
using Robust.Shared.Serialization.TypeSerializers.Implementations;
using Robust.Shared.Utility;
using Robust.UnitTesting;
using Robust.UnitTesting.Server;

namespace SS14.EditorKernel;

/// <summary>
/// A headless SS14 engine with the fork's content loaded: real prototypes, real components and
/// the real map loader, but no game systems running. This is the editor's source of truth for
/// anything the game itself decides.
/// </summary>
public sealed class EditorKernel
{
    private readonly ISimulation _sim;
    private readonly CollectingLogHandler _log;
    private readonly IEntityManager _entMan;
    private readonly MapLoaderSystem _loader;

    public IPrototypeManager Prototypes { get; }
    public ITileDefinitionManager Tiles { get; }

    /// <summary>Map and grid files held open as live engine entities.</summary>
    public DocumentManager Documents { get; }

    /// <summary>How long each boot phase took, for measurements.</summary>
    public IReadOnlyList<(string Phase, long Milliseconds)> BootTimings { get; }

    /// <summary>The components the cleanup on delete expects and the fork does not have, warned of at start.</summary>
    public IReadOnlyList<string> UnknownListingComponents { get; }

    private EditorKernel(ISimulation sim, CollectingLogHandler log, List<(string, long)> timings)
    {
        _sim = sim;
        _log = log;
        _entMan = sim.Resolve<IEntityManager>();
        _loader = _entMan.System<MapLoaderSystem>();
        Prototypes = sim.Resolve<IPrototypeManager>();
        Tiles = sim.Resolve<ITileDefinitionManager>();
        var components = new ComponentEditor(_entMan, sim.Resolve<IComponentFactory>(), sim.Resolve<Robust.Shared.Serialization.Manager.ISerializationManager>(), Prototypes);
        Documents = new DocumentManager(
            _entMan,
            Tiles,
            Prototypes,
            _loader,
            components,
            log);
        BootTimings = timings;

        // The cleanup on delete names its components. One the fork lacks is warned of once, so
        // that a rename in the fork is noticed.
        UnknownListingComponents = components.UnknownListing();
        var sawmill = sim.Resolve<ILogManager>().GetSawmill("editor");
        foreach (var name in UnknownListingComponents)
            sawmill.Warning($"The fork has no component {name}. References to deleted entities in it are not cleared.");
    }

    /// <summary>
    /// Boots the engine. <paramref name="roots"/> are mounted at the resource root in order, and
    /// must contain the engine's and the fork's <c>Resources</c> content (at least prototypes).
    /// With a <paramref name="prototypeCache"/>, prototype files come from the cache already
    /// parsed and the roots only need to supply everything else (such as migration.yml).
    /// </summary>
    public static EditorKernel Boot(
        IEnumerable<IContentRoot> roots,
        Action<string>? progress = null,
        PrototypeCache? prototypeCache = null)
    {
        var timings = new List<(string, long)>();
        var sw = Stopwatch.StartNew();
        void Phase(string name)
        {
            timings.Add((name, sw.ElapsedMilliseconds));
            sw.Restart();
        }

        var contentShared = typeof(Content.Shared.Entry.EntryPoint).Assembly;
        var contentServer = typeof(Content.Server.Entry.EntryPoint).Assembly;
        var log = new CollectingLogHandler();

        // The harness hands tests a mocked reflection manager that cannot resolve YAML type
        // tags. Prototypes need the real one, but the real one would also discover every
        // content entity system. Wrap it so system discovery stays opt-in.
        NoSystemDiscoveryReflection? reflection = null;

        progress?.Invoke("Starting engine");
        var sim = RobustServerSimulation.NewSimulation()
            .RegisterReflection(real =>
            {
                real.LoadAssemblies(ContentAssemblies.ForServer(contentShared, contentServer));
                reflection = new NoSystemDiscoveryReflection(real);
            })
            .RegisterDependencies(deps =>
            {
                deps.RegisterInstance<Robust.Shared.Reflection.IReflectionManager>(reflection!, overwrite: true);
                // The game's device link system writes to the admin log. This one keeps nothing.
                deps.Register<Content.Shared.Administration.Logs.ISharedAdminLogManager, Content.Shared.Administration.Logs.SharedAdminLogManager>();
                if (OperatingSystem.IsBrowser())
                    BrowserServices.Replace(deps);
            })
            // The harness's log handler fails a test on any error. The kernel reports errors
            // to its caller instead (see DrainLog), so raise that threshold out of the way.
            .ChangeCVar(cfg =>
            {
                cfg.SetCVar(RTCVars.FailureLogLevel, LogLevel.Fatal);
                // A game server deletes a grid the moment its last tile goes. In an editor that
                // tile may be about to come back (an undo, or the next stroke of the brush),
                // and everything that referred to the grid would be left pointing at nothing.
                cfg.SetCVar(RTCVars.GameDeleteEmptyGrids, false);
                // Likewise a game server makes a second grid out of any tiles that are no longer
                // joined to the rest (a ship broken in two). A mapper who paints a tile one cell
                // away from a floor means it for the grid being edited, and an undo has to find
                // it there.
                cfg.SetCVar(RTCVars.GridSplitting, false);
            })
            .RegisterComponents(factory =>
            {
                // The harness has already registered a handful of engine components by hand,
                // and DoAutoRegistrations throws on duplicates, so register only the rest.
                var missing = reflection!.FindTypesWithAttribute<RegisterComponentAttribute>()
                    .Where(type => !factory.TryGetRegistration(type, out _))
                    .ToArray();
                factory.RegisterTypes(missing);

                // Client-only components the real server ignores. The engine's list lives in
                // the internal ServerComponentFactory, which the harness does not use.
                factory.RegisterIgnore("Input", "AnimationPlayer", "GenericVisualizer", "Sprite");

                // Same client-only exclusions the real server declares in Content.Server's EntryPoint.
                factory.IgnoreMissingComponents("Visuals");
                factory.RegisterIgnore(Content.Server.Entry.IgnoredComponents.List);
                Phase("engine services");
            })
            .RegisterEntitySystems(systems =>
            {
                systems.LoadExtraSystemType<MapLoaderSystem>();
                systems.LoadExtraSystemType<DocumentTrackingSystem>();
                // Applies prototype renames from Resources/migration.yml while maps load.
                systems.LoadExtraSystemType<Content.Server.Maps.MapMigrationSystem>();
                // The game's device link system, for its methods and none of its handlers.
                systems.LoadExtraSystemType<EditorPopupSystem>();
                systems.LoadExtraSystemType<EditorDeviceLinkSystem>();
            })
            .AddRoot(res =>
            {
                Phase("entity systems");
                if (prototypeCache != null)
                {
                    res.AddRoot(ResPath.Root, new PrototypeCacheRoot(prototypeCache));
                    PrototypeCache.SetEngineSource(prototypeCache.TryGetDocuments);
                }

                foreach (var root in roots)
                    res.AddRoot(ResPath.Root, root);
            })
            .RegisterPrototypes(protoMan =>
            {
                // The harness registers these two kinds itself and throws on duplicates, so
                // prototype kind discovery is hidden from it and the rest are registered here.
                var kinds = reflection!.Inner.GetAllChildren<IPrototype>()
                    .Where(kind => kind != typeof(EntityPrototype) && kind != typeof(EntityCategoryPrototype))
                    .ToArray();
                protoMan.RegisterKind(kinds);
                protoMan.RegisterIgnore("parallax");
                // And the other kinds only the client has, as each module's entry point ignores them.
                foreach (var kind in ContentAssemblies.OtherSideKinds(contentServer, ContentAssemblies.ServerEngine, ContentAssemblies.ClientEngine))
                    protoMan.RegisterIgnore(kind);
                // The fork's partial and ignored prototype folders, as its entry point sets them.
                ForkPrototypeRules.Apply(protoMan, IoCManager.Resolve<IResourceManager>());

                progress?.Invoke("Reading prototypes");
                protoMan.LoadDirectory(new ResPath("/EnginePrototypes"));
                protoMan.LoadDirectory(new ResPath("/Prototypes"));
                // The cache is only for this first load; later reloads parse text as usual.
                if (prototypeCache != null)
                    PrototypeCache.SetEngineSource(null);
                Phase(prototypeCache != null ? "read prototype cache" : "read prototype files");
                progress?.Invoke("Resolving prototypes");
            })
            .InitializeInstance();
        Phase("resolve prototypes");

        sim.Resolve<ILogManager>().RootSawmill.AddHandler(log);

        var kernel = new EditorKernel(sim, log, timings);
        kernel.RegisterTileDefinitions();
        Phase("tile definitions");
        progress?.Invoke("Engine ready");
        return kernel;
    }

    /// <summary>Mirrors Content.Shared's TurfSystem.RegisterTileDefinitions, which is private.</summary>
    private void RegisterTileDefinitions()
    {
        Tiles.Register(Prototypes.Index<ContentTileDefinition>(ContentTileDefinition.SpaceID));
        var others = Prototypes.EnumeratePrototypes<ContentTileDefinition>()
            .Where(t => t.ID != ContentTileDefinition.SpaceID)
            .OrderBy(t => t.ID, StringComparer.Ordinal);
        foreach (var tile in others)
            Tiles.Register(tile);
        Tiles.Initialize();
    }

    /// <summary>Engine log messages at warning level or above since the last call.</summary>
    public IReadOnlyList<KernelLogEntry> DrainLog() => _log.Drain();

    public PrototypeSummary GetPrototypeSummary()
    {
        var kinds = Prototypes.EnumeratePrototypeKinds().ToArray();
        return new PrototypeSummary(
            kinds.Length,
            kinds.Sum(k => Prototypes.EnumeratePrototypes(k).Count()),
            Prototypes.EnumeratePrototypes<EntityPrototype>().Count(),
            Prototypes.EnumeratePrototypes<ContentTileDefinition>().Count());
    }

    /// <summary>Tile definitions as the engine resolved them (inheritance applied).</summary>
    public IReadOnlyList<TileInfo> GetTiles()
    {
        return Prototypes.EnumeratePrototypes<ContentTileDefinition>()
            .OrderBy(t => t.ID, StringComparer.Ordinal)
            .Select(t => new TileInfo(t.ID, t.Variants, t.Sprite?.ToString(), t.IsSubFloor, t.BaseTurf, t.AllowRotationMirror, t.CanCrowbar))
            .ToArray();
    }

    /// <summary>Entity prototypes as the engine resolved them (inheritance applied).</summary>
    public IReadOnlyList<EntityPrototypeInfo> GetEntityPrototypes()
    {
        var factory = _sim.Resolve<IComponentFactory>();
        return Prototypes.EnumeratePrototypes<EntityPrototype>()
            .OrderBy(p => p.ID, StringComparer.Ordinal)
            .Select(p => new EntityPrototypeInfo(
                p.ID,
                p.Name,
                p.Description,
                p.EditorSuffix,
                p.Parents ?? [],
                p.Categories.Select(c => c.ID).OrderBy(id => id, StringComparer.Ordinal).ToArray(),
                p.Abstract,
                p.HideSpawnMenu,
                p.PlacementMode,
                p.Components.Keys.OrderBy(name => name, StringComparer.Ordinal).ToArray(),
                p.TryComp<TagComponent>(out var tag, factory)
                    ? tag.Tags.Select(id => id.Id).OrderBy(id => id, StringComparer.Ordinal).ToArray()
                    : [],
                p.TryComp<TransformComponent>(out var xform, factory) && xform.Anchored,
                TubeExits(p, factory)))
            .ToArray();
    }

    /// <summary>
    /// A disposal tube's exits, as its component gives them. None on a fork whose tubes keep
    /// them otherwise (src/Directory.Build.props).
    /// </summary>
    private static int[]? TubeExits(EntityPrototype p, IComponentFactory factory)
    {
#if FORK_DISPOSAL_EXITS
        return p.TryComp<Content.Shared.Disposal.Tube.DisposalTubeComponent>(out var tube, factory)
            ? tube.Exits.Select(exit => (int) exit).ToArray()
            : null;
#else
        return null;
#endif
    }

    /// <summary>
    /// The game's device ports, and which of them each entity prototype has. A port's name and
    /// description are given as the game's localisation keys: the kernel loads no texts.
    /// </summary>
    public PortFacts GetPorts()
    {
        var factory = _sim.Resolve<IComponentFactory>();
        var prototypes = new List<PrototypePorts>();
        foreach (var prototype in Prototypes.EnumeratePrototypes<EntityPrototype>().OrderBy(p => p.ID, StringComparer.Ordinal))
        {
            prototype.TryComp<DeviceLinkSourceComponent>(out var source, factory);
            prototype.TryComp<DeviceLinkSinkComponent>(out var sink, factory);
            if ((source?.Ports.Count ?? 0) + (sink?.Ports.Count ?? 0) == 0)
                continue;

            prototypes.Add(new PrototypePorts(
                prototype.ID,
                source == null ? [] : source.Ports.Select(port => port.Id).OrderBy(id => id, StringComparer.Ordinal).ToArray(),
                sink == null ? [] : sink.Ports.Select(port => port.Id).OrderBy(id => id, StringComparer.Ordinal).ToArray()));
        }

        return new PortFacts(
            Prototypes.EnumeratePrototypes<Content.Shared.DeviceLinking.SourcePortPrototype>()
                .OrderBy(port => port.ID, StringComparer.Ordinal)
                .Select(port => new PortInfo(port.ID, port.Name, port.Description,
                    port.DefaultLinks?.Select(sink => ForkCompat.IdOf(sink)).OrderBy(id => id, StringComparer.Ordinal).ToArray()))
                .ToArray(),
            Prototypes.EnumeratePrototypes<Content.Shared.DeviceLinking.SinkPortPrototype>()
                .OrderBy(port => port.ID, StringComparer.Ordinal)
                .Select(port => new PortInfo(port.ID, port.Name, port.Description, null))
                .ToArray(),
            prototypes);
    }

    /// <summary>
    /// The fork's draw depths by name (Content.Shared/DrawDepth/DrawDepth.cs), lowest first.
    /// Forks number them differently: Space Wizards draw walls at -8, Starlight at -2. The
    /// editor sorts entities onto its layer switches by these names (editor-core draw_depths.rs).
    /// </summary>
    public IReadOnlyList<DrawDepthInfo> GetDrawDepths()
    {
        var type = typeof(Content.Shared.DrawDepth.DrawDepth);
        return Enum.GetNames(type)
            .Select(name => new DrawDepthInfo(name, Convert.ToInt32(Enum.Parse(type, name), System.Globalization.CultureInfo.InvariantCulture)))
            .OrderBy(depth => depth.Value)
            .ThenBy(depth => depth.Name, StringComparer.Ordinal)
            .ToArray();
    }

    /// <summary>Decal prototypes as the engine resolved them (inheritance applied).</summary>
    public IReadOnlyList<DecalPrototypeInfo> GetDecalPrototypes()
    {
        return Prototypes.EnumeratePrototypes<DecalPrototype>()
            .OrderBy(d => d.ID, StringComparer.Ordinal)
            .Select(d =>
            {
                var (path, state) = Picture(d.Sprite);
                return new DecalPrototypeInfo(
                    Id: d.ID,
                    SpritePath: path,
                    SpriteState: state,
                    Tags: d.Tags.ToArray(),
                    ShowMenu: d.ShowMenu,
                    SnapCardinals: d.SnapCardinals,
                    DefaultCustomColor: d.DefaultCustomColor,
                    DefaultSnap: d.DefaultSnap,
                    DefaultCleanable: d.DefaultCleanable);
            })
            .ToArray();
    }

    /// <summary>
    /// A picture as the path and state a draw list names that texture by (TextureSource in
    /// DrawRecorder.cs): for a state of an RSI, the path the client loads the RSI from and
    /// the state's name, and for an image, its path and no state. Neither for anything else.
    /// </summary>
    private static (string? Path, string? State) Picture(SpriteSpecifier sprite)
    {
        return sprite switch
        {
            _ when sprite.Equals(SpriteSpecifier.Invalid) => (null, null),
            SpriteSpecifier.Rsi rsi => ((SpriteSpecifierSerializer.TextureRoot / rsi.RsiPath).ToString(), rsi.RsiState),
            SpriteSpecifier.Texture texture => ((SpriteSpecifierSerializer.TextureRoot / texture.TexturePath).ToString(), null),
            _ => (null, null),
        };
    }

    /// <summary>
    /// Loads a map with the game's own loader and reports what the engine made of it, without
    /// keeping it. This is the check for "will the game accept this file".
    /// </summary>
    public MapReport Validate(string yaml, string source)
    {
        _log.Drain();
        var sw = Stopwatch.StartNew();
        if (!TryLoad(yaml, source, out var result, out var failure))
            return MapReport.Failed(failure, _log.Drain(), sw.ElapsedMilliseconds);

        var report = Describe(result!, sw.ElapsedMilliseconds);
        _loader.Delete(result!);
        return report;
    }

    /// <summary>
    /// Diagnostic: loads a map and reports the live managed heap (in MB, after a full collection)
    /// before the load, while the map is loaded, and after it has been deleted again.
    /// </summary>
    public (long Before, long Loaded, long After, bool Ok) MeasureLoad(string yaml, string source)
    {
        static long LiveMb() => GC.GetTotalMemory(forceFullCollection: true) / (1024 * 1024);

        var before = LiveMb();
        if (!TryLoad(yaml, source, out var result, out _))
            return (before, LiveMb(), LiveMb(), false);

        var loaded = LiveMb();
        _loader.Delete(result!);
        _entMan.CullRemovedComponents();
        return (before, loaded, LiveMb(), true);
    }

    /// <summary>
    /// Loads a map and writes it back out with the game's own serializer, using the same
    /// options and save logic as the game's <c>resave</c> command. On a fork that keeps its
    /// decals on chunk entities, the decals get what the game's decal system does to them as a
    /// map loads first, as a document's do as it opens (DocumentManager.StartDecals).
    /// </summary>
    public MapSaveResult Resave(string yaml, string source)
    {
        _log.Drain();
        var sw = Stopwatch.StartNew();
        if (!TryLoad(yaml, source, out var result, out var failure))
            return new MapSaveResult(MapReport.Failed(failure, _log.Drain(), sw.ElapsedMilliseconds), null);

        var loadMs = sw.ElapsedMilliseconds;
#if FORK_DECAL_CHUNK_ENTITIES
        foreach (var map in result!.Maps)
            Documents.StartDecals(map.Owner);
        foreach (var grid in result.Grids)
            Documents.StartDecals(grid.Owner);
#endif
        _entMan.CullRemovedComponents();

        using var writer = new StringWriter { NewLine = "\n" };
        var saved = false;
        string? saveFailure = null;
        try
        {
            var mapEnt = result!.Maps.FirstOrDefault();
            if (mapEnt.Owner.IsValid() && _entMan.HasComponent<LoadedMapComponent>(mapEnt))
                saved = _loader.TrySaveMap(mapEnt.Owner, writer);
            else if (result.Grids.Count == 1)
                saved = _loader.TrySaveGrid(result.Grids.First().Owner, writer);
            else
                saveFailure = $"Files with {result.Maps.Count} maps and {result.Grids.Count} grids are not supported by the engine's resave logic.";
        }
        catch (Exception e)
        {
            saveFailure = e.Message;
        }

        var report = Describe(result!, loadMs);
        _loader.Delete(result!);

        if (!saved)
        {
            return new MapSaveResult(
                report with { Ok = false, Failure = saveFailure ?? "The engine failed to save the map." },
                null);
        }

        // The engine writes platform line endings regardless of the writer's setting.
        return new MapSaveResult(report, writer.ToString().Replace("\r\n", "\n"));
    }

    private bool TryLoad(string yaml, string source, out LoadResult? result, out string failure)
    {
        // Same options as the game's own `resave` command (Content.Server/Maps/ResaveCommand.cs).
        var options = MapLoadOptions.Default with
        {
            DeserializationOptions = DeserializationOptions.Default with
            {
                StoreYamlUids = true,
                LogOrphanedGrids = false,
            },
        };

        failure = "";
        result = null;
        try
        {
            using var reader = new StringReader(yaml);
            if (_loader.TryLoadGeneric(reader, source, out result, options))
                return true;

            failure = "The engine's map loader rejected the file.";
            return false;
        }
        catch (Exception e)
        {
            failure = $"{e.GetType().Name}: {e.Message}";
            return false;
        }
    }

    private MapReport Describe(LoadResult result, long milliseconds)
    {
        return new MapReport(
            true,
            null,
            result.Category.ToString(),
            result.Version,
            result.EngineVersion,
            result.Entities.Count,
            result.Maps.Count,
            result.Grids.Count,
            result.Orphans.Count,
            result.NullspaceEntities.Count,
            milliseconds,
            _log.Drain());
    }
}

public sealed record PrototypeSummary(int Kinds, int Prototypes, int Entities, int Tiles);

public sealed record TileInfo(
    string Id,
    int Variants,
    string? Sprite,
    bool IsSubFloor,
    string? BaseTurf,
    /// <summary>
    /// Whether the game draws this tile turned and mirrored by a tile's rotation byte
    /// (allowRotationMirror). Only for such tiles does turning a region turn the byte.
    /// </summary>
    bool AllowsRotation,
    /// <summary>Whether a crowbar lifts it (canCrowbar), as TileSystem.PryTile asks.</summary>
    bool CanCrowbar = false);

public sealed record MapReport(
    bool Ok,
    string? Failure,
    string? Category,
    int FormatVersion,
    string? EngineVersion,
    int Entities,
    int Maps,
    int Grids,
    int Orphans,
    int Nullspace,
    long Milliseconds,
    IReadOnlyList<KernelLogEntry> Log)
{
    public static MapReport Failed(string failure, IReadOnlyList<KernelLogEntry> log, long milliseconds) =>
        new(false, failure, null, 0, null, 0, 0, 0, 0, 0, milliseconds, log);
}

public sealed record MapSaveResult(MapReport Report, string? Yaml);
