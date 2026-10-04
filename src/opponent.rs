//! The computer's cars. They don't drive: each plays back a drive round the circuit
//! that was recorded for it, faster or slower as the race demands, and gives way
//! sideways when shoved. Follows `RacerPhysics::UpdateRouteMotion` and `RouteCursor`.

use crate::assets::route::Record;
use crate::kart::Kart;
use crate::physics::{self, UNIT};
use crate::scenery::to_world;
use bevy::prelude::*;
use std::sync::Arc;

/// Playback speeds, as multiples of the speed the drive was recorded at.
const SPIN_SPEED: f32 = 0.3;
const SPIN_OUT_SPEED: f32 = 0.1;
const BOOST_SPEED: f32 = 1.7;
const WARP_SPEED: f32 = 4.5;
const CURSE_SPEED: f32 = 0.5;
const SPEED_RANGE: (f32, f32) = (-0.5, 2.75);
/// How quickly the speed moves towards what it should be, per second.
const ACCELERATION: f32 = 0.488;
const DECELERATION: f32 = 2.0;
const BOOST_ACCELERATION: f32 = 10.0;
const PUSHED_ACCELERATION: f32 = 2.0;
const PUSHED_DECELERATION: f32 = 0.732;
/// A push of this much, in the game's units, changes the speed by one.
const PUSH_IMPULSE: f32 = 300.0;
/// A car shoved off its line drifts back at this rate, in the game's units a second.
const SIDE_RETURN: f32 = 1.953;
/// Blown up, the car leaves the ground at this speed and comes back down under this.
const JUMP_SPEED: f32 = 50.0;
const JUMP_GRAVITY: f32 = 100.0;
/// How long a shove goes on being felt.
const PUSH_TIME: f32 = 0.4;
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
    pushed: f32,
    push_target: f32,
    /// The lights have gone out.
    pub racing: bool,
    spinning: bool,
    blown: bool,
    warping: bool,
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
            pushed: 0.0,
            push_target: 0.0,
            racing: false,
            spinning: false,
            blown: false,
            warping: false,
        }
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
            // What has just begun changes the speed at once.
            let (spinning, blown, warping) = (self.spin > 0.0, self.spin_out > 0.0, self.warp > 0.0);
            if spinning && !route.spinning {
                route.speed = SPIN_SPEED;
            }
            if blown && !route.blown {
                (route.speed, route.jump_speed) = (SPIN_OUT_SPEED, JUMP_SPEED);
            }
            if warping && !route.warping {
                route.speed = WARP_SPEED;
            }
            if !warping && route.warping {
                route.speed = route.base;
            }
            (route.spinning, route.blown, route.warping) = (spinning, blown, warping);

            // A pull (a grappling hook's, say) is a push along the way the car faces.
            let forward = self.rot * Vec3::NEG_Z;
            let pull = self.external_force.dot(forward) / UNIT;
            if pull != 0.0 {
                route.pushed = PUSH_TIME;
                route.push_target = (route.speed + pull / PUSH_IMPULSE).clamp(SPEED_RANGE.0, SPEED_RANGE.1);
            }
            route.pushed = (route.pushed - dt).max(0.0);
            let pushed = route.pushed > 0.0;
            let boosting = self.boost > 0.0;
            let target = if pushed {
                route.push_target
            } else if self.magnet > 0.0 {
                0.0
            } else if spinning {
                SPIN_SPEED
            } else if boosting {
                BOOST_SPEED
            } else if self.cursed > 0.0 {
                CURSE_SPEED
            } else {
                route.base
            };
            // Spinning, boosting or cursed on the ground, the speed is left as it was set.
            if pushed || !(spinning || blown || warping) {
                if route.speed < target {
                    let rate = if pushed {
                        PUSHED_ACCELERATION
                    } else if boosting {
                        BOOST_ACCELERATION
                    } else {
                        ACCELERATION
                    };
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
                    route.jump_speed = 0.0;
                }
            }

            let (position, rotation, width) = route.pose();
            route.side = route.side.clamp(-width[0], width[1]);
            let lift = Vec3::Y * route.jump * UNIT;
            self.pos = position + rotation * Vec3::X * route.side * UNIT + lift;
            // A spin turns the car round on the spot as it goes.
            self.rot = rotation * Quat::from_rotation_y(self.spin * physics::SPIN_RATE);
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

    /// Shoves a car on a recording: sideways off its line, and on or back along it.
    pub(crate) fn shove(&mut self, moved: Vec3, speed_change: Vec3) {
        let (right, forward) = (self.rot * Vec3::X, self.rot * Vec3::NEG_Z);
        let recorded = self.vel.length().max(10.0);
        let Some(route) = &mut self.route else { return };
        route.side += moved.dot(right) / UNIT;
        let along = speed_change.dot(forward) / recorded;
        if along != 0.0 {
            route.pushed = PUSH_TIME;
            route.push_target = (route.speed + along).clamp(SPEED_RANGE.0, SPEED_RANGE.1);
        }
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
    // Blown up, it leaves the ground and all but stops.
    kart.spin_out = 0.5;
    kart.play_route(0.05);
    assert!(kart.route.as_ref().unwrap().jump > 0.0 && kart.route.as_ref().unwrap().speed <= SPIN_OUT_SPEED + 0.01);
}
