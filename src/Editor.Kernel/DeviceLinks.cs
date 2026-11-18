using Content.Shared.DeviceLinking;
using Content.Shared.Popups;
using Robust.Shared.GameObjects;
using Robust.Shared.Map;
using Robust.Shared.Player;
#if FORK_SEALED_DEVICE_LINK_SYSTEM
using Content.Shared.DeviceLinking.Events;
using Robust.Shared.IoC;
using Robust.Shared.Prototypes;
using Robust.Shared.Utility;
#endif

namespace SS14.EditorKernel;

/// <summary>
/// The game's device link system, used for its methods only. The kernel calls ToggleLink, the
/// same call the game's network configurator makes, so links are made and refused by the game's
/// rules.
///
/// What it leaves out is everything the system does on its own. On a game server it drops links
/// it finds wrong in a loaded map, and unlinks things as their components go away. A map in the
/// editor must load and save exactly as it is, so none of those events are subscribed to. The
/// server's own system adds signal sending on top of this and needs the device network running,
/// which the kernel doesn't do.
/// </summary>
#if FORK_SEALED_DEVICE_LINK_SYSTEM
public sealed class EditorDeviceLinkSystem : EntitySystem
{
    [Dependency] private readonly SharedTransformSystem _transform = default!;

    // The game's DeviceLinkSystem is sealed (src/Directory.Build.props) and starts with the
    // handlers this kernel must not run, so its ToggleLink is written out here with its rules:
    // both ports there, the sink in the source's range, and the link refused when the source or
    // the sink cancels the game's LinkAttemptEvent. No admin log, popup or new link event.
    public bool ToggleLink(EntityUid? userId, EntityUid sourceUid, EntityUid sinkUid, ProtoId<SourcePortPrototype> sourcePort, ProtoId<SinkPortPrototype> sinkPort, DeviceLinkSourceComponent source, DeviceLinkSinkComponent sink)
    {
        var link = new DeviceLink(sourcePort, sinkPort);
        if (source.LinkedPorts.TryGetValue(sinkUid, out var existing) && existing.Contains(link))
        {
            if (source.Outputs.TryGetValue(sourcePort, out var outputs))
                outputs.Remove(sinkUid);

            existing.Remove(link);
            if (existing.Count == 0)
            {
                source.LinkedPorts.Remove(sinkUid);
                sink.LinkedSources.Remove(sourceUid);
            }

            return true;
        }

        if (!source.Ports.Contains(sourcePort) || !sink.Ports.Contains(sinkPort))
            return false;

        if (!_transform.GetMapCoordinates(sourceUid).InRange(_transform.GetMapCoordinates(sinkUid), source.Range))
            return false;

        var attempt = new LinkAttemptEvent(userId, sourceUid, sourcePort, sinkUid, sinkPort);
        RaiseLocalEvent(sourceUid, ref attempt, true);
        if (attempt.Cancelled)
            return false;

        RaiseLocalEvent(sinkUid, ref attempt, true);
        if (attempt.Cancelled)
            return false;

        source.Outputs.GetOrNew(sourcePort).Add(sinkUid);
        source.LinkedPorts.GetOrNew(sinkUid).Add(link);
        sink.LinkedSources.Add(sourceUid);
        return true;
    }
}
#else
public sealed class EditorDeviceLinkSystem : SharedDeviceLinkSystem
{
    public override void Initialize()
    {
    }
}
#endif

/// <summary>
/// The game's device link system tells a player what it did in a popup. It is never given a
/// player here, but it will not start without a popup system. This one shows nothing.
/// </summary>
public sealed class EditorPopupSystem : SharedPopupSystem
{
#if FORK_PREDICTED_POPUPS
    // The shape of the Space Wizards' self-predicting popups (src/Directory.Build.props): fewer
    // overloads, each recipient optional, and the rest built on them by the shared system.
    public override void PopupCursor(string? message, EntityUid? recipient, PopupType type = PopupType.Small) { }
    public override void PopupCursor(string? message, ICommonSession recipient, PopupType type = PopupType.Small) { }
    public override void PopupCursor(string? message, Filter filter, bool recordReplay, PopupType type = PopupType.Small) { }
    public override void PopupCoordinates(string? message, EntityCoordinates coordinates, PopupType type = PopupType.Small, int predictionKey = 0) { }
    public override void PopupCoordinates(string? message, EntityCoordinates coordinates, EntityUid? recipient, PopupType type = PopupType.Small, int predictionKey = 0) { }
    public override void PopupCoordinates(string? message, EntityCoordinates coordinates, ICommonSession recipient, PopupType type = PopupType.Small, int predictionKey = 0) { }
    public override void PopupCoordinates(string? message, EntityCoordinates coordinates, Filter filter, bool recordReplay, PopupType type = PopupType.Small, int predictionKey = 0) { }
    public override void PopupEntity(string? message, EntityUid uid, PopupType type = PopupType.Small) { }
    public override void PopupEntity(string? message, EntityUid uid, EntityUid? recipient, PopupType type = PopupType.Small) { }
    public override void PopupEntity(string? message, EntityUid uid, ICommonSession recipient, PopupType type = PopupType.Small) { }
    public override void PopupEntity(string? message, EntityUid uid, Filter filter, bool recordReplay, PopupType type = PopupType.Small) { }
#else
    public override void PopupCursor(string? message, PopupType type = PopupType.Small) { }
    public override void PopupCursor(string? message, ICommonSession recipient, PopupType type = PopupType.Small) { }
    public override void PopupCursor(string? message, EntityUid recipient, PopupType type = PopupType.Small) { }
    public override void PopupPredictedCursor(string? message, ICommonSession recipient, PopupType type = PopupType.Small) { }
    public override void PopupPredictedCursor(string? message, EntityUid recipient, PopupType type = PopupType.Small) { }
    public override void PopupCoordinates(string? message, EntityCoordinates coordinates, PopupType type = PopupType.Small) { }
    public override void PopupCoordinates(string? message, EntityCoordinates coordinates, Filter filter, bool recordReplay, PopupType type = PopupType.Small) { }
    public override void PopupCoordinates(string? message, EntityCoordinates coordinates, EntityUid recipient, PopupType type = PopupType.Small) { }
    public override void PopupCoordinates(string? message, EntityCoordinates coordinates, ICommonSession recipient, PopupType type = PopupType.Small) { }
    public override void PopupPredictedCoordinates(string? message, EntityCoordinates coordinates, EntityUid? recipient, PopupType type = PopupType.Small) { }
    public override void PopupEntity(string? message, EntityUid uid, PopupType type = PopupType.Small) { }
    public override void PopupEntity(string? message, EntityUid uid, EntityUid recipient, PopupType type = PopupType.Small) { }
    public override void PopupEntity(string? message, EntityUid uid, ICommonSession recipient, PopupType type = PopupType.Small) { }
    public override void PopupEntity(string? message, EntityUid uid, Filter filter, bool recordReplay, PopupType type = PopupType.Small) { }
    public override void PopupClient(string? message, EntityUid? recipient, PopupType type = PopupType.Small) { }
    public override void PopupClient(string? message, EntityUid uid, EntityUid? recipient, PopupType type = PopupType.Small) { }
    public override void PopupClient(string? message, EntityCoordinates coordinates, EntityUid? recipient, PopupType type = PopupType.Small) { }
    public override void PopupPredicted(string? message, EntityUid uid, EntityUid? recipient, PopupType type = PopupType.Small) { }
    public override void PopupPredicted(string? message, EntityUid uid, EntityUid? recipient, Filter filter, bool recordReplay, PopupType type = PopupType.Small) { }
    public override void PopupPredicted(string? recipientMessage, string? othersMessage, EntityUid uid, EntityUid? recipient, PopupType type = PopupType.Small) { }
#endif
}
