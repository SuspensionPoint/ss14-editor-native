using System.Globalization;
using System.Numerics;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;
using Robust.Shared.Map;
using Robust.Shared.Maths;
using Robust.Shared.Prototypes;
using Robust.Shared.Reflection;
using Robust.Shared.Serialization;
using Robust.Shared.Serialization.Manager;
using Robust.Shared.Serialization.Markdown;
using Robust.Shared.Serialization.Markdown.Mapping;
using Robust.Shared.Serialization.Markdown.Validation;
using Robust.Shared.Serialization.Markdown.Value;
using Robust.Shared.Serialization.TypeSerializers.Interfaces;
using SS14.EditorKernel;

namespace Content.EditorView;

// Saved state: an entity shown as the map saves it, and not as its prototype alone.
//
// In the game's mapping mode the server sends the client its copy of each networked component,
// and sets appearance data of its own that the client could not work out. The kernel sends
// the first as the map file holds it (Editor.Kernel/SavedState.cs) and the second as values
// (SceneVisuals.Appearance). The data is read onto the entity before it starts, as the game's
// map loader does, so the client's systems set its looks from it as they would from the
// server's copy. When it changes, the entity is made again, for those systems look at the
// data as the entity starts.
public sealed partial class ViewWorld
{
    private readonly IComponentFactory _factory = IoCManager.Resolve<IComponentFactory>();
    private readonly ISerializationManager _serialization = IoCManager.Resolve<ISerializationManager>();
    private readonly IReflectionManager _reflection = IoCManager.Resolve<IReflectionManager>();
    private readonly Dictionary<string, Type?> _types = new();

    /// <summary>Saved components the client has no component for, by name, counted as they come.</summary>
    public readonly Dictionary<string, int> UnknownSaved = new();

    /// <summary>Saved components that could not be read, by name, with the first reason.</summary>
    public readonly Dictionary<string, string> FailedSaved = new();

    /// <summary>Appearance values whose key or value type the view does not know, by the key's text.</summary>
    public readonly Dictionary<string, int> UnknownAppearance = new();

    /// <summary>
    /// Takes in the saved data that changed. An entity shown already is taken away and named in
    /// the list returned, to be made again with its data once the rest of the delta is in.
    /// </summary>
    private List<int> TakeSaved(ViewDocument doc, IReadOnlyList<SceneSaved> saved)
    {
        var again = new List<int>();
        foreach (var record in saved)
        {
            if (record.Components.Count == 0)
                doc.Saved.Remove(record.Uid);
            else
                doc.Saved[record.Uid] = record.Components;

            if (!doc.Entities.TryGetValue(record.Uid, out var uid) || !_entMan.EntityExists(uid) || !doc.DocUids.ContainsKey(uid))
                continue;

            MarkAround(doc, uid);
            doc.DocUids.Remove(uid);
            doc.Entities.Remove(record.Uid);
            _hiddenByLayer.Remove(uid);
            _entMan.DeleteEntity(uid);
            again.Add(record.Uid);
        }

        return again;
    }

    /// <summary>
    /// Makes a shown entity from its prototype and its saved data, as the game's map loader
    /// does (Robust.Shared/EntitySerialization/EntityDeserializer.cs LoadEntity): the entity is
    /// made from the prototype, each saved component is merged with the prototype's data and
    /// read onto it, and then it starts.
    /// </summary>
    private EntityUid Spawn(ViewDocument doc, SceneEntity scene, EntityCoordinates coordinates, Angle rotation)
    {
        if (!doc.Saved.TryGetValue(scene.Uid, out var saved))
            return _entMan.SpawnAttachedTo(scene.Prototype, coordinates, rotation: rotation);

        var uid = _entMan.CreateEntityUninitialized(scene.Prototype, coordinates, rotation: rotation);
        var prototype = _protos.Index<EntityPrototype>(scene.Prototype);
        var context = new DocumentUids(_serialization, doc);
        foreach (var component in saved)
        {
            // A component only the server has, as the game's client ignores it.
            if (!_factory.TryGetRegistration(component.Name, out var registration))
            {
                UnknownSaved[component.Name] = UnknownSaved.GetValueOrDefault(component.Name) + 1;
                continue;
            }

            try
            {
                var data = component.Data;
                if (_protos.GetPrototypeData(prototype).TryGetValue(component.Name, out var fromPrototype))
                    data = _serialization.CombineMappings(data, fromPrototype);

                var read = (IComponent) _serialization.Read(registration.Type, data, context)!;
                if (!_entMan.TryGetComponent(uid, registration.Type, out var existing))
                {
                    existing = _factory.GetComponent(registration);
                    _entMan.AddComponent(uid, existing);
                }

                _serialization.CopyTo(read, ref existing, context, notNullableOverride: true);
                if (CheckReads)
                    CompareRead(doc, uid, registration, existing, component, scene.Uid);
            }
            catch (Exception e)
            {
                FailedSaved.TryAdd(component.Name, e.Message);
            }
        }

        // An entity it names that the view has not made yet reads as none. It is made again
        // once the rest are in (ResolveSaved).
        if (context.Missed)
            doc.Unresolved.Add(scene.Uid);

        try
        {
            _entMan.InitializeAndStartEntity(uid, _xform.GetMapId(coordinates));
        }
        catch
        {
            _entMan.DeleteEntity(uid);
            throw;
        }

        return uid;
    }

    /// <summary>
    /// Makes again the entities whose saved data named one the view had not made yet, as the
    /// game's map loader makes every entity before it reads any data. One that is still not
    /// shown (inside a container) stays none.
    /// </summary>
    private void ResolveSaved(ViewDocument doc)
    {
        if (doc.Unresolved.Count == 0)
            return;

        var again = doc.Unresolved.Where(doc.Placed.ContainsKey).ToList();
        doc.Unresolved.Clear();
        foreach (var docUid in again)
        {
            if (!doc.Entities.TryGetValue(docUid, out var uid) || !_entMan.EntityExists(uid))
                continue;

            doc.DocUids.Remove(uid);
            doc.Entities.Remove(docUid);
            _hiddenByLayer.Remove(uid);
            _entMan.DeleteEntity(uid);
            Upsert(doc, doc.Placed[docUid]);
        }

        // What it names may still be missing, and stays so.
        doc.Unresolved.Clear();
    }

    /// <summary>
    /// Sets the appearance values only the game's server works out, with the client's own
    /// appearance system, as if they had come with the server's appearance data.
    /// </summary>
    private void SetAppearance(EntityUid uid, AppearanceComponent appearance, IReadOnlyList<AppearanceValue> values)
    {
        var system = _entMan.System<SharedAppearanceSystem>();
        foreach (var value in values)
        {
            if (Decode(value) is not var (key, data))
            {
                var name = $"{value.KeyType}.{value.Key} ({value.Kind})";
                UnknownAppearance[name] = UnknownAppearance.GetValueOrDefault(name) + 1;
                continue;
            }

            system.SetData(uid, key, data, appearance);
        }
    }

    /// <summary>The key and value an appearance value names, or null if the client lacks its types.</summary>
    public (Enum Key, object Value)? Decode(AppearanceValue value)
    {
        if (EnumOf(value.KeyType, value.Key) is not { } key)
            return null;

        var text = value.Value;
        object? data = value.Kind switch
        {
            "bool" => text == "true",
            "int" => int.Parse(text, CultureInfo.InvariantCulture),
            "float" => float.Parse(text, CultureInfo.InvariantCulture),
            "double" => double.Parse(text, CultureInfo.InvariantCulture),
            "string" => text,
            "enum" => value.ValueType == null ? null : EnumOf(value.ValueType, text),
            "color" => Color.FromHex(text),
            "vector2" => text.Split(',') is [var x, var y]
                ? new Vector2(float.Parse(x, CultureInfo.InvariantCulture), float.Parse(y, CultureInfo.InvariantCulture))
                : null,
            _ => null,
        };

        return data == null ? null : (key, data);
    }

    private Enum? EnumOf(string type, string member)
    {
        if (!_types.TryGetValue(type, out var found))
            _types[type] = found = _reflection.GetType(type) ?? Type.GetType(type);

        return found is { IsEnum: true } && Enum.TryParse(found, member, out var value) ? (Enum) value! : null;
    }

    private static readonly System.Reflection.FieldInfo AppearanceData =
        typeof(AppearanceComponent).GetField("AppearanceData", System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic)
        ?? throw new InvalidOperationException("AppearanceComponent.AppearanceData has moved.");

    /// <summary>For the checks: whether the view shows an entity of the document.</summary>
    public bool Shows(int document, int uid) => _documents.TryGetValue(document, out var doc) && doc.Entities.ContainsKey(uid);

    /// <summary>For the checks: the appearance data of a shown entity, or null if it has none or is not shown.</summary>
    public IReadOnlyDictionary<Enum, object>? AppearanceOf(int document, int uid)
    {
        if (!_documents.TryGetValue(document, out var doc) || !doc.Entities.TryGetValue(uid, out var view)
            || !_entMan.TryGetComponent(view, out AppearanceComponent? appearance))
        {
            return null;
        }

        return (Dictionary<Enum, object>) AppearanceData.GetValue(appearance)!;
    }

    /// <summary>
    /// For the checks: a shown entity's component, all its data written as the kernel writes
    /// it. Known is false when the client has no such component. Data is null when the entity
    /// lacks it.
    /// </summary>
    public (bool Known, MappingDataNode? Data) SavedOf(int document, int uid, string name)
    {
        if (!_factory.TryGetRegistration(name, out var registration))
            return (false, null);

        if (!_documents.TryGetValue(document, out var doc) || !doc.Entities.TryGetValue(uid, out var view)
            || !_entMan.TryGetComponent(view, registration.Type, out var component))
        {
            return (true, null);
        }

        return (true, _serialization.WriteValueAs<MappingDataNode>(registration.Type, component, alwaysWrite: true, context: new DocumentUids(_serialization, doc)));
    }

    /// <summary>For the checks: compare what each saved component reads as before its entity starts.</summary>
    public bool CheckReads;

    /// <summary>For the checks: fields compared as read, and those that read otherwise, by component and field, with an example.</summary>
    public int ReadFields;

    public readonly Dictionary<string, (int Count, string Example)> ReadWrong = new();

    /// <summary>Fields the server's kind of a component has and the client's lacks, by component and field.</summary>
    public readonly HashSet<string> ServerFields = new();

    private void CompareRead(ViewDocument doc, EntityUid uid, ComponentRegistration registration, IComponent component, SavedComponent saved, int docUid)
    {
        // Each field as the component holds it, whole: the client's kind of a component may
        // start a field from another value than the server's kind, so a field can be as sent
        // and still be no different from the client's prototype.
        var context = new DocumentUids(_serialization, doc);
        var whole = _serialization.WriteValueAs<MappingDataNode>(registration.Type, component, alwaysWrite: true, context: context);
        // A component the prototype lacks is saved without the fields left at their defaults,
        // nested ones too, so it is compared written the same way as well.
        var brief = _serialization.WriteValueAs<MappingDataNode>(registration.Type, component, alwaysWrite: false, context: context);
        foreach (var (key, value) in saved.Data)
        {
            var name = $"{saved.Name}.{key}";
            if (!whole.TryGet(key, out var read))
            {
                ServerFields.Add(name);
                continue;
            }

            ReadFields++;
            if (read.Equals(value) || brief.TryGet(key, out var written) && written.Equals(value))
                continue;

            var (count, example) = ReadWrong.GetValueOrDefault(name, (0, ""));
            ReadWrong[name] = (count + 1, count == 0 ? $"{docUid}: sent {value}, read {read}" : example);
        }
    }

    /// <summary>
    /// Reads entity references in saved data, which name the document's uids, as the view's
    /// own entities for them. One the view does not show (inside a container) reads as an
    /// entity that is not there, one of its own for each, so that a list or a table keyed by
    /// them keeps every entry, as the game's client keeps an entity it has not been sent yet.
    /// </summary>
    private sealed class DocumentUids : ISerializationContext, ITypeSerializer<EntityUid, ValueDataNode>
    {
        private readonly ViewDocument _doc;

        /// <summary>Whether a reference read named an entity of the document the view does not have.</summary>
        public bool Missed { get; private set; }

        public SerializationManager.SerializerProvider SerializerProvider { get; }

        public bool WritingReadingPrototypes => false;

        public DocumentUids(ISerializationManager serialization, ViewDocument doc)
        {
            _doc = doc;
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
            IDependencyCollection dependencies,
            ISerializationContext? context = null)
        {
            return node.Value == "invalid" || int.TryParse(node.Value, out _)
                ? new ValidatedValueNode(node)
                : new ErrorNode(node, "Invalid EntityUid");
        }

        public DataNode Write(
            ISerializationManager serializationManager,
            EntityUid value,
            IDependencyCollection dependencies,
            bool alwaysWrite = false,
            ISerializationContext? context = null)
        {
            var id = value.Id < 0 ? -value.Id : _doc.DocUids.GetValueOrDefault(value);
            return new ValueDataNode(id != 0 ? id.ToString(CultureInfo.InvariantCulture) : "invalid");
        }

        public EntityUid Read(
            ISerializationManager serializationManager,
            ValueDataNode node,
            IDependencyCollection dependencies,
            SerializationHookContext hookCtx,
            ISerializationContext? context = null,
            ISerializationManager.InstantiationDelegate<EntityUid>? instanceProvider = null)
        {
            if (!int.TryParse(node.Value, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id))
                return EntityUid.Invalid;

            if (_doc.Entities.TryGetValue(id, out var uid))
                return uid;

            Missed = true;
            return new EntityUid(-id);
        }
    }
}
