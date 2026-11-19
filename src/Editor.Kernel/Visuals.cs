using Content.Server.Atmos.Piping.Components;
using Content.Shared.Atmos.Components;
using Content.Server.Power.Components;
using Content.Server.Power.Nodes;
using Content.Shared.NodeContainer;
using Content.Shared.SubFloor;
using Content.Shared.Wires;
using Robust.Shared.GameObjects;
using Robust.Shared.Map;
using Robust.Shared.Map.Components;
using Robust.Shared.Maths;

namespace SS14.EditorKernel;

/// <summary>
/// How cables and pipes look, as the game's server would work it out and send it to clients as
/// appearance data: which sides a cable joins on, and a pipe's connectors and colour. No node or
/// pipe system runs here, so it is worked out from the components using the game's rules, with
/// the game's lines cited. Paths are under upstream/starlight. The view kernel applies the result
/// with the client's appearance system (<see cref="SceneVisuals"/>).
///
/// What was last sent for each entity is remembered, so after an edit only what changed is sent
/// again. An edit can change the look of things it never named, since a newly laid cable joins
/// its neighbours. So the cells an edit touched are checked again, with one cell of margin all
/// round.
/// </summary>
internal sealed partial class VisualsReader(IEntityManager entMan, SharedMapSystem map, ITileDefinitionManager tiles)
{
    /// <summary>What was sent for one entity, and the cell it was on, to look at again when it goes.</summary>
    internal sealed record Sent(SceneVisuals Visuals, EntityUid Grid, Vector2i Cell);

    private static readonly (Direction Direction, WireVisDirFlags Flag, Vector2i Offset)[] Sides =
    [
        (Direction.North, WireVisDirFlags.North, new Vector2i(0, 1)),
        (Direction.South, WireVisDirFlags.South, new Vector2i(0, -1)),
        (Direction.East, WireVisDirFlags.East, new Vector2i(1, 0)),
        (Direction.West, WireVisDirFlags.West, new Vector2i(-1, 0)),
    ];

    private static readonly Vector2i[] Around = [Vector2i.Zero, new(0, 1), new(0, -1), new(1, 0), new(-1, 0)];

    /// <summary>Every look of a map, for its scene. What was sent before is forgotten.</summary>
    public List<SceneVisuals> All(EntityUid mapUid, Dictionary<EntityUid, Sent> sent)
    {
        sent.Clear();
        var things = new HashSet<EntityUid>();
        var cables = entMan.AllEntityQueryEnumerator<CableVisComponent, TransformComponent>();
        while (cables.MoveNext(out var uid, out _, out var xform))
        {
            if (xform.MapUid == mapUid)
                things.Add(uid);
        }

        var pipes = entMan.AllEntityQueryEnumerator<AtmosPipeColorComponent, TransformComponent>();
        while (pipes.MoveNext(out var uid, out _, out var xform))
        {
            if (xform.MapUid == mapUid)
                things.Add(uid);
        }

        // Pipes and pipe devices with no saved colour show connectors all the same.
        var connected = entMan.AllEntityQueryEnumerator<PipeAppearanceComponent, TransformComponent>();
        while (connected.MoveNext(out var uid, out _, out var xform))
        {
            if (xform.MapUid == mapUid)
                things.Add(uid);
        }

        AddPlumbingMachines(mapUid, things);
        AddDevices(mapUid, things);

        // What is on the Plumbing layer, should it have neither connectors nor pipe looks.
        var hidden = entMan.AllEntityQueryEnumerator<SubFloorHideComponent, NodeContainerComponent, TransformComponent>();
        while (hidden.MoveNext(out var uid, out _, out _, out var xform))
        {
            if (xform.MapUid == mapUid && OnPlumbingLayer(uid))
                things.Add(uid);
        }

        var result = new List<SceneVisuals>();
        foreach (var uid in things)
        {
            if (Look(uid, null) is { } look)
            {
                result.Add(look.Visuals);
                sent[uid] = look;
            }
        }

        return result;
    }

    /// <summary>
    /// The looks that changed after an edit: of the things named, of what stands on the cells
    /// given and on the cells beside them, and of what stood beside the removed.
    /// </summary>
    /// <param name="things">What the edit made, moved or changed the data of.</param>
    /// <param name="cells">Cells the edit changed or took something off.</param>
    /// <param name="removed">What left the document. Its looks are forgotten, so it is sent again if it comes back.</param>
    public List<SceneVisuals> Changed(
        EntityUid mapUid,
        Dictionary<EntityUid, Sent> sent,
        IEnumerable<EntityUid> things,
        IEnumerable<(EntityUid Grid, Vector2i Cell)> cells,
        IEnumerable<int> removed)
    {
        var around = new HashSet<(EntityUid Grid, Vector2i Cell)>(cells);
        var candidates = new HashSet<EntityUid>();
        foreach (var id in removed)
        {
            if (sent.Remove(new EntityUid(id), out var gone))
                around.Add((gone.Grid, gone.Cell));
        }

        foreach (var uid in things)
        {
            if (sent.TryGetValue(uid, out var before))
                around.Add((before.Grid, before.Cell));
            if (!entMan.TryGetComponent(uid, out TransformComponent? xform) || xform.MapUid != mapUid)
                continue;

            candidates.Add(uid);
            if (CellOf(xform) is { } cell)
                around.Add(cell);
        }

        // The cell and its four neighbours: a cable's and a pipe's joins are all to its cardinal neighbours.
        foreach (var (gridUid, cell) in around)
            AddAround(gridUid, cell, candidates);

        // A plumbing machine takes the layer of the duct it picks, and a duct on another of its
        // sides joins it by that layer. So what stands beside a machine is looked at too.
        foreach (var uid in candidates.ToArray())
        {
            if (entMan.TryGetComponent(uid, out TransformComponent? xform) && CellOf(xform) is { } at
                && PipesOf(uid, xform).Any(IsPlumbingMachine))
            {
                AddAround(at.Grid, at.Cell, candidates);
            }
        }

        var result = new List<SceneVisuals>();
        foreach (var uid in candidates)
        {
            if (!entMan.TryGetComponent(uid, out TransformComponent? xform) || xform.MapUid != mapUid)
                continue;

            sent.TryGetValue(uid, out var before);
            if (Look(uid, before?.Visuals) is not { } look)
            {
                sent.Remove(uid);
                continue;
            }

            sent[uid] = look;
            if (before?.Visuals != look.Visuals)
                result.Add(look.Visuals);
        }

        return result;
    }

    private void AddAround(EntityUid gridUid, Vector2i cell, HashSet<EntityUid> into)
    {
        if (!entMan.TryGetComponent(gridUid, out MapGridComponent? grid))
            return;

        foreach (var offset in Around)
        {
            var anchored = map.GetAnchoredEntitiesEnumerator(gridUid, grid, cell + offset);
            while (anchored.MoveNext(out var uid))
                into.Add(uid.Value);
        }
    }

    /// <summary>What the game would have an entity look like, or null if it says nothing of it.</summary>
    /// <param name="before">What was sent before: a colour or a layer once sent is sent again when it goes back to the prototype's.</param>
    private Sent? Look(EntityUid uid, SceneVisuals? before)
    {
        var xform = entMan.GetComponent<TransformComponent>(uid);
        var mask = CableMask(uid, xform);
        var state = PipeState(uid, xform);
        var color = PipeColor(uid) ?? (before?.PipeColor != null ? Color.White.ToHex() : null);
        var layer = PipeLayer(uid, before?.PipeLayer != null);
        var plumbing = PlumbingLooks(uid, xform);
        var plumbingLayer = OnPlumbingLayer(uid) ? true : before?.PlumbingLayer == true ? false : (bool?) null;
        var appearance = DeviceLooks(uid);
        if (mask == null && state == null && color == null && layer == null && plumbing == null && plumbingLayer == null && appearance == null)
            return null;

        var (grid, cell) = CellOf(xform) ?? (EntityUid.Invalid, Vector2i.Zero);
        return new Sent(new SceneVisuals(uid.Id, (int?) mask, state, color, layer, plumbing, plumbingLayer, appearance), grid, cell);
    }

    /// <summary>
    /// A pipe's colour, as the server's AtmosPipeColorSystem.OnStartup sends it from the saved
    /// AtmosPipeColor (Content.Server/Atmos/Piping/EntitySystems/AtmosPipeColorSystem.cs:19-25).
    /// Its OnShutdown sends white when the component goes (:27-33), which <see cref="Look"/> does.
    /// </summary>
    private string? PipeColor(EntityUid uid)
    {
        if (!entMan.TryGetComponent(uid, out AtmosPipeColorComponent? pipe) || !entMan.HasComponent<AppearanceComponent>(uid))
            return null;

        return pipe.Color.ToHex();
    }

    /// <summary>
    /// A cable's joins, as the server's CableVisSystem.UpdateAppearance sets them
    /// (Content.Server/Power/EntitySystems/CableVisSystem.cs:24-56): a bit for each cardinal
    /// neighbour that holds a cable node its node reaches. Null if it is no cable on a grid.
    /// </summary>
    private WireVisDirFlags? CableMask(EntityUid uid, TransformComponent xform)
    {
        if (!entMan.TryGetComponent(uid, out CableVisComponent? vis)
            || !entMan.TryGetComponent(uid, out NodeContainerComponent? container)
            || !container.Nodes.TryGetValue(vis.Node, out var own)
            || own is not CableNode
            || !entMan.TryGetComponent(xform.GridUid, out MapGridComponent? grid))
        {
            return null;
        }

        // The game's NodeGroupSystem.GetCompatibleNodes asks a node that cannot connect for
        // nothing (Content.Server/NodeContainer/EntitySystems/NodeGroupSystem.cs:356-357).
        if (!xform.Anchored || !Connectable(uid, own, xform))
            return WireVisDirFlags.None;

        var gridUid = xform.GridUid!.Value;
        var cell = map.TileIndicesFor(gridUid, grid, xform.Coordinates);

        // CableNode.GetReachableNodes (Content.Server/Power/Nodes/CableNode.cs:61-111): a side is
        // blocked by a terminal on this cell facing it, or by one on the neighbour facing back.
        var blocked = new HashSet<Direction>();
        foreach (var (_, node, other) in Nodes(gridUid, grid, cell))
        {
            if (node is CableTerminalNode)
                blocked.Add(other.LocalRotation.GetCardinalDir());
        }

        var mask = WireVisDirFlags.None;
        foreach (var (direction, flag, offset) in Sides)
        {
            var joined = false;
            foreach (var (neighbour, node, other) in Nodes(gridUid, grid, cell + offset))
            {
                if (node is CableTerminalNode && other.LocalRotation.GetCardinalDir().GetOpposite() == direction)
                    blocked.Add(direction);

                // Grouped only with a node of the same group that can connect too (NodeGroupSystem.cs:363-367).
                joined |= node is CableNode && node.NodeGroupID == own.NodeGroupID && Connectable(neighbour, node, other);
            }

            if (joined && !blocked.Contains(direction))
                mask |= flag;
        }

        return mask;
    }

    /// <summary>The nodes of what is anchored on a cell, as NodeHelpers.GetCardinalNeighborNodes finds them (Content.Server/NodeContainer/Nodes/NodeHelpers.cs:32-49).</summary>
    private IEnumerable<(EntityUid Uid, Node Node, TransformComponent Xform)> Nodes(EntityUid gridUid, MapGridComponent grid, Vector2i cell) =>
        NamedNodes(gridUid, grid, cell).Select(found => (found.Uid, found.Node, found.Xform));

    /// <summary>The same with each node's name in its container.</summary>
    private IEnumerable<(EntityUid Uid, Node Node, TransformComponent Xform, string Name)> NamedNodes(EntityUid gridUid, MapGridComponent grid, Vector2i cell)
    {
        foreach (var uid in map.GetAnchoredEntities((gridUid, grid), cell))
        {
            if (!entMan.TryGetComponent(uid, out NodeContainerComponent? container))
                continue;

            var xform = entMan.GetComponent<TransformComponent>(uid);
            foreach (var (name, node) in container.Nodes)
                yield return (uid, node, xform, name);
        }
    }

    private (EntityUid Grid, Vector2i Cell)? CellOf(TransformComponent xform)
    {
        if (xform.GridUid is not { } gridUid || xform.ParentUid != gridUid || !entMan.TryGetComponent(gridUid, out MapGridComponent? grid))
            return null;

        return (gridUid, map.TileIndicesFor(gridUid, grid, xform.Coordinates));
    }

    /// <summary>The cell a placement on a grid is on, or null if it is not on a grid.</summary>
    public (EntityUid Grid, Vector2i Cell)? CellOf(EntityUid parent, System.Numerics.Vector2 position)
    {
        if (!entMan.TryGetComponent(parent, out MapGridComponent? grid))
            return null;

        return (parent, map.TileIndicesFor(parent, grid, new EntityCoordinates(parent, position)));
    }
}
