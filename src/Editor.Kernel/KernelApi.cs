using System.Text.Json;
using Robust.Shared.Prototypes;

namespace SS14.EditorKernel;

/// <summary>
/// The kernel's operations as named commands with JSON arguments and results. Every host (the
/// local server, the browser worker) exposes exactly this, so the interface talks to the engine
/// the same way wherever it runs.
/// </summary>
public sealed class KernelApi(EditorKernel kernel)
{
    public static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);

    /// <summary>Runs one command. Must be called on the thread the kernel was booted on.</summary>
    public object? Handle(string op, JsonElement args)
    {
        var documents = kernel.Documents;
        switch (op)
        {
            case "info":
                return new KernelInfo(
                    kernel.GetPrototypeSummary(),
                    kernel.BootTimings.Select(t => new BootTiming(t.Phase, t.Milliseconds)).ToArray(),
                    documents.OpenDocuments);
            case "tiles":
                return kernel.GetTiles();
            case "prototypes":
                return kernel.GetEntityPrototypes();
            case "decals":
                return kernel.GetDecalPrototypes();
            case "validate":
                return kernel.Validate(Text(args, "yaml"), Text(args, "source", "map.yml"));
            case "resave":
                return kernel.Resave(Text(args, "yaml"), Text(args, "source", "map.yml"));
            case "open":
                return documents.Open(Text(args, "yaml"), Text(args, "source", "map.yml"));
            case "new":
                return documents.New(Text(args, "category", "Map"), Text(args, "source", "untitled.yml"));
            case "close":
                documents.Close(Int(args, "doc"));
                return null;
            case "scene":
                return documents.GetScene(Int(args, "doc"));
            case "details":
            {
                var uids = args.GetProperty("uids").Deserialize<List<int>>(Json) ?? [];
                return documents.Describe(Int(args, "doc"), uids);
            }
            case "component.fields":
            {
                var names = args.TryGetProperty("names", out var named) && named.ValueKind == JsonValueKind.Array ? named.Deserialize<List<string>>(Json) : null;
                var known = args.TryGetProperty("known", out var held) && held.ValueKind == JsonValueKind.Array ? held.Deserialize<List<string>>(Json) ?? [] : [];
                return documents.Fields(Int(args, "doc"), Int(args, "uid"), names, known.ToHashSet());
            }
            case "ports":
                return kernel.GetPorts();
            case "ports.of":
            {
                var uids = args.GetProperty("uids").Deserialize<List<int>>(Json) ?? [];
                return documents.Ports(Int(args, "doc"), uids);
            }
            case "links":
                return documents.Links(Int(args, "doc"));
            case "air":
            {
                var grids = args.TryGetProperty("grids", out var named) && named.ValueKind == JsonValueKind.Array ? named.Deserialize<List<int>>(Json) : null;
                return documents.Air(Int(args, "doc"), grids?.ToHashSet());
            }
            case "air.fix.preview":
            {
                var grids = args.TryGetProperty("grids", out var named) && named.ValueKind == JsonValueKind.Array ? named.Deserialize<List<int>>(Json) : null;
                return documents.AirFixPreview(Int(args, "doc"), grids?.ToHashSet());
            }
            case "containers":
            {
                var uids = args.GetProperty("uids").Deserialize<List<int>>(Json) ?? [];
                return documents.Containers(Int(args, "doc"), uids);
            }
            case "copy":
            {
                var uids = args.GetProperty("uids").Deserialize<List<int>>(Json) ?? [];
                return documents.Copy(Int(args, "doc"), uids);
            }
            case "edit":
            {
                var ops = args.GetProperty("ops").Deserialize<List<EditOp>>(Json) ?? [];
                return documents.Edit(Int(args, "doc"), Text(args, "label", "Edit"), ops);
            }
            case "undo":
                return documents.Undo(Int(args, "doc"));
            case "redo":
                return documents.Redo(Int(args, "doc"));
            case "save":
                return documents.Save(Int(args, "doc"));
            default:
                throw new KernelException($"Unknown engine command '{op}'.");
        }
    }

    /// <summary>Runs one command given as JSON text and returns the result as JSON text.</summary>
    public string HandleJson(string op, string argsJson)
    {
        using var parsed = JsonDocument.Parse(string.IsNullOrEmpty(argsJson) ? "{}" : argsJson);
        return JsonSerializer.Serialize(Handle(op, parsed.RootElement), Json);
    }

    private static string Text(JsonElement args, string name, string? fallback = null)
    {
        if (args.ValueKind == JsonValueKind.Object && args.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String)
            return value.GetString()!;

        return fallback ?? throw new KernelException($"Missing argument '{name}'.");
    }

    private static int Int(JsonElement args, string name)
    {
        if (args.ValueKind == JsonValueKind.Object && args.TryGetProperty(name, out var value) && value.TryGetInt32(out var number))
            return number;

        throw new KernelException($"Missing argument '{name}'.");
    }
}

public sealed record BootTiming(string Phase, long Milliseconds);

public sealed record KernelInfo(PrototypeSummary Summary, IReadOnlyList<BootTiming> Timings, IReadOnlyList<int> OpenDocuments);
