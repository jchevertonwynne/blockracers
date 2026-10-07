//! What stands round the helter skelter (`track::Layout::HelterSkelter`), a circuit of
//! the port's own: the tower its spiral winds round, a grandstand by the start, a
//! gantry at the jump, a pond, and rocks on the island the road swings out round.
//! None of it is solid; all of it is off the road.

use crate::meshgen::*;
use bevy::prelude::*;

/// Where each stands on the unmirrored circuit, as (x, z).
const TOWER: [f32; 2] = [495.1, 684.1];
const STAND: [f32; 2] = [60.0, -27.0];
const GANTRY: [f32; 2] = [216.6, 524.0];
const POND: [f32; 2] = [50.0, 400.0];
const ISLAND: [f32; 2] = [648.0, 554.0];

/// How long the grandstand is, and how many rows it has.
const STAND_LENGTH: f32 = 80.0;
const STAND_ROWS: usize = 5;
const POND_RADIUS: f32 = 34.0;
/// How far from the middle of the road the gantry's legs are, and how high its beam.
const GANTRY_LEGS: f32 = 13.6;
const GANTRY_HEIGHT: f32 = 15.0;

/// A place on the circuit as it is being raced, mirrored or not.
fn place([x, z]: [f32; 2]) -> Vec3 {
    let side = if crate::scenery::mirror() { -1.0 } else { 1.0 };
    Vec3::new(x, 0.0, z * side)
}

/// The ground the scattered scenery is kept off: centres and radii.
pub fn clearings() -> Vec<(Vec3, f32)> {
    let mut clearings = vec![
        (place(TOWER), 16.0),
        (place(POND), POND_RADIUS + 6.0),
        (place(ISLAND), 12.0),
    ];
    for along in [-0.4, -0.2, 0.0, 0.2, 0.4] {
        clearings.push((place(STAND) + Vec3::X * STAND_LENGTH * along, 22.0));
    }
    for side in [-1.0, 1.0] {
        clearings.push((place(GANTRY) + Vec3::X * GANTRY_LEGS * side, 5.0));
    }
    clearings
}

pub fn scenery(b: &mut BrickMesh) {
    let (up, still) = (Vec3::Y, Quat::IDENTITY);

    // The tower: a striped shaft on a plinth, a deck round its top, a roof in rings
    // and a flag.
    let tower = place(TOWER);
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

    // The grandstand: rows stepping up away from the road, a crowd on them, and a
    // roof on posts. It faces the road whichever side of it the circuit puts it.
    let stand = place(STAND);
    let back = Vec3::Z * stand.z.signum();
    let half = STAND_LENGTH / 2.0;
    let crowd = [RED, YELLOW, BLUE, WHITE, ORANGE, GREEN];
    for row in 0..STAND_ROWS {
        let height = 1.2 * (row + 1) as f32;
        let centre = stand + back * 3.0 * row as f32;
        let colour = if row % 2 == 0 { BLUE } else { WHITE };
        b.brick(
            centre + up * height / 2.0,
            Vec3::new(half, height / 2.0, 1.5),
            still,
            colour,
            (40, 1),
        );
        for seat in 0..22 {
            let at = centre + Vec3::X * (seat as f32 * 3.6 - half + 2.2 + (row % 2) as f32);
            let shirt = crowd[(seat * 7 + row * 3) % crowd.len()];
            b.cuboid(
                at + up * (height + 0.9),
                Vec3::new(0.6, 0.9, 0.4),
                still,
                shirt,
            );
            b.cyl(at + up * (height + 1.8), 0.45, 0.8, still, YELLOW);
        }
    }
    let rear = stand + back * (3.0 * STAND_ROWS as f32 - 1.0);
    let roof = 1.2 * STAND_ROWS as f32 + 6.0;
    for end in [-1.0, 0.0, 1.0] {
        let post = rear + Vec3::X * (half - 1.0) * end;
        b.cuboid(
            post + up * roof / 2.0,
            Vec3::new(0.6, roof / 2.0, 0.6),
            still,
            YELLOW,
        );
    }
    b.brick(
        stand + back * (1.5 * STAND_ROWS as f32 - 1.5) + up * (roof + 0.5),
        Vec3::new(half + 2.0, 0.5, 1.5 * STAND_ROWS as f32 + 2.0),
        still,
        RED,
        (20, 5),
    );

    // The gantry at the foot of the jump: a striped beam on two legs.
    let gantry = place(GANTRY);
    for side in [-1.0, 1.0] {
        let leg = gantry + Vec3::X * GANTRY_LEGS * side;
        let half = Vec3::new(0.9, GANTRY_HEIGHT / 2.0, 0.9);
        b.brick(leg + up * GANTRY_HEIGHT / 2.0, half, still, YELLOW, (2, 2));
    }
    const STRIPES: usize = 12;
    let wide = (GANTRY_LEGS + 0.9) / STRIPES as f32;
    for stripe in 0..2 * STRIPES {
        let across = (stripe as f32 + 0.5) * wide - GANTRY_LEGS - 0.9;
        let colour = if stripe % 2 == 0 { BLACK } else { YELLOW };
        b.cuboid(
            gantry + Vec3::X * across + up * (GANTRY_HEIGHT + 1.2),
            Vec3::new(wide / 2.0, 1.2, 0.9),
            still,
            colour,
        );
    }

    // The pond, with a bank of stones round part of it and a boat on it.
    let pond = place(POND);
    b.cyl(pond - up * 0.03, POND_RADIUS, 0.06, still, BLUE);
    for stone in 0..9 {
        let turn = Quat::from_rotation_y(0.5 + stone as f32 * 0.42);
        let size = 1.6 + (stone % 3) as f32 * 0.7;
        b.brick(
            pond + turn * Vec3::X * (POND_RADIUS + 1.0) + up * size * 0.4,
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

    // Rocks on the island.
    let island = place(ISLAND);
    for (rock, &(x, z, size)) in [
        (0.0, 0.0, 3.2),
        (5.0, 3.0, 2.2),
        (-4.0, 5.0, 1.8),
        (3.0, -6.0, 2.6),
        (-3.0, -4.0, 1.4),
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
