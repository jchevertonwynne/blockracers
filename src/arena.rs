//! The arenas: places to fight in and not to race round, for `net::modes`' battle and
//! game of tag. The port's own.
//!
//! An arena is a walled floor with no road: a car goes where it likes. Each is laid
//! out as a place to fight over is: a few ways through it that a car must choose
//! between, with buildings between them to break the sight of one from another
//! and gaps to cross by; high ground in the middle of it, reached by ramps, that
//! looks over the ways below and has the best bricks; a way under that for whoever
//! would rather not be seen; and places at its ends for the cars to start from,
//! out of sight of each other. It is built of blocks, which stand on the floor,
//! ramps, and decks, which are held up over it and can be driven under. Each is a
//! map (`map::Map`, a file of `maps/`) with a `Plan` in place of a road. Each still
//! has a line (`Plan::line`), a loop through
//! the open floor that nothing is made to follow: the bricks are laid along it, the
//! computer's cars wander it when they have nobody to go after (`kart::Ai::quarry`),
//! and the hazards are stood by it (`gauntlet::load`). The cars start spread round
//! the floor, facing its middle. There are no gates, lap zones or finishing line, so
//! nothing counts a lap or says a car is going the wrong way.
//!
//! The four the game carries are the same either side of the line east to west;
//! a mirrored race turns the plan over (`Plan::mirrored`), whatever it is.

use crate::assets::materials::Surface;
use crate::collision::Collision;
use crate::meshgen::*;
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

/// A floor held up at `height`, to be driven on and under.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Deck {
    pub at: [f32; 2],
    pub half: [f32; 2],
    pub height: f32,
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
    /// Where the six cars start, as (x, z); each faces the middle.
    pub grid: [[f32; 2]; 6],
    /// Where bricks lie besides those along the line, as (x, z): in the corners
    /// the line doesn't go to, to be gone out of the way for.
    pub bricks: Vec<[f32; 2]>,
    /// The floor's two colours, which alternate outwards from the middle.
    pub floor: [Color; 2],
}

fn ground([x, z]: [f32; 2]) -> Vec3 {
    Vec3::new(x, 0.0, z)
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
            grid: self.grid.map(over),
            bricks: self.bricks.iter().copied().map(over).collect(),
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

    /// Where the cars start and which way each faces: at the middle.
    pub fn grid(&self) -> Vec<(Vec3, Vec3)> {
        self.grid
            .iter()
            .map(|&place| (ground(place), -ground(place).normalize_or(Vec3::X)))
            .collect()
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
        for block in &self.blocks {
            let half = Vec3::new(block.half[0], block.height / 2.0, block.half[1]);
            let middle = ground(block.at) + Vec3::Y * half.y;
            b.brick(middle, half, Quat::IDENTITY, block.colour, (4, 4));
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
            b.brick(middle, half, Quat::IDENTITY, GREY, (4, 8));
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
            // Its other bricks are clear of what stands higher than they lie, and
            // where the line's rows aren't.
            for &spot in &plan.bricks {
                let over = ground(spot) + Vec3::Y * 20.0;
                let spot = track.collision.ground(over, 40.0).expect("ground").point;
                assert!(plan.open(spot, 3.0), "{layout} brick at {spot}");
                assert!(track.pts.iter().all(|p| p.distance(spot) > 5.0), "{layout} {spot}");
            }
            // Nowhere on the line is far from a block or the wall: the ways are
            // narrow.
            for &p in track.pts.iter().filter(|p| p.y < 1.0) {
                assert!(!plan.open(p, 32.0), "{layout} is wide open at {p}");
            }
            // Mirrored it is the same place, and turned over is that place.
            let over = plan.mirrored();
            for outline in plan.outlines() {
                for p in outline {
                    let flipped = Vec3::new(p.x, 0.0, -p.z);
                    assert_eq!(plan.open(p * 0.9, 0.0), plan.open(flipped * 0.9, 0.0));
                    assert_eq!(plan.open(p * 0.9, 0.0), over.open(flipped * 0.9, 0.0));
                }
            }
            assert_eq!(over.mirrored(), *plan);
        }
    }
}
