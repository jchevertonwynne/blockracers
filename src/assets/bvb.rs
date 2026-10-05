//! `.BVB` collision volumes: a vertex pool, triangles tagged with a surface material and
//! a BSP tree over them (which we skip in favour of our own spatial grid).

use super::tokens::{Reader, Token};

pub struct Volume {
    pub materials: Vec<String>,
    pub vertices: Vec<[f32; 3]>,
    /// Three vertex indices and a material index.
    pub triangles: Vec<[u16; 4]>,
}

impl Volume {
    pub fn parse(data: &[u8]) -> Option<Volume> {
        let mut r = Reader::new(data);
        let mut volume = Volume {
            materials: Vec::new(),
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        while let Some(token) = r.next() {
            match token {
                Token::Key(0x27) => {
                    for _ in 0..r.list_header()? {
                        volume.materials.push(r.string()?.to_lowercase());
                    }
                    r.expect(Token::RCurly)?;
                }
                Token::Key(0x34) => {
                    for _ in 0..r.list_header()? {
                        volume.vertices.push(r.floats()?);
                    }
                    r.expect(Token::RCurly)?;
                }
                Token::Key(0x2d) => {
                    for _ in 0..r.list_header()? {
                        let mut tri = [0u16; 4];
                        for v in &mut tri {
                            *v = r.int()? as u16;
                        }
                        volume.triangles.push(tri);
                    }
                    r.expect(Token::RCurly)?;
                }
                // BSP nodes and anything else.
                _ => {}
            }
        }
        (!volume.triangles.is_empty()).then_some(volume)
    }
}

#[cfg(test)]
#[test]
fn loads_collision_volume() {
    let Some(jam) = super::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else {
        return;
    };
    let v = Volume::parse(jam.get("/GAMEDATA/RACEC0R0/COLLIDE.BVB").unwrap()).unwrap();
    println!(
        "{} verts {} tris, materials {:?}",
        v.vertices.len(),
        v.triangles.len(),
        v.materials
    );
    let mut per = vec![0; v.materials.len().max(1)];
    for t in &v.triangles {
        per[t[3] as usize] += 1;
    }
    println!("triangles per material {per:?}");
    assert!(
        v.triangles
            .iter()
            .all(|t| t[..3].iter().all(|&i| (i as usize) < v.vertices.len()))
    );
}
