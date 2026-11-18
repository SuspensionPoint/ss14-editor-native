namespace SS14.EditorKernel;

// The data that crosses between the engine and the editor's user interface. These records are the
// single definition of that contract. The Rust side reads them as JSON (editor-core's model.rs
// and session), so a change here goes with a change there.

/// <summary>One message from the engine log, at warning level or above.</summary>
public sealed record KernelLogEntry(string Level, string Sawmill, string Message);

/// <summary>One open map or grid file, held as live engine entities.</summary>
public sealed record DocumentInfo(
    int Id,
    string Source,
    /// <summary>"Map" or "Grid": what kind of file this is and will be saved as.</summary>
    string Category,
    int FormatVersion,
    string? EngineVersion,
    int MapUid,
    int EntityCount,
    long LoadMilliseconds,
    IReadOnlyList<KernelLogEntry> Log);

/// <summary>Everything the interface needs to show a document.</summary>
public sealed record Scene(
    DocumentInfo Document,
    IReadOnlyList<SceneGrid> Grids,
    IReadOnlyList<SceneEntity> Entities,
    HistoryState History,
    /// <summary>The looks the game's server would send of the entities, where it says any.</summary>
    IReadOnlyList<SceneVisuals> Visuals);

public sealed record SceneGrid(
    int Uid,
    int YamlUid,
    string Name,
    /// <summary>Position of the grid's origin on its map, in tiles.</summary>
    float X,
    float Y,
    /// <summary>Rotation of the grid on its map, in radians.</summary>
    double Rotation,
    /// <summary>Tile prototype ids used by <see cref="Tiles"/>; index 0 is always empty space.</summary>
    IReadOnlyList<string> TileIds,
    /// <summary>
    /// Non-empty tiles as five integers each: x, y, index into <see cref="TileIds"/>, variant,
    /// and the engine's rotation-and-mirroring byte.
    /// </summary>
    IReadOnlyList<int> Tiles,
    IReadOnlyList<SceneDecal> Decals);

/// <summary>A decal on a grid, as the game's decal component holds it.</summary>
public sealed record SceneDecal(
    int Id,
    string Prototype,
    /// <summary>Position in the grid's own coordinates: the lower-left corner of the decal.</summary>
    float X,
    float Y,
    /// <summary>Tint as #RRGGBBAA, or null for the prototype's own colours.</summary>
    string? Color,
    double Angle,
    int ZIndex,
    bool Cleanable);

/// <summary>A decal that was added or changed, or (with no <see cref="Decal"/>) removed.</summary>
public sealed record DecalChange(int Grid, int Id, SceneDecal? Decal);

public sealed record SceneEntity(
    int Uid,
    /// <summary>The uid this entity has in the file, or 0 if it was created in this session.</summary>
    int YamlUid,
    /// <summary>Entity prototype id; empty for an entity without a prototype.</summary>
    string Prototype,
    /// <summary>The grid, map or containing entity this entity is attached to.</summary>
    int Parent,
    /// <summary>Position relative to <see cref="Parent"/>, in tiles.</summary>
    float X,
    float Y,
    /// <summary>Rotation relative to <see cref="Parent"/>, in radians.</summary>
    double Rotation,
    bool Anchored,
    /// <summary>True when the parent is another entity (a container) rather than a grid or map.</summary>
    bool Contained,
    /// <summary>The entity's name if it differs from its prototype's.</summary>
    string? Name);

/// <summary>
/// How an entity looks where the game's server works it out and sends it to its clients as
/// appearance data (Visuals.cs). It is sent apart from <see cref="SceneEntity"/>, so a change of
/// looks alone does not move the entity in the view. Each field holds the whole value as it is
/// now. A field that is null says nothing.
/// </summary>
public sealed record SceneVisuals(
    int Uid,
    /// <summary>The sides a cable joins another cable on, as the game's WireVisDirFlags: north 1, south 2, east 4, west 8.</summary>
    int? CableMask,
    /// <summary>A pipe's connections, as the game's PipeVisuals.VisualState: four bits for each pipe layer, north 1, south 2, west 4, east 8, on the grid.</summary>
    int? PipeState,
    /// <summary>A pipe's colour as #RRGGBBAA, as the game's PipeColorVisuals.Color.</summary>
    string? PipeColor,
    /// <summary>The pipe layer of a pipe whose own AtmosPipeLayers sets one apart from its prototype's, as the game's AtmosPipeLayer: 0 the first.</summary>
    int? PipeLayer,
    /// <summary>A plumbing machine's connectors, as the values Starlight's PlumbingConnectorAppearanceSystem sets, each by the name of its key in PlumbingVisuals.</summary>
    IReadOnlyList<SceneAppearance>? Plumbing,
    /// <summary>
    /// Whether the entity is on the editor's Plumbing layer: a piece of Starlight's plumbing that
    /// lives under the floor, its ducts and plumbing machines. The view gives its draw-list item
    /// a kind of its own. False only for one that was and is no more.
    /// </summary>
    bool? PlumbingLayer)
{
    /// <summary>Equal when every field is, the plumbing values one by one.</summary>
    public bool Equals(SceneVisuals? other) =>
        other is not null && Uid == other.Uid && CableMask == other.CableMask && PipeState == other.PipeState && PipeColor == other.PipeColor
        && PipeLayer == other.PipeLayer && PlumbingLayer == other.PlumbingLayer
        && (Plumbing == null ? other.Plumbing == null : other.Plumbing != null && Plumbing.SequenceEqual(other.Plumbing));

    public override int GetHashCode() => HashCode.Combine(Uid, CableMask, PipeState, PipeColor, PipeLayer, Plumbing?.Count, PlumbingLayer);
}

/// <summary>One appearance value the game's server sets, by the name of its key. A yes or no is 1 or 0.</summary>
public sealed record SceneAppearance(string Key, int Value);

public sealed record TileChange(
    int Grid,
    int X,
    int Y,
    /// <summary>Tile prototype id; "Space" clears the tile.</summary>
    string Tile,
    int Variant,
    int RotationMirroring);

/// <summary>What changed in the engine's world as the result of one command.</summary>
public sealed record SceneDelta(
    int Document,
    IReadOnlyList<TileChange> Tiles,
    /// <summary>Entities that were created or whose placement or name changed.</summary>
    IReadOnlyList<SceneEntity> Entities,
    /// <summary>
    /// Entities and grids that left the document. What a thing holds, and what stands on a
    /// grid, is named too, before the thing or the grid itself.
    /// </summary>
    IReadOnlyList<int> Removed,
    IReadOnlyList<SceneGrid> GridsChanged,
    HistoryState History,
    /// <summary>Uids of entities created by this command, in the order they were requested.</summary>
    IReadOnlyList<int> Created,
    IReadOnlyList<DecalChange> Decals,
    /// <summary>Ids of decals created by this command, in the order they were requested.</summary>
    IReadOnlyList<int> CreatedDecals,
    /// <summary>Fresh component data for entities whose components this command changed.</summary>
    IReadOnlyList<EntityDetails> Details,
    /// <summary>
    /// The links and listed devices of each entity whose links this command may have changed,
    /// whole, in place of all that was known of that entity's. An entity in
    /// <see cref="Removed"/> has taken its own with it and is not named here. What others
    /// linked to it is gone too, and those others are named here.
    /// </summary>
    IReadOnlyList<EntityLinks> Linked,
    /// <summary>The grids whose stored air or atmosphere this command changed, by an "air", "air.fix" or "air.add" op, an atmosphere the game would add, or by undo and redo of one.</summary>
    IReadOnlyList<int> Air,
    IReadOnlyList<KernelLogEntry> Log,
    /// <summary>
    /// The looks that changed, of the entities the command touched and of those on the cells
    /// beside them. Each is whole. An entity in <see cref="Removed"/> is not named here.
    /// </summary>
    IReadOnlyList<SceneVisuals> Visuals);

/// <summary>One device link: a port of a source wired to a port of a sink.</summary>
public sealed record SceneLink(int Source, string SourcePort, int Sink, string SinkPort);

/// <summary>One device on the device list of another entity, as an air alarm lists its vents.</summary>
public sealed record ListedDevice(int List, int Device);

/// <summary>Every device link and every device list entry of a document.</summary>
public sealed record LinkScene(IReadOnlyList<SceneLink> Links, IReadOnlyList<ListedDevice> Lists);

/// <summary>What one entity links to as a source, and the devices on its own device list.</summary>
public sealed record EntityLinks(int Uid, IReadOnlyList<SceneLink> Links, IReadOnlyList<int> Devices);

/// <summary>A port prototype of the game.</summary>
public sealed record PortInfo(
    string Id,
    /// <summary>The game's localisation keys for the name and the description, not the texts.</summary>
    string Name,
    string Description,
    /// <summary>
    /// For a source port, the sink ports the game links it to when asked to link two devices
    /// by default. Null if the prototype names none, and always null for a sink port. The
    /// game lets any source port be linked to any sink port.
    /// </summary>
    IReadOnlyList<string>? Defaults);

/// <summary>The ports an entity prototype has, sorted. Only prototypes with a port are listed.</summary>
public sealed record PrototypePorts(string Id, IReadOnlyList<string> Sources, IReadOnlyList<string> Sinks);

public sealed record PortFacts(
    IReadOnlyList<PortInfo> SourcePorts,
    IReadOnlyList<PortInfo> SinkPorts,
    IReadOnlyList<PrototypePorts> Prototypes);

/// <summary>The ports one entity of a document has now, which its own data can change, and how far its links reach.</summary>
public sealed record EntityPorts(
    int Uid,
    IReadOnlyList<string> Sources,
    IReadOnlyList<string> Sinks,
    /// <summary>The longest link the game lets it make as a source, in tiles. 0 if it is no source.</summary>
    float Range,
    /// <summary>Whether it keeps a device list, and whether it can be put on one.</summary>
    bool HasDeviceList,
    bool IsNetworkDevice);

public sealed record HistoryState(bool CanUndo, bool CanRedo, int Revision, string? UndoLabel, string? RedoLabel);

// Edits. One Edit call applies a list of these as a single undo step.

/// <summary>
/// One change to a document. <see cref="Kind"/> selects which of the other fields are read:
/// "tile" (Grid, X, Y, Tile, Variant, RotationMirroring), "spawn" (Prototype, Parent, X, Y, Rotation),
/// "delete" (Uid), "move" (Uid, Parent, X, Y), "rotate" (Uid, Rotation), "rename" (Uid, Name),
/// "decal" (Grid, X, Y, Prototype, Color, Rotation, ZIndex, Cleanable), "decal.update" (the same
/// plus Uid, the decal's id), "decal.remove" (Grid, Uid) and "components" (Uid, Components).
/// A decal's Prototype must be one the game has, except that a "decal.update" may name the
/// prototype the decal already has: a map can hold decals of prototypes the game lacks.
/// A "spawn" may carry Components too. Components is a JSON array in the form of
/// <see cref="EntityDetails.Components"/> and replaces the entity's stored component data,
/// apart from its Transform and MetaData.
/// "paste" (Text, Grid, FromX, FromY, X, Y, Quarters) makes new entities of the text a
/// <see cref="CopyResult"/> holds. FromX and FromY are the cell that answer names. X and Y are
/// where the lower left corner of that cell goes on the grid. Quarters is how many quarter
/// turns against the clock the copy is turned about the middle of that cell. A paste that
/// would put an entity on a cell with no tile is refused whole.
/// "contain" (Parent, Container, Prototype) makes a new entity inside the container of that
/// id on the entity Parent, with the game's container system. It may carry Components, as a
/// "spawn" may. "uncontain" (Uid) takes an entity that is inside a container out and deletes it.
/// "link" and "unlink" (Uid, Port, Target, TargetPort) wire the source port Port of the entity
/// Uid to the sink port TargetPort of the entity Target, or take that wire away, with the
/// game's device link system. A link that is there already, or an unlink of none, changes
/// nothing. "list.add" and "list.remove" (Uid, Target) put the device Target on the device
/// list of the entity Uid, or take it off.
/// "grid.add" (X, Y, Rotation, Name) makes a new empty grid on the document's map, there, with
/// that name, or the engine's "grid" if none is given. Its uid is in the delta's Created.
/// "grid.remove" (Grid) takes a grid away with all on it. A "rename" names a grid as it names
/// any entity. A document that is a grid file can neither gain nor lose a grid.
/// "air" (Grid, FromX, FromY, X, Y) carries the air the grid stores for the cell FromX, FromY
/// to the cell X, Y, as a region of tiles moves or turns. The cell X, Y then stores what the
/// first cell stored, or nothing if it stored nothing. The first cell keeps none, unless air
/// comes to it too. The air ops of one edit are carried all at once after its other ops, every
/// first cell read before any is written. A grid that stores no air is left alone. With Clear
/// it has no destination: the cell FromX, FromY keeps no air, unless air comes to it too, as
/// when a region is deleted.
/// "air.fix" (Grid) rebuilds the air the grid stores as the game's fixgridatmos does: every
/// floor standard air, walls none, a marker's tile the marker's mix, and the cells open to the
/// map's atmosphere the map's air (AirFix.cs). It is done after the edit's other ops and its
/// "air" ops, each grid once. A grid with no atmosphere is refused, as the game refuses it.
/// A grid whose air a fix would not change changes nothing.
/// "air.add" (Grid) gives a grid with no atmosphere one, as the game's addatmos and atmos:add
/// do: an empty GridAtmosphere, and a GasTileOverlay if it has none. A grid that has one is
/// refused, as the game refuses it. An "air.fix" of the same grid after it in one edit fills it.
/// An edit whose tiles change a grid's size gives the grid an atmosphere, as the game's
/// AutomaticAtmosSystem does, once it has none and its tiles weigh as much as seven (AirFix.cs).
/// </summary>
public sealed record EditOp(
    string Kind,
    int Uid = 0,
    int Grid = 0,
    int Parent = 0,
    float X = 0,
    float Y = 0,
    double Rotation = 0,
    string? Tile = null,
    int Variant = 0,
    int RotationMirroring = 0,
    string? Prototype = null,
    string? Name = null,
    string? Color = null,
    int ZIndex = 0,
    bool Cleanable = false,
    string? Components = null,
    string? Text = null,
    int FromX = 0,
    int FromY = 0,
    int Quarters = 0,
    string? Container = null,
    int Target = 0,
    string? Port = null,
    string? TargetPort = null,
    bool Clear = false);

/// <summary>One container of an entity, as the game's container system holds it.</summary>
public sealed record ContainerInfo(
    string Id,
    /// <summary>"Container" holds any number. "ContainerSlot" holds one.</summary>
    string Kind,
    /// <summary>What it holds, in the order the file lists it.</summary>
    IReadOnlyList<int> Entities,
    /// <summary>
    /// Whether the container itself has room for one more: a slot that is empty, or any other
    /// container. The rules of the game's own systems (a locker's capacity, what a slot
    /// accepts) are not asked, for the kernel does not run them.
    /// </summary>
    bool CanInsert);

/// <summary>The containers of one entity, in the order its component keeps them.</summary>
public sealed record EntityContainers(int Uid, IReadOnlyList<ContainerInfo> Containers);

/// <summary>
/// A set of entities of one grid, with all they hold, as the game's own entity text. What the
/// copies name among themselves is in the text. What they named outside the set is not.
/// </summary>
public sealed record CopyResult(
    /// <summary>What a "paste" op takes as its Text.</summary>
    string Text,
    /// <summary>How many entities of the grid the text holds.</summary>
    int Roots,
    /// <summary>How many entities it holds in all, with what those hold.</summary>
    int Entities,
    /// <summary>The lowest cell any of the entities of the grid stands on, each way.</summary>
    int FromX,
    int FromY,
    IReadOnlyList<KernelLogEntry> Log);

public sealed record SaveResult(bool Ok, string? Failure, string? Yaml, long Milliseconds, IReadOnlyList<KernelLogEntry> Log);

/// <summary>An entity prototype as the engine resolved it, for palettes and search.</summary>
public sealed record EntityPrototypeInfo(
    string Id,
    string Name,
    string Description,
    string? Suffix,
    IReadOnlyList<string> Parents,
    IReadOnlyList<string> Categories,
    bool Abstract,
    bool HideSpawnMenu,
    /// <summary>Placement mode the game uses when a mapper places this prototype.</summary>
    string PlacementMode,
    IReadOnlyList<string> Components,
    /// <summary>
    /// The tags of the prototype's Tag component, sorted (the game keeps them as a set).
    /// Empty if it has none.
    /// </summary>
    IReadOnlyList<string> Tags,
    /// <summary>Whether the prototype's Transform is anchored. False if it names none.</summary>
    bool Anchored,
    /// <summary>
    /// The exits of its DisposalTube, in its order, as the numbers of Direction before the
    /// entity is turned. Null if it is no disposal tube.
    /// </summary>
    IReadOnlyList<int>? TubeExits = null);

/// <summary>A decal prototype, for the palette.</summary>
public sealed record DecalPrototypeInfo(
    string Id,
    /// <summary>
    /// Rooted resource path of the RSI folder, or of the image itself when
    /// <see cref="SpriteState"/> is null. Null if the prototype names no picture.
    /// </summary>
    string? SpritePath,
    string? SpriteState,
    /// <summary>In the prototype's own order, not sorted: the first is the group a palette shows it under.</summary>
    IReadOnlyList<string> Tags,
    bool ShowMenu,
    bool SnapCardinals,
    /// <summary>This and the two after it are what the game's decal palette starts with for this decal.</summary>
    bool DefaultCustomColor,
    bool DefaultSnap,
    bool DefaultCleanable);

/// <summary>
/// An entity's component data as a map file would store it: per component, only what differs
/// from the prototype. References to other entities are engine uids.
/// </summary>
public sealed record EntityDetails(
    int Uid,
    /// <summary>A JSON array of component objects, each with a "type" member first.</summary>
    string Components);
