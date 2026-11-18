using System.Numerics;
using System.Reflection;
using System.Text.Json.Nodes;
using Robust.Shared.GameObjects;
using Robust.Shared.Maths;
using Robust.Shared.Prototypes;
using Robust.Shared.Serialization.Manager;
using Robust.Shared.Serialization.Manager.Attributes;
using Robust.Shared.Serialization.Manager.Definition;
using Robust.Shared.Serialization.Markdown.Mapping;
using Robust.Shared.Serialization.TypeSerializers.Implementations.Custom;

namespace SS14.EditorKernel;

/// <summary>The data fields of an entity's components, and the ids of the prototype kinds they name.</summary>
public sealed record FieldsAnswer(IReadOnlyList<ComponentFields> Components, IReadOnlyDictionary<string, IReadOnlyList<string>> Ids);

/// <summary>One component's data fields, in the order the component declares them.</summary>
public sealed record ComponentFields(string Name, IReadOnlyList<DataFieldInfo> Fields);

/// <summary>
/// One data field: its tag in the map file, the kind of widget it takes, its C# type's name, its
/// value now and the prototype's (as the JSON the component text uses), and whether the game
/// never writes it. An enum and a set of flags give their names, a prototype id its kind.
/// </summary>
public sealed record DataFieldInfo(
    string Tag,
    string Kind,
    string Type,
    JsonNode? Value,
    JsonNode? Prototype,
    bool ReadOnly,
    IReadOnlyList<string>? Names = null,
    string? PrototypeKind = null);

/// <summary>
/// The forms of components: each data field found by reflection, as the engine's own data
/// definitions find them, which are internal to it. The tag is the attribute's, else
/// <see cref="DataDefinitionUtility.AutoGenerateTag"/> of the member's name.
/// </summary>
internal sealed partial class ComponentEditor
{
    /// <summary>A data field. <c>Engine</c> is one every component has from the engine's base class (netsync), which is no mapper's business.</summary>
    private sealed record Member(string Tag, Type Type, DataFieldAttribute Field, bool Engine);

    private static readonly Dictionary<Type, List<Member>> MembersOf = new();
    private readonly Dictionary<string, MappingDataNode> _blank = new();

    /// <summary>
    /// The fields of each component of an entity, or of each named one, sorted by name. A
    /// component the entity lacks shows what it would have: the prototype's, or a new one's.
    /// The ids of a prototype kind are given once: not for those in <paramref name="known"/>,
    /// and never for entity prototypes, which the interface has.
    /// </summary>
    public FieldsAnswer Fields(EntityUid uid, IReadOnlyList<string>? names, IReadOnlySet<string> known)
    {
        var cache = PrototypeNodes(_entMan.GetComponent<MetaDataComponent>(uid).EntityPrototype);
        var registrations = new List<ComponentRegistration>();
        if (names == null)
        {
            foreach (var component in _entMan.GetComponents(uid))
            {
                var registration = _factory.GetRegistration(component.GetType());
                if (component.LifeStage < ComponentLifeStage.Stopping && !registration.Unsaved && !Reserved.Contains(registration.Name))
                    registrations.Add(registration);
            }
        }
        else
        {
            foreach (var name in names)
            {
                if (!_factory.TryGetRegistration(name, out var registration))
                    throw new KernelException($"The engine has no component called '{name}'.");
                registrations.Add(registration);
            }
        }

        registrations.Sort((a, b) => string.CompareOrdinal(a.Name, b.Name));
        var components = new List<ComponentFields>(registrations.Count);
        var ids = new Dictionary<string, IReadOnlyList<string>>();
        foreach (var registration in registrations)
        {
            var before = cache != null && cache.TryGetValue(registration.Name, out var fromPrototype) ? fromPrototype : Blank(registration);
            var now = _entMan.TryGetComponent(uid, registration.Type, out var component)
                ? _serialization.WriteValueAs<MappingDataNode>(registration.Type, component, alwaysWrite: true, context: _context)
                : before;

            JsonNode? Of(MappingDataNode data, string tag) => data.TryGet(tag, out var node) ? ToJson(node) : null;

            var fields = new List<DataFieldInfo>();
            var tags = new HashSet<string>();
            foreach (var member in Members(registration.Type))
            {
                // The tag is still taken, so that the data's copy of it is not shown as JSON below.
                if (!tags.Add(member.Tag) || member.Engine)
                    continue;

                var (kind, choices, prototypeKind) = KindOf(member.Type, member.Field.CustomTypeSerializer);
                fields.Add(new DataFieldInfo(member.Tag, kind, TypeName(member.Type), Of(now, member.Tag), Of(before, member.Tag), member.Field.ReadOnly, choices, prototypeKind));
                if (prototypeKind != null && prototypeKind != "entity" && !known.Contains(prototypeKind) && !ids.ContainsKey(prototypeKind))
                    ids[prototypeKind] = PrototypeIds(prototypeKind);
            }

            // What the data holds and no member names: the fields of a data definition included
            // whole, which the map file writes beside the component's own. Shown as JSON text.
            foreach (var tag in now.Keys.Concat(before.Keys))
            {
                if (tags.Add(tag))
                    fields.Add(new DataFieldInfo(tag, "other", "", Of(now, tag), Of(before, tag), false));
            }

            components.Add(new ComponentFields(registration.Name, fields));
        }

        return new FieldsAnswer(components, ids);
    }

    /// <summary>A new component's data, for one the prototype does not have. As the map save compares it.</summary>
    private MappingDataNode Blank(ComponentRegistration registration)
    {
        if (!_blank.TryGetValue(registration.Name, out var blank))
            _blank[registration.Name] = blank = _serialization.WriteValueAs<MappingDataNode>(registration.Type, _factory.GetComponent(registration), alwaysWrite: true, context: _context);
        return blank;
    }

    private List<string> PrototypeIds(string kind)
    {
        if (!_protos.TryGetKindType(kind, out var type))
            return [];
        var ids = _protos.EnumeratePrototypes(type).Select(prototype => prototype.ID).ToList();
        ids.Sort(string.CompareOrdinal);
        return ids;
    }

    /// <summary>The data fields and properties of a type and its bases, the type's own first.</summary>
    private static List<Member> Members(Type type)
    {
        lock (MembersOf)
        {
            if (MembersOf.TryGetValue(type, out var found))
                return found;

            found = [];
            const BindingFlags flags = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly;
            for (var at = type; at != null && at != typeof(object); at = at.BaseType)
            {
                foreach (var member in at.GetMembers(flags).OrderBy(member => member.MetadataToken))
                {
                    var memberType = member switch
                    {
                        FieldInfo field => field.FieldType,
                        PropertyInfo property => property.PropertyType,
                        _ => null,
                    };
                    if (memberType == null || member.GetCustomAttribute<DataFieldAttribute>(false) is not { } attribute)
                        continue;

                    found.Add(new Member(attribute.Tag ?? DataDefinitionUtility.AutoGenerateTag(member.Name), memberType, attribute, at == typeof(Component)));
                }
            }

            MembersOf[type] = found;
            return found;
        }
    }

    /// <summary>The kind of widget a field takes, with an enum's or a flag set's names, or a prototype id's kind.</summary>
    private (string Kind, IReadOnlyList<string>? Names, string? PrototypeKind) KindOf(Type declared, Type? serializer)
    {
        if (serializer != null)
        {
            if (serializer.IsGenericType)
            {
                var definition = serializer.GetGenericTypeDefinition();
                var argument = serializer.GetGenericArguments()[0];
                try
                {
                    if (definition == typeof(FlagSerializer<>))
                        return ("flags", FlagNames(_serialization.GetFlagTypeFromTag(argument)), null);
                    if (definition == typeof(ConstantSerializer<>))
                        return ("enum", Enum.GetNames(_serialization.GetConstantTypeFromTag(argument)), null);
                }
                catch (KeyNotFoundException)
                {
                    // A tag with no enum for it: the field is shown as text.
                }

                if (declared == typeof(string) && serializer.Name.StartsWith("PrototypeIdSerializer", StringComparison.Ordinal) && _protos.TryGetKindFrom(argument, out var named))
                    return ("prototype", null, named);
            }

            return ("other", null, null);
        }

        var type = Nullable.GetUnderlyingType(declared) ?? declared;
        if (type == typeof(bool))
            return ("bool", null, null);
        if (type == typeof(int) || type == typeof(uint) || type == typeof(long) || type == typeof(ulong) || type == typeof(short) || type == typeof(ushort) || type == typeof(byte) || type == typeof(sbyte))
            return ("int", null, null);
        if (type == typeof(float) || type == typeof(double) || type == typeof(decimal))
            return ("float", null, null);
        if (type == typeof(string))
            return ("string", null, null);
        if (type.IsEnum)
            return type.IsDefined(typeof(FlagsAttribute), false) ? ("flags", FlagNames(type), null) : ("enum", Enum.GetNames(type), null);
        if (type == typeof(Color))
            return ("color", null, null);
        if (type == typeof(Vector2) || type == typeof(Vector2i))
            return ("vector2", null, null);
        if (type == typeof(EntityUid) || type == typeof(NetEntity))
            return ("entity", null, null);
        if (type == typeof(TimeSpan))
            return ("timespan", null, null);
        if (type == typeof(EntProtoId) || type.IsGenericType && type.GetGenericTypeDefinition() == typeof(EntProtoId<>))
            return ("prototype", null, "entity");
        if (type.IsGenericType && type.GetGenericTypeDefinition() == typeof(ProtoId<>) && _protos.TryGetKindFrom(type.GetGenericArguments()[0], out var kind))
            return ("prototype", null, kind);
        return ("other", null, null);
    }

    /// <summary>A flag enum's names, but for one of no flags.</summary>
    private static IReadOnlyList<string> FlagNames(Type type) =>
        Enum.GetNames(type).Where(name => Convert.ToInt64(Enum.Parse(type, name)) != 0).ToArray();

    /// <summary>A type's name as C# writes it: List&lt;string&gt;, float?.</summary>
    private static string TypeName(Type type)
    {
        if (Nullable.GetUnderlyingType(type) is { } inner)
            return TypeName(inner) + "?";
        if (type.IsArray)
            return TypeName(type.GetElementType()!) + "[]";
        if (!type.IsGenericType)
            return type.Name;
        var name = type.Name;
        var tick = name.IndexOf('`');
        return $"{(tick < 0 ? name : name[..tick])}<{string.Join(", ", type.GetGenericArguments().Select(TypeName))}>";
    }
}
