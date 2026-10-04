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
//! Every other car is known a little late: the host's word of it is behind the host,
//! and the player's own car is driven ahead of the host. Shown as it was last said
//! to be, a car followed at speed would be drawn yards nearer than it is. So each is
//! shown where it will have got to by the moment the player's car is at, if it keeps
//! on as it was going between the last two words of it. A car that changes its mind
//! (hits a wall, is hit) is seen to a moment late, and is brought back.

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
    /// The knocks the player's car has given each other car that the host's word of
    /// that car doesn't show yet, by grid slot.
    shunts: [Vec<Shunt>; 6],
}

/// A knock the player's car gave another: at which of the player's steps, how far
/// it moved the other car and what speed it gave it. Until the host is seen to have
/// made the same of it, the other car is shown as knocked.
#[derive(Clone, Copy)]
struct Shunt {
    at: Tick,
    moved: Vec3,
    sped: Vec3,
    /// The host's step at which it had driven the player's car as far as the knock.
    heard: Option<Tick>,
}

impl Prediction {
    /// How far, and how much faster, a car is from where the host's word has it at
    /// one of the player's steps, for the knocks it had been given by then.
    fn knocked(&self, slot: usize, tick: Tick, dt: f32) -> (Vec3, Vec3) {
        let knocks = self.shunts.get(slot).map_or(&[][..], Vec::as_slice);
        knocks.iter().filter(|knock| knock.at <= tick).fold((Vec3::ZERO, Vec3::ZERO), |(moved, sped), knock| {
            (moved + knock.moved + knock.sped * ((tick - knock.at) as f32 * dt), sped + knock.sped)
        })
    }
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
    mut sfx: ResMut<crate::audio::Sfx>,
    clock: Res<Clock>,
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

    // The cars the player's own may run into: every other still in the race.
    let others: Vec<Entity> = karts.iter().filter(|(_, kart, own, shown)| (!own || *shown) && kart.out.is_none()).map(|(entity, ..)| entity).collect();
    let mut driven = None;
    for (entity, mut kart, own, shown) in &mut karts {
        let standing: Option<Standing> = snapshot.karts.iter().find(|k| k.0 as usize == kart.slot).map(|k| k.2);
        if !own || shown {
            if let Some(standing) = standing {
                standing.put(&mut kart);
            }
        } else {
            kart.place = standing.map_or(kart.place, |standing| standing.place as usize);
            // A white brick taken sounds as it does where the race is run, a note
            // higher for each one carried.
            if snapshot.own.whites() > kart.whites {
                sfx.play(crate::audio::id::WHITE_BRICK + kart.whites as usize);
            }
            // Its race run, the car is the host's to drive, and is shown like the rest.
            if snapshot.own.finished.is_some() || snapshot.own.out.is_some() {
                snapshot.own.put(&mut kart);
                commands.entity(entity).insert(Puppet);
            } else {
                driven = Some(entity);
            }
        }
        if own && kart.finished.is_some() && race.phase == Phase::Racing {
            race.phase = Phase::Finished;
        }
    }
    let Some(own) = driven else { return };
    let Ok((_, mut kart, ..)) = karts.get_mut(own) else { return };
    let was = (kart.pos, kart.rot);
    // Stepping the car again must not sound its sounds again.
    let owed = (kart.cues, kart.touched, kart.honked, kart.sparks);
    snapshot.own.put(&mut kart);
    p.sent.retain(|(tick, _)| *tick > snapshot.used);
    // The host has now driven the car past the knocks it gave up to there.
    for knock in p.shunts.iter_mut().flatten().filter(|knock| knock.heard.is_none() && knock.at <= snapshot.used) {
        knock.heard = Some(snapshot.tick);
    }
    // The others are shown as they were knocked up to the step before this one.
    let shown_at = clock.tick.saturating_sub(1);
    for (step, &(tick, drive)) in p.sent.iter().enumerate() {
        if let Ok((_, mut kart, ..)) = karts.get_mut(own) {
            kart.advance(&drive.controls(), &track, dt);
        }
        // Each step again meets the others as that step met them, knocked as far
        // as they had been by then and no farther.
        for &other in &others {
            if let Ok([(_, mut kart, ..), (_, mut other, ..)]) = karts.get_many_mut([own, other]) {
                // A step meets the others as they were shown at the end of the one before.
                let (now, then) = (p.knocked(other.slot, shown_at, dt), p.knocked(other.slot, tick.saturating_sub(1), dt));
                // The others are shown where they are by the last of these steps.
                let back = (p.sent.len() - 1 - step) as f32 * dt;
                meet_ahead(&mut kart, &mut other, -back, (then.0 - now.0, then.1 - now.1));
            }
        }
    }
    let Ok((_, mut kart, ..)) = karts.get_mut(own) else { return };
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

/// How far ahead of the host's word another car is shown, at the most, in seconds,
/// and the most it is taken to be speeding up or slowing by, a second.
const LEAD: f32 = 0.35;
const SURGE: f32 = 60.0;

/// The player's car bumps into another as that car is `lead` seconds from where it
/// is shown, at the speed it is going: a step driven again is one from a moment ago,
/// when the others were not yet where they are shown now. `knocked` is how much
/// farther on and faster the other is to be taken to be than it is shown. The other
/// car is left as it was, being shown from the host's word and not from what happens
/// here, and the bump is not sounded: the host's telling of it is. What the bump did
/// to the other car, in place and in speed, is given back.
fn meet_ahead(own: &mut Kart, other: &mut Kart, lead: f32, knocked: (Vec3, Vec3)) -> (Vec3, Vec3) {
    let was = (other.pos, other.vel, other.spin, other.spin_rate, other.cursed, other.boost, other.shove);
    other.vel += knocked.1;
    other.pos += knocked.0 + other.vel * lead;
    let met = (other.pos, other.vel);
    crate::kart::meet(own, other, None);
    let did = (other.pos - met.0, other.vel - met.1);
    (other.pos, other.vel, other.spin, other.spin_rate, other.cursed, other.boost, other.shove) = was;
    did
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

/// The player's car bumps into the others here and now, as it will be found to have
/// done on the host, and doesn't drive through them until the host says otherwise.
/// A car it knocks is shown knocked at once (`Shunt`).
pub fn bump(clock: Res<Clock>, mut p: ResMut<Prediction>, mut own: Query<&mut Kart, (With<Player>, Without<Puppet>)>, mut others: Query<&mut Kart, With<Puppet>>) {
    let (Ok(mut own), true) = (own.single_mut(), p.latest.is_some()) else { return };
    for mut other in others.iter_mut().filter(|other| other.out.is_none()) {
        let (moved, sped) = meet_ahead(&mut own, &mut other, 0.0, (Vec3::ZERO, Vec3::ZERO));
        if (moved != Vec3::ZERO || sped != Vec3::ZERO)
            && let Some(knocks) = p.shunts.get_mut(other.slot)
        {
            knocks.push(Shunt { at: clock.tick, moved, sped, heard: None });
        }
    }
}

/// Puts the cars the host drives where it had them a moment ago.
pub fn puppets(time: Res<Time>, clock: Res<Clock>, race: Res<Race>, mut p: ResMut<Prediction>, mut karts: Query<&mut Kart, With<Puppet>>) {
    let Some(latest) = p.latest else { return };
    // The moment shown moves on a step each step, and is drawn towards where it
    // should be if words from the host come faster or slower than that.
    let wanted = latest as f32 - BEHIND;
    p.shown = if (wanted - p.shown).abs() > 2.0 * BEHIND { wanted } else { p.shown + 1.0 + (wanted - p.shown - 1.0) * 0.1 };
    let shown = p.shown;
    let dt = time.delta_secs();
    // How far the player's car is ahead of the moment the host's word is read at:
    // the steps it has taken that the host hasn't answered, and how old the word is.
    let lead = ((p.sent.len() as f32 + (latest as f32 - shown).max(0.0)) * dt).min(LEAD);
    // A knock is done with once the cars are shown as the host had them after it.
    for knocks in &mut p.shunts {
        knocks.retain(|knock| knock.heard.is_none_or(|heard| shown < heard as f32));
    }
    for mut kart in &mut karts {
        let knocked = p.knocked(kart.slot, clock.tick, dt);
        let Some(seen) = p.seen.get_mut(kart.slot) else { continue };
        while seen.len() > 2 && seen[1].0 as f32 <= shown {
            seen.pop_front();
        }
        // The car as it was, and how its speed and heading were changing.
        let (pose, surge, turning) = match (seen.front(), seen.get(1)) {
            (Some(&(from, a)), Some(&(to, b))) => {
                let over = (to - from) as f32 * dt;
                let pose = a.towards(b, ((shown - from as f32) / (to - from) as f32).clamp(0.0, 1.0));
                (pose, ((b.vel - a.vel) / over).clamp_length_max(SURGE), Quat::IDENTITY.slerp(b.rot * a.rot.inverse(), lead / over))
            }
            (Some(&(_, only)), None) => (only, Vec3::ZERO, Quat::IDENTITY),
            _ => continue,
        };
        pose.put(&mut kart, race.time, dt);
        // On to where it will be by now, unless it is not driving anywhere: out of
        // the race, blown into the air, or whirling round.
        if kart.out.is_none() && kart.spin_out <= 0.0 && kart.spin <= 0.0 {
            let travel = kart.vel * lead + surge * (0.5 * lead * lead);
            kart.pos += travel;
            kart.vel += surge * lead;
            kart.rot = (turning * kart.rot).normalize();
        }
        // Knocked by the player's car, it is shown knocked until the host's word is.
        kart.pos += knocked.0;
        kart.vel += knocked.1;
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
