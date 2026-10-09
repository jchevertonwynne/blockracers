//! The original's build menu: the garage of racers built, and the screens a racer is
//! made on. A minifigure is put together of a hat, a face, a torso and legs; it is
//! given a name; and its car is built, a chassis and the bricks of the part sets put
//! on it one at a time. After `GarageScreen`, `EditDriverScreen`,
//! `DriverLicenseScreen`, `EditCarScreen` and `CarBuildScreen` with its
//! `CarPartPlacement`, and laid out by their `.MIB` files.
//!
//! The racer the garage shows is the one the player races as. Its test drive
//! (`GarageScreen::StartTestDrive`) is the race of the table's `test` entry, the
//! racer alone on it with no lights to wait for, and ends back in the garage. The garage can be come
//! to from a session's room, and goes back there; the racer it shows is then the one
//! raced as in the session too.
//!
//! What is not as the original has it:
//! - Bricks are placed with keys of the port's own, written beside what each
//!   works and at the end of each's help (`KEYS`): `I`, `J`, `K` and `L` move the
//!   brick, `R` turns it, `Enter` puts it on, `Backspace` takes the last off, `Tab`
//!   and `T` go on to the next brick and the next part set (back with `Shift`),
//!   the arrows turn the car round and raise and lower the view of it, and `C`
//!   puts the view back. The original moves the brick with the numeric keypad,
//!   which not every keyboard has.
//! - The car and the brick held are not lit up while the pointer is on them
//!   (`CarPartPlacement::FocusCar`, `FocusPiece`): nothing here is lit.
//! - How far the camera stands back where bricks are placed, and where on the
//!   screen the pointer finds the car, is by the car's length and width
//!   (`KartModel::outline`), not by the radius of the whole of it.

use super::{Action, Art, Item, LABEL, Menu, Page, Typed, Widget};
use crate::assets::{
    leb::{self, Library, PartSet},
    lrs::{Cosmetics, Racer},
};
use crate::audio::{Sfx, id};
use crate::build::{Car, Catalogue, Cursor, Palette, Refusal};
use super::stage::{self, Set};
use crate::garage::{self, Garage};
use crate::menu::Settings;
use crate::physics::UNIT;
use crate::progress::Progress;
use bevy::prelude::*;

// Strings of `MENUTEXT.SRF`.
mod text {
    pub const BUILD_MENU: usize = 3;
    pub const EDIT_RACER_BANNER: usize = 4;
    pub const BUILD_DRIVER: usize = 9;
    pub const MAKE_LICENSE: usize = 10;
    pub const BUILD_CAR: usize = 11;
    pub const BACK: usize = 26;
    pub const NEXT: usize = 28;
    pub const BUILD: usize = 37;
    pub const NEW_RACER: usize = 40;
    pub const EDIT_RACER: usize = 41;
    pub const COPY_RACER: usize = 42;
    pub const DELETE_RACER: usize = 44;
    pub const TEST_DRIVE: usize = 45;
    pub const MIX: usize = 56;
    pub const EXPRESSION: usize = 59;
    pub const FIRST_NAME: usize = 57;
    pub const REMOVE_BRICKS: usize = 60;
    pub const QUICK_BUILD: usize = 61;
    pub const DONE: usize = 62;
    pub const CANCEL: usize = 31;
    pub const CONTINUE: usize = 32;
    pub const YES: usize = 115;
    pub const NO: usize = 116;
    pub const DELETING: usize = 118;
    pub const ABANDONING: usize = 119;
    pub const LOSING: usize = 123;
    pub const LIMIT: usize = 186;
}

/// The pictures of the part sets, in the sets' order. The four the game begins
/// with are matched to theirs by what is in them; nothing in the data says which
/// is which.
pub const SET_PICTURES: [&str; 12] = [
    "bricks", "castle", "race", "space", "pirate", "islander", "magical", "adventur", "jungle",
    "alien", "rr", "vv",
];
/// What a new racer is called until it is called something.
const NEW_NAME: &str = "PLAYER";
/// The faces a driver can pull: the part catalogue's `dflt`, `angry`, `blink`,
/// `happy`, `sad` and `suprz`.
pub const EXPRESSIONS: u8 = 6;
/// How far over the car a brick is held, in studs (`g_carPartHoverHeight`).
const HOVER: f32 = 1.2;
/// A plate's height against a stud's width.
const PLATE: f32 = 0.4;
/// How fast the car turns on the car's page, in radians a second
/// (`EditCarScreen::CreateWidgets`, `m_spinSpeed`, which is to a millisecond).
const SPIN: f32 = 1.0;
/// Where the car stands in the set it is built in, in the game's units
/// (`m_position`, `m_piecePosition`).
const CAR_AT: Vec3 = Vec3::new(0.0, 0.0, 1.0);
/// Which way the car points when the view of it has not been turned, in radians
/// round from the game's X, and how far each turn of the view is
/// (`CarPartPlacement::CarPartPlacement`, `g_viewAngleStep`).
const VIEW_FROM: f32 = 1.57;
const VIEW_STEP: f32 = std::f32::consts::FRAC_PI_4;
/// The view a car is first seen from as bricks are placed (`SetViewSlot`).
const FIRST_VIEW: i32 = 1;
/// Where the camera is as bricks are placed, at each of the three heights the car
/// can be seen from, for the smallest car and the largest
/// (`g_carPartCameraMinPositions`, `g_carPartCameraMaxPositions`), how far across
/// those cars are (`g_carPartCameraMinDistance`, `g_carPartCameraMaxDistance`), and
/// how far over the car it looks (`ResetCamera`).
const EYES: [[Vec3; 2]; 3] = [
    [Vec3::new(0.0, -14.0, 9.0), Vec3::new(0.0, -18.0, 10.0)],
    [Vec3::new(0.0, -10.0, 14.0), Vec3::new(0.0, -12.0, 17.0)],
    [Vec3::new(0.0, -0.3, 17.0), Vec3::new(0.0, -0.3, 20.0)],
];
const REACHES: [f32; 2] = [5.9, 8.5];
const LOOK_OVER: f32 = 4.0;
/// The height a car is first seen from (`BeginViewReset`), and the highest, from
/// straight above.
const FIRST_HEIGHT: usize = 1;
const OVERHEAD: usize = 2;
/// How long the view takes to turn to the next, and to rise or sink to the next
/// height, in milliseconds (`RotateViewStep`, `PitchViewStep`).
const TURN_MS: f32 = 150.0;
const RISE_MS: f32 = 300.0;
/// Where the brick held floats, as the original counts heights: so far over where a
/// brick on the bare chassis' level would rest (`g_pieceCommitHeight`, and the 8.4
/// of `UpdatePieceBob`, which is twenty-one plates).
const HELD_AT: f32 = 1.0;
const HELD_OVER: f32 = 8.4;
/// How fast the ghost brick rises, and the brick put on falls, in units a
/// millisecond; how fast a brick taken up rises; and how long each may take
/// (`UpdatePieceBob`, `UpdateCommitFeedback`, `UpdateResetAnimation`).
const GHOST_SPEED: f32 = 0.01;
const TAKEN_SPEED: f32 = 0.006;
const GHOST_MS: f32 = 2500.0;
const TAKEN_MS: f32 = 2000.0;
/// How clear the ghost bricks are, of 255 (`CarPartPlacement::Draw`): the one that
/// moves, the one where the brick would rest, and that one's flashing where the
/// brick can't go, which is about the first by the second, once a second.
const GHOST_ALPHA: f32 = 64.0;
const REST_ALPHA: f32 = 150.0;
const FLASH_ALPHA: [f32; 2] = [100.0, 50.0];

/// The places to press on the pad that moves the brick held, each with its number
/// and where on the pad's picture it is: the eight ways of the compass, from
/// straight up and on round to the right (`procker` of `CARBUILD.MSB`).
const MOVE_SPOTS: [(i32, [f32; 4]); 8] = [
    (1, [42.0, 0.0, 72.0, 29.0]),
    (2, [73.0, 0.0, 120.0, 29.0]),
    (3, [73.0, 30.0, 120.0, 58.0]),
    (4, [73.0, 58.0, 120.0, 111.0]),
    (5, [43.0, 59.0, 72.0, 111.0]),
    (6, [0.0, 59.0, 42.0, 111.0]),
    (7, [0.0, 29.0, 42.0, 58.0]),
    (8, [0.0, 0.0, 42.0, 29.0]),
];
/// Those of the pad that changes the view (`crocker`): up, right, down and left,
/// and the middle (`CarBuildScreen::OnWidgetValueChanged`).
const VIEW_SPOTS: [(i32, [f32; 4]); 5] = [
    (VIEW_UP, [24.0, 0.0, 67.0, 24.0]),
    (VIEW_RIGHT, [67.0, 24.0, 91.0, 66.0]),
    (VIEW_DOWN, [24.0, 66.0, 67.0, 91.0]),
    (VIEW_LEFT, [0.0, 24.0, 24.0, 66.0]),
    (VIEW_HOME, [24.0, 24.0, 67.0, 66.0]),
];
const VIEW_UP: i32 = 1;
const VIEW_RIGHT: i32 = 3;
const VIEW_DOWN: i32 = 5;
const VIEW_LEFT: i32 = 7;
const VIEW_HOME: i32 = 9;

/// What the pointer can take hold of where bricks are placed.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Grip {
    Car,
    Brick,
}

/// How far the pointer pulls the brick held before it moves a stud, how long it
/// then waits before it moves again, how long a second press on it still counts
/// as a double one, in milliseconds, and how much of a view or a height the car
/// is pulled round by a pixel (`c_carBuildDragThreshold`, `c_carBuildDragDelay`,
/// `c_carBuildClickDelay`, `g_carBuildPreviewMouseScale`).
const PULL: f32 = 5.0;
const PULL_WAIT: f32 = 100.0;
const DOUBLE_CLICK: f32 = 500.0;
const SWING: f32 = 0.01;
/// The keys of the page bricks are placed on, for each of the help's strings that
/// ends by saying what the original's are: part sets, bricks, moving the brick,
/// turning it, putting it on, taking one off, and the view.
const KEYS: [&str; 7] = ["T", "TAB", "I J K L", "R", "ENTER", "BACKSPACE", "ARROWS, C"];

/// A string of the help with the port's keys where it names the original's: what
/// follows its last colon, which is where every language's says them.
pub fn help(words: &str, string: usize) -> String {
    match (KEYS.get(string), words.rfind(':')) {
        (Some(keys), Some(colon)) => format!("{} {keys}", &words[..=colon]),
        _ => words.to_string(),
    }
}

/// The help for the car and for the brick held, of `CARBUILD.SRF`.
const GRIP_HELP: [usize; 2] = [7, 8];

/// What the ghost brick is doing (`c_placementFeedbackLowering` and the rest).
#[derive(Default, Clone, Copy, PartialEq, Debug)]
enum Ghost {
    /// The brick can't go where it is held: there is no ghost that moves.
    #[default]
    Still,
    Lowering,
    Hold,
    Raising,
}

/// What moves where bricks are placed: the view of the car, turning and rising
/// from one to the next, the ghost of the brick held going down to where it would
/// rest and back, and the brick itself dropping onto the car or coming up off it.
/// After `CarPartPlacement` (`RotateViewStep`, `PitchViewStep`, `BeginViewReset`,
/// `Update` and what that calls, `CommitPiece`, `BeginResetAnimation`). Heights are
/// the original's: `HELD_AT` is where the brick held floats.
#[derive(Default)]
pub struct Motion {
    /// Which way the car points now, in radians round from the game's X, the way
    /// it pointed as it began to turn, and how long the turn has left.
    angle: f32,
    turned_from: f32,
    turning: f32,
    /// Which height the car is seen from or is on its way to, where the camera
    /// was as it set off, where it is now, and how long it has left to go.
    height: usize,
    eye_from: Vec3,
    eye: Vec3,
    rising: f32,
    /// The ghost: what it is doing, for how much longer at most, and how high it is.
    ghost: Ghost,
    ghost_ms: f32,
    ghost_at: f32,
    /// How high the brick would rest, and whether it can go there.
    rest: f32,
    fits: bool,
    /// The flashing's clock, which runs down a second at a time.
    flash: f32,
    /// How high the brick is on its way down onto the car, or up from it, and how
    /// long that may go on.
    falling: Option<f32>,
    taken: Option<f32>,
    feedback: f32,
    /// The view as the pointer has pulled it about, while it has hold of the car:
    /// how far round, in views, and how high, in heights (`m_viewAngleF`,
    /// `m_viewPitch`).
    swung: Option<f32>,
    tilted: Option<f32>,
}

impl Motion {
    /// Where the camera stands at one of the three heights, for a car this far across.
    fn eye_at(height: usize, reach: f32) -> Vec3 {
        let share = ((reach - REACHES[0]) / (REACHES[1] - REACHES[0])).clamp(0.0, 1.0);
        EYES[height][0].lerp(EYES[height][1], share)
    }

    /// The way the car points in one of the eight views of it.
    fn facing(view: i32) -> f32 {
        (VIEW_FROM + view.rem_euclid(8) as f32 * VIEW_STEP).rem_euclid(std::f32::consts::TAU)
    }

    /// As the page is come to: the view the original begins on.
    fn begin(&mut self, view: i32, reach: f32) {
        *self = Motion {
            angle: Motion::facing(view),
            height: FIRST_HEIGHT,
            eye: Motion::eye_at(FIRST_HEIGHT, reach),
            ..default()
        };
    }

    /// Whether a brick is on its way onto the car, when nothing else is to be done.
    pub fn busy(&self) -> bool {
        self.falling.is_some()
    }

    /// `RotateViewStep`: sets the car turning from one view to another. False while
    /// it is turning already.
    fn turn(&mut self, from: i32) -> bool {
        if self.turning > 0.0 {
            return false;
        }
        (self.turning, self.turned_from) = (TURN_MS, Motion::facing(from));
        true
    }

    /// `PitchViewStep`: sets the camera off for the height above or below. False
    /// while it is on its way, or with no such height.
    fn rise(&mut self, by: i32, reach: f32) -> bool {
        let to = self.height as i32 + by;
        if self.rising > 0.0 || !(0..=OVERHEAD as i32).contains(&to) {
            return false;
        }
        self.eye_from = Motion::eye_at(self.height, reach);
        (self.height, self.rising) = (to as usize, RISE_MS);
        true
    }

    /// `RotateViewAnalog`: the car pulled round by so much of a view.
    fn swing(&mut self, view: i32, by: f32) {
        if self.turning <= 0.0 && by != 0.0 {
            self.swung = Some((self.swung.unwrap_or(view as f32) + by).rem_euclid(8.0));
        }
    }

    /// `PitchViewAnalog`: the camera pulled up or down by so much of a height.
    fn tilt(&mut self, by: f32) {
        let now = self.tilted.unwrap_or(self.height as f32);
        let room = if by > 0.0 { now < OVERHEAD as f32 } else { now > 0.0 };
        if self.rising <= 0.0 && by != 0.0 && room {
            self.tilted = Some((now + by).clamp(0.0, OVERHEAD as f32));
        }
    }

    /// `SnapViewRotation`, `SnapViewPitch`: let go of, the view goes on to the
    /// nearest of the eight and the nearest of the three heights. The view it
    /// goes to.
    fn settle(&mut self, view: i32, reach: f32) -> i32 {
        let mut to = view;
        if let Some(swung) = self.swung.take() {
            let nearest = (swung + 0.5) as i32 % 8;
            if nearest != view || nearest as f32 != swung {
                (self.turning, self.turned_from, to) = (TURN_MS, self.angle, nearest);
            }
        }
        if self.tilted.take().is_some() {
            let away = |height: &usize| Motion::eye_at(*height, reach).distance(self.eye);
            let nearest = (0..=OVERHEAD).min_by(|a, b| away(a).total_cmp(&away(b))).unwrap_or(0);
            if away(&nearest) > 0.0 {
                (self.eye_from, self.rising) = (self.eye, RISE_MS);
            }
            self.height = nearest;
        }
        to
    }

    /// `CommitPiece`: the brick held sets off down onto the car.
    fn put_on(&mut self) {
        (self.falling, self.feedback) = (Some(HELD_AT), GHOST_MS);
    }

    /// `BeginResetAnimation`: the brick held comes up from where it rested.
    fn take_up(&mut self) {
        (self.taken, self.feedback) = (Some(self.rest), TAKEN_MS);
    }

    /// How far through a move with this much of its time left the thing moved is
    /// from where it will end: all the way at first, easing to nothing.
    fn eased(left: f32, whole: f32) -> f32 {
        ((left / whole).clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin()
    }

    /// A frame of `ms`: `CarPartPlacement::Update`, for a car seen in `view`, this
    /// far across, with the brick held resting on this many plates, if it can be
    /// put on at all. True as a brick put on reaches the car.
    fn update(&mut self, ms: f32, view: i32, reach: f32, plates: i32, fits: bool) -> bool {
        let moving = self.falling.is_some() || self.taken.is_some();
        // `UpdatePieceBob`.
        if !moving {
            if !fits {
                (self.ghost, self.ghost_at) = (Ghost::Still, self.rest);
            } else if self.ghost == Ghost::Still {
                (self.ghost, self.ghost_ms) = (Ghost::Lowering, GHOST_MS);
            }
            if self.ghost != Ghost::Still {
                if ms >= self.ghost_ms {
                    let over = ms - self.ghost_ms;
                    (self.ghost, self.ghost_ms) = match self.ghost {
                        Ghost::Lowering => (Ghost::Hold, 0.0),
                        Ghost::Hold => (Ghost::Raising, GHOST_MS - over),
                        _ => (Ghost::Lowering, GHOST_MS - over),
                    };
                } else {
                    self.ghost_ms -= ms;
                }
            }
            self.rest = plates as f32 * PLATE + (HELD_AT - HELD_OVER);
            match self.ghost {
                Ghost::Lowering => {
                    // Slower as it nears where it rests.
                    let near = ((self.ghost_at - self.rest).trunc() / 3.0).clamp(0.3, 1.0);
                    self.ghost_at -= near * GHOST_SPEED * ms;
                    if self.ghost_at <= self.rest {
                        (self.ghost_ms, self.ghost_at) = (0.0, self.rest);
                    }
                }
                Ghost::Raising => {
                    self.ghost_at += ms * GHOST_SPEED;
                    let top = HELD_AT - HOVER * 2.0;
                    if self.ghost_at >= top {
                        (self.ghost_ms, self.ghost_at) = (0.0, top);
                    }
                }
                Ghost::Hold => {}
                Ghost::Still => (self.ghost_ms, self.ghost_at) = (GHOST_MS, self.rest),
            }
        }
        self.fits = fits;
        self.flash = if ms >= self.flash { self.flash - ms + 1000.0 } else { self.flash - ms };
        // `UpdateViewRotation`: the nearer way round.
        if self.turning > 0.0 {
            self.turning = (self.turning - ms).max(0.0);
            let (from, mut to) = (self.turned_from, Motion::facing(view));
            if to > from + std::f32::consts::FRAC_PI_2 {
                to -= std::f32::consts::TAU;
            } else if to < from - std::f32::consts::FRAC_PI_2 {
                to += std::f32::consts::TAU;
            }
            self.angle = to + (from - to) * Motion::eased(self.turning, TURN_MS);
        } else if let Some(swung) = self.swung {
            self.angle = Motion::facing(swung as i32) + swung.fract() * VIEW_STEP;
        } else {
            self.angle = Motion::facing(view);
        }
        // `UpdateViewPitch`.
        let to = Motion::eye_at(self.height, reach);
        self.rising = (self.rising - ms).max(0.0);
        self.eye = match self.tilted {
            Some(tilted) => {
                let under = (tilted as usize).min(OVERHEAD - 1);
                let (low, high) = (Motion::eye_at(under, reach), Motion::eye_at(under + 1, reach));
                low.lerp(high, tilted - under as f32)
            }
            None => to + (self.eye_from - to) * Motion::eased(self.rising, RISE_MS),
        };
        // `UpdateCommitFeedback`: it is on the car the frame after it is down.
        let mut landed = false;
        if let Some(at) = self.falling {
            if ms >= self.feedback {
                (self.falling, self.feedback, landed) = (None, 0.0, true);
            } else {
                self.feedback -= ms;
                let at = at - ms * GHOST_SPEED;
                if at < self.rest {
                    self.feedback = 0.0;
                }
                self.falling = Some(at.max(self.rest));
            }
        }
        // `UpdateResetAnimation`.
        if let Some(at) = self.taken {
            if ms >= self.feedback {
                (self.taken, self.feedback) = (None, 0.0);
            } else {
                self.feedback -= ms;
                let at = at + ms * TAKEN_SPEED;
                if at > HELD_AT {
                    self.feedback = 0.0;
                }
                self.taken = Some(at.min(HELD_AT));
            }
        }
        landed
    }

    /// `CarPartPlacement::Draw`: how high each of the three bricks drawn is and how
    /// clear, of one: the brick held, the ghost that moves and the ghost where the
    /// brick would rest. Nothing for one that is not drawn.
    fn drawn(&self) -> [Option<(f32, f32)>; 3] {
        let moving = self.falling.or(self.taken);
        let overhead = self.height == OVERHEAD;
        let held = (moving.is_some() || !overhead).then(|| (moving.unwrap_or(HELD_AT), 1.0));
        if moving.is_some() {
            return [held, None, None];
        }
        let ghost = (self.ghost != Ghost::Still && !overhead)
            .then_some((self.ghost_at, GHOST_ALPHA / 255.0));
        let rest = if self.ghost != Ghost::Still {
            // It fades as the ghost that moves comes down onto it.
            let over = self.ghost_at - self.rest - HOVER;
            if over < HOVER && !overhead {
                (over / HOVER).max(0.0) * REST_ALPHA
            } else {
                REST_ALPHA
            }
        } else {
            let turn = self.flash * 0.001 * std::f32::consts::TAU;
            FLASH_ALPHA[0] + FLASH_ALPHA[1] * turn.cos()
        };
        [held, ghost, (rest >= 1.0).then_some((self.rest, rest / 255.0))]
    }
}
/// What is behind a racer on show.
const WALL: Color = Color::srgb_u8(6, 6, 70);

/// What the garage's screens can do.
#[derive(Clone, Copy, PartialEq)]
pub enum Act {
    /// A place pressed on the pad that moves the brick held, or on the one that
    /// changes the view of the car: the action's change is the place's number.
    Nudge,
    View,
    /// Which racer the garage shows, and the player races as.
    Pick,
    New,
    Edit,
    Copy,
    /// Asks before a racer is deleted; `Scrap` is the answer.
    Delete,
    /// The answer to the question the page of `Page::Scrap` asks.
    Scrap(bool),
    /// The test drive.
    Drive,
    /// One of a minifigure's four parts.
    Part(usize),
    Mix,
    /// The face the driver pulls for the photograph on the licence.
    Look,
    /// On to the next screen of a new racer, or done with the one being changed.
    Next,
    /// The way back from a screen a racer is made on.
    GiveUp,
    /// The chassis of the car, and the bricks that come with it.
    Chassis,
    Strip,
    Quick,
    /// On the screen bricks are placed on: which part set, which brick of it, and
    /// what is done with the brick held.
    Set,
    Brick,
    Turn,
    Add,
    Undo,
}

/// What the question page (`Page::Scrap`) asks, which the original asks with
/// `MenuScreen::ShowConfirmDialog`: the answer is "no" unless it is chosen otherwise.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum Ask {
    /// Whether to delete the racer on show.
    #[default]
    Delete,
    /// Whether to go back to a page, giving up what was changed on this one.
    Lose(Page),
    /// Whether to give up making a new racer.
    Abandon,
    /// Whether to replace a car that has been built on with one the game handed out.
    Quick,
    /// Whether to take a car that has been built on apart.
    Strip,
}

/// The game's data a racer is built from.
struct Kit {
    library: Library,
    sets: Vec<PartSet>,
    catalogue: Catalogue,
    /// The racers the quick build hands out.
    stock: Vec<Racer>,
}

/// The racer being made or changed, and how far its making has got.
#[derive(Resource, Default)]
pub struct Bench {
    kit: Option<Kit>,
    racer: Racer,
    /// Which of the garage's racers it is; none for a new one.
    slot: Option<usize>,
    car: Car,
    /// The part set bricks are being taken from, and which of its bricks is held.
    set: usize,
    brick: usize,
    cursor: Cursor,
    /// How far round the car is turned on the screen bricks are placed on, in
    /// eighths of a turn.
    view: i32,
    /// Which of the game's racers the quick build gave last.
    quick: usize,
    /// What the question page asks, and the page it was asked from, which "no" and
    /// the way back return to.
    ask: Ask,
    from: Page,
    /// The test drive has been chosen.
    pub drive: bool,
    /// Something to tell the player, until they do something else.
    said: Option<String>,
    /// What is on show is no longer what it should be.
    stale: bool,
    /// The name left on the licence as the page was done with: it may be a cheat code
    /// (`crate::cheats`), which is taken from here.
    code: Option<String>,
    /// The help the bricks page shows for what the pointer rests on.
    pub tip: Tip,
    /// How far across the car on show is, from its middle to a corner, in the
    /// game's units.
    reach: f32,
    /// What is moving where bricks are placed.
    pub motion: Motion,
    /// Where the car and the brick held are on the menu's screen, for the pointer
    /// to take hold of (`CarBuildScreen::UpdateHoverRegions`), which of them it
    /// has hold of, how far it has pulled the brick since the brick last moved,
    /// how long until the brick may move again, and how long a second press on
    /// the brick still puts it on, in milliseconds.
    grips: Option<[Rect; 2]>,
    grip: Option<Grip>,
    pulled: Vec2,
    pull_wait: f32,
    again: f32,
    /// The place of a pad that is being pressed: whether the pad is the view's,
    /// and the place's number.
    pub lit: Option<(bool, i32)>,
    /// The driver's page has been done with, and is waiting for the move its driver
    /// goes out on: the page to go to after, how long the move has left in
    /// milliseconds, and which of the two moves it is.
    parting: Option<(Page, f32, usize)>,
}

/// How long the pointer rests on something before its help is shown, how soon after
/// one help has gone the next comes, and how long a help stays, in milliseconds
/// (`CarBuildScreenBase::Update`).
const TIP_WAIT: f32 = 3000.0;
const TIP_SOON: f32 = 250.0;
const TIP_STAYS: f32 = 20000.0;
/// The font help is written in (`g_carBuildHelpFontName`), and the colours of its
/// box and of the border four pixels wide round it (`CarBuildScreenBase::Draw`).
pub const TIP_FONT: &str = "font_hlp";
pub const TIP_FILL: Color = Color::srgb(0.0, 0.0, 0.22);
pub const TIP_EDGE: Color = Color::srgb(0.125, 0.11, 0.878);
pub const TIP_BORDER: f32 = 4.0;

/// The help of the page bricks are placed on: words about whatever the pointer has
/// rested on for three seconds, shown beside it until the pointer leaves or twenty
/// seconds are up. After `CarBuildScreenBase` (`ShowTooltip`, `HideTooltip`,
/// `SuppressTooltip`, `Update`).
#[derive(Default)]
pub struct Tip {
    /// What the pointer is on: which of the help's strings is its, and where it is.
    over: Option<(usize, Rect)>,
    rested: f32,
    /// How long the help has been shown; nothing while it isn't, and less than
    /// nothing once it has been shown its time and is not to come back.
    shown: f32,
    /// How long ago a help that was being read went, while the next would come at once.
    soon: f32,
    read: bool,
}

impl Tip {
    /// The string of `CARBUILD.SRF` that is the help for one of the page's things
    /// (the help numbers of `CARBUILD.MIB`, through `g_carBuildTextIds`).
    pub(super) fn of(action: &Action) -> Option<usize> {
        match action {
            Action::Bench(Act::Set) => Some(0),
            Action::Bench(Act::Brick) => Some(1),
            Action::Bench(Act::Nudge) => Some(2),
            Action::Bench(Act::View) => Some(6),
            Action::Bench(Act::Turn) => Some(3),
            Action::Bench(Act::Add) => Some(4),
            Action::Bench(Act::Undo) => Some(5),
            _ => None,
        }
    }

    fn hide(&mut self) {
        (self.over, self.rested, self.shown) = (None, 0.0, 0.0);
        self.soon = if self.read { 1.0 } else { 0.0 };
    }

    /// A frame of `ms` with the pointer on `over`. Whether what is shown changed.
    pub fn update(&mut self, over: Option<(usize, Rect)>, ms: f32) -> bool {
        let before = self.showing();
        if self.soon > 0.0 {
            self.soon += ms;
            if self.soon > TIP_SOON {
                self.soon = 0.0;
            }
        }
        if self.over.map(|over| over.0) != over.map(|over| over.0) {
            if self.shown != 0.0 || self.over.is_some() {
                self.hide();
            }
            self.over = over;
        }
        if self.over.is_some() {
            if self.shown == 0.0 {
                self.rested += ms;
                if self.rested >= TIP_WAIT || self.soon > 0.0 {
                    (self.rested, self.shown, self.soon, self.read) = (TIP_WAIT, 1.0, 0.0, true);
                }
            } else if self.shown > 0.0 {
                self.shown += ms;
                if self.shown > TIP_STAYS {
                    (self.shown, self.rested, self.soon, self.read) = (-1.0, 0.0, 0.0, false);
                }
            }
        }
        before != self.showing()
    }

    /// The help to show, and what it is about.
    pub fn showing(&self) -> Option<(usize, Rect)> {
        self.over.filter(|_| self.shown > 0.0)
    }

    /// Where a help of this size goes on a screen, for the thing it is about: to
    /// the right of it if there is room, or else under it, over it or to the left
    /// of it, and failing all of those in the middle. `line` is how tall the font is.
    pub fn place(target: Rect, size: Vec2, line: f32, screen: Rect) -> Vec2 {
        let beside = |from: f32, to: f32, length: f32, low: f32, high: f32| {
            ((from + to - length) / 2.0).floor().clamp(low + line, (high - length - line).max(low + line))
        };
        let down = |x: f32| Vec2::new(x, beside(target.min.y, target.max.y, size.y, screen.min.y, screen.max.y));
        let along = |y: f32| Vec2::new(beside(target.min.x, target.max.x, size.x, screen.min.x, screen.max.x), y);
        if size.x + line * 2.0 < screen.max.x - target.max.x {
            down(target.max.x + line)
        } else if size.y + line * 2.0 < screen.max.y - target.max.y {
            along(target.max.y + line)
        } else if size.y + line * 2.0 < target.min.y - screen.min.y {
            along(target.min.y - size.y - line)
        } else if size.x + line * 2.0 < target.min.x - screen.min.x {
            down(target.min.x - size.x - line)
        } else {
            (screen.min + (screen.size() - size) / 2.0).floor()
        }
    }

    /// How wide a help's lines may be: so that it comes out about three times as
    /// wide as it is tall, and between an eighth of the screen and all of it.
    /// `long` is how wide the words are on one line.
    pub fn wrap(long: f32, line: f32, screen: Rect) -> f32 {
        let most = screen.width() - line * 2.0;
        ((line + 1.0) * 3.0 * long).sqrt().floor().min(most).max((most / 8.0).floor())
    }
}

impl Bench {
    fn kit(&mut self, art: &Art) -> Option<&Kit> {
        if self.kit.is_none() {
            let library = Library::open(&art.jam, true)?;
            self.kit = Some(Kit {
                sets: leb::part_sets(&art.jam, &library),
                catalogue: Catalogue::open(&art.jam)?,
                stock: garage::stock(&art.jam),
                library,
            });
        }
        self.kit.as_ref()
    }

    /// Which leaving move the driver is making, while the driver's page waits for it.
    pub fn parting(&self) -> Option<usize> {
        self.parting.map(|parting| parting.2)
    }

    /// What the minifigure on the bench is made of, and the face it pulls.
    pub fn cosmetics(&self) -> Cosmetics {
        self.racer.cosmetics
    }

    /// The name that was on the licence when it was done with, once.
    pub fn take_code(&mut self) -> Option<String> {
        self.code.take()
    }

    /// The racer as it stands, with the car as it has been built so far.
    fn shown(&self) -> Racer {
        let chassis = self
            .kit
            .as_ref()
            .and_then(|kit| self.car.chassis(&kit.library));
        Racer {
            chassis: chassis.map_or(self.racer.chassis.clone(), str::to_string),
            car: self.car.write(),
            ..self.racer.clone()
        }
    }

    /// Takes a racer of the garage's onto the bench, or a new one.
    fn take(&mut self, art: &Art, garage: &Garage, slot: Option<usize>) -> Option<()> {
        self.kit(art)?;
        let kit = self.kit.as_ref()?;
        self.slot = slot.filter(|&slot| slot < garage.racers.len());
        match self.slot {
            Some(slot) => {
                self.racer = garage.racers[slot].clone();
                self.car = Car::read(&kit.library, &self.racer.car);
            }
            None => {
                self.racer = Racer {
                    name: NEW_NAME.into(),
                    stock: true,
                    ..default()
                };
                self.car = Car::new(&kit.library, &kit.sets.first()?.chassis);
            }
        }
        let chassis = self.car.chassis(&kit.library).unwrap_or_default();
        self.set = kit
            .sets
            .iter()
            .position(|set| set.chassis == chassis)
            .unwrap_or(0);
        (self.brick, self.view, self.cursor) = (0, 0, Cursor::default());
        self.hold();
        self.stale = true;
        Some(())
    }

    /// Takes up the brick chosen: `SelectPieceChoice`.
    fn hold(&mut self) {
        let Some(kit) = &self.kit else { return };
        let Some(set) = kit.sets.get(self.set) else {
            return;
        };
        let Some(&(kind, colour)) = set.choices.get(self.brick) else {
            return;
        };
        if let Some(piece) = kit.library.piece(kind) {
            self.cursor.hold(piece, colour, set.kind);
        }
    }

    /// Puts the racer in the garage as it now is, and has the player race as it.
    fn save(&mut self, garage: &mut Garage, settings: &mut Settings) {
        let racer = self.shown();
        let slot = match self.slot {
            Some(slot) if slot < garage.racers.len() => {
                garage.racers[slot] = racer;
                slot
            }
            _ => {
                garage.racers.push(racer);
                garage.racers.len() - 1
            }
        };
        (self.slot, settings.racer) = (Some(slot), slot + 1);
        garage.keep();
    }
}

fn button(art: &Art, screen: &str, name: &str, label: usize, act: Act) -> Item {
    Item {
        widget: Widget::Button {
            at: art.place(screen, name).min,
            label: art.string(label),
            icon: None,
        },
        action: Action::Bench(act),
        enabled: true,
    }
}

/// The widgets of one of the garage's pages.
/// The trophy the racer on the bench has for a circuit: one for first place to
/// three for third, and nought for none.
pub fn trophy(bench: &Bench, circuit: usize) -> usize {
    bench.racer.trophy(circuit) as usize
}

/// The part sets there are to build with, by their places among the sets: those
/// every game begins with, those won, and any the car has a piece of already
/// (`CarModelScreenBase::PopulateCategoryCarousel`).
/// The bricks of the part set on offer, which of them is held, and the library they are
/// from: what the carousel of bricks shows (`CarPartCarousel`).
pub fn bricks(bench: &Bench) -> Option<(&Library, &[(u16, u8)], usize)> {
    let kit = bench.kit.as_ref()?;
    let set = kit.sets.get(bench.set)?;
    Some((&kit.library, &set.choices, bench.brick))
}

/// The set a page of the build menu shows, and the racer `stage` stands on it:
/// the garage's showcase and the driver's platform have theirs put there, and the
/// set a car is built in has the car `show` makes.
pub fn staged(
    page: Page,
    bench: &Bench,
    garage: &Garage,
    settings: &Settings,
) -> Option<(Option<Racer>, Set)> {
    let from = if page == Page::Scrap && bench.ask != Ask::Delete { bench.from } else { page };
    match from {
        Page::Garage | Page::Scrap => Some((Some(garage.racing(settings)?.clone()), Set::Showcase)),
        Page::Racer => Some((Some(bench.shown()), Set::Showcase)),
        Page::Driver => Some((Some(bench.shown()), Set::Platform)),
        Page::Car => Some((None, Set::Bay)),
        Page::Bricks => Some((None, Set::Bench)),
        _ => None,
    }
}

fn open_sets(bench: &Bench, progress: &Progress) -> Vec<usize> {
    let Some(kit) = &bench.kit else {
        return Vec::new();
    };
    let used = |kind: u16| {
        let mut pieces = bench.car.pieces.iter();
        pieces.any(|piece| piece.set == kind || piece.kind == kind)
    };
    (0..kit.sets.len())
        .filter(|&set| progress.set_open(set) || used(kit.sets[set].kind))
        .collect()
}

/// A place of the licence page's layout, which are all told from the corner of the
/// licence itself.
pub fn on_licence(art: &Art, name: &str) -> Rect {
    let card = art.place("drvrlice", "license").min;
    let place = art.place("drvrlice", name);
    Rect::from_corners(place.min + card, place.max + card)
}

/// The choices there are of a part of the minifigure: those nothing has to be won
/// for, those won, and the one the racer wore when it was last kept
/// (`MenuRacerCarousel::CollectHats` and the rest).
fn open_parts(bench: &Bench, garage: &Garage, progress: &Progress, part: usize) -> Vec<u8> {
    let Some(kit) = &bench.kit else {
        return Vec::new();
    };
    let kept = bench
        .slot
        .and_then(|slot| garage.racers.get(slot))
        .map(|racer| worn(racer.cosmetics)[part]);
    (0..kit.catalogue.count(part) as u8)
        .filter(|&at| {
            kept == Some(at) || progress.part_open(kit.catalogue.mark(part, at as usize))
        })
        .collect()
}

/// What each of the driver page's four selectors shows: the minifigure on the
/// bench, and the choices of each part with the one worn among them. The port's
/// own pictures of parts (`parts`) are made from this.
pub fn part_choices(
    bench: &Bench,
    garage: &Garage,
    progress: &Progress,
) -> (Cosmetics, [(Vec<u8>, usize); 4]) {
    let cosmetics = bench.racer.cosmetics;
    let choices = [0, 1, 2, 3].map(|part| {
        let open = open_parts(bench, garage, progress, part);
        let at = open
            .iter()
            .position(|at| *at == worn(cosmetics)[part])
            .unwrap_or(0);
        (open, at)
    });
    (cosmetics, choices)
}

/// A minifigure's parts in the order the builder has them.
fn worn(cosmetics: Cosmetics) -> [u8; 4] {
    [
        cosmetics.hat,
        cosmetics.face,
        cosmetics.torso,
        cosmetics.legs,
    ]
}

/// The choice `change` on from this one, round those there are.
fn turned<T: Copy + PartialEq>(open: &[T], now: T, change: i32) -> Option<T> {
    let at = open.iter().position(|choice| *choice == now).unwrap_or(0) as i32;
    open.get((at + change).rem_euclid(open.len().max(1) as i32) as usize)
        .copied()
}

pub fn items(
    page: Page,
    art: &Art,
    bench: &Bench,
    garage: &Garage,
    settings: &Settings,
    progress: &Progress,
    online: bool,
) -> Vec<Item> {
    // What is open of the parts is shown by `parts`, not counted here.
    let _ = progress;
    let selector = |area: Rect, picture: Option<String>, words: String, act: Act| Item {
        widget: Widget::Selector {
            area,
            picture,
            words,
        },
        action: Action::Bench(act),
        enabled: true,
    };
    let leave = |screen: &str, to: Page| Item {
        widget: Widget::Button {
            at: art.place(screen, "goback").min,
            label: art.string(text::BACK),
            icon: Some("txtarol"),
        },
        action: Action::Go(to),
        enabled: true,
    };
    let give_up = |screen: &str| Item {
        action: Action::Bench(Act::GiveUp),
        ..leave(screen, Page::Garage)
    };
    // A new racer goes on to the next screen; one being changed is done with.
    let next = |screen: &str, last: bool| {
        let label = if bench.slot.is_some() || last {
            text::DONE
        } else {
            text::NEXT
        };
        button(art, screen, "gonext", label, Act::Next)
    };
    let some = !garage.racers.is_empty();
    let only_if = |mut item: Item, enabled: bool| {
        item.enabled = enabled;
        item
    };
    match page {
        Page::Garage => {
            let name = garage
                .racing(settings)
                .map_or(String::new(), |racer| racer.name.clone());
            let room = garage.racers.len() < garage::MOST;
            vec![
                only_if(
                    selector(art.place("garage", "rcont"), None, name, Act::Pick),
                    garage.racers.len() > 1,
                ),
                only_if(
                    button(art, "garage", "newracer", text::NEW_RACER, Act::New),
                    room,
                ),
                only_if(
                    button(art, "garage", "editracr", text::EDIT_RACER, Act::Edit),
                    some,
                ),
                only_if(
                    button(art, "garage", "copyracr", text::COPY_RACER, Act::Copy),
                    some && room,
                ),
                only_if(
                    button(art, "garage", "delracer", text::DELETE_RACER, Act::Delete),
                    some,
                ),
                only_if(
                    button(art, "garage", "testtrck", text::TEST_DRIVE, Act::Drive),
                    some,
                ),
                Item {
                    widget: Widget::Button {
                        at: art.place("garage", "goback").min,
                        // In a session the garage was come to from its room.
                        label: art.string(if online {
                            text::BACK
                        } else {
                            super::text::MAIN_MENU
                        }),
                        icon: Some("txtarol"),
                    },
                    action: Action::Go(if online { Page::Room } else { Page::Main }),
                    enabled: true,
                },
            ]
        }
        Page::Scrap => {
            // The two kinds of question have their own words for the answers.
            let (yes, no) = match bench.ask {
                Ask::Delete | Ask::Abandon => (text::YES, text::NO),
                _ => (text::CONTINUE, text::CANCEL),
            };
            vec![
                button(art, "garage", "picka", yes, Act::Scrap(true)),
                button(art, "garage", "pickb", no, Act::Scrap(false)),
            ]
        }
        Page::Racer => vec![
            Item {
                action: Action::Go(Page::Driver),
                ..button(art, "garage", "editdrvr", text::BUILD_DRIVER, Act::Edit)
            },
            Item {
                action: Action::Go(Page::Licence),
                ..button(art, "garage", "editlice", text::MAKE_LICENSE, Act::Edit)
            },
            Item {
                action: Action::Go(Page::Car),
                ..button(art, "garage", "editcar", text::BUILD_CAR, Act::Edit)
            },
            leave("garage", Page::Garage),
        ],
        Page::Driver => {
            let mut items: Vec<Item> = ["hatsel", "facesel", "torsosel", "legsel"]
                .iter()
                .zip(worn(bench.racer.cosmetics))
                .enumerate()
                .map(|(part, (name, chosen))| {
                    let place = art.place("editdrvr", name);
                    // The arrows are at the selector's ends, with its carousel between.
                    let middle = place.min.y + 32.0;
                    let area = Rect::new(place.min.x, middle - 16.0, place.max.x, middle + 16.0);
                    let _ = (part, chosen);
                    selector(
                        area,
                        None,
                        // The part itself is shown in the selector (`parts`).
                        String::new(),
                        Act::Part(part),
                    )
                })
                .collect();
            items.push(button(art, "editdrvr", "mix", text::MIX, Act::Mix));
            items.push(next("editdrvr", false));
            items.push(give_up("editdrvr"));
            items
        }
        Page::Licence => {
            let place = on_licence(art, "ftext");
            vec![
                Item {
                    widget: Widget::Field {
                        area: place,
                        words: bench.racer.name.clone(),
                    },
                    action: Action::Type(Typed::Racer),
                    enabled: true,
                },
                button(art, "drvrlice", "swapface", text::EXPRESSION, Act::Look),
                only_if(next("drvrlice", false), !bench.racer.name.trim().is_empty()),
                give_up("drvrlice"),
            ]
        }
        Page::Car => {
            let bare = bench.car.pieces.len() <= 1;
            vec![
                // Changing the chassis takes the car apart, so a car that has been
                // worked on keeps the one it has.
                only_if(
                    selector(
                        art.place("editcar", "selector"),
                        SET_PICTURES.get(bench.set).map(|name| name.to_string()),
                        String::new(),
                        Act::Chassis,
                    ),
                    bare || bench.racer.stock,
                ),
                Item {
                    action: Action::Go(Page::Bricks),
                    ..button(art, "editcar", "build", text::BUILD, Act::Edit)
                },
                only_if(
                    button(art, "editcar", "ripapart", text::REMOVE_BRICKS, Act::Strip),
                    !bare,
                ),
                button(art, "editcar", "qbuild", text::QUICK_BUILD, Act::Quick),
                next("editcar", true),
                give_up("editcar"),
            ]
        }
        Page::Bricks => {
            let icon = |name: &str, picture: &'static str, act: Act| Item {
                widget: Widget::Button {
                    at: art.place("carbuild", name).min,
                    label: String::new(),
                    icon: Some(picture),
                },
                action: Action::Bench(act),
                enabled: true,
            };
            // `piecesel`'s arrows are at either end of the row of bricks, `pieces`.
            let row = art.place("carbuild", "pieces");
            vec![
                selector(
                    art.place("carbuild", "sets"),
                    SET_PICTURES.get(bench.set).map(|name| name.to_string()),
                    String::new(),
                    Act::Set,
                ),
                selector(
                    Rect::new(row.min.x, row.min.y + 11.0, row.max.x, row.min.y + 43.0),
                    None,
                    String::new(),
                    Act::Brick,
                ),
                Item {
                    widget: Widget::Pad {
                        at: art.place("carbuild", "procker").min,
                        size: Vec2::new(120.0, 112.0),
                        picture: "mtu",
                        lit: "mta",
                        spots: &MOVE_SPOTS,
                    },
                    action: Action::Bench(Act::Nudge),
                    enabled: true,
                },
                Item {
                    widget: Widget::Pad {
                        at: art.place("carbuild", "crocker").min,
                        size: Vec2::new(92.0, 92.0),
                        picture: "camerau",
                        lit: "cameraa",
                        spots: &VIEW_SPOTS,
                    },
                    action: Action::Bench(Act::View),
                    enabled: true,
                },
                icon("rotbrik", "rotateu", Act::Turn),
                icon("addbrik", "downu", Act::Add),
                icon("undobrik", "upu", Act::Undo),
                Item {
                    widget: Widget::Button {
                        at: art.place("carbuild", "goback").min,
                        label: String::new(),
                        icon: Some("exitu"),
                    },
                    action: Action::Go(Page::Car),
                    enabled: true,
                },
            ]
        }
        _ => Vec::new(),
    }
}

/// What the garage's pages say around their widgets.
pub fn notes(
    page: Page,
    art: &Art,
    bench: &Bench,
    garage: &Garage,
) -> Vec<(Rect, String, &'static str, Color, bool)> {
    let banner = |words: String| {
        (
            Rect::new(375.0, 20.0, 375.0, 68.0),
            words,
            "fontmenu",
            LABEL,
            true,
        )
    };
    let line = |top: f32, words: String| {
        (
            Rect::new(320.0, top, 320.0, top + 24.0),
            words,
            "font_ths",
            LABEL,
            true,
        )
    };
    let mut notes = match page {
        Page::Garage => {
            let mut notes = vec![banner(art.string(text::BUILD_MENU))];
            if garage.racers.is_empty() {
                notes.push(line(250.0, "NO RACERS YET".into()));
            }
            notes
        }
        Page::Scrap => vec![
            banner(art.string(text::BUILD_MENU)),
            line(
                92.0,
                art.string(match bench.ask {
                    Ask::Delete => text::DELETING,
                    Ask::Abandon => text::ABANDONING,
                    _ => text::LOSING,
                }),
            ),
        ],
        Page::Racer => vec![
            banner(art.string(text::EDIT_RACER_BANNER)),
            line(92.0, bench.racer.name.clone()),
        ],
        Page::Driver => {
            vec![banner(art.string(text::BUILD_DRIVER))]
        }
        Page::Licence => {
            let place = on_licence(art, "ftext");
            vec![
                banner(art.string(text::MAKE_LICENSE)),
                (
                    Rect::new(
                        place.min.x,
                        place.min.y - 36.0,
                        place.max.x,
                        place.min.y - 4.0,
                    ),
                    art.string(text::FIRST_NAME),
                    "font_ths",
                    LABEL,
                    false,
                ),
            ]
        }
        Page::Car => vec![banner(art.string(text::BUILD_CAR))],
        Page::Bricks => {
            // The keys, which are the port's own, beside what each works; and which
            // brick of the set is held.
            let small = |area: Rect, words: &str, centred: bool| {
                (area, words.to_string(), TIP_FONT, LABEL, centred)
            };
            // Its picture is an arrow and a brick, twice as wide as it is tall.
            let beside = |name: &str, words: &str| {
                let icon = art.place("carbuild", name).min;
                small(Rect::new(icon.x + 72.0, icon.y, 205.0, icon.y + 32.0), words, false)
            };
            let held = bench.kit.as_ref().and_then(|kit| {
                let set = kit.sets.get(bench.set)?;
                Some(format!("{} OF {}", bench.brick + 1, set.choices.len()))
            });
            let (moves, views) = (art.place("carbuild", "procker").min, art.place("carbuild", "crocker").min);
            let door = art.place("carbuild", "goback").min;
            // In lines down the right of something.
            let down = |from: Vec2, row: f32, words: &str| {
                let top = from.y + row * 14.0;
                small(Rect::new(from.x, top, 205.0, top + 14.0), words, false)
            };
            let by_view = views + Vec2::new(98.0, 32.0);
            let by_door = door + Vec2::new(70.0, 6.0);
            vec![
                small(Rect::new(8.0, moves.y - 14.0, 200.0, moves.y), KEYS[2], true),
                beside("rotbrik", KEYS[3]),
                beside("addbrik", KEYS[4]),
                beside("undobrik", KEYS[5]),
                down(by_view, 0.0, "ARROWS"),
                down(by_view, 1.0, "C"),
                down(by_door, 0.0, &format!("{}: BRICK", KEYS[1])),
                down(by_door, 1.0, &format!("{}: SET", KEYS[0])),
                down(by_door, 2.0, &held.unwrap_or_default()),
            ]
        }
        _ => Vec::new(),
    };
    if let Some(said) = &bench.said {
        notes.push(line(440.0, said.clone()));
    }
    notes
}

/// Where a page's way back with `Escape` leads, and whether what was changed on it
/// is given up on the way.
pub fn back(page: Page, bench: &Bench) -> Option<(Page, bool)> {
    let new = bench.slot.is_none();
    Some(match page {
        Page::Garage => (Page::Main, false),
        Page::Scrap => (bench.from, false),
        Page::Racer => (Page::Garage, false),
        Page::Driver if new => (Page::Garage, false),
        Page::Licence if new => (Page::Driver, false),
        Page::Car if new => (Page::Licence, false),
        Page::Driver | Page::Licence | Page::Car => (Page::Racer, true),
        Page::Bricks => (Page::Car, false),
        _ => return None,
    })
}

/// The way back with `Escape`, taken: what was changed on the page is given up if
/// its way back gives it up.
///
/// `EditDriverScreen`, `DriverLicenseScreen` and `EditCarScreen` ask first when there
/// is something to lose (`HasUnsavedChanges`), and the driver's screen of a new racer
/// whether to give up making it.
pub fn escape(page: Page, art: &Art, bench: &mut Bench, garage: &Garage) -> Option<Page> {
    let (to, given_up) = back(page, bench)?;
    if page == Page::Driver && bench.slot.is_none() {
        return Some(ask(bench, page, Ask::Abandon));
    }
    if given_up {
        if changed(page, bench, garage) {
            return Some(ask(bench, page, Ask::Lose(to)));
        }
        bench.take(art, garage, bench.slot)?;
    }
    Some(to)
}

/// Puts a question to the player: the page that asks it.
fn ask(bench: &mut Bench, from: Page, question: Ask) -> Page {
    (bench.ask, bench.from) = (question, from);
    Page::Scrap
}

/// Whether what is on a page of a racer already in the garage has been changed from
/// what is kept (`HasUnsavedChanges` of each screen).
fn changed(page: Page, bench: &Bench, garage: &Garage) -> bool {
    let Some(kept) = bench.slot.and_then(|slot| garage.racers.get(slot)) else {
        return false;
    };
    let (now, then) = (&bench.racer.cosmetics, &kept.cosmetics);
    match page {
        Page::Driver => {
            (now.hat, now.face, now.torso, now.legs) != (then.hat, then.face, then.torso, then.legs)
        }
        Page::Licence => bench.racer.name != kept.name || now.expression != then.expression,
        Page::Car => {
            let shown = bench.shown();
            shown.car != kept.car || shown.chassis != kept.chassis
        }
        _ => false,
    }
}

/// Whether a page is one of the garage's.
pub fn mine(page: Page) -> bool {
    matches!(
        page,
        Page::Garage
            | Page::Scrap
            | Page::Racer
            | Page::Driver
            | Page::Licence
            | Page::Car
            | Page::Bricks
    )
}

/// The name being typed on the licence.
pub fn name(bench: &mut Bench) -> &mut String {
    bench.stale = true;
    &mut bench.racer.name
}

/// A page of the garage's is come to: the bench is made ready for it. For the pages
/// a racer is made on reached with nothing on the bench (a demo's), the garage's
/// first racer is taken up.
pub fn arrive(page: Page, art: &Art, bench: &mut Bench, garage: &Garage, settings: &mut Settings) {
    bench.said = None;
    bench.stale = true;
    // With racers in the garage, one of them is the player's.
    if settings.racer == 0 && !garage.racers.is_empty() {
        settings.racer = 1;
    }
    let making = matches!(
        page,
        Page::Racer | Page::Driver | Page::Licence | Page::Car | Page::Bricks
    );
    if page == Page::Garage || (making && bench.kit.is_none()) {
        let slot = settings.racer.checked_sub(1);
        if bench.take(art, garage, slot).is_none() {
            warn!("the game's bricks could not be read");
        }
    }
    if page == Page::Bricks {
        bench.hold();
        bench.view = FIRST_VIEW;
        let reach = bench.reach;
        bench.motion.begin(FIRST_VIEW, reach);
    }
}

/// The car taken apart to its chassis.
fn strip(bench: &mut Bench) -> Option<()> {
    let kit = bench.kit.as_ref()?;
    let chassis = bench.car.chassis(&kit.library)?.to_string();
    bench.car = Car::new(&kit.library, &chassis);
    bench.racer.stock = true;
    Some(())
}

/// `LoadQuickBuildCar`: the next of the game's cars on this chassis.
fn quick(bench: &mut Bench) -> Option<()> {
    let kit = bench.kit.as_ref()?;
    let chassis = bench.car.chassis(&kit.library)?;
    let count = kit.stock.len();
    let next = (1..=count)
        .map(|step| (bench.quick + step) % count)
        .find(|&at| kit.stock[at].chassis == chassis)?;
    bench.car = Car::read(&kit.library, &kit.stock[next].car);
    (bench.quick, bench.racer.stock) = (next, true);
    Some(())
}

/// Does what a widget of the garage's is for. `change` is which way a selector was
/// turned, and nought for something chosen. Returns the page to go to, if another.
pub fn act(
    act: Act,
    change: i32,
    art: &Art,
    bench: &mut Bench,
    garage: &mut Garage,
    settings: &mut Settings,
    progress: &Progress,
    menu: &Menu,
    sfx: &mut Sfx,
) -> Option<Page> {
    let turn = |value: usize, count: usize| {
        (value as i32 + change).rem_euclid(count.max(1) as i32) as usize
    };
    bench.said = None;
    bench.stale = true;
    let chosen = change == 0;
    let page = menu.page;
    match act {
        Act::Pick if !chosen && !garage.racers.is_empty() => {
            settings.racer = turn(settings.racer.saturating_sub(1), garage.racers.len()) + 1;
        }
        Act::New if chosen => {
            bench.take(art, garage, None)?;
            return Some(Page::Driver);
        }
        Act::Edit if chosen => {
            bench.take(art, garage, settings.racer.checked_sub(1))?;
            return Some(Page::Racer);
        }
        Act::Copy if chosen => {
            let copy = garage.racing(settings)?.clone();
            garage.racers.push(copy);
            settings.racer = garage.racers.len();
            garage.keep();
        }
        Act::Delete if chosen => return Some(ask(bench, page, Ask::Delete)),
        Act::Drive if chosen => {
            bench.drive = true;
            return None;
        }
        Act::Scrap(yes) if chosen => {
            let (question, from) = (std::mem::take(&mut bench.ask), bench.from);
            if !yes {
                return Some(from);
            }
            return match question {
                Ask::Delete => {
                    if let Some(slot) = settings.racer.checked_sub(1)
                        && slot < garage.racers.len()
                    {
                        garage.racers.remove(slot);
                        settings.racer = settings.racer.min(garage.racers.len());
                        garage.keep();
                    }
                    Some(Page::Garage)
                }
                Ask::Abandon => Some(Page::Garage),
                Ask::Lose(to) => {
                    bench.take(art, garage, bench.slot)?;
                    Some(to)
                }
                Ask::Quick => {
                    quick(bench)?;
                    Some(from)
                }
                Ask::Strip => {
                    strip(bench)?;
                    Some(from)
                }
            };
        }
        Act::Part(part) if !chosen => {
            let open = open_parts(bench, garage, progress, part);
            let c = &mut bench.racer.cosmetics;
            let value = match part {
                0 => &mut c.hat,
                1 => &mut c.face,
                2 => &mut c.torso,
                _ => &mut c.legs,
            };
            *value = turned(&open, *value, change)?;
            // `EditDriverScreen`: a new face is pulled with its own expression.
            if part == 1 {
                bench.racer.cosmetics.expression = 0;
            }
        }
        Act::Mix if chosen => {
            // `EditDriverScreen`: a part of each kind, picked at random from those
            // there are.
            let open: [Vec<u8>; 4] =
                std::array::from_fn(|part| open_parts(bench, garage, progress, part));
            let mut pick = |part: usize| {
                let choices = &open[part];
                let at = sfx.roll(choices.len().max(1) as u32) as usize;
                choices.get(at).copied().unwrap_or(0)
            };
            bench.racer.cosmetics = Cosmetics {
                hat: pick(0),
                face: pick(1),
                torso: pick(2),
                legs: pick(3),
                expression: 0,
            };
        }
        Act::Look if chosen => {
            // `DriverLicenseScreen::OnIconUnfocused`: the next of the six expressions.
            let c = &mut bench.racer.cosmetics;
            c.expression = (c.expression + 1) % EXPRESSIONS;
        }
        Act::Next if chosen => {
            if page == Page::Licence {
                bench.code = Some(bench.racer.name.clone());
            }
            // A new racer is made a screen at a time, and kept at the last of them.
            let to = match (page, bench.slot) {
                (Page::Driver, None) => Page::Licence,
                (Page::Licence, None) => Page::Car,
                _ => {
                    bench.save(garage, settings);
                    Page::Garage
                }
            };
            // `EditDriverScreen::OnIconUnfocused`, `CanNavigate`: the driver makes
            // one or other of two moves, and the page is left when it is done.
            let which = (std::time::UNIX_EPOCH.elapsed().map_or(0, |now| now.subsec_millis()) % 2) as usize;
            if let (Page::Driver, Some(ms)) = (page, stage::exit_move(art, which)) {
                bench.parting = Some((to, ms, which));
                return None;
            }
            return Some(to);
        }
        Act::GiveUp if chosen => {
            // Backing out of a new racer's licence keeps its name, and so a code.
            if page == Page::Licence && bench.slot.is_none() {
                bench.code = Some(bench.racer.name.clone());
            }
            sfx.play(id::MENU_BACK);
            return escape(page, art, bench, garage);
        }
        Act::Chassis if !chosen => {
            // `EditCarScreen::OnWidgetValueChanged`: the chassis alone, as handed out.
            bench.set = turned(&open_sets(bench, progress), bench.set, change)?;
            let kit = bench.kit.as_ref()?;
            bench.car = Car::new(&kit.library, &kit.sets.get(bench.set)?.chassis);
            (bench.racer.stock, bench.brick) = (true, 0);
        }
        // `EditCarScreen::OnIconUnfocused`: a car built on is not replaced unasked.
        Act::Strip if chosen => {
            if !bench.racer.stock {
                return Some(ask(bench, page, Ask::Strip));
            }
            strip(bench)?;
        }
        Act::Quick if chosen => {
            if !bench.racer.stock {
                return Some(ask(bench, page, Ask::Quick));
            }
            quick(bench)?;
        }
        Act::Set if !chosen => {
            bench.set = turned(&open_sets(bench, progress), bench.set, change)?;
            bench.brick = 0;
            bench.hold();
        }
        Act::Brick if !chosen => {
            let count = bench.kit.as_ref()?.sets.get(bench.set)?.choices.len();
            bench.brick = turn(bench.brick, count);
            bench.hold();
        }
        // `CarBuildScreen::OnIconUnfocused`: nothing while a brick is on its way down.
        Act::Turn | Act::Add | Act::Undo | Act::Set | Act::Brick | Act::Nudge | Act::View
            if bench.motion.busy() =>
        {
            return None;
        }
        Act::Nudge if !chosen => nudge(bench, change - 1, sfx),
        Act::View if !chosen => view(bench, change, sfx),
        Act::Turn if chosen => bench.cursor.turn(),
        Act::Add if chosen => {
            put_on(bench, art, sfx);
            return None;
        }
        Act::Undo if chosen => {
            // `UndoLastPiece`: the brick comes back into the hand, where it was.
            let kit = bench.kit.as_ref()?;
            let Some(last) = bench.car.undo(&kit.library) else {
                sfx.play(id::MENU_REFUSE);
                return None;
            };
            if let Some(piece) = kit.library.piece(last.kind) {
                bench.cursor.put(piece, &last);
            }
            let from = kit.sets.iter().position(|set| set.kind == last.set);
            let brick = from.and_then(|set| {
                let choices = &kit.sets[set].choices;
                let at = choices
                    .iter()
                    .position(|&c| c == (last.kind, last.colour))?;
                Some((set, at))
            });
            if let Some((set, brick)) = brick {
                (bench.set, bench.brick) = (set, brick);
            }
            bench.racer.stock = false;
            // `c_modeResetView`: it comes up off the car into the hand.
            bench.motion.take_up();
            sfx.play(id::MENU_BACK);
            return None;
        }
        _ => return None,
    }
    sfx.play(if chosen {
        id::MENU_CONFIRM
    } else {
        id::MENU_SELECT
    });
    None
}

/// `CommitPiece`: the brick held is put on the car, if it can go where it is.
fn put_on(bench: &mut Bench, art: &Art, sfx: &mut Sfx) -> Option<()> {
    let kit = bench.kit.as_ref()?;
    let cursor = bench.cursor;
    match bench.car.test(&kit.library, cursor.kind, cursor.x, cursor.y, cursor.rotation) {
        // It goes down onto the car, and is on it when it gets there (`tick`).
        Ok(_) => bench.motion.put_on(),
        Err(refusal) => {
            if refusal == Refusal::TooMany {
                bench.said = Some(art.string(text::LIMIT));
            }
            sfx.play(id::MENU_REFUSE);
        }
    }
    Some(())
}

/// The pointer on the car and on the brick held, where bricks are placed
/// (`CarBuildScreen::HandleSceneClick`, `HandleCursorDrag`, `HandleViewDrag`,
/// `HandleKeyUp`): the car is pulled round and up and down and goes on to the
/// nearest view when it is let go; the brick is pulled a stud at a time the way
/// the pointer goes, turned by the other button, and put on by a second press
/// soon after the first. `at` is where the pointer is and `was` where it was a
/// frame of `ms` ago. True if the pointer was the scene's to deal with.
pub fn grip(
    bench: &mut Bench,
    at: Option<Vec2>,
    was: Option<Vec2>,
    mouse: &ButtonInput<MouseButton>,
    ms: f32,
    art: &Art,
    sfx: &mut Sfx,
) -> bool {
    bench.pull_wait = (bench.pull_wait - ms).max(0.0);
    bench.again = (bench.again - ms).max(0.0);
    let (view, reach) = (bench.view, bench.reach);
    if bench.motion.busy() {
        bench.grip = None;
        return false;
    }
    // Let go of, the view goes on to the nearest.
    if bench.grip.is_some() && !mouse.pressed(MouseButton::Left) {
        if bench.grip == Some(Grip::Car) {
            bench.view = bench.motion.settle(view, reach);
        }
        bench.grip = None;
        return true;
    }
    let (Some(at), Some([car, brick])) = (at, bench.grips) else {
        return false;
    };
    let over = if brick.contains(at) {
        Some(Grip::Brick)
    } else {
        car.contains(at).then_some(Grip::Car)
    };
    if mouse.just_pressed(MouseButton::Right) && over == Some(Grip::Brick) {
        bench.cursor.turn();
        sfx.play(id::MENU_SELECT);
        bench.stale = true;
        return true;
    }
    if mouse.just_pressed(MouseButton::Left) {
        let Some(over) = over else {
            return false;
        };
        if over == Grip::Brick && std::mem::take(&mut bench.again) > 0.0 {
            put_on(bench, art, sfx);
            return true;
        }
        if over == Grip::Brick {
            bench.again = DOUBLE_CLICK;
        }
        (bench.grip, bench.pulled) = (Some(over), Vec2::ZERO);
        return true;
    }
    let moved = was.map_or(Vec2::ZERO, |was| at - was);
    match bench.grip {
        Some(Grip::Car) => {
            bench.motion.swing(view, -moved.x * SWING);
            bench.motion.tilt(moved.y * SWING);
        }
        Some(Grip::Brick) => {
            bench.pulled += moved;
            let pulled = bench.pulled;
            // Which of the eight ways it has been pulled far enough: across, and
            // up or down, each or both.
            let way = match (pulled.x >= PULL, pulled.x <= -PULL, pulled.y >= PULL, pulled.y <= -PULL) {
                (true, _, true, _) => Some(3),
                (true, _, _, true) => Some(1),
                (true, ..) => Some(2),
                (_, true, true, _) => Some(5),
                (_, true, _, true) => Some(7),
                (_, true, ..) => Some(6),
                (_, _, true, _) => Some(4),
                (_, _, _, true) => Some(0),
                _ => None,
            };
            if let (Some(way), true) = (way, bench.pull_wait <= 0.0) {
                nudge(bench, way, sfx);
                (bench.pulled, bench.pull_wait) = (Vec2::ZERO, PULL_WAIT);
            }
        }
        None => return false,
    }
    true
}

/// The help for what of the scene the pointer is on, and where that is.
pub fn grip_help(bench: &Bench, at: Vec2) -> Option<(usize, Rect)> {
    let [car, brick] = bench.grips?;
    if brick.contains(at) {
        Some((GRIP_HELP[1], brick))
    } else {
        car.contains(at).then_some((GRIP_HELP[0], car))
    }
}

/// Moves the brick held one of the eight ways of the compass as the car is seen: up
/// is away, whichever way the car has been turned, which in every other view is
/// cornerwise across it (`g_carBuildDragHorizontalOffsets`,
/// `g_carBuildDragVerticalOffsets`: an eighth of a turn on for each view).
fn nudge(bench: &mut Bench, way: i32, sfx: &mut Sfx) {
    const COMPASS: [(i32, i32); 8] =
        [(1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1), (0, 1), (1, 1)];
    let (dx, dy) = COMPASS[(way - bench.view).rem_euclid(8) as usize];
    let moved = bench.cursor.step(dx, dy);
    sfx.play(if moved { id::MENU_HIGHLIGHT } else { id::MENU_REFUSE });
    bench.stale = true;
}

/// Changes the view of the car by a place of the pad for it: an eighth of the way
/// round, up or down a height, or back as it began (`RotateViewStep`,
/// `PitchViewStep`, `BeginViewReset`).
fn view(bench: &mut Bench, spot: i32, sfx: &mut Sfx) {
    let reach = bench.reach;
    let turned = |bench: &mut Bench, to: i32| {
        let turning = to != bench.view && bench.motion.turn(bench.view);
        if turning {
            bench.view = to;
        }
        turning
    };
    let moved = match spot {
        VIEW_RIGHT => turned(bench, (bench.view + 1).rem_euclid(8)),
        VIEW_LEFT => turned(bench, (bench.view - 1).rem_euclid(8)),
        VIEW_UP => bench.motion.rise(1, reach),
        VIEW_DOWN => bench.motion.rise(-1, reach),
        VIEW_HOME => {
            let by = FIRST_HEIGHT as i32 - bench.motion.height as i32;
            let risen = by != 0 && bench.motion.rise(by.signum(), reach);
            turned(bench, FIRST_VIEW) || risen
        }
        _ => false,
    };
    if moved {
        sfx.play(id::MENU_HIGHLIGHT);
    }
}

/// A frame of the driver's page once it has been done with: the page to go to, as
/// the move its driver goes out on ends.
pub fn depart(bench: &mut Bench, ms: f32) -> Option<Page> {
    let (to, left, _) = bench.parting.as_mut()?;
    *left -= ms;
    if *left > 0.0 {
        return None;
    }
    let to = *to;
    bench.parting = None;
    Some(to)
}

/// A frame of the screen bricks are placed on: what moves there moves, and a brick
/// that has come down onto the car is put on it (`CarPartPlacement::Update`,
/// `UpdateCommitFeedback`). True when the car has changed.
pub fn tick(bench: &mut Bench, ms: f32, sfx: &mut Sfx) -> bool {
    let Some(kit) = &bench.kit else {
        return false;
    };
    let cursor = bench.cursor;
    let Some(piece) = kit.library.piece(cursor.kind) else {
        return false;
    };
    let (width, depth) = piece.span(cursor.rotation);
    let rest = bench.car.test(&kit.library, cursor.kind, cursor.x, cursor.y, cursor.rotation);
    let plates = rest.unwrap_or_else(|_| bench.car.over(cursor.x, cursor.y, width, depth));
    let (view, reach) = (bench.view, bench.reach);
    if !bench.motion.update(ms, view, reach, plates, rest.is_ok()) {
        return false;
    }
    // `PlacePiece`, and the brick held again comes up from where it went.
    bench.car.place(
        &kit.library,
        cursor.kind,
        cursor.x,
        cursor.y,
        cursor.rotation,
        cursor.colour,
        cursor.set,
    );
    (bench.racer.stock, bench.stale) = (false, true);
    bench.motion.take_up();
    sfx.play(id::MENU_CONFIRM);
    true
}

/// The keys of the screen bricks are placed on, which are its own. True if one of
/// them did something.
pub fn keys(
    keys: &ButtonInput<KeyCode>,
    art: &Art,
    bench: &mut Bench,
    garage: &mut Garage,
    settings: &mut Settings,
    progress: &Progress,
    menu: &Menu,
    sfx: &mut Sfx,
) -> bool {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let way = if shift { -1 } else { 1 };
    // `CarBuildScreen::HandleKeyDown`: no key does anything while a brick is on its
    // way down onto the car.
    if bench.motion.busy() {
        return false;
    }
    // These move the brick, and the arrows the view: an eighth of the way round,
    // up or down a height, or with `C` back as it began. The original has the
    // numeric keypad for the brick, which not every keyboard has.
    let pressed = [
        (KeyCode::KeyI, Act::Nudge, 1),
        (KeyCode::KeyL, Act::Nudge, 3),
        (KeyCode::KeyK, Act::Nudge, 5),
        (KeyCode::KeyJ, Act::Nudge, 7),
        (KeyCode::ArrowRight, Act::View, VIEW_RIGHT),
        (KeyCode::ArrowLeft, Act::View, VIEW_LEFT),
        (KeyCode::ArrowUp, Act::View, VIEW_UP),
        (KeyCode::ArrowDown, Act::View, VIEW_DOWN),
        (KeyCode::KeyC, Act::View, VIEW_HOME),
    ];
    let mut done = false;
    for (key, what, spot) in pressed {
        if keys.just_pressed(key) {
            match what {
                Act::Nudge => nudge(bench, spot - 1, sfx),
                _ => view(bench, spot, sfx),
            }
            done = true;
        }
    }
    let acts = [
        (KeyCode::KeyR, Act::Turn, 0),
        (KeyCode::Enter, Act::Add, 0),
        (KeyCode::Space, Act::Add, 0),
        (KeyCode::Backspace, Act::Undo, 0),
        (KeyCode::Tab, Act::Brick, way),
        (KeyCode::KeyT, Act::Set, way),
    ];
    for (key, what, change) in acts {
        if keys.just_pressed(key) {
            act(what, change, art, bench, garage, settings, progress, menu, sfx);
            done = true;
        }
    }
    bench.stale |= done;
    done
}

/// What is on show: the racer, and on the screen bricks are placed on the brick held.
#[derive(Component)]
pub struct Exhibit;

/// The racer on show, which turns.
#[derive(Component)]
pub struct Turntable;

/// One of the three bricks drawn of the brick held (`Motion::drawn`): which, where
/// it is on the car when it floats at no height, and the materials it is drawn
/// with, which are its own.
#[derive(Component)]
pub struct Held {
    which: usize,
    place: Vec3,
    /// How far across the brick is, from its middle to a corner, in the game's units.
    reach: f32,
    materials: Vec<Handle<StandardMaterial>>,
}

/// Shows the racer a page of the garage's is about, in front of the camera, and
/// clears it away on any other page.
pub fn show(
    mut commands: Commands,
    art: Res<Art>,
    menu: Res<Menu>,
    garage: Res<Garage>,
    settings: Res<Settings>,
    time: Res<Time<Real>>,
    mut bench: ResMut<Bench>,
    mut clear: ResMut<ClearColor>,
    mut camera: Single<
        (&mut Transform, &mut Projection),
        (
            With<Camera3d>,
            Without<Turntable>,
            Without<bevy::camera::visibility::RenderLayers>,
            Without<super::licence::Lens>,
        ),
    >,
    mut tables: Query<&mut Transform, With<Turntable>>,
    mut lenses: Query<
        &mut Transform,
        (
            With<stage::Lens>,
            With<Camera3d>,
            With<bevy::camera::visibility::RenderLayers>,
            Without<Turntable>,
        ),
    >,
    exhibits: Query<Entity, With<Exhibit>>,
    mut bricks: Query<(&Held, &mut Transform, &mut Visibility), (Without<Turntable>, Without<Camera3d>)>,
    staged_set: Res<stage::Stage>,
    (mut meshes, mut materials, mut images): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
    ),
    mut shown: Local<Option<Page>>,
) {
    let page = mine(menu.page).then_some(menu.page);
    if page != *shown {
        (*shown, bench.stale) = (page, true);
    }
    let Some(page) = page else {
        for exhibit in &exhibits {
            commands.entity(exhibit).despawn();
        }
        return;
    };
    // The camera looks past the racer, which puts it to the right of the widgets.
    let (eye, at) = match page {
        Page::Driver | Page::Licence => (Vec3::new(-1.6, 1.8, 4.6), Vec3::new(-1.6, 0.6, 0.0)),
        Page::Bricks => (Vec3::new(-1.2, 3.2, 6.0), Vec3::new(-1.2, 0.7, 0.0)),
        // Lower, to stand clear of the name or the question written above the racer.
        Page::Garage | Page::Racer | Page::Scrap => {
            (Vec3::new(-1.3, 2.6, 6.0), Vec3::new(-1.3, 1.0, 0.0))
        }
        _ => (Vec3::new(-1.3, 2.0, 6.0), Vec3::new(-1.3, 0.4, 0.0)),
    };
    *camera.0 = Transform::from_translation(eye).looking_at(at, Vec3::Y);
    // A race leaves the camera seeing wider than the garage is laid out for.
    if let Projection::Perspective(lens) = &mut *camera.1 {
        lens.fov = PerspectiveProjection::default().fov;
    }
    clear.0 = WALL;
    // The garage's pages and the driver's have their racer put on its set by `stage`.
    let set = staged(page, &bench, &garage, &settings).map(|staged| staged.1);
    if matches!(set, Some(Set::Showcase | Set::Platform)) {
        for exhibit in &exhibits {
            commands.entity(exhibit).despawn();
        }
        return;
    }
    // The car stands in the set it is built in, turning by itself on the car's
    // page and as the view of it is turned where bricks are placed
    // (`CarPartPlacement::ApplyViewAngle`).
    let building = set == Some(Set::Bench);
    let angle = if building { bench.motion.angle } else { time.elapsed_secs() * SPIN };
    // The car points along the game's X turned that far round.
    let way = crate::scenery::to_world(Vec3::new(angle.cos(), angle.sin(), 0.0));
    let stood = Transform::from_translation(crate::scenery::to_world(CAR_AT))
        .looking_to(way.normalize_or(Vec3::NEG_Z), Vec3::Y);
    for mut table in &mut tables {
        *table = stood;
    }
    // `CarPartPlacement::ResetCamera`, `UpdateViewPitch`: where bricks are placed
    // the camera stands back by how large the car is, and looks over it.
    if let (true, Ok(mut lens)) = (building, lenses.single_mut()) {
        let eye = crate::scenery::to_world(bench.motion.eye);
        let over = crate::scenery::to_world(CAR_AT + Vec3::Z * LOOK_OVER);
        *lens = Transform::from_translation(eye).looking_at(over, Vec3::Y);
        // `CarBuildScreen::UpdateHoverRegions`: where the car and the brick held
        // are on the screen, as squares about what their camera sees of a ball
        // round each (`MenuSceneView::GetEntityScreenRect`).
        let area = Set::Bench.area(&art);
        let inward = lens.compute_affine().inverse();
        let focal = 1.0 / (staged_set.fov / 2.0).tan();
        let seen = |middle: Vec3, radius: f32| {
            let from = inward.transform_point3(middle);
            let depth = -from.z;
            if depth <= 0.0 || staged_set.fov <= 0.0 {
                return Rect::default();
            }
            let across = Vec2::new(from.x * area.height() / area.width(), -from.y) * focal / depth;
            let half = radius * focal / depth * area.height() / 2.0;
            let middle = area.center() + across * area.size() / 2.0;
            Rect::from_center_half_size(middle, Vec2::splat(half)).intersect(area)
        };
        let brick = bricks.iter().find(|brick| brick.0.which == 0).map(|brick| brick.0);
        let brick = brick.map_or(Rect::default(), |held| {
            seen(stood.transform_point(held_at(held.place, HELD_AT)), held.reach * UNIT)
        });
        bench.grips = Some([seen(stood.translation, bench.reach * UNIT), brick]);
    } else {
        bench.grips = None;
    }
    // `CarPartPlacement::Draw`: the brick held, and its ghosts.
    let drawn = bench.motion.drawn();
    for (held, mut transform, mut visibility) in &mut bricks {
        let Some((height, alpha)) = drawn[held.which] else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = Visibility::Inherited;
        transform.translation = held_at(held.place, height);
        for material in &held.materials {
            if let Some(mut material) = materials.get_mut(material) {
                material.base_color = Color::srgba(1.0, 1.0, 1.0, alpha);
            }
        }
    }
    if !std::mem::take(&mut bench.stale) {
        return;
    }
    for exhibit in &exhibits {
        commands.entity(exhibit).despawn();
    }
    // `DriverLicenseScreen::CreateDriverScene`: the licence has its driver alone, in
    // the photograph (`licence`).
    if page == Page::Licence {
        return;
    }
    let racer = match page {
        Page::Garage => garage.racing(&settings).cloned(),
        Page::Scrap if bench.ask == Ask::Delete => garage.racing(&settings).cloned(),
        _ => Some(bench.shown()),
    };
    let Some(model) = racer.and_then(|racer| crate::world::load_built(&art.jam, &racer, true))
    else {
        return;
    };
    // How far across the car is, from its middle to a corner.
    let [side, ahead, behind] = model.outline;
    bench.reach = side.hypot((ahead + behind) / 2.0);
    let table = commands
        .spawn((
            Exhibit,
            Turntable,
            stood,
            Visibility::default(),
        ))
        .id();
    // In its set, it is the set's camera that draws it.
    if set.is_some() {
        commands.entity(table).insert(stage::Staged);
    }
    crate::time_race::dress(
        &mut commands,
        table,
        model,
        &mut meshes,
        &mut materials,
        &mut images,
        None,
    );
    let (true, Some(kit)) = (building, &bench.kit) else {
        return;
    };
    // The brick held, over where it would go: `CarPartPlacement::Draw`.
    let cursor = bench.cursor;
    let (Some(palette), Some(piece)) = (
        Palette::open(&art.jam, true),
        kit.library.piece(cursor.kind),
    ) else {
        return;
    };
    let files = Palette::files(true);
    let library =
        crate::world::Library::new(&art.jam, files.iter().map(String::as_str), &[leb::DIR]);
    let held = Car::piece_model(&kit.library, &palette, cursor.kind, cursor.colour);
    let (width, depth) = piece.span(cursor.rotation);
    let [ox, oy, oz] = bench.car.offset(&kit.library);
    // Where its middle is with the brick resting on the chassis' own level.
    let place = Vec3::new(
        cursor.x as f32 + width as f32 / 2.0 + ox,
        cursor.y as f32 + depth as f32 / 2.0 + oy,
        piece.height() as f32 / 2.0 * PLATE + oz,
    );
    let quarter = Quat::from_rotation_z(-cursor.rotation as f32 * std::f32::consts::FRAC_PI_2);
    let drawn = bench.motion.drawn();
    commands.entity(table).with_children(|table| {
        for which in 0..drawn.len() {
            let mut own = Vec::new();
            let bundles: Vec<_> = library
                .surfaces(&held, |_| true, Vec3::from)
                .into_iter()
                .map(|surface| {
                    let mut bundle = crate::world::surface_bundle(
                        surface,
                        &mut meshes,
                        &mut materials,
                        &mut images,
                    );
                    // Each of the three is as clear as it is by itself.
                    if let Some(mut material) = materials.get(&bundle.1.0).cloned() {
                        if which > 0 {
                            material.alpha_mode = AlphaMode::Blend;
                        }
                        bundle.1 = MeshMaterial3d(materials.add(material));
                    }
                    own.push(bundle.1.0.clone());
                    bundle
                })
                .collect();
            let (height, shown) = match drawn[which] {
                Some((height, _)) => (height, Visibility::Inherited),
                None => (HELD_AT, Visibility::Hidden),
            };
            table
                .spawn((
                    Held {
                        which,
                        place,
                        reach: Vec3::new(width as f32, depth as f32, piece.height() as f32 * PLATE)
                            .length()
                            / 2.0,
                        materials: own,
                    },
                    Transform {
                        translation: held_at(place, height),
                        rotation: car_basis() * quarter,
                        scale: Vec3::splat(UNIT),
                    },
                    shown,
                ))
                .with_children(|brick| {
                    for bundle in bundles {
                        brick.spawn(bundle);
                    }
                });
        }
    });
}

/// The game's models have X forward, Y left and Z up; ours face -Z with Y up.
fn car_basis() -> Quat {
    Quat::from_mat3(&Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y))
}

/// Where on the car a brick is drawn that would rest at `place` on the chassis'
/// own level and is at a height of the original's counting.
fn held_at(place: Vec3, height: f32) -> Vec3 {
    car_basis() * (place + Vec3::Z * (height - HELD_AT + HELD_OVER)) * UNIT
}

/// Clears away what is on show when the menus are left.
pub fn put_away(
    mut commands: Commands,
    mut bench: ResMut<Bench>,
    exhibits: Query<Entity, With<Exhibit>>,
) {
    // Whatever page the menus are come back to has its racer to make again.
    bench.stale = true;
    for exhibit in &exhibits {
        commands.entity(exhibit).despawn();
    }
}

#[cfg(test)]
#[test]
fn help_comes_after_three_seconds_and_goes_with_the_pointer() {
    let on = |help: usize| Some((help, Rect::new(100.0, 100.0, 132.0, 132.0)));
    let mut tip = Tip::default();
    // Nothing until the pointer has rested three seconds.
    assert!(!tip.update(on(3), 2900.0) && tip.showing().is_none());
    assert!(tip.update(on(3), 200.0));
    assert_eq!(tip.showing().map(|shown| shown.0), Some(3));
    // It goes as the pointer leaves, and the next thing's comes at once if soon.
    assert!(tip.update(None, 16.0) && tip.showing().is_none());
    assert!(tip.update(on(4), 100.0));
    assert_eq!(tip.showing().map(|shown| shown.0), Some(4));
    // Left a while, the wait is the whole three seconds again.
    tip.update(None, 16.0);
    tip.update(None, 300.0);
    assert!(!tip.update(on(5), 100.0) && tip.showing().is_none());
    assert!(tip.update(on(5), 3000.0));
    // After twenty seconds it goes, and stays gone while the pointer stays.
    assert!(tip.update(on(5), 20000.0) && tip.showing().is_none());
    assert!(!tip.update(on(5), 5000.0) && tip.showing().is_none());

    // Beside its thing if there is room there, and under it if there isn't.
    let screen = Rect::new(0.0, 0.0, 640.0, 480.0);
    let (size, line) = (Vec2::new(200.0, 60.0), 10.0);
    let left = Rect::new(100.0, 100.0, 132.0, 132.0);
    assert_eq!(Tip::place(left, size, line, screen), Vec2::new(142.0, 86.0));
    let right = Rect::new(600.0, 100.0, 632.0, 132.0);
    assert_eq!(Tip::place(right, size, line, screen), Vec2::new(430.0, 142.0));
    // About three times as wide as tall, and never wider than the screen.
    assert_eq!(Tip::wrap(1200.0, 11.0, screen), 207.0);
    assert_eq!(Tip::wrap(100000.0, 11.0, screen), 618.0);
}

#[cfg(test)]
#[test]
fn the_view_of_a_car_being_built_turns_and_rises_and_its_brick_comes_down() {
    let mut motion = Motion::default();
    motion.begin(FIRST_VIEW, 6.0);
    // It begins from the middle height, an eighth of a turn round.
    assert_eq!(motion.height, FIRST_HEIGHT);
    assert_eq!(motion.eye, Motion::eye_at(1, 6.0));
    assert!((motion.angle - (VIEW_FROM + VIEW_STEP)).abs() < 1e-5);

    // A turn takes its time, and nothing else turns it meanwhile.
    assert!(motion.turn(1) && !motion.turn(2));
    motion.update(50.0, 2, 6.0, 0, true);
    let part = motion.angle - Motion::facing(1);
    assert!(part > 0.0 && part < VIEW_STEP, "{part}");
    motion.update(100.0, 2, 6.0, 0, true);
    assert!((motion.angle - Motion::facing(2)).abs() < 1e-5);
    // From the last view to the first is an eighth of a turn too, not seven back.
    assert!(motion.turn(7));
    motion.update(75.0, 0, 6.0, 0, true);
    let part = (motion.angle - Motion::facing(7)).rem_euclid(std::f32::consts::TAU);
    assert!(part > 0.0 && part < VIEW_STEP, "{part}");
    motion.update(75.0, 0, 6.0, 0, true);

    // There are three heights, and no more.
    assert!(motion.rise(1, 6.0) && motion.height == OVERHEAD);
    motion.update(150.0, 0, 6.0, 0, true);
    assert!(motion.eye.z > Motion::eye_at(1, 6.0).z && motion.eye.z < Motion::eye_at(2, 6.0).z);
    motion.update(150.0, 0, 6.0, 0, true);
    assert_eq!(motion.eye, Motion::eye_at(2, 6.0));
    assert!(!motion.rise(1, 6.0));
    // From overhead the brick held is not drawn, and nor is the ghost that moves.
    let [held, ghost, rest] = motion.drawn();
    assert!(held.is_none() && ghost.is_none() && rest.is_some());
    assert!(motion.rise(-1, 6.0));
    motion.update(300.0, 0, 6.0, 0, true);

    // A brick that fits has a ghost go up from where it would rest and down again.
    let mut motion = Motion::default();
    motion.begin(FIRST_VIEW, 6.0);
    let rest = 3.0 * PLATE + HELD_AT - HELD_OVER;
    let mut heights = Vec::new();
    for _ in 0..400 {
        motion.update(16.0, 1, 6.0, 3, true);
        heights.push(motion.ghost_at);
    }
    // It comes down to there first.
    let down = heights.iter().position(|&at| at <= rest + 1e-4).unwrap();
    let heights = &heights[down..];
    let top = heights.iter().copied().fold(f32::MIN, f32::max);
    assert!((top - (HELD_AT - HOVER * 2.0)).abs() < 1e-4, "{top}");
    assert!(heights.iter().all(|&at| at >= rest - 1e-4));
    assert!(heights.windows(2).any(|pair| pair[1] < pair[0]));
    // One that doesn't has only the ghost where it is held over, flashing.
    motion.update(16.0, 1, 6.0, 5, false);
    let [held, ghost, flashing] = motion.drawn();
    assert_eq!(held, Some((HELD_AT, 1.0)));
    assert!(ghost.is_none() && flashing.is_some());

    // Put on, the brick falls from where it floats to where it rests, and is on
    // the car the frame after; then the next comes up from there.
    motion.update(16.0, 1, 6.0, 3, true);
    motion.put_on();
    assert!(motion.busy());
    let mut frames = 0;
    while !motion.update(16.0, 1, 6.0, 3, true) {
        frames += 1;
        assert!(frames < 200);
        assert!(motion.drawn()[1].is_none() && motion.drawn()[2].is_none());
    }
    let fall = (HELD_AT - rest) / GHOST_SPEED / 16.0;
    assert!((frames as f32 - fall).abs() < 3.0, "{frames} {fall}");
    assert!(!motion.busy());
    motion.take_up();
    motion.update(16.0, 1, 6.0, 3, true);
    let (at, _) = motion.drawn()[0].unwrap();
    assert!(at > rest && at < HELD_AT);
    for _ in 0..200 {
        motion.update(16.0, 1, 6.0, 3, true);
    }
    assert_eq!(motion.drawn()[0], Some((HELD_AT, 1.0)));
}

#[cfg(test)]
#[test]
fn the_bricks_page_has_its_pads_where_the_layout_puts_them() {
    let Some(art) = super::load_art() else {
        return;
    };
    assert_eq!(art.place("carbuild", "procker").min, Vec2::new(51.0, 40.0));
    assert_eq!(art.place("carbuild", "crocker").min, Vec2::new(51.0, 301.0));
    // Places laid out the plainer way are where they were.
    assert_eq!(art.place("carbuild", "rotbrik").min, Vec2::new(60.0, 160.0));
    assert_eq!(art.place("carbuild", "garage"), Rect::new(207.0, 115.0, 627.0, 469.0));
    // Every place to press is inside its pad's picture, and no two overlap.
    for (spots, size) in [(&MOVE_SPOTS[..], Vec2::new(120.0, 112.0)), (&VIEW_SPOTS[..], Vec2::splat(92.0))] {
        for (n, (_, [left, top, right, bottom])) in spots.iter().enumerate() {
            assert!(*left >= 0.0 && *top >= 0.0 && *right <= size.x && *bottom <= size.y);
            let rect = Rect::new(*left, *top, *right, *bottom);
            for (_, [l, t, r, b]) in &spots[n + 1..] {
                let both = rect.intersect(Rect::new(*l, *t, *r, *b));
                assert!(both.width() < 1.0 || both.height() < 1.0, "{rect:?}");
            }
        }
    }
    // And each pad has its help.
    assert_eq!(Tip::of(&Action::Bench(Act::Nudge)), Some(2));
    assert_eq!(Tip::of(&Action::Bench(Act::View)), Some(6));
}

#[cfg(test)]
#[test]
fn a_car_pulled_about_by_the_pointer_goes_on_to_the_nearest_view() {
    let mut motion = Motion::default();
    motion.begin(FIRST_VIEW, 6.0);
    // Sixty pixels to the left is six tenths of a view on, and the car is there at once.
    motion.swing(FIRST_VIEW, 60.0 * SWING);
    motion.update(16.0, FIRST_VIEW, 6.0, 0, true);
    assert!((motion.angle - (Motion::facing(1) + 0.6 * VIEW_STEP)).abs() < 1e-4);
    // Let go, it goes on to the view it is nearest, in the time a turn takes.
    assert_eq!(motion.settle(FIRST_VIEW, 6.0), 2);
    motion.update(TURN_MS / 2.0, 2, 6.0, 0, true);
    assert!(motion.angle > Motion::facing(1) + 0.6 * VIEW_STEP && motion.angle < Motion::facing(2));
    motion.update(TURN_MS, 2, 6.0, 0, true);
    assert!((motion.angle - Motion::facing(2)).abs() < 1e-5);
    // Pulled back past the first view it comes round to the last.
    motion.swing(0, -0.3);
    assert_eq!(motion.settle(0, 6.0), 0);
    motion.update(TURN_MS, 0, 6.0, 0, true);
    motion.swing(0, -0.6);
    assert_eq!(motion.settle(0, 6.0), 7);
    motion.update(TURN_MS, 7, 6.0, 0, true);

    // Pulled down it rises between the heights, no higher than the highest.
    motion.tilt(0.4);
    motion.update(16.0, 7, 6.0, 0, true);
    let between = Motion::eye_at(1, 6.0).lerp(Motion::eye_at(2, 6.0), 0.4);
    assert!(motion.eye.distance(between) < 1e-4);
    motion.tilt(5.0);
    motion.update(16.0, 7, 6.0, 0, true);
    assert_eq!(motion.eye, Motion::eye_at(OVERHEAD, 6.0));
    motion.tilt(-1.4);
    motion.update(16.0, 7, 6.0, 0, true);
    // And goes on to the nearest height when let go.
    motion.settle(7, 6.0);
    assert_eq!(motion.height, 1);
    motion.update(RISE_MS, 7, 6.0, 0, true);
    assert_eq!(motion.eye, Motion::eye_at(1, 6.0));
}

#[cfg(test)]
#[test]
fn the_help_names_the_ports_keys_in_every_language() {
    let Some(mut art) = super::load_art() else {
        return;
    };
    for language in 0..crate::assets::font::LANGUAGES.len() {
        art.speak(language);
        for (string, keys) in KEYS.iter().enumerate() {
            let words = help(&art.help[string], string);
            assert!(words.ends_with(&format!(": {keys}")), "{language} {string}: {words}");
            // What is said before the keys is the game's own.
            assert!(words.len() > keys.len() + 10 && !words.contains("NUMERIC KEYPAD"), "{words}");
        }
        // The two about the pointer say nothing of keys, and are as they were.
        assert_eq!(help(&art.help[7], 7), art.help[7]);
        assert_eq!(help(&art.help[8], 8), art.help[8]);
    }
}
