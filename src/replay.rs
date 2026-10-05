//! Watching a race again, and stopping one to look round it. Neither is the original's.
//!
//! A replay is the cars' places and a little of their state, noted thirty times a
//! second and put back onto them afterwards, with whatever power-ups had put into the
//! world at each moment put back as it was. Bricks being taken and the sounds of it
//! all are not kept. It is watched from beside the road or from behind any of the
//! cars. Photo mode stops the clock and lets the camera go where it likes, without the
//! display, to take a picture.

use crate::audio::{Sfx, id};
use crate::items::{Action, Power};
use crate::kart::{Kart, WHEEL_RADIUS};
use crate::track::Track;
use crate::{Pause, Phase, Race};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use std::collections::HashMap;
use std::f32::consts::TAU;

/// How often the cars are noted, and for how long after the finish.
const INTERVAL: f32 = 1.0 / 30.0;
const TAIL: f32 = 3.0;
/// The most cars in a race.
const SLOTS: usize = 6;

/// One car at one moment. A race online shows the cars it doesn't drive by these too.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pose {
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    steer: f32,
    boost: f32,
    shield: f32,
    sliding: bool,
    out: bool,
    /// What power-ups have done to it, and what it holds.
    spin: f32,
    spin_out: f32,
    cursed: f32,
    magnet: f32,
    held: Option<Power>,
    whites: u8,
}

impl Pose {
    pub fn of(kart: &Kart) -> Self {
        Pose {
            pos: kart.pos,
            rot: kart.rot,
            vel: kart.vel,
            steer: kart.steer,
            boost: kart.boost,
            shield: kart.shield,
            sliding: kart.sliding,
            out: kart.out.is_some(),
            spin: kart.spin,
            spin_out: kart.spin_out,
            cursed: kart.cursed,
            magnet: kart.magnet,
            held: kart.held,
            whites: kart.whites,
        }
    }

    /// Puts the car as it was, `dt` after it was last put somewhere; `at` is when it
    /// went out of the race, if this is the first that is seen of that.
    pub fn put(self, kart: &mut Kart, at: f32, dt: f32) {
        let pose = self;
        (kart.pos, kart.rot, kart.vel, kart.steer) = (pose.pos, pose.rot, pose.vel, pose.steer);
        (kart.boost, kart.shield, kart.sliding) = (pose.boost, pose.shield, pose.sliding);
        (kart.spin, kart.spin_out, kart.cursed, kart.magnet) =
            (pose.spin, pose.spin_out, pose.cursed, pose.magnet);
        (kart.held, kart.whites) = (pose.held, pose.whites);
        kart.out = if pose.out {
            kart.out.or(Some(at))
        } else {
            None
        };
        let forward = pose.rot * Vec3::NEG_Z;
        kart.yaw = (-forward.x).atan2(-forward.z);
        kart.facing = forward.with_y(0.0).normalize_or(kart.facing);
        kart.wheel_angle = (kart.wheel_angle - pose.vel.dot(forward) * dt / WHEEL_RADIUS) % TAU;
    }

    /// This pose `along` the way to the `next`.
    pub fn towards(self, next: Pose, along: f32) -> Pose {
        let mix = |a: f32, b: f32| a + (b - a) * along;
        Pose {
            pos: self.pos.lerp(next.pos, along),
            rot: self.rot.slerp(next.rot, along),
            vel: self.vel.lerp(next.vel, along),
            steer: mix(self.steer, next.steer),
            boost: mix(self.boost, next.boost),
            shield: mix(self.shield, next.shield),
            ..self
        }
    }
}

struct Frame {
    time: f32,
    /// By grid slot.
    poses: [Option<Pose>; SLOTS],
    /// What power-ups had put into the world, each by the entity it was.
    actions: Vec<(Entity, Action, Transform)>,
}

/// Something put back into the world by a replay, for show.
#[derive(Component)]
pub struct Replayed;

/// How far apart the places beside the road are that a replay is watched from, how
/// far out from the racing line they are and how high, and how wide the view is.
const STATION_GAP: f32 = 70.0;
const STATION_OUT: f32 = 4.0;
const STATION_HEIGHT: f32 = 3.0;
pub const STATION_FOV: f32 = 0.75;

#[derive(Resource, Default)]
pub struct Replay {
    frames: Vec<Frame>,
    /// Seconds noted so far, until the next note is due, and left to note after the finish.
    clock: f32,
    due: f32,
    tail: f32,
    /// How far into the recording the replay being shown has come.
    pub showing: Option<f32>,
    /// The grid slot of the car being watched, where it isn't the player's.
    pub subject: Option<usize>,
    /// Watched from beside the road rather than from behind the car.
    pub trackside: bool,
    /// What the replay has put back into the world, by the entity each was recorded as.
    shown: HashMap<Entity, Entity>,
}

impl Replay {
    pub fn ready(&self) -> bool {
        self.frames.len() > 1
    }

    /// Shows the recording from its beginning; nothing more is added to it.
    pub fn start(&mut self) {
        if self.ready() {
            (self.showing, self.tail) = (Some(0.0), 0.0);
            (self.subject, self.trackside) = (None, true);
        }
    }

    /// Moves on to watching the next car round, or the one before.
    pub fn watch_next(&mut self, player: usize, step: isize) {
        let racing: Vec<usize> = self
            .frames
            .first()
            .map(|f| (0..SLOTS).filter(|&s| f.poses[s].is_some()).collect())
            .unwrap_or_default();
        let now = racing
            .iter()
            .position(|&slot| slot == self.subject.unwrap_or(player))
            .unwrap_or(0) as isize;
        if !racing.is_empty() {
            self.subject = Some(racing[(now + step).rem_euclid(racing.len() as isize) as usize]);
        }
    }

    /// Where beside the road a car is watched from: the nearest of the places set
    /// along the lap, on one side of the road and then the other.
    pub fn station(track: &Track, kart: &Kart) -> Vec3 {
        let number = (kart.s / STATION_GAP).round();
        let side = if number as i32 % 2 == 0 { 1.0 } else { -1.0 };
        let (line, _, right) = track.sample(number * STATION_GAP);
        let at = line + right * side * (track.road + STATION_OUT);
        let ground = track
            .collision
            .ground(at + Vec3::Y * 15.0, 40.0)
            .map_or(line.y, |hit| hit.point.y);
        at.with_y(ground.max(line.y) + STATION_HEIGHT)
    }

    fn length(&self) -> f32 {
        self.frames.last().map_or(0.0, |frame| frame.time)
    }

    fn note(
        &mut self,
        karts: impl Iterator<Item = (usize, Pose)>,
        actions: Vec<(Entity, Action, Transform)>,
    ) {
        let mut poses = [None; SLOTS];
        for (slot, pose) in karts {
            if let Some(place) = poses.get_mut(slot) {
                *place = Some(pose);
            }
        }
        self.frames.push(Frame {
            time: self.clock,
            poses,
            actions,
        });
    }

    /// Where the car in `slot` was `time` seconds in.
    fn pose(&self, slot: usize, time: f32) -> Option<Pose> {
        let next = self
            .frames
            .partition_point(|frame| frame.time <= time)
            .clamp(1, self.frames.len().max(2) - 1);
        let (from, to) = (self.frames.get(next - 1)?, self.frames.get(next)?);
        let along = ((time - from.time) / (to.time - from.time).max(1e-6)).clamp(0.0, 1.0);
        Some((*from.poses.get(slot)?)?.towards((*to.poses.get(slot)?)?, along))
    }
}

/// Whether the race is being run rather than shown again.
pub fn live(replay: Res<Replay>) -> bool {
    replay.showing.is_none()
}

/// Notes where the cars are, through the race and a little past its finish.
pub fn record(
    time: Res<Time>,
    race: Res<Race>,
    mut replay: ResMut<Replay>,
    karts: Query<&Kart>,
    actions: Query<(Entity, &Action, &Transform), Without<Replayed>>,
) {
    let replay = &mut *replay;
    match race.phase {
        Phase::Intro | Phase::Countdown => {
            *replay = Replay {
                tail: TAIL,
                ..default()
            }
        }
        Phase::Finished if replay.tail <= 0.0 => return,
        Phase::Finished => replay.tail -= time.delta_secs(),
        Phase::Racing => {}
    }
    if race.phase == Phase::Intro || race.phase == Phase::Countdown {
        return;
    }
    replay.clock += time.delta_secs();
    replay.due -= time.delta_secs();
    if replay.due <= 0.0 {
        replay.due += INTERVAL;
        let actions = actions
            .iter()
            .map(|(entity, action, transform)| (entity, action.clone(), *transform))
            .collect();
        replay.note(
            karts.iter().map(|kart| (kart.slot, Pose::of(kart))),
            actions,
        );
    }
}

/// Puts the cars where the recording has them, and what power-ups had put into the
/// world back into it.
pub fn play(
    mut commands: Commands,
    time: Res<Time>,
    mut replay: ResMut<Replay>,
    mut karts: Query<&mut Kart>,
    left: Query<Entity, (With<Action>, Without<Replayed>)>,
) {
    let replay = &mut *replay;
    let Some(shown) = replay.showing else {
        // The replay is over: what it put into the world goes.
        for (_, entity) in replay.shown.drain() {
            commands.entity(entity).try_despawn();
        }
        return;
    };
    // Whatever the race itself left lying about would be there twice over.
    for entity in &left {
        commands.entity(entity).try_despawn();
    }
    let dt = time.delta_secs();
    let at = (shown + dt).min(replay.length());
    for mut kart in &mut karts {
        let Some(pose) = replay.pose(kart.slot, at) else {
            continue;
        };
        pose.put(&mut kart, at, dt);
    }
    // Power-ups' doings are as they were at the last note taken.
    let frame = replay
        .frames
        .partition_point(|frame| frame.time <= at)
        .saturating_sub(1);
    let actions = replay
        .frames
        .get(frame)
        .map(|frame| frame.actions.as_slice())
        .unwrap_or_default();
    replay.shown.retain(|was, entity| {
        let still = actions.iter().any(|(recorded, ..)| recorded == was);
        if !still {
            commands.entity(*entity).try_despawn();
        }
        still
    });
    for (was, action, transform) in actions {
        match replay.shown.get(was) {
            Some(&entity) => {
                commands
                    .entity(entity)
                    .try_insert((action.clone(), *transform));
            }
            None => {
                replay.shown.insert(
                    *was,
                    commands.spawn((action.clone(), *transform, Replayed)).id(),
                );
            }
        }
    }
    replay.showing = (at < replay.length()).then_some(at);
}

/// The camera loose from the car, with the clock stopped.
pub struct Shot {
    yaw: f32,
    pitch: f32,
    /// The clock was already stopped, and is to be left so.
    was_paused: bool,
}

#[derive(Resource, Default)]
pub struct Photo(pub Option<Shot>);

/// The line saying what the keys do, which goes when a picture is taken.
#[derive(Component)]
pub struct Hint;

pub fn shooting(photo: Res<Photo>) -> bool {
    photo.0.is_some()
}

/// Metres a second the camera flies at, and how far a pixel of mouse turns it.
const FLY_SPEED: f32 = 14.0;
const LOOK: f32 = 0.004;
const ZOOM: (f32, f32) = (0.15, 2.0);

/// Where pictures go: `$BRICK_PHOTOS`, or a `screenshots` folder.
fn picture_path() -> String {
    let folder = std::env::var("BRICK_PHOTOS").unwrap_or("screenshots".into());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |t| t.as_millis());
    let _ = std::fs::create_dir_all(&folder);
    format!("{folder}/brick-racers-{stamp}.png")
}

/// P stops the race to look round it: the keys fly the camera, dragging turns it,
/// Enter takes the picture and P or Escape goes back.
pub fn photo(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    // The game's clock is stopped, so the camera moves by the real one.
    real: Res<Time<Real>>,
    mut clock: ResMut<Time<Virtual>>,
    pause: Res<Pause>,
    mut photo: ResMut<Photo>,
    mut sfx: ResMut<Sfx>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
    mut display: Query<&mut Visibility, (With<Node>, Without<ChildOf>, Without<Hint>)>,
    hints: Query<Entity, With<Hint>>,
) {
    let (mut transform, mut projection) = camera.into_inner();
    let Some(shot) = &mut photo.0 else {
        if keys.just_pressed(KeyCode::KeyP) && pause.0.is_none() {
            let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
            photo.0 = Some(Shot {
                yaw,
                pitch,
                was_paused: clock.is_paused(),
            });
            clock.pause();
            sfx.play(id::MENU_SELECT);
            commands.spawn((
                Hint,
                Text::new("PHOTO   WASD move   Q E down, up   drag to look   Z X zoom   ENTER save   P back"),
                TextFont { font_size: FontSize::Px(9.0), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                Node { position_type: PositionType::Absolute, bottom: Val::Px(6.0), left: Val::Px(8.0), padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)), ..default() },
            ));
        }
        return;
    };
    if keys.any_just_pressed([KeyCode::KeyP, KeyCode::Escape]) {
        if !shot.was_paused {
            clock.unpause();
        }
        photo.0 = None;
        sfx.play(id::MENU_BACK);
        for hint in &hints {
            commands.entity(hint).despawn();
        }
        for mut visibility in &mut display {
            visibility.set_if_neq(Visibility::Inherited);
        }
        return;
    }
    for mut visibility in &mut display {
        visibility.set_if_neq(Visibility::Hidden);
    }

    let dt = real.delta_secs();
    let held = |positive: [KeyCode; 2], negative: [KeyCode; 2]| {
        keys.any_pressed(positive) as i32 as f32 - keys.any_pressed(negative) as i32 as f32
    };
    if buttons.pressed(MouseButton::Left) {
        shot.yaw -= motion.delta.x * LOOK;
        shot.pitch = (shot.pitch - motion.delta.y * LOOK).clamp(-1.5, 1.5);
    }
    transform.rotation = Quat::from_euler(EulerRot::YXZ, shot.yaw, shot.pitch, 0.0);
    let fast = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        4.0
    } else {
        1.0
    };
    let fly = transform.rotation
        * Vec3::NEG_Z
        * held(
            [KeyCode::KeyW, KeyCode::ArrowUp],
            [KeyCode::KeyS, KeyCode::ArrowDown],
        )
        + transform.rotation
            * Vec3::X
            * held(
                [KeyCode::KeyD, KeyCode::ArrowRight],
                [KeyCode::KeyA, KeyCode::ArrowLeft],
            )
        + Vec3::Y
            * held(
                [KeyCode::KeyE, KeyCode::KeyE],
                [KeyCode::KeyQ, KeyCode::KeyQ],
            );
    transform.translation += fly * FLY_SPEED * fast * dt;
    if let Projection::Perspective(lens) = &mut *projection {
        let closer = held(
            [KeyCode::KeyZ, KeyCode::KeyZ],
            [KeyCode::KeyX, KeyCode::KeyX],
        ) * dt
            + scroll.delta.y * 0.05;
        lens.fov = (lens.fov * (1.0 - closer)).clamp(ZOOM.0, ZOOM.1);
    }

    if keys.any_just_pressed([KeyCode::Enter, KeyCode::Space]) {
        // The hint goes first, so the picture is of the race alone.
        for hint in &hints {
            commands.entity(hint).despawn();
        }
        let path = picture_path();
        info!("photo saved to {path}");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        sfx.play(id::MENU_CONFIRM);
    }
}

#[cfg(test)]
#[test]
fn a_replay_puts_cars_back_where_they_were() {
    let track = crate::track::Track::new();
    let mut replay = Replay::default();
    assert!(!replay.ready());
    replay.start();
    assert!(replay.showing.is_none());
    // Two cars going different ways for two seconds; the second is put out half way.
    let at = |slot: usize, time: f32| Vec3::new(time * 10.0, 0.0, slot as f32 * 5.0);
    for step in 0..=60 {
        replay.clock = step as f32 * INTERVAL;
        let poses = [0usize, 3].map(|slot| {
            let mut kart = Kart::new(&track, slot);
            (kart.pos, kart.steer) = (at(slot, replay.clock), replay.clock);
            kart.out = (slot == 3 && step >= 30).then_some(replay.clock);
            (slot, Pose::of(&kart))
        });
        replay.note(poses.into_iter(), Vec::new());
    }
    assert!((replay.length() - 2.0).abs() < 1e-4);
    // Between two notes a car is between its two places.
    for time in [0.0, 0.31, 1.234, 2.0] {
        let pose = replay.pose(3, time).unwrap();
        assert!(
            pose.pos.distance(at(3, time)) < 1e-3 && (pose.steer - time).abs() < 1e-3,
            "{time}"
        );
        assert_eq!(pose.out, time >= 1.0);
    }
    // Past either end it is at that end, and a slot nobody raced in has nothing.
    assert!(replay.pose(0, 9.0).unwrap().pos.distance(at(0, 2.0)) < 1e-3);
    assert!(replay.pose(0, -1.0).unwrap().pos.distance(at(0, 0.0)) < 1e-3);
    assert!(replay.pose(1, 1.0).is_none());
    replay.start();
    assert_eq!(replay.showing, Some(0.0));
    // It starts beside the road watching the player, and goes round the cars that raced.
    assert!(replay.trackside && replay.subject.is_none());
    replay.watch_next(3, 1);
    assert_eq!(replay.subject, Some(0));
    replay.watch_next(3, -1);
    assert_eq!(replay.subject, Some(3));
    // The places it is watched from are off the road, above it, and change along the lap.
    let mut kart = Kart::new(&track, 0);
    let stations: Vec<Vec3> = [10.0, 30.0, 80.0]
        .map(|s| {
            kart.s = s;
            Replay::station(&track, &kart)
        })
        .to_vec();
    assert_eq!(stations[0], stations[1]);
    assert!(stations[0].distance(stations[2]) > 40.0);
    let (_, _, lat) = track.project(stations[2], track.nearest(stations[2]));
    assert!(
        lat.abs() > track.road && stations[2].y > 2.0,
        "{lat} {}",
        stations[2]
    );
}

#[cfg(test)]
#[test]
fn a_replay_puts_power_ups_back_as_they_were() {
    use bevy::ecs::system::RunSystemOnce;
    use std::time::Duration;
    let mut world = World::new();
    let racing = Race {
        phase: Phase::Racing,
        intro: 0.0,
        countdown: 0.0,
        time: 0.0,
        demo: false,
        quick: true,
    };
    world.insert_resource(racing);
    world.insert_resource(Time::<()>::default());
    world.insert_resource(Replay {
        tail: TAIL,
        ..default()
    });
    let owner = world.spawn(Kart::new(&Track::new(), 0)).id();
    let step = |world: &mut World, system: fn(&mut World)| {
        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(INTERVAL));
        system(world);
    };
    // A second of racing; an oil slick lies on the road for the middle third of it.
    let mut slick = None;
    for frame in 0..30 {
        if frame == 10 {
            slick = Some(
                world
                    .spawn((
                        Action::OilSlick { owner, age: 0.0 },
                        Transform::from_xyz(5.0, 0.0, 7.0),
                    ))
                    .id(),
            );
        }
        if frame == 20 {
            world.despawn(slick.take().unwrap());
        }
        step(&mut world, |world| world.run_system_once(record).unwrap());
    }
    // Something the race left lying about when it ended.
    let left = world
        .spawn((
            Action::Explosion {
                age: 0.0,
                radius: 1.0,
                owner: None,
            },
            Transform::default(),
        ))
        .id();
    world.resource_mut::<Replay>().start();
    let mut seen = Vec::new();
    for _ in 0..40 {
        step(&mut world, |world| world.run_system_once(play).unwrap());
        let shown: Vec<Vec3> = world
            .query_filtered::<&Transform, With<Replayed>>()
            .iter(&world)
            .map(|t| t.translation)
            .collect();
        seen.push(shown);
    }
    // The slick is back for the frames it was there, where it was, and for no others;
    // the leftovers went as the replay began, and nothing of the replay's outlives it.
    assert!(world.get_entity(left).is_err());
    assert!(
        seen[2].is_empty() && seen[14] == [Vec3::new(5.0, 0.0, 7.0)] && seen[25].is_empty(),
        "{seen:?}"
    );
    assert_eq!(seen.iter().filter(|shown| !shown.is_empty()).count(), 10);
    assert!(world.resource::<Replay>().showing.is_none());
    assert_eq!(
        world
            .query_filtered::<(), With<Action>>()
            .iter(&world)
            .count(),
        0
    );
}
