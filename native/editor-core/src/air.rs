//! The air overlay. It holds the air each grid stores, as the engine read it (`air` in
//! NativeApi.cs), the colour each view gives that air, the legend that names the colours, the
//! text of the hover box over a tile, and the rectangles the renderer draws.
//!
//! The overlay shows what the map file stores, and nothing the game would work out later. A tile
//! with no stored air is either a wall that holds none, left uncoloured, or a floor that holds
//! none and would turn to vacuum in the game, drawn in yellow and black caution tape. A floor
//! with none under an open door is neither. The game gives it its neighbours' air, and the
//! overlay shows it that way.
//!
//! The Breathable view's colours are Okabe and Ito's, picked so red-green colour blind users can
//! tell them apart. They differ in brightness as well as hue, and each has its own mark, so no two
//! rely on hue alone. The pressure and temperature scales rise evenly in brightness from dark blue
//! to bright yellow, like cividis.
//!
//! The Rooms view shows the leak check instead (`crate::rooms`). Each room gets a soft colour.
//! A room open to the outside is outlined in orange dashes, with arrows pointing out through its
//! gaps, and the outside itself stays uncoloured. `crate::air_draw` draws all of it.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::air_checks::{self, GridFaults};
use crate::model::{Delta, SceneJson};
use crate::rooms::{self, GridRooms};

// The game's numbers. Paths are under upstream/starlight.

/// The gas constant (Content.Shared/Atmos/Atmospherics.cs, R).
pub(crate) const GAS_CONSTANT: f32 = 8.314_462_6;
/// Zero Celsius in kelvin (Atmospherics.T0C).
const T0C: f32 = 273.15;
/// At or below this pressure a body takes low pressure damage, as in vacuum
/// (Atmospherics.HazardLowPressure, used by Content.Server/Atmos/EntitySystems/BarotraumaSystem.cs).
pub const HAZARD_LOW_PRESSURE: f32 = 20.0;
/// Below this the station's air alarms call the pressure dangerous, since there is too
/// little oxygen in a breath (Resources/Prototypes/Atmospherics/Thresholds/airalarms.yml,
/// stationPressure lowerBound).
pub const THIN_PRESSURE: f32 = 85.0;
/// One atmosphere, normal station air (Atmospherics.OneAtmosphere).
pub const ONE_ATMOSPHERE: f32 = 101.325;
/// From this pressure the game warns of high pressure (Atmospherics.WarningHighPressure,
/// 0.7 of HazardHighPressure, used by BarotraumaSystem.cs).
pub const WARNING_HIGH_PRESSURE: f32 = 385.0;
/// From this pressure a body takes high pressure damage (Atmospherics.HazardHighPressure).
pub const HAZARD_HIGH_PRESSURE: f32 = 550.0;
/// The share of oxygen the station's air alarms take as safe (airalarms.yml stationOxygen,
/// danger at 17% and at 45%).
pub const OXYGEN_LOW: f32 = 0.17;
pub const OXYGEN_HIGH: f32 = 0.45;
/// The smallest share of a gas other than oxygen, nitrogen and water vapour that a station
/// alarm calls dangerous: tritium's 0.4% (airalarms.yml stationTritium). Water vapour is left
/// out as the alarms leave it out up to 150%.
pub const OTHER_GAS: f32 = 0.004;
/// The temperature of freezer air (Atmospherics.FreezerTemp). Air this cold or colder is a
/// freezer's, and not the station's.
pub const FREEZER: f32 = 235.0;
/// The temperature a body takes heat damage at (heatDamageThreshold of MobAtmosStandard,
/// Resources/Prototypes/Entities/Mobs/base.yml).
pub const HEAT_DAMAGE: f32 = 325.0;
/// The temperature of space (Atmospherics.TCMB).
pub const SPACE_TEMPERATURE: f32 = 2.7;
/// The station alarms' danger above this temperature (airalarms.yml stationTemperature upperBound).
pub const ALARM_HOT: f32 = 393.15;

/// One gas mix as a grid stores it.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Mix {
    /// Kelvin.
    pub temperature: f32,
    /// Litres.
    pub volume: f32,
    /// The map's own atmosphere, on space and lattice and the cells beside the grid.
    pub immutable: bool,
    /// By gas, in the order of [`AirMirror::gases`].
    pub moles: Vec<f32>,
}

impl Mix {
    pub fn total_moles(&self) -> f32 {
        // From plus zero: a sum of nothing is minus zero, which would be shown as "-0.0 kPa".
        self.moles.iter().fold(0.0, |total, moles| total + moles)
    }

    /// In kPa: moles times R times temperature over volume, as the game's GasMixture.Pressure.
    pub fn pressure(&self) -> f32 {
        if self.volume <= 0.0 {
            return 0.0;
        }
        self.total_moles() * GAS_CONSTANT * self.temperature / self.volume
    }

    pub fn celsius(&self) -> f32 {
        self.temperature - T0C
    }

    /// The share of one gas, by its name, from 0 to 1.
    pub(crate) fn share(&self, gases: &[String], name: &str) -> f32 {
        let total = self.total_moles();
        match gases.iter().position(|gas| gas == name) {
            Some(index) if total > 0.0 => self.moles.get(index).copied().unwrap_or(0.0) / total,
            _ => 0.0,
        }
    }
}

/// The air one grid stores, and its rooms.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridAir {
    pub mixes: Vec<Mix>,
    /// The index of each tile's mix.
    pub tiles: HashMap<(i32, i32), u32>,
    /// Tiles of the grid with no air: walls that hold none, and floors that would turn to vacuum.
    pub walls: HashSet<(i32, i32)>,
    pub unfilled: HashSet<(i32, i32)>,
    /// Floors with no air under an open door or another airtight thing, with the air the game
    /// gives them from their neighbours. They are not among `unfilled`.
    pub filled: HashMap<(i32, i32), Mix>,
    /// The rooms of the grid, from what blocks air on its tiles (`crate::rooms`).
    pub rooms: GridRooms,
    /// The faults of its stored air, for Validate Map (`crate::air_checks`).
    pub faults: GridFaults,
    /// Whether it has an atmosphere at all. The game's fixgridatmos refuses a grid with none.
    pub atmosphere: bool,
    /// Whether it has none and can be given one, as the game's atmos:add gives one: it is not
    /// the map's own ground, as a planet's is.
    pub addable: bool,
}

/// What the engine answers to `air`.
#[derive(Deserialize, Debug)]
pub struct AirAnswer {
    pub gases: Vec<String>,
    pub grids: Vec<GridAirJson>,
    /// Whether the map's own atmosphere is space. An engine from before it said is taken as space.
    #[serde(rename = "mapIsSpace", default = "space")]
    pub map_is_space: bool,
}

fn space() -> bool {
    true
}

impl Default for AirAnswer {
    fn default() -> AirAnswer {
        AirAnswer { gases: Vec::new(), grids: Vec::new(), map_is_space: true }
    }
}

#[derive(Deserialize, Debug, Default)]
pub struct GridAirJson {
    pub grid: i32,
    pub mixes: Vec<Mix>,
    /// Three numbers a tile: x, y and its mix.
    pub tiles: Vec<i32>,
    /// Two numbers a tile, each.
    pub blocked: Vec<i32>,
    pub unfilled: Vec<i32>,
    /// Three numbers for each tile of the grid: x, y and what blocks air there
    /// (`crate::rooms`, its flags). None from an engine from before it told them.
    #[serde(default)]
    pub cells: Vec<i32>,
    /// Whether the grid has an atmosphere at all, which the game's fixgridatmos needs. None
    /// from an engine from before it told, taken as yes.
    #[serde(default)]
    pub atmosphere: Option<bool>,
    /// Whether it has none and can be given one (`GridAir::addable`). None from an engine from
    /// before it told, taken as no.
    #[serde(default)]
    pub addable: Option<bool>,
    /// The rooms, found on the engine's thread as the answer came, so that no frame waits for
    /// them. Found when the answer is taken in otherwise.
    #[serde(skip)]
    pub rooms: Option<GridRooms>,
    /// The faults of its air, found with the rooms.
    #[serde(skip)]
    pub faults: Option<GridFaults>,
}

/// The mix of each tile from the engine's three numbers a tile: x, y and the mix.
fn tiles_of(numbers: &[i32]) -> HashMap<(i32, i32), u32> {
    numbers.chunks_exact(3).map(|tile| ((tile[0], tile[1]), tile[2] as u32)).collect()
}

impl GridAirJson {
    /// Finds the grid's rooms and the faults of its air, once.
    pub fn find_rooms(&mut self) {
        if self.rooms.is_some() && self.faults.is_some() {
            return;
        }
        let cells = rooms::cells_of(&self.cells);
        let rooms = self.rooms.take().unwrap_or_else(|| rooms::find(&cells));
        if self.faults.is_none() {
            self.faults = Some(air_checks::find(&self.mixes, &tiles_of(&self.tiles), &cells, &rooms));
        }
        self.rooms = Some(rooms);
    }

    fn into_air(mut self) -> GridAir {
        self.find_rooms();
        let pairs = |numbers: &[i32]| numbers.chunks_exact(2).map(|pair| (pair[0], pair[1])).collect();
        let tiles = tiles_of(&self.tiles);
        let mut unfilled: HashSet<(i32, i32)> = pairs(&self.unfilled);
        let filled = filled_from_neighbours(&unfilled, &tiles, &self.mixes, &rooms::cells_of(&self.cells));
        unfilled.retain(|cell| !filled.contains_key(cell));
        GridAir {
            tiles,
            walls: pairs(&self.blocked),
            unfilled,
            filled,
            mixes: self.mixes,
            rooms: self.rooms.unwrap_or_default(),
            faults: self.faults.unwrap_or_default(),
            atmosphere: self.atmosphere != Some(false),
            addable: self.atmosphere == Some(false) && self.addable == Some(true),
        }
    }
}

/// The floors with no air under an airtight thing, as an open door, with the air the game gives
/// them as the round starts: the mean of the air of the neighbours that share air with them and
/// store some, its moles and its temperature each (AtmosphereSystem.GridAtmosphere.cs
/// GridFixTileVacuum, run by UpdateTileAir for a thing with FixVacuum). One with no such
/// neighbour, as a closed door, keeps none, and the game makes it vacuum.
fn filled_from_neighbours(unfilled: &HashSet<(i32, i32)>, tiles: &HashMap<(i32, i32), u32>, mixes: &[Mix], cells: &HashMap<(i32, i32), u8>) -> HashMap<(i32, i32), Mix> {
    let flags = |cell: &(i32, i32)| cells.get(cell).copied().unwrap_or(0);
    let mut filled = HashMap::new();
    for cell in unfilled.iter().filter(|cell| flags(cell) & rooms::FIX_VACUUM != 0) {
        let neighbours: Vec<&Mix> = rooms::SIDES
            .iter()
            .filter_map(|&(side, (dx, dy), shared)| {
                let next = (cell.0 + dx, cell.1 + dy);
                let open = flags(cell) & side == 0 && flags(&next) & shared == 0;
                open.then(|| tiles.get(&next).and_then(|&index| mixes.get(index as usize))).flatten()
            })
            .collect();
        let Some(first) = neighbours.first() else { continue };
        let count = neighbours.len() as f32;
        let gases = neighbours.iter().map(|mix| mix.moles.len()).max().unwrap_or(0);
        let moles = (0..gases).map(|gas| neighbours.iter().map(|mix| mix.moles.get(gas).copied().unwrap_or(0.0)).sum::<f32>() / count).collect();
        let temperature = neighbours.iter().map(|mix| mix.temperature).sum::<f32>() / count;
        filled.insert(*cell, Mix { temperature, volume: first.volume, immutable: false, moles });
    }
    filled
}

/// What one tile holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TileAir<'a> {
    Stored(&'a Mix),
    /// A floor with no air under an open door, with the air the game gives it from its neighbours.
    Filled(&'a Mix),
    /// A wall that holds no air.
    Wall,
    /// A floor with no air, that would turn to vacuum in the game.
    Unfilled,
    Nothing,
}

/// The mirror of the air of the open document, kept as the engine reads it again for the
/// grids an edit touched.
#[derive(Debug, Clone, Default)]
pub struct AirMirror {
    /// The game's gases, in the order a mix's moles are.
    pub gases: Vec<String>,
    pub grids: HashMap<i32, GridAir>,
    /// Whether the map's own atmosphere is air, as Cork's is, and not space.
    pub map_has_air: bool,
    /// Counts up at every change, so that what is drawn from it is built again only then.
    pub revision: u64,
}

impl AirMirror {
    pub fn from_answer(answer: AirAnswer) -> AirMirror {
        let mut mirror = AirMirror::default();
        mirror.update(None, answer);
        mirror
    }

    /// Takes in the air of the grids read again. Of `asked`, a grid the engine did not answer
    /// for is gone. With none asked, the answer is the whole document's.
    pub fn update(&mut self, asked: Option<&[i32]>, answer: AirAnswer) {
        match asked {
            Some(grids) => grids.iter().for_each(|grid| {
                self.grids.remove(grid);
            }),
            None => self.grids.clear(),
        }
        if !answer.gases.is_empty() {
            self.gases = answer.gases;
        }
        self.map_has_air = !answer.map_is_space;
        for grid in answer.grids {
            self.grids.insert(grid.grid, grid.into_air());
        }
        self.revision += 1;
    }

    /// What a cell of a grid holds.
    pub fn at(&self, grid: i32, cell: (i32, i32)) -> TileAir<'_> {
        let Some(air) = self.grids.get(&grid) else { return TileAir::Nothing };
        if let Some(mix) = air.tiles.get(&cell).and_then(|&index| air.mixes.get(index as usize)) {
            TileAir::Stored(mix)
        } else if let Some(mix) = air.filled.get(&cell) {
            TileAir::Filled(mix)
        } else if air.walls.contains(&cell) {
            TileAir::Wall
        } else if air.unfilled.contains(&cell) {
            TileAir::Unfilled
        } else {
            TileAir::Nothing
        }
    }
}

/// The four views of the overlay. The first three colour the air each tile stores, and Rooms
/// the rooms its airtight things make (`crate::rooms`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AirView {
    #[default]
    Breathable,
    Pressure,
    Temperature,
    Rooms,
}

impl AirView {
    pub const ALL: [AirView; 4] = [AirView::Breathable, AirView::Pressure, AirView::Temperature, AirView::Rooms];

    pub fn name(self) -> &'static str {
        match self {
            AirView::Breathable => "Breathable",
            AirView::Pressure => "Pressure",
            AirView::Temperature => "Temperature",
            AirView::Rooms => "Rooms",
        }
    }
}

/// The soft colours of rooms, so that neighbours are told apart. Paul Tol's light colours,
/// which stay apart for red-green colour blindness, with pale grey in place of his orange,
/// which is the leaks'.
pub const ROOM_COLOURS: [[u8; 3]; rooms::COLOURS] = [
    [119, 170, 221],
    [238, 221, 136],
    [68, 187, 153],
    [255, 170, 187],
    [153, 221, 255],
    [187, 204, 51],
    [221, 221, 221],
    [170, 170, 0],
];

/// The orange of a leak: the dashed outline of a room open to the outside, its arrows and its label.
pub const LEAK: [u8; 3] = [230, 159, 0];

/// What the outside is called: space, or on a map whose atmosphere is air, as Cork's, that.
pub(crate) fn outside_name(map_has_air: bool) -> &'static str {
    if map_has_air { "the map's atmosphere" } else { "space" }
}

/// "1 tile" or "n tiles".
fn tile_count(count: usize) -> String {
    if count == 1 { "1 tile".to_owned() } else { format!("{count} tiles") }
}

/// The text of a room as the box by the mouse and Validate Map tell it, by its number from 1:
/// "Room 12, 48 tiles, sealed", or open to space through its leaks.
pub fn room_text(number: usize, room: &rooms::Room, map_has_air: bool) -> String {
    let state = if room.is_open() { format!("open to {} through {}", outside_name(map_has_air), tile_count(room.leaks.len())) } else { "sealed".to_owned() };
    format!("Room {number}, {}, {state}", tile_count(room.size))
}

/// The box by the mouse in the Rooms view: the room of the tile, the outside, or a wall. A
/// sealed room with a fault of its air says it on a second line (`crate::air_checks`). None
/// over a tile with nothing to tell.
pub fn room_hover(mirror: &AirMirror, grid: i32, cell: (i32, i32)) -> Option<Vec<String>> {
    let air = mirror.grids.get(&grid)?;
    let Some(&number) = air.rooms.room_of.get(&cell) else {
        return air.walls.contains(&cell).then(|| vec!["Wall".to_owned(), "Blocks all sides, so it holds no air".to_owned()]);
    };
    let room = &air.rooms.rooms[number];
    if room.is_outside() {
        return Some(vec!["Outside".to_owned(), format!("Open to {} on {} of its {}", outside_name(mirror.map_has_air), room.leaks.len(), tile_count(room.size))]);
    }
    let mut lines = vec![room_text(number + 1, room, mirror.map_has_air)];
    lines.extend(air.faults.rooms.get(&number).and_then(|faults| faults.hover_line(&mirror.gases)));
    Some(lines)
}

/// How many rooms of the open document are sealed, and how many are open, the outside left out.
pub fn room_counts(mirror: &AirMirror) -> (usize, usize) {
    let rooms = mirror.grids.values().flat_map(|air| &air.rooms.rooms).filter(|room| !room.is_outside());
    rooms.fold((0, 0), |(sealed, open), room| if room.is_open() { (sealed, open + 1) } else { (sealed + 1, open) })
}

// Okabe and Ito's colours, apart for every kind of colour vision.
const SKY: [u8; 3] = [86, 180, 233];
const BLUE: [u8; 3] = [0, 114, 178];
const YELLOW: [u8; 3] = [240, 228, 66];
const ORANGE: [u8; 3] = [230, 159, 0];
const PURPLE: [u8; 3] = [204, 121, 167];
const NEAR_BLACK: [u8; 3] = [20, 20, 28];

/// The mark drawn on each tile of a kind, so that the kinds differ in shape as well as colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mark {
    None,
    /// Three lines crossed in the middle.
    Snowflake,
    /// Three dots.
    Dots,
    /// Upright bars.
    Bars,
    /// Wavy lines across.
    Waves,
    /// Three small stars.
    Stars,
    /// Yellow and black stripes over the whole tile.
    Tape,
}

/// The icon of a label and of the box by the mouse, drawn in a coloured circle
/// (`crate::air_draw::icon_picture`). It is drawn and not written, as the fonts of a system
/// may not have the signs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Tick,
    Snowflake,
    Bang,
    Triangle,
    Cross,
    Cloud,
    Question,
    Arrow,
}

impl Icon {
    /// The sign it stands for, as the mockup wrote it.
    pub fn glyph(self) -> &'static str {
        match self {
            Icon::Tick => "✓",
            Icon::Snowflake => "❄",
            Icon::Bang => "!",
            Icon::Triangle => "▲",
            Icon::Cross => "✕",
            Icon::Cloud => "☁",
            Icon::Question => "?",
            Icon::Arrow => "⇢",
        }
    }
}

/// How the overlay draws a kind of tile: its colour, how strongly it covers the floor, its
/// mark, and the icon of its label and of the box by the mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub colour: [u8; 3],
    pub alpha: f32,
    pub mark: Mark,
    pub icon: Icon,
}

impl Look {
    /// The ink of its mark: dark on yellow, light on the rest.
    pub fn ink(&self) -> [u8; 4] {
        if self.colour == YELLOW { [40, 40, 20, 191] } else { [255, 255, 255, 204] }
    }

    /// The colour of its icon's circle: its own, but pale grey for vacuum, whose near-black
    /// would not show.
    pub fn badge(&self) -> [u8; 3] {
        if self.colour == NEAR_BLACK { [187, 187, 187] } else { self.colour }
    }
}

/// A floor with no air, which turns to vacuum in the game: caution tape.
pub const NO_AIR: Look = Look { colour: YELLOW, alpha: 0.9, mark: Mark::Tape, icon: Icon::Question };

/// What the Breathable view makes of a mix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Breath {
    /// Station air: oxygen and nitrogen near one atmosphere at a temperature a body bears.
    Normal,
    /// The same, below freezing.
    Cold,
    /// Breathable gases, too thin to breathe.
    Thin,
    HighPressure,
    Vacuum,
    /// Anything else: plasma, nitrogen alone, too little oxygen, freezer air, too hot.
    Other,
}

impl Breath {
    pub const ALL: [Breath; 6] = [Breath::Normal, Breath::Cold, Breath::Thin, Breath::HighPressure, Breath::Vacuum, Breath::Other];

    /// How the Breathable view draws it. Normal air is a light tint, so that the problems
    /// carry the strong colours.
    pub fn look(self) -> Look {
        match self {
            Breath::Normal => Look { colour: SKY, alpha: 0.42, mark: Mark::None, icon: Icon::Tick },
            Breath::Cold => Look { colour: BLUE, alpha: 0.75, mark: Mark::Snowflake, icon: Icon::Snowflake },
            Breath::Thin => Look { colour: YELLOW, alpha: 0.7, mark: Mark::Dots, icon: Icon::Bang },
            Breath::HighPressure => Look { colour: ORANGE, alpha: 0.75, mark: Mark::Bars, icon: Icon::Triangle },
            Breath::Vacuum => Look { colour: NEAR_BLACK, alpha: 0.85, mark: Mark::Stars, icon: Icon::Cross },
            Breath::Other => Look { colour: PURPLE, alpha: 0.75, mark: Mark::Waves, icon: Icon::Cloud },
        }
    }

    pub fn colour(self) -> [u8; 3] {
        self.look().colour
    }

    pub fn label(self) -> &'static str {
        match self {
            Breath::Normal => "Normal air",
            Breath::Cold => "Cold air",
            Breath::Thin => "Too thin",
            Breath::HighPressure => "Too much pressure",
            Breath::Vacuum => "Vacuum",
            Breath::Other => "Other mix",
        }
    }

    /// What the legend says it means, with the game's numbers.
    pub fn meaning(self) -> String {
        match self {
            Breath::Normal => format!("Oxygen and nitrogen, {THIN_PRESSURE:.0} to {WARNING_HIGH_PRESSURE:.0} kPa, 0 to {:.0} °C", HEAT_DAMAGE - T0C),
            Breath::Cold => format!("Breathable, but below 0 °C and above {:.0} °C", FREEZER - T0C),
            Breath::Thin => format!("Breathable gases at {HAZARD_LOW_PRESSURE:.0} to {THIN_PRESSURE:.0} kPa"),
            Breath::HighPressure => format!("{WARNING_HIGH_PRESSURE:.0} kPa or more"),
            Breath::Vacuum => format!("{HAZARD_LOW_PRESSURE:.0} kPa or less"),
            Breath::Other => "Plasma, nitrogen only, too little oxygen, freezer air, or too hot".to_owned(),
        }
    }
}

/// The Breathable view's verdict on a mix. Pressure first, then what the mix is made of, then
/// how warm it is. The thresholds are the game's, above.
pub fn breathable(mix: &Mix, gases: &[String]) -> Breath {
    let pressure = mix.pressure();
    if pressure <= HAZARD_LOW_PRESSURE {
        return Breath::Vacuum;
    }
    if pressure >= WARNING_HIGH_PRESSURE {
        return Breath::HighPressure;
    }
    let oxygen = mix.share(gases, "Oxygen");
    let ordinary = oxygen + mix.share(gases, "Nitrogen") + mix.share(gases, "WaterVapor");
    if !(OXYGEN_LOW..=OXYGEN_HIGH).contains(&oxygen) || 1.0 - ordinary >= OTHER_GAS {
        return Breath::Other;
    }
    if mix.temperature <= FREEZER || mix.temperature >= HEAT_DAMAGE {
        return Breath::Other;
    }
    if pressure < THIN_PRESSURE {
        return Breath::Thin;
    }
    if mix.temperature < T0C {
        return Breath::Cold;
    }
    Breath::Normal
}

/// A colour scale: the values of its ticks, in order, with their labels, and the band a body
/// bears, boxed on the legend. The ticks are spaced evenly along it.
#[derive(Debug, PartialEq)]
pub struct Scale {
    pub unit: &'static str,
    pub stops: &'static [(f32, &'static str)],
    pub safe: (f32, f32),
}

/// Pressure in kPa: from vacuum through thin, normal and the warning to the damage. Safe is
/// normal air's pressure.
pub const PRESSURE_SCALE: Scale = Scale {
    unit: "kPa",
    stops: &[(0.0, "0"), (HAZARD_LOW_PRESSURE, "20"), (THIN_PRESSURE, "85"), (ONE_ATMOSPHERE, "101"), (WARNING_HIGH_PRESSURE, "385"), (HAZARD_HIGH_PRESSURE, "550+")],
    safe: (THIN_PRESSURE, WARNING_HIGH_PRESSURE),
};

/// Temperature in °C: from space through freezer air, freezing, the station's 20 °C and the
/// heat a body takes damage at, to the station alarms' danger. Safe is normal air's temperature.
pub const TEMPERATURE_SCALE: Scale = Scale {
    unit: "°C",
    stops: &[(SPACE_TEMPERATURE - T0C, "-270"), (FREEZER - T0C, "-38"), (0.0, "0"), (20.0, "20"), (HEAT_DAMAGE - T0C, "52"), (ALARM_HOT - T0C, "120+")],
    safe: (0.0, HEAT_DAMAGE - T0C),
};

/// A scale like cividis: dark blue through grey to bright yellow, its brightness rising
/// evenly, so it reads the same for every kind of colour vision.
pub const CIVIDIS: [[u8; 3]; 7] = [[0, 32, 77], [33, 60, 110], [87, 93, 109], [124, 123, 120], [166, 157, 117], [211, 193, 100], [253, 231, 55]];

/// The colour of a place along [`CIVIDIS`], from 0 to 1.
pub fn cividis(place: f32) -> [u8; 3] {
    let place = if place.is_nan() { 0.0 } else { place.clamp(0.0, 1.0) } * (CIVIDIS.len() - 1) as f32;
    let index = (place.floor() as usize).min(CIVIDIS.len() - 2);
    let (from, to, t) = (CIVIDIS[index], CIVIDIS[index + 1], place - index as f32);
    std::array::from_fn(|channel| (from[channel] as f32 + (to[channel] as f32 - from[channel] as f32) * t).round() as u8)
}

impl Scale {
    /// Where a value is along the scale, from 0 to 1: at its tick, between two ticks in step,
    /// and at the end beyond either end.
    pub fn place(&self, value: f32) -> f32 {
        let last = (self.stops.len() - 1) as f32;
        if value.is_nan() || value <= self.stops[0].0 {
            return 0.0;
        }
        for (index, pair) in self.stops.windows(2).enumerate() {
            let ((low, _), (high, _)) = (pair[0], pair[1]);
            if value <= high {
                return (index as f32 + (value - low) / (high - low)) / last;
            }
        }
        1.0
    }

    pub fn colour(&self, value: f32) -> [u8; 3] {
        cividis(self.place(value))
    }
}

/// How strongly the scales cover the floor.
pub const SCALE_ALPHA: f32 = 0.75;

/// How a view draws a mix.
pub fn look(view: AirView, mix: &Mix, gases: &[String]) -> Look {
    let scaled = |colour| Look { colour, alpha: SCALE_ALPHA, mark: Mark::None, icon: breathable(mix, gases).look().icon };
    match view {
        AirView::Pressure => scaled(PRESSURE_SCALE.colour(mix.pressure())),
        AirView::Temperature => scaled(TEMPERATURE_SCALE.colour(mix.celsius())),
        // Rooms are not coloured by their air.
        AirView::Breathable | AirView::Rooms => breathable(mix, gases).look(),
    }
}

/// The colour a view gives a mix.
pub fn colour(view: AirView, mix: &Mix, gases: &[String]) -> [u8; 3] {
    look(view, mix, gases).colour
}

/// The little tile a legend's row shows, drawn as the map draws it (`crate::air_draw::sample`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Sample {
    /// The floor as it is.
    #[default]
    Floor,
    Breath(Breath),
    /// A floor with no air: caution tape.
    NoAir,
    Wall,
    /// The outside: dark, with stars.
    Outside,
    /// A room in one of the soft colours.
    Room(usize),
    /// A tile of a room open to the outside: the orange dashes and an arrow out.
    Leak,
}

/// A row of the legend: a sample tile, a short name and what it means.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LegendRow {
    pub sample: Sample,
    pub label: String,
    pub meaning: String,
}

/// What the legend of a view shows: rows for the Breathable and Rooms views, a scale for the
/// others with a line on how to read it, and the kinds of tile the view leaves uncoloured.
#[derive(Clone, Debug, PartialEq)]
pub struct Legend {
    pub title: String,
    pub rows: Vec<LegendRow>,
    pub scale: Option<&'static Scale>,
    pub note: Option<String>,
    pub special: Vec<LegendRow>,
}

/// The legend of a view. The Rooms view's counts its rooms in the mirror, if there is one.
pub fn legend(view: AirView, mirror: Option<&AirMirror>) -> Legend {
    let map_has_air = mirror.is_some_and(|mirror| mirror.map_has_air);
    if view == AirView::Rooms {
        return rooms_legend(mirror.map(room_counts).unwrap_or_default(), map_has_air);
    }
    let rows = match view {
        AirView::Breathable => Breath::ALL.iter().map(|&breath| LegendRow { sample: Sample::Breath(breath), label: breath.label().to_owned(), meaning: breath.meaning() }).collect(),
        _ => Vec::new(),
    };
    let (title, scale, note) = match view {
        AirView::Pressure => (format!("Air: pressure ({})", PRESSURE_SCALE.unit), Some(&PRESSURE_SCALE), Some("Darker means less air, brighter means more. The box marks normal air pressure".to_owned())),
        AirView::Temperature => (format!("Air: temperature ({})", TEMPERATURE_SCALE.unit), Some(&TEMPERATURE_SCALE), Some("Darker is colder, brighter is hotter. The box marks normal air temperature".to_owned())),
        AirView::Breathable | AirView::Rooms => ("Air: breathable".to_owned(), None, None),
    };
    let mut special = vec![
        LegendRow {
            sample: Sample::NoAir,
            label: "Floor with no air".to_owned(),
            meaning: "Stores none, so the game turns it to vacuum. Under an open door it shows the air the game gives it from its neighbours".to_owned(),
        },
        LegendRow { sample: Sample::Wall, label: "Wall".to_owned(), meaning: "Blocks all sides and holds no air".to_owned() },
    ];
    if view == AirView::Breathable {
        special.push(leak_row(map_has_air));
    }
    Legend { title, rows, scale, note, special }
}

/// The legend's row of a room open to the outside.
fn leak_row(map_has_air: bool) -> LegendRow {
    let outside = outside_name(map_has_air);
    LegendRow { sample: Sample::Leak, label: format!("Leaking to {outside}"), meaning: format!("Room open to {outside}. The arrows point out through each gap") }
}

/// The Rooms view's legend, with how many rooms are sealed and how many open.
fn rooms_legend((sealed, open): (usize, usize), map_has_air: bool) -> Legend {
    let rooms = |count: usize| if count == 1 { "1 room".to_owned() } else { format!("{count} rooms") };
    let leaking = leak_row(map_has_air);
    let rows = vec![
        LegendRow { sample: Sample::Room(0), label: format!("Sealed room ({})", rooms(sealed)), meaning: "Its air stays in. Each room gets its own colour".to_owned() },
        LegendRow { label: format!("{} ({})", leaking.label, rooms(open)), ..leaking },
    ];
    let special = vec![
        LegendRow { sample: Sample::Outside, label: "Outside".to_owned(), meaning: "Open along most of its edge, like a walkway on the hull or the ground outside".to_owned() },
        LegendRow { sample: Sample::Wall, label: "Wall or closed door".to_owned(), meaning: "Blocks all sides, so it belongs to no room".to_owned() },
    ];
    Legend { title: "Air: rooms".to_owned(), rows, scale: None, note: None, special }
}

/// A gas's name as people write it: "WaterVapor" is "Water vapor".
pub(crate) fn gas_name(name: &str) -> String {
    let mut spoken = String::new();
    for (index, letter) in name.chars().enumerate() {
        let next_lower = name.chars().nth(index + 1).is_some_and(char::is_lowercase);
        let previous_lower = index > 0 && name.chars().nth(index - 1).is_some_and(char::is_lowercase);
        if index > 0 && letter.is_uppercase() && (previous_lower || next_lower) {
            spoken.push(' ');
            spoken.extend(letter.to_lowercase());
        } else {
            spoken.push(letter);
        }
    }
    spoken
}

/// The icon that leads the box by the mouse over a tile, with the colour of its circle: its
/// kind's. None over a wall and a tile with nothing to tell.
pub fn hover_icon(air: TileAir<'_>, gases: &[String]) -> Option<(Icon, [u8; 3])> {
    let look = match air {
        TileAir::Stored(mix) | TileAir::Filled(mix) => breathable(mix, gases).look(),
        TileAir::Unfilled => NO_AIR,
        TileAir::Wall | TileAir::Nothing => return None,
    };
    Some((look.icon, look.badge()))
}

/// The box shown by the mouse over a tile: a title, then pressure and temperature, then each
/// gas with its share, most first. None over a tile that has nothing to tell.
pub fn hover_text(air: TileAir<'_>, gases: &[String]) -> Option<Vec<String>> {
    let (mix, filled) = match air {
        TileAir::Stored(mix) => (mix, false),
        TileAir::Filled(mix) => (mix, true),
        TileAir::Wall => return Some(vec!["Wall".to_owned(), "Blocks all sides, so it holds no air".to_owned()]),
        TileAir::Unfilled => return Some(vec!["Floor with no air".to_owned(), "The game turns it to vacuum".to_owned()]),
        TileAir::Nothing => return None,
    };
    let title = if mix.immutable { format!("Map atmosphere: {}", breathable(mix, gases).label().to_lowercase()) } else { breathable(mix, gases).label().to_owned() };
    let mut lines = vec![title];
    if filled {
        lines.push("Stores no air. The game gives it its neighbours' air".to_owned());
    }
    lines.push(format!("{:.1} kPa, {:.1} °C", mix.pressure(), mix.celsius()));
    let total = mix.total_moles();
    let mut present: Vec<(f32, &str)> = mix.moles.iter().zip(gases).filter(|(moles, _)| **moles > 0.0).map(|(moles, gas)| (moles / total, gas.as_str())).collect();
    present.sort_by(|a, b| b.0.total_cmp(&a.0));
    if present.is_empty() {
        lines.push("No gas".to_owned());
    }
    for (share, gas) in present {
        let percent = share * 100.0;
        let shown = if percent < 0.1 { "<0.1".to_owned() } else { format!("{percent:.1}") };
        lines.push(format!("{} {shown}%", gas_name(gas)));
    }
    Some(lines)
}

/// The leaks of every room open to the outside, the outside itself left out, as each grid's
/// cells, for their outlines.
pub fn leaks(mirror: &AirMirror) -> impl Iterator<Item = (i32, (i32, i32))> + '_ {
    mirror.grids.iter().flat_map(|(&grid, air)| air.rooms.open().filter(|(_, room)| !room.is_outside()).flat_map(move |(_, room)| room.leaks.iter().map(move |&cell| (grid, cell))))
}

/// Which grid each entity stands on, kept from the scene and each change, so that the grids a
/// change touched can be told, those of entities it removed too.
#[derive(Debug, Default)]
pub struct GridsOf {
    parents: HashMap<i32, i32>,
    grids: HashSet<i32>,
}

impl GridsOf {
    pub fn new(scene: &SceneJson) -> GridsOf {
        GridsOf { parents: scene.entities.iter().map(|entity| (entity.uid, entity.parent)).collect(), grids: scene.grids.iter().map(|grid| grid.uid).collect() }
    }

    /// The grid under an entity, through whatever holds it.
    fn grid_of(&self, mut uid: i32) -> Option<i32> {
        for _ in 0..64 {
            if self.grids.contains(&uid) {
                return Some(uid);
            }
            uid = *self.parents.get(&uid)?;
        }
        None
    }

    /// The grids a change touched, in order: those whose tiles changed, those changed or
    /// removed themselves, and those an entity it named stood on before or stands on now.
    /// Then takes the change in.
    pub fn touched(&mut self, delta: &Delta) -> Vec<i32> {
        let mut touched: HashSet<i32> = delta.tiles.iter().map(|tile| tile.grid).collect();
        touched.extend(delta.grids_changed.iter().map(|grid| grid.uid));
        // Those whose stored air the engine says it changed, as a fix of the air.
        touched.extend(delta.air.iter().copied());
        // New component data of a grid, or of a thing on one, as a door opened in its data.
        touched.extend(delta.details.iter().filter_map(|details| self.grid_of(details.uid)));
        let named = delta.entities.iter().map(|entity| entity.uid).chain(delta.removed.iter().copied());
        touched.extend(named.filter_map(|uid| self.grid_of(uid)));

        self.grids.extend(delta.grids_changed.iter().map(|grid| grid.uid));
        for uid in &delta.removed {
            self.parents.remove(uid);
            self.grids.remove(uid);
        }
        for entity in &delta.entities {
            self.parents.insert(entity.uid, entity.parent);
        }
        touched.extend(delta.entities.iter().filter_map(|entity| self.grid_of(entity.uid)));

        let mut touched: Vec<i32> = touched.into_iter().collect();
        touched.sort_unstable();
        touched
    }
}

#[cfg(test)]
#[path = "tests/air.rs"]
mod tests;
