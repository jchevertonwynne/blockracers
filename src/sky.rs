//! The sky, as `RaceSkyState`: a dome round the camera shaded from the three colours
//! of the circuit's `.SKB`, and in front of it the models of the circuit's sky world
//! (`BACKGRD.WDB`: stars, clouds and the like), which also go where the camera goes.
//!
//! The original draws both first with the depth buffer off. Here they are drawn many
//! times their size instead, which from the middle looks the same and leaves them
//! behind everything else. The dome is open at the top in the original; here the hole
//! is closed with the colour of its rim. Only the state a sky starts in is shown: the
//! changes of state that events ask for (`StartTransition`) are not followed.

use crate::assets::tokens::{Token, tokenize};
use crate::physics::UNIT;
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::Indices,
    prelude::*,
    render::render_resource::PrimitiveTopology,
};
use std::f32::consts::{FRAC_PI_2, TAU};

/// The dome: how many sides it has, and how big it is in the game's units.
const SEGMENTS: usize = 11;
const RADIUS: f32 = 100.0;
/// The dome's middle is this far below the camera, and the sky world's this far above
/// the dome's (`g_unk0x004afde0`, `g_raceSkyDomeDepth`), less the sky's own offset.
const DOME_DROP: f32 = 10.0;
const WORLD_RISE: f32 = 40.0;
/// How many times their size the dome and the sky world are drawn.
const DOME_SCALE: f32 = 100.0;
pub const WORLD_SCALE: f32 = 30.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sky {
    /// The colours of the dome's rings, from the horizon up. The file calls the
    /// horizon's the top colour, which is the order `ApplyColors` hands them out in.
    pub rings: [[u8; 3]; 3],
    pub height_offset: f32,
}

impl Sky {
    pub fn parse(data: &[u8]) -> Option<Sky> {
        let tokens = tokenize(data);
        // The state to start in is named after the states; without one it is the first.
        let named = tokens.iter().rposition(|t| *t == Token::Key(0x2d)).and_then(|at| tokens.get(at + 1));
        let start = named
            .and_then(|name| tokens.iter().position(|t| t == name))
            .or_else(|| tokens.iter().position(|t| matches!(t, Token::Str(_))))?;
        let colour = |key: u16| {
            let at = start + tokens[start..].iter().position(|t| *t == Token::Key(key))?;
            let part = |n: usize| match tokens.get(at + 1 + n) {
                Some(Token::Int(v)) => Some(*v as u8),
                _ => None,
            };
            Some([part(0)?, part(1)?, part(2)?])
        };
        let height_offset = tokens.iter().position(|t| *t == Token::Key(0x2e)).and_then(|at| match tokens.get(at + 1) {
            Some(Token::Float(v)) => Some(*v),
            Some(Token::Int(v)) => Some(*v as f32),
            _ => None,
        });
        Some(Sky { rings: [colour(0x29)?, colour(0x2a)?, colour(0x2b)?], height_offset: height_offset.unwrap_or(0.0) })
    }

    /// What shows below the dome's rim.
    pub fn ground(&self) -> Color {
        let [r, g, b] = self.rings[0];
        Color::srgb_u8(r, g, b)
    }

    /// The dome of `ModelBuilder::BuildSphere`: three rings of eleven, the lowest on
    /// the horizon, with Y up.
    fn dome(&self) -> Mesh {
        let step = TAU / SEGMENTS as f32;
        let linear = |[r, g, b]: [u8; 3]| [r, g, b].map(|c| (c as f32 / 255.0).powf(2.2));
        let (mut positions, mut colours) = (Vec::new(), Vec::new());
        // From the highest ring down; the last is put on the horizon whatever the step.
        for ring in 0..3 {
            let down = if ring == 2 { FRAC_PI_2 } else { step * (ring + 1) as f32 };
            let [r, g, b] = linear(self.rings[2 - ring]);
            for segment in 0..SEGMENTS {
                let round = step * segment as f32;
                positions.push([round.cos() * down.sin() * RADIUS, down.cos() * RADIUS, -round.sin() * down.sin() * RADIUS]);
                colours.push([r, g, b, 1.0]);
            }
        }
        let [r, g, b] = linear(self.rings[2]);
        positions.push([0.0, RADIUS, 0.0]);
        colours.push([r, g, b, 1.0]);
        let top = (positions.len() - 1) as u32;
        let mut indices = Vec::new();
        for segment in 0..SEGMENTS as u32 {
            let next = (segment + 1) % SEGMENTS as u32;
            indices.extend([top, segment, next]);
            for ring in 0..2 {
                let (upper, lower) = (ring * SEGMENTS as u32, (ring + 1) * SEGMENTS as u32);
                indices.extend([upper + segment, lower + segment, lower + next, upper + segment, lower + next, upper + next]);
            }
        }
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
            .with_inserted_indices(Indices::U32(indices))
            .with_computed_normals()
    }
}

/// The dome, and how far below the camera its middle is kept.
#[derive(Component)]
pub struct Dome(f32);

/// A model of the sky world.
#[derive(Component)]
pub struct Backdrop;

pub fn spawn(
    mut commands: Commands,
    world: Res<crate::world::LoadedWorld>,
    mut clear: ResMut<ClearColor>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(sky) = world.sky else {
        clear.0 = crate::SKY;
        return;
    };
    clear.0 = sky.ground();
    let material = StandardMaterial { unlit: true, cull_mode: None, ..default() };
    commands.spawn((
        Dome((DOME_DROP - sky.height_offset) * UNIT * DOME_SCALE),
        Mesh3d(meshes.add(sky.dome())),
        MeshMaterial3d(materials.add(material)),
        Transform::from_scale(Vec3::splat(UNIT * DOME_SCALE)),
        NotShadowCaster,
    ));
}

/// Keeps the sky round the camera.
pub fn follow(
    camera: Single<&Transform, With<Camera3d>>,
    mut dome: Query<(&Dome, &mut Transform), (Without<Camera3d>, Without<Backdrop>)>,
    mut backdrop: Query<&mut Transform, (With<Backdrop>, Without<Camera3d>, Without<Dome>)>,
) {
    for (dome, mut transform) in &mut dome {
        transform.translation = camera.translation - Vec3::Y * dome.0;
    }
    // The sky world sits a fixed way above the dome's middle, whatever the sky's offset.
    for mut transform in &mut backdrop {
        transform.translation = camera.translation + Vec3::Y * (WORLD_RISE - DOME_DROP) * UNIT * WORLD_SCALE;
    }
}

#[cfg(test)]
#[test]
fn every_circuit_has_a_sky() {
    let Some(jam) = crate::assets::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else { return };
    let sky = |race: &str| Sky::parse(jam.get(&format!("/GAMEDATA/{race}/BACKGRND.SKB")).unwrap()).unwrap();
    for (race, _) in crate::world::circuits() {
        sky(&race);
    }
    // A yellow horizon under deep blue at the castle; black all the way up in the forest.
    assert_eq!(sky("RACEC0R0"), Sky { rings: [[250, 250, 120], [100, 100, 255], [25, 25, 255]], height_offset: 0.0 });
    assert_eq!(sky("RACEC1R0").rings, [[0; 3]; 3]);
    // The moon starts in the open air, not in its flash, and some skies sit lower.
    assert_eq!(sky("RACEC0R3").rings[0], [250, 60, 0]);
    assert_eq!(sky("RACEC2R1").height_offset, -30.0);
    // The dome: three rings and the cap, the lowest ring level with its middle.
    let mesh = sky("RACEC0R0").dome();
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
    assert_eq!(positions.len(), 3 * SEGMENTS + 1);
    assert!(positions[2 * SEGMENTS][1].abs() < 1e-3 && (positions[2 * SEGMENTS][0] - RADIUS).abs() < 1e-3);
    assert!((positions[0][1] - RADIUS * (TAU / 11.0).cos()).abs() < 1e-3);
}
