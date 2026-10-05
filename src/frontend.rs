//! The original game's front end, drawn from its own menu data: the screen layouts
//! (`.MIB`), pictures, bitmap fonts and string tables in `MENUDATA`. Main menu, single
//! race and options are here, and the build menu in `workshop`; the screens for
//! things the port doesn't have (controls) are shown but can't be chosen. The video options are the port's
//! own, and so is the extras page: the ways of racing the original doesn't have.
//!
//! Racing online is the port's own too, and is reached where the original has its
//! two-player race: a page to host or join from, the list of sessions the lobby
//! has, and the room a session waits in, where everyone votes for the next race's
//! circuit and the host says how the rest of it is run (`net::room`). Their widgets are placed by hand; the original has
//! no screens to take the places from.
//!
//! Colours and the make-up of each widget follow the styles in `GSTYLES.MSB`. The
//! spinning models the original shows on these screens are not drawn.

use crate::assets::{
    Jam,
    font::{Font, load_fonts, load_strings},
    image::decode_bmp,
    lrs::Cosmetics,
    tokens::{Token, tokenize},
};
use crate::audio::{Sfx, id};
use crate::championship::Championship;
use crate::film::{Request, Showing};
use crate::garage::Garage;
use crate::input::{Bindings, Bound, Devices, EVENTS, PAD};
use crate::progress::Progress;
use crate::menu::{
    Circuits, DIFFICULTIES, Extra, LAP_CHOICES, MAX_OPPONENTS, MAX_VOLUME, NAME_LENGTH, Screen,
    Settings,
};
use crate::net::{
    self, Role, Session,
    lobby::Lobby,
    protocol::{Ride, Rules},
    room::Room,
};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::PrimaryWindow,
};
use std::collections::HashMap;

mod mascot;
mod portraits;
mod workshop;
use mascot::Mascot;
use portraits::Portraits;
use workshop::Bench;

/// The original lays its menus out on a screen this size.
const SCREEN: Vec2 = Vec2::new(640.0, 480.0);
const DIR: &str = "/MENUDATA";
/// Every text button has a picture this size before its words.
const ICON: f32 = 32.0;

// Text colours of a button by state, from the `nubutton` style.
const DISABLED: Color = Color::srgb_u8(75, 75, 75);
const NORMAL: Color = Color::srgb_u8(118, 107, 15);
const SELECTED: Color = Color::srgb_u8(246, 230, 6);
/// Labels and banners.
const LABEL: Color = Color::srgb_u8(239, 239, 239);
/// The fill of the `brickbox` frame.
const BOX_FILL: Color = Color::srgb_u8(8, 8, 115);

// Strings of `MENUTEXT.SRF`.
mod text {
    pub const MAIN_MENU: usize = 2;
    pub const OPTIONS_BANNER: usize = 16;
    pub const GAME_OPTIONS: usize = 17;
    pub const VIDEO_OPTIONS: usize = 18;
    pub const AUDIO_OPTIONS: usize = 19;
    pub const CONTROLS: [usize; 2] = [23, 24];
    pub const FINISH: usize = 30;
    pub const CIRCUIT_RACE: usize = 33;
    pub const SINGLE_RACE: usize = 34;
    pub const TIME_RACE: usize = 36;
    pub const BUILD: usize = 37;
    pub const OPTIONS: usize = 38;
    pub const QUIT: usize = 39;
    pub const CREDITS: usize = 88;
    pub const OPPONENTS: usize = 89;
    pub const MUSIC_VOLUME: usize = 93;
    pub const SOUND_VOLUME: usize = 94;
    /// The first of the nine things a player's keys do, in the order they are bound.
    pub const EVENTS: usize = 105;
    pub const OK: usize = 114;
    pub const TIME_TRIAL_WON: usize = 73;
    pub const NEW_CIRCUIT: usize = 124;
    pub const LANGUAGE: usize = 156;
}

/// The pictures on the circuit selector, by which of the game's circuits a race is
/// in; the last is for the port's own brick circuit.
/// The pictures for the seven circuits of the circuit race: the three sets of four
/// races, the same three mirrored, and Rocket Racer's.
const SERIES_ICONS: [&str; 7] = [
    "pirate", "islander", "magical", "pirate", "islander", "magical", "rr",
];
const CIRCUIT_ICONS: [&str; 5] = ["pirate", "islander", "magical", "rr", "bricks"];

#[derive(Clone, Copy, PartialEq, Default)]
enum Page {
    #[default]
    Main,
    SingleRace,
    CircuitRace,
    TimeRace,
    Options,
    GameOptions,
    VideoOptions,
    AudioOptions,
    /// The original's `ControlConfigScreen`: what the keys and a pad's buttons do.
    Controls,
    /// What a circuit raced to the end, or the last record beaten, has won, said
    /// after the circuit's film (`film`). The port's own: the original leaves it to
    /// a notice on the main menu and to the build menu to show.
    Award,
    Extras,
    /// The port's own, for racing online: where to host or join from, what to host
    /// as, the sessions there are to join, the password one of them wants, the wait
    /// for its host to answer, and the room a session waits in between races. Off
    /// the room, how the last race went, and for the host who is let in.
    Online,
    Host,
    Join,
    Password,
    Connecting,
    Room,
    Results,
    Session,
    /// Off the room too: the vote for the next race's circuit, and for the host how
    /// the rest of it is to be run.
    Wishes,
    Rules,
    /// Finding a session by the code its host has passed on.
    Code,
    /// The original's build menu (`workshop`): the garage, the question asked before
    /// a racer is deleted, what of a racer to change, and the screens its
    /// minifigure, its name and its car are made on, the last of them where bricks
    /// are put on the car.
    Garage,
    Scrap,
    Racer,
    Driver,
    Licence,
    Car,
    Bricks,
}

/// What can be typed into.
#[derive(Clone, Copy, PartialEq)]
enum Typed {
    Name,
    Title,
    Password,
    /// The password of the session being joined.
    Key,
    /// The password of the session being hosted, changed while it is.
    Lock,
    /// Something to say to the room, and the code of a session to find.
    Say,
    Code,
    /// The name of the racer being built.
    Racer,
}

/// What the host says of a race besides its laps and field: the port's own ways of racing
/// that are everyone's affair. How a player steers is their own.
/// How many races a session's series may be of; none is no series.
const SERIES: [u8; 4] = [0, 3, 5, 7];
const VOTED: [Extra; 4] = [
    Extra::Mirror,
    Extra::Reverse,
    Extra::Bricks,
    Extra::Elimination,
];
/// The most sessions the join page lists.
const LISTED: usize = 7;
/// The longest a session's title and its password may be.
const TITLE_LENGTH: usize = 20;
const PASSWORD_LENGTH: usize = 16;

/// What is being typed and picked on the online pages.
#[derive(Resource, Default)]
struct Online {
    title: String,
    password: String,
    key: String,
    /// The session picked to join.
    picked: Option<lobby_api::Session>,
    /// Seconds until the list of sessions is read again.
    refresh: f32,
    /// What of the room and of the list has been drawn.
    seen: (u32, u32),
    /// The player the host has asked to remove, and not yet said again.
    removing: Option<net::protocol::Peer>,
    /// Whether the session to be hosted is kept off the list.
    unlisted: bool,
    /// What is being typed to say to the room.
    say: String,
    /// The code typed to find a session by, and how the search for it stands.
    code: String,
    seeking: Option<bool>,
}

/// What the online pages show: who this game is to the session, the session, its
/// room, the lobby's list, and what has been typed.
struct Wired<'a> {
    role: Role,
    session: &'a Session,
    room: &'a Room,
    lobby: &'a Lobby,
    online: &'a Online,
    devices: &'a Devices,
    progress: &'a Progress,
}

impl Wired<'_> {
    /// The race the player here has asked for, as settings to show it by.
    fn wish(&self, settings: &Settings, circuits: &Circuits) -> Settings {
        let mut wish = settings.clone();
        if let Some(ballot) = &self.room.ballot {
            ballot.apply(&mut wish, circuits);
        }
        wish
    }
}

impl Page {
    /// The item a page opens on.
    fn first(self) -> usize {
        match self {
            Page::Main => 2,
            // Past the extras, to the first of the original's own.
            Page::Options => 1,
            // Past the selectors, to something to choose; and to "no".
            Page::Garage | Page::Scrap | Page::Car => 1,
            _ => 0,
        }
    }

    /// The port's settings a page shows, a selector to each.
    fn extras(self) -> &'static [Extra] {
        match self {
            Page::VideoOptions => &Extra::VIDEO,
            Page::Extras => &Extra::RACE,
            _ => &[],
        }
    }
}

/// Where a circuit was finished, as the award page says it, and the trophy for it.
const PLACES: [&str; 6] = ["FIRST", "SECOND", "THIRD", "FOURTH", "FIFTH", "SIXTH"];
const TROPHIES: [&str; 3] = ["gtrophy", "strophy", "btrophy"];

/// The rows of the controls page as `CONTROL.MIB` names them: the button a binding
/// is shown on, and the words beside it that say what it is for.
const CONTROL_ROWS: [(&str, &str); EVENTS] = [
    ("turnleft", "lefttext"),
    ("turnrght", "rghttext"),
    ("acceler", "acletext"),
    ("brake", "brketext"),
    ("fire", "firetext"),
    ("camera", "camtext"),
    ("map", "maptext"),
    ("slide", "pwsltext"),
    ("lookback", "lkbktext"),
];

/// Where the rows of a page of the port's settings go: the rectangles the original
/// gives the two rows of its game options, carried on down the screen.
fn rows(art: &Art, count: usize, names: [&str; 2]) -> Vec<Rect> {
    let (first, second) = (
        art.place("options", names[0]),
        art.place("options", names[1]),
    );
    // Three rows fit at the original's spacing; more are closed up.
    let step = if count > 3 {
        2.2 / (count - 1) as f32
    } else {
        1.0
    };
    let row = |n: usize| {
        let n = n as f32 * step;
        Rect::from_corners(
            first.min + (second.min - first.min) * n,
            first.max + (second.max - first.max) * n,
        )
    };
    (0..count).map(row).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
    Go(Page),
    Race,
    Quit,
    /// Shown, but not something the port can do.
    Nothing,
    Circuit,
    RaceChoice,
    /// Which circuit to race for, and the go-ahead.
    Series,
    StartSeries,
    /// Off against the clock.
    TimeRace,
    Opponents,
    Laps,
    Difficulty,
    Music,
    Sound,
    /// One of the port's own settings.
    Extra(Extra),
    /// Online: somewhere to type, hosting a session, picking one off the list and
    /// dialling it, reading the list again, and in the room being ready, starting
    /// the race, and leaving.
    Type(Typed),
    BeginHosting,
    Pick(usize),
    Dial,
    Refresh,
    Ready,
    Begin,
    Leave,
    /// Who to race as online.
    Car,
    /// The host's: how many the session takes, and putting a player out of it.
    Limit,
    Remove(net::protocol::Peer),
    /// The host's too: whether the session is on the list, how many races its
    /// series is of, and beginning the points again.
    Listed,
    Length,
    ResetPoints,
    /// Going to the race that is on, and finding the session a code is for.
    Enter,
    Find,
    /// A vote for one of the circuits, to race it next.
    Map(usize),
    /// Something of the garage's.
    Bench(workshop::Act),
    /// On the controls page: which set of bindings is shown, and giving one of the
    /// things bound another key or button.
    Device,
    Bind(usize),
    /// Taking what has been won, and going on to the main menu.
    Collect,
}

enum Widget {
    /// A picture and some words: the picture is `icon` or nothing.
    Button {
        at: Vec2,
        label: String,
        icon: Option<&'static str>,
    },
    /// Arrows either side of a picture or some words.
    Selector {
        area: Rect,
        picture: Option<String>,
        words: String,
    },
    /// A rail with a thumb on it, `value` steps of `MAX_VOLUME` along.
    Slider { area: Rect, value: usize },
    /// A box with what has been typed into it.
    Field { area: Rect, words: String },
}

struct Item {
    widget: Widget,
    action: Action,
    enabled: bool,
}

/// Everything the menus are drawn with.
#[derive(Resource)]
pub struct Art {
    jam: Jam,
    fonts: HashMap<String, Font>,
    strings: Vec<String>,
    /// Colour keys of the pictures, from the image lists (`.IDB`).
    keys: HashMap<String, Option<[u8; 3]>>,
    /// Where each screen puts its widgets: `[left, top, right, bottom]` by name.
    layouts: HashMap<&'static str, HashMap<String, [f32; 4]>>,
    pictures: HashMap<String, (Handle<Image>, Vec2)>,
    written: HashMap<(String, String, bool), (Handle<Image>, Vec2)>,
}

#[derive(Resource, Default)]
struct Menu {
    page: Page,
    focus: usize,
    drawn: bool,
}

#[derive(Component)]
struct Root;

pub fn plugin(app: &mut App) {
    // Loaded here rather than in a system, so that it is there before the first
    // screen is entered and the plain menu knows not to appear.
    if let Some(art) = load_art() {
        app.insert_resource(art);
    }
    app.init_resource::<Menu>()
        .init_resource::<Online>()
        .init_resource::<Bench>()
        .init_resource::<Portraits>()
        .init_resource::<Mascot>()
        .add_systems(OnEnter(Screen::Menu), enter)
        .add_systems(
            OnExit(Screen::Menu),
            (
                leave,
                workshop::put_away,
                portraits::put_away,
                mascot::put_away,
            ),
        )
        .add_systems(
            Update,
            (
                input,
                arrive,
                ride,
                portraits::keep,
                mascot::keep,
                mascot::dress,
                draw,
                workshop::show,
            )
                .chain()
                .run_if(in_state(Screen::Menu))
                .run_if(resource_exists::<Art>),
        );
}

/// The rectangles a layout file gives its widgets: `key "name" { 0x36 { 0x2f l t r b`.
fn layout(data: &[u8]) -> HashMap<String, [f32; 4]> {
    let tokens = tokenize(data);
    let number = |at: usize| match tokens.get(at) {
        Some(Token::Int(v)) => *v as f32,
        Some(Token::Float(v)) => *v,
        _ => 0.0,
    };
    let mut out = HashMap::new();
    for (i, token) in tokens.iter().enumerate() {
        let (Token::Key(_), Some(Token::Str(name)), Some(Token::LCurly), Some(Token::Key(0x36))) = (
            token,
            tokens.get(i + 1),
            tokens.get(i + 2),
            tokens.get(i + 3),
        ) else {
            continue;
        };
        if tokens.get(i + 5) == Some(&Token::Key(0x2f)) {
            out.insert(
                name.to_lowercase(),
                [number(i + 6), number(i + 7), number(i + 8), number(i + 9)],
            );
        }
    }
    out
}

fn load_art() -> Option<Art> {
    let path =
        std::env::var("BRICK_JAM").unwrap_or("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM".into());
    let jam = Jam::open(path)?;
    let fonts = load_fonts(&jam, &format!("{DIR}/ENGLISH"), "GFONTS.FDB");
    let strings = jam
        .get(&format!("{DIR}/ENGLISH/MENUTEXT.SRF"))
        .map(load_strings)
        .unwrap_or_default();
    let mut layouts = HashMap::new();
    for (screen, file) in [
        ("main", "MAINMENU"),
        ("race", "SINGRACE"),
        ("options", "OPTIONS"),
        ("control", "CONTROL"),
        ("garage", "GARAGE"),
        ("editdrvr", "EDITDRVR"),
        ("drvrlice", "DRVRLICE"),
        ("editcar", "EDITCAR"),
        ("carbuild", "CARBUILD"),
    ] {
        layouts.insert(
            screen,
            jam.get(&format!("{DIR}/{file}.MIB"))
                .map(layout)
                .unwrap_or_default(),
        );
    }
    // `0x27 "name" { 0x29 [0x2b r g b] }`: a picture and the colour that is see-through.
    let mut keys = HashMap::new();
    for list in ["GIMAGES", "SINGRACE", "OPTIONS", "CONTROL", "BUILDER", "DRVRLICE"] {
        let tokens = tokenize(jam.get(&format!("{DIR}/{list}.IDB")).unwrap_or_default());
        for (i, token) in tokens.iter().enumerate() {
            let (Token::Key(0x27), Some(Token::Str(name)), Some(Token::LCurly)) =
                (token, tokens.get(i + 1), tokens.get(i + 2))
            else {
                continue;
            };
            let value = |at: usize| match tokens.get(at) {
                Some(Token::Int(v)) => *v as u8,
                _ => 0,
            };
            let key = (tokens.get(i + 4) == Some(&Token::Key(0x2b)))
                .then(|| [value(i + 5), value(i + 6), value(i + 7)]);
            keys.entry(name.to_lowercase()).or_insert(key);
        }
    }
    if fonts.is_empty() || strings.is_empty() || layouts.values().any(HashMap::is_empty) {
        return None;
    }
    Some(Art {
        jam,
        fonts,
        strings,
        keys,
        layouts,
        pictures: HashMap::new(),
        written: HashMap::new(),
    })
}

fn image(
    pixels: crate::assets::image::Pixels,
    images: &mut Assets<Image>,
) -> (Handle<Image>, Vec2) {
    let size = Vec2::new(pixels.width as f32, pixels.height as f32);
    let extent = Extent3d {
        width: pixels.width,
        height: pixels.height,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new(
        extent,
        TextureDimension::D2,
        pixels.rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // Crisp, as on the original's low-resolution screen.
    image.sampler = ImageSampler::nearest();
    (images.add(image), size)
}

impl Art {
    fn picture(&mut self, name: &str, images: &mut Assets<Image>) -> Option<(Handle<Image>, Vec2)> {
        if !self.pictures.contains_key(name) {
            let key = self.keys.get(name).copied().flatten();
            let pixels = decode_bmp(self.jam.get(&format!("{DIR}/{name}.BMP"))?, key)?;
            self.pictures
                .insert(name.to_string(), image(pixels, images));
        }
        self.pictures.get(name).cloned()
    }

    /// The game's archive.
    pub fn jam(&self) -> &Jam {
        &self.jam
    }

    /// Words in one of the game's fonts, as a picture and its size.
    pub fn write(
        &mut self,
        font: &str,
        words: &str,
        centred: bool,
        images: &mut Assets<Image>,
    ) -> Option<(Handle<Image>, Vec2)> {
        let key = (font.to_string(), words.to_string(), centred);
        if !self.written.contains_key(&key) {
            let pixels = self.fonts.get(font)?.render(words, centred);
            self.written.insert(key.clone(), image(pixels, images));
        }
        self.written.get(&key).cloned()
    }

    fn string(&self, index: usize) -> String {
        self.strings.get(index).cloned().unwrap_or_default()
    }

    /// A widget's rectangle on a screen.
    fn place(&self, screen: &str, name: &str) -> Rect {
        let [left, top, right, bottom] = self
            .layouts
            .get(screen)
            .and_then(|l| l.get(name))
            .copied()
            .unwrap_or_default();
        Rect::new(left, top, right.max(left), bottom.max(top))
    }
}

/// Which of the game's circuits a race belongs to.
fn group(circuits: &Circuits, index: usize) -> usize {
    circuits.0[index].group.min(CIRCUIT_ICONS.len() - 1)
}

/// The widgets of a page that can be chosen or changed, top to bottom.
fn items(
    page: Page,
    art: &Art,
    circuits: &Circuits,
    settings: &Settings,
    championship: &Championship,
    wired: &Wired,
    garage: &Garage,
    bench: &Bench,
) -> Vec<Item> {
    if workshop::mine(page) {
        let online = wired.role != Role::Offline;
        return workshop::items(page, art, bench, garage, settings, wired.progress, online);
    }
    let button =
        |screen: &str, name: &str, label: usize, action: Action, icon: Option<&'static str>| Item {
            widget: Widget::Button {
                at: art.place(screen, name).min,
                label: art.string(label),
                icon,
            },
            action,
            enabled: action != Action::Nothing,
        };
    let selector = |area: Rect, picture: Option<String>, words: String, action: Action| Item {
        widget: Widget::Selector {
            area,
            picture,
            words,
        },
        action,
        enabled: true,
    };
    let back = |screen: &str, to: Page| {
        button(
            screen,
            "goback",
            text::MAIN_MENU,
            Action::Go(to),
            Some("txtarol"),
        )
    };
    match page {
        Page::Main => vec![
            button(
                "main",
                "garage",
                text::BUILD,
                Action::Go(Page::Garage),
                None,
            ),
            button(
                "main",
                "circuit",
                text::CIRCUIT_RACE,
                Action::Go(Page::CircuitRace),
                None,
            ),
            button(
                "main",
                "single",
                text::SINGLE_RACE,
                Action::Go(Page::SingleRace),
                None,
            ),
            // Where the original races two on one screen, the port races online.
            Item {
                widget: Widget::Button {
                    at: art.place("main", "vs").min,
                    label: "ONLINE RACE".into(),
                    icon: None,
                },
                action: Action::Go(Page::Online),
                enabled: true,
            },
            button(
                "main",
                "time",
                text::TIME_RACE,
                Action::Go(Page::TimeRace),
                None,
            ),
            button(
                "main",
                "options",
                text::OPTIONS,
                Action::Go(Page::Options),
                None,
            ),
            button("main", "quit", text::QUIT, Action::Quit, None),
        ],
        Page::SingleRace | Page::TimeRace => {
            let go = if page == Page::TimeRace {
                Action::TimeRace
            } else {
                Action::Race
            };
            let icon = CIRCUIT_ICONS[group(circuits, settings.circuit)].to_string();
            let name = circuits.0[settings.circuit].name.clone();
            vec![
                selector(
                    art.place("race", "selector"),
                    Some(icon),
                    String::new(),
                    Action::Circuit,
                ),
                selector(art.place("race", "racesel"), None, name, Action::RaceChoice),
                button("race", "gonext", text::OK, go, Some("chck")),
                back("race", Page::Main),
            ]
        }
        Page::CircuitRace => {
            // The single race page's widgets: the circuit's picture, and whose it is.
            let chosen = championship
                .chosen
                .min(championship.series.len().saturating_sub(1));
            let icon = SERIES_ICONS[chosen % SERIES_ICONS.len()].to_string();
            let words = match championship.series.get(chosen) {
                Some(series) if chosen < championship.unlocked => {
                    format!("{}: {}", chosen + 1, series.champion)
                }
                Some(_) => format!("{}: LOCKED", chosen + 1),
                None => String::new(),
            };
            let mut start = button(
                "race",
                "gonext",
                text::OK,
                Action::StartSeries,
                Some("chck"),
            );
            start.enabled = chosen < championship.unlocked;
            vec![
                selector(
                    art.place("race", "selector"),
                    Some(icon),
                    String::new(),
                    Action::Series,
                ),
                selector(art.place("race", "racesel"), None, words, Action::Series),
                start,
                back("race", Page::Main),
            ]
        }
        Page::Options => vec![
            // The port's own page, a row above the original's first.
            Item {
                widget: Widget::Button {
                    at: art.place("options", "game").min * 2.0 - art.place("options", "video").min,
                    label: "EXTRAS".into(),
                    icon: None,
                },
                action: Action::Go(Page::Extras),
                enabled: true,
            },
            button(
                "options",
                "game",
                text::GAME_OPTIONS,
                Action::Go(Page::GameOptions),
                None,
            ),
            button(
                "options",
                "video",
                text::VIDEO_OPTIONS,
                Action::Go(Page::VideoOptions),
                None,
            ),
            button(
                "options",
                "audio",
                text::AUDIO_OPTIONS,
                Action::Go(Page::AudioOptions),
                None,
            ),
            button(
                "options",
                "player1",
                text::CONTROLS[0],
                Action::Go(Page::Controls),
                None,
            ),
            button(
                "options",
                "player2",
                text::CONTROLS[1],
                Action::Nothing,
                None,
            ),
            button("options", "language", text::LANGUAGE, Action::Nothing, None),
            button("options", "credits", text::CREDITS, Action::Nothing, None),
            back("options", Page::Main),
        ],
        Page::GameOptions => {
            // The original has two selectors on this page; the port's own settings go
            // in more of the same, a row apiece.
            let (first, second) = (
                art.place("options", "chmpcont"),
                art.place("options", "lapcont"),
            );
            let row = |n: f32| {
                Rect::from_corners(
                    first.min + (second.min - first.min) * n,
                    first.max + (second.max - first.max) * n,
                )
            };
            vec![
                selector(
                    row(0.0),
                    None,
                    settings.opponents.to_string(),
                    Action::Opponents,
                ),
                selector(row(1.0), None, settings.laps().to_string(), Action::Laps),
                selector(
                    row(2.0),
                    None,
                    DIFFICULTIES[settings.difficulty].0.to_string(),
                    Action::Difficulty,
                ),
                back("options", Page::Options),
            ]
        }
        Page::VideoOptions | Page::Extras => {
            let extras = page.extras();
            let mut items: Vec<Item> = rows(art, extras.len(), ["chmpcont", "lapcont"])
                .into_iter()
                .zip(extras)
                .map(|(area, &extra)| {
                    selector(area, None, settings.shown(extra), Action::Extra(extra))
                })
                .collect();
            items.push(back("options", Page::Options));
            items
        }
        Page::Controls => {
            // `ControlConfigScreen::CreateWidgets`: the device, the nine things bound
            // and the way out. A pad is steered with its stick, and has no button to
            // give turning (`ControlConfigScreen::Update`).
            let entry = wired.devices.entry();
            let device = selector(
                art.place("control", "contcon"),
                Some(if entry == PAD { "gamepad" } else { "keyboard" }.into()),
                String::new(),
                Action::Device,
            );
            let binds = CONTROL_ROWS.iter().enumerate().map(|(event, (name, _))| {
                let stick = entry == PAD && event < 2;
                let label = if wired.devices.awaiting == Some(event) {
                    ".......".into()
                } else if stick {
                    "STICK".into()
                } else {
                    crate::input::name(settings.controls.0[entry][event])
                };
                Item {
                    widget: Widget::Button {
                        at: art.place("control", name).min,
                        label,
                        icon: None,
                    },
                    action: Action::Bind(event),
                    enabled: !stick,
                }
            });
            let out = Item {
                widget: Widget::Button {
                    at: art.place("control", "goback").min,
                    label: art.string(text::FINISH),
                    icon: Some("txtarol"),
                },
                action: Action::Go(Page::Options),
                enabled: true,
            };
            std::iter::once(device)
                .chain(binds)
                .chain(std::iter::once(out))
                .collect()
        }
        Page::Award => vec![Item {
            widget: Widget::Button {
                at: art.place("options", "goback").min,
                label: art.string(text::OK),
                icon: None,
            },
            action: Action::Collect,
            enabled: true,
        }],
        Page::AudioOptions => vec![
            Item {
                widget: Widget::Slider {
                    area: art.place("options", "musicvol"),
                    value: settings.music,
                },
                action: Action::Music,
                enabled: true,
            },
            Item {
                widget: Widget::Slider {
                    area: art.place("options", "soundvol"),
                    value: settings.sound,
                },
                action: Action::Sound,
                enabled: true,
            },
            back("options", Page::Options),
        ],
        Page::Online => vec![
            field(FIELD[0], settings.name.clone(), Typed::Name),
            selector(
                Rect::from_corners(FIELD[1].min - Vec2::X * ICON, FIELD[1].max + Vec2::X * ICON),
                None,
                racing_as(settings, garage),
                Action::Car,
            ),
            plain(Vec2::new(3.0, 178.0), "HOST A RACE", Action::Go(Page::Host)),
            plain(Vec2::new(3.0, 218.0), "JOIN A RACE", Action::Go(Page::Join)),
            back("options", Page::Main),
        ],
        Page::Host => vec![
            field(FIELD[0], wired.online.title.clone(), Typed::Title),
            field(FIELD[1], wired.online.password.clone(), Typed::Password),
            selector(
                wide(line(2)),
                None,
                listed(wired.online.unlisted).into(),
                Action::Listed,
            ),
            Item {
                widget: Widget::Button {
                    at: art.place("race", "gonext").min,
                    label: art.string(text::OK),
                    icon: Some("chck"),
                },
                action: Action::BeginHosting,
                enabled: !wired.online.title.trim().is_empty(),
            },
            way_back(art, Page::Online),
        ],
        Page::Join => {
            let mut items: Vec<Item> = wired
                .lobby
                .sessions
                .iter()
                .take(LISTED)
                .enumerate()
                .map(|(n, listed)| {
                    let mut label = format!(
                        "{} - {} {}/{}",
                        listed.name, listed.host, listed.status.players, listed.max
                    );
                    for (so, word) in [
                        (listed.locked, " LOCKED"),
                        (listed.status.racing, " RACING"),
                    ] {
                        if so {
                            label += word;
                        }
                    }
                    Item {
                        widget: Widget::Button {
                            at: Vec2::new(3.0, 80.0 + 36.0 * n as f32),
                            label,
                            icon: None,
                        },
                        action: Action::Pick(n),
                        enabled: listed.status.players < listed.max,
                    }
                })
                .collect();
            let refresh = art.place("race", "gonext").min;
            items.push(plain(refresh, "REFRESH", Action::Refresh));
            items.push(plain(
                Vec2::new(250.0, refresh.y),
                "ENTER A CODE",
                Action::Go(Page::Code),
            ));
            items.push(way_back(art, Page::Online));
            items
        }
        Page::Code => vec![
            field(FIELD[0], wired.online.code.clone(), Typed::Code),
            Item {
                widget: Widget::Button {
                    at: art.place("race", "gonext").min,
                    label: art.string(text::OK),
                    icon: Some("chck"),
                },
                action: Action::Find,
                enabled: wired.online.code.len() == lobby_api::CODE_LENGTH,
            },
            way_back(art, Page::Join),
        ],
        Page::Password => vec![
            field(FIELD[0], wired.online.key.clone(), Typed::Key),
            Item {
                widget: Widget::Button {
                    at: art.place("race", "gonext").min,
                    label: art.string(text::OK),
                    icon: Some("chck"),
                },
                action: Action::Dial,
                enabled: true,
            },
            way_back(art, Page::Join),
        ],
        Page::Connecting => vec![Item {
            widget: Widget::Button {
                at: art.place("race", "goback").min,
                label: "CANCEL".into(),
                icon: Some("txtarol"),
            },
            action: Action::Leave,
            enabled: true,
        }],
        Page::Wishes => {
            // Every circuit there is, to be voted for.
            let mut items: Vec<Item> = circuits
                .0
                .iter()
                .enumerate()
                .map(|(n, circuit)| {
                    plain(
                        map_place(n, circuits.0.len()),
                        &circuit.name,
                        Action::Map(n),
                    )
                })
                .collect();
            items.push(way_back(art, Page::Room));
            items
        }
        Page::Rules => {
            let wish = wired.wish(settings, circuits);
            let row = |n: usize| {
                Rect::new(
                    164.0,
                    ROOM_TOP + ROOM_STEP * n as f32,
                    ROOM_ROWS_END,
                    ROOM_TOP + ROOM_STEP * n as f32 + ICON,
                )
            };
            let mut items = vec![
                selector(row(0), None, wish.laps().to_string(), Action::Laps),
                selector(row(1), None, wish.opponents.to_string(), Action::Opponents),
            ];
            items.extend(VOTED.iter().enumerate().map(|(n, &extra)| {
                selector(row(2 + n), None, wish.shown(extra), Action::Extra(extra))
            }));
            items.push(way_back(art, Page::Room));
            items
        }
        Page::Room => {
            // The next race's circuit is voted for on a page of its own; what to
            // race as is the player's own affair, and not voted on.
            let mut items = vec![
                plain(
                    Vec2::new(3.0, ROOM_RACE),
                    "NEXT RACE",
                    Action::Go(Page::Wishes),
                ),
                selector(
                    Rect::new(164.0, ROOM_RIDE, ROOM_ROWS_END, ROOM_RIDE + ICON),
                    None,
                    racing_as(settings, garage),
                    Action::Car,
                ),
            ];
            items.push(plain(
                Vec2::new(3.0, ROOM_BUTTONS),
                if wired.room.ready {
                    "READY: YES"
                } else {
                    "READY: NO"
                },
                Action::Ready,
            ));
            let below = ROOM_BUTTONS + ROOM_STEP;
            if wired.role == Role::Host {
                items.push(plain(Vec2::new(3.0, below), "START NOW", Action::Begin));
            } else if wired.room.racing {
                items.push(plain(
                    Vec2::new(3.0, below),
                    "GO TO THE RACE",
                    Action::Enter,
                ));
            }
            if !wired.room.results.is_empty() {
                items.push(plain(
                    Vec2::new(155.0, below),
                    "LAST RACE",
                    Action::Go(Page::Results),
                ));
            }
            if wired.role == Role::Host {
                items.push(plain(
                    Vec2::new(258.0, below),
                    "SESSION",
                    Action::Go(Page::Session),
                ));
            }
            // How the race is run, its circuit apart, is the host's to say.
            if wired.role == Role::Host {
                items.push(plain(
                    Vec2::new(440.0, below),
                    "RULES",
                    Action::Go(Page::Rules),
                ));
            }
            // To the garage, to build a racer or change one, and back here.
            items.push(plain(
                Vec2::new(354.0, below),
                "BUILD",
                Action::Go(Page::Garage),
            ));
            items.push(field(CHAT, wired.online.say.clone(), Typed::Say));
            items.push(Item {
                widget: Widget::Button {
                    at: art.place("race", "goback").min,
                    label: "LEAVE".into(),
                    icon: Some("txtarol"),
                },
                action: Action::Leave,
                enabled: true,
            });
            items
        }
        Page::Results => vec![Item {
            widget: Widget::Button {
                at: art.place("race", "gonext").min,
                label: art.string(text::OK),
                icon: Some("chck"),
            },
            action: Action::Go(Page::Room),
            enabled: true,
        }],
        Page::Session => {
            let mut items = vec![
                field(FIELD[0], wired.session.password.clone(), Typed::Lock),
                selector(
                    Rect::from_corners(
                        FIELD[1].min - Vec2::X * ICON,
                        FIELD[1].max + Vec2::X * ICON,
                    ),
                    None,
                    wired.session.most().to_string(),
                    Action::Limit,
                ),
                selector(
                    wide(line(2)),
                    None,
                    match wired.session.series {
                        0 => "OFF".to_string(),
                        races => format!("{races} RACES"),
                    },
                    Action::Length,
                ),
                selector(
                    wide(line(3)),
                    None,
                    listed(wired.session.unlisted).into(),
                    Action::Listed,
                ),
            ];
            // Each player, to be put out: asked twice, so that it isn't done by a slip.
            items.extend(wired.session.members.iter().enumerate().map(|(n, member)| {
                let label = if wired.online.removing == Some(member.peer) {
                    format!("REMOVE {}? AGAIN TO DO IT", member.name)
                } else {
                    format!("REMOVE {}", member.name)
                };
                plain(
                    Vec2::new(3.0, 262.0 + 32.0 * n as f32),
                    &label,
                    Action::Remove(member.peer),
                )
            }));
            let back = art.place("race", "goback").min;
            items.push(plain(
                Vec2::new(330.0, back.y),
                "RESET POINTS",
                Action::ResetPoints,
            ));
            items.push(way_back(art, Page::Room));
            items
        }
        // The garage's pages, which `workshop::items` has already answered for.
        _ => Vec::new(),
    }
}

/// Where the online pages put what is typed, and beside it what it is.
const FIELD: [Rect; 2] = [
    Rect {
        min: Vec2::new(190.0, 96.0),
        max: Vec2::new(480.0, 128.0),
    },
    Rect {
        min: Vec2::new(190.0, 136.0),
        max: Vec2::new(480.0, 168.0),
    },
];
/// Where the room's rows begin and how far apart they are.
const ROOM_TOP: f32 = 76.0;
const ROOM_STEP: f32 = 32.0;
/// Which of the room's widgets is "ready", where the room opens: after the way to
/// the next race's settings and what to race as.
const READY_AT: usize = 2;
/// Where the rows of the next race's settings end, with how many agree on each
/// after that.
const ROOM_ROWS_END: f32 = 440.0;
/// In the room, how high the minifigures of those in it are, and where under them
/// the way to the next race's settings and what to race as go.
const FIGURE_HEIGHT: f32 = 112.0;
const ROOM_RACE: f32 = 240.0;
const ROOM_RIDE: f32 = 272.0;
/// Where the room's buttons begin, under its rows; where what is to be said to it
/// is typed; and where what has been said is shown, and how far apart its lines are.
const ROOM_BUTTONS: f32 = 304.0;
const CHAT: Rect = Rect {
    min: Vec2::new(110.0, 372.0),
    max: Vec2::new(636.0, 400.0),
};
const CHAT_TOP: f32 = 404.0;
const CHAT_STEP: f32 = 24.0;

/// A further row under the two of `FIELD`, and a row made wide enough for a
/// selector's arrows.
fn line(n: usize) -> Rect {
    let down = Vec2::Y * 40.0 * n as f32;
    Rect::from_corners(FIELD[0].min + down, FIELD[0].max + down)
}

fn wide(area: Rect) -> Rect {
    Rect::from_corners(area.min - Vec2::X * ICON, area.max + Vec2::X * ICON)
}

/// Whether a session is on the lobby's list, as the menu says it.
fn listed(unlisted: bool) -> &'static str {
    if unlisted { "CODE ONLY" } else { "ON THE LIST" }
}
/// Where the circuits to be voted for end, down the screen.
const MAPS_END: f32 = 308.0;

/// Where the `n`th of `count` circuits to be voted for goes: in two columns, down
/// the first and then the second.
fn map_place(n: usize, count: usize) -> Vec2 {
    let rows = count.div_ceil(2).max(1);
    let step = ((MAPS_END - ROOM_TOP) / rows as f32).min(ROOM_STEP);
    Vec2::new(
        3.0 + (n / rows) as f32 * SCREEN.x / 2.0,
        ROOM_TOP + step * (n % rows) as f32,
    )
}

/// How high a line of writing under a minifigure is, and the most of the screen's
/// width any one of those in the room is given.
const VOTER_LINE: f32 = 24.0;
const VOTER_MOST: f32 = 160.0;

/// Where the `n`th of `count` in the room goes: their minifigure, their name under
/// it, and under that how long their way to the host takes. Side by side across
/// the screen, in the middle of it.
fn voter_places(n: usize, count: usize) -> (Rect, Rect, Rect) {
    let wide = (SCREEN.x / count.max(1) as f32).min(VOTER_MOST);
    let left = (SCREEN.x - wide * count as f32) / 2.0 + wide * n as f32;
    let figure = FIGURE_HEIGHT * portraits::SIZE.x / portraits::SIZE.y;
    let line = |row: f32| {
        let top = ROOM_TOP + FIGURE_HEIGHT + row * VOTER_LINE;
        Rect::new(left, top, left + wide, top + VOTER_LINE)
    };
    (
        Rect::new(
            left + (wide - figure) / 2.0,
            ROOM_TOP,
            left + (wide + figure) / 2.0,
            ROOM_TOP + FIGURE_HEIGHT,
        ),
        line(0.0),
        line(1.0),
    )
}

/// Where the rows of the last race's results begin, how far apart they are, and
/// where each column begins: place, driver, time, how far behind, best lap, points.
const RESULTS_TOP: f32 = 84.0;
const RESULT_STEP: f32 = 30.0;
const RESULT_COLUMNS: [f32; 6] = [16.0, 48.0, 284.0, 374.0, 474.0, 574.0];

fn field(area: Rect, words: String, typed: Typed) -> Item {
    Item {
        widget: Widget::Field { area, words },
        action: Action::Type(typed),
        enabled: true,
    }
}

/// The way back from one of the online pages to the one before it.
fn way_back(art: &Art, to: Page) -> Item {
    Item {
        widget: Widget::Button {
            at: art.place("race", "goback").min,
            label: "BACK".into(),
            icon: Some("txtarol"),
        },
        action: Action::Go(to),
        enabled: true,
    }
}

/// Who the player races as online, as the menu says it.
fn racing_as(settings: &Settings, garage: &Garage) -> String {
    match garage.ride(settings) {
        Ride::Slot => "ANYONE".into(),
        Ride::Driver(code) => {
            let driver = crate::roster::NAMES.iter().find(|driver| driver.0 == code);
            driver.map_or("ANYONE", |driver| driver.1).into()
        }
        Ride::Built(racer) => racer.name,
    }
}

/// A button of the port's own, with words the original hasn't a string for.
fn plain(at: Vec2, label: &str, action: Action) -> Item {
    Item {
        widget: Widget::Button {
            at,
            label: label.into(),
            icon: None,
        },
        action,
        enabled: true,
    }
}

/// What the online pages say beside and around their widgets: where, what, in which
/// font and colour, and whether centred there.
fn notes(
    page: Page,
    art: &Art,
    wired: &Wired,
    settings: &Settings,
    circuits: &Circuits,
) -> Vec<(Rect, String, &'static str, Color, bool)> {
    let banner = |words: &str| {
        (
            Rect::new(375.0, 20.0, 375.0, 68.0),
            words.to_string(),
            "fontmenu",
            LABEL,
            true,
        )
    };
    let beside = |area: Rect, words: &str| {
        (
            Rect::new(8.0, area.min.y, area.min.x - 8.0, area.max.y),
            words.to_string(),
            "font_ths",
            LABEL,
            false,
        )
    };
    let middle = |words: &str| {
        (
            Rect::new(320.0, 300.0, 320.0, 332.0),
            words.to_string(),
            "font_ths",
            LABEL,
            true,
        )
    };
    match page {
        Page::Award => {
            let Some(award) = wired.progress.award else {
                return Vec::new();
            };
            let line = |row: f32, words: String| {
                let at = Rect::new(320.0, 250.0 + row * 34.0, 320.0, 282.0 + row * 34.0);
                (at, words, "font_ths", LABEL, true)
            };
            let title = match award.place {
                Some(place) => format!("{} PLACE", PLACES[(place - 1).min(PLACES.len() - 1)]),
                None => art.string(text::TIME_TRIAL_WON),
            };
            let mut notes = vec![banner(&title)];
            let mut row = 0.0;
            if award.circuit {
                // The game's own words for it are one line too long for the screen.
                let words = art.string(text::NEW_CIRCUIT).replacen("! ", "!\n", 1);
                notes.push(line(row, words));
                row += 2.0;
            }
            if award.parts.is_some() {
                notes.push(line(row, "NEW BRICKS TO BUILD WITH".into()));
            }
            notes
        }
        Page::Controls => CONTROL_ROWS
            .iter()
            .enumerate()
            .map(|(event, (_, name))| {
                let at = art.place("control", name).min;
                (
                    Rect::from_corners(at, at + Vec2::new(0.0, ICON)),
                    art.string(text::EVENTS + event),
                    "font_ths",
                    LABEL,
                    false,
                )
            })
            .collect(),
        Page::Online => {
            let mut notes = vec![
                banner("ONLINE RACE"),
                beside(FIELD[0], "YOUR NAME"),
                beside(FIELD[1], "RACING AS"),
            ];
            // Why the last session ended, if it was not left by choice.
            notes.extend(wired.session.notice.as_deref().map(middle));
            notes
        }
        Page::Host => vec![
            banner("HOST A RACE"),
            beside(FIELD[0], "CALLED"),
            beside(FIELD[1], "PASSWORD"),
            beside(line(2), "FOUND"),
            middle("LEAVE THE PASSWORD EMPTY TO LET ANYONE IN"),
        ],
        Page::Code => {
            let mut notes = vec![banner("JOIN A RACE"), beside(FIELD[0], "CODE")];
            notes.extend(match wired.online.seeking {
                Some(true) => Some(middle("ASKING THE LOBBY")),
                Some(false) => Some(middle("NO SESSION HAS THAT CODE")),
                None => None,
            });
            notes
        }
        Page::Join => {
            let mut notes = vec![banner("JOIN A RACE")];
            if let Some(trouble) = wired
                .lobby
                .trouble
                .as_ref()
                .filter(|_| wired.lobby.sessions.is_empty())
            {
                bevy::log::debug!("{trouble}");
                notes.push(middle("THE LOBBY CAN'T BE REACHED"));
            } else if wired.lobby.sessions.is_empty() {
                notes.push(middle("NOBODY IS HOSTING A RACE"));
            }
            notes
        }
        Page::Password => vec![banner("JOIN A RACE"), beside(FIELD[0], "PASSWORD")],
        Page::Connecting => vec![banner("JOIN A RACE"), middle("CALLING THE HOST")],
        Page::Wishes => {
            let room = wired.room;
            let mut notes = vec![banner("NEXT RACE")];
            // Before each circuit, how many have voted for it: the player's own
            // vote in white.
            let mine = room
                .ballot
                .as_ref()
                .map(|_| wired.wish(settings, circuits).circuit);
            for (n, circuit) in circuits.0.iter().enumerate() {
                let votes = room.votes(net::protocol::key(circuit));
                if votes > 0 {
                    let corner = map_place(n, circuits.0.len());
                    notes.push((
                        Rect::from_corners(corner, corner + Vec2::splat(ICON)),
                        votes.to_string(),
                        "font_ths",
                        if Some(n) == mine { LABEL } else { NORMAL },
                        true,
                    ));
                }
            }
            // Under them, how the host has said the race is to be run.
            if let Some(rules) = room.hosts() {
                let mut said = vec![
                    format!(
                        "{} LAPS",
                        LAP_CHOICES[rules.lap_choice as usize % LAP_CHOICES.len()]
                    ),
                    format!("{} OPPONENTS", rules.opponents),
                ];
                said.extend(rules.mirror.then(|| "MIRRORED".to_string()));
                said.extend(rules.reverse.then(|| "REVERSED".to_string()));
                let bricks = crate::menu::BRICK_RULES.get(rules.bricks as usize);
                said.extend(
                    bricks
                        .filter(|_| rules.bricks > 0)
                        .map(|bricks| format!("BRICKS {bricks}")),
                );
                said.extend(rules.elimination.then(|| "ELIMINATION".to_string()));
                notes.push((
                    Rect::new(320.0, MAPS_END + 8.0, 320.0, MAPS_END + 8.0 + ICON),
                    said.join(", "),
                    "font_ths",
                    LABEL,
                    true,
                ));
            }
            notes
        }
        Page::Rules => {
            let mut notes = vec![banner("RACE RULES")];
            let label = |n: usize, words: &str| {
                (
                    Rect::new(
                        8.0,
                        ROOM_TOP + ROOM_STEP * n as f32,
                        150.0,
                        ROOM_TOP + ROOM_STEP * n as f32 + ICON,
                    ),
                    words.to_string(),
                    "font_ths",
                    LABEL,
                    false,
                )
            };
            notes.push(label(0, "LAPS"));
            notes.push(label(1, "OPPONENTS"));
            notes.extend(
                VOTED
                    .iter()
                    .enumerate()
                    .map(|(n, extra)| label(2 + n, extra.label())),
            );
            notes
        }
        Page::Room => {
            let room = wired.room;
            let mut notes = vec![banner(&wired.session.title)];
            // Beside the way to the vote, the circuit the player has voted for; and
            // what the selector under it is for.
            let wish = wired.wish(settings, circuits);
            let voted = match &room.ballot {
                Some(_) => circuits.0[wish.circuit].name.as_str(),
                None => "NONE YET",
            };
            notes.push((
                Rect::new(164.0, ROOM_RACE, SCREEN.x, ROOM_RACE + ICON),
                format!("YOUR VOTE: {voted}"),
                "font_ths",
                NORMAL,
                false,
            ));
            notes.push((
                Rect::new(8.0, ROOM_RIDE, 150.0, ROOM_RIDE + ICON),
                "RACING AS".to_string(),
                "font_ths",
                LABEL,
                false,
            ));
            // Who is here, under their minifigures: lit when ready, and under each
            // how long their way to the host takes.
            // Once anyone has scored, each has their points after their name.
            let scored = room.series.is_some() || room.voters.iter().any(|voter| voter.points > 0);
            for (n, voter) in room.voters.iter().enumerate() {
                let points = if scored {
                    format!(" {}", voter.points)
                } else {
                    String::new()
                };
                let (_, name, under) = voter_places(n, room.voters.len());
                notes.push((
                    name,
                    format!("{}{points}", voter.name),
                    "font_ths",
                    if voter.ready { SELECTED } else { NORMAL },
                    true,
                ));
                let link = match voter.link {
                    Some(link) => format!("{} MS", link.ping),
                    None if voter.peer == net::protocol::HOST => "HOST".to_string(),
                    None => String::new(),
                };
                // The player here is told which of them they are.
                let link = match (voter.peer == wired.session.you, link.is_empty()) {
                    (true, true) => "YOU".to_string(),
                    (true, false) => format!("YOU, {link}"),
                    (false, _) => link,
                };
                notes.push((under, link, "font_ths", LABEL, true));
            }
            // Beside "ready": the clock, a race that is on, how many are ready, or how
            // far the series has got.
            let ready = room.voters.iter().filter(|voter| voter.ready).count();
            let standing = match (room.closing, room.racing, room.series) {
                (Some(left), ..) => format!("RACE STARTS IN {}", left.max(0.0).ceil() as i32),
                (None, true, _) => "A RACE IS ON".to_string(),
                // Until most are ready no clock runs: how many are is said instead.
                (None, false, _) if ready > 0 => {
                    format!("{ready} OF {} READY", room.voters.len())
                }
                (None, false, Some((raced, of))) if raced >= of => "THE SERIES IS OVER".to_string(),
                (None, false, Some((raced, of))) => format!("RACE {} OF {of} NEXT", raced + 1),
                (None, false, None) => String::new(),
            };
            notes.push((
                Rect::new(200.0, ROOM_BUTTONS, 480.0, ROOM_BUTTONS + ROOM_STEP),
                standing,
                "font_ths",
                LABEL,
                true,
            ));
            // What has been said, and where to say something.
            notes.push((
                Rect::new(8.0, CHAT.min.y, CHAT.min.x, CHAT.max.y),
                "SAY".to_string(),
                "font_ths",
                LABEL,
                false,
            ));
            for (n, said) in room.chat.iter().enumerate() {
                let top = CHAT_TOP + CHAT_STEP * n as f32;
                notes.push((
                    Rect::new(CHAT.min.x, top, CHAT.max.x, top + CHAT_STEP),
                    said.clone(),
                    "font_ths",
                    LABEL,
                    false,
                ));
            }
            notes
        }
        Page::Results => {
            let room = wired.room;
            let mut notes = vec![banner("LAST RACE")];
            let cell = |row: usize, column: usize, words: String, colour: Color| {
                let (left, top) = (
                    RESULT_COLUMNS[column],
                    RESULTS_TOP + RESULT_STEP * row as f32,
                );
                (
                    Rect::new(left, top, left, top + RESULT_STEP),
                    words,
                    "font_ths",
                    colour,
                    false,
                )
            };
            for (column, heading) in ["", "", "TIME", "BEHIND", "BEST LAP", "PTS"]
                .into_iter()
                .enumerate()
            {
                notes.push(cell(0, column, heading.to_string(), LABEL));
            }
            let clock =
                |time: Option<f32>| time.map_or("-".to_string(), crate::hud::original::clock);
            let winner = room.results.first().and_then(|finish| finish.time);
            for (n, finish) in room.results.iter().enumerate() {
                // The players are lit, the player here brightest.
                let colour = match (finish.player, finish.name == wired.session.name) {
                    (true, true) => SELECTED,
                    (true, false) => LABEL,
                    (false, _) => NORMAL,
                };
                let behind = match (n, winner, finish.time) {
                    (1.., Some(winner), Some(time)) => {
                        format!("+{}", crate::hud::original::clock(time - winner))
                    }
                    _ => String::new(),
                };
                for (column, words) in [
                    (n + 1).to_string(),
                    finish.name.clone(),
                    clock(finish.time),
                    behind,
                    clock(finish.best),
                    format!("+{}", finish.points),
                ]
                .into_iter()
                .enumerate()
                {
                    notes.push(cell(n + 1, column, words, colour));
                }
            }
            // A series run to its end has a winner: whoever of the room has most.
            let leader = room.voters.iter().max_by_key(|voter| voter.points);
            if let (Some((raced, of)), Some(leader)) = (room.series, leader) {
                let words = if raced >= of {
                    format!("{} WINS THE SERIES WITH {}", leader.name, leader.points)
                } else {
                    format!("RACE {raced} OF {of}")
                };
                notes.push((
                    Rect::new(320.0, 300.0, 320.0, 332.0),
                    words,
                    "font_ths",
                    LABEL,
                    true,
                ));
            }
            notes
        }
        Page::Session => {
            let mut notes = vec![
                banner(&wired.session.title),
                beside(FIELD[0], "PASSWORD"),
                beside(FIELD[1], "PLAYERS"),
                beside(line(2), "SERIES"),
                beside(line(3), "FOUND"),
            ];
            if !wired.session.code.is_empty() {
                let code = format!("CODE {}", wired.session.code);
                notes.push((
                    Rect::new(375.0, 66.0, 375.0, 94.0),
                    code,
                    "font_ths",
                    SELECTED,
                    true,
                ));
            }
            if wired.session.members.is_empty() {
                notes.push(middle("NOBODY ELSE IS HERE"));
            }
            notes
        }
        _ => Vec::new(),
    }
}

/// Words that go beside the widgets of a page: (where, what, which font).
fn labels(page: Page, art: &Art) -> Vec<(Rect, String, &'static str)> {
    let beside = |name: &str, words: String| (art.place("options", name), words, "font_ths");
    let banner = |words: usize| {
        (
            Rect::new(375.0, 20.0, 375.0, 68.0),
            art.string(words),
            "fontmenu",
        )
    };
    match page {
        Page::Main => Vec::new(),
        Page::SingleRace => vec![banner(text::SINGLE_RACE)],
        Page::CircuitRace => vec![banner(text::CIRCUIT_RACE)],
        Page::TimeRace => vec![banner(text::TIME_RACE)],
        Page::Options => vec![banner(text::OPTIONS_BANNER)],
        Page::GameOptions => {
            let (first, second) = (
                art.place("options", "chmptext"),
                art.place("options", "laptext"),
            );
            let third =
                Rect::from_corners(second.min * 2.0 - first.min, second.max * 2.0 - first.max);
            vec![
                banner(text::GAME_OPTIONS),
                (first, art.string(text::OPPONENTS), "font_ths"),
                (second, "NUMBER OF LAPS".into(), "font_ths"),
                (third, "DIFFICULTY".into(), "font_ths"),
            ]
        }
        Page::VideoOptions | Page::Extras => {
            let extras = page.extras();
            let title = if page == Page::Extras {
                "EXTRAS".to_string()
            } else {
                art.string(text::VIDEO_OPTIONS)
            };
            let mut labels = vec![(Rect::new(375.0, 20.0, 375.0, 68.0), title, "fontmenu")];
            let places = rows(art, extras.len(), ["chmptext", "laptext"]);
            labels.extend(
                places
                    .into_iter()
                    .zip(extras)
                    .map(|(area, extra)| (area, extra.label().to_string(), "font_ths")),
            );
            labels
        }
        // What each binding is for is in `notes`, which can write from the left.
        Page::Controls => vec![banner(text::CONTROLS[0])],
        Page::AudioOptions => vec![
            banner(text::AUDIO_OPTIONS),
            beside("mvoltext", art.string(text::MUSIC_VOLUME)),
            beside("svoltext", art.string(text::SOUND_VOLUME)),
        ],
        // The port's online pages say what they have to in `notes`.
        Page::Online
        | Page::Host
        | Page::Join
        | Page::Password
        | Page::Connecting
        | Page::Room
        | Page::Results
        | Page::Session
        | Page::Wishes
        | Page::Rules
        | Page::Code => Vec::new(),
        // The garage's pages say what they have to in `workshop::notes`.
        _ => Vec::new(),
    }
}

fn enter(
    mut menu: ResMut<Menu>,
    mut session: ResMut<Session>,
    mut progress: ResMut<Progress>,
    mut showing: ResMut<Showing>,
    garage: Res<Garage>,
    settings: Res<Settings>,
    championship: Res<Championship>,
    art: Option<Res<Art>>,
) {
    *menu = Menu::default();
    menu.focus = 2;
    // `BRICK_MENU=award` shows what winning the first circuit for the first time
    // looks like, for screenshots.
    if std::env::var("BRICK_MENU").as_deref() == Ok("award") {
        progress.award = Some(crate::progress::Award {
            place: Some(1),
            circuit: true,
            parts: Some(0),
        });
    }
    // Something has just been won, and is said before anything else: a circuit
    // raced to the end has its film first (`AwardCinematicScreen`).
    if let Some(award) = progress.award {
        if let Some(place) = award.place {
            let racer = garage.racing(&settings).map(|racer| racer.cosmetics);
            let racer = racer.unwrap_or_default();
            showing.request = Some(Request::award(place, racer));
            // A part set won has a film of its own after that: the champion's
            // car, or for Rocket Racer's the racer who won it.
            let circuit = award.parts.and_then(|set| championship.series.get(set));
            if let (Some(circuit), Some(art)) = (circuit, &art) {
                showing.next = Request::car_set(art.jam(), &circuit.code, racer);
            }
        }
        (menu.page, menu.focus) = (Page::Award, 0);
        return;
    }
    // Out of a session, the way back in is where the menu opens.
    if std::mem::take(&mut session.left) || session.notice.is_some() {
        (menu.page, menu.focus) = (Page::Online, 2);
        return;
    }
    // `BRICK_MENU=race` (or options, game, audio) opens on that page, for screenshots.
    let page = match std::env::var("BRICK_MENU").as_deref() {
        Ok("race") => Page::SingleRace,
        Ok("circuit") => Page::CircuitRace,
        Ok("time") => Page::TimeRace,
        Ok("options") => Page::Options,
        Ok("game") => Page::GameOptions,
        Ok("audio") => Page::AudioOptions,
        Ok("video") => Page::VideoOptions,
        Ok("extras") => Page::Extras,
        Ok("controls") => Page::Controls,
        Ok("online") => Page::Online,
        Ok("host") => Page::Host,
        Ok("join") => Page::Join,
        Ok("garage") => Page::Garage,
        Ok("racer") => Page::Racer,
        Ok("driver") => Page::Driver,
        Ok("licence") => Page::Licence,
        Ok("car") => Page::Car,
        Ok("bricks") => Page::Bricks,
        _ => return,
    };
    (menu.page, menu.focus) = (page, page.first());
}

/// What the minifigure of whoever won the room's last race is made of: the racer a
/// player built, the game's driver they raced as, or the one of the computer's that
/// came first. Nothing if nobody finished.
fn winner(room: &Room, jam: &Jam) -> Option<Cosmetics> {
    let first = room.results.first().filter(|first| first.time.is_some())?;
    let driver = |code: &str| crate::roster::cosmetics_of(jam, code).unwrap_or_default();
    if !first.player {
        let names = crate::roster::NAMES.iter();
        let code = names.clone().find(|name| name.1 == first.name);
        return Some(code.map_or(Cosmetics::default(), |name| driver(name.0)));
    }
    let voter = room.voters.iter().find(|voter| voter.name == first.name);
    let ride = voter.and_then(|voter| room.rides.iter().find(|ride| ride.0 == voter.peer));
    Some(match ride.map(|ride| &ride.1) {
        Some(Ride::Built(racer)) => racer.cosmetics,
        Some(Ride::Driver(code)) => driver(code),
        // Whoever the circuit put in their place is not something the room is told.
        Some(Ride::Slot) | None => Cosmetics::default(),
    })
}

/// Makes the garage's bench ready for a page of its that has been come to.
fn arrive(
    art: Res<Art>,
    menu: Res<Menu>,
    garage: Res<Garage>,
    mut bench: ResMut<Bench>,
    mut settings: ResMut<Settings>,
    mut last: Local<Option<Page>>,
) {
    if *last != Some(menu.page) {
        // Back in a session's room from the garage, the racer it showed is raced as.
        let from_garage = last.is_some_and(workshop::mine);
        if from_garage && menu.page == Page::Room && settings.racer > 0 {
            settings.car = crate::roster::NAMES.len() + settings.racer;
        }
        *last = Some(menu.page);
        if workshop::mine(menu.page) {
            workshop::arrive(menu.page, &art, &mut bench, &garage, &mut settings);
        }
    }
}

/// In a session, what the player races as is whatever they have chosen to: the
/// host hears of it when it changes, a racer changed in the garage being one that has.
fn ride(
    settings: Res<Settings>,
    garage: Res<Garage>,
    role: Res<Role>,
    mut session: ResMut<Session>,
) {
    if *role != Role::Offline {
        let ride = garage.ride(&settings);
        if session.car != ride {
            session.car = ride;
        }
    }
}

fn leave(mut commands: Commands, roots: Query<Entity, With<Root>>, mut scale: ResMut<UiScale>) {
    for root in &roots {
        commands.entity(root).despawn();
    }
    scale.0 = 1.0;
}

/// Where the pointer is on the original's 640 by 480 screen.
fn pointer(window: &Window) -> Option<Vec2> {
    let size = Vec2::new(window.width(), window.height());
    let scale = (size / SCREEN).min_element();
    Some((window.cursor_position()? - (size - SCREEN * scale) / 2.0) / scale)
}

/// The parts of an item that can be pointed at: the whole of it, and for selectors
/// and sliders the arrow at each end.
fn hit(item: &Item, art: &Art, at: Vec2) -> Option<i32> {
    match &item.widget {
        Widget::Button {
            at: corner, label, ..
        } => {
            let width = art
                .fonts
                .get("font_ths")
                .map_or(0.0, |f| f.render(label, false).width as f32);
            Rect::from_corners(*corner, *corner + Vec2::new(ICON + width, ICON))
                .contains(at)
                .then_some(0)
        }
        Widget::Field { area, .. } => area.contains(at).then_some(0),
        Widget::Selector { area, .. } | Widget::Slider { area, .. } => {
            let end = if matches!(item.widget, Widget::Slider { .. }) {
                64.0
            } else {
                ICON
            };
            area.contains(at).then_some({
                if at.x < area.min.x + end {
                    -1
                } else if at.x > area.max.x - end {
                    1
                } else {
                    0
                }
            })
        }
    }
}

fn input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    art: Res<Art>,
    circuits: Res<Circuits>,
    mut settings: ResMut<Settings>,
    mut menu: ResMut<Menu>,
    mut next: ResMut<NextState<Screen>>,
    mut sfx: ResMut<Sfx>,
    mut exit: MessageWriter<AppExit>,
    mut pointed: Local<Option<Vec2>>,
    mut championship: ResMut<Championship>,
    mut commands: Commands,
    mut typed: MessageReader<KeyboardInput>,
    time: Res<Time<Real>>,
    (
        role,
        mut session,
        mut room,
        mut lobby,
        mut online,
        mut wire,
        mut garage,
        mut bench,
        mut devices,
        mut progress,
        mut showing,
    ): (
        Res<Role>,
        ResMut<Session>,
        ResMut<Room>,
        ResMut<Lobby>,
        ResMut<Online>,
        Option<ResMut<net::Wire>>,
        ResMut<Garage>,
        ResMut<Bench>,
        ResMut<Devices>,
        ResMut<Progress>,
        ResMut<Showing>,
    ),
) {
    // While a film is shown the keys are its own.
    if showing.busy() {
        typed.clear();
        return;
    }
    // `ControlConfigScreen::HandleKeyDown`: the page is waiting to be told what to
    // bind something to, and the next key or button is it, or is refused.
    if let (Page::Controls, Some(event)) = (menu.page, devices.awaiting) {
        typed.clear();
        let entry = devices.entry();
        let key = keys.get_just_pressed().next().map(|key| Bound::Key(*key));
        let button = devices.pressed.first().map(|button| Bound::Button(*button));
        match key.or(button) {
            Some(to) if Bindings::allowed(entry, to) => {
                settings.controls.set(entry, event, to);
                devices.awaiting = None;
                sfx.play(id::MENU_CONFIRM);
            }
            Some(_) => sfx.play(id::MENU_REFUSE),
            None => return,
        }
        menu.drawn = false;
        return;
    }
    devices.awaiting = None;
    // The lobby has said what a code is for: its session is joined, by way of its
    // password if it has one, or there is none.
    if let (Page::Code, Some(true), Some(found)) = (menu.page, online.seeking, lobby.found.take()) {
        match found {
            Some(listed) if listed.locked => {
                (online.picked, online.key, online.seeking) = (Some(listed), String::new(), None);
                (menu.page, menu.focus, menu.drawn) = (Page::Password, 0, false);
            }
            Some(listed) => {
                online.seeking = None;
                net::join_session(
                    &mut commands,
                    &mut session,
                    &settings,
                    &circuits,
                    &listed,
                    "",
                    garage.ride(&settings),
                );
            }
            None => (online.seeking, menu.drawn) = (Some(false), false),
        }
    }
    // In a session the menu is its room, or the wait to be let into it.
    let home = match *role {
        Role::Offline => None,
        Role::Client if session.you == 0 => Some(Page::Connecting),
        _ => Some(Page::Room),
    };
    // Off the room are the last race's results, the vote for the next race's
    // circuit, the garage, and for the host the race's rules and the session's
    // affairs.
    let aside = matches!(menu.page, Page::Results | Page::Wishes)
        || workshop::mine(menu.page)
        || (matches!(menu.page, Page::Session | Page::Rules) && *role == Role::Host);
    let fresh = room.fresh && home == Some(Page::Room);
    if fresh {
        room.fresh = false;
        // The port's own: whoever won is seen celebrating before the results are.
        if let Some(cosmetics) = winner(&room, art.jam()) {
            showing.request = Some(Request::winner(cosmetics));
        }
    }
    match home {
        // A race just run is shown before anything else.
        Some(_) if fresh => (menu.page, menu.focus, menu.drawn) = (Page::Results, 0, false),
        Some(Page::Room) if aside => {}
        // The room opens on being ready.
        Some(page) if menu.page != page => {
            (menu.page, menu.focus, menu.drawn) =
                (page, if page == Page::Room { READY_AT } else { 0 }, false)
        }
        None if matches!(
            menu.page,
            Page::Room | Page::Connecting | Page::Results | Page::Session | Page::Wishes
        ) =>
        {
            (menu.page, menu.focus, menu.drawn) = (Page::Online, 2, false)
        }
        _ => {}
    }
    if online.seen != (room.revision, lobby.revision) {
        (online.seen, menu.drawn) = ((room.revision, lobby.revision), false);
    }
    // The list of sessions is kept fresh while it is being looked at.
    if menu.page == Page::Join {
        online.refresh -= time.delta_secs();
        if online.refresh <= 0.0 {
            online.refresh = 3.0;
            lobby.list();
        }
    }
    let items = items(
        menu.page,
        &art,
        &circuits,
        &settings,
        &championship,
        &Wired {
            role: *role,
            session: &session,
            room: &room,
            lobby: &lobby,
            online: &online,
            devices: &devices,
            progress: &progress,
        },
        &garage,
        &bench,
    );
    let focus = menu.focus.min(items.len() - 1);
    // Typing takes the letters and the space bar, which otherwise work the menu.
    let typing = match items[focus].action {
        Action::Type(what) => Some(what),
        _ => None,
    };
    // Where bricks are placed the keys are the bricks', but for the way out.
    let building = menu.page == Page::Bricks;
    if building
        && workshop::keys(
            &keys,
            &art,
            &mut bench,
            &mut garage,
            &mut settings,
            &progress,
            &menu,
            &mut sfx,
        )
    {
        menu.drawn = false;
    }
    let pressed = |codes: &[KeyCode]| {
        keys.any_just_pressed(codes.iter().copied().filter(|code| {
            if building {
                return *code == KeyCode::Escape;
            }
            typing.is_none()
                || matches!(
                    code,
                    KeyCode::ArrowDown
                        | KeyCode::ArrowUp
                        | KeyCode::ArrowLeft
                        | KeyCode::ArrowRight
                        | KeyCode::Enter
                        | KeyCode::Escape
                )
        }))
    };
    match typing {
        Some(what) => {
            let (words, most): (&mut String, usize) = match what {
                Typed::Name => (&mut settings.name, NAME_LENGTH),
                Typed::Title => (&mut online.title, TITLE_LENGTH),
                Typed::Password => (&mut online.password, PASSWORD_LENGTH),
                Typed::Key => (&mut online.key, PASSWORD_LENGTH),
                Typed::Lock => (&mut session.password, PASSWORD_LENGTH),
                Typed::Say => (&mut online.say, net::room::CHAT_LENGTH),
                Typed::Code => (&mut online.code, lobby_api::CODE_LENGTH),
                Typed::Racer => (workshop::name(&mut bench), crate::assets::lrs::NAME_LENGTH),
            };
            for key in typed.read().filter(|key| key.state.is_pressed()) {
                match &key.logical_key {
                    Key::Backspace => drop(words.pop()),
                    Key::Space if words.chars().count() < most => words.push(' '),
                    Key::Character(letters) => {
                        // The game's lettering is capitals, and so is what is typed.
                        let fit = letters
                            .chars()
                            .map(|c| c.to_ascii_uppercase())
                            .filter(|c| c.is_ascii_alphanumeric() || "-_'!?.".contains(*c));
                        words.extend(fit.take(most.saturating_sub(words.chars().count())));
                    }
                    _ => continue,
                }
                menu.drawn = false;
            }
        }
        None => typed.clear(),
    }
    let step = |by: usize| {
        // On to the next thing that can be chosen.
        let mut to = focus;
        for _ in 0..items.len() {
            to = (to + by) % items.len();
            if items[to].enabled {
                break;
            }
        }
        to
    };
    let mut focus = focus;
    if pressed(&[KeyCode::ArrowDown, KeyCode::KeyS]) {
        focus = step(1);
    }
    if pressed(&[KeyCode::ArrowUp, KeyCode::KeyW]) {
        focus = step(items.len() - 1);
    }
    let mut change = pressed(&[KeyCode::ArrowRight, KeyCode::KeyD]) as i32
        - pressed(&[KeyCode::ArrowLeft, KeyCode::KeyA]) as i32;
    let mut chosen = pressed(&[KeyCode::Enter, KeyCode::Space]);

    // The pointer chooses whatever it is moved onto, and a click works it.
    let at = pointer(&window);
    let moved = at != *pointed;
    *pointed = at;
    if let Some(at) = at {
        let over = items
            .iter()
            .position(|item| item.enabled && hit(item, &art, at).is_some());
        if let (Some(over), true) = (over, moved || mouse.just_pressed(MouseButton::Left)) {
            focus = over;
        }
        if let (Some(over), true) = (over, mouse.just_pressed(MouseButton::Left)) {
            match hit(&items[over], &art, at) {
                Some(0) if matches!(items[over].widget, Widget::Button { .. }) => chosen = true,
                Some(side) => change = if side == 0 { 1 } else { side },
                None => {}
            }
        } else if over.is_none() && mouse.just_pressed(MouseButton::Left) {
            // A click on something that can't be chosen.
            if items
                .iter()
                .any(|item| !item.enabled && hit(item, &art, at).is_some())
            {
                sfx.play(id::MENU_REFUSE);
            }
        }
    }
    if focus != menu.focus {
        menu.focus = focus;
        menu.drawn = false;
        online.removing = None;
        sfx.play(id::MENU_HIGHLIGHT);
    }

    let go = |menu: &mut Menu, page: Page, focus: usize| {
        (menu.page, menu.focus, menu.drawn) = (page, focus, false);
    };
    if pressed(&[KeyCode::Escape]) {
        let back = match menu.page {
            Page::Main => None,
            Page::SingleRace
            | Page::CircuitRace
            | Page::TimeRace
            | Page::Options
            | Page::Online => Some(Page::Main),
            Page::GameOptions
            | Page::VideoOptions
            | Page::AudioOptions
            | Page::Extras
            | Page::Controls => Some(Page::Options),
            Page::Award => {
                progress.award = None;
                Some(Page::Main)
            }
            Page::Host | Page::Join => Some(Page::Online),
            Page::Password | Page::Code => Some(Page::Join),
            Page::Results | Page::Session | Page::Wishes | Page::Rules => Some(Page::Room),
            // Backing out of a session is leaving it.
            Page::Room | Page::Connecting => {
                net::leave(&mut commands, &mut session, &mut settings, None, &mut next);
                Some(Page::Online)
            }
            // From a session's room the garage is come to, and gone back from.
            page => workshop::escape(page, &art, &mut bench, &garage).map(|to| {
                if to == Page::Main && *role != Role::Offline {
                    Page::Room
                } else {
                    to
                }
            }),
        };
        if let Some(back) = back {
            sfx.play(id::MENU_BACK);
            go(
                &mut menu,
                back,
                if back == Page::Room {
                    READY_AT
                } else {
                    back.first()
                },
            );
        }
        return;
    }

    let action = items[focus].action;
    // Steps a choice round its `count` options.
    let turn =
        |value: usize, count: usize| (value as i32 + change).rem_euclid(count as i32) as usize;
    // What to race as is the player's own, in the room as out of it: whoever has the
    // slot, the game's drivers, then the garage's racers.
    if let (Action::Car, true) = (action, change != 0) {
        let rides = 1 + crate::roster::NAMES.len() + garage.racers.len();
        settings.car = turn(settings.car.min(rides - 1), rides);
        sfx.play(id::MENU_SELECT);
        menu.drawn = false;
        return;
    }
    if change != 0 {
        // In the room it is the player's wish for the next race that is changed, and
        // not their own settings.
        let mut wish = (menu.page == Page::Rules).then(|| {
            Wired {
                role: *role,
                session: &session,
                room: &room,
                lobby: &lobby,
                online: &online,
                devices: &devices,
                progress: &progress,
            }
            .wish(&settings, &circuits)
        });
        let voting = wish.is_some();
        let settings: &mut Settings = match &mut wish {
            Some(wish) => wish,
            None => &mut settings,
        };
        let races: Vec<usize> = (0..circuits.0.len()).collect();
        let here = group(&circuits, settings.circuit);
        match action {
            // The room has one selector for the circuit, which goes round them all.
            Action::RaceChoice if voting => {
                settings.circuit = turn(settings.circuit, circuits.0.len())
            }
            Action::Circuit => {
                // On to the first race of the next circuit that has any.
                let groups = CIRCUIT_ICONS.len();
                let mut to = here;
                for _ in 0..groups {
                    to = (to as i32 + change).rem_euclid(groups as i32) as usize;
                    if let Some(&race) = races.iter().find(|&&r| group(&circuits, r) == to) {
                        settings.circuit = race;
                        break;
                    }
                }
            }
            Action::Series => {
                championship.chosen = turn(championship.chosen, championship.series.len().max(1))
            }
            Action::RaceChoice => {
                let within: Vec<usize> = races
                    .into_iter()
                    .filter(|&r| group(&circuits, r) == here)
                    .collect();
                let at = within
                    .iter()
                    .position(|&r| r == settings.circuit)
                    .unwrap_or(0);
                settings.circuit = within[turn(at, within.len())];
            }
            Action::Opponents => settings.opponents = turn(settings.opponents, MAX_OPPONENTS + 1),
            Action::Laps => settings.lap_choice = turn(settings.lap_choice, LAP_CHOICES.len()),
            Action::Difficulty => {
                settings.difficulty = turn(settings.difficulty, DIFFICULTIES.len())
            }
            Action::Music => {
                settings.music =
                    (settings.music as i32 + change).clamp(0, MAX_VOLUME as i32) as usize
            }
            Action::Sound => {
                settings.sound =
                    (settings.sound as i32 + change).clamp(0, MAX_VOLUME as i32) as usize
            }
            Action::Extra(extra) => settings.turn(extra, change),
            Action::Device => devices.turn(change),
            // Fewer than are here already puts nobody out, and lets nobody else in.
            Action::Limit => {
                session.limit = Some(
                    (session.most() as i32 + change).clamp(2, lobby_api::MAX_PLAYERS as i32) as u8,
                )
            }
            Action::Listed if menu.page == Page::Host => online.unlisted = !online.unlisted,
            Action::Listed => session.unlisted = !session.unlisted,
            Action::Length => {
                let at = SERIES
                    .iter()
                    .position(|races| *races == session.series)
                    .unwrap_or(0);
                session.series = SERIES[turn(at, SERIES.len())];
            }
            _ => {}
        }
        if let Some(wish) = wish {
            (room.ballot, room.revision) = (Some(Rules::of(&wish, &circuits)), room.revision + 1);
        }
        if matches!(
            items[focus].widget,
            Widget::Selector { .. } | Widget::Slider { .. }
        ) {
            sfx.play(if matches!(action, Action::Music | Action::Sound) {
                id::MENU_SLIDER
            } else {
                id::MENU_SELECT
            });
            menu.drawn = false;
        }
    }
    if let (Action::Bench(act), true) = (action, change != 0 || chosen) {
        let to = workshop::act(
            act,
            change,
            &art,
            &mut bench,
            &mut garage,
            &mut settings,
            &progress,
            &menu,
            &mut sfx,
        );
        menu.drawn = false;
        if let Some(page) = to {
            go(&mut menu, page, page.first());
        }
        return;
    }
    if chosen {
        match action {
            // `ControlConfigScreen::OnIconFocused`: what it was bound to is gone,
            // and the page waits for what it is to be bound to instead.
            Action::Collect => {
                progress.award = None;
                sfx.play(id::MENU_CONFIRM);
                go(&mut menu, Page::Main, Page::Main.first());
            }
            Action::Bind(event) => {
                let entry = devices.entry();
                settings.controls.0[entry][event] = Bound::None;
                devices.awaiting = Some(event);
                sfx.play(id::MENU_CONFIRM);
                menu.drawn = false;
            }
            Action::Go(page) => {
                let forward = !matches!(page, Page::Main | Page::Room)
                    && !(page == Page::Options && menu.page != Page::Main)
                    && !(page == Page::Online && menu.page != Page::Main);
                sfx.play(if forward {
                    id::MENU_CONFIRM
                } else {
                    id::MENU_BACK
                });
                match page {
                    // A session is called after its host until it is called something else.
                    Page::Host if online.title.is_empty() => {
                        online.title = format!("{}'S RACE", settings.name)
                            .chars()
                            .take(TITLE_LENGTH)
                            .collect()
                    }
                    Page::Join => online.refresh = 0.0,
                    Page::Code => online.seeking = None,
                    _ => {}
                }
                go(
                    &mut menu,
                    page,
                    match page {
                        Page::Online => 2,
                        Page::Room => READY_AT,
                        _ => page.first(),
                    },
                );
            }
            Action::Remove(peer) if online.removing == Some(peer) => {
                sfx.play(id::MENU_CONFIRM);
                session.removing.push(peer);
                (online.removing, menu.drawn) = (None, false);
            }
            Action::Remove(peer) => {
                sfx.play(id::MENU_SELECT);
                (online.removing, menu.drawn) = (Some(peer), false);
            }
            // Typed, a field is done with: on to the next thing.
            // Said, something stays where it was typed, for the next thing to say.
            Action::Type(Typed::Say) => {
                if let Some(wire) = &mut wire {
                    net::say(*role, &session, &mut room, wire, &online.say);
                }
                online.say.clear();
                menu.drawn = false;
                sfx.play(id::MENU_SELECT);
            }
            Action::Type(_) => {
                (menu.focus, menu.drawn) = (step(1), false);
                sfx.play(id::MENU_HIGHLIGHT);
            }
            Action::Find => {
                sfx.play(id::MENU_CONFIRM);
                lobby.find(&online.code);
                (online.seeking, menu.drawn) = (Some(true), false);
            }
            Action::Enter => {
                sfx.play(id::MENU_CONFIRM);
                if let Some(wire) = &mut wire {
                    net::enter(wire);
                }
            }
            Action::ResetPoints => {
                sfx.play(id::MENU_SELECT);
                room.reset();
            }
            Action::BeginHosting | Action::Pick(_) | Action::Dial => {
                if settings.name.trim().is_empty() {
                    settings.name = "PLAYER".into();
                }
                let picked = match action {
                    Action::Pick(n) => lobby.sessions.get(n).cloned(),
                    _ => online.picked.clone(),
                };
                match (action, picked) {
                    (Action::BeginHosting, _) => {
                        sfx.play(id::MENU_CONFIRM);
                        net::host_session(
                            &mut commands,
                            &mut session,
                            &settings,
                            &circuits,
                            online.title.trim(),
                            &online.password,
                            garage.ride(&settings),
                        );
                        session.unlisted = online.unlisted;
                    }
                    // A locked session wants its password first.
                    (Action::Pick(_), Some(listed)) if listed.locked => {
                        sfx.play(id::MENU_CONFIRM);
                        (online.picked, online.key) = (Some(listed), String::new());
                        go(&mut menu, Page::Password, 0);
                    }
                    (_, Some(listed)) => {
                        sfx.play(id::MENU_CONFIRM);
                        net::join_session(
                            &mut commands,
                            &mut session,
                            &settings,
                            &circuits,
                            &listed,
                            &online.key,
                            garage.ride(&settings),
                        );
                        online.key.clear();
                    }
                    (_, None) => sfx.play(id::MENU_REFUSE),
                }
            }
            Action::Refresh => {
                sfx.play(id::MENU_SELECT);
                online.refresh = 3.0;
                lobby.list();
            }
            Action::Map(circuit) => {
                // The vote goes in with the rest of what the player's game says of
                // the next race, which only the host's is heeded in.
                let mut wish = Wired {
                    role: *role,
                    session: &session,
                    room: &room,
                    lobby: &lobby,
                    online: &online,
                    devices: &devices,
                    progress: &progress,
                }
                .wish(&settings, &circuits);
                wish.circuit = circuit;
                (room.ballot, room.revision) =
                    (Some(Rules::of(&wish, &circuits)), room.revision + 1);
                sfx.play(id::MENU_SELECT);
                menu.drawn = false;
            }
            Action::Ready => {
                sfx.play(id::MENU_SELECT);
                (room.ready, room.revision) = (!room.ready, room.revision + 1);
            }
            Action::Begin => {
                sfx.play(id::MENU_CONFIRM);
                room.begin = true;
            }
            Action::Leave => {
                sfx.play(id::MENU_BACK);
                net::leave(&mut commands, &mut session, &mut settings, None, &mut next);
            }
            Action::Race | Action::TimeRace => {
                sfx.play(id::MENU_CONFIRM);
                (settings.time_race, settings.championship) = (action == Action::TimeRace, None);
                next.set(Screen::Race);
            }
            Action::StartSeries => match championship.begin() {
                // A circuit is three laps a race against a full field.
                Some((code, folder)) => {
                    sfx.play(id::MENU_CONFIRM);
                    settings.championship = Some(code);
                    settings.circuit = circuits
                        .0
                        .iter()
                        .position(|c| c.race.as_deref() == Some(folder.as_str()))
                        .unwrap_or(0);
                    settings.time_race = false;
                    next.set(Screen::Race);
                }
                None => sfx.play(id::MENU_REFUSE),
            },
            Action::Quit => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
    }
}

fn draw(
    mut commands: Commands,
    mut art: ResMut<Art>,
    mut menu: ResMut<Menu>,
    mut images: ResMut<Assets<Image>>,
    mut scale: ResMut<UiScale>,
    circuits: Res<Circuits>,
    settings: Res<Settings>,
    championship: Res<Championship>,
    window: Single<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<Root>>,
    (
        role,
        session,
        room,
        lobby,
        online,
        garage,
        bench,
        portraits,
        devices,
        progress,
        showing,
        mascot,
    ): (
        Res<Role>,
        Res<Session>,
        Res<Room>,
        Res<Lobby>,
        Res<Online>,
        Res<Garage>,
        Res<Bench>,
        Res<Portraits>,
        Res<Devices>,
        Res<Progress>,
        Res<Showing>,
        Res<Mascot>,
    ),
) {
    // A film has the screen to itself, and the menu is drawn afresh after it.
    if showing.busy() {
        for root in &roots {
            commands.entity(root).despawn();
        }
        menu.drawn = false;
        return;
    }
    // The whole screen is scaled to fit the window.
    let fit = (Vec2::new(window.width(), window.height()) / SCREEN).min_element();
    if scale.0 != fit {
        scale.0 = fit;
    }
    if menu.drawn && !roots.is_empty() {
        return;
    }
    menu.drawn = true;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let art = &mut *art;
    let wired = Wired {
        role: *role,
        session: &session,
        room: &room,
        lobby: &lobby,
        online: &online,
        devices: &devices,
        progress: &progress,
    };
    let items = items(
        menu.page,
        art,
        &circuits,
        &settings,
        &championship,
        &wired,
        &garage,
        &bench,
    );
    let labels = labels(menu.page, art);
    let mut notes = notes(menu.page, art, &wired, &settings, &circuits);
    notes.extend(workshop::notes(menu.page, art, &bench, &garage));
    let focus = menu.focus.min(items.len() - 1);

    // Everything is a picture put somewhere on the 640 by 480 screen.
    // The last of each says whether the picture repeats to fill its place.
    let mut pieces: Vec<(Handle<Image>, Rect, Color, bool)> = Vec::new();
    let mut fills: Vec<(Rect, Color)> = Vec::new();
    macro_rules! picture {
        ($name:expr, $at:expr, $colour:expr) => {
            if let Some((handle, size)) = art.picture($name, &mut images) {
                pieces.push((handle, Rect::from_corners($at, $at + size), $colour, false));
            }
        };
    }
    // Words centred in a rectangle (or on a point, for an empty one).
    macro_rules! words {
        ($font:expr, $words:expr, $area:expr, $colour:expr, $centred:expr) => {
            if let Some((handle, size)) = art.write($font, $words, true, &mut images) {
                let area: Rect = $area;
                let corner = if $centred {
                    area.center() - size / 2.0
                } else {
                    Vec2::new(area.min.x, area.center().y - size.y / 2.0)
                };
                pieces.push((
                    handle,
                    Rect::from_corners(corner.round(), corner.round() + size),
                    $colour,
                    false,
                ));
            }
        };
    }

    // The garage's pages are laid over the racer they show.
    if !workshop::mine(menu.page) {
        picture!("backdrp", Vec2::ZERO, Color::WHITE);
    }
    if menu.page == Page::Main {
        picture!("racers", art.place("main", "racers").min, Color::WHITE);
        // `MainMenuScreen::CreateWidgets`: the scene the figure stands in, over the
        // background and under the buttons.
        if let Some(picture) = &mascot.picture {
            let scene = art.place("main", "platform");
            pieces.push((picture.clone(), scene, Color::WHITE, false));
        }
    }
    if menu.page == Page::Licence {
        // `DriverLicenseScreen::CreateWidgets`: the trophy the racer has for each
        // circuit, where the licence has a place for it.
        let card = art.place("drvrlice", "license").min;
        for circuit in 0..8 {
            let trophy = workshop::trophy(&bench, circuit);
            if let Some(name) = trophy.checked_sub(1).and_then(|at| TROPHIES.get(at)) {
                let at = art.place("drvrlice", &format!("trophy{}", circuit + 1)).min;
                picture!(name, card + at, Color::WHITE);
            }
        }
    }
    if let (Page::Award, Some(award)) = (menu.page, progress.award) {
        // The trophy for the place, and beside it the part set won.
        let trophy = award
            .place
            .and_then(|place| TROPHIES.get(place - 1).copied());
        let set = award
            .parts
            .and_then(|set| workshop::SET_PICTURES.get(crate::progress::FREE_SETS + set));
        let shown: Vec<&str> = trophy.into_iter().chain(set.copied()).collect();
        let sizes: Vec<Vec2> = shown
            .iter()
            .map(|name| art.picture(name, &mut images).map_or(Vec2::ZERO, |p| p.1))
            .collect();
        let width: f32 = sizes.iter().map(|size| size.x + 24.0).sum::<f32>() - 24.0;
        let mut left = 320.0 - width / 2.0;
        for (name, size) in shown.iter().zip(&sizes) {
            picture!(
                name,
                Vec2::new(left, 160.0 - size.y / 2.0).round(),
                Color::WHITE
            );
            left += size.x + 24.0;
        }
    }
    if matches!(
        menu.page,
        Page::SingleRace | Page::CircuitRace | Page::TimeRace
    ) {
        // The frame the original shows the circuit in; here it holds the race's settings.
        let frame = art.place("race", "brickbox");
        fills.push((frame, BOX_FILL));
        let (corner, edge) = (16.0, frame.size() - Vec2::splat(32.0));
        let inner = frame.min + Vec2::splat(corner);
        for (name, at, size) in [
            ("tul", frame.min, Vec2::splat(corner)),
            (
                "tt",
                Vec2::new(inner.x, frame.min.y),
                Vec2::new(edge.x, corner),
            ),
            (
                "tur",
                Vec2::new(frame.max.x - corner, frame.min.y),
                Vec2::splat(corner),
            ),
            (
                "tr",
                Vec2::new(frame.max.x - corner, inner.y),
                Vec2::new(corner, edge.y),
            ),
            ("tbr", frame.max - Vec2::splat(corner), Vec2::splat(corner)),
            (
                "tb",
                Vec2::new(inner.x, frame.max.y - corner),
                Vec2::new(edge.x, corner),
            ),
            (
                "tbl",
                Vec2::new(frame.min.x, frame.max.y - corner),
                Vec2::splat(corner),
            ),
            (
                "tl",
                Vec2::new(frame.min.x, inner.y),
                Vec2::new(corner, edge.y),
            ),
        ] {
            if let Some((handle, _)) = art.picture(name, &mut images) {
                pieces.push((
                    handle,
                    Rect::from_corners(at, at + size),
                    Color::WHITE,
                    true,
                ));
            }
        }
        let summary = if menu.page == Page::CircuitRace {
            // The races the circuit is made of.
            let name = |folder: &String| {
                circuits
                    .0
                    .iter()
                    .find(|c| c.race.as_ref() == Some(folder))
                    .map_or(folder.clone(), |c| c.name.clone())
            };
            let rounds = championship
                .series
                .get(championship.chosen)
                .map(|s| s.rounds.iter().map(name).collect::<Vec<_>>());
            rounds.unwrap_or_default().join("\n\n")
        } else if menu.page == Page::TimeRace {
            format!(
                "{}\n\nLAPS {}",
                circuits.0[settings.circuit].name,
                crate::time_race::LAPS
            )
        } else {
            // Whichever of the port's ways of racing are on, under the original's settings.
            let mut extras: Vec<String> = Vec::new();
            let reversed = crate::variant::Variant::of(
                &settings,
                &championship,
                circuits.0[settings.circuit].race.as_deref(),
            )
            .reverse;
            for (on, extra) in [
                (settings.mirror, Extra::Mirror),
                (reversed, Extra::Reverse),
                (settings.eliminating(), Extra::Elimination),
            ] {
                if on {
                    extras.push(extra.label().to_string());
                }
            }
            if settings.bricks > 0 {
                extras.push(format!("BRICKS {}", settings.shown(Extra::Bricks)));
            }
            let mut lines = vec![
                circuits.0[settings.circuit].name.clone(),
                format!("LAPS {}", settings.laps()),
                format!("OPPONENTS {}", settings.opponents),
                DIFFICULTIES[settings.difficulty].0.to_string(),
            ];
            // The frame holds the original's four lines spaced out; any more are closed up.
            let gap = if extras.is_empty() { "\n\n" } else { "\n" };
            lines.extend(extras);
            lines.join(gap)
        };
        words!("font_ths", &summary, frame, LABEL, true);
    }
    for (area, text, font) in &labels {
        words!(font, text, *area, LABEL, true);
    }
    for (area, text, font, colour, centred) in &notes {
        words!(font, text, *area, *colour, *centred);
    }
    // In the room, everyone's minifigure over their name, the player's own picked
    // out by a panel behind it.
    if menu.page == Page::Room {
        for (n, voter) in room.voters.iter().enumerate() {
            if voter.peer == session.you {
                let (figure, _, under) = voter_places(n, room.voters.len());
                let panel = Rect::new(
                    under.min.x + 2.0,
                    figure.min.y,
                    under.max.x - 2.0,
                    under.max.y,
                );
                fills.push((panel, BOX_FILL));
            }
            if let Some(picture) = portraits.of(voter.peer) {
                let (area, ..) = voter_places(n, room.voters.len());
                pieces.push((picture, area, Color::WHITE, false));
            }
        }
    }
    for (index, item) in items.iter().enumerate() {
        let colour = match (item.enabled, index == focus) {
            (false, _) => DISABLED,
            (true, true) => SELECTED,
            (true, false) => NORMAL,
        };
        // Arrows are plain until their widget is the chosen one.
        let arrows = if index == focus {
            ["arrowls", "arrowrs"]
        } else {
            ["arrowlu", "arrowru"]
        };
        match &item.widget {
            Widget::Button { at, label, icon } => {
                if let Some(icon) = icon {
                    picture!(icon, *at, colour);
                }
                let line = Rect::from_corners(*at + Vec2::X * ICON, *at + Vec2::new(ICON, ICON));
                words!("font_ths", label, line, colour, false);
            }
            Widget::Selector {
                area,
                picture,
                words,
            } => {
                let middle = area.center().y - ICON / 2.0;
                picture!(arrows[0], Vec2::new(area.min.x, middle), Color::WHITE);
                picture!(
                    arrows[1],
                    Vec2::new(area.max.x - ICON, middle),
                    Color::WHITE
                );
                if let Some((handle, size)) = picture
                    .as_deref()
                    .and_then(|name| art.picture(name, &mut images))
                {
                    let corner = (area.center() - size / 2.0).round();
                    pieces.push((
                        handle,
                        Rect::from_corners(corner, corner + size),
                        Color::WHITE,
                        false,
                    ));
                }
                if !words.is_empty() {
                    words!("font_ths", words, *area, colour, true);
                }
            }
            Widget::Field { area, words } => {
                fills.push((*area, BOX_FILL));
                // Where the next letter goes is marked while it is being typed into.
                words!(
                    "font_ths",
                    words,
                    Rect::from_corners(area.min + Vec2::X * 6.0, area.max),
                    colour,
                    false
                );
                if index == focus {
                    let width = if words.is_empty() {
                        0.0
                    } else {
                        art.write("font_ths", words, true, &mut images)
                            .map_or(0.0, |written| written.1.x)
                    };
                    let at = Vec2::new(area.min.x + 8.0 + width, area.min.y + 5.0);
                    fills.push((Rect::from_corners(at, at + Vec2::new(3.0, 22.0)), SELECTED));
                }
            }
            Widget::Slider { area, value } => {
                // Quieter and louder at the ends, and between them a rail with a thumb.
                let middle = area.center().y;
                picture!(
                    "sounddwn",
                    Vec2::new(area.min.x, middle - 32.0),
                    Color::WHITE
                );
                picture!(
                    "soundup",
                    Vec2::new(area.max.x - 64.0, middle - 32.0),
                    Color::WHITE
                );
                let rail = Rect::new(
                    area.min.x + 64.0,
                    middle - 16.0,
                    area.max.x - 64.0,
                    middle + 16.0,
                );
                if let Some((handle, _)) = art.picture("bar", &mut images) {
                    pieces.push((handle, rail, Color::WHITE, true));
                }
                let along = rail.min.x + (rail.width() - ICON) * *value as f32 / MAX_VOLUME as f32;
                picture!("tab", Vec2::new(along.round(), rail.min.y), colour);
            }
        }
    }

    let side = |length: f32| Val::Px(length);
    let screen = Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    // Black round the screen, but for the garage's pages, which the racer shows through.
    let behind = if workshop::mine(menu.page) {
        Color::NONE
    } else {
        Color::BLACK
    };
    commands
        .spawn((Root, screen, BackgroundColor(behind)))
        .with_children(|root| {
            let canvas = Node {
                width: side(SCREEN.x),
                height: side(SCREEN.y),
                overflow: Overflow::clip(),
                ..default()
            };
            root.spawn(canvas).with_children(|canvas| {
                let node = |area: Rect| Node {
                    position_type: PositionType::Absolute,
                    left: side(area.min.x),
                    top: side(area.min.y),
                    width: side(area.width()),
                    height: side(area.height()),
                    ..default()
                };
                // The backdrop first, then the fills, then everything else in order.
                let mut pieces = pieces.into_iter();
                let picture = |handle: Handle<Image>, colour: Color, tiled: bool| {
                    let image_mode = if tiled {
                        NodeImageMode::Tiled {
                            tile_x: true,
                            tile_y: true,
                            stretch_value: 1.0,
                        }
                    } else {
                        NodeImageMode::Stretch
                    };
                    ImageNode {
                        image: handle,
                        color: colour,
                        image_mode,
                        ..default()
                    }
                };
                if let Some((handle, area, colour, tiled)) = pieces.next() {
                    canvas.spawn((node(area), picture(handle, colour, tiled)));
                }
                for (area, colour) in fills {
                    canvas.spawn((node(area), BackgroundColor(colour)));
                }
                for (handle, area, colour, tiled) in pieces {
                    canvas.spawn((node(area), picture(handle, colour, tiled)));
                }
            });
        });
}

#[cfg(test)]
#[test]
fn the_winner_of_a_race_online_is_who_celebrates() {
    use crate::assets::lrs::Racer;
    use crate::net::room::{Finish, Voter};
    let Some(jam) = crate::world::jam() else {
        return;
    };
    let finish = |name: &str, player: bool, time: Option<f32>| Finish {
        name: name.into(),
        player,
        time,
        best: None,
        points: 0,
    };
    let voter = |peer, name: &str| Voter {
        peer,
        name: name.into(),
        ready: false,
        ballot: None,
        link: None,
        points: 0,
    };
    let built = Cosmetics {
        hat: 4,
        face: 5,
        torso: 25,
        legs: 3,
        expression: 0,
    };
    let racer = Racer {
        cosmetics: built,
        ..default()
    };
    let mut room = Room::default();
    room.voters = vec![voter(1, "ANNA"), voter(2, "BEN")];
    room.rides = vec![(1, Ride::Built(racer)), (2, Ride::Driver("KK".into()))];
    room.results = vec![
        finish("ANNA", true, Some(61.0)),
        finish("BEN", true, Some(62.0)),
    ];
    // A player who built their racer is seen as they built it.
    assert_eq!(winner(&room, &jam), Some(built));
    // One racing as a driver of the game's is that driver, and so is the computer.
    room.results.swap(0, 1);
    let kahuka = crate::roster::cosmetics_of(&jam, "KK");
    assert!(kahuka.is_some() && winner(&room, &jam) == kahuka);
    room.results[0] = finish("King Kahuka", false, Some(60.0));
    assert_eq!(winner(&room, &jam), kahuka);
    // A race nobody finished has nobody to celebrate.
    room.results[0].time = None;
    assert_eq!(winner(&room, &jam), None);
}
