//! The front end: pick a circuit and a few race settings.

use crate::audio::{Sfx, id};
use crate::map::Map;
use crate::meshgen::YELLOW;
use crate::world;
use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Screen {
    #[default]
    Menu,
    Race,
    /// Between two races of a circuit: straight on to the next.
    Loading,
    /// The map builder (`builder`), the port's own.
    Build,
}

pub struct Circuit {
    pub name: String,
    /// Who made it, for one of the port's own maps that says.
    pub author: String,
    /// Folder in the original game's archive; `None` is one of the built-in circuits.
    pub race: Option<String>,
    /// Which of the port's own maps, when it is one.
    pub map: Option<Map>,
    /// A map a session's host has and this game hasn't, until the host sends it
    /// with a race's start: what it is known by, and whether it is an arena.
    pub hosts: Option<(String, bool)>,
    /// Which of the game's circuits the race is one of, counting from none; the
    /// built-in circuits are in a set of their own after the last.
    pub group: usize,
}

impl Circuit {
    /// Whether it is one of the arenas, which are for a battle or a game of tag.
    pub fn arena(&self) -> bool {
        self.map.as_ref().is_some_and(Map::arena)
            || self.hosts.as_ref().is_some_and(|hosts| hosts.1)
    }

    /// Who made it, as the menus say after its name; nothing where nobody is said to.
    pub fn credit(&self) -> String {
        match self.author.as_str() {
            "" => String::new(),
            author => format!(" BY {author}"),
        }
    }

    /// The map its road is built from: its own, or for one of the game's circuits
    /// that didn't load, the first the game carries.
    pub fn map(&self) -> &Map {
        self.map.as_ref().unwrap_or(&crate::map::stock()[0])
    }

    /// What it is known by between games, to `$BRICK_RACE` and to the ghosts kept
    /// of it: its folder in the game's data, or its map's key.
    pub fn key(&self) -> &str {
        let hosts = self.hosts.as_ref().map(|hosts| hosts.0.as_str());
        self.race.as_deref().or(hosts).unwrap_or(&self.map().key)
    }
}

#[derive(Resource)]
pub struct Circuits(pub Vec<Circuit>);

impl Circuits {
    /// Every circuit in the original game's data, if it's there, and the maps the
    /// game carries.
    #[cfg(test)]
    pub fn find() -> Self {
        Circuits::of(crate::map::stock().to_vec())
    }

    /// Every circuit in the original game's data, if it's there, and these maps.
    pub fn of(maps: Vec<Map>) -> Self {
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
                    author: String::new(),
                    race: Some(race),
                    map: None,
                    hosts: None,
                    group,
                }
            })
            .collect();
        // The arenas come last, and are for a battle or a game of tag online.
        for map in maps {
            // A map named as a folder of the game's would never be found by name.
            if circuits.iter().any(|c| c.key() == map.key) || map.key == TEST_TRACK {
                crate::home::say(&format!(
                    "the map {} is left out: the game has a race called that",
                    map.key
                ));
                continue;
            }
            circuits.push(Circuit {
                name: map.name.clone(),
                author: map.author.clone(),
                race: None,
                map: Some(map),
                hosts: None,
                group: sets,
            });
        }
        Circuits(circuits)
    }

    /// Takes a map as a session's host has it, to be raced in place of this
    /// game's own of that name, or as well as its own: which circuit it then is,
    /// or what is wrong with it.
    pub fn lend(&mut self, key: &str, text: &str) -> Result<usize, String> {
        let had = self
            .0
            .iter()
            .position(|c| c.race.is_none() && c.key() == key);
        if let Some(at) =
            had.filter(|&at| self.0[at].hosts.is_none() && self.0[at].map().text == text)
        {
            return Ok(at);
        }
        if self.0.iter().any(|c| c.race.as_deref() == Some(key)) || key == TEST_TRACK {
            return Err("the game has a race called that".into());
        }
        let mut map = Map::tried(key, text)?;
        match had {
            Some(at) => {
                // Its file is still this game's own, to be read again later.
                map.file = self.0[at].map().file.clone();
                (self.0[at].name, self.0[at].author) = (map.name.clone(), map.author.clone());
                (self.0[at].map, self.0[at].hosts) = (Some(map), None);
                Ok(at)
            }
            None => {
                self.0.push(Circuit {
                    name: map.name.clone(),
                    author: map.author.clone(),
                    race: None,
                    map: Some(map),
                    hosts: None,
                    group: self.own(),
                });
                Ok(self.0.len() - 1)
            }
        }
    }

    /// Takes a map made or changed in the builder, in place of the one of its
    /// name or as another.
    pub fn keep(&mut self, map: Map) {
        let group = self.own();
        match self
            .0
            .iter_mut()
            .find(|c| c.race.is_none() && c.key() == map.key)
        {
            Some(had) => {
                (had.name, had.author) = (map.name.clone(), map.author.clone());
                (had.map, had.hosts) = (Some(map), None);
            }
            None => self.0.push(Circuit {
                name: map.name.clone(),
                author: map.author.clone(),
                race: None,
                map: Some(map),
                hosts: None,
                group,
            }),
        }
    }

    /// Takes a map the builder has in hand to be raced as it is, kept or not:
    /// which circuit it is, until the builder takes it away again.
    pub fn try_out(&mut self, text: &str) -> Result<usize, String> {
        let key = crate::builder::TRIAL;
        self.0.retain(|c| c.key() != key);
        let map = Map::tried(key, text)?;
        let group = self.own();
        self.0.push(Circuit {
            name: map.name.clone(),
            author: map.author.clone(),
            race: None,
            map: Some(map),
            hosts: None,
            group,
        });
        Ok(self.0.len() - 1)
    }

    /// The set the port's own maps are in, after the game's circuits.
    fn own(&self) -> usize {
        let maps = self
            .0
            .iter()
            .filter(|c| c.race.is_none())
            .map(|c| c.group)
            .next();
        maps.or(self.0.iter().map(|c| c.group + 1).max())
            .unwrap_or(0)
    }

    /// Takes the name of a map a session's host has and this game hasn't, to be
    /// voted for: the map itself comes with the start of a race on it (`lend`).
    pub fn promise(&mut self, key: &str, name: &str, author: &str, arena: bool) {
        if self.0.iter().any(|c| c.key() == key) || key == TEST_TRACK {
            return;
        }
        self.0.push(Circuit {
            name: name.chars().take(lobby_api::MAX_NAME).collect(),
            author: author.chars().take(lobby_api::MAX_NAME).collect(),
            race: None,
            map: None,
            hosts: Some((key.to_string(), arena)),
            group: self.own(),
        });
    }

    /// Forgets the maps a host promised and never sent, the session being over.
    pub fn forget(&mut self) {
        self.0.retain(|c| c.hosts.is_none());
    }

    /// Reads a circuit's map again if its file has changed, so that a map being
    /// worked on is raced as it now is.
    pub fn reread(&mut self, circuit: usize) {
        if let Some(c) = self.0.get_mut(circuit)
            && let Some(map) = c.map.as_ref().and_then(Map::reread)
        {
            info!("read {} again", map.key);
            (c.name, c.author, c.map) = (map.name.clone(), map.author.clone(), Some(map));
        }
    }
}

impl Circuits {
    /// What `GarageScreen::StartTestDrive` does: the race to run is `test` of the race
    /// table (the folder of the same name), one racer alone on it. The circuit is put
    /// among the others for as long as the drive lasts, and `settings` keeps the one
    /// that was chosen to go back to.
    pub fn begin_test_drive(&mut self, settings: &mut Settings) {
        if settings.test_drive.is_some() {
            return;
        }
        settings.test_drive = Some(settings.circuit);
        let group = self.0.iter().map(|c| c.group).max().map_or(0, |g| g + 1);
        self.0.push(Circuit {
            name: "Test track".into(),
            author: String::new(),
            race: Some(TEST_TRACK.into()),
            map: None,
            hosts: None,
            group,
        });
        settings.circuit = self.0.len() - 1;
    }

    /// The test drive is over: its circuit is gone from among the others, wherever
    /// it now is among them, and the circuit chosen before it is chosen again.
    /// Whether there was a drive.
    pub fn end_test_drive(&mut self, settings: &mut Settings) -> bool {
        let Some(before) = settings.test_drive.take() else {
            return false;
        };
        self.0.retain(|c| c.race.as_deref() != Some(TEST_TRACK));
        settings.circuit = before;
        true
    }
}

/// The folder of the race the garage's test drive is run on.
pub const TEST_TRACK: &str = "TEST";

/// The last three are longer than any race of the original's.
pub const LAP_CHOICES: [i32; 7] = [1, 3, 5, 7, 10, 15, 20];
pub const MAX_OPPONENTS: usize = 5;
/// The longest name a player may go by online.
pub const NAME_LENGTH: usize = 10;
const CIRCUIT_LAPS: i32 = 3;
pub const DIFFICULTIES: [(&str, f32); 3] = [("Easy", 0.92), ("Normal", 1.0), ("Hard", 1.06)];

/// How a race online is won. The port's own, all but the first; those after the
/// second are `net::modes`'.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// To the line, as the original races.
    #[default]
    Race,
    /// The last car is put out each lap (`rules::elimination`).
    Elimination,
    /// Every car has a few lives, and a hit takes one; the last car left wins.
    Battle,
    /// One car is "it" until it bumps another, which is "it" then; the car that is
    /// when the clock runs out is out, and the last car left wins.
    Tag,
    /// One car is "it", and every car it or another that is "it" bumps is "it"
    /// from then on; whoever is caught last wins.
    Infection,
    /// One car holds the flag until another bumps it and has it; whoever has held
    /// it longest wins.
    KeepAway,
    /// A car alone on the hill is its king; whoever has been king longest wins.
    Hill,
}

impl Mode {
    pub const ALL: [Mode; 7] = [
        Mode::Race,
        Mode::Elimination,
        Mode::Battle,
        Mode::Tag,
        Mode::Infection,
        Mode::KeepAway,
        Mode::Hill,
    ];

    /// Whether laps don't end it: it is then fought out, and in an arena (`arena`).
    pub fn lapless(self) -> bool {
        !matches!(self, Mode::Race | Mode::Elimination)
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Race => "Race",
            Mode::Elimination => "Elimination",
            Mode::Battle => "Battle",
            Mode::Tag => "Tag",
            Mode::Infection => "Infection",
            Mode::KeepAway => "Keep away",
            Mode::Hill => "King of the hill",
        }
    }
}

#[derive(Resource, Clone)]
pub struct Settings {
    pub circuit: usize,
    /// A test drive from the garage is on (`Circuits::begin_test_drive`): the circuit
    /// that was chosen before it.
    pub test_drive: Option<usize>,
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
    /// How a race online is won, which its host says (`Rules`). A game alone races.
    pub mode: Mode,
    /// How many other cars are in the race online, players' and the computer's
    /// together; nothing for a game alone, where they are `opponents`.
    pub rivals: Option<usize>,
    /// The wheels go to full lock almost at once, instead of turning as slowly as the
    /// original's do.
    pub quick_steering: bool,
    /// Which of `SPEEDS` the cars go at.
    pub speed: usize,
    /// A battle online, as its host has it: the lives each car begins with, and
    /// which of `BATTLE_TIMES` it is fought for and of `SPARES` a car that has
    /// lost a life is spared another for.
    pub lives: usize,
    pub battle_time: usize,
    pub spared: usize,
    /// A game of tag online: which of `TAG_LEASTS` is the least on the clock for a
    /// car just caught, and of `TAG_BACKS` how long it can't catch another.
    pub tag_least: usize,
    pub tag_back: usize,
    /// Keep-away and king of the hill online: which of `GOALS` wins them.
    pub goal: usize,
    /// The games laps don't end are played by two teams (`net::modes::team`),
    /// and which each grid slot's car is of in the race being run: a bit to a
    /// slot, set for the second team. The host's to say (`net::Lineup::seat`).
    pub teams: bool,
    pub sides: u8,
    /// A car with no lives left in a battle comes back to it (`net::modes::battle`).
    pub returns: bool,
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
    /// What the keys and a pad's buttons do in a race.
    pub controls: crate::input::Bindings,
    /// Which of `assets::font::LANGUAGES` the game's words are in
    /// (`GameState::m_languageIndex`, the original's English to begin with).
    pub language: usize,
}

/// What may be done with a circuit's bricks: left alone, every coloured one made the
/// same colour, none put out at all, each coloured one a colour picked afresh every
/// time it appears, each going from colour to colour as it lies, or each a colour
/// for the place of the car coming to it (`items`).
pub const BRICK_RULES: [&str; 9] = [
    "Normal",
    "All red",
    "All yellow",
    "All blue",
    "All green",
    "None",
    "Random",
    "Changing",
    "By place",
];
/// The teams the cars are of until a host says: the even grid slots against the odd.
pub const SIDES: u8 = 0b101010;
/// The rule that picks colours afresh, the one that has them change, and the one
/// that has them go by place.
pub const RANDOM_BRICKS: usize = 6;
pub const CHANGING_BRICKS: usize = 7;
pub const PLACED_BRICKS: usize = 8;

/// How fast the cars may be, the computer's among them, the original's being one
/// (`Kart::pace`).
/// The most lives a car may begin a battle with.
pub const MAX_LIVES: usize = 9;
/// How long a battle may be fought for, in seconds; the last is for as long as it
/// takes.
pub const BATTLE_TIMES: [f32; 7] = [60.0, 120.0, 180.0, 240.0, 300.0, 600.0, f32::INFINITY];
/// How long a car that has just lost a life may be spared another, in seconds.
pub const SPARES: [f32; 6] = [0.0, 0.5, 1.0, 1.5, 2.0, 3.0];
/// The least there may be on the clock for a car just made "it" by a bump, in
/// seconds: with less, the clock is put up to it. None leaves the clock alone.
pub const TAG_LEASTS: [f32; 6] = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
/// How long a car just made "it" may be kept from making another so, in seconds.
pub const TAG_BACKS: [f32; 6] = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
/// How long the flag must be held, or the hill, to win keep-away or king of the
/// hill outright, in seconds.
pub const GOALS: [f32; 5] = [20.0, 30.0, 45.0, 60.0, 90.0];

/// A number of seconds as the menus show it: "Off" for none, "No limit" for no
/// end, minutes for whole ones and seconds otherwise.
fn seconds(time: f32) -> String {
    match time {
        time if time <= 0.0 => "Off".to_string(),
        time if time.is_infinite() => "No limit".to_string(),
        time if time >= 60.0 => format!("{} min", time / 60.0),
        time => format!("{time} s"),
    }
}

pub const SPEEDS: [(&str, f32); 5] = [
    ("Original", 1.0),
    ("1.25x", 1.25),
    ("1.5x", 1.5),
    ("1.75x", 1.75),
    ("2x", 2.0),
];

/// The settings the original has no counterpart for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Extra {
    Mirror,
    Reverse,
    Bricks,
    Elimination,
    /// The host's, online: which of `Mode::ALL` the race is.
    Mode,
    Steering,
    Speed,
    /// The host's too: how a battle is fought and a game of tag played.
    Lives,
    BattleTime,
    Spared,
    TagLeast,
    TagBack,
    /// And how long wins keep-away and king of the hill.
    Goal,
    /// And whether they are played by two teams, and whether a car knocked out
    /// of a battle comes back.
    Teams,
    Returns,
    VSync,
    Fullscreen,
    Smoothing,
}

impl Extra {
    pub const RACE: [Extra; 6] = [
        Extra::Mirror,
        Extra::Reverse,
        Extra::Bricks,
        Extra::Elimination,
        Extra::Steering,
        Extra::Speed,
    ];
    pub const VIDEO: [Extra; 3] = [Extra::VSync, Extra::Fullscreen, Extra::Smoothing];

    /// What the host says of a race that is won as `mode` is, and of no other.
    pub fn of(mode: Mode) -> &'static [Extra] {
        match mode {
            Mode::Battle => &[
                Extra::Lives,
                Extra::BattleTime,
                Extra::Spared,
                Extra::Returns,
                Extra::Teams,
            ],
            Mode::Tag => &[Extra::TagLeast, Extra::TagBack, Extra::Teams],
            Mode::Infection => &[Extra::BattleTime],
            Mode::KeepAway => &[Extra::Goal, Extra::BattleTime, Extra::TagBack, Extra::Teams],
            Mode::Hill => &[Extra::Goal, Extra::BattleTime, Extra::Teams],
            Mode::Race | Mode::Elimination => &[],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Extra::Mirror => "Mirrored",
            Extra::Reverse => "Reversed",
            Extra::Bricks => "Bricks",
            Extra::Elimination => "Elimination",
            Extra::Mode => "Mode",
            Extra::Steering => "Steering",
            Extra::Speed => "Car speed",
            Extra::Lives => "Lives",
            Extra::BattleTime => "Time limit",
            Extra::Spared => "Spared",
            Extra::TagLeast => "Late catch",
            Extra::TagBack => "No catch back",
            Extra::Goal => "First to",
            Extra::Teams => "Teams",
            Extra::Returns => "Knocked out",
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
        let circuit = circuits.0.iter().position(|c| {
            wanted
                .as_deref()
                .is_some_and(|wanted| wanted.eq_ignore_ascii_case(c.key()))
        });
        Settings {
            circuit: circuit.unwrap_or(0),
            test_drive: None,
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
            mode: Mode::Race,
            rivals: None,
            quick_steering: false,
            speed: 0,
            lives: 3,
            battle_time: 3,
            spared: 3,
            tag_least: 3,
            tag_back: 2,
            goal: 1,
            teams: false,
            sides: SIDES,
            returns: false,
            vsync: true,
            fullscreen: false,
            smoothing: true,
            name: "PLAYER".into(),
            car: 0,
            racer: 0,
            controls: default(),
            language: 0,
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
            ("speed", self.speed),
            ("lives", self.lives),
            ("battle_time", self.battle_time),
            ("spared", self.spared),
            ("tag_least", self.tag_least),
            ("tag_back", self.tag_back),
            ("goal", self.goal),
            ("teams", on(self.teams)),
            ("returns", on(self.returns)),
            ("vsync", on(self.vsync)),
            ("fullscreen", on(self.fullscreen)),
            ("smoothing", on(self.smoothing)),
            ("car", self.car),
            ("racer", self.racer),
            ("language", self.language),
        ];
        let numbers: String = kept
            .iter()
            .map(|(name, value)| format!("{name}={value}\n"))
            .collect();
        format!("{numbers}name={}\n{}", self.name, self.controls.write())
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
            if self.controls.read(line) {
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
                "speed" if value < SPEEDS.len() => self.speed = value,
                "lives" if (1..=MAX_LIVES).contains(&value) => self.lives = value,
                "battle_time" if value < BATTLE_TIMES.len() => self.battle_time = value,
                "spared" if value < SPARES.len() => self.spared = value,
                "tag_least" if value < TAG_LEASTS.len() => self.tag_least = value,
                "tag_back" if value < TAG_BACKS.len() => self.tag_back = value,
                "goal" if value < GOALS.len() => self.goal = value,
                "teams" => self.teams = on,
                "returns" => self.returns = on,
                "vsync" => self.vsync = on,
                "fullscreen" => self.fullscreen = on,
                "smoothing" => self.smoothing = on,
                // Past the game's drivers are the garage's racers, however many it has.
                "car" => self.car = value,
                "racer" => self.racer = value,
                "language" if value < crate::assets::font::LANGUAGES.len() => self.language = value,
                _ => {}
            }
        }
        self.controls.mend();
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
        (self.elimination || self.mode == Mode::Elimination)
            && self.single()
            && self.rivals.unwrap_or(self.opponents) > 0
    }

    /// Whether the race is one of those that laps don't end (`net::modes`).
    pub fn lapless(&self) -> bool {
        self.mode.lapless()
    }

    /// The rule the race's bricks follow, as an index into `BRICK_RULES`.
    pub fn brick_rule(&self) -> usize {
        if self.single() { self.bricks } else { 0 }
    }

    /// How long a battle is fought for, and a car that has lost a life is spared
    /// another; the least on the clock for a car caught in a game of tag, and how
    /// long it can't catch another. In seconds.
    pub fn battle_time(&self) -> f32 {
        BATTLE_TIMES[self.battle_time.min(BATTLE_TIMES.len() - 1)]
    }

    pub fn spared(&self) -> f32 {
        SPARES[self.spared.min(SPARES.len() - 1)]
    }

    pub fn tag_least(&self) -> f32 {
        TAG_LEASTS[self.tag_least.min(TAG_LEASTS.len() - 1)]
    }

    pub fn tag_back(&self) -> f32 {
        TAG_BACKS[self.tag_back.min(TAG_BACKS.len() - 1)]
    }

    /// How long the flag or the hill must be held to win, in seconds.
    pub fn goal(&self) -> f32 {
        GOALS[self.goal.min(GOALS.len() - 1)]
    }

    /// Steps an extra setting on or back.
    pub fn turn(&mut self, extra: Extra, change: i32) {
        let flip = |on: &mut bool| *on = !*on;
        let step =
            |at: &mut usize, of: usize| *at = (*at as i32 + change).rem_euclid(of as i32) as usize;
        match extra {
            Extra::Mirror => flip(&mut self.mirror),
            Extra::Reverse => flip(&mut self.reverse),
            Extra::Bricks => {
                self.bricks =
                    (self.bricks as i32 + change).rem_euclid(BRICK_RULES.len() as i32) as usize
            }
            Extra::Elimination => flip(&mut self.elimination),
            Extra::Mode => {
                let at = Mode::ALL.iter().position(|mode| *mode == self.mode);
                let to = at.unwrap_or(0) as i32 + change;
                self.mode = Mode::ALL[to.rem_euclid(Mode::ALL.len() as i32) as usize]
            }
            Extra::Steering => flip(&mut self.quick_steering),
            Extra::Speed => {
                self.speed = (self.speed as i32 + change).rem_euclid(SPEEDS.len() as i32) as usize
            }
            // From one life to the most, and round.
            Extra::Lives => {
                self.lives =
                    (self.lives as i32 - 1 + change).rem_euclid(MAX_LIVES as i32) as usize + 1
            }
            Extra::BattleTime => step(&mut self.battle_time, BATTLE_TIMES.len()),
            Extra::Spared => step(&mut self.spared, SPARES.len()),
            Extra::TagLeast => step(&mut self.tag_least, TAG_LEASTS.len()),
            Extra::TagBack => step(&mut self.tag_back, TAG_BACKS.len()),
            Extra::Goal => step(&mut self.goal, GOALS.len()),
            Extra::Teams => flip(&mut self.teams),
            Extra::Returns => flip(&mut self.returns),
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
            Extra::Mode => self.mode.label().to_string(),
            Extra::Steering => if self.quick_steering {
                "Quick"
            } else {
                "Original"
            }
            .to_string(),
            Extra::Speed => SPEEDS[self.speed].0.to_string(),
            Extra::Lives => self.lives.to_string(),
            Extra::BattleTime => seconds(self.battle_time()),
            Extra::Spared => seconds(self.spared()),
            Extra::TagLeast => seconds(self.tag_least()),
            Extra::TagBack => seconds(self.tag_back()),
            Extra::Goal => seconds(self.goal()),
            Extra::Teams => on(self.teams),
            Extra::Returns => if self.returns {
                "Comes back"
            } else {
                "For good"
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
        if self.time_race || self.test_drive.is_some() {
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
            return self.rivals.unwrap_or(self.opponents) as i32;
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

    /// How many times as fast as the original's the cars are.
    pub fn pace(&self) -> f32 {
        SPEEDS[self.speed].1
    }

    /// Multiplier on the AI drivers' top speed.
    pub fn ai_pace(&self) -> f32 {
        DIFFICULTIES[self.difficulty].1
    }
}

/// Where the settings are kept: `$BRICK_SETTINGS`, or `settings` in `home`'s folder.
fn settings_file() -> Option<std::path::PathBuf> {
    crate::home::kept("BRICK_SETTINGS", "settings")
}

/// Keeps the settings whenever they change, for the next session.
pub fn keep(settings: Res<Settings>, mut kept: Local<Option<String>>) {
    if !settings.is_changed() {
        return;
    }
    let text = settings.write();
    // The first look is at what was just restored, which is on file already.
    if kept.as_ref().is_some_and(|kept| *kept != text)
        && let Some(Err(error)) = settings_file().map(|file| crate::home::write(&file, &text))
    {
        warn!("could not keep the settings: {error}");
    }
    *kept = Some(text);
}

pub const MAX_VOLUME: usize = 20;
/// The plain menu's rows: the six settings it always had, the extras, and the start.
const EXTRAS: [Extra; 9] = [
    Extra::Mirror,
    Extra::Reverse,
    Extra::Bricks,
    Extra::Elimination,
    Extra::Steering,
    Extra::Speed,
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
            0 => {
                let circuit = &circuits.0[settings.circuit];
                option(
                    "Circuit",
                    format!("{}{}", circuit.name, circuit.credit().to_lowercase()),
                )
            }
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
    settings.speed = 4;
    (settings.mirror, settings.vsync, settings.elimination) = (true, false, true);
    let mut back = Settings::new(&Circuits(Vec::new()));
    back.read(&settings.write());
    assert_eq!(back.write(), settings.write());
    assert_eq!(
        (back.lap_choice, back.opponents, back.music, back.bricks),
        (4, 2, 7, 3)
    );
    assert!(back.mirror && !back.vsync && back.elimination && !back.reverse && back.smoothing);
    assert_eq!(back.pace(), 2.0);
    // A circuit race is three laps against a full field, and leaves the settings be.
    back.championship = Some("c0".into());
    assert_eq!(
        (back.laps(), back.field(), back.lap_choice, back.opponents),
        (3, MAX_OPPONENTS, 4, 2)
    );
    (back.championship, back.elimination) = (None, false);
    assert_eq!((back.laps(), back.field()), (LAP_CHOICES[4], 2));
    // Nonsense and things out of range are passed over.
    back.read("laps=99\nbricks=six\nfuel=3\nopponents=1\n\nmusic 4\nspeed=5");
    assert_eq!(back.speed, 4);
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
    // The built-in circuits come last, in a set of their own, and the arenas after them.
    assert_eq!((listed.len(), listed[13].1, listed[16].1), (21, 4, 4));
    let arenas: Vec<bool> = circuits.0.iter().map(Circuit::arena).collect();
    assert!(arenas[..17].iter().all(|arena| !arena) && arenas[17..].iter().all(|arena| *arena));
}

#[cfg(test)]
#[test]
fn the_language_is_kept_with_the_settings() {
    let mut settings = Settings::new(&Circuits(Vec::new()));
    assert_eq!(settings.language, 0);
    settings.language = 7;
    let mut back = Settings::new(&Circuits(Vec::new()));
    back.read(&settings.write());
    assert_eq!(back.language, 7);
    // One past the nine the original lists is left alone.
    back.read("language=9\n");
    assert_eq!(back.language, 7);
}

#[cfg(test)]
#[test]
fn a_host_s_map_is_raced_in_place_of_this_game_s_own() {
    let mut circuits = Circuits::find();
    let count = circuits.0.len();
    let own = crate::map::named("BRICK");
    let brick = circuits.0.iter().position(|c| c.key() == "BRICK").unwrap();
    // The same map is the one this game has.
    assert_eq!(circuits.lend("BRICK", &own.text), Ok(brick));
    assert_eq!(circuits.0[brick].map(), own);
    // Another of the same name is raced in its place, and one this game hasn't
    // is another circuit, among the port's own.
    let theirs = own
        .text
        .replace("Brick Circuit", "Their Circuit")
        .replace("[88, 0, 8, 32]", "[90, 0, 8, 30]");
    assert_eq!(circuits.lend("BRICK", &theirs), Ok(brick));
    assert_eq!(circuits.0[brick].name, "Their Circuit");
    assert_eq!(
        circuits.0[brick].map().road.corners[1],
        [90.0, 0.0, 8.0, 30.0]
    );
    assert_eq!(circuits.lend("THEIRS", &theirs), Ok(count));
    assert_eq!(
        (circuits.0[count].key(), circuits.0[count].group),
        ("THEIRS", circuits.0[brick].group)
    );
    assert!(!circuits.0[count].arena());
    // One that is no map, or is named as a race of the game's, is refused.
    assert!(circuits.lend("BROKEN", "name = 3").is_err());
    assert!(circuits.lend(TEST_TRACK, &own.text).is_err());
    assert!(circuits.lend("../BRICK", &theirs).is_err());
    assert_eq!(circuits.0.len(), count + 1);
}

#[cfg(test)]
#[test]
fn a_map_changed_on_file_is_read_again() {
    let file = std::env::temp_dir().join(format!("brick_reread_{}.toml", std::process::id()));
    let own = crate::map::named("BRICK");
    std::fs::write(&file, &own.text).unwrap();
    let mut circuits = Circuits::of(vec![Map {
        file: Some(file.clone()),
        ..own.clone()
    }]);
    let at = circuits.0.len() - 1;
    circuits.reread(at);
    assert_eq!(circuits.0[at].map().text, own.text);
    // Changed, it is the map as it now is; broken, it is left as it was.
    std::fs::write(&file, own.text.replace("Brick Circuit", "Brick Circus")).unwrap();
    circuits.reread(at);
    assert_eq!(circuits.0[at].name, "Brick Circus");
    assert_eq!(circuits.0[at].map().file.as_ref(), Some(&file));
    std::fs::write(&file, "name = \"Brick Circus\"\ncorners = [[0, 0, 0, 0]]\n").unwrap();
    circuits.reread(at);
    assert_eq!(
        circuits.0[at].map().road.corners.len(),
        own.road.corners.len()
    );
    let _ = std::fs::remove_file(file);
}

#[cfg(test)]
#[test]
fn a_host_s_map_is_voted_for_before_this_game_has_it() {
    let mut circuits = Circuits::find();
    let count = circuits.0.len();
    // One this game has is its own; one it hasn't is there by name alone.
    circuits.promise("BRICK", "Brick Circuit", "", false);
    circuits.promise("PEN", "The Pen", "Penny", true);
    assert_eq!(circuits.0[count].credit(), " BY Penny");
    assert_eq!(circuits.0.len(), count + 1);
    let pen = &circuits.0[count];
    assert_eq!(
        (pen.key(), pen.name.as_str(), pen.arena()),
        ("PEN", "The Pen", true)
    );
    assert_eq!(pen.group, circuits.0[count - 1].group);
    // Sent with a start, it is that map; never sent, it goes with the session.
    circuits.promise("OVAL", "Oval", "Someone", false);
    let oval = "name = \"Oval\"\ncorners = [[0, 0, 0, 0], [100, 0, 0, 30], [100, 0, 80, 30], [-100, 0, 80, 30], [-100, 0, 0, 30]]\n";
    assert_eq!(circuits.lend("OVAL", oval), Ok(count + 1));
    assert!(circuits.0[count + 1].hosts.is_none() && circuits.0[count + 1].map.is_some());
    assert_eq!(circuits.0[count + 1].credit(), "");
    circuits.forget();
    let keys: Vec<&str> = circuits.0[count..].iter().map(Circuit::key).collect();
    assert_eq!(keys, ["OVAL"]);
}
