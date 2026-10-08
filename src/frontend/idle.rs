//! The main menu left alone goes to a demo race, and a key ends it. Follows
//! `MainMenuScreen` (`m_idleTimeoutMs`, counted down in `Update`, 60 seconds from the
//! screen's `Reset`: nothing the player does on the page puts it back, only leaving
//! the page and coming to it again; a film, being a screen of its own, stops the
//! count), `MenuManager::PrepareRaceContext` (the menus ended with no racer chosen:
//! a single race on one of the four races of the first circuit, picked at random,
//! six cars) and `RaceSession` in `m_demoMode` (the first car is the computer's too,
//! the race is left by itself ten seconds after that car finishes, any key, mouse
//! button or pad button on the way ends it, and `DrawDemoText` flashes string 0x2e
//! of `GAME.SRF` over the bottom of the picture: shown for the first 750 ms of each
//! second, `c_demoTextCycleMs` and `c_overlayDrawDelayMs`).
//!
//! `BRICK_IDLE=<seconds>` is the port's own: the time the main menu waits, which also
//! lets a `BRICK_DEMO` run (that otherwise never goes to a demo race) go to one.

use bevy::input::gamepad::GamepadButton;
use bevy::prelude::*;

use super::{Menu, Page};
use crate::film::Showing;
use crate::menu::{Circuits, LAP_CHOICES, MAX_OPPONENTS, Screen, Settings};
use crate::net::Role;

/// `MainMenuScreen::Reset`: how long the main menu waits, in seconds.
pub const TIMEOUT: f32 = 60.0;
/// `RaceSession::c_demoTextCycleMs`, in seconds.
const CYCLE: f32 = 1.0;
/// `RaceSession::c_overlayDrawDelayMs`: the words are shown while more than this of
/// the cycle is left.
const SHOWN_UNTIL: f32 = 0.25;
/// The laps of the demo, as `RaceSession` sets for a race without split screen.
const LAPS: i32 = 3;

#[derive(Resource)]
pub struct Idle {
    /// Seconds before the main menu goes to a demo race.
    pub timeout: f32,
    left: f32,
    /// A `BRICK_DEMO` run is not to go to a demo race unless `BRICK_IDLE` is set.
    asked: bool,
    /// A demo race is being run, from the menu going to it until it is back.
    pub running: bool,
    /// What the player had set, put back when the demo is over.
    saved: Option<Settings>,
    /// `RaceSession::m_demoTextMs`.
    flash: f32,
    /// Which of the first circuit's races was last picked.
    picked: usize,
}

impl Idle {
    pub fn from_env() -> Self {
        let asked = std::env::var("BRICK_IDLE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok());
        Idle {
            timeout: asked.unwrap_or(TIMEOUT),
            left: asked.unwrap_or(TIMEOUT),
            asked: asked.is_some(),
            running: false,
            saved: None,
            flash: 0.0,
            picked: 0,
        }
    }

    /// `MainMenuScreen::Reset`.
    fn reset(&mut self) {
        self.left = self.timeout;
    }

    /// `MainMenuScreen::Update`: whether the time is up.
    fn tick(&mut self, elapsed: f32) -> bool {
        self.left = (self.left - elapsed).max(0.0);
        self.left == 0.0
    }

    /// `RaceSession::Update`'s count of `m_demoTextMs`.
    fn flashing(&mut self, elapsed: f32) {
        if elapsed > self.flash {
            self.flash = CYCLE;
        } else {
            self.flash -= elapsed;
        }
    }

    /// `RaceSession::DrawDemoText`: whether the words are on show.
    pub fn words_shown(&self) -> bool {
        self.running && self.flash > SHOWN_UNTIL
    }
}

pub fn running(idle: Res<Idle>) -> bool {
    idle.running
}

/// The races of the first circuit: what the demo picks from.
fn races(circuits: &Circuits) -> Vec<usize> {
    (0..circuits.0.len())
        .filter(|&i| circuits.0[i].race.is_some() && circuits.0[i].group == 0)
        .collect()
}

/// Sets the race the demo is: a single race, three laps, a full field, the computer
/// in the player's car, and none of the port's own rules.
fn arrange(settings: &mut Settings, circuit: usize) {
    settings.circuit = circuit;
    settings.test_drive = None;
    settings.championship = None;
    settings.time_race = false;
    settings.opponents = MAX_OPPONENTS;
    settings.lap_choice = LAP_CHOICES.iter().position(|&l| l == LAPS).unwrap_or(1);
    settings.racer = 0;
    settings.mirror = false;
    settings.reverse = false;
    settings.bricks = 0;
    settings.elimination = false;
}

/// Counts the main menu's time out and goes to the demo race when it is up.
pub(super) fn watch(
    mut idle: ResMut<Idle>,
    menu: Res<Menu>,
    showing: Res<Showing>,
    role: Res<Role>,
    demo: Option<Res<crate::DemoShot>>,
    real: Res<Time<Real>>,
    circuits: Res<Circuits>,
    mut settings: ResMut<Settings>,
    mut next: ResMut<NextState<Screen>>,
) {
    let waiting = menu.page == Page::Main
        && !menu.starting
        && !showing.busy()
        && *role == Role::Offline
        && (demo.is_none() || idle.asked);
    if !waiting {
        idle.reset();
        return;
    }
    if !idle.tick(real.delta_secs()) {
        return;
    }
    let races = races(&circuits);
    if races.is_empty() {
        // Without the game's races there is nothing to show.
        idle.reset();
        return;
    }
    // `PrepareRaceContext` picks one of the circuit's four at random.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() as usize);
    idle.picked = (idle.picked + 1 + seed) % races.len();
    idle.saved = Some(settings.clone());
    arrange(&mut settings, races[idle.picked]);
    idle.running = true;
    idle.flash = 0.0;
    next.set(Screen::Loading);
}

/// Back at the menu: what the player had set is theirs again, and the main menu's
/// time begins afresh.
pub(super) fn restore(mut idle: ResMut<Idle>, mut settings: ResMut<Settings>) {
    if let Some(saved) = idle.saved.take() {
        *settings = saved;
    }
    idle.running = false;
    idle.reset();
}

/// In the demo race: flashes the words, and ends it at a key, a mouse button or a
/// pad's button (`RaceSession::OnKeyDown` with `c_keySourceAbortMask`, which leaves out
/// the axes' buttons).
pub fn end_demo(
    mut idle: ResMut<Idle>,
    real: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<&Gamepad>,
    mut next: ResMut<NextState<Screen>>,
) {
    if !idle.running {
        return;
    }
    idle.flashing(real.delta_secs());
    let pad = pads.iter().any(|pad| {
        pad.get_just_pressed()
            .any(|b| !matches!(b, GamepadButton::LeftTrigger2 | GamepadButton::RightTrigger2))
    });
    if keys.get_just_pressed().next().is_some() || mouse.get_just_pressed().next().is_some() || pad
    {
        next.set(Screen::Menu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_waits_sixty_seconds() {
        let mut idle = Idle::from_env();
        if idle.asked {
            return;
        }
        assert_eq!(idle.timeout, 60.0);
        for _ in 0..59 {
            assert!(!idle.tick(1.0));
        }
        assert!(idle.tick(1.0));
        // A frame longer than what is left also ends it, and it stays ended.
        idle.reset();
        assert!(!idle.tick(59.5));
        assert!(idle.tick(5.0));
        assert!(idle.tick(0.0));
        // Leaving the page puts it back.
        idle.reset();
        assert!(!idle.tick(1.0));
    }

    #[test]
    fn the_words_flash_three_quarters_of_each_second() {
        let mut idle = Idle::from_env();
        idle.running = true;
        // The first frame starts a cycle.
        assert!(!idle.words_shown());
        idle.flashing(0.016);
        assert!(idle.words_shown());
        let mut shown = 0;
        for _ in 0..1000 {
            idle.flashing(0.001);
            shown += idle.words_shown() as u32;
        }
        assert!((730..=770).contains(&shown), "{shown}");
        idle.running = false;
        assert!(!idle.words_shown());
    }

    #[test]
    fn the_demo_is_a_single_race_with_a_full_field() {
        let circuits = Circuits::find();
        let mut settings = Settings::new(&circuits);
        settings.championship = Some("c1".into());
        settings.time_race = true;
        settings.racer = 3;
        settings.mirror = true;
        settings.elimination = true;
        settings.lap_choice = 5;
        arrange(&mut settings, 0);
        assert_eq!((settings.laps(), settings.field()), (3, 5));
        assert!(settings.championship.is_none() && !settings.time_race);
        assert_eq!(settings.racer, 0);
        assert!(!settings.mirror && !settings.elimination);
    }

    #[test]
    fn the_demo_picks_from_the_first_circuits_four_races() {
        let circuits = Circuits::find();
        let picks = races(&circuits);
        if picks.is_empty() {
            return;
        }
        assert_eq!(picks.len(), 4);
        // The circuit list's first entry is `c0`, whose races these are.
        let jam = crate::assets::jam::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM")
            .unwrap();
        let (code, rounds) = &crate::roster::circuits(&jam)[0];
        assert_eq!(code, "c0");
        for pick in picks {
            let folder = circuits.0[pick].race.as_deref().unwrap();
            assert!(rounds.iter().any(|r| r.folder == folder), "{folder}");
        }
    }

    /// The words are string 0x2e of the game's table, set in the first font of its
    /// file.
    #[test]
    fn the_words_are_demo() {
        use crate::assets::{font::load_strings, jam::Jam};
        let Some(jam) = Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else {
            return;
        };
        let strings = load_strings(jam.get("/GAMEDATA/COMMON/ENGLISH/GAME.SRF").unwrap());
        assert_eq!(strings[0x2e], "DEMO");
    }
}
