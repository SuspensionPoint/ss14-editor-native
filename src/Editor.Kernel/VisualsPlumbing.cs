// Only a fork with Starlight's plumbing has it (src/Directory.Build.props). Without it,
// VisualsNoPlumbing.cs says no entity is plumbing.
#if FORK_PLUMBING
using Content.Server._Starlight.Plumbing.Components;
using Content.Server._Starlight.Plumbing.Nodes;
using Content.Shared._Starlight.Plumbing;
using Content.Shared._Starlight.Plumbing.Components;
using Content.Shared.Atmos;
using Content.Shared.Atmos.Components;
using Content.Shared.Maps;
using Content.Shared.NodeContainer;
using Content.Shared.SubFloor;
using Content.Shared.Tag;
using Robust.Shared.GameObjects;
using Robust.Shared.Map.Components;
using Robust.Shared.Prototypes;

namespace SS14.EditorKernel;

/// <summary>
/// Starlight's plumbing, which joins its ducts and machines by its own rules. Paths are under
/// upstream/starlight/Content.Server/_Starlight/Plumbing.
/// </summary>
internal sealed partial class VisualsReader
{
    private static readonly ProtoId<TagPrototype> DuctTag = "PlumbingDuct";

    /// <summary>The order PlumbingNode.GetCardinalDirections gives a node's sides in (Nodes/PlumbingNode.cs:182-192).</summary>
    private static readonly PipeDirection[] PlumbingSides = [PipeDirection.North, PipeDirection.South, PipeDirection.East, PipeDirection.West];

    // The manifold's two sides (EntitySystems/PlumbingConnectorAppearanceSystem.cs:33-34).
    private const PipeDirection ManifoldSideA = PipeDirection.North;
    private const PipeDirection ManifoldSideB = PipeDirection.South;

    /// <summary>The plumbing machines of a map, whose connectors are looked at.</summary>
    private void AddPlumbingMachines(EntityUid mapUid, HashSet<EntityUid> things)
    {
        var plumbing = entMan.AllEntityQueryEnumerator<PlumbingConnectorAppearanceComponent, TransformComponent>();
        while (plumbing.MoveNext(out var uid, out _, out var xform))
        {
            if (xform.MapUid == mapUid)
                things.Add(uid);
        }
    }

    private static bool IsPlumbing(PipeEnd end) => end.Node is PlumbingNode;

    /// <summary>A plumbing node takes the entity's layer, unless it is a manifold's (Nodes/PlumbingNode.cs:31-40).</summary>
    private bool TakesEntityLayer(PipeEnd end) => end.Node is PlumbingNode && !entMan.HasComponent<PlumbingManifoldComponent>(end.Uid);

    /// <summary>
    /// A plumbing machine's connectors, as Starlight's server PlumbingConnectorAppearanceSystem
    /// sets them (EntitySystems/PlumbingConnectorAppearanceSystem.cs:124-254): the sides its
    /// plumbing nodes have, those joined to a node of their layer on the cell beside, the layer
    /// of each joined side, its inlets, outlets and mixing inlets, and whether a floor covers
    /// it. A manifold says which of its slots are joined instead. Null if it is no such machine
    /// on a grid.
    /// </summary>
    private List<SceneAppearance>? PlumbingLooks(EntityUid uid, TransformComponent xform)
    {
        if (!entMan.HasComponent<PlumbingConnectorAppearanceComponent>(uid)
            || !entMan.HasComponent<AppearanceComponent>(uid)
            || !entMan.HasComponent<NodeContainerComponent>(uid)
            || !entMan.TryGetComponent(xform.GridUid, out MapGridComponent? grid)
            || CellOf(xform) is not { } at)
        {
            return null;
        }

        var gridUid = at.Grid;
        // HasFloorCover (:117-122): any tile that is not a sub-floor covers it.
        var tile = map.GetTileRef(gridUid, grid, at.Cell);
        var covered = !((ContentTileDefinition) tiles[tile.Tile.TypeId]).IsSubFloor;
        var nodes = PipesOf(uid, xform).Where(end => end.Node is PlumbingNode).ToList();
        var nodeDirections = PipeDirection.None;
        var connectedDirections = PipeDirection.None;
        var connectedLayers = 0;

        if (entMan.TryGetComponent(uid, out PlumbingManifoldComponent? manifold))
        {
            // UpdateManifoldAppearance (:193-255).
            var sideA = ManifoldSideA.RotatePipeDirection(xform.LocalRotation);
            var sideB = ManifoldSideB.RotatePipeDirection(xform.LocalRotation);
            var slots = 0;
            foreach (var end in nodes)
            {
                var direction = DirectionOf(end);
                nodeDirections |= direction;
                var joined = ConnectedDirections(end, direction, gridUid, grid, ref connectedLayers);
                connectedDirections |= joined;
                if (!joined.HasDirection(direction))
                    continue;

                var layer = LayerOf(end, gridUid, grid);
                if (Named(end.Name, manifold.SideANodeNames) || direction.HasDirection(sideA))
                    slots = SlotJoined(slots, direction, SlotOf(layer));
                if (Named(end.Name, manifold.SideBNodeNames) || direction.HasDirection(sideB))
                    slots = SlotJoined(slots, direction, SlotOf(layer));
            }

            return
            [
                new(nameof(PlumbingVisuals.NodeDirections), (int) nodeDirections),
                new(nameof(PlumbingVisuals.ConnectedDirections), (int) connectedDirections),
                new(nameof(PlumbingVisuals.ConnectedLayerByDirection), connectedLayers),
                new(nameof(PlumbingVisuals.InletDirections), (int) sideA),
                new(nameof(PlumbingVisuals.OutletDirections), (int) sideB),
                new(nameof(PlumbingVisuals.MixingInletDirections), 0),
                new(nameof(PlumbingVisuals.ManifoldMode), 1),
                new(nameof(PlumbingVisuals.ManifoldConnectedSlotsByDirection), slots),
                new(nameof(PlumbingVisuals.CoveredByFloor), covered ? 1 : 0),
            ];
        }

        // UpdateAppearance (:144-190) and the names of its inlets, outlets and mixing inlets (:368-408).
        var inletNames = entMan.TryGetComponent(uid, out PlumbingInletComponent? inlet) ? inlet.InletNames.ToList() : [];
        var outletNames = entMan.TryGetComponent(uid, out PlumbingOutletComponent? outlet) ? outlet.OutletNames.ToList() : [];
        var mixingNames = entMan.TryGetComponent(uid, out PlumbingPillPressComponent? press)
            ? new[] { press.InletEastNodeName, press.InletWestNodeName }.Where(name => !string.IsNullOrWhiteSpace(name)).ToList()
            : [];
        var inlets = PipeDirection.None;
        var outlets = PipeDirection.None;
        var mixing = PipeDirection.None;
        foreach (var end in nodes)
        {
            var direction = DirectionOf(end);
            nodeDirections |= direction;
            if (Named(end.Name, mixingNames))
                mixing |= direction;
            else if (Named(end.Name, inletNames))
                inlets |= direction;
            else if (Named(end.Name, outletNames))
                outlets |= direction;

            connectedDirections |= ConnectedDirections(end, direction, gridUid, grid, ref connectedLayers);
        }

        return
        [
            new(nameof(PlumbingVisuals.NodeDirections), (int) nodeDirections),
            new(nameof(PlumbingVisuals.ConnectedDirections), (int) connectedDirections),
            new(nameof(PlumbingVisuals.ConnectedLayerByDirection), connectedLayers),
            new(nameof(PlumbingVisuals.InletDirections), (int) inlets),
            new(nameof(PlumbingVisuals.OutletDirections), (int) outlets),
            new(nameof(PlumbingVisuals.MixingInletDirections), (int) mixing),
            new(nameof(PlumbingVisuals.ManifoldMode), 0),
            new(nameof(PlumbingVisuals.CoveredByFloor), covered ? 1 : 0),
        ];
    }

    private static bool Named(string name, IEnumerable<string> names) =>
        names.Any(configured => name.Equals(configured, StringComparison.OrdinalIgnoreCase));

    /// <summary>
    /// GetConnectedDirections (:268-306): each side of the node that a plumbing node of another
    /// entity it reaches lies on, of the node's own layer, with that layer plus one in the
    /// side's four bits, north lowest, then south, east and west.
    /// </summary>
    private PipeDirection ConnectedDirections(PipeEnd end, PipeDirection directions, EntityUid gridUid, MapGridComponent grid, ref int layers)
    {
        var connected = PipeDirection.None;
        var layer = LayerOf(end, gridUid, grid);
        var reached = Reachable(end, gridUid, grid);
        foreach (var side in PlumbingSides)
        {
            if (!directions.HasFlag(side))
                continue;

            if (reached.Any(other => other.Node is PlumbingNode && other.Uid != end.Uid && LayerOf(other, gridUid, grid) == layer && other.Cell == end.Cell + Offset(side)))
            {
                connected |= side;
                var shift = NibbleShift(side);
                layers = (layers & ~(0xF << shift)) | ((((int) layer + 1) & 0xF) << shift);
            }
        }

        return connected;
    }

    private static int NibbleShift(PipeDirection side) => side switch
    {
        PipeDirection.North => 0,
        PipeDirection.South => 4,
        PipeDirection.East => 8,
        _ => 12,
    };

    /// <summary>SetManifoldSlotConnected (:327-346). A direction of more than one side sets nothing, as in the game.</summary>
    private static int SlotJoined(int slots, PipeDirection direction, int slot)
    {
        if (direction is not (PipeDirection.North or PipeDirection.South or PipeDirection.East or PipeDirection.West) || (uint) slot > 3)
            return slots;
        return slots | ((1 << slot) << NibbleShift(direction));
    }

    /// <summary>GetSlotIndexForLayer (:348-366) for the three slots a side the game gives.</summary>
    private static int SlotOf(AtmosPipeLayer layer) => layer switch
    {
        AtmosPipeLayer.Tertiary => 0,
        AtmosPipeLayer.Secondary => 2,
        _ => 1,
    };

    /// <summary>
    /// Whether an entity is on the editor's Plumbing layer: it lives under the floor
    /// (SubFloorHide) and has a plumbing node. That is every duct and every plumbing machine of
    /// the fork's plumbing folder, and leaves out the machines of the floor above that only take
    /// plumbing, such as the chem master, the chem dispenser, the grinder and the gas condenser.
    /// </summary>
    private bool OnPlumbingLayer(EntityUid uid) =>
        entMan.HasComponent<SubFloorHideComponent>(uid)
        && entMan.TryGetComponent(uid, out NodeContainerComponent? container)
        && container.Nodes.Values.Any(node => node is PlumbingNode);

    /// <summary>Whether it is a duct, as the game's tag says. No tag system runs here, so the tags are read.</summary>
    private bool IsDuct(EntityUid uid) => entMan.TryGetComponent(uid, out TagComponent? tags) && tags.Tags.Contains(DuctTag);

    /// <summary>A plumbing node of a machine, which picks one duct a side (Nodes/PlumbingNode.cs:84-136).</summary>
    private bool IsPlumbingMachine(PipeEnd end) =>
        end.Node is PlumbingNode && end.Xform.Anchored && !IsDuct(end.Uid) && Connectable(end.Uid, end.Node, end.Xform);

    /// <summary>
    /// The node a machine's plumbing node picks on one of its sides: the first pipe node on the
    /// cell that way that faces back and is of its group, whatever its layer
    /// (Nodes/PlumbingNode.cs:111-133). The game keeps the pick in PlumbingManifoldSystem for the
    /// ducts to ask. It is worked out again here when asked.
    /// </summary>
    private PipeEnd? Picked(PipeEnd machine, PipeDirection side, EntityUid gridUid, MapGridComponent grid)
    {
        foreach (var other in PipesOn(gridUid, grid, machine.Cell + Offset(side)))
        {
            if (DirectionOf(other).HasDirection(side.GetOpposite()) && other.Node.NodeGroupID == machine.Node.NodeGroupID)
                return other;
        }

        return null;
    }

    /// <summary>
    /// A node's layer once its reachable nodes have been found. A machine's plumbing node takes
    /// the layer of each duct it picks, so the last side picked in its order decides
    /// (Nodes/PlumbingNode.cs:127). A picked machine is taken at its first layer, so that two
    /// machines that pick each other do not ask each other without end.
    /// </summary>
    private AtmosPipeLayer LayerOf(PipeEnd end, EntityUid gridUid, MapGridComponent grid)
    {
        var layer = StartLayerOf(end);
        if (!IsPlumbingMachine(end))
            return layer;

        var directions = DirectionOf(end);
        foreach (var side in PlumbingSides)
        {
            if (directions.HasDirection(side) && Picked(end, side, gridUid, grid) is { } picked)
                layer = StartLayerOf(picked);
        }

        return layer;
    }

    /// <summary>
    /// PlumbingNode.GetReachableNodes (Nodes/PlumbingNode.cs:43-180), of the nodes on the cells
    /// beside it. A machine's node reaches the node it picks on each side and nothing else. A
    /// duct reaches a machine's node that faces it unless the machine picked another on that
    /// side, and then whatever a plain pipe would. The links within one machine (a manifold's
    /// two sides, outlets joined together) show no connector and are left out.
    /// </summary>
    private IEnumerable<PipeEnd> PlumbingReachable(PipeEnd end, EntityUid gridUid, MapGridComponent grid)
    {
        if (!end.Xform.Anchored)
            yield break;

        var directions = DirectionOf(end);
        if (!IsDuct(end.Uid))
        {
            foreach (var side in PlumbingSides)
            {
                if (directions.HasDirection(side) && Picked(end, side, gridUid, grid) is { } picked)
                    yield return picked;
            }

            yield break;
        }

        var found = new List<PipeEnd>();
        foreach (var side in PlumbingSides)
        {
            if (!directions.HasDirection(side))
                continue;

            foreach (var other in PipesOn(gridUid, grid, end.Cell + Offset(side)))
            {
                if (other.Node.NodeGroupID != end.Node.NodeGroupID
                    || !DirectionOf(other).HasDirection(side.GetOpposite())
                    || IsDuct(other.Uid))
                {
                    continue;
                }

                // The game asks the pick its machine made on that side, if it made one.
                if (other.Node is PlumbingNode && other.Xform.Anchored
                    && Picked(other, side.GetOpposite(), gridUid, grid) is { } picked && picked.Uid != end.Uid)
                {
                    continue;
                }

                found.Add(other);
            }
        }

        foreach (var other in PipeReachable(end, gridUid, grid))
        {
            if (!found.Any(already => already.Node == other.Node))
                found.Add(other);
        }

        foreach (var other in found)
            yield return other;
    }
}
#endif
