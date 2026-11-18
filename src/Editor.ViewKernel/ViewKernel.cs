using System.Reflection;
using Content.Client.Parallax;
using Content.Client.Parallax.Data;
using Content.Client.Parallax.Managers;
using Content.EditorView;
using Moq;
using Robust.Client;
using Robust.Client.Graphics;
using Robust.Client.State;
using Robust.Client.UserInterface.XAML.Proxy;
using Robust.Shared;
using Robust.Shared.Configuration;
using Robust.Shared.ContentPack;
using Robust.Shared.IoC;
using Robust.Shared.Log;
using Robust.Shared.Prototypes;
using Robust.Shared.Threading;
using Robust.Shared.Timing;
using Robust.Shared.Utility;
using SS14.EditorKernel;

namespace SS14.EditorViewKernel;

/// <summary>
/// The game client with no window, renderer, sound or server, started inside this process and
/// stepped by hand. It exists to answer one question, "what would the client draw for this map?".
/// It holds a copy of the document as client entities and records what the engine's sprite code
/// asks to draw (see DrawRecorder).
///
/// It starts the way the engine's own integration tests start a headless client
/// (RobustIntegrationTest.ClientIntegrationInstance): the client's services, a few of them swapped
/// for stand-ins, then the two halves of the game controller's start-up. The fork's own entry
/// points run as they do in the game, so every one of its client systems is present and what is
/// shown is decided by the same code as in the native engine view.
///
/// A smaller world, with the engine's systems and only a hand-picked few of the fork's, was tried
/// first. A fifth of a station then looked different, because doors, cables under floors, alarms
/// and so on are each dressed by one of the fork's systems, and those systems depend on each other
/// too widely to pick from.
/// </summary>
public sealed class ViewKernel
{
    private readonly GameController _client;
    private readonly IGameTiming _timing;
    private readonly IPrototypeManager _prototypes;
    private readonly IResourceManager _resources;
    private readonly CollectingLogHandler _log;
    private readonly ViewWorld _world;
    private readonly MethodInfo _tick;
    private readonly MethodInfo _update;

    public IReadOnlyList<(string Phase, long Milliseconds)> BootTimings { get; }

    /// <summary>Set when starting with a trace: also told where a call has got to.</summary>
    private Action<string>? _trace;

    private ViewKernel(GameController client, IDependencyCollection deps, CollectingLogHandler log, List<(string, long)> timings)
    {
        _client = client;
        _log = log;
        _timing = deps.Resolve<IGameTiming>();
        _prototypes = deps.Resolve<IPrototypeManager>();
        _resources = deps.Resolve<IResourceManager>();
        _world = new ViewWorld();
        BootTimings = timings;

        // The two halves of a frame, which the game loop normally calls.
        const BindingFlags flags = BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public;
        _tick = typeof(GameController).GetMethod("Tick", flags, [typeof(FrameEventArgs)])
                ?? throw new InvalidOperationException("GameController.Tick(FrameEventArgs) has moved.");
        _update = typeof(GameController).GetMethod("Update", flags, [typeof(FrameEventArgs)])
                  ?? throw new InvalidOperationException("GameController.Update(FrameEventArgs) has moved.");
    }

    /// <param name="roots">The engine's and the fork's Resources folders, in whatever form.</param>
    /// <param name="prototypeCache">Pre-parsed prototype files (the WebAssembly build), or null to parse the YAML in the roots.</param>
    /// <param name="trace">Given every engine log message while starting, at any level.</param>
    public static ViewKernel Boot(IEnumerable<IContentRoot> roots, Action<string>? progress = null, PrototypeCache? prototypeCache = null, Action<string>? trace = null)
    {
        var timings = new List<(string, long)>();
        var sw = System.Diagnostics.Stopwatch.StartNew();
        void Phase(string name)
        {
            timings.Add((name, sw.ElapsedMilliseconds));
            sw.Restart();
        }

        progress?.Invoke("Starting the client");
        var deps = new DependencyCollection();
        IoCManager.InitThread(deps, replaceExisting: true);
        ClientIoC.RegisterIoC(GameController.DisplayMode.Headless, deps);

        // Content is already loaded into this process; nothing is read from disk or checked.
        deps.Register<IModLoader, LoadedAssembliesModLoader>(overwrite: true);
        deps.Register<IModLoaderInternal, LoadedAssembliesModLoader>(overwrite: true);
        deps.Register<LoadedAssembliesModLoader, LoadedAssembliesModLoader>(overwrite: true);
        deps.Register<IXamlProxyManager, XamlProxyManagerStub>(overwrite: true);
        deps.Register<IXamlHotReloadManager, XamlHotReloadManagerStub>(overwrite: true);

        // Fonts go through a native library, and nothing here draws text.
        var fonts = new Mock<IFontManagerInternal> { DefaultValue = DefaultValue.Mock }.Object;
        deps.RegisterInstance<IFontManager>(fonts, overwrite: true);
        deps.RegisterInstance<IFontManagerInternal>(fonts, overwrite: true);

        if (OperatingSystem.IsBrowser())
            BrowserStandIns.Register(deps);

        // An engine built for an older .NET runs on a newer runtime when its own is not there
        // (Editor.Native.csproj RollForward), where its network serializer refuses types the
        // runtime has since changed (BitArray on .NET 10, for RMC-14's .NET 9). The view sends
        // nothing over a network, so it starts with a serializer that does nothing then.
        if (Environment.Version.Major > EngineDotnet())
            deps.RegisterInstance<Robust.Shared.Serialization.IRobustSerializer>(new Mock<Robust.Shared.Serialization.IRobustSerializer>().Object, overwrite: true);

        deps.BuildGraph();
        GameController.RegisterReflection(deps);
        Phase("client services");

        deps.Resolve<LoadedAssembliesModLoader>().Assemblies =
        [
            // Content.Shared, Content.Client and the fork's modules beside them, if any.
            .. ContentAssemblies.ForClient(typeof(Content.Shared.Entry.EntryPoint).Assembly, typeof(Content.Client.Entry.EntryPoint).Assembly),
            // This assembly too, last: its entry point adjusts what the fork's set up.
            typeof(ViewKernel).Assembly,
        ];
#if FORK_CLIENT_SERVICES_AT_INIT
        // The fork's client registers its services in its entry point's Init, after every
        // PreInit, where ViewKernelEntryPoint would put in its stand-in for the backdrop, and
        // refuses one already there. The stand-in goes in right after them instead, by the
        // hook the game gives its tests (src/Directory.Build.props).
        deps.Resolve<LoadedAssembliesModLoader>().SetModuleBaseCallbacks(new Content.Client.IoC.ClientModuleTestingCallbacks
        {
            ClientBeforeIoC = () => deps.Register<IParallaxManager, NoParallax>(overwrite: true),
        });
#endif

        // Resources are given rather than found next to the executable. A development build of
        // the engine insists on mounting "<engine root>/Resources" from disk all the same, so
        // it is pointed at an empty folder (which also exists in a browser's in-memory files).
        var scratch = Path.Combine(Path.GetTempPath(), "editor-view-kernel");
        Directory.CreateDirectory(Path.Combine(scratch, "RobustToolbox", "Resources"));
        Directory.CreateDirectory(Path.Combine(scratch, "a", "b"));
        ProgramShared.PathOffset = Path.Combine(scratch, "a", "b") + "/";

        // Nothing here looks at a pixel, so sprite sheets are laid out from their descriptions
        // and their images are never read (engine patch 0004). The client loads every sprite
        // that any entity prototype uses as it starts: some fifty thousand images otherwise.
        var layoutOnly = typeof(Robust.Client.ResourceManagement.RSIResource).GetField("LayoutOnly", BindingFlags.Public | BindingFlags.Static);
        if (layoutOnly != null)
            layoutOnly.SetValue(null, true);
        else
            progress?.Invoke("Engine patch 0004 is not applied: every sprite image will be read");

        var resources = deps.Resolve<IResourceManagerInternal>();
        if (prototypeCache != null)
        {
            resources.AddRoot(ResPath.Root, new PrototypeCacheRoot(prototypeCache));
            PrototypeCache.SetEngineSource(prototypeCache.TryGetDocuments);
        }

        foreach (var root in roots)
            resources.AddRoot(ResPath.Root, root);

        var config = deps.Resolve<IConfigurationManagerInternal>();
        var log = new CollectingLogHandler { Trace = trace };
        var client = deps.Resolve<GameController>();
        client.ContentStart = true;

        progress?.Invoke("Starting the engine");
        if (!client.StartupSystemSplash(new GameControllerOptions { LoadContentResources = false, LoadConfigAndUserData = false }, () => log))
            throw new InvalidOperationException("The client did not start (first half).");

        config.OverrideConVars(
        [
            (CVars.DiscordEnabled.Name, "false"),
            (CVars.ResTexturePreloadingEnabled.Name, "false"),
            (CVars.SysGCCollectStart.Name, "false"),
            (CVars.ResPrototypeReloadWatch.Name, "false"),
            (CVars.ResCheckBadFileExtensions.Name, "false"),
        ]);
        Phase("engine start");

        progress?.Invoke("Reading prototypes");
        if (!client.StartupContinue(GameController.DisplayMode.Headless))
            throw new InvalidOperationException("The client did not start (second half).");
        // The cache is only for this first load; later reloads parse text as usual.
        if (prototypeCache != null)
            PrototypeCache.SetEngineSource(null);
        Phase("content and prototypes");

        progress?.Invoke("Client ready");
        return new ViewKernel(client, deps, log, timings) { _trace = trace };
    }

    /// <summary>The major version of .NET the engine was built for, or of the runtime when it does not say.</summary>
    private static int EngineDotnet()
    {
        var framework = typeof(GameController).Assembly.GetCustomAttribute<System.Runtime.Versioning.TargetFrameworkAttribute>()?.FrameworkName;
        var version = framework?.Split("Version=v").ElementAtOrDefault(1);
        return Version.TryParse(version, out var parsed) ? parsed.Major : Environment.Version.Major;
    }

    /// <summary>
    /// Engine log messages at warning level or above since the last call. A client with no
    /// server complains each time one of the fork's systems tries to ask the server something
    /// as it starts; those are left out, since here that is how things are meant to be.
    /// </summary>
    public IReadOnlyList<KernelLogEntry> DrainLog() =>
        _log.Drain().Where(entry => !(entry.Sawmill == "net" && entry.Message.StartsWith("Tried to send message while not connected", StringComparison.Ordinal))).ToArray();

    /// <summary>
    /// Runs a few frames of the client. Wall smoothing, appearance changes and the like are
    /// queued when something is placed and worked off in the next frames, which nothing drives
    /// here unless asked.
    /// </summary>
    public void Settle(int frames = 3)
    {
        var args = new FrameEventArgs(1 / 60f);
        for (var i = 0; i < frames; i++)
        {
            _trace?.Invoke($"frame {i + 1} of {frames}: tick");
            _tick.Invoke(_client, [args]);
            _timing.CurTick = new GameTick(_timing.CurTick.Value + 1);
            _trace?.Invoke($"frame {i + 1} of {frames}: update");
            _update.Invoke(_client, [args]);
        }

        _trace?.Invoke("frames done");
    }

    public void ApplyScene(Scene scene)
    {
        _trace?.Invoke("applying scene");
        _world.ApplyScene(scene);
        Settle();
    }

    public void ApplyDelta(SceneDelta delta)
    {
        _world.ApplyDelta(delta);
        Settle();
    }

    public void Close(int document) => _world.Close(document);

    public int ShownEntities(int document) => _world.EntityCount(document);

    /// <summary>The document as drawing data; see ViewWorld.BuildDrawList for the layout.</summary>
    public byte[]? BuildDrawList(int document, bool full) => _world.BuildDrawList(document, full);

    /// <summary>How entity prototypes look; see ViewWorld.BuildPreviews.</summary>
    public byte[] BuildPreviews(IReadOnlyList<(int Key, string Prototype)> requests, bool directions) =>
        _world.BuildPreviews(requests, directions);

    public void SetLayers(bool entities, bool markers, bool decals, IReadOnlyList<int> hiddenDepths, bool subFloor)
    {
        _world.SetLayers(entities, markers, decals, hiddenDepths);
        SubFloorView.Show(subFloor);
        _world.InvalidateDrawLists();
        Settle();
    }

    /// <summary>
    /// The layers of one of the game's backdrops (a "parallax" prototype), bottom first, as the
    /// game's ParallaxOverlay would draw them. Nothing is painted or loaded here: a layer is
    /// either an image in the resources or a recipe the game paints stars and dust from, and
    /// both are only named. A backdrop the fork does not have has no layers.
    /// </summary>
    public BackdropLayers Backdrop(string name)
    {
        // Animated layers are Starlight's own. Other forks have none (src/Directory.Build.props).
#if FORK_ANIMATED_BACKDROPS
        static bool IsAnimated(ParallaxLayerConfig layer) => layer.Sprite;
#else
        static bool IsAnimated(ParallaxLayerConfig layer) => false;
#endif

        var layers = new List<BackdropLayer>();
        var skipped = 0;
        if (!_prototypes.TryIndex<ParallaxPrototype>(name, out var prototype))
            return new BackdropLayers(layers, skipped);

        foreach (var layer in prototype.Layers)
        {
            string? image = null, generated = null, recipe = null;
            switch (layer.Texture)
            {
                // An animated layer (the fork's own addition) would need its frames; none of
                // the fork's backdrops has one.
                case ImageParallaxTextureSource source when !IsAnimated(layer):
                    image = source.Path.ToString();
                    break;
                case GeneratedParallaxTextureSource source when !IsAnimated(layer) && _resources.TryContentFileRead(source.ParallaxConfigPath, out var stream):
                    using (var reader = new StreamReader(stream, EncodingHelpers.UTF8))
                        recipe = reader.ReadToEnd().Replace(Environment.NewLine, "\n");
                    generated = source.Identifier;
                    break;
                default:
                    skipped++;
                    continue;
            }

            layers.Add(new BackdropLayer(
                image, generated, recipe,
                layer.Scale.X, layer.Scale.Y, layer.Tiled,
                layer.WorldHomePosition.X, layer.WorldHomePosition.Y,
                layer.WorldAdjustPosition.X, layer.WorldAdjustPosition.Y,
                layer.Slowness, layer.Scrolling.X, layer.Scrolling.Y));
        }

        return new BackdropLayers(layers, skipped);
    }

    /// <summary>The ids of every backdrop the fork has, in order whatever their case.</summary>
    public IReadOnlyList<string> BackdropNames() =>
        _prototypes.EnumeratePrototypes<ParallaxPrototype>().Select(prototype => prototype.ID).Order(StringComparer.OrdinalIgnoreCase).ToArray();
}

/// <summary>
/// One layer of a backdrop, with the numbers the game's ParallaxOverlay places it by. Exactly
/// one of <paramref name="Image"/> (a path in the resources) and <paramref name="Generated"/>
/// (the layer's id, with <paramref name="Recipe"/> the text of its recipe file) is set.
/// </summary>
public sealed record BackdropLayer(
    string? Image, string? Generated, string? Recipe,
    float ScaleX, float ScaleY, bool Tiled,
    float HomeX, float HomeY, float AdjustX, float AdjustY,
    float Slowness, float ScrollX, float ScrollY);

/// <param name="Skipped">Layers of a kind that is not shown here.</param>
public sealed record BackdropLayers(IReadOnlyList<BackdropLayer> Layers, int Skipped);

/// <summary>
/// Runs after the fork's own entry points, as the native engine view's does.
/// </summary>
public sealed class ViewKernelEntryPoint : GameClient
{
    public override void PreInit()
    {
        // The fork paints its space backdrop from noise when it starts, which takes seconds
        // of processor time and hundreds of megabytes, into textures that nothing here can
        // show. A host that wants the backdrop asks for its layers (ViewKernel.Backdrop) and
        // has only the ones a map uses painted, once. A fork whose client registers its services
        // later gets it then (ViewKernel.Boot).
#if !FORK_CLIENT_SERVICES_AT_INIT
        Dependencies.Register<IParallaxManager, NoParallax>(overwrite: true);
#endif
    }

    public override void PostInit()
    {
        // "Single player" is the engine's mode for a client that owns its own entities, and
        // the blank state replaces the main menu the fork opens.
        Dependencies.Resolve<IBaseClient>().StartSinglePlayer();
        Dependencies.Resolve<IStateManager>().RequestStateChange<BlankState>();

        // A mapping aid the game hides from players: marker entities such as spawn points.
        Dependencies.Resolve<Robust.Shared.GameObjects.IEntityManager>().System<Content.Client.Markers.MarkerSystem>().MarkersVisible = true;
    }
}

internal sealed class NoParallax : IParallaxManager
{
    public System.Numerics.Vector2 ParallaxAnchor { get; set; }
    public bool IsLoaded(string name) => false;
    public ParallaxLayerPrepared[] GetParallaxLayers(string name) => [];

    public void LoadDefaultParallax()
    {
    }

    public Task LoadParallaxByName(string name) => Task.CompletedTask;

    public void UnloadParallax(string name)
    {
    }
}

/// <summary>The client state the view sits in: nothing on screen, no input.</summary>
public sealed class BlankState : State
{
    protected override void Startup()
    {
    }

    protected override void Shutdown()
    {
    }
}

/// <summary>
/// Hands the engine content assemblies that this process has already loaded, as the engine's
/// test mod loader does. The real one reads them from the resources and verifies them.
/// </summary>
internal sealed class LoadedAssembliesModLoader : BaseModLoader, IModLoaderInternal
{
    public Assembly[] Assemblies { get; set; } = [];

    public bool TryLoadModulesFrom(ResPath mountPath, string filterPrefix) => TryLoadModules([]);

    public bool TryLoadModules(IEnumerable<ResPath> paths)
    {
        foreach (var assembly in Assemblies)
            InitMod(assembly);

        return true;
    }

    public void LoadGameAssembly(Stream assembly, Stream? symbols = null, bool skipVerify = false) => throw new NotSupportedException();
    public void LoadGameAssembly(string diskPath, bool skipVerify = false) => throw new NotSupportedException();
    public bool TryLoadAssembly(string assemblyName) => throw new NotSupportedException();

    public void SetUseLoadContext(bool useLoadContext)
    {
    }

    public void SetEnableSandboxing(bool sandboxing)
    {
    }

    public Func<string, Stream?>? VerifierExtraLoadHandler { get; set; }

    public void AddEngineModuleDirectory(string dir)
    {
    }

#pragma warning disable CS0067 // Needed by the interface
    public event ExtraModuleLoad? ExtraModuleLoaders;
#pragma warning restore CS0067
}

/// <summary>
/// Services that cannot be created in a browser as they are, replaced before the client starts.
/// </summary>
internal static class BrowserStandIns
{
    public static void Register(IDependencyCollection deps)
    {
        // The engine's shared HTTP client sets up its own socket handling, which a browser
        // does not have. Nothing here makes requests; a plain client stands in.
        var holder = (Robust.Shared.Network.HttpClientHolder)System.Runtime.CompilerServices.RuntimeHelpers.GetUninitializedObject(typeof(Robust.Shared.Network.HttpClientHolder));
        var client = typeof(Robust.Shared.Network.HttpClientHolder).GetField("<Client>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)
                     ?? throw new InvalidOperationException("HttpClientHolder.Client has moved.");
        client.SetValue(holder, new HttpClient());
        deps.RegisterInstance<Robust.Shared.Network.HttpClientHolder>(holder, overwrite: true);

        // The engine hands batches of work (physics, mostly) to the thread pool and waits for
        // them. With one thread the wait never ends, so the work is done on the spot. Only the
        // browser's engine (Starlight's) is built for a browser, and an older engine's jobs have
        // another shape (src/Directory.Build.props).
#if ENGINE_PARALLEL_BULK_JOBS
        deps.Register<IParallelManager, InlineParallelManager>(overwrite: true);
        deps.Register<IParallelManagerInternal, InlineParallelManager>(overwrite: true);
#endif
    }
}

#if ENGINE_PARALLEL_BULK_JOBS
/// <summary>Runs every job where it is asked for, for a runtime with a single thread.</summary>
internal sealed class InlineParallelManager : IParallelManagerInternal
{
    private static readonly ManualResetEvent Done = new(true);

#pragma warning disable CS0067 // The count never changes
    public event Action? ParallelCountChanged;
#pragma warning restore CS0067

    public int ParallelProcessCount => 1;

    public void Initialize()
    {
    }

    public void AddAndInvokeParallelCountChanged(Action changed)
    {
        ParallelCountChanged += changed;
        changed();
    }

    public WaitHandle Process(IRobustJob job)
    {
        job.Execute();
        return Done;
    }

    public void ProcessNow(IRobustJob job) => job.Execute();

    public void ProcessNow(IParallelRobustJob jobs, int amount) => Range(jobs, amount);
    public void ProcessSerialNow(IParallelRobustJob jobs, int amount) => Range(jobs, amount);

    public WaitHandle Process(IParallelRobustJob jobs, int amount)
    {
        Range(jobs, amount);
        return Done;
    }

    public void ProcessNow(IParallelBulkRobustJob jobs, int amount) => Range(jobs, amount);
    public void ProcessSerialNow(IParallelBulkRobustJob jobs, int amount) => Range(jobs, amount);

    public WaitHandle Process(IParallelBulkRobustJob jobs, int amount)
    {
        Range(jobs, amount);
        return Done;
    }

    private static void Range(IParallelRangeRobustJob jobs, int amount)
    {
        if (amount > 0)
            jobs.ExecuteRange(0, amount);
    }
}
#endif
