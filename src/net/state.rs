//! A car as it goes over the network. The port's own.
//!
//! `State` is everything `Kart::advance` reads and writes for a car that is driven,
//! so that a player's game can take up its own car where the host has it and drive on
//! from there. The other cars are only looked at, and go as `replay::Pose` with the
//! `Standing` the display shows of them.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::items::Power;
use crate::kart::Kart;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Standing {
    pub lap: i32,
    pub place: u8,
    pub progress: f32,
    pub finished: Option<f32>,
}

impl Standing {
    pub fn of(k: &Kart) -> Self {
        Standing {
            lap: k.lap,
            place: k.place as u8,
            progress: k.progress,
            finished: k.finished,
        }
    }

    pub fn put(self, k: &mut Kart) {
        (k.lap, k.place, k.progress, k.finished) =
            (self.lap, self.place as usize, self.progress, self.finished);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct State {
    pos: Vec3,
    vel: Vec3,
    rot: Quat,
    facing: Vec3,
    contacts: u8,
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
}
