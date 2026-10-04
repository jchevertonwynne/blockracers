//! Karts: the player's controls, AI drivers and race bookkeeping. The driving model
//! itself is in `physics`.

use crate::items::Power;
use crate::meshgen::*;
use crate::assets::materials::Surface;
use crate::physics::{self, MAX_SPEED, UNIT};
use crate::track::{Checkpoint, Track};
use crate::audio::{Sfx, id};
use crate::opponent::{RUBBER_BAND, RoutePlay};
use crate::racer_sounds::{Cues, RacerAudio};
use crate::menu::Settings;
use crate::net::{Lineup, Puppet, Remote, Role, Who};
use crate::world::{Chassis, KartModel, LoadedWorld};
use crate::{Phase, Race};
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

pub const WHEEL_RADIUS: f32 = 0.45;
const WARP_SHRINK: f32 = 0.25;
/// Where the warp's tunnel is shown, from where the car really is: out of sight of the circuit.
pub const TUNNEL: Vec3 = Vec3::new(0.0, -3000.0, 0.0);
/// Lateral acceleration the AI is willing to corner at; a little under what the tyres
/// hold before slip steering sets in.
const AI_LAT_ACCEL: f32 = 30.0;
/// Longest physics step; frames are split into steps no longer than this.
const MAX_STEP: f32 = 1.0 / 120.0;
/// A warp carries the kart along the racing line this fast, then drops it at this speed.
const WARP_SPEED: f32 = 600.0 * UNIT;
const WARP_EXIT_SPEED: f32 = 700.0 / 4500.0 * 1000.0 * UNIT;
/// A car on a recording stays blown up until it lands; this is only longer than that.
const ROUTE_SPIN_OUT: f32 = 60.0;
/// `PlayerControls::UpdateSteering`: how fast the wheels turn, a second, of full lock:
/// with a key held, back through the middle, in a slide and in a tight slide, and
/// towards where a stick is held.
const STEER_RATE: f32 = 1.25;
const STEER_RETURN_RATE: f32 = 8.25;
const STEER_DRIFT_RATE: f32 = 5.0;
const STEER_SLIDE_RATE: f32 = 16.0;
const STEER_IDLE_RATE: f32 = 2.5;
/// A floating car rises to this height at this rate, a recorded one coming down at
/// this; it leans up to this far, at this rate (`RacerCarBody::UpdateSlideBank`).
const HOVER_HEIGHT: f32 = 6.0 * UNIT;
const HOVER_RISE: f32 = 3.0 * UNIT;
const HOVER_FALL: f32 = 15.0 * UNIT;
const HOVER_BANK: f32 = std::f32::consts::FRAC_PI_4;
const HOVER_BANK_RATE: f32 = 5.0;
/// The strongest turbo dies away over this long, and may give out early once it has
/// this much or less of its burn left.
const TURBO_FADE: f32 = 0.7;
const TURBO_EARLY_END: f32 = 4.5;
/// A shove from a shield lasts this long (`Racer::ApplyShove`).
const SHOVE_TIME: f32 = 0.75;
/// `RacerPhysics::Update`: a car outside these heights, in game units, is put back
/// where it last stood.
const WORLD_HEIGHTS: (f32, f32) = (-250.0, 340.0);

struct Driver {
    name: &'static str,
    body: Color,
    accent: Color,
    skill: f32,
}

/// The grid slot the player's car has: the last of the roster.
pub const PLAYER_SLOT: usize = DRIVERS.len() - 1;

/// The player is last in the list, and so starts at the back of the grid.
const DRIVERS: &[Driver] = &[
    Driver { name: "Rocket Racer", body: WHITE, accent: RED, skill: 0.99 },
    Driver { name: "Captain Redbeard", body: BLACK, accent: WHITE, skill: 0.97 },
    Driver { name: "King Kahuka", body: YELLOW, accent: BROWN, skill: 0.95 },
    Driver { name: "Basil the Batlord", body: DARK_GREY, accent: BLUE, skill: 0.94 },
    Driver { name: "Johnny Thunder", body: GREEN, accent: TAN, skill: 0.92 },
    Driver { name: "You", body: RED, accent: YELLOW, skill: 1.0 },
];

#[derive(Component)]
pub struct Kart {
    pub name: std::borrow::Cow<'static, str>,
    pub slot: usize,
    /// Where the kart touches the ground, midway between the wheels.
    pub pos: Vec3,
    pub vel: Vec3,
    pub rot: Quat,
    /// Heading of the body, kept for the camera and the AI.
    pub yaw: f32,
    /// Direction of travel; lags behind the body when sliding.
    pub facing: Vec3,
    /// Wheels on the ground, and what they are standing on.
    pub contacts: u8,
    pub ground_normal: Vec3,
    pub surface: Surface,
    pub wall_contact: bool,
    pub air_time: f32,
    /// Powersliding, and whether it is the tight slide of brake and accelerator together.
    pub sliding: bool,
    pub slide_tight: bool,
    /// A slide has been asked for and not yet let go of (`Racer::c_flagDrifting`).
    pub drifting: bool,
    /// How much of the tyres' hold the last slip steering gave up.
    pub slip_ratio: f32,
    /// The turn the car was last set to, signed.
    pub turn_radius: f32,
    /// A wall is turning the car: time left, and how fast.
    pub yaw_impulse: f32,
    pub yaw_kick: f32,
    /// The turbo in use has met a wall, and is at half strength.
    pub turbo_weak: bool,
    /// A curse's hold on the accelerator: time until it changes, and what it adds.
    pub curse_timer: f32,
    pub curse_throttle: f32,
    /// Where the car last had all four wheels down, clear of any wall.
    pub safe: (Vec3, Quat),
    /// A push from a shielded car: its acceleration and the time left of it.
    pub shove: (Vec3, f32),
    pub(crate) noise: u32,
    /// Turning tighter than the tyres can hold.
    pub slipping: bool,
    /// Smoothed steering input, -1..1 (positive is left).
    pub steer: f32,
    pub wheel_angle: f32,
    pub top_factor: f32,

    // Track-space state.
    pub idx: usize,
    pub s: f32,
    pub lat: f32,
    pub lap: i32,
    pub progress: f32,
    pub place: usize,
    pub finished: Option<f32>,
    /// When the car was put out of an elimination race.
    pub out: Option<f32>,

    // Power-ups and their effects (timers in seconds).
    pub held: Option<Power>,
    /// White bricks collected: the level the held power-up will fire at.
    pub whites: u8,
    /// White bricks knocked out of the car since the bricks last looked.
    pub white_drops: u8,
    /// A coloured brick has just been taken.
    pub collected: bool,
    /// Whirling round on the spot.
    pub spin: f32,
    /// How fast, in radians a second.
    pub spin_rate: f32,
    /// Blown into the air, with no control.
    pub spin_out: f32,
    pub boost: f32,
    /// Level of the turbo in use.
    pub boost_level: u8,
    /// Time until this kart may make another scraping sound.
    pub scrape_cooldown: f32,
    pub shield: f32,
    pub shield_level: u8,
    /// Steering reversed and top speed halved.
    pub cursed: f32,
    /// Held in place by a magnet.
    pub magnet: f32,
    /// Hurtling along the racing line, out of harm's way.
    pub warp: f32,
    /// A warp opening: time until it carries the kart off.
    pub warp_start: f32,
    /// Where a warp that doesn't follow the road is taking the car: from, to, and the
    /// way it faces when it gets there (a warp pad's, `WarpAction`).
    pub warp_to: Option<(Vec3, Vec3, Vec3)>,
    /// What the car's colours are multiplied by: the dark of a tunnel, the glow of lava
    /// (`ColorTransformResource`).
    pub tint: Vec3,
    /// Extra acceleration for the coming physics step (a grappling hook's pull).
    pub external_force: Vec3,

    // Race rules.
    /// Last checkpoint gate crossed, and whether that was in the right direction.
    pub checkpoint: Option<usize>,
    pub checkpoint_forward: bool,
    /// Times gate 0 has been passed forwards, less one.
    pub checkpoint_count: i32,
    pub crossed_backward: bool,
    /// The lap zone the kart is in and the two before it. Zone 1 is the finish line,
    /// 2 the stretch after it and 0 the rest of the lap.
    pub zones: [u8; 3],

    // The car itself.
    pub wheels: [Vec3; 4],
    pub body: [Vec3; 4],
    /// Half-width, and the Z of the car's nose and tail, for bumping into other cars.
    pub outline: [f32; 3],
    pub stats: Stats,
    /// How high the engine revs, from the chassis table.
    pub engine_pitch: f32,
    /// Sounds owed for things that have just happened to this kart.
    pub cues: Cues,
    /// The event of a pass-through surface the kart has just driven through.
    pub touched: Option<i32>,
    /// The car has touched a surface that ends its race.
    pub ended: bool,
    /// The horn has just sounded.
    pub honked: bool,
    /// Where this kart has just struck another.
    pub sparks: Option<Vec3>,
    /// The recorded drive this car plays back, if it is one of the computer's.
    pub route: Option<crate::opponent::RoutePlay>,
    /// The player's car, its race run, making its way onto a recording.
    pub returning: Option<Return>,
    /// Floating clear of the road: the strongest turbo's doing, or a magnet's
    /// (`Racer::Halt`). How high it has risen, and how far it leans.
    pub hover: bool,
    pub hover_lift: f32,
    pub hover_bank: f32,
}

/// Multipliers from the car's handling, top speed and acceleration ratings.
#[derive(Clone, Copy)]
pub struct Stats {
    pub handling: f32,
    pub top_speed: f32,
    pub acceleration: f32,
}

impl Stats {
    /// From ratings of 0 to 100, as the original scales them.
    pub fn from_ratings([handling, top_speed, acceleration]: [f32; 3]) -> Self {
        Stats {
            handling: 0.7 + 0.003 * handling,
            top_speed: 1.0 - (50.0 - top_speed) * 0.001,
            acceleration: 1.0 - (50.0 - acceleration) * 0.001,
        }
    }
}

impl Kart {
    pub(crate) fn new(track: &Track, slot: usize) -> Self {
        // The circuit's own grid if it has one (the player starts from slot 0, at the
        // back); otherwise two columns behind the line.
        let grid_slot = (slot + 1) % DRIVERS.len();
        let (pos, dir) = match track.course.grid.get(grid_slot) {
            Some(&(pos, dir)) if track.course.grid.len() >= DRIVERS.len() => {
                let ground = track.collision.ground(pos + Vec3::Y * 2.0, 8.0);
                (ground.map_or(pos, |hit| hit.point), dir)
            }
            _ => {
                let s = -8.0 - (slot / 2) as f32 * 6.0;
                let lat = track.road * if slot.is_multiple_of(2) { -0.375 } else { 0.375 };
                (track.surface_point(s, lat), track.sample(s).2.cross(Vec3::NEG_Y).normalize())
            }
        };
        let yaw = (-dir.x).atan2(-dir.z);
        let (idx, s, lat) = track.project(pos, track.nearest(pos));
        Kart {
            name: DRIVERS[slot].name.into(),
            slot,
            pos,
            vel: Vec3::ZERO,
            rot: Quat::from_rotation_y(yaw),
            yaw,
            facing: dir,
            contacts: 4,
            ground_normal: Vec3::Y,
            surface: Surface::default(),
            wall_contact: false,
            air_time: 0.0,
            sliding: false,
            slide_tight: false,
            drifting: false,
            slip_ratio: 0.0,
            turn_radius: 0.0,
            yaw_impulse: 0.0,
            yaw_kick: 0.0,
            turbo_weak: false,
            curse_timer: 0.0,
            curse_throttle: 0.0,
            safe: (pos, Quat::from_rotation_y(yaw)),
            shove: (Vec3::ZERO, 0.0),
            noise: 0x9e37_79b9 ^ slot as u32,
            slipping: false,
            steer: 0.0,
            wheel_angle: 0.0,
            top_factor: 1.0,
            idx,
            s,
            lat,
            lap: 0,
            progress: 0.0,
            place: slot + 1,
            finished: None,
            out: None,
            held: None,
            whites: 0,
            white_drops: 0,
            collected: false,
            spin: 0.0,
            spin_rate: physics::SPIN_RATE,
            spin_out: 0.0,
            boost: 0.0,
            boost_level: 0,
            scrape_cooldown: 0.0,
            shield: 0.0,
            shield_level: 0,
            cursed: 0.0,
            magnet: 0.0,
            warp: 0.0,
            warp_start: 0.0,
            warp_to: None,
            tint: Vec3::ONE,
            external_force: Vec3::ZERO,
            checkpoint: None,
            checkpoint_forward: true,
            checkpoint_count: -1,
            crossed_backward: false,
            zones: [0, 2, 1],
            wheels: physics::WHEELS,
            body: physics::BODY_POINTS,
            outline: [1.2, -1.6, 1.6],
            stats: Stats::from_ratings([50.0; 3]),
            engine_pitch: 1.0,
            cues: Cues::default(),
            touched: None,
            ended: false,
            honked: false,
            sparks: None,
            route: None,
            returning: None,
            hover: false,
            hover_lift: 0.0,
            hover_bank: 0.0,
        }
    }

    pub fn reset(&mut self, track: &Track) {
        let (wheels, body, outline, stats, engine_pitch) = (self.wheels, self.body, self.outline, self.stats, self.engine_pitch);
        let (name, mut route) = (self.name.clone(), self.route.take());
        if let Some(route) = &mut route {
            route.restart();
        }
        *self = Kart { wheels, body, outline, stats, engine_pitch, name, route, ..Kart::new(track, self.slot) };
    }

    /// Takes the car's contact points, footprint and ratings from the chassis table.
    fn set_chassis(&mut self, chassis: &Chassis, outline: [f32; 3]) {
        self.outline = [outline[0] * UNIT, -outline[1] * UNIT, outline[2] * UNIT];
        // The game's cars have X forward and Y left; ours face -Z with X to the right.
        let local = |v: Vec3| Vec3::new(-v.y, 0.0, -v.x) * UNIT;
        self.wheels = chassis.wheels.map(local);
        let half = chassis.footprint * 0.5;
        self.body = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].map(|(x, z)| {
            Vec3::new(x * half.x * UNIT, physics::BODY_POINT_HEIGHT, z * half.y * UNIT)
        });
        self.stats = Stats::from_ratings(chassis.stats);
        self.engine_pitch = chassis.engine_pitch;
    }

    /// Sets the kart down on the road at rest, pointing along the racing line.
    #[cfg(test)]
    pub(crate) fn place(&mut self, track: &Track, s: f32, lat: f32) {
        let dir = track.sample(s).1.with_y(0.0).normalize();
        self.pos = track.surface_point(s, lat) + Vec3::Y * 0.2;
        self.vel = Vec3::ZERO;
        self.yaw = (-dir.x).atan2(-dir.z);
        self.rot = Quat::from_rotation_y(self.yaw);
        self.facing = dir;
        self.air_time = 0.0;
        (self.idx, self.s, self.lat) = track.project(self.pos, track.nearest(self.pos));
    }

    pub fn shielded(&self) -> bool {
        self.shield > 0.0
    }

    /// Something that would hurt arrives. Returns false if a shield, or being in warp,
    /// keeps it out.
    fn vulnerable(&self) -> bool {
        !self.shielded() && self.warp <= 0.0
    }

    /// Whirls the kart round `turns` times, unless protected.
    pub fn spin_round(&mut self, turns: f32) {
        self.spin_at(turns, physics::SPIN_RATE);
    }

    /// `RacerPhysics::StartSpin`: `turns` times round at `rate` radians a second.
    pub fn spin_at(&mut self, turns: f32, rate: f32) {
        if self.vulnerable() && self.spin <= 0.0 {
            (self.spin, self.spin_rate) = (turns * TAU / rate, rate);
        }
    }

    /// Stops the kart dead and throws it forwards and up with `force` of the full
    /// throw, unless protected (`PowerupExplosion::OnEvent`, `LightningAction::OnHitRacer`).
    /// A car on a recording hops where it is instead (`RacerPhysics::StartSpinOut`).
    pub fn launch(&mut self, force: f32) -> bool {
        if !self.vulnerable() {
            return false;
        }
        // A car on a recording is blown up until it comes back down.
        self.spin_out = if self.route.is_some() { ROUTE_SPIN_OUT } else { physics::SPIN_OUT_TIME };
        match &mut self.route {
            Some(route) => route.blow(),
            None => {
                self.vel = (self.facing * physics::LAUNCH_FORWARD_SPEED + Vec3::Y * physics::LAUNCH_UP_SPEED) * force;
                self.contacts = 0;
            }
        }
        true
    }

    /// `Racer::AttachCurse`: cursed for so long, which ends any turbo.
    pub fn curse(&mut self, time: f32) {
        (self.cursed, self.boost) = (time, 0.0);
    }

    /// `Racer::ApplyShove`: pushed for a while, unless being pushed already.
    pub fn shove_with(&mut self, push: Vec3) {
        if self.shove.1 <= 0.0 {
            self.shove = (push, SHOVE_TIME);
        }
    }

    fn random(&mut self, below: u32) -> u32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        self.noise % below
    }

    /// `RacerPhysics::StartBoost`: a cursed car gets no turbo.
    pub fn start_boost(&mut self, level: u8) {
        if self.cursed <= 0.0 {
            (self.boost, self.boost_level) = (crate::items::TURBO_TIMES[level.min(2) as usize], level);
            // `Racer::StartTurbo`: at full strength, and going the way the car points.
            self.turbo_weak = false;
            self.facing = (self.rot * Vec3::NEG_Z).normalize_or(self.facing);
        }
    }

    /// Held by a magnet and brought to rest (`Racer::Halt`).
    pub fn halted(&self) -> bool {
        self.magnet > 0.0 && self.vel.length() <= 2.0 * UNIT
    }

    /// `Racer::DropWhiteBrick`: a hit knocks one of the white bricks out onto the road.
    pub fn drop_white(&mut self) {
        if self.whites > 0 {
            self.whites -= 1;
            self.white_drops += 1;
        }
    }

    pub fn display_lap(&self, laps: i32) -> i32 {
        self.lap.clamp(1, laps)
    }

    /// Whether the last gate was crossed backwards.
    pub fn wrong_way(&self) -> bool {
        self.checkpoint.is_some() && !self.checkpoint_forward
    }

    /// The race rules, applied to the move from `from` to where the kart is now.
    fn follow_course(&mut self, track: &Track, from: Vec3) {
        let course = &track.course;
        let lift = Vec3::Y * physics::BODY_POINT_HEIGHT;
        let travel = self.pos - from;
        if let Some(hit) = course.gates.any(from + lift, self.pos + lift)
            && let Some(gate) = course.checkpoints.get(hit.tag) {
                self.cross_checkpoint(hit.tag, gate, travel.dot(gate.normal) < 0.0);
            }
        for &(centre, radius, zone) in &course.zones {
            if centre.distance_squared(self.pos) < radius * radius {
                self.enter_zone(zone);
            }
        }
        if course.finish.any(from + lift, self.pos + lift).is_some()
            && course.checkpoints.first().is_none_or(|c| travel.dot(c.normal) < 0.0)
        {
            // `Racer::CrossFinishLine`: the line counts only if the kart got here by
            // way of the zone after the line and then the rest of the lap. The grid
            // starts in that state, so the first crossing begins lap one.
            if self.zones == [0, 2, 1] {
                self.lap += 1;
            }
            self.enter_zone(1);
        }

        // Race order: gates passed, plus how far towards the next one.
        self.progress = match self.checkpoint.and_then(|i| course.checkpoints.get(i)) {
            Some(gate) => {
                let next = gate.next.first().and_then(|&n| course.checkpoints.get(n));
                let towards = next.map_or(0.0, |next| {
                    let leg = next.position - gate.position;
                    let step = (next.fraction - gate.fraction).rem_euclid(1.0);
                    step * ((self.pos - gate.position).dot(leg) / leg.length_squared()).clamp(0.0, 0.99)
                });
                self.checkpoint_count as f32 + gate.fraction + towards
            }
            // Still on the grid: nearest the first gate leads.
            None => course.checkpoints.first().map_or(0.0, |c| {
                -1.0 - c.position.distance(self.pos) / track.length
            }),
        };
    }

    /// `Racer::OnCheckpointCrossed`: only gate 0 advances the count, and crossing it
    /// backwards has to be undone before it will count again.
    fn cross_checkpoint(&mut self, index: usize, gate: &Checkpoint, forward: bool) {
        if self.checkpoint == Some(index) && self.checkpoint_forward == forward {
            return;
        }
        if gate.fraction == 0.0 {
            if !forward {
                self.crossed_backward = true;
                self.checkpoint_forward = false;
                self.checkpoint = Some(index);
                return;
            }
            if !self.crossed_backward {
                self.checkpoint_count += 1;
            }
            self.crossed_backward = false;
        } else if self.crossed_backward {
            self.checkpoint_count -= 1;
            self.crossed_backward = false;
        }
        self.checkpoint = Some(index);
        self.checkpoint_forward = forward;
    }

    fn enter_zone(&mut self, zone: u8) {
        if self.zones[0] != zone {
            self.zones = [zone, self.zones[0], self.zones[1]];
        }
    }
}

/// `DriveController::StartReturnToPath`: the recording being made for, how far along
/// its lap the place aimed at is, that place and the way the recording faces there,
/// and getting unstuck.
pub struct Return {
    record: std::sync::Arc<crate::assets::route::Record>,
    ahead: f32,
    target: Vec3,
    heading: Vec3,
    stuck: f32,
    reversing: bool,
}

/// `DriveController::UpdateReturnToPath`: the place aimed at is reached this close,
/// moved on when this close (or pointing the wrong way) but not from farther than
/// this, by this much of the recording; the car goes at this part of full thrust.
const RETURN_ARRIVE: f32 = 3.0 * UNIT;
const RETURN_NEAR: f32 = 30.0 * UNIT;
const RETURN_FAR: f32 = 80.0 * UNIT;
const RETURN_STEP: f32 = 250.0;
const RETURN_START: f32 = 1000.0;
const RETURN_THRUST: f32 = 18.0 / 54.0;
/// `UpdateStuckDetection`: slower than this for a second, the car backs up for two.
const STUCK_SPEED: f32 = 9.0 * UNIT;

impl Return {
    fn new(record: std::sync::Arc<crate::assets::route::Record>) -> Self {
        let (target, heading) = RoutePlay::preview(&record, RETURN_START);
        Return { record, ahead: RETURN_START, target, heading, stuck: 0.0, reversing: false }
    }

    /// The turn and thrust that take the car on towards the recording, or the
    /// recording itself once the car is on it.
    fn drive(&mut self, k: &Kart, dt: f32) -> Result<(f32, f32), RoutePlay> {
        let delta = self.target - k.pos;
        let distance = delta.length();
        if distance < RETURN_ARRIVE {
            return Err(RoutePlay::at_loop(self.record.clone(), self.ahead));
        }
        self.stuck += dt;
        let forward_speed = k.vel.dot(k.facing);
        if k.spin > 0.0 {
            (self.stuck, self.reversing) = (0.0, false);
        } else if self.reversing {
            if self.stuck >= 2.0 {
                (self.stuck, self.reversing) = (0.0, false);
            }
        } else if forward_speed.abs() > STUCK_SPEED {
            self.stuck = 0.0;
        } else if self.stuck >= 1.0 {
            (self.stuck, self.reversing) = (0.0, true);
        }
        // The circle that leaves the car's nose and passes through the place.
        let left = k.rot * Vec3::NEG_X;
        let to_left = left.dot(delta) >= 0.0;
        let closing = left.dot(delta / distance).abs();
        let mut radius = if closing < 0.0005 { 4096.0 * UNIT } else { distance / (2.0 * closing) };
        if !to_left {
            radius = -radius;
        }
        let mut thrust = RETURN_THRUST;
        if self.reversing {
            (radius, thrust) = (-radius, -1.0);
        }
        let pointing = if k.contacts >= 3 { k.rot * Vec3::NEG_Z } else { k.facing };
        if (distance < RETURN_NEAR || self.heading.dot(pointing) < 0.5) && distance <= RETURN_FAR {
            self.ahead += RETURN_STEP;
            (self.target, self.heading) = RoutePlay::preview(&self.record, self.ahead);
        }
        Ok((radius, thrust))
    }
}

#[derive(Component, Default)]
pub struct Controls {
    pub throttle: f32,
    pub steer: f32,
    /// The slide is asked for: the slide key with the accelerator down.
    pub drift: bool,
    /// The brake is down too, which makes a slide the tight one.
    pub tight: bool,
    pub use_item: bool,
    /// A turbo start has just been earned, of this strength.
    pub start_boost: Option<u8>,
    /// `DriveController::UpdateReturnToPath` sets the turn and the thrust themselves:
    /// the turn's radius, to the left if positive, and the thrust as a part of full.
    pub course: Option<(f32, f32)>,
    /// Steering is taken as it is given, not turned at the original's rates: for the
    /// port's own driver, which the original has none of, and its quick steering.
    pub direct: bool,
}

#[derive(Component)]
pub struct Player;

/// Every kart has one; the player's only takes over once they've finished.
#[derive(Component)]
pub struct Ai {
    skill: f32,
    lane: f32,
    lane_timer: f32,
    /// `Racer::UpdateTimers`: time since the driver last thought about the brick it
    /// holds, and how long it leaves between thoughts.
    check: f32,
    interval: f32,
    /// Out of 256, how likely the driver is to use a red, yellow, green or blue brick
    /// when it thinks of it.
    chances: [u32; 4],
    /// The colour the driver saves white bricks for, and how many.
    charge: Option<(Power, u8)>,
    /// Time until the driver may sound its horn at a car ahead.
    taunt: f32,
    /// The race has begun for this driver (`Racer::OnRaceStart`).
    started: bool,
    /// Time spent going nowhere, and time left backing out of it.
    stuck: f32,
    reversing: f32,
}

#[derive(Component)]
pub struct Wheel {
    pub kart: Entity,
    /// Front wheels turn with the steering.
    pub front: bool,
    pub rest: Quat,
    /// In the parent's space.
    pub steer_axis: Vec3,
    /// In the wheel's own space; turning about it by the kart's wheel angle rolls forward.
    pub spin_axis: Vec3,
    /// Spin rate relative to the brick kart's wheels, for wheels of another size.
    pub spin_ratio: f32,
}

#[derive(Component)]
pub struct Shield;

/// `Racer::Initialize`: a driver's keenness on a colour, 0 to 100, as a chance in 256.
fn chance(keenness: i32) -> u32 {
    ((keenness as f32 * 0.8 * 100.0 * 0.011_111_111 * 0.01 + 0.2).min(1.0) * 255.0) as u32
}

/// How often a driver thinks about the brick it holds; how long it waits after
/// picking one up, and after deciding against using it.
const AI_CHECK: f32 = 0.15;
const AI_CHECK_COLLECTED: f32 = 0.3;
const AI_CHECK_PUT_OFF: f32 = 1.0;
/// A driver only fires a red brick with a car this far ahead, in this cone.
const AI_RED_TARGET: (f32, f32, f32) = (10.0 * UNIT, 250.0 * UNIT, 0.96);
/// A driver sounds its horn at a car within this distance ahead, in this cone, and
/// looks for one this often.
const AI_TAUNT_TARGET: (f32, f32) = (13.0 * UNIT, 0.3);
const AI_TAUNT_WAIT: f32 = 2.0;
/// The field is not paced against the player until the race is this old (`RaceSetup`).
const RUBBER_BAND_DELAY: f32 = 15.0;

impl Ai {
    /// Takes the driver's habits from the game's table.
    fn knows(&mut self, driver: &crate::roster::Driver) {
        self.chances = driver.keenness.map(chance);
        let colour = match driver.charge.0 {
            1 => Some(Power::Red),
            2 => Some(Power::Blue),
            3 => Some(Power::Green),
            4 => Some(Power::Yellow),
            _ => None,
        };
        self.charge = colour.map(|colour| (colour, driver.charge.1.clamp(0, 3) as u8));
    }

    fn new(skill: f32, slot: usize) -> Self {
        Ai {
            skill,
            lane: 0.0,
            lane_timer: slot as f32 * 0.7,
            check: 0.0,
            interval: AI_CHECK,
            chances: [50; 4].map(chance),
            charge: None,
            taunt: AI_TAUNT_WAIT + slot as f32,
            started: false,
            stuck: 0.0,
            reversing: 0.0,
        }
    }

    /// Follows the racing line: steering, throttle and getting unstuck.
    fn drive(&mut self, k: &Kart, c: &mut Controls, track: &Track, rng: &mut Rng, dt: f32) {
        self.lane_timer -= dt;
        if self.lane_timer <= 0.0 {
            self.lane = track.road * rng.range(-0.6, 0.6);
            self.lane_timer = rng.range(2.0, 5.0);
        }

        // Steer at a point a little way up the road.
        let speed = k.vel.length();
        // Where part of the road is shut, the open part is steered for.
        let ahead = k.s + 8.0 + speed * 0.4;
        let to = track.point(ahead, track.lane(ahead, self.lane)) - k.pos;
        let err = wrap_angle((-to.x).atan2(-to.z) - k.yaw);
        c.steer = (err * 3.0).clamp(-1.0, 1.0);

        // Brake for the tightest corner coming up.
        let tightest = (2..18).map(|j| track.curv[(k.idx + j) % track.n()]).fold(1e-4, f32::max);
        c.throttle = if speed > (AI_LAT_ACCEL / tightest).sqrt() { -0.6 } else { 1.0 };
        (c.drift, c.tight, c.direct) = (false, false, true);

        // Wedged against something: back out with the wheels turned the other way.
        self.stuck = if speed < 2.0 && k.spin <= 0.0 { self.stuck + dt } else { 0.0 };
        if self.stuck > 1.0 {
            self.stuck = 0.0;
            self.reversing = 1.5;
        }
        if self.reversing > 0.0 {
            self.reversing -= dt;
            c.throttle = -1.0;
            c.steer = -c.steer;
        }
    }
}

fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

fn kart_mesh(body: Color, accent: Color) -> Mesh {
    let mut b = BrickMesh::default();
    let id = Quat::IDENTITY;
    let v = Vec3::new;
    // Chassis, bumper, nose and body. Forward is -Z.
    b.cuboid(v(0.0, 0.4, 0.0), v(0.7, 0.1, 1.5), id, DARK_GREY);
    b.cuboid(v(0.0, 0.4, -1.55), v(0.75, 0.1, 0.1), id, GREY);
    b.brick(v(0.0, 0.65, -0.95), v(0.45, 0.15, 0.55), id, body, (2, 2));
    b.cuboid(v(0.0, 0.7, 0.5), v(0.7, 0.2, 0.9), id, body);
    // Seat back, engine block and rear wing.
    b.cuboid(v(0.0, 1.1, 0.72), v(0.5, 0.3, 0.08), id, DARK_GREY);
    b.brick(v(0.0, 1.05, 1.1), v(0.6, 0.15, 0.3), id, accent, (3, 1));
    for x in [-0.5, 0.5] {
        b.cuboid(v(x, 1.3, 1.35), v(0.06, 0.25, 0.06), id, GREY);
    }
    b.cuboid(v(0.0, 1.58, 1.4), v(0.85, 0.04, 0.22), id, body);
    // Steering wheel.
    b.cuboid(v(0.0, 1.05, -0.3), v(0.18, 0.18, 0.03), Quat::from_rotation_x(-0.5), BLACK);
    // Minifigure: torso, arms, hands, head, face and helmet.
    b.cuboid(v(0.0, 1.15, 0.35), v(0.28, 0.25, 0.16), id, accent);
    for x in [-0.36, 0.36] {
        b.cuboid(v(x, 1.2, 0.08), v(0.07, 0.07, 0.3), id, accent);
        b.cuboid(v(x * 0.7, 1.18, -0.22), v(0.07, 0.07, 0.07), id, YELLOW);
        b.cuboid(v(x * 0.2, 1.56, 0.145), v(0.025, 0.03, 0.01), id, BLACK);
    }
    b.cyl(v(0.0, 1.4, 0.35), 0.2, 0.3, id, YELLOW);
    b.cyl(v(0.0, 1.66, 0.35), 0.24, 0.16, id, body);
    b.cyl(v(0.0, 1.82, 0.35), 0.1, 0.07, id, body);
    b.build()
}

fn wheel_mesh() -> Mesh {
    let mut b = BrickMesh::default();
    let along_x = Quat::from_rotation_z(-FRAC_PI_2);
    b.cyl(Vec3::X * -0.2, WHEEL_RADIUS, 0.4, along_x, BLACK);
    b.cyl(Vec3::X * -0.23, 0.26, 0.46, along_x, GREY);
    // Spokes so that the spin is visible.
    b.cuboid(Vec3::ZERO, Vec3::new(0.24, 0.3, 0.05), Quat::IDENTITY, WHITE);
    b.cuboid(Vec3::ZERO, Vec3::new(0.24, 0.05, 0.3), Quat::IDENTITY, WHITE);
    b.build()
}

pub fn spawn_karts(
    mut commands: Commands,
    track: Res<Track>,
    settings: Res<Settings>,
    variant: Res<crate::variant::Variant>,
    (role, lineup): (Res<Role>, Option<Res<Lineup>>),
    mut loaded: Option<ResMut<LoadedWorld>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let plastic = materials.add(StandardMaterial {
        perceptual_roughness: 0.35,
        ..default()
    });
    let wheel = meshes.add(wheel_mesh());
    let shield_mesh = meshes.add(Sphere::new(2.3));
    let shield_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.2, 0.5, 1.0, 0.3),
        emissive: LinearRgba::new(0.0, 0.2, 0.8, 1.0),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    // The original cars, if the game data provided them.
    let models = loaded.as_mut().map(|l| std::mem::take(&mut l.karts)).unwrap_or_default();
    let mut models: Vec<Option<KartModel>> = models.into_iter().map(Some).collect();

    // The player is the last driver on the roster; opponents fill it from the front.
    // Online the host has said who sits where.
    let lineup = lineup.filter(|_| *role != Role::Offline);
    for (slot, driver) in DRIVERS.iter().enumerate() {
        let seat = match &lineup {
            Some(lineup) => lineup.driver(slot),
            None if slot == PLAYER_SLOT => Some((Who::Local, None)),
            None if slot < settings.field() => Some((Who::Computer, None)),
            None => None,
        };
        let Some((who, called)) = seat else { continue };
        let human = who != Who::Computer;
        // What a host's player sees of every car but their own is what the host says of it.
        let shown = *role == Role::Client && who != Who::Local;
        let model = models.get_mut(slot).and_then(Option::take);
        let mut state = Kart::new(&track, slot);
        if let Some(model) = &model {
            state.set_chassis(&model.chassis, model.outline);
        }
        // The game's own field: its drivers, and for the computer's the drives recorded
        // for them.
        let mut skill = driver.skill;
        if let Some(loaded) = &loaded {
            if let Some(entry) = loaded.field.get(slot) {
                state.name = if human { driver.name.into() } else { entry.name.into() };
            }
            // There are no recordings of a circuit driven backwards.
            if let Some(record) = loaded.routes.get(slot).filter(|_| !human && !shown && !variant.reverse) {
                state.route = Some(RoutePlay::new(record.clone()));
                state.play_route(0.0);
                skill = 1.0;
            }
        }
        if let Some(called) = called {
            state.name = called.into();
        }
        let mut habits = Ai::new(skill * settings.ai_pace(), slot);
        if let Some(entry) = loaded.as_ref().and_then(|loaded| loaded.field.get(slot)).filter(|_| !human) {
            habits.knows(entry);
        } else if human {
            // `RaceState::CreateRacer`: a player's own driver is keen on everything.
            habits.chances = [100; 4].map(chance);
        }
        let mut kart = commands.spawn((
            state,
            Controls::default(),
            habits,
            RacerAudio::default(),
            crate::kart_effects::Effects::default(),
            Transform::default(),
            Visibility::default(),
        ));
        match who {
            Who::Local => drop(kart.insert(Player)),
            Who::Remote(peer) if !shown => drop(kart.insert(Remote::new(peer))),
            _ => {}
        }
        if shown {
            kart.insert(Puppet);
        }
        let id = kart.id();
        kart.with_child((
            Shield,
            Mesh3d(shield_mesh.clone()),
            MeshMaterial3d(shield_mat.clone()),
            Transform::from_xyz(0.0, 0.9, 0.0),
            Visibility::Hidden,
        ));

        let Some(model) = model else {
            // Brick-built stand-in.
            kart.insert((
                Mesh3d(meshes.add(kart_mesh(driver.body, driver.accent))),
                MeshMaterial3d(plastic.clone()),
            ));
            for (x, z) in [(-0.95, -1.0), (0.95, -1.0), (-0.95, 1.0), (0.95, 1.0)] {
                kart.with_child((
                    Wheel {
                        kart: id,
                        front: z < 0.0,
                        rest: Quat::IDENTITY,
                        steer_axis: Vec3::Y,
                        spin_axis: Vec3::X,
                        spin_ratio: 1.0,
                    },
                    Mesh3d(wheel.clone()),
                    MeshMaterial3d(plastic.clone()),
                    Transform::from_xyz(x, WHEEL_RADIUS, z),
                ));
            }
            continue;
        };

        crate::time_race::dress(&mut commands, id, model, &mut meshes, &mut materials, &mut images, None);
    }
}

/// The turbo start, as `PlayerControls::TryStartBoost`: pressing the accelerator just
/// before the off, or just after it, fires a turbo. The press before only counts if
/// the accelerator had been left alone for a while.
#[derive(Default)]
pub struct StartBoost {
    /// Time left in which the off (or a press, after it) fires the turbo.
    window: f32,
    since_press: f32,
    racing: bool,
}

/// How long a press stays good for, and how much of that must be left for the
/// stronger turbo.
const BOOST_WINDOW: f32 = 0.1;
const BOOST_WINDOW_STRONG: f32 = 0.06;
/// A press on the grid counts only this long after the one before.
const BOOST_REST: f32 = 2.0;

pub fn player_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    race: Res<Race>,
    settings: Res<Settings>,
    pause: Res<crate::Pause>,
    photo: Res<crate::replay::Photo>,
    mut start: Local<StartBoost>,
    mut q: Query<(&mut Kart, &mut Controls), With<Player>>,
) {
    let Ok((kart, mut c)) = q.single_mut() else { return };
    c.start_boost = None;
    // The keys are the pause menu's while it is up, and the camera's in photo mode.
    if pause.0.is_some() || photo.0.is_some() {
        return;
    }
    if kart.finished.is_some() || race.demo {
        *c = Controls::default();
        return;
    }
    let axis = |pos: [KeyCode; 2], neg: [KeyCode; 2]| {
        keys.any_pressed(pos) as i32 as f32 - keys.any_pressed(neg) as i32 as f32
    };
    // `PlayerControls::UpdateThrottle`: a slide is the slide key with the accelerator
    // down, and holds the accelerator full on; without it, accelerator and brake
    // together are half throttle.
    let (go, stop) = (keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]), keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]));
    c.steer = axis([KeyCode::KeyA, KeyCode::ArrowLeft], [KeyCode::KeyD, KeyCode::ArrowRight]);
    c.drift = go && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    c.tight = stop;
    c.throttle = match (go, stop) {
        (true, _) if c.drift => 1.0,
        (true, true) => 0.5,
        (go, stop) => go as i32 as f32 - stop as i32 as f32,
    };
    // The port's quick steering takes the keys as they are.
    c.direct = settings.quick_steering;
    c.use_item = keys.just_pressed(KeyCode::Space);

    let dt = time.delta_secs();
    let pressed = keys.any_just_pressed([KeyCode::KeyW, KeyCode::ArrowUp]);
    let racing = race.phase == Phase::Racing;
    let mut fire = false;
    start.window = (start.window - dt).max(0.0);
    start.since_press += dt;
    if matches!(race.phase, Phase::Intro | Phase::Countdown) {
        if pressed {
            if start.since_press > BOOST_REST {
                start.window = BOOST_WINDOW;
            }
            start.since_press = 0.0;
        }
    } else if racing && !start.racing {
        // The off: a press still good fires; otherwise one very soon will.
        fire = start.window > 0.0;
        if !fire {
            start.window = BOOST_WINDOW;
        }
    } else if racing && pressed && start.window > 0.0 {
        fire = true;
    }
    if fire {
        c.start_boost = Some((start.window >= BOOST_WINDOW_STRONG) as u8);
        start.window = 0.0;
    }
    start.racing = racing;
}

pub fn ai_drive(
    time: Res<Time>,
    track: Res<Track>,
    race: Res<Race>,
    variant: Res<crate::variant::Variant>,
    mut rng: ResMut<Rng>,
    loaded: Option<Res<LoadedWorld>>,
    mut q: Query<(&mut Kart, &mut Ai, &mut Controls, Has<Player>, Has<Remote>)>,
) {
    let dt = time.delta_secs();
    let racing = matches!(race.phase, Phase::Racing | Phase::Finished);
    // The field is paced against the player; online, where there are several, against
    // whichever of them leads.
    let player_progress = q.iter().filter(|x| x.3 || x.4).map(|x| x.0.progress).reduce(f32::max).unwrap_or(0.0);
    let cars: Vec<(usize, Vec3)> = q.iter().filter(|x| x.0.out.is_none()).map(|x| (x.0.slot, x.0.pos)).collect();
    // The nearest other car between two distances inside a cone ahead.
    let ahead = |k: &Kart, (min, max, cone): (f32, f32, f32)| {
        let forward = k.rot * Vec3::NEG_Z;
        cars.iter().any(|&(slot, at)| {
            let to = at - k.pos;
            let distance = to.length();
            slot != k.slot && (min..=max).contains(&distance) && to.dot(forward) >= cone * distance
        })
    };
    for (mut k, mut ai, mut c, here, elsewhere) in &mut q {
        // A player is a player wherever they sit.
        let is_player = here || elsewhere;
        if (is_player && k.finished.is_none() && !race.demo) || k.out.is_some() {
            ai.started = racing;
            continue;
        }
        // A car on a recording is paced, not driven: once the race has settled, ahead
        // of the player it is held back a little, and behind it hurried (`RaceSetup::Update`).
        let band = if race.time < RUBBER_BAND_DELAY || k.progress == player_progress {
            None
        } else if k.progress > player_progress {
            Some(1.0 - RUBBER_BAND)
        } else {
            Some(1.0 + RUBBER_BAND)
        };
        let skill = ai.skill;
        if let Some(route) = &mut k.route {
            route.racing = racing;
            if race.time < RUBBER_BAND_DELAY {
                route.base = skill;
            } else if let Some(band) = band {
                route.base = skill * (band + variant.rubber_band_boost());
            }
        }
        if !racing {
            *c = Controls::default();
            ai.started = false;
            continue;
        }
        // `Racer::OnRaceStart`: a driver keen on green bricks may be off with a turbo.
        if !ai.started {
            ai.started = true;
            if !is_player && (rng.range(0.0, 256.0) as u32) < ai.chances[2] {
                k.start_boost(0);
            }
        }
        // `Racer::SwitchToAiControl`: the player's car, its race run, is driven onto
        // the recording that begins nearest to it and then plays that.
        if is_player && !race.demo && k.route.is_none() && k.returning.is_none() && !variant.reverse {
            let nearest = loaded.as_ref().and_then(|loaded| {
                let begins = |record: &std::sync::Arc<crate::assets::route::Record>| RoutePlay::loop_start(record).distance_squared(k.pos);
                loaded.routes.iter().min_by(|a, b| begins(a).total_cmp(&begins(b)))
            });
            k.returning = nearest.map(|record| Return::new(record.clone()));
        }
        c.course = None;
        if let Some(mut returning) = k.returning.take() {
            match returning.drive(&k, dt) {
                Ok(course) => {
                    *c = Controls { course: Some(course), ..default() };
                    k.returning = Some(returning);
                }
                Err(route) => {
                    k.route = Some(route);
                    *c = Controls::default();
                }
            }
        } else if k.route.is_none() {
            ai.drive(&k, &mut c, &track, &mut rng, dt);
        }

        // Rubber-banding keeps the pack close to the player.
        let gap = (k.progress - player_progress) * track.length;
        k.top_factor = ai.skill
            * if gap > 60.0 {
                0.93
            } else if gap < -60.0 {
                1.06
            } else {
                1.0
            };

        // `Racer::UpdateTimers` and `AiConsiderPowerup`.
        c.use_item = false;
        if std::mem::take(&mut k.collected) {
            (ai.check, ai.interval) = (0.0, AI_CHECK_COLLECTED);
        }
        ai.check += dt;
        if ai.check > ai.interval {
            (ai.check, ai.interval) = (0.0, AI_CHECK);
            if let Some(held) = k.held {
                let mut keen = |colour: usize| (rng.range(0.0, 256.0) as u32) < ai.chances[colour];
                let saving = ai.charge.is_some_and(|(colour, wanted)| colour == held && k.whites < wanted);
                let fire = if saving {
                    None
                } else {
                    match held {
                        Power::Green => Some(keen(2)).filter(|keen| !*keen || !(k.spin > 0.0 || (k.finished.is_some() && k.whites == 3))),
                        // A red brick waits for something to shoot at.
                        Power::Red => ahead(&k, AI_RED_TARGET).then(|| keen(0)),
                        Power::Yellow => Some(keen(1)),
                        Power::Blue => Some(keen(3)),
                    }
                };
                match fire {
                    Some(true) => c.use_item = true,
                    Some(false) => ai.interval = AI_CHECK_PUT_OFF,
                    None if saving => ai.interval = AI_CHECK_PUT_OFF,
                    None => {}
                }
            }
        }

        // A driver with a car right in front of it sounds its horn.
        ai.taunt -= dt;
        if ai.taunt <= 0.0 {
            ai.taunt = AI_TAUNT_WAIT;
            if ahead(&k, (0.0, AI_TAUNT_TARGET.0, AI_TAUNT_TARGET.1)) {
                k.cues.horn = true;
                ai.taunt += rng.range(0.0, 65536.0).floor() * 0.008;
            }
        }
    }
}

/// What every champion's car weighs.
const CAR_MASS: f32 = 4500.0;
/// How much of their closing speed two cars bounce apart with.
const COLLISION_RESTITUTION: f32 = 0.75;
/// A shield of the second strength shoves a car that touches it side on, with this
/// acceleration; the stronger ones spin it, at this rate.
const SHIELD_SHOVE: f32 = 200.0 * UNIT;
const SHIELD_SHOVE_CONE: f32 = 0.7;
const SHIELD_SPIN_RATE: f32 = 9.0;
/// Gap between one kart's scraping sounds.
const SCRAPE_COOLDOWN: f32 = 0.25;

/// Cars shown from what a host says of them (`Puppet`) are not driven here.
pub fn kart_physics(time: Res<Time>, track: Res<Track>, mut q: Query<(&mut Kart, &Controls), Without<Puppet>>) {
    let dt = time.delta_secs().min(0.05);
    for (mut kart, c) in &mut q {
        if kart.out.is_none() {
            kart.advance(c, &track, dt);
        }
    }
}

impl Kart {
    /// Runs the physics for `dt` seconds, then works out where on the lap that leaves us.
    pub fn advance(&mut self, c: &Controls, track: &Track, dt: f32) {
        let k = self;
        let from = k.pos;
        if let Some(level) = c.start_boost {
            k.start_boost(level);
        }
        if k.warp_start > 0.0 {
            k.warp_start -= dt;
            if k.warp_start <= 0.0 {
                (k.warp_start, k.warp) = (0.0, crate::items::WARP_TIME + dt);
            }
        }
        let warping = k.warp > 0.0;
        for timer in [
            &mut k.spin,
            &mut k.spin_out,
            &mut k.boost,
            &mut k.shield,
            &mut k.cursed,
            &mut k.magnet,
            &mut k.warp,
            &mut k.yaw_impulse,
            &mut k.shove.1,
        ] {
            *timer = (*timer - dt).max(0.0);
        }
        if c.direct {
            k.steer += (c.steer - k.steer) * (1.0 - (-10.0 * dt).exp());
        } else {
            // `PlayerControls::UpdateSteering`.
            let rate = match (k.drifting, k.slide_tight) {
                (true, true) => STEER_SLIDE_RATE,
                (true, false) => STEER_DRIFT_RATE,
                (false, _) if c.steer.abs() >= 1.0 => STEER_RATE,
                (false, _) => STEER_IDLE_RATE,
            };
            let (side, held) = (c.steer.signum(), c.steer.abs().min(1.0));
            if held == 0.0 {
                // Let go, the wheels come back to the middle and stop there.
                k.steer -= k.steer.signum() * (STEER_RETURN_RATE * dt).min(k.steer.abs());
            } else {
                let turn = if k.steer * side < 0.0 {
                    STEER_RETURN_RATE
                } else if held >= 1.0 {
                    rate
                } else {
                    ((1.0 - held) * 0.5 + 0.5) * rate
                };
                k.steer = if side > 0.0 { (k.steer + turn * dt).min(held) } else { (k.steer - turn * dt).max(-held) };
            }
        }
        // `DriveController::Update`: a curse leans on the accelerator, changing its
        // mind every half second or less.
        if k.cursed > 0.0 {
            k.curse_timer -= dt;
            if k.curse_timer < 0.0 {
                k.curse_timer = k.random(500) as f32 * 0.001;
                k.curse_throttle = k.random(200) as f32 * 0.01 - 1.0;
            }
        }
        if k.shove.1 > 0.0 {
            k.external_force += k.shove.0;
        }

        if k.route.is_some() {
            let mark = k.route.as_ref().map(|route| route.mark());
            k.play_route(dt);
            // Something solid that isn't the circuit itself (a shut door, say) turns
            // a car on a recording back.
            let lift = Vec3::Y * physics::BODY_POINT_HEIGHT;
            if track.collision.wall(from + lift, k.pos + lift).is_some_and(|hit| hit.tag != 0) {
                if let (Some(route), Some(mark)) = (&mut k.route, mark) {
                    route.back_off(mark);
                }
                (k.pos, k.vel) = (from, Vec3::ZERO);
            }
        } else if let (true, Some((from, to, facing))) = (warping, k.warp_to) {
            // Taken straight to where the warp comes out.
            k.pos = to - (to - from) * (k.warp / crate::items::WARP_TIME).clamp(0.0, 1.0);
            k.facing = facing.with_y(0.0).normalize_or(k.facing);
            k.rot = Transform::IDENTITY.looking_to(k.facing, Vec3::Y).rotation;
            k.vel = k.facing * WARP_EXIT_SPEED;
            k.contacts = 4;
            if k.warp <= 0.0 {
                k.warp_to = None;
            }
        } else if warping {
            // Carried along the racing line, drifting to its middle.
            let (s, lat) = (k.s + WARP_SPEED * dt, k.lat * (1.0 - 2.0 * dt).max(0.0));
            let dir = track.sample(s).1;
            k.pos = track.surface_point(s, lat);
            k.facing = dir.with_y(0.0).normalize_or(k.facing);
            k.rot = Transform::IDENTITY.looking_to(k.facing, Vec3::Y).rotation;
            // Dropped back onto the road at speed when it ends.
            k.vel = k.facing * WARP_EXIT_SPEED;
            k.contacts = 4;
        } else {
            let steps = (dt / MAX_STEP).ceil().max(1.0);
            for _ in 0..steps as usize {
                physics::step(k, c, &track.collision, dt / steps);
            }
            // `Racer::UpdateTimers`: a car that drives for itself is over being blown
            // up at once; it is only ever thrown.
            k.spin_out = 0.0;
            if k.contacts == 4 && !k.wall_contact {
                k.safe = (k.pos, k.rot);
            }
        }
        // `Racer::Halt` and `Resume`: the strongest turbo floats the car, and so does
        // a magnet once it has stopped it. The turbo gives out early on a car that
        // isn't getting anywhere (`TurboAction::Update`).
        let turbo = k.boost > 0.0 && k.boost_level == 2;
        if turbo && k.boost > TURBO_FADE && k.boost < TURBO_FADE + TURBO_EARLY_END && k.vel.length() < physics::MOVING_SPEED {
            k.boost = TURBO_FADE;
        }
        k.hover = turbo || (k.magnet > 0.0 && (k.hover || k.halted()));
        let rise = |lift: f32, target: f32, rate: f32| lift + (target - lift).clamp(-rate * dt, rate * dt);
        k.hover_lift = match (k.hover || warping, k.route.is_some()) {
            (true, _) => rise(k.hover_lift, HOVER_HEIGHT, HOVER_RISE),
            // A car on a recording settles; one that drives is simply down.
            (false, true) => rise(k.hover_lift, 0.0, HOVER_FALL),
            (false, false) => 0.0,
        };
        // `ComputeSlideBankTarget`: it leans into the turn it is set to.
        let lean = if k.hover && k.route.is_none() && k.turn_radius != 0.0 {
            -k.turn_radius.signum() * (1.0 - (k.turn_radius.abs() / (4096.0 * UNIT)).min(1.0)) * HOVER_BANK
        } else {
            0.0
        };
        k.hover_bank = if k.hover { rise(k.hover_bank, lean, HOVER_BANK_RATE) } else { 0.0 };
        k.external_force = Vec3::ZERO;
        let forward = k.rot * Vec3::NEG_Z;
        k.yaw = (-forward.x).atan2(-forward.z);
        k.wheel_angle = (k.wheel_angle - k.vel.dot(forward) * dt / WHEEL_RADIUS) % TAU;

        // Where we are along the racing line, for the AI and anything that follows it.
        let (mut idx, mut s, mut lat) = track.project(k.pos, k.idx);
        if lat.abs() > 40.0 {
            // A long way from where we last were; look everywhere.
            (idx, s, lat) = track.project(k.pos, track.nearest(k.pos));
        }
        (k.idx, k.s, k.lat) = (idx, s, lat);

        // Out of the world: back to where the car last stood.
        if k.route.is_none() && !warping && !(WORLD_HEIGHTS.0 * UNIT..=WORLD_HEIGHTS.1 * UNIT).contains(&k.pos.y) {
            (k.pos, k.rot) = k.safe;
            k.vel = Vec3::ZERO;
            k.facing = (k.rot * Vec3::NEG_Z).normalize_or(k.facing);
            (k.idx, k.s, k.lat) = track.project(k.pos, track.nearest(k.pos));
        } else {
            let lift = Vec3::Y * physics::BODY_POINT_HEIGHT;
            if let Some(hit) = track.collision.touched(from + lift, k.pos + lift) {
                k.touched = hit.surface.touch_event;
                // `RacerPhysics::OnCollisionRecord`: a finishing surface ends the race.
                if hit.surface.finish {
                    k.ended = true;
                }
            }
            k.follow_course(track, from);
        }
    }
}

impl Kart {
    /// The car's shape for bumping into other cars: circles as wide as the car at its
    /// nose, middle and tail. Returns their centres and radius.
    fn hull(&self) -> ([Vec3; 3], f32) {
        let [width, front, rear] = self.outline;
        let middle = (front + rear) / 2.0;
        let ends = [(front + width).min(middle), middle, (rear - width).max(middle)];
        (ends.map(|z| self.pos + self.rot * Vec3::new(0.0, 0.0, z)), width)
    }
}

/// `Racer::OnEvent`: cars that meet are parted and bounce off each other; a shield
/// shoves or spins the car that touches it, and a curse is passed on.
pub fn kart_collisions(mut sfx: ResMut<Sfx>, mut q: Query<(&mut Kart, Has<Player>, Has<Remote>)>) {
    let mut pairs = q.iter_combinations_mut();
    while let Some([(mut a, a_here, a_elsewhere), (mut b, b_here, b_elsewhere)]) = pairs.fetch_next() {
        // Online a player elsewhere hears their own bumps by way of the host.
        let (a_player, b_player) = (a_here || a_elsewhere, b_here || b_elsewhere);
        // Karts on different levels (a bridge, say), in warp or blown into the air
        // pass each other by.
        if (a.pos.y - b.pos.y).abs() > 2.0 || a.warp > 0.0 || b.warp > 0.0 || a.spin_out > 0.0 || b.spin_out > 0.0 {
            continue;
        }
        let ((ends_a, radius_a), (ends_b, radius_b)) = (a.hull(), b.hull());
        // The deepest overlap between any of a's circles and any of b's.
        let mut worst: Option<(f32, Vec3)> = None;
        for ca in ends_a {
            for cb in ends_b {
                let d = (cb - ca).with_y(0.0);
                let overlap = radius_a + radius_b - d.length();
                if overlap > 0.0 && worst.is_none_or(|w| overlap > w.0) {
                    worst = Some((overlap, d.try_normalize().unwrap_or(Vec3::X)));
                }
            }
        }
        let Some((overlap, normal)) = worst else { continue };
        a.pos -= normal * overlap * 0.5;
        b.pos += normal * overlap * 0.5;
        // Equal weights, and so each car takes half of what the bounce gives back.
        let closing = (b.vel - a.vel).dot(normal) * (1.0 + COLLISION_RESTITUTION) * 0.5;
        // Cars on recordings are moved off their line, and on or back along it, by
        // the blow in the game's own units: its weight of car, its speeds.
        let blow = -closing / UNIT * CAR_MASS / 1000.0;
        a.shove(-normal * overlap * 0.5, -normal, blow);
        b.shove(normal * overlap * 0.5, normal, blow);
        a.vel += normal * closing;
        b.vel -= normal * closing;

        // Only bumps the player is part of are heard.
        if a_player || b_player {
            let (hitter, hit) = if a.vel.length_squared() > b.vel.length_squared() { (&mut a, &mut b) } else { (&mut b, &mut a) };
            if hitter.scrape_cooldown <= 0.0 && hit.scrape_cooldown <= 0.0 {
                let sound = id::CAR_HITS[sfx.roll(2) as usize];
                let contact = (hitter.pos + hit.pos) * 0.5;
                sfx.play_at(sound, contact);
                (hitter.sparks, hit.sparks) = (Some(contact + Vec3::Y * 0.6), Some(contact + Vec3::Y * 0.6));
                hitter.scrape_cooldown = SCRAPE_COOLDOWN;
                hit.scrape_cooldown = SCRAPE_COOLDOWN;
            }
            // Whoever ran into the other grumbles, unless a shield spared them.
            if hitter.shielded() {
                hit.cues.reaction = Some(false);
            } else {
                hitter.cues.reaction = Some(false);
            }
        }

        let touched = |shielded: &Kart, other: &mut Kart, away: Vec3| {
            if !shielded.shielded() || other.shielded() {
                return;
            }
            match shielded.shield_level {
                // Struck in the side, the other car is pushed off.
                1 if (other.rot * Vec3::NEG_Z).dot(away).abs() < SHIELD_SHOVE_CONE => other.shove_with(away * SHIELD_SHOVE),
                2 => other.spin_at(1.0, SHIELD_SPIN_RATE),
                3 => other.spin_at(2.0, SHIELD_SPIN_RATE),
                _ => {}
            }
        };
        touched(&a, &mut b, normal);
        touched(&b, &mut a, -normal);
        // A curse goes to the car that touches its bearer.
        let passed = |cursed: &mut Kart, other: &mut Kart| {
            let passes = cursed.cursed > 0.0 && other.cursed <= 0.0 && !other.shielded();
            if passes {
                other.curse(cursed.cursed);
                cursed.cursed = 0.0;
            }
            passes
        };
        if !passed(&mut a, &mut b) {
            passed(&mut b, &mut a);
        }
    }
}

/// What karts are ranked by, biggest first: finishers ahead of everyone still racing
/// and the earliest of them first, then the rest by how far round they are.
/// Cars put out of an elimination race come after those, the last to go first.
fn place_key(k: &Kart) -> (u8, f32) {
    match (k.out, k.finished) {
        (Some(time), _) => (0, time),
        (None, Some(time)) => (2, -time),
        (None, None) => (1, k.progress),
    }
}

pub fn update_places(race: Res<Race>, settings: Res<Settings>, mut q: Query<&mut Kart>) {
    let mut order: Vec<((u8, f32), Mut<Kart>)> = q
        .iter_mut()
        .map(|mut k| {
            // The rest of the field goes on finishing after the player has.
            let timed = matches!(race.phase, Phase::Racing | Phase::Finished);
            if timed && k.finished.is_none() && k.out.is_none() && (k.lap > settings.laps() || k.ended) {
                k.finished = Some(race.time);
            }
            (place_key(&k), k)
        })
        .collect();
    order.sort_by(|a, b| b.0.0.cmp(&a.0.0).then(b.0.1.total_cmp(&a.0.1)));
    for (i, (_, k)) in order.iter_mut().enumerate() {
        if k.place != i + 1 {
            // Gaining a place is worth a cheer, and losing one a groan.
            if race.phase == Phase::Racing && k.finished.is_none() {
                k.cues.reaction = Some(i + 1 < k.place);
            }
            k.place = i + 1;
        }
    }
}

pub fn sync_karts(mut q: Query<(&Kart, &mut Transform, &mut Visibility, Has<Player>)>) {
    for (k, mut t, mut visibility, is_player) in &mut q {
        visibility.set_if_neq(if k.out.is_some() { Visibility::Hidden } else { Visibility::Inherited });
        // Lean into corners.
        let lean = if k.warp > 0.0 {
            0.0
        } else if k.hover {
            k.hover_bank
        } else {
            -k.steer * 0.07 * (k.vel.length() / MAX_SPEED).min(1.0)
        };
        // The player's warp is seen from a tunnel, which is put well away from the circuit.
        t.translation = k.pos + Vec3::Y * k.hover_lift + if is_player && k.warp > 0.0 { TUNNEL } else { Vec3::ZERO };
        t.rotation = k.rot * Quat::from_rotation_z(lean);
        // A warp opening swallows the car over its last quarter second.
        let size = if k.warp_start > 0.0 { (k.warp_start / WARP_SHRINK).min(1.0) } else { 1.0 };
        t.scale = Vec3::splat(size.max(0.001));
    }
}

pub fn sync_wheels(
    karts: Query<&Kart>,
    mut wheels: Query<(&Wheel, &mut Transform)>,
    mut shields: Query<(&ChildOf, &mut Visibility), With<Shield>>,
) {
    for (wheel, mut t) in &mut wheels {
        let Ok(k) = karts.get(wheel.kart) else { continue };
        let steer = if wheel.front { k.steer * 0.4 } else { 0.0 };
        t.rotation = Quat::from_axis_angle(wheel.steer_axis, steer)
            * wheel.rest
            * Quat::from_axis_angle(wheel.spin_axis, k.wheel_angle * wheel.spin_ratio);
    }
    for (child_of, mut vis) in &mut shields {
        let Ok(k) = karts.get(child_of.parent()) else { continue };
        // Flicker when about to run out.
        let on = k.shield > 1.0 || (k.shield > 0.0 && (k.shield * 10.0) as i32 % 2 == 0);
        vis.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finishers_are_placed_by_their_times() {
        let track = Track::new();
        let kart = |finished: Option<f32>, progress: f32| {
            let mut kart = Kart::new(&track, 0);
            (kart.finished, kart.progress) = (finished, progress);
            kart
        };
        // Finishing times under a second apart, and two karts still out on the lap.
        let mut karts = [kart(Some(49.16), 0.0), kart(None, 310.0), kart(Some(46.79), 0.0), kart(Some(48.56), 0.0), kart(None, 325.0)];
        karts.sort_by(|a, b| {
            let (a, b) = (place_key(a), place_key(b));
            b.0.cmp(&a.0).then(b.1.total_cmp(&a.1))
        });
        let order: Vec<_> = karts.iter().map(|k| (k.finished, k.progress)).collect();
        assert_eq!(order, [(Some(46.79), 0.0), (Some(48.56), 0.0), (Some(49.16), 0.0), (None, 325.0), (None, 310.0)]);
        // Put out of an elimination race, a car is behind everyone still in it, and
        // ahead of those who went before.
        (karts[3].out, karts[0].out, karts[0].finished) = (Some(20.0), Some(30.0), Some(30.0));
        karts.sort_by(|a, b| {
            let (a, b) = (place_key(a), place_key(b));
            b.0.cmp(&a.0).then(b.1.total_cmp(&a.1))
        });
        let order: Vec<_> = karts.iter().map(|k| (k.out, k.progress)).collect();
        assert_eq!(order, [(None, 0.0), (None, 0.0), (None, 310.0), (Some(30.0), 0.0), (Some(20.0), 325.0)]);
    }

    #[test]
    fn cars_still_racing_are_timed_after_the_player_finishes() {
        use bevy::ecs::system::RunSystemOnce;
        let track = Track::new();
        let mut world = World::new();
        let settings = Settings::new(&crate::menu::Circuits(Vec::new()));
        let laps = settings.laps();
        world.insert_resource(settings);
        // The player is home; one car comes in six seconds later, another is still out.
        world.insert_resource(Race { phase: Phase::Finished, intro: 0.0, countdown: 0.0, time: 126.5, demo: false, quick: true });
        let car = |world: &mut World, slot: usize, lap: i32, finished: Option<f32>| {
            let mut kart = Kart::new(&track, slot);
            (kart.lap, kart.finished, kart.progress) = (lap, finished, lap as f32);
            world.spawn(kart).id()
        };
        let (player, second, third) = (car(&mut world, 5, laps + 1, Some(120.5)), car(&mut world, 0, laps + 1, None), car(&mut world, 1, laps, None));
        world.run_system_once(update_places).unwrap();
        let of = |world: &World, e: Entity| (world.get::<Kart>(e).unwrap().finished, world.get::<Kart>(e).unwrap().place);
        assert_eq!((of(&world, player), of(&world, second), of(&world, third)), ((Some(120.5), 1), (Some(126.5), 2), (None, 3)));
    }

    /// Lets the AI drive one kart alone and returns the times at which it started each lap.
    fn solo_run(track: &Track, seconds: f32) -> Vec<f32> {
        let mut kart = Kart::new(track, 0);
        let mut ai = Ai::new(1.0, 0);
        let mut rng = Rng(7);
        let mut c = Controls::default();
        let (dt, mut laps, mut top) = (1.0 / 60.0, Vec::new(), 0.0f32);
        for frame in 0..(seconds / dt) as usize {
            let lap = kart.lap;
            ai.drive(&kart, &mut c, track, &mut rng, dt);
            kart.advance(&c, track, dt);
            top = top.max(kart.vel.length());
            assert!(kart.pos.is_finite() && kart.vel.length() < 80.0, "physics blew up");
            if kart.lap > lap {
                laps.push(frame as f32 * dt);
            }
        }
        println!("lap starts {laps:?}, top speed {top:.1}, lap length {:.0}", track.length);
        laps
    }

    /// Long enough for two laps and the run up to the line, the longer the circuit.
    fn lapping_time(track: &Track) -> f32 {
        150.0f32.max(track.length * 0.09)
    }

    #[test]
    fn ai_laps_the_built_in_circuits() {
        for layout in crate::track::Layout::ALL {
            let track = Track::built(layout);
            let laps = solo_run(&track, lapping_time(&track));
            assert!(laps.len() >= 3, "{layout:?} {laps:?}");
        }
    }

    #[test]
    fn ai_laps_the_built_in_circuits_backwards() {
        for layout in crate::track::Layout::ALL {
            let mut track = Track::built(layout);
            track.reverse();
            let laps = solo_run(&track, lapping_time(&track));
            assert!(laps.len() >= 3, "{layout:?} {laps:?}");
        }
    }

    /// Needs the original game data; silently passes without it.
    #[test]
    fn a_finished_car_follows_a_recording_round() {
        let Some((track, world)) = crate::world::load("RACEC0R0") else { return };
        let mut kart = Kart::new(&track, PLAYER_SLOT);
        let mut returning = Return::new(world.routes[0].clone());
        let (start, mut far, mut on_route) = (kart.pos, 0.0f32, false);
        for _ in 0..(30.0 * 60.0) as usize {
            let c = match returning.drive(&kart, 1.0 / 60.0) {
                Ok(course) => Controls { course: Some(course), ..default() },
                Err(route) => {
                    kart.route = Some(route);
                    on_route = true;
                    break;
                }
            };
            kart.advance(&c, &track, 1.0 / 60.0);
            far = far.max(kart.pos.distance(start));
            assert!(kart.pos.is_finite());
        }
        println!("went {far:.0} from the grid at up to a third thrust; on the recording: {on_route}");
        // Half a minute at a third of the thrust takes it well round the circuit.
        assert!(on_route || far > 100.0, "{far}");
    }

    /// Needs the original game data; silently passes without it.
    #[test]
    fn ai_laps_the_original_circuits_backwards() {
        let mut failed = Vec::new();
        for (race, name) in crate::world::circuits() {
            let Some((mut track, _)) = crate::world::load(&race) else { continue };
            track.reverse();
            print!("{race} {name} reversed: ");
            let laps = solo_run(&track, 240.0);
            if laps.len() < 3 {
                failed.push(race);
            }
        }
        // The ones it can't get round are the ones that are never reversed.
        assert!(failed.is_empty() || failed == crate::variant::ONE_WAY, "{failed:#?}");
    }

    /// Needs the original game data; silently passes without it.
    #[test]
    fn ai_laps_the_original_circuits() {
        let mut failed = Vec::new();
        for (race, name) in crate::world::circuits() {
            let Some((track, _)) = crate::world::load(&race) else {
                failed.push(format!("{race} {name}: did not load"));
                continue;
            };
            print!("{race} {name}: ");
            let laps = solo_run(&track, 240.0);
            if laps.len() < 3 {
                failed.push(format!("{race} {name}: {} laps", laps.len()));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
