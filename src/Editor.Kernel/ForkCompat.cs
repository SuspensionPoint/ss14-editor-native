#if FORK_SERVER_ATMOSPHERE
// A grid's atmosphere where a fork keeps it in Content.Server (src/Directory.Build.props).
global using TileAtmosphere = Content.Server.Atmos.TileAtmosphere;
global using GridAtmosphereComponent = Content.Server.Atmos.Components.GridAtmosphereComponent;
#endif
#if FORK_SEALED_DEVICE_LINK_SYSTEM
// The device link components where the Space Wizards' refactor put them (src/Directory.Build.props).
global using Content.Shared.DeviceLinking.Components;
#endif
#if FORK_LINKED_DEVICE_NETWORK
// A device list and the component on a device that names the lists it is on, where the
// refactor put them (src/Directory.Build.props).
global using Content.Shared.DeviceConfigurator.Components;
global using ListedDeviceComponent = Content.Shared.DeviceConfigurator.Components.LinkedDeviceNetworkComponent;
#else
global using ListedDeviceComponent = Content.Shared.DeviceNetwork.Components.DeviceNetworkComponent;
#endif
using System.Diagnostics.CodeAnalysis;
using Robust.Shared.GameObjects;
using Robust.Shared.Prototypes;
#if !ENGINE_MAP_LOADER_TEXT
using Robust.Shared.EntitySerialization;
using Robust.Shared.EntitySerialization.Systems;
using Robust.Shared.Map;
using Robust.Shared.Map.Components;
using Robust.Shared.Serialization;
using YamlDotNet.Core;
using YamlDotNet.RepresentationModel;
#endif

namespace SS14.EditorKernel;

/// <summary>
/// Small differences between the forks' code that the kernel smooths over, so the same lines build
/// against each fork (src/Directory.Build.props). The rest is handled by the constants defined
/// there.
/// </summary>
internal static class ForkCompat
{
    /// <summary>A prototype id, as a fork keeps it: plain text, or a ProtoId as Starlight does.</summary>
    public static string IdOf(string id) => id;

    public static string IdOf<T>(ProtoId<T> id) where T : class, IPrototype => id.Id;

    /// <summary>The component of a device that names the device lists it is on (ListedDeviceComponent).</summary>
#if FORK_LINKED_DEVICE_NETWORK
    public const string ListedDevice = "LinkedDeviceNetwork";
#else
    public const string ListedDevice = "DeviceNetwork";
#endif

#if !ENGINE_PROTOTYPE_TRYCOMP
    /// <summary>EntityPrototype.TryComp for an engine without it, by its older name.</summary>
    public static bool TryComp<T>(this EntityPrototype prototype, [NotNullWhen(true)] out T? component, IComponentFactory factory) where T : IComponent, new() =>
        prototype.TryGetComponent(out component, factory);
#endif

#if !ENGINE_MAP_LOADER_TEXT
    // The map loader's text overloads for an engine that reads and writes map files by path
    // alone: the same steps, through what it has (a stream to load, the serialized data to save).

    public static bool TryLoadGeneric(this MapLoaderSystem loader, TextReader reader, string source, [NotNullWhen(true)] out LoadResult? result, MapLoadOptions? options = null)
    {
        using var stream = new MemoryStream(System.Text.Encoding.UTF8.GetBytes(reader.ReadToEnd()));
        return loader.TryLoadGeneric(stream, source, out result, options);
    }

    public static bool TryLoadGrid(this MapLoaderSystem loader, MapId map, TextReader reader, string source, [NotNullWhen(true)] out Entity<MapGridComponent>? grid, DeserializationOptions? options = null)
    {
        var load = new MapLoadOptions
        {
            MergeMap = map,
            DeserializationOptions = options ?? DeserializationOptions.Default,
            ExpectedCategory = FileCategory.Grid,
        };

        grid = null;
        if (!loader.TryLoadGeneric(reader, source, out var result, load))
            return false;

        if (result.Grids.Count == 1)
        {
            grid = result.Grids.Single();
            return true;
        }

        loader.Delete(result);
        return false;
    }

    public static bool TrySaveMap(this MapLoaderSystem loader, EntityUid map, TextWriter target, SerializationOptions? options = null) =>
        Save(loader, [map], target, (options ?? SerializationOptions.Default) with { Category = FileCategory.Map }, out var category) && category == FileCategory.Map;

    public static bool TrySaveGrid(this MapLoaderSystem loader, EntityUid grid, TextWriter target, SerializationOptions? options = null) =>
        Save(loader, [grid], target, (options ?? SerializationOptions.Default) with { Category = FileCategory.Grid }, out var category) && category == FileCategory.Grid;

    public static bool TrySaveGeneric(this MapLoaderSystem loader, HashSet<EntityUid> entities, TextWriter target, out FileCategory category, SerializationOptions? options = null)
    {
        category = FileCategory.Unknown;
        return entities.Count > 0 && Save(loader, entities, target, options ?? SerializationOptions.Default, out category);
    }

    /// <summary>Serializes the entities and writes them as the engine writes a map file. Nothing is written for a category other than the one asked for.</summary>
    private static bool Save(MapLoaderSystem loader, HashSet<EntityUid> entities, TextWriter target, SerializationOptions options, out FileCategory category)
    {
        var (data, written) = loader.SerializeEntitiesRecursive(entities, options);
        category = written;
        if (options.Category != FileCategory.Unknown && written != options.Category)
            return false;

        var stream = new YamlStream { new YamlDocument(data.ToYaml()) };
        stream.Save(new YamlMappingFix(new Emitter(target)), false);
        return true;
    }
#endif
}
