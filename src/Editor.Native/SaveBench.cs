using System.Diagnostics;
using Robust.Shared.EntitySerialization;
using Robust.Shared.EntitySerialization.Systems;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;
using Robust.Shared.Serialization;
using Robust.Shared.Serialization.Manager;
using Robust.Shared.Serialization.Markdown.Mapping;
using SS14.EditorKernel;
using YamlDotNet.Core;
using YamlDotNet.RepresentationModel;
using DocumentKernel = SS14.EditorKernel.EditorKernel;

namespace SS14.EditorNative;

/// <summary>
/// Where the time of one save goes. It saves each map whole a few times, as the editor does, then
/// runs the engine's steps one at a time with a clock on each, and finally asks the engine's
/// serializer about each kind of component. The steps here are only measured and write no file.
/// Their text is compared with the real save's to show they do the same work.
/// </summary>
public static class SaveBench
{
    public static int Run(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, int rounds, Action<string> say)
    {
        var session = new EngineSession(forkRepo, kernelPack, parallel: true);
        session.Ready.GetAwaiter().GetResult();
        return session.OnDocumentKernel(kernel =>
        {
            var same = true;
            foreach (var map in maps)
                same &= Measure(kernel, forkRepo, map, rounds, say);
            return same ? 0 : 1;
        });
    }

    private readonly record struct Collected(TimeSpan Pause, int Gen0, int Gen1, int Gen2, long Allocated)
    {
        public static Collected Now() => new(GC.GetTotalPauseDuration(), GC.CollectionCount(0), GC.CollectionCount(1), GC.CollectionCount(2), GC.GetTotalAllocatedBytes());

        public string Since(Collected before) =>
            $"collector paused {(Pause - before.Pause).TotalMilliseconds:0} ms in {Gen0 - before.Gen0}/{Gen1 - before.Gen1}/{Gen2 - before.Gen2} collections, {(Allocated - before.Allocated) / 1048576} MB allocated";
    }

    private static bool Measure(DocumentKernel kernel, string forkRepo, string map, int rounds, Action<string> say)
    {
        var entMan = (EntityManager) IoCManager.Resolve<IEntityManager>();
        var loader = entMan.System<MapLoaderSystem>();

        var scene = kernel.Documents.Open(File.ReadAllText(Path.Combine(forkRepo, "Resources", map)), map);
        var doc = scene.Document.Id;
        var root = new EntityUid(scene.Document.MapUid);
        say($"\n{Path.GetFileName(map)}: {scene.Entities.Count} entities");

        string? text = null;
        for (var round = 1; round <= rounds; round++)
        {
            var before = Collected.Now();
            var sw = Stopwatch.StartNew();
            var saved = kernel.Documents.Save(doc);
            var ms = sw.ElapsedMilliseconds;
            if (!saved.Ok || saved.Yaml == null)
                throw new InvalidOperationException($"not saved: {saved.Failure}");

            text = saved.Yaml;
            say($"  whole save {round}: {ms,5} ms  ({Collected.Now().Since(before)})");
        }

        say($"  the saved text: {text!.Length / 1024} KB, {text.AsSpan().Count('\n')} lines, {text.AsSpan().Count("    - type: ")} components");

        var same = true;
        for (var round = 1; round <= rounds; round++)
        {
            var before = Collected.Now();
            var sw = Stopwatch.StartNew();
            double Lap()
            {
                var took = sw.Elapsed.TotalMilliseconds;
                sw.Restart();
                return took;
            }

            entMan.CullRemovedComponents();
            var cull = Lap();

            var (data, _) = loader.SerializeEntitiesRecursive([root], SerializationOptions.Default with { Category = FileCategory.Map });
            var nodes = Lap();

            var document = new YamlDocument(data.ToYaml());
            var yamlNodes = Lap();

            using var writer = new StringWriter { NewLine = "\n" };
            new YamlStream { document }.Save(new YamlMappingFix(new Emitter(writer)), false);
            var emitted = Lap();

            var whole = writer.ToString();
            var copied = Lap();

            var unix = whole.Replace("\r\n", "\n");
            var endings = Lap();

            say($"  by step {round}: clean-up {cull:0}, entities to data nodes {nodes:0}, data nodes to YAML nodes {yamlNodes:0}, YAML nodes to text {emitted:0}, text out of the writer {copied:0}, line endings {endings:0} ms  ({Collected.Now().Since(before)})");
            same &= WithoutTime(unix) == WithoutTime(text);
        }

        say(same ? "  the steps give the saved text" : "  FAIL  the steps give another text than the save");
        Inside(entMan, root, say);
        kernel.DrainLog();
        kernel.Documents.Close(doc);
        return same;
    }

    /// <summary>The engine writes the time of the save into the file.</summary>
    private static string WithoutTime(string yaml) =>
        string.Join('\n', yaml.Split('\n').Where(line => !line.StartsWith("  time: ", StringComparison.Ordinal)));

    /// <summary>
    /// The first step again, in the parts the engine's serializer has, and then one component
    /// at a time. The second is close to what the serializer does for each, not the same calls.
    /// </summary>
    private static void Inside(EntityManager entMan, EntityUid root, Action<string> say)
    {
        var serialization = IoCManager.Resolve<ISerializationManager>();
        var factory = IoCManager.Resolve<IComponentFactory>();

        var sw = Stopwatch.StartNew();
        var serializer = new EntitySerializer(IoCManager.Resolve<IEntitySystemManager>().DependencyCollection,SerializationOptions.Default with { Category = FileCategory.Map });
        var made = sw.Elapsed.TotalMilliseconds;
        sw.Restart();
        // One root, by the overload every engine has (older ones have no set of roots).
        serializer.SerializeEntityRecursive(root);
        var entities = sw.Elapsed.TotalMilliseconds;
        sw.Restart();
        serializer.Write();
        var sections = sw.Elapsed.TotalMilliseconds;
        say($"  inside entities to data nodes: serializer made {made:0}, every entity {entities:0}, sections put together {sections:0} ms");

        // What the engine writes once per prototype in each save, to leave out what an entity has from it.
        var prototypes = serializer.EntityData.Values
            .Select(entity => entMan.GetComponent<MetaDataComponent>(entity.Uid).EntityPrototype)
            .Where(prototype => prototype != null)
            .Distinct()
            .ToArray();
        sw.Restart();
        var prototypeComponents = 0;
        foreach (var prototype in prototypes)
        {
            foreach (var (_, component) in prototype!.Components)
            {
                serialization.WriteValueAs<MappingDataNode>(component.Component.GetType(), component.Component, alwaysWrite: true, context: serializer);
                prototypeComponents++;
            }
        }

        say($"  {prototypes.Length} prototypes written as data nodes: {sw.Elapsed.TotalMilliseconds:0} ms for {prototypeComponents} components");

        var kinds = new Dictionary<string, (long Write, long Except, int Count)>();
        foreach (var (uid, _) in serializer.EntityData.Values)
        {
            var prototype = entMan.GetComponent<MetaDataComponent>(uid).EntityPrototype;
            var cache = prototype == null ? null : serializer.PrototypeCache[prototype.ID];
            foreach (var component in entMan.GetComponentsInternal(uid))
            {
#if ENGINE_PENDING_REMOVAL
                // Only an engine that has this skips these when it saves.
                if (entMan.IsComponentPendingRemoval(component))
                    continue;
#endif

                var type = component.GetType();
                var registration = factory.GetRegistration(type);
                if (registration.Unsaved)
                    continue;

                MappingDataNode? fromPrototype = null;
                var known = cache != null && cache.TryGetValue(registration.Name, out fromPrototype);
                var started = Stopwatch.GetTimestamp();
                var written = serialization.WriteValueAs<MappingDataNode>(type, component, alwaysWrite: known, context: serializer);
                var wrote = Stopwatch.GetTimestamp();
                if (known)
                    written.Except(fromPrototype!);
                var compared = Stopwatch.GetTimestamp();

                var kind = kinds.GetValueOrDefault(registration.Name);
                kinds[registration.Name] = (kind.Write + wrote - started, kind.Except + compared - wrote, kind.Count + 1);
            }
        }

        double Ms(long ticks) => ticks * 1000.0 / Stopwatch.Frequency;
        say($"  {kinds.Values.Sum(kind => kind.Count)} components of {kinds.Count} kinds: written {Ms(kinds.Values.Sum(kind => kind.Write)):0} ms, compared with the prototype's {Ms(kinds.Values.Sum(kind => kind.Except)):0} ms");
        foreach (var (name, kind) in kinds.OrderByDescending(kind => kind.Value.Write + kind.Value.Except).Take(15))
            say($"    {name,-28}{kind.Count,7} times  written {Ms(kind.Write),6:0} ms  compared {Ms(kind.Except),6:0} ms");
    }
}
