//! The racer the garage shows: the minifigure standing on its pad, going through
//! its moves one after another, and beside it the racer's car on its own, in the
//! set the original has them in. After `RacerModelScreenBase` (`CreateModelSlots`:
//! where each is put; `RefreshSlotModel`: what each is made of; `AlignDriverSlots`
//! and `AlignCarSlots`: which way each faces; `PlayRandomAnimation` and `Update`:
//! which move the figure makes next, a different one each time the last has
//! played out), `RacerModelSlot` (the slots, neither of which turns, and the figure
//! of which moves) and `MenuFramedSceneView` over `MenuSceneView` (the showcase of
//! `GARAGE.MIB`, with the world `RS_SET/RACER.WDB` and its camera). The set's own
//! models (the ground, the pools of light, the pad and the shadows) are loaded as any
//! world file's are.
//!
//! The original draws it straight onto the screen. The port's menus are drawn over
//! everything else, so here it has a camera of its own that draws it onto a picture,
//! which the menu shows where the original's showcase is, as the main menu's figure is
//! (`mascot`). The showcase's frame is not drawn.

use std::sync::Arc;

use super::{Art, Menu, mascot, workshop};
use crate::assets::{
    adb::Animation,
    gdb::parse_skeleton,
    lrs::Racer,
    tokens::{Token, tokenize},
};
use crate::garage::Garage;
use crate::menu::Settings;
use crate::scenery::{self, Animated, PropDef, Rig};
use crate::world::Library;
use crate::{build, physics::UNIT};
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    mesh::skinning::SkinnedMeshInverseBindposes,
    prelude::*,
    render::render_resource::TextureFormat,
};

const SET: &str = "/MENUDATA/RS_SET";
const SCENE: &str = "/MENUDATA/RS_SET/RACER.WDB";
const BLENDED: &str = "/MENUDATA/RS_SET/BLENDED.WDB";
const MOVES: &str = "/MENUDATA/RSANIM.ADB";
/// `RacerModelScreenBase::CreateModelSlots`: where the car and the figure are put.
const CAR_AT: Vec3 = Vec3::new(-11.52, -6.767, 0.0);
const FIGURE_AT: Vec3 = Vec3::new(-0.938, -0.898, 1.487);
/// `AlignDriverSlots`: which way the figure faces.
const FIGURE_FACING: Vec3 = Vec3::new(0.963_631, -0.267_238, 0.0);
/// `AlignCarSlots`: the car faces as the shadow it stands on (`crsdow` of
/// `BLENDED.WDB`) does at the start of its animation, which is as it is placed.
const CAR_FACING: Vec3 = Vec3::new(0.829_038, -0.559_193, 0.0);
/// `g_racerIdleAnimTextIds`: the moves of `RSANIM.ADB` the figure makes, by name.
const IDLE: [&str; 7] = ["breath1", "breath2", "swayf", "watch", "hips", "tapfoot", "hey!"];
/// How many times finer than the menu's screen its picture is drawn.
const DETAIL: f32 = 3.0;
const LAYER: usize = 13;

/// The garage's racer on show, while there is one.
#[derive(Resource, Default)]
pub struct Stage {
    pub picture: Option<Handle<Image>>,
    /// Whose it is, which is what it is remade for.
    shown: Option<Racer>,
    entities: Vec<Entity>,
    /// Which parts of the moves are the idle ones.
    idle: Vec<usize>,
}

/// What the showcase's camera draws: the set and the racer in it.
#[derive(Component)]
pub struct Staged;

/// The minifigure.
#[derive(Component)]
pub struct Figure;

/// The names of a `.ADB` file's parts, in order.
fn parts(data: &[u8]) -> Vec<String> {
    let tokens = tokenize(data);
    tokens
        .windows(3)
        .filter_map(|w| match (&w[0], &w[1], &w[2]) {
            (Token::Key(0x2c), Token::Str(name), Token::LCurly) => Some(name.to_lowercase()),
            _ => None,
        })
        .collect()
}

/// The figure of a racer on its skeleton with the garage's moves, on its pad.
fn figure(art: &Art, racer: &Racer) -> Option<PropDef> {
    let jam = art.jam();
    let catalogue = build::Catalogue::open(jam)?;
    let model = build::figure(jam, &catalogue, racer.cosmetics, true)?;
    let (files, folders) = build::Catalogue::files();
    let library = Library::new(jam, files.iter().map(String::as_str), &folders);
    let rig = Rig {
        bones: Arc::new(parse_skeleton(build::skeleton(jam, &catalogue, racer.cosmetics, true)?)?),
        animation: Arc::new(Animation::parse(jam.get(MOVES)?)?),
    };
    let mut made = PropDef::made("racer", &model, Some(rig), &library);
    // Facing along X is facing as it was made; it is turned about Z from there.
    made.moved(Quat::from_rotation_z(FIGURE_FACING.y.atan2(FIGURE_FACING.x)), FIGURE_AT, 1.0);
    Some(made)
}

/// The set's camera (`Camera01`, of the cameras the world file lists), which comes after its models.
fn camera(scene: &[Token]) -> Option<(Vec3, Vec3, Vec3, f32)> {
    let at = scene.iter().position(|token| *token == Token::Key(0x43))?;
    mascot::camera(&scene[at..])
}

fn clear(commands: &mut Commands, stage: &mut Stage) {
    for entity in stage.entities.drain(..) {
        commands.entity(entity).despawn();
    }
    (stage.picture, stage.shown) = (None, None);
}

/// Keeps the racer the garage's page shows on its stage, and nowhere else.
pub fn keep(
    mut commands: Commands,
    art: Res<Art>,
    garage: Res<Garage>,
    settings: Res<Settings>,
    bench: Res<workshop::Bench>,
    mut menu: ResMut<Menu>,
    mut stage: ResMut<Stage>,
    (mut meshes, mut materials, mut images, mut binds): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<SkinnedMeshInverseBindposes>>,
    ),
) {
    let Some(racer) = workshop::showcased(menu.page, &bench, &garage, &settings) else {
        if !stage.entities.is_empty() {
            clear(&mut commands, &mut stage);
        }
        return;
    };
    if stage.shown.as_ref() == Some(&racer) {
        return;
    }
    clear(&mut commands, &mut stage);
    let jam = art.jam();
    // A race run mirrored leaves the world mirrored; the menu's is as it was made.
    scenery::set_mirror(false);
    let scene = tokenize(jam.get(SCENE).unwrap_or_default());
    let (Some(figure), Some((eye, forward, up, fov))) = (figure(&art, &racer), camera(&scene))
    else {
        return;
    };
    let mut entities = Vec::new();
    // The set: the ground, the pools of light, the pad and the shadows.
    let mut own: Vec<&str> = jam
        .list(SET)
        .filter(|file| file.ends_with(".MDB") || file.ends_with(".TDB"))
        .collect();
    own.sort();
    let library = Library::new(jam, own.iter().copied(), &[SET]);
    for def in scenery::load_files(jam, SET, &[SCENE, BLENDED], &library, |_, _| true) {
        let prop = scenery::spawn(def, &mut commands, &mut meshes, &mut materials, &mut images, &mut binds);
        commands.entity(prop).insert(Staged);
        entities.push(prop);
    }
    // The figure, which begins on a move of its own.
    let figure = scenery::spawn(figure, &mut commands, &mut meshes, &mut materials, &mut images, &mut binds);
    commands.entity(figure).insert((Staged, Figure));
    entities.push(figure);
    // The car, on the ground and facing as `AlignCarSlots` has it.
    if let Some(mut model) = crate::world::load_built(jam, &racer, true) {
        // The driver stands beside it.
        model.driver.clear();
        let way = scenery::to_world(CAR_FACING).normalize_or_zero();
        let car = commands
            .spawn((
                Staged,
                Transform::from_translation(scenery::to_world(CAR_AT) + Vec3::Y * crate::physics::RIDE_HEIGHT)
                    .looking_to(way, Vec3::Y),
                Visibility::default(),
            ))
            .id();
        crate::time_race::dress(&mut commands, car, model, &mut meshes, &mut materials, &mut images, None);
        entities.push(car);
    }
    // `MenuSceneView::SetupCamera`: the set's own camera, drawn onto a picture the
    // size of the showcase.
    let area = art.place("garage", "showcase");
    let size = (area.size() * DETAIL).max(Vec2::ONE).as_uvec2();
    let picture = images.add(Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None));
    let way = |v: Vec3| scenery::to_world(v).normalize_or_zero();
    let lens = PerspectiveProjection {
        fov: fov.to_radians(),
        near: 5.0 * UNIT,
        far: 800.0 * UNIT,
        ..default()
    };
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -6,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            Projection::Perspective(lens),
            RenderTarget::from(picture.clone()),
            RenderLayers::layer(LAYER),
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            Transform::from_translation(scenery::to_world(eye)).looking_to(way(forward), way(up)),
        ))
        .id();
    entities.push(camera);
    let moves = parts(jam.get(MOVES).unwrap_or_default());
    *stage = Stage {
        picture: Some(picture),
        shown: Some(racer),
        entities,
        idle: IDLE.iter().filter_map(|name| moves.iter().position(|m| m == name)).collect(),
    };
    menu.drawn = false;
}

/// `RacerModelScreenBase::Update`: when a move has played out the figure makes
/// another, never the same twice running.
pub fn idle(
    stage: Res<Stage>,
    time: Res<Time<Real>>,
    mut figures: Query<&mut Animated, With<Figure>>,
    mut chance: Local<u32>,
) {
    let Ok(mut animated) = figures.single_mut() else {
        return;
    };
    if stage.idle.is_empty() || (animated.playing && stage.idle.contains(&animated.part)) {
        return;
    }
    // Which is a random pick from the seven.
    *chance = chance
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223)
        .wrapping_add(time.elapsed().subsec_nanos());
    let mut part = stage.idle[(*chance >> 8) as usize % stage.idle.len()];
    if part == animated.part && stage.idle.len() > 1 {
        let at = stage.idle.iter().position(|&p| p == part).unwrap_or(0);
        part = stage.idle[(at + 1) % stage.idle.len()];
    }
    animated.play(part, false);
}

/// Has the stage's camera, and no other, draw the meshes it is made of.
pub fn dress(
    mut commands: Commands,
    made: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    staged: Query<(), With<Staged>>,
) {
    for mesh in &made {
        if parents.iter_ancestors(mesh).any(|above| staged.contains(above)) {
            commands.entity(mesh).insert(RenderLayers::layer(LAYER));
        }
    }
}

/// Clears the stage away when the menus are left.
pub fn put_away(mut commands: Commands, mut stage: ResMut<Stage>) {
    clear(&mut commands, &mut stage);
}

#[cfg(test)]
#[test]
fn the_figure_has_the_idle_moves_and_the_set_has_a_camera() {
    let Some(art) = super::load_art() else {
        return;
    };
    let moves = parts(art.jam().get(MOVES).unwrap());
    for name in IDLE {
        assert!(moves.iter().any(|m| m == name), "{name}");
    }
    let scene = tokenize(art.jam().get(SCENE).unwrap());
    let (eye, forward, _, fov) = camera(&scene).unwrap();
    assert_eq!(fov.round(), 32.0);
    // It looks at the car, which is far down its line of sight.
    let to = scenery::to_world(CAR_AT) - scenery::to_world(eye);
    assert!(to.normalize().dot(scenery::to_world(forward).normalize()) > 0.95);
    let racer = art_racer(&art);
    assert!(figure(&art, &racer).is_some());
}

#[cfg(test)]
fn art_racer(art: &Art) -> Racer {
    crate::garage::stock(art.jam()).into_iter().next().unwrap_or_default()
}
