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
//! Behaviour and numbers follow the original's power-up actions. What is spawned here
//! is a plain shape; `item_models` puts the original's models and particles on it.
//!
//! The port's own: under the random rule (`menu::RANDOM_BRICKS`) a coloured brick is
//! any colour each time it appears.

use crate::audio::{Emitter, Sfx, id};
use crate::events::TrackEvents;
use crate::kart::{Controls, Kart};
use crate::meshgen::*;
use crate::physics::UNIT;
use crate::scenery::{Models, Motion, Swatches};
use crate::track::Track;
use crate::world::LoadedWorld;
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
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
/// A coloured brick that has been taken is back after this long (`PickupBrick`).
const BRICK_RESPAWN: f32 = 3.0;
/// A brick grows into place over this long, a little too big at first, and shrinks
/// away over this long when it is taken.
const BRICK_APPEAR: f32 = 0.5;
const BRICK_OVERSHOOT: f32 = 0.4;
const BRICK_SHRINK: f32 = 0.25;
/// A white brick knocked out of a car lies where it fell this long, then goes home.
const BRICK_DROPPED: f32 = 10.0;
/// It is put this far above the ground under the car, found within this reach.
const BRICK_DROP_HEIGHT: f32 = 5.0 * UNIT;
const BRICK_DROP_PROBE: f32 = 25.0 * UNIT;
/// How close a kart has to come to a brick to collect it.
const PICKUP_RADIUS: f32 = 2.7;
/// Bricks float this far above the road.
pub const BRICK_HEIGHT: f32 = 1.1;
const BRICK_SCALE: f32 = 0.8;

const SHIELD_TIMES: [f32; 4] = [4.0, 6.0, 8.0, 10.0];
/// Shields of this level and up send cannon balls back where they came from.
const DEFLECTING_SHIELD: u8 = 2;
/// A turbo lights for 0.4 s, burns for 1, 1.5 or 5 s and dies away over 0.7 s; the car
/// is boosted for all of it (`TurboAction`).
pub const TURBO_TIMES: [f32; 3] = [0.4 + 1.0 + 0.7, 0.4 + 1.5 + 0.7, 0.4 + 5.0 + 0.7];
pub const WARP_TIME: f32 = 1.5;
/// The warp takes this long to open before it carries the kart off.
pub const WARP_START: f32 = 0.5;

// Aiming: cars and target points between two distances, inside a cone ahead
// (`racepowerupmanagerglobals.cpp`): least distance, greatest, and the cone's cosine.
const AIM_RACER: (f32, f32, f32) = (10.0 * UNIT, 400.0 * UNIT, 0.9);
const AIM_POINT: (f32, f32, f32) = (10.0 * UNIT, 400.0 * UNIT, 0.95);
const HOOK_FAR: (f32, f32, f32) = (10.0 * UNIT, 250.0 * UNIT, 0.93);
const HOOK_WIDE: (f32, f32, f32) = (10.0 * UNIT, 400.0 * UNIT, 0.6);
// The original's cosine is 0.7071 as written, not the square root itself.
#[allow(clippy::approx_constant)]
const MISSILE_AIM: (f32, f32, f32) = (10.0 * UNIT, 400.0 * UNIT, 0.7071);

const LAUNCH_HEIGHT: f32 = 5.0 * UNIT;
/// Shots are aimed this far above a car's wheels.
const TARGET_HEIGHT: f32 = 5.0 * UNIT;
const CANNONBALL_SPEED: f32 = 180.0 * UNIT;
const CANNONBALL_GRAVITY: f32 = 32.176 * UNIT;
const CANNONBALL_RANGE: f32 = 500.0 * UNIT;
const CANNONBALL_LIFE: f32 = 3.0;
/// A cannon ball fired by the circuit lasts this long (`LauncherHazard`).
const EMPLACED_LIFE: f32 = 3.0;
const CANNONBALL_BLAST: f32 = 5.0 * UNIT;
const HOOK_SPEED: f32 = 320.0 * UNIT;
const HOOK_GRAVITY: f32 = 90.176 * UNIT;
const HOOK_RANGE: f32 = 500.0 * UNIT;
const HOOK_FLIGHT_TIME: f32 = 3.0;
const HOOK_PULL_TIME: f32 = 4.0;
/// Acceleration on both ends of the rope.
const HOOK_PULL: f32 = 180.0 * UNIT;
const HOOK_RELEASE_DISTANCE: f32 = 12.0 * UNIT;
const LIGHTNING_TIME: f32 = 7.0;
const LIGHTNING_RANGE: f32 = 50.0 * UNIT;
const LIGHTNING_MIN_RANGE: f32 = 3.0 * UNIT;
const LIGHTNING_CONE: f32 = 0.5;
/// The bolt stays on the car it has struck this long before it can strike another.
const LIGHTNING_SHOCK: f32 = 1.0;
const MISSILES: usize = 3;
const MISSILE_SPEED: f32 = 170.0 * UNIT;
const MISSILE_FLIGHT_TIME: f32 = 5.5;
const MISSILE_HEIGHT: f32 = 4.0 * UNIT;
/// With nothing to chase, the outer two missiles are aimed this far to either side,
/// which only tells in their speed.
const MISSILE_SPREAD: f32 = 150.0 * UNIT;
const MISSILE_RANGE: f32 = 500.0 * UNIT;
/// Within this distance the missile turns on its target, and this close it has hit.
const MISSILE_SNAP_DISTANCE: f32 = 60.0 * UNIT;
const MISSILE_HIT_DISTANCE: f32 = 3.0 * UNIT;
/// It looks for a target this often.
const MISSILE_LOOK: f32 = 1.0;
/// It flies this far above the road, coming down no faster than this, and makes for
/// checkpoints no farther off than this.
const MISSILE_CLEARANCE: f32 = 6.0 * UNIT;
const MISSILE_DESCENT: f32 = 6.0 * UNIT;
const MISSILE_GATE_REACH: f32 = 300.0 * UNIT;
/// It corkscrews: out to this far, growing at this rate, turning this fast.
const MISSILE_SPIRAL: (f32, f32, f32) = (4.0 * UNIT, 6.0 * UNIT, 10.0);
const MISSILE_SPIN_TURNS: f32 = 2.0;
const BIG_BLAST: f32 = 10.0 * UNIT;

const OIL_TIME: f32 = 10.0;
const OIL_SPIN_TURNS: f32 = 1.0;
const DYNAMITE_THROW: f32 = 90.0 * UNIT;
const DYNAMITE_SPEED: f32 = 40.0 * UNIT;
const DYNAMITE_FLIGHT_TIME: f32 = 3.0;
const DYNAMITE_TUMBLE: f32 = 12.0;
const DYNAMITE_BLASTS: u8 = 3;
const DYNAMITE_BLAST_INTERVAL: f32 = 0.5;
/// The second and third blasts are up to this many units off, each way.
const DYNAMITE_SCATTER: u32 = 6;
const MAGNET_ARMED_TIME: f32 = 20.0;
const MAGNET_HOLD_TIME: f32 = 4.0;
const MAGNET_FADE: f32 = 1.0;
/// A car this close to the spot under the magnet is held; others in reach are drawn in
/// with this acceleration.
const MAGNET_GRAB: f32 = 9.0 * UNIT;
const MAGNET_PULL: f32 = 1000.0 / 4500.0 * 1000.0 * UNIT;
/// A held car counts as stopped below this speed.
const MAGNET_STOPPED: f32 = 2.0 * UNIT;
/// How long a car goes on braking after the magnet last had hold of it.
const MAGNET_HELD: f32 = 0.1;
const CURSE_ARMED_TIME: f32 = 15.0;
const CURSE_TIME: f32 = 10.0;
/// Magnets and curses catch racers who come this close.
const TRAP_RADIUS: f32 = 10.0 * UNIT;
/// What is dropped is put this far above the ground under the car, found within this reach.
const DROP_HEIGHT: f32 = 1.0 * UNIT;
const DROP_PROBE: f32 = 50.0 * UNIT;
/// How close to a kart's middle counts as touching it.
const KART_RADIUS: f32 = 1.5;
/// An explosion lasts this long, growing from this part of its size, and from a ball
/// this big.
const EXPLOSION_TIME: f32 = 1.0;
const BLAST_START: f32 = 0.05;
const BLAST_CORE: f32 = 0.1 * UNIT;

/// Bricks are heard from this far off, in the original's units.
const BRICK_SOUND_RANGE: (f32, f32) = (30.0, 150.0);
/// The hum of a shot in flight carries this far; only the one nearest the player is heard.
const FLIGHT_SOUND_RANGE: (f32, f32) = (200.0, 500.0);
/// The lightning wand's hum drops by this much as it gives out, over this long.
const LIGHTNING_FADE: (f32, f32) = (0.1, 0.5);
/// It crackles this often (a minimum plus up to this much more), somewhere along its reach.
const LIGHTNING_CRACKLE: (f32, f32) = (0.2, 0.3);
/// A curse lying in wait hovers this far above the road.
const CURSE_HEIGHT: f32 = 13.0 * UNIT;

/// Loops of which only the one nearest the player sounds.
mod flight {
    pub const CANNONBALL: u16 = 100;
    pub const MISSILE: u16 = 101;
    pub const HOOK: u16 = 102;
    pub const HOOK_PULL: u16 = 103;
}

/// What a brick is doing: `PickupBrick`'s states.
#[derive(Clone, Copy, PartialEq)]
enum BrickState {
    /// There to be taken.
    Idle,
    /// Gone. A coloured brick comes back by itself; a white one when its car lets it go.
    Wait,
    /// Growing into place.
    Appear,
    /// Shrinking away, into `Pickup::next`.
    Shrink,
}

/// A brick, as the original's `ColorBrick` and `WhiteBrick`.
#[derive(Component, Clone)]
pub struct Pickup {
    /// The colour it is now; `None` is a white brick.
    power: Option<Power>,
    /// The colour it was put on the circuit as, and goes back to.
    home: Option<Power>,
    /// What it turns into once it has shrunk away.
    next_power: Option<Power>,
    next: BrickState,
    state: BrickState,
    /// Seconds in this state.
    timer: f32,
    pos: Vec3,
    /// Where a white brick belongs.
    home_pos: Vec3,
    /// How far above `pos` it is shown.
    lift: f32,
    /// Shown as the original's model, which turns by itself.
    modelled: bool,
    /// A car is on it, and was on it the frame before.
    touched: bool,
    was_touched: bool,
    /// The car carrying a white brick.
    holder: Option<Entity>,
    /// How long a white brick has lain where it was dropped.
    dropped: Option<f32>,
    /// A white brick shrinking away to go home.
    going_home: bool,
    /// The port's random bricks: a coloured brick that comes back as any colour.
    random: bool,
}

impl Pickup {
    fn new(power: Option<Power>, pos: Vec3, lift: f32, modelled: bool) -> Self {
        Pickup {
            power,
            home: power,
            next_power: power,
            next: BrickState::Wait,
            // Bricks grow into place as the circuit is loaded.
            state: BrickState::Appear,
            timer: 0.0,
            pos,
            home_pos: pos,
            lift,
            modelled,
            touched: false,
            was_touched: false,
            holder: None,
            dropped: None,
            going_home: false,
            random: false,
        }
    }

    /// `PickupBrick::Update`'s size for each state, as a part of full size.
    fn size(&self) -> f32 {
        match self.state {
            BrickState::Idle => 1.0,
            BrickState::Wait => 0.0,
            BrickState::Appear if self.timer < BRICK_OVERSHOOT => self.timer / BRICK_OVERSHOOT / BRICK_SCALE,
            BrickState::Appear => {
                let settling = ((self.timer - BRICK_OVERSHOOT) / (BRICK_APPEAR - BRICK_OVERSHOOT)).min(1.0);
                (1.0 - (1.0 - BRICK_SCALE) * settling) / BRICK_SCALE
            }
            BrickState::Shrink => (1.0 - self.timer / BRICK_SHRINK).max(0.0),
        }
    }

    /// `DroppableBrick::ReturnHome`: one lying on the road shrinks away first; one
    /// being carried is simply back.
    fn go_home(&mut self) {
        (self.holder, self.dropped) = (None, None);
        if self.state == BrickState::Idle {
            (self.going_home, self.state, self.timer) = (true, BrickState::Shrink, 0.0);
        } else {
            (self.pos, self.state, self.timer) = (self.home_pos, BrickState::Appear, 0.0);
        }
    }
}

/// Something a power-up has put into the world. Its position is its `Transform`.
#[derive(Clone, Copy, PartialEq)]
pub enum MagnetState {
    Armed,
    Holding,
    Fade,
}

#[derive(Component, Clone)]
pub enum Action {
    /// `on_hit` is an event of the circuit's to set off where it lands.
    Cannonball { owner: Entity, shot: Shot, on_hit: Option<i32> },
    /// Flying until `pulling` is set, then reeling owner and victim together.
    Hook { owner: Entity, shot: Shot, time: f32, pulling: Option<Entity> },
    /// `shocked` is the car it has struck, and how long ago.
    Lightning { owner: Entity, time: f32, crackle: f32, shocked: Option<(Entity, f32)> },
    /// `HomingProjectile`: where it is before it is set spiralling, the way it is
    /// going, its speed over the road and when it turns on its target, the place on
    /// the road it is making for and the checkpoint that place came from, and how
    /// long since it last looked for a target.
    Missile {
        owner: Entity,
        target: Option<Entity>,
        at: Vec3,
        heading: Vec3,
        speed: f32,
        dash: f32,
        waypoint: Option<Vec3>,
        gate: Option<usize>,
        looked: f32,
        spiral: (f32, f32),
        time: f32,
    },
    OilSlick { owner: Entity, age: f32 },
    /// In the air while it has a `shot`; after that, `wait` until the next blast.
    Dynamite { owner: Entity, shot: Option<Shot>, blasts: u8, wait: f32 },
    /// `time` is what is left of its state. `held` is the car under it, and `stopped`
    /// whether that car has come to rest.
    Magnet { owner: Entity, time: f32, state: MagnetState, held: Option<Entity>, stopped: bool },
    Curse { owner: Entity, age: f32 },
    /// `owner` is whose weapon it was; it does them no harm.
    Explosion { age: f32, radius: f32, owner: Option<Entity> },
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
    /// The plain brick, and what it is made of: the four colours, then white.
    brick: Handle<Mesh>,
    bricks: [Handle<StandardMaterial>; 5],
}

const POWERS: [Power; 4] = [Power::Red, Power::Yellow, Power::Blue, Power::Green];

fn any_colour(rng: &mut crate::meshgen::Rng) -> Power {
    POWERS[(rng.f() * POWERS.len() as f32) as usize % POWERS.len()]
}

impl ItemAssets {
    fn brick_material(&self, power: Option<Power>) -> Handle<StandardMaterial> {
        self.bricks[power.map_or(4, |p| POWERS.iter().position(|&q| q == p).unwrap())].clone()
    }

    /// Puts a brick into the world: the original's brick and the glow around it, or a
    /// plain brick.
    fn spawn_brick(&self, commands: &mut Commands, models: Option<&Models>, mut pickup: Pickup) {
        let names = match pickup.power {
            Some(Power::Red) => ["gen-p", "genblen-p"],
            Some(Power::Yellow) => ["gen-m", "genblen-m"],
            Some(Power::Blue) => ["gen-s", "genblen-s"],
            Some(Power::Green) => ["gen-t", "genblen-t"],
            None => ["enh", "enhblen"],
        };
        debug!("brick {:?} at {}", pickup.power, pickup.pos);
        let at = Transform::from_translation(pickup.pos + Vec3::Y * pickup.lift).with_scale(Vec3::splat(BRICK_SCALE * pickup.size()));
        let model = models.and_then(|models| {
            let brick = models.spawn(commands, names[0], at, Motion::Loop)?;
            if let Some(glow) = models.spawn(commands, names[1], Transform::IDENTITY, Motion::Loop) {
                commands.entity(brick).add_child(glow);
            }
            Some(brick)
        });
        pickup.modelled = model.is_some();
        match model {
            Some(brick) => {
                commands.entity(brick).insert(pickup);
            }
            None => {
                let material = self.brick_material(pickup.power);
                commands.spawn((pickup, Mesh3d(self.brick.clone()), MeshMaterial3d(material), at));
            }
        }
    }
}

pub fn setup_items(
    mut commands: Commands,
    track: Res<Track>,
    settings: Res<crate::menu::Settings>,
    mut rng: ResMut<crate::meshgen::Rng>,
    loaded: Option<Res<LoadedWorld>>,
    models: Option<Res<Models>>,
    swatches: Option<Res<Swatches>>,
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
        brick: Handle::default(),
        bricks: default(),
    };
    let mut assets = assets;
    let mut brick = BrickMesh::default();
    brick.brick(Vec3::ZERO, Vec3::new(0.55, 0.33, 0.55), Quat::IDENTITY, Color::WHITE, (2, 2));
    assets.brick = meshes.add(brick.build());
    assets.bricks = [Power::Red.color(), Power::Yellow.color(), Power::Blue.color(), Power::Green.color(), WHITE].map(|color| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 0.4,
            perceptual_roughness: 0.3,
            ..default()
        })
    });
    if let Some(mut fire) = materials.get_mut(&assets.fire) {
        fire.alpha_mode = AlphaMode::Blend;
    }
    // The oil slick's own picture, where there is one.
    if let (Some(picture), Some(mut oil)) = (swatches.and_then(|s| s.0.get("oilslck").cloned()), materials.get_mut(&assets.oil)) {
        *oil = StandardMaterial {
            base_color_texture: Some(picture),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        };
    }
    commands.insert_resource(Targets(loaded.as_ref().map(|loaded| loaded.targets.clone()).unwrap_or_default()));
    let rule = settings.brick_rule();
    let placed = loaded.is_some();
    let models = models.as_deref();
    let mut spawn = |pos: Vec3, power: Option<Power>| {
        // The race's rule may recolour the brick, or leave it out.
        let Some(power) = crate::rules::brick(rule, power) else { return };
        // The original's own placements are where the brick is drawn (`PickupBrick::Draw`);
        // ours are on the road, and the brick is held above it.
        let lift = if placed { 0.0 } else { BRICK_HEIGHT };
        // The port's random bricks begin as any colour, too.
        let random = rule == crate::menu::RANDOM_BRICKS && power.is_some();
        let power = if random { Some(any_colour(&mut rng)) } else { power };
        assets.spawn_brick(&mut commands, models, Pickup { random, ..Pickup::new(power, pos, lift, false) });
    };
    let powers = POWERS;

    // Circuits from the original game come with their own brick placements.
    if let Some(loaded) = loaded {
        for &(power, pos) in &loaded.bricks {
            spawn(pos, power);
        }
        commands.insert_resource(assets);
        return;
    }

    // Otherwise: rows of bricks at regular stations around the lap, alternating
    // coloured and white, each row spread over the width of the road.
    const STATIONS: usize = 10;
    for station in 1..STATIONS {
        let s = track.length * station as f32 / STATIONS as f32;
        if station % 2 == 1 {
            for (i, lat) in [-0.75, -0.25, 0.25, 0.75].into_iter().enumerate() {
                spawn(track.point(s, lat * track.road), Some(powers[(i + station / 2) % 4]));
            }
        } else {
            for lat in [-0.65, 0.0, 0.65] {
                spawn(track.point(s, lat * track.road), None);
            }
        }
    }
    commands.insert_resource(assets);
}

pub fn pickups(
    mut commands: Commands,
    time: Res<Time>,
    race: Res<crate::Race>,
    track: Res<Track>,
    assets: Res<ItemAssets>,
    models: Option<Res<Models>>,
    mut rng: ResMut<crate::meshgen::Rng>,
    mut sfx: ResMut<Sfx>,
    mut picks: Query<(Entity, &mut Pickup, &mut Transform, &mut Visibility)>,
    mut karts: Query<(Entity, &mut Kart)>,
) {
    let (t, dt) = (time.elapsed_secs(), time.delta_secs());
    // Bricks are only heard once the race is on (`SetBricksAudible`); before it they
    // are all put back as they began (`ColorBrick::Respawn`, `WhiteBrick::Respawn`).
    let audible = matches!(race.phase, crate::Phase::Racing | crate::Phase::Finished);

    // White bricks leave their cars: knocked out by a hit, they fall where the car is;
    // spent on a power-up, they go home.
    let mut owed: Vec<(Entity, u8, u8, Vec3)> = karts.iter_mut().map(|(e, mut k)| (e, k.whites, std::mem::take(&mut k.white_drops), k.pos)).collect();
    for (_, mut p, ..) in &mut picks {
        let Some(owner) = p.holder.and_then(|holder| owed.iter_mut().find(|o| o.0 == holder)) else {
            if p.holder.take().is_some() {
                p.go_home();
            }
            continue;
        };
        if owner.2 > 0 {
            // `DroppableBrick::DropAt`.
            owner.2 -= 1;
            let ground = track.collision.ground(owner.3 + Vec3::Y * BRICK_DROP_PROBE, BRICK_DROP_PROBE * 2.0);
            p.pos = ground.map_or(owner.3, |hit| hit.point) + Vec3::Y * (BRICK_DROP_HEIGHT - p.lift);
            (p.holder, p.dropped, p.state, p.timer) = (None, Some(0.0), BrickState::Appear, 0.0);
        } else if owner.1 == 0 {
            p.go_home();
        } else {
            owner.1 -= 1;
        }
    }

    for (entity, mut p, mut tf, mut vis) in &mut picks {
        if !audible && (p.state == BrickState::Wait || (p.power != p.home && !p.random) || p.pos != p.home_pos) {
            // A random brick is put back as the colour it has.
            let colour = if p.random { p.power } else { p.home };
            let fresh = Pickup { random: p.random, ..Pickup::new(colour, p.home_pos, p.lift, p.modelled) };
            if p.power != p.home && !p.random {
                commands.entity(entity).despawn();
                assets.spawn_brick(&mut commands, models.as_deref(), fresh);
                continue;
            }
            *p = fresh;
        }
        p.timer += dt;
        if let Some(lying) = &mut p.dropped {
            *lying += dt;
            if *lying > BRICK_DROPPED {
                p.go_home();
            }
        }
        let mut formed = false;
        match p.state {
            BrickState::Shrink if p.timer > BRICK_SHRINK => {
                (p.state, p.timer) = (p.next, 0.0);
                if std::mem::take(&mut p.going_home) {
                    (p.pos, p.state) = (p.home_pos, BrickState::Appear);
                }
                formed = p.state == BrickState::Appear && p.power.is_some();
                if p.next_power != p.power {
                    // Another colour is another model.
                    p.power = p.next_power;
                    if p.modelled {
                        commands.entity(entity).despawn();
                        assets.spawn_brick(&mut commands, models.as_deref(), p.clone());
                    } else {
                        commands.entity(entity).insert(MeshMaterial3d(assets.brick_material(p.power)));
                    }
                }
            }
            BrickState::Appear if p.timer > BRICK_APPEAR => (p.state, p.timer) = (BrickState::Idle, 0.0),
            BrickState::Wait if p.power.is_some() && p.timer >= BRICK_RESPAWN => {
                (p.state, p.timer, formed) = (BrickState::Appear, 0.0, true);
            }
            _ => {}
        }
        if formed && audible {
            sfx.emit(id::BRICK_RESPAWN, brick_sound(p.pos));
        }

        vis.set_if_neq(if p.state == BrickState::Wait { Visibility::Hidden } else { Visibility::Inherited });
        tf.scale = Vec3::splat(BRICK_SCALE * p.size()).max(Vec3::splat(0.001));
        tf.translation = p.pos + Vec3::Y * p.lift;
        if !p.modelled {
            tf.rotation = Quat::from_rotation_y(t * 2.0);
            tf.translation.y += (t * 3.0 + p.pos.x).sin() * 0.15;
        }

        // `PickupBrick::OnEvent`: a car on the brick takes it if it is there to be
        // taken, but one already holding a colour only as it first comes onto it.
        (p.was_touched, p.touched) = (p.touched, false);
        for (kart, mut k) in &mut karts {
            if k.pos.distance_squared(p.pos) > PICKUP_RADIUS * PICKUP_RADIUS {
                continue;
            }
            p.touched = true;
            if p.state != BrickState::Idle || k.warp > 0.0 || (p.was_touched && k.held.is_some()) {
                continue;
            }
            match p.power {
                // `ColorBrick::OnTouched`: the brick is gone for a while, or, taken by
                // a car that had a colour already, becomes that colour at once.
                Some(power) => {
                    let had = k.held.replace(power);
                    k.collected = true;
                    (p.next_power, p.next) = match had {
                        Some(old) => (Some(old), BrickState::Appear),
                        None if p.random => (Some(any_colour(&mut rng)), BrickState::Wait),
                        None => (p.home, BrickState::Wait),
                    };
                    if audible {
                        sfx.emit(if had.is_some() { id::BRICK_SWAP } else { id::BRICK_COLLECT }, brick_sound(p.pos));
                    }
                }
                // `WhiteBrick::OnTouched`: carried until it is used or knocked out.
                None if k.whites < MAX_WHITE_BRICKS => {
                    sfx.play(id::WHITE_BRICK + k.whites as usize);
                    k.whites += 1;
                    if k.whites == MAX_WHITE_BRICKS {
                        k.cues.reaction = Some(true);
                    }
                    (p.holder, p.dropped, p.next) = (Some(kart), None, BrickState::Wait);
                }
                // Already carrying all the white bricks there's room for.
                None => continue,
            }
            (p.state, p.timer) = (BrickState::Shrink, 0.0);
        }
    }
}

fn brick_sound(at: Vec3) -> Emitter {
    Emitter::at(at).range(BRICK_SOUND_RANGE.0, BRICK_SOUND_RANGE.1)
}

/// A car as the cone searches see it. They go through the racers in the original's
/// order: the player first, then the computer's cars.
#[derive(Clone, Copy)]
struct Seen {
    entity: Entity,
    pos: Vec3,
    vel: Vec3,
}

fn seen<'a>(karts: impl Iterator<Item = (Entity, &'a Kart)>) -> Vec<Seen> {
    let mut cars: Vec<(usize, Seen)> = karts
        .filter(|(_, k)| k.out.is_none())
        .map(|(entity, k)| ((k.slot + 1) % (crate::kart::PLAYER_SLOT + 1), Seen { entity, pos: k.pos, vel: k.vel }))
        .collect();
    cars.sort_by_key(|car| car.0);
    cars.into_iter().map(|car| car.1).collect()
}

/// The square of how far off a point is, if it is between two distances and inside a
/// cone around `forward`.
fn in_cone(at: Vec3, from: Vec3, forward: Vec3, (min, max, cone): (f32, f32, f32)) -> Option<f32> {
    let to = at - from;
    let distance = to.length_squared();
    ((min * min..=max * max).contains(&distance) && to.dot(forward) >= cone * distance.sqrt()).then_some(distance)
}

/// `RaceState::FindNearestRacerInCone`. The car asking is too close to find itself.
fn nearest(cars: &[Seen], from: Vec3, forward: Vec3, cone: (f32, f32, f32)) -> Option<Seen> {
    let found = cars.iter().filter_map(|car| Some((in_cone(car.pos, from, forward, cone)?, *car)));
    found.min_by(|a, b| a.0.total_cmp(&b.0)).map(|found| found.1)
}

/// `RaceState::FindFarthestRacerInCone`.
fn farthest(cars: &[Seen], from: Vec3, forward: Vec3, cone: (f32, f32, f32)) -> Option<Seen> {
    let found = cars.iter().filter_map(|car| Some((in_cone(car.pos, from, forward, cone)?, *car)));
    found.max_by(|a, b| a.0.total_cmp(&b.0)).map(|found| found.1)
}

/// `RaceState::FindRacerInCone` and as many `FindNextRacerInCone` after it: the cars
/// in the cone, in the order of the racers.
fn each_in_cone<'a>(cars: &'a [Seen], from: Vec3, forward: Vec3, cone: (f32, f32, f32)) -> impl Iterator<Item = Seen> + 'a {
    cars.iter().filter(move |car| in_cone(car.pos, from, forward, cone).is_some()).copied()
}

/// The circuit's target points: what the player's shots lock onto when one is ahead
/// (`TargetPointList`). Each is put out of use by the event it is numbered for.
#[derive(Resource, Default)]
pub struct Targets(pub Vec<(Vec3, i32)>);

impl Targets {
    /// `TargetPointList::FindTargetInCone`.
    fn find(&self, spent: &[i32], from: Vec3, forward: Vec3, cone: (f32, f32, f32)) -> Option<Vec3> {
        let live = self.0.iter().filter(|target| !spent.contains(&target.1));
        let found = live.filter_map(|target| Some((in_cone(target.0, from, forward, cone)?, target.0)));
        found.min_by(|a, b| a.0.total_cmp(&b.0)).map(|found| found.1)
    }
}

/// Something thrown or fired, on its way: `PowerupProjectile`. Where it is follows
/// from where it began and how long it has been going.
#[derive(Clone, Copy)]
pub struct Shot {
    from: Vec3,
    vel: Vec3,
    gravity: f32,
    /// Its speed over the ground.
    speed: f32,
    age: f32,
    life: f32,
}

/// What a shot can come to.
enum Flight {
    Flying,
    HitWorld(Vec3, Option<i32>),
    Expired,
}

impl Shot {
    /// `ComputeTrajectory`: there in `duration` seconds, arcing under gravity.
    fn timed(from: Vec3, to: Vec3, duration: f32, gravity: f32, life: f32) -> Shot {
        let duration = duration.max(1e-3);
        let delta = to - from;
        let vel = delta / duration + Vec3::Y * 0.5 * gravity * duration;
        Shot { from, vel, gravity, speed: delta.with_y(0.0).length() / duration, age: 0.0, life }
    }

    /// `LaunchAtPosition`: at a place, at a speed over the ground.
    fn lobbed(from: Vec3, to: Vec3, speed: f32, gravity: f32, life: f32) -> Shot {
        Shot::timed(from, to, (to - from).with_y(0.0).length() / speed, gravity, life)
    }

    /// `LaunchAtPoint`: at a place, the faster for the shooter's own speed that way.
    fn at_point(from: Vec3, to: Vec3, speed: f32, gravity: f32, life: f32, shooter: Vec3) -> Shot {
        let flat = (to - from).with_y(0.0);
        let distance = if flat.length() == 0.0 { 1.0 } else { flat.length() };
        let speed = speed + shooter.dot(flat / distance).max(0.0);
        Shot::timed(from, to, distance / speed, gravity, life)
    }

    /// `LaunchAtRacer`: at where a car will be when the shot gets there. `behind` is
    /// the thrower's facing when the lead is only taken if that place is behind it.
    fn at_racer(from: Vec3, target: Seen, speed: f32, gravity: f32, life: f32, shooter: Vec3, behind: Option<Vec3>) -> Shot {
        let aim = target.pos + Vec3::Y * TARGET_HEIGHT;
        let flat = (aim - from).with_y(0.0);
        let distance = if flat.length() == 0.0 { 1.0 } else { flat.length() };
        let speed = (speed + shooter.dot(flat / distance).abs()).max(1e-3);
        let duration = distance / speed;
        let led = aim + target.vel * duration;
        let to = match behind {
            Some(facing) if (led - from).dot(facing) >= 0.0 => aim,
            _ => led,
        };
        Shot::timed(from, to, duration, gravity, life)
    }

    /// `Deflect`: sent from where it is at whoever fired it, or where it came from.
    fn deflect(&mut self, at: Vec3, back: Option<Seen>) {
        let (target, vel) = back.map_or((self.from, Vec3::ZERO), |car| (car.pos + Vec3::Y * TARGET_HEIGHT, car.vel));
        let flat = (target - at).with_y(0.0).length();
        let duration = if flat == 0.0 { 1.0 } else { flat } / self.speed.max(1e-3);
        *self = Shot::timed(at, target + vel * duration, duration, self.gravity, self.life);
    }

    fn position(&self) -> Vec3 {
        self.from + self.vel * self.age - Vec3::Y * 0.5 * self.gravity * self.age * self.age
    }

    fn velocity(&self) -> Vec3 {
        self.vel - Vec3::Y * self.gravity * self.age
    }

    /// `Update`: moves the shot on from `pos`, to where it is now or what stopped it.
    fn fly(&mut self, pos: &mut Vec3, dt: f32, track: &Track) -> Flight {
        self.age += dt;
        if self.age >= self.life {
            return Flight::Expired;
        }
        let next = self.position();
        if let Some(hit) = track.collision.shot(*pos, next) {
            *pos = hit.point;
            return Flight::HitWorld(hit.point, hit.surface.shot_event);
        }
        *pos = next;
        Flight::Flying
    }
}

pub fn use_items(
    mut commands: Commands,
    assets: Res<ItemAssets>,
    track: Res<Track>,
    targets: Option<Res<Targets>>,
    events: Option<Res<TrackEvents>>,
    mut sfx: ResMut<Sfx>,
    mut q: Query<(Entity, &mut Kart, &mut Controls)>,
) {
    let cars = seen(q.iter().map(|(e, k, _)| (e, k)));
    let spent = events.as_ref().map_or(&[][..], |events| &events.started[..]);
    for (owner, mut k, mut c) in &mut q {
        if !std::mem::take(&mut c.use_item) {
            continue;
        }
        if k.spin > 0.0 || k.warp > 0.0 || k.warp_start > 0.0 {
            continue;
        }
        // With nothing to fire, the button sounds the horn.
        let Some(power) = k.held.take() else {
            k.cues.horn = true;
            continue;
        };
        let level = std::mem::take(&mut k.whites).min(3);

        // What is dropped lands on the ground under the car (`ComputeDropPosition`).
        let ground = track.collision.ground(k.pos + Vec3::Y * 0.5, DROP_PROBE);
        let dropped = ground.map_or(k.pos, |hit| hit.point + Vec3::Y * DROP_HEIGHT);
        match (power, level) {
            (Power::Red, 0) => sfx.play_at(id::CANNON_FIRE, k.pos),
            (Power::Red, 1) => sfx.play_at(id::HOOK_FIRE, k.pos),
            (Power::Red, 2) => {}
            (Power::Red, _) => sfx.play_at(id::MISSILE_FIRE, k.pos),
            (Power::Yellow, 0) => sfx.play_at(id::OIL_DROP, dropped),
            (Power::Yellow, 2) => sfx.play_at(id::MAGNET_DROP, dropped),
            // Dynamite and curses only make the sounds they keep up; shields and
            // turbos are heard from the kart.
            _ => {}
        }
        // Drivers are pleased with themselves for a drop or a shield (`Racer::AiUsePowerup`);
        // for a turbo, once it lights.
        if matches!(power, Power::Yellow | Power::Blue) || (power == Power::Green && level < 3) {
            k.cues.reaction = Some(true);
        }

        let forward = (k.rot * Vec3::NEG_Z).normalize();
        let muzzle = k.pos + Vec3::Y * LAUNCH_HEIGHT;
        // Only the player's own shots lock onto the circuit's target points.
        let point = |cone: (f32, f32, f32)| {
            let targets = targets.as_ref().filter(|_| k.route.is_none())?;
            targets.find(spent, k.pos, forward, cone)
        };
        let mut spawn = |action: Action, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Vec3, size: Vec3| {
            commands.spawn((
                action,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(at).with_scale(size),
            ));
        };
        match (power, level) {
            // `RacePowerupManager::FireCannonball`: a target point ahead, else the
            // nearest car ahead, else straight on.
            (Power::Red, 0) => {
                let shot = if let Some(at) = point(AIM_POINT) {
                    Shot::at_point(muzzle, at, CANNONBALL_SPEED, CANNONBALL_GRAVITY, CANNONBALL_LIFE, k.vel)
                } else if let Some(car) = nearest(&cars, k.pos, forward, AIM_RACER) {
                    Shot::at_racer(muzzle, car, CANNONBALL_SPEED, CANNONBALL_GRAVITY, CANNONBALL_LIFE, k.vel, None)
                } else {
                    let at = k.pos + forward * CANNONBALL_RANGE + Vec3::Y * TARGET_HEIGHT;
                    Shot::at_point(muzzle, at, CANNONBALL_SPEED, CANNONBALL_GRAVITY, CANNONBALL_LIFE, k.vel)
                };
                spawn(Action::Cannonball { owner, shot, on_hit: None }, &assets.sphere, &assets.black, muzzle, Vec3::splat(0.5));
            }
            // `FireGrapplingHook`: the farthest car dead ahead, else the nearest in a
            // wide cone, else a target point, else straight on.
            (Power::Red, 1) => {
                let car = farthest(&cars, k.pos, forward, HOOK_FAR).or_else(|| nearest(&cars, k.pos, forward, HOOK_WIDE));
                let shot = match car {
                    Some(car) => Shot::at_racer(muzzle, car, HOOK_SPEED, HOOK_GRAVITY, HOOK_FLIGHT_TIME, k.vel, None),
                    None => {
                        let at = point(AIM_POINT).unwrap_or(k.pos + forward * HOOK_RANGE + Vec3::Y * TARGET_HEIGHT);
                        Shot::at_point(muzzle, at, HOOK_SPEED, HOOK_GRAVITY, HOOK_FLIGHT_TIME, k.vel)
                    }
                };
                spawn(Action::Hook { owner, shot, time: HOOK_FLIGHT_TIME, pulling: None }, &assets.cube, &assets.grey, muzzle, Vec3::splat(0.5));
            }
            (Power::Red, 2) => {
                let size = Vec3::new(0.3, 0.3, LIGHTNING_RANGE);
                spawn(Action::Lightning { owner, time: LIGHTNING_TIME, crackle: 0.0, shocked: None }, &assets.cube, &assets.bolt, muzzle, size);
            }
            // `FireHomingMissiles`: three, each after the next car in the cone; with
            // too few cars there, the rest fan out ahead.
            (Power::Red, _) => {
                let mut ahead = each_in_cone(&cars, k.pos, forward, MISSILE_AIM);
                let first = ahead.next();
                let mut target = first;
                // `HomingProjectile::StartHoming`: it takes the road from the car's
                // last checkpoint, unless the car is pointing back at it.
                let gate = k.checkpoint.filter(|&gate| track.course.checkpoints.get(gate).is_some_and(|gate| forward.dot(gate.normal) <= 0.0));
                for index in 0..MISSILES {
                    if index > 0 {
                        target = target.and_then(|_| ahead.next());
                    }
                    // The launch only settles how fast it goes.
                    let shot = match target {
                        Some(car) => Shot::at_racer(k.pos, car, MISSILE_SPEED, CANNONBALL_GRAVITY, MISSILE_FLIGHT_TIME, k.vel, None),
                        None => {
                            let side = forward.cross(Vec3::Y) * [-1.0, 0.0, 1.0][index] * MISSILE_SPREAD;
                            let at = k.pos + forward * MISSILE_RANGE + side + Vec3::Y * TARGET_HEIGHT;
                            Shot::at_point(k.pos, at, MISSILE_SPEED, CANNONBALL_GRAVITY, MISSILE_FLIGHT_TIME, k.vel)
                        }
                    };
                    let action = Action::Missile {
                        owner,
                        target: target.map(|car| car.entity),
                        at: k.pos,
                        heading: forward,
                        speed: shot.speed,
                        dash: shot.vel.length(),
                        waypoint: None,
                        gate,
                        looked: 0.0,
                        spiral: (0.0, 0.0),
                        time: MISSILE_FLIGHT_TIME,
                    };
                    spawn(action, &assets.sphere, &assets.red, k.pos + Vec3::Y * MISSILE_HEIGHT, Vec3::new(0.35, 0.35, 0.8));
                }
            }
            (Power::Yellow, 0) => {
                spawn(Action::OilSlick { owner, age: 0.0 }, &assets.disc, &assets.oil, dropped, Vec3::new(1.4, 1.0, 1.4));
            }
            // `ThrowDynamite`: at the nearest car behind, else well back down the road.
            (Power::Yellow, 1) => {
                let shot = match nearest(&cars, k.pos, -forward, AIM_RACER) {
                    Some(car) => Shot::at_racer(muzzle, car, DYNAMITE_SPEED, CANNONBALL_GRAVITY, DYNAMITE_FLIGHT_TIME, k.vel, Some(k.facing)),
                    None => {
                        let at = k.pos - forward * DYNAMITE_THROW + Vec3::Y * LAUNCH_HEIGHT;
                        Shot::at_point(muzzle, at, DYNAMITE_SPEED, CANNONBALL_GRAVITY, DYNAMITE_FLIGHT_TIME, k.vel)
                    }
                };
                let action = Action::Dynamite { owner, shot: Some(shot), blasts: DYNAMITE_BLASTS, wait: 0.0 };
                spawn(action, &assets.stick, &assets.red, muzzle, Vec3::ONE);
            }
            (Power::Yellow, 2) => {
                let action = Action::Magnet { owner, time: MAGNET_ARMED_TIME, state: MagnetState::Armed, held: None, stopped: false };
                spawn(action, &assets.disc, &assets.magnet, dropped, Vec3::new(1.2, 3.0, 1.2));
            }
            (Power::Yellow, _) => {
                spawn(Action::Curse { owner, age: 0.0 }, &assets.disc, &assets.curse, dropped, Vec3::new(1.2, 3.0, 1.2));
            }
            (Power::Blue, level) => {
                k.shield = SHIELD_TIMES[level as usize];
                k.shield_level = level;
                // A shield lifts a curse.
                k.cursed = 0.0;
            }
            (Power::Green, 3) => k.warp_start = WARP_START,
            (Power::Green, level) => k.start_boost(level),
        }
    }
}

/// Something other than a kart that a lightning bolt comes from.
#[derive(Component)]
pub struct Beam {
    pub from: Vec3,
    pub forward: Vec3,
}

impl ItemAssets {
    /// A cannon ball fired by the circuit itself, from one place at another.
    pub fn cannonball(&self, commands: &mut Commands, from: Vec3, to: Vec3, on_hit: Option<i32>) -> Entity {
        let shot = if from.with_y(0.0).distance(to.with_y(0.0)) < 1.0 {
            Shot { from, vel: Vec3::ZERO, gravity: CANNONBALL_GRAVITY, speed: CANNONBALL_SPEED, age: 0.0, life: EMPLACED_LIFE }
        } else {
            Shot::lobbed(from, to, CANNONBALL_SPEED, CANNONBALL_GRAVITY, EMPLACED_LIFE)
        };
        let action = Action::Cannonball { owner: Entity::PLACEHOLDER, shot, on_hit };
        let transform = Transform::from_translation(from).with_scale(Vec3::splat(0.5));
        commands.spawn((action, Mesh3d(self.sphere.clone()), MeshMaterial3d(self.black.clone()), transform)).id()
    }

    /// A mummy's curse left lying in wait by the circuit.
    pub fn curse(&self, commands: &mut Commands, at: Vec3) {
        let action = Action::Curse { owner: Entity::PLACEHOLDER, age: 0.0 };
        let transform = Transform::from_translation(at).with_scale(Vec3::new(1.2, 3.0, 1.2));
        commands.spawn((action, Mesh3d(self.disc.clone()), MeshMaterial3d(self.curse.clone()), transform));
    }

    /// A lightning bolt from `beam`, an entity with a [`Beam`].
    pub fn lightning(&self, commands: &mut Commands, beam: Entity) {
        let action = Action::Lightning { owner: beam, time: LIGHTNING_TIME, crackle: 0.0, shocked: None };
        let transform = Transform::from_scale(Vec3::new(0.3, 0.3, LIGHTNING_RANGE));
        commands.spawn((action, Mesh3d(self.cube.clone()), MeshMaterial3d(self.bolt.clone()), transform));
    }
}

fn flight_sound(at: Vec3, vel: Vec3) -> Emitter {
    Emitter::at(at).moving(vel).range(FLIGHT_SOUND_RANGE.0, FLIGHT_SOUND_RANGE.1)
}

/// How far an explosion has grown, of 1, this long into its life
/// (`PowerupExplosion::UpdateFlash`).
fn blast_growth(age: f32) -> f32 {
    let rate = 1.0 / EXPLOSION_TIME;
    let slowing = 2.0 * (1.0 - BLAST_START - 2.0 * rate) * rate * rate;
    BLAST_START + 2.0 * rate * age + 0.5 * slowing * age * age
}

/// Runs everything power-ups have put into the world.
pub fn actions(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<ItemAssets>,
    track: Res<Track>,
    mut sfx: ResMut<Sfx>,
    mut events: Option<ResMut<TrackEvents>>,
    mut actions: Query<(Entity, &mut Action, &mut Transform)>,
    mut karts: Query<(Entity, &mut Kart)>,
    beams: Query<&Beam>,
) {
    let dt = time.delta_secs();
    // Explosions are collected and set off once every action has had its turn.
    let mut blasts: Vec<(Vec3, f32, Entity)> = Vec::new();
    let touching = |k: &Kart, at: Vec3, radius: f32| (k.pos + Vec3::Y * 0.6).distance_squared(at) < radius * radius;
    let cars = seen(karts.iter());
    let car = |entity: Entity| cars.iter().find(|car| car.entity == entity).copied();

    for (entity, mut action, mut tf) in &mut actions {
        let pos = tf.translation;
        let mut done = false;
        match &mut *action {
            // `CannonballAction`.
            Action::Cannonball { owner, shot, on_hit } => {
                let flight = shot.fly(&mut tf.translation, dt, &track);
                let at = tf.translation;
                sfx.sustain_nearest(flight::CANNONBALL, id::CANNON_FLIGHT, flight_sound(at, shot.velocity()), FLIGHT_SOUND_RANGE.1);
                // `LauncherHazard`: a shot the circuit fires at a spot meets no car on
                // its way; only its landing counts, and one that never lands is gone.
                let launched = *owner == Entity::PLACEHOLDER && on_hit.is_some();
                let struck = karts.iter_mut().find(|(e, k)| !launched && e != owner && k.warp <= 0.0 && touching(k, at, KART_RADIUS));
                let mut deflected = false;
                if let (Flight::Flying, Some((victim, mut k))) = (&flight, struck) {
                    done = true;
                    let hit = k.pos;
                    if k.shielded() {
                        k.cues.reaction = Some(true);
                        k.cues.shield_hit = true;
                        if k.shield_level >= DEFLECTING_SHIELD {
                            // Sent back; it now belongs to whoever deflected it.
                            shot.deflect(at, car(*owner));
                            (*owner, deflected, done) = (victim, true, false);
                        }
                    } else {
                        k.cues.reaction = Some(false);
                        k.drop_white();
                        sfx.emit(id::EXPLOSION, Emitter::at(hit).far());
                        if let Ok((_, mut shooter)) = karts.get_mut(*owner) {
                            shooter.cues.reaction = Some(true);
                        }
                    }
                    if !deflected {
                        blasts.push((hit, CANNONBALL_BLAST, *owner));
                    }
                } else if launched && matches!(flight, Flight::Expired) {
                    commands.entity(entity).despawn();
                    continue;
                } else if !matches!(flight, Flight::Flying) {
                    // Some surfaces answer to being shot.
                    if let (Flight::HitWorld(point, Some(event)), Some(events)) = (&flight, &mut events) {
                        events.fire(*event, Some(*point), &mut sfx);
                    }
                    sfx.emit(id::EXPLOSION, Emitter::at(at).far());
                    blasts.push((at, CANNONBALL_BLAST, *owner));
                    done = true;
                }
                if let (true, Some(event), Some(events)) = (done, *on_hit, &mut events) {
                    events.fire(event, Some(at), &mut sfx);
                }
            }
            // `GrapplingHookAction`.
            Action::Hook { owner, shot, time, pulling } => {
                *time -= dt;
                done = *time <= 0.0;
                match *pulling {
                    None => {
                        let flight = shot.fly(&mut tf.translation, dt, &track);
                        let at = tf.translation;
                        sfx.sustain_nearest(flight::HOOK, id::HOOK_FLIGHT, flight_sound(at, shot.velocity()), FLIGHT_SOUND_RANGE.1);
                        let caught = karts.iter_mut().find(|(e, k)| e != owner && k.warp <= 0.0 && touching(k, at, KART_RADIUS));
                        let mut missed = !matches!(flight, Flight::Flying) || done;
                        let mut hooked = false;
                        if let (Flight::Flying, Some((victim, mut k))) = (&flight, caught) {
                            if k.shielded() {
                                k.cues.reaction = Some(true);
                                k.cues.shield_hit = true;
                                missed = true;
                            } else {
                                *pulling = Some(victim);
                                *time = HOOK_PULL_TIME;
                                k.cues.reaction = Some(false);
                                k.drop_white();
                                sfx.play_at(id::HOOK_HIT, k.pos);
                                (missed, hooked, done) = (false, true, false);
                            }
                        }
                        if hooked
                            && let Ok((_, mut k)) = karts.get_mut(*owner) {
                                k.cues.reaction = Some(true);
                            }
                        if missed {
                            // The line snaps back.
                            sfx.play_at(id::HOOK_MISS, at);
                            sfx.play_at(id::HOOK_RETRACT, at);
                            done = true;
                        }
                    }
                    Some(victim) => {
                        // Reel the two karts towards each other until they meet, or
                        // the one hooked is no longer ahead.
                        let ends = (karts.get(*owner).map(|k| (k.1.pos, k.1.rot * Vec3::NEG_Z)), karts.get(victim).map(|k| (k.1.pos, k.1.shielded())));
                        if let (Ok((from, forward)), Ok((to, shielded))) = ends {
                            let rope = to - from;
                            if shielded || rope.length() < HOOK_RELEASE_DISTANCE || rope.dot(forward) < 0.0 || done {
                                sfx.play_at(id::HOOK_RELEASE, to);
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
                                sfx.sustain_nearest(flight::HOOK_PULL, id::HOOK_PULL, flight_sound(to, Vec3::ZERO), FLIGHT_SOUND_RANGE.1);
                            }
                        } else {
                            done = true;
                        }
                    }
                }
            }
            // `LightningAction`: whoever it strikes it stays on for a second, and
            // strikes nobody else until then.
            Action::Lightning { owner, time, crackle, shocked } => {
                *time -= dt;
                done = *time <= 0.0;
                let wielder = karts.get(*owner).map(|(_, k)| (k.pos, (k.rot * Vec3::NEG_Z).normalize(), k.vel));
                let Ok((from, forward, vel)) = wielder.or(beams.get(*owner).map(|b| (b.from, b.forward, Vec3::ZERO))) else {
                    commands.entity(entity).despawn();
                    continue;
                };
                // The wand hums, sinking as it gives out, and crackles along its reach.
                let fading = ((LIGHTNING_FADE.1 - *time) / LIGHTNING_FADE.1).clamp(0.0, 1.0);
                let hum = Emitter::at(from).moving(vel).pitch(1.0 - LIGHTNING_FADE.0 * fading);
                sfx.sustain(entity, 0, id::LIGHTNING_LOOP, hum);
                *crackle -= dt;
                if *crackle <= 0.0 && *time > LIGHTNING_FADE.1 {
                    let along = sfx.roll((LIGHTNING_RANGE / UNIT) as u32) as f32 * UNIT;
                    sfx.play_at(id::LIGHTNING_CRACKLE, from + forward * along);
                    *crackle = LIGHTNING_CRACKLE.0 + sfx.roll(1000) as f32 * 0.001 * LIGHTNING_CRACKLE.1;
                }
                if done {
                    sfx.play_at(id::LIGHTNING_END, from);
                }
                // The bolt reaches out ahead of the kart holding the wand.
                tf.translation = from + Vec3::Y * 0.9 + forward * LIGHTNING_RANGE * 0.5;
                tf.rotation = Transform::IDENTITY.looking_to(forward, Vec3::Y).rotation;
                if let Some((_, since)) = shocked {
                    *since += dt;
                    if *since > LIGHTNING_SHOCK {
                        *shocked = None;
                    }
                }
                let mut struck = false;
                for victim in each_in_cone(&cars, from, forward, (LIGHTNING_MIN_RANGE, LIGHTNING_RANGE, LIGHTNING_CONE)) {
                    if shocked.is_some() || victim.entity == *owner {
                        continue;
                    }
                    let Ok((_, mut k)) = karts.get_mut(victim.entity) else { continue };
                    if k.shielded() {
                        k.cues.reaction = Some(true);
                        k.cues.shield_hit = true;
                    } else if k.spin_out <= 0.0 && k.launch(1.0) {
                        sfx.play_at(id::LIGHTNING_ZAP, k.pos);
                        k.cues.reaction = Some(false);
                        k.drop_white();
                        (*shocked, struck) = (Some((victim.entity, 0.0)), true);
                    }
                }
                if struck
                    && let Ok((_, mut k)) = karts.get_mut(*owner) {
                        k.cues.reaction = Some(true);
                    }
            }
            // `HomingMissileAction` and `HomingProjectile`.
            Action::Missile { owner, target, at, heading, speed, dash, waypoint, gate, looked, spiral, time } => {
                *time -= dt;
                // `UpdateTargeting`: every so often, or when its target is no use, it
                // takes the first car ahead of it that is.
                *looked += dt;
                let usable = |entity: Entity| karts.get(entity).is_ok_and(|(_, k)| k.spin_out <= 0.0 && k.warp <= 0.0 && k.out.is_none());
                if !target.is_some_and(usable) || *looked > MISSILE_LOOK {
                    *looked = 0.0;
                    *target = each_in_cone(&cars, *at, *heading, MISSILE_AIM).map(|car| car.entity).find(|&car| car != *owner && usable(car));
                }
                // With no place on the road to make for, it takes the next checkpoint
                // that is ahead of it and that it hasn't passed.
                let gates = &track.course.checkpoints;
                if waypoint.is_none() {
                    while let Some(found) = gate.and_then(|index| gates.get(index)) {
                        if (found.position - *at).dot(*heading) > 0.0 && found.normal.dot(*at - found.position) >= 0.0 {
                            break;
                        }
                        *gate = found.next.first().copied().filter(|&next| {
                            gates.get(next).is_some_and(|next| next.position.distance(*at) <= MISSILE_GATE_REACH && next.normal.dot(*heading) <= 0.0)
                        });
                    }
                    if let Some(found) = gate.and_then(|index| gates.get(index)) {
                        let ground = track.collision.ground(found.position + Vec3::Y * 5.0 * UNIT, 55.0 * UNIT);
                        *waypoint = ground.map(|hit| hit.point + Vec3::Y * MISSILE_CLEARANCE);
                    }
                }

                let aim = target.and_then(|t| karts.get(t).ok()).map(|(_, k)| k.pos + Vec3::Y * TARGET_HEIGHT);
                if let Some(aim) = aim {
                    *heading = aim - *at;
                }
                let mut next;
                let mut reached = false;
                match aim {
                    Some(aim) if aim.distance(*at) < MISSILE_SNAP_DISTANCE => {
                        *heading = heading.normalize_or(Vec3::NEG_Z);
                        next = *at + *heading * *dash * dt;
                        reached = aim.distance(*at) < MISSILE_HIT_DISTANCE || (aim - next).dot(*heading) <= 0.0;
                        if reached {
                            next = aim;
                        }
                    }
                    _ => {
                        if let Some(place) = *waypoint {
                            if (place - *at).dot(*heading) > 0.0 {
                                *heading = place - *at;
                            } else {
                                *waypoint = None;
                            }
                        }
                        *heading = heading.normalize_or(Vec3::NEG_Z);
                        next = *at + *heading * *speed * dt;
                        // It keeps its height over the road, and comes down gently.
                        if let Some(hit) = track.collision.ground(next + Vec3::Y * 2.0 * UNIT, 2.0 * UNIT + MISSILE_CLEARANCE) {
                            next.y = hit.point.y + MISSILE_CLEARANCE;
                            if at.y > next.y {
                                next.y = next.y.max(at.y - MISSILE_DESCENT * dt);
                            }
                        }
                    }
                }
                *at = next;
                // `ApplySpiral`.
                spiral.0 = (spiral.0 + MISSILE_SPIRAL.1 * dt).min(MISSILE_SPIRAL.0);
                spiral.1 += MISSILE_SPIRAL.2 * dt;
                let shown = if reached { next } else { next + Quat::from_axis_angle(*heading, spiral.1) * heading.any_orthonormal_vector() * spiral.0 };
                tf.look_to(*heading, Vec3::Y);
                tf.translation = shown;
                sfx.sustain_nearest(flight::MISSILE, id::MISSILE_FLIGHT, flight_sound(shown, *heading * *speed), FLIGHT_SOUND_RANGE.1);

                let struck = karts.iter_mut().find(|(e, k)| e != owner && k.warp <= 0.0 && touching(k, shown, KART_RADIUS));
                if let Some((victim, mut k)) = struck {
                    let hit = k.pos;
                    done = true;
                    if k.shielded() {
                        k.cues.reaction = Some(true);
                        k.cues.shield_hit = true;
                        if k.shield_level >= DEFLECTING_SHIELD {
                            // Sent back at whoever fired it.
                            (*target, *owner, *time, *looked, done) = (Some(*owner), victim, MISSILE_FLIGHT_TIME, 0.0, false);
                            (*waypoint, *gate) = (None, None);
                        }
                    } else {
                        k.cues.reaction = Some(false);
                        k.drop_white();
                        k.spin_round(MISSILE_SPIN_TURNS);
                        sfx.emit(id::MISSILE_EXPLODE, Emitter::at(hit).far());
                        if let Ok((_, mut shooter)) = karts.get_mut(*owner) {
                            shooter.cues.reaction = Some(true);
                        }
                    }
                    if done {
                        blasts.push((hit, CANNONBALL_BLAST, *owner));
                    }
                } else if *time <= 0.0 || reached || track.collision.shot(pos, shown).is_some() {
                    let burst = track.collision.shot(pos, shown).map_or(shown, |hit| hit.point);
                    sfx.emit(id::MISSILE_EXPLODE, Emitter::at(burst).far());
                    blasts.push((burst, CANNONBALL_BLAST, *owner));
                    done = true;
                }
            }
            // `OilSlickAction`, and for every dropped thing `HazardActionBase::OnEvent`:
            // never the car that dropped it; a shield takes it and, but for a curse,
            // that is the end of it.
            Action::OilSlick { owner, age } => {
                *age += dt;
                done = *age > OIL_TIME;
                sfx.sustain(entity, 0, id::OIL_LOOP, Emitter::at(pos));
                for (e, mut k) in &mut karts {
                    if e == *owner || k.warp > 0.0 || !touching(&k, pos, KART_RADIUS) {
                        continue;
                    }
                    if k.shielded() {
                        k.cues.shield_hit = true;
                    } else if k.halted() {
                        continue;
                    } else {
                        k.spin_round(OIL_SPIN_TURNS);
                        sfx.emit(id::OIL_SLIP, Emitter::at(k.pos).far());
                    }
                    done = true;
                    break;
                }
            }
            // `DynamiteAction`: thrown, it goes off where it comes down or on whoever
            // it meets, and twice more close by.
            Action::Dynamite { owner, shot, blasts: left, wait } => {
                let first = *left == DYNAMITE_BLASTS;
                let mut at = pos;
                let landed = match shot {
                    Some(flying) => {
                        sfx.sustain(entity, 0, id::DYNAMITE_FUSE, Emitter::at(pos));
                        let flight = flying.fly(&mut tf.translation, dt, &track);
                        at = tf.translation;
                        tf.rotate_local_z(DYNAMITE_TUMBLE * dt);
                        !matches!(flight, Flight::Flying) || karts.iter().any(|(e, k)| e != *owner && k.warp <= 0.0 && touching(k, at, KART_RADIUS))
                    }
                    None => {
                        *wait -= dt;
                        *wait <= 0.0
                    }
                };
                if landed {
                    if first {
                        sfx.emit(id::EXPLOSION, Emitter::at(at).far());
                    } else {
                        // The later blasts wander a little.
                        let mut wander = || (sfx.roll(DYNAMITE_SCATTER * 2 + 1) as f32 - DYNAMITE_SCATTER as f32) * UNIT;
                        at += Vec3::new(wander(), 0.0, wander());
                        tf.translation = at;
                    }
                    blasts.push((at, BIG_BLAST, *owner));
                    (*shot, *left, *wait) = (None, *left - 1, DYNAMITE_BLAST_INTERVAL);
                    done = *left == 0;
                }
            }
            // `MagnetAction`: waits for a car, then holds whoever comes right under it
            // and draws in anyone else in reach.
            Action::Magnet { owner, time, state, held, stopped } => {
                *time -= dt;
                sfx.sustain(entity, 0, id::MAGNET_LOOP, Emitter::at(pos));
                for (e, mut k) in &mut karts {
                    if e == *owner || *state == MagnetState::Fade || k.warp > 0.0 || !touching(&k, pos, TRAP_RADIUS) {
                        continue;
                    }
                    if k.shielded() {
                        k.cues.shield_hit = true;
                        (*state, *time) = (MagnetState::Fade, 0.0);
                        break;
                    }
                    if *state == MagnetState::Armed {
                        (*state, *time) = (MagnetState::Holding, MAGNET_HOLD_TIME);
                    }
                    if k.pos.distance_squared(pos) <= MAGNET_GRAB * MAGNET_GRAB {
                        if held.is_none() {
                            *held = Some(e);
                            k.cues.reaction = Some(false);
                            sfx.play_at(id::MAGNET_GRAB, pos);
                        }
                    } else if *held != Some(e) {
                        let pull = (pos - k.pos).normalize_or_zero() * MAGNET_PULL;
                        k.external_force += pull;
                    }
                }
                if let Some((_, mut k)) = held.and_then(|held| karts.get_mut(held).ok()) {
                    // `Racer::StartMagnetHold`: the car brakes to a stop, and is let go
                    // soon after it has.
                    k.magnet = MAGNET_HELD;
                    k.boost = 0.0;
                    if k.vel.length() <= MAGNET_STOPPED {
                        if !*stopped && *state == MagnetState::Holding {
                            *time = time.min(MAGNET_FADE);
                        }
                        *stopped = true;
                        k.spin = 0.0;
                    }
                }
                if *time <= 0.0 {
                    match *state {
                        MagnetState::Fade => {
                            done = true;
                            if let Some((_, mut k)) = held.and_then(|held| karts.get_mut(held).ok()) {
                                k.magnet = 0.0;
                            }
                        }
                        _ => (*state, *time) = (MagnetState::Fade, MAGNET_FADE),
                    }
                }
            }
            // `CurseAction`: it stays where it is, for everyone who comes by.
            Action::Curse { owner, age } => {
                *age += dt;
                done = *age > CURSE_ARMED_TIME;
                sfx.sustain(entity, 0, id::CURSE_LOOP, Emitter::at(pos + Vec3::Y * CURSE_HEIGHT));
                for (e, mut k) in &mut karts {
                    if e == *owner || k.warp > 0.0 || !touching(&k, pos, TRAP_RADIUS) {
                        continue;
                    }
                    if k.shielded() {
                        k.cues.shield_hit = true;
                    } else if k.cursed <= 0.0 {
                        k.curse(CURSE_TIME);
                    }
                }
            }
            // `PowerupExplosion`: it throws whoever its growing ball reaches, hardest
            // in the first half of its life.
            Action::Explosion { age, radius, owner } => {
                *age += dt;
                done = *age > EXPLOSION_TIME;
                let growth = blast_growth(age.min(EXPLOSION_TIME));
                tf.scale = Vec3::splat(*radius * (BLAST_START + (1.0 - BLAST_START) * growth).min(1.0));
                let reach = BLAST_CORE + (*radius - BLAST_CORE) * growth + KART_RADIUS;
                let force = (2.0 * (1.0 - *age / EXPLOSION_TIME)).clamp(0.0, 1.0);
                for (e, mut k) in &mut karts {
                    // The player's car is thrown once; the computer's as long as it is in reach.
                    if Some(e) != *owner && !k.shielded() && touching(&k, pos, reach) && !(k.route.is_none() && k.spin_out > 0.0) {
                        k.launch(force);
                    }
                }
            }
        }
        if done {
            commands.entity(entity).despawn();
        }
    }

    for (at, radius, owner) in blasts {
        commands.spawn((
            Action::Explosion { age: 0.0, radius, owner: Some(owner) },
            Mesh3d(assets.sphere.clone()),
            MeshMaterial3d(assets.fire.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(radius * BLAST_START)),
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
            brick: handle(),
            bricks: default(),
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
        world.init_resource::<Sfx>();
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

    /// Thrown into the air, as by an explosion.
    fn thrown(world: &World, e: Entity) -> bool {
        kart(world, e).vel.y > 5.0
    }

    #[test]
    fn cannon_ball_launches_its_target_and_costs_it_a_white_brick() {
        let (mut world, owner, victim) = arena(12.0);
        world.get_mut::<Kart>(victim).unwrap().whites = 2;
        fire(&mut world, owner, Power::Red, 0);
        assert!(ever(&mut world, 2.0, |w| thrown(w, victim)));
        assert_eq!(kart(&world, victim).whites, 1);
        assert!(kart(&world, owner).held.is_none());
    }

    #[test]
    fn strong_shield_sends_a_cannon_ball_back() {
        let (mut world, owner, victim) = arena(12.0);
        fire(&mut world, victim, Power::Blue, 2);
        fire(&mut world, owner, Power::Red, 0);
        assert!(ever(&mut world, 3.0, |w| thrown(w, owner)));
    }

    #[test]
    fn weak_shield_just_absorbs_a_cannon_ball() {
        let (mut world, owner, victim) = arena(12.0);
        fire(&mut world, victim, Power::Blue, 0);
        fire(&mut world, owner, Power::Red, 0);
        assert!(!ever(&mut world, 3.0, |w| thrown(w, owner) || thrown(w, victim)));
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
        assert!(ever(&mut world, 1.0, |w| thrown(w, victim)));

        let (mut world, owner, victim) = arena(-8.0);
        fire(&mut world, owner, Power::Red, 2);
        assert!(!ever(&mut world, 1.0, |w| thrown(w, victim)));
    }

    #[test]
    fn homing_missile_follows_the_road_and_spins_its_target() {
        let (mut world, owner, victim) = arena(60.0);
        fire(&mut world, owner, Power::Red, 3);
        assert!(ever(&mut world, 5.0, |w| kart(w, victim).spin > 0.0));
    }

    #[test]
    fn dropped_hazards_catch_the_kart_behind() {
        // The victim is right on the owner's tail: things are dropped where the owner is.
        let hazard = |level: u8, seconds: f32, check: fn(&Kart) -> bool| {
            let (mut world, owner, victim) = arena(-1.0);
            fire(&mut world, owner, Power::Yellow, level);
            assert!(ever(&mut world, seconds, |w| check(kart(w, victim))), "yellow level {level}");
            assert!(!check(kart(&world, owner)), "yellow level {level} caught its owner");
        };
        hazard(0, 1.0, |k| k.spin > 0.0);
        hazard(2, 1.0, |k| k.magnet > 0.0);
        hazard(3, 1.0, |k| k.cursed > 0.0);
    }

    #[test]
    fn dynamite_is_thrown_at_the_car_behind_and_goes_off_when_it_gets_there() {
        let (mut world, owner, victim) = arena(-DYNAMITE_THROW);
        fire(&mut world, owner, Power::Yellow, 1);
        // It is a couple of seconds in the air.
        assert!(!ever(&mut world, 1.5, |w| thrown(w, victim)));
        assert!(ever(&mut world, 1.5, |w| thrown(w, victim)));
        assert!(!thrown(&world, owner));
    }

    #[test]
    fn three_missiles_go_after_three_cars() {
        let (mut world, owner, _) = arena(60.0);
        fire(&mut world, owner, Power::Red, 3);
        let mut missiles = world.query::<&Action>();
        let targets: Vec<bool> = missiles.iter(&world).filter_map(|a| if let Action::Missile { target, .. } = a { Some(target.is_some()) } else { None }).collect();
        // One car ahead: the first missile has it, and the other two fan out.
        assert_eq!(targets.iter().filter(|t| **t).count(), 1);
        assert_eq!(targets.len(), 3);
    }

    #[test]
    fn a_curse_stays_for_the_next_car_and_never_takes_its_owner() {
        let (mut world, owner, victim) = arena(-1.0);
        fire(&mut world, owner, Power::Yellow, 3);
        assert!(ever(&mut world, 0.5, |w| kart(w, victim).cursed > 0.0));
        assert_eq!(world.query::<&Action>().iter(&world).filter(|a| matches!(a, Action::Curse { .. })).count(), 1);
        assert_eq!(kart(&world, owner).cursed, 0.0);
        // A cursed car gets no turbo.
        fire(&mut world, victim, Power::Green, 0);
        assert_eq!(kart(&world, victim).boost, 0.0);
    }

    #[test]
    fn turbo_and_warp_carry_the_kart_forward() {
        let (mut world, owner, _) = arena(100.0);
        fire(&mut world, owner, Power::Green, 0);
        assert!(ever(&mut world, 1.0, |w| kart(w, owner).vel.length() > crate::physics::MAX_SPEED));

        let (mut world, owner, _) = arena(100.0);
        let start = kart(&world, owner).s;
        fire(&mut world, owner, Power::Green, 3);
        ever(&mut world, WARP_START + WARP_TIME + 0.2, |_| false);
        let k = kart(&world, owner);
        assert!(k.s - start > 200.0, "warped {}", k.s - start);
        assert!(k.vel.length() > crate::physics::MAX_SPEED);
    }
}
