//! The arenas: places to fight in and not to race round, for `net::modes`' battle and
//! game of tag. The port's own.
//!
//! An arena is a walled floor with no road: a car goes where it likes. Each is laid
//! out as a place to fight over is: a few ways through it that a car must choose
//! between, with buildings between them to break the sight of one from another
//! and gaps to cross by; high ground in the middle of it, reached by ramps, that
//! looks over the ways below and has the best bricks; a way under that for whoever
//! would rather not be seen; and places at its ends for the cars to start from,
//! out of sight of each other. It is built of blocks, which stand on the floor,
//! ramps, and decks, which are held up over it and can be driven under. Each still has a line (`track::Layout::control`), a loop through
//! the open floor that nothing is made to follow: the bricks are laid along it, the
//! computer's cars wander it when they have nobody to go after (`kart::Ai::quarry`),
//! and the hazards are stood by it (`gauntlet::load`). The cars start spread round
//! the floor, facing its middle. There are no gates, lap zones or finishing line, so
//! nothing counts a lap or says a car is going the wrong way.
//!
//! All four are the same either side of the line east to west, so that mirrored
//! they are themselves.

use crate::assets::materials::Surface;
use crate::collision::Collision;
use crate::meshgen::*;
use bevy::prelude::*;
use std::f32::consts::TAU;

/// How high an arena's wall is: more than a car blown up rises.
const WALL_HEIGHT: f32 = 7.0;
const WALL_THICK: f32 = 1.2;
/// How long each brick of the wall is, or thereabouts.
const WALL_BRICK: f32 = 8.0;
/// How wide the road an arena's line is taken to have: what the bricks' rows and the
/// computer's wandering are spread over.
pub const LINE_ROAD: f32 = 5.0;

/// Something solid standing on the floor: where its middle is (x, z), how far it
/// reaches either way, and how high it is.
pub struct Block {
    pub at: [f32; 2],
    pub half: [f32; 2],
    pub height: f32,
    pub colour: Color,
}

/// A slope from the floor up to `height`: where its middle is, how far it reaches
/// either way, and which way along the ground it climbs.
pub struct Ramp {
    pub at: [f32; 2],
    pub half: [f32; 2],
    pub height: f32,
    pub climbs: [f32; 2],
}

/// A floor held up at `height`, to be driven on and under.
pub struct Deck {
    pub at: [f32; 2],
    pub half: [f32; 2],
    pub height: f32,
}

/// How thick a deck is.
const DECK_THICK: f32 = 1.0;

pub struct Plan {
    /// The corners of the floor in order, as (x, z). Every one can be seen from the
    /// middle, which is (0, 0).
    pub outline: &'static [[f32; 2]],
    pub blocks: &'static [Block],
    pub ramps: &'static [Ramp],
    pub decks: &'static [Deck],
    /// The line's control points, as `track`'s circuits have theirs: (x, height, z).
    pub line: &'static [[f32; 3]],
    /// Where the six cars start, as (x, z); each faces the middle.
    pub grid: [[f32; 2]; 6],
    /// Where bricks lie besides those along the line, as (x, z): in the corners
    /// the line doesn't go to, to be gone out of the way for.
    pub bricks: &'static [[f32; 2]],
    /// The floor's two colours, which alternate outwards from the middle.
    pub floor: [Color; 2],
}

const fn block(x: f32, z: f32, half_x: f32, half_z: f32, height: f32, colour: Color) -> Block {
    Block {
        at: [x, z],
        half: [half_x, half_z],
        height,
        colour,
    }
}

const fn ramp(x: f32, z: f32, half_x: f32, half_z: f32, height: f32, climbs: [f32; 2]) -> Ramp {
    Ramp {
        at: [x, z],
        half: [half_x, half_z],
        height,
        climbs,
    }
}

const fn deck(x: f32, z: f32, half_x: f32, half_z: f32, height: f32) -> Deck {
    Deck {
        at: [x, z],
        half: [half_x, half_z],
        height,
    }
}

/// How high the arenas' high ground is: a car under it is clear of it.
const HIGH: f32 = 6.0;

/// The yard: three ways from one end to the other. The middle one is the short one
/// and goes under a bridge; the two outside it are longer, behind buildings, and
/// each has a ramp up onto that bridge, which crosses the middle way and looks
/// along it both ways. The buildings' roofs are as high as the bridge and joined to
/// it by catwalks, with a ramp up to each from its end of the yard, so that there
/// is a way round up there too, over the gaps the ways below are crossed by, and
/// off any edge of it onto whoever is underneath. The cars start in the open ends,
/// three at each. A ghost keeps the way under the bridge (`gauntlet::load`).
pub const YARD: Plan = Plan {
    outline: &[[130.0, -85.0], [130.0, 85.0], [-130.0, 85.0], [-130.0, -85.0]],
    blocks: &[
        // The bridge's two piers, either side of the way under it.
        block(0.0, -22.0, 12.0, 8.0, HIGH, YELLOW),
        block(0.0, 22.0, 12.0, 8.0, HIGH, YELLOW),
        // The buildings between the middle way and the outside ones.
        block(-62.0, -30.0, 22.0, 8.0, HIGH, RED),
        block(62.0, -30.0, 22.0, 8.0, HIGH, BLUE),
        block(-62.0, 30.0, 22.0, 8.0, HIGH, RED),
        block(62.0, 30.0, 22.0, 8.0, HIGH, BLUE),
        // A chimney on each roof, and a stack of crates in each outside way.
        block(-76.0, -34.0, 3.0, 3.0, 10.0, DARK_GREY),
        block(76.0, -34.0, 3.0, 3.0, 10.0, DARK_GREY),
        block(-76.0, 34.0, 3.0, 3.0, 10.0, DARK_GREY),
        block(76.0, 34.0, 3.0, 3.0, 10.0, DARK_GREY),
        block(-30.0, -48.0, 3.0, 3.0, 4.0, BROWN),
        block(30.0, -48.0, 3.0, 3.0, 4.0, BROWN),
        block(-30.0, 48.0, 3.0, 3.0, 4.0, BROWN),
        block(30.0, 48.0, 3.0, 3.0, 4.0, BROWN),
        // Something to be behind in the middle way, and on the outside ones.
        block(-45.0, 0.0, 4.0, 6.0, 4.0, WHITE),
        block(45.0, 0.0, 4.0, 6.0, 4.0, WHITE),
        block(-42.0, -79.0, 4.0, 4.0, 4.0, WHITE),
        block(42.0, -79.0, 4.0, 4.0, 4.0, WHITE),
        block(-42.0, 79.0, 4.0, 4.0, 4.0, WHITE),
        block(42.0, 79.0, 4.0, 4.0, 4.0, WHITE),
        // And between the ends and the middle way, so that a car starting isn't
        // looking straight down it.
        block(-96.0, 0.0, 3.0, 9.0, 5.0, RED),
        block(96.0, 0.0, 3.0, 9.0, 5.0, BLUE),
    ],
    ramps: &[
        ramp(0.0, -42.0, 12.0, 12.0, HIGH, [0.0, 1.0]),
        ramp(0.0, 42.0, 12.0, 12.0, HIGH, [0.0, -1.0]),
        // Up onto each roof from its end of the yard.
        ramp(-96.0, -30.0, 12.0, 8.0, HIGH, [1.0, 0.0]),
        ramp(96.0, -30.0, 12.0, 8.0, HIGH, [-1.0, 0.0]),
        ramp(-96.0, 30.0, 12.0, 8.0, HIGH, [1.0, 0.0]),
        ramp(96.0, 30.0, 12.0, 8.0, HIGH, [-1.0, 0.0]),
    ],
    decks: &[
        deck(0.0, 0.0, 12.0, 14.0, HIGH),
        // The catwalks from the bridge's piers to the roofs.
        deck(-26.0, -26.0, 14.0, 4.0, HIGH),
        deck(26.0, -26.0, 14.0, 4.0, HIGH),
        deck(-26.0, 26.0, 14.0, 4.0, HIGH),
        deck(26.0, 26.0, 14.0, 4.0, HIGH),
    ],
    // Along the north way, round the east end, back along the south way and home
    // over the bridge.
    line: &[
        [16.0, 0.0, -68.0],
        [60.0, 0.0, -64.0],
        [100.0, 0.0, -56.0],
        [114.0, 0.0, -28.0],
        [116.0, 0.0, 0.0],
        [114.0, 0.0, 28.0],
        [100.0, 0.0, 56.0],
        [60.0, 0.0, 64.0],
        [20.0, 0.0, 66.0],
        [5.0, 0.0, 61.0],
        [0.0, 0.5, 52.0],
        [0.0, 3.0, 42.0],
        [0.0, 5.5, 32.0],
        [0.0, 6.0, 20.0],
        [0.0, 6.0, 0.0],
        [0.0, 6.0, -20.0],
        [0.0, 5.5, -32.0],
        [0.0, 3.0, -42.0],
        [0.0, 0.5, -52.0],
        [4.0, 0.0, -62.0],
    ],
    grid: [
        [-114.0, -52.0],
        [-116.0, 0.0],
        [-114.0, 52.0],
        [114.0, 52.0],
        [116.0, 0.0],
        [114.0, -52.0],
    ],
    bricks: &[
        [-7.0, 0.0],
        [7.0, 0.0],
        [-70.0, 0.0],
        [-22.0, 0.0],
        [24.0, 0.0],
        [70.0, 0.0],
        [-70.0, -62.0],
        [-70.0, 62.0],
        [-26.0, -36.0],
        [-26.0, 36.0],
        [26.0, -36.0],
        [26.0, 36.0],
        [-56.0, -28.0],
        [-56.0, 28.0],
        [56.0, -28.0],
        [56.0, 28.0],
    ],
    floor: [TAN, GREY],
};

/// The crossroads: four arms off a junction, and over the junction a flyover from
/// the north arm to the south, climbed by a ramp in each, with a deck across it
/// from east to west climbed by a ramp from either side: a crossroads over the
/// crossroads. East and west go under it, either side of those ramps. The east and west arms are each shut twice by walls that leave a way
/// round, with a pocket at the end where the best bricks are: two tight turns in
/// and the same two out, so a car there is cornered. Cannons fire into the west
/// pocket at a car on its way in, and cannonballs rain on the east one.
pub const CROSSROADS: Plan = Plan {
    outline: &[
        [38.0, -135.0],
        [38.0, -38.0],
        [135.0, -38.0],
        [135.0, 38.0],
        [38.0, 38.0],
        [38.0, 135.0],
        [-38.0, 135.0],
        [-38.0, 38.0],
        [-135.0, 38.0],
        [-135.0, -38.0],
        [-38.0, -38.0],
        [-38.0, -135.0],
    ],
    blocks: &[
        // The flyover's piers, where each arm meets the junction.
        block(0.0, -49.0, 8.0, 11.0, HIGH, YELLOW),
        block(0.0, 49.0, 8.0, 11.0, HIGH, YELLOW),
        // East and west: an island to go either side of, then a gate in the middle.
        block(68.0, 0.0, 3.0, 16.0, 5.0, BLUE),
        block(100.0, -25.0, 3.0, 13.0, 5.0, BLUE),
        block(100.0, 25.0, 3.0, 13.0, 5.0, BLUE),
        block(-68.0, 0.0, 3.0, 16.0, 5.0, RED),
        block(-100.0, -25.0, 3.0, 13.0, 5.0, RED),
        block(-100.0, 25.0, 3.0, 13.0, 5.0, RED),
        // North and south: a wall across the end of each, behind the ramp's foot,
        // and a pillar either side of that.
        block(0.0, -116.0, 14.0, 3.0, 5.0, WHITE),
        block(0.0, 116.0, 14.0, 3.0, 5.0, WHITE),
        block(-24.0, -108.0, 4.0, 4.0, 7.0, YELLOW),
        block(24.0, -108.0, 4.0, 4.0, 7.0, YELLOW),
        block(-24.0, 108.0, 4.0, 4.0, 7.0, YELLOW),
        block(24.0, 108.0, 4.0, 4.0, 7.0, YELLOW),
    ],
    ramps: &[
        ramp(0.0, -80.0, 8.0, 20.0, HIGH, [0.0, 1.0]),
        ramp(0.0, 80.0, 8.0, 20.0, HIGH, [0.0, -1.0]),
        ramp(-40.0, 0.0, 10.0, 6.0, HIGH, [1.0, 0.0]),
        ramp(40.0, 0.0, 10.0, 6.0, HIGH, [-1.0, 0.0]),
    ],
    decks: &[
        deck(0.0, 0.0, 8.0, 38.0, HIGH),
        // The deck across it, either side of the flyover.
        deck(-19.0, 0.0, 11.0, 6.0, HIGH),
        deck(19.0, 0.0, 11.0, 6.0, HIGH),
    ],
    // Round the junction under the flyover, a little way into the east and west
    // arms.
    line: &[
        [54.0, 0.0, -20.0],
        [56.0, 0.0, 0.0],
        [54.0, 0.0, 20.0],
        [30.0, 0.0, 26.0],
        [14.0, 0.0, 29.0],
        [0.0, 0.0, 29.0],
        [-14.0, 0.0, 29.0],
        [-30.0, 0.0, 26.0],
        [-54.0, 0.0, 20.0],
        [-56.0, 0.0, 0.0],
        [-54.0, 0.0, -20.0],
        [-30.0, 0.0, -26.0],
        [-14.0, 0.0, -29.0],
        [0.0, 0.0, -29.0],
        [14.0, 0.0, -29.0],
        [30.0, 0.0, -26.0],
    ],
    grid: [
        [84.0, -26.0],
        [84.0, 26.0],
        [24.0, 86.0],
        [-84.0, 26.0],
        [-84.0, -26.0],
        [24.0, -86.0],
    ],
    bricks: &[
        [0.0, 0.0],
        [120.0, -12.0],
        [120.0, 12.0],
        [-120.0, -12.0],
        [-120.0, 12.0],
        [0.0, -127.0],
        [0.0, 127.0],
        [84.0, 0.0],
        [-84.0, 0.0],
        [-24.0, -86.0],
        [-24.0, 86.0],
    ],
    floor: [GREY, DARK_GREY],
};

/// The town: a short street with a house at each end, facing each other, and a bus
/// and a truck left in the middle of it so that neither house sees the other's
/// door. A footbridge runs down the middle of the street from the one house's
/// upper floor to the other's, over the bus and the truck: the quick way across,
/// in plain sight, with the street to fall into either side. Each house is driven through, front to back, and has an upper floor
/// reached by a ramp up either side from its back yard, open at the front: a car
/// up there looks down the street, and comes off the edge into it. The cars
/// start in the back yards, three behind each house, and a path goes round each
/// side of each house for whoever would rather not go through it. Small enough
/// that nobody is out of it for long. Cannonballs rain between the bus and the
/// truck on a car that goes that way (`gauntlet::load`).
pub const TOWN: Plan = Plan {
    outline: &[[150.0, -75.0], [150.0, 75.0], [-150.0, 75.0], [-150.0, -75.0]],
    blocks: &[
        // The bus and the truck.
        block(-14.0, 0.0, 4.0, 14.0, 5.0, YELLOW),
        block(14.0, 0.0, 4.0, 11.0, 5.0, BLUE),
        // The signs at the sides of the street.
        block(0.0, -60.0, 14.0, 2.0, 4.0, WHITE),
        block(0.0, 60.0, 14.0, 2.0, 4.0, WHITE),
        // The west house: its front and back walls, each with a way through the
        // middle, and its two sides.
        block(-71.0, -19.0, 1.5, 9.0, HIGH, YELLOW),
        block(-71.0, 19.0, 1.5, 9.0, HIGH, YELLOW),
        block(-119.0, -19.0, 1.5, 9.0, HIGH, YELLOW),
        block(-119.0, 19.0, 1.5, 9.0, HIGH, YELLOW),
        block(-95.0, -27.5, 22.5, 1.5, HIGH, YELLOW),
        block(-95.0, 27.5, 22.5, 1.5, HIGH, YELLOW),
        // Its sheds, the fences between its side yards and the street, and the
        // cars parked in front of it.
        block(-100.0, -67.0, 7.0, 5.0, 5.0, BROWN),
        block(-100.0, 67.0, 7.0, 5.0, 5.0, BROWN),
        block(-70.0, -58.0, 1.5, 10.0, 4.0, WHITE),
        block(-70.0, 58.0, 1.5, 10.0, 4.0, WHITE),
        block(-34.0, -52.0, 6.0, 3.0, 3.0, BLUE),
        block(-34.0, 52.0, 6.0, 3.0, 3.0, ORANGE),
        // The east house: its front and back walls, each with a way through the
        // middle, and its two sides.
        block(71.0, -19.0, 1.5, 9.0, HIGH, GREEN),
        block(71.0, 19.0, 1.5, 9.0, HIGH, GREEN),
        block(119.0, -19.0, 1.5, 9.0, HIGH, GREEN),
        block(119.0, 19.0, 1.5, 9.0, HIGH, GREEN),
        block(95.0, -27.5, 22.5, 1.5, HIGH, GREEN),
        block(95.0, 27.5, 22.5, 1.5, HIGH, GREEN),
        // Its sheds, the fences between its side yards and the street, and the
        // cars parked in front of it.
        block(100.0, -67.0, 7.0, 5.0, 5.0, BROWN),
        block(100.0, 67.0, 7.0, 5.0, 5.0, BROWN),
        block(70.0, -58.0, 1.5, 10.0, 4.0, WHITE),
        block(70.0, 58.0, 1.5, 10.0, 4.0, WHITE),
        block(34.0, -52.0, 6.0, 3.0, 3.0, RED),
        block(34.0, 52.0, 6.0, 3.0, 3.0, WHITE),
    ],
    ramps: &[
        ramp(-116.0, -36.0, 12.0, 7.0, HIGH, [1.0, 0.0]),
        ramp(-116.0, 36.0, 12.0, 7.0, HIGH, [1.0, 0.0]),
        ramp(116.0, -36.0, 12.0, 7.0, HIGH, [-1.0, 0.0]),
        ramp(116.0, 36.0, 12.0, 7.0, HIGH, [-1.0, 0.0]),
    ],
    decks: &[
        // Each house's upper floor, and the landing at the head of each ramp.
        deck(-95.0, 0.0, 24.0, 29.0, HIGH),
        deck(-96.0, -36.0, 8.0, 7.0, HIGH),
        deck(-96.0, 36.0, 8.0, 7.0, HIGH),
        deck(95.0, 0.0, 24.0, 29.0, HIGH),
        deck(96.0, -36.0, 8.0, 7.0, HIGH),
        deck(96.0, 36.0, 8.0, 7.0, HIGH),
        // The footbridge between them.
        deck(0.0, 0.0, 71.0, 4.0, HIGH),
    ],
    // Through the west house, up the north side of the street, through the east
    // house, round its back yard and south side, down the south side of the
    // street, and round the west house's south side and back yard.
    line: &[
        [-95.0, 0.0, 0.0],
        [-66.0, 0.0, 0.0],
        [-50.0, 0.0, -10.0],
        [-30.0, 0.0, -26.0],
        [0.0, 0.0, -30.0],
        [30.0, 0.0, -26.0],
        [50.0, 0.0, -10.0],
        [66.0, 0.0, 0.0],
        [95.0, 0.0, 0.0],
        [124.0, 0.0, 0.0],
        [138.0, 0.0, 10.0],
        [141.0, 0.0, 32.0],
        [130.0, 0.0, 51.0],
        [100.0, 0.0, 51.0],
        [70.0, 0.0, 38.0],
        [50.0, 0.0, 34.0],
        [20.0, 0.0, 32.0],
        [0.0, 0.0, 30.0],
        [-20.0, 0.0, 32.0],
        [-50.0, 0.0, 34.0],
        [-70.0, 0.0, 38.0],
        [-100.0, 0.0, 51.0],
        [-130.0, 0.0, 51.0],
        [-141.0, 0.0, 32.0],
        [-138.0, 0.0, 10.0],
        [-124.0, 0.0, 0.0],
    ],
    grid: [
        [-138.0, -48.0],
        [-141.0, 0.0],
        [-138.0, 48.0],
        [138.0, 48.0],
        [141.0, 0.0],
        [138.0, -48.0],
    ],
    bricks: &[
        [0.0, 0.0],
        [-95.0, -16.0],
        [-95.0, 16.0],
        [95.0, -16.0],
        [95.0, 16.0],
        [-76.0, 0.0],
        [76.0, 0.0],
        [-142.0, -68.0],
        [142.0, -68.0],
        [0.0, -68.0],
        [0.0, 68.0],
        [-84.0, -64.0],
        [84.0, -64.0],
    ],
    floor: [GREY, DARK_GREY],
};

/// How far out the ring's wall is, and its two rings of blocks: the pillars, and
/// the buttresses against the wall.
const RING: f32 = 100.0;
const PILLARS: f32 = 68.0;
const BUTTRESSES: f32 = 95.0;
/// A point of a circle about the middle: so many twelfths of the way round, `radius`
/// out.
const fn round(twelfth: usize, radius: f32) -> [f32; 2] {
    // Sines and cosines of twelfths of a turn, which is all that is asked for.
    const UNIT_CIRCLE: [[f32; 2]; 12] = [
        [1.0, 0.0],
        [0.8660254, 0.5],
        [0.5, 0.8660254],
        [0.0, 1.0],
        [-0.5, 0.8660254],
        [-0.8660254, 0.5],
        [-1.0, 0.0],
        [-0.8660254, -0.5],
        [-0.5, -0.8660254],
        [0.0, -1.0],
        [0.5, -0.8660254],
        [0.8660254, -0.5],
    ];
    let [x, z] = UNIT_CIRCLE[twelfth % 12];
    [x * radius, z * radius]
}

const fn pillar(twelfth: usize) -> Block {
    let [x, z] = round(twelfth, PILLARS);
    block(x, z, 4.0, 4.0, 7.0, if twelfth % 4 == 1 { RED } else { WHITE })
}

const fn buttress(twelfth: usize) -> Block {
    let [x, z] = round(twelfth, BUTTRESSES);
    block(x, z, 4.0, 4.0, 6.0, YELLOW)
}

const fn weave(twelfth: usize) -> [f32; 3] {
    // Outside the ring of pillars on the east side and inside it on the west,
    // crossing it between two pillars north and south.
    let radius = match twelfth {
        0 | 1 | 11 => 83.0,
        2 | 10 => 66.0,
        _ => 52.0,
    };
    let [x, z] = round(twelfth, radius);
    [x, 0.0, z]
}

/// The ring: a hill to hold. In the middle of a round floor is a square of high
/// ground with a ramp up each of its four sides and the best bricks on top; round
/// it are a ring of pillars and the wall's buttresses, to come at it from behind,
/// and against the wall north and south a gallery as high as the hill, with a ramp
/// up each end of it, that looks across at whoever holds the hill.
/// On a plinth on the hill the ark turns and fires at whatever it finds, so that
/// nobody holds it for long.
pub const THE_RING: Plan = Plan {
    outline: &RING_OUTLINE,
    blocks: &[
        block(0.0, 0.0, 18.0, 18.0, 5.0, TAN),
        block(0.0, 0.0, 5.0, 5.0, 8.0, YELLOW),
        pillar(1),
        pillar(3),
        pillar(5),
        pillar(7),
        pillar(9),
        pillar(11),
        buttress(0),
        buttress(2),
        buttress(4),
        buttress(6),
        buttress(8),
        buttress(10),
    ],
    ramps: &[
        ramp(30.0, 0.0, 12.0, 8.0, 5.0, [-1.0, 0.0]),
        ramp(-30.0, 0.0, 12.0, 8.0, 5.0, [1.0, 0.0]),
        ramp(0.0, 30.0, 8.0, 12.0, 5.0, [0.0, -1.0]),
        ramp(0.0, -30.0, 8.0, 12.0, 5.0, [0.0, 1.0]),
        // Up each end of each gallery.
        ramp(-30.0, -80.0, 10.0, 5.0, 5.0, [1.0, 0.0]),
        ramp(30.0, -80.0, 10.0, 5.0, 5.0, [-1.0, 0.0]),
        ramp(-30.0, 80.0, 10.0, 5.0, 5.0, [1.0, 0.0]),
        ramp(30.0, 80.0, 10.0, 5.0, 5.0, [-1.0, 0.0]),
    ],
    decks: &[deck(0.0, -80.0, 20.0, 5.0, 5.0), deck(0.0, 80.0, 20.0, 5.0, 5.0)],
    line: &[
        weave(0),
        weave(1),
        weave(2),
        weave(3),
        weave(4),
        weave(5),
        weave(6),
        weave(7),
        weave(8),
        weave(9),
        weave(10),
        weave(11),
    ],
    grid: [
        round(0, 80.0),
        round(2, 80.0),
        round(4, 80.0),
        round(6, 80.0),
        round(8, 80.0),
        round(10, 80.0),
    ],
    bricks: &[
        [12.0, 12.0],
        [12.0, -12.0],
        [-12.0, 12.0],
        [-12.0, -12.0],
        [0.0, 80.0],
        round(5, 84.0),
        round(7, 84.0),
        [0.0, -80.0],
    ],
    floor: [TAN, ORANGE],
};

/// The ring's wall: a corner every fifteen degrees, half way between the twelfths.
const RING_OUTLINE: [[f32; 2]; 24] = {
    // The cosine and sine of fifteen degrees, to turn each twelfth by.
    const TURN: [f32; 2] = [0.9659258, 0.2588190];
    let mut corners = [[0.0; 2]; 24];
    let mut i = 0;
    while i < 12 {
        let [x, z] = round(i, RING);
        corners[2 * i] = [x, z];
        corners[2 * i + 1] = [x * TURN[0] - z * TURN[1], x * TURN[1] + z * TURN[0]];
        i += 1;
    }
    corners
};

fn ground([x, z]: [f32; 2]) -> Vec3 {
    Vec3::new(x, 0.0, z)
}

impl Ramp {
    /// Its corners on the floor: the two of its low end, then the two of its high
    /// end, round in order.
    fn corners(&self) -> [Vec3; 4] {
        let (middle, up) = (ground(self.at), ground(self.climbs));
        let across = Vec3::new(-up.z, 0.0, up.x);
        // How far it reaches along the way it climbs, and across that.
        let reach = |way: Vec3| (way.x * self.half[0]).abs() + (way.z * self.half[1]).abs();
        let (along, side) = (up * reach(up), across * reach(across));
        [
            middle - along - side,
            middle - along + side,
            middle + along + side,
            middle + along - side,
        ]
    }
}

impl Plan {
    /// The corners of the floor, each with the next.
    fn sides(&self) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
        let count = self.outline.len();
        (0..count).map(move |i| (ground(self.outline[i]), ground(self.outline[(i + 1) % count])))
    }

    /// The outline of the floor and of each block, each closed, for the map.
    pub fn outlines(&self) -> Vec<Vec<Vec3>> {
        let mut floor: Vec<Vec3> = self.outline.iter().map(|&corner| ground(corner)).collect();
        floor.push(floor[0]);
        let blocks = self.blocks.iter().map(|block| {
            let [x, z] = block.at;
            let [w, d] = block.half;
            [(-w, -d), (w, -d), (w, d), (-w, d), (-w, -d)]
                .map(|(dx, dz)| Vec3::new(x + dx, 0.0, z + dz))
                .to_vec()
        });
        let on = |at: [f32; 2], half: [f32; 2]| {
            let ([x, z], [w, d]) = (at, half);
            [(-w, -d), (w, -d), (w, d), (-w, d), (-w, -d)]
                .map(|(dx, dz)| Vec3::new(x + dx, 0.0, z + dz))
                .to_vec()
        };
        let ramps = self.ramps.iter().map(|ramp| on(ramp.at, ramp.half));
        let decks = self.decks.iter().map(|deck| on(deck.at, deck.half));
        std::iter::once(floor).chain(blocks).chain(ramps).chain(decks).collect()
    }

    /// How far from the middle the floor reaches.
    pub fn reach(&self) -> f32 {
        self.outline
            .iter()
            .map(|&corner| ground(corner).length())
            .fold(0.0, f32::max)
    }

    /// Whether a place is on the floor, `clear` of its wall and of every block that
    /// stands higher than it is.
    #[cfg(test)]
    pub fn open(&self, p: Vec3, clear: f32) -> bool {
        let (height, p) = (p.y, p.with_y(0.0));
        // On the floor: in one of the triangles it is made of, each a side and the
        // middle; and no nearer any side than asked.
        let inside = self.sides().any(|(a, b)| {
            let turn = |from: Vec3, to: Vec3| (to - from).cross(p - from).y;
            let (ab, bo, oa) = (turn(a, b), turn(b, Vec3::ZERO), turn(Vec3::ZERO, a));
            (ab >= 0.0 && bo >= 0.0 && oa >= 0.0) || (ab <= 0.0 && bo <= 0.0 && oa <= 0.0)
        });
        let off_wall = self.sides().all(|(a, b)| {
            let along = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
            p.distance(a + (b - a) * along) >= clear
        });
        let off_blocks = self.blocks.iter().filter(|block| block.height > height + 1.0).all(|block| {
            let beyond = |at: f32, middle: f32, half: f32| ((at - middle).abs() - half).max(0.0);
            let (dx, dz) = (
                beyond(p.x, block.at[0], block.half[0]),
                beyond(p.z, block.at[1], block.half[1]),
            );
            dx.hypot(dz) >= clear && (dx > 0.0 || dz > 0.0 || clear < 0.0)
        });
        inside && off_wall && off_blocks
    }

    /// Where the cars start and which way each faces: at the middle.
    pub fn grid(&self) -> Vec<(Vec3, Vec3)> {
        self.grid
            .iter()
            .map(|&place| (ground(place), -ground(place).normalize_or(Vec3::X)))
            .collect()
    }

    /// The floor, the wall round it and the blocks on it, to be driven on and into.
    pub fn collide(&self, collision: &mut Collision) {
        let solid = Surface::default();
        let mut quad = |[a, b, c, d]: [Vec3; 4]| {
            collision.add([a, b, c], solid);
            collision.add([a, c, d], solid);
        };
        for (a, b) in self.sides() {
            quad([Vec3::ZERO, a, b, b]);
            let top = Vec3::Y * WALL_HEIGHT;
            quad([a - Vec3::Y, b - Vec3::Y, b + top, a + top]);
        }
        for block in self.blocks {
            let (middle, [w, d]) = (ground(block.at), block.half);
            let corners = [(-w, -d), (w, -d), (w, d), (-w, d)]
                .map(|(dx, dz)| middle + Vec3::new(dx, 0.0, dz));
            let top = Vec3::Y * block.height;
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                quad([a - Vec3::Y, b - Vec3::Y, b + top, a + top]);
            }
            // Its top, to be driven on where a ramp comes up to it.
            quad(corners.map(|corner| corner + top));
        }
        for ramp in self.ramps {
            let [low_a, low_b, high_b, high_a] = ramp.corners();
            let top = Vec3::Y * ramp.height;
            quad([low_a, low_b, high_b + top, high_a + top]);
            // Its two sides, and its end where nothing is built up to it.
            quad([low_a, high_a, high_a + top, high_a + top]);
            quad([low_b, high_b, high_b + top, high_b + top]);
            quad([high_a - Vec3::Y, high_b - Vec3::Y, high_b + top, high_a + top]);
        }
        for deck in self.decks {
            let (middle, [w, d]) = (ground(deck.at), deck.half);
            let corners = [(-w, -d), (w, -d), (w, d), (-w, d)]
                .map(|(dx, dz)| middle + Vec3::new(dx, deck.height, dz));
            quad(corners);
            // Its edges are as thick as it is, and no thicker: what is under it
            // goes under them.
            let under = Vec3::Y * DECK_THICK;
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                quad([a - under, b - under, b, a]);
            }
        }
    }

    /// What is seen of it: the floor in rings of its two colours, the wall in bricks
    /// and the blocks.
    pub fn mesh(&self) -> Mesh {
        let mut b = BrickMesh::default();
        const RINGS: usize = 6;
        for (a, c) in self.sides() {
            for ring in 0..RINGS {
                let (near, far) = (ring as f32 / RINGS as f32, (ring + 1) as f32 / RINGS as f32);
                let colour = self.floor[ring % 2];
                // Face up, whichever way round the corners are given, and just
                // over the ground the scenery stands on.
                let lift = Vec3::Y * 0.03;
                let corners = [a * near, a * far, c * far, c * near].map(|p| p + lift);
                let [p, q, r, t] = corners;
                if (q - p).cross(t - p).y + (r - q).cross(t - q).y >= 0.0 {
                    b.quad(p, q, r, t, colour);
                } else {
                    b.quad(t, r, q, p, colour);
                }
            }
            // The wall, a brick at a time, in the barriers' red and white.
            let length = a.distance(c);
            let bricks = (length / WALL_BRICK).round().max(1.0) as usize;
            let along = (c - a) / bricks as f32;
            let rot = Transform::IDENTITY.looking_to(along, Vec3::Y).rotation;
            let out = along.normalize().cross(Vec3::Y) * (WALL_THICK / 2.0);
            // The side away from the middle is the outside, whichever way round the
            // corners are given.
            let out = if out.dot(a + c) < 0.0 { -out } else { out };
            for i in 0..bricks {
                let middle = a + along * (i as f32 + 0.5) + out;
                for (course, height) in [(0, 2.0), (1, WALL_HEIGHT - 2.0)] {
                    let half = Vec3::new(WALL_THICK / 2.0, height / 2.0, along.length() / 2.0);
                    let up = if course == 0 { height / 2.0 } else { 2.0 + height / 2.0 };
                    let colour = if (i + course) % 2 == 0 { RED } else { WHITE };
                    b.brick(middle + Vec3::Y * up, half, rot, colour, (1, 4));
                }
            }
        }
        for block in self.blocks {
            let half = Vec3::new(block.half[0], block.height / 2.0, block.half[1]);
            let middle = ground(block.at) + Vec3::Y * half.y;
            b.brick(middle, half, Quat::IDENTITY, block.colour, (4, 4));
        }
        // A ramp is striped, as a jump is, so that it is seen for what it is.
        for ramp in self.ramps {
            let [low_a, low_b, high_b, high_a] = ramp.corners();
            let top = Vec3::Y * ramp.height;
            const STRIPES: usize = 6;
            for stripe in 0..STRIPES {
                let (from, to) = (
                    stripe as f32 / STRIPES as f32,
                    (stripe + 1) as f32 / STRIPES as f32,
                );
                let at = |low: Vec3, high: Vec3, part: f32| low.lerp(high + top, part) + Vec3::Y * 0.02;
                let corners = [
                    at(low_a, high_a, from),
                    at(low_b, high_b, from),
                    at(low_b, high_b, to),
                    at(low_a, high_a, to),
                ];
                let colour = [YELLOW, BLACK][stripe % 2];
                // Both faces: which is up depends on the way it climbs.
                b.quad(corners[0], corners[1], corners[2], corners[3], colour);
                b.quad(corners[3], corners[2], corners[1], corners[0], colour);
            }
            for (low, high) in [(low_a, high_a), (low_b, high_b)] {
                b.tri(low, high, high + top, DARK_GREY);
                b.tri(high + top, high, low, DARK_GREY);
            }
            b.quad(high_a, high_b, high_b + top, high_a + top, DARK_GREY);
            b.quad(high_b, high_a, high_a + top, high_b + top, DARK_GREY);
        }
        for deck in self.decks {
            let half = Vec3::new(deck.half[0], DECK_THICK / 2.0, deck.half[1]);
            let middle = ground(deck.at) + Vec3::Y * (deck.height - half.y);
            b.brick(middle, half, Quat::IDENTITY, GREY, (4, 8));
        }
        // A mark in the middle of the floor, to know it by.
        let spokes = 16;
        for i in 0..spokes {
            let (from, to) = (i as f32 / spokes as f32 * TAU, (i + 1) as f32 / spokes as f32 * TAU);
            let at = |angle: f32, radius: f32| {
                Vec3::new(angle.cos() * radius, 0.02, angle.sin() * radius)
            };
            b.quad(at(from, 12.0), at(from, 13.5), at(to, 13.5), at(to, 12.0), WHITE);
        }
        b.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{Layout, Track};

    #[test]
    fn an_arena_is_a_walled_floor_with_room_to_go_anywhere() {
        for layout in Layout::ARENAS {
            let plan = layout.plan().expect("an arena's plan");
            let track = Track::built(layout);
            let lift = Vec3::Y * 0.6;
            // The line and the grid are on open floor, well off the wall and the
            // blocks, and there is ground under every part of them.
            // The line may go up a ramp and over a deck: wherever it is, what is
            // under it is about as high as it says.
            for &p in &track.pts {
                // On its way up it is beside whatever the ramp leads onto.
                let clear = if p.y > 1.0 { 0.0 } else { 7.0 };
                assert!(plan.open(p, clear), "{layout:?} line at {p}");
                let under = track.collision.ground(p + Vec3::Y * 3.0, 8.0);
                let under = under.unwrap_or_else(|| panic!("{layout:?} nothing under {p}"));
                assert!((under.point.y - p.y).abs() < 1.6, "{layout:?} {p} over {}", under.point);
            }
            // Its high ground is there to be driven onto and under.
            for deck in plan.decks {
                let over = ground(deck.at) + Vec3::Y * (deck.height + 2.0);
                let top = track.collision.ground(over, 4.0).expect("a deck's top");
                assert!((top.point.y - deck.height).abs() < 0.1);
                let below = track.collision.ground(ground(deck.at) + Vec3::Y * 2.0, 4.0);
                assert!(below.is_some_and(|floor| floor.point.y.abs() < 0.1), "{layout:?}");
            }
            for ramp in plan.ramps {
                let [low_a, low_b, high_b, high_a] = ramp.corners();
                let (foot, head) = ((low_a + low_b) / 2.0, (high_a + high_b) / 2.0);
                let at = |part: f32| foot.lerp(head, part) + Vec3::Y * 12.0;
                let (low, high) = (
                    track.collision.ground(at(0.1), 20.0).unwrap().point.y,
                    track.collision.ground(at(0.9), 20.0).unwrap().point.y,
                );
                assert!(low < 1.0 && high > ramp.height - 1.0, "{layout:?} {low} {high}");
            }
            assert_eq!(track.course.grid.len(), 6);
            for &(place, facing) in &track.course.grid {
                assert!(plan.open(place, 6.0), "{layout:?} grid at {place}");
                let floor = track.collision.ground(place + Vec3::Y * 2.0, 4.0);
                assert!(floor.is_some_and(|floor| floor.point.y.abs() < 0.1));
                // Nothing stands right in front of a car on the grid.
                let ahead = place + facing * 12.0;
                assert!(track.collision.wall(place + lift, ahead + lift).is_none());
            }
            // The wall stops what goes at it, all the way round, and so does each
            // block; nothing counts laps.
            for (a, b) in plan.sides() {
                let middle = (a + b) / 2.0;
                let (inside, outside) = (middle * 0.95, middle * 1.05);
                assert!(
                    track.collision.wall(inside + lift, outside + lift).is_some(),
                    "{layout:?} wall at {middle}"
                );
            }
            for block in plan.blocks {
                let middle = ground(block.at);
                let beside = middle + Vec3::X * (block.half[0] + 4.0);
                assert!(track.collision.wall(beside + lift, middle + lift).is_some());
                assert!(!plan.open(middle, 0.0));
            }
            assert!(track.course.checkpoints.is_empty() && track.course.zones.is_empty());
            // Its other bricks are clear of what stands higher than they lie, and
            // where the line's rows aren't.
            for &spot in plan.bricks {
                let over = ground(spot) + Vec3::Y * 20.0;
                let spot = track.collision.ground(over, 40.0).expect("ground").point;
                assert!(plan.open(spot, 3.0), "{layout:?} brick at {spot}");
                assert!(track.pts.iter().all(|p| p.distance(spot) > 5.0), "{layout:?} {spot}");
            }
            // Nowhere on the line is far from a block or the wall: the ways are
            // narrow.
            for &p in track.pts.iter().filter(|p| p.y < 1.0) {
                assert!(!plan.open(p, 32.0), "{layout:?} is wide open at {p}");
            }
            // Mirrored it is the same place.
            for outline in plan.outlines() {
                for p in outline {
                    let flipped = Vec3::new(p.x, 0.0, -p.z);
                    assert_eq!(plan.open(p * 0.9, 0.0), plan.open(flipped * 0.9, 0.0));
                }
            }
        }
    }
}
