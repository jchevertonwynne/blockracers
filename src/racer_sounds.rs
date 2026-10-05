//! The sounds racers make, as `Racer`, `RacerPhysics`, `RacerCarBody` and `CarVisuals`
//! make them in the original: engines, brakes, tyres, horns, voices and the hum of
//! whatever power-up is at work on them.

use crate::audio::{Emitter, Sfx, VoicePlaces, id};
use crate::kart::{Controls, Kart, Player};
use crate::physics::UNIT;
use crate::{Phase, Race};
use bevy::prelude::*;

// Speeds are in the original's units per millisecond.
const ENGINE_DRIVE_MIN_SPEED: f32 = 0.015;
const ENGINE_VOLUME: f32 = 0.7;
const ENGINE_VOLUME_FINISHED: f32 = 0.5;
/// Engine notes fade in and out by this much volume per millisecond; the idle note
/// comes in half as fast again.
const ENGINE_FADE_RATE: f32 = 0.03 * 0.06;
const ENGINE_IDLE_FADE_SCALE: f32 = 1.5;
/// A fading note's volume follows a quarter sine, reaching full at the normal volume.
const ENGINE_FADE_CURVE: f32 = 2.243_994_7;
const ENGINE_PITCH_FLOOR: f32 = 0.4;
const ENGINE_PITCH_DRIVE_BAND: f32 = 0.1;
const ENGINE_PITCH_SPEED_RANGE: f32 = 0.17;
/// Off the ground this long, the engine races.
const ENGINE_AIRBORNE: f32 = 0.05;
const BRAKE_MIN_SPEED: f32 = 0.01;
const SCRAPE_COOLDOWN: f32 = 0.25;
const LANDING_AIR_TIME: f32 = 0.4;
const SURFACE_MIN_SPEED: f32 = 0.009;
const SURFACE_PITCH_SPEED: f32 = 0.22;
const SURFACE_FADE_IN: f32 = 0.28;
const SKID_VOLUME: f32 = 0.8;
/// Slip steering squeals once the car is pointing this far off its line.
const SKID_ALIGNMENT_MAX: f32 = 0.9;
const POWERSLIDE_FACTOR: f32 = 0.4;
/// The engine heard from the nearest other racer.
const OTHER_ENGINE_RANGE: (f32, f32) = (30.0, 200.0);
const OTHER_ENGINE_VOLUME: f32 = 0.8;
const OTHER_ENGINE_AIRBORNE_PITCH: f32 = 0.2;
/// Horns sound at racers this close ahead, inside a cone this wide.
const HORN_DISTANCE: f32 = 13.0;
const HORN_CONE: f32 = 0.3;
const HORN_RETRY: f32 = 2.0;
const VOICE_RANGE: (f32, f32) = (100.0, 400.0);
/// A curse hovers this far above its victim, and a shield gives out this far up.
const CURSE_HEIGHT: f32 = 9.0;
const SHIELD_EXPIRE_HEIGHT: f32 = 5.0;
const TURBO_END_VOLUME: (f32, f32) = (0.6, 0.2);

/// Which of a racer's loops a sound is.
mod slot {
    pub const ENGINE: u16 = 0; // And the two after it.
    pub const BRAKE: u16 = 3;
    pub const SKID: u16 = 4;
    pub const SPIN: u16 = 5;
    pub const SURFACE: u16 = 6;
    pub const TURBO: u16 = 7;
    pub const WARP: u16 = 8;
    pub const CURSE: u16 = 9;
    pub const SHIELD: u16 = 10;
    pub const OTHER_ENGINE: u16 = 11;
    pub const MAGNET: u16 = 12;
}

/// The three engine notes: idling, under power and coasting.
const ENGINE_NOTES: [usize; 3] = [id::ENGINE_IDLE, id::ENGINE, id::ENGINE_COAST];

/// Things that happen to a racer elsewhere in the game and want a sound.
#[derive(Default, Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Cues {
    /// A remark: happy or not.
    pub reaction: Option<bool>,
    pub shield_hit: bool,
    pub horn: bool,
}

#[derive(Component, Default)]
pub struct RacerAudio {
    started: bool,
    /// Volume of each engine note, which are sounding, and which is coming in.
    engine: [f32; 3],
    engine_on: [bool; 3],
    engine_note: usize,
    braking: bool,
    skidding: bool,
    airborne: bool,
    contacts: u8,
    /// The surface sound under the wheels and how long it has been going.
    surface: Option<(usize, f32)>,
    horn_cooldown: f32,
    reaction_cooldown: f32,
    // Last frame's state, to catch things starting and stopping.
    sliding: bool,
    shielded: bool,
    boost: f32,
    warping: bool,
    held: bool,
}

/// Everything about the other racers that a racer's sounds depend on.
struct Other {
    entity: Entity,
    pos: Vec3,
    vel: Vec3,
    engine_pitch: f32,
    air_time: f32,
}

fn timer(value: &mut f32, dt: f32) {
    *value = (*value - dt).max(0.0);
}

pub fn racer_sounds(
    time: Res<Time>,
    race: Res<Race>,
    places: Option<Res<VoicePlaces>>,
    mut sfx: ResMut<Sfx>,
    mut racers: Query<(Entity, &mut Kart, &Controls, &mut RacerAudio, Has<Player>)>,
) {
    let dt = time.delta_secs().min(0.05);
    let racing = matches!(race.phase, Phase::Racing | Phase::Finished);
    let others: Vec<Other> = racers
        .iter()
        .map(|(entity, k, ..)| Other {
            entity,
            pos: k.pos,
            vel: k.vel,
            engine_pitch: k.engine_pitch,
            air_time: k.air_time,
        })
        .collect();

    for (entity, mut kart, controls, mut audio, is_player) in &mut racers {
        let (k, a) = (&mut *kart, &mut *audio);
        if race.phase == Phase::Intro {
            *a = RacerAudio {
                contacts: k.contacts,
                ..default()
            };
            k.cues = Cues::default();
            continue;
        }
        let forward = k.rot * Vec3::NEG_Z;
        let speed = k.vel.length() / UNIT / 1000.0;
        let forward_speed = k.vel.dot(forward) / UNIT / 1000.0;
        let spinning = k.spin > 0.0 || k.spin_out > 0.0;
        let here = Emitter::at(k.pos).moving(k.vel);
        // The player's kart is the AI's to drive once the race is run, or in a demo.
        let driven_by_ai = !is_player || k.finished.is_some() || race.demo;

        if is_player {
            if !a.started {
                a.started = true;
                sfx.play_at(id::ENGINE_START, k.pos);
            }
            engine(k, a, controls, entity, here, speed, dt, &mut sfx);
            // The AI feathers the brakes far too fast for them to squeal.
            if !driven_by_ai {
                brake(a, controls, entity, here, speed, forward_speed, &mut sfx);
            }
            if k.boost > 0.0 && k.boost_level < 3 {
                sfx.sustain(
                    entity,
                    slot::TURBO,
                    id::TURBO_LOOP + k.boost_level as usize,
                    here,
                );
            }
            if k.warp > 0.0 {
                // The original never tells this one where the racer is, so it stays
                // at the middle of the world.
                sfx.sustain(entity, slot::WARP, id::WARP_LOOP, Emitter::at(Vec3::ZERO));
            }
            if racing {
                other_engine(entity, k.pos, &others, &mut sfx);
            }
        }

        // Scraping along a wall.
        timer(&mut k.scrape_cooldown, dt);
        if k.wall_contact && k.scrape_cooldown <= 0.0 {
            let sound = id::WALL_HITS[sfx.roll(2) as usize];
            sfx.play_at(sound, k.pos);
            k.scrape_cooldown = SCRAPE_COOLDOWN;
        }

        // Landing: which sound depends on how many wheels came down together.
        if k.air_time > LANDING_AIR_TIME {
            a.airborne = true;
        }
        if a.airborne && k.contacts > a.contacts {
            a.airborne = false;
            let sound = match k.contacts - a.contacts {
                1 => id::LAND_ONE_WHEEL,
                4 => id::LAND_FOUR_WHEELS,
                _ => id::LAND_SOME_WHEELS,
            };
            sfx.play_at(sound, k.pos);
        }
        a.contacts = k.contacts;

        // Tyres: squealing while steering past their grip, whirring through a spin
        // and rumbling over whatever the road is made of.
        if !(k.sliding || k.slipping) {
            a.skidding = false;
        } else if !a.skidding && racing && !spinning && !k.wall_contact {
            let aligned = k.vel.normalize_or_zero().dot(forward);
            a.skidding = k.sliding || aligned < SKID_ALIGNMENT_MAX;
        }
        if a.skidding {
            let slide = if k.sliding {
                POWERSLIDE_FACTOR * 0.5
            } else {
                0.0
            };
            let pitch = slide + 1.4 - (ENGINE_PITCH_SPEED_RANGE - forward_speed) * 4.0;
            sfx.sustain(
                entity,
                slot::SKID,
                id::SKID,
                here.volume(SKID_VOLUME).pitch(pitch),
            );
        }
        if k.spin > 0.0 && racing {
            sfx.sustain(entity, slot::SPIN, id::SPIN, Emitter::at(k.pos));
        }
        let rolling = k.contacts > 0 && !spinning && speed >= SURFACE_MIN_SPEED;
        a.surface = match (k.surface.sound, rolling) {
            (Some(sound), true) => {
                let age = a.surface.filter(|s| s.0 == sound).map_or(0.0, |s| s.1) + dt;
                let pitch = (speed / SURFACE_PITCH_SPEED + 0.4).clamp(0.5, 2.0);
                let volume = (age / SURFACE_FADE_IN).min(1.0);
                sfx.sustain(
                    entity,
                    slot::SURFACE,
                    sound,
                    here.volume(volume).pitch(pitch),
                );
                Some((sound, age))
            }
            _ => None,
        };
        if k.sliding && !a.sliding && !driven_by_ai && !spinning {
            sfx.play_at(id::DRIFT_START, k.pos);
        }
        a.sliding = k.sliding;

        // Horns: the AI sounds off at whoever is in its way.
        timer(&mut a.horn_cooldown, dt);
        if driven_by_ai && racing && a.horn_cooldown <= 0.0 {
            let blocked = others.iter().any(|o| {
                let to = (o.pos - k.pos) / UNIT;
                let distance = to.length();
                o.entity != entity
                    && distance > 0.0
                    && distance < HORN_DISTANCE
                    && to.dot(forward) / distance > HORN_CONE
            });
            a.horn_cooldown = HORN_RETRY;
            if blocked {
                k.cues.horn = true;
                a.horn_cooldown += sfx.roll(1024) as f32 * 0.008;
            }
        }
        let cues = std::mem::take(&mut k.cues);
        if cues.horn {
            k.honked = true;
            let place = places
                .as_ref()
                .and_then(|p| p.0.get(k.slot))
                .copied()
                .unwrap_or(0);
            sfx.play_at(
                if is_player {
                    id::PLAYER_HORN
                } else {
                    id::HORNS[place.min(5)]
                },
                k.pos,
            );
        }
        timer(&mut a.reaction_cooldown, dt);
        if let (Some(happy), true) = (cues.reaction, a.reaction_cooldown <= 0.0) {
            let remark = sfx.roll(6) as usize + if happy { 6 } else { 0 };
            let sound = id::VOICES + k.slot * id::VOICES_EACH + remark;
            sfx.emit(
                sound,
                Emitter::at(k.pos).range(VOICE_RANGE.0, VOICE_RANGE.1),
            );
            a.reaction_cooldown = 5.0 + sfx.roll(1024) as f32 * 0.004;
        }
        if cues.shield_hit {
            let sound = id::SHIELD_HITS[sfx.roll(3) as usize];
            sfx.emit(sound, Emitter::at(k.pos).far());
        }

        // Power-ups at work on this racer.
        if k.shielded() {
            sfx.sustain(
                entity,
                slot::SHIELD,
                id::SHIELDS[k.shield_level.min(3) as usize],
                here,
            );
        } else if a.shielded {
            sfx.play_at(
                id::SHIELD_EXPIRE,
                k.pos + Vec3::Y * SHIELD_EXPIRE_HEIGHT * UNIT,
            );
        }
        a.shielded = k.shielded();
        if k.cursed > 0.0 {
            sfx.sustain(
                entity,
                slot::CURSE,
                id::CURSED_LOOP,
                Emitter::at(k.pos + Vec3::Y * CURSE_HEIGHT * UNIT),
            );
        }
        if k.boost > a.boost {
            sfx.play_at(id::TURBO_START + k.boost_level.min(2) as usize, k.pos);
            sfx.play_at(id::WHOOSH, k.pos);
        } else if a.boost > 0.0 && k.boost <= 0.0 {
            // The longest turbo winds down with its own sound.
            if k.boost_level == 2 {
                sfx.play_at(id::TURBO_END_LONG, k.pos);
            } else {
                let volume = TURBO_END_VOLUME.0 + TURBO_END_VOLUME.1 * k.boost_level as f32;
                sfx.emit(id::TURBO_END, Emitter::at(k.pos).volume(volume));
            }
        }
        a.boost = k.boost;
        // A magnet goes on humming around whoever it has caught, then lets go.
        if k.magnet > 0.0 {
            sfx.sustain(entity, slot::MAGNET, id::MAGNET_LOOP, Emitter::at(k.pos));
        } else if a.held {
            sfx.play_at(id::MAGNET_RELEASE, k.pos);
        }
        a.held = k.magnet > 0.0;
        let warping = k.warp > 0.0;
        if warping && !a.warping {
            if !driven_by_ai {
                sfx.play(id::WARP_START);
            }
            sfx.emit(id::WHOOSH, Emitter::at(k.pos).far());
        } else if a.warping && !warping && !driven_by_ai {
            sfx.play(id::WARP_END);
        }
        a.warping = warping;
    }
}

/// `Racer::UpdateEngineSound`: three notes cross-fading, pitched by speed.
fn engine(
    k: &Kart,
    a: &mut RacerAudio,
    controls: &Controls,
    entity: Entity,
    here: Emitter,
    speed: f32,
    dt: f32,
    sfx: &mut Sfx,
) {
    // A note takes over only once it has fallen silent, so quick changes of mind
    // leave the old note playing.
    let wanted = if controls.throttle == 0.0 && k.boost <= 0.0 {
        if speed > ENGINE_DRIVE_MIN_SPEED { 2 } else { 0 }
    } else {
        1
    };
    if !a.engine_on[wanted] {
        a.engine_on[wanted] = true;
        a.engine_note = wanted;
    }
    let target = if k.finished.is_some() {
        ENGINE_VOLUME_FINISHED
    } else {
        ENGINE_VOLUME
    };
    let step = dt * 1000.0 * ENGINE_FADE_RATE;
    let scale = k.engine_pitch;
    let pitches = [
        scale,
        speed / ENGINE_PITCH_SPEED_RANGE
            * (1.0 - ENGINE_PITCH_FLOOR - ENGINE_PITCH_DRIVE_BAND)
            * scale
            + ENGINE_PITCH_FLOOR
            + if k.air_time > ENGINE_AIRBORNE {
                ENGINE_PITCH_DRIVE_BAND
            } else {
                0.0
            },
        speed / ENGINE_PITCH_SPEED_RANGE * (1.0 - ENGINE_PITCH_FLOOR) * scale + ENGINE_PITCH_FLOOR,
    ];
    for note in 0..3 {
        let volume = &mut a.engine[note];
        if note == a.engine_note {
            if *volume < target {
                *volume += step
                    * if note == 0 {
                        ENGINE_IDLE_FADE_SCALE
                    } else {
                        1.0
                    };
            }
            *volume = volume.min(target);
        } else {
            *volume = (*volume - step).max(0.0);
        }
        if *volume == 0.0 {
            a.engine_on[note] = false;
        }
        if a.engine_on[note] {
            let heard = if *volume == target {
                target
            } else {
                (*volume * ENGINE_FADE_CURVE).sin() * target
            };
            let tone = here.volume(heard).pitch(pitches[note].clamp(0.0, 1.0));
            sfx.sustain(entity, slot::ENGINE + note as u16, ENGINE_NOTES[note], tone);
        }
    }
}

/// Braking: a chirp as the brakes go on, then a squeal that drops with the speed.
fn brake(
    a: &mut RacerAudio,
    controls: &Controls,
    entity: Entity,
    here: Emitter,
    speed: f32,
    forward_speed: f32,
    sfx: &mut Sfx,
) {
    let braking = controls.throttle < 0.0 && forward_speed >= BRAKE_MIN_SPEED;
    if braking && !a.braking {
        sfx.play_at(id::BRAKE, here.pos);
    }
    if braking {
        let pitch = 1.0 - (ENGINE_PITCH_SPEED_RANGE - speed);
        sfx.sustain(entity, slot::BRAKE, id::BRAKE_LOOP, here.pitch(pitch));
    }
    a.braking = braking;
}

/// `RaceState`'s proximity sound: the engine of the nearest other racer.
fn other_engine(me: Entity, from: Vec3, others: &[Other], sfx: &mut Sfx) {
    let distance = |o: &Other| o.pos.distance_squared(from);
    let nearest = others
        .iter()
        .filter(|o| o.entity != me)
        .min_by(|a, b| distance(a).total_cmp(&distance(b)));
    let Some(nearest) = nearest.filter(|o| distance(o).sqrt() / UNIT < OTHER_ENGINE_RANGE.1) else {
        return;
    };
    let speed = nearest.vel.length() / UNIT / 1000.0;
    let mut pitch = (speed / ENGINE_PITCH_SPEED_RANGE
        * (1.0 - ENGINE_PITCH_FLOOR - ENGINE_PITCH_DRIVE_BAND)
        * nearest.engine_pitch
        + ENGINE_PITCH_FLOOR)
        .clamp(0.0, 1.0);
    if nearest.air_time > ENGINE_AIRBORNE {
        pitch += OTHER_ENGINE_AIRBORNE_PITCH;
    }
    let emitter = Emitter::at(nearest.pos)
        .moving(nearest.vel * 2.0)
        .range(OTHER_ENGINE_RANGE.0, OTHER_ENGINE_RANGE.1)
        .volume(OTHER_ENGINE_VOLUME)
        .pitch(pitch);
    sfx.sustain_global(slot::OTHER_ENGINE, id::OTHER_ENGINE, emitter);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Track;

    #[test]
    fn engine_notes_cross_fade_and_rise_with_speed() {
        let track = Track::new();
        let mut kart = Kart::new(&track, 0);
        let mut audio = RacerAudio::default();
        let mut sfx = Sfx::default();
        let entity = Entity::PLACEHOLDER;
        let here = Emitter::at(Vec3::ZERO);
        let mut run =
            |audio: &mut RacerAudio, kart: &Kart, throttle: f32, speed: f32, seconds: f32| {
                let controls = Controls {
                    throttle,
                    ..default()
                };
                for _ in 0..(seconds * 60.0) as usize {
                    engine(
                        kart,
                        audio,
                        &controls,
                        entity,
                        here,
                        speed,
                        1.0 / 60.0,
                        &mut sfx,
                    );
                }
            };
        // At rest only the idle note sounds, and it comes up to the normal volume.
        run(&mut audio, &kart, 0.0, 0.0, 1.0);
        assert_eq!(audio.engine, [ENGINE_VOLUME, 0.0, 0.0]);
        // Under power the drive note takes over and the idle note dies away.
        run(&mut audio, &kart, 1.0, 0.1, 1.0);
        assert_eq!(audio.engine, [0.0, ENGINE_VOLUME, 0.0]);
        assert_eq!(audio.engine_on, [false, true, false]);
        // Lifting off at speed brings in the coasting note.
        run(&mut audio, &kart, 0.0, 0.1, 1.0);
        assert_eq!(audio.engine, [0.0, 0.0, ENGINE_VOLUME]);
        // Finished racers' engines are quieter.
        kart.finished = Some(1.0);
        run(&mut audio, &kart, 0.0, 0.1, 0.1);
        assert_eq!(audio.engine[2], ENGINE_VOLUME_FINISHED);
    }
}
