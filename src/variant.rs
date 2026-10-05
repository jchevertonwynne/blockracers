//! A circuit the other way about: mirrored, as the original runs the races of its
//! fourth, fifth and sixth circuits, or reversed, which is the port's own.
//!
//! The original mirrors by negating Y in everything it loads (`GolWorldDatabase::MirrorY`,
//! and the `p_mirror` that each loader of `RaceSession` takes). Here that is done where
//! the game's axes are turned into ours: see `scenery::set_mirror`.
//!
//! Reversed, the racing line, the checkpoints and the lap zones are walked backwards
//! (`Track::reverse`) and the computer's cars drive for themselves, there being no
//! recordings of the circuit that way round.

use crate::championship::Championship;
use crate::menu::Settings;
use bevy::prelude::*;

/// How the race in hand is run.
#[derive(Resource, Clone, Copy, Default, PartialEq, Debug)]
pub struct Variant {
    pub mirror: bool,
    pub reverse: bool,
}

/// What a mirrored race adds to the pace of the computer's cars (`g_mirroredRaceStateRouteScale`).
pub const MIRROR_BOOST: f32 = 0.05;

/// Circuits with a drop or a jump that can only be taken forwards: the computer's car
/// can't get round these backwards (`ai_laps_the_original_circuits_backwards`), and
/// they are always raced the right way.
pub const ONE_WAY: [&str; 2] = ["RACEC1R2", "RACEC1R3"];

impl Variant {
    /// A race of a circuit is mirrored when the game's tables say so; one on its own,
    /// as the settings have it. `folder` is the race's, if it is one of the original's.
    pub fn of(settings: &Settings, championship: &Championship, folder: Option<&str>) -> Self {
        match championship.mirrored() {
            Some(mirror) => Variant {
                mirror,
                reverse: false,
            },
            None => Variant {
                mirror: settings.mirror,
                reverse: settings.reverse && !folder.is_some_and(|f| ONE_WAY.contains(&f)),
            },
        }
    }

    /// What tells this variant's records from another's.
    pub fn suffix(self) -> &'static str {
        match (self.mirror, self.reverse) {
            (false, false) => "",
            (true, false) => "-mirror",
            (false, true) => "-reverse",
            (true, true) => "-mirror-reverse",
        }
    }

    pub fn rubber_band_boost(self) -> f32 {
        if self.mirror { MIRROR_BOOST } else { 0.0 }
    }
}
