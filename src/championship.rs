//! Circuit races: a circuit's races run one after another against its champion's
//! field, with points for each finish. Coming in the first three overall opens the
//! next circuit. Follows `CircuitRaceRunner` and `CircuitStandings`.

use crate::assets::Jam;
use crate::roster;
use bevy::prelude::*;
use std::path::PathBuf;

/// Points for first place down to sixth.
pub const POINTS: [u32; 6] = [30, 20, 10, 3, 2, 1];
/// Finishing a circuit this high or better opens the next.
const QUALIFYING_PLACE: usize = 3;

pub struct Series {
    /// The game's name for the circuit: `c0` and so on.
    pub code: String,
    /// Whose circuit it is.
    pub champion: &'static str,
    /// The folders of its races, in the order they are run.
    pub rounds: Vec<String>,
}

/// A circuit being raced: which race it has come to and who has what.
pub struct Run {
    pub series: usize,
    pub round: usize,
    /// Points by grid slot: in all, and from the race just run.
    pub points: [u32; 6],
    pub round_points: [u32; 6],
    /// The race in hand has been scored.
    pub scored: bool,
}

#[derive(Resource, Default)]
pub struct Championship {
    pub series: Vec<Series>,
    /// How many of the circuits have been opened.
    pub unlocked: usize,
    /// The one picked in the menu.
    pub chosen: usize,
    pub run: Option<Run>,
}

fn save_file() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".brick_racers_progress"))
}

impl Championship {
    pub fn load() -> Self {
        let Some(jam) = Jam::open(std::env::var("LEGO_JAM").unwrap_or("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM".into())) else {
            return Championship::default();
        };
        let series: Vec<Series> = roster::circuits(&jam)
            .into_iter()
            .filter(|(_, rounds)| !rounds.is_empty())
            .map(|(code, rounds)| Series {
                champion: roster::field(&jam, &code).first().map_or("", |d| d.name),
                code,
                rounds: rounds.into_iter().map(|r| r.folder).collect(),
            })
            .collect();
        let saved = save_file().and_then(|file| std::fs::read_to_string(file).ok()).and_then(|text| text.trim().parse().ok());
        Championship { unlocked: saved.unwrap_or(1).clamp(1, series.len().max(1)), series, chosen: 0, run: None }
    }

    /// Begins the chosen circuit, if it has been opened. Returns its first race.
    pub fn begin(&mut self) -> Option<(String, String)> {
        let series = self.series.get(self.chosen).filter(|_| self.chosen < self.unlocked)?;
        self.run = Some(Run { series: self.chosen, round: 0, points: [0; 6], round_points: [0; 6], scored: false });
        Some((series.code.clone(), series.rounds[0].clone()))
    }

    /// Gives out the points for a race from the places its racers took, by grid slot.
    pub fn score(&mut self, places: &[(usize, usize)]) {
        let Some(run) = self.run.as_mut().filter(|run| !run.scored) else { return };
        run.round_points = [0; 6];
        for &(slot, place) in places {
            let points = POINTS.get(place - 1).copied().unwrap_or(0);
            if let Some(total) = run.points.get_mut(slot) {
                *total += points;
                run.round_points[slot] = points;
            }
        }
        run.scored = true;
    }

    /// Where a slot stands on points, first being 1. Ties go to the lower slot.
    pub fn standing(&self, slot: usize) -> usize {
        let Some(run) = &self.run else { return 1 };
        let ahead = |other: usize| run.points[other] > run.points[slot] || (run.points[other] == run.points[slot] && other < slot);
        1 + (0..run.points.len()).filter(|&other| other != slot && ahead(other)).count()
    }

    /// Whether the race just run was the circuit's last.
    pub fn over(&self) -> bool {
        self.run.as_ref().is_some_and(|run| run.scored && run.round + 1 >= self.series[run.series].rounds.len())
    }

    /// Moves on after a scored race: to the next one's folder, or, the circuit being
    /// over, to nothing, opening the next circuit if the player did well enough.
    pub fn advance(&mut self, player: usize) -> Option<String> {
        if self.over() {
            let series = self.run.as_ref().map_or(0, |run| run.series);
            if self.standing(player) <= QUALIFYING_PLACE && series + 1 >= self.unlocked && series + 1 < self.series.len() {
                self.unlocked = series + 2;
                if let Some(file) = save_file() {
                    let _ = std::fs::write(file, self.unlocked.to_string());
                }
            }
            self.run = None;
            return None;
        }
        let run = self.run.as_mut()?;
        (run.round, run.scored) = (run.round + 1, false);
        Some(self.series[run.series].rounds[run.round].clone())
    }
}

#[cfg(test)]
#[test]
fn a_circuit_is_scored_race_by_race() {
    let series = |code: &str, rounds: usize| Series { code: code.into(), champion: "", rounds: vec!["RACE".into(); rounds] };
    let mut championship = Championship { series: vec![series("c0", 2), series("c1", 4)], unlocked: 1, chosen: 1, run: None };
    // The second circuit isn't open yet.
    assert!(championship.begin().is_none());
    championship.chosen = 0;
    assert_eq!(championship.begin(), Some(("c0".into(), "RACE".into())));
    // The player (slot 5) wins, then comes fourth.
    championship.score(&[(5, 1), (0, 2), (1, 3), (2, 4), (3, 5), (4, 6)]);
    championship.score(&[(5, 6)]);
    assert_eq!(championship.run.as_ref().unwrap().points, [20, 10, 3, 2, 1, 30]);
    assert!(!championship.over() && championship.advance(5).is_some());
    championship.score(&[(0, 1), (1, 2), (2, 3), (5, 4), (3, 5), (4, 6)]);
    assert_eq!(championship.run.as_ref().unwrap().points, [50, 30, 13, 4, 2, 33]);
    assert_eq!((championship.standing(0), championship.standing(5), championship.standing(1)), (1, 2, 3));
    // Second overall is enough to open the next circuit. (The save goes to HOME.)
    assert!(championship.over());
    let home = std::env::temp_dir().join("brick_racers_test_home");
    std::fs::create_dir_all(&home).unwrap();
    unsafe { std::env::set_var("HOME", &home) };
    assert!(championship.advance(5).is_none() && championship.run.is_none());
    assert_eq!(championship.unlocked, 2);
}
