//! Two ways of racing online that laps don't end: a battle and a game of tag. The
//! port's own, and the host's to choose (`menu::Mode`, in `protocol::Rules`).
//!
//! In a battle every car has `LIVES` lives. Anything that spins a car round or
//! blows it into the air takes one (`Kart::hits`), and for `SPARED` seconds after
//! nothing takes another: a blast throws a car for as long as it is in reach, and
//! that is one hit. A car with none left is out of the race; the last car in it
//! wins, or after `BATTLE_TIME` whoever has most left.
//!
//! In a game of tag every car has a clock of `TAG_TIME` seconds, and one car, drawn
//! at the start, is "it". Its clock runs down until it bumps another car, which is
//! "it" from then on, and can't hand it straight back for `TAG_BACK` seconds. The
//! car that is "it" is `IT_PACE` times as fast as the rest, so that it can catch
//! them, the computer's on their recorded lines too. The first clock to run out
//! ends the game: that car is last, and the rest come in the order of what their
//! clocks have left.
//!
//! Both are run on the host, as the rest of the race is, and what each car has left
//! (`Kart::stake`, `Kart::it`) goes to the players with its `state::Standing`.

use bevy::prelude::*;

use crate::kart::{Bumps, Kart};
use crate::menu::{Mode, Settings};
use crate::meshgen::Rng;
use crate::rules::put_out;
use crate::{Phase, Race};

/// The lives a car begins a battle with, and how long one is fought for.
pub const LIVES: f32 = 3.0;
pub const BATTLE_TIME: f32 = 240.0;
/// How long a car that has just lost a life can't lose another.
pub const SPARED: f32 = 1.5;
/// The seconds on each car's clock in a game of tag.
pub const TAG_TIME: f32 = 30.0;
/// How long a car that has just been made "it" can't make another so.
const TAG_BACK: f32 = 2.0;
/// How many times as fast as the rest the car that is "it" is.
pub const IT_PACE: f32 = 1.15;
/// Cars that end a game together are a thousandth of a second apart for each of
/// what they had left, so that the places they are given are in that order.
const APART: f32 = 1e-3;

/// How the game in hand is getting on, as its host has it.
#[derive(Resource, Default)]
pub struct Play {
    /// Every car has been given what it begins with.
    begun: bool,
    /// How many hits each car, by grid slot, had taken at the last step, and what
    /// is left of the time it is spared another life in.
    hits: [u32; crate::kart::PLAYER_SLOT + 1],
    spared: [f32; crate::kart::PLAYER_SLOT + 1],
    /// What is left of the time "it" can't be passed on in.
    held: f32,
}

/// Whether the cars are being raced: the race goes on for the rest after the car
/// driven on the host's screen is done.
fn on(race: &Race) -> bool {
    matches!(race.phase, Phase::Racing | Phase::Finished)
}

/// The cars still in it are home, in the order of what they have left.
fn end<'a>(karts: impl Iterator<Item = Mut<'a, Kart>>, time: f32) {
    for mut kart in karts.filter(|kart| kart.out.is_none() && kart.finished.is_none()) {
        kart.finished = Some(time - kart.stake * APART);
    }
}

/// A battle: a car hit loses a life, and one with none left is out.
pub fn battle(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    mut play: ResMut<Play>,
    mut karts: Query<&mut Kart>,
) {
    if settings.mode != Mode::Battle {
        return;
    }
    if !play.begun {
        play.begun = true;
        for mut kart in &mut karts {
            kart.stake = LIVES;
        }
    }
    if !on(&race) {
        return;
    }
    let dt = time.delta_secs();
    let play = &mut *play;
    for mut kart in &mut karts {
        let (Some(seen), Some(spared)) =
            (play.hits.get_mut(kart.slot), play.spared.get_mut(kart.slot))
        else {
            continue;
        };
        let hit = std::mem::replace(seen, kart.hits) != kart.hits;
        *spared = (*spared - dt).max(0.0);
        if !hit || *spared > 0.0 || kart.out.is_some() || kart.finished.is_some() {
            continue;
        }
        *spared = SPARED;
        kart.stake = (kart.stake - 1.0).max(0.0);
        debug!("{} has {} lives left", kart.name, kart.stake);
        if kart.stake <= 0.0 {
            put_out(&mut kart, race.time);
        }
    }
    let left = karts
        .iter()
        .filter(|kart| kart.out.is_none() && kart.finished.is_none())
        .count();
    if left == 1 || (left > 1 && race.time >= BATTLE_TIME) {
        end(karts.iter_mut(), race.time);
    }
}

/// A game of tag: "it" goes to whichever car the one that is "it" bumps, and the
/// game is over when a clock has run out.
pub fn tag(
    time: Res<Time>,
    race: Res<Race>,
    settings: Res<Settings>,
    bumps: Res<Bumps>,
    mut play: ResMut<Play>,
    mut dice: ResMut<Rng>,
    mut karts: Query<&mut Kart>,
) {
    if settings.mode != Mode::Tag {
        return;
    }
    if !play.begun {
        play.begun = true;
        let count = karts.iter().count();
        let drawn = (dice.f() * count as f32) as usize % count.max(1);
        for (n, mut kart) in karts.iter_mut().enumerate() {
            (kart.stake, kart.it) = (TAG_TIME, n == drawn);
        }
    }
    // Over once any car is out or home.
    if !on(&race)
        || karts
            .iter()
            .any(|kart| kart.out.is_some() || kart.finished.is_some())
    {
        return;
    }
    let dt = time.delta_secs();
    play.held = (play.held - dt).max(0.0);
    let Some(it) = karts.iter().find(|kart| kart.it).map(|kart| kart.slot) else {
        return;
    };
    let caught = bumps.0.iter().find_map(|&(a, b)| match (a == it, b == it) {
        (true, false) => Some(b),
        (false, true) => Some(a),
        _ => None,
    });
    if let Some(caught) = caught.filter(|_| play.held <= 0.0) {
        play.held = TAG_BACK;
        for mut kart in &mut karts {
            kart.it = kart.slot == caught;
            // The driver caught starts at it.
            kart.cues.hit |= kart.it;
        }
        return;
    }
    let mut ran_out = false;
    for mut kart in karts.iter_mut().filter(|kart| kart.it) {
        kart.stake = (kart.stake - dt).max(0.0);
        if kart.stake <= 0.0 {
            debug!("{}'s clock has run out at {:.1}s", kart.name, race.time);
            put_out(&mut kart, race.time);
            ran_out = true;
        }
    }
    if ran_out {
        end(karts.iter_mut(), race.time);
    }
}

/// What the display counts for a car where a race counts its laps: the word for it
/// and how many, if the race is a battle or a game of tag.
pub fn left(settings: &Settings, kart: &Kart) -> Option<(&'static str, String)> {
    match settings.mode {
        Mode::Battle => Some(("LIVES", (kart.stake as u32).to_string())),
        Mode::Tag if kart.it => Some(("IT", (kart.stake.ceil() as u32).to_string())),
        Mode::Tag => Some(("SAFE", (kart.stake.ceil() as u32).to_string())),
        Mode::Race | Mode::Elimination => None,
    }
}

/// The car that is "it" is quicker than the rest. Run on every game alike, so that a
/// player's game drives their own car as the host does.
pub fn pace(settings: Res<Settings>, mut karts: Query<&mut Kart>) {
    if settings.mode != Mode::Tag {
        return;
    }
    for mut kart in &mut karts {
        let pace = settings.pace() * if kart.it { IT_PACE } else { 1.0 };
        if kart.pace != pace {
            kart.pace = pace;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Track;
    use bevy::ecs::system::RunSystemOnce;

    fn world(mode: Mode, cars: usize) -> World {
        let track = Track::new();
        let mut world = World::new();
        let mut settings = Settings::new(&crate::menu::Circuits(Vec::new()));
        settings.mode = mode;
        world.insert_resource(settings);
        world.insert_resource(Race {
            phase: Phase::Racing,
            intro: 0.0,
            countdown: 0.0,
            time: 10.0,
            demo: false,
            quick: true,
        });
        world.insert_resource(Play::default());
        world.insert_resource(Bumps::default());
        world.insert_resource(Rng(7));
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs(1));
        world.insert_resource(time);
        for slot in 0..cars {
            world.spawn(Kart::new(&track, slot));
        }
        world
    }

    /// What the tests look at of a car.
    struct Seen {
        stake: f32,
        it: bool,
        out: Option<f32>,
        finished: Option<f32>,
        place: usize,
        pace: f32,
    }

    /// The cars, in the order of the grid.
    fn cars(world: &mut World) -> Vec<Seen> {
        let mut cars: Vec<&Kart> = world.query::<&Kart>().iter(world).collect();
        cars.sort_by_key(|kart| kart.slot);
        cars.iter()
            .map(|kart| Seen {
                stake: kart.stake,
                it: kart.it,
                out: kart.out,
                finished: kart.finished,
                place: kart.place,
                pace: kart.pace,
            })
            .collect()
    }

    fn car(world: &mut World, slot: usize) -> Mut<'_, Kart> {
        let mut query = world.query::<&mut Kart>();
        query
            .iter_mut(world)
            .find(|kart| kart.slot == slot)
            .expect("a car in the slot")
    }

    #[test]
    fn a_hit_takes_a_life_and_the_last_car_left_wins() {
        let mut world = world(Mode::Battle, 3);
        world.run_system_once(battle).unwrap();
        assert!(cars(&mut world).iter().all(|kart| kart.stake == LIVES));
        // A blast throws a car for as long as it is in reach, and a car that drives
        // for itself is over being thrown by the next step: that is one life all
        // the same, and not one for every step of it.
        for _ in 0..3 {
            assert!(car(&mut world, 0).launch(1.0));
            car(&mut world, 0).spin_out = 0.0;
        }
        world.run_system_once(battle).unwrap();
        assert_eq!(car(&mut world, 0).stake, LIVES - 1.0);
        // Thrown again at once it is spared; a step of a second on, it isn't, and
        // a spin costs it what a throw does. With no lives it is out.
        car(&mut world, 0).launch(1.0);
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
        world.run_system_once(battle).unwrap();
        assert_eq!(car(&mut world, 0).stake, LIVES - 1.0);
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs(2));
        world.run_system_once(battle).unwrap();
        for _ in 0..2 {
            car(&mut world, 0).spin = 0.0;
            car(&mut world, 0).spin_round(1.0);
            world.run_system_once(battle).unwrap();
        }
        let all = cars(&mut world);
        assert!(all[0].out.is_some() && all[0].finished.is_none());
        assert!(all[1].finished.is_none() && all[2].finished.is_none());
        // A second car out leaves the winner, who is home.
        car(&mut world, 1).stake = 1.0;
        car(&mut world, 1).spin_round(1.0);
        world.run_system_once(battle).unwrap();
        let all = cars(&mut world);
        assert!(all[1].out.is_some() && all[2].out.is_none());
        assert!(all[2].finished.is_some());
    }

    #[test]
    fn a_battle_out_of_time_goes_to_whoever_has_most_left() {
        let mut world = world(Mode::Battle, 3);
        world.run_system_once(battle).unwrap();
        car(&mut world, 1).stake = 1.0;
        car(&mut world, 2).stake = 2.0;
        world.resource_mut::<Race>().time = BATTLE_TIME;
        world.run_system_once(battle).unwrap();
        world.run_system_once(crate::kart::update_places).unwrap();
        let places: Vec<usize> = cars(&mut world).iter().map(|kart| kart.place).collect();
        assert_eq!(places, [1, 3, 2]);
    }

    #[test]
    fn it_is_passed_on_by_a_bump_and_not_straight_back() {
        let mut world = world(Mode::Tag, 3);
        world.run_system_once(tag).unwrap();
        let all = cars(&mut world);
        assert_eq!(all.iter().filter(|kart| kart.it).count(), 1);
        let it = all.iter().position(|kart| kart.it).unwrap();
        let (other, third) = ((it + 1) % 3, (it + 2) % 3);
        // Its clock has run for the one second of that step, and nobody else's.
        assert_eq!(all[it].stake, TAG_TIME - 1.0);
        assert_eq!(all[other].stake, TAG_TIME);
        // Two cars that aren't "it" bumping passes nothing.
        world.resource_mut::<Bumps>().0 = vec![(other, third)];
        world.run_system_once(tag).unwrap();
        assert!(car(&mut world, it).it);
        // A bump with the one that is does, either way round it is told.
        world.resource_mut::<Bumps>().0 = vec![(other, it)];
        world.run_system_once(tag).unwrap();
        assert!(car(&mut world, other).it && !car(&mut world, it).it);
        // A second later the same bump is too soon to pass it back; after two it does.
        world.run_system_once(tag).unwrap();
        assert!(car(&mut world, other).it);
        world.run_system_once(tag).unwrap();
        world.run_system_once(tag).unwrap();
        assert!(car(&mut world, it).it && !car(&mut world, other).it);
        // The car that is "it" is the quicker.
        world.run_system_once(pace).unwrap();
        let all = cars(&mut world);
        assert_eq!(all[it].pace, all[other].pace * IT_PACE);
    }

    #[test]
    fn the_first_clock_to_run_out_ends_the_game() {
        let mut world = world(Mode::Tag, 3);
        world.run_system_once(tag).unwrap();
        let it = cars(&mut world).iter().position(|kart| kart.it).unwrap();
        let (other, third) = ((it + 1) % 3, (it + 2) % 3);
        car(&mut world, it).stake = 0.5;
        car(&mut world, other).stake = 12.0;
        world.run_system_once(tag).unwrap();
        world.run_system_once(crate::kart::update_places).unwrap();
        let all = cars(&mut world);
        assert!(all[it].out.is_some());
        assert_eq!(
            (all[third].place, all[other].place, all[it].place),
            (1, 2, 3)
        );
        // Nothing more comes of it once it is over.
        world.resource_mut::<Bumps>().0 = vec![(other, third)];
        world.run_system_once(tag).unwrap();
        assert_eq!(car(&mut world, third).stake, TAG_TIME);
    }
}
