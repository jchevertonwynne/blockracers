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
/// Skid marks are as wide as the car's chassis says and laid in pieces at least this
/// long. A wheel's trail of them is as long as a second of its skid, a quarter of
/// that in a powerslide (`CarVisuals::UpdateSkidMarks`, with `RaceDecalManager::Trail`,
/// which keeps only its last few segments). Which wheels leave one is `marking`'s to
/// say. A wheel that stops marking while the car still skids loses its trail at once;
/// when the skid is over what is left fades for a second (`StopSkidEffects`).
const MARK_STEP: f32 = 0.25;
const MARK_LIFE: f32 = 1.0;
const MARK_LIFE_SLIDING: f32 = 0.25;
const MARK_FADE: f32 = 1.0;
/// `g_raceDecalTrailOffsetZ` and `g_raceDecalDefaultDepth`: a mark's box begins this
/// far above the wheel and is this deep.
const MARK_ABOVE: f32 = 6.0 * UNIT;
const MARK_DEPTH: f32 = 15.0 * UNIT;
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
    /// Where each wheel's skid mark has got to.
    marks: [Option<Vec3>; 4],
    /// The trails left when a skid ended: what they are drawn with, black and
    /// burning, and how long they have been fading.
    fades: Vec<([Handle<StandardMaterial>; 2], f32)>,
}

/// A piece of skid mark: how long it has lain, how long it stays, which wheel it is
/// of, and whether a turbo burnt it. One that is fading out is `Faded`.
#[derive(Component)]
pub struct Mark(f32, f32, usize, bool);

/// A piece of a trail that is fading, with how long it has been.
#[derive(Component)]
pub struct Faded(f32);

/// `CarVisuals::UpdateSkidMarks`: which wheels (front left, front right, rear left,
/// rear right) leave a mark while the car skids. In a spin all four do, under a turbo
/// the back two, and otherwise the two of the side the car is sliding towards.
fn marking(spinning: bool, turbo: bool, leftward: bool) -> [bool; 4] {
    if spinning {
        [true; 4]
    } else if turbo {
        [false, false, true, true]
    } else {
        [leftward, !leftward, leftward, !leftward]
    }
}

/// What marks and shadows are drawn with.
pub struct Looks {
    square: Handle<Mesh>,
    skid: Handle<StandardMaterial>,
    burn: Handle<StandardMaterial>,
}

impl Effects {
    fn coin(&mut self) -> bool {
        self.toss = self
            .toss
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.toss >> 16 & 1 == 1
    }
}

pub fn kart_effects(
    mut commands: Commands,
    emitters: Option<Res<Emitters>>,
    mut player: Query<(&mut Kart, &mut Effects), With<Player>>,
    mut sources: Query<(&mut Emitter, &mut Transform)>,
    time: Res<Time>,
    mut marks: Query<(Entity, &mut Mark), Without<Faded>>,
    mut faded: Query<(Entity, &mut Faded)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut looks: Local<Option<Looks>>,
    track: Option<Res<crate::track::Track>>,
) {
    for (entity, mut mark) in &mut marks {
        mark.0 += time.delta_secs();
        if mark.0 > mark.1 {
            commands.entity(entity).despawn();
        }
    }
    for (entity, mut fade) in &mut faded {
        fade.0 += time.delta_secs();
        if fade.0 >= MARK_FADE {
            commands.entity(entity).despawn();
        }
    }
    let looks = looks.get_or_insert_with(|| {
        let mut flat = |colour: Color, alpha_mode: AlphaMode| {
            materials.add(StandardMaterial {
                base_color: colour,
                unlit: true,
                alpha_mode,
                cull_mode: None,
                ..default()
            })
        };
        Looks {
            square: meshes.add(Plane3d::default().mesh().size(1.0, 1.0)),
            skid: flat(Color::srgba(0.0, 0.0, 0.0, 0.45), AlphaMode::Blend),
            // A turbo's marks burn.
            burn: flat(Color::srgba(1.0, 0.45, 0.05, 0.6), AlphaMode::Add),
        }
    });
    let (Some(emitters), Ok((mut k, mut fx))) = (emitters, player.single_mut()) else {
        return;
    };
    let fx = &mut *fx;
    let sparks = k.sparks.take();
    let k = &*k;
    let wheel = |i: usize| k.pos + k.rot * k.wheels[i];
    let carried = k.vel * CARRIED_SPEED;
    let start = |commands: &mut Commands, name: &str, at: Vec3| {
        debug!("{name} at {at}");
        emitters.spawn(
            commands,
            name,
            Transform::from_translation(at).with_rotation(k.rot),
        )
    };
    // Keeps an emitter with the car; false once it has gone.
    let mut follow = |entity: Entity, at: Vec3| {
        let Ok((mut emitter, mut transform)) = sources.get_mut(entity) else {
            return None;
        };
        (transform.translation, transform.rotation, emitter.velocity) = (at, k.rot, carried);
        Some(emitter.spawned)
    };

    // Spray from whatever the wheels are on.
    let spraying = k.contacts > 0
        && !k.sliding
        && k.spin <= 0.0
        && k.spin_out <= 0.0
        && k.vel.dot(k.rot * Vec3::NEG_Z) > SPRAY_SPEED;
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
            let text = String::from_utf8_lossy(&name)
                .trim_end_matches('\0')
                .to_lowercase();
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
    if let Some(entity) = fx.smoke
        && follow(entity, exhaust).is_none_or(|puffs| puffs >= SMOKE_PUFFS)
    {
        commands.entity(entity).try_despawn();
        fx.smoke = None;
    }

    // Tyre smoke for as long as the tyres are skidding.
    let skidding = k.contacts > 0
        && (k.sliding || k.spin > 0.0 || k.magnet > 0.0 || (boosting && k.boost_level > 0));
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

    // Trails left by skids that are over fade away.
    fx.fades.retain_mut(|(shades, age)| {
        *age += time.delta_secs();
        let left = 1.0 - *age / MARK_FADE;
        for (shade, full) in shades.iter().zip([&looks.skid, &looks.burn]) {
            let full = materials.get(full).map_or(1.0, |m| m.base_color.alpha());
            if let Some(mut shade) = materials.get_mut(shade) {
                shade.base_color.set_alpha(full * left.max(0.0));
            }
        }
        if left <= 0.0 {
            shades.iter().for_each(|shade| {
                materials.remove(shade);
            });
        }
        left > 0.0
    });
    // The skid over, every trail is let go together, to fade.
    if !skidding && fx.marks.iter().any(Option::is_some) {
        fx.marks = [None; 4];
        let shades = [&looks.skid, &looks.burn].map(|look| {
            let copy = materials.get(look).cloned().unwrap_or_default();
            materials.add(copy)
        });
        for (entity, mark) in &marks {
            // Those that have lasted their time are gone already.
            if mark.0 <= mark.1 {
                commands.entity(entity).insert((
                    Faded(0.0),
                    MeshMaterial3d(shades[mark.3 as usize].clone()),
                ));
            }
        }
        fx.fades.push((shades, 0.0));
    }
    // Marks on the road behind the wheels that skid, for as long as they do.
    let leftward = k.vel.dot(k.rot * Vec3::X) < 0.0;
    let marking = marking(k.spin > 0.0, boosting && k.boost_level > 0, leftward);
    for (wheel_index, marking) in marking.into_iter().enumerate() {
        if !skidding {
            continue;
        }
        // A powerslide's trail is a shorter one, and a trail of the one kind is
        // dropped for the other (`UpdateSkidMarks`), as is a wheel's that has
        // stopped marking.
        let life = if k.sliding {
            MARK_LIFE_SLIDING
        } else {
            MARK_LIFE
        };
        let other = marks
            .iter()
            .any(|(_, mark)| mark.2 == wheel_index && mark.1 != life);
        if !marking || other {
            fx.marks[wheel_index] = None;
            for (entity, mark) in &marks {
                if mark.2 == wheel_index && mark.0 <= mark.1 {
                    commands.entity(entity).despawn();
                }
            }
            if !marking {
                continue;
            }
        }
        let side = wheel_index;
        let up = k.rot * Vec3::Y;
        // `RaceDecalManager::Trail::AddSample`: a mark is laid on whatever of the
        // road is under the wheel, from a little above it to well below, so that a
        // car riding over the road, or floating over it, still marks it.
        let under = track.as_ref().and_then(|track| {
            track
                .collision
                .ground(wheel(wheel_index) + Vec3::Y * MARK_ABOVE, MARK_DEPTH)
        });
        let Some(under) = under else {
            fx.marks[wheel_index] = None;
            continue;
        };
        let at = under.point + under.normal * MARK_LIFT;
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
            .with_scale(Vec3::new(k.skid[wheel_index / 2], 1.0, along.length()));
        commands.spawn((
            Mark(0.0, life, side, boosting),
            Mesh3d(looks.square.clone()),
            MeshMaterial3d(material.clone()),
            piece,
        ));
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

/// The colours a car's materials were last multiplied by.
#[derive(Component)]
pub struct Tinted(Vec3);

/// Shows each car in the colours the circuit's events have given it.
pub fn tints(
    mut commands: Commands,
    karts: Query<(Entity, &Kart, Option<&Tinted>)>,
    children: Query<&Children>,
    parts: Query<
        &MeshMaterial3d<StandardMaterial>,
        (Without<Shadow>, Without<crate::kart::Shield>),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, kart, tinted) in &karts {
        if tinted.map_or(Vec3::ONE, |t| t.0) == kart.tint {
            continue;
        }
        commands.entity(entity).insert(Tinted(kart.tint));
        for part in children.iter_descendants(entity) {
            let Ok(handle) = parts.get(part) else {
                continue;
            };
            // The original's cars, which are drawn unlit; the brick-built ones share
            // their materials and are left alone.
            if let Some(mut material) = materials.get_mut(&handle.0).filter(|m| m.unlit) {
                material.base_color = Color::srgb(kart.tint.x, kart.tint.y, kart.tint.z);
            }
        }
    }
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
        let blot = Mesh::from(Circle::new(0.5))
            .rotated_by(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
        (meshes.add(blot), materials.add(material))
    });
    for (entity, kart) in &bare {
        let [width, front, rear] = kart.outline;
        let size = Vec3::new(width * 2.6, 1.0, (rear - front) * 1.25);
        let place = Transform::from_xyz(0.0, MARK_LIFT, (front + rear) / 2.0).with_scale(size);
        commands.entity(entity).with_child((
            Shadow,
            Mesh3d(blot.clone()),
            MeshMaterial3d(material.clone()),
            place,
        ));
    }
    for (child_of, mut transform, mut visibility) in &mut shadows {
        let Ok(kart) = karts.get(child_of.parent()) else {
            continue;
        };
        // In the air the shadow stays on the road below, while there is one near.
        let down = kart.rot.inverse() * Vec3::NEG_Y;
        let ground = track
            .collision
            .ground(kart.pos + Vec3::Y, 12.0)
            .map(|hit| kart.pos.y - hit.point.y);
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

#[cfg(test)]
mod tests {
    use super::marking;

    /// Front left, front right, rear left, rear right.
    #[test]
    fn the_wheels_that_mark_the_road_are_the_original_s() {
        assert_eq!(marking(true, true, false), [true; 4]);
        assert_eq!(marking(false, true, true), [false, false, true, true]);
        assert_eq!(marking(false, false, true), [true, false, true, false]);
        assert_eq!(marking(false, false, false), [false, true, false, true]);
    }
}
