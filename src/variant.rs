//! A circuit the other way about: mirrored, as the original runs the races of its
//! fourth, fifth and sixth circuits, or reversed, which is the port's own.
//!
//! The original mirrors by negating Y in everything it loads (`GolWorldDatabase::MirrorY`,
//! and the `p_mirror` that each loader of `RaceSession` takes). This is a stand-in for
//! that: the world is loaded as it is, and the view of it flipped left to right, with
//! the steering, the sound and the map flipped to match. What is seen is the same but
//! for the cars and their drivers, which the original leaves unmirrored.
//!
//! Reversed, the racing line, the checkpoints and the lap zones are walked backwards
//! (`Track::reverse`) and the computer's cars drive for themselves, there being no
//! recordings of the circuit that way round.

use crate::championship::Championship;
use crate::menu::Settings;
use bevy::{
    camera::{CameraProjection, SubCameraView},
    math::Vec3A,
    prelude::*,
    render::render_resource::Face,
};

/// How the race in hand is run.
#[derive(Resource, Clone, Copy, Default, PartialEq, Debug)]
pub struct Variant {
    pub mirror: bool,
    pub reverse: bool,
}

/// What a mirrored race adds to the pace of the computer's cars (`g_mirroredRaceStateRouteScale`).
pub const MIRROR_BOOST: f32 = 0.05;

/// Circuits with a drop or a jump that can only be taken forwards: the computer's car
/// can't get round these backwards (`ai_laps_the_original_circuits_backwards`), and
/// they are always raced the right way.
pub const ONE_WAY: [&str; 2] = ["RACEC1R2", "RACEC1R3"];

impl Variant {
    /// A race of a circuit is mirrored when the game's tables say so; one on its own,
    /// as the settings have it. `folder` is the race's, if it is one of the original's.
    pub fn of(settings: &Settings, championship: &Championship, folder: Option<&str>) -> Self {
        match championship.mirrored() {
            Some(mirror) => Variant { mirror, reverse: false },
            None => Variant { mirror: settings.mirror, reverse: settings.reverse && !folder.is_some_and(|f| ONE_WAY.contains(&f)) },
        }
    }

    /// What tells this variant's records from another's.
    pub fn suffix(self) -> &'static str {
        match (self.mirror, self.reverse) {
            (false, false) => "",
            (true, false) => "-mirror",
            (false, true) => "-reverse",
            (true, true) => "-mirror-reverse",
        }
    }

    /// 1, or -1 for things that swap sides in the mirror.
    pub fn side(self) -> f32 {
        if self.mirror { -1.0 } else { 1.0 }
    }

    pub fn rubber_band_boost(self) -> f32 {
        if self.mirror { MIRROR_BOOST } else { 0.0 }
    }
}

/// A perspective view with left and right swapped.
#[derive(Debug, Clone)]
pub struct Flipped(pub PerspectiveProjection);

const FLIP: Vec3 = Vec3::new(-1.0, 1.0, 1.0);

impl CameraProjection for Flipped {
    fn get_clip_from_view(&self) -> Mat4 {
        Mat4::from_scale(FLIP) * self.0.get_clip_from_view()
    }

    fn get_clip_from_view_for_sub(&self, sub_view: &SubCameraView) -> Mat4 {
        Mat4::from_scale(FLIP) * self.0.get_clip_from_view_for_sub(sub_view)
    }

    fn update(&mut self, width: f32, height: f32) {
        self.0.update(width, height);
    }

    fn far(&self) -> f32 {
        self.0.far()
    }

    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        self.0.get_frustum_corners(z_near, z_far)
    }
}

/// The perspective a camera sees with, flipped or not.
pub fn lens(projection: &mut Projection) -> Option<&mut PerspectiveProjection> {
    match projection {
        Projection::Perspective(lens) => Some(lens),
        Projection::Custom(custom) => custom.get_mut::<Flipped>().map(|flipped| &mut flipped.0),
        Projection::Orthographic(_) => None,
    }
}

/// Flips the view for a mirrored race, and puts it back for any other.
pub fn set_view(variant: Res<Variant>, mut camera: Single<&mut Projection, With<Camera3d>>) {
    let flipped = matches!(**camera, Projection::Custom(_));
    let Some(lens) = lens(&mut camera).map(|lens| lens.clone()).filter(|_| flipped != variant.mirror) else { return };
    **camera = if variant.mirror { Projection::custom(Flipped(lens)) } else { Projection::Perspective(lens) };
}

/// A flipped view turns every face's winding round, so the faces that would be culled
/// are the ones facing the camera. In a mirrored race nothing is culled.
pub fn uncull(variant: Res<Variant>, mut materials: ResMut<Assets<StandardMaterial>>) {
    if !variant.mirror {
        return;
    }
    let culled: Vec<_> = materials.iter().filter(|(_, m)| m.cull_mode == Some(Face::Back)).map(|(id, _)| id).collect();
    for id in culled {
        if let Some(mut material) = materials.get_mut(id) {
            material.cull_mode = None;
        }
    }
}

#[cfg(test)]
#[test]
fn a_flipped_view_swaps_left_and_right_and_nothing_else() {
    let lens = PerspectiveProjection::default();
    let (plain, flipped) = (lens.get_clip_from_view(), Flipped(lens.clone()).get_clip_from_view());
    let point = Vec4::new(3.0, 2.0, -10.0, 1.0);
    let (seen, mirrored) = (plain * point, flipped * point);
    assert_eq!(mirrored, Vec4::new(-seen.x, seen.y, seen.z, seen.w));
    // The lens underneath can still be reached, to widen it for a turbo.
    let mut projection = Projection::custom(Flipped(lens));
    self::lens(&mut projection).unwrap().fov = 1.5;
    assert_eq!(self::lens(&mut projection).unwrap().fov, 1.5);
}
