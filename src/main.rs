mod assets;
mod collision;
mod hud;
mod items;
mod kart;
mod meshgen;
mod physics;
mod track;
mod world;

use bevy::{
    light::CascadeShadowConfigBuilder,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use items::{Hazard, Projectile};
use kart::{Kart, Player};
use meshgen::Rng;
use track::Track;

const SKY: Color = Color::srgb(0.45, 0.70, 0.95);

#[derive(Clone, Copy, PartialEq)]
pub enum Phase {
    Title,
    Countdown,
    Racing,
    Finished,
}

#[derive(Resource)]
pub struct Race {
    pub phase: Phase,
    pub countdown: f32,
    /// Seconds since the lights went out.
    pub time: f32,
    /// Attract mode: the AI drives the player's kart too.
    pub demo: bool,
}

/// `BRICK_DEMO=<seconds>:<png path>` runs attract mode, saves a screenshot and quits.
#[derive(Resource)]
struct DemoShot {
    at: f32,
    path: String,
}

fn main() {
    let demo = std::env::var("BRICK_DEMO").ok().and_then(|v| {
        let (at, path) = v.split_once(':')?;
        Some(DemoShot { at: at.parse().ok()?, path: path.to_string() })
    });
    let mut app = App::new();
    if let Some(demo) = demo {
        app.insert_resource(demo).add_systems(Update, demo_shot);
    }
    // Race on a circuit from the original game if its data is present.
    match world::load() {
        Some((track, loaded)) => {
            app.insert_resource(track)
                .insert_resource(loaded)
                .add_systems(Startup, world::spawn_world);
        }
        None => {
            app.insert_resource(Track::new()).add_systems(Startup, setup_brick_world);
        }
    }
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "Brick Racers".into(), ..default() }),
            ..default()
        }))
        .insert_resource(ClearColor(SKY))
        .insert_resource(GlobalAmbientLight { brightness: 500.0, ..default() })
        .insert_resource(Rng(0x1EC0_1999))
        .insert_resource(Race { phase: Phase::Title, countdown: 0.0, time: 0.0, demo: false })
        .add_systems(Startup, (setup_scene, kart::spawn_karts, items::setup_items, hud::setup_hud))
        .add_systems(
            Update,
            (
                race_flow,
                kart::player_input,
                kart::ai_drive,
                items::use_items,
                kart::kart_physics,
                kart::kart_collisions,
                kart::update_places,
                items::pickups,
                items::projectiles,
                items::hazards,
                kart::sync_karts,
                kart::sync_wheels,
                chase_camera,
                hud::update_hud,
            )
                .chain(),
        )
        .run();
}

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 10.0, 20.0),
        DistanceFog {
            color: SKY,
            falloff: FogFalloff::Linear { start: 250.0, end: 700.0 },
            ..default()
        },
    ));
    commands.spawn((
        DirectionalLight { illuminance: 11_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(60.0, 100.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder { maximum_distance: 160.0, ..default() }.build(),
    ));
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
    mut race: ResMut<Race>,
    mut exit: MessageWriter<AppExit>,
    mut taken: Local<bool>,
) {
    if race.phase == Phase::Title {
        race.demo = true;
        race.phase = Phase::Countdown;
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
    mut karts: Query<(&mut Kart, Has<Player>)>,
    debris: Query<Entity, Or<(With<Projectile>, With<Hazard>)>>,
) {
    let start = keys.just_pressed(KeyCode::Enter);
    match race.phase {
        Phase::Title if start => {
            race.phase = Phase::Countdown;
            race.countdown = 3.0;
        }
        Phase::Title => {}
        Phase::Countdown => {
            race.countdown -= time.delta_secs();
            if race.countdown <= 0.0 {
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
        Phase::Finished if start => {
            for (mut k, _) in &mut karts {
                k.reset(&track);
            }
            for e in &debris {
                commands.entity(e).despawn();
            }
            race.phase = Phase::Countdown;
            race.countdown = 3.0;
        }
        Phase::Finished => {}
    }
}

fn chase_camera(
    time: Res<Time>,
    player: Single<&Kart, With<Player>>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
    mut cam_yaw: Local<Option<f32>>,
) {
    let (mut t, mut projection) = camera.into_inner();
    let ease = |rate: f32| 1.0 - (-rate * time.delta_secs()).exp();

    // The camera's heading lags the kart's, so powerslides show the kart side-on.
    let yaw = cam_yaw.get_or_insert(player.yaw);
    let delta = (player.yaw - *yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    *yaw += delta * ease(6.0);
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());

    t.translation = player.pos - forward * 9.0 + Vec3::Y * 4.0;
    t.look_at(player.pos + forward * 5.0 + Vec3::Y, Vec3::Y);

    if let Projection::Perspective(p) = &mut *projection {
        let fov = if player.boost > 0.0 { 78.0f32 } else { 62.0 }.to_radians();
        p.fov += (fov - p.fov) * ease(5.0);
    }
}
