//! The minifigure that stands on the main menu: the champion of the last circuit
//! opened, shifting from foot to foot and every so often doing a turn. Follows
//! `MainMenuScreen` (who it is, where it stands and what it does) and
//! `MainMenuModelSlot` (what it is made of), seen through the camera of the scene
//! the main menu's layout names (`TT.WDB`).
//!
//! The original draws it straight onto the screen between the menu's background
//! and its buttons. The port's menus are drawn over everything else, so here it has
//! a camera of its own that draws it onto a picture, which the menu shows where the
//! original's scene is.

use std::sync::Arc;

use super::{Art, Menu, Page};
use crate::assets::{
    adb::Animation,
    gdb::parse_skeleton,
    lrs::Cosmetics,
    tokens::{Token, tokenize},
};
use crate::audio::{Sfx, id};
use crate::championship::Championship;
use crate::film::Showing;
use crate::scenery::{self, Animated, PropDef, Rig};
use crate::world::Library;
use crate::{build, roster};
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    mesh::skinning::SkinnedMeshInverseBindposes,
    prelude::*,
    render::render_resource::TextureFormat,
};

/// `g_menuDriverCosmeticIds`: whose figure it is, by how many circuits are open.
const WHO: [usize; 8] = [0x13, 0x12, 0x16, 0x15, 0x14, 0x17, 0, 0];
/// `MainMenuScreen::CreateDriverScene`: where it stands and which way it faces.
const PLACE: Vec3 = Vec3::new(18.181_229, -10.622_759, 0.025708);
const FACING: Vec3 = Vec3::new(0.972_37, -0.233445, 0.0);
/// The scene the main menu's layout has it in, and the moves it is given there.
const SCENE: &str = "/MENUDATA/TT.WDB";
const MOVES: &str = "/MENUDATA/LEGOMAN.ADB";
/// The parts of those moves: it comes on with the first, then stands about, and
/// has the turn.
const IDLE: usize = 1;
const TURN: usize = 2;
/// `MainMenuScreen::Update`: the turn comes round this often, in seconds, and this
/// many frames into it is heard.
const TURN_EVERY: f32 = 30.0;
const TURN_SOUND_FRAME: f32 = 208.0;
/// Which of the front end's sounds that is; the original numbers them from one.
const TURN_SOUND: usize = id::MENU + 0x1d - 1;
/// How many times finer than the menu's screen its picture is drawn.
const DETAIL: f32 = 3.0;
/// It is drawn by its own camera alone.
const LAYER: usize = 8;

/// The figure on the main menu, while there is one.
#[derive(Resource, Default)]
pub struct Mascot {
    /// The picture it is drawn onto.
    pub picture: Option<Handle<Image>>,
    /// The figure and its camera.
    stage: Option<[Entity; 2]>,
    /// Time until its next turn, and whether this turn has been heard.
    turn: f32,
    heard: bool,
}

/// The figure itself.
#[derive(Component)]
pub struct Standing;

/// The scene's camera: where it is, the way it looks, what is up to it and how
/// wide it sees, in the game's own terms.
fn camera(tokens: &[Token]) -> Option<(Vec3, Vec3, Vec3, f32)> {
    let number = |key: u16, n: usize| {
        let at = tokens.iter().position(|t| *t == Token::Key(key))?;
        match tokens.get(at + 1 + n)? {
            Token::Float(value) => Some(*value),
            Token::Int(value) => Some(*value as f32),
            _ => None,
        }
    };
    let vec3 = |key: u16, from: usize| {
        Some(Vec3::new(
            number(key, from)?,
            number(key, from + 1)?,
            number(key, from + 2)?,
        ))
    };
    Some((vec3(0x31, 0)?, vec3(0x32, 0)?, vec3(0x32, 3)?, number(0x47, 0)?))
}

/// The figure of whoever is champion of the last circuit opened, ready to stand.
fn figure(art: &Art, opened: usize) -> Option<PropDef> {
    let jam = art.jam();
    let who = WHO[opened.saturating_sub(1).min(WHO.len() - 1)];
    let cosmetics = roster::cosmetics(jam, who).unwrap_or(Cosmetics::default());
    let catalogue = build::Catalogue::open(jam)?;
    let model = build::figure(jam, &catalogue, cosmetics, true)?;
    let (files, folders) = build::Catalogue::files();
    let library = Library::new(jam, files.iter().map(String::as_str), &folders);
    let rig = Rig {
        bones: Arc::new(parse_skeleton(build::skeleton(
            jam, &catalogue, cosmetics, true,
        )?)?),
        animation: Arc::new(Animation::parse(jam.get(MOVES)?)?),
    };
    let mut made = PropDef::made("mascot", &model, Some(rig), &library);
    // Facing along X is facing as it was made; it is turned about Z from there.
    made.moved(Quat::from_rotation_z(FACING.y.atan2(FACING.x)), PLACE, 1.0);
    Some(made)
}

/// Keeps the figure on the main menu, and nowhere else.
pub fn keep(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    showing: Res<Showing>,
    championship: Res<Championship>,
    mut menu: ResMut<Menu>,
    mut mascot: ResMut<Mascot>,
    mut sfx: ResMut<Sfx>,
    mut figures: Query<&mut Animated, With<Standing>>,
    (mut meshes, mut materials, mut images, mut binds): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<SkinnedMeshInverseBindposes>>,
    ),
) {
    let wanted = menu.page == Page::Main && !showing.busy();
    if !wanted {
        put_away(commands, mascot);
        return;
    }
    if mascot.stage.is_none() {
        // A race run mirrored leaves the world mirrored; the menu's is as it was made.
        scenery::set_mirror(false);
        let scene = tokenize(art.jam().get(SCENE).unwrap_or_default());
        let (Some(made), Some((eye, forward, up, fov))) =
            (figure(&art, championship.unlocked), camera(&scene))
        else {
            return;
        };
        let figure = scenery::spawn(
            made,
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut binds,
        );
        commands.entity(figure).insert(Standing);
        // `MainMenuScreen::CreateSceneView`: the scene has the right of the screen.
        let area = art.place("main", "platform");
        let size = (area.size() * DETAIL).max(Vec2::ONE).as_uvec2();
        let picture = images.add(Image::new_target_texture(
            size.x,
            size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        let way = |v: Vec3| scenery::to_world(v).normalize_or_zero();
        let lens = PerspectiveProjection {
            fov: fov.to_radians(),
            ..default()
        };
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    // Before the screen's own camera, with nothing behind the figure.
                    order: -2,
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
        *mascot = Mascot {
            picture: Some(picture),
            stage: Some([figure, camera]),
            turn: TURN_EVERY,
            heard: false,
        };
        menu.drawn = false;
        return;
    }
    let Ok(mut animated) = figures.single_mut() else {
        return;
    };
    // It comes on, then stands about; the turn takes it from that and gives it back.
    if animated.part != IDLE && animated.queued.is_none() {
        (animated.looping, animated.queued) = (false, Some((IDLE, true)));
    }
    mascot.turn -= time.delta_secs();
    if mascot.turn <= 0.0 {
        animated.play(TURN, false);
        animated.queued = Some((IDLE, true));
        (mascot.turn, mascot.heard) = (TURN_EVERY, false);
    }
    if animated.part == TURN && animated.frame() >= TURN_SOUND_FRAME && !mascot.heard {
        mascot.heard = true;
        sfx.play(TURN_SOUND);
    }
}

/// Has the figure's own camera, and no other, draw the meshes it is made of.
pub fn dress(
    mut commands: Commands,
    made: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    figures: Query<(), With<Standing>>,
) {
    for mesh in &made {
        if parents.iter_ancestors(mesh).any(|above| figures.contains(above)) {
            commands.entity(mesh).insert(RenderLayers::layer(LAYER));
        }
    }
}

/// Clears the figure away.
pub fn put_away(mut commands: Commands, mut mascot: ResMut<Mascot>) {
    if let Some(stage) = mascot.stage.take() {
        for entity in stage {
            commands.entity(entity).despawn();
        }
        mascot.picture = None;
    }
}

#[cfg(test)]
#[test]
fn the_champion_stands_before_the_scene_s_camera() {
    let Some(art) = super::load_art() else {
        return;
    };
    let scene = tokenize(art.jam().get(SCENE).unwrap());
    let (eye, forward, up, fov) = camera(&scene).unwrap();
    assert_eq!(fov.round(), 36.0);
    // The camera looks back along X at where the figure stands, a little below it.
    let to = PLACE - eye;
    assert!(to.normalize().dot(forward.normalize()) > 0.9, "{to} {forward}");
    assert!(up.z > 0.99 && to.length() > 5.0);
    // Whoever it is, it has a bone for every part of it and the three moves.
    for opened in [1, 4, 7] {
        let made = figure(&art, opened).unwrap();
        let rig = made.rig().unwrap();
        assert_eq!(rig.animation.parts.len(), 3);
        assert_eq!(rig.bones.len(), 29);
        assert_eq!(made.position(), PLACE);
    }
}
