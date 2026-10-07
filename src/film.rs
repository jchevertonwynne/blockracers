//! The films the menus show: models moved about a set by their animations, a camera
//! that rides one of them, lights that come and go, sounds, a fade and words on the
//! screen, all to a list of what begins and ends at which frame. Follows
//! `CutsceneDefinition` (the `.CDB` file and its frames' events), `CutscenePlayer`
//! (the `.CEB` file: what the events set off), `MenuSceneScreen::SceneWidget` and,
//! for the racer the film is about, `AwardCinematicScreen::CreateWidgets`.
//!
//! A film's ambient and directional lights light the models that have normals
//! (`lighting`); the rest keep the colours they were made with. Of what a `.CEB`
//! file can set off, here are sounds, fades, words, pictures, changes of a model's
//! colours and sprays of particles. No film has streamed sound, and it is not here.
//! The game's two opening films are video files, and not films of this kind.
//!
//! `BRICK_FILM=<folder>` shows a film of `/MENUDATA` when the menu opens: `C_AWARD1`
//! to `C_AWARD4` are those for the places of a circuit, `WINCAR` the one for a
//! champion's car set won (`WINCAR:c2` for the third circuit's champion),
//! `WINRRCAR` and `WINVVCAR` those for Rocket Racer's and Veronica Voltage's,
//! `CIRCUIT1` to `CIRCUIT7` those before each circuit, `CREDITS` and `LEGAL`.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::{
    asset::RenderAssetUsages,
    mesh::skinning::SkinnedMeshInverseBindposes,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::assets::{
    Jam,
    adb::Animation,
    font::load_strings,
    image::decode_bmp,
    gdb::parse_skeleton,
    lrs::Cosmetics,
    tokens::{Token, tokenize},
};
use crate::frontend::Art;
use crate::lighting::{Beam, Lights, Lit};
use crate::particles::{self, EmitterDef, Emitters, Look};
use crate::scenery::{self, Animated, Prop, PropDef, Recast, ReelDef, Rig};
use crate::world::Library;
use crate::assets::lrs::Racer;
use crate::{Screen, build, roster};

/// The part set that is Rocket Racer's, counted from the first that is won.
const ROCKET_RACERS_SET: usize = 6;

const DIR: &str = "/MENUDATA";
/// The model a film has standing in for the racer it is about, and the two it shows
/// only for a racer with a peg for a leg.
const RACER: &str = "guy1";
const PEG_LEG_MODELS: [&str; 2] = ["swap", "pleg"];
const PEG_LEG: u8 = 10;
/// The model a film has standing in for the car it is about.
const CAR: &str = "carbody";
/// The words of the film for a champion's car set that name a champion; only the
/// ones for the champion beaten are shown, whose code ends their name.
const CHAMPIONS_WORDS: [&str; 6] = ["textcr", "textkk", "textbb", "textjt", "textgm", "textbvb"];
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
/// The most directional lights a frame casts.
const MOST_BEAMS: usize = 7;
/// The screen the films' words are placed on.
const SCREEN: Vec2 = Vec2::new(640.0, 480.0);
/// Where nothing is.
const NOWHERE: Vec3 = Vec3::new(0.0, -4000.0, 0.0);
/// The longest step a film is run on by, as the animations are.
const LONGEST_STEP: f32 = 0.05;

/// Every `key "name" { ... }` of a file, with what is between its braces.
pub(crate) fn entries(tokens: &[Token], key: u16) -> Vec<(String, &[Token])> {
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
pub(crate) fn number(fields: &[Token], key: u16, n: usize) -> Option<f32> {
    let at = fields.iter().position(|t| *t == Token::Key(key))?;
    match fields.get(at + 1 + n)? {
        Token::Int(value) => Some(*value as f32),
        Token::Float(value) => Some(*value),
        _ => None,
    }
}

pub(crate) fn text(fields: &[Token], key: u16) -> Option<String> {
    let at = fields.iter().position(|t| *t == Token::Key(key))?;
    match fields.get(at + 1)? {
        Token::Str(value) => Some(value.to_lowercase()),
        _ => None,
    }
}

pub(crate) fn vec3(fields: &[Token], key: u16, from: usize) -> Option<Vec3> {
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
    /// A colour over the screen for so many seconds: coming over it, or, if it
    /// `falls`, going from it.
    Fade { seconds: f32, colour: Color, falls: bool },
    /// Words across the screen, this far down it, in a colour if they have one.
    Words(String, f32, Option<Color>),
    /// A picture in the middle of the screen, by its file.
    Picture(String),
    /// `CutsceneColorEvent`: the colours baked into a model's vertices are shifted
    /// down by bits and have an offset added, the offset changing at a rate a second.
    Colour {
        target: String,
        shifts: [u32; 3],
        offsets: [f32; 3],
        rates: [f32; 3],
    },
    /// `CutsceneAnimationEvent`: a spray of particles of one of the film's emitters,
    /// at the mark that sets it off unless it says where, or at what it names.
    Spray {
        emitter: String,
        target: Option<String>,
        position: Option<Vec3>,
        facing: Option<(Vec3, Vec3)>,
    },
}

/// A mark in a film's frame: a time, and a place that what it sets off may be put at
/// (position, the way it faces and what is up to it, in the game's axes).
struct Mark {
    name: String,
    span: Span,
    position: Vec3,
    direction: Vec3,
    up: Vec3,
}

/// A light a film turns on for a while (`AmbientLightEvent`, `DirectionalLightEvent`):
/// its colour of nought to one, which way it shines if it is a directional one and
/// the times it is on and off for if it blinks.
struct Glow {
    span: Span,
    colour: Vec3,
    direction: Vec3,
    blink: Option<(f32, f32)>,
    /// Whether it is lit at the moment, and how long that has to go.
    shown: bool,
    timer: f32,
}

impl Glow {
    fn of(fields: &[Token]) -> Glow {
        // The blink is given in frames of the original's 30 a second.
        let blink = number(fields, 0x3c, 0)
            .zip(number(fields, 0x3c, 1))
            .map(|(on, off)| (on * 0.033_333_33, off * 0.033_333_33));
        Glow {
            span: Span::of(fields),
            colour: vec3(fields, 0x38, 0).unwrap_or_default() / 255.0,
            direction: vec3(fields, 0x39, 0).unwrap_or(Vec3::NEG_Z),
            blink,
            shown: false,
            timer: 0.0,
        }
    }

    /// Runs it on by a step: whether it is on now, having begun and not ended, and
    /// blinking as the original's `Update` does.
    fn on(&mut self, frame: u32, step: f32) -> bool {
        if self.span.begins(frame) {
            self.shown = true;
            self.timer = self.blink.map_or(0.0, |blink| blink.0);
        } else if self.span.state == State::Going {
            if let Some((on, off)) = self.blink {
                if step > self.timer {
                    self.shown = !self.shown;
                    self.timer = if self.shown { on } else { off };
                } else {
                    self.timer -= step;
                }
            }
        }
        if self.span.ends(frame) {
            self.shown = false;
        }
        self.shown
    }
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
    /// `Frame::TransformEvent`: a name, a time and where the mark is.
    marks: Vec<Mark>,
    /// The emitters of the film's own animations, for its sprays.
    emitters: Vec<(String, EmitterDef, Look)>,
    /// The ambient light and the directional ones.
    glows: Vec<Glow>,
    beams: Vec<Glow>,
    /// What a mark sets off as it begins, and what goes as it ends: the mark's
    /// name and the effect's kind and name.
    effects: HashMap<(u16, String), Effect>,
    begun: Vec<(String, u16, String)>,
    ended: Vec<(String, u16, String)>,
    props: Vec<PropDef>,
    /// Skeletons with nothing on them, for cameras to ride.
    riders: Vec<(String, Rig, Vec3, Quat)>,
    /// The car the film is about, if it is about one.
    car: Option<Racer>,
    /// The materials of the champion a film is the introduction of, for the parts
    /// of a figure the film's models leave to be filled in.
    champion: Option<Vec<(&'static str, String)>>,
}

const SOUND: u16 = 0x2f;
const FADE: u16 = 0x60;
const WORDS: u16 = 0x3f;
const PICTURE: u16 = 0x4d;
const COLOUR: u16 = 0x2b;
const SPRAY: u16 = 0x3c;

impl Film {
    /// The film in a folder of `/MENUDATA`, with the racer it is about made of
    /// these parts.
    pub fn load(jam: &Jam, request: &Request) -> Option<Film> {
        let cosmetics = request.cosmetics;
        let dir = format!("{DIR}/{}", request.folder.to_uppercase());
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
            emitters: Vec::new(),
            glows: Vec::new(),
            beams: Vec::new(),
            effects: HashMap::new(),
            begun: Vec::new(),
            ended: Vec::new(),
            props: Vec::new(),
            riders: Vec::new(),
            car: None,
            champion: request.wearing.and_then(|worn| {
                let catalogue = build::Catalogue::open(jam)?;
                let part = |names: &[String], at: u8| names.get(at as usize).cloned();
                Some(vec![
                    ("face", format!("{}dflt", build::face(&catalogue, worn)?)),
                    ("torso", part(&catalogue.torsos, worn.torso)?),
                    ("legs", part(&catalogue.legs, worn.legs)?),
                ])
            }),
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
            film.marks.push(Mark {
                name,
                span: Span::of(fields),
                position: vec3(fields, 0x33, 0).unwrap_or_default(),
                direction: vec3(fields, 0x34, 0).unwrap_or(Vec3::X),
                up: vec3(fields, 0x34, 3).unwrap_or(Vec3::Z),
            });
        }
        for (_, fields) in entries(frame, 0x35) {
            film.glows.push(Glow::of(fields));
        }
        for (_, fields) in entries(frame, 0x3a) {
            film.beams.push(Glow::of(fields));
        }
        film.worlds(jam, &dir, &worlds, (!request.own).then_some(cosmetics));
        film.effects(jam, &dir);
        // `AwardCinematicScreen::CreateWidgets`: of the words that name a champion,
        // only the beaten one's are left.
        let named = request.champion.as_ref().map(|code| format!("text{code}"));
        film.effects.retain(|(kind, name), _| {
            *kind != WORDS || !CHAMPIONS_WORDS.contains(&name.as_str()) || named.as_ref() == Some(name)
        });
        film.car = request.car.clone();
        // A film about a racer's car has nothing to show for a racer without one.
        if film.car.is_none() && !request.own {
            film.cues.retain(|cue| cue.model != CAR);
        }
        Some(film)
    }

    /// The models, the cameras and what they ride, from the film's world files.
    fn worlds(&mut self, jam: &Jam, dir: &str, worlds: &[String], cosmetics: Option<Cosmetics>) {
        // A champion in a film of their own is made of the minifigures' parts, and
        // their car's wheels are the bricks' own: the materials and pictures of
        // those are with them and not with the film. What the film has of its own
        // comes after, and is what a name of both means.
        let (parts, folders) = build::Catalogue::files();
        let bricks = build::Palette::files(true);
        let mut own: Vec<&str> = jam.list(dir).collect();
        own.retain(|file| file.ends_with(".MDB") || file.ends_with(".TDB"));
        own.sort();
        let shared = parts.iter().chain(&bricks).map(String::as_str);
        let lists: Vec<&str> = shared.chain(own).collect();
        let folders = [dir, folders[0], folders[1], crate::assets::leb::DIR];
        let mut library = Library::new(jam, lists.iter().copied(), &folders);
        library.plain();
        library.dynamic();
        // What the film leaves to be filled in is the champion's own.
        if let Some(champion) = &self.champion {
            for (part, material) in champion {
                library.alias(part, material);
            }
        }
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
        let racer = cosmetics.is_some_and(|cosmetics| self.racer(jam, dir, &animations, cosmetics));
        // Each model has the tracks its cues will set going, and the pictures of
        // no others: a film's animations have a great many tracks between them.
        let mut reels: HashMap<(usize, usize, Vec<usize>), Arc<Vec<ReelDef>>> = HashMap::new();
        for prop in &mut self.props {
            let name = prop.name().to_string();
            let cues = || self.cues.iter().filter(|cue| cue.model == name);
            let mut wanted: Vec<usize> = cues().flat_map(|cue| &cue.tracks).map(|t| t.1).collect();
            wanted.sort();
            wanted.dedup();
            for cue in cues() {
                let Some((world, animation)) = cue.reels.filter(|_| cue.model != RACER || !racer)
                else {
                    continue;
                };
                let Some(name) = animations.get(world).and_then(|world| world.get(animation))
                else {
                    continue;
                };
                let key = (world, animation, wanted.clone());
                let tracks = reels.entry(key).or_insert_with(|| {
                    let name = name.to_uppercase();
                    scenery::reels(jam, dir, &name, &library, str::to_string, Some(&wanted))
                });
                for (material, _) in &cue.tracks {
                    prop.reel(*material, tracks.clone());
                }
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
            // The film's own tracks work the face, from its first look.
            let figure =
                build::figure(jam, &catalogue, Cosmetics { expression: 0, ..cosmetics }, true)?;
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
                let wanted: Vec<usize> = cue.tracks.iter().map(|track| track.1).collect();
                let name = name.to_uppercase();
                let tracks = scenery::reels(jam, dir, &name, &parts, look, Some(&wanted));
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
            // `mode on` is the colour going, and `mode off` its coming.
            let falls = fields.windows(2).any(|pair| pair == [Token::Key(0x62), Token::Key(0x63)]);
            let fade = Effect::Fade {
                seconds,
                colour,
                falls,
            };
            self.effects.insert((FADE, name), fade);
        }
        for (name, fields) in entries(&tokens, WORDS) {
            let table = number(fields, 0x40, 0).unwrap_or(0.0) as usize;
            let line = number(fields, 0x40, 1).unwrap_or(0.0) as usize;
            let words = strings.get(table).and_then(|table| table.get(line));
            let down = number(fields, 0x44, 0).unwrap_or(0.5);
            let colour = vec3(fields, 0x66, 0).map(|c| Color::srgb(c.x / 255.0, c.y / 255.0, c.z / 255.0));
            if let Some(words) = words {
                self.effects
                    .insert((WORDS, name), Effect::Words(words.clone(), down, colour));
            }
        }
        for (name, fields) in entries(&tokens, PICTURE) {
            if let Some(picture) = text(fields, PICTURE) {
                let file = format!("{dir}/{}.BMP", picture.to_uppercase());
                self.effects.insert((PICTURE, name), Effect::Picture(file));
            }
        }
        // The jointed, model and bsp names an event may act on.
        let target = |fields: &[Token]| {
            [0x5d, 0x5e, 0x5f].iter().find_map(|key| text(fields, *key))
        };
        for (name, fields) in entries(&tokens, COLOUR) {
            let each = |key: u16| [0, 1, 2].map(|n| number(fields, key, n).unwrap_or(0.0));
            let Some(target) = target(fields) else { continue };
            let effect = Effect::Colour {
                target,
                shifts: each(0x2c).map(|shift| shift as u32),
                offsets: each(0x2d),
                rates: each(0x2e),
            };
            self.effects.insert((COLOUR, name), effect);
        }
        for (name, fields) in entries(&tokens, SPRAY) {
            // `animation "emitter"`
            let at = fields.iter().position(|t| *t == Token::Key(0x3d));
            let Some(Token::Str(emitter)) = at.and_then(|at| fields.get(at + 2)) else {
                continue;
            };
            let spray = Effect::Spray {
                emitter: emitter.to_lowercase(),
                target: target(fields),
                position: vec3(fields, 0x39, 0),
                facing: vec3(fields, 0x3e, 0).zip(vec3(fields, 0x3e, 3)),
            };
            self.effects.insert((SPRAY, name), spray);
        }
        if self.effects.keys().any(|key| key.0 == SPRAY) {
            self.emitters(jam, dir, &scenery::names(&tokens, 0x27));
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

    /// The emitters of the film's animations (`CutsceneAnimation::Load`): an emitter
    /// file and a material animation of the same name, with the pictures of the
    /// film's own materials.
    fn emitters(&mut self, jam: &Jam, dir: &str, animations: &[String]) {
        let mut libraries: Vec<&str> = jam.list(dir).collect();
        libraries.retain(|file| file.ends_with(".MDB") || file.ends_with(".TDB"));
        libraries.sort();
        let library = Library::new(jam, libraries.iter().copied(), &[dir]);
        for name in animations {
            let name = name.to_uppercase();
            crate::world::add_emitters(
                jam,
                &format!("{dir}/{name}.EMB"),
                &library,
                &format!("{dir}/{name}.MAB"),
                |_| true,
                &mut self.emitters,
            );
        }
    }

    /// How long the film runs, in seconds.
    #[cfg(test)]
    fn seconds(&self) -> f32 {
        self.length as f32 / self.rate.max(1.0)
    }
}

/// A film asked for.
#[derive(Default)]
pub struct Request {
    /// Its folder in `/MENUDATA`.
    pub folder: String,
    /// The parts of the racer it is about.
    pub cosmetics: Cosmetics,
    /// Whether a key ends it (`SceneWidget::m_skippable`).
    pub skippable: bool,
    /// Which of the menus' tunes it is heard with.
    pub tune: Option<usize>,
    /// The car it is about, which goes where the film's own stands in for one.
    pub car: Option<Racer>,
    /// The champion it names in its words, by the game's code for them.
    pub champion: Option<String>,
    /// The film's own figure is who it is about, and stays.
    pub own: bool,
    /// What the film's own figures wear where their models don't say: the parts
    /// of the champion it is the introduction of.
    pub wearing: Option<Cosmetics>,
    /// Its tune goes round until the film is over.
    pub looped: bool,
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
            tune: Some(tune),
            ..default()
        }
    }

    /// `MenuManager::ProcessRecordBeaten`: the film for every record beaten, which
    /// is Veronica Voltage's, and about her.
    pub fn records() -> Request {
        Request {
            folder: "WINVVCAR".into(),
            tune: Some(15),
            own: true,
            ..default()
        }
    }

    /// `CircuitSelectScreen`, and for the last circuit `AwardCinematicScreen`: the
    /// film before a circuit is raced, by which circuit it is of the seven. The
    /// first six are their champions'; Rocket Racer's has the racer who has come
    /// to race him, and their car. Any key ends it.
    pub fn circuit(
        jam: &Jam,
        circuit: usize,
        cosmetics: Cosmetics,
        car: Option<Racer>,
    ) -> Option<Request> {
        let folder = format!("CIRCUIT{}", circuit + 1);
        let champion = roster::field(jam, &format!("c{circuit}")).into_iter().next();
        let wearing = champion.and_then(|champion| roster::cosmetics_of(jam, &champion.code));
        match circuit {
            0..=5 => Some(Request {
                folder,
                tune: Some(5 + circuit),
                skippable: true,
                own: true,
                wearing,
                ..default()
            }),
            6 => Some(Request {
                folder,
                cosmetics,
                tune: Some(12),
                skippable: true,
                car,
                wearing,
                ..default()
            }),
            _ => None,
        }
    }

    /// `SplashCinematicScreen`: the notice the original opens on, which any key ends.
    /// The port opens on its menu, and shows this only when asked (`BRICK_FILM`).
    pub fn legal() -> Request {
        Request {
            folder: "LEGAL".into(),
            skippable: true,
            own: true,
            ..default()
        }
    }

    /// `SplashCinematicScreen`: who made the game, which any key ends.
    pub fn credits() -> Request {
        Request {
            folder: "CREDITS".into(),
            tune: Some(17),
            skippable: true,
            own: true,
            looped: true,
            ..default()
        }
    }

    /// `AwardCinematicScreen::Navigate`: the film that follows a circuit won for
    /// the first time, for the part set it wins. Rocket Racer's has the racer who
    /// won it; a champion's has the champion and their car, the one of the game's
    /// quick-build cars that is theirs (`CreateWinnerCar`).
    pub fn car_set(jam: &Jam, circuit: &str, cosmetics: Cosmetics) -> Option<Request> {
        if roster::part_set(jam, circuit)? == ROCKET_RACERS_SET {
            return Some(Request {
                folder: "WINRRCAR".into(),
                cosmetics,
                tune: Some(14),
                ..default()
            });
        }
        let champion = roster::field(jam, circuit).into_iter().next()?;
        let cars = crate::garage::stock(jam);
        let own = |racer: &&Racer| {
            racer.name == "CHAMP" && racer.chassis.eq_ignore_ascii_case(&champion.chassis)
        };
        Some(Request {
            folder: "WINCAR".into(),
            cosmetics: roster::cosmetics_of(jam, &champion.code)?,
            tune: Some(13),
            car: cars.iter().find(own).cloned(),
            champion: Some(champion.code.to_lowercase()),
            ..default()
        })
    }
}

/// The film the menus are showing, or are about to.
#[derive(Resource, Default)]
pub struct Showing {
    pub request: Option<Request>,
    /// The films to show after that one, in their order.
    pub next: Vec<Request>,
    playing: Option<Playing>,
}

impl Showing {
    /// Whether the screen is a film's, and not the menus'.
    pub fn busy(&self) -> bool {
        self.request.is_some() || !self.next.is_empty() || self.playing.is_some()
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
    /// The ambient light there is, of the film's, and the directional ones cast, in
    /// the order they were turned on (`Frame::SetAmbientMaterial`, `AddLight`).
    glow: Option<usize>,
    cast_beams: Vec<usize>,
    /// The colour changes going on, and the sprays, by the effect's name.
    tints: HashMap<String, Tint>,
    sprays: HashMap<String, Entity>,
    /// The fade there is: how far through it is, how long it takes, its colour and
    /// whether the colour is going.
    fade: Option<(f32, f32, Color, bool)>,
    /// The words and pictures on the screen, by their effect's kind and name.
    words: HashMap<(u16, String), Entity>,
    /// What is laid over the film: the fade, and on it the page the words go on.
    veil: Entity,
    page: Entity,
    /// The car the film is about, which is hung the other way about from a model
    /// of the film's own.
    car: Option<Entity>,
    /// Where the camera was and how it saw, to be given back.
    camera: (Transform, Projection, ClearColorConfig),
}

/// A colour event going on (`CutsceneColorEvent`): the model's meshes with the
/// colours they had.
struct Tint {
    shifts: [u32; 3],
    offsets: [f32; 3],
    rates: [f32; 3],
    saved: Vec<(Handle<Mesh>, Vec<[f32; 4]>)>,
}

impl Tint {
    /// `GdbColoredVertexArrayBase::ApplyColorTransform`: each colour is shifted down
    /// and the offset added, to at most 255. A mesh's colours are linear, and the
    /// transform is of the 8-bit ones.
    fn apply(&self, meshes: &mut Assets<Mesh>) {
        for (handle, base) in &self.saved {
            let colours: Vec<[f32; 4]> = base
                .iter()
                .map(|colour| {
                    let mut out = *colour;
                    for n in 0..3 {
                        let eight = (colour[n].max(0.0).powf(1.0 / 2.2) * 255.0).round() as i32;
                        let moved = ((eight >> self.shifts[n]) + self.offsets[n] as i32).clamp(0, 255);
                        out[n] = (moved as f32 / 255.0).powf(2.2);
                    }
                    out
                })
                .collect();
            if let Some(mut mesh) = meshes.get_mut(handle) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
            }
        }
    }

    /// `ClearColorTransform`: the colours as they were.
    fn clear(&self, meshes: &mut Assets<Mesh>) {
        for (handle, base) in &self.saved {
            if let Some(mut mesh) = meshes.get_mut(handle) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, base.clone());
            }
        }
    }
}

/// The meshes of a model, and of what hangs on it, whose vertices have colours baked
/// in: those that the scene's lights light have none to change.
fn coloured(
    root: Entity,
    children: &Query<&Children>,
    solids: &Query<(&Mesh3d, Has<Lit>)>,
) -> Vec<Handle<Mesh>> {
    let mut found = Vec::new();
    let mut open = vec![root];
    while let Some(at) = open.pop() {
        if let Ok((mesh, false)) = solids.get(at) {
            found.push(mesh.0.clone());
        }
        open.extend(children.get(at).into_iter().flatten().copied());
    }
    found
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
    mut lights: ResMut<Lights>,
    mut camera: Single<(&mut Transform, &mut Projection, &mut Camera), Screens>,
    all: Query<Entity, With<Piece>>,
) {
    *lights = Lights::default();
    showing.request = None;
    showing.next.clear();
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
    mut camera: Single<(&mut Transform, &Projection, &mut Camera), Screens>,
    mut sound: ResMut<crate::audio::Cue>,
    (mut meshes, mut materials, mut images, mut binds): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<SkinnedMeshInverseBindposes>>,
    ),
) {
    if showing.playing.is_some() {
        return;
    }
    let queued = |showing: &mut Showing| (!showing.next.is_empty()).then(|| showing.next.remove(0));
    let Some(request) = showing.request.take().or_else(|| queued(&mut showing)) else {
        return;
    };
    let Some(art) = art else {
        showing.next.clear();
        return;
    };
    let Some(mut film) = Film::load(art.jam(), &request) else {
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
    let defs = std::mem::take(&mut film.emitters);
    if !defs.is_empty() {
        commands.insert_resource(Emitters::new(defs, &mut meshes, &mut materials, &mut images));
    }
    // `SceneEntityGroup`: the car, on its wheels and with nobody in it, where the
    // film has a box for one.
    let built = film.car.take().and_then(|racer| {
        let mut model = crate::world::load_built(art.jam(), &racer, true)?;
        model.driver.clear();
        Some((model, cast.get(CAR).copied()?))
    });
    let mut car = None;
    if let Some((model, stood_in)) = built {
        commands.entity(stood_in).despawn();
        let prop = Prop {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: 1.0,
            scroll: Vec2::ZERO,
        };
        let hung = commands
            .spawn((Piece, prop, Transform::default(), Visibility::Hidden))
            .id();
        crate::time_race::dress(&mut commands, hung, model, &mut meshes, &mut materials, &mut images, None);
        cast.insert(CAR.to_string(), hung);
        car = Some(hung);
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
            easing: None,
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
    let tune = request.tune.map(|tune| (tune, request.looped));
    *sound = crate::audio::Cue::Film(film.folder.clone(), tune);
    // A film with no camera of its own is words on a dark screen: the screen's
    // camera is turned on nothing.
    if film.shots.is_empty() {
        *camera.0 = Transform::from_translation(NOWHERE).looking_to(Vec3::NEG_Y, Vec3::Z);
    }
    showing.playing = Some(Playing {
        film,
        seconds: 0.0,
        skippable: request.skippable,
        cast,
        shots: Vec::new(),
        glow: None,
        cast_beams: Vec::new(),
        tints: HashMap::new(),
        sprays: HashMap::new(),
        fade: None,
        words: HashMap::new(),
        car,
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
    mut lights: ResMut<Lights>,
    mut veils: Query<&mut BackgroundColor, With<Piece>>,
    (mut images, mut meshes): (ResMut<Assets<Image>>, ResMut<Assets<Mesh>>),
    all: Query<Entity, With<Piece>>,
    (children, solids): (Query<&Children>, Query<(&Mesh3d, Has<Lit>)>),
    emitters: Option<Res<Emitters>>,
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
        *lights = Lights::default();
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
            if playing.car == Some(model) {
                // `time_race::dress` hangs a car on something that faces -Z; the
                // film's models face along the game's X.
                let hung = Quat::from_mat3(&Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y));
                transform.rotation = placed.rotation * hung.inverse();
                transform.scale = Vec3::ONE;
            }
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

    // `Frame::Draw`: the lights are those the frame has by now.
    for (at, glow) in film.glows.iter_mut().enumerate() {
        let was = glow.shown;
        if glow.on(frame, step) {
            // The last to be lit is the ambient light.
            if !was {
                playing.glow = Some(at);
            }
        } else if playing.glow == Some(at) {
            playing.glow = None;
        }
    }
    for (at, beam) in film.beams.iter_mut().enumerate() {
        let was = beam.shown;
        let shown = beam.on(frame, step);
        if shown && !was && !playing.cast_beams.contains(&at) {
            // Seven at most; one more puts the first out.
            if playing.cast_beams.len() >= MOST_BEAMS {
                playing.cast_beams.remove(0);
            }
            playing.cast_beams.push(at);
        } else if !shown {
            playing.cast_beams.retain(|cast| *cast != at);
        }
    }
    let now = Lights {
        ambient: playing.glow.map(|at| film.glows[at].colour),
        beams: playing
            .cast_beams
            .iter()
            .map(|at| Beam {
                direction: film.beams[*at].direction,
                colour: film.beams[*at].colour,
            })
            .collect(),
    };
    if *lights != now {
        *lights = now;
    }

    let mut art = art;
    for mark in &mut film.marks {
        let name = &mark.name;
        if mark.span.begins(frame) {
            for (_, kind, effect) in film.begun.iter().filter(|bound| bound.0 == *name) {
                match film.effects.get(&(*kind, effect.clone())) {
                    Some(Effect::Colour { target, shifts, offsets, rates }) => {
                        if let (false, Some(&model)) =
                            (playing.tints.contains_key(effect), playing.cast.get(target))
                        {
                            let saved = coloured(model, &children, &solids)
                                .into_iter()
                                .filter_map(|handle| {
                                    let base = meshes.get(&handle)?.attribute(Mesh::ATTRIBUTE_COLOR)?;
                                    let bevy::mesh::VertexAttributeValues::Float32x4(base) = base else {
                                        return None;
                                    };
                                    Some((handle.clone(), base.clone()))
                                })
                                .collect();
                            let tint = Tint { shifts: *shifts, offsets: *offsets, rates: *rates, saved };
                            tint.apply(&mut meshes);
                            playing.tints.insert(effect.clone(), tint);
                        }
                    }
                    Some(Effect::Spray { emitter, target, position, facing: way }) => {
                        let Some(emitters) = &emitters else { continue };
                        if playing.sprays.contains_key(effect) {
                            continue;
                        }
                        // `CutsceneAnimationEvent::StartAt`: where it says, else where the
                        // model it names is, else where the mark is.
                        let model = target.as_ref().and_then(|target| {
                            let (transform, ..) = pieces.get(*playing.cast.get(target)?).ok()?;
                            Some(*transform)
                        });
                        let at = position.unwrap_or(mark.position);
                        let (direction, up) = way.unwrap_or((mark.direction, mark.up));
                        let turn = scenery::basis() * facing(direction, up) * scenery::basis().inverse();
                        let mut place = Transform::from_translation(scenery::to_world(at)).with_rotation(turn);
                        if let (None, Some(model)) = (position, model) {
                            place.translation = model.translation;
                        }
                        if let Some(spray) = emitters.spawn(&mut commands, emitter, place) {
                            commands.entity(spray).insert(Piece);
                            // One that ends itself can't be stopped.
                            if emitters.persistent(emitter) {
                                playing.sprays.insert(effect.clone(), spray);
                            }
                        }
                    }
                    Some(Effect::Sound(sound)) => sfx.play(crate::audio::id::AMBIENT + sound),
                    Some(Effect::Fade {
                        seconds,
                        colour,
                        falls,
                    }) => playing.fade = Some((0.0, *seconds, *colour, *falls)),
                    Some(Effect::Words(words, down, colour)) => {
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
                            let mut picture = ImageNode::new(picture);
                            if let Some(colour) = colour {
                                picture.color = *colour;
                            }
                            let line = commands.spawn((node, picture)).id();
                            commands.entity(playing.page).add_child(line);
                            playing.words.insert((*kind, effect.clone()), line);
                        }
                    }
                    Some(Effect::Picture(file)) => {
                        let pixels = art
                            .as_ref()
                            .and_then(|art| decode_bmp(art.jam().get(file)?, None));
                        if let Some(pixels) = pixels {
                            // `CutsceneVisual::ComputeLayout`: in the middle, as
                            // large as it is.
                            let size = Vec2::new(pixels.width as f32, pixels.height as f32);
                            let corner = ((SCREEN - size) / 2.0).round();
                            let node = Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(corner.x),
                                top: Val::Px(corner.y),
                                width: Val::Px(size.x),
                                height: Val::Px(size.y),
                                ..default()
                            };
                            let extent = Extent3d {
                                width: pixels.width,
                                height: pixels.height,
                                depth_or_array_layers: 1,
                            };
                            let picture = images.add(Image::new(
                                extent,
                                TextureDimension::D2,
                                pixels.rgba,
                                TextureFormat::Rgba8UnormSrgb,
                                RenderAssetUsages::default(),
                            ));
                            let shown = commands.spawn((node, ImageNode::new(picture))).id();
                            commands.entity(playing.page).add_child(shown);
                            playing.words.insert((*kind, effect.clone()), shown);
                        }
                    }
                    None => {}
                }
            }
        }
        if mark.span.ends(frame) {
            for (_, kind, effect) in film.ended.iter().filter(|bound| bound.0 == *name) {
                if let Some(tint) = playing.tints.remove(effect) {
                    tint.clear(&mut meshes);
                }
                if let Some(spray) = playing.sprays.remove(effect) {
                    commands.entity(spray).despawn();
                }
                if let Some(line) = playing.words.remove(&(*kind, effect.clone())) {
                    commands.entity(line).despawn();
                }
                if *kind == FADE {
                    playing.fade = None;
                }
            }
        }
    }
    // `CutsceneColorEvent::Update`: an offset that has a rate moves on, when it has
    // moved by a whole step of a colour.
    for tint in playing.tints.values_mut() {
        let by = tint.rates.map(|rate| rate * step);
        if by.iter().any(|by| *by as i32 != 0) {
            for n in 0..3 {
                tint.offsets[n] += by[n];
            }
            tint.apply(&mut meshes);
        }
    }
    // `MenuAnimationList::Entry`: the colour comes over the screen or goes from it,
    // and is gone the moment after it has.
    let mut veiled = Color::NONE;
    if let Some((through, seconds, colour, falls)) = &mut playing.fade {
        let part = (*through / seconds.max(1e-3)).min(1.0);
        veiled = colour.with_alpha(if *falls { 1.0 - part } else { part });
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
    let Ok(asked) = std::env::var("BRICK_FILM") else {
        return;
    };
    let (folder, circuit) = asked.split_once(':').unwrap_or((&asked, "c0"));
    // The films about a car set are about a circuit's champion too.
    let circuit = if folder == "WINRRCAR" { "c6" } else { circuit };
    let about = match folder {
        "WINCAR" | "WINRRCAR" => crate::world::jam()
            .and_then(|jam| Request::car_set(&jam, circuit, Cosmetics::default())),
        "WINVVCAR" => Some(Request::records()),
        "LEGAL" => Some(Request::legal()),
        circuit if circuit.starts_with("CIRCUIT") => {
            let number = circuit["CIRCUIT".len()..].parse::<usize>().unwrap_or(1);
            // The last is about a racer and their car: the first of the game's own.
            let jam = crate::world::jam();
            let car = jam.as_ref().and_then(|jam| crate::garage::stock(jam).into_iter().next());
            let cosmetics = car.as_ref().map_or(Cosmetics::default(), |car| car.cosmetics);
            jam.and_then(|jam| Request::circuit(&jam, number.saturating_sub(1), cosmetics, car))
        }
        "CREDITS" => Some(Request::credits()),
        _ => None,
    };
    showing.request = Some(Request {
        folder: folder.to_string(),
        skippable: true,
        tune: None,
        ..about.unwrap_or_default()
    });
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Showing>()
        .add_systems(OnEnter(Screen::Menu), asked)
        .add_systems(OnExit(Screen::Menu), leave)
        .add_systems(
            Update,
            (
                open,
                play,
                scenery::animate,
                scenery::cycle,
                particles::emit,
                particles::particles,
            )
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
        let film = Film::load(&jam, &Request::award(1, Cosmetics::default())).unwrap();
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
            Some(Effect::Fade { seconds, falls: false, .. }) if *seconds == 2.0
        ));
        assert!(film.begun.contains(&("fw".into(), SOUND, "firewrks".into())));
        assert_eq!((film.glows.len(), film.beams.len()), (26, 4));
    }

    #[test]
    fn the_third_circuit_s_film_has_lights_colour_events_and_sprays() {
        let Some(jam) = crate::world::jam() else {
            return;
        };
        let racer = crate::garage::stock(&jam).into_iter().next().unwrap();
        let request = Request::circuit(&jam, 2, racer.cosmetics, None).unwrap();
        let film = Film::load(&jam, &request).unwrap();
        // Thirteen ambient lights that make the lightning, and the one beam.
        assert_eq!((film.glows.len(), film.beams.len()), (13, 1));
        assert_eq!(film.beams[0].colour, Vec3::new(232.0, 206.0, 184.0) / 255.0);
        let colours = film.effects.keys().filter(|key| key.0 == COLOUR).count();
        let sprays = film.effects.keys().filter(|key| key.0 == SPRAY).count();
        assert_eq!((colours, sprays), (4, 3));
        assert!(film.emitters.iter().any(|emitter| emitter.0 == "bubbles"));
        // No film's `.CEB` has any streamed sound (kind 0x36).
        let mut files: Vec<String> = Vec::new();
        for dir in ["CIRCUIT1", "CIRCUIT2", "CIRCUIT3", "CIRCUIT4", "CIRCUIT5", "CIRCUIT6", "CIRCUIT7", "CREDITS", "C_AWARD1", "C_AWARD2", "C_AWARD3", "C_AWARD4", "LEGAL", "SINGRACE", "WINCAR", "WINRRCAR", "WINVVCAR"] {
            files.extend(jam.list(&format!("{DIR}/{dir}")).filter(|f| f.ends_with(".CEB")).map(String::from));
        }
        assert_eq!(files.len(), 17);
        for file in files {
            let tokens = tokenize(jam.get(&file).unwrap());
            assert!(entries(&tokens, 0x36).is_empty(), "{file}");
        }
    }

    #[test]
    fn the_other_films_of_a_circuit_are_read_and_have_their_words() {
        let Some(jam) = crate::world::jam() else {
            return;
        };
        for place in 2..=4 {
            let request = Request::award(place, Cosmetics::default());
            let film = Film::load(&jam, &request).unwrap();
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
        let film = Film::load(&jam, &Request::award(4, Cosmetics::default())).unwrap();
        assert!(matches!(
            film.effects.get(&(WORDS, "text1".into())),
            Some(Effect::Words(words, ..)) if !words.is_empty()
        ));
        assert!(film.ended.iter().any(|bound| bound.1 == WORDS));
    }

    #[test]
    fn a_car_set_won_has_the_champion_s_film_or_rocket_racer_s() {
        let Some(jam) = crate::world::jam() else {
            return;
        };
        let racer = Cosmetics {
            hat: 4,
            ..default()
        };
        // Each of the six champions stands by their own car, under their own name.
        for (circuit, code) in ["cr", "kk", "bb", "jt", "bvb", "gm"].iter().enumerate() {
            let request = Request::car_set(&jam, &format!("c{circuit}"), racer).unwrap();
            assert_eq!(request.folder, "WINCAR");
            assert_eq!(request.champion.as_deref(), Some(*code));
            assert_eq!(Some(request.cosmetics), roster::cosmetics_of(&jam, code));
            assert!(request.car.as_ref().is_some_and(|car| car.name == "CHAMP"));
            let film = Film::load(&jam, &request).unwrap();
            assert!(film.car.is_some() && film.cues.iter().any(|cue| cue.model == CAR));
            let words = film.effects.keys().filter(|(kind, _)| *kind == WORDS);
            let mut words: Vec<&str> = words.map(|(_, name)| name.as_str()).collect();
            words.sort();
            assert_eq!(words, ["text1", &format!("text{code}")]);
        }
        // Rocket Racer's is about the racer who won it, and has no car to put in.
        let request = Request::car_set(&jam, "c6", racer).unwrap();
        assert_eq!((request.folder.as_str(), request.cosmetics), ("WINRRCAR", racer));
        assert!(request.car.is_none() && request.champion.is_none());
        let film = Film::load(&jam, &request).unwrap();
        assert_eq!((film.length, film.shots.len()), (1100, 3));
        assert!(film.props.iter().any(|prop| prop.name() == RACER));
        assert_eq!(film.effects.keys().filter(|key| key.0 == WORDS).count(), 7);
    }

    #[test]
    fn veronica_voltage_s_film_is_her_own_and_the_credits_are_words() {
        let Some(jam) = crate::world::jam() else {
            return;
        };
        // Her film keeps the figure it was made with, and her car is a model of it.
        let film = Film::load(&jam, &Request::records()).unwrap();
        let racer = film.cues.iter().find(|cue| cue.model == RACER).unwrap();
        assert!(!racer.tracks.is_empty() && film.car.is_none());
        assert!(film.props.iter().any(|prop| prop.name() == "vv_car"));
        assert!(!film.shots.is_empty());
        // The credits have no camera and nothing to look at: pages of words, each
        // coming out of the dark and going back into it.
        let credits = Film::load(&jam, &Request::credits()).unwrap();
        assert_eq!((credits.length, credits.shots.len(), credits.cues.len()), (3510, 0, 0));
        assert!((credits.seconds() - 117.0).abs() < 0.01);
        let fade = |name: &str| match credits.effects.get(&(FADE, name.into())) {
            Some(Effect::Fade { seconds, falls, .. }) => Some((*seconds, *falls)),
            _ => None,
        };
        assert_eq!((fade("transin"), fade("transout")), (Some((1.0, true)), Some((1.0, false))));
        let coloured = |effect: &Effect| matches!(effect, Effect::Words(_, _, Some(_)));
        assert_eq!(credits.effects.values().filter(|effect| coloured(effect)).count(), 81);
    }

    #[test]
    fn every_circuit_has_its_film_and_the_game_its_notice() {
        let Some(jam) = crate::world::jam() else {
            return;
        };
        let racer = crate::garage::stock(&jam).into_iter().next().unwrap();
        for circuit in 0..7 {
            let car = Some(racer.clone());
            let request = Request::circuit(&jam, circuit, racer.cosmetics, car).unwrap();
            assert_eq!(request.folder, format!("CIRCUIT{}", circuit + 1));
            // The first six are the champions' own; the last has the racer in it.
            assert_eq!((request.own, request.skippable), (circuit < 6, true));
            let film = Film::load(&jam, &request).unwrap();
            assert!(film.length > 300 && !film.shots.is_empty(), "{}", request.folder);
            assert_eq!(film.champion.as_ref().map(Vec::len), Some(3));
            for cue in &film.cues {
                let rider = film.riders.iter().any(|rider| rider.0 == cue.model);
                let prop = film.props.iter().any(|prop| prop.name() == cue.model);
                assert!(rider || prop, "{} {}", request.folder, cue.model);
            }
            assert_eq!(film.car.is_some(), circuit == 6);
        }
        assert!(Request::circuit(&jam, 7, racer.cosmetics, None).is_none());
        // The notice is a picture that comes out of the dark and goes back into it.
        let notice = Film::load(&jam, &Request::legal()).unwrap();
        assert_eq!((notice.length, notice.marks.len()), (150, 3));
        assert!(matches!(
            notice.effects.get(&(PICTURE, "splash1".into())),
            Some(Effect::Picture(file)) if jam.get(file).is_some()
        ));
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
