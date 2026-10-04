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
}

#[derive(Resource)]
pub struct Circuits(pub Vec<Circuit>);

impl Circuits {
    /// Every circuit in the original game's data, if it's there, and our own.
    pub fn find() -> Self {
        let mut circuits: Vec<Circuit> = world::circuits()
            .into_iter()
            .map(|(race, name)| Circuit { name, race: Some(race), layout: Layout::default() })
            .collect();
        circuits.extend(Layout::ALL.map(|layout| Circuit { name: layout.name().into(), race: None, layout }));
        Circuits(circuits)
    }
}

pub const LAP_CHOICES: [i32; 4] = [1, 3, 5, 7];
pub const MAX_OPPONENTS: usize = 5;
pub const DIFFICULTIES: [(&str, f32); 3] = [("Easy", 0.92), ("Normal", 1.0), ("Hard", 1.06)];

#[derive(Resource)]
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
}

impl Settings {
    /// Starts on the circuit named by `$LEGO_RACE`, if there is one: a folder of the
    /// original's, or a built-in circuit's key.
    pub fn new(circuits: &Circuits) -> Self {
        let wanted = std::env::var("LEGO_RACE").ok();
        let circuit = circuits.0.iter().position(|c| match &c.race {
            Some(_) => c.race == wanted,
            None => wanted.as_deref() == Some(c.layout.key()),
        });
        Settings { circuit: circuit.unwrap_or(0), lap_choice: 1, championship: None, time_race: false, opponents: MAX_OPPONENTS, difficulty: 1, music: 14, sound: MAX_VOLUME }
    }

    /// How many of the computer's cars race: none against the clock.
    pub fn field(&self) -> usize {
        if self.time_race { 0 } else { self.opponents }
    }

    pub fn laps(&self) -> i32 {
        if self.time_race {
            return crate::time_race::LAPS as i32;
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

pub const MAX_VOLUME: usize = 20;
const ROWS: usize = 7;

#[derive(Component)]
struct Row(usize);

/// The highlighted row.
#[derive(Resource, Default)]
struct Cursor(usize);

pub fn plugin(app: &mut App) {
    app.init_resource::<Cursor>()
        // The plain menu is only for when the original's menu data isn't there.
        .add_systems(OnEnter(Screen::Menu), spawn_menu.run_if(not(resource_exists::<crate::frontend::Art>)))
        .add_systems(
            Update,
            menu.run_if(in_state(Screen::Menu)).run_if(not(resource_exists::<crate::frontend::Art>)),
        );
}

fn spawn_menu(mut commands: Commands) {
    let text = |size: f32| (TextFont { font_size: FontSize::Px(size), ..default() }, TextShadow::default());
    commands
        .spawn((
            DespawnOnExit(Screen::Menu),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(14.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.10, 0.16)),
        ))
        .with_children(|menu| {
            menu.spawn((Text::new("BRICK RACERS"), text(72.0), TextColor(YELLOW)));
            menu.spawn(Node { height: Val::Px(24.0), ..default() });
            for row in 0..ROWS {
                menu.spawn((Row(row), Text::new(""), text(34.0)));
            }
            menu.spawn(Node { height: Val::Px(24.0), ..default() });
            menu.spawn((
                Text::new(
                    "Up / Down: choose    Left / Right: change    Enter: race\n\
                     In a race    WASD / arrows: drive    Shift: powerslide    Space: power-up    Esc: menu",
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
        let step = |value: usize, count: usize| (value as isize + change).rem_euclid(count as isize) as usize;
        match cursor.0 {
            0 => settings.circuit = step(settings.circuit, circuits.0.len()),
            1 => settings.lap_choice = step(settings.lap_choice, LAP_CHOICES.len()),
            2 => settings.opponents = step(settings.opponents, MAX_OPPONENTS + 1),
            3 => settings.difficulty = step(settings.difficulty, DIFFICULTIES.len()),
            4 => settings.music = step(settings.music, MAX_VOLUME + 1),
            5 => settings.sound = step(settings.sound, MAX_VOLUME + 1),
            _ => {}
        }
        match cursor.0 {
            4 | 5 => sfx.play(id::MENU_SLIDER),
            6 => {}
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
            3 => option("Difficulty", DIFFICULTIES[settings.difficulty].0.to_string()),
            4 => option("Music", settings.music.to_string()),
            5 => option("Sound", settings.sound.to_string()),
            _ => "Start race".to_string(),
        };
        if text.0 != line {
            text.0 = line;
        }
        let wanted = if row.0 == cursor.0 { YELLOW } else { Color::WHITE };
        if colour.0 != wanted {
            colour.0 = wanted;
        }
    }
}
