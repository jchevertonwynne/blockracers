//! The game's audio: 4-bit ADPCM, either bare (mono effects) or behind an `ALP ` header
//! (music in `.tun` files, and some effects).

pub struct Sound {
    /// Interleaved when there are two channels.
    pub samples: Vec<i16>,
    pub channels: u16,
    pub rate: u32,
}

const INDEX_ADJUST: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];
const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73,
    80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494,
    544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499,
    2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487,
    12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

#[derive(Default)]
struct Channel {
    sample: i32,
    index: i32,
}

impl Channel {
    fn nibble(&mut self, nibble: u8) -> i16 {
        let delta = ((nibble & 7) as i32 * STEPS[self.index as usize]) >> 2;
        self.index = (self.index + INDEX_ADJUST[(nibble & 7) as usize]).clamp(0, 88);
        self.sample = if nibble & 8 == 0 { self.sample + delta } else { self.sample - delta };
        self.sample = self.sample.clamp(-32768, 32767);
        self.sample as i16
    }

    /// Each byte holds two samples, high nibble first.
    fn byte(&mut self, byte: u8) -> [i16; 2] {
        [self.nibble(byte >> 4), self.nibble(byte & 0xf)]
    }
}

/// Decodes a sound. `default_rate` applies when the file doesn't give one: 11025 Hz
/// for effects, 22050 Hz for music.
pub fn decode(data: &[u8], default_rate: u32) -> Sound {
    let (mut channels, mut rate, mut start) = (1, default_rate, 0);
    if data.len() >= 16 && &data[..4] == b"ALP " && &data[8..12] == b"ADPC" {
        start = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize + 8;
        channels = data[15].clamp(1, 2) as u16;
        if start > 16 && data.len() >= 20 {
            rate = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        }
    }
    let data = data.get(start..).unwrap_or_default();
    let mut samples = Vec::with_capacity(data.len() * 2);
    if channels == 1 {
        let mut channel = Channel::default();
        for &byte in data {
            samples.extend(channel.byte(byte));
        }
    } else {
        // Bytes alternate between the left and right channels.
        let (mut left, mut right) = (Channel::default(), Channel::default());
        for pair in data.as_chunks::<2>().0 {
            let (l, r) = (left.byte(pair[0]), right.byte(pair[1]));
            samples.extend([l[0], r[0], l[1], r[1]]);
        }
    }
    Sound { samples, channels, rate }
}

#[cfg(test)]
#[test]
fn decodes_music_and_effects() {
    let dir = "Lego_Racers_Win_Files_EN/Game Files";
    let Some(jam) = super::Jam::open(format!("{dir}/LEGO.JAM")) else { return };
    let music = decode(&std::fs::read(format!("{dir}/theme.tun")).unwrap(), 22050);
    let seconds = music.samples.len() as f32 / 2.0 / music.rate as f32;
    println!("theme: {} ch, {} Hz, {seconds:.1} s", music.channels, music.rate);
    assert_eq!((music.channels, music.rate), (2, 22050));
    assert!(seconds > 20.0);
    // Real audio wanders about zero rather than drifting off to the rails.
    let mean = music.samples.iter().map(|&s| s as f64).sum::<f64>() / music.samples.len() as f64;
    let loud = music.samples.iter().filter(|s| s.unsigned_abs() > 30000).count();
    assert!(mean.abs() < 500.0 && loud < music.samples.len() / 50, "mean {mean}, clipped {loud}");

    for name in ["ENGINE", "GO", "CANHIT", "SKID"] {
        let s = decode(jam.get(&format!("/GAMEDATA/COMMON/{name}.PCM")).unwrap(), 11025);
        let peak = s.samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
        println!("{name}: {} ch, {} Hz, {:.2} s, peak {peak}", s.channels, s.rate, s.samples.len() as f32 / s.channels as f32 / s.rate as f32);
        assert!(peak > 1000);
    }
}
