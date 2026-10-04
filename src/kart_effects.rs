//! What the player's car throws up and leaves behind as it goes: spray off the surface
//! under each wheel, dust, tyre smoke and marks on the road in a skid, smoke as a turbo
//! fires, a puff on landing and sparks where cars touch. Follows `CarVisuals`; the
//! original gives none of this to the computer's cars either. Every car has its shadow.

use crate::kart::{Kart, Player};
use crate::particles::{Emitter, Emitters};
use crate::physics::UNIT;
use bevy::prelude::*;

/// Particles carry on with this much of the car's speed.
const CARRIED_SPEED: f32 = 0.6;
/// Slower than this, the surface throws nothing up.
const SPRAY_SPEED: f32 = 8.0 * UNIT;
/// Dust and turbo smoke stop after this many puffs.
const DUST_PUFFS: u32 = 10;
const SMOKE_PUFFS: u32 = 4;
/// Turbo smoke comes from this far above the back axle.
const SMOKE_HEIGHT: f32 = 2.0 * UNIT;
/// A landing counts once the car has been off the ground this long.
const AIRBORNE: f32 = 0.4;
/// Skid marks are this wide, laid in pieces at least this long, and last this long.
const MARK_WIDTH: f32 = 0.28;
const MARK_STEP: f32 = 0.25;
const MARK_LIFE: f32 = 6.0;
/// They and the shadows sit this far off the road, to be seen.
const MARK_LIFT: f32 = 0.03;

#[derive(Component, Default)]
pub struct Effects {
    /// The emitter at each wheel, and which surface's it is.
    wheels: [Option<(Entity, [u8; 8])>; 4],
    dust: Option<Entity>,
    smoke: Option<Entity>,
    tyre_smoke: Option<Entity>,
    sliding: bool,
    boosting: bool,
    airborne: bool,
    contacts: u8,
    toss: u32,
    /// Where each back wheel's skid mark has got to.
    marks: [Option<Vec3>; 2],
}

/// A piece of skid mark, and how long it has lain.
#[derive(Component)]
pub struct Mark(f32);

/// What marks and shadows are drawn with.
pub struct Looks {
    square: Handle<Mesh>,
    skid: Handle<StandardMaterial>,
    burn: Handle<StandardMaterial>,
}

impl Effects {
    fn coin(&mut self) -> bool {
        self.toss = self.toss.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.toss >> 16 & 1 == 1
    }
}

pub fn kart_effects(
    mut commands: Commands,
    emitters: Option<Res<Emitters>>,
    mut player: Query<(&mut Kart, &mut Effects), With<Player>>,
    mut sources: Query<(&mut Emitter, &mut Transform)>,
    time: Res<Time>,
    mut marks: Query<(Entity, &mut Mark)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut looks: Local<Option<Looks>>,
) {
    for (entity, mut mark) in &mut marks {
        mark.0 += time.delta_secs();
        if mark.0 > MARK_LIFE {
            commands.entity(entity).despawn();
        }
    }
    let looks = looks.get_or_insert_with(|| {
        let mut flat = |colour: Color, alpha_mode: AlphaMode| {
            materials.add(StandardMaterial { base_color: colour, unlit: true, alpha_mode, cull_mode: None, ..default() })
        };
        Looks {
            square: meshes.add(Plane3d::default().mesh().size(1.0, 1.0)),
            skid: flat(Color::srgba(0.0, 0.0, 0.0, 0.45), AlphaMode::Blend),
            // A turbo's marks burn.
            burn: flat(Color::srgba(1.0, 0.45, 0.05, 0.6), AlphaMode::Add),
        }
    });
    let (Some(emitters), Ok((mut k, mut fx))) = (emitters, player.single_mut()) else { return };
    let fx = &mut *fx;
    let sparks = k.sparks.take();
    let k = &*k;
    let wheel = |i: usize| k.pos + k.rot * k.wheels[i];
    let carried = k.vel * CARRIED_SPEED;
    let start = |commands: &mut Commands, name: &str, at: Vec3| {
        debug!("{name} at {at}");
        emitters.spawn(commands, name, Transform::from_translation(at).with_rotation(k.rot))
    };
    // Keeps an emitter with the car; false once it has gone.
    let mut follow = |entity: Entity, at: Vec3| {
        let Ok((mut emitter, mut transform)) = sources.get_mut(entity) else { return None };
        (transform.translation, transform.rotation, emitter.velocity) = (at, k.rot, carried);
        Some(emitter.spawned)
    };

    // Spray from whatever the wheels are on.
    let spraying = k.contacts > 0 && !k.sliding && k.spin <= 0.0 && k.spin_out <= 0.0 && k.vel.dot(k.rot * Vec3::NEG_Z) > SPRAY_SPEED;
    let name = k.surface.particle;
    for i in 0..4 {
        if let Some((entity, current)) = fx.wheels[i] {
            if spraying && current == name && follow(entity, wheel(i)).is_some() {
                continue;
            }
            commands.entity(entity).try_despawn();
            fx.wheels[i] = None;
        }
        if spraying && name[0] != 0 {
            let text = String::from_utf8_lossy(&name).trim_end_matches('\0').to_lowercase();
            fx.wheels[i] = start(&mut commands, &text, wheel(i)).map(|e| (e, name));
        }
    }

    // Dust as a slide or a turbo begins, unless the back wheels are spraying already.
    let boosting = k.boost > 0.0;
    let begun = (k.sliding && !fx.sliding) || (boosting && !fx.boosting);
    if begun && fx.dust.is_none() && fx.wheels[2].is_none() && fx.wheels[3].is_none() {
        fx.dust = start(&mut commands, "dust", wheel(3));
    }
    if let Some(entity) = fx.dust {
        let at = wheel(if fx.coin() { 3 } else { 2 });
        if follow(entity, at).is_none_or(|puffs| puffs >= DUST_PUFFS) {
            commands.entity(entity).try_despawn();
            fx.dust = None;
        }
    }

    // Smoke out of the back as a turbo fires.
    let exhaust = (wheel(2) + wheel(3)) * 0.5 + k.rot * Vec3::Y * SMOKE_HEIGHT;
    if boosting && !fx.boosting && fx.smoke.is_none() {
        fx.smoke = start(&mut commands, "carsmke", exhaust);
    }
    if let Some(entity) = fx.smoke {
        if follow(entity, exhaust).is_none_or(|puffs| puffs >= SMOKE_PUFFS) {
            commands.entity(entity).try_despawn();
            fx.smoke = None;
        }
    }

    // Tyre smoke for as long as the tyres are skidding.
    let skidding = k.contacts > 0 && (k.sliding || k.spin > 0.0 || k.magnet > 0.0 || (boosting && k.boost_level > 0));
    match (skidding, fx.tyre_smoke) {
        (true, None) => fx.tyre_smoke = start(&mut commands, "tiresmk", wheel(3)),
        (true, Some(entity)) => {
            let at = wheel(if fx.coin() { 3 } else { 2 });
            if follow(entity, at).is_none() {
                fx.tyre_smoke = None;
            }
        }
        (false, Some(entity)) => {
            commands.entity(entity).try_despawn();
            fx.tyre_smoke = None;
        }
        (false, None) => {}
    }

    // Marks on the road behind the back wheels for as long as they skid.
    for (side, wheel_index) in [2, 3].into_iter().enumerate() {
        if !skidding {
            fx.marks[side] = None;
            continue;
        }
        let up = k.rot * Vec3::Y;
        let at = wheel(wheel_index) + up * MARK_LIFT;
        let Some(from) = fx.marks[side] else {
            fx.marks[side] = Some(at);
            continue;
        };
        let along = at - from;
        if along.length() < MARK_STEP {
            continue;
        }
        fx.marks[side] = Some(at);
        let material = if boosting { &looks.burn } else { &looks.skid };
        let piece = Transform::from_translation(from + along / 2.0)
            .looking_to(along, up)
            .with_scale(Vec3::new(MARK_WIDTH, 1.0, along.length()));
        commands.spawn((Mark(0.0), Mesh3d(looks.square.clone()), MeshMaterial3d(material.clone()), piece));
    }

    // A puff where the car comes back down.
    fx.airborne |= k.air_time > AIRBORNE;
    if fx.airborne {
        if k.contacts > fx.contacts {
            fx.airborne = false;
            start(&mut commands, "carland", k.pos);
        }
        fx.contacts = k.contacts;
    }

    if let Some(at) = sparks {
        start(&mut commands, "carsprk", at);
    }
    (fx.sliding, fx.boosting) = (k.sliding, boosting);
}

/// The dark patch under a car.
#[derive(Component)]
pub struct Shadow;

/// Gives every car its shadow, sized to the car, and keeps each on the road under it.
pub fn shadows(
    mut commands: Commands,
    track: Res<crate::track::Track>,
    bare: Query<(Entity, &Kart), Added<Kart>>,
    karts: Query<&Kart>,
    mut shadows: Query<(&ChildOf, &mut Transform, &mut Visibility), With<Shadow>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut look: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    let (blot, material) = look.get_or_insert_with(|| {
        let material = StandardMaterial {
            base_color: Color::srgba(0.0, 0.0, 0.0, 0.4),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        };
        // A round blot, lying flat.
        let blot = Mesh::from(Circle::new(0.5)).rotated_by(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
        (meshes.add(blot), materials.add(material))
    });
    for (entity, kart) in &bare {
        let [width, front, rear] = kart.outline;
        let size = Vec3::new(width * 2.6, 1.0, (rear - front) * 1.25);
        let place = Transform::from_xyz(0.0, MARK_LIFT, (front + rear) / 2.0).with_scale(size);
        commands.entity(entity).with_child((Shadow, Mesh3d(blot.clone()), MeshMaterial3d(material.clone()), place));
    }
    for (child_of, mut transform, mut visibility) in &mut shadows {
        let Ok(kart) = karts.get(child_of.parent()) else { continue };
        // In the air the shadow stays on the road below, while there is one near.
        let down = kart.rot.inverse() * Vec3::NEG_Y;
        let ground = track.collision.ground(kart.pos + Vec3::Y, 12.0).map(|hit| kart.pos.y - hit.point.y);
        match ground.filter(|_| kart.warp <= 0.0) {
            Some(drop) => {
                transform.translation.y = MARK_LIFT - drop * down.y.abs();
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}
