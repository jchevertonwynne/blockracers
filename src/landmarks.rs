//! What a map may stand beside its road to know it by (`scenery`): a tower, a
//! grandstand, a gantry over the road, a pond, rocks. The port's own. The helter
//! skelter has one of each: the tower in the middle of its spiral, which comes down
//! round it, a grandstand by the start, a gantry at the jump, a pond, and rocks on
//! the island the road swings out round. None of it is solid, and all of it wants
//! standing off the road.

use crate::meshgen::*;
use bevy::prelude::*;
use serde::Deserialize;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Tower,
    /// A grandstand: rows of seats, a crowd and a roof.
    Stand,
    /// A striped beam on two legs, across a road.
    Gantry,
    Pond,
    Rocks,
}

/// One of them, where it stands on the unmirrored circuit, as (x, z), and the way
/// it faces, in degrees round from east to north: a grandstand looks that way and a
/// gantry spans a road that runs that way.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Landmark {
    pub kind: Kind,
    pub place: [f32; 2],
    #[serde(default)]
    pub heading: f32,
    /// How long a grandstand is.
    #[serde(default = "Landmark::length")]
    pub length: f32,
    /// How far across a pond is, from its middle.
    #[serde(default = "Landmark::radius")]
    pub radius: f32,
}

/// How many rows a grandstand has.
const STAND_ROWS: usize = 5;
/// How far from the middle of the road a gantry's legs are, and how high its beam.
const GANTRY_LEGS: f32 = 10.6;
const GANTRY_HEIGHT: f32 = 15.0;

impl Landmark {
    fn length() -> f32 {
        80.0
    }

    fn radius() -> f32 {
        30.0
    }

    /// Where it is on the circuit as it is being raced, mirrored or not.
    fn at(&self) -> Vec3 {
        let side = if crate::scenery::mirror() { -1.0 } else { 1.0 };
        Vec3::new(self.place[0], 0.0, self.place[1] * side)
    }

    /// The way it faces along the ground, as it is being raced.
    fn facing(&self) -> Vec3 {
        let side = if crate::scenery::mirror() { -1.0 } else { 1.0 };
        let (sin, cos) = self.heading.to_radians().sin_cos();
        // The points of the compass are exact, so that what is square is square.
        let whole = |v: f32| if (v - v.round()).abs() < 1e-6 { v.round() } else { v };
        Vec3::new(whole(cos), 0.0, -whole(sin) * side)
    }
}

/// The ground the scattered scenery is kept off: centres and radii.
pub fn clearings(landmarks: &[Landmark]) -> Vec<(Vec3, f32)> {
    let mut clearings = Vec::new();
    for landmark in landmarks {
        let (at, facing) = (landmark.at(), landmark.facing());
        let across = Vec3::new(-facing.z, 0.0, facing.x);
        let rows = -across;
        match landmark.kind {
            Kind::Tower => clearings.push((at, 16.0)),
            Kind::Pond => clearings.push((at, landmark.radius + 6.0)),
            Kind::Rocks => clearings.push((at, 9.0)),
            Kind::Stand => {
                for along in [-0.4, -0.2, 0.0, 0.2, 0.4] {
                    clearings.push((at + rows * landmark.length * along, 22.0));
                }
            }
            Kind::Gantry => {
                for side in [-1.0, 1.0] {
                    clearings.push((at + across * GANTRY_LEGS * side, 5.0));
                }
            }
        }
    }
    clearings
}

/// Builds a map's landmarks of bricks.
pub fn draw(landmarks: &[Landmark], b: &mut BrickMesh) {
    for landmark in landmarks {
        match landmark.kind {
            Kind::Tower => tower(landmark, b),
            Kind::Stand => stand(landmark, b),
            Kind::Gantry => gantry(landmark, b),
            Kind::Pond => pond(landmark, b),
            Kind::Rocks => rocks(landmark, b),
        }
    }
}

/// The turn that takes east to `way`, which lies along the ground.
fn turned(way: Vec3) -> Quat {
    if way == Vec3::X {
        Quat::IDENTITY
    } else {
        Quat::from_rotation_y((-way.z).atan2(way.x))
    }
}

/// A striped shaft on a plinth, a deck round its top, a roof in rings and a flag.
fn tower(landmark: &Landmark, b: &mut BrickMesh) {
    let (up, still) = (Vec3::Y, Quat::IDENTITY);
    let tower = landmark.at();
    b.cyl(tower, 11.0, 3.0, still, GREY);
    const BANDS: usize = 9;
    for band in 0..BANDS {
        let radius = 8.5 - 0.3 * band as f32;
        let colour = if band % 2 == 0 { RED } else { WHITE };
        b.cyl(
            tower + up * (3.0 + 4.5 * band as f32),
            radius,
            4.5,
            still,
            colour,
        );
    }
    let top = tower + up * (3.0 + 4.5 * BANDS as f32);
    b.cyl(top, 10.0, 1.2, still, YELLOW);
    for ring in 0..6 {
        let radius = 8.5 - 1.4 * ring as f32;
        b.cyl(
            top + up * (5.2 + 1.6 * ring as f32),
            radius,
            1.6,
            still,
            BLUE,
        );
    }
    for post in 0..8 {
        let turn = Quat::from_rotation_y(post as f32 * std::f32::consts::FRAC_PI_4);
        b.cyl(
            top + turn * Vec3::X * 7.5 + up * 1.2,
            0.4,
            4.0,
            still,
            WHITE,
        );
    }
    let pole = top + up * 14.8;
    b.cyl(pole, 0.3, 8.0, still, WHITE);
    b.cuboid(
        pole + Vec3::new(2.2, 6.6, 0.0),
        Vec3::new(2.2, 1.3, 0.1),
        still,
        RED,
    );
}

/// Rows stepping up away from what it faces, a crowd on them, and a roof on posts.
fn stand(landmark: &Landmark, b: &mut BrickMesh) {
    let up = Vec3::Y;
    let (stand, back) = (landmark.at(), -landmark.facing());
    // Along its rows, and the turn that lays a brick along them.
    let along = Vec3::new(-back.z, 0.0, back.x);
    let turn = turned(along);
    let half = landmark.length / 2.0;
    let crowd = [RED, YELLOW, BLUE, WHITE, ORANGE, GREEN];
    let seats = (landmark.length / 3.6) as usize;
    for row in 0..STAND_ROWS {
        let height = 1.2 * (row + 1) as f32;
        let centre = stand + back * 3.0 * row as f32;
        let colour = if row % 2 == 0 { BLUE } else { WHITE };
        b.brick(
            centre + up * height / 2.0,
            Vec3::new(half, height / 2.0, 1.5),
            turn,
            colour,
            (half as u32, 1),
        );
        for seat in 0..seats {
            let at = centre + along * (seat as f32 * 3.6 - half + 2.2 + (row % 2) as f32);
            let shirt = crowd[(seat * 7 + row * 3) % crowd.len()];
            b.cuboid(
                at + up * (height + 0.9),
                Vec3::new(0.6, 0.9, 0.4),
                turn,
                shirt,
            );
            b.cyl(at + up * (height + 1.8), 0.45, 0.8, Quat::IDENTITY, YELLOW);
        }
    }
    let rear = stand + back * (3.0 * STAND_ROWS as f32 - 1.0);
    let roof = 1.2 * STAND_ROWS as f32 + 6.0;
    for end in [-1.0, 0.0, 1.0] {
        let post = rear + along * (half - 1.0) * end;
        b.cuboid(
            post + up * roof / 2.0,
            Vec3::new(0.6, roof / 2.0, 0.6),
            turn,
            YELLOW,
        );
    }
    b.brick(
        stand + back * (1.5 * STAND_ROWS as f32 - 1.5) + up * (roof + 0.5),
        Vec3::new(half + 2.0, 0.5, 1.5 * STAND_ROWS as f32 + 2.0),
        turn,
        RED,
        ((half / 2.0) as u32, 5),
    );
}

/// A striped beam on two legs, across a road that runs the way it faces.
fn gantry(landmark: &Landmark, b: &mut BrickMesh) {
    let up = Vec3::Y;
    let (gantry, facing) = (landmark.at(), landmark.facing());
    let (across, turn) = (Vec3::new(-facing.z, 0.0, facing.x), turned(facing));
    for side in [-1.0, 1.0] {
        let leg = gantry + across * GANTRY_LEGS * side;
        let half = Vec3::new(0.9, GANTRY_HEIGHT / 2.0, 0.9);
        b.brick(leg + up * GANTRY_HEIGHT / 2.0, half, turn, YELLOW, (2, 2));
    }
    const STRIPES: usize = 12;
    let wide = (GANTRY_LEGS + 0.9) / STRIPES as f32;
    for stripe in 0..2 * STRIPES {
        let out = (stripe as f32 + 0.5) * wide - GANTRY_LEGS - 0.9;
        let colour = if stripe % 2 == 0 { BLACK } else { YELLOW };
        b.cuboid(
            gantry + across * out + up * (GANTRY_HEIGHT + 1.2),
            Vec3::new(0.9, 1.2, wide / 2.0),
            turn,
            colour,
        );
    }
}

/// Water, with a bank of stones round part of it and a boat on it.
fn pond(landmark: &Landmark, b: &mut BrickMesh) {
    let (up, still) = (Vec3::Y, Quat::IDENTITY);
    let (pond, radius) = (landmark.at(), landmark.radius);
    b.cyl(pond - up * 0.03, radius, 0.06, still, BLUE);
    for stone in 0..9 {
        let turn = Quat::from_rotation_y(0.5 + stone as f32 * 0.42);
        let size = 1.6 + (stone % 3) as f32 * 0.7;
        b.brick(
            pond + turn * Vec3::X * (radius + 1.0) + up * size * 0.4,
            Vec3::new(size, size * 0.4, size),
            turn,
            if stone % 2 == 0 { GREY } else { DARK_GREY },
            (2, 2),
        );
    }
    let boat = pond + Vec3::new(-8.0, 0.0, 6.0);
    let heading = Quat::from_rotation_y(0.6);
    b.brick(
        boat + up * 0.6,
        Vec3::new(1.8, 0.6, 4.0),
        heading,
        RED,
        (2, 4),
    );
    b.cyl(boat + up * 1.2, 0.2, 6.0, still, BROWN);
    b.cuboid(boat + up * 4.6, Vec3::new(0.08, 2.2, 1.6), heading, WHITE);
}

/// A few rocks, the biggest with another on it.
fn rocks(landmark: &Landmark, b: &mut BrickMesh) {
    let up = Vec3::Y;
    let island = landmark.at();
    for (rock, &(x, z, size)) in [
        (0.0, 0.0, 2.6),
        (2.0, 5.0, 1.8),
        (-2.0, 8.0, 1.5),
        (1.5, -6.0, 2.1),
        (-1.5, -9.0, 1.2),
    ]
    .iter()
    .enumerate()
    {
        let turn = Quat::from_rotation_y(rock as f32 * 1.3);
        let colour = if rock % 2 == 0 { GREY } else { DARK_GREY };
        let at = island + Vec3::new(x, 0.0, z);
        b.brick(
            at + up * size * 0.5,
            Vec3::new(size, size * 0.5, size),
            turn,
            colour,
            (2, 2),
        );
        if rock == 0 {
            b.brick(
                at + up * size * 1.4,
                Vec3::splat(size * 0.4).with_x(size * 0.6),
                turn,
                DARK_GREY,
                (1, 1),
            );
        }
    }
}
