//! The room a session waits in between races, where its race is chosen. The port's own.
//!
//! Everyone in the room votes for the circuit of the next race, and the circuit most
//! voted for is raced, one of them drawn where several are level. Everything else
//! about the race is the host's to say: the laps, the port's own ways of racing, and
//! how many of the computer's cars make up the field. The vote closes when everyone
//! is ready, half a minute after more than half of them are, or when the host says
//! so: one player being ready hurries nobody.
//!
//! The host keeps the room and tells the others how it stands; these are the rules
//! it keeps it by.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::link::Quality;
use super::protocol::{HOST, Peer, Ride, Rules};
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
    /// What they have scored in the session's races so far.
    pub points: u32,
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
    /// What its place scored.
    pub points: u32,
}

/// The room, as the host has it or as a player was last told it is.
#[derive(Resource, Default)]
pub struct Room {
    pub voters: Vec<Voter>,
    /// Seconds until the vote closes, once most of the room is ready.
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
    /// The races run of a series and how many it is of, if one is being run; the
    /// host's own count of them is `raced`.
    pub series: Option<(u8, u8)>,
    pub raced: u8,
    /// The last things said in the room, oldest first.
    pub chat: Vec<String>,
    /// What the player here has asked for, and whether they are ready.
    pub ballot: Option<Rules>,
    pub ready: bool,
    /// The host wants the race begun now.
    pub begin: bool,
    /// Counts up whenever the room changes, for whatever draws it.
    pub revision: u32,
    /// What everyone in the room races as.
    pub rides: Vec<(Peer, Ride)>,
    /// What was last sent, so that it is sent again only when it changes.
    pub(super) told: Vec<u8>,
    pub(super) sent: Option<(Option<Rules>, bool)>,
    pub(super) rode: Option<Ride>,
}

/// Whether the vote is over.
pub fn closed(voters: &[Voter], closing: Option<f32>, begin: bool) -> bool {
    begin
        || (!voters.is_empty() && voters.iter().all(|voter| voter.ready))
        || closing.is_some_and(|left| left <= 0.0)
}

/// How the race is to be run: on the circuit most voted for, one of them drawn
/// where several are level, and otherwise as the host would have it. `fallback` is
/// what a player who has said nothing is taken to want.
pub fn decide(voters: &[Voter], fallback: &Rules, dice: &mut Rng) -> Rules {
    fn wished<'a>(voter: &'a Voter, fallback: &'a Rules) -> &'a Rules {
        voter.ballot.as_ref().unwrap_or(fallback)
    }
    let wish = |voter| wished(voter, fallback);
    let hosts = voters
        .iter()
        .find(|voter| voter.peer == HOST)
        .map_or(fallback, wish);
    let votes = |circuit: &str| voters.iter().filter(|v| wish(v).circuit == circuit).count();
    let most = voters.iter().map(|v| votes(&wish(v).circuit)).max();
    let mut level: Vec<&str> = Vec::new();
    for voter in voters {
        let circuit = wish(voter).circuit.as_str();
        if Some(votes(circuit)) == most && !level.contains(&circuit) {
            level.push(circuit);
        }
    }
    let circuit = match level.len() {
        0 => hosts.circuit.as_str(),
        count => level[(dice.f() * count as f32) as usize % count],
    };
    Rules {
        circuit: circuit.to_string(),
        ..hosts.clone()
    }
}

/// The most lines of what has been said that a room keeps, and the longest one said.
pub const CHAT_LINES: usize = 3;
pub const CHAT_LENGTH: usize = 36;

impl Room {
    /// Takes in something said.
    pub fn hear(&mut self, line: String) {
        self.chat.push(line);
        let extra = self.chat.len().saturating_sub(CHAT_LINES);
        self.chat.drain(..extra);
        self.revision += 1;
    }

    /// Gives each car of a race run the points its place scores, as the original's
    /// circuits score them, and the players theirs to keep. `who` is which of the
    /// room a result is, if any.
    pub fn score(&mut self, who: impl Fn(usize) -> Option<Peer>) {
        for (place, finish) in self.results.iter_mut().enumerate() {
            finish.points = crate::championship::POINTS.get(place).copied().unwrap_or(0);
            if let Some(voter) =
                who(place).and_then(|peer| self.voters.iter_mut().find(|voter| voter.peer == peer))
            {
                voter.points += finish.points;
            }
        }
        self.raced = self.raced.saturating_add(1);
    }

    /// The series is begun again: nobody has any points and no race of it is run.
    pub fn reset(&mut self) {
        for voter in &mut self.voters {
            voter.points = 0;
        }
        (self.raced, self.revision) = (0, self.revision + 1);
    }

    /// How many in the room have voted for a circuit, someone who has said nothing
    /// being taken to want the last race's again.
    pub fn votes(&self, circuit: &str) -> usize {
        self.voters
            .iter()
            .filter(|voter| {
                let wish = voter.ballot.as_ref().or(self.last.as_ref());
                wish.is_some_and(|rules| rules.circuit == circuit)
            })
            .count()
    }

    /// How the host would have the next race run, as far as the room has heard.
    pub fn hosts(&self) -> Option<&Rules> {
        let host = self.voters.iter().find(|voter| voter.peer == HOST)?;
        host.ballot.as_ref().or(self.last.as_ref())
    }

    /// Steps the vote on by `dt` seconds. The rules to race by, once it has closed.
    pub fn tick(&mut self, dt: f32, fallback: &Rules, dice: &mut Rng) -> Option<Rules> {
        // The clock runs while most of the room is ready, and not for one of many.
        let ready = self.voters.iter().filter(|voter| voter.ready).count();
        let most = ready * 2 > self.voters.len();
        self.closing = match (most, self.closing) {
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
            speed: 0,
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
            points: 0,
        }
    }

    #[test]
    fn the_circuit_most_voted_for_is_raced() {
        let voters = [
            voter(0, Some(rules("A", 1))),
            voter(1, Some(rules("B", 1))),
            voter(2, Some(rules("B", 1))),
        ];
        let mut dice = Rng(99);
        for _ in 0..50 {
            assert_eq!(decide(&voters, &rules("Z", 1), &mut dice).circuit, "B");
        }
        let room = Room {
            voters: voters.to_vec(),
            ..default()
        };
        assert_eq!(
            ["A", "B", "Z"].map(|circuit| room.votes(circuit)),
            [1, 2, 0]
        );
    }

    #[test]
    fn circuits_level_on_votes_are_drawn_between() {
        let voters = [
            voter(0, Some(rules("A", 1))),
            voter(1, Some(rules("B", 1))),
            voter(2, Some(rules("B", 1))),
            voter(3, Some(rules("C", 1))),
            voter(4, Some(rules("C", 1))),
        ];
        let mut dice = Rng(99);
        let mut drawn = std::collections::HashMap::new();
        for _ in 0..300 {
            *drawn
                .entry(decide(&voters, &rules("Z", 1), &mut dice).circuit)
                .or_insert(0) += 1;
        }
        // Each of the two that are level comes up, and the one behind them never.
        assert_eq!(drawn.len(), 2);
        assert!(drawn["B"] > 60 && drawn["C"] > 60, "{drawn:?}");
    }

    #[test]
    fn the_rest_is_as_the_host_would_have_it() {
        let mut dice = Rng(1);
        let with = |laps: u8, mirror: bool| {
            Some(Rules {
                mirror,
                ..rules("A", laps)
            })
        };
        // Two to one for three laps, mirrored, against the host: the host's it is.
        let voters = [
            voter(0, with(0, false)),
            voter(1, with(3, true)),
            voter(2, with(3, true)),
        ];
        let decided = decide(&voters, &rules("Z", 1), &mut dice);
        assert_eq!((decided.lap_choice, decided.mirror), (0, false));
    }

    #[test]
    fn a_player_who_says_nothing_wants_the_last_race_again() {
        let mut dice = Rng(1);
        let voters = [
            voter(0, Some(rules("A", 0))),
            voter(1, None),
            voter(2, None),
        ];
        // Two for the last race's circuit, by saying nothing, against one for another.
        let decided = decide(&voters, &rules("Z", 4), &mut dice);
        assert_eq!((decided.circuit.as_str(), decided.lap_choice), ("Z", 0));
    }

    #[test]
    fn the_vote_closes_when_all_are_ready_or_time_is_up_or_the_host_says() {
        let fallback = rules("Z", 1);
        let mut dice = Rng(5);
        let mut room = Room {
            voters: (0..3)
                .map(|peer| voter(peer, Some(rules("A", 1))))
                .collect(),
            ..default()
        };
        // Nobody ready: it stays open, and no clock runs.
        assert!(room.tick(1.0, &fallback, &mut dice).is_none());
        assert_eq!(room.closing, None);
        // One of three ready: still no clock, however long they wait.
        room.voters[1].ready = true;
        assert!(
            room.tick(CLOSES_AFTER * 2.0, &fallback, &mut dice)
                .is_none()
        );
        assert_eq!(room.closing, None);
        // Two of three: the clock starts, and runs out.
        room.voters[2].ready = true;
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
        // One of the two who were ready changes their mind: the clock stops.
        (room.voters[0].ready, room.voters[1].ready) = (true, true);
        room.tick(1.0, &fallback, &mut dice);
        assert!(room.closing.is_some());
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
