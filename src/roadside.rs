//! What stands beside the roads of the circuits built here (`map::Map`), so that
//! little is seen from a car but the road it is on. The port's own.
//!
//! Folded in on themselves as those circuits are, a car on one stretch of road looks
//! across at two or three others, each with its striped barriers, its cars and its
//! hazards, and past them at whatever is scattered about. So half way between each
//! two stretches that run beside each other on the level stands a wall, as high as
//! hides the one from the other; and along the outside of a stretch with nothing
//! beside it runs a hedge, a little way back from the road, which hides what is
//! beyond it (`Fence`). Neither is solid: both are behind the road's barriers, where
//! no car comes. The walls at the corners themselves, which a car can reach, are
//! `track`'s (`Track::screens`).
//!
//! What a driver wants to see is left to be seen. Where the road goes next is one
//! such thing: so the wall between the way into a hairpin and the way out of it is
//! low for the last of the one and the first of the other, low enough to be seen
//! over from behind a car, as the wall inside a corner is, and a hedge on the inside
//! of a bend is low too; it is what is on the outside of a corner, and between
//! stretches that have nothing to do with each other, that is high. And nothing at all stands near a crossing on the level, where the other
//! road's cars are to be seen coming; across the mouth of a byway or the grass
//! inside a corner; or where a hazard has something standing (`Track::clearings`).

use crate::meshgen::*;
use crate::track::{INSIDE_CLEAR, SCREEN_TURN, STRETCH, Track, WALL, xz_dist2};
use bevy::prelude::*;
use serde::Deserialize;

/// How thick a wall is, and a hedge.
const WALL_THICK: f32 = 1.0;
const HEDGE_THICK: f32 = 2.4;
/// How high either stands over the road it is beside, to hide what is behind it:
/// more than the camera behind a car is over the road. And how high one is that is
/// to be seen over.
const HEIGHT: f32 = 6.0;
const LOW: f32 = 1.4;
/// Two stretches no further apart than this round the lap are the two ends of one
/// corner, and the wall between them is low: the last of the way into a hairpin and
/// the first of the way out of it, with the hairpin between.
const SOON: f32 = 110.0;
/// A bend tighter than this has a low hedge on the inside of it, from so many
/// samples before the bend to so many after.
const INSIDE_TURN: f32 = 1.0 / 60.0;
const INSIDE_ROUND: (usize, usize) = (20, 10);
/// The nearest two stretches of road can be and have a wall between them, and the
/// furthest: further off than that the other is left to the hedges.
const NEAREST: f32 = 2.0 * (WALL + 0.8) + WALL_THICK + 0.8;
const FURTHEST: f32 = 64.0;
/// How nearly level two stretches are that have a wall between them; and how far
/// ahead of a place or behind it the other stretch may be, for each unit it is out
/// to the side, and still be beside it.
const LEVEL: f32 = 4.0;
const ABEAM: f32 = 0.6;
/// How far from the middle of a road its hedge is, and how high off the ground a
/// road may be and have one: under a road that is higher, a hedge hides nothing.
const HEDGE_OUT: f32 = WALL + 6.0;
const HEDGE_ROAD: f32 = 1.5;
/// Nothing stands this near where the road crosses itself on the level.
const CROSSING_SIGHT: f32 = 44.0;
/// The shortest wall or hedge worth building, from end to end; how far apart the
/// places it is drawn through are; and how far apart two of them may be and be of
/// one wall.
const SHORTEST: f32 = 12.0;
const STEP: f32 = 3.0;
const JOINED: f32 = 8.0;
/// How near a wall or a hedge a place is that is taken to have one already.
const TAKEN: f32 = 5.0;
/// A hedge's two greens.
const HEDGE: Color = Color::srgb(0.07, 0.31, 0.15);
const HEDGE_TOP: Color = Color::srgb(0.12, 0.42, 0.20);

/// A wall or a hedge: the places along the ground its middle goes through, each with
/// the height of its top there, and whether it is low there, to be seen over.
pub struct Fence {
    pub line: Vec<Vec3>,
    pub low: Vec<bool>,
    pub hedge: bool,
}

/// What a circuit's walls are built of, as its map says (`walls`); one that says
/// nothing has the fair's.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    #[serde(deserialize_with = "crate::map::colour")]
    wall: Color,
    #[serde(deserialize_with = "crate::map::colour")]
    footing: Color,
    #[serde(deserialize_with = "crate::map::colour")]
    coping: Color,
    /// What caps the piers that stand along it.
    #[serde(deserialize_with = "crate::map::colour")]
    cap: Color,
    battlements: bool,
}

impl Default for Look {
    fn default() -> Look {
        Look {
            wall: ORANGE,
            footing: BROWN,
            coping: WHITE,
            cap: BLUE,
            battlements: false,
        }
    }
}

/// The sample of another stretch of road that is beside sample `i` on one side of it
/// and on its level, and how far off it is.
fn beside(track: &Track, i: usize, side: f32) -> Option<(usize, f32)> {
    let (here, out, along) = (track.pts[i], track.right[i] * side, track.flat[i]);
    (0..track.n())
        .filter(|&k| track.apart(i, k) > STRETCH && (track.pts[k].y - here.y).abs() < LEVEL)
        .filter_map(|k| {
            let to = (track.pts[k] - here).with_y(0.0);
            let off = to.dot(out);
            (off > 0.0 && to.dot(along).abs() < ABEAM * off && to.length() < FURTHEST)
                .then(|| (k, to.length()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Whether one side of the road at sample `i` is the inside of a bend that is there,
/// is coming or is just past.
fn inside(track: &Track, i: usize, side: f32) -> bool {
    let n = track.n();
    (0..=INSIDE_ROUND.0 + INSIDE_ROUND.1).any(|step| {
        let j = (i + n + step - INSIDE_ROUND.1) % n;
        let turn = track.flat[j].cross(track.flat[(j + 1) % n]).y;
        track.curv[j] > INSIDE_TURN && -turn * side > 0.0
    })
}

/// The walls and the hedges of a circuit, as its road is.
pub fn fences(track: &Track) -> Vec<Fence> {
    let n = track.n();
    if track.arena.is_some() || track.unrouted || n == 0 {
        return Vec::new();
    }
    let covered = |i: usize| track.tunnels.iter().any(|t| (t.0..t.1).contains(&i));
    let sighted = |p: Vec3| {
        let near = CROSSING_SIGHT * CROSSING_SIGHT;
        track
            .crossings
            .iter()
            .any(|crossing| xz_dist2(track.pts[crossing[0]], p) < near)
    };
    // Clear of every road that is not well over its top, and of every byway.
    let clear = |p: Vec3, top: f32, by: f32| {
        track
            .pts
            .iter()
            .all(|&q| xz_dist2(p, q) >= by * by || q.y > top + 1.0)
            && track.branches.iter().all(|branch| {
                let by = branch.wall + by - WALL;
                branch.pts.iter().all(|&q| xz_dist2(p, q) >= by * by)
            })
    };
    // The walls first, then the hedges, each side of the road in turn. A place
    // where something already stands is left: two stretches see the one wall
    // between them, and each would build it.
    let mut out: Vec<Fence> = Vec::new();
    for hedge in [false, true] {
        for side in [-1.0f32, 1.0] {
            let places: Vec<Option<(Vec3, bool)>> = (0..n)
                .map(|i| {
                    let here = track.pts[i];
                    if covered(i) {
                        return None;
                    }
                    let (p, top, by, low) = match (beside(track, i, side), hedge) {
                        // A wall half way to the other stretch: a low one if that
                        // is where the road goes next, or has just been, and
                        // then only as far as the corner between the two, which
                        // has walls of its own.
                        (Some((k, apart)), false)
                            if apart >= NEAREST
                                && !covered(k)
                                && (track.apart(i, k) as f32 * track.spacing > SOON
                                    || track.curv[i].max(track.curv[k]) < SCREEN_TURN) =>
                        {
                            let low = track.apart(i, k) as f32 * track.spacing <= SOON;
                            // A high one hides each road from the other, and
                            // a low one is low from both.
                            let top = if low {
                                here.y.min(track.pts[k].y) + LOW
                            } else {
                                here.y.max(track.pts[k].y) + HEIGHT
                            };
                            ((here + track.pts[k]) / 2.0, top, WALL_THICK, low)
                        }
                        // A hedge, where the road is on the ground and has no
                        // wall of its own on that side.
                        (None, true)
                            if here.y < HEDGE_ROAD
                                && !track.screens.iter().any(|screen| screen.stands(i, side)) =>
                        {
                            let p = here + track.right[i] * side * HEDGE_OUT;
                            let low = inside(track, i, side);
                            let high = if low { LOW } else { HEIGHT };
                            (p, here.y + high, HEDGE_THICK, low)
                        }
                        _ => return None,
                    };
                    let p = p.with_y(top);
                    let stood = out.iter().any(|fence| fence.near(p, TAKEN));
                    (!stood && !sighted(p) && clear(p, top, WALL + 0.8 + by / 2.0 + 0.3))
                        .then_some((p, low))
                })
                .collect();
            // Each run of them is one wall or one hedge, begun where there is none.
            let start = (0..n).find(|&i| places[i].is_none()).unwrap_or(0);
            let mut run: Vec<(Vec3, bool)> = Vec::new();
            for step in 0..=n {
                let place = places[(start + step) % n].filter(|_| step < n);
                let joined = match (place, run.last()) {
                    (Some(new), Some(last)) => xz_dist2(new.0, last.0) < JOINED * JOINED,
                    _ => false,
                };
                if !joined {
                    let long = run.first().zip(run.last()).map(|(a, b)| xz_dist2(a.0, b.0));
                    if long.is_some_and(|long| long >= SHORTEST * SHORTEST) {
                        out.push(Fence::through(&run, hedge));
                    }
                    run.clear();
                }
                run.extend(place);
            }
        }
    }
    out
}

impl Fence {
    /// One through the places of a run: every place a little way on from the last
    /// kept, and the last of all.
    fn through(run: &[(Vec3, bool)], hedge: bool) -> Fence {
        let mut kept = vec![run[0]];
        for (at, &place) in run.iter().enumerate().skip(1) {
            let last = kept[kept.len() - 1].0;
            if xz_dist2(place.0, last) >= STEP * STEP || at == run.len() - 1 {
                kept.push(place);
            }
        }
        Fence {
            line: kept.iter().map(|place| place.0).collect(),
            low: kept.iter().map(|place| place.1).collect(),
            hedge,
        }
    }

    /// Whether a place on the ground is within `by` of it.
    pub fn near(&self, p: Vec3, by: f32) -> bool {
        self.line.iter().any(|&q| xz_dist2(p, q) < by * by)
    }

    /// Whether a place on the ground is where nothing is to be scattered: against
    /// it, or anywhere about where it is low, and what is beyond it is looked at.
    pub fn shades(&self, p: Vec3) -> bool {
        self.line.iter().zip(&self.low).any(|(&q, &low)| {
            let clear: f32 = if low { INSIDE_CLEAR } else { 3.5 };
            xz_dist2(p, q) < clear * clear
        })
    }
}

/// Builds a circuit's walls and hedges of bricks.
pub fn draw(track: &Track, look: &Look, fences: &[Fence], b: &mut BrickMesh) {
    let up = Vec3::Y;
    for fence in fences {
        for (at, pair) in fence.line.windows(2).enumerate() {
            let (p, q) = (pair[0], pair[1]);
            let (top, middle) = ((p.y + q.y) / 2.0, ((p + q) / 2.0).with_y(0.0));
            // Not where something of a hazard's stands, or a byway or a lawn is.
            let taken = |&(centre, radius): &(Vec3, f32)| xz_dist2(centre, middle) < radius * radius;
            if track.clearings.iter().any(taken) {
                continue;
            }
            let along = (q - p).with_y(0.0);
            let rot = Transform::IDENTITY.looking_to(along, up).rotation;
            let long = along.length() / 2.0 + 0.15;
            if fence.hedge {
                let body = top - 0.8;
                b.cuboid(
                    middle + up * (body / 2.0 - 0.05),
                    Vec3::new(HEDGE_THICK / 2.0, body / 2.0 + 0.05, long),
                    rot,
                    HEDGE,
                );
                b.brick(
                    middle + up * (top - 0.4),
                    Vec3::new(HEDGE_THICK / 2.0 - 0.3, 0.4, long),
                    rot,
                    HEDGE_TOP,
                    (1, 1),
                );
                continue;
            }
            // A low wall is its footing and its coping, and no more.
            let coping = 0.5;
            let footing = (top - coping).min(1.0);
            let body = top - footing - coping;
            let thick = WALL_THICK / 2.0;
            b.cuboid(
                middle + up * (footing / 2.0 - 0.05),
                Vec3::new(thick + 0.15, footing / 2.0 + 0.05, long),
                rot,
                look.footing,
            );
            if body > 0.05 {
                b.cuboid(
                    middle + up * (footing + body / 2.0),
                    Vec3::new(thick, body / 2.0, long),
                    rot,
                    look.wall,
                );
            }
            b.brick(
                middle + up * (top - coping / 2.0),
                Vec3::new(thick + 0.2, coping / 2.0, long),
                rot,
                look.coping,
                (1, 2),
            );
            if look.battlements && at % 2 == 0 && body > 0.05 {
                b.cuboid(
                    middle + up * (top + 0.5),
                    Vec3::new(thick + 0.2, 0.5, long / 2.0),
                    rot,
                    look.coping,
                );
            }
            // A pier at each end of it and every so often along it, a little higher
            // than the wall, with a cap.
            let last = at + 2 == fence.line.len();
            for end in [p].into_iter().filter(|_| at % 10 == 0).chain(last.then_some(q)) {
                let high = end.y + if body > 0.05 { 1.0 } else { 0.4 };
                b.brick(
                    end.with_y(high / 2.0),
                    Vec3::new(0.9, high / 2.0, 0.9),
                    rot,
                    look.footing,
                    (2, 2),
                );
                b.brick(
                    end.with_y(high + 0.3),
                    Vec3::new(0.7, 0.3, 0.7),
                    rot,
                    look.cap,
                    (1, 1),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{SCREEN_THICK, parapet};

    /// How far on round the lap the road is that a car will be on next.
    const NEXT: f32 = 120.0;

    /// Something that is not seen through: its two ends, each as high as its top
    /// is there, and how high its foot is.
    type Blind = (Vec3, Vec3, f32);

    /// Everything of a circuit that hides what is behind it: the walls and hedges,
    /// the walls at the corners, the banks under road that is off the ground and
    /// the parapets along it, and the tunnels.
    fn blinds(track: &Track, fences: &[Fence]) -> Vec<Blind> {
        let n = track.n();
        let mut blinds: Vec<Blind> = Vec::new();
        for fence in fences {
            for pair in fence.line.windows(2) {
                let middle = ((pair[0] + pair[1]) / 2.0).with_y(0.0);
                let taken = |&(c, r): &(Vec3, f32)| xz_dist2(c, middle) < r * r;
                if !track.clearings.iter().any(taken) {
                    blinds.push((pair[0], pair[1], 0.0));
                }
            }
        }
        let bridged = track.bridged();
        for i in 0..n {
            let j = (i + 1) % n;
            for side in [-1.0, 1.0] {
                let at = |lat: f32, high: f32| {
                    let (a, b) = (track.edge(i, side * lat), track.edge(j, side * lat));
                    (a.with_y(a.y + high), b.with_y(b.y + high))
                };
                if let Some(screen) = track.screens.iter().find(|screen| screen.stands(i, side)) {
                    let (a, b) = at(WALL + 0.8 + SCREEN_THICK / 2.0, screen.height());
                    blinds.push((a, b, 0.0));
                }
                if track.tunnels.iter().any(|t| (t.0..t.1).contains(&i)) {
                    let (a, b) = at(WALL + 1.6, 10.0);
                    blinds.push((a, b, 0.0));
                }
                // A bank is solid to the ground; a bridge only as deep as its deck.
                let raised = (track.pts[i].y + track.pts[j].y) / 2.0;
                if raised > 0.8 {
                    let (a, b) = at(WALL + 0.4, 0.9 + parapet(raised));
                    let foot = if bridged[i] || bridged[j] { raised - 0.85 } else { 0.0 };
                    blinds.push((a, b, foot));
                }
            }
        }
        blinds
    }

    /// How much of the rest of a circuit's road is in plain sight from where the
    /// camera behind a car is, each a part of one, taken over every place that is
    /// not near a crossing: of the road on the ground within a hundred and ten ahead
    /// and to either side that is another stretch, and not the road straight on or
    /// the other road at a crossing, how much of it nothing stands before. First of
    /// the road that has nothing to do with where the car is going, which is better
    /// not seen; then of the road it will be on next, which is.
    fn in_sight(track: &Track, fences: &[Fence]) -> (f32, f32) {
        let n = track.n();
        let blinds = blinds(track, fences);
        let crossing = |p: Vec3| {
            track
                .crossings
                .iter()
                .any(|c| xz_dist2(track.pts[c[0]], p) < (CROSSING_SIGHT + 20.0).powi(2))
        };
        // Seen and looked for: of the road elsewhere, and of the road next.
        let mut counts = [(0, 0); 2];
        for i in (0..n).step_by(3) {
            let eye = track.pts[(i + n - 3) % n] + Vec3::Y * 3.6;
            if crossing(eye) {
                continue;
            }
            // The road straight on from here is not another stretch, however far.
            let ahead = (1..90)
                .take_while(|step| track.flat[(i + step) % n].dot(track.flat[i]) > 0.7)
                .count();
            let others = (0..n)
                .step_by(2)
                .filter(|&k| track.apart(i, k) > STRETCH && (k + n - i) % n > ahead)
                .filter(|&k| track.pts[k].y < 1.5 && !crossing(track.pts[k]));
            for k in others {
                let target = track.pts[k] + Vec3::Y * 0.8;
                let to = (target - eye).with_y(0.0);
                if to.length() > 110.0 || to.dot(track.flat[i]) < 0.34 * to.length() {
                    continue;
                }
                // Hidden by whatever the line to it passes through between its
                // foot and its top.
                let hidden = blinds.iter().any(|&(a, b, foot)| {
                    let (wall, from) = ((b - a).with_y(0.0), (a - eye).with_y(0.0));
                    let across = to.x * wall.z - to.z * wall.x;
                    if across.abs() < 1e-6 {
                        return false;
                    }
                    let along = (from.x * wall.z - from.z * wall.x) / across;
                    let on = (from.x * to.z - from.z * to.x) / across;
                    let high = eye.y + (target.y - eye.y) * along;
                    (0.0..=1.0).contains(&along)
                        && (0.0..=1.0).contains(&on)
                        && high < a.y + (b.y - a.y) * on
                        && high > foot
                });
                let next = ((k + n - i) % n) as f32 * track.spacing <= NEXT;
                counts[next as usize].0 += !hidden as usize;
                counts[next as usize].1 += 1;
            }
        }
        let part = |(seen, looked): (usize, usize)| seen as f32 / looked.max(1) as f32;
        (part(counts[0]), part(counts[1]))
    }

    #[test]
    fn little_of_the_rest_of_the_road_is_seen_from_a_car() {
        for map in crate::map::circuits() {
            let (layout, track) = (map.key.as_str(), Track::built(map));
            let fences = fences(&track);
            let (bare, hidden) = (in_sight(&track, &[]), in_sight(&track, &fences));
            let (walls, hedges): (Vec<&Fence>, Vec<&Fence>) = fences.iter().partition(|f| !f.hedge);
            let length = |fences: &[&Fence]| -> f32 {
                fences
                    .iter()
                    .flat_map(|fence| fence.line.windows(2))
                    .map(|pair| xz_dist2(pair[0], pair[1]).sqrt())
                    .sum()
            };
            println!(
                "{layout}: of the road elsewhere {:.0}% in sight without them and {:.0}% with, of the road next {:.0}% and {:.0}%; {} walls {:.0} long, {} hedges {:.0} long",
                100.0 * bare.0,
                100.0 * hidden.0,
                100.0 * bare.1,
                100.0 * hidden.1,
                walls.len(),
                length(&walls),
                hedges.len(),
                length(&hedges),
            );
            // Little of the road elsewhere is seen, and far less than without them;
            // and most of what could be seen of the road next still is.
            assert!(hidden.0 < 0.15 && hidden.0 < 0.65 * bare.0, "{layout} {bare:?} {hidden:?}");
            assert!(hidden.1 > 0.6 * bare.1, "{layout} {bare:?} {hidden:?}");
            // Nothing of them is on a road, a byway or its barriers, and nothing is
            // near the crossing.
            for fence in &fences {
                for &p in &fence.line {
                    let thick = if fence.hedge { HEDGE_THICK } else { WALL_THICK };
                    for &q in &track.pts {
                        let off = xz_dist2(p, q).sqrt();
                        assert!(
                            off >= WALL + 0.8 + thick / 2.0 || q.y > p.y + 1.0,
                            "{layout} {p} is {off} from the road at {q}"
                        );
                    }
                    for crossing in &track.crossings {
                        assert!(xz_dist2(track.pts[crossing[0]], p).sqrt() >= CROSSING_SIGHT);
                    }
                }
            }
        }
    }

    #[test]
    fn an_arena_has_no_walls_of_these() {
        for map in crate::map::arenas() {
            assert!(fences(&Track::built(map)).is_empty());
        }
    }
}
