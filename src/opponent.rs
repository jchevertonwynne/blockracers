//! The computer's cars. They don't drive: each plays back a drive round the circuit
//! that was recorded for it, faster or slower as the race demands, and gives way
//! sideways when shoved. Follows `RacerPhysics::UpdateRouteMotion` and `RouteCursor`.

use crate::assets::route::Record;
use crate::kart::Kart;
use crate::physics::UNIT;
use crate::scenery::to_world;
use bevy::prelude::*;
use std::sync::Arc;

/// Playback speeds, as multiples of the speed the drive was recorded at.
const SPIN_SPEED: f32 = 0.3;
const SPIN_OUT_SPEED: f32 = 0.1;
const WALL_BACK_SPEED: f32 = -0.1;
const BOOST_SPEED: f32 = 1.7;
const WARP_SPEED: f32 = 4.5;
const CURSE_SPEED: f32 = 0.5;
const SPEED_RANGE: (f32, f32) = (-0.5, 2.75);
/// How quickly the speed moves towards what it should be, per second.
const ACCELERATION: f32 = 0.488;
const DECELERATION: f32 = 2.0;
const PUSHED_ACCELERATION: f32 = 2.0;
const PUSHED_DECELERATION: f32 = 0.732;
/// A push of this much, in the game's units, changes the speed by one.
const PUSH_IMPULSE: f32 = 300.0;
/// A bump of this much or more changes the speed by one.
const BUMP_IMPULSE: f32 = 240.0;
/// A car shoved off its line drifts back at this rate, in the game's units a second.
const SIDE_RETURN: f32 = 1.953;
/// Blown up, the car leaves the ground at this speed and comes back down under this.
const JUMP_SPEED: f32 = 50.0;
const JUMP_GRAVITY: f32 = 100.0;
/// The leader is held back by this much and those behind hurried, to keep the field
/// with the player.
pub const RUBBER_BAND: f32 = 0.05;

pub struct RoutePlay {
    record: Arc<Record>,
    /// Milliseconds of the recording played.
    time: f32,
    speed: f32,
    /// The speed to hold when nothing is happening to the car.
    pub base: f32,
    /// How far right of the recorded line the car has been shoved, in game units.
    side: f32,
    jump: f32,
    jump_speed: f32,
    /// The lights have gone out.
    pub racing: bool,
    spinning: bool,
    warping: bool,
    boosting: bool,
    cursed: bool,
}

impl RoutePlay {
    pub fn new(record: Arc<Record>) -> Self {
        RoutePlay {
            record,
            time: 0.0,
            speed: 0.0,
            base: 1.0,
            side: 0.0,
            jump: 0.0,
            jump_speed: 0.0,
            racing: false,
            spinning: false,
            warping: false,
            boosting: false,
            cursed: false,
        }
    }

    /// `RacerPhysics::StartSpinOut`: all but stopped, and into the air.
    pub fn blow(&mut self) {
        (self.speed, self.jump_speed) = (SPIN_OUT_SPEED, JUMP_SPEED);
    }

    /// `RouteCursor::AttachAtLoop` and on by `ahead` milliseconds: on the lap part of
    /// the recording, at full speed.
    pub fn at_loop(record: Arc<Record>, ahead: f32) -> Self {
        let time = record.loop_time + ahead;
        RoutePlay { time, speed: 1.0, racing: true, ..RoutePlay::new(record) }
    }

    /// Where the lap part of a recording has the car `ahead` milliseconds in, and the
    /// way it faces there.
    pub fn preview(record: &Arc<Record>, ahead: f32) -> (Vec3, Vec3) {
        let (position, rotation, _) = RoutePlay::at_loop(record.clone(), ahead).pose();
        (position, rotation * Vec3::NEG_Z)
    }

    /// Where the lap part of the recording begins.
    pub fn loop_start(record: &Arc<Record>) -> Vec3 {
        RoutePlay::preview(record, 0.0).0
    }

    /// `RacerPhysics::UpdateRouteMotion` meeting something solid that isn't the
    /// circuit: back to where it was, and backing away.
    pub fn back_off(&mut self, (time, side): (f32, f32)) {
        (self.time, self.side, self.speed) = (time, side, WALL_BACK_SPEED);
    }

    /// What `back_off` puts back.
    pub fn mark(&self) -> (f32, f32) {
        (self.time, self.side)
    }

    pub fn restart(&mut self) {
        *self = RoutePlay::new(self.record.clone());
    }

    /// The car's place and facing on the recording right now, in our coordinates.
    fn pose(&self) -> (Vec3, Quat, [f32; 2]) {
        let (from, to, along) = self.record.at(self.record.wrap(self.time));
        let position = Vec3::from(from.position).lerp(Vec3::from(to.position), along);
        // The same turn can be stored either way up; go between them the short way.
        let (start, mut end) = (Quat::from_array(from.rotation).normalize(), Quat::from_array(to.rotation).normalize());
        if start.dot(end) < 0.0 {
            end = -end;
        }
        let rotation = start.lerp(end, along);
        let width = [0, 1].map(|i| from.width[i] + (to.width[i] - from.width[i]) * along);
        // In the mirror, the room on the car's left is on its right.
        let width = if crate::scenery::mirror() { [width[1], width[0]] } else { width };
        (to_world(position), facing(rotation), width)
    }
}

/// A car's recorded rotation as ours. The game's cars have X forward, Y left and Z up
/// in a world with Z up; ours face -Z in a world with Y up.
pub(crate) fn facing(rotation: Quat) -> Quat {
    let world = Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y);
    let car = Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y);
    // The game multiplies vectors from the other side, which turns the other way.
    let plain = Quat::from_mat3(&(world * Mat3::from_quat(rotation.conjugate()) * car.transpose()));
    if crate::scenery::mirror() { crate::scenery::mirrored_car(plain) } else { plain }
}

impl Kart {
    /// Moves a car that follows a recording on by `dt` seconds.
    pub(crate) fn play_route(&mut self, dt: f32) {
        let Some(route) = &mut self.route else { return };
        let from = self.pos;
        if !route.racing {
            let (position, rotation, _) = route.pose();
            (self.pos, self.rot, self.vel) = (position, rotation, Vec3::ZERO);
        } else {
            // What has just begun changes the speed at once (`StartSpin`, `StartBoost`,
            // `StartCurseSlow`, `StartRouteGhost`).
            let (spinning, warping) = (self.spin > 0.0, self.warp > 0.0);
            let (boosting, cursed) = (self.boost > 0.0, self.cursed > 0.0);
            if spinning && !route.spinning {
                route.speed = SPIN_SPEED;
            }
            if boosting && !route.boosting {
                route.speed = BOOST_SPEED;
            }
            if cursed && !route.cursed {
                route.speed = CURSE_SPEED;
            }
            if warping && !route.warping {
                route.speed = WARP_SPEED;
            }
            if !warping && route.warping {
                route.speed = route.base;
            }
            (route.spinning, route.warping, route.boosting, route.cursed) = (spinning, warping, boosting, cursed);

            // A pull (a grappling hook's, a magnet's, a shield's shove) is a push along
            // the way the car faces, for as long as it lasts (`StartRoutePush`).
            let forward = self.rot * Vec3::NEG_Z;
            let pull = self.external_force.dot(forward) / UNIT;
            let pushed = self.external_force != Vec3::ZERO;
            let target = if pushed {
                let from = if spinning { SPIN_SPEED + 0.1_f32.copysign(pull) } else { route.speed + pull / PUSH_IMPULSE };
                if pull >= 0.0 { from.min(SPEED_RANGE.1) } else { from.max(SPEED_RANGE.0) }
            } else if self.magnet > 0.0 {
                0.0
            } else if spinning {
                SPIN_SPEED
            } else {
                route.base
            };
            // Spinning, boosting, cursed or in warp, the speed is left as it was set.
            if pushed || !(spinning || boosting || cursed || warping) {
                if route.speed < target {
                    let rate = if pushed { PUSHED_ACCELERATION } else { ACCELERATION };
                    route.speed = (route.speed + rate * dt).min(target);
                } else {
                    let rate = if pushed { PUSHED_DECELERATION } else { DECELERATION };
                    route.speed = (route.speed - rate * dt).max(target);
                }
            }
            route.time = (route.time + route.speed * dt * 1000.0).max(0.0);
            route.side -= route.side.signum() * (SIDE_RETURN * dt).min(route.side.abs());
            if route.jump > 0.0 || route.jump_speed != 0.0 {
                route.jump_speed -= JUMP_GRAVITY * dt;
                route.jump = (route.jump + route.jump_speed * dt).max(0.0);
                if route.jump == 0.0 {
                    // Back on the ground, it is over being blown up.
                    (route.jump_speed, self.spin_out) = (0.0, 0.0);
                }
            }

            let (position, rotation, width) = route.pose();
            route.side = route.side.clamp(-width[0], width[1]);
            let lift = Vec3::Y * route.jump * UNIT;
            self.pos = position + rotation * Vec3::X * route.side * UNIT + lift;
            // A spin turns the car round on the spot as it goes.
            self.rot = rotation * Quat::from_rotation_y(self.spin * self.spin_rate);
            if dt > 0.0 {
                self.vel = (self.pos - from) / dt;
            }
        }
        self.external_force = Vec3::ZERO;
        let forward = self.rot * Vec3::NEG_Z;
        self.facing = forward.with_y(0.0).normalize_or(self.facing);
        self.ground_normal = self.rot * Vec3::Y;
        (self.contacts, self.air_time, self.sliding, self.slipping, self.wall_contact) = (4, 0.0, false, false, false);
    }

    /// Bumps a car on a recording: sideways off its line by `moved`, and on or back
    /// along it by an `impulse`, in the game's units, in `direction`
    /// (`RacerPhysics::MoveBy` and `ApplyDirectionalImpulse`).
    pub(crate) fn shove(&mut self, moved: Vec3, direction: Vec3, impulse: f32) {
        let (right, forward) = (self.rot * Vec3::X, self.rot * Vec3::NEG_Z);
        let spinning = self.spin > 0.0;
        let Some(route) = &mut self.route else { return };
        route.side += moved.dot(right) / UNIT;
        let (along, impulse) = if impulse < 0.0 { (-forward.dot(direction), -impulse) } else { (forward.dot(direction), impulse) };
        let change = impulse.min(BUMP_IMPULSE) / BUMP_IMPULSE;
        route.speed = if along >= 0.0 {
            let from = if spinning { SPIN_SPEED + 0.1 } else { route.speed + change };
            (from + (1.0 - along) * 0.05).min(SPEED_RANGE.1)
        } else {
            let from = if spinning { SPIN_SPEED - 0.1 } else { route.speed - change };
            (from - (along + 1.0) * 0.25).max(SPEED_RANGE.0)
        };
    }
}

#[cfg(test)]
#[test]
fn recorded_cars_face_the_way_they_go() {
    let Some(jam) = crate::assets::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else { return };
    let record = Arc::new(Record::parse(jam.get("/GAMEDATA/RACEC0R0/R2_M_0.RRB").unwrap(), false).unwrap());
    let mut kart = Kart::new(&crate::track::Track::new(), 0);
    kart.route = Some(RoutePlay::new(record.clone()));
    kart.play_route(0.016);
    assert_eq!(kart.vel, Vec3::ZERO);
    assert!(kart.pos.distance(to_world(Vec3::from(record.points[0].position))) < 0.01);
    // Twenty seconds of racing: always moving the way it points, at a car's speed.
    kart.route.as_mut().unwrap().racing = true;
    let mut worst = 1.0f32;
    for step in 0..1200 {
        kart.play_route(1.0 / 60.0);
        if step > 300 {
            assert!((5.0..70.0).contains(&kart.vel.length()), "{}", kart.vel.length());
            worst = worst.min(kart.vel.normalize().dot(kart.rot * Vec3::NEG_Z));
        }
    }
    assert!(worst > 0.7, "{worst}");
    // Blown up, it leaves the ground and all but stops, and is over it when it lands.
    assert!(kart.launch(1.0));
    kart.play_route(0.05);
    assert!(kart.route.as_ref().unwrap().jump > 0.0 && kart.route.as_ref().unwrap().speed < 0.2 && kart.spin_out > 0.0);
    for _ in 0..90 {
        kart.play_route(1.0 / 60.0);
    }
    assert_eq!((kart.route.as_ref().unwrap().jump, kart.spin_out), (0.0, 0.0));
}
