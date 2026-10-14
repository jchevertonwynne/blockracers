//! The builder's panels: what is on them, and what the pointer does on them. The
//! port's own.
//!
//! A panel is rows of cells, each some words and perhaps something that pressing
//! it does (`Act`). The screen (`view`) says each frame what the panels are to
//! have on them (`Sheet`) and this module puts it there, changing the words of
//! the cells that are there already where it can, so that a panel is the same
//! cells from frame to frame while it is being used. The pointer is the
//! builder's own (`pointer::Pointer`), which a demo's script can be, and not the
//! window's: what is under it is found here from where the cells have been laid.

use super::Layer;
use super::pointer::Pointer;
use crate::menu::Screen;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// What pressing a place on a panel does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    /// On the list of maps: one of them, opening the one it is on, deleting it,
    /// and leaving.
    Shelf(usize),
    Open,
    Delete,
    Leave,
    /// A kind of thing, and putting another of that kind on the map.
    Kind(Layer),
    Add(Layer),
    /// A row of the thing picked, turning it up or down so many steps, and typing
    /// into it.
    Row(usize),
    Turn(usize, f32),
    Type(usize),
    Undo,
    Redo,
    /// Looking at the whole map.
    Whole,
    /// Joining a road being drawn to its start, or taking a circuit's last
    /// stretch away to draw more.
    Loop,
    Try,
    Keep,
    /// Saying what the keys do, or not.
    Help,
}

/// How a cell is drawn.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Look {
    Plain,
    Dim,
    Title,
    /// The row or the kind that is picked.
    Lit,
    Button,
    /// A button that would do nothing as things are.
    Off,
    Good,
    Wrong,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Cell {
    pub words: String,
    pub act: Option<Act>,
    pub look: Look,
    /// How wide it is, or nothing for as wide as its words.
    pub wide: f32,
}

impl Cell {
    pub fn new(words: impl Into<String>, look: Look) -> Cell {
        Cell { words: words.into(), act: None, look, wide: 0.0 }
    }

    pub fn wide(self, wide: f32) -> Cell {
        Cell { wide, ..self }
    }

    pub fn does(self, act: Act) -> Cell {
        Cell { act: Some(act), ..self }
    }

    /// A button, which does something unless it is `off`.
    pub fn button(words: &str, act: Act, off: bool) -> Cell {
        match off {
            true => Cell::new(words, Look::Off),
            false => Cell::new(words, Look::Button).does(act),
        }
    }
}

/// The panels there are: down the left side, across the top right, what was last
/// said under that, and what the pointer and the keys do along the bottom.
const SIDE: usize = 0;
const BAR: usize = 1;
const SAID: usize = 2;
const HINT: usize = 3;

/// What the panels are to have on them.
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Sheet {
    pub side: Vec<Vec<Cell>>,
    pub bar: Vec<Cell>,
    pub said: Vec<Cell>,
    pub hint: Vec<String>,
}

impl Sheet {
    /// Each panel's rows.
    fn panels(&self) -> [Vec<Vec<Cell>>; 4] {
        let lone = |cells: &Vec<Cell>| if cells.is_empty() { Vec::new() } else { vec![cells.clone()] };
        let hint = self.hint.iter().map(|line| vec![Cell::new(line.as_str(), Look::Plain)]).collect();
        [self.side.clone(), lone(&self.bar), lone(&self.said), hint]
    }

    /// How many cells each row of each panel has, and how wide each is: while
    /// that is the same, the cells that are there are kept.
    fn shape(&self) -> Vec<Vec<Vec<u32>>> {
        let widths = |row: &Vec<Cell>| row.iter().map(|cell| cell.wide as u32).collect();
        self.panels().iter().map(|panel| panel.iter().map(widths).collect()).collect()
    }
}

/// What is on the panels, and what the pointer is over.
#[derive(Resource, Default)]
pub struct Board {
    shown: Sheet,
    over: Option<(usize, usize, usize)>,
    /// The cell the button is held on, if holding it does it again, and when.
    held: Option<(Act, f32)>,
}

/// What has been asked for on the panels, for the screen to do.
#[derive(Resource, Default)]
pub struct Asked(pub Vec<Act>);

/// A panel, and a cell of one: which panel, which row and which along it.
#[derive(Component)]
pub struct Pane(usize);

#[derive(Component)]
pub struct Spot(usize, usize, usize);

const WORDS: f32 = 15.0;
const SMALL: f32 = 12.5;
/// How long the button is held on something before it is done again, and how
/// often it then is.
const AGAIN: (f32, f32) = (0.35, 0.07);

fn colours(look: Look, over: bool) -> (Color, Color) {
    let clear = Color::NONE;
    match (look, over) {
        (Look::Plain, false) => (Color::WHITE, clear),
        (Look::Plain, true) => (Color::WHITE, Color::srgba(1.0, 1.0, 1.0, 0.16)),
        (Look::Dim, _) => (Color::srgb(0.72, 0.76, 0.86), clear),
        (Look::Title, _) => (Color::srgb(1.0, 0.92, 0.3), clear),
        (Look::Lit, false) => (Color::WHITE, Color::srgba(0.25, 0.45, 1.0, 0.75)),
        (Look::Lit, true) => (Color::WHITE, Color::srgba(0.35, 0.55, 1.0, 0.9)),
        (Look::Button, false) => (Color::WHITE, Color::srgba(1.0, 1.0, 1.0, 0.18)),
        (Look::Button, true) => (Color::BLACK, Color::srgba(1.0, 0.92, 0.3, 0.95)),
        (Look::Off, _) => (Color::srgba(1.0, 1.0, 1.0, 0.35), Color::srgba(1.0, 1.0, 1.0, 0.06)),
        (Look::Good, _) => (Color::srgb(0.75, 1.0, 0.75), clear),
        (Look::Wrong, _) => (Color::srgb(1.0, 0.62, 0.5), clear),
    }
}

/// Puts the panels up, empty, as the builder is come to.
pub fn open(mut commands: Commands, mut board: ResMut<Board>, mut asked: ResMut<Asked>) {
    (*board, asked.0) = (Board::default(), Vec::new());
    let back = BackgroundColor(Color::srgba(0.02, 0.03, 0.12, 0.78));
    let pane = |pane: usize, node: Node| {
        (
            DespawnOnExit(Screen::Build),
            Pane(pane),
            back,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(1.0),
                ..node
            },
        )
    };
    let edge = Val::Px(10.0);
    commands.spawn(pane(SIDE, Node { left: edge, top: edge, ..default() }));
    commands.spawn(pane(BAR, Node { right: edge, top: edge, ..default() }));
    commands.spawn(pane(SAID, Node { right: edge, top: Val::Px(62.0), max_width: Val::Px(560.0), ..default() }));
    commands.spawn(pane(HINT, Node { left: edge, bottom: edge, max_width: Val::Px(1240.0), ..default() }));
}

/// Has the panels say what `sheet` has: with the cells there are, if they are
/// as many and as wide as it wants, and with new ones if not.
pub fn lay(
    commands: &mut Commands,
    board: &mut Board,
    sheet: Sheet,
    panes: &mut Query<(Entity, &Pane, &mut Visibility)>,
    cells: &mut Query<(&Spot, &mut Text, &mut TextColor, &mut BackgroundColor), Without<Pane>>,
) {
    let panels = sheet.panels();
    if sheet.shape() != board.shown.shape() || cells.is_empty() {
        for (entity, pane, mut visibility) in panes.iter_mut() {
            let rows = &panels[pane.0];
            *visibility = if rows.is_empty() { Visibility::Hidden } else { Visibility::Visible };
            let size = if pane.0 == HINT { SMALL } else { WORDS };
            // What is said runs on to the next line, and a row of cells doesn't.
            let wrapped = if matches!(pane.0, SAID | HINT) { TextLayout::default() } else { TextLayout::no_wrap() };
            commands.entity(entity).despawn_related::<Children>().with_children(|panel| {
                for (row, line) in rows.iter().enumerate() {
                    let across = Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(3.0), ..default() };
                    panel.spawn(across).with_children(|across| {
                        for (column, cell) in line.iter().enumerate() {
                            let (ink, back) = colours(cell.look, false);
                            let width = if cell.wide > 0.0 { Val::Px(cell.wide) } else { Val::Auto };
                            across.spawn((
                                Spot(pane.0, row, column),
                                Text::new(cell.words.clone()),
                                wrapped,
                                TextFont { font_size: FontSize::Px(size), ..default() },
                                TextColor(ink),
                                BackgroundColor(back),
                                Node {
                                    width,
                                    padding: UiRect::axes(Val::Px(5.0), Val::Px(2.0)),
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                            ));
                        }
                    });
                }
            });
        }
    } else {
        for (spot, mut text, mut ink, mut back) in cells.iter_mut() {
            let Some(cell) = panels[spot.0].get(spot.1).and_then(|row| row.get(spot.2)) else {
                continue;
            };
            if text.0 != cell.words {
                text.0 = cell.words.clone();
            }
            let over = cell.act.is_some() && board.over == Some((spot.0, spot.1, spot.2));
            let (now, behind) = colours(cell.look, over);
            if ink.0 != now {
                ink.0 = now;
            }
            if back.0 != behind {
                back.0 = behind;
            }
        }
    }
    board.shown = sheet;
}

/// Finds what the pointer is over on the panels, and asks for what it presses
/// there. A pointer over a panel is not over the map.
pub fn pressed(
    mut board: ResMut<Board>,
    mut asked: ResMut<Asked>,
    mut pointer: ResMut<Pointer>,
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    panes: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<Pane>>,
    cells: Query<(&Spot, &ComputedNode, &UiGlobalTransform)>,
) {
    board.over = None;
    let Some(at) = pointer.at.map(|at| at * window.scale_factor()) else {
        board.held = None;
        return;
    };
    let on = |node: &ComputedNode, place: &UiGlobalTransform| node.contains_point(*place, at);
    pointer.taken = panes
        .iter()
        .any(|(node, place, visibility)| *visibility != Visibility::Hidden && on(node, place));
    if !pointer.taken {
        board.held = None;
        return;
    }
    let panels = board.shown.panels();
    let over = cells.iter().find(|(_, node, place)| on(node, place)).map(|(spot, ..)| (spot.0, spot.1, spot.2));
    let act = over.and_then(|(pane, row, column)| panels[pane].get(row)?.get(column)?.act);
    board.over = over;
    let now = time.elapsed_secs();
    // The wheel turns the row it is over, a step to a notch.
    if let (Some(Act::Row(row) | Act::Turn(row, _)), true) = (act, pointer.wheel != 0.0) {
        asked.0.push(Act::Turn(row, pointer.wheel.signum()));
    }
    pointer.wheel = 0.0;
    if pointer.left.pressed {
        asked.0.extend(act);
        board.held = act.filter(|act| matches!(act, Act::Turn(..))).map(|act| (act, now + AGAIN.0));
    } else if !pointer.left.down {
        board.held = None;
    } else if let Some((held, at)) = board.held
        && Some(held) == act
        && now >= at
    {
        asked.0.push(held);
        board.held = Some((held, now + AGAIN.1));
    }
}
