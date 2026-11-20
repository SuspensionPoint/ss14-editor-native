using System.Diagnostics;
using System.Numerics;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Content.Shared.DeviceLinking;
#if FORK_SEALED_DEVICE_LINK_SYSTEM
using Content.Shared.DeviceLinking.Components;
#endif
using Content.Shared.DeviceNetwork.Components;
#if FORK_LINKED_DEVICE_NETWORK
// Where a device list and a device's lists are after the device network refactor (src/Directory.Build.props).
using Content.Shared.DeviceConfigurator.Components;
using ListedDeviceComponent = Content.Shared.DeviceConfigurator.Components.LinkedDeviceNetworkComponent;
#else
using ListedDeviceComponent = Content.Shared.DeviceNetwork.Components.DeviceNetworkComponent;
#endif
using Robust.Shared.Containers;
using Robust.Shared.ContentPack;
using Robust.Shared.EntitySerialization.Components;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;
using Robust.Shared.Map;
using Robust.Shared.Map.Components;
using Robust.Shared.Maths;
using SS14.EditorKernel;
using DocumentKernel = SS14.EditorKernel.EditorKernel;

namespace SS14.EditorNative;

/// <summary>
/// Checks copy and paste on real maps: the kernel's `copy` command and the `paste` op of an edit.
///
/// The web editor copied an entity as YAML with the uids in it, so a pasted alarm still listed the
/// devices of the alarm it was copied from. Here the engine writes the text, numbering the copied
/// entities among themselves. The engine also reads it back, making new entities and resolving
/// those numbers to them. Anything a copy names outside itself is left out of the text.
///
/// The copied set is alarms with their devices, linked buttons, full containers and whatever
/// stands around them, plus, on purpose, one alarm without all its devices and one button without
/// what it is linked to. It is pasted beside the originals, twice, turned, where the originals
/// stood after a cut, and into another map. Each time, the check looks at the live entities and
/// the saved file, and at undo and redo.
///
/// Copies and pastes go through the kernel. The engine is only asked directly to inspect the
/// results, on the document kernel's thread.
///
/// Lines starting with "found" note where the engine behaves differently than one might expect.
/// They don't fail the check. Lines starting with "FAIL" do.
/// </summary>
public static class CopyCheck
{
    private const double Quarter = Math.PI / 2;

    /// <returns>0 if nothing failed.</returns>
    public static int Run(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        Directory.CreateDirectory(outFolder);
        forkRepo = Path.GetFullPath(forkRepo);
        EngineThread<DocumentKernel> engine;
        using (CheckTimes.In(CheckTimes.Boot))
        {
            var cache = kernelPack == null ? null : ResourcePack.Open(File.ReadAllBytes(kernelPack)).Cache
                ?? throw new InvalidDataException($"{kernelPack} has no prototype cache.");

            // The document kernel alone: nothing here is drawn.
            engine = new EngineThread<DocumentKernel>("Document kernel", progress => DocumentKernel.Boot(
                [
                    new DirContentRoot(ResourcePack.EngineResources(forkRepo)),
                    new DirContentRoot(Path.Combine(forkRepo, "Resources")),
                ],
                progress,
                cache));
            engine.Ready.GetAwaiter().GetResult();
        }

        var code = engine.Run(kernel => new Checks(kernel, forkRepo, outFolder, say).Run(maps));
        CheckTimes.Report("copies", say);
        CheckTimes.Summary(say);
        return code;
    }

    /// <summary>The check on an engine that is up already, on its document kernel.</summary>
    public static int Run(EngineSession session, string forkRepo, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        Directory.CreateDirectory(outFolder);
        forkRepo = Path.GetFullPath(forkRepo);
        return session.OnDocumentKernel(kernel => new Checks(kernel, forkRepo, outFolder, say).Run(maps));
    }

    // ---- what was copied, as the check remembers it ----

    /// <summary>
    /// One entity of a copy, by the number it has in the text: where it was, and which entities
    /// of the copy it named. A paste is checked against this, on the map it came from or another.
    /// </summary>
    /// <param name="Parent">The number of what holds it, or 0 if it stood on the grid.</param>
    /// <param name="Inside">For each field that lists entities, the numbers of those in the copy.</param>
    /// <param name="Outside">For each such field, how many it listed that are not in the copy.</param>
    /// <param name="Holds">For each of its containers, the numbers of what is in it.</param>
    private sealed record Copied(
        int Yaml, string Prototype, int Parent, Vector2 Position, Angle Rotation, bool Anchored,
        Dictionary<string, int[]> Inside, Dictionary<string, int> Outside, Dictionary<string, int[]> Holds);

    /// <param name="Order">The numbers of the text's entities, in the order the text has them.</param>
    /// <param name="Alarm">The number of the alarm copied without all its devices, or 0.</param>
    /// <param name="Source">The number of the linked thing copied without what it is linked to, or 0.</param>
    private sealed record Clip(CopyResult Copy, Dictionary<int, Copied> Entities, int[] Order, int Alarm, int Source)
    {
        public string Text => Copy.Text;
        public int Roots => Copy.Roots;
    }

    /// <param name="Skipped">What the copy should hold but cannot, for the map has none of it.</param>
    private sealed record Chosen(HashSet<EntityUid> Roots, EntityUid Alarm, List<EntityUid> LeftOut, EntityUid Source, string Summary, List<string> Skipped);

    /// <param name="ByYaml">The copy of each entity of the text, by its number there.</param>
    /// <param name="X">Where the paste was asked for, with <paramref name="Y"/> and <paramref name="Quarters"/>.</param>
    private sealed record Pasted(Dictionary<int, EntityUid> ByYaml, HashSet<EntityUid> Entities, SceneDelta Delta, long Ms, float X, float Y, int Quarters);

    private sealed class Checks(DocumentKernel kernel, string forkRepo, string outFolder, Action<string> say)
    {
        private readonly IEntityManager _entMan = IoCManager.Resolve<IEntityManager>();
        private SharedMapSystem Maps => _entMan.System<SharedMapSystem>();
        private DocumentManager Documents => kernel.Documents;
        private int _failures;
        private (long Ms, string What) _largestCopy = (-1, "");
        private (long Ms, string What) _largestPaste = (-1, "");

        // The document in hand.
        private string _name = "";
        private int _document;
        private EntityUid _grid;
        private string _floor = "";
        // Every entity the check has seen in it. What a paste reports beyond these is its copies.
        private readonly HashSet<int> _known = new();
        // How many steps there are to undo.
        private int _steps;

        private void Ok(string text) => say($"  ok    {text}");
        private void Info(string text) => say($"  info  {text}");
        private void Found(string text) => say($"  found {text}");
        private void Skip(string text) => CheckSkips.Skip(say, text);

        private void Fail(string text)
        {
            say($"  FAIL  {text}");
            _failures++;
        }

        private void Expect(bool holds, string ok, string fail)
        {
            if (holds)
                Ok(ok);
            else
                Fail(fail);
        }

        public int Run(IReadOnlyList<string> maps)
        {
            var skipsBefore = CheckSkips.Count;
            say("a new map");
            try
            {
                Partners();
            }
            catch (Exception e)
            {
                Fail($"{e.GetType().Name}: {e.Message}\n{e.StackTrace}");
            }

            Clip? first = null;
            foreach (var map in maps)
            {
                say(map);
                try
                {
                    var clip = Map(map);
                    first ??= clip;
                }
                catch (Exception e)
                {
                    Fail($"{e.GetType().Name}: {e.Message}\n{e.StackTrace}");
                }
            }

            if (maps.Count > 1 && first != null)
            {
                say($"{maps[0]} pasted into {maps[1]}");
                try
                {
                    Cross(first, maps[1]);
                }
                catch (Exception e)
                {
                    Fail($"{e.GetType().Name}: {e.Message}\n{e.StackTrace}");
                }
            }
            else if (maps.Count < 2)
            {
                Skip("a copy pasted into another map: give two maps for it");
            }

            say("");
            if (_largestCopy.Ms >= 0)
                say($"the largest copy: {_largestCopy.Ms} ms, {_largestCopy.What}");
            if (_largestPaste.Ms >= 0)
                say($"the largest paste: {_largestPaste.Ms} ms, {_largestPaste.What}");
            say(_failures == 0 ? "Nothing failed. The lines that begin \"found\" say what the engine does." : $"{_failures} checks failed.");
            CheckSkips.Report(say, skipsBefore);
            return _failures == 0 ? 0 : 1;
        }

        private Scene Opened(string stage, string yaml, string source)
        {
            using var timed = CheckTimes.In(stage);
            return Documents.Open(yaml, source);
        }

        private SaveResult Saving(int document)
        {
            using var timed = CheckTimes.In(CheckTimes.Save);
            return Documents.Save(document);
        }

        private SceneGrid Begin(Scene scene, string name)
        {
            var sceneGrid = scene.Grids.MaxBy(grid => grid.Tiles.Count) ?? throw new InvalidOperationException("the map has no grid");
            _name = name;
            _document = scene.Document.Id;
            _grid = new EntityUid(sceneGrid.Uid);
            _floor = sceneGrid.TileIds[sceneGrid.Tiles[2]];
            _steps = 0;
            _known.Clear();
            _known.Add(scene.Document.MapUid);
            _known.UnionWith(scene.Grids.Select(grid => grid.Uid));
            _known.UnionWith(scene.Entities.Select(entity => entity.Uid));
            return sceneGrid;
        }

        // ---- a partner left out ----

        /// <summary>
        /// On a new map: two docks that are docked with each other, and a console that names
        /// one of them as its analyzer. One dock and the console are copied without the other
        /// dock. Each names it in a field of its own, which is no list. The text must name
        /// nothing outside it, the map must be as it was, and a paste must put no "invalid"
        /// into the file.
        /// </summary>
        private void Partners()
        {
            var scene = Documents.New("Map", "partners.yml");
            _document = scene.Document.Id;
            try
            {
                var grid = scene.Grids[0].Uid;
                Documents.Edit(_document, "Tiles", Enumerable.Range(0, 6).Select(x => new EditOp("tile", Grid: grid, X: x, Y: 0, Tile: "Plating")).ToArray());
                var made = Documents.Edit(_document, "Place",
                [
                    new EditOp("spawn", Prototype: "AirlockShuttle", Parent: grid, X: 0.5f, Y: 0.5f),
                    new EditOp("spawn", Prototype: "AirlockShuttle", Parent: grid, X: 1.5f, Y: 0.5f),
                    new EditOp("spawn", Prototype: "ComputerAnalysisConsole", Parent: grid, X: 2.5f, Y: 0.5f),
                ]).Created;
                var (dock, other, console) = (made[0], made[1], made[2]);

                // An entity's component data with this component in place of its own of the kind.
                string With(int uid, JsonObject component)
                {
                    var components = JsonNode.Parse(Documents.Describe(_document, [uid])[0].Components)!.AsArray();
                    var type = component["type"]!.GetValue<string>();
                    foreach (var old in components.OfType<JsonObject>().Where(old => old["type"]?.GetValue<string>() == type).ToArray())
                        components.Remove(old);
                    components.Add(component);
                    return components.ToJsonString();
                }

                JsonObject Docked(int with) => new() { ["type"] = "Docking", ["dockedWith"] = with, ["dockJointId"] = "docking1" };
                Documents.Edit(_document, "Partners",
                [
                    new EditOp("components", Uid: dock, Components: With(dock, Docked(other))),
                    new EditOp("components", Uid: other, Components: With(other, Docked(dock))),
                    new EditOp("components", Uid: console, Components: With(console, new JsonObject { ["type"] = "AnalysisConsole", ["analyzerEntity"] = other })),
                ]);
                var before = Save("partners.before.yml");
                Expect(before.Count(line => line.Contains("dockedWith:")) == 2 && before.Count(line => line.Contains("analyzerEntity:")) == 1,
                    "two docks are docked with each other and a console names one of them",
                    "the docks and the console do not name each other in the save");

                var copy = Documents.Copy(_document, [dock, console]);
                File.WriteAllText(Path.Combine(outFolder, "partners.copy.yml"), copy.Text);
                var invalid = Lines(copy.Text).Count(Invalid);
                Expect(invalid == 0 && !copy.Text.Contains("dockJointId") && copy.Log.Count == 0,
                    "copied without the other dock, no line of the text reads \"invalid\", and the dock names no joint",
                    $"copied without the other dock, {invalid} lines of the text read \"invalid\". {Messages(copy.Log)}");
                Expect(SameButTime(before, Save("partners.after-copy.yml")),
                    "after the copy the map saves as the same file",
                    "after the copy the map does not save as the same file");

                kernel.DrainLog();
                var pasted = Documents.Edit(_document, "Paste", [new EditOp("paste", Grid: grid, Text: copy.Text, FromX: copy.FromX, FromY: copy.FromY, X: 3, Y: 0)]);
                var after = Save("partners.pasted.yml");
                Expect(pasted.Created.Count == 2 && pasted.Log.Count == 0 && after.Count(Invalid) == before.Count(Invalid),
                    "pasted, the file has not one more line that reads \"invalid\", and nothing is logged",
                    $"pasted, {after.Count(Invalid) - before.Count(Invalid)} more lines of the file read \"invalid\". {Messages(pasted.Log)}");
                Documents.Undo(_document);
                Expect(SameButTime(before, Save("partners.undone.yml")),
                    "undo gives the save before the paste",
                    "undo does not give the save before the paste");
            }
            finally
            {
                Documents.Close(_document);
            }
        }

        // ---- one map ----

        private Clip Map(string map)
        {
            var name = Path.GetFileNameWithoutExtension(map);
            var scene = Opened(CheckTimes.Open(map), File.ReadAllText(Path.Combine(forkRepo, "Resources", map)), map);
            try
            {
                var sceneGrid = Begin(scene, name);
                var before = Save($"{name}.before.yml");
                var chosen = Choose(scene, sceneGrid);
                Info(chosen.Summary);
                foreach (var skipped in chosen.Skipped)
                    Skip(skipped);

                // ---- copy ----
                var clip = Copy(chosen, scene);
                File.WriteAllText(Path.Combine(outFolder, $"{name}.copy.yml"), clip.Text);
                Expect(SameButTime(before, Save($"{name}.after-copies.yml")),
                    "after the copies the map saves as the same file",
                    "after the copies the map does not save as the same file");

                // ---- refused pastes, while there is nothing to undo ----
                Refusals(clip, sceneGrid, before, scene.Entities.Count);

                // ---- paste beside ----
                var (x, y) = Beside(clip, []);
                var ground = Pave(clip, [(x, y, 0)]) ? Save($"{name}.paved.yml") : before;
                var pasted = Paste(clip, x, y, 0);
                Ok($"pasted {x - clip.Copy.FromX},{y - clip.Copy.FromY} cells over as one edit: {pasted.Entities.Count} new entities in {pasted.Ms} ms");
                Verify(clip, pasted);
                var after = Save($"{name}.pasted.yml");
                Compare(ground, after, clip, "the paste");

                // ---- undo and redo ----
                var undone = Undo();
                Expect(pasted.Entities.All(uid => undone.Removed.Contains(uid.Id)) && SameButTime(ground, Save($"{name}.paste-undone.yml")),
                    "undo: every copy is reported gone, and the save is the save before the paste",
                    $"undo: {pasted.Entities.Count(uid => !undone.Removed.Contains(uid.Id))} copies are not reported gone, or the save is not the save before the paste");
                var redone = Redo();
                var back = redone.Entities.Select(entity => entity.Uid).ToHashSet();
                Expect(pasted.Entities.All(uid => back.Contains(uid.Id)) && SameButTime(after, Save($"{name}.paste-redone.yml")),
                    "redo: every copy is reported back, and the save is the pasted save",
                    $"redo: {pasted.Entities.Count(uid => !back.Contains(uid.Id))} copies are not reported back, or the save is not the pasted save");
                Undo();
                var again = SameButTime(ground, Save($"{name}.paste-undone-again.yml"));
                Redo();
                Expect(again && SameButTime(after, Save($"{name}.paste-redone-again.yml")),
                    "undo and redo again: the same two saves",
                    "undo and redo again: not the same two saves");
                say("        the copies after undo and redo:");
                Verify(clip, pasted);

                // ---- the pasted file, opened ----
                Reopened($"{name}.pasted.yml", after, scene.Entities.Count + clip.Entities.Count);

                // ---- a second paste of the same text ----
                var (x2, y2) = Beside(clip, [(x, y)]);
                var ground2 = Pave(clip, [(x2, y2, 0)]) ? Save($"{name}.paved-twice.yml") : after;
                var second = Paste(clip, x2, y2, 0);
                say($"        a second paste of the same text, {x2 - clip.Copy.FromX},{y2 - clip.Copy.FromY} cells over, and then the first again:");
                Verify(clip, second);
                Verify(clip, pasted);
                Expect(!second.Entities.Overlaps(pasted.Entities),
                    "the two pastes share no entity",
                    "the two pastes share an entity");
                Compare(ground2, Save($"{name}.pasted-twice.yml"), null, "the second paste");
                Emptied(before, "both pastes");

                // ---- turned ----
                Turned(clip, sceneGrid, before);

                // ---- cut, and pasted where they stood ----
                Cut(chosen, clip, before);
                return clip;
            }
            finally
            {
                Documents.Close(scene.Document.Id);
            }
        }

        private void Cross(Clip clip, string map)
        {
            var name = Path.GetFileNameWithoutExtension(map);
            var scene = Opened(CheckTimes.Open(map), File.ReadAllText(Path.Combine(forkRepo, "Resources", map)), map);
            try
            {
                var sceneGrid = Begin(scene, name);
                var before = Save($"{name}.cross-before.yml");

                // To the middle of this map's grid, from wherever it was on the other. This map
                // has other rooms there, so tiles are painted first wherever there is none.
                var roots = clip.Entities.Values.Where(entity => entity.Parent == 0).ToArray();
                var (sumX, sumY, count) = (0L, 0L, 0);
                for (var i = 0; i + 4 < sceneGrid.Tiles.Count; i += 5)
                    (sumX, sumY, count) = (sumX + sceneGrid.Tiles[i], sumY + sceneGrid.Tiles[i + 1], count + 1);
                var x = clip.Copy.FromX + MathF.Round(sumX / (float) count - roots.Average(entity => entity.Position.X));
                var y = clip.Copy.FromY + MathF.Round(sumY / (float) count - roots.Average(entity => entity.Position.Y));

                var ground = Pave(clip, [(x, y, 0)]) ? Save($"{name}.cross-paved.yml") : before;
                kernel.DrainLog();
                var pasted = Paste(clip, x, y, 0);
                Ok($"pasted with its lowest cell at {x:0},{y:0} as one edit: {pasted.Entities.Count} new entities in {pasted.Ms} ms");
                Verify(clip, pasted);

                var after = Save($"{name}.cross-pasted.yml");
                Compare(ground, after, null, "the paste");
                Expect(after.Count(Invalid) == ground.Count(Invalid),
                    "the file has not one more line that reads \"invalid\"",
                    $"{after.Count(Invalid) - ground.Count(Invalid)} more lines of the file read \"invalid\"");

                Undo();
                var undone = SameButTime(ground, Save($"{name}.cross-undone.yml"));
                Redo();
                Expect(undone && SameButTime(after, Save($"{name}.cross-redone.yml")),
                    "undo gives the save before the paste, and redo the pasted save",
                    "undo and redo do not give the save before the paste and the pasted save");
                Reopened($"{name}.cross-pasted.yml", after, scene.Entities.Count + clip.Entities.Count);
                Emptied(before, "the paste");
            }
            finally
            {
                Documents.Close(scene.Document.Id);
            }
        }

        // ---- choosing ----

        private Chosen Choose(Scene scene, SceneGrid grid)
        {
            var onGrid = scene.Entities.Where(entity => !entity.Contained && entity.Parent == grid.Uid).ToArray();
            var byUid = onGrid.ToDictionary(entity => entity.Uid);
            var roots = new HashSet<int>();

            // Everything standing in the box around these, widened by a margin.
            int Box(IEnumerable<SceneEntity> around, float margin, int most)
            {
                var corners = around.ToArray();
                var (left, right) = (corners.Min(e => e.X) - margin, corners.Max(e => e.X) + margin);
                var (bottom, top) = (corners.Min(e => e.Y) - margin, corners.Max(e => e.Y) + margin);
                var inside = onGrid.Where(e => e.X >= left && e.X <= right && e.Y >= bottom && e.Y <= top).ToArray();
                if (inside.Length > most)
                    return -1;

                var added = 0;
                foreach (var entity in inside)
                    added += roots.Add(entity.Uid) ? 1 : 0;
                return added;
            }

            // Alarms with every device they list, and the room between them.
            var alarms = onGrid
                .Where(entity => entity.Prototype.Contains("AirAlarm", StringComparison.Ordinal) || entity.Prototype.Contains("FireAlarm", StringComparison.Ordinal))
                .Where(entity => _entMan.HasComponent<DeviceListComponent>(new EntityUid(entity.Uid)))
                .ToArray();
            var (fullAlarms, listed, around) = (0, 0, 0);
            foreach (var alarm in alarms)
            {
                var devices = _entMan.GetComponent<DeviceListComponent>(new EntityUid(alarm.Uid)).Devices;
                if (fullAlarms >= 4 || devices.Count == 0 || !devices.All(device => byUid.ContainsKey(device.Id)))
                    continue;

                var added = Box(devices.Select(device => byUid[device.Id]).Append(alarm), 0.5f, 600);
                if (added < 0)
                    continue;

                fullAlarms++;
                listed += devices.Count;
                around += added;
            }

            // Buttons, switches and the like with all they are linked to.
            var sources = onGrid
                .Where(entity => !alarms.Contains(entity))
                .Where(entity => _entMan.TryGetComponent(new EntityUid(entity.Uid), out DeviceLinkSourceComponent? source) && source.LinkedPorts.Count > 0)
                .OrderByDescending(entity => entity.Prototype.Contains("Button", StringComparison.Ordinal) || entity.Prototype.Contains("Switch", StringComparison.Ordinal))
                .ToArray();
            var (fullSources, linked) = (0, 0);
            foreach (var source in sources)
            {
                var sinks = _entMan.GetComponent<DeviceLinkSourceComponent>(new EntityUid(source.Uid)).LinkedPorts.Keys;
                if (fullSources >= 4 || !sinks.All(sink => byUid.ContainsKey(sink.Id)))
                    continue;

                fullSources++;
                linked += sinks.Count;
                roots.Add(source.Uid);
                foreach (var sink in sinks)
                    roots.Add(sink.Id);
            }

            // The fullest lockers and crates, with the walls and loose things about them.
            var holders = scene.Entities.Where(entity => entity.Contained).GroupBy(entity => entity.Parent).ToDictionary(group => group.Key, group => group.Count());
            var (containers, contents) = (0, 0);
            foreach (var (holder, count) in holders.OrderByDescending(pair => pair.Value).ThenBy(pair => pair.Key))
            {
                if (containers >= 12 || !byUid.TryGetValue(holder, out var entity))
                    continue;

                containers++;
                contents += count;
                roots.Add(holder);
                around += Math.Max(0, Box([entity], 1.5f, 200));
            }

            // On purpose: an alarm without some of the devices it lists.
            var (partialAlarm, leftOut) = (EntityUid.Invalid, new List<EntityUid>());
            foreach (var alarm in alarms)
            {
                var devices = _entMan.GetComponent<DeviceListComponent>(new EntityUid(alarm.Uid)).Devices;
                var outside = devices.Where(device => !roots.Contains(device.Id)).ToList();
                if (roots.Contains(alarm.Uid) || devices.Count < 2 || outside.Count < 2 || !devices.All(device => byUid.ContainsKey(device.Id)))
                    continue;

                partialAlarm = new EntityUid(alarm.Uid);
                leftOut = outside.Take(outside.Count / 2).ToList();
                roots.Add(alarm.Uid);
                foreach (var device in devices.Except(leftOut))
                    roots.Add(device.Id);
                break;
            }

            // On purpose: something linked to one thing, without that thing.
            var partialSource = EntityUid.Invalid;
            foreach (var source in sources)
            {
                var sinks = _entMan.GetComponent<DeviceLinkSourceComponent>(new EntityUid(source.Uid)).LinkedPorts.Keys;
                if (roots.Contains(source.Uid) || sinks.Any(sink => roots.Contains(sink.Id) || sink == partialAlarm || leftOut.Contains(sink)))
                    continue;

                if (sinks.Count == 1)
                {
                    partialSource = new EntityUid(source.Uid);
                    break;
                }
            }

            if (partialSource.IsValid())
                roots.Add(partialSource.Id);

            var summary = $"chosen: {fullAlarms} alarms with the {listed} devices they list, {fullSources} linked things with the {linked} they are linked to, {containers} containers holding {contents} things, and what stands around them, {roots.Count} entities of the grid in all. "
                + (partialAlarm.IsValid() ? $"On purpose {Name(partialAlarm)}, which lists {_entMan.GetComponent<DeviceListComponent>(partialAlarm).Devices.Count} devices, without {leftOut.Count} of them. " : "No alarm could be chosen without some of its devices. ")
                + (partialSource.IsValid() ? $"On purpose {Name(partialSource)} without the one thing it is linked to." : "Nothing linked to one thing could be chosen without it.");
            var skipped = new List<string>();
            if (fullAlarms == 0)
                skipped.Add("an alarm copied with every device it lists: the grid has none whose devices all stand on it");
            if (fullSources == 0)
                skipped.Add("a linked thing copied with all it is linked to: the grid has none");
            if (containers == 0)
                skipped.Add("a container copied with what it holds: the grid has none that holds anything");
            if (!partialAlarm.IsValid())
                skipped.Add("an alarm copied without some of its devices: no alarm could be chosen so");
            if (!partialSource.IsValid())
                skipped.Add("a linked thing copied without the one thing it is linked to: nothing could be chosen so");
            return new Chosen(roots.Select(uid => new EntityUid(uid)).ToHashSet(), partialAlarm, leftOut, partialSource, summary, skipped);
        }

        private string Name(EntityUid uid) =>
            $"{_entMan.GetComponent<MetaDataComponent>(uid).EntityPrototype?.ID} (uid {Yaml(uid)} in the file)";

        private int Yaml(EntityUid uid) => _entMan.TryGetComponent(uid, out YamlUidComponent? yaml) ? yaml.Uid : 0;

        /// <summary>The fields in which an entity lists other entities, and what each lists.</summary>
        private IEnumerable<(string Field, IReadOnlyCollection<EntityUid> Uids)> Lists(EntityUid uid)
        {
            if (_entMan.TryGetComponent(uid, out DeviceListComponent? list))
            {
                yield return ("DeviceList.devices", list.Devices);
                yield return ("DeviceList.configurators", list.Configurators);
            }

            if (_entMan.TryGetComponent(uid, out ListedDeviceComponent? network))
            {
                yield return ("DeviceNetwork.deviceLists", network.DeviceLists);
                yield return ("DeviceNetwork.configurators", network.Configurators);
            }

            if (_entMan.TryGetComponent(uid, out DeviceLinkSourceComponent? source))
                yield return ("DeviceLinkSource.linkedPorts", source.LinkedPorts.Keys);
        }

        /// <summary>These and all they hold, as the serializer gathers them: nothing the game does not save.</summary>
        private HashSet<EntityUid> Descendants(IEnumerable<EntityUid> roots)
        {
            var all = new HashSet<EntityUid>();
            void Add(EntityUid uid)
            {
                if (_entMan.GetComponent<MetaDataComponent>(uid).EntityPrototype?.MapSavable == false)
                    return;

                all.Add(uid);
                var children = _entMan.GetComponent<TransformComponent>(uid).ChildEnumerator;
                while (children.MoveNext(out var child))
                    Add(child);
            }

            foreach (var root in roots)
                Add(root);
            return all;
        }

        private Clip Remember(Chosen chosen, CopyResult copy)
        {
            var all = Descendants(chosen.Roots);
            var entities = new Dictionary<int, Copied>();
            foreach (var uid in all)
            {
                var yaml = Yaml(uid);
                if (yaml <= 0 || entities.ContainsKey(yaml))
                    throw new InvalidOperationException($"entity {uid} has no number of its own from the file ({yaml})");

                var xform = _entMan.GetComponent<TransformComponent>(uid);
                var inside = new Dictionary<string, int[]>();
                var outside = new Dictionary<string, int>();
                foreach (var (field, uids) in Lists(uid))
                {
                    inside[field] = uids.Where(all.Contains).Select(Yaml).ToArray();
                    outside[field] = uids.Count(listed => !all.Contains(listed));
                }

                var holds = new Dictionary<string, int[]>();
                if (_entMan.TryGetComponent(uid, out ContainerManagerComponent? manager))
                {
                    foreach (var (id, container) in manager.Containers)
                        holds[id] = container.ContainedEntities.Where(all.Contains).Select(Yaml).ToArray();
                }

                entities[yaml] = new Copied(
                    yaml,
                    _entMan.GetComponent<MetaDataComponent>(uid).EntityPrototype?.ID ?? "",
                    chosen.Roots.Contains(uid) ? 0 : Yaml(xform.ParentUid),
                    xform.LocalPosition, xform.LocalRotation, xform.Anchored,
                    inside, outside, holds);
            }

            var order = Lines(copy.Text)
                .Where(line => line.StartsWith("  - uid: ", StringComparison.Ordinal))
                .Select(line => int.Parse(line.AsSpan(9)))
                .ToArray();
            return new Clip(copy, entities, order, chosen.Alarm.IsValid() ? Yaml(chosen.Alarm) : 0, chosen.Source.IsValid() ? Yaml(chosen.Source) : 0);
        }

        // ---- copying ----

        private static string? Refused(Action act)
        {
            try
            {
                act();
                return null;
            }
            catch (KernelException e)
            {
                return e.Message;
            }
        }

        /// <summary>The kernel's copy of the chosen set, and what a copy must be.</summary>
        private Clip Copy(Chosen chosen, Scene scene)
        {
            var uids = chosen.Roots.Select(uid => uid.Id).ToList();
            kernel.DrainLog();
            var sw = Stopwatch.StartNew();
            var copy = Documents.Copy(_document, uids);
            var ms = sw.ElapsedMilliseconds;
            if (ms > _largestCopy.Ms)
                _largestCopy = (ms, $"{copy.Entities} entities of {_name}, {copy.Text.Length / 1024} KB");

            var clip = Remember(chosen, copy);
            var lowest = clip.Entities.Values.Where(entity => entity.Parent == 0).Aggregate(
                (X: int.MaxValue, Y: int.MaxValue),
                (low, entity) => (Math.Min(low.X, (int) MathF.Floor(entity.Position.X)), Math.Min(low.Y, (int) MathF.Floor(entity.Position.Y))));
            Expect(copy.Roots == chosen.Roots.Count && copy.Entities == clip.Entities.Count && (copy.FromX, copy.FromY) == lowest && copy.Log.Count == 0,
                $"copied by the kernel: {copy.Roots} entities of the grid and the {copy.Entities - copy.Roots} they hold, measured from cell {copy.FromX},{copy.FromY}, in {ms} ms, {copy.Text.Length / 1024} KB, nothing logged",
                $"the copy says {copy.Roots} entities of the grid, {copy.Entities} in all, from cell {copy.FromX},{copy.FromY}. Expected {chosen.Roots.Count}, {clip.Entities.Count}, from {lowest.X},{lowest.Y}. {Messages(copy.Log)}");
            Info($"the text: {Header(copy.Text)}");

            Expect(clip.Order.ToHashSet().SetEquals(clip.Entities.Keys) && clip.Order.Length == clip.Entities.Count,
                $"the text holds the {clip.Order.Length} entities expected, each under the number it has in the map file",
                $"the text holds {clip.Order.Length} entities and {clip.Entities.Count} were expected ({clip.Order.Except(clip.Entities.Keys).Count()} not expected, {clip.Entities.Keys.Except(clip.Order).Count()} missing)");

            var outside = clip.Entities.Values.Sum(entity => entity.Outside.Values.Sum());
            var severed = clip.Entities.Values.Count(entity => entity.Outside.GetValueOrDefault("DeviceLinkSource.linkedPorts") >= 2);
            var invalid = Lines(copy.Text).Count(Invalid);
            Expect(invalid == 0,
                $"no line of the text reads \"invalid\". The set names {outside} things outside it, and {severed} of the set are linked to two or more of them",
                $"{invalid} lines of the text read \"invalid\"");

            // Given a second time, the same text. And the set is not changed by having been copied.
            var twice = Documents.Copy(_document, uids);
            Expect(SameText(twice.Text, copy.Text),
                "copied again, the text is the same",
                "copied again, the text is not the same");

            // A thing inside another of those given is not copied twice.
            var held = Descendants(chosen.Roots).Where(uid => !chosen.Roots.Contains(uid)).Take(8).Select(uid => uid.Id).ToList();
            if (held.Count > 0)
            {
                var with = Documents.Copy(_document, uids.Concat(held).ToList());
                Expect(SameText(with.Text, copy.Text) && with.Roots == copy.Roots && with.Entities == copy.Entities,
                    $"given {held.Count} of the held things as well, the copy is the same",
                    $"given {held.Count} of the held things as well, the copy is not the same: {with.Roots} entities of the grid, {with.Entities} in all");

                var alone = Refused(() => Documents.Copy(_document, [held[0]]));
                Expect(alone != null,
                    $"a held thing given alone is refused: \"{alone}\"",
                    "a held thing given alone was copied, and it is no child of a grid");
            }
            else
            {
                Skip("a copy given held things as well, and a held thing given alone: the copy holds nothing");
            }

            var grids = scene.Grids.Select(grid => grid.Uid).ToHashSet();
            var elsewhere = scene.Entities.FirstOrDefault(entity => entity.Parent != _grid.Id && grids.Contains(entity.Parent));
            if (elsewhere != null)
            {
                var two = Refused(() => Documents.Copy(_document, [uids[0], elsewhere.Uid]));
                Expect(two != null,
                    $"entities of two grids are refused: \"{two}\"",
                    "entities of two grids were copied as one set");
            }
            else
            {
                Skip("entities of two grids, refused: the map has no second grid with an entity on it");
            }

            var none = Refused(() => Documents.Copy(_document, []));
            var grid = Refused(() => Documents.Copy(_document, [_grid.Id]));
            Expect(none != null && grid != null,
                $"no entity, and the grid itself, are refused: \"{none}\", \"{grid}\"",
                "a copy of no entity, or of the grid itself, was made");
            return clip;
        }

        // ---- places ----

        /// <summary>Where the kernel is to put an entity of the grid that stood at this place.</summary>
        private static Vector2 Placed(Clip clip, Vector2 position, float x, float y, int quarters)
        {
            if (quarters == 0)
                return position + new Vector2(x - clip.Copy.FromX, y - clip.Copy.FromY);

            var (dx, dy) = (position.X - (clip.Copy.FromX + 0.5), position.Y - (clip.Copy.FromY + 0.5));
            for (var i = 0; i < quarters; i++)
                (dx, dy) = (-dy, dx);
            return new Vector2((float) (x + 0.5 + dx), (float) (y + 0.5 + dy));
        }

        /// <summary>For each entity of the grid that would come down where there is no tile, the cell.</summary>
        private List<Vector2i> Off(Clip clip, float x, float y, int quarters)
        {
            var gridComp = _entMan.GetComponent<MapGridComponent>(_grid);
            var off = new List<Vector2i>();
            foreach (var entity in clip.Entities.Values)
            {
                if (entity.Parent != 0)
                    continue;

                var place = Placed(clip, entity.Position, x, y, quarters);
                var cell = new Vector2i((int) MathF.Floor(place.X), (int) MathF.Floor(place.Y));
                if (Maps.GetTileRef(_grid, gridComp, cell).Tile.IsEmpty)
                    off.Add(cell);
            }

            return off;
        }

        /// <summary>
        /// A place a whole number of cells from where the copy stood, the nearest that puts
        /// all of it over tiles, or else the one that puts the most of it over tiles.
        /// </summary>
        private (float X, float Y) Beside(Clip clip, (float X, float Y)[] taken)
        {
            var offsets = new List<(int X, int Y)>();
            for (var dx = -12; dx <= 12; dx++)
            for (var dy = -12; dy <= 12; dy++)
            {
                if ((dx != 0 || dy != 0) && !taken.Contains((clip.Copy.FromX + dx, clip.Copy.FromY + dy)))
                    offsets.Add((dx, dy));
            }

            var (best, least) = ((X: 3, Y: 0), int.MaxValue);
            foreach (var offset in offsets.OrderBy(offset => offset.X * offset.X + offset.Y * offset.Y).ThenBy(offset => offset.X).ThenBy(offset => offset.Y))
            {
                var off = Off(clip, clip.Copy.FromX + offset.X, clip.Copy.FromY + offset.Y, 0).Count;
                if (off < least)
                    (best, least) = (offset, off);
                if (off == 0)
                    break;
            }

            return (clip.Copy.FromX + best.X, clip.Copy.FromY + best.Y);
        }

        /// <summary>
        /// Paints a tile, as one edit, on each cell these pastes would need and that has none.
        /// False if every cell has one.
        /// </summary>
        private bool Pave(Clip clip, (float X, float Y, int Quarters)[] pastes)
        {
            var cells = pastes.SelectMany(paste => Off(clip, paste.X, paste.Y, paste.Quarters)).Distinct().ToList();
            if (cells.Count == 0)
                return false;

            using (CheckTimes.In(CheckTimes.Edit))
                Documents.Edit(_document, "Paint tiles", cells.Select(cell => new EditOp("tile", Grid: _grid.Id, X: cell.X, Y: cell.Y, Tile: _floor)).ToList());
            _steps++;
            Info($"{cells.Count} cells the paste needs have no tile. {_floor} is painted on them first, as one edit");
            return true;
        }

        // ---- pasting ----

        private EditOp PasteOp(Clip clip, float x, float y, int quarters) =>
            new("paste", Grid: _grid.Id, X: x, Y: y, Text: clip.Text, FromX: clip.Copy.FromX, FromY: clip.Copy.FromY, Quarters: quarters);

        /// <summary>
        /// One edit with one paste op. The copies are told apart by the order they were made
        /// in, which is the order the text has them in: the kernel takes the text's numbers off.
        /// </summary>
        private Pasted Paste(Clip clip, float x, float y, int quarters)
        {
            var label = $"Paste {clip.Roots} entities";
            kernel.DrainLog();
            var sw = Stopwatch.StartNew();
            SceneDelta delta;
            using (CheckTimes.In(CheckTimes.Edit))
                delta = Documents.Edit(_document, label, [PasteOp(clip, x, y, quarters)]);
            var ms = sw.ElapsedMilliseconds;
            _steps++;
            if (ms > _largestPaste.Ms)
                _largestPaste = (ms, $"{clip.Entities.Count} entities into {_name}");

            var fresh = delta.Entities.Where(entity => !_known.Contains(entity.Uid)).OrderBy(entity => entity.Uid).ToArray();
            if (fresh.Length != clip.Order.Length)
                throw new InvalidOperationException($"the paste reports {fresh.Length} new entities and the text holds {clip.Order.Length}");

            var byYaml = new Dictionary<int, EntityUid>();
            var (others, strays) = (0, 0);
            for (var i = 0; i < fresh.Length; i++)
            {
                byYaml[clip.Order[i]] = new EntityUid(fresh[i].Uid);
                others += fresh[i].Prototype == clip.Entities[clip.Order[i]].Prototype ? 0 : 1;
                strays += fresh[i].YamlUid == 0 ? 0 : 1;
                _known.Add(fresh[i].Uid);
            }

            var roots = clip.Entities.Values.Where(entity => entity.Parent == 0).Select(entity => byYaml[entity.Yaml].Id).ToHashSet();
            Expect(others == 0 && strays == 0 && delta.Entities.Count == fresh.Length && delta.Removed.Count == 0 && roots.SetEquals(delta.Created) && delta.Created.Count == roots.Count
                   && delta.Tiles.Count == 0 && delta.Details.Count == 0 && delta.Log.Count == 0 && delta.History.UndoLabel == label,
                $"the edit reports the {fresh.Length} copies and nothing else, each of the prototype the text has in that place and with no number from a file. Created are the {roots.Count} of the grid. Nothing logged, and the step to undo is \"{label}\"",
                $"the edit reports {delta.Entities.Count} entities, {fresh.Length} of them new, {others} of another prototype than the text has in that place, {strays} with a number from a file, {delta.Removed.Count} removed, {delta.Created.Count} created for {roots.Count} of the grid, {delta.Tiles.Count} tiles, {delta.Details.Count} with changed components. {Messages(delta.Log)}. The step to undo is \"{delta.History.UndoLabel}\"");
            return new Pasted(byYaml, fresh.Select(entity => new EntityUid(entity.Uid)).ToHashSet(), delta, ms, x, y, quarters);
        }

        private SceneDelta Undo()
        {
            _steps--;
            using var timed = CheckTimes.In(CheckTimes.Edit);
            return Documents.Undo(_document);
        }

        private SceneDelta Redo()
        {
            _steps++;
            using var timed = CheckTimes.In(CheckTimes.Edit);
            return Documents.Redo(_document);
        }

        /// <summary>Every step undone: the save must be the first one, and nothing left to undo.</summary>
        private void Emptied(string[] before, string what)
        {
            HistoryState? last = null;
            while (_steps > 0)
                last = Undo().History;
            // With nothing to undo this changes nothing and says so.
            var history = last ?? Documents.Undo(_document).History;
            Expect(!history.CanUndo && SameButTime(before, Save($"{_name}.emptied.yml")),
                $"with {what} undone the save is the first save, and nothing is left to undo",
                $"with {what} undone the save is not the first save, or there is more to undo (\"{history.UndoLabel}\")");
        }

        /// <summary>
        /// Pastes the kernel must refuse, tried while the document has no history. Each must
        /// leave the entities, the history and the save as they were.
        /// </summary>
        private void Refusals(Clip clip, SceneGrid sceneGrid, string[] before, int entities)
        {
            // Maps are not counted. The kernel makes one of its own the first time an entity
            // is deleted, to park it on, and keeps it.
            int Count() => _entMan.EntityCount - _entMan.Count<MapComponent>();
            var count = Count();
            var far = Enumerable.Range(0, sceneGrid.Tiles.Count / 5).Max(i => sceneGrid.Tiles[i * 5]) + 400;
            kernel.DrainLog();

            var whole = Refused(() => Documents.Edit(_document, "Paste", [PasteOp(clip, far, clip.Copy.FromY, 0)]));
            Expect(whole != null && whole.Contains($"{clip.Roots} of the {clip.Roots} ", StringComparison.Ordinal),
                $"a paste onto cells with no tile is refused: \"{whole}\"",
                $"a paste onto cells with no tile was not refused as it should be: \"{whole}\"");

            // Some over tiles and some not.
            for (var dx = 6; dx < 400; dx += 6)
            {
                var off = Off(clip, clip.Copy.FromX + dx, clip.Copy.FromY, 0).Count;
                if (off == 0 || off > clip.Roots / 2)
                    continue;

                var part = Refused(() => Documents.Edit(_document, "Paste", [PasteOp(clip, clip.Copy.FromX + dx, clip.Copy.FromY, 0)]));
                Expect(part != null && part.Contains($"{off} of the {clip.Roots} ", StringComparison.Ordinal),
                    $"and so is one that would put only some on such cells, whole: \"{part}\"",
                    $"a paste that would put {off} entities on cells with no tile was not refused as it should be: \"{part}\"");
                break;
            }

            var turned = Refused(() => Documents.Edit(_document, "Paste", [PasteOp(clip, far, clip.Copy.FromY, 3)]));
            Expect(turned != null,
                "and a turned one",
                "a turned paste onto cells with no tile was not refused");

            // After other ops of the same edit, which must be taken back with it.
            var root = clip.Entities.Values.First(entity => entity.Parent == 0);
            var original = _known.Select(uid => new EntityUid(uid)).First(uid => Yaml(uid) == root.Yaml);
            var mixed = Refused(() => Documents.Edit(_document, "Paste",
            [
                new EditOp("tile", Grid: _grid.Id, X: far, Y: clip.Copy.FromY, Tile: _floor),
                new EditOp("spawn", Parent: _grid.Id, X: root.Position.X, Y: root.Position.Y, Prototype: root.Prototype),
                new EditOp("delete", Uid: original.Id),
                PasteOp(clip, far + 50, clip.Copy.FromY, 0),
            ]));
            Expect(mixed != null,
                "and one that comes after a tile, a spawn and a delete in the same edit",
                "a paste onto cells with no tile, after other ops in the same edit, was not refused");

            var notText = Refused(() => Documents.Edit(_document, "Paste", [PasteOp(clip with { Copy = clip.Copy with { Text = "meta: [" } }, clip.Copy.FromX, clip.Copy.FromY, 0)]));
            var notEntities = Refused(() => Documents.Edit(_document, "Paste", [PasteOp(clip with { Copy = clip.Copy with { Text = "a: 1\n" } }, clip.Copy.FromX, clip.Copy.FromY, 0)]));
            var noGrid = Refused(() => Documents.Edit(_document, "Paste", [PasteOp(clip, clip.Copy.FromX, clip.Copy.FromY, 0) with { Grid = original.Id }]));
            Expect(notText != null && notEntities != null && noGrid != null,
                $"text that is no entity text is refused, and a paste onto what is no grid: \"{Short(notText)}\", \"{Short(notEntities)}\", \"{noGrid}\"",
                $"text that is no entity text, or a paste onto what is no grid, was not refused: \"{Short(notText)}\", \"{Short(notEntities)}\", \"{noGrid}\"");

            var log = kernel.DrainLog();
            if (log.Count > 0)
                Found($"while those were refused the engine logged: {Messages(log)}");

            var history = Documents.Undo(_document).History;
            var scene = Documents.GetScene(_document);
            Expect(Count() == count && scene.Entities.Count == entities && !history.CanUndo && !history.CanRedo && history.Revision == 0 && SameButTime(before, Save($"{_name}.refused.yml")),
                "after them the engine has as many entities as before, the map has its own, there is nothing to undo or redo, and the save is the first save",
                $"after them the engine has {Count()} entities and had {count},the map has {scene.Entities.Count} and had {entities}, undo {history.CanUndo}, redo {history.CanRedo}, revision {history.Revision}, or the save is not the first save");
        }

        private static string Short(string? text) => text == null ? "" : (text.Length > 110 ? text[..110] + "..." : text).Replace('\n', ' ').Replace("\r", "");

        /// <summary>
        /// Turned pastes, far to the right of the map on tiles painted for them: one turned a
        /// quarter, and then four turned none to three quarters about the same cell.
        /// </summary>
        private void Turned(Clip clip, SceneGrid sceneGrid, string[] before)
        {
            var roots = clip.Entities.Values.Where(entity => entity.Parent == 0).ToArray();
            var reach = (int) MathF.Ceiling(MathF.Max(roots.Max(entity => entity.Position.X) - clip.Copy.FromX, roots.Max(entity => entity.Position.Y) - clip.Copy.FromY)) + 2;
            var x = Enumerable.Range(0, sceneGrid.Tiles.Count / 5).Max(i => sceneGrid.Tiles[i * 5]) + 8 + reach;
            var y = clip.Copy.FromY;

            say("        turned, on tiles painted far from the map:");
            Pave(clip, [(x, y, 0), (x, y, 1), (x, y, 2), (x, y, 3)]);
            var paved = Save($"{_name}.turn-paved.yml");

            var quarter = Paste(clip, x, y, 1);
            Verify(clip, quarter);
            Compare(paved, Save($"{_name}.turned.yml"), null, "the paste turned a quarter");
            Undo();
            Expect(SameButTime(paved, Save($"{_name}.turn-undone.yml")),
                "undone, the save is the save before it",
                "undone, the save is not the save before it");

            var four = Enumerable.Range(0, 4).Select(quarters => Paste(clip, x, y, quarters)).ToArray();
            foreach (var pasted in four)
                Verify(clip, pasted);
            Compare(paved, Save($"{_name}.turned-four.yml"), null, "four pastes turned none to three quarters");
            for (var i = 0; i < 4; i++)
                Undo();
            var cleared = SameButTime(paved, Save($"{_name}.turned-four-undone.yml"));
            Expect(cleared,
                "the four undone, the save is the save before them",
                "the four undone, the save is not the save before them");
            Emptied(before, "the tiles too");
        }

        /// <summary>The set copied, deleted as one edit, and pasted where it stood.</summary>
        private void Cut(Chosen chosen, Clip clip, string[] before)
        {
            say("        cut, and pasted where they stood:");
            var copy = Documents.Copy(_document, chosen.Roots.Select(uid => uid.Id).ToList());
            Expect(SameText(copy.Text, clip.Text),
                "copied for the cut, after all that was pasted and undone, the text is the first text",
                "copied for the cut, the text is not the first text");

            var label = $"Cut {copy.Roots} entities";
            SceneDelta cut;
            using (CheckTimes.In(CheckTimes.Edit))
                cut = Documents.Edit(_document, label, chosen.Roots.Select(uid => new EditOp("delete", Uid: uid.Id)).ToList());
            _steps++;
            var all = Descendants(chosen.Roots);
            Expect(all.All(uid => cut.Removed.Contains(uid.Id)) && cut.History.UndoLabel == label,
                $"the cut is one edit that reports all {all.Count} gone, with what they held",
                $"the cut does not report {all.Count(uid => !cut.Removed.Contains(uid.Id))} of {all.Count} gone, or the step to undo is \"{cut.History.UndoLabel}\"");

            var pasted = Paste(clip with { Copy = copy }, copy.FromX, copy.FromY, 0);
            Verify(clip, pasted);
            var after = Save($"{_name}.cut-pasted.yml");
            Expect(after.Count(Invalid) == before.Count(Invalid),
                "the file has not one more line that reads \"invalid\"",
                $"{after.Count(Invalid) - before.Count(Invalid)} more lines of the file read \"invalid\"");

            var history = Undo().History;
            history = Undo().History;
            Expect(!history.CanUndo && SameButTime(before, Save($"{_name}.cut-undone.yml")),
                "undo twice gives the first save, with nothing left to undo",
                $"undo twice does not give the first save, or there is more to undo (\"{history.UndoLabel}\")");
        }

        /// <summary>A saved file opened as another document, saved again and closed.</summary>
        private void Reopened(string file, string[] saved, int entities)
        {
            var reopened = Opened(CheckTimes.Reopen, File.ReadAllText(Path.Combine(outFolder, file)), file);
            try
            {
                var result = Saving(reopened.Document.Id);
                var again = Written(file.Replace(".yml", "-again.yml", StringComparison.Ordinal), result.Yaml ?? "");
                Expect(reopened.Entities.Count == entities && SameButTime(saved, again),
                    $"the pasted file opened has {reopened.Entities.Count} entities, those there were and those pasted, and saved again it is the same file",
                    $"the pasted file opened has {reopened.Entities.Count} entities and {entities} were expected. Saved again, {Differing(saved, again)} lines differ");
                if (reopened.Document.Log.Count > 0)
                    Found($"opening the file with the paste in it, the engine logged: {Messages(reopened.Document.Log)}");
            }
            finally
            {
                Documents.Close(reopened.Document.Id);
            }
        }

        // ---- looking at what was pasted ----

        /// <summary>
        /// An angle a quarter turn on, against the clock, as the editor counts it (ByQuarters
        /// in RegionCheck.cs, quarter_on in tools/select.rs).
        /// </summary>
        private static double ByQuarters(double angle)
        {
            var quarters = Math.Floor(angle / Quarter + 0.5);
            var rest = angle - quarters * Quarter;
            var first = 4 * Math.Floor((quarters + 1) / 4) - 1;
            var next = quarters + 1;
            if (next > first + 3)
                next -= 4;
            return next * Quarter + rest;
        }

        private void Verify(Clip clip, Pasted pasted)
        {
            var gridComp = _entMan.GetComponent<MapGridComponent>(_grid);
            var copies = pasted.Entities;
            var quarters = pasted.Quarters;

            // Parent, place and facing.
            var (roots, misplaced, held, misheld) = (0, 0, 0, 0);
            var (misfaced, unturned, unturnedFreely, reduced) = (0, 0, 0, 0);
            var (wasAnchored, anchored, notOnLookup, wronglyAnchored) = (0, 0, 0, 0);
            foreach (var (yaml, was) in clip.Entities)
            {
                var uid = pasted.ByYaml[yaml];
                var xform = _entMan.GetComponent<TransformComponent>(uid);
                if (was.Parent != 0)
                {
                    held++;
                    if (xform.ParentUid != pasted.ByYaml[was.Parent])
                        misheld++;
                    continue;
                }

                roots++;
                var place = Placed(clip, was.Position, pasted.X, pasted.Y, quarters);
                if (xform.ParentUid != _grid || (xform.LocalPosition - place).Length() > 0.001f)
                    misplaced++;

                var facing = was.Rotation.Theta;
                for (var i = 0; i < quarters; i++)
                    facing = ByQuarters(facing);
                var now = xform.LocalRotation.Theta;
                if (Math.Abs(now - facing) <= 1e-9)
                {
                }
                else if (quarters != 0 && now == was.Rotation.Theta)
                {
                    unturned++;
                    unturnedFreely += xform.NoLocalRotation ? 0 : 1;
                }
                else if (Math.Abs(now - new Angle(facing).Reduced().Theta) <= 1e-9)
                {
                    reduced++;
                }
                else
                {
                    misfaced++;
                }

                var cell = new Vector2i((int) MathF.Floor(place.X), (int) MathF.Floor(place.Y));
                if (!was.Anchored)
                {
                    wronglyAnchored += xform.Anchored ? 1 : 0;
                    continue;
                }

                wasAnchored++;
                anchored += xform.Anchored ? 1 : 0;
                if (!Maps.GetAnchoredEntities(_grid, gridComp, cell).Contains(uid))
                    notOnLookup++;
            }

            var where = quarters == 0
                ? "where they stood plus the offset, facing as they did"
                : $"turned {quarters} quarters about the cell they are measured from, each facing {quarters} quarters on";
            Expect(misplaced == 0 && misfaced == 0 && unturnedFreely == 0,
                $"all {roots} entities of the grid are children of the grid, {where}",
                $"of {roots} entities of the grid, {misplaced} are not children of the grid {where}. {misfaced} face otherwise, and {unturnedFreely} that the engine does turn face as before");
            if (unturned > 0)
                Found($"{unturned} of them face as their originals do. They are of the kind the engine never turns (NoLocalRotation).");
            if (reduced > 0)
                Found($"{reduced} of them face that way less whole turns. The engine takes whole turns off an angle it is given with a place (Angle.Reduced in SetCoordinates).");
            Expect(anchored == wasAnchored && notOnLookup == 0 && wronglyAnchored == 0,
                $"all {wasAnchored} of them that were anchored are anchored, and the grid finds each on its tile. None that was loose is anchored",
                $"of {wasAnchored} of them that were anchored, {anchored} are anchored and {notOnLookup} are not found by the grid on their tile. {wronglyAnchored} that were loose are anchored");

            // Containers.
            var (containers, contents, strangers) = (0, 0, 0);
            foreach (var (yaml, was) in clip.Entities)
            {
                if (was.Holds.Values.All(inside => inside.Length == 0))
                    continue;

                containers++;
                var manager = _entMan.GetComponent<ContainerManagerComponent>(pasted.ByYaml[yaml]);
                foreach (var (id, inside) in was.Holds)
                {
                    var now = manager.Containers.TryGetValue(id, out var container) ? container.ContainedEntities.ToHashSet() : [];
                    contents += inside.Length;
                    if (!now.SetEquals(inside.Select(number => pasted.ByYaml[number])))
                        strangers++;
                }
            }

            Expect(strangers == 0 && misheld == 0,
                $"all {containers} pasted containers hold the copies of the {contents} things theirs held and nothing else, and all {held} held things are children of the copy of what held them",
                $"{strangers} containers of {containers} pasted do not hold just the copies of what theirs held. {misheld} of {held} held things are not children of the copy of what held them");

            // Lists of entities: the copies of what was named in the copy, and nothing else.
            var (lists, named, missing, outside) = (0, 0, 0, 0);
            var (holdInvalid, holdOriginal, holdOther) = (0, 0, 0);
            foreach (var (yaml, was) in clip.Entities)
            {
                foreach (var (field, uids) in Lists(pasted.ByYaml[yaml]))
                {
                    var now = uids.ToHashSet();
                    var inside = was.Inside.GetValueOrDefault(field) ?? [];
                    var expected = inside.Select(number => pasted.ByYaml[number]).ToHashSet();
                    if (inside.Length > 0)
                    {
                        lists++;
                        named += inside.Length;
                        missing += expected.Count(uid => !now.Contains(uid));
                    }

                    outside += was.Outside.GetValueOrDefault(field);
                    now.ExceptWith(expected);
                    holdInvalid += now.Count(uid => !uid.IsValid());
                    holdOriginal += now.Count(uid => uid.IsValid() && !copies.Contains(uid));
                    holdOther += now.Count(copies.Contains);
                }
            }

            Expect(missing == 0 && holdInvalid == 0 && holdOriginal == 0 && holdOther == 0,
                $"the {lists} pasted lists name the copies of the {named} they named in the copy and nothing else. The {outside} things named outside the copy are gone from them: no invalid uid, no original, no entity of another paste",
                $"of {named} named in {lists} pasted lists, {missing} copies are not named. The lists hold {holdInvalid} invalid uids, {holdOriginal} entities that are not of this paste, and {holdOther} copies they should not");

            // The two chosen on purpose, in full.
            foreach (var (what, yaml) in new[] { ("the alarm chosen on purpose", clip.Alarm), ("the linked thing chosen on purpose", clip.Source) })
            {
                if (yaml == 0)
                    continue;

                var parts = Lists(pasted.ByYaml[yaml])
                    .Where(list => list.Uids.Count > 0)
                    .Select(list => $"{list.Field} holds {string.Join(", ", list.Uids.Select(uid => !uid.IsValid() ? "invalid" : copies.Contains(uid) ? "a copy" : "A STRANGER").GroupBy(kind => kind).Select(group => $"{group.Count()} {group.Key}"))}");
                Info($"{what}, pasted: {string.Join(". ", parts.DefaultIfEmpty("it names nothing"))}");
            }
        }

        // ---- the saved file ----

        private string[] Save(string file)
        {
            var saved = Saving(_document);
            if (!saved.Ok || saved.Yaml == null)
                throw new InvalidOperationException($"not saved: {saved.Failure} {Messages(saved.Log)}");

            var lines = Written(file, saved.Yaml);
            if (saved.Log.Count > 0)
                Found($"saving {file}, the engine logged: {Messages(saved.Log)}");
            return lines;
        }

        private string[] Written(string file, string yaml)
        {
            using var timed = CheckTimes.In(CheckTimes.Text);
            File.WriteAllText(Path.Combine(outFolder, file), yaml);
            return Lines(yaml);
        }

        /// <summary>
        /// The file after a paste against the file before: every line of before must be there
        /// still, in order, and what is new must be whole entities under numbers not used before.
        /// With the clip of the same map, each new entity is also held against the one it copies.
        /// </summary>
        private void Compare(string[] before, string[] after, Clip? clip, string what)
        {
            using var timed = CheckTimes.In(CheckTimes.Compare);
            // A fork's decal chunk entities that the file did not number are numbered after
            // everything else, so a paste moves their numbers on. The decals are no part of it.
            if (ChunkDecalLines.Chunked)
                (before, after) = (ChunkDecalLines.Without(before), ChunkDecalLines.Without(after));
            var uids = after.Where(line => line.StartsWith("  - uid: ", StringComparison.Ordinal)).Select(line => line.Trim()).ToArray();
            Expect(uids.Length == uids.Distinct().Count(),
                $"no uid appears twice among the {uids.Length} in the file",
                $"{uids.Length - uids.Distinct().Count()} uids appear twice in the file");

            var (was, now) = (Blocks(before), Blocks(after));
            var added = now.Keys.Where(uid => !was.ContainsKey(uid)).ToHashSet();
            var stripped = Without(after, added);
            var differing = Differing(before, stripped);
            Expect(differing == 0,
                $"the originals are untouched: the file after {what} is the file before with {added.Count} entities added, {after.Length - before.Length} lines, and no other line differs but the entity count",
                $"the originals are not untouched: with the {added.Count} added entities taken out, {differing} lines differ from the file before, the first: {FirstDifference(before, stripped)}");

            if (clip == null)
                return;

            // A few entities of a map are saved as map-initialized. Their copies should be, and no others.
            var (came, expected) = (after.Count(Lifecycle) - before.Count(Lifecycle), clip.Entities.Keys.Sum(yaml => was[yaml].Count(Lifecycle)));
            Expect(came == expected,
                $"{came} lines saying paused or mapInit came with the paste, as many as the originals copied have",
                $"{came} lines saying paused or mapInit came with the paste, and the originals copied have {expected}");

            // Each added entity against an original, with every number in them blanked: the
            // copies are numbered anew and stand elsewhere. A copy whose original names things
            // outside the copy has fewer lines than it, and no other may differ.
            var originals = clip.Entities.Keys.Select(yaml => Blank(was[yaml])).GroupBy(text => text).ToDictionary(group => group.Key, group => group.Count());
            var unmatched = new List<string[]>();
            foreach (var uid in added)
            {
                var text = Blank(now[uid]);
                if (originals.TryGetValue(text, out var left) && left > 0)
                    originals[text] = left - 1;
                else
                    unmatched.Add(now[uid]);
            }

            var naming = clip.Entities.Values.Count(entity => entity.Outside.Values.Sum() > 0);
            Expect(unmatched.Count <= naming,
                $"{added.Count - unmatched.Count} of the {added.Count} added entities are written as an original is, numbers apart. The other {unmatched.Count} are among the copies of the {naming} that name things outside the copy",
                $"{unmatched.Count} of the {added.Count} added entities are not written as an original is, numbers apart, and only {naming} of the originals name things outside the copy");
            if (unmatched.Count > naming)
            {
                foreach (var block in unmatched.Take(3))
                    say($"        not as its original: {string.Join(" | ", block.Take(14).Select(line => line.Trim()))}");
            }
        }

        private static bool Lifecycle(string line) =>
            line.StartsWith("    paused:", StringComparison.Ordinal) || line.StartsWith("    mapInit:", StringComparison.Ordinal);
    }

    // ---- text ----

    private static string[] Lines(string yaml) => yaml.Split('\n');

    private static bool Invalid(string line) => line.Contains("invalid", StringComparison.Ordinal);

    private static string Messages(IReadOnlyList<KernelLogEntry> log) => log.Count == 0
        ? "nothing logged"
        : $"{(log.Count >= 500 ? "500 or more" : log.Count)} messages logged, the first: [{log[0].Level}] {(log[0].Message.Length > 260 ? log[0].Message[..260] + "..." : log[0].Message).Replace('\n', ' ').Replace("\r", "")}";

    /// <summary>The head of a file the serializer wrote, in one line.</summary>
    private static string Header(string text)
    {
        var parts = new List<string>();
        var (key, count) = ("", 0);
        void Close()
        {
            if (key is "meta" or "")
                return;
            parts.Add($"{key}: {count}");
        }

        foreach (var line in Lines(text))
        {
            if (line.StartsWith("entities:", StringComparison.Ordinal))
                break;

            if (line.Length > 0 && !char.IsWhiteSpace(line[0]) && line[0] != '-')
            {
                Close();
                (key, count) = (line.Split(':')[0], 0);
                if (line.TrimEnd().EndsWith("[]", StringComparison.Ordinal) || line.TrimEnd().EndsWith("{}", StringComparison.Ordinal))
                    count = 0;
            }
            else if (key == "meta")
            {
                parts.Add(line.Trim());
            }
            else
            {
                count++;
            }
        }

        Close();
        return string.Join(", ", parts);
    }

    /// <summary>The lines of each entity of a saved file, by its uid there.</summary>
    private static Dictionary<int, string[]> Blocks(string[] lines)
    {
        var blocks = new Dictionary<int, string[]>();
        var (uid, start) = (0, 0);
        void Close(int end)
        {
            if (uid != 0)
                blocks[uid] = lines[start..end];
            uid = 0;
        }

        for (var i = 0; i < lines.Length; i++)
        {
            var line = lines[i];
            if (line.StartsWith("  - uid: ", StringComparison.Ordinal))
            {
                Close(i);
                (uid, start) = (int.Parse(line.AsSpan(9)), i);
            }
            else if (!line.StartsWith("  ", StringComparison.Ordinal))
            {
                Close(i);
            }
        }

        Close(lines.Length);
        return blocks;
    }

    /// <summary>A saved file with these entities cut out, and the headings left with none.</summary>
    private static string[] Without(string[] lines, HashSet<int> gone)
    {
        var kept = new List<string>();
        var skipping = false;
        foreach (var line in lines)
        {
            if (line.StartsWith("  - uid: ", StringComparison.Ordinal))
                skipping = gone.Contains(int.Parse(line.AsSpan(9)));
            else if (!line.StartsWith("  ", StringComparison.Ordinal))
                skipping = false;

            if (!skipping)
                kept.Add(line);
        }

        var result = new List<string>();
        for (var i = 0; i < kept.Count; i++)
        {
            if (kept[i].StartsWith("- proto:", StringComparison.Ordinal) && i + 1 < kept.Count && kept[i + 1] == "  entities:"
                && (i + 2 >= kept.Count || !kept[i + 2].StartsWith("  - uid: ", StringComparison.Ordinal)))
            {
                i++;
                continue;
            }

            result.Add(kept[i]);
        }

        return result.ToArray();
    }

    /// <summary>The lines of the file's heading that say when it was saved and how many entities it has.</summary>
    private static bool Heading(string[] lines, int i) =>
        i < 12 && (lines[i].TrimStart().StartsWith("time:", StringComparison.Ordinal) || lines[i].TrimStart().StartsWith("entityCount:", StringComparison.Ordinal));

    private static int Differing(string[] a, string[] b)
    {
        var differing = Math.Abs(a.Length - b.Length);
        for (var i = 0; i < Math.Min(a.Length, b.Length); i++)
        {
            if (a[i] != b[i] && !(Heading(a, i) && Heading(b, i)))
                differing++;
        }

        return differing;
    }

    private static string FirstDifference(string[] a, string[] b)
    {
        for (var i = 0; i < Math.Min(a.Length, b.Length); i++)
        {
            if (a[i] != b[i] && !(Heading(a, i) && Heading(b, i)))
                return $"line {i + 1}: \"{a[i].Trim()}\" became \"{b[i].Trim()}\"";
        }

        return $"{a.Length} lines became {b.Length}";
    }

    private static bool SameButTime(string[] a, string[] b)
    {
        using var timed = CheckTimes.In(CheckTimes.Compare);
        if (a.Length != b.Length)
            return false;

        for (var i = 0; i < a.Length; i++)
        {
            if (a[i] != b[i] && !(i < 12 && a[i].TrimStart().StartsWith("time:", StringComparison.Ordinal) && b[i].TrimStart().StartsWith("time:", StringComparison.Ordinal)))
                return false;
        }

        return true;
    }

    /// <summary>Two texts of a copy, which say when each was written and may differ in that.</summary>
    private static bool SameText(string a, string b) => SameButTime(Lines(a), Lines(b));

    private static readonly Regex Number =new(@"-?\d+(\.\d+)?(E-?\d+)?", RegexOptions.Compiled);

    private static string Blank(string[] block) => Number.Replace(string.Join("\n", block), "#");
}
