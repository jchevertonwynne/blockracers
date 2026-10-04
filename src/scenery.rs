//! Everything a circuit's world files (`.WDB`) place around the track: scenery models
//! and the jointed, animated ones (hammers, doors, carts) that hazards and events
//! drive. Follows `GolWorldDatabase` and `GolAnimatedEntity`.

use crate::assets::{
    Jam,
    adb::{Animation, turn},
    gdb::{Bone, Model, parse_skeleton},
    image::Pixels,
    mab::{MaterialAnimation, Track},
    tokens::{Token, tokenize},
};
use crate::particles::Emitters;
use crate::physics::UNIT;
use crate::world::{Library, LoadedWorld, Surface, surface_bundle};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
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
    /// How fast its textures slide, in widths a second.
    scroll: Vec2,
    /// Materials of the model that play through pictures: which material, the track
    /// it starts on, and the tracks there are.
    cycles: Vec<(usize, usize, Arc<Vec<ReelDef>>)>,
    /// Part of the sky world, which goes where the camera goes.
    backdrop: bool,
}

/// One track of a material animation: its timing and the pictures it shows, with the
/// frame each comes in at.
pub struct ReelDef {
    track: Track,
    pictures: Vec<(u32, Pixels)>,
}

struct Reel {
    track: Track,
    frames: Vec<u32>,
    pictures: Vec<Handle<Image>>,
}

/// A mesh whose picture changes as a material animation's track plays.
#[derive(Component)]
pub struct Cycle {
    prop: Entity,
    reels: Arc<Vec<Reel>>,
    reel: usize,
    time: f32,
    looping: bool,
    shown: usize,
}

/// Put on a prop to move its materials from one track to another: pairs of the track
/// playing and the one to play instead, and whether that goes round and round.
#[derive(Component)]
pub struct Retrack(pub Vec<(usize, usize)>, pub bool);

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
    /// How fast its textures slide, in widths a second.
    pub scroll: Vec2,
}

/// The materials of a prop whose textures slide, and how far they have slid.
#[derive(Component, Default)]
pub struct Scrolling {
    offset: Vec2,
    pub materials: Vec<Handle<StandardMaterial>>,
}

/// Pictures that hazards swap onto models, by material name.
#[derive(Resource, Default)]
pub struct Swatches(pub HashMap<String, Handle<Image>>);

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
        (rotation.unwrap_or(turn(rest.rotation)), position.unwrap_or(Vec3::from(rest.position)))
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
    load_files(jam, dir, &files, library, skip)
}

/// Loads what the given `.WDB` files of `dir` place.
pub fn load_files(jam: &Jam, dir: &str, files: &[&str], library: &Library, skip: &str) -> Vec<PropDef> {
    let mut props = Vec::new();
    for file in files {
        let tokens = tokenize(jam.get(file).unwrap_or_default());
        let (models, skeletons, animations) = (names(&tokens, 0x2a), names(&tokens, 0x2c), names(&tokens, 0x29));
        // Material animations, each decoded the once however many models use it.
        let reels: Vec<Arc<Vec<ReelDef>>> = names(&tokens, 0x3d)
            .iter()
            .map(|name| {
                let animation = jam.get(&format!("{dir}/{name}.MAB")).and_then(MaterialAnimation::parse).unwrap_or_default();
                let reel = |(index, track): (usize, &Track)| ReelDef {
                    track: *track,
                    pictures: animation.materials(index).iter().filter_map(|(m, frame)| Some((*frame, library.texture(m)?))).collect(),
                };
                Arc::new(animation.tracks.iter().enumerate().map(reel).collect())
            })
            .collect();
        for (i, token) in tokens.iter().enumerate() {
            // A placement may go without a name, and is then known by its model's.
            let (kind, name, open) = match (token, tokens.get(i + 1), tokens.get(i + 2)) {
                (Token::Key(kind @ (0x2e | 0x2f)), Some(Token::Str(name)), Some(Token::LCurly)) => (kind, Some(name), i + 2),
                (Token::Key(kind @ (0x2e | 0x2f)), Some(Token::LCurly), _) => (kind, None, i + 1),
                _ => continue,
            };
            let end = i + tokens[i..].iter().position(|t| *t == Token::RCurly).unwrap_or(0);
            let fields = &tokens[open + 1..end];
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
                backdrop: file.to_uppercase().ends_with("/BACKGRD.WDB"),
                name: name.unwrap_or(model_name).to_lowercase(),
                surfaces,
                rig,
                position: vec3(0x31, 0).unwrap_or_default(),
                rotation,
                scale,
                scroll: Vec2::new(field(0x3f, 0).unwrap_or(0.0), field(0x3f, 1).unwrap_or(0.0)),
                // `[count] { animation track material model ... }`
                cycles: (0..field(0x3e, 1).unwrap_or(0.0) as usize)
                    .filter_map(|n| {
                        let value = |k: usize| field(0x3e, 4 + n * 4 + k).map(|v| v as usize);
                        Some((value(2)?, value(1)?, reels.get(value(0)?)?.clone()))
                    })
                    .collect(),
            });
        }
    }
    props
}

/// A model ready to be put into the world any number of times.
struct Template {
    /// Meshes and their materials by the bone they hang from, and which of the model's
    /// materials each is.
    parts: Vec<(Option<usize>, Vec<(Handle<Mesh>, Handle<StandardMaterial>, usize)>)>,
    cycles: Vec<(usize, usize, Arc<Vec<Reel>>)>,
    /// Where its file puts it, in the game's coordinates.
    position: Vec3,
    rig: Option<Rig>,
    scale: f32,
    scroll: Vec2,
}

impl Template {
    fn new(
        def: &mut PropDef,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Self {
        let parts = std::mem::take(&mut def.surfaces)
            .into_iter()
            .map(|(bone, surfaces)| {
                let surfaces = surfaces.into_iter().map(|s| {
                    let material = s.material;
                    let bundle = surface_bundle(s, meshes, materials, images);
                    (bundle.0.0, bundle.1.0, material)
                });
                (bone, surfaces.collect())
            })
            .collect();
        let cycles = std::mem::take(&mut def.cycles)
            .into_iter()
            .map(|(material, start, reels)| {
                let reel = |def: &ReelDef| Reel {
                    track: def.track,
                    frames: def.pictures.iter().map(|p| p.0).collect(),
                    pictures: def.pictures.iter().map(|(_, p)| images.add(repeating(p))).collect(),
                };
                (material, start, Arc::new(reels.iter().map(reel).collect()))
            })
            .collect();
        Template { parts, cycles, position: def.position, rig: def.rig.clone(), scale: def.scale, scroll: def.scroll }
    }

    /// Hangs the model's bones and meshes on `root`, which wants a `Transform` in the
    /// game's axes, and has it play the first part of its animation.
    fn build(&self, commands: &mut Commands, root: Entity, prop: Prop) {
        let mut joints = Vec::new();
        if let Some(rig) = &self.rig {
            // One entity per bone, each inside its parent, with the bone's meshes on it.
            for (bone, rest) in rig.bones.iter().enumerate() {
                let transform = Transform::from_translation(Vec3::from(rest.position)).with_rotation(turn(rest.rotation));
                joints.push(commands.spawn((Joint { prop: root, bone }, transform, Visibility::default())).id());
            }
            for (bone, rest) in rig.bones.iter().enumerate() {
                commands.entity(rest.parent.map_or(root, |parent| joints[parent])).add_child(joints[bone]);
            }
            commands.entity(root).insert(self.animated(rig));
        }
        let mut scrolling = Scrolling::default();
        for (bone, surfaces) in &self.parts {
            let parent = bone.and_then(|b| joints.get(b)).copied().unwrap_or(root);
            for (mesh, material, index) in surfaces {
                scrolling.materials.push(material.clone());
                let mesh = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()))).id();
                commands.entity(parent).add_child(mesh);
                if let Some((_, start, reels)) = self.cycles.iter().find(|c| c.0 == *index && c.1 < c.2.len()) {
                    let cycle = Cycle { prop: root, reels: reels.clone(), reel: *start, time: 0.0, looping: true, shown: usize::MAX };
                    commands.entity(mesh).insert(cycle);
                }
            }
        }
        commands.entity(root).insert((prop, scrolling));
    }
}

impl Template {
    fn animated(&self, rig: &Rig) -> Animated {
        Animated { rig: rig.clone(), part: 0, time: 0.0, playing: true, looping: true, rate: 1.0, queued: None }
    }
}

/// How a model put into the world is to move.
#[derive(Clone, Copy)]
pub enum Motion {
    /// Round and round its first part.
    Loop,
    /// Through its first part once, then round and round this one.
    Then(usize),
    /// Held at the start of a part, shifted so that this bone sits where the model is put.
    Held(usize, usize),
}

/// The models power-ups are made of, by name.
#[derive(Resource, Default)]
pub struct Models(HashMap<String, Template>);

impl Models {
    pub fn has(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Puts one of the models into the world. The entity returned is in our axes,
    /// facing -Z, and takes its place from `at`; the model's own forward is the
    /// game's X.
    pub fn spawn(&self, commands: &mut Commands, name: &str, at: Transform, motion: Motion) -> Option<Entity> {
        let template = self.0.get(name)?;
        // The game's X, Y and Z are our -Z, -X and Y.
        let axes = Quat::from_mat3(&Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y));
        let size = template.scale * UNIT;
        let mut inner = Transform::from_rotation(axes).with_scale(Vec3::splat(size));
        let model = commands.spawn(Visibility::default()).id();
        let prop = Prop { position: Vec3::ZERO, rotation: Quat::IDENTITY, scale: template.scale, scroll: template.scroll };
        template.build(commands, model, prop);
        if let Some(rig) = &template.rig {
            let mut animated = template.animated(rig);
            match motion {
                Motion::Loop => {}
                Motion::Then(part) => (animated.looping, animated.queued) = (false, Some((part, true))),
                Motion::Held(part, bone) => {
                    animated.freeze(part);
                    let (_, position) = animated.pose(bone.min(rig.bones.len() - 1), 0.0);
                    inner.translation = -(axes * position * size);
                }
            }
            commands.entity(model).insert(animated);
        }
        commands.entity(model).insert(inner);
        Some(commands.spawn((at, Visibility::default())).add_child(model).id())
    }

    /// Where a model's file places it, in the frame `spawn` puts models in.
    pub fn placed(&self, name: &str) -> Vec3 {
        let axes = Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y);
        self.0.get(name).map_or(Vec3::ZERO, |template| axes * template.position * UNIT)
    }

    /// Adds a copy of a model that is drawn with another picture.
    pub fn repaint(&mut self, name: &str, copy: &str, picture: &Handle<Image>, materials: &mut Assets<StandardMaterial>) {
        let Some(template) = self.0.get(name) else { return };
        let mut repaint = |material: &Handle<StandardMaterial>| {
            let mut material = materials.get(material).cloned().unwrap_or_default();
            material.base_color_texture = Some(picture.clone());
            materials.add(material)
        };
        let parts = template
            .parts
            .iter()
            .map(|(bone, surfaces)| (*bone, surfaces.iter().map(|(mesh, material, index)| (mesh.clone(), repaint(material), *index)).collect()));
        let copied = Template { parts: parts.collect(), cycles: Vec::new(), position: template.position, rig: template.rig.clone(), scale: template.scale, scroll: template.scroll };
        self.0.insert(copy.to_string(), copied);
    }

    /// Has a model put into the world with `spawn` go on to another part of its
    /// animation: once through, or round and round.
    pub fn play(&self, model: Entity, part: usize, looping: bool, children: &Query<&Children>, animated: &mut Query<&mut Animated>) {
        for &child in children.get(model).into_iter().flatten() {
            if let Ok(mut animated) = animated.get_mut(child) {
                animated.play(part, looping);
            }
        }
    }
}

pub fn spawn_scenery(
    mut commands: Commands,
    mut world: ResMut<LoadedWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut scenery = Scenery::default();
    for mut def in std::mem::take(&mut world.props) {
        // The code puzzle's lights are worked by the puzzle.
        if crate::hazards::CODE_LIGHTS.contains(&def.name.as_str()) {
            def.cycles.clear();
        }
        // The sky's models are put round the camera (`RaceSkyState::SetPosition`), and
        // drawn large enough to be behind everything.
        let size = if def.backdrop { crate::sky::WORLD_SCALE } else { 1.0 };
        let transform = Transform {
            translation: to_world(def.position),
            rotation: basis() * def.rotation,
            scale: Vec3::splat(def.scale * UNIT * size),
        };
        let prop = Prop { position: def.position, rotation: def.rotation, scale: def.scale, scroll: def.scroll };
        let root = commands.spawn((transform, Visibility::default())).id();
        if def.backdrop {
            commands.entity(root).insert(crate::sky::Backdrop);
        }
        Template::new(&mut def, &mut meshes, &mut materials, &mut images).build(&mut commands, root, prop);
        scenery.0.insert(def.name, root);
    }
    let mut models = Models::default();
    for mut def in std::mem::take(&mut world.models) {
        let template = Template::new(&mut def, &mut meshes, &mut materials, &mut images);
        models.0.insert(def.name, template);
    }
    commands.insert_resource(scenery);
    let mut swatches = Swatches::default();
    for (name, pixels) in std::mem::take(&mut world.swatches) {
        let size = Extent3d { width: pixels.width, height: pixels.height, depth_or_array_layers: 1 };
        let image = Image::new(size, TextureDimension::D2, pixels.rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
        swatches.0.insert(name, images.add(image));
    }
    // Bricks come in each power-up's colour.
    for colour in ["p", "m", "s", "t"] {
        for (model, picture) in [("gen", "pbrick"), ("genblen", "ptrail")] {
            if let Some(picture) = swatches.0.get(&format!("{picture}{colour}")) {
                models.repaint(model, &format!("{model}-{colour}"), picture, &mut materials);
            }
        }
    }
    commands.insert_resource(models);
    commands.insert_resource(swatches);
    let emitters = std::mem::take(&mut world.emitters);
    commands.insert_resource(Emitters::new(emitters, &mut meshes, &mut materials, &mut images));
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

/// A picture that tiles, as the models' textures do.
fn repeating(pixels: &Pixels) -> Image {
    let size = Extent3d { width: pixels.width, height: pixels.height, depth_or_array_layers: 1 };
    let mut image =
        Image::new(size, TextureDimension::D2, pixels.rgba.clone(), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// Plays the material animations: each mesh shows the picture its track has reached.
pub fn cycle(
    mut commands: Commands,
    time: Res<Time>,
    requests: Query<(Entity, &Retrack)>,
    mut cycles: Query<(&mut Cycle, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs().min(0.05);
    for (prop, Retrack(swaps, looping)) in &requests {
        for (mut cycle, _) in cycles.iter_mut().filter(|c| c.0.prop == prop) {
            if let Some(&(_, to)) = swaps.iter().find(|s| s.0 == cycle.reel && s.1 < cycle.reels.len()) {
                (cycle.reel, cycle.time, cycle.looping) = (to, 0.0, *looping);
            }
        }
        commands.entity(prop).remove::<Retrack>();
    }
    for (mut cycle, material) in &mut cycles {
        cycle.time += dt;
        let reels = cycle.reels.clone();
        let reel = &reels[cycle.reel];
        if reel.pictures.is_empty() {
            continue;
        }
        // One played once holds its last picture.
        let at = if cycle.looping { cycle.time } else { cycle.time.min(reel.track.duration() - 0.001) };
        let index = reel.track.sample(&reel.frames, at).min(reel.pictures.len() - 1);
        if index != cycle.shown {
            cycle.shown = index;
            if let Some(mut material) = materials.get_mut(&material.0) {
                material.base_color_texture = Some(reel.pictures[index].clone());
            }
        }
    }
}

/// Slides the textures of props that have them moving: water, lava, flags.
pub fn scroll(time: Res<Time>, mut props: Query<(&Prop, &mut Scrolling)>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let dt = time.delta_secs().min(0.05);
    for (prop, mut scrolling) in &mut props {
        if prop.scroll == Vec2::ZERO {
            continue;
        }
        scrolling.offset = (scrolling.offset + prop.scroll * dt).fract();
        for handle in &scrolling.materials {
            if let Some(mut material) = materials.get_mut(handle) {
                material.uv_transform.translation = scrolling.offset;
            }
        }
    }
}

#[cfg(test)]
#[test]
fn power_up_models_load() {
    let Some((_, world)) = crate::world::load("RACEC0R0") else { return };
    let names: Vec<&str> = world.models.iter().map(|m| m.name.as_str()).collect();
    for name in ["grapple", "explsn", "barrel", "magnet", "magring", "insd", "curse", "cgreen", "dmissil", "shield0", "shldin3", "turbol2", "turb0f1", "gen", "enh", "brick1", "dbricks", "dtube", "warpprt"] {
        assert!(names.contains(&name), "{name} of {names:?}");
    }
    for model in &world.models {
        let untextured = model.surfaces.iter().flat_map(|s| &s.1).filter(|s| s.texture.is_none()).count();
        println!("{}: {} parts, rig {}, {untextured} untextured", model.name, model.surfaces.len(), model.rig.is_some());
    }
}

#[cfg(test)]
#[test]
fn material_animations_bind_to_models() {
    let count = |race: &str| {
        let (_, world) = crate::world::load(race)?;
        let cycles: Vec<_> = world.props.iter().flat_map(|p| p.cycles.iter().map(move |c| (p.name.as_str(), c))).collect();
        for (name, (_, start, reels)) in &cycles {
            assert!(!reels[*start].pictures.is_empty(), "{race} {name}");
        }
        Some(cycles.len())
    };
    if count("RACEC0R0").is_none() {
        return;
    }
    assert_eq!((count("RACEC0R0"), count("RACEC1R0"), count("RACEC2R0"), count("RACEC0R3")), (Some(0), Some(4), Some(12), Some(6)));
    // The sphinx starts whole, and has its falling apart to go on to.
    let (_, world) = crate::world::load("RACEC0R2").unwrap();
    let sphinx = world.props.iter().find(|p| p.name == "blowup").unwrap();
    let mut starts: Vec<usize> = sphinx.cycles.iter().map(|c| c.1).collect();
    starts.sort();
    assert_eq!(starts, [0, 1]);
    assert_eq!(sphinx.cycles[0].2.iter().map(|r| r.pictures.len()).collect::<Vec<_>>(), [1, 1, 5, 6]);
}
