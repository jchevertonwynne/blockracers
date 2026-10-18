//! The builder's screen: the map seen from above, with what it is made of marked
//! on it, and panels beside it (`panel`). The port's own, and its words are
//! English whatever language is chosen.
//!
//! It opens on the maps there are (`Shelf`), each shown behind the list as it is
//! come to: a new circuit, a new arena, and every map of the folder. One of the
//! game's own is opened as another like it, and the rest as themselves.
//!
//! Everything is done with the pointer (`pointer::Pointer`, which a demo's script
//! can be), and most of it with the keys as well. A road is drawn: the end of one
//! that is not yet a loop is dragged, and the road goes where the pointer does,
//! or the ground is clicked for a corner there, until it is brought back to the
//! straight its start is on (`ground`). Whatever is marked with a ring is
//! dragged by it, and the small squares between a road's corners are dragged
//! out into corners of their own. Dragging the ground slides the view, the right
//! button turns it and the wheel comes nearer to where the pointer is. A panel's
//! rows are pressed to turn them up and down, and its `+` puts another of a kind
//! on the map, which follows the pointer until it is put down.
//!
//! A map that is a circuit or an arena as it stands is built as the game would
//! build it (`show`), with the bricks the game would lay on it, its grid and
//! where its hazards stand marked by blocks of the builder's own, the hazards'
//! models being the game's and not here to show. A road that is not yet a loop,
//! or that something is wrong with, is no map, and is drawn as a bare road
//! (`sketch`). `T` tries the map as it is: a race alone on it (`Trial`), which
//! comes back here when it is left, whatever it was left by.

use super::draw::Stroke;
use super::panel::{self, Act, Asked, Board, Cell, Look, Sheet};
use super::pointer::{self, Pointer, Script};
use super::{Draft, Layer, TRIAL, tint};
use crate::items::Power;
use crate::map::Map;
use crate::menu::{Circuits, Screen, Settings};
use crate::meshgen::*;
use crate::track::{Track, VERGE, WIDTH};
use bevy::camera::visibility::RenderLayers;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_8, TAU};

const SKY: Color = Color::srgb(0.25, 0.55, 0.95);
/// How high the screen is taken to be, for the size of its words.
const SCREEN: f32 = 720.0;
/// How near the pointer must be to a ring to be on it, and how far it must go
/// with the button down to be dragging and not clicking: both in the window's
/// own units.
const HOLD: f32 = 15.0;
const CLICK: f32 = 5.0;
/// How many things have their numbers beside them at once.
const LABELS: usize = 96;
/// How long a key is held before it repeats, and how often it then does.
const REPEAT: (f32, f32) = (0.3, 0.05);
/// How far the view is from what it looks at, at the nearest and the furthest,
/// and how steeply it looks down, in radians.
const FAR: (f32, f32) = (20.0, 2500.0);
const TILT: (f32, f32) = (0.25, 1.5);
/// How far from straight a line drawn must bend to make a corner, how far apart
/// the corners it makes are at the least, and how near the start's straight a
/// road is brought to join it: so much on the ground, or so much on the window
/// if that is more.
const SLACK: (f32, f32) = (4.0, 7.0);
const LEAST: (f32, f32) = (18.0, 22.0);
const SNAP: (f32, f32) = (11.0, 20.0);
/// How many seconds something said is said for, unless it is asked.
const SAID_FOR: f32 = 6.0;
/// How far to the left of the middle of the screen the view looks, for each unit
/// it is from what it looks at, so that the map is in the middle of what the
/// panel down the left leaves.
const BESIDE: f32 = 0.2;

/// A race being run to try a map out: what the settings were before it.
#[derive(Resource)]
pub struct Trial {
    circuit: usize,
    time_race: bool,
    championship: Option<String>,
}

/// One of the rows the builder opens on.
#[derive(Clone, Copy, PartialEq)]
enum Shelf {
    /// A new circuit, or a new arena.
    New(bool),
    /// One of the maps there are, by its place among the circuits.
    Old(usize),
}

/// Where the view looks, and from where.
struct Eye {
    focus: Vec3,
    yaw: f32,
    pitch: f32,
    far: f32,
    /// Somewhere it is on its way to looking at.
    goal: Option<Vec3>,
}

/// What the pointer has hold of, with its left button down.
enum Hold {
    Nothing,
    /// A handle of the thing picked: which of them.
    Handle(usize),
    /// The ground, to slide the view: the place on it that was taken hold of.
    Ground(Vec3),
    /// The end of a road being drawn: the line drawn from it so far, how many
    /// corners the road had when the line was begun, and whether the line has
    /// been laid as road yet.
    Road(Stroke, usize, bool),
}

#[derive(Resource)]
pub struct Builder {
    draft: Option<Draft>,
    /// A map is being changed, and not only looked at on the list.
    editing: bool,
    row: usize,
    /// What the list's map was made from, so that it is made once.
    listed: Option<Shelf>,
    /// The words being typed into a row.
    typing: Option<String>,
    /// What was last said, whether it is something wrong, and when.
    said: (String, bool),
    said_at: std::time::Instant,
    /// Asked once whether to leave what hasn't been kept, or to delete a map.
    sure: bool,
    eye: Eye,
    hold: Hold,
    /// Where the pointer was when its button went down, and whether it has gone
    /// far enough from there since to be dragging.
    pressed: Vec2,
    dragged: bool,
    /// The right button is turning the view.
    turning: bool,
    /// Something just added is following the pointer to where it is to be put.
    carrying: bool,
    /// The ring the pointer is over: whose it is, and which of that thing's.
    over: Option<(Layer, usize, usize)>,
    /// The square between two corners the pointer is over: which it comes after.
    between: Option<usize>,
    /// Where a road being drawn would go on to, which is where the pointer is on
    /// the ground, and whether it joins the start there.
    live: Option<[f32; 2]>,
    joining: bool,
    /// Which draft is built, which state of it, and whether as a bare road.
    built: Option<(u32, u32, bool)>,
    /// The bare road that is drawn: of which draft, in which state, and whether
    /// as joined to its start.
    sketched: Option<(u32, u32, bool)>,
    drafts: u32,
    /// The key held down, and when it next repeats.
    held: Option<(KeyCode, f32)>,
    /// The keys are said along the bottom.
    help: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Builder {
            draft: None,
            editing: false,
            row: 0,
            listed: None,
            typing: None,
            said: default(),
            said_at: std::time::Instant::now(),
            sure: false,
            eye: Eye {
                focus: Vec3::ZERO,
                yaw: 0.0,
                pitch: 1.1,
                far: 300.0,
                goal: None,
            },
            hold: Hold::Nothing,
            pressed: Vec2::ZERO,
            dragged: false,
            turning: false,
            carrying: false,
            over: None,
            between: None,
            live: None,
            joining: false,
            built: None,
            sketched: None,
            drafts: 0,
            held: None,
            help: false,
        }
    }
}

/// What is built of the map, which goes when it is built again, and the bare
/// road drawn of one that is not built.
#[derive(Component)]
struct Built;

#[derive(Component)]
struct Sketch;

/// The number beside one of the things marked.
#[derive(Component)]
struct Label(usize);

pub fn plugin(app: &mut App) {
    if let Some(script) = Script::from_env() {
        app.insert_resource(script);
    }
    app.init_resource::<Builder>()
        .init_resource::<Pointer>()
        .init_resource::<Board>()
        .init_resource::<Asked>()
        .add_systems(OnEnter(Screen::Build), (enter, panel::open))
        .add_systems(OnExit(Screen::Build), leave)
        .add_systems(Last, come_back.run_if(resource_exists::<Trial>))
        .add_systems(
            Update,
            (
                pointer::read,
                panel::pressed,
                keys,
                ground,
                act,
                show,
                sketch,
                look,
                mark,
                say,
            )
                .chain()
                .run_if(in_state(Screen::Build)),
        );
}

/// A race that was trying a map out goes back to the builder, and not to the
/// menu, whatever ended it.
fn come_back(screen: Res<State<Screen>>, mut next: ResMut<NextState<Screen>>) {
    if *screen.get() != Screen::Build && matches!(*next, NextState::Pending(Screen::Menu)) {
        next.set(Screen::Build);
    }
}

fn enter(
    mut commands: Commands,
    mut builder: ResMut<Builder>,
    trial: Option<Res<Trial>>,
    mut circuits: ResMut<Circuits>,
    mut settings: ResMut<Settings>,
    mut gizmos: ResMut<GizmoConfigStore>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    crate::scenery::set_mirror(false);
    commands.insert_resource(ClearColor(SKY));
    // What is marked on the map is seen through whatever is in front of it.
    let (config, _) = gizmos.config_mut::<DefaultGizmoConfigGroup>();
    (config.depth_bias, config.line.width) = (-1.0, 2.5);
    match trial {
        // Back from trying a map out: the race it was tried as is gone, and the
        // settings are as they were.
        Some(trial) => {
            circuits.0.retain(|circuit| circuit.key() != TRIAL);
            settings.circuit = trial.circuit.min(circuits.0.len().saturating_sub(1));
            (settings.time_race, settings.championship) =
                (trial.time_race, trial.championship.clone());
            commands.remove_resource::<Trial>();
            (builder.built, builder.sketched, builder.hold) = (None, None, Hold::Nothing);
        }
        None => {
            *builder = Builder {
                drafts: builder.drafts,
                ..default()
            }
        }
    }
    // `BRICK_BUILD=FIGURE8` opens a demo on that map being changed, and
    // `BRICK_BUILD=circuit` or `arena` on a new one; anything else on the list.
    if builder.draft.is_none()
        && let Ok(wanted) = std::env::var("BRICK_BUILD")
    {
        let named = |circuit: &crate::menu::Circuit| {
            circuit.map.is_some() && circuit.key().eq_ignore_ascii_case(&wanted)
        };
        let on = match (wanted.as_str(), circuits.0.iter().position(named)) {
            ("circuit", _) => Some(Shelf::New(false)),
            ("arena", _) => Some(Shelf::New(true)),
            (_, Some(index)) => Some(Shelf::Old(index)),
            _ => None,
        };
        if let Some(on) = on {
            builder.row = shelf(&circuits)
                .iter()
                .position(|row| *row == on)
                .unwrap_or(0);
            builder.listed = Some(on);
            builder.open(on.draft(&circuits));
            builder.editing = true;
        }
    }
    commands.spawn((
        DespawnOnExit(Screen::Build),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(6000.0, 6000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: GREEN,
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.08, 0.0),
    ));
    for n in 0..LABELS {
        commands.spawn((
            DespawnOnExit(Screen::Build),
            Label(n),
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextShadow {
                offset: Vec2::splat(1.0),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(-1),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
        ));
    }
}

fn leave(mut scale: ResMut<UiScale>, mut gizmos: ResMut<GizmoConfigStore>) {
    scale.0 = 1.0;
    let (config, _) = gizmos.config_mut::<DefaultGizmoConfigGroup>();
    (config.depth_bias, config.line.width) = (0.0, 2.0);
}

/// The rows the builder opens on: the two new ones, then every map there is.
fn shelf(circuits: &Circuits) -> Vec<Shelf> {
    let maps = circuits
        .0
        .iter()
        .enumerate()
        .filter(|(_, circuit)| circuit.map.is_some());
    [Shelf::New(false), Shelf::New(true)]
        .into_iter()
        .chain(maps.map(|(index, _)| Shelf::Old(index)))
        .collect()
}

impl Shelf {
    /// The map of a row, to be changed: one of the game's own as another like it.
    fn draft(self, circuits: &Circuits) -> Draft {
        match self {
            Shelf::New(arena) => Draft::new(arena),
            Shelf::Old(index) => {
                let map = circuits.0[index].map();
                Draft::of(map, crate::map::carried(&map.key))
            }
        }
    }

    /// The file of a row's map, if it is one of the player's own, to be deleted.
    fn own(self, circuits: &Circuits) -> Option<std::path::PathBuf> {
        match self {
            Shelf::Old(index) => {
                let circuit = &circuits.0[index];
                circuit
                    .map()
                    .file
                    .clone()
                    .filter(|_| !crate::map::carried(circuit.key()))
            }
            Shelf::New(_) => None,
        }
    }
}

impl Builder {
    fn say(&mut self, what: impl Into<String>, wrong: bool) {
        (self.said, self.said_at) = ((what.into(), wrong), std::time::Instant::now());
    }

    fn open(&mut self, draft: Draft) {
        self.draft = Some(draft);
        self.drafts += 1;
        (self.built, self.sketched, self.hold) = (None, None, Hold::Nothing);
        (self.carrying, self.live, self.joining) = (false, None, false);
        self.whole();
    }

    /// Looks at the whole of the map from over its south side: or, of a road
    /// that is hardly begun, at the ground there is to draw it on.
    fn whole(&mut self) {
        let Some(draft) = &self.draft else { return };
        let places: Vec<Vec3> = match &draft.map.plan {
            Some(plan) => plan
                .outline
                .iter()
                .map(|&[x, z]| Vec3::new(x, 0.0, z))
                .collect(),
            None => draft
                .map
                .road
                .corners
                .iter()
                .map(|&[x, y, z, _]| Vec3::new(x, y, z))
                .collect(),
        };
        let ends = (Vec3::MAX, Vec3::MIN);
        let (least, most) = places
            .iter()
            .fold(ends, |(least, most), &at| (least.min(at), most.max(at)));
        let size = (most - least).with_y(0.0);
        // Far enough off for it to be seen whole beside the panel down the left,
        // and for there to be ground to draw a road on that is hardly begun.
        let across = (size.x * 1.35)
            .max(size.z * 1.75)
            .max(if draft.open { 420.0 } else { 80.0 });
        let far = across.clamp(FAR.0, FAR.1);
        self.eye = Eye {
            focus: ((least + most) / 2.0).with_y(0.0) - Vec3::X * far * BESIDE,
            yaw: 0.0,
            pitch: 1.1,
            far,
            goal: None,
        };
    }

    /// Has the view go to the thing picked.
    fn seek(&mut self) {
        let at = self
            .draft
            .as_ref()
            .and_then(|draft| draft.handles(draft.layer, draft.picked).first().copied());
        self.eye.goal = at.or(self.eye.goal);
    }

    /// How much of the ground a unit of the window is, about, where the view is
    /// looking.
    fn scale(&self, window: &Window) -> f32 {
        self.eye.far * 2.0 * FRAC_PI_8.tan() / window.height().max(1.0)
    }
}

/// Whether a key has just gone down, or has been held long enough to count
/// again.
fn tapped(
    held: &mut Option<(KeyCode, f32)>,
    keys: &ButtonInput<KeyCode>,
    now: f32,
    key: KeyCode,
) -> bool {
    if keys.just_pressed(key) {
        *held = Some((key, now + REPEAT.0));
        return true;
    }
    match *held {
        Some((was, at)) if was == key && keys.pressed(key) && now >= at => {
            *held = Some((key, now + REPEAT.1));
            true
        }
        _ => false,
    }
}

/// What the keys do: the same as the panels and the pointer, by another way.
fn keys(
    mut builder: ResMut<Builder>,
    mut asked: ResMut<Asked>,
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<KeyboardInput>,
    time: Res<Time<Real>>,
    circuits: Res<Circuits>,
    camera: Single<&Transform, (With<Camera3d>, Without<RenderLayers>)>,
) {
    let builder = &mut *builder;
    let now = time.elapsed_secs();
    // Words being typed take every key until they are done with.
    if let Some(words) = &mut builder.typing {
        for event in typed.read().filter(|event| event.state.is_pressed()) {
            match &event.logical_key {
                Key::Backspace => drop(words.pop()),
                Key::Space if words.len() < lobby_api::MAX_NAME => words.push(' '),
                Key::Character(letters) => {
                    let fit = letters.chars().filter(|c| !c.is_control());
                    words.extend(fit.take(lobby_api::MAX_NAME.saturating_sub(words.len())));
                }
                _ => {}
            }
        }
        if keys.just_pressed(KeyCode::Enter) {
            let words = builder.typing.take().unwrap_or_default();
            if let Some(draft) = &mut builder.draft {
                draft.type_into(draft.row, words.trim().to_string());
            }
        } else if keys.just_pressed(KeyCode::Escape) {
            builder.typing = None;
        }
        return;
    }
    typed.clear();
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let command = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    if !builder.editing {
        let rows = shelf(&circuits).len();
        if tapped(&mut builder.held, &keys, now, KeyCode::ArrowDown) {
            asked.0.push(Act::Shelf((builder.row + 1) % rows));
        }
        if tapped(&mut builder.held, &keys, now, KeyCode::ArrowUp) {
            asked.0.push(Act::Shelf((builder.row + rows - 1) % rows));
        }
        for (key, act) in [
            (KeyCode::Enter, Act::Open),
            (KeyCode::Backspace, Act::Delete),
            (KeyCode::Delete, Act::Delete),
            (KeyCode::Escape, Act::Leave),
        ] {
            if keys.just_pressed(key) {
                asked.0.push(act);
            }
        }
        return;
    }
    if command {
        if keys.just_pressed(KeyCode::KeyS) {
            asked.0.push(Act::Keep);
        }
        if keys.just_pressed(KeyCode::KeyZ) {
            asked.0.push(if shift { Act::Redo } else { Act::Undo });
        }
        return;
    }
    for (key, act) in [
        (KeyCode::F5, Act::Keep),
        (KeyCode::KeyT, Act::Try),
        (KeyCode::KeyU, Act::Undo),
        (KeyCode::KeyY, Act::Redo),
        (KeyCode::KeyC, Act::Whole),
        (KeyCode::KeyO, Act::Loop),
        (KeyCode::KeyH, Act::Help),
        (KeyCode::Escape, Act::Leave),
    ] {
        if keys.just_pressed(key) {
            asked.0.push(act);
        }
    }
    let Some(draft) = &mut builder.draft else {
        return;
    };
    let rows = draft.rows();
    let mut sought = false;
    if keys.just_pressed(KeyCode::Tab) {
        draft.step_layer(if shift { -1 } else { 1 });
        sought = true;
    }
    for (key, step) in [(KeyCode::Period, 1), (KeyCode::Comma, -1)] {
        if tapped(&mut builder.held, &keys, now, key) {
            draft.step_item(step);
            sought = true;
        }
    }
    for (key, step) in [(KeyCode::KeyS, 1), (KeyCode::KeyW, -1)] {
        if tapped(&mut builder.held, &keys, now, key) && !rows.is_empty() {
            draft.row = (draft.row as i32 + step).rem_euclid(rows.len() as i32) as usize;
        }
    }
    draft.row = draft.row.min(rows.len().saturating_sub(1));
    for (key, way) in [(KeyCode::KeyD, 1.0), (KeyCode::KeyA, -1.0)] {
        if tapped(&mut builder.held, &keys, now, key) {
            asked.0.push(Act::Turn(draft.row, way));
        }
    }
    if keys.just_pressed(KeyCode::Enter) && rows.get(draft.row).is_some_and(|row| row.typed) {
        asked.0.push(Act::Type(draft.row));
    }
    // Over the ground the way the screen has it: right is whichever of the four
    // ways is most to the right as it is looked at.
    let square = |way: Vec3| match way.x.abs() > way.z.abs() {
        true => Vec3::X * way.x.signum(),
        false => Vec3::Z * way.z.signum(),
    };
    let (right, up) = (
        square(*camera.right()),
        square(camera.forward().with_y(0.0)),
    );
    let fine = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let by = if shift {
        5.0
    } else if fine {
        0.25
    } else {
        1.0
    };
    let mut pushed = Vec2::ZERO;
    for (key, way) in [
        (KeyCode::ArrowRight, Vec2::X),
        (KeyCode::ArrowLeft, Vec2::NEG_X),
        (KeyCode::ArrowUp, Vec2::Y),
        (KeyCode::ArrowDown, Vec2::NEG_Y),
    ] {
        if tapped(&mut builder.held, &keys, now, key) {
            pushed += way;
        }
    }
    if pushed != Vec2::ZERO {
        let over = (right * pushed.x + up * pushed.y) * by;
        draft.nudge(over.x, over.z, pushed.x * by, pushed.y * by);
    }
    if keys.just_pressed(KeyCode::KeyN) {
        let had = draft.count(draft.layer);
        draft.add(builder.eye.focus);
        let said = match (draft.count(draft.layer) > had, draft.layer) {
            (true, _) => "",
            (false, Layer::Hazards) => "It has one of every hazard there is.",
            (false, Layer::Grid) => "A grid is six places: move them.",
            (false, Layer::Corners) if draft.open => "The road ends here: draw it on from its end.",
            (false, _) => "There is no more of that to add.",
        };
        (builder.said, builder.said_at) = ((said.into(), false), std::time::Instant::now());
        sought = true;
    }
    if keys.just_pressed(KeyCode::Backspace) || keys.just_pressed(KeyCode::Delete) {
        draft.remove();
    }
    // The view: round, up and down, and nearer and further.
    let dt = time.delta_secs();
    let held = |key: KeyCode| if keys.pressed(key) { 1.0 } else { 0.0 };
    builder.eye.yaw += (held(KeyCode::KeyE) - held(KeyCode::KeyQ)) * dt * 1.6;
    builder.eye.pitch += (held(KeyCode::KeyR) - held(KeyCode::KeyF)) * dt;
    builder.eye.far *= 1.0 + (held(KeyCode::KeyX) - held(KeyCode::KeyZ)) * dt * 1.5;
    if sought {
        builder.seek();
    }
}

/// Where a ray from the eye comes to the ground.
fn floor(ray: Ray3d) -> Option<Vec3> {
    let far = -ray.origin.y / ray.direction.y;
    (ray.direction.y < -1e-4 && far < 20000.0).then(|| ray.origin + *ray.direction * far)
}

/// What the pointer does on the map: drawing a road, moving what is marked, and
/// moving the view.
fn ground(
    mut builder: ResMut<Builder>,
    pointer: Res<Pointer>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), (With<Camera3d>, Without<RenderLayers>)>,
) {
    let builder = &mut *builder;
    let (camera, eye) = *camera;
    (builder.over, builder.between) = (None, None);
    if !builder.editing || builder.typing.is_some() {
        (builder.hold, builder.live, builder.joining) = (Hold::Nothing, None, false);
        return;
    }
    let Some(at) = pointer.at else {
        if !pointer.left.down {
            builder.hold = Hold::Nothing;
        }
        return;
    };
    let ray = camera.viewport_to_world(eye, at).ok();
    let under = ray.and_then(floor);
    let unit = builder.scale(&window);
    // The view: the wheel comes nearer to what the pointer is over, the right
    // button turns it and the middle one slides it.
    if pointer.wheel != 0.0 && !pointer.taken {
        let far =
            (builder.eye.far * (1.0 - pointer.wheel * 0.1).clamp(0.5, 1.5)).clamp(FAR.0, FAR.1);
        if let Some(under) = under {
            let part = far / builder.eye.far;
            builder.eye.focus = under - (under - builder.eye.focus) * part;
        }
        (builder.eye.far, builder.eye.goal) = (far, None);
    }
    if pointer.right.pressed && !pointer.taken && !builder.carrying {
        builder.turning = true;
    }
    if !pointer.right.down {
        builder.turning = false;
    }
    if builder.turning {
        builder.eye.yaw -= pointer.moved.x * 0.006;
        builder.eye.pitch += pointer.moved.y * 0.006;
    }
    if pointer.middle.down && !pointer.taken {
        let (right, on) = (
            eye.right().with_y(0.0),
            eye.forward().with_y(0.0).normalize_or_zero(),
        );
        builder.eye.focus += (on * pointer.moved.y - right * pointer.moved.x) * unit;
        builder.eye.goal = None;
    }
    let Some(draft) = &mut builder.draft else {
        return;
    };
    let seen = |place: Vec3| {
        camera
            .world_to_viewport(eye, place)
            .ok()
            .map(|seen| seen.distance(at))
    };
    // Something just added goes where the pointer does until it is put down.
    if builder.carrying {
        if let (Some(ray), false) = (ray, pointer.taken) {
            draft.put(0, ray.origin, *ray.direction);
            if pointer.left.pressed {
                builder.carrying = false;
                draft.seal();
            }
        }
        return;
    }
    // What the pointer is over: the nearest ring, one of the kind picked before
    // any other, and failing a ring one of the squares between corners.
    if matches!(builder.hold, Hold::Nothing) && !pointer.taken {
        let mut nearest: Option<(f32, Layer, usize, usize)> = None;
        for &layer in draft.layers() {
            let own = layer == draft.layer;
            for index in 0..draft.count(layer) {
                let picked = own && index == draft.picked;
                for (part, handle) in draft.handles(layer, index).into_iter().enumerate() {
                    // Only the thing picked is taken by its ends or its sides.
                    let Some(off) = seen(handle).filter(|_| part == 0 || picked) else {
                        continue;
                    };
                    let off = off + if own { 0.0 } else { HOLD * 0.4 };
                    if off < HOLD && nearest.is_none_or(|nearest| off < nearest.0) {
                        nearest = Some((off, layer, index, part));
                    }
                }
            }
        }
        builder.over = nearest.map(|(_, layer, index, part)| (layer, index, part));
        if builder.over.is_none() {
            let near =
                |mid: &(usize, Vec3)| Some((seen(mid.1).filter(|off| *off < HOLD * 0.8)?, mid.0));
            let nearest = draft
                .mids()
                .iter()
                .filter_map(near)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            builder.between = nearest.map(|mid| mid.1);
        }
    }
    let on_ground = |under: Vec3| Vec2::new(under.x, under.z);
    let (slack, least, snap) = (
        SLACK.0.max(SLACK.1 * unit),
        LEAST.0.max(LEAST.1 * unit),
        SNAP.0.max(SNAP.1 * unit),
    );
    let end = draft.ends().map(|(end, _)| end);
    let last = draft.map.road.corners.len().saturating_sub(1);
    if pointer.left.pressed && !pointer.taken {
        (builder.pressed, builder.dragged) = (at, false);
        builder.hold = match (builder.over, builder.between) {
            (Some((layer, index, part)), _) => {
                draft.pick(layer, index);
                builder.eye.goal = None;
                // The end of a road being drawn is dragged to draw it on.
                match end.filter(|_| layer == Layer::Corners && index + 1 == last) {
                    Some(end) => Hold::Road(Stroke::begun(on_ground(end)), last + 1, false),
                    None => Hold::Handle(part),
                }
            }
            (None, Some(after)) => {
                draft.split(after);
                Hold::Handle(0)
            }
            (None, None) => under.map_or(Hold::Nothing, Hold::Ground),
        };
    }
    if pointer.left.down && at.distance(builder.pressed) > CLICK {
        builder.dragged = true;
    }
    (builder.live, builder.joining) = (None, false);
    match &mut builder.hold {
        Hold::Handle(part) if pointer.left.down && builder.dragged => {
            if let Some(ray) = ray {
                draft.put(*part, ray.origin, *ray.direction);
            }
        }
        Hold::Ground(held) if pointer.left.down && builder.dragged => {
            if let Some(under) = under {
                builder.eye.focus += *held - under;
                builder.eye.goal = None;
            }
        }
        // The road is laid along the line as far as the pointer has taken it,
        // or as far as the line's last corner while the pointer is where the
        // road joins its start.
        Hold::Road(stroke, had, laid) if pointer.left.down => {
            if let Some(under) = under.filter(|_| builder.dragged) {
                let place = on_ground(under);
                stroke.drawn(place, slack, least);
                builder.joining = draft.joins(place, snap);
                builder.live = Some([under.x, under.z]);
                let tail = draft.ends().map(|(_, tail)| on_ground(tail));
                let corners = match builder.joining {
                    true => stroke.corners(None, tail, slack, least),
                    false => stroke.corners(Some(place), None, slack, least),
                };
                if *laid || !corners.is_empty() {
                    draft.trace(*had, &corners, !*laid);
                    *laid = true;
                }
            }
        }
        _ => {}
    }
    if pointer.left.released {
        let place = under.map(on_ground);
        let joins = place.is_some_and(|place| draft.joins(place, snap));
        let mut closed = false;
        match std::mem::replace(&mut builder.hold, Hold::Nothing) {
            Hold::Handle(_) => {
                // The back of the start's straight, clicked and not dragged, is
                // where a road being drawn is joined.
                let tail = draft.open && draft.layer == Layer::Corners && draft.picked == last;
                closed = tail && !builder.dragged && draft.close();
                draft.seal();
            }
            // A click on the ground lays a road being drawn on to there.
            Hold::Ground(_) if !builder.dragged && draft.open && draft.layer == Layer::Corners => {
                match (joins, place) {
                    (true, _) => closed = draft.close(),
                    (false, Some(place)) => draft.draw([place.x, place.y]),
                    (false, None) => {}
                }
            }
            // The line is done: the road is as it was last laid along it, and
            // joined to its start if that is where the pointer let go.
            Hold::Road(..) if builder.dragged && joins => closed = draft.close(),
            _ => {}
        }
        if closed {
            let said = match (&draft.wrong, draft.lap()) {
                (None, Some(lap)) => format!(
                    "That is a lap, {lap:.0} round. Drag its corners to change it, and try it with T."
                ),
                _ => String::new(),
            };
            (builder.said, builder.said_at) = ((said, false), std::time::Instant::now());
        }
    }
    // Where a click would lay the road on to, while one is being drawn.
    let drawing = matches!(builder.hold, Hold::Nothing)
        && !pointer.taken
        && draft.open
        && draft.layer == Layer::Corners;
    if let (true, Some(under), None, None) = (drawing, under, builder.over, builder.between) {
        builder.live = Some([under.x, under.z]);
        builder.joining = draft.joins(on_ground(under), snap);
    }
}

/// Keeps a map as a file of the folder, and has the game know it: what the file
/// is, or why there is none.
fn keep(draft: &mut Draft, circuits: &mut Circuits) -> Result<String, String> {
    let text = match (draft.open, &draft.wrong, draft.sound()) {
        (true, ..) => {
            return Err("A circuit is a lap: bring the road back to its start first.".into());
        }
        (_, Some(wrong), _) => return Err(format!("It can't be kept as it is: {wrong}")),
        (_, _, Some(map)) => map.text.clone(),
        _ => return Err("There is nothing of it to keep yet.".into()),
    };
    let key = draft.file_key(|key| {
        circuits
            .0
            .iter()
            .any(|circuit| circuit.key().eq_ignore_ascii_case(key))
    });
    let file = crate::map::save(&key, &text)?;
    let map = Map::tried(&key, &text)?;
    circuits.keep(Map {
        file: Some(file.clone()),
        ..map
    });
    draft.kept(key);
    Ok(file.display().to_string())
}

/// Does what the panels and the keys have asked for.
fn act(
    mut builder: ResMut<Builder>,
    mut asked: ResMut<Asked>,
    keys: Res<ButtonInput<KeyCode>>,
    mut circuits: ResMut<Circuits>,
    mut settings: ResMut<Settings>,
    mut next: ResMut<NextState<Screen>>,
    mut commands: Commands,
) {
    let builder = &mut *builder;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let fine = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let by = if shift {
        5.0
    } else if fine {
        0.25
    } else {
        1.0
    };
    for act in std::mem::take(&mut asked.0) {
        // Anything but the answer to what was asked is no answer.
        let sure = std::mem::take(&mut builder.sure);
        if !builder.editing {
            let rows = shelf(&circuits);
            builder.row = builder.row.min(rows.len() - 1);
            let on = rows[builder.row];
            match act {
                Act::Shelf(row) if row == builder.row => asked.0.push(Act::Open),
                Act::Shelf(row) => {
                    builder.row = row.min(rows.len() - 1);
                    builder.say("", false);
                }
                Act::Open if builder.draft.is_some() => {
                    builder.editing = true;
                    let said = match on {
                        Shelf::Old(index) if crate::map::carried(circuits.0[index].key()) => {
                            "One of the game's own: this is another like it, kept as a map of yours."
                        }
                        _ => "",
                    };
                    builder.say(said, false);
                }
                Act::Delete => match (on.own(&circuits), sure, on) {
                    (Some(file), true, Shelf::Old(index)) => match std::fs::remove_file(&file) {
                        Ok(()) => {
                            builder.say(format!("Deleted {}.", file.display()), false);
                            circuits.0.remove(index);
                            if settings.circuit >= index && settings.circuit > 0 {
                                settings.circuit -= 1;
                            }
                            builder.listed = None;
                            builder.row = builder.row.saturating_sub(1);
                        }
                        Err(error) => builder.say(
                            format!("Could not delete {}: {error}", file.display()),
                            true,
                        ),
                    },
                    (Some(_), ..) => {
                        builder.sure = true;
                        builder.say(
                            "Delete this map? Delete again takes its file away for good.",
                            true,
                        );
                    }
                    (None, ..) => builder.say("Only a map of your own can be deleted.", true),
                },
                Act::Leave => next.set(Screen::Menu),
                _ => {}
            }
            continue;
        }
        let Some(draft) = &mut builder.draft else {
            continue;
        };
        match act {
            Act::Kind(layer) if draft.layers().contains(&layer) => {
                if layer != draft.layer {
                    draft.pick(layer, 0);
                }
                builder.carrying = false;
            }
            Act::Add(layer) if draft.layers().contains(&layer) => {
                if layer != draft.layer {
                    draft.pick(layer, 0);
                }
                let had = draft.count(layer);
                draft.add(builder.eye.focus);
                builder.carrying = draft.count(layer) > had;
                let said = match (builder.carrying, layer) {
                    (true, _) => "",
                    (false, Layer::Hazards) => "It has one of every hazard there is.",
                    (false, _) => "There is no more of that to add.",
                };
                builder.say(said, false);
            }
            Act::Row(row) => draft.row = row,
            Act::Turn(row, way) => {
                draft.row = row;
                if draft.rows().get(row).is_some_and(|row| !row.typed) {
                    draft.turn(row, way * by);
                }
            }
            Act::Type(row) => {
                draft.row = row;
                builder.typing = draft
                    .rows()
                    .get(row)
                    .filter(|row| row.typed)
                    .map(|row| row.value.clone());
            }
            Act::Undo | Act::Redo => {
                let done = if act == Act::Undo {
                    draft.undo()
                } else {
                    draft.redo()
                };
                builder.carrying = false;
                builder.say(
                    if done {
                        ""
                    } else {
                        "There is nothing to take back."
                    },
                    false,
                );
            }
            Act::Whole => builder.whole(),
            Act::Loop => match draft.open {
                true => {
                    let said = match draft.close() {
                        true => "",
                        false => "There is no road yet to join to the start: draw some.",
                    };
                    builder.say(said, false);
                }
                false if !draft.arena() => {
                    draft.reopen();
                    builder.say("The road is open again where it came back to the start: draw it on from its end.", false);
                }
                false => {}
            },
            Act::Help => builder.help = !builder.help,
            Act::Keep => match keep(draft, &mut circuits) {
                Ok(file) => builder.say(format!("Kept as {file}."), false),
                Err(wrong) => builder.say(wrong, true),
            },
            Act::Try => {
                let tried = match (draft.open, draft.sound()) {
                    (true, _) => Err(
                        "a circuit is a lap, so bring the road back to its start first."
                            .to_string(),
                    ),
                    (_, None) => Err(draft.wrong.clone().unwrap_or_default()),
                    (_, Some(map)) => circuits.try_out(&map.text),
                };
                match tried {
                    Ok(circuit) => {
                        commands.insert_resource(Trial {
                            circuit: settings.circuit,
                            time_race: settings.time_race,
                            championship: settings.championship.take(),
                        });
                        (settings.circuit, settings.time_race) = (circuit, false);
                        next.set(Screen::Loading);
                    }
                    Err(wrong) => builder.say(format!("It can't be tried: {wrong}"), true),
                }
            }
            // Leaving: what is being carried first, then the map, if that is
            // kept or it has been asked.
            Act::Leave if builder.carrying => {
                draft.remove();
                builder.carrying = false;
            }
            Act::Leave if draft.unsaved && !sure => {
                builder.sure = true;
                builder.say("It has changes that are not kept. Leave again to lose them, or keep them first.", true);
            }
            Act::Leave => {
                (builder.editing, builder.listed, builder.hold) = (false, None, Hold::Nothing);
                builder.say("", false);
            }
            _ => {}
        }
    }
    // The map of the row the list is on is shown behind it.
    if !builder.editing {
        let rows = shelf(&circuits);
        builder.row = builder.row.min(rows.len() - 1);
        let on = rows[builder.row];
        if builder.listed != Some(on) {
            builder.listed = Some(on);
            builder.open(on.draft(&circuits));
        }
    }
}

/// Builds what is drawn of the map again, when it has changed: nothing of one
/// that is not a map as it stands, which is drawn as a bare road instead.
fn show(
    mut commands: Commands,
    mut builder: ResMut<Builder>,
    built: Query<Entity, With<Built>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let builder = &mut *builder;
    let Some(draft) = &builder.draft else { return };
    let state = (builder.drafts, draft.version, draft.sound().is_none());
    if builder.built == Some(state) {
        return;
    }
    for entity in &built {
        commands.entity(entity).despawn();
    }
    builder.built = Some(state);
    let Some(map) = draft.sound() else { return };
    let track = Track::made(map, false);
    let plastic = materials.add(StandardMaterial {
        perceptual_roughness: 0.5,
        ..default()
    });
    let mut marks = BrickMesh::default();
    // The bricks the game would lay, each its colour.
    for (power, at) in crate::items::laid(&track) {
        let colour = match power {
            None => WHITE,
            Some(Power::Red) => RED,
            Some(Power::Yellow) => YELLOW,
            Some(Power::Blue) => BLUE,
            Some(Power::Green) => GREEN,
        };
        marks.brick(
            at + Vec3::Y * 0.6,
            Vec3::new(0.7, 0.45, 0.7),
            Quat::IDENTITY,
            colour,
            (2, 2),
        );
    }
    // An arena's grid: a block where each car starts, and a smaller one the way
    // it faces.
    for &(at, facing) in &track.course.grid {
        marks.cuboid(
            at + Vec3::Y * 0.7,
            Vec3::new(1.4, 0.7, 1.4),
            Quat::IDENTITY,
            ORANGE,
        );
        marks.cuboid(
            at + facing * 2.2 + Vec3::Y * 0.5,
            Vec3::splat(0.5),
            Quat::IDENTITY,
            WHITE,
        );
    }
    // And a post where each hazard stands.
    for index in 0..draft.count(Layer::Hazards) {
        for (part, at) in draft.handles(Layer::Hazards, index).into_iter().enumerate() {
            let (high, colour) = if part == 0 { (5.0, RED) } else { (2.0, ORANGE) };
            marks.cuboid(
                at + Vec3::Y * high,
                Vec3::new(0.5, high, 0.5),
                Quat::IDENTITY,
                colour,
            );
            marks.cuboid(
                at + Vec3::Y * (high * 2.0 + 0.8),
                Vec3::splat(1.1),
                Quat::IDENTITY,
                colour,
            );
        }
    }
    let scenery = track.build_scenery(map, &mut Rng(0x1EC0_1999));
    for mesh in [track.build_mesh(), scenery, marks.build()] {
        if mesh.count_vertices() > 0 {
            commands.spawn((
                Built,
                DespawnOnExit(Screen::Build),
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(plastic.clone()),
            ));
        }
    }
}

/// The colours of a bare road, which are those of one that is built: the two its
/// tarmac goes by turns, and the two its kerbs do.
const TARMAC: [Color; 2] = [Color::srgb(0.25, 0.26, 0.28), Color::srgb(0.28, 0.29, 0.31)];
const KERBS: [Color; 2] = [RED, WHITE];

/// A bare road along `middle`, each place of it with how far its barriers would
/// be from it, flat and without barriers: tarmac between kerbs, with a broken
/// line down it.
fn road(middle: &[(Vec3, f32)], mesh: &mut BrickMesh) {
    let n = middle.len();
    if n < 2 {
        return;
    }
    let lift = Vec3::Y * 0.06;
    let across: Vec<Vec3> = (0..n)
        .map(|i| {
            let along = (middle[(i + 1).min(n - 1)].0 - middle[i.saturating_sub(1)].0)
                .with_y(0.0)
                .normalize_or_zero();
            Vec3::new(-along.z, 0.0, along.x)
        })
        .collect();
    // So far across at each place: from its middle, or from its barriers.
    let edge =
        |i: usize, right: &dyn Fn(f32) -> f32| middle[i].0 + across[i] * right(middle[i].1) + lift;
    for i in 0..n - 1 {
        let mut strip =
            |from: &dyn Fn(f32) -> f32, to: &dyn Fn(f32) -> f32, colour: Color, lift: Vec3| {
                mesh.quad(
                    edge(i, from) + lift,
                    edge(i, to) + lift,
                    edge(i + 1, to) + lift,
                    edge(i + 1, from) + lift,
                    colour,
                );
            };
        let kerbed = 0.4 - VERGE;
        strip(
            &|h| -h - kerbed,
            &|h| h + kerbed,
            TARMAC[(i / 2) % 2],
            Vec3::ZERO,
        );
        let kerb = KERBS[(i / 2) % 2];
        strip(&|h| -h, &|h| -h - kerbed, kerb, Vec3::ZERO);
        strip(&|h| h + kerbed, &|h| h, kerb, Vec3::ZERO);
        if (i / 2) % 2 == 0 {
            strip(&|_| -0.22, &|_| 0.22, WHITE, Vec3::Y * 0.02);
        }
    }
}

/// Draws the bare road of a draft that is not a map as it stands, as far as it
/// has been drawn, with the line its cars start from and the places they line up
/// behind it.
fn sketch(
    mut commands: Commands,
    mut builder: ResMut<Builder>,
    sketched: Query<Entity, With<Sketch>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let builder = &mut *builder;
    let Some(draft) = builder
        .draft
        .as_ref()
        .filter(|draft| !draft.arena() && draft.sound().is_none())
    else {
        if builder.sketched.take().is_some() {
            sketched
                .iter()
                .for_each(|entity| commands.entity(entity).despawn());
        }
        return;
    };
    // A road whose end is being dragged to where it joins its start is drawn
    // as joined.
    let joined = matches!(builder.hold, Hold::Road(..)) && builder.joining;
    let state = (builder.drafts, draft.edition, joined);
    if builder.sketched == Some(state) {
        return;
    }
    builder.sketched = Some(state);
    sketched
        .iter()
        .for_each(|entity| commands.entity(entity).despawn());
    let mut mesh = BrickMesh::default();
    road(&draft.laid(joined), &mut mesh);
    // The line, across the tarmac however wide it is there, and the six places
    // behind it.
    let corners = &draft.map.road.corners;
    if let (Some(start), Some(front), Some(tail)) =
        (corners.first(), corners.get(1), corners.last())
    {
        let at = |corner: &[f32; 4]| Vec3::new(corner[0], corner[1], corner[2]);
        let (start, along) = (
            at(start),
            (at(front) - at(tail)).with_y(0.0).normalize_or_zero(),
        );
        let (right, turned) = (
            Vec3::new(-along.z, 0.0, along.x),
            Quat::from_rotation_arc(Vec3::X, along),
        );
        let tarmac = draft.map.road.widths.first().copied().unwrap_or(WIDTH) / 2.0 - VERGE;
        for square in 0..12 {
            let colour = if square % 2 == 0 { WHITE } else { BLACK };
            let across = right * (square as f32 - 5.5) * (tarmac * 2.0 / 12.0);
            mesh.cuboid(
                start + across + Vec3::Y * 0.12,
                Vec3::new(0.7, 0.03, tarmac / 12.0),
                turned,
                colour,
            );
        }
        for place in 0..6 {
            let back = 8.0 + (place / 2) as f32 * 6.0;
            let side = if place % 2 == 0 { -2.25 } else { 2.25 };
            mesh.cuboid(
                start - along * back + right * side + Vec3::Y * 0.1,
                Vec3::new(1.6, 0.02, 1.0),
                turned,
                GREY,
            );
        }
    }
    let mesh = mesh.build();
    if mesh.count_vertices() > 0 {
        commands.spawn((
            Sketch,
            DespawnOnExit(Screen::Build),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                perceptual_roughness: 0.7,
                cull_mode: None,
                double_sided: true,
                ..default()
            })),
        ));
    }
}

fn look(
    mut builder: ResMut<Builder>,
    time: Res<Time<Real>>,
    camera: Single<(&mut Transform, &mut Projection), (With<Camera3d>, Without<RenderLayers>)>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut scale: ResMut<UiScale>,
) {
    let builder = &mut *builder;
    scale.0 = window.height() / SCREEN;
    let (mut transform, mut projection) = camera.into_inner();
    let eye = &mut builder.eye;
    // The list's map turns slowly behind it.
    if builder.draft.is_some() && !builder.editing {
        eye.yaw += time.delta_secs() * 0.15;
    }
    if let Some(goal) = eye.goal {
        eye.focus = eye.focus.lerp(goal, (time.delta_secs() * 8.0).min(1.0));
        if eye.focus.distance(goal) < 0.2 {
            eye.goal = None;
        }
    }
    eye.pitch = eye.pitch.clamp(TILT.0, TILT.1);
    eye.far = eye.far.clamp(FAR.0, FAR.1);
    eye.yaw = eye.yaw.rem_euclid(TAU);
    let back = Quat::from_euler(EulerRot::YXZ, eye.yaw, -eye.pitch, 0.0) * Vec3::Z;
    *transform =
        Transform::from_translation(eye.focus + back * eye.far).looking_at(eye.focus, Vec3::Y);
    *projection = Projection::Perspective(PerspectiveProjection {
        far: 8000.0,
        ..default()
    });
}

/// A ring flat on the ground, and a square.
fn ring(gizmos: &mut Gizmos, at: Vec3, radius: f32, colour: Color) {
    let flat = Isometry3d::new(at + Vec3::Y * 0.3, Quat::from_rotation_x(FRAC_PI_2));
    gizmos.circle(flat, radius, colour);
}

fn square(gizmos: &mut Gizmos, at: Vec3, half: f32, colour: Color) {
    let corners = [
        (-1.0, -1.0),
        (1.0, -1.0),
        (1.0, 1.0),
        (-1.0, 1.0),
        (-1.0, -1.0),
    ];
    gizmos.linestrip(
        corners.map(|(x, z)| at + Vec3::new(x * half, 0.3, z * half)),
        colour,
    );
}

/// Marks on the map what it is made of: the kind picked brightly, with numbers,
/// the thing picked brightest, and whatever the pointer is over.
fn mark(
    builder: Res<Builder>,
    mut gizmos: Gizmos,
    camera: Single<(&Camera, &GlobalTransform), (With<Camera3d>, Without<RenderLayers>)>,
    mut labels: Query<(
        &Label,
        &mut Text,
        &mut Node,
        &mut Visibility,
        &mut TextColor,
    )>,
    scale: Res<UiScale>,
) {
    let (camera, eye) = *camera;
    let mut named: Vec<(Vec3, String, Color)> = Vec::new();
    if let Some(draft) = builder.draft.as_ref().filter(|_| builder.editing) {
        let lift = Vec3::Y * 0.4;
        let dim = |colour: Color| colour.with_alpha(0.45);
        let picked = Color::srgb(1.0, 0.95, 0.2);
        let bright = Color::WHITE;
        let join = Color::srgb(0.3, 1.0, 0.45);
        let size = (builder.eye.far / 90.0).clamp(0.8, 12.0);
        let corners = &draft.map.road.corners;
        let place = |corner: &[f32; 4]| Vec3::new(corner[0], corner[1], corner[2]);
        if !draft.arena() && draft.layer == Layer::Corners {
            // The straights the corners round off, but where there is no road yet.
            for (i, corner) in corners.iter().enumerate() {
                if !(draft.open && i + 2 == corners.len()) {
                    let next = &corners[(i + 1) % corners.len()];
                    gizmos.line(
                        place(corner) + lift,
                        place(next) + lift,
                        Color::srgba(1.0, 1.0, 1.0, 0.16),
                    );
                }
            }
        }
        // How wide the road is at the corner picked: across it, from barrier to
        // barrier, with a ring at its edge to drag.
        if draft.layer == Layer::Corners && draft.picked < corners.len() {
            let (middle, right, half) = draft.across(draft.picked);
            gizmos.line(
                middle - right * half + lift,
                middle + right * half + lift,
                picked.with_alpha(0.8),
            );
            named.push((
                middle + right * (half + size * 2.0),
                format!("{} wide", half * 2.0),
                picked,
            ));
        }
        if let Some((end, tail)) = draft.ends() {
            // Where the road is drawn on from, and where it is to come back to.
            let before = place(&corners[corners.len().saturating_sub(3)]);
            let along = (end - before).with_y(0.0).normalize_or(Vec3::X);
            gizmos.arrow(end + lift, end + along * size * 5.0 + lift, picked);
            ring(&mut gizmos, end, size * 2.2, picked);
            let reached = corners.len() > 3;
            let says = if reached {
                "go on from here"
            } else {
                "draw from here"
            };
            named.push((end + along * size * 5.0, says.into(), picked));
            if reached {
                let colour = if builder.joining { join } else { dim(join) };
                ring(&mut gizmos, tail, size * 2.2, colour);
                gizmos.line(tail + lift, place(&corners[0]) + lift, colour);
                named.push((tail, "join here".into(), colour));
            }
            // Where a click would take it.
            if let (Some([x, z]), Hold::Nothing) = (builder.live, &builder.hold) {
                let to = if builder.joining {
                    tail
                } else {
                    Vec3::new(x, end.y, z)
                };
                let colour = if builder.joining {
                    join
                } else {
                    Color::srgba(1.0, 1.0, 1.0, 0.5)
                };
                gizmos.line(end + lift, to + lift, colour);
                ring(&mut gizmos, to, size * 0.7, colour);
            }
        } else if let Some(line) = draft.line().filter(|_| draft.sound().is_some()) {
            // Every hundred round the lap, when what is picked is so far round it.
            let measured = matches!(
                draft.layer,
                Layer::Pads | Layer::Banks | Layer::Infields | Layer::Byways | Layer::Hazards
            );
            for hundred in (0..(line.length / 100.0) as usize + 1).filter(|_| measured) {
                let (at, _, right) = line.sample(hundred as f32 * 100.0);
                gizmos.line(
                    at - right * 9.0 + lift,
                    at + right * 9.0 + lift,
                    Color::srgba(1.0, 1.0, 1.0, 0.5),
                );
                named.push((
                    at + right * 10.0,
                    format!("{}", hundred * 100),
                    Color::srgba(1.0, 1.0, 1.0, 0.7),
                ));
            }
            // Stretches of it: from where each begins to where it ends.
            let road = &draft.map.road;
            let banks = road
                .banks
                .iter()
                .map(|bank| (Layer::Banks, bank.from, bank.to));
            let infields = road
                .infields
                .iter()
                .map(|infield| (Layer::Infields, infield.from, infield.to));
            let byways = road
                .byways
                .iter()
                .map(|byway| (Layer::Byways, byway.from, byway.to));
            let mut counted = [0; 3];
            for (layer, from, to) in banks.chain(infields).chain(byways) {
                let slot = match layer {
                    Layer::Banks => 0,
                    Layer::Infields => 1,
                    _ => 2,
                };
                let index = counted[slot];
                counted[slot] += 1;
                let mine = layer == draft.layer;
                let colour = if mine && index == draft.picked {
                    picked
                } else if mine {
                    tint(layer)
                } else {
                    dim(tint(layer))
                };
                let steps = ((to - from) / line.spacing).max(1.0) as usize;
                let side = if layer == Layer::Byways { 0.0 } else { 7.0 };
                let along = (0..=steps).map(|step| {
                    let (at, _, right) =
                        line.sample(from + (to - from) * step as f32 / steps as f32);
                    at + right * side + lift * 2.0
                });
                gizmos.linestrip(along, colour);
            }
            for tunnel in &road.tunnels {
                let ends = [tunnel.from, tunnel.to].map(|[x, z]| Vec3::new(x, 9.0, z));
                gizmos.line(ends[0], ends[1], dim(tint(Layer::Tunnels)));
            }
        }
        if let Some(plan) = &draft.map.plan {
            let floor = plan
                .outline
                .iter()
                .chain(plan.outline.first())
                .map(|&[x, z]| Vec3::new(x, 0.0, z) + lift);
            gizmos.linestrip(floor, Color::srgba(1.0, 1.0, 1.0, 0.5));
            let line = plan
                .line
                .iter()
                .chain(plan.line.first())
                .map(|&at| Vec3::from(at) + lift);
            gizmos.linestrip(line, dim(tint(Layer::Line)));
            // What stands on the floor, the kind picked: each one's edges.
            let stood = |at: [f32; 2], half: [f32; 2], height: f32| {
                [
                    (-1.0, -1.0),
                    (1.0, -1.0),
                    (1.0, 1.0),
                    (-1.0, 1.0),
                    (-1.0, -1.0),
                ]
                .map(|(x, z)| Vec3::new(at[0] + half[0] * x, height + 0.3, at[1] + half[1] * z))
            };
            let edges: Vec<[Vec3; 5]> = match draft.layer {
                Layer::Blocks => plan
                    .blocks
                    .iter()
                    .map(|block| stood(block.at, block.half, block.height))
                    .collect(),
                Layer::Ramps => plan
                    .ramps
                    .iter()
                    .map(|ramp| stood(ramp.at, ramp.half, 0.0))
                    .collect(),
                Layer::Decks => plan
                    .decks
                    .iter()
                    .map(|deck| stood(deck.at, deck.half, deck.height))
                    .collect(),
                _ => Vec::new(),
            };
            for (index, edge) in edges.into_iter().enumerate() {
                gizmos.linestrip(
                    edge,
                    if index == draft.picked {
                        picked
                    } else {
                        tint(draft.layer)
                    },
                );
            }
        }
        // The rings: every kind's, faintly, and the kind picked with numbers.
        for &layer in draft.layers() {
            let mine = layer == draft.layer;
            for index in 0..draft.count(layer) {
                let chosen = mine && index == draft.picked;
                let tight = layer == Layer::Corners && draft.tight(index);
                for (part, at) in draft.handles(layer, index).into_iter().enumerate() {
                    if part > 0 && !chosen {
                        continue;
                    }
                    let over = builder.over == Some((layer, index, part));
                    let colour = match (over, chosen, tight, mine) {
                        (true, ..) => bright,
                        (_, true, ..) => picked,
                        (_, _, true, _) => ORANGE,
                        (.., true) => tint(layer),
                        _ => dim(tint(layer)),
                    };
                    let radius = size
                        * if over {
                            1.7
                        } else if chosen && part == 0 {
                            1.5
                        } else if mine {
                            1.0
                        } else {
                            0.6
                        };
                    ring(&mut gizmos, at, radius, colour);
                    if chosen || over {
                        ring(&mut gizmos, at, radius * 0.6, colour);
                        gizmos.line(at, at.with_y(0.0), dim(colour));
                    }
                    if mine && part == 0 {
                        let name = match layer {
                            Layer::Hazards => {
                                format!("{:?}", draft.map.hazards[index].kind).to_lowercase()
                            }
                            Layer::Scenery => {
                                format!("{:?}", draft.map.scenery[index].kind).to_lowercase()
                            }
                            Layer::Corners if index == 0 => "start".into(),
                            Layer::Corners if tight => format!("{} tight", index + 1),
                            _ => format!("{}", index + 1),
                        };
                        named.push((at, name, colour));
                    }
                }
            }
        }
        // The squares half way between, to be dragged out into more.
        for (after, at) in draft.mids() {
            let over = builder.between == Some(after);
            let colour = if over { bright } else { dim(tint(draft.layer)) };
            square(&mut gizmos, at, size * if over { 0.9 } else { 0.5 }, colour);
        }
    }
    // The numbers, each beside its thing as the screen has it.
    for (label, mut text, mut node, mut visibility, mut colour) in &mut labels {
        let seen = named.get(label.0).and_then(|(at, name, colour)| {
            let at = camera.world_to_viewport(eye, *at).ok()?;
            Some((at / scale.0, name, *colour))
        });
        match seen {
            Some((at, name, tint)) => {
                if text.0 != *name {
                    text.0 = name.clone();
                }
                (node.left, node.top) = (Val::Px(at.x + 8.0), Val::Px(at.y - 16.0));
                (*visibility, colour.0) = (Visibility::Visible, tint.with_alpha(1.0));
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

const KEYS: [&str; 4] = [
    "TAB the next kind of thing    , . the next of it    ARROWS move it    W S its rows    A D turn a row (SHIFT more, ALT less)    ENTER type into one",
    "N another    BACKSPACE take it away    U undo    Y redo    O join a road to its start, or open it again    T try the map    F5 keep it",
    "Q E turn the view    R F tilt it    Z X nearer, further    C the whole map    H these keys    ESCAPE leave",
    "The pointer: drag a ring to move what it marks, the ground to slide the view, the right button to turn it; the wheel comes nearer.",
];

/// Says what the panels have on them: the list, or the map and what is picked of
/// it, with what can be done just now along the bottom.
fn say(
    mut commands: Commands,
    builder: Res<Builder>,
    circuits: Res<Circuits>,
    mut board: ResMut<Board>,
    mut panes: Query<(Entity, &panel::Pane, &mut Visibility)>,
    mut cells: Query<
        (
            &panel::Spot,
            &mut Text,
            &mut TextColor,
            &mut BackgroundColor,
        ),
        Without<panel::Pane>,
    >,
) {
    let mut sheet = Sheet::default();
    let title = |words: String| vec![Cell::new(words, Look::Title)];
    let lit = |on: bool| if on { Look::Lit } else { Look::Plain };
    let draft = builder.draft.as_ref().filter(|_| builder.editing);
    match draft {
        None => {
            sheet.side.push(title("BUILD A MAP".into()));
            let rows = shelf(&circuits);
            for (row, on) in rows.iter().enumerate() {
                let name = match *on {
                    Shelf::New(false) => "A new circuit".to_string(),
                    Shelf::New(true) => "A new arena".to_string(),
                    Shelf::Old(index) => {
                        let circuit = &circuits.0[index];
                        let kind = if circuit.arena() { "arena" } else { "circuit" };
                        let whose = if crate::map::carried(circuit.key()) {
                            ", the game's"
                        } else {
                            ""
                        };
                        let by = if circuit.author.is_empty() {
                            String::new()
                        } else {
                            format!(" by {}", circuit.author)
                        };
                        format!("{}   ({kind}{whose}){by}", circuit.name)
                    }
                };
                sheet.side.push(vec![
                    Cell::new(name, lit(row == builder.row))
                        .wide(470.0)
                        .does(Act::Shelf(row)),
                ]);
            }
            let own = rows
                .get(builder.row)
                .is_some_and(|on| on.own(&circuits).is_some());
            sheet.bar = vec![
                Cell::button("Open", Act::Open, false),
                Cell::button("Delete", Act::Delete, !own),
                Cell::button("Main menu", Act::Leave, false),
            ];
            sheet.hint = vec![
                "Click a map to see it, and click it again, or Open, to change it. One of the game's own is changed as another like it.".into(),
                "A new circuit is a start line for its road to be drawn from, and a new arena a floor to put things on.".into(),
            ];
        }
        Some(draft) => {
            let kind = match (
                draft.arena(),
                draft.open,
                draft.lap().filter(|_| draft.sound().is_some()),
            ) {
                (true, ..) => "arena".to_string(),
                (_, true, _) => "a road being drawn".into(),
                (_, _, Some(lap)) => format!("circuit, {lap:.0} round"),
                _ => "circuit".into(),
            };
            let kept = if draft.unsaved { "not kept" } else { "kept" };
            sheet
                .side
                .push(title(format!("{}   ({kind}, {kept})", draft.map.name)));
            for &layer in draft.layers() {
                let mut row = vec![
                    Cell::new(layer.label(), lit(layer == draft.layer))
                        .wide(150.0)
                        .does(Act::Kind(layer)),
                ];
                let count = if layer == Layer::Map {
                    String::new()
                } else {
                    draft.count(layer).to_string()
                };
                row.push(Cell::new(count, Look::Dim).wide(34.0));
                if layer.added() {
                    row.push(Cell::button("+", Act::Add(layer), false));
                }
                sheet.side.push(row);
            }
            let rows = draft.rows();
            let which = match (draft.count(draft.layer), draft.layer) {
                (_, Layer::Map) => String::new(),
                (0, layer) if layer.added() => {
                    format!("no {} yet: + puts one on the map", layer.label())
                }
                (0, layer) => format!("no {} yet", layer.label()),
                (count, layer) => format!("{} {} of {count}", layer.one(), draft.picked + 1),
            };
            sheet.side.push(vec![Cell::new(which, Look::Dim)]);
            for (n, row) in rows.iter().enumerate() {
                let on = n == draft.row;
                let label = Cell::new(row.label.as_str(), lit(on))
                    .wide(200.0)
                    .does(Act::Row(n));
                sheet.side.push(match (row.typed, &builder.typing) {
                    (true, Some(typing)) if on => vec![
                        label,
                        Cell::new(format!("{typing}_"), Look::Lit).wide(206.0),
                    ],
                    (true, _) => vec![
                        label,
                        Cell::new(row.value.as_str(), Look::Plain)
                            .wide(206.0)
                            .does(Act::Type(n)),
                    ],
                    (false, _) => vec![
                        label,
                        Cell::button("<", Act::Turn(n, -1.0), false),
                        Cell::new(row.value.as_str(), Look::Plain)
                            .wide(150.0)
                            .does(Act::Row(n)),
                        Cell::button(">", Act::Turn(n, 1.0), false),
                    ],
                });
            }
            let join = match (draft.arena(), draft.open) {
                (true, _) => None,
                (_, true) => Some(Cell::button(
                    "Join the start",
                    Act::Loop,
                    draft.map.road.corners.len() < 4,
                )),
                (_, false) => Some(Cell::button("Draw more", Act::Loop, false)),
            };
            sheet.bar = vec![
                Cell::button("Undo", Act::Undo, false),
                Cell::button("Redo", Act::Redo, false),
            ];
            sheet.bar.push(Cell::button("Whole map", Act::Whole, false));
            sheet.bar.extend(join);
            sheet.bar.extend([
                Cell::button("Try it", Act::Try, draft.sound().is_none()),
                Cell::button("Keep", Act::Keep, draft.sound().is_none()),
                Cell::button("Leave", Act::Leave, false),
            ]);
            // What the pointer does just now.
            let drawn = draft.map.road.corners.len() > 3;
            let now = match (builder.carrying, draft.open, draft.layer) {
                (true, ..) => "Click on the map to put it down, or press Escape to do without it.".to_string(),
                (_, true, _) if !drawn => "Draw the road: drag the end of the start's straight wherever the road is to go, or click where each corner is to be.".into(),
                (_, true, _) => "Go on from the road's end, dragging or clicking. Bring it back to the straight behind the start line to finish the lap. Backspace takes a corner away.".into(),
                (_, _, Layer::Corners) => "Drag a ring to move a corner, a small square to pull another out of the road, and the ring at the road's edge to make it wider or narrower there. The rows on the left raise it, round it and widen it.".into(),
                (_, _, layer) if layer.added() => format!("{} The + beside its name puts another on the map, and a ring is dragged to move one.", layer.about()),
                (_, _, layer) => layer.about().into(),
            };
            sheet.hint = vec![now];
            match builder.help {
                true => sheet.hint.extend(KEYS.map(String::from)),
                false => sheet.hint.push("Drag the ground to slide the view and the right button to turn it; the wheel comes nearer. H says what the keys do.".into()),
            }
        }
    }
    // What is wrong with the map is said until it is put right, but while
    // something is being asked; what is asked until it is answered; and
    // anything else for a little while.
    let wrong = draft
        .and_then(|draft| draft.wrong.clone())
        .filter(|_| !builder.sure);
    let (said, bad) = match wrong {
        Some(wrong) => (format!("Not a map as it is: {wrong}"), true),
        None if builder.sure || builder.said_at.elapsed().as_secs_f32() < SAID_FOR => {
            builder.said.clone()
        }
        None => default(),
    };
    if !said.is_empty() {
        sheet.said = vec![Cell::new(said, if bad { Look::Wrong } else { Look::Good })];
    }
    panel::lay(&mut commands, &mut board, sheet, &mut panes, &mut cells);
}
