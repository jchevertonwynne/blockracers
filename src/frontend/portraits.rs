//! Pictures of the minifigures of everyone in a session's room, beside their names.
//! The port's own. Each is the figure of what the player races as (`world::
//! load_figure`), stood somewhere out of the way with a camera of its own, which
//! draws it onto a picture the room's page shows.

use super::{Art, Menu, Page};
use crate::net::{
    Role,
    protocol::{Peer, Ride},
    room::Room,
};
use crate::physics::UNIT;
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    render::render_resource::TextureFormat,
};

/// The shape of a picture, and how many times finer than that it is drawn.
pub const SIZE: Vec2 = Vec2::new(44.0, 48.0);
const DETAIL: f32 = 8.0;
/// The figures are drawn by their own cameras alone.
const LAYER: usize = 7;
/// Where they stand: far under anything else, this far apart.
const STUDIO: Vec3 = Vec3::new(0.0, -900.0, 0.0);
const APART: f32 = 40.0;
/// How far round from face on a figure is turned.
const TURNED: f32 = 0.5;

struct Portrait {
    peer: Peer,
    ride: Ride,
    picture: Handle<Image>,
    /// Where its figure stands.
    at: Vec3,
    /// The figure and its camera.
    stage: [Entity; 2],
}

#[derive(Resource, Default)]
pub struct Portraits(Vec<Portrait>);

impl Portraits {
    /// The picture of a player in the room.
    pub fn of(&self, peer: Peer) -> Option<Handle<Image>> {
        let portrait = self.0.iter().find(|portrait| portrait.peer == peer)?;
        Some(portrait.picture.clone())
    }
}

/// A figure being drawn.
#[derive(Component)]
pub struct Sitter;

/// Keeps a picture of everyone the room says is in it, while the room is shown.
pub fn keep(
    mut commands: Commands,
    art: Res<Art>,
    role: Res<Role>,
    room: Res<Room>,
    mut menu: ResMut<Menu>,
    mut portraits: ResMut<Portraits>,
    (mut meshes, mut materials, mut images): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
    ),
) {
    let wanted: &[(Peer, Ride)] = if menu.page == Page::Room && *role != Role::Offline {
        &room.rides
    } else {
        &[]
    };
    let before = portraits.0.len();
    portraits.0.retain(|portrait| {
        let still = wanted.contains(&(portrait.peer, portrait.ride.clone()));
        if !still {
            for entity in portrait.stage {
                commands.entity(entity).despawn();
            }
        }
        still
    });
    let mut changed = portraits.0.len() != before;
    for (place, (peer, ride)) in wanted.iter().enumerate() {
        if portraits.0.iter().any(|portrait| portrait.peer == *peer) {
            continue;
        }
        let Some((surfaces, scale)) = crate::world::load_figure(&art.jam, ride) else {
            continue;
        };
        // No two stand in the same place, whoever comes and goes.
        let taken = |spot: usize| {
            let at = STUDIO + Vec3::X * APART * spot as f32;
            portraits.0.iter().any(|portrait| portrait.at == at)
        };
        let spot = (0..).find(|&spot| !taken(spot)).unwrap_or(place);
        let at = STUDIO + Vec3::X * APART * spot as f32;
        // The game's models have X forward, Y left and Z up; ours face -Z with Y up.
        let basis = Quat::from_mat3(&Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y));
        let figure = commands
            .spawn((
                Sitter,
                Transform {
                    translation: at,
                    rotation: Quat::from_rotation_y(TURNED) * basis,
                    scale: Vec3::splat(scale * UNIT),
                },
                Visibility::default(),
            ))
            .with_children(|figure| {
                for surface in surfaces {
                    figure.spawn((
                        crate::world::surface_bundle(
                            surface,
                            &mut meshes,
                            &mut materials,
                            &mut images,
                        ),
                        RenderLayers::layer(LAYER),
                    ));
                }
            })
            .id();
        let size = (SIZE * DETAIL).as_uvec2();
        let picture = images.add(Image::new_target_texture(
            size.x,
            size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    // Before the screen's own camera, with nothing behind the figure.
                    order: -1,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::from(picture.clone()),
                RenderLayers::layer(LAYER),
                bevy::core_pipeline::tonemapping::Tonemapping::None,
                Transform::from_translation(at + Vec3::new(0.0, 0.75, -1.95))
                    .looking_at(at + Vec3::Y * 0.5, Vec3::Y),
            ))
            .id();
        portraits.0.push(Portrait {
            peer: *peer,
            ride: ride.clone(),
            picture,
            at,
            stage: [figure, camera],
        });
        changed = true;
    }
    if changed {
        menu.drawn = false;
    }
}

/// Clears the pictures away when the menus are left.
pub fn put_away(mut commands: Commands, mut portraits: ResMut<Portraits>) {
    for portrait in portraits.0.drain(..) {
        for entity in portrait.stage {
            commands.entity(entity).despawn();
        }
    }
}
