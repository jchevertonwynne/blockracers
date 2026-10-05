//! `.LEB` files: the bricks cars are built of. The piece library (`LPIECEHI.LEB`, and
//! the plainer `LPIECELO.LEB` races use) holds every brick's footprint and faces; the
//! colour list (`L_COLORS.LEB`) names the colours bricks come in; and the part sets
//! (`CRSTMGR.LEB` and the `*_CSET.LEB` it lists) say which bricks go with which
//! chassis. After `LegoPieceLibrary`, `LegoColorTable` and `CarPartSet`.

use super::Jam;
use super::tokens::{Reader, Token};

const PIECES: u16 = 0x27;
const INDICES: u16 = 0x28;
const POSITIONS: u16 = 0x29;
const NORMALS: u16 = 0x2a;
const TEXTURE_COORDINATES: u16 = 0x2b;
const SHAPES: u16 = 0x2c;
const SET_PIECE: u16 = 0x2e;
const SET_CHOICES: u16 = 0x30;
const SET_NAME: u16 = 0x31;

/// Pieces numbered from here are bricks; those below are chassis.
pub const BRICK: u16 = 0x800;
/// The stud the detailed library puts on every bare cell.
pub const STUD: u16 = 0x800;
/// What a cell's heights are kept in.
const HEIGHT: u8 = 0x3f;

pub const DIR: &str = "/MENUDATA/PIECEDB";

/// One square of a piece's footprint.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Cell {
    first: u8,
    second: u8,
}

impl Cell {
    /// How high the piece stands here, in plates.
    pub fn top(self) -> i32 {
        (self.first & HEIGHT) as i32
    }

    /// How far off the floor its underside is here: nought for a bottom said to be
    /// above the top.
    pub fn bottom(self) -> i32 {
        let lower = (self.second & HEIGHT) as i32;
        if lower > self.top() { 0 } else { lower }
    }

    /// Whether the piece is here at all.
    pub fn solid(self) -> bool {
        (self.first | self.second) & HEIGHT != 0
    }

    /// Whether there is a stud on top to build on.
    pub fn studded(self) -> bool {
        self.first & 0x80 != 0
    }

    /// Whether the underside takes a stud.
    pub fn socketed(self) -> bool {
        self.second & 0x80 != 0
    }

    /// Whether the underside may rest level with a stud without taking it.
    pub fn resting(self) -> bool {
        self.second & 0xc0 != 0
    }

    pub fn overhanging(self) -> bool {
        self.second & 0x40 != 0
    }

    /// What a chassis says of its studs here: the material they are made of, kept
    /// where a brick keeps a bottom above its top.
    pub fn raised(self) -> i32 {
        ((self.second & HEIGHT) as i32 - self.top()).max(0)
    }
}

pub struct Piece {
    pub name: String,
    /// The number saved cars know the piece by.
    pub kind: u16,
    pub width: i32,
    pub depth: i32,
    cells: Vec<Cell>,
    /// For a chassis, which of the library's positions is where its car's middle is.
    pub origin: u16,
    /// How many faces it has, and where in the library's indices they begin.
    pub faces: usize,
    pub start: usize,
}

impl Piece {
    /// The cell at `x`, `y` of the piece turned `rotation` quarter turns.
    pub fn cell(&self, x: i32, y: i32, rotation: i32) -> Cell {
        // Cells are kept a column of the depth at a time.
        let (w, d) = (self.width, self.depth);
        let index = match rotation & 3 {
            0 => x * d + y,
            1 => (w - y - 1) * d + x,
            2 => (w - x - 1) * d + (d - y - 1),
            _ => y * d + (d - x - 1),
        };
        self.cells.get(index as usize).copied().unwrap_or_default()
    }

    /// Width and depth as it lies when turned.
    pub fn span(&self, rotation: i32) -> (i32, i32) {
        if rotation & 1 == 1 {
            (self.depth, self.width)
        } else {
            (self.width, self.depth)
        }
    }

    /// The tallest it stands anywhere.
    pub fn height(&self) -> i32 {
        self.cells.iter().map(|c| c.top()).max().unwrap_or(0)
    }

    pub fn is_brick(&self) -> bool {
        self.kind >= BRICK
    }
}

/// One corner of a face, as the library has it.
#[derive(Clone, Copy)]
pub struct Corner {
    /// In studs across and plates up, from the piece's own corner.
    pub position: [f32; 3],
    pub normal: [i8; 3],
    pub uv: Option<[f32; 2]>,
}

/// A face of a piece: three corners, or four.
pub struct Face {
    /// What the face is made of: below three the piece's own colour, and from three
    /// up one of the library's materials outright.
    pub material: u16,
    /// The material and the bit beside it that marks a face for the stud's picture.
    pub flags: u16,
    pub corners: Vec<Corner>,
}

pub struct Library {
    /// In order of their numbers.
    pub pieces: Vec<Piece>,
    indices: Vec<u16>,
    positions: Vec<[i16; 3]>,
    normals: Vec<[i8; 3]>,
    uvs: Vec<[i16; 2]>,
}

fn block(r: &mut Reader) -> Option<Vec<Token>> {
    r.list_header()?;
    let mut out = Vec::new();
    loop {
        match r.next()? {
            Token::RCurly => return Some(out),
            token => out.push(token),
        }
    }
}

fn int(token: &Token) -> i32 {
    match token {
        Token::Int(v) => *v,
        Token::Float(v) => *v as i32,
        _ => 0,
    }
}

impl Library {
    /// The detailed library the builder shows, or the plain one of the races.
    pub fn open(jam: &Jam, detailed: bool) -> Option<Library> {
        let name = if detailed { "LPIECEHI" } else { "LPIECELO" };
        Library::parse(jam.get(&format!("{DIR}/{name}.LEB"))?)
    }

    pub fn parse(data: &[u8]) -> Option<Library> {
        let mut r = Reader::new(data);
        let mut library = Library {
            pieces: Vec::new(),
            indices: Vec::new(),
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
        };
        let mut listed = Vec::new();
        let mut shapes = Vec::new();
        while let Some(token) = r.next() {
            let Token::Key(key) = token else { continue };
            let values = block(&mut r)?;
            match key {
                PIECES => listed = values,
                INDICES => library.indices = values.iter().map(|t| int(t) as u16).collect(),
                POSITIONS => {
                    library.positions = values
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|p| [int(&p[0]) as i16, int(&p[1]) as i16, int(&p[2]) as i16])
                        .collect()
                }
                NORMALS => {
                    library.normals = values
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|n| [int(&n[0]) as i8, int(&n[1]) as i8, int(&n[2]) as i8])
                        .collect()
                }
                TEXTURE_COORDINATES => {
                    library.uvs = values
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|t| [int(&t[0]) as i16, int(&t[1]) as i16])
                        .collect()
                }
                SHAPES => shapes = values.iter().map(|t| int(t) as u8).collect(),
                _ => {}
            }
        }
        // A shape is its width and depth, a pair of heights for each cell, and for a
        // chassis the pair after them.
        for piece in listed.as_chunks::<5>().0 {
            let Token::Str(name) = &piece[0] else {
                return None;
            };
            let at = int(&piece[2]) as usize * 2;
            let (width, depth) = (*shapes.get(at)? as i32, *shapes.get(at + 1)? as i32);
            let cells = shapes.get(at + 2..at + 2 + (width * depth) as usize * 2)?;
            let after = at + 2 + cells.len();
            let origin = shapes
                .get(after..after + 2)
                .map_or(0, |pair| u16::from_le_bytes([pair[0], pair[1]]));
            library.pieces.push(Piece {
                name: name.to_lowercase(),
                kind: int(&piece[1]) as u16,
                width,
                depth,
                cells: cells
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| Cell {
                        first: c[0],
                        second: c[1],
                    })
                    .collect(),
                origin,
                faces: int(&piece[3]) as usize,
                start: int(&piece[4]) as usize,
            });
        }
        library.pieces.sort_by_key(|p| (p.kind, p.faces));
        Some(library)
    }

    /// The piece of a number: of those that share it, the second plainest, as the
    /// game asks for it.
    pub fn piece(&self, kind: u16) -> Option<&Piece> {
        let mut same = self.pieces.iter().filter(|p| p.kind == kind);
        let first = same.next()?;
        Some(same.next().unwrap_or(first))
    }

    pub fn named(&self, name: &str) -> Option<&Piece> {
        self.pieces
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// One of the library's positions, in studs across and plates up.
    pub fn position(&self, index: u16) -> Option<[f32; 3]> {
        let p = self.positions.get(index as usize)?;
        Some(p.map(|v| v as f32 / 256.0))
    }

    /// The faces of a piece as the library draws them: a list of commands, each a
    /// triangle or a fourth corner for the triangle before it.
    pub fn faces(&self, piece: &Piece) -> Vec<Face> {
        const KIND: u16 = 0x3000;
        const TEXTURED: u16 = 0x1000;
        const FOURTH: u16 = 0x2000;
        let mut faces: Vec<Face> = Vec::new();
        let mut words = self.indices[piece.start.min(self.indices.len())..]
            .iter()
            .copied();
        let (mut textured, mut normals, mut normal) = (false, false, 0usize);
        for _ in 0..piece.faces {
            let Some(command) = words.next() else { break };
            let fourth = command & KIND == FOURTH;
            let mut shared = false;
            if !fourth {
                textured = command & KIND == TEXTURED;
                normals = command & 0x4000 != 0;
                shared = command & 0x8000 != 0;
                faces.push(Face {
                    material: command & 0x7ff,
                    flags: command & 0xfff,
                    corners: Vec::new(),
                });
            }
            let Some(face) = faces.last_mut() else { break };
            for corner in 0..if fourth { 1 } else { 3 } {
                let Some(position) = words.next().and_then(|i| self.position(i)) else {
                    return faces;
                };
                let own = if fourth {
                    normals
                } else {
                    !shared && (normals || corner == 0)
                };
                if own {
                    normal = words.next().unwrap_or(0) as usize;
                }
                let uv = textured
                    .then(|| words.next())
                    .flatten()
                    .and_then(|i| self.uvs.get(i as usize))
                    .map(|t| t.map(|v| v as f32 / 1024.0));
                face.corners.push(Corner {
                    position,
                    normal: self.normals.get(normal).copied().unwrap_or([0, 0, 127]),
                    uv,
                });
            }
        }
        faces
    }
}

/// The colours bricks come in, in the order saved cars number them.
pub fn colours(jam: &Jam) -> Vec<String> {
    let Some(data) = jam.get(&format!("{DIR}/L_COLORS.LEB")) else {
        return Vec::new();
    };
    let mut r = Reader::new(data);
    r.next();
    block(&mut r)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| match t {
            Token::Str(name) => Some(name.to_lowercase()),
            _ => None,
        })
        .collect()
}

/// The names of a material library's materials in the order they are numbered.
pub fn material_names(data: &[u8]) -> Vec<String> {
    let mut r = Reader::new(data);
    let mut names = Vec::new();
    r.next();
    let Some(count) = r.list_header() else {
        return names;
    };
    for _ in 0..count {
        r.next();
        let Some(name) = r.string() else { break };
        names.push(name.to_lowercase());
        let mut depth = 0;
        while let Some(token) = r.next() {
            depth += (token == Token::LCurly) as i32 - (token == Token::RCurly) as i32;
            if depth == 0 {
                break;
            }
        }
    }
    names
}

/// A chassis and the bricks that go with it.
pub struct PartSet {
    /// The chassis table's name for the chassis, which is its piece's name too.
    pub chassis: String,
    /// The chassis piece's number, which each brick placed from the set is marked with.
    pub kind: u16,
    /// The bricks on offer, each a piece's number and a colour's.
    pub choices: Vec<(u16, u8)>,
}

/// Every part set, in the order the game offers them.
pub fn part_sets(jam: &Jam, library: &Library) -> Vec<PartSet> {
    let colours = colours(jam);
    let Some(list) = jam.get(&format!("{DIR}/CRSTMGR.LEB")) else {
        return Vec::new();
    };
    let mut r = Reader::new(list);
    r.next();
    let files = block(&mut r).unwrap_or_default();
    files
        .iter()
        .filter_map(|file| {
            let Token::Str(file) = file else { return None };
            // Listed as the text files the game was made from.
            let file = file.to_uppercase().replace(".LEG", ".LEB");
            let mut r = Reader::new(jam.get(&format!("{DIR}/{file}"))?);
            let mut set = PartSet {
                chassis: String::new(),
                kind: 0,
                choices: Vec::new(),
            };
            while let Some(token) = r.next() {
                match token {
                    Token::Key(SET_PIECE) => set.kind = r.int()? as u16,
                    Token::Key(SET_NAME) => set.chassis = r.string()?.to_lowercase(),
                    Token::Key(SET_CHOICES) => {
                        for choice in block(&mut r)?.as_chunks::<2>().0 {
                            let (Token::Str(piece), Token::Str(colour)) = (&choice[0], &choice[1])
                            else {
                                continue;
                            };
                            let kind = library.named(piece).map_or(0, |p| p.kind);
                            let colour = colours
                                .iter()
                                .position(|c| c.eq_ignore_ascii_case(colour))
                                .unwrap_or(0);
                            set.choices.push((kind, colour as u8));
                        }
                    }
                    _ => {}
                }
            }
            Some(set)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jam() -> Option<Jam> {
        Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM")
    }

    #[test]
    fn the_library_has_its_chassis_and_its_bricks() {
        let Some(jam) = jam() else { return };
        for detailed in [true, false] {
            let library = Library::open(&jam, detailed).unwrap();
            let chassis: Vec<&Piece> = library.pieces.iter().filter(|p| !p.is_brick()).collect();
            assert_eq!(chassis.len(), 12);
            assert!(chassis.iter().all(|p| (p.width, p.depth) == (10, 6)));
            let rocket = library.named("rrchas0").unwrap();
            assert_eq!(rocket.kind, 21);
            assert!(library.position(rocket.origin).is_some());
            // Every face is a triangle or a quadrilateral with a place for each corner.
            for piece in &library.pieces {
                let faces = library.faces(piece);
                assert!(!faces.is_empty(), "{}", piece.name);
                assert!(
                    faces.iter().all(|f| matches!(f.corners.len(), 3 | 4)),
                    "{}",
                    piece.name
                );
            }
        }
        // Only the detailed library has a stud to put on things.
        assert!(
            Library::open(&jam, true)
                .unwrap()
                .piece(STUD)
                .is_some_and(|p| p.name == "cylinder")
        );
    }

    #[test]
    fn a_two_by_four_brick_is_two_by_four_and_three_plates_high() {
        let Some(jam) = jam() else { return };
        let library = Library::open(&jam, true).unwrap();
        let brick = library.named("l300100").unwrap();
        assert_eq!((brick.width, brick.depth, brick.height()), (2, 4, 3));
        assert_eq!(brick.span(1), (4, 2));
        for rotation in 0..4 {
            let (w, d) = brick.span(rotation);
            for x in 0..w {
                for y in 0..d {
                    let cell = brick.cell(x, y, rotation);
                    assert!(cell.studded() && cell.socketed() && cell.bottom() == 0);
                }
            }
        }
    }

    #[test]
    fn a_turned_piece_keeps_its_cells() {
        let Some(jam) = jam() else { return };
        let library = Library::open(&jam, true).unwrap();
        // A slope: two plates high along one edge and three along the rest.
        let slope = library.named("l329700").unwrap();
        assert_eq!((slope.width, slope.depth), (4, 3));
        let tops = |rotation: i32| {
            let (w, d) = slope.span(rotation);
            (0..w)
                .map(|x| (0..d).map(|y| slope.cell(x, y, rotation).top()).collect())
                .collect::<Vec<Vec<i32>>>()
        };
        assert_eq!(tops(0), vec![vec![2, 3, 3]; 4]);
        assert_eq!(tops(2), vec![vec![3, 3, 2]; 4]);
        assert_eq!(tops(1), vec![vec![2; 4], vec![3; 4], vec![3; 4]]);
        assert_eq!(tops(3), vec![vec![3; 4], vec![3; 4], vec![2; 4]]);
    }

    #[test]
    fn there_are_twelve_part_sets_and_ten_colours() {
        let Some(jam) = jam() else { return };
        let library = Library::open(&jam, true).unwrap();
        assert_eq!(colours(&jam).len(), 10);
        assert_eq!(colours(&jam)[5], "red");
        let sets = part_sets(&jam, &library);
        assert_eq!(sets.len(), 12);
        assert_eq!(sets[10].chassis, "rrchas0");
        assert_eq!(sets[10].kind, 21);
        assert_eq!(sets[10].choices.len(), 13);
        for set in &sets {
            assert!(library.named(&set.chassis).is_some(), "{}", set.chassis);
            assert!(set.choices.iter().all(|c| c.0 >= BRICK), "{}", set.chassis);
        }
    }
}
