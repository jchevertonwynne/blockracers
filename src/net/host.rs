//! The host's side of a race online. The port's own.
//!
//! The host runs the race as it would alone: every car, the computer's and the other
//! players' alike, is stepped here, and what happens here is what happened. The other
//! players' cars are driven by what their games say they are pressing, a step at a
//! time, and thirty times a second each player is told how everything stands.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;

use super::protocol::{Drive, Peer, SNAPSHOT_EVERY, Snapshot, Stage, Tick, ToPlayer, encode};
use super::room::Finish;
use super::state::{Standing, State};
use super::{Clock, Inbox, Session, Wire};
use crate::kart::{Controls, Kart, Player};
use crate::menu::Screen;
use crate::replay::Pose;
use crate::{Phase, Race};

/// A player's steps are kept waiting no deeper than this; a game that has got ahead
/// of the host has its oldest dropped.
const QUEUE: usize = 4;
/// How long the start waits for a player's game to load the circuit.
const LOAD_WAIT: f32 = 20.0;
/// How long the cars still out are given once the first player is home, as the
/// original gives the field (`RaceSession::UpdateFinishedState`), and how long the
/// result is then looked at.
const FINISH_WAIT: f32 = 10.0;
const RESULT_WAIT: f32 = 5.0;

/// A car driven by a player somewhere else.
#[derive(Component)]
pub struct Remote {
    pub peer: Peer,
    /// The steps heard of and not yet driven, oldest first.
    queue: VecDeque<(Tick, Drive)>,
    newest: Option<Tick>,
    /// The last of the player's steps the car has been driven by.
    used: Tick,
}

impl Remote {
    pub fn new(peer: Peer) -> Self {
        Remote {
            peer,
            queue: VecDeque::new(),
            newest: None,
            used: 0,
        }
    }

    /// Takes in what a player's game says it pressed, less what was heard before.
    fn hear(&mut self, last: Tick, drives: Vec<Drive>) {
        let count = drives.len() as Tick;
        for (i, drive) in drives.into_iter().enumerate() {
            let Some(tick) = (last + 1 + i as Tick).checked_sub(count) else {
                continue;
            };
            if self.newest.is_none_or(|newest| tick > newest) {
                self.queue.push_back((tick, drive));
                self.newest = Some(tick);
            }
        }
        while self.queue.len() > QUEUE {
            // A step dropped is not driven, but a button pressed in it was pressed.
            if let (Some((_, old)), Some((_, next))) =
                (self.queue.pop_front(), self.queue.front_mut())
            {
                next.use_item |= old.use_item;
                next.start_boost = next.start_boost.or(old.start_boost);
            }
        }
    }
}

/// How the race is getting on, for `flow`.
#[derive(Resource, Default)]
pub struct Flow {
    /// How long the start has waited for the players' games.
    waited: f32,
    /// How long the result has been looked at.
    shown: f32,
    /// Each car's laps, by grid slot: the lap it is on, when it began it, and the
    /// quickest it has finished.
    laps: HashMap<usize, (i32, f32, Option<f32>)>,
    /// The cars that were still out when the race was ended for them.
    unfinished: Vec<usize>,
}

pub fn receive(mut inbox: ResMut<Inbox>, mut remotes: Query<&mut Remote>) {
    for (peer, inputs) in inbox.inputs.drain(..) {
        if let Some(mut remote) = remotes.iter_mut().find(|remote| remote.peer == peer) {
            remote.hear(inputs.last, inputs.drives);
        }
    }
}

/// Sets each player's car to what they pressed in the next of their steps. A step
/// that hasn't come is driven as the last was, with no button newly pressed.
pub fn drive_remotes(mut remotes: Query<(&Kart, &mut Remote, &mut Controls)>) {
    for (kart, mut remote, mut c) in &mut remotes {
        let next = remote.queue.pop_front();
        if let Some((tick, _)) = next {
            remote.used = tick;
        }
        // Its race run, the car is the computer's to drive.
        if kart.finished.is_some() || kart.out.is_some() {
            continue;
        }
        match next {
            Some((_, drive)) => *c = drive.controls(),
            None => (c.use_item, c.start_boost) = (false, None),
        }
    }
}

/// The race's phases, as `race_flow` runs them for a game alone, but waiting for the
/// players at the start and for all of them at the finish.
pub fn flow(
    time: Res<Time>,
    mut race: ResMut<Race>,
    mut flow: ResMut<Flow>,
    mut session: ResMut<Session>,
    mut room: ResMut<super::room::Room>,
    mut wire: ResMut<Wire>,
    mut next: ResMut<NextState<Screen>>,
    mut karts: Query<(&mut Kart, Has<Player>, Option<&Remote>)>,
    lineup: Option<Res<super::Lineup>>,
) {
    let dt = time.delta_secs();
    match race.phase {
        Phase::Intro => {
            // The drop-in begins once everyone is there to see it.
            // Whoever is in the room and not in the race is not waited for.
            let ready = karts.iter().filter_map(|k| k.2).all(|remote| {
                session
                    .members
                    .iter()
                    .any(|member| member.peer == remote.peer && member.loaded)
            });
            if !ready && flow.waited < LOAD_WAIT {
                flow.waited += dt;
                return;
            }
            race.intro -= dt;
            if race.intro <= 0.0 || race.quick {
                race.phase = Phase::Countdown;
            }
        }
        Phase::Countdown => {
            race.countdown -= dt;
            if race.countdown <= 0.0 || race.quick {
                (race.phase, race.time) = (Phase::Racing, 0.0);
            }
        }
        Phase::Racing | Phase::Finished => {
            race.time += dt;
            // Over, on this screen, when the car driven here is home.
            if karts
                .iter()
                .any(|(k, here, _)| here && k.finished.is_some())
            {
                race.phase = Phase::Finished;
            }
        }
    }
    if !matches!(race.phase, Phase::Racing | Phase::Finished) {
        return;
    }
    // Each car's laps are timed as the display times the player's.
    for (k, ..) in &karts {
        let now = k.finished.unwrap_or(race.time);
        let (lap, began, best) = flow.laps.entry(k.slot).or_insert((k.lap, now, None));
        if k.lap > *lap {
            if *lap >= 1 {
                *best = Some(best.map_or(now - *began, |best| best.min(now - *began)));
            }
            (*lap, *began) = (k.lap, now);
        }
    }
    // The players still out are given a while after the first of them is home; the
    // computer's cars are only waited for as long as a player is.
    let players = || {
        karts
            .iter()
            .filter(|(_, here, elsewhere)| *here || elsewhere.is_some())
            .map(|(k, ..)| k)
    };
    let first_home = players().filter_map(|k| k.finished).reduce(f32::min);
    let still_out = players().any(|k| k.finished.is_none() && k.out.is_none());
    if still_out && !first_home.is_some_and(|at| race.time - at >= FINISH_WAIT) {
        return;
    }
    // Cars still out take the places left in the order of the grid, as
    // `RaceSession::UpdateFinishedState` gives them.
    let mut waiting: Vec<Mut<Kart>> = karts
        .iter_mut()
        .map(|(k, ..)| k)
        .filter(|k| k.finished.is_none() && k.out.is_none())
        .collect();
    waiting.sort_by_key(|k| k.slot);
    for (i, k) in waiting.iter_mut().enumerate() {
        k.finished = Some(race.time + i as f32 * 1e-3);
        flow.unfinished.push(k.slot);
    }
    flow.shown += dt;
    if flow.shown >= RESULT_WAIT {
        let mut order: Vec<(usize, Finish, usize)> = karts
            .iter()
            .map(|(k, ..)| {
                // A player's car is theirs in the results though they gave it up.
                let player = lineup.as_ref().is_some_and(|lineup| {
                    lineup
                        .seats
                        .iter()
                        .any(|seat| seat.slot as usize == k.slot && seat.peer.is_some())
                });
                let time = k
                    .finished
                    .filter(|_| k.out.is_none() && !flow.unfinished.contains(&k.slot));
                let best = flow.laps.get(&k.slot).and_then(|laps| laps.2);
                (
                    k.place,
                    Finish {
                        name: k.name.to_string(),
                        player,
                        time,
                        best,
                        points: 0,
                    },
                    k.slot,
                )
            })
            .collect();
        order.sort_by_key(|(place, ..)| *place);
        // Each place scores, and the players keep what theirs did.
        let slots: Vec<usize> = order.iter().map(|(.., slot)| *slot).collect();
        (room.results, room.fresh) = (
            order.into_iter().map(|(_, finish, _)| finish).collect(),
            true,
        );
        room.score(|place| {
            let seats = &lineup.as_ref()?.seats;
            seats
                .iter()
                .find(|seat| seat.slot as usize == slots[place])?
                .peer
        });
        (session.racing, session.watching) = (None, Vec::new());
        let over = encode(&ToPlayer::Over);
        for member in &mut session.members {
            wire.0.send(member.peer, over.clone());
            member.loaded = false;
        }
        next.set(Screen::Menu);
    }
}

/// Tells each player how things stand.
pub fn send(
    clock: Res<Clock>,
    race: Res<Race>,
    session: Res<Session>,
    mut wire: ResMut<Wire>,
    karts: Query<(&Kart, Option<&Remote>)>,
) {
    if !clock.tick.is_multiple_of(SNAPSHOT_EVERY) {
        return;
    }
    let stage = match race.phase {
        Phase::Intro => Stage::Intro,
        Phase::Countdown => Stage::Countdown,
        Phase::Racing | Phase::Finished => Stage::Racing,
    };
    let all: Vec<(u8, Pose, Standing)> = karts
        .iter()
        .map(|(k, _)| (k.slot as u8, Pose::of(k), Standing::of(k)))
        .collect();
    for (kart, remote) in &karts {
        let Some(remote) = remote else { continue };
        let snapshot = Snapshot {
            tick: clock.tick,
            used: remote.used,
            stage,
            clocks: [race.intro, race.countdown, race.time],
            own: Some(State::of(kart)),
            karts: all.clone(),
        };
        wire.0.datagram(remote.peer, encode(&snapshot));
    }
    // Those watching are told of every car and of none in particular.
    for &peer in &session.watching {
        let snapshot = Snapshot {
            tick: clock.tick,
            used: 0,
            stage,
            clocks: [race.intro, race.countdown, race.time],
            own: None,
            karts: all.clone(),
        };
        wire.0.datagram(peer, encode(&snapshot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_players_steps_are_heard_once_and_in_order() {
        let mut remote = Remote::new(1);
        let step = |throttle: f32| Drive {
            throttle,
            ..default()
        };
        // Steps 1 to 3, then 2 to 5: what was heard before is not heard again.
        remote.hear(3, vec![step(1.0), step(2.0), step(3.0)]);
        remote.hear(5, vec![step(2.0), step(3.0), step(4.0), step(5.0)]);
        // Five waiting is one too many: the oldest goes.
        let waiting: Vec<(Tick, f32)> = remote
            .queue
            .iter()
            .map(|(tick, drive)| (*tick, drive.throttle))
            .collect();
        assert_eq!(waiting, [(2, 2.0), (3, 3.0), (4, 4.0), (5, 5.0)]);
        // A late one that has been overtaken is ignored.
        remote.hear(3, vec![step(9.0)]);
        assert_eq!(remote.queue.len(), 4);
    }

    #[test]
    fn a_button_pressed_in_a_dropped_step_is_still_pressed() {
        let mut remote = Remote::new(1);
        let mut drives = vec![Drive::default(); QUEUE + 1];
        (drives[0].use_item, drives[0].start_boost) = (true, Some(1));
        remote.hear(QUEUE as Tick + 1, drives);
        let (tick, first) = remote.queue[0];
        assert_eq!(
            (tick, first.use_item, first.start_boost),
            (2, true, Some(1))
        );
    }
}
