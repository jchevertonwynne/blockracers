//! The look of power-ups: the original's models and particles, put on what `items`
//! sets going and on the karts that carry shields and turbos. Without the game data
//! the plain shapes `items` spawns are left as they are.

use crate::items::Action;
use crate::items::WARP_START;
use crate::kart::{Kart, Player, Shield};
use crate::particles::{Emitter, Emitters};
use crate::physics::UNIT;
use crate::scenery::{Animated, Models, Motion};
use bevy::prelude::*;

/// A model or emitter that goes where a power-up goes, and goes away with it.
#[derive(Component)]
pub struct Dressing {
    of: Entity,
    offset: Vec3,
    last: Vec3,
    /// Points the way it is travelling.
    aimed: bool,
    /// Grows with the power-up, reaching full size when that is this big.
    sized: Option<f32>,
}

/// Materials of the power-ups whose pictures are put on things by hand.
pub const PICTURES: [&str; 9] = [
    "pbrickp", "pbrickm", "pbricks", "pbrickt", "ptrailp", "ptrailm", "ptrails", "ptrailt",
    "oilslck",
];

/// The turbo pack sits this far up and back from the car's middle.
const TURBO_PACK: Vec3 = Vec3::new(0.0, 3.0 * UNIT, 2.0 * UNIT);
/// It burns down over this long as the turbo gives out.
const TURBO_FADE: f32 = 0.7;
/// A magnet hangs this far above the road, and a curse this far.
const MAGNET_HEIGHT: f32 = 30.0 * UNIT;
const CURSE_HEIGHT: f32 = 13.0 * UNIT;
/// A warp's hole opens this far above the car.
const PORTAL_HEIGHT: f32 = 6.0 * UNIT;
/// Blasts bigger than this are the spiked kind.
const SPIKED_BLAST: f32 = 7.5 * UNIT;
/// The missile's own animation is its launch from a car; it is held at the start of
/// this part of it, by this bone.
const MISSILE_POSE: Motion = Motion::Held(2, 1);

pub fn dress_actions(
    mut commands: Commands,
    models: Option<Res<Models>>,
    emitters: Option<Res<Emitters>>,
    new: Query<(Entity, &Action, &Transform), Added<Action>>,
    actions: Query<&Transform, (With<Action>, Without<Dressing>)>,
    mut dressings: Query<(Entity, &mut Dressing, &mut Transform), Without<Action>>,
) {
    let (Some(models), Some(emitters)) = (models, emitters) else {
        return;
    };
    for (of, action, at) in &new {
        let dressing = |offset: Vec3, aimed: bool, sized: Option<f32>| Dressing {
            of,
            offset,
            last: at.translation,
            aimed,
            sized,
        };
        // What stands for it, what it gives off, how far up from where `items` has it,
        // and whether it points along its path.
        let (parts, particles, lift, aimed): (&[&str], &[&str], f32, bool) = match action {
            Action::Cannonball { .. } => (&[], &["cannsmk"], 0.0, false),
            Action::Hook { .. } => (&["grapple"], &["cannsmk"], 0.0, true),
            Action::Missile { .. } => (&["dmissil"], &[], 0.0, true),
            Action::OilSlick { .. } => (&[], &["oilbub"], 0.0, false),
            Action::Dynamite { .. } => (&["barrel"], &["dynsprk"], -0.45, false),
            Action::Magnet { .. } => (&["magnet", "magring", "insd"], &[], MAGNET_HEIGHT, false),
            Action::Curse { .. } => (&["curse", "cgreen", "cgreen2"], &[], CURSE_HEIGHT, false),
            Action::Explosion { radius, .. } if *radius > SPIKED_BLAST => {
                (&["spikexp"], &["explode"], 0.0, false)
            }
            Action::Explosion { .. } => (&["explsn"], &["explode"], 0.0, false),
            Action::Lightning { .. } => (&[], &[], 0.0, false),
        };
        let offset = Vec3::Y * lift;
        let place = Transform::from_translation(at.translation + offset);
        // `CurseAction::Activate`: only the skull hovers; its auras stand on the road.
        let grounded: &[&str] = if matches!(action, Action::Curse { .. }) {
            &["cgreen", "cgreen2"]
        } else {
            &[]
        };
        let sized = match action {
            Action::Explosion { radius, .. } => Some(*radius),
            _ => None,
        };
        let motion = if matches!(action, Action::Missile { .. }) {
            MISSILE_POSE
        } else {
            Motion::Loop
        };
        let mut dressed = false;
        for part in parts {
            let (place, offset) = if grounded.contains(part) {
                (Transform::from_translation(at.translation), Vec3::ZERO)
            } else {
                (place, offset)
            };
            if let Some(model) = models.spawn(&mut commands, part, place, motion) {
                commands
                    .entity(model)
                    .insert(dressing(offset, aimed, sized));
                dressed = true;
            }
        }
        debug!(
            "power-up at {}: {parts:?} {particles:?}, dressed {dressed}",
            at.translation
        );
        if dressed {
            commands.entity(of).remove::<Mesh3d>();
        }
        for name in particles {
            if let Some(emitter) = emitters.spawn(&mut commands, name, place) {
                commands
                    .entity(emitter)
                    .insert(dressing(offset, false, None));
            }
        }
    }

    for (entity, mut dressing, mut transform) in &mut dressings {
        let Ok(target) = actions.get(dressing.of) else {
            commands.entity(entity).despawn();
            continue;
        };
        let moved = target.translation - dressing.last;
        if dressing.aimed && moved.length_squared() > 1e-6 {
            transform.look_to(moved, Vec3::Y);
        }
        if let Some(size) = dressing.sized {
            transform.scale = target.scale / size;
        }
        dressing.last = target.translation;
        transform.translation = target.translation + dressing.offset;
    }
}

/// What a kart is wearing: the models of its shield and its turbo, and at what level.
#[derive(Component, Default)]
pub struct Worn {
    shield: Option<(u8, Vec<Entity>)>,
    turbo: Option<(u8, Vec<Entity>)>,
    /// The turbo is burning down.
    fading: bool,
    smoke: Option<Entity>,
    /// The hole a warp opens over the car, and the tunnel it then goes down.
    portal: Option<Entity>,
    tunnel: Vec<Entity>,
}

pub fn dress_karts(
    mut commands: Commands,
    models: Option<Res<Models>>,
    emitters: Option<Res<Emitters>>,
    bare: Query<Entity, (With<Kart>, Without<Worn>)>,
    stand_ins: Query<Entity, With<Shield>>,
    mut karts: Query<(Entity, &Kart, &mut Worn, Has<Player>)>,
    mut smoke: Query<(&mut Emitter, &mut Transform)>,
    mut placed: Query<&mut Transform, Without<Emitter>>,
    mut seen: Query<&mut Visibility>,
    children: Query<&Children>,
    mut animated: Query<&mut Animated>,
) {
    let (Some(models), Some(emitters)) = (models, emitters) else {
        return;
    };
    for kart in &bare {
        commands.entity(kart).insert(Worn::default());
    }
    if models.has("shield0") {
        for sphere in &stand_ins {
            commands.entity(sphere).despawn();
        }
    }
    for (entity, k, mut worn, is_player) in &mut karts {
        // Puts on, changes or takes off a set of models as the level they show changes.
        let mut wear = |slot: &mut Option<(u8, Vec<Entity>)>,
                        level: Option<u8>,
                        at: Transform,
                        motion: Motion,
                        names: &dyn Fn(u8) -> Vec<String>| {
            if slot.as_ref().map(|s| s.0) == level {
                return;
            }
            for part in slot.take().into_iter().flat_map(|s| s.1) {
                commands.entity(part).despawn();
            }
            let Some(level) = level else { return };
            let parts: Vec<Entity> = names(level)
                .iter()
                .filter_map(|name| models.spawn(&mut commands, name, at, motion))
                .collect();
            commands.entity(entity).add_children(&parts);
            *slot = Some((level, parts));
        };
        let shield = (k.shield > 0.0).then_some(k.shield_level.min(3));
        wear(
            &mut worn.shield,
            shield,
            Transform::IDENTITY,
            Motion::Loop,
            &|level| vec![format!("shield{level}"), format!("shldin{level}")],
        );
        // The turbo pack lights, burns, and burns down as the turbo gives out.
        let turbo = (k.boost > 0.0).then_some(k.boost_level.min(2));
        let lit = worn.turbo.is_some();
        // The pack's own forward is the car's left.
        let pack = Transform::from_translation(TURBO_PACK)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        wear(&mut worn.turbo, turbo, pack, Motion::Then(1), &|level| {
            vec![
                format!("turbol{level}"),
                format!("turb{level}f1"),
                format!("turb{level}f2"),
            ]
        });
        if !lit {
            worn.fading = false;
        } else if k.boost < TURBO_FADE && !worn.fading {
            worn.fading = true;
            for &part in worn.turbo.iter().flat_map(|t| &t.1) {
                models.play(part, 2, false, &children, &mut animated);
            }
        }

        // A shield flickers when about to run out.
        let on = k.shield > 1.0 || (k.shield * 10.0) as i32 % 2 == 0;
        for &part in worn.shield.iter().flat_map(|s| &s.1) {
            if let Ok(mut visibility) = seen.get_mut(part) {
                visibility.set_if_neq(if on {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        }

        // A warp: a hole opens over the car and closes on it, and then whoever is
        // watching from behind it sees the tunnel it goes down.
        match (k.warp_start > 0.0, worn.portal) {
            (true, None) => {
                let above = Transform::from_translation(k.pos + k.rot * Vec3::Y * PORTAL_HEIGHT)
                    .with_rotation(k.rot);
                worn.portal = models.spawn(
                    &mut commands,
                    "warpprt",
                    above.with_scale(Vec3::splat(0.001)),
                    Motion::Loop,
                );
            }
            (true, Some(portal)) => {
                if let Ok(mut transform) = placed.get_mut(portal) {
                    // Swelling and shrinking away again over the time it takes.
                    let size = (std::f32::consts::PI * (1.0 - k.warp_start / WARP_START)).sin();
                    transform.translation =
                        k.pos + k.rot * Vec3::Y * PORTAL_HEIGHT * (k.warp_start / WARP_START);
                    transform.scale = Vec3::splat(size.max(0.001));
                }
            }
            (false, Some(portal)) => {
                commands.entity(portal).try_despawn();
                worn.portal = None;
            }
            (false, None) => {}
        }
        let tunnelled = is_player && k.warp > 0.0;
        if tunnelled && worn.tunnel.is_empty() {
            // The tunnel's own forward is the game's -Y; the car's is ours.
            let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
            for name in ["dtube", "dbricks"] {
                let place =
                    Transform::from_translation(turn * models.placed(name)).with_rotation(turn);
                worn.tunnel
                    .extend(models.spawn(&mut commands, name, place, Motion::Loop));
            }
            commands.entity(entity).add_children(&worn.tunnel);
        } else if !tunnelled {
            for part in worn.tunnel.drain(..) {
                commands.entity(part).despawn();
            }
        }

        // Smoke from the turbo for as long as it burns.
        let exhaust = k.pos + k.rot * TURBO_PACK;
        match (k.boost > 0.0, worn.smoke) {
            (true, None) => {
                worn.smoke = emitters.spawn(
                    &mut commands,
                    "trbsmke",
                    Transform::from_translation(exhaust),
                )
            }
            (true, Some(entity)) => match smoke.get_mut(entity) {
                Ok((mut emitter, mut transform)) => {
                    (transform.translation, transform.rotation, emitter.velocity) =
                        (exhaust, k.rot, k.vel * 0.6);
                }
                Err(_) => worn.smoke = None,
            },
            (false, Some(entity)) => {
                commands.entity(entity).try_despawn();
                worn.smoke = None;
            }
            (false, None) => {}
        }
    }
}
