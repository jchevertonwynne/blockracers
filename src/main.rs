mod assets;
mod audio;
mod collision;
mod events;
mod hud;
mod items;
mod kart;
mod menu;
mod meshgen;
mod mixer;
mod physics;
mod racer_sounds;
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
}

fn main() {
    let demo = std::env::var("BRICK_DEMO").ok().and_then(|v| {
        let mut parts = v.split(':');
        let (at, path) = (parts.next()?.parse().ok()?, parts.next()?.to_string());
        let mode = parts.next();
        Some((DemoShot { at, path, cycle: mode == Some("cycle") }, mode == Some("menu")))
    });
    let circuits = Circuits::find();
    let settings = Settings::new(&circuits);

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Brick Racers".into(), ..default() }),
        ..default()
    }));
    match demo {
        Some((shot, on_menu)) => {
            app.insert_resource(shot).add_systems(Update, demo_shot);
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
        .add_plugins((menu::plugin, audio::plugin))
        .add_systems(Startup, setup_scene)
        .add_systems(
            OnEnter(Screen::Race),
            (
                load_race,
                world::spawn_world.run_if(resource_exists::<LoadedWorld>),
                setup_brick_world.run_if(not(resource_exists::<LoadedWorld>)),
                kart::spawn_karts,
                items::setup_items,
                hud::setup_hud,
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
                kart::sync_karts,
                kart::sync_wheels,
                chase_camera,
                hud::update_hud,
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
    commands.insert_resource(Rng(0x1EC0_1999));
    commands.insert_resource(ChaseYaw::default());
    commands.insert_resource(Race {
        phase: Phase::Intro,
        intro: INTRO,
        countdown: COUNTDOWN,
        time: 0.0,
        demo: demo.is_some(),
    });
}

/// Everything a race puts in the world goes when the race does.
fn tag_race_entities(
    mut commands: Commands,
    new: Query<Entity, (Or<(Added<Mesh3d>, Added<Node>, Added<Kart>)>, Without<ChildOf>)>,
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

fn demo_shot(
    mut commands: Commands,
    time: Res<Time>,
    demo: Res<DemoShot>,
    mut exit: MessageWriter<AppExit>,
    mut next: ResMut<NextState<Screen>>,
    mut taken: Local<bool>,
    mut cycled: Local<u8>,
) {
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
) {
    if keys.just_pressed(KeyCode::Escape) {
        sfx.play(audio::id::MENU_BACK);
        next.set(Screen::Menu);
        return;
    }
    match race.phase {
        Phase::Intro => {
            race.intro -= time.delta_secs();
            if race.intro <= 0.0 || race.demo {
                race.phase = Phase::Countdown;
            }
        }
        Phase::Countdown => {
            race.countdown -= time.delta_secs();
            if race.countdown <= 0.0 || race.demo {
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
        Phase::Finished if keys.just_pressed(KeyCode::Enter) => {
            for (mut k, _) in &mut karts {
                k.reset(&track);
            }
            for e in &debris {
                commands.entity(e).despawn();
            }
            race.phase = Phase::Intro;
            race.intro = INTRO;
            race.countdown = COUNTDOWN;
        }
        Phase::Finished => {}
    }
}

fn chase_camera(
    time: Res<Time>,
    race: Res<Race>,
    player: Single<&Kart, With<Player>>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
    mut cam_yaw: ResMut<ChaseYaw>,
) {
    let (mut t, mut projection) = camera.into_inner();
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
