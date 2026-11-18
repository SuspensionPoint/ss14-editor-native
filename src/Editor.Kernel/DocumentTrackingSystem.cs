using Robust.Shared.GameObjects;

namespace SS14.EditorKernel;

/// <summary>
/// Relays engine events that can only be subscribed to from an entity system. The engine locks
/// its event bus once it has started, so <see cref="DocumentManager"/> can't subscribe by itself.
/// </summary>
public sealed class DocumentTrackingSystem : EntitySystem
{
    public delegate void AnchorChangedHandler(ref AnchorStateChangedEvent ev);

    /// <summary>Raised whenever any entity is anchored to, or released from, a grid tile.</summary>
    public event AnchorChangedHandler? AnchorChanged;

    public override void Initialize()
    {
        base.Initialize();
        SubscribeLocalEvent<AnchorStateChangedEvent>(OnAnchorChanged);
    }

    private void OnAnchorChanged(ref AnchorStateChangedEvent ev) => AnchorChanged?.Invoke(ref ev);
}
