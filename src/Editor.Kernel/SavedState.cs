using Robust.Shared.GameObjects;
using Robust.Shared.Map.Components;
using Robust.Shared.Serialization.Markdown.Mapping;
using Robust.Shared.Serialization.Markdown.Sequence;
using Robust.Shared.Serialization.Markdown.Value;

namespace SS14.EditorKernel;

/// <summary>
/// The saved data the view is sent of each entity it shows, so that it looks as the map saves
/// it and not as its prototype alone (<see cref="SceneSaved"/>).
///
/// In the game's mapping mode the server loads the map, paused and not map-initialized, and
/// sends each client its copy of every networked component of each entity
/// (Robust.Shared/GameStates/NetworkedComponentAttribute.cs). The client's systems then set the
/// appearance from it, as SharedDoorSystem does from a door's state and SharedGasValveSystem
/// from whether a valve is open. Here the kernel sends that data as the map file holds it: what
/// differs from the prototype (ComponentEditor.Sent). The view reads it onto the entity before
/// the entity starts, as the game's map loader does.
///
/// What was sent of each entity is remembered, so after an edit only what changed goes again.
/// An entity counts as sent as its prototype has it until it is sent anything else.
/// </summary>
internal sealed class SavedStateReader(IEntityManager entMan, ComponentEditor components)
{
    /// <summary>What was sent of each entity, and which entities have been looked at.</summary>
    internal sealed class Sent
    {
        public readonly Dictionary<EntityUid, IReadOnlyList<SavedComponent>> Data = new();
        public readonly HashSet<EntityUid> Seen = new();

        /// <summary>
        /// The components the file saves of each entity, by its uid in the file, while the
        /// document is as it was opened. An entity whose file saves none of the components sent
        /// is as its prototype has them, and is not looked at. Null when not known.
        /// </summary>
        public Dictionary<int, HashSet<string>>? Listed;

        public void Clear()
        {
            Data.Clear();
            Seen.Clear();
        }
    }

    /// <summary>The names of the components a map file saves of each entity, by its uid in the file. Null if it cannot tell.</summary>
    public static Dictionary<int, HashSet<string>>? Listed(MappingDataNode file)
    {
        if (!file.TryGet("entities", out SequenceDataNode? groups))
            return null;

        var listed = new Dictionary<int, HashSet<string>>();
        foreach (var group in groups.OfType<MappingDataNode>())
        {
            if (!group.TryGet("entities", out SequenceDataNode? entities))
                continue;

            foreach (var entity in entities.OfType<MappingDataNode>())
            {
                if (!entity.TryGet("uid", out ValueDataNode? uid) || !int.TryParse(uid.Value, out var id))
                    return null;

                var names = listed[id] = new HashSet<string>();
                if (!entity.TryGet("components", out SequenceDataNode? components))
                    continue;

                foreach (var component in components.OfType<MappingDataNode>())
                {
                    if (component.TryGet("type", out ValueDataNode? type))
                        names.Add(type.Value);
                }
            }
        }

        return listed;
    }

    /// <summary>The saved data of every entity the scene shows. What was sent before is forgotten.</summary>
    public List<SceneSaved> All(IEnumerable<SceneEntity> entities, Sent sent)
    {
        sent.Clear();
        var listed = sent.Listed;
        sent.Listed = null;
        var result = new List<SceneSaved>();
        foreach (var entity in entities)
        {
            if (entity.Contained || entity.Prototype.Length == 0)
                continue;

            var uid = new EntityUid(entity.Uid);
            sent.Seen.Add(uid);
            // Most entities save nothing but where they are, and reading every component of
            // every one costs about as much as saving the map. So only what the file saves is read.
            HashSet<string>? names = null;
            if (listed != null && entity.YamlUid != 0 && listed.TryGetValue(entity.YamlUid, out names) && !names.Any(components.IsSent))
                continue;

            var data = Read(uid, names);
            if (data.Count == 0)
                continue;

            sent.Data[uid] = data;
            result.Add(new SceneSaved(entity.Uid, data));
        }

        return result;
    }

    /// <summary>
    /// The saved data that changed after an edit: of the entities whose components it changed,
    /// and of those that came into the document or out of a container. What left the document
    /// is forgotten, so it is sent again if it comes back.
    /// </summary>
    /// <param name="changed">Entities whose component data the edit changed.</param>
    /// <param name="touched">Entities the edit made, moved or put back.</param>
    public List<SceneSaved> Changed(EntityUid mapUid, Sent sent, IEnumerable<EntityUid> changed, IEnumerable<EntityUid> touched, IEnumerable<int> removed)
    {
        foreach (var id in removed)
        {
            sent.Data.Remove(new EntityUid(id));
            sent.Seen.Remove(new EntityUid(id));
        }

        var candidates = new HashSet<EntityUid>(changed);
        foreach (var uid in touched)
        {
            if (!sent.Seen.Contains(uid))
                candidates.Add(uid);
        }

        var result = new List<SceneSaved>();
        foreach (var uid in candidates)
        {
            // What a container holds is not shown. It is looked at when it comes out.
            if (!Shown(uid, mapUid))
                continue;

            sent.Seen.Add(uid);
            var data = Read(uid);
            var before = sent.Data.GetValueOrDefault(uid) ?? [];
            if (Same(before, data))
                continue;

            if (data.Count == 0)
                sent.Data.Remove(uid);
            else
                sent.Data[uid] = data;
            result.Add(new SceneSaved(uid.Id, data));
        }

        return result;
    }

    /// <summary>Whether the view shows the entity: one of the document's, on a grid or the map, with a prototype.</summary>
    private bool Shown(EntityUid uid, EntityUid mapUid)
    {
        if (!entMan.TryGetComponent(uid, out TransformComponent? xform) || xform.MapUid != mapUid || uid == mapUid)
            return false;

        if (entMan.HasComponent<MapGridComponent>(uid) || entMan.GetComponent<MetaDataComponent>(uid).EntityPrototype == null)
            return false;

        var parent = xform.ParentUid;
        return parent.IsValid() && (entMan.HasComponent<MapGridComponent>(parent) || entMan.HasComponent<MapComponent>(parent));
    }

    private IReadOnlyList<SavedComponent> Read(EntityUid uid, IReadOnlySet<string>? listed = null) =>
        components.Sent(uid, listed).Select(pair => new SavedComponent(pair.Name, pair.Node)).ToArray();

    private static bool Same(IReadOnlyList<SavedComponent> a, IReadOnlyList<SavedComponent> b)
    {
        if (a.Count != b.Count)
            return false;

        for (var i = 0; i < a.Count; i++)
        {
            if (a[i].Name != b[i].Name || !a[i].Data.Equals(b[i].Data))
                return false;
        }

        return true;
    }
}
