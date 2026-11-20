using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Robust.Shared.Serialization.Markdown;
using Robust.Shared.Serialization.Markdown.Mapping;
using SS14.EditorKernel;
using static SS14.EditorNative.CheckKit;

namespace SS14.EditorNative;

/// <summary>
/// Checks that the view shows each entity as the map saves it (SavedState.cs, VisualsDevices.cs,
/// ViewWorld.Saved.cs).
///
/// On each map given, every field of saved data the kernel sends must be what the view's
/// component reads as before the entity starts, and every appearance value the kernel works out
/// as the game's server would must be in the view's appearance data. Then closed and open
/// valves, doors saved welded or open, and filters on and off must be drawn so, by their sprite
/// states. Last, a valve is closed through its component data, which must make the view draw it
/// closed, and undo opens it.
/// </summary>
public static partial class VisualsCheck
{
    /// <returns>0 if every map passed.</returns>
    public static int RunSaved(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, Action<string> say)
    {
        var session = EngineSession.Booted(forkRepo, kernelPack);
        var code = RunSaved(session, forkRepo, maps, say);
        CheckTimes.Report("saved", say);
        CheckTimes.Summary(say);
        return code;
    }

    /// <summary>The check on an engine that is up already.</summary>
    public static int RunSaved(EngineSession session, string forkRepo, IReadOnlyList<string> maps, Action<string> say) =>
        OnEach(maps, say, "Every entity was shown as the map saves it.", (map, tally) =>
        {
            var yaml = File.ReadAllText(Path.Combine(forkRepo, "Resources", map));
            session.OnViewKernel(view =>
            {
                var world = view.World;
                world.CheckReads = true;
                world.ReadFields = 0;
                world.ReadWrong.Clear();
                world.ServerFields.Clear();
                world.FailedSaved.Clear();
                return true;
            });
            var scene = Opened(session, CheckTimes.Open(map), yaml, map);
            var doc = scene.Document.Id;
            try
            {
                DataReached(session, scene, say, tally);
                AppearanceReached(session, scene, say, tally);
                // The list of the open has everything. The one after showing what is under the
                // floor has what that changed.
                var list = DrawList.Read(session, doc, null);
                session.ShowSubFloor(doc, true);
                var under = DrawList.Read(session, doc, list.Textures);
                foreach (var (uid, item) in under.Items)
                    list.Items[uid] = item;
                StatesDrawn(session, scene, list, say, tally);
                DecalsDrawn(session, scene, yaml, list, say, tally);
                FramesLinked(list, say, tally);
                ValveClosed(session, scene, list, say, tally);
            }
            finally
            {
                session.ShowSubFloor(doc, false);
                session.Close(doc);
                session.OnViewKernel(view => view.World.CheckReads = false);
            }

            return true;
        });

    /// <summary>
    /// Every saved field the kernel sent is what the view's component reads as, before the
    /// entity starts. What the client's own systems then change as it starts (a door's timer,
    /// a link to a device they find wanting) is told, as the game's client would do the same.
    /// </summary>
    private static void DataReached(EngineSession session, Scene scene, Action<string> say, Tally tally)
    {
        var doc = scene.Document.Id;
        var (entities, components, serverOnly, changed) = session.OnViewKernel(view =>
        {
            var world = view.World;
            var compared = (Entities: 0, Components: 0, ServerOnly: 0);
            var changed = new Dictionary<string, int>();
            foreach (var saved in scene.Saved)
            {
                compared.Entities++;
                foreach (var component in saved.Components)
                {
                    var (known, data) = world.SavedOf(doc, saved.Uid, component.Name);
                    if (!known)
                    {
                        compared.ServerOnly++;
                        continue;
                    }

                    compared.Components++;
                    foreach (var (key, value) in component.Data)
                    {
                        if (data != null && data.TryGet(key, out var held) && !held.Equals(value))
                            changed[$"{component.Name}.{key}"] = changed.GetValueOrDefault($"{component.Name}.{key}") + 1;
                    }
                }
            }

            return (compared.Entities, compared.Components, compared.ServerOnly, changed);
        });

        var (fields, wrong, serverFields, failed) = session.OnViewKernel(view =>
            (view.World.ReadFields, view.World.ReadWrong.ToDictionary(), view.World.ServerFields.ToList(), view.World.FailedSaved.ToDictionary()));
        say($"        saved data of {entities} entities sent: {components + serverOnly} components, {serverOnly} of them only the server has, {fields} fields compared as read");
        if (serverFields.Count > 0)
            say($"        fields only the server's kind of the component has: {string.Join(", ", serverFields.Order())}");
        foreach (var (name, reason) in failed)
            say($"        {name} could not be read: {reason}");
        tally.Expect(wrong.Count == 0 && failed.Count == 0,
            $"every saved field reached the view's components: {wrong.Values.Sum(pair => pair.Count)} read otherwise{(wrong.Count > 0 ? ": " + string.Join("; ", wrong.OrderByDescending(pair => pair.Value.Count).Take(6).Select(pair => $"{pair.Key} x{pair.Value.Count} ({pair.Value.Example})")) : "")}");
        if (changed.Count > 0)
            say($"        changed by the client's systems as the entities started: {string.Join(", ", changed.OrderByDescending(pair => pair.Value).Select(pair => $"{pair.Key} x{pair.Value}"))}");
    }

    /// <summary>Every appearance value the kernel worked out is in the view's appearance data.</summary>
    private static void AppearanceReached(EngineSession session, Scene scene, Action<string> say, Tally tally)
    {
        var doc = scene.Document.Id;
        var sent = scene.Visuals.Where(look => look.Appearance != null).ToList();
        if (sent.Count == 0)
        {
            CheckSkips.Skip(say, "appearance values: the map has no device the server dresses");
            return;
        }

        var (values, wrong, keys) = session.OnViewKernel(view =>
        {
            var world = view.World;
            var values = 0;
            var keys = 0;
            var wrong = new List<string>();
            foreach (var look in sent)
            {
                var held = world.AppearanceOf(doc, look.Uid);
                keys += held?.Count ?? 0;
                foreach (var value in look.Appearance!)
                {
                    values++;
                    if (world.Decode(value) is not var (key, data))
                    {
                        wrong.Add($"{look.Uid}: {value.KeyType}.{value.Key} unknown to the client");
                        continue;
                    }

                    if (held == null || !held.TryGetValue(key, out var there) || !there.Equals(data))
                        wrong.Add($"{look.Uid}: {value.Key} sent {value.Value}, view {(held != null && held.TryGetValue(key, out var t) ? t : "nothing")}");
                }
            }

            return (values, wrong, keys);
        });

        tally.Expect(wrong.Count == 0,
            $"every one of {values} appearance values sent of {sent.Count} entities is in the view's appearance data, which holds {keys} values for them{(wrong.Count > 0 ? ": " + string.Join("; ", wrong.Take(4)) : "")}");
        lock (VisualsReader.UnknownKinds)
        {
            if (VisualsReader.UnknownKinds.Count > 0)
                say($"        value kinds the kernel could not write: {string.Join(", ", VisualsReader.UnknownKinds.Select(pair => $"{pair.Key} x{pair.Value}"))}");
        }
    }

    /// <summary>The sprite states an entity is drawn with.</summary>
    private static List<string> StatesOf(DrawList list, int uid) =>
        list.Items.TryGetValue(uid, out var item)
            ? item.Quads.Select(quad => list.Textures.GetValueOrDefault(quad.Texture)?.State).OfType<string>().ToList()
            : [];

    /// <summary>A saved field of an entity, as the kernel sent it.</summary>
    private static string? SavedField(Scene scene, int uid, string component, string field) =>
        scene.Saved.FirstOrDefault(saved => saved.Uid == uid)?.Components.FirstOrDefault(c => c.Name == component)?.Data is { } data
        && data.TryGet(field, out var node) ? node.ToString() : null;

    /// <summary>Valves, doors and filters drawn as their saved data says.</summary>
    private static void StatesDrawn(EngineSession session, Scene scene, DrawList list, Action<string> say, Tally tally)
    {
        void Case(string what, Func<SceneEntity, bool> pick, Func<List<string>, bool> drawn)
        {
            var found = scene.Entities.Where(entity => !entity.Contained && pick(entity) && list.Items.ContainsKey(entity.Uid)).ToList();
            if (found.Count == 0)
            {
                CheckSkips.Skip(say, $"{what}: none on the map");
                return;
            }

            var wrong = found.Where(entity => !drawn(StatesOf(list, entity.Uid))).ToList();
            string Appearance(SceneEntity entity) => session.OnViewKernel(view =>
                string.Join(", ", view.World.AppearanceOf(scene.Document.Id, entity.Uid)?.Select(pair => $"{pair.Key}={pair.Value}") ?? []));
            tally.Expect(wrong.Count == 0,
                $"each {what} is drawn so, of {found.Count}{(wrong.Count > 0 ? ": " + string.Join("; ", wrong.Take(3).Select(entity => $"{entity.Prototype} {entity.Uid}: {string.Join(", ", StatesOf(list, entity.Uid))} (appearance {Appearance(entity)})")) : "")}");
        }

        Case("closed valve", entity => SavedField(scene, entity.Uid, "GasValve", "open") == "False",
            states => states.Contains("pumpManualValve") && !states.Contains("pumpManualValveOn"));
        Case("open valve", entity => entity.Prototype == "GasValve" && SavedField(scene, entity.Uid, "GasValve", "open") == null,
            states => states.Contains("pumpManualValveOn"));
        // The game draws a door saved welded as a closed one. Its weld shows only once someone
        // welds it, from the appearance the weld sets (Content.Shared/Tools/Systems/WeldableSystem.cs:116-120),
        // and appearance data is not saved.
        Case("door saved welded", entity => SavedField(scene, entity.Uid, "Door", "state") == "Welded",
            states => states.Contains("closed") && !states.Contains("open"));
        Case("door saved open", entity => SavedField(scene, entity.Uid, "Door", "state") == "Open",
            states => states.Contains("open") && !states.Contains("closed"));
        // A light fixture as mapping mode shows it: no bulb, since the server puts one in only
        // as the map starts (Content.Server/Light/EntitySystems/PoweredLightSystem.cs OnMapInit),
        // and no bulb state, which only UpdateLight sets and nothing calls as a map loads
        // (Content.Shared/Light/EntitySystems/SharedPoweredLightSystem.cs OnInit). So the client's
        // PoweredLightVisualizerSystem leaves the prototype's layers as they are: the tube and its glow.
        Case("light fixture as mapping mode shows it", entity => entity.Prototype == "Poweredlight",
            states => states.Contains("base") && states.Contains("glow"));
        bool? FilterOn(SceneEntity entity) =>
            scene.Visuals.FirstOrDefault(look => look.Uid == entity.Uid)?.Appearance?.FirstOrDefault(value => value.KeyType.EndsWith(".FilterVisuals", StringComparison.Ordinal))?.Value is { } on ? on == "true" : null;
        Case("filter that is on", entity => entity.Prototype.StartsWith("GasFilter", StringComparison.Ordinal) && FilterOn(entity) == true,
            states => states.Any(state => state.StartsWith("gasFilter", StringComparison.Ordinal) && state.EndsWith("On", StringComparison.Ordinal)));
        Case("filter that is off", entity => entity.Prototype.StartsWith("GasFilter", StringComparison.Ordinal) && FilterOn(entity) == false,
            states => states.Any(state => state.StartsWith("gasFilter", StringComparison.Ordinal))
                      && !states.Any(state => state.StartsWith("gasFilter", StringComparison.Ordinal) && state.EndsWith("On", StringComparison.Ordinal)));
    }

    /// <summary>
    /// Every decal of the map is sent and drawn: those a grid keeps on chunk entities, as Space
    /// Wizards' maps do, are counted in the file, and each sent decal of a prototype the game has
    /// is drawn with one rectangle.
    /// </summary>
    private static void DecalsDrawn(EngineSession session, Scene scene, string yaml, DrawList list, Action<string> say, Tally tally)
    {
        var sent = scene.Grids.Sum(grid => grid.Decals.Count);
        // In the file a chunk's decals are listed by id under each kind, as "            12: 30,-34".
        var onChunks = 0;
        var inChunk = false;
        foreach (var line in yaml.Split('\n'))
        {
            if (line.StartsWith("    - type: ", StringComparison.Ordinal) || !line.StartsWith("      ", StringComparison.Ordinal))
                inChunk = line.TrimEnd() == "    - type: DecalChunk";
            else if (inChunk && Regex.IsMatch(line, @"^            \d+: "))
                onChunks++;
        }

        // Such a fork keeps all its decals there, those of the old way moved there as the map loads.
        if (onChunks > 0)
            tally.Expect(sent == onChunks, $"every one of {onChunks} decals the file keeps on chunk entities is sent: {sent}");

        if (sent == 0)
        {
            CheckSkips.Skip(say, "decals drawn: the map has none");
            return;
        }

        using var known = Ask(session, "decals", "{}");
        var prototypes = known.RootElement.EnumerateArray().Select(decal => decal.GetProperty("id").GetString()).ToHashSet();
        var drawable = scene.Grids.Sum(grid => grid.Decals.Count(decal => prototypes.Contains(decal.Prototype)));
        var drawn = list.Items.Values.Where(item => item.Kind == 2).Sum(item => item.Quads.Count);
        tally.Expect(drawn == drawable, $"each of {drawable} decals of a prototype the game has is drawn, of {sent} sent: {drawn} drawn");
    }

    /// <summary>
    /// Each frame of an animated state names the next one and how long it shows, so that the
    /// editor can move the sprite through them: following the links from any frame comes back
    /// to it through frames of the same state alone, each showing for some time.
    /// </summary>
    private static void FramesLinked(DrawList list, Action<string> say, Tally tally)
    {
        var linked = list.Textures.Where(pair => pair.Value.Next != null).ToList();
        if (linked.Count == 0)
        {
            CheckSkips.Skip(say, "animated frames: none drawn");
            return;
        }

        var broken = new List<string>();
        var states = new HashSet<(string, string?)>();
        foreach (var (id, source) in linked)
        {
            states.Add((source.Path, source.State));
            var at = id;
            var steps = 0;
            do
            {
                var here = list.Textures.GetValueOrDefault(at);
                if (here?.Next is not { } next || here.Path != source.Path || here.State != source.State || here.Delay <= 0 || ++steps > 1000)
                {
                    broken.Add($"{source.Path}/{source.State} frame {id}");
                    break;
                }

                at = next;
            } while (at != id);
        }

        tally.Expect(broken.Count == 0,
            $"each of {linked.Count} frames of {states.Count} animated states leads round its state and back{(broken.Count > 0 ? ": " + string.Join("; ", broken.Take(4)) : "")}");
    }

    /// <summary>A valve closed through its component data is drawn closed, and undo opens it again.</summary>
    private static void ValveClosed(EngineSession session, Scene scene, DrawList list, Action<string> say, Tally tally)
    {
        var doc = scene.Document.Id;
        var valve = scene.Entities.FirstOrDefault(entity => entity.Prototype == "GasValve" && !entity.Contained
                                                            && SavedField(scene, entity.Uid, "GasValve", "open") == null
                                                            && StatesOf(list, entity.Uid).Contains("pumpManualValveOn"));
        if (valve == null)
        {
            CheckSkips.Skip(say, "a valve closed by its data: no open valve drawn");
            return;
        }

        // The entity's components as they are, with the valve closed.
        using var described = Ask(session, "details", JsonSerializer.Serialize(new { doc, uids = new[] { valve.Uid } }));
        var components = JsonNode.Parse(described.RootElement[0].GetProperty("components").GetString()!)!.AsArray();
        if (components.FirstOrDefault(component => component!["type"]!.GetValue<string>() == "GasValve") is JsonObject own)
            own["open"] = false;
        else
            components.Add(new JsonObject { ["type"] = "GasValve", ["open"] = false });

        var closed = Edit(session, doc, "Close valve", new EditOp("components", Uid: valve.Uid, Components: components.ToJsonString()));
        list = DrawList.Read(session, doc, list.Textures);
        var states = StatesOf(list, valve.Uid);
        tally.Expect(closed.Saved.Any(saved => saved.Uid == valve.Uid) && states.Contains("pumpManualValve") && !states.Contains("pumpManualValveOn"),
            $"valve {valve.Uid} closed by its data is drawn closed: {string.Join(", ", states)}");

        var undone = Undo(session, doc);
        list = DrawList.Read(session, doc, list.Textures);
        states = StatesOf(list, valve.Uid);
        tally.Expect(undone.Saved.Any(saved => saved.Uid == valve.Uid && saved.Components.All(c => c.Name != "GasValve")) && states.Contains("pumpManualValveOn"),
            $"undo draws it open again: {string.Join(", ", states)}");
    }
}
