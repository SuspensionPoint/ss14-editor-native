using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using SS14.EditorKernel;
using static SS14.EditorNative.CheckKit;

namespace SS14.EditorNative;

/// <summary>
/// Checks the "component.fields" question that the typed forms are built from. It checks the kinds
/// it reports for the fields of a battery, a door and a light. Then it sets a light's radius the
/// way a form does, through the "components" op with the entity's own data, saves, reopens, and
/// resets it to the prototype. Every edit is undone afterwards.
/// </summary>
public static partial class ContainerCheck
{
    /// <summary>The entities made for the check, each with a component and the kinds of some of its fields.</summary>
    private static readonly (string Prototype, string Component, (string Tag, string Kind)[] Kinds)[] Formed =
    [
        ("SMESBasic", "Battery", [("maxCharge", "float"), ("startingCharge", "float"), ("lastUpdate", "other")]),
        ("AirlockGlass", "Door", [("state", "enum"), ("closeTimeOne", "timespan"), ("partial", "bool"), ("openSound", "other")]),
        ("PoweredSmallLight", "PointLight", [("color", "color"), ("radius", "float"), ("enabled", "bool"), ("offset", "vector2")]),
    ];

    private static void CheckFields(EngineSession session, int document, SceneEntity loose, Scene scene, Func<string, string> Out, Action<string> say, Tally tally)
    {
        var made = new Dictionary<string, int>();
        foreach (var (prototype, _, _) in Formed)
            made[prototype] = Edit(session, document, "Place", new EditOp("spawn", Prototype: prototype, Parent: loose.Parent, X: loose.X, Y: loose.Y)).Created[0];
        var edits = Formed.Length;

        try
        {
            var prototypeFields = 0;
            foreach (var (prototype, component, kinds) in Formed)
            {
                using var answer = Fields(session, document, made[prototype]);
                var fields = Component(answer, component);
                if (!tally.Expect(fields != null, $"component.fields gives the {component} of a {prototype}"))
                    continue;

                foreach (var (tag, kind) in kinds)
                {
                    var found = fields!.Value.EnumerateArray().FirstOrDefault(field => field.GetProperty("tag").GetString() == tag);
                    var given = found.ValueKind == JsonValueKind.Object ? found.GetProperty("kind").GetString() : "nothing";
                    tally.Expect(given == kind, $"  {component}.{tag} is {kind} (given {given})");
                }

                // Every prototype id field names its kind, and the ids of a kind but entities' come with it.
                foreach (var field in answer.RootElement.GetProperty("components").EnumerateArray().SelectMany(entry => entry.GetProperty("fields").EnumerateArray()))
                {
                    if (field.GetProperty("kind").GetString() != "prototype")
                        continue;
                    prototypeFields++;
                    var kindOf = field.GetProperty("prototypeKind").GetString();
                    var listed = kindOf == "entity" || answer.RootElement.GetProperty("ids").TryGetProperty(kindOf ?? "", out _);
                    if (!listed)
                        tally.Expect(false, $"  the ids of {kindOf} come with the field {field.GetProperty("tag").GetString()}");
                }
            }

            say($"  {prototypeFields} prototype id fields on the three");
            using (var door = Fields(session, document, made["AirlockGlass"]))
            {
                var state = Component(door, "Door")!.Value.EnumerateArray().First(field => field.GetProperty("tag").GetString() == "state");
                var names = state.GetProperty("names").EnumerateArray().Select(name => name.GetString()).ToArray();
                tally.Expect(names.Contains("Closed") && names.Contains("Open") && state.GetProperty("value").GetString() == "Closed",
                    $"the door's state names its values ({string.Join(", ", names)}) and is Closed");

                // The engine's own netsync, which every component has, is left out.
                var netsync = door.RootElement.GetProperty("components").EnumerateArray()
                    .Count(component => component.GetProperty("fields").EnumerateArray().Any(field => field.GetProperty("tag").GetString() == "netsync"));
                tally.Expect(netsync == 0, $"no component of the door shows netsync ({netsync} do)");
            }

            // ---- a light's radius set as a form sets it ----
            var light = made["PoweredSmallLight"];
            double Radius(JsonElement answer, string which) => Component(answer, "PointLight")!.Value.EnumerateArray().First(field => field.GetProperty("tag").GetString() == "radius").GetProperty(which).GetDouble();
            double before;
            using (var answer = Fields(session, document, light))
            {
                before = Radius(answer.RootElement, "prototype");
                tally.Expect(Radius(answer.RootElement, "value") == before, $"the new light's radius is the prototype's ({before})");
            }

            var wanted = before + 1.5;
            var radius = wanted.ToString(CultureInfo.InvariantCulture);
            Edit(session, document, "Edit component", new EditOp("components", Uid: light, Components: WithOwn(session, document, light, "PointLight", "radius", JsonValue.Create(wanted))));
            edits++;
            using (var answer = Fields(session, document, light))
                tally.Expect(Radius(answer.RootElement, "value") == wanted && Radius(answer.RootElement, "prototype") == before, $"set through the op, the radius is {radius} and the prototype's is still {before}");

            var saved = Save(session, document, Out("radius"));
            tally.Expect(saved.Count(line => line.Trim() == $"radius: {radius}") == 1, $"the save writes the light's radius: {radius} once");
            var reopened = Opened(session, CheckTimes.Reopen, File.ReadAllText(Out("radius")), "radius.yml");
            try
            {
                var known = scene.Entities.Select(entity => entity.YamlUid).ToHashSet();
                var again = reopened.Entities.First(entity => !known.Contains(entity.YamlUid) && entity.Prototype == "PoweredSmallLight");
                using var answer = Fields(session, reopened.Document.Id, again.Uid);
                tally.Expect(Radius(answer.RootElement, "value") == wanted, $"opened again, component.fields gives the radius {radius}");
            }
            finally
            {
                session.Close(reopened.Document.Id);
            }

            // Given back to the prototype: the entity's own data without the field.
            Edit(session, document, "Reset field", new EditOp("components", Uid: light, Components: WithOwn(session, document, light, "PointLight", "radius", null)));
            edits++;
            using (var answer = Fields(session, document, light))
                tally.Expect(Radius(answer.RootElement, "value") == before, $"the field given back to the prototype, the radius is {before} again");
            var reset = Save(session, document, Out("radius-reset"));
            tally.Expect(!reset.Any(line => line.Trim() == $"radius: {radius}"), "and the save writes no radius of its own");
        }
        finally
        {
            for (var step = 0; step < edits; step++)
                Undo(session, document);
        }
    }

    private static JsonDocument Fields(EngineSession session, int document, int uid) =>
        Ask(session, "component.fields", $"{{\"doc\":{document},\"uid\":{uid}}}");

    /// <summary>The fields of one component in an answer of "component.fields".</summary>
    private static JsonElement? Component(JsonDocument answer, string name) => Component(answer.RootElement, name);

    private static JsonElement? Component(JsonElement answer, string name)
    {
        foreach (var component in answer.GetProperty("components").EnumerateArray())
        {
            if (component.GetProperty("name").GetString() == name)
                return component.GetProperty("fields");
        }

        return null;
    }

    /// <summary>
    /// The entity's components as "details" gives them, with one field of one component set,
    /// or with none taken out, as the editor's forms send them.
    /// </summary>
    private static string WithOwn(EngineSession session, int document, int uid, string name, string tag, JsonNode? value)
    {
        using var details = Ask(session, "details", $"{{\"doc\":{document},\"uids\":[{uid}]}}");
        var components = JsonNode.Parse(details.RootElement[0].GetProperty("components").GetString() ?? "[]")!.AsArray();
        var component = components.OfType<JsonObject>().FirstOrDefault(component => component["type"]?.GetValue<string>() == name);
        if (component == null)
        {
            component = new JsonObject { ["type"] = name };
            components.Add(component);
        }

        if (value == null)
            component.Remove(tag);
        else
            component[tag] = value;
        return components.ToJsonString();
    }
}
