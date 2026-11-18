using System.IO.Compression;
using Robust.Shared.ContentPack;
using Robust.Shared.Serialization.Markdown;

namespace SS14.EditorKernel;

/// <summary>
/// The single file the browser kernel boots from: a zip holding the prototype cache and the few
/// loose resource files the kernel reads (currently only the map migration table).
/// </summary>
public static class ResourcePack
{
    public const string CacheEntry = "prototypes.cache";

    /// <summary>Prototype directories inside a Resources folder that the kernel loads.</summary>
    private static readonly string[] PrototypeDirectories = ["EnginePrototypes", "Prototypes"];

    /// <summary>Loose files copied into the pack as they are.</summary>
    private static readonly string[] LooseFiles = ["migration.yml"];

    /// <summary>
    /// Folders copied into the pack as they are: the fork's lists of partial and ignored
    /// prototype folders (see ForkPrototypeRules).
    /// </summary>
    private static readonly string[] LooseDirectories = ["PartialPrototypes", "IgnoredPrototypes"];

    /// <summary>
    /// The engine's Resources folder for a fork: its RobustToolbox's, or, when the checkout has
    /// not set up the engine's submodule, the copy beside an engine side the editor built for it
    /// (native/editor-core/src/forks/engines.rs).
    /// </summary>
    public static string EngineResources(string forkRepo)
    {
        var own = Path.Combine(forkRepo, "RobustToolbox", "Resources");
        if (Directory.Exists(Path.Combine(own, "EnginePrototypes")))
            return own;
        var here = Path.GetDirectoryName(typeof(ResourcePack).Assembly.Location);
        var beside = string.IsNullOrEmpty(here) ? null : Path.Combine(here, "RobustToolbox", "Resources");
        return beside != null && Directory.Exists(beside) ? beside : own;
    }

    /// <summary>
    /// Builds a pack from a fork checkout by running the engine's own YAML parser over every
    /// prototype file. Returns the zip and the number of prototype files it holds.
    /// </summary>
    public static (byte[] Zip, int PrototypeFiles) Build(string forkRepo)
    {
        var resourceRoots = new[]
        {
            EngineResources(forkRepo),
            Path.Combine(forkRepo, "Resources"),
        };

        // Later roots win, matching the order the kernel mounts them in.
        var files = new SortedDictionary<string, string>(StringComparer.Ordinal);
        foreach (var root in resourceRoots)
        {
            foreach (var directory in PrototypeDirectories)
            {
                var full = Path.Combine(root, directory);
                if (!Directory.Exists(full))
                    continue;

                foreach (var file in Directory.EnumerateFiles(full, "*.yml", SearchOption.AllDirectories))
                {
                    // The engine skips dot-files when it loads a prototype directory.
                    if (Path.GetFileName(file).StartsWith('.'))
                        continue;
                    files["/" + Path.GetRelativePath(root, file).Replace('\\', '/')] = file;
                }
            }
        }

        var parsed = new List<(string, IReadOnlyList<DataNodeDocument>)>(files.Count);
        foreach (var (rooted, file) in files)
        {
            using var reader = new StreamReader(file);
            parsed.Add((rooted, DataNodeParser.ParseYamlStream(reader).ToArray()));
        }

        var cache = PrototypeCache.Build(parsed);

        var output = new MemoryStream();
        using (var zip = new ZipArchive(output, ZipArchiveMode.Create, leaveOpen: true))
        {
            WriteEntry(zip, CacheEntry, cache);
            foreach (var loose in LooseFiles)
            {
                var source = resourceRoots.Select(root => Path.Combine(root, loose)).LastOrDefault(File.Exists);
                if (source != null)
                    WriteEntry(zip, loose, File.ReadAllBytes(source));
            }

            var inDirectories = new SortedDictionary<string, string>(StringComparer.Ordinal);
            foreach (var root in resourceRoots)
            {
                foreach (var directory in LooseDirectories.Select(name => Path.Combine(root, name)).Where(Directory.Exists))
                {
                    foreach (var file in Directory.EnumerateFiles(directory, "*", SearchOption.AllDirectories))
                        inDirectories[Path.GetRelativePath(root, file).Replace('\\', '/')] = file;
                }
            }

            foreach (var (name, file) in inDirectories)
                WriteEntry(zip, name, File.ReadAllBytes(file));
        }

        return (output.ToArray(), files.Count);
    }

    /// <summary>
    /// Opens a pack. Returns the content root for its loose files, and the prototype cache if
    /// the pack has one (a pack may instead carry prototype YAML files directly).
    /// </summary>
    public static (ZipContentRoot Root, PrototypeCache? Cache) Open(byte[] zip)
    {
        var root = new ZipContentRoot(zip);
        var cacheBytes = root.ReadAllBytes(CacheEntry);
        return (root, cacheBytes == null ? null : PrototypeCache.Load(cacheBytes));
    }

    private static void WriteEntry(ZipArchive zip, string name, byte[] contents)
    {
        var entry = zip.CreateEntry(name, CompressionLevel.SmallestSize);
        using var stream = entry.Open();
        stream.Write(contents);
    }
}
