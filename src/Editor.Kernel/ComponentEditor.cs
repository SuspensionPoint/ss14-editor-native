using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using Content.Shared.DeviceLinking;
using Content.Shared.DeviceNetwork.Components;
using Content.Shared.Storage;
using Robust.Shared.GameObjects;
using Robust.Shared.Prototypes;
using Robust.Shared.Serialization;
using Robust.Shared.Serialization.Manager;
using Robust.Shared.Serialization.Markdown;
using Robust.Shared.Serialization.Markdown.Mapping;
using Robust.Shared.Serialization.Markdown.Sequence;
using Robust.Shared.Serialization.Markdown.Validation;
using Robust.Shared.Serialization.Markdown.Value;
using Robust.Shared.Serialization.TypeSerializers.Interfaces;

namespace SS14.EditorKernel;

/// <summary>
/// Reads and writes the component data of live entities with the engine's own serialization
/// manager, in the form a map file stores it: per component, only what differs from the
/// prototype.
///
/// Reading follows EntitySerializer.SerializeComponents and writing follows
/// EntityDeserializer.LoadEntity, call for call. The one difference is how a reference to another
/// entity is written. A map file uses file uids, which only exist while a file is being read or
/// written. Here a reference is the engine's own uid, the same number the interface uses for that
/// entity.
/// </summary>
internal sealed partial class ComponentEditor
{
    private readonly IEntityManager _entMan;
    private readonly IComponentFactory _factory;
    private readonly ISerializationManager _serialization;
    private readonly IPrototypeManager _protos;
    private readonly LiveUidContext _context;
    private readonly Dictionary<string, Dictionary<string, MappingDataNode>> _prototypeCache = new();
    private readonly MappingDataNode _defaultMetaData;
    private readonly MappingDataNode _defaultTransform;

    /// <summary>Components the placement and naming operations own; they are not edited as data.</summary>
    private static readonly HashSet<string> Reserved = ["Transform", "MetaData"];

    public ComponentEditor(IEntityManager entMan, IComponentFactory factory, ISerializationManager serialization, IPrototypeManager protos)
    {
        _entMan = entMan;
        _factory = factory;
        _serialization = serialization;
        _protos = protos;
        _context = new LiveUidContext(serialization);

        // Every entity has these two without its prototype having to list them. The game's map
        // save compares them against a blank component, so only what was set is written.
        _defaultMetaData = serialization.WriteValueAs<MappingDataNode>(typeof(MetaDataComponent), new MetaDataComponent(), alwaysWrite: true, context: _context);
        _defaultTransform = serialization.WriteValueAs<MappingDataNode>(typeof(TransformComponent), new TransformComponent(), alwaysWrite: true, context: _context);
    }

    /// <summary>The entity's components as a map file would store them, as a JSON array.</summary>
    public string Describe(EntityUid uid)
    {
        var array = new JsonArray();
        foreach (var (name, node) in Delta(uid, includeReserved: true))
        {
            var json = (JsonObject) ToJson(node)!;
            var ordered = new JsonObject { ["type"] = name };
            foreach (var (key, value) in json.ToArray())
            {
                json.Remove(key);
                ordered[key] = value;
            }

            array.Add(ordered);
        }

        return array.ToJsonString();
    }

    /// <summary>Per component, what differs from the prototype. Same logic as the game's map save.</summary>
    private List<(string Name, MappingDataNode Node)> Delta(EntityUid uid, bool includeReserved)
    {
        var result = new List<(string, MappingDataNode)>();
        var prototype = _entMan.GetComponent<MetaDataComponent>(uid).EntityPrototype;
        var cache = PrototypeNodes(prototype);

        foreach (var component in _entMan.GetComponents(uid))
        {
            if (component.LifeStage >= ComponentLifeStage.Stopping)
                continue;

            var type = component.GetType();
            var registration = _factory.GetRegistration(type);
            if (registration.Unsaved || (!includeReserved && Reserved.Contains(registration.Name)))
                continue;

            MappingDataNode? node;
            MappingDataNode? fromPrototype = null;
            if (cache != null && cache.TryGetValue(registration.Name, out fromPrototype))
            {
                node = _serialization.WriteValueAs<MappingDataNode>(type, component, alwaysWrite: true, context: _context).Except(fromPrototype);
                if (node == null)
                    continue;
            }
            else
            {
                node = _serialization.WriteValueAs<MappingDataNode>(type, component, alwaysWrite: false, context: _context);
            }

            if (node.Children.Count == 0 && fromPrototype != null)
                continue;

            result.Add((registration.Name, node));
        }

        result.Sort((a, b) => string.CompareOrdinal(a.Item1, b.Item1));
        return result;
    }

    private Dictionary<string, MappingDataNode>? PrototypeNodes(EntityPrototype? prototype)
    {
        if (prototype == null)
            return null;

        if (_prototypeCache.TryGetValue(prototype.ID, out var cache))
            return cache;

        _prototypeCache[prototype.ID] = cache = new Dictionary<string, MappingDataNode>(prototype.Components.Count);
        _context.WritingReadingPrototypes = true;
        foreach (var (name, entry) in prototype.Components)
            cache[name] = _serialization.WriteValueAs<MappingDataNode>(entry.Component.GetType(), entry.Component, alwaysWrite: true, context: _context);
        _context.WritingReadingPrototypes = false;
        cache.TryAdd("MetaData", _defaultMetaData);
        cache.TryAdd("Transform", _defaultTransform);
        return cache;
    }

    /// <summary>A component's complete data, or null if the entity does not have it. For undo.</summary>
    public MappingDataNode? Snapshot(EntityUid uid, string name)
    {
        var registration = _factory.GetRegistration(name);
        if (!_entMan.TryGetComponent(uid, registration.Type, out var component))
            return null;

        return _serialization.WriteValueAs<MappingDataNode>(registration.Type, component, alwaysWrite: true, context: _context);
    }

    /// <summary>Puts a component back to a snapshot: its complete data, or absent.</summary>
    public void Restore(EntityUid uid, string name, MappingDataNode? snapshot)
    {
        var registration = _factory.GetRegistration(name);
        if (snapshot == null)
        {
            if (_entMan.HasComponent(uid, registration.Type))
                _entMan.RemoveComponent(uid, registration.Type);
            return;
        }

        Apply(uid, registration, snapshot);
    }

    /// <summary>
    /// The fields in which a component lists other entities of the map, by the component's name:
    /// what a button is linked to, the devices of an alarm, the alarms of a device, and where
    /// in its grid of cells a bag keeps each thing it holds.
    /// </summary>
    private static readonly Dictionary<string, string[]> Listing = new()
    {
        ["DeviceLinkSource"] = ["linkedPorts"],
        ["DeviceList"] = ["devices", "configurators"],
        [ForkCompat.ListedDevice] = ["deviceLists", "configurators"],
        ["Storage"] = ["storedItems"],
        // What else names another entity on the two stations, found by a survey of them
        // on 2026-11-16. Few entities have these, so they are found by name and not by type,
        // and a fork that lacks one of them loses nothing.
        ["AnalysisConsole"] = ["analyzerEntity"],
        ["BodyScanner"] = ["tableEntity"],
        ["OperatingTable"] = ["scanner"],
        ["ItemPlacer"] = ["placedEntities"],
        ["SiliconLawUpdater"] = ["core"],
        ["StationAiCore"] = ["lawConsole", "remoteEntity"],
        ["GasTank"] = ["toggleActionEntity"],
        ["HandheldLight"] = ["toggleActionEntity", "selfToggleActionEntity"],
        ["Docking"] = ["dockedWith"],
    };

    /// <summary>
    /// What goes with a field of <see cref="Listing"/> when the one entity it names goes. A
    /// dock that loses the dock it is docked with keeps no name of a joint either, as in the
    /// game's DockingSystem.Cleanup.
    /// </summary>
    private static readonly Dictionary<string, string[]> GoesWith = new()
    {
        ["dockedWith"] = ["dockJointId"],
    };

    /// <summary>
    /// The components of <see cref="Listing"/> the fork's component factory does not know, in
    /// the table's order. A fork that renamed one would keep references to deleted entities in
    /// it, so the kernel warns of each at start.
    /// </summary>
    public IReadOnlyList<string> UnknownListing() =>
        Listing.Keys.Where(name => !_factory.TryGetRegistration(name, out _)).ToArray();

    /// <summary>The components of <see cref="Listing"/> that <see cref="Listers"/> finds by their type.</summary>
    private static readonly HashSet<string> Typed = ["DeviceLinkSource", "DeviceList", ForkCompat.ListedDevice, "Storage"];

    /// <summary>Every component of the kinds in <see cref="Listing"/> that lists one of these entities.</summary>
    public List<(EntityUid Uid, string Name)> Listers(IReadOnlySet<EntityUid> listed)
    {
        var found = new List<(EntityUid, string)>();
        var sources = _entMan.AllEntityQueryEnumerator<DeviceLinkSourceComponent>();
        while (sources.MoveNext(out var uid, out var source))
        {
            foreach (var sink in source.LinkedPorts.Keys)
            {
                if (!listed.Contains(sink))
                    continue;

                found.Add((uid, "DeviceLinkSource"));
                break;
            }
        }

        var lists = _entMan.AllEntityQueryEnumerator<DeviceListComponent>();
        while (lists.MoveNext(out var uid, out var list))
        {
            if (list.Devices.Overlaps(listed) || list.Configurators.Overlaps(listed))
                found.Add((uid, "DeviceList"));
        }

        var devices = _entMan.AllEntityQueryEnumerator<ListedDeviceComponent>();
        while (devices.MoveNext(out var uid, out var device))
        {
            if (device.DeviceLists.Overlaps(listed) || device.Configurators.Overlaps(listed))
                found.Add((uid, ForkCompat.ListedDevice));
        }

        var storages = _entMan.AllEntityQueryEnumerator<StorageComponent>();
        while (storages.MoveNext(out var uid, out var storage))
        {
            if (storage.StoredItems.Keys.Any(listed.Contains))
                found.Add((uid, "Storage"));
        }

        foreach (var (name, fields) in Listing)
        {
            if (Typed.Contains(name) || !_factory.TryGetRegistration(name, out var registration))
                continue;

            foreach (var (uid, component) in _entMan.GetAllComponents(registration.Type, includePaused: true))
            {
                var data = _serialization.WriteValueAs<MappingDataNode>(registration.Type, component, alwaysWrite: true, context: _context);
                if (fields.Any(field => data.TryGet(field, out var node) && Names(node, listed)))
                    found.Add((uid, name));
            }
        }

        return found;
    }

    /// <summary>Whether a field's data, one entity or a list of them, names one of these.</summary>
    private static bool Names(DataNode node, IReadOnlySet<EntityUid> listed)
    {
        bool Listed(string text) => int.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id) && listed.Contains(new EntityUid(id));

        return node switch
        {
            ValueDataNode value => Listed(value.Value),
            SequenceDataNode sequence => sequence.Any(item => item is ValueDataNode value && Listed(value.Value)),
            MappingDataNode mapping => mapping.Keys.Any(Listed),
            _ => false,
        };
    }

    /// <summary>The entities a field's data names, one or a list of them.</summary>
    private static IEnumerable<EntityUid> Named(DataNode node)
    {
        IEnumerable<string> texts = node switch
        {
            ValueDataNode value => [value.Value],
            SequenceDataNode sequence => sequence.OfType<ValueDataNode>().Select(value => value.Value),
            MappingDataNode mapping => mapping.Keys,
            _ => Enumerable.Empty<string>(),
        };

        foreach (var text in texts)
        {
            if (int.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id))
                yield return new EntityUid(id);
        }
    }

    /// <summary>
    /// Cuts the fields <see cref="Listing"/> names, on these entities, down to these entities,
    /// for as long as a copy of them is written. The serializer writes "invalid" for an entity
    /// left out of a copy, and cannot write a button linked to two of them at all. Each list
    /// is swapped for a shorter one and not changed. The action returned puts the very lists
    /// back, so the map is as it was. A field that names one entity has no list to swap.
    /// </summary>
    public Action Trim(IReadOnlySet<EntityUid> kept)
    {
        var back = new List<Action>();
        HashSet<EntityUid> Cut(HashSet<EntityUid> list) => list.Where(kept.Contains).ToHashSet();

        foreach (var uid in kept)
        {
            if (_entMan.TryGetComponent(uid, out DeviceLinkSourceComponent? source) && source.LinkedPorts.Keys.Any(sink => !kept.Contains(sink)))
            {
                var links = source.LinkedPorts;
                source.LinkedPorts = links.Where(link => kept.Contains(link.Key)).ToDictionary(link => link.Key, link => link.Value);
                back.Add(() => source.LinkedPorts = links);
            }

            if (_entMan.TryGetComponent(uid, out DeviceListComponent? list) && !(list.Devices.IsSubsetOf(kept) && list.Configurators.IsSubsetOf(kept)))
            {
                var (devices, configurators) = (list.Devices, list.Configurators);
                (list.Devices, list.Configurators) = (Cut(devices), Cut(configurators));
                back.Add(() => (list.Devices, list.Configurators) = (devices, configurators));
            }

            if (_entMan.TryGetComponent(uid, out ListedDeviceComponent? device) && !(device.DeviceLists.IsSubsetOf(kept) && device.Configurators.IsSubsetOf(kept)))
            {
                var (lists, configurators) = (device.DeviceLists, device.Configurators);
                (device.DeviceLists, device.Configurators) = (Cut(lists), Cut(configurators));
                back.Add(() => (device.DeviceLists, device.Configurators) = (lists, configurators));
            }

            // The other components of the table are found by name and have no lists to swap.
            // One that names an entity left out is set to its data without that name, and
            // put back to its data as it was, as undo puts back a component.
            foreach (var (name, fields) in Listing)
            {
                if (Typed.Contains(name) || !_factory.TryGetRegistration(name, out var registration) || !_entMan.HasComponent(uid, registration.Type))
                    continue;

                var whole = Snapshot(uid, name)!;
                var outside = new HashSet<EntityUid>();
                foreach (var field in fields)
                {
                    if (whole.TryGet(field, out var node))
                        outside.UnionWith(Named(node).Where(named => !kept.Contains(named)));
                }

                if (outside.Count == 0)
                    continue;

                back.Add(() => Restore(uid, name, whole));
                try
                {
                    Restore(uid, name, Without(name, whole, outside));
                }
                catch
                {
                    // No copy is written. Nothing cut so far may stay cut.
                    foreach (var put in back)
                        put();
                    throw;
                }
            }
        }

        return () =>
        {
            foreach (var put in back)
                put();
        };
    }

    /// <summary>
    /// A component's snapshot with these entities taken out of the fields that list entities.
    /// The rest of it, and the order of what is left, is as the snapshot has it.
    /// </summary>
    public MappingDataNode Without(string name, MappingDataNode snapshot, IReadOnlySet<EntityUid> listed)
    {
        bool Listed(string text) => int.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id) && listed.Contains(new EntityUid(id));

        var result = snapshot.Copy();
        foreach (var field in Listing[name])
        {
            if (!result.TryGet(field, out var node))
                continue;

            if (node is MappingDataNode mapping)
            {
                foreach (var key in mapping.Keys)
                {
                    if (Listed(key))
                        mapping.Remove(key);
                }
            }
            else if (node is SequenceDataNode sequence)
            {
                for (var i = sequence.Count - 1; i >= 0; i--)
                {
                    if (sequence[i] is ValueDataNode value && Listed(value.Value))
                        sequence.RemoveAt(i);
                }
            }
            else if (node is ValueDataNode one && Listed(one.Value))
            {
                // A field that names one entity names none.
                result[field] = ValueDataNode.Null();
                foreach (var other in GoesWith.GetValueOrDefault(field, []))
                    result[other] = ValueDataNode.Null();
            }
        }

        return result;
    }

    /// <summary>
    /// Makes the entity's stored component data (apart from its placement and name) exactly the
    /// given list. Returns the names of the components that changed.
    /// </summary>
    public List<string> SetComponents(EntityUid uid, string componentsJson, Action<string>? beforeChange = null)
    {
        var wanted = new Dictionary<string, MappingDataNode>();
        foreach (var item in JsonNode.Parse(componentsJson)?.AsArray() ?? [])
        {
            if (item is not JsonObject json || json["type"]?.GetValue<string>() is not { } name || Reserved.Contains(name))
                continue;

            if (!_factory.TryGetRegistration(name, out _))
                throw new KernelException($"The engine has no component called '{name}'.");

            var node = (MappingDataNode) ToNode(json);
            node.Remove("type");
            wanted[name] = node;
        }

        var current = Delta(uid, includeReserved: false).ToDictionary(pair => pair.Name, pair => pair.Node);
        var prototype = _entMan.GetComponent<MetaDataComponent>(uid).EntityPrototype;
        var prototypeData = prototype == null ? null : _protos.GetPrototypeData(prototype);
        var changed = new List<string>();

        foreach (var name in current.Keys.Union(wanted.Keys).ToArray())
        {
            current.TryGetValue(name, out var now);
            wanted.TryGetValue(name, out var target);
            if (now != null && target != null && now.Equals(target))
                continue;

            var registration = _factory.GetRegistration(name);
            MappingDataNode? fromPrototype = null;
            prototypeData?.TryGetValue(name, out fromPrototype);
            beforeChange?.Invoke(name);

            if (target != null)
            {
                // As when a map loads: the entity's data laid over the prototype's.
                Apply(uid, registration, fromPrototype != null ? _serialization.CombineMappings(target, fromPrototype) : target);
            }
            else if (fromPrototype != null)
            {
                // Nothing of its own any more: back to what the prototype says.
                Apply(uid, registration, fromPrototype);
            }
            else if (_entMan.HasComponent(uid, registration.Type))
            {
                _entMan.RemoveComponent(uid, registration.Type);
            }

            changed.Add(name);
        }

        return changed;
    }

    private void Apply(EntityUid uid, ComponentRegistration registration, MappingDataNode data)
    {
        var read = (IComponent) _serialization.Read(registration.Type, data, context: _context, notNullableOverride: true)!;
        if (!_entMan.TryGetComponent(uid, registration.Type, out var existing))
        {
            existing = _factory.GetComponent(registration);
            _entMan.AddComponent(uid, existing);
        }

        object target = existing;
        _serialization.CopyTo(read, ref target, context: _context, notNullableOverride: true);
    }

    // ---- data nodes to and from JSON ----
    //
    // The interface holds component data as plain objects, as its YAML parser produced them:
    // typed scalars, and a `_ss14Tag` member wherever the YAML had a !type: tag.

    private static JsonNode? ToJson(DataNode node)
    {
        switch (node)
        {
            case MappingDataNode mapping:
            {
                var json = new JsonObject();
                if (mapping.Tag != null)
                    json["_ss14Tag"] = mapping.Tag;
                foreach (var (key, value) in mapping.Children)
                    json[key] = ToJson(value);
                return json;
            }
            case SequenceDataNode sequence:
            {
                var array = new JsonArray();
                foreach (var item in sequence.Sequence)
                    array.Add(ToJson(item));
                return sequence.Tag == null ? array : new JsonObject { ["_ss14Tag"] = sequence.Tag, ["items"] = array };
            }
            case ValueDataNode value:
            {
                var scalar = Scalar(value);
                return value.Tag == null ? scalar : new JsonObject { ["_ss14Tag"] = value.Tag, ["value"] = scalar };
            }
            default:
                return null;
        }
    }

    /// <summary>A plain YAML scalar as the type a YAML parser would give it.</summary>
    private static JsonNode? Scalar(ValueDataNode value)
    {
        if (value.IsNull)
            return null;

        var text = value.Value;
        if (text is "true" or "True")
            return JsonValue.Create(true);
        if (text is "false" or "False")
            return JsonValue.Create(false);
        if (text.Length > 0 && (char.IsAsciiDigit(text[0]) || text[0] == '-') && text.All(c => char.IsAsciiDigit(c) || c is '-' or '.')
            && double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var number) && number.ToString(CultureInfo.InvariantCulture) == text)
        {
            return JsonValue.Create(number);
        }

        return JsonValue.Create(text);
    }

    private static DataNode ToNode(JsonNode? json)
    {
        switch (json)
        {
            case null:
                return ValueDataNode.Null();
            case JsonArray array:
                return new SequenceDataNode(array.Select(ToNode).ToList());
            case JsonObject tagged when tagged["_ss14Tag"] is { } tag:
            {
                DataNode node;
                if (tagged.Count == 2 && tagged.ContainsKey("value"))
                    node = ToNode(tagged["value"]);
                else if (tagged.Count == 2 && tagged["items"] is JsonArray items)
                    node = ToNode(items);
                else
                    node = Mapping(tagged);

                var name = tag.GetValue<string>();
                node.Tag = name.StartsWith('!') ? name : "!type:" + name;
                return node;
            }
            case JsonObject plain:
                return Mapping(plain);
            default:
            {
                var element = json.GetValue<JsonElement>();
                return new ValueDataNode(element.ValueKind switch
                {
                    JsonValueKind.True => "true",
                    JsonValueKind.False => "false",
                    JsonValueKind.Number => element.GetRawText(),
                    _ => element.GetString() ?? "",
                });
            }
        }
    }

    private static MappingDataNode Mapping(JsonObject json)
    {
        var mapping = new MappingDataNode();
        foreach (var (key, value) in json)
        {
            if (key != "_ss14Tag")
                mapping.Add(key, ToNode(value));
        }

        return mapping;
    }

    /// <summary>Writes and reads entity references as the engine's own uids.</summary>
    private sealed class LiveUidContext : ISerializationContext, ITypeSerializer<EntityUid, ValueDataNode>
    {
        public SerializationManager.SerializerProvider SerializerProvider { get; }

        public bool WritingReadingPrototypes { get; set; }

        public LiveUidContext(ISerializationManager serialization)
        {
#if ENGINE_SERIALIZER_PROVIDER_TAKES_MANAGER
            SerializerProvider = new SerializationManager.SerializerProvider(serialization);
#else
            SerializerProvider = new SerializationManager.SerializerProvider();
#endif
            SerializerProvider.RegisterSerializer(this);
        }

        public ValidationNode Validate(
            ISerializationManager serializationManager,
            ValueDataNode node,
            Robust.Shared.IoC.IDependencyCollection dependencies,
            ISerializationContext? context = null)
        {
            return node.Value == "invalid" || int.TryParse(node.Value, out _)
                ? new ValidatedValueNode(node)
                : new ErrorNode(node, "Invalid EntityUid");
        }

        public DataNode Write(
            ISerializationManager serializationManager,
            EntityUid value,
            Robust.Shared.IoC.IDependencyCollection dependencies,
            bool alwaysWrite = false,
            ISerializationContext? context = null)
        {
            return new ValueDataNode(value.IsValid() ? value.Id.ToString(CultureInfo.InvariantCulture) : "invalid");
        }

        public EntityUid Read(
            ISerializationManager serializationManager,
            ValueDataNode node,
            Robust.Shared.IoC.IDependencyCollection dependencies,
            SerializationHookContext hookCtx,
            ISerializationContext? context = null,
            ISerializationManager.InstantiationDelegate<EntityUid>? instanceProvider = null)
        {
            return int.TryParse(node.Value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id) ? new EntityUid(id) : EntityUid.Invalid;
        }
    }
}
