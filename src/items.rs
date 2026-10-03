//! Power-up bricks: red (cannonball), yellow (oil slick), blue (shield), green (turbo),
//! plus white bricks that upgrade whichever power-up is currently held.

use crate::kart::{Controls, Kart};
use crate::meshgen::*;
use crate::track::Track;
use crate::world::LoadedWorld;
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub enum Power {
    Red,
    Yellow,
    Blue,
    Green,
}

impl Power {
    pub fn color(self) -> Color {
        match self {
            Power::Red => RED,
            Power::Yellow => YELLOW,
            Power::Blue => BLUE,
            Power::Green => GREEN,
        }
    }

    pub fn name(self, level: u8) -> &'static str {
        let names = match self {
            Power::Red => ["Cannonball", "Homing ball", "Triple shot", "Lightning"],
            Power::Yellow => ["Oil slick", "Oil trio", "Oil trio", "Mummy's curse"],
            Power::Blue => ["Shield", "Shield+", "Shield++", "Super shield"],
            Power::Green => ["Turbo", "Turbo+", "Turbo++", "Warp turbo"],
        };
        names[level as usize]
    }
}

#[derive(Component)]
pub struct Pickup {
    /// `None` is a white upgrade brick.
    power: Option<Power>,
    pos: Vec3,
    respawn: f32,
}

#[derive(Component)]
pub struct Projectile {
    s: f32,
    lat: f32,
    speed: f32,
    homing: bool,
    owner: Entity,
    life: f32,
}

#[derive(Component)]
pub struct Hazard {
    pos: Vec3,
    owner: Entity,
    age: f32,
}

#[derive(Resource)]
pub struct ItemAssets {
    ball: Handle<Mesh>,
    ball_mat: Handle<StandardMaterial>,
    slick: Handle<Mesh>,
    slick_mat: Handle<StandardMaterial>,
}

pub fn setup_items(
    mut commands: Commands,
    track: Res<Track>,
    loaded: Option<Res<LoadedWorld>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(ItemAssets {
        ball: meshes.add(Sphere::new(0.6)),
        ball_mat: materials.add(StandardMaterial {
            base_color: BLACK,
            perceptual_roughness: 0.3,
            ..default()
        }),
        slick: meshes.add(Cylinder::new(1.4, 0.06)),
        slick_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(0.02, 0.02, 0.03),
            perceptual_roughness: 0.05,
            ..default()
        }),
    });

    let mut brick = BrickMesh::default();
    brick.brick(Vec3::ZERO, Vec3::new(0.55, 0.33, 0.55), Quat::IDENTITY, Color::WHITE, (2, 2));
    let brick = meshes.add(brick.build());
    let mut material = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 0.4,
            perceptual_roughness: 0.3,
            ..default()
        })
    };
    let powers = [Power::Red, Power::Yellow, Power::Blue, Power::Green];
    let coloured = powers.map(|p| material(p.color()));
    let white = material(WHITE);

    let mut spawn = |pos: Vec3, power: Option<Power>| {
        let mat = match power {
            Some(p) => &coloured[powers.iter().position(|&q| q == p).unwrap()],
            None => &white,
        };
        commands.spawn((
            Pickup { power, pos, respawn: 0.0 },
            Mesh3d(brick.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(pos),
        ));
    };

    // Circuits from the original game come with their own brick placements.
    if let Some(loaded) = loaded {
        for &(power, pos) in &loaded.bricks {
            spawn(pos, power);
        }
        return;
    }

    // Otherwise: rows of bricks at regular stations around the lap, alternating
    // coloured and white.
    const STATIONS: usize = 10;
    for station in 1..STATIONS {
        let s = track.length * station as f32 / STATIONS as f32;
        if station % 2 == 1 {
            for (i, lat) in [-4.5, -1.5, 1.5, 4.5].into_iter().enumerate() {
                spawn(track.point(s, lat), Some(powers[(i + station / 2) % 4]));
            }
        } else {
            for lat in [-3.0, 0.0, 3.0] {
                spawn(track.point(s, lat), None);
            }
        }
    }
}

pub fn pickups(
    time: Res<Time>,
    mut picks: Query<(&mut Pickup, &mut Transform, &mut Visibility)>,
    mut karts: Query<&mut Kart>,
) {
    let t = time.elapsed_secs();
    for (mut p, mut tf, mut vis) in &mut picks {
        if p.respawn > 0.0 {
            p.respawn -= time.delta_secs();
            if p.respawn <= 0.0 {
                *vis = Visibility::Inherited;
            }
            continue;
        }
        tf.rotation = Quat::from_rotation_y(t * 2.0);
        tf.translation.y = p.pos.y + 1.1 + (t * 3.0 + p.pos.x).sin() * 0.15;
        for mut k in &mut karts {
            if k.pos.distance_squared(p.pos) > 2.0 * 2.0 {
                continue;
            }
            match p.power {
                Some(power) => {
                    k.held = Some(power);
                    k.level = 0;
                }
                None if k.held.is_some() => k.level = (k.level + 1).min(3),
                None => {}
            }
            p.respawn = 5.0;
            *vis = Visibility::Hidden;
            break;
        }
    }
}

pub fn use_items(
    mut commands: Commands,
    assets: Res<ItemAssets>,
    track: Res<Track>,
    mut q: Query<(Entity, &mut Kart, &mut Controls)>,
) {
    /// Effects that land on other karts, applied once we're done iterating.
    enum Global {
        Lightning { owner: Entity, progress: f32 },
        Curse { owner: Entity },
    }
    let mut globals = Vec::new();

    for (owner, mut k, mut c) in &mut q {
        if !c.use_item {
            continue;
        }
        c.use_item = false;
        if k.spin > 0.0 {
            continue;
        }
        let Some(power) = k.held.take() else { continue };
        let level = std::mem::take(&mut k.level);
        let spread: &[f32] = if level >= 1 { &[-2.5, 0.0, 2.5] } else { &[0.0] };
        match power {
            Power::Green => k.boost = 1.5 + level as f32,
            Power::Blue => k.shield = 4.0 + 2.0 * level as f32,
            Power::Red if level == 3 => {
                globals.push(Global::Lightning { owner, progress: k.progress });
            }
            Power::Red => {
                let shots = if level == 2 { spread } else { &[0.0] };
                for dl in shots {
                    commands.spawn((
                        Projectile {
                            s: k.s + 3.0,
                            lat: k.lat + dl,
                            speed: k.vel.length() + 40.0,
                            homing: level >= 1,
                            owner,
                            life: 4.0,
                        },
                        Mesh3d(assets.ball.clone()),
                        MeshMaterial3d(assets.ball_mat.clone()),
                        Transform::from_translation(k.pos),
                    ));
                }
            }
            Power::Yellow if level == 3 => globals.push(Global::Curse { owner }),
            Power::Yellow => {
                for dl in spread {
                    let pos = track.surface_point(k.s - 3.5, (k.lat + dl).clamp(-track.road, track.road));
                    commands.spawn((
                        Hazard { pos, owner, age: 0.0 },
                        Mesh3d(assets.slick.clone()),
                        MeshMaterial3d(assets.slick_mat.clone()),
                        Transform::from_translation(pos + Vec3::Y * 0.05),
                    ));
                }
            }
        }
    }

    for global in globals {
        for (e, mut k, _) in &mut q {
            match global {
                Global::Lightning { owner, progress } if e != owner && k.progress > progress => {
                    k.hit();
                }
                Global::Curse { owner } if e != owner && k.shield <= 0.0 => k.slow = 4.0,
                _ => {}
            }
        }
    }
}

pub fn projectiles(
    mut commands: Commands,
    time: Res<Time>,
    track: Res<Track>,
    mut shots: Query<(Entity, &mut Projectile, &mut Transform)>,
    mut karts: Query<(Entity, &mut Kart)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p, mut tf) in &mut shots {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        p.s += p.speed * dt;
        if p.homing {
            // Drift towards the lane of the nearest kart up the road.
            let gap = |k: &Kart| (k.s - p.s).rem_euclid(track.length);
            let target = karts
                .iter()
                .filter(|(e, k)| *e != p.owner && gap(k) < 120.0)
                .min_by(|a, b| gap(a.1).total_cmp(&gap(b.1)))
                .map(|(_, k)| k.lat);
            if let Some(lat) = target {
                p.lat += (lat - p.lat).clamp(-12.0 * dt, 12.0 * dt);
            }
        }
        let pos = track.surface_point(p.s, p.lat) + Vec3::Y * 0.7;
        tf.translation = pos;
        for (e, mut k) in &mut karts {
            if e != p.owner && k.pos.distance_squared(pos) < 2.0 * 2.0 {
                k.hit();
                commands.entity(entity).despawn();
                break;
            }
        }
    }
}

pub fn hazards(
    mut commands: Commands,
    time: Res<Time>,
    mut slicks: Query<(Entity, &mut Hazard)>,
    mut karts: Query<(Entity, &mut Kart)>,
) {
    for (entity, mut h) in &mut slicks {
        h.age += time.delta_secs();
        if h.age > 25.0 {
            commands.entity(entity).despawn();
            continue;
        }
        for (e, mut k) in &mut karts {
            // The kart that dropped it gets a moment to drive clear.
            let armed = e != h.owner || h.age > 1.5;
            if armed && k.pos.distance_squared(h.pos) < 1.9 * 1.9 {
                k.hit();
                commands.entity(entity).despawn();
                break;
            }
        }
    }
}
