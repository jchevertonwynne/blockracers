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
