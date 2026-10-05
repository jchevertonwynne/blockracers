//! `.ADB` skeletal animations: keyframed positions and rotations for each bone of a
//! skeleton, grouped into named parts (clips). Follows `CmbModelPart`.

use super::tokens::{Reader, Token};
use bevy::math::{Quat, Vec3};

/// Where one bone's keys are in the shared pools.
#[derive(Default, Clone, Copy)]
struct Track {
    rotations: usize,
    rotation_keys: usize,
    rotation_count: usize,
    positions: usize,
    position_keys: usize,
    position_count: usize,
}

pub struct Part {
    pub frames: f32,
    pub ms_per_frame: f32,
    /// The part's first track; bone `n` uses the one `n` after it.
    track: usize,
}

/// A rotation as the files give it, turned into one of ours.
pub fn turn(rotation: [f32; 4]) -> Quat {
    let rotation = Quat::from_array(rotation);
    if rotation.length_squared() > 1e-6 {
        rotation.normalize().conjugate()
    } else {
        Quat::IDENTITY
    }
}

#[derive(Default)]
pub struct Animation {
    positions: Vec<Vec3>,
    rotations: Vec<Quat>,
    /// Frame numbers of the keys.
    keys: Vec<f32>,
    tracks: Vec<Track>,
    pub parts: Vec<Part>,
}

impl Animation {
    pub fn parse(data: &[u8]) -> Option<Animation> {
        let mut r = Reader::new(data);
        let mut animation = Animation::default();
        while let Some(token) = r.next() {
            match token {
                Token::Key(0x27) => {
                    r.expect(Token::LCurly)?;
                    loop {
                        match r.next()? {
                            Token::RCurly => break,
                            Token::Key(0x28) => {
                                for _ in 0..r.list_header()? {
                                    animation.positions.push(Vec3::from(r.floats()?));
                                }
                                r.expect(Token::RCurly)?;
                            }
                            Token::Key(0x29) => {
                                for _ in 0..r.list_header()? {
                                    // The original applies its rotations the other way round from us.
                                    animation.rotations.push(turn(r.floats()?));
                                }
                                r.expect(Token::RCurly)?;
                            }
                            Token::Key(0x2a) => {
                                for _ in 0..r.list_header()? {
                                    animation.keys.push(r.float()?);
                                }
                                r.expect(Token::RCurly)?;
                            }
                            _ => {}
                        }
                    }
                }
                Token::Key(0x2b) => {
                    for _ in 0..r.list_header()? {
                        let mut v = [0usize; 6];
                        for value in &mut v {
                            *value = r.int()? as usize;
                        }
                        animation.tracks.push(Track {
                            rotations: v[0],
                            rotation_keys: v[1],
                            rotation_count: v[2],
                            positions: v[3],
                            position_keys: v[4],
                            position_count: v[5],
                        });
                    }
                    r.expect(Token::RCurly)?;
                }
                Token::Key(0x2c) => {
                    for _ in 0..r.list_header()? {
                        r.expect(Token::Key(0x2c))?;
                        r.string()?;
                        let mut part = Part {
                            frames: 1.0,
                            ms_per_frame: 33.0,
                            track: 0,
                        };
                        r.expect(Token::LCurly)?;
                        loop {
                            match r.next()? {
                                Token::RCurly => break,
                                Token::Key(0x2d) => part.frames = r.float()?,
                                Token::Key(0x2f) => part.ms_per_frame = r.float()?,
                                Token::Key(0x2b) => part.track = r.int()? as usize,
                                _ => {}
                            }
                        }
                        animation.parts.push(part);
                    }
                    r.expect(Token::RCurly)?;
                }
                _ => {}
            }
        }
        (!animation.parts.is_empty()).then_some(animation)
    }

    /// The keys either side of `frame` and how far between them it is, wrapping from
    /// the last key round to the first (`CmbModelPartTrackData::Interpolate*`).
    fn span(&self, keys: usize, count: usize, frame: f32, frames: f32) -> (usize, usize, f32) {
        let key = |i: usize| self.keys.get(keys + i).copied().unwrap_or(0.0);
        let next = (0..count).find(|&i| key(i) > frame).unwrap_or(count);
        let (first, last) = (key(0), key(count - 1));
        let (from, to, length, elapsed) = if next == 0 {
            let length = frames + first - last;
            (count - 1, 0, length, length - first + frame)
        } else if next == count {
            (count - 1, 0, frames + first - last, frame - last)
        } else {
            (
                next - 1,
                next,
                key(next) - key(next - 1),
                frame - key(next - 1),
            )
        };
        (from, to, if length == 0.0 { 0.0 } else { elapsed / length })
    }

    /// A bone's position and rotation `frame` frames into a part, where it has keys.
    pub fn sample(&self, part: usize, bone: usize, frame: f32) -> (Option<Vec3>, Option<Quat>) {
        let Some(part) = self.parts.get(part) else {
            return (None, None);
        };
        let Some(track) = self.tracks.get(part.track + bone) else {
            return (None, None);
        };
        let position = match track.position_count {
            0 => None,
            1 => self.positions.get(track.positions).copied(),
            count => {
                let (from, to, amount) = self.span(track.position_keys, count, frame, part.frames);
                let at = |i: usize| {
                    self.positions
                        .get(track.positions + i)
                        .copied()
                        .unwrap_or_default()
                };
                Some(at(from).lerp(at(to), amount))
            }
        };
        let rotation = match track.rotation_count {
            0 => None,
            1 => self.rotations.get(track.rotations).copied(),
            count => {
                let (from, to, amount) = self.span(track.rotation_keys, count, frame, part.frames);
                let at = |i: usize| {
                    self.rotations
                        .get(track.rotations + i)
                        .copied()
                        .unwrap_or_default()
                };
                let (from, to) = (at(from), at(to));
                // The short way round.
                Some(from.lerp(if from.dot(to) < 0.0 { -to } else { to }, amount))
            }
        };
        (position, rotation)
    }
}

#[cfg(test)]
#[test]
fn hammer_swings_and_comes_back() {
    let Some(jam) = super::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else {
        return;
    };
    let hammer = Animation::parse(jam.get("/GAMEDATA/RACEC0R0/RKHAMM02.ADB").unwrap()).unwrap();
    assert_eq!(hammer.parts.len(), 1);
    assert_eq!(
        (hammer.parts[0].frames, hammer.parts[0].ms_per_frame),
        (100.0, 30.0)
    );
    let angle = |frame: f32| hammer.sample(0, 0, frame).1.unwrap().to_axis_angle().1;
    assert!(angle(0.0) < 0.01 && angle(99.9) < 0.01);
    assert!(angle(50.0) > 1.4, "{}", angle(50.0));
    // The second bone has no keys of its own.
    assert!(hammer.sample(0, 1, 10.0).1.is_none());
    // Every animation in the archive parses.
    let mut count = 0;
    for dir in crate::world::circuits() {
        for file in jam
            .list(&format!("/GAMEDATA/{}", dir.0))
            .filter(|f| f.ends_with(".ADB"))
        {
            assert!(Animation::parse(jam.get(file).unwrap()).is_some(), "{file}");
            count += 1;
        }
    }
    println!("{count} animations");
}
