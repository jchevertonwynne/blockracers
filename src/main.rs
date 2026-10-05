mod assets;
mod audio;
mod camera;
mod championship;
mod collision;
mod events;
mod frontend;
mod gauntlet;
mod hazards;
mod hud;
mod item_models;
mod items;
mod kart;
mod kart_effects;
mod menu;
mod meshgen;
mod mixer;
mod net;
mod opponent;
mod particles;
mod physics;
mod racer_sounds;
mod replay;
mod roster;
mod rules;
mod scenery;
mod sky;
mod time_race;
mod track;
mod variant;
mod video;
mod world;

use bevy::{
    core_pipeline::tonemapping::Tonemapping,
    light::CascadeShadowConfigBuilder,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use items::Action;
use kart::{Kart, Player};
use menu::{Circuits, Screen, Settings};
use meshgen::Rng;
use track::Track;
use world::LoadedWorld;

const SKY: Color = Color::srgb(0.25, 0.55, 0.95);
const COUNTDOWN: f32 = 3.0;
/// The camera sweeps in over the grid for this long, to the starting jingle.
const INTRO: f32 = 2.0;
/// How long the rest of the field is given to come in once the player has
/// (`RaceSession::UpdateFinishedState`).
const FINISH_WAIT: f32 = 10.0;
/// How long a circuit's standings are then shown (`RaceSession::UpdateResultsState`).
const STANDINGS_WAIT: f32 = 5.0;

/// Ends the race for cars still out on the circuit: they take the places left in the
/// order of the grid, as `RaceSession::UpdateFinishedState` gives them, and the race's
/// points are handed out.
fn settle(
    karts: &mut Query<(&mut Kart, Has<Player>)>,
    time: f32,
    championship: &mut championship::Championship,
) {
    let mut waiting: Vec<Mut<Kart>> = karts
        .iter_mut()
        .map(|(k, _)| k)
        .filter(|k| k.finished.is_none() && k.out.is_none())
        .collect();
    waiting.sort_by_key(|k| k.slot);
    for (i, k) in waiting.iter_mut().enumerate() {
        k.finished = Some(time + i as f32 * 1e-3);
    }
    let mut order: Vec<(bool, f32, usize)> = karts
        .iter()
        .map(|(k, _)| (k.out.is_some(), k.finished.unwrap_or(f32::MAX), k.slot))
        .collect();
    order.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let places: Vec<(usize, usize)> = order
        .iter()
        .enumerate()
        .map(|(i, &(_, _, slot))| (slot, i + 1))
        .collect();
    championship.score(&places);
}

#[derive(Clone, Copy, PartialEq)]
pub enum Phase {
    Intro,
    Countdown,
    Racing,
    Finished,
}

#[derive(Resource)]
pub struct Race {
    pub phase: Phase,
    /// Seconds of the intro still to run.
    pub intro: f32,
    pub countdown: f32,
    /// Seconds since the lights went out.
    pub time: f32,
    /// Attract mode: the AI drives the player's kart too.
    pub demo: bool,
    /// Go straight to the racing, with no drop-in or countdown.
    pub quick: bool,
}

/// The menu a race is paused with, as the original's `RaceDialog`: a question and
/// answers to pick from. All are strings of the game's.
#[derive(Resource, Default)]
pub struct Pause(pub Option<Dialog>);

pub struct Dialog {
    pub prompt: usize,
    pub options: Vec<usize>,
    pub selected: usize,
    /// What saying yes will do, when this is the "are you sure?" that follows a choice.
    pending: Option<Pending>,
}

#[derive(Clone, Copy)]
enum Pending {
    Restart,
    Exit,
}

impl Dialog {
    const CONTINUE: usize = 14;
    const RESTART: usize = 15;
    const EXIT: usize = 17;
    const YES: usize = 18;
    const PAUSED: usize = 20;
    const SURE: usize = 44;

    fn paused() -> Self {
        Dialog {
            prompt: Self::PAUSED,
            options: vec![Self::CONTINUE, Self::RESTART, Self::EXIT],
            selected: 0,
            pending: None,
        }
    }

    /// Starts on "no".
    fn sure(pending: Pending) -> Self {
        Dialog {
            prompt: Self::SURE,
            options: vec![Self::YES, Self::YES + 1],
            selected: 1,
            pending: Some(pending),
        }
    }
}

/// `BRICK_DEMO=<seconds>:<png path>` goes straight into a race in attract mode, saves a
/// screenshot and quits. Add `:menu` to photograph the menu instead, or `:cycle` to
/// leave the race for the menu and come back before the screenshot.
#[derive(Resource)]
struct DemoShot {
    at: f32,
    path: String,
    cycle: bool,
    /// `BRICK_CAM=x,y,z,tx,ty,tz` (the game's coordinates): look from one place at another.
    camera: Option<(Vec3, Vec3)>,
    /// `BRICK_EVENTS=18@3,12@5.5`: circuit events to set off, and when.
    events: Vec<(i32, f32)>,
    /// `BRICK_KEYS=Enter@1.5,Down@2,W@3+0.5`: keys to press, when, and for how long
    /// they are held (a frame, where it isn't said).
    keys: Vec<(KeyCode, f32, f32)>,
    /// `BRICK_VIEW=back,up,right`: where the camera sits relative to the player's kart,
    /// which it looks at.
    view: Option<Vec3>,
    /// `BRICK_POWER=green2@4,red0@6`: power-ups (brick colour and level) the player
    /// fires, and when.
    powers: Vec<(items::Power, u8, f32)>,
}

fn main() {
    let demo = std::env::var("BRICK_DEMO").ok().and_then(|v| {
        let mut parts = v.split(':');
        let (at, path) = (parts.next()?.parse().ok()?, parts.next()?.to_string());
        let mode = parts.next();
        let numbers = |name: &str| -> Vec<f32> {
            let value = std::env::var(name).unwrap_or_default();
            value
                .split([',', '@'])
                .filter_map(|n| n.parse().ok())
                .collect()
        };
        let camera = match numbers("BRICK_CAM")[..] {
            [x, y, z, tx, ty, tz] => Some((Vec3::new(x, y, z), Vec3::new(tx, ty, tz))),
            _ => None,
        };
        let view = match numbers("BRICK_VIEW")[..] {
            [back, up, right] => Some(Vec3::new(back, up, right)),
            _ => None,
        };
        let events = numbers("BRICK_EVENTS")
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| (pair[0] as i32, pair[1]))
            .collect();
        let keys = std::env::var("BRICK_KEYS").unwrap_or_default();
        let keys = keys.split(',').filter_map(|press| {
            let (key, at) = press.split_once('@')?;
            let key = match key {
                "Enter" => KeyCode::Enter,
                "Esc" => KeyCode::Escape,
                "Up" => KeyCode::ArrowUp,
                "Down" => KeyCode::ArrowDown,
                "Escape" => KeyCode::Escape,
                "Tab" => KeyCode::Tab,
                "Left" => KeyCode::ArrowLeft,
                "Right" => KeyCode::ArrowRight,
                "P" => KeyCode::KeyP,
                "R" => KeyCode::KeyR,
                "W" => KeyCode::KeyW,
                "E" => KeyCode::KeyE,
                "T" => KeyCode::KeyT,
                _ => return None,
            };
            let (at, held) = at.split_once('+').unwrap_or((at, "0"));
            Some((key, at.parse().ok()?, held.parse().ok()?))
        });
        let keys = keys.collect();
        let powers = std::env::var("BRICK_POWER").unwrap_or_default();
        let powers = powers
            .split(',')
            .filter_map(|power| {
                let (what, at) = power.split_once('@')?;
                let colour = match what.trim_end_matches(char::is_numeric) {
                    "red" => items::Power::Red,
                    "yellow" => items::Power::Yellow,
                    "blue" => items::Power::Blue,
                    "green" => items::Power::Green,
                    _ => return None,
                };
                Some((
                    colour,
                    what.chars().last()?.to_digit(10)? as u8,
                    at.parse().ok()?,
                ))
            })
            .collect();
        Some((
            DemoShot {
                at,
                path,
                cycle: mode == Some("cycle"),
                camera,
                view,
                events,
                keys,
                powers,
            },
            mode == Some("menu"),
        ))
    });
    let circuits = Circuits::find();
    let mut settings = Settings::new(&circuits);
    // A demo is set up by its variables alone, and leaves the kept settings as they are.
    if demo.is_none() {
        settings.restore();
    }
    let keeping = demo.is_none();
    // `BRICK_LAPS=1`: how long a demo's race is.
    let laps = std::env::var("BRICK_LAPS")
        .ok()
        .and_then(|laps| laps.parse::<i32>().ok());
    if let Some(choice) = laps.and_then(|laps| menu::LAP_CHOICES.iter().position(|&l| l == laps)) {
        settings.lap_choice = choice;
    }

    // Demos are for looking at, and run silent unless `BRICK_SOUND` asks to hear them.
    if demo.is_some() && std::env::var("BRICK_SOUND").is_err() {
        (settings.music, settings.sound) = (0, 0);
    }
    // `BRICK_MIRROR`, `BRICK_REVERSE`, `BRICK_ELIMINATION` and `BRICK_BRICKS=red` (or
    // yellow, blue, green, none, random) turn on the port's own ways of racing.
    let set = |name: &str| std::env::var(name).is_ok();
    (settings.mirror, settings.reverse, settings.elimination) = (
        set("BRICK_MIRROR"),
        set("BRICK_REVERSE"),
        set("BRICK_ELIMINATION"),
    );
    if let Ok(colour) = std::env::var("BRICK_BRICKS") {
        settings.bricks = ["normal", "red", "yellow", "blue", "green", "none", "random"]
            .iter()
            .position(|&c| c == colour)
            .unwrap_or(0);
    }
    // `BRICK_OPPONENTS=1`: how many of the computer's cars a demo races.
    if let Some(opponents) = std::env::var("BRICK_OPPONENTS")
        .ok()
        .and_then(|n| n.parse::<usize>().ok())
    {
        settings.opponents = opponents.min(menu::MAX_OPPONENTS);
    }
    // `BRICK_TIME=1`: a demo's race is against the clock.
    settings.time_race = std::env::var("BRICK_TIME").is_ok();
    // `BRICK_SERIES=0`: a demo races this circuit's races rather than one on its own.
    let mut championship = championship::Championship::load();
    if let Some(series) = std::env::var("BRICK_SERIES")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        (championship.chosen, championship.unlocked) = (series, championship.series.len());
        if let Some((code, folder)) = championship.begin() {
            settings.championship = Some(code);
            settings.circuit = circuits
                .0
                .iter()
                .position(|c| c.race.as_deref() == Some(folder.as_str()))
                .unwrap_or(0);
        }
    }

    // `BRICK_NET` hosts or joins a session without the menus' help.
    let auto = net::Auto::from_env();
    let mut app = App::new();
    if let Some(auto) = net::Auto::from_env() {
        app.insert_resource(auto);
    }
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Brick Racers".into(),
            ..default()
        }),
        ..default()
    }));
    match demo {
        Some((shot, on_menu)) => {
            app.insert_resource(shot).add_systems(
                Update,
                demo_shot.after(kart::ai_drive).before(items::use_items),
            );
            app.add_systems(PreUpdate, demo_keys.after(bevy::input::InputSystems));
            // A demo online waits at the menu for its session's race to begin.
            app.insert_state(if on_menu || auto.is_some() {
                Screen::Menu
            } else {
                Screen::Race
            });
        }
        None => {
            app.init_state::<Screen>();
        }
    }
    app.insert_resource(ClearColor(SKY))
        .insert_resource(GlobalAmbientLight {
            brightness: 500.0,
            ..default()
        })
        .insert_resource(circuits)
        .insert_resource(settings)
        .init_resource::<camera::Rig>()
        .init_resource::<Pause>()
        .init_resource::<variant::Variant>()
        .init_resource::<replay::Replay>()
        .init_resource::<replay::Photo>()
        // Online the settings are the session's, and not the player's to be kept.
        .add_systems(
            Update,
            (
                video::apply,
                menu::keep.run_if(move || keeping).run_if(not(net::online)),
            ),
        )
        .init_resource::<time_race::TimeRace>()
        .insert_resource(championship)
        .add_systems(
            OnEnter(Screen::Loading),
            |mut next: ResMut<NextState<Screen>>| next.set(Screen::Race),
        )
        .add_plugins((menu::plugin, frontend::plugin, audio::plugin, net::plugin))
        .add_systems(
            Update,
            leave_online
                .run_if(in_state(Screen::Race))
                .run_if(net::online),
        )
        .add_systems(Startup, setup_scene)
        .add_systems(
            OnEnter(Screen::Race),
            (
                load_race,
                net::enter_race,
                world::spawn_world.run_if(resource_exists::<LoadedWorld>),
                scenery::spawn_scenery.run_if(resource_exists::<LoadedWorld>),
                sky::spawn.run_if(resource_exists::<LoadedWorld>),
                setup_brick_world.run_if(resource_exists::<BrickWorld>),
                kart::spawn_karts,
                time_race::spawn_ghosts,
                items::setup_items,
                hud::original::load,
                hud::setup_text_hud.run_if(not(resource_exists::<hud::original::Art>)),
                net::client::loaded.run_if(net::joined),
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                // Online the host's `net::host::flow` runs the race, and the race is
                // stepped by `net::plugin` at its own rate.
                race_flow.run_if(not(net::online)),
                // A replay shows the race again rather than running it on.
                (
                    kart::player_input,
                    kart::ai_drive,
                    items::use_items,
                    items::actions,
                    kart::kart_physics,
                    kart::kart_collisions,
                    (
                        kart::update_places,
                        rules::elimination,
                        time_race::time_race,
                    )
                        .chain(),
                    items::pickups,
                    replay::record,
                )
                    .chain()
                    .run_if(replay::live)
                    .run_if(not(net::online)),
                replay::play,
                racer_sounds::racer_sounds,
                events::track_events,
                events::part_animations,
                events::effects,
                (hazards::hazards, hazards::code_lights, sky::change).chain(),
                (
                    item_models::dress_actions,
                    item_models::dress_karts,
                    scenery::animate,
                    scenery::cycle,
                    scenery::scroll,
                    scenery::fade,
                    kart_effects::kart_effects,
                    kart_effects::tints,
                    kart_effects::shadows,
                    particles::emit,
                )
                    .chain(),
                kart::sync_karts,
                kart::sync_wheels,
                (
                    chase_camera.run_if(not(replay::shooting)),
                    replay::photo.run_if(not(net::online)),
                    sky::follow,
                    particles::particles,
                )
                    .chain(),
                hud::update_text_hud.run_if(not(resource_exists::<hud::original::Art>)),
                hud::original::draw
                    .run_if(resource_exists::<hud::original::Art>)
                    .run_if(not(replay::shooting)),
                tag_race_entities,
            )
                .chain()
                .run_if(in_state(Screen::Race)),
        )
        .run();
}

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        // Far enough to take in the sky, which is drawn large and a long way off.
        Projection::Perspective(PerspectiveProjection {
            far: 5000.0,
            ..default()
        }),
        Transform::from_xyz(0.0, 10.0, 20.0),
        // The original's colours are baked in and meant to reach the screen as they are.
        Tonemapping::None,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 11_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(60.0, 100.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: 160.0,
            ..default()
        }
        .build(),
    ));
}

/// Loads the chosen circuit and resets the race.
fn load_race(
    mut commands: Commands,
    circuits: Res<Circuits>,
    settings: Res<Settings>,
    demo: Option<Res<DemoShot>>,
    championship: Res<championship::Championship>,
    mut rig: ResMut<camera::Rig>,
    role: Res<net::Role>,
    lineup: Option<Res<net::Lineup>>,
) {
    let circuit = &circuits.0[settings.circuit];
    let variant = variant::Variant::of(&settings, &championship, circuit.race.as_deref());
    commands.insert_resource(variant);
    // Everything loaded and placed from here on is the mirror's side of the circuit.
    scenery::set_mirror(variant.mirror);
    commands.insert_resource(replay::Replay::default());
    let mut events = circuit.race.as_deref().and_then(events::load);
    let mut hazards = circuit.race.as_deref().and_then(hazards::load);
    commands.remove_resource::<gauntlet::Stands>();
    match circuit
        .race
        .as_deref()
        .and_then(|race| world::load_in(race, settings.championship.as_deref(), settings.time_race))
    {
        Some((mut track, mut loaded)) => {
            if variant.reverse {
                track.reverse();
            }
            // Online, players race as whoever they chose to.
            for (slot, code) in lineup
                .iter()
                .filter(|_| *role != net::Role::Offline)
                .flat_map(|lineup| lineup.cast())
            {
                if !world::recast(&mut loaded, slot, code) {
                    warn!("nobody to race as {code}");
                }
            }
            commands.insert_resource(track);
            commands.insert_resource(loaded);
            commands.remove_resource::<BrickWorld>();
        }
        None => {
            if circuit.race.is_some() {
                warn!("could not load {}; using the brick circuit", circuit.name);
            }
            let mut track = Track::built(circuit.layout);
            // The gauntlet has hazards of the game's to stand round its road, where
            // the game's data is there to take them from.
            let gauntlet = circuit.race.is_none() && circuit.layout == track::Layout::Gauntlet;
            let furnished = if gauntlet {
                gauntlet::load(&mut track, settings.championship.as_deref())
            } else {
                None
            };
            if variant.reverse {
                track.reverse();
            }
            commands.insert_resource(track);
            commands.insert_resource(BrickWorld);
            commands.insert_resource(ClearColor(SKY));
            match furnished {
                Some((loaded, its_events, its_hazards, stands)) => {
                    (events, hazards) = (Some(its_events), Some(its_hazards));
                    commands.insert_resource(loaded);
                    commands.insert_resource(stands);
                }
                None => commands.remove_resource::<LoadedWorld>(),
            }
        }
    }
    match events {
        Some(events) => commands.insert_resource(events),
        None => commands.remove_resource::<events::TrackEvents>(),
    }
    match hazards {
        Some(hazards) => commands.insert_resource(hazards),
        None => commands.remove_resource::<hazards::Hazards>(),
    }
    commands.insert_resource(Rng(0x1EC0_1999));
    rig.reset();
    commands.insert_resource(Race {
        phase: Phase::Intro,
        intro: INTRO,
        countdown: COUNTDOWN,
        time: 0.0,
        // Online the cars are driven by whoever is at them, a demo's too.
        demo: demo.is_some() && *role == net::Role::Offline,
        quick: demo.is_some() && std::env::var("BRICK_START").is_err(),
    });
}

/// Everything a race puts in the world goes when the race does.
fn tag_race_entities(
    mut commands: Commands,
    new: Query<
        Entity,
        (
            Or<(
                Added<Mesh3d>,
                Added<Node>,
                Added<Kart>,
                Added<scenery::Prop>,
                Added<scenery::Placed>,
                Added<particles::Emitter>,
                Added<item_models::Dressing>,
                Added<items::Action>,
            )>,
            Without<ChildOf>,
        ),
    >,
) {
    for entity in &new {
        commands.entity(entity).insert(DespawnOnExit(Screen::Race));
    }
}

/// The race is on a road built of bricks here, not on one of the game's.
#[derive(Resource)]
struct BrickWorld;

/// A built-in circuit: one of our own, or the stand-in for one that didn't load.
fn setup_brick_world(
    mut commands: Commands,
    track: Res<Track>,
    stands: Option<Res<gauntlet::Stands>>,
    circuits: Res<Circuits>,
    settings: Res<Settings>,
    mut rng: ResMut<Rng>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Everything static is vertex-coloured and shares one plastic material.
    let plastic = materials.add(StandardMaterial {
        perceptual_roughness: 0.5,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(track.build_mesh())),
        MeshMaterial3d(plastic.clone()),
    ));
    commands.spawn((
        Mesh3d(meshes.add(track.build_scenery(circuits.0[settings.circuit].layout, &mut rng))),
        MeshMaterial3d(plastic.clone()),
    ));
    if let Some(stands) = stands {
        commands.spawn((
            Mesh3d(meshes.add(stands.0.clone())),
            MeshMaterial3d(plastic),
        ));
    }
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(3000.0, 3000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: meshgen::GREEN,
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.08, 0.0),
    ));
}

/// Presses the keys a demo asks for, each for one frame.
fn demo_keys(
    time: Res<Time<Real>>,
    demo: Res<DemoShot>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut pressed: Local<usize>,
) {
    for &(key, at, held) in demo.keys.iter().take(*pressed) {
        if time.elapsed_secs() >= at + held {
            keys.release(key);
        }
    }
    if let Some(&(key, ..)) = demo
        .keys
        .get(*pressed)
        .filter(|k| time.elapsed_secs() >= k.1)
    {
        keys.press(key);
        *pressed += 1;
    }
}

fn demo_shot(
    mut commands: Commands,
    time: Res<Time<Real>>,
    demo: Res<DemoShot>,
    mut exit: MessageWriter<AppExit>,
    mut next: ResMut<NextState<Screen>>,
    mut taken: Local<bool>,
    mut cycled: Local<u8>,
    mut fired: Local<usize>,
    mut used: Local<usize>,
    events: Option<ResMut<events::TrackEvents>>,
    mut sfx: ResMut<audio::Sfx>,
    mut player: Query<(&mut Kart, &mut kart::Controls), With<Player>>,
    mut pending: ResMut<net::Pending>,
) {
    if let Some(&(power, level, _)) = demo
        .powers
        .get(*used)
        .filter(|p| time.elapsed_secs() >= p.2)
        && let Ok((mut kart, mut controls)) = player.single_mut()
    {
        (kart.held, kart.whites, controls.use_item) = (Some(power), level, true);
        // Online the press is taken by the next step of the race.
        pending.use_item();
        *used += 1;
    }
    if let Some(mut events) = events {
        while let Some(&(event, _)) = demo
            .events
            .get(*fired)
            .filter(|e| time.elapsed_secs() >= e.1)
        {
            events.fire(event, None, &mut sfx);
            *fired += 1;
        }
    }
    if demo.cycle {
        // Out to the menu a third of the way in, back to the race at two thirds.
        let stage = (time.elapsed_secs() / demo.at * 3.0) as u8;
        if stage > *cycled && stage < 3 {
            *cycled = stage;
            next.set(if stage == 1 {
                Screen::Menu
            } else {
                Screen::Race
            });
        }
    }
    if time.elapsed_secs() > demo.at && !*taken {
        *taken = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(demo.path.clone()));
    }
    if time.elapsed_secs() > demo.at + 1.0 {
        exit.write(AppExit::Success);
    }
}

fn race_flow(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    track: Res<Track>,
    mut race: ResMut<Race>,
    mut next: ResMut<NextState<Screen>>,
    mut sfx: ResMut<audio::Sfx>,
    mut karts: Query<(&mut Kart, Has<Player>)>,
    debris: Query<Entity, With<Action>>,
    mut pause: ResMut<Pause>,
    mut clock: ResMut<Time<Virtual>>,
    art: Option<Res<hud::original::Art>>,
    mut championship: ResMut<championship::Championship>,
    circuits: Res<Circuits>,
    mut settings: ResMut<Settings>,
    (mut replay, photo): (ResMut<replay::Replay>, Res<replay::Photo>),
) {
    // In photo mode the keys are the camera's.
    if photo.0.is_some() {
        return;
    }
    let mut restart = false;
    if let Some(dialog) = &mut pause.0 {
        // Paused: the keys work the menu and nothing else moves.
        let count = dialog.options.len();
        let step = keys.just_pressed(KeyCode::ArrowDown) as usize
            + keys.just_pressed(KeyCode::ArrowUp) as usize * (count - 1);
        if step > 0 {
            dialog.selected = (dialog.selected + step) % count;
            sfx.play(audio::id::MENU_HIGHLIGHT);
        }
        let chosen = if keys.just_pressed(KeyCode::Escape) {
            // Backing out is "continue", or "no".
            Some(if dialog.pending.is_some() { 1 } else { 0 })
        } else {
            keys.just_pressed(KeyCode::Enter).then_some(dialog.selected)
        };
        let Some(chosen) = chosen else { return };
        debug!("chose {chosen}");
        sfx.play(audio::id::MENU_SELECT);
        pause.0 = match (dialog.pending, chosen) {
            (None, 0) => None,
            (None, 1) => Some(Dialog::sure(Pending::Restart)),
            (None, _) => Some(Dialog::sure(Pending::Exit)),
            (Some(Pending::Exit), 0) => {
                clock.unpause();
                pause.0 = None;
                // Leaving gives up any circuit being raced.
                (championship.run, settings.championship) = (None, None);
                next.set(Screen::Menu);
                return;
            }
            (Some(Pending::Restart), 0) => {
                restart = true;
                None
            }
            (Some(_), _) => Some(Dialog::paused()),
        };
        if pause.0.is_none() {
            clock.unpause();
        }
        if !restart {
            return;
        }
    } else if replay.showing.is_some() {
        // Any of the keys that began or would leave the replay ends it.
        if keys.any_just_pressed([KeyCode::Escape, KeyCode::Enter, KeyCode::KeyR]) {
            sfx.play(audio::id::MENU_BACK);
            replay.showing = None;
        }
        return;
    } else if race.phase == Phase::Finished && keys.just_pressed(KeyCode::KeyR) && replay.ready() {
        sfx.play(audio::id::MENU_CONFIRM);
        replay.start();
        return;
    } else if keys.just_pressed(KeyCode::Escape) {
        sfx.play(audio::id::MENU_BACK);
        // Without the game's own lettering there is no menu to show: just leave.
        if art.is_none() || race.phase == Phase::Finished {
            (championship.run, settings.championship) = (None, None);
            next.set(Screen::Menu);
        } else {
            pause.0 = Some(Dialog::paused());
            clock.pause();
            debug!("paused");
        }
        return;
    }
    // `RaceSession::UpdateFinishedState` and `UpdateResultsState`: the race is left by
    // itself, ten seconds after the player finishes, or after five more with a
    // circuit's standings to show. A time race waits to be asked, and so does a demo.
    let since_finish = karts
        .iter()
        .find(|(_, player)| *player)
        .and_then(|(k, _)| k.finished)
        .map(|at| race.time - at);
    let wait = if championship.run.is_some() {
        FINISH_WAIT + STANDINGS_WAIT
    } else {
        FINISH_WAIT
    };
    let over = race.phase == Phase::Finished
        && !race.demo
        && !settings.time_race
        && !restart
        && since_finish.is_some_and(|since| since >= wait);
    // After a race of a circuit comes the next, or the way out.
    if race.phase == Phase::Finished
        && (keys.just_pressed(KeyCode::Enter) || over)
        && championship.run.is_some()
        && !restart
    {
        settle(&mut karts, race.time, &mut championship);
        let player = karts.iter().find(|k| k.1).map_or(0, |k| k.0.slot);
        sfx.play(audio::id::MENU_CONFIRM);
        match championship.advance(player) {
            Some(folder) => {
                settings.circuit = circuits
                    .0
                    .iter()
                    .position(|c| c.race.as_deref() == Some(folder.as_str()))
                    .unwrap_or(settings.circuit);
                next.set(Screen::Loading);
            }
            None => {
                settings.championship = None;
                next.set(Screen::Menu);
            }
        }
        return;
    }
    if over {
        next.set(Screen::Menu);
        return;
    }
    if restart || (race.phase == Phase::Finished && keys.just_pressed(KeyCode::Enter)) {
        for (mut k, _) in &mut karts {
            k.reset(&track);
        }
        for e in &debris {
            commands.entity(e).despawn();
        }
        race.phase = Phase::Intro;
        race.intro = INTRO;
        race.countdown = COUNTDOWN;
        return;
    }
    match race.phase {
        Phase::Intro => {
            race.intro -= time.delta_secs();
            if race.intro <= 0.0 || race.quick {
                race.phase = Phase::Countdown;
            }
        }
        Phase::Countdown => {
            race.countdown -= time.delta_secs();
            if race.countdown <= 0.0 || race.quick {
                race.phase = Phase::Racing;
                race.time = 0.0;
            }
        }
        Phase::Racing => {
            race.time += time.delta_secs();
            if karts
                .iter()
                .any(|(k, player)| player && k.finished.is_some())
            {
                race.phase = Phase::Finished;
            }
        }
        // The clock runs on for the cars still racing, to time them as they come in;
        // after a while those still out are placed as they stand on the grid.
        Phase::Finished => {
            race.time += time.delta_secs();
            let finished = karts
                .iter()
                .find(|(_, player)| *player)
                .and_then(|(k, _)| k.finished);
            let waiting = karts
                .iter()
                .any(|(k, _)| k.finished.is_none() && k.out.is_none());
            if finished.is_some_and(|at| race.time - at >= FINISH_WAIT)
                && (waiting || championship.run.as_ref().is_some_and(|run| !run.scored))
            {
                settle(&mut karts, race.time, &mut championship);
            }
        }
    }
}

/// Online, Escape asks whether to leave, as the original asks before giving up a
/// race; the race goes on behind the question, since others are in it. A player who
/// leaves is back in the session's room, and the computer drives their car for the
/// rest of the race. The host leaving a race calls it off for everyone, and they are
/// all back in the room.
fn leave_online(
    keys: Res<ButtonInput<KeyCode>>,
    art: Option<Res<hud::original::Art>>,
    mut pause: ResMut<Pause>,
    mut sfx: ResMut<audio::Sfx>,
    mut session: ResMut<net::Session>,
    mut next: ResMut<NextState<Screen>>,
    role: Res<net::Role>,
    mut wire: ResMut<net::Wire>,
) {
    let mut leave = |next: &mut NextState<Screen>| match *role {
        net::Role::Host => net::call_off(&mut session, &mut wire, next),
        _ => net::retire(&mut wire, next),
    };
    let Some(dialog) = &mut pause.0 else {
        if keys.just_pressed(KeyCode::Escape) {
            sfx.play(audio::id::MENU_BACK);
            // Without the game's own lettering there is no question to show: just leave.
            match art {
                Some(_) => pause.0 = Some(Dialog::sure(Pending::Exit)),
                None => leave(&mut next),
            }
        }
        return;
    };
    let count = dialog.options.len();
    let step = keys.just_pressed(KeyCode::ArrowDown) as usize
        + keys.just_pressed(KeyCode::ArrowUp) as usize * (count - 1);
    if step > 0 {
        dialog.selected = (dialog.selected + step) % count;
        sfx.play(audio::id::MENU_HIGHLIGHT);
    }
    // Backing out is "no".
    let chosen = if keys.just_pressed(KeyCode::Escape) {
        Some(1)
    } else {
        keys.just_pressed(KeyCode::Enter).then_some(dialog.selected)
    };
    if let Some(chosen) = chosen {
        sfx.play(audio::id::MENU_SELECT);
        pause.0 = None;
        if chosen == 0 {
            leave(&mut next);
        }
    }
}

fn chase_camera(
    time: Res<Time>,
    race: Res<Race>,
    keys: Res<ButtonInput<KeyCode>>,
    demo: Option<Res<DemoShot>>,
    karts: Query<(&Kart, Has<Player>)>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
    mut rig: ResMut<camera::Rig>,
    track: Res<Track>,
    mut replay: ResMut<replay::Replay>,
    watching: Res<net::Watching>,
    mut watched: Local<Option<usize>>,
) {
    let (mut t, mut projection) = camera.into_inner();
    let Some((own, _)) = karts.iter().find(|k| k.1) else {
        return;
    };
    // A replay may be watched over any car's shoulder, or from beside the road.
    let mut player = own;
    // Online, a player with no car to drive follows whichever they choose.
    if let Some((other, _)) = watching
        .slot
        .and_then(|slot| karts.iter().find(|k| k.0.slot == slot))
    {
        player = other;
    }
    if *watched != watching.slot {
        *watched = watching.slot;
        rig.reset();
    }
    if replay.showing.is_some() {
        let step = keys.just_pressed(KeyCode::ArrowRight) as isize
            - keys.just_pressed(KeyCode::ArrowLeft) as isize;
        if step != 0 {
            replay.watch_next(own.slot, step);
            rig.reset();
        }
        if keys.just_pressed(KeyCode::KeyT) {
            replay.trackside = !replay.trackside;
            rig.reset();
        }
        player = replay
            .subject
            .and_then(|slot| karts.iter().find(|k| k.0.slot == slot))
            .map_or(own, |k| k.0);
        if replay.trackside {
            t.translation = replay::Replay::station(&track, player);
            t.look_at(player.pos + Vec3::Y * 0.8, Vec3::Y);
            if let Projection::Perspective(lens) = &mut *projection {
                lens.fov = replay::STATION_FOV;
            }
            return;
        }
    }
    if let Some((from, to)) = demo.as_ref().and_then(|d| d.camera) {
        t.translation = scenery::to_world(from);
        t.look_at(scenery::to_world(to), Vec3::Y);
        return;
    }
    if let Some(view) = demo.and_then(|d| d.view) {
        t.translation = player.pos + player.rot * Vec3::new(view.z, view.y, view.x);
        t.look_at(player.pos + Vec3::Y * 0.5, Vec3::Y);
        return;
    }
    // C goes round the views; V looks behind for as long as it is held.
    if keys.just_pressed(KeyCode::KeyC) {
        rig.view = (rig.view + 1) % 4;
    }
    rig.look_back = keys.pressed(KeyCode::KeyV);
    // Its race run, the player's car is watched from in front.
    rig.finished(
        player.finished.is_some()
            && player.warp <= 0.0
            && player.warp_start <= 0.0
            && replay.showing.is_none()
            && !race.demo,
    );
    // Down a warp's tunnel the camera is held still behind the car.
    let (position, rotation) = if player.warp > 0.0 {
        rig.fixed(player)
    } else {
        rig.follow(player, time.delta_secs())
    };

    // During the intro the camera drops in from high behind the grid.
    let sweep = if race.phase == Phase::Intro {
        (race.intro / INTRO).clamp(0.0, 1.0).powi(2)
    } else {
        0.0
    };
    let back = rotation * Vec3::Z;
    t.translation = position + (back * 14.0 + Vec3::Y * 10.0) * sweep;
    if player.warp > 0.0 {
        t.translation += kart::TUNNEL;
    }
    t.rotation = rotation;

    if let Projection::Perspective(p) = &mut *projection {
        if player.warp > 0.0 {
            // Down the tunnel the view opens right out and closes again.
            let through = 1.0 - player.warp / items::WARP_TIME;
            p.fov = (62.0 + 45.0 * (std::f32::consts::PI * through).sin()).to_radians();
        } else {
            // A turbo widens it, and a warp opening widens it more.
            let fov = if player.warp_start > 0.0 {
                92.0f32
            } else if player.boost > 0.0 {
                78.0
            } else {
                62.0
            };
            p.fov += (fov.to_radians() - p.fov) * (1.0 - (-5.0 * time.delta_secs()).exp());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::state::app::StatesPlugin;

    /// A model placed in a race, as a brick's is, is gone once the race is left.
    #[test]
    fn placed_models_leave_with_the_race() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(Screen::Race)
            .add_systems(Update, tag_race_entities.run_if(in_state(Screen::Race)));
        let brick = app
            .world_mut()
            .spawn((scenery::Placed, Transform::default()))
            .id();
        app.update();
        app.world_mut()
            .resource_mut::<NextState<Screen>>()
            .set(Screen::Loading);
        app.update();
        assert!(app.world().get_entity(brick).is_err());
    }
}
