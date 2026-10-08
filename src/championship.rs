//! Circuit races: a circuit's races run one after another against its champion's
//! field, with points for each finish. Coming first or second overall opens the next
//! circuit (`AwardCinematicScreen::GrantAwards`); falling behind ten points a race ends
//! it early. Follows `CircuitRaceRunner` and `CircuitStandings`. What a circuit
//! raced to the end wins is `progress`'s to give and to keep.

use crate::assets::Jam;
use crate::roster;
use bevy::prelude::*;

/// Points for first place down to sixth.
pub const POINTS: [u32; 6] = [30, 20, 10, 3, 2, 1];
/// Finishing a circuit this high or better opens the next.
const QUALIFYING_PLACE: usize = 2;
/// The player must have this many points for each race run to be let into the next.
const POINTS_TO_GO_ON: u32 = 10;

pub struct Series {
    /// The game's name for the circuit: `c0` and so on.
    pub code: String,
    /// Whose circuit it is, by the game's short name for them.
    pub champion: String,
    /// The folders of its races, in the order they are run.
    pub rounds: Vec<String>,
    /// Which of them are run mirrored.
    pub mirrored: Vec<bool>,
    /// The part set its winner is given, counted from the first that is won.
    pub parts: Option<usize>,
}

/// What comes after a race of a circuit.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// The next race, by its folder.
    Race(String),
    /// The circuit has been raced to the end: where the player came, and whether
    /// that opened the next circuit.
    Finished { series: usize, place: usize, opened: bool },
    /// The player was too far behind to go on.
    Out,
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

impl Championship {
    /// The game's circuits, this many of them opened.
    pub fn load(unlocked: usize) -> Self {
        let Some(jam) = Jam::open(
            std::env::var("BRICK_JAM")
                .unwrap_or("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM".into()),
        ) else {
            return Championship::default();
        };
        let series: Vec<Series> = roster::circuits(&jam)
            .into_iter()
            .filter(|(_, rounds)| !rounds.is_empty())
            .map(|(code, rounds)| Series {
                champion: roster::field(&jam, &code).first().map(|d| d.code.clone()).unwrap_or_default(),
                parts: roster::part_set(&jam, &code),
                code,
                mirrored: rounds.iter().map(|r| r.mirrored).collect(),
                rounds: rounds.into_iter().map(|r| r.folder).collect(),
            })
            .collect();
        Championship {
            unlocked: unlocked.clamp(1, series.len().max(1)),
            series,
            chosen: 0,
            run: None,
        }
    }

    /// Begins the chosen circuit, if it has been opened. Returns its first race.
    pub fn begin(&mut self) -> Option<(String, String)> {
        let series = self
            .series
            .get(self.chosen)
            .filter(|_| self.chosen < self.unlocked)?;
        self.run = Some(Run {
            series: self.chosen,
            round: 0,
            points: [0; 6],
            round_points: [0; 6],
            scored: false,
        });
        Some((series.code.clone(), series.rounds[0].clone()))
    }

    /// Whether the race in hand is run mirrored; `None` when no circuit is being raced.
    pub fn mirrored(&self) -> Option<bool> {
        let run = self.run.as_ref()?;
        Some(
            self.series
                .get(run.series)?
                .mirrored
                .get(run.round)
                .copied()
                .unwrap_or(false),
        )
    }

    /// Gives out the points for a race from the places its racers took, by grid slot.
    pub fn score(&mut self, places: &[(usize, usize)]) {
        let Some(run) = self.run.as_mut().filter(|run| !run.scored) else {
            return;
        };
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

    /// Where a slot stands on points, first being 1. Those level on points share the
    /// better place (`CircuitStandings::GetRank`).
    pub fn standing(&self, slot: usize) -> usize {
        let Some(run) = &self.run else { return 1 };
        1 + run
            .points
            .iter()
            .filter(|&&points| points > run.points[slot])
            .count()
    }

    /// Whether the race just run was the circuit's last.
    pub fn over(&self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|run| run.scored && run.round + 1 >= self.series[run.series].rounds.len())
    }

    /// Moves on after a scored race: to the next one, or, the circuit being over
    /// or the player too far behind, out of it, opening the next circuit if the
    /// player did well enough.
    pub fn advance(&mut self, player: usize) -> Step {
        if self.over() {
            let series = self.run.as_ref().map_or(0, |run| run.series);
            let place = self.standing(player);
            let opened = place <= QUALIFYING_PLACE
                && series + 1 >= self.unlocked
                && series + 1 < self.series.len();
            if opened {
                self.unlocked = series + 2;
            }
            self.run = None;
            return Step::Finished {
                series,
                place,
                opened,
            };
        }
        let Some(run) = self.run.as_mut() else {
            return Step::Out;
        };
        // Too few points to be let into the next race: the circuit ends here.
        if run.points[player] < POINTS_TO_GO_ON * (run.round as u32 + 1) {
            self.run = None;
            return Step::Out;
        }
        (run.round, run.scored) = (run.round + 1, false);
        Step::Race(self.series[run.series].rounds[run.round].clone())
    }
}

#[cfg(test)]
#[test]
fn a_circuit_is_scored_race_by_race() {
    let series = |code: &str, rounds: usize| Series {
        code: code.into(),
        champion: String::new(),
        rounds: vec!["RACE".into(); rounds],
        mirrored: vec![code == "c1"; rounds],
        parts: None,
    };
    let mut championship = Championship {
        series: vec![series("c0", 2), series("c1", 4)],
        unlocked: 1,
        chosen: 1,
        run: None,
    };
    // The second circuit isn't open yet.
    assert!(championship.begin().is_none());
    championship.chosen = 0;
    assert_eq!(championship.mirrored(), None);
    assert_eq!(championship.begin(), Some(("c0".into(), "RACE".into())));
    assert_eq!(championship.mirrored(), Some(false));
    // The player (slot 5) wins, then comes fourth.
    championship.score(&[(5, 1), (0, 2), (1, 3), (2, 4), (3, 5), (4, 6)]);
    championship.score(&[(5, 6)]);
    assert_eq!(
        championship.run.as_ref().unwrap().points,
        [20, 10, 3, 2, 1, 30]
    );
    assert!(!championship.over());
    assert_eq!(championship.advance(5), Step::Race("RACE".into()));
    championship.score(&[(0, 1), (1, 2), (2, 3), (5, 4), (3, 5), (4, 6)]);
    assert_eq!(
        championship.run.as_ref().unwrap().points,
        [50, 30, 13, 4, 2, 33]
    );
    assert_eq!(
        (
            championship.standing(0),
            championship.standing(5),
            championship.standing(1)
        ),
        (1, 2, 3)
    );
    // Second overall is enough to open the next circuit.
    assert!(championship.over());
    let finished = Step::Finished {
        series: 0,
        place: 2,
        opened: true,
    };
    assert!(championship.advance(5) == finished && championship.run.is_none());
    assert_eq!(championship.unlocked, 2);
    // Fourth in the first race of the next is under ten points: no second race.
    championship.chosen = 1;
    assert!(championship.begin().is_some());
    championship.score(&[(0, 1), (1, 1), (2, 3), (5, 4), (3, 5), (4, 6)]);
    // The two level on points are both first, and the car behind them third.
    assert_eq!(
        (
            championship.standing(0),
            championship.standing(1),
            championship.standing(2)
        ),
        (1, 1, 3)
    );
    assert!(
        !championship.over() && championship.advance(5) == Step::Out && championship.run.is_none()
    );
    assert_eq!(championship.unlocked, 2);
}
