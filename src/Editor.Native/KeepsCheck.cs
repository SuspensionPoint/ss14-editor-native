using System.Diagnostics;
using System.Text.RegularExpressions;
using DocumentKernel = SS14.EditorKernel.EditorKernel;

namespace SS14.EditorNative;

/// <summary>
/// Checks that a map keeps its components through the editor. Each map is opened and saved with no
/// edit, and every kind of component is counted in the file and in the save, on the entities in
/// both. If the save has fewer of a kind, that is a FAIL. It means the engine side doesn't know
/// the component and dropped it, which is exactly what the forks' safety rule exists to prevent
/// (docs/architecture.md). Kinds named with --expect must be in at least one of the files. Each
/// step reports how long it took.
///
/// Entities missing from the save are listed separately, with their prototypes, because the
/// fork's migrations delete some ids as a map loads (a migration to null), just as the game does.
/// </summary>
public static partial class KeepsCheck
{
    [GeneratedRegex(@"^- proto: ?(.*?)\s*$")]
    private static partial Regex ProtoLine();

    [GeneratedRegex(@"^  - uid: (\d+)\s*$")]
    private static partial Regex UidLine();

    [GeneratedRegex(@"^\s*- type: ([A-Za-z0-9_]+)\s*$")]
    private static partial Regex ComponentLine();

    /// <returns>0 if nothing failed.</returns>
    public static int Run(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, IReadOnlyList<string> expected, string outFolder, Action<string> say)
    {
        Directory.CreateDirectory(outFolder);
        forkRepo = Path.GetFullPath(forkRepo);
        var clock = Stopwatch.StartNew();
        var session = new EngineSession(forkRepo, kernelPack, parallel: true);
        session.Ready.GetAwaiter().GetResult();
        say($"engine started in {clock.Elapsed.TotalSeconds:0.0} s");
        return session.OnDocumentKernel(kernel =>
        {
            var failures = 0;
            var inFiles = new Dictionary<string, int>(StringComparer.Ordinal);
            var inSaves = new Dictionary<string, int>(StringComparer.Ordinal);
            foreach (var map in maps)
                failures += Check(kernel, forkRepo, map, expected, outFolder, inFiles, inSaves, say);

            foreach (var kind in expected)
            {
                var count = inFiles.GetValueOrDefault(kind);
                var kept = inSaves.GetValueOrDefault(kind);
                var ok = count > 0 && kept >= count;
                say($"{(ok ? "ok  " : "FAIL")}  {kind}: {count} in the files, {kept} in the saves");
                if (!ok)
                    failures++;
            }
            say(failures == 0 ? "\nok    every map kept every component" : $"\nFAIL  {failures} failures");
            return failures == 0 ? 0 : 1;
        });
    }

    private static int Check(DocumentKernel kernel, string forkRepo, string map, IReadOnlyList<string> expected, string outFolder,
        Dictionary<string, int> inFiles, Dictionary<string, int> inSaves, Action<string> say)
    {
        var failures = 0;
        var text = File.ReadAllText(Path.Combine(forkRepo, "Resources", map));
        var clock = Stopwatch.StartNew();
        var scene = kernel.Documents.Open(text, map);
        say($"\n{map}: opened in {clock.Elapsed.TotalSeconds:0.0} s, {scene.Entities.Count} entities");

        clock.Restart();
        var saved = kernel.Documents.Save(scene.Document.Id);
        kernel.Documents.Close(scene.Document.Id);
        if (!saved.Ok || saved.Yaml == null)
        {
            say($"  FAIL  the save failed: {saved.Failure}");
            return 1;
        }

        say($"  saved in {clock.Elapsed.TotalSeconds:0.0} s");
        var output = Path.Combine(outFolder, Path.GetFileName(map));
        File.WriteAllText(output, saved.Yaml);

        var fileEntities = Entities(text);
        var savedEntities = Entities(saved.Yaml);
        // An entity is the same in both when its uid and prototype are. The fork's migrations
        // delete some at load and give others another prototype, and an entity made anew at load
        // (a chunk entity of a fork's decals) is saved with a uid freed by them.
        bool Kept(int uid) => savedEntities.TryGetValue(uid, out var now) && now.Prototype == fileEntities[uid].Prototype;
        var gone = fileEntities.Keys.Where(uid => !Kept(uid)).ToList();
        if (gone.Count > 0)
        {
            var prototypes = gone.GroupBy(uid => fileEntities[uid].Prototype).OrderBy(group => group.Key, StringComparer.Ordinal).Select(group => $"{group.Key} {group.Count()}");
            say($"  found {gone.Count} entities gone or changed at load, as the fork's migrations do, or made anew: {string.Join(", ", prototypes)}");
        }

        var before = Count(fileEntities.Where(pair => Kept(pair.Key)).Select(pair => pair.Value));
        var after = Count(savedEntities.Values);
        var factory = Robust.Shared.IoC.IoCManager.Resolve<Robust.Shared.GameObjects.IComponentFactory>();
        foreach (var (kind, count) in before.OrderBy(pair => pair.Key, StringComparer.Ordinal))
        {
            var kept = after.GetValueOrDefault(kind);
            if (kept >= count)
                continue;
            // A kind the engine marks as never saved goes from any save, the game's too.
            if (factory.TryGetRegistration(kind, out var registration) && registration.Unsaved)
            {
                say($"  found {kind}: {count} in the file, {kept} in the save, which the engine never saves");
                continue;
            }

            // A fork that keeps its decals on chunk entities moves a grid's own decals onto them
            // as the map loads, as the game's DecalSystem does. Every decal must still be there.
            if (kind == "DecalGrid" && OnChunks)
            {
                var (was, now) = (DecalCount(text), DecalCount(saved.Yaml));
                say($"  {(was == now ? "found" : "FAIL ")} {kind}: {count} in the file, {kept} in the save. The game moves a grid's decals onto chunk entities as the map loads: {was} decals in the file, {now} on chunk entities in the save");
                if (was != now)
                    failures++;
                continue;
            }

            var known = factory.GetComponentAvailability(kind) == Robust.Shared.GameObjects.ComponentAvailability.Available ? "" : ", a kind this engine side does not know";
            say($"  FAIL  {kind}: {count} in the file, {kept} in the save{known}");
            failures++;
        }

        foreach (var (kind, count) in after.Where(pair => pair.Value > before.GetValueOrDefault(pair.Key)).OrderBy(pair => pair.Key, StringComparer.Ordinal))
            say($"  found {kind}: {before.GetValueOrDefault(kind)} in the file, {count} in the save");

        foreach (var kind in expected.Where(before.ContainsKey))
        {
            say($"  {kind}: {before[kind]} in the file, {after.GetValueOrDefault(kind)} in the save");
            inFiles[kind] = inFiles.GetValueOrDefault(kind) + before[kind];
            inSaves[kind] = inSaves.GetValueOrDefault(kind) + after.GetValueOrDefault(kind);
        }

        say($"  {before.Count} kinds of component, {before.Values.Sum()} in all, compared with {output}");
        return failures;
    }

    /// <summary>Whether the fork keeps its decals on chunk entities (FORK_DECAL_CHUNK_ENTITIES).</summary>
#if FORK_DECAL_CHUNK_ENTITIES
    private static readonly bool OnChunks = true;
#else
    private static readonly bool OnChunks = false;
#endif

    [GeneratedRegex(@"^            \d+: ")]
    private static partial Regex DecalLine();

    /// <summary>How many decals a map file lists, on its grids and on chunk entities.</summary>
    private static int DecalCount(string yaml)
    {
        var (count, inDecals) = (0, false);
        foreach (var line in yaml.Split('\n'))
        {
            if (ComponentLine().Match(line) is { Success: true } component)
                inDecals = component.Groups[1].Value is "DecalGrid" or "DecalChunk";
            else if (!line.StartsWith("      ", StringComparison.Ordinal))
                inDecals = false;
            else if (inDecals && DecalLine().IsMatch(line))
                count++;
        }

        return count;
    }

    private sealed record Entity(string Prototype, List<string> Components);

    /// <summary>The entities of a map file by their uid, with their prototype and the kinds of their components.</summary>
    private static Dictionary<int, Entity> Entities(string yaml)
    {
        var entities = new Dictionary<int, Entity>();
        var prototype = "";
        Entity? current = null;
        foreach (var line in yaml.Split('\n'))
        {
            if (ProtoLine().Match(line) is { Success: true } proto)
            {
                prototype = proto.Groups[1].Value;
                current = null;
            }
            else if (UidLine().Match(line) is { Success: true } uid)
            {
                current = new Entity(prototype, []);
                entities[int.Parse(uid.Groups[1].Value)] = current;
            }
            else if (current != null && ComponentLine().Match(line) is { Success: true } component)
            {
                current.Components.Add(component.Groups[1].Value);
            }
        }

        return entities;
    }

    private static Dictionary<string, int> Count(IEnumerable<Entity> entities)
    {
        var counts = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (var kind in entities.SelectMany(entity => entity.Components))
            counts[kind] = counts.GetValueOrDefault(kind) + 1;
        return counts;
    }
}
