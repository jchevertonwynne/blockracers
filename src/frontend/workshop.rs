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
//! - Bricks are placed with the keyboard, the original being played with a pad:
//!   the arrows move the brick, `R` turns it, `Enter` puts it on, `Backspace` takes
//!   the last off, `Tab` and `T` go on to the next brick and the next part set
//!   (back with `Shift`), and `,` and `.` turn the car round.

use super::{Action, Art, Item, LABEL, Menu, Page, Typed, Widget};
use crate::assets::{
    leb::{self, Library, PartSet},
    lrs::{Cosmetics, Racer},
};
use crate::audio::{Sfx, id};
use crate::build::{Car, Catalogue, Cursor, Palette, Refusal};
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
/// How fast a racer on show turns, in turns a second.
const SPIN: f32 = 0.1;
/// What is behind a racer on show.
const WALL: Color = Color::srgb_u8(6, 6, 70);

/// What the garage's screens can do.
#[derive(Clone, Copy, PartialEq)]
pub enum Act {
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

/// The racer one of the build menu's sets has on show, on the pages that have one,
/// and whether the set is the platform a driver is dressed on and not the
/// garage's showcase.
pub fn showcased(
    page: Page,
    bench: &Bench,
    garage: &Garage,
    settings: &Settings,
) -> Option<(Racer, bool)> {
    match page {
        Page::Garage => Some((garage.racing(settings)?.clone(), false)),
        Page::Scrap if bench.ask == Ask::Delete => Some((garage.racing(settings)?.clone(), false)),
        Page::Racer => Some((bench.shown(), false)),
        Page::Driver => Some((bench.shown(), true)),
        Page::Scrap if matches!(bench.from, Page::Garage | Page::Racer) => {
            Some((bench.shown(), false))
        }
        Page::Scrap if bench.from == Page::Driver => Some((bench.shown(), true)),
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
            let hint = |row: f32, words: &str| {
                (
                    Rect::new(8.0, 284.0 + row * 24.0, 200.0, 308.0 + row * 24.0),
                    words.to_string(),
                    "font_ths",
                    LABEL,
                    false,
                )
            };
            // Which brick of the set is held, under the row of them.
            let held = bench.kit.as_ref().and_then(|kit| {
                let set = kit.sets.get(bench.set)?;
                Some(format!("BRICK {} OF {}", bench.brick + 1, set.choices.len()))
            });
            let row = art.place("carbuild", "pieces");
            vec![
                (
                    Rect::new(row.min.x + 32.0, row.max.y, row.max.x - 32.0, row.max.y + 24.0),
                    held.unwrap_or_default(),
                    "font_ths",
                    LABEL,
                    true,
                ),
                hint(0.0, "ARROWS MOVE"),
                hint(1.0, "R TURN"),
                hint(2.0, "ENTER ADD"),
                hint(3.0, "BACKSPACE UNDO"),
                hint(4.0, "TAB BRICK  T SET"),
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
            return Some(match (page, bench.slot) {
                (Page::Driver, None) => Page::Licence,
                (Page::Licence, None) => Page::Car,
                _ => {
                    bench.save(garage, settings);
                    Page::Garage
                }
            });
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
        Act::Turn if chosen => bench.cursor.turn(),
        Act::Add if chosen => {
            // `CommitPiece`.
            let kit = bench.kit.as_ref()?;
            let cursor = bench.cursor;
            match bench.car.test(
                &kit.library,
                cursor.kind,
                cursor.x,
                cursor.y,
                cursor.rotation,
            ) {
                Ok(_) => {
                    bench.car.place(
                        &kit.library,
                        cursor.kind,
                        cursor.x,
                        cursor.y,
                        cursor.rotation,
                        cursor.colour,
                        cursor.set,
                    );
                    bench.racer.stock = false;
                    sfx.play(id::MENU_CONFIRM);
                }
                Err(refusal) => {
                    if refusal == Refusal::TooMany {
                        bench.said = Some(art.string(text::LIMIT));
                    }
                    sfx.play(id::MENU_REFUSE);
                }
            }
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
    // The arrows move the brick as the car is seen: up is away, whichever way the
    // car has been turned.
    let quarter = (bench.view + 1).div_euclid(2).rem_euclid(4);
    let steer = |dx: i32, dy: i32| match quarter {
        0 => (dx, dy),
        1 => (dy, -dx),
        2 => (-dx, -dy),
        _ => (-dy, dx),
    };
    let moves = [
        (KeyCode::ArrowUp, (1, 0)),
        (KeyCode::ArrowDown, (-1, 0)),
        (KeyCode::ArrowLeft, (0, 1)),
        (KeyCode::ArrowRight, (0, -1)),
    ];
    let mut done = false;
    for (key, (dx, dy)) in moves {
        if keys.just_pressed(key) {
            let (dx, dy) = steer(dx, dy);
            let moved = bench.cursor.step(dx, dy);
            sfx.play(if moved {
                id::MENU_HIGHLIGHT
            } else {
                id::MENU_REFUSE
            });
            done = true;
        }
    }
    let turns =
        keys.just_pressed(KeyCode::Period) as i32 - keys.just_pressed(KeyCode::Comma) as i32;
    if turns != 0 {
        bench.view = (bench.view + turns).rem_euclid(8);
        done = true;
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
    exhibits: Query<Entity, With<Exhibit>>,
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
    // The garage's pages have their racer in the showcase (`stage`).
    if showcased(page, &bench, &garage, &settings).is_some() {
        for exhibit in &exhibits {
            commands.entity(exhibit).despawn();
        }
        return;
    }
    let building = page == Page::Bricks;
    for mut table in &mut tables {
        table.rotation = if building {
            Quat::from_rotation_y(bench.view as f32 * std::f32::consts::FRAC_PI_4)
        } else {
            Quat::from_rotation_y(time.elapsed_secs() * SPIN * std::f32::consts::TAU)
        };
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
    let table = commands
        .spawn((
            Exhibit,
            Turntable,
            Transform::default(),
            Visibility::default(),
        ))
        .id();
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
    let rest = bench.car.test(
        &kit.library,
        cursor.kind,
        cursor.x,
        cursor.y,
        cursor.rotation,
    );
    let floor = rest.unwrap_or_else(|_| bench.car.over(cursor.x, cursor.y, width, depth));
    let [ox, oy, oz] = bench.car.offset(&kit.library);
    let middle = Vec3::new(
        cursor.x as f32 + width as f32 / 2.0 + ox,
        cursor.y as f32 + depth as f32 / 2.0 + oy,
        (floor as f32 + piece.height() as f32 / 2.0) * PLATE + oz + HOVER,
    );
    // The game's models have X forward, Y left and Z up; ours face -Z with Y up.
    let basis = Quat::from_mat3(&Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y));
    let quarter = Quat::from_rotation_z(-cursor.rotation as f32 * std::f32::consts::FRAC_PI_2);
    commands.entity(table).with_children(|table| {
        table
            .spawn((
                Transform {
                    translation: basis * middle * UNIT,
                    rotation: basis * quarter,
                    scale: Vec3::splat(UNIT),
                },
                Visibility::default(),
            ))
            .with_children(|brick| {
                for surface in library.surfaces(&held, |_| true, Vec3::from) {
                    let bundle = crate::world::surface_bundle(
                        surface,
                        &mut meshes,
                        &mut materials,
                        &mut images,
                    );
                    // A brick that can't go where it is held is shown faintly.
                    if let (Err(_), Some(mut material)) = (rest, materials.get_mut(&bundle.1.0)) {
                        material.base_color = Color::srgba(1.0, 1.0, 1.0, 0.35);
                        material.alpha_mode = AlphaMode::Blend;
                    }
                    brick.spawn(bundle);
                }
            });
    });
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
