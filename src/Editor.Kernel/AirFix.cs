using Content.Server.Atmos.Components;
using Content.Shared.Atmos;
using Content.Shared.Atmos.Components;
using Content.Shared.Maps;
using Content.Shared.Shuttles.Systems;
using Robust.Shared.GameObjects;
using Robust.Shared.Map;
using Robust.Shared.Map.Components;
using Robust.Shared.Maths;
using Robust.Shared.Physics;
using Robust.Shared.Physics.Dynamics;
using Robust.Shared.Physics.Systems;

namespace SS14.EditorKernel;

/// <summary>
/// What fixing a grid's air would change: how many of its tiles would store different air, and how
/// many cells off its edge. <paramref name="Refused"/> says why it can't be fixed, and then the
/// counts are 0. The exception is <paramref name="Addable"/>. That grid has no atmosphere, the
/// editor offers to add one first the way the game's atmos:add does, and the counts are what
/// adding one and then fixing would change.
/// </summary>
public sealed record AirFixPreview(int Grid, string? Refused, int Tiles, int Outside, bool Addable = false);

/// <summary>
/// The game's fixgridatmos, done on a grid's data with no game system running. The steps are
/// those of AtmosphereSystem.RebuildGridAtmosphere and RebuildGridTiles
/// (Content.Server/Atmos/EntitySystems/AtmosphereSystem.Commands.cs 69-173 in the fork), in
/// the same order and with the same values, so that the stored air comes out as the game
/// leaves it. Whoever bumps the fork compares those lines with this file.
///
/// It is the fix as a mapper runs it, on a paused map: nothing of the running game follows.
/// The cells off the grid keep no air, no gas moves, no fire burns, and no device is told.
/// </summary>
internal static class AirFixer
{
    /// <summary>
    /// Why the game's fixgridatmos would refuse this grid, or null if it would fix it. It
    /// refuses a grid with no GridAtmosphere and says to add one with addatmos
    /// (AtmosphereSystem.Commands.cs 55-59). Starlight's toolshed atmos:fix adds one instead
    /// (Content.Server/_Starlight/Atmos/AtmosCommand.cs 37-48). The editor follows fixgridatmos.
    /// </summary>
    public static string? Refusal(IEntityManager entMan, EntityUid grid)
    {
        if (!entMan.HasComponent<MapGridComponent>(grid))
            return $"Entity {grid.Id} is no grid.";
        if (!entMan.HasComponent<GridAtmosphereComponent>(grid))
            return $"The grid {Name(entMan, grid)} has no atmosphere. The game's fixgridatmos refuses it too. A grid gets one with the game's addatmos.";
        return null;
    }

    /// <summary>
    /// Why the game's addatmos would refuse this grid, or null if it would give it an
    /// atmosphere. It refuses what is no grid and a grid that has one already
    /// (Content.Server/Atmos/Commands/AddAtmosCommand.cs 33-45). Starlight's toolshed atmos:add
    /// refuses the same (Content.Server/_Starlight/Atmos/AtmosCommand.cs 50-63). Neither asks
    /// what map the grid is on.
    /// </summary>
    public static string? AddRefusal(IEntityManager entMan, EntityUid grid)
    {
        if (!entMan.HasComponent<MapGridComponent>(grid))
            return $"Entity {grid.Id} is no grid.";
        if (entMan.HasComponent<GridAtmosphereComponent>(grid))
            return $"The grid {Name(entMan, grid)} has an atmosphere already.";
        return null;
    }

    /// <summary>
    /// Whether the editor offers to add an atmosphere to this grid: it has none and is not the
    /// map's own ground, as a planet's is. The game gives a planet's ground none: its air is the
    /// map's (BiomeSystem SetMapAtmosphere), and the game never builds the ground's collision,
    /// so the automatic atmosphere below never comes to it (SharedMapSystem.Grid.cs RegenerateCollision).
    /// </summary>
    public static bool Addable(IEntityManager entMan, EntityUid grid) =>
        entMan.HasComponent<MapGridComponent>(grid) && !entMan.HasComponent<GridAtmosphereComponent>(grid) && !entMan.HasComponent<MapComponent>(grid);

    /// <summary>
    /// Whether the game would give this grid an atmosphere by itself now. Its
    /// AutomaticAtmosSystem does, as a grid's mass changes, once the mass is that of seven tiles
    /// (Content.Server/Atmos/EntitySystems/AutomaticAtmosSystem.cs 22-38). The mass is its
    /// collision's, each tile's fixture at the shuttle density
    /// (Content.Shared/Shuttles/Systems/SharedShuttleSystem.cs 50-57), worked out as the
    /// engine's ResetMassData does (SharedPhysicsSystem.Components.cs 331-370). The engine
    /// builds the same fixtures here, and only the density is the game's to set.
    /// </summary>
    public static bool WouldGainAtmosphere(IEntityManager entMan, EntityUid grid)
    {
        if (!Addable(entMan, grid) || !entMan.TryGetComponent(grid, out FixturesComponent? fixtures))
            return false;

        var mass = 0f;
        foreach (var fixture in fixtures.Fixtures.Values)
        {
            var data = new MassData();
            FixtureSystem.GetMassData(fixture.Shape, ref data, SharedShuttleSystem.TileDensityMultiplier);
            mass += data.Mass;
        }

        // A body with no mass is given a mass of one.
        if (mass <= 0f)
            mass = 1f;
        return mass / SharedShuttleSystem.TileDensityMultiplier >= 7.0f;
    }

    private static string Name(IEntityManager entMan, EntityUid uid) =>
        entMan.TryGetComponent(uid, out MetaDataComponent? meta) && !string.IsNullOrEmpty(meta.EntityName) ? $"{meta.EntityName} ({uid.Id})" : uid.Id.ToString();

    /// <summary>
    /// The nine mixes a marker can ask for, by its mode, each a new mix of one cell at 20 °C
    /// (AtmosphereSystem.Commands.cs 71-107).
    /// </summary>
    private static GasMixture[] Presets()
    {
        var mixtures = new GasMixture[9];
        for (var i = 0; i < mixtures.Length; i++)
            mixtures[i] = new GasMixture(Atmospherics.CellVolume) { Temperature = Atmospherics.T20C };

        // 0: air.
        mixtures[0].AdjustMoles(Gas.Oxygen, Atmospherics.OxygenMolesStandard);
        mixtures[0].AdjustMoles(Gas.Nitrogen, Atmospherics.NitrogenMolesStandard);
        // 1: vacuum.
        // 2: oxygen at gas miner pressure.
        mixtures[2].AdjustMoles(Gas.Oxygen, Atmospherics.MolesCellGasMiner);
        // 3: nitrogen at gas miner pressure.
        mixtures[3].AdjustMoles(Gas.Nitrogen, Atmospherics.MolesCellGasMiner);
        // 4: plasma at gas miner pressure.
        mixtures[4].AdjustMoles(Gas.Plasma, Atmospherics.MolesCellGasMiner);
        // 5: a plasma fire.
        mixtures[5].AdjustMoles(Gas.Oxygen, Atmospherics.MolesCellGasMiner);
        mixtures[5].AdjustMoles(Gas.Plasma, Atmospherics.MolesCellGasMiner);
        mixtures[5].Temperature = 5000f;
        // 6: a walk-in freezer.
        mixtures[6].AdjustMoles(Gas.Oxygen, Atmospherics.OxygenMolesFreezer);
        mixtures[6].AdjustMoles(Gas.Nitrogen, Atmospherics.NitrogenMolesFreezer);
        mixtures[6].Temperature = Atmospherics.FreezerTemp;
        // 7: nitrogen at one atmosphere, for vox rooms.
        mixtures[7].AdjustMoles(Gas.Nitrogen, Atmospherics.MolesCellStandard);
        // 8: air at gas miner pressure.
        mixtures[8].AdjustMoles(Gas.Oxygen, Atmospherics.OxygenMolesGasMiner);
        mixtures[8].AdjustMoles(Gas.Nitrogen, Atmospherics.NitrogenMolesGasMiner);
        return mixtures;
    }

    /// <summary>
    /// The map's own air, which every cell open to it shares: the map's MapAtmosphere mix, or
    /// space with none (AtmosphereSystem.GridAtmosphere.cs GetDefaultMapAtmosphere). The game
    /// marks the map's mix immutable as the component starts (AtmosphereSystem.Map.cs
    /// OnMapStartup). No system runs here, so a mix the file did not mark is marked on a copy.
    /// </summary>
    private static GasMixture MapAir(IEntityManager entMan, EntityUid? mapUid)
    {
        if (mapUid is not { } map || !entMan.TryGetComponent(map, out MapAtmosphereComponent? atmosphere))
            return GasMixture.SpaceGas;

        if (atmosphere.Mixture.Immutable)
            return atmosphere.Mixture;

        var marked = new GasMixture(atmosphere.Mixture);
        marked.MarkImmutable();
        return marked;
    }

    /// <summary>
    /// The grid's stored air as the game's fixgridatmos leaves it, in the order its dictionary
    /// would hold it, since the save orders mixes and chunks by that. Changes nothing: the
    /// caller puts it in place. Call <see cref="Refusal"/> first.
    /// </summary>
    public static KeyValuePair<Vector2i, TileAtmosphere>[] Rebuild(IEntityManager entMan, SharedMapSystem map, ITileDefinitionManager tiles, EntityUid gridUid, MapGridComponent grid)
    {
        var mixtures = Presets();
        var mapAir = MapAir(entMan, entMan.GetComponent<TransformComponent>(gridUid).MapUid);
        // GetVolumeForTiles (AtmosphereSystem.Utils.cs 83-86).
        var volume = Atmospherics.CellVolume * grid.TileSize;
        var airtight = AirReader.AirtightCells(entMan, map, [(gridUid, grid)])[gridUid];

        // Tiles.Clear() (Commands.cs 160). A dictionary that only grows keeps its entries in
        // the order they were added, as the game's does after it is cleared.
        var built = new Dictionary<Vector2i, TileAtmosphere>();
        TileAtmosphere GetOrNew(Vector2i cell)
        {
            // AtmosphereSystem.Processing.cs GetOrNewTile.
            if (!built.TryGetValue(cell, out var tile))
                built[cell] = tile = new TileAtmosphere { GridIndex = gridUid, GridIndices = cell };
            return tile;
        }

        // Every tile of the grid in the engine's order (Commands.cs 165-172).
        var enumerator = map.GetAllTilesEnumerator(gridUid, grid);
        while (enumerator.MoveNext(out var tileRef))
        {
            var cell = tileRef.Value.GridIndices;
            var tile = GetOrNew(cell);

            // UpdateTileData (Processing.cs 173-225): a tile open to the map's atmosphere takes
            // the map's air.
            var open = tiles[tileRef.Value.Tile.TypeId] is ContentTileDefinition { MapAtmosphere: true };
            if (open)
            {
                tile.Air = mapAir;
                tile.MapAtmosphere = true;
            }

            // UpdateAdjacentTiles (AtmosphereSystem.GridAtmosphere.cs 162-224): a tile of the
            // grid makes each of its four neighbours, north, south, east and west in turn.
            for (var i = 0; i < Atmospherics.Directions; i++)
                GetOrNew(cell.Offset((AtmosDirection) (1 << i)));

            // UpdateTileAir (Processing.cs 244-284): none under things that block all four
            // sides of which one leaves no air, and else a new mix at 20 °C. The new mix of a
            // tile that fixes vacuum is filled from its neighbours there, and emptied again
            // below, so that step is left out.
            if (open)
                continue;

            var flags = airtight.GetValueOrDefault(cell);
            if ((flags & AirReader.Sides) == AirReader.Sides && (flags & AirReader.NoAir) != 0)
                tile.Air = null;
            else
                tile.Air ??= new GasMixture(volume) { Temperature = Atmospherics.T20C };
        }

        // Every mutable mix filled with air, or with the mix of the marker anchored there, the
        // last one the engine lists if there are several (Commands.cs 116-134).
        foreach (var (cell, tile) in built)
        {
            if (tile.Air is not { Immutable: false } air)
                continue;

            air.Clear();
            var mode = 0;
            var anchored = map.GetAnchoredEntitiesEnumerator(gridUid, grid, cell);
            while (anchored.MoveNext(out var uid))
            {
                if (uid is { } anchoredUid && entMan.TryGetComponent(anchoredUid, out AtmosFixMarkerComponent? marker))
                    mode = marker.Mode;
            }

            // The game would stop on the first such marker with the grid half fixed.
            if (mode < 0 || mode >= mixtures.Length)
                throw new KernelException($"The marker at {cell.X}, {cell.Y} of grid {gridUid.Id} asks for mix {mode}, and the game knows only 0 to {mixtures.Length - 1}. Nothing was fixed.");

            // Merge into an empty mix and then the mix's temperature, as the game does: the
            // moles are the preset's own and so is the temperature.
            var mixture = mixtures[mode];
            for (var gas = 0; gas < Atmospherics.TotalNumberOfGases; gas++)
                air.AdjustMoles(gas, mixture.Moles[gas]);
            air.Temperature = mixture.Temperature;
        }

        // The cells off the grid that were made as neighbours keep no air. A running game would
        // give them the map's air on its next tick (Processing.cs ProcessRevalidate and
        // UpdateTileData), but a map being mapped is paused (Content.Server/Mapping/MappingCommand.cs
        // 173) and its atmosphere does not run (Processing.cs 661), so a map fixed and saved
        // there stores none of them. The fork's maps agree: 110 of 116 surveyed store no such
        // cell, and the rest a stray few, where a save after a tick stores the whole ring round
        // the grid. They stay in the dictionary with no air, as the game's do.
        return built.ToArray();
    }

    /// <summary>
    /// How many cells would store other air after a rebuild than now, those with a tile of
    /// the grid and those off it. A cell that stores air on one side and none on the other
    /// counts, and so does one whose mix would differ in anything a save writes.
    /// </summary>
    public static (int Tiles, int Outside) Changes(SharedMapSystem map, EntityUid gridUid, MapGridComponent grid, IReadOnlyDictionary<Vector2i, TileAtmosphere> before, IReadOnlyList<KeyValuePair<Vector2i, TileAtmosphere>> after)
    {
        var (tiles, outside) = (0, 0);
        var seen = new HashSet<Vector2i>();
        void Count(Vector2i cell, GasMixture? was, GasMixture? now)
        {
            if (!seen.Add(cell) || SameMix(was, now))
                return;

            if (map.TryGetTile(grid, cell, out var tile) && !tile.IsEmpty)
                tiles++;
            else
                outside++;
        }

        foreach (var (cell, tile) in after)
            Count(cell, before.GetValueOrDefault(cell)?.Air, tile.Air);
        foreach (var (cell, tile) in before)
            Count(cell, tile.Air, null);
        return (tiles, outside);
    }

    /// <summary>Whether two stored airs are the same in everything a save writes of them.</summary>
    public static bool SameMix(GasMixture? a, GasMixture? b)
    {
        if (a == null || b == null)
            return a == b;

        return a.Temperature.Equals(b.Temperature) && a.Volume.Equals(b.Volume) && a.Immutable == b.Immutable
               && a.Moles.AsSpan(0, Atmospherics.TotalNumberOfGases).SequenceEqual(b.Moles.AsSpan(0, Atmospherics.TotalNumberOfGases));
    }

    /// <summary>Whether two sets of stored air are the same, cell by cell and in the same order, so that they save the same.</summary>
    public static bool SameAir(IReadOnlyList<KeyValuePair<Vector2i, TileAtmosphere>> a, IReadOnlyList<KeyValuePair<Vector2i, TileAtmosphere>> b)
    {
        if (a.Count != b.Count)
            return false;

        for (var i = 0; i < a.Count; i++)
        {
            if (a[i].Key != b[i].Key || !SameMix(a[i].Value.Air, b[i].Value.Air))
                return false;
        }

        return true;
    }
}
