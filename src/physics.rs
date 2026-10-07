//! Kart dynamics, following the original game's `RacerCarBody` and `DriveController`
//! (as recovered by the isledecomp/racers decompilation) and using its constants.
//!
//! The model: steering input picks a turn radius; the kart's direction of travel
//! (`facing`) follows a circle of that radius under a centripetal force while the body
//! yaws at speed / radius. Ask for a tighter turn than the tyres can hold and the kart
//! "slip steers": the body yaws faster than the direction of travel, friction scrubs
//! speed and sideways grip drops. Engine thrust is balanced by quadratic drag chosen so
//! that the terminal speed is the kart's top speed.
//!
//! A floating car (`Kart::hover`, the original's "slide" body) is shown lifted and
//! leaning by `kart`; here it only stops feeling the surface under it and bounces
//! off any landing.
//!
//! The body is the original's rigid body (`RacerRigidBody`, `RacerCarBody`): it has a
//! mass, a centre of mass and the inertia of the box every car is given, and carries
//! angular momentum that yaw, roll and pitch impulses and the support of the wheels
//! add to and take from. Each step integrates it as the original does (forces, then
//! the body's position and attitude, then the tilt limit), and then four wheel probes
//! find the ground and, with two wheels or more in contact, set the body on it
//! (`UpdateWheelContacts`, `SnapToContacts`). A car floating on its turbo or a magnet
//! (the original's slide body) is still levelled by `level` and has no angular
//! momentum but its yaw.

use crate::collision::Collision;
use crate::kart::{Controls, Kart};
use bevy::prelude::*;
use std::f32::consts::PI;

/// Size of one original game unit in ours; lengths below are given in game units.
pub const UNIT: f32 = 0.3;

const GRAVITY: f32 = 39.0 * UNIT;
/// Gravity is quadrupled while no wheel touches the ground.
const AIRBORNE_GRAVITY_SCALE: f32 = 4.0;
const THRUST: f32 = 54.0 * UNIT;
/// Thrust against the direction of travel (braking) is doubled.
const BRAKE_SCALE: f32 = 2.0;
const TURBO_THRUST: f32 = THRUST * 8.0;
pub const MAX_SPEED: f32 = 120.0 * UNIT;
const BOOST_MAX_SPEED: f32 = 176.0 * UNIT;
const BOOST_GRIP_SCALE: f32 = 1.5;

// Steering input maps linearly onto curvature between these two.
const INV_MIN_TURN_RADIUS: f32 = 0.025 / UNIT;
const INV_MAX_TURN_RADIUS: f32 = 0.00025 / UNIT;
const MIN_TURN_RADIUS: f32 = 40.0 * UNIT;
const MAX_TURN_RADIUS: f32 = 4096.0 * UNIT;
/// Grip-limited radius assumed while airborne.
const FALLBACK_TURN_RADIUS: f32 = 100.0 * UNIT;
/// Below this speed the turn radius is cut to a fifth.
const LOW_SPEED: f32 = 40.0 * UNIT;
const LOW_SPEED_STEER_SCALE: f32 = 0.2;
/// Yaw rate is computed as if moving at least this fast, so karts can turn from rest.
const CREEP_SPEED: f32 = 30.0 * UNIT;
const STEER_MAX_SPEED: f32 = 155.0 * UNIT;
const POWERSLIDE_MIN_SPEED: f32 = 50.0 * UNIT;
const POWERSLIDE_ALIGNMENT_MIN: f32 = 0.85;
// The steering slip's last term outside a powerslide: 0.7071 as the original writes it,
// not the square root itself.
#[allow(clippy::approx_constant)]
const STEER_SLIP_ANGLE: f32 = 0.7071;

/// Per-second decay rates of sideways velocity and (off the throttle) forward velocity.
const LATERAL_DAMPING: f32 = 10.0;
const SPIN_LATERAL_DAMPING: f32 = 2.0;
const COAST_DAMPING: f32 = 1.0;
/// How fast the direction of travel swings round to where the body points, rad/s.
const FACING_TURN_RATE: f32 = 2.5;
/// `RacerCarBody::ApplyWallResponse`: a wall gives back this much of the speed the car
/// met it with (less of it upwards), pushes it off at this speed, turns it away at up
/// to this rate for this long, and won't throw it up faster than this.
const WALL_HORIZONTAL_DAMPING: f32 = 0.3;
const WALL_VERTICAL_DAMPING: f32 = 0.15;
const WALL_PUSH: f32 = 4.0 * UNIT;
const WALL_YAW: f32 = 4.0;
pub const YAW_IMPULSE_TIME: f32 = 0.2;
const WALL_MAX_RISE: f32 = 300.0 * UNIT;
/// A car that has been in the air longer than this bounces if it lands harder than
/// this, by this much of the speed it came down with.
const LANDING_AIR_TIME: f32 = 0.4;
const LANDING_BOUNCE_SPEED: f32 = 50.0 * UNIT;
const LANDING_BOUNCE: f32 = 1.15;
const HOVER_BOUNCE: f32 = 1.3;
/// `RacerPhysics::IsMoving`.
pub const MOVING_SPEED: f32 = 40.0 * UNIT;
/// Leaving the ground starts the car downwards at this speed.
const AIRBORNE_DROP: f32 = 8.0 * UNIT;
/// Thrust that brings a car held by a magnet to a stop.
const STOP_THRUST: f32 = THRUST * 2.0;
/// Every champion's car weighs this (`CHAMPS.CCB`); it turns a surface's rolling
/// resistance into drag.
pub const MASS: f32 = 4500.0;
/// Where a champion's car has its centre of mass, in the game's units and axes
/// (`CHAMPS.CCB`).
pub const CENTRE_OF_MASS: Vec3 = Vec3::new(-1.0, 0.0, -1.0);

/// A centre of mass given in the game's units and axes (X forward, Y left, Z up) from
/// the car's origin, in ours.
pub fn centre_of_mass(game: Vec3) -> Vec3 {
    Vec3::new(-game.y, game.z, -game.x) * UNIT
}

/// `Racer::InitializePhysics` gives every car the same box to take its inertia from:
/// length, width and height.
const BOX: [f32; 3] = [8.0, 5.0, 6.2];
const BOX_INERTIA: f32 = 0.083333336;
/// The body's angular half is kept in the original's units and on its clock: game
/// units, milliseconds, and mass in whatever the car's mass is given in.
pub const MS: f32 = 1000.0;
/// Below this much turning in a step, with nothing twisting the body, it stops turning
/// (`RacerRigidBody::Update`, a squared angle).
const ANGULAR_REST: f32 = 0.00060000003;
/// `LimitUprightTilt`: the body may lean 45 degrees from upright, and is put back to
/// that when it leans further.
const UPRIGHT_ANGLE: f32 = 0.78539819;
const UPRIGHT_MIN_COS: f32 = 0.70710677;
/// Each wheel in contact is held up with this share of a gravity (`1 / (wheels + 8)`)
/// that the body's torque is worked out from.
const CONTACT_SHARE: f32 = 8.0;
/// `g_defaultRideHeight` (0.2 units) is left out: the port's wheel points are already
/// where the tyres meet the road, and lifting the body by it catches on the byways of
/// the circuits built here. How far, a millisecond, a wheel looks below itself for the
/// ground while the car is on it.
const RIDE_HEIGHT: f32 = 0.0;
const SUPPORT_SWEEP: f32 = 0.04 * UNIT;
/// `g_wheelLengthwiseIndices`, `g_wheelSidewaysIndices` and `g_wheelDiagonalIndices`:
/// the wheel in line with each lengthwise, across and corner to corner.
const LENGTHWISE: [usize; 4] = [2, 3, 0, 1];
const SIDEWAYS: [usize; 4] = [1, 0, 3, 2];
const DIAGONAL: [usize; 4] = [3, 2, 1, 0];

/// The angular half of `RacerRigidBody`. Its inertia is that of the car's box, in the
/// body's own axes: about the sideways axis, the up axis and the forward one.
pub fn inertia(mass: f32) -> Vec3 {
    let [x, y, z] = BOX;
    let (xx, yy, zz) = (x * x, y * y, z * z);
    Vec3::new(
        mass / y * (zz + xx) * BOX_INERTIA,
        mass / z * (yy + xx) * BOX_INERTIA,
        mass / x * (zz + yy) * BOX_INERTIA,
    )
}

/// What a car's body keeps from step to step beyond where it is and how it is turned.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rigid {
    /// Angular momentum, on the world's axes.
    pub momentum: Vec3,
    /// Time left, in seconds, of a yaw, a roll and a pitch impulse. When one runs out
    /// the momentum about that axis is taken away again.
    pub yaw: f32,
    pub roll: f32,
    pub pitch: f32,
}

impl Rigid {
    /// `UpdateAngularVelocity`: radians a millisecond, on the world's axes.
    fn velocity(&self, rot: Quat, mass: f32) -> Vec3 {
        rot * ((rot.inverse() * self.momentum) / inertia(mass))
    }

    /// `AddAngularImpulse`: the momentum that turns the body at `rate` radians a
    /// millisecond about `axis`.
    fn impulse(&mut self, rot: Quat, mass: f32, axis: Vec3, rate: f32) {
        self.momentum += rot * ((rot.inverse() * (axis * rate)) * inertia(mass));
    }

    /// `ApplyYawImpulse`: turns at `rate` for `time` more, whatever the body did before.
    fn turn(&mut self, rot: Quat, mass: f32, rate: f32, time: f32) {
        self.yaw = time;
        let up = rot * Vec3::Y;
        self.cancel_along(up);
        self.impulse(rot, mass, up, rate);
    }

    /// `ApplyPitchImpulse`, unless a roll is going: noses the body over at `rate` a
    /// millisecond for `time` seconds (the sideways axis is the pitch's).
    pub fn pitch(&mut self, rot: Quat, mass: f32, rate: f32, time: f32) {
        if self.roll > 0.0 {
            return;
        }
        self.pitch = time;
        let side = rot * Vec3::NEG_X;
        self.cancel_along(side);
        self.impulse(rot, mass, side, rate);
    }

    /// `CancelAngularMomentumAlong`.
    fn cancel_along(&mut self, axis: Vec3) {
        self.momentum -= axis * self.momentum.dot(axis);
    }

    /// `CancelAngularMomentum`: takes away the momentum that would tip the body about
    /// the line the support at `offset` (from the centre of mass) makes with the way
    /// it pushes, `direction`, so that the body does not turn into the ground.
    fn cancel(&mut self, direction: Vec3, offset: Vec3) {
        let axis = offset.cross(direction).normalize_or_zero();
        let along = self.momentum.dot(axis);
        if along >= 0.0 {
            self.momentum -= axis * along;
        }
    }

    /// The timers of `RacerCarBody::Update`, which take the momentum an impulse gave
    /// away when the impulse is over. A car that is spinning is turned by the spin.
    fn tick(&mut self, rot: Quat, dt: f32, spinning: bool) {
        if !spinning {
            if dt >= self.yaw {
                self.yaw = 0.0;
                self.cancel_along(rot * Vec3::Y);
            } else {
                self.yaw -= dt;
            }
        }
        if self.roll > 0.0 {
            if dt >= self.roll {
                self.roll = 0.0;
                self.cancel_along(rot * Vec3::NEG_Z);
            } else {
                self.roll -= dt;
            }
        }
        if self.pitch > 0.0 {
            if dt >= self.pitch {
                self.pitch = 0.0;
                self.cancel_along(rot * Vec3::NEG_X);
            } else {
                self.pitch -= dt;
            }
        }
    }
}

/// A body's attitude from its forward and up directions, which are made square
/// (`SetDirectionUp`: the forward one is kept).
fn attitude(forward: Vec3, up: Vec3) -> Option<Quat> {
    let forward = forward.try_normalize()?;
    let up = up.reject_from(forward).try_normalize()?;
    let left = up.cross(forward);
    Some(Quat::from_mat3(&Mat3::from_cols(-left, up, -forward)).normalize())
}

/// (yaw gain, slip ratio, maximum angle between body and direction of travel)
type Slip = (f32, f32, f32);

/// Where the brick-built kart's wheels touch the ground: front left, front right, rear
/// left, rear right.
pub const WHEELS: [Vec3; 4] = [
    Vec3::new(-0.95, 0.0, -1.0),
    Vec3::new(0.95, 0.0, -1.0),
    Vec3::new(-0.95, 0.0, 1.0),
    Vec3::new(0.95, 0.0, 1.0),
];
/// Corners of its body, which are what hit walls.
pub const BODY_POINTS: [Vec3; 4] = [
    Vec3::new(-0.95, BODY_POINT_HEIGHT, -1.5),
    Vec3::new(0.95, BODY_POINT_HEIGHT, -1.5),
    Vec3::new(-0.95, BODY_POINT_HEIGHT, 1.5),
    Vec3::new(0.95, BODY_POINT_HEIGHT, 1.5),
];
pub const BODY_POINT_HEIGHT: f32 = 0.6;
/// Yaw rate of a kart that has been spun, rad/s.
pub const SPIN_RATE: f32 = 7.0;
/// Being blown up throws a kart forwards and upwards at these speeds, and takes its
/// controls away for a moment.
pub const LAUNCH_FORWARD_SPEED: f32 = 80.0 / MASS * 1000.0 * UNIT;
pub const LAUNCH_UP_SPEED: f32 = 200.0 / MASS * 1000.0 * UNIT;
pub const SPIN_OUT_TIME: f32 = 0.6;
/// Tallest ledge the wheels will climb.
const STEP_UP: f32 = 0.5;
/// How far below a wheel the ground may be and still count as contact.
const CONTACT_PADDING: f32 = 0.4 * UNIT;

fn tangent(v: Vec3, normal: Vec3, fallback: Vec3) -> Vec3 {
    v.reject_from_normalized(normal)
        .try_normalize()
        .unwrap_or(fallback)
}

/// Advances one kart by `dt` (which should be small: a hundredth of a second or so).
pub fn step(k: &mut Kart, c: &Controls, world: &Collision, dt: f32) {
    // `RacerCarBody::Update`: an impulse that has run its course leaves no momentum.
    let spinning = k.spin > 0.0;
    k.rigid.tick(k.rot, dt, spinning);
    let grounded = k.contacts > 0;
    let boosting = k.boost > 0.0;
    let up = if grounded { k.ground_normal } else { Vec3::Y };
    let body_fwd = tangent(k.rot * Vec3::NEG_Z, up, Vec3::NEG_Z);
    let mut facing = tangent(k.facing, up, body_fwd);
    let speed = k.vel.length();
    let vel_dir = if speed > 1e-3 {
        k.vel / speed
    } else {
        body_fwd
    };
    let vf = k.vel.dot(facing);
    let aligned = body_fwd.dot(vel_dir);

    // --- DriveController::Update: a slide ends when the car has all but stopped, and
    // a turbo that meets a wall is weakened for the rest of its run.
    if k.sliding && vf < CREEP_SPEED {
        k.sliding = false;
    }
    if !boosting {
        k.turbo_weak = false;
    } else if k.wall_contact {
        k.turbo_weak = true;
    }

    // --- PlayerControls::UpdateThrottle, Racer::StartDrift and EndDrift: the slide is
    // asked for with the accelerator down and the wheels turned, and has to be let go
    // of before another can begin.
    if c.drift && k.steer != 0.0 {
        let can_slide = !k.wall_contact
            && (k.hover || k.contacts >= 3)
            && aligned > POWERSLIDE_ALIGNMENT_MIN
            && vf >= POWERSLIDE_MIN_SPEED;
        if !k.drifting && can_slide {
            (k.drifting, k.sliding, k.slide_tight) = (true, true, c.tight);
        }
    } else {
        (k.drifting, k.sliding) = (false, false);
    }

    // --- DriveController::SetThrottleInput and ApplyThrust. A cursed driver's foot
    // is not their own.
    let cursed = k.cursed > 0.0;
    let throttle = if cursed {
        (c.throttle + k.curse_throttle).clamp(-1.0, 1.0)
    } else {
        c.throttle
    };
    let mut thrust = THRUST * throttle;
    if throttle * vf < 0.0 {
        thrust *= BRAKE_SCALE;
    }
    if k.magnet > 0.0 {
        // `UpdateBrakeToStop`.
        thrust = if vf > 0.0 { -STOP_THRUST } else { 0.0 };
    }
    let mut max_speed = MAX_SPEED * k.stats.top_speed * k.top_factor;
    let mut grip_scale = 1.0;
    if boosting {
        thrust = TURBO_THRUST * if k.turbo_weak { 0.5 } else { 1.0 };
        max_speed = BOOST_MAX_SPEED * k.stats.top_speed;
        grip_scale = BOOST_GRIP_SCALE;
    }
    let thrust = thrust * k.stats.acceleration;

    // --- DriveController::SetSteeringInput: steering input to turn radius (positive
    // turns left). A cursed driver's steering is reversed.
    let mut input = k.steer * if cursed { -1.0 } else { 1.0 } * k.stats.handling;
    let mut radius = 0.0;
    if input != 0.0 {
        // `Controls::hand`.
        let turned = input.abs() / if c.hand { k.pace } else { 1.0 };
        radius = 1.0 / (INV_MAX_TURN_RADIUS + (INV_MIN_TURN_RADIUS - INV_MAX_TURN_RADIUS) * turned);
    }
    // `UpdateReturnToPath` sets the turn and the thrust without the driver's controls.
    let mut thrust = thrust;
    if let Some((turn, push)) = c.course {
        (radius, input) = (turn.abs(), turn.signum());
        if !boosting && k.magnet <= 0.0 {
            thrust = THRUST * push * k.stats.acceleration;
        }
    }
    if radius != 0.0 && speed < LOW_SPEED {
        radius *= LOW_SPEED_STEER_SCALE;
    }
    // The tightest circle the tyres can hold at this speed.
    let limit = if grounded {
        vf * vf / (k.surface.lateral_grip * grip_scale * GRAVITY)
    } else {
        FALLBACK_TURN_RADIUS
    };

    // --- DriveController::ApplySteering, with RacerPhysics::CanSteer.
    let against = input * k.turn_radius < 0.0;
    let can_steer = aligned > 0.0
        && !against
        && (k.slipping || aligned >= 0.9)
        && !k.wall_contact
        && speed <= STEER_MAX_SPEED;
    let mut slip: Option<Slip> = None;
    if radius > 0.0 {
        if k.sliding {
            if can_steer {
                let assist = (1.0 - radius / limit).max(0.05);
                slip = Some(if k.slide_tight {
                    (1.0 + 2.0 * assist, 0.25, PI)
                } else {
                    (1.0 + assist, 0.85, PI)
                });
                if radius < limit {
                    radius += (limit - radius) * 0.25;
                }
            }
        } else {
            if can_steer && radius < limit {
                slip = Some((2.0 - radius / limit, 0.85, STEER_SLIP_ANGLE));
            }
            if radius < limit {
                radius = (limit + radius) * 0.5;
            }
        }
        radius = if radius > MAX_TURN_RADIUS {
            0.0
        } else {
            radius.max(MIN_TURN_RADIUS)
        };
    }
    let radius = radius * input.signum();
    k.turn_radius = radius;
    // `AccumulateForces` gives up slip steering once the car is going backwards.
    if aligned <= 0.0 {
        slip = None;
    }
    k.slipping = slip.is_some();
    if let Some((_, ratio, _)) = slip {
        k.slip_ratio = ratio;
    }

    // --- RacerCarBody::AccumulateForces, as accelerations.
    let turned = k.yaw_impulse > 0.0;
    let mut acc = Vec3::ZERO;
    // `ApplyYawImpulse`: the rate, and how long it is asked for.
    let mut yaw: Option<(f32, f32)> = None;
    let mut facing_rate = 0.0;
    if !grounded {
        acc.y -= GRAVITY * AIRBORNE_GRAVITY_SCALE;
        let mut push = facing * thrust;
        push.y = push.y.min(GRAVITY);
        acc += push;
        if radius != 0.0 && !turned {
            yaw = Some((vf / radius, YAW_IMPULSE_TIME));
        }
    } else {
        // Gravity pulls along the slope, if it is steep enough for this surface; the
        // ground carries the rest.
        let pull = Vec3::NEG_Y * GRAVITY + up * GRAVITY * up.y;
        if pull.length() > GRAVITY * k.surface.support {
            acc += pull;
        }

        let forward_vel = facing * vf;
        let lateral_vel = k.vel - forward_vel - up * k.vel.dot(up);
        let mut contact_scale = 1.0;
        if slip.is_some() || spinning {
            if speed > 0.5 {
                acc -= vel_dir * GRAVITY * k.surface.friction * k.slip_ratio;
            }
            contact_scale = 1.0 - k.slip_ratio;
        }
        if spinning || turned {
            acc -= lateral_vel * SPIN_LATERAL_DAMPING * contact_scale;
        } else {
            acc -= lateral_vel * LATERAL_DAMPING * contact_scale;
            if thrust != 0.0 {
                acc += thrust
                    * if k.contacts >= 3 && slip.is_none() {
                        body_fwd
                    } else {
                        facing
                    };
            } else {
                acc -= forward_vel * COAST_DAMPING;
            }
            if radius != 0.0 {
                // The pull to the middle of the turn is level, whatever the road is.
                acc += Vec3::Y.cross(facing).normalize_or_zero() * vf * vf / radius;
                facing_rate = vf / radius;
                let rate = match slip {
                    Some((gain, _, max_lag)) if aligned >= max_lag.cos() => gain * facing_rate,
                    Some(_) => 0.0,
                    None if vf > 0.5 * UNIT && vf < CREEP_SPEED => CREEP_SPEED / radius,
                    None => facing_rate,
                };
                yaw = Some((rate, YAW_IMPULSE_TIME));
            }
        }
    }
    if k.spin_out <= 0.0 && !k.hover {
        acc += Vec3::from(k.surface.force);
    }
    acc += k.external_force;
    if spinning {
        yaw = Some((k.spin_rate, k.spin));
    } else if turned {
        yaw = Some((k.yaw_kick, k.yaw_impulse));
    }
    let rolling = if k.hover {
        0.0
    } else {
        k.surface.rolling_resistance
    };
    let drag = thrust.abs() / (max_speed * max_speed) + rolling / k.mass / UNIT;
    acc -= k.vel * speed * drag;

    // --- The body's angular side of `AccumulateForces`. With fewer than three wheels
    // down each is held up by a share of gravity that twists the body about its centre
    // of mass; and each wheel's support takes away the momentum that would turn the
    // body into the ground at the wheel across from it.
    let ms = dt * MS;
    let mut torque = Vec3::ZERO;
    if grounded && !k.hover && k.rigid.roll <= 0.0 && k.rigid.pitch <= 0.0 {
        let down = if k.contacts >= 3 { 0b1111 } else { k.wheel_mask };
        let held = k.mass * GRAVITY / UNIT / (MS * MS) / (k.contacts as f32 + CONTACT_SHARE);
        let (rot, centre, wheels) = (k.rot, k.centre, k.wheels);
        let from_centre = |wheel: usize| rot * (wheels[wheel] - centre) / UNIT;
        for wheel in (0..4).filter(|wheel| down >> wheel & 1 == 1) {
            if k.contacts < 3 {
                torque += from_centre(wheel).cross(Vec3::Y * held);
            }
            k.rigid
                .cancel(k.ground_normal, from_centre(DIAGONAL[wheel]));
        }
    }
    if let Some((rate, time)) = yaw {
        k.rigid.turn(k.rot, k.mass, rate / MS, time);
    }

    // --- RacerRigidBody::Update: the position moves by the old velocity and half of
    // what the forces add, the body turns by its angular velocity, and then the
    // torque is added to its momentum.
    let carried = (k.vel + acc * dt * 0.5) * dt;
    k.vel += acc * dt;
    let turned_by = k.rigid.velocity(k.rot, k.mass) * ms;
    let (forward, upward) = (k.rot * Vec3::NEG_Z, k.rot * Vec3::Y);
    if let Some(rot) = attitude(
        forward + turned_by.cross(forward),
        upward + turned_by.cross(upward),
    ) {
        k.rot = rot;
    }
    k.rigid.momentum += torque * ms;
    if torque == Vec3::ZERO && turned_by.length_squared() < ANGULAR_REST {
        k.rigid.momentum = Vec3::ZERO;
    }
    limit_tilt(k);

    // --- RacerCarBody::UpdateFacingDirection.
    let body_fwd = tangent(k.rot * Vec3::NEG_Z, up, body_fwd);
    match slip {
        Some((_, _, max_lag))
            if grounded && k.contacts > 2 && vf >= CREEP_SPEED && thrust > 0.0 =>
        {
            let lag = facing.angle_between(body_fwd) + (facing_rate * dt).abs();
            facing = if lag > max_lag {
                let side = up.dot(facing.cross(body_fwd)).signum();
                Quat::from_axis_angle(up, -side * max_lag) * body_fwd
            } else {
                Quat::from_axis_angle(up, facing_rate * dt) * facing
            };
        }
        _ => {
            let angle = facing.angle_between(body_fwd);
            let max_step = FACING_TURN_RATE * dt;
            facing = if facing.dot(body_fwd) > 0.98 || max_step >= angle {
                body_fwd
            } else {
                let side = up.dot(facing.cross(body_fwd)).signum();
                Quat::from_axis_angle(up, side * max_step) * facing
            };
        }
    }
    k.facing = facing.normalize_or(body_fwd);

    let previous_centre = k.pos + Vec3::Y * BODY_POINT_HEIGHT;
    k.pos += carried;
    collide_walls(k, world, previous_centre);
    probe_ground(k, world, dt);
}

/// `LimitUprightTilt`: a body that leans more than 45 degrees is put at 45, still
/// leaning the way it was and still facing the way it was.
fn limit_tilt(k: &mut Kart) {
    let up = k.rot * Vec3::Y;
    if up.y >= UPRIGHT_MIN_COS {
        return;
    }
    let Some(lean) = up.with_y(0.0).try_normalize() else {
        return;
    };
    let up = lean * UPRIGHT_ANGLE.sin() + Vec3::Y * UPRIGHT_ANGLE.cos();
    // `SetUpDirection`: the up direction is kept, the forward one made square to it.
    if let Some(forward) = (k.rot * Vec3::NEG_Z).reject_from(up).try_normalize() {
        k.rot = Quat::from_mat3(&Mat3::from_cols(-up.cross(forward), up, -forward)).normalize();
    }
}

/// Keeps the body out of walls: the kart's centre may not pass through one, and nor
/// may the lines from the centre to each corner of the body. The wall met deepest is
/// the one the car answers to.
fn collide_walls(k: &mut Kart, world: &Collision, previous_centre: Vec3) {
    k.wall_contact = false;
    let mut worst: Option<(f32, Vec3)> = None;
    let mut push_out = |k: &mut Kart, normal: Vec3, push: f32| {
        let Some(flat) = normal.with_y(0.0).try_normalize() else {
            return;
        };
        k.pos += flat * (push + 0.01);
        if worst.is_none_or(|w| push > w.0) {
            worst = Some((push, normal));
        }
    };

    let centre = k.pos + Vec3::Y * BODY_POINT_HEIGHT;
    if let Some(hit) = world.wall(previous_centre, centre) {
        k.pos = hit.point - Vec3::Y * BODY_POINT_HEIGHT;
        push_out(k, hit.normal, 0.3);
    }
    for _ in 0..2 {
        for corner in k.body {
            let centre = k.pos + Vec3::Y * corner.y;
            let reach = k.pos + k.rot * corner - centre;
            if let Some(hit) = world.wall(centre, centre + reach) {
                let normal = hit.normal.with_y(0.0).normalize_or_zero();
                push_out(k, hit.normal, (1.0 - hit.t) * reach.dot(-normal).max(0.0));
            }
        }
    }
    let Some((_, normal)) = worst else { return };
    k.wall_contact = true;

    // --- RacerCarBody::ApplyWallResponse.
    k.slipping = false;
    let into = k.vel.dot(normal);
    if into < 0.0 {
        k.vel -= normal * into;
    }
    // Nose to the wall, the car is turned away from it.
    if k.spin <= 0.0 && (k.rot * Vec3::NEG_Z).dot(normal) < 0.0 {
        let side = (k.rot * Vec3::NEG_X).dot(normal);
        k.yaw_kick = if side < 0.0 {
            -WALL_YAW * ((side + 1.0) * 0.5 + 0.5)
        } else {
            WALL_YAW * ((1.0 - side) * 0.5 + 0.5)
        };
        k.yaw_impulse = YAW_IMPULSE_TIME;
    }
    if into < 0.0 {
        k.vel.x -= normal.x * WALL_HORIZONTAL_DAMPING * into;
        k.vel.z -= normal.z * WALL_HORIZONTAL_DAMPING * into;
        k.vel.y -= normal.y * WALL_VERTICAL_DAMPING * into;
    }
    k.vel += normal * WALL_PUSH;
    k.vel.y = k.vel.y.min(WALL_MAX_RISE);
}

/// A floating car's ground: each wheel's, averaged, and the body levelled onto it.
fn probe_hover(k: &mut Kart, world: &Collision, dt: f32) {
    let was_grounded = k.contacts > 0 && k.spin_out <= 0.0;
    let reach = if was_grounded { CONTACT_PADDING } else { 0.0 };
    let mut hits = [None; 4];
    let mut lift = 0.0;
    let mut normal_sum = Vec3::ZERO;
    let had_contact = k.contacts > 0;
    k.contacts = 0;
    k.wheel_mask = 0;
    let mut force = Vec3::ZERO;
    for (index, (wheel, hit_point)) in k.wheels.iter().zip(&mut hits).enumerate() {
        let foot = k.pos + k.rot * *wheel;
        let Some(hit) = world.ground(foot + Vec3::Y * STEP_UP, STEP_UP + reach) else {
            continue;
        };
        *hit_point = Some(hit.point);
        lift += hit.point.y - foot.y;
        normal_sum += if hit.normal.y < 0.0 {
            -hit.normal
        } else {
            hit.normal
        };
        force += Vec3::from(hit.surface.force);
        if k.contacts == 0 {
            k.surface = hit.surface;
        } else {
            k.surface.rolling_resistance += hit.surface.rolling_resistance;
            k.surface.lateral_grip += hit.surface.lateral_grip;
            k.surface.friction += hit.surface.friction;
            k.surface.support += hit.surface.support;
        }
        k.wheel_mask |= 1 << index;
        k.contacts += 1;
    }
    if k.contacts == 0 {
        if had_contact {
            k.vel.y -= AIRBORNE_DROP;
        }
        k.ground_normal = Vec3::Y;
        k.surface = default();
        k.air_time += dt;
        return;
    }
    let count = k.contacts as f32;
    k.surface.rolling_resistance /= count;
    k.surface.lateral_grip /= count;
    k.surface.friction /= count;
    k.surface.support /= count;
    k.surface.force = (force / count).to_array();
    // `RacerPhysics::ApplyWheelSurface` under `NSLWJ`: only the sound and what the
    // wheels throw up are taken from the surface; the rest is as on bare ground.
    if k.ignore_surfaces {
        k.surface = crate::assets::materials::Surface {
            sound: k.surface.sound,
            particle: k.surface.particle,
            ..default()
        };
    }
    k.pos.y += lift / count;

    // With all four wheels down, the diagonals give the plane the kart sits on.
    let mut normal = normal_sum.normalize_or(Vec3::Y);
    if let [Some(fl), Some(fr), Some(rl), Some(rr)] = hits {
        let n = (fl - rr).cross(fr - rl);
        if let Some(n) = (n * n.y.signum()).try_normalize() {
            normal = n;
        }
    }
    let into = k.vel.dot(normal);
    // `UpdateWheelContacts`: after a long enough fall a hard landing bounces the car
    // back into the air.
    // A floating car bounces off any landing, and harder (`UpdateSlideContacts`).
    let (bounces, bounce) = if k.hover {
        (true, HOVER_BOUNCE)
    } else {
        (k.air_time > LANDING_AIR_TIME, LANDING_BOUNCE)
    };
    if !was_grounded && bounces && into < -LANDING_BOUNCE_SPEED {
        k.vel.y -= into * bounce;
        k.contacts = 0;
        k.ground_normal = Vec3::Y;
        k.air_time += dt;
        return;
    }
    k.air_time = 0.0;
    k.ground_normal = normal;
    if into < 0.0 {
        k.vel -= normal * into;
    }
    let forward = tangent(k.rot * Vec3::NEG_Z, normal, Vec3::NEG_Z);
    let level = Transform::IDENTITY.looking_to(forward, normal).rotation;
    k.rot = k.rot.slerp(level, (15.0 * dt).min(1.0));
}

/// `UpdateWheelContacts` and `SnapToContacts`: finds the ground under each wheel.
/// The wheels within a little of the highest ground are the ones in contact; with
/// two or more the body is turned to lie on their ground, with three or more it counts
/// as on all four, and the wheel whose ground is highest is set on it, a ride height
/// above.
fn probe_ground(k: &mut Kart, world: &Collision, dt: f32) {
    if k.hover {
        return probe_hover(k, world, dt);
    }
    let was_grounded = k.contacts > 0 && k.spin_out <= 0.0;
    // How far below itself a wheel looks: as far as the car moves in the time at
    // least, and never less than it rides above the ground.
    let reach = if was_grounded {
        (SUPPORT_SWEEP * dt * MS).max(RIDE_HEIGHT * 1.5)
    } else {
        0.0
    };
    let had_contact = k.contacts > 0;
    let feet = k.wheels.map(|wheel| k.pos + k.rot * wheel);
    let mut hits: [Option<_>; 4] = Default::default();
    // How far above the bottom of its sweep each wheel's ground is.
    let mut rise = [f32::NEG_INFINITY; 4];
    let mut best: Option<usize> = None;
    for (index, foot) in feet.iter().enumerate() {
        let Some(hit) = world.ground(*foot + Vec3::Y * STEP_UP, STEP_UP + reach) else {
            continue;
        };
        rise[index] = hit.point.y - (foot.y - reach);
        if best.is_none_or(|b| rise[index] > rise[b]) {
            best = Some(index);
        }
        hits[index] = Some(hit);
    }
    let Some(selected) = best else {
        k.contacts = 0;
        k.wheel_mask = 0;
        if had_contact {
            k.vel.y -= AIRBORNE_DROP;
        }
        k.ground_normal = Vec3::Y;
        k.surface = default();
        k.air_time += dt;
        return;
    };
    let limit = (rise[selected] - CONTACT_PADDING).max(0.0);
    let mut mask = 0u8;
    let mut normal_sum = Vec3::ZERO;
    let mut force = Vec3::ZERO;
    let mut touching = 0;
    for (index, hit) in hits.iter().enumerate() {
        let Some(hit) = hit else { continue };
        force += Vec3::from(hit.surface.force);
        if touching == 0 {
            k.surface = hit.surface;
        } else {
            k.surface.rolling_resistance += hit.surface.rolling_resistance;
            k.surface.lateral_grip += hit.surface.lateral_grip;
            k.surface.friction += hit.surface.friction;
            k.surface.support += hit.surface.support;
        }
        touching += 1;
        if rise[index] >= limit {
            mask |= 1 << index;
            normal_sum += if hit.normal.y < 0.0 {
                -hit.normal
            } else {
                hit.normal
            };
        }
    }
    let count = touching as f32;
    k.surface.rolling_resistance /= count;
    k.surface.lateral_grip /= count;
    k.surface.friction /= count;
    k.surface.support /= count;
    k.surface.force = (force / count).to_array();
    let in_contact = mask.count_ones();

    // `SnapToContacts`: the body takes the lie of the ground between the wheels in
    // contact: its forward direction along the two on one side (or the car's own if
    // the other is off the ground), and its sideways one across the two at one end.
    let down = |wheel: usize| mask >> wheel & 1 == 1;
    let point = |wheel: usize| hits[wheel].as_ref().map_or(feet[wheel], |hit| hit.point);
    if in_contact > 1 && k.rigid.roll <= 0.0 && k.rigid.pitch <= 0.0 {
        // The right front wheel leads when all four are down.
        let lead = if in_contact < 4 { selected } else { 1 };
        let (along, across) = (LENGTHWISE[lead], SIDEWAYS[lead]);
        let forward = if down(along) {
            point(lead.min(along)) - point(lead.max(along))
        } else {
            k.rot * Vec3::NEG_Z
        };
        let left = if down(across) {
            point(across.min(lead)) - point(across.max(lead))
        } else {
            k.rot * Vec3::NEG_X
        };
        // `SetDirectionSide`.
        if let Some(forward) = forward.try_normalize()
            && let Some(left) = left.reject_from(forward).try_normalize()
        {
            let up = forward.cross(left);
            k.rot = Quat::from_mat3(&Mat3::from_cols(-left, up, -forward)).normalize();
        }
    }
    k.pos.y += point(selected).y - feet[selected].y + RIDE_HEIGHT;

    let normal = if in_contact >= 3 {
        mask = 0b1111;
        k.rot * Vec3::Y
    } else {
        normal_sum.normalize_or(Vec3::Y)
    };
    k.wheel_mask = mask;
    k.contacts = if in_contact >= 3 { 4 } else { in_contact as u8 };
    let into = k.vel.dot(normal);
    // `UpdateWheelContacts`: after a long enough fall a hard landing bounces the car
    // back into the air.
    if !was_grounded && k.air_time > LANDING_AIR_TIME && into < -LANDING_BOUNCE_SPEED {
        k.vel.y -= into * LANDING_BOUNCE;
        k.contacts = 0;
        k.wheel_mask = 0;
        k.ground_normal = Vec3::Y;
        k.air_time += dt;
        return;
    }
    k.air_time = 0.0;
    k.ground_normal = normal;
    if into < 0.0 {
        k.vel -= normal * into;
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_car_s_inertia_is_that_of_the_original_s_box() {
        // `RacerBoxBody::ComputeInertiaTensor` with the box (8, 5, 6.2) and a mass of 4500.
        let i = inertia(4500.0);
        let f = 0.083333336;
        assert!((i.x - 4500.0 / 5.0 * (6.2f32 * 6.2 + 64.0) * f).abs() < 1.0);
        assert!((i.y - 4500.0 / 6.2 * (25.0 + 64.0) * f).abs() < 1.0);
        assert!((i.z - 4500.0 / 8.0 * (6.2f32 * 6.2 + 25.0) * f).abs() < 1.0);
    }

    #[test]
    fn a_yaw_impulse_turns_the_body_at_its_rate_and_ends_with_it() {
        let (rot, mass) = (Quat::from_rotation_y(0.7), 4500.0);
        let mut r = Rigid::default();
        r.turn(rot, mass, 0.004, 0.2);
        assert!((r.velocity(rot, mass) - rot * Vec3::Y * 0.004).length() < 1e-6);
        r.tick(rot, 0.25, false);
        assert!(r.momentum.length() < 1e-3);
    }

    #[test]
    fn a_heavier_car_turns_no_slower_for_a_yaw_impulse_but_pitches_alike() {
        let rot = Quat::IDENTITY;
        let (mut light, mut heavy) = (Rigid::default(), Rigid::default());
        light.turn(rot, 2000.0, 0.003, 0.2);
        heavy.turn(rot, 6000.0, 0.003, 0.2);
        assert!((light.velocity(rot, 2000.0) - heavy.velocity(rot, 6000.0)).length() < 1e-6);
        assert!(heavy.momentum.length() > light.momentum.length() * 2.9);
    }

    #[test]
    fn support_takes_the_momentum_that_tips_the_body_into_the_ground() {
        let mut r = Rigid {
            momentum: Vec3::X * 5.0,
            ..default()
        };
        // A wheel ahead of the centre, the ground pushing up: the axis is sideways.
        r.cancel(Vec3::Y, Vec3::NEG_Z);
        assert!(r.momentum.length() < 1e-5 || r.momentum.x.abs() < 5.0);
    }

    #[test]
    fn the_centre_of_mass_is_in_our_axes() {
        let c = centre_of_mass(CENTRE_OF_MASS);
        assert!((c - Vec3::new(0.0, -UNIT, 1.0 * UNIT)).length() < 1e-6);
    }
}
