//! What a host tells its players of the race besides where the cars are: what
//! power-ups have put into the world, which bricks have been taken or have come back,
//! the sounds of both, and every event of the circuit's. The port's own.
//!
//! None of it is worked out twice. The host runs the bricks, the power-ups and what
//! sets the circuit's events off, as it would alone, and says what came of it; a
//! player's game shows and sounds what it is told, and its hazards and scenery hang
//! on the host's events exactly as the host's own do.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::protocol::{SNAPSHOT_EVERY, TICKS, ToPlayer, encode};
use super::{Clock, Inbox, Session, Wire};
use crate::audio::{Emitter, Looped, Sfx};
use crate::events::{Fired, Logged, TrackEvents};
use crate::items::{Action, BrickMark, ItemAssets, Pickup};
use crate::kart::Kart;
use crate::racer_sounds::Cues;
use crate::scenery::Models;

/// How things other than the cars stand on the host.
#[derive(Serialize, Deserialize, Clone)]
pub struct Scene {
    /// Everything power-ups have in the world just now: which it is, as the host
    /// knows it, what it is doing, and where it is, how turned and how big.
    actions: Vec<(Entity, Action, Vec3, Quat, Vec3)>,
    /// The bricks that have changed.
    bricks: Vec<Pickup>,
    /// The sounds made somewhere on the circuit since the last telling, and the
    /// loops that are sounding now.
    sounds: Vec<(u32, Emitter)>,
    loops: Vec<Looped>,
    /// What has just happened to cars that their drivers have something to say
    /// about, or a horn or a shield to sound for, by grid slot.
    cues: Vec<(u8, Cues)>,
}

/// An event of the circuit's starting or ending on the host: `events::Logged`, with
/// the racer whose doing it was named by grid slot.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct EventNote {
    event: i32,
    start: bool,
    at: Option<Vec3>,
    racer: Option<u8>,
    whole: bool,
}

/// What a host has told its players, so as to tell them only what is new.
#[derive(Resource, Default)]
pub struct Told {
    bricks: HashMap<u16, BrickMark>,
    /// The last telling had something of a power-up's in it, which the next must
    /// say has gone if it has.
    actions: bool,
    /// What each car owed a sound for when last looked at, by grid slot, and what
    /// has come up since the last telling.
    cues: [Cues; 6],
    cued: Vec<(u8, Cues)>,
    /// The events of the circuit's own that have started and not ended.
    going: Vec<EventNote>,
}

impl Told {
    /// Someone has come to the race late: every brick is told again, to everyone,
    /// and what is given back is the events still going on, for the latecomer.
    pub fn again(&mut self) -> Vec<EventNote> {
        self.bricks.clear();
        self.going.clone()
    }
}

/// What a player's game has put into the world on its host's word, by which of the
/// host's it is.
#[derive(Resource, Default)]
pub struct Shown {
    actions: HashMap<Entity, Entity>,
    /// The loops the host last said were sounding.
    loops: Vec<Looped>,
}

#[cfg(test)]
impl Shown {
    pub fn loops(&self) -> &[Looped] {
        &self.loops
    }
}

/// Something shown on its way from where the host last had it to where it has it
/// now. The host says thirty times a second and the screen is drawn oftener.
#[derive(Component)]
pub struct Glide {
    from: Transform,
    to: Transform,
    along: f32,
}

/// Farther than this between two tellings is a thing put somewhere, not on its way.
const PUT: f32 = 20.0;

/// The host keeps note of the sounds its race makes while it is stepped, to pass on.
/// The loops are asked for afresh each step, so only the last step's are kept.
pub fn listen(mut sfx: ResMut<Sfx>) {
    sfx.listening = true;
    sfx.looping.clear();
}

pub fn tell(
    clock: Res<Clock>,
    session: Res<Session>,
    mut wire: ResMut<Wire>,
    mut told: ResMut<Told>,
    mut sfx: ResMut<Sfx>,
    actions: Query<(Entity, &Action, &Transform)>,
    bricks: Query<&Pickup>,
    karts: Query<&Kart>,
) {
    // The step is over; what sounds from here on is not the race's to pass on.
    sfx.listening = false;
    // A cue stands until it has been sounded here, which may be several steps:
    // it is passed on once, as it comes up.
    for kart in &karts {
        let Some(before) = told.cues.get_mut(kart.slot) else {
            continue;
        };
        let new = Cues {
            reaction: kart
                .cues
                .reaction
                .filter(|_| kart.cues.reaction != before.reaction),
            shield_hit: kart.cues.shield_hit && !before.shield_hit,
            horn: kart.cues.horn && !before.horn,
        };
        *before = kart.cues;
        if new != Cues::default() {
            told.cued.push((kart.slot as u8, new));
        }
    }
    if !clock.tick.is_multiple_of(SNAPSHOT_EVERY) {
        return;
    }
    let scene = Scene {
        actions: actions
            .iter()
            .map(|(entity, action, at)| {
                (
                    entity,
                    action.clone(),
                    at.translation,
                    at.rotation,
                    at.scale,
                )
            })
            .collect(),
        bricks: crate::items::bricks_changed(&bricks, &mut told.bricks),
        sounds: std::mem::take(&mut sfx.heard)
            .into_iter()
            .map(|(sound, emitter)| (sound as u32, emitter))
            .collect(),
        loops: sfx.looping.clone(),
        cues: std::mem::take(&mut told.cued),
    };
    let anything = !scene.actions.is_empty()
        || !scene.bricks.is_empty()
        || !scene.sounds.is_empty()
        || !scene.cues.is_empty();
    if anything || told.actions {
        told.actions = !scene.actions.is_empty();
        let scene = encode(&ToPlayer::Scene(scene));
        for member in &session.members {
            wire.0.send(member.peer, scene.clone());
        }
    }
}

/// Passes on the events of the circuit's that have started and ended.
pub fn tell_events(
    session: Res<Session>,
    mut told: ResMut<Told>,
    mut wire: ResMut<Wire>,
    events: Option<ResMut<TrackEvents>>,
    karts: Query<(Entity, &Kart)>,
) {
    let Some(mut events) = events else { return };
    if events.log.is_empty() {
        return;
    }
    let slot = |racer: Entity| karts.get(racer).ok().map(|(_, kart)| kart.slot as u8);
    let notes: Vec<EventNote> = events
        .log
        .drain(..)
        // What some racer did is of no use without the racer.
        .filter(|logged| {
            logged.whole
                || logged
                    .fired
                    .racer
                    .is_some_and(|racer| slot(racer).is_some())
        })
        .map(|Logged { fired, whole }| EventNote {
            event: fired.event,
            start: fired.start,
            at: fired.at,
            racer: fired.racer.and_then(slot),
            whole,
        })
        .collect();
    for note in notes.iter().filter(|note| note.whole) {
        told.going.retain(|going| going.event != note.event);
        if note.start {
            told.going.push(*note);
        }
    }
    let notes = encode(&ToPlayer::Events(notes));
    for member in &session.members {
        wire.0.send(member.peer, notes.clone());
    }
}

/// A player's game makes its bricks and what power-ups have put about as the host
/// says they are, and sounds what the host heard.
pub fn take(
    mut commands: Commands,
    mut inbox: ResMut<Inbox>,
    mut shown: ResMut<Shown>,
    mut sfx: ResMut<Sfx>,
    assets: Option<Res<ItemAssets>>,
    models: Option<Res<Models>>,
    mut bricks: Query<(Entity, &mut Pickup, &mut Transform, &mut Visibility)>,
    mut karts: Query<&mut Kart>,
    glides: Query<&Glide>,
) {
    let Some(assets) = assets else { return };
    let scenes = std::mem::take(&mut inbox.scenes);
    let count = scenes.len();
    for (n, scene) in scenes.into_iter().enumerate() {
        crate::items::bricks_told(
            &mut commands,
            &assets,
            models.as_deref(),
            scene.bricks,
            &mut bricks,
        );
        for (sound, emitter) in scene.sounds {
            sfx.emit(sound as usize, emitter);
        }
        for (slot, cues) in scene.cues {
            if let Some(mut kart) = karts.iter_mut().find(|kart| kart.slot == slot as usize) {
                kart.cues = Cues {
                    reaction: cues.reaction.or(kart.cues.reaction),
                    shield_hit: cues.shield_hit || kart.cues.shield_hit,
                    horn: cues.horn || kart.cues.horn,
                };
            }
        }
        // Of several tellings at once only the last says what is there now.
        if n + 1 < count {
            continue;
        }
        shown.loops = scene.loops;
        shown.actions.retain(|theirs, ours| {
            let still = scene.actions.iter().any(|action| action.0 == *theirs);
            if !still {
                commands.entity(*ours).try_despawn();
            }
            still
        });
        for (theirs, action, translation, rotation, scale) in scene.actions {
            let at = Transform {
                translation,
                rotation,
                scale,
            };
            match shown.actions.get(&theirs) {
                Some(&ours) => {
                    // On from where it was last said to be, unless it has been put elsewhere.
                    let from = glides.get(ours).map_or(at, |glide| glide.to);
                    let from = if from.translation.distance_squared(at.translation) > PUT * PUT {
                        at
                    } else {
                        from
                    };
                    commands.entity(ours).try_insert((
                        action,
                        Glide {
                            from,
                            to: at,
                            along: 0.0,
                        },
                    ));
                }
                None => {
                    let (mesh, material) = assets.look(&action);
                    let glide = Glide {
                        from: at,
                        to: at,
                        along: 1.0,
                    };
                    shown.actions.insert(
                        theirs,
                        commands
                            .spawn((action, Mesh3d(mesh), MeshMaterial3d(material), at, glide))
                            .id(),
                    );
                }
            }
        }
    }
}

/// A player's game starts and ends the circuit's events as its host did.
pub fn follow_events(
    mut inbox: ResMut<Inbox>,
    mut sfx: ResMut<Sfx>,
    events: Option<ResMut<TrackEvents>>,
    karts: Query<(Entity, &Kart)>,
) {
    let notes = std::mem::take(&mut inbox.events);
    let Some(mut events) = events else { return };
    for note in notes {
        let racer = note
            .racer
            .and_then(|slot| karts.iter().find(|(_, kart)| kart.slot == slot as usize))
            .map(|(entity, _)| entity);
        if !note.whole && racer.is_none() {
            continue;
        }
        events.follow(
            Logged {
                fired: Fired {
                    event: note.event,
                    start: note.start,
                    at: note.at,
                    racer,
                },
                whole: note.whole,
            },
            &mut sfx,
        );
    }
}

/// Moves what power-ups have put about from where the host last had each to where it
/// has it now, over the time between two tellings.
pub fn glide(time: Res<Time>, mut gliding: Query<(&mut Glide, &mut Transform)>) {
    let step = time.delta_secs() * (TICKS / SNAPSHOT_EVERY as f64) as f32;
    for (mut glide, mut at) in &mut gliding {
        glide.along = (glide.along + step).min(1.0);
        let (from, to, along) = (glide.from, glide.to, glide.along);
        at.translation = from.translation.lerp(to.translation, along);
        at.rotation = from.rotation.slerp(to.rotation, along);
        at.scale = from.scale.lerp(to.scale, along);
    }
}

/// Keeps the loops the host last said were sounding, sounding.
pub fn sound_loops(shown: Res<Shown>, mut sfx: ResMut<Sfx>) {
    for &looped in &shown.loops {
        sfx.again(looped);
    }
}
