using Robust.Shared.ContentPack;
using Robust.Shared.Log;
using Robust.Shared.Prototypes;
using Robust.Shared.Serialization.Markdown;
using Robust.Shared.Serialization.Markdown.Sequence;
using Robust.Shared.Serialization.Markdown.Value;
using Robust.Shared.Utility;
using Serilog.Events;

namespace SS14.EditorKernel;

// Pieces both engine hosts use: the document kernel (server side) and the view kernel (client
// side). This file has no server or client references, so each project compiles it in.

/// <summary>Read-only directory mount. The engine's own DirLoader is internal.</summary>
public sealed class DirContentRoot(string directory) : IContentRoot
{
    private readonly string _root = Path.GetFullPath(directory).TrimEnd(Path.DirectorySeparatorChar);

    public void Mount()
    {
        if (!Directory.Exists(_root))
            throw new DirectoryNotFoundException(_root);
    }

    public bool TryGetFile(ResPath relPath, out Stream? stream)
    {
        var path = GetPath(relPath);
        if (!File.Exists(path))
        {
            stream = null;
            return false;
        }

        stream = File.OpenRead(path);
        return true;
    }

    public bool FileExists(ResPath relPath) => File.Exists(GetPath(relPath));

    public IEnumerable<ResPath> FindFiles(ResPath path)
    {
        var full = GetPath(path);
        if (!Directory.Exists(full))
            yield break;

        foreach (var file in Directory.EnumerateFiles(full, "*", SearchOption.AllDirectories))
            yield return ToResPath(file);
    }

    public IEnumerable<string> GetRelativeFilePaths()
    {
        foreach (var file in Directory.EnumerateFiles(_root, "*", SearchOption.AllDirectories))
            yield return ToResPath(file).ToRootedPath().ToString();
    }

    // The engine's ResPath system-path helpers bake the path separator in at compile time
    // (#if WINDOWS). An engine built on Windows therefore produces backslashes even when it is
    // running in a browser, so convert with the separator of the platform we are running on.
    private ResPath ToResPath(string file) =>
        new(file[_root.Length..].Replace(Path.DirectorySeparatorChar, '/'));

    private string GetPath(ResPath relPath) =>
        Path.Combine(_root, relPath.ToRelativePath().ToString().Replace('/', Path.DirectorySeparatorChar));
}

/// <summary>Keeps engine warnings and errors so callers can show them, instead of throwing.</summary>
internal sealed class CollectingLogHandler : ILogHandler
{
    private const int MaxEntries = 500;
    private readonly List<KernelLogEntry> _entries = new();

    /// <summary>When set, is given every message at any level as it is logged, for finding out where a start-up stalls.</summary>
    public Action<string>? Trace;

    public void Log(string sawmillName, LogEvent message)
    {
        Trace?.Invoke($"{message.Level} {sawmillName}: {message.RenderMessage()}");
        if (message.Level < LogEventLevel.Warning)
            return;

        var text = message.RenderMessage();
        if (message.Exception != null)
            text += $" ({message.Exception.GetType().Name}: {message.Exception.Message})";

        lock (_entries)
        {
            if (_entries.Count < MaxEntries)
                _entries.Add(new KernelLogEntry(message.Level.ToString(), sawmillName, text));
        }
    }

    public IReadOnlyList<KernelLogEntry> Drain()
    {
        lock (_entries)
        {
            var copy = _entries.ToArray();
            _entries.Clear();
            return copy;
        }
    }
}

/// <summary>
/// The fork's two lists that change how prototype files are read: folders whose prototypes are
/// made abstract, and folders of "partial" prototypes that amend prototypes defined elsewhere.
/// The game applies them in Content.Shared's entry point (IgnorePrototypes and
/// PartialPrototypes), which neither engine host here runs, so this mirrors it. It has to
/// happen before the prototype folders are read.
/// </summary>
public static class ForkPrototypeRules
{
    public static void Apply(IPrototypeManager prototypes, IResourceManager resources)
    {
        foreach (var path in ReadList(resources, new ResPath("/IgnoredPrototypes/")))
        {
            if (string.IsNullOrEmpty(path.Extension))
                prototypes.AbstractDirectory(path);
            else
                prototypes.AbstractFile(path);
        }

#if ENGINE_PARTIAL_PROTOTYPES
        // Only an engine with partial prototypes can apply them (src/Directory.Build.props). A
        // fork on an engine without them has none.
        var index = 0;
        foreach (var path in ReadList(resources, new ResPath("/PartialPrototypes/")))
        {
            if (string.IsNullOrEmpty(path.Extension))
                prototypes.PartialDirectory(path, index++);
            else
                prototypes.PartialFile(path, index++);
        }
#endif
    }

    private static IEnumerable<ResPath> ReadList(IResourceManager resources, ResPath directory)
    {
        foreach (var file in resources.ContentFindFiles(directory).OrderBy(path => path.CanonPath))
        {
            if (!resources.TryContentFileRead(file, out var stream))
                continue;

            using var reader = new StreamReader(stream, EncodingHelpers.UTF8);
            if (DataNodeParser.ParseYamlStream(reader).FirstOrDefault()?.Root is not SequenceDataNode list)
                continue;

            foreach (var node in list.Sequence)
            {
                if (node is ValueDataNode value)
                    yield return new ResPath(value.Value);
            }
        }
    }
}

/// <summary>
/// The content assemblies each engine host loads, as the game does: Content.Shared with
/// Content.Server or Content.Client, and a fork's modules beside them, as Goob Station's
/// (Content.Goobstation.Shared, Content.Goobstation.Common, Content.Common, and
/// Content.Goobstation.Server or Content.Goobstation.Client), whose components and prototypes a
/// map uses as much as the rest. The game loads each Content assembly of its folder that has an
/// entry point. The engine side's folder holds both sides', told apart by the engine they refer
/// to. A fork with no modules (Starlight) gets the two it always had.
/// </summary>
public static class ContentAssemblies
{
    public const string ServerEngine = "Robust.Server";
    public const string ClientEngine = "Robust.Client";

    public static System.Reflection.Assembly[] ForServer(System.Reflection.Assembly shared, System.Reflection.Assembly server) =>
        [shared, server, .. Side(server, ClientEngine).Where(assembly => assembly != shared && assembly != server)];

    public static System.Reflection.Assembly[] ForClient(System.Reflection.Assembly shared, System.Reflection.Assembly client) =>
        [shared, client, .. Side(client, ServerEngine).Where(assembly => assembly != shared && assembly != client)];

    /// <summary>
    /// The prototype kinds only the other side has, which this side ignores, as the game's entry
    /// points each say by hand ("newsEntry" on Goob Station's server). A file of prototypes with
    /// a kind neither known nor ignored is not read at all, with every prototype in it.
    /// </summary>
    public static string[] OtherSideKinds(System.Reflection.Assembly anchor, string thisEngine, string otherEngine)
    {
        var mine = Side(anchor, otherEngine).SelectMany(KindNames).ToHashSet();
        return Side(anchor, thisEngine).SelectMany(KindNames).Where(kind => !mine.Contains(kind)).Distinct().Order(StringComparer.Ordinal).ToArray();
    }

    /// <summary>The Content assemblies with an entry point beside this one, but those that refer to the given engine.</summary>
    private static IEnumerable<System.Reflection.Assembly> Side(System.Reflection.Assembly anchor, string notReferring)
    {
        // In a browser no assembly has a folder, and the fork there is Starlight, which has no modules.
        var folder = Path.GetDirectoryName(anchor.Location);
        if (string.IsNullOrEmpty(folder))
            yield break;

        foreach (var file in Directory.GetFiles(folder, "Content.*.dll").Order(StringComparer.Ordinal))
        {
            System.Reflection.Assembly assembly;
            try
            {
                assembly = System.Reflection.Assembly.Load(System.Reflection.AssemblyName.GetAssemblyName(file));
            }
            catch (Exception)
            {
                continue;
            }

            if (!assembly.GetReferencedAssemblies().Any(name => name.Name == notReferring) && TypesOf(assembly).Any(type => !type.IsAbstract && type.IsSubclassOf(typeof(GameShared))))
                yield return assembly;
        }
    }

    private static IEnumerable<string> KindNames(System.Reflection.Assembly assembly) =>
        TypesOf(assembly)
            .Where(type => !type.IsAbstract && typeof(IPrototype).IsAssignableFrom(type))
            .Select(type => Attribute.GetCustomAttribute(type, typeof(PrototypeAttribute)) is PrototypeAttribute attribute ? attribute.Type ?? KindName(type.Name) : null)
            .OfType<string>();

    /// <summary>
    /// The name the engine gives a kind of prototype with none of its own: "FooBarPrototype" is
    /// "fooBar". As PrototypeUtility.CalculatePrototypeName, which older engines do not have.
    /// </summary>
    private static string KindName(string type)
    {
        const string ending = "Prototype";
        var name = type.EndsWith(ending, StringComparison.Ordinal) ? type[..^ending.Length] : type;
        return name.Length == 0 ? name : char.ToLowerInvariant(name[0]) + name[1..];
    }

    private static Type[] TypesOf(System.Reflection.Assembly assembly)
    {
        try
        {
            return assembly.GetTypes();
        }
        catch (System.Reflection.ReflectionTypeLoadException error)
        {
            return error.Types.OfType<Type>().ToArray();
        }
    }
}
