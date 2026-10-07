//! The original's licence-plate cheat codes. A name typed on the driver's licence that
//! is one of the thirteen below is not a name but a switch: the code turns its cheat on,
//! or off again if it was on, and the race reads the cheats that are on. After
//! `DriverLicenseScreen::ApplyCheatCode` (`cheatflags.h`), which keeps them in
//! `LegoRacers::Context::m_cheatFlags`: they last for the session and are not saved
//! with the racer or the settings. The original makes no sound and shows no message
//! when a code is taken.
//!
//! What each does, where the original reads it:
//! - `NSLWJ` (`Racer::Initialize`, `RacerPhysics::ApplyWheelSurface`): the surfaces
//!   of the road no longer slow, grip or push the cars; they all drive as on bare
//!   ground.
//! - `FLYSKYHGH` (`Racer::Update`, `TurboAction`, `Racer::AiUsePowerup`): every car
//!   that is not held fires a full turbo every frame, so it never runs out; green
//!   bricks are of no use but at full charge, when they warp.
//! - `PGLLRD`, `PGLLYLL`, `PGLLGRN` (`RacePowerupManager::ParseColorBricks`): every
//!   coloured brick is red, yellow or green. One cancels the other two and `RPCRNLY`.
//! - `RPCRNLY` (`ParseColorBricks`, `Racer::AiUsePowerup`): every coloured brick is
//!   red, and whatever brick is held fires the red hook of the first level.
//! - `MXPMX` (`Racer::AiUsePowerup`): whatever brick is held fires at full strength,
//!   white bricks or none.
//! - `LNFRRRM` (`MenuManager::PrepareRaceContext`): Rocket Racer's race (circuit `c6`)
//!   is run mirrored.
//! - `FSTFRWRD` (`RaceSession::Update`): the race runs 1.75 times as fast.
//! - `NWHLS`, `NCHSSS`, `NDRVR` (`CarVisuals::Draw`): the wheels, the chassis or the
//!   driver is not drawn. Of the three at most two are on at once.
//! - `NMRCHTS` turns every cheat off.
//!
//! The original clears the cheats as a circuit race or a time race begins
//! (`PrepareRaceContext`), so only a race on its own has them. Online a race is run by
//! the session's rules and not a player's own, so the cheats that change the race
//! (everything but the last three before `NMRCHTS`) are not taken into it; the ones
//! that only change how a player's own car looks are, on their own car alone. These
//! are the port's own choices; `BRICK_CHEATS=FSTFRWRD,NWHLS` sets codes without
//! typing them, which is the port's own too.

use crate::championship::Championship;
use crate::items::Power;
use crate::kart::{Kart, Player};
use crate::menu::Screen;
use crate::net::Role;
use crate::time_race::Part;
use crate::{Phase, Race};
use bevy::prelude::*;

/// The codes, in the order of the flags: the first is bit 0.
pub const CODES: [&str; 13] = [
    "NSLWJ",
    "FLYSKYHGH",
    "PGLLRD",
    "PGLLYLL",
    "PGLLGRN",
    "LNFRRRM",
    "RPCRNLY",
    "MXPMX",
    "FSTFRWRD",
    "NWHLS",
    "NCHSSS",
    "NDRVR",
    "NMRCHTS",
];

pub const NSLWJ: u16 = 1 << 0;
pub const FLYSKYHGH: u16 = 1 << 1;
pub const PGLLRD: u16 = 1 << 2;
pub const PGLLYLL: u16 = 1 << 3;
pub const PGLLGRN: u16 = 1 << 4;
pub const LNFRRRM: u16 = 1 << 5;
pub const RPCRNLY: u16 = 1 << 6;
pub const MXPMX: u16 = 1 << 7;
pub const FSTFRWRD: u16 = 1 << 8;
pub const NWHLS: u16 = 1 << 9;
pub const NCHSSS: u16 = 1 << 10;
pub const NDRVR: u16 = 1 << 11;

/// What only changes how a player's own car looks.
const LOOKS: u16 = NWHLS | NCHSSS | NDRVR;

/// How much faster `FSTFRWRD` runs the race (`g_unk0x004b08c0`).
pub const FAST_FORWARD: f32 = 1.75;

/// What a code did when it was entered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entered {
    /// The name is not a code.
    Nothing,
    On(u16),
    Off(u16),
    /// `NMRCHTS`.
    AllOff,
}

/// The cheats that are on: `m_cheatFlags`, kept for the session.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cheats(pub u16);

impl Cheats {
    /// `ApplyCheatCode`: a name that is a code switches its cheat, and putting one on
    /// takes off the ones it can't go with.
    pub fn enter(&mut self, name: &str) -> Entered {
        let Some(at) = CODES.iter().position(|code| *code == name) else {
            return Entered::Nothing;
        };
        if at == CODES.len() - 1 {
            self.0 = 0;
            return Entered::AllOff;
        }
        let flag = 1 << at;
        if self.0 & flag != 0 {
            self.0 &= !flag;
            return Entered::Off(flag);
        }
        self.0 |= flag;
        match flag {
            PGLLRD => self.0 &= !(PGLLYLL | PGLLGRN | RPCRNLY),
            PGLLYLL => self.0 &= !(PGLLRD | PGLLGRN | RPCRNLY),
            PGLLGRN => self.0 &= !(PGLLRD | PGLLYLL | RPCRNLY),
            RPCRNLY => self.0 &= !(PGLLRD | PGLLYLL | PGLLGRN | MXPMX),
            MXPMX => self.0 &= !RPCRNLY,
            // Of the three that take a part of the car away, never all three.
            NWHLS if self.0 & NCHSSS != 0 && self.0 & NDRVR != 0 => self.0 &= !NDRVR,
            NCHSSS if self.0 & NWHLS != 0 && self.0 & NDRVR != 0 => self.0 &= !NWHLS,
            NDRVR if self.0 & NCHSSS != 0 && self.0 & NWHLS != 0 => self.0 &= !NCHSSS,
            _ => {}
        }
        Entered::On(flag)
    }

    /// `BRICK_CHEATS=FSTFRWRD,NWHLS`: codes entered one after another.
    pub fn from_env() -> Self {
        let mut cheats = Cheats::default();
        for code in std::env::var("BRICK_CHEATS").unwrap_or_default().split(',') {
            cheats.enter(code.trim().to_ascii_uppercase().as_str());
        }
        cheats
    }

    /// A race is about to begin: what it is to have of the cheats. A circuit race or a
    /// time race has none, and clears them (`MenuManager::PrepareRaceContext`).
    pub fn begin(&mut self, circuit_race: bool, time_race: bool, online: bool) -> Raced {
        if circuit_race || time_race {
            self.0 = 0;
        }
        Raced(if online { self.0 & LOOKS } else { self.0 })
    }
}

/// The cheats the race in hand has.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Raced(pub u16);

impl Raced {
    pub fn has(self, flag: u16) -> bool {
        self.0 & flag != 0
    }

    /// Which of `menu::BRICK_RULES` the coloured bricks are made by, if a cheat sets one.
    pub fn brick_rule(self) -> Option<usize> {
        if self.has(PGLLRD | RPCRNLY) {
            Some(1)
        } else if self.has(PGLLYLL) {
            Some(2)
        } else if self.has(PGLLGRN) {
            Some(4)
        } else {
            None
        }
    }

    /// Whether `LNFRRRM` has this race run mirrored: it is Rocket Racer's.
    pub fn mirrors(self, championship: &Championship, folder: Option<&str>) -> bool {
        self.has(LNFRRRM)
            && folder.is_some_and(|folder| {
                championship
                    .series
                    .iter()
                    .any(|series| series.code == "c6" && series.rounds.iter().any(|r| r == folder))
            })
    }

    /// What a held brick does under `RPCRNLY`, `MXPMX` and `FLYSKYHGH`
    /// (`Racer::AiUsePowerup`): its colour and level, or `None` for a green one that is
    /// let go to waste.
    pub fn fired(self, power: Power, level: u8) -> Option<(Power, u8)> {
        if self.has(RPCRNLY) {
            return Some((Power::Red, 1));
        }
        let level = if self.has(MXPMX) { 3 } else { level };
        if power == Power::Green && self.has(FLYSKYHGH) && level != 3 {
            return None;
        }
        Some((power, level))
    }
}

/// A licence done with: its name is taken as a code if it is one
/// (`DriverLicenseScreen::ApplyCheatCode`).
pub fn take_codes(mut bench: ResMut<crate::frontend::Bench>, mut cheats: ResMut<Cheats>) {
    if let Some(name) = bench.take_code() {
        let entered = cheats.enter(&name);
        if entered != Entered::Nothing {
            info!("cheat code {name}: {entered:?}, now {:?}", cheats.0);
        }
    }
}

/// Runs the race faster under `FSTFRWRD`: all of the race's own time goes by faster,
/// as the original's `RaceSession::Update` has it.
pub fn fast_forward(
    raced: Res<Raced>,
    screen: Res<State<Screen>>,
    mut clock: ResMut<Time<Virtual>>,
) {
    let wanted = if *screen.get() == Screen::Race && raced.has(FSTFRWRD) {
        FAST_FORWARD
    } else {
        1.0
    };
    if clock.relative_speed() != wanted {
        clock.set_relative_speed(wanted);
    }
}

/// `Racer::Update`: a car that is not held fires a full turbo every frame.
pub fn fly_sky_high(raced: Res<Raced>, race: Res<Race>, mut karts: Query<&mut Kart>) {
    if !raced.has(FLYSKYHGH) || race.phase != Phase::Racing {
        return;
    }
    for mut kart in &mut karts {
        if kart.out.is_none() && !kart.halted() {
            kart.start_boost(2);
        }
    }
}

/// `CarVisuals::Draw`: the parts of a car that a cheat takes away are not drawn.
pub fn hide_parts(
    raced: Res<Raced>,
    role: Res<Role>,
    mut parts: Query<(&Part, &ChildOf, &mut Visibility)>,
    karts: Query<Has<Player>, With<Kart>>,
) {
    for (part, parent, mut visibility) in &mut parts {
        // Online it is the player's own car that is changed.
        let Ok(own) = karts.get(parent.parent()) else {
            continue;
        };
        let flag = match part {
            Part::Wheels => NWHLS,
            Part::Chassis => NCHSSS,
            Part::Driver => NDRVR,
        };
        if raced.has(flag) && (own || *role == Role::Offline) {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(cheats: &mut Cheats, codes: &[&str]) {
        for code in codes {
            cheats.enter(code);
        }
    }

    #[test]
    fn a_name_that_is_a_code_switches_its_cheat() {
        let mut cheats = Cheats::default();
        assert_eq!(cheats.enter("PLAYER"), Entered::Nothing);
        assert_eq!(cheats.enter("nwhls"), Entered::Nothing);
        assert_eq!(cheats.enter("NWHLS"), Entered::On(NWHLS));
        assert_eq!(cheats.0, NWHLS);
        assert_eq!(cheats.enter("NWHLS"), Entered::Off(NWHLS));
        assert_eq!(cheats.0, 0);
        // Each code has the flag of its place in the list.
        for (at, code) in CODES[..12].iter().enumerate() {
            let mut cheats = Cheats::default();
            assert_eq!(cheats.enter(code), Entered::On(1 << at));
        }
    }

    #[test]
    fn the_brick_colours_cancel_each_other_and_red_only() {
        let mut cheats = Cheats::default();
        typed(&mut cheats, &["PGLLRD", "PGLLYLL"]);
        assert_eq!(cheats.0, PGLLYLL);
        typed(&mut cheats, &["PGLLGRN"]);
        assert_eq!(cheats.0, PGLLGRN);
        typed(&mut cheats, &["RPCRNLY"]);
        assert_eq!(cheats.0, RPCRNLY);
        // Red only and the maximum power-ups go no further together.
        typed(&mut cheats, &["MXPMX"]);
        assert_eq!(cheats.0, MXPMX);
        typed(&mut cheats, &["RPCRNLY", "PGLLRD"]);
        assert_eq!(cheats.0, PGLLRD);
        // The others don't care.
        typed(&mut cheats, &["MXPMX", "FSTFRWRD", "NSLWJ"]);
        assert_eq!(cheats.0, PGLLRD | MXPMX | FSTFRWRD | NSLWJ);
    }

    #[test]
    fn a_car_is_never_left_with_nothing_of_it_drawn() {
        let mut cheats = Cheats::default();
        typed(&mut cheats, &["NWHLS", "NCHSSS"]);
        assert_eq!(cheats.0, NWHLS | NCHSSS);
        // The third takes the one entered before it away, as the original does.
        typed(&mut cheats, &["NDRVR"]);
        assert_eq!(cheats.0, NWHLS | NDRVR);
        typed(&mut cheats, &["NCHSSS"]);
        assert_eq!(cheats.0, NCHSSS | NDRVR);
        typed(&mut cheats, &["NWHLS"]);
        assert_eq!(cheats.0, NWHLS | NCHSSS);
    }

    #[test]
    fn nmrchts_turns_them_all_off() {
        let mut cheats = Cheats::default();
        typed(&mut cheats, &["NSLWJ", "FSTFRWRD", "NWHLS", "LNFRRRM"]);
        assert_ne!(cheats.0, 0);
        assert_eq!(cheats.enter("NMRCHTS"), Entered::AllOff);
        assert_eq!(cheats.0, 0);
    }

    #[test]
    fn a_circuit_race_or_a_time_race_clears_them_and_online_only_the_looks_stay() {
        let mut cheats = Cheats(FSTFRWRD | NWHLS | PGLLRD);
        assert_eq!(cheats.begin(false, false, false).0, FSTFRWRD | NWHLS | PGLLRD);
        assert_eq!(cheats.begin(false, false, true).0, NWHLS);
        assert_eq!(cheats.0, FSTFRWRD | NWHLS | PGLLRD);
        assert_eq!(cheats.begin(true, false, false), Raced(0));
        assert_eq!(cheats.0, 0);
        let mut cheats = Cheats(MXPMX);
        assert_eq!(cheats.begin(false, true, false), Raced(0));
        assert_eq!(cheats.0, 0);
    }

    #[test]
    fn held_bricks_are_fired_as_the_cheats_say() {
        let none = Raced(0);
        assert_eq!(none.fired(Power::Blue, 1), Some((Power::Blue, 1)));
        assert_eq!(Raced(RPCRNLY).fired(Power::Blue, 3), Some((Power::Red, 1)));
        assert_eq!(Raced(MXPMX).fired(Power::Yellow, 0), Some((Power::Yellow, 3)));
        // Turbos are not fired by a car that is flying already, but warps are.
        assert_eq!(Raced(FLYSKYHGH).fired(Power::Green, 2), None);
        assert_eq!(Raced(FLYSKYHGH).fired(Power::Red, 2), Some((Power::Red, 2)));
        assert_eq!(
            Raced(FLYSKYHGH | MXPMX).fired(Power::Green, 0),
            Some((Power::Green, 3))
        );
        assert_eq!(Raced(PGLLYLL).brick_rule(), Some(2));
        assert_eq!(Raced(PGLLGRN).brick_rule(), Some(4));
        assert_eq!(Raced(RPCRNLY).brick_rule(), Some(1));
        assert_eq!(Raced(NSLWJ).brick_rule(), None);
    }

    #[test]
    fn rocket_racer_s_race_is_the_one_that_is_mirrored() {
        let championship = Championship::load(7);
        let Some(series) = championship.series.iter().find(|s| s.code == "c6") else {
            return;
        };
        let folder = series.rounds[0].as_str();
        assert!(Raced(LNFRRRM).mirrors(&championship, Some(folder)));
        assert!(!Raced(0).mirrors(&championship, Some(folder)));
        assert!(!Raced(LNFRRRM).mirrors(&championship, Some("RACEC0R0")));
        assert!(!Raced(LNFRRRM).mirrors(&championship, None));
    }
}
