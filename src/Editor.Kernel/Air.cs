using Content.Server.Atmos.Components;
using Content.Shared.Atmos;
using Content.Shared.Atmos.Components;
using Content.Shared.Doors.Components;
using Content.Shared.Maps;
using Robust.Shared.GameObjects;
using Robust.Shared.Map;
using Robust.Shared.Map.Components;
using Robust.Shared.Maths;

namespace SS14.EditorKernel;

/// <summary>One gas mix as a grid stores it. Moles are by gas, in the order of <see cref="AirScene.Gases"/>.</summary>
public sealed record AirMix(float Temperature, float Volume, bool Immutable, float[] Moles);

/// <summary>
/// The air stored on one grid. <paramref name="Tiles"/> is three numbers a tile: x, y and the
/// index of its mix. <paramref name="Blocked"/> and <paramref name="Unfilled"/> are two numbers
/// a tile, of the grid's tiles that store no air: those a wall fills, and floors that would
/// turn to vacuum in the game. <paramref name="Cells"/> is three numbers for each tile of the
/// grid: x, y and what blocks air there, as <see cref="AirReader.Sides"/> and the flags after it say.
/// <paramref name="Atmosphere"/> is whether the grid has a GridAtmosphere at all, which the
/// game's fixgridatmos needs. <paramref name="Addable"/> is whether it has none and the editor
/// offers to add one as the game's atmos:add does: it is not the map's own ground, as a planet's
/// is (<see cref="AirFixer.Addable"/>).
/// </summary>
public sealed record GridAir(int Grid, IReadOnlyList<AirMix> Mixes, IReadOnlyList<int> Tiles, IReadOnlyList<int> Blocked, IReadOnlyList<int> Unfilled, IReadOnlyList<int> Cells, bool Atmosphere, bool Addable);

/// <param name="MapIsSpace">Whether the map's own atmosphere is space. False on a map whose air is breathable, as Cork's.</param>
public sealed record AirScene(IReadOnlyList<string> Gases, IReadOnlyList<GridAir> Grids, bool MapIsSpace);

/// <summary>
/// Reads the air a map's grids store, for the native editor's air overlay, and what blocks air
/// on each tile, for its rooms. It reads the components the map loader filled and changes
/// nothing. No atmosphere or door system runs here: what they would work out at the start of
/// a round is worked out from the components.
/// </summary>
internal static class AirReader
{
    private static readonly string[] GasNames = Enumerable.Range(0, Atmospherics.TotalNumberOfGases).Select(gas => ((Gas) gas).ToString()).ToArray();

    /// <summary>The low four bits of a cell's flags: the sides air cannot cross, as <see cref="AtmosDirection"/> (north 1, south 2, east 4, west 8).</summary>
    public const int Sides = 15;

    /// <summary>A flag of a cell: something that blocks air there leaves no air when all four sides are blocked.</summary>
    public const int NoAir = 16;

    /// <summary>A flag of a cell: its tile is open to the map's atmosphere, as space and lattice are.</summary>
    public const int Open = 32;

    /// <summary>
    /// A flag of a cell: an anchored airtight thing stands there, blocking or not, as an open
    /// door. The game gives such a floor with no air the mean of its neighbours' air and not
    /// vacuum (FixVacuum, true unless a prototype says otherwise, and none does).
    /// </summary>
    public const int FixVacuum = 64;

    /// <param name="only">The grids to read, or null for every grid of the map. A uid that is not a grid of it is left out.</param>
    public static AirScene Read(IEntityManager entMan, SharedMapSystem map, ITileDefinitionManager tiles, EntityUid mapUid, IReadOnlyCollection<int>? only)
    {
        var grids = new List<(EntityUid Uid, MapGridComponent Grid)>();
        var query = entMan.AllEntityQueryEnumerator<MapGridComponent, TransformComponent>();
        while (query.MoveNext(out var uid, out var grid, out var xform))
        {
            if (xform.MapUid == mapUid && (only == null || only.Contains(uid.Id)))
                grids.Add((uid, grid));
        }

        var blocked = AirtightCells(entMan, map, grids);
        var result = new List<GridAir>(grids.Count);
        foreach (var (uid, grid) in grids)
        {
            var mixes = new List<AirMix>();
            var indexOf = new Dictionary<GasMixture, int>();
            var stored = new List<int>();
            var held = new HashSet<Vector2i>();
            if (entMan.TryGetComponent(uid, out GridAtmosphereComponent? atmosphere))
            {
                // In the order the game's writer goes, which numbers the mixes as a save does.
                foreach (var (indices, tile) in atmosphere.Tiles)
                {
                    if (tile.Air is not { } air)
                        continue;

                    if (!indexOf.TryGetValue(air, out var index))
                    {
                        index = mixes.Count;
                        indexOf[air] = index;
                        mixes.Add(new AirMix(air.Temperature, air.Volume, air.Immutable, air.Moles[..Atmospherics.TotalNumberOfGases]));
                    }

                    stored.Add(indices.X);
                    stored.Add(indices.Y);
                    stored.Add(index);
                    held.Add(indices);
                }
            }

            var onGrid = blocked[uid];
            var walls = new List<int>();
            var unfilled = new List<int>();
            var cells = new List<int>();
            foreach (var tileRef in map.GetAllTiles(uid, grid))
            {
                var cell = tileRef.GridIndices;
                // A tile open to the map's atmosphere takes the map's air in the game.
                var open = tiles[tileRef.Tile.TypeId] is ContentTileDefinition { MapAtmosphere: true };
                var flags = onGrid.GetValueOrDefault(cell) | (open ? Open : 0);
                cells.Add(cell.X);
                cells.Add(cell.Y);
                cells.Add(flags);
                if (held.Contains(cell) || open)
                    continue;

                // The game gives no air to a tile blocked on all four sides by things of which
                // one leaves none (AtmosphereSystem.Processing.cs UpdateTileAir).
                var into = (flags & Sides) == Sides && (flags & NoAir) != 0 ? walls : unfilled;
                into.Add(cell.X);
                into.Add(cell.Y);
            }

            result.Add(new GridAir(uid.Id, mixes, stored, walls, unfilled, cells, atmosphere != null, AirFixer.Addable(entMan, uid)));
        }

        var mapIsSpace = !entMan.TryGetComponent(mapUid, out MapAtmosphereComponent? mapAtmosphere) || mapAtmosphere.Space;
        return new AirScene(GasNames, result, mapIsSpace);
    }

    /// <summary>
    /// What blocks air on each cell of these grids, as the game gathers it (AtmosphereSystem.Utils.cs
    /// GetAirtightData): the sides of every anchored airtight thing that blocks air, and whether
    /// one of those leaves no air when all four sides are blocked. Whether one fixes vacuum the
    /// game takes from every airtight thing, blocking or not. As <see cref="Sides"/> and the
    /// flags after it, by grid and cell. A cell with nothing airtight is not listed.
    /// </summary>
    public static Dictionary<EntityUid, Dictionary<Vector2i, int>> AirtightCells(IEntityManager entMan, SharedMapSystem map, IReadOnlyList<(EntityUid Uid, MapGridComponent Grid)> grids)
    {
        var blocked = new Dictionary<EntityUid, Dictionary<Vector2i, int>>();
        foreach (var (uid, _) in grids)
            blocked[uid] = new Dictionary<Vector2i, int>();
        var airtight = entMan.AllEntityQueryEnumerator<AirtightComponent, TransformComponent>();
        while (airtight.MoveNext(out var uid, out var wall, out var xform))
        {
            if (!xform.Anchored || xform.GridUid is not { } gridUid || !blocked.TryGetValue(gridUid, out var cells))
                continue;

            var grid = grids.First(found => found.Uid == gridUid).Grid;
            var cell = map.TileIndicesFor(gridUid, grid, xform.Coordinates);
            var flags = wall.FixVacuum ? FixVacuum : 0;
            if (BlocksAir(entMan, uid, wall))
                flags |= (int) SidesBlocked(wall, xform.LocalRotation) | (wall.NoAirWhenFullyAirBlocked ? NoAir : 0);
            cells[cell] = cells.GetValueOrDefault(cell) | flags;
        }

        return blocked;
    }

    /// <summary>
    /// Whether an airtight thing blocks air as a round starts. A door decides it by its state
    /// (DoorSystem.SetCollidable, called by SharedDoorSystem.OnComponentInit). The map loader
    /// keeps no time of a door's next change, so a door saved opening is open and one saved
    /// closing is closed, and then only a closed door blocks. A welded one does not: the game's
    /// start counts only Closed. Anything else blocks as its data says.
    /// </summary>
    public static bool BlocksAir(IEntityManager entMan, EntityUid uid, AirtightComponent airtight)
    {
        if (!entMan.TryGetComponent(uid, out DoorComponent? door) || !door.ChangeAirtight)
            return airtight.AirBlocked;

        return door.State is DoorState.Closed or DoorState.Closing;
    }

    /// <summary>
    /// The sides of its cell a thing blocks, on its grid: the prototype's sides turned by the
    /// thing's own rotation, as AirtightSystem.Rotate turns them. A thing that blocks all four
    /// is not turned.
    /// </summary>
    public static AtmosDirection SidesBlocked(AirtightComponent airtight, Angle rotation)
    {
        var own = (AtmosDirection) airtight.InitialAirBlockedDirection;
        if (own == AtmosDirection.All || rotation == Angle.Zero)
            return own;

        var turned = AtmosDirection.Invalid;
        for (var i = 0; i < Atmospherics.Directions; i++)
        {
            var direction = (AtmosDirection) (1 << i);
            if (own.IsFlagSet(direction))
                turned |= (direction.ToAngle() + rotation).ToAtmosDirectionCardinal();
        }

        return turned;
    }
}
