//! Sound: the original game's music and effects, when its data is present. Without it
//! the game is silent.
//!
//! Game code asks for sounds through [`Sfx`]; this module turns those requests into
//! mixer voices and places them around the player the way the original's sound
//! manager does (`SpatialSoundInstance::UpdateSpatialFromNode`).

use crate::assets::{Jam, sound};
use crate::kart::{Kart, Player};
use crate::menu::{Circuits, Screen, Settings};
use crate::mixer::{Clip, Mixer, MixerOutput, Tone};
use crate::physics::UNIT;
use crate::{Phase, Race};
use bevy::audio::AddAudioSource;
use bevy::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Distances, in the original's units, inside which a sound is at full volume and
/// beyond which it is silent. Most sounds use the first; impacts carry further.
pub const NEAR: (f32, f32) = (30.0, 300.0);
pub const FAR: (f32, f32) = (200.0, 600.0);
const SPEED_OF_SOUND: f32 = 343.0;
/// Sounds dead to one side are panned this far.
const PAN_SCALE: f32 = 0.7;
/// A spatial sound's priority is its volume times this; at zero it is dropped.
const PRIORITY_SCALE: f32 = 2048.0;

/// The original game's sound ids, named for how its code uses them. Below 1000 they
/// are positions in the general bank (`GENERAL.SBK`); 1000 and up are the racers'
/// voices, twelve per racer; 3000 and up are the circuit's own bank.
pub mod id {
    pub const COUNTDOWN: usize = 0x00; // 321
    pub const SHIELD_HITS: [usize; 3] = [0x01, 0x46, 0x47]; // block, block1, block2
    pub const BRAKE: usize = 0x02; // brake1
    pub const BRAKE_LOOP: usize = 0x03; // brake2
    pub const LAND_FOUR_WHEELS: usize = 0x04; // btmout
    pub const EXPLOSION: usize = 0x05; // canhit: cannon ball and dynamite
    pub const CANNON_FIRE: usize = 0x06; // cansht
    pub const CANNON_FLIGHT: usize = 0x07; // cansus
    pub const CURSED_LOOP: usize = 0x08; // cursehit: follows a cursed racer
    pub const CURSE_LOOP: usize = 0x09; // cursesus: a curse lying in wait
    pub const ENGINE: usize = 0x0a; // engine
    pub const WHITE_BRICK: usize = 0x0b; // enhan01..03, by bricks already held
    pub const BRICK_RESPAWN: usize = 0x0e; // form
    pub const GO: usize = 0x0f; // go
    pub const DYNAMITE_FUSE: usize = 0x11; // gpwdrsus
    pub const HOOK_HIT: usize = 0x12; // grphit
    pub const HOOK_PULL: usize = 0x13; // grppull
    pub const HOOK_RELEASE: usize = 0x14; // grprel
    pub const HOOK_FIRE: usize = 0x15; // grpsht
    pub const HOOK_MISS: usize = 0x16; // grpsnap
    pub const HOOK_FLIGHT: usize = 0x17; // grpsus
    pub const CAR_HITS: [usize; 2] = [0x18, 0x37]; // hitcar, skrcar
    pub const WALL_HITS: [usize; 2] = [0x38, 0x19]; // skrenv, hitenv
    pub const PLAYER_HORN: usize = 0x1a; // horn01
    /// horn02..06, by the racer's place in the circuit's voice bank.
    pub const HORNS: [usize; 6] = [0x1c, 0x1b, 0x1d, 0x1e, 0x1f, 0x1f];
    pub const ENGINE_IDLE: usize = 0x20; // idle
    pub const MAGNET_DROP: usize = 0x21; // mdrop
    pub const MAGNET_LOOP: usize = 0x22; // mmine
    pub const MAGNET_GRAB: usize = 0x23; // mtrap
    pub const TURBO_START: usize = 0x24; // octact0..2, by level
    pub const WARP_START: usize = 0x27; // octact3
    pub const TURBO_END: usize = 0x28; // octexp0
    pub const WARP_END: usize = 0x29; // octexp3
    pub const TURBO_LOOP: usize = 0x2a; // octsus0..2, by level
    pub const WARP_LOOP: usize = 0x2d; // octsus3
    pub const OIL_DROP: usize = 0x2e; // oildrp
    pub const OIL_SLIP: usize = 0x2f; // oilhit
    pub const OIL_LOOP: usize = 0x30; // oilsus
    pub const BRICK_COLLECT: usize = 0x31; // power
    pub const MISSILE_EXPLODE: usize = 0x32; // rockhit
    pub const MISSILE_FIRE: usize = 0x33; // rocksht
    pub const MISSILE_FLIGHT: usize = 0x34; // rocksus
    pub const SHIELDS: [usize; 4] = [0x4c, 0x35, 0x4d, 0x4e]; // shield0..3
    pub const DRIFT_START: usize = 0x39; // slide1
    pub const SKID: usize = 0x3a; // slide2
    pub const SHIELD_EXPIRE: usize = 0x3b; // soff
    pub const SPIN: usize = 0x3c; // spin
    pub const ENGINE_COAST: usize = 0x3d; // sput
    pub const ENGINE_START: usize = 0x3e; // start
    pub const LAND_ONE_WHEEL: usize = 0x3f; // tire1
    pub const LAND_SOME_WHEELS: usize = 0x40; // tire2
    pub const WHOOSH: usize = 0x41; // turbo: turbo and warp launch, end of a drift
    pub const LIGHTNING_END: usize = 0x42; // wndexp
    pub const LIGHTNING_ZAP: usize = 0x43; // wndhit
    pub const LIGHTNING_LOOP: usize = 0x44; // wndsus1
    pub const LIGHTNING_CRACKLE: usize = 0x45; // wndsus2
    pub const BRICK_SWAP: usize = 0x48; // enhbonus
    pub const HOOK_RETRACT: usize = 0x49; // grphtenv
    pub const MAGNET_RELEASE: usize = 0x4a; // mrel
    pub const TURBO_END_LONG: usize = 0x4b; // octexp2
    pub const OTHER_ENGINE: usize = 0x4f; // eengine

    /// Each racer has six unhappy remarks, then six happy ones.
    pub const VOICES: usize = 1000;
    pub const VOICES_EACH: usize = 12;
    pub const AMBIENT: usize = 3000;

    // The front end's bank (`GENC0R0.SBK`), which the original numbers from zero too.
    pub const MENU: usize = 5000;
    pub const MENU_BACK: usize = MENU + 1; // backup
    pub const MENU_REFUSE: usize = MENU + 7; // cantdo2
    pub const MENU_CONFIRM: usize = MENU + 8; // confirm
    pub const MENU_HIGHLIGHT: usize = MENU + 13; // hilight1
    pub const MENU_SELECT: usize = MENU + 22; // setselct
    pub const MENU_SLIDER: usize = MENU + 23; // slider2
}

/// Where a sound comes from and how far it carries.
#[derive(Clone, Copy)]
pub struct Emitter {
    pub pos: Vec3,
    pub vel: Vec3,
    /// Full-volume and silent distances, in the original's units.
    pub range: (f32, f32),
    pub volume: f32,
    pub pitch: f32,
}

impl Emitter {
    pub fn at(pos: Vec3) -> Self {
        Emitter { pos, vel: Vec3::ZERO, range: NEAR, volume: 1.0, pitch: 1.0 }
    }

    pub fn range(self, min: f32, max: f32) -> Self {
        Emitter { range: (min, max), ..self }
    }

    pub fn far(self) -> Self {
        Emitter { range: FAR, ..self }
    }

    pub fn moving(self, vel: Vec3) -> Self {
        Emitter { vel, ..self }
    }

    pub fn volume(self, volume: f32) -> Self {
        Emitter { volume, ..self }
    }

    pub fn pitch(self, pitch: f32) -> Self {
        Emitter { pitch, ..self }
    }
}

/// Looping sounds are named by who makes them and which of their sounds it is.
type LoopKey = (u64, u16);

/// Owner of loops that belong to the race rather than to anything in it.
pub const RACE: u64 = u64::MAX;

enum Shot {
    Flat(usize),
    Placed(usize, Emitter),
}

/// Where game code asks for sounds. One-shots are played once; loops sound for as
/// long as they are asked for every frame, and start afresh after a gap.
#[derive(Resource)]
pub struct Sfx {
    shots: Vec<Shot>,
    loops: Vec<(LoopKey, usize, Emitter)>,
    /// Loops of which only the candidate nearest the player sounds: slot, sound,
    /// emitter and how near (in the original's units) it has to be.
    nearest: Vec<(u16, usize, Emitter, f32)>,
    seed: u32,
}

impl Default for Sfx {
    fn default() -> Self {
        Sfx { shots: Vec::new(), loops: Vec::new(), nearest: Vec::new(), seed: 0x2545_f491 }
    }
}

impl Sfx {
    /// Plays a sound that isn't anywhere in particular.
    pub fn play(&mut self, sound: usize) {
        self.shots.push(Shot::Flat(sound));
    }

    /// Plays a sound that happens somewhere on the track.
    pub fn play_at(&mut self, sound: usize, at: Vec3) {
        self.emit(sound, Emitter::at(at));
    }

    pub fn emit(&mut self, sound: usize, emitter: Emitter) {
        self.shots.push(Shot::Placed(sound, emitter));
    }

    pub fn sustain(&mut self, owner: Entity, slot: u16, sound: usize, emitter: Emitter) {
        self.loops.push(((owner.to_bits(), slot), sound, emitter));
    }

    /// A loop that belongs to the race as a whole.
    pub fn sustain_global(&mut self, slot: u16, sound: usize, emitter: Emitter) {
        self.loops.push(((RACE, slot), sound, emitter));
    }

    /// Offers a source for a loop that follows whichever source is nearest the player.
    pub fn sustain_nearest(&mut self, slot: u16, sound: usize, emitter: Emitter, within: f32) {
        self.nearest.push((slot, sound, emitter, within));
    }

    /// A random number below `n`, for the choices the original makes with its random table.
    pub fn roll(&mut self, n: u32) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed % n.max(1)
    }
}

type Bank = Vec<Option<Arc<Clip>>>;

#[derive(Resource, Default)]
struct Library {
    general: Bank,
    /// Twelve per racer, in roster order.
    voices: Bank,
    /// The circuit's own sounds.
    ambient: Bank,
    menu: Bank,
    /// Folder holding the `.tun` music files.
    music_dir: Option<PathBuf>,
}

impl Library {
    /// `RacerSoundSource::ResolveSoundId`, with the front end's bank on the end.
    fn clip(&self, sound: usize) -> Option<&Arc<Clip>> {
        let (bank, index) = match sound {
            id::MENU.. => (&self.menu, sound - id::MENU),
            id::AMBIENT.. => (&self.ambient, sound - id::AMBIENT),
            id::VOICES.. => (&self.voices, sound - id::VOICES),
            _ => (&self.general, sound),
        };
        bank.get(index)?.as_ref()
    }
}

/// What is sounding now.
#[derive(Resource, Default)]
struct Playing {
    loops: HashMap<LoopKey, (u32, usize)>,
    /// Placed one-shots, which are kept in place as the player moves.
    shots: Vec<(u32, Emitter)>,
    music: Option<u32>,
    /// The race phase and countdown second last given their cues.
    cued: Option<(Phase, i32)>,
}

/// The racer the player hears through (`RaceCameraController::UpdateListener`).
struct Listener {
    pos: Vec3,
    vel: Vec3,
    left: Vec3,
}

pub fn plugin(app: &mut App) {
    app.add_audio_source::<MixerOutput>()
        .init_resource::<Sfx>()
        .init_resource::<Mixer>()
        .init_resource::<Library>()
        .init_resource::<Playing>()
        // Before the first screen is entered, which wants its music.
        .add_systems(PreStartup, (load_banks, start_mixer))
        .add_systems(OnEnter(Screen::Menu), menu_music)
        .add_systems(OnEnter(Screen::Race), load_circuit_bank)
        .add_systems(Update, music_volume)
        .add_systems(Update, race_cues.run_if(in_state(Screen::Race)))
        .add_systems(PostUpdate, flush);
}

fn jam_path() -> PathBuf {
    std::env::var("LEGO_JAM").unwrap_or("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM".into()).into()
}

fn clip(data: &[u8], default_rate: u32) -> Arc<Clip> {
    let sound = sound::decode(data, default_rate);
    Arc::new(Clip { samples: sound.samples, channels: sound.channels, rate: sound.rate })
}

/// The file names listed in a sound bank: a folder, a count, then one name per line.
fn bank_names(jam: &Jam, path: &str) -> Vec<String> {
    let Some(bank) = jam.get(path) else { return Vec::new() };
    let text = String::from_utf8_lossy(bank);
    text.lines().map(str::trim).filter(|l| l.to_lowercase().ends_with(".pcm")).map(String::from).collect()
}

/// Loads a bank whose sounds are in `dir`. Missing files keep their place in the numbering.
fn load_bank(jam: &Jam, path: &str, dir: &str) -> Bank {
    let sound = |name: &String| jam.get(&format!("{dir}/{name}")).map(|data| clip(data, 11025));
    bank_names(jam, path).iter().map(sound).collect()
}

/// Each racer's twelve remarks, found in whichever voice bank lists them, and the
/// racer's place in that bank (which settles their horn).
fn load_voices(jam: &Jam) -> (Bank, Vec<usize>) {
    const DIR: &str = "/GAMEDATA/VOICES";
    let mut banks: Vec<&str> = jam.list(DIR).filter(|f| f.ends_with(".SBK")).collect();
    // The circuits' banks first: they hold six racers each.
    banks.sort_by_key(|f| (!f.contains("VOICEC"), f.to_string()));
    let banks: Vec<Vec<String>> = banks.iter().map(|path| bank_names(jam, path)).collect();
    let (mut voices, mut places) = (Bank::new(), Vec::new());
    for prefix in crate::world::KART_PREFIXES {
        let own = |name: &String| name.to_lowercase().starts_with(&format!("{prefix}_"));
        let found = banks.iter().find_map(|names| Some((names, names.iter().position(own)?)));
        let (names, start) = found.map_or((&[][..], 0), |(names, start)| (&names[..], start));
        for n in 0..id::VOICES_EACH {
            let data = names.get(start + n).filter(|name| own(name)).and_then(|name| jam.get(&format!("{DIR}/{name}")));
            voices.push(data.map(|data| clip(data, 11025)));
        }
        places.push(start / id::VOICES_EACH);
    }
    (voices, places)
}

/// Each racer's place in its voice bank, in roster order.
#[derive(Resource, Default)]
pub struct VoicePlaces(pub Vec<usize>);

fn load_banks(mut commands: Commands, mut library: ResMut<Library>) {
    let Some(jam) = Jam::open(jam_path()) else { return };
    library.music_dir = jam_path().parent().map(PathBuf::from);
    library.general = load_bank(&jam, "/GAMEDATA/COMMON/GENERAL.SBK", "/GAMEDATA/COMMON");
    library.menu = load_bank(&jam, "/MENUDATA/GENC0R0.SBK", "/MENUDATA/SOUNDS");
    let (voices, places) = load_voices(&jam);
    library.voices = voices;
    commands.insert_resource(VoicePlaces(places));
}

fn load_circuit_bank(
    mut library: ResMut<Library>,
    mut playing: ResMut<Playing>,
    circuits: Res<Circuits>,
    settings: Res<Settings>,
) {
    playing.cued = None;
    library.ambient = Bank::new();
    let Some(race) = circuits.0[settings.circuit].race.as_deref() else { return };
    let Some(jam) = Jam::open(jam_path()) else { return };
    let dir = format!("/GAMEDATA/{race}");
    if let Some(bank) = jam.list(&dir).find(|f| f.ends_with(".SBK")) {
        library.ambient = load_bank(&jam, bank, &dir);
    }
}

fn start_mixer(mut commands: Commands, mixer: Res<Mixer>, mut outputs: ResMut<Assets<MixerOutput>>) {
    commands.spawn(AudioPlayer(outputs.add(MixerOutput(mixer.clone()))));
}

/// Replaces whatever music is playing with `tune` (a `.tun` file name).
fn play_music(mixer: &Mixer, playing: &mut Playing, library: &Library, settings: &Settings, tune: &str, looped: bool) {
    if let Some(voice) = playing.music.take() {
        mixer.stop(voice);
    }
    let Some(data) = library.music_dir.as_ref().and_then(|dir| std::fs::read(dir.join(tune)).ok()) else {
        return;
    };
    let tone = Tone { volume: settings.music_volume(), ..default() };
    playing.music = Some(mixer.play(&clip(&data, 22050), looped, tone));
}

fn menu_music(mixer: Res<Mixer>, mut playing: ResMut<Playing>, library: Res<Library>, settings: Res<Settings>) {
    play_music(&mixer, &mut playing, &library, &settings, "theme.tun", true);
}

/// Applies the music volume setting as it is changed.
fn music_volume(settings: Res<Settings>, mixer: Res<Mixer>, playing: Res<Playing>) {
    if let (true, Some(voice)) = (settings.is_changed(), playing.music) {
        mixer.set(voice, Tone { volume: settings.music_volume(), ..default() });
    }
}

/// The circuit's music list (`LEGOMSC`): start jingle, the circuit's own tune, the
/// losing and winning jingles, then more tunes.
fn tunes(circuits: &Circuits, settings: &Settings) -> Vec<String> {
    let race = circuits.0[settings.circuit].race.as_deref();
    let list = race.and_then(|race| {
        let jam = Jam::open(jam_path())?;
        Some(String::from_utf8_lossy(jam.get(&format!("/GAMEDATA/{race}/LEGOMSC"))?).into_owned())
    });
    match list {
        Some(list) => list.lines().map(str::trim).filter(|l| l.ends_with(".tun")).map(String::from).collect(),
        None => ["start.tun", "circuit1.tun", "lose.tun", "win.tun"].map(String::from).to_vec(),
    }
}

/// Countdown, start and finish, as `RaceHud` and `RaceSession` sound them.
fn race_cues(
    race: Res<Race>,
    circuits: Res<Circuits>,
    settings: Res<Settings>,
    library: Res<Library>,
    mixer: Res<Mixer>,
    mut playing: ResMut<Playing>,
    player: Single<&Kart, With<Player>>,
    mut sfx: ResMut<Sfx>,
) {
    let second = race.countdown.ceil() as i32;
    let before = playing.cued.replace((race.phase, second));
    let entered = before.map(|b| b.0) != Some(race.phase);
    let mut music = |index: usize, looped: bool| {
        let tune = tunes(&circuits, &settings).get(index).cloned().unwrap_or_default();
        play_music(&mixer, &mut playing, &library, &settings, &tune, looped);
    };
    match race.phase {
        Phase::Intro if entered => music(0, false),
        // A beep for each of three, two and one.
        Phase::Countdown if entered || before.map(|b| b.1) != Some(second) => sfx.play(id::COUNTDOWN),
        Phase::Racing if entered => {
            sfx.play(id::GO);
            // A single race picks one of the circuit's tunes, if it has a choice.
            let count = tunes(&circuits, &settings).len();
            let choice = if count > 4 { sfx.roll(count as u32 - 3) } else { 0 };
            music([1, 4, 5, 6].get(choice as usize).copied().unwrap_or(1), true);
        }
        Phase::Finished if entered => music(if player.place == 1 { 3 } else { 2 }, false),
        _ => {}
    }
}

/// `SpatialSoundInstance::UpdateSpatialFromNode`: volume falls off with the square of
/// the distance between the two ranges, pan follows how far to one side the sound
/// is, and pitch shifts with the speed the two are closing at.
fn place(emitter: &Emitter, listener: Option<&Listener>, scale: f32) -> Tone {
    let volume = emitter.volume * scale;
    let Some(listener) = listener else { return Tone { volume, pan: 0.0, pitch: emitter.pitch } };
    let offset = (emitter.pos - listener.pos) / UNIT;
    let distance_squared = offset.length_squared();
    let (min, max) = (emitter.range.0 * emitter.range.0, emitter.range.1 * emitter.range.1);
    let volume = if distance_squared <= min {
        volume
    } else if distance_squared >= max {
        0.0
    } else {
        (1.0 - (distance_squared - min) / (max - min)) * volume
    };
    if volume == 0.0 || distance_squared == 0.0 {
        return Tone { volume, pan: 0.0, pitch: emitter.pitch };
    }
    let side = offset.dot(listener.left);
    let pan = -side.signum() * PAN_SCALE * side * side / distance_squared;
    // Speeds are in the original's units per millisecond, as it feeds them in.
    let towards = -offset / distance_squared.sqrt();
    let closing = ((emitter.vel - listener.vel) / UNIT / 1000.0).dot(towards).min(SPEED_OF_SOUND * 0.5);
    Tone { volume, pan, pitch: emitter.pitch * SPEED_OF_SOUND / (SPEED_OF_SOUND - closing) }
}

fn audible(tone: &Tone) -> bool {
    (tone.volume * PRIORITY_SCALE) as i32 > 0
}

fn flush(
    mut sfx: ResMut<Sfx>,
    mut playing: ResMut<Playing>,
    mixer: Res<Mixer>,
    library: Res<Library>,
    settings: Res<Settings>,
    screen: Res<State<Screen>>,
    player: Query<&Kart, With<Player>>,
) {
    let racing = *screen.get() == Screen::Race;
    let listener = player.single().ok().filter(|_| racing).map(|k| Listener {
        pos: k.pos,
        vel: k.vel,
        left: k.rot * Vec3::NEG_X,
    });
    let listener = listener.as_ref();
    let scale = settings.sound_volume();

    for shot in sfx.shots.drain(..) {
        match shot {
            Shot::Flat(sound) => {
                if let Some(clip) = library.clip(sound) {
                    mixer.play(clip, false, Tone { volume: scale, ..default() });
                }
            }
            Shot::Placed(sound, emitter) => {
                let tone = place(&emitter, listener, scale);
                if let (true, Some(clip)) = (audible(&tone), library.clip(sound)) {
                    playing.shots.push((mixer.play(clip, false, tone), emitter));
                }
            }
        }
    }
    // Sounds already under way stay where they were made as the player moves on;
    // any that fall out of earshot are cut short.
    playing.shots.retain(|(voice, emitter)| {
        let tone = place(emitter, listener, scale);
        if audible(&tone) {
            mixer.set(*voice, tone);
        } else {
            mixer.stop(*voice);
        }
        mixer.playing(*voice)
    });

    let mut wanted = std::mem::take(&mut sfx.loops);
    let mut slots: Vec<u16> = sfx.nearest.iter().map(|n| n.0).collect();
    slots.sort();
    slots.dedup();
    for slot in slots {
        let distance = |emitter: &Emitter| listener.map_or(0.0, |l| l.pos.distance(emitter.pos) / UNIT);
        let candidates = sfx.nearest.iter().filter(|n| n.0 == slot && distance(&n.2) < n.3);
        if let Some(&(_, sound, emitter, _)) = candidates.min_by(|a, b| distance(&a.2).total_cmp(&distance(&b.2))) {
            wanted.push(((RACE, slot), sound, emitter));
        }
    }
    sfx.nearest.clear();
    if !racing {
        // Nothing from the track follows the player out to the menu.
        wanted.clear();
        for (voice, _) in playing.shots.drain(..) {
            mixer.stop(voice);
        }
    }
    playing.loops.retain(|key, (voice, sound)| {
        let keep = wanted.iter().any(|w| w.0 == *key && w.1 == *sound);
        if !keep {
            mixer.stop(*voice);
        }
        keep
    });
    for (key, sound, emitter) in wanted {
        let tone = place(&emitter, listener, scale);
        match playing.loops.get(&key) {
            Some((voice, _)) => mixer.set(*voice, tone),
            None => {
                if let Some(clip) = library.clip(sound) {
                    playing.loops.insert(key, (mixer.play(clip, true, tone), sound));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_ids_line_up_with_the_sound_bank() {
        let Some(jam) = Jam::open(jam_path()) else { return };
        let names = bank_names(&jam, "/GAMEDATA/COMMON/GENERAL.SBK");
        for (sound, file) in [
            (id::COUNTDOWN, "321.pcm"),
            (id::BRAKE_LOOP, "brake2.pcm"),
            (id::EXPLOSION, "canhit.pcm"),
            (id::CANNON_FLIGHT, "cansus.pcm"),
            (id::ENGINE, "engine.pcm"),
            (id::WHITE_BRICK + 2, "enhan03.pcm"),
            (id::BRICK_RESPAWN, "form.pcm"),
            (id::GO, "go.pcm"),
            (id::HOOK_PULL, "grppull.pcm"),
            (id::HOOK_FLIGHT, "grpsus.pcm"),
            (id::PLAYER_HORN, "horn01.pcm"),
            (id::HORNS[5], "horn06.pcm"),
            (id::ENGINE_IDLE, "idle.pcm"),
            (id::MAGNET_LOOP, "mmine.pcm"),
            (id::TURBO_START + 2, "octact2.pcm"),
            (id::TURBO_LOOP + 2, "octsus2.pcm"),
            (id::WARP_LOOP, "octsus3.pcm"),
            (id::OIL_LOOP, "oilsus.pcm"),
            (id::BRICK_COLLECT, "power.pcm"),
            (id::MISSILE_FLIGHT, "rocksus.pcm"),
            (id::SKID, "slide2.pcm"),
            (id::SPIN, "spin.pcm"),
            (id::ENGINE_COAST, "sput.pcm"),
            (id::LIGHTNING_LOOP, "wndsus1.pcm"),
            (id::LIGHTNING_CRACKLE, "wndsus2.pcm"),
            (id::SHIELDS[0], "shield0.pcm"),
            (id::SHIELDS[3], "shield3.pcm"),
            (id::SHIELD_HITS[2], "block2.pcm"),
            (id::MAGNET_RELEASE, "mrel.pcm"),
            (id::OTHER_ENGINE, "eengine.pcm"),
        ] {
            assert_eq!(names[sound], file, "sound {sound:#x}");
        }
        assert!(load_bank(&jam, "/GAMEDATA/COMMON/GENERAL.SBK", "/GAMEDATA/COMMON").iter().all(Option::is_some));

        let menu = bank_names(&jam, "/MENUDATA/GENC0R0.SBK");
        assert_eq!(menu[id::MENU_HIGHLIGHT - id::MENU], "hilight1.pcm");
        assert_eq!(menu[id::MENU_SLIDER - id::MENU], "slider2.pcm");
        assert!(load_bank(&jam, "/MENUDATA/GENC0R0.SBK", "/MENUDATA/SOUNDS").iter().all(Option::is_some));
    }

    #[test]
    fn every_racer_has_a_voice() {
        let Some(jam) = Jam::open(jam_path()) else { return };
        let (voices, places) = load_voices(&jam);
        assert_eq!(voices.len(), crate::world::KART_PREFIXES.len() * id::VOICES_EACH);
        let missing: Vec<usize> = voices.iter().enumerate().filter(|v| v.1.is_none()).map(|v| v.0).collect();
        assert!(missing.is_empty(), "no voice for {missing:?}");
        assert!(places.iter().all(|&p| p < 6), "{places:?}");
    }

    #[test]
    fn sounds_fade_with_distance_and_pan_to_their_side() {
        let listener = Listener { pos: Vec3::ZERO, vel: Vec3::ZERO, left: Vec3::NEG_X };
        let hear = |x: f32, z: f32| place(&Emitter::at(Vec3::new(x, 0.0, z) * UNIT), Some(&listener), 1.0);
        assert_eq!(hear(0.0, -20.0).volume, 1.0);
        assert_eq!(hear(0.0, -300.0).volume, 0.0);
        let halfway = hear(0.0, -200.0).volume;
        assert!((halfway - (1.0 - (40000.0 - 900.0) / (90000.0 - 900.0))).abs() < 1e-5);
        // Dead ahead is centred; off to the right pans right, and to the left, left.
        assert_eq!(hear(0.0, -50.0).pan, 0.0);
        assert!((hear(50.0, 0.0).pan - 0.7).abs() < 1e-5);
        assert!((hear(-50.0, 0.0).pan + 0.7).abs() < 1e-5);
        assert!(!audible(&hear(0.0, -299.99)));
    }
}
