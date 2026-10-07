//! A minifigure's parts in the driver page's four selectors: the hat, the face, the
//! torso and the legs each shown by itself, the one worn in the middle and larger
//! and those on either side of it in the slots round it. After
//! `MenuRacerCarousel` (`CollectItems`, `RefreshItemModel`: the model of each
//! choice, with its own material) and the slot rectangles of `partbox` and
//! `headbox` in `EDITDRVR.MIB`, which are laid out by `MenuModelCarousel`.
//!
//! The original does not turn these (`MenuModelCarousel::LayoutItem` does nothing
//! for the racer carousel; only the car part carousel and the car on its own
//! screen spin), so they stand face on. Each selector is drawn by a camera of its
//! own onto a picture, as the main menu's figure is (`mascot`); the original fits
//! each model to its slot through a perspective camera, and here a camera that
//! looks straight on does the same.

use super::{Art, Menu, Page, workshop};
use crate::assets::lrs::Cosmetics;
use crate::garage::Garage;
use crate::progress::Progress;
use bevy::camera::primitives::MeshAabb;
use bevy::{
    camera::{OrthographicProjection, RenderTarget, ScalingMode, visibility::RenderLayers},
    prelude::*,
    render::render_resource::TextureFormat,
};

const SELECTORS: [&str; 4] = ["hatsel", "facesel", "torsosel", "legsel"];
/// Where the carousel is in its selector, and how big: `partbox` and `headbox`.
const FROM_LEFT: f32 = 32.0;
const SIZE: Vec2 = Vec2::new(200.0, 64.0);
/// The five slots (left, top, right, bottom) of the carousel for a hat, torso and
/// legs, and for a face; the third is the one worn.
const SLOTS: [[f32; 4]; 5] = [
    [-40.0, 8.0, -5.0, 64.0],
    [5.0, 8.0, 60.0, 64.0],
    [65.0, 0.0, 140.0, 72.0],
    [145.0, 8.0, 200.0, 64.0],
    [205.0, 8.0, 240.0, 64.0],
];
const FACE_SLOTS: [[f32; 4]; 5] = [
    [-40.0, 12.0, -5.0, 60.0],
    [5.0, 12.0, 60.0, 60.0],
    [65.0, 8.0, 140.0, 64.0],
    [145.0, 12.0, 200.0, 60.0],
    [205.0, 12.0, 240.0, 60.0],
];
/// How much of its slot a part fills.
const FILL: f32 = 0.9;
const DETAIL: f32 = 3.0;
const LAYER: usize = 10;
/// Where the studios are: far under anything else, this far apart.
const STUDIO: Vec3 = Vec3::new(0.0, -2000.0, 0.0);
const APART: f32 = 400.0;

struct Selector {
    /// What it shows: the choice at each slot and the minifigure's other parts.
    shown: ([Option<u8>; 5], Cosmetics),
    picture: Handle<Image>,
    stage: Vec<Entity>,
    camera: Entity,
}

#[derive(Resource, Default)]
pub struct Parts(Vec<Selector>);

impl Parts {
    /// The pictures of the four selectors, and where each goes on the screen.
    pub fn pictures(&self, art: &Art) -> Vec<(Handle<Image>, Rect)> {
        self.0
            .iter()
            .zip(SELECTORS)
            .map(|(selector, name)| {
                let place = art.place("editdrvr", name);
                let at = Vec2::new(place.min.x + FROM_LEFT, place.min.y);
                (selector.picture.clone(), Rect::from_corners(at, at + SIZE))
            })
            .collect()
    }
}

/// Marks what a part is drawn of, and where it is to be centred.
#[derive(Component)]
pub struct Shown;

/// The choices of the slots round the one worn: those there are, wrapping round
/// when there are enough to.
fn ring(open: &[u8], at: usize) -> [Option<u8>; 5] {
    let n = open.len() as i32;
    [0, 1, 2, 3, 4].map(|slot| {
        let wanted = at as i32 + slot - 2;
        let wrapped = if n >= 4 { wanted.rem_euclid(n) } else { wanted };
        (0..n).contains(&wrapped).then(|| open[wrapped as usize])
    })
}

fn clear(commands: &mut Commands, parts: &mut Parts) {
    for selector in parts.0.drain(..) {
        for entity in selector.stage {
            commands.entity(entity).despawn();
        }
        commands.entity(selector.camera).despawn();
    }
}

/// Keeps a picture of each part's choices while the driver page is shown.
pub fn keep(
    mut commands: Commands,
    art: Res<Art>,
    garage: Res<Garage>,
    progress: Res<Progress>,
    bench: Res<workshop::Bench>,
    mut menu: ResMut<Menu>,
    mut parts: ResMut<Parts>,
    (mut meshes, mut materials, mut images): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
    ),
) {
    if menu.page != Page::Driver {
        if !parts.0.is_empty() {
            clear(&mut commands, &mut parts);
        }
        return;
    }
    let (cosmetics, choices) = workshop::part_choices(&bench, &garage, &progress);
    if choices.iter().all(|(open, _)| open.is_empty()) {
        return;
    }
    let size = (SIZE * DETAIL).as_uvec2();
    for (part, (open, at)) in choices.iter().enumerate() {
        let shown = (ring(open, *at), cosmetics);
        if let Some(selector) = parts.0.get(part) {
            // Only the part's own choices matter to what is drawn of it.
            let same_parts = selector.shown.0 == shown.0
                && selector.shown.1.torso == cosmetics.torso
                && selector.shown.1.legs == cosmetics.legs;
            if same_parts {
                continue;
            }
        }
        let centre = STUDIO + Vec3::new(0.0, -APART * part as f32, 0.0);
        let (picture, camera) = match parts.0.get_mut(part) {
            Some(selector) => {
                for entity in selector.stage.drain(..) {
                    commands.entity(entity).despawn();
                }
                (selector.picture.clone(), selector.camera)
            }
            None => {
                let picture = images.add(Image::new_target_texture(
                    size.x,
                    size.y,
                    TextureFormat::Rgba8UnormSrgb,
                    None,
                ));
                let mut lens = OrthographicProjection::default_3d();
                lens.scaling_mode = ScalingMode::Fixed {
                    width: SIZE.x,
                    height: SIZE.y,
                };
                lens.near = -100.0;
                lens.far = 100.0;
                let camera = commands
                    .spawn((
                        Camera3d::default(),
                        Camera {
                            order: -4,
                            clear_color: ClearColorConfig::Custom(Color::NONE),
                            ..default()
                        },
                        Projection::Orthographic(lens),
                        RenderTarget::from(picture.clone()),
                        RenderLayers::layer(LAYER),
                        bevy::core_pipeline::tonemapping::Tonemapping::None,
                        // The picture's middle, looking at it face on.
                        Transform::from_translation(centre + Vec3::Z * 10.0),
                    ))
                    .id();
                (picture, camera)
            }
        };
        let slots = if part == 1 { &FACE_SLOTS } else { &SLOTS };
        let mut stage = Vec::new();
        for (slot, choice) in shown.0.iter().enumerate() {
            let Some(choice) = choice else { continue };
            let mut worn = cosmetics;
            [&mut worn.hat, &mut worn.face, &mut worn.torso, &mut worn.legs][part]
                .clone_from(choice);
            let Some(surfaces) = crate::world::figure_part(art.jam(), worn, part) else {
                continue;
            };
            let [left, top, right, bottom] = slots[slot];
            // Where the slot is, from the picture's middle, upwards being up.
            let middle = Vec2::new((left + right) / 2.0 - SIZE.x / 2.0, SIZE.y / 2.0 - (top + bottom) / 2.0);
            let fit = (right - left).min(bottom - top) * FILL;
            let bundles: Vec<_> = surfaces
                .into_iter()
                .map(|surface| {
                    crate::world::surface_bundle(surface, &mut meshes, &mut materials, &mut images)
                })
                .collect();
            // The part's bounds, in the game's own units, to fit it to the slot.
            let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for (mesh, _) in &bundles {
                if let Some(aabb) = meshes.get(&mesh.0).and_then(|mesh| mesh.compute_aabb()) {
                    low = low.min(Vec3::from(aabb.center - aabb.half_extents));
                    high = high.max(Vec3::from(aabb.center + aabb.half_extents));
                }
            }
            if low.x > high.x {
                continue;
            }
            let diameter = (high - low).length().max(1e-3);
            let scale = fit / diameter;
            // Game axes: X forward, Y left, Z up; seen from the front, upright.
            let basis = Quat::from_mat3(&Mat3::from_cols(Vec3::Z, Vec3::X, Vec3::Y));
            let middle_of_part = (low + high) / 2.0;
            let holder = commands
                .spawn((
                    Shown,
                    Transform {
                        translation: centre + middle.extend(0.0) - basis * middle_of_part * scale,
                        rotation: basis,
                        scale: Vec3::splat(scale),
                    },
                    Visibility::default(),
                ))
                .with_children(|holder| {
                    for bundle in bundles {
                        holder.spawn((bundle, RenderLayers::layer(LAYER)));
                    }
                })
                .id();
            stage.push(holder);
        }
        let selector = Selector {
            shown,
            picture,
            stage,
            camera,
        };
        if part < parts.0.len() {
            parts.0[part] = selector;
        } else {
            parts.0.push(selector);
        }
        menu.drawn = false;
    }
}

/// Clears the pictures away when the menus are left.
pub fn put_away(mut commands: Commands, mut parts: ResMut<Parts>) {
    clear(&mut commands, &mut parts);
}
