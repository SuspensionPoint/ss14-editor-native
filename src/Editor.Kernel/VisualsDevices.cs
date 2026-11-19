using System.Globalization;
using System.Reflection;
using Content.Shared.Doors.Components;
using Robust.Shared.Containers;
using Robust.Shared.GameObjects;
using Robust.Shared.Maths;

namespace SS14.EditorKernel;

/// <summary>
/// Appearance values the game's server sets as a map loads, which a client cannot work out
/// from the saved data it is sent (SavedState.cs), sent as <see cref="SceneVisuals.Appearance"/>.
///
/// In mapping mode the map is loaded paused and not map-initialized, so only what the server's
/// systems set on ComponentInit or ComponentStartup reaches the client. A survey of those systems
/// in Starlight and Space Wizards found these that a mapped station shows and that depend on the
/// map alone:
///
/// - A gas filter, mixer, pressure-controlled valve and pressure regulator set their light from
///   whether they are on. Content.Server/Atmos/Piping/Trinary/EntitySystems/GasFilterSystem.cs:50-53
///   and :172-178, GasMixerSystem.cs:49-52 and :188, PressureControlledValveSystem.cs:29-32 and
///   :90, Content.Server/Atmos/Piping/Binary/EntitySystems/GasPressureRegulatorSystem.cs:70-73 and
///   :159-161. The components live in Content.Server on some forks, so they are found by name.
/// - A door shows its state. The shared SharedDoorSystem.OnComponentInit sets it
///   (Content.Shared/Doors/Systems/SharedDoorSystem.cs:82-110), but the client's DoorSystem
///   overrides that handler without it (Content.Client/Doors/DoorSystem.cs:24), so on a client
///   only the server's copy sets it. Without it a door saved open is drawn closed.
/// - A cabinet shows whether its slot holds something
///   (Content.Shared/Cabinet/ItemCabinetSystem.cs:32-46). The client runs this itself, but the
///   view shows nothing inside a container, so the client would always find the slot empty.
///
/// What depends on power, the station, the date or chance (thrusters, computer screens, alert
/// level displays, kudzu, random looks) is left as the client has it.
/// </summary>
internal sealed partial class VisualsReader
{
    /// <summary>A component whose bool field the server copies into an appearance key on init.</summary>
    private sealed record Switch(string Component, string Field, string KeyType, string Key);

    private static readonly Switch[] Switches =
    [
        new("GasFilter", "Enabled", "Content.Shared.Atmos.Piping.FilterVisuals", "Enabled"),
        new("GasMixer", "Enabled", "Content.Shared.Atmos.Piping.FilterVisuals", "Enabled"),
        new("PressureControlledValve", "Enabled", "Content.Shared.Atmos.Piping.FilterVisuals", "Enabled"),
        new("GasPressureRegulator", "Enabled", "Content.Shared.Atmos.Piping.PressureRegulatorVisuals", "State"),
    ];

    private const string Cabinet = "ItemCabinet";

    /// <summary>The component types of <see cref="Switches"/> and the cabinet the fork has, with the field read, found once.</summary>
    private List<(Type Type, MemberInfo? Field, Switch? Rule)>? _devices;

    private List<(Type Type, MemberInfo? Field, Switch? Rule)> Devices()
    {
        if (_devices != null)
            return _devices;

        _devices = new();
        var factory = entMan.ComponentFactory;
        foreach (var rule in Switches)
        {
            if (!factory.TryGetRegistration(rule.Component, out var registration))
                continue;

            var field = (MemberInfo?) registration.Type.GetField(rule.Field, BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic)
                        ?? registration.Type.GetProperty(rule.Field, BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic);
            if (field != null)
                _devices.Add((registration.Type, field, rule));
        }

        _devices.Add((typeof(DoorComponent), null, null));
        if (factory.TryGetRegistration(Cabinet, out var cabinet))
            _devices.Add((cabinet.Type, cabinet.Type.GetField("Slot", BindingFlags.Instance | BindingFlags.Public), null));
        return _devices;
    }

    /// <summary>The map's entities that have a component of <see cref="Devices"/>.</summary>
    private void AddDevices(EntityUid mapUid, HashSet<EntityUid> into)
    {
        foreach (var (type, _, _) in Devices())
        {
            foreach (var (uid, _) in entMan.GetAllComponents(type, includePaused: true))
            {
                if (entMan.GetComponent<TransformComponent>(uid).MapUid == mapUid)
                    into.Add(uid);
            }
        }
    }

    /// <summary>What the server would set of an entity's appearance on init, or null if it says nothing.</summary>
    private IReadOnlyList<AppearanceValue>? DeviceLooks(EntityUid uid)
    {
        if (!entMan.HasComponent<AppearanceComponent>(uid))
            return null;

        List<AppearanceValue>? values = null;
        foreach (var (type, field, rule) in Devices())
        {
            if (!entMan.TryGetComponent(uid, type, out var component))
                continue;

            values ??= new();
            if (component is DoorComponent door)
            {
                // A door stuck opening or closing with no time to change is put open or closed.
                var state = door.NextStateChange != null ? door.State : door.State switch
                {
                    DoorState.Opening => DoorState.Open,
                    DoorState.Closing => DoorState.Closed,
                    _ => door.State,
                };
                if (Encode(DoorVisuals.State, state) is { } value)
                    values.Add(value);
            }
            else if (rule != null)
            {
                var on = field switch
                {
                    FieldInfo info => info.GetValue(component),
                    PropertyInfo info => info.GetValue(component),
                    _ => null,
                };
                if (on is bool enabled)
                    values.Add(new AppearanceValue(rule.KeyType, rule.Key, "bool", enabled ? "true" : "false"));
            }
            else if ((field as FieldInfo)?.GetValue(component) is string slot)
            {
                // ItemCabinetSystem.HasItem: the slot's container holds something.
                var held = entMan.TryGetComponent(uid, out ContainerManagerComponent? containers)
                           && containers.Containers.TryGetValue(slot, out var container)
                           && container.ContainedEntities.Count > 0;
                values.Add(new AppearanceValue("Content.Shared.Cabinet.ItemCabinetVisuals", "ContainsItem", "bool", held ? "true" : "false"));
            }
        }

        return values is { Count: > 0 } ? values : null;
    }

    /// <summary>
    /// An appearance value as text, for any of the kinds the game's appearance data holds that
    /// the view can read back. Null for another kind, which <see cref="UnknownKinds"/> counts.
    /// </summary>
    public static AppearanceValue? Encode(Enum key, object value)
    {
        var keyType = key.GetType().FullName!;
        var member = key.ToString();
        var (kind, text, valueType) = value switch
        {
            bool b => ("bool", b ? "true" : "false", null),
            int i => ("int", i.ToString(CultureInfo.InvariantCulture), null),
            float f => ("float", f.ToString("R", CultureInfo.InvariantCulture), null),
            double d => ("double", d.ToString("R", CultureInfo.InvariantCulture), null),
            string s => ("string", s, null),
            Enum e => ("enum", e.ToString(), e.GetType().FullName),
            Color c => ("color", c.ToHex(), null),
            System.Numerics.Vector2 v => ("vector2", $"{v.X.ToString("R", CultureInfo.InvariantCulture)},{v.Y.ToString("R", CultureInfo.InvariantCulture)}", null),
            _ => ((string?) null, (string?) null, (string?) null),
        };

        if (kind == null)
        {
            var name = value.GetType().FullName ?? value.GetType().Name;
            lock (UnknownKinds)
                UnknownKinds[name] = UnknownKinds.GetValueOrDefault(name) + 1;
            return null;
        }

        return new AppearanceValue(keyType, member, kind, text!, valueType);
    }

    /// <summary>Value types <see cref="Encode"/> was given and could not write, counted.</summary>
    public static readonly Dictionary<string, int> UnknownKinds = new();
}
