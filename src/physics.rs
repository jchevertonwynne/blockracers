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
/// A mid-range handling stat (0.7 + 0.003 * 50).
const HANDLING: f32 = 0.85;
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
const WALL_HORIZONTAL_DAMPING: f32 = 0.3;
/// Every champion's car weighs this (`CHAMPS.CCB`); it turns a surface's rolling
/// resistance into drag.
const MASS: f32 = 4500.0;

/// (yaw gain, slip ratio, maximum angle between body and direction of travel)
type Slip = (f32, f32, f32);

const WHEELS: [Vec3; 4] = [
    Vec3::new(-0.95, 0.0, -1.0),
    Vec3::new(0.95, 0.0, -1.0),
    Vec3::new(-0.95, 0.0, 1.0),
    Vec3::new(0.95, 0.0, 1.0),
];
const BODY_POINTS: [Vec3; 4] = [
    Vec3::new(-0.95, 0.6, -1.5),
    Vec3::new(0.95, 0.6, -1.5),
    Vec3::new(-0.95, 0.6, 1.5),
    Vec3::new(0.95, 0.6, 1.5),
];
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

    // --- DriveController: throttle to thrust.
    let throttle = if spinning { 0.0 } else { c.throttle };
    let mut thrust = THRUST * throttle;
    if throttle * vf < 0.0 {
        thrust *= BRAKE_SCALE;
    }
    let mut max_speed = MAX_SPEED * k.top_factor;
    let mut grip_scale = 1.0;
    if k.slow > 0.0 {
        max_speed *= 0.5;
    }
    if boosting {
        thrust = TURBO_THRUST * if k.wall_contact { 0.5 } else { 1.0 };
        max_speed = BOOST_MAX_SPEED;
        grip_scale = BOOST_GRIP_SCALE;
    }

    // --- DriveController: steering input to turn radius (positive turns left).
    let input = if spinning { 0.0 } else { k.steer * HANDLING };
    let mut radius = 0.0;
    if input.abs() > 1e-3 {
        radius = 1.0 / (INV_MAX_TURN_RADIUS + (INV_MIN_TURN_RADIUS - INV_MAX_TURN_RADIUS) * input.abs());
        if speed < LOW_SPEED {
            radius *= LOW_SPEED_STEER_SCALE;
        }
    }
    // The tightest circle the tyres can hold at this speed.
    let limit = if grounded {
        vf * vf / (k.surface.lateral_grip * grip_scale * GRAVITY)
    } else {
        FALLBACK_TURN_RADIUS
    };

    if c.drift
        && !k.sliding
        && k.contacts >= 3
        && !k.wall_contact
        && aligned > POWERSLIDE_ALIGNMENT_MIN
        && vf >= POWERSLIDE_MIN_SPEED
    {
        k.sliding = true;
    }
    if k.sliding && (!c.drift || vf < CREEP_SPEED) {
        k.sliding = false;
    }

    let can_steer = aligned > 0.0
        && (k.slipping || aligned >= 0.9)
        && !k.wall_contact
        && speed <= STEER_MAX_SPEED;
    let mut slip: Option<Slip> = None;
    if radius > 0.0 {
        if k.sliding {
            if can_steer {
                let assist = (1.0 - radius / limit).max(0.05);
                slip = Some((1.0 + 2.0 * assist, 0.25, PI));
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
    k.slipping = slip.is_some();

    // --- RacerCarBody::AccumulateForces, as accelerations.
    let mut acc = Vec3::ZERO;
    let mut yaw_rate = 0.0;
    let mut facing_rate = 0.0;
    if !grounded {
        acc.y -= GRAVITY * AIRBORNE_GRAVITY_SCALE;
        let mut push = facing * thrust;
        push.y = push.y.min(GRAVITY);
        acc += push;
        if radius != 0.0 {
            yaw_rate = vf / radius;
        }
    } else {
        // Gravity pulls along the slope; the ground carries the rest.
        acc += Vec3::NEG_Y * GRAVITY + up * GRAVITY * up.y;

        let forward_vel = facing * vf;
        let lateral_vel = k.vel - forward_vel - up * k.vel.dot(up);
        let mut contact_scale = 1.0;
        let slip_ratio = if spinning { Some(0.85) } else { slip.map(|s| s.1) };
        if let Some(ratio) = slip_ratio {
            if speed > 0.5 {
                acc -= vel_dir * GRAVITY * k.surface.friction * ratio;
            }
            contact_scale = 1.0 - ratio;
        }
        if spinning {
            acc -= lateral_vel * SPIN_LATERAL_DAMPING * contact_scale;
        } else {
            acc -= lateral_vel * LATERAL_DAMPING * contact_scale;
            if thrust != 0.0 {
                acc += thrust * if k.contacts >= 3 && slip.is_none() { body_fwd } else { facing };
            } else {
                acc -= forward_vel * COAST_DAMPING;
            }
            if radius != 0.0 {
                acc += up.cross(facing) * vf * vf / radius;
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
    let drag = thrust.abs() / (max_speed * max_speed) + k.surface.rolling_resistance / MASS / UNIT;
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

    let previous_centre = k.pos + Vec3::Y * BODY_POINTS[0].y;
    k.pos += k.vel * dt;
    collide_walls(k, world, previous_centre);
    probe_ground(k, world, dt);
}

/// Keeps the body out of walls: the kart's centre may not pass through one, and nor
/// may the lines from the centre to each corner of the body.
fn collide_walls(k: &mut Kart, world: &Collision, previous_centre: Vec3) {
    k.wall_contact = false;
    let respond = |k: &mut Kart, normal: Vec3, push: f32| {
        let Some(normal) = normal.with_y(0.0).try_normalize() else { return };
        k.pos += normal * (push + 0.01);
        let into = k.vel.dot(normal);
        if into < 0.0 {
            let head_on = (-into / k.vel.length().max(1e-3)).min(1.0);
            k.vel -= normal * into;
            let keep = 1.0 - WALL_HORIZONTAL_DAMPING * head_on;
            k.vel.x *= keep;
            k.vel.z *= keep;
        }
        k.wall_contact = true;
    };

    let centre = k.pos + Vec3::Y * BODY_POINTS[0].y;
    if let Some(hit) = world.wall(previous_centre, centre) {
        k.pos = hit.point - Vec3::Y * BODY_POINTS[0].y;
        respond(k, hit.normal, 0.3);
    }
    for _ in 0..2 {
        for corner in BODY_POINTS {
            let centre = k.pos + Vec3::Y * corner.y;
            let reach = k.pos + k.rot * corner - centre;
            if let Some(hit) = world.wall(centre, centre + reach) {
                let normal = hit.normal.with_y(0.0).normalize_or_zero();
                respond(k, hit.normal, (1.0 - hit.t) * reach.dot(-normal).max(0.0));
            }
        }
    }
}

/// Finds the ground under each wheel, rests the kart on it and levels the body.
fn probe_ground(k: &mut Kart, world: &Collision, dt: f32) {
    let was_grounded = k.contacts > 0;
    let reach = if was_grounded { CONTACT_PADDING } else { 0.0 };
    let mut hits = [None; 4];
    let mut lift = 0.0;
    let mut normal_sum = Vec3::ZERO;
    k.contacts = 0;
    for (wheel, hit_point) in WHEELS.iter().zip(&mut hits) {
        let foot = k.pos + k.rot * *wheel;
        let Some(hit) = world.ground(foot + Vec3::Y * STEP_UP, STEP_UP + reach) else { continue };
        *hit_point = Some(hit.point);
        lift += hit.point.y - foot.y;
        normal_sum += if hit.normal.y < 0.0 { -hit.normal } else { hit.normal };
        if k.contacts == 0 {
            k.surface = hit.surface;
        } else {
            k.surface.rolling_resistance += hit.surface.rolling_resistance;
            k.surface.lateral_grip += hit.surface.lateral_grip;
            k.surface.friction += hit.surface.friction;
        }
        k.contacts += 1;
    }
    if k.contacts == 0 {
        k.ground_normal = Vec3::Y;
        k.surface = default();
        k.air_time += dt;
        return;
    }
    let count = k.contacts as f32;
    k.surface.rolling_resistance /= count;
    k.surface.lateral_grip /= count;
    k.surface.friction /= count;
    k.air_time = 0.0;
    k.pos.y += lift / count;

    // With all four wheels down, the diagonals give the plane the kart sits on.
    let mut normal = normal_sum.normalize_or(Vec3::Y);
    if let [Some(fl), Some(fr), Some(rl), Some(rr)] = hits {
        let n = (fl - rr).cross(fr - rl);
        if let Some(n) = (n * n.y.signum()).try_normalize() {
            normal = n;
        }
    }
    k.ground_normal = normal;
    let into = k.vel.dot(normal);
    if into < 0.0 {
        k.vel -= normal * into;
    }
    let forward = tangent(k.rot * Vec3::NEG_Z, normal, Vec3::NEG_Z);
    let level = Transform::IDENTITY.looking_to(forward, normal).rotation;
    k.rot = k.rot.slerp(level, (15.0 * dt).min(1.0));
}
