//! A car as it goes over the network. The port's own.
//!
//! `State` is everything `Kart::advance` reads and writes for a car that is driven,
//! so that a player's game can take up its own car where the host has it and drive on
//! from there. The other cars are only looked at, and go as a `Sight`: the
//! `replay::Pose` they are shown by and the `Standing` the display shows of them,
//! packed small, so that a `protocol::Snapshot` of a full grid fits a datagram.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::items::Power;
use crate::kart::Kart;
use crate::replay::Pose;
use crate::physics::Rigid;

/// What the display shows of a car besides where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Standing {
    pub lap: i32,
    pub place: u8,
    pub progress: f32,
    pub finished: Option<f32>,
    /// What it has left in a battle or a game of tag, and that it is "it".
    pub stake: f32,
    pub it: bool,
}

impl Standing {
    pub fn put(self, k: &mut Kart) {
        (k.lap, k.place, k.progress, k.finished) =
            (self.lap, self.place as usize, self.progress, self.finished);
        (k.stake, k.it) = (self.stake, self.it);
    }
}

/// How many of a `Sight`'s parts of a turn make one, and of its speeds a unit a
/// second; and how many of its timers a second, the longest it can say being a
/// little under thirteen.
const TURNED: f32 = 32767.0 * std::f32::consts::SQRT_2;
const SPED: f32 = 64.0;
const TIMED: f32 = 20.0;

/// Three numbers as they go in a `Sight`: sixteen bits each, whatever they are.
fn packed(parts: [f32; 3]) -> [u8; 6] {
    let mut bytes = [0; 6];
    for (two, part) in bytes.chunks_exact_mut(2).zip(parts) {
        two.copy_from_slice(&(part.round() as i16).to_le_bytes());
    }
    bytes
}

fn unpacked(bytes: [u8; 6]) -> [f32; 3] {
    [0, 2, 4].map(|at| i16::from_le_bytes([bytes[at], bytes[at + 1]]) as f32)
}

/// A car as the games that only look at it are told of it: a `replay::Pose` and a
/// `Standing`, in half the bytes. Where it is is told exactly; how it is turned
/// goes as the three smallest parts of its quaternion, its speed to a sixty-fourth
/// of a unit a second, and what power-ups have done to it to a twentieth of a
/// second. Its lap times change once a lap, and go with the `scene` when they do.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Sight {
    pub slot: u8,
    pos: Vec3,
    turn: [u8; 6],
    vel: [u8; 6],
    steer: i8,
    /// Which part of its turn is left out, in the low two bits, and above them
    /// that it is sliding, out of the race, and "it".
    bits: u8,
    /// What is left of its turbo, shield, whirl, flight, curse and magnet.
    timers: [u8; 6],
    held: Option<Power>,
    whites: u8,
    lap: i32,
    place: u8,
    progress: f32,
    finished: Option<f32>,
    stake: f32,
}

impl Sight {
    const SLIDING: u8 = 4;
    const OUT: u8 = 8;
    const IT: u8 = 16;

    pub fn of(k: &Kart) -> Self {
        // The biggest part of a turn is the one made up again from the others,
        // and is made positive, the same turn being said by both signs.
        let turn = k.rot.normalize().to_array();
        let most = (0..4).fold(0, |most, at| {
            if turn[at].abs() > turn[most].abs() {
                at
            } else {
                most
            }
        });
        let sign = TURNED.copysign(turn[most]);
        let mut rest = turn.into_iter().enumerate().filter(|(at, _)| *at != most);
        let rest = [(); 3].map(|_| rest.next().map_or(0.0, |(_, part)| part * sign));
        let limit = i16::MAX as f32 / SPED;
        let timer = |left: f32| (left * TIMED).ceil().clamp(0.0, 255.0) as u8;
        Sight {
            slot: k.slot as u8,
            pos: k.pos,
            turn: packed(rest),
            vel: packed((k.vel.clamp(Vec3::splat(-limit), Vec3::splat(limit)) * SPED).to_array()),
            steer: (k.steer.clamp(-1.0, 1.0) * 127.0).round() as i8,
            bits: most as u8
                | if k.sliding { Self::SLIDING } else { 0 }
                | if k.out.is_some() { Self::OUT } else { 0 }
                | if k.it { Self::IT } else { 0 },
            timers: [k.boost, k.shield, k.spin, k.spin_out, k.cursed, k.magnet].map(timer),
            held: k.held,
            whites: k.whites,
            lap: k.lap,
            place: k.place as u8,
            progress: k.progress,
            finished: k.finished,
            stake: k.stake,
        }
    }

    /// Where the car is and how it looks.
    pub fn pose(&self) -> Pose {
        let rest = unpacked(self.turn).map(|part| part / TURNED);
        let most = (self.bits & 3) as usize;
        let left = (1.0 - rest.iter().map(|part| part * part).sum::<f32>()).max(0.0).sqrt();
        let mut rest = rest.into_iter();
        let turn = [0, 1, 2, 3].map(|at| {
            if at == most {
                left
            } else {
                rest.next().unwrap_or(0.0)
            }
        });
        let [boost, shield, spin, spin_out, cursed, magnet] =
            self.timers.map(|left| left as f32 / TIMED);
        Pose {
            pos: self.pos,
            rot: Quat::from_array(turn).normalize(),
            vel: Vec3::from_array(unpacked(self.vel)) / SPED,
            steer: self.steer as f32 / 127.0,
            boost,
            shield,
            sliding: self.bits & Self::SLIDING != 0,
            out: self.bits & Self::OUT != 0,
            spin,
            spin_out,
            cursed,
            magnet,
            held: self.held,
            whites: self.whites,
        }
    }

    pub fn standing(&self) -> Standing {
        Standing {
            lap: self.lap,
            place: self.place,
            progress: self.progress,
            finished: self.finished,
            stake: self.stake,
            it: self.bits & Self::IT != 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct State {
    pos: Vec3,
    vel: Vec3,
    rot: Quat,
    facing: Vec3,
    contacts: u8,
    wheel_mask: u8,
    /// The body's angular momentum and what is left of its impulses.
    rigid: Rigid,
    ground_normal: Vec3,
    /// What of the surface under the car its driving feels: how it rolls, grips,
    /// holds on a slope and is pushed.
    surface: ([f32; 4], [f32; 3]),
    wall_contact: bool,
    air_time: f32,
    sliding: bool,
    slide_tight: bool,
    drifting: bool,
    slipping: bool,
    slip_ratio: f32,
    turn_radius: f32,
    yaw_impulse: f32,
    yaw_kick: f32,
    turbo_weak: bool,
    curse_timer: f32,
    curse_throttle: f32,
    safe: (Vec3, Quat),
    shove: (Vec3, f32),
    noise: u32,
    steer: f32,
    top_factor: f32,
    track: (u32, f32, f32),
    lap: i32,
    progress: f32,
    pub finished: Option<f32>,
    pub out: Option<f32>,
    held: Option<Power>,
    whites: u8,
    spin: f32,
    spin_rate: f32,
    spin_out: f32,
    boost: f32,
    boost_level: u8,
    shield: f32,
    shield_level: u8,
    cursed: f32,
    magnet: f32,
    warp: f32,
    warp_start: f32,
    warp_to: Option<(Vec3, Vec3, Vec3)>,
    checkpoint: Option<u32>,
    checkpoint_forward: bool,
    checkpoint_count: i32,
    crossed_backward: bool,
    zones: [u8; 3],
    hover: bool,
    hover_lift: f32,
    hover_bank: f32,
    ended: bool,
}

impl State {
    /// The white bricks the car carries.
    pub fn whites(&self) -> u8 {
        self.whites
    }

    pub fn of(k: &Kart) -> Self {
        State {
            pos: k.pos,
            vel: k.vel,
            rot: k.rot,
            facing: k.facing,
            contacts: k.contacts,
            wheel_mask: k.wheel_mask,
            rigid: k.rigid,
            ground_normal: k.ground_normal,
            surface: (
                [
                    k.surface.rolling_resistance,
                    k.surface.friction,
                    k.surface.lateral_grip,
                    k.surface.support,
                ],
                k.surface.force,
            ),
            wall_contact: k.wall_contact,
            air_time: k.air_time,
            sliding: k.sliding,
            slide_tight: k.slide_tight,
            drifting: k.drifting,
            slipping: k.slipping,
            slip_ratio: k.slip_ratio,
            turn_radius: k.turn_radius,
            yaw_impulse: k.yaw_impulse,
            yaw_kick: k.yaw_kick,
            turbo_weak: k.turbo_weak,
            curse_timer: k.curse_timer,
            curse_throttle: k.curse_throttle,
            safe: k.safe,
            shove: k.shove,
            noise: k.noise,
            steer: k.steer,
            top_factor: k.top_factor,
            track: (k.idx as u32, k.s, k.lat),
            lap: k.lap,
            progress: k.progress,
            finished: k.finished,
            out: k.out,
            held: k.held,
            whites: k.whites,
            spin: k.spin,
            spin_rate: k.spin_rate,
            spin_out: k.spin_out,
            boost: k.boost,
            boost_level: k.boost_level,
            shield: k.shield,
            shield_level: k.shield_level,
            cursed: k.cursed,
            magnet: k.magnet,
            warp: k.warp,
            warp_start: k.warp_start,
            warp_to: k.warp_to,
            checkpoint: k.checkpoint.map(|gate| gate as u32),
            checkpoint_forward: k.checkpoint_forward,
            checkpoint_count: k.checkpoint_count,
            crossed_backward: k.crossed_backward,
            zones: k.zones,
            hover: k.hover,
            hover_lift: k.hover_lift,
            hover_bank: k.hover_bank,
            ended: k.ended,
        }
    }

    /// Makes the car as the host has it. What is only seen or heard of a car (its
    /// wheels' turning, the sounds it owes) is left as it was.
    pub fn put(&self, k: &mut Kart) {
        let s = self.clone();
        (k.pos, k.vel, k.rot, k.facing) = (s.pos, s.vel, s.rot, s.facing);
        (k.contacts, k.ground_normal, k.wall_contact, k.air_time) =
            (s.contacts, s.ground_normal, s.wall_contact, s.air_time);
        (k.wheel_mask, k.rigid) = (s.wheel_mask, s.rigid);
        (k.sliding, k.slide_tight, k.drifting, k.slipping) =
            (s.sliding, s.slide_tight, s.drifting, s.slipping);
        (k.slip_ratio, k.turn_radius, k.yaw_impulse, k.yaw_kick) =
            (s.slip_ratio, s.turn_radius, s.yaw_impulse, s.yaw_kick);
        (k.turbo_weak, k.curse_timer, k.curse_throttle) =
            (s.turbo_weak, s.curse_timer, s.curse_throttle);
        (k.safe, k.shove, k.noise, k.steer, k.top_factor) =
            (s.safe, s.shove, s.noise, s.steer, s.top_factor);
        (k.idx, k.s, k.lat) = (s.track.0 as usize, s.track.1, s.track.2);
        (k.lap, k.progress, k.finished, k.out) = (s.lap, s.progress, s.finished, s.out);
        (k.held, k.whites) = (s.held, s.whites);
        (k.spin, k.spin_rate, k.spin_out) = (s.spin, s.spin_rate, s.spin_out);
        (k.boost, k.boost_level, k.shield, k.shield_level) =
            (s.boost, s.boost_level, s.shield, s.shield_level);
        (k.cursed, k.magnet, k.warp, k.warp_start, k.warp_to) =
            (s.cursed, s.magnet, s.warp, s.warp_start, s.warp_to);
        (k.checkpoint, k.checkpoint_forward) =
            (s.checkpoint.map(|gate| gate as usize), s.checkpoint_forward);
        (k.checkpoint_count, k.crossed_backward, k.zones) =
            (s.checkpoint_count, s.crossed_backward, s.zones);
        (k.hover, k.hover_lift, k.hover_bank, k.ended) =
            (s.hover, s.hover_lift, s.hover_bank, s.ended);
        (
            [
                k.surface.rolling_resistance,
                k.surface.friction,
                k.surface.lateral_grip,
                k.surface.support,
            ],
            k.surface.force,
        ) = s.surface;
        let forward = k.rot * Vec3::NEG_Z;
        k.yaw = (-forward.x).atan2(-forward.z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::Controls;
    use crate::track::Track;

    /// A car taken up from its `State` drives on exactly as the car it was taken from.
    #[test]
    fn a_car_taken_up_from_its_state_drives_on_the_same() {
        let track = Track::new();
        let dt = 1.0 / 60.0;
        let c = Controls {
            throttle: 1.0,
            steer: 0.6,
            ..default()
        };
        let mut driven = Kart::new(&track, 2);
        for _ in 0..90 {
            driven.advance(&c, &track, dt);
        }
        // A second car, from elsewhere on the grid, made into the first.
        let mut taken = Kart::new(&track, 4);
        State::of(&driven).put(&mut taken);
        for step in 0..240 {
            // Something of everything: a turn the other way, a slide, a curse's dice.
            let c = Controls {
                throttle: 1.0,
                steer: if step < 120 { -1.0 } else { 1.0 },
                drift: step > 60,
                ..default()
            };
            if step == 30 {
                driven.curse(2.0);
                taken.curse(2.0);
            }
            driven.advance(&c, &track, dt);
            taken.advance(&c, &track, dt);
        }
        assert!(driven.vel.length() > 5.0, "the car should have got going");
        assert_eq!(State::of(&driven), State::of(&taken));
    }

    /// A car is shown from its `Sight` as near as makes no difference to where and
    /// how it was, whichever way it is turned.
    #[test]
    fn a_car_is_seen_as_it_was() {
        let track = Track::new();
        let mut k = Kart::new(&track, 3);
        (k.vel, k.steer, k.sliding) = (Vec3::new(31.4159, -2.5, -47.123), -0.37, true);
        (k.boost, k.shield, k.spin_out, k.cursed) = (0.31, 40.0, 0.0, 0.004);
        (k.held, k.whites, k.it, k.stake) = (Some(Power::Blue), 2, true, 17.5);
        (k.lap, k.place, k.progress, k.finished) = (2, 4, 1234.5, Some(61.25));
        let turns = [
            Quat::IDENTITY,
            Quat::from_rotation_y(3.1),
            Quat::from_euler(EulerRot::YXZ, -2.0, 0.4, -0.3),
            Quat::from_euler(EulerRot::YXZ, 1.0, -1.5, 3.0),
            -Quat::from_rotation_x(1.6),
        ];
        for turn in turns {
            k.rot = turn;
            let sight = Sight::of(&k);
            let (pose, was) = (sight.pose(), Pose::of(&k));
            assert_eq!(pose.pos, was.pos);
            assert!(pose.rot.angle_between(was.rot) < 1e-3, "turned {turn:?}");
            assert!(pose.vel.distance(was.vel) < 0.02);
            assert!((pose.steer - was.steer).abs() < 0.005);
            assert_eq!((pose.sliding, pose.out, pose.held, pose.whites), (true, false, was.held, 2));
            // What is left of something is never said to be none while there is some,
            // and more than can be said is said as the most that can.
            assert!((pose.boost - 0.31).abs() <= 0.05 && pose.cursed > 0.0);
            assert_eq!((pose.shield, pose.spin_out), (12.75, 0.0));
            let standing = sight.standing();
            assert_eq!((standing.lap, standing.place, standing.progress), (2, 4, 1234.5));
            assert_eq!((standing.finished, standing.stake, standing.it), (Some(61.25), 17.5, true));
        }
    }
}
