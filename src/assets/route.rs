//! Race data: `.RRB` recorded AI routes and `.PWB` power-up brick placements.

use super::tokens::{Reader, Token};

pub struct Route {
    /// Absolute positions (game coordinates, Z up) of one closed lap.
    pub lap: Vec<[f32; 3]>,
}

impl Route {
    pub fn parse(data: &[u8]) -> Option<Route> {
        let mut r = Reader::new(data);
        let mut start = [0.0f32; 3];
        let mut loop_index = 0;
        let mut deltas = Vec::new();
        while let Some(token) = r.next() {
            match token {
                Token::Key(0x27) => {
                    for _ in 0..r.list_header()? {
                        let (x, y, z) = (r.int()? as i16, r.int()? as i16, r.int()? as i8);
                        // Rotation quaternion, corridor widths, segment type and duration.
                        for _ in 0..7 {
                            r.int()?;
                        }
                        deltas.push([x as f32 / 256.0, y as f32 / 256.0, z as f32 / 16.0]);
                    }
                    r.expect(Token::RCurly)?;
                }
                Token::Key(0x29) => start = r.floats()?,
                Token::Key(0x2d) => loop_index = r.int()? as usize,
                _ => {}
            }
        }
        // Point 0 sits at the start position; each later point is an offset from the
        // one before. Playback wraps from the last point back to the loop point.
        let mut pos = start;
        let mut points = vec![pos];
        for d in deltas.iter().skip(1) {
            pos = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
            points.push(pos);
        }
        Some(Route { lap: points.get(loop_index..)?.to_vec() })
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Brick {
    Red,
    Yellow,
    Blue,
    Green,
    White,
}

/// Brick positions in game coordinates.
pub fn parse_powerups(data: &[u8]) -> Vec<(Brick, [f32; 3])> {
    let mut r = Reader::new(data);
    let mut out = Vec::new();
    let mut white = false;
    let mut current: Option<(Brick, [f32; 3])> = None;
    while let Some(token) = r.next() {
        match token {
            Token::Key(0x2f) => white = true,
            Token::LCurly => current = Some((if white { Brick::White } else { Brick::Red }, [0.0; 3])),
            Token::RCurly => out.extend(current.take()),
            Token::Key(key) => {
                let Some(brick) = &mut current else { continue };
                match key {
                    0x28 => brick.1 = r.floats().unwrap_or_default(),
                    0x2a => brick.0 = Brick::Red,
                    0x2b => brick.0 = Brick::Yellow,
                    0x2c => brick.0 = Brick::Blue,
                    0x2d => brick.0 = Brick::Green,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// One gate of the `.CPB` checkpoint graph, in game coordinates.
pub struct Checkpoint {
    /// Plane through the gate: `normal · p + distance = 0`. Racers going the right way
    /// cross it against the normal.
    pub normal: [f32; 3],
    pub distance: f32,
    pub position: [f32; 3],
    /// Gates that can follow this one; the first is the main route.
    pub next: Vec<usize>,
}

pub fn parse_checkpoints(data: &[u8]) -> Option<Vec<Checkpoint>> {
    let mut r = Reader::new(data);
    r.next()?;
    let mut out = Vec::new();
    for _ in 0..r.list_header()? {
        r.next()?;
        r.expect(Token::LCurly)?;
        let mut c = Checkpoint { normal: [0.0; 3], distance: 0.0, position: [0.0; 3], next: Vec::new() };
        loop {
            match r.next()? {
                Token::RCurly => break,
                Token::Key(0x28) => {
                    c.normal = r.floats()?;
                    c.distance = r.float()?;
                }
                Token::Key(0x29) => {
                    for _ in 0..4 {
                        let next = r.int()?;
                        if next != 255 {
                            c.next.push(next as usize);
                        }
                    }
                }
                Token::Key(0x2a) => c.position = r.floats()?,
                _ => return None,
            }
        }
        out.push(c);
    }
    Some(out)
}

/// `.SPB` starting grid: (slot number, position, forward direction).
pub fn parse_start_positions(data: &[u8]) -> Option<Vec<(usize, [f32; 3], [f32; 3])>> {
    let mut r = Reader::new(data);
    r.next()?;
    let mut out = Vec::new();
    for _ in 0..r.list_header()? {
        r.next()?;
        let slot = r.int()? as usize;
        r.expect(Token::LCurly)?;
        let (mut position, mut forward) = ([0.0; 3], [1.0, 0.0, 0.0]);
        loop {
            match r.next()? {
                Token::RCurly => break,
                Token::Key(0x28) => position = r.floats()?,
                Token::Key(0x29) => {
                    forward = r.floats()?;
                    r.floats::<3>()?; // Up.
                }
                _ => return None,
            }
        }
        out.push((slot, position, forward));
    }
    Some(out)
}

/// Where a collision `.WDB` places each of its volumes: (name, position, forward, up).
pub fn parse_placements(data: &[u8]) -> Vec<(String, [f32; 3], [f32; 3], [f32; 3])> {
    let tokens = super::tokens::tokenize(data);
    let mut out = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        // Each entry is `0x41 "name" { 0x40 index 0x31 position 0x32 forward up }`.
        let (Token::Key(0x41), Some(Token::Str(name)), Some(Token::LCurly)) =
            (token, tokens.get(i + 1), tokens.get(i + 2))
        else {
            continue;
        };
        let floats = |key: u16, count: usize| -> Option<Vec<f32>> {
            let end = i + tokens[i..].iter().position(|t| *t == Token::RCurly)?;
            let at = i + tokens[i..end].iter().position(|t| *t == Token::Key(key))?;
            tokens.get(at + 1..at + 1 + count)?.iter().map(|t| match t {
                Token::Float(v) => Some(*v),
                Token::Int(v) => Some(*v as f32),
                _ => None,
            }).collect()
        };
        let position = floats(0x31, 3).unwrap_or(vec![0.0; 3]);
        let axes = floats(0x32, 6).unwrap_or(vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        out.push((
            name.to_lowercase(),
            [position[0], position[1], position[2]],
            [axes[0], axes[1], axes[2]],
            [axes[3], axes[4], axes[5]],
        ));
    }
    out
}

/// `.TRB` trigger spheres: (centre, radius, event id).
pub fn parse_triggers(data: &[u8]) -> Vec<([f32; 3], f32, i32)> {
    let mut r = Reader::new(data);
    let mut out = Vec::new();
    let mut current: Option<([f32; 3], f32, i32)> = None;
    r.next();
    while let Some(token) = r.next() {
        match token {
            Token::LCurly => current = Some(([0.0; 3], 0.0, -1)),
            Token::RCurly => out.extend(current.take()),
            Token::Key(key) => {
                let Some(trigger) = &mut current else { continue };
                match key {
                    0x29 => trigger.0 = r.floats().unwrap_or_default(),
                    0x2a => trigger.1 = r.float().unwrap_or_default(),
                    0x2b => trigger.2 = r.int().unwrap_or(-1),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// The lap zones of an `.EVB` event table: (event id, zone). Zone 1 is the finish
/// line, 2 the stretch after it and 0 the rest of the lap; see `Kart::enter_zone`.
pub fn parse_lap_zones(data: &[u8]) -> Vec<(i32, u8)> {
    let tokens = super::tokens::tokenize(data);
    let Some(start) = tokens.iter().position(|t| *t == Token::Key(0x51)) else { return Vec::new() };
    let mut out = Vec::new();
    let mut at = start + 5; // Past `[count] {`.
    while let (Some(Token::Key(0x27)), Some(Token::Int(event))) = (tokens.get(at), tokens.get(at + 1)) {
        let end = at + tokens[at..].iter().position(|t| *t == Token::RCurly).unwrap_or(0);
        let body = &tokens[at + 2..end];
        let zone = if body.contains(&Token::Key(0x36)) {
            0
        } else if body.contains(&Token::Key(0x37)) {
            2
        } else {
            1
        };
        out.push((*event, zone));
        at = end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{Jam, gdb::Model, image::decode_bmp, materials::*};
    use super::*;

    /// Needs the original game data; silently passes without it.
    #[test]
    fn loads_imperial_grand_prix() {
        let Some(jam) = Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else { return };
        let dir = "/GAMEDATA/RACEC0R0";
        let model = Model::parse(jam.get(&format!("{dir}/RKTK.GDB")).unwrap()).unwrap();
        let tris: usize = model.batches.iter().map(|b| b.indices.len() / 3).sum();
        println!("model: {} verts, {} tris, {} materials", model.vertices.len(), tris, model.materials.len());
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &model.vertices {
            for i in 0..3 {
                lo[i] = lo[i].min(v.pos[i]);
                hi[i] = hi[i].max(v.pos[i]);
            }
        }
        println!("bounds {lo:?} {hi:?}");
        assert!(tris > 1000);

        let mdb = parse_mdb(jam.get(&format!("{dir}/COMBINED.MDB")).unwrap());
        let tdb = parse_tdb(jam.get(&format!("{dir}/COMBINED.TDB")).unwrap());
        println!("{} materials, {} textures", mdb.len(), tdb.len());
        for name in &model.materials {
            let m = mdb.get(name).unwrap_or_else(|| panic!("material {name}"));
            let tex = m.texture.clone().unwrap_or_default();
            let t = tdb.get(&tex).cloned().unwrap_or_default();
            let px = jam
                .get(&format!("{dir}/{tex}.BMP"))
                .and_then(|d| decode_bmp(d, t.color_key))
                .map(|p| (p.width, p.height));
            println!("  {name}: tex {tex} {px:?} key {:?} tga {} test {} blend {}", t.color_key, t.tga, m.alpha_test, m.blend);
        }

        let route = Route::parse(jam.get(&format!("{dir}/R1_F_0.RRB")).unwrap()).unwrap();
        let n = route.lap.len();
        let dist = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        let length: f32 = (0..n).map(|i| dist(route.lap[i], route.lap[(i + 1) % n])).sum();
        println!("route: {n} points, lap length {length}, first {:?}, last {:?}", route.lap[0], route.lap[n - 1]);
        assert!(dist(route.lap[0], route.lap[n - 1]) < 60.0);

        let bricks = parse_powerups(jam.get(&format!("{dir}/POWERUP.PWB")).unwrap());
        println!("{} bricks, e.g. {:?}", bricks.len(), &bricks[..3]);
        assert!(bricks.len() > 20);
    }
}
