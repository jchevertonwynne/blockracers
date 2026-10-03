//! Particle emitters, as the circuits' `.EMB` files define them: snow, smoke, dust and
//! the like. An emitter throws out a flat, camera-facing particle at intervals, each
//! with one of the emitter's velocities; particles drift, grow and fade. Follows
//! `CutsceneParticle`.

use crate::assets::{
    image::Pixels,
    mab::Track,
    tokens::{Token, tokenize},
};
use crate::physics::UNIT;
use crate::scenery::to_world;
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::collections::HashMap;
use std::sync::Arc;

/// Particles are never drawn smaller than this; a size of nothing at all upsets the renderer.
const MIN_SIZE: f32 = 0.001;

/// One kind of emitter. Lengths and speeds are in the original's units.
pub struct EmitterDef {
    /// Seconds between particles, and the chance that one is skipped.
    interval: f32,
    skip: f32,
    size: Vec2,
    /// Growth over a particle's life.
    growth: Vec2,
    life: f32,
    /// How long the emitter itself lasts, if not for ever.
    duration: Option<f32>,
    acceleration: Vec3,
    velocities: Vec<Vec3>,
    pub material: Option<String>,
    /// Or the track of the material animation alongside that its particles play through.
    pub track: Option<usize>,
}

/// What an emitter's particles look like: one picture, or several with the frames they
/// show from and the track that times them.
#[derive(Default)]
pub struct Look {
    pub frames: Vec<(u32, Pixels)>,
    pub track: Option<Track>,
    pub additive: bool,
}

fn number(token: Option<&Token>) -> f32 {
    match token {
        Some(Token::Float(v)) => *v,
        Some(Token::Int(v)) => *v as f32,
        _ => 0.0,
    }
}

/// The emitters of an `.EMB` file, by name.
pub fn parse(data: &[u8]) -> Vec<(String, EmitterDef)> {
    let tokens = tokenize(data);
    let mut out = Vec::new();
    for (i, token) in tokens.iter().enumerate().skip(1) {
        let (Token::Key(0x27), Some(Token::Str(name)), Some(Token::LCurly)) = (token, tokens.get(i + 1), tokens.get(i + 2))
        else {
            continue;
        };
        let mut def = EmitterDef {
            interval: 0.1,
            skip: 0.0,
            size: Vec2::ONE,
            growth: Vec2::ZERO,
            life: 1.0,
            duration: None,
            acceleration: Vec3::ZERO,
            velocities: Vec::new(),
            material: None,
            track: None,
        };
        let mut at = i + 3;
        while let Some(token) = tokens.get(at) {
            let value = |n: usize| number(tokens.get(at + 1 + n));
            match token {
                Token::RCurly => break,
                // So many a second.
                Token::Key(0x28) => def.interval = 1.0 / value(0).max(0.001),
                Token::Key(0x29) => def.skip = value(0),
                Token::Key(0x2a) => def.acceleration = Vec3::new(value(0), value(1), value(2)),
                Token::Key(0x2b) => {
                    // `[count] { x y z ... }`
                    let count = value(1) as usize;
                    def.velocities = (0..count).map(|n| Vec3::new(value(4 + n * 3), value(5 + n * 3), value(6 + n * 3))).collect();
                    at += 5 + count * 3;
                }
                Token::Key(0x2e) => def.track = Some(value(0) as usize),
                Token::Key(0x2c) => def.size.y = value(0),
                Token::Key(0x2d) => def.size.x = value(0),
                Token::Key(0x2f) => def.life = value(0) / 1000.0,
                Token::Key(0x30) => def.duration = Some(value(0) / 1000.0).filter(|d| *d >= 0.0),
                Token::Key(0x31) => def.growth.y = value(0),
                Token::Key(0x32) => def.growth.x = value(0),
                Token::Key(0x34) => {
                    if let Some(Token::Str(material)) = tokens.get(at + 1) {
                        def.material = Some(material.to_lowercase());
                    }
                }
                _ => {}
            }
            at += 1;
        }
        out.push((name.to_lowercase(), def));
    }
    out
}

struct Kind {
    def: EmitterDef,
    /// One material per picture, with the frame each shows from.
    materials: Vec<Handle<StandardMaterial>>,
    frames: Vec<u32>,
    track: Option<Track>,
}

impl Kind {
    fn material(&self, age: f32) -> Handle<StandardMaterial> {
        let index = self.track.map_or(0, |t| t.sample(&self.frames, age));
        self.materials[index.min(self.materials.len() - 1)].clone()
    }
}

/// The circuit's emitters, ready to use.
#[derive(Resource, Default)]
pub struct Emitters {
    kinds: HashMap<String, Arc<Kind>>,
    quad: Handle<Mesh>,
}

impl Emitters {
    /// Builds the emitters and the materials their particles are drawn with: each
    /// one's own picture, or a soft puff for those that take theirs from an animation.
    pub fn new(
        defs: Vec<(String, EmitterDef, Look)>,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Self {
        let mut image = |width: u32, height: u32, rgba: Vec<u8>| {
            let size = Extent3d { width, height, depth_or_array_layers: 1 };
            images.add(Image::new(size, TextureDimension::D2, rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default()))
        };
        // A round, soft-edged grey blob.
        let puff: Vec<u8> = (0..32 * 32)
            .flat_map(|i| {
                let (x, y) = ((i % 32) as f32 - 15.5, (i / 32) as f32 - 15.5);
                let fade = (1.0 - (x * x + y * y).sqrt() / 16.0).clamp(0.0, 1.0);
                [200, 200, 200, (fade * 200.0) as u8]
            })
            .collect();
        let puff = image(32, 32, puff);
        let mut kinds = HashMap::new();
        for (name, def, look) in defs {
            let alpha_mode = if look.additive { AlphaMode::Add } else { AlphaMode::Blend };
            let mut material = |texture| {
                materials.add(StandardMaterial {
                    base_color_texture: Some(texture),
                    unlit: true,
                    alpha_mode,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                })
            };
            let frames: Vec<u32> = look.frames.iter().map(|f| f.0).collect();
            let mut pictures: Vec<_> = look.frames.into_iter().map(|(_, p)| material(image(p.width, p.height, p.rgba))).collect();
            if pictures.is_empty() {
                pictures.push(material(puff.clone()));
            }
            kinds.insert(name, Arc::new(Kind { def, materials: pictures, frames, track: look.track }));
        }
        Emitters { kinds, quad: meshes.add(Rectangle::new(1.0, 1.0)) }
    }

    /// An emitter of the named kind, to spawn with a `Transform` saying where it is.
    pub fn emitter(&self, name: &str) -> Option<Emitter> {
        let kind = self.kinds.get(name)?.clone();
        // Due at once.
        Some(Emitter { timer: kind.def.interval, kind, age: 0.0, seed: 0x9e37_79b9, velocity: Vec3::ZERO, spawned: 0 })
    }

    /// Starts an emitter of the named kind at a place; those with a duration end themselves.
    pub fn spawn(&self, commands: &mut Commands, name: &str, at: Transform) -> Option<Entity> {
        Some(commands.spawn((self.emitter(name)?, at)).id())
    }
}

/// Something throwing out particles from where its `Transform` puts it.
#[derive(Component)]
pub struct Emitter {
    kind: Arc<Kind>,
    timer: f32,
    age: f32,
    seed: u32,
    /// Added to each particle's own velocity.
    pub velocity: Vec3,
    /// How many particles it has thrown out.
    pub spawned: u32,
}

impl Emitter {
    fn roll(&mut self, n: u32) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed % n.max(1)
    }
}

#[derive(Component)]
pub struct Particle {
    kind: Arc<Kind>,
    velocity: Vec3,
    acceleration: Vec3,
    size: Vec2,
    growth: Vec2,
    age: f32,
    life: f32,
}

pub fn emit(mut commands: Commands, time: Res<Time>, emitters: Option<Res<Emitters>>, mut sources: Query<(Entity, &mut Emitter, &Transform)>) {
    let Some(emitters) = emitters else { return };
    let dt = time.delta_secs().min(0.05);
    for (entity, mut emitter, transform) in &mut sources {
        let kind = emitter.kind.clone();
        emitter.age += dt;
        if kind.def.duration.is_some_and(|d| emitter.age >= d) {
            commands.entity(entity).despawn();
            continue;
        }
        emitter.timer += dt;
        if emitter.timer < kind.def.interval {
            continue;
        }
        emitter.timer = 0.0;
        if (emitter.roll(1000) as f32) < kind.def.skip * 1000.0 || kind.def.velocities.is_empty() {
            continue;
        }
        let velocity = kind.def.velocities[emitter.roll(kind.def.velocities.len() as u32) as usize];
        emitter.spawned += 1;
        commands.spawn((
            Particle {
                kind: kind.clone(),
                velocity: transform.rotation * to_world(velocity) + emitter.velocity,
                acceleration: to_world(kind.def.acceleration),
                size: kind.def.size * UNIT,
                growth: kind.def.growth * UNIT,
                age: 0.0,
                life: kind.def.life,
            },
            Mesh3d(emitters.quad.clone()),
            MeshMaterial3d(kind.material(0.0)),
            Transform::from_translation(transform.translation).with_scale(Vec3::splat(MIN_SIZE)),
        ));
    }
}

/// Moves the particles, turns them to face the camera and removes the spent ones.
pub fn particles(
    mut commands: Commands,
    time: Res<Time>,
    camera: Single<&Transform, (With<Camera3d>, Without<Particle>)>,
    mut all: Query<(Entity, &mut Particle, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let dt = time.delta_secs().min(0.05);
    for (entity, mut particle, mut transform, mut material) in &mut all {
        particle.age += dt;
        if particle.age >= particle.life {
            commands.entity(entity).despawn();
            continue;
        }
        let acceleration = particle.acceleration;
        particle.velocity += acceleration * dt;
        transform.translation += particle.velocity * dt;
        transform.rotation = camera.rotation;
        if particle.kind.track.is_some() {
            let now = particle.kind.material(particle.age);
            if material.0 != now {
                material.0 = now;
            }
        }
        let size = (particle.size + particle.growth * (particle.age / particle.life)).max(Vec2::splat(MIN_SIZE));
        transform.scale = Vec3::new(size.x, size.y, 1.0);
    }
}

#[cfg(test)]
#[test]
fn snow_and_smoke_are_defined_on_the_ice_circuit() {
    let Some(jam) = crate::assets::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else { return };
    let emitters = parse(jam.get("/GAMEDATA/RACEC1R3/RACEC1R3.EMB").unwrap());
    let snow = &emitters.iter().find(|e| e.0 == "snow").unwrap().1;
    assert_eq!((snow.interval, snow.life, snow.velocities.len()), (1.0 / 12.0, 1.5, 3));
    assert_eq!(snow.material.as_deref(), Some("snowflak"));
    assert!(snow.velocities.iter().all(|v| v.z < -20.0) && snow.duration.is_none());
    let smoke = &emitters.iter().find(|e| e.0 == "smoke").unwrap().1;
    assert_eq!((smoke.size, smoke.growth, smoke.velocities.len()), (Vec2::splat(5.0), Vec2::splat(5.0), 4));
    assert_eq!((smoke.material.as_deref(), smoke.track), (None, Some(0)));
    assert!(parse(jam.get("/GAMEDATA/COMMON/EMITTER.EMB").unwrap()).len() >= 10);
}

#[cfg(test)]
#[test]
fn every_emitter_has_its_pictures() {
    for (race, _) in crate::world::circuits() {
        let Some((_, world)) = crate::world::load(&race) else { return };
        for (name, def, look) in &world.emitters {
            assert!(!look.frames.is_empty(), "{race} {name}");
            assert_eq!(def.track.is_some() && def.material.is_none(), look.track.is_some(), "{race} {name}");
        }
        assert!(world.emitters.iter().any(|e| e.0 == "dust" && e.2.frames.len() == 4));
    }
}
