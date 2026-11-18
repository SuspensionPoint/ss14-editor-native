using System.Buffers.Binary;
using System.IO.Compression;
using System.Text;
using Robust.Shared.ContentPack;
using Robust.Shared.Utility;
using SS14.EditorKernel;

namespace SS14.EditorViewKernel;

/// <summary>
/// The file a browser's view kernel starts from, next to the document kernel's pack (which holds
/// the prototypes). A client reads far more than a server does: translations, shaders, key
/// bindings, interface themes, and the description of every sprite it shows.
///
/// What goes in is found by watching, not by listing. The pack is built by starting the view
/// kernel on a desktop and noting every file the engine opens. On top of that it carries
/// - the description (meta.json) of every sprite folder, since those are read when something
///   that uses them is first placed, and
/// - the size of every image, but no image.
///
/// The view kernel never looks at a pixel. It records which image a sprite uses and where it goes,
/// and the page draws the real image. The engine does open the images it refers to, though, to
/// lay out its texture sheets, so each is answered with a blank image of the right size
/// (<see cref="BlankImageRoot"/>). Fonts are answered with empty files for the same reason.
/// </summary>
public static class ViewPack
{
    public const string ImageIndexEntry = "images.index";

    private static readonly string[] PrototypeDirectories = ["/EnginePrototypes/", "/Prototypes/"];

    /// <param name="forkRepo">The fork checkout: its Resources and RobustToolbox/Resources are read.</param>
    /// <param name="opened">Rooted resource paths the engine opened while it was watched.</param>
    public static (byte[] Zip, int Files, int Images) Build(string forkRepo, IEnumerable<string> opened)
    {
        // Later roots win, matching the order the engine mounts them in.
        var roots = new[]
        {
            ResourcePack.EngineResources(forkRepo),
            Path.Combine(forkRepo, "Resources"),
        };
        string? Locate(string rooted) =>
            roots.Select(root => Path.Combine(root, rooted.TrimStart('/').Replace('/', Path.DirectorySeparatorChar))).LastOrDefault(File.Exists);

        var files = new SortedDictionary<string, byte[]>(StringComparer.Ordinal);
        foreach (var rooted in opened)
        {
            if (rooted.EndsWith(".png", StringComparison.OrdinalIgnoreCase) || PrototypeDirectories.Any(dir => rooted.StartsWith(dir, StringComparison.Ordinal)))
                continue;

            var source = Locate(rooted);
            if (source == null)
                continue;

            var font = rooted.EndsWith(".ttf", StringComparison.OrdinalIgnoreCase) || rooted.EndsWith(".otf", StringComparison.OrdinalIgnoreCase);
            files[rooted] = font ? [] : File.ReadAllBytes(source);
        }

        // Every image's size, and every sprite folder's description.
        var images = new SortedDictionary<string, SortedDictionary<string, (int Width, int Height)>>(StringComparer.Ordinal);
        var imageCount = 0;
        var header = new byte[24];
        foreach (var root in roots)
        {
            foreach (var file in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories))
            {
                var rooted = "/" + Path.GetRelativePath(root, file).Replace('\\', '/');
                if (rooted.EndsWith(".rsi/meta.json", StringComparison.Ordinal))
                {
                    files[rooted] = File.ReadAllBytes(file);
                    continue;
                }

                if (!rooted.EndsWith(".png", StringComparison.OrdinalIgnoreCase))
                    continue;

                using (var stream = File.OpenRead(file))
                {
                    // A PNG starts with an 8-byte signature and the IHDR chunk: length, "IHDR", width, height.
                    if (stream.Read(header) != header.Length || header[12] != 'I' || header[13] != 'H')
                        continue;
                }

                var slash = rooted.LastIndexOf('/');
                var directory = rooted[..slash];
                if (!images.TryGetValue(directory, out var inDirectory))
                    images[directory] = inDirectory = new SortedDictionary<string, (int, int)>(StringComparer.Ordinal);
                if (!inDirectory.ContainsKey(rooted[(slash + 1)..]))
                    imageCount++;
                inDirectory[rooted[(slash + 1)..]] = (BinaryPrimitives.ReadInt32BigEndian(header.AsSpan(16)), BinaryPrimitives.ReadInt32BigEndian(header.AsSpan(20)));
            }
        }

        // One line per folder: the folder, then name:width:height for each image in it.
        var index = new StringBuilder();
        foreach (var (directory, inDirectory) in images)
        {
            index.Append(directory);
            foreach (var (name, size) in inDirectory)
                index.Append('\t').Append(name).Append(':').Append(size.Width).Append(':').Append(size.Height);
            index.Append('\n');
        }

        var output = new MemoryStream();
        using (var zip = new ZipArchive(output, ZipArchiveMode.Create, leaveOpen: true))
        {
            Write(zip, ImageIndexEntry, Encoding.UTF8.GetBytes(index.ToString()));
            foreach (var (rooted, contents) in files)
                Write(zip, rooted.TrimStart('/'), contents);
        }

        return (output.ToArray(), files.Count, imageCount);
    }

    /// <summary>Opens a pack as the content roots to start a view kernel with.</summary>
    public static IContentRoot[] Open(byte[] zip)
    {
        var root = new ZipContentRoot(zip);
        var index = root.ReadAllBytes(ImageIndexEntry) ?? throw new InvalidDataException("The view pack has no image index.");
        return [new BlankImageRoot(Encoding.UTF8.GetString(index)), root];
    }

    private static void Write(ZipArchive zip, string name, byte[] contents)
    {
        var entry = zip.CreateEntry(name, CompressionLevel.SmallestSize);
        using var stream = entry.Open();
        stream.Write(contents);
    }
}

/// <summary>Notes which files the engine opens, for building a <see cref="ViewPack"/>.</summary>
public sealed class TracingContentRoot(IContentRoot inner, HashSet<string> opened) : IContentRoot
{
    public void Mount() => inner.Mount();

    public bool TryGetFile(ResPath relPath, out Stream? stream)
    {
        if (!inner.TryGetFile(relPath, out stream))
            return false;

        lock (opened)
            opened.Add(relPath.ToRootedPath().ToString());
        return true;
    }

    public bool FileExists(ResPath relPath) => inner.FileExists(relPath);
    public IEnumerable<ResPath> FindFiles(ResPath path) => inner.FindFiles(path);
    public IEnumerable<string> GetRelativeFilePaths() => inner.GetRelativeFilePaths();
}

/// <summary>
/// Answers for every image in the game's resources with a blank image of the same size, made
/// when asked for. See <see cref="ViewPack"/> for why that is enough.
/// </summary>
public sealed class BlankImageRoot : IContentRoot
{
    private readonly Dictionary<string, Dictionary<string, (int Width, int Height)>> _images = new(StringComparer.Ordinal);
    private readonly Dictionary<(int, int), byte[]> _blank = new();

    public int Count { get; }

    public BlankImageRoot(string index)
    {
        foreach (var line in index.Split('\n', StringSplitOptions.RemoveEmptyEntries))
        {
            var parts = line.Split('\t');
            var inDirectory = new Dictionary<string, (int, int)>(parts.Length - 1, StringComparer.Ordinal);
            for (var i = 1; i < parts.Length; i++)
            {
                // Names may contain colons in principle; the two sizes are always last.
                var height = parts[i].LastIndexOf(':');
                var width = parts[i].LastIndexOf(':', height - 1);
                inDirectory[parts[i][..width]] = (int.Parse(parts[i].AsSpan(width + 1, height - width - 1)), int.Parse(parts[i].AsSpan(height + 1)));
            }

            _images[parts[0]] = inDirectory;
            Count += inDirectory.Count;
        }
    }

    public void Mount()
    {
    }

    private bool TryFind(ResPath path, out (int Width, int Height) size)
    {
        var rooted = path.ToRootedPath().ToString();
        var slash = rooted.LastIndexOf('/');
        size = default;
        return _images.TryGetValue(rooted[..slash], out var inDirectory) && inDirectory.TryGetValue(rooted[(slash + 1)..], out size);
    }

    public bool TryGetFile(ResPath relPath, out Stream? stream)
    {
        if (!TryFind(relPath, out var size))
        {
            stream = null;
            return false;
        }

        if (!_blank.TryGetValue(size, out var png))
            _blank[size] = png = BlankPng(size.Width, size.Height);
        stream = new MemoryStream(png, writable: false);
        return true;
    }

    public bool FileExists(ResPath relPath) => TryFind(relPath, out _);

    public IEnumerable<ResPath> FindFiles(ResPath path)
    {
        var prefix = path.ToRootedPath().ToString().TrimEnd('/');
        foreach (var (directory, inDirectory) in _images)
        {
            if (directory != prefix && !directory.StartsWith(prefix + "/", StringComparison.Ordinal))
                continue;

            foreach (var name in inDirectory.Keys)
                yield return new ResPath($"{directory}/{name}").ToRelativePath();
        }
    }

    public IEnumerable<string> GetRelativeFilePaths() =>
        _images.SelectMany(directory => directory.Value.Keys.Select(name => $"{directory.Key}/{name}"));

    /// <summary>
    /// A fully transparent image. One bit per pixel with "black is transparent", which is the
    /// least a decoder can be given to chew on.
    /// </summary>
    public static byte[] BlankPng(int width, int height)
    {
        var output = new MemoryStream();
        output.Write([0x89, (byte)'P', (byte)'N', (byte)'G', 0x0D, 0x0A, 0x1A, 0x0A]);

        var header = new byte[13];
        BinaryPrimitives.WriteInt32BigEndian(header, width);
        BinaryPrimitives.WriteInt32BigEndian(header.AsSpan(4), height);
        header[8] = 1; // bits per sample
        header[9] = 0; // greyscale
        Chunk(output, "IHDR", header);
        // The grey level that stands for "transparent": 0, which every pixel is.
        Chunk(output, "tRNS", [0, 0]);

        var rows = new MemoryStream();
        using (var zlib = new ZLibStream(rows, CompressionLevel.Optimal, leaveOpen: true))
        {
            // Each row: a filter byte (none) and the pixels, all zero.
            var row = new byte[1 + (width + 7) / 8];
            for (var y = 0; y < height; y++)
                zlib.Write(row);
        }

        Chunk(output, "IDAT", rows.ToArray());
        Chunk(output, "IEND", []);
        return output.ToArray();
    }

    private static void Chunk(Stream output, string type, byte[] data)
    {
        Span<byte> length = stackalloc byte[4];
        BinaryPrimitives.WriteInt32BigEndian(length, data.Length);
        output.Write(length);

        var body = new byte[4 + data.Length];
        Encoding.ASCII.GetBytes(type, body);
        data.CopyTo(body, 4);
        output.Write(body);

        Span<byte> crc = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32BigEndian(crc, Crc32(body));
        output.Write(crc);
    }

    private static readonly uint[] CrcTable = MakeCrcTable();

    private static uint[] MakeCrcTable()
    {
        var table = new uint[256];
        for (uint n = 0; n < 256; n++)
        {
            var c = n;
            for (var k = 0; k < 8; k++)
                c = (c & 1) != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
            table[n] = c;
        }

        return table;
    }

    private static uint Crc32(ReadOnlySpan<byte> data)
    {
        var crc = 0xFFFFFFFF;
        foreach (var b in data)
            crc = CrcTable[(crc ^ b) & 0xFF] ^ (crc >> 8);
        return crc ^ 0xFFFFFFFF;
    }
}
