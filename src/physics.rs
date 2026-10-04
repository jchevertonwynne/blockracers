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
//! Where the original integrates a full rigid body with inertia tensors and then
//! cancels most of the angular motion again, this keeps the body's attitude kinematic:
//! four wheel probes find the ground and the body is levelled onto it.

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
const MASS: f32 = 4500.0;

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
    v.reject_from_normalized(normal).try_normalize().unwrap_or(fallback)
}

/// Advances one kart by `dt` (which should be small: a hundredth of a second or so).
pub fn step(k: &mut Kart, c: &Controls, world: &Collision, dt: f32) {
    let grounded = k.contacts > 0;
    let spinning = k.spin > 0.0;
    let boosting = k.boost > 0.0;
    let up = if grounded { k.ground_normal } else { Vec3::Y };
    let body_fwd = tangent(k.rot * Vec3::NEG_Z, up, Vec3::NEG_Z);
    let mut facing = tangent(k.facing, up, body_fwd);
    let speed = k.vel.length();
    let vel_dir = if speed > 1e-3 { k.vel / speed } else { body_fwd };
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
        let can_slide = !k.wall_contact && (k.hover || k.contacts >= 3) && aligned > POWERSLIDE_ALIGNMENT_MIN && vf >= POWERSLIDE_MIN_SPEED;
        if !k.drifting && can_slide {
            (k.drifting, k.sliding, k.slide_tight) = (true, true, c.tight);
        }
    } else {
        (k.drifting, k.sliding) = (false, false);
    }

    // --- DriveController::SetThrottleInput and ApplyThrust. A cursed driver's foot
    // is not their own.
    let cursed = k.cursed > 0.0;
    let throttle = if cursed { (c.throttle + k.curse_throttle).clamp(-1.0, 1.0) } else { c.throttle };
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
        radius = 1.0 / (INV_MAX_TURN_RADIUS + (INV_MIN_TURN_RADIUS - INV_MAX_TURN_RADIUS) * input.abs());
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
    let can_steer = aligned > 0.0 && !against && (k.slipping || aligned >= 0.9) && !k.wall_contact && speed <= STEER_MAX_SPEED;
    let mut slip: Option<Slip> = None;
    if radius > 0.0 {
        if k.sliding {
            if can_steer {
                let assist = (1.0 - radius / limit).max(0.05);
                slip = Some(if k.slide_tight { (1.0 + 2.0 * assist, 0.25, PI) } else { (1.0 + assist, 0.85, PI) });
                if radius < limit {
                    radius += (limit - radius) * 0.25;
                }
            }
        } else {
            if can_steer && radius < limit {
                slip = Some((2.0 - radius / limit, 0.85, 0.7071));
            }
            if radius < limit {
                radius = (limit + radius) * 0.5;
            }
        }
        radius = if radius > MAX_TURN_RADIUS { 0.0 } else { radius.max(MIN_TURN_RADIUS) };
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
    let mut yaw_rate = 0.0;
    let mut facing_rate = 0.0;
    if !grounded {
        acc.y -= GRAVITY * AIRBORNE_GRAVITY_SCALE;
        let mut push = facing * thrust;
        push.y = push.y.min(GRAVITY);
        acc += push;
        if radius != 0.0 && !turned {
            yaw_rate = vf / radius;
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
                acc += thrust * if k.contacts >= 3 && slip.is_none() { body_fwd } else { facing };
            } else {
                acc -= forward_vel * COAST_DAMPING;
            }
            if radius != 0.0 {
                // The pull to the middle of the turn is level, whatever the road is.
                acc += Vec3::Y.cross(facing).normalize_or_zero() * vf * vf / radius;
                facing_rate = vf / radius;
                yaw_rate = match slip {
                    Some((gain, _, max_lag)) if aligned >= max_lag.cos() => gain * facing_rate,
                    Some(_) => 0.0,
                    None if vf > 0.5 * UNIT && vf < CREEP_SPEED => CREEP_SPEED / radius,
                    None => facing_rate,
                };
            }
        }
    }
    if k.spin_out <= 0.0 && !k.hover {
        acc += Vec3::from(k.surface.force);
    }
    acc += k.external_force;
    if spinning {
        yaw_rate = k.spin_rate;
    } else if turned {
        yaw_rate = k.yaw_kick;
    }
    let rolling = if k.hover { 0.0 } else { k.surface.rolling_resistance };
    let drag = thrust.abs() / (max_speed * max_speed) + rolling / MASS / UNIT;
    acc -= k.vel * speed * drag;

    k.vel += acc * dt;
    k.rot = (Quat::from_axis_angle(up, yaw_rate * dt) * k.rot).normalize();

    // --- RacerCarBody::UpdateFacingDirection.
    let body_fwd = tangent(k.rot * Vec3::NEG_Z, up, body_fwd);
    match slip {
        Some((_, _, max_lag)) if grounded && k.contacts > 2 && vf >= CREEP_SPEED && thrust > 0.0 => {
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
    k.pos += k.vel * dt;
    collide_walls(k, world, previous_centre);
    probe_ground(k, world, dt);
}

/// Keeps the body out of walls: the kart's centre may not pass through one, and nor
/// may the lines from the centre to each corner of the body. The wall met deepest is
/// the one the car answers to.
fn collide_walls(k: &mut Kart, world: &Collision, previous_centre: Vec3) {
    k.wall_contact = false;
    let mut worst: Option<(f32, Vec3)> = None;
    let mut push_out = |k: &mut Kart, normal: Vec3, push: f32| {
        let Some(flat) = normal.with_y(0.0).try_normalize() else { return };
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
        k.yaw_kick = if side < 0.0 { -WALL_YAW * ((side + 1.0) * 0.5 + 0.5) } else { WALL_YAW * ((1.0 - side) * 0.5 + 0.5) };
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

/// Finds the ground under each wheel, rests the kart on it and levels the body.
fn probe_ground(k: &mut Kart, world: &Collision, dt: f32) {
    let was_grounded = k.contacts > 0 && k.spin_out <= 0.0;
    let reach = if was_grounded { CONTACT_PADDING } else { 0.0 };
    let mut hits = [None; 4];
    let mut lift = 0.0;
    let mut normal_sum = Vec3::ZERO;
    let had_contact = k.contacts > 0;
    k.contacts = 0;
    let mut force = Vec3::ZERO;
    for (wheel, hit_point) in k.wheels.iter().zip(&mut hits) {
        let foot = k.pos + k.rot * *wheel;
        let Some(hit) = world.ground(foot + Vec3::Y * STEP_UP, STEP_UP + reach) else { continue };
        *hit_point = Some(hit.point);
        lift += hit.point.y - foot.y;
        normal_sum += if hit.normal.y < 0.0 { -hit.normal } else { hit.normal };
        force += Vec3::from(hit.surface.force);
        if k.contacts == 0 {
            k.surface = hit.surface;
        } else {
            k.surface.rolling_resistance += hit.surface.rolling_resistance;
            k.surface.lateral_grip += hit.surface.lateral_grip;
            k.surface.friction += hit.surface.friction;
            k.surface.support += hit.surface.support;
        }
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
    let (bounces, bounce) = if k.hover { (true, HOVER_BOUNCE) } else { (k.air_time > LANDING_AIR_TIME, LANDING_BOUNCE) };
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
