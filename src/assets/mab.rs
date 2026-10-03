//! `.MAB` material animations: a list of materials, each with the frame it comes in at,
//! and tracks that each play a run of those in turn. Follows `MabMaterialAnimation`.

use super::tokens::{Reader, Token};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    /// The run of entries in `frames` this track plays.
    pub first: usize,
    pub count: usize,
    /// How many frames the track lasts before it starts again.
    pub length: u32,
    pub rate: f32,
}

#[derive(Default)]
pub struct MaterialAnimation {
    /// Material, and the frame from which it shows.
    pub frames: Vec<(String, u32)>,
    pub tracks: Vec<Track>,
}

impl MaterialAnimation {
    pub fn parse(data: &[u8]) -> Option<Self> {
        let mut r = Reader::new(data);
        let mut out = MaterialAnimation::default();
        r.next()?;
        for _ in 0..r.list_header()? {
            out.frames.push((r.string()?.to_lowercase(), r.int()? as u32));
        }
        r.expect(Token::RCurly)?;
        r.next()?;
        for _ in 0..r.list_header()? {
            r.next()?;
            r.expect(Token::LCurly)?;
            let mut track = Track { first: 0, count: 1, length: 1, rate: 30.0 };
            loop {
                match r.next()? {
                    Token::RCurly => break,
                    Token::Key(0x27) => (track.first, track.count) = (r.int()? as usize, r.int()? as usize),
                    Token::Key(0x29) => track.length = r.int()? as u32,
                    Token::Key(0x2a) => track.rate = r.int()? as f32,
                    _ => {}
                }
            }
            out.tracks.push(track);
        }
        Some(out)
    }

    /// The materials of a track with the frames they show from, in order.
    pub fn materials(&self, track: usize) -> &[(String, u32)] {
        self.tracks
            .get(track)
            .and_then(|t| self.frames.get(t.first..t.first + t.count))
            .unwrap_or_default()
    }
}

impl Track {
    /// Which of the track's materials shows `seconds` in: the last one whose frame has
    /// come, going round again once the track's length is up.
    pub fn sample(&self, frames: &[u32], seconds: f32) -> usize {
        let frame = (seconds * self.rate) as u32 % self.length.max(1);
        frames.iter().rposition(|&from| frame >= from).unwrap_or(frames.len().saturating_sub(1))
    }

    /// Seconds for one run through.
    pub fn duration(&self) -> f32 {
        self.length as f32 / self.rate.max(1.0)
    }
}

#[cfg(test)]
#[test]
fn shared_emitter_animations_parse() {
    let Some(jam) = crate::assets::Jam::open("Lego_Racers_Win_Files_EN/Game Files/LEGO.JAM") else { return };
    let mab = MaterialAnimation::parse(jam.get("/GAMEDATA/COMMON/EMITTER.MAB").unwrap()).unwrap();
    assert_eq!((mab.frames.len(), mab.tracks.len()), (28, 7));
    assert_eq!(mab.tracks[2], Track { first: 8, count: 4, length: 15, rate: 30.0 });
    let sparks = mab.materials(2);
    assert_eq!(sparks.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["carspar1", "carspar2", "carspar3", "carspar4"]);
    let frames: Vec<u32> = sparks.iter().map(|f| f.1).collect();
    let at = |seconds| mab.tracks[2].sample(&frames, seconds);
    assert_eq!((at(0.0), at(0.11), at(0.25), at(0.45), at(0.5)), (0, 1, 2, 3, 0));
}
