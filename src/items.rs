//! Power-ups. A coloured brick gives a power-up; each white brick collected (up to
//! three) raises the level it will fire at:
//!
//! | brick  | 0           | 1              | 2              | 3              |
//! |--------|-------------|----------------|----------------|----------------|
//! | red    | cannon ball | grappling hook | lightning wand | homing missile |
//! | yellow | oil slick   | dynamite       | magnet         | mummy's curse  |
//! | blue   | shield, lasting longer at each level and deflecting shots from level 2 |
//! | green  | turbo boost, longer at each level              | warp           |
//!
//! Behaviour and numbers follow the original's power-up actions; the models and
//! effects are stand-ins.

use crate::kart::{Controls, Kart};
use crate::meshgen::*;
use crate::physics::UNIT;
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
            Power::Red => ["Cannon ball", "Grappling hook", "Lightning wand", "Homing missile"],
            Power::Yellow => ["Oil slick", "Dynamite", "Magnet", "Mummy's curse"],
            Power::Blue => ["Shield", "Shield II", "Shield III", "Shield IV"],
            Power::Green => ["Turbo", "Turbo II", "Turbo III", "Warp"],
        };
        names[level.min(3) as usize]
    }
}

const MAX_WHITE_BRICKS: u8 = 3;
const BRICK_RESPAWN: f32 = 5.0;

const SHIELD_TIMES: [f32; 4] = [4.0, 6.0, 8.0, 10.0];
/// Shields of this level and up send cannon balls back where they came from.
const DEFLECTING_SHIELD: u8 = 2;
const TURBO_TIMES: [f32; 3] = [1.0, 1.5, 5.0];
const WARP_TIME: f32 = 1.5;

// Aiming: the nearest racer inside a cone ahead, between these distances.
const AIM_MIN: f32 = 10.0 * UNIT;
const AIM_MAX: f32 = 400.0 * UNIT;
const AIM_CONE: f32 = 0.9;
const HOOK_CONE: f32 = 0.6;
const MISSILE_CONE: f32 = 0.7071;

const LAUNCH_HEIGHT: f32 = 5.0 * UNIT;
const CANNONBALL_SPEED: f32 = 180.0 * UNIT;
const CANNONBALL_GRAVITY: f32 = 32.176 * UNIT;
const CANNONBALL_RANGE: f32 = 500.0 * UNIT;
const CANNONBALL_BLAST: f32 = 5.0 * UNIT;
const HOOK_SPEED: f32 = 320.0 * UNIT;
const HOOK_GRAVITY: f32 = 90.176 * UNIT;
const HOOK_FLIGHT_TIME: f32 = 3.0;
const HOOK_PULL_TIME: f32 = 4.0;
/// Acceleration on both ends of the rope.
const HOOK_PULL: f32 = 180.0 * UNIT;
const HOOK_RELEASE_DISTANCE: f32 = 12.0 * UNIT;
const LIGHTNING_TIME: f32 = 7.0;
const LIGHTNING_RANGE: f32 = 50.0 * UNIT;
const LIGHTNING_MIN_RANGE: f32 = 3.0 * UNIT;
const LIGHTNING_CONE: f32 = 0.5;
const MISSILE_SPEED: f32 = 170.0 * UNIT;
const MISSILE_FLIGHT_TIME: f32 = 5.5;
/// Within this distance the missile leaves the racing line and goes straight for its target.
const MISSILE_SNAP_DISTANCE: f32 = 60.0 * UNIT;
const MISSILE_SPIN_TURNS: f32 = 2.0;
const BIG_BLAST: f32 = 10.0 * UNIT;

const OIL_TIME: f32 = 10.0;
const OIL_SPIN_TURNS: f32 = 1.0;
const DYNAMITE_THROW: f32 = 90.0 * UNIT;
const DYNAMITE_FUSE: f32 = 5.0;
const DYNAMITE_BLASTS: u8 = 3;
const DYNAMITE_BLAST_INTERVAL: f32 = 0.5;
const MAGNET_ARMED_TIME: f32 = 20.0;
const MAGNET_HOLD_TIME: f32 = 4.0;
const CURSE_ARMED_TIME: f32 = 15.0;
const CURSE_TIME: f32 = 10.0;
/// Magnets and curses catch racers who come this close.
const TRAP_RADIUS: f32 = 10.0 * UNIT;
/// Things dropped on the road are safe for the kart that dropped them for this long.
const DROP_GRACE: f32 = 1.0;
/// How close to a kart's middle counts as touching it.
const KART_RADIUS: f32 = 1.5;
const EXPLOSION_TIME: f32 = 0.4;

#[derive(Component)]
pub struct Pickup {
    /// `None` is a white brick.
    power: Option<Power>,
    pos: Vec3,
    respawn: f32,
}

/// Something a power-up has put into the world. Its position is its `Transform`.
#[derive(Component)]
pub enum Action {
    Cannonball { owner: Entity, vel: Vec3, travelled: f32 },
    /// Flying until `pulling` is set, then reeling owner and victim together.
    Hook { owner: Entity, vel: Vec3, time: f32, pulling: Option<Entity> },
    Lightning { owner: Entity, time: f32 },
    /// Follows the racing line at (s, lat) until close to its target.
    Missile { owner: Entity, target: Option<Entity>, s: f32, lat: f32, time: f32 },
    OilSlick { owner: Entity, age: f32 },
    Dynamite { owner: Entity, fuse: f32, blasts: u8 },
    Magnet { owner: Entity, age: f32 },
    Curse { owner: Entity, age: f32 },
    Explosion { age: f32, radius: f32 },
}

#[derive(Resource)]
pub struct ItemAssets {
    sphere: Handle<Mesh>,
    disc: Handle<Mesh>,
    stick: Handle<Mesh>,
    cube: Handle<Mesh>,
    black: Handle<StandardMaterial>,
    grey: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    oil: Handle<StandardMaterial>,
    magnet: Handle<StandardMaterial>,
    curse: Handle<StandardMaterial>,
    fire: Handle<StandardMaterial>,
    bolt: Handle<StandardMaterial>,
}

pub fn setup_items(
    mut commands: Commands,
    track: Res<Track>,
    loaded: Option<Res<LoadedWorld>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut plain = |color: Color, glow: f32| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * glow,
            perceptual_roughness: 0.3,
            ..default()
        })
    };
    let assets = ItemAssets {
        sphere: meshes.add(Sphere::new(1.0)),
        disc: meshes.add(Cylinder::new(1.0, 0.06)),
        stick: meshes.add(Cylinder::new(0.25, 0.9)),
        cube: meshes.add(Cuboid::from_length(1.0)),
        black: plain(BLACK, 0.0),
        grey: plain(GREY, 0.0),
        red: plain(RED, 0.3),
        oil: plain(Color::srgb(0.02, 0.02, 0.03), 0.0),
        magnet: plain(Color::srgb(0.3, 0.35, 0.8), 0.8),
        curse: plain(Color::srgb(0.5, 0.1, 0.7), 1.5),
        fire: plain(Color::srgba(1.0, 0.55, 0.1, 0.6), 6.0),
        bolt: plain(Color::srgb(0.1, 0.25, 0.96), 12.0),
    };
    if let Some(mut fire) = materials.get_mut(&assets.fire) {
        fire.alpha_mode = AlphaMode::Blend;
    }
    commands.insert_resource(assets);

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
                // A new colour replaces the old one; white bricks are kept.
                Some(power) => k.held = Some(power),
                None if k.whites < MAX_WHITE_BRICKS => k.whites += 1,
                // Already carrying all the white bricks there's room for.
                None => continue,
            }
            p.respawn = BRICK_RESPAWN;
            *vis = Visibility::Hidden;
            break;
        }
    }
}

/// The nearest kart other than `owner` inside a cone around `forward`.
fn aim(
    karts: &[(Entity, Vec3)],
    owner: Entity,
    from: Vec3,
    forward: Vec3,
    cone: f32,
) -> Option<(Entity, Vec3)> {
    karts
        .iter()
        .filter(|(e, pos)| {
            let to = *pos - from;
            let distance = to.length();
            *e != owner
                && (AIM_MIN..AIM_MAX).contains(&distance)
                && to.dot(forward) / distance >= cone
        })
        .min_by(|a, b| a.1.distance_squared(from).total_cmp(&b.1.distance_squared(from)))
        .copied()
}

/// Velocity that carries a projectile from `from` to `to` at `speed` along the ground,
/// arcing under `gravity`.
fn lob(from: Vec3, to: Vec3, speed: f32, gravity: f32) -> Vec3 {
    let time = (from.distance(to) / speed).max(0.05);
    (to - from) / time + Vec3::Y * 0.5 * gravity * time
}

pub fn use_items(
    mut commands: Commands,
    assets: Res<ItemAssets>,
    track: Res<Track>,
    mut q: Query<(Entity, &mut Kart, &mut Controls)>,
) {
    let positions: Vec<(Entity, Vec3)> = q.iter().map(|(e, k, _)| (e, k.pos)).collect();
    for (owner, mut k, mut c) in &mut q {
        if !std::mem::take(&mut c.use_item) {
            continue;
        }
        if k.spin > 0.0 || k.spin_out > 0.0 || k.magnet > 0.0 || k.warp > 0.0 {
            continue;
        }
        let Some(power) = k.held.take() else { continue };
        let level = std::mem::take(&mut k.whites).min(3);

        let forward = (k.rot * Vec3::NEG_Z).normalize();
        let muzzle = k.pos + Vec3::Y * LAUNCH_HEIGHT;
        let behind = track.surface_point(k.s - 2.5, k.lat);
        let mut spawn = |action: Action, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Vec3, size: Vec3| {
            commands.spawn((
                action,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(at).with_scale(size),
            ));
        };
        match (power, level) {
            (Power::Red, 0) => {
                let target = aim(&positions, owner, k.pos, forward, AIM_CONE)
                    .map_or(muzzle + forward * CANNONBALL_RANGE - Vec3::Y * LAUNCH_HEIGHT, |t| t.1 + Vec3::Y * 0.6);
                let vel = lob(muzzle, target, CANNONBALL_SPEED, CANNONBALL_GRAVITY);
                spawn(Action::Cannonball { owner, vel, travelled: 0.0 }, &assets.sphere, &assets.black, muzzle, Vec3::splat(0.5));
            }
            (Power::Red, 1) => {
                let target = aim(&positions, owner, k.pos, forward, HOOK_CONE)
                    .map_or(muzzle + forward * AIM_MAX * 0.5, |t| t.1 + Vec3::Y * 0.6);
                let vel = lob(muzzle, target, HOOK_SPEED, HOOK_GRAVITY);
                spawn(Action::Hook { owner, vel, time: HOOK_FLIGHT_TIME, pulling: None }, &assets.cube, &assets.grey, muzzle, Vec3::splat(0.5));
            }
            (Power::Red, 2) => {
                let size = Vec3::new(0.3, 0.3, LIGHTNING_RANGE);
                spawn(Action::Lightning { owner, time: LIGHTNING_TIME }, &assets.cube, &assets.bolt, muzzle, size);
            }
            (Power::Red, _) => {
                let target = aim(&positions, owner, k.pos, forward, MISSILE_CONE).map(|t| t.0);
                let action = Action::Missile { owner, target, s: k.s + 3.0, lat: k.lat, time: MISSILE_FLIGHT_TIME };
                spawn(action, &assets.sphere, &assets.red, muzzle, Vec3::new(0.35, 0.35, 0.8));
            }
            (Power::Yellow, 0) => {
                spawn(Action::OilSlick { owner, age: 0.0 }, &assets.disc, &assets.oil, behind + Vec3::Y * 0.05, Vec3::new(1.4, 1.0, 1.4));
            }
            (Power::Yellow, 1) => {
                let landing = track.surface_point(k.s - DYNAMITE_THROW, k.lat);
                let action = Action::Dynamite { owner, fuse: DYNAMITE_FUSE, blasts: DYNAMITE_BLASTS };
                spawn(action, &assets.stick, &assets.red, landing + Vec3::Y * 0.45, Vec3::ONE);
            }
            (Power::Yellow, 2) => {
                spawn(Action::Magnet { owner, age: 0.0 }, &assets.disc, &assets.magnet, behind + Vec3::Y * 0.08, Vec3::new(1.2, 3.0, 1.2));
            }
            (Power::Yellow, _) => {
                spawn(Action::Curse { owner, age: 0.0 }, &assets.disc, &assets.curse, behind + Vec3::Y * 0.08, Vec3::new(1.2, 3.0, 1.2));
            }
            (Power::Blue, level) => {
                k.shield = SHIELD_TIMES[level as usize];
                k.shield_level = level;
            }
            (Power::Green, 3) => k.warp = WARP_TIME,
            (Power::Green, level) => k.boost = TURBO_TIMES[level as usize],
        }
    }
}

/// Runs everything power-ups have put into the world.
pub fn actions(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<ItemAssets>,
    track: Res<Track>,
    mut actions: Query<(Entity, &mut Action, &mut Transform)>,
    mut karts: Query<(Entity, &mut Kart)>,
) {
    let dt = time.delta_secs();
    // Blasts are collected and set off once every action has had its turn.
    let mut blasts: Vec<(Vec3, f32, Entity)> = Vec::new();
    let touching = |k: &Kart, at: Vec3, radius: f32| (k.pos + Vec3::Y * 0.6).distance_squared(at) < radius * radius;

    for (entity, mut action, mut tf) in &mut actions {
        let pos = tf.translation;
        let mut done = false;
        match &mut *action {
            Action::Cannonball { owner, vel, travelled } => {
                vel.y -= CANNONBALL_GRAVITY * dt;
                let next = pos + *vel * dt;
                *travelled += vel.length() * dt;
                tf.translation = next;
                let struck = karts.iter_mut().find(|(e, k)| e != owner && k.warp <= 0.0 && touching(k, next, KART_RADIUS));
                if let Some((victim, mut k)) = struck {
                    if k.shielded() && k.shield_level >= DEFLECTING_SHIELD {
                        // Sent back; it now belongs to whoever deflected it.
                        *vel = -*vel;
                        *owner = victim;
                    } else {
                        if !k.shielded() {
                            k.whites = k.whites.saturating_sub(1);
                            blasts.push((next, CANNONBALL_BLAST, *owner));
                        }
                        done = true;
                    }
                } else if track.collision.any(pos, next).is_some() || *travelled > CANNONBALL_RANGE * 1.5 {
                    blasts.push((next, CANNONBALL_BLAST, *owner));
                    done = true;
                }
            }
            Action::Hook { owner, vel, time, pulling } => {
                *time -= dt;
                done = *time <= 0.0;
                match *pulling {
                    None => {
                        vel.y -= HOOK_GRAVITY * dt;
                        let next = pos + *vel * dt;
                        tf.translation = next;
                        let caught = karts.iter().find(|(e, k)| e != owner && k.warp <= 0.0 && touching(k, next, KART_RADIUS));
                        if let Some((victim, k)) = caught {
                            if k.shielded() {
                                done = true;
                            } else {
                                *pulling = Some(victim);
                                *time = HOOK_PULL_TIME;
                            }
                        } else if track.collision.any(pos, next).is_some() {
                            done = true;
                        }
                    }
                    Some(victim) => {
                        // Reel the two karts towards each other until they meet.
                        let ends = (karts.get(*owner).map(|k| k.1.pos), karts.get(victim).map(|k| (k.1.pos, k.1.shielded())));
                        if let (Ok(from), Ok((to, shielded))) = ends {
                            let rope = to - from;
                            if shielded || rope.length() < HOOK_RELEASE_DISTANCE {
                                done = true;
                            } else {
                                let pull = rope.normalize() * HOOK_PULL;
                                if let Ok((_, mut k)) = karts.get_mut(*owner) {
                                    k.external_force += pull;
                                }
                                if let Ok((_, mut k)) = karts.get_mut(victim) {
                                    k.external_force -= pull;
                                }
                                tf.translation = to + Vec3::Y * 0.8;
                            }
                        } else {
                            done = true;
                        }
                    }
                }
            }
            Action::Lightning { owner, time } => {
                *time -= dt;
                done = *time <= 0.0;
                let Ok((from, forward)) = karts.get(*owner).map(|(_, k)| (k.pos, (k.rot * Vec3::NEG_Z).normalize())) else {
                    commands.entity(entity).despawn();
                    continue;
                };
                // The bolt reaches out ahead of the kart holding the wand.
                tf.translation = from + Vec3::Y * 0.9 + forward * LIGHTNING_RANGE * 0.5;
                tf.rotation = Transform::IDENTITY.looking_to(forward, Vec3::Y).rotation;
                for (e, mut k) in &mut karts {
                    let to = k.pos - from;
                    let distance = to.length();
                    if e != *owner
                        && (LIGHTNING_MIN_RANGE..LIGHTNING_RANGE).contains(&distance)
                        && to.dot(forward) / distance >= LIGHTNING_CONE
                    {
                        k.launch();
                    }
                }
            }
            Action::Missile { owner, target, s, lat, time } => {
                *time -= dt;
                let aim = target.and_then(|t| karts.get(t).ok()).map(|(_, k)| (k.pos + Vec3::Y * 0.6, k.lat));
                let next = match aim {
                    Some((at, _)) if at.distance(pos) < MISSILE_SNAP_DISTANCE => {
                        pos + (at - pos).normalize_or_zero() * MISSILE_SPEED * dt
                    }
                    _ => {
                        *s += MISSILE_SPEED * dt;
                        if let Some((_, target_lat)) = aim {
                            *lat += (target_lat - *lat).clamp(-12.0 * dt, 12.0 * dt);
                        }
                        track.surface_point(*s, *lat) + Vec3::Y * LAUNCH_HEIGHT * 0.8
                    }
                };
                tf.look_to(next - pos, Vec3::Y);
                tf.translation = next;
                let struck = karts.iter_mut().find(|(e, k)| e != owner && k.warp <= 0.0 && touching(k, next, KART_RADIUS));
                if let Some((_, mut k)) = struck {
                    if !k.shielded() {
                        k.spin_round(MISSILE_SPIN_TURNS);
                        blasts.push((next, BIG_BLAST, *owner));
                    }
                    done = true;
                } else if *time <= 0.0 {
                    blasts.push((next, BIG_BLAST, *owner));
                    done = true;
                }
            }
            Action::OilSlick { owner, age } => {
                *age += dt;
                done = *age > OIL_TIME;
                for (e, mut k) in &mut karts {
                    let armed = e != *owner || *age > DROP_GRACE;
                    if armed && k.contacts > 0 && k.spin <= 0.0 && touching(&k, pos, KART_RADIUS) {
                        k.spin_round(OIL_SPIN_TURNS);
                        done = true;
                        break;
                    }
                }
            }
            Action::Dynamite { owner, fuse, blasts: left } => {
                *fuse -= dt;
                if *fuse <= 0.0 {
                    blasts.push((pos, BIG_BLAST, *owner));
                    *left -= 1;
                    *fuse = DYNAMITE_BLAST_INTERVAL;
                    done = *left == 0;
                }
            }
            Action::Magnet { owner, age } => {
                *age += dt;
                done = *age > MAGNET_ARMED_TIME;
                for (e, mut k) in &mut karts {
                    if e != *owner && !k.shielded() && k.warp <= 0.0 && touching(&k, pos, TRAP_RADIUS) {
                        k.magnet = MAGNET_HOLD_TIME;
                        done = true;
                        break;
                    }
                }
            }
            Action::Curse { owner, age } => {
                *age += dt;
                done = *age > CURSE_ARMED_TIME;
                for (e, mut k) in &mut karts {
                    if e != *owner && !k.shielded() && k.warp <= 0.0 && touching(&k, pos, TRAP_RADIUS) {
                        k.cursed = CURSE_TIME;
                        // A curse ends any turbo.
                        k.boost = 0.0;
                        done = true;
                        break;
                    }
                }
            }
            Action::Explosion { age, radius } => {
                *age += dt;
                tf.scale = Vec3::splat(*radius * (0.3 + 0.7 * *age / EXPLOSION_TIME));
                done = *age > EXPLOSION_TIME;
            }
        }
        if done {
            commands.entity(entity).despawn();
        }
    }

    // Blasts throw every kart in reach except the one whose weapon it was.
    for (at, radius, owner) in blasts {
        for (e, mut k) in &mut karts {
            if e != owner && touching(&k, at, radius + KART_RADIUS) {
                k.launch();
            }
        }
        commands.spawn((
            Action::Explosion { age: 0.0, radius },
            Mesh3d(assets.sphere.clone()),
            MeshMaterial3d(assets.fire.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(radius * 0.3)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use std::time::Duration;

    /// A world with an owner on the brick circuit and a victim `gap` ahead of it.
    fn arena(gap: f32) -> (World, Entity, Entity) {
        let mut world = World::new();
        let track = Track::new();
        let handle = Handle::<Mesh>::default;
        let material = Handle::<StandardMaterial>::default;
        world.insert_resource(ItemAssets {
            sphere: handle(),
            disc: handle(),
            stick: handle(),
            cube: handle(),
            black: material(),
            grey: material(),
            red: material(),
            oil: material(),
            magnet: material(),
            curse: material(),
            fire: material(),
            bolt: material(),
        });
        let spawn = |world: &mut World, slot: usize, s: f32| {
            let mut kart = Kart::new(&track, slot);
            kart.place(&track, s, 0.0);
            world.spawn((kart, Controls::default(), Transform::default())).id()
        };
        let owner = spawn(&mut world, 0, 200.0);
        let victim = spawn(&mut world, 1, 200.0 + gap);
        world.insert_resource(track);
        world.insert_resource(Time::<()>::default());
        (world, owner, victim)
    }

    fn fire(world: &mut World, owner: Entity, power: Power, level: u8) {
        let mut kart = world.get_mut::<Kart>(owner).unwrap();
        kart.held = Some(power);
        kart.whites = level;
        world.get_mut::<Controls>(owner).unwrap().use_item = true;
        world.run_system_once(use_items).unwrap();
    }

    /// Runs the game for a while and reports whether `check` ever held.
    fn ever(world: &mut World, seconds: f32, check: impl Fn(&World) -> bool) -> bool {
        let mut seen = false;
        for _ in 0..(seconds * 60.0) as usize {
            world.resource_mut::<Time>().advance_by(Duration::from_secs_f32(1.0 / 60.0));
            world.run_system_once(actions).unwrap();
            world.run_system_once(crate::kart::kart_physics).unwrap();
            seen |= check(world);
        }
        seen
    }

    fn kart(world: &World, e: Entity) -> &Kart {
        world.get::<Kart>(e).unwrap()
    }

    #[test]
    fn cannon_ball_launches_its_target_and_costs_it_a_white_brick() {
        let (mut world, owner, victim) = arena(12.0);
        world.get_mut::<Kart>(victim).unwrap().whites = 2;
        fire(&mut world, owner, Power::Red, 0);
        assert!(ever(&mut world, 2.0, |w| kart(w, victim).spin_out > 0.0));
        assert_eq!(kart(&world, victim).whites, 1);
        assert!(kart(&world, owner).held.is_none());
    }

    #[test]
    fn strong_shield_sends_a_cannon_ball_back() {
        let (mut world, owner, victim) = arena(12.0);
        fire(&mut world, victim, Power::Blue, 2);
        fire(&mut world, owner, Power::Red, 0);
        assert!(ever(&mut world, 3.0, |w| kart(w, owner).spin_out > 0.0));
        assert_eq!(kart(&world, victim).spin_out, 0.0);
    }

    #[test]
    fn weak_shield_just_absorbs_a_cannon_ball() {
        let (mut world, owner, victim) = arena(12.0);
        fire(&mut world, victim, Power::Blue, 0);
        fire(&mut world, owner, Power::Red, 0);
        assert!(!ever(&mut world, 3.0, |w| kart(w, owner).spin_out > 0.0 || kart(w, victim).spin_out > 0.0));
    }

    #[test]
    fn grappling_hook_reels_the_karts_together() {
        let (mut world, owner, victim) = arena(14.0);
        fire(&mut world, owner, Power::Red, 1);
        let gap = |w: &World| kart(w, owner).pos.distance(kart(w, victim).pos);
        let before = gap(&world);
        assert!(ever(&mut world, 3.0, |w| gap(w) < before - 5.0));
    }

    #[test]
    fn lightning_launches_karts_ahead_but_not_behind() {
        let (mut world, owner, victim) = arena(8.0);
        fire(&mut world, owner, Power::Red, 2);
        assert!(ever(&mut world, 1.0, |w| kart(w, victim).spin_out > 0.0));

        let (mut world, owner, victim) = arena(-8.0);
        fire(&mut world, owner, Power::Red, 2);
        assert!(!ever(&mut world, 1.0, |w| kart(w, victim).spin_out > 0.0));
    }

    #[test]
    fn homing_missile_follows_the_road_and_spins_its_target() {
        let (mut world, owner, victim) = arena(60.0);
        fire(&mut world, owner, Power::Red, 3);
        assert!(ever(&mut world, 5.0, |w| kart(w, victim).spin > 0.0));
    }

    #[test]
    fn dropped_hazards_catch_the_kart_behind() {
        // The victim sits just behind, where things get dropped.
        let hazard = |level: u8, seconds: f32, check: fn(&Kart) -> bool| {
            let (mut world, owner, victim) = arena(-3.0);
            fire(&mut world, owner, Power::Yellow, level);
            assert!(ever(&mut world, seconds, |w| check(kart(w, victim))), "yellow level {level}");
            assert!(!check(kart(&world, owner)), "yellow level {level} caught its owner");
        };
        hazard(0, 1.0, |k| k.spin > 0.0);
        hazard(2, 1.0, |k| k.magnet > 0.0);
        hazard(3, 1.0, |k| k.cursed > 0.0);
    }

    #[test]
    fn dynamite_lands_far_behind_and_goes_off_after_its_fuse() {
        let (mut world, owner, victim) = arena(-DYNAMITE_THROW);
        fire(&mut world, owner, Power::Yellow, 1);
        assert!(!ever(&mut world, DYNAMITE_FUSE - 0.5, |w| kart(w, victim).spin_out > 0.0));
        assert!(ever(&mut world, 1.0, |w| kart(w, victim).spin_out > 0.0));
    }

    #[test]
    fn turbo_and_warp_carry_the_kart_forward() {
        let (mut world, owner, _) = arena(100.0);
        fire(&mut world, owner, Power::Green, 0);
        assert!(ever(&mut world, 1.0, |w| kart(w, owner).vel.length() > crate::physics::MAX_SPEED));

        let (mut world, owner, _) = arena(100.0);
        let start = kart(&world, owner).s;
        fire(&mut world, owner, Power::Green, 3);
        ever(&mut world, WARP_TIME + 0.2, |_| false);
        let k = kart(&world, owner);
        assert!(k.s - start > 200.0, "warped {}", k.s - start);
        assert!(k.vel.length() > crate::physics::MAX_SPEED);
    }
}
