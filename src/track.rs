//! The circuit: a closed spline resampled to evenly spaced points. Karts, projectiles and
//! pickups all live in "track space" (distance along the track + lateral offset), which
//! keeps collisions with the walls and the hills trivial.

use crate::meshgen::*;
use bevy::prelude::*;
use crate::assets::materials::Surface;
use crate::collision::Collision;

/// Half-width of the tarmac.
pub const ROAD_HW: f32 = 8.0;
/// Lateral offset of the inner face of the barrier.
pub const WALL: f32 = 11.0;
const KERB: f32 = 0.8;

/// Control points as (x, height, z).
const CONTROL: &[[f32; 3]] = &[
    [0.0, 0.0, 0.0],
    [60.0, 0.0, 0.0],
    [120.0, 0.0, -10.0],
    [160.0, 3.0, -50.0],
    [150.0, 6.0, -100.0],
    [100.0, 6.0, -120.0],
    [60.0, 3.0, -90.0],
    [20.0, 0.0, -110.0],
    [-30.0, 0.0, -150.0],
    [-90.0, 4.0, -140.0],
    [-120.0, 8.0, -90.0],
    [-105.0, 4.0, -50.0],
    [-122.0, 1.0, -15.0],
    [-108.0, 0.0, 12.0],
    [-75.0, 0.0, 20.0],
    [-40.0, 0.0, 6.0],
];
const SCALE: f32 = 1.4;

#[derive(Resource)]
pub struct Track {
    pub pts: Vec<Vec3>,
    /// Unit tangent, including slope.
    pub fwd: Vec<Vec3>,
    /// Unit tangent flattened onto the ground plane.
    pub flat: Vec<Vec3>,
    pub right: Vec<Vec3>,
    /// 1 / turn radius.
    pub curv: Vec<f32>,
    pub spacing: f32,
    pub length: f32,
    /// Half-width of the band around the racing line that AI drivers and the starting
    /// grid spread across.
    pub road: f32,
    pub collision: Collision,
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t)
}

impl Track {
    /// The built-in brick circuit.
    pub fn new() -> Self {
        let ctrl: Vec<Vec3> = CONTROL
            .iter()
            .map(|c| Vec3::new(c[0] * SCALE, c[1], c[2] * SCALE))
            .collect();
        let mut track = Track::from_loop(&ctrl, ROAD_HW);
        // Tarmac, verges and the inner faces of the barriers.
        let n = track.n();
        let grass = Surface { rolling_resistance: 20.0, ..default() };
        for i in 0..n {
            let j = (i + 1) % n;
            let at = |k: usize, lat: f32| track.pts[k] + track.right[k] * lat;
            let mut quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3, surface: Surface| {
                track.collision.add([a, b, c], surface);
                track.collision.add([a, c, d], surface);
            };
            let mut strip = |from: f32, to: f32, surface: Surface| {
                quad(at(i, from), at(i, to), at(j, to), at(j, from), surface);
            };
            strip(-ROAD_HW - KERB, ROAD_HW + KERB, Surface::default());
            strip(-WALL, -ROAD_HW - KERB, grass);
            strip(ROAD_HW + KERB, WALL, grass);
            for side in [-WALL, WALL] {
                let (a, b) = (at(i, side), at(j, side));
                quad(a - Vec3::Y, b - Vec3::Y, b + Vec3::Y * 3.0, a + Vec3::Y * 3.0, Surface::default());
            }
        }
        track
    }

    /// Fits a closed spline through `ctrl` and resamples it at even spacing.
    pub fn from_loop(ctrl: &[Vec3], road: f32) -> Self {
        let m = ctrl.len();
        let mut dense = Vec::new();
        for c in 0..m {
            for t in 0..40 {
                dense.push(catmull_rom(
                    ctrl[(c + m - 1) % m],
                    ctrl[c],
                    ctrl[(c + 1) % m],
                    ctrl[(c + 2) % m],
                    t as f32 / 40.0,
                ));
            }
        }
        let d = dense.len();
        let mut cum = vec![0.0; d + 1];
        for i in 0..d {
            cum[i + 1] = cum[i] + dense[i].distance(dense[(i + 1) % d]);
        }
        let length = cum[d];
        let n = (length / 2.0).round() as usize;
        let spacing = length / n as f32;

        let mut pts = Vec::with_capacity(n);
        let mut j = 0;
        for k in 0..n {
            let target = k as f32 * spacing;
            while cum[j + 1] < target {
                j += 1;
            }
            let t = (target - cum[j]) / (cum[j + 1] - cum[j]);
            pts.push(dense[j].lerp(dense[(j + 1) % d], t));
        }

        let fwd: Vec<Vec3> = (0..n)
            .map(|i| (pts[(i + 1) % n] - pts[(i + n - 1) % n]).normalize())
            .collect();
        let flat: Vec<Vec3> = fwd.iter().map(|f| Vec3::new(f.x, 0.0, f.z).normalize()).collect();
        let right = flat.iter().map(|f| Vec3::new(-f.z, 0.0, f.x)).collect();
        let curv = (0..n)
            .map(|i| flat[i].angle_between(flat[(i + 1) % n]) / spacing)
            .collect();
        Track { pts, fwd, flat, right, curv, spacing, length, road, collision: Collision::default() }
    }

    pub fn n(&self) -> usize {
        self.pts.len()
    }

    /// Interpolated (position, tangent, right) at distance `s`, which may be any real number.
    pub fn sample(&self, s: f32) -> (Vec3, Vec3, Vec3) {
        let x = s.rem_euclid(self.length) / self.spacing;
        let i = x as usize % self.n();
        let j = (i + 1) % self.n();
        let t = x.fract();
        (
            self.pts[i].lerp(self.pts[j], t),
            self.fwd[i].lerp(self.fwd[j], t).normalize(),
            self.right[i].lerp(self.right[j], t).normalize(),
        )
    }

    pub fn point(&self, s: f32, lat: f32) -> Vec3 {
        let (p, _, r) = self.sample(s);
        p + r * lat
    }

    /// The point on the driving surface at distance `s` and lateral offset `lat`.
    pub fn surface_point(&self, s: f32, lat: f32) -> Vec3 {
        let p = self.point(s, lat);
        self.collision.ground(p + Vec3::Y * 4.0, 12.0).map_or(p, |hit| hit.point)
    }

    pub fn nearest(&self, pos: Vec3) -> usize {
        (0..self.n())
            .min_by(|&a, &b| {
                xz_dist2(self.pts[a], pos).total_cmp(&xz_dist2(self.pts[b], pos))
            })
            .unwrap()
    }

    /// Projects a world position near sample `hint` into track space: (sample index,
    /// distance along the lap, lateral offset).
    pub fn project(&self, pos: Vec3, hint: usize) -> (usize, f32, f32) {
        let n = self.n() as i32;
        let mut best = hint;
        let mut best_d = f32::MAX;
        for o in -10..=10 {
            let i = (hint as i32 + o).rem_euclid(n) as usize;
            let d = xz_dist2(self.pts[i], pos);
            if d < best_d {
                (best, best_d) = (i, d);
            }
        }
        let d = pos - self.pts[best];
        (best, best as f32 * self.spacing + d.dot(self.flat[best]), d.dot(self.right[best]))
    }

    pub fn build_mesh(&self) -> Mesh {
        let mut b = BrickMesh::default();
        let n = self.n();
        let up = Vec3::Y;
        let road = [Color::srgb(0.25, 0.26, 0.28), Color::srgb(0.28, 0.29, 0.31)];
        for i in 0..n {
            let j = (i + 1) % n;
            let (p0, p1) = (self.pts[i], self.pts[j]);
            let (r0, r1) = (self.right[i], self.right[j]);
            let at = |lat: f32| (p0 + r0 * lat, p1 + r1 * lat);
            let strip = |b: &mut BrickMesh, from: f32, to: f32, lift: f32, c: Color| {
                let ((a0, a1), (b0, b1)) = (at(from), at(to));
                let l = up * lift;
                b.quad(a0 + l, b0 + l, b1 + l, a1 + l, c);
            };

            strip(&mut b, -ROAD_HW, ROAD_HW, 0.0, road[(i / 2) % 2]);
            if i % 6 < 2 {
                strip(&mut b, -0.2, 0.2, 0.02, WHITE);
            }
            let kerb = if (i / 2) % 2 == 0 { RED } else { WHITE };
            strip(&mut b, ROAD_HW, ROAD_HW + KERB, 0.01, kerb);
            strip(&mut b, -ROAD_HW - KERB, -ROAD_HW, 0.01, kerb);
            strip(&mut b, ROAD_HW + KERB, WALL, 0.0, LIME);
            strip(&mut b, -WALL, -ROAD_HW - KERB, 0.0, LIME);

            let barrier = if (i / 3) % 2 == 0 { WHITE } else { RED };
            for side in [-1.0, 1.0] {
                let (a0, a1) = at(side * (WALL + 0.4));
                let rot = Transform::IDENTITY.looking_to(a1 - a0, up).rotation;
                let mid = (a0 + a1) / 2.0;
                let half = Vec3::new(0.4, 0.45, a0.distance(a1) / 2.0 + 0.05);
                b.cuboid(mid + up * 0.45, half, rot, barrier);
                b.cyl(mid + up * 0.9, 0.25, 0.15, rot, barrier);

                // Embankment under raised sections, visible from both sides.
                if p0.y > 0.05 || p1.y > 0.05 {
                    let (o0, o1) = at(side * (WALL + 0.8));
                    let (g0, g1) = (o0.with_y(-0.1), o1.with_y(-0.1));
                    b.quad(o0, o1, g1, g0, TAN);
                    b.quad(o1, o0, g0, g1, TAN);
                }
            }
        }

        // Chequered start line and gantry.
        let rot = Transform::IDENTITY.looking_to(self.fwd[0], up).rotation;
        let chequer = |i: i32, row: i32| if (i + row) % 2 == 0 { WHITE } else { BLACK };
        for row in 0..2 {
            for i in -8..8 {
                let c = self.point(row as f32 + 0.5, i as f32 + 0.5) + up * 0.03;
                b.cuboid(c, Vec3::new(0.5, 0.01, 0.5), rot, chequer(i, row));
            }
            for i in -6..6 {
                let c = self.point(0.0, (i as f32 + 0.5) * 2.1) + up * (8.5 + row as f32);
                b.cuboid(c, Vec3::new(1.05, 0.5, 0.4), rot, chequer(i, row));
            }
        }
        for side in [-1.0, 1.0] {
            let c = self.point(0.0, side * (WALL + 1.6)) + up * 5.0;
            b.brick(c, Vec3::new(0.8, 5.0, 0.8), rot, YELLOW, (2, 2));
        }
        b.build()
    }

    /// Brick trees and oversized bricks scattered around the outside of the circuit.
    pub fn build_scenery(&self, rng: &mut Rng) -> Mesh {
        let mut b = BrickMesh::default();
        for _ in 0..420 {
            let pos = Vec3::new(rng.range(-300.0, 350.0), 0.0, rng.range(-340.0, 150.0));
            let clear = xz_dist2(self.pts[self.nearest(pos)], pos).sqrt();
            if clear < WALL + 7.0 {
                continue;
            }
            let rot = Quat::from_rotation_y(rng.range(0.0, std::f32::consts::TAU));
            if rng.f() < 0.65 {
                let s = rng.range(0.8, 1.7);
                b.cuboid(pos + Vec3::Y * 1.5 * s, Vec3::new(0.6, 1.5, 0.6) * s, rot, BROWN);
                let leaf = rng.pick(&[GREEN, GREEN, LIME]);
                for (k, studs) in [4u32, 3, 2, 1].into_iter().enumerate() {
                    let w = 0.6 * studs as f32 * s;
                    let y = (3.6 + 1.2 * k as f32) * s;
                    b.brick(pos + Vec3::Y * y, Vec3::new(w, 0.6 * s, w), rot, leaf, (studs, studs));
                }
            } else {
                let s = rng.range(1.0, 2.5);
                let half = Vec3::new(2.0, 1.2, 4.0) * s;
                for level in 0..if rng.f() < 0.3 { 2 } else { 1 } {
                    let colour = rng.pick(&[RED, BLUE, YELLOW, WHITE, ORANGE, GREY]);
                    let y = half.y * (1.0 + 2.0 * level as f32);
                    b.brick(pos + Vec3::Y * y, half, rot, colour, (2, 4));
                }
            }
        }
        b.build()
    }
}

pub fn xz_dist2(a: Vec3, b: Vec3) -> f32 {
    (a.x - b.x).powi(2) + (a.z - b.z).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barriers_never_fold_and_track_never_touches_itself() {
        let t = Track::new();
        let n = t.n();
        assert!(t.length > 900.0 && t.length < 1400.0, "length {}", t.length);
        for i in 0..n {
            let j = (i + 1) % n;
            for side in [-1.0, 1.0] {
                let a = t.pts[i] + t.right[i] * side * (WALL + 0.8);
                let b = t.pts[j] + t.right[j] * side * (WALL + 0.8);
                assert!((b - a).dot(t.fwd[i]) > 0.2, "barrier folds at {i}");
            }
            // Any other part of the track must be far away unless it's nearby along the lap.
            for k in 0..n {
                let along = (i as i32 - k as i32).rem_euclid(n as i32).min((k as i32 - i as i32).rem_euclid(n as i32));
                if along > 30 {
                    assert!(xz_dist2(t.pts[i], t.pts[k]).sqrt() > 2.0 * WALL + 4.0, "{i} near {k}");
                }
            }
        }
    }

    #[test]
    fn projection_round_trips() {
        let t = Track::new();
        for (s, lat) in [(10.0, 3.0), (500.3, -7.5), (t.length - 1.0, 0.0)] {
            let p = t.point(s, lat);
            let (_, s2, lat2) = t.project(p, t.nearest(p));
            let ds = (s2 - s + t.length / 2.0).rem_euclid(t.length) - t.length / 2.0;
            assert!(ds.abs() < 0.3 && (lat2 - lat).abs() < 0.3, "{s} {lat} -> {s2} {lat2}");
        }
    }
}
