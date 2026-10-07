//! Cars built of bricks, and the minifigures that drive them. A car is a list of
//! pieces on a grid ten studs by six: a chassis first, then bricks, each resting on
//! whatever is under it. The rules of what may go where, the bytes a car is saved
//! as, and the model made of it follow `CarBuildModel` and its `PieceGrid`,
//! `PieceList` and `Placement`; the minifigure follows `DriverModelBuilder`.
//!
//! Before a car is drawn the faces one brick hides behind another are cut away, as
//! `ResolvePrimitiveIntersections` does: where flat faces of two pieces lie in one
//! plane and overlap, both are cut down to the overlap and the overlap taken out.
//!
//! A built car weighs what its chassis does and a unit more for each plate of its
//! bricks, and its centre of mass is moved to where they are (`weight`,
//! `ComputeHighPieceCentroid`); `world::load_built` hands both to the car's body.

use crate::assets::{
    Jam,
    gcb::Parts,
    gdb::{Batch, Model, Vertex},
    leb::{self, Library, Piece, STUD},
    lrs::Cosmetics,
    tokens::{Reader, Token},
};
use std::collections::HashMap;

/// Where the middle of the grid is, in studs (`g_carBuildModelCenterXOffset` and `Y`).
const CENTRE_X: f32 = 4.5;
const CENTRE_Y: f32 = 2.5;
/// A stud's width in the game's units on the model (`g_carBuildModelTextureCoordinateScale`).
pub const STUD_SIZE: f32 = 0.25;
pub const WIDTH: i32 = 10;
pub const DEPTH: i32 = 6;
/// The most pieces a car may be made of, its chassis among them.
pub const MOST: usize = 64;
/// How high a car may be built, in plates.
pub const TALLEST: i32 = 15;
/// A plate's height against a stud's width.
const PLATE: f32 = 0.4;
/// How much of the picture on a stud's top goes on each stud.
const STUD_PICTURE: f32 = 0.25;
/// The colour a chassis is saved as; it has its own.
const CHASSIS_COLOUR: u8 = 3;

/// One piece of a car, where it was put.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placed {
    /// The piece's number in the library.
    pub kind: u16,
    pub x: i32,
    pub y: i32,
    /// Quarter turns.
    pub rotation: i32,
    pub colour: u8,
    /// The part set it came out of, by the number of the set's chassis.
    pub set: u16,
    /// How far up it rests, in plates.
    pub height: i32,
}

#[derive(Clone, Copy, Default)]
struct Square {
    /// Which piece is uppermost here.
    piece: Option<usize>,
    height: i32,
    /// Whether what is uppermost has a stud on it.
    studded: bool,
}

/// Why a piece can't go where it is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Refusal {
    /// Off the grid, or over nothing it can sit on.
    Nowhere,
    /// Something is in the way.
    Blocked,
    TooTall,
    TooMany,
}

/// A stud left showing: where, how high, and of which piece.
struct Stud {
    x: i32,
    y: i32,
    height: i32,
    colour: u8,
    /// The material a chassis says its stud here is made of; nought for the colour.
    material: i32,
}

#[derive(Clone, Default)]
pub struct Car {
    pub pieces: Vec<Placed>,
    grid: [[Square; DEPTH as usize]; WIDTH as usize],
}

impl Car {
    /// A chassis and nothing on it.
    pub fn new(library: &Library, chassis: &str) -> Car {
        let mut car = Car::default();
        if let Some(piece) = library.named(chassis) {
            car.place(library, piece.kind, 0, 0, 0, CHASSIS_COLOUR, 0);
        }
        car
    }

    fn square(&self, x: i32, y: i32) -> Square {
        self.grid[x as usize][y as usize]
    }

    /// How high a piece would rest at a place: `FindPlacementHeight`. A brick rests on
    /// the studs under its sockets; a chassis, which `on_nothing` is for, on the floor.
    fn rest(&self, piece: &Piece, x: i32, y: i32, rotation: i32, on_nothing: bool) -> Option<i32> {
        let (width, depth) = piece.span(rotation);
        if x < 0 || y < 0 || x + width > WIDTH || y + depth > DEPTH {
            return None;
        }
        let mut height = -1;
        for i in 0..width {
            for j in 0..depth {
                let square = self.square(x + i, y + j);
                let cell = piece.cell(i, j, rotation);
                if square.studded {
                    if cell.socketed() {
                        height = height.max(square.height - cell.bottom());
                    }
                } else if on_nothing && square.height == 0 {
                    height = height.max(-cell.bottom());
                }
            }
        }
        (height >= 0).then_some(height)
    }

    /// How many of a piece's cells would be inside what is already there:
    /// `HasCollision`.
    fn clashes(&self, piece: &Piece, x: i32, y: i32, rotation: i32, height: i32) -> usize {
        let (width, depth) = piece.span(rotation);
        let mut clashes = 0;
        for i in 0..width {
            for j in 0..depth {
                let square = self.square(x + i, y + j);
                let cell = piece.cell(i, j, rotation);
                if !cell.solid() {
                    continue;
                }
                let above = square.height - height;
                if above > cell.bottom()
                    || (above == cell.bottom() && square.studded && !cell.resting())
                {
                    clashes += 1;
                }
            }
        }
        clashes
    }

    /// Marks the grid with a piece: `StampPiece`. With `studs`, notes each stud it
    /// covers without taking, which is left showing under it.
    fn stamp(
        &mut self,
        library: &Library,
        index: usize,
        mut studs: Option<&mut Vec<Stud>>,
    ) -> Option<()> {
        let placed = self.pieces[index];
        let piece = library.piece(placed.kind)?;
        let (width, depth) = piece.span(placed.rotation);
        for i in 0..width {
            for j in 0..depth {
                let cell = piece.cell(i, j, placed.rotation);
                if !cell.solid() {
                    continue;
                }
                let (x, y) = (placed.x + i, placed.y + j);
                let under = self.square(x, y);
                if let (Some(studs), true) = (studs.as_deref_mut(), under.studded) {
                    let snug = under.height == placed.height + cell.bottom();
                    if !snug || cell.overhanging() {
                        studs.extend(self.stud(library, x, y));
                    }
                }
                self.grid[x as usize][y as usize] = Square {
                    piece: Some(index),
                    height: placed.height + cell.top(),
                    studded: cell.studded(),
                };
            }
        }
        Some(())
    }

    /// The stud showing on a square.
    fn stud(&self, library: &Library, x: i32, y: i32) -> Option<Stud> {
        let square = self.square(x, y);
        let owner = self.pieces[square.piece?];
        let piece = library.piece(owner.kind)?;
        let material = if piece.is_brick() {
            0
        } else {
            piece
                .cell(x - owner.x, y - owner.y, owner.rotation)
                .raised()
        };
        Some(Stud {
            x,
            y,
            height: square.height,
            colour: owner.colour,
            material,
        })
    }

    /// How high the car stands at its highest under a piece so wide and deep.
    pub fn over(&self, x: i32, y: i32, width: i32, depth: i32) -> i32 {
        let across = x.max(0)..(x + width).min(WIDTH);
        across
            .flat_map(|i| (y.max(0)..(y + depth).min(DEPTH)).map(move |j| (i, j)))
            .map(|(i, j)| self.square(i, j).height)
            .max()
            .unwrap_or(0)
    }

    /// Whether a piece may go at a place, and how high it would rest: `TestPlacement`.
    pub fn test(
        &self,
        library: &Library,
        kind: u16,
        x: i32,
        y: i32,
        rotation: i32,
    ) -> Result<i32, Refusal> {
        let piece = library.piece(kind).ok_or(Refusal::Nowhere)?;
        let height = self
            .rest(piece, x, y, rotation, !piece.is_brick())
            .ok_or(Refusal::Nowhere)?;
        if self.clashes(piece, x, y, rotation, height) > 0 {
            Err(Refusal::Blocked)
        } else if height + piece.height() > TALLEST {
            Err(Refusal::TooTall)
        } else if self.pieces.len() >= MOST {
            Err(Refusal::TooMany)
        } else {
            Ok(height)
        }
    }

    /// Puts a piece on the car if it may go there: `PlacePiece`.
    pub fn place(
        &mut self,
        library: &Library,
        kind: u16,
        x: i32,
        y: i32,
        rotation: i32,
        colour: u8,
        set: u16,
    ) -> bool {
        let Some(piece) = library.piece(kind) else {
            return false;
        };
        self.add(
            library,
            piece,
            x,
            y,
            rotation,
            colour,
            set,
            !piece.is_brick(),
        )
    }

    /// `PieceGrid::AddPiece`.
    fn add(
        &mut self,
        library: &Library,
        piece: &Piece,
        x: i32,
        y: i32,
        rotation: i32,
        colour: u8,
        set: u16,
        on_nothing: bool,
    ) -> bool {
        let Some(height) = self.rest(piece, x, y, rotation, on_nothing) else {
            return false;
        };
        if self.clashes(piece, x, y, rotation, height) > 0 || self.pieces.len() >= MOST {
            return false;
        }
        self.pieces.push(Placed {
            kind: piece.kind,
            x,
            y,
            rotation: rotation & 3,
            colour,
            set,
            height,
        });
        self.stamp(library, self.pieces.len() - 1, None);
        true
    }

    /// Takes the last brick off again, never the chassis: `UndoLastPiece`.
    pub fn undo(&mut self, library: &Library) -> Option<Placed> {
        if self.pieces.len() <= 1 {
            return None;
        }
        let last = self.pieces.pop()?;
        self.restamp(library, None);
        Some(last)
    }

    /// Marks the grid afresh with every piece in turn: `PieceList::RebuildGrid`.
    fn restamp(&mut self, library: &Library, mut studs: Option<&mut Vec<Stud>>) {
        self.grid = Default::default();
        for index in 0..self.pieces.len() {
            self.stamp(library, index, studs.as_deref_mut());
        }
    }

    /// A car from the bytes it was saved as: `PieceList::Deserialize`. Pieces the
    /// library hasn't got, and ones that no longer fit, are left off.
    pub fn read(library: &Library, bytes: &[u8]) -> Car {
        let mut car = Car::default();
        let count = bytes
            .get(..2)
            .map_or(0, |b| u16::from_be_bytes([b[0], b[1]]) as usize);
        if count > MOST {
            return car;
        }
        for record in bytes[2.min(bytes.len())..]
            .as_chunks::<8>()
            .0
            .iter()
            .take(count)
        {
            let kind = u16::from_be_bytes([record[0], record[1]]);
            let (x, y, rotation) = (record[2] as i32, record[3] as i32, record[4] as i32);
            let set = u16::from_be_bytes([record[6], record[7]]);
            let Some(piece) = library.piece(kind) else {
                continue;
            };
            if let Some(height) = car.rest(piece, x, y, rotation, false)
                && height + piece.height() > TALLEST
            {
                continue;
            }
            if !car.add(library, piece, x, y, rotation, record[5], set, false) {
                car.add(library, piece, x, y, rotation, record[5], set, true);
            }
        }
        car
    }

    /// The bytes a car is saved as: `PieceList::Serialize`.
    pub fn write(&self) -> Vec<u8> {
        let mut bytes = (self.pieces.len() as u16).to_be_bytes().to_vec();
        for piece in &self.pieces {
            bytes.extend(piece.kind.to_be_bytes());
            bytes.extend([
                piece.x as u8,
                piece.y as u8,
                piece.rotation as u8,
                piece.colour,
            ]);
            bytes.extend(piece.set.to_be_bytes());
        }
        bytes
    }

    /// `CarBuildModel::ComputeHighPieceCentroid`: how many plates the bricks are in
    /// all, and where their middle is in studs from the middle of the grid (nought
    /// for a car without bricks). The chassis is not counted.
    pub fn weight(&self, library: &Library) -> (i32, [f32; 3]) {
        let (mut count, mut sums) = (0, [0i64; 3]);
        for placed in &self.pieces {
            let Some(piece) = library.piece(placed.kind).filter(|piece| piece.is_brick()) else {
                continue;
            };
            let (width, depth) = piece.span(placed.rotation);
            for y in 0..depth {
                for x in 0..width {
                    let cell = piece.cell(x, y, placed.rotation);
                    let (top, bottom) = (cell.top(), cell.bottom());
                    let plates = (top - bottom) as i64;
                    count += plates as i32;
                    sums[0] += plates * (x + placed.x) as i64;
                    sums[1] += plates * (y + placed.y) as i64;
                    sums[2] += (bottom..top).map(|z| (z + placed.height) as i64).sum::<i64>();
                }
            }
        }
        if count == 0 {
            return (0, [0.0; 3]);
        }
        let n = count as f32;
        (
            count,
            [
                sums[0] as f32 / n - CENTRE_X,
                sums[1] as f32 / n - CENTRE_Y,
                sums[2] as f32 / n,
            ],
        )
    }

    /// The chassis table's name for the car's chassis.
    pub fn chassis<'a>(&self, library: &'a Library) -> Option<&'a str> {
        let piece = library.piece(self.pieces.first()?.kind)?;
        (!piece.is_brick()).then_some(piece.name.as_str())
    }

    /// Where the grid's corner is from the car's middle, which the chassis says:
    /// `UpdateOffset`.
    pub fn offset(&self, library: &Library) -> [f32; 3] {
        let origin = self
            .pieces
            .first()
            .and_then(|first| library.piece(first.kind))
            .filter(|piece| !piece.is_brick())
            .and_then(|piece| library.position(piece.origin));
        match origin {
            Some([x, y, z]) => [-x, -y, -z * PLATE],
            None => [-5.0, -3.0, 0.0],
        }
    }

    /// The car as a model, in studs from its middle: `RebuildModel`. With the detailed
    /// library every stud left showing is modelled; with the plain one the tops of
    /// bricks are given a picture of studs.
    pub fn model(&self, library: &Library, palette: &Palette) -> Model {
        self.model_cut(library, palette, true)
    }

    /// The car as a model, with the faces its bricks hide from each other cut away
    /// or, with `cut` off, left in.
    pub fn model_cut(&self, library: &Library, palette: &Palette, cut: bool) -> Model {
        let mut shape = Shape::new(palette, library.piece(STUD).is_none(), cut);
        for piece in &self.pieces {
            if let Some(shape_of) = library.piece(piece.kind) {
                shape.add(
                    library,
                    shape_of,
                    [piece.x, piece.y, piece.height],
                    piece.rotation,
                    piece.colour,
                );
            }
        }
        if let Some(stud) = library.piece(STUD) {
            let mut studs: Vec<Stud> = (0..WIDTH)
                .flat_map(|x| (0..DEPTH).map(move |y| (x, y)))
                .filter(|&(x, y)| self.square(x, y).studded)
                .filter_map(|(x, y)| self.stud(library, x, y))
                .collect();
            self.clone().restamp(library, Some(&mut studs));
            for at in studs {
                let colour = match at.material {
                    0 => at.colour,
                    material => palette
                        .colours
                        .iter()
                        .position(|&m| m == material as usize)
                        .unwrap_or(0) as u8,
                };
                shape.add(library, stud, [at.x, at.y, at.height], 0, colour);
            }
        }
        shape.finish(self.offset(library))
    }

    /// One piece on its own as a model, its middle at the model's: what the builder
    /// holds over the car. `BuildPieceModel`, after `CenterOnPiece`.
    pub fn piece_model(library: &Library, palette: &Palette, kind: u16, colour: u8) -> Model {
        let mut shape = Shape::new(palette, library.piece(STUD).is_none(), true);
        let Some(piece) = library.piece(kind) else {
            return shape.finish([0.0; 3]);
        };
        shape.add(library, piece, [0; 3], 0, colour);
        if let Some(stud) = library.piece(STUD) {
            for x in 0..piece.width {
                for y in 0..piece.depth {
                    let cell = piece.cell(x, y, 0);
                    if cell.studded() {
                        shape.add(library, stud, [x, y, cell.top()], 0, colour);
                    }
                }
            }
        }
        shape.finish([
            -piece.width as f32 / 2.0,
            -piece.depth as f32 / 2.0,
            -piece.height() as f32 * PLATE / 2.0,
        ])
    }
}

/// The materials bricks are made of: the library's, and which each colour is.
pub struct Palette {
    pub materials: Vec<String>,
    /// The material of each colour, by the colour's number.
    pub colours: Vec<usize>,
}

impl Palette {
    pub fn open(jam: &Jam, detailed: bool) -> Option<Palette> {
        let name = if detailed { "LPIECEHI" } else { "LPIECELO" };
        let materials = leb::material_names(jam.get(&format!("{}/{name}.MDB", leb::DIR))?);
        let colours = leb::colours(jam)
            .iter()
            .map(|colour| materials.iter().position(|m| m == colour).unwrap_or(0))
            .collect();
        Some(Palette { materials, colours })
    }

    /// The files the materials and their pictures are in.
    pub fn files(detailed: bool) -> [String; 2] {
        let name = if detailed { "LPIECEHI" } else { "LPIECELO" };
        ["MDB", "TDB"].map(|ext| format!("{}/{name}.{ext}", leb::DIR))
    }
}

/// A corner of a face as the car has it: where, which way it faces and where in the
/// picture.
#[derive(Clone, Copy)]
struct Corner {
    pos: [f32; 3],
    normal: [i8; 3],
    uv: [f32; 2],
}

impl Corner {
    /// A corner between two others: `from` and `to`, `to` being `along` of the way.
    /// Its place is for the caller to say.
    fn between(from: Corner, to: Corner, along: f32) -> Corner {
        let mix = |a: f32, b: f32| a * (1.0 - along) + b * along;
        Corner {
            pos: from.pos,
            normal: [0, 1, 2].map(|i| mix(from.normal[i] as f32, to.normal[i] as f32) as i8),
            uv: [0, 1].map(|i| mix(from.uv[i], to.uv[i])),
        }
    }
}

/// `BuildPrimitive`: a face of one piece, three corners or four.
#[derive(Clone)]
struct Prim {
    material: usize,
    /// An underside, which is never drawn.
    bottom: bool,
    /// Which piece of the car it belongs to.
    part: usize,
    flags: u8,
    corners: Vec<Corner>,
}

// What `BuildPrimitive::m_flags` says of a face: every corner on a whole stud and
// plate, all in one plane across X, Y or Z, and a parallelogram.
const ON_GRID: u8 = 0x01;
const SAME_X: u8 = 0x02;
const SAME_Y: u8 = 0x04;
const SAME_Z: u8 = 0x08;
const PARALLELOGRAM: u8 = 0x80;
/// Faces in a plane this close count as in the one plane (the original reuses
/// `g_minAudibleSoundVolume` for it, and its negative).
const PLANE_EPSILON: f32 = 0.005;

impl Prim {
    fn low(&self, axis: usize) -> f32 {
        self.corners
            .iter()
            .map(|c| c.pos[axis])
            .fold(f32::MAX, f32::min)
    }

    fn high(&self, axis: usize) -> f32 {
        self.corners
            .iter()
            .map(|c| c.pos[axis])
            .fold(f32::MIN, f32::max)
    }

    /// Whether the two overlap, not just touch, along `axis`.
    fn overlaps(&self, other: &Prim, axis: usize) -> bool {
        self.low(axis) < other.high(axis) && other.low(axis) < self.high(axis)
    }

    /// Turns the quad round its corners until its first two are at the low end of
    /// `axis` (or its last two at the high end), the way `ROTATE_BUILD_PRIMITIVE_*`
    /// do; false if it has no such edge.
    fn turn_to(&mut self, axis: usize, low: bool) -> bool {
        let (lo, hi) = (self.low(axis), self.high(axis));
        for _ in 0..4 {
            let c = &self.corners;
            let (at, edge) = if low { (lo, [0, 1]) } else { (hi, [2, 3]) };
            if edge.iter().all(|&i| c[i].pos[axis] == at) {
                return true;
            }
            self.corners = vec![c[1], c[3], c[0], c[2]];
        }
        false
    }
}

/// Cuts `lhs` and `rhs`, two quads in a plane, down to where they overlap, giving
/// back the pieces cut off (`CLIP_BUILD_PRIMITIVE_MIN` and `_MAX`). `first` and
/// `second` are the two axes of the plane, cut in that order. None if a quad is not
/// square to them.
fn clip(lhs: &mut Prim, rhs: &mut Prim, [first, second]: [usize; 2]) -> Option<Vec<Prim>> {
    let mut outside = Vec::new();
    for (cut, other) in [(first, second), (second, first)] {
        for low in [true, false] {
            let (l, r) = if low {
                (lhs.low(cut), rhs.low(cut))
            } else {
                (lhs.high(cut), rhs.high(cut))
            };
            if l == r {
                continue;
            }
            // The one that reaches further is the one that is cut.
            if (low && r < l) || (!low && r > l) {
                std::mem::swap(lhs, rhs);
            }
            if !lhs.turn_to(cut, low) {
                return None;
            }
            let (min, max) = (lhs.low(cut), lhs.high(cut));
            let at = if low { rhs.low(cut) } else { rhs.high(cut) };
            let along = (at - min) / (max - min);
            let c = lhs.corners.clone();
            let mut first_corner = Corner::between(c[0], c[2], along);
            first_corner.pos[cut] = at;
            let mut second_corner = Corner::between(c[1], c[3], along);
            second_corner.pos = first_corner.pos;
            second_corner.pos[other] = c[1].pos[other];
            let mut off = lhs.clone();
            if low {
                (off.corners[2], off.corners[3]) = (first_corner, second_corner);
                (lhs.corners[0], lhs.corners[1]) = (first_corner, second_corner);
            } else {
                (off.corners[0], off.corners[1]) = (first_corner, second_corner);
                (lhs.corners[2], lhs.corners[3]) = (first_corner, second_corner);
            }
            outside.push(off);
        }
    }
    Some(outside)
}

/// `ResolvePrimitiveIntersections`: where quads of different pieces lie in one plane
/// and overlap, both are cut to the overlap and the overlap is taken out.
fn resolve(prims: &mut Vec<Prim>) {
    // Each pass is for faces across one axis: the flags that say so, the axis, and
    // the plane's own two axes in the order they are cut.
    const PASSES: [(u8, usize, [usize; 2]); 3] = [
        (PARALLELOGRAM | SAME_Z | ON_GRID, 2, [1, 0]),
        (PARALLELOGRAM | SAME_Y | ON_GRID, 1, [2, 0]),
        (PARALLELOGRAM | SAME_X | ON_GRID, 0, [2, 1]),
    ];
    for (mask, axis, plane_axes) in PASSES {
        let (mut plane, rest): (Vec<Prim>, Vec<Prim>) = std::mem::take(prims)
            .into_iter()
            .partition(|p| p.flags & mask == mask && p.corners.len() == 4);
        *prims = rest;
        let mut l = 0;
        while l + 1 < plane.len() {
            let mut r = l + 1;
            while r < plane.len() {
                let (a, b) = (&plane[l], &plane[r]);
                let level = if axis == 2 {
                    let delta = a.low(2) - b.low(2);
                    -PLANE_EPSILON < delta && delta < PLANE_EPSILON
                } else {
                    a.low(axis) == b.low(axis)
                };
                let [p, q] = plane_axes;
                if !(level && a.part != b.part && a.overlaps(b, p) && a.overlaps(b, q)) {
                    r += 1;
                    continue;
                }
                let (mut a, mut b) = (a.clone(), b.clone());
                let Some(cut) = clip(&mut a, &mut b, plane_axes) else {
                    r += 1;
                    continue;
                };
                plane.swap_remove(r);
                plane.swap_remove(l);
                plane.extend(cut);
                r = l + 1;
            }
            l += 1;
        }
        prims.extend(plane);
    }
}

/// A model being put together a face at a time: `EmitPieceGeometry`.
struct Shape<'a> {
    palette: &'a Palette,
    /// Whether tops are given the picture of studs, there being no studs to model.
    pictured: bool,
    /// Whether the faces pieces hide from each other are cut away.
    cut: bool,
    prims: Vec<Prim>,
    parts: usize,
}

impl<'a> Shape<'a> {
    fn new(palette: &'a Palette, pictured: bool, cut: bool) -> Self {
        Shape {
            palette,
            pictured,
            cut,
            prims: Vec::new(),
            parts: 0,
        }
    }

    fn add(&mut self, library: &Library, piece: &Piece, at: [i32; 3], rotation: i32, colour: u8) {
        /// Undersides, which are never drawn.
        const BOTTOM: u16 = 1;
        /// The tops of studs.
        const TOP: u16 = 2;
        /// Marks a face of some other material for the picture of studs.
        const PICTURED: u16 = 0x800;
        let part = self.parts;
        self.parts += 1;
        let (width, depth) = (piece.width as f32, piece.depth as f32);
        let origin = at.map(|v| v as f32);
        let coloured = self
            .palette
            .colours
            .get(colour as usize)
            .copied()
            .unwrap_or(0);
        for face in library.faces(piece) {
            let pictured = self.pictured && (face.flags == TOP || face.flags & PICTURED != 0);
            let material = match face.material {
                3.. if self.pictured && face.flags & PICTURED != 0 => face.material as usize + 1,
                3.. => face.material as usize,
                TOP if self.pictured => coloured + 1,
                _ => coloured,
            };
            let on_grid = face
                .corners
                .iter()
                .all(|c| c.position.iter().all(|v| v.fract() == 0.0));
            let corners: Vec<Corner> = face
                .corners
                .iter()
                .map(|corner| {
                    let [sx, sy, sz] = corner.position;
                    let [nx, ny, nz] = corner.normal;
                    let flip = |n: i8| n.saturating_neg();
                    let (x, y, normal) = match rotation & 3 {
                        0 => (sx, sy, [nx, ny, nz]),
                        1 => (sy, width - sx, [ny, flip(nx), nz]),
                        2 => (width - sx, depth - sy, [flip(nx), flip(ny), nz]),
                        _ => (depth - sy, sx, [flip(ny), nx, nz]),
                    };
                    let uv = match corner.uv {
                        Some(uv) => uv,
                        None if pictured => [x * STUD_PICTURE, y * STUD_PICTURE],
                        None => [0.0; 2],
                    };
                    Corner {
                        pos: [x + origin[0], y + origin[1], sz + origin[2]],
                        normal,
                        uv,
                    }
                })
                .collect();
            let mut flags = if on_grid { ON_GRID } else { 0 };
            for (axis, same) in [(0, SAME_X), (1, SAME_Y), (2, SAME_Z)] {
                if corners.iter().all(|c| c.pos[axis] == corners[0].pos[axis]) {
                    flags |= same;
                }
            }
            if let [a, b, c, d] = &corners[..]
                && (0..3).all(|i| b.pos[i] + c.pos[i] == d.pos[i] + a.pos[i])
            {
                flags |= PARALLELOGRAM;
            }
            self.prims.push(Prim {
                material,
                bottom: face.flags == BOTTOM,
                part,
                flags,
                corners,
            });
        }
    }

    fn finish(mut self, offset: [f32; 3]) -> Model {
        if self.cut {
            resolve(&mut self.prims);
        }
        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut by_material: HashMap<usize, Vec<u32>> = HashMap::new();
        for prim in self.prims.iter().filter(|p| !p.bottom) {
            let first = vertices.len() as u32;
            for corner in &prim.corners {
                let [x, y, z] = corner.pos;
                vertices.push(Vertex {
                    pos: [x + offset[0], y + offset[1], z * PLATE + offset[2]],
                    uv: corner.uv,
                    color: [255; 4],
                });
                normals.push(corner.normal.map(|n| n as f32 / 127.0));
            }
            let triangles = by_material.entry(prim.material).or_default();
            triangles.extend([first, first + 1, first + 2]);
            if prim.corners.len() == 4 {
                triangles.extend([first + 2, first + 1, first + 3]);
            }
        }
        let mut materials: Vec<usize> = by_material.keys().copied().collect();
        materials.sort();
        Model {
            materials: self.palette.materials.clone(),
            batches: materials
                .into_iter()
                .map(|material| Batch {
                    material,
                    bone: None,
                    indices: by_material.remove(&material).unwrap_or_default(),
                    joints: Vec::new(),
                })
                .collect(),
            vertices,
            scale: 1.0,
            normals,
        }
    }
}

/// Where the builder holds the piece to be placed: `Placement`. The piece keeps the
/// corner nearest the edge of the grid where it is when it is turned or changed for
/// another, so that it is turned about that corner.
#[derive(Clone, Copy, Default)]
pub struct Cursor {
    pub kind: u16,
    pub colour: u8,
    pub set: u16,
    held: bool,
    size: (i32, i32),
    /// The corner kept, and which it is: the far one in x with 2, in y with 1.
    corner: (i32, i32),
    anchor: u8,
    pub x: i32,
    pub y: i32,
    pub rotation: i32,
}

impl Cursor {
    fn span(&self) -> (i32, i32) {
        if self.rotation & 1 == 1 {
            (self.size.1, self.size.0)
        } else {
            self.size
        }
    }

    fn settle(&mut self) {
        let (width, depth) = self.span();
        self.x = self.corner.0 - if self.anchor & 2 != 0 { width } else { 0 };
        self.y = self.corner.1 - if self.anchor & 1 != 0 { depth } else { 0 };
    }

    fn mark_corner(&mut self) {
        let (width, depth) = self.span();
        self.corner = (
            self.x + if self.anchor & 2 != 0 { width } else { 0 },
            self.y + if self.anchor & 1 != 0 { depth } else { 0 },
        );
    }

    fn clamp(&mut self) {
        let (width, depth) = self.span();
        self.x = self.x.min(WIDTH - width).max(0);
        self.y = self.y.min(DEPTH - depth).max(0);
    }

    /// Keeps the corner in whichever quarter of the grid the kept one is now in.
    fn pick_corner(&mut self) {
        let anchor = ((self.corner.0 >= WIDTH / 2) as u8 * 2) | (self.corner.1 >= DEPTH / 2) as u8;
        self.anchor = anchor;
        self.mark_corner();
    }

    /// Takes up a piece, in the middle of the grid if it is the first: `SetPiece`.
    pub fn hold(&mut self, piece: &Piece, colour: u8, set: u16) {
        let (width, depth) = (piece.width, piece.depth);
        self.size = (width, depth);
        (self.kind, self.colour, self.set) = (piece.kind, colour, set);
        if !self.held {
            (self.anchor, self.rotation) = (0, 0);
            (self.x, self.y) = ((WIDTH - width) >> 1, (DEPTH - depth) >> 1);
            if self.x < 0 || self.y < 0 {
                self.rotation = 1;
                (self.x, self.y) = ((WIDTH - depth) >> 1, (DEPTH - width) >> 1);
            }
        } else {
            // Turned on if it doesn't fit the way the last piece lay.
            let (across, along) = self.span();
            if across > WIDTH || along > DEPTH {
                self.rotation = (self.rotation + 1) & 3;
            }
            self.settle();
        }
        self.held = true;
        self.clamp();
        self.mark_corner();
        self.pick_corner();
    }

    /// A quarter turn, or two where one wouldn't fit: `Rotate`.
    pub fn turn(&mut self) {
        let (across, along) = self.span();
        if along > WIDTH || across > DEPTH {
            self.rotation += 1;
        }
        self.rotation = (self.rotation + 1) & 3;
        self.settle();
        self.clamp();
        self.mark_corner();
    }

    /// A step along the car or across it; false at the edge. `MoveX`, `MoveY`.
    pub fn step(&mut self, dx: i32, dy: i32) -> bool {
        let before = (self.x, self.y);
        self.x += dx;
        self.y += dy;
        self.clamp();
        self.mark_corner();
        self.pick_corner();
        before != (self.x, self.y)
    }

    /// Back to where a piece was, to hold it again: `SetPlacement`.
    pub fn put(&mut self, piece: &Piece, placed: &Placed) {
        (self.corner, self.rotation, self.anchor) = ((placed.x, placed.y), placed.rotation & 3, 0);
        self.held = true;
        self.hold(piece, placed.colour, placed.set);
    }
}

/// The lists of what a minifigure can be made of (`BODYPART.PCB`), after
/// `DriverPartCatalog`.
#[derive(Default)]
pub struct Catalogue {
    /// What each hat's head is called in the part library.
    pub hats: Vec<String>,
    /// What each face's materials begin with.
    pub faces: Vec<String>,
    /// Each torso's material, and each pair of legs'.
    pub torsos: Vec<String>,
    pub legs: Vec<String>,
    /// What each hat, face, torso and pair of legs is marked with, which says what
    /// has to be won before it can be worn (`progress::Progress::part_open`).
    marks: [Vec<u8>; 4],
    /// Which of the standing bodies each torso and each pair of legs goes on: a
    /// hook for a hand, a peg for a leg.
    variants: [Vec<u8>; 2],
    /// The bodies: the four that stand, by torso and legs, then the one that sits.
    bodies: Vec<String>,
}

const PARTS: &str = "/MENUDATA/PARTDB";
/// The directory of what races make a minifigure of.
const GAME_PARTS: &str = "/MENUDATA/PARTDB/GAMEPART";
/// The directory of what the menus and the films make one of.
const MENU_PARTS: &str = "/MENUDATA/PARTDB/MENUPART";

impl Catalogue {
    pub fn open(jam: &Jam) -> Option<Catalogue> {
        let mut r = Reader::new(jam.get(&format!("{PARTS}/BODYPART.PCB"))?);
        let mut catalogue = Catalogue::default();
        while let Some(token) = r.next() {
            let Token::Key(key) = token else { continue };
            r.list_header()?;
            let mut names = Vec::new();
            let mut marks: Vec<u8> = Vec::new();
            let mut variants: Vec<u8> = Vec::new();
            let mut fresh = false;
            loop {
                match r.next()? {
                    Token::RCurly => break,
                    Token::Str(name) => {
                        names.push(name.to_lowercase());
                        marks.push(0);
                        variants.push(0);
                        fresh = true;
                    }
                    // A torso and a pair of legs say which model they go on, and
                    // every part ends with what it is marked with.
                    Token::Int(mark) => {
                        if let Some(last) = marks.last_mut() {
                            *last = mark as u8;
                        }
                        if let (true, Some(last)) = (fresh, variants.last_mut()) {
                            *last = mark as u8;
                        }
                        fresh = false;
                    }
                    _ => {}
                }
            }
            // The first names of the faces, torsos and legs are the models the
            // builder shows them on.
            let (part, models) = match key {
                0x2d => (0, 0),
                0x2a => (1, 1),
                0x2b => (2, 2),
                0x2c => (3, 2),
                0x2e => {
                    catalogue.bodies = names;
                    continue;
                }
                _ => continue,
            };
            if let 2 | 3 = part {
                catalogue.variants[part - 2] = variants.split_off(models.min(variants.len()));
            }
            let names = names.split_off(models.min(names.len()));
            catalogue.marks[part] = marks.split_off(models.min(marks.len()));
            *[
                &mut catalogue.hats,
                &mut catalogue.faces,
                &mut catalogue.torsos,
                &mut catalogue.legs,
            ][part] = names;
        }
        Some(catalogue)
    }

    /// How many there are of a part to choose between.
    pub fn count(&self, part: usize) -> usize {
        [&self.hats, &self.faces, &self.torsos, &self.legs][part].len()
    }

    /// What a part is marked with.
    pub fn mark(&self, part: usize, index: usize) -> u8 {
        self.marks[part].get(index).copied().unwrap_or(0)
    }

    /// The files a figure's materials and their pictures are in, and where the
    /// pictures are.
    pub fn files() -> ([String; 2], [&'static str; 2]) {
        (
            ["MDB", "TDB"].map(|ext| format!("{PARTS}/BODYPART.{ext}")),
            [PARTS, "/GAMEDATA/COMMON"],
        )
    }
}

/// The file of the body a figure is made on, less its ending: the one that sits in
/// a car, or of those that stand the one for its torso and legs
/// (`DriverPartResources::GetBodyModel`).
fn body(catalogue: &Catalogue, cosmetics: Cosmetics, standing: bool) -> Option<String> {
    let variant = |part: usize, at: u8| {
        let variants = &catalogue.variants[part];
        variants.get(at as usize).copied().unwrap_or(0) as usize
    };
    let (folder, at) = if standing {
        let at = 2 * variant(1, cosmetics.legs) + variant(0, cosmetics.torso);
        (MENU_PARTS, at)
    } else {
        (GAME_PARTS, 4)
    };
    Some(format!("{folder}/{}", catalogue.bodies.get(at)?.to_uppercase()))
}

/// What ends the name of a face's materials, one to each of the looks it can have
/// (`DriverPartCatalog::m_faceExpressions`).
pub const LOOKS: [&str; 6] = ["dflt", "angry", "blink", "happy", "sad", "suprz"];

/// A minifigure: sitting, as races show it, or standing, as the menus and the films
/// do. Its body, and the head its hat is part of, with the faces of each given the
/// materials the figure was made with. `DriverModelBuilder::BuildDriverModel`, for
/// the part resources of the race or of the menus. The bones are those of
/// `skeleton`.
pub fn figure(
    jam: &Jam,
    catalogue: &Catalogue,
    cosmetics: Cosmetics,
    standing: bool,
) -> Option<Model> {
    let body = body(catalogue, cosmetics, standing)?;
    let heads = if standing {
        format!("{MENU_PARTS}/CBBODIES.GCB")
    } else {
        format!("{GAME_PARTS}/ICB_CHAR.GCB")
    };
    let mut model = Model::parse_lit(jam.get(&format!("{body}.GDB"))?)?;
    let parts = Parts::parse(jam.get(&heads)?)?;
    fn pick(names: &[String], at: u8) -> Option<&String> {
        names.get(at as usize).or(names.first())
    }
    let head = parts.model(pick(&catalogue.hats, cosmetics.hat)?)?;
    // `DriverModelBuilder::ApplyFaceExpression`: the face's material of the look chosen.
    let look = LOOKS.get(cosmetics.expression as usize).unwrap_or(&LOOKS[0]);
    let face = format!("{}{look}", pick(&catalogue.faces, cosmetics.face)?);
    let torso = pick(&catalogue.torsos, cosmetics.torso)?.clone();
    let legs = pick(&catalogue.legs, cosmetics.legs)?.clone();

    // The body's own head is a box the head goes in place of, on the same bone.
    let stand_in = model.materials.iter().position(|m| m == "face")?;
    let bone = model
        .batches
        .iter()
        .find(|b| b.material == stand_in)
        .and_then(|b| b.bone);
    model.batches.retain(|b| b.material != stand_in);
    let first = model.vertices.len() as u32;
    model.vertices.extend(&head.vertices);
    model.normals.extend(&head.normals);
    for batch in head.batches {
        let name = &head.materials[batch.material];
        let material = model
            .materials
            .iter()
            .position(|m| m == name)
            .unwrap_or_else(|| {
                model.materials.push(name.clone());
                model.materials.len() - 1
            });
        model.batches.push(Batch {
            material,
            bone,
            indices: batch.indices.iter().map(|i| i + first).collect(),
            joints: Vec::new(),
        });
    }
    for material in &mut model.materials {
        match material.as_str() {
            "face" => *material = face.clone(),
            "torso" => *material = torso.clone(),
            "legs" => *material = legs.clone(),
            _ => {}
        }
    }
    Some(model)
}

/// The bones of the figure `figure` makes.
pub fn skeleton<'a>(
    jam: &'a Jam,
    catalogue: &Catalogue,
    cosmetics: Cosmetics,
    standing: bool,
) -> Option<&'a [u8]> {
    jam.get(&format!("{}.SDB", body(catalogue, cosmetics, standing)?))
}

/// The start of the name of a face's materials, one to each look it can have.
pub fn face(catalogue: &Catalogue, cosmetics: Cosmetics) -> Option<&str> {
    let faces = &catalogue.faces;
    faces.get(cosmetics.face as usize).or(faces.first()).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::lrs;

    fn jam() -> Option<Jam> {
        Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM")
    }

    #[test]
    fn the_game_s_own_cars_are_read_whole_and_written_back_the_same() {
        let Some(jam) = jam() else { return };
        let library = Library::open(&jam, true).unwrap();
        let palette = Palette::open(&jam, true).unwrap();
        for file in ["QBUILD", "DEFAULT"] {
            let racers = lrs::read(jam.get(&format!("/MENUDATA/{file}.LRS")).unwrap());
            for racer in racers {
                let car = Car::read(&library, &racer.car);
                // Nothing of them is left off, and each piece rests where the rules
                // would now put it.
                assert_eq!(car.write(), racer.car, "{file} {}", racer.chassis);
                assert_eq!(car.chassis(&library), Some(racer.chassis.as_str()));
                assert!(car.pieces.len() > 4);
                let model = car.model(&library, &palette);
                assert!(model.vertices.len() > 500);
                assert_eq!(model.vertices.len(), model.normals.len());
                // Within the grid, about the car's middle.
                // About the car's middle, and no bigger than the grid lets it be.
                for vertex in &model.vertices {
                    assert!(vertex.pos[0].abs() < 9.0 && vertex.pos[1].abs() < 5.0);
                    assert!((0.0..9.0).contains(&vertex.pos[2]));
                }
            }
        }
    }

    /// Cutting away what bricks hide from each other leaves fewer triangles, and
    /// never any that were not there.
    #[test]
    fn the_faces_bricks_hide_from_each_other_are_cut_away() {
        let Some(jam) = jam() else { return };
        let library = Library::open(&jam, true).unwrap();
        let palette = Palette::open(&jam, true).unwrap();
        let count = |model: &Model| model.batches.iter().map(|b| b.indices.len() / 3).sum::<usize>();
        let (mut whole, mut cut) = (0, 0);
        for racer in lrs::read(jam.get("/MENUDATA/QBUILD.LRS").unwrap()) {
            let car = Car::read(&library, &racer.car);
            let (all, less) = (
                count(&car.model_cut(&library, &palette, false)),
                count(&car.model_cut(&library, &palette, true)),
            );
            assert!(less < all, "{}: {less} of {all}", racer.chassis);
            whole += all;
            cut += less;
        }
        println!("triangles of the quick-build cars: {whole} whole, {cut} cut");
    }

    #[test]
    fn bricks_rest_on_studs_and_not_in_one_another() {
        let Some(jam) = jam() else { return };
        let library = Library::open(&jam, true).unwrap();
        let brick = library.named("l300100").unwrap().kind;
        let mut car = Car::new(&library, "rrchas0");
        assert_eq!(car.pieces.len(), 1);
        // The chassis has studs somewhere for a two by four to stand on.
        let spot = (0..WIDTH)
            .flat_map(|x| (0..DEPTH).map(move |y| (x, y)))
            .find_map(|(x, y)| car.test(&library, brick, x, y, 0).ok().map(|h| (x, y, h)));
        let (x, y, height) = spot.unwrap();
        assert!(car.place(&library, brick, x, y, 0, 5, 21));
        // A second goes on top of the first, three plates up, and comes off again.
        assert_eq!(car.test(&library, brick, x, y, 0), Ok(height + 3));
        assert!(car.place(&library, brick, x, y, 0, 7, 21));
        assert_eq!(car.undo(&library).map(|p| p.height), Some(height + 3));
        assert_eq!(car.test(&library, brick, x, y, 0), Ok(height + 3));
        // Off the grid there is nowhere, and the chassis is never taken off.
        assert_eq!(car.test(&library, brick, 9, 5, 0), Err(Refusal::Nowhere));
        car.undo(&library);
        assert!(car.undo(&library).is_none());
        assert_eq!(car.pieces.len(), 1);
        // Stacked until it is too tall.
        while car.place(&library, brick, x, y, 0, 5, 21) {
            if car.test(&library, brick, x, y, 0) == Err(Refusal::TooTall) {
                break;
            }
        }
        assert_eq!(car.test(&library, brick, x, y, 0), Err(Refusal::TooTall));
        assert_eq!(Car::read(&library, &car.write()).pieces, car.pieces);
    }

    #[test]
    fn the_cursor_stays_on_the_grid() {
        let Some(jam) = jam() else { return };
        let library = Library::open(&jam, true).unwrap();
        let brick = library.named("l300100").unwrap();
        let mut cursor = Cursor::default();
        cursor.hold(brick, 5, 21);
        assert_eq!((cursor.x, cursor.y, cursor.rotation), (4, 1, 0));
        for _ in 0..12 {
            cursor.step(1, 1);
        }
        assert_eq!((cursor.x, cursor.y), (8, 2));
        assert!(!cursor.step(1, 0));
        // Turned about the corner it keeps, it is still against the end of the grid.
        cursor.turn();
        assert_eq!((cursor.x, cursor.y, cursor.rotation), (6, 2, 1));
        // A chassis only lies one way.
        let chassis = library.named("rrchas0").unwrap();
        let mut cursor = Cursor::default();
        cursor.hold(chassis, 3, 0);
        cursor.turn();
        assert_eq!((cursor.x, cursor.y, cursor.rotation & 1), (0, 0, 0));
    }

    #[test]
    fn a_figure_is_made_of_the_parts_it_was_given() {
        let Some(jam) = jam() else { return };
        let catalogue = Catalogue::open(&jam).unwrap();
        assert_eq!(
            [0, 1, 2, 3].map(|part| catalogue.count(part)),
            [36, 30, 29, 20]
        );
        let cosmetics = Cosmetics {
            hat: 1,
            face: 28,
            torso: 15,
            legs: 18,
            expression: 0,
        };
        // Sitting in a car and standing in the menus, it is made of the same parts.
        for standing in [false, true] {
            let model = figure(&jam, &catalogue, cosmetics, standing).unwrap();
            for material in ["rr_dflt", "rr_chst", "rr_leg", "helmetrr"] {
                assert!(model.materials.iter().any(|m| m == material), "{material}");
            }
            assert_eq!(model.vertices.len(), model.normals.len());
            assert!(model.batches.iter().all(|b| b.bone.is_some()));
            // Its bones are enough for every part of it.
            let bones = crate::assets::gdb::parse_skeleton(skeleton(&jam, &catalogue, cosmetics, standing).unwrap());
            let bones = bones.unwrap().len();
            assert!(model.batches.iter().all(|b| b.bone.unwrap() < bones));
            // Every figure there could be has its parts.
            for hat in 0..catalogue.count(0) as u8 {
                let cosmetics = Cosmetics { hat, ..cosmetics };
                let made = figure(&jam, &catalogue, cosmetics, standing);
                assert!(made.is_some(), "hat {hat}");
            }
        }
        // A hook for a hand and a peg for a leg are bodies of their own.
        let hooked = Cosmetics { torso: 24, legs: 10, ..cosmetics };
        assert!(body(&catalogue, hooked, true).unwrap().ends_with("/HP"));
        assert!(body(&catalogue, cosmetics, true).unwrap().ends_with("/RR"));
        assert!(body(&catalogue, hooked, false).unwrap().ends_with("/LEG_BOX"));
    }
}

#[cfg(test)]
#[test]
fn the_catalogue_says_what_each_part_is_won_by() {
    let Some(jam) = crate::world::jam() else {
        return;
    };
    let catalogue = Catalogue::open(&jam).unwrap();
    // No hat has to be won; the last face, torso and legs go with every record beaten.
    assert!((0..catalogue.count(0)).all(|hat| catalogue.mark(0, hat) == 0));
    for part in 1..4 {
        assert_eq!(catalogue.marks[part].len(), catalogue.count(part));
        assert_eq!(catalogue.mark(part, catalogue.count(part) - 1), 0x80);
    }
    assert_eq!((catalogue.mark(1, 1), catalogue.mark(2, 8)), (4, 3));
    // Each circuit's winner is given the next of the part sets.
    for (set, circuit) in ["c0", "c1", "c2", "c3", "c4", "c5", "c6"].iter().enumerate() {
        assert_eq!(crate::roster::part_set(&jam, circuit), Some(set));
    }
}
