//! The gauntlet: a circuit of the port's own that runs past as many of the original's
//! hazards as can be moved. Each is borrowed from the circuit it belongs to (its models,
//! its animation, its emitters) and stood on a stretch of a brick-built road, turned
//! so that the stretch of its own road it was made for lies along ours. The hazards
//! themselves are `hazards`' and behave as they do at home. Between the straights the
//! road twists: zigzags, hairpins, hills, two tunnels and a loop over a bridge, all
//! laid out in `track`, which leaves two long straights.
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
//! Without the game's data there is nothing to borrow, and the gauntlet is a plain
//! circuit.

use crate::assets::materials::Surface;
use crate::events::TrackEvents;
use crate::hazards::{Hazards, Kind, Places};
use crate::items::{BRICK_HEIGHT, Power};
use crate::meshgen::*;
use crate::physics::UNIT;
use crate::scenery::{mirror, to_world};
use crate::track::{Lane, Layout, Track, WALL};
use crate::world::{self, LoadedWorld};
use bevy::prelude::*;
use std::f32::consts::TAU;

/// What is taken from each race: models, then emitters.
const BORROWED: [(&str, &[&str], &[&str]); 10] = [
    ("RACEC0R0", &["rkhamm02"], &[]),
    ("RACEC0R1", &["crane", "block", "barrel", "barrel01", "barrel02"], &[]),
    ("RACEC0R2", &["piltop"], &[]),
    ("RACEC0R3", &["mmlavbl", "mmrocon", "mmrocof"], &["lavasmk"]),
    ("RACEC1R0", &["ghostly"], &["ghsttrl"]),
    ("RACEC1R1", &["water01"], &[]),
    ("RACEC1R3", &["dp_def"], &["smoke", "snow"]),
    ("RACEC2R0", &["prntdum"], &[]),
    ("RACEC2R2", &["arktp", "dum02", "dum03", "dum04"], &[]),
    ("RACEC2R3", &["ufofly1", "beamdumy"], &[]),
];

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
/// Where the force field's gate is, as (x, z) of ours.
const FIELD_AT: (f32, f32) = (467.0, -348.4);

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
    /// The road nearest (`x`, `z`) of ours, `lat` to the right of its middle.
    fn on(track: &Track, x: f32, z: f32, lat: f32) -> Site {
        let i = track.nearest(Vec3::new(x, 0.0, z));
        let (at, flat) = (track.pts[i] + track.right[i] * lat, track.flat[i]);
        Site { origin: Vec3::new(at.x, -at.z, at.y) / UNIT, heading: (-flat.z).atan2(flat.x), s: i as f32 * track.spacing }
    }

    /// The point this far on, to the right and up, in metres.
    fn at(&self, along: f32, right: f32, up: f32) -> Vec3 {
        let (sin, cos) = self.heading.sin_cos();
        self.origin + (Vec3::new(cos, sin, 0.0) * along + Vec3::new(sin, -cos, 0.0) * right + Vec3::Z * up) / UNIT
    }

    /// Lays a stretch of another circuit over this one: `key` is a point on the ground
    /// of its road and `heading` the way its road runs there, in degrees.
    fn fit(&self, key: Vec3, heading: f32, scale: f32) -> Fit {
        Fit { origin: self.origin, key, turn: self.heading - heading.to_radians(), scale }
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
        for def in self.world.props.iter_mut().filter(|def| names.contains(&def.name())) {
            def.moved(Quat::from_rotation_z(fit.turn), fit.point(def.position()), fit.scale);
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
        self.stands.brick(centre, Vec3::new(half, top.y / 2.0, half), Quat::IDENTITY, colour, (2, 2));
        self.track.clearings.push((centre, half + 5.0));
    }

    /// A bar of bricks between two points.
    fn beam(&mut self, from: Vec3, to: Vec3, half: f32, colour: Color) {
        let (from, to) = (to_world(from), to_world(to));
        let turn = Transform::IDENTITY.looking_to(to - from, Vec3::Y).rotation;
        self.stands.cuboid((from + to) / 2.0, Vec3::new(half, half, from.distance(to) / 2.0), turn, colour);
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
        self.stands.cyl(breech, 0.7, 3.0, Quat::from_rotation_arc(Vec3::Y, aim), BLACK);
        let base = Vec3::new(breech.x, (breech.y - 0.4) / 2.0, breech.z);
        self.stands.brick(base, Vec3::new(1.4, base.y, 1.4), Quat::IDENTITY, BROWN, (2, 2));
        self.track.clearings.push((base, 6.0));
    }

    /// A wall for cars between two points on the ground, this high.
    fn wall(&mut self, tag: usize, from: Vec3, to: Vec3, height: f32) {
        let (a, b) = (to_world(from) - Vec3::Y, to_world(to) - Vec3::Y);
        let up = Vec3::Y * (height + 1.0);
        self.track.collision.add_tagged([a, b, b + up], Surface::default(), tag);
        self.track.collision.add_tagged([a, b + up, a + up], Surface::default(), tag);
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
        self.wall(0, site.at(0.0, 0.0, 0.0), site.at(LANE_LENGTH, 0.0, 0.0), 3.0);
        for piece in 0..7 {
            let from = piece as f32 * LANE_LENGTH / 7.0;
            let colour = if piece % 2 == 0 { RED } else { WHITE };
            self.beam(site.at(from, 0.0, 0.5), site.at(from + LANE_LENGTH / 7.0, 0.0, 0.5), 0.45, colour);
        }
        for (right, power) in [(2.5, Power::Red), (5.0, Power::Green), (7.5, Power::Yellow)] {
            self.world.bricks.push((Some(power), to_world(site.at(14.0, right, BRICK_HEIGHT))));
        }
        for right in [3.75, 6.25] {
            self.world.bricks.push((None, to_world(site.at(24.0, right, BRICK_HEIGHT))));
        }
        self.keep_left(site, LANE_WARNING, LANE_LENGTH + 5.0, -2.0);
    }

    /// Has the computer's cars keep `most` or further to the left from `before` the
    /// site to `after` it.
    fn keep_left(&mut self, site: Site, before: f32, after: f32, most: f32) {
        let (least, most) = if mirror() { (-most, self.track.road) } else { (-self.track.road, most) };
        self.track.lanes.push(Lane { from: site.s - before, to: site.s + after, least, most });
    }
}

/// Stands the hazards round the gauntlet, which `track` is, with the field of
/// `circuit`. `None` without the game's data, and `track` is then left as it was.
pub fn load(track: &mut Track, circuit: Option<&str>) -> Option<(LoadedWorld, TrackEvents, Hazards, Stands)> {
    let mut world = world::borrowed(circuit.unwrap_or("c0"), &BORROWED)?;
    // Everything is worked out on the circuit as it is unmirrored, in the game's
    // coordinates, and mirrored with the rest on its way into the world.
    let plain = Track::plain(Layout::Gauntlet);
    let on = |x: f32, z: f32, lat: f32| Site::on(&plain, x, z, lat);
    let mut places = Places::default();
    let mut b = Builder { track, world: &mut world, events: TrackEvents::default(), hazards: Vec::new(), stands: BrickMesh::default() };

    // The castle's hammer swings across the road from a frame over it.
    let site = on(45.0, 0.0, 0.0);
    b.place(&site.fit(Vec3::new(86.3, -649.0, -6.2), -15.5, 1.0), &["rkhamm02"]);
    b.hazard(HAMMER, Kind::Hammer { raised: true }, site.origin, 60.0);
    b.hazards.push((HAMMER, Kind::RollingRock { prop: "rkhamm02".into(), radius: 2.8, start: 0.0, last: None }));
    for side in [-14.0, 14.0] {
        b.plinth(site.at(0.0, side, 18.6), 1.3, GREY);
    }
    b.beam(site.at(0.0, -14.0, 18.0), site.at(0.0, 14.0, 18.0), 0.8, DARK_GREY);

    // The temple's three stones roll across it one after another.
    let rocks = [
        ("dum02", Vec3::new(-713.0, 234.6, -78.5), 82.0, 100.0),
        ("dum03", Vec3::new(-682.0, 109.0, -89.9), 141.6, 0.0),
        ("dum04", Vec3::new(-697.5, 347.6, -53.5), 86.5, 150.0),
    ];
    for (i, (name, key, heading, start)) in rocks.into_iter().enumerate() {
        let site = on(80.0 + 25.0 * i as f32, 0.0, 0.0);
        b.place(&site.fit(key, heading, 1.0), &[name]);
        b.hazard(ROCKS + i as i32, Kind::RollingRock { prop: name.into(), radius: 4.5, start, last: None }, site.origin, 45.0);
    }

    // The aliens' saucer comes down the road at its lowest.
    let site = on(236.0, -60.0, 0.0);
    b.place(&site.fit(Vec3::new(-261.0, 394.0, 160.0), -65.0, 1.0), &["ufofly1", "beamdumy"]);
    let grabber = Kind::Grabber { prop: "ufofly1".into(), strength: 800.0, frames: (85.0, 200.0), held: 0.0, rest: 0.0 };
    b.hazard(UFO, grabber, site.origin, 90.0);

    // The dock's crane swings its load in from the left.
    let site = on(401.0, -60.0, 2.0);
    let fit = site.fit(Vec3::new(-492.0, -508.0, -76.3), 139.0, 1.0);
    b.place(&fit, &["crane"]);
    b.hazard(CRANE, Kind::Crane { pending: true }, site.origin, 70.0);
    b.plinth(fit.point(Vec3::new(-530.15, -569.28, -31.5)), 3.0, YELLOW);

    // A warp pad on the left of the road, and where it comes out: past the switchback.
    let (pad, out) = (on(459.0, -100.0, -5.5), on(531.0, -80.0, 0.0));
    b.events.trigger(to_world(pad.origin), 3.5, WARP, false);
    b.hazards.push((WARP, Kind::WarpPad));
    (places.warp_to, places.warp_facing) = (out.at(0.0, 0.0, 0.2), out.at(1.0, 0.0, 0.0) - out.origin);
    b.disc(pad.origin, 3.2, BLUE);
    b.disc(out.origin, 2.0, BLUE);

    // Two cannons, one either side, each firing at the road as cars come up to it.
    let site = on(531.0, -120.0, 0.0);
    for (i, side) in [1.0, -1.0].into_iter().enumerate() {
        let muzzle = site.at(-6.0 * side, 16.0 * side, 5.5);
        let targets = [(-20.0, -4.0), (-9.0, 3.0), (3.0, -2.0), (14.0, 4.0)];
        let targets = targets.iter().map(|&(along, right)| (to_world(site.at(along, right * side, 0.3)), LANDED)).collect();
        let mouth = site.at(-45.0 + 28.0 * i as f32, 0.0, 0.0);
        b.events.trigger(to_world(mouth), 10.0, CANNONS + i as i32, false);
        let launcher = Kind::Launcher {
            sources: vec![(to_world(muzzle), -1)],
            targets,
            near: Some((to_world(site.origin), 150.0)),
            event: CANNONS + i as i32,
            ball: None,
            multi: true,
            landing: None,
        };
        b.hazards.push((-1, launcher));
        b.cannon(muzzle, site.origin);
    }

    // The moon's lava leaps between three pools, two of them at the road.
    let site = on(531.0, -275.0, 0.0);
    let fit = site.fit(Vec3::new(550.0, -462.0, 6.0), 58.0, 1.0);
    b.place(&fit, &["mmlavbl"]);
    places.lava_pools = places.lava_pools.map(|pool| fit.point(pool));
    b.hazard(LAVA, Kind::LavaGeyser { cooldown: 0.0, flying: false, smoke: 0.0 }, site.origin, 70.0);
    for pool in places.lava_pools {
        b.disc(pool, 4.0, ORANGE);
    }

    // The moon's force field shuts a lane to all but the shielded.
    let site = on(FIELD_AT.0, FIELD_AT.1, 0.0);
    let gate = on(FIELD_AT.0, FIELD_AT.1, WALL / 2.0);
    b.place(&gate.fit(Vec3::new(411.06, -116.03, 0.0), 0.0, 0.7), &["mmrocon", "mmrocof"]);
    b.hazard(FIELD, Kind::Rocket { open: false }, gate.origin, 105.0);
    b.shut_lane(site, FIELD_WALL);
    b.surface("mmrocc", FIELD_WALL, false);

    // The ice planet's craft hangs over the road letting out smoke, and it snows.
    let site = on(379.0, -328.6, 0.0);
    b.place(&site.fit(Vec3::new(454.6, -530.6, -95.0), 21.2, 1.0), &["dp_def"]);
    b.hazard(SMOKE, Kind::SmokeVent { emitter: None }, site.origin, 70.0);
    b.events.trigger(to_world(site.origin), 100.0, SNOW, true);
    b.hazards.push((SNOW, Kind::Snowfall { emitter: None }));

    // The forest's ghost comes up the road the other way.
    let site = on(192.0, -308.8, 0.0);
    b.place(&site.fit(Vec3::new(-413.0, -485.0, -181.9), -168.5, 1.0), &["ghostly"]);
    b.hazard(GHOST, Kind::Ghost { search: 0.0, waver: 0.0, depth: 0.0, trail: None }, site.origin, 90.0);

    // The knights' dragon flies up and down above it, raining cannon balls.
    let site = on(112.0, -308.8, 0.0);
    b.place(&site.fit(Vec3::new(-229.0, -360.0, -88.4), -89.2, 1.0), &["prntdum"]);
    b.hazard(DRAGON, Kind::CannonballRain { prop: "prntdum".into(), interval: 1.5, timer: 0.0 }, site.origin, 70.0);

    // The dock's barrels shut a lane until a cannon, set off by a horn, clears them.
    let site = on(-331.0, -63.0, 0.0);
    let gate = on(-331.0, -63.0, WALL / 2.0);
    let barrels = ["block", "barrel", "barrel01", "barrel02"];
    b.place(&gate.fit(Vec3::new(-402.0, 513.5, 52.5), 57.5, 0.8), &barrels);
    b.hazards.push((BARRELS, Kind::TriggeredAnimation { surface: "shootme".into(), props: barrels.map(String::from).to_vec() }));
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
    b.hazards.push((-1, launcher));
    b.cannon(muzzle, gate.origin);
    b.shut_lane(site, BARREL_WALL);
    b.surface("shootme", BARREL_WALL, false);

    // The desert's pillar falls across the right of the road as the first car comes.
    let site = on(-258.0, 0.0, 1.0);
    let fit = site.fit(Vec3::new(842.0, 296.7, -60.0), 90.0, 1.0);
    b.place(&fit, &["piltop"]);
    b.events.trigger(to_world(site.at(-60.0, 0.0, 0.0)), 15.0, PILLAR, false);
    b.hazards.push((PILLAR, Kind::FallingPillar { fallen: false, landed: false }));
    b.plinth(fit.point(Vec3::new(902.5, 294.0, -0.5)), 3.0, TAN);
    // Where it lies once it has fallen.
    let corner = |x: f32, y: f32| fit.point(Vec3::new(x, y, -60.0));
    let corners = [corner(842.0, 292.0), corner(885.0, 292.0), corner(885.0, 301.0), corner(842.0, 301.0)];
    for i in 0..4 {
        b.wall(PILLAR_WALL, corners[i], corners[(i + 1) % 4], 8.0);
    }
    b.surface("pilcol", PILLAR_WALL, true);
    b.keep_left(site, 45.0, 12.0, -2.5);

    // The temple's ark sweeps its lightning round from a plinth beside the road.
    let site = on(-115.0, 0.0, 13.5);
    let fit = site.fit(Vec3::new(540.35, -495.04, -72.5), 0.0, 1.0);
    b.place(&fit, &["arktp"]);
    let ark = Kind::SweepCannon { prop: Some("arktp".into()), source: Vec3::ZERO, period: 2.0, sweep: [TAU, 0.0, -0.4], time: 0.0, cooldown: 0.0, beam: None };
    b.hazard(ARK, ark, site.origin, 50.0);
    b.plinth(fit.point(Vec3::new(540.35, -495.04, -45.1)), 3.2, TAN);

    // And a mummy's curse is left on the road ahead of whoever comes first.
    b.hazard(CURSE, Kind::CurseDrop, on(-85.0, 0.0, 0.0).origin, 12.0);
    places.curse_drops = [(-60.0, -3.0), (-50.0, 3.0), (-40.0, 0.0)].map(|(x, right)| on(x, 0.0, right).origin);

    // The island's water lies in the middle of it all, sloshing.
    let middle = Site { origin: Vec3::new(70.0, 130.0, 0.0) / UNIT, heading: 0.0, s: 0.0 };
    b.place(&middle.fit(Vec3::new(-722.5, -416.5, -15.9), 90.0, 1.0), &["water01"]);
    b.hazards.push((-1, Kind::Oscillator { prop: "water01".into(), amplitude: Vec2::new(0.2, 0.0), time: 0.0 }));
    for along in [-54.0, 0.0, 54.0] {
        b.track.clearings.push((to_world(middle.at(along, 0.0, 0.0)), 50.0));
    }

    // Bricks at regular stations round the lap, as on the other built-in circuits.
    const STATIONS: usize = 16;
    let powers = [Power::Red, Power::Yellow, Power::Blue, Power::Green];
    for station in 1..STATIONS {
        let s = b.track.length * station as f32 / STATIONS as f32;
        let lift = Vec3::Y * BRICK_HEIGHT;
        if station % 2 == 1 {
            for (i, lat) in [-0.75, -0.25, 0.25, 0.75].into_iter().enumerate() {
                b.world.bricks.push((Some(powers[(i + station / 2) % 4]), b.track.point(s, lat * b.track.road) + lift));
            }
        } else {
            for lat in [-0.65, 0.0, 0.65] {
                b.world.bricks.push((None, b.track.point(s, lat * b.track.road) + lift));
            }
        }
    }

    let Builder { events, hazards, stands, .. } = b;
    Some((world, events, Hazards::of(hazards, places), Stands(stands.build())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the original game data; silently passes without it.
    #[test]
    fn every_borrowed_model_is_found_and_moved_to_the_gauntlet() {
        let mut track = Track::built(Layout::Gauntlet);
        let Some((world, ..)) = load(&mut track, None) else { return };
        let wanted: Vec<&str> = BORROWED.iter().flat_map(|b| b.1.iter().copied()).collect();
        for name in &wanted {
            let found: Vec<_> = world.props.iter().filter(|p| p.name() == *name).collect();
            assert_eq!(found.len(), 1, "{name}");
            // Each stands within reach of the gauntlet, wherever its own circuit had it.
            let at = to_world(found[0].position());
            let off = track.pts.iter().map(|&p| crate::track::xz_dist2(p, at)).fold(f32::MAX, f32::min).sqrt();
            assert!(off < 140.0, "{name} is {off} from the road");
        }
        assert_eq!(world.props.len(), wanted.len());
        for emitter in BORROWED.iter().flat_map(|b| b.2.iter()) {
            assert!(world.emitters.iter().any(|e| e.0 == *emitter), "{emitter}");
        }
        // The gates are shut and the fallen pillar isn't there yet.
        for (name, passable) in [("mmrocc", false), ("shootme", false), ("pilcol", true)] {
            assert_eq!(track.surfaces.get(name).map(|s| s.1), Some(passable), "{name}");
        }
        // The computer's cars are kept out of the shut lanes, and bricks are left in them.
        let gate = Site::on(&Track::plain(Layout::Gauntlet), FIELD_AT.0, FIELD_AT.1, 0.0);
        assert_eq!(track.lane(gate.s, 5.0), -2.0);
        assert_eq!(track.lane(gate.s - 100.0, 5.0), 5.0);
        assert!(world.bricks.iter().any(|b| b.1.distance(to_world(gate.at(14.0, 5.0, BRICK_HEIGHT))) < 0.1));
    }

    #[test]
    fn a_fit_lays_another_circuit_s_road_along_ours() {
        // A site heading north-west, and a road of another circuit's heading east.
        let site = Site { origin: Vec3::new(10.0, 20.0, 0.0), heading: 0.75 * std::f32::consts::PI, s: 0.0 };
        let fit = site.fit(Vec3::new(100.0, 200.0, -50.0), 0.0, 2.0);
        assert!(fit.point(Vec3::new(100.0, 200.0, -50.0)).distance(site.origin) < 1e-3);
        // A point further along that road and above it is as far along ours, twice over.
        let ahead = fit.point(Vec3::new(101.0, 200.0, -47.0));
        assert!(ahead.distance(site.at(2.0 * UNIT, 0.0, 6.0 * UNIT)) < 1e-3, "{ahead}");
        // One to its left is to our left.
        let left = fit.point(Vec3::new(100.0, 201.0, -50.0));
        assert!(left.distance(site.at(0.0, -2.0 * UNIT, 0.0)) < 1e-3, "{left}");
    }
}
