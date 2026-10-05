//! The front end: pick a circuit and a few race settings.

use crate::audio::{Sfx, id};
use crate::meshgen::YELLOW;
use crate::track::Layout;
use crate::world;
use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Screen {
    #[default]
    Menu,
    Race,
    /// Between two races of a circuit: straight on to the next.
    Loading,
}

pub struct Circuit {
    pub name: String,
    /// Folder in the original game's archive; `None` is one of the built-in circuits.
    pub race: Option<String>,
    /// Which built-in circuit, when it is one.
    pub layout: Layout,
    /// Which of the game's circuits the race is one of, counting from none; the
    /// built-in circuits are in a set of their own after the last.
    pub group: usize,
}

#[derive(Resource)]
pub struct Circuits(pub Vec<Circuit>);

impl Circuits {
    /// Every circuit in the original game's data, if it's there, and our own.
    pub fn find() -> Self {
        // In the order the circuits run them, which is not the order of their folders.
        let order = world::circuit_order();
        let place = |race: &str| {
            order
                .iter()
                .position(|o| o.0 == race)
                .unwrap_or(order.len())
        };
        let mut found = world::circuits();
        found.sort_by_key(|(race, _)| place(race));
        let sets = order.iter().map(|o| o.1 + 1).max().unwrap_or(0);
        let mut circuits: Vec<Circuit> = found
            .into_iter()
            .map(|(race, name)| {
                let group = order.get(place(&race)).map_or(sets, |o| o.1);
                Circuit {
                    name,
                    race: Some(race),
                    layout: Layout::default(),
                    group,
                }
            })
            .collect();
        circuits.extend(Layout::ALL.map(|layout| Circuit {
            name: layout.name().into(),
            race: None,
            layout,
            group: sets,
        }));
        Circuits(circuits)
    }
}

/// The last three are longer than any race of the original's.
pub const LAP_CHOICES: [i32; 7] = [1, 3, 5, 7, 10, 15, 20];
pub const MAX_OPPONENTS: usize = 5;
/// The longest name a player may go by online.
pub const NAME_LENGTH: usize = 10;
const CIRCUIT_LAPS: i32 = 3;
pub const DIFFICULTIES: [(&str, f32); 3] = [("Easy", 0.92), ("Normal", 1.0), ("Hard", 1.06)];

#[derive(Resource, Clone)]
pub struct Settings {
    pub circuit: usize,
    pub lap_choice: usize,
    /// The circuit (`c0` and so on) being raced for, when the race is one of a series.
    pub championship: Option<String>,
    /// Racing the clock, alone but for the ghosts.
    pub time_race: bool,
    pub opponents: usize,
    pub difficulty: usize,
    /// Steps of the original's volume sliders, 0 to 20.
    pub music: usize,
    pub sound: usize,
    /// What the port adds to the original's game. All start off.
    pub mirror: bool,
    /// Round the circuit the other way.
    pub reverse: bool,
    /// Which of `BRICK_RULES` the circuit's bricks follow.
    pub bricks: usize,
    /// The last car is put out each lap, until one is left.
    pub elimination: bool,
    /// The wheels go to full lock almost at once, instead of turning as slowly as the
    /// original's do.
    pub quick_steering: bool,
    pub vsync: bool,
    pub fullscreen: bool,
    /// Edges smoothed by multisampling.
    pub smoothing: bool,
    /// What the player goes by when racing online.
    pub name: String,
    /// Who the player races as online: one of `roster::NAMES`, counted from one, or
    /// nought for whoever has the grid slot they are given; past the last of those
    /// names, one of the garage's racers (`Garage::ride`).
    pub car: usize,
    /// Which of the garage's racers the player races as, counted from one; nought
    /// while there are none, when the game's stand-in races.
    pub racer: usize,
}

/// What may be done with a circuit's bricks: left alone, every coloured one made the
/// same colour, none put out at all, or each coloured one a colour picked afresh every
/// time it appears.
pub const BRICK_RULES: [&str; 7] = [
    "Normal",
    "All red",
    "All yellow",
    "All blue",
    "All green",
    "None",
    "Random",
];
/// The rule that picks colours afresh.
pub const RANDOM_BRICKS: usize = 6;

/// The settings the original has no counterpart for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Extra {
    Mirror,
    Reverse,
    Bricks,
    Elimination,
    Steering,
    VSync,
    Fullscreen,
    Smoothing,
}

impl Extra {
    pub const RACE: [Extra; 5] = [
        Extra::Mirror,
        Extra::Reverse,
        Extra::Bricks,
        Extra::Elimination,
        Extra::Steering,
    ];
    pub const VIDEO: [Extra; 3] = [Extra::VSync, Extra::Fullscreen, Extra::Smoothing];

    pub fn label(self) -> &'static str {
        match self {
            Extra::Mirror => "Mirrored",
            Extra::Reverse => "Reversed",
            Extra::Bricks => "Bricks",
            Extra::Elimination => "Elimination",
            Extra::Steering => "Steering",
            Extra::VSync => "Frame rate",
            Extra::Fullscreen => "Full screen",
            Extra::Smoothing => "Smooth edges",
        }
    }
}

impl Settings {
    /// Starts on the circuit named by `$BRICK_RACE`, if there is one: a folder of the
    /// original's, or a built-in circuit's key.
    pub fn new(circuits: &Circuits) -> Self {
        let wanted = std::env::var("BRICK_RACE").ok();
        let circuit = circuits.0.iter().position(|c| match &c.race {
            Some(_) => c.race == wanted,
            None => wanted.as_deref() == Some(c.layout.key()),
        });
        Settings {
            circuit: circuit.unwrap_or(0),
            lap_choice: 1,
            championship: None,
            time_race: false,
            opponents: MAX_OPPONENTS,
            difficulty: 1,
            music: 14,
            sound: MAX_VOLUME,
            mirror: false,
            reverse: false,
            bricks: 0,
            elimination: false,
            quick_steering: false,
            vsync: true,
            fullscreen: false,
            smoothing: true,
            name: "PLAYER".into(),
            car: 0,
            racer: 0,
        }
    }

    /// The settings that are kept between sessions, a line each. Which circuit and
    /// what kind of race are chosen afresh each time.
    fn write(&self) -> String {
        let on = |on: bool| on as usize;
        let kept = [
            ("laps", self.lap_choice),
            ("opponents", self.opponents),
            ("difficulty", self.difficulty),
            ("music", self.music),
            ("sound", self.sound),
            ("mirror", on(self.mirror)),
            ("reverse", on(self.reverse)),
            ("bricks", self.bricks),
            ("elimination", on(self.elimination)),
            ("steering", on(self.quick_steering)),
            ("vsync", on(self.vsync)),
            ("fullscreen", on(self.fullscreen)),
            ("smoothing", on(self.smoothing)),
            ("car", self.car),
            ("racer", self.racer),
        ];
        let numbers: String = kept
            .iter()
            .map(|(name, value)| format!("{name}={value}\n"))
            .collect();
        format!("{numbers}name={}\n", self.name)
    }

    /// Takes what a file of `write`'s has to say, leaving alone anything it doesn't
    /// mention or that is out of range.
    fn read(&mut self, text: &str) {
        for line in text.lines() {
            if let Some(name) = line
                .strip_prefix("name=")
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                self.name = name.chars().take(NAME_LENGTH).collect();
                continue;
            }
            let Some((name, Ok(value))) = line
                .split_once('=')
                .map(|(name, value)| (name, value.trim().parse::<usize>()))
            else {
                continue;
            };
            let on = value != 0;
            match name {
                "laps" if value < LAP_CHOICES.len() => self.lap_choice = value,
                "opponents" if value <= MAX_OPPONENTS => self.opponents = value,
                "difficulty" if value < DIFFICULTIES.len() => self.difficulty = value,
                "music" if value <= MAX_VOLUME => self.music = value,
                "sound" if value <= MAX_VOLUME => self.sound = value,
                "mirror" => self.mirror = on,
                "reverse" => self.reverse = on,
                "bricks" if value < BRICK_RULES.len() => self.bricks = value,
                "elimination" => self.elimination = on,
                "steering" => self.quick_steering = on,
                "vsync" => self.vsync = on,
                "fullscreen" => self.fullscreen = on,
                "smoothing" => self.smoothing = on,
                // Past the game's drivers are the garage's racers, however many it has.
                "car" => self.car = value,
                "racer" => self.racer = value,
                _ => {}
            }
        }
    }

    /// Takes up the settings left by the last session, if there was one.
    pub fn restore(&mut self) {
        if let Some(text) = settings_file().and_then(|file| std::fs::read_to_string(file).ok()) {
            self.read(&text);
        }
    }

    /// Whether this race is one on its own, which is where the port's rules apply.
    fn single(&self) -> bool {
        !self.time_race && self.championship.is_none()
    }

    /// Whether the last car goes out each lap: it takes someone to race against.
    pub fn eliminating(&self) -> bool {
        self.elimination && self.single() && self.opponents > 0
    }

    /// The rule the race's bricks follow, as an index into `BRICK_RULES`.
    pub fn brick_rule(&self) -> usize {
        if self.single() { self.bricks } else { 0 }
    }

    /// Steps an extra setting on or back.
    pub fn turn(&mut self, extra: Extra, change: i32) {
        let flip = |on: &mut bool| *on = !*on;
        match extra {
            Extra::Mirror => flip(&mut self.mirror),
            Extra::Reverse => flip(&mut self.reverse),
            Extra::Bricks => {
                self.bricks =
                    (self.bricks as i32 + change).rem_euclid(BRICK_RULES.len() as i32) as usize
            }
            Extra::Elimination => flip(&mut self.elimination),
            Extra::Steering => flip(&mut self.quick_steering),
            Extra::VSync => flip(&mut self.vsync),
            Extra::Fullscreen => flip(&mut self.fullscreen),
            Extra::Smoothing => flip(&mut self.smoothing),
        }
    }

    /// An extra setting as the menus show it.
    pub fn shown(&self, extra: Extra) -> String {
        let on = |on: bool| if on { "On" } else { "Off" }.to_string();
        match extra {
            Extra::Mirror => on(self.mirror),
            Extra::Reverse => on(self.reverse),
            Extra::Bricks => BRICK_RULES[self.bricks].to_string(),
            Extra::Elimination => on(self.elimination),
            Extra::Steering => if self.quick_steering {
                "Quick"
            } else {
                "Original"
            }
            .to_string(),
            Extra::VSync => if self.vsync { "Synced" } else { "Unlimited" }.to_string(),
            Extra::Fullscreen => on(self.fullscreen),
            Extra::Smoothing => on(self.smoothing),
        }
    }

    /// How many of the computer's cars race: none against the clock, and a full field
    /// in a circuit race, whatever a single race is set to.
    pub fn field(&self) -> usize {
        if self.time_race {
            0
        } else if self.championship.is_some() {
            MAX_OPPONENTS
        } else {
            self.opponents
        }
    }

    pub fn laps(&self) -> i32 {
        if self.time_race {
            return crate::time_race::LAPS as i32;
        }
        // A circuit's races are three laps each.
        if self.championship.is_some() {
            return CIRCUIT_LAPS;
        }
        // One car goes at the end of each lap, and the last lap leaves the winner.
        if self.eliminating() {
            return self.opponents as i32;
        }
        LAP_CHOICES[self.lap_choice]
    }

    /// The original's music and sound volume scales, 0 to 1.
    pub fn music_volume(&self) -> f32 {
        self.music as f32 / MAX_VOLUME as f32
    }

    pub fn sound_volume(&self) -> f32 {
        self.sound as f32 / MAX_VOLUME as f32
    }

    /// Multiplier on the AI drivers' top speed.
    pub fn ai_pace(&self) -> f32 {
        DIFFICULTIES[self.difficulty].1
    }
}

/// Where the settings are kept: `$BRICK_SETTINGS`, or a file in the home folder.
fn settings_file() -> Option<std::path::PathBuf> {
    match std::env::var_os("BRICK_SETTINGS") {
        Some(file) => Some(file.into()),
        None => {
            Some(std::path::PathBuf::from(std::env::var_os("HOME")?).join(".brick_racers_settings"))
        }
    }
}

/// Keeps the settings whenever they change, for the next session.
pub fn keep(settings: Res<Settings>, mut kept: Local<Option<String>>) {
    if !settings.is_changed() {
        return;
    }
    let text = settings.write();
    // The first look is at what was just restored, which is on file already.
    if kept.as_ref().is_some_and(|kept| *kept != text)
        && let Some(Err(error)) = settings_file().map(|file| std::fs::write(file, &text))
    {
        warn!("could not keep the settings: {error}");
    }
    *kept = Some(text);
}

pub const MAX_VOLUME: usize = 20;
/// The plain menu's rows: the six settings it always had, the extras, and the start.
const EXTRAS: [Extra; 8] = [
    Extra::Mirror,
    Extra::Reverse,
    Extra::Bricks,
    Extra::Elimination,
    Extra::Steering,
    Extra::VSync,
    Extra::Fullscreen,
    Extra::Smoothing,
];
const ROWS: usize = 7 + EXTRAS.len();
const START: usize = ROWS - 1;

#[derive(Component)]
struct Row(usize);

/// The highlighted row.
#[derive(Resource, Default)]
struct Cursor(usize);

pub fn plugin(app: &mut App) {
    app.init_resource::<Cursor>()
        // The plain menu is only for when the original's menu data isn't there.
        .add_systems(
            OnEnter(Screen::Menu),
            spawn_menu.run_if(not(resource_exists::<crate::frontend::Art>)),
        )
        .add_systems(
            Update,
            menu.run_if(in_state(Screen::Menu))
                .run_if(not(resource_exists::<crate::frontend::Art>)),
        );
}

fn spawn_menu(mut commands: Commands) {
    let text = |size: f32| {
        (
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextShadow::default(),
        )
    };
    commands
        .spawn((
            DespawnOnExit(Screen::Menu),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.10, 0.16)),
        ))
        .with_children(|menu| {
            menu.spawn((Text::new("BRICK RACERS"), text(60.0), TextColor(YELLOW)));
            menu.spawn(Node { height: Val::Px(10.0), ..default() });
            for row in 0..ROWS {
                menu.spawn((Row(row), Text::new(""), text(26.0)));
            }
            menu.spawn(Node { height: Val::Px(10.0), ..default() });
            menu.spawn((
                Text::new(
                    "Up / Down: choose    Left / Right: change    Enter: race\n\
                     In a race    WASD / arrows: drive    Shift: powerslide    Space: power-up    Esc: menu\n\
                     P: photo mode    R at the finish: replay",
                ),
                text(18.0),
                TextLayout::justify(Justify::Center),
                TextColor(Color::srgb(0.7, 0.7, 0.75)),
            ));
        });
}

fn menu(
    keys: Res<ButtonInput<KeyCode>>,
    circuits: Res<Circuits>,
    mut settings: ResMut<Settings>,
    mut cursor: ResMut<Cursor>,
    mut next: ResMut<NextState<Screen>>,
    mut sfx: ResMut<Sfx>,
    mut rows: Query<(&Row, &mut Text, &mut TextColor)>,
) {
    let pressed = |codes: [KeyCode; 2]| keys.any_just_pressed(codes);
    if pressed([KeyCode::ArrowDown, KeyCode::KeyS]) {
        cursor.0 = (cursor.0 + 1) % ROWS;
        sfx.play(id::MENU_HIGHLIGHT);
    }
    if pressed([KeyCode::ArrowUp, KeyCode::KeyW]) {
        cursor.0 = (cursor.0 + ROWS - 1) % ROWS;
        sfx.play(id::MENU_HIGHLIGHT);
    }
    let change = pressed([KeyCode::ArrowRight, KeyCode::KeyD]) as usize as isize
        - pressed([KeyCode::ArrowLeft, KeyCode::KeyA]) as usize as isize;
    if change != 0 {
        // Steps a choice round its `count` options.
        let step = |value: usize, count: usize| {
            (value as isize + change).rem_euclid(count as isize) as usize
        };
        match cursor.0 {
            0 => settings.circuit = step(settings.circuit, circuits.0.len()),
            1 => settings.lap_choice = step(settings.lap_choice, LAP_CHOICES.len()),
            2 => settings.opponents = step(settings.opponents, MAX_OPPONENTS + 1),
            3 => settings.difficulty = step(settings.difficulty, DIFFICULTIES.len()),
            4 => settings.music = step(settings.music, MAX_VOLUME + 1),
            5 => settings.sound = step(settings.sound, MAX_VOLUME + 1),
            START => {}
            row => settings.turn(EXTRAS[row - 6], change as i32),
        }
        match cursor.0 {
            4 | 5 => sfx.play(id::MENU_SLIDER),
            START => {}
            _ => sfx.play(id::MENU_SELECT),
        }
    }
    if keys.just_pressed(KeyCode::Enter) {
        sfx.play(id::MENU_CONFIRM);
        next.set(Screen::Race);
    }

    for (row, mut text, mut colour) in &mut rows {
        let option = |label: &str, value: String| format!("{label}    <  {value}  >");
        let line = match row.0 {
            0 => option("Circuit", circuits.0[settings.circuit].name.clone()),
            1 => option("Laps", settings.laps().to_string()),
            2 => option("Opponents", settings.opponents.to_string()),
            3 => option(
                "Difficulty",
                DIFFICULTIES[settings.difficulty].0.to_string(),
            ),
            4 => option("Music", settings.music.to_string()),
            5 => option("Sound", settings.sound.to_string()),
            START => "Start race".to_string(),
            row => option(EXTRAS[row - 6].label(), settings.shown(EXTRAS[row - 6])),
        };
        if text.0 != line {
            text.0 = line;
        }
        let wanted = if row.0 == cursor.0 {
            YELLOW
        } else {
            Color::WHITE
        };
        if colour.0 != wanted {
            colour.0 = wanted;
        }
    }
}

#[cfg(test)]
#[test]
fn settings_come_back_as_they_were_kept() {
    let mut settings = Settings::new(&Circuits(Vec::new()));
    (
        settings.lap_choice,
        settings.opponents,
        settings.music,
        settings.bricks,
    ) = (4, 2, 7, 3);
    (settings.mirror, settings.vsync, settings.elimination) = (true, false, true);
    let mut back = Settings::new(&Circuits(Vec::new()));
    back.read(&settings.write());
    assert_eq!(back.write(), settings.write());
    assert_eq!(
        (back.lap_choice, back.opponents, back.music, back.bricks),
        (4, 2, 7, 3)
    );
    assert!(back.mirror && !back.vsync && back.elimination && !back.reverse && back.smoothing);
    // A circuit race is three laps against a full field, and leaves the settings be.
    back.championship = Some("c0".into());
    assert_eq!(
        (back.laps(), back.field(), back.lap_choice, back.opponents),
        (3, MAX_OPPONENTS, 4, 2)
    );
    (back.championship, back.elimination) = (None, false);
    assert_eq!((back.laps(), back.field()), (LAP_CHOICES[4], 2));
    // Nonsense and things out of range are passed over.
    back.read("laps=99\nbricks=six\nfuel=3\nopponents=1\n\nmusic 4");
    assert_eq!(
        (back.lap_choice, back.bricks, back.opponents, back.music),
        (4, 3, 1, 7)
    );
}

#[cfg(test)]
#[test]
fn single_races_are_listed_in_their_circuits_order() {
    let circuits = Circuits::find();
    // Needs the original game data; without it there are only the built-in circuits.
    if circuits.0.iter().all(|c| c.race.is_none()) {
        return;
    }
    let listed: Vec<(&str, usize)> = circuits
        .0
        .iter()
        .map(|c| (c.name.as_str(), c.group))
        .collect();
    assert_eq!(
        listed[..5],
        [
            ("Imperial Grand Prix", 0),
            ("Dark Forest Dash", 0),
            ("Magma Moon Marathon", 0),
            ("Desert Adventure Dragway", 0),
            ("Tribal Island Trail", 1)
        ]
    );
    assert_eq!(listed[12], ("Rocket Racer Run", 3));
    // The built-in circuits come last, in a set of their own.
    assert_eq!((listed.len(), listed[13].1, listed[15].1), (16, 4, 4));
}
