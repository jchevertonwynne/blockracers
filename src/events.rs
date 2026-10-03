//! The circuits' own sounds: birdsong, waterfalls, tunnels and the rest. Each circuit's
//! event table (`.EVB`) ties sounds to numbered events, which are set going by racers
//! driving into trigger spheres (`.TRB`) or onto marked surfaces, and by timers
//! (`.TIB`). This follows `RaceEventTable`, `SoundResource`, `TriggerList` and
//! `RaceTimerList`; the event tables' animations and particles are not played.

use crate::assets::{
    Jam,
    route,
    tokens::{Token, tokenize},
};
use crate::audio::{Emitter, Sfx};
use crate::kart::Kart;
use crate::physics::UNIT;
use crate::{Phase, Race};
use bevy::prelude::*;
use std::collections::HashMap;

/// Sounds that play now and then come up for another go this often: a minimum plus up
/// to this much more.
const RETRIGGER: (f32, f32) = (0.5, 1.0);
/// Loop slots for event sounds start here.
const LOOP_SLOT: u16 = 1000;

struct EventSound {
    event: i32,
    sound: usize,
    emitter: Emitter,
    looping: bool,
    /// Keeps going once started.
    no_end: bool,
    /// Starts when its event ends rather than when it starts.
    on_end: bool,
    /// Sounds from wherever its event happened.
    at_event: bool,
    /// Chance of playing each time it comes up, if it isn't a certainty.
    chance: Option<f32>,
    active: bool,
    retrigger: f32,
}

struct Trigger {
    centre: Vec3,
    radius: f32,
    event: i32,
    active: bool,
}

struct Timer {
    /// How long its event runs and how long it rests, and whether each is random.
    on: (f32, bool),
    off: (f32, bool),
    delay: f32,
    event: i32,
    active: bool,
    remaining: Option<f32>,
}

#[derive(Resource, Default)]
pub struct TrackEvents {
    sounds: Vec<EventSound>,
    triggers: Vec<Trigger>,
    timers: Vec<Timer>,
    /// The enter, leave and touch events of the surface each kart is on.
    surfaces: HashMap<Entity, [Option<i32>; 3]>,
}

fn to_world(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1]) * UNIT
}

fn number(token: Option<&Token>) -> f32 {
    match token {
        Some(Token::Float(v)) => *v,
        Some(Token::Int(v)) => *v as f32,
        _ => 0.0,
    }
}

/// The records of the section of a token stream that opens with `key`:
/// `key [count] { 0x27 ... { fields } ... }`. Each is its header and its fields.
fn records(tokens: &[Token], key: u16) -> Vec<(&[Token], &[Token])> {
    let opens = |i: usize| tokens[i] == Token::Key(key) && tokens.get(i + 1) == Some(&Token::LBracket);
    let Some(start) = (0..tokens.len()).find(|&i| opens(i)) else { return Vec::new() };
    let mut out = Vec::new();
    let mut at = start + 5;
    while tokens.get(at) == Some(&Token::Key(0x27)) {
        let Some(open) = tokens[at..].iter().position(|t| *t == Token::LCurly) else { break };
        let Some(close) = tokens[at..].iter().position(|t| *t == Token::RCurly) else { break };
        out.push((&tokens[at + 1..at + open], &tokens[at + open + 1..at + close]));
        at += close + 1;
    }
    out
}

fn parse_sounds(tokens: &[Token]) -> Vec<EventSound> {
    records(tokens, 0x2a)
        .into_iter()
        .map(|(header, fields)| {
            let mut sound = EventSound {
                event: number(header.first()) as i32,
                sound: 0,
                emitter: Emitter::at(Vec3::ZERO),
                looping: false,
                no_end: false,
                on_end: header.contains(&Token::Key(0x3c)),
                at_event: false,
                chance: None,
                active: false,
                retrigger: 0.0,
            };
            for (i, token) in fields.iter().enumerate() {
                let value = |n: usize| number(fields.get(i + 1 + n));
                match token {
                    Token::Key(0x2c) => sound.sound = value(0) as usize,
                    Token::Key(0x2d) => sound.looping = true,
                    Token::Key(0x2f) => sound.emitter.volume = value(0),
                    Token::Key(0x30) => sound.emitter.pitch = value(0),
                    Token::Key(0x31) => sound.emitter.range.0 = value(0),
                    Token::Key(0x32) => sound.emitter.range.1 = value(0),
                    Token::Key(0x3a) => sound.no_end = true,
                    Token::Key(0x3b) => sound.emitter.pos = to_world([value(0), value(1), value(2)]),
                    Token::Key(0x3f) => sound.at_event = true,
                    Token::Key(0x40) => sound.chance = Some(value(0)).filter(|&c| (c * 255.0) as u8 != 255),
                    _ => {}
                }
            }
            sound
        })
        .collect()
}

fn parse_timers(tokens: &[Token]) -> Vec<Timer> {
    records(tokens, 0x27)
        .into_iter()
        .map(|(_, fields)| {
            let mut timer = Timer { on: (0.0, false), off: (0.0, false), delay: 0.0, event: -1, active: false, remaining: None };
            for (i, token) in fields.iter().enumerate() {
                // A phase's length may be marked as random.
                let phase = || {
                    let random = fields.get(i + 1) == Some(&Token::Key(0x2b));
                    (number(fields.get(i + 1 + random as usize)) / 1000.0, random)
                };
                match token {
                    Token::Key(0x28) => timer.on = phase(),
                    Token::Key(0x29) => timer.off = phase(),
                    Token::Key(0x2a) => timer.event = number(fields.get(i + 1)) as i32,
                    Token::Key(0x2d) => timer.delay = number(fields.get(i + 1)) / 1000.0,
                    _ => {}
                }
            }
            timer
        })
        .collect()
}

/// Loads the events of a race (a folder name such as `RACEC0R0`).
pub fn load(race: &str) -> Option<TrackEvents> {
    let jam = Jam::open(std::env::var("LEGO_JAM").unwrap_or("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM".into()))?;
    let dir = format!("/GAMEDATA/{race}");
    let mut files: Vec<&str> = jam.list(&dir).collect();
    files.sort();
    let with_ext = |ext: &'static str| files.iter().copied().filter(move |f| f.ends_with(ext)).filter_map(|f| jam.get(f));
    let mut events = TrackEvents::default();
    for data in with_ext(".EVB") {
        events.sounds.extend(parse_sounds(&tokenize(data)));
    }
    for data in with_ext(".TRB") {
        events.triggers.extend(route::parse_triggers(data).into_iter().map(|(centre, radius, event)| Trigger {
            centre: to_world(centre),
            radius: radius * UNIT,
            event,
            active: false,
        }));
    }
    for data in with_ext(".TIB") {
        events.timers.extend(parse_timers(&tokenize(data)));
    }
    Some(events)
}

/// A phase of a timer: as long as it says, or some random part of that.
fn phase_length((length, random): (f32, bool), sfx: &mut Sfx) -> f32 {
    if random { length * sfx.roll(1024) as f32 / 1023.0 } else { length }
}

impl TrackEvents {
    fn retrigger_delay(sfx: &mut Sfx) -> f32 {
        RETRIGGER.0 + sfx.roll(1000) as f32 * 0.001 * RETRIGGER.1
    }

    /// `SoundResource::OnStartAt`.
    fn start_sound(sound: &mut EventSound, at: Option<Vec3>, sfx: &mut Sfx) {
        if let (Some(at), true) = (at, sound.at_event) {
            sound.emitter.pos = at;
        }
        if sound.chance.is_some() || sound.looping {
            sound.active = true;
        } else {
            sfx.emit(sound.sound, sound.emitter);
        }
    }

    /// `RaceEventTable::StartEventsAt`, for the sounds.
    fn start(&mut self, event: i32, at: Option<Vec3>, sfx: &mut Sfx) {
        for sound in self.sounds.iter_mut().filter(|s| s.event == event) {
            if !sound.on_end && !sound.active {
                Self::start_sound(sound, at, sfx);
            }
        }
    }

    /// `RaceEventTable::EndEventsAt`, for the sounds.
    fn end(&mut self, event: i32, at: Option<Vec3>, sfx: &mut Sfx) {
        for sound in self.sounds.iter_mut().filter(|s| s.event == event) {
            if sound.on_end && !sound.active {
                Self::start_sound(sound, at, sfx);
            }
            if !sound.no_end {
                sound.active = false;
            }
        }
    }
}

pub fn track_events(
    time: Res<Time>,
    race: Res<Race>,
    events: Option<ResMut<TrackEvents>>,
    mut sfx: ResMut<Sfx>,
    karts: Query<(Entity, &Kart)>,
) {
    let Some(mut events) = events else { return };
    let events = &mut *events;
    let dt = time.delta_secs().min(0.05);
    if race.phase == Phase::Intro {
        // A fresh start: nothing is under way.
        for sound in &mut events.sounds {
            sound.active = false;
            sound.retrigger = TrackEvents::retrigger_delay(&mut sfx);
        }
        events.triggers.iter_mut().for_each(|t| t.active = false);
        for timer in &mut events.timers {
            timer.active = false;
            timer.remaining = (timer.delay <= 0.0).then(|| phase_length(timer.off, &mut sfx));
        }
        events.surfaces.clear();
    }

    // Trigger spheres: their events run for as long as any racer is inside.
    for i in 0..events.triggers.len() {
        let trigger = &events.triggers[i];
        let (centre, radius, event, active) = (trigger.centre, trigger.radius, trigger.event, trigger.active);
        let touched = karts.iter().any(|(_, k)| k.pos.distance_squared(centre) < radius * radius);
        events.triggers[i].active = touched;
        match (touched, active) {
            (true, false) => events.start(event, Some(centre), &mut sfx),
            (false, true) => events.end(event, Some(centre), &mut sfx),
            _ => {}
        }
    }

    // Surfaces that set off events as racers drive on and off them.
    for (entity, k) in &karts {
        let now = [k.surface.enter_event, k.surface.leave_event, k.surface.touch_event];
        let before = events.surfaces.insert(entity, now).unwrap_or_default();
        if now != before {
            if let Some(event) = before[1] {
                events.end(event, Some(k.pos), &mut sfx);
            }
            for event in [now[0], now[2]].into_iter().flatten() {
                events.start(event, Some(k.pos), &mut sfx);
            }
        }
    }

    // Timers: on for a while, off for a while.
    for i in 0..events.timers.len() {
        let timer = &mut events.timers[i];
        let Some(remaining) = &mut timer.remaining else {
            timer.delay -= dt;
            if timer.delay <= 0.0 {
                timer.remaining = Some(phase_length(timer.on, &mut sfx));
            }
            continue;
        };
        *remaining -= dt;
        if *remaining > 0.0 {
            continue;
        }
        let (event, was_active) = (timer.event, timer.active);
        timer.active = !was_active;
        timer.remaining = Some(phase_length(if was_active { timer.off } else { timer.on }, &mut sfx));
        if was_active {
            events.end(event, None, &mut sfx);
        } else {
            events.start(event, None, &mut sfx);
        }
    }

    // Sounds under way: loops keep going, and the occasional ones take their chances.
    for (i, sound) in events.sounds.iter_mut().enumerate().filter(|s| s.1.active) {
        match sound.chance {
            Some(chance) => {
                sound.retrigger -= dt;
                if sound.retrigger <= 0.0 {
                    sound.retrigger = TrackEvents::retrigger_delay(&mut sfx);
                    if (sfx.roll(255) as f32) < (chance * 255.0).floor() {
                        sfx.emit(sound.sound, sound.emitter);
                    }
                }
            }
            None => sfx.sustain_global(LOOP_SLOT + i as u16, sound.sound, sound.emitter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{NEAR, id};

    /// Needs the original game data; silently passes without it.
    #[test]
    fn every_circuit_has_ambient_sounds_wired_to_triggers() {
        for (race, name) in crate::world::circuits() {
            let events = load(&race).unwrap();
            assert!(!events.sounds.is_empty() && !events.triggers.is_empty(), "{name}");
            let triggered = events.sounds.iter().filter(|s| events.triggers.iter().any(|t| t.event == s.event)).count();
            let loops = events.sounds.iter().filter(|s| s.looping).count();
            println!(
                "{race} {name}: {} sounds ({triggered} on triggers, {loops} loops), {} triggers, {} timers",
                events.sounds.len(),
                events.triggers.len(),
                events.timers.len()
            );
            assert!(triggered > 0, "{name}");
            for sound in &events.sounds {
                assert!(sound.emitter.range.0 > 0.0 && sound.emitter.range.1 >= sound.emitter.range.0, "{name}");
                assert!(sound.emitter.volume > 0.0 && sound.emitter.volume <= 1.0, "{name}");
            }
        }
    }

    #[test]
    fn sounds_start_and_stop_with_their_events() {
        let sound = |event, looping, chance, on_end| EventSound {
            event,
            sound: id::AMBIENT,
            emitter: Emitter::at(Vec3::ZERO).range(NEAR.0, NEAR.1),
            looping,
            no_end: false,
            on_end,
            at_event: true,
            chance,
            active: false,
            retrigger: 0.0,
        };
        let mut events = TrackEvents { sounds: vec![sound(1, true, None, false), sound(2, false, Some(0.5), false)], ..default() };
        let mut sfx = Sfx::default();
        events.start(1, Some(Vec3::X), &mut sfx);
        events.start(2, None, &mut sfx);
        assert!(events.sounds[0].active && events.sounds[1].active);
        assert_eq!(events.sounds[0].emitter.pos, Vec3::X);
        events.end(1, None, &mut sfx);
        assert!(!events.sounds[0].active && events.sounds[1].active);
    }
}
