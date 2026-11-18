using System.Text.Json;
using System.Text.Json.Nodes;
using SS14.EditorKernel;
using static SS14.EditorNative.CheckKit;

namespace SS14.EditorNative;

/// <summary>
/// Checks the kernel's device links: the answers to the "links" and "ports" questions, the
/// "link", "unlink", "list.add" and "list.remove" ops, and what happens to links when a linked
/// thing is deleted.
///
/// A link is made between two things that have the ports, saved, reopened and found intact, and
/// undo gives back the exact file the map was first saved as. The same goes for a device added to
/// an alarm's list. A sink, a listed device and a source are deleted in turn, and after each the
/// save may not name anything that isn't in it. Throughout, the check keeps its own mirror of the
/// links from the deltas alone, by the rule SceneDelta.Linked states, and compares it with the
/// kernel's answer after every step.
/// </summary>
public static class LinkCheck
{
    /// <returns>0 if every map passed.</returns>
    public static int Run(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        var session = EngineSession.Booted(forkRepo, kernelPack);
        var code = Run(session, forkRepo, maps, outFolder, say);
        CheckTimes.Report("links", say);
        CheckTimes.Summary(say);
        return code;
    }

    /// <summary>The check on an engine that is up already.</summary>
    public static int Run(EngineSession session, string forkRepo, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        Directory.CreateDirectory(outFolder);
        var tally = new Tally(say);
        say("ports");
        try
        {
            PortFacts(session, tally, say);
        }
        catch (Exception e)
        {
            tally.Expect(false, $"stopped: {e.Message}");
        }

        var code = OnEach(maps, say, "Links were made, kept and taken away, and undo gave back the first file each time.",
            (map, each) => Check(session, forkRepo, map, outFolder, say, each));
        return tally.Failed == 0 ? code : 1;
    }

    private sealed record Ported(int Uid, string[] Sources, string[] Sinks, float Range, bool HasDeviceList, bool IsNetworkDevice);

    private sealed record LinkSet(HashSet<SceneLink> Links, HashSet<ListedDevice> Lists);

    private static void PortFacts(EngineSession session, Tally tally, Action<string> say)
    {
        using var ports = Ask(session, "ports", "{}");
        var root = ports.RootElement;
        JsonElement Named(string list, string id) => root.GetProperty(list).EnumerateArray().FirstOrDefault(item => item.GetProperty("id").GetString() == id);
        string[] Strings(JsonElement item, string name) =>
            item.ValueKind == JsonValueKind.Object && item.GetProperty(name).ValueKind == JsonValueKind.Array
                ? item.GetProperty(name).EnumerateArray().Select(value => value.GetString()!).ToArray()
                : [];

        say($"  {root.GetProperty("sourcePorts").GetArrayLength()} source ports, {root.GetProperty("sinkPorts").GetArrayLength()} sink ports, {root.GetProperty("prototypes").GetArrayLength()} prototypes with a port");
        var pressed = Named("sourcePorts", "Pressed");
        tally.Expect(Strings(pressed, "defaults").SequenceEqual(["Timer", "Toggle", "Trigger"]), $"the source port Pressed links by default to Timer, Toggle and Trigger (said: {string.Join(", ", Strings(pressed, "defaults"))})");
        var toggle = Named("sinkPorts", "Toggle");
        tally.Expect(toggle.ValueKind == JsonValueKind.Object && toggle.GetProperty("defaults").ValueKind == JsonValueKind.Null && toggle.GetProperty("name").GetString()!.Length > 0,
            "the sink port Toggle is listed, with a name and no defaults");
        tally.Expect(Strings(Named("prototypes", "SignalButton"), "sources").Contains("Pressed"), "SignalButton has the source port Pressed");
        var airlock = Strings(Named("prototypes", "Airlock"), "sinks");
        tally.Expect(new[] { "Close", "Open", "Toggle" }.All(airlock.Contains), $"Airlock has the sink ports Open, Close and Toggle (its sinks: {string.Join(", ", airlock)})");
    }

    private static bool Check(EngineSession session, string forkRepo, string map, string outFolder, Action<string> say, Tally tally)
    {
        var name = Path.GetFileNameWithoutExtension(map);
        string Out(string step) => Path.Combine(outFolder, $"{name}.{step}.yml");

        var scene = Opened(session, CheckTimes.Open(map), File.ReadAllText(Path.Combine(forkRepo, "Resources", map)), map);
        var document = scene.Document.Id;
        try
        {
            var before = Save(session, document, Out("before"));
            var byUid = scene.Entities.ToDictionary(entity => entity.Uid);
            var grids = scene.Grids.Select(grid => grid.Uid).ToHashSet();
            var firstLog = scene.Document.Log.Select(entry => entry.Message).ToHashSet();
            var yamlOf = scene.Entities.ToDictionary(entity => entity.Uid, entity => entity.YamlUid);

            var first = Links(session, document);
            say($"  {first.Links.Count} links from {first.Links.Select(link => link.Source).Distinct().Count()} sources, {first.Lists.Count} devices on {first.Lists.Select(entry => entry.List).Distinct().Count()} lists");
            tally.Expect(first.Links.All(link => byUid.ContainsKey(link.Source) && byUid.ContainsKey(link.Sink)) && first.Lists.All(entry => byUid.ContainsKey(entry.List) && byUid.ContainsKey(entry.Device)),
                "every link and every listed device is between entities of the scene");
            AgainstDetails(session, document, scene, byUid, first, tally);

            // The mirror an interface would keep: the first answer, then the deltas alone.
            var mirror = new LinkSet([.. first.Links], [.. first.Lists]);
            SceneDelta Follow(SceneDelta delta)
            {
                foreach (var gone in delta.Removed)
                {
                    mirror.Links.RemoveWhere(link => link.Source == gone);
                    mirror.Lists.RemoveWhere(entry => entry.List == gone);
                }

                foreach (var entity in delta.Linked)
                {
                    mirror.Links.RemoveWhere(link => link.Source == entity.Uid);
                    mirror.Lists.RemoveWhere(entry => entry.List == entity.Uid);
                    mirror.Links.UnionWith(entity.Links);
                    mirror.Lists.UnionWith(entity.Devices.Select(device => new ListedDevice(entity.Uid, device)));
                }

                return delta;
            }

            SceneDelta Do(string label, params EditOp[] ops) => Follow(Edit(session, document, label, ops));
            SceneDelta Back() => Follow(Undo(session, document));
            SceneDelta Forth() => Follow(Redo(session, document));
            void Mirrored(string after)
            {
                var now = Links(session, document);
                tally.Expect(now.Links.SetEquals(mirror.Links) && now.Lists.SetEquals(mirror.Lists), $"the mirror kept from the deltas is the kernel's answer {after} ({now.Links.Count} links, {now.Lists.Count} listed)");
            }

            var onGrid = scene.Entities.Where(entity => grids.Contains(entity.Parent)).ToArray();
            var ported = PortsOf(session, document, onGrid.Select(entity => entity.Uid)).ToDictionary(entity => entity.Uid);
            float Apart(int a, int b) => MathF.Sqrt(MathF.Pow(byUid[a].X - byUid[b].X, 2) + MathF.Pow(byUid[a].Y - byUid[b].Y, 2));

            // ---- a new link ----
            var pair = (from source in ported.Values
                        where source.Sources.Length > 0
                        from sink in ported.Values
                        where sink.Sinks.Length > 0 && sink.Uid != source.Uid && byUid[sink.Uid].Parent == byUid[source.Uid].Parent
                        where Apart(source.Uid, sink.Uid) < source.Range - 1
                        where !first.Links.Contains(new SceneLink(source.Uid, source.Sources[0], sink.Uid, sink.Sinks[0]))
                        orderby first.Links.Count(link => link.Source == source.Uid) descending, Apart(source.Uid, sink.Uid)
                        select new SceneLink(source.Uid, source.Sources[0], sink.Uid, sink.Sinks[0])).FirstOrDefault();
            if (pair == null)
            {
                CheckSkips.Skip(say, "a link made, saved and undone: the map has no source and sink in reach of each other that are not linked");
            }
            else
            {
                var link = new EditOp("link", Uid: pair.Source, Port: pair.SourcePort, Target: pair.Sink, TargetPort: pair.SinkPort);
                var unlink = link with { Kind = "unlink" };
                var delta = Do("Link", link);
                tally.Expect(delta.Linked.Any(entity => entity.Uid == pair.Source && entity.Links.Contains(pair)) && delta.Details.Any(details => details.Uid == pair.Source),
                    $"a {byUid[pair.Source].Prototype} linked {pair.SourcePort} to {pair.SinkPort} of a {byUid[pair.Sink].Prototype}: the delta carries the source's links with the new one, and its data");
                Mirrored("after the link");

                var linked = Save(session, document, Out("linked"));
                tally.Expect(Differences(before, linked).Count > 0 && Invalid(linked) == Invalid(before), "the save differs from the first and names nothing that is not in it");
                Reopen(session, tally, say, Out("linked"), linked, firstLog, scene.Entities.Count, yamlOf, Links(session, document));

                var again = Do("Link", link);
                tally.Expect(again.Linked.Count == 0 && again.History.Revision == delta.History.Revision, "the same link asked for again changes nothing and is no undo step");

                var undone = Back();
                tally.Expect(undone.Linked.Any(entity => entity.Uid == pair.Source && !entity.Links.Contains(pair)), "undo reports the source's links without it");
                tally.Same(before, Save(session, document, Out("linked-undone")), "undone, the save is the first file");
                Forth();
                tally.Same(linked, Save(session, document, Out("linked-redone")), "redone, the save is the file with the link");
                Do("Unlink", unlink);
                tally.Same(before, Save(session, document, Out("linked-unlinked")), "unlinked by the op, the save is the first file");
                Mirrored("after the unlink");
                Back();
                Back();

                // What the game refuses.
                var deaf = onGrid.FirstOrDefault(entity => ported[entity.Uid].Sinks.Length == 0 && ported[entity.Uid].Sources.Length == 0);
                var refusals = new List<(string What, string? Said)>
                {
                    ("a source port the entity does not have", Refusal(session, document, link with { Port = "NoSuchPort" })),
                    ("a sink port the entity does not have", Refusal(session, document, link with { TargetPort = "NoSuchPort" })),
                };
                if (deaf != null)
                {
                    refusals.Add(("a link to a thing that takes no signals", Refusal(session, document, link with { Target = deaf.Uid })));
                    refusals.Add(("a link from a thing that sends none", Refusal(session, document, link with { Uid = deaf.Uid })));
                }

                // Out of reach, by the game's own rule: the source is given a reach shorter than the way.
                if (Apart(pair.Source, pair.Sink) > 0.6f)
                {
                    Do("Reach", new EditOp("components", Uid: pair.Source, Components: WithRange(session, document, pair.Source, 0.5f)));
                    refusals.Add(("a link farther than the source reaches", Refusal(session, document, link)));
                    Back();
                }
                else
                {
                    CheckSkips.Skip(say, "a link farther than the source reaches refused: the two chosen stand in one place");
                }

                foreach (var (what, said) in refusals)
                    tally.Expect(said != null, $"refused: {what} ({said})");
                Mirrored("after the refusals");
            }

            // ---- a link the map came with ----
            var old = first.Links.OrderByDescending(link => first.Links.Count(other => other.Source == link.Source)).FirstOrDefault();
            if (old == null)
            {
                CheckSkips.Skip(say, "a link of the map taken away and put back by undo: the map has no links");
            }
            else
            {
                Do("Unlink", new EditOp("unlink", Uid: old.Source, Port: old.SourcePort, Target: old.Sink, TargetPort: old.SinkPort));
                var unlinked = Save(session, document, Out("unlinked"));
                tally.Expect(Differences(before, unlinked).Count > 0 && Invalid(unlinked) == Invalid(before), $"a link of the map's taken away ({byUid[old.Source].Prototype} {old.SourcePort} to {byUid[old.Sink].Prototype} {old.SinkPort}): the save differs and names nothing that is not in it");
                Mirrored("after it");
                Reopen(session, tally, say, Out("unlinked"), unlinked, firstLog, scene.Entities.Count, yamlOf, Links(session, document));
                Back();
                tally.Same(before, Save(session, document, Out("unlinked-undone")), "undone, the save is the first file");
            }

            // ---- device lists ----
            var keeper = ported.Values.Where(entity => entity.HasDeviceList).OrderByDescending(entity => first.Lists.Count(entry => entry.List == entity.Uid)).FirstOrDefault();
            var joiner = keeper == null ? null : ported.Values
                .Where(entity => entity.IsNetworkDevice && entity.Uid != keeper.Uid && !first.Lists.Contains(new ListedDevice(keeper.Uid, entity.Uid)))
                .OrderBy(entity => first.Lists.Count(entry => entry.Device == entity.Uid))
                .FirstOrDefault();
            if (keeper == null || joiner == null)
            {
                CheckSkips.Skip(say, "a device put on a device list, saved and undone: the map has nothing that keeps a list, or no device to add");
            }
            else
            {
                var entry = new ListedDevice(keeper.Uid, joiner.Uid);
                var add = new EditOp("list.add", Uid: keeper.Uid, Target: joiner.Uid);
                var delta = Do("List", add);
                tally.Expect(delta.Linked.Any(entity => entity.Uid == keeper.Uid && entity.Devices.Contains(joiner.Uid)) && delta.Details.Any(details => details.Uid == keeper.Uid) && delta.Details.Any(details => details.Uid == joiner.Uid),
                    $"a {byUid[joiner.Uid].Prototype} put on the list of a {byUid[keeper.Uid].Prototype}: the delta carries the list with it, and the data of both");
                Mirrored("after it");
                var listed = Save(session, document, Out("listed"));
                tally.Expect(Differences(before, listed).Count > 0 && Invalid(listed) == Invalid(before), "the save differs from the first and names nothing that is not in it");
                Reopen(session, tally, say, Out("listed"), listed, firstLog, scene.Entities.Count, yamlOf, Links(session, document));

                var again = Do("List", add);
                tally.Expect(again.Linked.Count == 0 && again.History.Revision == delta.History.Revision, "the same device listed again changes nothing and is no undo step");
                Back();
                tally.Same(before, Save(session, document, Out("listed-undone")), "undone, the save is the first file");
                Forth();
                tally.Same(listed, Save(session, document, Out("listed-redone")), "redone, the save is the file with the device listed");
                Do("Unlist", add with { Kind = "list.remove" });
                tally.Same(before, Save(session, document, Out("listed-unlisted")), "taken off by the op, the save is the first file");
                Back();
                Back();

                var loose = onGrid.FirstOrDefault(entity => !ported[entity.Uid].IsNetworkDevice && !ported[entity.Uid].HasDeviceList);
                if (loose != null)
                {
                    var notDevice = Refusal(session, document, add with { Target = loose.Uid });
                    var notList = Refusal(session, document, add with { Uid = loose.Uid });
                    tally.Expect(notDevice != null && notList != null, $"refused: a thing that is no network device on a list, and a list on a thing that keeps none ({notDevice} {notList})");
                }

                var member = first.Lists.FirstOrDefault(listedDevice => listedDevice.List == keeper.Uid);
                if (member == null)
                {
                    CheckSkips.Skip(say, "a device of the map taken off its list and put back by undo: the list is empty");
                }
                else
                {
                    Do("Unlist", new EditOp("list.remove", Uid: member.List, Target: member.Device));
                    var unlisted = Save(session, document, Out("unlisted"));
                    tally.Expect(Differences(before, unlisted).Count > 0 && Invalid(unlisted) == Invalid(before), "a device of the map taken off its list: the save differs and names nothing that is not in it");
                    Mirrored("after it");
                    Reopen(session, tally, say, Out("unlisted"), unlisted, firstLog, scene.Entities.Count, yamlOf, Links(session, document));
                    Back();
                    tally.Same(before, Save(session, document, Out("unlisted-undone")), "undone, the save is the first file");
                }
            }

            // ---- deleting what is linked ----
            var targets = new List<(string What, int Uid)>();
            if (first.Links.Count > 0)
            {
                var busiest = first.Links.GroupBy(link => link.Sink).OrderByDescending(group => group.Count()).First().Key;
                targets.Add(("a sink that sources are linked to", busiest));
                targets.Add(("a source with links", first.Links.GroupBy(link => link.Source).OrderByDescending(group => group.Count()).First().Key));
            }
            else
            {
                CheckSkips.Skip(say, "a linked sink and a source deleted: the map has no links");
            }

            if (first.Lists.Count > 0)
                targets.Add(("a device on a device list", first.Lists.GroupBy(entry => entry.Device).OrderByDescending(group => group.Count()).First().Key));
            else
                CheckSkips.Skip(say, "a listed device deleted: the map has no device lists with a device");

            foreach (var (what, uid) in targets)
            {
                var step = what.Split(' ')[1];
                var delta = Do("Delete", new EditOp("delete", Uid: uid));
                var now = Links(session, document);
                var named = now.Links.Any(link => link.Source == uid || link.Sink == uid) || now.Lists.Any(entry => entry.List == uid || entry.Device == uid);
                tally.Expect(delta.Removed.Contains(uid) && !named, $"{what} deleted ({byUid[uid].Prototype}): nothing links to it or lists it any more");
                Mirrored("after it");
                var deleted = Save(session, document, Out($"deleted-{step}"));
                tally.Expect(Invalid(deleted) == Invalid(before), $"the save has no \"invalid\" the first file had not ({Invalid(deleted)} lines with the word, {Invalid(before)} before)");
                Reopen(session, tally, say, Out($"deleted-{step}"), deleted, firstLog, scene.Entities.Count - delta.Removed.Count, yamlOf, now);
                Back();
                Mirrored("after undo");
                tally.Same(before, Save(session, document, Out($"deleted-{step}-undone")), "undone, the save is the first file");
            }

            tally.Same(before, Save(session, document, Out("after")), "after all of it the save is the first file");
            return tally.Failed == 0;
        }
        finally
        {
            session.Close(document);
        }
    }

    /// <summary>
    /// The "links" answer held against the component data the "details" question gives: for
    /// each source the sinks it names and how many links to each, and for each list its devices.
    /// </summary>
    private static void AgainstDetails(EngineSession session, int document, Scene scene, Dictionary<int, SceneEntity> byUid, LinkSet first, Tally tally)
    {
        var (sources, lists, wrong) = (0, 0, 0);
        using var details = Ask(session, "details", $"{{\"doc\":{document},\"uids\":[{string.Join(",", scene.Entities.Select(entity => entity.Uid))}]}}");
        foreach (var entity in details.RootElement.EnumerateArray())
        {
            var uid = entity.GetProperty("uid").GetInt32();
            var links = new List<(int Sink, int Count)>();
            var devices = new List<int>();
            using var components = JsonDocument.Parse(entity.GetProperty("components").GetString() ?? "[]");
            foreach (var component in components.RootElement.EnumerateArray())
            {
                var type = component.GetProperty("type").GetString();
                if (type == "DeviceLinkSource" && component.TryGetProperty("linkedPorts", out var linked) && linked.ValueKind == JsonValueKind.Object)
                    links.AddRange(linked.EnumerateObject().Select(sink => (int.Parse(sink.Name), sink.Value.GetArrayLength())));
                if (type == "DeviceList" && component.TryGetProperty("devices", out var listed) && listed.ValueKind == JsonValueKind.Array)
                    devices.AddRange(listed.EnumerateArray().Select(device => device.ValueKind == JsonValueKind.Number ? device.GetInt32() : int.Parse(device.GetString()!)));
            }

            var said = first.Links.Where(link => link.Source == uid).GroupBy(link => link.Sink).Select(group => (group.Key, group.Count())).ToHashSet();
            if (!said.SetEquals(links))
                wrong++;
            if (!first.Lists.Where(entry => entry.List == uid).Select(entry => entry.Device).ToHashSet().SetEquals(devices))
                wrong++;
            sources += links.Count > 0 ? 1 : 0;
            lists += devices.Count > 0 ? 1 : 0;
        }

        tally.Expect(wrong == 0, $"the links and lists agree with the component data of the {sources} sources and {lists} lists that have any");
    }

    /// <summary>
    /// Opens a save again: nothing may be logged that the map did not log when first opened,
    /// the links and lists must be those the document had, entity for entity by the numbers
    /// of the file, and saved again it must be the same file.
    /// </summary>
    private static void Reopen(EngineSession session, Tally tally, Action<string> say, string path, string[] saved, HashSet<string> firstLog, int entities, Dictionary<int, int> yamlOf, LinkSet had)
    {
        var reopened = Opened(session, CheckTimes.Reopen, File.ReadAllText(path), Path.GetFileName(path));
        try
        {
            var logged = reopened.Document.Log.Where(entry => !firstLog.Contains(entry.Message)).ToArray();
            if (!tally.Expect(logged.Length == 0 && reopened.Entities.Count == entities, $"opened again it has {reopened.Entities.Count} entities (expected {entities}) and logs {logged.Length} new messages"))
            {
                foreach (var entry in logged.Take(6))
                    say($"        {entry.Level} {entry.Sawmill}: {entry.Message}");
            }

            var yamlNow = reopened.Entities.ToDictionary(entity => entity.Uid, entity => entity.YamlUid);
            var now = Links(session, reopened.Document.Id);
            var linksHad = had.Links.Select(link => (yamlOf[link.Source], link.SourcePort, yamlOf[link.Sink], link.SinkPort)).ToHashSet();
            var linksNow = now.Links.Select(link => (yamlNow[link.Source], link.SourcePort, yamlNow[link.Sink], link.SinkPort)).ToHashSet();
            var listsHad = had.Lists.Select(entry => (yamlOf[entry.List], yamlOf[entry.Device])).ToHashSet();
            var listsNow = now.Lists.Select(entry => (yamlNow[entry.List], yamlNow[entry.Device])).ToHashSet();
            tally.Expect(linksHad.SetEquals(linksNow) && listsHad.SetEquals(listsNow), $"its {now.Links.Count} links and {now.Lists.Count} listed devices are those the document had");
            tally.Same(saved, Save(session, reopened.Document.Id, Path.ChangeExtension(path, ".again.yml")), "saved again it is the same file");
        }
        finally
        {
            session.Close(reopened.Document.Id);
        }
    }

    private static LinkSet Links(EngineSession session, int document)
    {
        using var answer = Ask(session, "links", $"{{\"doc\":{document}}}");
        var scene = answer.RootElement.Deserialize<LinkScene>(KernelApi.Json)!;
        return new LinkSet([.. scene.Links], [.. scene.Lists]);
    }

    private static List<Ported> PortsOf(EngineSession session, int document, IEnumerable<int> uids)
    {
        using var answer = Ask(session, "ports.of", $"{{\"doc\":{document},\"uids\":[{string.Join(",", uids)}]}}");
        return answer.RootElement.EnumerateArray().Select(entity =>
        {
            var ports = entity.Deserialize<EntityPorts>(KernelApi.Json)!;
            return new Ported(ports.Uid, [.. ports.Sources], [.. ports.Sinks], ports.Range, ports.HasDeviceList, ports.IsNetworkDevice);
        }).ToList();
    }

    /// <summary>An entity's component data, as "details" gives it, with its reach as a source set.</summary>
    private static string WithRange(EngineSession session, int document, int uid, float range)
    {
        using var details = Ask(session, "details", $"{{\"doc\":{document},\"uids\":[{uid}]}}");
        var components = JsonNode.Parse(details.RootElement[0].GetProperty("components").GetString() ?? "[]")!.AsArray();
        var source = components.OfType<JsonObject>().FirstOrDefault(component => component["type"]?.GetValue<string>() == "DeviceLinkSource");
        if (source == null)
        {
            source = new JsonObject { ["type"] = "DeviceLinkSource" };
            components.Add(source);
        }

        source["range"] = range;
        return components.ToJsonString();
    }
}
