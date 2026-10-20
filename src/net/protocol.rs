//! What a host and its players say to each other. The port's own.
//!
//! Two kinds of message. Those that must arrive, and in order, go as `ToHost` and
//! `ToPlayer`: joining, who sits where, the race beginning and ending. Those that are
//! out of date a moment later go as `Inputs` and `Snapshot`, sixty and thirty times a
//! second, and a lost one is simply made up for by the next.

use crate::assets::lrs::{self, Racer};
use bevy::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::room::{Finish, Voter};
use super::scene::{EventNote, Scene};
use super::state::{Sight, State};
use crate::kart::Controls;
use crate::menu::{Circuits, LAP_CHOICES, Mode, Settings};

/// A step of the race. Host and players each count their own.
pub type Tick = u32;
/// Who a message is from or for, as the host numbers them. The host is `HOST`.
pub type Peer = u32;
pub const HOST: Peer = 0;

/// The race is stepped this many times a second online, on every machine alike.
pub const TICKS: f64 = 60.0;
/// The host says how things stand every so many steps.
pub const SNAPSHOT_EVERY: Tick = 2;
/// Each `Inputs` carries this many steps' pressing, so that one lost costs nothing.
pub const RESENT: usize = 4;
/// The most a datagram may be, in bytes: what the real link is tried with, and under
/// what it carries. One bigger is not sent at all, so a `Snapshot` is kept inside it.
pub const DATAGRAM: usize = 1000;

#[derive(thiserror::Error, Debug)]
pub enum WireError {
    #[error("a message that could not be read: {0}")]
    Unreadable(#[from] postcard::Error),
}

pub fn encode<T: Serialize>(message: &T) -> Vec<u8> {
    // Nothing sent has a map with keys that can't be written or a length not known.
    postcard::to_stdvec(message).expect("a message that can be written")
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, WireError> {
    Ok(postcard::from_bytes(bytes)?)
}

/// What a player is pressing for one step: `Controls`, less what only the computer's
/// drivers set.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Drive {
    pub throttle: f32,
    pub steer: f32,
    pub drift: bool,
    pub tight: bool,
    pub use_item: bool,
    pub direct: bool,
    pub start_boost: Option<u8>,
}

impl Drive {
    pub fn of(c: &Controls) -> Self {
        Drive {
            throttle: c.throttle,
            steer: c.steer,
            drift: c.drift,
            tight: c.tight,
            use_item: c.use_item,
            direct: c.direct,
            start_boost: c.start_boost,
        }
    }

    pub fn controls(self) -> Controls {
        Controls {
            // A player is trusted, but a pedal still only goes down so far.
            throttle: self.throttle.clamp(-1.0, 1.0),
            steer: self.steer.clamp(-1.0, 1.0),
            drift: self.drift,
            tight: self.tight,
            use_item: self.use_item,
            start_boost: self.start_boost.map(|level| level.min(1)),
            course: None,
            direct: self.direct,
            hand: true,
        }
    }
}

/// A player's pressing, for the steps up to and including `last`, oldest first.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Inputs {
    pub last: Tick,
    pub drives: Vec<Drive>,
}

/// How far the race has got, for everyone. Whether it is over for a player is that
/// player's own car's affair.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    Intro,
    Countdown,
    Racing,
}

/// How things stand on the host at its step `tick`, for one player.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub tick: Tick,
    /// The last of the player's steps the host has driven their car by.
    pub used: Tick,
    pub stage: Stage,
    /// What is left of the intro and of the countdown, and how long the race has run.
    pub clocks: [f32; 3],
    /// The player's own car, in full; nothing for someone only watching.
    pub own: Option<State>,
    /// Every car, the player's too.
    pub karts: Vec<Sight>,
    /// Where the thing a game is played for lies, when it is nobody's: the flag
    /// of keep-away, knocked out of the hands of whoever had it (`modes::Mark`).
    pub mark: Option<Vec3>,
}

/// How a race is to be run: what the host's `Settings` say of it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rules {
    /// The circuit's race folder, or the key of one of the port's own maps.
    pub circuit: String,
    pub lap_choice: u8,
    pub mirror: bool,
    pub reverse: bool,
    pub bricks: u8,
    /// Which of `Mode::ALL` the race is.
    pub mode: u8,
    /// Which of `menu::SPEEDS` every car goes at.
    pub speed: u8,
    pub opponents: u8,
    pub difficulty: u8,
    /// A battle's lives, and which of `menu::BATTLE_TIMES` and `menu::SPARES` it is
    /// fought by; which of `menu::TAG_LEASTS` and `menu::TAG_BACKS` a game of tag is.
    pub lives: u8,
    pub battle_time: u8,
    pub spared: u8,
    pub tag_least: u8,
    pub tag_back: u8,
    /// Which of `menu::GOALS` wins keep-away and king of the hill.
    pub goal: u8,
    /// Whether a game laps don't end is played by teams, and whether a car
    /// knocked out of a battle comes back to it.
    pub teams: bool,
    pub returns: bool,
}

/// What a circuit is known by between games: its folder in the game's data, or
/// for one of the port's own its name.
pub fn key(circuit: &crate::menu::Circuit) -> &str {
    circuit.key()
}

impl Rules {
    pub fn of(settings: &Settings, circuits: &Circuits) -> Self {
        Rules {
            circuit: circuits
                .0
                .get(settings.circuit)
                .map(key)
                .unwrap_or_default()
                .to_string(),
            lap_choice: settings.lap_choice as u8,
            mirror: settings.mirror,
            reverse: settings.reverse,
            bricks: settings.bricks as u8,
            // A host who races alone by elimination is taken to want it here too.
            mode: match settings.mode {
                Mode::Race if settings.elimination => Mode::Elimination,
                mode => mode,
            } as u8,
            speed: settings.speed as u8,
            opponents: settings.opponents as u8,
            difficulty: settings.difficulty as u8,
            lives: settings.lives as u8,
            battle_time: settings.battle_time as u8,
            spared: settings.spared as u8,
            tag_least: settings.tag_least as u8,
            tag_back: settings.tag_back as u8,
            goal: settings.goal as u8,
            teams: settings.teams,
            returns: settings.returns,
        }
    }

    /// The circuit's map as this game has it, if it is one of the port's own: what
    /// a host sends its players with the start.
    pub fn map(&self, circuits: &Circuits) -> Option<String> {
        let circuit = circuits.0.iter().find(|c| key(c) == self.circuit)?;
        circuit.map.as_ref().map(|map| map.text.clone())
    }

    /// Whether a race run by these is played by two teams (`modes::team`).
    pub fn sided(&self) -> bool {
        self.teams && self.mode().lapless() && self.mode() != Mode::Infection
    }

    pub fn mode(&self) -> Mode {
        Mode::ALL[(self.mode as usize).min(Mode::ALL.len() - 1)]
    }

    /// These, on a circuit they can be raced on: an arena is for a battle or a game
    /// of tag and a circuit for the rest, so a race voted onto an arena is run on
    /// the first circuit there is and a battle voted onto a circuit in the first
    /// arena, as is one voted onto a map that only the player who voted for it has.
    /// With no arena to be had, a battle stays on the circuit voted for.
    pub fn fitted(mut self, circuits: &Circuits) -> Self {
        let fought = self.mode().lapless();
        let fits = |circuit: &&crate::menu::Circuit| circuit.arena() == fought;
        let voted = circuits.0.iter().find(|c| key(c) == self.circuit);
        if !voted.is_some_and(|c| fits(&c)) {
            let first = circuits.0.iter().find(fits);
            let run = first.or(voted.filter(|_| fought)).or(circuits.0.first());
            self.circuit = run.map(key).unwrap_or_default().to_string();
        }
        self
    }

    /// Sets a game up to race by these. False if it hasn't the circuit.
    pub fn apply(&self, settings: &mut Settings, circuits: &Circuits) -> bool {
        let Some(circuit) = circuits.0.iter().position(|c| key(c) == self.circuit) else {
            return false;
        };
        settings.circuit = circuit;
        self.set(settings);
        true
    }

    /// What `apply` sets but the circuit.
    fn set(&self, settings: &mut Settings) {
        settings.lap_choice = (self.lap_choice as usize).min(LAP_CHOICES.len() - 1);
        (settings.championship, settings.time_race) = (None, false);
        (settings.mirror, settings.reverse, settings.elimination) =
            (self.mirror, self.reverse, false);
        settings.mode = self.mode();
        settings.bricks = (self.bricks as usize).min(crate::menu::BRICK_RULES.len() - 1);
        settings.speed = (self.speed as usize).min(crate::menu::SPEEDS.len() - 1);
        settings.opponents = (self.opponents as usize).min(crate::menu::MAX_OPPONENTS);
        settings.difficulty = (self.difficulty as usize).min(crate::menu::DIFFICULTIES.len() - 1);
        settings.lives = (self.lives as usize).clamp(1, crate::menu::MAX_LIVES);
        settings.battle_time = (self.battle_time as usize).min(crate::menu::BATTLE_TIMES.len() - 1);
        settings.spared = (self.spared as usize).min(crate::menu::SPARES.len() - 1);
        settings.tag_least = (self.tag_least as usize).min(crate::menu::TAG_LEASTS.len() - 1);
        settings.tag_back = (self.tag_back as usize).min(crate::menu::TAG_BACKS.len() - 1);
        settings.goal = (self.goal as usize).min(crate::menu::GOALS.len() - 1);
        (settings.teams, settings.returns) = (self.teams, self.returns);
    }

    /// Puts back over `settings` what `apply` and a race's start set, as `own` has
    /// it: everything else of them is the player's own, changed in a session or not.
    pub fn put_back(settings: &mut Settings, own: &Settings) {
        settings.circuit = own.circuit;
        settings.lap_choice = own.lap_choice;
        (settings.championship, settings.time_race) = (own.championship.clone(), own.time_race);
        (settings.mirror, settings.reverse, settings.elimination) =
            (own.mirror, own.reverse, own.elimination);
        settings.mode = own.mode;
        settings.bricks = own.bricks;
        settings.speed = own.speed;
        settings.opponents = own.opponents;
        settings.difficulty = own.difficulty;
        settings.lives = own.lives;
        settings.battle_time = own.battle_time;
        settings.spared = own.spared;
        settings.tag_least = own.tag_least;
        settings.tag_back = own.tag_back;
        settings.goal = own.goal;
        (settings.teams, settings.returns) = (own.teams, own.returns);
        (settings.rivals, settings.sides) = (own.rivals, own.sides);
    }

    /// A host's rules for its session, kept as its own for races alone: all of
    /// them but how a race is won, which alone is a race, by elimination or not.
    pub fn keep(&self, own: &mut Settings) {
        let (mode, championship, time_race) = (own.mode, own.championship.take(), own.time_race);
        self.set(own);
        (own.mode, own.championship, own.time_race) = (mode, championship, time_race);
        own.elimination = self.mode() == Mode::Elimination;
    }
}

/// A circuit a host has, for its players to vote for: what it is known by, what
/// it is called, and for one of the port's own maps whether it is an arena.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Listed {
    pub key: String,
    pub name: String,
    pub author: String,
    pub map: Option<bool>,
}

impl Listed {
    pub fn all(circuits: &Circuits) -> Vec<Listed> {
        let listed = |c: &crate::menu::Circuit| Listed {
            key: key(c).to_string(),
            name: c.name.clone(),
            author: c.author.clone(),
            map: c.map.as_ref().map(|map| map.arena()),
        };
        circuits.0.iter().map(listed).collect()
    }
}

/// What a player races as.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum Ride {
    /// Whoever the circuit puts in their grid slot.
    #[default]
    Slot,
    /// One of the game's drivers, by its code for them (`roster::NAMES`).
    Driver(String),
    /// A racer they built, which every game in the session makes the car of afresh.
    Built(Racer),
}

impl Ride {
    /// The ride as a host will have it from someone else's game: nothing of it
    /// longer than the game itself would make it.
    pub fn checked(self) -> Ride {
        match self {
            Ride::Slot => Ride::Slot,
            Ride::Driver(code) => Ride::Driver(code.chars().take(8).collect()),
            Ride::Built(mut racer) => {
                racer.name = racer.name.chars().take(lrs::NAME_LENGTH).collect();
                racer.chassis = racer.chassis.chars().take(8).collect();
                racer.car.truncate(2 + crate::build::MOST * 8);
                Ride::Built(racer)
            }
        }
    }
}

/// A place on the grid and who has it: a player, or one of the computer's drivers.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Seat {
    pub slot: u8,
    pub peer: Option<Peer>,
    pub name: String,
    /// What a player races as.
    pub car: Ride,
    /// Which of the two teams the car is of, where the game is played by teams.
    pub side: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Refusal {
    /// The two games don't speak the same `lobby_api::PROTOCOL`.
    Version,
    Password,
    Full,
    /// The host has put the player out of the session.
    Removed,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToHost {
    Hello {
        protocol: u32,
        name: String,
        password: String,
        car: Ride,
        /// What the host gave them when they were here before, if they were: by
        /// it they are known for who they were, whatever they are called now.
        back: Option<u64>,
    },
    /// The race asked for is loaded and its cars are on the grid.
    Loaded,
    /// How the player would have the next race run, and whether they are ready for it.
    Vote(Rules),
    Ready(bool),
    /// The player has given the race up and is back in the room; their car is the
    /// computer's to drive.
    Back,
    /// The player wants into the race that is on: to drive their car if they have
    /// one in it, and otherwise to watch.
    Enter,
    /// Something said to the room.
    Say(String),
    /// What the player races as from now on, changed in the room.
    Ride(Ride),
    /// The team the player would be of, or that they don't mind.
    Side(Option<u8>),
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ToPlayer {
    Welcome {
        you: Peer,
        /// What to say on coming back, should the way here break (`ToHost::Hello`).
        token: u64,
        /// What the session is called, for someone who dialled it by its address.
        title: String,
    },
    Refused(Refusal),
    Start {
        rules: Rules,
        seats: Vec<Seat>,
        /// The circuit's map as the host has it, if it is one of the port's own:
        /// what is raced, whatever the player's game has of that name.
        map: Option<String>,
    },
    /// How the room stands: who is in it and what they want, the seconds until the
    /// vote closes if a clock is running, whether a race is on, how the last race
    /// was run and who came where in it.
    Room {
        voters: Vec<Voter>,
        closing: Option<f32>,
        racing: bool,
        /// The races run of a series and how many it is of, if one is being run.
        series: Option<(u8, u8)>,
        last: Option<Rules>,
        results: Vec<Finish>,
    },
    /// The race is run, and everyone is back in the room.
    Over,
    /// What has become of the bricks and the power-ups, and the circuit's events
    /// that have started and ended (`scene`).
    Scene(Scene),
    Events(Vec<EventNote>),
    /// What someone in the room has said, their name before it.
    Said(String),
    /// What everyone in the room races as, told when any of it changes.
    Rides(Vec<(Peer, Ride)>),
    /// The circuits the host has, which are the ones there are to vote for.
    Circuits(Vec<Listed>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_come_back_as_they_went() {
        let inputs = Inputs {
            last: 77,
            drives: vec![
                Drive {
                    throttle: 1.0,
                    steer: -0.5,
                    use_item: true,
                    start_boost: Some(1),
                    ..default()
                };
                RESENT
            ],
        };
        assert_eq!(decode::<Inputs>(&encode(&inputs)).unwrap(), inputs);
        let hello = ToHost::Hello {
            protocol: 1,
            name: "Rocket".into(),
            password: "bricks".into(),
            car: Ride::Driver("RR".into()),
            back: Some(7),
        };
        assert_eq!(decode::<ToHost>(&encode(&hello)).unwrap(), hello);
        assert!(decode::<ToPlayer>(&[0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn an_arena_is_for_a_battle_or_a_game_of_tag() {
        let circuits = Circuits::find();
        let settings = Settings::new(&circuits);
        let arena = circuits.0.iter().find(|c| c.arena()).expect("an arena");
        let wanted = |mode: Mode| Rules {
            circuit: key(arena).to_string(),
            mode: mode as u8,
            ..Rules::of(&settings, &circuits)
        };
        assert_eq!(wanted(Mode::Battle).fitted(&circuits).circuit, key(arena));
        assert_eq!(wanted(Mode::Tag).fitted(&circuits).circuit, key(arena));
        for mode in [Mode::Race, Mode::Elimination] {
            let fitted = wanted(mode).fitted(&circuits);
            assert_eq!(fitted.circuit, key(&circuits.0[0]));
            assert_eq!(fitted.mode(), mode);
        }
        // A circuit that isn't an arena is for a race: a battle or a game of tag
        // voted onto one is fought in the first arena.
        for mode in [Mode::Battle, Mode::Tag] {
            let mut plain = wanted(mode);
            plain.circuit = key(&circuits.0[0]).to_string();
            assert_eq!(plain.fitted(&circuits).circuit, key(arena));
        }
    }

    #[test]
    fn how_a_battle_and_a_game_of_tag_are_run_goes_with_the_rules() {
        let circuits = Circuits::find();
        let mut hosts = Settings::new(&circuits);
        (hosts.lives, hosts.battle_time, hosts.spared) = (7, 0, 5);
        (hosts.tag_least, hosts.tag_back) = (0, 4);
        let rules = Rules::of(&hosts, &circuits);
        let mut players = Settings::new(&circuits);
        assert!(
            decode::<Rules>(&encode(&rules))
                .unwrap()
                .apply(&mut players, &circuits)
        );
        assert_eq!(
            (players.lives, players.battle_time(), players.spared()),
            (7, 60.0, 3.0)
        );
        assert_eq!((players.tag_least(), players.tag_back()), (0.0, 4.0));
        // What is out of range is brought into it.
        let wild = Rules {
            lives: 0,
            battle_time: 99,
            ..rules
        };
        assert!(wild.apply(&mut players, &circuits));
        assert_eq!((players.lives, players.battle_time()), (1, f32::INFINITY));
    }

    #[test]
    fn a_session_s_rules_are_put_back_and_the_player_s_own_choices_kept() {
        let circuits = Circuits::find();
        let own = Settings::new(&circuits);
        let mut hosts = Settings::new(&circuits);
        (hosts.circuit, hosts.lap_choice, hosts.mirror, hosts.reverse) = (1, 2, true, true);
        (hosts.mode, hosts.bricks, hosts.speed, hosts.opponents) = (Mode::Tag, 3, 4, 2);
        (
            hosts.difficulty,
            hosts.lives,
            hosts.battle_time,
            hosts.spared,
        ) = (2, 7, 0, 5);
        (hosts.tag_least, hosts.tag_back, hosts.goal) = (0, 4, 2);
        (hosts.teams, hosts.returns) = (true, true);
        let rules = Rules::of(&hosts, &circuits);
        // What the session sets, and what the player changes of their own in it.
        let mut raced = own.clone();
        assert!(rules.apply(&mut raced, &circuits));
        (raced.rivals, raced.sides) = (Some(5), 0b1010);
        (raced.music, raced.racer, raced.car, raced.language) = (3, 2, 9, 4);
        let session = super::super::Session {
            own: Some(own.clone()),
            ..default()
        };
        let back = session.own(&raced).unwrap();
        let rules_of = |s: &Settings| {
            (
                (s.circuit, s.lap_choice, s.mirror, s.reverse, s.elimination),
                (s.mode, s.bricks, s.speed, s.opponents, s.difficulty),
                (s.lives, s.battle_time, s.spared, s.tag_least, s.tag_back),
                (s.goal, s.teams, s.returns, s.rivals, s.sides),
                (s.championship.clone(), s.time_race),
            )
        };
        assert_eq!(rules_of(&back), rules_of(&own));
        assert_eq!(
            (back.music, back.racer, back.car, back.language),
            (3, 2, 9, 4)
        );
        // A host's rules are its own, but for how a race is won.
        let mut kept = own.clone();
        rules.keep(&mut kept);
        assert_eq!(
            (
                kept.lap_choice,
                kept.mirror,
                kept.lives,
                kept.tag_back,
                kept.teams
            ),
            (2, true, 7, 4, true)
        );
        assert_eq!(
            (kept.mode, kept.circuit, kept.elimination),
            (own.mode, own.circuit, false)
        );
        Rules {
            mode: Mode::Elimination as u8,
            ..rules
        }
        .keep(&mut kept);
        assert!(kept.elimination && kept.mode == own.mode);
    }

    #[test]
    fn a_map_of_the_port_s_own_goes_with_the_start() {
        let circuits = Circuits::find();
        let on = |circuit: usize| {
            let settings = Settings {
                circuit,
                ..Settings::new(&circuits)
            };
            Rules::of(&settings, &circuits)
        };
        let brick = circuits.0.iter().position(|c| c.key() == "BRICK").unwrap();
        let text = on(brick).map(&circuits).expect("the brick circuit's map");
        assert_eq!(text, crate::map::named("BRICK").text);
        // One of the game's own circuits is in everyone's data, and isn't sent.
        if circuits.0[0].race.is_some() {
            assert_eq!(on(0).map(&circuits), None);
        }
        // It fits in a message with the rest of the start, and comes back whole.
        let start = encode(&ToPlayer::Start {
            rules: on(brick),
            seats: Vec::new(),
            map: Some(text.clone()),
        });
        assert!(start.len() < 16 * 1024);
        let Ok(ToPlayer::Start { map, .. }) = decode::<ToPlayer>(&start) else {
            panic!("a start");
        };
        assert_eq!(map, Some(text));
        // A vote for a map the host hasn't got is run on the first circuit there is.
        let mut wish = on(brick);
        wish.circuit = "NOWHERE".into();
        assert_eq!(wish.fitted(&circuits).circuit, key(&circuits.0[0]));
    }

    #[test]
    fn a_pedal_only_goes_down_so_far() {
        let c = Drive {
            throttle: 9.0,
            steer: -9.0,
            start_boost: Some(7),
            ..default()
        }
        .controls();
        assert_eq!((c.throttle, c.steer, c.start_boost), (1.0, -1.0, Some(1)));
    }
}
