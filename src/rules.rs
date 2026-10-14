//! Ways to race that the original doesn't have: a circuit's bricks all of one colour,
//! taken away, of a colour picked afresh each time, changing as they lie or a colour
//! for the place of the car coming to them, and elimination, where the last car is put out each lap.

use crate::items::Power;
use crate::kart::{Kart, Player};
use crate::menu::Settings;
use crate::{Phase, Race};
use bevy::prelude::*;

/// Where a car that has been put out is kept: well under the circuit, out of reach of
/// everything on it.
const PARKED: Vec3 = Vec3::new(0.0, -1000.0, 0.0);

/// What a brick of the circuit's becomes under one of `menu::BRICK_RULES`, if it is
/// there at all. White bricks stay white.
pub fn brick(rule: usize, brick: Option<Power>) -> Option<Option<Power>> {
    let colours = [Power::Red, Power::Yellow, Power::Blue, Power::Green];
    match (rule, brick) {
        // Under the random rule the bricks stand where they did; `items` picks colours,
        // as it does those of a brick that changes or is a colour for who is coming.
        (0, brick) => Some(brick),
        (rule, brick) if rule >= crate::menu::RANDOM_BRICKS => Some(brick),
        (rule, _) if rule > colours.len() => None,
        (_, None) => Some(None),
        (rule, Some(_)) => Some(Some(colours[rule - 1])),
    }
}

/// Puts a car out of the race at `time`: it is gone from the circuit.
pub fn put_out(kart: &mut Kart, time: f32) {
    kart.out = Some(time);
    kart.pos += PARKED;
    kart.vel = Vec3::ZERO;
}

/// How many cars should have gone by the time the leader is on `lap`: one for each lap
/// the leader has finished.
fn gone_by(lap: i32) -> usize {
    (lap - 1).max(0) as usize
}

/// Puts the last car out each time the leader finishes a lap.
pub fn elimination(
    race: Res<Race>,
    settings: Res<Settings>,
    role: Res<crate::net::Role>,
    mut karts: Query<(&mut Kart, Has<Player>)>,
) {
    // Online the race goes on for the rest once the host's own car is out of it.
    let online = *role != crate::net::Role::Offline;
    let on = race.phase == Phase::Racing || (online && race.phase == Phase::Finished);
    if !settings.eliminating() || !on {
        return;
    }
    let running = karts.iter().filter(|(kart, _)| kart.out.is_none()).count();
    let gone = karts.iter().count() - running;
    let leader = karts
        .iter()
        .filter(|(kart, _)| kart.out.is_none())
        .map(|(kart, _)| kart.lap)
        .max()
        .unwrap_or(0);
    if running < 2 || gone >= gone_by(leader) {
        return;
    }
    let last = karts
        .iter_mut()
        .filter(|(kart, _)| kart.out.is_none())
        .max_by_key(|(kart, _)| kart.place);
    if let Some((mut kart, player)) = last {
        debug!(
            "{} is out in place {} at {:.1}s",
            kart.name, kart.place, race.time
        );
        put_out(&mut kart, race.time);
        // For a player alone that is the end of the race; online they watch the
        // rest of it, and the race isn't over for the others (`net::host::flow`).
        if player && !online {
            kart.finished = Some(race.time);
        }
    }
}

#[cfg(test)]
#[test]
fn bricks_follow_the_rule_and_cars_go_a_lap_at_a_time() {
    assert_eq!(brick(0, Some(Power::Blue)), Some(Some(Power::Blue)));
    assert_eq!(brick(1, Some(Power::Blue)), Some(Some(Power::Red)));
    assert_eq!(brick(4, Some(Power::Red)), Some(Some(Power::Green)));
    assert_eq!(brick(4, None), Some(None));
    assert_eq!(brick(5, Some(Power::Red)), None);
    assert_eq!(brick(5, None), None);
    for rule in [crate::menu::CHANGING_BRICKS, crate::menu::PLACED_BRICKS] {
        assert_eq!(brick(rule, Some(Power::Red)), Some(Some(Power::Red)));
        assert_eq!(brick(rule, None), Some(None));
    }
    assert_eq!(
        brick(crate::menu::RANDOM_BRICKS, Some(Power::Red)),
        Some(Some(Power::Red))
    );
    assert_eq!(crate::menu::BRICK_RULES.len(), 9);
    // Nobody goes on the grid or during the first lap; one has gone once the leader
    // starts the second, and with six cars the fifth lap's end leaves one.
    assert_eq!([0, 1, 2, 6].map(gone_by), [0, 0, 1, 5]);
}
