using Content.Server.NodeContainer.Nodes;
using Content.Shared.Atmos;
using Content.Shared.Atmos.Components;
using Content.Shared.NodeContainer;
using Robust.Shared.GameObjects;
using Robust.Shared.Map.Components;
using Robust.Shared.Maths;

namespace SS14.EditorKernel;

/// <summary>
/// Which pipes join, and so which connectors each pipe shows, worked out the way the game's node
/// systems would leave them. No NodeContainerSystem runs here, so a node's current direction and
/// layer are never set on the game's node objects. They are worked out from the node's data and
/// its entity instead, and the game's connection rules are written out here.
/// </summary>
internal sealed partial class VisualsReader
{
    /// <summary>One pipe node on a grid's cell, with its entity.</summary>
    private sealed record PipeEnd(EntityUid Uid, TransformComponent Xform, string Name, PipeNode Node, Vector2i Cell);

    private static readonly PipeDirection[] PipeSides = [PipeDirection.North, PipeDirection.South, PipeDirection.West, PipeDirection.East];

    private static Vector2i Offset(PipeDirection side) => side switch
    {
        PipeDirection.North => new Vector2i(0, 1),
        PipeDirection.South => new Vector2i(0, -1),
        PipeDirection.East => new Vector2i(1, 0),
        PipeDirection.West => new Vector2i(-1, 0),
        _ => Vector2i.Zero,
    };

    /// <summary>
    /// A node's directions as PipeNode keeps them in CurrentPipeDirection: its data turned by
    /// the entity's rotation on the grid, or as it is when it does not turn
    /// (Content.Server/NodeContainer/Nodes/PipeNode.cs:113-122, 144-159).
    /// </summary>
    private static PipeDirection DirectionOf(PipeEnd end) =>
        end.Node.RotationsEnabled ? end.Node.OriginalPipeDirection.RotatePipeDirection(end.Xform.LocalRotation) : end.Node.OriginalPipeDirection;

    /// <summary>
    /// A node's layer as the game's systems leave it at start. The server's
    /// AtmosPipeLayersSystem puts every pipe node of an entity on the entity's layer, unless its
    /// layers are locked (Content.Server/Atmos/EntitySystems/AtmosPipeLayersSystem.cs:33-62, and
    /// the clamp of Content.Shared/Atmos/EntitySystems/SharedAtmosPipeLayersSystem.cs:210).
    /// A plumbing node takes the entity's layer even then, unless it is a manifold's
    /// (Content.Server/_Starlight/Plumbing/Nodes/PlumbingNode.cs:31-40). A plumbing machine
    /// takes the layer of the duct it picks on a side (<see cref="LayerOf"/>).
    /// </summary>
    private AtmosPipeLayer StartLayerOf(PipeEnd end)
    {
        if (!entMan.TryGetComponent(end.Uid, out AtmosPipeLayersComponent? layers))
            return end.Node.CurrentPipeLayer;

        if (TakesEntityLayer(end))
            return layers.CurrentPipeLayer;

        if (layers.PipeLayersLocked)
            return end.Node.CurrentPipeLayer;

        return (AtmosPipeLayer) Math.Clamp((int) layers.CurrentPipeLayer, 0, layers.NumberOfPipeLayers - 1);
    }

    /// <summary>
    /// The pipe layer of an entity whose own AtmosPipeLayers sets one apart from its
    /// prototype's, as a map can save it on a pipe of the first layer. The server's
    /// AtmosPipeLayersSystem gives every such entity its layer as it starts
    /// (Content.Server/Atmos/EntitySystems/AtmosPipeLayersSystem.cs:33-36), and the view, which
    /// makes its copy from the prototype alone, has to be told. Null when it is the prototype's
    /// and none was sent before.
    /// </summary>
    private int? PipeLayer(EntityUid uid, bool sentBefore)
    {
        if (!entMan.TryGetComponent(uid, out AtmosPipeLayersComponent? layers))
            return null;

        var own = entMan.GetComponent<MetaDataComponent>(uid).EntityPrototype is { } prototype
                  && prototype.Components.TryGetValue("AtmosPipeLayers", out var entry)
                  && entry.Component is AtmosPipeLayersComponent its
            ? its.CurrentPipeLayer
            : (AtmosPipeLayer?) null;
        if (layers.CurrentPipeLayer == own && !sentBefore)
            return null;

        return (int) layers.CurrentPipeLayer;
    }

    /// <summary>
    /// Whether a node can join others, as Node.Connectable and PipeNode.Connectable say
    /// (Content.Shared/NodeContainer/Node.cs:33-46, PipeNode.cs:80-83). Written out here: the
    /// game's own asks after the node's owner, which no system has set.
    /// </summary>
    private bool Connectable(EntityUid uid, Node node, TransformComponent xform)
    {
        if (node.Deleting || entMan.IsQueuedForDeletion(uid))
            return false;

        if (node is PipeNode { ConnectionsEnabled: false })
            return false;

        return !node.NeedAnchored || xform.Anchored;
    }

    /// <summary>The pipe nodes of what is anchored on a cell, as PipeNode.PipesInDirection finds them (PipeNode.cs:236-256).</summary>
    private IEnumerable<PipeEnd> PipesOn(EntityUid gridUid, MapGridComponent grid, Vector2i cell)
    {
        foreach (var (uid, node, xform, name) in NamedNodes(gridUid, grid, cell))
        {
            if (node is PipeNode pipe)
                yield return new PipeEnd(uid, xform, name, pipe, cell);
        }
    }

    /// <summary>The pipe nodes of an entity anchored on a grid, or none.</summary>
    private IEnumerable<PipeEnd> PipesOf(EntityUid uid, TransformComponent xform)
    {
        if (!entMan.TryGetComponent(uid, out NodeContainerComponent? container) || CellOf(xform) is not { } at)
            yield break;

        foreach (var (name, node) in container.Nodes)
        {
            if (node is PipeNode pipe)
                yield return new PipeEnd(uid, xform, name, pipe, at.Cell);
        }
    }

    /// <summary>
    /// The nodes a pipe node finds itself, as NodeGroupSystem.GetCompatibleNodes takes them
    /// from GetReachableNodes (Content.Server/NodeContainer/EntitySystems/NodeGroupSystem.cs:349-366):
    /// none if it cannot connect, and of those it finds only the ones of its group that can.
    /// Only those on the cells beside it are kept. A port and a portable thing on one cell join
    /// too (PortPipeNode.cs, PortablePipeNode.cs), which shows no connector.
    /// </summary>
    private IEnumerable<PipeEnd> Compatible(PipeEnd end, EntityUid gridUid, MapGridComponent grid)
    {
        if (!Connectable(end.Uid, end.Node, end.Xform))
            return [];

        var found = IsPlumbing(end)
            ? PlumbingReachable(end, gridUid, grid)
            : PipeReachable(end, gridUid, grid);
        return found.Where(other => other.Node.NodeGroupID == end.Node.NodeGroupID && Connectable(other.Uid, other.Node, other.Xform));
    }

    /// <summary>
    /// PipeNode.GetReachableNodes and LinkableNodesInDirection (PipeNode.cs:161-231): for each
    /// side the pipe has, a pipe on the cell that way of the same group and layer that has the
    /// side facing back.
    /// </summary>
    private IEnumerable<PipeEnd> PipeReachable(PipeEnd end, EntityUid gridUid, MapGridComponent grid)
    {
        if (!end.Xform.Anchored)
            yield break;

        var directions = DirectionOf(end);
        var layer = LayerOf(end, gridUid, grid);
        foreach (var side in PipeSides)
        {
            if (!directions.HasDirection(side))
                continue;

            foreach (var other in PipesOn(gridUid, grid, end.Cell + Offset(side)))
            {
                if (other.Node.NodeGroupID == end.Node.NodeGroupID
                    && LayerOf(other, gridUid, grid) == layer
                    && DirectionOf(other).HasDirection(side.GetOpposite()))
                {
                    yield return other;
                }
            }
        }
    }

    /// <summary>
    /// The nodes beside a pipe node that the game joins it to. The game makes the graph
    /// undirected: a node reaches those it finds and those that find it
    /// (NodeGroupSystem.cs:194-231).
    /// </summary>
    private List<PipeEnd> Reachable(PipeEnd end, EntityUid gridUid, MapGridComponent grid)
    {
        var joined = Compatible(end, gridUid, grid).ToList();
        foreach (var side in PipeSides)
        {
            foreach (var other in PipesOn(gridUid, grid, end.Cell + Offset(side)))
            {
                if (joined.Any(found => found.Node == other.Node))
                    continue;
                if (Compatible(other, gridUid, grid).Any(found => found.Node == end.Node))
                    joined.Add(other);
            }
        }

        return joined;
    }

    /// <summary>
    /// A pipe's connectors, as the server's AtmosPipeAppearanceSystem.UpdateAppearance sets
    /// PipeVisuals.VisualState (Content.Server/Atmos/Piping/EntitySystems/AtmosPipeAppearanceSystem.cs:28-95):
    /// for each node it joins, the side of the grid it lies on, gathered by that node's layer,
    /// four bits a layer with layer i shifted by 4 * i. Layers at or above the entity's number
    /// of pipe layers are left out: one without AtmosPipeLayers. Null if it is no pipe on a grid.
    /// </summary>
    private int? PipeState(EntityUid uid, TransformComponent xform)
    {
        if (!entMan.HasComponent<PipeAppearanceComponent>(uid)
            || !entMan.HasComponent<AppearanceComponent>(uid)
            || !entMan.HasComponent<NodeContainerComponent>(uid)
            || !entMan.TryGetComponent(xform.GridUid, out MapGridComponent? grid))
        {
            return null;
        }

        var ends = PipesOf(uid, xform).ToList();
        if (ends.Count == 0)
            return null;

        // SharedAtmosPipeAppearanceSystem.GetNumberOfPipeLayers (Content.Shared/Atmos/EntitySystems/SharedAtmosPipeAppearanceSystem.cs:14-17).
        var layers = entMan.TryGetComponent(uid, out AtmosPipeLayersComponent? pipeLayers) ? pipeLayers.NumberOfPipeLayers : 1;
        var connected = new PipeDirection[layers];
        var gridUid = xform.GridUid!.Value;
        foreach (var end in ends)
        {
            foreach (var other in Reachable(end, gridUid, grid))
            {
                var index = (int) LayerOf(other, gridUid, grid);
                if (index >= layers)
                    continue;

                var offset = other.Cell - end.Cell;
                connected[index] |= PipeSides.FirstOrDefault(side => Offset(side) == offset, PipeDirection.None);
            }
        }

        var state = 0;
        for (var i = layers - 1; i >= 0; i--)
            state += (int) connected[i] << (PipeDirectionHelpers.PipeDirections * i);
        return state;
    }
}
