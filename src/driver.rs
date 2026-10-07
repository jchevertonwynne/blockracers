//! The driver in a car, who doesn't sit still: leaning into the corners, turning to
//! look at whoever comes alongside or up behind, looking back to reverse, starting at
//! a knock, and at the end of the race celebrating or not. Follows
//! `CarVisuals::UpdateDriver`, which picks among the eighteen parts of the animation
//! every driver shares (`pelvis`, on `DriverCosmeticTable`'s scene node), and
//! `GolAnimatedEntity::TransitionToPart` for the easing between the steering ones.
//!
//! The figure is a rigged model hung on its car (`kart::spawn_karts`); cars shown
//! without one of those (ghosts, the films', the build menu's) keep the figure as it
//! sits at rest.

use crate::kart::{Controls, Kart};
use crate::physics::UNIT;
use crate::scenery::Animated;
use bevy::prelude::*;

/// The parts of the drivers' animation, as `UpdateDriver` uses them.
mod part {
    /// Thrown forward as the car stops dead.
    pub const JOLT: usize = 0;
    /// Looking back over a shoulder, the turn to it and the turn back from it.
    pub const LOOK_BACK: usize = 2;
    pub const LOOKING_BACK: usize = 3;
    pub const LOOK_AHEAD: usize = 4;
    /// Leaning with the steering, for a turn of a negative radius and of a positive.
    pub const STEER: [usize; 2] = [5, 6];
    /// A look at a car alongside and at one behind, to the one side.
    pub const GLANCE: [usize; 2] = [7, 8];
    pub const DRIVING: usize = 9;
    /// Struck by something.
    pub const HIT: usize = 10;
    /// The race lost: slumping, and slumped.
    pub const LOSE: usize = 11;
    pub const LOST: usize = 12;
    /// The race won, one way and the other (which has a part to settle into).
    pub const WIN: usize = 13;
    pub const CHEER: usize = 14;
    pub const CHEERING: usize = 15;
    /// The same looks to the other side.
    pub const GLANCE_OTHER: [usize; 2] = [16, 17];
}

/// `c_animationTransitionMs`.
const EASE: f32 = 300.0;
/// A car going faster than the first that is at once slower than the second has
/// stopped dead (`g_unk0x004b0544` and the 0.01 beside it; game units a millisecond).
const STOPPED_DEAD: (f32, f32) = (0.05, 0.01);
/// `FindNearestRacerInRange`: a driver looks at the nearest car between these
/// distances, in game units.
const GLANCE_RANGE: (f32, f32) = (1.4142135 * UNIT, 200.0 * UNIT);
/// How far ahead of or behind the driver the other car is, as the cosine of the
/// angle to it: alongside between the first two, behind between the last two
/// (`g_unk0x004b02e0`, `g_lookAtDotBeside`, `g_lookAtDotBehind`).
const GLANCE_DOTS: (f32, f32, f32) = (0.2, -0.2, -0.6);
/// `c_avoidanceCooldownBaseMs` and `c_avoidanceCooldownRangeMs`, in seconds.
const GLANCE_WAIT: (f32, f32) = (5.0, 1.0);

/// A car's driver: whose it is, how fast the car was going a step ago, how long
/// until it may look about again, and its luck.
#[derive(Component)]
pub struct Driver {
    car: Entity,
    /// Set down in its seat, driving (`CarVisuals::Initialize` plays that part).
    seated: bool,
    speed: f32,
    wait: f32,
    luck: u32,
}

impl Driver {
    pub fn of(car: Entity, slot: usize) -> Self {
        Driver {
            car,
            seated: false,
            speed: 0.0,
            wait: 0.0,
            luck: 0x9e37_79b9 ^ (slot as u32 + 1).wrapping_mul(0x85eb_ca6b),
        }
    }

    fn roll(&mut self, of: u32) -> u32 {
        self.luck = self.luck.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.luck >> 16) % of
    }
}

/// What a driver sees of its own car and of the nearest other.
pub struct Seen {
    pub finished: bool,
    pub place: usize,
    /// Forward speed, in game units a millisecond.
    pub speed: f32,
    pub struck: bool,
    /// The accelerator is asking for reverse.
    pub backing: bool,
    pub turn_radius: f32,
    /// The way to the nearest other car in range, in the car's own frame: how far
    /// ahead it is and how far to the left, of one.
    pub other: Option<(f32, f32)>,
}

/// `CarVisuals::UpdateDriver`, after the animation has been run on: sets the driver
/// to whatever part what it sees calls for. True if the car stopped dead, which the
/// driver has something to say about (`Racer::PlayReaction`).
pub fn react(driver: &mut Driver, figure: &mut Animated, seen: &Seen, dt: f32) -> bool {
    let active = figure.part;
    let once = |figure: &mut Animated, part: usize, then: usize| {
        figure.play(part, false);
        figure.queued = Some((then, true));
    };
    driver.wait = (driver.wait - dt).max(0.0);
    if seen.finished {
        if seen.place == 1 {
            if ![part::WIN, part::CHEER, part::CHEERING].contains(&active) {
                if driver.roll(2) != 0 {
                    once(figure, part::CHEER, part::CHEERING);
                } else {
                    figure.play(part::WIN, true);
                }
            }
        } else if ![part::LOSE, part::LOST].contains(&active) {
            once(figure, part::LOSE, part::LOST);
        }
        return false;
    }
    if driver.speed > STOPPED_DEAD.0 && seen.speed < STOPPED_DEAD.1 && active != part::JOLT {
        once(figure, part::JOLT, part::DRIVING);
        driver.speed = 0.0;
        return true;
    }
    driver.speed = seen.speed;
    if seen.struck {
        once(figure, part::HIT, part::DRIVING);
        return false;
    }
    if [part::JOLT, 1, part::HIT, part::WIN, part::CHEERING, part::LOST].contains(&active) {
        return false;
    }
    if seen.backing && seen.speed < 0.0 {
        if ![part::LOOK_BACK, part::LOOKING_BACK].contains(&active) {
            once(figure, part::LOOK_BACK, part::LOOKING_BACK);
        }
        return false;
    }
    if active == part::LOOKING_BACK {
        once(figure, part::LOOK_AHEAD, part::DRIVING);
        return false;
    }
    if part::GLANCE.contains(&active) || part::GLANCE_OTHER.contains(&active) {
        return false;
    }
    if let (true, Some((ahead, left))) = (driver.wait == 0.0, seen.other) {
        let which = if ahead < GLANCE_DOTS.0 && ahead > GLANCE_DOTS.1 {
            Some(0)
        } else if ahead <= GLANCE_DOTS.1 && ahead > GLANCE_DOTS.2 {
            Some(1)
        } else {
            None
        };
        if let Some(which) = which {
            // `row1 . direction < 0`: the car is on the side its second row points
            // away from.
            let glance = if left < 0.0 { part::GLANCE } else { part::GLANCE_OTHER };
            driver.wait = GLANCE_WAIT.0 + driver.roll(1000) as f32 / 1000.0 * GLANCE_WAIT.1;
            once(figure, glance[which], part::DRIVING);
            return false;
        }
    }
    let wanted = if seen.turn_radius < 0.0 {
        part::STEER[0]
    } else if seen.turn_radius > 0.0 {
        part::STEER[1]
    } else {
        part::DRIVING
    };
    if active != wanted {
        figure.transition(wanted, EASE);
    }
    false
}

/// Has every car's driver do what `react` says, and a remark made by one whose car
/// has run into something.
pub fn drivers(
    time: Res<Time>,
    mut karts: Query<(Entity, &mut Kart, &Controls)>,
    mut figures: Query<(&mut Driver, &mut Animated)>,
) {
    let dt = time.delta_secs();
    let places: Vec<(Entity, Vec3)> = karts.iter().map(|(car, kart, _)| (car, kart.pos)).collect();
    for (mut driver, mut figure) in &mut figures {
        let Ok((car, mut kart, controls)) = karts.get_mut(driver.car) else {
            continue;
        };
        if !std::mem::replace(&mut driver.seated, true) {
            figure.play(part::DRIVING, true);
        }
        let (forward, left) = (kart.rot * Vec3::NEG_Z, kart.rot * Vec3::NEG_X);
        let other = places
            .iter()
            .filter(|(other, _)| *other != car)
            .map(|(_, at)| *at - kart.pos)
            .filter(|to| (GLANCE_RANGE.0..=GLANCE_RANGE.1).contains(&to.length()))
            .min_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))
            .map(|to| to.normalize())
            .map(|to| (to.dot(forward), to.dot(left)));
        let seen = Seen {
            finished: kart.finished.is_some(),
            place: kart.place,
            speed: kart.vel.dot(forward) / UNIT / 1000.0,
            struck: std::mem::take(&mut kart.struck),
            backing: controls.throttle < 0.0,
            turn_radius: kart.turn_radius,
            other,
        };
        let before = figure.part;
        if react(&mut driver, &mut figure, &seen, dt) {
            kart.cues.reaction = Some(false);
        }
        if figure.part != before {
            debug!("driver {}: part {before} to {} at speed {:.3}", kart.slot, figure.part, seen.speed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen() -> Seen {
        Seen {
            finished: false,
            place: 3,
            speed: 0.1,
            struck: false,
            backing: false,
            turn_radius: 0.0,
            other: None,
        }
    }

    /// A driver of the game's own, driving.
    fn driver() -> Option<(Driver, Animated)> {
        let jam = crate::world::jam()?;
        let rig = crate::world::driver_rig(&jam)?;
        let mut figure = Animated {
            rig,
            part: 0,
            time: 0.0,
            playing: true,
            looping: true,
            rate: 1.0,
            queued: None,
            easing: None,
        };
        figure.play(part::DRIVING, true);
        Some((Driver::of(Entity::PLACEHOLDER, 0), figure))
    }

    #[test]
    fn the_animation_every_driver_shares_has_all_the_parts_a_driver_plays() {
        let Some((_, figure)) = driver() else { return };
        assert!(figure.rig.animation.parts.len() > part::GLANCE_OTHER[1]);
    }

    #[test]
    fn a_driver_leans_with_the_wheel_starts_at_a_knock_and_looks_about() {
        let Some((mut driver, mut figure)) = driver() else { return };
        let dt = 1.0 / 60.0;
        // Into a turn, easing; and out of it again.
        let turning = Seen { turn_radius: -30.0, ..seen() };
        assert!(!react(&mut driver, &mut figure, &turning, dt));
        assert!(figure.part == part::STEER[0] && figure.easing.is_some());
        react(&mut driver, &mut figure, &seen(), dt);
        assert_eq!(figure.part, part::DRIVING);
        // Struck: a start, and then back to driving.
        react(&mut driver, &mut figure, &Seen { struck: true, ..seen() }, dt);
        assert_eq!((figure.part, figure.queued), (part::HIT, Some((part::DRIVING, true))));
        // While it starts, the wheel doesn't have it lean.
        react(&mut driver, &mut figure, &turning, dt);
        assert_eq!(figure.part, part::HIT);
        figure.play(part::DRIVING, true);
        // A car alongside on the left gets a look, and the next one waits its turn.
        let beside = Seen { other: Some((0.0, 1.0)), ..seen() };
        react(&mut driver, &mut figure, &beside, dt);
        assert_eq!(figure.part, part::GLANCE_OTHER[0]);
        figure.play(part::DRIVING, true);
        react(&mut driver, &mut figure, &Seen { other: Some((-0.4, -1.0)), ..seen() }, dt);
        assert_eq!(figure.part, part::DRIVING);
        driver.wait = 0.0;
        react(&mut driver, &mut figure, &Seen { other: Some((-0.4, -1.0)), ..seen() }, dt);
        assert_eq!(figure.part, part::GLANCE[1]);
        // One dead ahead gets none.
        figure.play(part::DRIVING, true);
        driver.wait = 0.0;
        react(&mut driver, &mut figure, &Seen { other: Some((0.9, 0.1)), ..seen() }, dt);
        assert_eq!(figure.part, part::DRIVING);
    }

    #[test]
    fn a_driver_is_thrown_forward_by_a_dead_stop_and_says_so() {
        let Some((mut driver, mut figure)) = driver() else { return };
        let dt = 1.0 / 60.0;
        react(&mut driver, &mut figure, &seen(), dt);
        assert!(react(&mut driver, &mut figure, &Seen { speed: 0.0, ..seen() }, dt));
        assert_eq!(figure.part, part::JOLT);
        // Once, not for as long as the car stands.
        assert!(!react(&mut driver, &mut figure, &Seen { speed: 0.0, ..seen() }, dt));
    }

    #[test]
    fn a_driver_looks_back_to_reverse_and_ends_the_race_as_it_went() {
        let Some((mut driver, mut figure)) = driver() else { return };
        let dt = 1.0 / 60.0;
        let backing = Seen { backing: true, speed: -0.02, ..seen() };
        driver.speed = -0.02;
        react(&mut driver, &mut figure, &backing, dt);
        assert_eq!((figure.part, figure.queued), (part::LOOK_BACK, Some((part::LOOKING_BACK, true))));
        figure.play(part::LOOKING_BACK, true);
        react(&mut driver, &mut figure, &Seen { speed: 0.02, ..seen() }, dt);
        assert_eq!(figure.part, part::LOOK_AHEAD);
        // Fourth home: a slump. First: one of the two celebrations, and it stays in it.
        react(&mut driver, &mut figure, &Seen { finished: true, place: 4, ..seen() }, dt);
        assert_eq!(figure.part, part::LOSE);
        react(&mut driver, &mut figure, &Seen { finished: true, place: 1, ..seen() }, dt);
        assert!([part::WIN, part::CHEER].contains(&figure.part));
        let settled = figure.part;
        react(&mut driver, &mut figure, &Seen { finished: true, place: 1, ..seen() }, dt);
        assert_eq!(figure.part, settled);
    }
}
