//! Time races: three laps alone against the clock, with two ghost cars for company —
//! Veronica Voltage's record run, from the circuit's `GHOST.GHB`, and the player's own
//! best. Follows `TimeRaceManager`.
//!
//! The port keeps the player's best runs between sessions, as files in
//! `$BRICK_GHOSTS` or `~/.brick_racers_ghosts`, one for each circuit and each way of
//! running it (mirrored, reversed).

use crate::assets::tokens::{Reader, Token};
use crate::kart::{Kart, Player};
use crate::menu::{Circuits, Settings};
use crate::opponent::facing;
use crate::scenery::to_world;
use crate::variant::Variant;
use crate::world::{KartModel, LoadedWorld};
use crate::{Phase, Race};
use bevy::prelude::*;
use std::collections::HashMap;

/// A run is sampled this often.
const SAMPLE_INTERVAL: f32 = 0.25;
/// Stored positions are in these, and rotations in these.
const POSITION: f32 = 1.0 / 32.0;
const ROTATION: f32 = 1.0 / 127.0;
/// How solid a ghost car is.
const GHOST_ALPHA: f32 = 0.45;
pub const LAPS: usize = 3;

/// A drive round a circuit: where the car was every quarter second, in our
/// coordinates, and how long each lap took.
#[derive(Clone, Default)]
pub struct Run {
    pub laps: [f32; LAPS],
    samples: Vec<(Vec3, Quat)>,
}

impl Run {
    pub fn parse(data: &[u8]) -> Option<Run> {
        let mut r = Reader::new(data);
        let mut run = Run::default();
        while let Some(token) = r.next() {
            match token {
                Token::Key(0x2a) => {
                    for lap in &mut run.laps {
                        *lap = r.int()? as f32 / 1000.0;
                    }
                }
                Token::Key(0x27) => {
                    for _ in 0..r.list_header()? {
                        let mut v = [0i32; 7];
                        for value in &mut v {
                            *value = r.int()?;
                        }
                        let position =
                            Vec3::new(v[0] as i16 as f32, v[1] as i16 as f32, v[2] as i16 as f32)
                                * POSITION;
                        let rotation =
                            Quat::from_array([3, 4, 5, 6].map(|i| v[i] as i8 as f32 * ROTATION));
                        run.samples
                            .push((to_world(position), facing(rotation.normalize())));
                    }
                }
                _ => {}
            }
        }
        (!run.samples.is_empty()).then_some(run)
    }

    /// As the lines of a file: the laps' times, then a sample to a line.
    fn write(&self) -> String {
        let mut out = format!("{} {} {}\n", self.laps[0], self.laps[1], self.laps[2]);
        for (p, q) in &self.samples {
            out += &format!("{} {} {} {} {} {} {}\n", p.x, p.y, p.z, q.x, q.y, q.z, q.w);
        }
        out
    }

    fn read(text: &str) -> Option<Run> {
        let mut lines = text.lines().map(|line| {
            line.split(' ')
                .map(str::parse::<f32>)
                .collect::<Result<Vec<_>, _>>()
        });
        let laps = lines.next()?.ok()?;
        let mut run = Run {
            laps: [*laps.first()?, *laps.get(1)?, *laps.get(2)?],
            samples: Vec::new(),
        };
        for line in lines {
            let &[x, y, z, qx, qy, qz, qw] = &line.ok()?[..] else {
                return None;
            };
            run.samples
                .push((Vec3::new(x, y, z), Quat::from_xyzw(qx, qy, qz, qw)));
        }
        (!run.samples.is_empty()).then_some(run)
    }

    pub fn total(&self) -> f32 {
        self.laps.iter().sum()
    }

    /// Where the car was `time` seconds in; `None` once the run is over.
    fn at(&self, time: f32) -> Option<(Vec3, Quat)> {
        let along = time.max(0.0) / SAMPLE_INTERVAL;
        let (from, to) = (
            self.samples.get(along as usize)?,
            self.samples.get(along as usize + 1)?,
        );
        Some((
            from.0.lerp(to.0, along.fract()),
            from.1.slerp(to.1, along.fract()),
        ))
    }
}

#[derive(Resource, Default)]
pub struct TimeRace {
    /// The run to beat, and the player's best on each circuit so far.
    pub record: Option<Run>,
    best: HashMap<String, Run>,
    /// The run in hand.
    pub run: Run,
    sample_due: f32,
    lap: i32,
    lap_started: f32,
    /// How the run in hand came out, once it is over: whether it beat the record.
    pub result: Option<bool>,
}

/// Where the player's best runs are kept.
fn ghost_folder() -> Option<std::path::PathBuf> {
    match std::env::var_os("BRICK_GHOSTS") {
        Some(folder) => Some(folder.into()),
        None => {
            Some(std::path::PathBuf::from(std::env::var_os("HOME")?).join(".brick_racers_ghosts"))
        }
    }
}

/// What a circuit's best run goes by: its folder (or a built-in circuit's key), and
/// which way round it was run.
fn run_key(circuits: &Circuits, settings: &Settings, variant: Variant) -> String {
    let circuit = &circuits.0[settings.circuit];
    format!(
        "{}{}",
        circuit.race.as_deref().unwrap_or(circuit.layout.key()),
        variant.suffix()
    )
}

/// A ghost car: which run it replays.
#[derive(Component)]
pub struct Ghost {
    record: bool,
}

/// Puts the ghosts on the grid of a time race.
pub fn spawn_ghosts(
    mut commands: Commands,
    settings: Res<Settings>,
    circuits: Res<Circuits>,
    variant: Res<Variant>,
    mut time_race: ResMut<TimeRace>,
    mut loaded: Option<ResMut<LoadedWorld>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    *time_race = TimeRace {
        best: std::mem::take(&mut time_race.best),
        ..default()
    };
    if !settings.time_race {
        return;
    }
    // A best run from an earlier session.
    let folder = run_key(&circuits, &settings, *variant);
    if !time_race.best.contains_key(&folder) {
        let saved = ghost_folder().and_then(|dir| std::fs::read_to_string(dir.join(&folder)).ok());
        if let Some(run) = saved.as_deref().and_then(Run::read) {
            time_race.best.insert(folder.clone(), run);
        }
    }
    let Some(loaded) = loaded.as_mut() else {
        return;
    };
    // The record was not set going round backwards.
    time_race.record = loaded.ghost.take().filter(|_| !variant.reverse);
    let runs = [
        (true, time_race.record.is_some()),
        (false, time_race.best.contains_key(&folder)),
    ];
    for (record, there) in runs {
        let Some(model) = loaded.ghost_models.pop().filter(|_| there) else {
            continue;
        };
        let ghost = commands
            .spawn((Ghost { record }, Transform::default(), Visibility::Hidden))
            .id();
        dress(
            &mut commands,
            ghost,
            model,
            &mut meshes,
            &mut materials,
            &mut images,
            Some(GHOST_ALPHA),
        );
    }
}

/// Hangs a car's model on an entity that faces -Z: body, driver and wheels. A `ghost`
/// is see-through by that much, and its wheels are not turned.
pub fn dress(
    commands: &mut Commands,
    car: Entity,
    model: KartModel,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    ghost: Option<f32>,
) {
    use crate::kart::{WHEEL_RADIUS, Wheel};
    use crate::physics::UNIT;
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
    let mut bundle = |surface| {
        let bundle = crate::world::surface_bundle(surface, meshes, materials, images);
        if let (Some(alpha), Some(mut material)) = (ghost, materials.get_mut(&bundle.1.0)) {
            material.base_color = Color::srgba(1.0, 1.0, 1.0, alpha);
            material.alpha_mode = AlphaMode::Blend;
        }
        bundle
    };
    commands.entity(car).with_children(|parent| {
        parent
            .spawn(part(Vec3::ZERO, model.body_scale))
            .with_children(|body| {
                for surface in model.body {
                    body.spawn(bundle(surface));
                }
            });
        parent
            .spawn(part(model.chassis.mount, model.driver_scale))
            .with_children(|figure| {
                for surface in model.driver {
                    figure.spawn(bundle(surface));
                }
            });
        parent
            .spawn(part(Vec3::ZERO, model.wheel_scale))
            .with_children(|wheels| {
                for axle in model.axles {
                    let wheel = Wheel {
                        kart: car,
                        front: axle.position.x > 0.0,
                        rest: axle.rotation,
                        steer_axis: Vec3::Z,
                        spin_axis: axle.rotation.inverse() * Vec3::NEG_Y,
                        spin_ratio: WHEEL_RADIUS / (axle.radius * model.wheel_scale * UNIT),
                    };
                    let transform =
                        Transform::from_translation(axle.position).with_rotation(axle.rotation);
                    wheels
                        .spawn((wheel, transform, Visibility::default()))
                        .with_children(|axle_entity| {
                            for surface in axle.surfaces {
                                axle_entity.spawn(bundle(surface));
                            }
                        });
                }
            });
    });
}

/// Runs the ghosts, records the player's run and settles it at the finish.
pub fn time_race(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    circuits: Res<Circuits>,
    variant: Res<Variant>,
    mut time_race: ResMut<TimeRace>,
    player: Query<&Kart, With<Player>>,
    mut ghosts: Query<(&Ghost, &mut Transform, &mut Visibility)>,
) {
    let (true, Ok(player)) = (settings.time_race, player.single()) else {
        return;
    };
    let folder = run_key(&circuits, &settings, *variant);
    let time_race = &mut *time_race;
    for (ghost, mut transform, mut visibility) in &mut ghosts {
        let run = if ghost.record {
            time_race.record.as_ref()
        } else {
            time_race.best.get(&folder)
        };
        let clock = if race.phase == Phase::Racing {
            race.time
        } else {
            0.0
        };
        match run
            .and_then(|run| run.at(clock))
            .filter(|_| race.phase != Phase::Finished)
        {
            Some((position, rotation)) => {
                (transform.translation, transform.rotation) = (position, rotation);
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
    match race.phase {
        Phase::Intro | Phase::Countdown => {
            (
                time_race.run,
                time_race.sample_due,
                time_race.lap,
                time_race.lap_started,
                time_race.result,
            ) = (Run::default(), 0.0, 1, 0.0, None);
        }
        Phase::Racing => {
            time_race.sample_due -= time.delta_secs();
            if time_race.sample_due <= 0.0 {
                time_race.sample_due += SAMPLE_INTERVAL;
                time_race.run.samples.push((player.pos, player.rot));
            }
            // A lap's time is taken as the next begins.
            if player.lap > time_race.lap && player.lap >= 2 {
                if let Some(lap) = time_race.run.laps.get_mut(player.lap as usize - 2) {
                    *lap = race.time - time_race.lap_started;
                }
                time_race.lap_started = race.time;
            }
            time_race.lap = time_race.lap.max(player.lap);
        }
        Phase::Finished if time_race.result.is_none() => {
            let total = time_race.run.total();
            if time_race
                .best
                .get(&folder)
                .is_none_or(|best| total < best.total())
            {
                if let Some(dir) = ghost_folder() {
                    let _ = std::fs::create_dir_all(&dir);
                    if let Err(error) = std::fs::write(dir.join(&folder), time_race.run.write()) {
                        warn!("could not keep the best run: {error}");
                    }
                }
                time_race.best.insert(folder, time_race.run.clone());
            }
            time_race.result = Some(
                time_race
                    .record
                    .as_ref()
                    .is_some_and(|record| total < record.total()),
            );
        }
        Phase::Finished => {}
    }
}

#[cfg(test)]
#[test]
fn a_run_comes_back_from_its_file_as_it_went() {
    let samples = (0..40)
        .map(|i| {
            (
                Vec3::new(i as f32 * 1.37, 0.25, -3.0),
                Quat::from_rotation_y(i as f32 * 0.1),
            )
        })
        .collect();
    let run = Run {
        laps: [31.25, 30.5, 33.125],
        samples,
    };
    let back = Run::read(&run.write()).unwrap();
    assert_eq!((back.laps, back.samples.len()), (run.laps, 40));
    assert_eq!(back.at(3.1).unwrap(), run.at(3.1).unwrap());
    // Anything else in the file, and there is no run.
    assert!(
        Run::read("").is_none()
            && Run::read("1 2 3\n").is_none()
            && Run::read("1 2 3\n4 5 six 7 8 9 10\n").is_none()
    );
}

#[cfg(test)]
#[test]
fn the_record_run_is_three_laps_from_the_grid() {
    let Some(jam) = crate::assets::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else {
        return;
    };
    let run = Run::parse(jam.get("/GAMEDATA/RACEC0R0/GHOST.GHB").unwrap()).unwrap();
    assert_eq!(run.laps, [32.266, 30.015, 33.066]);
    // Sampled every quarter second for as long as the laps took.
    assert!((run.samples.len() as f32 * SAMPLE_INTERVAL - run.total()).abs() < 1.0);
    let start = to_world(Vec3::new(356.978, 211.333, 0.3719));
    assert!(
        run.at(0.0).unwrap().0.distance(start) < 1.0,
        "{}",
        run.at(0.0).unwrap().0
    );
    // It moves the way it faces, and stops existing when it is done.
    let (here, rotation) = run.at(10.0).unwrap();
    let (there, _) = run.at(10.25).unwrap();
    assert!((there - here).normalize().dot(rotation * Vec3::NEG_Z) > 0.8);
    assert!(run.at(run.total() + 1.0).is_none());
}
