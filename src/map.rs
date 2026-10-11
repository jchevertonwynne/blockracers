//! The maps: the circuits and arenas of the port's own, each a file that is read as
//! the game runs, so that one is changed, or another made, without building the game
//! again. The port's own.
//!
//! A map is a TOML file named for what it is known by: `brick.toml` is `BRICK` to
//! `$BRICK_RACE`, to the ghosts kept of it and to the other games of a session. It
//! has a `name`, may say its `author`, and has either a road, which makes it a circuit (`corners`, and what
//! `track::Road` has besides), or a floor, which makes it an arena (`outline`, and
//! the rest of `arena::Plan`); and it may have `hazards` of the game's stood about
//! it (`gauntlet::Stood`), `scenery` (`landmarks::Landmark`) and, a circuit, the
//! look of its `walls` (`roadside::Look`). The files of `maps/` say what each of
//! these is as they give it. Distances are the game's own units as the port has
//! them, with x east, z south and heights up.
//!
//! The game carries the maps of `maps/` (`stock`), and they are all a demo or a
//! test has unless `$BRICK_MAPS` names a folder. A game being played keeps its
//! maps in that folder, or in `maps` of `home`'s: every file there is a map, one
//! named as one of the game's own is raced in its place, and the game's own are
//! written there to be changed (`stock_up`). A map that can't be read, or that
//! the road can't be built from, is said in the log and left out.
//!
//! Racing online the host's map is the one raced: it goes to the players with the
//! start, and is theirs until they race alone again (`menu::Circuits::lend`).

use crate::arena::{Block, Deck, Plan, Ramp};
use crate::gauntlet::Stood;
use crate::landmarks::Landmark;
use crate::meshgen::*;
use crate::roadside::Look;
use crate::track::{Banked, Byway, Jump, Padded, Road, Stretch, Track, Tunnel};
use bevy::prelude::*;
use serde::de::Error;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// What a map's file is called after its name.
const EXTENSION: &str = "toml";
/// The file, in a folder of maps, of what the game has written there (`stock_up`).
const WRITTEN: &str = ".written";
/// The longest a map's file may be, in bytes: it is sent whole to a session's
/// players. And the longest its file's name, without what comes after the dot.
const LONGEST: usize = 48 * 1024;
const LONGEST_KEY: usize = 24;
/// The fewest corners a road is made of and the most, the shortest lap there is
/// room on for the grid, the gates and the lap zones, and the longest.
const CORNERS: (usize, usize) = (3, 400);
const LAP: (f32, f32) = (250.0, 8000.0);
/// How far from the middle of the map anything may be, and how high.
const REACH: f32 = 3000.0;
const HIGHEST: f32 = 200.0;
/// The most of any one kind of thing a map may have.
const MOST: usize = 400;

/// The maps the game carries, in the order the menus list them: the circuits, then
/// the arenas.
const STOCK: [(&str, &str); 8] = [
    ("BRICK", include_str!("../maps/brick.toml")),
    ("FIGURE8", include_str!("../maps/figure8.toml")),
    ("GAUNTLET", include_str!("../maps/gauntlet.toml")),
    ("HELTER", include_str!("../maps/helter.toml")),
    ("YARD", include_str!("../maps/yard.toml")),
    ("CROSSROADS", include_str!("../maps/crossroads.toml")),
    ("RING", include_str!("../maps/ring.toml")),
    ("TOWN", include_str!("../maps/town.toml")),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    /// What it is known by: its file's name, in capitals.
    pub key: String,
    pub name: String,
    /// Who made it, if the file says (`author`).
    pub author: String,
    /// The file as it was read, which is what a session's players are sent.
    pub text: String,
    /// The file itself, if it is one of a folder's.
    pub file: Option<PathBuf>,
    /// A circuit's road; an arena has none.
    pub road: Road,
    /// How an arena is laid out; a circuit isn't.
    pub plan: Option<Plan>,
    pub hazards: Vec<Stood>,
    pub look: Look,
    pub scenery: Vec<Landmark>,
}

/// A map's file, as it is written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sheet {
    name: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    corners: Vec<[f32; 4]>,
    #[serde(default)]
    tunnels: Vec<Tunnel>,
    #[serde(default)]
    jumps: Vec<Jump>,
    #[serde(default)]
    byways: Vec<Byway>,
    #[serde(default)]
    infields: Vec<Stretch>,
    #[serde(default)]
    pads: Vec<Padded>,
    #[serde(default)]
    banks: Vec<Banked>,
    walls: Option<Look>,
    #[serde(default)]
    scenery: Vec<Landmark>,
    #[serde(default)]
    outline: Vec<[f32; 2]>,
    floor: Option<[Tint; 2]>,
    #[serde(default)]
    blocks: Vec<Block>,
    #[serde(default)]
    ramps: Vec<Ramp>,
    #[serde(default)]
    decks: Vec<Deck>,
    #[serde(default)]
    line: Vec<[f32; 3]>,
    grid: Option<[[f32; 2]; 6]>,
    #[serde(default)]
    bricks: Vec<[f32; 2]>,
    #[serde(default)]
    hazards: Vec<Stood>,
}

/// The colours a map may name.
const COLOURS: [(&str, Color); 12] = [
    ("red", RED),
    ("blue", BLUE),
    ("yellow", YELLOW),
    ("green", GREEN),
    ("lime", LIME),
    ("white", WHITE),
    ("black", BLACK),
    ("grey", GREY),
    ("dark grey", DARK_GREY),
    ("orange", ORANGE),
    ("brown", BROWN),
    ("tan", TAN),
];

/// A colour as a map gives it: one of `COLOURS` by name, or its red, green and
/// blue, each from nought to one.
pub fn colour<'de, D: Deserializer<'de>>(from: D) -> Result<Color, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Paint {
        Named(String),
        Mixed([f32; 3]),
    }
    match Paint::deserialize(from) {
        Ok(Paint::Named(name)) => COLOURS
            .iter()
            .find(|colour| colour.0 == name)
            .map(|colour| colour.1)
            .ok_or_else(|| {
                let known: Vec<&str> = COLOURS.iter().map(|colour| colour.0).collect();
                D::Error::custom(format!("no colour is called {name}: there are {}", known.join(", ")))
            }),
        Ok(Paint::Mixed([red, green, blue])) => Ok(Color::srgb(red, green, blue)),
        Err(_) => Err(D::Error::custom(
            "a colour is its name, or its [red, green, blue] from 0 to 1",
        )),
    }
}

#[derive(Deserialize)]
struct Tint(#[serde(deserialize_with = "colour")] Color);

impl Map {
    pub fn arena(&self) -> bool {
        self.plan.is_some()
    }

    /// The map a file's text is, if it is one a road or a floor can be made of:
    /// otherwise, what is wrong with it.
    pub fn read(key: &str, text: &str) -> Result<Map, String> {
        if text.len() > LONGEST {
            return Err(format!("it is {} bytes, and a map is {LONGEST} at the most", text.len()));
        }
        let sheet: Sheet = toml::from_str(text).map_err(|error| error.to_string())?;
        let circuit = !sheet.corners.is_empty();
        if circuit == !sheet.outline.is_empty() {
            return Err("a map has `corners`, which makes it a circuit, or an `outline`, \
                        which makes it an arena: one of them"
                .into());
        }
        let floored = sheet.grid.is_some()
            || sheet.floor.is_some()
            || !(sheet.blocks.is_empty() && sheet.ramps.is_empty() && sheet.decks.is_empty())
            || !(sheet.line.is_empty() && sheet.bricks.is_empty());
        if circuit && floored {
            return Err("a circuit has no floor, blocks, grid or line of an arena's".into());
        }
        let plan = match circuit {
            true => None,
            false => Some(Plan {
                outline: sheet.outline,
                blocks: sheet.blocks,
                ramps: sheet.ramps,
                decks: sheet.decks,
                line: sheet.line,
                grid: sheet.grid.ok_or("an arena wants a `grid`: the six places its cars start")?,
                bricks: sheet.bricks,
                floor: sheet.floor.map_or([GREY, DARK_GREY], |floor| floor.map(|tint| tint.0)),
            }),
        };
        let road = Road {
            corners: sheet.corners,
            tunnels: sheet.tunnels,
            jumps: sheet.jumps,
            byways: sheet.byways,
            infields: sheet.infields,
            pads: sheet.pads,
            banks: sheet.banks,
        };
        if !circuit && road != Road::default() {
            return Err("an arena has no road for tunnels, jumps, byways, infields, pads or banks".into());
        }
        let map = Map {
            key: key.to_string(),
            name: sheet.name.trim().to_string(),
            author: sheet.author.trim().to_string(),
            text: text.to_string(),
            file: None,
            road,
            plan,
            hazards: sheet.hazards,
            look: sheet.walls.unwrap_or_default(),
            scenery: sheet.scenery,
        };
        match map.amiss() {
            Some(wrong) => Err(wrong),
            None => Ok(map),
        }
    }

    /// As `read`, for a map that isn't the game's own: one the game can't build
    /// everything of is refused too, whatever the reason.
    pub fn tried(key: &str, text: &str) -> Result<Map, String> {
        if !keyed(key) {
            return Err("it is not named as a map is".into());
        }
        let map = Map::read(key, text)?;
        let built = std::panic::catch_unwind(|| {
            for mirror in [false, true] {
                let mut track = Track::made(&map, mirror);
                crate::roadside::fences(&track);
                crate::items::laid(&track);
                track.reverse();
                crate::items::laid(&track);
            }
        });
        match built {
            Ok(()) => Ok(map),
            Err(_) => Err("the game could not build it".into()),
        }
    }

    /// What is wrong with it, if anything is that would stop it being built.
    fn amiss(&self) -> Option<String> {
        if self.name.is_empty() || self.name.len() > lobby_api::MAX_NAME {
            return Some(format!("its name is from 1 to {} letters long", lobby_api::MAX_NAME));
        }
        if self.author.len() > lobby_api::MAX_NAME {
            return Some(format!("its author is {} letters long at the most", lobby_api::MAX_NAME));
        }
        let lap = match &self.plan {
            Some(plan) => {
                if let Some(wrong) = floor_amiss(plan) {
                    return Some(wrong);
                }
                None
            }
            None => match self.road_amiss() {
                Ok(lap) => Some(lap),
                Err(wrong) => return Some(wrong),
            },
        };
        for (at, stood) in self.hazards.iter().enumerate() {
            if self.hazards[..at].iter().any(|other| other.kind == stood.kind) {
                let name = format!("{:?}", stood.kind).to_lowercase();
                return Some(format!("it has more than one {name}, and there is only one"));
            }
            if let Some(wrong) = stood.amiss(lap) {
                return Some(wrong);
            }
        }
        let places = self.hazards.iter().filter_map(|stood| stood.place);
        if places.chain(self.scenery.iter().map(|landmark| landmark.place)).any(|place| !near(&place)) {
            return Some("something of it stands too far off".into());
        }
        (self.hazards.len() > MOST || self.scenery.len() > MOST).then(|| "it has too much".into())
    }

    /// How long a circuit's lap is, or what is wrong with its road.
    fn road_amiss(&self) -> Result<f32, String> {
        let road = &self.road;
        let corners = &road.corners;
        if corners.len() < CORNERS.0 || corners.len() > CORNERS.1 {
            return Err(format!("a road has from {} to {} corners", CORNERS.0, CORNERS.1));
        }
        let m = corners.len();
        let at = |i: usize| Vec2::new(corners[i % m][0], corners[i % m][2]);
        // How far along each of its two straights a corner's rounding reaches.
        let reach = |i: usize| {
            let (into, out) = (at(i) - at(i + m - 1), at(i + 1) - at(i));
            corners[i % m][3] * (into.angle_to(out).abs() / 2.0).tan()
        };
        for (i, corner) in corners.iter().enumerate() {
            let [x, height, z, radius] = *corner;
            let number = i + 1;
            if !near(&[x, z]) || !(0.0..=HIGHEST).contains(&height) || !(0.0..=REACH).contains(&radius) {
                return Err(format!("corner {number} is out of range: {corner:?}"));
            }
            let apart = at(i).distance(at(i + 1));
            if apart < 1.0 {
                return Err(format!("corner {number} and the next are in the same place"));
            }
            if at(i + m - 1).distance(at(i + 1)) < 1.0 && radius > 0.0 {
                return Err(format!("corner {number} is rounded, and turns straight back"));
            }
            if reach(i) + reach(i + 1) > apart + 0.01 {
                return Err(format!(
                    "corner {number} and the next are rounded to more than the {apart} between them"
                ));
            }
        }
        let counts = [
            road.tunnels.len(),
            road.jumps.len(),
            road.byways.len(),
            road.infields.len(),
            road.pads.len(),
            road.banks.len(),
        ];
        if counts.iter().any(|&count| count > MOST) {
            return Err("it has too much".into());
        }
        let line = Track::line(self, false);
        let (lap, n) = (line.length, line.n());
        if !(LAP.0..=LAP.1).contains(&lap) {
            return Err(format!("its lap is {lap} long, and a lap is from {} to {}", LAP.0, LAP.1));
        }
        // What is so far round the lap is on it, and begins before it ends.
        let on = |s: f32| (0.0..lap).contains(&s);
        let sample = |s: f32| (s / line.spacing).round() as usize % n;
        let stretch = |what: &str, from: f32, to: f32| {
            (on(from) && on(to) && sample(from) + 2 <= sample(to))
                .then_some(())
                .ok_or(format!("the {what} from {from} to {to} is not a stretch of a lap {lap} long"))
        };
        for byway in &road.byways {
            stretch("byway", byway.from, byway.to)?;
            let straight = line.pts[sample(byway.from)].distance(line.pts[sample(byway.to)]);
            let sized = byway.road > 0.5 && byway.wall >= byway.road && byway.wall < 40.0;
            if straight < 20.0 || !sized || !(0.0..20.0).contains(&byway.bumps) || !(0.0..=1.0).contains(&byway.taken) {
                return Err(format!("the byway from {} is too short, or its sizes are out of range", byway.from));
            }
        }
        for infield in &road.infields {
            stretch("infield", infield.from, infield.to)?;
        }
        for bank in &road.banks {
            stretch("bank", bank.from, bank.to)?;
            if bank.tilt.abs() > 1.0 {
                return Err(format!("the bank from {} is too steep", bank.from));
            }
        }
        if let Some(pad) = road.pads.iter().find(|pad| !on(pad.at) || pad.right.abs() > crate::track::ROAD_HW) {
            return Err(format!("the pad at {} is not on the road", pad.at));
        }
        let nearest = |[x, z]: [f32; 2]| line.nearest(Vec3::new(x, 0.0, z));
        for tunnel in &road.tunnels {
            if !near(&tunnel.from) || !near(&tunnel.to) || nearest(tunnel.from) >= nearest(tunnel.to) {
                return Err(format!(
                    "the tunnel from {:?} ends before it begins, round the lap",
                    tunnel.from
                ));
            }
        }
        // A jump's ramp and landing are cut into the road either side of its lip.
        let room = (crate::track::JUMP_LENGTH / line.spacing) as usize + 8;
        for jump in &road.jumps {
            let lip = if near(&jump.lip) { nearest(jump.lip) } else { 0 };
            if lip < room || lip + room >= n {
                return Err(format!("the jump at {:?} is too near the start", jump.lip));
            }
        }
        Ok(lap)
    }
}

/// Whether a place is no further from the middle of the map than anything may be.
fn near(place: &[f32; 2]) -> bool {
    place.iter().all(|v| v.abs() <= REACH)
}

/// What is wrong with an arena's plan, if anything is.
fn floor_amiss(plan: &Plan) -> Option<String> {
    let counts = [
        plan.outline.len(),
        plan.blocks.len(),
        plan.ramps.len(),
        plan.decks.len(),
        plan.line.len(),
        plan.bricks.len(),
    ];
    if counts.iter().any(|&count| count > MOST) {
        return Some("it has too much".into());
    }
    // The floor is laid from its middle out to each side of it in turn.
    let corner = |i: usize| Vec2::from(plan.outline[i % plan.outline.len()]);
    let turns: Vec<f32> = (0..plan.outline.len())
        .map(|i| corner(i).perp_dot(corner(i + 1)))
        .collect();
    let round = turns.iter().all(|&turn| turn > 1.0) || turns.iter().all(|&turn| turn < -1.0);
    if plan.outline.len() < 3 || !round || !plan.outline.iter().all(near) {
        return Some(
            "the corners of its `outline` go round the middle, which is [0, 0], and each is \
             seen from there"
                .into(),
        );
    }
    let sized = |half: &[f32; 2], height: f32| {
        half.iter().all(|&half| (0.1..=REACH).contains(&half)) && (0.1..=HIGHEST).contains(&height)
    };
    let stands = plan
        .blocks
        .iter()
        .map(|block| (&block.at, &block.half, block.height))
        .chain(plan.ramps.iter().map(|ramp| (&ramp.at, &ramp.half, ramp.height)))
        .chain(plan.decks.iter().map(|deck| (&deck.at, &deck.half, deck.height)));
    for (at, half, height) in stands {
        if !near(at) || !sized(half, height) {
            return Some(format!("what stands at {at:?} is out of range"));
        }
    }
    let line: Vec<Vec3> = plan.line.iter().map(|&[x, y, z]| Vec3::new(x, y, z)).collect();
    let apart = |i: usize| line[i].distance(line[(i + 1) % line.len()]);
    let ranged = plan.line.iter().all(|&[x, y, z]| near(&[x, z]) && (0.0..=HIGHEST).contains(&y));
    if line.len() < 4 || !ranged || (0..line.len()).any(|i| apart(i) < 1.0) {
        return Some("its `line` is four places or more, each apart from the next".into());
    }
    if !plan.grid.iter().chain(&plan.bricks).all(near) {
        return Some("its grid or its bricks are out of range".into());
    }
    None
}

/// The maps the game carries.
pub fn stock() -> &'static [Map] {
    static MAPS: LazyLock<Vec<Map>> = LazyLock::new(|| {
        STOCK
            .iter()
            .map(|(key, text)| match Map::read(key, text) {
                Ok(map) => map,
                Err(wrong) => panic!("the game's own map {key}: {wrong}"),
            })
            .collect()
    });
    &MAPS
}

/// The circuits of those, and the arenas.
#[cfg(test)]
pub fn circuits() -> impl Iterator<Item = &'static Map> {
    stock().iter().filter(|map| !map.arena())
}

#[cfg(test)]
pub fn arenas() -> impl Iterator<Item = &'static Map> {
    stock().iter().filter(|map| map.arena())
}

/// One of them by its key.
#[cfg(test)]
pub fn named(key: &str) -> &'static Map {
    stock().iter().find(|map| map.key == key).expect("a map of the game's own")
}

/// The folder a game's maps are in: `$BRICK_MAPS`, or `maps` in `home`'s folder.
/// A demo has none unless it names one.
fn folder(demo: bool) -> Option<PathBuf> {
    let named = std::env::var_os("BRICK_MAPS").map(PathBuf::from);
    named.or_else(|| crate::home::kept("BRICK_MAPS", "maps").filter(|_| !demo))
}

/// The maps a game has: its own, each as its folder has it if it has it, and
/// whatever else is in the folder, the circuits before the arenas. A game that
/// keeps things (`keeping`) keeps its own maps in the folder too.
pub fn kept(demo: bool, keeping: bool) -> Vec<Map> {
    let mut maps = stock().to_vec();
    let Some(folder) = folder(demo) else {
        return maps;
    };
    if keeping {
        stock_up(&folder, &STOCK);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|file| file.extension().is_some_and(|extension| extension == EXTENSION))
        .collect();
    files.sort();
    for file in files {
        let read = key_of(&file).and_then(|key| {
            let text = std::fs::read_to_string(&file).map_err(|error| error.to_string())?;
            // The game's own, as the game has it, needs no trying.
            match maps.iter().find(|map| map.key == key && map.text == text) {
                Some(own) => Ok(own.clone()),
                None => Map::tried(&key, &text),
            }
        });
        match read {
            Ok(mut map) => {
                map.file = Some(file);
                match maps.iter_mut().find(|own| own.key == map.key) {
                    Some(own) => *own = map,
                    None => maps.push(map),
                }
            }
            Err(wrong) => crate::home::say(&format!("{} is left out: {wrong}", file.display())),
        }
    }
    // The order they were found in, but the circuits first.
    maps.sort_by_key(Map::arena);
    maps
}

/// Whether something is what a map may be known by.
pub fn keyed(key: &str) -> bool {
    key_of(Path::new(key)).as_deref() == Ok(key)
}

/// What a map's file makes it known by: its name in capitals.
fn key_of(file: &Path) -> Result<String, String> {
    let stem = file.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
    let plain = stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if stem.is_empty() || stem.len() > LONGEST_KEY || !plain {
        return Err(format!(
            "a map's file is named in up to {LONGEST_KEY} letters, digits, - and _"
        ));
    }
    Ok(stem.to_ascii_uppercase())
}

impl Map {
    /// The map as its file has it now, if that has changed since it was read and
    /// is still a map.
    pub fn reread(&self) -> Option<Map> {
        let file = self.file.as_ref()?;
        let text = std::fs::read_to_string(file).ok().filter(|text| *text != self.text)?;
        match Map::tried(&self.key, &text) {
            Ok(map) => Some(Map {
                file: self.file.clone(),
                ..map
            }),
            Err(wrong) => {
                warn!("{} is raced as it was: {wrong}", file.display());
                None
            }
        }
    }
}

/// A file's text as a number, to tell whether it is what was written (FNV-1a).
fn print(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Writes the game's own maps into a folder: each where the folder has none of that
/// name, and over one that is still as the game last wrote it, so that a map nobody
/// has changed keeps up with the game's. One that has been changed is left alone.
/// What was written is kept in the folder (`WRITTEN`), a line to a map.
fn stock_up(folder: &Path, stock: &[(&str, &str)]) {
    let record = folder.join(WRITTEN);
    let was = std::fs::read_to_string(&record).unwrap_or_default();
    let mut written: HashMap<String, u64> = was
        .lines()
        .filter_map(|line| line.split_once(' '))
        .filter_map(|(key, print)| Some((key.to_string(), print.parse().ok()?)))
        .collect();
    for &(key, text) in stock {
        let file = folder.join(format!("{}.{EXTENSION}", key.to_ascii_lowercase()));
        let on_file = std::fs::read_to_string(&file).ok();
        let ours = on_file
            .as_deref()
            .is_none_or(|on_file| written.get(key) == Some(&print(on_file)));
        if on_file.as_deref() == Some(text) {
            written.insert(key.to_string(), print(text));
        } else if ours {
            match crate::home::write(&file, text) {
                Ok(()) => drop(written.insert(key.to_string(), print(text))),
                Err(error) => crate::home::say(&format!("could not write {}: {error}", file.display())),
            }
        }
    }
    let mut lines: Vec<String> = written.iter().map(|(key, print)| format!("{key} {print}\n")).collect();
    lines.sort();
    let now = lines.concat();
    if now != was
        && let Err(error) = crate::home::write(&record, now)
    {
        crate::home::say(&format!("could not write {}: {error}", record.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_games_own_maps_are_four_circuits_and_four_arenas() {
        let keys: Vec<&str> = stock().iter().map(|map| map.key.as_str()).collect();
        assert_eq!(
            keys,
            ["BRICK", "FIGURE8", "GAUNTLET", "HELTER", "YARD", "CROSSROADS", "RING", "TOWN"]
        );
        assert_eq!(circuits().count(), 4);
        assert!(arenas().all(|map| map.road == Road::default() && map.hazards.len() <= 2));
        // Each is small enough to send, can be built every way, and is named.
        for (key, text) in STOCK {
            assert_eq!(Map::tried(key, text).as_ref(), Ok(named(key)));
            assert!(text.len() < LONGEST / 4, "{key}");
        }
        assert_eq!(named("FIGURE8").name, "Figure Eight");
        assert!(stock().iter().all(|map| map.author == "Brick Racers"));
    }

    /// A circuit as short as a file of one can be.
    const OVAL: &str = "name = \"Oval\"\ncorners = [[0, 0, 0, 0], [100, 0, 0, 30], [100, 0, 80, 30], [-100, 0, 80, 30], [-100, 0, 0, 30]]\n";

    #[test]
    fn a_map_of_a_few_lines_is_a_circuit() {
        let map = Map::tried("OVAL", OVAL).unwrap();
        assert!(!map.arena() && map.look == Look::default() && map.hazards.is_empty());
        assert_eq!(map.author, "");
        let signed = Map::tried("OVAL", &format!("{OVAL}author = \" Pepper Roni \"\n")).unwrap();
        assert_eq!(signed.author, "Pepper Roni");
        let track = Track::made(&map, false);
        assert!(track.length > 400.0 && track.course.zones.len() == 2);
        assert!(track.course.checkpoints.len() > 10);
        // With what a road may have, each said so far round it.
        let more = format!(
            "{OVAL}pads = [{{ at = 20 }}]\nbanks = [{{ from = 90, to = 130, tilt = 0.1 }}]\n\
             walls = {{ wall = \"red\", cap = [0.5, 0.5, 0.5] }}\n\
             hazards = [{{ kind = \"hammer\", at = 40 }}, {{ kind = \"dragon\", place = [0, 40], reach = 30 }}]\n\
             scenery = [{{ kind = \"pond\", place = [0, 40], radius = 12 }}]\n"
        );
        let map = Map::tried("OVAL", &more).unwrap();
        assert_eq!((map.road.pads.len(), map.hazards.len(), map.scenery.len()), (1, 2, 1));
        assert!(map.look != Look::default());
        assert!(Track::made(&map, false).bank.iter().any(|&bank| bank != 0.0));
    }

    #[test]
    fn what_is_wrong_with_a_map_is_said() {
        let wrong = |text: &str| Map::tried("X", text).expect_err(text);
        let with = |more: &str| wrong(&format!("{OVAL}{more}"));
        // Not a file of the kind, with where it goes wrong.
        assert!(wrong("name = \"Oval\"\ncorners = [[0, 0").contains("line 2"));
        assert!(with("cornres = []\n").contains("cornres"));
        assert!(wrong("name = \"Nothing\"\n").contains("corners"));
        assert!(with("outline = [[0, 0], [1, 1], [2, 0]]\n").contains("one of them"));
        // A road that can't be laid.
        let tight = OVAL.replace("[100, 0, 0, 30], [100, 0, 80, 30]", "[100, 0, 0, 50], [100, 0, 80, 50]");
        assert!(wrong(&tight).contains("corner 2"));
        assert!(wrong("name = \"Dot\"\ncorners = [[0, 0, 0, 0], [20, 0, 0, 5], [20, 0, 20, 5]]\n").contains("lap"));
        assert!(wrong(&OVAL.replace("[100, 0, 80, 30]", "[100, 0, 0.5, 30]")).contains("same place"));
        // Things that aren't on it, or that there is one of.
        assert!(with("pads = [{ at = 5000 }]\n").contains("pad"));
        assert!(with("banks = [{ from = 130, to = 90, tilt = 0.1 }]\n").contains("bank"));
        assert!(with("jumps = [{ lip = [10, 0] }]\n").contains("jump"));
        assert!(with("byways = [{ from = 20, to = 24 }]\n").contains("byway"));
        assert!(with("hazards = [{ kind = \"ghost\", at = 9000 }]\n").contains("ghost"));
        assert!(with("hazards = [{ kind = \"ghost\" }]\n").contains("place"));
        assert!(with("hazards = [{ kind = \"curse\", at = 90 }]\n").contains("drops"));
        assert!(with("hazards = [{ kind = \"ark\", at = 9 }, { kind = \"ark\", at = 99 }]\n").contains("one ark"));
        assert!(with("hazards = [{ kind = \"wizard\", at = 9 }]\n").contains("wizard"));
        assert!(with("walls = { wall = \"puce\" }\n").contains("puce"));
        // An arena wants a floor that goes round its middle, and its six places.
        let floor = "name = \"Pen\"\noutline = [[60, -60], [60, 60], [-60, 60], [-60, -60]]\n\
                     line = [[30, 0, -30], [30, 0, 30], [-30, 0, 30], [-30, 0, -30]]\n";
        let grid = "grid = [[40, 0], [-40, 0], [0, 40], [0, -40], [40, 40], [-40, -40]]\n";
        assert!(wrong(floor).contains("grid"));
        let pen = Map::tried("PEN", &format!("{floor}{grid}")).unwrap();
        assert!(pen.arena() && Track::made(&pen, true).course.grid.len() == 6);
        assert!(wrong(&format!("{floor}{grid}").replace("[60, 60]", "[-90, -70]")).contains("outline"));
        assert!(wrong(&format!("{floor}{grid}pads = [{{ at = 3 }}]\n")).contains("arena"));
        let stood = "hazards = [{ kind = \"ghost\", at = 30 }]\n";
        assert!(wrong(&format!("{floor}{grid}{stood}")).contains("arena"));
        let ramp = "ramps = [{ at = [0, 30], half = [5, 5], height = 4, climbs = \"up\" }]\n";
        assert!(wrong(&format!("{floor}{grid}{ramp}")).contains("north"));
    }

    #[test]
    fn a_folder_s_maps_are_raced_in_place_of_the_game_s_own() {
        let folder = std::env::temp_dir().join(format!("brick_maps_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let file = |name: &str| folder.join(name);
        let read = |name: &str| std::fs::read_to_string(file(name)).ok();
        let (first, second) = ("name = \"One\"\n", "name = \"Two\"\n");
        // Into a folder that isn't there, the game's own are written.
        stock_up(&folder, &[("BRICK", first), ("RING", first)]);
        assert_eq!(read("brick.toml").as_deref(), Some(first));
        // One that is changed is left as it is when the game's is another, and
        // one that is not keeps up; one that is gone is written again.
        std::fs::write(file("brick.toml"), "name = \"Mine\"\n").unwrap();
        stock_up(&folder, &[("BRICK", second), ("RING", second)]);
        assert_eq!(read("brick.toml").as_deref(), Some("name = \"Mine\"\n"));
        assert_eq!(read("ring.toml").as_deref(), Some(second));
        std::fs::remove_file(file("ring.toml")).unwrap();
        stock_up(&folder, &[("BRICK", second), ("RING", first)]);
        assert_eq!(read("ring.toml").as_deref(), Some(first));
        assert_eq!(read("brick.toml").as_deref(), Some("name = \"Mine\"\n"));
        // One changed to what the game's now is, is the game's again.
        std::fs::write(file("brick.toml"), second).unwrap();
        stock_up(&folder, &[("BRICK", second)]);
        stock_up(&folder, &[("BRICK", first)]);
        assert_eq!(read("brick.toml").as_deref(), Some(first));
        // Without what was written to go by, whatever is there is left.
        std::fs::remove_file(file(WRITTEN)).unwrap();
        stock_up(&folder, &[("BRICK", second)]);
        assert_eq!(read("brick.toml").as_deref(), Some(first));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_map_is_known_by_its_file_s_name() {
        assert_eq!(key_of(Path::new("/maps/my-oval_2.toml")).as_deref(), Ok("MY-OVAL_2"));
        assert!(key_of(Path::new("/maps/my oval.toml")).is_err());
        assert!(key_of(Path::new("/maps/.toml")).is_err());
    }
}
