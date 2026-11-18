using Content.Shared.Atmos;
using SS14.EditorKernel;
using static SS14.EditorNative.CheckKit;

namespace SS14.EditorNative;

/// <summary>
/// Checks the plumbing connectors the kernel sends for Starlight's plumbing machines against the
/// prototypes' nodes and the ducts beside them.
/// </summary>
public static partial class VisualsCheck
{
    private const string Duct = "PlumbingDuctStraight";

    private static int? Value(SceneVisuals? look, string key) => look?.Plumbing?.FirstOrDefault(value => value.Key == key)?.Value;

    private static IEnumerable<SceneEntity> Machines(Pipes pipes) =>
        pipes.World.Cells.Values.SelectMany(here => here).Where(entity => pipes.Of(entity) is { Connectors: true });

    /// <summary>The plumbing nodes of an entity that face one side only, with that side.</summary>
    private static IEnumerable<(NodeFact Node, PipeDirection Side, Robust.Shared.Maths.Vector2i Offset)> OneSided(Pipes pipes, SceneEntity entity) =>
        pipes.Of(entity)!.Nodes.Where(node => node.Plumbing)
            .SelectMany(node => PipeSides.Where(side => Pipes.Turned(entity, node) == side.Side).Select(side => (node, side.Side, side.Offset)));

    /// <summary>What on a cell has a plumbing node.</summary>
    private static List<SceneEntity> PlumbingOn(Pipes pipes, int grid, Robust.Shared.Maths.Vector2i cell) =>
        pipes.World.On(grid, cell).Where(entity => pipes.Of(entity) is { Plumbing: true }).ToList();

    private static void PlumbingConnectors(Pipes pipes, Action<string> say, Tally tally)
    {
        var machines = Machines(pipes).ToList();
        if (machines.Count == 0)
        {
            CheckSkips.Skip(say, "plumbing connectors: the map has no plumbing machine");
            return;
        }

        var wrong = new List<string>();
        var joined = 0;
        foreach (var machine in machines)
        {
            var look = pipes.World.Looks.GetValueOrDefault(machine.Uid);
            var nodes = Value(look, "NodeDirections");
            var connected = Value(look, "ConnectedDirections");
            var expected = pipes.Of(machine)!.Nodes.Where(node => node.Plumbing).Aggregate(PipeDirection.None, (all, node) => all | Pipes.Turned(machine, node));
            if (nodes != (int) expected || connected is not { } joins || (joins & ~(int) expected) != 0)
            {
                wrong.Add($"{machine.Prototype} {machine.Uid}: its nodes face {expected}, sent {nodes}, joined {connected}");
                continue;
            }

            // A side with one duct beside it that faces back: the game joins them.
            foreach (var (_, side, offset) in OneSided(pipes, machine))
            {
                var here = PlumbingOn(pipes, machine.Parent, World.CellOf(machine) + offset);
                if (here.Count != 1 || pipes.Of(here[0]) is not { Connectors: false, Nodes.Count: 1 } duct
                    || !Pipes.Turned(here[0], duct.Nodes[0]).HasDirection(side.GetOpposite()))
                {
                    continue;
                }

                joined++;
                var layer = pipes.LayerOf(here[0], duct, duct.Nodes[0]);
                var state = pipes.World.Looks.GetValueOrDefault(here[0].Uid)?.PipeState ?? 0;
                if ((joins & (int) side) == 0 || (state & ((int) side.GetOpposite() << (4 * layer))) == 0)
                    wrong.Add($"{machine.Prototype} {machine.Uid} and {here[0].Prototype} {here[0].Uid} on its {side}: joined {joins}, the duct's connectors {state}");
            }
        }

        tally.Expect(wrong.Count == 0,
            $"each of {machines.Count} plumbing machines is sent the sides of its nodes, and {joined} sides with one duct facing back are joined at both ends{(wrong.Count > 0 ? ": " + string.Join("; ", wrong.Take(4)) : "")}");
        if (joined == 0)
            CheckSkips.Skip(say, "plumbing connectors: no machine with a duct beside it");
    }

    /// <summary>
    /// What lives under the floor and has a plumbing node is named for the Plumbing layer, and
    /// nothing else is. The view draws it with the kind of its own.
    /// </summary>
    private static void PlumbingLayer(DrawList list, Pipes pipes, Action<string> say, Tally tally)
    {
        var entities = pipes.World.Cells.Values.SelectMany(here => here).ToList();
        var members = entities.Where(entity => pipes.Of(entity) is { Plumbing: true, SubFloor: true }).ToList();
        if (members.Count == 0)
        {
            CheckSkips.Skip(say, "the Plumbing layer: the map has no plumbing under the floor");
            return;
        }

        var named = entities.Where(entity => pipes.World.Looks.GetValueOrDefault(entity.Uid)?.PlumbingLayer == true).ToList();
        var drawn = members.Count(entity => list.Items.TryGetValue(entity.Uid, out var item) && item.Kind == 5);
        // Plumbing that is loose or in a container is named as well, and drawn so.
        var others = list.Items.Count(item => item.Value.Kind == 5 && pipes.World.Looks.GetValueOrDefault(item.Key)?.PlumbingLayer != true);
        var left = entities.Where(entity => pipes.Of(entity) is { Plumbing: true, SubFloor: false }).Select(entity => entity.Prototype).Distinct().ToList();
        tally.Expect(named.Count == members.Count && named.All(members.Contains) && drawn == members.Count && others == 0,
            $"{named.Count} of {members.Count} pieces of plumbing under the floor are named for the Plumbing layer and {drawn} drawn as such, {others} others ({string.Join(", ", members.Select(entity => entity.Prototype).Distinct().Take(6))}); left on their layers: {(left.Count > 0 ? string.Join(", ", left) : "none")}");
    }

    /// <summary>
    /// Disposal tubes under the floor are drawn with the kind of the Disposals layer, and nothing
    /// else is. The view goes by component; this goes by the fork's names, all Disposal*.
    /// </summary>
    private static void DisposalsLayer(DrawList list, World world, Action<string> say, Tally tally)
    {
        var entities = world.Cells.Values.SelectMany(here => here).Where(entity => list.Items.ContainsKey(entity.Uid)).ToList();
        if (!entities.Any(entity => entity.Prototype.StartsWith("Disposal", StringComparison.Ordinal) && list.Items[entity.Uid].Kind is 4 or 6))
        {
            CheckSkips.Skip(say, "the Disposals layer: the map has no disposal tube under the floor");
            return;
        }

        var disposals = entities.Where(entity => list.Items[entity.Uid].Kind == 6).ToList();
        var strangers = disposals.Where(entity => !entity.Prototype.StartsWith("Disposal", StringComparison.Ordinal)).Select(entity => entity.Prototype).Distinct().ToList();
        var missed = entities.Where(entity => list.Items[entity.Uid].Kind == 4 && entity.Prototype.StartsWith("Disposal", StringComparison.Ordinal)).Select(entity => entity.Prototype).Distinct().ToList();
        tally.Expect(disposals.Count > 0 && strangers.Count == 0 && missed.Count == 0,
            $"{disposals.Count} disposal tubes are drawn for the Disposals layer ({string.Join(", ", disposals.Select(entity => entity.Prototype).Distinct().Take(6))}); others there: {(strangers.Count > 0 ? string.Join(", ", strangers) : "none")}; disposals left under SubFloor: {(missed.Count > 0 ? string.Join(", ", missed) : "none")}");
    }

    /// <summary>A duct laid beside a plumbing machine's node joins it at both ends, and undo parts them again.</summary>
    private static void DuctLaid(EngineSession session, int doc, Pipes pipes, Action<string> say, Tally tally)
    {
        foreach (var machine in Machines(pipes))
        {
            var before = Value(pipes.World.Looks.GetValueOrDefault(machine.Uid), "ConnectedDirections");
            foreach (var (_, side, offset) in OneSided(pipes, machine))
            {
                var cell = World.CellOf(machine) + offset;
                if (before is not { } joins || (joins & (int) side) != 0 || !pipes.World.Tiles[machine.Parent].Contains(cell) || PlumbingOn(pipes, machine.Parent, cell).Count > 0)
                    continue;

                var rotation = side is PipeDirection.North or PipeDirection.South ? 0 : Math.PI / 2;
                var laid = Edit(session, doc, "Duct", new EditOp("spawn", Prototype: Duct, Parent: machine.Parent, X: cell.X + 0.5f, Y: cell.Y + 0.5f, Rotation: rotation));
                var made = laid.Created.Single();
                var mine = Value(laid.Visuals.FirstOrDefault(look => look.Uid == machine.Uid), "ConnectedDirections");
                var theirs = StateIn(laid, made);
                tally.Expect(mine == (joins | (int) side) && theirs is { } state && (state & (int) side.GetOpposite()) != 0,
                    $"a {Duct} laid on the {side} of {machine.Prototype} {machine.Uid} joins it: its joined sides {joins} became {mine}, the duct's connectors are {theirs}");

                // The new duct is on the Plumbing layer, as the kernel names it and the view draws it.
                var list = DrawList.Read(session, doc, null);
                var kind = list.Items.TryGetValue(made, out var item) ? item.Kind : -1;
                tally.Expect(laid.Visuals.FirstOrDefault(look => look.Uid == made)?.PlumbingLayer == true && kind == 5,
                    $"the duct is named for the Plumbing layer and drawn as such: kind {kind}");

                var undone = Undo(session, doc);
                tally.Expect(Value(undone.Visuals.FirstOrDefault(look => look.Uid == machine.Uid), "ConnectedDirections") == joins && undone.Removed.Contains(made),
                    $"undo takes the duct away and the machine's joined sides go back to {joins}");
                return;
            }
        }

        CheckSkips.Skip(say, "a duct laid beside a plumbing machine: no machine with a floor free of plumbing beside a node");
    }
}
