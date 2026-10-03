//! `.MDB` material libraries and `.TDB` texture definitions.

use super::tokens::{Reader, Token};
use std::collections::HashMap;

#[derive(Default, Clone)]
pub struct Material {
    pub diffuse: [u8; 4],
    pub texture: Option<String>,
    pub alpha_test: bool,
    pub blend: bool,
}

#[derive(Default, Clone)]
pub struct Texture {
    pub color_key: Option<[u8; 3]>,
    pub flip: bool,
    pub tga: bool,
}

/// Every library is `KEY [count] { KEY "name" { properties } ... }`.
fn entries(data: &[u8], mut property: impl FnMut(&str, u16, &mut Reader) -> Option<()>) -> Option<()> {
    let mut r = Reader::new(data);
    r.next()?;
    for _ in 0..r.list_header()? {
        r.next()?;
        let name = r.string()?.to_lowercase();
        r.expect(Token::LCurly)?;
        loop {
            match r.next()? {
                Token::RCurly => break,
                Token::Key(key) => property(&name, key, &mut r)?,
                // Arguments of properties we don't interpret.
                _ => {}
            }
        }
    }
    Some(())
}

pub fn parse_mdb(data: &[u8]) -> HashMap<String, Material> {
    let mut out: HashMap<String, Material> = HashMap::new();
    entries(data, |name, key, r| {
        let m = out.entry(name.to_string()).or_insert(Material { diffuse: [255; 4], ..Default::default() });
        match key {
            0x29 => {
                for c in &mut m.diffuse {
                    *c = r.int()? as u8;
                }
            }
            0x2c => m.texture = Some(r.string()?.to_lowercase()),
            0x2f => m.alpha_test = true,
            0x38 | 0x46 | 0x4d..=0x50 => m.blend = true,
            _ => {}
        }
        Some(())
    });
    out
}

pub fn parse_tdb(data: &[u8]) -> HashMap<String, Texture> {
    let mut out: HashMap<String, Texture> = HashMap::new();
    entries(data, |name, key, r| {
        let t = out.entry(name.to_string()).or_default();
        match key {
            0x28 => t.flip = true,
            0x29 => {
                r.int()?;
            }
            0x2b => t.tga = true,
            0x2c => t.color_key = Some([r.int()? as u8, r.int()? as u8, r.int()? as u8]),
            _ => {}
        }
        Some(())
    });
    out
}

/// How a collision surface affects a kart driving on it (`.TMB` surface tables).
#[derive(Clone, Copy)]
pub struct Surface {
    pub rolling_resistance: f32,
    pub friction: f32,
    pub lateral_grip: f32,
    /// Triggers and checkpoints: not part of the solid world.
    pub non_solid: bool,
}

impl Default for Surface {
    fn default() -> Self {
        Surface { rolling_resistance: 0.0, friction: 0.25, lateral_grip: 3.0, non_solid: false }
    }
}

pub fn parse_tmb(data: &[u8]) -> HashMap<String, Surface> {
    let mut out: HashMap<String, Surface> = HashMap::new();
    entries(data, |name, key, r| {
        let s = out.entry(name.to_string()).or_default();
        match key {
            0x33 => s.friction = r.float()?,
            0x34 => s.lateral_grip = r.float()?,
            0x36 => s.rolling_resistance = r.float()?,
            0x37 => s.non_solid = true,
            _ => {}
        }
        Some(())
    });
    out
}
