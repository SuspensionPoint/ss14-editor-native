using System.IO.Compression;
using Robust.Shared.ContentPack;
using Robust.Shared.Utility;

namespace SS14.EditorKernel;

/// <summary>
/// Mounts a zip archive held in memory as engine resources. This is how the browser build gets its
/// prototypes: the page downloads one resource pack and hands the bytes to the kernel.
/// </summary>
public sealed class ZipContentRoot : IContentRoot, IDisposable
{
    private readonly ZipArchive _archive;
    private readonly Dictionary<string, ZipArchiveEntry> _entries = new(StringComparer.Ordinal);

    public ZipContentRoot(byte[] zip)
    {
        _archive = new ZipArchive(new MemoryStream(zip, writable: false), ZipArchiveMode.Read);
        foreach (var entry in _archive.Entries)
        {
            // Directory entries have an empty name.
            if (entry.Name.Length != 0)
                _entries[entry.FullName.Replace('\\', '/')] = entry;
        }
    }

    public int FileCount => _entries.Count;

    public void Mount()
    {
    }

    public bool TryGetFile(ResPath relPath, out Stream? stream)
    {
        if (!_entries.TryGetValue(Key(relPath), out var entry))
        {
            stream = null;
            return false;
        }

        // The engine reads prototype files from several threads on desktop, and a ZipArchive
        // serves one reader at a time, so hand out an independent copy of the entry's bytes.
        lock (_archive)
        {
            var copy = new MemoryStream(checked((int)entry.Length));
            using (var source = entry.Open())
                source.CopyTo(copy);
            copy.Position = 0;
            stream = copy;
        }

        return true;
    }

    public bool FileExists(ResPath relPath) => _entries.ContainsKey(Key(relPath));

    /// <summary>The full contents of an entry by its path inside the archive, or null if absent.</summary>
    public byte[]? ReadAllBytes(string entryPath)
    {
        if (!_entries.TryGetValue(entryPath, out var entry))
            return null;

        lock (_archive)
        {
            var buffer = new MemoryStream(checked((int)entry.Length));
            using (var source = entry.Open())
                source.CopyTo(buffer);
            return buffer.ToArray();
        }
    }

    public IEnumerable<ResPath> FindFiles(ResPath path)
    {
        var prefix = Key(path);
        if (prefix.Length != 0 && !prefix.EndsWith('/'))
            prefix += "/";

        foreach (var name in _entries.Keys)
        {
            if (name.StartsWith(prefix, StringComparison.Ordinal))
                yield return new ResPath(name);
        }
    }

    public IEnumerable<string> GetRelativeFilePaths() => _entries.Keys.Select(name => "/" + name);

    public void Dispose() => _archive.Dispose();

    private static string Key(ResPath path)
    {
        var text = path.ToRelativePath().ToString();
        return text == "." ? "" : text;
    }
}
