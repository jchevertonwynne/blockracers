//! `.MDB` material libraries and `.TDB` texture definitions.

use super::tokens::{Reader, Token};
use std::collections::HashMap;

#[derive(Default, Clone)]
pub struct Material {
    pub diffuse: [u8; 4],
    pub texture: Option<String>,
    pub alpha_test: bool,
    pub blend: bool,
    /// Added to what is behind it rather than laid over it: glows, flames, beams.
    pub additive: bool,
    /// How solid it is drawn, where the material says: 255 is fully.
    pub alpha: Option<u8>,
}

#[derive(Default, Clone)]
pub struct Texture {
    pub color_key: Option<[u8; 3]>,
    pub flip: bool,
    pub tga: bool,
}

/// Every library is `KEY [count] { KEY "name" { properties } ... }`.
fn entries(
    data: &[u8],
    mut property: impl FnMut(&str, u16, &mut Reader) -> Option<()>,
) -> Option<()> {
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
        let m = out.entry(name.to_string()).or_insert(Material {
            diffuse: [255; 4],
            ..Default::default()
        });
        match key {
            0x29 => {
                for c in &mut m.diffuse {
                    *c = r.int()? as u8;
                }
            }
            0x2c => m.texture = Some(r.string()?.to_lowercase()),
            0x2f => m.alpha_test = true,
            0x46 => {
                m.blend = true;
                m.alpha = Some(r.int()? as u8);
            }
            0x38 => {
                // The source and destination factors follow; a destination of one adds.
                r.next()?;
                m.blend = true;
                m.additive = r.next()? == Token::Key(0x3a);
            }
            0x4d..=0x50 => m.blend = true,
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
    /// How steep it has to be, as the sine of its slope, before a car slides down it.
    pub support: f32,
    /// Touching it ends the race for the car that does.
    pub finish: bool,
    /// Push applied to karts standing on it (a current, say), in game coordinates.
    pub force: [f32; 3],
    /// Triggers and checkpoints: not part of the solid world.
    pub non_solid: bool,
    /// Lets shots through. On its own this marks the original's invisible barriers.
    pub shots_pass: bool,
    /// The sound of driving on it, from the circuit's sound bank.
    pub sound: Option<usize>,
    /// Events set off as a kart drives onto it, off it, or touches it.
    pub enter_event: Option<i32>,
    pub leave_event: Option<i32>,
    pub touch_event: Option<i32>,
    /// Set off when a shot hits it.
    pub shot_event: Option<i32>,
    /// The emitter of what wheels throw up from it; all zero for none.
    pub particle: [u8; 8],
}

impl Default for Surface {
    fn default() -> Self {
        Surface {
            rolling_resistance: 0.0,
            friction: 0.25,
            lateral_grip: 3.0,
            support: 0.5,
            finish: false,
            force: [0.0; 3],
            non_solid: false,
            shots_pass: false,
            sound: None,
            enter_event: None,
            leave_event: None,
            touch_event: None,
            shot_event: None,
            particle: [0; 8],
        }
    }
}

pub fn parse_tmb(data: &[u8]) -> HashMap<String, Surface> {
    let mut out: HashMap<String, Surface> = HashMap::new();
    entries(data, |name, key, r| {
        let s = out.entry(name.to_string()).or_default();
        match key {
            0x28 => s.enter_event = Some(r.int()?),
            0x29 => s.leave_event = Some(r.int()?),
            0x2a => s.touch_event = Some(r.int()?),
            0x2b => s.shot_event = Some(r.int()?),
            0x2d => s.force = r.floats()?,
            0x2e => s.sound = Some(r.int()? as usize),
            0x31 => {
                let name = r.string()?;
                let bytes = &name.as_bytes()[..name.len().min(8)];
                s.particle[..bytes.len()].copy_from_slice(bytes);
            }
            0x32 => s.support = r.float()?,
            0x33 => s.friction = r.float()?,
            0x34 => s.lateral_grip = r.float()?,
            0x36 => s.rolling_resistance = r.float()?,
            0x37 => s.non_solid = true,
            0x38 => s.shots_pass = true,
            0x39 => s.finish = true,
            _ => {}
        }
        Some(())
    });
    out
}
