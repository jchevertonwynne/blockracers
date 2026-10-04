//! The race camera, as `RaceCameraController`: three views from behind the car and one
//! from the driver's seat, each trailing the car by its own amount, with a look behind.
//! It leans into corners and closes in when a turbo fires.

use crate::kart::Kart;
use crate::physics::UNIT;
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

/// A view from behind: how far down it looks and how far above the car it sits (both
/// as angles), how far back, and how much it lags in place and in turning.
struct View {
    pitch: f32,
    height: f32,
    distance: f32,
    position_lag: f32,
    rotation_lag: f32,
}

const VIEWS: [View; 3] = [
    View { pitch: 5.0, height: 35.0, distance: 20.0, position_lag: 0.1, rotation_lag: 0.25 },
    View { pitch: 8.0, height: 25.0, distance: 30.0, position_lag: 0.1, rotation_lag: 0.25 },
    View { pitch: 8.0, height: 45.0, distance: 10.0, position_lag: 0.05, rotation_lag: 0.25 },
];
/// The view a finished race is watched from: over two seconds the camera swings
/// round to the front of the car (`c_modeFinish`).
const FINISH: View = View { pitch: 20.0, height: 15.0, distance: 32.0, position_lag: 0.18, rotation_lag: 0.35 };
const FINISH_SWING: f32 = 2000.0;
/// The fourth view is the driver's.
const COCKPIT: usize = 3;
const COCKPIT_LAG: (f32, f32) = (0.1, 0.25);
/// How far above the car's origin the driver's eyes are.
const EYE_HEIGHT: f32 = 4.5 * UNIT;
/// The camera is never closer than this.
const MIN_DISTANCE: f32 = 12.0;
/// The view swings ahead of the car by this much of its rate of turn, up to a limit.
const TURN_LEAD: f32 = 0.08;
const TURN_LEAD_MAX: f32 = 0.3;
/// How quickly things settle, per millisecond: the lead, the heading, the point looked at.
const LEAD_RATE: f32 = 0.004_444_444_6;
const HEADING_RATE: f32 = 0.016_666_668;
const TARGET_RATE: f32 = 0.09;
/// Headings this close are left alone.
const HEADING_DEADBAND: f32 = 0.014;
/// A turbo of each level pulls the camera in to this much of its distance; it eases
/// back out at this rate a millisecond.
const TURBO_PULL: [f32; 3] = [0.2, 0.4, 0.6];
const TURBO_RELEASE: f32 = 0.002;

#[derive(Resource, Default)]
pub struct Rig {
    pub view: usize,
    pub look_back: bool,
    /// The heading the camera looks along, and how far it leads the car's.
    heading: f32,
    lead: f32,
    target: Vec3,
    position: Vec3,
    rotation: Quat,
    /// How much of the view's distance is in use; negative while a turbo's pull is easing off.
    reach: f32,
    boosting: bool,
    car_yaw: f32,
    settled: bool,
    /// Milliseconds into the swing round a finished car.
    finish: Option<f32>,
}

impl Rig {
    /// Forgets where it was: the next frame starts behind the car.
    pub fn reset(&mut self) {
        *self = Rig { view: self.view, ..default() };
    }

    /// Where the camera sits fixed behind the car, turning and tilting with it, with
    /// no lag at all: how a warp's tunnel is seen.
    pub fn fixed(&mut self, kart: &Kart) -> (Vec3, Quat) {
        let view = &VIEWS[self.view.min(VIEWS.len() - 1)];
        let (pitch, lift) = (view.pitch.to_radians(), view.height.to_radians().sin());
        let distance = view.distance * UNIT;
        let back = Vec3::new(0.0, pitch.sin() + lift, pitch.cos()) * distance;
        // The next frame out of the tunnel starts from behind the car again.
        self.settled = false;
        (kart.pos + kart.rot * back, kart.rot * Quat::from_rotation_x(-pitch))
    }

    /// A lag as the fraction of the old value kept after `ms` milliseconds.
    fn kept(lag: f32, ms: f32) -> f32 {
        1.0 / ((1.0 - lag) / (lag * 250.0) * ms + 1.0)
    }

    /// `RaceCameraController::SetView(4)`: the race is run, and the camera comes round.
    /// Any other time it is behind the car as usual.
    pub fn finished(&mut self, finished: bool) {
        if finished {
            self.finish.get_or_insert(0.0);
        } else {
            self.finish = None;
        }
    }

    /// Where the camera goes for the car as it is now, `dt` seconds on.
    pub fn follow(&mut self, kart: &Kart, dt: f32) -> (Vec3, Quat) {
        let ms = dt * 1000.0;
        let wrap = |angle: f32| (angle + PI).rem_euclid(TAU) - PI;
        if !self.settled {
            (self.heading, self.lead, self.target, self.reach, self.car_yaw) = (kart.yaw, 0.0, kart.pos, 1.0, kart.yaw);
        } else if kart.spin <= 0.0 && dt > 0.0 {
            // Swing ahead into the turn, and bring the heading round after the car's.
            let lead = (wrap(kart.yaw - self.car_yaw) / dt * TURN_LEAD).clamp(-TURN_LEAD_MAX, TURN_LEAD_MAX);
            self.lead = lead + (self.lead - lead) / (LEAD_RATE * ms + 1.0);
            let wanted = kart.yaw + self.lead;
            let off = wrap(self.heading - wanted);
            if off.abs() > HEADING_DEADBAND {
                self.heading = wanted + off / (HEADING_RATE * ms + 1.0);
            }
        }
        self.car_yaw = kart.yaw;
        self.target = kart.pos + (self.target - kart.pos) / (TARGET_RATE * ms + 1.0);

        // A turbo snatches the camera in close, and it drifts back out.
        let boosting = kart.boost > 0.0;
        if boosting && !self.boosting {
            self.reach = -TURBO_PULL[kart.boost_level.min(2) as usize];
        }
        self.boosting = boosting;
        if self.reach < 1.0 {
            self.reach = (self.reach + TURBO_RELEASE * ms).min(1.0);
        }

        if let Some(since) = &mut self.finish {
            *since = (*since + ms).min(FINISH_SWING);
        }
        let (raw_position, raw_rotation, lags) = if let Some(since) = self.finish {
            // From the view it had, which for the driver's is none at all.
            let from = VIEWS.get(self.view).map_or((0.0f32, 0.0f32, 0.0), |view| (view.pitch, view.height, view.distance));
            let left = 1.0 - since / FINISH_SWING;
            let mix = |from: f32, to: f32| to + (from - to) * left;
            let distance = mix(from.2, FINISH.distance) * UNIT;
            let pitch_sine = mix(from.0.to_radians().sin(), FINISH.pitch.to_radians().sin());
            let pitch_cosine = mix(from.0.to_radians().cos(), FINISH.pitch.to_radians().cos());
            let lift = mix(from.1.to_radians().sin(), FINISH.height.to_radians().sin());
            let level = Quat::from_rotation_y(since / FINISH_SWING * PI) * Vec3::new(-self.heading.sin(), 0.0, -self.heading.cos());
            let look = level * pitch_cosine - Vec3::Y * pitch_sine;
            let back = if self.settled { self.rotation * Vec3::NEG_Z } else { look };
            let position = self.target - back * distance + Vec3::Y * lift * distance;
            (position, Transform::IDENTITY.looking_to(look, Vec3::Y).rotation, (FINISH.position_lag, FINISH.rotation_lag))
        } else if self.view == COCKPIT {
            (kart.pos + kart.rot * Vec3::Y * EYE_HEIGHT, kart.rot, COCKPIT_LAG)
        } else {
            let view = &VIEWS[self.view.min(VIEWS.len() - 1)];
            let distance = if self.reach >= 1.0 { view.distance } else { (self.reach.abs() * view.distance).max(MIN_DISTANCE) } * UNIT;
            let (pitch, lift) = (view.pitch.to_radians(), view.height.to_radians().sin());
            let level = Vec3::new(-self.heading.sin(), 0.0, -self.heading.cos());
            let look = level * pitch.cos() - Vec3::Y * pitch.sin();
            // Back along the way it was last looking, which is what makes it swing.
            let back = if self.settled { self.rotation * Vec3::NEG_Z } else { look };
            let position = self.target - back * distance + Vec3::Y * lift * distance;
            (position, Transform::IDENTITY.looking_to(look, Vec3::Y).rotation, (view.position_lag, view.rotation_lag))
        };
        if self.settled {
            self.position = raw_position + (self.position - raw_position) * Self::kept(lags.0, ms);
            self.rotation = raw_rotation.slerp(self.rotation, Self::kept(lags.1, ms));
        } else {
            (self.position, self.rotation) = (raw_position, raw_rotation);
        }
        self.settled = true;

        if !self.look_back {
            return (self.position, self.rotation);
        }
        // Looking behind: from the far side of the car, facing the other way.
        let forward = self.rotation * Vec3::NEG_Z;
        let behind = Vec3::new(-forward.x, forward.y, -forward.z);
        let position = if self.view == COCKPIT {
            self.position
        } else {
            Vec3::new(2.0 * self.target.x - self.position.x, self.position.y, 2.0 * self.target.z - self.position.z)
        };
        (position, Transform::IDENTITY.looking_to(behind, Vec3::Y).rotation)
    }
}

#[cfg(test)]
#[test]
fn the_camera_settles_behind_the_car_and_closes_in_for_a_turbo() {
    let mut kart = Kart::new(&crate::track::Track::new(), 0);
    let mut rig = Rig::default();
    let forward = kart.rot * Vec3::NEG_Z;
    let behind = |rig: &mut Rig, kart: &Kart| {
        let mut at = (Vec3::ZERO, Quat::IDENTITY);
        for _ in 0..240 {
            at = rig.follow(kart, 1.0 / 60.0);
        }
        ((kart.pos - at.0).dot(forward), at.0.y - kart.pos.y, (at.1 * Vec3::NEG_Z).dot(forward))
    };
    // The first view: twenty units back less the tilt, lifted by its two angles.
    let (back, up, facing) = behind(&mut rig, &kart);
    let view = &VIEWS[0];
    assert!((back - view.distance * view.pitch.to_radians().cos() * UNIT).abs() < 0.05, "{back}");
    assert!((up - view.distance * (view.pitch.to_radians().sin() + view.height.to_radians().sin()) * UNIT).abs() < 0.05, "{up}");
    assert!(facing > 0.99);
    // A turbo brings it in to the least distance at once.
    (kart.boost, kart.boost_level) = (1.0, 0);
    let (position, _) = rig.follow(&kart, 1.0 / 60.0);
    assert!(rig.reach < 0.0 && position.distance(kart.pos) < back + up);
    // Looking back, it is in front and facing the car.
    kart.boost = 0.0;
    rig.look_back = true;
    let (back, _, facing) = behind(&mut rig, &kart);
    assert!(back < 0.0 && facing < -0.99);
}
