//! The host's side of a race online. The port's own.
//!
//! The host runs the race as it would alone: every car, the computer's and the other
//! players' alike, is stepped here, and what happens here is what happened. The other
//! players' cars are driven by what their games say they are pressing, a step at a
//! time, and thirty times a second each player is told how everything stands.

use std::collections::VecDeque;

use bevy::prelude::*;

use super::protocol::{
    DATAGRAM, Drive, Peer, SNAPSHOT_EVERY, Snapshot, Stage, Tick, ToPlayer, encode,
};
use super::room::Finish;
use super::state::{Sight, State};
use super::{Clock, Inbox, Session, Wire};
use crate::kart::{Controls, Kart, Player};
use crate::menu::Screen;
use crate::{Phase, Race};

/// How many of a player's steps the host has in hand to begin with, the next among
/// them: enough that one a little later than the last is there when it is wanted.
const DEPTH: Tick = 2;
/// How many steps of the race those in hand are watched over before more or fewer
/// are kept: a second.
const WINDOW: u32 = 60;
/// A run of a player's steps that didn't come in time no longer than this is the
/// connection being uneven, which one more in hand would have covered; a longer one
/// is the connection or the player's game stopping for a moment, which no number
/// in hand would have.
const SHORT: u32 = 2;
/// How many times running the newest of a player's steps may come too late to be
/// driven before they are taken in hand again.
const LATE: u32 = 6;
/// A player whose steps have stopped coming is driven as they last pressed for this
/// many steps, and by the computer from then until one comes: half a second, which
/// is more than a poor connection loses at once and less than a car takes to be
/// driven off the road.
const STALLED: u32 = 30;
/// How long the start waits for a player's game to load the circuit.
const LOAD_WAIT: f32 = 20.0;
/// How long the cars still out are given once the first player is home, as the
/// original gives the field (`RaceSession::UpdateFinishedState`), and how long the
/// result is then looked at.
const FINISH_WAIT: f32 = 10.0;
const RESULT_WAIT: f32 = 5.0;

/// A car driven by a player somewhere else.
///
/// It is driven by one of the player's steps for each of the host's, so that it is
/// where the player's game, which drove it by the same steps, had it. A step that
/// hasn't come when it is wanted is driven as the one before was, and given up when
/// it comes: waited for, the car would be a step behind the player's game for the
/// rest of the race, and the player's car put back by a step's travel when it was
/// found out. A few of the player's steps are kept in hand so that that is seldom,
/// and how many is changed only when the connection has (steps come too late time
/// after time, or a little late now and then, or more are waiting than are ever
/// needed), which is the one time it puts the player's car right.
#[derive(Component)]
pub struct Remote {
    pub peer: Peer,
    /// The steps heard of and not yet driven, oldest first.
    queue: VecDeque<(Tick, Drive)>,
    newest: Option<Tick>,
    /// A step newer than any before has been heard since the car was last driven.
    fresh: bool,
    /// The last of the player's steps the car has been driven by, or in the place of;
    /// none until one has been heard.
    used: Option<Tick>,
    /// Steps of the race the car is to be driven as before without taking one of the
    /// player's, so that more of them are in hand.
    hold: Tick,
    /// The buttons pressed in steps that were never driven, for the next that is.
    owed: (bool, Option<u8>),
    /// How many steps running have had none of the player's to drive it by.
    starved: u32,
    /// How many of the player's steps running didn't come in time, and how many times
    /// running the newest heard came too late to be driven.
    missed: u32,
    late: u32,
    /// What has been seen of the steps in hand over the steps of the race counted so
    /// far: the fewest there were, and how many short runs didn't come in time.
    counted: u32,
    fewest: i64,
    short: u32,
}

/// What a player's car is driven by for a step of the race.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Next {
    /// Nothing has been heard of the player: the car is left as it is.
    Nothing,
    /// The player's next step.
    Step(Drive),
    /// The step before again, with no button newly pressed: the player's next step
    /// hasn't come, and the car is driven in its place.
    Missing,
    /// The step before again, while the player's are taken in hand.
    Held,
}

impl Remote {
    pub fn new(peer: Peer) -> Self {
        Remote {
            peer,
            queue: VecDeque::new(),
            newest: None,
            fresh: false,
            used: None,
            hold: 0,
            owed: (false, None),
            starved: 0,
            missed: 0,
            late: 0,
            counted: 0,
            fewest: i64::MAX,
            short: 0,
        }
    }

    /// Nothing has been heard of the player's pressing for too long, and the car
    /// is the computer's to drive until something is.
    pub fn stalled(&self) -> bool {
        self.starved >= STALLED
    }

    /// The last of the player's steps the car has been driven by, or in the place of.
    pub fn used(&self) -> Tick {
        self.used.unwrap_or(0)
    }

    /// A button pressed in a step that won't be driven is pressed in the next that is.
    fn owe(&mut self, drive: &Drive) {
        self.owed.0 |= drive.use_item;
        self.owed.1 = self.owed.1.or(drive.start_boost);
    }

    /// Takes in what a player's game says it pressed, less what was heard before.
    fn hear(&mut self, last: Tick, drives: Vec<Drive>) {
        let count = drives.len() as Tick;
        for (i, drive) in drives.into_iter().enumerate() {
            let Some(tick) = (last + 1 + i as Tick).checked_sub(count) else {
                continue;
            };
            if self.newest.is_some_and(|newest| tick <= newest) {
                continue;
            }
            (self.newest, self.fresh) = (Some(tick), true);
            // Too late: the car has been driven in its place.
            if self.used.is_some_and(|used| tick <= used) {
                self.owe(&drive);
            } else {
                self.queue.push_back((tick, drive));
            }
        }
    }

    /// What the car is to be driven by for the next step of the race.
    fn next(&mut self) -> Next {
        let Some(newest) = self.newest else {
            return Next::Nothing;
        };
        let used = match self.used {
            Some(used) => used,
            None => {
                // Driving begins with the first heard and the one before it in hand,
                // or, the first being the player's first, after a step's wait.
                let used = newest.saturating_sub(DEPTH);
                (self.used, self.hold) = (Some(used), DEPTH - (newest - used));
                used
            }
        };
        self.watch(newest, newest as i64 - used as i64);
        if self.hold > 0 {
            self.hold -= 1;
            return Next::Held;
        }
        let next = self.used() + 1;
        self.used = Some(next);
        // Steps passed over are not driven, but what was pressed in them was.
        while let Some(&(tick, drive)) = self.queue.front()
            && tick < next
        {
            self.queue.pop_front();
            self.owe(&drive);
        }
        match self.queue.front() {
            Some(&(tick, mut drive)) if tick == next => {
                self.queue.pop_front();
                drive.use_item |= self.owed.0;
                drive.start_boost = drive.start_boost.or(self.owed.1);
                self.owed = (false, None);
                if (1..=SHORT).contains(&self.missed) {
                    self.short += 1;
                }
                self.missed = 0;
                Next::Step(drive)
            }
            _ => {
                self.missed += 1;
                Next::Missing
            }
        }
    }

    /// Keeps more or fewer of the player's steps in hand, `spare` being how many there
    /// are now, the next among them.
    fn watch(&mut self, newest: Tick, spare: i64) {
        let fresh = std::mem::take(&mut self.fresh);
        if spare >= 1 {
            self.late = 0;
        } else if fresh {
            self.late += 1;
        }
        if self.late >= LATE {
            // Steps keep coming, and too late: the player's game is behind (it stopped
            // for longer than it could make up). The car is driven as before while
            // they are taken in hand from the newest on.
            debug!(
                "player {}: steps {} late, taken in hand again",
                self.peer,
                -spare + 1
            );
            (self.used, self.hold, self.late) = (Some(newest), DEPTH, 0);
            (self.counted, self.fewest, self.short) = (0, i64::MAX, 0);
            return;
        }
        self.fewest = self.fewest.min(spare);
        self.counted += 1;
        if self.counted < WINDOW {
            return;
        }
        if self.short >= 2 {
            // Steps come a little late now and then: one more is kept in hand.
            debug!(
                "player {}: steps a little late {} times, one more kept in hand",
                self.peer, self.short
            );
            self.hold += 1;
        } else if self.fewest > DEPTH as i64 {
            // More have been waiting all along than were ever wanted: the car is
            // driven on past those it doesn't need.
            let past = (self.fewest - DEPTH as i64) as Tick;
            debug!(
                "player {}: {past} steps more in hand than wanted, driven past",
                self.peer
            );
            self.used = Some(self.used() + past);
        }
        (self.counted, self.fewest, self.short) = (0, i64::MAX, 0);
    }
}

/// How the race is getting on, for `flow`.
#[derive(Resource, Default)]
pub struct Flow {
    /// How long the start has waited for the players' games.
    waited: f32,
    /// How long the result has been looked at.
    shown: f32,
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
/// that hasn't come is driven as the last was, with no button newly pressed; when
/// none has come for a while (`Remote::stalled`) the computer drives, as
/// `kart::ai_drive` has just set the car to be.
pub fn drive_remotes(race: Res<Race>, mut remotes: Query<(&Kart, &mut Remote, &mut Controls)>) {
    for (kart, mut remote, mut c) in &mut remotes {
        let next = remote.next();
        match next {
            Next::Step(_) => remote.starved = 0,
            Next::Held => {}
            // Nobody is waited for to press anything before the race is on.
            _ if race.phase == Phase::Racing => remote.starved += 1,
            _ => {}
        }
        // Its race run, the car is the computer's to drive.
        if kart.finished.is_some() || kart.out.is_some() {
            continue;
        }
        match next {
            Next::Step(drive) => *c = drive.controls(),
            _ if remote.stalled() => {}
            _ => (c.use_item, c.start_boost) = (false, None),
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
    // Every car's laps are timed, as the display times the player's for a game
    // alone: whoever follows a car is shown its times, and the results have its best.
    for (mut k, ..) in &mut karts {
        let now = k.finished.unwrap_or(race.time);
        if k.lap > k.laps.lap {
            if k.laps.lap >= 1 {
                let taken = now - k.laps.began;
                k.laps.last = Some(taken);
                k.laps.best = Some(k.laps.best.map_or(taken, |best| best.min(taken)));
            }
            (k.laps.lap, k.laps.began) = (k.lap, now);
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
                let best = k.laps.best;
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
    mark: Res<super::modes::Mark>,
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
    let all: Vec<Sight> = karts.iter().map(|(k, _)| Sight::of(k)).collect();
    for (kart, remote) in &karts {
        let Some(remote) = remote else { continue };
        let snapshot = Snapshot {
            tick: clock.tick,
            used: remote.used(),
            stage,
            clocks: [race.intro, race.countdown, race.time],
            own: Some(State::of(kart)),
            karts: all.clone(),
            mark: mark.0,
        };
        let snapshot = encode(&snapshot);
        debug_assert!(
            snapshot.len() <= DATAGRAM,
            "a snapshot of {}",
            snapshot.len()
        );
        wire.0.datagram(remote.peer, snapshot);
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
            mark: mark.0,
        };
        wire.0.datagram(peer, encode(&snapshot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A step whose pedal is down by its own number, to tell it by.
    fn step(tick: Tick) -> Drive {
        Drive {
            throttle: tick as f32,
            ..default()
        }
    }

    /// Hears a player's step `tick`, with the three before it, as their game sends it.
    fn hear(remote: &mut Remote, tick: Tick) {
        remote.hear(
            tick,
            (tick.saturating_sub(3).max(1)..=tick).map(step).collect(),
        );
    }

    /// Which of the player's steps the car is driven by for a step of the race, if any.
    fn driven(remote: &mut Remote) -> Option<Tick> {
        match remote.next() {
            Next::Step(drive) => Some(drive.throttle as Tick),
            _ => None,
        }
    }

    #[test]
    fn a_players_steps_are_heard_once_and_in_order() {
        let mut remote = Remote::new(1);
        // Steps 1 to 3, then 2 to 5: what was heard before is not heard again.
        remote.hear(3, vec![step(1), step(2), step(3)]);
        remote.hear(5, vec![step(2), step(3), step(4), step(5)]);
        let waiting: Vec<Tick> = remote.queue.iter().map(|(tick, _)| *tick).collect();
        assert_eq!(waiting, [1, 2, 3, 4, 5]);
        // A late one that has been overtaken is ignored.
        remote.hear(3, vec![step(9)]);
        assert_eq!(remote.queue.len(), 5);
    }

    /// The car is driven by one of the player's steps for each of the host's. One
    /// held up on its way is driven in the place of, and those after it in their
    /// turn: the car is never a step behind the player's game, nor ahead of it.
    #[test]
    fn a_step_held_up_is_driven_in_the_place_of() {
        let mut remote = Remote::new(1);
        hear(&mut remote, 4);
        // Two in hand, the next among them.
        assert_eq!(driven(&mut remote), Some(3));
        hear(&mut remote, 5);
        assert_eq!(driven(&mut remote), Some(4));
        assert_eq!(driven(&mut remote), Some(5));
        // Steps 6 to 9 are held up, and come together.
        assert_eq!(driven(&mut remote), None);
        assert_eq!(driven(&mut remote), None);
        assert_eq!(remote.used(), 7);
        hear(&mut remote, 9);
        assert_eq!(driven(&mut remote), Some(8));
        hear(&mut remote, 10);
        assert_eq!(driven(&mut remote), Some(9));
    }

    #[test]
    fn a_button_pressed_in_a_step_given_up_is_still_pressed() {
        let mut remote = Remote::new(1);
        hear(&mut remote, 3);
        assert_eq!(driven(&mut remote), Some(2));
        assert_eq!(driven(&mut remote), Some(3));
        assert_eq!(driven(&mut remote), None);
        let mut late = vec![step(4), step(5)];
        (late[0].use_item, late[0].start_boost) = (true, Some(1));
        remote.hear(5, late);
        let Next::Step(drive) = remote.next() else {
            panic!("step 5 should have been driven");
        };
        assert_eq!(
            (drive.throttle, drive.use_item, drive.start_boost),
            (5.0, true, Some(1))
        );
    }

    /// Two little gaps in a second, each of which leaves a step to be driven in the
    /// place of, and one more step is kept in hand: a third like them leaves none.
    #[test]
    fn steps_a_little_late_now_and_then_have_one_more_kept_in_hand() {
        let mut remote = Remote::new(1);
        let (mut heard, mut missed) = (0, Vec::new());
        for at in 0..WINDOW + 40 {
            // At each gap two steps' pressing is a step late, and comes with the next.
            let gap = [10, 30, WINDOW + 20]
                .iter()
                .find(|&&gap| (gap..gap + 3).contains(&at));
            let coming = match gap {
                Some(&gap) if at < gap + 2 => 0,
                Some(_) => 3,
                None => 1,
            };
            for _ in 0..coming {
                heard += 1;
                hear(&mut remote, heard);
            }
            if remote.next() == Next::Missing {
                missed.push(at);
            }
        }
        assert_eq!(missed, [11, 31]);
    }

    /// A player's game that stopped for longer than it could make up has its steps
    /// all come too late, until they are taken in hand again.
    #[test]
    fn steps_that_keep_coming_too_late_are_taken_in_hand_again() {
        let mut remote = Remote::new(1);
        hear(&mut remote, 4);
        for _ in 0..20 {
            driven(&mut remote);
        }
        let mut by: Vec<(Tick, Option<Tick>)> = Vec::new();
        for heard in 5..5 + LATE + DEPTH + 3 {
            hear(&mut remote, heard);
            by.push((heard, driven(&mut remote)));
        }
        // Given up until one has come too late too many times, then held back while
        // as many are taken in hand as at first.
        let (given_up, again) = by.split_at((LATE + DEPTH - 1) as usize);
        assert!(
            given_up.iter().all(|(_, driven)| driven.is_none()),
            "{by:?}"
        );
        assert!(
            again
                .iter()
                .all(|(heard, driven)| *driven == Some(heard - 1)),
            "{by:?}"
        );
    }

    /// A player's game that got further ahead of the host than it needs to be, and
    /// stayed there, has the steps it needn't have in hand driven past.
    #[test]
    fn steps_always_waiting_in_numbers_are_driven_past() {
        let mut remote = Remote::new(1);
        let mut heard = 0;
        for at in 0..2 * WINDOW {
            for _ in 0..if at < 8 { 2 } else { 1 } {
                heard += 1;
                hear(&mut remote, heard);
            }
            driven(&mut remote);
        }
        assert_eq!(heard - remote.used(), DEPTH - 1);
    }
}
