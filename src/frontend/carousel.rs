//! The bricks of the part set on offer, in a row on the page bricks are put on the car
//! with: the one held in the middle and larger, two to either side of it, each
//! turning about a slanted axis. After `CarPartCarousel` (`RefreshItemModel`, which
//! builds each choice's brick; `LayoutItem`, which turns it; `SetSelection`, which
//! has the ring wrap round only when there are bricks enough to fill it) over
//! `MenuModelCarousel` (`CreateItems`, `DestroyItems`, `GetItemPosition`: each brick
//! fitted to its slot by the size of its bounds), and the slot rectangles and
//! speed of `piecesel` in `CARBUILD.MIB`: seven slots, the fourth in focus, the first and last
//! outside the widget, for the row to slide through.
//!
//! The original has a camera of its own for the row, a long way off and seeing very
//! narrowly (5 degrees); here a camera that looks straight on, with no perspective,
//! draws it onto a picture, as the driver page's parts are (`parts`). The row does
//! not slide from one brick to the next as the original's does: it is at once as it
//! will be.

use super::{Art, Menu, Page, workshop};
use crate::assets::leb;
use crate::build::{Car, Palette};
use bevy::camera::primitives::MeshAabb;
use bevy::{
    camera::{OrthographicProjection, RenderTarget, ScalingMode, visibility::RenderLayers},
    prelude::*,
    render::render_resource::TextureFormat,
};

/// `piecesel`: where the row is in its selector, and how big it is.
const FROM_LEFT: f32 = 32.0;
const SIZE: Vec2 = Vec2::new(356.0, 55.0);
/// The seven slots (left, top, right, bottom); the fourth is the one held.
const SLOTS: [[f32; 4]; 7] = [
    [-70.0, 10.0, -10.0, 55.0],
    [0.0, 10.0, 60.0, 55.0],
    [70.0, 10.0, 130.0, 55.0],
    [140.0, 0.0, 200.0, 65.0],
    [210.0, 10.0, 280.0, 55.0],
    [290.0, 10.0, 350.0, 55.0],
    [360.0, 10.0, 410.0, 55.0],
];
const FOCUSED: usize = 3;
/// `piecesel`'s turn, in radians a millisecond.
const SCROLL_STEP: f32 = 0.002;
/// `CarPartCarousel::LayoutItem`: the way up of a brick, as cosines of
/// the table's 1024 steps of a circle (`c_vectorXCosineIndex`, `c_vectorZCosineIndex`),
/// in the game's axes.
const SLANT: [usize; 2] = [882, 114];
const DETAIL: f32 = 3.0;
const LAYER: usize = 12;
/// Where the studio is: far under anything else.
const STUDIO: Vec3 = Vec3::new(0.0, -3000.0, 0.0);

/// What is in each slot, by the bricks' pieces and colours.
type Ring = [Option<(u16, u8)>; 7];

#[derive(Resource, Default)]
pub struct Bricks {
    pub picture: Option<Handle<Image>>,
    shown: Option<Ring>,
    camera: Option<Entity>,
    stage: Vec<Entity>,
}

impl Bricks {
    /// The row's picture and where it goes on the screen.
    pub fn picture(&self, art: &Art) -> Option<(Handle<Image>, Rect)> {
        let place = art.place("carbuild", "pieces");
        let at = Vec2::new(place.min.x + FROM_LEFT, place.min.y);
        Some((self.picture.clone()?, Rect::from_corners(at, at + SIZE)))
    }
}

/// A brick in a slot: where the middle of the slot is in the picture, how large its
/// brick is made, and where the middle of the brick is, in the game's units.
#[derive(Component)]
pub struct Slot {
    middle: Vec2,
    scale: f32,
    centre: Vec3,
}

/// The choices of the slots round the one held: those there are, wrapping round
/// when there are enough to fill the ring but one.
fn ring(choices: &[(u16, u8)], at: usize) -> Ring {
    let n = choices.len() as i32;
    std::array::from_fn(|slot| {
        let wanted = at as i32 + slot as i32 - FOCUSED as i32;
        let wrapped = if n >= SLOTS.len() as i32 - 1 {
            wanted.rem_euclid(n)
        } else {
            wanted
        };
        (0..n).contains(&wrapped).then(|| choices[wrapped as usize])
    })
}

/// `g_cosineTable`: a circle of 1024 steps.
fn cosine(step: usize) -> f32 {
    (std::f32::consts::TAU * step as f32 / 1024.0).cos()
}

/// How a brick is turned `angle` radians, in the game's axes: its up is the slanted
/// one, and what was its front is turned about that (`GolMath::RotateAboutAxis` on
/// a front of `(1, 0, 0)`, then `SetUpDirection`).
fn turned(angle: f32) -> Quat {
    let up = Vec3::new(cosine(SLANT[0]), 0.0, cosine(SLANT[1])).normalize();
    let front = Quat::from_axis_angle(up, angle) * Vec3::X;
    let front = (front - up * front.dot(up)).normalize();
    let left = up.cross(front);
    Quat::from_mat3(&Mat3::from_cols(front, left, up))
}

/// The game's axes (X toward the camera, Y to its right, Z up) as the picture's.
fn basis() -> Quat {
    Quat::from_mat3(&Mat3::from_cols(Vec3::Z, Vec3::X, Vec3::Y))
}

fn clear(commands: &mut Commands, bricks: &mut Bricks) {
    for entity in bricks.stage.drain(..).chain(bricks.camera.take()) {
        commands.entity(entity).despawn();
    }
    (bricks.picture, bricks.shown) = (None, None);
}

/// Keeps the picture of the row of bricks while the page for placing them is shown.
pub fn keep(
    mut commands: Commands,
    art: Res<Art>,
    bench: Res<workshop::Bench>,
    mut menu: ResMut<Menu>,
    mut bricks: ResMut<Bricks>,
    (mut meshes, mut materials, mut images): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
    ),
) {
    if menu.page != Page::Bricks {
        if bricks.camera.is_some() {
            clear(&mut commands, &mut bricks);
        }
        return;
    }
    let Some((library, choices, held)) = workshop::bricks(&bench) else {
        return;
    };
    let wanted = ring(choices, held);
    if bricks.shown == Some(wanted) {
        return;
    }
    let Some(palette) = Palette::open(art.jam(), true) else {
        return;
    };
    let files = Palette::files(true);
    let surfaces = crate::world::Library::new(art.jam(), files.iter().map(String::as_str), &[leb::DIR]);
    if bricks.camera.is_none() {
        let size = (SIZE * DETAIL).as_uvec2();
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
                    order: -5,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                Projection::Orthographic(lens),
                RenderTarget::from(picture.clone()),
                RenderLayers::layer(LAYER),
                bevy::core_pipeline::tonemapping::Tonemapping::None,
                Transform::from_translation(STUDIO + Vec3::Z * 10.0),
            ))
            .id();
        (bricks.picture, bricks.camera) = (Some(picture), Some(camera));
    }
    for entity in bricks.stage.drain(..) {
        commands.entity(entity).despawn();
    }
    for (slot, choice) in wanted.iter().enumerate() {
        let Some(&(kind, colour)) = choice.as_ref() else {
            continue;
        };
        let model = Car::piece_model(library, &palette, kind, colour);
        let bundles: Vec<_> = surfaces
            .surfaces(&model, |_| true, Vec3::from)
            .into_iter()
            .map(|surface| {
                crate::world::surface_bundle(surface, &mut meshes, &mut materials, &mut images)
            })
            .collect();
        // The brick's bounds, in the game's own units, to fit it to its slot.
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
        let [left, top, right, bottom] = SLOTS[slot];
        // `MenuModelCarousel::DestroyItems`: it is fitted by the smaller side of its slot.
        let fit = (right - left).min(bottom - top);
        let diameter = (high - low).length().max(1e-3);
        let middle = Vec2::new(
            (left + right) / 2.0 - SIZE.x / 2.0,
            SIZE.y / 2.0 - (top + bottom) / 2.0,
        );
        let holder = commands
            .spawn((
                Slot {
                    middle,
                    scale: fit / diameter,
                    centre: (low + high) / 2.0,
                },
                Transform::from_translation(STUDIO),
                Visibility::default(),
            ))
            .with_children(|holder| {
                for bundle in bundles {
                    holder.spawn((bundle, RenderLayers::layer(LAYER)));
                }
            })
            .id();
        bricks.stage.push(holder);
    }
    bricks.shown = Some(wanted);
    menu.drawn = false;
}

/// Turns the bricks, which `CarPartCarousel::OnEvent` does as time goes by.
pub fn spin(time: Res<Time<Real>>, mut slots: Query<(&Slot, &mut Transform)>) {
    let angle = (time.elapsed_secs() * 1000.0 * SCROLL_STEP).rem_euclid(std::f32::consts::TAU);
    let rotation = basis() * turned(angle);
    for (slot, mut transform) in &mut slots {
        transform.rotation = rotation;
        transform.scale = Vec3::splat(slot.scale);
        // Turned about the middle of the brick, which is set where its slot's is.
        transform.translation =
            STUDIO + slot.middle.extend(0.0) - rotation * slot.centre * slot.scale;
    }
}

/// Clears the row away when the menus are left.
pub fn put_away(mut commands: Commands, mut bricks: ResMut<Bricks>) {
    clear(&mut commands, &mut bricks);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_wraps_only_when_it_is_nearly_full() {
        let choices: Vec<(u16, u8)> = (0..15).map(|n| (n, 0)).collect();
        let at_first = ring(&choices, 0);
        assert_eq!(at_first[FOCUSED], Some((0, 0)));
        assert_eq!(at_first[0], Some((12, 0)));
        let few: Vec<(u16, u8)> = (0..4).map(|n| (n, 0)).collect();
        let at_first = ring(&few, 0);
        assert_eq!(at_first[..FOCUSED], [None; 3]);
        assert_eq!(at_first[FOCUSED + 1], Some((1, 0)));
        assert_eq!(at_first[FOCUSED + 3], Some((3, 0)));
    }

    #[test]
    fn a_brick_is_slanted_toward_the_camera() {
        let up = turned(0.0) * Vec3::Z;
        assert!((up.x - 0.643).abs() < 0.01 && (up.z - 0.766).abs() < 0.01, "{up}");
        // Turning it leaves its way up as it was.
        assert!((turned(2.0) * Vec3::Z - up).length() < 1e-4);
    }
}
