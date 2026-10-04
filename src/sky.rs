//! The sky, as `RaceSkyState`: a dome round the camera shaded from the three colours
//! of the circuit's `.SKB`, and in front of it the models of the circuit's sky world
//! (`BACKGRD.WDB`: stars, clouds and the like), which also go where the camera goes.
//!
//! The original draws both first with the depth buffer off. Here they are drawn many
//! times their size instead, which from the middle looks the same and leaves them
//! behind everything else. The dome is open at the top in the original; here the hole
//! is closed with the colour of its rim.
//!
//! A sky has one or more states, each with its own colours, and events of the circuit
//! move it from one to another over a time (`SkyStateResource`, `StartTransition`), or
//! hide and show the dome and the sky world. A state's colours are its first key's:
//! no sky in the game has a state with more than one.

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

/// The colours of the dome's rings, from the horizon up. The file calls the horizon's
/// the top colour, which is the order `ApplyColors` hands them out in.
type Rings = [[u8; 3]; 3];

#[derive(Clone, PartialEq, Debug)]
pub struct Sky {
    /// Each state's name and colours.
    pub states: Vec<(String, Rings)>,
    /// The state it starts in.
    pub initial: usize,
    pub height_offset: f32,
}

/// What an event of the circuit does to the sky.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub event: i32,
    /// Made when the event ends rather than when it starts.
    pub on_end: bool,
    /// The state to move to, and over how long.
    pub state: Option<String>,
    pub seconds: f32,
    /// Whether the dome and the sky world are to be shown, where the event says.
    pub dome: Option<bool>,
    pub world: Option<bool>,
}

/// The sky as it stands in the race.
#[derive(Resource)]
pub struct Showing {
    sky: Sky,
    state: usize,
    /// The state being left, how long the move has gone on and how long it takes.
    previous: usize,
    moving: Option<(f32, f32)>,
    dome: bool,
    world: bool,
    /// The colours last put on the dome.
    shown: Option<Rings>,
}

impl Showing {
    fn new(sky: Sky) -> Self {
        let state = sky.initial;
        Showing { sky, state, previous: state, moving: None, dome: true, world: true, shown: None }
    }

    fn apply(&mut self, change: &Change) {
        let named = change.state.as_ref().and_then(|name| self.sky.states.iter().position(|s| s.0 == *name));
        if let Some(state) = named.filter(|&state| state != self.state) {
            (self.previous, self.state) = (self.state, state);
            self.moving = (change.seconds > 0.0).then_some((0.0, change.seconds));
        }
        (self.dome, self.world) = (change.dome.unwrap_or(self.dome), change.world.unwrap_or(self.world));
    }

    /// The colours for now: the state's, or part way to them from the last state's.
    fn rings(&self) -> Rings {
        let to = self.sky.states[self.state].1;
        let Some((elapsed, length)) = self.moving else { return to };
        let (from, along) = (self.sky.states[self.previous].1, (elapsed / length).clamp(0.0, 1.0));
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * along) as u8;
        [0, 1, 2].map(|ring| [0, 1, 2].map(|part| mix(from[ring][part], to[ring][part])))
    }
}

impl Sky {
    pub fn parse(data: &[u8]) -> Option<Sky> {
        let tokens = tokenize(data);
        let colour = |start: usize, key: u16| {
            let at = start + tokens[start..].iter().position(|t| *t == Token::Key(key))?;
            let part = |n: usize| match tokens.get(at + 1 + n) {
                Some(Token::Int(v)) => Some(*v as u8),
                _ => None,
            };
            Some([part(0)?, part(1)?, part(2)?])
        };
        // Each state is `0x27 [keys] "name" { 0x27 { colours } }`.
        let mut states = Vec::new();
        for (at, token) in tokens.iter().enumerate() {
            if let (Token::Str(name), Some(Token::LCurly)) = (token, tokens.get(at + 1)) {
                states.push((name.to_lowercase(), [colour(at, 0x29)?, colour(at, 0x2a)?, colour(at, 0x2b)?]));
            }
        }
        // The state to start in is named after the states; without one it is the first.
        let named = tokens.iter().rposition(|t| *t == Token::Key(0x2d)).and_then(|at| match tokens.get(at + 1) {
            Some(Token::Str(name)) => Some(name.to_lowercase()),
            _ => None,
        });
        let initial = named.and_then(|name| states.iter().position(|s| s.0 == name)).unwrap_or(0);
        if states.is_empty() {
            return None;
        }
        let height_offset = tokens.iter().position(|t| *t == Token::Key(0x2e)).and_then(|at| match tokens.get(at + 1) {
            Some(Token::Float(v)) => Some(*v),
            Some(Token::Int(v)) => Some(*v as f32),
            _ => None,
        });
        Some(Sky { states, initial, height_offset: height_offset.unwrap_or(0.0) })
    }
}

/// What shows below the dome's rim.
fn ground(rings: Rings) -> Color {
    let [r, g, b] = rings[0];
    Color::srgb_u8(r, g, b)
}

/// The colours of the dome's corners, in the order `dome` makes them: the highest ring
/// first, then the cap.
fn colours(rings: Rings) -> Vec<[f32; 4]> {
    let linear = |[r, g, b]: [u8; 3]| [(r as f32 / 255.0).powf(2.2), (g as f32 / 255.0).powf(2.2), (b as f32 / 255.0).powf(2.2), 1.0];
    let mut out: Vec<[f32; 4]> = (0..3).flat_map(|ring| [linear(rings[2 - ring]); SEGMENTS]).collect();
    out.push(linear(rings[2]));
    out
}

impl Sky {
    /// The dome of `ModelBuilder::BuildSphere`: three rings of eleven, the lowest on
    /// the horizon, with Y up.
    fn dome(rings: Rings) -> Mesh {
        let step = TAU / SEGMENTS as f32;
        let mut positions = Vec::new();
        // From the highest ring down; the last is put on the horizon whatever the step.
        for ring in 0..3 {
            let down = if ring == 2 { FRAC_PI_2 } else { step * (ring + 1) as f32 };
            for segment in 0..SEGMENTS {
                let round = step * segment as f32;
                positions.push([round.cos() * down.sin() * RADIUS, down.cos() * RADIUS, -round.sin() * down.sin() * RADIUS]);
            }
        }
        positions.push([0.0, RADIUS, 0.0]);
        let colours = colours(rings);
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
    let Some(sky) = world.sky.clone() else {
        clear.0 = crate::SKY;
        commands.remove_resource::<Showing>();
        return;
    };
    let rings = sky.states[sky.initial].1;
    clear.0 = ground(rings);
    let material = StandardMaterial { unlit: true, cull_mode: None, ..default() };
    commands.spawn((
        Dome((DOME_DROP - sky.height_offset) * UNIT * DOME_SCALE),
        Mesh3d(meshes.add(Sky::dome(rings))),
        MeshMaterial3d(materials.add(material)),
        Transform::from_scale(Vec3::splat(UNIT * DOME_SCALE)),
        NotShadowCaster,
    ));
    commands.insert_resource(Showing { shown: Some(rings), ..Showing::new(sky) });
}

/// Moves the sky between its states as the circuit's events ask.
pub fn change(
    time: Res<Time>,
    race: Res<crate::Race>,
    showing: Option<ResMut<Showing>>,
    events: Option<ResMut<crate::events::TrackEvents>>,
    mut clear: ResMut<ClearColor>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut dome: Query<(&Mesh3d, &mut Visibility), (With<Dome>, Without<Backdrop>)>,
    mut backdrop: Query<&mut Visibility, (With<Backdrop>, Without<Dome>)>,
) {
    let Some(mut showing) = showing else { return };
    let asked = events.map(|mut events| std::mem::take(&mut events.sky)).unwrap_or_default();
    if race.phase == crate::Phase::Intro {
        // A new race, or a restart, has the sky it began with.
        *showing = Showing { shown: showing.shown, ..Showing::new(showing.sky.clone()) };
    } else {
        for change in &asked {
            showing.apply(change);
        }
    }
    if let Some((elapsed, length)) = &mut showing.moving {
        *elapsed += time.delta_secs();
        if *elapsed >= *length {
            showing.moving = None;
        }
    }
    let rings = showing.rings();
    let shown = |on: bool| if on { Visibility::Inherited } else { Visibility::Hidden };
    for (mesh, mut visibility) in &mut dome {
        // With the dome hidden nothing is drawn at all, and the sky world goes with it.
        visibility.set_if_neq(shown(showing.dome));
        if showing.shown != Some(rings)
            && let Some(mut mesh) = meshes.get_mut(&mesh.0) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours(rings));
            }
    }
    for mut visibility in &mut backdrop {
        visibility.set_if_neq(shown(showing.dome && showing.world));
    }
    if showing.shown != Some(rings) {
        clear.0 = ground(rings);
        showing.shown = Some(rings);
    }
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
    // A yellow horizon under deep blue at the castle, and a darker sky inside it;
    // black all the way up in the forest.
    let castle = sky("RACEC0R0");
    assert_eq!(castle.states[0], ("openair".to_string(), [[250, 250, 120], [100, 100, 255], [25, 25, 255]]));
    assert_eq!(castle.states[1], ("castle".to_string(), [[0, 0, 180], [0, 0, 40], [0, 0, 0]]));
    assert_eq!((castle.states.len(), castle.initial, castle.height_offset), (2, 0, 0.0));
    assert_eq!(sky("RACEC1R0").states, [("openair".to_string(), [[0; 3]; 3])]);
    // The moon starts in the open air, not in its flash, and some skies sit lower.
    let moon = sky("RACEC0R3");
    assert_eq!((moon.states[moon.initial].1[0], moon.states[1].0.as_str()), ([250, 60, 0], "flash"));
    assert_eq!(sky("RACEC2R1").height_offset, -30.0);
    // Asked into the castle over two seconds, the sky is half way there after one.
    let mut showing = Showing::new(castle.clone());
    showing.apply(&Change { event: 1, on_end: false, state: Some("castle".into()), seconds: 2.0, dome: None, world: Some(false) });
    showing.moving = Some((1.0, 2.0));
    assert_eq!((showing.rings()[0], showing.world, showing.dome), ([125, 125, 150], false, true));
    showing.moving = None;
    assert_eq!(showing.rings(), castle.states[1].1);
    // The dome: three rings and the cap, the lowest ring level with its middle.
    let mesh = Sky::dome(castle.states[0].1);
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
    assert_eq!(positions.len(), 3 * SEGMENTS + 1);
    assert!(positions[2 * SEGMENTS][1].abs() < 1e-3 && (positions[2 * SEGMENTS][0] - RADIUS).abs() < 1e-3);
    assert!((positions[0][1] - RADIUS * (TAU / 11.0).cos()).abs() < 1e-3);
}
