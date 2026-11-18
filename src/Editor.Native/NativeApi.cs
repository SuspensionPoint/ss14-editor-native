using System.Diagnostics;
using System.Text.Json;
using SS14.EditorKernel;

namespace SS14.EditorNative;

/// <summary>
/// The engine as the native editor's interface uses it: named commands with JSON arguments and
/// results, each covering both kernels. Where a command changes how a document looks, its draw
/// list is recorded before the command returns and is read with NativeExports.DrawList.
///
///   open      { path }                          the scene and how long it took
///   new       { }                               the same, for an empty map
///   close     { doc }
///   edit      { doc, label, ops, merge }        what changed. merge joins the previous undo step
///   undo/redo { doc }                           what changed
///   save      { doc, path }                     writes the file
///   layers    { doc, subFloor }                 show what lies under floors. doc may be 0
///   previews  { items: [{key, prototype}], directions }   draw list for document -1
///   backdrop  { doc, cache }                    the map's backdrop as image files. Slow the
///                                               first time, safe to call from another thread.
///                                               "chosen" if the map names it, "planet" if the
///                                               game draws none behind the map
///   backdrop.named { name, cache }              one of the fork's backdrops by its id, the same
///   backdrops { }                               the ids of the fork's backdrops, in order
///
/// A map's backdrop is set with the "components" op of an edit on the map entity: its
/// "Parallax" component, or none for the game's default. A grid file and a planet refuse it.
///
/// Anything else is one of the document kernel's own commands (KernelApi.cs). Among them:
///
///   copy      { doc, uids }                     the entities as text for a "paste" op of an
///                                               edit, with the counts and the cell they are
///                                               measured from. It changes nothing.
///   containers { doc, uids }                    each entity's containers, with what each
///                                               holds and whether it has room. The "contain"
///                                               and "uncontain" ops of an edit change them.
///   links     { doc }                           every device link and device list entry. After
///                                               this the "linked" of each delta keeps a
///                                               mirror right. The "link", "unlink", "list.add"
///                                               and "list.remove" ops of an edit change them.
///   ports     { }                               the game's ports and which each prototype has
///   ports.of  { doc, uids }                     the ports those entities have now
///   component.fields { doc, uid, names, known } each data field of the entity's components
///                                               (or the named ones): tag, kind, C# type,
///                                               value now and the prototype's, read-only,
///                                               an enum's names, a prototype id's kind, and
///                                               the ids of each prototype kind not in known
///                                               (ComponentFields.cs). It changes nothing.
///   air       { doc, grids }                    the air each grid stores: its mixes, the mix
///                                               of each tile, and the tiles with none, walls
///                                               and floors apart (Air.cs). grids may be left
///                                               out for all of them. It changes nothing.
///   air.fix.preview { doc, grids }              for each grid, how many of its tiles and of
///                                               the cells off its edge a fix of its air would
///                                               change, or why the game would refuse to fix
///                                               it (AirFix.cs). The "air.fix" op of an edit
///                                               fixes it. It changes nothing.
///
/// Grids come and go by the "grid.add" and "grid.remove" ops of an edit. A delta that removes
/// a grid is followed by a complete draw list.
/// </summary>
public sealed class NativeApi(EngineSession session)
{
    private static readonly JsonSerializerOptions Json = KernelApi.Json;

    private sealed record PreviewRequest(int Key, string Prototype);

    public string Handle(string op, string argsJson)
    {
        using var parsed = JsonDocument.Parse(string.IsNullOrEmpty(argsJson) ? "{}" : argsJson);
        var args = parsed.RootElement;
        switch (op)
        {
            case "open":
            {
                var path = args.GetProperty("path").GetString()!;
                var (scene, timing) = session.Open(File.ReadAllText(path), Path.GetFileName(path));
                return JsonSerializer.Serialize(new { scene, timing }, Json);
            }
            case "new":
            {
                var (scene, timing) = session.New();
                return JsonSerializer.Serialize(new { scene, timing }, Json);
            }
            case "close":
                session.Close(args.GetProperty("doc").GetInt32());
                return "null";
            case "edit":
            {
                var ops = args.GetProperty("ops").Deserialize<List<EditOp>>(Json) ?? [];
                var label = args.TryGetProperty("label", out var given) ? given.GetString() ?? "Edit" : "Edit";
                var merge = args.TryGetProperty("merge", out var joins) && joins.ValueKind == JsonValueKind.True;
                var (delta, timing) = session.Edit(args.GetProperty("doc").GetInt32(), label, ops, merge);
                return JsonSerializer.Serialize(new { delta, timing }, Json);
            }
            case "undo":
            case "redo":
            {
                var document = args.GetProperty("doc").GetInt32();
                var (delta, timing) = op == "undo" ? session.Undo(document) : session.Redo(document);
                return JsonSerializer.Serialize(new { delta, timing }, Json);
            }
            case "save":
            {
                var saved = session.Save(args.GetProperty("doc").GetInt32());
                var path = args.GetProperty("path").GetString()!;
                if (saved.Ok && saved.Yaml != null)
                    File.WriteAllText(path, saved.Yaml);
                return JsonSerializer.Serialize(new { saved.Ok, saved.Failure, saved.Milliseconds, bytes = saved.Yaml?.Length ?? 0, saved.Log }, Json);
            }
            case "layers":
            {
                var timing = session.ShowSubFloor(args.GetProperty("doc").GetInt32(), args.GetProperty("subFloor").GetBoolean());
                return JsonSerializer.Serialize(new { timing }, Json);
            }
            case "previews":
            {
                var items = args.GetProperty("items").Deserialize<List<PreviewRequest>>(Json) ?? [];
                var directions = args.TryGetProperty("directions", out var all) && all.ValueKind == JsonValueKind.True;
                var sw = Stopwatch.StartNew();
                var bytes = session.Previews(items.Select(item => (item.Key, item.Prototype)).ToArray(), directions);
                return JsonSerializer.Serialize(new { count = items.Count, bytes, milliseconds = sw.Elapsed.TotalMilliseconds }, Json);
            }
            case "backdrop":
                return JsonSerializer.Serialize(session.Backdrop(args.GetProperty("doc").GetInt32(), args.GetProperty("cache").GetString()!), Json);
            case "backdrop.named":
                return JsonSerializer.Serialize(session.NamedBackdrop(args.GetProperty("name").GetString()!, args.GetProperty("cache").GetString()!), Json);
            case "backdrops":
                return JsonSerializer.Serialize(session.BackdropNames(), Json);
            default:
                return session.Command(op, argsJson);
        }
    }

    /// <summary>
    /// Runs every kind of command once on a small map that is then thrown away. The runtime
    /// prepares code the first time it runs, which made the user's first edit of each kind
    /// several times slower than the ones after it; this pays that cost while the engine is
    /// still starting. Returns how long it took. Never fails: a warm-up that cannot run is
    /// only a slower first edit.
    /// </summary>
    public long WarmUp()
    {
        var sw = Stopwatch.StartNew();
        var document = 0;
        try
        {
            using var opened = JsonDocument.Parse(Handle("new", "{}"));
            var scene = opened.RootElement.GetProperty("scene");
            document = scene.GetProperty("document").GetProperty("id").GetInt32();
            var grid = scene.GetProperty("grids")[0].GetProperty("uid").GetInt32();

            using var tiles = JsonDocument.Parse(Handle("tiles", "{}"));
            var floor = tiles.RootElement.EnumerateArray()
                .Select(tile => tile.GetProperty("id").GetString()!)
                .FirstOrDefault(id => id != "Space" && id.StartsWith("Floor", StringComparison.Ordinal)) ?? "Plating";

            string Edit(string ops, bool merge = false) =>
                Handle("edit", $"{{\"doc\":{document},\"label\":\"Warm-up\",\"merge\":{(merge ? "true" : "false")},\"ops\":[{ops}]}}");
            string Tile(int x, int y, string id) => $"{{\"kind\":\"tile\",\"grid\":{grid},\"x\":{x},\"y\":{y},\"tile\":\"{id}\"}}";

            // A patch of floor, a stroke across it, and a tile taken away again.
            Edit(string.Join(",", Enumerable.Range(0, 36).Select(i => Tile(i % 6, i / 6, floor))));
            Edit(Tile(7, 0, floor));
            Edit(Tile(8, 0, floor), merge: true);
            Edit(Tile(8, 0, "Space"));

            // Something anchored, as walls and machines are, and something loose; placed next
            // to each other, moved, turned, deleted, and all of it undone and redone.
            using var prototypes = JsonDocument.Parse(Handle("prototypes", "{}"));
            string? Pick(bool snapped) => prototypes.RootElement.EnumerateArray()
                .Where(p => !p.GetProperty("abstract").GetBoolean() && !p.GetProperty("hideSpawnMenu").GetBoolean())
                .Where(p => p.GetProperty("placementMode").GetString() == "SnapgridCenter" == snapped)
                .Where(p => p.GetProperty("components").EnumerateArray().Any(c => c.GetString() == "Sprite"))
                .Where(p => !snapped || p.GetProperty("components").EnumerateArray().Any(c => c.GetString() == "IconSmooth"))
                .Select(p => p.GetProperty("id").GetString())
                .FirstOrDefault();

            foreach (var prototype in new[] { Pick(snapped: true), Pick(snapped: false) })
            {
                if (prototype == null)
                    continue;

                try
                {
                    string Spawn(float x, float y) => $"{{\"kind\":\"spawn\",\"prototype\":\"{prototype}\",\"parent\":{grid},\"x\":{x},\"y\":{y}}}";
                    Edit(Spawn(1.5f, 1.5f));
                    using var placed = JsonDocument.Parse(Edit(Spawn(2.5f, 1.5f)));
                    var created = placed.RootElement.GetProperty("delta").GetProperty("created");
                    if (created.GetArrayLength() == 0)
                        continue;

                    var uid = created[0].GetInt32();
                    Edit($"{{\"kind\":\"move\",\"uid\":{uid},\"x\":3.5,\"y\":2.5}}");
                    Edit($"{{\"kind\":\"move\",\"uid\":{uid},\"x\":3.5,\"y\":3.5}}", merge: true);
                    Edit($"{{\"kind\":\"rotate\",\"uid\":{uid},\"rotation\":1.5707963}}");
                    Handle("details", $"{{\"doc\":{document},\"uids\":[{uid}]}}");
                    Handle("component.fields", $"{{\"doc\":{document},\"uid\":{uid}}}");
                    Edit($"{{\"kind\":\"delete\",\"uid\":{uid}}}");
                    Handle("previews", $"{{\"items\":[{{\"key\":0,\"prototype\":\"{prototype}\"}}],\"directions\":true}}");
                }
                catch (Exception)
                {
                    // This prototype would not be placed; the rest still warms up.
                }
            }

            for (var i = 0; i < 4; i++)
                Handle("undo", $"{{\"doc\":{document}}}");
            for (var i = 0; i < 4; i++)
                Handle("redo", $"{{\"doc\":{document}}}");
        }
        catch (Exception)
        {
        }

        try
        {
            if (document != 0)
                session.Close(document);
        }
        catch (Exception)
        {
        }

        return sw.ElapsedMilliseconds;
    }
}
