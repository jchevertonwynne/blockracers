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
//! The other circuits of the port's own have a few of the same hazards each, stood
//! the same way: the brick circuit the hammer, the ghost and the cannons; the figure
//! of eight the curse, the stones and the dragon; the helter skelter the ark, the
//! hammer before its jump, the cannons and the stones.
//!
//! Without the game's data there is nothing to borrow, and each is a plain circuit.

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

/// What a circuit takes from each race: models, then emitters.
type Borrowed = [(
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
)];

/// What the brick circuit, the figure of eight and the helter skelter take.
const BRICK_BORROWED: &Borrowed = &[
    ("RACEC0R0", &["rkhamm02"], &[]),
    ("RACEC1R0", &["ghostly"], &["ghsttrl"]),
];
const FIGURE_EIGHT_BORROWED: &Borrowed = &[
    ("RACEC2R0", &["prntdum"], &[]),
    ("RACEC2R2", &["dum02", "dum03", "dum04"], &[]),
];
const HELTER_BORROWED: &Borrowed = &[
    ("RACEC0R0", &["rkhamm02"], &[]),
    ("RACEC2R2", &["arktp", "dum02", "dum03", "dum04"], &[]),
];

/// What the arenas take.
const YARD_BORROWED: &Borrowed = &[("RACEC1R0", &["ghostly"], &["ghsttrl"])];
const CROSSROADS_BORROWED: &Borrowed = &[("RACEC2R0", &["prntdum"], &[])];
const RING_BORROWED: &Borrowed = &[("RACEC2R2", &["arktp"], &[])];

/// What the gauntlet takes.
const BORROWED: [(&str, &[&str], &[&str]); 10] = [
    ("RACEC0R0", &["rkhamm02"], &[]),
    (
        "RACEC0R1",
        &["crane", "block", "barrel", "barrel01", "barrel02"],
        &[],
    ),
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
/// How far round the lap the force field's gate is.
const FIELD_AT: f32 = 900.0;
/// How far from the middle of the road the legs of the hammer's frame stand, the
/// cannons' muzzles are and the ark's plinth is.
const FRAME_OUT: f32 = WALL + 3.0;
const CANNON_OUT: f32 = WALL + 5.0;
const ARK_OUT: f32 = WALL + 3.5;

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

    /// A mummy's curse is left on the road ahead of whoever comes first to `site`,
    /// at one of `drops`.
    fn curse(&mut self, places: &mut Places, site: Site, drops: [Site; 3]) {
        self.hazard(CURSE, Kind::CurseDrop, site.origin, 12.0);
        places.curse_drops = drops.map(|drop| drop.origin);
    }
}

/// Stands its hazards round `track`, which is `layout`'s, with the field of `circuit`.
/// `None` without the game's data, and `track` is then left as it was.
pub fn load(
    layout: Layout,
    track: &mut Track,
    circuit: Option<&str>,
) -> Option<(LoadedWorld, TrackEvents, Hazards, Stands)> {
    let borrowed = match layout {
        Layout::Brick => BRICK_BORROWED,
        Layout::FigureEight => FIGURE_EIGHT_BORROWED,
        Layout::Gauntlet => &BORROWED,
        Layout::HelterSkelter => HELTER_BORROWED,
        Layout::Yard => YARD_BORROWED,
        Layout::Crossroads => CROSSROADS_BORROWED,
        Layout::Ring => RING_BORROWED,
        Layout::Town => CROSSROADS_BORROWED,
    };
    let mut world = world::borrowed(circuit.unwrap_or("c0"), borrowed)?;
    // Everything is worked out on the circuit as it is unmirrored, in the game's
    // coordinates, and mirrored with the rest on its way into the world.
    let plain = Track::plain(layout);
    let mut places = Places::default();
    let mut b = Builder {
        track,
        world: &mut world,
        events: TrackEvents::default(),
        hazards: Vec::new(),
        stands: BrickMesh::default(),
    };
    let at = |s: f32, lat: f32| Site::along(&plain, s, lat);
    match layout {
        Layout::Brick => {
            b.hammer(at(60.0, 0.0));
            b.ghost(at(174.0, 0.0));
            b.cannons(at(985.0, 0.0));
        }
        Layout::FigureEight => {
            let drops = [(24.0, -3.0), (30.0, 3.0), (36.0, 0.0)].map(|(s, lat)| at(s, lat));
            b.curse(&mut places, at(10.0, 0.0), drops);
            // The dragon is over the crossing.
            b.dragon(at(467.0, 0.0), 70.0);
            b.stones([930.0, 950.0, 970.0].map(|s| at(s, 0.0)));
        }
        Layout::Gauntlet => gauntlet(&mut b, &mut places, &plain),
        Layout::HelterSkelter => {
            b.hammer(at(30.0, 0.0));
            b.stones([110.0, 122.0, 134.0].map(|s| at(s, 0.0)));
            b.ark(at(257.0, -ARK_OUT));
            b.cannons(at(700.0, 0.0));
        }
        // An arena's are where its cars come on them whichever way they go: the
        // yard's ghost in its square, the crossroads' cannons firing into its west
        // pocket at a car on its way in and its dragon over the east one, the
        // ring's ark on the plinth in its middle.
        Layout::Yard => b.ghost(Site::place(0.0, 0.0, 0.0)),
        Layout::Crossroads => {
            b.cannons(Site::place(-116.0, 0.0, TAU / 2.0));
            // Only a car in the pocket itself sets the dragon off.
            b.dragon(Site::place(118.0, 0.0, 0.0), 28.0);
        }
        Layout::Ring => b.ark(Site::place(0.0, 0.0, 0.0)),
        // The town's dragon is between the bus and the truck, and rains on a car
        // that comes through there.
        Layout::Town => b.dragon(Site::place(0.0, 0.0, 0.0), 24.0),
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

/// The gauntlet's hazards, each so far round the lap, in the order the road comes to
/// them: one in the middle of a row, on the straight between its two bends, and
/// another at one end of it.
fn gauntlet(b: &mut Builder, places: &mut Places, plain: &Track) {
    let at = |s: f32, lat: f32| Site::along(plain, s, lat);
    // A mummy's curse waits just past the line, and the temple's stones after it.
    let drops = [(30.0, -3.0), (38.0, 3.0), (46.0, 0.0)].map(|(s, right)| at(s, right));
    b.curse(places, at(15.0, 0.0), drops);
    b.stones([55.0, 77.0, 99.0].map(|s| at(s, 0.0)));

    // The aliens' saucer comes down the road at its lowest.
    let site = at(242.0, 0.0);
    b.place(
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
    b.hazard(UFO, grabber, site.origin, 90.0);

    b.hammer(at(314.0, 0.0));
    b.cannons(at(466.0, 0.0));

    // The moon's lava leaps between three pools, two of them at the road.
    let site = at(690.0, 0.0);
    let fit = site.fit(Vec3::new(550.0, -462.0, 6.0), 58.0, 1.0);
    b.place(&fit, &["mmlavbl"]);
    places.lava_pools = places.lava_pools.map(|pool| fit.point(pool));
    b.hazard(
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
        b.disc(pool, 4.0, ORANGE);
    }

    b.dragon(at(764.0, 0.0), 70.0);

    // The moon's force field shuts a lane to all but the shielded.
    let site = at(FIELD_AT, 0.0);
    let gate = at(FIELD_AT, WALL / 2.0);
    b.place(
        &gate.fit(Vec3::new(411.06, -116.03, 0.0), 0.0, 0.7),
        &["mmrocon", "mmrocof"],
    );
    b.hazard(FIELD, Kind::Rocket { open: false }, gate.origin, 105.0);
    b.shut_lane(site, FIELD_WALL);
    b.surface("mmrocc", FIELD_WALL, false);

    b.ark(at(988.0, ARK_OUT));
    b.ghost(at(1140.0, 0.0));

    // The desert's pillar falls across the right of the road as the first car comes.
    let site = at(1208.0, 1.0);
    let fit = site.fit(Vec3::new(842.0, 296.7, -60.0), 90.0, 1.0);
    b.place(&fit, &["piltop"]);
    b.events
        .trigger(to_world(site.at(-60.0, 0.0, 0.0)), 15.0, PILLAR, false);
    b.hazards.push((
        PILLAR,
        Kind::FallingPillar {
            fallen: false,
            landed: false,
        },
    ));
    b.plinth(fit.point(Vec3::new(902.5, 294.0, -0.5)), 3.0, TAN);
    // Where it lies once it has fallen.
    let corner = |x: f32, y: f32| fit.point(Vec3::new(x, y, -60.0));
    let corners = [
        corner(842.0, 292.0),
        corner(885.0, 292.0),
        corner(885.0, 301.0),
        corner(842.0, 301.0),
    ];
    for i in 0..4 {
        b.wall(PILLAR_WALL, corners[i], corners[(i + 1) % 4], 8.0);
    }
    b.surface("pilcol", PILLAR_WALL, true);
    b.keep_left(site, 45.0, 12.0, -2.5);

    // The dock's barrels shut a lane until a cannon, set off by a horn, clears them.
    let site = at(1350.0, 0.0);
    let gate = at(1350.0, WALL / 2.0);
    let barrels = ["block", "barrel", "barrel01", "barrel02"];
    b.place(
        &gate.fit(Vec3::new(-402.0, 513.5, 52.5), 57.5, 0.8),
        &barrels,
    );
    b.hazards.push((
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
    b.hazards.push((-1, launcher));
    b.cannon(muzzle, gate.origin);
    b.shut_lane(site, BARREL_WALL);
    b.surface("shootme", BARREL_WALL, false);

    // The dock's crane swings its load in from the left: the outside of the last row.
    let site = at(1581.0, 2.0);
    let fit = site.fit(Vec3::new(-492.0, -508.0, -76.3), 139.0, 1.0);
    b.place(&fit, &["crane"]);
    b.hazard(CRANE, Kind::Crane { pending: true }, site.origin, 70.0);
    b.plinth(fit.point(Vec3::new(-530.15, -569.28, -31.5)), 3.0, YELLOW);

    // A warp pad on the left of the road, and where it comes out: up on the viaduct.
    let (pad, out) = (at(1599.0, -4.0), at(1707.0, 0.0));
    b.events.trigger(to_world(pad.origin), 3.5, WARP, false);
    b.hazards.push((WARP, Kind::WarpPad));
    (places.warp_to, places.warp_facing) =
        (out.at(0.0, 0.0, 0.2), out.at(1.0, 0.0, 0.0) - out.origin);
    b.disc(pad.origin, 3.2, BLUE);

    // The ice planet's craft hangs over the top of the viaduct letting out smoke, and
    // it snows there.
    let site = at(1805.0, 0.0);
    b.place(
        &site.fit(Vec3::new(454.6, -530.6, -95.0), 21.2, 1.0),
        &["dp_def"],
    );
    b.hazard(SMOKE, Kind::SmokeVent { emitter: None }, site.origin, 70.0);
    b.events.trigger(to_world(site.origin), 100.0, SNOW, true);
    b.hazards.push((SNOW, Kind::Snowfall { emitter: None }));

    // The island's water lies beside it all, to the east, sloshing.
    let middle = Site {
        origin: Vec3::new(185.0, 0.0, 0.0) / UNIT,
        heading: TAU / 4.0,
        s: 0.0,
    };
    b.place(
        &middle.fit(Vec3::new(-722.5, -416.5, -15.9), 90.0, 1.0),
        &["water01"],
    );
    b.hazards.push((
        -1,
        Kind::Oscillator {
            prop: "water01".into(),
            amplitude: Vec2::new(0.2, 0.0),
            time: 0.0,
        },
    ));
    for along in [-54.0, 0.0, 54.0] {
        b.track
            .clearings
            .push((to_world(middle.at(along, 0.0, 0.0)), 50.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the original game data; silently passes without it.
    #[test]
    fn every_borrowed_model_is_found_and_moved_to_the_gauntlet() {
        let mut track = Track::built(Layout::Gauntlet);
        let Some((world, ..)) = load(Layout::Gauntlet, &mut track, None) else {
            return;
        };
        let wanted: Vec<&str> = BORROWED.iter().flat_map(|b| b.1.iter().copied()).collect();
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
        for emitter in BORROWED.iter().flat_map(|b| b.2.iter()) {
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
        let gate = Site::along(&Track::plain(Layout::Gauntlet), FIELD_AT, 0.0);
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
        for (layout, borrowed) in [
            (Layout::Brick, BRICK_BORROWED),
            (Layout::FigureEight, FIGURE_EIGHT_BORROWED),
            (Layout::HelterSkelter, HELTER_BORROWED),
        ] {
            let mut track = Track::built(layout);
            let Some((world, ..)) = load(layout, &mut track, None) else {
                return;
            };
            let wanted: Vec<&str> = borrowed.iter().flat_map(|b| b.1.iter().copied()).collect();
            assert_eq!(world.props.len(), wanted.len(), "{layout:?}");
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
                    "{layout:?} {} is {off} from the road",
                    prop.name()
                );
            }
            // The bricks are the ones the circuit has without its hazards.
            assert_eq!(world.bricks.len(), crate::items::laid(&track).len());
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
