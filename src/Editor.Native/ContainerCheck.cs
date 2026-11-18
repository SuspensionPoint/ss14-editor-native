using System.Text.Json;
using System.Text.Json.Nodes;
using SS14.EditorKernel;
using static SS14.EditorNative.CheckKit;

namespace SS14.EditorNative;

/// <summary>
/// Checks the kernel's "contain" and "uncontain": a new thing put into a locker and into a
/// one-item slot, and a thing taken out of the middle of a container's contents.
///
/// Each time, the map is saved and the file read back. The new thing must appear once, inside its
/// container. The file must reopen with nothing logged and save as the same file. Undo must give
/// back the exact file the map was first saved as, and redo the file with the change. Taking a
/// thing out and undoing it must also leave the container's list in its original order, because
/// a map file records that order. Finally a bag is made with two things in it and one of them is
/// deleted. A bag names its contents a second time, so the bag must forget the deleted thing and
/// undo must bring it back.
/// </summary>
public static partial class ContainerCheck
{
    /// <summary>What goes into a locker, and into a slot. Any small thing the game has will do.</summary>
    private const string Item = "Crowbar";

    /// <summary>A bag, and the container the game's storage keeps its things in.</summary>
    private const string Bag = "ClothingBeltUtility";
    private const string BagContainer = "storagebase";

    /// <returns>0 if every map passed.</returns>
    public static int Run(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        var session = EngineSession.Booted(forkRepo, kernelPack);
        var code = Run(session, forkRepo, maps, outFolder, say);
        CheckTimes.Report("containers", say);
        CheckTimes.Summary(say);
        return code;
    }

    /// <summary>The check on an engine that is up already.</summary>
    public static int Run(EngineSession session, string forkRepo, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        Directory.CreateDirectory(outFolder);
        return OnEach(maps, say, "Containers took and gave up their things, and undo gave back the first file each time.",
            (map, tally) => Check(session, forkRepo, map, outFolder, say, tally));
    }

    private sealed record Holder(int Uid, string Id, string Kind, int[] Entities, bool CanInsert);

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

            // Only what stands on a grid itself: a thing inside a thing is the same case again.
            var holders = Containers(session, document, scene.Entities.Where(entity => grids.Contains(entity.Parent)).Select(entity => entity.Uid));
            say($"  {holders.Count} containers on {holders.Select(holder => holder.Uid).Distinct().Count()} entities, holding {holders.Sum(holder => holder.Entities.Length)} things");

            // ---- a new thing in a locker ----
            var locker = holders.Where(holder => holder.Kind == "Container")
                .OrderByDescending(holder => holder.Id == "entity_storage")
                .ThenByDescending(holder => holder.Entities.Length)
                .FirstOrDefault();
            if (locker == null)
            {
                CheckSkips.Skip(say, "a new thing put into a container: nothing on the map has a container that holds many");
            }
            else
            {
                var delta = Edit(session, document, "Put in", new EditOp("contain", Parent: locker.Uid, Container: locker.Id, Prototype: Item));
                var made = delta.Created.Count == 1 ? delta.Created[0] : 0;
                var reported = delta.Entities.FirstOrDefault(entity => entity.Uid == made);
                tally.Expect(reported is { Contained: true } && reported.Parent == locker.Uid && reported.Prototype == Item,
                    $"a {Item} put into '{locker.Id}' of a {byUid[locker.Uid].Prototype} is reported as made, inside it");
                tally.Expect(delta.Details.Any(details => details.Uid == locker.Uid), "the delta carries the holder's component data anew");
                var now = Containers(session, document, [locker.Uid]).First(holder => holder.Id == locker.Id);
                tally.Expect(now.Entities.SequenceEqual(locker.Entities.Append(made)), $"the container lists it last, after the {locker.Entities.Length} it held");

                var added = Save(session, document, Out("added"));
                tally.Expect(Invalid(added) == Invalid(before), "the save names nothing that is not in it");
                Reopen(session, tally, say, Out("added"), added, firstLog, scene.Entities.Count + 1, reopened =>
                {
                    var known = scene.Entities.Select(entity => entity.YamlUid).ToHashSet();
                    var holder = reopened.Entities.First(entity => entity.YamlUid == byUid[locker.Uid].YamlUid);
                    var fresh = reopened.Entities.Where(entity => !known.Contains(entity.YamlUid)).ToArray();
                    var inside = Containers(session, reopened.Document.Id, [holder.Uid]).First(found => found.Id == locker.Id);
                    var once = fresh.Length == 1 && fresh[0].Prototype == Item && fresh[0].Contained && fresh[0].Parent == holder.Uid
                               && inside.Entities.Count(uid => uid == fresh[0].Uid) == 1 && inside.Entities.Length == locker.Entities.Length + 1;
                    tally.Expect(once, $"opened again, the file holds one new {Item}, inside the container");
                    if (fresh.Length == 1)
                    {
                        var (asEntity, asListed) = (added.Count(line => line.Trim() == $"- uid: {fresh[0].YamlUid}"), added.Count(line => line.Trim() == $"- {fresh[0].YamlUid}"));
                        tally.Expect(asEntity == 1 && asListed == 1, $"the text has it once as an entity and names it once in a list (uid {fresh[0].YamlUid})");
                    }
                });

                var undone = Undo(session, document);
                tally.Expect(undone.Removed.Contains(made) && undone.Details.Any(details => details.Uid == locker.Uid), "undo reports it removed, and the holder's data anew");
                tally.Same(before, Save(session, document, Out("added-undone")), "undone, the save is the first file");
                Redo(session, document);
                tally.Same(added, Save(session, document, Out("added-redone")), "redone, the save is the file with the thing in it");
                Undo(session, document);
            }

            // ---- a slot holds one ----
            var slot = holders.FirstOrDefault(holder => holder.Kind == "ContainerSlot" && holder.Entities.Length == 0 && holder.CanInsert);
            if (slot == null)
            {
                CheckSkips.Skip(say, "a thing put into a slot, and a second one refused: nothing on the map has an empty slot");
            }
            else
            {
                var delta = Edit(session, document, "Put in", new EditOp("contain", Parent: slot.Uid, Container: slot.Id, Prototype: Item));
                var filled = Containers(session, document, [slot.Uid]).First(holder => holder.Id == slot.Id);
                tally.Expect(delta.Created.Count == 1 && filled.Entities.SequenceEqual(delta.Created) && !filled.CanInsert,
                    $"the empty slot '{slot.Id}' of a {byUid[slot.Uid].Prototype} takes one thing and then says it has no room");
                var refusal = Refusal(session, document, new EditOp("contain", Parent: slot.Uid, Container: slot.Id, Prototype: Item));
                var after = Containers(session, document, [slot.Uid]).First(holder => holder.Id == slot.Id);
                tally.Expect(refusal != null && after.Entities.SequenceEqual(filled.Entities), $"a second thing is refused and nothing is left of it ({refusal})");

                var slotted = Save(session, document, Out("slotted"));
                Reopen(session, tally, say, Out("slotted"), slotted, firstLog, scene.Entities.Count + 1, null);
                Undo(session, document);
                tally.Same(before, Save(session, document, Out("slotted-undone")), "undone, the save is the first file");
            }

            // ---- a thing taken out from among others ----
            var full = holders.Where(holder => holder.Kind == "Container" && holder.Entities.Length >= 2).OrderByDescending(holder => holder.Entities.Length).FirstOrDefault();
            if (full == null)
            {
                CheckSkips.Skip(say, "a thing taken out from among others and put back by undo: no container on the map holds two things");
            }
            else
            {
                var taken = full.Entities[0];
                foreach (var kind in new[] { "uncontain", "delete" })
                {
                    var delta = Edit(session, document, "Take out", new EditOp(kind, Uid: taken));
                    var left = Containers(session, document, [full.Uid]).First(holder => holder.Id == full.Id);
                    tally.Expect(delta.Removed.Contains(taken) && left.Entities.SequenceEqual(full.Entities.Skip(1)) && delta.Details.Any(details => details.Uid == full.Uid),
                        $"\"{kind}\" of the first of {full.Entities.Length} things in a {byUid[full.Uid].Prototype} removes it and leaves the others in order");

                    var removed = Save(session, document, Out($"{kind}"));
                    tally.Expect(Invalid(removed) == Invalid(before), "the save names nothing that is not in it");
                    if (kind == "uncontain")
                        Reopen(session, tally, say, Out(kind), removed, firstLog, scene.Entities.Count - delta.Removed.Count, null);

                    Undo(session, document);
                    var back = Containers(session, document, [full.Uid]).First(holder => holder.Id == full.Id);
                    tally.Expect(back.Entities.SequenceEqual(full.Entities), "undo puts it back first in the container's list");
                    tally.Same(before, Save(session, document, Out($"{kind}-undone")), "undone, the save is the first file");
                    if (kind == "uncontain")
                    {
                        Redo(session, document);
                        tally.Same(removed, Save(session, document, Out($"{kind}-redone")), "redone, the save is the file without it");
                        Undo(session, document);
                    }
                }

                // The engine puts a thing into a container at 0,0 unturned. Undo of a delete
                // takes out and puts in again those behind it, and each must keep its own.
                var placing = full.Entities.SelectMany((uid, i) => new[]
                {
                    new EditOp("move", Uid: uid, X: 0.25f + i, Y: -0.5f),
                    new EditOp("rotate", Uid: uid, Rotation: Math.PI / 2 * (i % 3 + 1)),
                }).ToArray();
                Edit(session, document, "Places", placing);
                var placed = Save(session, document, Out("placed"));
                Edit(session, document, "Take out", new EditOp("delete", Uid: taken));
                Undo(session, document);
                tally.Same(placed, Save(session, document, Out("placed-undone")),
                    $"with a place and a turn for each thing in the {byUid[full.Uid].Prototype}, undone the delete of the first saves the file before it");
                Undo(session, document);
            }

            // ---- a bag names what it holds ----
            // A bag keeps, beside its container, a place in its grid of cells for each thing
            // in it, by the thing's uid. A small map may have no bag, so one is made here.
            var loose = scene.Entities.First(entity => grids.Contains(entity.Parent));
            {
                var bag = Edit(session, document, "Place", new EditOp("spawn", Prototype: Bag, Parent: loose.Parent, X: loose.X, Y: loose.Y)).Created[0];
                var things = new[] { 0, 1 }.Select(_ => Edit(session, document, "Put in", new EditOp("contain", Parent: bag, Container: BagContainer, Prototype: Item)).Created[0]).ToArray();

                // The game gives a thing its place as it goes into a bag, and so must "contain".
                var bagged = Save(session, document, Out("bagged"));
                Reopen(session, tally, say, Out("bagged"), bagged, firstLog, scene.Entities.Count + 3, reopened =>
                {
                    var known = scene.Entities.Select(entity => entity.YamlUid).ToHashSet();
                    var saved = reopened.Entities.First(entity => !known.Contains(entity.YamlUid) && entity.Prototype == Bag);
                    var held = reopened.Entities.Where(entity => entity.Parent == saved.Uid && entity.Contained).Select(entity => entity.Uid).Order().ToArray();
                    var named = Places(session, reopened.Document.Id, saved.Uid).Order().ToArray();
                    tally.Expect(held.Length == 2 && named.SequenceEqual(held), $"saved, the bag's storedItems names each {Item} put into it ({named.Length} named of {held.Length})");
                });
                Undo(session, document);
                Redo(session, document);
                tally.Same(bagged, Save(session, document, Out("bagged-redone")), "the second put in undone and redone, the save is the file with both in the bag");
                Edit(session, document, "Places", new EditOp("components", Uid: bag, Components: WithPlaces(session, document, bag, things)));
                var packed = Save(session, document, Out("packed"));
                tally.Expect(things.All(thing => Places(session, document, bag).Contains(thing)) && Invalid(packed) == Invalid(before),
                    $"a {Bag} made with two things in it names a place for each");

                var delta = Edit(session, document, "Take out", new EditOp("delete", Uid: things[0]));
                var left = Places(session, document, bag);
                tally.Expect(delta.Removed.Contains(things[0]) && left.SequenceEqual([things[1]]), "\"delete\" of the first takes its place out of the bag's list and leaves the other");
                var unpacked = Save(session, document, Out("unpacked"));
                tally.Expect(Invalid(unpacked) == Invalid(before), "the save names nothing that is not in it");
                Reopen(session, tally, say, Out("unpacked"), unpacked, firstLog, scene.Entities.Count + 2, null);

                Undo(session, document);
                tally.Expect(Places(session, document, bag).SequenceEqual(things), "undo puts its place back, first in the list");
                tally.Same(packed, Save(session, document, Out("unpacked-undone")), "undone, the save is the file with both things in the bag");
                for (var step = 0; step < 4; step++)
                    Undo(session, document);
            }

            // ---- the fields of a component's form (ContainerCheck.Fields.cs) ----
            CheckFields(session, document, loose, scene, Out, say, tally);

            // ---- what the kernel must refuse ----
            var anyHolder = holders.FirstOrDefault();
            var refusals = new List<(string What, string? Said)>
            {
                ("\"uncontain\" of a thing that is in no container", Refusal(session, document, new EditOp("uncontain", Uid: loose.Uid))),
                ("a container the entity does not have", Refusal(session, document, new EditOp("contain", Parent: loose.Uid, Container: "no such container", Prototype: Item))),
            };
            if (anyHolder != null)
                refusals.Add(("a prototype the game does not have", Refusal(session, document, new EditOp("contain", Parent: anyHolder.Uid, Container: anyHolder.Id, Prototype: "NoSuchPrototypeAtAll"))));
            foreach (var (what, said) in refusals)
                tally.Expect(said != null, $"refused: {what} ({said})");

            tally.Same(before, Save(session, document, Out("after")), "after all of it the save is the first file");
            return tally.Failed == 0;
        }
        finally
        {
            session.Close(document);
        }
    }

    /// <summary>
    /// Opens a save again: nothing may be logged that the map did not log when first opened,
    /// the count of entities must be right, and saved again it must be the same file.
    /// </summary>
    private static void Reopen(EngineSession session, Tally tally, Action<string> say, string path, string[] saved, HashSet<string> firstLog, int entities, Action<Scene>? look)
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

            look?.Invoke(reopened);
            tally.Same(saved, Save(session, reopened.Document.Id, Path.ChangeExtension(path, ".again.yml")), "saved again it is the same file");
        }
        finally
        {
            session.Close(reopened.Document.Id);
        }
    }

    /// <summary>The things a bag's component data names a place for, in the order it names them.</summary>
    private static int[] Places(EngineSession session, int document, int bag)
    {
        using var details = Ask(session, "details", $"{{\"doc\":{document},\"uids\":[{bag}]}}");
        var components = JsonNode.Parse(details.RootElement[0].GetProperty("components").GetString() ?? "[]")!.AsArray();
        var storage = components.OfType<JsonObject>().FirstOrDefault(component => component["type"]?.GetValue<string>() == "Storage");
        return storage?["storedItems"] is JsonObject places ? places.Select(place => int.Parse(place.Key)).ToArray() : [];
    }

    /// <summary>A bag's component data, as "details" gives it, with a place for each of these things.</summary>
    private static string WithPlaces(EngineSession session, int document, int bag, int[] things)
    {
        using var details = Ask(session, "details", $"{{\"doc\":{document},\"uids\":[{bag}]}}");
        var components = JsonNode.Parse(details.RootElement[0].GetProperty("components").GetString() ?? "[]")!.AsArray();
        var storage = components.OfType<JsonObject>().FirstOrDefault(component => component["type"]?.GetValue<string>() == "Storage");
        if (storage == null)
        {
            storage = new JsonObject { ["type"] = "Storage" };
            components.Add(storage);
        }

        var places = new JsonObject();
        for (var i = 0; i < things.Length; i++)
            places[things[i].ToString()] = new JsonObject { ["position"] = $"{i},0", ["_rotation"] = "South" };
        storage["storedItems"] = places;
        return components.ToJsonString();
    }

    /// <summary>The "containers" question, one row for each container.</summary>
    private static List<Holder> Containers(EngineSession session, int document, IEnumerable<int> uids)
    {
        using var answer = Ask(session, "containers", $"{{\"doc\":{document},\"uids\":[{string.Join(",", uids)}]}}");
        var found = new List<Holder>();
        foreach (var entity in answer.RootElement.EnumerateArray())
        {
            foreach (var container in entity.GetProperty("containers").EnumerateArray())
            {
                found.Add(new Holder(
                    entity.GetProperty("uid").GetInt32(),
                    container.GetProperty("id").GetString()!,
                    container.GetProperty("kind").GetString()!,
                    container.GetProperty("entities").EnumerateArray().Select(uid => uid.GetInt32()).ToArray(),
                    container.GetProperty("canInsert").GetBoolean()));
            }
        }

        return found;
    }
}
