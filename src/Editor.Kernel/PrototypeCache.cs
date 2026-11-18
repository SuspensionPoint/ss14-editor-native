using System.Text;
using Robust.Shared.ContentPack;
using Robust.Shared.Serialization.Markdown;
using Robust.Shared.Serialization.Markdown.Mapping;
using Robust.Shared.Serialization.Markdown.Sequence;
using Robust.Shared.Serialization.Markdown.Value;
using Robust.Shared.Utility;

namespace SS14.EditorKernel;

/// <summary>
/// A compact binary form of the engine's parsed prototype files.
///
/// Parsing 28 MB of prototype YAML is the slowest part of starting the engine in a browser. The
/// cache is built on a desktop machine by running the engine's own YAML parser over every
/// prototype file and storing the resulting node trees. The browser then rebuilds the same trees
/// without touching YAML. The cache holds exactly what the parser produced (values, tags, null
/// flags, key order) and nothing derived from it, so prototype resolution doesn't change. Source
/// positions aren't kept, so engine messages about a prototype lose their line numbers.
/// </summary>
public sealed class PrototypeCache
{
    private const uint Magic = 0x32435053; // "SPC2"
    private const byte KindValue = 0;
    private const byte KindSequence = 1;
    private const byte KindMapping = 2;
    private const byte KindMask = 0x03;
    private const byte FlagTag = 0x04;
    private const byte FlagNull = 0x08;
    private const byte FlagKeyTags = 0x10;

    // A mapping can carry a tag on a key as well as on a value ("!Remove key:"), which the
    // fork's partial prototypes rely on. The engine keeps those internal, so they are reached
    // by name; only the few mappings that have any pay for it when a cache is read.
#if ENGINE_KEY_TAGS
    private static readonly System.Reflection.FieldInfo? KeyTagsField =
        typeof(MappingDataNode).GetField("_keyTags", System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic)
        ?? throw new InvalidOperationException("MappingDataNode._keyTags has moved; the prototype cache must keep key tags.");
#else
    // An engine without tags on keys keeps none (src/Directory.Build.props).
    private static readonly System.Reflection.FieldInfo? KeyTagsField = null;
#endif

    private static Dictionary<string, string>? KeyTags(MappingDataNode mapping)
    {
        var tags = (Dictionary<string, string>?)KeyTagsField?.GetValue(mapping);
        return tags is { Count: > 0 } ? tags : null;
    }

    private readonly byte[] _data;
    private readonly string[] _strings;
    private readonly Dictionary<string, (int Offset, int Length)> _files;

    private PrototypeCache(byte[] data, string[] strings, Dictionary<string, (int, int)> files)
    {
        _data = data;
        _strings = strings;
        _files = files;
    }

    /// <summary>
    /// Makes the engine take prototype files from <paramref name="source"/> instead of parsing
    /// them (null: back to parsing). This sets the hook engine patch 0002 adds. It is looked up
    /// by name so that the engine hosts also build against an unpatched engine: only starting
    /// from a cache needs the patch.
    ///
    /// The hook is one for the whole process. Two engine worlds starting side by side in one
    /// process (the native editor) each set it and each clear it, so uses are counted and it is
    /// cleared when the last one is done.
    /// </summary>
    public static void SetEngineSource(Func<ResPath, IEnumerable<DataNodeDocument>?>? source)
    {
        var hook = typeof(Robust.Shared.Prototypes.PrototypeManager).GetField("PreParsedDocuments", System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Static)
                   ?? throw new InvalidOperationException("Starting from a prototype cache needs engine patch 0002 (patches/engine).");
        lock (EngineSourceLock)
        {
            if (source != null)
            {
                _engineSourceUses++;
                hook.SetValue(null, source);
            }
            else if (_engineSourceUses > 0 && --_engineSourceUses == 0)
            {
                hook.SetValue(null, null);
            }
        }
    }

    private static readonly object EngineSourceLock = new();
    private static int _engineSourceUses;

    /// <summary>Rooted resource paths of every cached file, e.g. <c>/Prototypes/Tiles/floors.yml</c>.</summary>
    public IEnumerable<string> Files => _files.Keys;

    public bool Contains(string rootedPath) => _files.ContainsKey(rootedPath);

    /// <summary>The parsed documents of a cached file, or null if the file is not in the cache.</summary>
    public IEnumerable<DataNodeDocument>? TryGetDocuments(ResPath file)
    {
        if (!_files.TryGetValue(file.ToRootedPath().ToString(), out var entry))
            return null;

        var reader = new Reader(_data, entry.Offset, _strings);
        var count = reader.ReadVarint();
        var documents = new DataNodeDocument[count];
        for (var i = 0; i < count; i++)
            documents[i] = new DataNodeDocument(reader.ReadNode());
        return documents;
    }

    // ---- Building ----

    /// <summary>
    /// Builds a cache from parsed files. Each file's documents are checked to survive a write and
    /// read unchanged; a mismatch throws rather than producing a cache that differs from the YAML.
    /// </summary>
    public static byte[] Build(IEnumerable<(string RootedPath, IReadOnlyList<DataNodeDocument> Documents)> files)
    {
        var strings = new List<string>();
        var stringIds = new Dictionary<string, int>(StringComparer.Ordinal);
        int Intern(string value)
        {
            if (!stringIds.TryGetValue(value, out var id))
            {
                id = strings.Count;
                strings.Add(value);
                stringIds.Add(value, id);
            }

            return id;
        }

        var body = new MemoryStream();
        var index = new List<(string Path, int Offset, int Length)>();
        foreach (var (path, documents) in files)
        {
            var start = (int)body.Position;
            WriteVarint(body, documents.Count);
            foreach (var document in documents)
                WriteNode(body, document.Root, Intern);
            index.Add((path, start, (int)body.Position - start));
        }

        var output = new MemoryStream();
        using (var writer = new BinaryWriter(output, Encoding.UTF8, leaveOpen: true))
        {
            writer.Write(Magic);
            writer.Write(strings.Count);
            foreach (var value in strings)
                writer.Write(value);
            writer.Write(index.Count);
            foreach (var (path, offset, length) in index)
            {
                writer.Write(path);
                writer.Write(offset);
                writer.Write(length);
            }

            writer.Write((int)body.Length);
            writer.Write(body.GetBuffer(), 0, (int)body.Length);
        }

        var bytes = output.ToArray();

        // Self-check: everything read back must equal what the parser produced.
        var cache = Load(bytes);
        foreach (var (path, documents) in files)
        {
            var readBack = cache.TryGetDocuments(new ResPath(path))?.ToArray()
                           ?? throw new InvalidDataException($"Prototype cache lost {path}");
            if (readBack.Length != documents.Count)
                throw new InvalidDataException($"Prototype cache changed the document count of {path}");
            for (var i = 0; i < readBack.Length; i++)
            {
                if (!SameNode(documents[i].Root, readBack[i].Root))
                    throw new InvalidDataException($"Prototype cache changed the contents of {path} (document {i})");
            }
        }

        return bytes;
    }

    public static PrototypeCache Load(byte[] bytes)
    {
        using var reader = new BinaryReader(new MemoryStream(bytes, writable: false), Encoding.UTF8);
        if (reader.ReadUInt32() != Magic)
            throw new InvalidDataException("Not a prototype cache.");

        var strings = new string[reader.ReadInt32()];
        for (var i = 0; i < strings.Length; i++)
            strings[i] = string.Intern(reader.ReadString());

        var fileCount = reader.ReadInt32();
        var entries = new (string Path, int Offset, int Length)[fileCount];
        for (var i = 0; i < fileCount; i++)
            entries[i] = (reader.ReadString(), reader.ReadInt32(), reader.ReadInt32());

        var bodyLength = reader.ReadInt32();
        var bodyStart = (int)reader.BaseStream.Position;
        if (bodyStart + bodyLength > bytes.Length)
            throw new InvalidDataException("Prototype cache is truncated.");

        var files = new Dictionary<string, (int, int)>(fileCount, StringComparer.Ordinal);
        foreach (var (path, offset, length) in entries)
            files[path] = (bodyStart + offset, length);

        return new PrototypeCache(bytes, strings, files);
    }

    private static void WriteNode(Stream stream, DataNode node, Func<string, int> intern)
    {
        var kind = node switch
        {
            ValueDataNode => KindValue,
            SequenceDataNode => KindSequence,
            MappingDataNode => KindMapping,
            _ => throw new InvalidDataException($"Unexpected node type {node.GetType().Name} in a prototype file"),
        };

        var header = kind;
        if (node.Tag != null)
            header |= FlagTag;
        if (node.IsNull)
            header |= FlagNull;
        var keyTags = node is MappingDataNode tagged ? KeyTags(tagged) : null;
        if (keyTags != null)
            header |= FlagKeyTags;
        stream.WriteByte(header);
        if (node.Tag != null)
            WriteVarint(stream, intern(node.Tag));

        switch (node)
        {
            case ValueDataNode value:
                WriteVarint(stream, intern(value.Value));
                break;
            case SequenceDataNode sequence:
                WriteVarint(stream, sequence.Count);
                foreach (var child in sequence.Sequence)
                    WriteNode(stream, child, intern);
                break;
            case MappingDataNode mapping:
                WriteVarint(stream, mapping.Count);
                foreach (var (key, child) in mapping)
                {
                    WriteVarint(stream, intern(key));
                    WriteNode(stream, child, intern);
                }

                if (keyTags != null)
                {
                    WriteVarint(stream, keyTags.Count);
                    foreach (var (key, keyTag) in keyTags)
                    {
                        WriteVarint(stream, intern(key));
                        WriteVarint(stream, intern(keyTag));
                    }
                }

                break;
        }
    }

    private static void WriteVarint(Stream stream, int value)
    {
        var remaining = (uint)value;
        while (remaining >= 0x80)
        {
            stream.WriteByte((byte)(remaining | 0x80));
            remaining >>= 7;
        }

        stream.WriteByte((byte)remaining);
    }

    /// <summary>Structural equality on everything the cache stores, independent of DataNode.Equals.</summary>
    private static bool SameNode(DataNode a, DataNode b)
    {
        if (a.GetType() != b.GetType() || a.Tag != b.Tag || a.IsNull != b.IsNull)
            return false;

        switch (a)
        {
            case ValueDataNode value:
                return value.Value == ((ValueDataNode)b).Value;
            case SequenceDataNode sequence:
                var otherSequence = (SequenceDataNode)b;
                if (sequence.Count != otherSequence.Count)
                    return false;
                for (var i = 0; i < sequence.Count; i++)
                {
                    if (!SameNode(sequence[i], otherSequence[i]))
                        return false;
                }

                return true;
            case MappingDataNode mapping:
                var otherMapping = (MappingDataNode)b;
                if (mapping.Count != otherMapping.Count)
                    return false;
                var tags = KeyTags(mapping);
                var otherTags = KeyTags(otherMapping);
                if ((tags?.Count ?? 0) != (otherTags?.Count ?? 0))
                    return false;
                if (tags != null && tags.Any(tag => otherTags!.GetValueOrDefault(tag.Key) != tag.Value))
                    return false;
                for (var i = 0; i < mapping.Count; i++)
                {
                    var (key, child) = mapping[i];
                    var (otherKey, otherChild) = otherMapping[i];
                    if (key != otherKey || !SameNode(child, otherChild))
                        return false;
                }

                return true;
            default:
                return false;
        }
    }

    private struct Reader(byte[] data, int position, string[] strings)
    {
        private int _position = position;

        public int ReadVarint()
        {
            var result = 0;
            var shift = 0;
            while (true)
            {
                var current = data[_position++];
                result |= (current & 0x7F) << shift;
                if ((current & 0x80) == 0)
                    return result;
                shift += 7;
            }
        }

        public DataNode ReadNode()
        {
            var header = data[_position++];
            var tag = (header & FlagTag) != 0 ? strings[ReadVarint()] : null;
            var isNull = (header & FlagNull) != 0;

            switch (header & KindMask)
            {
                case KindValue:
                    return new ValueDataNode(strings[ReadVarint()]) { IsNull = isNull, Tag = tag };
                case KindSequence:
                {
                    var count = ReadVarint();
                    var sequence = new SequenceDataNode(count) { IsNull = isNull, Tag = tag };
                    for (var i = 0; i < count; i++)
                        sequence.Add(ReadNode());
                    return sequence;
                }
                case KindMapping:
                {
                    var count = ReadVarint();
                    var mapping = new MappingDataNode(count) { IsNull = isNull, Tag = tag };
                    for (var i = 0; i < count; i++)
                    {
                        var key = strings[ReadVarint()];
                        mapping.Add(key, ReadNode());
                    }

                    if ((header & FlagKeyTags) != 0)
                    {
                        var tagCount = ReadVarint();
                        var keyTags = new Dictionary<string, string>(tagCount);
                        for (var i = 0; i < tagCount; i++)
                        {
                            var key = strings[ReadVarint()];
                            keyTags[key] = strings[ReadVarint()];
                        }

                        if (KeyTagsField == null)
                            throw new InvalidDataException("This prototype cache has tags on keys, which this engine does not keep.");
                        KeyTagsField.SetValue(mapping, keyTags);
                    }

                    return mapping;
                }
                default:
                    throw new InvalidDataException("Corrupt prototype cache.");
            }
        }
    }
}

/// <summary>
/// Makes the files in a <see cref="PrototypeCache"/> visible to the engine's resource system.
/// The engine lists and opens each prototype file before asking for its documents, so every
/// cached file appears here as an empty file; the contents come from the cache.
/// </summary>
public sealed class PrototypeCacheRoot(PrototypeCache cache) : IContentRoot
{
    public void Mount()
    {
    }

    public bool TryGetFile(ResPath relPath, out Stream? stream)
    {
        if (!cache.Contains(relPath.ToRootedPath().ToString()))
        {
            stream = null;
            return false;
        }

        stream = new MemoryStream(Array.Empty<byte>(), writable: false);
        return true;
    }

    public bool FileExists(ResPath relPath) => cache.Contains(relPath.ToRootedPath().ToString());

    public IEnumerable<ResPath> FindFiles(ResPath path)
    {
        var prefix = path.ToRootedPath().ToString();
        if (!prefix.EndsWith('/'))
            prefix += "/";

        foreach (var file in cache.Files)
        {
            if (file.StartsWith(prefix, StringComparison.Ordinal))
                yield return new ResPath(file).ToRelativePath();
        }
    }

    public IEnumerable<string> GetRelativeFilePaths() => cache.Files;
}
