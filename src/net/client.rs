//! A player's side of a race online. The port's own.
//!
//! The player's own car is driven here at once, without waiting to hear from the
//! host: each step's pressing is sent and remembered, and the car stepped by it.
//! When the host says where it has the car, and which of the player's steps it had
//! driven it by, the car is put there and stepped again by the steps since. Mostly
//! that lands where the car already was. When it doesn't (another car was in the
//! way, a cannonball arrived), the car is where the host says, and what the player
//! sees of it is brought across over a moment rather than jumped.
//!
//! Every other car is shown a little behind the host, between the last two places it
//! was said to be.

use std::collections::VecDeque;

use bevy::prelude::*;

use super::protocol::{Drive, HOST, Inputs, RESENT, Stage, Tick, ToHost, encode};
use super::state::Standing;
use super::{Clock, Inbox, Puppet, Wire};
use crate::kart::{Controls, Kart, Player};
use crate::replay::Pose;
use crate::track::Track;
use crate::{Phase, Race};

/// The other cars are shown this many of the host's steps behind the newest heard of:
/// enough that there are nearly always two places to show a car between.
const BEHIND: f32 = 6.0;
/// The steps remembered for stepping the car again: two seconds' worth, far more than
/// any connection worth racing on leaves unanswered.
const REMEMBERED: usize = 120;
/// What is seen of a correction is brought across at this rate, a second; one bigger
/// than this is not smoothed at all.
const SMOOTHING: f32 = 12.0;
const JUMP: f32 = 12.0;

#[derive(Resource, Default)]
pub struct Prediction {
    /// The steps sent that the host hasn't yet said it used.
    sent: VecDeque<(Tick, Drive)>,
    /// The host's step last heard from.
    latest: Option<Tick>,
    /// Where each car has been said to be, by grid slot, oldest first.
    seen: [VecDeque<(Tick, Pose)>; 6],
    /// The host's step the other cars are being shown at.
    shown: f32,
    /// How far, and how far round, the player's car is shown from where it is.
    pub offset: Vec3,
    pub turn: Quat,
    /// How far the last word from the host moved the player's car.
    pub corrected: f32,
}

/// Takes in what the host has said: the race's clocks, where every car is, and where
/// the player's own car was when the host had driven it as far as it had heard.
pub fn receive(
    mut commands: Commands,
    time: Res<Time>,
    track: Res<Track>,
    mut inbox: ResMut<Inbox>,
    mut race: ResMut<Race>,
    mut p: ResMut<Prediction>,
    mut karts: Query<(Entity, &mut Kart, Has<Player>, Has<Puppet>)>,
) {
    let dt = time.delta_secs();
    // Between words from the host the clocks run on by themselves.
    match race.phase {
        Phase::Intro => {}
        Phase::Countdown => race.countdown -= dt,
        Phase::Racing | Phase::Finished => race.time += dt,
    }
    let mut heard = std::mem::take(&mut inbox.snapshots);
    heard.retain(|snapshot| p.latest.is_none_or(|latest| snapshot.tick > latest));
    heard.sort_by_key(|snapshot| snapshot.tick);
    for snapshot in &heard {
        for (slot, pose, _) in &snapshot.karts {
            if let Some(seen) = p.seen.get_mut(*slot as usize) {
                seen.push_back((snapshot.tick, *pose));
            }
        }
    }
    let Some(snapshot) = heard.pop() else { return };
    p.latest = Some(snapshot.tick);
    [race.intro, race.countdown, race.time] = snapshot.clocks;
    race.phase = match snapshot.stage {
        Stage::Intro => Phase::Intro,
        Stage::Countdown => Phase::Countdown,
        Stage::Racing => Phase::Racing,
    };

    for (entity, mut kart, own, shown) in &mut karts {
        let standing: Option<Standing> = snapshot.karts.iter().find(|k| k.0 as usize == kart.slot).map(|k| k.2);
        if !own || shown {
            if let Some(standing) = standing {
                standing.put(&mut kart);
            }
        } else {
            kart.place = standing.map_or(kart.place, |standing| standing.place as usize);
            // Its race run, the car is the host's to drive, and is shown like the rest.
            if snapshot.own.finished.is_some() || snapshot.own.out.is_some() {
                snapshot.own.put(&mut kart);
                commands.entity(entity).insert(Puppet);
            } else {
                let was = (kart.pos, kart.rot);
                // Stepping the car again must not sound its sounds again.
                let owed = (kart.cues, kart.touched, kart.honked, kart.sparks);
                snapshot.own.put(&mut kart);
                p.sent.retain(|(tick, _)| *tick > snapshot.used);
                for (_, drive) in &p.sent {
                    kart.advance(&drive.controls(), &track, dt);
                }
                (kart.cues, kart.touched, kart.honked, kart.sparks) = owed;
                // What is seen stays where it was, and is brought across from there.
                let moved = was.0 - kart.pos;
                p.corrected = moved.length();
                if p.corrected > JUMP {
                    (p.offset, p.turn) = (Vec3::ZERO, Quat::IDENTITY);
                } else {
                    p.offset += moved;
                    p.turn = (p.turn * was.1 * kart.rot.inverse()).normalize();
                }
            }
        }
        if own && kart.finished.is_some() && race.phase == Phase::Racing {
            race.phase = Phase::Finished;
        }
    }
}

/// Sends this step's pressing, with the last few steps' again in case they were lost.
pub fn send(clock: Res<Clock>, mut wire: ResMut<Wire>, mut p: ResMut<Prediction>, own: Query<&Controls, (With<Player>, Without<Puppet>)>) {
    let Ok(c) = own.single() else { return };
    p.sent.push_back((clock.tick, Drive::of(c)));
    while p.sent.len() > REMEMBERED {
        p.sent.pop_front();
    }
    let drives: Vec<Drive> = p.sent.iter().rev().take(RESENT).rev().map(|(_, drive)| *drive).collect();
    wire.0.datagram(HOST, encode(&Inputs { last: clock.tick, drives }));
}

/// Puts the cars the host drives where it had them a moment ago.
pub fn puppets(time: Res<Time>, race: Res<Race>, mut p: ResMut<Prediction>, mut karts: Query<&mut Kart, With<Puppet>>) {
    let Some(latest) = p.latest else { return };
    // The moment shown moves on a step each step, and is drawn towards where it
    // should be if words from the host come faster or slower than that.
    let wanted = latest as f32 - BEHIND;
    p.shown = if (wanted - p.shown).abs() > 2.0 * BEHIND { wanted } else { p.shown + 1.0 + (wanted - p.shown - 1.0) * 0.1 };
    let shown = p.shown;
    for mut kart in &mut karts {
        let Some(seen) = p.seen.get_mut(kart.slot) else { continue };
        while seen.len() > 2 && seen[1].0 as f32 <= shown {
            seen.pop_front();
        }
        let pose = match (seen.front(), seen.get(1)) {
            (Some(&(from, a)), Some(&(to, b))) => a.towards(b, ((shown - from as f32) / (to - from) as f32).clamp(0.0, 1.0)),
            (Some(&(_, only)), None) => only,
            _ => continue,
        };
        pose.put(&mut kart, race.time, time.delta_secs());
    }
}

/// Shows the player's car where it was seen before a correction, less each frame.
pub fn smooth(time: Res<Time>, mut p: ResMut<Prediction>, mut own: Query<&mut Transform, (With<Player>, Without<Puppet>)>) {
    let keep = (-SMOOTHING * time.delta_secs()).exp();
    p.offset *= keep;
    p.turn = Quat::IDENTITY.slerp(p.turn, keep);
    if let Ok(mut transform) = own.single_mut() {
        transform.translation += p.offset;
        transform.rotation = p.turn * transform.rotation;
    }
}

/// Tells the host the race is loaded and its cars are on the grid.
pub fn loaded(mut wire: ResMut<Wire>) {
    wire.0.send(HOST, encode(&ToHost::Loaded));
}
