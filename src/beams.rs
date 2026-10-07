//! The ribbons power-ups and hazards draw: the lightning wand's bolt, the grappling
//! hook's rope (`BeamMesh`, with `LightningAction` and `TetherProjectile`) and the
//! streaks behind missiles and cannon balls (`RaceTrailManager::Trail`).
//!
//! They are drawn from the `Action`s in the world and so are seen in replays and by
//! the players of an online race just as they are by the host: each is made afresh
//! from the action it follows.

use crate::collision::Hit;
use crate::items::Action;
use crate::kart::Kart;
use crate::menu::Screen;
use crate::physics::UNIT;
use crate::scenery::Swatches;
use crate::track::Track;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

type Rgba = [u8; 4];

/// `BeamMesh::g_beamMinStepDistanceSquared`: shorter spans are left out.
const MIN_STEP_SQUARED: f32 = 0.02 * UNIT * UNIT;

const LIGHTNING_COLOURS: [Rgba; 3] = [[0x19, 0x41, 0xf5, 255], [0x19, 0x41, 0xff, 255], [0x19, 0x41, 0xeb, 255]];
const LIGHTNING_HIT: [Rgba; 3] = [[255; 4]; 3];
const LIGHTNING_THICKNESS: f32 = 0.85;
const LIGHTNING_AMPLITUDE: f32 = 9.0;
/// How far a step of the bolt goes when nothing is in the way, in game units
/// (`g_lightningRange`).
const LIGHTNING_STEP: f32 = 50.0;
const LIGHTNING_RAMP: f32 = 0.5;
/// How often a new jitter is dealt (`c_jitterIntervalMs`), and how many there are.
const JITTER_EVERY: f32 = 0.05;
const JITTERS: usize = 20;

const ROPE_COLOURS: [Rgba; 3] = [[0x64, 0x3c, 0x0e, 255], [0x8f, 0x5a, 0x1c, 255], [0x14, 0x14, 0x00, 255]];
const ROPE_THICKNESS: f32 = 0.6;
const ROPE_WAVE: f32 = 4.0;
pub const ROPE_ATTACH: f32 = 3.0;

/// What a ribbon's cross-section is: three corners across and down, and where each is
/// along the picture's width.
struct Ring {
    corners: [(f32, f32); 3],
    across: [f32; 3],
}

impl Ring {
    /// The V the lightning and the rope are both made with (`g_lightningBeamThickness`).
    fn vee(thickness: f32, low: f32) -> Ring {
        Ring {
            corners: [(thickness, low), (0.0, thickness * 0.5), (-thickness, low)],
            across: [0.0, 0.5, 1.0],
        }
    }
}

/// A ribbon being put together: `BeamMesh`.
struct Ribbon {
    positions: Vec<[f32; 3]>,
    colours: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    /// The ring last laid, to join the next to.
    last_ring: Option<u32>,
    column: f32,
    textured_columns: f32,
}

fn linear(c: Rgba) -> [f32; 4] {
    Color::srgba_u8(c[0], c[1], c[2], c[3]).to_linear().to_f32_array()
}

fn blend(a: Rgba, b: Rgba, amount: f32) -> Rgba {
    let mix = |a: u8, b: u8| (a as i32 + ((b as i32 - a as i32) as f32 * amount) as i32) as u8;
    [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2]), mix(a[3], b[3])]
}

impl Ribbon {
    fn new(columns: f32) -> Ribbon {
        Ribbon {
            positions: Vec::new(),
            colours: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            last_ring: None,
            column: 0.0,
            textured_columns: columns,
        }
    }

    /// `EmitRing` and `EmitQuads`: a ring of corners about `at`, joined to the one before.
    fn ring(&mut self, at: Vec3, axes: [Vec3; 3], local: Vec3, ring: &Ring, colour: Rgba, column: f32) {
        let first = self.positions.len() as u32;
        for (i, &(y, z)) in ring.corners.iter().enumerate() {
            let offset = axes[0] * local.x + axes[1] * (local.y + y * UNIT) + axes[2] * (local.z + z * UNIT);
            self.positions.push((at + offset).to_array());
            self.colours.push(linear(colour));
            self.uvs.push([ring.across[i], column]);
        }
        if let Some(before) = self.last_ring {
            for i in 0..2 {
                let (lower, upper) = (first + i, before + i);
                self.indices.extend([lower, lower + 1, upper, lower + 1, upper + 1, upper]);
            }
        }
        self.last_ring = Some(first);
    }

    /// Steps the picture on a column, taking a fresh ring at the start of the next
    /// run of columns as the original does.
    fn next_column(&mut self, at: Vec3, axes: [Vec3; 3], local: Vec3, ring: &Ring, colour: Rgba, wrap: bool) {
        self.column += 1.0;
        if self.column > self.textured_columns {
            if wrap {
                self.ring(at, axes, local, ring, colour, 0.0);
            }
            self.column = 1.0;
        }
    }
}

/// The frame a span of a ribbon is laid in: along it, across it and up from it.
fn frame(forward: Vec3, up: Vec3) -> [Vec3; 3] {
    let up = if forward.cross(up).length_squared() < 1e-6 { Vec3::X } else { up };
    let across = up.cross(forward).normalize();
    [forward, across, forward.cross(across).normalize()]
}

/// One ribbon: from `origin` through each of `spans` (where it ends, and how far its
/// middle bows out), in `segments` pieces each (`AppendSpan`). `offsets` shift each
/// piece's ring across the span, in game units. `faced` is the way the camera's
/// right is, for a ribbon turned to face it.
struct Beam<'a> {
    origin: Vec3,
    spans: &'a [(Vec3, f32)],
    segments: usize,
    offsets: &'a dyn Fn(usize, usize) -> f32,
    colours: [Rgba; 3],
    ring: Ring,
    faced: Option<Vec3>,
}

impl Beam<'_> {
    fn build(&self) -> Ribbon {
        let mut ribbon = Ribbon::new(3.0);
        let mut last = self.origin;
        let [base, secondary, tertiary] = self.colours;
        let mut started = false;
        for (index, &(to, amount)) in self.spans.iter().enumerate() {
            let delta = to - last;
            let distance_squared = delta.length_squared();
            if distance_squared < MIN_STEP_SQUARED {
                continue;
            }
            let distance = distance_squared.sqrt();
            let forward = delta / distance;
            let axes = match self.faced {
                Some(right) => {
                    // Its width lies across the screen, so that its bends show.
                    let across = (right - forward * right.dot(forward)).try_normalize();
                    match across {
                        Some(across) => [forward, across, forward.cross(across).normalize()],
                        None => frame(forward, Vec3::Y),
                    }
                }
                None => frame(forward, Vec3::Y),
            };
            if !started {
                started = true;
                ribbon.ring(last, axes, Vec3::ZERO, &self.ring, base, 0.0);
                ribbon.column = 1.0;
            }
            let end_colour = if amount > 0.0 { secondary } else { tertiary };
            let bow = amount * UNIT;
            if self.segments <= 1 {
                let local = Vec3::new(distance, 0.0, 0.0);
                ribbon.ring(last, axes, local, &self.ring, base, ribbon.column);
                ribbon.next_column(last, axes, local, &self.ring, base, true);
            } else {
                let middle = Vec3::new(distance * 0.5, bow, 0.0);
                let end = Vec3::new(distance, 0.0, 0.0);
                for i in 1..self.segments {
                    let a = i as f32 / self.segments as f32;
                    let first = Vec3::ZERO.lerp(middle, a);
                    let second = middle.lerp(end, a);
                    let mut local = first.lerp(second, a);
                    let colour = blend(
                        blend(base, end_colour, a),
                        blend(end_colour, base, a),
                        a,
                    );
                    let shift = (self.offsets)(index, i - 1) * UNIT;
                    local.x += shift;
                    local.y += shift;
                    ribbon.ring(last, axes, local, &self.ring, colour, ribbon.column);
                    ribbon.next_column(last, axes, local, &self.ring, colour, true);
                }
                let mut local = end;
                let shift = (self.offsets)(index, self.segments - 1) * UNIT;
                local.x += shift;
                local.y += shift;
                ribbon.ring(last, axes, local, &self.ring, base, ribbon.column);
                ribbon.next_column(last, axes, local, &self.ring, base, false);
            }
            last = to;
        }
        ribbon
    }
}

fn fill(mesh: &mut Mesh, mut ribbon: Ribbon) {
    // The renderer can't hold a mesh of nothing: a ribbon with no length yet is one
    // triangle of no size.
    if ribbon.indices.is_empty() {
        ribbon.positions = vec![[0.0; 3]; 3];
        ribbon.colours = vec![[0.0; 4]; 3];
        ribbon.uvs = vec![[0.0; 2]; 3];
        ribbon.indices = vec![0, 1, 2];
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, ribbon.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, ribbon.colours);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, ribbon.uvs);
    mesh.insert_indices(Indices::U32(ribbon.indices));
}

fn empty_mesh() -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    fill(&mut mesh, Ribbon::new(3.0));
    mesh
}

fn ribbon_material(picture: Option<Handle<Image>>, additive: bool) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: picture,
        unlit: true,
        cull_mode: None,
        alpha_mode: if additive { AlphaMode::Add } else { AlphaMode::Blend },
        ..default()
    }
}

/// Has a picture tile across the ribbon rather than stretch.
fn repeat(images: &mut Assets<Image>, handle: &Handle<Image>) {
    if let Some(mut image) = images.get_mut(handle) {
        let mut sampler = ImageSamplerDescriptor::linear();
        sampler.address_mode_u = ImageAddressMode::Repeat;
        sampler.address_mode_v = ImageAddressMode::Repeat;
        image.sampler = ImageSampler::Descriptor(sampler);
    }
}

/// The pictures ribbons are drawn with.
pub const PICTURES: [&str; 5] = ["lightng", "tether", "streak", "canstrk", "mslstrk"];

/// A ribbon of an action's: it is gone when the action is.
#[derive(Component)]
pub struct Visual {
    of: Entity,
}

/// Marks an action that has its ribbon.
#[derive(Component)]
pub struct Ribboned;

/// The jitter dealt to the bolt, a new one every `JITTER_EVERY`.
#[derive(Default)]
pub struct Jitter {
    table: [f32; JITTERS],
    cursor: usize,
    since: f32,
    seed: u32,
}

impl Jitter {
    fn deal(&mut self) {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let unit = (self.seed >> 16) as f32 / 65536.0;
        self.table[self.cursor] = (unit - 0.5) * 5.0;
        self.cursor = (self.cursor + 1) % JITTERS;
    }

    fn tick(&mut self, dt: f32) {
        if self.table.iter().all(|v| *v == 0.0) {
            for _ in 0..JITTERS {
                self.deal();
            }
        }
        self.since += dt;
        while self.since > JITTER_EVERY {
            self.since -= JITTER_EVERY;
            self.deal();
        }
    }

    /// `RebuildBolt`: the shift for piece `piece` of span `span`.
    fn at(&self, span: usize, piece: usize) -> f32 {
        let back = span * 5 + piece;
        self.table[(self.cursor + JITTERS * 2 - 1 - back % JITTERS) % JITTERS]
    }
}

/// Gives each lightning bolt and each hook its ribbon, and keeps them true to it.
pub fn beams(
    mut commands: Commands,
    time: Res<Time>,
    track: Option<Res<Track>>,
    swatches: Option<Res<Swatches>>,
    camera: Query<&Transform, (With<Camera3d>, Without<Action>)>,
    mut actions: Query<(Entity, &Action, &Transform, &mut Visibility), Without<Visual>>,
    visuals: Query<(Entity, &Visual, &Mesh3d)>,
    karts: Query<&Kart>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut jitter: Local<Jitter>,
    mut tiled: Local<bool>,
    ribboned: Query<(), With<Ribboned>>,
) {
    let Some(swatches) = swatches else { return };
    if !*tiled {
        for name in PICTURES {
            if let Some(handle) = swatches.0.get(name) {
                repeat(&mut images, handle);
            }
        }
        *tiled = true;
    }
    let dt = time.delta_secs();
    jitter.tick(dt);
    let right = camera.iter().next().map(|c| c.right().as_vec3());
    // New ones.
    for (entity, action, _, mut visibility) in &mut actions {
        if ribboned.contains(entity) {
            continue;
        }
        let (name, additive) = match action {
            Action::Lightning { .. } => ("lightng", true),
            Action::Hook { .. } => ("tether", false),
            _ => continue,
        };
        if matches!(action, Action::Lightning { .. }) {
            // The bolt is its ribbon alone.
            *visibility = Visibility::Hidden;
        }
        let picture = swatches.0.get(name).cloned();
        let material = materials.add(ribbon_material(picture, additive));
        commands.entity(entity).insert(Ribboned);
        commands.spawn((
            Visual { of: entity },
            Mesh3d(meshes.add(empty_mesh())),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Visibility::default(),
            bevy::camera::visibility::NoFrustumCulling,
            DespawnOnExit(Screen::Race),
        ));
    }
    // Every ribbon follows its action.
    for (visual_entity, visual, mesh) in &visuals {
        let Ok((_, action, tf, _)) = actions.get(visual.of).map(|a| a).or_else(|_| Err(())) else {
            commands.entity(visual_entity).despawn();
            continue;
        };
        let ribbon = match action {
            Action::Lightning { time, shocked, .. } => {
                let Some(track) = &track else { continue };
                lightning(tf, *time, *shocked, &track, &karts, &jitter, right)
            }
            Action::Hook { owner, shot, pulling, time, released } => {
                let Ok(from) = karts.get(*owner).map(|k| k.pos + Vec3::Y * ROPE_ATTACH * UNIT) else {
                    continue;
                };
                rope(from, tf.translation, shot, *pulling, *time, *released)
            }
            _ => continue,
        };
        if let Some(mut mesh) = meshes.get_mut(&mesh.0) {
            *mesh = empty_mesh();
            fill(&mut mesh, ribbon);
        }
    }
}

/// `LightningAction::UpdateBoltPath` and `RebuildBolt`: four spans from the wand, each
/// bowing the other way, as long as the bolt has grown, or ending at what it strikes.
fn lightning(
    tf: &Transform,
    time: f32,
    shocked: Option<(Entity, f32)>,
    track: &Track,
    karts: &Query<&Kart>,
    jitter: &Jitter,
    right: Option<Vec3>,
) -> Ribbon {
    let forward = (tf.rotation * Vec3::NEG_Z).normalize();
    // The action stands half a range ahead of the wand (`items::actions`).
    let source = tf.translation - forward * 25.0 * UNIT;
    let age = crate::items::LIGHTNING_SECONDS - time;
    let grown = if age < LIGHTNING_RAMP {
        age / LIGHTNING_RAMP
    } else if time < LIGHTNING_RAMP {
        time / LIGHTNING_RAMP
    } else {
        1.0
    };
    let mut step = grown * LIGHTNING_STEP * UNIT;
    let mut direction = forward;
    let end = source + forward * step * 4.0;
    match shocked.and_then(|(victim, _)| karts.get(victim).ok()) {
        Some(victim) => {
            let to = victim.pos + Vec3::Y * 0.6;
            let delta = to - source;
            direction = delta.normalize_or(forward);
            step = delta.length() * 0.25;
        }
        None => {
            let hit: Option<Hit> = track.collision.any(source, end);
            if let Some(hit) = hit {
                step = source.distance(hit.point) * 0.25;
            }
        }
    }
    let mut spans = Vec::new();
    let mut amount = LIGHTNING_AMPLITUDE;
    for i in 1..=4 {
        spans.push((source + direction * step * i as f32, amount));
        amount = -amount;
    }
    let colours = if shocked.is_some() { LIGHTNING_HIT } else { LIGHTNING_COLOURS };
    let offsets = |span: usize, piece: usize| jitter.at(span, piece);
    Beam {
        origin: source,
        spans: &spans,
        segments: 5,
        offsets: &offsets,
        colours,
        ring: Ring::vee(LIGHTNING_THICKNESS, -0.25),
        faced: right,
    }
    .build()
}

/// `TetherProjectile::RebuildBeam` and `UpdateAttached`: the rope follows the way the
/// hook has flown, bowing less as it nears where it was aimed, and goes taut once it has
/// hold of a car. `UpdateReleased`: let go, it is wound in along a straight line, the
/// slack it has (`m_tension`) being gone in the first step of that.
fn rope(
    from: Vec3,
    to: Vec3,
    shot: &crate::items::Shot,
    pulling: Option<Entity>,
    left: f32,
    released: bool,
) -> Ribbon {
    let tension = pulling.map_or(0.0, |_| ((crate::items::HOOK_PULL_SECONDS - left) / 1.0).clamp(0.0, 1.0));
    let flying = 1.0 - shot.progress();
    let amount = if pulling.is_some() { 0.0 } else { flying * ROPE_WAVE };
    let flown = shot.age();
    let mut spans = Vec::new();
    let mut bow = amount;
    for i in 1..=4 {
        let along = i as f32 / 5.0;
        let straight = from.lerp(to, along);
        // Height of the path the hook took, so many fifths of the way back in time.
        let arc = shot.height_back(flown * (1.0 - along));
        let high = if pulling.is_some() {
            arc * (1.0 - tension) + straight.y * tension
        } else {
            arc
        };
        if released {
            spans.push((straight, 0.0));
            continue;
        }
        // The line is drawn from the hook's end, which the arc began from.
        spans.push((Vec3::new(straight.x, high.max(straight.y.min(to.y) - 1.0), straight.z), bow));
        bow = -bow;
    }
    spans.push((to, if released { 0.0 } else { bow }));
    let no_shift = |_: usize, _: usize| 0.0;
    Beam {
        origin: from,
        spans: &spans,
        segments: 5,
        offsets: &no_shift,
        colours: ROPE_COLOURS,
        ring: Ring::vee(ROPE_THICKNESS, 0.0),
        faced: None,
    }
    .build()
}

// ---- Streaks behind missiles and cannon balls ----

/// How a trail is set (`RaceTrailManager::Trail::Params`): how long it is, how big
/// across, its colour and which picture.
#[derive(Clone, Copy)]
struct Streak {
    duration: f32,
    size: f32,
    colour: Rgba,
    picture: &'static str,
}

const MISSILE_STREAK: Streak = Streak { duration: 0.4, size: 1.0, colour: [255, 255, 255, 0xc8], picture: "mslstrk" };
const CANNONBALL_STREAK: Streak = Streak { duration: 0.3, size: 3.0, colour: [0x32, 0x32, 0x32, 0xc8], picture: "canstrk" };
const LAUNCHER_STREAK: Streak = Streak { duration: 0.3, size: 3.0, colour: [0x32, 0x32, 0x32, 0x64], picture: "streak" };
/// A trail is kept as this many pieces of its length, the newest still growing, and
/// each ring shrinks to the middle by `END_SCALE` at the far end and fades to nothing.
const SAMPLES: usize = 4;
const END_SCALE: f32 = 0.1;

/// One moment of a trail: the corners of its square, its middle, how long it covers.
#[derive(Clone, Copy)]
struct Row {
    points: [Vec3; 4],
    centre: Vec3,
    covers: f32,
}

#[derive(Component)]
pub struct Trail {
    of: Option<Entity>,
    streak: Streak,
    rows: Vec<Row>,
    last: Option<Vec3>,
}

/// Gives each missile and cannon ball its trail, and builds every trail afresh each frame.
pub fn trails(
    mut commands: Commands,
    time: Res<Time>,
    swatches: Option<Res<Swatches>>,
    new: Query<(Entity, &Action), (Added<Action>, Without<Trail>)>,
    flying: Query<(&Action, &Transform), Without<Trail>>,
    mut trails: Query<(Entity, &mut Trail, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut tiled: Local<bool>,
) {
    let Some(swatches) = swatches else { return };
    if !*tiled {
        for name in PICTURES {
            if let Some(handle) = swatches.0.get(name) {
                repeat(&mut images, handle);
            }
        }
        *tiled = true;
    }
    for (entity, action) in &new {
        let streak = match action {
            Action::Missile { .. } => MISSILE_STREAK,
            Action::Cannonball { owner, on_hit, .. } => {
                if *owner == Entity::PLACEHOLDER {
                    if on_hit.is_none() {
                        continue;
                    }
                    LAUNCHER_STREAK
                } else {
                    CANNONBALL_STREAK
                }
            }
            _ => continue,
        };
        let material = materials.add(ribbon_material(swatches.0.get(streak.picture).cloned(), false));
        commands.spawn((
            Trail { of: Some(entity), streak, rows: Vec::new(), last: None },
            Mesh3d(meshes.add(empty_mesh())),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Visibility::default(),
            bevy::camera::visibility::NoFrustumCulling,
            DespawnOnExit(Screen::Race),
        ));
    }
    let dt = time.delta_secs();
    for (entity, mut trail, mesh) in &mut trails {
        let Streak { duration, size, colour, .. } = trail.streak;
        let sample = duration / SAMPLES as f32;
        // The head row grows while the projectile is there; once it is gone the whole
        // trail runs out (`Release`).
        let place = trail.of.and_then(|of| flying.get(of).ok()).map(|(_, tf)| tf.translation);
        match place {
            Some(at) => {
                let velocity = trail.last.map_or(Vec3::ZERO, |last| at - last);
                trail.last = Some(at);
                let level = Vec3::new(-velocity.z, 0.0, velocity.x);
                if let Some(side) = level.try_normalize() {
                    let across = side * size * UNIT;
                    let high = Vec3::Y * size * UNIT;
                    let p0 = at - across * 0.5 + high * 0.5;
                    let points = [p0, p0 - high, p0 + across - high, p0 + across];
                    match trail.rows.last_mut() {
                        Some(head) if head.covers < sample => {
                            head.points = points;
                            head.centre = at;
                            head.covers += dt;
                        }
                        _ => trail.rows.push(Row { points, centre: at, covers: dt }),
                    }
                }
            }
            None => {
                trail.of = None;
                // The oldest rows go first.
                let mut left = dt;
                while left > 0.0 && !trail.rows.is_empty() {
                    let oldest = &mut trail.rows[0];
                    if oldest.covers > left {
                        oldest.covers -= left;
                        left = 0.0;
                    } else {
                        left -= oldest.covers;
                        trail.rows.remove(0);
                    }
                }
                if trail.rows.len() < 2 {
                    commands.entity(entity).despawn();
                    continue;
                }
            }
        }
        // Only what the duration holds.
        let mut total = 0.0;
        let mut keep = 0;
        for (n, row) in trail.rows.iter().enumerate().rev() {
            total += row.covers;
            keep = n;
            if total > duration {
                break;
            }
        }
        trail.rows.drain(..keep);
        let ribbon = streak_ribbon(&trail.rows, duration, colour);
        if let Some(mut mesh) = meshes.get_mut(&mesh.0) {
            *mesh = empty_mesh();
            fill(&mut mesh, ribbon);
        }
    }
}

/// `RaceTrailManager::Trail::RebuildGeometry`: newest row first, each pulled in towards
/// its middle and made fainter the longer ago it was.
fn streak_ribbon(rows: &[Row], duration: f32, colour: Rgba) -> Ribbon {
    let mut ribbon = Ribbon::new(f32::MAX);
    if rows.len() < 2 {
        return ribbon;
    }
    let alpha = colour[3] as f32 / 255.0;
    let step = alpha / (SAMPLES + 1) as f32;
    let (v_step, u_step) = (1.0 / (SAMPLES + 1) as f32, 1.0 / 5.0);
    let mut elapsed = 0.0;
    let mut faded = alpha;
    let mut v = 0.0;
    let mut rings: Vec<u32> = Vec::new();
    for (n, row) in rows.iter().rev().enumerate() {
        let pull = if n == 0 { 0.0 } else { (elapsed / duration).min(1.0) * (1.0 - END_SCALE) };
        if n > 0 {
            faded = (faded - step).max(0.0);
        }
        let first = ribbon.positions.len() as u32;
        let mut u = 0.0;
        for point in row.points {
            ribbon.positions.push(point.lerp(row.centre, pull).to_array());
            let c = [colour[0], colour[1], colour[2], (faded * 255.0) as u8];
            ribbon.colours.push(linear(c));
            ribbon.uvs.push([u, v]);
            u += u_step;
        }
        rings.push(first);
        v += v_step;
        elapsed += row.covers;
    }
    for pair in rings.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        for i in 0..4u32 {
            let next = (i + 1) % 4;
            ribbon.indices.extend([b + i, a + i, b + next, b + next, a + i, a + next]);
        }
    }
    ribbon
}
