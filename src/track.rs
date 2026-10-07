//! The circuit: a closed spline resampled to evenly spaced points. Karts, projectiles and
//! pickups all live in "track space" (distance along the track + lateral offset), which
//! keeps collisions with the walls and the hills trivial.

use crate::assets::materials::Surface;
use crate::collision::Collision;
use crate::meshgen::*;
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::TAU;

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

/// A piece of a circuit that is drawn rather than plotted, from where the last one
/// left off.
enum Leg {
    /// Straight on for a length, climbing by the second.
    Straight(f32, f32),
    /// Round to the right through so many degrees, or to the left if they are fewer
    /// than none, at a radius, climbing by the last.
    Turn(f32, f32, f32),
}
use Leg::{Straight, Turn};

/// The helter skelter, from the start line and back to it: esses, rollers, a tunnel
/// that winds, switchbacks over a hill, a swing out round an island, a spiral of two
/// turns up over itself, the long way down from the top of it, a jump, a tunnel with a
/// kink in it, a crest, a chicane and a hairpin with rollers after it, and more esses
/// on the way home. The lengths of the straights before the island and either side of
/// the crest are what bring it back to where it began.
const HELTER: &[Leg] = &[
    Straight(170.0, 0.0),
    // Esses.
    Turn(-40.0, 34.0, 0.0),
    Turn(80.0, 34.0, 0.0),
    Turn(-80.0, 34.0, 0.0),
    Turn(40.0, 34.0, 0.0),
    // Rollers.
    Straight(15.0, 0.0),
    Straight(15.0, ROLLER),
    Straight(15.0, -ROLLER),
    Straight(15.0, ROLLER),
    Straight(15.0, -ROLLER),
    Straight(15.0, ROLLER),
    Straight(15.0, -ROLLER),
    Straight(15.0, ROLLER),
    Straight(15.0, -ROLLER),
    Straight(15.0, 0.0),
    Turn(90.0, 60.0, 0.0),
    Straight(30.0, 0.0),
    // The winding tunnel.
    Turn(-35.0, 55.0, 0.0),
    Turn(70.0, 55.0, 0.0),
    Turn(-35.0, 55.0, 0.0),
    Straight(30.0, 0.0),
    // Switchbacks over a hill.
    Turn(-90.0, 22.0, 0.0),
    Straight(70.0, 3.0),
    Turn(180.0, 22.0, 2.0),
    Straight(70.0, 1.0),
    Turn(-180.0, 22.0, -1.0),
    Straight(70.0, -3.0),
    Turn(90.0, 22.0, -2.0),
    Straight(110.23, 0.0),
    // Out round the island, which the first of the byways goes straight past.
    Turn(-50.0, 45.0, 0.0),
    Straight(20.0, 0.0),
    Turn(100.0, 45.0, 0.0),
    Straight(20.0, 0.0),
    Turn(-50.0, 45.0, 0.0),
    Straight(40.0, 0.0),
    Turn(90.0, 50.0, 0.0),
    Straight(80.0, 0.0),
    // The spiral: twice round, each turn far enough over the last to drive under.
    Turn(720.0, 42.0, 2.0 * SPIRAL_RISE),
    Straight(50.0, 0.0),
    // The long way down.
    Turn(-60.0, 70.0, -5.0),
    Turn(150.0, 90.0, -17.0),
    Straight(70.0, -6.0),
    // The run up to the jump, and the landing.
    Straight(150.0, 0.0),
    Straight(60.0, 0.0),
    // The tunnel with a kink in it.
    Straight(40.0, 0.0),
    Turn(-30.0, 60.0, 0.0),
    Turn(30.0, 60.0, 0.0),
    Straight(60.0, 0.0),
    Turn(-90.0, 30.0, 0.0),
    // A straight with a crest in the middle of it.
    Straight(90.85, 0.0),
    Straight(20.0, 3.0),
    Straight(20.0, -3.0),
    Straight(90.85, 0.0),
    // Out through a chicane to a hairpin, whose inside the other byway cuts across,
    // and back over rollers.
    Turn(-90.0, 30.0, 0.0),
    Straight(50.0, 0.0),
    Turn(30.0, 40.0, 0.0),
    Turn(-60.0, 40.0, 0.0),
    Turn(30.0, 40.0, 0.0),
    Straight(80.0, 0.0),
    Turn(180.0, 30.0, 0.0),
    Straight(35.0, 0.0),
    Straight(15.0, ROLLER),
    Straight(15.0, -ROLLER),
    Straight(15.0, ROLLER),
    Straight(15.0, -ROLLER),
    Straight(15.0, 0.0),
    // Esses, a kink, and home.
    Turn(40.0, 36.0, 0.0),
    Turn(-80.0, 36.0, 0.0),
    Turn(80.0, 36.0, 0.0),
    Turn(-40.0, 36.0, 0.0),
    Straight(120.0, 0.0),
    Turn(25.0, 50.0, 0.0),
    Turn(-25.0, 50.0, 0.0),
    Straight(30.0, 0.0),
    Turn(90.0, 45.0, 0.0),
    Straight(70.0, 0.0),
];
/// How high the helter skelter's rollers are.
const ROLLER: f32 = 1.5;
/// How far each turn of the helter skelter's spiral is over the one before.
const SPIRAL_RISE: f32 = 14.0;
/// The helter skelter's tunnels: where each begins and ends, as (x, z).
const HELTER_TUNNELS: &[[[f32; 2]; 2]] = &[
    [[511.1, 74.1], [511.1, 200.3]],
    [[216.6, 426.0], [200.5, 306.0]],
];
/// Where the lip of the helter skelter's jump is, as (x, z).
const HELTER_JUMPS: &[[f32; 2]] = &[[216.6, 496.1]];

/// A way off a circuit's road and back onto it further round, straight from one place
/// on the racing line to another, each so far round the lap: how far its tarmac and
/// its barriers are from its middle, how high the bumps along it are, whether it has
/// a shed over it, and how often one of the computer's cars takes it.
struct Byway {
    from: f32,
    to: f32,
    road: f32,
    wall: f32,
    bumps: f32,
    covered: bool,
    taken: f32,
}

/// What a circuit has besides its road, each thing so far round the lap.
struct Extras {
    byways: &'static [Byway],
    /// Corners with grass inside them and no barrier before it, to be cut across:
    /// where the open stretch begins and ends.
    infields: &'static [(f32, f32)],
    /// Speed pads: where, and how far right of the middle of the road.
    pads: &'static [(f32, f32)],
    /// Banked corners: where the bank begins and ends, and how steep it is across the
    /// road at its steepest.
    banks: &'static [(f32, f32, f32)],
}

/// The brick circuit's: a ridge road straight across the dip, the last corner's grass,
/// and the hill's two sweeps banked.
const BRICK_EXTRAS: Extras = Extras {
    byways: &[Byway {
        from: 410.0,
        to: 524.0,
        road: 5.0,
        wall: 7.0,
        bumps: 0.0,
        covered: false,
        taken: 0.4,
    }],
    infields: &[(924.0, 996.0)],
    pads: &[(60.0, 0.0), (570.0, 0.0)],
    banks: &[(236.0, 330.0, 0.15), (606.0, 644.0, 0.15)],
};
/// The figure of eight's: a cut across the west loop's kink, the grass inside that
/// loop's far end, and the east loop banked.
const FIGURE_EIGHT_EXTRAS: Extras = Extras {
    byways: &[Byway {
        from: 956.0,
        to: 1096.0,
        road: 5.0,
        wall: 7.0,
        bumps: 0.0,
        covered: false,
        taken: 0.4,
    }],
    infields: &[(876.0, 936.0)],
    pads: &[(50.0, 0.0), (240.0, 0.0)],
    banks: &[(180.0, 320.0, 0.15)],
};
/// The gauntlet's: a gap between the legs of a hairpin, the grass inside the corner
/// after the lava, and the climbing loop banked.
const GAUNTLET_EXTRAS: Extras = Extras {
    byways: &[Byway {
        from: 1970.0,
        to: 2060.0,
        road: 5.0,
        wall: 6.5,
        bumps: 0.0,
        covered: false,
        taken: 0.3,
    }],
    infields: &[(990.0, 1024.0)],
    pads: &[(1904.0, 0.0), (2554.0, 0.0)],
    banks: &[(2330.0, 2490.0, 0.15)],
};
/// The helter skelter's. One byway goes straight on where the road swings out round
/// the island: the shorter way, but narrow and bumpy. The other cuts across the inside
/// of the hairpin at a right angle to the road, through a shed that hides it. The
/// first corner and the one before the spiral have grass inside them, and the spiral
/// and the sweep down from it are banked.
const HELTER_EXTRAS: Extras = Extras {
    byways: &[
        Byway {
            from: 1279.0,
            to: 1476.0,
            road: 6.0,
            wall: 8.5,
            bumps: 1.2,
            covered: false,
            taken: 0.4,
        },
        Byway {
            from: 3509.0,
            to: 3653.0,
            road: 4.0,
            wall: 5.5,
            bumps: 0.0,
            covered: true,
            taken: 0.25,
        },
    ],
    infields: &[(462.0, 557.0), (1516.0, 1594.0)],
    pads: &[(140.0, 0.0), (1200.0, 0.0), (3120.0, 0.0)],
    banks: &[(1690.0, 2190.0, 0.12), (2335.0, 2550.0, 0.15)],
};
/// How many samples a bank takes to come up to its steepest, and to go down again.
const BANK_EASE: f32 = 12.0;
/// How long and how wide a speed pad is.
pub const PAD_LENGTH: f32 = 8.0;
pub const PAD_WIDTH: f32 = 7.0;
/// How long each bump of a byway is, and how far from the byway's ends they keep, so
/// that it is level where it leaves the road and comes back to it.
const BUMP_LENGTH: f32 = 20.0;
const BUMP_MARGIN: f32 = 40.0;
/// How much the grass inside a corner slows a car, where the verge's is 20.
const LAWN_DRAG: f32 = 30.0;
/// How many samples a lap zone is kept from the ends of a byway or an infield.
const ZONE_CLEAR: usize = 15;
/// How far a byway's surface is over the road's where they share the ground, so that a
/// wheel on both is on the byway.
const BYWAY_LIFT: f32 = 0.01;
/// How far in from a byway's ends its shed begins, how high the shed's roof is and how
/// thick its walls and roof are.
const SHED_INSET: f32 = 12.0;
const SHED_HEIGHT: f32 = 6.0;
const SHED_THICK: f32 = 1.0;

/// How far apart the points of a drawn circuit are put, at most: close and even, so
/// that the spline keeps to the straights and the arcs.
const DRAWN_STEP: f32 = 10.0;

/// The control points of a circuit drawn as `legs`, as (x, height, z): it sets off
/// from the origin along x. The last leg's end is left to the first one's start.
fn drawn(legs: &[Leg]) -> Vec<[f32; 3]> {
    let (mut at, mut height, mut heading) = (Vec2::ZERO, 0.0, 0.0f32);
    let mut points = Vec::new();
    for leg in legs {
        match *leg {
            Straight(length, rise) => {
                let count = (length / DRAWN_STEP).ceil().max(1.0);
                let along = Vec2::from_angle(heading) * length;
                for k in 0..count as usize {
                    let t = k as f32 / count;
                    let p = at + along * t;
                    points.push([p.x, height + rise * t, p.y]);
                }
                (at, height) = (at + along, height + rise);
            }
            Turn(degrees, radius, rise) => {
                let angle = degrees.to_radians();
                // The centre is off to the side turned to.
                let out =
                    |heading: f32| Vec2::from_angle(heading).perp() * -radius * angle.signum();
                let centre = at - out(heading);
                let count = (angle.abs() * radius / DRAWN_STEP)
                    .max(degrees.abs() / 30.0)
                    .ceil();
                for k in 0..count as usize {
                    let t = k as f32 / count;
                    let p = centre + out(heading + angle * t);
                    points.push([p.x, height + rise * t, p.y]);
                }
                heading += angle;
                (at, height) = (centre + out(heading), height + rise);
            }
        }
    }
    points
}

/// A jump, from the foot of its ramp: the road curves up to a lip, drops into a dip,
/// and comes up again to a lower lip with a slope down from it to land on. The dip's
/// sides can be driven up, so a car that falls short gets out, whichever way it is
/// going. These are the lengths of the ramp, of each side of the dip, of its floor and
/// of the landing, and the heights of the two lips.
const JUMP_RAMP: f32 = 24.0;
const JUMP_SIDES: (f32, f32) = (6.0, 6.0);
const JUMP_FLOOR: f32 = 4.0;
const JUMP_LANDING: f32 = 18.0;
const JUMP_LIPS: (f32, f32) = (6.0, 4.5);
/// How long a jump is from end to end.
const JUMP_LENGTH: f32 = JUMP_RAMP + JUMP_SIDES.0 + JUMP_FLOOR + JUMP_SIDES.1 + JUMP_LANDING;

/// How high a jump's road is, `along` it from the foot of the ramp.
fn jump_height(along: f32) -> f32 {
    if along < 0.0 {
        return 0.0;
    }
    if along < JUMP_RAMP {
        return JUMP_LIPS.0 * (along / JUMP_RAMP).powi(2);
    }
    // The stretches after the lip: the length of each, and the heights it runs between.
    let mut left = along - JUMP_RAMP;
    for (length, from, to) in [
        (JUMP_SIDES.0, JUMP_LIPS.0, 0.0),
        (JUMP_FLOOR, 0.0, 0.0),
        (JUMP_SIDES.1, 0.0, JUMP_LIPS.1),
        (JUMP_LANDING, JUMP_LIPS.1, 0.0),
    ] {
        if left < length {
            return from + (to - from) * left / length;
        }
        left -= length;
    }
    0.0
}

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
    HelterSkelter,
}

impl Layout {
    pub const ALL: [Layout; 4] = [
        Layout::Brick,
        Layout::FigureEight,
        Layout::Gauntlet,
        Layout::HelterSkelter,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Brick => "Brick Circuit",
            Layout::FigureEight => "Figure Eight",
            Layout::Gauntlet => "Gauntlet",
            Layout::HelterSkelter => "Helter Skelter",
        }
    }

    /// What `$BRICK_RACE` calls it.
    pub fn key(self) -> &'static str {
        match self {
            Layout::Brick => "BRICK",
            Layout::FigureEight => "FIGURE8",
            Layout::Gauntlet => "GAUNTLET",
            Layout::HelterSkelter => "HELTER",
        }
    }

    fn control(self, mirror: bool) -> Vec<Vec3> {
        let (points, scale) = match self {
            Layout::Brick => (CONTROL.to_vec(), SCALE),
            Layout::FigureEight => (FIGURE_EIGHT.to_vec(), 1.0),
            Layout::Gauntlet => (GAUNTLET.to_vec(), 1.0),
            Layout::HelterSkelter => (drawn(HELTER), 1.0),
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
        match self {
            Layout::Gauntlet => GAUNTLET_TUNNELS,
            Layout::HelterSkelter => HELTER_TUNNELS,
            _ => &[],
        }
    }

    /// What it has besides its road.
    fn extras(self) -> &'static Extras {
        match self {
            Layout::Brick => &BRICK_EXTRAS,
            Layout::FigureEight => &FIGURE_EIGHT_EXTRAS,
            Layout::Gauntlet => &GAUNTLET_EXTRAS,
            Layout::HelterSkelter => &HELTER_EXTRAS,
        }
    }

    /// Where the lips of its jumps are, as (x, z) on the unmirrored circuit.
    fn jumps(self) -> &'static [[f32; 2]] {
        if self == Layout::HelterSkelter {
            HELTER_JUMPS
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
            Layout::HelterSkelter => (Vec2::new(-260.0, -130.0), Vec2::new(790.0, 920.0)),
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
    /// The stretches that are a jump, from the foot of the ramp to the end of the
    /// landing: the first sample of each and the one after its last.
    pub jumps: Vec<(usize, usize)>,
    /// The ways off the road and back onto it.
    pub branches: Vec<Branch>,
    /// The road is a stand-in: the circuit has no route of its own (the test
    /// track, `world::load_in`), and nothing of the game's is to follow this one.
    pub unrouted: bool,
    /// How steeply each sample's road is banked: how much higher it is for each unit
    /// to the right of its middle.
    pub bank: Vec<f32>,
    /// The corners with grass inside them and no barrier before it.
    pub infields: Vec<Infield>,
    pub pads: Vec<Pad>,
}

/// A stretch of the road with no barrier on one side of it, and grass beyond from one
/// end of the stretch to the other.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Infield {
    /// Its first sample, and its last.
    pub from: usize,
    pub to: usize,
    /// Which side the grass is: 1 for the right, -1 for the left.
    pub side: f32,
}

impl Infield {
    /// Whether the barrier that would run on from sample `i` on `side` is left out.
    fn opens(&self, i: usize, side: f32) -> bool {
        (self.from..self.to).contains(&i) && side * self.side > 0.0
    }
}

/// A speed pad: the sample its middle is at, and how far right of the middle of the road.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pad {
    pub at: usize,
    pub lat: f32,
}

/// A way off the road and back onto it: a straight strip of its own, with barriers of
/// its own where it is clear of the road, which has none across its ends.
#[derive(Clone)]
pub struct Branch {
    /// Its middle, at even spacing from one end to the other, bumps and all.
    pub pts: Vec<Vec3>,
    /// The way it goes, on the ground, and what is to the right of that.
    pub along: Vec3,
    pub right: Vec3,
    /// How far its tarmac and the inner faces of its barriers are from its middle.
    pub road: f32,
    pub wall: f32,
    /// Whether it has a shed over it.
    pub covered: bool,
    /// The samples of the road it leaves from and comes back to.
    pub ends: [usize; 2],
    /// How often one of the computer's cars takes it.
    pub taken: f32,
}

impl Branch {
    pub fn length(&self) -> f32 {
        (self.pts[self.pts.len() - 1] - self.pts[0]).dot(self.along)
    }

    /// How far along it a place is, and how far to the right of its middle.
    pub fn place(&self, p: Vec3) -> (f32, f32) {
        let from = p - self.pts[0];
        (from.dot(self.along), from.dot(self.right))
    }

    /// Whether a place on its level is between its barriers.
    fn holds(&self, p: Vec3) -> bool {
        let (along, across) = self.place(p);
        let (first, last) = (self.pts[0], self.pts[self.pts.len() - 1]);
        let level = first.y + (last.y - first.y) * along / self.length();
        along > 0.0
            && along < self.length()
            && across.abs() < self.wall
            && (p.y - level).abs() < 3.0
    }

    /// The place on its middle this far along it, which may be past either end.
    pub fn point(&self, along: f32) -> Vec3 {
        let at = (along / self.length()).clamp(0.0, 1.0) * (self.pts.len() - 1) as f32;
        let i = (at as usize).min(self.pts.len() - 2);
        self.pts[i].lerp(self.pts[i + 1], at - i as f32)
            + self.along * (along - along.clamp(0.0, self.length()))
    }

    /// Whether its shed is over the stretch that begins `along` it.
    fn roofed(&self, along: f32) -> bool {
        self.covered && along >= SHED_INSET && along < self.length() - SHED_INSET
    }
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

    /// The road of a built-in circuit as it is unmirrored: its line, jumps and banks,
    /// and nothing to drive on.
    pub fn plain(layout: Layout) -> Self {
        Track::shaped(layout, false)
    }

    /// The line of a built-in circuit, with its tunnels, jumps and banks.
    fn shaped(layout: Layout, mirror: bool) -> Self {
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
        // A jump is cut into level road, its lip on the sample nearest where it is put.
        let n = track.n();
        let ramp = (JUMP_RAMP / track.spacing).round() as usize;
        let after = ((JUMP_LENGTH - JUMP_RAMP) / track.spacing).round() as usize;
        track.jumps = layout
            .jumps()
            .iter()
            .map(|&[x, z]| track.nearest(Vec3::new(x, 0.0, z * side)))
            .map(|lip| (lip - ramp, lip + after + 1))
            .collect();
        for (from, to) in track.jumps.clone() {
            for i in from..to {
                // Measured from the lip, so that the lip is as high as it should be.
                let past = (i as f32 - (from + ramp) as f32) * track.spacing;
                track.pts[i].y = jump_height(JUMP_RAMP + past);
            }
        }
        // A banked corner is tipped about its inner edge, which stays where it was:
        // the outside of the corner is the high side.
        for &(from, to, tilt) in layout.extras().banks {
            let (from, to) = (track.index(from), track.index(to));
            let turn: f32 = (from..to)
                .map(|i| track.flat[i].cross(track.flat[i + 1]).y)
                .sum();
            for i in from..=to {
                let ease = ((i - from).min(to - i) as f32 / BANK_EASE).min(1.0);
                let ease = ease * ease * (3.0 - 2.0 * ease);
                track.bank[i] = tilt * ease * turn.signum();
                track.pts[i].y += tilt * ease * WALL;
            }
        }
        track.fwd = (0..n)
            .map(|i| (track.pts[(i + 1) % n] - track.pts[(i + n - 1) % n]).normalize())
            .collect();
        track
    }

    /// The sample this far round the lap.
    fn index(&self, s: f32) -> usize {
        (s / self.spacing).round() as usize % self.n()
    }

    /// The place `lat` to the right of the middle of sample `i`'s road, on its bank.
    pub fn edge(&self, i: usize, lat: f32) -> Vec3 {
        self.pts[i] + self.right[i] * lat + Vec3::Y * lat * self.bank[i]
    }

    /// The grass of an infield, as the quads it is laid in: each runs from one side of
    /// the corner to the other, between the places the barrier would have stood.
    pub fn grass(&self, infield: &Infield) -> Vec<[Vec3; 4]> {
        let edge = |i: usize| self.edge(i, infield.side * WALL);
        (0..(infield.to - infield.from) / 2)
            .map(|k| (infield.from + k, infield.to - k))
            .map(|(near, far)| [edge(near), edge(near + 1), edge(far - 1), edge(far)])
            .collect()
    }

    /// One of the built-in circuits.
    pub fn built(layout: Layout) -> Self {
        let mut track = Track::shaped(layout, crate::scenery::mirror());
        let (n, extras) = (track.n(), layout.extras());
        // A byway runs straight from one place on the line to another, at an even
        // slope but for its bumps.
        track.branches = extras
            .byways
            .iter()
            .map(|byway| {
                let ends = [track.index(byway.from), track.index(byway.to)];
                let [from, to] = ends.map(|i| track.pts[i]);
                let length = xz_dist2(from, to).sqrt();
                let along = (to - from).with_y(0.0) / length;
                let count = (length / 2.0).round();
                let bumps = ((length - 2.0 * BUMP_MARGIN) / BUMP_LENGTH)
                    .floor()
                    .max(0.0);
                let first = (length - bumps * BUMP_LENGTH) / 2.0;
                let height = |at: f32| {
                    let over = at - first;
                    let bump = if over > 0.0 && over < bumps * BUMP_LENGTH {
                        byway.bumps * 0.5 * (1.0 - (TAU * over / BUMP_LENGTH).cos())
                    } else {
                        0.0
                    };
                    from.y + (to.y - from.y) * at / length + bump
                };
                Branch {
                    pts: (0..=count as usize)
                        .map(|k| length * k as f32 / count)
                        .map(|at| (from + along * at).with_y(height(at)))
                        .collect(),
                    along,
                    right: Vec3::new(-along.z, 0.0, along.x),
                    road: byway.road,
                    wall: byway.wall,
                    covered: byway.covered,
                    ends,
                    taken: byway.taken,
                }
            })
            .collect();
        // The grass of an infield is on the inside of its corner.
        track.infields = extras
            .infields
            .iter()
            .map(|&(from, to)| (track.index(from), track.index(to)))
            .map(|(from, to)| {
                let turn: f32 = (from..to)
                    .map(|i| track.flat[i].cross(track.flat[i + 1]).y)
                    .sum();
                Infield {
                    from,
                    to,
                    side: -turn.signum(),
                }
            })
            .collect();
        track.pads = extras
            .pads
            .iter()
            .map(|&(s, lat)| Pad {
                at: track.index(s),
                lat,
            })
            .collect();
        // Scattered scenery keeps off all of them, and off what stands round the
        // helter skelter.
        if layout == Layout::HelterSkelter {
            track.clearings.extend(crate::helter::clearings());
        }
        for branch in &track.branches {
            for p in branch.pts.iter().step_by(4) {
                track.clearings.push((*p, branch.wall + 7.0));
            }
        }
        for infield in &track.infields {
            for [a, _, _, d] in track.grass(infield) {
                track
                    .clearings
                    .push(((a + d) / 2.0, a.distance(d) / 2.0 + 4.0));
            }
        }

        let (branches, infields) = (track.branches.clone(), track.infields.clone());
        let lawns: Vec<[Vec3; 4]> = infields
            .iter()
            .flat_map(|infield| track.grass(infield))
            .collect();
        let mut quads: Vec<([Vec3; 4], Surface)> = Vec::new();
        // Tarmac, verges and the inner faces of the barriers.
        let grass = Surface {
            rolling_resistance: 20.0,
            ..default()
        };
        for i in 0..n {
            let j = (i + 1) % n;
            let mut strip = |from: f32, to: f32, surface: Surface| {
                let corners = [
                    track.edge(i, from),
                    track.edge(i, to),
                    track.edge(j, to),
                    track.edge(j, from),
                ];
                quads.push((corners, surface));
            };
            strip(-ROAD_HW - KERB, ROAD_HW + KERB, Surface::default());
            strip(-WALL, -ROAD_HW - KERB, grass);
            strip(ROAD_HW + KERB, WALL, grass);
            // Beside a jump they are as high as a car in the air.
            let jump = track.jumps.iter().any(|jump| (jump.0..jump.1).contains(&i));
            let top = Vec3::Y * if jump { JUMP_LIPS.0 + 6.0 } else { 3.0 };
            for side in [-1.0, 1.0] {
                let (a, b) = (track.edge(i, side * WALL), track.edge(j, side * WALL));
                // There is none across the end of a byway, or before an infield.
                if branches.iter().any(|branch| branch.holds((a + b) / 2.0))
                    || infields.iter().any(|infield| infield.opens(i, side))
                {
                    continue;
                }
                quads.push((
                    [a - Vec3::Y, b - Vec3::Y, b + top, a + top],
                    Surface::default(),
                ));
            }
        }
        // The grass inside a corner, which is slower going than the verge.
        let lawn = Surface {
            rolling_resistance: LAWN_DRAG,
            ..default()
        };
        quads.extend(lawns.into_iter().map(|corners| (corners, lawn)));
        // A byway's tarmac from barrier to barrier, and its barriers where they are
        // clear of the road.
        for branch in &branches {
            let across = branch.right * branch.wall;
            let lift = Vec3::Y * BYWAY_LIFT;
            for step in branch.pts.windows(2) {
                let (p, q) = (step[0] + lift, step[1] + lift);
                quads.push((
                    [p - across, p + across, q + across, q - across],
                    Surface::default(),
                ));
                for side in [-1.0, 1.0] {
                    let (a, b) = (p + across * side, q + across * side);
                    if !track.on_road((a + b) / 2.0, WALL) {
                        let (low, high) = (Vec3::Y, Vec3::Y * 3.0);
                        quads.push(([a - low, b - low, b + high, a + high], Surface::default()));
                    }
                }
            }
        }
        for ([a, b, c, d], surface) in quads {
            track.collision.add([a, b, c], surface);
            track.collision.add([a, c, d], surface);
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
        // The lap zones are on road that no byway goes round and no infield is beside.
        let stretches: Vec<(usize, usize)> = branches
            .iter()
            .map(|branch| (branch.ends[0], branch.ends[1]))
            .chain(infields.iter().map(|infield| (infield.from, infield.to)))
            .collect();
        for (zone, mut at) in [(2, n / 3), (0, 2 * n / 3)] {
            while let Some(&(_, to)) = stretches.iter().find(|&&(from, to)| {
                (from.saturating_sub(ZONE_CLEAR)..to + ZONE_CLEAR).contains(&at)
            }) {
                at = to + ZONE_CLEAR;
            }
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
            jumps: Vec::new(),
            branches: Vec::new(),
            unrouted: false,
            bank: vec![0.0; n],
            infields: Vec::new(),
            pads: Vec::new(),
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
        // A tunnel is entered by what was its way out, and a jump taken from its landing.
        for stretch in self.tunnels.iter_mut().chain(&mut self.jumps) {
            *stretch = (n + 1 - stretch.1, n + 1 - stretch.0);
        }
        // A byway is driven from what was its far end, a bank leans the other way from
        // the new right, and an infield and a pad are on the other side.
        for branch in &mut self.branches {
            branch.pts.reverse();
            (branch.along, branch.right) = (-branch.along, -branch.right);
            branch.ends = [(n - branch.ends[1]) % n, (n - branch.ends[0]) % n];
        }
        self.bank[1..].reverse();
        for bank in &mut self.bank {
            *bank = -*bank;
        }
        for infield in &mut self.infields {
            *infield = Infield {
                from: n - infield.to,
                to: n - infield.from,
                side: -infield.side,
            };
        }
        for pad in &mut self.pads {
            *pad = Pad {
                at: (n - pad.at) % n,
                lat: -pad.lat,
            };
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
        let x = s.rem_euclid(self.length) / self.spacing;
        let (i, j) = (x as usize % self.n(), (x as usize + 1) % self.n());
        let bank = self.bank[i] + (self.bank[j] - self.bank[i]) * x.fract();
        p + r * lat + Vec3::Y * lat * bank
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

    /// Whether a place is on the road proper: within `reach` of the racing line to
    /// either side, on its level.
    pub fn on_road(&self, p: Vec3, reach: f32) -> bool {
        let i = self.nearest(p);
        let from = p - self.pts[i];
        from.dot(self.right[i]).abs() < reach
            && from.dot(self.flat[i]).abs() < 2.0 * self.spacing
            && from.y.abs() < 3.0
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
            let at = |lat: f32| (self.edge(i, lat), self.edge(j, lat));
            let strip = |b: &mut BrickMesh, from: f32, to: f32, lift: f32, c: Color| {
                let ((a0, a1), (b0, b1)) = (at(from), at(to));
                let l = up * lift;
                b.quad(a0 + l, b0 + l, b1 + l, a1 + l, c);
            };

            // A jump is striped from end to end.
            let surface = if self.jumps.iter().any(|jump| (jump.0..jump.1).contains(&i)) {
                [YELLOW, BLACK][i % 2]
            } else {
                road[(i / 2) % 2]
            };
            strip(&mut b, -ROAD_HW, ROAD_HW, 0.0, surface);
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
                // There is none across the end of a byway, or before an infield.
                if self.branches.iter().any(|branch| branch.holds(mid))
                    || self.infields.iter().any(|infield| infield.opens(i, side))
                {
                    continue;
                }
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

            if span(i) && i % 8 == 0 {
                // Where the road under the bridge goes the same way, as a spiral's
                // does, columns stand on its barriers.
                let under = (0..n).find(|&k| {
                    p0.y - self.pts[k].y > 6.0
                        && xz_dist2(p0, self.pts[k]) < 9.0
                        && self.flat[k].dot(self.flat[i]).abs() > 0.95
                });
                if let Some(under) = under {
                    let foot = self.pts[under].y + 1.05;
                    for side in [-1.0, 1.0] {
                        let base = (p0 + r0 * side * (WALL + 0.4)).with_y(foot);
                        b.cyl(base, 0.4, p0.y - 0.85 - foot, Quat::IDENTITY, WHITE);
                    }
                }
            }
            if span(i) {
                // A bridge: a deck under the road, banked as it is, and nothing under that.
                let lean = (up - (r0 + r1) / 2.0 * self.bank[i]).normalize();
                let rot = Transform::IDENTITY.looking_to(p1 - p0, lean).rotation;
                let half = Vec3::new(WALL + 0.8, 0.4, p0.distance(p1) / 2.0 + 0.05);
                b.cuboid((p0 + p1) / 2.0 - lean * 0.45, half, rot, GREY);
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

        // The grass inside the corners that have it, a shade off the verge's.
        let lawn = Color::srgb(0.24, 0.56, 0.26);
        for infield in &self.infields {
            for [a, c, d, e] in self.grass(infield) {
                let lift = up * 0.002;
                let corners = [a + lift, c + lift, d + lift, e + lift];
                // Seen from above, whichever way round the corner goes.
                if (c - a).cross(d - a).y > 0.0 {
                    b.quad(corners[0], corners[1], corners[2], corners[3], lawn);
                } else {
                    b.quad(corners[3], corners[2], corners[1], corners[0], lawn);
                }
            }
        }
        // The speed pads: a bright patch with arrows up it, the way the race is run.
        for pad in &self.pads {
            let s = pad.at as f32 * self.spacing;
            let place = |along: f32, across: f32, lift: f32| {
                self.point(s + along, pad.lat + across) + up * lift
            };
            let (long, wide) = (PAD_LENGTH / 2.0, PAD_WIDTH / 2.0);
            b.quad(
                place(-long, -wide, 0.02),
                place(-long, wide, 0.02),
                place(long, wide, 0.02),
                place(long, -wide, 0.02),
                ORANGE,
            );
            for arrow in 0..3 {
                let back = -long + 0.6 + arrow as f32 * 2.4;
                b.tri(
                    place(back, -wide + 0.8, 0.035),
                    place(back, wide - 0.8, 0.035),
                    place(back + 2.0, 0.0, 0.035),
                    YELLOW,
                );
            }
        }
        // The byways: dirt between barriers of their own, where they are clear of the
        // road, with banks under their bumps.
        let dirt = [Color::srgb(0.58, 0.44, 0.28), Color::srgb(0.63, 0.49, 0.32)];
        for branch in &self.branches {
            for (k, step) in branch.pts.windows(2).enumerate() {
                let (p, q) = (step[0], step[1]);
                let (mid, rot) = (
                    (p + q) / 2.0,
                    Transform::IDENTITY.looking_to(q - p, up).rotation,
                );
                let half = p.distance(q) / 2.0 + 0.05;
                let strip = |b: &mut BrickMesh, from: f32, to: f32, lift: f32, colour: Color| {
                    let (a, c, l) = (branch.right * from, branch.right * to, up * lift);
                    b.quad(p + a + l, p + c + l, q + c + l, q + a + l, colour);
                };
                // The dirt is laid in strips, each only where the road's tarmac isn't.
                const STRIPS: usize = 6;
                let wide = 2.0 * branch.road / STRIPS as f32;
                for lane in 0..STRIPS {
                    let from = -branch.road + wide * lane as f32;
                    if !self.on_road(mid + branch.right * (from + wide / 2.0), ROAD_HW + KERB) {
                        strip(&mut b, from, from + wide, 0.012, dirt[(k / 2) % 2]);
                    }
                }
                let roofed = branch.roofed(branch.place(p).0);
                let portal = roofed != branch.roofed(branch.place(p).0 - 4.0)
                    || roofed != branch.roofed(branch.place(q).0 + 4.0);
                for side in [-1.0, 1.0] {
                    if self.on_road(mid + branch.right * side * branch.wall, WALL) {
                        continue;
                    }
                    let (near, far) = (side * branch.road, side * branch.wall);
                    strip(&mut b, near.min(far), near.max(far), 0.004, LIME);
                    if roofed {
                        // The shed's wall.
                        let out = branch.right * side * (branch.wall + SHED_THICK / 2.0);
                        let colour = if portal { YELLOW } else { RED };
                        b.cuboid(
                            mid + out + up * (SHED_HEIGHT / 2.0),
                            Vec3::new(SHED_THICK / 2.0, SHED_HEIGHT / 2.0, half),
                            rot,
                            colour,
                        );
                        continue;
                    }
                    let barrier = if (k / 3) % 2 == 0 { WHITE } else { RED };
                    let at = mid + branch.right * side * (branch.wall + 0.4);
                    b.cuboid(at + up * 0.45, Vec3::new(0.4, 0.45, half), rot, barrier);
                    b.cyl(at + up * 0.9, 0.25, 0.15, rot, barrier);
                    if p.y > 0.05 || q.y > 0.05 {
                        let out = branch.right * side * (branch.wall + 0.8);
                        let (o0, o1) = (p + out, q + out);
                        let (g0, g1) = (o0.with_y(-0.1), o1.with_y(-0.1));
                        b.quad(o0, o1, g1, g0, TAN);
                        b.quad(o1, o0, g0, g1, TAN);
                    }
                }
                if roofed {
                    let colour = if portal { YELLOW } else { DARK_GREY };
                    b.brick(
                        mid + up * (SHED_HEIGHT + SHED_THICK / 2.0),
                        Vec3::new(branch.wall + SHED_THICK, SHED_THICK / 2.0, half),
                        rot,
                        colour,
                        (4, 1),
                    );
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
        if layout == Layout::HelterSkelter {
            crate::helter::scenery(&mut b);
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
            let (shortest, longest) = match layout {
                Layout::Gauntlet => (900.0, 3500.0),
                Layout::HelterSkelter => (4100.0, 4400.0),
                _ => (900.0, 1400.0),
            };
            assert!(
                t.length > shortest && t.length < longest,
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
    fn the_helter_skelter_ends_where_it_begins() {
        // Drawn on past its last leg, it is back at the origin and going along x.
        let mut twice: Vec<Leg> = Vec::new();
        for leg in HELTER.iter().chain(&HELTER[..1]) {
            twice.push(match *leg {
                Straight(length, rise) => Straight(length, rise),
                Turn(degrees, radius, rise) => Turn(degrees, radius, rise),
            });
        }
        let (once, twice) = (drawn(HELTER), drawn(&twice));
        let (end, next) = (
            Vec3::from(twice[once.len()]),
            Vec3::from(twice[once.len() + 1]),
        );
        assert!(end.length() < 0.1, "{end}");
        assert!((next - end).normalize().dot(Vec3::X) > 0.9999);
        // Its points are near enough evenly spaced.
        for (a, b) in once.iter().zip(&once[1..]) {
            let apart = xz_dist2(Vec3::from(*a), Vec3::from(*b)).sqrt();
            assert!(apart > 5.0 && apart < DRAWN_STEP + 0.1, "{a:?} {b:?}");
        }
    }

    #[test]
    fn the_helter_skelter_has_a_spiral_two_tunnels_and_a_jump() {
        let t = Track::built(Layout::HelterSkelter);
        let n = t.n();
        // A whole turn of the road has another over it with room to drive under, and
        // where the spiral begins and ends the road is three deep.
        let over = |i: usize| {
            let mut above: Vec<f32> = (0..n)
                .filter(|&k| k.abs_diff(i) > 30 && t.pts[k].y > t.pts[i].y + 1.0)
                .filter(|&k| xz_dist2(t.pts[i], t.pts[k]) < 6.0)
                .map(|k| t.pts[k].y - t.pts[i].y)
                .collect();
            above.sort_by(f32::total_cmp);
            above.dedup_by(|a, b| (*a - *b).abs() < 1.0);
            above
        };
        let stacked: Vec<usize> = (0..n).filter(|&i| !over(i).is_empty()).collect();
        assert!(
            stacked.len() as f32 * t.spacing > 250.0,
            "{}",
            stacked.len()
        );
        assert!(stacked.iter().any(|&i| over(i).len() == 2));
        assert!(SPIRAL_RISE > TUNNEL_HEIGHT);
        for &i in &stacked {
            for (level, rise) in over(i).into_iter().enumerate() {
                assert!(
                    (rise - SPIRAL_RISE * (level + 1) as f32).abs() < 2.5,
                    "{i} {rise}"
                );
            }
        }
        // Each level of it is the road found from that level.
        for &i in &stacked {
            let s = i as f32 * t.spacing;
            assert!((t.surface_point(s, 0.0).y - t.pts[i].y).abs() < 0.3, "{i}");
            assert_eq!(t.nearest(t.pts[i]), i);
        }
        // The tunnels are on level road with nothing over it.
        assert_eq!(t.tunnels.len(), 2);
        let bridged = t.bridged();
        for &(from, to) in &t.tunnels {
            assert!((to - from) as f32 * t.spacing > 100.0, "{from}..{to}");
            assert!((from..to).all(|i| t.pts[i].y < 0.1 && !bridged[i]));
        }
        // The jump is cut into a straight, level but for itself, with a run up to it
        // and no gate across it for a car in the air to miss.
        let &[(from, to)] = &t.jumps[..] else {
            panic!("{:?}", t.jumps);
        };
        let lip = (from..to)
            .max_by(|&a, &b| t.pts[a].y.total_cmp(&t.pts[b].y))
            .unwrap();
        assert_eq!(t.pts[lip].y, JUMP_LIPS.0);
        assert!(t.pts[lip + 3].y < 0.01 && t.pts[lip + 5].y < 0.01);
        assert!(t.pts[from].y < 0.01 && t.pts[to - 1].y < 0.3 && t.pts[to].y < 0.01);
        assert!(
            (from - 50..to + 10)
                .all(|i| t.curv[i] < 1e-3 && t.tunnels.iter().all(|t| !(t.0..t.1).contains(&i)))
        );
        assert!((from - 50..from).all(|i| t.pts[i].y < 0.1));
        for gate in &t.course.checkpoints {
            assert!(!(from..to).contains(&t.nearest(gate.position)));
        }
        // No side of the dip is too steep to be driven up.
        for i in from..to {
            let step = t.pts[i + 1] - t.pts[i];
            assert!(step.y.abs() < step.with_y(0.0).length() * 1.1, "{i}");
        }
        // Turned round, the same road is the jump.
        let mut back = Track::built(Layout::HelterSkelter);
        back.reverse();
        let (back_from, back_to) = back.jumps[0];
        assert_eq!(
            (back.pts[back_from], back.pts[back_to - 1]),
            (t.pts[to - 1], t.pts[from])
        );
    }

    #[test]
    fn byways_leave_the_road_and_come_back_to_it() {
        for layout in Layout::ALL {
            let t = Track::built(layout);
            let (n, lift) = (t.n(), Vec3::Y * 0.6);
            let byways = if layout == Layout::HelterSkelter {
                2
            } else {
                1
            };
            assert_eq!(t.branches.len(), byways, "{layout:?}");
            for branch in &t.branches {
                // Each runs between two places on the line, and is the shorter way.
                let (first, last) = (branch.pts[0], branch.pts[branch.pts.len() - 1]);
                let [from, to] = branch.ends;
                assert_eq!([t.nearest(first), t.nearest(last)], [from, to]);
                assert!(t.pts[from].distance(first) < 0.01 && t.pts[to].distance(last) < 0.01);
                let round = (to - from) as f32 * t.spacing;
                assert!(
                    to > from && branch.length() < round - 10.0,
                    "{layout:?} {round}"
                );
                // Nothing stands in it from end to end, there is ground under all of
                // it, and it has a barrier either side where it is clear of the road.
                for step in branch.pts.windows(2) {
                    assert!(t.collision.wall(step[0] + lift, step[1] + lift).is_none());
                    let ground = t.collision.ground(step[0] + Vec3::Y * 2.0, 4.0).unwrap();
                    let over = ground.point.y - step[0].y;
                    assert!((-0.02..1.5).contains(&over), "{layout:?} {over}");
                }
                let middle = branch.pts[branch.pts.len() / 2];
                for side in [-1.0, 1.0] {
                    let out = middle + branch.right * side * (branch.wall + 1.0);
                    assert!(t.collision.wall(middle + lift, out + lift).is_some());
                }
                // It keeps clear of the rest of the road, tunnels and jumps too.
                assert!(!t.on_road(middle, WALL + branch.wall), "{layout:?}");
                for stretch in t.tunnels.iter().chain(&t.jumps) {
                    assert!(!(stretch.0..stretch.1).contains(&from));
                    assert!(!(stretch.0..stretch.1).contains(&to));
                }
            }
            // The road's own barriers stand everywhere but across the byways' ends
            // and before the infields.
            let mut open = 0;
            for i in 0..n {
                for side in [-1.0, 1.0] {
                    let (mid, out) = (t.edge(i, 0.0), t.edge(i, side * (WALL + 1.0)));
                    if t.collision.wall(mid + Vec3::Y, out + Vec3::Y).is_none() {
                        open += 1;
                        let gap = t.edge(i, side * WALL);
                        assert!(
                            t.branches.iter().any(|b| b.holds(gap))
                                || t.infields.iter().any(|f| f.opens(i, side)),
                            "{layout:?} {i} {side}"
                        );
                    }
                }
            }
            assert!(open > 8 && open < 200, "{layout:?} {open}");
            // Neither lap zone is on road that can be gone round, or near its ends.
            let stretches = t
                .branches
                .iter()
                .map(|b| (b.ends[0], b.ends[1]))
                .chain(t.infields.iter().map(|f| (f.from, f.to)));
            for (from, to) in stretches {
                for &(centre, ..) in &t.course.zones {
                    let at = t.nearest(centre);
                    assert!(at + 10 < from || at > to + 10, "{layout:?} zone at {at}");
                }
            }
            // Turned round, each byway is the same road from its other end.
            let mut back = Track::built(layout);
            back.reverse();
            for (branch, turned) in t.branches.iter().zip(&back.branches) {
                assert_eq!(turned.pts[0], branch.pts[branch.pts.len() - 1]);
                assert_eq!(back.nearest(turned.pts[0]), turned.ends[0]);
                assert!(turned.ends[0] < turned.ends[1]);
                assert!(turned.along.dot(branch.along) < -0.999);
            }
        }
    }

    #[test]
    fn infields_pads_and_banks_are_where_the_road_can_take_them() {
        for layout in Layout::ALL {
            let t = Track::built(layout);
            let (n, lift) = (t.n(), Vec3::Y * 0.6);
            let mut back = Track::built(layout);
            back.reverse();
            assert!(!t.infields.is_empty() && t.pads.len() >= 2, "{layout:?}");
            for (infield, turned) in t.infields.iter().zip(&back.infields) {
                // The grass is slow going, inside the corner, clear of the road, and
                // nothing stands between it and the road.
                let grass = t.grass(infield);
                let middle = (infield.from + infield.to) / 2;
                for &[a, _, _, d] in &grass {
                    let on = (a + d) / 2.0;
                    let ground = t.collision.ground(on + Vec3::Y * 2.0, 4.0).unwrap();
                    assert!(ground.surface.rolling_resistance >= LAWN_DRAG, "{layout:?}");
                    assert!(
                        a.distance(d) < 2.0 || !t.on_road(on, WALL - 1.0),
                        "{layout:?}"
                    );
                    assert!(t.collision.wall(t.pts[middle] + lift, on + lift).is_none());
                }
                let inside = t.edge(middle, infield.side * (WALL + 2.0));
                let outside = t.edge(middle, -infield.side * (WALL + 2.0));
                assert!(
                    t.collision
                        .wall(t.pts[middle] + lift, inside + lift)
                        .is_none()
                );
                assert!(
                    t.collision
                        .wall(t.pts[middle] + lift, outside + lift)
                        .is_some()
                );
                // Turned round it is the same grass.
                assert_eq!(
                    back.edge(turned.from, turned.side * WALL),
                    t.edge(infield.to, infield.side * WALL)
                );
                for stretch in t.tunnels.iter().chain(&t.jumps) {
                    assert!(stretch.1 < infield.from || stretch.0 > infield.to);
                }
            }
            // A pad lies on the road, the same place either way round.
            for (pad, turned) in t.pads.iter().zip(&back.pads) {
                assert!(pad.lat.abs() + PAD_WIDTH / 2.0 <= ROAD_HW);
                let (here, there) = (t.edge(pad.at, pad.lat), back.edge(turned.at, turned.lat));
                assert!(here.distance(there) < 0.01);
            }
            // A bank's high side is the outside of its corner, and a car's wheels
            // find it.
            let banked: Vec<usize> = (0..n).filter(|&i| t.bank[i] != 0.0).collect();
            assert!(banked.len() > 30, "{layout:?}");
            for &i in &banked {
                let turn = t.flat[i].cross(t.flat[(i + 1) % n]).y;
                assert!(turn * t.bank[i] >= 0.0, "{layout:?} {i}");
                let high = t.bank[i].signum() * (ROAD_HW - 1.0);
                let s = i as f32 * t.spacing;
                let (up, down) = (t.surface_point(s, high), t.surface_point(s, -high));
                assert!((up.y - t.edge(i, high).y).abs() < 0.1, "{layout:?} {i}");
                assert!(up.y >= down.y && back.bank[n - i] == -t.bank[i]);
            }
        }
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
