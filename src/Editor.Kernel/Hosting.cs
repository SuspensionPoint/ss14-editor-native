using System.Collections.Immutable;
using System.Reflection;
using Robust.Shared.ContentPack;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;
using Robust.Shared.Log;
using Robust.Shared.Prototypes;
using Robust.Shared.Reflection;
using Robust.Shared.Utility;
using Serilog.Events;

namespace SS14.EditorKernel;

/// <summary>
/// The real reflection manager, with automatic discovery of entity systems, entity console
/// commands and prototype kinds switched off. System and command discovery is what pulls in the
/// content's game logic. Prototype kinds are registered by hand instead (see EditorKernel.Boot).
/// </summary>
internal sealed class NoSystemDiscoveryReflection(IReflectionManager inner) : IReflectionManager
{
    // Matched by name because IEntityConsoleCommand is internal to the engine, as is the
    // EntityConsoleHost.DiscoverCommands switch that would otherwise be the clean way to do this.
    private static readonly HashSet<string> Hidden =
    [
        typeof(IEntitySystem).FullName!,
        typeof(IPrototype).FullName!,
        "Robust.Shared.Console.IEntityConsoleCommand",
    ];

    public IReflectionManager Inner => inner;

    public IEnumerable<Type> GetAllChildren<T>(bool inclusive = false) =>
        Hidden.Contains(typeof(T).FullName!) ? [] : inner.GetAllChildren<T>(inclusive);

    public IEnumerable<Type> GetAllChildren(Type baseType, bool inclusive = false) =>
        Hidden.Contains(baseType.FullName!) ? [] : inner.GetAllChildren(baseType, inclusive);

    public IReadOnlyList<Assembly> Assemblies => inner.Assemblies;
    public Type? GetType(string name) => inner.GetType(name);
    public Type LooseGetType(string name) => inner.LooseGetType(name);
    public bool TryLooseGetType(string name, out Type? type) => inner.TryLooseGetType(name, out type);
    public IEnumerable<Type> FindTypesWithAttribute<T>() where T : Attribute => inner.FindTypesWithAttribute<T>();
    public IEnumerable<Type> FindTypesWithAttribute(Type attributeType) => inner.FindTypesWithAttribute(attributeType);
    public void LoadAssemblies(IEnumerable<Assembly> assemblies) => inner.LoadAssemblies(assemblies);
    public void LoadAssemblies(params Assembly[] args) => inner.LoadAssemblies(args);

    public event EventHandler<ReflectionUpdateEventArgs>? OnAssemblyAdded
    {
        add => inner.OnAssemblyAdded += value;
        remove => inner.OnAssemblyAdded -= value;
    }

    public bool TryParseEnumReference(string reference, out Enum? @enum, bool shouldThrow = true) =>
        inner.TryParseEnumReference(reference, out @enum, shouldThrow);

    public string GetEnumReference(Enum @enum) => inner.GetEnumReference(@enum);
    public Type? YamlTypeTagLookup(Type baseType, string typeName) => inner.YamlTypeTagLookup(baseType, typeName);
    public IEnumerable<Type> FindAllTypes() => inner.FindAllTypes();
    public void Initialize() => inner.Initialize();
    // Only some engines have these two (src/Directory.Build.props).
#if ENGINE_REFLECTION_IS_ATTRIBUTE_DEFINED
    public bool IsAttributeDefined(Type type, Type attribute) => inner.IsAttributeDefined(type, attribute);
#endif
#if ENGINE_REFLECTION_ATTRIBUTE_SET
    public ImmutableHashSet<Type> FindTypesWithAttributeSet<T>() => inner.FindTypesWithAttributeSet<T>();
#endif
}

/// <summary>Swaps out services the harness registers that cannot be constructed in a browser.</summary>
internal static class BrowserServices
{
    public static void Replace(IDependencyCollection deps)
    {
        // HttpClientHolder's constructor configures a socket connect callback, which throws in a
        // browser. Nothing in the kernel makes HTTP requests, so register an instance whose
        // constructor never ran. The type is internal, hence the lookup by name.
        var holderType = typeof(Robust.Shared.Network.IHttpClientHolder).Assembly
            .GetType("Robust.Shared.Network.HttpClientHolder", throwOnError: true)!;
        var holder = System.Runtime.CompilerServices.RuntimeHelpers.GetUninitializedObject(holderType);
        deps.RegisterInstance(holderType, holder, overwrite: true);
        deps.RegisterInstance<Robust.Shared.Network.IHttpClientHolder>(holder, overwrite: true);

        // The harness initializes the real NetManager as a server, which generates auth keys with
        // the native libsodium library. There is no network in the kernel, so stand in a mock that
        // still reports "server" to the shared code that branches on it.
        var net = new Moq.Mock<Robust.Shared.Network.IServerNetManager>();
        net.SetupGet(n => n.IsServer).Returns(true);
        net.SetupGet(n => n.IsClient).Returns(false);
        deps.RegisterInstance<Robust.Shared.Network.IServerNetManager>(net.Object, overwrite: true);
        deps.RegisterInstance<Robust.Shared.Network.INetManager>(net.Object, overwrite: true);
    }
}

