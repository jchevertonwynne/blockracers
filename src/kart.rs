//! Karts: the player's controls, AI drivers and race bookkeeping. The driving model
//! itself is in `physics`.

use crate::items::Power;
use crate::meshgen::*;
use crate::assets::materials::Surface;
use crate::physics::{self, MAX_SPEED, UNIT};
use crate::track::{Checkpoint, Track};
use crate::world::{Chassis, LoadedWorld, surface_bundle};
use crate::{Phase, Race};
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

pub const LAPS: i32 = 3;
const WHEEL_RADIUS: f32 = 0.45;
const KART_RADIUS: f32 = 1.1;
/// Lateral acceleration the AI is willing to corner at; a little under what the tyres
/// hold before slip steering sets in.
const AI_LAT_ACCEL: f32 = 30.0;
/// Longest physics step; frames are split into steps no longer than this.
const MAX_STEP: f32 = 1.0 / 120.0;
/// A warp carries the kart along the racing line this fast, then drops it at this speed.
const WARP_SPEED: f32 = 600.0 * UNIT;
const WARP_EXIT_SPEED: f32 = 700.0 / 4500.0 * 1000.0 * UNIT;

struct Driver {
    name: &'static str,
    body: Color,
    accent: Color,
    skill: f32,
}

/// The player is last in the list, and so starts at the back of the grid.
const DRIVERS: &[Driver] = &[
    Driver { name: "Rocket Racer", body: WHITE, accent: RED, skill: 0.99 },
    Driver { name: "Captain Redbeard", body: BLACK, accent: WHITE, skill: 0.97 },
    Driver { name: "King Kahuka", body: YELLOW, accent: BROWN, skill: 0.95 },
    Driver { name: "Basil the Batlord", body: DARK_GREY, accent: BLUE, skill: 0.94 },
    Driver { name: "Johnny Thunder", body: GREEN, accent: TAN, skill: 0.92 },
    Driver { name: "You", body: RED, accent: YELLOW, skill: 1.0 },
];

#[derive(Component)]
pub struct Kart {
    pub name: &'static str,
    pub slot: usize,
    /// Where the kart touches the ground, midway between the wheels.
    pub pos: Vec3,
    pub vel: Vec3,
    pub rot: Quat,
    /// Heading of the body, kept for the camera and the AI.
    pub yaw: f32,
    /// Direction of travel; lags behind the body when sliding.
    pub facing: Vec3,
    /// Wheels on the ground, and what they are standing on.
    pub contacts: u8,
    pub ground_normal: Vec3,
    pub surface: Surface,
    pub wall_contact: bool,
    pub air_time: f32,
    /// Powersliding.
    pub sliding: bool,
    /// Turning tighter than the tyres can hold.
    pub slipping: bool,
    /// Smoothed steering input, -1..1 (positive is left).
    pub steer: f32,
    pub wheel_angle: f32,
    pub top_factor: f32,

    // Track-space state.
    pub idx: usize,
    pub s: f32,
    pub lat: f32,
    pub lap: i32,
    pub progress: f32,
    pub place: usize,
    pub finished: Option<f32>,

    // Power-ups and their effects (timers in seconds).
    pub held: Option<Power>,
    /// White bricks collected: the level the held power-up will fire at.
    pub whites: u8,
    /// Whirling round on the spot.
    pub spin: f32,
    /// Blown into the air, with no control.
    pub spin_out: f32,
    pub boost: f32,
    pub shield: f32,
    pub shield_level: u8,
    /// Steering reversed and top speed halved.
    pub cursed: f32,
    /// Held in place by a magnet.
    pub magnet: f32,
    /// Hurtling along the racing line, out of harm's way.
    pub warp: f32,
    /// Extra acceleration for the coming physics step (a grappling hook's pull).
    pub external_force: Vec3,

    // Race rules.
    /// Last checkpoint gate crossed, and whether that was in the right direction.
    pub checkpoint: Option<usize>,
    pub checkpoint_forward: bool,
    /// Times gate 0 has been passed forwards, less one.
    pub checkpoint_count: i32,
    pub crossed_backward: bool,
    /// The lap zone the kart is in and the two before it. Zone 1 is the finish line,
    /// 2 the stretch after it and 0 the rest of the lap.
    pub zones: [u8; 3],

    // The car itself.
    pub wheels: [Vec3; 4],
    pub body: [Vec3; 4],
    pub stats: Stats,
}

/// Multipliers from the car's handling, top speed and acceleration ratings.
#[derive(Clone, Copy)]
pub struct Stats {
    pub handling: f32,
    pub top_speed: f32,
    pub acceleration: f32,
}

impl Stats {
    /// From ratings of 0 to 100, as the original scales them.
    pub fn from_ratings([handling, top_speed, acceleration]: [f32; 3]) -> Self {
        Stats {
            handling: 0.7 + 0.003 * handling,
            top_speed: 1.0 - (50.0 - top_speed) * 0.001,
            acceleration: 1.0 - (50.0 - acceleration) * 0.001,
        }
    }
}

impl Kart {
    pub(crate) fn new(track: &Track, slot: usize) -> Self {
        // The circuit's own grid if it has one (the player starts from slot 0, at the
        // back); otherwise two columns behind the line.
        let grid_slot = (slot + 1) % DRIVERS.len();
        let (pos, dir) = match track.course.grid.get(grid_slot) {
            Some(&(pos, dir)) if track.course.grid.len() >= DRIVERS.len() => {
                let ground = track.collision.ground(pos + Vec3::Y * 2.0, 8.0);
                (ground.map_or(pos, |hit| hit.point), dir)
            }
            _ => {
                let s = -8.0 - (slot / 2) as f32 * 6.0;
                let lat = track.road * if slot % 2 == 0 { -0.375 } else { 0.375 };
                (track.surface_point(s, lat), track.sample(s).2.cross(Vec3::NEG_Y).normalize())
            }
        };
        let yaw = (-dir.x).atan2(-dir.z);
        let (idx, s, lat) = track.project(pos, track.nearest(pos));
        Kart {
            name: DRIVERS[slot].name,
            slot,
            pos,
            vel: Vec3::ZERO,
            rot: Quat::from_rotation_y(yaw),
            yaw,
            facing: dir,
            contacts: 4,
            ground_normal: Vec3::Y,
            surface: Surface::default(),
            wall_contact: false,
            air_time: 0.0,
            sliding: false,
            slipping: false,
            steer: 0.0,
            wheel_angle: 0.0,
            top_factor: 1.0,
            idx,
            s,
            lat,
            lap: 0,
            progress: 0.0,
            place: slot + 1,
            finished: None,
            held: None,
            whites: 0,
            spin: 0.0,
            spin_out: 0.0,
            boost: 0.0,
            shield: 0.0,
            shield_level: 0,
            cursed: 0.0,
            magnet: 0.0,
            warp: 0.0,
            external_force: Vec3::ZERO,
            checkpoint: None,
            checkpoint_forward: true,
            checkpoint_count: -1,
            crossed_backward: false,
            zones: [0, 2, 1],
            wheels: physics::WHEELS,
            body: physics::BODY_POINTS,
            stats: Stats::from_ratings([50.0; 3]),
        }
    }

    pub fn reset(&mut self, track: &Track) {
        let (wheels, body, stats) = (self.wheels, self.body, self.stats);
        *self = Kart { wheels, body, stats, ..Kart::new(track, self.slot) };
    }

    /// Takes the car's contact points, footprint and ratings from the chassis table.
    fn set_chassis(&mut self, chassis: &Chassis) {
        // The game's cars have X forward and Y left; ours face -Z with X to the right.
        let local = |v: Vec3| Vec3::new(-v.y, 0.0, -v.x) * UNIT;
        self.wheels = chassis.wheels.map(local);
        let half = chassis.footprint * 0.5;
        self.body = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].map(|(x, z)| {
            Vec3::new(x * half.x * UNIT, physics::BODY_POINT_HEIGHT, z * half.y * UNIT)
        });
        self.stats = Stats::from_ratings(chassis.stats);
    }

    /// Sets the kart down on the road at rest, pointing along the racing line.
    pub(crate) fn place(&mut self, track: &Track, s: f32, lat: f32) {
        let dir = track.sample(s).1.with_y(0.0).normalize();
        self.pos = track.surface_point(s, lat) + Vec3::Y * 0.2;
        self.vel = Vec3::ZERO;
        self.yaw = (-dir.x).atan2(-dir.z);
        self.rot = Quat::from_rotation_y(self.yaw);
        self.facing = dir;
        self.air_time = 0.0;
        (self.idx, self.s, self.lat) = track.project(self.pos, track.nearest(self.pos));
    }

    pub fn shielded(&self) -> bool {
        self.shield > 0.0
    }

    /// Something that would hurt arrives. Returns false if a shield, or being in warp,
    /// keeps it out.
    fn vulnerable(&self) -> bool {
        !self.shielded() && self.warp <= 0.0
    }

    /// Whirls the kart round `turns` times, unless protected.
    pub fn spin_round(&mut self, turns: f32) {
        if self.vulnerable() && self.spin <= 0.0 {
            self.spin = turns * TAU / physics::SPIN_RATE;
        }
    }

    /// Stops the kart dead and throws it forwards and up, unless protected.
    pub fn launch(&mut self) {
        if self.vulnerable() && self.spin_out <= 0.0 {
            self.vel = self.facing * physics::LAUNCH_FORWARD_SPEED + Vec3::Y * physics::LAUNCH_UP_SPEED;
            self.spin_out = physics::SPIN_OUT_TIME;
            self.contacts = 0;
        }
    }

    pub fn display_lap(&self) -> i32 {
        self.lap.clamp(1, LAPS)
    }

    /// Whether the last gate was crossed backwards.
    pub fn wrong_way(&self) -> bool {
        self.checkpoint.is_some() && !self.checkpoint_forward
    }

    /// The race rules, applied to the move from `from` to where the kart is now.
    fn follow_course(&mut self, track: &Track, from: Vec3) {
        let course = &track.course;
        let lift = Vec3::Y * physics::BODY_POINT_HEIGHT;
        let travel = self.pos - from;
        if let Some(hit) = course.gates.any(from + lift, self.pos + lift) {
            if let Some(gate) = course.checkpoints.get(hit.tag) {
                self.cross_checkpoint(hit.tag, gate, travel.dot(gate.normal) < 0.0);
            }
        }
        for &(centre, radius, zone) in &course.zones {
            if centre.distance_squared(self.pos) < radius * radius {
                self.enter_zone(zone);
            }
        }
        if course.finish.any(from + lift, self.pos + lift).is_some()
            && course.checkpoints.first().is_none_or(|c| travel.dot(c.normal) < 0.0)
        {
            // `Racer::CrossFinishLine`: the line counts only if the kart got here by
            // way of the zone after the line and then the rest of the lap. The grid
            // starts in that state, so the first crossing begins lap one.
            if self.zones == [0, 2, 1] {
                self.lap += 1;
            }
            self.enter_zone(1);
        }

        // Race order: gates passed, plus how far towards the next one.
        self.progress = match self.checkpoint.and_then(|i| course.checkpoints.get(i)) {
            Some(gate) => {
                let next = gate.next.first().and_then(|&n| course.checkpoints.get(n));
                let towards = next.map_or(0.0, |next| {
                    let leg = next.position - gate.position;
                    let step = (next.fraction - gate.fraction).rem_euclid(1.0);
                    step * ((self.pos - gate.position).dot(leg) / leg.length_squared()).clamp(0.0, 0.99)
                });
                self.checkpoint_count as f32 + gate.fraction + towards
            }
            // Still on the grid: nearest the first gate leads.
            None => course.checkpoints.first().map_or(0.0, |c| {
                -1.0 - c.position.distance(self.pos) / track.length
            }),
        };
    }

    /// `Racer::OnCheckpointCrossed`: only gate 0 advances the count, and crossing it
    /// backwards has to be undone before it will count again.
    fn cross_checkpoint(&mut self, index: usize, gate: &Checkpoint, forward: bool) {
        if self.checkpoint == Some(index) && self.checkpoint_forward == forward {
            return;
        }
        if gate.fraction == 0.0 {
            if !forward {
                self.crossed_backward = true;
                self.checkpoint_forward = false;
                self.checkpoint = Some(index);
                return;
            }
            if !self.crossed_backward {
                self.checkpoint_count += 1;
            }
            self.crossed_backward = false;
        } else if self.crossed_backward {
            self.checkpoint_count -= 1;
            self.crossed_backward = false;
        }
        self.checkpoint = Some(index);
        self.checkpoint_forward = forward;
    }

    fn enter_zone(&mut self, zone: u8) {
        if self.zones[0] != zone {
            self.zones = [zone, self.zones[0], self.zones[1]];
        }
    }
}

#[derive(Component, Default)]
pub struct Controls {
    pub throttle: f32,
    pub steer: f32,
    pub drift: bool,
    pub use_item: bool,
}

#[derive(Component)]
pub struct Player;

/// Every kart has one; the player's only takes over once they've finished.
#[derive(Component)]
pub struct Ai {
    skill: f32,
    lane: f32,
    lane_timer: f32,
    use_timer: f32,
    /// Time spent going nowhere, and time left backing out of it.
    stuck: f32,
    reversing: f32,
}

#[derive(Component)]
pub struct Wheel {
    kart: Entity,
    /// Front wheels turn with the steering.
    front: bool,
    rest: Quat,
    /// In the parent's space.
    steer_axis: Vec3,
    /// In the wheel's own space; turning about it by the kart's wheel angle rolls forward.
    spin_axis: Vec3,
    /// Spin rate relative to the brick kart's wheels, for wheels of another size.
    spin_ratio: f32,
}

#[derive(Component)]
pub struct Shield;

impl Ai {
    fn new(skill: f32, slot: usize) -> Self {
        Ai {
            skill,
            lane: 0.0,
            lane_timer: slot as f32 * 0.7,
            use_timer: 2.0 + slot as f32,
            stuck: 0.0,
            reversing: 0.0,
        }
    }

    /// Follows the racing line: steering, throttle and getting unstuck.
    fn drive(&mut self, k: &Kart, c: &mut Controls, track: &Track, rng: &mut Rng, dt: f32) {
        self.lane_timer -= dt;
        if self.lane_timer <= 0.0 {
            self.lane = track.road * rng.range(-0.6, 0.6);
            self.lane_timer = rng.range(2.0, 5.0);
        }

        // Steer at a point a little way up the road.
        let speed = k.vel.length();
        let to = track.point(k.s + 8.0 + speed * 0.4, self.lane) - k.pos;
        let err = wrap_angle((-to.x).atan2(-to.z) - k.yaw);
        c.steer = (err * 3.0).clamp(-1.0, 1.0);

        // Brake for the tightest corner coming up.
        let tightest = (2..18).map(|j| track.curv[(k.idx + j) % track.n()]).fold(1e-4, f32::max);
        c.throttle = if speed > (AI_LAT_ACCEL / tightest).sqrt() { -0.6 } else { 1.0 };
        c.drift = false;

        // Wedged against something: back out with the wheels turned the other way.
        self.stuck = if speed < 2.0 && k.spin <= 0.0 { self.stuck + dt } else { 0.0 };
        if self.stuck > 1.0 {
            self.stuck = 0.0;
            self.reversing = 1.5;
        }
        if self.reversing > 0.0 {
            self.reversing -= dt;
            c.throttle = -1.0;
            c.steer = -c.steer;
        }
    }
}

fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

fn kart_mesh(body: Color, accent: Color) -> Mesh {
    let mut b = BrickMesh::default();
    let id = Quat::IDENTITY;
    let v = Vec3::new;
    // Chassis, bumper, nose and body. Forward is -Z.
    b.cuboid(v(0.0, 0.4, 0.0), v(0.7, 0.1, 1.5), id, DARK_GREY);
    b.cuboid(v(0.0, 0.4, -1.55), v(0.75, 0.1, 0.1), id, GREY);
    b.brick(v(0.0, 0.65, -0.95), v(0.45, 0.15, 0.55), id, body, (2, 2));
    b.cuboid(v(0.0, 0.7, 0.5), v(0.7, 0.2, 0.9), id, body);
    // Seat back, engine block and rear wing.
    b.cuboid(v(0.0, 1.1, 0.72), v(0.5, 0.3, 0.08), id, DARK_GREY);
    b.brick(v(0.0, 1.05, 1.1), v(0.6, 0.15, 0.3), id, accent, (3, 1));
    for x in [-0.5, 0.5] {
        b.cuboid(v(x, 1.3, 1.35), v(0.06, 0.25, 0.06), id, GREY);
    }
    b.cuboid(v(0.0, 1.58, 1.4), v(0.85, 0.04, 0.22), id, body);
    // Steering wheel.
    b.cuboid(v(0.0, 1.05, -0.3), v(0.18, 0.18, 0.03), Quat::from_rotation_x(-0.5), BLACK);
    // Minifigure: torso, arms, hands, head, face and helmet.
    b.cuboid(v(0.0, 1.15, 0.35), v(0.28, 0.25, 0.16), id, accent);
    for x in [-0.36, 0.36] {
        b.cuboid(v(x, 1.2, 0.08), v(0.07, 0.07, 0.3), id, accent);
        b.cuboid(v(x * 0.7, 1.18, -0.22), v(0.07, 0.07, 0.07), id, YELLOW);
        b.cuboid(v(x * 0.2, 1.56, 0.145), v(0.025, 0.03, 0.01), id, BLACK);
    }
    b.cyl(v(0.0, 1.4, 0.35), 0.2, 0.3, id, YELLOW);
    b.cyl(v(0.0, 1.66, 0.35), 0.24, 0.16, id, body);
    b.cyl(v(0.0, 1.82, 0.35), 0.1, 0.07, id, body);
    b.build()
}

fn wheel_mesh() -> Mesh {
    let mut b = BrickMesh::default();
    let along_x = Quat::from_rotation_z(-FRAC_PI_2);
    b.cyl(Vec3::X * -0.2, WHEEL_RADIUS, 0.4, along_x, BLACK);
    b.cyl(Vec3::X * -0.23, 0.26, 0.46, along_x, GREY);
    // Spokes so that the spin is visible.
    b.cuboid(Vec3::ZERO, Vec3::new(0.24, 0.3, 0.05), Quat::IDENTITY, WHITE);
    b.cuboid(Vec3::ZERO, Vec3::new(0.24, 0.05, 0.3), Quat::IDENTITY, WHITE);
    b.build()
}

pub fn spawn_karts(
    mut commands: Commands,
    track: Res<Track>,
    mut loaded: Option<ResMut<LoadedWorld>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let plastic = materials.add(StandardMaterial {
        perceptual_roughness: 0.35,
        ..default()
    });
    let wheel = meshes.add(wheel_mesh());
    let shield_mesh = meshes.add(Sphere::new(2.3));
    let shield_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.2, 0.5, 1.0, 0.3),
        emissive: LinearRgba::new(0.0, 0.2, 0.8, 1.0),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    // The original cars, if the game data provided them.
    let mut models = loaded.as_mut().map(|l| std::mem::take(&mut l.karts)).unwrap_or_default();
    models.reverse();

    for (slot, driver) in DRIVERS.iter().enumerate() {
        let model = models.pop();
        let mut state = Kart::new(&track, slot);
        if let Some(model) = &model {
            state.set_chassis(&model.chassis);
        }
        let mut kart = commands.spawn((
            state,
            Controls::default(),
            Ai::new(driver.skill, slot),
            Transform::default(),
            Visibility::default(),
        ));
        if slot == DRIVERS.len() - 1 {
            kart.insert(Player);
        }
        let id = kart.id();
        kart.with_child((
            Shield,
            Mesh3d(shield_mesh.clone()),
            MeshMaterial3d(shield_mat.clone()),
            Transform::from_xyz(0.0, 0.9, 0.0),
            Visibility::Hidden,
        ));

        let Some(model) = model else {
            // Brick-built stand-in.
            kart.insert((
                Mesh3d(meshes.add(kart_mesh(driver.body, driver.accent))),
                MeshMaterial3d(plastic.clone()),
            ));
            for (x, z) in [(-0.95, -1.0), (0.95, -1.0), (-0.95, 1.0), (0.95, 1.0)] {
                kart.with_child((
                    Wheel {
                        kart: id,
                        front: z < 0.0,
                        rest: Quat::IDENTITY,
                        steer_axis: Vec3::Y,
                        spin_axis: Vec3::X,
                        spin_ratio: 1.0,
                    },
                    Mesh3d(wheel.clone()),
                    MeshMaterial3d(plastic.clone()),
                    Transform::from_xyz(x, WHEEL_RADIUS, z),
                ));
            }
            continue;
        };

        // The game's models have X forward, Y left and Z up; ours face -Z with Y up.
        let basis = Quat::from_mat3(&Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y));
        let part = |offset: Vec3, scale: f32| {
            let transform = Transform {
                translation: basis * offset * UNIT,
                rotation: basis,
                scale: Vec3::splat(scale * UNIT),
            };
            (transform, Visibility::default())
        };
        let mut bundle = |surface| surface_bundle(surface, &mut meshes, &mut materials, &mut images);
        kart.with_children(|parent| {
            parent.spawn(part(Vec3::ZERO, model.body_scale)).with_children(|body| {
                for surface in model.body {
                    body.spawn(bundle(surface));
                }
            });
            parent.spawn(part(model.chassis.mount, model.driver_scale)).with_children(|figure| {
                for surface in model.driver {
                    figure.spawn(bundle(surface));
                }
            });
            parent.spawn(part(Vec3::ZERO, model.wheel_scale)).with_children(|wheels| {
                for axle in model.axles {
                    let wheel = Wheel {
                        kart: id,
                        front: axle.position.x > 0.0,
                        rest: axle.rotation,
                        steer_axis: Vec3::Z,
                        spin_axis: axle.rotation.inverse() * Vec3::NEG_Y,
                        spin_ratio: WHEEL_RADIUS / (axle.radius * model.wheel_scale * UNIT),
                    };
                    let transform = Transform::from_translation(axle.position).with_rotation(axle.rotation);
                    wheels.spawn((wheel, transform, Visibility::default())).with_children(|axle_entity| {
                        for surface in axle.surfaces {
                            axle_entity.spawn(bundle(surface));
                        }
                    });
                }
            });
        });
    }
}

pub fn player_input(
    keys: Res<ButtonInput<KeyCode>>,
    race: Res<Race>,
    mut q: Query<(&Kart, &mut Controls), With<Player>>,
) {
    let Ok((kart, mut c)) = q.single_mut() else { return };
    if kart.finished.is_some() || race.demo {
        return;
    }
    if race.phase != Phase::Racing {
        *c = Controls::default();
        return;
    }
    let axis = |pos: [KeyCode; 2], neg: [KeyCode; 2]| {
        keys.any_pressed(pos) as i32 as f32 - keys.any_pressed(neg) as i32 as f32
    };
    c.throttle = axis([KeyCode::KeyW, KeyCode::ArrowUp], [KeyCode::KeyS, KeyCode::ArrowDown]);
    c.steer = axis([KeyCode::KeyA, KeyCode::ArrowLeft], [KeyCode::KeyD, KeyCode::ArrowRight]);
    c.drift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    c.use_item = keys.just_pressed(KeyCode::Space);
}

pub fn ai_drive(
    time: Res<Time>,
    track: Res<Track>,
    race: Res<Race>,
    mut rng: ResMut<Rng>,
    mut q: Query<(&mut Kart, &mut Ai, &mut Controls, Has<Player>)>,
) {
    let dt = time.delta_secs();
    let racing = matches!(race.phase, Phase::Racing | Phase::Finished);
    let player_progress = q.iter().find(|x| x.3).map_or(0.0, |x| x.0.progress);
    for (mut k, mut ai, mut c, is_player) in &mut q {
        if is_player && k.finished.is_none() && !race.demo {
            continue;
        }
        if !racing {
            *c = Controls::default();
            continue;
        }

        ai.drive(&k, &mut c, &track, &mut rng, dt);

        // Rubber-banding keeps the pack close to the player.
        let gap = (k.progress - player_progress) * track.length;
        k.top_factor = ai.skill
            * if gap > 60.0 {
                0.93
            } else if gap < -60.0 {
                1.06
            } else {
                1.0
            };

        c.use_item = false;
        if k.held.is_some() {
            ai.use_timer -= dt;
            if ai.use_timer <= 0.0 {
                c.use_item = true;
                ai.use_timer = rng.range(1.5, 5.0);
            }
        }
    }
}

pub fn kart_physics(time: Res<Time>, track: Res<Track>, mut q: Query<(&mut Kart, &Controls)>) {
    let dt = time.delta_secs().min(0.05);
    for (mut kart, c) in &mut q {
        kart.advance(c, &track, dt);
    }
}

impl Kart {
    /// Runs the physics for `dt` seconds, then works out where on the lap that leaves us.
    pub fn advance(&mut self, c: &Controls, track: &Track, dt: f32) {
        let k = self;
        let from = k.pos;
        let warping = k.warp > 0.0;
        for timer in [
            &mut k.spin,
            &mut k.spin_out,
            &mut k.boost,
            &mut k.shield,
            &mut k.cursed,
            &mut k.magnet,
            &mut k.warp,
        ] {
            *timer = (*timer - dt).max(0.0);
        }
        let steer = if k.spin > 0.0 || k.spin_out > 0.0 { 0.0 } else { c.steer };
        k.steer += (steer - k.steer) * (1.0 - (-10.0 * dt).exp());

        if warping {
            // Carried along the racing line, drifting to its middle.
            let (s, lat) = (k.s + WARP_SPEED * dt, k.lat * (1.0 - 2.0 * dt).max(0.0));
            let dir = track.sample(s).1;
            k.pos = track.surface_point(s, lat);
            k.facing = dir.with_y(0.0).normalize_or(k.facing);
            k.rot = Transform::IDENTITY.looking_to(k.facing, Vec3::Y).rotation;
            // Dropped back onto the road at speed when it ends.
            k.vel = k.facing * WARP_EXIT_SPEED;
            k.contacts = 4;
        } else if k.magnet > 0.0 {
            k.vel = Vec3::ZERO;
        } else {
            let steps = (dt / MAX_STEP).ceil().max(1.0);
            for _ in 0..steps as usize {
                physics::step(k, c, &track.collision, dt / steps);
            }
        }
        k.external_force = Vec3::ZERO;
        let forward = k.rot * Vec3::NEG_Z;
        k.yaw = (-forward.x).atan2(-forward.z);
        k.wheel_angle = (k.wheel_angle - k.vel.dot(forward) * dt / WHEEL_RADIUS) % TAU;

        // Where we are along the racing line, for the AI and anything that follows it.
        let (mut idx, mut s, mut lat) = track.project(k.pos, k.idx);
        if lat.abs() > 40.0 {
            // A long way from where we last were; look everywhere.
            (idx, s, lat) = track.project(k.pos, track.nearest(k.pos));
        }
        (k.idx, k.s, k.lat) = (idx, s, lat);

        // Fallen out of the world, or flying for far too long: back onto the road.
        if k.air_time > 4.0 || k.pos.y < track.pts[idx].y - 60.0 {
            k.place(track, s, 0.0);
        } else {
            k.follow_course(track, from);
        }
    }
}

pub fn kart_collisions(mut q: Query<&mut Kart>) {
    let mut pairs = q.iter_combinations_mut();
    while let Some([mut a, mut b]) = pairs.fetch_next() {
        let d = (b.pos - a.pos).with_y(0.0);
        let dist = d.length();
        if dist < 2.0 * KART_RADIUS && dist > 1e-3 {
            let normal = d / dist;
            let push = normal * (2.0 * KART_RADIUS - dist) * 0.5;
            a.pos -= push;
            b.pos += push;
            let closing = (b.vel - a.vel).dot(normal);
            if closing < 0.0 {
                a.vel += normal * closing * 0.6;
                b.vel -= normal * closing * 0.6;
            }
        }
    }
}

pub fn update_places(race: Res<Race>, mut q: Query<&mut Kart>) {
    let mut order: Vec<(f32, Mut<Kart>)> = q
        .iter_mut()
        .map(|mut k| {
            if race.phase == Phase::Racing && k.finished.is_none() && k.lap > LAPS {
                k.finished = Some(race.time);
            }
            // Finishers rank ahead of everyone still racing, earliest first.
            (k.finished.map_or(k.progress, |t| 1e9 - t), k)
        })
        .collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (i, (_, k)) in order.iter_mut().enumerate() {
        if k.place != i + 1 {
            k.place = i + 1;
        }
    }
}

pub fn sync_karts(mut q: Query<(&Kart, &mut Transform)>) {
    for (k, mut t) in &mut q {
        // Lean into corners.
        let lean = -k.steer * 0.07 * (k.vel.length() / MAX_SPEED).min(1.0);
        t.translation = k.pos;
        t.rotation = k.rot * Quat::from_rotation_z(lean);
    }
}

pub fn sync_wheels(
    karts: Query<&Kart>,
    mut wheels: Query<(&Wheel, &mut Transform)>,
    mut shields: Query<(&ChildOf, &mut Visibility), With<Shield>>,
) {
    for (wheel, mut t) in &mut wheels {
        let Ok(k) = karts.get(wheel.kart) else { continue };
        let steer = if wheel.front { k.steer * 0.4 } else { 0.0 };
        t.rotation = Quat::from_axis_angle(wheel.steer_axis, steer)
            * wheel.rest
            * Quat::from_axis_angle(wheel.spin_axis, k.wheel_angle * wheel.spin_ratio);
    }
    for (child_of, mut vis) in &mut shields {
        let Ok(k) = karts.get(child_of.parent()) else { continue };
        // Flicker when about to run out.
        let on = k.shield > 1.0 || (k.shield > 0.0 && (k.shield * 10.0) as i32 % 2 == 0);
        vis.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lets the AI drive one kart alone and returns the times at which it started each lap.
    fn solo_run(track: &Track, seconds: f32) -> Vec<f32> {
        let mut kart = Kart::new(track, 0);
        let mut ai = Ai::new(1.0, 0);
        let mut rng = Rng(7);
        let mut c = Controls::default();
        let (dt, mut laps, mut top) = (1.0 / 60.0, Vec::new(), 0.0f32);
        for frame in 0..(seconds / dt) as usize {
            let lap = kart.lap;
            ai.drive(&kart, &mut c, track, &mut rng, dt);
            kart.advance(&c, track, dt);
            top = top.max(kart.vel.length());
            assert!(kart.pos.is_finite() && kart.vel.length() < 80.0, "physics blew up");
            if kart.lap > lap {
                laps.push(frame as f32 * dt);
            }
        }
        println!("lap starts {laps:?}, top speed {top:.1}, lap length {:.0}", track.length);
        laps
    }

    #[test]
    fn ai_laps_the_brick_circuit() {
        let laps = solo_run(&Track::new(), 150.0);
        assert!(laps.len() >= 3, "{laps:?}");
    }

    /// Needs the original game data; silently passes without it.
    #[test]
    fn ai_laps_the_original_circuits() {
        for race in ["RACEC0R0", "RACEC1R1", "RACEC2R0"] {
            // SAFETY: tests that read this variable all run in this one thread.
            unsafe { std::env::set_var("LEGO_RACE", race) };
            if crate::assets::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM").is_none() {
                return;
            }
            let (track, _) = crate::world::load().unwrap_or_else(|| panic!("{race} failed to load"));
            let laps = solo_run(&track, 200.0);
            assert!(laps.len() >= 3, "{race}: {laps:?}");
        }
    }
}
