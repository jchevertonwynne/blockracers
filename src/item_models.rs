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
pub const PICTURES: [&str; 10] = [
    "pbrickp", "pbrickm", "pbricks", "pbrickt", "ptrailp", "ptrailm", "ptrails", "ptrailt",
    "oilslck", SCAR,
];
/// The picture of the scar an explosion leaves on the road.
const SCAR: &str = "exscar";
/// `RacePowerupManager::CreateExplosionPools`: how wide the scar of a blast gets and
/// of a spiked one, how long it then takes to fade, and what it is drawn at until
/// it does (`g_explosionScarAlpha`); `PowerupExplosion::Spawn` has its box begin
/// this far above the blast and `g_explosionScarDepth` deep.
const SCAR_WIDTHS: (f32, f32) = (15.0 * UNIT, 5.0 * UNIT);
const SCAR_START: f32 = 0.1 * UNIT;
const SCAR_FADE: f32 = 5.0;
const SCAR_ALPHA: f32 = 180.0 / 255.0;
const SCAR_ABOVE: f32 = 5.0 * UNIT;
const SCAR_DEPTH: f32 = 15.0 * UNIT;
/// `BrickDebris`: the models thrown off a car that is shot, one after another, and
/// how many at the most are thrown at once (`c_randomBurstMax`).
const DEBRIS: [&str; 4] = ["brick1", "brick2", "brick3", "brick4"];
const DEBRIS_MOST: u32 = 3;

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

/// A scar on the road: where its blast was, how wide it gets, and how long it has
/// been there.
#[derive(Component)]
pub struct Scar {
    at: Vec3,
    width: f32,
    age: f32,
}

/// Bricks thrown off a car, gone once they have played out.
#[derive(Component)]
pub struct Debris;

/// What explosions leave behind them (`PowerupExplosion`, `BrickDebris`): a scar on
/// the road that grows as the blast does and then fades, and bricks off a car.
pub fn aftermath(
    mut commands: Commands,
    time: Res<Time>,
    models: Option<Res<Models>>,
    swatches: Option<Res<crate::scenery::Swatches>>,
    track: Option<Res<crate::track::Track>>,
    blasts: Query<(&Action, &Transform), Added<Action>>,
    mut scars: Query<(Entity, &mut Scar, &Mesh3d, &MeshMaterial3d<StandardMaterial>)>,
    debris: Query<(Entity, &Children), With<Debris>>,
    playing: Query<&Animated>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut thrown: Local<u32>,
) {
    for (action, at) in &blasts {
        let Action::Explosion {
            radius,
            scar,
            debris,
            ..
        } = action
        else {
            continue;
        };
        let picture = swatches.as_ref().and_then(|s| s.0.get(SCAR));
        debug!(
            "a blast at {}: scar {scar}, debris {debris:?}, picture {}",
            at.translation,
            picture.is_some()
        );
        if let (true, Some(picture)) = (*scar, picture) {
            let width = if *radius > SPIKED_BLAST {
                SCAR_WIDTHS.1
            } else {
                SCAR_WIDTHS.0
            };
            let material = StandardMaterial {
                base_color: Color::WHITE.with_alpha(SCAR_ALPHA),
                base_color_texture: Some(picture.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            };
            // The renderer can't hold a mesh of nothing.
            let nothing = [(Vec3::ZERO, Vec2::ZERO); 3];
            commands.spawn((
                Scar {
                    at: at.translation,
                    width,
                    age: 0.0,
                },
                Mesh3d(meshes.add(crate::hazards::laid(&nothing, Vec3::Y))),
                MeshMaterial3d(materials.add(material)),
                Transform::default(),
                bevy::camera::visibility::NoFrustumCulling,
            ));
        }
        if let (Some(back), Some(models)) = (debris, &models) {
            // `SpawnBrickDebris`: one to three of them, each a little off the way
            // back along the shot.
            *thrown = thrown.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            for _ in 0..=(*thrown >> 16) % DEBRIS_MOST {
                *thrown = thrown.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let aside = |roll: u32| (roll % 100) as f32 * 0.004 - 0.2;
                let way = (*back + Vec3::new(aside(*thrown >> 8), 0.0, aside(*thrown >> 20)))
                    .with_y(0.0)
                    .normalize_or(*back);
                let place = Transform::from_translation(at.translation).looking_to(way, Vec3::Y);
                let (model, part) = (DEBRIS[(*thrown >> 4) as usize % 4], (*thrown >> 12) as usize);
                if let Some(brick) = models.spawn(&mut commands, model, place, Motion::Once(part)) {
                    commands.entity(brick).insert(Debris);
                }
            }
        }
    }
    let dt = time.delta_secs();
    for (entity, mut scar, mesh, material) in &mut scars {
        let growing = scar.age < crate::items::EXPLOSION_TIME;
        scar.age += dt;
        if scar.age >= crate::items::EXPLOSION_TIME + SCAR_FADE {
            commands.entity(entity).despawn();
            continue;
        }
        // It is laid afresh for as long as it grows, and then lies as it is and fades.
        if let (true, Some(track), Some(mut mesh)) = (growing, &track, meshes.get_mut(&mesh.0)) {
            let grown = crate::items::blast_growth(scar.age.min(crate::items::EXPLOSION_TIME));
            let wide = SCAR_START + (scar.width - SCAR_START) * grown;
            let lies = track.collision.decal(
                scar.at + Vec3::Y * SCAR_ABOVE,
                Vec3::NEG_Y,
                Vec3::X,
                [wide, wide, SCAR_DEPTH],
            );
            if !lies.is_empty() {
                *mesh = crate::hazards::laid(&lies, Vec3::Y);
            }
        } else if let Some(mut material) = materials.get_mut(&material.0) {
            let left = 1.0 - (scar.age - crate::items::EXPLOSION_TIME) / SCAR_FADE;
            material.base_color.set_alpha(SCAR_ALPHA * left.clamp(0.0, 1.0));
        }
    }
    for (entity, children) in &debris {
        let over = children
            .iter()
            .filter_map(|child| playing.get(child).ok())
            .all(Animated::done);
        if over {
            commands.entity(entity).despawn();
        }
    }
}

/// The emitter of the puff a hook leaves where it lets go (`world::hook_puff`).
pub const HOOK_PUFF: &str = "hookpuff";

/// A hook whose puff has been shown; the puff's emitter, while it has still to
/// throw its one picture out, with the hook it is of; and then the picture.
#[derive(Component)]
pub struct Puffed;
#[derive(Component)]
pub struct Puff(Entity);

/// `GrapplingHookAction::ReleaseHook`: a puff where the hook let go, once.
pub fn hook_puffs(
    mut commands: Commands,
    emitters: Option<Res<Emitters>>,
    hooks: Query<(Entity, &Action, &Transform), Without<Puffed>>,
    puffs: Query<(Entity, &Emitter, &Puff)>,
    pictures: Query<(Entity, &crate::particles::From)>,
    shown: Query<(Entity, &Puff), Without<Emitter>>,
    all: Query<(), With<Action>>,
) {
    let Some(emitters) = emitters else { return };
    for (hook, action, at) in &hooks {
        if matches!(action, Action::Hook { released: true, .. }) {
            commands.entity(hook).insert(Puffed);
            let place = Transform::from_translation(at.translation);
            let puff = emitters.spawn(&mut commands, HOOK_PUFF, place);
            debug!("a hook lets go at {}, with a puff: {}", at.translation, puff.is_some());
            if let Some(puff) = puff {
                commands.entity(puff).insert(Puff(hook));
            }
        }
    }
    for (puff, emitter, of) in &puffs {
        if emitter.spawned > 0 {
            for (picture, _) in pictures.iter().filter(|picture| picture.1.0 == puff) {
                commands.entity(picture).insert(Puff(of.0));
            }
            commands.entity(puff).despawn();
        }
    }
    // `GrapplingHookAction::Draw`: the puff is there for as long as the rope is
    // being wound in, and no longer.
    for (picture, of) in &shown {
        if !all.contains(of.0) {
            commands.entity(picture).despawn();
        }
    }
}

pub fn dress_actions(
    mut commands: Commands,
    models: Option<Res<Models>>,
    emitters: Option<Res<Emitters>>,
    new: Query<(Entity, &Action, &Transform), Added<Action>>,
    actions: Query<(&Transform, &Action), Without<Dressing>>,
    mut dressings: Query<
        (Entity, &mut Dressing, &mut Transform, Option<&mut Visibility>),
        Without<Action>,
    >,
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

    for (entity, mut dressing, mut transform, seen) in &mut dressings {
        let Ok((target, action)) = actions.get(dressing.of) else {
            commands.entity(entity).despawn();
            continue;
        };
        // `GrapplingHookAction::Draw`: a hook that has let go is its rope, and the
        // puff where it let go (`hook_puffs`).
        if matches!(action, Action::Hook { released: true, .. }) {
            // Its model is hidden; its smoke, which has nothing to hide, stops.
            match seen {
                Some(mut seen) => {
                    seen.set_if_neq(Visibility::Hidden);
                }
                None => {
                    commands.entity(entity).despawn();
                    continue;
                }
            }
        }
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

/// An emitter has no `Visibility`, and must still go when its power-up does.
#[cfg(test)]
#[test]
fn an_emitter_goes_with_its_power_up() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = World::new();
    world.init_resource::<Models>();
    world.init_resource::<Emitters>();
    let of = world.spawn_empty().id();
    world.despawn(of);
    let dressing = || Dressing {
        of,
        offset: Vec3::ZERO,
        last: Vec3::ZERO,
        aimed: false,
        sized: None,
    };
    let emitter = world.spawn((dressing(), Transform::default())).id();
    let model = world
        .spawn((dressing(), Transform::default(), Visibility::default()))
        .id();
    world.run_system_once(dress_actions).unwrap();
    assert!(world.get_entity(emitter).is_err() && world.get_entity(model).is_err());
}
