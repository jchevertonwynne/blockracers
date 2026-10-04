//! The original game's front end, drawn from its own menu data: the screen layouts
//! (`.MIB`), pictures, bitmap fonts and string tables in `MENUDATA`. Main menu, single
//! race and options are here; the screens for things the port doesn't have (building,
//! controls) are shown but can't be chosen. The video options are the port's
//! own, and so is the extras page: the ways of racing the original doesn't have.
//!
//! Racing online is the port's own too, and is reached where the original has its
//! two-player race: a page to host or join from, the list of sessions the lobby
//! has, and the room a session waits in, where everyone says how they would have the
//! next race run (`net::room`). Their widgets are placed by hand; the original has
//! no screens to take the places from.
//!
//! Colours and the make-up of each widget follow the styles in `GSTYLES.MSB`. The
//! spinning models the original shows on these screens are not drawn.

use crate::championship::Championship;
use crate::assets::{
    Jam,
    font::{Font, load_fonts, load_strings},
    image::decode_bmp,
    tokens::{Token, tokenize},
};
use crate::audio::{Sfx, id};
use crate::menu::{Circuits, DIFFICULTIES, Extra, LAP_CHOICES, MAX_OPPONENTS, MAX_VOLUME, NAME_LENGTH, Screen, Settings};
use crate::net::{self, Role, Session, lobby::Lobby, protocol::Rules, room::Room};
use bevy::{
    asset::RenderAssetUsages,
    input::keyboard::{Key, KeyboardInput},
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::PrimaryWindow,
};
use std::collections::HashMap;

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
    pub const OK: usize = 114;
    pub const LANGUAGE: usize = 156;
}

/// The pictures on the circuit selector, by which of the game's circuits a race is
/// in; the last is for the port's own brick circuit.
/// The pictures for the seven circuits of the circuit race: the three sets of four
/// races, the same three mirrored, and Rocket Racer's.
const SERIES_ICONS: [&str; 7] = ["pirate", "islander", "magical", "pirate", "islander", "magical", "rr"];
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
    Extras,
    /// The port's own, for racing online: where to host or join from, what to host
    /// as, the sessions there are to join, the password one of them wants, the wait
    /// for its host to answer, and the room a session waits in between races.
    Online,
    Host,
    Join,
    Password,
    Connecting,
    Room,
}

/// What can be typed into.
#[derive(Clone, Copy, PartialEq)]
enum Typed {
    Name,
    Title,
    Password,
    /// The password of the session being joined.
    Key,
}

/// The things a room votes on, but for the circuit: the port's own ways of racing
/// that are everyone's affair. How a player steers is their own.
const VOTED: [Extra; 4] = [Extra::Mirror, Extra::Reverse, Extra::Bricks, Extra::Elimination];
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
}

/// What the online pages show: who this game is to the session, the session, its
/// room, the lobby's list, and what has been typed.
struct Wired<'a> {
    role: Role,
    session: &'a Session,
    room: &'a Room,
    lobby: &'a Lobby,
    online: &'a Online,
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

/// Where the rows of a page of the port's settings go: the rectangles the original
/// gives the two rows of its game options, carried on down the screen.
fn rows(art: &Art, count: usize, names: [&str; 2]) -> Vec<Rect> {
    let (first, second) = (art.place("options", names[0]), art.place("options", names[1]));
    // Three rows fit at the original's spacing; more are closed up.
    let step = if count > 3 { 2.2 / (count - 1) as f32 } else { 1.0 };
    let row = |n: usize| {
        let n = n as f32 * step;
        Rect::from_corners(first.min + (second.min - first.min) * n, first.max + (second.max - first.max) * n)
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
}

enum Widget {
    /// A picture and some words: the picture is `icon` or nothing.
    Button { at: Vec2, label: String, icon: Option<&'static str> },
    /// Arrows either side of a picture or some words.
    Selector { area: Rect, picture: Option<String>, words: String },
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
        .add_systems(OnEnter(Screen::Menu), enter)
        .add_systems(OnExit(Screen::Menu), leave)
        .add_systems(Update, (input, draw).chain().run_if(in_state(Screen::Menu)).run_if(resource_exists::<Art>));
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
        let (Token::Key(_), Some(Token::Str(name)), Some(Token::LCurly), Some(Token::Key(0x36))) =
            (token, tokens.get(i + 1), tokens.get(i + 2), tokens.get(i + 3))
        else {
            continue;
        };
        if tokens.get(i + 5) == Some(&Token::Key(0x2f)) {
            out.insert(name.to_lowercase(), [number(i + 6), number(i + 7), number(i + 8), number(i + 9)]);
        }
    }
    out
}

fn load_art() -> Option<Art> {
    let path = std::env::var("LEGO_JAM").unwrap_or("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM".into());
    let jam = Jam::open(path)?;
    let fonts = load_fonts(&jam, &format!("{DIR}/ENGLISH"), "GFONTS.FDB");
    let strings = jam.get(&format!("{DIR}/ENGLISH/MENUTEXT.SRF")).map(load_strings).unwrap_or_default();
    let mut layouts = HashMap::new();
    for (screen, file) in [("main", "MAINMENU"), ("race", "SINGRACE"), ("options", "OPTIONS")] {
        layouts.insert(screen, jam.get(&format!("{DIR}/{file}.MIB")).map(layout).unwrap_or_default());
    }
    // `0x27 "name" { 0x29 [0x2b r g b] }`: a picture and the colour that is see-through.
    let mut keys = HashMap::new();
    for list in ["GIMAGES", "SINGRACE", "OPTIONS", "BUILDER"] {
        let tokens = tokenize(jam.get(&format!("{DIR}/{list}.IDB")).unwrap_or_default());
        for (i, token) in tokens.iter().enumerate() {
            let (Token::Key(0x27), Some(Token::Str(name)), Some(Token::LCurly)) = (token, tokens.get(i + 1), tokens.get(i + 2))
            else {
                continue;
            };
            let value = |at: usize| match tokens.get(at) {
                Some(Token::Int(v)) => *v as u8,
                _ => 0,
            };
            let key = (tokens.get(i + 4) == Some(&Token::Key(0x2b))).then(|| [value(i + 5), value(i + 6), value(i + 7)]);
            keys.entry(name.to_lowercase()).or_insert(key);
        }
    }
    if fonts.is_empty() || strings.is_empty() || layouts.values().any(HashMap::is_empty) {
        return None;
    }
    Some(Art { jam, fonts, strings, keys, layouts, pictures: HashMap::new(), written: HashMap::new() })
}

fn image(pixels: crate::assets::image::Pixels, images: &mut Assets<Image>) -> (Handle<Image>, Vec2) {
    let size = Vec2::new(pixels.width as f32, pixels.height as f32);
    let extent = Extent3d { width: pixels.width, height: pixels.height, depth_or_array_layers: 1 };
    let mut image =
        Image::new(extent, TextureDimension::D2, pixels.rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    // Crisp, as on the original's low-resolution screen.
    image.sampler = ImageSampler::nearest();
    (images.add(image), size)
}

impl Art {
    fn picture(&mut self, name: &str, images: &mut Assets<Image>) -> Option<(Handle<Image>, Vec2)> {
        if !self.pictures.contains_key(name) {
            let key = self.keys.get(name).copied().flatten();
            let pixels = decode_bmp(self.jam.get(&format!("{DIR}/{name}.BMP"))?, key)?;
            self.pictures.insert(name.to_string(), image(pixels, images));
        }
        self.pictures.get(name).cloned()
    }

    fn write(&mut self, font: &str, words: &str, centred: bool, images: &mut Assets<Image>) -> Option<(Handle<Image>, Vec2)> {
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
        let [left, top, right, bottom] = self.layouts.get(screen).and_then(|l| l.get(name)).copied().unwrap_or_default();
        Rect::new(left, top, right.max(left), bottom.max(top))
    }
}

/// Which of the game's circuits a race belongs to.
fn group(circuits: &Circuits, index: usize) -> usize {
    circuits.0[index].group.min(CIRCUIT_ICONS.len() - 1)
}

/// The widgets of a page that can be chosen or changed, top to bottom.
fn items(page: Page, art: &Art, circuits: &Circuits, settings: &Settings, championship: &Championship, wired: &Wired) -> Vec<Item> {
    let button = |screen: &str, name: &str, label: usize, action: Action, icon: Option<&'static str>| Item {
        widget: Widget::Button { at: art.place(screen, name).min, label: art.string(label), icon },
        action,
        enabled: action != Action::Nothing,
    };
    let selector = |area: Rect, picture: Option<String>, words: String, action: Action| Item {
        widget: Widget::Selector { area, picture, words },
        action,
        enabled: true,
    };
    let back = |screen: &str, to: Page| button(screen, "goback", text::MAIN_MENU, Action::Go(to), Some("txtarol"));
    match page {
        Page::Main => vec![
            button("main", "garage", text::BUILD, Action::Nothing, None),
            button("main", "circuit", text::CIRCUIT_RACE, Action::Go(Page::CircuitRace), None),
            button("main", "single", text::SINGLE_RACE, Action::Go(Page::SingleRace), None),
            // Where the original races two on one screen, the port races online.
            Item { widget: Widget::Button { at: art.place("main", "vs").min, label: "ONLINE RACE".into(), icon: None }, action: Action::Go(Page::Online), enabled: true },
            button("main", "time", text::TIME_RACE, Action::Go(Page::TimeRace), None),
            button("main", "options", text::OPTIONS, Action::Go(Page::Options), None),
            button("main", "quit", text::QUIT, Action::Quit, None),
        ],
        Page::SingleRace | Page::TimeRace => {
            let go = if page == Page::TimeRace { Action::TimeRace } else { Action::Race };
            let icon = CIRCUIT_ICONS[group(circuits, settings.circuit)].to_string();
            let name = circuits.0[settings.circuit].name.clone();
            vec![
                selector(art.place("race", "selector"), Some(icon), String::new(), Action::Circuit),
                selector(art.place("race", "racesel"), None, name, Action::RaceChoice),
                button("race", "gonext", text::OK, go, Some("chck")),
                back("race", Page::Main),
            ]
        }
        Page::CircuitRace => {
            // The single race page's widgets: the circuit's picture, and whose it is.
            let chosen = championship.chosen.min(championship.series.len().saturating_sub(1));
            let icon = SERIES_ICONS[chosen % SERIES_ICONS.len()].to_string();
            let words = match championship.series.get(chosen) {
                Some(series) if chosen < championship.unlocked => format!("{}: {}", chosen + 1, series.champion),
                Some(_) => format!("{}: LOCKED", chosen + 1),
                None => String::new(),
            };
            let mut start = button("race", "gonext", text::OK, Action::StartSeries, Some("chck"));
            start.enabled = chosen < championship.unlocked;
            vec![
                selector(art.place("race", "selector"), Some(icon), String::new(), Action::Series),
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
            button("options", "game", text::GAME_OPTIONS, Action::Go(Page::GameOptions), None),
            button("options", "video", text::VIDEO_OPTIONS, Action::Go(Page::VideoOptions), None),
            button("options", "audio", text::AUDIO_OPTIONS, Action::Go(Page::AudioOptions), None),
            button("options", "player1", text::CONTROLS[0], Action::Nothing, None),
            button("options", "player2", text::CONTROLS[1], Action::Nothing, None),
            button("options", "language", text::LANGUAGE, Action::Nothing, None),
            button("options", "credits", text::CREDITS, Action::Nothing, None),
            back("options", Page::Main),
        ],
        Page::GameOptions => {
            // The original has two selectors on this page; the port's own settings go
            // in more of the same, a row apiece.
            let (first, second) = (art.place("options", "chmpcont"), art.place("options", "lapcont"));
            let row = |n: f32| Rect::from_corners(first.min + (second.min - first.min) * n, first.max + (second.max - first.max) * n);
            vec![
                selector(row(0.0), None, settings.opponents.to_string(), Action::Opponents),
                selector(row(1.0), None, settings.laps().to_string(), Action::Laps),
                selector(row(2.0), None, DIFFICULTIES[settings.difficulty].0.to_string(), Action::Difficulty),
                back("options", Page::Options),
            ]
        }
        Page::VideoOptions | Page::Extras => {
            let extras = page.extras();
            let mut items: Vec<Item> = rows(art, extras.len(), ["chmpcont", "lapcont"])
                .into_iter()
                .zip(extras)
                .map(|(area, &extra)| selector(area, None, settings.shown(extra), Action::Extra(extra)))
                .collect();
            items.push(back("options", Page::Options));
            items
        }
        Page::AudioOptions => vec![
            Item { widget: Widget::Slider { area: art.place("options", "musicvol"), value: settings.music }, action: Action::Music, enabled: true },
            Item { widget: Widget::Slider { area: art.place("options", "soundvol"), value: settings.sound }, action: Action::Sound, enabled: true },
            back("options", Page::Options),
        ],
        Page::Online => vec![
            field(FIELD[0], settings.name.clone(), Typed::Name),
            selector(Rect::from_corners(FIELD[1].min - Vec2::X * ICON, FIELD[1].max + Vec2::X * ICON), None, racing_as(settings).to_string(), Action::Car),
            plain(Vec2::new(3.0, 178.0), "HOST A RACE", Action::Go(Page::Host)),
            plain(Vec2::new(3.0, 218.0), "JOIN A RACE", Action::Go(Page::Join)),
            back("options", Page::Main),
        ],
        Page::Host => vec![
            field(FIELD[0], wired.online.title.clone(), Typed::Title),
            field(FIELD[1], wired.online.password.clone(), Typed::Password),
            Item { widget: Widget::Button { at: art.place("race", "gonext").min, label: art.string(text::OK), icon: Some("chck") }, action: Action::BeginHosting, enabled: !wired.online.title.trim().is_empty() },
            way_back(art, Page::Online),
        ],
        Page::Join => {
            let mut items: Vec<Item> = wired.lobby.sessions.iter().take(LISTED).enumerate().map(|(n, listed)| {
                let mut label = format!("{} - {} {}/{}", listed.name, listed.host, listed.status.players, listed.max);
                for (so, word) in [(listed.locked, " LOCKED"), (listed.status.racing, " RACING")] {
                    if so {
                        label += word;
                    }
                }
                Item { widget: Widget::Button { at: Vec2::new(3.0, 80.0 + 36.0 * n as f32), label, icon: None }, action: Action::Pick(n), enabled: listed.status.players < listed.max }
            }).collect();
            items.push(plain(art.place("race", "gonext").min, "REFRESH", Action::Refresh));
            items.push(way_back(art, Page::Online));
            items
        }
        Page::Password => vec![
            field(FIELD[0], wired.online.key.clone(), Typed::Key),
            Item { widget: Widget::Button { at: art.place("race", "gonext").min, label: art.string(text::OK), icon: Some("chck") }, action: Action::Dial, enabled: true },
            way_back(art, Page::Join),
        ],
        Page::Connecting => vec![Item { widget: Widget::Button { at: art.place("race", "goback").min, label: "CANCEL".into(), icon: Some("txtarol") }, action: Action::Leave, enabled: true }],
        Page::Room => {
            let wish = wired.wish(settings, circuits);
            let row = |n: usize| Rect::new(164.0, ROOM_TOP + ROOM_STEP * n as f32, 440.0, ROOM_TOP + ROOM_STEP * n as f32 + ICON);
            let mut items = vec![
                selector(Rect::new(8.0, ROOM_TOP, 440.0, ROOM_TOP + ICON), None, circuits.0[wish.circuit].name.clone(), Action::RaceChoice),
                selector(row(1), None, wish.laps().to_string(), Action::Laps),
                selector(row(2), None, wish.opponents.to_string(), Action::Opponents),
            ];
            items.extend(VOTED.iter().enumerate().map(|(n, &extra)| selector(row(3 + n), None, wish.shown(extra), Action::Extra(extra))));
            items.push(plain(Vec2::new(3.0, 338.0), if wired.room.ready { "READY: YES" } else { "READY: NO" }, Action::Ready));
            if wired.role == Role::Host {
                items.push(plain(Vec2::new(3.0, 378.0), "START NOW", Action::Begin));
            }
            items.push(Item { widget: Widget::Button { at: art.place("race", "goback").min, label: "LEAVE".into(), icon: Some("txtarol") }, action: Action::Leave, enabled: true });
            items
        }
    }
}

/// Where the online pages put what is typed, and beside it what it is.
const FIELD: [Rect; 2] = [Rect { min: Vec2::new(190.0, 96.0), max: Vec2::new(480.0, 128.0) }, Rect { min: Vec2::new(190.0, 136.0), max: Vec2::new(480.0, 168.0) }];
/// Where the room's rows begin and how far apart they are.
const ROOM_TOP: f32 = 76.0;
const ROOM_STEP: f32 = 36.0;

fn field(area: Rect, words: String, typed: Typed) -> Item {
    Item { widget: Widget::Field { area, words }, action: Action::Type(typed), enabled: true }
}

/// The way back from one of the online pages to the one before it.
fn way_back(art: &Art, to: Page) -> Item {
    Item { widget: Widget::Button { at: art.place("race", "goback").min, label: "BACK".into(), icon: Some("txtarol") }, action: Action::Go(to), enabled: true }
}

/// Who the player races as online, as the menu says it.
fn racing_as(settings: &Settings) -> &'static str {
    settings.car.checked_sub(1).and_then(|n| crate::roster::NAMES.get(n)).map_or("ANYONE", |driver| driver.1)
}

/// A button of the port's own, with words the original hasn't a string for.
fn plain(at: Vec2, label: &str, action: Action) -> Item {
    Item { widget: Widget::Button { at, label: label.into(), icon: None }, action, enabled: true }
}

/// What the online pages say beside and around their widgets: where, what, in which
/// font and colour, and whether centred there.
fn notes(page: Page, wired: &Wired, settings: &Settings, circuits: &Circuits) -> Vec<(Rect, String, &'static str, Color, bool)> {
    let banner = |words: &str| (Rect::new(375.0, 20.0, 375.0, 68.0), words.to_string(), "fontmenu", LABEL, true);
    let beside = |area: Rect, words: &str| (Rect::new(8.0, area.min.y, area.min.x - 8.0, area.max.y), words.to_string(), "font_ths", LABEL, false);
    let middle = |words: &str| (Rect::new(320.0, 300.0, 320.0, 332.0), words.to_string(), "font_ths", LABEL, true);
    match page {
        Page::Online => {
            let mut notes = vec![banner("ONLINE RACE"), beside(FIELD[0], "YOUR NAME"), beside(FIELD[1], "RACING AS")];
            // Why the last session ended, if it was not left by choice.
            notes.extend(wired.session.notice.as_deref().map(middle));
            notes
        }
        Page::Host => vec![banner("HOST A RACE"), beside(FIELD[0], "CALLED"), beside(FIELD[1], "PASSWORD"), middle("LEAVE THE PASSWORD EMPTY TO LET ANYONE IN")],
        Page::Join => {
            let mut notes = vec![banner("JOIN A RACE")];
            if let Some(trouble) = wired.lobby.trouble.as_ref().filter(|_| wired.lobby.sessions.is_empty()) {
                bevy::log::debug!("{trouble}");
                notes.push(middle("THE LOBBY CAN'T BE REACHED"));
            } else if wired.lobby.sessions.is_empty() {
                notes.push(middle("NOBODY IS HOSTING A RACE"));
            }
            notes
        }
        Page::Password => vec![banner("JOIN A RACE"), beside(FIELD[0], "PASSWORD")],
        Page::Connecting => vec![banner("JOIN A RACE"), middle("CALLING THE HOST")],
        Page::Room => {
            let room = wired.room;
            let mut notes = vec![banner(&wired.session.title)];
            let label = |n: usize, words: &str| (Rect::new(8.0, ROOM_TOP + ROOM_STEP * n as f32, 150.0, ROOM_TOP + ROOM_STEP * n as f32 + ICON), words.to_string(), "font_ths", LABEL, false);
            notes.push(label(1, "LAPS"));
            notes.push(label(2, "OPPONENTS"));
            notes.extend(VOTED.iter().enumerate().map(|(n, extra)| label(3 + n, extra.label())));
            // Beside each, how many in the room want the same.
            let agreeing: [(usize, usize); 7] = [
                room.agreeing(|rules| rules.circuit.clone()),
                room.agreeing(|rules| rules.lap_choice),
                room.agreeing(|rules| rules.opponents),
                room.agreeing(|rules| rules.mirror),
                room.agreeing(|rules| rules.reverse),
                room.agreeing(|rules| rules.bricks),
                room.agreeing(|rules| rules.elimination),
            ];
            for (n, (agree, of)) in agreeing.into_iter().enumerate() {
                let at = Rect::new(444.0, ROOM_TOP + ROOM_STEP * n as f32, 484.0, ROOM_TOP + ROOM_STEP * n as f32 + ICON);
                notes.push((at, format!("{agree}/{of}"), "font_ths", NORMAL, false));
            }
            // Who is here, lit when ready, and before each where they came last race.
            let _ = (settings, circuits);
            for (n, voter) in room.voters.iter().enumerate() {
                let place = room.results.iter().position(|name| *name == voter.name).map_or(String::new(), |place| format!("{} ", place + 1));
                let at = Rect::new(500.0, ROOM_TOP + 32.0 * n as f32, 636.0, ROOM_TOP + 32.0 * (n + 1) as f32);
                notes.push((at, format!("{place}{}", voter.name), "font_ths", if voter.ready { SELECTED } else { NORMAL }, false));
            }
            if let Some(left) = room.closing {
                notes.push((Rect::new(300.0, 338.0, 636.0, 370.0), format!("RACE STARTS IN {}", left.max(0.0).ceil() as i32), "font_ths", LABEL, true));
            }
            notes
        }
        _ => Vec::new(),
    }
}

/// Words that go beside the widgets of a page: (where, what, which font).
fn labels(page: Page, art: &Art) -> Vec<(Rect, String, &'static str)> {
    let beside = |name: &str, words: String| (art.place("options", name), words, "font_ths");
    let banner = |words: usize| (Rect::new(375.0, 20.0, 375.0, 68.0), art.string(words), "fontmenu");
    match page {
        Page::Main => Vec::new(),
        Page::SingleRace => vec![banner(text::SINGLE_RACE)],
        Page::CircuitRace => vec![banner(text::CIRCUIT_RACE)],
        Page::TimeRace => vec![banner(text::TIME_RACE)],
        Page::Options => vec![banner(text::OPTIONS_BANNER)],
        Page::GameOptions => {
            let (first, second) = (art.place("options", "chmptext"), art.place("options", "laptext"));
            let third = Rect::from_corners(second.min * 2.0 - first.min, second.max * 2.0 - first.max);
            vec![
                banner(text::GAME_OPTIONS),
                (first, art.string(text::OPPONENTS), "font_ths"),
                (second, "NUMBER OF LAPS".into(), "font_ths"),
                (third, "DIFFICULTY".into(), "font_ths"),
            ]
        }
        Page::VideoOptions | Page::Extras => {
            let extras = page.extras();
            let title = if page == Page::Extras { "EXTRAS".to_string() } else { art.string(text::VIDEO_OPTIONS) };
            let mut labels = vec![(Rect::new(375.0, 20.0, 375.0, 68.0), title, "fontmenu")];
            let places = rows(art, extras.len(), ["chmptext", "laptext"]);
            labels.extend(places.into_iter().zip(extras).map(|(area, extra)| (area, extra.label().to_string(), "font_ths")));
            labels
        }
        Page::AudioOptions => vec![
            banner(text::AUDIO_OPTIONS),
            beside("mvoltext", art.string(text::MUSIC_VOLUME)),
            beside("svoltext", art.string(text::SOUND_VOLUME)),
        ],
        // The port's online pages say what they have to in `notes`.
        Page::Online | Page::Host | Page::Join | Page::Password | Page::Connecting | Page::Room => Vec::new(),
    }
}

fn enter(mut menu: ResMut<Menu>, mut session: ResMut<Session>) {
    *menu = Menu::default();
    menu.focus = 2;
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
        Ok("online") => Page::Online,
        Ok("host") => Page::Host,
        Ok("join") => Page::Join,
        _ => return,
    };
    (menu.page, menu.focus) = (page, page.first());
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
        Widget::Button { at: corner, label, .. } => {
            let width = art.fonts.get("font_ths").map_or(0.0, |f| f.render(label, false).width as f32);
            Rect::from_corners(*corner, *corner + Vec2::new(ICON + width, ICON)).contains(at).then_some(0)
        }
        Widget::Field { area, .. } => area.contains(at).then_some(0),
        Widget::Selector { area, .. } | Widget::Slider { area, .. } => {
            let end = if matches!(item.widget, Widget::Slider { .. }) { 64.0 } else { ICON };
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
    (role, mut session, mut room, lobby, mut online): (Res<Role>, ResMut<Session>, ResMut<Room>, Res<Lobby>, ResMut<Online>),
) {
    // In a session the menu is its room, or the wait to be let into it.
    let home = match *role {
        Role::Offline => None,
        Role::Client if session.you == 0 => Some(Page::Connecting),
        _ => Some(Page::Room),
    };
    match home {
        // The room opens on being ready.
        Some(page) if menu.page != page => (menu.page, menu.focus, menu.drawn) = (page, if page == Page::Room { 3 + VOTED.len() } else { 0 }, false),
        None if matches!(menu.page, Page::Room | Page::Connecting) => (menu.page, menu.focus, menu.drawn) = (Page::Online, 2, false),
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
    let items = items(menu.page, &art, &circuits, &settings, &championship, &Wired { role: *role, session: &session, room: &room, lobby: &lobby, online: &online });
    let focus = menu.focus.min(items.len() - 1);
    // Typing takes the letters and the space bar, which otherwise work the menu.
    let typing = match items[focus].action {
        Action::Type(what) => Some(what),
        _ => None,
    };
    let pressed = |codes: &[KeyCode]| keys.any_just_pressed(codes.iter().copied().filter(|code| typing.is_none() || matches!(code, KeyCode::ArrowDown | KeyCode::ArrowUp | KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::Enter | KeyCode::Escape)));
    match typing {
        Some(what) => {
            let (words, most): (&mut String, usize) = match what {
                Typed::Name => (&mut settings.name, NAME_LENGTH),
                Typed::Title => (&mut online.title, TITLE_LENGTH),
                Typed::Password => (&mut online.password, PASSWORD_LENGTH),
                Typed::Key => (&mut online.key, PASSWORD_LENGTH),
            };
            for key in typed.read().filter(|key| key.state.is_pressed()) {
                match &key.logical_key {
                    Key::Backspace => drop(words.pop()),
                    Key::Space if words.chars().count() < most => words.push(' '),
                    Key::Character(letters) => {
                        // The game's lettering is capitals, and so is what is typed.
                        let fit = letters.chars().map(|c| c.to_ascii_uppercase()).filter(|c| c.is_ascii_alphanumeric() || "-_'!?.".contains(*c));
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
    let mut change = pressed(&[KeyCode::ArrowRight, KeyCode::KeyD]) as i32 - pressed(&[KeyCode::ArrowLeft, KeyCode::KeyA]) as i32;
    let mut chosen = pressed(&[KeyCode::Enter, KeyCode::Space]);

    // The pointer chooses whatever it is moved onto, and a click works it.
    let at = pointer(&window);
    let moved = at != *pointed;
    *pointed = at;
    if let Some(at) = at {
        let over = items.iter().position(|item| item.enabled && hit(item, &art, at).is_some());
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
            if items.iter().any(|item| !item.enabled && hit(item, &art, at).is_some()) {
                sfx.play(id::MENU_REFUSE);
            }
        }
    }
    if focus != menu.focus {
        menu.focus = focus;
        menu.drawn = false;
        sfx.play(id::MENU_HIGHLIGHT);
    }

    let go = |menu: &mut Menu, page: Page, focus: usize| {
        (menu.page, menu.focus, menu.drawn) = (page, focus, false);
    };
    if pressed(&[KeyCode::Escape]) {
        let back = match menu.page {
            Page::Main => None,
            Page::SingleRace | Page::CircuitRace | Page::TimeRace | Page::Options | Page::Online => Some(Page::Main),
            Page::GameOptions | Page::VideoOptions | Page::AudioOptions | Page::Extras => Some(Page::Options),
            Page::Host | Page::Join => Some(Page::Online),
            Page::Password => Some(Page::Join),
            // Backing out of a session is leaving it.
            Page::Room | Page::Connecting => {
                net::leave(&mut commands, &mut session, &mut settings, None, &mut next);
                Some(Page::Online)
            }
        };
        if let Some(back) = back {
            sfx.play(id::MENU_BACK);
            go(&mut menu, back, back.first());
        }
        return;
    }

    let action = items[focus].action;
    // Steps a choice round its `count` options.
    let turn = |value: usize, count: usize| (value as i32 + change).rem_euclid(count as i32) as usize;
    if change != 0 {
        // In the room it is the player's wish for the next race that is changed, and
        // not their own settings.
        let mut wish = (menu.page == Page::Room).then(|| Wired { role: *role, session: &session, room: &room, lobby: &lobby, online: &online }.wish(&settings, &circuits));
        let voting = wish.is_some();
        let settings: &mut Settings = match &mut wish {
            Some(wish) => wish,
            None => &mut settings,
        };
        let races: Vec<usize> = (0..circuits.0.len()).collect();
        let here = group(&circuits, settings.circuit);
        match action {
            // The room has one selector for the circuit, which goes round them all.
            Action::RaceChoice if voting => settings.circuit = turn(settings.circuit, circuits.0.len()),
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
            Action::Series => championship.chosen = turn(championship.chosen, championship.series.len().max(1)),
            Action::RaceChoice => {
                let within: Vec<usize> = races.into_iter().filter(|&r| group(&circuits, r) == here).collect();
                let at = within.iter().position(|&r| r == settings.circuit).unwrap_or(0);
                settings.circuit = within[turn(at, within.len())];
            }
            Action::Opponents => settings.opponents = turn(settings.opponents, MAX_OPPONENTS + 1),
            Action::Laps => settings.lap_choice = turn(settings.lap_choice, LAP_CHOICES.len()),
            Action::Difficulty => settings.difficulty = turn(settings.difficulty, DIFFICULTIES.len()),
            Action::Music => settings.music = (settings.music as i32 + change).clamp(0, MAX_VOLUME as i32) as usize,
            Action::Sound => settings.sound = (settings.sound as i32 + change).clamp(0, MAX_VOLUME as i32) as usize,
            Action::Extra(extra) => settings.turn(extra, change),
            Action::Car => settings.car = turn(settings.car, crate::roster::NAMES.len() + 1),
            _ => {}
        }
        if let Some(wish) = wish {
            (room.ballot, room.revision) = (Some(Rules::of(&wish, &circuits)), room.revision + 1);
        }
        if matches!(items[focus].widget, Widget::Selector { .. } | Widget::Slider { .. }) {
            sfx.play(if matches!(action, Action::Music | Action::Sound) { id::MENU_SLIDER } else { id::MENU_SELECT });
            menu.drawn = false;
        }
    }
    if chosen {
        match action {
            Action::Go(page) => {
                let forward = !matches!(page, Page::Main) && !(page == Page::Options && menu.page != Page::Main) && !(page == Page::Online && menu.page != Page::Main);
                sfx.play(if forward { id::MENU_CONFIRM } else { id::MENU_BACK });
                match page {
                    // A session is called after its host until it is called something else.
                    Page::Host if online.title.is_empty() => online.title = format!("{}'S RACE", settings.name).chars().take(TITLE_LENGTH).collect(),
                    Page::Join => online.refresh = 0.0,
                    _ => {}
                }
                go(&mut menu, page, if page == Page::Online { 2 } else { page.first() });
            }
            // Typed, a field is done with: on to the next thing.
            Action::Type(_) => {
                (menu.focus, menu.drawn) = (step(1), false);
                sfx.play(id::MENU_HIGHLIGHT);
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
                        net::host_session(&mut commands, &mut session, &settings, &circuits, online.title.trim(), &online.password);
                    }
                    // A locked session wants its password first.
                    (Action::Pick(_), Some(listed)) if listed.locked => {
                        sfx.play(id::MENU_CONFIRM);
                        (online.picked, online.key) = (Some(listed), String::new());
                        go(&mut menu, Page::Password, 0);
                    }
                    (_, Some(listed)) => {
                        sfx.play(id::MENU_CONFIRM);
                        net::join_session(&mut commands, &mut session, &settings, &circuits, &listed, &online.key);
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
                    settings.circuit = circuits.0.iter().position(|c| c.race.as_deref() == Some(folder.as_str())).unwrap_or(0);
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
    (role, session, room, lobby, online): (Res<Role>, Res<Session>, Res<Room>, Res<Lobby>, Res<Online>),
) {
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
    let wired = Wired { role: *role, session: &session, room: &room, lobby: &lobby, online: &online };
    let items = items(menu.page, art, &circuits, &settings, &championship, &wired);
    let labels = labels(menu.page, art);
    let notes = notes(menu.page, &wired, &settings, &circuits);
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
                let corner = if $centred { area.center() - size / 2.0 } else { Vec2::new(area.min.x, area.center().y - size.y / 2.0) };
                pieces.push((handle, Rect::from_corners(corner.round(), corner.round() + size), $colour, false));
            }
        };
    }

    picture!("backdrp", Vec2::ZERO, Color::WHITE);
    if menu.page == Page::Main {
        picture!("racers", art.place("main", "racers").min, Color::WHITE);
    }
    if matches!(menu.page, Page::SingleRace | Page::CircuitRace | Page::TimeRace) {
        // The frame the original shows the circuit in; here it holds the race's settings.
        let frame = art.place("race", "brickbox");
        fills.push((frame, BOX_FILL));
        let (corner, edge) = (16.0, frame.size() - Vec2::splat(32.0));
        let inner = frame.min + Vec2::splat(corner);
        for (name, at, size) in [
            ("tul", frame.min, Vec2::splat(corner)),
            ("tt", Vec2::new(inner.x, frame.min.y), Vec2::new(edge.x, corner)),
            ("tur", Vec2::new(frame.max.x - corner, frame.min.y), Vec2::splat(corner)),
            ("tr", Vec2::new(frame.max.x - corner, inner.y), Vec2::new(corner, edge.y)),
            ("tbr", frame.max - Vec2::splat(corner), Vec2::splat(corner)),
            ("tb", Vec2::new(inner.x, frame.max.y - corner), Vec2::new(edge.x, corner)),
            ("tbl", Vec2::new(frame.min.x, frame.max.y - corner), Vec2::splat(corner)),
            ("tl", Vec2::new(frame.min.x, inner.y), Vec2::new(corner, edge.y)),
        ] {
            if let Some((handle, _)) = art.picture(name, &mut images) {
                pieces.push((handle, Rect::from_corners(at, at + size), Color::WHITE, true));
            }
        }
        let summary = if menu.page == Page::CircuitRace {
            // The races the circuit is made of.
            let name = |folder: &String| circuits.0.iter().find(|c| c.race.as_ref() == Some(folder)).map_or(folder.clone(), |c| c.name.clone());
            let rounds = championship.series.get(championship.chosen).map(|s| s.rounds.iter().map(name).collect::<Vec<_>>());
            rounds.unwrap_or_default().join("\n\n")
        } else if menu.page == Page::TimeRace {
            format!("{}\n\nLAPS {}", circuits.0[settings.circuit].name, crate::time_race::LAPS)
        } else {
            // Whichever of the port's ways of racing are on, under the original's settings.
            let mut extras: Vec<String> = Vec::new();
            let reversed = crate::variant::Variant::of(&settings, &championship, circuits.0[settings.circuit].race.as_deref()).reverse;
            for (on, extra) in [(settings.mirror, Extra::Mirror), (reversed, Extra::Reverse), (settings.eliminating(), Extra::Elimination)] {
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
    for (index, item) in items.iter().enumerate() {
        let colour = match (item.enabled, index == focus) {
            (false, _) => DISABLED,
            (true, true) => SELECTED,
            (true, false) => NORMAL,
        };
        // Arrows are plain until their widget is the chosen one.
        let arrows = if index == focus { ["arrowls", "arrowrs"] } else { ["arrowlu", "arrowru"] };
        match &item.widget {
            Widget::Button { at, label, icon } => {
                if let Some(icon) = icon {
                    picture!(icon, *at, colour);
                }
                let line = Rect::from_corners(*at + Vec2::X * ICON, *at + Vec2::new(ICON, ICON));
                words!("font_ths", label, line, colour, false);
            }
            Widget::Selector { area, picture, words } => {
                let middle = area.center().y - ICON / 2.0;
                picture!(arrows[0], Vec2::new(area.min.x, middle), Color::WHITE);
                picture!(arrows[1], Vec2::new(area.max.x - ICON, middle), Color::WHITE);
                if let Some((handle, size)) = picture.as_deref().and_then(|name| art.picture(name, &mut images)) {
                    let corner = (area.center() - size / 2.0).round();
                    pieces.push((handle, Rect::from_corners(corner, corner + size), Color::WHITE, false));
                }
                if !words.is_empty() {
                    words!("font_ths", words, *area, colour, true);
                }
            }
            Widget::Field { area, words } => {
                fills.push((*area, BOX_FILL));
                // Where the next letter goes is marked while it is being typed into.
                words!("font_ths", words, Rect::from_corners(area.min + Vec2::X * 6.0, area.max), colour, false);
                if index == focus {
                    let width = if words.is_empty() { 0.0 } else { art.write("font_ths", words, true, &mut images).map_or(0.0, |written| written.1.x) };
                    let at = Vec2::new(area.min.x + 8.0 + width, area.min.y + 5.0);
                    fills.push((Rect::from_corners(at, at + Vec2::new(3.0, 22.0)), SELECTED));
                }
            }
            Widget::Slider { area, value } => {
                // Quieter and louder at the ends, and between them a rail with a thumb.
                let middle = area.center().y;
                picture!("sounddwn", Vec2::new(area.min.x, middle - 32.0), Color::WHITE);
                picture!("soundup", Vec2::new(area.max.x - 64.0, middle - 32.0), Color::WHITE);
                let rail = Rect::new(area.min.x + 64.0, middle - 16.0, area.max.x - 64.0, middle + 16.0);
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
    commands.spawn((Root, screen, BackgroundColor(Color::BLACK))).with_children(|root| {
        let canvas = Node { width: side(SCREEN.x), height: side(SCREEN.y), overflow: Overflow::clip(), ..default() };
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
                    NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 1.0 }
                } else {
                    NodeImageMode::Stretch
                };
                ImageNode { image: handle, color: colour, image_mode, ..default() }
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
