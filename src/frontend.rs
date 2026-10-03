//! The original game's front end, drawn from its own menu data: the screen layouts
//! (`.MIB`), pictures, bitmap fonts and string tables in `MENUDATA`. Main menu, single
//! race and options are here; the screens for things the port doesn't have (building,
//! circuit race, versus, time race, video, controls) are shown but can't be chosen.
//!
//! Colours and the make-up of each widget follow the styles in `GSTYLES.MSB`. The
//! spinning models the original shows on these screens are not drawn.

use crate::assets::{
    Jam,
    font::{Font, load_fonts, load_strings},
    image::decode_bmp,
    tokens::{Token, tokenize},
};
use crate::audio::{Sfx, id};
use crate::menu::{Circuits, DIFFICULTIES, LAP_CHOICES, MAX_OPPONENTS, MAX_VOLUME, Screen, Settings};
use bevy::{
    asset::RenderAssetUsages,
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
    pub const VERSUS_RACE: usize = 35;
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
const CIRCUIT_ICONS: [&str; 5] = ["pirate", "islander", "magical", "rr", "bricks"];

#[derive(Clone, Copy, PartialEq, Default)]
enum Page {
    #[default]
    Main,
    SingleRace,
    Options,
    GameOptions,
    AudioOptions,
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
    Opponents,
    Laps,
    Difficulty,
    Music,
    Sound,
}

enum Widget {
    /// A picture and some words: the picture is `icon` or nothing.
    Button { at: Vec2, label: String, icon: Option<&'static str> },
    /// Arrows either side of a picture or some words.
    Selector { area: Rect, picture: Option<String>, words: String },
    /// A rail with a thumb on it, `value` steps of `MAX_VOLUME` along.
    Slider { area: Rect, value: usize },
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
    let race = circuits.0[index].race.as_deref();
    race.and_then(|r| r.chars().nth(5)).and_then(|c| c.to_digit(10)).map_or(CIRCUIT_ICONS.len() - 1, |g| g as usize)
}

/// The widgets of a page that can be chosen or changed, top to bottom.
fn items(page: Page, art: &Art, circuits: &Circuits, settings: &Settings) -> Vec<Item> {
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
            button("main", "circuit", text::CIRCUIT_RACE, Action::Nothing, None),
            button("main", "single", text::SINGLE_RACE, Action::Go(Page::SingleRace), None),
            button("main", "vs", text::VERSUS_RACE, Action::Nothing, None),
            button("main", "time", text::TIME_RACE, Action::Nothing, None),
            button("main", "options", text::OPTIONS, Action::Go(Page::Options), None),
            button("main", "quit", text::QUIT, Action::Quit, None),
        ],
        Page::SingleRace => {
            let icon = CIRCUIT_ICONS[group(circuits, settings.circuit)].to_string();
            let name = circuits.0[settings.circuit].name.clone();
            vec![
                selector(art.place("race", "selector"), Some(icon), String::new(), Action::Circuit),
                selector(art.place("race", "racesel"), None, name, Action::RaceChoice),
                button("race", "gonext", text::OK, Action::Race, Some("chck")),
                back("race", Page::Main),
            ]
        }
        Page::Options => vec![
            button("options", "game", text::GAME_OPTIONS, Action::Go(Page::GameOptions), None),
            button("options", "video", text::VIDEO_OPTIONS, Action::Nothing, None),
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
        Page::AudioOptions => vec![
            Item { widget: Widget::Slider { area: art.place("options", "musicvol"), value: settings.music }, action: Action::Music, enabled: true },
            Item { widget: Widget::Slider { area: art.place("options", "soundvol"), value: settings.sound }, action: Action::Sound, enabled: true },
            back("options", Page::Options),
        ],
    }
}

/// Words that go beside the widgets of a page: (where, what, which font).
fn labels(page: Page, art: &Art) -> Vec<(Rect, String, &'static str)> {
    let beside = |name: &str, words: String| (art.place("options", name), words, "font_ths");
    let banner = |words: usize| (Rect::new(375.0, 20.0, 375.0, 68.0), art.string(words), "fontmenu");
    match page {
        Page::Main => Vec::new(),
        Page::SingleRace => vec![banner(text::SINGLE_RACE)],
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
        Page::AudioOptions => vec![
            banner(text::AUDIO_OPTIONS),
            beside("mvoltext", art.string(text::MUSIC_VOLUME)),
            beside("svoltext", art.string(text::SOUND_VOLUME)),
        ],
    }
}

fn enter(mut menu: ResMut<Menu>) {
    *menu = Menu::default();
    menu.focus = 2;
    // `BRICK_MENU=race` (or options, game, audio) opens on that page, for screenshots.
    let page = match std::env::var("BRICK_MENU").as_deref() {
        Ok("race") => Page::SingleRace,
        Ok("options") => Page::Options,
        Ok("game") => Page::GameOptions,
        Ok("audio") => Page::AudioOptions,
        _ => return,
    };
    (menu.page, menu.focus) = (page, 0);
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
        Widget::Selector { area, .. } | Widget::Slider { area, .. } => {
            let end = if matches!(item.widget, Widget::Slider { .. }) { 64.0 } else { ICON };
            area.contains(at).then(|| {
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
) {
    let items = items(menu.page, &art, &circuits, &settings);
    let pressed = |codes: &[KeyCode]| keys.any_just_pressed(codes.iter().copied());
    let focus = menu.focus.min(items.len() - 1);
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
            Page::SingleRace | Page::Options => Some(Page::Main),
            Page::GameOptions | Page::AudioOptions => Some(Page::Options),
        };
        if let Some(back) = back {
            sfx.play(id::MENU_BACK);
            go(&mut menu, back, if back == Page::Main { 2 } else { 0 });
        }
        return;
    }

    let action = items[focus].action;
    // Steps a choice round its `count` options.
    let turn = |value: usize, count: usize| (value as i32 + change).rem_euclid(count as i32) as usize;
    if change != 0 {
        let races: Vec<usize> = (0..circuits.0.len()).collect();
        let here = group(&circuits, settings.circuit);
        match action {
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
            _ => {}
        }
        if !matches!(action, Action::Go(_) | Action::Race | Action::Quit | Action::Nothing) {
            sfx.play(if matches!(action, Action::Music | Action::Sound) { id::MENU_SLIDER } else { id::MENU_SELECT });
            menu.drawn = false;
        }
    }
    if chosen {
        match action {
            Action::Go(page) => {
                let forward = !matches!(page, Page::Main) && !(page == Page::Options && menu.page != Page::Main);
                sfx.play(if forward { id::MENU_CONFIRM } else { id::MENU_BACK });
                go(&mut menu, page, if page == Page::Main { 2 } else { 0 });
            }
            Action::Race => {
                sfx.play(id::MENU_CONFIRM);
                next.set(Screen::Race);
            }
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
    window: Single<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<Root>>,
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
    let items = items(menu.page, art, &circuits, &settings);
    let labels = labels(menu.page, art);
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
    if menu.page == Page::SingleRace {
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
        let summary = format!(
            "{}\n\nLAPS {}\n\nOPPONENTS {}\n\n{}",
            circuits.0[settings.circuit].name,
            settings.laps(),
            settings.opponents,
            DIFFICULTIES[settings.difficulty].0
        );
        words!("font_ths", &summary, frame, LABEL, true);
    }
    for (area, text, font) in &labels {
        words!(font, text, *area, LABEL, true);
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
