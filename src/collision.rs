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
    tag: usize,
}

pub struct Hit {
    /// Fraction of the way along the segment.
    pub t: f32,
    pub point: Vec3,
    /// Unit normal, on the side the segment came from.
    pub normal: Vec3,
    pub surface: Surface,
    /// Whatever the triangle was tagged with when added (a checkpoint number, say).
    pub tag: usize,
}

#[derive(Default)]
pub struct Collision {
    triangles: Vec<Triangle>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Tags whose triangles are, for now, not there at all (an open door, say).
    passable: Vec<bool>,
    /// Tags whose triangles stop cars but not shots: the original's invisible barriers.
    shots_pass: Vec<bool>,
}

fn cell_of(x: f32, z: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (z / CELL).floor() as i32)
}

impl Collision {
    pub fn add(&mut self, triangle: [Vec3; 3], surface: Surface) {
        self.add_tagged(triangle, surface, 0);
    }

    pub fn add_tagged(&mut self, [a, b, c]: [Vec3; 3], surface: Surface, tag: usize) {
        let normal = (b - a).cross(c - a).normalize_or_zero();
        if normal == Vec3::ZERO {
            return;
        }
        let index = self.triangles.len() as u32;
        self.triangles.push(Triangle { a, ab: b - a, ac: c - a, normal, surface, tag });
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
        self.segment_through(from, to, accept, false)
    }

    /// As `segment`, but among either the solid triangles or the passable ones.
    fn segment_through(&self, from: Vec3, to: Vec3, accept: impl Fn(Vec3) -> bool, passable: bool) -> Option<Hit> {
        let dir = to - from;
        let (lo, hi) = (from.min(to), from.max(to));
        let (lo, hi) = (cell_of(lo.x, lo.z), cell_of(hi.x, hi.z));
        let mut best: Option<(f32, &Triangle)> = None;
        for x in lo.0..=hi.0 {
            for z in lo.1..=hi.1 {
                for &i in self.cells.get(&(x, z)).into_iter().flatten() {
                    let tri = &self.triangles[i as usize];
                    if !accept(tri.normal) || self.passable.get(tri.tag).is_some_and(|&p| p) != passable {
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
            tag: tri.tag,
        })
    }

    /// Opens or closes every triangle with this tag.
    pub fn set_passable(&mut self, tag: usize, passable: bool) {
        if self.passable.len() <= tag {
            self.passable.resize(tag + 1, false);
        }
        self.passable[tag] = passable;
    }

    /// The box round every triangle with this tag: its least and greatest corners.
    pub fn bounds(&self, tag: usize) -> Option<(Vec3, Vec3)> {
        let corners = self.triangles.iter().filter(|t| t.tag == tag).flat_map(|t| [t.a, t.a + t.ab, t.a + t.ac]);
        corners.fold(None, |bounds: Option<(Vec3, Vec3)>, p| Some(bounds.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p)))))
    }

    /// Lets shots through every triangle with this tag, for good.
    pub fn set_shots_pass(&mut self, tag: usize) {
        if self.shots_pass.len() <= tag {
            self.shots_pass.resize(tag + 1, false);
        }
        self.shots_pass[tag] = true;
    }

    /// What a shot going between two points strikes: anything but the barriers that
    /// are only there for cars.
    pub fn shot(&self, mut from: Vec3, to: Vec3) -> Option<Hit> {
        // Past each barrier in the way, on to whatever is behind it.
        for _ in 0..8 {
            let hit = self.any(from, to)?;
            if !self.shots_pass.get(hit.tag).is_some_and(|&pass| pass) {
                return Some(hit);
            }
            from = hit.point + (to - from).normalize_or_zero() * 1e-3;
        }
        None
    }

    /// A surface that isn't solid but notices being driven through, between two points.
    pub fn touched(&self, from: Vec3, to: Vec3) -> Option<Hit> {
        self.segment_through(from, to, |_| true, true).filter(|hit| hit.surface.touch_event.is_some())
    }

    /// Drivable surface on the way straight down from `from`, at most `depth` below.
    pub fn ground(&self, from: Vec3, depth: f32) -> Option<Hit> {
        self.segment(from, from - Vec3::Y * depth, |n| n.y.abs() >= WALKABLE)
    }

    /// Anything at all between two points.
    pub fn any(&self, from: Vec3, to: Vec3) -> Option<Hit> {
        self.segment(from, to, |_| true)
    }

    /// Wall between two points.
    pub fn wall(&self, from: Vec3, to: Vec3) -> Option<Hit> {
        self.segment(from, to, |n| n.y.abs() < WALKABLE)
    }
}

#[cfg(test)]
#[test]
fn a_barrier_stops_cars_and_lets_shots_by() {
    let mut world = Collision::default();
    let wall = |x: f32| [Vec3::new(x, -5.0, -5.0), Vec3::new(x, 5.0, -5.0), Vec3::new(x, 0.0, 5.0)];
    world.add_tagged(wall(2.0), Surface::default(), 1);
    world.add_tagged(wall(4.0), Surface::default(), 2);
    world.set_shots_pass(1);
    let (from, to) = (Vec3::ZERO, Vec3::X * 6.0);
    // A car meets the barrier; a shot goes on to the wall behind it.
    assert_eq!(world.wall(from, to).unwrap().tag, 1);
    assert_eq!(world.shot(from, to).unwrap().tag, 2);
    assert!(world.shot(from, Vec3::X * 3.0).is_none());
}
