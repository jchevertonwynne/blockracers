//! `.GCB` part libraries: many small models sharing one pool of vertices, which the
//! game makes a model of one at a time. A minifigure's head and what it wears on it
//! come out of these. After `GdbPartLibrary`.

use super::gdb::{Batch, Model, Vertex};
use super::tokens::{Reader, Token};

const MATERIAL: u16 = 0x27;
const VERTICES: u16 = 0x28;
const TRIANGLES: u16 = 0x2a;
const GROUPS: u16 = 0x2b;
const PARTS: u16 = 0x2c;
const SCALE: u16 = 0x2d;

struct Part {
    name: String,
    scale: f32,
    /// A material's name and the triangles drawn with it.
    groups: Vec<(String, Vec<u16>)>,
}

pub struct Parts {
    vertices: Vec<Vertex>,
    normals: Vec<[f32; 3]>,
    parts: Vec<Part>,
}

impl Parts {
    pub fn parse(data: &[u8]) -> Option<Parts> {
        let mut r = Reader::new(data);
        let mut parts = Parts {
            vertices: Vec::new(),
            normals: Vec::new(),
            parts: Vec::new(),
        };
        while let Some(token) = r.next() {
            match token {
                Token::Key(VERTICES) => {
                    for _ in 0..r.list_header()? {
                        let pos = r.floats()?;
                        let uv = r.floats::<2>()?.map(|v| v / 4096.0);
                        parts.normals.push(r.floats::<3>()?.map(|v| v / 127.0));
                        parts.vertices.push(Vertex {
                            pos,
                            uv,
                            color: [255; 4],
                        });
                    }
                    r.expect(Token::RCurly)?;
                }
                Token::Key(PARTS) => {
                    for _ in 0..r.list_header()? {
                        r.expect(Token::Key(PARTS))?;
                        parts.parts.push(part(&mut r)?);
                    }
                }
                _ => {}
            }
        }
        Some(parts)
    }

    #[cfg(test)]
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().map(|p| p.name.as_str())
    }

    /// A model of one part, with its normals, a vertex to each corner of each triangle.
    pub fn model(&self, name: &str) -> Option<Model> {
        let part = self
            .parts
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))?;
        let mut model = Model {
            materials: Vec::new(),
            vertices: Vec::new(),
            batches: Vec::new(),
            scale: part.scale,
            normals: Vec::new(),
        };
        for (material, indices) in &part.groups {
            let mut batch = Batch {
                material: model.materials.len(),
                bone: None,
                indices: Vec::new(),
                joints: Vec::new(),
            };
            model.materials.push(material.clone());
            for &index in indices {
                batch.indices.push(model.vertices.len() as u32);
                model.vertices.push(*self.vertices.get(index as usize)?);
                model.normals.push(*self.normals.get(index as usize)?);
            }
            model.batches.push(batch);
        }
        Some(model)
    }
}

fn part(r: &mut Reader) -> Option<Part> {
    let mut part = Part {
        name: r.string()?.to_lowercase(),
        scale: 1.0,
        groups: Vec::new(),
    };
    r.expect(Token::LCurly)?;
    loop {
        match r.next()? {
            Token::RCurly => return Some(part),
            Token::Key(SCALE) => part.scale = r.float()?,
            Token::Key(GROUPS) => {
                for _ in 0..r.list_header()? {
                    r.expect(Token::Key(GROUPS))?;
                    r.expect(Token::LCurly)?;
                    let mut group = (String::new(), Vec::new());
                    loop {
                        match r.next()? {
                            Token::RCurly => break,
                            Token::Key(MATERIAL) => group.0 = r.string()?.to_lowercase(),
                            Token::Key(TRIANGLES) => {
                                for _ in 0..r.list_header()? * 3 {
                                    group.1.push(r.int()? as u16);
                                }
                                r.expect(Token::RCurly)?;
                            }
                            _ => return None,
                        }
                    }
                    part.groups.push(group);
                }
                r.expect(Token::RCurly)?;
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
#[test]
fn every_head_the_catalogue_names_is_in_the_race_s_part_library() {
    let Some(jam) = super::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else {
        return;
    };
    let parts = Parts::parse(jam.get("/MENUDATA/PARTDB/GAMEPART/ICB_CHAR.GCB").unwrap()).unwrap();
    assert_eq!(parts.names().count(), 36);
    let bare = parts.model("head").unwrap();
    assert_eq!(bare.materials, ["face"]);
    assert_eq!(bare.scale, 0.015625);
    assert!(!bare.vertices.is_empty() && bare.vertices.len() == bare.normals.len());
    for name in parts.names() {
        assert!(parts.model(name).is_some(), "{name}");
    }
}
