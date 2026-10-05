//! The films the menus show: models moved about a set by their animations, a camera
//! that rides one of them, lights that come and go, sounds, a fade and words on the
//! screen, all to a list of what begins and ends at which frame. Follows
//! `CutsceneDefinition` (the `.CDB` file and its frames' events), `CutscenePlayer`
//! (the `.CEB` file: what the events set off), `MenuSceneScreen::SceneWidget` and,
//! for the racer the film is about, `AwardCinematicScreen::CreateWidgets`.
//!
//! The port draws its models unlit, so a film's lights are not cast: everything in
//! it is made as bright as its lights come to. Of what a `.CEB` file can set off,
//! the films the port shows use sounds, fades and words, and those are what is
//! here; pictures, sprays of particles and streamed sound are not.
//!
//! `BRICK_FILM=<folder>` shows a film of `/MENUDATA` when the menu opens: `C_AWARD1`
//! to `C_AWARD4` are those for the places of a circuit.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::{mesh::skinning::SkinnedMeshInverseBindposes, prelude::*};

use crate::assets::{
    Jam,
    adb::Animation,
    font::load_strings,
    gdb::parse_skeleton,
    lrs::Cosmetics,
    tokens::{Token, tokenize},
};
use crate::frontend::Art;
use crate::scenery::{self, Animated, Prop, PropDef, Recast, ReelDef, Rig, Scrolling};
use crate::world::Library;
use crate::{Screen, build};

const DIR: &str = "/MENUDATA";
/// The model a film has standing in for the racer it is about, and the two it shows
/// only for a racer with a peg for a leg.
const RACER: &str = "guy1";
const PEG_LEG_MODELS: [&str; 2] = ["swap", "pleg"];
const PEG_LEG: u8 = 10;
/// The looks a face has, as the films' material animations call them and as the
/// part catalogue ends the names of a face's materials.
const LOOKS: [(&str, &str); 6] = [
    ("face", "dflt"),
    ("angry", "angry"),
    ("blink", "blink"),
    ("happy", "happy"),
    ("sad", "sad"),
    ("suprz", "suprz"),
];
/// How much of a directional light's colour everything is brightened by, the port
/// having no way to cast it.
const BEAM_SHARE: f32 = 0.5;
/// The screen the films' words are placed on.
const SCREEN: Vec2 = Vec2::new(640.0, 480.0);
/// The longest step a film is run on by, as the animations are.
const LONGEST_STEP: f32 = 0.05;

/// Every `key "name" { ... }` of a file, with what is between its braces.
fn entries(tokens: &[Token], key: u16) -> Vec<(String, &[Token])> {
    let mut out = Vec::new();
    for at in 0..tokens.len().saturating_sub(2) {
        let (Token::Key(found), Token::Str(name), Token::LCurly) =
            (&tokens[at], &tokens[at + 1], &tokens[at + 2])
        else {
            continue;
        };
        if *found != key {
            continue;
        }
        let mut depth = 0;
        let end = tokens[at + 2..].iter().position(|t| {
            depth += (*t == Token::LCurly) as i32 - (*t == Token::RCurly) as i32;
            depth == 0
        });
        if let Some(end) = end {
            out.push((name.to_lowercase(), &tokens[at + 3..at + 2 + end]));
        }
    }
    out
}

/// The `n`th number after a key.
fn number(fields: &[Token], key: u16, n: usize) -> Option<f32> {
    let at = fields.iter().position(|t| *t == Token::Key(key))?;
    match fields.get(at + 1 + n)? {
        Token::Int(value) => Some(*value as f32),
        Token::Float(value) => Some(*value),
        _ => None,
    }
}

fn text(fields: &[Token], key: u16) -> Option<String> {
    let at = fields.iter().position(|t| *t == Token::Key(key))?;
    match fields.get(at + 1)? {
        Token::Str(value) => Some(value.to_lowercase()),
        _ => None,
    }
}

fn vec3(fields: &[Token], key: u16, from: usize) -> Option<Vec3> {
    Some(Vec3::new(
        number(fields, key, from)?,
        number(fields, key, from + 1)?,
        number(fields, key, from + 2)?,
    ))
}

/// The turn of something that faces along `direction` with `up` over it.
fn facing(direction: Vec3, up: Vec3) -> Quat {
    let x = direction.normalize_or(Vec3::X);
    let y = up.cross(x).normalize_or(Vec3::Y);
    Quat::from_mat3(&Mat3::from_cols(x, y, x.cross(y)))
}

/// When something of a film begins and ends, in frames, and how far it has got.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Span {
    start: u32,
    end: u32,
    state: State,
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
enum State {
    #[default]
    Waiting,
    Going,
    Over,
}

impl Span {
    fn of(fields: &[Token]) -> Span {
        let start = number(fields, 0x2b, 0).unwrap_or(0.0) as u32;
        Span {
            start,
            end: start + number(fields, 0x2c, 0).unwrap_or(0.0) as u32,
            state: State::Waiting,
        }
    }

    /// Whether it begins by this frame, having not begun.
    fn begins(&mut self, frame: u32) -> bool {
        let begins = self.state == State::Waiting && self.start <= frame;
        if begins {
            self.state = State::Going;
        }
        begins
    }

    /// Whether it ends by this frame, having begun.
    fn ends(&mut self, frame: u32) -> bool {
        let ends = self.state == State::Going && self.end <= frame;
        if ends {
            self.state = State::Over;
        }
        ends
    }
}

/// A model on the set for a while: `Frame::ModelEvent`.
struct Cue {
    model: String,
    span: Span,
    /// The part of its animation it plays, round and round.
    part: Option<usize>,
    position: Vec3,
    rotation: Quat,
    /// The material animations set going on it: the material and the track, of
    /// this world's file and this one of that file's animations.
    tracks: Vec<(usize, usize)>,
    reels: Option<(usize, usize)>,
}

/// The camera a film is seen through for a while: `Frame::CameraEvent`.
struct Shot {
    camera: String,
    span: Span,
    part: Option<usize>,
}

/// A camera of a world file: where it is and how wide it sees.
struct Lens {
    mount: Mount,
    fov: f32,
}

enum Mount {
    /// On a bone of something that moves.
    Rider(String, usize),
    /// Where it was put: its place, the way it looks and what is up to it.
    Fixed(Vec3, Vec3, Vec3),
}

/// What a film's `.CEB` file has that a mark can set off.
enum Effect {
    /// One of the film's own sounds.
    Sound(usize),
    /// A colour coming over the screen in so many seconds.
    Fade(f32, Color),
    /// Words across the screen, this far down it.
    Words(String, f32),
}

/// A film, read and ready to be put on.
pub struct Film {
    folder: String,
    /// Frames a second, and how many frames long it is.
    rate: f32,
    length: u32,
    cues: Vec<Cue>,
    shots: Vec<Shot>,
    lenses: HashMap<String, Lens>,
    /// `Frame::TransformEvent`: names with a time, which is all the films use of them.
    marks: Vec<(String, Span)>,
    /// The ambient light and the directional ones, as colours of nought to one.
    glows: Vec<(Span, Vec3)>,
    beams: Vec<(Span, Vec3)>,
    /// What a mark sets off as it begins, and what goes as it ends: the mark's
    /// name and the effect's kind and name.
    effects: HashMap<(u16, String), Effect>,
    begun: Vec<(String, u16, String)>,
    ended: Vec<(String, u16, String)>,
    props: Vec<PropDef>,
    /// Skeletons with nothing on them, for cameras to ride.
    riders: Vec<(String, Rig, Vec3, Quat)>,
}

const SOUND: u16 = 0x2f;
const FADE: u16 = 0x60;
const WORDS: u16 = 0x3f;

impl Film {
    /// The film in a folder of `/MENUDATA`, with the racer it is about made of
    /// these parts.
    pub fn load(jam: &Jam, folder: &str, cosmetics: Cosmetics) -> Option<Film> {
        let dir = format!("{DIR}/{}", folder.to_uppercase());
        let file = jam.list(&dir).find(|f| f.ends_with(".CDB"))?.to_string();
        let tokens = tokenize(jam.get(&file)?);
        let worlds: Vec<String> = scenery::names(&tokens, 0x28);
        // The first of its frames is the film; none the port shows has more.
        let frames = entries(&tokens, 0x27);
        let (_, frame) = frames.first()?;
        let mut film = Film {
            folder: dir.clone(),
            rate: number(frame, 0x3b, 0).unwrap_or(30.0),
            length: number(frame, 0x2c, 0).unwrap_or(0.0) as u32,
            cues: Vec::new(),
            shots: Vec::new(),
            lenses: HashMap::new(),
            marks: Vec::new(),
            glows: Vec::new(),
            beams: Vec::new(),
            effects: HashMap::new(),
            begun: Vec::new(),
            ended: Vec::new(),
            props: Vec::new(),
            riders: Vec::new(),
        };
        let part = |fields: &[Token]| {
            number(fields, 0x2d, 0)
                .filter(|part| *part >= 0.0)
                .map(|part| part as usize)
        };
        for (_, fields) in entries(frame, 0x29) {
            film.shots.push(Shot {
                camera: text(fields, 0x2a).unwrap_or_default(),
                span: Span::of(fields),
                part: part(fields),
            });
        }
        for (_, fields) in entries(frame, 0x2e) {
            let Some(model) = text(fields, 0x30).or_else(|| text(fields, 0x2f)) else {
                continue;
            };
            if PEG_LEG_MODELS.contains(&model.as_str()) && cosmetics.legs != PEG_LEG {
                continue;
            }
            // `[count] { world animation track material table }`, the first of them.
            let count = number(fields, 0x36, 1).unwrap_or(0.0) as usize;
            let value = |n: usize, k: usize| number(fields, 0x36, 4 + n * 5 + k).map(|v| v as usize);
            let tracks = (0..count).filter_map(|n| Some((value(n, 3)?, value(n, 2)?)));
            film.cues.push(Cue {
                model,
                span: Span::of(fields),
                part: part(fields).filter(|_| text(fields, 0x30).is_some()),
                position: vec3(fields, 0x33, 0).unwrap_or_default(),
                rotation: facing(
                    vec3(fields, 0x34, 0).unwrap_or(Vec3::X),
                    vec3(fields, 0x34, 3).unwrap_or(Vec3::Z),
                ),
                tracks: tracks.collect(),
                reels: Some((value(0, 0), value(0, 1))).and_then(|(w, a)| Some((w?, a?))),
            });
        }
        for (name, fields) in entries(frame, 0x37) {
            film.marks.push((name, Span::of(fields)));
        }
        let colour = |fields: &[Token]| vec3(fields, 0x38, 0).unwrap_or_default() / 255.0;
        for (_, fields) in entries(frame, 0x35) {
            film.glows.push((Span::of(fields), colour(fields)));
        }
        for (_, fields) in entries(frame, 0x3a) {
            film.beams.push((Span::of(fields), colour(fields)));
        }
        film.worlds(jam, &dir, &worlds, cosmetics);
        film.effects(jam, &dir);
        Some(film)
    }

    /// The models, the cameras and what they ride, from the film's world files.
    fn worlds(&mut self, jam: &Jam, dir: &str, worlds: &[String], cosmetics: Cosmetics) {
        let mut lists: Vec<&str> = jam.list(dir).collect();
        lists.retain(|file| file.ends_with(".MDB") || file.ends_with(".TDB"));
        lists.sort();
        let library = Library::new(jam, lists.iter().copied(), &[dir]);
        let files: Vec<String> = worlds
            .iter()
            .map(|world| format!("{dir}/{}.WDB", world.to_uppercase()))
            .collect();
        let mut animations: Vec<Vec<String>> = Vec::new();
        for file in &files {
            let tokens = tokenize(jam.get(file).unwrap_or_default());
            animations.push(scenery::names(&tokens, 0x3d));
            let (skeletons, moves) = (scenery::names(&tokens, 0x2c), scenery::names(&tokens, 0x29));
            let jointed = entries(&tokens, 0x2f);
            for (name, fields) in &jointed {
                // One with a model is the scenery's to load; one without is bones alone.
                if number(fields, 0x33, 0).is_some() {
                    continue;
                }
                let named = |list: &[String], n: usize| {
                    let at = number(fields, 0x2c, n)? as usize;
                    Some(list.get(at)?.to_uppercase())
                };
                let rig = (|| {
                    let bones = jam.get(&format!("{dir}/{}.SDB", named(&skeletons, 0)?))?;
                    let animation = jam.get(&format!("{dir}/{}.ADB", named(&moves, 1)?))?;
                    Some(Rig {
                        bones: Arc::new(parse_skeleton(bones)?),
                        animation: Arc::new(Animation::parse(animation)?),
                    })
                })();
                if let Some(rig) = rig {
                    let turn = facing(
                        vec3(fields, 0x32, 0).unwrap_or(Vec3::X),
                        vec3(fields, 0x32, 3).unwrap_or(Vec3::Z),
                    );
                    let at = vec3(fields, 0x31, 0).unwrap_or_default();
                    self.riders.push((name.clone(), rig, at, turn));
                }
            }
            for (name, fields) in entries(&tokens, 0x43) {
                let rider = number(fields, 0x2f, 0).and_then(|at| jointed.get(at as usize));
                let mount = match rider {
                    Some((rider, _)) => {
                        let bone = number(fields, 0x2f, 1).unwrap_or(0.0) as usize;
                        Mount::Rider(rider.clone(), bone)
                    }
                    // `GolWorldDatabase::ParseCameras`: one that rides nothing looks
                    // along X with Z beneath it unless it says otherwise.
                    None => Mount::Fixed(
                        vec3(fields, 0x31, 0).unwrap_or_default(),
                        vec3(fields, 0x32, 0).unwrap_or(Vec3::X),
                        vec3(fields, 0x32, 3).unwrap_or(Vec3::NEG_Z),
                    ),
                };
                let fov = number(fields, 0x47, 0).unwrap_or(65.0);
                self.lenses.insert(name, Lens { mount, fov });
            }
        }
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        self.props = scenery::load_files(jam, dir, &files, &library, |_, _| true);
        let racer = self.racer(jam, dir, &animations, cosmetics);
        // Each model has the tracks of whatever a cue of its will set going.
        let mut reels: HashMap<(usize, usize), Arc<Vec<ReelDef>>> = HashMap::new();
        for cue in &self.cues {
            let (Some(key), Some(prop)) = (
                cue.reels.filter(|_| cue.model != RACER || !racer),
                self.props.iter_mut().find(|prop| prop.name() == cue.model),
            ) else {
                continue;
            };
            let Some(name) = animations.get(key.0).and_then(|world| world.get(key.1)) else {
                continue;
            };
            let tracks = reels.entry(key).or_insert_with(|| {
                scenery::reels(jam, dir, &name.to_uppercase(), &library, str::to_string)
            });
            for (material, _) in &cue.tracks {
                prop.reel(*material, tracks.clone());
            }
        }
    }

    /// `AwardCinematicScreen::CreateWidgets`: the film's own figure gives way to
    /// one made of the racer's parts, on the same bones and with the same moves,
    /// whose face goes through the looks the film's does. Whether it did.
    fn racer(
        &mut self,
        jam: &Jam,
        dir: &str,
        animations: &[Vec<String>],
        cosmetics: Cosmetics,
    ) -> bool {
        let Some(at) = self.props.iter().position(|prop| prop.name() == RACER) else {
            return false;
        };
        let made = (|| {
            let catalogue = build::Catalogue::open(jam)?;
            let figure = build::figure(jam, &catalogue, cosmetics, true)?;
            let (files, folders) = build::Catalogue::files();
            let parts = Library::new(jam, files.iter().map(String::as_str), &folders);
            let face = build::face(&catalogue, cosmetics)?.to_string();
            let own = figure
                .materials
                .iter()
                .position(|material| *material == format!("{face}dflt"));
            let mut made = PropDef::made(RACER, &figure, self.props[at].rig().cloned(), &parts);
            made.stand_in(&self.props[at]);
            for cue in self.cues.iter_mut().filter(|cue| cue.model == RACER) {
                let name = cue
                    .reels
                    .and_then(|(world, animation)| animations.get(world)?.get(animation));
                let (Some(own), Some(name)) = (own, name) else {
                    cue.tracks.clear();
                    continue;
                };
                let look = |material: &str| {
                    let look = LOOKS.iter().find(|look| look.0 == material);
                    format!("{face}{}", look.map_or("dflt", |look| look.1))
                };
                let tracks = scenery::reels(jam, dir, &name.to_uppercase(), &parts, look);
                made.reel(own, tracks);
                // The film's face is whichever material its tracks are set on.
                for track in &mut cue.tracks {
                    track.0 = own;
                }
            }
            Some(made)
        })();
        match made {
            Some(made) => self.props[at] = made,
            None => return false,
        }
        true
    }

    /// What the film's marks set off, from its `.CEB` file.
    fn effects(&mut self, jam: &Jam, dir: &str) {
        let Some(file) = jam.list(dir).find(|f| f.ends_with(".CEB")) else {
            return;
        };
        let tokens = tokenize(jam.get(file).unwrap_or_default());
        let strings: Vec<Vec<String>> = scenery::names(&tokens, 0x28)
            .iter()
            .map(|table| {
                jam.get(&format!("{dir}/{}.SRF", table.to_uppercase()))
                    .map(load_strings)
                    .unwrap_or_default()
            })
            .collect();
        for (name, fields) in entries(&tokens, SOUND) {
            let sound = number(fields, 0x30, 1).unwrap_or(0.0) as usize;
            self.effects.insert((SOUND, name), Effect::Sound(sound));
        }
        for (name, fields) in entries(&tokens, FADE) {
            let seconds = number(fields, 0x61, 0).unwrap_or(0.0) / 1000.0;
            let colour = vec3(fields, 0x66, 0).unwrap_or_default() / 255.0;
            let colour = Color::srgb(colour.x, colour.y, colour.z);
            self.effects.insert((FADE, name), Effect::Fade(seconds, colour));
        }
        for (name, fields) in entries(&tokens, WORDS) {
            let table = number(fields, 0x40, 0).unwrap_or(0.0) as usize;
            let line = number(fields, 0x40, 1).unwrap_or(0.0) as usize;
            let words = strings.get(table).and_then(|table| table.get(line));
            let down = number(fields, 0x44, 0).unwrap_or(0.5);
            if let Some(words) = words {
                self.effects
                    .insert((WORDS, name), Effect::Words(words.clone(), down));
            }
        }
        // `"mark" { kind attached "effect" }`
        let bound = |key: u16| {
            entries(&tokens, key).into_iter().filter_map(|(mark, fields)| {
                let (Some(Token::Key(kind)), Some(Token::Str(effect))) =
                    (fields.first(), fields.get(2))
                else {
                    return None;
                };
                Some((mark, *kind, effect.to_lowercase()))
            })
        };
        self.begun = bound(0x56).collect();
        self.ended = bound(0x57).collect();
    }

    /// How long the film runs, in seconds.
    #[cfg(test)]
    fn seconds(&self) -> f32 {
        self.length as f32 / self.rate.max(1.0)
    }
}

/// A film asked for.
pub struct Request {
    /// Its folder in `/MENUDATA`.
    pub folder: String,
    /// The parts of the racer it is about.
    pub cosmetics: Cosmetics,
    /// Whether a key ends it (`SceneWidget::m_skippable`).
    pub skippable: bool,
    /// Which of the menus' tunes it is heard with.
    pub tune: Option<usize>,
}

impl Request {
    /// The port's own, for a race run online: the film for first place in a
    /// circuit, about whoever won the race. Any key ends it.
    pub fn winner(cosmetics: Cosmetics) -> Request {
        Request {
            skippable: true,
            ..Request::award(1, cosmetics)
        }
    }

    /// `AwardCinematicScreen`: the film for a place in a circuit, and its tune.
    pub fn award(place: usize, cosmetics: Cosmetics) -> Request {
        let (folder, tune) = match place {
            1 => ("C_AWARD1", 2),
            2 => ("C_AWARD2", 3),
            3 => ("C_AWARD3", 4),
            _ => ("C_AWARD4", 11),
        };
        Request {
            folder: folder.into(),
            cosmetics,
            skippable: false,
            tune: Some(tune),
        }
    }
}

/// The film the menus are showing, or are about to.
#[derive(Resource, Default)]
pub struct Showing {
    pub request: Option<Request>,
    playing: Option<Playing>,
}

impl Showing {
    /// Whether the screen is a film's, and not the menus'.
    pub fn busy(&self) -> bool {
        self.request.is_some() || self.playing.is_some()
    }
}

struct Playing {
    film: Film,
    seconds: f32,
    skippable: bool,
    /// The models and the camera's mounts, by name.
    cast: HashMap<String, Entity>,
    /// The cameras in use, the last to begin being the one seen through.
    shots: Vec<usize>,
    /// The ambient light there is, of the film's.
    glow: Option<usize>,
    /// How bright everything was last made.
    lit: Vec3,
    materials: Vec<Handle<StandardMaterial>>,
    /// The fade there is: how far through it is, how long it takes and its colour.
    fade: Option<(f32, f32, Color)>,
    /// The words on the screen, by their effect's name.
    words: HashMap<String, Entity>,
    /// What is laid over the film: the fade, and on it the page the words go on.
    veil: Entity,
    page: Entity,
    /// Where the camera was and how it saw, to be given back.
    camera: (Transform, Projection, ClearColorConfig),
}

/// Something of a film's, which goes when the film does.
#[derive(Component)]
struct Piece;

/// The camera the screen is seen through: not one of those that draw a minifigure
/// onto a picture for the menus, which have a layer of their own.
type Screens = (
    With<Camera3d>,
    Without<Piece>,
    Without<bevy::camera::visibility::RenderLayers>,
);

impl Playing {
    /// Takes the film off: everything of it gone, and the camera as it was.
    fn close(
        &self,
        commands: &mut Commands,
        pieces: impl Iterator<Item = Entity>,
        camera: (&mut Transform, &mut Projection, &mut Camera),
    ) {
        for piece in pieces {
            commands.entity(piece).despawn();
        }
        (*camera.0, *camera.1) = (self.camera.0, self.camera.1.clone());
        camera.2.clear_color = self.camera.2.clone();
    }
}

/// A film still showing as the menus are left is taken off.
fn leave(
    mut commands: Commands,
    mut showing: ResMut<Showing>,
    mut camera: Single<(&mut Transform, &mut Projection, &mut Camera), Screens>,
    all: Query<Entity, With<Piece>>,
) {
    showing.request = None;
    if let Some(playing) = showing.playing.take() {
        let (transform, lens, camera) = &mut *camera;
        playing.close(&mut commands, all.iter(), (&mut **transform, &mut **lens, &mut **camera));
    }
}

/// Puts a film asked for on the set: everything in it out of sight until its cue.
fn open(
    mut commands: Commands,
    mut showing: ResMut<Showing>,
    art: Option<Res<Art>>,
    mut camera: Single<(&Transform, &Projection, &mut Camera), Screens>,
    mut sound: ResMut<crate::audio::Cue>,
    (mut meshes, mut materials, mut images, mut binds): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<SkinnedMeshInverseBindposes>>,
    ),
) {
    let Some(request) = showing.request.take() else {
        return;
    };
    let Some(mut film) = art.and_then(|art| Film::load(art.jam(), &request.folder, request.cosmetics))
    else {
        warn!("no film in {}", request.folder);
        return;
    };
    // A race run mirrored leaves the world mirrored; a film's set is as it was made.
    scenery::set_mirror(false);
    let mut cast = HashMap::new();
    for def in std::mem::take(&mut film.props) {
        let name = def.name().to_string();
        let prop = scenery::spawn(def, &mut commands, &mut meshes, &mut materials, &mut images, &mut binds);
        commands.entity(prop).insert((Piece, Visibility::Hidden));
        cast.insert(name, prop);
    }
    for (name, rig, position, rotation) in std::mem::take(&mut film.riders) {
        let rider = Animated {
            rig,
            part: 0,
            time: 0.0,
            playing: true,
            looping: true,
            rate: 1.0,
            queued: None,
        };
        let prop = Prop {
            position,
            rotation,
            scale: 1.0,
            scroll: Vec2::ZERO,
        };
        let mount = (Piece, rider, prop, Transform::default(), Visibility::Hidden);
        cast.insert(name, commands.spawn(mount).id());
    }
    let whole = Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    let page = Node {
        width: Val::Px(SCREEN.x),
        height: Val::Px(SCREEN.y),
        ..default()
    };
    let page = commands.spawn(page).id();
    let veil = commands
        .spawn((Piece, whole, BackgroundColor(Color::NONE), GlobalZIndex(10)))
        .add_child(page)
        .id();
    *sound = crate::audio::Cue::Film(film.folder.clone(), request.tune);
    showing.playing = Some(Playing {
        film,
        seconds: 0.0,
        skippable: request.skippable,
        cast,
        shots: Vec::new(),
        glow: None,
        lit: Vec3::ONE,
        materials: Vec::new(),
        fade: None,
        words: HashMap::new(),
        veil,
        page,
        camera: (*camera.0, camera.1.clone(), camera.2.clear_color.clone()),
    });
    // Whatever a film doesn't show is dark.
    camera.2.clear_color = ClearColorConfig::Custom(Color::BLACK);
}

/// Runs the film on: what begins and ends by the frame it has come to, the camera,
/// the lights, the fade and the words.
fn play(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut showing: ResMut<Showing>,
    art: Option<ResMut<Art>>,
    mut sfx: ResMut<crate::audio::Sfx>,
    mut sound: ResMut<crate::audio::Cue>,
    mut camera: Single<(&mut Transform, &mut Projection, &mut Camera), Screens>,
    mut pieces: Query<(&mut Transform, &mut Visibility, &mut Prop, Option<&mut Animated>), With<Piece>>,
    scrolling: Query<&Scrolling, With<Piece>>,
    mut veils: Query<&mut BackgroundColor, With<Piece>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    all: Query<Entity, With<Piece>>,
) {
    let Some(playing) = &mut showing.playing else {
        return;
    };
    let step = time.delta_secs().min(LONGEST_STEP);
    playing.seconds += step;
    let film = &mut playing.film;
    let frame = (playing.seconds * film.rate) as u32;
    // `SceneWidget::OnKeyDown`: a key ends a film that may be ended, a second in.
    let skipped = playing.skippable
        && playing.seconds >= 1.0
        && keys.get_just_pressed().next().is_some();
    if frame >= film.length || skipped {
        let (transform, lens, camera) = &mut *camera;
        playing.close(&mut commands, all.iter(), (&mut **transform, &mut **lens, &mut **camera));
        *sound = crate::audio::Cue::Theme;
        showing.playing = None;
        return;
    }

    for cue in &mut film.cues {
        let Some(&model) = playing.cast.get(&cue.model) else {
            continue;
        };
        let Ok((mut transform, mut visibility, mut prop, animated)) = pieces.get_mut(model) else {
            continue;
        };
        if cue.span.begins(frame) {
            (prop.position, prop.rotation) = (cue.position, cue.rotation);
            let placed = scenery::placed(cue.position, cue.rotation, prop.scale);
            (transform.translation, transform.rotation) = (placed.translation, placed.rotation);
            *visibility = Visibility::Visible;
            if let (Some(part), Some(mut animated)) = (cue.part, animated) {
                animated.play(part, true);
            }
            if !cue.tracks.is_empty() {
                commands.entity(model).insert(Recast(cue.tracks.clone()));
            }
        }
        if cue.span.ends(frame) {
            *visibility = Visibility::Hidden;
        }
    }

    for (at, shot) in film.shots.iter_mut().enumerate() {
        if shot.span.begins(frame) {
            playing.shots.push(at);
            let rider = match film.lenses.get(&shot.camera).map(|lens| &lens.mount) {
                Some(Mount::Rider(rider, _)) => playing.cast.get(rider),
                _ => None,
            };
            let animated = rider.and_then(|rider| pieces.get_mut(*rider).ok()?.3);
            if let (Some(part), Some(mut animated)) = (shot.part, animated) {
                animated.play(part, true);
            }
        }
        if shot.span.ends(frame) {
            playing.shots.retain(|shot| *shot != at);
        }
    }
    // `GolCameraBase::UpdateFromTrackedEntity`: the camera is where its bone is,
    // looking along the bone. The original has the bone's up beneath the camera,
    // its screens being counted downwards.
    let seen = playing.shots.last().and_then(|at| {
        let lens = film.lenses.get(&film.shots[*at].camera)?;
        let (rider, bone) = match &lens.mount {
            Mount::Rider(rider, bone) => (rider, *bone),
            Mount::Fixed(at, forward, up) => {
                let way = |v: Vec3| scenery::to_world(v).normalize_or_zero();
                return Some((scenery::to_world(*at), way(*forward), way(*up), lens.fov));
            }
        };
        let (_, _, prop, animated) = pieces.get(*playing.cast.get(rider)?).ok()?;
        let animated = animated?;
        let bone = bone.min(animated.rig.bones.len().checked_sub(1)?);
        Some((
            animated.bone_position(prop, bone, 0.0),
            animated.bone_axis(prop, bone, Vec3::X),
            animated.bone_axis(prop, bone, Vec3::Z),
            lens.fov,
        ))
    });
    if let Some((eye, forward, up, fov)) = seen {
        *camera.0 = Transform::from_translation(eye).looking_to(forward, up);
        if let Projection::Perspective(lens) = &mut *camera.1 {
            lens.fov = fov.to_radians();
        }
    }

    for (at, (span, _)) in film.glows.iter_mut().enumerate() {
        if span.begins(frame) {
            playing.glow = Some(at);
        }
        if span.ends(frame) && playing.glow == Some(at) {
            playing.glow = None;
        }
    }
    let mut beams = Vec3::ZERO;
    for (span, colour) in &mut film.beams {
        span.begins(frame);
        span.ends(frame);
        if span.state == State::Going {
            beams += *colour;
        }
    }
    let lit = match playing.glow {
        Some(glow) => (film.glows[glow].1 + beams * BEAM_SHARE).min(Vec3::ONE),
        None if beams != Vec3::ZERO => (beams * BEAM_SHARE).min(Vec3::ONE),
        None => Vec3::ONE,
    };
    if playing.materials.is_empty() {
        let handles = scrolling.iter().flat_map(|prop| prop.materials.iter().cloned());
        playing.materials = handles.collect();
    }
    if lit != playing.lit {
        playing.lit = lit;
        for handle in &playing.materials {
            if let Some(mut material) = materials.get_mut(handle) {
                let alpha = material.base_color.alpha();
                material.base_color = Color::srgba(lit.x, lit.y, lit.z, alpha);
            }
        }
    }

    let mut art = art;
    for (name, span) in &mut film.marks {
        if span.begins(frame) {
            for (_, kind, effect) in film.begun.iter().filter(|bound| bound.0 == *name) {
                match film.effects.get(&(*kind, effect.clone())) {
                    Some(Effect::Sound(sound)) => sfx.play(crate::audio::id::AMBIENT + sound),
                    Some(Effect::Fade(seconds, colour)) => {
                        playing.fade = Some((0.0, *seconds, *colour))
                    }
                    Some(Effect::Words(words, down)) => {
                        let written = art
                            .as_mut()
                            .and_then(|art| art.write("font_ths", words, true, &mut images));
                        if let Some((picture, size)) = written {
                            // `CutsceneVisual::ComputeLayout`: across the middle.
                            let node = Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(((SCREEN.x - size.x) / 2.0).round()),
                                top: Val::Px((SCREEN.y * down).round()),
                                width: Val::Px(size.x),
                                height: Val::Px(size.y),
                                ..default()
                            };
                            let line = commands.spawn((node, ImageNode::new(picture))).id();
                            commands.entity(playing.page).add_child(line);
                            playing.words.insert(effect.clone(), line);
                        }
                    }
                    None => {}
                }
            }
        }
        if span.ends(frame) {
            for (_, kind, effect) in film.ended.iter().filter(|bound| bound.0 == *name) {
                if let Some(line) = playing.words.remove(effect) {
                    commands.entity(line).despawn();
                }
                if *kind == FADE {
                    playing.fade = None;
                }
            }
        }
    }
    // `MenuAnimationList::Entry`: the colour comes over the screen, and is gone
    // again the moment after it is whole.
    let mut veiled = Color::NONE;
    if let Some((through, seconds, colour)) = &mut playing.fade {
        veiled = colour.with_alpha((*through / seconds.max(1e-3)).min(1.0));
        if *through > *seconds {
            playing.fade = None;
        } else {
            *through += step;
        }
    }
    if let Ok(mut veil) = veils.get_mut(playing.veil) {
        veil.0 = veiled;
    }
}

/// `BRICK_FILM=<folder>`: a film to show as the menu opens.
fn asked(mut showing: ResMut<Showing>) {
    if let Ok(folder) = std::env::var("BRICK_FILM") {
        showing.request = Some(Request {
            folder,
            cosmetics: Cosmetics::default(),
            skippable: true,
            tune: None,
        });
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Showing>()
        .add_systems(OnEnter(Screen::Menu), asked)
        .add_systems(OnExit(Screen::Menu), leave)
        .add_systems(
            Update,
            (open, play, scenery::animate, scenery::cycle)
                .chain()
                .run_if(in_state(Screen::Menu)),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_place_film_is_read_whole() {
        let Some(jam) = crate::world::jam() else {
            return;
        };
        let film = Film::load(&jam, "C_AWARD1", Cosmetics::default()).unwrap();
        assert_eq!((film.rate, film.length), (30.0, 346));
        assert!((film.seconds() - 11.53).abs() < 0.01);
        // One camera for all of it, riding a bone of a skeleton with no model.
        assert_eq!(film.shots.len(), 1);
        let lens = &film.lenses[&film.shots[0].camera];
        assert!(matches!(&lens.mount, Mount::Rider(rider, 1) if rider == "dumy01"));
        assert_eq!(lens.fov.round(), 36.0);
        assert!(film.riders.iter().any(|rider| rider.0 == "dumy01"));
        // Every model a cue is for is there to be shown, the racer among them.
        assert_eq!(film.cues.len(), 68);
        for cue in &film.cues {
            let rider = film.riders.iter().any(|rider| rider.0 == cue.model);
            let prop = film.props.iter().any(|prop| prop.name() == cue.model);
            assert!(rider || prop, "{}", cue.model);
        }
        let racer = film.cues.iter().find(|cue| cue.model == RACER).unwrap();
        assert_eq!((racer.span.start, racer.span.end, racer.part), (0, 345, Some(0)));
        assert_eq!(racer.tracks.len(), 1);
        // Fireworks and flashbulbs are heard, and the screen goes black at the end.
        assert_eq!(film.marks.len(), 9);
        assert!(matches!(
            film.effects.get(&(SOUND, "flshblb4".into())),
            Some(Effect::Sound(1))
        ));
        assert!(matches!(
            film.effects.get(&(FADE, "fade".into())),
            Some(Effect::Fade(seconds, _)) if *seconds == 2.0
        ));
        assert!(film.begun.contains(&("fw".into(), SOUND, "firewrks".into())));
        assert_eq!((film.glows.len(), film.beams.len()), (26, 4));
    }

    #[test]
    fn the_other_films_of_a_circuit_are_read_and_have_their_words() {
        let Some(jam) = crate::world::jam() else {
            return;
        };
        for place in 2..=4 {
            let request = Request::award(place, Cosmetics::default());
            let film = Film::load(&jam, &request.folder, request.cosmetics).unwrap();
            assert!(film.length > 0 && !film.shots.is_empty(), "{}", request.folder);
            assert!(film.props.iter().any(|prop| prop.name() == RACER));
            // Each camera is one of the world's, and what it rides is there to ride.
            for shot in &film.shots {
                match &film.lenses[&shot.camera].mount {
                    Mount::Rider(rider, _) => {
                        let bare = film.riders.iter().any(|bare| bare.0 == *rider);
                        let prop = film.props.iter().any(|prop| prop.name() == rider);
                        assert!(bare || prop, "{rider}");
                    }
                    Mount::Fixed(..) => assert_eq!(place, 4),
                }
            }
        }
        let film = Film::load(&jam, "C_AWARD4", Cosmetics::default()).unwrap();
        assert!(matches!(
            film.effects.get(&(WORDS, "text1".into())),
            Some(Effect::Words(words, _)) if !words.is_empty()
        ));
        assert!(film.ended.iter().any(|bound| bound.1 == WORDS));
    }

    #[test]
    fn something_begins_once_and_ends_once() {
        let mut span = Span {
            start: 5,
            end: 5,
            state: State::Waiting,
        };
        assert!(!span.begins(4) && !span.ends(4));
        // Over as soon as it has begun, when it lasts no time.
        assert!(span.begins(7) && span.ends(7));
        assert!(!span.begins(8) && !span.ends(8));
    }
}
