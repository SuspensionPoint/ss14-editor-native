using System.Diagnostics;
using System.Text.Json;
using SS14.EditorKernel;

namespace SS14.EditorNative;

/// <summary>
/// How fast the engine starts, opens a map, shows it, takes an edit and saves, with both kernels
/// in one process. Nothing on the Rust side can make these faster. They are the floor.
/// </summary>
public static class Bench
{
    public sealed record MapResult(
        string Map, int Entities, int Grids,
        long OpenDocumentMs, long OpenViewMs, long OpenRecordMs, long OpenTotalMs, int ListKb,
        double[] TileEditMs, double[] TileEditDocumentMs, double[] TileEditViewMs, double[] PlaceEntityMs, double[] UndoMs,
        long SaveMs, long ManagedMbWithMapOpen, long WorkingSetMbWithMapOpen);

    public sealed record Result(
        string Configuration, bool Parallel, bool Cache,
        long RuntimeToMainMs, long DocumentKernelReadyMs, long ViewKernelReadyMs, long BothReadyMs,
        Dictionary<string, long> DocumentKernelPhases, Dictionary<string, long> ViewKernelPhases,
        long ManagedMbAfterStart, long WorkingSetMbAfterStart, List<MapResult> Maps, long PeakWorkingSetMb);

    /// <param name="saveLists">A folder to write each map's first, complete draw list to (to compare with the game client's), or null.</param>
    public static Result Run(string forkRepo, string? kernelPack, bool parallel, IReadOnlyList<string> maps, Action<string> say, string? saveLists = null)
    {
        using var process = Process.GetCurrentProcess();
        var runtimeToMain = (long)(DateTime.Now - process.StartTime).TotalMilliseconds;

        var session = new EngineSession(forkRepo, kernelPack, parallel);
        session.Ready.GetAwaiter().GetResult();
        say($"document kernel ready after {session.DocumentKernelReadyMs} ms, view kernel after {session.ViewKernelReadyMs} ms (from its own start), both after {session.StartedAfterMs} ms");

        var documentPhases = session.DocumentKernelTimings.ToDictionary(t => t.Phase, t => t.Milliseconds);
        var viewPhases = session.ViewKernelTimings.ToDictionary(t => t.Phase, t => t.Milliseconds);
        say("   document kernel: " + string.Join(", ", documentPhases.Select(phase => $"{phase.Key} {phase.Value}")));
        say("   view kernel: " + string.Join(", ", viewPhases.Select(phase => $"{phase.Key} {phase.Value}")));
        var managedAfterStart = GC.GetTotalMemory(true) / 1048576;
        process.Refresh();
        var workingAfterStart = process.WorkingSet64 / 1048576;
        say($"after start: {managedAfterStart} MB of live objects, {workingAfterStart} MB working set");

        var results = new List<MapResult>();
        foreach (var map in maps)
        {
            var yaml = File.ReadAllText(Path.Combine(forkRepo, "Resources", map));
            var (scene, open) = session.Open(yaml, map);
            var doc = scene.Document.Id;
            if (saveLists != null)
            {
                var (address, length) = session.LatestDrawList(doc);
                var copy = new byte[length];
                System.Runtime.InteropServices.Marshal.Copy(address, copy, 0, length);
                File.WriteAllBytes(Path.Combine(saveLists, Path.GetFileNameWithoutExtension(map) + ".native.drw"), copy);
            }
            say($"{map}: {scene.Entities.Count} entities; document {open.DocumentMs} ms, view {open.ViewMs} ms, draw list {open.RecordMs} ms ({open.ListBytes / 1024} KB), total {open.TotalMs} ms");

            // The grid with the most tiles, and a run of its tiles to paint one at a time.
            var grid = scene.Grids.OrderByDescending(g => g.Tiles.Count).First();
            var tiles = Enumerable.Range(0, Math.Min(24, grid.Tiles.Count / 5)).Select(i => (X: grid.Tiles[(grid.Tiles.Count / 10 / 5 + i) * 5], Y: grid.Tiles[(grid.Tiles.Count / 10 / 5 + i) * 5 + 1])).ToArray();

            var tileEdits = new List<Shown>();
            foreach (var (x, y) in tiles)
                tileEdits.Add(session.Edit(doc, "Paint tile", [new EditOp("tile", Grid: grid.Uid, X: x, Y: y, Tile: tileEdits.Count % 2 == 0 ? "FloorWood" : "FloorSteel")]).Timing);

            var places = new List<Shown>();
            foreach (var (x, y) in tiles.Take(12))
                places.Add(session.Edit(doc, "Place entity", [new EditOp("spawn", Parent: grid.Uid, X: x + 0.5f, Y: y + 0.5f, Prototype: "ChairWood")]).Timing);

            var undos = new List<Shown>();
            for (var i = 0; i < tileEdits.Count + places.Count; i++)
                undos.Add(session.Undo(doc).Timing);

            // The first few of anything include one-off work (code compiled on first use).
            double[] Settled(IEnumerable<Shown> all, Func<Shown, long> pick) => all.Skip(4).Select(s => (double)pick(s)).OrderBy(v => v).ToArray();
            var tileMs = Settled(tileEdits, s => s.TotalMs);
            say($"   one tile painted -> changed draw list: median {Median(tileMs)} ms (document {Median(Settled(tileEdits, s => s.DocumentMs))}, view {Median(Settled(tileEdits, s => s.ViewMs))}, record {Median(Settled(tileEdits, s => s.RecordMs))}); first {tileEdits[0].TotalMs} ms");
            var placeMs = Settled(places, s => s.TotalMs);
            say($"   one entity placed -> changed draw list: median {Median(placeMs)} ms; first {places[0].TotalMs} ms");
            var undoMs = Settled(undos, s => s.TotalMs);
            say($"   one undo -> changed draw list: median {Median(undoMs)} ms");

            var saving = Stopwatch.StartNew();
            var saved = session.Save(doc);
            var saveMs = saving.ElapsedMilliseconds;
            say($"   save: {saveMs} ms ({(saved.Ok ? $"{saved.Yaml!.Length / 1024} KB" : "FAILED: " + saved.Failure)})");

            var managed = GC.GetTotalMemory(true) / 1048576;
            process.Refresh();
            var working = process.WorkingSet64 / 1048576;
            say($"   with the map open: {managed} MB of live objects, {working} MB working set");

            results.Add(new MapResult(map, scene.Entities.Count, scene.Grids.Count,
                open.DocumentMs, open.ViewMs, open.RecordMs, open.TotalMs, open.ListBytes / 1024,
                tileMs, Settled(tileEdits, s => s.DocumentMs), Settled(tileEdits, s => s.ViewMs), placeMs, undoMs,
                saveMs, managed, working));
            session.Close(doc);
        }

        process.Refresh();
        var configuration =
#if DEBUG
            "Debug";
#else
            "Release";
#endif
        return new Result(configuration, parallel, kernelPack != null, runtimeToMain,
            session.DocumentKernelReadyMs, session.ViewKernelReadyMs, session.StartedAfterMs,
            documentPhases, viewPhases, managedAfterStart, workingAfterStart, results, process.PeakWorkingSet64 / 1048576);
    }

    public static double Median(double[] sorted) => sorted.Length == 0 ? double.NaN : sorted[sorted.Length / 2];

    public static string ToJson(Result result) => JsonSerializer.Serialize(result, new JsonSerializerOptions(JsonSerializerDefaults.Web) { WriteIndented = true });
}
