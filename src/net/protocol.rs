//! What a host and its players say to each other. The port's own.
//!
//! Two kinds of message. Those that must arrive, and in order, go as `ToHost` and
//! `ToPlayer`: joining, who sits where, the race beginning and ending. Those that are
//! out of date a moment later go as `Inputs` and `Snapshot`, sixty and thirty times a
//! second, and a lost one is simply made up for by the next.

use bevy::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::room::Voter;
use super::state::{Standing, State};
use crate::kart::Controls;
use crate::menu::{Circuits, LAP_CHOICES, Settings};
use crate::replay::Pose;

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
        Drive { throttle: c.throttle, steer: c.steer, drift: c.drift, tight: c.tight, use_item: c.use_item, direct: c.direct, start_boost: c.start_boost }
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
    /// The player's own car, in full.
    pub own: State,
    /// Every car, the player's too, by grid slot.
    pub karts: Vec<(u8, Pose, Standing)>,
}

/// How a race is to be run: what the host's `Settings` say of it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rules {
    /// The circuit's race folder, or the key of one of the port's own layouts.
    pub circuit: String,
    pub lap_choice: u8,
    pub mirror: bool,
    pub reverse: bool,
    pub bricks: u8,
    pub elimination: bool,
    pub opponents: u8,
    pub difficulty: u8,
}

fn key(circuit: &crate::menu::Circuit) -> &str {
    circuit.race.as_deref().unwrap_or(circuit.layout.key())
}

impl Rules {
    pub fn of(settings: &Settings, circuits: &Circuits) -> Self {
        Rules {
            circuit: circuits.0.get(settings.circuit).map(key).unwrap_or_default().to_string(),
            lap_choice: settings.lap_choice as u8,
            mirror: settings.mirror,
            reverse: settings.reverse,
            bricks: settings.bricks as u8,
            elimination: settings.elimination,
            opponents: settings.opponents as u8,
            difficulty: settings.difficulty as u8,
        }
    }

    /// Sets a game up to race by these. False if it hasn't the circuit.
    pub fn apply(&self, settings: &mut Settings, circuits: &Circuits) -> bool {
        let Some(circuit) = circuits.0.iter().position(|c| key(c) == self.circuit) else { return false };
        settings.circuit = circuit;
        settings.lap_choice = (self.lap_choice as usize).min(LAP_CHOICES.len() - 1);
        (settings.championship, settings.time_race) = (None, false);
        (settings.mirror, settings.reverse, settings.elimination) = (self.mirror, self.reverse, self.elimination);
        settings.bricks = (self.bricks as usize).min(crate::menu::BRICK_RULES.len() - 1);
        settings.opponents = (self.opponents as usize).min(crate::menu::MAX_OPPONENTS);
        settings.difficulty = (self.difficulty as usize).min(crate::menu::DIFFICULTIES.len() - 1);
        true
    }
}

/// A place on the grid and who has it: a player, or one of the computer's drivers.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Seat {
    pub slot: u8,
    pub peer: Option<Peer>,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Refusal {
    /// The two games don't speak the same `lobby_api::PROTOCOL`.
    Version,
    Password,
    Full,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToHost {
    Hello { protocol: u32, name: String, password: String },
    /// The race asked for is loaded and its cars are on the grid.
    Loaded,
    /// How the player would have the next race run, and whether they are ready for it.
    Vote(Rules),
    Ready(bool),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToPlayer {
    Welcome { you: Peer },
    Refused(Refusal),
    Start { rules: Rules, seats: Vec<Seat> },
    /// How the room stands: who is in it and what they want, the seconds until the
    /// vote closes if a clock is running, how the last race was run and who came
    /// where in it.
    Room { voters: Vec<Voter>, closing: Option<f32>, last: Option<Rules>, results: Vec<String> },
    /// The race is run, and everyone is back in the room.
    Over,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_come_back_as_they_went() {
        let inputs = Inputs { last: 77, drives: vec![Drive { throttle: 1.0, steer: -0.5, use_item: true, start_boost: Some(1), ..default() }; RESENT] };
        assert_eq!(decode::<Inputs>(&encode(&inputs)).unwrap(), inputs);
        let hello = ToHost::Hello { protocol: 1, name: "Rocket".into(), password: "bricks".into() };
        assert_eq!(decode::<ToHost>(&encode(&hello)).unwrap(), hello);
        assert!(decode::<ToPlayer>(&[0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn a_pedal_only_goes_down_so_far() {
        let c = Drive { throttle: 9.0, steer: -9.0, start_boost: Some(7), ..default() }.controls();
        assert_eq!((c.throttle, c.steer, c.start_boost), (1.0, -1.0, Some(1)));
    }
}
