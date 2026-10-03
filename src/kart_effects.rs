//! What the player's car throws up as it goes: spray off the surface under each wheel,
//! dust and tyre smoke in a skid, smoke as a turbo fires, a puff on landing and sparks
//! where cars touch. Follows `CarVisuals`; the original gives none of this to the
//! computer's cars either.

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
) {
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
