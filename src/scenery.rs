//! Everything a circuit's world files (`.WDB`) place around the track: scenery models
//! and the jointed, animated ones (hammers, doors, carts) that hazards and events
//! drive. Follows `GolWorldDatabase` and `GolAnimatedEntity`.

use crate::assets::{
    Jam,
    adb::Animation,
    gdb::{Bone, Model, parse_skeleton},
    tokens::{Token, tokenize},
};
use crate::physics::UNIT;
use crate::world::{Library, LoadedWorld, Surface, surface_bundle};
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

/// Game axes (X forward, Y left, Z up) onto ours (Y up).
fn basis() -> Quat {
    Quat::from_mat3(&Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y))
}

pub fn to_world(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.z, -p.y) * UNIT
}

/// A model placed in the world, ready to spawn.
pub struct PropDef {
    name: String,
    /// Meshes by the bone they hang from; `None` is the model itself.
    surfaces: Vec<(Option<usize>, Vec<Surface>)>,
    rig: Option<Rig>,
    /// In game coordinates and axes.
    position: Vec3,
    rotation: Quat,
    scale: f32,
}

/// A skeleton and what moves it.
#[derive(Clone)]
pub struct Rig {
    pub bones: Arc<Vec<Bone>>,
    pub animation: Arc<Animation>,
}

/// A placed model. Positions here are in game coordinates.
#[derive(Component)]
pub struct Prop {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
}

/// A prop with a skeleton, playing one part of its animation.
#[derive(Component)]
pub struct Animated {
    pub rig: Rig,
    pub part: usize,
    /// Milliseconds into the part.
    pub time: f32,
    pub playing: bool,
    pub looping: bool,
    /// Playback speed; negative runs backwards.
    pub rate: f32,
    /// The part to go on to, and whether to loop it, once this one has played out
    /// (or at once, if this one loops).
    pub queued: Option<(usize, bool)>,
}

impl Animated {
    pub fn play(&mut self, part: usize, looping: bool) {
        (self.part, self.time, self.playing, self.looping, self.rate) = (part, 0.0, true, looping, 1.0);
        self.queued = None;
    }

    /// Holds the first frame of a part.
    pub fn freeze(&mut self, part: usize) {
        self.play(part, false);
        self.playing = false;
    }

    /// How many frames into its part it is.
    pub fn frame(&self) -> f32 {
        self.rig.animation.parts.get(self.part).map_or(0.0, |p| self.time / p.ms_per_frame)
    }

    pub fn length(&self) -> f32 {
        self.rig.animation.parts.get(self.part).map_or(0.0, |p| p.frames * p.ms_per_frame)
    }

    /// Whether a part played once has run to its end.
    pub fn done(&self) -> bool {
        !self.playing
    }

    /// A bone's place within its parent at the current moment.
    fn local(&self, bone: usize, frame: f32) -> (Quat, Vec3) {
        let rest = &self.rig.bones[bone];
        let (position, rotation) = self.rig.animation.sample(self.part, bone, frame);
        (rotation.unwrap_or(Quat::from_array(rest.rotation)), position.unwrap_or(Vec3::from(rest.position)))
    }

    /// A bone's rotation and position in the model's own space, `frame` frames in.
    fn pose(&self, bone: usize, frame: f32) -> (Quat, Vec3) {
        let (rotation, position) = self.local(bone, frame);
        match self.rig.bones[bone].parent {
            Some(parent) => {
                let (parent_rotation, parent_position) = self.pose(parent, frame);
                (parent_rotation * rotation, parent_position + parent_rotation * position)
            }
            None => (rotation, position),
        }
    }

    /// Where a bone is in the world (ours, not the game's) right now, or was `back`
    /// frames ago.
    pub fn bone_position(&self, prop: &Prop, bone: usize, back: f32) -> Vec3 {
        let frames = self.rig.animation.parts.get(self.part).map_or(1.0, |p| p.frames);
        let frame = (self.frame() - back).rem_euclid(frames.max(1.0));
        let (_, position) = self.pose(bone.min(self.rig.bones.len() - 1), frame);
        to_world(prop.position + prop.rotation * (position * prop.scale))
    }
}

/// One bone of an animated prop.
#[derive(Component)]
pub struct Joint {
    prop: Entity,
    bone: usize,
}

/// The props of the circuit, by name.
#[derive(Resource, Default)]
pub struct Scenery(pub HashMap<String, Entity>);

/// The names in a `key [count] { "a" "b" ... }` list.
fn names(tokens: &[Token], key: u16) -> Vec<String> {
    let Some(at) = tokens.windows(2).position(|w| w[0] == Token::Key(key) && w[1] == Token::LBracket) else {
        return Vec::new();
    };
    let strings = tokens[at + 5..].iter().map_while(|t| match t {
        Token::Str(name) => Some(name.to_lowercase()),
        _ => None,
    });
    strings.collect()
}

fn number(token: Option<&Token>) -> f32 {
    match token {
        Some(Token::Float(v)) => *v,
        Some(Token::Int(v)) => *v as f32,
        _ => 0.0,
    }
}

/// Reads every `.WDB` in `dir` and loads what they place, apart from the track itself
/// (`skip`), which is loaded on its own.
pub fn load(jam: &Jam, dir: &str, library: &Library, skip: &str) -> Vec<PropDef> {
    let mut files: Vec<&str> = jam.list(dir).filter(|f| f.ends_with(".WDB")).collect();
    files.sort();
    let mut props = Vec::new();
    for file in files {
        let tokens = tokenize(jam.get(file).unwrap_or_default());
        let (models, skeletons, animations) = (names(&tokens, 0x2a), names(&tokens, 0x2c), names(&tokens, 0x29));
        for (i, token) in tokens.iter().enumerate() {
            let (Token::Key(kind @ (0x2e | 0x2f)), Some(Token::Str(name)), Some(Token::LCurly)) =
                (token, tokens.get(i + 1), tokens.get(i + 2))
            else {
                continue;
            };
            let end = i + tokens[i..].iter().position(|t| *t == Token::RCurly).unwrap_or(0);
            let fields = &tokens[i + 3..end];
            let field = |key: u16, n: usize| {
                let at = fields.iter().position(|t| *t == Token::Key(key))?;
                Some(number(fields.get(at + 1 + n)))
            };
            let vec3 = |key: u16, from: usize| Some(Vec3::new(field(key, from)?, field(key, from + 1)?, field(key, from + 2)?));
            // Static models name a model; jointed ones a model, a skeleton and an animation.
            let model_key = if *kind == 0x2e { 0x2a } else { 0x33 };
            let Some(model_name) = field(model_key, 0).and_then(|index| models.get(index as usize)) else { continue };
            if model_name == skip {
                continue;
            }
            let Some(model) = jam.get(&format!("{dir}/{model_name}.GDB")).and_then(Model::parse) else { continue };
            let rig = (*kind == 0x2f)
                .then(|| {
                    let skeleton = skeletons.get(field(0x33, 1)? as usize)?;
                    let animation = animations.get(field(0x33, 2)? as usize)?;
                    Some(Rig {
                        bones: Arc::new(parse_skeleton(jam.get(&format!("{dir}/{skeleton}.SDB"))?)?),
                        animation: Arc::new(Animation::parse(jam.get(&format!("{dir}/{animation}.ADB"))?)?),
                    })
                })
                .flatten();

            // The orientation is the model's forward and up axes, of any length.
            let x = vec3(0x32, 0).unwrap_or(Vec3::X).normalize_or(Vec3::X);
            let y = vec3(0x32, 3).unwrap_or(Vec3::Z).cross(x).normalize_or(Vec3::Y);
            let rotation = Quat::from_mat3(&Mat3::from_cols(x, y, x.cross(y)));
            let scale = field(0x36, 0).unwrap_or(1.0) * model.scale;

            let mut bones: Vec<Option<usize>> = model.batches.iter().map(|b| b.bone.filter(|_| rig.is_some())).collect();
            bones.sort();
            bones.dedup();
            let surfaces = bones
                .into_iter()
                .map(|bone| (bone, library.surfaces(&model, |b| b.bone.filter(|_| rig.is_some()) == bone, Vec3::from)))
                .collect();
            props.push(PropDef {
                name: name.to_lowercase(),
                surfaces,
                rig,
                position: vec3(0x31, 0).unwrap_or_default(),
                rotation,
                scale,
            });
        }
    }
    props
}

pub fn spawn_scenery(
    mut commands: Commands,
    mut world: ResMut<LoadedWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut scenery = Scenery::default();
    for def in std::mem::take(&mut world.props) {
        let transform = Transform {
            translation: to_world(def.position),
            rotation: basis() * def.rotation,
            scale: Vec3::splat(def.scale * UNIT),
        };
        let prop = Prop { position: def.position, rotation: def.rotation, scale: def.scale };
        let root = commands.spawn((prop, transform, Visibility::default())).id();
        // One entity per bone, each inside its parent, with the bone's meshes on it.
        let mut joints = Vec::new();
        if let Some(rig) = &def.rig {
            for (bone, rest) in rig.bones.iter().enumerate() {
                let transform = Transform::from_translation(Vec3::from(rest.position))
                    .with_rotation(Quat::from_array(rest.rotation));
                let joint = commands.spawn((Joint { prop: root, bone }, transform, Visibility::default())).id();
                joints.push(joint);
            }
            for (bone, rest) in rig.bones.iter().enumerate() {
                commands.entity(rest.parent.map_or(root, |parent| joints[parent])).add_child(joints[bone]);
            }
            let animated =
                Animated { rig: rig.clone(), part: 0, time: 0.0, playing: true, looping: true, rate: 1.0, queued: None };
            commands.entity(root).insert(animated);
        }
        for (bone, surfaces) in def.surfaces {
            let parent = bone.and_then(|b| joints.get(b)).copied().unwrap_or(root);
            for surface in surfaces {
                let mesh = commands.spawn(surface_bundle(surface, &mut meshes, &mut materials, &mut images)).id();
                commands.entity(parent).add_child(mesh);
            }
        }
        scenery.0.insert(def.name, root);
    }
    commands.insert_resource(scenery);
}

/// Runs the animations on and poses the bones.
pub fn animate(
    time: Res<Time>,
    mut props: Query<&mut Animated>,
    mut joints: Query<(&Joint, &mut Transform)>,
) {
    let ms = time.delta_secs().min(0.05) * 1000.0;
    for mut animated in &mut props {
        if let (Some((part, looping)), true) = (animated.queued, animated.looping || !animated.playing) {
            animated.play(part, looping);
        }
        if !animated.playing {
            continue;
        }
        let length = animated.length();
        animated.time += ms * animated.rate;
        if animated.looping {
            animated.time = animated.time.rem_euclid(length.max(1.0));
        } else if animated.time >= length || animated.time < 0.0 {
            // Hold the last pose.
            animated.time = animated.time.clamp(0.0, (length - 0.01).max(0.0));
            animated.playing = false;
        }
    }
    for (joint, mut transform) in &mut joints {
        let Ok(animated) = props.get(joint.prop) else { continue };
        let (rotation, position) = animated.local(joint.bone, animated.frame());
        (transform.rotation, transform.translation) = (rotation, position);
    }
}
