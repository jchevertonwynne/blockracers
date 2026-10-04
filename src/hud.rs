//! The race display. With the original game's data it is drawn as the original draws
//! it (see `original`); without, as plain text.

use crate::kart::{Kart, Player};
use crate::menu::Settings;
use crate::{Phase, Race};
use bevy::prelude::*;

#[path = "hud_original.rs"]
pub mod original;

#[derive(Component)]
pub struct LapText;
#[derive(Component)]
pub struct PlaceText;
#[derive(Component)]
pub struct TimeText;
#[derive(Component)]
pub struct ItemBox;
#[derive(Component)]
pub struct ItemText;
#[derive(Component)]
pub struct CenterText;

fn font(size: f32) -> TextFont {
    TextFont { font_size: FontSize::Px(size), ..default() }
}

fn corner(top: Val, bottom: Val, left: Val, right: Val) -> Node {
    Node { position_type: PositionType::Absolute, top, bottom, left, right, ..default() }
}

pub fn setup_text_hud(mut commands: Commands) {
    let (auto, edge) = (Val::Auto, Val::Px(16.0));
    commands.spawn((LapText, Text::new(""), font(34.0), TextShadow::default(), corner(edge, auto, edge, auto)));
    commands.spawn((PlaceText, Text::new(""), font(48.0), TextShadow::default(), corner(edge, auto, auto, edge)));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: edge,
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(TimeText, Text::new(""), font(28.0), TextShadow::default())],
    ));
    commands.spawn((
        ItemBox,
        Node { padding: UiRect::all(Val::Px(12.0)), ..corner(auto, edge, edge, auto) },
        BackgroundColor(Color::NONE),
        children![(ItemText, Text::new(""), font(26.0))],
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(22.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            CenterText,
            Text::new(""),
            font(44.0),
            TextShadow::default(),
            TextLayout::justify(Justify::Center),
        )],
    ));
}

fn ordinal(place: usize) -> &'static str {
    ["1st", "2nd", "3rd", "4th", "5th", "6th"].get(place - 1).copied().unwrap_or("-")
}

fn clock(t: f32) -> String {
    format!("{}:{:05.2}", (t / 60.0) as u32, t % 60.0)
}

pub fn update_text_hud(
    race: Res<Race>,
    settings: Res<Settings>,
    replay: Res<crate::replay::Replay>,
    karts: Query<(&Kart, Has<Player>)>,
    mut lap: Single<&mut Text, (With<LapText>, Without<PlaceText>, Without<TimeText>, Without<ItemText>, Without<CenterText>)>,
    mut place: Single<&mut Text, (With<PlaceText>, Without<TimeText>, Without<ItemText>, Without<CenterText>)>,
    mut timer: Single<&mut Text, (With<TimeText>, Without<ItemText>, Without<CenterText>)>,
    mut item: Single<&mut Text, (With<ItemText>, Without<CenterText>)>,
    mut center: Single<&mut Text, With<CenterText>>,
    mut item_box: Single<&mut BackgroundColor, With<ItemBox>>,
) {
    let Some((player, _)) = karts.iter().find(|k| k.1) else { return };
    let set = |text: &mut Text, value: String| {
        if text.0 != value {
            text.0 = value;
        }
    };

    set(&mut lap, format!("LAP {}/{}", player.display_lap(settings.laps()), settings.laps()));
    set(&mut place, ordinal(player.place).to_string());
    set(&mut timer, clock(player.finished.unwrap_or(race.time)));

    let (label, colour) = match player.held {
        Some(p) => (
            p.name(player.whites).to_string(),
            p.color().with_alpha(0.85),
        ),
        // White bricks waiting for a colour to go with.
        None if player.whites > 0 => (
            format!("{} white", player.whites),
            Color::srgba(0.5, 0.5, 0.5, 0.85),
        ),
        None => (String::new(), Color::NONE),
    };
    set(&mut item, label);
    item_box.set_if_neq(BackgroundColor(colour));

    let message = match race.phase {
        Phase::Intro => String::new(),
        Phase::Countdown => format!("{}", race.countdown.ceil() as u32),
        Phase::Racing if race.time < 1.0 => "GO!".to_string(),
        Phase::Racing if player.wrong_way() => "WRONG WAY".to_string(),
        Phase::Racing => String::new(),
        Phase::Finished if replay.showing.is_some() => "REPLAY\n\nLeft / Right: car    T: camera    ESC: back".to_string(),
        Phase::Finished => {
            let mut rows: Vec<&Kart> = karts.iter().map(|k| k.0).collect();
            rows.sort_by_key(|k| k.place);
            let mut out = format!("You finished {}!\n\n", ordinal(player.place));
            for k in rows {
                let time = k.finished.map_or("--".to_string(), clock);
                out += &format!("{}  {}  {}\n", ordinal(k.place), k.name, time);
            }
            out + "\nENTER: race again    ESC: menu\nR: replay    P: photo"
        }
    };
    set(&mut center, message);
}
