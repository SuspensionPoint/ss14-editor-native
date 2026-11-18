#if FORK_PLUMBING
using Content.Server._Starlight.Plumbing.Nodes;
#endif
using Content.Server.NodeContainer.Nodes;
using Content.Shared.Atmos;
using Content.Shared.Atmos.Components;
using Content.Shared.NodeContainer;
using Robust.Shared.Maths;
using Robust.Shared.Prototypes;
using SS14.EditorKernel;
using YamlDotNet.RepresentationModel;
using static SS14.EditorNative.CheckKit;

namespace SS14.EditorNative;

/// <summary>
/// Checks the pipe connectors the kernel sends against what each pipe's prototype data and its
/// neighbours call for. A node joins a node on the neighbouring cell that has the same group and
/// layer and faces back. Plumbing follows its own rules and gets its own step.
/// </summary>
public static partial class VisualsCheck
{
    /// <summary>One pipe node of a prototype, as its data has it.</summary>
    private sealed record NodeFact(string Group, PipeDirection Directions, int Layer, bool Turns, bool Connects, bool Plumbing);

    /// <summary>The pipe nodes of a prototype and its AtmosPipeLayers, if it has one.</summary>
    /// <param name="Files">The files of its pipe's sprite layer by pipe layer, as its AtmosPipeLayers names them.</param>
    /// <param name="Connectors">Whether it shows plumbing connectors of its own (PlumbingConnectorAppearance).</param>
    /// <param name="SubFloor">Whether it lives under the floor (SubFloorHide).</param>
    private sealed record PipeFacts(List<NodeFact> Nodes, int Layers, int? EntityLayer, bool Locked, Dictionary<int, string> Files, bool Connectors, bool SubFloor)
    {
        public bool Plumbing => Nodes.Any(node => node.Plumbing);
    }

    /// <summary>Whether a node is Starlight's plumbing. Never on a fork without it.</summary>
#if FORK_PLUMBING
    private static bool IsPlumbing(PipeNode node) => node is PlumbingNode;
#else
    private static bool IsPlumbing(PipeNode node) => false;
#endif

    /// <summary>What the check knows of the pipes of one map.</summary>
    private sealed class Pipes(World world, Dictionary<string, PipeFacts> facts, Dictionary<int, int> overrides)
    {
        public readonly World World = world;
        public readonly Dictionary<string, PipeFacts> Facts = facts;

        public PipeFacts? Of(SceneEntity entity) => Facts.GetValueOrDefault(entity.Prototype);

        /// <summary>A node's layer: the entity's own, as saved or from its prototype, unless its layers are locked.</summary>
        public int LayerOf(SceneEntity entity, PipeFacts pipe, NodeFact node)
        {
            if (pipe.EntityLayer is not { } layer || pipe.Locked)
                return node.Layer;
            return Math.Clamp(overrides.TryGetValue(entity.YamlUid, out var saved) ? saved : layer, 0, pipe.Layers - 1);
        }

        public bool SavesLayer(SceneEntity entity) => overrides.ContainsKey(entity.YamlUid);

        /// <summary>The entities that save a layer of their own apart from their prototype's, with it.</summary>
        public IEnumerable<(SceneEntity Entity, int Layer)> Overridden() =>
            World.Cells.Values.SelectMany(here => here)
                .Where(entity => overrides.ContainsKey(entity.YamlUid) && Of(entity) is { EntityLayer: { } own } facts && Math.Clamp(overrides[entity.YamlUid], 0, facts.Layers - 1) != own)
                .Select(entity => (entity, Math.Clamp(overrides[entity.YamlUid], 0, Of(entity)!.Layers - 1)));

        public static PipeDirection Turned(SceneEntity entity, NodeFact node) =>
            node.Turns ? node.Directions.RotatePipeDirection(entity.Rotation) : node.Directions;

        /// <summary>
        /// The connectors a gas pipe must show: for each layer below its number of layers, the
        /// sides on which one of its nodes meets a node of its group and layer facing back.
        /// Null for plumbing and what has no pipe node.
        /// </summary>
        public int? Expected(SceneEntity entity)
        {
            if (Of(entity) is not { Plumbing: false } pipe)
                return null;

            var bits = new int[pipe.Layers];
            foreach (var node in pipe.Nodes)
            {
                if (!node.Connects)
                    continue;

                var layer = LayerOf(entity, pipe, node);
                var turned = Turned(entity, node);
                foreach (var (side, offset) in PipeSides)
                {
                    if (!turned.HasDirection(side) || layer >= pipe.Layers)
                        continue;

                    foreach (var other in World.On(entity.Parent, World.CellOf(entity) + offset))
                    {
                        if (Of(other) is not { } facts)
                            continue;
                        if (facts.Nodes.Any(far => far.Connects && far.Group == node.Group && LayerOf(other, facts, far) == layer
                                                   && Turned(other, far).HasDirection(side.GetOpposite())))
                        {
                            bits[layer] |= (int) side;
                        }
                    }
                }
            }

            var state = 0;
            for (var i = 0; i < bits.Length; i++)
                state |= bits[i] << (4 * i);
            return state;
        }

        /// <summary>Whether a node of the entity faces a node of its group on another layer, which it must not join.</summary>
        public bool BesideOtherLayer(SceneEntity entity)
        {
            if (Of(entity) is not { Plumbing: false } pipe)
                return false;

            foreach (var node in pipe.Nodes)
            {
                var layer = LayerOf(entity, pipe, node);
                foreach (var (side, offset) in PipeSides)
                {
                    if (!Turned(entity, node).HasDirection(side))
                        continue;

                    foreach (var other in World.On(entity.Parent, World.CellOf(entity) + offset))
                    {
                        if (Of(other) is { } facts && facts.Nodes.Any(far => far.Group == node.Group && LayerOf(other, facts, far) != layer
                                                                              && Turned(other, far).HasDirection(side.GetOpposite())))
                        {
                            return true;
                        }
                    }
                }
            }

            return false;
        }
    }

    private static readonly (PipeDirection Side, Vector2i Offset)[] PipeSides =
    [
        (PipeDirection.North, new Vector2i(0, 1)), (PipeDirection.South, new Vector2i(0, -1)),
        (PipeDirection.West, new Vector2i(-1, 0)), (PipeDirection.East, new Vector2i(1, 0)),
    ];

    /// <summary>The pipe facts of the prototypes given, read from the kernel's prototypes.</summary>
    private static Dictionary<string, PipeFacts> PipeFactsOf(EngineSession session, IEnumerable<string> prototypes)
    {
        var ids = prototypes.Distinct().ToList();
        return session.OnDocumentKernel(kernel =>
        {
            var facts = new Dictionary<string, PipeFacts>();
            foreach (var id in ids)
            {
                if (!kernel.Prototypes.TryIndex<EntityPrototype>(id, out var prototype)
                    || !prototype.Components.TryGetValue("NodeContainer", out var entry)
                    || entry.Component is not NodeContainerComponent container)
                {
                    continue;
                }

                var nodes = container.Nodes.Values.OfType<PipeNode>()
                    .Select(node => new NodeFact(node.NodeGroupID.ToString(), node.OriginalPipeDirection, (int) node.CurrentPipeLayer, node.RotationsEnabled, node.ConnectionsEnabled, IsPlumbing(node)))
                    .ToList();
                if (nodes.Count == 0)
                    continue;

                var layers = prototype.Components.TryGetValue("AtmosPipeLayers", out var layersEntry) ? layersEntry.Component as AtmosPipeLayersComponent : null;
                var files = layers?.SpriteLayersRsiPaths.Values.FirstOrDefault()?.ToDictionary(file => (int) file.Key, file => file.Value) ?? new Dictionary<int, string>();
                facts[id] = new PipeFacts(nodes, layers?.NumberOfPipeLayers ?? 1, layers == null ? null : (int) layers.CurrentPipeLayer, layers?.PipeLayersLocked ?? false, files,
                    prototype.Components.ContainsKey("PlumbingConnectorAppearance"), prototype.Components.ContainsKey("SubFloorHide"));
            }

            return facts;
        });
    }

    /// <summary>The pipe layer each entity of the file saves in its own AtmosPipeLayers, by its uid in the file.</summary>
    private static Dictionary<int, int> SavedLayers(string yaml)
    {
        var layers = new Dictionary<int, int>();
        foreach (var (uid, component) in SavedComponents(yaml, "AtmosPipeLayers"))
        {
            if (component.Children.TryGetValue("pipeLayer", out var value))
            {
                var text = ((YamlScalarNode) value).Value!;
                layers[uid] = int.TryParse(text, out var number) ? number : (int) Enum.Parse<AtmosPipeLayer>(text);
            }
        }

        return layers;
    }

    /// <summary>Each saved component of a type, with the uid in the file of its entity.</summary>
    private static IEnumerable<(int Uid, YamlMappingNode Component)> SavedComponents(string yaml, string type)
    {
        var stream = new YamlStream();
        stream.Load(new StringReader(yaml));
        var root = (YamlMappingNode) stream.Documents[0].RootNode;
        foreach (var entity in ((YamlSequenceNode) root["entities"]).Children
                     .SelectMany(prototype => ((YamlSequenceNode) prototype["entities"]).Children.Cast<YamlMappingNode>()))
        {
            if (!entity.Children.TryGetValue("components", out var components))
                continue;

            foreach (var component in ((YamlSequenceNode) components).Children.Cast<YamlMappingNode>())
            {
                if (((YamlScalarNode) component["type"]).Value == type)
                    yield return (int.Parse(((YamlScalarNode) entity["uid"]).Value!), component);
            }
        }
    }

    private static string Shape(PipeFacts pipe)
    {
        if (pipe.Nodes.Count != 1)
            return "device";

        var directions = pipe.Nodes[0].Directions;
        return System.Numerics.BitOperations.PopCount((uint) directions) switch
        {
            2 when directions is PipeDirection.Longitudinal or PipeDirection.Lateral => "straight",
            2 => "bend",
            3 => "T",
            4 => "four-way",
            _ => "other",
        };
    }

    private static void Connectors(Pipes pipes, Action<string> say, Tally tally)
    {
        var looks = pipes.World.Looks;
        var kinds = new Dictionary<string, int> { ["straight"] = 0, ["bend"] = 0, ["T"] = 0, ["four-way"] = 0, ["pump"] = 0, ["device"] = 0, ["other"] = 0 };
        var wrong = new List<string>();
        var compared = 0;
        var besideOther = 0;
        foreach (var entity in pipes.World.Cells.Values.SelectMany(here => here))
        {
            if (!looks.TryGetValue(entity.Uid, out var look) || look.PipeState == null || pipes.Expected(entity) is not { } expected)
                continue;

            compared++;
            var kind = entity.Prototype.Contains("Pump", StringComparison.Ordinal) && !entity.Prototype.Contains("Vent", StringComparison.Ordinal) ? "pump" : Shape(pipes.Of(entity)!);
            kinds[kind]++;
            if (pipes.BesideOtherLayer(entity))
                besideOther++;
            if (look.PipeState != expected)
                wrong.Add($"{entity.Prototype} {entity.Uid} at {World.CellOf(entity)}: expected {expected}, sent {look.PipeState}");
        }

        if (compared == 0)
        {
            CheckSkips.Skip(say, "pipe connectors: the map has no gas pipes");
            return;
        }

        tally.Expect(wrong.Count == 0,
            $"every gas pipe shows the connectors its neighbours call for: {string.Join(", ", kinds.Select(kind => $"{kind.Value} {kind.Key}"))}, {besideOther} beside a pipe of another layer, of {compared}{(wrong.Count > 0 ? $": {wrong.Count} wrong, " + string.Join("; ", wrong.Take(4)) : "")}");
        foreach (var kind in new[] { "straight", "bend", "T", "four-way", "pump" })
        {
            if (kinds[kind] == 0)
                CheckSkips.Skip(say, $"pipe connectors: no {kind} on the map");
        }

        if (besideOther == 0)
            CheckSkips.Skip(say, "pipe connectors: no pipe beside one of another layer");
    }

    /// <summary>
    /// A pipe beside another taken away and laid again: the other loses the connector and gets
    /// it back, the new one joins it, and undo, twice, takes it back step by step.
    /// </summary>
    private static void PipeLaid(EngineSession session, int doc, Pipes pipes, Action<string> say, Tally tally)
    {
        foreach (var entity in pipes.World.Cells.Values.SelectMany(here => here))
        {
            if (pipes.Of(entity) is not { Plumbing: false, Nodes.Count: 1 } pipe || pipes.Expected(entity) is not { } before
                || pipes.World.Looks.GetValueOrDefault(entity.Uid)?.PipeState != before)
            {
                continue;
            }

            var node = pipe.Nodes[0];
            var layer = pipes.LayerOf(entity, pipe, node);
            if (node.Group != "Pipe" || !node.Connects)
                continue;

            foreach (var (side, offset) in PipeSides)
            {
                // The one pipe beside it on that side, joined to it: a plain pipe with no layer
                // of its own saved, so that one of its prototype laid there is the same.
                var bit = (int) side << (4 * layer);
                var here = pipes.World.On(entity.Parent, World.CellOf(entity) + offset).Where(other => pipes.Of(other) != null).ToList();
                if ((before & bit) == 0 || here.Count != 1 || pipes.Of(here[0]) is not { Plumbing: false, Nodes.Count: 1 } || pipes.SavesLayer(here[0]))
                {
                    continue;
                }

                var next = here[0];
                var taken = Edit(session, doc, "Take", new EditOp("delete", Uid: next.Uid));
                tally.Expect(StateIn(taken, entity.Uid) == (before & ~bit),
                    $"with {next.Prototype} {next.Uid} taken away, {entity.Prototype} {entity.Uid} loses that connector: {before} became {StateIn(taken, entity.Uid)}");

                var laid = Edit(session, doc, "Pipe", new EditOp("spawn", Prototype: next.Prototype, Parent: next.Parent, X: next.X, Y: next.Y, Rotation: next.Rotation));
                var made = laid.Created.Single();
                var mine = StateIn(laid, entity.Uid);
                var theirs = StateIn(laid, made);
                var back = (int) side.GetOpposite() << (4 * layer);
                tally.Expect(mine == before && theirs is { } state && (state & back) != 0 && theirs == pipes.World.Looks[next.Uid].PipeState,
                    $"a {next.Prototype} laid there again joins it: its connectors became {mine} of {before}, the new one's are {theirs} of {pipes.World.Looks[next.Uid].PipeState}");

                var undone = Undo(session, doc);
                tally.Expect(StateIn(undone, entity.Uid) == (before & ~bit) && undone.Removed.Contains(made),
                    $"undo takes the new pipe away and the connector with it: {StateIn(undone, entity.Uid)}");
                var back2 = Undo(session, doc);
                tally.Expect(StateIn(back2, entity.Uid) == before,
                    $"undo again gives back {next.Prototype} {next.Uid} and the connectors {before}: {StateIn(back2, entity.Uid)}");
                return;
            }
        }

        CheckSkips.Skip(say, "a pipe laid beside another: no plain gas pipe joined to another");
    }

    /// <summary>Each pipe that saves a layer of its own is sent that layer, and no other pipe is sent one.</summary>
    private static void LayersSent(Pipes pipes, Action<string> say, Tally tally)
    {
        var overridden = pipes.Overridden().ToList();
        if (overridden.Count == 0)
        {
            CheckSkips.Skip(say, "pipe layers: the map saves no pipe on a layer apart from its prototype's");
            return;
        }

        var wrong = overridden.Where(pair => pipes.World.Looks.GetValueOrDefault(pair.Entity.Uid)?.PipeLayer != pair.Layer)
            .Select(pair => $"{pair.Entity.Prototype} {pair.Entity.Uid}: saved {pair.Layer}, sent {pipes.World.Looks.GetValueOrDefault(pair.Entity.Uid)?.PipeLayer}").ToList();
        var others = pipes.World.Looks.Values.Count(look => look.PipeLayer != null) - overridden.Count(pair => pipes.World.Looks.GetValueOrDefault(pair.Entity.Uid)?.PipeLayer != null);
        tally.Expect(wrong.Count == 0 && others == 0,
            $"each of {overridden.Count} pipes saved on a layer of its own is sent it, and {others} others are sent one{(wrong.Count > 0 ? ": " + string.Join("; ", wrong.Take(4)) : "")}");
    }

    /// <summary>A pipe saved on a layer of its own is drawn from the files of that layer.</summary>
    private static void LayerDrawn(DrawList list, Pipes pipes, Action<string> say, Tally tally)
    {
        foreach (var (entity, layer) in pipes.Overridden())
        {
            if (!list.Items.TryGetValue(entity.Uid, out var item) || !pipes.Of(entity)!.Files.TryGetValue(layer, out var file))
                continue;

            var paths = item.Quads.Select(quad => list.Textures.GetValueOrDefault(quad.Texture)?.Path ?? "").Distinct().ToList();
            tally.Expect(paths.Any(path => path.Contains(file, StringComparison.Ordinal)),
                $"{entity.Prototype} {entity.Uid} saved on layer {layer} is drawn from {file}: {string.Join(", ", paths)}");
            return;
        }

        CheckSkips.Skip(say, "a pipe drawn on a layer of its own: none is drawn");
    }

    private static int? StateIn(SceneDelta delta, int uid) => delta.Visuals.FirstOrDefault(look => look.Uid == uid)?.PipeState;
}
