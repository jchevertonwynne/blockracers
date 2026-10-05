//! What the keys and a gamepad are taken to mean: the nine things a player can do in
//! a race (`PlayerControls::InputState`), what each is bound to
//! (`GameState::SetInputEvent`, `GetInputEvent`), a stick's dead zone
//! (`JoystickDevice`) and the shaking of a pad (`RaceForceFeedback`).
//!
//! The original keeps a set of bindings for each joystick and three for the keyboard,
//! and a player races with the one set they picked. Here there is one player to a
//! game, so every set works at once: the pad's, and two of the keyboard's. A pad
//! also works the menus, which is the port's own: its buttons are passed on as the
//! keys the menus know.
//!
//! Not here: the sine a force-feedback wheel hums with as the engine turns
//! (`RaceForceFeedback::CreateEngineEffect`), which a pad's motors have nothing for,
//! and the accelerator on an axis, which the original gives one make of wheel only.

use std::time::Duration;

use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::prelude::*;
use serde::Deserialize;
use serde::de::value::{Error, StrDeserializer};

use crate::kart::{Kart, Player};
use crate::menu::Settings;
use crate::physics::UNIT;
use crate::{Phase, Race, Screen};

/// The things bound, in the original's order.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Event {
    Left,
    Right,
    Accelerate,
    Brake,
    Powerup,
    Camera,
    Display,
    Slide,
    LookBack,
}

pub const EVENTS: usize = 9;
/// The sets of bindings: the pad's, then the keyboard's.
pub const ENTRIES: usize = 3;
pub const PAD: usize = 0;

/// What one thing is bound to in one set. A pad's set may have keys in it, as the
/// original's may; a keyboard's has no buttons.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Bound {
    None,
    Key(KeyCode),
    Button(GamepadButton),
}

/// `JoystickDevice::c_defaultDeadZonePercent`: this much of a stick's travel, either
/// side of the middle, is nothing; the rest is the whole of its range.
const DEAD_ZONE: f32 = 0.35;
/// How far a stick is pushed before the menus take it for an arrow key.
const STICK_AS_KEY: f32 = 0.5;

/// The keys the original won't have bound (`g_controlConfigBlockedEvents`).
const BLOCKED: [KeyCode; 11] = [
    KeyCode::Escape,
    KeyCode::PrintScreen,
    KeyCode::ScrollLock,
    KeyCode::Pause,
    KeyCode::NumLock,
    KeyCode::SuperLeft,
    KeyCode::SuperRight,
    KeyCode::ContextMenu,
    KeyCode::Power,
    KeyCode::Sleep,
    KeyCode::WakeUp,
];

/// What each set starts with. The keyboard's are the port's keys, the arrows and
/// the letters; the original numbers a joystick's buttons off in order, which says
/// nothing of where they are on a pad, so the pad's are the port's too.
fn default(entry: usize, event: usize) -> Bound {
    use GamepadButton as B;
    use KeyCode as K;
    const PAD_BUTTONS: [GamepadButton; EVENTS] = [
        B::DPadLeft,
        B::DPadRight,
        B::RightTrigger2,
        B::LeftTrigger2,
        B::South,
        B::North,
        B::Select,
        B::West,
        B::East,
    ];
    const KEYS: [[KeyCode; EVENTS]; 2] = [
        [
            K::ArrowLeft,
            K::ArrowRight,
            K::ArrowUp,
            K::ArrowDown,
            K::Space,
            K::KeyC,
            K::Tab,
            K::ShiftRight,
            K::KeyV,
        ],
        [
            K::KeyA,
            K::KeyD,
            K::KeyW,
            K::KeyS,
            K::AltLeft,
            K::KeyQ,
            K::KeyE,
            K::ShiftLeft,
            K::KeyZ,
        ],
    ];
    match entry {
        PAD => Bound::Button(PAD_BUTTONS[event]),
        entry => Bound::Key(KEYS[entry - 1][event]),
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Bindings(pub [[Bound; EVENTS]; ENTRIES]);

impl Default for Bindings {
    fn default() -> Self {
        Bindings(std::array::from_fn(|entry| {
            std::array::from_fn(|event| default(entry, event))
        }))
    }
}

impl Bindings {
    /// `GameState::IsInputEventBound`: a key is bound if any set has it, a button if
    /// this set has.
    fn bound(&self, entry: usize, to: Bound) -> bool {
        match to {
            Bound::None => false,
            Bound::Key(_) => self.0.iter().flatten().any(|bound| *bound == to),
            Bound::Button(_) => self.0[entry].contains(&to),
        }
    }

    /// `GameState::GetInputEvent`: something bound to nothing has what it began with
    /// back, if nothing else has taken that since.
    fn settle(&mut self) {
        for entry in 0..ENTRIES {
            for event in 0..EVENTS {
                let first = default(entry, event);
                if self.0[entry][event] == Bound::None && !self.bound(entry, first) {
                    self.0[entry][event] = first;
                }
            }
        }
    }

    /// Whether the original would take this for a binding in this set: not one of
    /// the keys it keeps back, and a button only in the pad's own.
    pub fn allowed(entry: usize, to: Bound) -> bool {
        match to {
            Bound::None => false,
            Bound::Key(key) => !BLOCKED.contains(&key),
            Bound::Button(_) => entry == PAD,
        }
    }

    /// `GameState::SetInputEvent`: binds something, taking the key from whatever had
    /// it in any set, or the button from whatever had it in this one.
    pub fn set(&mut self, entry: usize, event: usize, to: Bound) {
        if self.0[entry][event] != to {
            for (other, events) in self.0.iter_mut().enumerate() {
                if matches!(to, Bound::Key(_)) || other == entry {
                    for bound in events.iter_mut().filter(|bound| **bound == to) {
                        *bound = Bound::None;
                    }
                }
            }
            self.0[entry][event] = to;
        }
        self.settle();
    }

    /// The lines the bindings are kept as, one to a set.
    pub fn write(&self) -> String {
        let name = |bound: &Bound| match bound {
            Bound::None => "-".to_string(),
            Bound::Key(key) => format!("{key:?}"),
            Bound::Button(button) => format!("pad:{button:?}"),
        };
        let line = |(entry, events): (usize, &[Bound; EVENTS])| {
            let names: Vec<String> = events.iter().map(name).collect();
            format!("controls{entry}={}\n", names.join(","))
        };
        self.0.iter().enumerate().map(line).collect()
    }

    /// Takes one of `write`'s lines, if this is one. Anything in it that can't be
    /// read is left bound to nothing.
    pub fn read(&mut self, line: &str) -> bool {
        let Some((entry, names)) = line
            .strip_prefix("controls")
            .and_then(|rest| rest.split_once('='))
            .and_then(|(entry, names)| Some((entry.parse::<usize>().ok()?, names)))
            .filter(|(entry, _)| *entry < ENTRIES)
        else {
            return false;
        };
        let named = |name: &str| {
            let word = |name| StrDeserializer::<Error>::new(name);
            let bound = match name.strip_prefix("pad:") {
                Some(button) => GamepadButton::deserialize(word(button)).map(Bound::Button),
                None => KeyCode::deserialize(word(name)).map(Bound::Key),
            };
            bound.ok().filter(|bound| Self::allowed(entry, *bound))
        };
        let mut names = names.trim().split(',');
        for event in 0..EVENTS {
            self.0[entry][event] = names.next().and_then(named).unwrap_or(Bound::None);
        }
        true
    }

    /// Makes good what `read` left: nothing bound twice, the first to have a key or
    /// a button keeping it, and nothing unbound that could have what it began with.
    pub fn mend(&mut self) {
        let read = std::mem::replace(self, Bindings([[Bound::None; EVENTS]; ENTRIES]));
        for (entry, events) in read.0.iter().enumerate() {
            for (event, bound) in events.iter().enumerate() {
                if !self.bound(entry, *bound) {
                    self.0[entry][event] = *bound;
                }
            }
        }
        self.settle();
    }
}

/// A binding as the controls page shows it, in the capitals of the game's lettering.
pub fn name(bound: Bound) -> String {
    use GamepadButton as B;
    let key = match bound {
        Bound::None => return String::new(),
        Bound::Button(button) => {
            return match button {
                B::South => "A".into(),
                B::East => "B".into(),
                B::West => "X".into(),
                B::North => "Y".into(),
                B::LeftTrigger => "LB".into(),
                B::RightTrigger => "RB".into(),
                B::LeftTrigger2 => "LT".into(),
                B::RightTrigger2 => "RT".into(),
                B::Select => "BACK".into(),
                B::LeftThumb => "LEFT STICK".into(),
                B::RightThumb => "RIGHT STICK".into(),
                B::DPadUp => "DPAD UP".into(),
                B::DPadDown => "DPAD DOWN".into(),
                B::DPadLeft => "DPAD LEFT".into(),
                B::DPadRight => "DPAD RIGHT".into(),
                B::Other(number) => format!("BUTTON {number}"),
                other => format!("{other:?}").to_uppercase(),
            };
        }
        Bound::Key(key) => format!("{key:?}"),
    };
    // `KeyW`, `Digit1`, `ArrowLeft`, `ShiftRight`, `NumpadEnter`: the last word of
    // a key with a side to it goes first.
    let plain = ["Key", "Digit", "Arrow"]
        .iter()
        .find_map(|prefix| key.strip_prefix(prefix))
        .unwrap_or(&key);
    let mut words: Vec<String> = Vec::new();
    for letter in plain.chars() {
        match words.last_mut() {
            Some(word) if !letter.is_uppercase() => word.push(letter.to_ascii_uppercase()),
            _ => words.push(letter.to_string()),
        }
    }
    if words.len() > 1 && matches!(words.last().map(String::as_str), Some("LEFT" | "RIGHT")) {
        words.rotate_right(1);
    }
    words.join(" ")
}

/// What the player is asking of their car this frame.
#[derive(Resource, Default)]
pub struct Actions {
    held: [bool; EVENTS],
    pressed: [bool; EVENTS],
    /// A stick's steering, left of the middle being more than nought.
    pub stick: f32,
}

impl Actions {
    pub fn held(&self, event: Event) -> bool {
        self.held[event as usize]
    }

    pub fn pressed(&self, event: Event) -> bool {
        self.pressed[event as usize]
    }
}

/// What is plugged in, and what the controls page is doing with it.
#[derive(Resource, Default)]
pub struct Devices {
    /// There is a pad.
    pub pad: bool,
    /// The pad's buttons that went down this frame.
    pub pressed: Vec<GamepadButton>,
    /// The set of bindings the controls page is showing.
    pub shown: usize,
    /// The thing the controls page is waiting to be given a key or a button for;
    /// while it waits the pad works no menu.
    pub awaiting: Option<usize>,
}

impl Devices {
    /// The set of bindings shown: the pad's only while there is a pad.
    pub fn entry(&self) -> usize {
        if self.shown == PAD && !self.pad {
            PAD + 1
        } else {
            self.shown
        }
    }

    /// Goes on to the next of the sets there are devices for, or back to the last.
    pub fn turn(&mut self, by: i32) {
        let first = if self.pad { PAD } else { PAD + 1 };
        let sets = (ENTRIES - first) as i32;
        self.shown = first + (self.entry() as i32 - first as i32 + by).rem_euclid(sets) as usize;
    }
}

/// A stick's travel with the dead zone taken out of it.
fn past_dead_zone(axis: f32) -> f32 {
    let travel = (axis.abs() - DEAD_ZONE).max(0.0) / (1.0 - DEAD_ZONE);
    travel.min(1.0).copysign(axis)
}

/// Reads the keys and the pads into what they are bound to.
pub fn read(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    settings: Res<Settings>,
    mut actions: ResMut<Actions>,
    mut devices: ResMut<Devices>,
) {
    devices.pad = !pads.is_empty();
    devices.pressed = pads
        .iter()
        .flat_map(|pad| pad.get_just_pressed().copied())
        .collect();
    let down = |bound: &Bound| match *bound {
        Bound::None => false,
        Bound::Key(key) => keys.pressed(key),
        Bound::Button(button) => pads.iter().any(|pad| pad.pressed(button)),
    };
    for event in 0..EVENTS {
        let held = settings.controls.0.iter().any(|entry| down(&entry[event]));
        actions.pressed[event] = held && !actions.held[event];
        actions.held[event] = held;
    }
    // `PlayerControls::UpdateSteering` turns the stick's axis about.
    actions.stick = pads
        .iter()
        .map(|pad| -past_dead_zone(pad.get(GamepadAxis::LeftStickX).unwrap_or(0.0)))
        .fold(0.0, |most: f32, stick| {
            if stick.abs() > most.abs() {
                stick
            } else {
                most
            }
        });
}

/// Something on a pad that can stand for a key.
#[derive(Clone, Copy, PartialEq)]
enum Source {
    Button(GamepadButton),
    /// The stick pushed up, down, left or right.
    Stick(usize),
}

/// The key a pad's button is to the menus, and to the bench bricks are put on a car
/// at; with a car to drive, a pad has only the way to the pause menu.
fn key_for(source: Source, driving: bool, racing: bool) -> Option<KeyCode> {
    use GamepadButton as B;
    use KeyCode as K;
    const ARROWS: [KeyCode; 4] = [K::ArrowUp, K::ArrowDown, K::ArrowLeft, K::ArrowRight];
    let button = match source {
        Source::Stick(way) => return (!driving).then_some(ARROWS[way]),
        Source::Button(button) => button,
    };
    if button == B::Start {
        return Some(if racing { K::Escape } else { K::Enter });
    }
    if driving {
        return None;
    }
    Some(match button {
        B::DPadUp => K::ArrowUp,
        B::DPadDown => K::ArrowDown,
        B::DPadLeft => K::ArrowLeft,
        B::DPadRight => K::ArrowRight,
        B::South => K::Enter,
        B::East => K::Escape,
        B::West => K::Backspace,
        B::North => K::KeyR,
        B::LeftTrigger => K::KeyT,
        B::RightTrigger => K::Tab,
        B::LeftTrigger2 => K::Comma,
        B::RightTrigger2 => K::Period,
        _ => return None,
    })
}

/// Presses the keys a pad's buttons stand for, and lets them go with the buttons.
fn pad_keys(
    pads: Query<&Gamepad>,
    devices: Res<Devices>,
    screen: Res<State<Screen>>,
    race: Option<Res<Race>>,
    pause: Res<crate::Pause>,
    photo: Res<crate::replay::Photo>,
    watching: Option<Res<crate::net::Watching>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut was: Local<Vec<Source>>,
    mut held: Local<Vec<(Source, KeyCode)>>,
) {
    let mut down: Vec<Source> = Vec::new();
    for pad in &pads {
        down.extend(pad.get_pressed().map(|button| Source::Button(*button)));
        let stick = pad.left_stick();
        let ways = [
            stick.y > STICK_AS_KEY,
            stick.y < -STICK_AS_KEY,
            stick.x < -STICK_AS_KEY,
            stick.x > STICK_AS_KEY,
        ];
        down.extend((0..4).filter(|way| ways[*way]).map(Source::Stick));
    }
    held.retain(|(source, key)| {
        let still = down.contains(source);
        if !still {
            keys.release(*key);
        }
        still
    });
    let racing = *screen.get() == Screen::Race;
    let driving = racing
        && pause.0.is_none()
        && photo.0.is_none()
        && race.is_some_and(|race| race.phase != Phase::Finished)
        && !watching.is_some_and(|watching| watching.free);
    if devices.awaiting.is_none() {
        for source in down.iter().filter(|source| !was.contains(source)) {
            if let Some(key) = key_for(*source, driving, racing) {
                keys.press(key);
                held.push((*source, key));
            }
        }
    }
    *was = down;
}

/// `RaceForceFeedback`: the shaking of a pad, in bursts with rests between them.
/// Times are in thousandths of a second, as the original counts them.
#[derive(Default, Debug, PartialEq)]
struct Rumble {
    total: u32,
    phase: u32,
    on: u32,
    off: u32,
    /// The rolling resistance of the ground under the car.
    surface: f32,
    /// Nought is still, one shaking, two resting between bursts.
    state: u8,
    /// The shaking is the ground's, and goes on for as long as the ground does.
    of_surface: bool,
}

/// The strength of `DirectInputDevice::CreateForceFeedbackEffect`'s wave, of full.
const RUMBLE_STRENGTH: f32 = 0.3;
/// A burst of the ground's is this long for each unit of its rolling resistance at
/// this speed or more, in game units a millisecond; the rests are this long.
const SURFACE_BURST: f32 = 20.0;
const SURFACE_FULL_SPEED: f32 = 0.2;
const SURFACE_REST: u32 = 50;
/// `CarVisuals`: a car going faster than the first that is at once slower than the
/// second has run into something.
const CRASH_SPEEDS: (f32, f32) = (0.05, 0.01);

impl Rumble {
    /// Steps the bursts on. `Some(true)` is the motors starting, for the time the
    /// burst is to last, and `Some(false)` their stopping.
    fn update(&mut self, elapsed: u32, speed: f32) -> Option<bool> {
        if self.state == 0 {
            return None;
        }
        if self.of_surface {
            self.surface_pulse(speed);
        }
        if elapsed >= self.total {
            self.total = 0;
            return self.stop();
        }
        self.total -= elapsed;
        self.phase = self.phase.saturating_sub(elapsed);
        if self.phase != 0 {
            return None;
        }
        if self.state == 2 && self.on != 0 {
            (self.state, self.phase) = (1, self.on);
            Some(true)
        } else {
            (self.state, self.phase) = (2, self.off);
            Some(false)
        }
    }

    fn stop(&mut self) -> Option<bool> {
        if self.state == 0 {
            return None;
        }
        (self.total, self.phase, self.on, self.off, self.state) = (0, 0, 0, 0, 0);
        if self.surface != 0.0 {
            self.start_surface();
        }
        Some(false)
    }

    fn start_pulses(&mut self, total: u32, on: u32, off: u32) {
        (self.total, self.on, self.off) = (total, on, off);
        (self.state, self.of_surface) = (2, false);
    }

    fn start_surface(&mut self) {
        self.of_surface = true;
        self.total = u32::MAX;
        self.on = (self.surface * SURFACE_BURST) as u32;
        self.off = SURFACE_REST;
        (self.phase, self.state) = (0, 2);
    }

    fn surface_pulse(&mut self, speed: f32) {
        let part = speed.abs().min(SURFACE_FULL_SPEED) / SURFACE_FULL_SPEED;
        let part = Some(part * part).filter(|part| *part >= 0.02).unwrap_or(0.0);
        self.on = (part * SURFACE_BURST * self.surface) as u32;
    }

    fn set_surface(&mut self, resistance: f32) -> Option<bool> {
        self.surface = resistance;
        if resistance == 0.0 && self.of_surface {
            self.of_surface = false;
            return self.stop();
        }
        if self.state == 0 && resistance != 0.0 {
            self.start_surface();
        }
        None
    }

    /// A turbo of each level, and the warp that is the fourth.
    fn turbo(&mut self, level: u8) {
        let (total, on, off) = match level {
            0 => (1000, 500, 0),
            1 => (1500, 750, 0),
            2 => (5000, 500, 100),
            _ => (1000, 1000, 0),
        };
        self.start_pulses(total, on, off);
    }

    fn reaction(&mut self) {
        self.start_pulses(500, 500, 0);
    }

    /// Another car's touch.
    fn light(&mut self) {
        self.start_pulses(150, 150, 0);
    }

    /// A wall's, which gives way to anything else.
    fn scrape(&mut self) {
        if self.state == 0 {
            self.start_pulses(100, 100, 0);
        }
    }
}

/// What the car was doing a frame ago, to tell what has just happened to it.
#[derive(Default)]
struct Before {
    boost: f32,
    warping: bool,
    thrown: bool,
    scrape: f32,
    speed: f32,
    still: bool,
}

/// Shakes the pad as the original shakes a joystick: for a turbo, a warp, being
/// hit, running into something, another car's touch, a wall's, and rough ground. A
/// hit is whatever sets the car whirling or throws it in the air.
fn rumble(
    time: Res<Time>,
    race: Res<Race>,
    pause: Res<crate::Pause>,
    karts: Query<&Kart, With<Player>>,
    pads: Query<Entity, With<Gamepad>>,
    mut requests: MessageWriter<GamepadRumbleRequest>,
    mut rumble: Local<Rumble>,
    mut before: Local<Before>,
) {
    let driving = pause.0.is_none() && !race.demo && race.phase == Phase::Racing;
    let kart = karts.single().ok().filter(|kart| kart.finished.is_none());
    let mut sent: Option<bool> = None;
    let (Some(kart), true) = (kart, driving) else {
        // `RaceForceFeedback::Pause`.
        if !std::mem::replace(&mut before.still, true) {
            *rumble = Rumble::default();
            for gamepad in &pads {
                requests.write(GamepadRumbleRequest::Stop { gamepad });
            }
        }
        return;
    };
    before.still = false;
    let speed = kart.vel.length() / UNIT / 1000.0;
    if kart.boost > before.boost {
        rumble.turbo(kart.boost_level.min(2));
    }
    let warping = kart.warp > 0.0;
    if warping && !before.warping {
        rumble.turbo(3);
    }
    let thrown = kart.spin > 0.0 || kart.spin_out > 0.0;
    if (thrown && !before.thrown) || (before.speed > CRASH_SPEEDS.0 && speed < CRASH_SPEEDS.1) {
        rumble.reaction();
    }
    if kart.scrape_cooldown > before.scrape {
        rumble.light();
    }
    if kart.wall_contact {
        rumble.scrape();
    }
    let ground = if kart.contacts > 0 && !kart.hover {
        kart.surface.rolling_resistance
    } else {
        0.0
    };
    sent = rumble.set_surface(ground).or(sent);
    *before = Before {
        boost: kart.boost,
        warping,
        thrown,
        scrape: kart.scrape_cooldown,
        speed,
        still: false,
    };
    let elapsed = (time.delta_secs() * 1000.0) as u32;
    sent = rumble.update(elapsed, speed).or(sent);
    for gamepad in &pads {
        match sent {
            Some(true) => {
                requests.write(GamepadRumbleRequest::Add {
                    gamepad,
                    duration: Duration::from_millis(rumble.phase as u64),
                    intensity: GamepadRumbleIntensity {
                        strong_motor: RUMBLE_STRENGTH,
                        weak_motor: RUMBLE_STRENGTH,
                    },
                });
            }
            Some(false) => {
                requests.write(GamepadRumbleRequest::Stop { gamepad });
            }
            None => {}
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Actions>()
        .init_resource::<Devices>()
        .add_systems(
            PreUpdate,
            (read, pad_keys).chain().after(bevy::input::InputSystems),
        )
        .add_systems(Update, rumble.run_if(in_state(Screen::Race)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_bound_again_is_taken_from_what_had_it() {
        let mut bindings = Bindings::default();
        // The letters' accelerator, given to the arrows' brake.
        bindings.set(1, Event::Brake as usize, Bound::Key(KeyCode::KeyW));
        assert_eq!(bindings.0[1][Event::Brake as usize], Bound::Key(KeyCode::KeyW));
        // The letters' accelerator has nothing to go back to, its own key being taken.
        assert_eq!(bindings.0[2][Event::Accelerate as usize], Bound::None);
        // The down arrow is free again, and nothing was bound to nothing but that.
        assert!(!bindings.bound(1, Bound::Key(KeyCode::ArrowDown)));
        // Bound somewhere else, the accelerator's own key goes back to it.
        bindings.set(1, Event::Brake as usize, Bound::Key(KeyCode::ArrowDown));
        assert_eq!(bindings, Bindings::default());
    }

    #[test]
    fn a_button_is_only_the_pads_and_some_keys_are_nobodys() {
        assert!(Bindings::allowed(PAD, Bound::Button(GamepadButton::South)));
        assert!(!Bindings::allowed(1, Bound::Button(GamepadButton::South)));
        assert!(Bindings::allowed(PAD, Bound::Key(KeyCode::KeyJ)));
        assert!(!Bindings::allowed(1, Bound::Key(KeyCode::Escape)));
        let mut bindings = Bindings::default();
        // A button swapped within the pad's set leaves the other with its own back.
        bindings.set(PAD, Event::Slide as usize, Bound::Button(GamepadButton::South));
        assert_eq!(bindings.0[PAD][Event::Powerup as usize], Bound::None);
        bindings.set(PAD, Event::Powerup as usize, Bound::Button(GamepadButton::West));
        assert_eq!(
            bindings.0[PAD][Event::Powerup as usize],
            Bound::Button(GamepadButton::West)
        );
    }

    #[test]
    fn bindings_come_back_as_they_were_written() {
        let mut bindings = Bindings::default();
        bindings.set(PAD, Event::Camera as usize, Bound::Key(KeyCode::Numpad5));
        bindings.set(2, Event::Powerup as usize, Bound::Key(KeyCode::Enter));
        let mut read = Bindings::default();
        for line in bindings.write().lines() {
            assert!(read.read(line));
        }
        read.mend();
        assert_eq!(read, bindings);
        // A line of something else is not theirs, and nonsense binds nothing twice.
        assert!(!read.read("music=3"));
        assert!(read.read("controls1=KeyW,KeyW,Nonsense,Escape,pad:South"));
        read.mend();
        let keys = read.0.iter().flatten().filter(|b| **b == Bound::Key(KeyCode::KeyW));
        assert_eq!(keys.count(), 1);
        // The letters' accelerator lost its key to the arrows' set, and has no other.
        assert_eq!(read.0[2][Event::Accelerate as usize], Bound::None);
        assert!(read.0[1].iter().all(|bound| *bound != Bound::None));
    }

    #[test]
    fn bindings_are_named_in_capitals() {
        assert_eq!(name(Bound::Key(KeyCode::KeyW)), "W");
        assert_eq!(name(Bound::Key(KeyCode::ArrowLeft)), "LEFT");
        assert_eq!(name(Bound::Key(KeyCode::ShiftRight)), "RIGHT SHIFT");
        assert_eq!(name(Bound::Key(KeyCode::NumpadEnter)), "NUMPAD ENTER");
        assert_eq!(name(Bound::Key(KeyCode::Digit4)), "4");
        assert_eq!(name(Bound::Button(GamepadButton::RightTrigger2)), "RT");
        assert_eq!(name(Bound::Button(GamepadButton::Start)), "START");
    }

    #[test]
    fn a_stick_is_dead_about_the_middle() {
        assert_eq!(past_dead_zone(0.3), 0.0);
        assert_eq!(past_dead_zone(-1.0), -1.0);
        assert!((past_dead_zone(0.675) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_pad_has_only_the_pause_menu_while_its_car_is_driven() {
        let south = Source::Button(GamepadButton::South);
        let start = Source::Button(GamepadButton::Start);
        assert_eq!(key_for(south, true, true), None);
        assert_eq!(key_for(Source::Stick(2), true, true), None);
        assert_eq!(key_for(start, true, true), Some(KeyCode::Escape));
        assert_eq!(key_for(south, false, true), Some(KeyCode::Enter));
        assert_eq!(key_for(start, false, false), Some(KeyCode::Enter));
        assert_eq!(key_for(Source::Stick(2), false, false), Some(KeyCode::ArrowLeft));
    }

    #[test]
    fn a_long_turbo_shakes_in_bursts_and_the_ground_takes_over_after() {
        let mut rumble = Rumble::default();
        rumble.turbo(2);
        // Half a second on, a tenth off, for five seconds.
        assert_eq!(rumble.update(16, 0.1), Some(true));
        assert_eq!(rumble.update(400, 0.1), None);
        assert_eq!(rumble.update(100, 0.1), Some(false));
        assert_eq!(rumble.update(100, 0.1), Some(true));
        // A wall's scrape doesn't break in on it; another car's touch does.
        rumble.scrape();
        assert_eq!((rumble.total, rumble.on), (5000 - 616, 500));
        rumble.light();
        assert_eq!(rumble.update(16, 0.1), None);
        assert_eq!(rumble.update(200, 0.1), Some(false));
        assert_eq!(rumble, Rumble::default());
        // Rough ground shakes for as long as it is driven over, harder the faster.
        assert_eq!(rumble.set_surface(2.0), None);
        assert_eq!(rumble.update(16, 0.2), Some(true));
        assert_eq!(rumble.phase, 40);
        assert_eq!(rumble.update(40, 0.2), Some(false));
        assert_eq!(rumble.update(50, 0.0), Some(false));
        assert_eq!(rumble.set_surface(0.0), Some(false));
        assert_eq!(rumble.update(16, 0.2), None);
    }
}
