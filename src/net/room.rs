//! The room a session waits in between races, where its race is chosen. The port's own.
//!
//! Everyone in the room says how they would have the next race run: the circuit, the
//! laps, the port's own ways of racing, and how many of the computer's cars make up
//! the field. When the vote closes the circuit is drawn from the circuits voted for,
//! a ticket a player, so that the favourite is likeliest and nobody's pick is
//! impossible; everything else goes to whatever most asked for, the host's wish
//! settling a tie it is part of. The vote closes when everyone is ready, half a
//! minute after the first is, or when the host says so.
//!
//! The host keeps the room and tells the others how it stands; these are the rules
//! it keeps it by.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::link::Quality;
use super::protocol::{HOST, Peer, Rules};
use crate::meshgen::Rng;

/// How long the vote stays open once somebody is ready, in seconds.
pub const CLOSES_AFTER: f32 = 30.0;

/// Someone in the room, and what they have asked for.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Voter {
    pub peer: Peer,
    pub name: String,
    pub ready: bool,
    /// How they would have the race run, once they have said.
    pub ballot: Option<Rules>,
    /// How good their way to the host is; nothing for the host itself.
    pub link: Option<Quality>,
}

/// How a car's race went.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Finish {
    pub name: String,
    /// A player's car, and not one of the computer's.
    pub player: bool,
    /// When it crossed the line; nothing for a car put out or still out at the end.
    pub time: Option<f32>,
    /// Its quickest lap, if it finished one.
    pub best: Option<f32>,
}

/// The room, as the host has it or as a player was last told it is.
#[derive(Resource, Default)]
pub struct Room {
    pub voters: Vec<Voter>,
    /// Seconds until the vote closes, once somebody is ready.
    pub closing: Option<f32>,
    /// How the last race was run, which is what a player who says nothing is taken
    /// to want again.
    pub last: Option<Rules>,
    /// The cars of the last race, in the order they came in, and that the player
    /// here has yet to be shown them.
    pub results: Vec<Finish>,
    pub fresh: bool,
    /// A race is on, which those in the room are not in.
    pub racing: bool,
    /// What the player here has asked for, and whether they are ready.
    pub ballot: Option<Rules>,
    pub ready: bool,
    /// The host wants the race begun now.
    pub begin: bool,
    /// Counts up whenever the room changes, for whatever draws it.
    pub revision: u32,
    /// What was last sent, so that it is sent again only when it changes.
    pub(super) told: Vec<u8>,
    pub(super) sent: Option<(Option<Rules>, bool)>,
}

/// The option most asked for; among several asked for equally, the host's if it is
/// one of them and otherwise the first to have been asked for.
fn favourite<T: PartialEq + Clone>(wishes: &[T], hosts: &T) -> T {
    let count = |wish: &T| wishes.iter().filter(|other| *other == wish).count();
    let most = wishes.iter().map(count).max().unwrap_or(0);
    if count(hosts) == most {
        return hosts.clone();
    }
    wishes
        .iter()
        .find(|wish| count(wish) == most)
        .unwrap_or(hosts)
        .clone()
}

/// Whether the vote is over.
pub fn closed(voters: &[Voter], closing: Option<f32>, begin: bool) -> bool {
    begin
        || (!voters.is_empty() && voters.iter().all(|voter| voter.ready))
        || closing.is_some_and(|left| left <= 0.0)
}

/// How the race is to be run. `fallback` is what a player who has said nothing is
/// taken to want.
pub fn decide(voters: &[Voter], fallback: &Rules, dice: &mut Rng) -> Rules {
    let wishes: Vec<Rules> = voters
        .iter()
        .map(|voter| voter.ballot.clone().unwrap_or_else(|| fallback.clone()))
        .collect();
    let hosts = voters
        .iter()
        .position(|voter| voter.peer == HOST)
        .and_then(|at| wishes.get(at))
        .unwrap_or(fallback)
        .clone();
    let each =
        |of: fn(&Rules) -> u8| favourite(&wishes.iter().map(of).collect::<Vec<u8>>(), &of(&hosts));
    let flag = |of: fn(&Rules) -> bool| {
        favourite(&wishes.iter().map(of).collect::<Vec<bool>>(), &of(&hosts))
    };
    let drawn = if wishes.is_empty() {
        &hosts
    } else {
        &wishes[(dice.f() * wishes.len() as f32) as usize % wishes.len()]
    };
    Rules {
        circuit: drawn.circuit.clone(),
        lap_choice: each(|rules| rules.lap_choice),
        mirror: flag(|rules| rules.mirror),
        reverse: flag(|rules| rules.reverse),
        bricks: each(|rules| rules.bricks),
        elimination: flag(|rules| rules.elimination),
        opponents: each(|rules| rules.opponents),
        // How hard the computer's cars drive is the host's affair.
        difficulty: hosts.difficulty,
    }
}

impl Room {
    /// How many in the room want what the player here wants of one thing.
    pub fn agreeing<T: PartialEq>(&self, of: impl Fn(&Rules) -> T) -> (usize, usize) {
        let Some(mine) = self.ballot.as_ref().map(&of) else {
            return (0, self.voters.len());
        };
        let fallback = self.last.as_ref().or(self.ballot.as_ref());
        let agree = self
            .voters
            .iter()
            .filter(|voter| voter.ballot.as_ref().or(fallback).map(&of).as_ref() == Some(&mine))
            .count();
        (agree, self.voters.len())
    }

    /// Steps the vote on by `dt` seconds. The rules to race by, once it has closed.
    pub fn tick(&mut self, dt: f32, fallback: &Rules, dice: &mut Rng) -> Option<Rules> {
        let anyone = self.voters.iter().any(|voter| voter.ready);
        self.closing = match (anyone, self.closing) {
            (false, _) => None,
            (true, None) => Some(CLOSES_AFTER),
            (true, Some(left)) => Some(left - dt),
        };
        if !closed(&self.voters, self.closing, self.begin) {
            return None;
        }
        let rules = decide(&self.voters, self.last.as_ref().unwrap_or(fallback), dice);
        // The next vote begins with nobody ready.
        for voter in &mut self.voters {
            voter.ready = false;
        }
        (self.closing, self.begin, self.ready, self.last) =
            (None, false, false, Some(rules.clone()));
        self.revision += 1;
        Some(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(circuit: &str, laps: u8) -> Rules {
        Rules {
            circuit: circuit.into(),
            lap_choice: laps,
            mirror: false,
            reverse: false,
            bricks: 0,
            elimination: false,
            opponents: 2,
            difficulty: 1,
        }
    }

    fn voter(peer: Peer, ballot: Option<Rules>) -> Voter {
        Voter {
            peer,
            name: format!("P{peer}"),
            ready: false,
            ballot,
            link: None,
        }
    }

    #[test]
    fn the_circuit_drawn_is_one_that_was_voted_for() {
        let voters = [
            voter(0, Some(rules("A", 1))),
            voter(1, Some(rules("B", 1))),
            voter(2, Some(rules("B", 1))),
        ];
        let mut dice = Rng(99);
        let mut drawn = std::collections::HashMap::new();
        for _ in 0..300 {
            *drawn
                .entry(decide(&voters, &rules("Z", 1), &mut dice).circuit)
                .or_insert(0) += 1;
        }
        // Both that were asked for come up, the one asked for twice more often, and
        // nothing else does.
        assert_eq!(drawn.len(), 2);
        assert!(drawn["B"] > drawn["A"] && drawn["A"] > 30, "{drawn:?}");
    }

    #[test]
    fn the_rest_goes_to_the_most_asked_for_and_the_host_settles_a_tie() {
        let mut dice = Rng(1);
        let with = |laps: u8, mirror: bool| {
            Some(Rules {
                mirror,
                ..rules("A", laps)
            })
        };
        // Two to one for three laps, mirrored, against the host.
        let voters = [
            voter(0, with(0, false)),
            voter(1, with(3, true)),
            voter(2, with(3, true)),
        ];
        let decided = decide(&voters, &rules("Z", 1), &mut dice);
        assert_eq!((decided.lap_choice, decided.mirror), (3, true));
        // One each: the host's wish.
        let voters = [voter(0, with(0, false)), voter(1, with(3, true))];
        let decided = decide(&voters, &rules("Z", 1), &mut dice);
        assert_eq!((decided.lap_choice, decided.mirror), (0, false));
        // A tie the host is no part of goes to the wish made first.
        let voters = [
            voter(0, with(0, false)),
            voter(1, with(3, true)),
            voter(2, with(3, true)),
            voter(3, with(5, true)),
            voter(4, with(5, true)),
        ];
        assert_eq!(decide(&voters, &rules("Z", 1), &mut dice).lap_choice, 3);
    }

    #[test]
    fn a_player_who_says_nothing_wants_the_last_race_again() {
        let mut dice = Rng(1);
        let voters = [
            voter(0, Some(rules("A", 0))),
            voter(1, None),
            voter(2, None),
        ];
        let decided = decide(&voters, &rules("Z", 4), &mut dice);
        assert_eq!(decided.lap_choice, 4);
    }

    #[test]
    fn the_vote_closes_when_all_are_ready_or_time_is_up_or_the_host_says() {
        let fallback = rules("Z", 1);
        let mut dice = Rng(5);
        let mut room = Room {
            voters: vec![voter(0, Some(rules("A", 1))), voter(1, Some(rules("A", 1)))],
            ..default()
        };
        // Nobody ready: it stays open, and no clock runs.
        assert!(room.tick(1.0, &fallback, &mut dice).is_none());
        assert_eq!(room.closing, None);
        // One ready: the clock starts, and runs out.
        room.voters[1].ready = true;
        assert!(room.tick(1.0, &fallback, &mut dice).is_none());
        assert_eq!(room.closing, Some(CLOSES_AFTER));
        assert!(
            room.tick(CLOSES_AFTER - 1.0, &fallback, &mut dice)
                .is_none()
        );
        let decided = room.tick(1.5, &fallback, &mut dice).expect("time is up");
        assert_eq!(decided.circuit, "A");
        // The next vote starts afresh, remembering how the last race was run.
        assert!(room.voters.iter().all(|voter| !voter.ready) && room.closing.is_none());
        assert_eq!(room.last.as_ref(), Some(&decided));
        // The one who was ready changes their mind: the clock stops.
        room.voters[0].ready = true;
        room.tick(1.0, &fallback, &mut dice);
        room.voters[0].ready = false;
        room.tick(1.0, &fallback, &mut dice);
        assert_eq!(room.closing, None);
        // Everyone ready closes it at once, and so does the host's saying so.
        room.voters.iter_mut().for_each(|voter| voter.ready = true);
        assert!(room.tick(0.0, &fallback, &mut dice).is_some());
        room.begin = true;
        assert!(room.tick(0.0, &fallback, &mut dice).is_some());
        assert!(!room.begin);
    }
}
