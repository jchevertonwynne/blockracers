//! Tiny vertex-coloured mesh builder used to make everything out of bricks.

use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*,
    render::render_resource::PrimitiveTopology,
};
use std::f32::consts::TAU;

#[derive(Default)]
pub struct BrickMesh {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl BrickMesh {
    fn face(&mut self, pts: &[Vec3], color: Color) {
        let n = (pts[1] - pts[0]).cross(pts[2] - pts[0]).normalize_or_zero();
        let base = self.pos.len() as u32;
        let col = color.to_linear().to_f32_array();
        for p in pts {
            self.pos.push(p.to_array());
            self.nrm.push(n.to_array());
            self.col.push(col);
        }
        for i in 1..pts.len() as u32 - 1 {
            self.idx.extend([base, base + i, base + i + 1]);
        }
    }

    /// Counter-clockwise when seen from the front.
    pub fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Color) {
        self.face(&[a, b, c], color);
    }

    /// Counter-clockwise when seen from the front.
    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Color) {
        self.face(&[a, b, c, d], color);
    }

    pub fn cuboid(&mut self, center: Vec3, half: Vec3, rot: Quat, color: Color) {
        let x = rot * Vec3::X * half.x;
        let y = rot * Vec3::Y * half.y;
        let z = rot * Vec3::Z * half.z;
        // (normal, u, v) with u × v = normal
        for (n, u, v) in [(x, y, z), (-x, z, y), (y, z, x), (-y, x, z), (z, x, y), (-z, y, x)] {
            let c = center + n;
            self.quad(c - u - v, c + u - v, c + u + v, c - u + v, color);
        }
    }

    /// Capped cylinder whose axis is `rot * Y`, starting at `base`.
    pub fn cyl(&mut self, base: Vec3, radius: f32, height: f32, rot: Quat, color: Color) {
        const SIDES: usize = 12;
        let top = base + rot * Vec3::Y * height;
        let ring = |i: usize| {
            let a = i as f32 / SIDES as f32 * TAU;
            rot * Vec3::new(a.cos() * radius, 0.0, a.sin() * radius)
        };
        for i in 0..SIDES {
            let (p0, p1) = (ring(i), ring(i + 1));
            self.quad(base + p1, base + p0, top + p0, top + p1, color);
            self.tri(top, top + p1, top + p0, color);
            self.tri(base, base + p0, base + p1, color);
        }
    }

    /// A cuboid with a grid of studs on top.
    pub fn brick(&mut self, center: Vec3, half: Vec3, rot: Quat, color: Color, studs: (u32, u32)) {
        self.cuboid(center, half, rot, color);
        let cell = Vec2::new(2.0 * half.x / studs.0 as f32, 2.0 * half.z / studs.1 as f32);
        let radius = 0.3 * cell.min_element();
        for i in 0..studs.0 {
            for j in 0..studs.1 {
                let local = Vec3::new(
                    (i as f32 + 0.5) * cell.x - half.x,
                    half.y,
                    (j as f32 + 0.5) * cell.y - half.z,
                );
                self.cyl(center + rot * local, radius, radius * 0.55, rot, color);
            }
        }
    }

    pub fn build(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

/// Deterministic xorshift RNG; plenty for scenery and AI whims.
#[derive(Resource)]
pub struct Rng(pub u32);

impl Rng {
    pub fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.f() * items.len() as f32) as usize % items.len()]
    }
}

pub const RED: Color = Color::srgb(0.79, 0.10, 0.09);
pub const BLUE: Color = Color::srgb(0.0, 0.33, 0.75);
pub const YELLOW: Color = Color::srgb(0.96, 0.80, 0.18);
pub const GREEN: Color = Color::srgb(0.14, 0.47, 0.25);
pub const LIME: Color = Color::srgb(0.29, 0.62, 0.29);
pub const WHITE: Color = Color::srgb(0.95, 0.95, 0.94);
pub const BLACK: Color = Color::srgb(0.03, 0.03, 0.04);
pub const GREY: Color = Color::srgb(0.63, 0.65, 0.66);
pub const DARK_GREY: Color = Color::srgb(0.33, 0.35, 0.36);
pub const ORANGE: Color = Color::srgb(0.85, 0.52, 0.42);
pub const BROWN: Color = Color::srgb(0.35, 0.20, 0.12);
pub const TAN: Color = Color::srgb(0.84, 0.75, 0.55);
