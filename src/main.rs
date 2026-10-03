mod assets;
mod audio;
mod collision;
mod events;
mod frontend;
mod hazards;
mod hud;
mod item_models;
mod items;
mod kart;
mod kart_effects;
mod menu;
mod meshgen;
mod mixer;
mod particles;
mod physics;
mod racer_sounds;
mod scenery;
mod track;
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
        Dialog { prompt: Self::PAUSED, options: vec![Self::CONTINUE, Self::RESTART, Self::EXIT], selected: 0, pending: None }
    }

    /// Starts on "no".
    fn sure(pending: Pending) -> Self {
        Dialog { prompt: Self::SURE, options: vec![Self::YES, Self::YES + 1], selected: 1, pending: Some(pending) }
    }
}

/// The chase camera's heading, which trails the kart's.
#[derive(Resource, Default)]
struct ChaseYaw(Option<f32>);

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
    /// `BRICK_KEYS=Enter@1.5,Down@2`: keys to press, and when.
    keys: Vec<(KeyCode, f32)>,
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
            value.split([',', '@']).filter_map(|n| n.parse().ok()).collect()
        };
        let camera = match numbers("BRICK_CAM")[..] {
            [x, y, z, tx, ty, tz] => Some((Vec3::new(x, y, z), Vec3::new(tx, ty, tz))),
            _ => None,
        };
        let view = match numbers("BRICK_VIEW")[..] {
            [back, up, right] => Some(Vec3::new(back, up, right)),
            _ => None,
        };
        let events = numbers("BRICK_EVENTS").chunks_exact(2).map(|pair| (pair[0] as i32, pair[1])).collect();
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
                _ => return None,
            };
            Some((key, at.parse().ok()?))
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
                Some((colour, what.chars().last()?.to_digit(10)? as u8, at.parse().ok()?))
            })
            .collect();
        Some((DemoShot { at, path, cycle: mode == Some("cycle"), camera, view, events, keys, powers }, mode == Some("menu")))
    });
    let circuits = Circuits::find();
    let mut settings = Settings::new(&circuits);
    // `BRICK_LAPS=1`: how long a demo's race is.
    let laps = std::env::var("BRICK_LAPS").ok().and_then(|laps| laps.parse::<i32>().ok());
    if let Some(choice) = laps.and_then(|laps| menu::LAP_CHOICES.iter().position(|&l| l == laps)) {
        settings.lap_choice = choice;
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Brick Racers".into(), ..default() }),
        ..default()
    }));
    match demo {
        Some((shot, on_menu)) => {
            app.insert_resource(shot).add_systems(Update, demo_shot.after(kart::ai_drive).before(items::use_items));
            app.add_systems(PreUpdate, demo_keys.after(bevy::input::InputSystems));
            app.insert_state(if on_menu { Screen::Menu } else { Screen::Race });
        }
        None => {
            app.init_state::<Screen>();
        }
    }
    app.insert_resource(ClearColor(SKY))
        .insert_resource(GlobalAmbientLight { brightness: 500.0, ..default() })
        .insert_resource(circuits)
        .insert_resource(settings)
        .init_resource::<ChaseYaw>()
        .init_resource::<Pause>()
        .add_plugins((menu::plugin, frontend::plugin, audio::plugin))
        .add_systems(Startup, setup_scene)
        .add_systems(
            OnEnter(Screen::Race),
            (
                load_race,
                world::spawn_world.run_if(resource_exists::<LoadedWorld>),
                scenery::spawn_scenery.run_if(resource_exists::<LoadedWorld>),
                setup_brick_world.run_if(not(resource_exists::<LoadedWorld>)),
                kart::spawn_karts,
                items::setup_items,
                hud::original::load,
                hud::setup_text_hud.run_if(not(resource_exists::<hud::original::Art>)),
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                race_flow,
                kart::player_input,
                kart::ai_drive,
                items::use_items,
                items::actions,
                kart::kart_physics,
                kart::kart_collisions,
                kart::update_places,
                items::pickups,
                racer_sounds::racer_sounds,
                events::track_events,
                events::part_animations,
                (hazards::hazards, hazards::code_lights).chain(),
                (
                    item_models::dress_actions,
                    item_models::dress_karts,
                    scenery::animate,
                    scenery::cycle,
                    scenery::scroll,
                    kart_effects::kart_effects,
                    particles::emit,
                )
                    .chain(),
                kart::sync_karts,
                kart::sync_wheels,
                (chase_camera, particles::particles).chain(),
                hud::update_text_hud.run_if(not(resource_exists::<hud::original::Art>)),
                hud::original::draw.run_if(resource_exists::<hud::original::Art>),
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
        Transform::from_xyz(0.0, 10.0, 20.0),
        // The original's colours are baked in and meant to reach the screen as they are.
        Tonemapping::None,
    ));
    commands.spawn((
        DirectionalLight { illuminance: 11_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(60.0, 100.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder { maximum_distance: 160.0, ..default() }.build(),
    ));
}

/// Loads the chosen circuit and resets the race.
fn load_race(
    mut commands: Commands,
    circuits: Res<Circuits>,
    settings: Res<Settings>,
    demo: Option<Res<DemoShot>>,
) {
    let circuit = &circuits.0[settings.circuit];
    match circuit.race.as_deref().and_then(world::load) {
        Some((track, loaded)) => {
            commands.insert_resource(track);
            commands.insert_resource(loaded);
        }
        None => {
            if circuit.race.is_some() {
                warn!("could not load {}; using the brick circuit", circuit.name);
            }
            commands.insert_resource(Track::new());
            commands.remove_resource::<LoadedWorld>();
        }
    }
    match circuit.race.as_deref().and_then(events::load) {
        Some(events) => commands.insert_resource(events),
        None => commands.remove_resource::<events::TrackEvents>(),
    }
    match circuit.race.as_deref().and_then(hazards::load) {
        Some(hazards) => commands.insert_resource(hazards),
        None => commands.remove_resource::<hazards::Hazards>(),
    }
    commands.insert_resource(Rng(0x1EC0_1999));
    commands.insert_resource(ChaseYaw::default());
    commands.insert_resource(Race {
        phase: Phase::Intro,
        intro: INTRO,
        countdown: COUNTDOWN,
        time: 0.0,
        demo: demo.is_some(),
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

/// The built-in circuit, used when the original game's data isn't available.
fn setup_brick_world(
    mut commands: Commands,
    track: Res<Track>,
    mut rng: ResMut<Rng>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Everything static is vertex-coloured and shares one plastic material.
    let plastic = materials.add(StandardMaterial { perceptual_roughness: 0.5, ..default() });
    commands.spawn((Mesh3d(meshes.add(track.build_mesh())), MeshMaterial3d(plastic.clone())));
    commands.spawn((Mesh3d(meshes.add(track.build_scenery(&mut rng))), MeshMaterial3d(plastic)));
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
fn demo_keys(time: Res<Time<Real>>, demo: Res<DemoShot>, mut keys: ResMut<ButtonInput<KeyCode>>, mut pressed: Local<usize>) {
    for &(key, _) in demo.keys.iter().take(*pressed) {
        keys.release(key);
    }
    if let Some(&(key, _)) = demo.keys.get(*pressed).filter(|k| time.elapsed_secs() >= k.1) {
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
) {
    if let Some(&(power, level, _)) = demo.powers.get(*used).filter(|p| time.elapsed_secs() >= p.2) {
        if let Ok((mut kart, mut controls)) = player.single_mut() {
            (kart.held, kart.whites, controls.use_item) = (Some(power), level, true);
            *used += 1;
        }
    }
    if let Some(mut events) = events {
        while let Some(&(event, _)) = demo.events.get(*fired).filter(|e| time.elapsed_secs() >= e.1) {
            events.fire(event, None, &mut sfx);
            *fired += 1;
        }
    }
    if demo.cycle {
        // Out to the menu a third of the way in, back to the race at two thirds.
        let stage = (time.elapsed_secs() / demo.at * 3.0) as u8;
        if stage > *cycled && stage < 3 {
            *cycled = stage;
            next.set(if stage == 1 { Screen::Menu } else { Screen::Race });
        }
    }
    if time.elapsed_secs() > demo.at && !*taken {
        *taken = true;
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(demo.path.clone()));
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
) {
    let mut restart = false;
    if let Some(dialog) = &mut pause.0 {
        // Paused: the keys work the menu and nothing else moves.
        let count = dialog.options.len();
        let step = keys.just_pressed(KeyCode::ArrowDown) as usize + keys.just_pressed(KeyCode::ArrowUp) as usize * (count - 1);
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
    } else if keys.just_pressed(KeyCode::Escape) {
        sfx.play(audio::id::MENU_BACK);
        // Without the game's own lettering there is no menu to show: just leave.
        if art.is_none() || race.phase == Phase::Finished {
            next.set(Screen::Menu);
        } else {
            pause.0 = Some(Dialog::paused());
            clock.pause();
            debug!("paused");
        }
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
            if karts.iter().any(|(k, player)| player && k.finished.is_some()) {
                race.phase = Phase::Finished;
            }
        }
        Phase::Finished => {}
    }
}

fn chase_camera(
    time: Res<Time>,
    race: Res<Race>,
    demo: Option<Res<DemoShot>>,
    player: Single<&Kart, With<Player>>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
    mut cam_yaw: ResMut<ChaseYaw>,
) {
    let (mut t, mut projection) = camera.into_inner();
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
    let ease = |rate: f32| 1.0 - (-rate * time.delta_secs()).exp();

    // The camera's heading lags the kart's, so powerslides show the kart side-on.
    let yaw = cam_yaw.0.get_or_insert(player.yaw);
    let delta = (player.yaw - *yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    *yaw += delta * ease(6.0);
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());

    // During the intro the camera drops in from high behind the grid.
    let sweep = if race.phase == Phase::Intro { (race.intro / INTRO).clamp(0.0, 1.0).powi(2) } else { 0.0 };
    t.translation = player.pos - forward * (9.0 + 14.0 * sweep) + Vec3::Y * (4.0 + 10.0 * sweep);
    t.look_at(player.pos + forward * 5.0 + Vec3::Y, Vec3::Y);

    if let Projection::Perspective(p) = &mut *projection {
        let fov = if player.boost > 0.0 { 78.0f32 } else { 62.0 }.to_radians();
        p.fov += (fov - p.fov) * ease(5.0);
    }
}
