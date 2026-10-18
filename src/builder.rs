//! The map builder: making a circuit or an arena in the game, and changing one
//! there is. The port's own.
//!
//! What is built is a map as `map` reads one, and nothing a file couldn't say: a
//! `Draft` is a `Map` being changed, written out as its file would be after every
//! change (`Map::written`) and read back (`Map::tried`), so that what is wrong
//! with it is what would be wrong with the file, and is said in the same words.
//! While something is wrong it is neither tried nor kept.
//!
//! A circuit is drawn. A new one is no more than the straight its cars start
//! from, and its road is laid on from the end of that, a corner where the
//! pointer is clicked or wherever a line drawn with it bends (`Draft::draw`,
//! `draw::Stroke`), until it is brought back to the other end of the straight
//! (`Draft::close`). Until then it is `open`: its corners are those the loop
//! will have, with no road yet from the last drawn to the last of all. Every
//! corner is rounded to what is wanted of it or to what there is room for
//! (`draw::fitted`), whatever is drawn or dragged, and the start stays on the
//! straight between the last corner and the first as those are moved.
//!
//! A map is gone through a kind of thing at a time (`Layer`): its corners, its
//! pads, its hazards and the rest, an arena's being its outline, blocks, ramps,
//! decks, line, grid and bricks. The thing picked has rows (`Draft::rows`), each a
//! number, a choice or words, which are turned up and down; and it is moved over
//! the ground (`Draft::nudge`), or to where the pointer is (`Draft::put`), by
//! whichever of its handles is held (`Draft::handles`): the ends of a stretch and
//! the sides of a block are handles of their own. What a circuit has so far round
//! its lap is kept where it is on the ground when the road is changed under it
//! (`Draft::remeasure`), which a file changed by hand wants done by hand.
//!
//! `view` is the screen all this is done on, and what is kept goes to the folder
//! of maps (`map::save`), where the game finds it like any other.

use crate::arena::{Block, Brick, Deck, Hue, Pad, Plan, Ramp};
use crate::gauntlet::{DRAGON_REACH, STONES_APART, Sort, Stood};
use crate::items::Power;
use crate::landmarks::{Kind, Landmark};
use crate::map::{COLOURS, Map, write};
use crate::meshgen::{GREY, RED};
use crate::track::{
    Banked, Byway, Jump, Padded, ROAD_HW, Road, Stretch, Track, Tunnel, WIDTH, WIDTHS,
};
use bevy::prelude::*;
use draw::ROUNDED;

pub mod draw;
pub mod panel;
pub mod pointer;
pub mod view;

/// What a map that has no file yet is known by, and what one being tried is
/// raced as (`menu::Circuits::try_out`).
const DRAFT: &str = "DRAFT";
pub const TRIAL: &str = "TRIAL-RUN";
/// How many changes can be taken back.
const UNDONE: usize = 200;
/// How far to either side of its road something that stands by it may be put.
const BESIDE: f32 = 30.0;
/// How far round the lap a step along it is.
const ALONG: f32 = 2.0;

/// How far behind the line a new circuit's straight begins and how far past it
/// the straight ends, and how much of each is kept clear of the corners' rounding:
/// behind the line for the cars to line up on, and a little past it.
const STRAIGHT: (f32, f32) = (64.0, 44.0);
const KEPT: (f32, f32) = (26.0, 8.0);
/// A corner rounded to less than this, and turning enough to matter, is one a car
/// must slow for.
const TIGHTEST: f32 = 12.0;
const NEW_ARENA: &str = "name = \"New Arena\"\n\
    outline = [[80, -80], [80, 80], [-80, 80], [-80, -80]]\n\
    blocks = [{ at = [0, 0], half = [10, 10], height = 6, colour = \"red\" }]\n\
    line = [[50, 0, -50], [50, 0, 0], [50, 0, 50], [0, 0, 50], [-50, 0, 50], [-50, 0, 0], [-50, 0, -50], [0, 0, -50]]\n\
    grid = [[-64, -24], [-64, 24], [64, 24], [64, -24], [0, 64], [0, -64]]\n";

/// The kinds of thing a map is made of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// The map itself: what it is called, and what it is built of.
    Map,
    Corners,
    Pads,
    Banks,
    Infields,
    Byways,
    Tunnels,
    Jumps,
    Outline,
    Blocks,
    Ramps,
    Decks,
    Line,
    Grid,
    Bricks,
    Hazards,
    Scenery,
}

impl Layer {
    /// Those a circuit has, or an arena, in the order they are gone through.
    pub fn of(arena: bool) -> &'static [Layer] {
        use Layer::*;
        match arena {
            false => &[
                Map, Corners, Pads, Banks, Infields, Byways, Tunnels, Jumps, Hazards, Scenery,
            ],
            true => &[
                Map, Outline, Blocks, Ramps, Decks, Line, Grid, Bricks, Pads, Hazards, Scenery,
            ],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Layer::Map => "map",
            Layer::Corners => "corners",
            Layer::Pads => "speed pads",
            Layer::Banks => "banks",
            Layer::Infields => "infields",
            Layer::Byways => "byways",
            Layer::Tunnels => "tunnels",
            Layer::Jumps => "jumps",
            Layer::Outline => "outline",
            Layer::Blocks => "blocks",
            Layer::Ramps => "ramps",
            Layer::Decks => "decks",
            Layer::Line => "line",
            Layer::Grid => "grid",
            Layer::Bricks => "bricks",
            Layer::Hazards => "hazards",
            Layer::Scenery => "scenery",
        }
    }

    /// What one of the kind is called.
    pub fn one(self) -> &'static str {
        match self {
            Layer::Map => "map",
            Layer::Corners => "corner",
            Layer::Pads => "speed pad",
            Layer::Banks => "bank",
            Layer::Infields => "infield",
            Layer::Byways => "byway",
            Layer::Tunnels => "tunnel",
            Layer::Jumps => "jump",
            Layer::Outline => "corner",
            Layer::Blocks => "block",
            Layer::Ramps => "ramp",
            Layer::Decks => "deck",
            Layer::Line => "place",
            Layer::Grid => "place",
            Layer::Bricks => "brick",
            Layer::Hazards => "hazard",
            Layer::Scenery => "landmark",
        }
    }

    /// Whether another of the kind is put on the map wherever it is wanted: not
    /// the map itself, a grid's six places, or what comes one after another, a
    /// road's corners, an arena's outline and its line, which are drawn or
    /// pulled out from between the two they come between.
    pub fn added(self) -> bool {
        !matches!(
            self,
            Layer::Map | Layer::Corners | Layer::Outline | Layer::Line | Layer::Grid
        )
    }

    /// What the kind is, in a line.
    pub fn about(self) -> &'static str {
        match self {
            Layer::Map => {
                "What the map is called, who made it and what it is built of. Click a name to type another."
            }
            Layer::Corners => "The road's corners.",
            Layer::Pads => "Speed pads: a car that crosses one is sent on faster.",
            Layer::Banks => {
                "Banked stretches: the road tipped up towards the outside of the corner."
            }
            Layer::Infields => {
                "Infields: corners with grass inside them and no barrier, to be cut across."
            }
            Layer::Byways => {
                "Byways: short cuts, straight from one place on the road to another further round."
            }
            Layer::Tunnels => "Tunnels: stretches with a roof over them.",
            Layer::Jumps => "Jumps: the road rises to a lip and comes down past a dip.",
            Layer::Outline => {
                "The corners of the floor, round its middle, with a wall round it. Drag a ring to move one, and a small square to pull out another."
            }
            Layer::Blocks => {
                "Blocks stand on the floor, solid. Their tops are driven on where a ramp or a deck comes up to them."
            }
            Layer::Ramps => "Ramps slope from the floor up to their height.",
            Layer::Decks => "Decks are floors held up, driven on and under.",
            Layer::Line => {
                "The loop the computer's cars go round when they have nobody to go after. Drag a ring to move a place on it, and a small square to pull out another."
            }
            Layer::Grid => "Where the six cars start. Drag a ring to move one.",
            Layer::Bricks => "The bricks that give a car its power-ups.",
            Layer::Hazards => {
                "The game's hazards, one of each at the most. A red post stands for each here: its own model is seen in a race."
            }
            Layer::Scenery => "Landmarks, to be looked at.",
        }
    }
}

const SORTS: [(&str, Sort); 16] = [
    ("hammer", Sort::Hammer),
    ("stones", Sort::Stones),
    ("cannons", Sort::Cannons),
    ("ghost", Sort::Ghost),
    ("dragon", Sort::Dragon),
    ("ark", Sort::Ark),
    ("curse", Sort::Curse),
    ("saucer", Sort::Saucer),
    ("lava", Sort::Lava),
    ("field", Sort::Field),
    ("pillar", Sort::Pillar),
    ("barrels", Sort::Barrels),
    ("crane", Sort::Crane),
    ("warp", Sort::Warp),
    ("smoke", Sort::Smoke),
    ("water", Sort::Water),
];

/// The hazards that want a road to be on (`Stood::amiss`).
fn roaded(sort: Sort) -> bool {
    matches!(
        sort,
        Sort::Stones | Sort::Curse | Sort::Field | Sort::Pillar | Sort::Barrels | Sort::Warp
    )
}

const KINDS: [(&str, Kind); 5] = [
    ("tower", Kind::Tower),
    ("stand", Kind::Stand),
    ("gantry", Kind::Gantry),
    ("pond", Kind::Pond),
    ("rocks", Kind::Rocks),
];

const HUES: [(&str, Hue); 7] = [
    ("any", Hue::Any),
    ("white", Hue::White),
    ("red", Hue::Is(Power::Red)),
    ("yellow", Hue::Is(Power::Yellow)),
    ("blue", Hue::Is(Power::Blue)),
    ("green", Hue::Is(Power::Green)),
    ("changing", Hue::Changing),
];

const COMPASS: [(&str, [f32; 2]); 4] = [
    ("north", [0.0, -1.0]),
    ("east", [1.0, 0.0]),
    ("south", [0.0, 1.0]),
    ("west", [-1.0, 0.0]),
];

/// One of the things that can be changed of whatever is picked: what it is, and
/// what it is now. Words are typed, and the rest turned up and down.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub label: String,
    pub value: String,
    pub typed: bool,
}

/// The rows of a thing as they are gone through, one of them perhaps being
/// changed on the way.
#[derive(Default)]
struct Form {
    rows: Vec<Row>,
    /// The row turned, and how many steps up.
    turn: Option<(usize, f32)>,
    /// The row typed into, and what it now says.
    typed: Option<(usize, String)>,
    changed: bool,
}

impl Form {
    fn turned(&self) -> Option<f32> {
        self.turn
            .filter(|turn| turn.0 == self.rows.len())
            .map(|turn| turn.1)
    }

    fn row(&mut self, label: &str, value: String, typed: bool) {
        self.rows.push(Row {
            label: label.into(),
            value,
            typed,
        });
    }

    fn number(&mut self, label: &str, value: &mut f32, step: f32, least: f32, most: f32) {
        if let Some(by) = self.turned() {
            let to = (((*value + by * step) * 100.0).round() / 100.0).clamp(least, most);
            self.changed |= to != *value;
            *value = to;
        }
        // Nought is nought, whichever side of it it was come to from.
        self.row(label, (*value + 0.0).to_string(), false);
    }

    /// One of several by name; what is none of them is called `other`.
    fn choice<T: PartialEq + Clone>(
        &mut self,
        label: &str,
        value: &mut T,
        among: &[(&str, T)],
        other: &str,
    ) {
        let at = among.iter().position(|choice| choice.1 == *value);
        if let Some(by) = self.turned()
            && !among.is_empty()
        {
            let count = among.len() as i32;
            let step = if by < 0.0 { -1 } else { 1 };
            let to = at.map_or(0, |at| (at as i32 + step).rem_euclid(count)) as usize;
            self.changed |= among[to].1 != *value;
            *value = among[to].1.clone();
        }
        let named = among.iter().find(|choice| choice.1 == *value);
        self.row(
            label,
            named.map_or(other, |choice| choice.0).to_string(),
            false,
        );
    }

    fn flag(&mut self, label: &str, value: &mut bool) {
        if self.turned().is_some() {
            *value = !*value;
            self.changed = true;
        }
        self.row(label, if *value { "yes" } else { "no" }.into(), false);
    }

    fn colour(&mut self, label: &str, value: &mut Color) {
        let own = write::colour(*value);
        self.choice(label, value, &COLOURS, &own);
    }

    fn words(&mut self, label: &str, value: &mut String) {
        if let Some((_, typed)) = self.typed.take_if(|typed| typed.0 == self.rows.len()) {
            self.changed |= typed != *value;
            *value = typed;
        }
        self.row(label, value.clone(), true);
    }

    /// A place on the ground, as two rows.
    fn place(&mut self, at: &mut [f32; 2]) {
        self.number("x", &mut at[0], 1.0, -REACH, REACH);
        self.number("z", &mut at[1], 1.0, -REACH, REACH);
    }

    /// What stands on an arena's floor: where, how far either way, and how high.
    fn stood(&mut self, at: &mut [f32; 2], half: &mut [f32; 2], height: &mut f32) {
        self.place(at);
        self.number("half across", &mut half[0], 1.0, 0.5, REACH);
        self.number("half deep", &mut half[1], 1.0, 0.5, REACH);
        self.number("height", height, 0.5, 0.5, HIGHEST);
    }
}

/// How far from the middle anything may be put, and how high: within what a map
/// may have (`map`'s `REACH` and `HIGHEST`).
const REACH: f32 = 2000.0;
const HIGHEST: f32 = 120.0;

/// A map being made or changed.
pub struct Draft {
    /// What its file is known by, once it has one.
    pub key: Option<String>,
    /// The map as it has been changed to, which may be one that can't be built.
    pub map: Map,
    /// The last of it that could be, if any of it has been.
    pub good: Option<Map>,
    /// What is wrong with it as it is, if anything.
    pub wrong: Option<String>,
    /// A circuit's line through the latest of its corners that make a road: what
    /// is so far round the lap is so far round this.
    line: Option<Track>,
    pub layer: Layer,
    /// Which of that kind of thing is picked, and which of its rows.
    pub picked: usize,
    pub row: usize,
    /// Changed since it was kept.
    pub unsaved: bool,
    /// How many times `good` has changed, for whoever draws it, and how many
    /// times anything has.
    pub version: u32,
    pub edition: u32,
    /// A circuit's road is still being drawn: it is not yet a loop, and there
    /// is none from the last corner drawn to the last of all, which is the back
    /// of the straight the start is on.
    pub open: bool,
    /// What each of a circuit's corners is to be rounded to where there is
    /// room: the map has what there is room for.
    wanted: Vec<f32>,
    /// If the start was on the straight between the last corner and the first
    /// before the change in hand: how far it was from the last corner, and how
    /// much of the straight behind it and ahead of it those corners' rounding
    /// left alone.
    straight: Option<(f32, f32, f32)>,
    undone: Vec<Kept>,
    redone: Vec<Kept>,
    /// What the last change was to, so that a thing moved a step at a time is
    /// taken back in one.
    last: Option<(Layer, usize, u8)>,
}

/// A draft as it was, to be gone back to.
#[derive(Clone)]
struct Kept {
    map: Map,
    wanted: Vec<f32>,
    open: bool,
}

fn flat(corner: &[f32; 4]) -> Vec2 {
    Vec2::new(corner[0], corner[2])
}

/// The place `s` round a line and so far right of its middle.
fn spot(line: &Track, s: f32, right: f32) -> Vec3 {
    let (at, _, across) = line.sample(s);
    at + across * right
}

/// How far round `line` the place nearest `at` is, looking first near `guess`,
/// which settles it where two stretches of the road are in the same place.
fn round_lap(line: &Track, at: Vec3, guess: f32) -> f32 {
    let n = line.n();
    let off = |i: usize| {
        let apart = (i as f32 * line.spacing - guess).abs();
        line.pts[i].distance(at) + 0.2 * apart.min(line.length - apart)
    };
    let i = (0..n)
        .min_by(|&a, &b| off(a).total_cmp(&off(b)))
        .unwrap_or(0);
    // Between that sample and the nearer of the two beside it.
    let (before, after) = ((i + n - 1) % n, (i + 1) % n);
    let (from, to) = match line.pts[before].distance(at) < line.pts[after].distance(at) {
        true => (before, i),
        false => (i, after),
    };
    let span = line.pts[to] - line.pts[from];
    let part = ((at - line.pts[from]).dot(span) / span.length_squared().max(1e-6)).clamp(0.0, 1.0);
    let s = (from as f32 + part) * line.spacing;
    ((s * 10.0).round() / 10.0)
        .rem_euclid(line.length)
        .min(line.length - 0.1)
}

/// The sample of `line` a ray passes nearest, and how far right of it.
fn pointed(line: &Track, origin: Vec3, way: Vec3) -> (usize, f32) {
    let off = |i: usize| {
        let to = line.pts[i] - origin;
        (to - way * to.dot(way).max(0.0)).length_squared()
    };
    let i = (0..line.n())
        .min_by(|&a, &b| off(a).total_cmp(&off(b)))
        .unwrap_or(0);
    let nearest = origin + way * (line.pts[i] - origin).dot(way).max(0.0);
    (i, (nearest - line.pts[i]).dot(line.right[i]))
}

/// Where a ray comes to the level `height`, to the nearest half.
fn levelled(origin: Vec3, way: Vec3, height: f32) -> Option<[f32; 2]> {
    let far = (height - origin.y) / way.y;
    (way.y.abs() > 1e-4 && far > 0.0 && far < 20000.0).then(|| {
        let at = origin + way * far;
        [(at.x * 2.0).round() / 2.0, (at.z * 2.0).round() / 2.0]
    })
}

/// How high the uppermost of an arena is at a place.
fn top(plan: &Plan, [x, z]: [f32; 2]) -> f32 {
    let over = |at: &[f32; 2], half: &[f32; 2]| {
        (x - at[0]).abs() <= half[0] && (z - at[1]).abs() <= half[1]
    };
    let blocks = plan
        .blocks
        .iter()
        .filter(|block| over(&block.at, &block.half))
        .map(|block| block.height);
    let decks = plan
        .decks
        .iter()
        .filter(|deck| over(&deck.at, &deck.half))
        .map(|deck| deck.height);
    blocks.chain(decks).fold(0.0, f32::max)
}

fn ground([x, z]: [f32; 2], height: f32) -> Vec3 {
    Vec3::new(x, height, z)
}

fn whole(v: f32) -> f32 {
    v.round()
}

impl Draft {
    /// One of a map there is: itself, or another like it with no file yet.
    pub fn of(map: &Map, copy: bool) -> Draft {
        let mut map = map.clone();
        let key = (!copy).then(|| map.key.clone());
        if copy {
            map.key = DRAFT.into();
            map.file = None;
            map.author = String::new();
            if map.name.len() + 2 <= lobby_api::MAX_NAME {
                map.name += " 2";
            }
        }
        let mut draft = Draft::bare(map);
        (draft.key, draft.unsaved) = (key, copy);
        draft.settle(false);
        draft
    }

    /// A map to be changed, as it is given, before anything is known of it.
    fn bare(map: Map) -> Draft {
        let layer = Layer::of(map.arena())[1];
        Draft {
            key: None,
            good: None,
            wanted: map.road.corners.iter().map(|corner| corner[3]).collect(),
            map,
            wrong: None,
            line: None,
            layer,
            picked: 0,
            row: 0,
            unsaved: true,
            version: 0,
            edition: 0,
            open: false,
            straight: None,
            undone: Vec::new(),
            redone: Vec::new(),
            last: None,
        }
    }

    /// A new one: an arena that is a square floor with a block in the middle, or
    /// a circuit that is no more than the straight its start is on, for its
    /// road to be drawn from.
    pub fn new(arena: bool) -> Draft {
        if arena {
            let map = Map::read(DRAFT, NEW_ARENA).expect("the builder's own map");
            let mut draft = Draft::of(&map, false);
            (draft.key, draft.unsaved) = (None, true);
            return draft;
        }
        let corners = vec![
            [0.0, 0.0, 0.0, 0.0],
            [STRAIGHT.1, 0.0, 0.0, ROUNDED],
            [-STRAIGHT.0, 0.0, 0.0, ROUNDED],
        ];
        let mut draft = Draft::bare(Map {
            key: DRAFT.into(),
            name: "New Circuit".into(),
            author: String::new(),
            text: String::new(),
            file: None,
            road: Road {
                corners,
                ..default()
            },
            plan: None,
            hazards: Vec::new(),
            look: default(),
            scenery: Vec::new(),
        });
        draft.open = true;
        draft.pick(Layer::Corners, 1);
        draft.settle(false);
        draft
    }

    /// The kinds of thing there are to change of it: of a circuit whose road is
    /// still being drawn, only that.
    pub fn layers(&self) -> &'static [Layer] {
        match self.open {
            true => &[Layer::Map, Layer::Corners],
            false => Layer::of(self.arena()),
        }
    }

    /// The map, if it is built as it is: a loop, with nothing wrong with it.
    pub fn sound(&self) -> Option<&Map> {
        self.good
            .as_ref()
            .filter(|_| !self.open && self.wrong.is_none())
    }

    pub fn arena(&self) -> bool {
        self.map.arena()
    }

    /// How long its lap is, if it is a circuit with a road.
    pub fn lap(&self) -> Option<f32> {
        self.line.as_ref().map(|line| line.length)
    }

    /// The line through its road, if it is a circuit.
    pub fn line(&self) -> Option<&Track> {
        self.line.as_ref()
    }

    /// How many there are of a kind of thing.
    pub fn count(&self, layer: Layer) -> usize {
        let (road, plan) = (&self.map.road, self.map.plan.as_ref());
        let of = |count: fn(&Plan) -> usize| plan.map_or(0, count);
        match layer {
            Layer::Map => 1,
            Layer::Corners => road.corners.len(),
            Layer::Pads => road.pads.len() + of(|plan| plan.pads.len()),
            Layer::Banks => road.banks.len(),
            Layer::Infields => road.infields.len(),
            Layer::Byways => road.byways.len(),
            Layer::Tunnels => road.tunnels.len(),
            Layer::Jumps => road.jumps.len(),
            Layer::Outline => of(|plan| plan.outline.len()),
            Layer::Blocks => of(|plan| plan.blocks.len()),
            Layer::Ramps => of(|plan| plan.ramps.len()),
            Layer::Decks => of(|plan| plan.decks.len()),
            Layer::Line => of(|plan| plan.line.len()),
            Layer::Grid => of(|plan| plan.grid.len()),
            Layer::Bricks => of(|plan| plan.bricks.len()),
            Layer::Hazards => self.map.hazards.len(),
            Layer::Scenery => self.map.scenery.len(),
        }
    }

    /// Picks a thing, which ends whatever was being done to the last.
    pub fn pick(&mut self, layer: Layer, index: usize) {
        if (layer, index) != (self.layer, self.picked) {
            (self.layer, self.row, self.last) = (layer, 0, None);
        }
        self.picked = index.min(self.count(layer).saturating_sub(1));
    }

    /// The next kind of thing, or the one before.
    pub fn step_layer(&mut self, by: i32) {
        let layers = self.layers();
        let at = layers
            .iter()
            .position(|&layer| layer == self.layer)
            .unwrap_or(0) as i32;
        let layer = layers[(at + by).rem_euclid(layers.len() as i32) as usize];
        if layer != self.layer {
            self.pick(layer, 0);
        }
    }

    /// The next of the kind picked, or the one before.
    pub fn step_item(&mut self, by: i32) {
        let count = self.count(self.layer) as i32;
        if count > 0 {
            self.pick(
                self.layer,
                (self.picked as i32 + by).rem_euclid(count) as usize,
            );
        }
    }

    /// Whatever is being done to the thing picked is done: the next change to it
    /// is taken back by itself.
    pub fn seal(&mut self) {
        self.last = None;
    }

    fn as_it_is(&self) -> Kept {
        Kept {
            map: self.map.clone(),
            wanted: self.wanted.clone(),
            open: self.open,
        }
    }

    /// Before a change: the map as it is can be gone back to, unless this is
    /// more of the change before.
    fn touch(&mut self, part: u8) {
        let what = (self.layer, self.picked, part);
        if self.last != Some(what) {
            self.mark();
            self.last = Some(what);
        }
        self.note();
    }

    /// Before a change that is taken back by itself.
    fn mark(&mut self) {
        self.undone.push(self.as_it_is());
        if self.undone.len() > UNDONE {
            self.undone.remove(0);
        }
        self.redone.clear();
        self.last = None;
        self.note();
    }

    /// Notes where a circuit's start is before a change, to keep it on its
    /// straight afterwards, with as much of the straight clear either side of
    /// it as it had, or as the cars want.
    fn note(&mut self) {
        let corners = &self.map.road.corners;
        let n = corners.len();
        self.straight = self.start_back().map(|back| {
            let at = |i: usize| flat(&corners[i % n]);
            let reach = |i: usize| corners[i % n][3] * draw::reach(at(i + n - 1), at(i), at(i + 1));
            (back, back - reach(n - 1), at(0).distance(at(1)) - reach(1))
        });
    }

    /// How much of the straight behind the start and ahead of it the corners'
    /// rounding is to leave alone: what the cars want, unless it had less before
    /// the change in hand.
    fn clear(&self) -> (f32, f32) {
        self.straight.map_or((0.0, 0.0), |(_, behind, ahead)| {
            (KEPT.0.min(behind).max(0.0), KEPT.1.min(ahead).max(0.0))
        })
    }

    /// How far the start is from the last corner, if it is a place on the
    /// straight from that to the first corner after it, as a circuit drawn here
    /// has it and the game's own have.
    fn start_back(&self) -> Option<f32> {
        let corners = &self.map.road.corners;
        let n = corners.len();
        if self.arena() || n < 3 || corners[0][3] != 0.0 {
            return None;
        }
        let (tail, start, front) = (flat(&corners[n - 1]), flat(&corners[0]), flat(&corners[1]));
        let straight = front - tail;
        let length = straight.length();
        if length < 1.0 {
            return None;
        }
        let (back, beside) = (
            (start - tail).dot(straight) / length,
            (start - tail).perp_dot(straight) / length,
        );
        (beside.abs() < 0.75 && back > 0.0 && back < length).then_some(back)
    }

    /// Rounds a circuit's corners to what is wanted of each, or to what there is
    /// room for.
    fn fit(&mut self) {
        let kept = self.clear();
        let corners = &mut self.map.road.corners;
        if self.wanted.len() != corners.len() {
            self.wanted = corners.iter().map(|corner| corner[3]).collect();
        }
        let radii = draw::fitted(corners, &self.wanted, kept);
        for (corner, radius) in corners.iter_mut().zip(radii) {
            corner[3] = radius;
        }
    }

    /// After a change: the map is written and read, and what is wrong with it
    /// found. If its road `moved`, what is so far round the lap is measured again.
    fn settle(&mut self, moved: bool) {
        self.edition += 1;
        // A road as wide as the game's all the way round says nothing of it.
        if self.map.road.widths.iter().all(|&width| width == WIDTH) {
            self.map.road.widths.clear();
        }
        if !self.arena() && moved {
            self.fit();
        }
        // A road that is not a loop yet is no circuit, and has nothing to be
        // wrong with it.
        if self.open {
            (self.map.text, self.wrong) = (self.map.written(), None);
            return;
        }
        if !self.arena() {
            let bare = Map {
                road: Road {
                    corners: self.map.road.corners.clone(),
                    ..default()
                },
                hazards: Vec::new(),
                scenery: Vec::new(),
                ..self.map.clone()
            };
            if let Ok(bare) = Map::read(DRAFT, &bare.written()) {
                let line = Track::line(&bare, false);
                if let Some(old) = self.line.as_ref().filter(|_| moved) {
                    remeasure(&mut self.map, old, &line);
                }
                self.line = Some(line);
            }
        }
        self.map.text = self.map.written();
        match Map::tried(DRAFT, &self.map.text) {
            Ok(_) => {
                (self.good, self.wrong) = (Some(self.map.clone()), None);
                self.version += 1;
            }
            Err(wrong) => self.wrong = Some(wrong),
        }
    }

    fn changed(&mut self, moved: bool) {
        self.unsaved = true;
        // The start stays on the straight between the last corner and the first,
        // wherever those have gone, as near as it can to where it was or to
        // where it was put: clear of where the cars line up behind it.
        let kept = self.clear();
        let corners = &mut self.map.road.corners;
        let n = corners.len();
        if let (true, Some((was, ..)), true) = (moved, self.straight, n >= 3) {
            let (tail, front) = (flat(&corners[n - 1]), flat(&corners[1]));
            let straight = front - tail;
            let length = straight.length();
            if length >= 1.0 {
                let back = (flat(&corners[0]) - tail).dot(straight) / length;
                // Clear of the rounding of the corners either side of it by as
                // much as the cars want, if there is the room; and if there is
                // not, as far from them as it can be, and their rounding gives.
                let reach = |before: usize, corner: usize, after: usize| {
                    corners[corner][3]
                        * draw::reach(
                            flat(&corners[before]),
                            flat(&corners[corner]),
                            flat(&corners[after]),
                        )
                };
                let (least, most) = (
                    reach(n - 2, n - 1, 1) + kept.0,
                    length - reach(n - 1, 1, 2 % n) - kept.1,
                );
                let back = match (
                    least <= most,
                    (least - 0.001..=most + 0.001).contains(&back),
                ) {
                    (true, true) => back,
                    // With a little over, where it is put being to the hundredth.
                    (true, false) => back.clamp(least + 0.02, (most - 0.02).max(least + 0.02)),
                    (false, _) => back.clamp(
                        KEPT.0.min(was).min(length / 2.0),
                        (length - kept.1).max(length / 2.0),
                    ),
                };
                let at = tail + straight / length * back;
                (corners[0][0], corners[0][2]) = (
                    (at.x * 100.0).round() / 100.0,
                    (at.y * 100.0).round() / 100.0,
                );
            }
        }
        self.settle(moved);
    }

    /// Takes the last change back. Whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(kept) = self.undone.pop() else {
            return false;
        };
        self.redone.push(self.as_it_is());
        self.restored(kept);
        true
    }

    /// Makes the change last taken back again.
    pub fn redo(&mut self) -> bool {
        let Some(kept) = self.redone.pop() else {
            return false;
        };
        self.undone.push(self.as_it_is());
        self.restored(kept);
        true
    }

    fn restored(&mut self, kept: Kept) {
        (self.map, self.wanted, self.open) = (kept.map, kept.wanted, kept.open);
        self.last = None;
        if !self.layers().contains(&self.layer) {
            self.layer = Layer::Corners;
        }
        self.picked = self.picked.min(self.count(self.layer).saturating_sub(1));
        self.changed(false);
    }

    /// The rows of the thing picked.
    pub fn rows(&self) -> Vec<Row> {
        let mut form = Form::default();
        if self.picked < self.count(self.layer) {
            let mut map = self.map.clone();
            // A corner's radius is what is wanted of it, and the row says if
            // there is room for less.
            let fits = self.wants(&mut map);
            fill(
                &mut map,
                self.line.as_ref(),
                self.layer,
                self.picked,
                &mut form,
            );
            if let (Some(fits), Some(row)) =
                (fits, form.rows.iter_mut().find(|row| row.label == "radius"))
            {
                row.value = format!("{}  (room for {fits})", row.value);
            }
        }
        form.rows
    }

    /// Gives the corner picked, in `map`, the radius that is wanted of it: what
    /// there is room for, if that is less.
    fn wants(&self, map: &mut Map) -> Option<f32> {
        let wanted = *self
            .wanted
            .get(self.picked)
            .filter(|_| self.layer == Layer::Corners)?;
        let corner = map.road.corners.get_mut(self.picked)?;
        let fits = std::mem::replace(&mut corner[3], wanted);
        (fits < wanted - 0.05).then_some(fits)
    }

    /// Turns a row of the thing picked so many steps up.
    pub fn turn(&mut self, row: usize, by: f32) {
        self.reform(
            Form {
                turn: Some((row, by)),
                ..default()
            },
            row,
        );
    }

    /// Has a row of the thing picked say something else.
    pub fn type_into(&mut self, row: usize, words: String) {
        self.reform(
            Form {
                typed: Some((row, words)),
                ..default()
            },
            row,
        );
    }

    fn reform(&mut self, mut form: Form, row: usize) {
        let before = self.map.clone();
        if self.picked < self.count(self.layer) {
            let mut map = self.map.clone();
            self.wants(&mut map);
            fill(
                &mut map,
                self.line.as_ref(),
                self.layer,
                self.picked,
                &mut form,
            );
            self.map = map;
        }
        if form.changed {
            let after = std::mem::replace(&mut self.map, before);
            self.touch(row as u8 + 8);
            self.map = after;
            if let (Layer::Corners, Some(corner)) =
                (self.layer, self.map.road.corners.get(self.picked))
            {
                self.wanted[self.picked] = corner[3];
            }
            self.changed(self.layer == Layer::Corners);
        } else {
            self.map = before;
        }
    }

    /// Where a thing's handles are: the first is the thing itself, and those
    /// after it the ends of a stretch, the sides of a block, or the places a
    /// hazard has besides its own.
    pub fn handles(&self, layer: Layer, i: usize) -> Vec<Vec3> {
        if i >= self.count(layer) {
            return Vec::new();
        }
        let (road, plan, line) = (&self.map.road, self.map.plan.as_ref(), self.line.as_ref());
        let on = |s: f32, right: f32| line.map_or(Vec3::ZERO, |line| spot(line, s, right));
        // A place on the ground by a circuit's road is as high as the road is there.
        let by_road = |at: [f32; 2]| {
            let flat = ground(at, 0.0);
            let height = line.map_or(0.0, |line| {
                let near = |p: &&Vec3| crate::track::xz_dist2(**p, flat);
                line.pts
                    .iter()
                    .min_by(|a, b| near(a).total_cmp(&near(b)))
                    .map_or(0.0, |p| p.y)
            });
            ground(at, height)
        };
        let stretch =
            |from: f32, to: f32| vec![on((from + to) / 2.0, 0.0), on(from, 0.0), on(to, 0.0)];
        let stood = |at: [f32; 2], half: [f32; 2], height: f32| {
            vec![
                ground(at, height),
                ground([at[0] + half[0], at[1]], height),
                ground([at[0], at[1] + half[1]], height),
            ]
        };
        match (layer, plan) {
            (Layer::Map, _) => Vec::new(),
            // A corner, and the road's edge at its middle, which is dragged to make
            // the road wider or narrower there.
            (Layer::Corners, _) => {
                let [x, height, z, _] = road.corners[i];
                let (middle, right, half) = self.across(i);
                vec![Vec3::new(x, height, z), middle + right * half]
            }
            (Layer::Pads, Some(plan)) => vec![ground(plan.pads[i].at, 0.0)],
            (Layer::Pads, None) => vec![on(road.pads[i].at, road.pads[i].right)],
            (Layer::Banks, _) => stretch(road.banks[i].from, road.banks[i].to),
            (Layer::Infields, _) => stretch(road.infields[i].from, road.infields[i].to),
            (Layer::Byways, _) => stretch(road.byways[i].from, road.byways[i].to),
            (Layer::Tunnels, _) => {
                let (from, to) = (by_road(road.tunnels[i].from), by_road(road.tunnels[i].to));
                vec![(from + to) / 2.0, from, to]
            }
            (Layer::Jumps, _) => vec![by_road(road.jumps[i].lip)],
            (Layer::Hazards, _) => {
                let stood = &self.map.hazards[i];
                let mut handles = match (stood.at, stood.place) {
                    (Some(at), _) => vec![on(at, stood.right)],
                    (None, Some(place)) if plan.is_some() => vec![ground(place, 0.0)],
                    (None, Some(place)) => vec![by_road(place)],
                    (None, None) => Vec::new(),
                };
                handles.extend(
                    stood
                        .drops
                        .iter()
                        .flatten()
                        .map(|drop| on(drop[0], drop[1])),
                );
                handles.extend(
                    stood
                        .out
                        .filter(|_| stood.kind == Sort::Warp)
                        .map(|out| on(out, 0.0)),
                );
                handles
            }
            (Layer::Scenery, _) => vec![ground(self.map.scenery[i].place, 0.0)],
            (_, None) => Vec::new(),
            (Layer::Outline, Some(plan)) => vec![ground(plan.outline[i], 0.0)],
            (Layer::Blocks, Some(plan)) => stood(
                plan.blocks[i].at,
                plan.blocks[i].half,
                plan.blocks[i].height,
            ),
            (Layer::Ramps, Some(plan)) => stood(
                plan.ramps[i].at,
                plan.ramps[i].half,
                plan.ramps[i].height / 2.0,
            ),
            (Layer::Decks, Some(plan)) => {
                stood(plan.decks[i].at, plan.decks[i].half, plan.decks[i].height)
            }
            (Layer::Line, Some(plan)) => vec![Vec3::from(plan.line[i])],
            (Layer::Grid, Some(plan)) => vec![ground(plan.grid[i].at, 0.0)],
            (Layer::Bricks, Some(plan)) => {
                let brick = &plan.bricks[i];
                vec![ground(
                    brick.at,
                    if brick.low { 0.0 } else { top(plan, brick.at) },
                )]
            }
        }
    }

    /// Moves the thing picked a step: over the ground, so far `east` and `south`,
    /// or if it is somewhere round a lap, so many steps `along` it and `across`.
    pub fn nudge(&mut self, east: f32, south: f32, along: f32, across: f32) {
        let i = self.picked;
        if i >= self.count(self.layer) || self.layer == Layer::Map {
            return;
        }
        self.touch(0);
        let lap = self.lap().unwrap_or(0.0);
        let on = |s: f32| {
            (s + along * ALONG)
                .rem_euclid(lap.max(1.0))
                .min((lap - 0.1).max(0.0))
        };
        let shift = |at: &mut [f32; 2]| *at = [at[0] + east, at[1] + south];
        // A stretch is moved whole, and stops at the ends of the lap.
        let slide = |from: &mut f32, to: &mut f32| {
            let by = (along * ALONG).clamp(-*from, (lap - 0.1 - *to).max(0.0));
            (*from, *to) = (*from + by, *to + by);
        };
        let Map {
            road,
            plan,
            hazards,
            scenery,
            ..
        } = &mut self.map;
        match (self.layer, plan) {
            (Layer::Corners, _) => {
                road.corners[i][0] += east;
                road.corners[i][2] += south;
            }
            (Layer::Pads, Some(plan)) => shift(&mut plan.pads[i].at),
            (Layer::Pads, None) => {
                let pad = &mut road.pads[i];
                (pad.at, pad.right) = (
                    on(pad.at),
                    (pad.right + across * 0.5).clamp(-ROAD_HW, ROAD_HW),
                );
            }
            (Layer::Banks, _) => {
                let one = &mut road.banks[i];
                slide(&mut one.from, &mut one.to)
            }
            (Layer::Infields, _) => {
                let one = &mut road.infields[i];
                slide(&mut one.from, &mut one.to)
            }
            (Layer::Byways, _) => {
                let one = &mut road.byways[i];
                slide(&mut one.from, &mut one.to)
            }
            (Layer::Tunnels, _) => {
                shift(&mut road.tunnels[i].from);
                shift(&mut road.tunnels[i].to);
            }
            (Layer::Jumps, _) => shift(&mut road.jumps[i].lip),
            (Layer::Hazards, _) => {
                let stood = &mut hazards[i];
                match (&mut stood.at, &mut stood.place) {
                    (Some(at), _) => {
                        *at = on(*at);
                        stood.right = (stood.right + across * 0.5).clamp(-BESIDE, BESIDE);
                    }
                    (None, Some(place)) => shift(place),
                    (None, None) => {}
                }
            }
            (Layer::Scenery, _) => shift(&mut scenery[i].place),
            (_, None) => {}
            (Layer::Outline, Some(plan)) => shift(&mut plan.outline[i]),
            (Layer::Blocks, Some(plan)) => shift(&mut plan.blocks[i].at),
            (Layer::Ramps, Some(plan)) => shift(&mut plan.ramps[i].at),
            (Layer::Decks, Some(plan)) => shift(&mut plan.decks[i].at),
            (Layer::Line, Some(plan)) => {
                plan.line[i][0] += east;
                plan.line[i][2] += south;
            }
            (Layer::Grid, Some(plan)) => shift(&mut plan.grid[i].at),
            (Layer::Bricks, Some(plan)) => shift(&mut plan.bricks[i].at),
            (Layer::Map, _) => {}
        }
        self.changed(self.layer == Layer::Corners);
    }

    /// Puts a handle of the thing picked where the pointer is: under a ray from
    /// the eye, `way` being its direction.
    pub fn put(&mut self, part: usize, origin: Vec3, way: Vec3) {
        let i = self.picked;
        let Some(&held) = self.handles(self.layer, i).get(part) else {
            return;
        };
        let Some(level) = levelled(origin, way, held.y) else {
            return;
        };
        // Where round the lap the pointer is, and how far right of the road.
        let lap = self.lap().unwrap_or(0.0);
        let aimed = self.line.as_ref().map(|line| {
            let (sample, right) = pointed(line, origin, way);
            let s = whole(sample as f32 * line.spacing).min((lap - 0.1).max(0.0));
            (
                s,
                (right * 2.0).round() / 2.0,
                [whole(line.pts[sample].x), whole(line.pts[sample].z)],
            )
        });
        let before = self.map.clone();
        // A corner by its edge: the road as wide as twice as far as that is from
        // its middle.
        let across = (self.layer == Layer::Corners && part == 1).then(|| self.across(i));
        let Map {
            road,
            plan,
            hazards,
            scenery,
            ..
        } = &mut self.map;
        // A stretch by its middle or by either end, which don't pass each other.
        let stretch = |from: &mut f32, to: &mut f32| {
            let Some((s, ..)) = aimed else { return };
            match part {
                0 => {
                    let half = (*to - *from) / 2.0;
                    let middle = s.clamp(half, (lap - 0.1 - half).max(half));
                    (*from, *to) = (whole(middle - half), whole(middle - half) + half * 2.0);
                }
                1 => *from = s.min(*to - 4.0).max(0.0),
                _ => *to = s.max(*from + 4.0),
            }
        };
        // What stands on a floor by its middle, or by a side of it.
        let stood = |at: &mut [f32; 2], half: &mut [f32; 2]| match part {
            0 => *at = level,
            1 => half[0] = (level[0] - at[0]).abs().max(0.5),
            _ => half[1] = (level[1] - at[1]).abs().max(0.5),
        };
        match (self.layer, plan) {
            (Layer::Corners, _) => match across {
                Some((middle, right, _)) => {
                    // Where the pointer is to the hair, the width being to the half.
                    let pointed = origin + way * ((middle.y - origin.y) / way.y);
                    let out = (pointed - middle).dot(right).abs();
                    if road.widths.len() != road.corners.len() {
                        road.widths = vec![WIDTH; road.corners.len()];
                    }
                    road.widths[i] =
                        (out * 4.0).round().clamp(WIDTHS.0 * 2.0, WIDTHS.1 * 2.0) / 2.0;
                }
                None => (road.corners[i][0], road.corners[i][2]) = (level[0], level[1]),
            },
            (Layer::Pads, Some(plan)) => plan.pads[i].at = level,
            (Layer::Pads, None) => {
                if let Some((s, right, _)) = aimed {
                    (road.pads[i].at, road.pads[i].right) = (s, right.clamp(-ROAD_HW, ROAD_HW));
                }
            }
            (Layer::Banks, _) => {
                let one = &mut road.banks[i];
                stretch(&mut one.from, &mut one.to)
            }
            (Layer::Infields, _) => {
                let one = &mut road.infields[i];
                stretch(&mut one.from, &mut one.to)
            }
            (Layer::Byways, _) => {
                let one = &mut road.byways[i];
                stretch(&mut one.from, &mut one.to)
            }
            (Layer::Tunnels, _) => {
                if let Some((.., place)) = aimed {
                    let tunnel = &mut road.tunnels[i];
                    match part {
                        0 => {
                            let by = [
                                place[0] - (tunnel.from[0] + tunnel.to[0]) / 2.0,
                                place[1] - (tunnel.from[1] + tunnel.to[1]) / 2.0,
                            ];
                            for end in [&mut tunnel.from, &mut tunnel.to] {
                                *end = [whole(end[0] + by[0]), whole(end[1] + by[1])];
                            }
                        }
                        1 => tunnel.from = place,
                        _ => tunnel.to = place,
                    }
                }
            }
            (Layer::Jumps, _) => {
                if let Some((.., place)) = aimed {
                    road.jumps[i].lip = place;
                }
            }
            (Layer::Hazards, _) => {
                let stood = &mut hazards[i];
                match (part, aimed) {
                    (0, Some((s, right, _))) if stood.at.is_some() => {
                        (stood.at, stood.right) = (Some(s), right.clamp(-BESIDE, BESIDE));
                    }
                    (0, _) if stood.place.is_some() => stood.place = Some(level),
                    (part, Some((s, right, _))) => match (&mut stood.drops, &mut stood.out) {
                        (Some(drops), _) if part <= drops.len() => {
                            drops[part - 1] = [s, right.clamp(-ROAD_HW, ROAD_HW)];
                        }
                        (_, Some(out)) => *out = s,
                        _ => {}
                    },
                    _ => {}
                }
            }
            (Layer::Scenery, _) => scenery[i].place = level,
            (_, None) => {}
            (Layer::Outline, Some(plan)) => plan.outline[i] = level,
            (Layer::Blocks, Some(plan)) => {
                let one = &mut plan.blocks[i];
                stood(&mut one.at, &mut one.half)
            }
            (Layer::Ramps, Some(plan)) => {
                let one = &mut plan.ramps[i];
                stood(&mut one.at, &mut one.half)
            }
            (Layer::Decks, Some(plan)) => {
                let one = &mut plan.decks[i];
                stood(&mut one.at, &mut one.half)
            }
            (Layer::Line, Some(plan)) => (plan.line[i][0], plan.line[i][2]) = (level[0], level[1]),
            (Layer::Grid, Some(plan)) => plan.grid[i].at = level,
            (Layer::Bricks, Some(plan)) => plan.bricks[i].at = level,
            (Layer::Map, _) => {}
        }
        if self.map != before {
            let after = std::mem::replace(&mut self.map, before);
            self.touch(part as u8);
            self.map = after;
            self.changed(self.layer == Layer::Corners);
        }
    }

    /// Adds another of the kind picked: a corner, or a place on an outline or a
    /// line, half way to the next, and anything else at `focus`, which is where
    /// the screen is looking.
    pub fn add(&mut self, focus: Vec3) {
        let (i, count) = (self.picked, self.count(self.layer));
        // Nor is a corner put where a road being drawn has not yet come.
        if count >= 300
            || i >= count.max(1)
            || (self.open && self.layer == Layer::Corners && i + 2 == count)
        {
            return;
        }
        self.touch(255);
        let place = [whole(focus.x), whole(focus.z)];
        let lap = self.lap().unwrap_or(0.0);
        // Where on the lap is nearest what is looked at, and so far either side
        // of that, short of the lap's ends.
        let s = self.line.as_ref().map_or(0.0, |line| {
            let near = |p: &&Vec3| crate::track::xz_dist2(**p, focus);
            let nearest = line
                .pts
                .iter()
                .enumerate()
                .min_by(|a, b| near(&a.1).total_cmp(&near(&b.1)));
            whole(nearest.map_or(0.0, |(i, _)| i as f32 * line.spacing))
        });
        let round = |from: f32, to: f32| {
            let middle = s.clamp(-from, (lap - 0.1 - to).max(-from));
            (whole(middle + from), whole(middle + to))
        };
        let on = |s: f32| {
            self.line.as_ref().map_or(place, |line| {
                let at = spot(line, s, 0.0);
                [whole(at.x), whole(at.z)]
            })
        };
        let had: Vec<Sort> = self.map.hazards.iter().map(|stood| stood.kind).collect();
        let arena = self.arena();
        let mut to = count;
        let Map {
            road,
            plan,
            hazards,
            scenery,
            ..
        } = &mut self.map;
        match (self.layer, plan) {
            (Layer::Corners, _) => {
                let (this, next) = (road.corners[i], road.corners[(i + 1) % count]);
                let half = |n: usize| ((this[n] + next[n]) * 5.0).round() / 10.0;
                road.corners
                    .insert(i + 1, [half(0), half(1), half(2), ROUNDED]);
                self.wanted.insert(i + 1, ROUNDED);
                if !road.widths.is_empty() {
                    let width = (road.widths[i] + road.widths[(i + 1) % count]) / 2.0;
                    road.widths.insert(i + 1, (width * 2.0).round() / 2.0);
                }
                to = i + 1;
            }
            (Layer::Pads, Some(plan)) => plan.pads.push(Pad {
                at: place,
                heading: 0.0,
            }),
            (Layer::Pads, None) => road.pads.push(Padded { at: s, right: 0.0 }),
            (Layer::Banks, _) => {
                let (from, to) = round(-20.0, 20.0);
                road.banks.push(Banked {
                    from,
                    to,
                    tilt: 0.15,
                });
            }
            (Layer::Infields, _) => {
                let (from, to) = round(-25.0, 25.0);
                road.infields.push(Stretch { from, to });
            }
            (Layer::Byways, _) => {
                let (from, to) = round(-45.0, 45.0);
                road.byways.push(Byway {
                    from,
                    to,
                    road: 4.0,
                    wall: 5.5,
                    bumps: 0.0,
                    covered: false,
                    taken: 0.3,
                });
            }
            (Layer::Tunnels, _) => {
                let (from, to) = round(-30.0, 30.0);
                road.tunnels.push(Tunnel {
                    from: on(from),
                    to: on(to),
                });
            }
            (Layer::Jumps, _) => road.jumps.push(Jump { lip: on(s) }),
            (Layer::Hazards, _) => {
                let free =
                    |sort: &&(&str, Sort)| !had.contains(&sort.1) && !(arena && roaded(sort.1));
                let Some(&(_, kind)) = SORTS.iter().find(free) else {
                    self.undone.pop();
                    return;
                };
                let mut stood = Stood {
                    kind,
                    at: (!arena).then_some(s),
                    right: 0.0,
                    place: arena.then_some(place),
                    heading: 0.0,
                    reach: None,
                    apart: None,
                    drops: None,
                    out: None,
                };
                fit(&mut stood, self.line.as_ref());
                hazards.push(stood);
            }
            (Layer::Scenery, _) => scenery.push(Landmark {
                kind: Kind::Rocks,
                place,
                heading: 0.0,
                length: Landmark::length(),
                radius: Landmark::radius(),
            }),
            (Layer::Outline, Some(plan)) => {
                let (this, next) = (plan.outline[i], plan.outline[(i + 1) % count]);
                plan.outline.insert(
                    i + 1,
                    [
                        whole((this[0] + next[0]) / 2.0),
                        whole((this[1] + next[1]) / 2.0),
                    ],
                );
                to = i + 1;
            }
            (Layer::Blocks, Some(plan)) => {
                plan.blocks.push(Block {
                    at: place,
                    half: [6.0, 6.0],
                    height: 6.0,
                    colour: RED,
                });
            }
            (Layer::Ramps, Some(plan)) => {
                plan.ramps.push(Ramp {
                    at: place,
                    half: [8.0, 8.0],
                    height: 6.0,
                    climbs: [0.0, -1.0],
                });
            }
            (Layer::Decks, Some(plan)) => {
                plan.decks.push(Deck {
                    at: place,
                    half: [8.0, 8.0],
                    height: 6.0,
                    colour: None,
                });
            }
            (Layer::Line, Some(plan)) => {
                let (this, next) = (plan.line[i], plan.line[(i + 1) % count]);
                let half = |n: usize| ((this[n] + next[n]) * 5.0).round() / 10.0;
                plan.line.insert(i + 1, [half(0), half(1), half(2)]);
                to = i + 1;
            }
            (Layer::Bricks, Some(plan)) => plan.bricks.push(Brick {
                at: place,
                hue: Hue::Any,
                low: false,
            }),
            // There are six places on a grid, and one map.
            _ => {
                self.undone.pop();
                return;
            }
        }
        self.picked = to;
        self.last = None;
        self.changed(self.layer == Layer::Corners);
    }

    /// Takes the thing picked away, where the map can do without it.
    pub fn remove(&mut self) {
        let (i, count) = (self.picked, self.count(self.layer));
        let least = match self.layer {
            Layer::Map | Layer::Grid => usize::MAX,
            Layer::Corners | Layer::Outline => 3,
            Layer::Line => 4,
            _ => 0,
        };
        // A start on its straight is not taken away, the road being measured
        // from it.
        let start = self.layer == Layer::Corners && i == 0 && self.start_back().is_some();
        if i >= count || count <= least || start {
            return;
        }
        self.touch(255);
        let Map {
            road,
            plan,
            hazards,
            scenery,
            ..
        } = &mut self.map;
        match (self.layer, plan) {
            (Layer::Corners, _) => {
                road.corners.remove(i);
                self.wanted.remove(i);
                if !road.widths.is_empty() {
                    road.widths.remove(i);
                }
            }
            (Layer::Pads, Some(plan)) => drop(plan.pads.remove(i)),
            (Layer::Pads, None) => drop(road.pads.remove(i)),
            (Layer::Banks, _) => drop(road.banks.remove(i)),
            (Layer::Infields, _) => drop(road.infields.remove(i)),
            (Layer::Byways, _) => drop(road.byways.remove(i)),
            (Layer::Tunnels, _) => drop(road.tunnels.remove(i)),
            (Layer::Jumps, _) => drop(road.jumps.remove(i)),
            (Layer::Hazards, _) => drop(hazards.remove(i)),
            (Layer::Scenery, _) => drop(scenery.remove(i)),
            (Layer::Outline, Some(plan)) => drop(plan.outline.remove(i)),
            (Layer::Blocks, Some(plan)) => drop(plan.blocks.remove(i)),
            (Layer::Ramps, Some(plan)) => drop(plan.ramps.remove(i)),
            (Layer::Decks, Some(plan)) => drop(plan.decks.remove(i)),
            (Layer::Line, Some(plan)) => drop(plan.line.remove(i)),
            (Layer::Bricks, Some(plan)) => drop(plan.bricks.remove(i)),
            _ => {}
        }
        // Of a road being drawn, its end is picked next, to go on from.
        let drawing = self.open && self.layer == Layer::Corners;
        self.picked = if drawing {
            count.saturating_sub(3)
        } else {
            i.min(count.saturating_sub(2))
        };
        self.last = None;
        self.changed(self.layer == Layer::Corners);
    }

    /// Where a road being drawn ends, and where it is to come back to: the last
    /// corner drawn, and the back of the straight the start is on.
    pub fn ends(&self) -> Option<(Vec3, Vec3)> {
        let corners = &self.map.road.corners;
        let n = corners.len();
        let at = |corner: [f32; 4]| Vec3::new(corner[0], corner[1], corner[2]);
        (self.open && n >= 3).then(|| (at(corners[n - 2]), at(corners[n - 1])))
    }

    /// Whether a place on the ground is where a road being drawn joins its
    /// start: within `snap` of the straight behind the line. There must be some
    /// road to join it.
    pub fn joins(&self, at: Vec2, snap: f32) -> bool {
        let corners = &self.map.road.corners;
        let n = corners.len();
        if !self.open || n < 4 {
            return false;
        }
        let (tail, start) = (flat(&corners[n - 1]), flat(&corners[0]));
        let straight = start - tail;
        let along =
            ((at - tail).dot(straight) / straight.length_squared().max(1e-3)).clamp(0.0, 1.0);
        at.distance(tail + straight * along) < snap
    }

    /// Lays a road being drawn on to `at`: a corner more, before the last of
    /// all.
    pub fn draw(&mut self, at: [f32; 2]) {
        let n = self.map.road.corners.len();
        self.trace(n, &[(Vec2::from(at), ROUNDED)], true);
    }

    /// Has a road being drawn go on from the corners it had when there were
    /// `had` of them in all, round `corners`, each where it is and what it is
    /// to be rounded to: whatever else it had since is gone. A line drawn with
    /// the pointer is laid so, again and again as it grows, and is `fresh` the
    /// first time: all of it is taken back as one thing.
    pub fn trace(&mut self, had: usize, corners: &[(Vec2, f32)], fresh: bool) {
        let n = self.map.road.corners.len();
        if !self.open || had < 3 || had > n || had + corners.len() > 300 {
            return;
        }
        self.layer = Layer::Corners;
        match fresh {
            true => self.mark(),
            false => self.note(),
        }
        // What was drawn since is between the end there was and the last of all.
        let height = self.map.road.corners[had - 2][1];
        let half = |v: f32| (v * 2.0).round() / 2.0;
        let laid = corners
            .iter()
            .map(|(at, _)| [half(at.x), height, half(at.y), 0.0]);
        self.map.road.corners.splice(had - 1..n - 1, laid);
        self.wanted
            .splice(had - 1..n - 1, corners.iter().map(|corner| corner.1));
        // As wide as the road was where it ended.
        if !self.map.road.widths.is_empty() {
            let width = self.map.road.widths[had - 2];
            self.map
                .road
                .widths
                .splice(had - 1..n - 1, corners.iter().map(|_| width));
        }
        (self.picked, self.row, self.last) = (self.map.road.corners.len() - 2, 0, None);
        self.changed(true);
    }

    /// Joins a road being drawn to its start, which makes it a circuit. Whether
    /// there was road to join.
    pub fn close(&mut self) -> bool {
        if !self.open || self.map.road.corners.len() < 4 {
            return false;
        }
        self.mark();
        self.open = false;
        self.changed(true);
        true
    }

    /// Takes away the road from a circuit's last corner but one to its last, for
    /// more to be drawn from there.
    pub fn reopen(&mut self) {
        let n = self.map.road.corners.len();
        if self.arena() || self.open || n < 3 {
            return;
        }
        self.mark();
        self.open = true;
        (self.layer, self.picked, self.row) = (Layer::Corners, n - 2, 0);
        self.changed(false);
    }

    /// The middle of the road as its corners now have it, built or not: all the
    /// way round, or as far as it has been drawn; and all the way round if it
    /// is as good as `joined` to its start, the pointer being where it joins.
    pub fn laid(&self, joined: bool) -> Vec<(Vec3, f32)> {
        let (mut corners, mut widths) =
            (self.map.road.corners.clone(), self.map.road.widths.clone());
        if !self.open || corners.len() < 3 || joined {
            return draw::rounded_wide(&corners, &widths, true);
        }
        // From the back of the start's straight, which is the last corner.
        corners.rotate_right(1);
        if !widths.is_empty() {
            widths.rotate_right(1);
        }
        draw::rounded_wide(&corners, &widths, false)
    }

    /// Where the middle of one of a circuit's corners is, which is inside the
    /// corner by as much as its rounding cuts off; which way is to the right
    /// across the road there; and how far its barriers are from its middle.
    pub fn across(&self, i: usize) -> (Vec3, Vec3, f32) {
        let corners = &self.map.road.corners;
        let n = corners.len().max(1);
        let at = |k: usize| flat(&corners[k % n]);
        let (here, into, out) = (
            at(i),
            (at(i) - at(i + n - 1)).normalize_or(Vec2::X),
            (at(i + 1) - at(i)).normalize_or(Vec2::X),
        );
        let along = (into + out).normalize_or(into);
        let turn = into.angle_to(out).abs().min(3.0);
        let middle = here
            + (out - into).normalize_or_zero()
                * corners[i % n][3]
                * (1.0 / (turn / 2.0).cos() - 1.0);
        let half = self.map.road.widths.get(i).copied().unwrap_or(WIDTH) / 2.0;
        (
            ground(middle.into(), corners[i % n][1]),
            Vec3::new(-along.y, 0.0, along.x),
            half,
        )
    }

    /// Whether a corner is one a car must slow for.
    pub fn tight(&self, corner: usize) -> bool {
        let corners = &self.map.road.corners;
        let end = self.open && corner + 2 >= corners.len();
        // Its inside barrier, if the road is wide, is tighter than its middle.
        let half = self.map.road.widths.get(corner).copied().unwrap_or(WIDTH) / 2.0;
        corner < corners.len() && !end && draw::tight(corners, corner, TIGHTEST.max(half + 2.0))
    }

    /// The places half way from each of the kind picked to the next, where
    /// another can be put between them, and which each comes after: of a road's
    /// corners, an arena's outline and its line.
    pub fn mids(&self) -> Vec<(usize, Vec3)> {
        let between = |places: Vec<Vec3>, skip: &dyn Fn(usize) -> bool| {
            let count = places.len();
            (0..count)
                .filter(|&i| !skip(i))
                .map(|i| (i, (places[i] + places[(i + 1) % count]) / 2.0))
                .collect()
        };
        match (self.layer, &self.map.plan) {
            (Layer::Corners, None) => {
                let corners = &self.map.road.corners;
                let (n, straight) = (corners.len(), self.start_back().is_some());
                // Not where there is no road yet, nor either side of a start
                // that is on its straight.
                let skip =
                    |i: usize| (self.open && i + 2 == n) || (straight && (i == 0 || i + 1 == n));
                between(
                    corners
                        .iter()
                        .map(|c| Vec3::new(c[0], c[1], c[2]))
                        .collect(),
                    &skip,
                )
            }
            (Layer::Outline, Some(plan)) => between(
                plan.outline.iter().map(|&at| ground(at, 0.0)).collect(),
                &|_| false,
            ),
            (Layer::Line, Some(plan)) => between(
                plan.line.iter().map(|&at| Vec3::from(at)).collect(),
                &|_| false,
            ),
            _ => Vec::new(),
        }
    }

    /// Puts another of the kind picked half way from one of them to the next,
    /// and picks it.
    pub fn split(&mut self, after: usize) {
        if after < self.count(self.layer) {
            self.picked = after;
            self.add(Vec3::ZERO);
        }
    }

    /// What its file will be known by: what it is already, or its name made
    /// into one that nothing `taken` has.
    pub fn file_key(&self, taken: impl Fn(&str) -> bool) -> String {
        if let Some(key) = &self.key {
            return key.clone();
        }
        let plain = |c: char| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '-'
            }
        };
        let made: String = self.map.name.chars().map(plain).take(18).collect();
        let made = made.trim_matches('-').to_string();
        let made = if made.is_empty() {
            "MAP".to_string()
        } else {
            made
        };
        let mut key = made.clone();
        let mut n = 2;
        while taken(&key) || key == TRIAL || key == DRAFT {
            key = format!("{made}-{n}");
            n += 1;
        }
        key
    }

    /// It has been kept, as the file known by `key`.
    pub fn kept(&mut self, key: String) {
        (self.key, self.unsaved) = (Some(key), false);
    }
}

/// Goes through the rows of one of a map's things, changing whichever of them the
/// form is to change.
fn fill(map: &mut Map, line: Option<&Track>, layer: Layer, i: usize, form: &mut Form) {
    let (lap, arena) = (line.map_or(0.0, |line| line.length), map.arena());
    // Round a lap, short of the end of it.
    let last = (lap - 0.1).max(0.0);
    match layer {
        Layer::Map => {
            form.words("name", &mut map.name);
            form.words("author", &mut map.author);
            match &mut map.plan {
                Some(plan) => {
                    form.colour("floor", &mut plan.floor[0]);
                    form.colour("floor, between", &mut plan.floor[1]);
                    // Rows across the line: as many as it says, or as a
                    // circuit as long would have.
                    let mut rows = plan.rows.map_or(-1.0, |rows| rows as f32);
                    if let Some(by) = form.turned() {
                        rows = (rows + by.signum()).clamp(-1.0, 40.0);
                        form.changed = true;
                    }
                    plan.rows = (rows >= 0.0).then_some(rows as usize);
                    let said = plan
                        .rows
                        .map_or("as a circuit's".into(), |rows| rows.to_string());
                    form.row("rows of bricks", said, false);
                }
                None => {
                    form.colour("walls", &mut map.look.wall);
                    form.colour("their footing", &mut map.look.footing);
                    form.colour("their coping", &mut map.look.coping);
                    form.colour("their piers' caps", &mut map.look.cap);
                    form.flag("battlements", &mut map.look.battlements);
                }
            }
        }
        Layer::Corners => {
            // How wide the road is at every corner, if it is to say so at one.
            let count = map.road.corners.len();
            if map.road.widths.len() != count {
                map.road.widths = vec![WIDTH; count];
            }
            let corner = &mut map.road.corners[i];
            let [x, height, z, radius] = corner;
            form.number("x", x, 1.0, -REACH, REACH);
            form.number("z", z, 1.0, -REACH, REACH);
            form.number("height", height, 0.5, 0.0, HIGHEST);
            form.number("radius", radius, 1.0, 0.0, 400.0);
            form.number("width", &mut map.road.widths[i], 1.0, WIDTHS.0, WIDTHS.1);
        }
        Layer::Pads if arena => {
            let pad = &mut map.plan.as_mut().unwrap().pads[i];
            form.place(&mut pad.at);
            form.number("heading", &mut pad.heading, 15.0, -180.0, 180.0);
        }
        Layer::Pads => {
            let pad = &mut map.road.pads[i];
            form.number("round the lap", &mut pad.at, ALONG, 0.0, last);
            form.number(
                "right of the middle",
                &mut pad.right,
                0.5,
                -ROAD_HW,
                ROAD_HW,
            );
        }
        Layer::Banks => {
            let bank = &mut map.road.banks[i];
            form.number("from", &mut bank.from, ALONG, 0.0, last);
            form.number("to", &mut bank.to, ALONG, 0.0, last);
            form.number("tilt", &mut bank.tilt, 0.01, -1.0, 1.0);
        }
        Layer::Infields => {
            let infield = &mut map.road.infields[i];
            form.number("from", &mut infield.from, ALONG, 0.0, last);
            form.number("to", &mut infield.to, ALONG, 0.0, last);
        }
        Layer::Byways => {
            let byway = &mut map.road.byways[i];
            form.number("from", &mut byway.from, ALONG, 0.0, last);
            form.number("to", &mut byway.to, ALONG, 0.0, last);
            form.number("road, from its middle", &mut byway.road, 0.5, 1.0, 20.0);
            form.number("walls, from its middle", &mut byway.wall, 0.5, 1.0, 39.5);
            form.number("bumps", &mut byway.bumps, 0.5, 0.0, 19.5);
            form.flag("covered", &mut byway.covered);
            form.number("taken by the computer", &mut byway.taken, 0.05, 0.0, 1.0);
        }
        Layer::Tunnels => {
            let tunnel = &mut map.road.tunnels[i];
            form.number("from x", &mut tunnel.from[0], 1.0, -REACH, REACH);
            form.number("from z", &mut tunnel.from[1], 1.0, -REACH, REACH);
            form.number("to x", &mut tunnel.to[0], 1.0, -REACH, REACH);
            form.number("to z", &mut tunnel.to[1], 1.0, -REACH, REACH);
        }
        Layer::Jumps => form.place(&mut map.road.jumps[i].lip),
        Layer::Outline => form.place(&mut map.plan.as_mut().unwrap().outline[i]),
        Layer::Blocks => {
            let block = &mut map.plan.as_mut().unwrap().blocks[i];
            form.stood(&mut block.at, &mut block.half, &mut block.height);
            form.colour("colour", &mut block.colour);
        }
        Layer::Ramps => {
            let ramp = &mut map.plan.as_mut().unwrap().ramps[i];
            form.stood(&mut ramp.at, &mut ramp.half, &mut ramp.height);
            form.choice("climbs", &mut ramp.climbs, &COMPASS, "");
        }
        Layer::Decks => {
            let deck = &mut map.plan.as_mut().unwrap().decks[i];
            form.stood(&mut deck.at, &mut deck.half, &mut deck.height);
            let mut tints = vec![("the floor's", None)];
            tints.extend(COLOURS.iter().map(|named| (named.0, Some(named.1))));
            let own = deck.colour.map(write::colour).unwrap_or_default();
            form.choice("colour", &mut deck.colour, &tints, &own);
        }
        Layer::Line => {
            let [x, height, z] = &mut map.plan.as_mut().unwrap().line[i];
            form.number("x", x, 1.0, -REACH, REACH);
            form.number("z", z, 1.0, -REACH, REACH);
            form.number("height", height, 0.5, 0.0, HIGHEST);
        }
        Layer::Grid => {
            let start = &mut map.plan.as_mut().unwrap().grid[i];
            form.place(&mut start.at);
            let mut ways = vec![("the middle", None)];
            ways.extend(COMPASS.iter().map(|way| (way.0, Some(way.1))));
            form.choice("faces", &mut start.faces, &ways, "");
        }
        Layer::Bricks => {
            let brick = &mut map.plan.as_mut().unwrap().bricks[i];
            form.place(&mut brick.at);
            form.choice("colour", &mut brick.hue, &HUES, "");
            form.flag("low, under what is over it", &mut brick.low);
        }
        Layer::Hazards => {
            let had: Vec<Sort> = map.hazards.iter().map(|stood| stood.kind).collect();
            let stood = &mut map.hazards[i];
            let free: Vec<(&str, Sort)> = SORTS
                .iter()
                .filter(|sort| sort.1 == stood.kind || !had.contains(&sort.1))
                .filter(|sort| !(arena && roaded(sort.1)))
                .copied()
                .collect();
            let was = stood.kind;
            form.choice("kind", &mut stood.kind, &free, "");
            if stood.kind != was {
                fit(stood, line);
            }
            if let Some(line) = line.filter(|_| !roaded(stood.kind)) {
                let mut by_road = stood.at.is_some();
                form.flag("by the road", &mut by_road);
                match (by_road, stood.at, stood.place) {
                    (true, None, Some(place)) => to_road(stood, line, place),
                    (false, Some(at), None) => to_ground(stood, line, at),
                    _ => {}
                }
            }
            match (&mut stood.at, &mut stood.place) {
                (Some(at), _) => {
                    form.number("round the lap", at, ALONG, 0.0, last);
                    form.number(
                        "right of the middle",
                        &mut stood.right,
                        0.5,
                        -BESIDE,
                        BESIDE,
                    );
                }
                (None, Some(place)) => {
                    form.place(place);
                    form.number("heading", &mut stood.heading, 15.0, -180.0, 180.0);
                }
                (None, None) => {}
            }
            // What only one kind has, and has whether its file says or not.
            match stood.kind {
                Sort::Dragon => {
                    let mut reach = stood.reach.unwrap_or(DRAGON_REACH);
                    form.number("reach", &mut reach, 5.0, 10.0, 400.0);
                    stood.reach = (stood.reach.is_some() || reach != DRAGON_REACH).then_some(reach);
                }
                Sort::Stones => {
                    let mut apart = stood.apart.unwrap_or(STONES_APART);
                    form.number("apart", &mut apart, 1.0, 4.0, 100.0);
                    stood.apart = (stood.apart.is_some() || apart != STONES_APART).then_some(apart);
                }
                Sort::Warp => {
                    if let Some(out) = &mut stood.out {
                        form.number("comes out", out, ALONG, 0.0, last);
                    }
                }
                Sort::Curse => {
                    for (n, drop) in stood.drops.iter_mut().flatten().enumerate() {
                        form.number(&format!("drop {}", n + 1), &mut drop[0], ALONG, 0.0, last);
                        form.number("right of the middle", &mut drop[1], 0.5, -ROAD_HW, ROAD_HW);
                    }
                }
                _ => {}
            }
        }
        Layer::Scenery => {
            let landmark = &mut map.scenery[i];
            form.choice("kind", &mut landmark.kind, &KINDS, "");
            form.place(&mut landmark.place);
            form.number("heading", &mut landmark.heading, 15.0, -180.0, 180.0);
            match landmark.kind {
                Kind::Stand => form.number("length", &mut landmark.length, 5.0, 10.0, 400.0),
                Kind::Pond => form.number("radius", &mut landmark.radius, 1.0, 4.0, 200.0),
                _ => {}
            }
        }
    }
}

/// Gives a hazard what its kind wants and takes from it what its kind hasn't.
fn fit(stood: &mut Stood, line: Option<&Track>) {
    if let (Some(line), Some(place), true) = (line, stood.place, roaded(stood.kind)) {
        to_road(stood, line, place);
    }
    let (at, lap) = (
        stood.at.unwrap_or(0.0),
        line.map_or(0.0, |line| line.length),
    );
    let on = |s: f32| whole(s.rem_euclid(lap.max(1.0))).min((lap - 0.1).max(0.0));
    if stood.kind != Sort::Dragon {
        stood.reach = None;
    }
    if stood.kind != Sort::Stones {
        stood.apart = None;
    }
    // A curse is left half way round the lap from where it is set off, all on
    // one side of the road, and a warp's car comes out a third of the way on.
    stood.drops = (stood.kind == Sort::Curse).then(|| {
        let half = at + lap / 2.0;
        stood.drops.unwrap_or([
            [on(half - 6.0), -3.0],
            [on(half), -3.0],
            [on(half + 6.0), -3.0],
        ])
    });
    stood.out = (stood.kind == Sort::Warp).then(|| stood.out.unwrap_or(on(at + lap / 3.0)));
}

/// Stands a hazard by the road, where it was on the ground.
fn to_road(stood: &mut Stood, line: &Track, place: [f32; 2]) {
    let at = ground(place, 0.0);
    let near = |i: &usize| crate::track::xz_dist2(line.pts[*i], at);
    let i = (0..line.n())
        .min_by(|a, b| near(a).total_cmp(&near(b)))
        .unwrap_or(0);
    let right = (at - line.pts[i]).with_y(0.0).dot(line.right[i]);
    stood.at = Some(whole(i as f32 * line.spacing).min(line.length - 0.1));
    stood.right = ((right * 2.0).round() / 2.0).clamp(-BESIDE, BESIDE);
    (stood.place, stood.heading) = (None, 0.0);
}

/// Stands a hazard on the ground, where it was by the road.
fn to_ground(stood: &mut Stood, line: &Track, at: f32) {
    let (place, way, _) = line.sample(at);
    let place = place + line.sample(at).2 * stood.right;
    stood.place = Some([whole(place.x), whole(place.z)]);
    stood.heading = whole((-way.z).atan2(way.x).to_degrees());
    (stood.at, stood.right) = (None, 0.0);
}

/// Measures again what a circuit has so far round its lap, its road having been
/// changed: each is left where it was on the ground, as near as the new road
/// comes to there.
fn remeasure(map: &mut Map, old: &Track, new: &Track) {
    let moved = |s: &mut f32| {
        let guess = *s / old.length * new.length;
        *s = round_lap(new, spot(old, *s, 0.0), guess);
    };
    let road = &mut map.road;
    road.pads.iter_mut().for_each(|pad| moved(&mut pad.at));
    let banks = road
        .banks
        .iter_mut()
        .map(|bank| (&mut bank.from, &mut bank.to));
    let infields = road
        .infields
        .iter_mut()
        .map(|infield| (&mut infield.from, &mut infield.to));
    let byways = road
        .byways
        .iter_mut()
        .map(|byway| (&mut byway.from, &mut byway.to));
    for (from, to) in banks.chain(infields).chain(byways) {
        moved(from);
        moved(to);
    }
    for stood in &mut map.hazards {
        stood.at.iter_mut().for_each(&moved);
        stood.out.iter_mut().for_each(&moved);
        stood
            .drops
            .iter_mut()
            .flatten()
            .for_each(|drop| moved(&mut drop[0]));
    }
}

/// The colour a thing of a kind is marked in.
pub fn tint(layer: Layer) -> Color {
    match layer {
        Layer::Corners | Layer::Outline => Color::srgb(1.0, 1.0, 1.0),
        Layer::Line => Color::srgb(0.4, 0.9, 1.0),
        Layer::Pads => Color::srgb(1.0, 0.85, 0.1),
        Layer::Hazards => Color::srgb(1.0, 0.3, 0.3),
        Layer::Scenery => Color::srgb(0.5, 1.0, 0.5),
        Layer::Grid => Color::srgb(1.0, 0.6, 0.2),
        Layer::Bricks => GREY,
        _ => Color::srgb(0.9, 0.6, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn every_layer(draft: &Draft) -> Vec<Layer> {
        Layer::of(draft.arena()).to_vec()
    }

    /// A circuit of a few corners, as a file has one.
    const OVAL: &str = "name = \"Oval\"\ncorners = [[0, 0, 0, 0], [100, 0, 0, 30], [100, 0, 80, 30], [-100, 0, 80, 30], [-100, 0, 0, 30]]\n";

    fn oval() -> Draft {
        Draft::of(&Map::read("OVAL", OVAL).unwrap(), false)
    }

    /// A new circuit whose road has been drawn by clicking round a rectangle,
    /// and not yet joined to its start.
    fn clicked() -> Draft {
        let mut draft = Draft::new(false);
        for at in [[200.0, 0.0], [200.0, 150.0], [-150.0, 150.0], [-150.0, 0.0]] {
            draft.draw(at);
        }
        draft
    }

    fn start(draft: &Draft) -> Vec2 {
        flat(&draft.map.road.corners[0])
    }

    #[test]
    fn a_new_circuit_is_a_start_and_its_road_is_drawn_back_to_it() {
        let mut draft = Draft::new(false);
        // Nothing but the straight the start is on: no circuit, and nothing wrong.
        assert!(
            draft.open && draft.wrong.is_none() && draft.good.is_none() && draft.sound().is_none()
        );
        assert_eq!(
            (draft.map.road.corners.len(), draft.layers()),
            (3, &[Layer::Map, Layer::Corners][..])
        );
        let (end, tail) = draft.ends().unwrap();
        assert_eq!(
            (end, tail),
            (
                Vec3::new(STRAIGHT.1, 0.0, 0.0),
                Vec3::new(-STRAIGHT.0, 0.0, 0.0)
            )
        );
        assert!(!draft.close() && !draft.joins(Vec2::new(-30.0, 0.0), 11.0));
        let near = |at: Vec3, to: Vec3| at.distance(to) < 0.1;
        let laid = draft.laid(false);
        assert!(
            near(laid[0].0, tail)
                && near(laid.last().unwrap().0, end)
                && laid.iter().all(|at| at.0.z == 0.0)
        );
        // Each click is a corner more, and the road ends there.
        let mut draft = clicked();
        assert_eq!((draft.map.road.corners.len(), draft.picked), (7, 5));
        assert_eq!(draft.ends().unwrap().0, Vec3::new(-150.0, 0.0, 0.0));
        assert!(draft.open && draft.sound().is_none());
        // It joins its start on the straight behind the line, and nowhere else.
        assert!(
            draft.joins(Vec2::new(-30.0, 4.0), 11.0) && draft.joins(Vec2::new(-70.0, -5.0), 11.0)
        );
        assert!(
            !draft.joins(Vec2::new(25.0, 0.0), 11.0) && !draft.joins(Vec2::new(-30.0, 15.0), 11.0)
        );
        // The road as it is drawn goes from the back of the straight to its end,
        // and all the way round when it is as good as joined.
        let laid = draft.laid(false);
        assert!(near(laid[0].0, tail) && near(laid.last().unwrap().0, Vec3::new(-150.0, 0.0, 0.0)));
        let round = draft.laid(true);
        assert!(near(round[0].0, round.last().unwrap().0) && round.len() > laid.len());
        // Joined, it is a circuit: a lap of the rectangle, less its corners.
        assert!(draft.close() && !draft.open);
        assert_eq!(draft.wrong, None);
        let lap = draft.lap().unwrap();
        assert!((900.0..1000.0).contains(&lap), "{lap}");
        assert!(draft.sound().is_some() && draft.layers().len() > 2 && draft.ends().is_none());
        assert_eq!(draft.map.road.corners[0], [0.0; 4]);
        // The cars line up behind the line on a straight that is left alone.
        let line = draft.line().unwrap();
        assert!((0..15).all(|back| line.sample(-(back as f32) * 2.0).0.z.abs() < 0.01));
        // It is taken back a step at a time, to the bare start.
        assert!(draft.undo() && draft.open && draft.map.road.corners.len() == 7);
        for corners in [6, 5, 4, 3] {
            assert!(draft.undo() && draft.map.road.corners.len() == corners);
        }
        assert!(!draft.undo() && draft.redo() && draft.map.road.corners.len() == 4);
    }

    #[test]
    fn a_new_map_of_either_kind_can_be_built_and_kept() {
        let folder = std::env::temp_dir().join(format!("brick_built_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        for arena in [false, true] {
            let mut draft = if arena { Draft::new(true) } else { clicked() };
            assert!(draft.wrong.is_none() && draft.unsaved && draft.arena() == arena);
            assert_eq!(draft.close(), !arena);
            draft.pick(Layer::Map, 0);
            draft.type_into(0, "My First".into());
            // Named for what it is called, and not as anything there already is.
            let key = draft.file_key(|key| key == "MY-FIRST");
            assert_eq!(key, "MY-FIRST-2");
            let text = draft.sound().unwrap().text.clone();
            let file = crate::map::save_in(&folder, &key, &text).unwrap();
            let read = Map::tried(&key, &std::fs::read_to_string(file).unwrap()).unwrap();
            assert_eq!((read.name.as_str(), read.arena()), ("My First", arena));
            draft.kept(key.clone());
            assert!(!draft.unsaved && draft.file_key(|_| true) == key);
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_road_drawn_by_hand_is_the_road_that_was_drawn() {
        // From the end of the start's straight, round a field with corners of
        // fifty and back along the straight behind the line.
        let mut places = vec![Vec2::new(STRAIGHT.1, 0.0)];
        let mut go = |to: Vec2| {
            let from = *places.last().unwrap();
            let steps = (from.distance(to) / 2.0).ceil() as usize;
            places.extend((1..=steps).map(|step| from.lerp(to, step as f32 / steps as f32)));
        };
        let bends = [
            (Vec2::new(250.0, 50.0), 270.0),
            (Vec2::new(250.0, 150.0), 0.0),
            (Vec2::new(-200.0, 150.0), 90.0),
            (Vec2::new(-200.0, 50.0), 180.0),
        ];
        for (centre, from) in bends {
            for degree in (0..=90).step_by(3) {
                go(centre + Vec2::from_angle((from + degree as f32) * TAU / 360.0) * 50.0);
            }
        }
        go(Vec2::new(-50.0, 0.0));
        let mut draft = Draft::new(false);
        let mut stroke = draw::Stroke::begun(places[0]);
        let (had, mut laid) = (draft.map.road.corners.len(), false);
        for &at in &places[1..] {
            // As the screen lays it: to where the pointer is, or to the line's
            // last corner while the pointer is where the road joins its start.
            stroke.drawn(at, 5.0, 20.0);
            let tail = draft.ends().map(|(_, tail)| Vec2::new(tail.x, tail.z));
            let corners = match draft.joins(at, 11.0) {
                true => stroke.corners(None, tail, 5.0, 20.0),
                false => stroke.corners(Some(at), None, 5.0, 20.0),
            };
            if laid || !corners.is_empty() {
                draft.trace(had, &corners, !laid);
                laid = true;
            }
        }
        assert!(draft.joins(*places.last().unwrap(), 11.0));
        let corners = draft.map.road.corners.len();
        assert!((10..=26).contains(&corners), "{corners} corners");
        assert!(draft.close());
        assert_eq!(draft.wrong, None);
        // The road built is nowhere far from where the pointer went.
        let line = draft.line().unwrap();
        let off = |at: Vec2| {
            line.pts
                .iter()
                .map(|p| Vec2::new(p.x, p.z).distance(at))
                .fold(f32::MAX, f32::min)
        };
        let furthest = places.iter().map(|&at| off(at)).fold(0.0, f32::max);
        assert!(furthest < 4.0, "the road is {furthest} from what was drawn");
        assert!((0..corners).all(|corner| !draft.tight(corner)));
        // All that was drawn in one go is taken back in one.
        assert!(draft.undo() && draft.open && draft.undo());
        assert_eq!(draft.map.road.corners.len(), 3);
    }

    #[test]
    fn the_start_stays_on_the_straight_between_the_corners_either_side_of_it() {
        let mut draft = clicked();
        assert!(draft.close());
        let last = draft.map.road.corners.len() - 1;
        let on_straight = |draft: &Draft| draft.start_back().is_some() && draft.wrong.is_none();
        // The corner after it is moved, and the one before it: the straight
        // turns about the other, and the start with it.
        draft.pick(Layer::Corners, 1);
        draft.nudge(0.0, 25.0, 0.0, 0.0);
        assert!(
            on_straight(&draft) && start(&draft).y > 5.0,
            "{:?}",
            start(&draft)
        );
        draft.pick(Layer::Corners, last);
        draft.nudge(0.0, 25.0, 0.0, 0.0);
        assert!(
            on_straight(&draft) && (start(&draft).y - 25.0).abs() < 0.1,
            "{:?}",
            start(&draft)
        );
        // Moved itself, it goes along the straight and not off it: no nearer
        // the corner behind than the cars line up, and not past the one ahead.
        draft.pick(Layer::Corners, 0);
        let was = start(&draft);
        draft.nudge(10.0, -40.0, 0.0, 0.0);
        assert!(on_straight(&draft) && (start(&draft) - was - Vec2::new(10.0, 0.0)).length() < 0.1);
        // It stops where the cars have their room clear of the corners' rounding,
        // which is left as it was.
        let rounded = |draft: &Draft| {
            (
                draft.map.road.corners[1][3],
                draft.map.road.corners[last][3],
            )
        };
        draft.put(0, Vec3::new(-500.0, 300.0, -90.0), Vec3::NEG_Y);
        draft.note();
        let (_, behind, _) = draft.straight.unwrap();
        assert!(
            on_straight(&draft) && (behind - KEPT.0).abs() < 0.1 && rounded(&draft) == (30.0, 30.0),
            "{behind}"
        );
        draft.put(0, Vec3::new(900.0, 300.0, 0.0), Vec3::NEG_Y);
        draft.note();
        let (_, _, ahead) = draft.straight.unwrap();
        assert!(
            on_straight(&draft) && (ahead - KEPT.1).abs() < 0.1 && rounded(&draft) == (30.0, 30.0),
            "{ahead}"
        );
        // Nor is it taken away, though the corners either side of it may be.
        draft.remove();
        assert_eq!(draft.map.road.corners.len(), last + 1);
        draft.pick(Layer::Corners, 1);
        draft.remove();
        assert!(draft.map.road.corners.len() == last && on_straight(&draft));
    }

    #[test]
    fn a_corner_is_rounded_to_what_there_is_room_for_wherever_it_is_dragged() {
        let mut draft = oval();
        let radius = |draft: &Draft, corner: usize| draft.map.road.corners[corner][3];
        // The third corner is brought up to the second, a little at a time.
        draft.pick(Layer::Corners, 2);
        for step in 0..14 {
            draft.nudge(0.0, -5.0, 0.0, 0.0);
            assert_eq!(draft.wrong, None, "after {} north", (step + 1) * 5);
        }
        assert!(
            radius(&draft, 1) < 8.0 && radius(&draft, 2) < 8.0,
            "{:?}",
            draft.map.road.corners
        );
        assert!(draft.tight(1) && draft.tight(2) && !draft.tight(3));
        // What was wanted of them is had again when there is room again.
        let rows = draft.rows();
        let row = rows.iter().position(|row| row.label == "radius").unwrap();
        assert!(
            rows[row].value.starts_with("30  (room for"),
            "{}",
            rows[row].value
        );
        draft.nudge(0.0, 70.0, 0.0, 0.0);
        assert_eq!((radius(&draft, 1), radius(&draft, 2)), (30.0, 30.0));
        assert_eq!(draft.rows()[row].value, "30");
        // More is wanted than there is room for, and what there is is had.
        draft.turn(row, 60.0);
        assert_eq!(draft.wrong, None);
        assert!(
            radius(&draft, 2) > 35.0 && radius(&draft, 2) < 60.0,
            "{}",
            radius(&draft, 2)
        );
        assert!(draft.rows()[row].value.starts_with("90  (room for"));
        assert!(draft.undo() && radius(&draft, 2) == 30.0 && draft.rows()[row].value == "30");
    }

    #[test]
    fn a_road_is_made_wider_or_narrower_at_a_corner() {
        let mut draft = oval();
        draft.pick(Layer::Corners, 2);
        let rows = draft.rows();
        let width = rows.iter().position(|row| row.label == "width").unwrap();
        assert_eq!(rows[width].value, "16");
        // Turned up, the road says how wide it is at every corner.
        draft.turn(width, 8.0);
        assert_eq!(
            (draft.map.road.widths.clone(), draft.wrong.clone()),
            (vec![16.0, 16.0, 24.0, 16.0, 16.0], None)
        );
        // Its ring is at the road's edge, and the road is twice as wide as that
        // is dragged out from its middle.
        let (middle, right, half) = draft.across(2);
        assert!(
            half == 12.0
                && draft.handles(Layer::Corners, 2)[1].distance(middle + right * 12.0) < 0.01
        );
        draft.put(1, middle + right * 15.0 + Vec3::Y * 100.0, Vec3::NEG_Y);
        assert_eq!(
            (draft.map.road.widths[2], draft.wrong.clone()),
            (30.0, None)
        );
        // Built, it is as wide as that at the corner, and evenly less wide on the
        // way to the next.
        let track = Track::made(draft.sound().unwrap(), false);
        let i = track.nearest(middle);
        assert!((track.half[i] - 15.0).abs() < 0.3, "{}", track.half[i]);
        let toward = track.nearest(Vec3::new(0.0, 0.0, 80.0));
        assert!(track.half[toward] < 15.0 && track.half[toward] > WIDTH / 2.0);
        // A corner pulled out between two is as wide as half way between them,
        // and one taken away takes its width with it.
        draft.split(2);
        assert_eq!(draft.map.road.widths, [16.0, 16.0, 30.0, 23.0, 16.0, 16.0]);
        draft.remove();
        assert_eq!(draft.map.road.widths, [16.0, 16.0, 30.0, 16.0, 16.0]);
        // Taken back to as wide as the game's all the way round, it says nothing.
        while draft.undo() {}
        assert!(draft.map.road.widths.is_empty() && !draft.map.text.contains(", 16]"));
        // A road drawn on from a wide end is as wide.
        let mut draft = clicked();
        draft.pick(Layer::Corners, 5);
        let width = draft
            .rows()
            .iter()
            .position(|row| row.label == "width")
            .unwrap();
        draft.turn(width, 10.0);
        draft.draw([-150.0, -60.0]);
        assert_eq!(draft.map.road.widths[6], 26.0);
        assert_eq!(draft.map.road.widths.len(), draft.map.road.corners.len());
    }

    #[test]
    fn the_game_s_circuits_are_rounded_as_they_were() {
        for map in crate::map::circuits() {
            let mut draft = Draft::of(map, true);
            assert!(draft.start_back().is_some(), "{}", map.key);
            draft.pick(Layer::Corners, 2);
            draft.nudge(0.0, 0.0, 0.0, 0.0);
            assert_eq!(draft.wrong, None, "{}", map.key);
            for (now, was) in draft.map.road.corners.iter().zip(&map.road.corners) {
                assert!(
                    (now[3] - was[3]).abs() < 0.05 && flat(now).distance(flat(was)) < 0.02,
                    "{}: {was:?} is {now:?}",
                    map.key
                );
            }
        }
    }

    #[test]
    fn another_is_pulled_out_from_between_two() {
        let mut draft = oval();
        // Not from either side of the start, which is on its straight.
        let mids = draft.mids();
        assert_eq!(mids.iter().map(|mid| mid.0).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(mids[1].1, Vec3::new(0.0, 0.0, 80.0));
        draft.split(2);
        assert_eq!(
            (
                draft.map.road.corners.len(),
                draft.picked,
                draft.wrong.clone()
            ),
            (6, 3, None)
        );
        assert_eq!(flat(&draft.map.road.corners[3]), Vec2::new(0.0, 80.0));
        draft.put(0, Vec3::new(0.0, 200.0, 130.0), Vec3::NEG_Y);
        assert_eq!(
            (draft.wrong.clone(), flat(&draft.map.road.corners[3])),
            (None, Vec2::new(0.0, 130.0))
        );
        assert!(draft.undo() && draft.undo() && draft.map.road.corners.len() == 5);
        // Nor where a road being drawn has not got to.
        let draft = clicked();
        assert_eq!(
            draft.mids().iter().map(|mid| mid.0).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        // An arena's floor and its line have them all the way round.
        let mut draft = Draft::of(crate::map::named("YARD"), true);
        draft.pick(Layer::Outline, 0);
        assert_eq!(draft.mids().len(), 4);
        draft.split(0);
        assert_eq!(
            (
                draft.count(Layer::Outline),
                draft.picked,
                draft.wrong.clone()
            ),
            (5, 1, None)
        );
        draft.pick(Layer::Line, 0);
        assert_eq!(draft.mids().len(), draft.count(Layer::Line));
        draft.pick(Layer::Blocks, 0);
        assert!(draft.mids().is_empty());
    }

    #[test]
    fn a_circuit_is_opened_again_for_more_to_be_drawn() {
        let map = crate::map::named("BRICK");
        let mut draft = Draft::of(map, true);
        let place = |draft: &Draft, layer: Layer, i: usize| draft.handles(layer, i)[0];
        let stood: Vec<Vec3> = (0..3).map(|i| place(&draft, Layer::Hazards, i)).collect();
        let (corners, lap) = (map.road.corners.len(), draft.lap().unwrap());
        draft.reopen();
        assert!(draft.open && draft.sound().is_none() && draft.layers().len() == 2);
        // The road ends at the last corner but one, and wants bringing back to
        // the last, which is behind the start.
        let (end, tail) = draft.ends().unwrap();
        assert_eq!((end.x, end.z, tail.x, tail.z), (-124.0, 48.0, -124.0, 8.0));
        draft.draw([-150.0, 40.0]);
        draft.draw([-150.0, 20.0]);
        assert!(draft.close());
        assert_eq!(draft.wrong, None);
        assert!(draft.map.road.corners.len() == corners + 2 && draft.lap().unwrap() > lap + 10.0);
        // What stands round the lap is where it stood.
        for (i, was) in stood.iter().enumerate() {
            assert!(
                place(&draft, Layer::Hazards, i).distance(*was) < 1.5,
                "hazard {i}"
            );
        }
        // Taking the last corner drawn away while it is open leaves its end
        // picked, to go on from.
        draft.reopen();
        draft.remove();
        assert_eq!(
            (draft.map.road.corners.len(), draft.picked),
            (corners + 1, corners - 1)
        );
    }

    #[test]
    fn every_row_of_everything_in_the_game_s_maps_is_turned_and_turned_back() {
        for map in crate::map::stock() {
            let mut draft = Draft::of(map, false);
            assert_eq!(draft.wrong, None, "{}", map.key);
            for layer in every_layer(&draft) {
                for index in 0..draft.count(layer).min(3) {
                    draft.pick(layer, index);
                    let rows = draft.rows();
                    assert!(!rows.is_empty(), "{} {layer:?}", map.key);
                    assert!(!draft.handles(layer, index).is_empty() || layer == Layer::Map);
                    for row in 0..rows.len() {
                        if rows[row].typed {
                            continue;
                        }
                        let before = draft.map.clone();
                        draft.turn(row, 1.0);
                        // A change is one step back from being undone.
                        if draft.map != before {
                            assert!(draft.undo(), "{} {layer:?} {}", map.key, rows[row].label);
                        }
                        assert_eq!(
                            draft.map.road, before.road,
                            "{} {layer:?} {}",
                            map.key, rows[row].label
                        );
                        assert_eq!(draft.map.plan, before.plan);
                        assert_eq!(draft.map.hazards, before.hazards);
                    }
                }
            }
            assert_eq!(draft.wrong, None, "{}", map.key);
            assert_eq!(draft.map.written(), map.written(), "{}", map.key);
        }
    }

    #[test]
    fn what_is_added_can_be_built_and_what_is_taken_away_is_gone() {
        for map in crate::map::stock() {
            let mut draft = Draft::of(map, true);
            let focus = draft.handles(draft.layer, 0)[0];
            for layer in every_layer(&draft) {
                draft.pick(layer, 0);
                let count = draft.count(layer);
                draft.add(focus + Vec3::new(3.0, 0.0, 3.0));
                let added = draft.count(layer) > count;
                // There is one map, six places on a grid and one of each hazard.
                let full = layer == Layer::Hazards && count == SORTS.len();
                assert_eq!(
                    added,
                    !matches!(layer, Layer::Map | Layer::Grid) && !full,
                    "{} {layer:?}",
                    map.key
                );
                if added {
                    // One that comes after another is put after the one picked.
                    assert_eq!(
                        draft.picked,
                        if layer.added() { count } else { 1 },
                        "{} {layer:?}",
                        map.key
                    );
                    // The new thing can be moved by its handles and by steps.
                    for part in 0..draft.handles(layer, draft.picked).len() {
                        draft.put(part, focus + Vec3::new(4.0, 300.0, 9.0), Vec3::NEG_Y);
                    }
                    draft.nudge(1.0, 1.0, 1.0, 1.0);
                    draft.remove();
                    assert_eq!(draft.count(layer), count, "{} {layer:?}", map.key);
                }
            }
            // Taken back as far as there is to take, it is the map it was.
            while draft.undo() {}
            assert_eq!(draft.wrong, None, "{}", map.key);
            assert_eq!(
                (&draft.map.road, &draft.map.plan, &draft.map.hazards),
                (&map.road, &map.plan, &map.hazards)
            );
        }
    }

    #[test]
    fn what_is_round_the_lap_stays_where_it_is_when_the_road_is_changed() {
        let map = crate::map::named("BRICK");
        let mut draft = Draft::of(map, true);
        let place = |draft: &Draft, layer: Layer, i: usize| draft.handles(layer, i)[0];
        let stood: Vec<Vec3> = (0..3).map(|i| place(&draft, Layer::Hazards, i)).collect();
        let pads: Vec<Vec3> = (0..2).map(|i| place(&draft, Layer::Pads, i)).collect();
        let lap = draft.lap().unwrap();
        // The second corner, at the end of the first straight, is pushed out,
        // which makes everything after it further round.
        draft.pick(Layer::Corners, 1);
        for _ in 0..12 {
            draft.nudge(1.0, 0.0, 0.0, 0.0);
        }
        assert_eq!(draft.wrong, None);
        assert!(draft.lap().unwrap() > lap + 8.0);
        assert!(draft.map.road.pads[0].at > map.road.pads[0].at + 8.0);
        // The hammer is on the straight before it, and is no further round.
        assert!((draft.map.hazards[0].at.unwrap() - map.hazards[0].at.unwrap()).abs() < 1.0);
        for (i, was) in stood.iter().enumerate().skip(1) {
            assert!(
                place(&draft, Layer::Hazards, i).distance(*was) < 13.0,
                "hazard {i}"
            );
        }
        for (i, was) in pads.iter().enumerate() {
            assert!(
                place(&draft, Layer::Pads, i).distance(*was) < 1.0,
                "pad {i}"
            );
        }
        // All of that was one thing done, and is taken back as one.
        assert!(draft.undo() && !draft.undo());
        assert_eq!(draft.map.road, map.road);
    }

    #[test]
    fn what_is_wrong_is_said_and_the_last_that_was_right_is_kept() {
        let mut draft = oval();
        // A kind it has none of has no rows.
        draft.pick(Layer::Pads, 0);
        assert!(draft.rows().is_empty() && draft.handles(Layer::Pads, 0).is_empty());
        // A byway turned until it ends before it begins is no byway.
        draft.pick(Layer::Byways, 0);
        draft.add(Vec3::new(0.0, 0.0, 80.0));
        assert_eq!(draft.wrong, None);
        let version = draft.version;
        let to = draft
            .rows()
            .iter()
            .position(|row| row.label == "to")
            .unwrap();
        draft.turn(to, -50.0);
        assert!(
            draft
                .wrong
                .as_deref()
                .is_some_and(|wrong| wrong.contains("byway")),
            "{:?}",
            draft.wrong
        );
        assert!(draft.sound().is_none() && draft.version == version);
        assert_eq!(
            draft.good.as_ref().unwrap().road.byways[0].to - draft.map.road.byways[0].to,
            100.0
        );
        draft.turn(to, 50.0);
        assert!(draft.wrong.is_none() && draft.sound().is_some() && draft.version > version);
        // A hazard changed to one that wants more is given it.
        draft.pick(Layer::Hazards, 0);
        draft.add(Vec3::ZERO);
        while draft.map.hazards[0].kind != Sort::Curse {
            draft.turn(0, 1.0);
        }
        assert!(draft.map.hazards[0].drops.is_some() && draft.wrong.is_none());
        assert_eq!(draft.handles(Layer::Hazards, 0).len(), 4);
        // And one of a kind there is already is not offered.
        draft.add(Vec3::ZERO);
        assert_eq!(draft.map.hazards[1].kind, Sort::Hammer);
        for _ in 0..40 {
            draft.turn(0, 1.0);
            assert_ne!(draft.map.hazards[1].kind, Sort::Curse);
        }
        while draft.map.hazards[1].kind != Sort::Hammer {
            draft.turn(0, 1.0);
        }
        // Stood on the ground and by the road again, it is where it was.
        let was = draft.handles(Layer::Hazards, 1)[0];
        let flag = draft
            .rows()
            .iter()
            .position(|row| row.label == "by the road")
            .unwrap();
        draft.turn(flag, 1.0);
        assert!(
            draft.map.hazards[1].place.is_some()
                && draft.handles(Layer::Hazards, 1)[0].distance(was) < 1.5
        );
        draft.turn(flag, 1.0);
        assert!(
            draft.map.hazards[1].at.is_some()
                && draft.handles(Layer::Hazards, 1)[0].distance(was) < 2.5
        );
    }
}
