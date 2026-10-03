//! The solid world: a triangle soup with a grid over the XZ plane, queried with short
//! segments (wheel probes straight down, body probes sideways).

use crate::assets::materials::Surface;
use bevy::prelude::*;
use std::collections::HashMap;

const CELL: f32 = 6.0;
/// Triangles whose normal is at least this vertical can be driven on; the rest are walls.
const WALKABLE: f32 = 0.5;

struct Triangle {
    a: Vec3,
    ab: Vec3,
    ac: Vec3,
    normal: Vec3,
    surface: Surface,
}

pub struct Hit {
    /// Fraction of the way along the segment.
    pub t: f32,
    pub point: Vec3,
    /// Unit normal, on the side the segment came from.
    pub normal: Vec3,
    pub surface: Surface,
}

#[derive(Default)]
pub struct Collision {
    triangles: Vec<Triangle>,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

fn cell_of(x: f32, z: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (z / CELL).floor() as i32)
}

impl Collision {
    pub fn add(&mut self, [a, b, c]: [Vec3; 3], surface: Surface) {
        let normal = (b - a).cross(c - a).normalize_or_zero();
        if normal == Vec3::ZERO {
            return;
        }
        let index = self.triangles.len() as u32;
        self.triangles.push(Triangle { a, ab: b - a, ac: c - a, normal, surface });
        let (lo, hi) = (a.min(b).min(c), a.max(b).max(c));
        let (lo, hi) = (cell_of(lo.x, lo.z), cell_of(hi.x, hi.z));
        for x in lo.0..=hi.0 {
            for z in lo.1..=hi.1 {
                self.cells.entry((x, z)).or_default().push(index);
            }
        }
    }

    /// First triangle crossed going from `from` to `to`, among those whose (unsigned)
    /// normal passes `accept`.
    fn segment(&self, from: Vec3, to: Vec3, accept: impl Fn(Vec3) -> bool) -> Option<Hit> {
        let dir = to - from;
        let (lo, hi) = (from.min(to), from.max(to));
        let (lo, hi) = (cell_of(lo.x, lo.z), cell_of(hi.x, hi.z));
        let mut best: Option<(f32, &Triangle)> = None;
        for x in lo.0..=hi.0 {
            for z in lo.1..=hi.1 {
                for &i in self.cells.get(&(x, z)).into_iter().flatten() {
                    let tri = &self.triangles[i as usize];
                    if !accept(tri.normal) {
                        continue;
                    }
                    // Möller–Trumbore, both faces.
                    let p = dir.cross(tri.ac);
                    let det = tri.ab.dot(p);
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let s = from - tri.a;
                    let u = s.dot(p) / det;
                    let q = s.cross(tri.ab);
                    let v = dir.dot(q) / det;
                    let t = tri.ac.dot(q) / det;
                    let inside = u >= -1e-4 && v >= -1e-4 && u + v <= 1.0001;
                    if inside && (0.0..=1.0).contains(&t) && best.is_none_or(|b| t < b.0) {
                        best = Some((t, tri));
                    }
                }
            }
        }
        best.map(|(t, tri)| Hit {
            t,
            point: from + dir * t,
            normal: if tri.normal.dot(dir) > 0.0 { -tri.normal } else { tri.normal },
            surface: tri.surface,
        })
    }

    /// Drivable surface on the way straight down from `from`, at most `depth` below.
    pub fn ground(&self, from: Vec3, depth: f32) -> Option<Hit> {
        self.segment(from, from - Vec3::Y * depth, |n| n.y.abs() >= WALKABLE)
    }

    /// Wall between two points.
    pub fn wall(&self, from: Vec3, to: Vec3) -> Option<Hit> {
        self.segment(from, to, |n| n.y.abs() < WALKABLE)
    }
}
