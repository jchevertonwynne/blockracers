//! What the player has won, and where it is kept: the circuits opened, the part sets
//! and minifigure parts earned, and the races whose record has been beaten. Follows
//! the unlock fields of `PersistentGameState`, as `GameState` sets them and
//! `AwardCinematicScreen::GrantAwards`, `MenuManager::ProcessRecordBeaten`,
//! `CarModelScreenBase::PopulateCategoryCarousel` and `MenuRacerCarousel` use them.
//!
//! `BRICK_PROGRESS=<file>` says which file (default `~/.brick_racers_progress`). A
//! demo without it has everything, and keeps nothing.
//!
//! The original gives nothing to a player racing as one of its own drivers; here
//! only the trophy needs a racer of the player's to go to.

use crate::assets::Jam;
use crate::roster;
use bevy::prelude::*;
use std::path::PathBuf;

/// The part sets every game begins with; the rest are won.
pub const FREE_SETS: usize = 4;
/// The part set that goes with every record beaten, Veronica Voltage's.
pub const RECORD_SET: usize = 7;
/// The races with a record to beat (`GameState::AreAllRacesUnlocked`).
const RECORDS: usize = 12;
const ALL_RECORDS: u16 = 0x0fff;
/// A minifigure part marked with this is had by beating every record; one marked
/// with more than `FREE_PARTS` goes with the part set that many less three.
const RECORD_PART: u8 = 0x80;
const FREE_PARTS: u8 = 2;

/// What has just been won, for the menu to say.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Award {
    /// Where the player came in a circuit, if a circuit is what was raced.
    pub place: Option<usize>,
    /// The next circuit is open, and wasn't.
    pub circuit: bool,
    /// The part set that is now the player's, counted from the first that is won.
    pub parts: Option<usize>,
}

#[derive(Resource, Default)]
pub struct Progress {
    /// How many of the circuits have been opened.
    pub circuits: usize,
    /// The part sets won, a bit each (`m_partUnlockFlags`).
    pub parts: u8,
    /// The races whose record has been beaten, a bit each (`m_unlockedRaces`).
    pub records: u16,
    /// What the last race won, until the menu has said so.
    pub award: Option<Award>,
    /// The folders of the races that have a record, in the order of their bits.
    races: Vec<String>,
    file: Option<PathBuf>,
}

impl Progress {
    /// What had been won when the game was last left; a demo has everything unless
    /// it names a file.
    pub fn open(demo: bool, jam: Option<&Jam>) -> Progress {
        let named = std::env::var_os("BRICK_PROGRESS").map(PathBuf::from);
        let home = || Some(PathBuf::from(std::env::var_os("HOME")?).join(".brick_racers_progress"));
        let races = jam.map(roster::races).unwrap_or_default();
        let mut progress = Progress {
            circuits: 1,
            races: races.into_iter().take(RECORDS).map(|r| r.folder).collect(),
            ..default()
        };
        match named.or_else(|| home().filter(|_| !demo)) {
            Some(file) => {
                progress.read(&std::fs::read_to_string(&file).unwrap_or_default());
                progress.file = Some(file);
            }
            None => {
                (progress.circuits, progress.parts, progress.records) =
                    (usize::MAX, u8::MAX, ALL_RECORDS)
            }
        }
        progress
    }

    /// Takes what a file of `write`'s says. A file of one number is the port's as
    /// it was when only the circuits opened were kept.
    fn read(&mut self, text: &str) {
        for line in text.lines() {
            let (name, value) = line.split_once('=').unwrap_or(("circuits", line));
            let Ok(value) = value.trim().parse::<usize>() else {
                continue;
            };
            match name {
                "circuits" => self.circuits = value.max(1),
                "parts" => self.parts = value as u8,
                "records" => self.records = value as u16 & ALL_RECORDS,
                _ => {}
            }
        }
    }

    fn write(&self) -> String {
        format!(
            "circuits={}\nparts={}\nrecords={}\n",
            self.circuits, self.parts, self.records
        )
    }

    /// Gives up everything that has been won, and keeps that: the port's own, for
    /// the options' "reset progress".
    pub fn forget(&mut self) {
        (self.circuits, self.parts, self.records, self.award) = (1, 0, 0, None);
        self.keep();
    }

    /// Writes what has been won out, after more is.
    pub fn keep(&self) {
        if let Some(Err(error)) = self
            .file
            .as_ref()
            .map(|file| std::fs::write(file, self.write()))
        {
            warn!("could not keep what has been won: {error}");
        }
    }

    /// Whether a part set is the player's to build with, by its place among the sets.
    pub fn set_open(&self, set: usize) -> bool {
        set < FREE_SETS || self.parts & (1 << (set - FREE_SETS).min(7)) != 0
    }

    /// Whether a minifigure part is the player's, by what the catalogue marks it with.
    pub fn part_open(&self, mark: u8) -> bool {
        match mark {
            RECORD_PART => self.records == ALL_RECORDS,
            mark if mark > FREE_PARTS => self.parts & (1 << (mark - FREE_PARTS - 1).min(7)) != 0,
            _ => true,
        }
    }

    /// `AwardCinematicScreen::UnlockPartSet`: the part set a circuit's winner is
    /// given. Whether it is new to the player.
    fn win_parts(&mut self, set: usize) -> bool {
        let bit = 1u8 << set.min(7);
        let new = self.parts & bit == 0;
        self.parts |= bit;
        new
    }

    /// `AwardCinematicScreen::GrantAwards`: a circuit has been raced to the end. The
    /// first three have a trophy, which the caller gives the racer; the winner has
    /// the circuit's part set.
    pub fn finish(&mut self, place: usize, opened: bool, parts: Option<usize>) {
        let parts = parts.filter(|set| place == 1 && self.win_parts(*set));
        self.circuits = self.circuits.max(1) + opened as usize;
        self.award = Some(Award {
            place: Some(place),
            circuit: opened,
            parts,
        });
        self.keep();
    }

    /// `MenuManager::ProcessRecordBeaten`: the record of the race in this folder has
    /// been beaten. The last of them to be beaten wins a part set.
    pub fn beat(&mut self, folder: &str) {
        let Some(race) = self.races.iter().position(|race| race == folder) else {
            return;
        };
        let bit = 1 << race;
        if self.records & bit != 0 {
            return;
        }
        self.records |= bit;
        if self.records == ALL_RECORDS && self.win_parts(RECORD_SET) {
            self.award = Some(Award {
                parts: Some(RECORD_SET),
                ..default()
            });
        }
        self.keep();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_circuit_won_gives_its_parts_once() {
        let mut progress = Progress {
            circuits: 1,
            ..default()
        };
        assert!(progress.set_open(3) && !progress.set_open(4));
        // Second opens the next circuit and wins no parts.
        progress.finish(2, true, Some(0));
        assert_eq!((progress.circuits, progress.parts), (2, 0));
        assert_eq!(
            progress.award,
            Some(Award {
                place: Some(2),
                circuit: true,
                parts: None
            })
        );
        progress.finish(1, false, Some(0));
        assert_eq!(progress.award.unwrap().parts, Some(0));
        assert!(progress.set_open(4) && !progress.set_open(5));
        // A part marked three goes with the first set won; one marked five doesn't.
        assert!(progress.part_open(3) && !progress.part_open(5) && progress.part_open(2));
        // Won again, the parts are nothing new.
        progress.finish(1, false, Some(0));
        assert_eq!(progress.award.unwrap().parts, None);
        assert_eq!((progress.circuits, progress.parts), (2, 1));
    }

    #[test]
    fn every_record_beaten_gives_the_last_parts() {
        let mut progress = Progress {
            races: (0..RECORDS).map(|race| format!("RACE{race}")).collect(),
            ..default()
        };
        for race in 0..RECORDS - 1 {
            progress.beat(&format!("RACE{race}"));
        }
        progress.beat("RACE0");
        progress.beat("SOMEWHERE");
        assert!(!progress.part_open(RECORD_PART) && progress.award.is_none());
        progress.beat("RACE11");
        assert!(progress.part_open(RECORD_PART) && progress.set_open(FREE_SETS + RECORD_SET));
        assert_eq!(progress.award.unwrap().parts, Some(RECORD_SET));
    }

    #[test]
    fn what_was_won_comes_back_from_its_file() {
        let mut progress = Progress {
            circuits: 3,
            parts: 0b101,
            records: 0x801,
            ..default()
        };
        let text = progress.write();
        progress = Progress::default();
        progress.read(&text);
        assert_eq!(
            (progress.circuits, progress.parts, progress.records),
            (3, 0b101, 0x801)
        );
        // The file as it was before parts were kept: the circuits opened, alone.
        let mut old = Progress::default();
        old.read("4\n");
        assert_eq!((old.circuits, old.parts), (4, 0));
    }

    #[test]
    fn the_game_has_a_record_for_each_of_twelve_races() {
        let Some(jam) = crate::world::jam() else {
            return;
        };
        let progress = Progress::open(true, Some(&jam));
        assert_eq!(progress.races.len(), RECORDS);
        assert!(progress.races.iter().all(|race| race.starts_with("RACEC")));
    }
}

#[cfg(test)]
#[test]
fn progress_given_up_is_a_new_games_and_is_kept() {
    let file = std::env::temp_dir().join(format!("brick_progress_{}", std::process::id()));
    let mut progress = Progress {
        circuits: 5,
        parts: 0b1011,
        records: 0b110,
        award: Some(Award::default()),
        file: Some(file.clone()),
        ..default()
    };
    progress.keep();
    progress.forget();
    assert_eq!((progress.circuits, progress.parts, progress.records), (1, 0, 0));
    assert!(progress.award.is_none() && !progress.set_open(FREE_SETS));
    // What the file says now is what a game with nothing won reads.
    let mut read = Progress { circuits: 9, parts: 9, records: 9, ..default() };
    read.read(&std::fs::read_to_string(&file).unwrap());
    assert_eq!((read.circuits, read.parts, read.records), (1, 0, 0));
    let _ = std::fs::remove_file(file);
}
