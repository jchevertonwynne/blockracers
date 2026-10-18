//! Drawing a road: the geometry of it, which `Draft` and the builder's screen
//! share. The port's own.
//!
//! A road is its corners, each rounded to a radius (`track::cornered`): it runs
//! straight from each towards the next and cuts each off in an arc, so that it
//! goes inside its corners and not through them. A line drawn with the pointer
//! is made into corners that the road comes round as the line went (`Stroke`):
//! each a little outside the line, and rounded as the line was drawn there. And
//! a corner is given no more rounding than there is room for between it and the
//! corners either side (`fitted`), so that no road drawn or dragged about is
//! one the game can't lay.
//! `rounded` is the middle of such a road as places, for one that is not yet a
//! loop and can't be built as a circuit: the builder draws that itself.

use bevy::prelude::*;

/// How far apart the places of a road's middle are.
const STEP: f32 = 3.0;
/// How far along a line drawn, either side of a place on it, is looked at for
/// how round it is there.
const ROUND: f32 = 12.0;
/// The least a corner drawn is rounded to, which is what a car going flat out
/// can be slid round, and the most; and what the end of a line drawn is rounded
/// to, as a corner clicked is, when the road goes on from it.
pub const ROUNDS: (f32, f32) = (18.0, 90.0);
pub const ROUNDED: f32 = 30.0;
/// A corner that turns back on itself by more than this many units of reach to
/// one of radius is rounded to nothing.
const SHARPEST: f32 = 40.0;

fn flat(corner: &[f32; 4]) -> Vec2 {
    Vec2::new(corner[0], corner[2])
}

/// How far a corner turns, to the left or the right, between the way into it and
/// the way out.
fn turn(before: Vec2, at: Vec2, after: Vec2) -> f32 {
    let (into, out) = (
        (at - before).normalize_or_zero(),
        (after - at).normalize_or_zero(),
    );
    into.perp_dot(out).atan2(into.dot(out))
}

/// How far along each of its straights a corner's rounding reaches for each unit
/// of its radius: nothing where the road goes straight on.
pub fn reach(before: Vec2, at: Vec2, after: Vec2) -> f32 {
    let turn = turn(before, at, after).abs();
    if turn < 1e-3 { 0.0 } else { (turn / 2.0).tan() }
}

/// What two corners may have of the straight between them, which is `room` long,
/// when they want so much of it each: what they want if there is room, and if
/// not, what the one that wants less than half wants and the rest for the other.
fn shared(first: f32, second: f32, room: f32) -> (f32, f32) {
    let half = room / 2.0;
    if first + second <= room {
        (first, second)
    } else if first <= half {
        (first, room - first)
    } else if second <= half {
        (room - second, second)
    } else {
        (half, half)
    }
}

/// The radius each corner of a road is rounded to: what is `wanted` of it, or as
/// much of that as there is room for on the straights either side of it. `kept`
/// is how much of the straight into the first corner and of the one out of it is
/// left alone, which is where a circuit's cars line up and start from.
pub fn fitted(corners: &[[f32; 4]], wanted: &[f32], kept: (f32, f32)) -> Vec<f32> {
    let n = corners.len();
    if n < 3 {
        return wanted.to_vec();
    }
    let at = |i: usize| flat(&corners[i % n]);
    let reaches: Vec<f32> = (0..n)
        .map(|i| reach(at(i + n - 1), at(i), at(i + 1)))
        .collect();
    // What each corner may have of the straight after it, and the next of the same.
    let rooms: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let next = (i + 1) % n;
            let kept = if next == 0 {
                kept.0
            } else if i == 0 {
                kept.1
            } else {
                0.0
            };
            let room = (at(i).distance(at(next)) - kept).max(0.0);
            shared(wanted[i] * reaches[i], wanted[next] * reaches[next], room)
        })
        .collect();
    (0..n)
        .map(|i| {
            if reaches[i] <= 0.0 {
                return wanted[i];
            }
            // To the hundredth below, but for what is lost in the sum: corners
            // that have all the room between them have exactly that.
            let most = (rooms[i].0.min(rooms[(i + n - 1) % n].1) + 0.002) / reaches[i];
            let radius = (most.min(wanted[i]) * 100.0).floor() / 100.0;
            if reaches[i] > SHARPEST {
                0.0
            } else {
                radius.max(0.0)
            }
        })
        .collect()
}

/// Whether a corner is one a car going flat out can't be slid round: rounded to
/// less than `tightest`, and turning enough to matter.
pub fn tight(corners: &[[f32; 4]], i: usize, tightest: f32) -> bool {
    let n = corners.len();
    let at = |i: usize| flat(&corners[i % n]);
    n >= 3 && corners[i][3] < tightest && turn(at(i + n - 1), at(i), at(i + 1)).abs() > 0.3
}

/// The middle of a road through `path`'s corners, each (x, height, z, radius), as
/// places a little way apart: all the way round if it is `closed`, when the last
/// place is the first again, and from its first corner to its last if it is not.
/// The road is as high at the middle of each corner as the corner is, and goes
/// evenly from one height to the next.
#[cfg(test)]
pub fn rounded(path: &[[f32; 4]], closed: bool) -> Vec<Vec3> {
    rounded_wide(path, &[], closed)
        .into_iter()
        .map(|(at, _)| at)
        .collect()
}

/// The same, with how far the road's barriers are from its middle at each place:
/// half of what `widths` has for each corner, or of `track::WIDTH` if it has
/// nothing, going from one corner's middle to the next's in a straight line, as
/// a circuit's road does (`track::cornered`).
pub fn rounded_wide(path: &[[f32; 4]], widths: &[f32], closed: bool) -> Vec<(Vec3, f32)> {
    let n = path.len();
    let half = |i: usize| widths.get(i).copied().unwrap_or(crate::track::WIDTH) / 2.0;
    if n < 2 || (closed && n < 3) {
        return path
            .iter()
            .enumerate()
            .map(|(i, corner)| (Vec3::new(corner[0], corner[1], corner[2]), half(i)))
            .collect();
    }
    let at = |i: usize| flat(&path[i % n]);
    // Where each corner's arc begins and ends, and how far round it turns: a
    // corner that is an end of the road has none.
    let arcs: Vec<(Vec2, Vec2, f32)> = (0..n)
        .map(|i| {
            if !closed && (i == 0 || i == n - 1) {
                return (at(i), at(i), 0.0);
            }
            let (before, after) = (at(i + n - 1), at(i + 1));
            let (turn, radius) = (turn(before, at(i), after), path[i][3]);
            if radius <= 0.0 || turn.abs() < 1e-3 {
                return (at(i), at(i), 0.0);
            }
            let reach = radius * (turn.abs() / 2.0).tan();
            let (into, out) = (
                (at(i) - before).normalize_or_zero(),
                (after - at(i)).normalize_or_zero(),
            );
            (at(i) - into * reach, at(i) + out * reach, turn)
        })
        .collect();
    // The road on the ground, how far along it each place is, and how far along
    // it the middle of each corner is.
    let (mut line, mut along): (Vec<Vec2>, Vec<f32>) = (Vec::new(), Vec::new());
    fn push(line: &mut Vec<Vec2>, along: &mut Vec<f32>, p: Vec2) {
        match line.last() {
            Some(&last) if last.distance(p) < 1e-3 => {}
            Some(&last) => {
                along.push(along[along.len() - 1] + last.distance(p));
                line.push(p);
            }
            None => {
                along.push(0.0);
                line.push(p);
            }
        }
    }
    let mut middles = Vec::with_capacity(n + 1);
    for i in 0..n {
        let (start, end, turn) = arcs[i];
        if turn == 0.0 {
            push(&mut line, &mut along, start);
            middles.push(along[along.len() - 1]);
        } else {
            let (into, radius) = ((at(i) - at(i + n - 1)).normalize_or_zero(), path[i][3]);
            let centre = start + into.perp() * turn.signum() * radius;
            let from = (start - centre).to_angle();
            let steps = ((turn.abs() * radius / STEP) as usize).max(2);
            for k in 0..=steps {
                push(
                    &mut line,
                    &mut along,
                    centre + Vec2::from_angle(from + turn * k as f32 / steps as f32) * radius,
                );
                if k == steps / 2 {
                    middles.push(along[along.len() - 1]);
                }
            }
        }
        if !closed && i == n - 1 {
            break;
        }
        let next = arcs[(i + 1) % n].0;
        let steps = ((end.distance(next) / STEP) as usize).max(1);
        for k in 1..=steps {
            push(
                &mut line,
                &mut along,
                end.lerp(next, k as f32 / steps as f32),
            );
        }
    }
    // A loop comes back to the middle of its first corner.
    let total = along[along.len() - 1];
    let heights: Vec<f32> = path.iter().map(|corner| corner[1]).collect();
    line.iter()
        .zip(&along)
        .map(|(p, &s)| {
            // The corner whose middle was last passed, and the one that is next:
            // before the middle of a loop's first corner, its last and its first.
            let (passed, s) = match middles.iter().rposition(|&middle| middle <= s + 1e-4) {
                Some(passed) => (passed, s),
                None => (n - 1, s + total),
            };
            let next = passed + 1;
            let (from, to) = match (next < n, closed) {
                (true, _) => (middles[passed], middles[next]),
                (false, true) => (middles[passed], middles[0] + total),
                (false, false) => (middles[passed], middles[passed] + 1.0),
            };
            let along = ((s - from) / (to - from).max(1e-3)).clamp(0.0, 1.0);
            let part = along * along * (3.0 - 2.0 * along);
            let after = if closed { next % n } else { next.min(n - 1) };
            let (low, high) = (heights[passed], heights[after]);
            let wide = half(passed) + (half(after) - half(passed)) * along;
            (Vec3::new(p.x, low + (high - low) * part, p.y), wide)
        })
        .collect()
}

/// A line being drawn by hand, for a road to follow.
pub struct Stroke {
    /// Where the road ended before it was begun.
    origin: Vec2,
    /// The last place on it that is a corner, and where the pointer has been
    /// since.
    from: Vec2,
    run: Vec<Vec2>,
    /// The places on it that are corners, and how round it was drawn at each.
    made: Vec<(Vec2, f32)>,
}

impl Stroke {
    /// One begun from where the road ends.
    pub fn begun(from: Vec2) -> Stroke {
        Stroke {
            origin: from,
            from,
            run: Vec::new(),
            made: Vec::new(),
        }
    }

    /// The pointer has come to `at`. That makes a corner when the way it has
    /// come since the last one has bent by more than `slack` from straight, at
    /// the place on that way furthest from straight, which is no nearer the last
    /// corner than `least`. Whether it made one.
    pub fn drawn(&mut self, at: Vec2, slack: f32, least: f32) -> bool {
        if self.run.last().unwrap_or(&self.from).distance(at) < 0.5 {
            return false;
        }
        self.run.push(at);
        let straight = at - self.from;
        let length = straight.length();
        if length < least {
            return false;
        }
        // How far each place the pointer has been is from the straight to where
        // it is now.
        let off = |p: &Vec2| {
            let along = ((*p - self.from).dot(straight) / (length * length)).clamp(0.0, 1.0);
            p.distance(self.from + straight * along)
        };
        let furthest = self
            .run
            .iter()
            .map(off)
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let Some((furthest, bend)) = furthest else {
            return false;
        };
        let corner = self.run[furthest];
        if bend <= slack || corner.distance(self.from) < least {
            return false;
        }
        self.made.push((corner, self.round(furthest)));
        self.from = corner;
        self.run.drain(..=furthest);
        true
    }

    /// How round the line was drawn at one of the places the pointer has been
    /// since the last corner: the radius of the bend through that place and the
    /// places a little way along the line either side of it.
    fn round(&self, place: usize) -> f32 {
        let line: Vec<Vec2> = std::iter::once(self.from)
            .chain(self.run.iter().copied())
            .collect();
        let here = line[place + 1];
        let far = |p: &&Vec2| p.distance(here) >= ROUND;
        let before = line[..=place].iter().rev().find(far).unwrap_or(&line[0]);
        let after = line[place + 2..]
            .iter()
            .find(far)
            .unwrap_or(&line[line.len() - 1]);
        let turn = turn(*before, here, *after).abs();
        let span = (before.distance(here) + here.distance(*after)) / 2.0;
        if turn < 0.05 {
            return ROUNDS.1;
        }
        (span / (2.0 * (turn / 2.0).sin())).clamp(ROUNDS.0, ROUNDS.1)
    }

    /// The corners of the road the line makes, each where it is and what it is
    /// rounded to: those the line has made, and the road's `end` if the pointer
    /// has gone any way from the last of them. A road cuts its corners off, so
    /// each is put outside the line by as much as the road will come inside it,
    /// or by `slack` at the most, for the road to come round as the line did.
    /// `beyond` is where the road goes on to after the last of them, if anywhere.
    pub fn corners(
        &self,
        end: Option<Vec2>,
        beyond: Option<Vec2>,
        slack: f32,
        least: f32,
    ) -> Vec<(Vec2, f32)> {
        let mut places = self.made.clone();
        places.extend(
            end.filter(|end| end.distance(self.from) >= least * 0.4)
                .map(|end| (end, ROUNDED)),
        );
        let count = places.len();
        (0..count)
            .map(|i| {
                let (here, round) = places[i];
                let before = if i == 0 { self.origin } else { places[i - 1].0 };
                let Some(after) = places.get(i + 1).map(|next| next.0).or(beyond) else {
                    return (here, round);
                };
                let (into, out) = (
                    (here - before).normalize_or_zero(),
                    (after - here).normalize_or_zero(),
                );
                let turn = turn(before, here, after).abs();
                // As the corners of a shape drawn round a circle are outside
                // those of the same shape drawn in it.
                let leg = before.distance(here).min(here.distance(after));
                let out_by =
                    (leg * (turn / 4.0).tan() / (2.0 * (turn / 2.0).cos().max(0.2))).min(slack);
                (here + (into - out).normalize_or_zero() * out_by, round)
            })
            .collect()
    }
}

/// How far a place is from the nearest of a road's middle, over the ground.
#[cfg(test)]
fn off(road: &[Vec3], at: Vec2) -> f32 {
    road.iter()
        .map(|p| Vec2::new(p.x, p.z).distance(at))
        .fold(f32::MAX, f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// The corners of the road a line drawn through `places` makes, from where
    /// the line began.
    fn stroked(places: &[Vec2]) -> Vec<[f32; 4]> {
        let mut stroke = Stroke::begun(places[0]);
        for &at in &places[1..] {
            stroke.drawn(at, 5.0, 20.0);
        }
        let corners = stroke.corners(places.last().copied(), None, 5.0, 20.0);
        let from = std::iter::once((places[0], 0.0));
        from.chain(corners)
            .map(|(at, round)| [at.x, 0.0, at.y, round])
            .collect()
    }

    /// Rounds corners to what they want or what there is room for.
    fn fit(corners: &mut [[f32; 4]]) {
        let wanted: Vec<f32> = corners.iter().map(|corner| corner[3]).collect();
        let radii = fitted(corners, &wanted, (0.0, 0.0));
        for (corner, radius) in corners.iter_mut().zip(radii) {
            corner[3] = radius;
        }
    }

    #[test]
    fn a_straight_line_drawn_with_an_unsteady_hand_is_one_straight() {
        let places: Vec<Vec2> = (0..=200)
            .map(|k| Vec2::new(k as f32, (k as f32 * 0.7).sin() * 2.0))
            .collect();
        let corners = stroked(&places);
        assert_eq!(corners.len(), 2, "{corners:?}");
    }

    #[test]
    fn a_square_corner_drawn_is_one_corner_and_the_road_is_rounded_inside_it() {
        let east = (0..=100).map(|k| Vec2::new(k as f32, 0.0));
        let south = (1..=100).map(|k| Vec2::new(100.0, k as f32));
        let places: Vec<Vec2> = east.chain(south).collect();
        let mut corners = stroked(&places);
        assert_eq!(corners.len(), 3, "{corners:?}");
        // The corner is outside where it was drawn, and as sharp as a car can
        // be slid round, the line having been drawn sharper.
        assert!(
            flat(&corners[1]).distance(Vec2::new(103.5, -3.5)) < 2.0,
            "{corners:?}"
        );
        assert_eq!(corners[1][3], ROUNDS.0);
        fit(&mut corners);
        let road = rounded(&corners, false);
        // It begins and ends where the line did, and cuts the corner off close
        // inside it.
        assert!(off(&road, Vec2::ZERO) < 0.1 && off(&road, Vec2::new(100.0, 100.0)) < 0.1);
        let cut = off(&road, Vec2::new(100.0, 0.0));
        assert!((1.0..5.0).contains(&cut), "{cut}");
    }

    #[test]
    fn a_round_bend_drawn_is_followed_closely_by_a_few_corners() {
        // Three quarters of a circle of a hundred and fifty, of sixty and of
        // twenty, which is a hairpin.
        for (radius, most, near) in [(150.0, 24, 3.0), (60.0, 16, 3.0), (20.0, 10, 3.0)] {
            // Each with a straight into it and a straight out of it, as a bend
            // in a road has.
            let into = (0..40).map(|k| Vec2::new(radius, k as f32 - 40.0));
            let round =
                (0..=270).map(|degree| Vec2::from_angle(degree as f32 * TAU / 360.0) * radius);
            let out = (1..=40).map(|k| Vec2::new(k as f32, -radius));
            let places: Vec<Vec2> = into.chain(round).chain(out).collect();
            let mut corners = stroked(&places);
            assert!(
                (4..=most).contains(&corners.len()),
                "{radius}: {}",
                corners.len()
            );
            fit(&mut corners);
            let road = rounded(&corners, false);
            let furthest = places.iter().map(|&at| off(&road, at)).fold(0.0, f32::max);
            assert!(
                furthest < near,
                "{radius}: the road is {furthest} from what was drawn"
            );
            // And is no tighter anywhere than the line was, or than a car can
            // be slid round.
            let tightest = corners[1..corners.len() - 1]
                .iter()
                .map(|corner| corner[3])
                .fold(f32::MAX, f32::min);
            assert!(
                tightest >= radius.min(ROUNDS.1) * 0.75,
                "{radius}: rounded to {tightest}"
            );
        }
    }

    #[test]
    fn corners_are_rounded_to_what_there_is_room_for() {
        // Two square corners forty apart, each wanting thirty: twenty each.
        let corners = [
            [0.0, 0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0, 0.0],
            [100.0, 0.0, 40.0, 0.0],
            [0.0, 0.0, 40.0, 0.0],
        ];
        let radii = fitted(&corners, &[30.0; 4], (0.0, 0.0));
        assert_eq!(radii, [20.0; 4]);
        // One that wants little leaves the rest to the next.
        let radii = fitted(&corners, &[30.0, 5.0, 60.0, 30.0], (0.0, 0.0));
        assert!(
            radii[1] == 5.0 && radii[2] == 35.0 && radii[0] == 20.0 && radii[3] == 20.0,
            "{radii:?}"
        );
        // With room, each has what it wants; and what is kept by the first is not had.
        let wide = corners.map(|[x, y, z, r]| [x * 3.0, y, z * 3.0, r]);
        assert_eq!(fitted(&wide, &[30.0; 4], (0.0, 0.0)), [30.0; 4]);
        let radii = fitted(&wide, &[0.0, 30.0, 30.0, 30.0], (100.0, 290.0));
        assert!(radii[3] == 20.0 && radii[1] == 10.0, "{radii:?}");
        // A corner on a straight is rounded to nothing that matters, and one
        // that turns straight back to nothing.
        let straight = [
            [0.0, 0.0, 0.0, 0.0],
            [50.0, 0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0, 0.0],
            [50.0, 0.0, 80.0, 0.0],
        ];
        assert_eq!(fitted(&straight, &[30.0; 4], (0.0, 0.0))[1], 30.0);
        let back = [
            [0.0, 0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.1, 0.0],
            [50.0, 0.0, 80.0, 0.0],
        ];
        assert_eq!(fitted(&back, &[30.0; 4], (0.0, 0.0))[1], 0.0);
    }

    #[test]
    fn a_road_all_the_way_round_comes_back_to_where_it_began() {
        let corners = [
            [0.0, 0.0, 0.0, 20.0],
            [100.0, 4.0, 0.0, 20.0],
            [100.0, 8.0, 80.0, 20.0],
            [0.0, 4.0, 80.0, 20.0],
        ];
        let road = rounded(&corners, true);
        assert!(road[0].distance(*road.last().unwrap()) < 0.01);
        // As high at each corner's middle as the corner is, and never higher or
        // lower than the two it is between.
        assert!(road.iter().all(|p| (0.0..=8.0).contains(&p.y)));
        let at = |x: f32, z: f32| {
            road.iter().min_by(|a, b| {
                let off = |p: &Vec3| Vec2::new(p.x - x, p.z - z).length();
                off(a).total_cmp(&off(b))
            })
        };
        assert!(
            (at(100.0, 80.0).unwrap().y - 8.0).abs() < 0.1 && at(0.0, 0.0).unwrap().y.abs() < 0.1
        );
    }
}
