//! `.GDB` models: a vertex pool, a triangle list with 8-bit indices and a small command
//! list that loads runs of vertices into a 64-entry cache, selects materials and draws
//! runs of triangles out of the cache.

use super::tokens::{Reader, Token};

const MATERIALS: u16 = 0x27;
const VERTICES_PLAIN: u16 = 0x28;
const VERTICES_NORMAL: u16 = 0x29;
const VERTICES_COLOR: u16 = 0x2a;
const INDICES: u16 = 0x2d;
const GROUPS: u16 = 0x2e;
const PUSH_MATRIX: u16 = 0x2f;
const POP_MATRIX: u16 = 0x30;
const LOAD_VERTICES: u16 = 0x31;
const SET_MATRIX: u16 = 0x32;
const SCALE: u16 = 0x33;

#[derive(Clone, Copy)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

pub struct Batch {
    pub material: usize,
    /// Skeleton bone whose space the vertices are in, for rigged models.
    pub bone: Option<usize>,
    /// Vertex indices of a triangle list.
    pub indices: Vec<u32>,
}

pub struct Model {
    pub materials: Vec<String>,
    pub vertices: Vec<Vertex>,
    pub batches: Vec<Batch>,
    /// Multiplier from stored positions to game units.
    pub scale: f32,
}

/// One bone of a `.SDB` skeleton, placed relative to its parent.
pub struct Bone {
    pub name: String,
    pub position: [f32; 3],
    /// Quaternion as x, y, z, w.
    pub rotation: [f32; 4],
    pub parent: Option<usize>,
}

pub fn parse_skeleton(data: &[u8]) -> Option<Vec<Bone>> {
    let mut r = Reader::new(data);
    r.next()?;
    let mut bones: Vec<Bone> = Vec::new();
    for _ in 0..r.list_header()? {
        r.next()?;
        let mut bone = Bone {
            name: r.string()?.to_lowercase(),
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            parent: None,
        };
        r.expect(Token::LCurly)?;
        loop {
            match r.next()? {
                Token::RCurly => break,
                Token::Key(0x28) => bone.position = r.floats()?,
                Token::Key(0x29) => bone.rotation = r.floats()?,
                Token::Key(0x2a) => {
                    let parent = r.string()?.to_lowercase();
                    bone.parent = bones.iter().position(|b| b.name == parent);
                }
                _ => {}
            }
        }
        bones.push(bone);
    }
    Some(bones)
}

impl Model {
    pub fn parse(data: &[u8]) -> Option<Model> {
        let mut r = Reader::new(data);
        let mut model =
            Model { materials: Vec::new(), vertices: Vec::new(), batches: Vec::new(), scale: 1.0 };
        let mut triangles: Vec<[u8; 3]> = Vec::new();
        while let Some(token) = r.next() {
            let Token::Key(key) = token else { return None };
            match key {
                MATERIALS => {
                    for _ in 0..r.list_header()? {
                        model.materials.push(r.string()?.to_lowercase());
                    }
                    r.expect(Token::RCurly)?;
                }
                VERTICES_PLAIN | VERTICES_NORMAL | VERTICES_COLOR => {
                    for _ in 0..r.list_header()? {
                        let pos = r.floats()?;
                        let uv = r.floats()?;
                        let mut color = [255; 4];
                        match key {
                            VERTICES_NORMAL => {
                                r.floats::<3>()?;
                            }
                            VERTICES_COLOR => {
                                for c in &mut color {
                                    *c = r.int()? as u8;
                                }
                            }
                            _ => {}
                        }
                        model.vertices.push(Vertex { pos, uv, color });
                    }
                    r.expect(Token::RCurly)?;
                }
                INDICES => {
                    for _ in 0..r.list_header()? {
                        triangles.push([r.int()? as u8, r.int()? as u8, r.int()? as u8]);
                    }
                    r.expect(Token::RCurly)?;
                }
                GROUPS => {
                    let mut cache = [0u32; 64];
                    let mut material = 0;
                    let mut bones: Vec<usize> = Vec::new();
                    for _ in 0..r.list_header()? {
                        let Token::Key(command) = r.next()? else { return None };
                        match command {
                            LOAD_VERTICES => {
                                let slot = r.int()? as usize;
                                let first = r.int()? as u32;
                                let count = r.int()? as usize;
                                for (i, entry) in cache.get_mut(slot..slot + count)?.iter_mut().enumerate() {
                                    *entry = first + i as u32;
                                }
                            }
                            INDICES => {
                                let first = r.int()? as usize;
                                let count = r.int()? as usize;
                                let bone = bones.last().copied();
                                if model.batches.last().is_none_or(|b| b.material != material || b.bone != bone) {
                                    model.batches.push(Batch { material, bone, indices: Vec::new() });
                                }
                                let batch = &mut model.batches.last_mut()?.indices;
                                for tri in triangles.get(first..first + count)? {
                                    batch.extend(tri.iter().map(|&i| cache[i as usize & 63]));
                                }
                            }
                            MATERIALS => material = r.int()? as usize,
                            PUSH_MATRIX => bones.push(r.int()? as usize),
                            POP_MATRIX => {
                                bones.pop();
                            }
                            SET_MATRIX => {
                                bones.pop();
                                bones.push(r.int()? as usize);
                            }
                            _ => {}
                        }
                    }
                    r.expect(Token::RCurly)?;
                }
                SCALE => model.scale = r.float()?,
                _ => {}
            }
        }
        Some(model)
    }
}
