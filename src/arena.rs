//! The arenas: places to fight in and not to race round, for `net::modes`' battle and
//! game of tag. The port's own.
//!
//! An arena is a walled floor with no road: a car goes where it likes. Each is laid
//! out for the two games fought in it. Every way through it comes round again, so
//! that a car is never stopped at the end of one: a car chased has always somewhere
//! to go, and the car after it somewhere to cut it off. There is high ground with
//! more than one way up and any edge of it to come down by, which is near to what
//! is under it and a long way from it by road, so that a car is not caught, or
//! shot, from below without being gone after. What stands on the floor breaks the
//! sight of one way from another, for a shot needs a clear line and so does the
//! car that is "it", which the others see through anything. And the bricks are
//! put where they are on purpose: coloured ones by the starts and round the
//! outside, to be had at once, and the white ones, which a car carries until it
//! fires and which make whatever it fires the stronger, only in the middle or on
//! the high ground, where everybody is seen and a hazard stands over them. A car
//! has those to go in for or to keep out of.
//!
//! It is built of blocks, which stand on the floor, ramps, and decks, which are
//! held up over it and can be driven under, with speed pads on the floor where its
//! map lays them. Each is a map (`map::Map`, a file of `maps/`) with a `Plan` in
//! place of a road. Each still has a line (`Plan::line`), a loop over the floor and
//! the high ground that nothing is made to follow. The computer's cars go round
//! it when they have nobody to go after (`kart::Ai::quarry`), some one way and
//! some the other, so that they meet. One that has left it comes back to the
//! nearest of it on its own level (`Track::rejoined`), round whatever is in the
//! way by the floor's own ways back (`Ways`), and goes on round it whichever way
//! it is then heading. A row of bricks lies across the line every so often if
//! the map asks for rows. The cars start where the map says, facing its middle or
//! the way it says. There are no gates, lap zones or finishing line, so nothing
//! counts a lap or says a car is going the wrong way.
//!
//! Every start of the four the game carries is about as far as the rest from its
//! first brick and from the white ones. A mirrored race turns the plan over
//! (`Plan::mirrored`), whatever it is.

use crate::assets::materials::Surface;
use crate::collision::Collision;
use crate::items::Power;
use crate::meshgen::*;
use crate::track::{PAD_LENGTH, PAD_WIDTH};
use bevy::prelude::*;
use serde::de::Error;
use serde::{Deserialize, Deserializer};
use std::f32::consts::TAU;

/// How high an arena's wall is: more than a car blown up rises.
const WALL_HEIGHT: f32 = 7.0;
const WALL_THICK: f32 = 1.2;
/// How long each brick of the wall is, or thereabouts.
const WALL_BRICK: f32 = 8.0;
/// How wide the road an arena's line is taken to have: what the bricks' rows and the
/// computer's wandering are spread over.
pub const LINE_ROAD: f32 = 5.0;

/// Something solid standing on the floor: where its middle is (x, z), how far it
/// reaches either way, and how high it is.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub at: [f32; 2],
    pub half: [f32; 2],
    pub height: f32,
    #[serde(deserialize_with = "crate::map::colour")]
    pub colour: Color,
}

/// A slope from the floor up to `height`: where its middle is, how far it reaches
/// either way, and which way along the ground it climbs, which a map says as a
/// point of the compass.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Ramp {
    pub at: [f32; 2],
    pub half: [f32; 2],
    pub height: f32,
    #[serde(deserialize_with = "compass")]
    pub climbs: [f32; 2],
}

/// A floor held up at `height`, to be driven on and under, and its colour where it
/// isn't the grey of the rest.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Deck {
    pub at: [f32; 2],
    pub half: [f32; 2],
    pub height: f32,
    #[serde(default, deserialize_with = "tinted")]
    pub colour: Option<Color>,
}

fn tinted<'de, D: Deserializer<'de>>(from: D) -> Result<Option<Color>, D::Error> {
    crate::map::colour(from).map(Some)
}

/// What a brick an arena's map lays is: a colour of its own by its place among
/// them, where the map doesn't say, a white one, the colour the map says, or one
/// that changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hue {
    Any,
    White,
    Is(Power),
    /// Going from one colour to the next as it lies (`items`).
    Changing,
}

/// A brick lying in an arena: where, (x, z), on whatever is uppermost there, or on
/// the floor under it if it is `low`.
#[derive(Clone, Debug, PartialEq)]
pub struct Brick {
    pub at: [f32; 2],
    pub hue: Hue,
    pub low: bool,
}

impl<'de> Deserialize<'de> for Brick {
    /// A place, `[x, z]`, or `{ at = [x, z] }` with its `colour`, and `low` if it
    /// lies under something.
    fn deserialize<D: Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Said {
            at: [f32; 2],
            colour: Option<String>,
            #[serde(default)]
            low: bool,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Laid {
            Place([f32; 2]),
            Said(Said),
        }
        let said = match Laid::deserialize(from) {
            Ok(Laid::Place(at)) => return Ok(Brick { at, hue: Hue::Any, low: false }),
            Ok(Laid::Said(said)) => said,
            Err(_) => {
                return Err(D::Error::custom(
                    "a brick is its place, [x, z], or { at = [x, z] } with its `colour` and whether it is `low`",
                ));
            }
        };
        let hue = match said.colour.as_deref() {
            None => Hue::Any,
            Some("white") => Hue::White,
            Some("red") => Hue::Is(Power::Red),
            Some("yellow") => Hue::Is(Power::Yellow),
            Some("blue") => Hue::Is(Power::Blue),
            Some("green") => Hue::Is(Power::Green),
            Some("changing") => Hue::Changing,
            Some(other) => {
                return Err(D::Error::custom(format!(
                    "a brick is white, red, yellow, blue, green or changing, not {other}"
                )));
            }
        };
        Ok(Brick { at: said.at, hue, low: said.low })
    }
}

/// Where a car starts, (x, z), and the way it faces along the ground if that is
/// not at the middle of the floor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    pub at: [f32; 2],
    pub faces: Option<[f32; 2]>,
}

impl<'de> Deserialize<'de> for Start {
    /// A place, `[x, z]`, or `{ at = [x, z], faces = "north" }`.
    fn deserialize<D: Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Faced {
            at: [f32; 2],
            #[serde(deserialize_with = "compass")]
            faces: [f32; 2],
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Placed {
            Place([f32; 2]),
            Faced(Faced),
        }
        match Placed::deserialize(from) {
            Ok(Placed::Place(at)) => Ok(Start { at, faces: None }),
            Ok(Placed::Faced(start)) => Ok(Start { at: start.at, faces: Some(start.faces) }),
            Err(_) => Err(D::Error::custom(
                "a place on the grid is [x, z], or { at = [x, z], faces = \"north\" } (or south, east, west)",
            )),
        }
    }
}

/// A speed pad on the floor: where its middle is, (x, z), and the way its arrows
/// point, in degrees round from east to north. It sets a car off whichever way
/// the car crosses it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pad {
    pub at: [f32; 2],
    pub heading: f32,
}

impl Pad {
    /// The way it points along the ground, and across that to the right.
    fn axes(&self) -> (Vec3, Vec3) {
        let (sin, cos) = self.heading.to_radians().sin_cos();
        (Vec3::new(cos, 0.0, -sin), Vec3::new(sin, 0.0, cos))
    }
}

/// The way a point of the compass is along the ground: east is x, and south z.
fn compass<'de, D: Deserializer<'de>>(from: D) -> Result<[f32; 2], D::Error> {
    match String::deserialize(from)?.as_str() {
        "east" => Ok([1.0, 0.0]),
        "west" => Ok([-1.0, 0.0]),
        "south" => Ok([0.0, 1.0]),
        "north" => Ok([0.0, -1.0]),
        other => Err(D::Error::custom(format!(
            "a ramp climbs north, south, east or west, not {other}"
        ))),
    }
}

/// How thick a deck is.
const DECK_THICK: f32 = 1.0;
/// How far apart the studs on a block or a deck are, about.
const STUD: f32 = 4.0;

/// How an arena is laid out, as its map has it (`map::Map`).
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The corners of the floor in order, as (x, z). Every one can be seen from the
    /// middle, which is (0, 0).
    pub outline: Vec<[f32; 2]>,
    pub blocks: Vec<Block>,
    pub ramps: Vec<Ramp>,
    pub decks: Vec<Deck>,
    /// The line's control points, as (x, height, z): a spline is put through them.
    pub line: Vec<[f32; 3]>,
    /// Where the six cars start.
    pub grid: [Start; 6],
    /// The bricks the map lays.
    pub bricks: Vec<Brick>,
    /// How many rows of bricks lie across the line besides, coloured and white by
    /// turns: as many as a circuit as long would have, if the map doesn't say.
    pub rows: Option<usize>,
    pub pads: Vec<Pad>,
    /// The floor's two colours, which alternate outwards from the middle.
    pub floor: [Color; 2],
}

fn ground([x, z]: [f32; 2]) -> Vec3 {
    Vec3::new(x, 0.0, z)
}

/// How wide the squares are that an arena's floor is cut into for its ways back to
/// the line, and the most of them there are across it, a bigger floor having bigger
/// ones; how far round the middle of one must be clear for a car to be sent
/// through it, and how high ground may be and still be floor: the foot of a ramp is.
const WAY_CELL: f32 = 4.0;
const WAY_ACROSS: f32 = 160.0;
const WAY_CLEAR: f32 = 2.5;
const WAY_FLOOR: f32 = 1.2;
/// How many squares on from its own a car is sent for.
const WAY_AHEAD: usize = 6;

/// The ways back to an arena's line over its floor, for a car that has left the line
/// and can't see it: round what stands between, by way of whichever square of the
/// floor is the next nearer to the line from the one the car is in.
pub struct Ways {
    low: Vec3,
    cell: f32,
    across: usize,
    /// For each square, the one next nearer the line from it, if a car can be there
    /// and the line isn't in it.
    next: Vec<Option<usize>>,
    /// The squares the line crosses on the floor.
    line: Vec<bool>,
}

impl Ways {
    /// From the floor as it is built, and the line's samples.
    pub fn found(collision: &Collision, reach: f32, line: &[Vec3]) -> Ways {
        let lift = Vec3::Y * crate::physics::BODY_POINT_HEIGHT;
        let cell = WAY_CELL.max(2.0 * reach / WAY_ACROSS);
        let across = (2.0 * reach / cell).ceil() as usize + 1;
        let mut ways = Ways {
            low: Vec3::new(-reach, 0.0, -reach),
            cell,
            across,
            next: vec![None; across * across],
            line: vec![false; across * across],
        };
        // A square a car can be in: floor, with nothing standing close round its middle.
        let on_floor = |at: Vec3| {
            let under = collision.ground(at + Vec3::Y * (WAY_FLOOR + 1.0), WAY_FLOOR + 3.0);
            under.is_some_and(|hit| hit.point.y < WAY_FLOOR)
        };
        let open = |at: Vec3, to: Vec3| collision.wall(at + lift, to + lift).is_none();
        let round = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z];
        let free: Vec<bool> = (0..across * across)
            .map(|square| {
                let at = ways.middle(square);
                on_floor(at) && round.iter().all(|&way| open(at, at + way * WAY_CLEAR))
            })
            .collect();
        // Outwards from the squares the line crosses on the floor, a square at a time.
        let on_line = line.iter().filter(|p| p.y < WAY_FLOOR).filter_map(|&p| ways.square(p));
        let mut reached: std::collections::VecDeque<usize> = on_line.filter(|&square| free[square]).collect();
        for &square in &reached {
            ways.line[square] = true;
        }
        while let Some(square) = reached.pop_front() {
            let (x, z) = ((square % across) as i32, (square / across) as i32);
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, nz) = (x + dx, z + dz);
                if nx < 0 || nz < 0 || nx >= across as i32 || nz >= across as i32 {
                    continue;
                }
                let beside = nx as usize + nz as usize * across;
                let known = ways.line[beside] || ways.next[beside].is_some();
                if known || !free[beside] || !open(ways.middle(beside), ways.middle(square)) {
                    continue;
                }
                ways.next[beside] = Some(square);
                reached.push_back(beside);
            }
        }
        ways
    }

    /// The square a place is in, if it is one of them.
    fn square(&self, p: Vec3) -> Option<usize> {
        let at = (p - self.low) / self.cell;
        let inside = at.x >= 0.0 && at.z >= 0.0 && (at.x as usize) < self.across && (at.z as usize) < self.across;
        inside.then(|| at.x as usize + at.z as usize * self.across)
    }

    fn middle(&self, square: usize) -> Vec3 {
        let (x, z) = ((square % self.across) as f32, (square / self.across) as f32);
        self.low + Vec3::new(x + 0.5, 0.0, z + 0.5) * self.cell
    }

    /// Where a car on the floor at `pos` is to make for to get back to the line:
    /// the middle of the square furthest on from its own, of the next few, that it
    /// has a `clear` run at. A car against something is in a square no way goes
    /// through, and begins from the nearest about it that one does. Nowhere, if it
    /// is on the line's own squares already or somewhere no way was found from.
    pub fn back(&self, pos: Vec3, clear: impl Fn(Vec3) -> bool) -> Option<Vec3> {
        let own = self.square(pos)?;
        if self.line[own] {
            return None;
        }
        let known = |square: &usize| self.line[*square] || self.next[*square].is_some();
        let from = match self.next[own] {
            Some(next) => next,
            None => {
                let (x, z) = ((own % self.across) as i32, (own / self.across) as i32);
                let about = (-2..=2).flat_map(|dz| (-2..=2).map(move |dx| (x + dx, z + dz)));
                let inside = |&(x, z): &(i32, i32)| {
                    x >= 0 && z >= 0 && x < self.across as i32 && z < self.across as i32
                };
                let away = |square: &usize| self.middle(*square).distance(pos);
                about
                    .filter(inside)
                    .map(|(x, z)| x as usize + z as usize * self.across)
                    .filter(known)
                    .filter(|&square| clear(self.middle(square)))
                    .min_by(|a, b| away(a).total_cmp(&away(b)))?
            }
        };
        let mut on = vec![from];
        for _ in 1..WAY_AHEAD {
            on.extend(self.next[on[on.len() - 1]]);
        }
        let seen = on.iter().rev().map(|&square| self.middle(square)).find(|&middle| clear(middle));
        Some(seen.unwrap_or(self.middle(from)))
    }
}

impl Ramp {
    /// Its corners on the floor: the two of its low end, then the two of its high
    /// end, round in order.
    fn corners(&self) -> [Vec3; 4] {
        let (middle, up) = (ground(self.at), ground(self.climbs));
        let across = Vec3::new(-up.z, 0.0, up.x);
        // How far it reaches along the way it climbs, and across that.
        let reach = |way: Vec3| (way.x * self.half[0]).abs() + (way.z * self.half[1]).abs();
        let (along, side) = (up * reach(up), across * reach(across));
        [
            middle - along - side,
            middle - along + side,
            middle + along + side,
            middle + along - side,
        ]
    }
}

impl Plan {
    /// The plan turned over, north for south, as a mirrored race has it.
    pub fn mirrored(&self) -> Plan {
        let over = |[x, z]: [f32; 2]| [x, -z];
        Plan {
            outline: self.outline.iter().copied().map(over).collect(),
            blocks: self.blocks.iter().map(|block| Block { at: over(block.at), ..block.clone() }).collect(),
            ramps: self
                .ramps
                .iter()
                .map(|ramp| Ramp { at: over(ramp.at), climbs: over(ramp.climbs), ..ramp.clone() })
                .collect(),
            decks: self.decks.iter().map(|deck| Deck { at: over(deck.at), ..deck.clone() }).collect(),
            line: self.line.iter().map(|&[x, y, z]| [x, y, -z]).collect(),
            grid: self.grid.map(|start| Start { at: over(start.at), faces: start.faces.map(over) }),
            bricks: self.bricks.iter().map(|brick| Brick { at: over(brick.at), ..brick.clone() }).collect(),
            rows: self.rows,
            pads: self.pads.iter().map(|pad| Pad { at: over(pad.at), heading: -pad.heading }).collect(),
            floor: self.floor,
        }
    }

    /// The corners of the floor, each with the next.
    fn sides(&self) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
        let count = self.outline.len();
        (0..count).map(move |i| (ground(self.outline[i]), ground(self.outline[(i + 1) % count])))
    }

    /// The outline of the floor and of each block, each closed, for the map.
    pub fn outlines(&self) -> Vec<Vec<Vec3>> {
        let mut floor: Vec<Vec3> = self.outline.iter().map(|&corner| ground(corner)).collect();
        floor.push(floor[0]);
        let blocks = self.blocks.iter().map(|block| {
            let [x, z] = block.at;
            let [w, d] = block.half;
            [(-w, -d), (w, -d), (w, d), (-w, d), (-w, -d)]
                .map(|(dx, dz)| Vec3::new(x + dx, 0.0, z + dz))
                .to_vec()
        });
        let on = |at: [f32; 2], half: [f32; 2]| {
            let ([x, z], [w, d]) = (at, half);
            [(-w, -d), (w, -d), (w, d), (-w, d), (-w, -d)]
                .map(|(dx, dz)| Vec3::new(x + dx, 0.0, z + dz))
                .to_vec()
        };
        let ramps = self.ramps.iter().map(|ramp| on(ramp.at, ramp.half));
        let decks = self.decks.iter().map(|deck| on(deck.at, deck.half));
        std::iter::once(floor).chain(blocks).chain(ramps).chain(decks).collect()
    }

    /// How far from the middle the floor reaches.
    pub fn reach(&self) -> f32 {
        self.outline
            .iter()
            .map(|&corner| ground(corner).length())
            .fold(0.0, f32::max)
    }

    /// Whether a place is on the floor, `clear` of its wall and of every block that
    /// stands higher than it is.
    #[cfg(test)]
    pub fn open(&self, p: Vec3, clear: f32) -> bool {
        let (height, p) = (p.y, p.with_y(0.0));
        // On the floor: in one of the triangles it is made of, each a side and the
        // middle; and no nearer any side than asked.
        let inside = self.sides().any(|(a, b)| {
            let turn = |from: Vec3, to: Vec3| (to - from).cross(p - from).y;
            let (ab, bo, oa) = (turn(a, b), turn(b, Vec3::ZERO), turn(Vec3::ZERO, a));
            (ab >= 0.0 && bo >= 0.0 && oa >= 0.0) || (ab <= 0.0 && bo <= 0.0 && oa <= 0.0)
        });
        let off_wall = self.sides().all(|(a, b)| {
            let along = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
            p.distance(a + (b - a) * along) >= clear
        });
        let off_blocks = self.blocks.iter().filter(|block| block.height > height + 1.0).all(|block| {
            let beyond = |at: f32, middle: f32, half: f32| ((at - middle).abs() - half).max(0.0);
            let (dx, dz) = (
                beyond(p.x, block.at[0], block.half[0]),
                beyond(p.z, block.at[1], block.half[1]),
            );
            dx.hypot(dz) >= clear && (dx > 0.0 || dz > 0.0 || clear < 0.0)
        });
        inside && off_wall && off_blocks
    }

    /// Where the cars start and which way each faces: at the middle, unless the
    /// map says another.
    pub fn grid(&self) -> Vec<(Vec3, Vec3)> {
        self.grid
            .iter()
            .map(|start| {
                let place = ground(start.at);
                (place, start.faces.map_or(-place.normalize_or(Vec3::X), ground))
            })
            .collect()
    }

    /// Whether a car at `pos` is on one of the speed pads.
    pub fn padded(&self, pos: Vec3) -> bool {
        pos.y < 1.0
            && self.pads.iter().any(|pad| {
                let (along, across) = pad.axes();
                let from = pos - ground(pad.at);
                from.dot(along).abs() < PAD_LENGTH / 2.0 && from.dot(across).abs() < PAD_WIDTH / 2.0
            })
    }

    /// The floor, the wall round it and the blocks on it, to be driven on and into.
    pub fn collide(&self, collision: &mut Collision) {
        let solid = Surface::default();
        let mut quad = |[a, b, c, d]: [Vec3; 4]| {
            collision.add([a, b, c], solid);
            collision.add([a, c, d], solid);
        };
        for (a, b) in self.sides() {
            quad([Vec3::ZERO, a, b, b]);
            let top = Vec3::Y * WALL_HEIGHT;
            quad([a - Vec3::Y, b - Vec3::Y, b + top, a + top]);
        }
        for block in &self.blocks {
            let (middle, [w, d]) = (ground(block.at), block.half);
            let corners = [(-w, -d), (w, -d), (w, d), (-w, d)]
                .map(|(dx, dz)| middle + Vec3::new(dx, 0.0, dz));
            let top = Vec3::Y * block.height;
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                quad([a - Vec3::Y, b - Vec3::Y, b + top, a + top]);
            }
            // Its top, to be driven on where a ramp comes up to it.
            quad(corners.map(|corner| corner + top));
        }
        for ramp in &self.ramps {
            let [low_a, low_b, high_b, high_a] = ramp.corners();
            let top = Vec3::Y * ramp.height;
            quad([low_a, low_b, high_b + top, high_a + top]);
            // Its two sides, and its end where nothing is built up to it.
            quad([low_a, high_a, high_a + top, high_a + top]);
            quad([low_b, high_b, high_b + top, high_b + top]);
            quad([high_a - Vec3::Y, high_b - Vec3::Y, high_b + top, high_a + top]);
        }
        for deck in &self.decks {
            let (middle, [w, d]) = (ground(deck.at), deck.half);
            let corners = [(-w, -d), (w, -d), (w, d), (-w, d)]
                .map(|(dx, dz)| middle + Vec3::new(dx, deck.height, dz));
            quad(corners);
            // Its edges are as thick as it is, and no thicker: what is under it
            // goes under them.
            let under = Vec3::Y * DECK_THICK;
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                quad([a - under, b - under, b, a]);
            }
        }
    }

    /// What is seen of it: the floor in rings of its two colours, the wall in bricks
    /// and the blocks.
    pub fn mesh(&self) -> Mesh {
        let mut b = BrickMesh::default();
        const RINGS: usize = 6;
        for (a, c) in self.sides() {
            for ring in 0..RINGS {
                let (near, far) = (ring as f32 / RINGS as f32, (ring + 1) as f32 / RINGS as f32);
                let colour = self.floor[ring % 2];
                // Face up, whichever way round the corners are given, and just
                // over the ground the scenery stands on.
                let lift = Vec3::Y * 0.03;
                let corners = [a * near, a * far, c * far, c * near].map(|p| p + lift);
                let [p, q, r, t] = corners;
                if (q - p).cross(t - p).y + (r - q).cross(t - q).y >= 0.0 {
                    b.quad(p, q, r, t, colour);
                } else {
                    b.quad(t, r, q, p, colour);
                }
            }
            // The wall, a brick at a time, in the barriers' red and white.
            let length = a.distance(c);
            let bricks = (length / WALL_BRICK).round().max(1.0) as usize;
            let along = (c - a) / bricks as f32;
            let rot = Transform::IDENTITY.looking_to(along, Vec3::Y).rotation;
            let out = along.normalize().cross(Vec3::Y) * (WALL_THICK / 2.0);
            // The side away from the middle is the outside, whichever way round the
            // corners are given.
            let out = if out.dot(a + c) < 0.0 { -out } else { out };
            for i in 0..bricks {
                let middle = a + along * (i as f32 + 0.5) + out;
                for (course, height) in [(0, 2.0), (1, WALL_HEIGHT - 2.0)] {
                    let half = Vec3::new(WALL_THICK / 2.0, height / 2.0, along.length() / 2.0);
                    let up = if course == 0 { height / 2.0 } else { 2.0 + height / 2.0 };
                    let colour = if (i + course) % 2 == 0 { RED } else { WHITE };
                    b.brick(middle + Vec3::Y * up, half, rot, colour, (1, 4));
                }
            }
        }
        // What is built is studded as a brick is, a stud to every so much of it
        // however big it is: its top may be driven on, and a car is not to be lost
        // among them.
        let studs = |half: [f32; 2]| {
            let across = |half: f32| (2.0 * half / STUD).round().max(1.0) as u32;
            (across(half[0]), across(half[1]))
        };
        for block in &self.blocks {
            let half = Vec3::new(block.half[0], block.height / 2.0, block.half[1]);
            let middle = ground(block.at) + Vec3::Y * half.y;
            b.brick(middle, half, Quat::IDENTITY, block.colour, studs(block.half));
        }
        // A ramp is striped, as a jump is, so that it is seen for what it is.
        for ramp in &self.ramps {
            let [low_a, low_b, high_b, high_a] = ramp.corners();
            let top = Vec3::Y * ramp.height;
            const STRIPES: usize = 6;
            for stripe in 0..STRIPES {
                let (from, to) = (
                    stripe as f32 / STRIPES as f32,
                    (stripe + 1) as f32 / STRIPES as f32,
                );
                let at = |low: Vec3, high: Vec3, part: f32| low.lerp(high + top, part) + Vec3::Y * 0.02;
                let corners = [
                    at(low_a, high_a, from),
                    at(low_b, high_b, from),
                    at(low_b, high_b, to),
                    at(low_a, high_a, to),
                ];
                let colour = [YELLOW, BLACK][stripe % 2];
                // Both faces: which is up depends on the way it climbs.
                b.quad(corners[0], corners[1], corners[2], corners[3], colour);
                b.quad(corners[3], corners[2], corners[1], corners[0], colour);
            }
            for (low, high) in [(low_a, high_a), (low_b, high_b)] {
                b.tri(low, high, high + top, DARK_GREY);
                b.tri(high + top, high, low, DARK_GREY);
            }
            b.quad(high_a, high_b, high_b + top, high_a + top, DARK_GREY);
            b.quad(high_b, high_a, high_a + top, high_b + top, DARK_GREY);
        }
        for deck in &self.decks {
            let half = Vec3::new(deck.half[0], DECK_THICK / 2.0, deck.half[1]);
            let middle = ground(deck.at) + Vec3::Y * (deck.height - half.y);
            b.brick(middle, half, Quat::IDENTITY, deck.colour.unwrap_or(GREY), studs(deck.half));
        }
        // The speed pads: a bright patch with arrows up it, as a circuit's are.
        for pad in &self.pads {
            let (along, across) = pad.axes();
            let place = |on: f32, over: f32, lift: f32| {
                ground(pad.at) + along * on + across * over + Vec3::Y * lift
            };
            let (long, wide) = (PAD_LENGTH / 2.0, PAD_WIDTH / 2.0);
            b.quad(
                place(-long, -wide, 0.05),
                place(-long, wide, 0.05),
                place(long, wide, 0.05),
                place(long, -wide, 0.05),
                ORANGE,
            );
            for arrow in 0..3 {
                let back = -long + 0.6 + arrow as f32 * 2.4;
                b.tri(
                    place(back, -wide + 0.8, 0.065),
                    place(back, wide - 0.8, 0.065),
                    place(back + 2.0, 0.0, 0.065),
                    YELLOW,
                );
            }
        }
        // A mark in the middle of the floor, to know it by.
        let spokes = 16;
        for i in 0..spokes {
            let (from, to) = (i as f32 / spokes as f32 * TAU, (i + 1) as f32 / spokes as f32 * TAU);
            let at = |angle: f32, radius: f32| {
                Vec3::new(angle.cos() * radius, 0.02, angle.sin() * radius)
            };
            b.quad(at(from, 12.0), at(from, 13.5), at(to, 13.5), at(to, 12.0), WHITE);
        }
        b.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Track;

    /// How much of an arena's line is in sight from a place on it, taking one
    /// place with another: the share of the pairs of places along it, a car's
    /// length or so above the ground, that nothing stands between.
    fn sight(track: &Track) -> f32 {
        let eye = Vec3::Y * 1.5;
        let places: Vec<Vec3> = track.pts.iter().step_by(5).map(|&p| p + eye).collect();
        let (mut pairs, mut seen) = (0, 0);
        for (i, &from) in places.iter().enumerate() {
            for &to in &places[i + 1..] {
                pairs += 1;
                seen += track.collision.shot(from, to).is_none() as usize;
            }
        }
        seen as f32 / pairs.max(1) as f32
    }

    #[test]
    fn an_arena_is_a_walled_floor_with_room_to_go_anywhere() {
        for map in crate::map::arenas() {
            let (layout, plan) = (&map.key, map.plan.as_ref().expect("an arena's plan"));
            let track = Track::built(map);
            let lift = Vec3::Y * 0.6;
            // The line and the grid are on open floor, well off the wall and the
            // blocks, and there is ground under every part of them.
            // The line may go up a ramp and over a deck: wherever it is, what is
            // under it is about as high as it says.
            for &p in &track.pts {
                // On its way up it is beside whatever the ramp leads onto.
                let clear = if p.y > 1.0 { 0.0 } else { 7.0 };
                assert!(plan.open(p, clear), "{layout} line at {p}");
                let under = track.collision.ground(p + Vec3::Y * 3.0, 8.0);
                let under = under.unwrap_or_else(|| panic!("{layout} nothing under {p}"));
                assert!((under.point.y - p.y).abs() < 1.6, "{layout} {p} over {}", under.point);
            }
            // Its high ground is there to be driven onto and under.
            for deck in &plan.decks {
                let over = ground(deck.at) + Vec3::Y * (deck.height + 2.0);
                let top = track.collision.ground(over, 4.0).expect("a deck's top");
                assert!((top.point.y - deck.height).abs() < 0.1);
                let below = track.collision.ground(ground(deck.at) + Vec3::Y * 2.0, 4.0);
                assert!(below.is_some_and(|floor| floor.point.y.abs() < 0.1), "{layout}");
            }
            for ramp in &plan.ramps {
                let [low_a, low_b, high_b, high_a] = ramp.corners();
                let (foot, head) = ((low_a + low_b) / 2.0, (high_a + high_b) / 2.0);
                let at = |part: f32| foot.lerp(head, part) + Vec3::Y * 12.0;
                let (low, high) = (
                    track.collision.ground(at(0.1), 20.0).unwrap().point.y,
                    track.collision.ground(at(0.9), 20.0).unwrap().point.y,
                );
                assert!(low < 1.0 && high > ramp.height - 1.0, "{layout} {low} {high}");
            }
            assert_eq!(track.course.grid.len(), 6);
            for &(place, facing) in &track.course.grid {
                assert!(plan.open(place, 6.0), "{layout} grid at {place}");
                let floor = track.collision.ground(place + Vec3::Y * 2.0, 4.0);
                assert!(floor.is_some_and(|floor| floor.point.y.abs() < 0.1));
                // Nothing stands right in front of a car on the grid.
                let ahead = place + facing * 12.0;
                assert!(track.collision.wall(place + lift, ahead + lift).is_none());
            }
            // The wall stops what goes at it, all the way round, and so does each
            // block; nothing counts laps.
            for (a, b) in plan.sides() {
                let middle = (a + b) / 2.0;
                let (inside, outside) = (middle * 0.95, middle * 1.05);
                assert!(
                    track.collision.wall(inside + lift, outside + lift).is_some(),
                    "{layout} wall at {middle}"
                );
            }
            for block in &plan.blocks {
                let middle = ground(block.at);
                let beside = middle + Vec3::X * (block.half[0] + 4.0);
                assert!(track.collision.wall(beside + lift, middle + lift).is_some());
                assert!(!plan.open(middle, 0.0));
            }
            assert!(track.course.checkpoints.is_empty() && track.course.zones.is_empty());
            // Its bricks are clear of what stands higher than they lie, and of
            // each other.
            let bricks = crate::items::laid(&track);
            for (i, &(_, spot)) in bricks.iter().enumerate() {
                assert!(plan.open(spot, 2.0), "{layout} brick at {spot}");
                let apart = |other: &(Option<Power>, Vec3)| other.1.distance(spot) > 3.0;
                assert!(bricks[..i].iter().all(apart), "{layout} two bricks at {spot}");
            }
            // Its pads are on open floor, and set off a car on them and no other.
            for pad in &plan.pads {
                let middle = ground(pad.at);
                assert!(plan.open(middle, 4.0), "{layout} pad at {middle}");
                assert!(plan.padded(middle) && !plan.padded(middle + Vec3::Y * 4.0));
                assert!(!plan.padded(middle + pad.axes().0 * PAD_LENGTH));
            }
            // Nowhere on the line is far from a block or the wall, and from nowhere
            // on it is much of the rest of it seen: what stands on the floor hides
            // one way from another.
            for &p in track.pts.iter().filter(|p| p.y < 1.0) {
                assert!(!plan.open(p, 44.0), "{layout} is wide open at {p}");
            }
            let seen = sight(&track);
            println!("{layout}: {:.0}% of the line is in sight from a place on it", seen * 100.0);
            assert!((0.08..0.5).contains(&seen), "{layout} {seen}");
            // Every start is about as far as the rest from the nearest coloured
            // brick, and from the nearest white one, of which there are a few.
            let nearest = |white: bool| -> Vec<f32> {
                let from = |place: Vec3| {
                    let of_kind = bricks.iter().filter(|brick| brick.0.is_none() == white);
                    let far = of_kind.map(|brick| brick.1.with_y(0.0).distance(place));
                    far.fold(f32::MAX, f32::min)
                };
                track.course.grid.iter().map(|&(place, _)| from(place)).collect()
            };
            let spread = |far: &[f32]| {
                let least = far.iter().copied().fold(f32::MAX, f32::min);
                (least, far.iter().copied().fold(0.0, f32::max))
            };
            let (coloured, white) = (spread(&nearest(false)), spread(&nearest(true)));
            assert!(coloured.1 < 30.0 && coloured.1 - coloured.0 < 6.0, "{layout} {coloured:?}");
            assert!(white.0 > 60.0 && white.1 < white.0 * 1.15, "{layout} {white:?}");
            let whites = bricks.iter().filter(|brick| brick.0.is_none()).count();
            assert!((3..=6).contains(&whites), "{layout} {whites}");
            // Turned over, as a mirrored race has it, it is the same place the other
            // way up.
            let over = plan.mirrored();
            for outline in plan.outlines() {
                for p in outline {
                    let flipped = Vec3::new(p.x, 0.0, -p.z);
                    assert_eq!(plan.open(p * 0.9, 0.0), over.open(flipped * 0.9, 0.0));
                }
            }
            assert_eq!(over.mirrored(), *plan);
        }
    }
}
