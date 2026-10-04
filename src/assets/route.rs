//! Race data: `.RRB` recorded AI routes and `.PWB` power-up brick placements.

use super::tokens::{Reader, Token};

/// A recorded drive round a circuit, as `RaceRouteRecord`: where the car was, which
/// way it faced and how much room it had either side, at points a known time apart.
/// It runs from the grid into a lap that repeats.
pub struct Record {
    pub points: Vec<Point>,
    /// The point playback goes back to after the last, and the time it is reached at.
    pub loop_index: usize,
    pub loop_time: f32,
}

/// Positions are in game coordinates (Z up), times in milliseconds from the start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub position: [f32; 3],
    /// Quaternion as x, y, z, w.
    pub rotation: [f32; 4],
    /// Room to the left and right of the line.
    pub width: [f32; 2],
    pub kind: u8,
    pub time: f32,
}

/// Stored positions are steps from the point before, in these units.
const STEP_XY: f32 = 1.0 / 256.0;
const STEP_Z: f32 = 1.0 / 16.0;
const ROTATION: f32 = 1.0 / 127.0;
const WIDTH: f32 = 0.125;
/// The low six bits of a point's last byte are its segment's length in these.
const LENGTH_MS: f32 = 32.0;

impl Record {
    pub fn parse(data: &[u8], mirror: bool) -> Option<Record> {
        let mut r = Reader::new(data);
        let flip = if mirror { -1.0 } else { 1.0 };
        let (mut start, mut start_rotation) = ([0.0f32; 3], [0.0, 0.0, 0.0, 1.0]);
        let (mut loop_index, mut loop_time) = (0, 0.0);
        let mut steps = Vec::new();
        while let Some(token) = r.next() {
            match token {
                Token::Key(0x27) => {
                    for _ in 0..r.list_header()? {
                        let mut v = [0i32; 10];
                        for value in &mut v {
                            *value = r.int()?;
                        }
                        // Two 16-bit steps, then signed bytes.
                        let n = |i: usize| if i < 2 { v[i] as i16 as f32 } else { v[i] as i8 as f32 };
                        let width = if mirror { [n(8), n(7)] } else { [n(7), n(8)] };
                        steps.push(Point {
                            position: [n(0) * STEP_XY, n(1) * STEP_XY * flip, n(2) * STEP_Z],
                            rotation: [n(3) * ROTATION, n(4) * ROTATION * flip, n(5) * ROTATION, n(6) * ROTATION * flip],
                            width: width.map(|w| w * WIDTH),
                            kind: (v[9] as u8 >> 6).min(3),
                            time: (v[9] as u8 & 0x3f) as f32 * LENGTH_MS,
                        });
                    }
                    r.expect(Token::RCurly)?;
                }
                Token::Key(0x28) => start_rotation = r.floats()?,
                Token::Key(0x29) => start = r.floats()?,
                Token::Key(0x2c) => loop_time = r.int()? as f32,
                Token::Key(0x2d) => loop_index = r.int()? as usize,
                _ => {}
            }
        }
        if mirror {
            start[1] = -start[1];
            (start_rotation[1], start_rotation[3]) = (-start_rotation[1], -start_rotation[3]);
        }
        // The car sits at the start until time nothing; each point is then a step on
        // from the one before, and its segment's length later.
        let mut points = vec![Point { position: start, rotation: start_rotation, width: [0.0; 2], kind: 3, time: 0.0 }];
        for step in steps {
            let last = points[points.len() - 1];
            let position = [0, 1, 2].map(|i| last.position[i] + step.position[i]);
            points.push(Point { position, time: last.time + step.time, ..step });
        }
        // Stored indices don't count the start.
        let loop_index = loop_index + 1;
        (loop_index + 1 < points.len()).then_some(Record { points, loop_index, loop_time })
    }

    /// The time on the record that `time` played comes to, going round the lap again
    /// once past the end.
    pub fn wrap(&self, time: f32) -> f32 {
        let end = self.points[self.points.len() - 1].time;
        let lap = end - self.loop_time;
        if time <= end || lap <= 0.0 { time.max(0.0) } else { self.loop_time + (time - self.loop_time).rem_euclid(lap) }
    }

    /// Where the car is `time` into the record (already wrapped): the points either
    /// side of it and how far between them it has come.
    pub fn at(&self, time: f32) -> (Point, Point, f32) {
        let next = self.points.partition_point(|p| p.time <= time).clamp(1, self.points.len() - 1);
        let (from, to) = (self.points[next - 1], self.points[next]);
        let span = to.time - from.time;
        (from, to, if span > 0.0 { ((time - from.time) / span).clamp(0.0, 1.0) } else { 1.0 })
    }

    /// The positions of one lap.
    pub fn lap(&self) -> Vec<[f32; 3]> {
        self.points[self.loop_index..].iter().map(|p| p.position).collect()
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

/// A trigger sphere of a `.TRB`, as `RacerTriggerList` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Trigger {
    pub centre: [f32; 3],
    pub radius: f32,
    pub event: i32,
    /// The flag that `TriggerList::RegisterTrigger` turns into the one `RaceRoster`
    /// skips the computer's cars for.
    pub players_only: bool,
    /// The laps a racer must have finished for the trigger to notice them.
    pub lap: Option<i32>,
    /// The collision volume tested against a racer only while they are inside. Each is
    /// a finish line or a door's, which the port keeps in the world all the time.
    pub volume: Option<String>,
}

pub fn parse_triggers(data: &[u8]) -> Vec<Trigger> {
    let mut r = Reader::new(data);
    let mut out = Vec::new();
    let mut current: Option<Trigger> = None;
    r.next();
    while let Some(token) = r.next() {
        match token {
            Token::LCurly => {
                current = Some(Trigger { centre: [0.0; 3], radius: 0.0, event: -1, players_only: false, lap: None, volume: None })
            }
            Token::RCurly => out.extend(current.take()),
            Token::Key(key) => {
                let Some(trigger) = &mut current else { continue };
                match key {
                    0x29 => trigger.centre = r.floats().unwrap_or_default(),
                    0x2a => trigger.radius = r.float().unwrap_or_default(),
                    0x2b => trigger.event = r.int().unwrap_or(-1),
                    0x2d => trigger.volume = r.string().map(|name| name.to_lowercase()),
                    0x2e => trigger.lap = r.int(),
                    0x2f => trigger.players_only = true,
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

        let route = Record::parse(jam.get(&format!("{dir}/R1_F_0.RRB")).unwrap(), false).unwrap();
        let lap = route.lap();
        let n = lap.len();
        let dist = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        let length: f32 = (0..n).map(|i| dist(lap[i], lap[(i + 1) % n])).sum();
        println!("route: {n} points, lap length {length}, first {:?}, last {:?}", lap[0], lap[n - 1]);
        assert!(dist(lap[0], lap[n - 1]) < 60.0);
        // The lap closes on itself in time as well as place, and takes about a minute.
        let (first, last) = (route.points[route.loop_index], route.points[route.points.len() - 1]);
        assert!((first.time - route.loop_time).abs() < 1.0, "{} {}", first.time, route.loop_time);
        let lap_time = last.time - route.loop_time;
        assert!((30_000.0..120_000.0).contains(&lap_time), "{lap_time}");
        assert_eq!(route.wrap(last.time + 500.0), route.loop_time + 500.0);
        let (from, to, along) = route.at(route.wrap(last.time + 500.0));
        assert!(from.time <= route.loop_time + 500.0 && to.time >= route.loop_time + 500.0 && (0.0..=1.0).contains(&along));
        // Mirrored, it is the same drive on the other hand.
        let mirrored = Record::parse(jam.get(&format!("{dir}/R1_F_0.RRB")).unwrap(), true).unwrap();
        assert_eq!(mirrored.points[40].position[1], -route.points[40].position[1]);

        let bricks = parse_powerups(jam.get(&format!("{dir}/POWERUP.PWB")).unwrap());
        println!("{} bricks, e.g. {:?}", bricks.len(), &bricks[..3]);
        assert!(bricks.len() > 20);
    }
}

#[cfg(test)]
#[test]
fn the_code_pads_are_for_players_only() {
    let Some(jam) = crate::assets::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else { return };
    let triggers = parse_triggers(jam.get("/GAMEDATA/RACEC0R3/MAINTRIG.TRB").unwrap());
    // The six pads of the moon's code, which the computer's cars drive over too.
    let pads: Vec<_> = triggers.iter().filter(|t| (200..=205).contains(&t.event)).collect();
    assert_eq!(pads.len(), 6);
    assert!(pads.iter().all(|t| t.players_only));
    // The triggers that bring in the doors' collision are everyone's.
    let volumes: Vec<_> = triggers.iter().filter_map(|t| t.volume.as_deref().filter(|_| !t.players_only)).collect();
    assert_eq!(volumes, ["colrtdor", "colftdor", "starfin"]);
    // One trigger in the desert is for racers who have done a lap, and no others.
    let desert = parse_triggers(jam.get("/GAMEDATA/RACEC0R2/NEWTRIG.TRB").unwrap());
    let gated: Vec<_> = desert.iter().filter(|t| t.lap.is_some()).map(|t| (t.event, t.lap)).collect();
    assert_eq!(gated, [(10, Some(1))]);
}
