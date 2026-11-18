using System.Text.Json;
using SS14.EditorKernel;

namespace SS14.EditorNative;

/// <summary>
/// Checks that moving things leaves everything that refers to them intact: the devices an air
/// alarm or fire alarm is linked to, and what lockers, crates and the like hold.
///
/// The web editor moved an entity by deleting it and adding a copy under a new uid. The uids kept
/// in other entities' data then pointed at nothing, or at whatever got that number next. Here a
/// move is the engine moving the entity it already has, so the only question is whether that holds
/// all the way to the file. The check saves the map as opened. Then it moves alarms with the
/// devices they list, everything around them, and containers with their contents, one cell over,
/// and saves again. The two files are compared line by line, and only position lines may differ,
/// one for each entity moved. Then the moved map is reopened and saved once more, which must give
/// the same file, since the loader would drop any reference it couldn't resolve. The header line
/// that says when the file was saved is ignored in both comparisons.
/// </summary>
public static class MoveCheck
{
    /// <returns>0 if every map passed.</returns>
    public static int Run(string forkRepo, string? kernelPack, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        var session = EngineSession.Booted(forkRepo, kernelPack);
        var code = Run(session, forkRepo, maps, outFolder, say);
        CheckTimes.Report("moves", say);
        CheckTimes.Summary(say);
        return code;
    }

    /// <summary>The check on an engine that is up already.</summary>
    public static int Run(EngineSession session, string forkRepo, IReadOnlyList<string> maps, string outFolder, Action<string> say)
    {
        Directory.CreateDirectory(outFolder);

        var skipsBefore = CheckSkips.Count;
        var failed = 0;
        foreach (var map in maps)
        {
            say(map);
            try
            {
                if (!Check(session, forkRepo, map, outFolder, say))
                    failed++;
            }
            catch (Exception e)
            {
                say($"  FAIL  {e.Message}");
                failed++;
            }
        }

        say(failed == 0 ? "\nMoving left every reference as it was." : $"\n{failed} of {maps.Count} maps failed.");
        CheckSkips.Report(say, skipsBefore);
        return failed == 0 ? 0 : 1;
    }

    private static bool Check(EngineSession session, string forkRepo, string map, string outFolder, Action<string> say)
    {
        var name = Path.GetFileNameWithoutExtension(map);
        var scene = Opened(session, CheckTimes.Open(map), File.ReadAllText(Path.Combine(forkRepo, "Resources", map)), map);
        var document = scene.Document.Id;
        try
        {
            var before = Save(session, document, Path.Combine(outFolder, $"{name}.before.yml"));

            var byUid = scene.Entities.ToDictionary(entity => entity.Uid);
            var grids = scene.Grids.ToDictionary(grid => grid.Uid);
            var tiles = scene.Grids.ToDictionary(grid => grid.Uid, grid =>
            {
                var cells = new HashSet<(int, int)>();
                for (var i = 0; i + 4 < grid.Tiles.Count; i += 5)
                    cells.Add((grid.Tiles[i], grid.Tiles[i + 1]));
                return cells;
            });

            // Alarms, with the devices each lists.
            var alarms = scene.Entities
                .Where(entity => !entity.Contained && grids.ContainsKey(entity.Parent))
                .Where(entity => entity.Prototype.Contains("AirAlarm", StringComparison.Ordinal) || entity.Prototype.Contains("FireAlarm", StringComparison.Ordinal))
                .ToArray();
            var chosen = new HashSet<int>();
            var (linkedAlarms, links) = (0, 0);
            foreach (var (alarm, devices) in Devices(session, document, alarms, byUid))
            {
                if (devices.Count == 0 || linkedAlarms >= 12)
                    continue;

                linkedAlarms++;
                links += devices.Count;
                // The room, more or less: everything on the grid in the box around the alarm
                // and its devices.
                var room = devices.Where(device => device.Parent == alarm.Parent).Append(alarm).ToArray();
                var (left, right) = (room.Min(e => e.X) - 0.5f, room.Max(e => e.X) + 0.5f);
                var (bottom, top) = (room.Min(e => e.Y) - 0.5f, room.Max(e => e.Y) + 0.5f);
                foreach (var entity in scene.Entities)
                {
                    if (!entity.Contained && entity.Parent == alarm.Parent && entity.X >= left && entity.X <= right && entity.Y >= bottom && entity.Y <= top)
                        chosen.Add(entity.Uid);
                }

                foreach (var device in devices)
                    chosen.Add(device.Uid);
            }

            // Things that hold other things.
            var holders = scene.Entities.Where(entity => entity.Contained).GroupBy(entity => entity.Parent).ToDictionary(group => group.Key, group => group.Count());
            var (containers, contents) = (0, 0);
            foreach (var (holder, count) in holders.OrderByDescending(pair => pair.Value))
            {
                if (containers >= 40 || !byUid.TryGetValue(holder, out var entity) || entity.Contained || !grids.ContainsKey(entity.Parent))
                    continue;

                chosen.Add(holder);
                containers++;
                contents += count;
            }

            // One cell over, for everything chosen that still has floor under it there.
            var moved = chosen
                .Select(uid => byUid[uid])
                .Where(entity => !entity.Contained && tiles.TryGetValue(entity.Parent, out var cells) && cells.Contains(((int)MathF.Floor(entity.X + 1), (int)MathF.Floor(entity.Y))))
                .ToArray();
            var movedContainers = moved.Count(entity => holders.ContainsKey(entity.Uid));
            if (moved.Length == 0)
            {
                say("  FAIL  found nothing to move");
                return false;
            }

            // As the editor asks for it when a selection is dragged (session.rs).
            var ops = moved.Select(entity => new EditOp("move", Uid: entity.Uid, X: entity.X + 1, Y: entity.Y)).ToArray();
            SceneDelta delta;
            using (CheckTimes.In(CheckTimes.Edit))
                (delta, _) = session.Edit(document, "Move", ops);
            var after = Save(session, document, Path.Combine(outFolder, $"{name}.moved.yml"));
            say($"  moved {moved.Length} entities one cell: {linkedAlarms} alarms with {links} linked devices and what stands around them, {movedContainers} containers holding {contents} things (of {containers} looked at)");
            if (linkedAlarms == 0)
                CheckSkips.Skip(say, "an alarm moved with the devices it lists: the map has no alarm that lists a device");
            if (movedContainers == 0)
                CheckSkips.Skip(say, "a container moved with what it holds: the map has none with floor beside it");

            var ok = true;
            if (delta.Removed.Count > 0 || delta.Created.Count > 0)
            {
                say($"  FAIL  the move removed {delta.Removed.Count} entities and created {delta.Created.Count}");
                ok = false;
            }

            var (same, positions, others) = Compare(before, after);
            if (others.Count > 0 || !same || positions != moved.Length)
            {
                say($"  FAIL  between the two saves {positions} position lines differ for {moved.Length} entities moved, and {others.Count} other lines{(same ? "" : "; they are not the same length")}:");
                foreach (var line in others.Take(12))
                    say($"        {line}");
                ok = false;
            }
            else
            {
                say($"  ok    the saved file differs in {positions} position lines, one for each entity moved, and in nothing else ({before.Length} lines)");
            }

            // Whatever was only half true in the file would show when it is read again.
            var reopened = Opened(session, CheckTimes.Reopen, File.ReadAllText(Path.Combine(outFolder, $"{name}.moved.yml")), $"{name}.moved.yml");
            try
            {
                var again = Save(session, reopened.Document.Id, Path.Combine(outFolder, $"{name}.moved-again.yml"));
                var (sameAgain, positionsAgain, othersAgain) = Compare(after, again);
                if (reopened.Entities.Count != scene.Entities.Count || !sameAgain || positionsAgain + othersAgain.Count > 0)
                {
                    say($"  FAIL  opened again: {reopened.Entities.Count} entities (were {scene.Entities.Count}); saved again, {positionsAgain + othersAgain.Count} lines differ");
                    foreach (var line in othersAgain.Take(12))
                        say($"        {line}");
                    ok = false;
                }
                else
                {
                    say($"  ok    opened again it has the same {reopened.Entities.Count} entities, and saved again it is the same file");
                }
            }
            finally
            {
                session.Close(reopened.Document.Id);
            }

            return ok;
        }
        finally
        {
            session.Close(document);
        }
    }

    private static Scene Opened(EngineSession session, string stage, string yaml, string source)
    {
        using var timed = CheckTimes.In(stage);
        return session.Open(yaml, source).Scene;
    }

    private static string[] Save(EngineSession session, int document, string path)
    {
        SaveResult saved;
        using (CheckTimes.In(CheckTimes.Save))
            saved = session.Save(document);
        if (!saved.Ok || saved.Yaml == null)
            throw new InvalidOperationException($"not saved: {saved.Failure}");

        using var timed = CheckTimes.In(CheckTimes.Text);
        File.WriteAllText(path, saved.Yaml);
        return saved.Yaml.Split('\n');
    }

    /// <summary>
    /// The devices each alarm lists, read from the component data the map file stores (the
    /// "details" command the editor's selection panel uses), where a device is named by the
    /// uid the scene has for it, and not by its uid in the file.
    /// </summary>
    internal static IEnumerable<(SceneEntity Alarm, List<SceneEntity> Devices)> Devices(
        EngineSession session, int document, IReadOnlyList<SceneEntity> alarms, Dictionary<int, SceneEntity> byUid)
    {
        if (alarms.Count == 0)
            yield break;

        var json = session.Command("details", $"{{\"doc\":{document},\"uids\":[{string.Join(",", alarms.Select(alarm => alarm.Uid))}]}}");
        using var details = JsonDocument.Parse(json);
        var asked = alarms.ToDictionary(alarm => alarm.Uid);
        foreach (var entity in details.RootElement.EnumerateArray())
        {
            using var components = JsonDocument.Parse(entity.GetProperty("components").GetString() ?? "[]");
            var devices = new List<SceneEntity>();
            foreach (var component in components.RootElement.EnumerateArray())
            {
                if (component.GetProperty("type").GetString() != "DeviceList" || !component.TryGetProperty("devices", out var listed) || listed.ValueKind != JsonValueKind.Array)
                    continue;

                foreach (var device in listed.EnumerateArray())
                {
                    var uid = device.ValueKind == JsonValueKind.Number ? device.GetInt32() : int.TryParse(device.GetString(), out var parsed) ? parsed : 0;
                    if (byUid.TryGetValue(uid, out var found))
                        devices.Add(found);
                }
            }

            yield return (asked[entity.GetProperty("uid").GetInt32()], devices);
        }
    }

    /// <summary>
    /// Whether two saves have the same number of lines, how many lines differ that are an
    /// entity's position, and the other lines that differ.
    /// </summary>
    private static (bool SameLength, int Positions, List<string> Others) Compare(string[] before, string[] after)
    {
        using var timed = CheckTimes.In(CheckTimes.Compare);
        var positions = 0;
        var others = new List<string>();
        for (var i = 0; i < Math.Min(before.Length, after.Length); i++)
        {
            if (before[i] == after[i])
                continue;

            // The file's heading says when it was saved.
            if (i < 12 && before[i].TrimStart().StartsWith("time:", StringComparison.Ordinal) && after[i].TrimStart().StartsWith("time:", StringComparison.Ordinal))
                continue;

            if (before[i].TrimStart().StartsWith("pos:", StringComparison.Ordinal) && after[i].TrimStart().StartsWith("pos:", StringComparison.Ordinal))
                positions++;
            else
                others.Add($"line {i + 1}: \"{before[i].Trim()}\" became \"{after[i].Trim()}\"");
        }

        return (before.Length == after.Length, positions, others);
    }
}
