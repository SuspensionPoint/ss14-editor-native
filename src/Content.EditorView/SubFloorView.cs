using System.Reflection;
using Content.Client.SubFloor;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;
using Robust.Shared.Log;

namespace Content.EditorView;

/// <summary>
/// Shows or hides what sits under floor tiles (pipes, cables, disposals), the way the game's
/// mapping tools do. The game normally flips this when a server message arrives, so there is no
/// public call that works without a server. Forks also store the setting differently: upstream has
/// a ShowAll property, Starlight a per-layer mask. This looks for either one by name, so it keeps
/// working (or quietly does nothing) as the fork's client code changes.
/// </summary>
public static class SubFloorView
{
    private const BindingFlags Any = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;

    public static void Show(bool show)
    {
        try
        {
            var system = IoCManager.Resolve<IEntityManager>().System<SubFloorHideSystem>();
            var type = system.GetType();

            if (type.GetField("_showLayers", Any) is { } mask)
            {
                // Starlight: an enum of layers, where every bit set means everything is shown.
                var all = Enum.GetValues(mask.FieldType).Cast<object>().Aggregate(0, (bits, value) => bits | Convert.ToInt32(value));
                mask.SetValue(system, Enum.ToObject(mask.FieldType, show ? all : 0));
            }
            else if (type.GetField("_showAll", Any) is { } flag)
            {
                flag.SetValue(system, show);
            }
            else
            {
                return;
            }

            type.GetMethod("UpdateAll", Any, Type.EmptyTypes)?.Invoke(system, null);
        }
        catch (Exception e)
        {
            Logger.GetSawmill("editor.view").Warning($"Could not change subfloor visibility: {e.Message}");
        }
    }
}
