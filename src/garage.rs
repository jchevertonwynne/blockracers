//! The racers the player has built, and where they are kept: a file of the game's own
//! kind for saved racers (`assets::lrs`), so that one of the game's is as good.
//!
//! `BRICK_GARAGE=<file>` says which file (default `~/.brick_racers_garage`). A demo
//! without it has the game's quick-build racers for a garage, and keeps nothing.

use crate::assets::{
    Jam,
    lrs::{self, Racer},
};
use crate::menu::Settings;
use crate::net::protocol::Ride;
use bevy::prelude::*;
use std::path::PathBuf;

/// The game's own racers, one for each chassis and more: what its quick build hands
/// out, and what a new car is offered.
const QUICK_BUILD: &str = "/MENUDATA/QBUILD.LRS";
/// The most racers the game's garage holds.
pub const MOST: usize = 209;

#[derive(Resource, Default)]
pub struct Garage {
    pub racers: Vec<Racer>,
    file: Option<PathBuf>,
}

impl Garage {
    /// The garage as it was left; a demo's is `stock` unless it names a file.
    pub fn open(demo: bool, jam: Option<&Jam>) -> Garage {
        let named = std::env::var_os("BRICK_GARAGE").map(PathBuf::from);
        let home = || Some(PathBuf::from(std::env::var_os("HOME")?).join(".brick_racers_garage"));
        match named.or_else(|| home().filter(|_| !demo)) {
            Some(file) => Garage {
                racers: std::fs::read(&file).map_or_else(|_| Vec::new(), |data| lrs::read(&data)),
                file: Some(file),
            },
            None => Garage {
                racers: jam.map(stock).unwrap_or_default(),
                file: None,
            },
        }
    }

    /// Writes the garage out, after a change to it.
    pub fn keep(&self) {
        if let Some(Err(error)) = self
            .file
            .as_ref()
            .map(|file| std::fs::write(file, lrs::write(&self.racers)))
        {
            warn!("could not keep the garage: {error}");
        }
    }

    /// What the player races as online: whoever has their grid slot, one of the game's
    /// drivers, or after those one of the racers here.
    pub fn ride(&self, settings: &Settings) -> Ride {
        let drivers = crate::roster::NAMES;
        match settings.car.checked_sub(1) {
            None => Ride::Slot,
            Some(n) if n < drivers.len() => Ride::Driver(drivers[n].0.into()),
            Some(n) => self
                .racers
                .get(n - drivers.len())
                .map_or(Ride::Slot, |racer| Ride::Built(racer.clone())),
        }
    }

    /// The racer the player races as, if they have built one.
    pub fn racing(&self, settings: &Settings) -> Option<&Racer> {
        self.racers.get(settings.racer.checked_sub(1)?)
    }
}

/// The game's quick-build racers.
pub fn stock(jam: &Jam) -> Vec<Racer> {
    jam.get(QUICK_BUILD).map(lrs::read).unwrap_or_default()
}
