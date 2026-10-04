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
use crate::scenery::{Animated, Scenery};
use crate::track::Track;
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
    /// The computer's cars pass through without setting it off.
    players_only: bool,
    /// Only racers who have finished this many laps set it off.
    lap: Option<i32>,
}

/// An event starting or ending, for the hazards and animations that hang on it.
pub struct Fired {
    pub event: i32,
    pub start: bool,
    pub at: Option<Vec3>,
    /// The racer who set it off, for events that are about one racer.
    pub racer: Option<Entity>,
}

/// Plays parts of a model's animation as an event comes and goes
/// (`PartAnimationResource`): an optional start part, an active part, an optional
/// end part, then back to the idle part.
struct PartAnimation {
    event: i32,
    on_end: bool,
    prop: String,
    active: usize,
    idle: usize,
    start: Option<usize>,
    end: Option<usize>,
    looping: bool,
    no_end: bool,
    /// Events that run while the start, active and end parts play.
    state_events: [Option<i32>; 3],
    /// 0 idle, 1 starting, 2 active, 3 ending.
    state: u8,
}

/// Holds another event open for a while once its own starts — or, for some, once
/// its own ends (`TimerResource`).
struct Delay {
    event: i32,
    on_end: bool,
    seconds: f32,
    then: Option<i32>,
    remaining: Option<f32>,
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
    animations: Vec<PartAnimation>,
    delays: Vec<Delay>,
    /// Collision volumes that open while a model is away from its resting part.
    doors: Vec<(String, String)>,
    /// Which racers are inside each trigger.
    inside: Vec<Vec<Entity>>,
    /// Everything that has started or ended since the hazards last looked.
    pub fired: Vec<Fired>,
    /// What events do to the sky, and the changes asked for since the sky last looked.
    skies: Vec<crate::sky::Change>,
    pub sky: Vec<crate::sky::Change>,
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

fn parse_animations(tokens: &[Token]) -> Vec<PartAnimation> {
    records(tokens, 0x28)
        .into_iter()
        .map(|(header, fields)| {
            let mut animation = PartAnimation {
                event: number(header.first()) as i32,
                on_end: header.contains(&Token::Key(0x3c)),
                prop: String::new(),
                active: 0,
                idle: 0,
                start: None,
                end: None,
                looping: false,
                no_end: false,
                state_events: [None; 3],
                state: 0,
            };
            let mut i = 0;
            while i < fields.len() {
                let value = number(fields.get(i + 1)) as usize;
                match &fields[i] {
                    // `event <state> <id>`: an event tied to one of the parts.
                    Token::Key(0x27) => {
                        let state = match fields.get(i + 1) {
                            Some(Token::Key(0x36)) => 0,
                            Some(Token::Key(0x34)) => 1,
                            _ => 2,
                        };
                        animation.state_events[state] = Some(number(fields.get(i + 2)) as i32);
                        i += 2;
                    }
                    Token::Key(0x33) => {
                        if let Some(Token::Str(name)) = fields.get(i + 1) {
                            animation.prop = name.to_lowercase();
                        }
                    }
                    Token::Key(0x34) => animation.active = value,
                    Token::Key(0x35) => animation.idle = value,
                    Token::Key(0x36) => animation.start = Some(value),
                    Token::Key(0x37) => animation.end = Some(value),
                    Token::Key(0x2d) => animation.looping = true,
                    Token::Key(0x3a) => animation.no_end = true,
                    _ => {}
                }
                i += 1;
            }
            animation
        })
        .collect()
}

fn parse_delays(tokens: &[Token]) -> Vec<Delay> {
    records(tokens, 0x4b)
        .into_iter()
        .map(|(header, fields)| {
            let after = |key: u16| fields.iter().position(|t| *t == Token::Key(key));
            Delay {
                event: number(header.first()) as i32,
                on_end: header.contains(&Token::Key(0x3c)),
                seconds: after(0x49).map_or(0.0, |i| number(fields.get(i + 1)) / 1000.0),
                then: after(0x27).map(|i| number(fields.get(i + 2)) as i32),
                remaining: None,
            }
        })
        .collect()
}

/// The (model, collision volume) pairs of the node transforms section.
fn parse_doors(tokens: &[Token]) -> Vec<(String, String)> {
    let name = |fields: &[Token], key: u16| {
        let at = fields.iter().position(|t| *t == Token::Key(key))?;
        match fields.get(at + 1)? {
            Token::Str(name) => Some(name.to_lowercase()),
            _ => None,
        }
    };
    records(tokens, 0x52).into_iter().filter_map(|(_, fields)| Some((name(fields, 0x33)?, name(fields, 0x4a)?))).collect()
}

/// The sky state records: `event [on end] { name, time, what to hide and show }`.
fn parse_skies(tokens: &[Token]) -> Vec<crate::sky::Change> {
    records(tokens, 0x42)
        .into_iter()
        .map(|(header, fields)| {
            let after = |key: u16| fields.iter().position(|t| *t == Token::Key(key)).and_then(|at| fields.get(at + 1));
            let has = |key: u16| fields.contains(&Token::Key(key));
            crate::sky::Change {
                event: number(header.first()) as i32,
                on_end: header.contains(&Token::Key(0x3c)),
                state: match after(0x43) {
                    Some(Token::Str(name)) => Some(name.to_lowercase()),
                    _ => None,
                },
                seconds: after(0x44).map_or(0.0, |t| number(Some(t)) / 1000.0),
                dome: if has(0x45) { Some(false) } else { has(0x46).then_some(true) },
                world: if has(0x47) { Some(false) } else { has(0x48).then_some(true) },
            }
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
        let tokens = tokenize(data);
        events.sounds.extend(parse_sounds(&tokens));
        events.animations.extend(parse_animations(&tokens));
        events.delays.extend(parse_delays(&tokens));
        events.doors.extend(parse_doors(&tokens));
        events.skies.extend(parse_skies(&tokens));
    }
    for data in with_ext(".TRB") {
        events.triggers.extend(route::parse_triggers(data).into_iter().map(|t| Trigger {
            centre: to_world(t.centre),
            radius: t.radius * UNIT,
            event: t.event,
            active: false,
            players_only: t.players_only,
            lap: t.lap,
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

    /// Starts the events that timers hold open after `event` starts or ends.
    fn hold(&mut self, event: i32, ended: bool, sfx: &mut Sfx) {
        let mut held = Vec::new();
        for delay in &mut self.delays {
            if delay.event == event && delay.on_end == ended && delay.remaining.is_none() {
                delay.remaining = Some(delay.seconds);
                held.extend(delay.then);
            }
        }
        for event in held {
            self.start(event, None, sfx);
        }
    }

    /// Starts and at once ends an event (`RaceEventTable::FireEventsAt`).
    pub fn fire(&mut self, event: i32, at: Option<Vec3>, sfx: &mut Sfx) {
        self.start(event, at, sfx);
        self.end(event, at, sfx);
    }

    /// `RaceEventTable::StartEventsAt`.
    pub fn start(&mut self, event: i32, at: Option<Vec3>, sfx: &mut Sfx) {
        debug!("event {event} starts");
        self.fired.push(Fired { event, start: true, at, racer: None });
        self.sky.extend(self.skies.iter().filter(|s| s.event == event && !s.on_end).cloned());
        self.hold(event, false, sfx);
        for sound in self.sounds.iter_mut().filter(|s| s.event == event) {
            if !sound.on_end && !sound.active {
                Self::start_sound(sound, at, sfx);
            }
        }
    }

    /// `RaceEventTable::EndEventsAt`.
    pub fn end(&mut self, event: i32, at: Option<Vec3>, sfx: &mut Sfx) {
        debug!("event {event} ends");
        self.fired.push(Fired { event, start: false, at, racer: None });
        self.sky.extend(self.skies.iter().filter(|s| s.event == event && s.on_end).cloned());
        self.hold(event, true, sfx);
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
    mut karts: Query<(Entity, &mut Kart, Has<crate::kart::Player>)>,
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
        events.inside.clear();
        events.fired.clear();
        events.delays.iter_mut().for_each(|d| d.remaining = None);
        events.animations.iter_mut().for_each(|a| a.state = 0);
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
        let (players_only, lap) = (trigger.players_only, trigger.lap);
        if event < 0 {
            continue;
        }
        let inside: Vec<Entity> = karts
            .iter()
            // `RacerTriggerList::Entry::OnEvent`: a lap's trigger is for racers on that lap.
            .filter(|(_, k, player)| (*player || !players_only) && lap.is_none_or(|lap| k.lap - 1 == lap))
            .filter(|(_, k, _)| k.pos.distance_squared(centre) < radius * radius)
            .map(|(e, ..)| e)
            .collect();
        let touched = !inside.is_empty();
        events.triggers[i].active = touched;
        // Each racer's own comings and goings matter to some hazards.
        events.inside.resize(events.triggers.len(), Vec::new());
        let before = std::mem::replace(&mut events.inside[i], inside.clone());
        let at = Some(centre);
        for &racer in inside.iter().filter(|e| !before.contains(e)) {
            events.fired.push(Fired { event, start: true, at, racer: Some(racer) });
        }
        for &racer in before.iter().filter(|e| !inside.contains(e)) {
            events.fired.push(Fired { event, start: false, at, racer: Some(racer) });
        }
        match (touched, active) {
            (true, false) => events.start(event, Some(centre), &mut sfx),
            (false, true) => events.end(event, Some(centre), &mut sfx),
            _ => {}
        }
    }

    // Surfaces that set off events as racers drive on and off them.
    for (entity, mut k, _) in &mut karts {
        // Driving through a surface that isn't solid, or sounding the horn, are events too.
        let horn = std::mem::take(&mut k.honked).then_some(999);
        for event in [k.touched.take(), horn].into_iter().flatten() {
            events.fired.push(Fired { event, start: true, at: Some(k.pos), racer: Some(entity) });
            events.fire(event, Some(k.pos), &mut sfx);
            events.fired.push(Fired { event, start: false, at: Some(k.pos), racer: Some(entity) });
        }
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

    // Events held open for a while come to their end.
    for i in 0..events.delays.len() {
        let delay = &mut events.delays[i];
        let Some(remaining) = &mut delay.remaining else { continue };
        *remaining -= dt;
        if *remaining <= 0.0 {
            delay.remaining = None;
            if let Some(then) = delay.then {
                events.end(then, None, &mut sfx);
            }
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

/// Runs the animations that events drive, and opens the collision volumes that move
/// with them.
pub fn part_animations(
    events: Option<ResMut<TrackEvents>>,
    scenery: Option<Res<Scenery>>,
    track: Option<ResMut<Track>>,
    mut sfx: ResMut<Sfx>,
    mut props: Query<&mut Animated>,
) {
    let (Some(mut events), Some(scenery), Some(mut track)) = (events, scenery, track) else { return };
    let events = &mut *events;
    let mut notices: Vec<(Option<i32>, Option<i32>)> = Vec::new();
    for animation in &mut events.animations {
        let Some(mut prop) = scenery.0.get(&animation.prop).and_then(|&e| props.get_mut(e).ok()) else { continue };
        let once = |part: usize| Some((part, false));
        let idle = Some((animation.idle, true));
        for fired in events.fired.iter().filter(|f| f.event == animation.event && f.racer.is_none()) {
            let resting = animation.state == 0 || animation.state == 3;
            if fired.start != animation.on_end && resting {
                // `OnStartAt`: the start part if there is one, else straight to the active part.
                prop.queued = animation.start.map_or(Some((animation.active, true)), once);
            }
            if !fired.start && !animation.no_end && animation.state != 0 && prop.part != animation.idle {
                prop.queued = match animation.end {
                    Some(end) if prop.part != end => once(end),
                    _ => idle,
                };
            }
        }
        // Each part hands on to the next when it has played out.
        if prop.queued.is_none() {
            match animation.state {
                1 if Some(prop.part) == animation.start => prop.queued = Some((animation.active, animation.looping)),
                2 if !animation.looping && prop.part == animation.active => {
                    prop.queued = animation.end.map_or(idle, once);
                }
                3 if Some(prop.part) == animation.end => prop.queued = idle,
                _ => {}
            }
        }
        let state = if Some(prop.part) == animation.start {
            1
        } else if prop.part == animation.active {
            2
        } else if Some(prop.part) == animation.end {
            3
        } else {
            0
        };
        // Events tied to the parts start and end as the parts do (`NotifyStateChange`).
        if state != animation.state && !(state == 2 && animation.state == 0 && animation.active == animation.idle) {
            let tied = |state: u8| state.checked_sub(1).and_then(|s| animation.state_events[s as usize]);
            notices.push((tied(animation.state), tied(state)));
            animation.state = state;
        }
    }
    for (ending, starting) in notices {
        if let Some(event) = ending {
            events.end(event, None, &mut sfx);
        }
        if let Some(event) = starting {
            events.start(event, None, &mut sfx);
        }
    }
    for (prop, volume) in &events.doors {
        let resting = events.animations.iter().find(|a| a.prop == *prop).map(|a| a.idle);
        let (Some(resting), Some(animated)) = (resting, scenery.0.get(prop).and_then(|&e| props.get(e).ok())) else {
            continue;
        };
        if let Some(&(tag, _)) = track.surfaces.get(volume) {
            track.collision.set_passable(tag, animated.part != resting);
        }
    }
}

#[cfg(test)]
#[test]
fn the_moon_s_events_change_its_sky() {
    use crate::sky::Change;
    let Some(events) = load("RACEC0R3") else { return };
    // Event 50 flashes the sky as it starts, and lets it back to the open air as it ends.
    let change = |on_end, state: &str, seconds| Change { event: 50, on_end, state: Some(state.into()), seconds, dome: None, world: None };
    assert_eq!(events.skies, [change(false, "flash", 0.25), change(true, "openair", 0.5)]);
    // The castle has a second sky, and nothing that asks for it.
    assert!(load("RACEC0R0").unwrap().skies.is_empty());
}
