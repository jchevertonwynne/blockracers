//! The circuit: a closed line resampled to evenly spaced points. Karts, projectiles and
//! pickups all live in "track space" (distance along the track + lateral offset), which
//! keeps collisions with the walls and the hills trivial.
//!
//! The circuits built here are laid out to be driven at the game's speeds: narrow,
//! folded in on themselves so that one stretch runs beside, over or under another,
//! and never straight or level for long, so that little of the road ahead is seen
//! at once. Each is a map (`map::Map`, a file of `maps/`) whose road is a list of
//! corners (`Road`, `cornered`). What hides one stretch from another is the walls
//! at the corners here (`Track::screens`) and `roadside`'s walls and hedges.

use crate::assets::materials::Surface;
use crate::collision::Collision;
use crate::map::Map;
use crate::meshgen::*;
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::f32::consts::TAU;

/// Half-width of the tarmac.
pub const ROAD_HW: f32 = 6.0;
/// Lateral offset of the inner face of the barrier.
pub const WALL: f32 = 8.0;
const KERB: f32 = 0.8;

/// A circuit's road, as its map has it. The corners are each (x, height, z, radius):
/// the road runs straight from each towards the next and is rounded at each to its
/// radius, and a corner with no radius is only a height on the way (`cornered`). The
/// first is the start.
///
/// No corner of the circuits the game carries is tighter than a car going flat out
/// can be slid round (`TIGHTEST`): a hairpin is eighteen or twenty to its middle, and
/// two bends one after the other, one each way, are gentler than that.
///
/// What it has besides is each so far round the lap, but its tunnels and jumps,
/// which are where they are on the ground.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Road {
    pub corners: Vec<[f32; 4]>,
    pub tunnels: Vec<Tunnel>,
    pub jumps: Vec<Jump>,
    pub byways: Vec<Byway>,
    /// Corners with grass inside them and no barrier before it, to be cut across.
    pub infields: Vec<Stretch>,
    pub pads: Vec<Padded>,
    pub banks: Vec<Banked>,
}

/// Where a tunnel begins and ends, as (x, z).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tunnel {
    pub from: [f32; 2],
    pub to: [f32; 2],
}

/// Where the lip of a jump is, as (x, z).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Jump {
    pub lip: [f32; 2],
}

/// Where a stretch of the road begins and ends.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stretch {
    pub from: f32,
    pub to: f32,
}

/// A speed pad: where, and how far right of the middle of the road.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Padded {
    pub at: f32,
    #[serde(default)]
    pub right: f32,
}

/// A banked corner: where the bank begins and ends, and how steep it is across the
/// road at its steepest.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Banked {
    pub from: f32,
    pub to: f32,
    pub tilt: f32,
}

/// How far over the road below a bridge's road must be: more than a launched kart
/// rises, so nothing on the road below reaches the deck.
const DECK: f32 = 8.0;
/// The tightest the middle of a road is turned, but the gauntlet's, which has little
/// that is tight: a car sliding flat out turns in a circle of eighteen
/// (`physics::step`), and has the width of the road besides.
#[cfg(test)]
const TIGHTEST: f32 = 17.5;

/// How far each turn of the helter skelter's spiral is under the one before.
#[cfg(test)]
const SPIRAL_RISE: f32 = 10.5;

/// A way off a circuit's road and back onto it further round, straight from one place
/// on the racing line to another, each so far round the lap: how far its tarmac and
/// its barriers are from its middle, how high the bumps along it are, whether it has
/// a shed over it, and how often one of the computer's cars takes it.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Byway {
    pub from: f32,
    pub to: f32,
    #[serde(default = "Byway::road")]
    pub road: f32,
    #[serde(default = "Byway::wall")]
    pub wall: f32,
    #[serde(default)]
    pub bumps: f32,
    #[serde(default)]
    pub covered: bool,
    #[serde(default = "Byway::taken")]
    pub taken: f32,
}

impl Byway {
    fn road() -> f32 {
        4.0
    }

    fn wall() -> f32 {
        5.5
    }

    fn taken() -> f32 {
        0.3
    }
}

/// How many samples a bank takes to come up to its steepest, and to go down again.
const BANK_EASE: f32 = 12.0;
/// How long and how wide a speed pad is.
pub const PAD_LENGTH: f32 = 8.0;
pub const PAD_WIDTH: f32 = 7.0;
/// How long each bump of a byway is, and how far from the byway's ends they keep, so
/// that it is level where it leaves the road and comes back to it.
const BUMP_LENGTH: f32 = 20.0;
const BUMP_MARGIN: f32 = 30.0;
/// How much the grass inside a corner slows a car, where the verge's is 20.
const LAWN_DRAG: f32 = 30.0;
/// How many samples a lap zone is kept from the ends of a byway or an infield.
const ZONE_CLEAR: usize = 15;
/// How far a byway's surface is over the road's where they share the ground, so that a
/// wheel on both is on the byway.
const BYWAY_LIFT: f32 = 0.01;
/// How far in from a byway's ends its shed begins, how high the shed's roof is and how
/// thick its walls and roof are.
const SHED_INSET: f32 = 12.0;
const SHED_HEIGHT: f32 = 6.0;
const SHED_THICK: f32 = 1.0;

/// A jump, from the foot of its ramp: the road curves up to a lip, drops into a dip,
/// and comes up again to a lower lip with a slope down from it to land on. The dip's
/// sides can be driven up, so a car that falls short gets out, whichever way it is
/// going. These are the lengths of the ramp, of each side of the dip, of its floor and
/// of the landing, and the heights of the two lips.
const JUMP_RAMP: f32 = 24.0;
const JUMP_SIDES: (f32, f32) = (6.0, 6.0);
const JUMP_FLOOR: f32 = 4.0;
const JUMP_LANDING: f32 = 18.0;
const JUMP_LIPS: (f32, f32) = (6.0, 4.5);
/// How long a jump is from end to end.
pub(crate) const JUMP_LENGTH: f32 = JUMP_RAMP + JUMP_SIDES.0 + JUMP_FLOOR + JUMP_SIDES.1 + JUMP_LANDING;

/// How high a jump's road is, `along` it from the foot of the ramp.
fn jump_height(along: f32) -> f32 {
    if along < 0.0 {
        return 0.0;
    }
    if along < JUMP_RAMP {
        return JUMP_LIPS.0 * (along / JUMP_RAMP).powi(2);
    }
    // The stretches after the lip: the length of each, and the heights it runs between.
    let mut left = along - JUMP_RAMP;
    for (length, from, to) in [
        (JUMP_SIDES.0, JUMP_LIPS.0, 0.0),
        (JUMP_FLOOR, 0.0, 0.0),
        (JUMP_SIDES.1, 0.0, JUMP_LIPS.1),
        (JUMP_LANDING, JUMP_LIPS.1, 0.0),
    ] {
        if left < length {
            return from + (to - from) * left / length;
        }
        left -= length;
    }
    0.0
}

/// How many samples round the lap two stretches of road must be from each other to
/// be two stretches, and not the one road going round a corner.
pub(crate) const STRETCH: usize = 30;
/// How near each other two stretches' middles are where the road crosses itself, and
/// how many samples either side of that either road is looked at for the other.
const CROSSING_NEAR: f32 = 1.5;
const CROSSING_SAMPLES: usize = 16;
/// How far apart the checkpoint gates are, about, and how far a gate reaches below
/// the road and above it.
const GATE_SPACING: f32 = 32.0;
const GATE_REACH: (f32, f32) = (2.0, 6.0);
/// How far across a lap zone is.
const ZONE_RADIUS: f32 = WALL + 3.0;

/// A corner tighter than this has a wall inside it, this thick, if the wall would be
/// this many samples long or longer; none stands this near the line. The wall round
/// the outside of a corner is high, so that nothing is seen past the corner; the one
/// inside it is low, so that where the road goes next is seen over it. The rest of
/// what hides one stretch of road from another is `roadside`'s.
pub(crate) const SCREEN_TURN: f32 = 1.0 / 31.0;
pub(crate) const SCREEN_HEIGHT: f32 = 6.0;
pub(crate) const INNER_HEIGHT: f32 = 1.6;
pub(crate) const SCREEN_THICK: f32 = 1.0;
const SCREEN_SHORTEST: usize = 6;
const SCREEN_OFF_LINE: usize = 12;
/// A corner tighter than this has a wall round the outside of it too, with arrows on
/// it the way the corner goes: what is looked at at the end of the straight before.
const BACKDROP_TURN: f32 = 1.0 / 45.0;

/// How many trees and bricks are scattered round a circuit, about, and how far out
/// from its road they go, or from an arena's floor.
const SCATTERED: usize = 520;
const SCATTER_ROUND: f32 = 45.0;
const ARENA_ROUND: f32 = 100.0;
/// A car in an arena is off its line when it is this far to one side of it, or
/// this far above or below it, which is on other ground: under a deck the line goes
/// over, or on one it goes under.
const ARENA_STRAYS: (f32, f32) = (14.0, 3.0);
/// How far above or below a car a sample of an arena's line may be and be on its
/// level; and of the samples that are, nearest first, every which one is looked at
/// for a clear way to it, and how many.
const REJOIN_LEVEL: f32 = 2.5;
const REJOIN_STEP: usize = 4;
const REJOIN_TRIES: usize = 16;
/// How far from a road's barrier one of the scattered bricks keeps.
const BRICK_ROOM: f32 = 14.0;
/// How far from the low wall inside a corner, or from a low wall of `roadside`'s,
/// nothing is scattered: what is seen over the wall is the road beyond it.
pub(crate) const INSIDE_CLEAR: f32 = 13.0;

/// What the walls at the corners are made of: pale and plain.
const CONCRETE: Color = Color::srgb(0.80, 0.80, 0.77);
/// How high over its road the parapet of a road well off the ground is, how high off
/// the ground a road is that begins to have one, and how high each course of it is.
const PARAPET: f32 = 3.0;
const PARAPET_FROM: f32 = 0.8;
const PARAPET_COURSE: f32 = 0.7;

/// How far over the top of its barrier the parapet of a road this high off the
/// ground stands: a course at a time, the higher the road.
pub(crate) fn parapet(raised: f32) -> f32 {
    let courses = ((raised - PARAPET_FROM) * 1.5 / PARAPET_COURSE).floor().max(0.0);
    (courses * PARAPET_COURSE).min(PARAPET - 0.9)
}

/// How high a tunnel's roof is over its road, and how thick its walls and roof are.
const TUNNEL_HEIGHT: f32 = 8.5;
const TUNNEL_THICK: f32 = 1.6;

#[derive(Resource)]
pub struct Track {
    pub pts: Vec<Vec3>,
    /// Unit tangent, including slope.
    pub fwd: Vec<Vec3>,
    /// Unit tangent flattened onto the ground plane.
    pub flat: Vec<Vec3>,
    pub right: Vec<Vec3>,
    /// 1 / turn radius.
    pub curv: Vec<f32>,
    pub spacing: f32,
    pub length: f32,
    /// Half-width of the band around the racing line that AI drivers and the starting
    /// grid spread across.
    pub road: f32,
    pub collision: Collision,
    pub course: Course,
    /// The collision surfaces by name: each one's tag in `collision`, and whether it
    /// starts out passable. Hazards open and close some of them.
    pub surfaces: HashMap<String, (usize, bool)>,
    /// Stretches where part of the road is shut, which the computer's cars keep out of.
    pub lanes: Vec<Lane>,
    /// Ground the scattered scenery is kept off: centres and radii.
    pub clearings: Vec<(Vec3, f32)>,
    /// Where a hazard fires from at whatever comes near, and how far: the
    /// computer's cars in an arena don't go through there after another car.
    pub perils: Vec<(Vec3, f32)>,
    /// The stretches that run through a tunnel: the first sample of each and the one
    /// after its last.
    pub tunnels: Vec<(usize, usize)>,
    /// The stretches that are a jump, from the foot of the ramp to the end of the
    /// landing: the first sample of each and the one after its last.
    pub jumps: Vec<(usize, usize)>,
    /// The ways off the road and back onto it.
    pub branches: Vec<Branch>,
    /// The road is a stand-in: the circuit has no route of its own (the test
    /// track, `world::load_in`), and nothing of the game's is to follow this one.
    pub unrouted: bool,
    /// How it is laid out, if it is an arena and not a circuit: there is then no
    /// road, and the line is only where the computer's cars go when they have
    /// nobody to go after (`arena`).
    pub arena: Option<crate::arena::Plan>,
    /// An arena's ways back to its line over its floor (`arena::Ways`).
    pub ways: Option<crate::arena::Ways>,
    /// How steeply each sample's road is banked: how much higher it is for each unit
    /// to the right of its middle.
    pub bank: Vec<f32>,
    /// The corners with grass inside them and no barrier before it.
    pub infields: Vec<Infield>,
    pub pads: Vec<Pad>,
    /// Where the road crosses itself on the level: the two samples that are in the
    /// same place. Neither road has a barrier across the other there.
    pub crossings: Vec<[usize; 2]>,
    /// How far to the side of the stretch of road it was last on a car may be, and
    /// how far over or under it, before it is looked for round the whole lap. On
    /// the circuits built here that is no further than the next stretch is.
    pub strays: (f32, f32),
    /// The walls that stand inside its tighter corners, to hide what is round them.
    pub screens: Vec<Screen>,
}

/// A wall at a corner, where the barrier is: round the outside of the corner, high
/// enough that nothing beyond it is seen over it, or inside the corner, and low.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Screen {
    /// Its first sample, and the one after its last.
    pub from: usize,
    pub to: usize,
    /// Which side of the road it is: 1 for the right, -1 for the left.
    pub side: f32,
    /// It is round the outside of its corner, and has arrows on it.
    pub outside: bool,
}

impl Screen {
    pub(crate) fn stands(&self, i: usize, side: f32) -> bool {
        (self.from..self.to).contains(&i) && side * self.side > 0.0
    }

    /// How high it stands over the road.
    pub(crate) fn height(&self) -> f32 {
        if self.outside { SCREEN_HEIGHT } else { INNER_HEIGHT }
    }
}

/// A stretch of the road with no barrier on one side of it, and grass beyond from one
/// end of the stretch to the other.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Infield {
    /// Its first sample, and its last.
    pub from: usize,
    pub to: usize,
    /// Which side the grass is: 1 for the right, -1 for the left.
    pub side: f32,
}

impl Infield {
    /// Whether the barrier that would run on from sample `i` on `side` is left out.
    fn opens(&self, i: usize, side: f32) -> bool {
        (self.from..self.to).contains(&i) && side * self.side > 0.0
    }
}

/// A speed pad: the sample its middle is at, and how far right of the middle of the road.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pad {
    pub at: usize,
    pub lat: f32,
}

/// A way off the road and back onto it: a straight strip of its own, with barriers of
/// its own where it is clear of the road, which has none across its ends.
#[derive(Clone)]
pub struct Branch {
    /// Its middle, at even spacing from one end to the other, bumps and all.
    pub pts: Vec<Vec3>,
    /// The way it goes, on the ground, and what is to the right of that.
    pub along: Vec3,
    pub right: Vec3,
    /// How far its tarmac and the inner faces of its barriers are from its middle.
    pub road: f32,
    pub wall: f32,
    /// Whether it has a shed over it.
    pub covered: bool,
    /// The samples of the road it leaves from and comes back to.
    pub ends: [usize; 2],
    /// How often one of the computer's cars takes it.
    pub taken: f32,
}

impl Branch {
    pub fn length(&self) -> f32 {
        (self.pts[self.pts.len() - 1] - self.pts[0]).dot(self.along)
    }

    /// How far along it a place is, and how far to the right of its middle.
    pub fn place(&self, p: Vec3) -> (f32, f32) {
        let from = p - self.pts[0];
        (from.dot(self.along), from.dot(self.right))
    }

    /// Whether a place on its level is between its barriers.
    fn holds(&self, p: Vec3) -> bool {
        let (along, across) = self.place(p);
        let (first, last) = (self.pts[0], self.pts[self.pts.len() - 1]);
        let level = first.y + (last.y - first.y) * along / self.length();
        along > 0.0
            && along < self.length()
            && across.abs() < self.wall
            && (p.y - level).abs() < 3.0
    }

    /// The place on its middle this far along it, which may be past either end.
    pub fn point(&self, along: f32) -> Vec3 {
        let at = (along / self.length()).clamp(0.0, 1.0) * (self.pts.len() - 1) as f32;
        let i = (at as usize).min(self.pts.len() - 2);
        self.pts[i].lerp(self.pts[i + 1], at - i as f32)
            + self.along * (along - along.clamp(0.0, self.length()))
    }

    /// Whether its shed is over the stretch that begins `along` it.
    fn roofed(&self, along: f32) -> bool {
        self.covered && along >= SHED_INSET && along < self.length() - SHED_INSET
    }
}

/// The part of the road that is open between two distances round the lap, as lateral
/// offsets.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lane {
    pub from: f32,
    pub to: f32,
    pub least: f32,
    pub most: f32,
}

/// A gate of the checkpoint graph that orders the racers and guards against shortcuts.
pub struct Checkpoint {
    /// Racers going the right way cross the gate against this.
    pub normal: Vec3,
    pub position: Vec3,
    /// Gates that can follow; the first is the main route.
    pub next: Vec<usize>,
    /// How far round the lap this gate is, 0..1. Gate 0 is at 0.
    pub fraction: f32,
}

/// The race rules' view of a circuit.
#[derive(Default)]
pub struct Course {
    pub checkpoints: Vec<Checkpoint>,
    /// Gate surfaces, tagged with their checkpoint's index.
    pub gates: Collision,
    pub finish: Collision,
    /// Trigger spheres (centre, radius) for the lap zones: a lap only counts if the
    /// kart went through zone 2 and then zone 0 on its way back to the line.
    pub zones: Vec<(Vec3, f32, u8)>,
    /// Starting position and heading per grid slot; empty to line up behind the line.
    pub grid: Vec<(Vec3, Vec3)>,
}

impl Course {
    /// Spreads lap fractions along the main route (following each gate's first
    /// successor from gate 0), then interpolates along alternative branches.
    pub fn compute_fractions(&mut self) {
        let count = self.checkpoints.len();
        for c in &mut self.checkpoints {
            c.fraction = -1.0;
        }
        let mut main = vec![0];
        while let Some(&next) = self.checkpoints[*main.last().unwrap()].next.first() {
            if next == 0 || main.len() >= count {
                break;
            }
            main.push(next);
        }
        for (i, &c) in main.iter().enumerate() {
            self.checkpoints[c].fraction = i as f32 / main.len() as f32;
        }
        for &from in &main {
            for branch in self.checkpoints[from].next.clone().into_iter().skip(1) {
                // Walk the branch until it rejoins gates that already have a fraction.
                let mut path = Vec::new();
                let mut at = branch;
                while self.checkpoints[at].fraction < 0.0 && path.len() < count {
                    path.push(at);
                    let Some(&next) = self.checkpoints[at].next.first() else {
                        break;
                    };
                    at = next;
                }
                let start = self.checkpoints[from].fraction;
                let end = match self.checkpoints[at].fraction {
                    f if f > start => f,
                    _ => 1.0,
                };
                let step = (end - start) / (path.len() + 1) as f32;
                for (i, c) in path.into_iter().enumerate() {
                    self.checkpoints[c].fraction = start + step * (i + 1) as f32;
                }
            }
        }
        for c in &mut self.checkpoints {
            c.fraction = c.fraction.max(0.0);
        }
    }
}

/// A closed spline through `ctrl`, forty points to each of them.
fn splined(ctrl: &[Vec3]) -> Vec<Vec3> {
    let m = ctrl.len();
    let mut dense = Vec::new();
    for c in 0..m {
        for t in 0..40 {
            dense.push(catmull_rom(
                ctrl[(c + m - 1) % m],
                ctrl[c],
                ctrl[(c + 1) % m],
                ctrl[(c + 2) % m],
                t as f32 / 40.0,
            ));
        }
    }
    dense
}

/// How far apart the points of a circuit's line are laid, before it is resampled.
const LINE_STEP: f32 = 0.5;

/// The line through a circuit's corners, each (x, height, z, radius): straight from
/// each corner towards the next, and round each in an arc of its radius that leaves
/// the one straight and joins the other with no kink. A corner's height is that of
/// the middle of its arc, and from one corner to the next the height is eased so
/// that the road has no kink in it either and is never higher or lower than both (a
/// monotone cubic): two corners at one height have level road between them, and a
/// corner higher than those either side of it is a crest. `side` is -1 for the
/// circuit mirrored.
fn cornered(corners: &[[f32; 4]], side: f32) -> Vec<Vec3> {
    let m = corners.len();
    let at = |i: usize| Vec2::new(corners[i % m][0], corners[i % m][2] * side);
    // Each corner's arc: where it begins and ends, and how far round it turns.
    let arcs: Vec<(Vec2, Vec2, f32)> = (0..m)
        .map(|i| {
            let (into, out) = (
                (at(i) - at(i + m - 1)).normalize(),
                (at(i + 1) - at(i)).normalize(),
            );
            let turn = into.perp_dot(out).atan2(into.dot(out));
            let radius = corners[i][3];
            if radius <= 0.0 || turn.abs() < 1e-3 {
                return (at(i), at(i), 0.0);
            }
            let reach = radius * (turn.abs() / 2.0).tan();
            (at(i) - into * reach, at(i) + out * reach, turn)
        })
        .collect();
    // The line on the ground, how far along it each point is, and how far along it
    // the middle of each corner is.
    let (mut line, mut along): (Vec<Vec2>, Vec<f32>) = (Vec::new(), Vec::new());
    let mut middles = Vec::with_capacity(m);
    fn push(line: &mut Vec<Vec2>, along: &mut Vec<f32>, p: Vec2) {
        match line.last() {
            Some(&last) if last.distance(p) < 1e-4 => {}
            Some(&last) => {
                along.push(along[along.len() - 1] + last.distance(p));
                line.push(p);
            }
            None => {
                along.push(0.0);
                line.push(p);
            }
        }
    }
    for i in 0..m {
        let (start, end, turn) = arcs[i];
        if turn == 0.0 {
            push(&mut line, &mut along, start);
            middles.push(along[along.len() - 1]);
        } else {
            let (into, radius) = ((at(i) - at(i + m - 1)).normalize(), corners[i][3]);
            let centre = start + into.perp() * turn.signum() * radius;
            let from = (start - centre).to_angle();
            let steps = ((turn.abs() * radius / LINE_STEP) as usize).max(2);
            for k in 0..=steps {
                let angle = from + turn * k as f32 / steps as f32;
                push(&mut line, &mut along, centre + Vec2::from_angle(angle) * radius);
                if k == steps / 2 {
                    middles.push(along[along.len() - 1]);
                }
            }
        }
        let next = arcs[(i + 1) % m].0;
        let steps = ((end.distance(next) / LINE_STEP) as usize).max(1);
        for k in 1..steps {
            push(&mut line, &mut along, end.lerp(next, k as f32 / steps as f32));
        }
    }
    let total = along[along.len() - 1] + line[line.len() - 1].distance(line[0]);

    // The heights: from each corner's middle to the next's, a cubic with the slopes
    // at its ends that keep it between the two.
    let height = |i: usize| corners[i % m][1];
    let span: Vec<f32> = (0..m)
        .map(|i| (middles[(i + 1) % m] - middles[i]).rem_euclid(total))
        .map(|span| if span > 0.0 { span } else { total })
        .collect();
    let rise: Vec<f32> = (0..m).map(|i| (height(i + 1) - height(i)) / span[i]).collect();
    let slope: Vec<f32> = (0..m)
        .map(|i| {
            let before = (i + m - 1) % m;
            let (a, b) = (rise[before], rise[i]);
            if a * b <= 0.0 {
                return 0.0;
            }
            let (w1, w2) = (2.0 * span[i] + span[before], span[i] + 2.0 * span[before]);
            (w1 + w2) / (w1 / a + w2 / b)
        })
        .collect();
    line.iter()
        .zip(&along)
        .map(|(p, &s)| {
            let mut past = (s - middles[0]).rem_euclid(total);
            let mut i = 0;
            while i + 1 < m && past >= span[i] {
                past -= span[i];
                i += 1;
            }
            let t = (past / span[i]).min(1.0);
            let (t2, t3) = (t * t, t * t * t);
            let y = (2.0 * t3 - 3.0 * t2 + 1.0) * height(i)
                + (t3 - 2.0 * t2 + t) * span[i] * slope[i]
                + (3.0 * t2 - 2.0 * t3) * height(i + 1)
                + (t3 - t2) * span[i] * slope[(i + 1) % m];
            Vec3::new(p.x, y, p.y)
        })
        .collect()
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t)
}

impl Track {
    /// The hill of king of the hill (`net::modes::hill`): the middle of the highest
    /// of the line, which in an arena is the top of its high ground.
    pub fn hill(&self) -> Vec3 {
        let top = self.pts.iter().map(|at| at.y).fold(f32::MIN, f32::max);
        let high: Vec<Vec3> = self.pts.iter().copied().filter(|at| at.y > top - 0.5).collect();
        let middle = high.iter().sum::<Vec3>() / high.len().max(1) as f32;
        let nearest = high
            .iter()
            .min_by(|a, b| a.distance(middle).total_cmp(&b.distance(middle)));
        nearest.copied().unwrap_or_default()
    }

    /// The built-in brick circuit.
    #[cfg(test)]
    pub fn new() -> Self {
        Track::built(crate::map::named("BRICK"))
    }

    /// The road of a map's circuit as it is unmirrored: its line, jumps and banks,
    /// and nothing to drive on.
    pub fn plain(map: &Map) -> Self {
        Track::shaped(map, false)
    }

    /// A map's line and no more, a point every little way, as it is unmirrored or
    /// mirrored: through a circuit's corners, or a spline through an arena's points.
    pub fn line(map: &Map, mirror: bool) -> Self {
        // Mirrored, these are turned over the same way the game's circuits are.
        let side = if mirror { -1.0 } else { 1.0 };
        match &map.plan {
            Some(plan) => {
                let points: Vec<Vec3> = plan
                    .line
                    .iter()
                    .map(|c| Vec3::new(c[0], c[1], c[2] * side))
                    .collect();
                Track::resampled(&splined(&points), crate::arena::LINE_ROAD)
            }
            None => Track::resampled(&cornered(&map.road.corners, side), ROAD_HW),
        }
    }

    /// The line of a map, with its tunnels, jumps and banks.
    fn shaped(map: &Map, mirror: bool) -> Self {
        let (mut track, road) = (Track::line(map, mirror), &map.road);
        if !map.arena() {
            track.strays = (WALL + 4.0, DECK - 1.0);
        }
        let side = if mirror { -1.0 } else { 1.0 };
        track.tunnels = road
            .tunnels
            .iter()
            .map(|tunnel| [tunnel.from, tunnel.to])
            .map(|ends| ends.map(|[x, z]| track.nearest(Vec3::new(x, 0.0, z * side))))
            .map(|[from, to]| (from, to))
            .collect();
        // The spline dips a little before each climb; the road stays on the ground.
        for p in &mut track.pts {
            p.y = p.y.max(0.0);
        }
        // A jump is cut into level road, its lip on the sample nearest where it is put.
        let n = track.n();
        let ramp = (JUMP_RAMP / track.spacing).round() as usize;
        let after = ((JUMP_LENGTH - JUMP_RAMP) / track.spacing).round() as usize;
        track.jumps = road
            .jumps
            .iter()
            .map(|&Jump { lip: [x, z] }| track.nearest(Vec3::new(x, 0.0, z * side)))
            .map(|lip| (lip - ramp, lip + after + 1))
            .collect();
        for (from, to) in track.jumps.clone() {
            for i in from..to {
                // Measured from the lip, so that the lip is as high as it should be.
                let past = (i as f32 - (from + ramp) as f32) * track.spacing;
                track.pts[i].y = jump_height(JUMP_RAMP + past);
            }
        }
        // A banked corner is tipped about its inner edge, which stays where it was:
        // the outside of the corner is the high side.
        for &Banked { from, to, tilt } in &road.banks {
            let (from, to) = (track.index(from), track.index(to));
            let turn: f32 = (from..to)
                .map(|i| track.flat[i].cross(track.flat[i + 1]).y)
                .sum();
            for i in from..=to {
                let ease = ((i - from).min(to - i) as f32 / BANK_EASE).min(1.0);
                let ease = ease * ease * (3.0 - 2.0 * ease);
                track.bank[i] = tilt * ease * turn.signum();
                track.pts[i].y += tilt * ease * WALL;
            }
        }
        track.fwd = (0..n)
            .map(|i| (track.pts[(i + 1) % n] - track.pts[(i + n - 1) % n]).normalize())
            .collect();
        // Where two stretches meet on the level, the nearest two samples of each
        // meeting are the crossing.
        let mut met: Vec<(f32, [usize; 2])> = Vec::new();
        for i in 0..n {
            for k in i + STRETCH..n {
                let apart = xz_dist2(track.pts[i], track.pts[k]);
                if k - i < n - STRETCH
                    && apart < CROSSING_NEAR * CROSSING_NEAR
                    && (track.pts[i].y - track.pts[k].y).abs() < 1.0
                {
                    met.push((apart, [i, k]));
                }
            }
        }
        met.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, pair) in met {
            let known = |crossing: &[usize; 2]| track.apart(crossing[0], pair[0]) < STRETCH;
            if !track.crossings.iter().any(known) {
                track.crossings.push(pair);
            }
        }
        track
    }

    /// How many samples apart two samples are, the shorter way round the lap.
    pub(crate) fn apart(&self, a: usize, b: usize) -> usize {
        let n = self.n();
        ((a + n - b) % n).min((b + n - a) % n)
    }

    /// Whether a place beside sample `i` is on the other road of a level crossing,
    /// within `reach` of its middle.
    fn crossed(&self, i: usize, p: Vec3, reach: f32) -> bool {
        let n = self.n();
        self.crossings.iter().any(|pair| {
            [(pair[0], pair[1]), (pair[1], pair[0])]
                .into_iter()
                .filter(|&(mine, _)| self.apart(i, mine) <= CROSSING_SAMPLES)
                .any(|(_, other)| {
                    (0..=2 * CROSSING_SAMPLES).any(|step| {
                        let k = (other + n + step - CROSSING_SAMPLES) % n;
                        let from = p - self.pts[k];
                        from.dot(self.right[k]).abs() < reach
                            && from.dot(self.flat[k]).abs() <= self.spacing
                    })
                })
        })
    }

    /// Whether nothing on another stretch of the road comes near sample `i`'s: no
    /// other stretch's middle is within `reach` of it and between `below` under it
    /// and `above` over it.
    fn alone(&self, i: usize, reach: f32, below: f32, above: f32) -> bool {
        (0..self.n())
            .filter(|&k| self.apart(i, k) > STRETCH)
            .all(|k| {
                let higher = self.pts[k].y - self.pts[i].y;
                xz_dist2(self.pts[i], self.pts[k]) >= reach * reach
                    || higher <= -below
                    || higher >= above
            })
    }

    /// The sample this far round the lap.
    fn index(&self, s: f32) -> usize {
        (s / self.spacing).round() as usize % self.n()
    }

    /// The place `lat` to the right of the middle of sample `i`'s road, on its bank.
    pub fn edge(&self, i: usize, lat: f32) -> Vec3 {
        self.pts[i] + self.right[i] * lat + Vec3::Y * lat * self.bank[i]
    }

    /// The grass of an infield, as the quads it is laid in: each runs from one side of
    /// the corner to the other, between the places the barrier would have stood.
    pub fn grass(&self, infield: &Infield) -> Vec<[Vec3; 4]> {
        let edge = |i: usize| self.edge(i, infield.side * WALL);
        (0..(infield.to - infield.from) / 2)
            .map(|k| (infield.from + k, infield.to - k))
            .map(|(near, far)| [edge(near), edge(near + 1), edge(far - 1), edge(far)])
            .collect()
    }

    /// A map's circuit or arena, to be driven on, as the race in hand has it.
    pub fn built(map: &Map) -> Self {
        Track::made(map, crate::scenery::mirror())
    }

    /// A map's circuit or arena, mirrored or not.
    pub fn made(map: &Map, mirror: bool) -> Self {
        let mut track = Track::shaped(map, mirror);
        // An arena is its floor, its wall and its blocks, with nothing along the
        // line to keep a car to it or to count it round.
        if let Some(plan) = &map.plan {
            let plan = if mirror { plan.mirrored() } else { plan.clone() };
            plan.collide(&mut track.collision);
            track.strays = ARENA_STRAYS;
            track.course.grid = plan.grid();
            track.clearings.push((Vec3::ZERO, plan.reach() + 12.0));
            track.ways = Some(crate::arena::Ways::found(&track.collision, plan.reach(), &track.pts));
            track.arena = Some(plan);
            return track;
        }
        let (n, extras) = (track.n(), &map.road);
        // A byway runs straight from one place on the line to another, at an even
        // slope but for its bumps from where it leaves the one road to where it
        // meets the other.
        track.branches = extras
            .byways
            .iter()
            .map(|byway| {
                let ends = [track.index(byway.from), track.index(byway.to)];
                let [from, to] = ends.map(|i| track.pts[i]);
                let length = xz_dist2(from, to).sqrt();
                let along = (to - from).with_y(0.0) / length;
                let count = (length / 2.0).round();
                let bumps = ((length - 2.0 * BUMP_MARGIN) / BUMP_LENGTH)
                    .floor()
                    .max(0.0);
                let first = (length - bumps * BUMP_LENGTH) / 2.0;
                let height = |at: f32| {
                    let over = at - first;
                    let bump = if over > 0.0 && over < bumps * BUMP_LENGTH {
                        byway.bumps * 0.5 * (1.0 - (TAU * over / BUMP_LENGTH).cos())
                    } else {
                        0.0
                    };
                    // Across the road at either end it is as high as that road, and
                    // all its climbing is done between the two.
                    let climbed = ((at - WALL) / (length - 2.0 * WALL).max(1.0)).clamp(0.0, 1.0);
                    from.y + (to.y - from.y) * climbed + bump
                };
                Branch {
                    pts: (0..=count as usize)
                        .map(|k| length * k as f32 / count)
                        .map(|at| (from + along * at).with_y(height(at)))
                        .collect(),
                    along,
                    right: Vec3::new(-along.z, 0.0, along.x),
                    road: byway.road,
                    wall: byway.wall,
                    covered: byway.covered,
                    ends,
                    taken: byway.taken,
                }
            })
            .collect();
        // The grass of an infield is on the inside of its corner.
        track.infields = extras
            .infields
            .iter()
            .map(|stretch| (track.index(stretch.from), track.index(stretch.to)))
            .map(|(from, to)| {
                let turn: f32 = (from..to)
                    .map(|i| track.flat[i].cross(track.flat[i + 1]).y)
                    .sum();
                Infield {
                    from,
                    to,
                    side: -turn.signum(),
                }
            })
            .collect();
        track.pads = extras
            .pads
            .iter()
            .map(|pad| Pad {
                at: track.index(pad.at),
                lat: pad.right,
            })
            .collect();
        // Scattered scenery keeps off all of them, and off the map's landmarks.
        track.clearings.extend(crate::landmarks::clearings(&map.scenery));
        for branch in &track.branches {
            for p in branch.pts.iter().step_by(4) {
                track.clearings.push((*p, branch.wall + 7.0));
            }
        }
        for infield in &track.infields {
            for [a, _, _, d] in track.grass(infield) {
                track
                    .clearings
                    .push(((a + d) / 2.0, a.distance(d) / 2.0 + 4.0));
            }
        }

        let (branches, infields) = (track.branches.clone(), track.infields.clone());
        // A wall inside each of the tighter corners and round the outside of each
        // corner, where the road is on the ground with its barrier standing and
        // nothing else is close by: not across a byway's end, an infield or a
        // crossing, in a tunnel, beside a jump or against another stretch of the
        // road.
        let inside = |i: usize| -track.flat[i].cross(track.flat[(i + 1) % n]).y.signum();
        let walled = |i: usize, outside: bool| {
            let side = if outside { -inside(i) } else { inside(i) };
            let barrier = track.edge(i, side * WALL);
            let covered = |stretch: &(usize, usize)| (stretch.0..stretch.1).contains(&i);
            // A banked corner is on the ground for all that its middle is lifted.
            let lifted = track.pts[i].y - track.bank[i].abs() * WALL;
            track.curv[i] > if outside { BACKDROP_TURN } else { SCREEN_TURN }
                && lifted < 0.3
                && track.apart(i, 0) > SCREEN_OFF_LINE
                && !track.tunnels.iter().chain(&track.jumps).any(covered)
                && !branches.iter().any(|branch| branch.holds(barrier))
                && !infields.iter().any(|infield| infield.opens(i, side))
                && !track.crossed(i, barrier, WALL + 2.0)
                && track.alone(i, 2.0 * WALL + 4.0, DECK - 1.0, DECK - 1.0)
        };
        let mut screens = Vec::new();
        for outside in [false, true] {
            let mut i = 0;
            while i < n {
                let from = i;
                while i < n && walled(i, outside) && inside(i) == inside(from) {
                    i += 1;
                }
                if i - from >= SCREEN_SHORTEST {
                    let side = if outside { -inside(from) } else { inside(from) };
                    screens.push(Screen {
                        from,
                        to: i,
                        side,
                        outside,
                    });
                }
                i = i.max(from + 1);
            }
        }
        track.screens = screens.clone();
        let lawns: Vec<[Vec3; 4]> = infields
            .iter()
            .flat_map(|infield| track.grass(infield))
            .collect();
        let mut quads: Vec<([Vec3; 4], Surface)> = Vec::new();
        // Tarmac, verges and the inner faces of the barriers.
        let grass = Surface {
            rolling_resistance: 20.0,
            ..default()
        };
        for i in 0..n {
            let j = (i + 1) % n;
            let mut strip = |from: f32, to: f32, surface: Surface| {
                let corners = [
                    track.edge(i, from),
                    track.edge(i, to),
                    track.edge(j, to),
                    track.edge(j, from),
                ];
                quads.push((corners, surface));
            };
            strip(-ROAD_HW - KERB, ROAD_HW + KERB, Surface::default());
            // At a crossing the other road's tarmac is what lies where the verge was.
            for side in [-1.0, 1.0] {
                let middle = track.edge(i, side * (WALL + ROAD_HW + KERB) / 2.0);
                if !track.crossed(i, middle, WALL) {
                    let (near, far) = (side * (ROAD_HW + KERB), side * WALL);
                    strip(near.min(far), near.max(far), grass);
                }
            }
            // Beside a jump they are as high as a car in the air, and where a wall
            // stands as high as the wall.
            let jump = track.jumps.iter().any(|jump| (jump.0..jump.1).contains(&i));
            for side in [-1.0, 1.0] {
                let wall = screens.iter().find(|screen| screen.stands(i, side));
                let top = Vec3::Y
                    * if jump {
                        JUMP_LIPS.0 + 6.0
                    } else {
                        wall.map_or(3.0, |wall| wall.height().max(3.0))
                    };
                let (mut a, mut b) = (track.edge(i, side * WALL), track.edge(j, side * WALL));
                // There is none across the end of a byway, before an infield, or
                // across the other road of a crossing. One that begins outside a
                // byway's end runs up to the byway's own barrier and stops there,
                // so that the two meet.
                let held = |p: Vec3| branches.iter().any(|branch| branch.holds(p));
                let ends = [held(a), held(b)];
                if ends == [true, true]
                    || infields.iter().any(|infield| infield.opens(i, side))
                    || track.crossed(i, (a + b) / 2.0, WALL)
                {
                    continue;
                }
                if ends[0] != ends[1] {
                    let (mut within, mut without) = if ends[0] { (a, b) } else { (b, a) };
                    for _ in 0..8 {
                        let middle = (within + without) / 2.0;
                        if held(middle) {
                            within = middle;
                        } else {
                            without = middle;
                        }
                    }
                    if ends[0] { a = within } else { b = within }
                }
                quads.push((
                    [a - Vec3::Y, b - Vec3::Y, b + top, a + top],
                    Surface::default(),
                ));
            }
        }
        // The grass inside a corner, which is slower going than the verge.
        let lawn = Surface {
            rolling_resistance: LAWN_DRAG,
            ..default()
        };
        quads.extend(lawns.into_iter().map(|corners| (corners, lawn)));
        // A byway's tarmac from barrier to barrier, and its barriers where they are
        // clear of the road.
        for branch in &branches {
            let across = branch.right * branch.wall;
            let lift = Vec3::Y * BYWAY_LIFT;
            for step in branch.pts.windows(2) {
                let (p, q) = (step[0] + lift, step[1] + lift);
                quads.push((
                    [p - across, p + across, q + across, q - across],
                    Surface::default(),
                ));
                for side in [-1.0, 1.0] {
                    let (mut a, mut b) = (p + across * side, q + across * side);
                    let over = [track.on_road(a, WALL), track.on_road(b, WALL)];
                    if over == [true, true] {
                        continue;
                    }
                    // None of it stands out past the road's own barrier, where a
                    // car going along that barrier would catch on its end; it
                    // begins at that barrier, and leaves no way out between them.
                    if over[0] != over[1] {
                        let (mut on, mut off) = if over[0] { (a, b) } else { (b, a) };
                        for _ in 0..8 {
                            let middle = (on + off) / 2.0;
                            if track.on_road(middle, WALL) {
                                on = middle;
                            } else {
                                off = middle;
                            }
                        }
                        if over[0] { a = on } else { b = on }
                    }
                    let (low, high) = (Vec3::Y, Vec3::Y * 3.0);
                    quads.push(([a - low, b - low, b + high, a + high], Surface::default()));
                }
            }
        }
        for ([a, b, c, d], surface) in quads {
            track.collision.add([a, b, c], surface);
            track.collision.add([a, c, d], surface);
        }

        // Checkpoint gates at even distances round the lap, the first on the start
        // line. One that would stand where a car on another stretch of the road
        // could go through it, at a level crossing, stands a little further on.
        let wanted = (track.length / GATE_SPACING).round() as usize;
        let mut places: Vec<usize> = Vec::new();
        for gate in 0..wanted {
            let mut i = gate * n / wanted;
            // Nor does one stand across a jump, for a car in the air to miss.
            let jumped = |i: usize| track.jumps.iter().any(|jump| (jump.0..jump.1).contains(&i));
            let reach = (GATE_REACH.0 + 1.0, GATE_REACH.1 + 1.0);
            while gate > 0
                && i < n
                && (jumped(i) || !track.alone(i, 2.0 * WALL + 3.0, reach.0, reach.1))
            {
                i += 1;
            }
            if i < n && places.last().is_none_or(|&last| i > last + 4) {
                places.push(i);
            }
        }
        for (gate, &i) in places.iter().enumerate() {
            let (p, r) = (track.pts[i], track.right[i] * (WALL + 1.0));
            let (low, high) = (Vec3::Y * -GATE_REACH.0, Vec3::Y * GATE_REACH.1);
            let corners = [p - r + low, p + r + low, p + r + high, p - r + high];
            for tri in [
                [corners[0], corners[1], corners[2]],
                [corners[0], corners[2], corners[3]],
            ] {
                track.course.gates.add_tagged(tri, Surface::default(), gate);
                if gate == 0 {
                    track.course.finish.add(tri, Surface::default());
                }
            }
            track.course.checkpoints.push(Checkpoint {
                normal: -track.flat[i],
                position: p,
                next: vec![(gate + 1) % places.len()],
                fraction: 0.0,
            });
        }
        track.course.compute_fractions();
        // The lap zones are on road that no byway goes round and no infield is beside,
        // and that no other stretch of the road comes near enough to be in.
        let stretches: Vec<(usize, usize)> = branches
            .iter()
            .map(|branch| (branch.ends[0], branch.ends[1]))
            .chain(infields.iter().map(|infield| (infield.from, infield.to)))
            .collect();
        let mut last = 0;
        for (zone, mut at) in [(2, n / 3), (0, 2 * n / 3)] {
            // Where a byway goes round more than half the lap (the bowl's), the
            // second is half way from the first to the line.
            if at <= last {
                at = last + (n - last) / 2;
            }
            let wanted = at;
            loop {
                let beside = stretches.iter().find(|&&(from, to)| {
                    (from.saturating_sub(ZONE_CLEAR)..to + ZONE_CLEAR).contains(&at)
                });
                if let Some(&(_, to)) = beside {
                    at = to + ZONE_CLEAR;
                } else if at >= n {
                    // A road with nowhere such has it where it was first wanted.
                    at = wanted.min(n - 1);
                    break;
                } else if !track.alone(at, ZONE_RADIUS + WALL + 0.5, ZONE_RADIUS, ZONE_RADIUS) {
                    at += 1;
                } else {
                    break;
                }
            }
            track.course.zones.push((track.pts[at], ZONE_RADIUS, zone));
            last = at;
        }
        track
    }

    /// Fits a closed spline through `ctrl` and resamples it at even spacing.
    pub fn from_loop(ctrl: &[Vec3], road: f32) -> Self {
        Track::resampled(&splined(ctrl), road)
    }

    /// A closed line, resampled at even spacing.
    fn resampled(dense: &[Vec3], road: f32) -> Self {
        let d = dense.len();
        let mut cum = vec![0.0; d + 1];
        for i in 0..d {
            cum[i + 1] = cum[i] + dense[i].distance(dense[(i + 1) % d]);
        }
        let length = cum[d];
        let n = (length / 2.0).round() as usize;
        let spacing = length / n as f32;

        let mut pts = Vec::with_capacity(n);
        let mut j = 0;
        for k in 0..n {
            let target = k as f32 * spacing;
            while cum[j + 1] < target {
                j += 1;
            }
            let t = (target - cum[j]) / (cum[j + 1] - cum[j]);
            pts.push(dense[j].lerp(dense[(j + 1) % d], t));
        }

        let fwd: Vec<Vec3> = (0..n)
            .map(|i| (pts[(i + 1) % n] - pts[(i + n - 1) % n]).normalize())
            .collect();
        let flat: Vec<Vec3> = fwd
            .iter()
            .map(|f| Vec3::new(f.x, 0.0, f.z).normalize())
            .collect();
        let right = flat.iter().map(|f| Vec3::new(-f.z, 0.0, f.x)).collect();
        let curv = (0..n)
            .map(|i| flat[i].angle_between(flat[(i + 1) % n]) / spacing)
            .collect();
        Track {
            pts,
            fwd,
            flat,
            right,
            curv,
            spacing,
            length,
            road,
            collision: Collision::default(),
            course: Course::default(),
            surfaces: HashMap::new(),
            lanes: Vec::new(),
            clearings: Vec::new(),
            perils: Vec::new(),
            tunnels: Vec::new(),
            jumps: Vec::new(),
            branches: Vec::new(),
            unrouted: false,
            arena: None,
            ways: None,
            bank: vec![0.0; n],
            infields: Vec::new(),
            pads: Vec::new(),
            crossings: Vec::new(),
            strays: (40.0, f32::MAX),
            screens: Vec::new(),
        }
    }

    /// The lateral offset nearest `wanted` that is open at distance `s`.
    pub fn lane(&self, s: f32, wanted: f32) -> f32 {
        let s = s.rem_euclid(self.length);
        self.lanes
            .iter()
            .filter(|lane| s >= lane.from && s <= lane.to)
            .fold(wanted, |lat, lane| lat.clamp(lane.least, lane.most))
    }

    pub fn n(&self) -> usize {
        self.pts.len()
    }

    /// Where along the lap the finish line is crossed.
    fn finish_distance(&self) -> f32 {
        let lift = Vec3::Y;
        (0..self.n())
            .find(|&i| {
                self.course
                    .finish
                    .any(self.pts[i] + lift, self.pts[(i + 1) % self.n()] + lift)
                    .is_some()
            })
            .map_or(0.0, |i| i as f32 * self.spacing)
    }

    /// Turns the circuit round, to be raced the other way: the racing line, the
    /// checkpoints and the lap zones are walked backwards, and the grid is put on what
    /// was the far side of the finish line.
    pub fn reverse(&mut self) {
        // Two columns, the same way the grid is drawn up on a circuit without one.
        let finish = self.finish_distance();
        self.course.grid = (0..6)
            .map(|place| {
                // The first place on a grid is the one furthest back.
                let s = finish + 8.0 + ((5 - place) / 2) as f32 * 6.0;
                let lat = self.road * if place % 2 == 0 { 0.375 } else { -0.375 };
                (
                    self.surface_point(s, lat),
                    -self.sample(s).1.with_y(0.0).normalize(),
                )
            })
            .collect();

        // The line: the same samples from the same first one, the other way.
        let n = self.n();
        self.pts[1..].reverse();
        for list in [&mut self.fwd, &mut self.flat, &mut self.right] {
            list[1..].reverse();
            for v in list.iter_mut() {
                *v = -*v;
            }
        }
        self.curv = (0..n)
            .map(|i| self.flat[i].angle_between(self.flat[(i + 1) % n]) / self.spacing)
            .collect();

        // Each gate leads to the ones that led to it, the main route's first.
        let gates = &mut self.course.checkpoints;
        let mut before = vec![Vec::new(); gates.len()];
        for main in [true, false] {
            for (from, gate) in gates.iter().enumerate() {
                for (branch, &to) in gate.next.iter().enumerate() {
                    if (branch == 0) == main && to < before.len() && !before[to].contains(&from) {
                        before[to].push(from);
                    }
                }
            }
        }
        for (gate, next) in gates.iter_mut().zip(before) {
            (gate.normal, gate.next) = (-gate.normal, next);
        }
        if !gates.is_empty() {
            self.course.compute_fractions();
        }
        // A tunnel is entered by what was its way out, and a jump taken from its landing.
        for stretch in self.tunnels.iter_mut().chain(&mut self.jumps) {
            *stretch = (n + 1 - stretch.1, n + 1 - stretch.0);
        }
        // A byway is driven from what was its far end, a bank leans the other way from
        // the new right, and an infield and a pad are on the other side.
        for branch in &mut self.branches {
            branch.pts.reverse();
            (branch.along, branch.right) = (-branch.along, -branch.right);
            branch.ends = [(n - branch.ends[1]) % n, (n - branch.ends[0]) % n];
        }
        self.bank[1..].reverse();
        for bank in &mut self.bank {
            *bank = -*bank;
        }
        for infield in &mut self.infields {
            *infield = Infield {
                from: n - infield.to,
                to: n - infield.from,
                side: -infield.side,
            };
        }
        for pad in &mut self.pads {
            *pad = Pad {
                at: (n - pad.at) % n,
                lat: -pad.lat,
            };
        }
        for crossing in &mut self.crossings {
            *crossing = crossing.map(|i| (n - i) % n);
        }
        for screen in &mut self.screens {
            *screen = Screen {
                from: n - screen.to,
                to: n - screen.from,
                side: -screen.side,
                outside: screen.outside,
            };
        }
        // What was on the right is on the left, and as far from the line the other way.
        for lane in &mut self.lanes {
            *lane = Lane {
                from: self.length - lane.to,
                to: self.length - lane.from,
                least: -lane.most,
                most: -lane.least,
            };
        }
        // The stretch after the line is now the one before it.
        for zone in &mut self.course.zones {
            zone.2 = match zone.2 {
                0 => 2,
                2 => 0,
                other => other,
            };
        }
    }

    /// Interpolated (position, tangent, right) at distance `s`, which may be any real number.
    pub fn sample(&self, s: f32) -> (Vec3, Vec3, Vec3) {
        let x = s.rem_euclid(self.length) / self.spacing;
        let i = x as usize % self.n();
        let j = (i + 1) % self.n();
        let t = x.fract();
        (
            self.pts[i].lerp(self.pts[j], t),
            self.fwd[i].lerp(self.fwd[j], t).normalize(),
            self.right[i].lerp(self.right[j], t).normalize(),
        )
    }

    pub fn point(&self, s: f32, lat: f32) -> Vec3 {
        let (p, _, r) = self.sample(s);
        let x = s.rem_euclid(self.length) / self.spacing;
        let (i, j) = (x as usize % self.n(), (x as usize + 1) % self.n());
        let bank = self.bank[i] + (self.bank[j] - self.bank[i]) * x.fract();
        p + r * lat + Vec3::Y * lat * bank
    }

    /// The point on the driving surface at distance `s` and lateral offset `lat`.
    pub fn surface_point(&self, s: f32, lat: f32) -> Vec3 {
        let p = self.point(s, lat);
        self.collision
            .ground(p + Vec3::Y * 4.0, 12.0)
            .map_or(p, |hit| hit.point)
    }

    /// The sample closest to `pos`, height included: where the road passes over
    /// itself, the level `pos` is on.
    pub fn nearest(&self, pos: Vec3) -> usize {
        (0..self.n())
            .min_by(|&a, &b| {
                self.pts[a]
                    .distance_squared(pos)
                    .total_cmp(&self.pts[b].distance_squared(pos))
            })
            .unwrap()
    }

    /// Where on an arena's line a car that has left it is to make for: the nearest
    /// sample of those on the car's own level that nothing stands between it and;
    /// failing that the nearest on its level, and failing that the nearest.
    pub fn rejoined(&self, pos: Vec3) -> usize {
        let lift = Vec3::Y * crate::physics::BODY_POINT_HEIGHT;
        let mut level: Vec<usize> = (0..self.n())
            .filter(|&i| (self.pts[i].y - pos.y).abs() < REJOIN_LEVEL)
            .collect();
        level.sort_by(|&a, &b| xz_dist2(self.pts[a], pos).total_cmp(&xz_dist2(self.pts[b], pos)));
        // At the car's own height: a place up a ramp is seen over the ramp's side,
        // and is not got at from there.
        let seen = |&&i: &&usize| self.collision.wall(pos + lift, self.pts[i].with_y(pos.y) + lift).is_none();
        let in_sight = level.iter().step_by(REJOIN_STEP).take(REJOIN_TRIES).find(seen);
        in_sight.or(level.first()).copied().unwrap_or_else(|| self.nearest(pos))
    }

    /// Whether a place is on the road proper: within `reach` of the racing line to
    /// either side, on its level.
    pub fn on_road(&self, p: Vec3, reach: f32) -> bool {
        let i = self.nearest(p);
        let from = p - self.pts[i];
        from.dot(self.right[i]).abs() < reach
            && from.dot(self.flat[i]).abs() < 2.0 * self.spacing
            && from.y.abs() < 3.0
    }

    /// Whether each sample's road is carried over another stretch of the circuit, so
    /// that what holds it up has to leave the way underneath clear.
    pub(crate) fn bridged(&self) -> Vec<bool> {
        let reach = (WALL + 4.0).powi(2);
        let edges = [-WALL - 0.8, 0.0, WALL + 0.8];
        (0..self.n())
            .map(|i| {
                self.pts.iter().any(|&below| {
                    self.pts[i].y - below.y > 6.0
                        && edges
                            .iter()
                            .any(|&lat| xz_dist2(self.pts[i] + self.right[i] * lat, below) < reach)
                })
            })
            .collect()
    }

    /// Projects a world position near sample `hint` into track space: (sample index,
    /// distance along the lap, lateral offset).
    pub fn project(&self, pos: Vec3, hint: usize) -> (usize, f32, f32) {
        let n = self.n() as i32;
        let mut best = hint;
        let mut best_d = f32::MAX;
        for o in -10..=10 {
            let i = (hint as i32 + o).rem_euclid(n) as usize;
            let d = xz_dist2(self.pts[i], pos);
            if d < best_d {
                (best, best_d) = (i, d);
            }
        }
        let d = pos - self.pts[best];
        (
            best,
            best as f32 * self.spacing + d.dot(self.flat[best]),
            d.dot(self.right[best]),
        )
    }

    pub fn build_mesh(&self) -> Mesh {
        if let Some(plan) = &self.arena {
            return plan.mesh();
        }
        let mut b = BrickMesh::default();
        let n = self.n();
        let up = Vec3::Y;
        let road = [Color::srgb(0.25, 0.26, 0.28), Color::srgb(0.28, 0.29, 0.31)];
        let bridged = self.bridged();
        let span = |i: usize| bridged[i] || bridged[(i + 1) % n];
        for i in 0..n {
            let j = (i + 1) % n;
            let (p0, p1) = (self.pts[i], self.pts[j]);
            let (r0, r1) = (self.right[i], self.right[j]);
            let at = |lat: f32| (self.edge(i, lat), self.edge(j, lat));
            let strip = |b: &mut BrickMesh, from: f32, to: f32, lift: f32, c: Color| {
                let ((a0, a1), (b0, b1)) = (at(from), at(to));
                let l = up * lift;
                b.quad(a0 + l, b0 + l, b1 + l, a1 + l, c);
            };

            // A jump is striped from end to end.
            let surface = if self.jumps.iter().any(|jump| (jump.0..jump.1).contains(&i)) {
                [YELLOW, BLACK][i % 2]
            } else {
                road[(i / 2) % 2]
            };
            strip(&mut b, -ROAD_HW, ROAD_HW, 0.0, surface);
            // Over a crossing the road has no line down it, and neither road's kerb
            // or verge lies across the other.
            let middle = (p0 + p1) / 2.0;
            if i % 6 < 2 && !self.crossed(i, middle, WALL) {
                strip(&mut b, -0.2, 0.2, 0.02, WHITE);
            }
            let kerb = if (i / 2) % 2 == 0 { RED } else { WHITE };
            for side in [-1.0, 1.0] {
                let beside = middle + (r0 + r1) / 2.0 * side * (ROAD_HW + WALL) / 2.0;
                if self.crossed(i, beside, WALL + 1.0) {
                    continue;
                }
                let (near, far) = (side * ROAD_HW, side * (ROAD_HW + KERB));
                strip(&mut b, near.min(far), near.max(far), 0.01, kerb);
                let (near, far) = (far, side * WALL);
                strip(&mut b, near.min(far), near.max(far), 0.0, LIME);
            }

            let barrier = if (i / 3) % 2 == 0 { WHITE } else { RED };
            for side in [-1.0, 1.0] {
                let (a0, a1) = at(side * (WALL + 0.4));
                let rot = Transform::IDENTITY.looking_to(a1 - a0, up).rotation;
                let mid = (a0 + a1) / 2.0;
                // There is none across the end of a byway, before an infield, or
                // across the other road of a crossing.
                let (e0, e1) = at(side * WALL);
                if self.branches.iter().any(|branch| branch.holds(e0) && branch.holds(e1))
                    || self.infields.iter().any(|infield| infield.opens(i, side))
                    || self.crossed(i, mid, WALL)
                {
                    continue;
                }
                let half = Vec3::new(0.4, 0.45, a0.distance(a1) / 2.0 + 0.05);
                b.cuboid(mid + up * 0.45, half, rot, barrier);
                b.cyl(mid + up * 0.9, 0.25, 0.15, rot, barrier);
                // A wall at a corner, behind the barrier: plain, so that it is the
                // road that is looked at, on a dark footing and under a coping. One
                // round the outside of its corner has arrows along it, the way the
                // corner goes.
                let screen = self.screens.iter().find(|screen| screen.stands(i, side));
                if let Some(screen) = screen {
                    let (b0, b1) = at(side * (WALL + 0.8 + SCREEN_THICK / 2.0));
                    let (mid, long) = ((b0 + b1) / 2.0, a0.distance(a1) / 2.0 + 0.3);
                    let (high, coping) = (screen.height(), 0.5);
                    let footing = (0.4 * high).min(1.0);
                    let body = high - footing - coping;
                    let thick = SCREEN_THICK / 2.0;
                    b.cuboid(
                        mid + up * (footing / 2.0 - 0.5),
                        Vec3::new(thick + 0.1, footing / 2.0 + 0.5, long),
                        rot,
                        DARK_GREY,
                    );
                    b.cuboid(
                        mid + up * (footing + body / 2.0),
                        Vec3::new(thick, body / 2.0, long),
                        rot,
                        CONCRETE,
                    );
                    b.brick(
                        mid + up * (high - coping / 2.0),
                        Vec3::new(thick + 0.15, coping / 2.0, long),
                        rot,
                        GREY,
                        (1, 1),
                    );
                    if screen.outside {
                        // On the face of it, just clear of the wall: a band, and on
                        // every other stretch of the band an arrowhead.
                        let (f0, f1) = at(side * (WALL + 0.74));
                        let on = |along: f32, high: f32| f0.lerp(f1, along) + up * high;
                        let mut face = |corners: [Vec3; 4], colour: Color| {
                            let [p, q, r, t] = corners;
                            b.quad(p, q, r, t, colour);
                            b.quad(t, r, q, p, colour);
                        };
                        let (low, high) = (1.6, 4.8);
                        face([on(0.0, low), on(1.0, low), on(1.0, high), on(0.0, high)], YELLOW);
                        if i % 2 == 0 {
                            let (g0, g1) = at(side * (WALL + 0.68));
                            let on = |along: f32, high: f32| g0.lerp(g1, along) + up * high;
                            let (middle, tip) = ((low + high) / 2.0, 0.95);
                            let arms = [(high - 0.3, middle), (low + 0.3, middle)];
                            for (from, to) in arms {
                                face(
                                    [on(0.05, from), on(0.45, from), on(tip, to), on(tip - 0.4, to)],
                                    BLACK,
                                );
                            }
                        }
                    }
                }

                // Road that is up off the ground has a parapet over its barrier,
                // the higher the road the higher it, up to what the barrier stops:
                // from up there nothing below is seen but what is a long way off,
                // and from below nothing of what is on it.
                let raised = (p0.y + p1.y) / 2.0 - self.bank[i].abs() * WALL;
                let parapet = parapet(raised);
                let covered = |stretch: &(usize, usize)| (stretch.0..stretch.1).contains(&i);
                if parapet > 0.1 && screen.is_none() && !self.tunnels.iter().chain(&self.jumps).any(covered) {
                    b.cuboid(
                        mid + up * (0.9 + parapet / 2.0),
                        Vec3::new(0.3, parapet / 2.0, a0.distance(a1) / 2.0 + 0.05),
                        rot,
                        CONCRETE,
                    );
                }

                // Embankment under raised sections, visible from both sides.
                if (p0.y > 0.05 || p1.y > 0.05) && !span(i) {
                    let (o0, o1) = at(side * (WALL + 0.8));
                    let (g0, g1) = (o0.with_y(-0.1), o1.with_y(-0.1));
                    b.quad(o0, o1, g1, g0, TAN);
                    b.quad(o1, o0, g0, g1, TAN);
                }
            }

            if span(i) && i % 8 == 0 {
                // Where the road under the bridge goes the same way, as a spiral's
                // does, columns stand on its barriers.
                let under = (0..n).find(|&k| {
                    p0.y - self.pts[k].y > 6.0
                        && xz_dist2(p0, self.pts[k]) < 9.0
                        && self.flat[k].dot(self.flat[i]).abs() > 0.95
                });
                if let Some(under) = under {
                    let foot = self.pts[under].y + 1.05;
                    for side in [-1.0, 1.0] {
                        let base = (p0 + r0 * side * (WALL + 0.4)).with_y(foot);
                        b.cyl(base, 0.4, p0.y - 0.85 - foot, Quat::IDENTITY, WHITE);
                    }
                }
            }
            if span(i) {
                // A bridge: a deck under the road, banked as it is, and nothing under that.
                let lean = (up - (r0 + r1) / 2.0 * self.bank[i]).normalize();
                let rot = Transform::IDENTITY.looking_to(p1 - p0, lean).rotation;
                let half = Vec3::new(WALL + 0.8, 0.4, p0.distance(p1) / 2.0 + 0.05);
                b.cuboid((p0 + p1) / 2.0 - lean * 0.45, half, rot, GREY);
            }
            if let Some(&(from, to)) = self.tunnels.iter().find(|t| (t.0..t.1).contains(&i)) {
                // A tunnel: a wall either side and a roof, with a portal at each end.
                let rot = Transform::IDENTITY.looking_to(p1 - p0, up).rotation;
                let (mid, length) = ((p0 + p1) / 2.0, p0.distance(p1) / 2.0 + 0.05);
                let portal = i < from + 2 || i + 2 >= to;
                // Its bricks are two greys that are nearly one: it is not striped.
                let (colour, extra) = match (portal, (i / 4) % 2 == 0) {
                    (true, _) => (YELLOW, 0.6),
                    (false, true) => (GREY, 0.0),
                    (false, false) => (Color::srgb(0.56, 0.58, 0.59), 0.0),
                };
                let across = (r0 + r1).normalize() * (WALL + 0.8 + TUNNEL_THICK / 2.0);
                let wall = Vec3::new(TUNNEL_THICK / 2.0 + extra, TUNNEL_HEIGHT / 2.0, length);
                for side in [-1.0, 1.0] {
                    b.cuboid(
                        mid + across * side + up * (TUNNEL_HEIGHT / 2.0 - 0.1),
                        wall,
                        rot,
                        colour,
                    );
                }
                let roof = Vec3::new(
                    WALL + 0.8 + TUNNEL_THICK + extra,
                    TUNNEL_THICK / 2.0 + extra,
                    length,
                );
                b.brick(
                    mid + up * (TUNNEL_HEIGHT + TUNNEL_THICK / 2.0),
                    roof,
                    rot,
                    colour,
                    (8, 1),
                );
            }
            if span(i) != span((i + n - 1) % n) {
                // Where the embankment stops for the bridge: its end, and a pier at
                // each corner.
                let (a, c) = (p0 - r0 * (WALL + 0.8), p0 + r0 * (WALL + 0.8));
                let (a0, c0) = (a.with_y(-0.1), c.with_y(-0.1));
                b.quad(a, c, c0, a0, TAN);
                b.quad(c, a, a0, c0, TAN);
                let rot = Transform::IDENTITY.looking_to(self.flat[i], up).rotation;
                for corner in [a, c] {
                    let half = Vec3::new(1.2, corner.y / 2.0 + 1.0, 1.2);
                    b.brick(corner.with_y(half.y), half, rot, YELLOW, (2, 2));
                }
            }
        }

        // A crossing is a plate over both roads, with a yellow box on it.
        for &[first, second] in &self.crossings {
            let (along, across) = (self.flat[first], self.flat[second]);
            let reach = (ROAD_HW + KERB) / along.cross(across).y.abs().max(0.2);
            let centre = (self.pts[first] + self.pts[second]) / 2.0;
            let corner = |a: f32, c: f32, lift: f32| centre + along * a + across * c + up * lift;
            // Seen from above, whichever way the two roads cross.
            let turn = along.cross(across).y.signum();
            let plate = |b: &mut BrickMesh, a: [f32; 2], c: [f32; 2], lift: f32, colour: Color| {
                let c = if turn > 0.0 { [c[1], c[0]] } else { c };
                b.quad(
                    corner(a[0], c[0], lift),
                    corner(a[1], c[0], lift),
                    corner(a[1], c[1], lift),
                    corner(a[0], c[1], lift),
                    colour,
                );
            };
            plate(&mut b, [-reach, reach], [-reach, reach], 0.015, road[0]);
            let (line, inset) = (0.35, reach - 1.2);
            for edge in [-inset, inset] {
                plate(&mut b, [-inset - line, inset + line], [edge - line, edge + line], 0.025, YELLOW);
                plate(&mut b, [edge - line, edge + line], [-inset, inset], 0.025, YELLOW);
            }
        }

        // The grass inside the corners that have it, a shade off the verge's.
        let lawn = Color::srgb(0.24, 0.56, 0.26);
        for infield in &self.infields {
            for [a, c, d, e] in self.grass(infield) {
                let lift = up * 0.002;
                let corners = [a + lift, c + lift, d + lift, e + lift];
                // Seen from above, whichever way round the corner goes.
                if (c - a).cross(d - a).y > 0.0 {
                    b.quad(corners[0], corners[1], corners[2], corners[3], lawn);
                } else {
                    b.quad(corners[3], corners[2], corners[1], corners[0], lawn);
                }
            }
        }
        // The speed pads: a bright patch with arrows up it, the way the race is run.
        for pad in &self.pads {
            let s = pad.at as f32 * self.spacing;
            let place = |along: f32, across: f32, lift: f32| {
                self.point(s + along, pad.lat + across) + up * lift
            };
            let (long, wide) = (PAD_LENGTH / 2.0, PAD_WIDTH / 2.0);
            b.quad(
                place(-long, -wide, 0.02),
                place(-long, wide, 0.02),
                place(long, wide, 0.02),
                place(long, -wide, 0.02),
                ORANGE,
            );
            for arrow in 0..3 {
                let back = -long + 0.6 + arrow as f32 * 2.4;
                b.tri(
                    place(back, -wide + 0.8, 0.035),
                    place(back, wide - 0.8, 0.035),
                    place(back + 2.0, 0.0, 0.035),
                    YELLOW,
                );
            }
        }
        // The byways: dirt between barriers of their own, where they are clear of the
        // road, with banks under their bumps.
        let dirt = [Color::srgb(0.58, 0.44, 0.28), Color::srgb(0.63, 0.49, 0.32)];
        for branch in &self.branches {
            for (k, step) in branch.pts.windows(2).enumerate() {
                let (p, q) = (step[0], step[1]);
                let (mid, rot) = (
                    (p + q) / 2.0,
                    Transform::IDENTITY.looking_to(q - p, up).rotation,
                );
                let half = p.distance(q) / 2.0 + 0.05;
                let strip = |b: &mut BrickMesh, from: f32, to: f32, lift: f32, colour: Color| {
                    let (a, c, l) = (branch.right * from, branch.right * to, up * lift);
                    b.quad(p + a + l, p + c + l, q + c + l, q + a + l, colour);
                };
                // The dirt is laid in strips, each only where the road's tarmac isn't.
                const STRIPS: usize = 6;
                let wide = 2.0 * branch.road / STRIPS as f32;
                for lane in 0..STRIPS {
                    let from = -branch.road + wide * lane as f32;
                    if !self.on_road(mid + branch.right * (from + wide / 2.0), ROAD_HW + KERB) {
                        strip(&mut b, from, from + wide, 0.012, dirt[(k / 2) % 2]);
                    }
                }
                let roofed = branch.roofed(branch.place(p).0);
                let portal = roofed != branch.roofed(branch.place(p).0 - 4.0)
                    || roofed != branch.roofed(branch.place(q).0 + 4.0);
                for side in [-1.0, 1.0] {
                    if self.on_road(mid + branch.right * side * branch.wall, WALL) {
                        continue;
                    }
                    let (near, far) = (side * branch.road, side * branch.wall);
                    strip(&mut b, near.min(far), near.max(far), 0.004, LIME);
                    if roofed {
                        // The shed's wall.
                        let out = branch.right * side * (branch.wall + SHED_THICK / 2.0);
                        let colour = if portal { YELLOW } else { RED };
                        b.cuboid(
                            mid + out + up * (SHED_HEIGHT / 2.0),
                            Vec3::new(SHED_THICK / 2.0, SHED_HEIGHT / 2.0, half),
                            rot,
                            colour,
                        );
                        continue;
                    }
                    let barrier = if (k / 3) % 2 == 0 { WHITE } else { RED };
                    let at = mid + branch.right * side * (branch.wall + 0.4);
                    b.cuboid(at + up * 0.45, Vec3::new(0.4, 0.45, half), rot, barrier);
                    b.cyl(at + up * 0.9, 0.25, 0.15, rot, barrier);
                    if p.y > 0.05 || q.y > 0.05 {
                        let out = branch.right * side * (branch.wall + 0.8);
                        let (o0, o1) = (p + out, q + out);
                        let (g0, g1) = (o0.with_y(-0.1), o1.with_y(-0.1));
                        b.quad(o0, o1, g1, g0, TAN);
                        b.quad(o1, o0, g0, g1, TAN);
                    }
                }
                if roofed {
                    let colour = if portal { YELLOW } else { DARK_GREY };
                    b.brick(
                        mid + up * (SHED_HEIGHT + SHED_THICK / 2.0),
                        Vec3::new(branch.wall + SHED_THICK, SHED_THICK / 2.0, half),
                        rot,
                        colour,
                        (4, 1),
                    );
                }
            }
        }

        // Chequered start line and gantry.
        let rot = Transform::IDENTITY.looking_to(self.fwd[0], up).rotation;
        let chequer = |i: i32, row: i32| if (i + row) % 2 == 0 { WHITE } else { BLACK };
        for row in 0..2 {
            for i in -8..8 {
                let c = self.point(row as f32 + 0.5, i as f32 + 0.5) + up * 0.03;
                b.cuboid(c, Vec3::new(0.5, 0.01, 0.5), rot, chequer(i, row));
            }
            for i in -6..6 {
                let c = self.point(0.0, (i as f32 + 0.5) * 2.1) + up * (8.5 + row as f32);
                b.cuboid(c, Vec3::new(1.05, 0.5, 0.4), rot, chequer(i, row));
            }
        }
        for side in [-1.0, 1.0] {
            let c = self.point(0.0, side * (WALL + 1.6)) + up * 5.0;
            b.brick(c, Vec3::new(0.8, 5.0, 0.8), rot, YELLOW, (2, 2));
        }
        b.build()
    }

    /// Brick trees and oversized bricks scattered around the outside of the circuit.
    pub fn build_scenery(&self, map: &Map, rng: &mut Rng) -> Mesh {
        let mut b = BrickMesh::default();
        // The walls and hedges that hide one stretch of road from another, which
        // what is scattered keeps off.
        let fences = crate::roadside::fences(self);
        crate::roadside::draw(self, &map.look, &fences, &mut b);
        // Nor does it stand against a wall at a corner, to show through it, or
        // anywhere inside a corner, which is looked across: each wall's places, and
        // how far from them is kept clear.
        let walls: Vec<(Vec3, f32)> = self
            .screens
            .iter()
            .flat_map(|screen| {
                let clear = if screen.outside { 5.0 } else { INSIDE_CLEAR };
                (screen.from..screen.to)
                    .map(move |i| (self.edge(i, screen.side * (WALL + 1.3)), clear))
            })
            .collect();
        // Over the circuit's own ground and a little way round it; round an arena,
        // whose floor it is kept off, further.
        let (least, most) = match &self.arena {
            Some(plan) => {
                let out = Vec3::splat(plan.reach() + ARENA_ROUND);
                (-out, out)
            }
            None => {
                let ends = (Vec3::MAX, Vec3::MIN);
                let (least, most) = self
                    .pts
                    .iter()
                    .fold(ends, |(least, most), &p| (least.min(p), most.max(p)));
                (least - Vec3::splat(SCATTER_ROUND), most + Vec3::splat(SCATTER_ROUND))
            }
        };
        for _ in 0..SCATTERED {
            let pos = Vec3::new(rng.range(least.x, most.x), 0.0, rng.range(least.z, most.z));
            let clear = self
                .pts
                .iter()
                .map(|&p| xz_dist2(p, pos))
                .fold(f32::MAX, f32::min)
                .sqrt();
            // How far it is to the nearest barrier: nothing is bigger than that.
            let room = clear - WALL - 1.0;
            if room < 1.5
                || fences.iter().any(|fence| fence.shades(pos))
                || walls.iter().any(|&(wall, clear)| xz_dist2(wall, pos) < clear * clear)
                || self
                    .clearings
                    .iter()
                    .any(|&(centre, radius)| xz_dist2(centre, pos) < radius * radius)
            {
                continue;
            }
            let rot = Quat::from_rotation_y(rng.range(0.0, std::f32::consts::TAU));
            // Trees nearly everywhere: a brick only now and then, and only well
            // away from the road, where it is not what a driver looks at.
            if rng.f() < 0.8 || room < BRICK_ROOM {
                let s = rng.range(0.8, 1.7).min(room / 2.5);
                b.cuboid(
                    pos + Vec3::Y * 1.5 * s,
                    Vec3::new(0.6, 1.5, 0.6) * s,
                    rot,
                    BROWN,
                );
                let leaf = rng.pick(&[GREEN, GREEN, LIME]);
                for (k, studs) in [4u32, 3, 2, 1].into_iter().enumerate() {
                    let w = 0.6 * studs as f32 * s;
                    let y = (3.6 + 1.2 * k as f32) * s;
                    b.brick(
                        pos + Vec3::Y * y,
                        Vec3::new(w, 0.6 * s, w),
                        rot,
                        leaf,
                        (studs, studs),
                    );
                }
            } else {
                let s = rng.range(1.0, 2.5).min(room / 4.5);
                let half = Vec3::new(2.0, 1.2, 4.0) * s;
                for level in 0..if rng.f() < 0.3 { 2 } else { 1 } {
                    let colour = rng.pick(&[RED, BLUE, YELLOW, WHITE, ORANGE, GREY]);
                    let y = half.y * (1.0 + 2.0 * level as f32);
                    b.brick(pos + Vec3::Y * y, half, rot, colour, (2, 4));
                }
            }
        }
        crate::landmarks::draw(&map.scenery, &mut b);
        b.build()
    }
}

pub fn xz_dist2(a: Vec3, b: Vec3) -> f32 {
    (a.x - b.x).powi(2) + (a.z - b.z).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barriers_never_fold_and_track_never_touches_itself() {
        for map in crate::map::circuits() {
            let (layout, t) = (map.key.as_str(), Track::built(map));
            let n = t.n();
            // Each takes the computer about as long to lap as the road it replaced,
            // which is less road, there being little of it straight.
            let (shortest, longest) = match layout {
                "BRICK" => (1000.0, 1150.0),
                "FIGURE8" => (1100.0, 1300.0),
                "GAUNTLET" => (1900.0, 2100.0),
                _ => (1450.0, 1650.0),
            };
            assert!(
                t.length > shortest && t.length < longest,
                "{layout} length {}",
                t.length
            );
            // Nowhere is it level and straight for long: the longest such stretch
            // is the one the grid stands on.
            let mut run = 0;
            for i in 0..2 * n {
                let level = (t.pts[(i + 1) % n].y - t.pts[i % n].y).abs() < 0.02;
                run = if t.curv[i % n] < 0.004 && level { run + 1 } else { 0 };
                assert!((run as f32) * t.spacing < 170.0, "{layout} straight to {i}");
            }
            for i in 0..n {
                let j = (i + 1) % n;
                for side in [-1.0, 1.0] {
                    let a = t.pts[i] + t.right[i] * side * (WALL + 0.8);
                    let b = t.pts[j] + t.right[j] * side * (WALL + 0.8);
                    assert!(
                        (b - a).dot(t.fwd[i]) > 0.2,
                        "{layout} barrier folds at {i}"
                    );
                }
                // Any other stretch of the road is far enough off for the barriers of
                // both to stand between them, unless it passes well overhead or
                // underneath, or the two meet at a crossing.
                for k in (0..n).filter(|&k| t.apart(i, k) > STRETCH) {
                    let beside = xz_dist2(t.pts[i], t.pts[k]).sqrt() > 2.0 * (WALL + 0.8) + 0.4;
                    let level = (t.pts[i].y - t.pts[k].y).abs() < DECK - 1.0;
                    let meet = t.crossings.iter().any(|pair| {
                        [(pair[0], pair[1]), (pair[1], pair[0])].into_iter().any(|(a, b)| {
                            t.apart(i, a) <= CROSSING_SAMPLES && t.apart(k, b) <= CROSSING_SAMPLES
                        })
                    });
                    assert!(
                        beside || !level || meet,
                        "{layout} {i} at {} near {k} at {}",
                        t.pts[i],
                        t.pts[k]
                    );
                }
            }
            // No slope is too steep to be driven up either way, and no corner too
            // tight to be slid round flat out.
            let tightest = if layout == "GAUNTLET" { 13.5 } else { TIGHTEST };
            for i in 0..n {
                let step = t.pts[(i + 1) % n] - t.pts[i];
                assert!(step.y.abs() < step.with_y(0.0).length() * 1.1, "{layout} {i}");
                assert!(t.curv[i] * tightest < 1.0, "{layout} {i} turns in {}", 1.0 / t.curv[i]);
            }
        }
    }

    /// A gate is gone through, and a lap zone entered, only by a car on the stretch of
    /// road it is on: not from a stretch beside it, over it, under it or across it.
    #[test]
    fn gates_and_lap_zones_belong_to_one_stretch_of_road() {
        for map in crate::map::circuits() {
            let (layout, t) = (map.key.as_str(), Track::built(map));
            let (n, lift) = (t.n(), Vec3::Y * 0.6);
            let gates = &t.course.checkpoints;
            assert!(gates.len() as f32 > t.length / 45.0, "{layout} {}", gates.len());
            assert_eq!(t.nearest(gates[0].position), 0);
            for pair in gates.windows(2) {
                assert!(t.nearest(pair[0].position) < t.nearest(pair[1].position));
            }
            for k in 0..n {
                for lat in [-WALL + 1.0, 0.0, WALL - 1.0] {
                    let (from, to) = (t.edge(k, lat) + lift, t.edge((k + 1) % n, lat) + lift);
                    if let Some(hit) = t.course.gates.any(from, to) {
                        let at = t.nearest(gates[hit.tag].position);
                        assert!(t.apart(at, k) <= 2, "{layout} gate at {at} met from {k}");
                    }
                }
                for &(centre, radius, _) in &t.course.zones {
                    let at = t.nearest(centre);
                    assert!(
                        t.pts[k].distance(centre) > radius + WALL || t.apart(at, k) < STRETCH,
                        "{layout} zone at {at} entered from {k}"
                    );
                }
            }
            assert_eq!(t.course.zones.len(), 2);
        }
    }

    #[test]
    fn the_figure_eight_crosses_itself_by_a_bridge_and_on_the_level() {
        let t = Track::built(crate::map::named("FIGURE8"));
        let bridged = t.bridged();
        let n = t.n();
        let starts = (0..n)
            .filter(|&i| bridged[i] && !bridged[(i + n - 1) % n])
            .count();
        assert_eq!(starts, 1);
        // On the bridge and under it, the road found is the one the racing line is on,
        // and so is the sample.
        for i in (0..n).filter(|&i| bridged[i]) {
            let s = i as f32 * t.spacing;
            assert!(
                (t.surface_point(s, 0.0).y - t.pts[i].y).abs() < 0.2,
                "deck at {i}"
            );
            assert_eq!(t.nearest(t.pts[i]), i);
            let under = (0..n)
                .find(|&k| t.pts[i].y - t.pts[k].y > 6.0 && xz_dist2(t.pts[i], t.pts[k]) < 4.0);
            if let Some(k) = under {
                assert!(
                    t.surface_point(k as f32 * t.spacing, 0.0).y < 0.5,
                    "road under {i}"
                );
                assert_eq!(t.nearest(t.pts[k]), k);
            }
        }
        // The crossing: the two roads meet squarely, each is driven straight over the
        // other with nothing in the way, and neither has grass lying across the
        // other's tarmac. A little way off each has its barriers again.
        let &[[first, second]] = &t.crossings[..] else {
            panic!("{:?}", t.crossings);
        };
        assert!(t.flat[first].dot(t.flat[second]).abs() < 0.1);
        let lift = Vec3::Y * 0.6;
        for at in [first, second] {
            let (before, after) = (t.pts[at - 12] + lift, t.pts[at + 12] + lift);
            assert!(t.collision.wall(before, after).is_none(), "{at}");
            for side in [-1.0, 1.0] {
                let out = |i: usize| t.edge(i, side * (WALL + 1.0)) + lift;
                assert!(t.collision.wall(t.pts[at] + lift, out(at)).is_none());
                assert!(t.collision.wall(t.pts[at - 10] + lift, out(at - 10)).is_some());
                assert!(t.collision.wall(t.pts[at + 10] + lift, out(at + 10)).is_some());
            }
            for step in 0..24 {
                let s = (at + step - 12) as f32 * t.spacing;
                for lat in [-ROAD_HW + 0.5, 0.0, ROAD_HW - 0.5] {
                    let p = t.point(s, lat);
                    let ground = t.collision.ground(p + Vec3::Y * 2.0, 4.0).unwrap();
                    assert_eq!(ground.surface.rolling_resistance, 0.0, "{at} {step} {lat}");
                }
            }
        }
        // Turned round it is the same crossing.
        let mut back = Track::built(crate::map::named("FIGURE8"));
        back.reverse();
        assert_eq!(back.crossings.len(), 1);
        assert_eq!(back.pts[back.crossings[0][0]], t.pts[first]);
        // No other circuit has one.
        for layout in ["BRICK", "GAUNTLET", "HELTER"] {
            assert!(Track::built(crate::map::named(layout)).crossings.is_empty(), "{layout}");
        }
    }

    #[test]
    fn the_gauntlet_has_two_tunnels() {
        let t = Track::built(crate::map::named("GAUNTLET"));
        let bridged = t.bridged();
        // Each tunnel is a good stretch of level road with nothing over it.
        assert_eq!(t.tunnels.len(), 2);
        for &(from, to) in &t.tunnels {
            assert!((to - from) as f32 * t.spacing > 100.0, "{from}..{to}");
            assert!((from..to).all(|i| t.pts[i].y < 1.0 && !bridged[i]));
        }
        // The viaduct goes over each of the western hairpins.
        let spans = (0..t.n())
            .filter(|&i| bridged[i] && !bridged[(i + t.n() - 1) % t.n()])
            .count();
        assert_eq!(spans, 3);
        // Turned round, the same road is roofed.
        let mut back = Track::built(crate::map::named("GAUNTLET"));
        back.reverse();
        for (&(from, to), &(back_from, back_to)) in t.tunnels.iter().zip(&back.tunnels) {
            assert_eq!(
                (back.pts[back_from], back.pts[back_to - 1]),
                (t.pts[to - 1], t.pts[from])
            );
        }
    }

    #[test]
    fn the_helter_skelter_has_a_spiral_two_tunnels_and_a_jump() {
        let t = Track::built(crate::map::named("HELTER"));
        let n = t.n();
        // A turn of the road has another over it with room to drive under.
        let over = |i: usize| {
            let mut above: Vec<f32> = (0..n)
                .filter(|&k| k.abs_diff(i) > 30 && t.pts[k].y > t.pts[i].y + 1.0)
                .filter(|&k| xz_dist2(t.pts[i], t.pts[k]) < 6.0)
                .map(|k| t.pts[k].y - t.pts[i].y)
                .collect();
            above.sort_by(f32::total_cmp);
            above.dedup_by(|a, b| (*a - *b).abs() < 1.0);
            above
        };
        let stacked: Vec<usize> = (0..n).filter(|&i| !over(i).is_empty()).collect();
        assert!(
            stacked.len() as f32 * t.spacing > 100.0,
            "{}",
            stacked.len()
        );
        assert!(SPIRAL_RISE > TUNNEL_HEIGHT);
        for &i in &stacked {
            for (level, rise) in over(i).into_iter().enumerate() {
                assert!(
                    (rise - SPIRAL_RISE * (level + 1) as f32).abs() < 2.5,
                    "{i} {rise}"
                );
            }
        }
        // Each level of it is the road found from that level.
        for &i in &stacked {
            let s = i as f32 * t.spacing;
            assert!((t.surface_point(s, 0.0).y - t.pts[i].y).abs() < 0.3, "{i}");
            assert_eq!(t.nearest(t.pts[i]), i);
        }
        // The tunnels are on level road with nothing over it.
        assert_eq!(t.tunnels.len(), 2);
        let bridged = t.bridged();
        for &(from, to) in &t.tunnels {
            assert!((to - from) as f32 * t.spacing > 100.0, "{from}..{to}");
            assert!((from..to).all(|i| t.pts[i].y < 0.1 && !bridged[i]));
        }
        // The jump is cut into a straight, level but for itself, with a run up to it
        // and no gate across it for a car in the air to miss.
        let &[(from, to)] = &t.jumps[..] else {
            panic!("{:?}", t.jumps);
        };
        let lip = (from..to)
            .max_by(|&a, &b| t.pts[a].y.total_cmp(&t.pts[b].y))
            .unwrap();
        assert_eq!(t.pts[lip].y, JUMP_LIPS.0);
        assert!(t.pts[lip + 3].y < 0.01 && t.pts[lip + 5].y < 0.01);
        assert!(t.pts[from].y < 0.01 && t.pts[to - 1].y < 0.3 && t.pts[to].y < 0.01);
        assert!(
            (from - 20..to + 10)
                .all(|i| t.curv[i] < 1e-3 && t.tunnels.iter().all(|t| !(t.0..t.1).contains(&i)))
        );
        assert!((from - 20..from).all(|i| t.pts[i].y < 0.1));
        for gate in &t.course.checkpoints {
            assert!(!(from..to).contains(&t.nearest(gate.position)));
        }
        // No side of the dip is too steep to be driven up.
        for i in from..to {
            let step = t.pts[i + 1] - t.pts[i];
            assert!(step.y.abs() < step.with_y(0.0).length() * 1.1, "{i}");
        }
        // Turned round, the same road is the jump.
        let mut back = Track::built(crate::map::named("HELTER"));
        back.reverse();
        let (back_from, back_to) = back.jumps[0];
        assert_eq!(
            (back.pts[back_from], back.pts[back_to - 1]),
            (t.pts[to - 1], t.pts[from])
        );
    }

    #[test]
    fn byways_leave_the_road_and_come_back_to_it() {
        for map in crate::map::circuits() {
            let (layout, t) = (map.key.as_str(), Track::built(map));
            let (n, lift) = (t.n(), Vec3::Y * 0.6);
            let byways = if layout == "HELTER" {
                2
            } else {
                1
            };
            let crossing = |i: usize, gap: Vec3| t.crossed(i, gap, WALL);
            assert_eq!(t.branches.len(), byways, "{layout}");
            for branch in &t.branches {
                // Each runs between two places on the line, and is the shorter way.
                let (first, last) = (branch.pts[0], branch.pts[branch.pts.len() - 1]);
                let [from, to] = branch.ends;
                assert_eq!([t.nearest(first), t.nearest(last)], [from, to]);
                assert!(t.pts[from].distance(first) < 0.01 && t.pts[to].distance(last) < 0.01);
                let round = (to - from) as f32 * t.spacing;
                assert!(
                    to > from && branch.length() < round - 10.0,
                    "{layout} {round}"
                );
                // Nothing stands in it from end to end, there is ground under all of
                // it, and it has a barrier either side where it is clear of the road.
                for step in branch.pts.windows(2) {
                    assert!(t.collision.wall(step[0] + lift, step[1] + lift).is_none());
                    let ground = t.collision.ground(step[0] + Vec3::Y * 2.0, 4.0).unwrap();
                    let over = ground.point.y - step[0].y;
                    assert!((-0.02..1.5).contains(&over), "{layout} {over}");
                }
                let middle = branch.pts[branch.pts.len() / 2];
                for side in [-1.0, 1.0] {
                    let out = middle + branch.right * side * (branch.wall + 1.0);
                    assert!(t.collision.wall(middle + lift, out + lift).is_some());
                }
                // It keeps clear of the rest of the road, tunnels and jumps too.
                assert!(!t.on_road(middle, WALL + branch.wall), "{layout}");
                for stretch in t.tunnels.iter().chain(&t.jumps) {
                    assert!(!(stretch.0..stretch.1).contains(&from));
                    assert!(!(stretch.0..stretch.1).contains(&to));
                }
            }
            // The road's own barriers stand everywhere but across the byways' ends
            // and before the infields.
            let mut open = 0;
            for i in 0..n {
                for side in [-1.0, 1.0] {
                    let (mid, out) = (t.edge(i, 0.0), t.edge(i, side * (WALL + 1.0)));
                    if t.collision.wall(mid + Vec3::Y, out + Vec3::Y).is_none() {
                        open += 1;
                        let gap = t.edge(i, side * WALL);
                        assert!(
                            t.branches.iter().any(|b| b.holds(gap))
                                || t.infields.iter().any(|f| f.opens(i, side))
                                || crossing(i, gap),
                            "{layout} {i} {side}"
                        );
                    }
                }
            }
            assert!(open > 4 && open < 200, "{layout} {open}");
            // Neither lap zone is on road that can be gone round, or near its ends.
            let stretches = t
                .branches
                .iter()
                .map(|b| (b.ends[0], b.ends[1]))
                .chain(t.infields.iter().map(|f| (f.from, f.to)));
            for (from, to) in stretches {
                for &(centre, ..) in &t.course.zones {
                    let at = t.nearest(centre);
                    assert!(at + 10 < from || at > to + 10, "{layout} zone at {at}");
                }
            }
            // Turned round, each byway is the same road from its other end.
            let mut back = Track::built(map);
            back.reverse();
            for (branch, turned) in t.branches.iter().zip(&back.branches) {
                assert_eq!(turned.pts[0], branch.pts[branch.pts.len() - 1]);
                assert_eq!(back.nearest(turned.pts[0]), turned.ends[0]);
                assert!(turned.ends[0] < turned.ends[1]);
                assert!(turned.along.dot(branch.along) < -0.999);
            }
        }
    }

    #[test]
    fn infields_pads_and_banks_are_where_the_road_can_take_them() {
        for map in crate::map::circuits() {
            let (layout, t) = (map.key.as_str(), Track::built(map));
            let (n, lift) = (t.n(), Vec3::Y * 0.6);
            let mut back = Track::built(map);
            back.reverse();
            assert!(t.pads.len() >= 2, "{layout}");
            for (infield, turned) in t.infields.iter().zip(&back.infields) {
                // The grass is slow going, inside the corner, clear of the road, and
                // nothing stands between it and the road.
                let grass = t.grass(infield);
                let middle = (infield.from + infield.to) / 2;
                for &[a, _, _, d] in &grass {
                    let on = (a + d) / 2.0;
                    let ground = t.collision.ground(on + Vec3::Y * 2.0, 4.0).unwrap();
                    assert!(ground.surface.rolling_resistance >= LAWN_DRAG, "{layout}");
                    assert!(
                        a.distance(d) < 2.0 || !t.on_road(on, WALL - 1.0),
                        "{layout} {on}"
                    );
                    assert!(t.collision.wall(t.pts[middle] + lift, on + lift).is_none());
                }
                let inside = t.edge(middle, infield.side * (WALL + 2.0));
                let outside = t.edge(middle, -infield.side * (WALL + 2.0));
                assert!(
                    t.collision
                        .wall(t.pts[middle] + lift, inside + lift)
                        .is_none()
                );
                assert!(
                    t.collision
                        .wall(t.pts[middle] + lift, outside + lift)
                        .is_some()
                );
                // Turned round it is the same grass.
                assert_eq!(
                    back.edge(turned.from, turned.side * WALL),
                    t.edge(infield.to, infield.side * WALL)
                );
                for stretch in t.tunnels.iter().chain(&t.jumps) {
                    assert!(stretch.1 < infield.from || stretch.0 > infield.to);
                }
            }
            // A pad lies on the road, the same place either way round.
            for (pad, turned) in t.pads.iter().zip(&back.pads) {
                assert!(pad.lat.abs() + PAD_WIDTH / 2.0 <= ROAD_HW);
                let (here, there) = (t.edge(pad.at, pad.lat), back.edge(turned.at, turned.lat));
                assert!(here.distance(there) < 0.01);
            }
            // A bank's high side is the outside of its corner, and a car's wheels
            // find it.
            let banked: Vec<usize> = (0..n).filter(|&i| t.bank[i] != 0.0).collect();
            assert!(banked.len() > 30, "{layout}");
            for &i in &banked {
                let turn = t.flat[i].cross(t.flat[(i + 1) % n]).y;
                assert!(turn * t.bank[i] >= 0.0, "{layout} {i}");
                let high = t.bank[i].signum() * (ROAD_HW - 1.0);
                let s = i as f32 * t.spacing;
                let (up, down) = (t.surface_point(s, high), t.surface_point(s, -high));
                assert!((up.y - t.edge(i, high).y).abs() < 0.1, "{layout} {i}");
                assert!(up.y >= down.y && back.bank[n - i] == -t.bank[i]);
            }
        }
    }

    /// The walls at the corners: one inside each tight corner that is on the ground
    /// and one round the outside of it, as high as hides the road beyond, solid to
    /// that height, and nowhere near the line, a crossing or a byway's mouth.
    #[test]
    fn walls_stand_inside_and_round_the_tight_corners() {
        for map in crate::map::circuits() {
            let (layout, t) = (map.key.as_str(), Track::built(map));
            let n = t.n();
            let (inside, outside): (Vec<&Screen>, Vec<&Screen>) =
                t.screens.iter().partition(|screen| !screen.outside);
            assert!(inside.len() >= 2 && outside.len() >= 4, "{layout} {:?}", t.screens);
            for screen in &t.screens {
                let middle = (screen.from + screen.to) / 2;
                // Which side the corner turns to is the inside of it.
                let turn: f32 = (screen.from..screen.to)
                    .map(|i| t.flat[i].cross(t.flat[(i + 1) % n]).y)
                    .sum();
                let within = -turn.signum() * screen.side > 0.0;
                assert_eq!(within, !screen.outside, "{layout} {screen:?}");
                // The one round the outside is high, and a car thrown as high as
                // its top is still stopped by it; the one inside is low enough to
                // be seen over from behind a car, and stops no more than a barrier.
                assert_eq!(screen.height() > 3.6, screen.outside);
                let out = t.edge(middle, screen.side * (WALL + 1.0));
                let stopped = |high: f32| {
                    let lift = Vec3::Y * high;
                    t.collision.wall(t.pts[middle] + lift, out + lift).is_some()
                };
                assert!(stopped(screen.height() - 0.5), "{layout} {screen:?}");
                assert_eq!(stopped(SCREEN_HEIGHT - 0.5), screen.outside, "{layout} {screen:?}");
                assert!(t.apart(screen.from, 0) > SCREEN_OFF_LINE && t.apart(screen.to, 0) > 8);
                for i in screen.from..screen.to {
                    let barrier = t.edge(i, screen.side * WALL);
                    assert!(!t.branches.iter().any(|branch| branch.holds(barrier)));
                    assert!(!t.crossed(i, barrier, WALL));
                }
            }
            // Turned round, each is the same wall, on what is then the other side.
            let mut back = Track::built(map);
            back.reverse();
            for (screen, turned) in t.screens.iter().zip(&back.screens) {
                assert_eq!((turned.side, turned.outside), (-screen.side, screen.outside));
                assert_eq!(back.pts[turned.from], t.pts[screen.to]);
            }
        }
    }

    #[test]
    fn a_reversed_circuit_is_the_same_road_the_other_way() {
        for map in crate::map::circuits() {
            let layout = map.key.as_str();
            let (forward, mut back) = (Track::built(map), Track::built(map));
            back.reverse();
            let n = forward.n();
            assert_eq!(back.pts[0], forward.pts[0]);
            for i in 1..n {
                assert_eq!(back.pts[i], forward.pts[n - i]);
                assert!(back.fwd[i].dot(forward.fwd[n - i]) < -0.999);
                assert!((back.pts[(i + 1) % n] - back.pts[i]).dot(back.flat[i]) > 0.0);
            }
            // The gates run the other way round the lap, each against its old self.
            let (was, now) = (&forward.course.checkpoints, &back.course.checkpoints);
            assert_eq!(now[0].next, [was.len() - 1]);
            assert_eq!(now[1].next, [0]);
            assert!((now[1].fraction - was[was.len() - 1].fraction).abs() < 1e-6);
            assert!(now.iter().zip(was).all(|(a, b)| a.normal == -b.normal));
            // The grid waits before the line, facing it, on what was the way out.
            for &(position, facing) in &back.course.grid {
                let (_, s, _) = back.project(position, back.nearest(position));
                assert!(s > back.length - 30.0, "{layout} grid at {s}");
                assert!(facing.dot(back.flat[back.nearest(position)]) > 0.95);
            }
        }
    }

    #[test]
    fn projection_round_trips() {
        let t = Track::new();
        for (s, lat) in [(10.0, 3.0), (500.3, -7.5), (t.length - 1.0, 0.0)] {
            let p = t.point(s, lat);
            let (_, s2, lat2) = t.project(p, t.nearest(p));
            let ds = (s2 - s + t.length / 2.0).rem_euclid(t.length) - t.length / 2.0;
            assert!(
                ds.abs() < 0.3 && (lat2 - lat).abs() < 0.3,
                "{s} {lat} -> {s2} {lat2}"
            );
        }
    }
}
