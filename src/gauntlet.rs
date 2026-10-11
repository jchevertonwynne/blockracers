//! The gauntlet: a circuit of the port's own that runs past as many of the original's
//! hazards as can be moved. Each is borrowed from the circuit it belongs to (its models,
//! its animation, its emitters) and stood on a stretch of a brick-built road, turned
//! so that the stretch of its own road it was made for lies along ours. The hazards
//! themselves are `hazards`' and behave as they do at home. The road is eight rows
//! one after another, a hairpin from each to the next, with a hazard in the middle of
//! most rows and another at one end, a bend between them, two of the hairpins in
//! tunnels and a viaduct back over the others to the start, all laid out in `track`,
//! which gives each hazard no more road than it wants. Whatever stands beside a
//! hazard's road has to fit between that row and the next.
//!
//! What the original circuits' own models held up is built of bricks here: the
//! hammer's frame, the plinths of the crane, the pillar and the ark, the cannons, the
//! lava's pools and the warp pad. The force field and the barrels shut the right half
//! of the road rather than a short cut, there being none, with bricks behind them. The
//! hazards make no sound: their sounds are in their own circuits' banks, which number
//! them differently.
//!
//! Left out: the sphinx, whose face is flat pictures set into the desert's own rock,
//! and the code puzzle, whose doors want a short cut to open onto.
//!
//! Which of them a circuit has, and where, is its map's to say (`Stood`, the
//! `hazards` of a file of `maps/`): one of each at the most, each so far round the
//! lap, or at a place on the ground, which is all an arena has. The other circuits
//! of the port's own have a few of the same hazards each, stood the same way: the
//! brick circuit the hammer, the ghost and the cannons; the figure of eight the
//! curse, the stones and the dragon; the helter skelter the ark, the hammer before
//! its jump, the cannons and the stones.
//!
//! Without the game's data there is nothing to borrow, and each is a plain circuit.

use crate::assets::materials::Surface;
use crate::events::TrackEvents;
use crate::hazards::{Hazards, Kind, Places};
use crate::items::{BRICK_HEIGHT, Power};
use crate::meshgen::*;
use crate::physics::UNIT;
use crate::scenery::{mirror, to_world};
use crate::map::Map;
use crate::track::{Lane, Track, WALL};
use crate::world::{self, LoadedWorld};
use bevy::prelude::*;
use serde::Deserialize;
use std::f32::consts::TAU;

/// The hazards there are to stand.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    Hammer,
    Stones,
    Cannons,
    Ghost,
    Dragon,
    Ark,
    Curse,
    Saucer,
    Lava,
    Field,
    Pillar,
    Barrels,
    Crane,
    Warp,
    Smoke,
    Water,
}

/// A hazard and where a map stands it: `at` so far round the lap and `right` of the
/// middle of the road, or at a `place` on the ground, (x, z), with the road it was
/// made for taken to run `heading` degrees round from east to north.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stood {
    pub kind: Sort,
    pub at: Option<f32>,
    #[serde(default)]
    pub right: f32,
    pub place: Option<[f32; 2]>,
    #[serde(default)]
    pub heading: f32,
    /// How near a car must come for the dragon's cannonballs to begin.
    pub reach: Option<f32>,
    /// How far each of the stones is round the lap from the one before.
    pub apart: Option<f32>,
    /// The three places a curse may be left, each so far round the lap and so far
    /// right of the middle of the road (`Curse`).
    pub drops: Option<[[f32; 2]; 3]>,
    /// How far round the lap a warp pad's car comes out.
    pub out: Option<f32>,
}

/// How far apart the stones are, and how near the dragon is come, where a map
/// doesn't say.
const STONES_APART: f32 = 20.0;
const DRAGON_REACH: f32 = 70.0;

/// What each hazard takes from the race it belongs to: models, then emitters. In
/// the order of the races.
const BORROWS: [(Sort, &str, &[&str], &[&str]); 13] = [
    (Sort::Hammer, "RACEC0R0", &["rkhamm02"], &[]),
    (Sort::Crane, "RACEC0R1", &["crane"], &[]),
    (Sort::Barrels, "RACEC0R1", &["block", "barrel", "barrel01", "barrel02"], &[]),
    (Sort::Pillar, "RACEC0R2", &["piltop"], &[]),
    (Sort::Lava, "RACEC0R3", &["mmlavbl"], &["lavasmk"]),
    (Sort::Field, "RACEC0R3", &["mmrocon", "mmrocof"], &[]),
    (Sort::Ghost, "RACEC1R0", &["ghostly"], &["ghsttrl"]),
    (Sort::Water, "RACEC1R1", &["water01"], &[]),
    (Sort::Smoke, "RACEC1R3", &["dp_def"], &["smoke", "snow"]),
    (Sort::Dragon, "RACEC2R0", &["prntdum"], &[]),
    (Sort::Ark, "RACEC2R2", &["arktp"], &[]),
    (Sort::Stones, "RACEC2R2", &["dum02", "dum03", "dum04"], &[]),
    (Sort::Saucer, "RACEC2R3", &["ufofly1", "beamdumy"], &[]),
];

/// What a map's hazards take between them, race by race: models, then emitters.
fn borrowed(hazards: &[Stood]) -> Vec<(&'static str, Vec<&'static str>, Vec<&'static str>)> {
    let mut taken: Vec<(&str, Vec<&str>, Vec<&str>)> = Vec::new();
    for (sort, race, models, emitters) in BORROWS {
        if !hazards.iter().any(|stood| stood.kind == sort) {
            continue;
        }
        if taken.last().is_none_or(|last| last.0 != race) {
            taken.push((race, Vec::new(), Vec::new()));
        }
        let last = taken.last_mut().unwrap();
        last.1.extend(models);
        last.2.extend(emitters);
    }
    taken
}

impl Stood {
    /// Why a map can't have it as it is: `lap` is how long its road is, if it has
    /// one.
    pub fn amiss(&self, lap: Option<f32>) -> Option<String> {
        let name = format!("{:?}", self.kind).to_lowercase();
        // Those that shut a lane, are left along the road or come out further
        // round it want a road to be on.
        let roaded = matches!(
            self.kind,
            Sort::Stones | Sort::Curse | Sort::Field | Sort::Pillar | Sort::Barrels | Sort::Warp
        );
        let within = |s: f32| lap.is_some_and(|lap| (0.0..lap).contains(&s));
        match (self.at, self.place, lap) {
            (Some(_), Some(_), _) | (None, None, _) => {
                return Some(format!("the {name} wants `at` or `place`, one of them"));
            }
            (Some(_), _, None) => {
                return Some(format!("the {name} is `at` a distance round a road, and an arena has none"));
            }
            (None, ..) if roaded => return Some(format!("the {name} wants a road to be `at`")),
            (Some(at), ..) if !within(at) => {
                return Some(format!("the {name} is at {at}, which is not on the lap"));
            }
            _ => {}
        }
        let rounds = self.drops.iter().flatten().map(|drop| drop[0]).chain(self.out);
        if let Some(off) = rounds.into_iter().find(|&s| !within(s)) {
            return Some(format!("the {name} has {off}, which is not on the lap"));
        }
        match self.kind {
            Sort::Curse if self.drops.is_none() => Some("the curse wants its three `drops`".into()),
            Sort::Warp if self.out.is_none() => Some("the warp wants where it comes `out`".into()),
            _ => None,
        }
    }
}

// The events that set each hazard going. None is one the hazards themselves give out.
const HAMMER: i32 = 101;
const ROCKS: i32 = 102;
const UFO: i32 = 105;
const CRANE: i32 = 106;
const WARP: i32 = 107;
const CANNONS: i32 = 108;
const LAVA: i32 = 110;
const FIELD: i32 = 111;
const SMOKE: i32 = 112;
const SNOW: i32 = 113;
const GHOST: i32 = 114;
const DRAGON: i32 = 115;
const BARRELS: i32 = 116;
const PILLAR: i32 = 117;
const ARK: i32 = 118;
const CURSE: i32 = 119;
/// What a cannon ball landing on the road sets off: nothing.
const LANDED: i32 = 120;
/// Sounding the horn.
const HORN: i32 = 999;

// Tags of the collision surfaces that hazards open and close.
const FIELD_WALL: usize = 1;
const BARREL_WALL: usize = 2;
const PILLAR_WALL: usize = 3;

/// How far a shut lane runs on behind its gate, and how far ahead of the gate the
/// computer's cars leave it.
const LANE_LENGTH: f32 = 35.0;
const LANE_WARNING: f32 = 60.0;
/// Where a circuit's curse is: how far round the lap the place is that sets it off,
/// and how far round and how far right of the middle of the road each of the three
/// places it may be left. Half way round the lap, where the cars are no longer
/// bunched as they are off the line, with the three places far enough on, and
/// straight enough on, to be seen in time to be steered round. Every car that
/// comes sets off another, so there may be one in each place at once: all three
/// are on the one side of the road, and the other side is the way by.
type Curse = (f32, [(f32, f32); 3]);
/// How far round the place that sets a curse off a car is that does.
const CURSE_REACH: f32 = 12.0;
/// How far from the middle of the road the legs of the hammer's frame stand and
/// the cannons' muzzles are.
const FRAME_OUT: f32 = WALL + 3.0;
const CANNON_OUT: f32 = WALL + 5.0;

/// The brick-built parts of the gauntlet.
#[derive(Resource)]
pub struct Stands(pub Mesh);

/// A place on the unmirrored circuit, in the game's coordinates, and the way the road
/// runs there.
#[derive(Clone, Copy)]
struct Site {
    origin: Vec3,
    heading: f32,
    /// How far round the lap it is.
    s: f32,
}

impl Site {
    /// A place on the ground that is on no road, as an arena's are: (`x`, `z`) of
    /// ours, facing so far round from east.
    fn place(x: f32, z: f32, heading: f32) -> Site {
        Site {
            origin: Vec3::new(x, -z, 0.0) / UNIT,
            heading,
            s: 0.0,
        }
    }

    /// The road `s` round the lap, `lat` to the right of its middle.
    fn along(track: &Track, s: f32, lat: f32) -> Site {
        Site::sample(track, (s / track.spacing).round() as usize % track.n(), lat)
    }

    fn sample(track: &Track, i: usize, lat: f32) -> Site {
        let (at, flat) = (track.pts[i] + track.right[i] * lat, track.flat[i]);
        Site {
            origin: Vec3::new(at.x, -at.z, at.y) / UNIT,
            heading: (-flat.z).atan2(flat.x),
            s: i as f32 * track.spacing,
        }
    }

    /// The point this far on, to the right and up, in metres.
    fn at(&self, along: f32, right: f32, up: f32) -> Vec3 {
        let (sin, cos) = self.heading.sin_cos();
        self.origin
            + (Vec3::new(cos, sin, 0.0) * along + Vec3::new(sin, -cos, 0.0) * right + Vec3::Z * up)
                / UNIT
    }

    /// Lays a stretch of another circuit over this one: `key` is a point on the ground
    /// of its road and `heading` the way its road runs there, in degrees.
    fn fit(&self, key: Vec3, heading: f32, scale: f32) -> Fit {
        Fit {
            origin: self.origin,
            key,
            turn: self.heading - heading.to_radians(),
            scale,
        }
    }
}

/// Where the things of another circuit go in ours.
struct Fit {
    origin: Vec3,
    key: Vec3,
    turn: f32,
    scale: f32,
}

impl Fit {
    fn point(&self, p: Vec3) -> Vec3 {
        self.origin + Quat::from_rotation_z(self.turn) * ((p - self.key) * self.scale)
    }
}

struct Builder<'a> {
    track: &'a mut Track,
    world: &'a mut LoadedWorld,
    events: TrackEvents,
    hazards: Vec<(i32, Kind)>,
    stands: BrickMesh,
}

impl Builder<'_> {
    /// Moves borrowed models from their own circuit to ours.
    fn place(&mut self, fit: &Fit, names: &[&str]) {
        for def in self
            .world
            .props
            .iter_mut()
            .filter(|def| names.contains(&def.name()))
        {
            def.moved(
                Quat::from_rotation_z(fit.turn),
                fit.point(def.position()),
                fit.scale,
            );
        }
    }

    /// A hazard, and the trigger that sets it going: a sphere this many metres across
    /// its middle.
    fn hazard(&mut self, event: i32, kind: Kind, at: Vec3, radius: f32) {
        self.events.trigger(to_world(at), radius, event, false);
        self.hazards.push((event, kind));
    }

    /// A column of bricks from the ground up to `top`.
    fn plinth(&mut self, top: Vec3, half: f32, colour: Color) {
        let top = to_world(top);
        let centre = Vec3::new(top.x, top.y / 2.0, top.z);
        self.stands.brick(
            centre,
            Vec3::new(half, top.y / 2.0, half),
            Quat::IDENTITY,
            colour,
            (2, 2),
        );
        self.track.clearings.push((centre, half + 5.0));
    }

    /// A bar of bricks between two points.
    fn beam(&mut self, from: Vec3, to: Vec3, half: f32, colour: Color) {
        let (from, to) = (to_world(from), to_world(to));
        let turn = Transform::IDENTITY.looking_to(to - from, Vec3::Y).rotation;
        self.stands.cuboid(
            (from + to) / 2.0,
            Vec3::new(half, half, from.distance(to) / 2.0),
            turn,
            colour,
        );
    }

    /// A flat round patch on the ground.
    fn disc(&mut self, at: Vec3, radius: f32, colour: Color) {
        let at = to_world(at).with_y(0.0);
        self.stands.cyl(at, radius, 0.12, Quat::IDENTITY, colour);
        self.track.clearings.push((at, radius + 3.0));
    }

    /// A cannon with its muzzle at `at`, pointed at `target`.
    fn cannon(&mut self, at: Vec3, target: Vec3) {
        let (at, target) = (to_world(at), to_world(target));
        let aim = ((target - at).with_y(0.0).normalize_or(Vec3::X) + Vec3::Y * 0.3).normalize();
        let breech = at - aim * 3.0;
        self.stands.cyl(
            breech,
            0.7,
            3.0,
            Quat::from_rotation_arc(Vec3::Y, aim),
            BLACK,
        );
        let base = Vec3::new(breech.x, (breech.y - 0.4) / 2.0, breech.z);
        self.stands.brick(
            base,
            Vec3::new(1.4, base.y, 1.4),
            Quat::IDENTITY,
            BROWN,
            (2, 2),
        );
        self.track.clearings.push((base, 6.0));
    }

    /// A wall for cars between two points on the ground, this high.
    fn wall(&mut self, tag: usize, from: Vec3, to: Vec3, height: f32) {
        let (a, b) = (to_world(from) - Vec3::Y, to_world(to) - Vec3::Y);
        let up = Vec3::Y * (height + 1.0);
        self.track
            .collision
            .add_tagged([a, b, b + up], Surface::default(), tag);
        self.track
            .collision
            .add_tagged([a, b + up, a + up], Surface::default(), tag);
    }

    /// Names a tag's walls for the hazard that opens and closes them, and says how
    /// they start out.
    fn surface(&mut self, name: &str, tag: usize, passable: bool) {
        self.track.collision.set_passable(tag, passable);
        self.track.surfaces.insert(name.into(), (tag, passable));
    }

    /// The right half of the road as a lane of its own behind a gate at `site`: a wall
    /// down the middle of the road with bricks to be had beside it, which the
    /// computer's cars keep to the left of. The gate itself is `tag`'s.
    fn shut_lane(&mut self, site: Site, tag: usize) {
        self.wall(tag, site.at(0.0, 0.0, 0.0), site.at(0.0, WALL, 0.0), 9.0);
        self.wall(
            0,
            site.at(0.0, 0.0, 0.0),
            site.at(LANE_LENGTH, 0.0, 0.0),
            3.0,
        );
        for piece in 0..7 {
            let from = piece as f32 * LANE_LENGTH / 7.0;
            let colour = if piece % 2 == 0 { RED } else { WHITE };
            self.beam(
                site.at(from, 0.0, 0.5),
                site.at(from + LANE_LENGTH / 7.0, 0.0, 0.5),
                0.45,
                colour,
            );
        }
        for (right, power) in [(0.25, Power::Red), (0.5, Power::Green), (0.75, Power::Yellow)] {
            let at = site.at(14.0, right * WALL, BRICK_HEIGHT);
            self.world.bricks.push((Some(power), to_world(at)));
        }
        for right in [0.375, 0.625] {
            let at = site.at(24.0, right * WALL, BRICK_HEIGHT);
            self.world.bricks.push((None, to_world(at)));
        }
        self.keep_left(site, LANE_WARNING, LANE_LENGTH + 5.0, -2.0);
    }

    /// Has the computer's cars keep `most` or further to the left from `before` the
    /// site to `after` it.
    fn keep_left(&mut self, site: Site, before: f32, after: f32, most: f32) {
        let (least, most) = if mirror() {
            (-most, self.track.road)
        } else {
            (-self.track.road, most)
        };
        self.track.lanes.push(Lane {
            from: site.s - before,
            to: site.s + after,
            least,
            most,
        });
    }

    /// The castle's hammer swings across the road from a frame over it.
    fn hammer(&mut self, site: Site) {
        self.place(
            &site.fit(Vec3::new(86.3, -649.0, -6.2), -15.5, 1.0),
            &["rkhamm02"],
        );
        self.hazard(HAMMER, Kind::Hammer { raised: true }, site.origin, 60.0);
        self.hazards.push((
            HAMMER,
            Kind::RollingRock {
                prop: "rkhamm02".into(),
                size: Vec3::new(30.0, 12.0, 14.0),
                turns: true,
                start: 0.0,
                last: None,
            },
        ));
        for side in [-FRAME_OUT, FRAME_OUT] {
            self.plinth(site.at(0.0, side, 18.6), 1.3, GREY);
        }
        self.beam(
            site.at(0.0, -FRAME_OUT, 18.0),
            site.at(0.0, FRAME_OUT, 18.0),
            0.8,
            DARK_GREY,
        );
    }

    /// The temple's three stones roll across the road one after another.
    fn stones(&mut self, sites: [Site; 3]) {
        let rocks = [
            ("dum02", Vec3::new(-713.0, 234.6, -78.5), 82.0, 100.0),
            ("dum03", Vec3::new(-682.0, 109.0, -89.9), 141.6, 0.0),
            ("dum04", Vec3::new(-697.5, 347.6, -53.5), 86.5, 150.0),
        ];
        for (i, ((name, key, heading, start), site)) in rocks.into_iter().zip(sites).enumerate() {
            self.place(&site.fit(key, heading, 1.0), &[name]);
            self.hazard(
                ROCKS + i as i32,
                Kind::RollingRock {
                    prop: name.into(),
                    size: Vec3::new(40.0, 10.0, 40.0),
                    turns: false,
                    start,
                    last: None,
                },
                site.origin,
                45.0,
            );
        }
    }

    /// Two cannons, one either side, each firing at the road as cars come up to it.
    fn cannons(&mut self, site: Site) {
        for (i, side) in [1.0, -1.0].into_iter().enumerate() {
            let muzzle = site.at(-6.0 * side, CANNON_OUT * side, 5.5);
            let targets = [(-20.0, -4.0), (-9.0, 3.0), (3.0, -2.0), (14.0, 4.0)];
            let targets = targets
                .iter()
                .map(|&(along, right)| (to_world(site.at(along, right * side, 0.3)), LANDED))
                .collect();
            let mouth = site.at(-45.0 + 28.0 * i as f32, 0.0, 0.0);
            self.events
                .trigger(to_world(mouth), 10.0, CANNONS + i as i32, false);
            let launcher = Kind::Launcher {
                sources: vec![(to_world(muzzle), -1)],
                targets,
                near: Some((to_world(site.origin), 150.0)),
                event: CANNONS + i as i32,
                ball: None,
                multi: true,
                landing: None,
            };
            self.hazards.push((-1, launcher));
            self.cannon(muzzle, site.origin);
        }
    }

    /// The forest's ghost comes up the road the other way.
    fn ghost(&mut self, site: Site) {
        self.place(
            &site.fit(Vec3::new(-413.0, -485.0, -181.9), -168.5, 1.0),
            &["ghostly"],
        );
        self.hazard(
            GHOST,
            Kind::Ghost {
                search: 0.0,
                waver: 0.0,
                depth: 0.0,
                trail: None,
            },
            site.origin,
            90.0,
        );
    }

    /// The knights' dragon flies up and down above the road, raining cannon balls.
    /// `reach` is how near a car must come for the cannonballs to begin.
    fn dragon(&mut self, site: Site, reach: f32) {
        self.place(
            &site.fit(Vec3::new(-229.0, -360.0, -88.4), -89.2, 1.0),
            &["prntdum"],
        );
        self.hazard(
            DRAGON,
            Kind::CannonballRain {
                prop: "prntdum".into(),
                interval: 1.5,
                timer: 0.0,
            },
            site.origin,
            reach,
        );
    }

    /// The temple's ark sweeps its lightning round from a plinth beside the road.
    fn ark(&mut self, site: Site) {
        let fit = site.fit(Vec3::new(540.35, -495.04, -72.5), 0.0, 1.0);
        self.place(&fit, &["arktp"]);
        let ark = Kind::SweepCannon {
            prop: Some("arktp".into()),
            source: Vec3::ZERO,
            period: 2.0,
            sweep: [TAU, 0.0, -0.4],
            time: 0.0,
            cooldown: 0.0,
            beam: None,
        };
        self.hazard(ARK, ark, site.origin, 50.0);
        self.plinth(fit.point(Vec3::new(540.35, -495.04, -45.1)), 3.2, TAN);
    }

    /// A mummy's curse is left on the road ahead of whoever comes first to where
    /// it is set off, at one of its three places.
    fn curse(&mut self, places: &mut Places, plain: &Track, (site, drops): Curse) {
        let site = Site::along(plain, site, 0.0);
        self.hazard(CURSE, Kind::CurseDrop, site.origin, CURSE_REACH);
        places.curse_drops = drops.map(|(s, right)| Site::along(plain, s, right).origin);
    }
}

/// Stands a map's hazards round `track`, which is that map's, with the field of
/// `circuit`. `None` without the game's data, and `track` is then left as it was.
pub fn load(
    map: &Map,
    track: &mut Track,
    circuit: Option<&str>,
) -> Option<(LoadedWorld, TrackEvents, Hazards, Stands)> {
    let taken = borrowed(&map.hazards);
    let taken: Vec<(&str, &[&str], &[&str])> = taken
        .iter()
        .map(|(race, models, emitters)| (*race, &models[..], &emitters[..]))
        .collect();
    let mut world = world::borrowed(circuit.unwrap_or("c0"), &taken)?;
    // Everything is worked out on the circuit as it is unmirrored, in the game's
    // coordinates, and mirrored with the rest on its way into the world.
    let plain = Track::plain(map);
    let mut places = Places::default();
    let mut b = Builder {
        track,
        world: &mut world,
        events: TrackEvents::default(),
        hazards: Vec::new(),
        stands: BrickMesh::default(),
    };
    for stood in &map.hazards {
        let at = |s: f32, lat: f32| Site::along(&plain, s, lat);
        // On the road, or at a place that is on none.
        let (s, right) = (stood.at.unwrap_or(0.0), stood.right);
        let site = match stood.place {
            Some([x, z]) => Site::place(x, z, stood.heading.to_radians()),
            None => at(s, right),
        };
        match stood.kind {
            Sort::Hammer => b.hammer(site),
            Sort::Stones => {
                let apart = stood.apart.unwrap_or(STONES_APART);
                b.stones([0.0, 1.0, 2.0].map(|stone| at(s + apart * stone, right)));
            }
            Sort::Cannons => b.cannons(site),
            Sort::Ghost => b.ghost(site),
            Sort::Dragon => b.dragon(site, stood.reach.unwrap_or(DRAGON_REACH)),
            Sort::Ark => b.ark(site),
            Sort::Curse => {
                let drops = stood.drops.unwrap_or_default().map(|[s, right]| (s, right));
                b.curse(&mut places, &plain, (s, drops));
            }
            Sort::Saucer => b.saucer(site),
            Sort::Lava => b.lava(&mut places, site),
            Sort::Field => b.field(site, at(s, right + WALL / 2.0)),
            Sort::Pillar => b.pillar(site),
            Sort::Barrels => b.barrels(site, at(s, right + WALL / 2.0)),
            Sort::Crane => b.crane(site),
            Sort::Warp => b.warp(&mut places, site, at(stood.out.unwrap_or(s), 0.0)),
            Sort::Smoke => b.smoke(site),
            Sort::Water => b.water(site),
        }
    }

    // Bricks where the circuit's own features ask for them, as without the hazards.
    for (power, on) in crate::items::laid(b.track) {
        b.world.bricks.push((power, on + Vec3::Y * BRICK_HEIGHT));
    }

    let Builder {
        events,
        hazards,
        stands,
        ..
    } = b;
    Some((
        world,
        events,
        Hazards::of(hazards, places),
        Stands(stands.build()),
    ))
}

/// The hazards only the gauntlet has, as the road comes to them.
impl Builder<'_> {
    /// The aliens' saucer comes down the road at its lowest.
    fn saucer(&mut self, site: Site) {
        self.place(
            &site.fit(Vec3::new(-261.0, 394.0, 160.0), -65.0, 1.0),
            &["ufofly1", "beamdumy"],
        );
        let grabber = Kind::Grabber {
            prop: "ufofly1".into(),
            strength: 800.0,
            frames: (85.0, 200.0),
            held: 0.0,
            rest: 0.0,
        };
        self.hazard(UFO, grabber, site.origin, 90.0);
    }

    /// The moon's lava leaps between three pools, two of them at the road.
    fn lava(&mut self, places: &mut Places, site: Site) {
        let fit = site.fit(Vec3::new(550.0, -462.0, 6.0), 58.0, 1.0);
        self.place(&fit, &["mmlavbl"]);
        places.lava_pools = places.lava_pools.map(|pool| fit.point(pool));
        self.hazard(
            LAVA,
            Kind::LavaGeyser {
                cooldown: 0.0,
                flying: false,
                smoke: 0.0,
            },
            site.origin,
            70.0,
        );
        for pool in places.lava_pools {
            self.disc(pool, 4.0, ORANGE);
        }
    }

    /// The moon's force field shuts a lane to all but the shielded: the right half
    /// of the road at `site`, with its `gate` in the middle of that half.
    fn field(&mut self, site: Site, gate: Site) {
        self.place(
            &gate.fit(Vec3::new(411.06, -116.03, 0.0), 0.0, 0.7),
            &["mmrocon", "mmrocof"],
        );
        self.hazard(FIELD, Kind::Rocket { open: false }, gate.origin, 105.0);
        self.shut_lane(site, FIELD_WALL);
        self.surface("mmrocc", FIELD_WALL, false);
    }

    /// The desert's pillar falls across the right of the road as the first car comes.
    fn pillar(&mut self, site: Site) {
        let fit = site.fit(Vec3::new(842.0, 296.7, -60.0), 90.0, 1.0);
        self.place(&fit, &["piltop"]);
        self.events
            .trigger(to_world(site.at(-60.0, 0.0, 0.0)), 15.0, PILLAR, false);
        self.hazards.push((
            PILLAR,
            Kind::FallingPillar {
                fallen: false,
                landed: false,
            },
        ));
        self.plinth(fit.point(Vec3::new(902.5, 294.0, -0.5)), 3.0, TAN);
        // Where it lies once it has fallen.
        let corner = |x: f32, y: f32| fit.point(Vec3::new(x, y, -60.0));
        let corners = [
            corner(842.0, 292.0),
            corner(885.0, 292.0),
            corner(885.0, 301.0),
            corner(842.0, 301.0),
        ];
        for i in 0..4 {
            self.wall(PILLAR_WALL, corners[i], corners[(i + 1) % 4], 8.0);
        }
        self.surface("pilcol", PILLAR_WALL, true);
        self.keep_left(site, 45.0, 12.0, -2.5);
    }

    /// The dock's barrels shut a lane until a cannon, set off by a horn, clears
    /// them: the right half of the road at `site`, with its `gate` in the middle of
    /// that half.
    fn barrels(&mut self, site: Site, gate: Site) {
        let barrels = ["block", "barrel", "barrel01", "barrel02"];
        self.place(
            &gate.fit(Vec3::new(-402.0, 513.5, 52.5), 57.5, 0.8),
            &barrels,
        );
        self.hazards.push((
            BARRELS,
            Kind::TriggeredAnimation {
                surface: "shootme".into(),
                props: barrels.map(String::from).to_vec(),
            },
        ));
        let muzzle = site.at(-25.0, -15.0, 5.5);
        let launcher = Kind::Launcher {
            sources: vec![(to_world(muzzle), -1)],
            targets: vec![(to_world(gate.at(0.0, 0.0, 1.5)), BARRELS)],
            near: Some((to_world(site.at(-20.0, 0.0, 0.0)), 45.0)),
            event: HORN,
            ball: None,
            multi: true,
            landing: None,
        };
        self.hazards.push((-1, launcher));
        self.cannon(muzzle, gate.origin);
        self.shut_lane(site, BARREL_WALL);
        self.surface("shootme", BARREL_WALL, false);
    }

    /// The dock's crane swings its load in from the left.
    fn crane(&mut self, site: Site) {
        let fit = site.fit(Vec3::new(-492.0, -508.0, -76.3), 139.0, 1.0);
        self.place(&fit, &["crane"]);
        self.hazard(CRANE, Kind::Crane { pending: true }, site.origin, 70.0);
        self.plinth(fit.point(Vec3::new(-530.15, -569.28, -31.5)), 3.0, YELLOW);
    }

    /// A warp pad, and where it comes out.
    fn warp(&mut self, places: &mut Places, pad: Site, out: Site) {
        self.events.trigger(to_world(pad.origin), 3.5, WARP, false);
        self.hazards.push((WARP, Kind::WarpPad));
        (places.warp_to, places.warp_facing) =
            (out.at(0.0, 0.0, 0.2), out.at(1.0, 0.0, 0.0) - out.origin);
        self.disc(pad.origin, 3.2, BLUE);
    }

    /// The ice planet's craft hangs over the road letting out smoke, and it snows
    /// there.
    fn smoke(&mut self, site: Site) {
        self.place(
            &site.fit(Vec3::new(454.6, -530.6, -95.0), 21.2, 1.0),
            &["dp_def"],
        );
        self.hazard(SMOKE, Kind::SmokeVent { emitter: None }, site.origin, 70.0);
        self.events.trigger(to_world(site.origin), 100.0, SNOW, true);
        self.hazards.push((SNOW, Kind::Snowfall { emitter: None }));
    }

    /// The island's water lies about its `middle`, sloshing.
    fn water(&mut self, middle: Site) {
        self.place(
            &middle.fit(Vec3::new(-722.5, -416.5, -15.9), 90.0, 1.0),
            &["water01"],
        );
        self.hazards.push((
            -1,
            Kind::Oscillator {
                prop: "water01".into(),
                amplitude: Vec2::new(0.2, 0.0),
                time: 0.0,
            },
        ));
        for along in [-54.0, 0.0, 54.0] {
            self.track
                .clearings
                .push((to_world(middle.at(along, 0.0, 0.0)), 50.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every model a map's hazards take.
    fn wanted(map: &Map) -> Vec<&'static str> {
        borrowed(&map.hazards).into_iter().flat_map(|taken| taken.1).collect()
    }

    /// Needs the original game data; silently passes without it.
    #[test]
    fn every_borrowed_model_is_found_and_moved_to_the_gauntlet() {
        let map = crate::map::named("GAUNTLET");
        let mut track = Track::built(map);
        let Some((world, ..)) = load(map, &mut track, None) else {
            return;
        };
        // Everything there is to stand is stood there, and takes what it did.
        assert_eq!(map.hazards.len(), 16);
        let wanted = wanted(map);
        assert_eq!(wanted.len(), 20);
        for name in &wanted {
            let found: Vec<_> = world.props.iter().filter(|p| p.name() == *name).collect();
            assert_eq!(found.len(), 1, "{name}");
            // Each stands within reach of the gauntlet, wherever its own circuit had it.
            let at = to_world(found[0].position());
            let off = track
                .pts
                .iter()
                .map(|&p| crate::track::xz_dist2(p, at))
                .fold(f32::MAX, f32::min)
                .sqrt();
            assert!(off < 140.0, "{name} is {off} from the road");
        }
        assert_eq!(world.props.len(), wanted.len());
        for emitter in borrowed(&map.hazards).iter().flat_map(|taken| taken.2.iter()) {
            assert!(world.emitters.iter().any(|e| e.0 == *emitter), "{emitter}");
        }
        // The gates are shut and the fallen pillar isn't there yet.
        for (name, passable) in [("mmrocc", false), ("shootme", false), ("pilcol", true)] {
            assert_eq!(
                track.surfaces.get(name).map(|s| s.1),
                Some(passable),
                "{name}"
            );
        }
        // The computer's cars are kept out of the shut lanes, and bricks are left in them.
        let field = map.hazards.iter().find(|stood| stood.kind == Sort::Field);
        let gate = Site::along(&Track::plain(map), field.and_then(|field| field.at).unwrap(), 0.0);
        assert_eq!(track.lane(gate.s, 5.0), -2.0);
        assert_eq!(track.lane(gate.s - 100.0, 5.0), 5.0);
        assert!(world.bricks.iter().any(|b| b.1.distance(to_world(gate.at(
            14.0,
            WALL / 2.0,
            BRICK_HEIGHT
        ))) < 0.1));
    }

    /// Needs the original game data; silently passes without it.
    #[test]
    fn the_other_circuits_hazards_are_found_and_stood_by_their_roads() {
        for layout in ["BRICK", "FIGURE8", "HELTER"] {
            let map = crate::map::named(layout);
            let mut track = Track::built(map);
            let Some((world, ..)) = load(map, &mut track, None) else {
                return;
            };
            assert!(!map.hazards.is_empty(), "{layout}");
            assert_eq!(world.props.len(), wanted(map).len(), "{layout}");
            for prop in &world.props {
                let at = to_world(prop.position());
                let off = track
                    .pts
                    .iter()
                    .map(|&p| crate::track::xz_dist2(p, at))
                    .fold(f32::MAX, f32::min)
                    .sqrt();
                assert!(
                    off < 200.0,
                    "{layout} {} is {off} from the road",
                    prop.name()
                );
            }
            // The bricks are the ones the circuit has without its hazards.
            assert_eq!(world.bricks.len(), crate::items::laid(&track).len());
        }
    }

    /// A curse is not left where the race begins, and where it is left it can be
    /// seen coming: the three places are well on from where it is set off, with
    /// little of a corner between, and each leaves the road beside it to go by on.
    #[test]
    fn a_curse_is_left_where_it_can_be_seen_and_gone_round() {
        let cursed: Vec<(&Map, &Stood)> = crate::map::circuits()
            .flat_map(|map| map.hazards.iter().map(move |stood| (map, stood)))
            .filter(|(_, stood)| stood.kind == Sort::Curse)
            .collect();
        assert_eq!(cursed.len(), 2);
        for (map, stood) in cursed {
            let (layout, site) = (map.key.as_str(), stood.at.unwrap());
            let drops = stood.drops.unwrap().map(|[s, right]| (s, right));
            let track = Track::plain(map);
            let at = |s: f32| (s / track.spacing).round() as usize % track.n();
            // In the middle half of the lap, far from the grid either way round.
            assert!(
                site > track.length / 4.0 && site < 3.0 * track.length / 4.0,
                "{layout} {site} of {}",
                track.length
            );
            for (s, right) in drops {
                // A second and a half off at the least for whoever sets it off,
                // going flat out, and more for a car just out of a hairpin.
                let warning = s - (site - CURSE_REACH);
                assert!((55.0..110.0).contains(&warning), "{layout} {warning}");
                // It is all but straight ahead of a car where it is set off.
                let to = (track.point(s, right) - track.pts[at(site)]).with_y(0.0);
                let off = to.angle_between(track.flat[at(site)]).to_degrees();
                assert!(off < 20.0, "{layout} is {off} degrees off");
                assert!(right.abs() < crate::track::ROAD_HW);
            }
            // With one lying in each of the three places at once there is still a
            // way by: the middle of a car may come within half a car's width of
            // either barrier and must keep a curse's reach off each, and what is
            // left of the road for it on one side is a car and a half wide.
            let (reach, car) = (10.0 * UNIT + 1.5 * UNIT, 0.95);
            let rights = drops.map(|drop| drop.1);
            let left = rights.iter().fold(f32::MAX, |least, &right| least.min(right)) - reach;
            let right = rights.iter().fold(f32::MIN, |most, &right| most.max(right)) + reach;
            let room = (left + (WALL - car)).max((WALL - car) - right);
            assert!(room > 3.0 * car, "{layout} {room}");
        }
    }

    #[test]
    fn a_fit_lays_another_circuit_s_road_along_ours() {
        // A site heading north-west, and a road of another circuit's heading east.
        let site = Site {
            origin: Vec3::new(10.0, 20.0, 0.0),
            heading: 0.75 * std::f32::consts::PI,
            s: 0.0,
        };
        let fit = site.fit(Vec3::new(100.0, 200.0, -50.0), 0.0, 2.0);
        assert!(
            fit.point(Vec3::new(100.0, 200.0, -50.0))
                .distance(site.origin)
                < 1e-3
        );
        // A point further along that road and above it is as far along ours, twice over.
        let ahead = fit.point(Vec3::new(101.0, 200.0, -47.0));
        assert!(
            ahead.distance(site.at(2.0 * UNIT, 0.0, 6.0 * UNIT)) < 1e-3,
            "{ahead}"
        );
        // One to its left is to our left.
        let left = fit.point(Vec3::new(100.0, 201.0, -50.0));
        assert!(
            left.distance(site.at(0.0, -2.0 * UNIT, 0.0)) < 1e-3,
            "{left}"
        );
    }
}
