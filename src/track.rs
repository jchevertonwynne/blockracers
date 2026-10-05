//! The circuit: a closed spline resampled to evenly spaced points. Karts, projectiles and
//! pickups all live in "track space" (distance along the track + lateral offset), which
//! keeps collisions with the walls and the hills trivial.

use crate::assets::materials::Surface;
use crate::collision::Collision;
use crate::meshgen::*;
use bevy::prelude::*;
use std::collections::HashMap;

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

/// Height of the figure of eight's bridge: more than a launched kart rises, so nothing
/// on the road below reaches the deck.
const DECK: f32 = 11.0;
/// The figure of eight's control points as (x, height, z). The line runs under the
/// bridge just after the start, round the wide east loop and up its far side, back
/// over the bridge, and home through the kinked west loop.
const FIGURE_EIGHT: &[[f32; 3]] = &[
    [-30.0, 0.0, -30.0],
    [0.0, 0.0, 0.0],
    [60.0, 0.0, 60.0],
    [120.0, 0.0, 85.0],
    [180.0, 0.0, 60.0],
    [205.0, 0.0, 0.0],
    [180.0, 2.0, -60.0],
    [120.0, 6.0, -85.0],
    [60.0, DECK, -60.0],
    [0.0, DECK, 0.0],
    [-60.0, DECK, 60.0],
    [-115.0, 6.0, 88.0],
    [-175.0, 2.0, 75.0],
    [-210.0, 0.0, 35.0],
    [-195.0, 0.0, -10.0],
    [-150.0, 0.0, -25.0],
    [-140.0, 0.0, -70.0],
    [-95.0, 0.0, -88.0],
];

/// The gauntlet's control points as (x, height, z). Two long straights, and short ones
/// between corners for the rest of the hazards to stand on (see `gauntlet`): zigzags,
/// esses, hairpins, hills, two tunnels and a loop that climbs over its own way in.
/// The points are close together at the corners so that the spline keeps to them.
const GAUNTLET: &[[f32; 3]] = &[
    [0.0, 0.0, 0.0],
    // The start straight: the hammer and the rolling stones.
    [9.0, 0.0, 0.0],
    [27.0, 0.0, 0.0],
    [63.0, 0.0, 0.0],
    [87.0, 0.0, 0.0],
    [123.0, 0.0, 0.0],
    [141.0, 0.0, 0.0],
    [150.0, 0.0, 0.0],
    // A zigzag of two right angles.
    [159.0, 0.0, -2.4],
    [165.6, 0.0, -9.0],
    [168.0, 0.0, -18.0],
    [168.0, 0.0, -30.0],
    [168.0, 0.0, -42.0],
    [170.4, 0.0, -51.0],
    [177.0, 0.0, -57.6],
    [186.0, 0.0, -60.0],
    // The saucer.
    [195.0, 0.0, -60.0],
    [213.0, 0.0, -60.0],
    [259.0, 0.0, -60.0],
    [277.0, 0.0, -60.0],
    [286.0, 0.0, -60.0],
    // Esses, then the crane.
    [297.5, 0.0, -57.7],
    [307.2, 0.0, -51.2],
    [316.9, 0.0, -44.7],
    [328.4, 0.0, -42.4],
    [339.9, 0.0, -44.7],
    [349.6, 0.0, -51.2],
    [359.4, 0.0, -57.7],
    [370.9, 0.0, -60.0],
    [379.9, 0.0, -60.0],
    [421.9, 0.0, -60.0],
    [430.9, 0.0, -60.0],
    [444.9, 0.0, -63.8],
    [455.1, 0.0, -74.0],
    [458.9, 0.0, -88.0],
    [458.9, 0.0, -100.0],
    [458.9, 0.0, -112.0],
    // A switchback of two hairpins over a hill, which the warp pad skips.
    [461.3, 0.8, -121.0],
    [467.9, 1.7, -127.6],
    [476.9, 2.5, -130.0],
    [485.9, 3.3, -127.6],
    [492.4, 4.2, -121.0],
    [494.9, 5.0, -112.0],
    [494.9, 5.0, -103.0],
    [494.9, 5.0, -71.0],
    [494.9, 5.0, -62.0],
    [497.3, 4.2, -53.0],
    [503.9, 3.3, -46.4],
    [512.9, 2.5, -44.0],
    [521.9, 1.7, -46.4],
    [528.4, 0.8, -53.0],
    [530.9, 0.0, -62.0],
    // The cannons.
    [530.9, 0.0, -71.0],
    [530.9, 0.0, -89.0],
    [530.9, 0.0, -115.0],
    [530.9, 0.0, -133.0],
    [530.9, 0.0, -142.0],
    // Esses, then the lava.
    [528.6, 0.0, -153.5],
    [522.1, 0.0, -163.2],
    [515.6, 0.0, -172.9],
    [513.3, 0.0, -184.4],
    [515.6, 0.0, -195.9],
    [522.1, 0.0, -205.6],
    [528.6, 0.0, -215.4],
    [530.9, 0.0, -226.9],
    [530.9, 0.0, -235.9],
    [530.9, 0.0, -253.9],
    [530.9, 0.0, -297.4],
    [530.9, 0.0, -315.4],
    [530.9, 0.0, -324.4],
    [527.6, 0.0, -336.4],
    [518.9, 0.0, -345.2],
    [506.9, 0.0, -348.4],
    // The force field.
    [497.9, 0.0, -348.4],
    [479.9, 0.0, -348.4],
    [443.9, 0.0, -348.4],
    [425.9, 0.0, -348.4],
    [416.9, 0.0, -348.4],
    // A kink, the smoke, and esses in a tunnel.
    [407.8, 0.0, -347.0],
    [399.6, 0.0, -343.0],
    [392.3, 0.0, -337.8],
    [366.1, 0.0, -319.4],
    [358.7, 0.0, -314.3],
    [350.5, 0.0, -310.2],
    [341.5, 0.0, -308.8],
    [326.7, 0.0, -312.5],
    [315.3, 0.0, -322.5],
    [303.8, 0.0, -332.5],
    [289.1, 0.0, -336.1],
    [274.3, 0.0, -332.5],
    [262.8, 0.0, -322.5],
    [251.4, 0.0, -312.5],
    [236.6, 0.0, -308.8],
    // The other long straight: the ghost and the dragon.
    [227.6, 0.0, -308.8],
    [209.6, 0.0, -308.8],
    [173.6, 0.0, -308.8],
    [129.6, 0.0, -308.8],
    [93.6, 0.0, -308.8],
    [75.6, 0.0, -308.8],
    [66.6, 0.0, -308.8],
    // A zigzag the other way, over a hump.
    [57.6, 0.7, -311.3],
    [51.0, 1.4, -317.8],
    [48.6, 2.0, -326.8],
    [48.6, 2.7, -335.8],
    [48.6, 3.0, -357.8],
    [48.6, 2.5, -366.8],
    [46.2, 2.1, -375.8],
    [39.6, 1.6, -382.4],
    [30.6, 1.2, -384.8],
    [21.6, 0.8, -384.8],
    [-0.4, 0.0, -384.8],
    [-9.4, 0.0, -384.8],
    // Two hairpins out into the middle and back.
    [-19.4, 0.0, -382.2],
    [-26.7, 0.0, -374.8],
    [-29.4, 0.0, -364.8],
    [-29.4, 0.0, -355.8],
    [-29.4, 0.0, -303.8],
    [-29.4, 0.0, -294.8],
    [-31.8, 0.0, -285.8],
    [-38.4, 0.0, -279.3],
    [-47.4, 0.0, -276.8],
    [-56.4, 0.0, -279.3],
    [-63.0, 0.0, -285.8],
    [-65.4, 0.0, -294.8],
    [-65.4, 0.0, -303.8],
    [-65.4, 0.0, -355.8],
    [-65.4, 0.0, -364.8],
    [-68.1, 0.0, -374.8],
    [-75.4, 0.0, -382.2],
    [-85.4, 0.0, -384.8],
    [-100.4, 0.0, -384.8],
    [-115.4, 0.0, -384.8],
    [-125.4, 0.0, -382.2],
    [-132.7, 0.0, -374.8],
    [-135.4, 0.0, -364.8],
    [-135.4, 0.0, -347.3],
    [-135.4, 0.0, -329.8],
    [-137.8, 0.0, -320.8],
    [-144.4, 0.0, -314.3],
    [-153.4, 0.0, -311.8],
    [-162.4, 0.0, -314.3],
    [-169.0, 0.0, -320.8],
    [-171.4, 0.0, -329.8],
    [-171.4, 0.0, -347.3],
    [-171.4, 0.0, -364.8],
    [-174.1, 0.0, -374.8],
    [-181.4, 0.0, -382.2],
    [-191.4, 0.0, -384.8],
    // Esses.
    [-201.4, 0.0, -384.8],
    [-211.4, 0.0, -384.8],
    [-222.9, 0.0, -382.6],
    [-232.6, 0.0, -376.1],
    [-242.3, 0.0, -369.6],
    [-253.8, 0.0, -367.3],
    [-265.3, 0.0, -369.6],
    [-275.0, 0.0, -376.1],
    [-284.7, 0.0, -382.6],
    [-296.2, 0.0, -384.8],
    // Under the bridge, round a climbing loop and back over it.
    [-305.2, 0.0, -384.8],
    [-323.2, 0.0, -384.8],
    [-349.1, 0.0, -384.8],
    [-367.1, 0.0, -384.8],
    [-376.1, 0.0, -384.8],
    [-398.6, 1.3, -390.9],
    [-415.1, 2.6, -407.3],
    [-421.1, 3.9, -429.8],
    [-415.1, 5.2, -452.3],
    [-398.6, 6.5, -468.8],
    [-376.1, 7.8, -474.8],
    [-353.6, 9.1, -468.8],
    [-337.2, 10.4, -452.3],
    [-331.1, 11.0, -429.8],
    [-331.1, 11.0, -420.8],
    [-331.1, 11.0, -402.8],
    [-331.1, 11.0, -376.8],
    [-331.1, 11.0, -358.8],
    [-331.1, 11.0, -349.8],
    // Down through esses, and through more in a tunnel.
    [-329.1, 9.5, -334.3],
    [-323.1, 8.0, -319.8],
    [-316.0, 6.0, -300.3],
    [-316.0, 4.0, -279.4],
    [-323.1, 2.0, -259.8],
    [-329.1, 0.5, -245.4],
    [-331.1, 0.0, -229.8],
    [-331.1, 0.0, -217.8],
    [-331.1, 0.0, -205.8],
    [-333.6, 0.0, -192.2],
    [-340.5, 0.0, -180.1],
    [-347.4, 0.0, -168.1],
    [-349.9, 0.0, -154.4],
    [-347.4, 0.0, -140.7],
    [-340.5, 0.0, -128.7],
    [-333.6, 0.0, -116.7],
    [-331.1, 0.0, -103.0],
    // The barrels.
    [-331.1, 0.0, -94.0],
    [-331.1, 0.0, -76.0],
    [-331.1, 0.0, -45.0],
    [-331.1, 0.0, -27.0],
    [-331.1, 0.0, -18.0],
    [-328.7, 0.0, -9.0],
    [-322.1, 0.0, -2.4],
    [-313.1, 0.0, 0.0],
    // The pillar.
    [-304.1, 0.0, 0.0],
    [-286.1, 0.0, 0.0],
    [-260.1, 0.0, 0.0],
    [-242.1, 0.0, 0.0],
    [-233.1, 0.0, 0.0],
    // Esses.
    [-224.9, 0.0, -1.4],
    [-217.7, 0.0, -5.6],
    [-212.4, 0.0, -12.0],
    [-205.7, 0.0, -19.4],
    [-196.6, 0.0, -23.5],
    [-186.6, 0.0, -23.5],
    [-177.5, 0.0, -19.4],
    [-170.8, 0.0, -12.0],
    [-165.4, 0.0, -5.6],
    [-158.2, 0.0, -1.4],
    [-150.0, 0.0, 0.0],
    // The ark and the curse, and home.
    [-141.0, 0.0, 0.0],
    [-123.0, 0.0, 0.0],
    [-87.0, 0.0, 0.0],
    [-63.0, 0.0, 0.0],
    [-27.0, 0.0, 0.0],
    [-9.0, 0.0, 0.0],
];
/// The gauntlet's tunnels: where each begins and ends, as (x, z).
const GAUNTLET_TUNNELS: &[[[f32; 2]; 2]] = &[
    [[341.5, -308.8], [236.6, -308.8]],
    [[-331.1, -213.8], [-331.1, -95.0]],
];
/// How high a tunnel's roof is over its road, and how thick its walls and roof are.
const TUNNEL_HEIGHT: f32 = 8.5;
const TUNNEL_THICK: f32 = 1.6;

/// The circuits built here rather than loaded from the original game's data.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Layout {
    #[default]
    Brick,
    FigureEight,
    Gauntlet,
}

impl Layout {
    pub const ALL: [Layout; 3] = [Layout::Brick, Layout::FigureEight, Layout::Gauntlet];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Brick => "Brick Circuit",
            Layout::FigureEight => "Figure Eight",
            Layout::Gauntlet => "Gauntlet",
        }
    }

    /// What `$BRICK_RACE` calls it.
    pub fn key(self) -> &'static str {
        match self {
            Layout::Brick => "BRICK",
            Layout::FigureEight => "FIGURE8",
            Layout::Gauntlet => "GAUNTLET",
        }
    }

    fn control(self, mirror: bool) -> Vec<Vec3> {
        let (points, scale) = match self {
            Layout::Brick => (CONTROL, SCALE),
            Layout::FigureEight => (FIGURE_EIGHT, 1.0),
            Layout::Gauntlet => (GAUNTLET, 1.0),
        };
        // Mirrored, the built-in circuits are turned over the same way the game's are.
        let side = if mirror { -1.0 } else { 1.0 };
        points
            .iter()
            .map(|c| Vec3::new(c[0] * scale, c[1], c[2] * scale * side))
            .collect()
    }

    /// Where its tunnels begin and end, as (x, z) on the unmirrored circuit.
    fn tunnels(self) -> &'static [[[f32; 2]; 2]] {
        if self == Layout::Gauntlet {
            GAUNTLET_TUNNELS
        } else {
            &[]
        }
    }

    /// The ground the scenery is scattered over: (least x and z, greatest x and z).
    fn grounds(self) -> (Vec2, Vec2) {
        let (least, most) = self.plain_grounds();
        if crate::scenery::mirror() {
            (Vec2::new(least.x, -most.y), Vec2::new(most.x, -least.y))
        } else {
            (least, most)
        }
    }

    fn plain_grounds(self) -> (Vec2, Vec2) {
        match self {
            Layout::Brick => (Vec2::new(-300.0, -340.0), Vec2::new(350.0, 150.0)),
            Layout::FigureEight => (Vec2::new(-340.0, -220.0), Vec2::new(330.0, 220.0)),
            Layout::Gauntlet => (Vec2::new(-540.0, -600.0), Vec2::new(650.0, 110.0)),
        }
    }
}

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
    pub course: Course,
    /// The collision surfaces by name: each one's tag in `collision`, and whether it
    /// starts out passable. Hazards open and close some of them.
    pub surfaces: HashMap<String, (usize, bool)>,
    /// Stretches where part of the road is shut, which the computer's cars keep out of.
    pub lanes: Vec<Lane>,
    /// Ground the scattered scenery is kept off: centres and radii.
    pub clearings: Vec<(Vec3, f32)>,
    /// The stretches that run through a tunnel: the first sample of each and the one
    /// after its last.
    pub tunnels: Vec<(usize, usize)>,
}

/// The part of the road that is open between two distances round the lap, as lateral
/// offsets.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lane {
    pub from: f32,
    pub to: f32,
    pub least: f32,
    pub most: f32,
}

/// A gate of the checkpoint graph that orders the racers and guards against shortcuts.
pub struct Checkpoint {
    /// Racers going the right way cross the gate against this.
    pub normal: Vec3,
    pub position: Vec3,
    /// Gates that can follow; the first is the main route.
    pub next: Vec<usize>,
    /// How far round the lap this gate is, 0..1. Gate 0 is at 0.
    pub fraction: f32,
}

/// The race rules' view of a circuit.
#[derive(Default)]
pub struct Course {
    pub checkpoints: Vec<Checkpoint>,
    /// Gate surfaces, tagged with their checkpoint's index.
    pub gates: Collision,
    pub finish: Collision,
    /// Trigger spheres (centre, radius) for the lap zones: a lap only counts if the
    /// kart went through zone 2 and then zone 0 on its way back to the line.
    pub zones: Vec<(Vec3, f32, u8)>,
    /// Starting position and heading per grid slot; empty to line up behind the line.
    pub grid: Vec<(Vec3, Vec3)>,
}

impl Course {
    /// Spreads lap fractions along the main route (following each gate's first
    /// successor from gate 0), then interpolates along alternative branches.
    pub fn compute_fractions(&mut self) {
        let count = self.checkpoints.len();
        for c in &mut self.checkpoints {
            c.fraction = -1.0;
        }
        let mut main = vec![0];
        while let Some(&next) = self.checkpoints[*main.last().unwrap()].next.first() {
            if next == 0 || main.len() >= count {
                break;
            }
            main.push(next);
        }
        for (i, &c) in main.iter().enumerate() {
            self.checkpoints[c].fraction = i as f32 / main.len() as f32;
        }
        for &from in &main {
            for branch in self.checkpoints[from].next.clone().into_iter().skip(1) {
                // Walk the branch until it rejoins gates that already have a fraction.
                let mut path = Vec::new();
                let mut at = branch;
                while self.checkpoints[at].fraction < 0.0 && path.len() < count {
                    path.push(at);
                    let Some(&next) = self.checkpoints[at].next.first() else {
                        break;
                    };
                    at = next;
                }
                let start = self.checkpoints[from].fraction;
                let end = match self.checkpoints[at].fraction {
                    f if f > start => f,
                    _ => 1.0,
                };
                let step = (end - start) / (path.len() + 1) as f32;
                for (i, c) in path.into_iter().enumerate() {
                    self.checkpoints[c].fraction = start + step * (i + 1) as f32;
                }
            }
        }
        for c in &mut self.checkpoints {
            c.fraction = c.fraction.max(0.0);
        }
    }
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t)
}

impl Track {
    /// The built-in brick circuit.
    #[cfg(test)]
    pub fn new() -> Self {
        Track::built(Layout::Brick)
    }

    /// The racing line of a built-in circuit as it is unmirrored, and nothing else.
    pub fn plain(layout: Layout) -> Self {
        Track::from_loop(&layout.control(false), ROAD_HW)
    }

    /// One of the built-in circuits.
    pub fn built(layout: Layout) -> Self {
        let mirror = crate::scenery::mirror();
        let mut track = Track::from_loop(&layout.control(mirror), ROAD_HW);
        let side = if mirror { -1.0 } else { 1.0 };
        track.tunnels = layout
            .tunnels()
            .iter()
            .map(|ends| ends.map(|[x, z]| track.nearest(Vec3::new(x, 0.0, z * side))))
            .map(|[from, to]| (from, to))
            .collect();
        // The spline dips a little before each climb; the road stays on the ground.
        for p in &mut track.pts {
            p.y = p.y.max(0.0);
        }
        // Tarmac, verges and the inner faces of the barriers.
        let n = track.n();
        let grass = Surface {
            rolling_resistance: 20.0,
            ..default()
        };
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
                quad(
                    a - Vec3::Y,
                    b - Vec3::Y,
                    b + Vec3::Y * 3.0,
                    a + Vec3::Y * 3.0,
                    Surface::default(),
                );
            }
        }

        // Sixteen evenly spaced checkpoint gates, the first on the start line.
        const GATES: usize = 16;
        for gate in 0..GATES {
            let i = gate * n / GATES;
            let (p, r) = (track.pts[i], track.right[i] * (WALL + 1.0));
            let (low, high) = (Vec3::Y * -2.0, Vec3::Y * 8.0);
            let corners = [p - r + low, p + r + low, p + r + high, p - r + high];
            for tri in [
                [corners[0], corners[1], corners[2]],
                [corners[0], corners[2], corners[3]],
            ] {
                track.course.gates.add_tagged(tri, Surface::default(), gate);
                if gate == 0 {
                    track.course.finish.add(tri, Surface::default());
                }
            }
            track.course.checkpoints.push(Checkpoint {
                normal: -track.flat[i],
                position: p,
                next: vec![(gate + 1) % GATES],
                fraction: 0.0,
            });
        }
        track.course.compute_fractions();
        for (zone, at) in [(2, n / 3), (0, 2 * n / 3)] {
            track.course.zones.push((track.pts[at], WALL + 3.0, zone));
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
        let flat: Vec<Vec3> = fwd
            .iter()
            .map(|f| Vec3::new(f.x, 0.0, f.z).normalize())
            .collect();
        let right = flat.iter().map(|f| Vec3::new(-f.z, 0.0, f.x)).collect();
        let curv = (0..n)
            .map(|i| flat[i].angle_between(flat[(i + 1) % n]) / spacing)
            .collect();
        Track {
            pts,
            fwd,
            flat,
            right,
            curv,
            spacing,
            length,
            road,
            collision: Collision::default(),
            course: Course::default(),
            surfaces: HashMap::new(),
            lanes: Vec::new(),
            clearings: Vec::new(),
            tunnels: Vec::new(),
        }
    }

    /// The lateral offset nearest `wanted` that is open at distance `s`.
    pub fn lane(&self, s: f32, wanted: f32) -> f32 {
        let s = s.rem_euclid(self.length);
        self.lanes
            .iter()
            .filter(|lane| s >= lane.from && s <= lane.to)
            .fold(wanted, |lat, lane| lat.clamp(lane.least, lane.most))
    }

    pub fn n(&self) -> usize {
        self.pts.len()
    }

    /// Where along the lap the finish line is crossed.
    fn finish_distance(&self) -> f32 {
        let lift = Vec3::Y;
        (0..self.n())
            .find(|&i| {
                self.course
                    .finish
                    .any(self.pts[i] + lift, self.pts[(i + 1) % self.n()] + lift)
                    .is_some()
            })
            .map_or(0.0, |i| i as f32 * self.spacing)
    }

    /// Turns the circuit round, to be raced the other way: the racing line, the
    /// checkpoints and the lap zones are walked backwards, and the grid is put on what
    /// was the far side of the finish line.
    pub fn reverse(&mut self) {
        // Two columns, the same way the grid is drawn up on a circuit without one.
        let finish = self.finish_distance();
        self.course.grid = (0..6)
            .map(|place| {
                // The first place on a grid is the one furthest back.
                let s = finish + 8.0 + ((5 - place) / 2) as f32 * 6.0;
                let lat = self.road * if place % 2 == 0 { 0.375 } else { -0.375 };
                (
                    self.surface_point(s, lat),
                    -self.sample(s).1.with_y(0.0).normalize(),
                )
            })
            .collect();

        // The line: the same samples from the same first one, the other way.
        let n = self.n();
        self.pts[1..].reverse();
        for list in [&mut self.fwd, &mut self.flat, &mut self.right] {
            list[1..].reverse();
            for v in list.iter_mut() {
                *v = -*v;
            }
        }
        self.curv = (0..n)
            .map(|i| self.flat[i].angle_between(self.flat[(i + 1) % n]) / self.spacing)
            .collect();

        // Each gate leads to the ones that led to it, the main route's first.
        let gates = &mut self.course.checkpoints;
        let mut before = vec![Vec::new(); gates.len()];
        for main in [true, false] {
            for (from, gate) in gates.iter().enumerate() {
                for (branch, &to) in gate.next.iter().enumerate() {
                    if (branch == 0) == main && to < before.len() && !before[to].contains(&from) {
                        before[to].push(from);
                    }
                }
            }
        }
        for (gate, next) in gates.iter_mut().zip(before) {
            (gate.normal, gate.next) = (-gate.normal, next);
        }
        if !gates.is_empty() {
            self.course.compute_fractions();
        }
        // A tunnel is entered by what was its way out.
        for tunnel in &mut self.tunnels {
            *tunnel = (n + 1 - tunnel.1, n + 1 - tunnel.0);
        }
        // What was on the right is on the left, and as far from the line the other way.
        for lane in &mut self.lanes {
            *lane = Lane {
                from: self.length - lane.to,
                to: self.length - lane.from,
                least: -lane.most,
                most: -lane.least,
            };
        }
        // The stretch after the line is now the one before it.
        for zone in &mut self.course.zones {
            zone.2 = match zone.2 {
                0 => 2,
                2 => 0,
                other => other,
            };
        }
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
        self.collision
            .ground(p + Vec3::Y * 4.0, 12.0)
            .map_or(p, |hit| hit.point)
    }

    /// The sample closest to `pos`, height included: where the road passes over
    /// itself, the level `pos` is on.
    pub fn nearest(&self, pos: Vec3) -> usize {
        (0..self.n())
            .min_by(|&a, &b| {
                self.pts[a]
                    .distance_squared(pos)
                    .total_cmp(&self.pts[b].distance_squared(pos))
            })
            .unwrap()
    }

    /// Whether each sample's road is carried over another stretch of the circuit, so
    /// that what holds it up has to leave the way underneath clear.
    fn bridged(&self) -> Vec<bool> {
        let reach = (WALL + 4.0).powi(2);
        let edges = [-WALL - 0.8, 0.0, WALL + 0.8];
        (0..self.n())
            .map(|i| {
                self.pts.iter().any(|&below| {
                    self.pts[i].y - below.y > 6.0
                        && edges
                            .iter()
                            .any(|&lat| xz_dist2(self.pts[i] + self.right[i] * lat, below) < reach)
                })
            })
            .collect()
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
        (
            best,
            best as f32 * self.spacing + d.dot(self.flat[best]),
            d.dot(self.right[best]),
        )
    }

    pub fn build_mesh(&self) -> Mesh {
        let mut b = BrickMesh::default();
        let n = self.n();
        let up = Vec3::Y;
        let road = [Color::srgb(0.25, 0.26, 0.28), Color::srgb(0.28, 0.29, 0.31)];
        let bridged = self.bridged();
        let span = |i: usize| bridged[i] || bridged[(i + 1) % n];
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
                if (p0.y > 0.05 || p1.y > 0.05) && !span(i) {
                    let (o0, o1) = at(side * (WALL + 0.8));
                    let (g0, g1) = (o0.with_y(-0.1), o1.with_y(-0.1));
                    b.quad(o0, o1, g1, g0, TAN);
                    b.quad(o1, o0, g0, g1, TAN);
                }
            }

            if span(i) {
                // A bridge: a deck under the road, and nothing under that.
                let rot = Transform::IDENTITY.looking_to(p1 - p0, up).rotation;
                let half = Vec3::new(WALL + 0.8, 0.4, p0.distance(p1) / 2.0 + 0.05);
                b.cuboid((p0 + p1) / 2.0 - up * 0.45, half, rot, GREY);
            }
            if let Some(&(from, to)) = self.tunnels.iter().find(|t| (t.0..t.1).contains(&i)) {
                // A tunnel: a wall either side and a roof, with a portal at each end.
                let rot = Transform::IDENTITY.looking_to(p1 - p0, up).rotation;
                let (mid, length) = ((p0 + p1) / 2.0, p0.distance(p1) / 2.0 + 0.05);
                let portal = i < from + 2 || i + 2 >= to;
                let (colour, extra) = match (portal, (i / 4) % 2 == 0) {
                    (true, _) => (YELLOW, 0.6),
                    (false, true) => (GREY, 0.0),
                    (false, false) => (DARK_GREY, 0.0),
                };
                let across = (r0 + r1).normalize() * (WALL + 0.8 + TUNNEL_THICK / 2.0);
                let wall = Vec3::new(TUNNEL_THICK / 2.0 + extra, TUNNEL_HEIGHT / 2.0, length);
                for side in [-1.0, 1.0] {
                    b.cuboid(
                        mid + across * side + up * (TUNNEL_HEIGHT / 2.0 - 0.1),
                        wall,
                        rot,
                        colour,
                    );
                }
                let roof = Vec3::new(
                    WALL + 0.8 + TUNNEL_THICK + extra,
                    TUNNEL_THICK / 2.0 + extra,
                    length,
                );
                b.brick(
                    mid + up * (TUNNEL_HEIGHT + TUNNEL_THICK / 2.0),
                    roof,
                    rot,
                    colour,
                    (8, 1),
                );
            }
            if span(i) != span((i + n - 1) % n) {
                // Where the embankment stops for the bridge: its end, and a pier at
                // each corner.
                let (a, c) = (p0 - r0 * (WALL + 0.8), p0 + r0 * (WALL + 0.8));
                let (a0, c0) = (a.with_y(-0.1), c.with_y(-0.1));
                b.quad(a, c, c0, a0, TAN);
                b.quad(c, a, a0, c0, TAN);
                let rot = Transform::IDENTITY.looking_to(self.flat[i], up).rotation;
                for corner in [a, c] {
                    let half = Vec3::new(1.2, corner.y / 2.0 + 1.0, 1.2);
                    b.brick(corner.with_y(half.y), half, rot, YELLOW, (2, 2));
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
    pub fn build_scenery(&self, layout: Layout, rng: &mut Rng) -> Mesh {
        let mut b = BrickMesh::default();
        let (least, most) = layout.grounds();
        for _ in 0..420 {
            let pos = Vec3::new(rng.range(least.x, most.x), 0.0, rng.range(least.y, most.y));
            let clear = self
                .pts
                .iter()
                .map(|&p| xz_dist2(p, pos))
                .fold(f32::MAX, f32::min)
                .sqrt();
            if clear < WALL + 7.0
                || self
                    .clearings
                    .iter()
                    .any(|&(centre, radius)| xz_dist2(centre, pos) < radius * radius)
            {
                continue;
            }
            let rot = Quat::from_rotation_y(rng.range(0.0, std::f32::consts::TAU));
            if rng.f() < 0.65 {
                let s = rng.range(0.8, 1.7);
                b.cuboid(
                    pos + Vec3::Y * 1.5 * s,
                    Vec3::new(0.6, 1.5, 0.6) * s,
                    rot,
                    BROWN,
                );
                let leaf = rng.pick(&[GREEN, GREEN, LIME]);
                for (k, studs) in [4u32, 3, 2, 1].into_iter().enumerate() {
                    let w = 0.6 * studs as f32 * s;
                    let y = (3.6 + 1.2 * k as f32) * s;
                    b.brick(
                        pos + Vec3::Y * y,
                        Vec3::new(w, 0.6 * s, w),
                        rot,
                        leaf,
                        (studs, studs),
                    );
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
        for layout in Layout::ALL {
            let t = Track::built(layout);
            let n = t.n();
            let longest = if layout == Layout::Gauntlet {
                3500.0
            } else {
                1400.0
            };
            assert!(
                t.length > 900.0 && t.length < longest,
                "{layout:?} length {}",
                t.length
            );
            for i in 0..n {
                let j = (i + 1) % n;
                for side in [-1.0, 1.0] {
                    let a = t.pts[i] + t.right[i] * side * (WALL + 0.8);
                    let b = t.pts[j] + t.right[j] * side * (WALL + 0.8);
                    assert!(
                        (b - a).dot(t.fwd[i]) > 0.2,
                        "{layout:?} barrier folds at {i}"
                    );
                }
                // Any other part of the track must be far away unless it's nearby along
                // the lap, or passes well overhead.
                for k in 0..n {
                    let along = (i as i32 - k as i32)
                        .rem_euclid(n as i32)
                        .min((k as i32 - i as i32).rem_euclid(n as i32));
                    let apart = xz_dist2(t.pts[i], t.pts[k]).sqrt() > 2.0 * WALL + 4.0;
                    let level = (t.pts[i].y - t.pts[k].y).abs() < DECK - 1.0;
                    assert!(along <= 30 || apart || !level, "{layout:?} {i} near {k}");
                }
            }
        }
    }

    #[test]
    fn the_figure_eight_crosses_itself_once_by_a_bridge() {
        let t = Track::built(Layout::FigureEight);
        let bridged = t.bridged();
        let n = t.n();
        let starts = (0..n)
            .filter(|&i| bridged[i] && !bridged[(i + n - 1) % n])
            .count();
        assert_eq!(starts, 1);
        assert!(Track::new().bridged().iter().all(|b| !b));
        // On the bridge and under it, the road found is the one the racing line is on,
        // and so is the sample.
        for i in (0..n).filter(|&i| bridged[i]) {
            let s = i as f32 * t.spacing;
            assert!(
                (t.surface_point(s, 0.0).y - t.pts[i].y).abs() < 0.2,
                "deck at {i}"
            );
            assert_eq!(t.nearest(t.pts[i]), i);
            let under = (0..n)
                .find(|&k| t.pts[i].y - t.pts[k].y > 6.0 && xz_dist2(t.pts[i], t.pts[k]) < 4.0);
            if let Some(k) = under {
                assert!(
                    t.surface_point(k as f32 * t.spacing, 0.0).y < 0.5,
                    "road under {i}"
                );
                assert_eq!(t.nearest(t.pts[k]), k);
            }
        }
        // Neither level's gates or lap zones reach the other.
        for gate in &t.course.checkpoints {
            let i = t.nearest(gate.position);
            assert!(!bridged[i], "gate on the bridge at {i}");
        }
        for &(centre, radius, _) in &t.course.zones {
            let crossing = t.pts[(0..n).find(|&i| bridged[i]).unwrap()];
            assert!(xz_dist2(centre, crossing).sqrt() > 2.0 * radius);
        }
    }

    #[test]
    fn the_gauntlet_has_two_tunnels_and_a_bridge_and_corners_sharper_than_the_others() {
        let t = Track::built(Layout::Gauntlet);
        let (n, bridged) = (t.n(), t.bridged());
        assert_eq!(
            (0..n)
                .filter(|&i| bridged[i] && !bridged[(i + n - 1) % n])
                .count(),
            1
        );
        // Each tunnel is a good stretch of level road, clear of the bridge.
        assert_eq!(t.tunnels.len(), 2);
        for &(from, to) in &t.tunnels {
            assert!((to - from) as f32 * t.spacing > 100.0, "{from}..{to}");
            assert!((from..to).all(|i| t.pts[i].y < 1.0 && !bridged[i]));
        }
        // Turned round, the same road is roofed.
        let mut back = Track::built(Layout::Gauntlet);
        back.reverse();
        for (&(from, to), &(back_from, back_to)) in t.tunnels.iter().zip(&back.tunnels) {
            assert_eq!(
                (back.pts[back_from], back.pts[back_to - 1]),
                (t.pts[to - 1], t.pts[from])
            );
        }
        let sharpest = |t: &Track| t.curv.iter().copied().fold(0.0, f32::max);
        assert!(sharpest(&t) > 1.0 / 20.0 && sharpest(&t) > sharpest(&Track::new()));
    }

    #[test]
    fn a_reversed_circuit_is_the_same_road_the_other_way() {
        for layout in Layout::ALL {
            let (forward, mut back) = (Track::built(layout), Track::built(layout));
            back.reverse();
            let n = forward.n();
            assert_eq!(back.pts[0], forward.pts[0]);
            for i in 1..n {
                assert_eq!(back.pts[i], forward.pts[n - i]);
                assert!(back.fwd[i].dot(forward.fwd[n - i]) < -0.999);
                assert!((back.pts[(i + 1) % n] - back.pts[i]).dot(back.flat[i]) > 0.0);
            }
            // The gates run the other way round the lap, each against its old self.
            let (was, now) = (&forward.course.checkpoints, &back.course.checkpoints);
            assert_eq!(now[0].next, [was.len() - 1]);
            assert_eq!(now[1].next, [0]);
            assert!((now[1].fraction - was[was.len() - 1].fraction).abs() < 1e-6);
            assert!(now.iter().zip(was).all(|(a, b)| a.normal == -b.normal));
            // The grid waits before the line, facing it, on what was the way out.
            for &(position, facing) in &back.course.grid {
                let (_, s, _) = back.project(position, back.nearest(position));
                assert!(s > back.length - 30.0, "{layout:?} grid at {s}");
                assert!(facing.dot(back.flat[back.nearest(position)]) > 0.95);
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
            assert!(
                ds.abs() < 0.3 && (lat2 - lat).abs() < 0.3,
                "{s} {lat} -> {s2} {lat2}"
            );
        }
    }
}
