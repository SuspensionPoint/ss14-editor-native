using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using Content.Client.Parallax;
using Moq;
using Nett;
using Robust.Shared.ContentPack;
using Robust.Shared.Log;
using SixLabors.ImageSharp;
using SS14.EditorKernel;
using SS14.EditorViewKernel;
using DocumentKernel = SS14.EditorKernel.EditorKernel;

namespace SS14.EditorNative;

/// <summary>How long each part of showing a document, or a change to it, took.</summary>
public sealed record Shown(long DocumentMs, long ViewMs, long RecordMs, long TotalMs, int ListBytes, int Entities);

/// <summary>
/// One layer of a map's backdrop: an image file, and the numbers the game places it by (see
/// ViewKernel.Backdrop and the game's ParallaxOverlay).
/// </summary>
public sealed record BackdropImage(
    string File,
    float ScaleX, float ScaleY, bool Tiled,
    float HomeX, float HomeY, float AdjustX, float AdjustY,
    float Slowness, float ScrollX, float ScrollY);

/// <param name="Skipped">Layers that are not shown: of a kind not handled, or with no image.</param>
/// <param name="Painted">Layers the game's generator had to paint for this answer, and how long that took.</param>
/// <param name="Chosen">Of a map's backdrop: the map names it, and is not left to the game's default.</param>
/// <param name="Planet">Of a map's backdrop: the map is a planet's, which the game draws no backdrop behind.</param>
public sealed record Backdrop(string Name, IReadOnlyList<BackdropImage> Layers, int Skipped, int Painted, long PaintMs, bool Chosen = false, bool Planet = false);

/// <summary>
/// The engine as the native editor uses it: the document kernel (a headless game server that
/// owns the open maps) and the view kernel (the game client with no window, recording what it
/// would draw) in one process. Whatever the document kernel reports about a document goes
/// straight to the view kernel, as the object it is, and comes out as a draw list.
/// </summary>
public sealed class EngineSession
{
    private readonly EngineThread<DocumentKernel> _documents;
    private readonly EngineThread<ViewKernel> _view;
    private readonly Dictionary<int, DrawList> _lists = new();
    // The grids of each open document, to know when one of the removed is a grid.
    private readonly Dictionary<int, HashSet<int>> _grids = new();
    private readonly string _forkRepo;

    public long StartedAfterMs { get; private set; }

    /// <param name="forkRepo">The fork checkout; its Resources and RobustToolbox/Resources are read.</param>
    /// <param name="kernelPack">A pack with pre-parsed prototypes (Editor.Native pack), or null to parse the YAML.</param>
    /// <param name="parallel">Start both kernels at once (the design) or one after the other (to compare).</param>
    public EngineSession(string forkRepo, string? kernelPack, bool parallel)
    {
        forkRepo = Path.GetFullPath(forkRepo);
        _forkRepo = forkRepo;
        IContentRoot[] Roots() =>
        [
            new DirContentRoot(ResourcePack.EngineResources(forkRepo)),
            new DirContentRoot(Path.Combine(forkRepo, "Resources")),
        ];

        // One cache for both kernels: it is only read once it is loaded.
        var cache = kernelPack == null ? null : ResourcePack.Open(File.ReadAllBytes(kernelPack)).Cache
            ?? throw new InvalidDataException($"{kernelPack} has no prototype cache.");

        var started = Stopwatch.StartNew();
        _documents = new EngineThread<DocumentKernel>("Document kernel", progress => DocumentKernel.Boot(Roots(), progress, cache));
        if (!parallel)
            _documents.Ready.GetAwaiter().GetResult();
        _view = new EngineThread<ViewKernel>("View kernel", progress => ViewKernel.Boot(Roots(), progress, cache));

        Ready = Task.WhenAll(_documents.Ready, _view.Ready).ContinueWith(task =>
        {
            task.GetAwaiter().GetResult();
            StartedAfterMs = started.ElapsedMilliseconds;
        });
    }

    /// <summary>Both kernels started and up, for a check, with the wait counted as its boot.</summary>
    internal static EngineSession Booted(string forkRepo, string? kernelPack)
    {
        using var timed = CheckTimes.In(CheckTimes.Boot);
        var session = new EngineSession(forkRepo, kernelPack, parallel: true);
        session.Ready.GetAwaiter().GetResult();
        return session;
    }

    /// <summary>Runs a check on the document kernel's thread, with the kernel in hand.</summary>
    internal R OnDocumentKernel<R>(Func<DocumentKernel, R> work) => _documents.Run(work);

    /// <summary>Completes when both kernels are up.</summary>
    public Task Ready { get; }

    public long DocumentKernelReadyMs => _documents.ReadyAfterMs;
    public long ViewKernelReadyMs => _view.ReadyAfterMs;
    public string Progress => $"{_documents.Progress} / {_view.Progress}";

    public IReadOnlyList<(string Phase, long Milliseconds)> DocumentKernelTimings => _documents.Run(kernel => kernel.BootTimings);
    public IReadOnlyList<(string Phase, long Milliseconds)> ViewKernelTimings => _view.Run(kernel => kernel.BootTimings);

    /// <summary>Opens a map and waits until its draw list is recorded.</summary>
    public (Scene Scene, Shown Timing) Open(string yaml, string source)
    {
        var total = Stopwatch.StartNew();
        var scene = _documents.Run(kernel => kernel.Documents.Open(yaml, source));
        var documentMs = total.ElapsedMilliseconds;
        lock (_grids)
            _grids[scene.Document.Id] = scene.Grids.Select(grid => grid.Uid).ToHashSet();
        var (viewMs, recordMs, bytes, shown) = _view.Run(view =>
        {
            var sw = Stopwatch.StartNew();
            view.ApplyScene(scene);
            var applied = sw.ElapsedMilliseconds;
            sw.Restart();
            var list = view.BuildDrawList(scene.Document.Id, full: true);
            Publish(scene.Document.Id, list);
            return (applied, sw.ElapsedMilliseconds, list?.Length ?? 0, view.ShownEntities(scene.Document.Id));
        });
        return (scene, new Shown(documentMs, viewMs, recordMs, total.ElapsedMilliseconds, bytes, shown));
    }

    /// <summary>A new, empty map, shown like an opened one.</summary>
    public (Scene Scene, Shown Timing) New()
    {
        var total = Stopwatch.StartNew();
        var scene = _documents.Run(kernel => kernel.Documents.New("Map", "untitled.yml"));
        var documentMs = total.ElapsedMilliseconds;
        lock (_grids)
            _grids[scene.Document.Id] = scene.Grids.Select(grid => grid.Uid).ToHashSet();
        var (viewMs, recordMs, bytes, shown) = _view.Run(view =>
        {
            var sw = Stopwatch.StartNew();
            view.ApplyScene(scene);
            var applied = sw.ElapsedMilliseconds;
            sw.Restart();
            var list = view.BuildDrawList(scene.Document.Id, full: true);
            Publish(scene.Document.Id, list);
            return (applied, sw.ElapsedMilliseconds, list?.Length ?? 0, view.ShownEntities(scene.Document.Id));
        });
        return (scene, new Shown(documentMs, viewMs, recordMs, total.ElapsedMilliseconds, bytes, shown));
    }

    /// <summary>
    /// Applies an edit and waits until the changed draw list is recorded. With
    /// <paramref name="merge"/> it joins the undo step before it (one stroke, many edits).
    /// </summary>
    public (SceneDelta Delta, Shown Timing) Edit(int document, string label, IReadOnlyList<EditOp> ops, bool merge = false) =>
        Change(document, kernel => kernel.Documents.Edit(document, label, ops, merge));

    public (SceneDelta Delta, Shown Timing) Undo(int document) => Change(document, kernel => kernel.Documents.Undo(document));

    public (SceneDelta Delta, Shown Timing) Redo(int document) => Change(document, kernel => kernel.Documents.Redo(document));

    /// <summary>
    /// Shows or hides what lies under floor tiles, which is the game's own decision about what
    /// is visible. Every other layer switch is the interface's: it has every item's depth and
    /// kind and leaves out what it does not want. <paramref name="document"/> may be 0 when
    /// nothing is open yet.
    /// </summary>
    public Shown ShowSubFloor(int document, bool show)
    {
        var total = Stopwatch.StartNew();
        var (viewMs, recordMs, bytes, shown) = _view.Run(view =>
        {
            var sw = Stopwatch.StartNew();
            view.SetLayers(entities: true, markers: true, decals: true, [], show);
            var applied = sw.ElapsedMilliseconds;
            sw.Restart();
            var list = view.BuildDrawList(document, full: false);
            Publish(document, list);
            return (applied, sw.ElapsedMilliseconds, list?.Length ?? 0, view.ShownEntities(document));
        });
        return new Shown(0, viewMs, recordMs, total.ElapsedMilliseconds, bytes, shown);
    }

    /// <summary>
    /// How entity prototypes look, as a draw list for document -1 (ViewWorld.BuildPreviews).
    /// Returns its length in bytes.
    /// </summary>
    public int Previews(IReadOnlyList<(int Key, string Prototype)> requests, bool directions)
    {
        var bytes = _view.Run(view => view.BuildPreviews(requests, directions));
        Publish(-1, bytes);
        return bytes.Length;
    }

    /// <summary>
    /// The backdrop the game draws behind a document's map, as image files (see
    /// <see cref="NamedBackdrop"/>). A map the game gives no backdrop (a planet) has no layers.
    /// </summary>
    public Backdrop Backdrop(int document, string cacheFolder)
    {
        var (name, drawn, chosen) = _documents.Run(kernel => kernel.Documents.Backdrop(document));
        if (!drawn)
            return new Backdrop(name, [], 0, 0, 0, chosen, Planet: true);

        return NamedBackdrop(name, cacheFolder) with { Chosen = chosen };
    }

    /// <summary>The ids of every backdrop the fork has, in order.</summary>
    public IReadOnlyList<string> BackdropNames() => _view.Run(view => view.BackdropNames());

    /// <summary>
    /// One of the fork's backdrops by its id, as image files. Layers that are pictures in the
    /// fork's resources are named where they are. Layers the game paints itself (stars and
    /// dust, from a recipe) are painted here by the game's own generator, at the game's size,
    /// into <paramref name="cacheFolder"/>, and found there from then on. Painting takes
    /// seconds, on the caller's thread and others, and neither kernel waits for it. A name the
    /// fork does not have has no layers.
    /// </summary>
    public Backdrop NamedBackdrop(string name, string cacheFolder)
    {
        var described = _view.Run(view => view.Backdrop(name));
        Directory.CreateDirectory(cacheFolder);
        var files = described.Layers.Select(layer => layer.Image != null
            ? ResourceFile(layer.Image)
            : Path.Combine(cacheFolder, $"{FileName(layer.Generated!)}-{Hash(layer.Recipe!)}.png")).ToArray();

        var unpainted = described.Layers
            .Select((layer, index) => (layer.Recipe, File: files[index]))
            .Where(layer => layer.Recipe != null && !File.Exists(layer.File))
            .DistinctBy(layer => layer.File)
            .ToArray();
        var sw = Stopwatch.StartNew();
        Parallel.ForEach(unpainted, layer => Paint(layer.Recipe!, layer.File!));

        var layers = new List<BackdropImage>();
        var skipped = described.Skipped;
        for (var i = 0; i < described.Layers.Count; i++)
        {
            var layer = described.Layers[i];
            if (files[i] is not { } file || !File.Exists(file))
            {
                skipped++;
                continue;
            }

            layers.Add(new BackdropImage(
                file, layer.ScaleX, layer.ScaleY, layer.Tiled,
                layer.HomeX, layer.HomeY, layer.AdjustX, layer.AdjustY,
                layer.Slowness, layer.ScrollX, layer.ScrollY));
        }

        return new Backdrop(name, layers, skipped, unpainted.Length, unpainted.Length == 0 ? 0 : sw.ElapsedMilliseconds);
    }

    /// <summary>The game's generator and the game's size (GeneratedParallaxCache).</summary>
    private static void Paint(string recipe, string file)
    {
        using var image = ParallaxGenerator.GenerateParallax(Toml.ReadString(recipe), new Size(1920, 1080), new Mock<ISawmill>().Object, null);
        // Under another name first: a file that is there is a file that is complete. Two
        // threads may paint the same layer at once, so each has a name of its own.
        var unfinished = $"{file}.{Environment.ProcessId}.{Environment.CurrentManagedThreadId}.part";
        image.SaveAsPng(unfinished);
        File.Move(unfinished, file, overwrite: true);
    }

    private string? ResourceFile(string path)
    {
        var relative = path.TrimStart('/').Replace('/', Path.DirectorySeparatorChar);
        return new[] { Path.Combine(_forkRepo, "Resources", relative), Path.Combine(ResourcePack.EngineResources(_forkRepo), relative) }.FirstOrDefault(File.Exists);
    }

    private static string FileName(string id) =>
        id.Length == 0 ? "default" : string.Concat(id.Select(c => char.IsLetterOrDigit(c) || c is '_' or '-' ? c : '_'));

    private static string Hash(string text) => Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(text)), 0, 8).ToLowerInvariant();

    private (SceneDelta, Shown) Change(int document, Func<DocumentKernel, SceneDelta> change)
    {
        var total = Stopwatch.StartNew();
        var delta = _documents.Run(change);
        var documentMs = total.ElapsedMilliseconds;

        // The view drops what it drew of an entity that goes, but not the tiles of a grid
        // that goes. So a grid gone is answered with a complete list, as a grid come is.
        bool gridGone;
        lock (_grids)
        {
            if (!_grids.TryGetValue(document, out var grids))
                _grids[document] = grids = new HashSet<int>();
            grids.UnionWith(delta.GridsChanged.Select(grid => grid.Uid));
            gridGone = delta.Removed.Count(grids.Remove) > 0;
        }

        var (viewMs, recordMs, bytes, shown) = _view.Run(view =>
        {
            var sw = Stopwatch.StartNew();
            view.ApplyDelta(delta);
            var applied = sw.ElapsedMilliseconds;
            sw.Restart();
            var list = view.BuildDrawList(document, full: gridGone);
            Publish(document, list);
            return (applied, sw.ElapsedMilliseconds, list?.Length ?? 0, view.ShownEntities(document));
        });
        return (delta, new Shown(documentMs, viewMs, recordMs, total.ElapsedMilliseconds, bytes, shown));
    }

    /// <summary>
    /// The interface's layer switches: which of the engine's sprites are shown. They change
    /// what the engine draws, so the changed draw list is recorded before this returns.
    /// </summary>
    public Shown SetLayers(int document, bool entities, bool markers, bool decals, IReadOnlyList<int> hiddenDepths, bool subFloor)
    {
        var total = Stopwatch.StartNew();
        var (viewMs, recordMs, bytes, shown) = _view.Run(view =>
        {
            var sw = Stopwatch.StartNew();
            view.SetLayers(entities, markers, decals, hiddenDepths, subFloor);
            var applied = sw.ElapsedMilliseconds;
            sw.Restart();
            var list = view.BuildDrawList(document, full: false);
            Publish(document, list);
            return (applied, sw.ElapsedMilliseconds, list?.Length ?? 0, view.ShownEntities(document));
        });
        return new Shown(0, viewMs, recordMs, total.ElapsedMilliseconds, bytes, shown);
    }

    public SaveResult Save(int document) => _documents.Run(kernel => kernel.Documents.Save(document));

    /// <summary>The whole document as the document kernel holds it now.</summary>
    public Scene Scene(int document) => _documents.Run(kernel => kernel.Documents.GetScene(document));

    public void Close(int document)
    {
        _documents.Run(kernel =>
        {
            kernel.Documents.Close(document);
            return true;
        });
        _view.Run(view =>
        {
            view.Close(document);
            return true;
        });
        lock (_lists)
        {
            if (_lists.Remove(document, out var list))
                list.Dispose();
        }

        lock (_grids)
            _grids.Remove(document);
    }

    /// <summary>A named command of the document kernel, as JSON in and out (KernelApi.cs).</summary>
    public string Command(string op, string argsJson) => _documents.Run(kernel => new KernelApi(kernel).HandleJson(op, argsJson));

    // ---- draw lists, kept where native code can read them in place ----

    private sealed class DrawList(byte[] bytes) : IDisposable
    {
        public readonly byte[] Bytes = bytes;
        private GCHandle _pin = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        public IntPtr Address => _pin.AddrOfPinnedObject();
        public void Dispose() => _pin.Free();
    }

    private void Publish(int document, byte[]? bytes)
    {
        if (bytes == null)
            return;

        lock (_lists)
        {
            if (_lists.Remove(document, out var previous))
                previous.Dispose();
            _lists[document] = new DrawList(bytes);
        }
    }

    /// <summary>
    /// The latest draw-list message recorded for a document, as an address and a length. The
    /// memory stays where it is until the next message for that document is recorded.
    /// </summary>
    public (IntPtr Address, int Length) LatestDrawList(int document)
    {
        lock (_lists)
            return _lists.TryGetValue(document, out var list) ? (list.Address, list.Bytes.Length) : (IntPtr.Zero, 0);
    }
}
