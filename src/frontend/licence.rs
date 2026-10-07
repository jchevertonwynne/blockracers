//! The driver's photograph on the licence: the minifigure standing in the frame the
//! licence's layout gives the scene (`platform`), pulling the face the player has
//! chosen. After `DriverLicenseScreen::CreateDriverScene` and `MainMenuModelSlot`
//! (`SetFace`).
//!
//! As the main menu's figure is (`mascot`), it is drawn by a camera of its own onto a
//! picture that the menu shows where the original's scene is. The camera is placed
//! to take the figure from the front at the layout's field of view; the original's
//! own camera is a scene file's, which the port doesn't have for this screen.

use std::sync::Arc;

use super::{Art, Menu, Page, workshop::Bench};
use crate::assets::{adb::Animation, gdb::parse_skeleton, lrs::Cosmetics};
use crate::scenery::{self, Animated, PropDef, Rig};
use crate::world::Library;
use crate::build;
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    mesh::skinning::SkinnedMeshInverseBindposes,
    prelude::*,
    render::render_resource::TextureFormat,
};

const MOVES: &str = "/MENUDATA/LEGOMAN.ADB";
/// The part of those moves it stands about in.
const IDLE: usize = 1;
/// The layout's camera: how wide it sees (`DRVRLICE.MIB`, 45 degrees).
const FOV: f32 = 45.0;
/// How many times finer than the menu's screen the picture is drawn.
const DETAIL: f32 = 3.0;
const LAYER: usize = 11;
/// Where the camera is, and what it looks at, in the figure's own space (X forward,
/// Z up).
const EYE: Vec3 = Vec3::new(12.5, 0.0, 1.5);
const AT: Vec3 = Vec3::new(0.0, 0.0, 1.3);

#[derive(Resource, Default)]
pub struct Photo {
    pub picture: Option<Handle<Image>>,
    stage: Option<[Entity; 2]>,
    shown: Option<Cosmetics>,
}

#[derive(Component)]
pub struct Posing;

/// The photograph's camera, which is not the menu's.
#[derive(Component)]
pub struct Lens;

fn figure(art: &Art, cosmetics: Cosmetics) -> Option<PropDef> {
    let jam = art.jam();
    let catalogue = build::Catalogue::open(jam)?;
    let model = build::figure(jam, &catalogue, cosmetics, true)?;
    let (files, folders) = build::Catalogue::files();
    let library = Library::new(jam, files.iter().map(String::as_str), &folders);
    let rig = Rig {
        bones: Arc::new(parse_skeleton(build::skeleton(jam, &catalogue, cosmetics, true)?)?),
        animation: Arc::new(Animation::parse(jam.get(MOVES)?)?),
    };
    Some(PropDef::made("licence", &model, Some(rig), &library))
}

/// Keeps the photograph on the licence page, as the face chosen has it.
pub fn keep(
    mut commands: Commands,
    art: Res<Art>,
    bench: Res<Bench>,
    mut menu: ResMut<Menu>,
    mut photo: ResMut<Photo>,
    mut figures: Query<&mut Animated, With<Posing>>,
    (mut meshes, mut materials, mut images, mut binds): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<SkinnedMeshInverseBindposes>>,
    ),
) {
    if menu.page != Page::Licence {
        put_away(commands, photo);
        return;
    }
    let cosmetics = bench.cosmetics();
    if photo.stage.is_some() && photo.shown == Some(cosmetics) {
        if let Ok(mut animated) = figures.single_mut()
            && animated.part != IDLE
        {
            animated.play(IDLE, true);
        }
        return;
    }
    let Some(made) = figure(&art, cosmetics) else {
        return;
    };
    // The picture and its camera are kept; the figure is made afresh.
    let picture = match photo.picture.clone().filter(|_| photo.stage.is_some()) {
        Some(picture) => picture,
        None => {
            let size = (art.place("drvrlice", "platform").size() * DETAIL)
                .max(Vec2::ONE)
                .as_uvec2();
            images.add(Image::new_target_texture(
                size.x,
                size.y,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ))
        }
    };
    let camera = match photo.stage {
        Some([old, camera]) => {
            commands.entity(old).despawn();
            camera
        }
        None => {
            scenery::set_mirror(false);
            let way = |v: Vec3| scenery::to_world(v);
            let lens = PerspectiveProjection {
                fov: FOV.to_radians(),
                ..default()
            };
            commands
                .spawn((
                    Camera3d::default(),
                    Lens,
                    Camera {
                        order: -5,
                        clear_color: ClearColorConfig::Custom(Color::NONE),
                        ..default()
                    },
                    Projection::Perspective(lens),
                    RenderTarget::from(picture.clone()),
                    RenderLayers::layer(LAYER),
                    bevy::core_pipeline::tonemapping::Tonemapping::None,
                    Transform::from_translation(way(EYE)).looking_at(way(AT), Vec3::Y),
                ))
                .id()
        }
    };
    let entity = scenery::spawn(
        made,
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        &mut binds,
    );
    commands.entity(entity).insert(Posing);
    *photo = Photo {
        picture: Some(picture),
        stage: Some([entity, camera]),
        shown: Some(cosmetics),
    };
    menu.drawn = false;
}

/// Has the photograph's own camera, and no other, draw the meshes the figure is made of.
pub fn dress(
    mut commands: Commands,
    made: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    figures: Query<(), With<Posing>>,
) {
    for mesh in &made {
        if parents.iter_ancestors(mesh).any(|above| figures.contains(above)) {
            commands.entity(mesh).insert(RenderLayers::layer(LAYER));
        }
    }
}

pub fn put_away(mut commands: Commands, mut photo: ResMut<Photo>) {
    if let Some(stage) = photo.stage.take() {
        for entity in stage {
            commands.entity(entity).despawn();
        }
        *photo = Photo::default();
    }
}
