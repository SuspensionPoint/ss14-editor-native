// A fork without Starlight's plumbing (src/Directory.Build.props): no entity is plumbing, none
// has plumbing connectors and none is on the editor's Plumbing layer. VisualsPlumbing.cs is the
// same with the plumbing.
#if !FORK_PLUMBING
using Content.Shared.Atmos.Components;
using Robust.Shared.GameObjects;
using Robust.Shared.Map.Components;

namespace SS14.EditorKernel;

internal sealed partial class VisualsReader
{
    private static void AddPlumbingMachines(EntityUid mapUid, HashSet<EntityUid> things)
    {
    }

    private static bool IsPlumbing(PipeEnd end) => false;

    private static bool TakesEntityLayer(PipeEnd end) => false;

    private static List<SceneAppearance>? PlumbingLooks(EntityUid uid, TransformComponent xform) => null;

    private static bool OnPlumbingLayer(EntityUid uid) => false;

    private static bool IsPlumbingMachine(PipeEnd end) => false;

    private AtmosPipeLayer LayerOf(PipeEnd end, EntityUid gridUid, MapGridComponent grid) => StartLayerOf(end);

    private static IEnumerable<PipeEnd> PlumbingReachable(PipeEnd end, EntityUid gridUid, MapGridComponent grid) => [];
}
#endif
