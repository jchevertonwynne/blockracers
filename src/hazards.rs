//! The circuits' hazards (`.HZB`): swinging hammers, a crane, cannons, a ghost, lava,
//! doors that open when shot, and the rest. Each follows its class in the original's
//! `race/hazards`; what they look like comes from the circuit's own animated models.
//!
//! Not here: the crane's shadow on the road, the ghost's three after-images, and the
//! solid box a rolling rock is in the original (here it is a ball).

use crate::assets::{
    Jam,
    tokens::{Token, tokenize},
};
use crate::audio::{Emitter, Sfx, id};
use crate::events::TrackEvents;
use crate::items::WARP_TIME;
use crate::items::{Action, Beam, ItemAssets};
use crate::kart::{Kart, PLAYER_SLOT};
use crate::particles::{Emitter as Particles, Emitters};
use crate::physics::{self, UNIT};
use crate::scenery::{Animated, Fade, Prop, Retrack, Scenery, Scrolling, Swatches, to_world};
use crate::track::Track;
use crate::{Phase, Race};
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

/// How close to a kart's middle counts as touching it, in game units.
const KART_RADIUS: f32 = 5.0;
/// Hazards' own looping sounds are numbered from here.
const LOOP_SLOT: u16 = 2000;
/// Force, in the original's units, to acceleration in ours (every car weighs 4500).
const FORCE: f32 = 1000.0 / 4500.0 * UNIT;

// Sounds from the circuits' own banks.
const CRANE_SOUND: usize = id::AMBIENT + 3;
const LAVA_SOUND: usize = id::AMBIENT + 2;
const GHOST_LOOP: usize = id::AMBIENT + 12;
const GHOST_NEAR: usize = id::AMBIENT + 13;
const GHOST_HIT: usize = id::AMBIENT + 15;

/// Materials swapped onto models: the code puzzle's lights show red where the first
/// pad of a pair is the right one and blue where the second is.
pub const SWATCHES: [&str; 2] = ["mmredco", "mmblueco"];
/// How long the sphinx takes to blow up: the length of its material animation.
const SPHINX_BLOW_UP: f32 = 18.0 / 30.0;
pub const CODE_LIGHTS: [&str; 3] = ["mmcode1", "mmcode2", "mmcode3"];
/// How fast each light flickers while the doors are open, in changes a second.
const CODE_FLICKER: [f32; 3] = [3.0, 4.0, 5.0];

/// Where the lava leaps between.
const LAVA_POOLS: [Vec3; 3] = [
    Vec3::new(577.0, -444.0, 7.334),
    Vec3::new(605.0, -500.0, 19.0),
    Vec3::new(525.0, -505.0, 4.6),
];
/// The frames of the lava's animation at which it leaves each pool and lands in the next.
const LAVA_LEAVES: [f32; 3] = [0.0, 61.0, 119.0];
const LAVA_LANDS: [f32; 3] = [26.0, 86.0, 146.0];
const CURSE_DROPS: [Vec3; 3] = [
    Vec3::new(-357.5827, 458.127, -11.665112),
    Vec3::new(-351.20313, 518.713, -94.21568),
    Vec3::new(-362.25818, 397.30392, -94.95952),
];

/// Where a warp pad delivers a car, and the way it leaves it facing.
const WARP_PAD_TO: Vec3 = Vec3::new(132.673, 86.304, 14.722);
const WARP_PAD_FACING: Vec3 = Vec3::new(1.0, -0.5, 0.0);
/// How far below their bone the rain of cannon balls starts, along the bone's own axis.
const RAIN_DROP: f32 = 35.0;
/// Smoke rises from a pool of lava this often while the lava is in the air, and the
/// lava's leaving and landing are noticed for this many frames.
const LAVA_SMOKE: f32 = 0.8;
const LAVA_WINDOW: f32 = 10.0;

/// What a hazard is and how it stands. The fields that say how it stands start out
/// as `parse` leaves them.
pub enum Kind {
    /// Sounds its blow as it comes down.
    Hammer {
        raised: bool,
    },
    /// A solid lump riding on a bone of an animated model.
    /// `start` is how far into its animation it begins, in frames.
    RollingRock {
        prop: String,
        radius: f32,
        start: f32,
        last: Option<Vec3>,
    },
    /// Models that play once and vanish, opening a surface as they go.
    TriggeredAnimation {
        surface: String,
        props: Vec<String>,
    },
    Crane {
        pending: bool,
    },
    /// Fires a cannon ball from one of its sources at one of its targets when its
    /// event starts near enough.
    /// One with several of each (`multi`) also sets off the target's own event once
    /// its shot is spent: `landing`, until then.
    Launcher {
        sources: Vec<(Vec3, i32)>,
        targets: Vec<(Vec3, i32)>,
        near: Option<(Vec3, f32)>,
        event: i32,
        ball: Option<Entity>,
        multi: bool,
        landing: Option<(i32, Vec3)>,
    },
    /// `landed` until its collision can be put in: see `hazards`.
    FallingPillar {
        fallen: bool,
        landed: bool,
    },
    Sphinx {
        blowing: f32,
    },
    LavaGeyser {
        cooldown: f32,
        flying: bool,
        smoke: f32,
    },
    /// Three two-way choices to get right in order; the right ones change each time.
    CodePuzzle {
        code: [bool; 3],
        progress: u8,
        opened: f32,
    },
    /// A force field that only the shielded can pass.
    Rocket {
        open: bool,
    },
    Ghost {
        search: f32,
        waver: f32,
        depth: f32,
        trail: Option<Entity>,
    },
    CannonballRain {
        prop: String,
        interval: f32,
        timer: f32,
    },
    CurseDrop,
    SweepCannon {
        prop: Option<String>,
        source: Vec3,
        period: f32,
        sweep: [f32; 3],
        time: f32,
        cooldown: f32,
        beam: Option<Entity>,
    },
    Grabber {
        prop: String,
        strength: f32,
        frames: (f32, f32),
        held: f32,
        rest: f32,
    },
    WarpPad,
    /// Snow falling ahead of the camera.
    Snowfall {
        emitter: Option<Entity>,
    },
    SmokeVent {
        emitter: Option<Entity>,
    },
    /// Water whose texture sloshes back and forth.
    Oscillator {
        prop: String,
        amplitude: Vec2,
        time: f32,
    },
    Unknown,
}

/// Where around the vent its smoke comes out.
const SMOKE_OFFSETS: [Vec3; 4] = [
    Vec3::new(-24.45, 26.74, -19.56),
    Vec3::new(-35.72, 9.41, -18.41),
    Vec3::new(-6.9, -9.13, -15.49),
    Vec3::new(4.37, 8.54, -16.65),
];
/// Snow starts this far ahead of the camera, this far up and up to this far to
/// either side.
const SNOW_AHEAD: f32 = 100.0;
const SNOW_ABOVE: f32 = 40.0;
const SNOW_SPREAD: u32 = 200;
/// The water's slosh takes this long to come round.
const OSCILLATOR_PERIOD: f32 = 10.0;

struct Hazard {
    /// The event that sets it going and stops it.
    trigger: i32,
    active: bool,
    kind: Kind,
}

/// The places the original has written into its hazards rather than its circuits'
/// files, in the game's coordinates. A circuit of the port's own has others.
pub struct Places {
    pub lava_pools: [Vec3; 3],
    pub curse_drops: [Vec3; 3],
    /// Where a warp pad delivers a car, and the way it leaves it facing.
    pub warp_to: Vec3,
    pub warp_facing: Vec3,
}

impl Default for Places {
    fn default() -> Self {
        Places {
            lava_pools: LAVA_POOLS,
            curse_drops: CURSE_DROPS,
            warp_to: WARP_PAD_TO,
            warp_facing: WARP_PAD_FACING,
        }
    }
}

#[derive(Resource, Default)]
pub struct Hazards {
    all: Vec<Hazard>,
    /// Set once they have been put in their starting state.
    ready: bool,
    places: Places,
}

impl Hazards {
    /// Hazards put together by hand: each with the event that sets it going.
    pub fn of(all: Vec<(i32, Kind)>, places: Places) -> Self {
        let all = all
            .into_iter()
            .map(|(trigger, kind)| Hazard {
                trigger,
                active: false,
                kind,
            })
            .collect();
        Hazards {
            all,
            ready: false,
            places,
        }
    }
}

fn number(token: Option<&Token>) -> f32 {
    match token {
        Some(Token::Float(v)) => *v,
        Some(Token::Int(v)) => *v as f32,
        _ => 0.0,
    }
}

/// What follows `key` among a hazard's fields.
fn after(fields: &[Token], key: u16) -> &[Token] {
    fields
        .iter()
        .position(|t| *t == Token::Key(key))
        .map_or(&[], |at| &fields[at + 1..])
}

fn vec3(tokens: &[Token]) -> Vec3 {
    Vec3::new(
        number(tokens.first()),
        number(tokens.get(1)),
        number(tokens.get(2)),
    )
}

fn strings(fields: &[Token], key: u16) -> Vec<String> {
    let named = |(i, t): (usize, &Token)| match (t, fields.get(i + 1)) {
        (Token::Key(k), Some(Token::Str(name))) if *k == key => Some(name.to_lowercase()),
        _ => None,
    };
    fields.iter().enumerate().filter_map(named).collect()
}

/// A `[count] { x y z event ... }` list of places.
fn places(tokens: &[Token]) -> Vec<(Vec3, i32)> {
    let count = number(tokens.get(1)) as usize;
    (0..count)
        .map(|i| {
            (
                to_world(vec3(&tokens[(4 + i * 4).min(tokens.len())..])),
                number(tokens.get(7 + i * 4)) as i32,
            )
        })
        .collect()
}

fn launcher(
    fields: &[Token],
    sources: Vec<(Vec3, i32)>,
    targets: Vec<(Vec3, i32)>,
    multi: bool,
) -> Hazard {
    let near = Some((
        to_world(vec3(after(fields, 0x39))),
        number(after(fields, 0x3a).first()) * UNIT,
    ));
    let event = number(after(fields, 0x3b).first()) as i32;
    Hazard {
        trigger: -1,
        active: false,
        kind: Kind::Launcher {
            sources,
            targets,
            near,
            event,
            ball: None,
            multi,
            landing: None,
        },
    }
}

fn parse(tokens: &[Token]) -> Vec<Hazard> {
    let mut hazards = Vec::new();
    // Past `hazards [count] {`; each hazard is a keyword and perhaps a braced body.
    let mut at = 5;
    while let Some(Token::Key(kind)) = tokens.get(at) {
        let mut end = at + 1;
        let mut depth = 0;
        while let Some(token) = tokens.get(end) {
            match token {
                Token::LCurly => depth += 1,
                Token::RCurly if depth == 0 => break,
                Token::RCurly => depth -= 1,
                Token::Key(_) if depth == 0 => break,
                _ => {}
            }
            end += 1;
        }
        let fields = &tokens[at + 1..end];
        at = end;
        let name = |key: u16| strings(fields, key).into_iter().next().unwrap_or_default();
        let trigger = number(after(fields, 0x3b).first()) as i32;
        let (trigger, kind) = match kind {
            0x28 => (
                10,
                Kind::FallingPillar {
                    fallen: false,
                    landed: false,
                },
            ),
            0x29 => (12, Kind::Sphinx { blowing: 0.0 }),
            0x2a => (50, Kind::Hammer { raised: true }),
            0x2b => (
                10,
                Kind::Ghost {
                    search: 0.0,
                    waver: 0.0,
                    depth: 0.0,
                    trail: None,
                },
            ),
            0x2c => (
                0,
                Kind::LavaGeyser {
                    cooldown: 0.0,
                    flying: false,
                    smoke: 0.0,
                },
            ),
            0x2d => (
                -1,
                Kind::CodePuzzle {
                    code: [false; 3],
                    progress: 0,
                    opened: 0.0,
                },
            ),
            0x2e => (1, Kind::Rocket { open: false }),
            0x2f => (-1, Kind::Snowfall { emitter: None }),
            0x30 => (10, Kind::SmokeVent { emitter: None }),
            0x36 => {
                let prop = match fields.first() {
                    Some(Token::Str(name)) => name.to_lowercase(),
                    _ => String::new(),
                };
                let amplitude = Vec2::new(number(fields.get(1)), number(fields.get(2)));
                (
                    -1,
                    Kind::Oscillator {
                        prop,
                        amplitude,
                        time: 0.0,
                    },
                )
            }
            0x32 => (1, Kind::Crane { pending: true }),
            0x33 => {
                let (source, target) = (
                    to_world(vec3(after(fields, 0x37))),
                    to_world(vec3(after(fields, 0x38))),
                );
                hazards.push(launcher(
                    fields,
                    vec![(source, 6)],
                    vec![(target, 7)],
                    false,
                ));
                continue;
            }
            0x34 => (
                trigger,
                Kind::TriggeredAnimation {
                    surface: name(0x41),
                    props: strings(fields, 0x42),
                },
            ),
            0x3d => {
                let inner = after(fields, 0x33);
                hazards.push(launcher(
                    inner,
                    places(after(fields, 0x37)),
                    places(after(fields, 0x38)),
                    true,
                ));
                continue;
            }
            0x3e => {
                let prop = match fields.get(1) {
                    Some(Token::Str(name)) => name.to_lowercase(),
                    _ => String::new(),
                };
                let size = vec3(&fields[4.min(fields.len())..]);
                let radius = (size.x + size.y + size.z) / 6.0 * UNIT;
                (
                    number(fields.get(2)) as i32,
                    Kind::RollingRock {
                        prop,
                        radius,
                        start: number(fields.get(3)),
                        last: None,
                    },
                )
            }
            0x3f => (8, Kind::CurseDrop),
            0x40 => {
                let sweep = after(fields, 0x47);
                let kind = Kind::SweepCannon {
                    prop: strings(fields, 0x42).into_iter().next(),
                    source: vec3(after(fields, 0x37)),
                    period: number(after(fields, 0x46).first()) / 1000.0,
                    sweep: [
                        number(sweep.first()),
                        number(sweep.get(1)),
                        number(sweep.get(2)),
                    ],
                    time: 0.0,
                    cooldown: 0.0,
                    beam: None,
                };
                (trigger, kind)
            }
            0x43 => {
                let interval = number(after(fields, 0x44).first()) / 1000.0;
                (
                    trigger,
                    Kind::CannonballRain {
                        prop: name(0x42),
                        interval,
                        timer: 0.0,
                    },
                )
            }
            0x48 => {
                let values = &after(fields, 0x42)[1.min(after(fields, 0x42).len())..];
                let frames = (number(values.get(1)), number(values.get(2)));
                (
                    trigger,
                    Kind::Grabber {
                        prop: name(0x42),
                        strength: number(values.first()),
                        frames,
                        held: 0.0,
                        rest: 0.0,
                    },
                )
            }
            0x49 => (0, Kind::WarpPad),
            _ => (-1, Kind::Unknown),
        };
        hazards.push(Hazard {
            trigger,
            active: false,
            kind,
        });
    }
    hazards
}

/// Loads the hazards of a race (a folder name such as `RACEC0R0`).
pub fn load(race: &str) -> Option<Hazards> {
    let jam = Jam::open(
        std::env::var("BRICK_JAM").unwrap_or("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM".into()),
    )?;
    Some(Hazards {
        all: parse(&tokenize(
            jam.get(&format!("/GAMEDATA/{race}/HAZARDS.HZB"))?,
        )),
        ..default()
    })
}

/// Spins a kart round once, as most hazards do to whoever touches them.
fn spin(k: &mut Kart) -> bool {
    let fresh = k.spin <= 0.0 && k.vel.length_squared() > 0.01;
    if fresh {
        k.spin = TAU / physics::SPIN_RATE;
    }
    fresh
}

fn touching(k: &Kart, at: Vec3, radius: f32) -> bool {
    (k.pos + Vec3::Y * physics::BODY_POINT_HEIGHT).distance_squared(at)
        < (radius + KART_RADIUS * UNIT).powi(2)
}

pub fn hazards(
    mut commands: Commands,
    time: Res<Time>,
    race: Res<Race>,
    hazards: Option<ResMut<Hazards>>,
    events: Option<ResMut<TrackEvents>>,
    scenery: Option<Res<Scenery>>,
    assets: Option<Res<ItemAssets>>,
    mut track: ResMut<Track>,
    mut sfx: ResMut<Sfx>,
    mut karts: Query<(Entity, &mut Kart)>,
    mut props: Query<(&mut Prop, Option<&mut Animated>, &mut Visibility)>,
    balls: Query<(), With<Action>>,
    mut beams: Query<&mut Beam>,
    emitters: Option<Res<Emitters>>,
    camera: Single<&Transform, (With<Camera3d>, Without<Particles>)>,
    mut smokes: Query<(&mut Transform, &mut Particles)>,
) {
    let (Some(mut hazards), Some(mut events), Some(scenery), Some(assets)) =
        (hazards, events, scenery, assets)
    else {
        return;
    };
    let events = &mut *events;
    let dt = time.delta_secs().min(0.05);
    let fired = std::mem::take(&mut events.fired);
    // A new race, or a restart, puts everything back.
    let first = !hazards.ready;
    let reset = race.phase == Phase::Intro || first;
    hazards.ready = true;
    if reset {
        // Every surface back to how the circuit starts out.
        let Track {
            surfaces,
            collision,
            ..
        } = &mut *track;
        for &(tag, passable) in surfaces.values() {
            collision.set_passable(tag, passable);
        }
    }
    let prop = |name: &str| scenery.0.get(name).copied();
    let pillar_box = track
        .surfaces
        .get("pilcol")
        .and_then(|&(tag, _)| track.collision.bounds(tag));
    let mut set_surface = |name: &str, passable: bool| {
        if let Some(&(tag, _)) = track.surfaces.get(name) {
            track.collision.set_passable(tag, passable);
        }
    };
    // Where a bone of a model is, and how far through its animation the model is.
    macro_rules! bone {
        ($name:expr, $bone:expr) => {
            prop($name)
                .and_then(|e| props.get(e).ok())
                .and_then(|(p, a, _)| Some((a?.bone_position(p, $bone, 0.0), a?.frame())))
        };
    }
    macro_rules! animated {
        ($name:expr) => {
            prop($name)
                .and_then(|e| props.get_mut(e).ok())
                .and_then(|(_, a, _)| a)
        };
    }
    macro_rules! show {
        ($name:expr, $visible:expr) => {
            if let Some((_, _, mut visibility)) = prop($name).and_then(|e| props.get_mut(e).ok()) {
                *visibility = if $visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        };
    }

    // An emitter of the hazard's own, made when first wanted and put where it should be.
    macro_rules! place {
        ($emitter:expr, $name:expr, $at:expr) => {
            match $emitter.and_then(|e| smokes.get_mut(e).ok()) {
                Some((mut transform, _)) => transform.translation = $at,
                None => {
                    let particles = emitters.as_ref().and_then(|e| e.emitter($name));
                    *$emitter = particles
                        .map(|p| commands.spawn((p, Transform::from_translation($at))).id());
                }
            }
        };
    }

    let Hazards { all, places, .. } = &mut *hazards;
    for (index, hazard) in all.iter_mut().enumerate() {
        let slot = LOOP_SLOT + index as u16;
        if reset {
            // The weather and the water are always at work, unless given a trigger; the
            // rest wait for theirs.
            hazard.active = hazard.trigger < 0
                && matches!(hazard.kind, Kind::Snowfall { .. } | Kind::Oscillator { .. });
            match &mut hazard.kind {
                Kind::FallingPillar { fallen, landed } => {
                    (*fallen, *landed) = (false, false);
                    set_surface("pilcol", true);
                    if let Some(mut pillar) = animated!("piltop") {
                        pillar.freeze(0);
                    }
                }
                Kind::TriggeredAnimation { props: names, .. } => {
                    for name in names.iter() {
                        show!(name, true);
                        if let Some(mut model) = animated!(name) {
                            model.freeze(0);
                        }
                        if let Some(entity) = prop(name) {
                            commands.entity(entity).insert(Fade(1.0));
                        }
                    }
                }
                Kind::Sphinx { blowing } => {
                    *blowing = 0.0;
                    show!("blowup", true);
                    if let Some(sphinx) = prop("blowup") {
                        commands
                            .entity(sphinx)
                            .insert(Retrack(vec![(2, 0), (3, 1)], true));
                    }
                }
                Kind::Rocket { open } => {
                    *open = false;
                    show!("mmrocon", false);
                    show!("mmrocof", true);
                }
                Kind::CodePuzzle {
                    code,
                    progress,
                    opened,
                } => {
                    *code = [0, 1, 2].map(|_| sfx.roll(2) == 1);
                    (*progress, *opened) = (0, 0.0);
                }
                Kind::Launcher { ball, landing, .. } => (*ball, *landing) = (None, None),
                Kind::SweepCannon { time, cooldown, .. } => (*time, *cooldown) = (0.0, 0.0),
                Kind::RollingRock {
                    prop: name,
                    start,
                    last,
                    ..
                } => {
                    *last = None;
                    // Each begins its round where the circuit says, so they don't all
                    // come by together.
                    if let Some(mut rock) = animated!(name).filter(|_| first) {
                        rock.seek(*start);
                    }
                }
                Kind::Ghost { trail, .. } => {
                    if let Some(entity) = trail.take() {
                        commands.entity(entity).try_despawn();
                    }
                }
                Kind::LavaGeyser { smoke, .. } => *smoke = 0.0,
                _ => {}
            }
            continue;
        }

        // Its trigger starting and ending sets it going and stops it. Only a warp pad
        // answers to each racer's own comings and goings (`Hazard::CanRetrigger`); the
        // rest go by the trigger as a whole: the first racer in, and the last out.
        for event in fired
            .iter()
            .filter(|f| f.event == hazard.trigger && hazard.trigger >= 0)
        {
            if matches!(hazard.kind, Kind::WarpPad) {
                let Some((_, mut k)) = event.racer.and_then(|e| karts.get_mut(e).ok()) else {
                    continue;
                };
                if event.start && !hazard.active && k.warp <= 0.0 {
                    // One racer at a time, taken to where the pad comes out.
                    hazard.active = true;
                    k.warp = WARP_TIME;
                    // A car on a recording keeps to its recording.
                    if k.route.is_none() {
                        k.warp_to = Some((
                            k.pos,
                            to_world(places.warp_to),
                            to_world(places.warp_facing),
                        ));
                    }
                } else if !event.start {
                    hazard.active = false;
                }
                continue;
            }
            if event.racer.is_some() {
                continue;
            }
            match (&mut hazard.kind, event.start) {
                (_, true) if !hazard.active => {
                    hazard.active = true;
                    debug!("hazard {index} set going by event {}", event.event);
                    match &mut hazard.kind {
                        Kind::Hammer { raised } => *raised = true,
                        Kind::Crane { pending } => *pending = true,
                        Kind::FallingPillar { fallen, landed } => {
                            (*fallen, *landed) = (false, false);
                            if let Some(mut pillar) = animated!("piltop") {
                                pillar.play(0, false);
                            }
                        }
                        Kind::TriggeredAnimation {
                            surface,
                            props: names,
                        } => {
                            for name in names.iter() {
                                if let Some(mut model) = animated!(name) {
                                    model.play(0, false);
                                }
                            }
                            set_surface(surface, true);
                            events.fire(9, None, &mut sfx);
                        }
                        Kind::Sphinx { blowing } => {
                            // Its face cracks and falls apart, picture by picture.
                            *blowing = SPHINX_BLOW_UP;
                            if let Some(sphinx) = prop("blowup") {
                                commands
                                    .entity(sphinx)
                                    .insert(Retrack(vec![(0, 2), (1, 3)], false));
                            }
                            set_surface("sphinx", true);
                            events.start(16, event.at, &mut sfx);
                        }
                        Kind::CurseDrop => {
                            let at = places.curse_drops[sfx.roll(3) as usize];
                            assets.curse(&mut commands, to_world(at));
                        }
                        Kind::CannonballRain {
                            interval, timer, ..
                        } => *timer = *interval,
                        Kind::SweepCannon { cooldown, .. } => *cooldown = 8.3,
                        Kind::Grabber { held, rest, .. } => (*held, *rest) = (0.0, 0.0),
                        _ => {}
                    }
                }
                (
                    Kind::FallingPillar { .. }
                    | Kind::Sphinx { .. }
                    | Kind::TriggeredAnimation { .. },
                    false,
                ) => {}
                (Kind::SmokeVent { emitter }, false)
                | (Kind::Snowfall { emitter }, false)
                | (Kind::Ghost { trail: emitter, .. }, false) => {
                    hazard.active = false;
                    if let Some(entity) = emitter.take() {
                        commands.entity(entity).try_despawn();
                    }
                }
                (Kind::Rocket { open }, false) => {
                    // The field comes back as the last racer leaves (`ShowOffModel`).
                    hazard.active = false;
                    if std::mem::take(open) {
                        show!("mmrocon", false);
                        show!("mmrocof", true);
                        set_surface("mmrocc", false);
                        events.end(35, None, &mut sfx);
                    }
                }
                (_, false) => {
                    if hazard.active {
                        debug!("hazard {index} stopped by event {}", event.event);
                    }
                    hazard.active = false;
                }
                _ => {}
            }
        }

        // Things that answer to events of their own.
        match &mut hazard.kind {
            Kind::Launcher {
                sources,
                targets,
                near,
                event,
                ball,
                multi,
                landing,
            } => {
                if ball.is_some_and(|b| balls.get(b).is_err()) {
                    *ball = None;
                    // The target's own event, once the shot is spent (`MultiLauncherHazard::OnDeactivate`).
                    if let Some((landed, at)) = landing.take() {
                        events.fire(landed, Some(at), &mut sfx);
                    }
                }
                let wanted = fired.iter().any(|f| {
                    let close =
                        |at: Vec3| near.is_none_or(|(centre, radius)| at.distance(centre) < radius);
                    // Near enough going by where the racer who set it off is, or
                    // failing a racer by where the trigger is.
                    let at = f
                        .racer
                        .and_then(|e| karts.get(e).ok())
                        .map(|(_, k)| k.pos)
                        .or(f.at);
                    f.event == *event && f.start && at.is_none_or(close)
                });
                if wanted && ball.is_none() && !sources.is_empty() && !targets.is_empty() {
                    let (from, fire_event) = sources[sfx.roll(sources.len() as u32) as usize];
                    let (to, hit_event) = targets[sfx.roll(targets.len() as u32) as usize];
                    events.fire(fire_event, Some(from), &mut sfx);
                    // A shot is always heard going off and landing (6 and 7); one of
                    // several sources and targets has their own events as well.
                    if *multi {
                        events.fire(6, Some(from), &mut sfx);
                        *landing = Some((hit_event, to));
                    }
                    *ball = Some(assets.cannonball(&mut commands, from, to, Some(7)));
                }
            }
            Kind::CodePuzzle {
                code,
                progress,
                opened,
            } => {
                if *opened > 0.0 {
                    *opened -= dt;
                    if *opened <= 0.0 {
                        events.end(28, None, &mut sfx);
                    }
                }
                for f in fired.iter().filter(|f| f.start) {
                    // 207 to 209 show each step's answer in turn.
                    if let Some(step) = f
                        .event
                        .checked_sub(207)
                        .filter(|s| (0..3).contains(s) && f.racer.is_none())
                    {
                        events.fire(if code[step as usize] { 29 } else { 20 }, f.at, &mut sfx);
                    }
                    // 200 to 205 are the two pads at each of the three steps. Every racer
                    // driving onto one counts (`HazardManager::DispatchEventStart`),
                    // whoever else is on it already.
                    let Some(pad) = f
                        .event
                        .checked_sub(200)
                        .filter(|c| (0..6).contains(c) && f.racer.is_some())
                    else {
                        continue;
                    };
                    let (step, first) = ((pad / 2) as u8, pad % 2 == 0);
                    events.fire(if first { 21 } else { 30 }, f.at, &mut sfx);
                    if first != code[step as usize] {
                        *progress = 0;
                    } else if step == 0 {
                        *progress = 1;
                    } else if *progress == step {
                        *progress += 1;
                    }
                    debug!(
                        "code pad {pad}: step {step}, {}, {} of 3 done",
                        if first == code[step as usize] {
                            "right"
                        } else {
                            "wrong"
                        },
                        *progress
                    );
                    if *progress == 3 {
                        events.fire(18, None, &mut sfx);
                        events.start(28, None, &mut sfx);
                        *code = [0, 1, 2].map(|_| sfx.roll(2) == 1);
                        (*progress, *opened) = (0, 2.5);
                    }
                }
            }
            _ => {}
        }
        if !hazard.active {
            continue;
        }

        match &mut hazard.kind {
            Kind::Hammer { raised } => {
                let Some((_, frame)) = bone!("rkhamm02", 0) else {
                    continue;
                };
                let within = |spans: [(f32, f32); 2]| {
                    spans.iter().any(|&(from, to)| frame > from && frame < to)
                };
                if *raised && within([(22.0, 28.0), (72.0, 78.0)]) {
                    events.fire(43, None, &mut sfx);
                    *raised = false;
                } else if !*raised && within([(0.0, 20.0), (40.0, 60.0)]) {
                    *raised = true;
                }
            }
            Kind::RollingRock {
                prop: name,
                radius,
                last,
                ..
            } => {
                let Some((centre, _)) = bone!(name, 1) else {
                    continue;
                };
                let moving = last.map_or(Vec3::ZERO, |last| (centre - last) / dt.max(1e-3));
                *last = Some(centre);
                for (_, mut k) in &mut karts {
                    if k.warp > 0.0 || !touching(&k, centre, *radius) {
                        continue;
                    }
                    // Shoved out of the way, taking on the lump's own motion.
                    let away = (k.pos - centre)
                        .with_y(0.0)
                        .try_normalize()
                        .unwrap_or(Vec3::X);
                    let overlap =
                        *radius + KART_RADIUS * UNIT - (k.pos - centre).with_y(0.0).length();
                    k.pos += away * overlap.max(0.0);
                    let closing = (k.vel - moving).dot(away);
                    if closing < 0.0 {
                        let bounce = away * closing * 1.5;
                        k.vel -= bounce;
                    }
                }
            }
            Kind::TriggeredAnimation { props: names, .. } => {
                // Each fades away as it plays (`TriggeredAnimationHazard::Draw`).
                for name in names.iter() {
                    let left = animated!(name).map(|model| 1.0 - model.progress());
                    if let (Some(left), Some(entity)) = (left, prop(name)) {
                        commands.entity(entity).insert(Fade(left));
                    }
                }
                let done = names
                    .first()
                    .and_then(|name| animated!(name))
                    .is_none_or(|model| model.done());
                if done {
                    for name in names.iter() {
                        show!(name, false);
                    }
                    hazard.active = false;
                }
            }
            Kind::Crane { pending } => {
                let Some((centre, frame)) = bone!("crane", 3) else {
                    continue;
                };
                if let Some((crane, ..)) = prop("crane").and_then(|e| props.get(e).ok()) {
                    sfx.sustain_global(
                        slot,
                        CRANE_SOUND,
                        Emitter::at(to_world(crane.position)).range(100.0, 300.0),
                    );
                }
                let within = |spans: [(f32, f32); 2]| {
                    spans.iter().any(|&(from, to)| frame > from && frame < to)
                };
                if *pending && within([(150.0, 180.0), (0.0, 30.0)]) {
                    events.fire(20, Some(centre), &mut sfx);
                    *pending = false;
                } else if !*pending && within([(60.0, 120.0), (210.0, 270.0)]) {
                    *pending = true;
                }
                for (_, mut k) in &mut karts {
                    if touching(&k, centre, 3.0 * UNIT) && spin(&mut k) {
                        events.fire(21, Some(k.pos), &mut sfx);
                    }
                }
            }
            Kind::FallingPillar { fallen, landed } => {
                if !*fallen && bone!("piltop", 0).is_some_and(|(_, frame)| frame > 50.0) {
                    events.fire(7, None, &mut sfx);
                    (*fallen, *landed) = (true, true);
                }
                // The original makes the pillar solid as it lands. Ours would shut in
                // any car underneath, the collision here having two sides, so it waits
                // for them to drive out from under it first.
                let under = |k: &Kart| {
                    pillar_box.is_some_and(|(lo, hi)| {
                        k.pos.cmpge(lo - 2.0).all() && k.pos.cmple(hi + 2.0).all()
                    })
                };
                if *landed && !karts.iter().any(|(_, k)| under(k)) {
                    set_surface("pilcol", false);
                    *landed = false;
                }
            }
            Kind::Sphinx { blowing } => {
                if *blowing > 0.0 {
                    *blowing -= dt;
                    if *blowing <= 0.0 {
                        events.end(16, None, &mut sfx);
                        show!("blowup", false);
                    }
                }
            }
            Kind::LavaGeyser {
                cooldown,
                flying,
                smoke,
            } => {
                let Some((centre, frame)) = bone!("mmlavbl", 0) else {
                    continue;
                };
                // The pool it has left smokes for as long as it is in the air.
                *smoke = (*smoke - dt).max(0.0);
                if *smoke == 0.0 {
                    for pool in 0..3 {
                        if frame > LAVA_LEAVES[pool] && frame < LAVA_LANDS[pool] {
                            if let Some(emitters) = &emitters {
                                emitters.spawn(
                                    &mut commands,
                                    "lavasmk",
                                    Transform::from_translation(to_world(places.lava_pools[pool])),
                                );
                            }
                            *smoke = LAVA_SMOKE;
                        }
                    }
                }
                if *flying {
                    sfx.sustain_global(slot, LAVA_SOUND, Emitter::at(centre).range(200.0, 300.0));
                }
                *cooldown = (*cooldown - dt).max(0.0);
                if *cooldown == 0.0 {
                    for pool in 0..3 {
                        let just = |at: f32| frame > at && frame < at + LAVA_WINDOW;
                        let (leaves, lands) = (just(LAVA_LEAVES[pool]), just(LAVA_LANDS[pool]));
                        if leaves || lands {
                            events.fire(43, Some(to_world(places.lava_pools[pool])), &mut sfx);
                            (*cooldown, *flying) = (0.4, lands);
                        }
                    }
                }
                for (_, mut k) in &mut karts {
                    if touching(&k, centre, 12.0 * UNIT) && spin(&mut k) {
                        k.cues.reaction = Some(false);
                    }
                }
            }
            Kind::Rocket { open } => {
                let Some(at) = prop("mmrocof")
                    .and_then(|e| props.get(e).ok())
                    .map(|(p, ..)| to_world(p.position))
                else {
                    continue;
                };
                let shielded = karts
                    .iter()
                    .any(|(_, k)| k.shielded() && k.pos.distance(at) < 350.0 * UNIT);
                if shielded != *open {
                    *open = shielded;
                    show!("mmrocon", shielded);
                    show!("mmrocof", !shielded);
                    set_surface("mmrocc", shielded);
                    if shielded {
                        events.start(35, None, &mut sfx);
                    } else {
                        events.end(35, None, &mut sfx);
                    }
                }
            }
            Kind::Ghost {
                search,
                waver,
                depth,
                trail,
            } => {
                let Some((centre, _)) = bone!("ghostly", 1) else {
                    continue;
                };
                place!(trail, "ghsttrl", centre - Vec3::Y * 5.0 * UNIT);
                // Its moan wavers, by a new amount every half second.
                *waver += dt;
                if *waver >= 0.5 {
                    *waver = 0.0;
                    *depth = sfx.roll(100) as f32 * 0.01 * 0.4;
                }
                let pitch = 1.0 - (*waver * 2.0 * PI).sin() * *depth;
                sfx.sustain_global(slot, GHOST_LOOP, Emitter::at(centre).pitch(pitch));
                *search += dt;
                if *search > 4.0 {
                    *search = 0.0;
                    if karts
                        .iter()
                        .any(|(_, k)| k.pos.distance(centre) < 60.0 * UNIT)
                    {
                        sfx.emit(GHOST_NEAR, Emitter::at(centre).range(200.0, 300.0));
                    }
                }
                for (_, mut k) in &mut karts {
                    if !k.shielded() && touching(&k, centre, 16.0 * UNIT) && spin(&mut k) {
                        // Stopped dead and tossed in the air.
                        k.vel = Vec3::Y * 150.0 * FORCE;
                        k.spin_out = physics::SPIN_OUT_TIME;
                        k.contacts = 0;
                        k.cues.reaction = Some(false);
                        sfx.emit(GHOST_HIT, Emitter::at(k.pos).range(200.0, 300.0));
                    }
                }
            }
            Kind::CannonballRain {
                prop: name,
                interval,
                timer,
            } => {
                *timer += dt;
                let Some((bone, _)) = bone!(name, 1) else {
                    continue;
                };
                // They come from below the bone, along its own third axis.
                let down = prop(name)
                    .and_then(|e| props.get(e).ok())
                    .and_then(|(p, a, _)| Some(a?.bone_axis(p, 1, Vec3::Z)));
                let from = bone - down.unwrap_or(Vec3::ZERO) * RAIN_DROP * UNIT;
                if *timer >= *interval {
                    *timer = 0.0;
                    let scatter =
                        Vec3::new(sfx.roll(4) as f32 - 2.0, 0.0, sfx.roll(4) as f32 - 2.0) * UNIT;
                    assets.cannonball(
                        &mut commands,
                        from,
                        from + scatter - Vec3::Y * 30.0 * UNIT,
                        None,
                    );
                }
            }
            Kind::SweepCannon {
                prop: name,
                source,
                period,
                sweep,
                time,
                cooldown,
                beam,
            } => {
                *time = (*time + dt) % period.max(0.1);
                let half = *period / 2.0;
                let mut across = *time / half * sweep[0];
                if *time > half {
                    across = PI - across;
                }
                let up = (*time / (*period / 4.0) * TAU).cos() * sweep[1] + sweep[2];
                let forward = to_world(Vec3::new(across.cos(), across.sin(), up)).normalize();
                let from = match name
                    .as_deref()
                    .and_then(&prop)
                    .and_then(|e| props.get(e).ok())
                {
                    Some((model, ..)) => to_world(model.position - Vec3::Z * 17.0),
                    None => to_world(*source),
                };
                let entity =
                    *beam.get_or_insert_with(|| commands.spawn(Beam { from, forward }).id());
                if let Ok(mut aim) = beams.get_mut(entity) {
                    (aim.from, aim.forward) = (from, forward);
                }
                *cooldown += dt;
                if *cooldown >= 8.3 {
                    *cooldown = 0.0;
                    assets.lightning(&mut commands, entity);
                }
            }
            Kind::Snowfall { emitter } => {
                // Where the player's car is tinted (under cover) there is no snow.
                if karts
                    .iter()
                    .any(|(_, k)| k.slot == PLAYER_SLOT && k.tint != Vec3::ONE)
                {
                    if let Some(entity) = emitter.take() {
                        commands.entity(entity).try_despawn();
                    }
                    continue;
                }
                let forward = camera.forward().as_vec3();
                let side = forward.cross(Vec3::Y).normalize_or_zero();
                let across = sfx.roll(SNOW_SPREAD) as f32 - SNOW_SPREAD as f32 / 2.0;
                let at = camera.translation
                    + (forward * SNOW_AHEAD + Vec3::Y * SNOW_ABOVE + side * across) * UNIT;
                place!(emitter, "snow", at);
            }
            Kind::SmokeVent { emitter } => {
                let Some((vent, _)) = bone!("dp_def", 0) else {
                    continue;
                };
                let Some(rotation) = prop("dp_def")
                    .and_then(|e| props.get(e).ok())
                    .map(|(p, ..)| p.rotation)
                else {
                    continue;
                };
                let offset = SMOKE_OFFSETS[sfx.roll(4) as usize];
                place!(emitter, "smoke", vent + to_world(rotation * offset));
            }
            Kind::Oscillator {
                prop: name,
                amplitude,
                time,
            } => {
                *time = (*time + dt) % OSCILLATOR_PERIOD;
                let slosh = (*time / OSCILLATOR_PERIOD * TAU).sin();
                if let Some((mut water, ..)) = prop(name).and_then(|e| props.get_mut(e).ok()) {
                    water.scroll = *amplitude * slosh;
                }
            }
            Kind::Grabber {
                prop: name,
                strength,
                frames,
                held,
                rest,
            } => {
                let Some((centre, frame)) = bone!(name, 0) else {
                    continue;
                };
                *rest = (*rest - dt).max(0.0);
                let reaching = frame <= frames.0 || frame >= frames.1;
                let mut grabbed = false;
                if *rest == 0.0 && reaching {
                    // It pulls in the first racer it finds, for a second at most.
                    let caught = karts
                        .iter_mut()
                        .find(|(_, k)| !k.shielded() && touching(k, centre, 45.0 * UNIT));
                    if let Some((_, mut k)) = caught {
                        let pull =
                            (centre - k.pos).with_y(0.0).normalize_or_zero() * *strength * FORCE;
                        k.external_force += pull;
                        grabbed = true;
                    }
                }
                *held = if grabbed { *held + dt } else { 0.0 };
                if *held >= 1.0 {
                    (*held, *rest) = (0.0, 1.0);
                }
            }
            _ => {}
        }
    }
}

/// Shows the code puzzle's answer on its three lights.
pub fn code_lights(
    time: Res<Time>,
    hazards: Option<Res<Hazards>>,
    scenery: Option<Res<Scenery>>,
    swatches: Option<Res<Swatches>>,
    props: Query<(&Scrolling, &crate::scenery::CodeLight)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(hazards), Some(scenery), Some(swatches)) = (hazards, scenery, swatches) else {
        return;
    };
    let puzzle = hazards.all.iter().find_map(|h| match &h.kind {
        Kind::CodePuzzle { code, opened, .. } => Some((*code, *opened > 0.0)),
        _ => None,
    });
    let Some((code, opened)) = puzzle else { return };
    for (step, name) in CODE_LIGHTS.iter().enumerate() {
        // With the doors open the lights flicker; otherwise they give the answer.
        let first = if opened {
            ((time.elapsed_secs() * CODE_FLICKER[step]) as u32).is_multiple_of(2)
        } else {
            code[step]
        };
        let Some(picture) = swatches.0.get(SWATCHES[if first { 0 } else { 1 }]) else {
            continue;
        };
        let Some((light, code)) = scenery.0.get(*name).and_then(|&e| props.get(e).ok()) else {
            continue;
        };
        // Only the part of the light its track is bound to changes (`MabMaterialTrack`).
        let changing = light
            .materials
            .iter()
            .zip(&light.indices)
            .filter(|(_, index)| code.0.contains(index));
        for (handle, _) in changing {
            if materials
                .get(handle)
                .is_some_and(|m| m.base_color_texture.as_ref() != Some(picture))
                && let Some(mut material) = materials.get_mut(handle)
            {
                material.base_color_texture = Some(picture.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the original game data; silently passes without it.
    #[test]
    fn every_circuit_s_hazards_load() {
        let mut kinds = 0;
        for (race, name) in crate::world::circuits() {
            let hazards = load(&race).unwrap().all;
            println!(
                "{race} {name}: {} hazards, triggers {:?}",
                hazards.len(),
                hazards.iter().map(|h| h.trigger).collect::<Vec<_>>()
            );
            kinds += hazards.len();
            for hazard in &hazards {
                match &hazard.kind {
                    Kind::RollingRock { prop, radius, .. } => {
                        assert!(!prop.is_empty() && *radius > 0.0, "{name}")
                    }
                    Kind::Launcher {
                        sources,
                        targets,
                        near,
                        event,
                        ..
                    } => {
                        assert!(
                            !sources.is_empty() && !targets.is_empty() && *event > 0,
                            "{name}"
                        );
                        assert!(near.is_some_and(|n| n.1 > 0.0), "{name}");
                    }
                    Kind::TriggeredAnimation { surface, props } => {
                        assert!(!surface.is_empty() && !props.is_empty(), "{name}")
                    }
                    Kind::CannonballRain { prop, interval, .. } => {
                        assert!(!prop.is_empty() && *interval > 0.0, "{name}")
                    }
                    Kind::SweepCannon { period, .. } => assert!(*period > 0.0, "{name}"),
                    Kind::Grabber {
                        prop,
                        strength,
                        frames,
                        ..
                    } => {
                        assert!(
                            !prop.is_empty() && *strength > 0.0 && frames.1 > frames.0,
                            "{name}"
                        )
                    }
                    _ => {}
                }
            }
        }
        assert!(kinds == 0 || kinds == 27, "{kinds} hazards");
    }
}
